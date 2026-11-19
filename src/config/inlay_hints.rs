//! `[inlay-hints]` section and the matching editor-supplied toggles.

use serde::Deserialize;

/// `[inlay-hints]` section: which kinds of inlay hint are shown.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct InlayHintsConfig {
    /// Parameter names (and the `&` of by-reference parameters) before
    /// the arguments of a call. On by default.
    #[serde(rename = "parameter-names")]
    pub parameter_names: Option<bool>,
    /// Inferred parameter and return types on closures and arrow
    /// functions that do not declare them. On by default.
    #[serde(rename = "closure-types")]
    pub closure_types: Option<bool>,
}

/// The inlay hint toggles an editor forwards from its own settings.
///
/// Read from `initializationOptions` and from the `settings` of a
/// `workspace/didChangeConfiguration` notification:
///
/// ```json
/// { "inlayHints": { "parameterNames": false, "closureTypes": true } }
/// ```
///
/// A key left out has no opinion. This layer can only switch a kind off,
/// never back on over `.phpantom.toml`: clients push their defaults along
/// with everything else (VS Code sends its whole `phpantom` section), so
/// letting an editor's `true` win would silently undo a project's `false`.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ClientInlayHintOptions {
    pub parameter_names: Option<bool>,
    pub closure_types: Option<bool>,
}

impl ClientInlayHintOptions {
    /// Read the `inlayHints` block out of a client-supplied settings blob,
    /// at the top level or under a `phpantom` key, like
    /// [`ClientIndexingOptions::from_client_settings`].
    ///
    /// Returns `None` when the blob carries no readable `inlayHints`
    /// block, meaning the notification is not about inlay hints.
    ///
    /// [`ClientIndexingOptions::from_client_settings`]: super::ClientIndexingOptions::from_client_settings
    pub fn from_client_settings(settings: &serde_json::Value) -> Option<Self> {
        let scoped = settings.get("phpantom").unwrap_or(settings);
        serde_json::from_value(scoped.get("inlayHints")?.clone()).ok()
    }
}

/// The inlay hint kinds in force once both layers are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlayHintKinds {
    pub parameter_names: bool,
    pub closure_types: bool,
}

impl InlayHintKinds {
    /// A kind is shown unless the config file or the editor turns it off.
    pub fn resolve(config: &InlayHintsConfig, client: &ClientInlayHintOptions) -> Self {
        let on = |file: Option<bool>, editor: Option<bool>| {
            file.unwrap_or(true) && editor.unwrap_or(true)
        };
        Self {
            parameter_names: on(config.parameter_names, client.parameter_names),
            closure_types: on(config.closure_types, client.closure_types),
        }
    }

    /// Whether no kind is shown at all.
    pub fn none(self) -> bool {
        !self.parameter_names && !self.closure_types
    }
}
