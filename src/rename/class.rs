//! Class rename and move edits.
//!
//! Handles `textDocument/rename` when the target is a class: updating
//! `use` imports (with alias and collision handling), moving the class to
//! a new namespace, and emitting `RenameFile` operations so the file
//! follows its PSR-4 location. Also holds the shared import-analysis
//! helpers used to rewrite `use` statement lines.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::{document_changes_edit, multi_file_edit};
use crate::completion::use_edit::UseBlockInfo;
use crate::symbol_map::{ClassRefContext, SymbolKind};
use crate::text_position::{offset_to_position, ranges_overlap};
use crate::util::{build_fqn, strip_fqn_prefix};

use super::RenameOutcome;

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

        let mut locations_by_file: HashMap<String, Vec<&Location>> = HashMap::new();
        for loc in locations {
            locations_by_file
                .entry(loc.uri.to_string())
                .or_default()
                .push(loc);
        }

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        for (file_uri_str, file_locations) in &locations_by_file {
            // Reference locations in a template are recorded against the
            // virtual PHP it lowers to, so the text behind them has to be
            // read there too; the edits are translated back below.
            let file_content = match self.reference_file_content(file_uri_str) {
                Some(c) => c,
                None => continue,
            };

            let file_use_map = self
                .file_imports
                .read()
                .get(file_uri_str)
                .cloned()
                .unwrap_or_default();

            let parsed_uri = match Url::parse(file_uri_str) {
                Ok(u) => u,
                Err(e) => {
                    tracing::warn!(
                        "rename: dropping edits for file with unparseable URI {file_uri_str:?}: {e}"
                    );
                    continue;
                }
            };

            let import_info = find_import_for_fqn(&file_use_map, old_fqn_normalized);

            // Determine whether the new short name would collide with
            // an existing import in this file.
            let has_collision = import_info.is_some()
                && new_short_name != old_short_name
                && has_import_collision(&file_use_map, old_fqn_normalized, new_short_name);

            // Decide what in-code references should be renamed to.
            // - If the import uses an explicit alias different from the old short
            //   name, in-code refs use the alias and should NOT change.
            // - If there's a collision, we introduce an alias and in-code refs
            //   must use that alias.
            // - Otherwise, in-code refs switch from old short name to new short name.
            let (skip_alias_refs, in_code_replacement) = match &import_info {
                Some(info) if info.has_explicit_alias => {
                    // Explicit alias: in-code refs use the alias, leave them alone.
                    (true, info.alias.clone())
                }
                Some(_) if has_collision => {
                    // Collision: introduce an alias for the renamed import.
                    let alias = pick_collision_alias(new_short_name, &file_use_map);
                    (false, alias)
                }
                _ => {
                    // Normal case: rename in-code refs to the new short name.
                    (false, new_short_name.to_string())
                }
            };

            // When the file has an import for the old class, plan its
            // update so we can (a) skip the reference location that falls
            // inside it (the plan's own edit(s) handle it instead) and (b)
            // append those edits below.
            let use_statement_edit = import_info.as_ref().and_then(|info| {
                build_use_statement_edit(
                    &file_content,
                    old_fqn_normalized,
                    &RenameTarget {
                        new_fqn: &new_fqn,
                        new_short_name,
                        has_collision,
                    },
                    info,
                    &file_use_map,
                    // A pure rename never changes the namespace, so the
                    // group prefix (if any) always still fits.
                    None,
                )
            });

            let mut file_edits: Vec<TextEdit> = Vec::new();

            for loc in file_locations {
                let start_off =
                    crate::text_position::position_to_byte_offset(&file_content, loc.range.start);
                let end_off =
                    crate::text_position::position_to_byte_offset(&file_content, loc.range.end);
                let source_text = file_content
                    .get(start_off..end_off)
                    .unwrap_or("")
                    .to_string();

                // If this reference falls inside the use-statement's
                // coverage, skip it — the edit(s) above handle it instead.
                if let Some((skip_range, _)) = &use_statement_edit
                    && ranges_overlap(&loc.range, skip_range)
                {
                    continue;
                }

                // self, static, and parent are keywords that should not
                // be renamed when the class they resolve to is renamed.
                if matches!(source_text.as_str(), "self" | "static" | "parent") {
                    continue;
                }

                if source_text.contains('\\') {
                    // This is an inline FQN reference (e.g. `\Ns\Foo`).
                    // Replace only the last segment.
                    let new_text = if let Some(ns_sep) = source_text.rfind('\\') {
                        format!("{}{}", &source_text[..=ns_sep], new_short_name)
                    } else {
                        new_short_name.to_string()
                    };
                    file_edits.push(TextEdit {
                        range: loc.range,
                        new_text,
                    });
                } else if skip_alias_refs
                    && import_info
                        .as_ref()
                        .is_some_and(|info| source_text.eq_ignore_ascii_case(&info.alias))
                {
                    // This reference uses the alias.  The alias is being
                    // preserved, so skip this edit entirely.
                    continue;
                } else {
                    // Normal in-code reference (short name or declaration).
                    file_edits.push(TextEdit {
                        range: loc.range,
                        new_text: in_code_replacement.clone(),
                    });
                }
            }

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
                changes.entry(parsed_uri).or_default().extend(file_edits);
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

        let mut locations_by_file: HashMap<String, Vec<&Location>> = HashMap::new();
        for loc in locations {
            locations_by_file
                .entry(loc.uri.to_string())
                .or_default()
                .push(loc);
        }

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        let def_uri_str = self
            .symbols
            .fqn_uri_index
            .read()
            .get(old_fqn_normalized)
            .cloned();

        for (file_uri_str, file_locations) in &locations_by_file {
            // Reference locations in a template are recorded against the
            // virtual PHP it lowers to, so the text behind them has to be
            // read there too; the edits are translated back below.
            let file_content = match self.reference_file_content(file_uri_str) {
                Some(c) => c,
                None => continue,
            };

            let parsed_uri = match Url::parse(file_uri_str) {
                Ok(u) => u,
                Err(e) => {
                    tracing::warn!(
                        "rename: dropping edits for file with unparseable URI {file_uri_str:?}: {e}"
                    );
                    continue;
                }
            };

            let file_use_map = self
                .file_imports
                .read()
                .get(file_uri_str)
                .cloned()
                .unwrap_or_default();

            let import_info = find_import_for_fqn(&file_use_map, old_fqn_normalized);

            let has_collision = class_name_changed
                && import_info.is_some()
                && has_import_collision(&file_use_map, old_fqn_normalized, new_short_name);

            let is_definition_file = def_uri_str.as_ref() == Some(file_uri_str);

            let file_namespace = self.first_file_namespace(file_uri_str);

            // A file with no import for the class reached it through its
            // own namespace, so moving the class out of that namespace
            // leaves every short-name reference dangling.  Such a file
            // needs a `use` statement added.
            let needs_new_import = namespace_changed
                && import_info.is_none()
                && !is_definition_file
                && namespace_owns(file_namespace.as_deref(), old_fqn_normalized)
                && !namespace_owns(file_namespace.as_deref(), &new_fqn_normalized);

            // The short name may already be taken in this file by an
            // unrelated import, in which case the added import has to be
            // aliased and the references rewritten to that alias.
            let new_import_alias = if needs_new_import
                && has_import_collision(&file_use_map, old_fqn_normalized, new_short_name)
            {
                Some(pick_collision_alias(new_short_name, &file_use_map))
            } else {
                None
            };

            let (skip_alias_refs, in_code_replacement) = match &import_info {
                Some(info) if info.has_explicit_alias => (true, info.alias.clone()),
                Some(_) if has_collision => {
                    let alias = pick_collision_alias(new_short_name, &file_use_map);
                    (false, alias)
                }
                _ => match &new_import_alias {
                    Some(alias) => (false, alias.clone()),
                    None if class_name_changed => (false, new_short_name.to_string()),
                    None => (true, old_short_name.to_string()),
                },
            };

            let rewrite_short_refs = class_name_changed || new_import_alias.is_some();

            let use_statement_edit = import_info.as_ref().and_then(|info| {
                build_use_statement_edit(
                    &file_content,
                    old_fqn_normalized,
                    &RenameTarget {
                        new_fqn: &new_fqn_normalized,
                        new_short_name,
                        has_collision,
                    },
                    info,
                    &file_use_map,
                    file_namespace.as_deref(),
                )
            });

            let mut file_edits: Vec<TextEdit> = Vec::new();
            let mut has_short_name_ref = false;

            if is_definition_file
                && namespace_changed
                && let Some(sm) = self.symbol_maps.read().get(file_uri_str).cloned()
            {
                // This is the one edit in this function built straight from
                // symbol-map offsets rather than from a verified reference
                // location, so it needs the same guard: the map must
                // describe the file, and the span must still spell the
                // namespace it claims to.
                if !sm.matches_source(&file_content) {
                    return Ok(None);
                }

                let siblings =
                    self.sibling_imports_for_move(&sm, &file_content, &file_use_map, old_ns);

                if let Some((ns_span, ns_name)) = sm.spans.iter().find_map(|s| match &s.kind {
                    SymbolKind::NamespaceDeclaration { name } => Some((s, name)),
                    _ => None,
                }) {
                    if file_content.get(ns_span.start as usize..ns_span.end as usize)
                        != Some(ns_name.as_str())
                    {
                        return Ok(None);
                    }
                    match new_ns {
                        Some(ns) => {
                            let start = offset_to_position(&file_content, ns_span.start as usize);
                            let end = offset_to_position(&file_content, ns_span.end as usize);
                            file_edits.push(TextEdit {
                                range: Range { start, end },
                                new_text: ns.to_string(),
                            });
                            file_edits.extend(build_sibling_import_edits(&file_content, &siblings));
                        }
                        // The destination has no namespace to write in
                        // place of the old one, so the whole statement
                        // goes rather than being left as `namespace ;`.
                        None => match namespace_statement(
                            &file_content,
                            ns_span.start as usize,
                            ns_span.end as usize,
                        ) {
                            NamespaceStatement::Statement {
                                range,
                                absorbed_blank_line,
                            } => {
                                let use_block =
                                    crate::completion::use_edit::analyze_use_block(&file_content);
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
                                        start: offset_to_position(&file_content, range.start),
                                        end: offset_to_position(&file_content, range.end),
                                    },
                                    new_text,
                                });
                                if !inline_siblings {
                                    file_edits.extend(build_sibling_import_edits(
                                        &file_content,
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
                    let insert_line = find_namespace_insert_line(&file_content);
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

            for loc in file_locations {
                let start_off =
                    crate::text_position::position_to_byte_offset(&file_content, loc.range.start);
                let end_off =
                    crate::text_position::position_to_byte_offset(&file_content, loc.range.end);
                let source_text = file_content
                    .get(start_off..end_off)
                    .unwrap_or("")
                    .to_string();

                if let Some((skip_range, _)) = &use_statement_edit
                    && ranges_overlap(&loc.range, skip_range)
                {
                    continue;
                }

                if matches!(source_text.as_str(), "self" | "static" | "parent") {
                    continue;
                }

                if source_text.contains('\\') {
                    let new_text = if source_text.starts_with('\\') {
                        format!("\\{}", new_fqn_normalized)
                    } else {
                        new_fqn_normalized.clone()
                    };
                    file_edits.push(TextEdit {
                        range: loc.range,
                        new_text,
                    });
                } else if skip_alias_refs
                    && import_info
                        .as_ref()
                        .is_some_and(|info| source_text.eq_ignore_ascii_case(&info.alias))
                {
                    continue;
                } else {
                    has_short_name_ref = true;
                    if rewrite_short_refs {
                        file_edits.push(TextEdit {
                            range: loc.range,
                            new_text: in_code_replacement.clone(),
                        });
                    }
                }
            }

            if let Some((_, edits)) = use_statement_edit {
                file_edits.extend(edits);
            }

            // Only worth importing when the file actually spells the
            // class by its short name; a file that only ever writes the
            // FQN had its references rewritten in full above.
            if needs_new_import && has_short_name_ref {
                let use_block = crate::completion::use_edit::analyze_use_block(&file_content);
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
                changes.entry(parsed_uri).or_default().extend(file_edits);
            }
        }

        if changes.is_empty() {
            return Ok(None);
        }

        let file_move = self.compute_class_file_move(old_fqn_normalized, &new_fqn_normalized);
        Ok(Some(Self::class_rename_workspace_edit(changes, file_move)))
    }

    /// The imports the moved file needs for the names it used to reach
    /// through its own namespace.
    ///
    /// An unqualified class, function, or constant name resolves against
    /// the namespace the file declares, so a file leaving a populated
    /// namespace silently loses every sibling it named that way: the
    /// reference still reads the same, it just points at a name the
    /// destination namespace has never heard of.  Each one becomes an
    /// explicit import of the name it used to reach.
    ///
    /// Returns the imports sorted into the order a `use` block puts them
    /// in: classes, then constants, then functions, alphabetical within
    /// each group.
    fn sibling_imports_for_move(
        &self,
        symbol_map: &crate::symbol_map::SymbolMap,
        content: &str,
        use_map: &HashMap<String, String>,
        old_ns: Option<&str>,
    ) -> Vec<SiblingImport> {
        // Only the first `namespace` declaration is rewritten, so in a
        // file that declares several there is no single old namespace to
        // resolve the unqualified names against.
        if symbol_map
            .spans
            .iter()
            .filter(|span| matches!(span.kind, SymbolKind::NamespaceDeclaration { .. }))
            .count()
            > 1
        {
            return Vec::new();
        }

        // Whatever the file declares itself travels with it.  Classes,
        // functions, and constants are three separate PHP namespaces, so
        // a file declaring `function helper()` says nothing about the
        // class `Helper` it names.
        let mut declared_classes: HashSet<String> = HashSet::new();
        let mut declared_functions: HashSet<String> = HashSet::new();
        let mut declared_constants: HashSet<&str> = HashSet::new();
        for span in &symbol_map.spans {
            match &span.kind {
                SymbolKind::ClassDeclaration { name } => {
                    declared_classes.insert(name.to_lowercase());
                }
                SymbolKind::FunctionCall {
                    name,
                    is_definition: true,
                    ..
                } => {
                    declared_functions.insert(name.to_lowercase());
                }
                SymbolKind::ConstantReference {
                    name,
                    is_definition: true,
                } => {
                    declared_constants.insert(name.as_str());
                }
                _ => {}
            }
        }

        let mut imports: Vec<SiblingImport> = Vec::new();
        let mut seen: HashSet<(SiblingKind, String)> = HashSet::new();

        for span in &symbol_map.spans {
            let (kind, name) = match &span.kind {
                // A prose-tolerant `@see` target is not worth an import:
                // it may name anything, and an unresolvable one is not an
                // error.
                SymbolKind::ClassReference {
                    name,
                    is_fqn: false,
                    context,
                } if !matches!(
                    context,
                    ClassRefContext::UseImport | ClassRefContext::DocblockSee
                ) =>
                {
                    (SiblingKind::Class, name.as_str())
                }
                SymbolKind::FunctionCall {
                    name,
                    is_definition: false,
                    is_docblock_reference: false,
                } => (SiblingKind::Function, name.as_str()),
                SymbolKind::ConstantReference {
                    name,
                    is_definition: false,
                } => (SiblingKind::Constant, name.as_str()),
                _ => continue,
            };

            if name.is_empty() || name.contains('\\') {
                continue;
            }
            // A leading `\` names the global namespace outright, and the
            // function and constant spans drop it from the stored name,
            // so the source text is what tells the two apart.
            if content
                .get(span.start as usize..span.end as usize)
                .is_some_and(|text| text.starts_with('\\'))
            {
                continue;
            }
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "self" | "static" | "parent"
            ) {
                continue;
            }
            let declared = match kind {
                SiblingKind::Class => declared_classes.contains(&name.to_lowercase()),
                SiblingKind::Function => declared_functions.contains(&name.to_lowercase()),
                SiblingKind::Constant => declared_constants.contains(name),
            };
            if declared {
                continue;
            }
            // An imported name never resolved through the namespace in
            // the first place.  Constant imports are case-sensitive;
            // class and function imports are not.
            let imported = match kind {
                SiblingKind::Constant => use_map.contains_key(name),
                _ => use_map.keys().any(|alias| alias.eq_ignore_ascii_case(name)),
            };
            if imported {
                continue;
            }

            let fqn = match old_ns {
                Some(ns) => format!("{ns}\\{name}"),
                None => name.to_string(),
            };
            // An unqualified function or constant falls back to the
            // global namespace, so one that was global stays reachable.
            if !fqn.contains('\\') && kind != SiblingKind::Class {
                continue;
            }
            if !self.sibling_symbol_exists(kind, &fqn) {
                continue;
            }
            let dedupe_key = match kind {
                SiblingKind::Constant => fqn.clone(),
                _ => fqn.to_lowercase(),
            };
            if !seen.insert((kind, dedupe_key)) {
                continue;
            }
            imports.push(SiblingImport {
                sort_key: kind.sort_key(&fqn),
                statement: kind.statement(&fqn),
            });
        }

        imports.sort_by(|a, b| {
            UseBlockInfo::key_group(&a.sort_key)
                .cmp(&UseBlockInfo::key_group(&b.sort_key))
                .then_with(|| a.sort_key.cmp(&b.sort_key))
        });
        imports
    }

    /// Whether the old namespace really holds the sibling an unqualified
    /// reference named.
    fn sibling_symbol_exists(&self, kind: SiblingKind, fqn: &str) -> bool {
        match kind {
            SiblingKind::Class => self.symbols.fqn_uri_index.read().contains_key(fqn),
            SiblingKind::Function => {
                self.symbols.global_functions.read().contains_key(fqn)
                    || self
                        .symbols
                        .autoload_function_index
                        .read()
                        .contains_key(fqn)
            }
            SiblingKind::Constant => {
                self.symbols.global_defines.read().contains_key(fqn)
                    || self
                        .symbols
                        .autoload_constant_index
                        .read()
                        .contains_key(fqn)
            }
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

// ─── Sibling imports for a class leaving its namespace ──────────────────────

/// Which flavour of `use` statement a sibling reference needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SiblingKind {
    Class,
    Constant,
    Function,
}

impl SiblingKind {
    /// The key [`UseBlockInfo`] sorts this import by.
    fn sort_key(self, fqn: &str) -> String {
        match self {
            Self::Class => fqn.to_lowercase(),
            Self::Constant => format!("const {}", fqn.to_lowercase()),
            Self::Function => format!("function {}", fqn.to_lowercase()),
        }
    }

    fn statement(self, fqn: &str) -> String {
        match self {
            Self::Class => format!("use {};", fqn),
            Self::Constant => format!("use const {};", fqn),
            Self::Function => format!("use function {};", fqn),
        }
    }
}

/// One `use` statement the moved file needs.
struct SiblingImport {
    sort_key: String,
    statement: String,
}

/// Place `imports` in the file's existing `use` block.
///
/// The positions are all computed against the unmodified block, so
/// imports that would land on the same line are merged into a single
/// edit: two zero-width edits sharing an offset land in whichever order
/// the client applies them.
fn build_sibling_import_edits(content: &str, imports: &[SiblingImport]) -> Vec<TextEdit> {
    if imports.is_empty() {
        return Vec::new();
    }

    let use_block = crate::completion::use_edit::analyze_use_block(content);

    let mut by_line: BTreeMap<u32, Vec<&SiblingImport>> = BTreeMap::new();
    for import in imports {
        let line = use_block.insert_position_for_key(&import.sort_key).line;
        by_line.entry(line).or_default().push(import);
    }

    // Which of the three import groups the file already writes, so the
    // blank line that separates a group is only added when this move
    // opens the group.
    let mut group_present = [false; 3];
    for (_, key) in &use_block.existing {
        group_present[UseBlockInfo::key_group(key) as usize] = true;
    }

    let mut first = true;
    let mut edits = Vec::with_capacity(by_line.len());
    for (line, group) in by_line {
        let mut new_text = String::new();
        for import in group {
            let idx = UseBlockInfo::key_group(&import.sort_key) as usize;
            let opens_use_block = first && use_block.existing.is_empty() && use_block.has_namespace;
            let opens_group =
                !group_present[idx] && group_present[..idx].iter().any(|present| *present);
            if opens_use_block || opens_group {
                new_text.push('\n');
            }
            new_text.push_str(&import.statement);
            new_text.push('\n');
            group_present[idx] = true;
            first = false;
        }
        let position = Position { line, character: 0 };
        edits.push(TextEdit {
            range: Range {
                start: position,
                end: position,
            },
            new_text,
        });
    }

    edits
}

// ─── Import analysis helpers ────────────────────────────────────────────────

/// Information about how a class is imported in a file.
struct ImportInfo {
    /// The alias (short name) used in code.  For `use Ns\Foo;` this is
    /// `"Foo"`.  For `use Ns\Foo as Bar;` this is `"Bar"`.
    alias: String,
    /// Whether an explicit `as` alias was used.
    has_explicit_alias: bool,
}

/// Look up the import entry for a given FQN in a file's use_map.
///
/// The use_map is `alias → fqn`, so we need a reverse lookup.
fn find_import_for_fqn(use_map: &HashMap<String, String>, target_fqn: &str) -> Option<ImportInfo> {
    let target_normalized = strip_fqn_prefix(target_fqn);
    let target_short = crate::util::short_name(target_normalized);

    for (alias, fqn) in use_map {
        let fqn_normalized = strip_fqn_prefix(fqn);
        if fqn_normalized.eq_ignore_ascii_case(target_normalized) {
            let has_explicit_alias = !alias.eq_ignore_ascii_case(target_short);
            return Some(ImportInfo {
                alias: alias.clone(),
                has_explicit_alias,
            });
        }
    }
    None
}

/// Whether a file declaring `file_namespace` resolves the short name of
/// `fqn` to `fqn` without needing a `use` import.
///
/// PHP falls back to the current namespace for unqualified class names,
/// so `namespace App\Support;` reaches `App\Support\Helper` as plain
/// `Helper`.  Namespace names are case-insensitive.
fn namespace_owns(file_namespace: Option<&str>, fqn: &str) -> bool {
    let class_namespace = fqn.rfind('\\').map(|i| &fqn[..i]);
    match (file_namespace, class_namespace) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => true,
        _ => false,
    }
}

/// Check whether importing `new_short_name` would collide with an
/// existing import in the file (other than the one being renamed).
fn has_import_collision(
    use_map: &HashMap<String, String>,
    old_fqn: &str,
    new_short_name: &str,
) -> bool {
    let old_normalized = strip_fqn_prefix(old_fqn);
    let new_lower = new_short_name.to_lowercase();

    for (alias, fqn) in use_map {
        let fqn_normalized = strip_fqn_prefix(fqn);
        // Skip the entry for the class being renamed.
        if fqn_normalized.eq_ignore_ascii_case(old_normalized) {
            continue;
        }
        if alias.to_lowercase() == new_lower {
            return true;
        }
    }
    false
}

/// Pick an alias name to avoid a collision.
///
/// Tries `"{name}Alias"` first, then `"{name}Alias2"`, etc.  An alias is
/// a class name, so a candidate that differs from an existing import only
/// in casing still collides.
fn pick_collision_alias(base_name: &str, use_map: &HashMap<String, String>) -> String {
    let is_free = |candidate: &str| {
        !use_map
            .keys()
            .any(|alias| alias.eq_ignore_ascii_case(candidate))
    };

    let candidate = format!("{}Alias", base_name);
    if is_free(&candidate) {
        return candidate;
    }
    for i in 2..100 {
        let candidate = format!("{}Alias{}", base_name, i);
        if is_free(&candidate) {
            return candidate;
        }
    }
    // Extremely unlikely fallback.
    format!("{}Alias99", base_name)
}

/// What a class is being renamed/moved to, bundled so
/// [`build_use_statement_edit`] stays under clippy's argument-count limit.
struct RenameTarget<'a> {
    new_fqn: &'a str,
    new_short_name: &'a str,
    /// Whether the new short name collides with another import already in
    /// the file, so the rewritten import needs an alias.
    has_collision: bool,
}

/// Plan the edit(s) needed to bring a file's `use` import for a renamed or
/// moved class up to date, together with the byte range those edits
/// already cover (so a per-reference edit can skip a location that falls
/// inside it instead of double-covering it).
///
/// Locates the import through the same statement scanner the unused-import
/// fixer uses ([`find_use_statement`] in `diagnostics/helpers`), so a group
/// import (`use App\Models\{User, Post};`) and a brace-less multi-import
/// list wrapped over several lines are both found, not just a whole
/// trimmed `use ... ;` line.
///
/// Two shapes are handled:
/// - A statement with only one item left after the rename (an ordinary
///   `use Old\Fqn [as Alias];`, or a group/list with a single member): one
///   `TextEdit` replacing the whole statement.
/// - One member of a multi-item group or list whose relative name still
///   fits under the shared prefix (empty for a brace-less list): a
///   `TextEdit` touching only that member. When it no longer fits (only
///   possible for a class *move*, which can change the namespace), the
///   member is dropped from the group instead and re-added as its own
///   `use` statement.
fn build_use_statement_edit(
    content: &str,
    old_fqn: &str,
    target: &RenameTarget,
    info: &ImportInfo,
    use_map: &HashMap<String, String>,
    file_namespace: Option<&str>,
) -> Option<(Range, Vec<TextEdit>)> {
    let &RenameTarget {
        new_fqn,
        new_short_name,
        has_collision,
    } = target;

    // The import may spell the class in different casing than its
    // canonical declared FQN (PHP class names are case-insensitive), so
    // the statement is located by the exact text the file wrote rather
    // than by `old_fqn`.
    let source_fqn = use_map
        .get(&info.alias)
        .map(String::as_str)
        .unwrap_or(old_fqn);

    let use_statement_spans = crate::diagnostics::helpers::compute_use_statement_spans(content);
    let location = crate::diagnostics::helpers::find_use_statement(
        content,
        &use_statement_spans,
        source_fqn,
        &info.alias,
    )?;

    let whole_statement_edit = |range: Range| {
        let new_line = format!(
            "use {};",
            build_member_text(new_fqn, info, has_collision, new_short_name, use_map)
        );
        (
            range,
            vec![TextEdit {
                range,
                new_text: new_line,
            }],
        )
    };

    // A one-member group (or an ordinary single-class import) has nothing
    // left to keep, so it is rewritten as a plain statement the way a lone
    // `use Foo\Bar;` would be.
    if location.member_count <= 1 {
        let range = Range {
            start: offset_to_position(content, location.statement.0),
            end: offset_to_position(content, location.statement.1),
        };
        return Some(whole_statement_edit(range));
    }

    let member_range = Range {
        start: offset_to_position(content, location.member.0),
        end: offset_to_position(content, location.member.1),
    };

    if let Some(relative) = strip_matching_prefix(new_fqn, location.prefix) {
        let new_text = build_member_text(relative, info, has_collision, new_short_name, use_map);
        return Some((
            member_range,
            vec![TextEdit {
                range: member_range,
                new_text,
            }],
        ));
    }

    // The new namespace no longer fits the group's shared prefix (only
    // reachable from a class move): drop the member from the group and
    // re-add it as its own `use` statement.
    let (del_start, del_end) =
        list_item_delete_range(content, location.member.0, location.member.1);
    let delete_range = Range {
        start: offset_to_position(content, del_start),
        end: offset_to_position(content, del_end),
    };
    let mut edits = vec![TextEdit {
        range: delete_range,
        new_text: String::new(),
    }];

    let alias_for_new = if has_collision {
        Some(pick_collision_alias(new_short_name, use_map))
    } else if info.has_explicit_alias {
        Some(info.alias.clone())
    } else {
        None
    };
    let use_block = crate::completion::use_edit::analyze_use_block(content);
    let file_namespace_owned = file_namespace.map(str::to_string);
    if let Some(import_edits) = crate::completion::use_edit::build_aliased_use_edit(
        new_fqn,
        alias_for_new.as_deref(),
        &use_block,
        &file_namespace_owned,
    ) {
        edits.extend(import_edits);
    }

    let skip_range = Range {
        start: offset_to_position(content, location.statement.0),
        end: offset_to_position(content, location.statement.1),
    };
    Some((skip_range, edits))
}

/// Build the text for one `use` import target: either a whole statement's
/// FQN or one group member's relative name, with the `as` clause a
/// collision or an explicit alias requires.
fn build_member_text(
    name: &str,
    import_info: &ImportInfo,
    has_collision: bool,
    new_short_name: &str,
    use_map: &HashMap<String, String>,
) -> String {
    if has_collision {
        let alias = pick_collision_alias(new_short_name, use_map);
        format!("{} as {}", name, alias)
    } else if import_info.has_explicit_alias {
        format!("{} as {}", name, import_info.alias)
    } else {
        name.to_string()
    }
}

/// Strip a group import's shared prefix from `fqn`, comparing
/// case-insensitively (namespaces are case-insensitive in PHP).
///
/// Returns `None` when `fqn` does not fall under `prefix`, in which case
/// the class can no longer be named as a member of that group.
fn strip_matching_prefix<'a>(fqn: &'a str, prefix: &str) -> Option<&'a str> {
    // A brace-less list item (`use Foo\Bar, Baz\Qux;`) has no shared
    // prefix and already spells its own full name, with no separator to
    // consume.
    if prefix.is_empty() {
        return Some(fqn);
    }
    if !fqn.is_char_boundary(prefix.len()) {
        return None;
    }
    let (head, rest) = fqn.split_at(prefix.len());
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    rest.strip_prefix('\\')
}

/// The byte range to delete from `content` to remove a comma-separated
/// list item spanning `item_start..item_end` (a group import member),
/// taking the trailing comma when present or the leading one otherwise,
/// along with the item's own indentation, so what remains reads as a valid
/// list with no blank line left where the item used to be.
fn list_item_delete_range(content: &str, item_start: usize, item_end: usize) -> (usize, usize) {
    let bytes = content.as_bytes();

    let mut start = item_start;
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }

    let mut after = item_end;
    while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
        after += 1;
    }
    if bytes.get(after) == Some(&b',') {
        after += 1;
        while bytes.get(after).is_some_and(|b| matches!(b, b' ' | b'\t')) {
            after += 1;
        }
        if bytes.get(after) == Some(&b'\n') {
            after += 1;
        }
        return (start, after);
    }

    // No following comma: this was the last member, so the preceding one
    // goes instead.
    let mut before = start;
    while before > 0 && bytes[before - 1].is_ascii_whitespace() {
        before -= 1;
    }
    if before > 0 && bytes[before - 1] == b',' {
        before -= 1;
    }
    (before, item_end)
}

