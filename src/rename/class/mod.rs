//! Class rename and move edits.
//!
//! Handles `textDocument/rename` when the target is a class: updating
//! `use` imports (with alias and collision handling), moving the class to
//! a new namespace, and emitting `RenameFile` operations so the file
//! follows its PSR-4 location. The import-analysis helpers live in
//! `imports`, the sibling-import planning in `siblings`, and the PSR-4
//! and namespace-statement layout helpers in `layout`.

mod imports;
mod layout;
mod siblings;

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::{document_changes_edit, multi_file_edit};
use crate::symbol_map::SymbolKind;
use crate::text_position::{offset_to_position, position_to_byte_offset, ranges_overlap};
use crate::util::{build_fqn, strip_fqn_prefix};

use super::{RenameOutcome, parse_edit_target_uri};
use imports::{
    ImportInfo, RenameTarget, build_use_statement_edit, find_import_for_fqn, has_import_collision,
    namespace_owns, pick_collision_alias,
};
use layout::{NamespaceStatement, compute_psr4_path, namespace_statement};
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

    /// Check whether renaming a class should also rename the file.
    ///
    /// Returns the old and new file URIs as `(old_uri, new_uri)` when:
    /// 1. The client supports file rename operations.
    /// 2. The definition file's basename (without `.php`) matches the
    ///    old class short name.
    /// 3. The file contains exactly one class/interface/trait/enum
    ///    declaration.
    fn should_rename_file(&self, old_fqn: &str, new_short_name: &str) -> Option<(Url, Url)> {
        if !self.supports_file_rename.load(Ordering::Acquire) {
            return None;
        }

        let old_short = crate::util::short_name(old_fqn);

        let def_uri_str = self.symbols.fqn_uri_index.read().get(old_fqn).cloned()?;

        let def_url = Url::parse(&def_uri_str).ok()?;
        let def_path = def_url.to_file_path().ok()?;

        let stem = def_path.file_stem()?.to_str()?;
        if stem != old_short {
            return None;
        }

        let classes = self.get_classes_for_uri(&def_uri_str)?;
        if classes.len() != 1 {
            return None;
        }

        let mut new_path = def_path.clone();
        new_path.set_file_name(format!("{}.php", new_short_name));

        let new_url = Url::from_file_path(&new_path).ok()?;

        Some((def_url, new_url))
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
        let old_fqn_normalized = strip_fqn_prefix(old_fqn);
        let new_fqn_normalized = strip_fqn_prefix(new_fqn_raw).to_string();
        let old_short_name = crate::util::short_name(old_fqn_normalized);
        let new_short_name = crate::util::short_name(&new_fqn_normalized);

        let old_ns = old_fqn_normalized
            .rfind('\\')
            .map(|i| &old_fqn_normalized[..i]);
        let new_ns = new_fqn_normalized
            .rfind('\\')
            .map(|i| &new_fqn_normalized[..i]);

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
        if let Some(occupant) = self.class_move_conflict(old_fqn_normalized, &new_fqn_normalized) {
            return Err(occupant);
        }

        let locations_by_file = group_locations_by_file(locations);

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        let def_uri_str = self
            .symbols
            .fqn_uri_index
            .read()
            .get(old_fqn_normalized)
            .cloned();

        for (file_uri_str, file_locations) in &locations_by_file {
            let Some(file) = self.file_rewrite(file_uri_str, old_fqn_normalized) else {
                continue;
            };

            let is_definition_file = def_uri_str.as_ref() == Some(file_uri_str);

            let file_namespace = self.first_file_namespace(file_uri_str);

            // A file with no import for the class reached it through its
            // own namespace, so moving the class out of that namespace
            // leaves every short-name reference dangling.  Such a file
            // needs a `use` statement added.
            let needs_new_import = namespace_changed
                && file.import_info.is_none()
                && !is_definition_file
                && namespace_owns(file_namespace.as_deref(), old_fqn_normalized)
                && !namespace_owns(file_namespace.as_deref(), &new_fqn_normalized);

            // The short name may already be taken in this file by an
            // unrelated import, in which case the added import has to be
            // aliased and the references rewritten to that alias.
            let new_import_alias = if needs_new_import
                && has_import_collision(&file.use_map, old_fqn_normalized, new_short_name)
            {
                Some(pick_collision_alias(new_short_name, &file.use_map))
            } else {
                None
            };

            let rewrite = file.reference_rewrite(
                old_fqn_normalized,
                old_short_name,
                new_short_name,
                new_import_alias.as_deref(),
            );

            let use_statement_edit = file.use_statement_edit(
                old_fqn_normalized,
                &new_fqn_normalized,
                new_short_name,
                &rewrite,
                file_namespace.as_deref(),
            );

            let mut file_edits: Vec<TextEdit> = Vec::new();

            if is_definition_file
                && namespace_changed
                && let Some(sm) = self.symbol_maps.read().get(file_uri_str).cloned()
            {
                // This is the one edit in this function built straight from
                // symbol-map offsets rather than from a verified reference
                // location, so it needs the same guard: the map must
                // describe the file, and the span must still spell the
                // namespace it claims to.
                if !sm.matches_source(&file.content) {
                    return Ok(None);
                }

                let siblings =
                    self.sibling_imports_for_move(&sm, &file.content, &file.use_map, old_ns);

                if let Some((ns_span, ns_name)) = sm.spans.iter().find_map(|s| match &s.kind {
                    SymbolKind::NamespaceDeclaration { name } => Some((s, name)),
                    _ => None,
                }) {
                    if file
                        .content
                        .get(ns_span.start as usize..ns_span.end as usize)
                        != Some(ns_name.as_str())
                    {
                        return Ok(None);
                    }
                    match new_ns {
                        Some(ns) => {
                            let start = offset_to_position(&file.content, ns_span.start as usize);
                            let end = offset_to_position(&file.content, ns_span.end as usize);
                            file_edits.push(TextEdit {
                                range: Range { start, end },
                                new_text: ns.to_string(),
                            });
                            file_edits.extend(build_sibling_import_edits(&file.content, &siblings));
                        }
                        // The destination has no namespace to write in
                        // place of the old one, so the whole statement
                        // goes rather than being left as `namespace ;`.
                        None => match namespace_statement(
                            &file.content,
                            ns_span.start as usize,
                            ns_span.end as usize,
                        ) {
                            NamespaceStatement::Statement {
                                range,
                                absorbed_blank_line,
                            } => {
                                let use_block =
                                    crate::completion::use_edit::analyze_use_block(&file.content);
                                // With no import block to sort into, a
                                // sibling import lands on the line the
                                // removal takes away.  Writing both as
                                // one edit keeps them off each other.
                                let inline_siblings =
                                    use_block.existing.is_empty() && !siblings.is_empty();
                                let mut new_text = String::new();
                                if inline_siblings {
                                    for import in &siblings {
                                        new_text.push_str(&import.statement);
                                        new_text.push('\n');
                                    }
                                    if absorbed_blank_line {
                                        new_text.push('\n');
                                    }
                                }
                                file_edits.push(TextEdit {
                                    range: Range {
                                        start: offset_to_position(&file.content, range.start),
                                        end: offset_to_position(&file.content, range.end),
                                    },
                                    new_text,
                                });
                                if !inline_siblings {
                                    file_edits.extend(build_sibling_import_edits(
                                        &file.content,
                                        &siblings,
                                    ));
                                }
                            }
                            NamespaceStatement::Block => {
                                return Err(format!(
                                    "Cannot move `{old_fqn_normalized}` into the global \
                                     namespace: {} writes its namespace as a brace block, \
                                     which the move would have to unwrap.",
                                    display_uri(file_uri_str)
                                ));
                            }
                            NamespaceStatement::Unrecognized => return Ok(None),
                        },
                    }
                } else if let Some(ns) = new_ns {
                    // The `namespace` line and the first import would be
                    // inserted at the same offset, and two edits sharing
                    // one offset land in whichever order the client
                    // applies them.  Writing both as one edit fixes the
                    // order.
                    let insert_line = crate::text_scan::header_insert_line(&file.content);
                    let mut new_text = format!("namespace {};\n\n", ns);
                    for import in &siblings {
                        new_text.push_str(&import.statement);
                        new_text.push('\n');
                    }
                    if !siblings.is_empty() {
                        new_text.push('\n');
                    }
                    file_edits.push(TextEdit {
                        range: Range {
                            start: Position {
                                line: insert_line,
                                character: 0,
                            },
                            end: Position {
                                line: insert_line,
                                character: 0,
                            },
                        },
                        new_text,
                    });
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
                        format!("\\{new_fqn_normalized}")
                    } else {
                        new_fqn_normalized.clone()
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
                    &new_fqn_normalized,
                    new_import_alias.as_deref(),
                    &use_block,
                    &file_namespace,
                ) {
                    file_edits.extend(import_edits);
                }
            }

            self.rewrite_template_edits(
                file_uri_str,
                &new_fqn_normalized,
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
            return Ok(None);
        }

        let file_move = self.compute_class_file_move(old_fqn_normalized, &new_fqn_normalized);
        Ok(Some(Self::class_rename_workspace_edit(changes, file_move)))
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

    /// Compute the file move for a class being moved to a new FQN.
    ///
    /// Returns `Some((old_uri, new_uri))` when the file can be moved
    /// to match the new PSR-4 location.
    fn compute_class_file_move(&self, old_fqn: &str, new_fqn: &str) -> Option<(Url, Url)> {
        if !self.supports_file_rename.load(Ordering::Acquire) {
            return None;
        }

        let def_uri_str = self.symbols.fqn_uri_index.read().get(old_fqn).cloned()?;
        let old_url = Url::parse(&def_uri_str).ok()?;

        let workspace_root = self.workspace_root().read().clone()?;
        let mappings = self.psr4_mappings().read().clone();

        let new_short = crate::util::short_name(new_fqn);
        let new_ns = new_fqn.rfind('\\').map(|i| &new_fqn[..i]);

        let new_path = compute_psr4_path(&mappings, &workspace_root, new_ns, new_short)?;
        let new_url = Url::from_file_path(&new_path).ok()?;

        if old_url == new_url {
            return None;
        }

        // A `RenameFile` onto a path that is already there is destructive
        // in every editor that honours it. `build_class_move_edit`
        // refuses the move before reaching this point, so a path that
        // still exists here holds something PSR-4 does not account for.
        if new_path.exists() {
            return None;
        }

        Some((old_url, new_url))
    }

    /// Why a class cannot move to `new_fqn`, or `None` when the
    /// destination is free.
    ///
    /// A class already declared under that name is the blocking case:
    /// the move would leave two declarations claiming it, and every
    /// reference the rename rewrites would then name whichever one the
    /// autoloader reaches first. The PSR-4 destination file is checked
    /// too, since a file can sit there without the index having a class
    /// for it.
    fn class_move_conflict(&self, old_fqn: &str, new_fqn: &str) -> Option<String> {
        if let Some((declared, uri)) = self
            .symbols
            .fqn_uri_index
            .read()
            .get_key_value(new_fqn)
            .map(|(k, v)| (k.to_string(), v.clone()))
            && !declared.eq_ignore_ascii_case(old_fqn)
        {
            return Some(format!(
                "Cannot rename to `{}`: a class with that name is already declared in {}.",
                declared,
                display_uri(&uri)
            ));
        }

        let workspace_root = self.workspace_root().read().clone()?;
        let mappings = self.psr4_mappings().read().clone();
        let new_ns = new_fqn.rfind('\\').map(|i| &new_fqn[..i]);
        let new_path = compute_psr4_path(
            &mappings,
            &workspace_root,
            new_ns,
            crate::util::short_name(new_fqn),
        )?;

        let old_path = self
            .symbols
            .fqn_uri_index
            .read()
            .get(old_fqn)
            .and_then(|u| Url::parse(u).ok())
            .and_then(|u| u.to_file_path().ok());

        if new_path.exists() && old_path.as_deref() != Some(new_path.as_path()) {
            return Some(format!(
                "Cannot rename to `{}`: {} already exists.",
                new_fqn,
                new_path.display()
            ));
        }

        None
    }
}

/// A file URI rendered as a plain path for a user-facing message.
fn display_uri(uri: &str) -> String {
    Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| uri.to_string())
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

/// One file that references the class, with what its own imports say
/// about how it reaches the class.
struct FileRewrite {
    /// The text the reference locations index into: for a template, the
    /// virtual PHP it lowers to.
    content: String,
    /// The URI the file's edits are filed under.
    target_uri: Url,
    /// The file's `alias → FQN` import table.
    use_map: HashMap<String, String>,
    /// The file's import of the class, when it has one.
    import_info: Option<ImportInfo>,
}

/// How one file's in-code references to the class are rewritten.
struct ReferenceRewrite {
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
    fn file_rewrite(&self, uri: &str, old_fqn: &str) -> Option<FileRewrite> {
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
    fn reference_rewrite(
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
    fn use_statement_edit(
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
    fn rewrite_locations(
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
