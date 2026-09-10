//! Import a qualified symbol and replace its usages with a short name.

use std::collections::{HashMap, HashSet};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::use_edit::{
    UseBlockInfo, analyze_use_block, build_aliased_typed_use_edit, build_aliased_use_edit,
};
use crate::symbol_map::{ClassRefContext, SymbolKind, SymbolMap, SymbolSpan};
use crate::text_position::position_to_byte_offset;
use crate::types::{FileContext, NamespaceSpan};
use crate::util::{short_name, strip_fqn_prefix};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
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

    /// Position of this kind's group within a `use` block, matching the
    /// order `UseBlockInfo` sorts imports into (class, const, function).
    fn group_order(self) -> u8 {
        match self {
            Self::Class => 0,
            Self::Constant => 1,
            Self::Function => 2,
        }
    }
}

/// Every occurrence of one qualified symbol inside a single namespace
/// block, grouped under the fully-qualified name they all resolve to.
struct QualifiedSymbol<'a> {
    kind: ImportKind,
    fqn: String,
    spans: Vec<&'a SymbolSpan>,
    /// Whether any occurrence is written qualified.  A group made up
    /// entirely of short names is already imported and needs no action.
    any_qualified: bool,
}

/// The file an import is being planned into.
struct FileImports<'a> {
    content: &'a str,
    use_block: &'a UseBlockInfo,
    use_map: &'a HashMap<String, String>,
    namespace: &'a Option<String>,
}

/// State threaded through a batch of imports so each one sees the short
/// names the earlier ones claimed.
struct ImportBatch {
    /// Short names bound in this file, by an existing `use` or by an
    /// earlier import in the same batch, mapped to the FQN they bind.
    claimed: HashMap<String, String>,
    planned_any: bool,
}

impl ImportBatch {
    fn new(use_map: &HashMap<String, String>) -> Self {
        Self {
            claimed: use_map.clone(),
            planned_any: false,
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

fn import_action(title: String, doc_uri: Url, edits: Vec<TextEdit>) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        diagnostics: None,
        edit: Some(crate::code_actions::single_file_edit(doc_uri, edits)),
        command: None,
        is_preferred: Some(false),
        disabled: None,
        data: None,
    })
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
        let cursor_fqn =
            strip_fqn_prefix(&file.resolve_name_at(written_name, cursor_span.start)).to_string();
        if !cursor_fqn.contains('\\') {
            return;
        }
        let Ok(doc_uri) = uri.parse::<Url>() else {
            return;
        };

        let namespace = self.namespace_at_offset(uri, cursor_span.start);
        let namespace_spans = self.namespace_spans_for_uri(uri);
        let use_block = analyze_use_block(content);
        let imports = FileImports {
            content,
            use_block: &use_block,
            use_map: &file.use_map,
            namespace: &namespace,
        };

        let symbols = self.qualified_symbols(&symbol_map, &file, &namespace_spans, &namespace);

        // ── Import the symbol under the cursor ──────────────────────────
        let Some(cursor_symbol) = symbols
            .iter()
            .find(|symbol| symbol.kind == kind && symbol.fqn.eq_ignore_ascii_case(&cursor_fqn))
        else {
            return;
        };
        let mut batch = ImportBatch::new(&file.use_map);
        if let Some((alias, edits)) = plan_import(cursor_symbol, &imports, &mut batch) {
            let title = match alias {
                Some(alias) => format!(
                    "Import {} `{}` as `{}` and shorten usages",
                    cursor_symbol.kind.label(),
                    cursor_symbol.fqn,
                    alias
                ),
                None => format!(
                    "Import {} `{}` and shorten usages",
                    cursor_symbol.kind.label(),
                    cursor_symbol.fqn
                ),
            };
            out.push(import_action(title, doc_uri.clone(), edits));
        }

