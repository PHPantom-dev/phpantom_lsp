//! Workspace startup: what `initialize` and `initialized` do, and the
//! background index they hand off to.
//!
//! The `LanguageServer` trait methods in `server.rs` delegate straight here.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::composer;
use crate::config::IndexingStrategy;
use crate::server::{run_blocking_cancel_safe, type_hierarchy_registration};

impl Backend {
    pub(crate) async fn on_initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
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

        // A tree indexed through a symlink is not inside any workspace
        // folder, so being told about a change in it takes a watcher that
        // names the link.  Clients that predate LSP 3.17 get none, and a
        // `git pull` into a linked framework needs a reload there.
        let client_supports_relative_pattern_watchers = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.did_change_watched_files.as_ref())
            .and_then(|w| w.relative_pattern_support)
            .unwrap_or(false);
        self.supports_relative_pattern_watchers
            .store(client_supports_relative_pattern_watchers, Ordering::Release);

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

    pub(crate) async fn on_initialized(&self) {
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
        let (watched_files_registration, watched_file_inputs) =
            self.build_watched_file_registration();
        registrations.push(watched_files_registration);
        *self.registered_watcher_state.write() = Some(watched_file_inputs);

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
        self.clear_resolved_class_cache();
        self.auth_user_type_cache.write().clear();
        *self.storage_disk_type_cache.write() = None;
        self.laravel_aliases.invalidate();

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

            // The walk above covers the whole workspace root, so it is
            // where a symlink nested below the roots the Composer pipeline
            // walked first turns up.  Each one needs its own watcher, and
            // the registration built during `initialized` could only carry
            // the links known by then.
            progress_backend.reregister_watched_files_if_changed();

            if let Some(tok) = progress_token {
                progress_backend
                    .progress_end(&tok, Some(format!("Parsed {} files", indexed_files)))
                    .await;
            }

            progress_backend.request_diagnostic_refresh().await;

            // Files opened before the index finished were annotated from a
            // still-filling index: hints resolved against classes that were
            // not parsed yet, and lenses showed reference counts that were
            // stale zeros.  Now that it's complete, ask for a re-pull.
            progress_backend.request_inlay_hint_refresh().await;
            progress_backend.request_code_lens_refresh().await;

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
