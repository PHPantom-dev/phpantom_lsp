//! Rename (`textDocument/rename`) and prepare-rename support.
//!
//! When the user triggers a rename on a symbol, the LSP first calls
//! `prepareRename` to validate that the symbol is renameable and to
//! return the range + current name of the symbol.  If the user
//! confirms, `rename` is called with the new name, and we produce a
//! `WorkspaceEdit` that replaces every occurrence across the workspace.
//!
//! The heavy lifting (finding all references) is delegated to the
//! existing `find_references` infrastructure.  This module adds:
//!
//! - Vendor rejection: symbols defined under the vendor directory
//!   cannot be renamed.
//! - Non-renameable symbol rejection: keywords like `self`, `static`,
//!   `parent`, and `$this` cannot be renamed.
//! - Property name fixup: `$this->foo` references need the edit to
//!   replace only `foo`, not the `$` prefix.  Static properties
//!   (`self::$prop`) include the `$` in the source but the rename
//!   should replace the whole `$prop` token consistently.
//! - Use-statement-aware class rename: when renaming a class, the
//!   `use` import FQN is updated (last segment only), aliases are
//!   preserved, and collisions with existing imports are resolved by
//!   introducing an alias.
//! - Namespace rename: when renaming a namespace segment, all
//!   `namespace` declarations, `use` statements, and fully-qualified
//!   references across the workspace are updated.  When a PSR-4
//!   mapping exists, `RenameFile` operations are emitted to move
//!   files so the directory structure stays consistent.

mod tests;

use std::collections::HashMap;

use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolKind;
use crate::util::{
    build_fqn, line_start_byte_offset, offset_to_position, ranges_overlap, strip_fqn_prefix,
};

