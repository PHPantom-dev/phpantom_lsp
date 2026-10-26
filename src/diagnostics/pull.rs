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

impl Backend {
    /// Body of `textDocument/diagnostic`. See [`server`](crate::server)'s
    /// module docs for how pull and push delivery are selected.
    pub(crate) fn document_pull_diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReport> {
        let uri_str = params.text_document.uri.to_string();

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

        // In pull mode the pull request *triggers* native diagnostic
        // computation, but never computes it inline: a synchronous
        // compute on the request path would hold a tower-lsp concurrency
        // slot for seconds and wedge the transport under a typing burst
        // (see `trigger_diagnostics_for_pull`). If the full cache is
        // missing for this URI, this call queues the debounced
        // background worker and returns whatever is currently cached
        // (possibly empty); the worker recomputes, caches the full set,
        // and requests a `workspace/diagnostic/refresh` so the editor
        // re-pulls the real results a moment later.
        let needs_compute = {
            let cache = self.diag.last_full.lock();
            !cache.contains_key(&uri_str)
        };

        if needs_compute {
            self.trigger_diagnostics_for_pull(&uri_str);
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
