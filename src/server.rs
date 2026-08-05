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
///   so the editor re-pulls only the files it cares about.
///
/// - **Push model** (fallback) — for clients without pull support, the
///   server pushes diagnostics via `textDocument/publishDiagnostics`
///   from a debounced background worker.  Each `did_change` bumps a
///   version counter; the worker waits for a quiet period before
///   publishing.
use std::collections::HashMap;
use std::path::PathBuf;
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
/// Returns `None` only if the blocking task itself panicked.
pub(crate) async fn run_blocking_cancel_safe<R, F>(f: F) -> Option<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    tokio::spawn(async move { tokio::task::spawn_blocking(f).await })
        .await
        .ok()
        .and_then(|inner| inner.ok())
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

        let client_supports_pull = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|td| td.diagnostic.as_ref())
            .is_some();
        self.supports_pull_diagnostics
            .store(client_supports_pull, Ordering::Release);

        // Detect whether the client supports file rename operations in
        // workspace edits.  Used by the rename handler to include a
        // `RenameFile` operation when a class rename matches PSR-4 naming.
        let client_supports_file_rename = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.workspace_edit.as_ref())
            .and_then(|we| we.resource_operations.as_ref())
            .is_some_and(|ops| ops.contains(&ResourceOperationKind::Rename));
        self.supports_file_rename
            .store(client_supports_file_rename, Ordering::Release);

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
                document_highlight_provider: Some(OneOf::Left(true)),
                linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(
                    true,
                )),
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
                    resolve_provider: Some(false),
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
            match crate::config::load_config(&root) {
                Ok(cfg) => {
                    *self.workspace.config.lock() = cfg;
                }
                Err(e) => {
                    self.log(
                        MessageType::WARNING,
                        format!("Failed to load .phpantom.toml: {}", e),
                    )
                    .await;
                }
            }

            let laravel_config = self.config().laravel;
            if laravel_config.schema.enabled() || laravel_config.migrations.enabled() {
                let bp_macros = self.laravel_macros.read().blueprint_macro_closures();
                match crate::virtual_members::laravel::database_schema::load_schema_index(
                    &root,
                    &laravel_config,
                    &bp_macros,
                ) {
                    Ok(index) => {
                        self.resolved_class_cache
                            .write()
                            .set_schema_index(index.clone());
                        *self.schema_index.write() = index;
                    }
                    Err(e) => {
                        self.log(
                            MessageType::WARNING,
                            format!("Failed to load Laravel schema dumps: {}", e),
                        )
                        .await;
                    }
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

            let has_composer_json = composer_package.is_some();

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

            if has_composer_json {
                // ── Single-project path (root composer.json exists) ──────
                self.init_single_project(&root, php_version, composer_package, Some(&progress))
                    .await;
            } else {
                // ── Monorepo / non-Composer path ────────────────────────
                let subprojects = composer::discover_subproject_roots(&root);

                if !subprojects.is_empty() {
                    self.init_monorepo(&root, &subprojects, php_version, Some(&progress))
                        .await;
                } else {
                    self.init_no_composer(&root, php_version, Some(&progress))
                        .await;
                }
            }

            // Warm the Eloquent Builder resolution cache only for Laravel
            // projects; a non-Laravel workspace has nothing to warm.
            if self.resolved_class_cache.read().is_laravel() {
                progress.set_percentage(90, "Warming Laravel completions");
                let warmed = self.warm_laravel_completion_cache();
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
        registrations.push(Registration {
            id: "workspace/didChangeWatchedFiles".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: Some(
                serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                    watchers: vec![
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/*.php".to_string()),
                            kind: Some(WatchKind::Create | WatchKind::Change | WatchKind::Delete),
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/composer.json".to_string()),
                            kind: Some(WatchKind::Change),
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/composer.lock".to_string()),
                            kind: Some(WatchKind::Change),
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/*.sql".to_string()),
                            kind: Some(WatchKind::Create | WatchKind::Change | WatchKind::Delete),
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/config/database.php".to_string()),
                            kind: Some(WatchKind::Create | WatchKind::Change | WatchKind::Delete),
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/.phpantom.toml".to_string()),
                            kind: Some(WatchKind::Create | WatchKind::Change | WatchKind::Delete),
                        },
                    ],
                })
                .unwrap(),
            ),
        });

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
        *self.laravel_aliases.write() = None;

        // Scan project source for Laravel macro registrations so macro
        // methods appear in completion, hover, and signature help.  Runs
        // after the cache clear above so injected macros are never shadowed
        // by a stale merge.
        if self.resolved_class_cache.read().is_laravel() {
            self.build_laravel_date_class();
            self.build_laravel_macro_index();
            self.build_provider_resources();
            self.build_laravel_command_index();
            self.build_laravel_morph_map_index();
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
            for (uri, content) in &file_snapshots {
                diagnostics_backend.schedule_diagnostics(uri.clone());
                diagnostics_backend
                    .publish_diagnostics_for_file(uri, content)
                    .await;
            }

            // In pull mode the eager publish above only pushed fast
            // diagnostics.  The full set (including slow diagnostics) is
            // now cached in `diag_last_full`.  Send a refresh so the
            // editor re-pulls and receives the complete diagnostics.
            if diagnostics_backend
                .supports_pull_diagnostics
                .load(Ordering::Acquire)
                && let Some(ref client) = diagnostics_backend.client
            {
                let _ = client.workspace_diagnostic_refresh().await;
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

        // Parse and update AST map, use map, and namespace map
        self.update_ast(&uri, &text);

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
                let result = tokio::task::spawn_blocking(move || {
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

                match result {
                    // A new symbol map was committed.  Tokens the editor
                    // already holds were computed from the pre-edit map
                    // (the semanticTokens request usually races ahead of
                    // this background parse), so ask for a re-pull.
                    Ok(true) => {
                        if refresh_backend
                            .supports_semantic_tokens_refresh
                            .load(Ordering::Acquire)
                            && let Some(ref client) = refresh_backend.client
                        {
                            let _ = client.semantic_tokens_refresh().await;
                        }
                    }
                    Ok(false) => {}
                    Err(err) => {
                        tracing::error!("PHPantom: didChange parse task failed: {}", err);
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

        // Clean up Blade preprocessor state for the closed file.
        if self.is_blade_file(&uri) {
            self.blade_virtual_content.write().remove(&uri);
            self.blade_source_maps.write().remove(&uri);
            self.blade_uris.write().remove(&uri);
        }

        self.clear_file_maps(&uri);

        // Clear diagnostics so stale warnings don't linger after the file is closed
        self.clear_diagnostics_for_file(&uri).await;

        self.log(MessageType::INFO, format!("Closed file: {}", uri))
            .await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        if let Some(text) = params.text {
            let text = Arc::new(text);
            self.open_files
                .write()
                .insert(uri.clone(), Arc::clone(&text));
            self.update_ast(&uri, &text);
        }

        // A save is a reliable sync point: re-diagnose the saved file
        // and all other open files.  This catches cross-file changes
        // (e.g. a function signature change in test2.php that affects
        // diagnostics in test.php) and provides a fallback for editors
        // (like Neovim) where didChange alone may not trigger a
        // visible diagnostic refresh.
        self.schedule_diagnostics(uri.clone());
        self.schedule_diagnostics_for_open_files(&uri);

        // External tools (PHPStan, PHPCS, Mago) are expensive and
        // serialized, so they are only triggered on save — not on
        // every keystroke.  This is the only place they are scheduled.
        self.schedule_external_diagnostics(uri);
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let workspace_root = self.workspace.workspace_root.read().clone();
        let Some(root) = workspace_root else {
            return;
        };

        // The whole batch is filtered and reindexed on a blocking thread
        // (wrapped in `tokio::spawn` so it always runs to completion).  A
        // refocused editor can deliver hundreds of KiB of events in one
        // notification; awaiting the blocking task yields to the LSP message
        // loop, so the server keeps draining hover, completion, and
        // diagnostic requests instead of freezing until the batch is handled.
        let backend = self.clone_for_blocking();
        let did_work = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || backend.apply_watched_file_changes(&params, &root))
                .await
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false);

        // Open files may reference a class that was just added or removed; ask
        // the editor to re-pull diagnostics so stale "unknown class" errors
        // (or missing ones) are corrected.
        if did_work
            && self.supports_pull_diagnostics.load(Ordering::Acquire)
            && let Some(ref client) = self.client
        {
            let _ = client.workspace_diagnostic_refresh().await;
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

        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        run_blocking_cancel_safe(move || {
            backend.handle_with_position("goto_definition", &uri_clone, position, |content, pos| {
                let locs = backend.resolve_definition(&uri_clone, content, pos);
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
        })
        .await
        .unwrap_or(Ok(None))
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
        //
        // Wrapped in tokio::spawn for cancellation safety (see references handler).
        let mut backend = self.clone_for_blocking();
        let poller = token.as_ref().map(|tok| {
            let state = crate::progress::ScanProgress::new();
            backend.request_progress = Some(Arc::clone(&state));
            self.spawn_progress_poller(tok.clone(), state)
        });
        let uri_clone = uri.clone();
        let result = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
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
            .unwrap_or(Ok(None))
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

        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        run_blocking_cancel_safe(move || {
            backend.handle_with_position(
                "goto_type_definition",
                &uri_clone,
                position,
                |content, pos| {
                    backend
                        .resolve_type_definition(&uri_clone, content, pos)
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
        .unwrap_or(Ok(None))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        run_blocking_cancel_safe(move || {
            // For Blade files, check if the cursor is on a `{{` or `{!!` echo
            // delimiter. If so, return hover for `e()` (escaped echo) or a
            // raw-echo explanation, rather than falling through to the virtual
            // PHP content where the position maps into boilerplate.
            if backend.is_blade_file(&uri_clone)
                && let Some(hover) = backend.blade_echo_delimiter_hover(&uri_clone, position)
            {
                return Ok(Some(hover));
            }

            backend.handle_with_position("hover", &uri_clone, position, |content, pos| {
                let mut hover = backend.handle_hover(&uri_clone, content, pos)?;
                if backend.is_blade_file(&uri_clone)
                    && let Some(range) = &mut hover.range
                {
                    range.start = backend.translate_php_to_blade(&uri_clone, range.start);
                    range.end = backend.translate_php_to_blade(&uri_clone, range.end);
                }
                Some(hover)
            })
        })
        .await
        .unwrap_or(Ok(None))
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
        let result = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || backend.handle_completion(params))
                .await
                .unwrap_or(Ok(None))
        })
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
        let item = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || backend.handle_completion_resolve(params)).await
        })
        .await
        .ok()
        .and_then(|joined| joined.ok())
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
        //
        // We wrap spawn_blocking inside tokio::spawn so the blocking
        // task is always awaited to completion even if tower-lsp
        // cancels this handler future via $/cancelRequest.  Without
        // this wrapper, dropping the handler future detaches the
        // spawn_blocking JoinHandle, and tower-lsp 0.20 may corrupt
        // its internal state when the orphaned task completes.
        let mut backend = self.clone_for_blocking();
        let poller = token.as_ref().map(|tok| {
            let state = crate::progress::ScanProgress::new();
            backend.request_progress = Some(Arc::clone(&state));
            self.spawn_progress_poller(tok.clone(), state)
        });
        let uri_clone = uri.clone();
        let result = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                backend.handle_with_position("references", &uri_clone, position, |content, pos| {
                    backend
                        .find_references(&uri_clone, content, pos, include_declaration)
                        .map(|locs| {
                            locs.into_iter()
                                .map(|l| backend.translate_location(l))
                                .collect()
                        })
                })
            })
            .await
            .unwrap_or(Ok(None))
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
        run_blocking_cancel_safe(move || {
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
        let (resolved, republish_uri) = self.resolve_code_action(action);

        // If a PHPStan quickfix was resolved, reassemble diagnostics so the
        // cleared diagnostic disappears immediately. In pull mode nothing is
        // pushed, so ask the editor to re-pull the freshly cached set.
        if let Some(uri_str) = republish_uri {
            self.assemble_and_push(&uri_str).await;
            if self.supports_pull_diagnostics.load(Ordering::Acquire)
                && let Some(client) = &self.client
            {
                let _ = client.workspace_diagnostic_refresh().await;
            }
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

        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        run_blocking_cancel_safe(move || {
            backend.handle_with_position("signature_help", &uri_clone, position, |content, pos| {
                backend.handle_signature_help(&uri_clone, content, pos)
            })
        })
        .await
        .unwrap_or(Ok(None))
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

        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        run_blocking_cancel_safe(move || {
            backend.handle_with_position(
                "document_highlight",
                &uri_clone,
                position,
                |content, pos| {
                    backend
                        .handle_document_highlight(&uri_clone, content, pos)
                        .map(|highlights| {
                            highlights
                                .into_iter()
                                .map(|h| {
                                    let mut h = h;
                                    h.range.start =
                                        backend.translate_php_to_blade(&uri_clone, h.range.start);
                                    h.range.end =
                                        backend.translate_php_to_blade(&uri_clone, h.range.end);
                                    h
                                })
                                .collect()
                        })
                },
            )
        })
        .await
        .unwrap_or(Ok(None))
    }

    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> Result<Option<LinkedEditingRanges>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let position = params.text_document_position_params.position;

        // Blade coordinate translation happens inside the handler, which has
        // the variable name needed to verify the translated ranges still
        // point at it in the template source.
        self.handle_with_position("linked_editing_range", &uri, position, |content, pos| {
            self.handle_linked_editing_range(&uri, content, pos)
        })
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri.to_string();
        let position = params.position;

        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        run_blocking_cancel_safe(move || {
            backend.handle_with_position("prepare_rename", &uri_clone, position, |content, pos| {
                backend
                    .handle_prepare_rename(&uri_clone, content, pos)
                    .map(|res| match res {
                        PrepareRenameResponse::Range(r) => PrepareRenameResponse::Range(Range {
                            start: backend.translate_php_to_blade(&uri_clone, r.start),
                            end: backend.translate_php_to_blade(&uri_clone, r.end),
                        }),
                        PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } => {
                            PrepareRenameResponse::RangeWithPlaceholder {
                                range: Range {
                                    start: backend.translate_php_to_blade(&uri_clone, range.start),
                                    end: backend.translate_php_to_blade(&uri_clone, range.end),
                                },
                                placeholder,
                            }
                        }
                        PrepareRenameResponse::DefaultBehavior { default_behavior } => {
                            PrepareRenameResponse::DefaultBehavior { default_behavior }
                        }
                    })
            })
        })
        .await
        .unwrap_or(Ok(None))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri.to_string();
        let position = params.text_document_position.position;
        let new_name = params.new_name.clone();

        let backend = self.clone_for_blocking();
        let uri_clone = uri.clone();
        run_blocking_cancel_safe(move || {
            backend.handle_with_position("rename", &uri_clone, position, |content, pos| {
                backend
                    .handle_rename(&uri_clone, content, pos, &new_name)
                    .map(|mut edit| {
                        if let Some(changes) = &mut edit.changes {
                            for (uri, edits) in changes {
                                let uri_str = uri.to_string();
                                if backend.is_blade_file(&uri_str) {
                                    for e in edits {
                                        e.range.start =
                                            backend.translate_php_to_blade(&uri_str, e.range.start);
                                        e.range.end =
                                            backend.translate_php_to_blade(&uri_str, e.range.end);
                                    }
                                }
                            }
                        }
                        edit
                    })
            })
        })
        .await
        .unwrap_or(Ok(None))
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
        Ok(self.handle_workspace_symbol(&params.query))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri.to_string();
        let backend = self.clone_for_blocking();
        let u = uri.clone();
        self.coalesced_whole_file("folding_range", &uri, move || {
            backend.handle_with_uri("folding_range", &u, |content| {
                backend.handle_folding_range(content)
            })
        })
        .await
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = params.text_document.uri.to_string();
        let backend = self.clone_for_blocking();
        let u = uri.clone();
        self.coalesced_whole_file("code_lens", &uri, move || {
            backend.handle_with_uri("code_lens", &u, |content| {
                backend.handle_code_lens(&u, content)
            })
        })
        .await
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
        self.handle_with_uri("selection_range", &uri, |content| {
            self.handle_selection_range(content, &positions)
        })
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
        self.handle_with_position("prepare_type_hierarchy", &uri, position, |content, pos| {
            self.prepare_type_hierarchy_impl(&uri, content, pos)
                .map(|items| {
                    items
                        .into_iter()
                        .map(|mut item| {
                            item.range.start = self.translate_php_to_blade(&uri, item.range.start);
                            item.range.end = self.translate_php_to_blade(&uri, item.range.end);
                            item.selection_range.start =
                                self.translate_php_to_blade(&uri, item.selection_range.start);
                            item.selection_range.end =
                                self.translate_php_to_blade(&uri, item.selection_range.end);
                            item
                        })
                        .collect()
                })
        })
    }

    async fn supertypes(
        &self,
        params: TypeHierarchySupertypesParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        Ok(self.supertypes_impl(&params.item))
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

        // Wrapped in tokio::spawn for cancellation safety (see references handler).
        let result = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || backend.subtypes_impl(&item))
                .await
                .unwrap_or(None)
        })
        .await
        .unwrap_or(None);

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

        let ctx = self.file_context(&uri);
        let class_loader = self.class_loader(&ctx);
        let function_loader = self.function_loader(&ctx);

        let edits = crate::completion::phpdoc::generation::try_generate_docblock_on_enter(
            &content,
            position,
            &ctx.use_map,
            &ctx.namespace,
            &ctx.classes,
            &class_loader,
            Some(self),
            Some(&function_loader),
        );

        Ok(edits)
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri.to_string();
        let config = self.config();

        // Read Composer metadata for require-dev detection and bin-dir.
        let workspace_root = self.workspace.workspace_root.read().clone();
        let composer_json: Option<composer::ComposerPackage> = workspace_root
            .as_deref()
            .and_then(composer::read_composer_package);
        let bin_dir: Option<String> = composer_json.as_ref().map(composer::get_bin_dir);

        // Resolve the formatting strategy: external tools, built-in, or disabled.
        let strategy = formatting::resolve_strategy(
            workspace_root.as_deref(),
            &config.formatting,
            composer_json.as_ref(),
            bin_dir.as_deref(),
        );

        // Resolve the file path from the URI for config discovery.
        let file_path = Url::parse(&uri).ok().and_then(|u| u.to_file_path().ok());
        let file_path = match file_path {
            Some(p) => p,
            None => return Ok(None),
        };

        let content = match self.get_file_content(&uri) {
            Some(c) => c,
            None => return Ok(None),
        };

        let php_version = self.php_version();

        // Execute the resolved formatting strategy on a blocking thread
        // to avoid stalling the async runtime while external tools run.
        let formatting_config = config.formatting.clone();
        let result = run_blocking_cancel_safe(move || {
            formatting::execute_strategy(
                &strategy,
                &content,
                &file_path,
                &formatting_config,
                php_version,
            )
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

// ─── Self-scan helpers ──────────────────────────────────────────────────────

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
            let indexed_files = tokio::task::spawn_blocking(move || {
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

            if progress_backend
                .supports_pull_diagnostics
                .load(Ordering::Acquire)
                && let Some(ref client) = progress_backend.client
            {
                let _ = client.workspace_diagnostic_refresh().await;
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
            progress_backend.run_workspace_diagnostics().await;
        });
    }

    /// Fetch the open-file content for `uri`, run `f` inside a panic
    /// guard, and return the result.
    ///
    /// Returns `None` when the file is not open or when `f` panics.
    /// Most LSP handlers follow the pattern "get content, run handler
    /// with panic protection, return result" — this helper captures
    /// that boilerplate in one place.
    pub(crate) fn with_file_content<T>(
        &self,
        handler_name: &str,
        uri: &str,
        position: Option<Position>,
        f: impl FnOnce(&str, Option<Position>) -> T,
    ) -> Option<T> {
        let mut content = self.get_file_content(uri)?;
        let mut pos = position;

        // If this is a Blade file, use the virtual PHP content and translate the position.
        if self.is_blade_file(uri)
            && let Some(virtual_content) = self.blade_virtual_content.read().get(uri)
        {
            content = virtual_content.clone();
            if let Some(p) = position {
                pos = Some(self.translate_blade_to_php(uri, p));
            }
        }

        // Activate the chain resolution cache so that shared chain prefixes
        // (e.g. `$model->where(...)` in `$model->where(...)->orderBy(...)`)
        // are resolved once and reused across all LSP handlers, not just
        // diagnostics.  The guard is re-entrant safe: if a diagnostic pass
        // already activated the cache, this is a no-op.
        let _chain_guard = crate::type_engine::resolver::with_chain_resolution_cache();

        // Activate the type-engine resolvers here rather than per feature.
        // Every LSP handler needs the file content, so this is the one
        // place none of them can bypass: go-to-definition, find-references,
        // signature help, code actions, rename, and inlay hints resolve an
        // expression with the same facilities hover and diagnostics have,
        // instead of a poorer answer for the identical code.
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();

        crate::util::catch_panic_unwind_safe(handler_name, uri, pos, || f(&content, pos))
    }

    /// Position-based handler helper. Extracts the URI and position from
    /// the params, fetches file content, runs the closure inside a panic
    /// guard, and flattens the nested `Option`.
    ///
    /// Covers the majority of LSP handlers that take a
    /// `TextDocumentPositionParams` and return `Option<T>`.
    fn handle_with_position<T>(
        &self,
        handler_name: &str,
        uri: &str,
        position: Position,
        f: impl FnOnce(&str, Position) -> Option<T>,
    ) -> Result<Option<T>> {
        Ok(self
            .with_file_content(
                handler_name,
                uri,
                Some(position),
                |content, pos| match pos {
                    Some(pos) => f(content, pos),
                    None => None,
                },
            )
            .flatten())
    }

    /// URI-only handler helper. Like [`handle_with_position`] but for
    /// handlers that only need the document URI (no cursor position).
    fn handle_with_uri<T>(
        &self,
        handler_name: &str,
        uri: &str,
        f: impl FnOnce(&str) -> Option<T>,
    ) -> Result<Option<T>> {
        Ok(self
            .with_file_content(handler_name, uri, None, |content, _| f(content))
            .flatten())
    }

    /// Run an expensive whole-file request (`kind`) for `uri` with coalescing.
    ///
    /// The `compute` closure runs on the blocking pool. At most one
    /// computation per `(kind, uri)` runs at a time; a request that is no
    /// longer the most recent of its kind when it acquires the slot returns
    /// the previous result instead of recomputing. This stops a keystroke
    /// burst from piling up dozens of un-cancellable full-file scans that
    /// would otherwise saturate every core and stall completion and hover.
    ///
    /// See [`WholeFileCoalesce`](crate::WholeFileCoalesce) for the rationale.
    async fn coalesced_whole_file<T, F>(
        &self,
        kind: &str,
        uri: &str,
        compute: F,
    ) -> Result<Option<T>>
    where
        T: Clone + Send + Sync + 'static,
        F: FnOnce() -> Result<Option<T>> + Send + 'static,
    {
        let key = format!("{kind}\u{0}{uri}");
        let coalesce = &self.whole_file_coalesce;
        let seq = coalesce.stamp(&key);

        let lock = coalesce.key_lock(&key);
        let _guard = lock.lock().await;

        // A newer request of the same kind for this file arrived while we
        // waited: it will produce the fresh result, so skip the scan and hand
        // back the previous result. The editor superseded (and likely already
        // cancelled) this request, so it discards whatever we return — but the
        // cached value avoids any chance of a momentary empty result.
        if !coalesce.is_latest(&key, seq) {
            return Ok(coalesce
                .last_result(&key)
                .and_then(|any| any.downcast_ref::<T>().cloned()));
        }

        let result = run_blocking_cancel_safe(compute)
            .await
            .unwrap_or(Ok(None))?;
        if let Some(value) = &result {
            coalesce.store_result(
                &key,
                Arc::new(value.clone()) as Arc<dyn std::any::Any + Send + Sync>,
            );
        }
        Ok(result)
    }

    // ── Initialization helpers ───────────────────────────────────────────

    /// Build the Laravel macro index by scanning the project's own source
    /// service providers, plus one level of classes they import, for
    /// `Target::macro('name', closure)` registrations.
    ///
    /// Vendor macros are recovered from the service providers packages register
    /// (via `extra.laravel.providers` in `installed.json`) plus any providers
    /// the app registers in `bootstrap/providers.php` / `config/app.php`,
    /// rather than re-reading the whole vendor tree. Project macros follow the
    /// same provider-rooted shape: each provider file is scanned directly and
    /// each imported class is scanned as a one-level helper candidate. Called
    /// once after indexing for Laravel projects. Files are byte-prefiltered for
    /// `macro(` so only candidates are parsed.
    pub(crate) fn build_laravel_macro_index(&self) {
        let php_version = Some(*self.workspace.php_version.lock());

        let mut index = crate::virtual_members::laravel::LaravelMacroIndex::default();
        let mut candidate_uris: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut provider_uris: Vec<String> = Vec::new();
        let mut imported_uris: Vec<String> = Vec::new();
        // Seed URI → the class references it contributed to this build.
        // `refresh_laravel_macros` compares an edited seed's references
        // against this snapshot and only rebuilds when they changed.
        let mut seeds: HashMap<String, Vec<String>> = HashMap::new();

        // The app's provider registration files are seeds too: adding a
        // provider there must trigger a rebuild.  Their reference
        // fingerprint is the provider class list itself.
        if let Some(root) = self.workspace.workspace_root.read().clone() {
            for rel in ["bootstrap/providers.php", "config/app.php"] {
                let path = root.join(rel);
                let uri = crate::util::path_to_uri(&path);
                let refs = self
                    .get_file_content(&uri)
                    .or_else(|| std::fs::read_to_string(&path).ok())
                    .map(|c| crate::virtual_members::laravel::parse_provider_class_list(&c))
                    .unwrap_or_default();
                seeds.insert(uri, refs);
            }
        }

        // Every mixin-class file the scan pulled macros from, so an edit to one
        // triggers a rebuild even though it holds no `macro(`/`mixin(` token.
        let mut mixin_uris: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Scan a single file's content into the index, keyed by its URI.
        let scan_content = |index: &mut crate::virtual_members::laravel::LaravelMacroIndex,
                            mixin_uris: &mut std::collections::HashSet<String>,
                            uri: String,
                            content: &str| {
            let has_macro = memchr::memmem::find(content.as_bytes(), b"macro(").is_some();
            let has_mixin = memchr::memmem::find(content.as_bytes(), b"mixin(").is_some();
            if !has_macro && !has_mixin {
                return;
            }
            let mut regs = if has_macro {
                crate::virtual_members::laravel::extract_macro_registrations(content, php_version)
            } else {
                Vec::new()
            };
            if has_mixin {
                for reg in self.synthesize_mixin_registrations(content, php_version) {
                    if let Some(mixin_uri) = &reg.definition_uri {
                        mixin_uris.insert(mixin_uri.clone());
                    }
                    regs.push(reg);
                }
            }
            if regs.is_empty() {
                return;
            }
            self.infer_laravel_macro_return_types(&mut regs, &uri, content);
            // A macro registered through a facade also attaches to the
            // facade's concrete container-bound class.
            self.expand_facade_macros(&mut regs);
            index.set_file(uri, regs);
        };

        // Vendor- and app-registered service providers seed macro discovery.
        for fqn in self.laravel_provider_fqns() {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            if candidate_uris.insert(uri.clone()) {
                provider_uris.push(uri);
            }
        }

        for uri in &provider_uris {
            let Some(content) = self.get_file_content(uri) else {
                seeds.insert(uri.clone(), Vec::new());
                continue;
            };
            scan_content(&mut index, &mut mixin_uris, uri.clone(), &content);

            let referenced =
                crate::virtual_members::laravel::parse_provider_referenced_classes(&content);
            for imported_fqn in &referenced {
                let Some(imported_uri) = self.resolve_class_uri(imported_fqn) else {
                    continue;
                };
                if !self.is_macro_helper_uri_allowed(uri, &imported_uri) {
                    continue;
                }
                if candidate_uris.insert(imported_uri.clone()) {
                    imported_uris.push(imported_uri);
                }
            }
            seeds.insert(uri.clone(), referenced);
        }

        for uri in &imported_uris {
            let Some(content) = self.get_file_content(uri) else {
                continue;
            };
            scan_content(&mut index, &mut mixin_uris, uri.clone(), &content);
        }

        index.rebuild();
        let has_macros = !index.is_empty();
        let new_targets = index.target_fqns();
        let target_count = new_targets.len();
        let old_targets = self.laravel_macros.read().target_fqns();
        *self.laravel_macros.write() = index;
        self.laravel_has_macros
            .store(has_macros, std::sync::atomic::Ordering::Relaxed);
        *self.laravel_macro_seeds.write() = seeds;
        *self.laravel_macro_mixin_uris.write() = mixin_uris;

        // Evict every class that had macros before or has them now, so a
        // rebuild triggered by a provider edit replaces stale cached merges
        // (both for added and for removed macros).
        {
            let mut cache = self.resolved_class_cache.write();
            for fqn in old_targets.iter().chain(new_targets.iter()) {
                crate::virtual_members::evict_fqn(&mut cache, fqn);
            }
        }

        tracing::info!(
            "PHPantom: scanned {} Laravel macro candidates ({} providers, {} imported classes), indexed {} macro targets",
            candidate_uris.len(),
            provider_uris.len(),
            imported_uris.len(),
            target_count,
        );
    }

    /// Scan project and vendor Artisan command classes and build the
    /// [`laravel_commands`](Backend::laravel_commands) index.
    ///
    /// Candidate files are those declaring a class whose short name ends in
    /// `Command` (the near-universal Laravel/Symfony convention), which live
    /// under a `Console/Commands/` directory (so project commands with
    /// unconventional names are still found), or any other non-vendor
    /// project class (so commands registered via `withCommands()` in
    /// `bootstrap/app.php` from arbitrary directories — e.g.
    /// `app/Actions/Sync` — are indexed too).  Each candidate is read once,
    /// gated by a cheap byte pre-filter for a
    /// `signature`/`Signature`/`AsCommand`/`$name` declaration before
    /// parsing, then scanned by
    /// [`scan_command_file`](crate::virtual_members::laravel::scan_command_file).
    pub(crate) fn build_laravel_command_index(&self) {
        let mut candidate_uris: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        {
            let idx = self.symbols.fqn_uri_index.read();
            for (fqn, uri) in idx.iter() {
                let short = fqn.rsplit('\\').next().unwrap_or(fqn);
                if short.ends_with("Command") || uri.contains("/Console/Commands/") {
                    candidate_uris.insert(uri.to_string());
                } else if !uri.contains("/vendor/") {
                    // A project class registered as a command outside the
                    // conventional locations — e.g. `withCommands()` in
                    // bootstrap/app.php pointing at an arbitrary directory
                    // such as app/Actions/Sync.  scan_command_file's
                    // extends-Command / attribute checks decide whether the
                    // file really declares a command.
                    candidate_uris.insert(uri.to_string());
                }
            }
        }

        let mut index = crate::virtual_members::laravel::LaravelCommandIndex::default();
        for uri in &candidate_uris {
            let Some(content) = self.get_file_content(uri) else {
                continue;
            };
            let bytes = content.as_bytes();
            let looks_like_command = memchr::memmem::find(bytes, b"signature").is_some()
                || memchr::memmem::find(bytes, b"Signature").is_some()
                || memchr::memmem::find(bytes, b"AsCommand").is_some()
                || memchr::memmem::find(bytes, b"$name").is_some();
            if !looks_like_command {
                continue;
            }
            let entries = crate::virtual_members::laravel::scan_command_file(&content, uri);
            index.set_file(uri.clone(), entries);
        }
        index.rebuild();

        let has_commands = !index.is_empty();
        let count = index.all_names().len();
        *self.laravel_commands.write() = index;
        self.laravel_has_commands
            .store(has_commands, std::sync::atomic::Ordering::Relaxed);

        tracing::info!(
            "PHPantom: scanned {} Laravel command candidates, indexed {} commands",
            candidate_uris.len(),
            count,
        );
    }

    /// Build the Eloquent morph-map index by scanning the project's registered
    /// service providers for `Relation::morphMap()` /
    /// `Relation::enforceMorphMap()` calls.
    ///
    /// Uses the same provider set as the macro scan (vendor packages'
    /// auto-discovered providers plus those the app lists in
    /// `bootstrap/providers.php` / `config/app.php`), since a morph map is
    /// registered from a provider's `boot()`.  Files are byte-prefiltered for
    /// the `orphMap(` token so only candidates are parsed.
    pub(crate) fn build_laravel_morph_map_index(&self) {
        let mut index = crate::virtual_members::laravel::LaravelMorphMapIndex::default();
        let mut scanned = 0usize;

        for fqn in self.laravel_provider_fqns() {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            if index.has_uri(&uri) {
                continue;
            }
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            scanned += 1;
            let mut scan = crate::virtual_members::laravel::scan_morph_map(&content);
            if scan.is_empty() {
                continue;
            }
            self.resolve_morph_map_table_aliases(&mut scan);
            index.set_file(uri, scan);
        }

        index.rebuild();
        let alias_count = index.all_aliases().len();
        *self.laravel_morph_map.write() = index;

        tracing::info!(
            "PHPantom: scanned {} Laravel provider files, indexed {} morph aliases",
            scanned,
            alias_count,
        );
    }

    /// Turn a `Relation::morphMap([Post::class, …])` list registration into
    /// `alias => model` entries by resolving each model's table name, which is
    /// the alias Laravel derives for it.
    ///
    /// A model whose table cannot be determined statically (it overrides
    /// `getTable()`) is dropped rather than guessed, so no wrong alias enters
    /// the index.
    fn resolve_morph_map_table_aliases(
        &self,
        scan: &mut crate::virtual_members::laravel::MorphMapScan,
    ) {
        for target in std::mem::take(&mut scan.table_keyed) {
            let Some(class) = self.find_or_load_class(&target.target_fqn) else {
                continue;
            };
            let Some(table) = crate::virtual_members::laravel::model_table_name(&class) else {
                continue;
            };
            scan.entries
                .push(crate::virtual_members::laravel::MorphMapEntry {
                    alias: table,
                    target_fqn: target.target_fqn,
                    alias_offset: target.offset,
                });
        }
    }

    /// Re-scan a single file's morph-map registrations after an edit.
    ///
    /// A cheap no-op unless the file currently contributes registrations or its
    /// new content contains a `morphMap(` call.  Only runs for Laravel projects.
    pub(crate) fn refresh_laravel_morph_map(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let was_contributor = self.laravel_morph_map.read().has_uri(uri);
        let has_token = memchr::memmem::find(content.as_bytes(), b"orphMap(").is_some();
        if !was_contributor && !has_token {
            return;
        }

        let mut scan = crate::virtual_members::laravel::scan_morph_map(content);
        self.resolve_morph_map_table_aliases(&mut scan);

        let mut index = self.laravel_morph_map.write();
        index.set_file(uri.to_string(), scan);
        index.rebuild();
    }

    /// Refresh the command index after a single file edit.
    ///
    /// Cheap: re-scans only the edited file when it is a command candidate
    /// (or was contributing before), replacing just that file's entries.
    pub(crate) fn refresh_laravel_command_index(&self, uri: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let was_contributor = self.laravel_commands.read().has_uri(uri);
        // Same candidate rule as the full build: a contributor file, a
        // conventionally-named command file, or any non-vendor project file
        // (commands registered via `withCommands()` may live anywhere).
        let looks_like_command_file = uri.ends_with("Command.php")
            || uri.contains("/Console/")
            || (!uri.contains("/vendor/") && uri.ends_with(".php"));
        if !was_contributor && !looks_like_command_file {
            return;
        }

        let entries = self
            .get_file_content(uri)
            .filter(|content| {
                let bytes = content.as_bytes();
                memchr::memmem::find(bytes, b"signature").is_some()
                    || memchr::memmem::find(bytes, b"Signature").is_some()
                    || memchr::memmem::find(bytes, b"AsCommand").is_some()
                    || memchr::memmem::find(bytes, b"$name").is_some()
            })
            .map(|content| crate::virtual_members::laravel::scan_command_file(&content, uri))
            .unwrap_or_default();

        if !was_contributor && entries.is_empty() {
            return;
        }

        let mut index = self.laravel_commands.write();
        index.set_file(uri.to_string(), entries);
        index.rebuild();
        let has_commands = !index.is_empty();
        drop(index);
        self.laravel_has_commands
            .store(has_commands, std::sync::atomic::Ordering::Relaxed);
    }

    /// Find the date class selected by project service providers. Laravel's
    /// helpers use this factory, so `now()` and `today()` return this class
    /// rather than their broad `CarbonInterface` declaration.
    ///
    /// Runs in both the LSP `initialized` handler and the headless `analyze`
    /// pipeline so every consumer resolves the date helpers to a concrete
    /// class. Until this has run, `laravel_date_class` stays `None` and the
    /// helpers resolve to nothing rather than a stale default.
    pub(crate) fn build_laravel_date_class(&self) {
        let mut configured = None;
        // Track every file this scan reads so the single-file refresh can tell
        // whether an edit could change the configured class.  The app's
        // provider-registration files are seeds too: editing them changes which
        // providers are registered, so a `Date::use()` in a newly added (or
        // removed) provider is picked up on the next scan.
        let mut seed_uris: std::collections::HashSet<String> = std::collections::HashSet::new();
        if let Some(root) = self.workspace.workspace_root.read().clone() {
            for rel in ["bootstrap/providers.php", "config/app.php"] {
                seed_uris.insert(crate::util::path_to_uri(&root.join(rel)));
            }
        }
        let providers = self.laravel_provider_fqns();
        for fqn in providers {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            let Ok(url) = tower_lsp::lsp_types::Url::parse(&uri) else {
                continue;
            };
            let Ok(path) = url.to_file_path() else {
                continue;
            };
            if self.is_in_vendor_dir(&path) {
                continue;
            }
            seed_uris.insert(uri.clone());
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            if let Some(class) =
                crate::virtual_members::laravel::extract_date_factory_class(&content)
            {
                configured = Some(class);
            }
        }
        *self.laravel_date_seed_uris.write() = seed_uris;
        *self.laravel_date_class.write() = Some(configured);
    }

    /// Collect the FQNs of every Laravel service provider that could register a
    /// macro: those installed vendor packages auto-discover (via
    /// `extra.laravel.providers` in each vendor's `installed.json`) plus those
    /// the app lists in `bootstrap/providers.php` / `config/app.php`.
    fn laravel_provider_fqns(&self) -> Vec<String> {
        let mut fqns: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut push = |fqns: &mut Vec<String>, fqn: String| {
            if seen.insert(fqn.clone()) {
                fqns.push(fqn);
            }
        };

        for vendor_dir in self.workspace.vendor_dir_paths.lock().iter() {
            let installed = vendor_dir.join("composer").join("installed.json");
            if let Ok(content) = std::fs::read_to_string(&installed) {
                for fqn in crate::virtual_members::laravel::parse_installed_providers(&content) {
                    push(&mut fqns, fqn);
                }
            }
        }

        if let Some(root) = self.workspace.workspace_root.read().clone() {
            for rel in ["bootstrap/providers.php", "config/app.php"] {
                let path = root.join(rel);
                let uri = crate::util::path_to_uri(&path);
                let content = self
                    .get_file_content(&uri)
                    .or_else(|| std::fs::read_to_string(&path).ok());
                if let Some(content) = content {
                    for fqn in crate::virtual_members::laravel::parse_provider_class_list(&content)
                    {
                        push(&mut fqns, fqn);
                    }
                }
            }
        }

        fqns
    }

    /// Resolve a class FQN to the URI of the file that declares it, loading the
    /// class if it is not yet in the FQN → URI index.  Used to locate provider
    /// source files for the macro scan.
    fn resolve_class_uri(&self, fqn: &str) -> Option<String> {
        if let Some(uri) = self.symbols.fqn_uri_index.read().get(fqn).cloned() {
            return Some(uri);
        }
        // Not indexed yet: loading the class populates its FQN → URI entry.
        self.find_or_load_class(fqn);
        self.symbols.fqn_uri_index.read().get(fqn).cloned()
    }

    /// Expand every `Target::mixin(new X)` / `Target::mixin(X::class)`
    /// registration found in `content` into the concrete macros the mixin class
    /// `X` contributes.
    ///
    /// The mixin class's methods live in a different file than the `::mixin(…)`
    /// call, so this resolves `X` to its source (via the class index, preferring
    /// an open editor buffer over disk) and parses each qualifying method's
    /// returned closure.  Each resulting registration records the mixin file's
    /// URI as its go-to-definition target.  Returns an empty vector when the
    /// file registers no mixins or the mixin classes cannot be located.
    fn synthesize_mixin_registrations(
        &self,
        content: &str,
        php_version: Option<crate::types::PhpVersion>,
    ) -> Vec<crate::virtual_members::laravel::MacroRegistration> {
        let mixins = crate::virtual_members::laravel::extract_mixin_registrations(content);
        let mut out = Vec::new();
        for mixin in mixins {
            let Some(uri) = self.resolve_class_uri(&mixin.mixin_fqn) else {
                continue;
            };
            let Some(mixin_source) = self.get_file_content(&uri).or_else(|| {
                let path = tower_lsp::lsp_types::Url::parse(&uri)
                    .ok()?
                    .to_file_path()
                    .ok()?;
                std::fs::read_to_string(path).ok()
            }) else {
                continue;
            };
            out.extend(crate::virtual_members::laravel::synthesize_mixin_macros(
                &mixin_source,
                &mixin.mixin_fqn,
                &uri,
                &mixin.target,
                php_version,
            ));
        }
        out
    }

    /// Re-scan a single file's macro registrations after an edit, keeping the
    /// index and the resolved-class cache coherent.
    ///
    /// A cheap no-op unless the file currently contributes macros or its new
    /// content contains a `macro(` call.  Only runs for Laravel projects.
    pub(crate) fn refresh_laravel_macros(&self, uri: &str, content: &str) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        // Re-run the full date-factory scan when the edited file is one that
        // could configure it: a registered service provider, or one of the
        // app's provider-registration files.  Scanning (rather than a one-way
        // set on any `::use` call) means adding, changing, or *removing* a
        // `Date::use()` / `DateFactory::use()` call is reflected, and an edit
        // to an unrelated file can neither override the configured class nor
        // leave a stale one behind.
        if self.laravel_date_seed_uris.read().contains(uri) {
            self.build_laravel_date_class();
        }
        // A `Macroable::mixin()` registration pulls its macros from another
        // file and records that file as a dependency.  Because those macros are
        // keyed under the registration site (not the mixin class) and the mixin
        // class carries no `macro(`/`mixin(` token of its own, the single-file
        // path below cannot keep them coherent.  So any edit that touches a
        // `mixin(` call site, or a file a mixin was read from, rebuilds the
        // whole index (which also refreshes the dependency set).  Mixin
        // registrations are rare, so the occasional full rebuild is cheap.
        if memchr::memmem::find(content.as_bytes(), b"mixin(").is_some()
            || self.laravel_macro_mixin_uris.read().contains(uri)
        {
            self.build_laravel_macro_index();
            return;
        }
        // An edit to a seed file (a service provider or the app's provider
        // registration files) that changes its class references alters which
        // files feed the index, so the index is rebuilt.  When the references
        // are unchanged the edit can only affect the seed's own
        // registrations, which the single-file path below picks up.
        let prev_refs = self.laravel_macro_seeds.read().get(uri).cloned();
        if let Some(prev_refs) = prev_refs {
            let refs = if self.is_laravel_provider_list_uri(uri) {
                crate::virtual_members::laravel::parse_provider_class_list(content)
            } else {
                crate::virtual_members::laravel::parse_provider_referenced_classes(content)
            };
            if refs != prev_refs {
                self.build_laravel_macro_index();
                return;
            }
        }
        let had = self.laravel_macros.read().has_uri(uri);
        let has_token = memchr::memmem::find(content.as_bytes(), b"macro(").is_some();
        if !had && !has_token {
            return;
        }

        let php_version = Some(*self.workspace.php_version.lock());
        let mut regs =
            crate::virtual_members::laravel::extract_macro_registrations(content, php_version);
        self.infer_laravel_macro_return_types(&mut regs, uri, content);
        // A macro registered through a facade also attaches to the facade's
        // concrete container-bound class.
        self.expand_facade_macros(&mut regs);

        let targets = {
            let mut index = self.laravel_macros.write();
            // Capture the pre-edit targets too, so a class whose last macro
            // this edit removed is also evicted below.
            let mut targets = index.target_fqns();
            index.set_file(uri.to_string(), regs);
            index.rebuild();
            self.laravel_has_macros
                .store(!index.is_empty(), std::sync::atomic::Ordering::Relaxed);
            targets.extend(index.target_fqns());
            targets
        };

        // Evict every class a macro attaches to so the next resolution picks
        // up the change instead of a stale cached merge.
        let mut cache = self.resolved_class_cache.write();
        for fqn in targets {
            crate::virtual_members::evict_fqn(&mut cache, &fqn);
        }
    }

    /// Whether `uri` is one of the app's provider registration files
    /// (`bootstrap/providers.php` / `config/app.php`), whose macro-relevant
    /// references are the provider class list rather than method-body class
    /// references.
    fn is_laravel_provider_list_uri(&self, uri: &str) -> bool {
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return false;
        };
        ["bootstrap/providers.php", "config/app.php"]
            .iter()
            .any(|rel| crate::util::path_to_uri(&root.join(rel)) == uri)
    }

    pub(crate) fn build_provider_resources(&self) {
        let mut resources = crate::virtual_members::laravel::ProviderResources::default();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let workspace_root = self
            .workspace
            .workspace_root
            .read()
            .clone()
            .unwrap_or_default();

        for fqn in self.laravel_provider_fqns() {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            if !seen.insert(uri.clone()) {
                continue;
            }
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            let file_dir = tower_lsp::lsp_types::Url::parse(&uri)
                .ok()
                .and_then(|u| u.to_file_path().ok())
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            let Some(file_dir) = file_dir else {
                continue;
            };

            resources.merge(crate::virtual_members::laravel::extract_provider_resources(
                &content,
                &file_dir,
                &workspace_root,
            ));
        }

        let config_count = resources.config_files.len();
        let view_count = resources.view_dirs.len();
        let trans_count = resources.trans_dirs.len();
        let route_count = resources.route_files.len();
        *self.laravel_provider_resources.write() = resources;

        if config_count + view_count + trans_count + route_count > 0 {
            let mut cache = self.laravel_string_key_cache.write();
            cache.config_keys = None;
            cache.config_trees = None;
            cache.view_names = None;
            cache.trans_keys = None;
            cache.routes = None;
        }

        tracing::info!(
            "PHPantom: discovered {} package config files, {} view dirs, {} translation dirs, {} route files from service providers",
            config_count,
            view_count,
            trans_count,
            route_count,
        );
    }

    fn infer_laravel_macro_return_types(
        &self,
        regs: &mut [crate::virtual_members::laravel::MacroRegistration],
        uri: &str,
        content: &str,
    ) {
        let file_ctx = self.file_context(uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        for reg in regs.iter_mut() {
            if reg.method.return_type.is_some() || reg.method.native_return_type.is_some() {
                continue;
            }
            if reg.closure_text.is_none() {
                continue;
            }
            // A mixin-derived macro's closure lives in the mixin class file, not
            // in the registration site, and its `name_offset` points into that
            // file.  Resolving the closure body against the registration file's
            // content would use a mismatched offset and the wrong scope, so
            // route it through its own file context instead.
            if let Some(def_uri) = reg.definition_uri.clone() {
                self.infer_mixin_macro_return_type(reg, &def_uri);
                continue;
            }
            let closure_text = reg.closure_text.as_deref().unwrap_or_default();
            let Some(target_class) = self.find_or_load_class(&reg.target) else {
                continue;
            };
            let rctx = crate::type_engine::resolver::ResolutionCtx {
                current_class: Some(target_class.as_ref()),
                all_classes: &file_ctx.classes,
                content,
                cursor_offset: reg.name_offset,
                class_loader: &class_loader,
                backend: Some(self),
                laravel_macro_this_resolver: None,
                resolved_class_cache: Some(&self.resolved_class_cache),
                function_loader: Some(&function_loader),
                scope_var_resolver: None,
                is_in_static_method: false,
                preserve_static: true,
            };
            if let Some(ty) = Self::infer_closure_return_type(closure_text, &rctx) {
                reg.method.return_type = Some(ty);
                reg.method.is_inferred_return = true;
            }
        }
    }

    /// Infer a mixin-derived macro's return type from the closure its mixin
    /// method returns, resolving against the mixin class file (where the closure
    /// actually lives) rather than the `::mixin(...)` registration site.
    ///
    /// A no-op when the mixin file cannot be read or the target class cannot be
    /// resolved.  `reg.name_offset` is an offset into `def_uri`'s content, so
    /// the file context and content must both come from that file.
    fn infer_mixin_macro_return_type(
        &self,
        reg: &mut crate::virtual_members::laravel::MacroRegistration,
        def_uri: &str,
    ) {
        let Some(content) = self.get_file_content(def_uri).or_else(|| {
            let path = tower_lsp::lsp_types::Url::parse(def_uri)
                .ok()?
                .to_file_path()
                .ok()?;
            std::fs::read_to_string(path).ok()
        }) else {
            return;
        };
        let Some(closure_text) = reg.closure_text.clone() else {
            return;
        };
        let Some(target_class) = self.find_or_load_class(&reg.target) else {
            return;
        };
        let file_ctx = self.file_context(def_uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        let rctx = crate::type_engine::resolver::ResolutionCtx {
            current_class: Some(target_class.as_ref()),
            all_classes: &file_ctx.classes,
            content: &content,
            cursor_offset: reg.name_offset,
            class_loader: &class_loader,
            backend: Some(self),
            laravel_macro_this_resolver: None,
            resolved_class_cache: Some(&self.resolved_class_cache),
            function_loader: Some(&function_loader),
            scope_var_resolver: None,
            is_in_static_method: false,
            preserve_static: true,
        };
        if let Some(ty) = Self::infer_closure_return_type(&closure_text, &rctx) {
            reg.method.return_type = Some(ty);
            reg.method.is_inferred_return = true;
        }
    }

    fn is_macro_helper_uri_allowed(&self, provider_uri: &str, helper_uri: &str) -> bool {
        let Ok(provider_url) = tower_lsp::lsp_types::Url::parse(provider_uri) else {
            return false;
        };
        let Ok(helper_url) = tower_lsp::lsp_types::Url::parse(helper_uri) else {
            return false;
        };
        let Ok(provider_path) = provider_url.to_file_path() else {
            return false;
        };
        let Ok(helper_path) = helper_url.to_file_path() else {
            return false;
        };

        // Vendor providers may live under the workspace root, so classify
        // package-local vendor helpers before the broader app-root check.
        if let Some(root) = self.vendor_package_root(&provider_path) {
            return helper_path.starts_with(&root);
        }

        if let Some(root) = self.workspace.workspace_root.read().clone()
            && provider_path.starts_with(&root)
        {
            return helper_path.starts_with(&root) && !self.is_in_vendor_dir(&helper_path);
        }

        false
    }

    fn vendor_package_root(&self, path: &std::path::Path) -> Option<std::path::PathBuf> {
        for vendor_dir in self.workspace.vendor_dir_paths.lock().iter() {
            if let Ok(rel) = path.strip_prefix(vendor_dir)
                && let mut comps = rel.components()
                && let (Some(vendor), Some(package)) = (comps.next(), comps.next())
            {
                return Some(vendor_dir.join(vendor).join(package));
            }
        }
        None
    }

    fn is_in_vendor_dir(&self, path: &std::path::Path) -> bool {
        self.workspace
            .vendor_dir_paths
            .lock()
            .iter()
            .any(|vendor_dir| path.starts_with(vendor_dir))
    }

    pub(crate) fn reload_laravel_schema_index(&self, root: &std::path::Path) {
        if let Ok(cfg) = crate::config::load_config(root) {
            *self.workspace.config.lock() = cfg;
        }

        let laravel_config = self.config().laravel;
        let index = if laravel_config.schema.enabled() || laravel_config.migrations.enabled() {
            let bp_macros = self.laravel_macros.read().blueprint_macro_closures();
            match crate::virtual_members::laravel::database_schema::load_schema_index(
                root,
                &laravel_config,
                &bp_macros,
            ) {
                Ok(index) => index,
                Err(err) => {
                    tracing::warn!("Failed to reload Laravel schema dumps: {}", err);
                    return;
                }
            }
        } else {
            crate::virtual_members::laravel::database_schema::SchemaIndex::default()
        };

        self.resolved_class_cache
            .write()
            .set_schema_index(index.clone());
        *self.schema_index.write() = index;
        self.resolved_class_cache.write().clear();
        self.member_completion_cache.lock().clear();
    }

    pub(crate) fn update_laravel_migrations(&self, changes: &[(PathBuf, FileChangeType)]) {
        let mut index = self.schema_index.write();
        let mut any_changed = false;
        for (path, change_type) in changes {
            if *change_type == FileChangeType::DELETED {
                if index.remove_migration_file(path) {
                    any_changed = true;
                }
            } else {
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        index.update_migration_file(path, content);
                        any_changed = true;
                    }
                    Err(err) => {
                        tracing::warn!("Failed to read migration file {}: {}", path.display(), err);
                    }
                }
            }
        }
        if any_changed {
            self.resolved_class_cache
                .write()
                .set_schema_index(index.clone());
            self.resolved_class_cache.write().clear();
            self.member_completion_cache.lock().clear();
        }
    }
}

#[cfg(test)]
mod coalesce_tests {
    use crate::Backend;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// A burst of concurrent whole-file requests for the same `(kind, uri)`
    /// must coalesce: only a small number actually compute, and the rest
    /// short-circuit. This is the mechanism that stops a keystroke burst from
    /// piling up un-cancellable full-file scans and starving completion.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn coalesced_whole_file_collapses_a_burst() {
        const N: usize = 20;

        let backend = Arc::new(Backend::new_test());
        let computes = Arc::new(AtomicUsize::new(0));

        // Fire N concurrent requests for the same kind+uri. Each "computation"
        // is deliberately slow so the whole burst arrives while the first one
        // is still running — exactly the editor's keystroke-burst pattern.
        let mut handles = Vec::new();
        for _ in 0..N {
            let b = Arc::clone(&backend);
            let c = Arc::clone(&computes);
            handles.push(tokio::spawn(async move {
                b.coalesced_whole_file("test_kind", "file:///burst.php", move || {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(100));
                    Ok(Some(n))
                })
                .await
            }));
        }

        let mut results = Vec::new();
        for h in handles {
            results.push(h.await.unwrap().unwrap());
        }

        let computed = computes.load(Ordering::SeqCst);
        assert!(computed >= 1, "at least one request must actually compute");
        assert!(
            computed < N,
            "burst should coalesce: {computed} of {N} requests computed (no coalescing)"
        );
        // The latest request always gets a freshly computed value; superseded
        // ones get either the cached value or None, but never block forever.
        assert!(
            results.iter().any(|r| r.is_some()),
            "at least one request must return a result"
        );
    }

    /// Requests for *different* files are not serialised against each other:
    /// distinct `(kind, uri)` keys each compute independently.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn coalesced_whole_file_is_per_uri() {
        let backend = Arc::new(Backend::new_test());
        let computes = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for i in 0..4 {
            let b = Arc::clone(&backend);
            let c = Arc::clone(&computes);
            let uri = format!("file:///file{i}.php");
            handles.push(tokio::spawn(async move {
                b.coalesced_whole_file("test_kind", &uri, move || {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(Some(i))
                })
                .await
            }));
        }
        for h in handles {
            let _ = h.await.unwrap().unwrap();
        }

        assert_eq!(
            computes.load(Ordering::SeqCst),
            4,
            "each distinct file must compute independently"
        );
    }
}
