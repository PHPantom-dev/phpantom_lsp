//! Pull-model diagnostics (`textDocument/diagnostic` and
//! `workspace/diagnostic`).
//!
//! Contains the resultId-cache logic that decides whether to return
//! `Unchanged`, trigger fresh native computation, or report the
//! background workspace pass's cached results. The LSP trait handlers
//! in `server.rs` are thin delegations to these methods.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

use tower_lsp_server::gen_lsp_types::*;
use tower_lsp_server::jsonrpc::Result;

use crate::Backend;

use super::state::DiagnosticState;

/// A held document pull's place in `pull_generations`.  Taking one
/// supersedes the URI's previous pull; dropping it, when the pull answers
/// or the client cancels it, removes the entry unless a newer pull owns it.
struct PullTicket<'a> {
    state: &'a DiagnosticState,
    uri: &'a str,
    generation: u64,
}

impl<'a> PullTicket<'a> {
    fn take(state: &'a DiagnosticState, uri: &'a str) -> Self {
        let generation = state.pull_seq.fetch_add(1, Ordering::Relaxed);
        state
            .pull_generations
            .lock()
            .insert(uri.to_string(), generation);
        state.pass_done.notify_waiters();
        Self {
            state,
            uri,
            generation,
        }
    }

    fn superseded(&self) -> bool {
        self.state.pull_generations.lock().get(self.uri) != Some(&self.generation)
    }
}

impl Drop for PullTicket<'_> {
    fn drop(&mut self) {
        let mut generations = self.state.pull_generations.lock();
        if generations.get(self.uri) == Some(&self.generation) {
            generations.remove(self.uri);
        }
    }
}

