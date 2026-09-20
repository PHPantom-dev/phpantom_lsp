/// LSP server trait implementation.
///
/// This module contains the `impl LanguageServer for Backend` block,
/// which handles all LSP protocol messages (initialize, didOpen, didChange,
/// didClose, completion, diagnostic, etc.).
///
/// **Diagnostic delivery.** Two native delivery models are supported and are
/// selected automatically from the client's capabilities. The server treats
/// pull diagnostics as the preferred modern path and uses push only as a
/// fallback for older clients; it deliberately does not send the same native
/// diagnostics through both channels for the same client.
///
/// - **Pull model** (preferred) — when the client advertises
///   `textDocument.diagnostic` support, the server registers a
///   `diagnostic_provider` capability.  The editor requests diagnostics
///   via `textDocument/diagnostic` for visible files and
///   `workspace/diagnostic` for all open files.  Cross-file invalidation
///   (e.g. a class signature change) sends `workspace/diagnostic/refresh`
///   so the editor re-pulls only the files it cares about.  The
///   background workspace pass over unopened files is deferred until the
///   client's first `workspace/diagnostic` request, since its results
///   are only deliverable through workspace pull responses.
///
/// - **Push model** (fallback) — for clients without pull support, the
///   server pushes diagnostics via `textDocument/publishDiagnostics`
///   from a debounced background worker.  Each `did_change` bumps a
///   version counter; the worker waits for a quiet period before
///   publishing.
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::LanguageServer;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::request::{
    GotoImplementationParams, GotoImplementationResponse, GotoTypeDefinitionParams,
    GotoTypeDefinitionResponse,
};
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::composer;
use crate::config::IndexingStrategy;
use crate::formatting;

