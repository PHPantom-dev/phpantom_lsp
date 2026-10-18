//! Import class code action.
//!
//! When the cursor is on an unresolved class name (a `ClassReference` in
//! the symbol map that cannot be resolved via use-map, namespace, or
//! local classes), offer code actions to add a `use` statement for each
//! matching class found in the class index and stubs.
//!
//! The bulk "Import all missing classes" action lives in
//! [`super::import_all_classes`].

use std::collections::{HashMap, HashSet};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::backend::file_access::ImportBlock;
use crate::completion::use_edit::{build_use_edit, use_import_conflicts};
use crate::diagnostics::unknown_classes::UNKNOWN_CLASS_CODE;

use crate::class_lookup::is_class_keyword;
use crate::symbol_map::{ClassRefContext, SymbolKind};
use crate::types::ClassLikeKind;
use crate::util::short_name;

fn span_matches_request(
    span_start: usize,
    span_end: usize,
    request_start: usize,
    request_end: usize,
) -> bool {
    if request_start == request_end {
        span_start <= request_start && request_start < span_end
    } else {
        span_start < request_end && span_end > request_start
    }
}

impl Backend {
    /// Push one "Import `Fqn`" code action per candidate that does not
    /// conflict with an existing import.
    ///
    /// `diagnostics` are attached to every action so an editor can offer
    /// each as a quick-fix for the diagnostic that flagged the name;
    /// `is_preferred` is set when `candidates` holds only the one.
    #[allow(clippy::too_many_arguments)]
    fn push_import_actions(
        &self,
        out: &mut Vec<CodeActionOrCommand>,
        doc_uri: &Url,
        candidates: &[String],
        file_use_map: &HashMap<String, String>,
        use_block: &crate::completion::use_edit::UseBlockInfo,
        file_namespace: &Option<String>,
        diagnostics: Option<&[Diagnostic]>,
    ) {
        for fqn in candidates {
            // Skip candidates that would conflict with an existing import
            // (e.g. a different class with the same short name is already
            // imported).
            if use_import_conflicts(fqn, file_use_map) {
                continue;
            }

            let edits = match build_use_edit(fqn, use_block, file_namespace) {
                Some(e) => e,
                // No edit needed (global class, no namespace) — skip.
                None => continue,
            };

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Import `{}`", fqn),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: diagnostics
                    .filter(|d| !d.is_empty())
                    .map(<[Diagnostic]>::to_vec),
                edit: Some(crate::code_actions::single_file_edit(
                    doc_uri.clone(),
                    edits,
                )),
                command: None,
                is_preferred: (candidates.len() == 1).then_some(true),
                disabled: None,
                data: None,
            }));
        }
    }

    /// Whether `ref_name`, a short (non-FQN) class name written in a file
    /// whose context is given by `file_use_map`/`local_classes`/
    /// `file_namespace`, resolves without an import.
    ///
    /// A name that is already imported, declared in the same file, reachable
    /// through the file's own namespace, or (for a namespace-less file)
    /// resolvable in the global scope needs no `use` statement; every other
    /// name is an import candidate.
    pub(super) fn class_name_needs_import(
        &self,
        ref_name: &str,
        file_use_map: &HashMap<String, String>,
        local_classes: &HashSet<String>,
        file_namespace: &Option<String>,
    ) -> bool {
        if file_use_map.contains_key(ref_name) {
            return false;
        }
        if local_classes.contains(ref_name) {
            return false;
        }
        if let Some(ns) = file_namespace {
            let ns_qualified = format!("{}\\{}", ns, ref_name);
            if self.find_or_load_class(&ns_qualified).is_some() {
                return false;
            }
        }
        if file_namespace.is_none() && self.find_or_load_class(ref_name).is_some() {
            return false;
        }
        true
    }

    /// Collect "Import class" code actions for the cursor position.
    ///
    /// For each unresolved `ClassReference` that overlaps with the
    /// request range, search the class index and stubs for
    /// classes whose short name matches, and offer a code action per
    /// candidate.
    pub(crate) fn collect_import_class_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        // Convert LSP range to byte offsets for comparison with symbol spans.
        let request_start =
            crate::text_position::position_to_byte_offset(content, params.range.start);
        let request_end = crate::text_position::position_to_byte_offset(content, params.range.end);

        // ── Gather the context of the block the cursor is in ────────────
        let block = self.import_block_at(uri, request_start);
        let file_use_map = &block.use_map;
        let file_namespace = &block.namespace;

        let symbol_map = match self.symbol_maps.read().get(uri) {
            Some(sm) => sm.clone(),
            None => return,
        };

        let local_classes = self.local_class_names(uri, file_namespace.as_deref());

        // ── Find ClassReference spans overlapping the request range ─────
        let affinity_table =
            crate::completion::class_completion::build_affinity_table(file_use_map, file_namespace);
        for span in &symbol_map.spans {
            if !span_matches_request(
                span.start as usize,
                span.end as usize,
                request_start,
                request_end,
            ) {
                continue;
            }

            let (ref_name, is_fqn, ref_context) = match &span.kind {
                SymbolKind::ClassReference {
                    name,
                    is_fqn,
                    context,
                } => (name.as_str(), *is_fqn, *context),
                _ => continue,
            };

            // Skip already-qualified names — they don't need importing.
            if is_fqn || ref_name.contains('\\') {
                continue;
            }

            // Skip names that resolve without an import.
            if !self.class_name_needs_import(ref_name, file_use_map, &local_classes, file_namespace)
            {
                continue;
            }

            // ── Name is unresolved — find import candidates ─────────────
            let mut candidates = self.find_import_candidates(ref_name, &affinity_table);
            self.filter_candidates_by_context(&mut candidates, ref_context);

            if candidates.is_empty() {
                continue;
            }

            let use_block = self.use_block_for(uri, content, block.range);
            let doc_uri: Url = match uri.parse() {
                Ok(u) => u,
                Err(_) => continue,
            };

            // Find any unknown_class diagnostics from the request context
            // that overlap this span so we can attach them to the code
            // action.  This lets editors show the import action as a
            // quick-fix for the diagnostic.
            let matching_diagnostics: Vec<Diagnostic> = params
                .context
                .diagnostics
                .iter()
                .filter(|d| {
                    matches!(
                        &d.code,
                        Some(NumberOrString::String(code)) if code == UNKNOWN_CLASS_CODE
                    )
                })
                .cloned()
                .collect();

            self.push_import_actions(
                out,
                &doc_uri,
                &candidates,
                file_use_map,
                &use_block,
                file_namespace,
                Some(&matching_diagnostics),
            );

            // Only process the first unresolved reference at the cursor.
            // Multiple overlapping references at the exact same position
            // are unlikely, and processing one keeps the action list tidy.
            break;
        }

        // ── Also check MemberAccess spans for unresolved static subjects ─
        // e.g. `Foo::bar()` where `Foo` is not imported — the symbol map
        // records this as a MemberAccess with subject_text "Foo", not a
        // ClassReference.  We handle this by looking for static member
        // accesses whose subject is an unresolved short name.
        self.collect_import_from_static_access(
            uri,
            content,
            params,
            request_start,
            request_end,
            &block,
            &local_classes,
            &symbol_map,
            out,
        );
    }

    /// Check static member access subjects for unresolved class names.
    #[allow(clippy::too_many_arguments)]
    fn collect_import_from_static_access(
        &self,
        uri: &str,
        content: &str,
        _params: &CodeActionParams,
        request_start: usize,
        request_end: usize,
        block: &ImportBlock,
        local_classes: &HashSet<String>,
        symbol_map: &crate::symbol_map::SymbolMap,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let file_use_map = &block.use_map;
        let file_namespace = &block.namespace;
        let affinity_table =
            crate::completion::class_completion::build_affinity_table(file_use_map, file_namespace);
        let Some(source) = symbol_map.source(content) else {
            return;
        };
        for span in &symbol_map.spans {
            if !span_matches_request(
                span.start as usize,
                span.end as usize,
                request_start,
                request_end,
            ) {
                continue;
            }

            let subject = match &span.kind {
                SymbolKind::MemberAccess {
                    subject_text,
                    is_static: true,
                    ..
                } => subject_text.as_str(source),
                _ => continue,
            };

            // Only handle simple unqualified names (not $this, self, parent, etc.)
            if subject.starts_with('$') || subject.contains('\\') || is_class_keyword(subject) {
                continue;
            }

            if !self.class_name_needs_import(subject, file_use_map, local_classes, file_namespace) {
                continue;
            }

            let mut candidates = self.find_import_candidates(subject, &affinity_table);
            // Static access subjects are always in a "call or constant"
            // context — no further kind filtering needed beyond what
            // affinity sorting provides.
            self.filter_candidates_by_context(&mut candidates, ClassRefContext::Other);
            if candidates.is_empty() {
                continue;
            }

            // The span covers the whole `Foo::bar` expression. We only
            // want the subject part for the diagnostic range, but for
            // the code action the span range is fine.
            let use_block = self.use_block_for(uri, content, block.range);
            let doc_uri: Url = match uri.parse() {
                Ok(u) => u,
                Err(_) => continue,
            };

            self.push_import_actions(
                out,
                &doc_uri,
                &candidates,
                file_use_map,
                &use_block,
                file_namespace,
                None,
            );

            break;
        }
    }

    /// Search all known class sources for classes whose short name matches
    /// `name` (case-insensitive).
    ///
    /// Returns a deduplicated, sorted list of fully-qualified class names.
    pub(super) fn find_import_candidates(
        &self,
        name: &str,
        affinity_table: &HashMap<String, u32>,
    ) -> Vec<String> {
        let mut candidates = Vec::new();
        let name_lower = name.to_lowercase();

        // ── 1. fqn_uri_index ──────────────────────────────────────────────
        {
            let idx = self.symbols.fqn_uri_index.read();
            for fqn in idx.keys() {
                if short_name(fqn).to_lowercase() == name_lower {
                    candidates.push(fqn.to_owned());
                }
            }
        }

        // ── 2. uri_classes_index (already-parsed files) ───────────────────────────
        {
            let amap = self.symbols.uri_classes_index.read();
            for classes in amap.values() {
                for cls in classes {
                    if cls.name.to_lowercase() == name_lower {
                        let fqn = match &cls.file_namespace {
                            Some(ns) if !ns.is_empty() => format!("{}\\{}", ns, cls.name),
                            _ => cls.name.to_string(),
                        };
                        if !candidates
                            .iter()
                            .any(|c: &String| c.eq_ignore_ascii_case(&fqn))
                        {
                            candidates.push(fqn);
                        }
                    }
                }
            }
        }

        // ── 3. Stubs (built-in PHP classes) ─────────────────────────────
        // Stubs are global-namespace classes, so the FQN is the short name.
        // Only add if the file has a namespace (otherwise no import needed).
        let stub_idx = self.stub_index.read();
        for stub_name in stub_idx.keys() {
            if short_name(stub_name).to_lowercase() == name_lower
                && !candidates
                    .iter()
                    .any(|c: &String| c.eq_ignore_ascii_case(stub_name))
            {
                candidates.push(stub_name.to_string());
            }
        }

        candidates.sort();
        candidates.dedup();

        // Sort by affinity score descending, with alphabetical tiebreak.
        candidates.sort_by(|a, b| {
            let score_a = crate::completion::class_completion::affinity_score(a, affinity_table);
            let score_b = crate::completion::class_completion::affinity_score(b, affinity_table);
            score_b.cmp(&score_a).then_with(|| a.cmp(b))
        });

        candidates
    }

    /// Filter import candidates by the syntactic context of the class
    /// reference.
    ///
    /// For example, after `implements` only interfaces are valid, after
    /// `use` inside a class body only traits are valid, etc.  When the
    /// context narrows the kind, candidates whose `ClassLikeKind` does
    /// not match are removed.  If filtering would remove *all*
    /// candidates (e.g. none of them are loaded yet), the list is left
    /// unchanged so the user still gets suggestions.
    pub(super) fn filter_candidates_by_context(
        &self,
        candidates: &mut Vec<String>,
        context: ClassRefContext,
    ) {
        let required_kind = match context {
            ClassRefContext::Implements => Some(ClassLikeKind::Interface),
            ClassRefContext::TraitUse => Some(ClassLikeKind::Trait),
            ClassRefContext::ExtendsClass => Some(ClassLikeKind::Class),
            ClassRefContext::ExtendsInterface => Some(ClassLikeKind::Interface),
            // Other contexts don't restrict to a single kind.
            _ => None,
        };

        let required_kind = match required_kind {
            Some(k) => k,
            None => return,
        };

        // Only filter if at least one candidate can be resolved to a
        // ClassInfo so we can check its kind.  If none are resolvable
        // (e.g. not yet indexed), keep the full list.
        let filtered: Vec<String> = candidates
            .iter()
            .filter(|fqn| {
                match self.find_or_load_class(fqn) {
                    Some(ci) => ci.kind == required_kind,
                    // Can't determine kind — keep the candidate.
                    None => true,
                }
            })
            .cloned()
            .collect();

        if !filtered.is_empty() {
            *candidates = filtered;
        }
    }
}

#[cfg(test)]
#[path = "import_class_tests.rs"]
mod tests;
