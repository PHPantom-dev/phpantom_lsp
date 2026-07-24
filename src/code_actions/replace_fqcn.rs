//! Replace FQCN with import code action.
//!
//! When the cursor is on a fully-qualified class name (leading `\`), offer
//! two code actions:
//!
//! 1. **Replace FQCN** — inserts a `use` statement and replaces every
//!    occurrence of that specific FQCN in the file with the short name.
//! 2. **Replace all FQCNs** — does the same for every distinct FQCN in
//!    the file (skipping those with import conflicts).

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::use_edit::{analyze_use_block, build_use_edit, use_import_conflicts};
use crate::symbol_map::{ClassRefContext, SymbolKind, SymbolSpan};
use crate::text_position::{offset_to_position, position_to_byte_offset};
use crate::util::short_name;

/// Build a replacement `TextEdit` for a single FQN span, including the
/// leading `\` if present in source.
fn fqn_replace_edit(span: &SymbolSpan, sn: &str, content: &str) -> TextEdit {
    let replace_start = if span.start > 0 {
        let before = &content[..span.start as usize];
        if before.ends_with('\\') {
            span.start as usize - 1
        } else {
            span.start as usize
        }
    } else {
        span.start as usize
    };
    TextEdit {
        range: Range {
            start: offset_to_position(content, replace_start),
            end: offset_to_position(content, span.end as usize),
        },
        new_text: sn.to_string(),
    }
}

impl Backend {
    /// Collect "Replace FQCN with import" and "Replace all FQCNs" code
    /// actions when the cursor is on a fully-qualified class name.
    pub(crate) fn collect_replace_fqcn_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        let symbol_map = match self.symbol_maps.read().get(uri) {
            Some(sm) => sm.clone(),
            None => return,
        };

        let request_start = position_to_byte_offset(content, params.range.start);
        let request_end = position_to_byte_offset(content, params.range.end);

        // Find the FQN under the cursor.
        let cursor_span = symbol_map.spans.iter().find(|span| {
            if span.start as usize >= request_end || span.end as usize <= request_start {
                return false;
            }
            matches!(
                &span.kind,
                SymbolKind::ClassReference {
                    is_fqn: true,
                    context,
                    ..
                } if !matches!(context, ClassRefContext::UseImport)
            )
        });

        let cursor_span = match cursor_span {
            Some(s) => s,
            None => return,
        };

        let fqn = match &cursor_span.kind {
            SymbolKind::ClassReference { name, .. } => name.as_str(),
            _ => return,
        };
        let sn = short_name(fqn);

        let already_imported = file_use_map.iter().any(|(alias, existing_fqn)| {
            alias.eq_ignore_ascii_case(sn) && existing_fqn.eq_ignore_ascii_case(fqn)
        });

        if !already_imported && use_import_conflicts(fqn, &file_use_map) {
            return;
        }

