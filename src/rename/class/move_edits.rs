//! Moving a class to another namespace: the namespace statement of its
//! own file, the references that have to follow it, and the file rename
//! its new PSR-4 location asks for.

use std::collections::HashMap;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolKind;
use crate::text_position::offset_to_position;
use crate::types::NamespaceSpan;
use crate::util::strip_fqn_prefix;

use super::RenameOutcome;
use super::imports::{has_import_collision, namespace_owns, pick_collision_alias};
use super::layout::{
    class_declaration_span, display_uri, insert_namespace_edit, remove_namespace_edits,
    section_neighbour,
};
use super::rewrite::{FileRewrite, locations_by_block};
use super::siblings::build_sibling_import_edits;
use super::{global_block_keyword_end, group_locations_by_file};

impl Backend {
    /// Plan a class move without requiring an LSP cursor position.
    pub(crate) fn plan_class_move(&self, old_fqn: &str, new_fqn: &str) -> RenameOutcome {
        let old_fqn = strip_fqn_prefix(old_fqn);
        let definition_uri = self
            .symbols
            .fqn_uri_index
            .read()
            .get(old_fqn)
            .cloned()
            .ok_or_else(|| format!("Class `{old_fqn}` was not found."))?;
        // `textDocument/rename` refuses a symbol declared in a vendor
        // package, and moving one headlessly is no safer: the edits would
        // land in a tree the next `composer install` overwrites.
        if self
            .workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| definition_uri.starts_with(prefix.as_str()))
        {
            return Err(format!(
                "`{old_fqn}` is declared in an installed package and cannot be moved."
            ));
        }
        let content = self
            .get_file_content(&definition_uri)
            .ok_or_else(|| format!("Could not read the definition of `{old_fqn}`."))?;
        let symbol_map = self
            .symbol_map_for(&definition_uri)
            .ok_or_else(|| format!("Could not index the definition of `{old_fqn}`."))?;
        let span = class_declaration_span(
            &symbol_map,
            &self.namespace_spans_for_uri(&definition_uri),
            old_fqn,
        )
        .ok_or_else(|| format!("Could not locate the declaration of `{old_fqn}`."))?;
        let position = offset_to_position(&content, span.start as usize);
        let locations = self
            .find_references_for_rename(&definition_uri, &content, position, true)
            .ok_or_else(|| format!("Could not find references to `{old_fqn}`."))?;
        if !self.rename_locations_verified(&span.kind, None, &locations) {
            return Err(format!(
                "The workspace changed while `{old_fqn}` was being indexed; retry the move."
            ));
        }
        self.build_class_move_edit(old_fqn, new_fqn, &locations)
    }

    /// Build a `WorkspaceEdit` that moves a class to a new FQN.
    ///
    /// Handles namespace change, class name change, file move, and
    /// updates all references across the workspace.  This is the
    /// handler for rename requests where `new_name` contains `\`.
    pub(in crate::rename) fn build_class_move_edit(
        &self,
        old_fqn: &str,
        new_fqn_raw: &str,
        locations: &[Location],
    ) -> RenameOutcome {
        let old_fqn = strip_fqn_prefix(old_fqn);
        let new_fqn = strip_fqn_prefix(new_fqn_raw);
        let old_short_name = crate::util::short_name(old_fqn);
        let new_short_name = crate::util::short_name(new_fqn);

        let old_ns = old_fqn.rfind('\\').map(|i| &old_fqn[..i]);
        let new_ns = new_fqn.rfind('\\').map(|i| &new_fqn[..i]);

        let class_name_changed = old_short_name != new_short_name;
        let namespace_changed = old_ns != new_ns;

        if !class_name_changed && !namespace_changed {
            return Ok(None);
        }

        // The destination has to be free before anything is emitted.
        // Every edit below assumes the class ends up at the new FQN, in
        // the file PSR-4 puts it in; letting it land on top of a class
        // that is already there would either clobber that file or leave
        // two declarations claiming one name.
        if let Some(occupant) = self.class_move_conflict(old_fqn, new_fqn) {
            return Err(occupant);
        }

        let mv = ClassMove {
            old_fqn,
            new_fqn,
            old_short_name,
            new_short_name,
            old_ns,
            new_ns,
            namespace_changed,
            definition_uri: self.symbols.fqn_uri_index.read().get(old_fqn).cloned(),
        };

        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
        for (file_uri_str, file_locations) in &group_locations_by_file(locations) {
            match self.class_move_file_edits(&mv, file_uri_str, file_locations)? {
                FileMoveEdits::Unreadable => {}
                FileMoveEdits::Stale => return Ok(None),
                FileMoveEdits::Edits(target_uri, file_edits) if !file_edits.is_empty() => {
                    changes.entry(target_uri).or_default().extend(file_edits);
                }
                FileMoveEdits::Edits(..) => {}
            }
        }

        self.collect_framework_class_edits(old_fqn, new_fqn, &mut changes);

        if changes.is_empty() {
            return Ok(None);
        }

        let file_move = self.compute_class_file_move(old_fqn, new_fqn);
        Ok(Some(Self::class_rename_workspace_edit(changes, file_move)))
    }

    /// Plan one file's part of a class move: its rewritten references,
    /// its import of the class, the `namespace` statement when it is the
    /// file declaring the class, and the import a former
    /// namespace-sibling now needs.
    fn class_move_file_edits(
        &self,
        mv: &ClassMove<'_>,
        file_uri_str: &str,
        file_locations: &[&Location],
    ) -> Result<FileMoveEdits, String> {
        let blocks = self.file_rewrites(file_uri_str, mv.old_fqn);
        let Some(first) = blocks.first() else {
            return Ok(FileMoveEdits::Unreadable);
        };

        let is_definition_file = mv.definition_uri.as_deref() == Some(file_uri_str);

        let mut file_edits: Vec<TextEdit> = Vec::new();

        if is_definition_file && mv.namespace_changed {
            match self.namespace_declaration_edits(mv, &blocks, file_uri_str)? {
                Some(edits) => file_edits.extend(edits),
                None => return Ok(FileMoveEdits::Stale),
            }
        }

        for (file, block_locations) in locations_by_block(&blocks, file_locations) {
            file_edits.extend(Self::class_move_block_edits(
                mv,
                file,
                &block_locations,
                is_definition_file,
            ));
        }

        self.rewrite_template_edits(file_uri_str, mv.new_fqn, mv.old_fqn, &mut file_edits);

        Ok(FileMoveEdits::Edits(first.target_uri.clone(), file_edits))
    }

    /// One `namespace` block's part of a class move: its rewritten
    /// references, its import of the class, and the import it needs when
    /// it reached the class through its own namespace.
    fn class_move_block_edits(
        mv: &ClassMove<'_>,
        file: &FileRewrite,
        block_locations: &[&Location],
        is_definition_file: bool,
    ) -> Vec<TextEdit> {
        let file_namespace = &file.namespace;

        // A block with no import for the class reached it through its
        // own namespace, so moving the class out of that namespace
        // leaves every short-name reference dangling.  Such a block
        // needs a `use` statement added.
        let needs_new_import = mv.namespace_changed
            && file.import_info.is_none()
            && !is_definition_file
            && namespace_owns(file_namespace.as_deref(), mv.old_fqn)
            && !namespace_owns(file_namespace.as_deref(), mv.new_fqn);

        // The short name may already be taken in this block by an
        // unrelated import, in which case the added import has to be
        // aliased and the references rewritten to that alias.
        let new_import_alias = if needs_new_import
            && has_import_collision(&file.use_map, mv.old_fqn, mv.new_short_name)
        {
            Some(pick_collision_alias(mv.new_short_name, &file.use_map))
        } else {
            None
        };

        let rewrite = file.reference_rewrite(
            mv.old_fqn,
            mv.old_short_name,
            mv.new_short_name,
            new_import_alias.as_deref(),
        );

        let use_statement_edit = file.use_statement_edit(
            mv.old_fqn,
            mv.new_fqn,
            mv.new_short_name,
            &rewrite,
            file_namespace.as_deref(),
        );

        // A qualified reference is rewritten in full, keeping its
        // leading backslash when it has one.
        let (mut edits, has_short_name_ref) = file.rewrite_locations(
            block_locations,
            use_statement_edit.as_ref(),
            &rewrite,
            |source| {
                if source.starts_with('\\') {
                    format!("\\{}", mv.new_fqn)
                } else {
                    mv.new_fqn.to_string()
                }
            },
        );

        if let Some((_, statement_edits)) = use_statement_edit {
            edits.extend(statement_edits);
        }

        // Only worth importing when the block actually spells the
        // class by its short name; a block that only ever writes the
        // FQN had its references rewritten in full above.
        if needs_new_import && has_short_name_ref {
            let use_block =
                crate::completion::use_edit::analyze_use_block_in(&file.content, file.block);
            if let Some(import_edits) = crate::completion::use_edit::build_aliased_use_edit(
                mv.new_fqn,
                new_import_alias.as_deref(),
                &use_block,
                file_namespace,
            ) {
                edits.extend(import_edits);
            }
        }

        edits
    }

    /// The edits that bring the declaring file's `namespace` statement to
    /// the destination namespace, together with the imports its former
    /// namespace-siblings then need.
    ///
    /// This is the one edit of a move built straight from symbol-map
    /// offsets rather than from a verified reference location, so it
    /// carries the same guard: `Ok(None)` abandons the move when the map
    /// does not describe the file, does not hold the class's declaration,
    /// or the span no longer spells the namespace it claims to.
    ///
    /// The statement names the namespace of everything its section
    /// declares, so `Err` refuses a move of a class that shares its
    /// section: the other declarations would change namespace with it
    /// while every reference to them stayed as it was.
    fn namespace_declaration_edits(
        &self,
        mv: &ClassMove<'_>,
        blocks: &[FileRewrite],
        file_uri_str: &str,
    ) -> Result<Option<Vec<TextEdit>>, String> {
        let Some(sm) = self.symbol_map_for(file_uri_str) else {
            return Ok(Some(Vec::new()));
        };

        // In a file with several `namespace` blocks, the statement that
        // moves is the one of the block declaring the class, and that
        // block's own imports are what its former siblings are planned
        // against.
        let namespaces = self.namespace_spans_for_uri(file_uri_str);
        let Some(class_start) =
            class_declaration_span(&sm, &namespaces, mv.old_fqn).map(|class| class.start)
        else {
            return Ok(None);
        };
        let (Some(class_block), Some(file)) = (
            NamespaceSpan::containing(&namespaces, class_start),
            blocks.get(FileRewrite::index_at(blocks, class_start as usize)),
        ) else {
            return Ok(None);
        };
        if !sm.matches_source(&file.content) {
            return Ok(None);
        }

        if let Some(neighbour) =
            section_neighbour(&sm, &file.content, class_block, mv.old_short_name)
        {
            let destination = mv.new_ns.map_or_else(
                || "the global namespace".to_string(),
                |ns| format!("`{ns}`"),
            );
            return Err(format!(
                "Cannot move `{}` to `{}`: {} also declares {neighbour} beside it, and the move \
                 would put {neighbour} in {destination} too.",
                mv.old_fqn,
                mv.new_fqn,
                display_uri(file_uri_str)
            ));
        }

        let siblings = self.sibling_imports_for_move(&sm, file, mv.old_ns);

        let declaration = sm.spans.iter().find_map(|s| match &s.kind {
            SymbolKind::NamespaceDeclaration { name }
                if class_block.start <= s.start && s.end <= class_block.end =>
            {
                Some((s, name))
            }
            _ => None,
        });
        let Some((ns_span, ns_name)) = declaration else {
            if let Some(keyword_end) = global_block_keyword_end(&file.content, class_start as usize)
            {
                return Ok(Some(match mv.new_ns {
                    Some(ns) => {
                        let at = offset_to_position(&file.content, keyword_end);
                        let mut edits = vec![TextEdit {
                            range: Range { start: at, end: at },
                            new_text: format!(" {ns}"),
                        }];
                        edits.extend(build_sibling_import_edits(file, &siblings));
                        edits
                    }
                    None => Vec::new(),
                }));
            }
            return Ok(Some(match mv.new_ns {
                Some(ns) => vec![insert_namespace_edit(&file.content, ns, &siblings)],
                None => Vec::new(),
            }));
        };
        if file
            .content
            .get(ns_span.start as usize..ns_span.end as usize)
            != Some(ns_name.as_str())
        {
            return Ok(None);
        }

        match mv.new_ns {
            Some(ns) => {
                let start = offset_to_position(&file.content, ns_span.start as usize);
                let end = offset_to_position(&file.content, ns_span.end as usize);
                let mut edits = vec![TextEdit {
                    range: Range { start, end },
                    new_text: ns.to_string(),
                }];
                edits.extend(build_sibling_import_edits(file, &siblings));
                Ok(Some(edits))
            }
            // The destination has no namespace to write in place of the
            // old one, so the whole statement goes rather than being
            // left as `namespace ;`.
            None => remove_namespace_edits(
                mv.old_fqn,
                file_uri_str,
                file,
                ns_span.start as usize,
                ns_span.end as usize,
                &siblings,
            ),
        }
    }
}

/// The names a class move is planned against, normalised once for every
/// file the move touches.
struct ClassMove<'a> {
    old_fqn: &'a str,
    new_fqn: &'a str,
    old_short_name: &'a str,
    new_short_name: &'a str,
    old_ns: Option<&'a str>,
    new_ns: Option<&'a str>,
    namespace_changed: bool,
    /// The file declaring the class, whose `namespace` statement moves
    /// with it.
    definition_uri: Option<String>,
}

/// One file's part of a class move.
enum FileMoveEdits {
    /// The file could not be read; the move goes on without it.
    Unreadable,
    /// The file's symbol map no longer describes it, so the move is
    /// abandoned rather than planned against stale offsets.
    Stale,
    /// The edits, filed under the URI the editor applies them to.
    Edits(Uri, Vec<TextEdit>),
}
