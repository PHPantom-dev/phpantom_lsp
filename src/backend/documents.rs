//! Document lifecycle: what the `did_open` / `did_change` / `did_save` /
//! `did_close` / `did_change_watched_files` notifications do.
//!
//! The `LanguageServer` trait methods in `server.rs` delegate straight here.

use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::server::{run_blocking_cancel_safe, spawn_blocking_detached};

impl Backend {
    pub(crate) async fn on_did_open(&self, params: DidOpenTextDocumentParams) {
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

    pub(crate) async fn on_did_change(&self, params: DidChangeTextDocumentParams) {
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
                    // A range whose start lies after its end would panic
                    // `replace_range`, and this runs on the service loop
                    // where a panic takes the whole server down.
                    let (start, end) = (start.min(end), start.max(end));
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

                if committed == Some(true) {
                    refresh_backend.request_code_lens_refresh().await;
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
                if committed == Some(true) {
                    refresh_backend.request_semantic_tokens_refresh().await;
                    refresh_backend.request_inlay_hint_refresh().await;
                    refresh_backend.request_code_lens_refresh().await;
                }
            });
        }
    }

    pub(crate) async fn on_did_close(&self, params: DidCloseTextDocumentParams) {
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
        } else if let Some(path) = self.workspace_index_path(&uri) {
            // A workspace file stays in the index once closed, as the file
            // on disk rather than the buffer: unsaved edits are discarded
            // with the buffer, and the references it made keep counting.
            // The re-parse (and the Laravel refreshes `update_ast` runs
            // for a provider or config file) goes to a blocking task, as
            // `did_change` does, so closing a file does not stall the
            // service loop.
            let reparse = move |backend: &Backend, uri: &str| match std::fs::read_to_string(&path) {
                Ok(content) => {
                    backend.update_ast(uri, &content);
                }
                Err(_) => backend.clear_file_maps(uri),
            };
            if self.sync_ast_updates {
                reparse(self, &uri);
            } else {
                let backend = self.clone_for_blocking();
                let uri = uri.clone();
                tokio::spawn(async move {
                    run_blocking_cancel_safe("did_close parse", move || {
                        // The file was reopened before this ran; its buffer is
                        // the truth now and `did_change` owns the parse.
                        if backend.open_files.read().contains_key(&uri) {
                            return;
                        }
                        reparse(&backend, &uri);
                    })
                    .await;
                });
            }
        } else {
            self.clear_file_maps(&uri);
        }

        // Clear diagnostics so stale warnings don't linger after the file is closed
        self.clear_diagnostics_for_file(&uri).await;

        self.log(MessageType::INFO, format!("Closed file: {}", uri))
            .await;
    }

    pub(crate) async fn on_did_save(&self, params: DidSaveTextDocumentParams) {
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

    pub(crate) async fn on_did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
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
            self.request_code_lens_refresh().await;
        }
    }
}