impl Backend {
    /// Body of `textDocument/diagnostic`. See [`server`](crate::server)'s
    /// module docs for how pull and push delivery are selected.
    pub(crate) async fn document_pull_diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReport> {
        let uri_str = params.text_document.uri.to_string();

        self.await_fresh_diagnostics(&uri_str).await;

        // Check resultId — if the client sends back the same resultId we
        // last returned AND the full cache is still present (not
        // invalidated by schedule_diagnostics), the diagnostics have not
        // changed and we can return Unchanged immediately.
        // Read both under the `last_full` guard: delivery swaps the set
        // and bumps the id under that same guard.
        if let Some(prev_id) = &params.previous_result_id {
            let cache = self.diag.last_full.lock();
            let ids = self.diag.result_ids.lock();
            if cache.contains_key(&uri_str)
                && let Some(&current_id) = ids.get(&uri_str)
                && prev_id == &current_id.to_string()
            {
                return Ok(
                    DocumentDiagnosticReport::RelatedUnchangedDocumentDiagnosticReport(
                        RelatedUnchangedDocumentDiagnosticReport {
                            related_documents: None,
                            unchanged_document_diagnostic_report:
                                UnchangedDocumentDiagnosticReport {
                                    result_id: current_id.to_string(),
                                },
                        },
                    ),
                );
            }
        }

        let (diagnostics, result_id) = {
            let cache = self.diag.last_full.lock();
            let ids = self.diag.result_ids.lock();
            let diags = cache.get(&uri_str).cloned().unwrap_or_default();
            let rid = ids.get(&uri_str).copied().unwrap_or(0).to_string();
            (diags, rid)
        };

        Ok(
            DocumentDiagnosticReport::RelatedFullDocumentDiagnosticReport(
                RelatedFullDocumentDiagnosticReport {
                    related_documents: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: Some(result_id),
                        items: diagnostics,
                    },
                },
            ),
        )
    }

    /// Hold a document pull until the diagnostics it answers with reflect
    /// the document's current text.
    ///
    /// Editors pull right after every `didChange`.  Answered at once, that
    /// pull reports the set from before the edit, as `Unchanged` when the
    /// client echoes the last `resultId`, and the fresh set only reaches a
    /// client that pulls the document again on `workspace/diagnostic/refresh`.
    /// Neovim answers the refresh with a `workspace/diagnostic` pull only,
    /// so it kept showing the stale set until the buffer was reloaded.  The
    /// same went for a pull that landed while the project was still being
    /// indexed at startup, which was answered with an empty set.
    ///
    /// So the pull waits for startup to finish, queues a pass when nothing
    /// is cached for the document, and waits while the worker still owes
    /// the document a pass.  Nothing is computed on the request path: a
    /// synchronous compute would hold a tower-lsp concurrency slot for
    /// seconds and wedge the transport under a typing burst (see
    /// `trigger_diagnostics_for_pull`).  A newer pull for the same document
    /// releases the held one, so a typing burst holds at most one request
    /// per document.  A cancelled request drops this future, which is
    /// equally safe.
    async fn await_fresh_diagnostics(&self, uri: &str) {
        let ticket = PullTicket::take(&self.diag, uri);
        let mut queued = false;

        loop {
            let woken = self.diag.pass_done.notified();
            tokio::pin!(woken);
            // Register before checking, so a wake-up between the check
            // and the await is not lost.
            woken.as_mut().enable();
            if ticket.superseded() || self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }
            if self.init_complete.load(Ordering::Acquire) {
                if !queued && !self.diag.last_full.lock().contains_key(uri) {
                    self.trigger_diagnostics_for_pull(uri);
                }
                queued = true;
                if !self.diagnostic_pass_outstanding(uri) {
                    return;
                }
            }
            woken.await;
        }
    }

    /// Whether the running diagnostic worker still owes `uri` a pass: its
    /// latest edit is still being parsed, or it is queued for the worker,
    /// or being diagnosed by it.  Each stage hands the URI on before it
    /// lets go of it, so they are read in that order: the parse queues the
    /// URI before it drops its `edits_awaiting_parse` entry, and the worker
    /// moves it into `in_flight_uris` before it releases `pending_uris`.
    fn diagnostic_pass_outstanding(&self, uri: &str) -> bool {
        if !self.diag.worker_alive.load(Ordering::Acquire) {
            return false;
        }
        if self.diag.edits_awaiting_parse.lock().contains_key(uri) {
            return true;
        }
        let pending = self.diag.pending_uris.lock();
        pending.contains(uri) || self.diag.in_flight_uris.lock().contains(uri)
    }

    /// Body of `workspace/diagnostic`. Reports open files from the live
    /// per-file pipeline and unopened files from the background workspace
    /// pass.
    pub(crate) fn workspace_pull_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReport> {
        // The first workspace pull releases the deferred background
        // workspace pass (see `wait_for_first_workspace_pull`).  This
        // request returns whatever is cached; the pass streams the rest
        // via `workspace/diagnostic/refresh` as batches complete.
        if !self.diag.workspace_pull_seen.swap(true, Ordering::AcqRel) {
            self.diag.workspace_pull_notify.notify_one();
        }

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
            // Check if the client already has up-to-date diagnostics.
            // The resultId must match AND the full cache must be present.
            // When `schedule_diagnostics` invalidates the cache (removing
            // diag_last_full), the resultId is intentionally kept so it
            // doesn't reset to 0.  But we must not return "unchanged"
            // when the cache is missing — that means fresh computation
            // is needed.  Both are read under the `last_full` guard,
            // which delivery holds while it swaps the set and bumps the id.
            let (cache_present, current_id) = {
                let cache = self.diag.last_full.lock();
                let ids = self.diag.result_ids.lock();
                (
                    cache.contains_key(uri_str.as_str()),
                    ids.get(uri_str.as_str()).copied().unwrap_or(0),
                )
            };
            if cache_present
                && let Some(prev_id) = previous.get(uri_str.as_str())
                && *prev_id == current_id.to_string()
            {
                let uri = match uri_str.parse::<Uri>() {
                    Ok(u) => u,
                    Err(_) => continue,
                };
                items.push(
                    WorkspaceDocumentDiagnosticReport::WorkspaceUnchangedDocumentDiagnosticReport(
                        WorkspaceUnchangedDocumentDiagnosticReport {
                            uri,
                            version: None,
                            unchanged_document_diagnostic_report:
                                UnchangedDocumentDiagnosticReport {
                                    result_id: current_id.to_string(),
                                },
                        },
                    ),
                );
                continue;
            }

            // If the cache is missing, trigger computation (same as
            // textDocument/diagnostic).
            let needs_compute = {
                let cache = self.diag.last_full.lock();
                !cache.contains_key(uri_str.as_str())
            };
            if needs_compute {
                self.trigger_diagnostics_for_pull(uri_str);
            }

            let (diagnostics, current_id) = {
                let cache = self.diag.last_full.lock();
                let ids = self.diag.result_ids.lock();
                (
                    cache.get(uri_str.as_str()).cloned().unwrap_or_default(),
                    ids.get(uri_str.as_str()).copied().unwrap_or(0),
                )
            };

            let uri = match uri_str.parse::<Uri>() {
                Ok(u) => u,
                Err(_) => continue,
            };

            items.push(
                WorkspaceDocumentDiagnosticReport::WorkspaceFullDocumentDiagnosticReport(
                    WorkspaceFullDocumentDiagnosticReport {
                        uri,
                        version: None,
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: Some(current_id.to_string()),
                            items: diagnostics,
                        },
                    },
                ),
            );
        }

        // ── Background workspace diagnostics (unopened files) ────────
        // Files the user has not opened are diagnosed by the background
        // workspace pass; report its cached results here.  Open files
        // are skipped — the live per-file pipeline above owns them.
        let open_uris: HashSet<&str> = open_uris.iter().map(|u| u.as_str()).collect();
        let workspace_items: Vec<(String, String, Option<Vec<Diagnostic>>)> = {
            let mut ws = self.diag.workspace_diags.lock();
            ws.tracked_uris()
                .into_iter()
                .filter(|uri| !open_uris.contains(uri.as_str()))
                .map(|uri| {
                    // `diff_for_pull` trusts the client's echoed id, and
                    // otherwise falls back to what the server itself
                    // last delivered — this is what keeps a file the
                    // client can't display (and so never echoes an id
                    // for) from being re-serialized in full on every
                    // pull for the rest of the session.
                    let client_previous = previous.get(uri.as_str()).copied();
                    let (result_id, diags) = ws.diff_for_pull(&uri, client_previous);
                    (uri, result_id, diags)
                })
                .collect()
        };

        for (uri_str, result_id, diagnostics) in workspace_items {
            let Ok(uri) = uri_str.parse::<Uri>() else {
                continue;
            };

            match diagnostics {
                // The client already has this exact result set.
                None => items.push(
                    WorkspaceDocumentDiagnosticReport::WorkspaceUnchangedDocumentDiagnosticReport(
                        WorkspaceUnchangedDocumentDiagnosticReport {
                            uri,
                            version: None,
                            unchanged_document_diagnostic_report:
                                UnchangedDocumentDiagnosticReport { result_id },
                        },
                    ),
                ),
                Some(diags) => items.push(
                    WorkspaceDocumentDiagnosticReport::WorkspaceFullDocumentDiagnosticReport(
                        WorkspaceFullDocumentDiagnosticReport {
                            uri,
                            version: None,
                            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                                result_id: Some(result_id),
                                items: diags,
                            },
                        },
                    ),
                ),
            }
        }

        Ok(WorkspaceDiagnosticReport { items })
    }
}