        // ── Import every qualified symbol in the same namespace ─────────
        // Only worth offering when it covers more than the cursor symbol
        // the first action already handles.
        let mut batch = ImportBatch::new(&file.use_map);
        let mut all_edits = Vec::new();
        let mut planned = 0;
        for symbol in &symbols {
            if let Some((_, edits)) = plan_import(symbol, &imports, &mut batch) {
                all_edits.extend(edits);
                planned += 1;
            }
        }
        if planned < 2 {
            return;
        }
        out.push(import_action(
            "Import all qualified symbols and shorten usages".to_string(),
            doc_uri,
            all_edits,
        ));
    }

    /// Group every importable reference in `namespace` by the
    /// fully-qualified name it resolves to, in `use`-block order so a
    /// batch of imports comes out deterministically sorted.
    fn qualified_symbols<'a>(
        &self,
        symbol_map: &'a SymbolMap,
        file: &FileContext,
        namespace_spans: &[NamespaceSpan],
        namespace: &Option<String>,
    ) -> Vec<QualifiedSymbol<'a>> {
        let mut symbols: Vec<QualifiedSymbol<'a>> = Vec::new();
        let mut index: HashMap<(ImportKind, String), usize> = HashMap::new();

        for span in &symbol_map.spans {
            let Some((name, kind)) = symbol_name_and_kind(span) else {
                continue;
            };
            if self.namespace_at_offset_from_spans(namespace_spans, span.start) != *namespace {
                continue;
            }
            let resolved = file.resolve_name_at(name, span.start);
            let fqn = strip_fqn_prefix(&resolved);
            if !fqn.contains('\\') {
                continue;
            }
            let position = *index.entry((kind, fqn.to_lowercase())).or_insert_with(|| {
                symbols.push(QualifiedSymbol {
                    kind,
                    fqn: fqn.to_string(),
                    spans: Vec::new(),
                    any_qualified: false,
                });
                symbols.len() - 1
            });
            symbols[position].spans.push(span);
            symbols[position].any_qualified |= name.contains('\\');
        }

        symbols.retain(|symbol| symbol.any_qualified);
        symbols.sort_by(|a, b| {
            a.kind
                .group_order()
                .cmp(&b.kind.group_order())
                .then_with(|| a.fqn.to_lowercase().cmp(&b.fqn.to_lowercase()))
        });
        symbols
    }

    fn namespace_at_offset_from_spans(
        &self,
        spans: &[NamespaceSpan],
        offset: u32,
    ) -> Option<String> {
        spans
            .iter()
            .find(|span| offset >= span.start && offset <= span.end)
            .and_then(|span| span.namespace.clone())
    }
}

/// Build the `use` statement and the short-name replacements for one
/// qualified symbol, returning the alias it had to be imported under (if
/// any) alongside the edits.
fn plan_import(
    symbol: &QualifiedSymbol<'_>,
    imports: &FileImports<'_>,
    batch: &mut ImportBatch,
) -> Option<(Option<String>, Vec<TextEdit>)> {
    let natural_name = short_name(&symbol.fqn);
    let alias = import_alias_for(&symbol.fqn, natural_name, &batch.claimed);
    let replacement = alias.as_deref().unwrap_or(natural_name);
    let already_imported = imports.use_map.iter().any(|(name, imported)| {
        name.eq_ignore_ascii_case(replacement) && imported.eq_ignore_ascii_case(&symbol.fqn)
    });

    let mut edits = Vec::new();
    if !already_imported {
        let mut import_edits = match symbol.kind.keyword() {
            None => build_aliased_use_edit(
                &symbol.fqn,
                alias.as_deref(),
                imports.use_block,
                imports.namespace,
            ),
            Some(keyword) => build_aliased_typed_use_edit(
                &symbol.fqn,
                alias.as_deref(),
                keyword,
                imports.use_block,
            ),
        }?;
        // With no existing `use` block every import in a batch inserts at
        // the same fallback position, and each would prepend the blank
        // line that separates the block from the `namespace` line.  Only
        // the first one should.
        if batch.planned_any
            && imports.use_block.existing.is_empty()
            && let Some(first) = import_edits.first_mut()
            && let Some(rest) = first.new_text.strip_prefix('\n')
        {
            first.new_text = rest.to_string();
        }
        edits.extend(import_edits);
    }

    for span in &symbol.spans {
        let Some((name, _)) = symbol_name_and_kind(span) else {
            continue;
        };
        if name.eq_ignore_ascii_case(replacement) && !name.contains('\\') {
            continue;
        }
        edits.push(replacement_edit(span, replacement, imports.content));
    }

    if edits.is_empty() {
        return None;
    }
    batch
        .claimed
        .insert(replacement.to_string(), symbol.fqn.clone());
    batch.planned_any = true;
    Some((alias, edits))
}

