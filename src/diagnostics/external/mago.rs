//! Mago lint and Mago analyze proxy diagnostics: schedule functions and
//! background workers.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::mago;

impl Backend {
    // ── Mago lint worker ────────────────────────────────────────────

    /// Schedule a Mago lint run for a single file.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// saves rapidly, earlier requests are superseded.
    pub(crate) fn schedule_mago_lint(&self, uri: String) {
        *self.mago_lint_tool.pending_uri.lock() = Some(uri);
        self.mago_lint_tool.notify.notify_one();
    }

    /// Long-lived background task that runs one Mago command on pending
    /// files.
    ///
    /// Spawned once per command during `initialized`. Each task is
    /// completely independent: native diagnostics, PHPStan, PHPCS, and
    /// the other Mago command are never blocked. At most one process per
    /// command runs at a time.
    ///
    /// `service` picks the [`mago::MagoServices`] flag that decides
    /// whether the project uses this command at all, and `run` is the
    /// command itself.
    async fn mago_worker(
        &self,
        tool: &crate::ExternalToolWorker,
        label: &'static str,
        service: fn(&mago::MagoServices) -> bool,
        run: mago::MagoFileRunner,
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

            // Drain any extra stored permits.
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

            // ── Step 3: resolve Mago binary ─────────────────────────
            let config = self.config();
            if config.mago.is_disabled() {
                continue;
            }

            let workspace_root = self.workspace.workspace_root.read().clone();
            let workspace_root = match workspace_root {
                Some(root) => root,
                None => continue,
            };

            let composer_pkg = crate::composer::read_composer_package(&workspace_root);
            let laravel = composer_pkg
                .as_ref()
                .is_some_and(crate::composer::is_laravel_project);

            // Mago requires mago.toml to operate, and its tables decide
            // whether the project uses this command at all.
            if !service(&mago::enabled_services(
                &workspace_root,
                &config.mago,
                laravel,
            )) {
                continue;
            }

            let file_path = match uri.parse::<Url>().ok().and_then(|u| u.to_file_path().ok()) {
                Some(p) => p,
                None => continue,
            };

            let bin_dir: Option<String> = composer_pkg.as_ref().map(crate::composer::get_bin_dir);

            let resolved = match mago::resolve_mago(
                Some(&workspace_root),
                &config.mago,
                bin_dir.as_deref(),
                composer_pkg.as_ref(),
            ) {
                Some(r) => r,
                None => continue,
            };

            // ── Step 4: run the command (the slow part) ─────────────
            let mago_config = config.mago.clone();
            let shutdown_flag = Arc::clone(&self.shutdown_flag);
            let mago_diags = {
                let result = crate::server::run_blocking_cancel_safe(label, move || {
                    run(
                        &resolved,
                        &content,
                        &file_path,
                        &workspace_root,
                        &mago_config,
                        &shutdown_flag,
                    )
                })
                .await;

                match result {
                    Some(Ok(diags)) => diags,
                    _ => continue,
                }
            };

            // ── Step 5: cache results and re-publish ────────────────
            {
                let files = self.open_files.read();
                if !files.contains_key(&uri) {
                    continue;
                }
            }

            tool.store_file_result(&uri, mago_diags);

            // In pull mode this also tells the editor to re-pull, but
            // only when the run actually changed the file's diagnostics.
            self.assemble_and_refresh(&uri).await;
        }
    }

    /// Run `mago lint` on pending files. See [`Backend::mago_worker`].
    pub(crate) async fn mago_lint_worker(&self) {
        self.mago_worker(
            &self.mago_lint_tool,
            "mago lint",
            |services| services.lint,
            mago::run_mago_lint,
        )
        .await;
    }

    // ── Mago analyze worker ─────────────────────────────────────────

    /// Schedule a Mago analyze run for a single file.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// saves rapidly, earlier requests are superseded.
    pub(crate) fn schedule_mago_analyze(&self, uri: String) {
        *self.mago_analyze_tool.pending_uri.lock() = Some(uri);
        self.mago_analyze_tool.notify.notify_one();
    }

    /// Run `mago analyze` on pending files. See [`Backend::mago_worker`].
    pub(crate) async fn mago_analyze_worker(&self) {
        self.mago_worker(
            &self.mago_analyze_tool,
            "mago analyze",
            |services| services.analyze,
            mago::run_mago_analyze,
        )
        .await;
    }
}
