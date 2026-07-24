//! Class rename and move edits.
//!
//! Handles `textDocument/rename` when the target is a class: updating
//! `use` imports (with alias and collision handling), moving the class to
//! a new namespace, and emitting `RenameFile` operations so the file
//! follows its PSR-4 location. Also holds the shared import-analysis
//! helpers used to rewrite `use` statement lines.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolKind;
use crate::text_position::{line_start_byte_offset, offset_to_position, ranges_overlap};
use crate::util::{build_fqn, strip_fqn_prefix};

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
                    name.clone()
                } else {
                    ctx.resolve_name_at(name, offset)
                };
                Some(strip_fqn_prefix(&fqn).to_string())
            }
            SymbolKind::ClassDeclaration { name } => {
                let ctx = self.file_context(uri);
                Some(build_fqn(name, ctx.namespace.as_deref()))
            }
            _ => None,
        }
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

        // Find the definition file URI from the fqn_uri_index.
        let def_uri_str = self.fqn_uri_index.read().get(old_fqn).cloned()?;

        let def_url = Url::parse(&def_uri_str).ok()?;
        let def_path = def_url.to_file_path().ok()?;

        // Check that the filename matches the old class name.
        let stem = def_path.file_stem()?.to_str()?;
        if stem != old_short {
            return None;
        }

        // Check that the file contains exactly one class-like declaration.
        let classes = self.get_classes_for_uri(&def_uri_str)?;
        if classes.len() != 1 {
            return None;
        }

        // Build the new file path: same directory, new name + .php.
        let mut new_path = def_path.clone();
        new_path.set_file_name(format!("{}.php", new_short_name));

        let new_url = Url::from_file_path(&new_path).ok()?;

        Some((def_url, new_url))
    }

    /// Convert a `changes` map into `document_changes` with a file rename.
    ///
    /// When the rename response needs to include a `RenameFile` operation,
    /// the `WorkspaceEdit` must use `document_changes` (an array of
    /// `DocumentChangeOperation`) instead of the simpler `changes` map,
    /// because the `changes` map does not support file operations.
    ///
    /// Text edits targeting the old file URI are rewritten to target the
    /// new URI so editors apply them after the rename.
    fn convert_to_document_changes(
        changes: HashMap<Url, Vec<TextEdit>>,
        old_uri: &Url,
        new_uri: &Url,
    ) -> DocumentChanges {
        let mut ops: Vec<DocumentChangeOperation> = Vec::new();

        // Add the file rename operation first.
        ops.push(DocumentChangeOperation::Op(ResourceOp::Rename(
            RenameFile {
                old_uri: old_uri.clone(),
                new_uri: new_uri.clone(),
                options: None,
                annotation_id: None,
            },
        )));

        // Convert each file's text edits into a TextDocumentEdit.
        for (uri, edits) in changes {
            // Edits that target the old file URI need to reference the
            // new URI instead, because the rename happens first.
            let target_uri = if uri == *old_uri {
                new_uri.clone()
            } else {
                uri
            };

            let text_doc_edit = TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: target_uri,
                    version: None,
                },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            };

            ops.push(DocumentChangeOperation::Edit(text_doc_edit));
        }

        DocumentChanges::Operations(ops)
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

        // Build the new FQN by replacing the last segment of the old FQN.
        let new_fqn = if let Some(ns_sep) = old_fqn_normalized.rfind('\\') {
            format!("{}\\{}", &old_fqn_normalized[..ns_sep], new_short_name)
        } else {
            new_short_name.to_string()
        };

        // Group locations by file URI for per-file processing.
        let mut locations_by_file: HashMap<String, Vec<&Location>> = HashMap::new();
        for loc in locations {
            locations_by_file
                .entry(loc.uri.to_string())
                .or_default()
                .push(loc);
        }

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        for (file_uri_str, file_locations) in &locations_by_file {
            let file_content = self.get_file_content(file_uri_str);
            let file_content = match file_content {
                Some(c) => c,
                None => continue,
            };

            // Get the file's use_map to understand import context.
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

            // Find the alias (if any) that imports the old FQN.
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
                Some(info) if info.alias != old_short_name => {
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

            // When the file has an import for the old class, find the
            // use-statement line range so we can (a) skip the FQN
            // reference that falls inside it (we replace the whole line
            // instead) and (b) generate a proper whole-line edit that
            // can add/remove aliases.
            let use_line_range = if import_info.is_some() {
                find_use_line_range(&file_content, old_fqn_normalized)
            } else {
                None
            };

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

                // If this reference falls inside the use-statement line,
                // skip it — the whole-line edit below will handle it.
                if let Some(ref ul) = use_line_range
                    && ranges_overlap(&loc.range, &ul.range)
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
                } else if skip_alias_refs && source_text == import_info.as_ref().unwrap().alias {
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

            // Generate a whole-line replacement for the `use` statement.
            if let Some(ref info) = import_info
                && let Some(ref ul) = use_line_range
            {
                let new_line =
                    build_use_line(&new_fqn, info, has_collision, new_short_name, &file_use_map);
                file_edits.push(TextEdit {
                    range: ul.range,
                    new_text: new_line,
                });
            }

            if !file_edits.is_empty() {
                changes.entry(parsed_uri).or_default().extend(file_edits);
            }
        }

        if changes.is_empty() {
            return None;
        }

        // Check whether the file should be renamed alongside the class.
        if let Some((old_file_uri, new_file_uri)) =
            self.should_rename_file(old_fqn_normalized, new_short_name)
        {
            let doc_changes =
                Self::convert_to_document_changes(changes, &old_file_uri, &new_file_uri);
            return Some(WorkspaceEdit {
                changes: None,
                document_changes: Some(doc_changes),
                change_annotations: None,
            });
        }

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
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
    ) -> Option<WorkspaceEdit> {
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
            return None;
        }

        let mut locations_by_file: HashMap<String, Vec<&Location>> = HashMap::new();
        for loc in locations {
            locations_by_file
                .entry(loc.uri.to_string())
                .or_default()
                .push(loc);
        }

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        let def_uri_str = self.fqn_uri_index.read().get(old_fqn_normalized).cloned();

        for (file_uri_str, file_locations) in &locations_by_file {
            let file_content = match self.get_file_content(file_uri_str) {
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

            let (skip_alias_refs, in_code_replacement) = match &import_info {
                Some(info) if info.alias != old_short_name => (true, info.alias.clone()),
                Some(_) if has_collision => {
                    let alias = pick_collision_alias(new_short_name, &file_use_map);
                    (false, alias)
                }
                _ if class_name_changed => (false, new_short_name.to_string()),
                _ => (true, old_short_name.to_string()),
            };

            let use_line_range = if import_info.is_some() {
                find_use_line_range(&file_content, old_fqn_normalized)
            } else {
                None
            };

            let mut file_edits: Vec<TextEdit> = Vec::new();

            let is_definition_file = def_uri_str.as_ref() == Some(file_uri_str);

            if is_definition_file
                && namespace_changed
                && let Some(sm) = self.symbol_maps.read().get(file_uri_str).cloned()
            {
                if let Some(ns_span) = sm
                    .spans
                    .iter()
                    .find(|s| matches!(&s.kind, SymbolKind::NamespaceDeclaration { .. }))
                {
                    let start = offset_to_position(&file_content, ns_span.start as usize);
                    let end = offset_to_position(&file_content, ns_span.end as usize);
                    file_edits.push(TextEdit {
                        range: Range { start, end },
                        new_text: new_ns.unwrap_or("").to_string(),
                    });
                } else if let Some(ns) = new_ns {
                    let insert_line = find_namespace_insert_line(&file_content);
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
                        new_text: format!("namespace {};\n\n", ns),
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

                if let Some(ref ul) = use_line_range
                    && ranges_overlap(&loc.range, &ul.range)
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
                        .is_some_and(|info| source_text == info.alias)
                {
                    continue;
                } else if class_name_changed {
                    file_edits.push(TextEdit {
                        range: loc.range,
                        new_text: in_code_replacement.clone(),
                    });
                }
            }

            if let Some(ref info) = import_info
                && let Some(ref ul) = use_line_range
            {
                let new_line = build_use_line(
                    &new_fqn_normalized,
                    info,
                    has_collision,
                    new_short_name,
                    &file_use_map,
                );
                file_edits.push(TextEdit {
                    range: ul.range,
                    new_text: new_line,
                });
            }

            if !file_edits.is_empty() {
                changes.entry(parsed_uri).or_default().extend(file_edits);
            }
        }

        if changes.is_empty() {
            return None;
        }

        let file_move = self.compute_class_file_move(old_fqn_normalized, &new_fqn_normalized);

        if let Some((old_uri, new_uri)) = file_move
            && self.supports_file_rename.load(Ordering::Acquire)
        {
            let doc_changes = Self::convert_to_document_changes(changes, &old_uri, &new_uri);
            return Some(WorkspaceEdit {
                changes: None,
                document_changes: Some(doc_changes),
                change_annotations: None,
            });
        }

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }

    /// Compute the file move for a class being moved to a new FQN.
    ///
    /// Returns `Some((old_uri, new_uri))` when the file can be moved
    /// to match the new PSR-4 location.
    fn compute_class_file_move(&self, old_fqn: &str, new_fqn: &str) -> Option<(Url, Url)> {
        if !self.supports_file_rename.load(Ordering::Acquire) {
            return None;
        }

        let def_uri_str = self.fqn_uri_index.read().get(old_fqn).cloned()?;
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

        Some((old_url, new_url))
    }
}