/// Pick an alias for `fqn` when its natural short name is already bound
/// to something else, preferring a name derived from the parent
/// namespace segment (`Node\Expr\Call` → `ExprCall`).
fn import_alias_for(
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

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::*;

    const BULK_TITLE: &str = "Import all qualified symbols and shorten usages";

    fn actions_at(content: &str, needle: &str) -> Vec<CodeAction> {
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
            .filter_map(|action| match action {
                CodeActionOrCommand::CodeAction(action)
                    if action.title.contains("shorten usages") =>
                {
                    Some(action)
                }
                _ => None,
            })
            .collect()
    }

    fn action(content: &str, needle: &str) -> CodeAction {
        actions_at(content, needle)
            .into_iter()
            .find(|action| action.title != BULK_TITLE)
            .expect("expected import-and-shorten action")
    }

    fn bulk_action(content: &str, needle: &str) -> Option<CodeAction> {
        actions_at(content, needle)
            .into_iter()
            .find(|action| action.title == BULK_TITLE)
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

    #[test]
    fn bulk_action_imports_every_qualified_symbol() {
        let src = "<?php\nnamespace App;\n\nuse Existing\\Thing;\n\nnew \\Vendor\\Alpha();\n\\Vendor\\Beta::make();\n\\Vendor\\Tools\\run();\n$v = \\Vendor\\Config\\ENABLED;\n";
        let action = bulk_action(src, "Vendor\\Alpha").expect("expected bulk action");
        let texts = new_texts(&action);
        assert!(texts.contains(&"use Vendor\\Alpha;\n"));
        assert!(texts.contains(&"use Vendor\\Beta;\n"));
        assert!(texts.contains(&"\nuse const Vendor\\Config\\ENABLED;\n"));
        assert!(texts.contains(&"\nuse function Vendor\\Tools\\run;\n"));
        assert!(texts.contains(&"Alpha"));
        assert!(texts.contains(&"Beta"));
        assert!(texts.contains(&"run"));
        assert!(texts.contains(&"ENABLED"));
    }

    #[test]
    fn bulk_action_is_not_offered_for_a_lone_symbol() {
        let src = "<?php\nnamespace App;\n\nnew \\Vendor\\Alpha();\n\\Vendor\\Alpha::make();\n";
        assert!(bulk_action(src, "Vendor\\Alpha").is_none());
    }

    #[test]
    fn bulk_action_aliases_short_name_collisions_within_the_batch() {
        let src = "<?php\nnamespace App;\n\nnew \\One\\Thing();\nnew \\Two\\Thing();\n";
        let action = bulk_action(src, "One\\Thing").expect("expected bulk action");
        let texts = new_texts(&action);
        assert!(texts.contains(&"\nuse One\\Thing;\n"));
        assert!(texts.contains(&"use Two\\Thing as TwoThing;\n"));
        assert!(texts.contains(&"Thing"));
        assert!(texts.contains(&"TwoThing"));
    }

    #[test]
    fn bulk_action_separates_the_use_block_from_the_namespace_once() {
        let src = "<?php\nnamespace App;\n\nnew \\Vendor\\Alpha();\nnew \\Vendor\\Beta();\n";
        let action = bulk_action(src, "Vendor\\Alpha").expect("expected bulk action");
        let texts = new_texts(&action);
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.starts_with("\nuse "))
                .count(),
            1
        );
        assert!(texts.contains(&"use Vendor\\Beta;\n"));
    }

    #[test]
    fn bulk_action_skips_other_namespace_blocks() {
        let src = "<?php\nnamespace App {\n    new \\Vendor\\Alpha();\n    new \\Vendor\\Beta();\n}\nnamespace Other {\n    new \\Vendor\\Gamma();\n}\n";
        let action = bulk_action(src, "Vendor\\Alpha").expect("expected bulk action");
        let texts = new_texts(&action);
        assert!(texts.contains(&"Alpha"));
        assert!(texts.contains(&"Beta"));
        assert!(!texts.iter().any(|text| text.contains("Gamma")));
    }
}
