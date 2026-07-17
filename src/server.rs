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
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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
use crate::classmap_scanner::{self, WorkspaceScanResult};
use crate::composer;
use crate::config::IndexingStrategy;
use crate::formatting;
use crate::phar;

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
        // Extract and store the workspace root path
        let workspace_root = params
            .root_uri
            .as_ref()
            .and_then(|uri| uri.to_file_path().ok());

        if let Some(root) = workspace_root {
            *self.workspace_root.write() = Some(root);
        }

        // Store the client name for quirks-mode adjustments.
        if let Some(info) = &params.client_info {
            *self.client_name.lock() = info.name.clone();
        }

        // Detect whether the client supports pull diagnostics.
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

        // Detect whether the client supports dynamic registration for
        // type hierarchy.
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
                        // Set to `true` only when the server can report
                        // diagnostics for files the user has not opened
                        // (e.g. project-wide PHPStan analysis).  Currently
                        // the workspace/diagnostic handler just mirrors
                        // per-file results, so `false` is accurate.
                        workspace_diagnostics: false,
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: None,
                        },
                    }))
                } else {
                    None
                },
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: self.name.clone(),
                version: Some(self.version.clone()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        // Parse composer.json for PSR-4 mappings if we have a workspace root
        let workspace_root = self.workspace_root.read().clone();

        if let Some(root) = workspace_root {
            // ── Load project configuration ──────────────────────────────
            // Read `.phpantom.toml` before anything else so that settings
            // (e.g. PHP version override, diagnostic toggles) are active
            // from the very first file load.
            match crate::config::load_config(&root) {
                Ok(cfg) => {
                    *self.config.lock() = cfg;
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

            let has_composer_json = composer_package.is_some();

            // ── Create a progress token for indexing feedback ────────
            let progress_token = self.progress_create("phpantom/indexing").await;
            if let Some(ref tok) = progress_token {
                self.progress_begin(tok, "PHPantom: Indexing", Some("Starting".to_string()))
                    .await;
            }

            if has_composer_json {
                // ── Single-project path (root composer.json exists) ──────
                self.init_single_project(
                    &root,
                    php_version,
                    composer_package,
                    progress_token.as_ref(),
                )
                .await;
            } else {
                // ── Monorepo / non-Composer path ────────────────────────
                let subprojects = composer::discover_subproject_roots(&root);

                if !subprojects.is_empty() {
                    self.init_monorepo(&root, &subprojects, php_version, progress_token.as_ref())
                        .await;
                } else {
                    // No subprojects found — pure non-Composer workspace.
                    self.init_no_composer(&root, php_version, progress_token.as_ref())
                        .await;
                }
            }

            // Warm the Eloquent Builder resolution cache only for Laravel
            // projects; a non-Laravel workspace has nothing to warm.
            if self.resolved_class_cache.read().is_laravel() {
                if let Some(ref tok) = progress_token {
                    self.progress_report(tok, 90, Some("Warming Laravel completions".to_string()))
                        .await;
                }
                let warmed = self.warm_laravel_completion_cache();
                if warmed > 0 {
                    tracing::info!("PHPantom: warmed {} Laravel completion classes", warmed);
                }
            }

            if let Some(ref tok) = progress_token {
                let classmap_count = self.fqn_uri_index.read().len();
                self.progress_end(tok, Some(format!("Indexed {} classes", classmap_count)))
                    .await;
            }
        } else {
            self.log(MessageType::INFO, "PHPantom initialized!".to_string())
                .await;
        }

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
        self.class_not_found_cache.write().clear();

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
            self.build_laravel_macro_index();
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
        self.diag_notify.notify_one();
        self.phpstan_notify.notify_one();
        self.phpcs_notify.notify_one();
        self.mago_lint_notify.notify_one();
        self.mago_analyze_notify.notify_one();
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

        // Store file content
        self.open_files
            .write()
            .insert(uri.clone(), Arc::clone(&text));

        // Parse and update AST map, use map, and namespace map
        self.update_ast(&uri, &text);

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
                    let start = crate::util::position_to_byte_offset(&current, range.start);
                    let end = crate::util::position_to_byte_offset(&current, range.end);
                    current.replace_range(start..end, &change.text);
                } else {
                    // Full content replacement (fallback)
                    current = change.text.clone();
                }
            }
            Arc::new(current)
        };

        // Update stored content
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

        // If the client sent the full text on save, update our copy.
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
        let workspace_root = self.workspace_root.read().clone();
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
            self.progress_begin(tok, "Go to Implementation", Some("Scanning…".to_string()))
                .await;
        }

        // Run on a blocking thread so the async runtime stays free to
        // flush progress notifications to the client.
        //
        // Wrapped in tokio::spawn for cancellation safety (see references handler).
        let backend = self.clone_for_blocking();
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
        let backend = self.clone_for_blocking();
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

        self.handle_with_position("linked_editing_range", &uri, position, |content, pos| {
            self.handle_linked_editing_range(&uri, content, pos)
                .map(|mut ler| {
                    ler.ranges = ler
                        .ranges
                        .into_iter()
                        .map(|r| Range {
                            start: self.translate_php_to_blade(&uri, r.start),
                            end: self.translate_php_to_blade(&uri, r.end),
                        })
                        .collect();
                    ler
                })
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
        let backend = self.clone_for_blocking();
        let item = params.item;
        let token = match params.work_done_progress_params.work_done_token {
            Some(t) => Some(t),
            None => self.progress_create("type_hierarchy_subtypes").await,
        };

        if let Some(ref tok) = token {
            self.progress_begin(tok, "Type Hierarchy", Some("Scanning…".to_string()))
                .await;
        }

        // Wrapped in tokio::spawn for cancellation safety (see references handler).
        let result = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || backend.subtypes_impl(&item))
                .await
                .unwrap_or(None)
        })
        .await
        .unwrap_or(None);

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
            Some(&function_loader),
        );

        Ok(edits)
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri.to_string();
        let config = self.config();

        // Read Composer metadata for require-dev detection and bin-dir.
        let workspace_root = self.workspace_root.read().clone();
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

        // Get the file content.
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
        let uri_str = params.text_document.uri.to_string();

        // Check resultId — if the client sends back the same resultId we
        // last returned AND the full cache is still present (not
        // invalidated by schedule_diagnostics), the diagnostics have not
        // changed and we can return Unchanged immediately.
        let cache_present = self.diag_last_full.lock().contains_key(&uri_str);
        if cache_present && let Some(prev_id) = &params.previous_result_id {
            let ids = self.diag_result_ids.lock();
            if let Some(&current_id) = ids.get(&uri_str)
                && prev_id == &current_id.to_string()
            {
                return Ok(DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Unchanged(RelatedUnchangedDocumentDiagnosticReport {
                        related_documents: None,
                        unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                            result_id: current_id.to_string(),
                        },
                    }),
                ));
            }
        }

        // In pull mode the pull request *triggers* native diagnostic
        // computation (no debounce — the IDE decided "now is the time").
        // If the full cache is missing for this URI, run the native
        // pipeline immediately and block until it finishes.  External
        // tool results (PHPStan, PHPCS, Mago) are delivered
        // incrementally via publishDiagnostics as each finishes; we do
        // not block on them here to keep the pull response fast.
        let needs_compute = {
            let cache = self.diag_last_full.lock();
            !cache.contains_key(&uri_str)
        };

        if needs_compute {
            self.trigger_diagnostics_for_pull(&uri_str);
        }

        let (diagnostics, result_id) = {
            let cache = self.diag_last_full.lock();
            let ids = self.diag_result_ids.lock();
            let diags = cache.get(&uri_str).cloned().unwrap_or_default();
            let rid = ids.get(&uri_str).copied().unwrap_or(0).to_string();
            (diags, rid)
        };

        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: Some(result_id),
                    items: diagnostics,
                },
            }),
        ))
    }

    async fn workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReportResult> {
        // Build a set of previous result IDs sent by the client so we
        // can return Unchanged for files that haven't changed.
        let previous: HashMap<&str, &str> = params
            .previous_result_ids
            .iter()
            .map(|p| (p.uri.as_str(), p.value.as_str()))
            .collect();

        let open_uris: Vec<String> = {
            let files = self.open_files.read();
            files.keys().cloned().collect()
        };

        let mut items = Vec::new();

        for uri_str in &open_uris {
            // Read the current resultId for this file.
            let current_id = {
                let ids = self.diag_result_ids.lock();
                ids.get(uri_str.as_str()).copied().unwrap_or(0)
            };

            // Check if the client already has up-to-date diagnostics.
            // The resultId must match AND the full cache must be present.
            // When `schedule_diagnostics` invalidates the cache (removing
            // diag_last_full), the resultId is intentionally kept so it
            // doesn't reset to 0.  But we must not return "unchanged"
            // when the cache is missing — that means fresh computation
            // is needed.
            let cache_present = self.diag_last_full.lock().contains_key(uri_str.as_str());
            if cache_present
                && let Some(prev_id) = previous.get(uri_str.as_str())
                && *prev_id == current_id.to_string()
            {
                let uri = match uri_str.parse::<Url>() {
                    Ok(u) => u,
                    Err(_) => continue,
                };
                items.push(WorkspaceDocumentDiagnosticReport::Unchanged(
                    WorkspaceUnchangedDocumentDiagnosticReport {
                        uri,
                        version: None,
                        unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                            result_id: current_id.to_string(),
                        },
                    },
                ));
                continue;
            }

            // If the cache is missing, trigger computation (same as
            // textDocument/diagnostic).
            let needs_compute = {
                let cache = self.diag_last_full.lock();
                !cache.contains_key(uri_str.as_str())
            };
            if needs_compute {
                self.trigger_diagnostics_for_pull(uri_str);
            }

            let diagnostics = {
                let cache = self.diag_last_full.lock();
                cache.get(uri_str.as_str()).cloned().unwrap_or_default()
            };

            // Re-read the resultId after potential computation.
            let current_id = {
                let ids = self.diag_result_ids.lock();
                ids.get(uri_str.as_str()).copied().unwrap_or(0)
            };

            let uri = match uri_str.parse::<Url>() {
                Ok(u) => u,
                Err(_) => continue,
            };

            items.push(WorkspaceDocumentDiagnosticReport::Full(
                WorkspaceFullDocumentDiagnosticReport {
                    uri,
                    version: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: Some(current_id.to_string()),
                        items: diagnostics,
                    },
                },
            ));
        }

        Ok(WorkspaceDiagnosticReportResult::Report(
            WorkspaceDiagnosticReport { items },
        ))
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
        let _chain_guard = crate::completion::resolver::with_chain_resolution_cache();
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
            .with_file_content(handler_name, uri, Some(position), |content, pos| {
                f(content, pos.unwrap())
            })
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

    /// Pre-resolve Laravel's shared builder classes so the first
    /// `Model::query()->` or `Model::with()->` completion does not pay
    /// the full inheritance + mixin + patch cost on the editor hot path.
    ///
    /// This intentionally warms only framework-level classes. Per-model
    /// generic specialisations like `Builder<User>` still depend on the
    /// concrete model and are resolved on demand.
    fn warm_laravel_completion_cache(&self) -> usize {
        let loader = |name: &str| self.find_or_load_class(name);
        let mut warmed = 0usize;

        for fqn in [
            crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN,
            "Illuminate\\Database\\Query\\Builder",
        ] {
            let Some(class_info) = self.find_or_load_class(fqn) else {
                continue;
            };
            crate::virtual_members::resolve_class_fully_cached(
                &class_info,
                &loader,
                &self.resolved_class_cache,
            );
            warmed += 1;
        }

        warmed
    }

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
    fn build_laravel_macro_index(&self) {
        let php_version = Some(*self.php_version.lock());

        let mut index = crate::virtual_members::laravel::LaravelMacroIndex::default();
        let mut candidate_uris: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut provider_uris: Vec<String> = Vec::new();
        let mut imported_uris: Vec<String> = Vec::new();

        // Scan a single file's content into the index, keyed by its URI.
        let scan_content = |index: &mut crate::virtual_members::laravel::LaravelMacroIndex,
                            uri: String,
                            content: &str| {
            if !self.laravel_macro_token_cached(&uri, content) {
                return;
            }
            let mut regs =
                crate::virtual_members::laravel::extract_macro_registrations(content, php_version);
            if regs.is_empty() {
                return;
            }
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
                continue;
            };
            scan_content(&mut index, uri.clone(), &content);

            for imported_fqn in
                crate::virtual_members::laravel::parse_provider_referenced_classes(&content)
            {
                let Some(imported_uri) = self.resolve_class_uri(&imported_fqn) else {
                    continue;
                };
                if !self.is_laravel_macro_helper_uri_allowed(uri, &imported_uri) {
                    continue;
                }
                if candidate_uris.insert(imported_uri.clone()) {
                    imported_uris.push(imported_uri);
                }
            }
        }

        for uri in &imported_uris {
            let Some(content) = self.get_file_content(uri) else {
                continue;
            };
            scan_content(&mut index, uri.clone(), &content);
        }

        index.rebuild();
        let has_macros = !index.is_empty();
        let target_count = index.target_fqns().len();
        *self.laravel_macros.write() = index;
        self.laravel_has_macros
            .store(has_macros, std::sync::atomic::Ordering::Relaxed);
        tracing::info!(
            "PHPantom: scanned {} Laravel macro candidates ({} providers, {} imported classes), indexed {} macro targets",
            candidate_uris.len(),
            provider_uris.len(),
            imported_uris.len(),
            target_count,
        );
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

        for vendor_dir in self.vendor_dir_paths.lock().iter() {
            let installed = vendor_dir.join("composer").join("installed.json");
            if let Ok(content) = std::fs::read_to_string(&installed) {
                for fqn in crate::virtual_members::laravel::parse_installed_providers(&content) {
                    push(&mut fqns, fqn);
                }
            }
        }

        if let Some(root) = self.workspace_root.read().clone() {
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
        if let Some(uri) = self.fqn_uri_index.read().get(fqn).cloned() {
            return Some(uri);
        }
        // Not indexed yet: loading the class populates its FQN → URI entry.
        self.find_or_load_class(fqn);
        self.fqn_uri_index.read().get(fqn).cloned()
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
        if self.is_laravel_macro_seed_uri(uri) {
            self.build_laravel_macro_index();
            return;
        }
        let had = self.laravel_macros.read().has_uri(uri);
        let has_token = memchr::memmem::find(content.as_bytes(), b"macro(").is_some();
        if !had && !has_token {
            return;
        }

        let php_version = Some(*self.php_version.lock());
        let mut regs =
            crate::virtual_members::laravel::extract_macro_registrations(content, php_version);
        // A macro registered through a facade also attaches to the facade's
        // concrete container-bound class.
        self.expand_facade_macros(&mut regs);

        let targets = {
            let mut index = self.laravel_macros.write();
            index.set_file(uri.to_string(), regs);
            index.rebuild();
            self.laravel_has_macros
                .store(!index.is_empty(), std::sync::atomic::Ordering::Relaxed);
            index.target_fqns()
        };

        // Evict every class a macro attaches to so the next resolution picks
        // up the change instead of a stale cached merge.
        let mut cache = self.resolved_class_cache.write();
        for fqn in targets {
            crate::virtual_members::evict_fqn(&mut cache, &fqn);
        }
    }

    fn is_laravel_macro_seed_uri(&self, uri: &str) -> bool {
        if let Some(root) = self.workspace_root.read().clone() {
            for rel in ["bootstrap/providers.php", "config/app.php"] {
                if crate::util::path_to_uri(&root.join(rel)) == uri {
                    return true;
                }
            }
        }

        for fqn in self.laravel_provider_fqns() {
            if self.resolve_class_uri(&fqn).as_deref() == Some(uri) {
                return true;
            }
        }

        false
    }

    fn is_laravel_macro_helper_uri_allowed(&self, provider_uri: &str, helper_uri: &str) -> bool {
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

        if let Some(root) = self.workspace_root.read().clone()
            && provider_path.starts_with(&root)
        {
            return helper_path.starts_with(&root) && !self.is_in_vendor_dir(&helper_path);
        }

        false
    }

    fn vendor_package_root(&self, path: &std::path::Path) -> Option<std::path::PathBuf> {
        for vendor_dir in self.vendor_dir_paths.lock().iter() {
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
        self.vendor_dir_paths
            .lock()
            .iter()
            .any(|vendor_dir| path.starts_with(vendor_dir))
    }

    /// Initialize a single-project workspace (root `composer.json` exists).
    ///
    /// This is the standard fast path: read PSR-4 mappings, build the
    /// classmap, scan autoload files.  Unchanged from the pre-monorepo
    /// behaviour except that vendor fields are now collections.
    pub(crate) async fn init_single_project(
        &self,
        root: &std::path::Path,
        php_version: crate::types::PhpVersion,
        composer_json: Option<composer::ComposerPackage>,
        progress_token: Option<&NumberOrString>,
    ) {
        if let Some(tok) = progress_token {
            self.progress_report(tok, 10, Some("Reading composer.json".to_string()))
                .await;
        }

        // Classify the project so Laravel-specific resolution (Eloquent
        // members, config/view/route keys, contract bindings, patches) is
        // skipped when no Laravel/Illuminate dependency is present.
        let is_laravel = composer_json
            .as_ref()
            .map(composer::is_laravel_project)
            .unwrap_or(false);
        self.resolved_class_cache.write().set_laravel(is_laravel);

        let (mappings, vendor_dir) = match &composer_json {
            Some(pkg) => {
                let mappings = composer::extract_psr4_mappings_from_package(pkg);
                let vendor_dir = composer::get_vendor_dir(pkg);
                (mappings, vendor_dir)
            }
            None => (Vec::new(), "vendor".to_string()),
        };

        // Cache the vendor dir path so cross-file scans can skip it
        // without re-reading composer.json on every request.
        let vendor_path = root.join(&vendor_dir);
        self.add_vendor_dir(&vendor_path);

        // Include PSR-4 mappings from path-repository packages (local
        // packages symlinked into vendor/, e.g. internachi/modular modules).
        let path_repo_mappings = composer::extract_path_repo_psr4_mappings(root, &vendor_dir);
        let mut all_mappings = mappings;
        all_mappings.extend(path_repo_mappings);
        // Keep the merged list longest-prefix-first so path-repo namespaces
        // are matched before any shorter root prefix (e.g. an empty-prefix
        // root fallback).
        all_mappings.sort_by_key(|m| std::cmp::Reverse(m.prefix.len()));
        *self.psr4_mappings.write() = all_mappings;

        // ── Build the classmap ──────────────────────────────────────
        let strategy = self.config().indexing.strategy();

        if let Some(tok) = progress_token {
            self.progress_report(tok, 20, Some("Building class index".to_string()))
                .await;
        }

        let explicit_deps = composer_json
            .as_ref()
            .map(crate::composer::explicit_dependency_names)
            .unwrap_or_default();

        let (classmap, source_label) = match strategy {
            IndexingStrategy::None => {
                let cm = composer::parse_autoload_classmap(root, &vendor_dir);
                (cm, "composer")
            }
            IndexingStrategy::SelfScan | IndexingStrategy::Full => {
                // "self" strategy: scan every PHP file under the
                // workspace root (ignoring .gitignore, hidden dirs,
                // etc.) to discover all classes, functions, and
                // constants — regardless of whether they appear in
                // composer.json's autoload sections.
                //
                // Explicitly skip the vendor directory so it is never
                // walked even when it is not in .gitignore.  Vendor
                // packages are scanned separately via installed.json
                // so that third-party classes are still indexed.
                let mut skip_dirs = HashSet::new();
                skip_dirs.insert(vendor_path.clone());
                let mut scan = classmap_scanner::scan_workspace_fallback_full(root, &skip_dirs);

                // Merge vendor packages (excluded from the workspace
                // walk above, scanned separately here).
                let vendor_scan = classmap_scanner::scan_vendor_packages_with_skip(
                    root,
                    &vendor_dir,
                    &HashSet::new(),
                    &explicit_deps,
                );
                for (fqcn, path) in vendor_scan.classmap {
                    scan.classmap.entry(fqcn).or_insert(path);
                }
                for (fqn, path) in vendor_scan.function_index {
                    scan.function_index.entry(fqn).or_insert(path);
                }
                for (name, path) in vendor_scan.constant_index {
                    scan.constant_index.entry(name).or_insert(path);
                }

                self.populate_autoload_indices(&scan);
                (scan.classmap, "self-scan")
            }
            IndexingStrategy::Composer => {
                // ── Merged classmap + self-scan pipeline ─────────────
                let composer_cm = composer::parse_autoload_classmap(root, &vendor_dir);
                let skip_paths: HashSet<PathBuf> = composer_cm.values().cloned().collect();
                let scan = self.build_self_scan_composer(
                    root,
                    &vendor_dir,
                    composer_json.as_ref(),
                    &skip_paths,
                );
                self.populate_autoload_indices(&scan);
                let mut merged = composer_cm;
                for (fqcn, path) in scan.classmap {
                    merged.entry(fqcn).or_insert(path);
                }
                (merged, "composer+scan")
            }
        };

        let vendor_package_roots =
            classmap_scanner::vendor_package_roots(root, &vendor_dir, &explicit_deps);

        let class_entries: Vec<(String, PathBuf)> = classmap.into_iter().collect();
        let symbol_count = class_entries.len();
        {
            let mut idx = self.fqn_uri_index.write();
            let mut origins = self.fqn_origin_index.write();
            origins.clear();
            for (fqn, path) in class_entries {
                let origin = classify_class_origin(&path, &vendor_path, &vendor_package_roots);
                origins.insert(fqn.clone(), origin);
                idx.or_insert_with(fqn, || crate::util::path_to_uri(&path));
            }
        }
        // Cache the package roots so path-based origin lookups
        // (functions, constants) can classify lazily parsed symbols.
        *self.vendor_package_origin_roots.write() = vendor_package_roots;

        // ── Drupal: scan web-root directories (gitignore bypassed) ──
        // Drupal's .gitignore excludes web/core, web/modules/contrib,
        // etc. because they are managed by Composer — but those paths
        // contain every base interface and hook definition that modules
        // depend on.  detect_drupal_web_root() returns None for
        // non-Drupal projects so this block is a no-op in that case.
        if let Some(ref pkg) = composer_json
            && let Some(drupal_web_root) = composer::detect_drupal_web_root(root, pkg)
        {
            let drupal_result = classmap_scanner::scan_drupal_directories(&drupal_web_root);
            let drupal_count = drupal_result.classmap.len()
                + drupal_result.function_index.len()
                + drupal_result.constant_index.len();
            {
                let mut idx = self.fqn_uri_index.write();
                for (fqn, path) in drupal_result.classmap {
                    idx.or_insert_with(fqn, || crate::util::path_to_uri(&path));
                }
            }
            {
                let mut fi = self.autoload_function_index.write();
                for (fqn, path) in drupal_result.function_index {
                    fi.or_insert_with(fqn, || path);
                }
            }
            {
                let mut ci = self.autoload_constant_index.write();
                for (name, path) in drupal_result.constant_index {
                    ci.entry(name).or_insert(path);
                }
            }
            tracing::info!(
                "PHPantom: Drupal web root {:?}, {} symbols indexed",
                drupal_web_root,
                drupal_count
            );
        }

        // ── PSR-0 (legacy) classmap ─────────────────────────────────
        // Packages that declare `autoload.psr-0` in their composer.json
        // (e.g. HTMLPurifier) are listed in `autoload_namespaces.php`.
        // Scan the listed directories and merge discovered classes into
        // the classmap so they are resolvable via `find_or_load_class`.
        let psr0_cm = composer::parse_autoload_namespaces(root, &vendor_dir);
        if !psr0_cm.is_empty() {
            let count = psr0_cm.len();
            let mut idx = self.fqn_uri_index.write();
            for (fqn, path) in psr0_cm {
                idx.or_insert_with(fqn, || crate::util::path_to_uri(&path));
            }
            tracing::info!("PSR-0: {} classes from autoload_namespaces.php", count);
        }

        // ── Autoload files ──────────────────────────────────────────
        if let Some(tok) = progress_token {
            self.progress_report(tok, 70, Some("Scanning autoload files".to_string()))
                .await;
        }

        self.scan_autoload_files(root, &vendor_dir);

        let symbol_count = symbol_count
            + self.autoload_function_index.read().len()
            + self.autoload_constant_index.read().len();

        self.log(
            MessageType::INFO,
            format!(
                "PHPantom v{}: PHP {}, {} symbols from {}, stubs {}",
                self.version,
                php_version,
                symbol_count,
                source_label,
                crate::stubs::STUBS_VERSION
            ),
        )
        .await;
    }

    /// Initialize a monorepo workspace (no root `composer.json`, but
    /// subprojects with their own `composer.json` were discovered).
    ///
    /// Each subproject is processed through the Composer pipeline (PSR-4,
    /// classmap, autoload files, vendor packages).  After all subprojects
    /// are processed, a gitignore-aware full-scan picks up loose PHP files
    /// outside any subproject directory.
    async fn init_monorepo(
        &self,
        root: &std::path::Path,
        subprojects: &[(PathBuf, String)],
        php_version: crate::types::PhpVersion,
        progress_token: Option<&NumberOrString>,
    ) {
        // Log the discovered subprojects.
        let sub_list: Vec<String> = subprojects
            .iter()
            .filter_map(|(p, _)| {
                p.strip_prefix(root)
                    .ok()
                    .map(|r| format!("  {}", r.display()))
            })
            .collect();
        self.log(
            MessageType::INFO,
            format!(
                "PHPantom: No root composer.json. Found {} Composer project(s):\n{}",
                subprojects.len(),
                sub_list.join("\n")
            ),
        )
        .await;

        // Collect subproject root paths for the skip set.
        let mut skip_dirs: HashSet<PathBuf> = HashSet::new();
        let sub_count = subprojects.len();

        // The workspace is treated as Laravel when any subproject depends on
        // Laravel/Illuminate, so Laravel-specific resolution runs there while
        // pure non-Laravel workspaces skip it.
        let mut any_laravel = false;

        for (sub_idx, (sub_root, vendor_dir)) in subprojects.iter().enumerate() {
            // Report per-subproject progress.  Reserve 10..80 for the
            // subproject loop, leaving 80..95 for the loose-file scan.
            if let Some(tok) = progress_token {
                let pct = 10 + (sub_idx as u32 * 70) / sub_count.max(1) as u32;
                let label = sub_root
                    .strip_prefix(root)
                    .unwrap_or(sub_root)
                    .display()
                    .to_string();
                self.progress_report(
                    tok,
                    pct,
                    Some(format!(
                        "Indexing subproject {} / {}: {}",
                        sub_idx + 1,
                        sub_count,
                        label
                    )),
                )
                .await;
            }
            skip_dirs.insert(sub_root.clone());

            if !any_laravel
                && let Some(pkg) = composer::read_composer_package(sub_root)
                && composer::is_laravel_project(&pkg)
            {
                any_laravel = true;
            }

            // ── PSR-4 mappings ──────────────────────────────────────
            let (mappings, _) = composer::parse_composer_json(sub_root);

            // Resolve base_path values to absolute paths so that
            // resolve_class_path works regardless of workspace_root.
            let abs_mappings: Vec<composer::Psr4Mapping> = mappings
                .into_iter()
                .map(|m| {
                    let abs_base = sub_root.join(&m.base_path).to_string_lossy().to_string();
                    composer::Psr4Mapping {
                        prefix: m.prefix,
                        base_path: composer::normalise_path(&abs_base),
                    }
                })
                .collect();
            {
                let mut psr4 = self.psr4_mappings.write();
                psr4.extend(abs_mappings);
            }

            // ── Vendor dir tracking ─────────────────────────────────
            let vendor_path = sub_root.join(vendor_dir);
            self.add_vendor_dir(&vendor_path);

            // ── Autoload files ──────────────────────────────────────
            self.scan_autoload_files(sub_root, vendor_dir);

            // ── Merged classmap + self-scan ──────────────────────────
            // Load the subproject's Composer classmap as a skip set,
            // then self-scan its PSR-4 directories and vendor packages
            // for anything the classmap missed.
            let mut sub_cm = composer::parse_autoload_classmap(sub_root, vendor_dir);
            // Merge PSR-0 classes for this subproject.
            let psr0_cm = composer::parse_autoload_namespaces(sub_root, vendor_dir);
            for (fqn, path) in psr0_cm {
                sub_cm.entry(fqn).or_insert(path);
            }
            let sub_skip: HashSet<PathBuf> = sub_cm.values().cloned().collect();
            let scan = self.build_self_scan_composer(sub_root, vendor_dir, None, &sub_skip);
            self.populate_autoload_indices(&scan);
            {
                let mut idx = self.fqn_uri_index.write();
                for (fqcn, path) in sub_cm {
                    idx.or_insert_with(fqcn, || crate::util::path_to_uri(&path));
                }
                for (fqcn, path) in scan.classmap {
                    idx.or_insert_with(fqcn, || crate::util::path_to_uri(&path));
                }
            }
        }

        self.resolved_class_cache.write().set_laravel(any_laravel);

        // Re-sort PSR-4 mappings by prefix length descending so
        // longest-prefix-first matching works.
        {
            let mut psr4 = self.psr4_mappings.write();
            psr4.sort_by_key(|b| std::cmp::Reverse(b.prefix.len()));
        }

        // ── Full-scan loose files ───────────────────────────────────
        // Walk the workspace for PHP files outside any subproject
        // directory, using gitignore-aware walking.
        if let Some(tok) = progress_token {
            self.progress_report(tok, 80, Some("Scanning loose PHP files".to_string()))
                .await;
        }

        let scan = classmap_scanner::scan_workspace_fallback_full(root, &skip_dirs);
        self.populate_autoload_indices(&scan);
        {
            let mut idx = self.fqn_uri_index.write();
            for (fqcn, path) in scan.classmap {
                idx.or_insert_with(fqcn, || crate::util::path_to_uri(&path));
            }
        }

        let symbol_count = self.fqn_uri_index.read().len()
            + self.autoload_function_index.read().len()
            + self.autoload_constant_index.read().len();

        self.log(
            MessageType::INFO,
            format!(
                "PHPantom v{}: PHP {}, {} symbols from {} subprojects, stubs {}",
                self.version,
                php_version,
                symbol_count,
                subprojects.len(),
                crate::stubs::STUBS_VERSION
            ),
        )
        .await;
    }

    /// Initialize a pure non-Composer workspace (no `composer.json`
    /// anywhere).  Full-scans all PHP files in the workspace.
    async fn init_no_composer(
        &self,
        root: &std::path::Path,
        php_version: crate::types::PhpVersion,
        progress_token: Option<&NumberOrString>,
    ) {
        self.log(
            MessageType::INFO,
            "PHPantom: No composer.json found. Scanning workspace for PHP classes.".to_string(),
        )
        .await;

        if let Some(tok) = progress_token {
            self.progress_report(
                tok,
                20,
                Some("Scanning workspace for PHP files".to_string()),
            )
            .await;
        }

        // No composer.json means no Laravel/Illuminate dependency, so
        // Laravel-specific resolution is disabled.
        self.resolved_class_cache.write().set_laravel(false);

        let skip_dirs = HashSet::new();
        let scan = classmap_scanner::scan_workspace_fallback_full(root, &skip_dirs);
        self.populate_autoload_indices(&scan);

        let symbol_count = scan.classmap.len();
        {
            let mut idx = self.fqn_uri_index.write();
            for (fqn, path) in scan.classmap {
                idx.or_insert_with(fqn, || crate::util::path_to_uri(&path));
            }
        }

        let symbol_count = symbol_count
            + self.autoload_function_index.read().len()
            + self.autoload_constant_index.read().len();

        self.log(
            MessageType::INFO,
            format!(
                "PHPantom v{}: PHP {}, {} symbols from workspace scan, stubs {}",
                self.version,
                php_version,
                symbol_count,
                crate::stubs::STUBS_VERSION
            ),
        )
        .await;
    }

    /// Register a vendor directory path and its URI prefix for
    /// vendor-file detection.
    pub(crate) fn add_vendor_dir(&self, vendor_path: &std::path::Path) {
        // Store the absolute path for filesystem-level skip logic.
        {
            let mut paths = self.vendor_dir_paths.lock();
            paths.push(vendor_path.to_path_buf());
        }
        // Store the URI prefix for URI-level skip logic (diagnostics,
        // find references, rename).
        let prefix = if let Ok(canonical) = vendor_path.canonicalize() {
            format!("{}/", crate::util::path_to_uri(&canonical))
        } else {
            format!("{}/", crate::util::path_to_uri(vendor_path))
        };
        {
            let mut prefixes = self.vendor_uri_prefixes.lock();
            prefixes.push(prefix);
        }
    }

    /// Apply a `workspace/didChangeWatchedFiles` batch to the indexes.
    ///
    /// Returns `true` if any PHP file or composer change was acted on (so the
    /// caller can ask the editor to re-pull diagnostics).  Runs entirely on a
    /// blocking thread; it parses no files on the async runtime.
    ///
    /// Editors cannot watch the filesystem while the window is unfocused, so
    /// on refocus they resynchronise by reporting the *entire* workspace as
    /// "changed" in one notification (hundreds of KiB of events).  Almost
    /// none of those files actually changed, and most were never parsed:
    /// PHPantom loads class details lazily, holding only a name→file pointer
    /// in the discovery index until something resolves the class.  Re-reading
    /// and re-scanning every reported file from disk would do thousands of
    /// wasted syscalls on every refocus.
    ///
    /// So a plain content change is only acted on for files we have actually
    /// parsed (whose cached details would otherwise go stale).  Created and
    /// deleted files are always handled: a creation makes a new class
    /// discoverable, and a deletion must purge a now-dangling entry, both of
    /// which matter even for files we never loaded.
    pub(crate) fn apply_watched_file_changes(
        &self,
        params: &DidChangeWatchedFilesParams,
        root: &std::path::Path,
    ) -> bool {
        let mut composer_changed = false;
        let mut php_changes: Vec<(String, PathBuf, FileChangeType)> = Vec::new();
        {
            let open = self.open_files.read();
            let parsed = self.parsed_uris.read();
            for change in &params.changes {
                let path_str = change.uri.path();
                if path_str.ends_with("/composer.json") || path_str.ends_with("/composer.lock") {
                    composer_changed = true;
                    continue;
                }
                if !path_str.ends_with(".php") {
                    continue;
                }

                // Open files are already tracked via did_open/did_change.
                let uri_str = change.uri.to_string();
                if open.contains_key(&uri_str) {
                    continue;
                }
                let Ok(file_path) = change.uri.to_file_path() else {
                    continue;
                };

                if change.typ == FileChangeType::CHANGED {
                    // `parsed_uris` records the editor URI for open files and
                    // the canonical `file://` URI for lazily loaded ones;
                    // check both spellings.
                    let canonical_uri = crate::util::path_to_uri(&file_path);
                    let loaded =
                        parsed.contains(&uri_str) || parsed.contains(canonical_uri.as_str());
                    if !loaded {
                        continue;
                    }
                }

                php_changes.push((uri_str, file_path, change.typ));
            }
        }

        if php_changes.is_empty() && !composer_changed {
            return false;
        }

        if !php_changes.is_empty() {
            tracing::info!(
                "PHPantom: {} watched PHP file(s) changed on disk, refreshing indexes",
                php_changes.len()
            );
            self.reindex_files_batch(&php_changes);
            // A class that was previously "not found" may now exist, and
            // resolved class info / member completions may be stale for a
            // class whose file changed.
            self.class_not_found_cache.write().clear();
            self.resolved_class_cache.write().clear();
            self.auth_user_type_cache.write().clear();
            *self.laravel_aliases.write() = None;
            self.member_completion_cache.lock().clear();
        }

        if composer_changed {
            tracing::info!("PHPantom: composer files changed, rescanning vendor");
            self.rescan_composer_indexes(root);
        }

        true
    }

    /// Rebuild the vendor-derived indexes after a `composer.json` /
    /// `composer.lock` change (e.g. a `composer install` or `update`).
    ///
    /// Re-reads PSR-4 mappings, rebuilds the vendor classmap and the
    /// autoload function/constant indexes, rescans autoload files, and
    /// clears the resolved-class caches so stale vendor versions do not
    /// linger.  This is the synchronous body of
    /// [`did_change_watched_files`](Self::did_change_watched_files)'s
    /// composer branch, factored out so it can run on a blocking thread.
    pub(crate) fn rescan_composer_indexes(&self, root: &std::path::Path) {
        // Re-read composer.json for updated PSR-4 mappings.
        if let Some(pkg) = composer::read_composer_package(root) {
            let mut mappings = composer::extract_psr4_mappings_from_package(&pkg);
            let vendor_dir = composer::get_vendor_dir(&pkg);
            mappings.extend(composer::extract_path_repo_psr4_mappings(root, &vendor_dir));
            // Keep the merged list longest-prefix-first so path-repo
            // namespaces are matched before any shorter root prefix.
            mappings.sort_by_key(|m| std::cmp::Reverse(m.prefix.len()));
            *self.psr4_mappings.write() = mappings;

            let vendor_path = root.join(&vendor_dir);

            // Rebuild vendor classmap, tracking dependency provenance so
            // completion ranking stays accurate after a composer change.
            let explicit_deps = composer::explicit_dependency_names(&pkg);
            let vendor_scan = classmap_scanner::scan_vendor_packages_with_skip(
                root,
                &vendor_dir,
                &HashSet::new(),
                &explicit_deps,
            );
            let vendor_package_roots =
                classmap_scanner::vendor_package_roots(root, &vendor_dir, &explicit_deps);
            {
                let vendor_uri_prefix = if let Ok(canonical) = vendor_path.canonicalize() {
                    format!("{}/", crate::util::path_to_uri(&canonical))
                } else {
                    format!("{}/", crate::util::path_to_uri(&vendor_path))
                };

                // Remove old vendor entries and insert new ones.
                let mut idx = self.fqn_uri_index.write();
                let mut origins = self.fqn_origin_index.write();
                idx.retain(|_, v| !v.starts_with(&vendor_uri_prefix));
                for (fqn, path) in vendor_scan.classmap {
                    let origin = classify_class_origin(&path, &vendor_path, &vendor_package_roots);
                    origins.insert(fqn.clone(), origin);
                    idx.insert(fqn, crate::util::path_to_uri(&path));
                }
            }
            {
                let mut fi = self.autoload_function_index.write();
                let mut origins = self.autoload_function_origin_index.write();
                // Purge functions that pointed into the old vendor tree
                // before re-inserting, so symbols removed by a
                // `composer update` no longer resolve.
                fi.retain(|_, v| !v.starts_with(&vendor_path));
                for (fqn, path) in vendor_scan.function_index {
                    let origin = vendor_scan
                        .function_origins
                        .get(&fqn)
                        .copied()
                        .unwrap_or(crate::ClassCompletionOrigin::Project);
                    origins.insert(fqn.clone(), origin);
                    fi.insert(fqn, path);
                }
            }
            {
                let mut ci = self.autoload_constant_index.write();
                let mut origins = self.autoload_constant_origin_index.write();
                // Same for constants from the old vendor tree.
                ci.retain(|_, v| !v.starts_with(&vendor_path));
                for (name, path) in vendor_scan.constant_index {
                    let origin = vendor_scan
                        .constant_origins
                        .get(&name)
                        .copied()
                        .unwrap_or(crate::ClassCompletionOrigin::Project);
                    origins.insert(name.clone(), origin);
                    ci.insert(name, path);
                }
            }

            // Refresh the cached package roots for path-based lookups.
            *self.vendor_package_origin_roots.write() = vendor_package_roots;

            // Rescan autoload files (they may have changed).
            self.scan_autoload_files(root, &vendor_dir);
        }

        // Clear all cached class info since vendor classes may have
        // changed versions.
        self.fqn_class_index.write().clear();
        self.method_store.write().clear();
        self.gti_index.write().clear();
        self.class_not_found_cache.write().clear();
        self.resolved_class_cache.write().clear();
        self.member_completion_cache.lock().clear();
    }

    /// Scan autoload files for a single project root and populate the
    /// autoload indices.  Returns the number of autoload file entries
    /// found.
    pub(crate) fn scan_autoload_files(
        &self,
        project_root: &std::path::Path,
        vendor_dir: &str,
    ) -> usize {
        let autoload_files = composer::parse_autoload_files(project_root, vendor_dir);
        let autoload_count = autoload_files.len();

        // Some frameworks (e.g. CakePHP) ship global function aliases in a
        // `*_global.php` sibling that is loaded via the application
        // bootstrap rather than Composer's `files` autoload, so it never
        // appears in `autoload_files.php`. Seed those siblings too, so
        // globals like `__()`/`h()` are indexed instead of resolving to
        // "unknown function".
        let sibling_globals = composer::discover_global_sibling_files(&autoload_files);

        // Work queue + visited set for following require_once chains.
        let mut file_queue: Vec<PathBuf> = autoload_files;
        file_queue.extend(sibling_globals);
        let mut visited: HashSet<PathBuf> = HashSet::new();

        while let Some(file_path) = file_queue.pop() {
            // Canonicalise to avoid revisiting the same file via
            // different relative paths.
            let canonical = file_path.canonicalize().unwrap_or(file_path);
            if !visited.insert(canonical.clone()) {
                continue;
            }

            if let Ok(content) = std::fs::read(&canonical) {
                let uri = crate::util::path_to_uri(&canonical);

                // Lightweight byte-level scan: extract symbol names
                // without building a full AST.
                let scan = classmap_scanner::find_symbols(&content);

                // Populate function index.
                {
                    let mut idx = self.autoload_function_index.write();
                    for fqn in &scan.functions {
                        idx.or_insert_with(fqn.as_str(), || canonical.clone());
                    }
                }

                // Populate constant index.
                {
                    let mut idx = self.autoload_constant_index.write();
                    for name in &scan.constants {
                        idx.entry(name.clone()).or_insert_with(|| canonical.clone());
                    }
                }

                // Populate fqn_uri_index so find_or_load_class can
                // lazily parse these classes later.
                {
                    let mut idx = self.fqn_uri_index.write();
                    for fqn in &scan.classes {
                        idx.or_insert_with(fqn.as_str(), || uri.clone());
                    }
                }

                let content_str = String::from_utf8_lossy(&content);

                // ── Phar detection ──────────────────────────────────
                // If this autoload file references a .phar archive,
                // parse the phar and scan its PHP files for classes.
                if let Some(file_dir) = canonical.parent() {
                    let phar_paths = composer::detect_phar_references(&content_str, file_dir);
                    for phar_path in phar_paths {
                        self.scan_phar_archive(&phar_path);
                    }
                }

                // Follow require_once statements to discover more files.
                let require_paths = composer::extract_require_once_paths(&content_str);
                if let Some(file_dir) = canonical.parent() {
                    for rel_path in require_paths {
                        let resolved = file_dir.join(&rel_path);
                        if resolved.is_file() {
                            file_queue.push(resolved);
                        }
                    }
                }
            }
        }

        // Record the visited autoload file paths and eagerly parse them.
        //
        // The byte-level scan above only discovers symbols at brace
        // depth 0.  Functions guarded by `if (! function_exists(...))`
        // (common in Laravel and similar helper files) live at brace
        // depth > 0 and are missed.  Without a full parse they would only
        // be found by the last-resort fallback in `find_or_load_function`,
        // which blocks the first interactive request that needs such a
        // function while it serially parses every unparsed autoload file.
        //
        // Parsing them here in parallel moves that one-time cost to
        // startup.  The paths are still recorded so the fallback (which
        // skips already-parsed files) remains correct for anything that
        // slips through.
        let visited: Vec<PathBuf> = visited.into_iter().collect();
        {
            let mut paths = self.autoload_file_paths.write();
            paths.extend(visited.iter().cloned());
        }
        self.preload_autoload_files(&visited);

        autoload_count
    }

    /// Eagerly full-parse the given autoload helper files in parallel.
    ///
    /// [`scan_autoload_files`](Self::scan_autoload_files) only byte-scans
    /// these files, which misses functions defined inside
    /// `function_exists` guards.  A full parse populates `global_functions`
    /// with those guarded helpers so that
    /// [`find_or_load_function`](Self::find_or_load_function) resolves them
    /// via its fast path instead of falling back to a serial parse of
    /// every autoload file on the first interactive lookup.
    ///
    /// Files already present in `parsed_uris` are skipped.
    pub fn preload_autoload_files(&self, paths: &[PathBuf]) {
        // Skip files that have already been parsed (e.g. opened in the
        // editor before indexing reached them).
        let pending: Vec<&PathBuf> = paths
            .iter()
            .filter(|p| {
                let uri = crate::util::path_to_uri(p);
                !self.parsed_uris.read().contains(&uri)
            })
            .collect();

        let file_count = pending.len();
        if file_count == 0 {
            return;
        }

        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(file_count);
        let next_idx = std::sync::atomic::AtomicUsize::new(0);
        let pending = &pending;
        let next_idx = &next_idx;

        std::thread::scope(|s| {
            for _ in 0..n_threads {
                std::thread::Builder::new()
                    .name("autoload-preload".into())
                    .stack_size(32 * 1024 * 1024)
                    .spawn_scoped(s, move || {
                        loop {
                            let i = next_idx.fetch_add(1, Ordering::Relaxed);
                            if i >= file_count {
                                break;
                            }
                            let path = pending[i];
                            if let Ok(content) = std::fs::read_to_string(path) {
                                let uri = crate::util::path_to_uri(path);
                                self.update_ast(&uri, &content);
                            }
                        }
                    })
                    .expect("failed to spawn autoload-preload thread");
            }
        });
    }

    /// Parse a `.phar` archive and register its PHP classes in the
    /// fqn_uri_index for lazy loading.
    ///
    /// The phar's raw bytes are read from disk, parsed by
    /// [`phar::PharArchive`], and stored in
    /// [`phar_archives`](crate::Backend::phar_archives).  Each `.php`
    /// file inside the archive is scanned with the lightweight
    /// [`find_classes`](classmap_scanner::find_classes) byte scanner,
    /// and discovered classes are registered in:
    ///
    /// - `fqn_uri_index` — with a sentinel path like
    ///   `/path/to/phpstan.phar!src/Type/Type.php` (the `!` separator
    ///   tells [`parse_and_cache_file`](crate::Backend::parse_and_cache_file)
    ///   to extract content from the phar instead of reading from disk)
    ///   and a `phar://` URI for completions and workspace symbols
    fn scan_phar_archive(&self, phar_path: &Path) {
        // Avoid scanning the same phar twice.
        if self.phar_archives.read().contains_key(phar_path) {
            return;
        }

        let data = match std::fs::read(phar_path) {
            Ok(d) => d,
            Err(_) => return,
        };

        let archive = match phar::PharArchive::parse(data) {
            Some(a) => a,
            None => {
                tracing::warn!("failed to parse phar archive: {}", phar_path.display());
                return;
            }
        };

        // Collect PHP file paths first so we can iterate while
        // holding the archive reference.
        let php_files: Vec<String> = archive
            .file_paths()
            .filter(|p| p.ends_with(".php"))
            .map(String::from)
            .collect();

        let mut classmap_entries: Vec<(String, PathBuf)> = Vec::new();
        let mut fqn_uri_entries: Vec<(String, String)> = Vec::new();

        for internal_path in &php_files {
            if let Some(content) = archive.read_file(internal_path) {
                let classes = classmap_scanner::find_classes(content);
                for fqn in classes {
                    // Sentinel path: "archive.phar!internal/path.php"
                    let sentinel =
                        PathBuf::from(format!("{}!{}", phar_path.display(), internal_path));
                    let phar_uri = format!("phar://{}/{}", phar_path.display(), internal_path);
                    classmap_entries.push((fqn.clone(), sentinel));
                    fqn_uri_entries.push((fqn, phar_uri));
                }
            }
        }

        let class_count = classmap_entries.len();

        // Register classes in the fqn_uri_index.
        {
            let mut idx = self.fqn_uri_index.write();
            for (fqn, path) in classmap_entries {
                idx.or_insert_with(fqn, || crate::util::path_to_uri(&path));
            }
            for (fqn, uri) in fqn_uri_entries {
                idx.or_insert_with(fqn, || uri);
            }
        }

        // Clear the negative class cache so that classes previously
        // looked up (and cached as "not found") before the phar was
        // scanned can now be resolved.
        if class_count > 0 {
            self.class_not_found_cache.write().clear();
        }

        tracing::info!(
            "scanned phar {}: {} PHP files, {} classes",
            phar_path.display(),
            php_files.len(),
            class_count,
        );

        // Store the parsed archive for lazy content extraction.
        self.phar_archives
            .write()
            .insert(phar_path.to_owned(), archive);
    }

    /// Build a workspace scan by self-scanning a Composer project's
    /// autoload directories (PSR-4 + classmap + vendor packages).
    ///
    /// Used by the merged classmap + self-scan pipeline and by the
    /// `"self"` / `"full"` indexing strategies.  The `project_root`
    /// is the directory containing `composer.json` (either the
    /// workspace root for single-project, or a subproject root for
    /// monorepo).
    ///
    /// `skip_paths` contains absolute file paths that should be
    /// excluded from scanning (typically the file paths already
    /// present in the Composer classmap).  Pass an empty set to
    /// scan everything.
    pub(crate) fn build_self_scan_composer(
        &self,
        project_root: &std::path::Path,
        vendor_dir: &str,
        preloaded_package: Option<&composer::ComposerPackage>,
        skip_paths: &HashSet<PathBuf>,
    ) -> WorkspaceScanResult {
        // Use the pre-parsed package when available; only read from disk
        // as a fallback (e.g. monorepo subproject calls).
        let owned_package;
        let package = match preloaded_package {
            Some(p) => p,
            None => {
                owned_package = composer::read_composer_package(project_root);
                match owned_package.as_ref() {
                    Some(p) => p,
                    None => {
                        let skip_dirs = HashSet::new();
                        return classmap_scanner::scan_workspace_fallback_full(
                            project_root,
                            &skip_dirs,
                        );
                    }
                }
            }
        };

        let scan_dirs = composer::extract_scan_dirs(package);

        let psr4_dirs: Vec<(String, PathBuf)> = scan_dirs
            .psr4
            .iter()
            .map(|(prefix, dir)| (prefix.clone(), project_root.join(dir)))
            .collect();

        let classmap_dirs: Vec<PathBuf> = scan_dirs
            .classmap
            .iter()
            .map(|dir| project_root.join(dir))
            .collect();

        // Scan user source directories (classes only for PSR-4).
        let vendor_dir_paths = vec![project_root.join(vendor_dir)];
        let classmap = classmap_scanner::scan_psr4_directories_with_skip(
            &psr4_dirs,
            &classmap_dirs,
            &vendor_dir_paths,
            skip_paths,
        );

        // Scan vendor packages from installed.json.
        let explicit_deps = crate::composer::explicit_dependency_names(package);
        let vendor_scan = classmap_scanner::scan_vendor_packages_with_skip(
            project_root,
            vendor_dir,
            skip_paths,
            &explicit_deps,
        );

        let mut result = WorkspaceScanResult {
            classmap,
            ..Default::default()
        };

        for (fqcn, path) in vendor_scan.classmap {
            result.classmap.entry(fqcn).or_insert(path);
        }
        for (fqn, path) in vendor_scan.function_index {
            result.function_index.entry(fqn).or_insert(path);
        }
        for (name, path) in vendor_scan.constant_index {
            result.constant_index.entry(name).or_insert(path);
        }

        result
    }

    /// Store the function and constant indices from a workspace scan
    /// into the backend's shared maps.
    ///
    /// Only has an effect for non-Composer projects (the "no
    /// `composer.json`" scenario) where the full-scan populates
    /// function and constant entries.  For Composer projects the scan
    /// result's function and constant indices are empty because those
    /// symbols are discovered via the `autoload_files.php` scan loop
    /// in `initialized()` instead.
    pub(crate) fn populate_autoload_indices(&self, scan: &WorkspaceScanResult) {
        if !scan.function_index.is_empty() {
            let mut idx = self.autoload_function_index.write();
            let mut origins = self.autoload_function_origin_index.write();
            for (fqn, path) in &scan.function_index {
                idx.or_insert_with(fqn.as_str(), || path.clone());
                let origin = scan
                    .function_origins
                    .get(fqn)
                    .copied()
                    .unwrap_or(crate::ClassCompletionOrigin::Project);
                origins.insert(fqn.clone(), origin);
            }
        }
        if !scan.constant_index.is_empty() {
            let mut idx = self.autoload_constant_index.write();
            let mut origins = self.autoload_constant_origin_index.write();
            for (name, path) in &scan.constant_index {
                idx.entry(name.clone()).or_insert_with(|| path.clone());
                let origin = scan
                    .constant_origins
                    .get(name)
                    .copied()
                    .unwrap_or(crate::ClassCompletionOrigin::Project);
                origins.insert(name.clone(), origin);
            }
        }
    }
}

fn classify_class_origin(
    path: &Path,
    vendor_path: &Path,
    vendor_package_roots: &[(PathBuf, crate::ClassCompletionOrigin, String)],
) -> crate::ClassCompletionOrigin {
    if !path.starts_with(vendor_path) {
        return crate::ClassCompletionOrigin::Project;
    }
    for (root, origin, _pkg_name) in vendor_package_roots {
        if path.starts_with(root) {
            return *origin;
        }
    }
    crate::ClassCompletionOrigin::VendorTransitive
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