/// Run `f` on a blocking thread in a way that survives `$/cancelRequest`.
///
/// tower-lsp 0.20 wedges its serve loop if a request handler future is
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
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let workspace_root = params
            .root_uri
            .as_ref()
            .and_then(|uri| uri.to_file_path().ok());

        if let Some(root) = workspace_root {
            *self.workspace.workspace_root.write() = Some(root);
        }

        // Store the client name for quirks-mode adjustments.
        if let Some(info) = &params.client_info {
            *self.client_name.lock() = info.name.clone();
        }

        // File filters the editor forwarded from its own settings. Read
        // before anything scans, so the very first discovery pass already
        // honours them rather than indexing excluded trees and dropping
        // them later.
        if let Some(options) = params
            .initialization_options
            .as_ref()
            .and_then(crate::config::ClientIndexingOptions::from_client_settings)
        {
            self.set_client_indexing_options(options);
        }

        let client_supports_pull = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|td| td.diagnostic.as_ref())
            .is_some();
        self.supports_pull_diagnostics
            .store(client_supports_pull, Ordering::Release);

        // Detect which resource operations the client accepts in workspace
        // edits: the rename handler includes a `RenameFile` operation when a
        // class rename matches PSR-4 naming, and the code actions that
        // create a file (extract interface, create a missing view) are only
        // offered when `CreateFile` is accepted.
        let resource_operations = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.workspace_edit.as_ref())
            .and_then(|we| we.resource_operations.as_deref())
            .unwrap_or_default();
        self.supports_file_rename.store(
            resource_operations.contains(&ResourceOperationKind::Rename),
            Ordering::Release,
        );
        self.supports_file_create.store(
            resource_operations.contains(&ResourceOperationKind::Create),
            Ordering::Release,
        );

        // Detect whether the client supports server-initiated work-done
        // progress (window/workDoneProgress/create).  Per the LSP spec,
        // we must not send that request unless the client opts in.
        let client_supports_work_done_progress = params
            .capabilities
            .window
            .as_ref()
            .and_then(|w| w.work_done_progress)
            .unwrap_or(false);
        self.supports_work_done_progress
            .store(client_supports_work_done_progress, Ordering::Release);

        // Detect whether the client handles `window/showDocument`.  Code
        // lens navigation routes through that request, so a client that
        // does not opt in needs a lens command it can act on itself.
        let client_supports_show_document = params
            .capabilities
            .window
            .as_ref()
            .and_then(|w| w.show_document.as_ref())
            .is_some_and(|sd| sd.support);
        self.supports_show_document
            .store(client_supports_show_document, Ordering::Release);

        // Detect whether the client supports server-initiated semantic
        // token refreshes (`workspace/semanticTokens/refresh`).  Used to
        // re-pull tokens after background didChange parses commit a new
        // symbol map.
        let client_supports_semantic_tokens_refresh = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.semantic_tokens.as_ref())
            .and_then(|st| st.refresh_support)
            .unwrap_or(false);
        self.supports_semantic_tokens_refresh
            .store(client_supports_semantic_tokens_refresh, Ordering::Release);

        let client_supports_code_lens_refresh = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.code_lens.as_ref())
            .and_then(|code_lens| code_lens.refresh_support)
            .unwrap_or(false);
        self.supports_code_lens_refresh
            .store(client_supports_code_lens_refresh, Ordering::Release);

        // Reference counts on declarations are computed off the request
        // path, so the hints an editor holds are the ones from before the
        // counts landed unless it can be asked to re-pull them.
        let client_supports_inlay_hint_refresh = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.inlay_hint.as_ref())
            .and_then(|ih| ih.refresh_support)
            .unwrap_or(false);
        self.supports_inlay_hint_refresh
            .store(client_supports_inlay_hint_refresh, Ordering::Release);

        let client_supports_type_hierarchy_dynamic_registration = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|td| td.type_hierarchy.as_ref())
            .and_then(|th| th.dynamic_registration)
            .unwrap_or(false);
        self.supports_type_hierarchy_dynamic_registration.store(
            client_supports_type_hierarchy_dynamic_registration,
            Ordering::Release,
        );

        Ok(InitializeResult {
            offset_encoding: None,
            capabilities: ServerCapabilities {
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![",".to_string(), ")".to_string()]),
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: None,
                    },
                }),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(true),
                    trigger_characters: Some(vec![
                        "$".to_string(),
                        ">".to_string(),
                        ":".to_string(),
                        "@".to_string(),
                        "'".to_string(),
                        "\"".to_string(),
                        "[".to_string(),
                        "\\".to_string(),
                        "/".to_string(),
                        "*".to_string(),
                    ]),
                    all_commit_characters: None,
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: None,
                    },
                    completion_item: None,
                }),
                inlay_hint_provider: Some(OneOf::Left(true)),
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        will_save: None,
                        will_save_wait_until: None,
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(false),
                        })),
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
                references_provider: Some(OneOf::Left(true)),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR_EXTRACT,
                            CodeActionKind::REFACTOR_INLINE,
                            CodeActionKind::new("source.organizeImports"),
                        ]),
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: None,
                        },
                        resolve_provider: Some(true),
                    },
                )),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: None,
                    },
                })),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(true),
                }),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                    first_trigger_character: "\n".to_string(),
                    more_trigger_character: None,
                }),
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: None,
                    },
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions {
                                work_done_progress: None,
                            },
                            legend: crate::semantic_tokens::legend(),
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                diagnostic_provider: if client_supports_pull {
                    Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
                        identifier: Some("phpantom".to_string()),
                        inter_file_dependencies: true,
                        // The workspace/diagnostic handler reports both
                        // per-open-file results and the background
                        // workspace diagnostics computed for files the
                        // user has not opened.
                        workspace_diagnostics: true,
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: None,
                        },
                    }))
                } else {
                    None
                },
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["phpantom.navigateToPrototype".to_string()],
                    ..ExecuteCommandOptions::default()
                }),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: self.name.clone(),
                version: Some(self.version.clone()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let workspace_root = self.workspace.workspace_root.read().clone();

        if let Some(root) = workspace_root {
            // ── Load project configuration ──────────────────────────────
            // Read `.phpantom.toml` before anything else so that settings
            // (e.g. PHP version override, diagnostic toggles) are active
            // from the very first file load.
            match crate::config::load_config_from(
                &root,
                self.workspace.global_config_path.as_deref(),
            ) {
                Ok(cfg) => {
                    self.set_config(cfg);
                }
                Err(e) => {
                    self.log(
                        MessageType::WARNING,
                        format!("Failed to load .phpantom.toml: {}", e),
                    )
                    .await;
                }
            }

            // Parse composer.json once up front.  The result is used for
            // PHP version detection and passed into init_single_project
            // so the file is never re-read during startup.
            let composer_package = composer::read_composer_package(&root);

            // Detect the target PHP version.  The config file override
            // takes precedence; otherwise fall back to composer.json.
            let php_version = self
                .config()
                .php
                .version
                .as_deref()
                .and_then(crate::types::PhpVersion::from_composer_constraint)
                .unwrap_or_else(|| {
                    composer_package
                        .as_ref()
                        .and_then(composer::detect_php_version_from_package)
                        .unwrap_or_default()
                });
            self.set_php_version(php_version);

            // ── Create a progress token for indexing feedback ────────
            // The heavy scans below run synchronously, so per-file
            // progress is written to a shared `ScanProgress` state and
            // flushed to the client by a background poller task.
            let progress_token = self.progress_create("phpantom/indexing").await;
            if let Some(ref tok) = progress_token {
                self.progress_begin(tok, "PHPantom: Indexing", Some("Starting".to_string()))
                    .await;
            }
            let progress = crate::progress::ScanProgress::new();
            let poller = progress_token
                .as_ref()
                .map(|tok| self.spawn_progress_poller(tok.clone(), Arc::clone(&progress)));

            self.discover_workspace_symbols(&root, php_version, composer_package, Some(&progress))
                .await;

            // Laravel-only startup work.  The project classification is
            // set by the init pass above from composer.json, so it has to
            // run after it: a Symfony workspace must never pay for the
            // whole-tree migration walk, let alone hang in it.
            if self.resolved_class_cache.read().is_laravel() {
                // The macro index must be built before the schema index:
                // `load_schema_index` takes the project's `Blueprint` macro
                // closures so a migration calling a custom column helper
                // (`$table->money(...)`, registered via `Blueprint::macro()`)
                // contributes its columns.  Building it here (rather than
                // later, alongside the other Laravel indexes) means the
                // first schema load already sees the populated macro map
                // instead of an empty one.
                let macro_backend = self.clone_for_blocking();
                run_blocking_cancel_safe("build_laravel_macro_index", move || {
                    macro_backend.build_laravel_macro_index()
                })
                .await;

                let laravel_config = self.config().laravel;
                if laravel_config.schema.enabled() || laravel_config.migrations.enabled() {
                    let bp_macros = self.laravel_macros.read().blueprint_macro_closures();
                    let schema_root = root.clone();
                    let schema_config = laravel_config.clone();
                    let loaded = run_blocking_cancel_safe("load_schema_index", move || {
                        crate::virtual_members::laravel::database_schema::load_schema_index(
                            &schema_root,
                            &schema_config,
                            &bp_macros,
                        )
                    })
                    .await;
                    match loaded {
                        Some(Ok(index)) => {
                            self.resolved_class_cache
                                .write()
                                .set_schema_index(index.clone());
                            *self.schema_index.write() = index;
                        }
                        Some(Err(e)) => {
                            self.log(
                                MessageType::WARNING,
                                format!("Failed to load Laravel schema dumps: {}", e),
                            )
                            .await;
                        }
                        None => {}
                    }
                }

                // Warm the Eloquent Builder resolution cache; a non-Laravel
                // workspace has nothing to warm.
                progress.set_percentage(90, "Warming Laravel completions");
                let warm_backend = self.clone_for_blocking();
                let warmed = run_blocking_cancel_safe("warm_laravel_completion_cache", move || {
                    warm_backend.warm_laravel_completion_cache()
                })
                .await
                .unwrap_or(0);
                if warmed > 0 {
                    tracing::info!("PHPantom: warmed {} Laravel completion classes", warmed);
                }
            }

            if let Some(poller) = poller {
                poller.finish().await;
            }
            if let Some(ref tok) = progress_token {
                let classmap_count = self.symbols.fqn_uri_index.read().len();
                self.progress_end(tok, Some(format!("Indexed {} classes", classmap_count)))
                    .await;
            }
        } else {
            self.log(MessageType::INFO, "PHPantom initialized!".to_string())
                .await;
        }

        self.start_full_background_index().await;

        // Spawn the background diagnostic worker. We build a shallow
        // clone of `self` that shares every `Arc`-wrapped field (maps,
        // caches, the diagnostic notify/pending slot) so the worker
        // sees all mutations the real Backend makes.  Non-Arc fields
        // (php_version, vendor_uri_prefixes, vendor_dir_paths) are
        // snapshotted — they are only written during init (above) and
        // never change afterwards.
        let worker_backend = self.clone_for_diagnostic_worker();
        tokio::spawn(async move {
            worker_backend.diagnostic_worker().await;
        });

        // Spawn the PHPStan worker as a separate background task.
        // PHPStan is extremely slow and resource-intensive, so it runs
        // in its own task with its own debounce timer and pending-URI
        // slot.  At most one PHPStan process runs at a time.  Native
        // diagnostics (fast + slow phases) are never blocked.
        let phpstan_backend = self.clone_for_diagnostic_worker();
        tokio::spawn(async move {
            phpstan_backend.phpstan_worker().await;
        });

        // Spawn the PHPCS worker as a separate background task.
        // Same pattern as the PHPStan worker: dedicated task, own
        // debounce timer, single pending-URI slot.
        let phpcs_backend = self.clone_for_diagnostic_worker();
        tokio::spawn(async move {
            phpcs_backend.phpcs_worker().await;
        });

        // Spawn the Mago lint worker.  Same pattern as PHPCS: dedicated
        // task, own debounce timer, single pending-URI slot.  Mago lint
        // is fast (AST-level rules) so it uses the same debounce as PHPCS.
        let mago_lint_backend = self.clone_for_diagnostic_worker();
        tokio::spawn(async move {
            mago_lint_backend.mago_lint_worker().await;
        });

        // Spawn the Mago analyze worker.  Mago analyze is slower
        // (type-aware) so it follows the PHPStan pattern with a longer
        // debounce.
        let mago_analyze_backend = self.clone_for_diagnostic_worker();
        tokio::spawn(async move {
            mago_analyze_backend.mago_analyze_worker().await;
        });

        // Spawn the global config watcher. Unlike the project's own
        // `.phpantom.toml` (covered by the file watcher registered below),
        // the global config lives outside the workspace and has to be
        // polled directly; see `global_config_watcher` for why.
        if let Some(root) = self.workspace.workspace_root.read().clone() {
            let config_watcher_backend = self.clone_for_diagnostic_worker();
            tokio::spawn(async move {
                config_watcher_backend.global_config_watcher(root).await;
            });
        }

        // ── Dynamic capability registration ─────────────────────────
        // lsp-types 0.94 does not expose a `type_hierarchy_provider`
        // field on `ServerCapabilities`, so we register the capability
        // dynamically via `client/registerCapability` instead.
        let mut registrations = Vec::new();

        if self
            .supports_type_hierarchy_dynamic_registration
            .load(Ordering::Acquire)
        {
            registrations.push(type_hierarchy_registration());
        }

        // Register file watchers for staleness detection.  The client
        // will notify us when PHP files or composer files change on disk
        // (even outside the editor), so we can refresh our indices.
        // `[indexing] extensions` entries get their own watchers so files
        // like Drupal's `.module` refresh the index the way `.php` does.
        // Built by the same helper `reload_config` uses to keep this
        // registration current when the extension list changes mid-session
        // (see `indexing::watch::reregister_watched_files_if_changed`).
        let (watched_files_registration, extra_extensions, is_laravel) =
            self.build_watched_file_registration();
        registrations.push(watched_files_registration);
        *self.registered_watcher_state.write() = Some((extra_extensions, is_laravel));

        if let Some(client) = &self.client {
            let _ = client.register_capability(registrations).await;
        }

        // Clear the negative class-resolution cache.  During startup,
        // `did_open` may have triggered `update_ast` → `find_or_load_class`
        // before the fqn_uri_index was fully populated, caching
        // "not found" for classes that are now resolvable.  Without this
        // clear, those stale entries cause false-positive "Class not found"
        // diagnostics even though hover and go-to-definition (which run
        // later) resolve the same symbols correctly.
        self.clear_class_not_found_cache();

        // Clear the resolved-class cache for the same reason.  A request
        // that arrives while indexing is still in progress (the editor
        // fires hover, completion, semantic-tokens, and inlay-hint
        // requests the moment a file opens) resolves classes against an
        // incomplete index.  When a class's parent, trait, or interface
        // is a vendor type not yet in `fqn_uri_index`, the inheritance
        // merge silently drops every inherited member and the partial
        // result is cached permanently.  Diagnostics then report
        // false-positive "unknown member" errors for inherited methods
        // (e.g. a controller's framework base-class methods) even though
        // hover — which walks the parent chain live rather than reading
        // the merged cache — resolves them correctly.  Clearing here lets
        // the now-complete index rebuild every merge correctly.
        self.resolved_class_cache.write().clear();
        self.auth_user_type_cache.write().clear();
        *self.storage_disk_type_cache.write() = None;
        *self.laravel_aliases.write() = None;

        // Scan project source for the remaining Laravel indexes (macros
        // were already scanned above, before the schema index load, so
        // `Blueprint` macro columns are present from the first load).
        if self.resolved_class_cache.read().is_laravel() {
            // Each of these parses the project's provider and Blade files, so
            // they run on the blocking pool: `initialized` is a notification
            // and tower-lsp cannot dispatch anything else while it runs.
            let index_backend = self.clone_for_blocking();
            let discovered = run_blocking_cancel_safe("build_laravel_indexes", move || {
                index_backend.build_laravel_date_class();
                index_backend.build_provider_resources();
                index_backend.build_laravel_command_index();
                index_backend.build_laravel_morph_map_index();
                index_backend.build_laravel_gate_index();

                // Build the Blade index now that the view roots and component
                // namespaces providers register are known, so the first
                // view-name completion in a template does not pay for the walk.
                let discovery = index_backend.blade_discovery();
                (
                    discovery.views.len(),
                    discovery.components.len(),
                    discovery.livewire.len(),
                )
            })
            .await;
            if let Some((views, components, livewire)) = discovered {
                tracing::info!(
                    "PHPantom: discovered {} Blade templates, {} component classes, {} Livewire components",
                    views,
                    components,
                    livewire,
                );
            }
        }

        // Mark initialization as complete so that diagnostic workers
        // and pull handlers know the project is fully indexed.
        self.init_complete
            .store(true, std::sync::atomic::Ordering::Release);

        // Files opened during startup (before indexing finished) were
        // not diagnosed because `schedule_diagnostics` skips work when
        // `init_complete` is false. Queue that catch-up work after
        // `initialized` returns so early completion requests are not
        // stuck behind diagnostics for the active file.
        let diagnostics_backend = self.clone_for_diagnostic_worker();
        tokio::spawn(async move {
            let file_snapshots: Vec<(String, Arc<String>)> = diagnostics_backend
                .open_files
                .read()
                .iter()
                .map(|(uri, content)| (uri.clone(), Arc::clone(content)))
                .collect();
            // Each file's own refresh (sent from
            // `publish_diagnostics_for_file` once its full set is
            // cached, and only when that set changed) is all the editor
            // needs; a trailing workspace-wide one here would just
            // invalidate every result again.
            for (uri, content) in &file_snapshots {
                diagnostics_backend.schedule_diagnostics(uri.clone());
                diagnostics_backend
                    .publish_diagnostics_for_file(uri, content)
                    .await;
            }
        });
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
        self.mago_lint_tool.notify.notify_one();
        self.mago_analyze_tool.notify.notify_one();
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        let uri = doc.uri.to_string();
        let text = Arc::new(doc.text);

        // Track files opened with languageId "blade" so they get
        // Blade preprocessing even without a .blade.php extension.
        if doc.language_id == "blade" && !crate::blade::is_blade_file(&uri) {
            self.blade_uris.write().insert(uri.clone());
        }

        self.open_files
            .write()
            .insert(uri.clone(), Arc::clone(&text));

        // Resource documents are not PHP source. Build a lightweight symbol
        // map so navigation, references, rename, and PHP declaration lenses
        // all consume the same indexed occurrences.
        if crate::resource_navigation::is_resource_document(&uri) {
            self.update_resource_symbol_index(&uri, &text);
            self.log(MessageType::INFO, format!("Opened resource file: {}", uri))
                .await;
            return;
        }

        // Parse and update AST map, use map, and namespace map
        self.update_ast(&uri, &text);

        // Opening a Blade template is the discrete point where its
        // call-site variable inference runs (update_ast itself only
        // reads the cached set).  On a blocking thread: inference
        // resolves passed-expression types in caller files, which can
        // lazily parse other files.
        if self.is_blade_file(&uri) {
            if self.sync_ast_updates {
                self.reinfer_blade_and_its_renders(&uri, &text);
            } else {
                let backend = self.clone_for_blocking();
                let blade_uri = uri.clone();
                let content = Arc::clone(&text);
                spawn_blocking_detached("did_open blade inference", move || {
                    backend.reinfer_blade_and_its_renders(&blade_uri, &content);
                });
            }
        }

        // Baseline for the first save: without it, that save would have to
        // re-diagnose every open file to be safe.
        self.capture_declaration_baseline(&uri);

        // Schedule diagnostics asynchronously so that the first-open
        // response is not blocked by lazy stub parsing (which can take
        // tens of seconds when many class references trigger cache-miss
        // parses).  This matches the did_change path.
        self.schedule_diagnostics(uri.clone());

        // Opening a file is a discrete event (not a per-keystroke one),
        // and the buffer matches what is on disk, so it is a safe and
        // useful point to run the external tools.  Without this the user
        // would see no PHPStan/PHPCS/Mago diagnostics until the first
        // save.  (During editing they are gated to save only; see
        // `did_save`.)
        self.schedule_external_diagnostics(uri.clone());

        self.log(MessageType::INFO, format!("Opened file: {}", uri))
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        if params.content_changes.is_empty() {
            return;
        }

        // Apply incremental edits to the current content.
        // Each change event either has a range (incremental) or replaces
        // the entire document (range is None).
        let text = {
            let open_files = self.open_files.read();
            let mut current = open_files
                .get(&uri)
                .map(|s| s.to_string())
                .unwrap_or_default();
            drop(open_files);

            for change in &params.content_changes {
                if let Some(range) = change.range {
                    let start =
                        crate::text_position::position_to_byte_offset(&current, range.start);
                    let end = crate::text_position::position_to_byte_offset(&current, range.end);
                    current.replace_range(start..end, &change.text);
                } else {
                    // Full content replacement (fallback)
                    current = change.text.clone();
                }
            }
            Arc::new(current)
        };

        self.open_files
            .write()
            .insert(uri.clone(), Arc::clone(&text));

        // A resource document is re-scanned the same way a PHP file is
        // re-parsed: on a blocking task, and only if the buffer it was
        // queued for is still the current one.  Scanning a large XML on the
        // service loop for every keystroke would stall interactive
        // requests, and refreshing lenses per keystroke would make the
        // client re-pull them faster than it can render them.
        if crate::resource_navigation::is_resource_document(&uri) {
            if self.sync_ast_updates {
                self.update_resource_symbol_index(&uri, &text);
                return;
            }
            let backend = self.clone_for_blocking();
            tokio::spawn(async move {
                let refresh_backend = backend.clone_for_blocking();
                let committed = run_blocking_cancel_safe("did_change resource scan", move || {
                    let is_latest_text = backend
                        .open_files
                        .read()
                        .get(&uri)
                        .is_some_and(|current| Arc::ptr_eq(current, &text));
                    if !is_latest_text {
                        return false;
                    }
                    backend.update_resource_symbol_index(&uri, &text);
                    true
                })
                .await;

                if committed == Some(true)
                    && refresh_backend
                        .supports_code_lens_refresh
                        .load(Ordering::Acquire)
                    && let Some(ref client) = refresh_backend.client
                {
                    let _ = client.code_lens_refresh().await;
                }
            });
            return;
        }

        // Re-parse in a blocking background task so typing does not
        // monopolize the LSP service loop and delay completion requests.
        //
        // Until this task completes, hover/completion may use the
        // previous symbol map for this file. That is preferable to
        // queuing interactive requests behind a full parse on every
        // keystroke; `update_ast` already tolerates stale maps when
        // incomplete code cannot be parsed.
        if self.sync_ast_updates {
            self.update_ast(&uri, &text);
            self.schedule_diagnostics(uri.clone());
        } else {
            let backend = self.clone_for_blocking();
            tokio::spawn(async move {
                let refresh_backend = backend.clone_for_blocking();
                let uri_for_diagnostics = uri.clone();
                let committed = run_blocking_cancel_safe("did_change parse", move || {
                    let parse_lock = {
                        let mut locks = backend.did_change_parse_locks.lock();
                        Arc::clone(
                            locks
                                .entry(uri.clone())
                                .or_insert_with(|| Arc::new(parking_lot::Mutex::new(()))),
                        )
                    };
                    let _parse_guard = parse_lock.lock();
                    let is_latest_text = backend
                        .open_files
                        .read()
                        .get(&uri)
                        .is_some_and(|current| Arc::ptr_eq(current, &text));
                    if !is_latest_text {
                        return false;
                    }

                    let started = std::time::Instant::now();
                    backend.update_ast(&uri, &text);
                    let elapsed = started.elapsed();
                    if elapsed >= std::time::Duration::from_millis(100) {
                        tracing::debug!(
                            target: "performance",
                            "PHPantom: didChange parse took {:?}",
                            elapsed
                        );
                    }
                    backend.schedule_diagnostics(uri_for_diagnostics);
                    true
                })
                .await;

                // A new symbol map was committed.  Tokens the editor already
                // holds were computed from the pre-edit map (the
                // semanticTokens request usually races ahead of this
                // background parse), so ask for a re-pull.
                if committed == Some(true)
                    && let Some(ref client) = refresh_backend.client
                {
                    if refresh_backend
                        .supports_semantic_tokens_refresh
                        .load(Ordering::Acquire)
                    {
                        let _ = client.semantic_tokens_refresh().await;
                    }
                    if refresh_backend
                        .supports_inlay_hint_refresh
                        .load(Ordering::Acquire)
                    {
                        let _ = client.inlay_hint_refresh().await;
                    }
                    if refresh_backend
                        .supports_code_lens_refresh
                        .load(Ordering::Acquire)
                    {
                        let _ = client.code_lens_refresh().await;
                    }
                }
            });
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        self.open_files.write().remove(&uri);
        self.did_change_parse_locks.lock().remove(&uri);
        self.clear_declaration_baseline(&uri);

        // Drop coalescing state for this file so the maps don't grow unbounded
        // across an editing session.
        let suffix = format!("\u{0}{uri}");
        {
            let coalesce = &self.whole_file_coalesce;
            coalesce.latest.lock().retain(|k, _| !k.ends_with(&suffix));
            coalesce.locks.lock().retain(|k, _| !k.ends_with(&suffix));
            coalesce.last.lock().retain(|k, _| !k.ends_with(&suffix));
        }

        if crate::resource_navigation::is_resource_document(&uri) {
            if let Some(content) = self.get_file_content(&uri) {
                self.update_resource_symbol_index(&uri, &content);
            } else {
                self.clear_file_maps(&uri);
            }
        } else {
            self.clear_file_maps(&uri);
        }

        // Clear diagnostics so stale warnings don't linger after the file is closed
        self.clear_diagnostics_for_file(&uri).await;

        self.log(MessageType::INFO, format!("Closed file: {}", uri))
            .await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let is_resource = crate::resource_navigation::is_resource_document(&uri);

        if let Some(text) = params.text {
            let text = Arc::new(text);
            self.open_files
                .write()
                .insert(uri.clone(), Arc::clone(&text));
            if is_resource {
                self.update_resource_symbol_index(&uri, &text);
            } else {
                self.update_ast(&uri, &text);
            }
        }

        if is_resource {
            return;
        }

        // A save is a reliable sync point: re-diagnose the saved file
        // and all other open files.  This catches cross-file changes
        // (e.g. a function signature change in test2.php that affects
        // diagnostics in test.php) and provides a fallback for editors
        // (like Neovim) where didChange alone may not trigger a
        // visible diagnostic refresh.
        self.schedule_diagnostics(uri.clone());
        self.schedule_diagnostics_for_open_files(&uri);

        // If the saved file passes data to Blade templates, re-run
        // call-site inference for those templates so a changed `view()`
        // call is reflected without waiting for the template's next
        // parse.  Runs on a blocking thread: inference resolves the
        // passed expressions' types, which can parse other files.
        {
            let backend = self.clone_for_blocking();
            let caller_uri = uri.clone();
            spawn_blocking_detached("did_save blade inference", move || {
                backend.refresh_blade_inference_for_caller(&caller_uri);
            });
        }

        // External tools (PHPStan, PHPCS, Mago) are expensive and
        // serialized, so they are only triggered on save — not on
        // every keystroke.  This is the only place they are scheduled.
        self.schedule_external_diagnostics(uri);
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        // Clients re-push settings for reasons of their own, so a
        // notification that carries no file filters is not a request to
        // drop the ones already in force, and one that repeats the
        // current filters is not a change.
        let Some(options) =
            crate::config::ClientIndexingOptions::from_client_settings(&params.settings)
        else {
            return;
        };
        // Captured before the change, so the reconciliation below can
        // tell which way the filters moved.
        let previous_filters = self.index_filters();
        if !self.set_client_indexing_options(options) {
            return;
        }

        // A newly added extension needs its own watcher, or the client
        // never reports events for those files and the index keeps
        // serving the last full scan. Same follow-up a live
        // `.phpantom.toml` edit performs.
        self.reregister_watched_files_if_changed();

        // The index was built under the filters the user just changed
        // away from: drop what they now exclude, and walk for what they
        // now admit.
        self.reconcile_index_for_filter_change(&previous_filters);

        // A narrowed exclude list makes classes resolvable that were
        // missing a moment ago, so the negative cache has to go or the
        // editor keeps showing "class not found" for them.
        self.clear_class_not_found_cache();

        self.request_diagnostic_refresh().await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let workspace_root = self.workspace.workspace_root.read().clone();
        let Some(root) = workspace_root else {
            return;
        };

        // The whole batch is filtered and reindexed on a blocking thread.  A
        // refocused editor can deliver hundreds of KiB of events in one
        // notification; awaiting the blocking task yields to the LSP message
        // loop, so the server keeps draining hover, completion, and
        // diagnostic requests instead of freezing until the batch is handled.
        let backend = self.clone_for_blocking();
        let did_work = run_blocking_cancel_safe("did_change_watched_files", move || {
            backend.apply_watched_file_changes(&params, &root)
        })
        .await
        .unwrap_or(false);

        // Open files may reference a class that was just added or removed; ask
        // the editor to re-pull diagnostics so stale "unknown class" errors
        // (or missing ones) are corrected.
        if did_work {
            self.request_diagnostic_refresh().await;
            if self.supports_code_lens_refresh.load(Ordering::Acquire)
                && let Some(ref client) = self.client
            {
                let _ = client.code_lens_refresh().await;
            }
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
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
                if crate::resource_navigation::is_resource_document(uri) {
                    let location = backend.get_file_content(uri).and_then(|content| {
                        crate::util::catch_panic_unwind_safe(
                            "goto_definition",
                            uri,
                            Some(position),
                            || backend.resolve_resource_definition(&content, position),
                        )
                        .flatten()
                    });
                    return Ok(location.map(GotoDefinitionResponse::Scalar));
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
                    return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                }
                // For Blade files, check if the cursor is on a `{{`/`}}` echo
                // delimiter first, so go-to-definition agrees with hover on the
                // same position (the implicit `e()` call) instead of falling
                // through to the virtual PHP content, where the position maps
                // to whichever expression happens to start at that offset.
                if backend.is_blade_file(uri)
                    && let Some(delimiter_result) =
                        backend.blade_echo_delimiter_definition(uri, position)
                {
                    return Ok(delimiter_result.map(GotoDefinitionResponse::Scalar));
                }
                backend.handle_with_position("goto_definition", uri, position, |content, pos| {
                    let locs = backend.resolve_definition(uri, content, pos);
                    if locs.is_empty() {
                        None
                    } else if locs.len() == 1 {
                        Some(GotoDefinitionResponse::Scalar(
                            backend.translate_location(locs[0].clone()),
                        ))
                    } else {
                        Some(GotoDefinitionResponse::Array(
                            locs.into_iter()
                                .map(|l| backend.translate_location(l))
                                .collect(),
                        ))
                    }
                })
            },
        )
        .await
    }

    async fn goto_implementation(
        &self,
        params: GotoImplementationParams,
    ) -> Result<Option<GotoImplementationResponse>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;
        let token = match params.work_done_progress_params.work_done_token {
            Some(t) => Some(t),
            None => self.progress_create("goto_implementation").await,
        };

        if let Some(ref tok) = token {
            self.progress_begin(tok, "Go to Implementation", Some("Resolving…".to_string()))
                .await;
        }

        // Run on a blocking thread so the async runtime stays free to
        // flush progress notifications to the client.
        let mut backend = self.clone_for_blocking();
        let poller = token.as_ref().map(|tok| {
            let state = crate::progress::ScanProgress::new();
            backend.request_progress = Some(Arc::clone(&state));
            self.spawn_progress_poller(tok.clone(), state)
        });
        let uri_clone = uri.clone();
        let result = run_blocking_cancel_safe("goto_implementation", move || {
            backend.handle_with_position(
                "goto_implementation",
                &uri_clone,
                position,
                |content, pos| {
                    backend
                        .resolve_implementation(&uri_clone, content, pos)
                        .map(|locs| {
                            locs.into_iter()
                                .map(|l| backend.translate_location(l))
                                .collect()
                        })
                        .and_then(wrap_locations)
                },
            )
        })
        .await
        .unwrap_or(Ok(None));

        if let Some(poller) = poller {
            poller.finish().await;
        }
        if let Some(ref tok) = token {
            self.progress_end(tok, Some("Done".to_string())).await;
        }

        result
    }

    async fn goto_type_definition(
        &self,
        params: GotoTypeDefinitionParams,
    ) -> Result<Option<GotoTypeDefinitionResponse>> {
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
            // For Blade files, check if the cursor is on a `{{` or `{!!` echo
            // delimiter. If so, return hover for `e()` (escaped echo) or a
            // raw-echo explanation, rather than falling through to the virtual
            // PHP content where the position maps into boilerplate.
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
            Ok(Some(CompletionResponse::Array(items))) => items.len(),
            Ok(Some(CompletionResponse::List(list))) => list.items.len(),
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
        let uri = params.text_document_position.text_document.uri.to_string();
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;
        let token = match params.work_done_progress_params.work_done_token {
            Some(t) => Some(t),
            None => self.progress_create("find_references").await,
        };

        if let Some(ref tok) = token {
            self.progress_begin(tok, "Find References", Some("Scanning…".to_string()))
                .await;
        }

        // Run on a blocking thread so the async runtime stays free to
        // flush progress notifications to the client.
        let mut backend = self.clone_for_blocking();
        let poller = token.as_ref().map(|tok| {
            let state = crate::progress::ScanProgress::new();
            backend.request_progress = Some(Arc::clone(&state));
            self.spawn_progress_poller(tok.clone(), state)
        });
        let uri_clone = uri.clone();
        let result = run_blocking_cancel_safe("references", move || {
            backend.handle_with_position("references", &uri_clone, position, |content, pos| {
                backend
                    .find_references(&uri_clone, content, pos, include_declaration)
                    .map(|locs| {
                        locs.into_iter()
                            .filter_map(|l| backend.try_translate_location(l))
                            .collect()
                    })
            })
        })
        .await
        .unwrap_or(Ok(None));

        if let Some(poller) = poller {
            poller.finish().await;
        }
        if let Some(ref tok) = token {
            self.progress_end(tok, Some("Done".to_string())).await;
        }

        result
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.to_string();

        // Code actions are not yet Blade-aware (edits target virtual PHP
        // coordinates and may insert code outside valid PHP regions).
        // Disabled until Phase 2 component support lands.
        if self.is_blade_file(&uri) {
            return Ok(None);
        }

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
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri.to_string();
        let position = params.position;

        self.run_position_request("prepare_rename", uri, position, |backend, uri, position| {
            backend.handle_with_position("prepare_rename", uri, position, |content, pos| {
                backend
                    .handle_prepare_rename(uri, content, pos)
                    .and_then(|res| match res {
                        PrepareRenameResponse::Range(r) => backend
                            .try_translate_blade_range(uri, r)
                            .map(PrepareRenameResponse::Range),
                        PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } => {
                            backend.try_translate_blade_range(uri, range).map(|range| {
                                PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }
                            })
                        }
                        PrepareRenameResponse::DefaultBehavior { default_behavior } => {
                            Some(PrepareRenameResponse::DefaultBehavior { default_behavior })
                        }
                    })
            })
        })
        .await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri.to_string();
        let position = params.text_document_position.position;
        let new_name = params.new_name.clone();

        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        let outcome = run_blocking_cancel_safe("rename", move || {
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
                self.log(MessageType::WARNING, message.clone()).await;
                Err(tower_lsp::jsonrpc::Error {
                    code: tower_lsp::jsonrpc::ErrorCode::InvalidRequest,
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
    ) -> Result<Option<Vec<SymbolInformation>>> {
        // The query is matched against every symbol of every parsed file, so
        // this walks the whole workspace index.
        let backend = self.clone_for_blocking();
        Ok(run_blocking_cancel_safe("symbol", move || {
            backend.handle_workspace_symbol(&params.query)
        })
        .await
        .flatten())
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
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
            && let [uri_val, pos_val] = params.arguments.as_slice()
            && let Ok(uri) = serde_json::from_value::<Url>(uri_val.clone())
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
    ) -> Result<Option<SemanticTokensResult>> {
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
        self.inlay_hint_request(params).await
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
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
        let mut backend = self.clone_for_blocking();
        let item = params.item;
        let token = match params.work_done_progress_params.work_done_token {
            Some(t) => Some(t),
            None => self.progress_create("type_hierarchy_subtypes").await,
        };

        if let Some(ref tok) = token {
            self.progress_begin(tok, "Type Hierarchy", Some("Scanning…".to_string()))
                .await;
        }
        let poller = token.as_ref().map(|tok| {
            let state = crate::progress::ScanProgress::new();
            backend.request_progress = Some(Arc::clone(&state));
            self.spawn_progress_poller(tok.clone(), state)
        });

        let result = run_blocking_cancel_safe("subtypes", move || backend.subtypes_impl(&item))
            .await
            .flatten();

        if let Some(poller) = poller {
            poller.finish().await;
        }
        if let Some(ref tok) = token {
            self.progress_end(tok, Some("Done".to_string())).await;
        }

        Ok(result)
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        // Only handle Enter ("\n") for PHPDoc block generation.
        if params.ch != "\n" {
            return Ok(None);
        }

        let uri = params.text_document_position.text_document.uri.to_string();
        let position = params.text_document_position.position;

        let content = match self.get_file_content(&uri) {
            Some(c) => c,
            None => return Ok(None),
        };

        // This fires on every Enter, and generating the block resolves the
        // documented signature's types, so it stays off the request task.
        let backend = self.clone_for_blocking();
        let u = uri.clone();
        Ok(run_blocking_cancel_safe("on_type_formatting", move || {
            let ctx = backend.file_context(&u);
            let class_loader = backend.class_loader(&ctx);
            let function_loader = backend.function_loader(&ctx);

            crate::completion::phpdoc::generation::try_generate_docblock_on_enter(
                &content,
                position,
                &ctx.use_map,
                &ctx.namespace,
                &ctx.classes,
                &class_loader,
                Some(&backend),
                Some(&function_loader),
            )
        })
        .await
        .flatten())
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri.to_string();

        // External tools discover their config from the file's real path.
        let Some(file_path) = Url::parse(&uri).ok().and_then(|u| u.to_file_path().ok()) else {
            return Ok(None);
        };
        let Some(content) = self.get_file_content(&uri) else {
            return Ok(None);
        };

        // Blade markup isn't PHP, so a template resolves its own strategy:
        // Pint when the project formats Blade with it, the built-in
        // reindenter otherwise.
        let is_blade = self.is_blade_file(&uri);
        let blade_options = formatting::blade::options_from_lsp(&params.options);

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
                backend.format_content(&strategy, &file_path, &content, &backend.shutdown_flag)
            };
            formatted
                .map(|formatted| formatted.map(|text| formatting::compute_edits(&content, &text)))
        })
        .await;

        match result {
            Some(Ok(edits)) => Ok(edits),
            Some(Err(e)) => {
                self.log(MessageType::ERROR, format!("Formatting failed: {}", e))
                    .await;
                Err(tower_lsp::jsonrpc::Error {
                    code: tower_lsp::jsonrpc::ErrorCode::InternalError,
                    message: format!("Formatting failed: {}", e).into(),
                    data: None,
                })
            }
            None => {
                let msg = "Formatting task panicked".to_string();
                self.log(MessageType::ERROR, msg.clone()).await;
                Err(tower_lsp::jsonrpc::Error {
                    code: tower_lsp::jsonrpc::ErrorCode::InternalError,
                    message: msg.into(),
                    data: None,
                })
            }
        }
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        self.document_pull_diagnostic(params)
    }

    async fn workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReportResult> {
        self.workspace_pull_diagnostic(params)
    }
}

