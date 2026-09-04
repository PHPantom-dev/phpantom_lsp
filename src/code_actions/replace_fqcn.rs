//! Import a qualified symbol and replace its usages with a short name.

use std::collections::{HashMap, HashSet};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::use_edit::{
    analyze_use_block, build_aliased_typed_use_edit, build_aliased_use_edit,
};
use crate::symbol_map::{ClassRefContext, SymbolKind, SymbolSpan};
use crate::text_position::position_to_byte_offset;
use crate::util::{short_name, strip_fqn_prefix};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportKind {
    Class,
    Function,
    Constant,
}

impl ImportKind {
    fn label(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Function => "function",
            Self::Constant => "constant",
        }
    }

    fn keyword(self) -> Option<&'static str> {
        match self {
            Self::Class => None,
            Self::Function => Some("function"),
            Self::Constant => Some("const"),
        }
    }
}

fn overlaps(span: &SymbolSpan, start: usize, end: usize) -> bool {
    if start == end {
        span.start as usize <= start && start < span.end as usize
    } else {
        (span.start as usize) < end && span.end as usize > start
    }
}

fn symbol_name_and_kind(span: &SymbolSpan) -> Option<(&str, ImportKind)> {
    match &span.kind {
        SymbolKind::ClassReference { name, context, .. }
            if !matches!(context, ClassRefContext::UseImport) =>
        {
            Some((name, ImportKind::Class))
        }
        SymbolKind::FunctionCall {
            name,
            is_definition: false,
            is_docblock_reference: false,
        } => Some((name, ImportKind::Function)),
        SymbolKind::ConstantReference {
            name,
            is_definition: false,
        } => Some((name, ImportKind::Constant)),
        _ => None,
    }
}

fn replacement_edit(span: &SymbolSpan, replacement: &str, content: &str) -> TextEdit {
    let start = span.start as usize;
    let source = &content[start..span.end as usize];
    let replace_start = if source.starts_with('\\') {
        start
    } else if start > 0 && content.as_bytes()[start - 1] == b'\\' {
        start - 1
    } else {
        start
    };
    TextEdit {
        range: crate::text_position::byte_range_to_lsp_range(
            content,
            replace_start,
            span.end as usize,
        ),
        new_text: replacement.to_string(),
    }
}

impl Backend {
    pub(crate) fn collect_replace_fqcn_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let Some(symbol_map) = self.symbol_map_for(uri) else {
            return;
        };
        if !symbol_map.matches_source(content) {
            return;
        }

        let request_start = position_to_byte_offset(content, params.range.start);
        let request_end = position_to_byte_offset(content, params.range.end);
        let Some(cursor_span) = symbol_map
            .spans
            .iter()
            .find(|span| overlaps(span, request_start, request_end))
        else {
            return;
        };
        let Some((written_name, kind)) = symbol_name_and_kind(cursor_span) else {
            return;
        };
        if !written_name.contains('\\') {
            return;
        }

        let file = self.file_context_at(uri, cursor_span.start);
        let resolved_name = file.resolve_name_at(written_name, cursor_span.start);
        let fqn = strip_fqn_prefix(&resolved_name);
        if !fqn.contains('\\') {
            return;
        }
        let natural_name = short_name(fqn);
        let alias = self.import_alias_for(fqn, natural_name, &file.use_map);
        let replacement = alias.as_deref().unwrap_or(natural_name);
        let already_imported = file.use_map.iter().any(|(name, imported)| {
            name.eq_ignore_ascii_case(replacement) && imported.eq_ignore_ascii_case(fqn)
        });

        let namespace = self.namespace_at_offset(uri, cursor_span.start);
        let namespace_spans = self.namespace_spans_for_uri(uri);
        let mut edits = Vec::new();
        if !already_imported {
            let use_block = analyze_use_block(content);
            let import_edits = match kind.keyword() {
                None => build_aliased_use_edit(fqn, alias.as_deref(), &use_block, &namespace),
                Some(keyword) => {
                    build_aliased_typed_use_edit(fqn, alias.as_deref(), keyword, &use_block)
                }
            };
            let Some(import_edits) = import_edits else {
                return;
            };
            edits.extend(import_edits);
        }

        for span in &symbol_map.spans {
            let Some((name, span_kind)) = symbol_name_and_kind(span) else {
                continue;
            };
            if span_kind != kind
                || self.namespace_at_offset_from_spans(&namespace_spans, span.start) != namespace
                || !file
                    .resolve_name_at(name, span.start)
                    .eq_ignore_ascii_case(fqn)
            {
                continue;
            }
            if name.eq_ignore_ascii_case(replacement) && !name.contains('\\') {
                continue;
            }
            edits.push(replacement_edit(span, replacement, content));
        }

