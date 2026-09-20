//! PHPStan proxy diagnostics: schedule function and background worker.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::phpstan;

impl Backend {
    // ── PHPStan worker ──────────────────────────────────────────────

    /// Schedule a PHPStan run for a single file.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// saves rapidly, earlier requests are superseded.
    pub(crate) fn schedule_phpstan(&self, uri: String) {
        *self.phpstan_tool.pending_uri.lock() = Some(uri);
        self.phpstan_tool.notify.notify_one();
    }

    /// Long-lived background task that runs PHPStan on pending files.
    ///
    /// Spawned once during `initialized`, alongside the main diagnostic
    /// worker. This task is completely independent: native diagnostics
    /// (phases 1 and 2) are never blocked by PHPStan.
    ///
    /// ## Serialization guarantee
    ///
    /// At most one PHPStan process runs at a time. The worker loop:
    ///
    /// 1. Wait for a notification (file saved).
    /// 2. Snapshot the pending URI and file content.
    /// 3. Resolve the PHPStan binary (skip if not found / disabled).
    /// 4. Run PHPStan (blocking — this is the slow part).
    /// 5. Cache the results and re-publish diagnostics for the file.
    /// 6. Loop back to step 1.
    ///
    /// If the user saves again while step 4 is in progress, the pending
    /// URI is updated. When step 4 finishes, the worker sees the new
    /// notification and loops back to step 1, starting a fresh run
    /// with the latest content.
    pub(crate) async fn phpstan_worker(&self) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 1: wait for work ───────────────────────────────
            self.phpstan_tool.notify.notified().await;

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // Drain any extra stored permits so that notifications
            // that arrived between the last run finishing and this
            // `notified()` call don't cause an immediate second run.
            let _ = tokio::time::timeout(
                std::time::Duration::ZERO,
                self.phpstan_tool.notify.notified(),
            )
            .await;

            // ── Step 2: snapshot the pending URI ────────────────────
            let uri = match self.phpstan_tool.pending_uri.lock().take() {
                Some(u) => u,
                None => continue,
            };

            // Snapshot the file content.
            let content = {
                let files = self.open_files.read();
                match files.get(&uri) {
                    Some(c) => c.clone(),
                    None => continue,
                }
            };

            // ── Step 3: resolve PHPStan binary ──────────────────────
            let config = self.config();
            if config.phpstan.is_disabled() {
                continue;
            }

            let file_path = match uri.parse::<Url>().ok().and_then(|u| u.to_file_path().ok()) {
                Some(p) => p,
                None => continue,
            };

            let workspace_root = self.workspace.workspace_root.read().clone();
            let workspace_root = match workspace_root {
                Some(root) => root,
                None => continue,
            };

            let composer_pkg = crate::composer::read_composer_package(&workspace_root);
            let bin_dir: Option<String> = composer_pkg.as_ref().map(crate::composer::get_bin_dir);

            let resolved = match phpstan::resolve_phpstan(
                Some(&workspace_root),
                &config.phpstan,
                bin_dir.as_deref(),
                composer_pkg.as_ref(),
            ) {
                Some(r) => r,
                None => continue,
            };

            // ── Step 4: run PHPStan (the slow part) ─────────────────
            // Move the blocking PHPStan execution onto a dedicated
            // OS thread. This is critical: `run_phpstan` contains a
            // poll loop that blocks the thread. If we ran it inline,
            // the tokio runtime could schedule other futures
            // (including a second iteration of this very worker) on
            // other threads, breaking the "at most one PHPStan
            // process" guarantee. By awaiting the offloaded call,
            // this task is suspended (not occupying a runtime
            // thread) and no re-entry can happen until it resolves.
            let phpstan_config = config.phpstan.clone();
            let shutdown_flag = Arc::clone(&self.shutdown_flag);
            let phpstan_diags = {
                let result = crate::server::run_blocking_cancel_safe("phpstan", move || {
                    phpstan::run_phpstan(
                        &resolved,
                        &content,
                        &file_path,
                        &workspace_root,
                        &phpstan_config,
                        &shutdown_flag,
                    )
                })
                .await;

                match result {
                    Some(Ok(diags)) => diags,
                    // PHPStan failures are silently ignored to avoid
                    // flooding the editor with errors when PHPStan is
                    // misconfigured or the project doesn't use it.
                    // (A panic is logged by the helper itself.)
                    _ => continue,
                }
            };

            // ── Step 5: cache results and re-publish ────────────────
            // Verify the file is still open *before* writing to the
            // cache. If the file was closed while PHPStan was running,
            // `clear_diagnostics_for_file` already purged the cache
            // entry — writing it back would leave stale diagnostics
            // that resurface on the next `did_open`.
            {
                let files = self.open_files.read();
                if !files.contains_key(&uri) {
                    continue;
                }
            }

            self.phpstan_tool.store_file_result(&uri, phpstan_diags);

            // Assemble and push so the editor sees fresh PHPStan
            // results merged with cached native diagnostics.  In pull
            // mode this also tells the editor to re-pull, but only when
            // the run actually changed the file's diagnostics.
            self.assemble_and_refresh(&uri).await;
        }
    }
}