fn type_hierarchy_registration() -> Registration {
    Registration {
        id: "type-hierarchy".to_string(),
        method: "textDocument/prepareTypeHierarchy".to_string(),
        register_options: Some(
            serde_json::to_value(TypeHierarchyRegistrationOptions {
                text_document_registration_options: TextDocumentRegistrationOptions {
                    document_selector: Some(vec![DocumentFilter {
                        language: Some("php".to_string()),
                        scheme: None,
                        pattern: None,
                    }]),
                },
                type_hierarchy_options: TypeHierarchyOptions::default(),
                static_registration_options: StaticRegistrationOptions::default(),
            })
            .expect("type hierarchy registration options serialize"),
        ),
    }
}

/// Convert a `Vec<Location>` into a `GotoDefinitionResponse`.
///
/// Returns `Scalar` for a single location, `Array` for multiple, and
/// `None` for an empty vec.  This is used by `goto_implementation` and
/// `goto_type_definition` which both share this pattern.
fn wrap_locations(locations: Vec<Location>) -> Option<GotoDefinitionResponse> {
    match locations.len() {
        0 => None,
        1 => Some(GotoDefinitionResponse::Scalar(
            locations.into_iter().next().unwrap(),
        )),
        _ => Some(GotoDefinitionResponse::Array(locations)),
    }
}

