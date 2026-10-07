//! PHPCS proxy diagnostics: schedule function and background worker.

use std::path::Path;

use crate::Backend;
use crate::config::Config;
use crate::phpcs;

use super::ExternalToolRun;

/// Resolve the PHPCS binary for the workspace.
fn prepare_phpcs(config: &Config, workspace_root: &Path) -> Option<ExternalToolRun> {
    if config.phpcs.is_disabled() {
        return None;
    }

    let composer_pkg = crate::composer::read_composer_package(workspace_root);
    let bin_dir: Option<String> = composer_pkg.as_ref().map(crate::composer::get_bin_dir);

    let resolved = phpcs::resolve_phpcs(
        Some(workspace_root),
        &config.phpcs,
        bin_dir.as_deref(),
        composer_pkg.as_ref(),
    )?;

    let phpcs_config = config.phpcs.clone();
    Some(Box::new(
        move |content, file_path, workspace_root, cancelled| {
            phpcs::run_phpcs(
                &resolved,
                content,
                file_path,
                workspace_root,
                &phpcs_config,
                cancelled,
            )
        },
    ))
}

impl Backend {
    // ── PHPCS worker ────────────────────────────────────────────────

    /// Schedule a PHPCS run for a single file.
    pub(crate) fn schedule_phpcs(&self, uri: String) {
        Self::schedule_external_tool(&self.phpcs_tool, uri);
    }

    /// Run PHPCS on pending files. See [`Backend::external_tool_worker`].
    pub(crate) async fn phpcs_worker(&self) {
        self.external_tool_worker(&self.phpcs_tool, "phpcs", prepare_phpcs)
            .await;
    }
}
