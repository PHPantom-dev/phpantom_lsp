//! Import class code action.
//!
//! When the cursor is on an unresolved class name (a `ClassReference` in
//! the symbol map that cannot be resolved via use-map, namespace, or
//! local classes), offer code actions to add a `use` statement for each
//! matching class found in the class index and stubs.
//!
//! Also provides a bulk "Import all missing classes" action that imports
//! every unresolved class name in the file at once.  When a name has
//! multiple candidates, the one with the highest namespace affinity is
//! chosen.  Short-name conflicts are detected and the conflicting name
//! is skipped.

use std::collections::{HashMap, HashSet};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::use_edit::{analyze_use_block, build_use_edit, use_import_conflicts};
use crate::diagnostics::unknown_classes::UNKNOWN_CLASS_CODE;

use crate::class_lookup::is_class_keyword;
use crate::symbol_map::{ClassRefContext, SymbolKind};
use crate::types::ClassLikeKind;
use crate::util::short_name;

use super::make_code_action_data;

impl Backend {
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
        // ── Gather file context ─────────────────────────────────────────
        let file_use_map: HashMap<String, String> = self.file_use_map(uri);

        let file_namespace: Option<String> = self.first_file_namespace(uri);

        let symbol_map = match self.symbol_maps.read().get(uri) {
            Some(sm) => sm.clone(),
            None => return,
        };

        let local_classes: Vec<crate::types::ClassInfo> = self
            .uri_classes_index
            .read()
            .get(uri)
            .map(|v| {
                v.iter()
                    .map(|c| crate::types::ClassInfo::clone(c))
                    .collect()
            })
            .unwrap_or_default();

        // Convert LSP range to byte offsets for comparison with symbol spans.
        let request_start =
            crate::text_position::position_to_byte_offset(content, params.range.start);
        let request_end = crate::text_position::position_to_byte_offset(content, params.range.end);

