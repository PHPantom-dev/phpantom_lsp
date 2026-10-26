//! Workspace startup: what `initialize` and `initialized` do, and the
//! background index they hand off to.
//!
//! The `LanguageServer` trait methods in `server.rs` delegate straight here.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp_server::gen_lsp_types::*;
use tower_lsp_server::jsonrpc::Result;

use crate::Backend;
use crate::backend::ClientCaps;
use crate::composer;
use crate::config::IndexingStrategy;
use crate::server::{run_blocking_cancel_safe, type_hierarchy_registration};

impl Backend {
    pub(crate) async fn on_initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // `rootUri` is deprecated in favour of `workspaceFolders`, but it is
        // what every client still sends and what the workspace root has
        // always been read from. A client that sends only the folders gets
        // the first one, which is what the others set `rootUri` to.
        let folders = match &params.workspace_folders_initialize_params.workspace_folders {
            Some(WorkspaceFolders::WorkspaceFolderList(folders)) => folders.as_slice(),
            Some(WorkspaceFolders::Null) | None => &[],
        };
        #[allow(deprecated)]
        let workspace_root = params
            .root_uri
            .as_ref()
            .and_then(crate::util::url_to_path)
            .or_else(|| {
                folders
                    .first()
                    .and_then(|folder| crate::util::url_to_path(&folder.uri))
            });

        if let Some(root) = workspace_root {
            *self.workspace.workspace_root.write() = Some(root);
        }

        // The other folders are indexed beside the root once discovery
        // runs in `initialized`.
        self.set_workspace_folders(folders);

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

        // The stub directory an editor extension ships, scanned with the
        // `[stubs]` paths once discovery runs.
        if let Some(options) = params
            .initialization_options
            .as_ref()
            .and_then(crate::config::ClientStubOptions::from_client_settings)
        {
            self.set_client_stub_options(options);
        }

        let caps = ClientCaps::from_capabilities(&params.capabilities);
        // A client that can register type hierarchy dynamically gets it
        // from `initialized` instead, so it is never registered twice.
        let type_hierarchy_provider = (!caps.type_hierarchy_dynamic_registration)
            .then_some(TypeHierarchyProvider::Bool(true));
        *self.client_caps.write() = caps;

