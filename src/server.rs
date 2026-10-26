//! LSP server trait implementation.
//!
//! This module contains the `impl LanguageServer for Backend` block,
//! which handles all LSP protocol messages (initialize, didOpen, didChange,
//! didClose, completion, diagnostic, etc.).
//!
//! **Diagnostic delivery.** Two native delivery models are supported and are
//! selected automatically from the client's capabilities. The server treats
//! pull diagnostics as the preferred modern path and uses push only as a
//! fallback for older clients; it deliberately does not send the same native
//! diagnostics through both channels for the same client.
//!
//! The `diagnostic_provider` capability is advertised to every client, but
//! the delivery model follows the client's own declaration.
//!
//! - **Pull model** (preferred) — when the client advertises
//!   `textDocument.diagnostic` support.  The editor requests diagnostics
//!   via `textDocument/diagnostic` for visible files and
//!   `workspace/diagnostic` for all open files.  Cross-file invalidation
//!   (e.g. a class signature change) sends `workspace/diagnostic/refresh`
//!   so the editor re-pulls only the files it cares about.  The
//!   background workspace pass over unopened files is deferred until the
//!   client's first `workspace/diagnostic` request, since its results
//!   are only deliverable through workspace pull responses.
//!
//! - **Push model** (fallback) — for clients without pull support, the
//!   server pushes diagnostics via `textDocument/publishDiagnostics`
//!   from a debounced background worker.  Each `did_change` bumps a
//!   version counter; the worker waits for a quiet period before
//!   publishing.  Each published set is also cached with a `resultId`,
//!   so a pull from such a client answers with what was last published.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;
use tower_lsp_server::jsonrpc::Result;

use crate::Backend;
use crate::formatting;

/// Run `f` on a blocking thread in a way that survives `$/cancelRequest`.
///
/// The tower-lsp serve loop wedges if a request handler future is
/// dropped (which is how it implements cancellation) while that future is
/// directly awaiting a `spawn_blocking` JoinHandle: dropping the await
/// detaches the handle, and when the orphaned blocking task later finishes it
/// corrupts tower-lsp's internal request/response state.  Once that happens
/// the server goes completely silent (every worker idle-parked, no responses)
/// even though nothing is deadlocked.  Editors cancel aggressively (a moving
/// cursor cancels each in-flight hover/highlight), so any blocking handler
/// that is not protected this way is a latent total-hang.
///
/// Wrapping the blocking call in an inner `tokio::spawn` keeps it owned by a
/// live task that always runs to completion, so the handle is never orphaned.
/// Returns `None` only if the blocking task itself panicked, in which case
/// `name` identifies the handler in the log.
///
/// Every request handler that does non-trivial CPU work (parsing, whole-file
/// scanning, workspace walking, class loading) must route it through here
/// rather than running it on the async request task, and must do so through
/// this one helper rather than an ad-hoc `spawn_blocking`.
pub(crate) async fn run_blocking_cancel_safe<R, F>(name: &'static str, f: F) -> Option<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    match tokio::spawn(async move { tokio::task::spawn_blocking(f).await }).await {
        Ok(Ok(value)) => Some(value),
        // The inner blocking task panicked, or the outer task carrying it did.
        // Either way the request answers with its fallback; without this log
        // the panic would be invisible and read as an empty result.
        Ok(Err(err)) => {
            tracing::error!("PHPantom: {name} blocking task failed: {err}");
            None
        }
        Err(err) => {
            tracing::error!("PHPantom: {name} task failed: {err}");
            None
        }
    }
}

