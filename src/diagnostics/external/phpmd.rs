//! PHPMD proxy diagnostics: schedule function and background worker.

use std::path::Path;

use crate::Backend;
use crate::config::Config;
use crate::phpmd;

use super::ExternalToolRun;

/// Resolve the PHPMD binary for the workspace.
fn prepare_phpmd(config: &Config, workspace_root: &Path) -> Option<ExternalToolRun> {
    if config.phpmd.is_disabled() {
        return None;
    }

    let bin_dir: Option<String> = crate::composer::read_composer_package(workspace_root)
        .map(|pkg| crate::composer::get_bin_dir(&pkg));

    let resolved = phpmd::resolve_phpmd(Some(workspace_root), &config.phpmd, bin_dir.as_deref())?;

    let phpmd_config = config.phpmd.clone();
    Some(Box::new(
        move |content, _file_path, workspace_root, cancelled| {
            phpmd::run_phpmd(&resolved, content, workspace_root, &phpmd_config, cancelled)
        },
    ))
}

impl Backend {
    // ── PHPMD worker ────────────────────────────────────────────────

    /// Schedule a PHPMD run for a single file.
    pub(crate) fn schedule_phpmd(&self, uri: String) {
        Self::schedule_external_tool(&self.phpmd_tool, uri);
    }

    /// Run PHPMD on pending files. See [`Backend::external_tool_worker`].
    pub(crate) async fn phpmd_worker(&self) {
        self.external_tool_worker(&self.phpmd_tool, "phpmd", prepare_phpmd)
            .await;
    }
}
