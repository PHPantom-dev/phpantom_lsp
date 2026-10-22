//! What the connected client supports.

use tower_lsp::lsp_types::{ClientCapabilities, ResourceOperationKind};

/// The capabilities the client advertised in its `initialize` request,
/// set once there and read wherever a feature depends on one.
///
/// Every flag starts out `false`: a feature that needs one is held back
/// until the client says it can take it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ClientCaps {
    /// Whether the client supports pull diagnostics.
    ///
    /// Set during `initialize` based on the client's
    /// `textDocument.diagnostic` capability.  When `true`, the server
    /// uses pull diagnostics (`textDocument/diagnostic`) as the primary
    /// path and sends `workspace/diagnostic/refresh` instead of
    /// `schedule_diagnostics_for_open_files`.  When `false`, the server
    /// falls back to the push model (`textDocument/publishDiagnostics`).
    pub(crate) pull_diagnostics: bool,
    /// Whether the client supports file rename operations in workspace edits.
    ///
    /// Set during `initialize` based on the client's
    /// `workspace.workspaceEdit.resourceOperations` capability.  When `true`
    /// and a class rename matches PSR-4 naming (filename == class name),
    /// the rename response includes a `RenameFile` operation alongside the
    /// text edits so the file is renamed to match the new class name.
    pub(crate) file_rename: bool,
    /// Whether the client supports file creation in workspace edits
    /// (`workspace.workspaceEdit.resourceOperations` includes `create`).
    /// The code actions that create a file (extract interface, create a
    /// missing view) are only offered when it does.
    pub(crate) file_create: bool,
    /// Whether the client can resolve deferred code actions: it lists `data`
    /// support and `edit` in `codeAction.resolveSupport.properties`.  When it
    /// cannot, `handle_code_action` sends every edit up front.
    pub(crate) code_action_resolve: bool,
    /// Whether the client supports server-initiated work-done progress.
    ///
    /// Set during `initialize` based on the client's
    /// `window.workDoneProgress` capability.  When `false`, the server
    /// must not send `window/workDoneProgress/create` requests because
    /// the client will not handle them, blocking the server indefinitely.
    pub(crate) work_done_progress: bool,
    /// Whether the client supports dynamic registration for type hierarchy.
    pub(crate) type_hierarchy_dynamic_registration: bool,
    /// Whether the client accepts dynamic registration of
    /// `workspace/didChangeWatchedFiles`
    /// (`workspace.didChangeWatchedFiles.dynamicRegistration`).
    ///
    /// The protocol has no static way to ask for file events, so this is
    /// the only way to hear about a change made outside the editor. A
    /// client that did not opt in is never sent the registration: it may
    /// not understand the request, and may never answer it.
    pub(crate) watched_files_dynamic_registration: bool,
    /// Whether the client can match a watcher pattern against a base URI
    /// (`workspace.didChangeWatchedFiles.relativePatternSupport`).
    ///
    /// A plain `**/*.php` pattern is matched against the files of the
    /// workspace folders, which gives a client no reason to watch a
    /// directory living outside them, and nothing obliges it to traverse a
    /// symlink to find one. A relative pattern names the link outright,
    /// which is the only way in the protocol to ask for those events;
    /// without the capability, a tree reached through a link is indexed but
    /// not watched.
    pub(crate) relative_pattern_watchers: bool,
    /// Whether the client supports `window/showDocument`.
    ///
    /// Set during `initialize` based on the client's
    /// `window.showDocument.support` capability.  Code lens navigation
    /// asks the client to open the prototype's file through that request,
    /// so a client that does not opt in gets a lens command it can act on
    /// by itself instead (see `code_lens::build_code_lens_command`).
    pub(crate) show_document: bool,
    /// Whether the client supports `workspace/semanticTokens/refresh`.
    ///
    /// Set during `initialize` based on the client's
    /// `workspace.semanticTokens.refreshSupport` capability.  When `true`,
    /// the server asks the client to re-pull semantic tokens after a
    /// background `didChange` parse commits a new symbol map — without
    /// this, editors keep showing tokens computed from the pre-edit
    /// symbol map until the next unrelated request.
    pub(crate) semantic_tokens_refresh: bool,
    /// Whether the client supports `workspace/codeLens/refresh`.
    ///
    /// Exact member-reference locations are computed outside the CodeLens
    /// request.  Supporting clients re-pull once that bounded cache is warm,
    /// avoiding a burst of lazy resolve requests for every declaration.
    pub(crate) code_lens_refresh: bool,
    /// Whether the client supports `workspace/inlayHint/refresh`.
    ///
    /// Set during `initialize` from the client's
    /// `workspace.inlayHint.refreshSupport` capability.  Hints resolve
    /// against the workspace index and a background parse, so without a
    /// refresh the editor keeps the ones it pulled before either was
    /// ready.
    pub(crate) inlay_hint_refresh: bool,
}