        // ── Find ClassReference spans overlapping the request range ─────
        let affinity_table = crate::completion::class_completion::build_affinity_table(
            &file_use_map,
            &file_namespace,
        );
        for span in &symbol_map.spans {
            // Check overlap: span overlaps the request range if
            // span.start < request_end && span.end > request_start
            if span.start as usize >= request_end || span.end as usize <= request_start {
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

            // Skip if the name is already imported via use-map.
            if file_use_map.contains_key(ref_name) {
                continue;
            }

            // Skip if it resolves as a local class (same file).
            if local_classes.iter().any(|c| c.name == ref_name) {
                continue;
            }

            // Skip if it resolves via same-namespace lookup.
            if let Some(ns) = &file_namespace {
                let ns_qualified = format!("{}\\{}", ns, ref_name);
                if self.find_or_load_class(&ns_qualified).is_some() {
                    continue;
                }
            }

            // Skip if the unqualified name resolves in global scope
            // (and the file has no namespace, so no import needed).
            if file_namespace.is_none() && self.find_or_load_class(ref_name).is_some() {
                continue;
            }

            // ── Name is unresolved — find import candidates ─────────────
            let mut candidates = self.find_import_candidates(ref_name, &affinity_table);
            self.filter_candidates_by_context(&mut candidates, ref_context);

            if candidates.is_empty() {
                continue;
            }

            let use_block = analyze_use_block(content);
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

            for fqn in &candidates {
                // Skip candidates that would conflict with an existing
                // import (e.g. a different class with the same short name
                // is already imported).
                if use_import_conflicts(fqn, &file_use_map) {
                    continue;
                }

                let edits = match build_use_edit(fqn, &use_block, &file_namespace) {
                    Some(e) => e,
                    // No edit needed (global class, no namespace) — skip.
                    None => continue,
                };

                let title = format!("Import `{}`", fqn);

                let mut changes = HashMap::new();
                changes.insert(doc_uri.clone(), edits);

                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: if matching_diagnostics.is_empty() {
                        None
                    } else {
                        Some(matching_diagnostics.clone())
                    },
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        document_changes: None,
                        change_annotations: None,
                    }),
                    command: None,
                    is_preferred: if candidates.len() == 1 {
                        Some(true)
                    } else {
                        None
                    },
                    disabled: None,
                    data: None,
                }));
            }

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
            &file_use_map,
            &file_namespace,
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
        file_use_map: &HashMap<String, String>,
        file_namespace: &Option<String>,
        local_classes: &[crate::types::ClassInfo],
        symbol_map: &crate::symbol_map::SymbolMap,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let affinity_table =
            crate::completion::class_completion::build_affinity_table(file_use_map, file_namespace);
        for span in &symbol_map.spans {
            if span.start as usize >= request_end || span.end as usize <= request_start {
                continue;
            }

            let subject = match &span.kind {
                SymbolKind::MemberAccess {
                    subject_text,
                    is_static: true,
                    ..
                } => subject_text.as_str(),
                _ => continue,
            };

            // Only handle simple unqualified names (not $this, self, parent, etc.)
            if subject.starts_with('$') || subject.contains('\\') || is_class_keyword(subject) {
                continue;
            }

            // Already imported?
            if file_use_map.contains_key(subject) {
                continue;
            }

            // Local class?
            if local_classes.iter().any(|c| c.name == subject) {
                continue;
            }

            // Resolves via namespace?
            if let Some(ns) = file_namespace {
                let ns_qualified = format!("{}\\{}", ns, subject);
                if self.find_or_load_class(&ns_qualified).is_some() {
                    continue;
                }
            }

            if file_namespace.is_none() && self.find_or_load_class(subject).is_some() {
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
            let use_block = analyze_use_block(content);
            let doc_uri: Url = match uri.parse() {
                Ok(u) => u,
                Err(_) => continue,
            };

            for fqn in &candidates {
                if use_import_conflicts(fqn, file_use_map) {
                    continue;
                }

                let edits = match build_use_edit(fqn, &use_block, file_namespace) {
                    Some(e) => e,
                    None => continue,
                };

                let title = format!("Import `{}`", fqn);

                let mut changes = HashMap::new();
                changes.insert(doc_uri.clone(), edits);

                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: None,
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        document_changes: None,
                        change_annotations: None,
                    }),
                    command: None,
                    is_preferred: if candidates.len() == 1 {
                        Some(true)
                    } else {
                        None
                    },
                    disabled: None,
                    data: None,
                }));
            }

            break;
        }
    }

    /// Search all known class sources for classes whose short name matches
    /// `name` (case-insensitive).
    ///
    /// Returns a deduplicated, sorted list of fully-qualified class names.
    fn find_import_candidates(
        &self,
        name: &str,
        affinity_table: &HashMap<String, u32>,
    ) -> Vec<String> {
        let mut candidates = Vec::new();
        let name_lower = name.to_lowercase();

        // ── 1. fqn_uri_index ──────────────────────────────────────────────
        {
            let idx = self.fqn_uri_index.read();
            for fqn in idx.keys() {
                if short_name(fqn).to_lowercase() == name_lower {
                    candidates.push(fqn.to_owned());
                }
            }
        }

        // ── 2. Class index ──────────────────────────────────────────────
        {
            let cmap = self.fqn_uri_index.read();
            for fqn in cmap.keys() {
                if short_name(fqn).to_lowercase() == name_lower
                    && !candidates
                        .iter()
                        .any(|c: &String| c.eq_ignore_ascii_case(fqn))
                {
                    candidates.push(fqn.to_owned());
                }
            }
        }

        // ── 3. uri_classes_index (already-parsed files) ───────────────────────────
        {
            let amap = self.uri_classes_index.read();
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

        // ── 4. Stubs (built-in PHP classes) ─────────────────────────────
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
    fn filter_candidates_by_context(&self, candidates: &mut Vec<String>, context: ClassRefContext) {
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

    /// Collect a bulk "Import all missing classes" code action.
    ///
    /// Scans the entire file for unresolved class names (the same
    /// condition that triggers the single-class import action) and
    /// offers a single `source.organizeImports` action that imports
    /// them all at once.  The action uses the deferred resolve model
    /// so the actual edits are computed in
    /// [`resolve_import_all_classes`](Self::resolve_import_all_classes).
    pub(crate) fn collect_import_all_classes_action(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        // Only show when the cursor overlaps an unresolved class name —
        // the same position where the single "Import class" action
        // appears.  We check against the symbol map spans directly
        // (not diagnostics) because unknown_class diagnostics
        // deliberately skip attribute blocks, yet the single import
        // action still fires there.
        if !self.cursor_on_unresolved_class(uri, content, params) {
            return;
        }

        let unresolved = self.find_all_unresolved_class_names(uri, content);
        if unresolved.len() < 2 {
            return;
        }

        // Count how many unresolved names have exactly one viable
        // candidate — only those will actually be imported by the
        // resolve step.  Don't show the action if fewer than 2 names
        // are unambiguously importable.
        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);
        let affinity_table = crate::completion::class_completion::build_affinity_table(
            &file_use_map,
            &file_namespace,
        );
        let importable_count = unresolved
            .iter()
            .filter(|(name, ctx)| {
                let mut candidates = self.find_import_candidates(name, &affinity_table);
                self.filter_candidates_by_context(&mut candidates, *ctx);
                let viable: Vec<_> = candidates
                    .iter()
                    .filter(|fqn| !use_import_conflicts(fqn, &file_use_map))
                    .collect();
                viable.len() == 1
            })
            .count();
        if importable_count < 2 {
            return;
        }

        // Collect all unknown_class diagnostics so the action can clear
        // them on resolve.  These may be a subset of the unresolved
        // names (attributes are excluded from diagnostics).
        let mut all_unknown_diags: Vec<Diagnostic> = Vec::new();
        self.collect_unknown_class_diagnostics(uri, content, &mut all_unknown_diags);

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Import all missing classes".to_string(),
            kind: Some(CodeActionKind::new("source.organizeImports")),
            diagnostics: if all_unknown_diags.is_empty() {
                None
            } else {
                Some(all_unknown_diags)
            },
            edit: None,
            command: None,
            is_preferred: None,
            disabled: None,
            data: Some(make_code_action_data(
                "source.importAllClasses",
                uri,
                &params.range,
                serde_json::json!({}),
            )),
        }));
    }

    /// Check whether the cursor overlaps an unresolved class name in the
    /// symbol map.
    ///
    /// Uses the same resolution logic as [`collect_import_class_actions`]
    /// — checking `ClassReference` and static `MemberAccess` spans —
    /// without computing candidates or building edits.
    fn cursor_on_unresolved_class(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
    ) -> bool {
        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        let symbol_map = match self.symbol_maps.read().get(uri) {
            Some(sm) => sm.clone(),
            None => return false,
        };

        let local_classes: Vec<crate::types::ClassInfo> = self
            .uri_classes_index
            .read()
            .get(uri)
            .map(|v| {
                v.iter()
                    .map(|c| crate::types::ClassInfo::clone(c))
                    .collect()
            })
            .unwrap_or_default();

        let request_start =
            crate::text_position::position_to_byte_offset(content, params.range.start);
        let request_end = crate::text_position::position_to_byte_offset(content, params.range.end);

        for span in &symbol_map.spans {
            if span.start as usize >= request_end || span.end as usize <= request_start {
                continue;
            }

            let ref_name = match &span.kind {
                SymbolKind::ClassReference {
                    name,
                    is_fqn: false,
                    ..
                } if !name.contains('\\') => name.as_str(),
                SymbolKind::MemberAccess {
                    subject_text,
                    is_static: true,
                    ..
                } if !subject_text.starts_with('$')
                    && !subject_text.contains('\\')
                    && !is_class_keyword(subject_text) =>
                {
                    subject_text.as_str()
                }
                _ => continue,
            };

            if file_use_map.contains_key(ref_name) {
                continue;
            }
            if local_classes.iter().any(|c| c.name == ref_name) {
                continue;
            }
            if let Some(ns) = &file_namespace {
                let ns_qualified = format!("{}\\{}", ns, ref_name);
                if self.find_or_load_class(&ns_qualified).is_some() {
                    continue;
                }
            }
            if file_namespace.is_none() && self.find_or_load_class(ref_name).is_some() {
                continue;
            }

            // Found an unresolved name at the cursor.
            return true;
        }

        false
    }

    /// Resolve a deferred "Import all missing classes" code action.
    ///
    /// Re-scans the file for unresolved class names (the set may have
    /// changed since Phase 1), picks the best candidate for each, and
    /// builds a combined `WorkspaceEdit` with all the `use` statements.
    pub(crate) fn resolve_import_all_classes(
        &self,
        data: &super::CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let doc_uri: Url = data.uri.parse().ok()?;

        let unresolved = self.find_all_unresolved_class_names(&data.uri, content);
        if unresolved.is_empty() {
            return None;
        }

        let file_use_map: HashMap<String, String> = self.file_use_map(&data.uri);
        let file_namespace: Option<String> = self.first_file_namespace(&data.uri);
        let affinity_table = crate::completion::class_completion::build_affinity_table(
            &file_use_map,
            &file_namespace,
        );

        // Track which short names we've already decided to import so we
        // can detect conflicts between two unresolved names that would
        // import different classes with the same short name.
        let mut imported_short_names: HashMap<String, String> = HashMap::new();

        // Pre-populate with existing imports so we don't conflict with them.
        for (alias, fqn) in &file_use_map {
            imported_short_names.insert(alias.to_lowercase(), fqn.clone());
        }

        let use_block = analyze_use_block(content);

        // First pass: decide which FQN to import for each unresolved name.
        let mut chosen_fqns: Vec<String> = Vec::new();

        for (ref_name, ref_context) in &unresolved {
            let mut candidates = self.find_import_candidates(ref_name, &affinity_table);
            self.filter_candidates_by_context(&mut candidates, *ref_context);
            if candidates.is_empty() {
                continue;
            }

            // Filter to candidates that don't conflict with existing or
            // already-chosen imports.
            let viable: Vec<&String> = candidates
                .iter()
                .filter(|fqn| {
                    let sn = short_name(fqn).to_lowercase();
                    if use_import_conflicts(fqn, &file_use_map) {
                        return false;
                    }
                    if let Some(existing_fqn) = imported_short_names.get(&sn) {
                        return existing_fqn.eq_ignore_ascii_case(fqn);
                    }
                    true
                })
                .collect();

            // Only auto-import when there is exactly one viable candidate.
            // Ambiguous names (multiple candidates) require manual
            // resolution via the single-class import action.
            if viable.len() != 1 {
                continue;
            }

            let fqn = viable[0].clone();

            // Verify that build_use_edit would produce an edit for this
            // FQN (e.g. global classes in non-namespaced files don't
            // need importing).
            if build_use_edit(&fqn, &use_block, &file_namespace).is_none() {
                continue;
            }

            // Record the short name so subsequent names can detect conflicts.
            let sn = short_name(&fqn).to_lowercase();
            imported_short_names.insert(sn, fqn.clone());

            chosen_fqns.push(fqn);
        }

        // Second pass: compute all edits against the original content.
        // All positions come from the same use_block (original file).
        // The editor applies all edits simultaneously, so multiple
        // inserts at the same line stack in array order.  We sort the
        // Sort alphabetically so they appear in order when multiple
        // inserts target the same line.
        chosen_fqns.sort_by_key(|a| a.to_lowercase());
        chosen_fqns.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

        let mut all_edits: Vec<TextEdit> = Vec::new();
        let mut first = true;
        for fqn in &chosen_fqns {
            if let Some(mut edits) = build_use_edit(fqn, &use_block, &file_namespace) {
                // build_use_edit prepends "\n" when there are no
                // existing imports and the file has a namespace.  In
                // a bulk insert every edit sees the same original
                // use_block, so they all get the prefix.  Strip it
                // from every edit after the first.
                if !first {
                    for e in &mut edits {
                        if let Some(rest) = e.new_text.strip_prefix('\n') {
                            e.new_text = rest.to_string();
                        }
                    }
                } else {
                    first = false;
                }
                all_edits.extend(edits);
            }
        }

        if all_edits.is_empty() {
            return None;
        }

        let mut changes = HashMap::new();
        changes.insert(doc_uri, all_edits);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }

    /// Find all unresolved class names in a file.
    ///
    /// Returns a deduplicated list of `(short_name, context)` pairs that
    /// cannot be resolved through use-map, namespace, local classes, or
    /// global scope.  The list is sorted alphabetically by name for
    /// deterministic ordering.
    fn find_all_unresolved_class_names(
        &self,
        uri: &str,
        content: &str,
    ) -> Vec<(String, ClassRefContext)> {
        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        let symbol_map = match self.symbol_maps.read().get(uri) {
            Some(sm) => sm.clone(),
            None => return Vec::new(),
        };

        let local_classes: Vec<crate::types::ClassInfo> = self
            .uri_classes_index
            .read()
            .get(uri)
            .map(|v| {
                v.iter()
                    .map(|c| crate::types::ClassInfo::clone(c))
                    .collect()
            })
            .unwrap_or_default();

        // Compute byte ranges of `use` statement lines so we skip
        // references that are import declarations themselves.
        let use_line_ranges = compute_use_line_ranges(content);

        let mut seen: HashSet<String> = HashSet::new();
        let mut unresolved: Vec<(String, ClassRefContext)> = Vec::new();

        for span in &symbol_map.spans {
            // Skip spans on `use` statement lines.
            if is_offset_in_ranges(span.start, &use_line_ranges) {
                continue;
            }

            let (ref_name, ref_context) = match &span.kind {
                SymbolKind::ClassReference {
                    name,
                    is_fqn: false,
                    context,
                } if !name.contains('\\') => (name.as_str(), *context),
                SymbolKind::MemberAccess {
                    subject_text,
                    is_static: true,
                    ..
                } if !subject_text.starts_with('$')
                    && !subject_text.contains('\\')
                    && !is_class_keyword(subject_text) =>
                {
                    (subject_text.as_str(), ClassRefContext::Other)
                }
                _ => continue,
            };

            // Deduplicate — only process each short name once.
            if !seen.insert(ref_name.to_lowercase()) {
                continue;
            }

            // Skip if already imported.
            if file_use_map.contains_key(ref_name) {
                continue;
            }

            // Skip local classes.
            if local_classes.iter().any(|c| c.name == ref_name) {
                continue;
            }

            // Skip if resolvable via same-namespace lookup.
            if let Some(ns) = &file_namespace {
                let ns_qualified = format!("{}\\{}", ns, ref_name);
                if self.find_or_load_class(&ns_qualified).is_some() {
                    continue;
                }
            }

            // Skip if global scope resolves it (and file has no namespace).
            if file_namespace.is_none() && self.find_or_load_class(ref_name).is_some() {
                continue;
            }

            unresolved.push((ref_name.to_string(), ref_context));
        }

        unresolved.sort_by(|a, b| a.0.cmp(&b.0));
        unresolved
    }
}