/// Compute the PSR-4 file path for a given namespace + class name.
fn compute_psr4_path(
    mappings: &[crate::composer::Psr4Mapping],
    workspace_root: &Path,
    namespace: Option<&str>,
    class_name: &str,
) -> Option<PathBuf> {
    let fqn = match namespace {
        Some(ns) => format!("{}\\{}", ns, class_name),
        None => class_name.to_string(),
    };

    for mapping in mappings {
        let relative = if mapping.prefix.is_empty() {
            Some(fqn.as_str())
        } else {
            fqn.strip_prefix(&mapping.prefix)
        };

        if let Some(relative_class) = relative {
            let relative_path = relative_class.replace('\\', "/");
            let file_path = workspace_root
                .join(&mapping.base_path)
                .join(format!("{}.php", relative_path));
            return Some(file_path);
        }
    }

    None
}

/// Find the line number after `<?php` (and any `declare` statements)
/// where a `namespace` declaration should be inserted.
fn find_namespace_insert_line(content: &str) -> u32 {
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("<?php") {
            return (i + 1) as u32;
        }
        if trimmed.starts_with("declare(") || trimmed.starts_with("declare (") {
            continue;
        }
        if !trimmed.is_empty()
            && !trimmed.starts_with("//")
            && !trimmed.starts_with("/*")
            && !trimmed.starts_with("*")
            && !trimmed.starts_with("<?")
        {
            return i as u32;
        }
    }
    1
}

