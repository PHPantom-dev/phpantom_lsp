//! "Import all missing classes" code action.
//!
//! Imports every unresolved class name in the file at once.  When a name
//! has multiple candidates, the one with the highest namespace affinity is
//! chosen.  Short-name conflicts are detected and the conflicting name is
//! skipped.

use std::collections::{HashMap, HashSet};

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::backend::file_access::ImportBlock;
use crate::completion::use_edit::{build_use_edit, use_import_conflicts};
use crate::diagnostics::helpers::is_offset_in_ranges;
use crate::diagnostics::use_statements::compute_use_line_ranges;
use crate::symbol_map::ClassRefContext;
use crate::util::short_name;

use super::import_class::unqualified_class_name;
use super::{make_code_action_data, span_overlaps_request};

impl Backend {
    /// Collect a bulk "Import all missing classes" code action.
    ///
    /// Scans the entire file for unresolved class names (the same
    /// condition that triggers the single-class import action) and
    /// offers a single `source.addMissingImports` action that imports
    /// them all at once.  It is kept out of `source.organizeImports` so an
    /// automatic organize pass never picks between candidates.  The action uses the deferred resolve model
    /// so the actual edits are computed in
    /// [`resolve_import_all_classes`](Self::resolve_import_all_classes).
    pub(crate) fn collect_import_all_classes_action(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
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
        if unresolved
            .iter()
            .map(|(_, names)| names.len())
            .sum::<usize>()
            < 2
        {
            return;
        }

        // Count how many unresolved names have exactly one viable
        // candidate — only those will actually be imported by the
        // resolve step.  Don't show the action if fewer than 2 names
        // are unambiguously importable.
        let mut importable_count = 0;
        for (block, names) in &unresolved {
            let affinity_table = crate::completion::class_completion::build_affinity_table(
                &block.use_map,
                &block.namespace,
            );
            importable_count += names
                .iter()
                .filter(|(name, ctx)| {
                    let mut candidates = self.find_import_candidates(name, &affinity_table);
                    self.filter_candidates_by_context(&mut candidates, *ctx);
                    let viable: Vec<_> = candidates
                        .iter()
                        .filter(|fqn| !use_import_conflicts(fqn, &block.use_map))
                        .collect();
                    viable.len() == 1
                })
                .count();
        }
        if importable_count < 2 {
            return;
        }

        // Collect all unknown_class diagnostics so the action can clear
        // them on resolve.  These may be a subset of the unresolved
        // names (attributes are excluded from diagnostics).
        let mut all_unknown_diags: Vec<Diagnostic> = Vec::new();
        self.collect_unknown_class_diagnostics(uri, content, &mut all_unknown_diags);

        out.push(CodeActionResponse::CodeAction(CodeAction {
            title: "Import all missing classes".to_string(),
            kind: Some(CodeActionKind::new("source.addMissingImports")),
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
            tags: None,
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
        let Some(symbol_map) = self.symbol_map_for(uri) else {
            return false;
        };

        let request_start =
            crate::text_position::position_to_byte_offset(content, params.range.start);
        let request_end = crate::text_position::position_to_byte_offset(content, params.range.end);

        let block = self.import_block_at(uri, request_start);
        let local_classes = self.local_class_names(uri, block.namespace.as_deref());

        let Some(source) = symbol_map.source(content) else {
            return false;
        };

        symbol_map.spans.iter().any(|span| {
            span_overlaps_request(span, request_start, request_end)
                && unqualified_class_name(span, source).is_some_and(|(ref_name, _)| {
                    self.class_name_needs_import(
                        ref_name,
                        &block.use_map,
                        &local_classes,
                        &block.namespace,
                    )
                })
        })
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
        let doc_uri: Uri = data.uri.parse().ok()?;

        let unresolved = self.find_all_unresolved_class_names(&data.uri, content);

        let mut all_edits: Vec<TextEdit> = Vec::new();
        for (block, names) in &unresolved {
            all_edits.extend(self.import_all_edits_for_block(&data.uri, content, block, names));
        }

        if all_edits.is_empty() {
            return None;
        }

        Some(crate::code_actions::single_file_edit(doc_uri, all_edits))
    }

    /// The `use` statements "Import all missing classes" adds to one
    /// `namespace` block for the unresolved `names` written in it.
    fn import_all_edits_for_block(
        &self,
        uri: &str,
        content: &str,
        block: &ImportBlock,
        names: &[(String, ClassRefContext)],
    ) -> Vec<TextEdit> {
        let file_use_map = &block.use_map;
        let file_namespace = &block.namespace;
        let affinity_table =
            crate::completion::class_completion::build_affinity_table(file_use_map, file_namespace);

        // Track which short names we've already decided to import so we
        // can detect conflicts between two unresolved names that would
        // import different classes with the same short name.
        let mut imported_short_names: HashMap<String, String> = HashMap::new();

        // Pre-populate with existing imports so we don't conflict with them.
        for (alias, fqn) in file_use_map {
            imported_short_names.insert(alias.to_lowercase(), fqn.clone());
        }

        let use_block = self.use_block_for(uri, content, block.range);

        // First pass: decide which FQN to import for each unresolved name.
        let mut chosen_fqns: Vec<String> = Vec::new();

        for (ref_name, ref_context) in names {
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
                    if use_import_conflicts(fqn, file_use_map) {
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
            if build_use_edit(&fqn, &use_block, file_namespace).is_none() {
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
        // inserts at the same line stack in array order.  Sort
        // alphabetically so they appear in order when multiple inserts
        // target the same line.
        chosen_fqns.sort_by_key(|a| a.to_lowercase());
        chosen_fqns.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

        let mut all_edits: Vec<TextEdit> = Vec::new();
        for fqn in &chosen_fqns {
            if let Some(mut edits) = build_use_edit(fqn, &use_block, file_namespace) {
                if !all_edits.is_empty() {
                    use_block.drop_repeated_separator(&mut edits);
                }
                all_edits.extend(edits);
            }
        }
        all_edits
    }

    /// Find all unresolved class names in a file, grouped by the
    /// `namespace` block they are written in.
    ///
    /// Each block lists the deduplicated `(short_name, context)` pairs that
    /// cannot be resolved through its imports, its namespace, local
    /// classes, or global scope, sorted alphabetically by name for
    /// deterministic ordering.  Blocks with no unresolved names are left
    /// out.
    fn find_all_unresolved_class_names(
        &self,
        uri: &str,
        content: &str,
    ) -> Vec<(ImportBlock, Vec<(String, ClassRefContext)>)> {
        let Some(symbol_map) = self.symbol_map_for(uri) else {
            return Vec::new();
        };

        let blocks = self.import_blocks(uri);
        let local_classes: Vec<HashSet<String>> = blocks
            .iter()
            .map(|block| self.local_class_names(uri, block.namespace.as_deref()))
            .collect();

        // Compute byte ranges of `use` statement lines so we skip
        // references that are import declarations themselves.
        let use_line_ranges = compute_use_line_ranges(content);

        let Some(source) = symbol_map.source(content) else {
            return Vec::new();
        };

        // A template's virtual PHP also names classes in its prologue and in
        // the PHP a component tag lowers to; only the names it wrote count.
        let template = self.template_text(uri);
        if template.is_none() && self.is_blade_file(uri) {
            return Vec::new();
        }

        let mut seen: HashSet<(usize, String)> = HashSet::new();
        let mut unresolved: Vec<Vec<(String, ClassRefContext)>> = vec![Vec::new(); blocks.len()];

        for span in &symbol_map.spans {
            // Skip spans on `use` statement lines.
            if is_offset_in_ranges(span.start, &use_line_ranges) {
                continue;
            }

            let Some((ref_name, ref_context)) = unqualified_class_name(span, source) else {
                continue;
            };
            if template.as_ref().is_some_and(|template| {
                !template.wrote_bytes(span.start as usize, span.end as usize)
            }) {
                continue;
            }

            let index = ImportBlock::index_at(&blocks, span.start as usize);

            // Deduplicate — only process each short name once per block.
            if !seen.insert((index, ref_name.to_lowercase())) {
                continue;
            }

            let block = &blocks[index];
            if !self.class_name_needs_import(
                ref_name,
                &block.use_map,
                &local_classes[index],
                &block.namespace,
            ) {
                continue;
            }

            unresolved[index].push((ref_name.to_string(), ref_context));
        }

        blocks
            .into_iter()
            .zip(unresolved)
            .filter(|(_, names)| !names.is_empty())
            .map(|(block, mut names)| {
                names.sort_by(|a, b| a.0.cmp(&b.0));
                (block, names)
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "import_all_classes_tests.rs"]
mod tests;
