//! Alias a class import, or remove the alias from one.
//!
//! On a `use Foo\Bar;` import, "Alias `Bar` as `FooBar`" adds an `as`
//! clause and rewrites every reference the block makes through the
//! import.  On a `use Foo\Bar as Baz;` import, "Remove alias `Baz`" drops
//! the clause and rewrites the references back to `Bar`.  Neither is
//! offered when the name it switches to is already bound in the block,
//! since the rewritten references would then name another class.
//!
//! [`ImportAliasScope`] is shared with the rename of an alias, which
//! makes the same edits under a name the user picks.

use std::collections::HashSet;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::backend::file_access::ImportBlock;
use crate::completion::use_edit::{import_alias, import_name_taken};
use crate::diagnostics::use_statements::{find_use_member, scan_use_statements};
use crate::symbol_map::{ClassRefContext, SymbolKind};
use crate::text_position::byte_range_to_lsp_range;
use crate::util::short_name;

/// One class import of a `use` statement.
pub(crate) struct ClassImport {
    /// The imported class, as the statement spells it.
    pub(crate) fqn: String,
    /// The name the import binds: the alias, or the short name.
    pub(crate) alias: String,
    /// The import's item in the `use` statement, `as` clause included.
    member: (usize, usize),
}

impl ClassImport {
    /// Whether the import has an `as` clause that binds a name other than
    /// the class's own.
    pub(crate) fn has_alias(&self) -> bool {
        !self.alias.eq_ignore_ascii_case(short_name(&self.fqn))
    }

    /// The byte range of the alias in the `as` clause, when there is one.
    pub(crate) fn alias_token(&self, content: &str) -> Option<(usize, usize)> {
        let (start, end) = self.member;
        let member = &content[start..end];
        let after_as = member.find(" as ")? + " as ".len();
        let rest = &member[after_as..];
        let token_start = start + after_as + (rest.len() - rest.trim_start().len());
        let token_end = token_start + self.alias.len();
        content
            .get(token_start..token_end)
            .is_some_and(|token| token == self.alias)
            .then_some((token_start, token_end))
    }
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

/// The `namespace` block an import is declared in, with every name the
/// block binds or writes, so an import's name can be switched without
/// making a reference name another class.
pub(crate) struct ImportAliasScope<'a> {
    content: &'a str,
    block: ImportBlock,
    /// The leading segment of every relative class name the block writes
    /// outside its `use` statements: the part an import binds.
    leading_segments: Vec<(usize, usize)>,
    /// The classes the block declares, lowercased.
    local_classes: HashSet<String>,
}

impl Backend {
    /// The import scope of the block containing `offset`, `None` in a
    /// Blade template or when the symbol map predates `content`.
    pub(crate) fn import_alias_scope<'a>(
        &self,
        uri: &str,
        content: &'a str,
        offset: usize,
    ) -> Option<ImportAliasScope<'a>> {
        if self.is_blade_file(uri) {
            return None;
        }
        let symbol_map = self.symbol_map_for(uri)?;
        if !symbol_map.matches_source(content) {
            return None;
        }
        let block = self.import_block_at(uri, offset);
        let leading_segments = symbol_map
            .spans
            .iter()
            .filter(|span| {
                matches!(
                    &span.kind,
                    SymbolKind::ClassReference { context, .. }
                        if *context != ClassRefContext::UseImport
                ) && block_contains(&block, span.start as usize)
            })
            .filter_map(|span| leading_segment(content, span.start as usize, span.end as usize))
            .collect();
        let local_classes = self
            .local_class_names(uri, block.namespace.as_deref())
            .into_iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        Some(ImportAliasScope {
            content,
            block,
            leading_segments,
            local_classes,
        })
    }
}

fn block_contains(block: &ImportBlock, offset: usize) -> bool {
    block
        .range
        .is_none_or(|(start, end)| offset >= start && offset <= end)
}