/// What the source around a `namespace` name turns out to be, once the
/// move needs to take the whole declaration away rather than rewrite
/// the name in place.
enum NamespaceStatement {
    /// A `namespace Foo;` statement occupying this byte range.
    Statement {
        range: std::ops::Range<usize>,
        /// Whether the range swallowed the blank line that followed the
        /// declaration, so anything written in its place has to supply
        /// that separation itself.
        absorbed_blank_line: bool,
    },
    /// `namespace Foo { … }`.  Removing the declaration means unwrapping
    /// the block it opens, which the move does not do.
    Block,
    /// Neither shape: the source does not read the way the symbol map
    /// says it does.
    Unrecognized,
}

/// The `namespace` statement whose name occupies `name_start..name_end`.
///
/// The span the symbol map records covers the name alone, which is all
/// a rename needs.  Removing the declaration takes the keyword before it
/// and the `;` after it as well, plus the line they sit on so the file
/// is not left with a stray blank.
fn namespace_statement(content: &str, name_start: usize, name_end: usize) -> NamespaceStatement {
    const KEYWORD: &str = "namespace";
    let bytes = content.as_bytes();

    let mut keyword_end = name_start;
    while keyword_end > 0 && bytes[keyword_end - 1].is_ascii_whitespace() {
        keyword_end -= 1;
    }
    let Some(keyword_start) = keyword_end.checked_sub(KEYWORD.len()) else {
        return NamespaceStatement::Unrecognized;
    };
    if !content.is_char_boundary(keyword_start)
        || !content[keyword_start..keyword_end].eq_ignore_ascii_case(KEYWORD)
    {
        return NamespaceStatement::Unrecognized;
    }

    let mut end = name_end;
    while bytes.get(end).is_some_and(u8::is_ascii_whitespace) {
        end += 1;
    }
    match bytes.get(end) {
        Some(b';') => end += 1,
        Some(b'{') => return NamespaceStatement::Block,
        _ => return NamespaceStatement::Unrecognized,
    }

    let line_start = content[..keyword_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let mut start = keyword_start;
    while start > line_start && bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }

    let mut absorbed_blank_line = false;
    if start == line_start {
        let line_end = skip_blanks(bytes, end);
        if bytes.get(line_end) == Some(&b'\n') {
            end = line_end + 1;
            // Removing the line would otherwise leave the blank above
            // the declaration and the blank below it stacked.
            let next_line_end = skip_blanks(bytes, end);
            if ends_with_blank_line(&content[..start]) && bytes.get(next_line_end) == Some(&b'\n') {
                end = next_line_end + 1;
                absorbed_blank_line = true;
            }
        }
    }

    NamespaceStatement::Statement {
        range: start..end,
        absorbed_blank_line,
    }
}

/// Whether `text` ends on a line that holds nothing, so appending to it
/// would leave a blank line above.
fn ends_with_blank_line(text: &str) -> bool {
    let text = text.strip_suffix('\n').unwrap_or(text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    text.ends_with('\n')
}

/// The offset of the first byte at or after `from` that is not
/// horizontal whitespace.
fn skip_blanks(bytes: &[u8], from: usize) -> usize {
    let mut cursor = from;
    while bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
    {
        cursor += 1;
    }
    cursor
}
