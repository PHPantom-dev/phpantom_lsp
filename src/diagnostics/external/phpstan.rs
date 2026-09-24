//! PHPStan proxy diagnostics: schedule function and background worker.

use std::path::Path;

use crate::Backend;
use crate::config::Config;
use crate::phpstan;

use super::ExternalToolRun;

/// Resolve the PHPStan binary for the workspace.
fn prepare_phpstan(config: &Config, workspace_root: &Path) -> Option<ExternalToolRun> {
    if config.phpstan.is_disabled() {
        return None;
    }

    let composer_pkg = crate::composer::read_composer_package(workspace_root);
    let bin_dir: Option<String> = composer_pkg.as_ref().map(crate::composer::get_bin_dir);

    let resolved = phpstan::resolve_phpstan(
        Some(workspace_root),
        &config.phpstan,
        bin_dir.as_deref(),
        composer_pkg.as_ref(),
    )?;

    let phpstan_config = config.phpstan.clone();
    Some(Box::new(
        move |content, file_path, workspace_root, cancelled| {
            phpstan::run_phpstan(
                &resolved,
                content,
                file_path,
                workspace_root,
                &phpstan_config,
                cancelled,
            )
        },
    ))
}

impl Backend {
    // ── PHPStan worker ──────────────────────────────────────────────

    /// Schedule a PHPStan run for a single file.
    pub(crate) fn schedule_phpstan(&self, uri: String) {
        Self::schedule_external_tool(&self.phpstan_tool, uri);
    }

    /// Run PHPStan on pending files. See [`Backend::external_tool_worker`].
    pub(crate) async fn phpstan_worker(&self) {
        self.external_tool_worker(&self.phpstan_tool, "phpstan", prepare_phpstan)
            .await;
    }
}