impl ImportAliasScope<'_> {
    /// Whether `name` already means something in the block: an import, a
    /// class it declares, or the leading segment of a name it writes.
    pub(crate) fn is_bound(&self, name: &str) -> bool {
        import_name_taken(name, &self.block.use_map)
            || self.local_classes.contains(&name.to_ascii_lowercase())
            || self
                .leading_segments
                .iter()
                .any(|&(s, e)| self.content[s..e].eq_ignore_ascii_case(name))
    }

    /// The class import of the `use` statement under `cursor`: the item
    /// the cursor is on, or the statement's only class import when the
    /// cursor is elsewhere in the statement (on the `use` keyword, say).
    pub(crate) fn import_under_cursor(&self, cursor: usize) -> Option<ClassImport> {
        let statement = scan_use_statements(self.content)
            .into_iter()
            .find(|stmt| stmt.top_level && stmt.keyword_start <= cursor && cursor <= stmt.end)?;
        let mut imports = self.statement_imports(statement.keyword_start, statement.end);
        if let Some(index) = imports
            .iter()
            .position(|import| import.member.0 <= cursor && cursor <= import.member.1)
        {
            return Some(imports.swap_remove(index));
        }
        (imports.len() == 1).then(|| imports.remove(0))
    }

    /// The import that binds `alias` in this block, when it has an `as`
    /// clause.
    pub(crate) fn aliased_import(&self, alias: &str) -> Option<ClassImport> {
        let (alias, _) = self
            .block
            .use_map
            .iter()
            .find(|(bound, _)| bound.eq_ignore_ascii_case(alias))?;
        scan_use_statements(self.content)
            .into_iter()
            .filter(|stmt| stmt.top_level && block_contains(&self.block, stmt.keyword_start))
            .flat_map(|stmt| self.statement_imports(stmt.keyword_start, stmt.end))
            .find(|import| import.alias == *alias && import.has_alias())
    }

    /// The leading segment of a name the block writes that contains
    /// `offset`.
    pub(crate) fn leading_segment_at(&self, offset: usize) -> Option<(usize, usize)> {
        self.leading_segments
            .iter()
            .copied()
            .find(|&(start, end)| start <= offset && offset <= end)
    }

    /// The class imports of the `use` statement spanning `start..end`.
    fn statement_imports(&self, start: usize, end: usize) -> Vec<ClassImport> {
        let statement_text = &self.content[start..end];
        let decl = statement_text.split(';').next().unwrap_or(statement_text);
        let mut imports = Vec::new();
        for (alias, fqn) in &self.block.use_map {
            let Some(member) = find_use_member(decl, fqn, alias) else {
                continue;
            };
            imports.push(ClassImport {
                fqn: fqn.clone(),
                alias: alias.clone(),
                member: (start + member.start, start + member.end),
            });
        }
        imports.sort_by_key(|import| import.member.0);
        imports
    }

    /// The edits that make `import` bind `new_name` instead: its `as`
    /// clause (dropped when `new_name` is the class's own short name) and
    /// every name the block writes through it.
    pub(crate) fn rebind_edits(&self, import: &ClassImport, new_name: &str) -> Vec<TextEdit> {
        let (member_start, member_end) = import.member;
        let member_text = &self.content[member_start..member_end];
        let member_name = member_text
            .split_once(" as ")
            .map_or(member_text, |(name, _)| name)
            .trim_end();
        let new_member = if new_name.eq_ignore_ascii_case(short_name(member_name)) {
            member_name.to_string()
        } else {
            format!("{} as {}", member_name, new_name)
        };

        let mut edits = vec![TextEdit {
            range: byte_range_to_lsp_range(self.content, member_start, member_end),
            new_text: new_member,
        }];
        for &(start, end) in &self.leading_segments {
            if self.content[start..end].eq_ignore_ascii_case(&import.alias) {
                edits.push(TextEdit {
                    range: byte_range_to_lsp_range(self.content, start, end),
                    new_text: new_name.to_string(),
                });
            }
        }
        edits
    }
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
        let Some(scope) = self.import_alias_scope(uri, content, cursor) else {
            return;
        };
        let Some(import) = scope.import_under_cursor(cursor) else {
            return;
        };
        let Ok(doc_uri) = uri.parse::<Uri>() else {
            return;
        };

        let natural_name = short_name(&import.fqn);
        let (title, kind, new_name) = if import.has_alias() {
            if scope.is_bound(natural_name) {
                return;
            }
            (
                format!("Remove alias `{}`", import.alias),
                CodeActionKind::RefactorInline,
                natural_name.to_string(),
            )
        } else {
            let alias = import_alias(&import.fqn, |name| scope.is_bound(name));
            (
                format!("Alias `{}` as `{}`", natural_name, alias),
                CodeActionKind::RefactorRewrite,
                alias,
            )
        };

        let edits = scope.rebind_edits(&import, &new_name);
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
