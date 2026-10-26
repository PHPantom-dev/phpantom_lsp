//! `[stubs]` section and the stub directory an editor extension supplies.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// `[stubs]` section: PHP files whose declarations win over every other
/// declaration of the same name.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StubsConfig {
    /// Stub files and directories, relative to the workspace root
    /// (absolute paths are also accepted).
    ///
    /// A class, function or constant declared here is what the name means,
    /// whatever the project, its dependencies or PHP's own stubs declare
    /// under it. Directories are walked recursively, hidden and gitignored
    /// entries included. Stub files are never analysed or edited.
    ///
    /// ```toml
    /// [stubs]
    /// paths = ["stubs", "/opt/company/php-stubs"]
    /// ```
    pub paths: Option<Vec<String>>,
}

impl StubsConfig {
    /// Resolve [`paths`](Self::paths) against the workspace root, dropping
    /// entries that do not exist.
    pub fn resolved_paths(&self, workspace_root: &Path) -> Vec<PathBuf> {
        self.paths
            .iter()
            .flatten()
            .map(|entry| workspace_root.join(entry))
            .filter(|path| path.exists())
            .collect()
    }
}

/// The stub directory an editor extension ships alongside the server.
///
/// An extension that bundles stubs (phpstorm-stubs as plain files, say)
/// knows where it put them; the user does not. It passes the location
/// through `initializationOptions`, and again through
/// `workspace/didChangeConfiguration` if it moves:
///
/// ```json
/// { "stubs": { "path": "/path/to/bundled/stubs" } }
/// ```
///
/// The `[stubs]` paths of `.phpantom.toml` win over this one where both
/// declare a name, since they are the project's own choice.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ClientStubOptions {
    /// The stub directory, or `None` for none.
    pub path: Option<PathBuf>,
}

impl ClientStubOptions {
    /// Read the `stubs` block out of a client-supplied settings blob, in
    /// the shapes [`ClientIndexingOptions::from_client_settings`] accepts.
    ///
    /// Returns `None` when the blob carries no readable `stubs` block,
    /// meaning the notification is not about stubs rather than that the
    /// extension dropped its path.
    ///
    /// [`ClientIndexingOptions::from_client_settings`]: super::ClientIndexingOptions::from_client_settings
    pub fn from_client_settings(settings: &serde_json::Value) -> Option<Self> {
        let scoped = settings.get("phpantom").unwrap_or(settings);
        serde_json::from_value(scoped.get("stubs")?.clone()).ok()
    }
}