// ─── Background indexing ────────────────────────────────────────────────────

impl Backend {
    pub(crate) async fn start_full_background_index(&self) {
        // Headless test backends have no client; skip so integration tests
        // stay deterministic instead of racing a background index thread.
        if self.client.is_none() {
            return;
        }
        if self.config().indexing.strategy() != IndexingStrategy::Full {
            return;
        }
        if self.workspace.workspace_root.read().is_none() {
            return;
        }
        if self.full_index_in_progress.swap(true, Ordering::AcqRel) {
            return;
        }

        let progress_token = self.progress_create("phpantom/full-index").await;
        if let Some(ref tok) = progress_token {
            self.progress_begin(
                tok,
                "PHPantom: Full index",
                Some("Parsing workspace files".to_string()),
            )
            .await;
        }

        let progress_state = crate::progress::ScanProgress::new();
        let poller = progress_token
            .as_ref()
            .map(|tok| self.spawn_progress_poller(tok.clone(), Arc::clone(&progress_state)));

        let parse_backend = self.clone_for_blocking();
        let progress_backend = self.clone_for_blocking();
        tokio::spawn(async move {
            let worker_state = Arc::clone(&progress_state);
            let indexed_files = run_blocking_cancel_safe("full_background_index", move || {
                let report_progress =
                    |percentage, message: String| worker_state.set_percentage(percentage, message);
                parse_backend.ensure_workspace_indexed_with_progress(Some(&report_progress));
                parse_backend.symbol_maps.read().len()
            })
            .await
            .unwrap_or(0);

            if let Some(poller) = poller {
                poller.finish().await;
            }

            progress_backend
                .full_index_in_progress
                .store(false, Ordering::Release);

            if let Some(tok) = progress_token {
                progress_backend
                    .progress_end(&tok, Some(format!("Parsed {} files", indexed_files)))
                    .await;
            }

            progress_backend.request_diagnostic_refresh().await;

            // Files opened before the index finished were rendering
            // member/class reference counts computed from a still-filling
            // index (stale zeros); now that it's complete, ask the editor
            // to re-pull inlay hints for those hints to catch up.
            if progress_backend
                .supports_inlay_hint_refresh
                .load(Ordering::Acquire)
                && let Some(ref client) = progress_backend.client
            {
                let _ = client.inlay_hint_refresh().await;
            }
            if progress_backend
                .supports_code_lens_refresh
                .load(Ordering::Acquire)
                && let Some(ref client) = progress_backend.client
            {
                let _ = client.code_lens_refresh().await;
            }

            // With the whole workspace parsed, eagerly resolve every
            // class so interactive requests hit a warm cache.  This
            // runs even when workspace diagnostics are disabled — it
            // serves completion, hover, and go-to-definition too.
            // Populating before `initialized` finishes would be wasted
            // work: it clears the resolution caches after the startup
            // scan.
            if progress_backend.wait_for_init_complete().await {
                progress_backend.eager_populate_resolved_classes().await;
            }

            // Then run the background workspace diagnostics pass
            // (native collectors over every unopened user file, then
            // project-wide external tools).  Deliberately chained after
            // the index so it never competes with startup for CPU.
            // Pull clients receive these results only through
            // `workspace/diagnostic` responses, so defer the pass until
            // the client sends its first workspace pull — a client that
            // never pulls never pays for the scan.  Check the toggle
            // before that wait, which otherwise parks this task until
            // shutdown on a pull client that has the pass switched off.
            if !progress_backend.config().diagnostics.workspace_enabled() {
                return;
            }
            if progress_backend
                .supports_pull_diagnostics
                .load(Ordering::Acquire)
            {
                progress_backend.wait_for_first_workspace_pull().await;
                if progress_backend.shutdown_flag.load(Ordering::Acquire) {
                    return;
                }
            }
            progress_backend.run_workspace_diagnostics().await;
        });
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

    /// `initialize` params carrying a workspace root and whatever the
    /// client chose to send as its initialization options.
    fn init_params(
        root: &std::path::Path,
        initialization_options: Option<serde_json::Value>,
    ) -> InitializeParams {
        #[allow(deprecated)]
        InitializeParams {
            root_uri: Some(Url::from_file_path(root).unwrap()),
            initialization_options,
            ..Default::default()
        }
    }

    /// The filters have to be live before anything scans, so the first
    /// discovery pass already honours them instead of indexing excluded
    /// trees and dropping them afterwards.
    #[tokio::test]
    async fn initialize_applies_client_supplied_file_filters() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Backend::new_test();

        backend
            .initialize(init_params(
                dir.path(),
                Some(serde_json::json!({
                    "indexing": { "exclude": ["generated"], "extensions": ["module"] }
                })),
            ))
            .await
            .unwrap();

        let filters = backend.index_filters();
        assert!(filters.is_excluded_entry(&dir.path().join("generated"), true));
        assert!(filters.is_php_file(&dir.path().join("a.module")));
    }

