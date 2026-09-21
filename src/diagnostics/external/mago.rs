//! Mago lint and Mago analyze proxy diagnostics: schedule functions and
//! background workers.

use std::path::Path;

use crate::Backend;
use crate::config::Config;
use crate::mago;

use super::ExternalToolRun;

/// Resolve the Mago binary for one Mago command.
///
/// `service` picks the [`mago::MagoServices`] flag that decides whether
/// the project uses this command at all, and `run` is the command itself.
fn prepare_mago(
    config: &Config,
    workspace_root: &Path,
    service: fn(&mago::MagoServices) -> bool,
    run: mago::MagoFileRunner,
) -> Option<ExternalToolRun> {
    if config.mago.is_disabled() {
        return None;
    }

    let composer_pkg = crate::composer::read_composer_package(workspace_root);
    let laravel = composer_pkg
        .as_ref()
        .is_some_and(crate::composer::is_laravel_project);

    // Mago requires mago.toml to operate, and its tables decide whether
    // the project uses this command at all.
    if !service(&mago::enabled_services(
        workspace_root,
        &config.mago,
        laravel,
    )) {
        return None;
    }

    let bin_dir: Option<String> = composer_pkg.as_ref().map(crate::composer::get_bin_dir);

    let resolved = mago::resolve_mago(
        Some(workspace_root),
        &config.mago,
        bin_dir.as_deref(),
        composer_pkg.as_ref(),
    )?;

    let mago_config = config.mago.clone();
    Some(Box::new(
        move |content, file_path, workspace_root, cancelled| {
            run(
                &resolved,
                content,
                file_path,
                workspace_root,
                &mago_config,
                cancelled,
            )
        },
    ))
}

impl Backend {
    // ── Mago lint worker ────────────────────────────────────────────

    /// Schedule a Mago lint run for a single file.
    pub(crate) fn schedule_mago_lint(&self, uri: String) {
        Self::schedule_external_tool(&self.mago_lint_tool, uri);
    }

    /// Run `mago lint` on pending files.
    /// See [`Backend::external_tool_worker`].
    pub(crate) async fn mago_lint_worker(&self) {
        self.external_tool_worker(&self.mago_lint_tool, "mago lint", |config, root| {
            prepare_mago(config, root, |services| services.lint, mago::run_mago_lint)
        })
        .await;
    }

    // ── Mago analyze worker ─────────────────────────────────────────

    /// Schedule a Mago analyze run for a single file.
    pub(crate) fn schedule_mago_analyze(&self, uri: String) {
        Self::schedule_external_tool(&self.mago_analyze_tool, uri);
    }

    /// Run `mago analyze` on pending files.
    /// See [`Backend::external_tool_worker`].
    pub(crate) async fn mago_analyze_worker(&self) {
        self.external_tool_worker(&self.mago_analyze_tool, "mago analyze", |config, root| {
            prepare_mago(
                config,
                root,
                |services| services.analyze,
                mago::run_mago_analyze,
            )
        })
        .await;
    }
}