/// Compute byte ranges `(start, end)` of top-level `use` statement lines.
///
/// This is used to skip `ClassReference` spans that fall on import
/// declaration lines (they are the imports themselves, not usages).
fn compute_use_line_ranges(content: &str) -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();
    let mut offset: u32 = 0;
    let mut brace_depth: u32 = 0;

    // Iterate with `split_inclusive` so the terminator stays attached to
    // each chunk. Advancing `offset` by the full chunk length keeps the
    // byte ranges correct on CRLF files (where `str::lines()` would strip
    // the `\r` and drift the offset by one byte per line).
    for chunk in content.split_inclusive('\n') {
        let line = chunk.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = line.trim();
        let line_start = offset;
        let line_end = offset + line.len() as u32;

        let depth_at_start = brace_depth;
        for ch in trimmed.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
        }

        if depth_at_start == 0
            && (trimmed.starts_with("use ") || trimmed.starts_with("use\t"))
            && !trimmed.starts_with("use (")
            && !trimmed.starts_with("use(")
        {
            ranges.push((line_start, line_end));
        }

        offset += chunk.len() as u32;
    }

    ranges
}

/// Check whether a byte offset falls within any of the given ranges.
fn is_offset_in_ranges(offset: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| offset >= *start && offset < *end)
}

#[cfg(test)]
#[path = "import_class_tests.rs"]
mod tests;