// ─── Import analysis helpers ────────────────────────────────────────────────

/// The line range of a `use` statement in a file.
struct UseLineRange {
    range: Range,
}

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
/// Tries `"{name}Alias"` first, then `"{name}Alias2"`, etc.
fn pick_collision_alias(base_name: &str, use_map: &HashMap<String, String>) -> String {
    let candidate = format!("{}Alias", base_name);
    if !use_map.contains_key(&candidate) {
        return candidate;
    }
    for i in 2..100 {
        let candidate = format!("{}Alias{}", base_name, i);
        if !use_map.contains_key(&candidate) {
            return candidate;
        }
    }
    // Extremely unlikely fallback.
    format!("{}Alias99", base_name)
}

/// Find the LSP range of the `use` statement line that imports `old_fqn`.
fn find_use_line_range(content: &str, old_fqn: &str) -> Option<UseLineRange> {
    let old_fqn_normalized = strip_fqn_prefix(old_fqn);

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("use ") {
            continue;
        }

        let rest = trimmed.strip_prefix("use ")?.trim();
        let rest = rest.strip_suffix(';').unwrap_or(rest).trim();

        let (fqn_part, _) = if let Some(as_pos) = rest.find(" as ") {
            (rest[..as_pos].trim(), Some(&rest[as_pos + 4..]))
        } else {
            (rest, None)
        };

        if !fqn_part.eq_ignore_ascii_case(old_fqn_normalized) {
            continue;
        }

        let line_start_byte = line_start_byte_offset(content, line_idx);
        let line_end_byte = line_start_byte + line.len();

        let start_pos = offset_to_position(content, line_start_byte);
        let end_pos = offset_to_position(content, line_end_byte);

        return Some(UseLineRange {
            range: Range {
                start: start_pos,
                end: end_pos,
            },
        });
    }

    None
}

/// Build the replacement text for a `use` statement line.
fn build_use_line(
    new_fqn: &str,
    import_info: &ImportInfo,
    has_collision: bool,
    new_short_name: &str,
    use_map: &HashMap<String, String>,
) -> String {
    if has_collision {
        let alias = pick_collision_alias(new_short_name, use_map);
        format!("use {} as {};", new_fqn, alias)
    } else if import_info.has_explicit_alias {
        format!("use {} as {};", new_fqn, import_info.alias)
    } else {
        format!("use {};", new_fqn)
    }
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
