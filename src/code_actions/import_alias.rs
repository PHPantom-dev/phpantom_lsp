//! Alias a class import, or remove the alias from one.
//!
//! On a `use Foo\Bar;` import, "Alias `Bar` as `FooBar`" adds an `as`
//! clause and rewrites every reference the block makes through the
//! import.  On a `use Foo\Bar as Baz;` import, "Remove alias `Baz`" drops
//! the clause and rewrites the references back to `Bar`.  Neither is
//! offered when the name it switches to is already bound in the block,
//! since the rewritten references would then name another class.

use std::collections::HashSet;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::backend::file_access::ImportBlock;
use crate::completion::use_edit::{import_alias, import_name_taken};
use crate::diagnostics::use_statements::{find_use_member, scan_use_statements};
use crate::symbol_map::{ClassRefContext, SymbolKind, SymbolMap};
use crate::text_position::byte_range_to_lsp_range;
use crate::util::short_name;

/// The class import under the cursor.
struct ImportUnderCursor {
    /// The imported class, as the statement spells it.
    fqn: String,
    /// The name the import binds: the alias, or the short name.
    alias: String,
    /// The import's item in the `use` statement, `as` clause included.
    member: (usize, usize),
}

/// The leading segment of a class name written at `start..end`, when the
/// name is relative (no leading `\`), as a byte range.
fn leading_segment(content: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let text = content.get(start..end)?;
    let first = text.split('\\').next()?;
    let is_identifier = !first.is_empty()
        && first
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80);
    is_identifier.then_some((start, start + first.len()))
}

impl Backend {
    pub(crate) fn collect_import_alias_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        let cursor = crate::text_position::position_to_byte_offset(content, params.range.start);
        let Some(symbol_map) = self.symbol_map_for(uri) else {
            return;
        };
        if !symbol_map.matches_source(content) {
            return;
        }
        let block = self.import_block_at(uri, cursor);
        let Some(import) = import_under_cursor(content, &symbol_map, &block, cursor) else {
            return;
        };
        let Ok(doc_uri) = uri.parse::<Uri>() else {
            return;
        };

        // The relative class names the block writes, as the byte range of
        // each one's leading segment: the part an import binds.
        let in_block = |offset: usize| {
            block
                .range
                .is_none_or(|(start, end)| offset >= start && offset <= end)
        };
        let leading_segments: Vec<(usize, usize)> = symbol_map
            .spans
            .iter()
            .filter(|span| {
                matches!(
                    &span.kind,
                    SymbolKind::ClassReference { context, .. }
                        if *context != ClassRefContext::UseImport
                ) && in_block(span.start as usize)
            })
            .filter_map(|span| leading_segment(content, span.start as usize, span.end as usize))
            .collect();
        let local_classes: HashSet<String> = self
            .local_class_names(uri, block.namespace.as_deref())
            .into_iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let is_bound = |name: &str| {
            import_name_taken(name, &block.use_map)
                || local_classes.contains(&name.to_ascii_lowercase())
                || leading_segments
                    .iter()
                    .any(|&(s, e)| content[s..e].eq_ignore_ascii_case(name))
        };

        let natural_name = short_name(&import.fqn);
        let has_alias = !import.alias.eq_ignore_ascii_case(natural_name);
        let (title, kind, new_name) = if has_alias {
            if is_bound(natural_name) {
                return;
            }
            (
                format!("Remove alias `{}`", import.alias),
                CodeActionKind::RefactorInline,
                natural_name.to_string(),
            )
        } else {
            let alias = import_alias(&import.fqn, is_bound);
            (
                format!("Alias `{}` as `{}`", natural_name, alias),
                CodeActionKind::RefactorRewrite,
                alias,
            )
        };

        let (member_start, member_end) = import.member;
        let member_text = &content[member_start..member_end];
        let member_name = member_text
            .split_once(" as ")
            .map_or(member_text, |(name, _)| name)
            .trim_end();
        let new_member = if has_alias {
            member_name.to_string()
        } else {
            format!("{} as {}", member_name, new_name)
        };

        let mut edits = vec![TextEdit {
            range: byte_range_to_lsp_range(content, member_start, member_end),
            new_text: new_member,
        }];
        for &(start, end) in &leading_segments {
            if content[start..end].eq_ignore_ascii_case(&import.alias) {
                edits.push(TextEdit {
                    range: byte_range_to_lsp_range(content, start, end),
                    new_text: new_name.clone(),
                });
            }
        }

        out.push(CodeActionResponse::CodeAction(CodeAction {
            title,
            kind: Some(kind),
            diagnostics: None,
            edit: Some(crate::code_actions::single_file_edit(doc_uri, edits)),
            command: None,
            is_preferred: None,
            disabled: None,
            data: None,
            tags: None,
        }));
    }
}

/// The class import of the `use` statement under `cursor`: the item the
/// cursor is on, or the statement's only class import when the cursor is
/// elsewhere in the statement (on the `use` keyword, say).
fn import_under_cursor(
    content: &str,
    symbol_map: &SymbolMap,
    block: &ImportBlock,
    cursor: usize,
) -> Option<ImportUnderCursor> {
    let statement = scan_use_statements(content)
        .into_iter()
        .find(|stmt| stmt.top_level && stmt.keyword_start <= cursor && cursor <= stmt.end)?;
    let decl_start = statement.keyword_start;
    let statement_text = &content[decl_start..statement.end];
    let decl = statement_text.split(';').next().unwrap_or(statement_text);

    let mut imports = Vec::new();
    for span in &symbol_map.spans {
        let SymbolKind::ClassReference {
            name,
            context: ClassRefContext::UseImport,
            ..
        } = &span.kind
        else {
            continue;
        };
        let start = span.start as usize;
        if start < decl_start || start > statement.end {
            continue;
        }
        let found = block.use_map.iter().find_map(|(alias, fqn)| {
            if !fqn.eq_ignore_ascii_case(name) {
                return None;
            }
            let member = find_use_member(decl, fqn, alias)?;
            let range = (decl_start + member.start, decl_start + member.end);
            (range.0 <= start && start <= range.1).then(|| ImportUnderCursor {
                fqn: fqn.clone(),
                alias: alias.clone(),
                member: range,
            })
        });
        if let Some(import) = found {
            imports.push(import);
        }
    }

    if let Some(index) = imports
        .iter()
        .position(|import| import.member.0 <= cursor && cursor <= import.member.1)
    {
        return Some(imports.swap_remove(index));
    }
    (imports.len() == 1).then(|| imports.remove(0))
}