impl Backend {
    /// Handle `textDocument/prepareRename`.
    ///
    /// Validates that the symbol under the cursor is renameable and
    /// returns its range and current name.  Returns `None` (which the
    /// LSP layer translates to an error) when the symbol cannot be
    /// renamed.
    pub(crate) fn handle_prepare_rename(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<PrepareRenameResponse> {
        let span = self.lookup_symbol_at_position(uri, content, position)?;

        // Reject non-renameable symbols.
        if let SymbolKind::SelfStaticParent(_) = &span.kind {
            // self, static, parent, and $this are never renameable.
            return None;
        }

        // Namespace rename: narrow the range to the segment under the cursor.
        if let SymbolKind::NamespaceDeclaration { ref name } = span.kind {
            let cursor_byte = crate::util::position_to_byte_offset(content, position);
            let (segment, seg_start, seg_end) =
                find_namespace_segment_at_offset(name, span.start, cursor_byte as u32)?;
            let range = Range {
                start: offset_to_position(content, seg_start as usize),
                end: offset_to_position(content, seg_end as usize),
            };
            return Some(PrepareRenameResponse::RangeWithPlaceholder {
                range,
                placeholder: segment.to_string(),
            });
        }

        // Extract the symbol name and validate it's something we can rename.
        let (name, range) =
            self.renameable_symbol_info(uri, content, &span.kind, span.start, span.end)?;

        // Reject vendor symbols: if the definition lives under the
        // vendor directory the user shouldn't rename it.
        if self.is_vendor_symbol(uri, content, position) {
            return None;
        }

        Some(PrepareRenameResponse::RangeWithPlaceholder {
            range,
            placeholder: name,
        })
    }

    /// Handle `textDocument/rename`.
    ///
    /// Produces a `WorkspaceEdit` that renames every occurrence of the
    /// symbol under the cursor to `new_name`.
    pub(crate) fn handle_rename(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        let span = self.lookup_symbol_at_position(uri, content, position)?;

        // Reject non-renameable symbols (same logic as prepare_rename).
        if let SymbolKind::SelfStaticParent(_) = &span.kind {
            // self, static, parent, and $this are never renameable.
            return None;
        }

        // Reject vendor symbols.
        if self.is_vendor_symbol(uri, content, position) {
            return None;
        }

        // Namespace rename: delegate to the specialised handler.
        if let SymbolKind::NamespaceDeclaration { ref name } = span.kind {
            let cursor_byte = crate::util::position_to_byte_offset(content, position);
            let (segment, _seg_start, _seg_end) =
                find_namespace_segment_at_offset(name, span.start, cursor_byte as u32)?;
            let segment_idx = name.split('\\').position(|s| s == segment)?;
            return self.build_namespace_rename_edit(name, segment_idx, new_name);
        }

        // Detect whether this is a class rename and resolve the FQN.
        let class_rename_fqn = self.resolve_class_rename_fqn(&span.kind, uri, span.start);

        // Find all references (including the declaration).
        let locations = self.find_references_for_rename(uri, content, position, true)?;

        if locations.is_empty() {
            return None;
        }

        // Determine whether this is a property rename.  Properties are
        // special because the `$` prefix is part of the declaration but
        // usage sites via `->` or `?->` don't include it.
        let is_property = self.is_property_rename(&span.kind, uri, &span);
        let is_variable = matches!(
            &span.kind,
            SymbolKind::Variable { .. } | SymbolKind::CompactVariable { .. }
        ) && !is_property;

        // For class renames, delegate to the specialised handler that
        // understands `use` statements, aliases, and collisions.
        if let Some(ref fqn) = class_rename_fqn {
            return self.build_class_rename_edit(fqn, new_name, &locations);
        }

        // Build the workspace edit.  Group text edits by document URI.
        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        for location in &locations {
            let loc_uri_str = location.uri.to_string();

            // For each reference location, we need the file content to
            // inspect what text is at that range.
            let loc_content = if loc_uri_str == uri {
                Some(content.to_string())
            } else {
                self.get_file_content(&loc_uri_str)
            };

            let edit_text = if is_variable {
                let bare_name = new_name.strip_prefix('$').unwrap_or(new_name);
                let loc_symbol = loc_content.as_deref().and_then(|c| {
                    self.lookup_symbol_at_position(&loc_uri_str, c, location.range.start)
                });
                match loc_symbol {
                    Some(crate::symbol_map::SymbolSpan {
                        kind: SymbolKind::CompactVariable { .. },
                        ..
                    }) => bare_name.to_string(),
                    _ => {
                        if new_name.starts_with('$') {
                            new_name.to_string()
                        } else {
                            format!("${}", new_name)
                        }
                    }
                }
            } else if is_property {
                // Properties: the reference may or may not include `$`.
                // Check the actual source text at the location to decide.
                let has_dollar = loc_content.as_ref().is_some_and(|c| {
                    let start_off = crate::util::position_to_byte_offset(c, location.range.start);
                    c.as_bytes().get(start_off) == Some(&b'$')
                });
                let bare_name = new_name.strip_prefix('$').unwrap_or(new_name);
                if has_dollar {
                    format!("${}", bare_name)
                } else {
                    bare_name.to_string()
                }
            } else {
                new_name.to_string()
            };

            let text_edit = TextEdit {
                range: location.range,
                new_text: edit_text,
            };

            changes
                .entry(location.uri.clone())
                .or_default()
                .push(text_edit);
        }

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }

    /// Resolve the fully-qualified class name for a class rename.
    ///
    /// Returns `Some(fqn)` when the symbol being renamed is a class
    /// reference or class declaration, `None` otherwise.
    fn resolve_class_rename_fqn(
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
    fn build_class_rename_edit(
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
                Err(_) => continue,
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
                    crate::util::position_to_byte_offset(&file_content, loc.range.start);
                let end_off = crate::util::position_to_byte_offset(&file_content, loc.range.end);
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

    /// Extract the renameable symbol name and its source range.
    ///
    /// Returns `None` for symbols that cannot be renamed.
    fn renameable_symbol_info(
        &self,
        _uri: &str,
        content: &str,
        kind: &SymbolKind,
        start: u32,
        end: u32,
    ) -> Option<(String, Range)> {
        let range = Range {
            start: offset_to_position(content, start as usize),
            end: offset_to_position(content, end as usize),
        };

        match kind {
            SymbolKind::Variable { name } => {
                // Include the `$` prefix in the range — the span already does.
                Some((format!("${}", name), range))
            }
            SymbolKind::CompactVariable { name } => Some((name.clone(), range)),
            SymbolKind::ClassReference { name, .. } => Some((name.clone(), range)),
            SymbolKind::ClassDeclaration { name } => Some((name.clone(), range)),
            SymbolKind::MemberAccess { member_name, .. } => Some((member_name.clone(), range)),
            SymbolKind::MemberDeclaration { name, .. } => Some((name.clone(), range)),
            SymbolKind::FunctionCall { name, .. } => Some((name.clone(), range)),
            SymbolKind::ConstantReference { name } => Some((name.clone(), range)),
            SymbolKind::NamespaceDeclaration { name } => Some((name.clone(), range)),
            SymbolKind::SelfStaticParent { .. } => None,
            SymbolKind::LaravelStringKey { .. }
            | SymbolKind::Keyword
            | SymbolKind::CastType
            | SymbolKind::Comment => None,
        }
    }

    /// Check whether the symbol under the cursor is defined in a vendor
    /// file.
    ///
    /// We check this by resolving the definition location.  If the
    /// definition URI starts with the vendor prefix, the rename is
    /// rejected.
    fn is_vendor_symbol(&self, uri: &str, content: &str, position: Position) -> bool {
        let vendor_prefixes = self.vendor_uri_prefixes.lock().clone();

        if vendor_prefixes.is_empty() {
            return false;
        }

        // Try to resolve the definition location.
        for loc in self.resolve_definition(uri, content, position) {
            let def_uri = loc.uri.to_string();
            if vendor_prefixes
                .iter()
                .any(|p| def_uri.starts_with(p.as_str()))
            {
                return true;
            }
        }

        false
    }

    /// Determine whether this rename targets a property (as opposed to
    /// a local variable or other symbol kind).
    fn is_property_rename(
        &self,
        kind: &SymbolKind,
        uri: &str,
        span: &crate::symbol_map::SymbolSpan,
    ) -> bool {
        match kind {
            SymbolKind::MemberAccess { is_method_call, .. } => !is_method_call,
            SymbolKind::MemberDeclaration { .. } => {
                // A MemberDeclaration is a property if it is NOT a method
                // and NOT a class constant.  We check the uri_classes_index to see
                // whether the offset matches a method or constant name.
                let is_method = self
                    .get_classes_for_uri(uri)
                    .iter()
                    .flat_map(|classes| classes.iter())
                    .flat_map(|c| c.methods.iter())
                    .any(|m| m.name_offset != 0 && m.name_offset == span.start);
                let is_constant = self
                    .get_classes_for_uri(uri)
                    .iter()
                    .flat_map(|classes| classes.iter())
                    .flat_map(|c| c.constants.iter())
                    .any(|con| con.name_offset != 0 && con.name_offset == span.start);
                !is_method && !is_constant
            }
            SymbolKind::Variable { name } => {
                // Variable spans can represent property declarations.
                self.lookup_var_def_kind_at(uri, name, span.start)
                    .is_some_and(|k| k == crate::symbol_map::VarDefKind::Property)
            }
            SymbolKind::CompactVariable { .. } => false,
            _ => false,
        }
    }
    // ─── Namespace rename ───────────────────────────────────────────────

    /// Build a `WorkspaceEdit` for renaming a namespace segment.
    ///
    /// `full_ns` is the full namespace at the declaration site (e.g.
    /// `"App\\Bar\\Service"`).  `segment_idx` is the 0-based index of
    /// the segment being renamed.  `new_segment` is the replacement
    /// text for that segment.
    ///
    /// The method scans every file known to the server to find:
    /// - `namespace` declarations that start with the old prefix
    /// - `use` statements that reference the old prefix
    /// - Inline FQN references (in code and docblocks)
    ///
    /// It also emits `RenameFile` operations when a PSR-4 mapping
    /// exists so that the directory structure stays consistent.
    fn build_namespace_rename_edit(
        &self,
        full_ns: &str,
        segment_idx: usize,
        new_segment: &str,
    ) -> Option<WorkspaceEdit> {
        let segments: Vec<&str> = full_ns.split('\\').collect();
        if segment_idx >= segments.len() {
            return None;
        }

        // Build the old prefix up to and including the renamed segment.
        // For example, if `full_ns` is `App\Bar\Service` and we rename
        // segment 1 (`Bar`), `old_prefix` is `App\Bar`.
        let old_prefix: String = segments[..=segment_idx].join("\\");
        let mut new_segments = segments.clone();
        new_segments[segment_idx] = new_segment;
        let new_prefix: String = new_segments[..=segment_idx].join("\\");

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        // Scan all known files via namespace_map, use_map, and symbol_maps.
        let all_uris: Vec<String> = {
            let nmap = self.file_namespaces.read();
            let umap = self.file_imports.read();
            let smap = self.symbol_maps.read();
            let mut uris: std::collections::HashSet<String> = std::collections::HashSet::new();
            for uri in nmap.keys() {
                uris.insert(uri.clone());
            }
            for uri in umap.keys() {
                uris.insert(uri.clone());
            }
            for uri in smap.keys() {
                uris.insert(uri.clone());
            }
            uris.into_iter().collect()
        };

        // Skip vendor files.
        let vendor_prefixes = self.vendor_uri_prefixes.lock().clone();

        for file_uri in &all_uris {
            if vendor_prefixes
                .iter()
                .any(|p| file_uri.starts_with(p.as_str()))
            {
                continue;
            }

            let content = match self.get_file_content(file_uri) {
                Some(c) => c,
                None => continue,
            };

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let mut file_edits: Vec<TextEdit> = Vec::new();

            // 1. Update `namespace` declarations.
            //    Find lines like `namespace App\Bar\Service;` or
            //    `namespace App\Bar\Service {` where the namespace
            //    starts with `old_prefix`.
            self.collect_namespace_decl_edits(&content, &old_prefix, &new_prefix, &mut file_edits);

            // 2. Update `use` statements.
            self.collect_use_statement_edits(&content, &old_prefix, &new_prefix, &mut file_edits);

            // 3. Update inline FQN references from the symbol map.
            self.collect_fqn_reference_edits(
                file_uri,
                &content,
                &old_prefix,
                &new_prefix,
                &mut file_edits,
            );

            if !file_edits.is_empty() {
                // Sort edits by start position descending so they don't
                // interfere with each other when applied.
                file_edits.sort_by(|a, b| {
                    b.range
                        .start
                        .line
                        .cmp(&a.range.start.line)
                        .then(b.range.start.character.cmp(&a.range.start.character))
                });
                // Deduplicate overlapping edits (keep first = largest line).
                file_edits.dedup_by(|a, b| ranges_overlap(&a.range, &b.range));
                changes.entry(parsed_uri).or_default().extend(file_edits);
            }
        }

        if changes.is_empty() {
            return None;
        }

        // PSR-4 directory rename: if a mapping exists, emit RenameFile
        // operations to move the directory.
        if let Some(ops) = self.build_namespace_psr4_rename_ops(&old_prefix, &new_prefix)
            && !ops.is_empty()
            && self.supports_file_rename.load(Ordering::Acquire)
        {
            let mut doc_ops: Vec<DocumentChangeOperation> = Vec::new();

            // Add directory/file rename operations first.
            for (old_uri, new_uri) in &ops {
                doc_ops.push(DocumentChangeOperation::Op(ResourceOp::Rename(
                    RenameFile {
                        old_uri: old_uri.clone(),
                        new_uri: new_uri.clone(),
                        options: None,
                        annotation_id: None,
                    },
                )));
            }

            // Convert text edits to document changes. Rewrite URIs
            // that fall inside a renamed directory.
            for (uri, edits) in changes {
                let target_uri = ops
                    .iter()
                    .find_map(|(old_u, new_u)| {
                        let old_str = old_u.as_str();
                        let uri_str = uri.as_str();
                        if let Some(rest) = uri_str.strip_prefix(old_str) {
                            Url::parse(&format!("{}{}", new_u.as_str(), rest)).ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(uri);

                let text_doc_edit = TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: target_uri,
                        version: None,
                    },
                    edits: edits.into_iter().map(OneOf::Left).collect(),
                };
                doc_ops.push(DocumentChangeOperation::Edit(text_doc_edit));
            }

            return Some(WorkspaceEdit {
                changes: None,
                document_changes: Some(DocumentChanges::Operations(doc_ops)),
                change_annotations: None,
            });
        }

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }

    /// Collect text edits for `namespace` declaration lines where the
    /// namespace starts with `old_prefix`.
    fn collect_namespace_decl_edits(
        &self,
        content: &str,
        old_prefix: &str,
        new_prefix: &str,
        edits: &mut Vec<TextEdit>,
    ) {
        let old_prefix_lower = old_prefix.to_lowercase();
        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if !trimmed.starts_with("namespace ") {
                continue;
            }
            let rest = trimmed.strip_prefix("namespace ").unwrap().trim();
            // Strip trailing `;` or `{`.
            let ns_name = rest.trim_end_matches(';').trim_end_matches('{').trim();

            if ns_name.is_empty() {
                continue;
            }

            let ns_lower = ns_name.to_lowercase();
            // The namespace must equal old_prefix or start with old_prefix + `\`.
            if ns_lower != old_prefix_lower
                && !ns_lower.starts_with(&format!("{}\\", old_prefix_lower))
            {
                continue;
            }

            // Build the new namespace name by replacing the prefix.
            let new_ns = if ns_name.len() == old_prefix.len() {
                new_prefix.to_string()
            } else {
                format!("{}{}", new_prefix, &ns_name[old_prefix.len()..])
            };

            // Find the byte range of the namespace name within the line.
            let line_start_byte = line_start_byte_offset(content, line_idx);
            let ns_offset_in_line = line.find(ns_name).unwrap_or(0);
            let ns_start = line_start_byte + ns_offset_in_line;
            let ns_end = ns_start + ns_name.len();

            edits.push(TextEdit {
                range: Range {
                    start: offset_to_position(content, ns_start),
                    end: offset_to_position(content, ns_end),
                },
                new_text: new_ns,
            });
        }
    }

    /// Collect text edits for `use` statement lines that reference the
    /// old namespace prefix.
    fn collect_use_statement_edits(
        &self,
        content: &str,
        old_prefix: &str,
        new_prefix: &str,
        edits: &mut Vec<TextEdit>,
    ) {
        let old_prefix_lower = old_prefix.to_lowercase();
        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if !trimmed.starts_with("use ") {
                continue;
            }

            let rest = trimmed.strip_prefix("use ").unwrap().trim();
            // Handle `use function` and `use const` prefixes.
            let rest = rest
                .strip_prefix("function ")
                .or_else(|| rest.strip_prefix("const "))
                .unwrap_or(rest)
                .trim();

            let rest = rest.strip_suffix(';').unwrap_or(rest).trim();

            // Handle group use: `use App\Old\{Foo, Bar};`
            if let Some(brace_pos) = rest.find('{') {
                let group_prefix = rest[..brace_pos].trim_end_matches('\\').trim();
                let group_lower = group_prefix.to_lowercase();

                if group_lower == old_prefix_lower
                    || group_lower.starts_with(&format!("{}\\", old_prefix_lower))
                {
                    let new_group_prefix = if group_prefix.len() == old_prefix.len() {
                        new_prefix.to_string()
                    } else {
                        format!("{}{}", new_prefix, &group_prefix[old_prefix.len()..])
                    };

                    let line_start_byte = line_start_byte_offset(content, line_idx);
                    let prefix_offset_in_line = line.find(group_prefix).unwrap_or(0);
                    let prefix_start = line_start_byte + prefix_offset_in_line;
                    let prefix_end = prefix_start + group_prefix.len();

                    edits.push(TextEdit {
                        range: Range {
                            start: offset_to_position(content, prefix_start),
                            end: offset_to_position(content, prefix_end),
                        },
                        new_text: new_group_prefix,
                    });
                }
                continue;
            }

            // Simple use: `use App\Old\Foo;` or `use App\Old\Foo as Bar;`
            let (fqn_part, _alias_part) = if let Some(as_pos) = rest.find(" as ") {
                (rest[..as_pos].trim(), Some(&rest[as_pos + 4..]))
            } else {
                (rest, None)
            };

            let fqn_lower = fqn_part.to_lowercase();
            if fqn_lower == old_prefix_lower
                || fqn_lower.starts_with(&format!("{}\\", old_prefix_lower))
            {
                let new_fqn = if fqn_part.len() == old_prefix.len() {
                    new_prefix.to_string()
                } else {
                    format!("{}{}", new_prefix, &fqn_part[old_prefix.len()..])
                };

                let line_start_byte = line_start_byte_offset(content, line_idx);
                let fqn_offset_in_line = line.find(fqn_part).unwrap_or(0);
                let fqn_start = line_start_byte + fqn_offset_in_line;
                let fqn_end = fqn_start + fqn_part.len();

                edits.push(TextEdit {
                    range: Range {
                        start: offset_to_position(content, fqn_start),
                        end: offset_to_position(content, fqn_end),
                    },
                    new_text: new_fqn,
                });
            }
        }
    }

    /// Collect text edits for inline FQN references (e.g. `\App\Old\Foo`
    /// in type hints or docblocks) that contain the old prefix.
    fn collect_fqn_reference_edits(
        &self,
        file_uri: &str,
        content: &str,
        old_prefix: &str,
        new_prefix: &str,
        edits: &mut Vec<TextEdit>,
    ) {
        let symbol_map = match self.symbol_maps.read().get(file_uri) {
            Some(sm) => sm.clone(),
            None => return,
        };

        let old_prefix_lower = old_prefix.to_lowercase();

        for span in &symbol_map.spans {
            let name = match &span.kind {
                SymbolKind::ClassReference {
                    name, is_fqn: true, ..
                } => name,
                _ => continue,
            };

            // Only process references that contain a backslash (FQN-style).
            let name_normalized = strip_fqn_prefix(name);
            let name_lower = name_normalized.to_lowercase();

            if name_lower != old_prefix_lower
                && !name_lower.starts_with(&format!("{}\\", old_prefix_lower))
            {
                continue;
            }

            // Check source text to see if this is an inline FQN reference
            // (contains `\` in source).  Use-statement references are
            // handled separately by collect_use_statement_edits.
            let source = content
                .get(span.start as usize..span.end as usize)
                .unwrap_or("");

            // Skip use-statement references (they don't have `\` in span
            // unless they are inline FQN like `\App\Foo` in code).
            // Actually, use-statement spans DO contain the full FQN.
            // We rely on deduplication to handle overlaps.

            let new_name = if name_normalized.len() == old_prefix.len() {
                if name.starts_with('\\') {
                    format!("\\{}", new_prefix)
                } else {
                    new_prefix.to_string()
                }
            } else {
                let suffix = &name_normalized[old_prefix.len()..];
                if name.starts_with('\\') {
                    format!("\\{}{}", new_prefix, suffix)
                } else {
                    format!("{}{}", new_prefix, suffix)
                }
            };

            // Only emit an edit if the text actually changes.
            if source == new_name {
                continue;
            }

            edits.push(TextEdit {
                range: Range {
                    start: offset_to_position(content, span.start as usize),
                    end: offset_to_position(content, span.end as usize),
                },
                new_text: new_name,
            });
        }
    }

    /// Determine PSR-4 directory rename operations for a namespace rename.
    ///
    /// Returns pairs of `(old_uri, new_uri)` for directories that should
    /// be renamed, or `None` if no PSR-4 mapping applies.
    fn build_namespace_psr4_rename_ops(
        &self,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Option<Vec<(Url, Url)>> {
        let psr4 = self.psr4_mappings.read();
        let workspace_root = self.workspace_root.read().clone()?;

        let mut ops: Vec<(Url, Url)> = Vec::new();

        for mapping in psr4.iter() {
            let mapping_ns = mapping.prefix.trim_end_matches('\\');

            // Check if old_prefix starts with this PSR-4 mapping's namespace.
            let old_lower = old_prefix.to_lowercase();
            let mapping_lower = mapping_ns.to_lowercase();

            let relative_ns = if old_lower == mapping_lower {
                ""
            } else if old_lower.starts_with(&format!("{}\\", mapping_lower)) {
                &old_prefix[mapping_ns.len() + 1..]
            } else {
                continue;
            };

            let new_relative_ns = if old_prefix.len() == mapping_ns.len() {
                // We're renaming at the PSR-4 root itself — new_prefix
                // replaces the mapping prefix entirely in the path.
                let new_without_mapping = &new_prefix[mapping_ns.len()..];
                new_without_mapping.trim_start_matches('\\').to_string()
            } else {
                let suffix = &new_prefix[mapping_ns.len() + 1..];
                suffix.to_string()
            };

            // Build old and new directory paths.
            let base_dir = workspace_root.join(&mapping.base_path);
            let old_dir = if relative_ns.is_empty() {
                base_dir.clone()
            } else {
                base_dir.join(relative_ns.replace('\\', std::path::MAIN_SEPARATOR_STR))
            };

            let new_dir = if new_relative_ns.is_empty() {
                base_dir
            } else {
                base_dir.join(new_relative_ns.replace('\\', std::path::MAIN_SEPARATOR_STR))
            };

            if old_dir == new_dir {
                continue;
            }

            // Only emit if the old directory actually exists.
            if !old_dir.is_dir() {
                continue;
            }

            let old_url = Url::from_file_path(&old_dir).ok()?;
            let new_url = Url::from_file_path(&new_dir).ok()?;
            ops.push((old_url, new_url));
        }

        if ops.is_empty() { None } else { Some(ops) }
    }
}

// ─── Namespace segment helpers ──────────────────────────────────────────────

/// Given a namespace name (e.g. `"App\\Bar\\Service"`) and its starting
/// byte offset in the source, find which segment the cursor (byte
/// offset) falls on.
///
/// Returns `(segment_text, segment_start_offset, segment_end_offset)`.
fn find_namespace_segment_at_offset(
    ns_name: &str,
    ns_start: u32,
    cursor: u32,
) -> Option<(&str, u32, u32)> {
    let mut offset = ns_start;
    for segment in ns_name.split('\\') {
        let seg_end = offset + segment.len() as u32;
        if cursor >= offset && cursor < seg_end {
            return Some((segment, offset, seg_end));
        }
        // Skip past the segment and the `\` separator.
        offset = seg_end + 1;
    }
    // If cursor is exactly at the end of the last segment, return that.
    let last_seg = ns_name.rsplit('\\').next()?;
    let last_start = ns_start + ns_name.len() as u32 - last_seg.len() as u32;
    let last_end = ns_start + ns_name.len() as u32;
    if cursor == last_end {
        return Some((last_seg, last_start, last_end));
    }
    None
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