/// Offload `f` to the blocking pool without waiting for it, logging a panic
/// instead of dropping it.
///
/// For fire-and-forget work started from a notification handler, where a bare
/// `spawn_blocking` would discard both the result and any panic.
pub(crate) fn spawn_blocking_detached<F>(name: &'static str, f: F)
where
    F: FnOnce() + Send + 'static,
{
    tokio::spawn(async move {
        run_blocking_cancel_safe(name, f).await;
    });
}

impl Backend {
    /// Whether `uri` is a resource document (a Twig template, a YAML or XML
    /// file), which the client serves with another extension.  PHPantom
    /// answers there only for the names it reads (completion, hover,
    /// definition, references, rename, and the code actions on a string
    /// key); every other request answers nothing, so the editor keeps that
    /// extension's formatter, highlighting, outline, and folding rather
    /// than having PHPantom's PHP features displace them.
    fn is_foreign_document(&self, uri: &Uri) -> bool {
        self.is_resource_uri(uri.as_str())
    }

    /// Run a position-based request handler on the blocking pool.
    ///
    /// Wires up the `Backend` clone, URI clone, and cancel-safe dispatch
    /// shared by every handler that resolves a
    /// `TextDocumentPositionParams`-shaped request and has no extra
    /// progress-token wrapping. `body` receives the cloned backend, the
    /// URI, and the position, and is responsible for calling
    /// [`Backend::handle_with_position`] itself (some handlers run
    /// Blade-specific checks first).
    async fn run_position_request<T, F>(
        &self,
        name: &'static str,
        uri: String,
        position: Position,
        body: F,
    ) -> Result<Option<T>>
    where
        T: Send + 'static,
        F: FnOnce(&Backend, &str, Position) -> Result<Option<T>> + Send + 'static,
    {
        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        run_blocking_cancel_safe(name, move || body(&backend, &uri_clone, position))
            .await
            .unwrap_or(Ok(None))
    }

    /// Run `body` on the blocking pool behind a work-done progress report.
    ///
    /// Reports through the client's `token`, or one created under `name`
    /// when the client sent none, and polls the [`ScanProgress`] the backend
    /// clone handed to `body` reports into. Running off the async runtime is
    /// what leaves it free to flush those notifications while `body` works.
    ///
    /// [`ScanProgress`]: crate::progress::ScanProgress
    async fn run_with_progress<R, F>(
        &self,
        name: &'static str,
        title: &str,
        message: &str,
        token: Option<ProgressToken>,
        body: F,
    ) -> Option<R>
    where
        R: Send + 'static,
        F: FnOnce(&Backend) -> R + Send + 'static,
    {
        let token = match token {
            Some(t) => Some(t),
            None => self.progress_create(name).await,
        };

        if let Some(ref tok) = token {
            self.progress_begin(tok, title, Some(message.to_string()))
                .await;
        }

        let mut backend = self.clone_for_blocking();
        let poller = token.as_ref().map(|tok| {
            let state = crate::progress::ScanProgress::new();
            backend.request_progress = Some(Arc::clone(&state));
            self.spawn_progress_poller(tok.clone(), state)
        });
        let result = run_blocking_cancel_safe(name, move || body(&backend)).await;

        if let Some(poller) = poller {
            poller.finish().await;
        }
        if let Some(ref tok) = token {
            self.progress_end(tok, Some("Done".to_string())).await;
        }

        result
    }

    /// Format a document, or with `range` only the lines it selects.
    ///
    /// The built-in formatter formats the construct enclosing the range;
    /// the other formatters work on a whole file.  Either way only the
    /// edits that touch the range are kept.
    async fn format_document(
        &self,
        uri: &Uri,
        options: &FormattingOptions,
        range: Option<Range>,
    ) -> Result<Option<Vec<TextEdit>>> {
        if self.is_foreign_document(uri) {
            return Ok(None);
        }
        let uri = uri.to_string();

        // External tools discover their config from the file's real path.
        let Some(file_path) = crate::util::uri_to_path(&uri) else {
            return Ok(None);
        };
        let Some(content) = self.get_file_content(&uri) else {
            return Ok(None);
        };

        // Blade markup isn't PHP, so a template resolves its own strategy:
        // Pint or blade-formatter when the project formats Blade with one,
        // the built-in reindenter otherwise.
        let is_blade = self.is_blade_file(&uri);
        let blade_options = formatting::blade::options_from_lsp(options);

        // Resolving the strategy reads composer.json and running it may
        // spawn an external tool, so all of it stays off the async runtime.
        let backend = self.clone_for_blocking();
        let result = run_blocking_cancel_safe("formatting", move || {
            let formatted = if is_blade {
                let strategy = backend.resolve_blade_formatting_strategy();
                backend.format_blade_content(
                    &strategy,
                    &file_path,
                    &content,
                    &blade_options,
                    &backend.shutdown_flag,
                )
            } else {
                let strategy = backend.resolve_formatting_strategy();
                let cancelled = &backend.shutdown_flag;
                match range {
                    Some(range) => backend
                        .format_range_content(&strategy, &file_path, &content, range, cancelled),
                    None => backend.format_content(&strategy, &file_path, &content, cancelled),
                }
            };
            formatted.map(|formatted| {
                formatted.map(|text| match range {
                    Some(range) => formatting::compute_range_edits(&content, &text, range),
                    None => formatting::compute_edits(&content, &text),
                })
            })
        })
        .await;

        match result {
            Some(Ok(edits)) => Ok(edits),
            Some(Err(e)) => {
                self.log(MessageType::Error, format!("Formatting failed: {}", e))
                    .await;
                Err(tower_lsp_server::jsonrpc::Error {
                    code: tower_lsp_server::jsonrpc::ErrorCode::InternalError,
                    message: format!("Formatting failed: {}", e).into(),
                    data: None,
                })
            }
            None => {
                let msg = "Formatting task panicked".to_string();
                self.log(MessageType::Error, msg.clone()).await;
                Err(tower_lsp_server::jsonrpc::Error {
                    code: tower_lsp_server::jsonrpc::ErrorCode::InternalError,
                    message: msg.into(),
                    data: None,
                })
            }
        }
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        self.on_initialize(params).await
    }

    async fn initialized(&self, _: InitializedParams) {
        self.on_initialized().await
    }

    async fn shutdown(&self) -> Result<()> {
        // Signal background workers (diagnostic, PHPStan, PHPCS) to
        // stop.  The PHPStan/PHPCS poll loops also check this flag,
        // so running child processes are killed within 50ms instead
        // of waiting up to 60 seconds.
        self.shutdown_flag.store(true, Ordering::Release);
        // Wake all workers so they see the flag immediately instead
        // of sleeping until the next edit arrives.
        self.diag.notify.notify_one();
        self.diag.workspace_pull_notify.notify_one();
        self.phpstan_tool.notify.notify_one();
        self.phpcs_tool.notify.notify_one();
        self.phpmd_tool.notify.notify_one();
        self.mago_lint_tool.notify.notify_one();
        self.mago_analyze_tool.notify.notify_one();
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.on_did_open(params).await
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.on_did_change(params).await
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.on_did_close(params).await
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.on_did_save(params).await
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        // Clients re-push settings for reasons of their own, so a
        // notification that carries no file filters or stub directory is
        // not a request to drop the ones already in force, and one that
        // repeats the current settings is not a change.
        let indexing = crate::config::ClientIndexingOptions::from_client_settings(&params.settings);
        let stubs = crate::config::ClientStubOptions::from_client_settings(&params.settings);
        if indexing.is_none() && stubs.is_none() {
            return;
        }
        // Captured before the change, so the reconciliation below can
        // tell which way the inputs moved.
        let previous_inputs = self.discovery_inputs();
        let filters_changed =
            indexing.is_some_and(|options| self.set_client_indexing_options(options));
        let stubs_changed = stubs.is_some_and(|options| self.set_client_stub_options(options));
        if !filters_changed && !stubs_changed {
            return;
        }

        // A newly added extension needs its own watcher, or the client
        // never reports events for those files and the index keeps
        // serving the last full scan. Same follow-up a live
        // `.phpantom.toml` edit performs.
        if filters_changed {
            self.reregister_watched_files_if_changed();
        }

        // The index was built under the settings the user just changed
        // away from: drop what the filters now exclude, walk for what they
        // now admit, and rebuild the stub index if its paths moved.
        self.reconcile_index_for_input_change(&previous_inputs);

        // A narrowed exclude list makes classes resolvable that were
        // missing a moment ago, so the negative cache has to go or the
        // editor keeps showing "class not found" for them.
        self.clear_not_found_caches();

        self.request_diagnostic_refresh().await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.on_did_change_watched_files(params).await
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        self.on_did_change_workspace_folders(params).await
    }

    async fn goto_definition(
        &self,
        params: DefinitionParams,
    ) -> Result<Option<DefinitionResponse>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        self.run_position_request(
            "goto_definition",
            uri,
            position,
            |backend, uri, position| {
                // YAML and XML may name PHP classes under any schema. Resolve
                // fully-qualified class and Class::member tokens before entering
                // the PHP-only symbol-map path below.
                if backend.is_resource_uri(uri) {
                    let locations = backend
                        .get_file_content(uri)
                        .and_then(|content| {
                            crate::util::catch_panic_unwind_safe(
                                "goto_definition",
                                uri,
                                Some(position),
                                || backend.resolve_resource_definition(uri, &content, position),
                            )
                        })
                        .unwrap_or_default();
                    return Ok(wrap_locations(locations).map(DefinitionResponse::Definition));
                }

                // A component tag is HTML, so it has no position in the virtual
                // PHP `handle_with_position` would swap in below; it is resolved
                // from the template's own source instead.
                if backend.is_blade_file(uri)
                    && let Some(location) = crate::util::catch_panic_unwind_safe(
                        "goto_definition",
                        uri,
                        Some(position),
                        || backend.blade_component_tag_definition(uri, position),
                    )
                    .flatten()
                {
                    return Ok(Some(DefinitionResponse::Definition(Definition::Location(
                        location,
                    ))));
                }
                // For Blade files, check if the cursor is on an echo delimiter
                // (`{{`/`}}` or `{!!`/`!!}`) first, so go-to-definition agrees
                // with hover on the same position (the implicit `e()` call, or
                // nothing for a raw echo) instead of falling through to the
                // virtual PHP content, where the position maps to whichever
                // expression happens to start at that offset.
                if backend.is_blade_file(uri)
                    && let Some(delimiter_result) =
                        backend.blade_echo_delimiter_definition(uri, position)
                {
                    return Ok(delimiter_result.map(|location| {
                        DefinitionResponse::Definition(Definition::Location(location))
                    }));
                }
                backend.handle_with_position("goto_definition", uri, position, |content, pos| {
                    let locs = backend
                        .resolve_definition(uri, content, pos)
                        .into_iter()
                        .map(|l| backend.translate_location(l))
                        .collect();
                    wrap_locations(locs).map(DefinitionResponse::Definition)
                })
            },
        )
        .await
    }

    async fn goto_implementation(
        &self,
        params: ImplementationParams,
    ) -> Result<Option<ImplementationResponse>> {
        if self.is_foreign_document(&params.text_document_position_params.text_document.uri) {
            return Ok(None);
        }
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;
        self.run_with_progress(
            "goto_implementation",
            "Go to Implementation",
            "Resolving…",
            params.work_done_progress_params.work_done_token,
            move |backend| {
                backend.handle_with_position(
                    "goto_implementation",
                    &uri,
                    position,
                    |content, pos| {
                        backend
                            .resolve_implementation(&uri, content, pos)
                            .map(|locs| {
                                locs.into_iter()
                                    .map(|l| backend.translate_location(l))
                                    .collect()
                            })
                            .and_then(wrap_locations)
                            .map(ImplementationResponse::Definition)
                    },
                )
            },
        )
        .await
        .unwrap_or(Ok(None))
    }

    async fn goto_type_definition(
        &self,
        params: TypeDefinitionParams,
    ) -> Result<Option<TypeDefinitionResponse>> {
        if self.is_foreign_document(&params.text_document_position_params.text_document.uri) {
            return Ok(None);
        }
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        self.run_position_request(
            "goto_type_definition",
            uri,
            position,
            |backend, uri, position| {
                backend.handle_with_position(
                    "goto_type_definition",
                    uri,
                    position,
                    |content, pos| {
                        backend
                            .resolve_type_definition(uri, content, pos)
                            .map(|locs| {
                                locs.into_iter()
                                    .map(|l| backend.translate_location(l))
                                    .collect()
                            })
                            .and_then(wrap_locations)
                            .map(TypeDefinitionResponse::Definition)
                    },
                )
            },
        )
        .await
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        self.run_position_request("hover", uri, position, |backend, uri, position| {
            // For Blade files, check if the cursor is on an echo delimiter
            // (`{{`/`}}` or `{!!`/`!!}`). If so, return hover for `e()`
            // (escaped echo) or a raw-echo explanation, rather than falling
            // through to the virtual PHP content where the position maps into
            // boilerplate or the expression behind the delimiter.
            if backend.is_blade_file(uri)
                && let Some(hover) = backend.blade_echo_delimiter_hover(uri, position)
            {
                return Ok(Some(hover));
            }

            backend.handle_with_position("hover", uri, position, |content, pos| {
                let mut hover = backend.handle_hover(uri, content, pos)?;
                if backend.is_blade_file(uri)
                    && let Some(range) = &mut hover.range
                {
                    *range = backend.translate_blade_range(uri, *range);
                }
                Some(hover)
            })
        })
        .await
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let started = std::time::Instant::now();

        // Run the (CPU-bound) resolution on a blocking thread so it does not
        // monopolize an async worker.  Editors fire a large request barrage on
        // every keystroke (completion, a resolve per item, diagnostics, code
        // lens, …); keeping completion off the async runtime lets those — and
        // the cancellations that supersede stale completions — make progress
        // instead of queueing behind a synchronous resolution.
        let backend = self.clone_for_blocking();
        let result =
            run_blocking_cancel_safe("completion", move || backend.handle_completion(params))
                .await
                .unwrap_or(Ok(None));

        let elapsed = started.elapsed();
        let item_count = match &result {
            Ok(Some(CompletionResponse::CompletionItemList(items))) => items.len(),
            Ok(Some(CompletionResponse::CompletionList(list))) => list.items.len(),
            _ => 0,
        };
        tracing::debug!(
            target: "performance",
            "PHPantom: completion took {:?}, returned {} items",
            elapsed,
            item_count
        );

        result
    }

    async fn completion_resolve(&self, params: CompletionItem) -> Result<CompletionItem> {
        // Offloaded to a blocking thread for the same reason as `completion`:
        // an editor resolves every visible item, so a dozen of these land per
        // keystroke and must not tie up async workers.
        let backend = self.clone_for_blocking();
        let fallback = params.clone();
        let item = run_blocking_cancel_safe("completion_resolve", move || {
            backend.handle_completion_resolve(params)
        })
        .await
        .unwrap_or(fallback);
        Ok(item)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;
        let include_declaration = params.context.include_declaration;
        self.run_with_progress(
            "find_references",
            "Find References",
            "Scanning…",
            params.work_done_progress_params.work_done_token,
            move |backend| {
                // Ahead of reading the file: a refresh that parses new files can
                // rewrite a template's virtual PHP (see `Backend::find_references`).
                backend.ensure_workspace_indexed_for_request();
                backend.handle_with_position("references", &uri, position, |content, pos| {
                    backend
                        .find_references(&uri, content, pos, include_declaration)
                        .map(|locs| {
                            locs.into_iter()
                                .filter_map(|l| backend.try_translate_location(l))
                                .collect()
                        })
                })
            },
        )
        .await
        .unwrap_or(Ok(None))
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<Vec<CodeActionResponse>>> {
        let uri = params.text_document.uri.to_string();

        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        run_blocking_cancel_safe("code_action", move || {
            backend.handle_with_uri("code_action", &uri_clone, |content| {
                let actions = backend.handle_code_action(&uri_clone, content, &params);
                if actions.is_empty() {
                    None
                } else {
                    Some(actions)
                }
            })
        })
        .await
        .unwrap_or(Ok(None))
    }

    async fn code_action_resolve(&self, action: CodeAction) -> Result<CodeAction> {
        // Resolving an action parses the file and walks the AST several times
        // (scope map, return analysis, return type), and the editor blocks its
        // UI on the reply, so the work belongs off the request task.
        let backend = self.clone_for_blocking();
        let fallback = action.clone();
        let (resolved, republish_uri) =
            run_blocking_cancel_safe("code_action_resolve", move || {
                backend.resolve_code_action(action)
            })
            .await
            .unwrap_or((fallback, None));

        // If a PHPStan quickfix was resolved, reassemble diagnostics so the
        // cleared diagnostic disappears immediately. In pull mode nothing is
        // pushed, so ask the editor to re-pull the freshly cached set.
        if let Some(uri_str) = republish_uri {
            self.assemble_and_refresh(&uri_str).await;
        }

        Ok(resolved)
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        if self.is_foreign_document(&params.text_document_position_params.text_document.uri) {
            return Ok(None);
        }
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        self.run_position_request("signature_help", uri, position, |backend, uri, position| {
            backend.handle_with_position("signature_help", uri, position, |content, pos| {
                backend.handle_signature_help(uri, content, pos)
            })
        })
        .await
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        if self.is_foreign_document(&params.text_document_position_params.text_document.uri) {
            return Ok(None);
        }
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        self.run_position_request(
            "document_highlight",
            uri,
            position,
            |backend, uri, position| {
                backend.handle_with_position("document_highlight", uri, position, |content, pos| {
                    backend
                        .handle_document_highlight(uri, content, pos)
                        .map(|highlights| {
                            highlights
                                .into_iter()
                                .filter_map(|mut h| {
                                    h.range = backend.try_translate_blade_range(uri, h.range)?;
                                    Some(h)
                                })
                                .collect()
                        })
                })
            },
        )
        .await
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResult>> {
        let uri = params.text_document.uri.to_string();
        let position = params.position;

        self.run_position_request("prepare_rename", uri, position, |backend, uri, position| {
            backend.handle_with_position("prepare_rename", uri, position, |content, pos| {
                backend
                    .handle_prepare_rename(uri, content, pos)
                    .and_then(|res| match res {
                        PrepareRenameResult::Range(r) => backend
                            .try_translate_blade_range(uri, r)
                            .map(PrepareRenameResult::Range),
                        PrepareRenameResult::PrepareRenamePlaceholder(p) => backend
                            .try_translate_blade_range(uri, p.range)
                            .map(|range| {
                                PrepareRenameResult::PrepareRenamePlaceholder(
                                    PrepareRenamePlaceholder {
                                        range,
                                        placeholder: p.placeholder,
                                    },
                                )
                            }),
                        default @ PrepareRenameResult::PrepareRenameDefaultBehavior(_) => {
                            Some(default)
                        }
                    })
            })
        })
        .await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;
        let new_name = params.new_name.clone();

        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        let outcome = run_blocking_cancel_safe("rename", move || {
            // The initial index can rewrite a template's virtual PHP, so
            // wait for it before reading the file. A local variable is
            // file-scoped and does not pay the refresh that discovers a
            // file the watcher missed. A cross-file rename still does, and
            // that refresh can rewrite a template, so the rename itself is
            // read after it.
            backend.ensure_workspace_index_ready_for_request();
            let needs_refresh = backend
                .handle_with_position("rename", &uri_clone, position, |content, pos| {
                    Some(backend.rename_needs_workspace_refresh(&uri_clone, content, pos))
                })
                .ok()
                .flatten()
                .unwrap_or(false);
            if needs_refresh {
                backend.ensure_workspace_indexed_for_request();
            }
            backend.handle_with_position("rename", &uri_clone, position, |content, pos| {
                Some(backend.handle_rename(&uri_clone, content, pos, &new_name))
            })
        })
        .await
        .unwrap_or(Ok(None))?;

        // A refusal carries a reason the user has to see (the move's
        // destination is taken), and an error response is the only part
        // of the rename protocol an editor shows them.
        match outcome {
            Some(Err(message)) => {
                self.log(MessageType::Warning, message.clone()).await;
                Err(tower_lsp_server::jsonrpc::Error {
                    code: tower_lsp_server::jsonrpc::ErrorCode::InvalidRequest,
                    message: message.into(),
                    data: None,
                })
            }
            Some(Ok(edit)) => Ok(edit),
            None => Ok(None),
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        if self.is_foreign_document(&params.text_document.uri) {
            return Ok(None);
        }
        let uri = params.text_document.uri.to_string();
        let backend = self.clone_for_blocking();
        let u = uri.clone();
        self.coalesced_whole_file("document_symbol", &uri, move || {
            backend.handle_with_uri("document_symbol", &u, |content| {
                backend.handle_document_symbol(&u, content)
            })
        })
        .await
    }

    #[allow(deprecated)] // SymbolInformation::deprecated is deprecated in the LSP types crate
    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<WorkspaceSymbolResponse>> {
        // The query is matched against every symbol of every parsed file, so
        // this walks the whole workspace index.
        let backend = self.clone_for_blocking();
        Ok(run_blocking_cancel_safe("symbol", move || {
            backend.handle_workspace_symbol(&params.query)
        })
        .await
        .flatten()
        .map(WorkspaceSymbolResponse::SymbolInformationList))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        if self.is_foreign_document(&params.text_document.uri) {
            return Ok(None);
        }
        let uri = params.text_document.uri.to_string();
        let backend = self.clone_for_blocking();
        let u = uri.clone();
        self.coalesced_whole_file("folding_range", &uri, move || {
            backend.handle_with_uri("folding_range", &u, |content| {
                backend.handle_folding_range(&u, content)
            })
        })
        .await
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        if self.is_foreign_document(&params.text_document.uri) {
            return Ok(None);
        }
        let uri = params.text_document.uri.to_string();
        let backend = self.clone_for_blocking();
        let u = uri.clone();
        let lenses = self
            .coalesced_whole_file("code_lens", &uri, move || {
                backend.handle_with_uri("code_lens", &u, |content| {
                    backend.handle_code_lens(&u, content)
                })
            })
            .await;
        self.schedule_member_ref_counts();
        lenses
    }

    async fn code_lens_resolve(&self, params: CodeLens) -> Result<CodeLens> {
        let fallback = params.clone();
        let backend = self.clone_for_blocking();
        Ok(run_blocking_cancel_safe("code_lens_resolve", move || {
            backend.resolve_code_lens_item(params)
        })
        .await
        .unwrap_or(fallback))
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        if params.command == "phpantom.navigateToPrototype"
            && let Some([uri_val, pos_val]) = params.arguments.as_deref()
            && let Ok(uri) = serde_json::from_value::<Uri>(uri_val.clone())
            && let Ok(position) = serde_json::from_value::<Position>(pos_val.clone())
            && let Some(ref client) = self.client
        {
            // Detached rather than awaited here: `showDocument` is a
            // server-to-client request, and tower-lsp aborts this handler
            // future when the client cancels the command or sends `exit`.
            // Dropping the request future mid-flight leaves its response
            // channel registered, and the answer arriving afterwards
            // panics the serve loop, killing the whole server (the same
            // failure `request_diagnostic_refresh` avoids with its pump).
            // Nothing here needs the result.
            let client = client.clone();
            tokio::spawn(async move {
                let _ = client
                    .show_document(ShowDocumentParams {
                        uri,
                        external: Some(false),
                        take_focus: Some(true),
                        selection: Some(Range {
                            start: position,
                            end: position,
                        }),
                    })
                    .await;
            });
        }
        Ok(None)
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        if self.is_foreign_document(&params.text_document.uri) {
            return Ok(None);
        }
        let uri = params.text_document.uri.to_string();
        let backend = self.clone_for_blocking();
        let u = uri.clone();
        self.coalesced_whole_file("document_link", &uri, move || {
            backend.handle_with_uri("document_link", &u, |content| {
                backend.handle_document_link(&u, content)
            })
        })
        .await
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        if self.is_foreign_document(&params.text_document.uri) {
            return Ok(None);
        }
        let uri = params.text_document.uri.to_string();
        let positions = params.positions;
        // Each request re-parses the whole file. Not coalesced like the other
        // whole-file requests: the answer depends on `positions`, so handing
        // back a superseded request's ranges would expand the wrong selection.
        let backend = self.clone_for_blocking();
        let u = uri.clone();
        run_blocking_cancel_safe("selection_range", move || {
            backend.handle_with_uri("selection_range", &u, |content| {
                backend.handle_selection_range(content, &positions)
            })
        })
        .await
        .unwrap_or(Ok(None))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokens>> {
        if self.is_foreign_document(&params.text_document.uri) {
            return Ok(None);
        }
        let uri = params.text_document.uri.to_string();
        // Highlighting is requested on every keystroke, re-serializes the whole
        // token array, and is one of the most expensive whole-file requests.
        // Coalesce it so a typing burst cannot pile up scans that saturate the
        // CPU and stall completion (see `coalesced_whole_file`).
        let backend = self.clone_for_blocking();
        let u = uri.clone();
        self.coalesced_whole_file("semantic_tokens_full", &uri, move || {
            backend.handle_with_uri("semantic_tokens_full", &u, |content| {
                backend.handle_semantic_tokens_full(&u, content)
            })
        })
        .await
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        if self.is_foreign_document(&params.text_document.uri) {
            return Ok(None);
        }
        self.inlay_hint_request(params).await
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        if self.is_foreign_document(&params.text_document_position_params.text_document.uri) {
            return Ok(None);
        }
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        self.run_position_request(
            "prepare_call_hierarchy",
            uri,
            position,
            |backend, uri, position| {
                backend.handle_with_position(
                    "prepare_call_hierarchy",
                    uri,
                    position,
                    |content, translated_position| {
                        backend.prepare_call_hierarchy_impl(uri, content, translated_position)
                    },
                )
            },
        )
        .await
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let backend = self.clone_for_blocking();
        Ok(run_blocking_cancel_safe("incoming_calls", move || {
            // Ahead of reading the file: a refresh that parses new files can
            // rewrite a template's virtual PHP (see `Backend::find_references`).
            backend.ensure_workspace_indexed_for_request();
            backend.incoming_calls_impl(&params.item)
        })
        .await
        .flatten())
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let backend = self.clone_for_blocking();
        Ok(run_blocking_cancel_safe("outgoing_calls", move || {
            backend.outgoing_calls_impl(&params.item)
        })
        .await
        .flatten())
    }

    async fn prepare_type_hierarchy(
        &self,
        params: TypeHierarchyPrepareParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        if self.is_foreign_document(&params.text_document_position_params.text_document.uri) {
            return Ok(None);
        }
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        self.run_position_request(
            "prepare_type_hierarchy",
            uri,
            position,
            |backend, uri, position| {
                backend.handle_with_position(
                    "prepare_type_hierarchy",
                    uri,
                    position,
                    |content, pos| {
                        backend
                            .prepare_type_hierarchy_impl(uri, content, pos)
                            .map(|items| {
                                items
                                    .into_iter()
                                    .map(|mut item| {
                                        item.range = backend.translate_blade_range(uri, item.range);
                                        item.selection_range = backend
                                            .translate_blade_range(uri, item.selection_range);
                                        item
                                    })
                                    .collect()
                            })
                    },
                )
            },
        )
        .await
    }

    async fn supertypes(
        &self,
        params: TypeHierarchySupertypesParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        // Walking to the parents loads each one, which can lazily parse files
        // that are not indexed yet.
        let backend = self.clone_for_blocking();
        Ok(
            run_blocking_cancel_safe("supertypes", move || backend.supertypes_impl(&params.item))
                .await
                .flatten(),
        )
    }

    async fn subtypes(
        &self,
        params: TypeHierarchySubtypesParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        let item = params.item;
        Ok(self
            .run_with_progress(
                "type_hierarchy_subtypes",
                "Type Hierarchy",
                "Scanning…",
                params.work_done_progress_params.work_done_token,
                move |backend| backend.subtypes_impl(&item),
            )
            .await
            .flatten())
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        // Only handle Enter ("\n") for PHPDoc block generation.
        if params.ch != "\n" || self.is_foreign_document(&params.text_document.uri) {
            return Ok(None);
        }

        let uri = params.text_document.uri.to_string();
        let position = params.position;

        let content = match self.get_file_content(&uri) {
            Some(c) => c,
            None => return Ok(None),
        };

        // This fires on every Enter, and generating the block resolves the
        // documented signature's types, so it stays off the request task.
        let backend = self.clone_for_blocking();
        let u = uri.clone();
        Ok(run_blocking_cancel_safe("on_type_formatting", move || {
            let offset = crate::text_position::position_to_offset(&content, position);
            let ctx = backend.file_context_at(&u, offset);
            let class_loader = backend.class_loader(&ctx);
            let resolved_class_loader = backend.resolved_class_loader(&ctx);
            let function_loader = backend.function_loader(&ctx);

            crate::completion::phpdoc::generation::try_generate_docblock_on_enter(
                &content,
                position,
                &ctx.use_map,
                &ctx.namespace,
                &ctx.classes,
                &class_loader,
                &resolved_class_loader,
                Some(&backend),
                Some(&function_loader),
                &u,
            )
        })
        .await
        .flatten())
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        self.format_document(&params.text_document.uri, &params.options, None)
            .await
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        self.format_document(
            &params.text_document.uri,
            &params.options,
            Some(params.range),
        )
        .await
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReport> {
        self.document_pull_diagnostic(params)
    }

    async fn workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReport> {
        self.workspace_pull_diagnostic(params)
    }
}

pub(crate) fn type_hierarchy_registration() -> Registration {
    Registration {
        id: "type-hierarchy".to_string(),
        method: "textDocument/prepareTypeHierarchy".to_string(),
        register_options: Some(
            serde_json::to_value(TypeHierarchyRegistrationOptions {
                text_document_registration_options: TextDocumentRegistrationOptions {
                    document_selector: Some(vec![DocumentFilter::TextDocumentFilter(
                        TextDocumentFilter::Language(TextDocumentFilterLanguage {
                            language: "php".to_string(),
                            scheme: None,
                            pattern: None,
                        }),
                    )]),
                },
                type_hierarchy_options: TypeHierarchyOptions::default(),
                static_registration_options: StaticRegistrationOptions::default(),
            })
            .expect("type hierarchy registration options serialize"),
        ),
    }
}

/// Convert a `Vec<Location>` into a `Definition`, the payload shared by
/// the definition, implementation and type-definition responses.
///
/// Returns a single `Location` for one location, a list for several, and
/// `None` for an empty vec.
fn wrap_locations(mut locations: Vec<Location>) -> Option<Definition> {
    match locations.len() {
        0 => None,
        1 => locations.pop().map(Definition::Location),
        _ => Some(Definition::LocationList(locations)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_hierarchy_registration_includes_php_document_selector() {
        let registration = type_hierarchy_registration();

        assert_eq!(registration.id, "type-hierarchy");
        assert_eq!(registration.method, "textDocument/prepareTypeHierarchy");

        let options = registration
            .register_options
            .expect("type hierarchy registration should include options");
        assert_eq!(options["documentSelector"][0]["language"], "php");
        assert!(options["documentSelector"][0].get("scheme").is_none());
        assert!(options["documentSelector"][0].get("pattern").is_none());
    }
}