    /// Most clients send nothing, and one that does may send a shape
    /// meant for something else entirely. Neither may switch filtering on.
    #[tokio::test]
    async fn initialize_without_client_filters_leaves_discovery_unfiltered() {
        let dir = tempfile::tempdir().unwrap();

        for options in [None, Some(serde_json::json!({ "unrelated": true }))] {
            let backend = Backend::new_test();
            backend
                .initialize(init_params(dir.path(), options))
                .await
                .unwrap();

            let filters = backend.index_filters();
            assert!(!filters.is_excluded_entry(&dir.path().join("generated"), true));
            assert!(!filters.is_php_file(&dir.path().join("a.module")));
        }
    }

    /// Editing the editor's own settings mid-session has to take effect
    /// without a restart, the same way a live `.phpantom.toml` edit does.
    #[tokio::test]
    async fn did_change_configuration_recompiles_the_filters() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Backend::new_test();
        backend
            .initialize(init_params(dir.path(), None))
            .await
            .unwrap();

        backend
            .did_change_configuration(DidChangeConfigurationParams {
                settings: serde_json::json!({
                    "phpantom": { "indexing": { "exclude": ["generated"] } }
                }),
            })
            .await;

        assert!(
            backend
                .index_filters()
                .is_excluded_entry(&dir.path().join("generated"), true)
        );

