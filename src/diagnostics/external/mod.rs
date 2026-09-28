//! External-tool proxy pipelines: PHPStan, PHPCS, and Mago (lint + analyze).
//!
//! Every pipeline shares one worker loop, [`Backend::external_tool_worker`]:
//! wait for a notification, drain extra permits, snapshot the pending URI
//! and file content, resolve the tool's binary, run it on the blocking
//! pool, cache the results, and re-publish diagnostics for the file. At
//! most one process per tool runs at a time. Each submodule supplies the
//! one thing that differs — how the tool's binary is resolved and which
//! command it runs — plus its schedule function.
//!
//! Native diagnostic scheduling (`schedule_diagnostics`,
//! `schedule_external_diagnostics`, `schedule_diagnostics_for_open_files`)
//! stays in `diagnostics::mod` — it orchestrates these pipelines but is
//! not itself an external-tool pipeline.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::config::Config;

pub(crate) mod mago;
pub(crate) mod phpcs;
pub(crate) mod phpstan;

/// A tool run whose binary and configuration are already resolved.
///
/// The arguments are the editor buffer, the file's real path on disk,
/// the workspace root, and the shutdown flag the run polls to abort.
pub(crate) type ExternalToolRun = Box<
    dyn FnOnce(&str, &Path, &Path, &AtomicBool) -> Result<Vec<Diagnostic>, String> + Send + 'static,
>;

impl Backend {
    /// Schedule an external-tool run for a single file.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// saves rapidly, earlier requests are superseded.
    fn schedule_external_tool(tool: &crate::ExternalToolWorker, uri: String) {
        *tool.pending_uri.lock() = Some(uri);
        tool.notify.notify_one();
    }

    /// Long-lived background task that runs one external tool on pending
    /// files.
    ///
    /// Spawned once per tool during `initialized`. Each task is completely
    /// independent: native diagnostics and the other tools are never
    /// blocked.
    ///
    /// ## Serialization guarantee
    ///
    /// At most one process per tool runs at a time. The worker loop:
    ///
    /// 1. Wait for a notification (file saved).
    /// 2. Snapshot the pending URI and file content.
    /// 3. Call `prepare` to resolve the binary (skip if not found /
    ///    disabled / the project doesn't use this tool).
    /// 4. Run the tool (blocking — this is the slow part).
    /// 5. Cache the results and re-publish diagnostics for the file.
    /// 6. Loop back to step 1.
    ///
    /// If the user saves again while step 4 is in progress, the pending
    /// URI is updated. When step 4 finishes, the worker sees the new
    /// notification and loops back to step 1, starting a fresh run with
    /// the latest content.
    ///
    /// `label` names the tool in timeout and panic messages.
    async fn external_tool_worker(
        &self,
        tool: &crate::ExternalToolWorker,
        label: &'static str,
        prepare: impl Fn(&Config, &Path) -> Option<ExternalToolRun>,
    ) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 1: wait for work ───────────────────────────────
            tool.notify.notified().await;

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // Drain any extra stored permits so that notifications that
            // arrived between the last run finishing and this
            // `notified()` call don't cause an immediate second run.
            let _ = tokio::time::timeout(std::time::Duration::ZERO, tool.notify.notified()).await;

            // ── Step 2: snapshot the pending URI ────────────────────
            let uri = match tool.pending_uri.lock().take() {
                Some(u) => u,
                None => continue,
            };

            let content = {
                let files = self.open_files.read();
                match files.get(&uri) {
                    Some(c) => c.clone(),
                    None => continue,
                }
            };

            // ── Step 3: resolve the tool's binary ───────────────────
            let file_path = match uri.parse::<Url>().ok().and_then(|u| u.to_file_path().ok()) {
                Some(p) => p,
                None => continue,
            };

            let workspace_root = self.workspace.workspace_root.read().clone();
            let workspace_root = match workspace_root {
                Some(root) => root,
                None => continue,
            };

            let run = match prepare(&self.config(), &workspace_root) {
                Some(run) => run,
                None => continue,
            };

            // ── Step 4: run the tool (the slow part) ────────────────
            // Move the blocking execution onto a dedicated OS thread.
            // This is critical: the runners contain a poll loop that
            // blocks the thread. If we ran one inline, the tokio runtime
            // could schedule other futures (including a second iteration
            // of this very worker) on other threads, breaking the "at
            // most one process" guarantee. By awaiting the offloaded
            // call, this task is suspended (not occupying a runtime
            // thread) and no re-entry can happen until it resolves.
            let shutdown_flag = Arc::clone(&self.shutdown_flag);
            let diags = {
                let result = crate::server::run_blocking_cancel_safe(label, move || {
                    run(&content, &file_path, &workspace_root, &shutdown_flag)
                })
                .await;

                match result {
                    Some(Ok(diags)) => diags,
                    // Tool failures are silently ignored to avoid
                    // flooding the editor with errors when the tool is
                    // misconfigured or the project doesn't use it.
                    // (A panic is logged by the helper itself.)
                    _ => continue,
                }
            };

            // ── Step 5: cache results and re-publish ────────────────
            // Verify the file is still open *before* writing to the
            // cache. If the file was closed while the tool was running,
            // `clear_diagnostics_for_file` already purged the cache
            // entry — writing it back would leave stale diagnostics that
            // resurface on the next `did_open`.
            {
                let files = self.open_files.read();
                if !files.contains_key(&uri) {
                    continue;
                }
            }

            tool.store_file_result(&uri, diags);

            // Assemble and push so the editor sees fresh results merged
            // with cached native diagnostics. In pull mode this also
            // tells the editor to re-pull, but only when the run actually
            // changed the file's diagnostics.
            self.assemble_and_refresh(&uri).await;
        }
    }
}
