//! Class rename and move edits.
//!
//! Handles `textDocument/rename` when the target is a class: updating
//! `use` imports (with alias and collision handling), moving the class to
//! a new namespace, and emitting `RenameFile` operations so the file
//! follows its PSR-4 location. The import-analysis helpers live in
//! `imports`, the per-file reference rewriting in `rewrite`, the
//! sibling-import planning in `siblings`, and the PSR-4 and
//! namespace-statement layout helpers in `layout`.

mod imports;
mod layout;
mod rewrite;
mod siblings;

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::{document_changes_edit, multi_file_edit};
use crate::symbol_map::SymbolKind;
use crate::text_position::offset_to_position;
use crate::util::{build_fqn, strip_fqn_prefix};

use super::RenameOutcome;
use imports::{has_import_collision, namespace_owns, pick_collision_alias};
use layout::{insert_namespace_edit, remove_namespace_edits};
use rewrite::FileRewrite;
use siblings::build_sibling_import_edits;

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
            .symbol_maps
            .read()
            .get(&definition_uri)
            .cloned()
            .ok_or_else(|| format!("Could not index the definition of `{old_fqn}`."))?;
        let span = symbol_map
            .spans
            .iter()
            .find(|span| {
                matches!(
                    &span.kind,
                    SymbolKind::ClassDeclaration { name }
                        if name.eq_ignore_ascii_case(crate::util::short_name(old_fqn))
                )
            })
            .ok_or_else(|| format!("Could not locate the declaration of `{old_fqn}`."))?;
        let position = offset_to_position(&content, span.start as usize);
        let locations = self
            .find_references_for_rename(&definition_uri, &content, position, true)
            .ok_or_else(|| format!("Could not find references to `{old_fqn}`."))?;
        if !self.rename_locations_verified(&span.kind, &locations) {
            return Err(format!(
                "The workspace changed while `{old_fqn}` was being indexed; retry the move."
            ));
        }
        self.build_class_move_edit(old_fqn, new_fqn, &locations)
    }

    /// Resolve the fully-qualified class name for a class rename.
    ///
    /// Returns `Some(fqn)` when the symbol being renamed is a class
    /// reference or class declaration, `None` otherwise.
    pub(super) fn resolve_class_rename_fqn(
        &self,
        kind: &SymbolKind,
        uri: &str,
        offset: u32,
    ) -> Option<String> {
        match kind {
            SymbolKind::ClassReference { name, is_fqn, .. } => {
                let ctx = self.file_context(uri);
                let fqn = if *is_fqn {
                    name.to_string()
                } else {
                    ctx.resolve_name_at(name, offset)
                };
                Some(self.canonical_class_fqn(strip_fqn_prefix(&fqn)))
            }
            SymbolKind::ClassDeclaration { name } => {
                let ctx = self.file_context(uri);
                Some(build_fqn(name, ctx.namespace.as_deref()))
            }
            _ => None,
        }
    }

    /// The spelling the class declares itself with.
    ///
    /// A reference may name a class in any casing (`new WIDGET()` reaches
    /// `App\Widget`), but every later step of the rename reads the old
    /// short name back out of this FQN: to decide whether an import is
    /// aliased, whether the new name collides, and whether the file is
    /// named after the class.  Answering those against the reference's
    /// casing rather than the declaration's gets all three wrong, so the
    /// name is canonicalized once here.
    fn canonical_class_fqn(&self, fqn: &str) -> String {
        self.symbols
            .fqn_uri_index
            .read()
            .get_key_value(fqn)
            .map(|(declared, _)| declared.to_string())
            .unwrap_or_else(|| fqn.to_string())
    }

    /// The edit for a class rename: plain per-file changes, or, when the
    /// declaring file moves with the class, the same changes carried as
    /// document changes alongside the file rename.
    fn class_rename_workspace_edit(
        changes: HashMap<Url, Vec<TextEdit>>,
        file_move: Option<(Url, Url)>,
    ) -> WorkspaceEdit {
        let Some((old_uri, new_uri)) = file_move else {
            return multi_file_edit(changes);
        };
        let rename = ResourceOp::Rename(RenameFile {
            old_uri: old_uri.clone(),
            new_uri: new_uri.clone(),
            options: None,
            annotation_id: None,
        });
        // Edits that target the old file URI need to reference the new
        // URI instead, because the rename happens first.
        let edits = changes.into_iter().map(|(uri, edits)| {
            let target_uri = if uri == old_uri { new_uri.clone() } else { uri };
            (target_uri, edits)
        });
        document_changes_edit([rename], edits)
    }

    /// Build a `WorkspaceEdit` for a class rename that correctly handles
    /// `use` import statements, aliases, and import collisions.
    ///
    /// When renaming class `OldName` to `NewName`:
    ///
    /// - **`use Ns\OldName;`** becomes `use Ns\NewName;` and in-code
    ///   references `OldName` become `NewName`.
    /// - **`use Ns\OldName as Alias;`** becomes `use Ns\NewName as Alias;`
    ///   and in-code references `Alias` are left unchanged.
    /// - **Collision**: if the file already imports a different class with
    ///   the same short name as `NewName`, the renamed import gets an
    ///   alias (`use Ns\NewName as NewNameAlias;`) and in-code references
    ///   are updated to use that alias.
    pub(super) fn build_class_rename_edit(
        &self,
        old_fqn: &str,
        new_short_name: &str,
        locations: &[Location],
    ) -> Option<WorkspaceEdit> {
        let old_fqn_normalized = strip_fqn_prefix(old_fqn);
        let old_short_name = crate::util::short_name(old_fqn_normalized);

        let new_fqn = if let Some(ns_sep) = old_fqn_normalized.rfind('\\') {
            format!("{}\\{}", &old_fqn_normalized[..ns_sep], new_short_name)
        } else {
            new_short_name.to_string()
        };

        let locations_by_file = group_locations_by_file(locations);

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        for (file_uri_str, file_locations) in &locations_by_file {
            let Some(file) = self.file_rewrite(file_uri_str, old_fqn_normalized) else {
                continue;
            };
            // A rename adds no import, so there is no alias for one either.
            let rewrite =
                file.reference_rewrite(old_fqn_normalized, old_short_name, new_short_name, None);
            // A pure rename never changes the namespace, so the group
            // prefix (if any) always still fits.
            let use_statement_edit = file.use_statement_edit(
                old_fqn_normalized,
                &new_fqn,
                new_short_name,
                &rewrite,
                None,
            );

            // An inline qualified reference (`\Ns\Foo`, `Sub\Foo`) keeps
            // its prefix; only the last segment changes.
            let (mut file_edits, _) = file.rewrite_locations(
                file_locations,
                use_statement_edit.as_ref(),
                &rewrite,
                |source| match source.rfind('\\') {
                    Some(ns_sep) => format!("{}{}", &source[..=ns_sep], new_short_name),
                    None => new_short_name.to_string(),
                },
            );

            if let Some((_, edits)) = use_statement_edit {
                file_edits.extend(edits);
            }

            self.rewrite_template_edits(
                file_uri_str,
                &new_fqn,
                old_fqn_normalized,
                &mut file_edits,
            );

            if !file_edits.is_empty() {
                changes
                    .entry(file.target_uri)
                    .or_default()
                    .extend(file_edits);
            }
        }

        if changes.is_empty() {
            return None;
        }

        let file_move = self.should_rename_file(old_fqn_normalized, new_short_name);
        Some(Self::class_rename_workspace_edit(changes, file_move))
    }

    /// Build a `WorkspaceEdit` that moves a class to a new FQN.
    ///
    /// Handles namespace change, class name change, file move, and
    /// updates all references across the workspace.  This is the
    /// handler for rename requests where `new_name` contains `\`.
    pub(super) fn build_class_move_edit(
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

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
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
        let Some(file) = self.file_rewrite(file_uri_str, mv.old_fqn) else {
            return Ok(FileMoveEdits::Unreadable);
        };

        let is_definition_file = mv.definition_uri.as_deref() == Some(file_uri_str);

        let file_namespace = self.first_file_namespace(file_uri_str);

        // A file with no import for the class reached it through its
        // own namespace, so moving the class out of that namespace
        // leaves every short-name reference dangling.  Such a file
        // needs a `use` statement added.
        let needs_new_import = mv.namespace_changed
            && file.import_info.is_none()
            && !is_definition_file
            && namespace_owns(file_namespace.as_deref(), mv.old_fqn)
            && !namespace_owns(file_namespace.as_deref(), mv.new_fqn);

        // The short name may already be taken in this file by an
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

        let mut file_edits: Vec<TextEdit> = Vec::new();

        if is_definition_file && mv.namespace_changed {
            match self.namespace_declaration_edits(mv, &file, file_uri_str)? {
                Some(edits) => file_edits.extend(edits),
                None => return Ok(FileMoveEdits::Stale),
            }
        }

        // A qualified reference is rewritten in full, keeping its
        // leading backslash when it has one.
        let (location_edits, has_short_name_ref) = file.rewrite_locations(
            file_locations,
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
        file_edits.extend(location_edits);

        if let Some((_, edits)) = use_statement_edit {
            file_edits.extend(edits);
        }

        // Only worth importing when the file actually spells the
        // class by its short name; a file that only ever writes the
        // FQN had its references rewritten in full above.
        if needs_new_import && has_short_name_ref {
            let use_block = crate::completion::use_edit::analyze_use_block(&file.content);
            if let Some(import_edits) = crate::completion::use_edit::build_aliased_use_edit(
                mv.new_fqn,
                new_import_alias.as_deref(),
                &use_block,
                &file_namespace,
            ) {
                file_edits.extend(import_edits);
            }
        }

        self.rewrite_template_edits(file_uri_str, mv.new_fqn, mv.old_fqn, &mut file_edits);

        Ok(FileMoveEdits::Edits(file.target_uri, file_edits))
    }

    /// The edits that bring the declaring file's `namespace` statement to
    /// the destination namespace, together with the imports its former
    /// namespace-siblings then need.
    ///
    /// This is the one edit of a move built straight from symbol-map
    /// offsets rather than from a verified reference location, so it
    /// carries the same guard: `Ok(None)` abandons the move when the map
    /// does not describe the file or the span no longer spells the
    /// namespace it claims to.
    fn namespace_declaration_edits(
        &self,
        mv: &ClassMove<'_>,
        file: &FileRewrite,
        file_uri_str: &str,
    ) -> Result<Option<Vec<TextEdit>>, String> {
        let Some(sm) = self.symbol_maps.read().get(file_uri_str).cloned() else {
            return Ok(Some(Vec::new()));
        };
        if !sm.matches_source(&file.content) {
            return Ok(None);
        }

        let siblings = self.sibling_imports_for_move(&sm, &file.content, &file.use_map, mv.old_ns);

        let declaration = sm.spans.iter().find_map(|s| match &s.kind {
            SymbolKind::NamespaceDeclaration { name } => Some((s, name)),
            _ => None,
        });
        let Some((ns_span, ns_name)) = declaration else {
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
                edits.extend(build_sibling_import_edits(&file.content, &siblings));
                Ok(Some(edits))
            }
            // The destination has no namespace to write in place of the
            // old one, so the whole statement goes rather than being
            // left as `namespace ;`.
            None => remove_namespace_edits(
                mv.old_fqn,
                file_uri_str,
                &file.content,
                ns_span.start as usize,
                ns_span.end as usize,
                &siblings,
            ),
        }
    }

    /// Bring a template's edits into the template's own coordinates and
    /// add the one edit its symbol map cannot describe.
    ///
    /// A no-op for every file that is not a template.  The edits collected
    /// so far were planned against the virtual PHP the preprocessor lowers
    /// the template to; a `@use` directive is hoisted into that file's
    /// prologue, so the import it declares is rewritten from the
    /// template's own text instead of from a reference location.
    fn rewrite_template_edits(
        &self,
        uri: &str,
        new_fqn: &str,
        old_fqn: &str,
        edits: &mut Vec<TextEdit>,
    ) {
        if !self.is_blade_file(uri) {
            return;
        }
        self.translate_template_edits(uri, edits);
        let Some(template) = self.get_file_content(uri) else {
            return;
        };
        super::blade::collect_use_directive_edits(
            &template,
            &|name| {
                name.eq_ignore_ascii_case(old_fqn)
                    .then(|| new_fqn.to_string())
            },
            edits,
        );
    }
}

/// Group reference locations by the file they fall in, in the shape
/// [`build_class_rename_edit`] and [`build_class_move_edit`] both walk
/// one file at a time in.
fn group_locations_by_file(locations: &[Location]) -> HashMap<String, Vec<&Location>> {
    let mut by_file: HashMap<String, Vec<&Location>> = HashMap::new();
    for loc in locations {
        by_file.entry(loc.uri.to_string()).or_default().push(loc);
    }
    by_file
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
    Edits(Url, Vec<TextEdit>),
}