        // Removing the entry again has to restore the unfiltered walk,
        // not merely stop adding to the exclude list.
        backend
            .did_change_configuration(DidChangeConfigurationParams {
                settings: serde_json::json!({ "phpantom": { "indexing": {} } }),
            })
            .await;

        assert!(
            !backend
                .index_filters()
                .is_excluded_entry(&dir.path().join("generated"), true)
        );
    }

    /// Recompiling the filters only governs the next scan. The
    /// notification also has to reconcile the index that was built under
    /// the old ones, or a class in a folder the user just hid keeps
    /// answering completion and workspace symbol search until restart.
    #[tokio::test]
    async fn did_change_configuration_evicts_newly_excluded_classes() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Backend::new_test();
        backend
            .initialize(init_params(dir.path(), None))
            .await
            .unwrap();

        let hidden = dir.path().join("generated/Hidden.php");
        std::fs::create_dir_all(hidden.parent().unwrap()).unwrap();
        std::fs::write(&hidden, "<?php\nclass Hidden {}\n").unwrap();
        let uri = crate::util::path_to_uri(&hidden);
        backend
            .symbols
            .with_class_declarations(|decls| decls.note_discovered("Hidden", uri));

        backend
            .did_change_configuration(DidChangeConfigurationParams {
                settings: serde_json::json!({
                    "phpantom": { "indexing": { "exclude": ["generated"] } }
                }),
            })
            .await;

        assert!(
            backend.symbols.fqn_uri_index.read().get("Hidden").is_none(),
            "a class under a newly excluded folder must leave the index"
        );
    }

    /// VS Code's client syncs the whole `phpantom` settings section on
    /// every change to any key in it. That notification says nothing
    /// about file filters, so it must leave the ones the extension
    /// forwarded at startup alone rather than reading as "cleared".
    #[tokio::test]
    async fn a_settings_push_without_filters_leaves_them_in_force() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Backend::new_test();
        backend
            .initialize(init_params(
                dir.path(),
                Some(serde_json::json!({ "indexing": { "exclude": ["generated"] } })),
            ))
            .await
            .unwrap();

        backend
            .did_change_configuration(DidChangeConfigurationParams {
                settings: serde_json::json!({
                    "phpantom": { "trace": { "server": "verbose" } }
                }),
            })
            .await;

        assert!(
            backend
                .index_filters()
                .is_excluded_entry(&dir.path().join("generated"), true),
            "an unrelated settings push must not discard the client's filters"
        );
    }
}
