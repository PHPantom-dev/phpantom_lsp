//! PHPCS proxy diagnostics: schedule function and background worker.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::phpcs;

impl Backend {
    // ── PHPCS worker ────────────────────────────────────────────────

    /// Schedule a PHPCS run for a single file.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// saves rapidly, earlier requests are superseded.
    pub(crate) fn schedule_phpcs(&self, uri: String) {
        *self.phpcs_tool.pending_uri.lock() = Some(uri);
        self.phpcs_tool.notify.notify_one();
    }

    /// Long-lived background task that runs PHPCS on pending files.
    ///
    /// Spawned once during `initialized`, alongside the main diagnostic
    /// worker and the PHPStan worker. This task is completely
    /// independent: native diagnostics and PHPStan are never blocked.
    ///
    /// ## Serialization guarantee
    ///
    /// At most one PHPCS process runs at a time. The worker loop:
    ///
    /// 1. Wait for a notification (file saved).
    /// 2. Snapshot the pending URI and file content.
    /// 3. Resolve the PHPCS binary (skip if not found / disabled).
    /// 4. Run PHPCS (blocking — this is the slow part).
    /// 5. Cache the results and re-publish diagnostics for the file.
    /// 6. Loop back to step 1.
    ///
    /// If the user saves again while step 4 is in progress, the pending
    /// URI is updated. When step 4 finishes, the worker sees the new
    /// notification and loops back to step 1, starting a fresh run
    /// with the latest content.
    pub(crate) async fn phpcs_worker(&self) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 1: wait for work ───────────────────────────────
            self.phpcs_tool.notify.notified().await;

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // Drain any extra stored permits (same rationale as the
            // PHPStan worker).
            let _ =
                tokio::time::timeout(std::time::Duration::ZERO, self.phpcs_tool.notify.notified())
                    .await;

            // ── Step 2: snapshot the pending URI ────────────────────
            let uri = match self.phpcs_tool.pending_uri.lock().take() {
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

            // ── Step 3: resolve PHPCS binary ────────────────────────
            let config = self.config();
            if config.phpcs.is_disabled() {
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

            let bin_dir: Option<String> = crate::composer::read_composer_package(&workspace_root)
                .map(|pkg| crate::composer::get_bin_dir(&pkg));

            let resolved = match phpcs::resolve_phpcs(
                Some(&workspace_root),
                &config.phpcs,
                bin_dir.as_deref(),
            ) {
                Some(r) => r,
                None => continue,
            };

            // ── Step 4: run PHPCS (the slow part) ───────────────────
            let phpcs_config = config.phpcs.clone();
            let shutdown_flag = Arc::clone(&self.shutdown_flag);
            let phpcs_diags = {
                let result = crate::server::run_blocking_cancel_safe("phpcs", move || {
                    phpcs::run_phpcs(
                        &resolved,
                        &content,
                        &file_path,
                        &workspace_root,
                        &phpcs_config,
                        &shutdown_flag,
                    )
                })
                .await;

                match result {
                    Some(Ok(diags)) => diags,
                    // PHPCS failures are silently ignored to avoid
                    // flooding the editor with errors when PHPCS is
                    // misconfigured or the project doesn't use it.
                    // (A panic is logged by the helper itself.)
                    _ => continue,
                }
            };

            // ── Step 5: cache results and re-publish ────────────────
            // Verify the file is still open before caching (same
            // rationale as the PHPStan worker).
            {
                let files = self.open_files.read();
                if !files.contains_key(&uri) {
                    continue;
                }
            }

            self.phpcs_tool.store_file_result(&uri, phpcs_diags);

            // Assemble and push so the editor sees fresh PHPCS
            // results merged with cached native diagnostics.  In pull
            // mode this also tells the editor to re-pull, but only when
            // the run actually changed the file's diagnostics.
            self.assemble_and_refresh(&uri).await;
        }
    }
}
