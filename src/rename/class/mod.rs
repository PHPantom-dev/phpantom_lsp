//! Class rename and move edits.
//!
//! Handles `textDocument/rename` when the target is a class: updating
//! `use` imports (with alias and collision handling), moving the class to
//! a new namespace, and emitting `RenameFile` operations so the file
//! follows its PSR-4 location. The import-analysis helpers live in
//! `imports`, the per-file reference rewriting in `rewrite`, the
//! sibling-import planning in `siblings`, the PSR-4 and
//! namespace-statement layout helpers in `layout`, and the class move
//! itself in `move_edits`.

mod imports;
mod layout;
mod move_edits;
mod rewrite;
mod siblings;

use std::collections::HashMap;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::code_actions::{document_changes_edit, multi_file_edit};
use crate::symbol_map::SymbolKind;
use crate::util::{build_fqn, strip_fqn_prefix};

use super::RenameOutcome;
use rewrite::locations_by_block;

impl Backend {
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
                Some(build_fqn(name, ctx.namespace_at(offset).as_deref()))
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
        changes: HashMap<Uri, Vec<TextEdit>>,
        file_move: Option<(Uri, Uri)>,
    ) -> WorkspaceEdit {
        let Some((old_uri, new_uri)) = file_move else {
            return multi_file_edit(changes);
        };
        let rename = DocumentChange::RenameFile(RenameFile {
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

        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();

        for (file_uri_str, file_locations) in &locations_by_file {
            let blocks = self.file_rewrites(file_uri_str, old_fqn_normalized);
            let Some(target_uri) = blocks.first().map(|file| file.target_uri.clone()) else {
                continue;
            };
            let mut file_edits: Vec<TextEdit> = Vec::new();
            for (file, block_locations) in locations_by_block(&blocks, file_locations) {
                // A rename adds no import, so there is no alias for one either.
                let rewrite = file.reference_rewrite(
                    old_fqn_normalized,
                    old_short_name,
                    new_short_name,
                    None,
                );
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
                let (location_edits, _) = file.rewrite_locations(
                    &block_locations,
                    use_statement_edit.as_ref(),
                    &rewrite,
                    |source| match source.rfind('\\') {
                        Some(ns_sep) => format!("{}{}", &source[..=ns_sep], new_short_name),
                        None => new_short_name.to_string(),
                    },
                );
                file_edits.extend(location_edits);

                if let Some((_, edits)) = use_statement_edit {
                    file_edits.extend(edits);
                }
            }

            self.rewrite_template_edits(
                file_uri_str,
                &new_fqn,
                old_fqn_normalized,
                &mut file_edits,
            );

            if !file_edits.is_empty() {
                changes.entry(target_uri).or_default().extend(file_edits);
            }
        }

        self.collect_framework_class_edits(old_fqn_normalized, &new_fqn, &mut changes);

        if changes.is_empty() {
            return None;
        }

        let file_move = self.should_rename_file(old_fqn_normalized, new_short_name);
        Some(Self::class_rename_workspace_edit(changes, file_move))
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

/// The offset just past the `namespace` keyword of the braced global
/// block (`namespace {`) that encloses `offset`, if that is what the
/// class sits in.
fn global_block_keyword_end(content: &str, offset: usize) -> Option<usize> {
    const KEYWORD: &str = "namespace";
    let head = content.get(..offset)?.to_ascii_lowercase();
    head.rmatch_indices(KEYWORD).find_map(|(start, _)| {
        let end = start + KEYWORD.len();
        head[end..].trim_start().starts_with('{').then_some(end)
    })
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
