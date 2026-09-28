//! Rewriting one file's references to a renamed or moved class.
//!
//! A rename and a move both walk the files that reference the class one
//! at a time; this is the per-file half they share: reading the file and
//! what its imports say about how it reaches the class, deciding what
//! each short-name reference becomes, and turning the reference
//! locations into edits.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::text_position::{position_to_byte_offset, ranges_overlap};

use super::super::parse_edit_target_uri;
use super::imports::{
    ImportInfo, RenameTarget, build_use_statement_edit, find_import_for_fqn, has_import_collision,
    pick_collision_alias,
};

/// One file that references the class, with what its own imports say
/// about how it reaches the class.
pub(super) struct FileRewrite {
    /// The text the reference locations index into: for a template, the
    /// virtual PHP it lowers to.
    pub(super) content: String,
    /// The URI the file's edits are filed under.
    pub(super) target_uri: Url,
    /// The file's `alias → FQN` import table.
    pub(super) use_map: HashMap<String, String>,
    /// The file's import of the class, when it has one.
    pub(super) import_info: Option<ImportInfo>,
}

/// How one file's in-code references to the class are rewritten.
pub(super) struct ReferenceRewrite {
    /// The new short name is already imported under another FQN, so the
    /// rewritten import needs an alias.
    has_collision: bool,
    /// References through the file's own explicit alias stay as they are.
    skip_alias_refs: bool,
    /// What a short-name reference becomes.
    in_code_replacement: String,
    /// Whether short-name references are rewritten at all: a move that
    /// keeps the short name and adds no aliased import leaves them alone.
    rewrite_short_refs: bool,
}

impl Backend {
    /// Read `uri` and its import table for a rename or move of `old_fqn`.
    ///
    /// Reference locations in a template are recorded against the virtual
    /// PHP it lowers to, so the text behind them is read there too; the
    /// edits are translated back by [`Self::rewrite_template_edits`].
    pub(super) fn file_rewrite(&self, uri: &str, old_fqn: &str) -> Option<FileRewrite> {
        let content = self.reference_file_content(uri)?;
        let target_uri = parse_edit_target_uri(uri)?;
        let use_map = self
            .file_imports
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let import_info = find_import_for_fqn(&use_map, old_fqn);
        Some(FileRewrite {
            content,
            target_uri,
            use_map,
            import_info,
        })
    }
}

impl FileRewrite {
    /// Decide what the file's in-code references become.
    ///
    /// - An import with an explicit alias keeps it, and references through
    ///   the alias are left alone.
    /// - An import whose new short name collides with another import is
    ///   given an alias, which the references switch to.
    /// - Otherwise references switch to `new_import_alias` when the move
    ///   adds an aliased import for this file, to the new short name when
    ///   it changed, and stay as they are when it did not.
    pub(super) fn reference_rewrite(
        &self,
        old_fqn: &str,
        old_short_name: &str,
        new_short_name: &str,
        new_import_alias: Option<&str>,
    ) -> ReferenceRewrite {
        let class_name_changed = old_short_name != new_short_name;
        let has_collision = class_name_changed
            && self.import_info.is_some()
            && has_import_collision(&self.use_map, old_fqn, new_short_name);
        let (skip_alias_refs, in_code_replacement) = match &self.import_info {
            Some(info) if info.has_explicit_alias => (true, info.alias.clone()),
            Some(_) if has_collision => {
                (false, pick_collision_alias(new_short_name, &self.use_map))
            }
            _ => match new_import_alias {
                Some(alias) => (false, alias.to_string()),
                None if class_name_changed => (false, new_short_name.to_string()),
                None => (true, old_short_name.to_string()),
            },
        };
        ReferenceRewrite {
            has_collision,
            skip_alias_refs,
            in_code_replacement,
            rewrite_short_refs: class_name_changed || new_import_alias.is_some(),
        }
    }

    /// The edit that brings the file's import of the class up to date,
    /// with the range it covers so a reference location inside it is not
    /// rewritten twice.  `None` when the file has no import to update.
    pub(super) fn use_statement_edit(
        &self,
        old_fqn: &str,
        new_fqn: &str,
        new_short_name: &str,
        rewrite: &ReferenceRewrite,
        file_namespace: Option<&str>,
    ) -> Option<(Range, Vec<TextEdit>)> {
        let info = self.import_info.as_ref()?;
        build_use_statement_edit(
            &self.content,
            old_fqn,
            &RenameTarget {
                new_fqn,
                new_short_name,
                has_collision: rewrite.has_collision,
            },
            info,
            &self.use_map,
            file_namespace,
        )
    }

    /// The edits for the file's reference locations, and whether any of
    /// them spells the class by its short name.
    ///
    /// A location the use-statement edit already covers is left to that
    /// edit, and `self`, `static`, and `parent` name the class without
    /// spelling it.  A qualified reference is rewritten by `qualified`; a
    /// short-name reference follows `rewrite`.
    pub(super) fn rewrite_locations(
        &self,
        locations: &[&Location],
        use_statement_edit: Option<&(Range, Vec<TextEdit>)>,
        rewrite: &ReferenceRewrite,
        qualified: impl Fn(&str) -> String,
    ) -> (Vec<TextEdit>, bool) {
        let mut edits = Vec::new();
        let mut has_short_name_ref = false;
        for loc in locations {
            let start = position_to_byte_offset(&self.content, loc.range.start);
            let end = position_to_byte_offset(&self.content, loc.range.end);
            let source_text = self.content.get(start..end).unwrap_or("");

            if let Some((covered, _)) = use_statement_edit
                && ranges_overlap(&loc.range, covered)
            {
                continue;
            }
            if matches!(source_text, "self" | "static" | "parent") {
                continue;
            }

            if source_text.contains('\\') {
                edits.push(TextEdit {
                    range: loc.range,
                    new_text: qualified(source_text),
                });
            } else if rewrite.skip_alias_refs
                && self
                    .import_info
                    .as_ref()
                    .is_some_and(|info| source_text.eq_ignore_ascii_case(&info.alias))
            {
                continue;
            } else {
                has_short_name_ref = true;
                if rewrite.rewrite_short_refs {
                    edits.push(TextEdit {
                        range: loc.range,
                        new_text: rewrite.in_code_replacement.clone(),
                    });
                }
            }
        }
        (edits, has_short_name_ref)
    }
}