        let doc_uri: Url = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        // ── Action 1: Replace this FQCN (all occurrences of the same name) ──
        {
            let mut edits: Vec<TextEdit> = Vec::new();

            if !already_imported {
                let use_block = analyze_use_block(content);
                if let Some(use_edits) = build_use_edit(fqn, &use_block, &file_namespace) {
                    edits.extend(use_edits);
                }
            }

            // Replace every occurrence of this FQN in the file.
            for span in &symbol_map.spans {
                let matches = matches!(
                    &span.kind,
                    SymbolKind::ClassReference {
                        name,
                        is_fqn: true,
                        context,
                    } if name.eq_ignore_ascii_case(fqn)
                        && !matches!(context, ClassRefContext::UseImport)
                );
                if matches {
                    edits.push(fqn_replace_edit(span, sn, content));
                }
            }

            let title = if already_imported {
                format!("Replace `\\{}` with short name `{}`", fqn, sn)
            } else {
                format!("Replace FQCN `\\{}` with import", fqn)
            };

            let mut changes = HashMap::new();
            changes.insert(doc_uri.clone(), edits);

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::REFACTOR),
                diagnostics: None,
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                }),
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: None,
            }));
        }

        // ── Action 2: Replace ALL FQCNs in the file ─────────────────────────
        // Collect every distinct FQN in the file that can be imported
        // without conflicts.
        {
            let mut seen: HashMap<String, Vec<&SymbolSpan>> = HashMap::new();
            for span in &symbol_map.spans {
                if let SymbolKind::ClassReference {
                    name,
                    is_fqn: true,
                    context,
                } = &span.kind
                {
                    if matches!(context, ClassRefContext::UseImport) {
                        continue;
                    }
                    seen.entry(name.to_string()).or_default().push(span);
                }
            }

            // Only offer this action if there are at least 2 distinct FQCNs,
            // or if there's 1 distinct FQCN with occurrences not already
            // covered by action 1 (i.e. a different FQCN exists).
            // More precisely: offer when there are FQCNs besides the one
            // under the cursor.
            if seen.len() < 2 {
                return;
            }

            let mut all_edits: Vec<TextEdit> = Vec::new();
            let use_block = analyze_use_block(content);
            let mut imported_short_names: Vec<String> =
                file_use_map.keys().map(|k| k.to_lowercase()).collect();

            // Sort FQNs for deterministic output.
            let mut fqns: Vec<_> = seen.keys().cloned().collect();
            fqns.sort();

            let mut distinct_replaced = 0;

            for fqn_key in &fqns {
                let sn_key = short_name(fqn_key);
                let sn_lower = sn_key.to_lowercase();

                let already = file_use_map.iter().any(|(alias, existing)| {
                    alias.eq_ignore_ascii_case(sn_key) && existing.eq_ignore_ascii_case(fqn_key)
                });

                // Skip if there's a conflict (different class with same
                // short name already imported or chosen in this batch).
                if !already {
                    if use_import_conflicts(fqn_key, &file_use_map)
                        || imported_short_names.contains(&sn_lower)
                    {
                        continue;
                    }
                    if let Some(use_edits) = build_use_edit(fqn_key, &use_block, &file_namespace) {
                        all_edits.extend(use_edits);
                    }
                    imported_short_names.push(sn_lower);
                }

                for span in &seen[fqn_key.as_str()] {
                    all_edits.push(fqn_replace_edit(span, sn_key, content));
                }
                distinct_replaced += 1;
            }

            // Only offer this action when it would replace more distinct
            // FQCNs than Action 1 alone (which already handles the cursor
            // FQCN).  If only one distinct FQCN survived conflict checks,
            // Action 1 already covers it.
            if distinct_replaced >= 2 {
                let mut changes = HashMap::new();
                changes.insert(doc_uri, all_edits);

                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: "Replace all FQCNs with imports".to_string(),
                    kind: Some(CodeActionKind::REFACTOR),
                    diagnostics: None,
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        document_changes: None,
                        change_annotations: None,
                    }),
                    command: None,
                    is_preferred: Some(false),
                    disabled: None,
                    data: None,
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::*;

    fn code_action_titles(content: &str, cursor_offset: usize) -> Vec<String> {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        backend.update_ast(uri, content);

        let pos = crate::text_position::offset_to_position(content, cursor_offset);
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: pos,
                end: pos,
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let mut actions = Vec::new();
        backend.collect_replace_fqcn_actions(uri, content, &params, &mut actions);
        actions
            .into_iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title,
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn offers_action_on_fqcn() {
        let src = "<?php\nnamespace App;\n\n\\Illuminate\\Support\\Str::plural('test');\n";
        // Cursor somewhere on the FQN (after the `\`)
        let offset = src.find("Illuminate\\Support\\Str").unwrap();
        let titles = code_action_titles(src, offset);
        assert_eq!(titles.len(), 1);
        assert!(titles[0].contains("Replace FQCN"));
        assert!(titles[0].contains("Illuminate\\Support\\Str"));
    }

    #[test]
    fn no_action_on_short_name() {
        let src =
            "<?php\nnamespace App;\n\nuse Illuminate\\Support\\Str;\n\nStr::plural('test');\n";
        let offset = src.find("Str::").unwrap();
        let titles = code_action_titles(src, offset);
        assert!(titles.is_empty());
    }

    #[test]
    fn reuses_existing_import() {
        let src = "<?php\nnamespace App;\n\nuse Illuminate\\Support\\Str;\n\n\\Illuminate\\Support\\Str::plural('test');\n";
        let offset = src.find("\\Illuminate\\Support\\Str::").unwrap() + 1;
        let titles = code_action_titles(src, offset);
        assert_eq!(titles.len(), 1);
        assert!(titles[0].contains("short name"));
    }

    #[test]
    fn skips_conflicting_import() {
        let src = "<?php\nnamespace App;\n\nuse Other\\Str;\n\n\\Illuminate\\Support\\Str::plural('test');\n";
        let offset = src.find("\\Illuminate\\Support\\Str::").unwrap() + 1;
        let titles = code_action_titles(src, offset);
        assert!(titles.is_empty());
    }

    #[test]
    fn offers_replace_all_with_multiple_occurrences() {
        let src = "<?php\nnamespace App;\n\n\\Illuminate\\Support\\Str::plural('a');\n\\Illuminate\\Support\\Str::lower('b');\n";
        let offset = src.find("Illuminate\\Support\\Str").unwrap();
        let titles = code_action_titles(src, offset);
        // Only 1 action: "Replace FQCN" replaces all occurrences of this FQCN.
        // No "Replace all FQCNs" because there's only one distinct FQCN.
        assert_eq!(titles.len(), 1);
        assert!(titles[0].contains("Replace FQCN"));
    }

    #[test]
    fn no_replace_all_with_single_occurrence() {
        let src = "<?php\nnamespace App;\n\n\\Illuminate\\Support\\Str::plural('a');\n";
        let offset = src.find("Illuminate\\Support\\Str").unwrap();
        let titles = code_action_titles(src, offset);
        assert_eq!(titles.len(), 1);
        assert!(titles[0].contains("Replace FQCN"));
    }

    #[test]
    fn offers_replace_all_fqcns_with_distinct_fqcns() {
        let src = "<?php\nnamespace App;\n\n\\Illuminate\\Support\\Str::plural('a');\n\\Illuminate\\Support\\Arr::first([]);\n";
        let offset = src.find("Illuminate\\Support\\Str").unwrap();
        let titles = code_action_titles(src, offset);
        assert_eq!(titles.len(), 2);
        assert!(
            titles[0].contains("Replace FQCN"),
            "first action: {}",
            titles[0]
        );
        assert!(
            titles[1].contains("Replace all FQCNs"),
            "second action: {}",
            titles[1]
        );
    }
}
