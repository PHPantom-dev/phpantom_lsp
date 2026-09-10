//! Prepare-rename and the top-level rename dispatch.
//!
//! `prepareRename` validates that the symbol under the cursor is
//! renameable and returns its range and current name. `rename` produces
//! the `WorkspaceEdit`, delegating class and namespace renames to the
//! specialised handlers and building variable/property/member edits
//! directly.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolKind;
use crate::text_position::offset_to_position;
use crate::util::build_fqn;

use super::RenameOutcome;
use super::namespace::find_namespace_segment_at_offset;
use super::validate::span_spells_its_name;

/// The text a single function or constant reference should be replaced
/// with, or `None` when it must be left exactly as it is.
///
/// Either is written three ways across its references and each one takes
/// a different edit:
///
/// - `use function Foo\bar;` names it qualified.  Only the last segment
///   is the symbol's name; replacing the whole span would drop the
///   namespace and leave `use function baz;`.
/// - `bar()` names it plainly and takes the new name.
/// - `quux()`, under `use function Foo\bar as quux;`, does not name the
///   symbol at all.  The alias is a local name for it and stays valid
///   once the symbol is renamed, so rewriting it to `baz()` would break
///   a file that compiles.
fn import_aware_edit_text(
    content: Option<&str>,
    range: Range,
    old_short_name: &str,
    new_name: &str,
    case_insensitive: bool,
) -> Option<String> {
    let Some(content) = content else {
        return Some(new_name.to_string());
    };

    let start = crate::text_position::position_to_byte_offset(content, range.start);
    let end = crate::text_position::position_to_byte_offset(content, range.end);
    let Some(text) = content.get(start..end) else {
        return Some(new_name.to_string());
    };

    let names_the_symbol = |candidate: &str| {
        if case_insensitive {
            candidate.eq_ignore_ascii_case(old_short_name)
        } else {
            candidate == old_short_name
        }
    };

    match text.rsplit_once('\\') {
        Some((namespace, short)) if names_the_symbol(short) => {
            Some(format!("{}\\{}", namespace, new_name))
        }
        // A qualified name whose last segment is not the symbol is an
        // alias reached through an imported namespace; leave it alone.
        Some(_) => None,
        None if names_the_symbol(text) => Some(new_name.to_string()),
        None => None,
    }
}

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
        // A YAML/XML occurrence is not a rename site.  Its text may be the
        // escaped `App\\Handler` form the document's quoting requires
        // rather than the PHP spelling, so nothing here can plan the edit
        // that replaces it, and the PHP occurrences are reached from the
        // declaration instead.
        if crate::resource_navigation::is_resource_document(uri) {
            return None;
        }

        let span = self.lookup_symbol_at_position(uri, content, position)?;

        // The range below is built from this span's byte offsets, and the
        // editor shows it as the text about to be replaced.  A map that
        // predates the buffer would put it over unrelated code.
        if !self.rename_map_matches(uri, content) || !span_spells_its_name(content, &span) {
            return None;
        }

        // self, static, parent, and $this are never renameable.
        if let SymbolKind::SelfStaticParent(_) = &span.kind {
            return None;
        }

        // Namespace rename: offer the full namespace so the user can edit
        // multiple segments at once.
        if let SymbolKind::NamespaceDeclaration { ref name } = span.kind {
            let range = Range {
                start: offset_to_position(content, span.start as usize),
                end: offset_to_position(content, span.end as usize),
            };
            return Some(PrepareRenameResponse::RangeWithPlaceholder {
                range,
                placeholder: name.to_string(),
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

        // For class declarations, show the FQN as placeholder so the
        // user can change the namespace to move the class.
        let placeholder = if let SymbolKind::ClassDeclaration { ref name } = span.kind {
            let ctx = self.file_context(uri);
            build_fqn(name, ctx.namespace.as_deref())
        } else {
            name
        };

        Some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder })
    }

    /// Handle `textDocument/rename`.
    ///
    /// Produces a `WorkspaceEdit` that renames every occurrence of the
    /// symbol under the cursor to `new_name`.
    ///
    /// `Ok(None)` is the "nothing to rename here" answer the editor
    /// reports in its own words.  `Err(message)` is a refusal the user
    /// needs to read: a move whose destination is already taken would
    /// either clobber a file or leave two classes claiming one name, and
    /// saying so beats emitting an edit the editor half-applies.
    pub(crate) fn handle_rename(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        new_name: &str,
    ) -> RenameOutcome {
        // A YAML/XML occurrence is not a rename site.  Its text may be the
        // escaped `App\\Handler` form the document's quoting requires
        // rather than the PHP spelling, so nothing here can plan the edit
        // that replaces it, and the PHP occurrences are reached from the
        // declaration instead.
        if crate::resource_navigation::is_resource_document(uri) {
            return Ok(None);
        }

        let Some(span) = self.lookup_symbol_at_position(uri, content, position) else {
            return Ok(None);
        };

        // Every edit below is derived, directly or through find-references,
        // from this span.  If the map it came from predates the buffer the
        // whole response is nonsense, so drop it rather than rename the
        // wrong symbol.
        if !self.rename_map_matches(uri, content) || !span_spells_its_name(content, &span) {
            return Ok(None);
        }

        // Reject non-renameable symbols (same logic as prepare_rename).
        if let SymbolKind::SelfStaticParent(_) = &span.kind {
            // self, static, parent, and $this are never renameable.
            return Ok(None);
        }

        if self.is_vendor_symbol(uri, content, position) {
            return Ok(None);
        }

        if let SymbolKind::NamespaceDeclaration { ref name } = span.kind {
            if new_name.contains('\\') {
                return self.build_namespace_prefix_rename_edit(name, new_name);
            }
            let cursor_byte = crate::text_position::position_to_byte_offset(content, position);
            let Some((segment, _seg_start, _seg_end)) =
                find_namespace_segment_at_offset(name, span.start, cursor_byte as u32)
            else {
                return Ok(None);
            };
            let Some(segment_idx) = name.split('\\').position(|s| s == segment) else {
                return Ok(None);
            };
            return self.build_namespace_rename_edit(name, segment_idx, new_name);
        }

        let class_rename_fqn = self.resolve_class_rename_fqn(&span.kind, uri, span.start);

        // Find all references (including the declaration).
        let Some(locations) = self.find_references_for_rename(uri, content, position, true) else {
            return Ok(None);
        };

        if locations.is_empty() {
            return Ok(None);
        }

        // The reference finders read one symbol map per file, so each
        // location has to be checked against *that* file's text, not the
        // buffer this request arrived on.
        if !self.rename_locations_verified(&span.kind, &locations) {
            return Ok(None);
        }

        // A function is named three different ways across its references,
        // and only one of them is the plain new name.  The old short name
        // comes from the resolved FQN rather than from the text under the
        // cursor, which is the alias itself when the rename is started
        // from an aliased call.
        // PHP matches function names case-insensitively and constant
        // names case-sensitively, so `BAR()` is a reference to `bar` but
        // `bar` is not a use of `BAR`.
        let import_aware_target = match &span.kind {
            SymbolKind::FunctionCall { name, .. } => {
                let fqn = self.function_fqn_at(uri, span.start, name);
                Some((crate::util::short_name(&fqn).to_string(), true))
            }
            SymbolKind::ConstantReference { name, .. } => {
                let fqn = self.constant_fqn_at(uri, span.start, name);
                Some((crate::util::short_name(&fqn).to_string(), false))
            }
            _ => None,
        };

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
            if new_name.contains('\\') {
                return self.build_class_move_edit(fqn, new_name, &locations);
            }
            return Ok(self.build_class_rename_edit(fqn, new_name, &locations));
        }

        // Build the workspace edit.  Group text edits by document URI.
        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        for location in &locations {
            let loc_uri_str = location.uri.to_string();

            // For each reference location, we need the file content to
            // inspect what text is at that range.  A template's locations
            // index the virtual PHP it lowers to, which is what the
            // request's own buffer already holds for it.
            let loc_content = if loc_uri_str == uri {
                Some(content.to_string())
            } else {
                self.reference_file_content(&loc_uri_str)
            };

            // An import names the function qualified (`use function
            // Foo\bar;`) and an aliased call does not name it at all, so
            // neither can take the new name as written.  A location that
            // yields no text is one an alias keeps naming under its own
            // name, and editing it would break a file that compiles.
            if let Some((ref old_short, case_insensitive)) = import_aware_target {
                if let Some(text) = import_aware_edit_text(
                    loc_content.as_deref(),
                    location.range,
                    old_short,
                    new_name,
                    case_insensitive,
                ) {
                    changes
                        .entry(location.uri.clone())
                        .or_default()
                        .push(TextEdit {
                            range: location.range,
                            new_text: text,
                        });
                }
                continue;
            }

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
                    let start_off =
                        crate::text_position::position_to_byte_offset(c, location.range.start);
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

        // A template's references were found in the virtual PHP it lowers
        // to, so its edits still have to come back to the template's own
        // coordinates.
        for (loc_uri, edits) in &mut changes {
            self.translate_template_edits(loc_uri.as_str(), edits);
        }
        changes.retain(|_, edits| !edits.is_empty());

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }))
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
            SymbolKind::CompactVariable { name } => Some((name.to_string(), range)),
            SymbolKind::ClassReference { name, .. } => Some((name.to_string(), range)),
            SymbolKind::ClassDeclaration { name } => Some((name.to_string(), range)),
            SymbolKind::MemberAccess { member_name, .. } => Some((member_name.to_string(), range)),
            SymbolKind::MemberDeclaration { name, .. } => Some((name.to_string(), range)),
            SymbolKind::FunctionCall { name, .. } => Some((name.to_string(), range)),
            SymbolKind::ConstantReference { name, .. } => Some((name.to_string(), range)),
            SymbolKind::NamespaceDeclaration { name } => Some((name.to_string(), range)),
            SymbolKind::LaravelMacroString { name } => Some((name.clone(), range)),
            SymbolKind::SelfStaticParent { .. } => None,
            SymbolKind::LaravelStringKey { .. }
            | SymbolKind::CommandOwnParam { .. }
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
        let vendor_prefixes = self.workspace.vendor_uri_prefixes.lock().clone();

        if vendor_prefixes.is_empty() {
            return false;
        }

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
                // Variable spans can represent property declarations,
                // including constructor-promoted properties (tagged
                // VarDefKind::Parameter since the token is also a real
                // parameter, but still declares a class property).
                self.lookup_var_def_kind_at(uri, name, span.start)
                    .is_some_and(|k| k == crate::symbol_map::VarDefKind::Property)
                    || self.is_promoted_property_param(uri, span.start)
            }
            SymbolKind::CompactVariable { .. } => false,
            _ => false,
        }
    }
}