        if edits.is_empty() {
            return;
        }
        let Ok(doc_uri) = uri.parse() else {
            return;
        };
        let title = match alias {
            Some(alias) => format!(
                "Import {} `{}` as `{}` and shorten usages",
                kind.label(),
                fqn,
                alias
            ),
            None => format!("Import {} `{}` and shorten usages", kind.label(), fqn),
        };
        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::REFACTOR_REWRITE),
            diagnostics: None,
            edit: Some(crate::code_actions::single_file_edit(doc_uri, edits)),
            command: None,
            is_preferred: Some(false),
            disabled: None,
            data: None,
        }));
    }

    fn namespace_at_offset_from_spans(
        &self,
        spans: &[crate::types::NamespaceSpan],
        offset: u32,
    ) -> Option<String> {
        spans
            .iter()
            .find(|span| offset >= span.start && offset <= span.end)
            .and_then(|span| span.namespace.clone())
    }

    fn import_alias_for(
        &self,
        fqn: &str,
        natural_name: &str,
        imports: &HashMap<String, String>,
    ) -> Option<String> {
        if !imports.iter().any(|(name, imported)| {
            name.eq_ignore_ascii_case(natural_name) && !imported.eq_ignore_ascii_case(fqn)
        }) {
            return None;
        }

        let occupied: HashSet<String> = imports.keys().map(|name| name.to_lowercase()).collect();
        let mut parts = fqn.rsplit('\\');
        let short = parts.next().unwrap_or(natural_name);
        let parent = parts.next().unwrap_or("Imported");
        let base = format!("{}{}", parent, short);
        if !occupied.contains(&base.to_lowercase()) {
            return Some(base);
        }
        for suffix in 2.. {
            let candidate = format!("{}{}", base, suffix);
            if !occupied.contains(&candidate.to_lowercase()) {
                return Some(candidate);
            }
        }
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::*;

    fn action(content: &str, needle: &str) -> CodeAction {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        backend.update_ast(uri, content);
        let offset = content.find(needle).unwrap();
        let pos = crate::text_position::offset_to_position(content, offset);
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
        backend
            .handle_code_action(uri, content, &params)
            .into_iter()
            .find_map(|action| match action {
                CodeActionOrCommand::CodeAction(action)
                    if action.title.contains("shorten usages") =>
                {
                    Some(action)
                }
                _ => None,
            })
            .expect("expected import-and-shorten action")
    }

    fn new_texts(action: &CodeAction) -> Vec<&str> {
        action
            .edit
            .as_ref()
            .unwrap()
            .changes
            .as_ref()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .iter()
            .map(|edit| edit.new_text.as_str())
            .collect()
    }

    #[test]
    fn imports_relative_class_and_replaces_all_usages() {
        let src = "<?php\nnamespace App;\n\nnew Node\\Expr\\Call();\nNode\\Expr\\Call::make();\n";
        let action = action(src, "Node\\Expr\\Call");
        let texts = new_texts(&action);
        assert!(texts.contains(&"\nuse App\\Node\\Expr\\Call;\n"));
        assert_eq!(texts.iter().filter(|text| **text == "Call").count(), 2);
    }

    #[test]
    fn imports_absolute_function() {
        let src = "<?php\nnamespace App;\n\n\\Vendor\\Tools\\run();\n";
        let action = action(src, "Vendor\\Tools\\run");
        let texts = new_texts(&action);
        assert!(texts.contains(&"\nuse function Vendor\\Tools\\run;\n"));
        assert!(texts.contains(&"run"));
    }

    #[test]
    fn imports_absolute_constant() {
        let src = "<?php\nnamespace App;\n\n$value = \\Vendor\\Config\\ENABLED;\n";
        let action = action(src, "Vendor\\Config\\ENABLED");
        let texts = new_texts(&action);
        assert!(texts.contains(&"\nuse const Vendor\\Config\\ENABLED;\n"));
        assert!(texts.contains(&"ENABLED"));
    }

    #[test]
    fn aliases_conflicting_class_import() {
        let src = "<?php\nnamespace App;\n\nuse Other\\Call;\n\nnew \\Node\\Expr\\Call();\n";
        let action = action(src, "Node\\Expr\\Call");
        let texts = new_texts(&action);
        assert!(texts.contains(&"use Node\\Expr\\Call as ExprCall;\n"));
        assert!(texts.contains(&"ExprCall"));
    }
}
