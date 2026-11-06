//! Renaming a class import's alias.
//!
//! An alias is a name local to the `namespace` block that declares it, so
//! renaming one, from the `as` clause or from a name written through it,
//! rewrites only that block: the clause and the leading segment of every
//! name that reaches the class through it.  The class keeps its name.

use std::collections::HashMap;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::code_actions::import_alias::{ClassImport, ImportAliasScope};
use crate::code_actions::multi_file_edit;
use crate::text_position::{byte_range_to_lsp_range, position_to_byte_offset};

use super::RenameOutcome;
use super::validate::is_name_token;

/// Names PHP reserves, which no import can bind.
const RESERVED_NAMES: &[&str] = &[
    "array", "bool", "callable", "false", "float", "int", "iterable", "mixed", "never", "null",
    "object", "parent", "self", "static", "string", "true", "void",
];

/// An alias under the cursor, with the import that declares it.
pub(super) struct AliasRename<'a> {
    scope: ImportAliasScope<'a>,
    import: ClassImport,
    /// The alias as written where the cursor is.
    token: (usize, usize),
}

impl Backend {
    /// The import alias at `position`: the alias in a `use … as Alias;`
    /// clause, or the leading segment of a name written through one.
    pub(super) fn alias_rename_at<'a>(
        &self,
        uri: &str,
        content: &'a str,
        position: Position,
    ) -> Option<AliasRename<'a>> {
        let cursor = position_to_byte_offset(content, position);
        let scope = self.import_alias_scope(uri, content, cursor)?;

        if let Some(import) = scope.import_under_cursor(cursor)
            && import.has_alias()
            && let Some(token) = import.alias_token(content)
            && token.0 <= cursor
            && cursor <= token.1
        {
            return Some(AliasRename {
                scope,
                import,
                token,
            });
        }

        let token = scope.leading_segment_at(cursor)?;
        let import = scope.aliased_import(&content[token.0..token.1])?;
        Some(AliasRename {
            scope,
            import,
            token,
        })
    }
}

impl AliasRename<'_> {
    pub(super) fn prepare(&self, content: &str) -> PrepareRenameResult {
        PrepareRenameResult::PrepareRenamePlaceholder(PrepareRenamePlaceholder {
            range: byte_range_to_lsp_range(content, self.token.0, self.token.1),
            placeholder: self.import.alias.clone(),
        })
    }

    pub(super) fn rename(&self, uri: &str, new_name: &str) -> RenameOutcome {
        if new_name.contains('\\')
            || !is_name_token(new_name)
            || RESERVED_NAMES
                .iter()
                .any(|reserved| reserved.eq_ignore_ascii_case(new_name))
        {
            return Err(format!("`{new_name}` is not a valid alias"));
        }
        // A change of case alone keeps binding the same name.
        if !new_name.eq_ignore_ascii_case(&self.import.alias) && self.scope.is_bound(new_name) {
            return Err(format!(
                "`{new_name}` already names a class in this file, so `{}` cannot take it",
                self.import.alias
            ));
        }
        let Ok(doc_uri) = uri.parse::<Uri>() else {
            return Ok(None);
        };
        let edits = self.scope.rebind_edits(&self.import, new_name);
        Ok(Some(multi_file_edit(HashMap::from([(doc_uri, edits)]))))
    }
}
