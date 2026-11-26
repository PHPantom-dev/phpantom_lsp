//! `[trust]` section: which workspaces may run the project's own tooling.
//!
//! Only the global config is read for this section.  A project cannot
//! vouch for itself, so `load_config_with_warnings` drops a `[trust]`
//! table found in a project's `.phpantom.toml` with a warning.

use serde::Deserialize;

/// `[trust]` section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TrustConfig {
    /// Treat every workspace as trusted, so PHPantom runs PHPStan,
    /// PHPCS, PHPMD, Mago and the external formatters without asking.
    ///
    /// - `None` / `false` (default) — ask the first time a workspace
    ///   the editor has not vouched for would run one of them.
    /// - `true` — never ask.
    #[serde(rename = "all-workspaces")]
    pub all_workspaces: Option<bool>,
}

impl TrustConfig {
    /// Whether every workspace is trusted.
    pub fn trusts_all_workspaces(&self) -> bool {
        self.all_workspaces == Some(true)
    }
}