        Ok(InitializeResult {
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
                inlay_hint_provider: Some(InlayHintProvider::Bool(true)),
                text_document_sync: Some(TextDocumentSync::Options(TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::Incremental),
                    will_save: None,
                    will_save_wait_until: None,
                    save: Some(Save::SaveOptions(SaveOptions {
                        include_text: Some(false),
                    })),
                })),
                hover_provider: Some(HoverProvider::Bool(true)),
                definition_provider: Some(DefinitionProvider::Bool(true)),
                type_definition_provider: Some(TypeDefinitionProvider::Bool(true)),
                implementation_provider: Some(ImplementationProvider::Bool(true)),
                references_provider: Some(ReferencesProvider::Bool(true)),
                call_hierarchy_provider: Some(CallHierarchyProvider::Bool(true)),
                type_hierarchy_provider,
                document_highlight_provider: Some(DocumentHighlightProvider::Bool(true)),
                code_action_provider: Some(CodeActionProvider::CodeActionOptions(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QuickFix,
                            CodeActionKind::RefactorExtract,
                            CodeActionKind::RefactorInline,
                            CodeActionKind::SourceOrganizeImports,
                            CodeActionKind::new("source.addMissingImports"),
                        ]),
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: None,
                        },
                        resolve_provider: Some(true),
                        documentation: None,
                    },
                )),
                rename_provider: Some(RenameProvider::RenameOptions(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: None,
                    },
                })),
                document_symbol_provider: Some(DocumentSymbolProvider::Bool(true)),
                workspace_symbol_provider: Some(WorkspaceSymbolProvider::Bool(true)),
                folding_range_provider: Some(FoldingRangeProvider::Bool(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: None,
                    },
                }),
                selection_range_provider: Some(SelectionRangeProvider::Bool(true)),
                document_formatting_provider: Some(DocumentFormattingProvider::Bool(true)),
                document_range_formatting_provider: Some(DocumentRangeFormattingProvider::Bool(
                    true,
                )),
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
                semantic_tokens_provider: Some(SemanticTokensProvider::SemanticTokensOptions(
                    SemanticTokensOptions {
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: None,
                        },
                        legend: crate::semantic_tokens::legend(),
                        range: Some(SemanticTokensOptionsRange::Bool(false)),
                        full: Some(Full::Bool(true)),
                    },
                )),
                // Advertised to every client: one that cannot pull ignores
                // it, and a push client that pulls anyway is answered from
                // the set last published.  Delivery mode still follows the
                // client's own `textDocument.diagnostic` declaration.
                diagnostic_provider: Some(DiagnosticProvider::DiagnosticOptions(
                    DiagnosticOptions {
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
                    },
                )),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["phpantom.navigateToPrototype".to_string()],
                    ..ExecuteCommandOptions::default()
                }),
                // Without this a client sends the folder list at startup
                // but never says when a folder is added or removed.
                workspace: Some(WorkspaceOptions {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(ChangeNotifications::Bool(true)),
                    }),
                    ..WorkspaceOptions::default()
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
        if !crate::stubs::has_embedded_stubs()
            && let Some(client) = &self.client
        {
            client
                .show_message(MessageType::Warning, crate::stubs::MISSING_STUBS_WARNING)
                .await;
        }

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
                        MessageType::Warning,
                        format!("Failed to load .phpantom.toml: {}", e),
                    )
                    .await;
                }
            }

            // Parse composer.json once up front.  The result is used for
            // PHP version detection and passed into init_single_project
            // so the file is never re-read during startup.
            let composer_package = match composer::try_read_composer_package(&root) {
                Ok(package) => package,
                Err(e) => {
                    self.log(
                        MessageType::Warning,
                        format!("{e}; PSR-4 mappings and the PHP version from it are ignored"),
                    )
                    .await;
                    None
                }
            };

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
            if self.resolved_class_cache.read().laravel().is_enabled() {
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
                    macro_backend.build_laravel_macro_index(&macro_backend.laravel_providers())
                })
                .await;

                let laravel_config = self.config().laravel;
                if laravel_config.schema.enabled() || laravel_config.migrations.enabled() {
                    let bp_macros = self.laravel.macros.read().blueprint_macro_closures();
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
                                .laravel_mut()
                                .set_schema_index(index.clone());
                            *self.laravel.schema_index.write() = index;
                        }
                        Some(Err(e)) => {
                            self.log(
                                MessageType::Warning,
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
            self.log(MessageType::Info, "PHPantom initialized!".to_string())
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
        let worker_backend = self.clone_for_blocking();
        tokio::spawn(async move {
            worker_backend.diagnostic_worker().await;
        });

        // Spawn the PHPStan worker as a separate background task.
        // PHPStan is extremely slow and resource-intensive, so it runs
        // in its own task with its own debounce timer and pending-URI
        // slot.  At most one PHPStan process runs at a time.  Native
        // diagnostics (fast + slow phases) are never blocked.
        let phpstan_backend = self.clone_for_blocking();
        tokio::spawn(async move {
            phpstan_backend.phpstan_worker().await;
        });

        // Spawn the PHPCS worker as a separate background task.
        // Same pattern as the PHPStan worker: dedicated task, own
        // debounce timer, single pending-URI slot.
        let phpcs_backend = self.clone_for_blocking();
        tokio::spawn(async move {
            phpcs_backend.phpcs_worker().await;
        });

        // Spawn the PHPMD worker.  Same pattern as PHPCS.
        let phpmd_backend = self.clone_for_blocking();
        tokio::spawn(async move {
            phpmd_backend.phpmd_worker().await;
        });

        // Spawn the Mago lint worker.  Same pattern as PHPCS: dedicated
        // task, own debounce timer, single pending-URI slot.  Mago lint
        // is fast (AST-level rules) so it uses the same debounce as PHPCS.
        let mago_lint_backend = self.clone_for_blocking();
        tokio::spawn(async move {
            mago_lint_backend.mago_lint_worker().await;
        });

        // Spawn the Mago analyze worker.  Mago analyze is slower
        // (type-aware) so it follows the PHPStan pattern with a longer
        // debounce.
        let mago_analyze_backend = self.clone_for_blocking();
        tokio::spawn(async move {
            mago_analyze_backend.mago_analyze_worker().await;
        });

        // Spawn the global config watcher. Unlike the project's own
        // `.phpantom.toml` (covered by the file watcher registered below),
        // the global config lives outside the workspace and has to be
        // polled directly; see `global_config_watcher` for why.
        if let Some(root) = self.workspace.workspace_root.read().clone() {
            let config_watcher_backend = self.clone_for_blocking();
            tokio::spawn(async move {
                config_watcher_backend.global_config_watcher(root).await;
            });
        }

        // ── Dynamic capability registration ─────────────────────────
        // Type hierarchy is registered here only for a client that
        // supports it; any other client got the static
        // `type_hierarchy_provider` capability from `initialize`.
        let mut registrations = Vec::new();

        if self.caps().type_hierarchy_dynamic_registration {
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
        registrations.extend(self.initial_watched_file_registration());

        // Sent from a task of its own rather than awaited here.  The
        // notifications that follow `initialized`, `didOpen` among them,
        // are not handled until it returns, so a client that is slow to
        // answer, or never does, would hold back every file's diagnostics.
        // And never put a timeout around the request: tower-lsp panics when
        // a reply arrives for a request whose future was dropped (see
        // `progress_create`).
        if !registrations.is_empty()
            && let Some(client) = self.client.clone()
        {
            tokio::spawn(async move {
                let _ = client.register_capability(registrations).await;
            });
        }

        // Clear the negative class-resolution cache.  During startup,
        // `did_open` may have triggered `update_ast` → `find_or_load_class`
        // before the fqn_uri_index was fully populated, caching
        // "not found" for classes that are now resolvable.  Without this
        // clear, those stale entries cause false-positive "Class not found"
        // diagnostics even though hover and go-to-definition (which run
        // later) resolve the same symbols correctly.
        self.clear_not_found_caches();

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
        self.laravel.clear_config_facts();

        // Scan project source for the remaining Laravel indexes (macros
        // were already scanned above, before the schema index load, so
        // `Blueprint` macro columns are present from the first load).
        if self.resolved_class_cache.read().laravel().is_enabled() {
            // Each of these parses the project's provider and Blade files, so
            // they run on the blocking pool: `initialized` is a notification
            // and tower-lsp cannot dispatch anything else while it runs.
            let index_backend = self.clone_for_blocking();
            let discovered = run_blocking_cancel_safe("build_laravel_indexes", move || {
                index_backend.build_laravel_provider_indexes();
                index_backend.build_laravel_command_index();
                index_backend.load_laravel_pivot_sources();

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
        let diagnostics_backend = self.clone_for_blocking();
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
        if self
            .workspace_index
            .full_in_progress
            .swap(true, Ordering::AcqRel)
        {
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
                parse_backend
                    .files
                    .read()
                    .values()
                    .filter(|state| state.symbol_map.is_some())
                    .count()
            })
            .await
            .unwrap_or(0);

            if let Some(poller) = poller {
                poller.finish().await;
            }

            progress_backend
                .workspace_index
                .full_in_progress
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

                // Then resolve each user file's member-access receivers,
                // which the warm class cache above makes far cheaper.  This
                // is what the session's first Find References or reference
                // CodeLens reads instead of walking every file that happens
                // to mention the name it is after.
                progress_backend.warm_member_reference_layer().await;
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
            if progress_backend.caps().pull_diagnostics {
                progress_backend.wait_for_first_workspace_pull().await;
                if progress_backend.shutdown_flag.load(Ordering::Acquire) {
                    return;
                }
            }
            progress_backend.run_workspace_diagnostics().await;
        });
    }
}