impl ClientCaps {
    /// What `capabilities` says the client can take.
    pub(crate) fn from_capabilities(capabilities: &ClientCapabilities) -> Self {
        let pull_diagnostics = capabilities
            .text_document
            .as_ref()
            .and_then(|td| td.diagnostic.as_ref())
            .is_some();

        // Detect which resource operations the client accepts in workspace
        // edits: the rename handler includes a `RenameFile` operation when a
        // class rename matches PSR-4 naming, and the code actions that
        // create a file (extract interface, create a missing view) are only
        // offered when `CreateFile` is accepted.
        let resource_operations = capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.workspace_edit.as_ref())
            .and_then(|we| we.resource_operations.as_deref())
            .unwrap_or_default();
        let file_rename = resource_operations.contains(&ResourceOperationKind::Rename);
        let file_create = resource_operations.contains(&ResourceOperationKind::Create);

        // Deferred code actions carry `data` and no `edit`, which the spec
        // only allows for a client that resolves `edit` lazily.
        let code_action = capabilities
            .text_document
            .as_ref()
            .and_then(|td| td.code_action.as_ref());
        let code_action_resolve = code_action.is_some_and(|ca| {
            ca.data_support == Some(true)
                && ca
                    .resolve_support
                    .as_ref()
                    .is_some_and(|rs| rs.properties.iter().any(|p| p == "edit"))
        });

        // Detect whether the client supports server-initiated work-done
        // progress (window/workDoneProgress/create).  Per the LSP spec,
        // we must not send that request unless the client opts in.
        let work_done_progress = capabilities
            .window
            .as_ref()
            .and_then(|w| w.work_done_progress)
            .unwrap_or(false);

        // Detect whether the client handles `window/showDocument`.  Code
        // lens navigation routes through that request, so a client that
        // does not opt in needs a lens command it can act on itself.
        let show_document = capabilities
            .window
            .as_ref()
            .and_then(|w| w.show_document.as_ref())
            .is_some_and(|sd| sd.support);

        // Detect whether the client supports server-initiated semantic
        // token refreshes (`workspace/semanticTokens/refresh`).  Used to
        // re-pull tokens after background didChange parses commit a new
        // symbol map.
        let semantic_tokens_refresh = capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.semantic_tokens.as_ref())
            .and_then(|st| st.refresh_support)
            .unwrap_or(false);

        let code_lens_refresh = capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.code_lens.as_ref())
            .and_then(|code_lens| code_lens.refresh_support)
            .unwrap_or(false);

        // Reference counts on declarations are computed off the request
        // path, so the hints an editor holds are the ones from before the
        // counts landed unless it can be asked to re-pull them.
        let inlay_hint_refresh = capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.inlay_hint.as_ref())
            .and_then(|ih| ih.refresh_support)
            .unwrap_or(false);

        let watched_files_capability = capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.did_change_watched_files.as_ref());

        // Asking a client to watch files is a `client/registerCapability`
        // request, and the protocol has no other way to do it.  One that did
        // not opt in may not understand the request, and may never answer
        // it, so it is not sent one.
        let watched_files_dynamic_registration = watched_files_capability
            .and_then(|w| w.dynamic_registration)
            .unwrap_or(false);

        // A tree indexed through a symlink is not inside any workspace
        // folder, so being told about a change in it takes a watcher that
        // names the link.  Clients that predate LSP 3.17 get none, and a
        // `git pull` into a linked framework needs a reload there.
        let relative_pattern_watchers = watched_files_capability
            .and_then(|w| w.relative_pattern_support)
            .unwrap_or(false);

        let type_hierarchy_dynamic_registration = capabilities
            .text_document
            .as_ref()
            .and_then(|td| td.type_hierarchy.as_ref())
            .and_then(|th| th.dynamic_registration)
            .unwrap_or(false);

        Self {
            pull_diagnostics,
            file_rename,
            file_create,
            code_action_resolve,
            work_done_progress,
            type_hierarchy_dynamic_registration,
            watched_files_dynamic_registration,
            relative_pattern_watchers,
            show_document,
            semantic_tokens_refresh,
            code_lens_refresh,
            inlay_hint_refresh,
        }
    }
}
