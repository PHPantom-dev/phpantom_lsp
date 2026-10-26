//! Goto-definition resolution — core entry points.
//!
//! Given a cursor position in a PHP file this module:
//!   1. Extracts the symbol (class / interface / trait / enum name) under the cursor.
//!   2. Resolves it to a fully-qualified name using the file's `use` map and namespace.
//!   3. Locates the file on disk via PSR-4 mappings.
//!   4. Finds the exact line of the symbol's declaration inside that file.
//!   5. Returns an LSP `Location` the editor can jump to.
//!
//! Member-access resolution (methods, properties, constants via `->`, `?->`,
//! `::`) is handled by the sibling [`super::member`] module.
//!
//! Variable definition resolution (`$var` → most recent assignment /
//! declaration) is also handled here, against the precomputed symbol map's
//! variable-definition index.

use std::sync::Arc;

use crate::symbol_map::VarDefKind;
use tower_lsp_server::gen_lsp_types::*;

use super::member::{MemberAccessHint, MemberDefinitionCtx, MemberKind};
use super::{class_keyword_location, point_location};
use crate::Backend;
use crate::composer;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind};
use crate::text_position::position_to_offset;
use crate::types::{AccessKind, ClassInfo};
use crate::util::short_name;
use crate::virtual_members::laravel;

impl Backend {
    /// Handle a "go to definition" request.
    ///
    /// Returns `Some(Location)` when the symbol under the cursor can be
    /// resolved to a file and a position inside that file, or `None` when
    /// resolution fails at any step.
    pub(crate) fn resolve_definition(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Vec<Location> {
        // Consult precomputed symbol map (retries one byte earlier for
        // end-of-token edge cases).
        let symbol = self.lookup_symbol_at_position(uri, content, position);
        if let Some(ref s) = symbol
            && let Some(resolved) =
                self.resolve_from_symbol(&s.kind, uri, content, position, s.start)
        {
            return resolved;
        }

        // Laravel config fallback: declaration sites in config/*.php
        if self.resolved_class_cache.read().laravel().is_enabled()
            && let Some(loc) =
                laravel::resolve_config_key_definition_fallback(self, uri, content, position)
        {
            return vec![loc];
        }

        // Request input keys: jump to the validation rule that declares them.
        if let Some(loc) = laravel::resolve_request_field_definition(self, uri, content, position) {
            return vec![loc];
        }

        // Path helpers: `base_path('routes/web.php')` and friends name a file
        // under a conventional directory of the project root.
        laravel::resolve_path_helper_definition(self, content, position)
            .into_iter()
            .collect()
    }

    /// Look up the symbol at the given byte offset in the precomputed
    /// symbol map for `uri`.
    ///
    /// Returns a cloned [`SymbolKind`] to avoid holding the mutex lock
    /// across the resolution logic.
    pub(crate) fn lookup_symbol_map(
        &self,
        uri: &str,
        offset: u32,
    ) -> Option<crate::symbol_map::SymbolSpan> {
        let map = self.symbol_map_for(uri)?;
        if let Some(span) = map.lookup(offset) {
            return Some(span.clone());
        }
        if map
            .morph_column_sites
            .iter()
            .any(|site| offset >= site.start && offset < site.end)
            && let Some(span) = self
                .morph_column_spans_for(uri, &map)
                .iter()
                .find(|span| offset >= span.start && offset < span.end)
        {
            return Some(span.clone());
        }
        // A key behind a typed receiver is a gap in the map — the indexer
        // could not tell it was one — so the cursor lands in what reads as
        // a plain string literal until the receiver is typed.
        if !map
            .typed_receiver_sites
            .iter()
            .any(|site| offset >= site.start && offset < site.end)
        {
            return None;
        }
        self.typed_receiver_spans_for(uri, &map)
            .iter()
            .find(|span| offset >= span.start && offset < span.end)
            .cloned()
    }

    /// Look up the symbol span at a cursor position, handling end-of-token
    /// edge cases by retrying one byte earlier when the exact offset
    /// produces no result.
    pub(crate) fn lookup_symbol_at_position(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<crate::symbol_map::SymbolSpan> {
        let offset = crate::text_position::position_to_offset(content, position);
        self.lookup_symbol_map(uri, offset).or_else(|| {
            if offset > 0 {
                self.lookup_symbol_map(uri, offset - 1)
            } else {
                None
            }
        })
    }

    /// Look up the most recent variable definition before `cursor_offset`
    /// in the precomputed symbol map for `uri`.
    ///
    /// Returns a cloned [`VarDefSite`] (if found) so that the mutex lock
    /// is not held across the resolution logic.
    fn lookup_var_definition(
        &self,
        uri: &str,
        var_name: &str,
        cursor_offset: u32,
    ) -> Option<crate::symbol_map::VarDefSite> {
        let map = self.symbol_map_for(uri)?;
        let scope_start = map.find_enclosing_scope(cursor_offset);
        map.find_var_definition(var_name, cursor_offset, scope_start)
            .cloned()
    }

    /// If the cursor is physically sitting on a variable definition token
    /// (assignment LHS, parameter, foreach binding, etc.), return the
    /// [`VarDefKind`] so the caller can decide how to handle it.
    pub(crate) fn lookup_var_def_kind_at(
        &self,
        uri: &str,
        var_name: &str,
        cursor_offset: u32,
    ) -> Option<VarDefKind> {
        let map = self.symbol_map_for(uri)?;
        map.var_def_kind_at(var_name, cursor_offset).cloned()
    }

    /// Whether `offset` is the declaration site of a constructor-promoted
    /// property parameter (`public function __construct(private int $x) {}`).
    ///
    /// Promoted parameters share the same byte offset as their
    /// `VarDefKind::Parameter` symbol-map entry, but they declare a real
    /// class property, not a local variable — callers that special-case
    /// `VarDefKind::Property` (cross-file references, rename, linked
    /// editing, document highlight) must treat these the same way.
    /// `ClassInfo::properties` already carries promoted parameters (built
    /// in `parser/classes.rs`), so this checks membership there instead of
    /// teaching the symbol map a new `VarDefKind`.
    pub(crate) fn is_promoted_property_param(&self, uri: &str, offset: u32) -> bool {
        self.shared_classes_for_uri(uri)
            .iter()
            .flat_map(|classes| classes.iter())
            .flat_map(|c| c.properties.iter())
            .any(|p| p.name_offset != 0 && p.name_offset == offset)
    }

    /// If the cursor is on a variable at its assignment definition site,
    /// return the `effective_from` offset (end of the assignment statement).
    ///
    /// This lets hover adjust the cursor offset so that the assignment's
    /// own RHS is included in the resolution — without it, hovering on
    /// the `$` of `$x = new Foo()` misses the assignment because the
    /// statement start coincides with the cursor offset and the
    /// "skip statements at or after cursor" guard excludes it.
    pub(crate) fn lookup_var_def_effective_from(
        &self,
        uri: &str,
        var_name: &str,
        cursor_offset: u32,
    ) -> Option<u32> {
        let map = self.symbol_map_for(uri)?;
        let def = map.var_def_at(var_name, cursor_offset)?;
        if matches!(
            def.kind,
            VarDefKind::Assignment | VarDefKind::CompoundAssignment
        ) {
            Some(def.effective_from)
        } else {
            None
        }
    }

    /// Dispatch a symbol-map hit to the appropriate resolution path.
    ///
    /// Each [`SymbolKind`] variant maps directly to existing resolution
    /// logic — the symbol map replaces the former text-scanning step
    /// with an O(log n) binary search.
    fn resolve_from_symbol(
        &self,
        kind: &SymbolKind,
        uri: &str,
        content: &str,
        position: Position,
        cursor_offset: u32,
    ) -> Option<Vec<Location>> {
        match kind {
            SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => {
                // Try the precomputed var_defs map first.
                // This avoids re-parsing the file at request time.

                // First, check if the cursor is physically on a definition
                // token (assignment LHS, parameter, foreach binding, etc.).
                // This must be checked before `find_var_definition` because
                // for assignments the definition's `effective_from` is past
                // the LHS token — the lookup would skip the definition and
                // find an earlier one instead of recognising "at definition".
                if let Some(def_kind) = self.lookup_var_def_kind_at(uri, name, cursor_offset) {
                    // Closure captures (`use ($var)`) are not terminal
                    // definition sites — the user wants to jump to the
                    // outer assignment, so we fall through to the
                    // outer-scope lookup.
                    if def_kind != VarDefKind::ClosureCapture {
                        // The cursor is on a variable at its definition
                        // site.  Return the symbol's own location so
                        // editors can fall back to Find References.
                        let parsed_uri = crate::util::parse_url(uri)?;
                        let end_offset = match kind {
                            SymbolKind::Variable { .. } => cursor_offset as usize + 1 + name.len(),
                            SymbolKind::CompactVariable { .. } => {
                                cursor_offset as usize + name.len()
                            }
                            _ => unreachable!(),
                        };
                        return Some(vec![Location {
                            uri: parsed_uri,
                            range: crate::text_position::byte_range_to_lsp_range(
                                content,
                                cursor_offset as usize,
                                end_offset,
                            ),
                        }]);
                    }
                }

                if let Some(var_def) = self.lookup_var_definition(uri, name, cursor_offset) {
                    // Found a prior definition — jump there.
                    let token_end = var_def.offset + 1 + var_def.name.len() as u32;
                    let target_uri = crate::util::parse_url(uri)?;
                    return Some(vec![Location {
                        uri: target_uri,
                        range: crate::text_position::byte_range_to_lsp_range(
                            content,
                            var_def.offset as usize,
                            token_end as usize,
                        ),
                    }]);
                }

                None
            }

            SymbolKind::MemberAccess {
                subject_text,
                member_name,
                is_static,
                is_method_call,
                ..
            } => {
                let access_kind = AccessKind::of_member_access(*is_static);
                let access_hint = if *is_method_call {
                    MemberAccessHint::MethodCall
                } else {
                    MemberAccessHint::PropertyAccess
                };
                let mctx = MemberDefinitionCtx {
                    member_name,
                    subject: subject_text.as_str(self.symbol_map_source(uri, content)?),
                    access_kind,
                    access_hint,
                };

                self.resolve_member_definition_with(uri, content, position, &mctx)
                    .map(|loc| vec![loc])
            }

            SymbolKind::SelfStaticParent(ssp_kind) => self
                .resolve_self_static_parent(uri, content, position, *ssp_kind)
                .map(|loc| vec![loc]),

            SymbolKind::ClassReference { name, is_fqn, .. } => self
                .resolve_class_reference(uri, content, name, *is_fqn, cursor_offset)
                .map(|loc| vec![loc]),

            SymbolKind::MemberDeclaration { name, .. } => {
                // Return self-location so editors detect "definition ==
                // cursor" and offer Find Usages instead of navigating.
                // Navigating to the interface or abstract prototype from a
                // declaration site makes the concrete method's usages
                // unreachable; the `implements`/`extends` clause and the
                // `textDocument/implementation` command handle prototype
                // navigation.
                self.declaration_or_usages(uri, content, cursor_offset, name)
            }

            SymbolKind::ClassDeclaration { name } => {
                // Answer with the declaration's own location even when the
                // class extends a parent: editors read "definition == cursor
                // position" as the cue to show usages instead (PHPStorm's
                // CMD+B, VS Code's alternative-definition command).  Jumping
                // to the parent here would make usages unreachable for any
                // class with an `extends` clause; the `extends` clause itself
                // is the place that navigates to the parent.
                self.declaration_or_usages(uri, content, cursor_offset, name)
            }

            SymbolKind::NamespaceDeclaration { name } => {
                self.declaration_or_usages(uri, content, cursor_offset, name)
            }

            SymbolKind::FunctionCall {
                name,
                is_definition,
                is_docblock_reference,
            } => {
                // The cursor is on the function's own name at its
                // declaration, so there is nothing to jump to: resolving
                // it would land on the line the cursor already sits on.
                // Answer with the declaration itself, which is how a
                // class, member, and namespace declaration answer, and
                // what an editor turns into a usage list.
                if *is_definition {
                    return self.declaration_or_usages(uri, content, cursor_offset, name);
                }

                // Build FQN candidates: the resolved name, the raw name,
                // and (if namespaced) the namespace-qualified version.
                let ctx = self.file_context_at(uri, cursor_offset);

                // An unqualified `@see name()` names the documented class's
                // own member first, as phpDocumentor reads it, and only
                // falls through to a global function when it has none.
                if *is_docblock_reference
                    && let Some((class, kind)) =
                        self.docblock_scope_member(&ctx, cursor_offset, name)
                {
                    let subject = class.fqn();
                    let mctx = MemberDefinitionCtx {
                        member_name: name,
                        subject: &subject,
                        access_kind: AccessKind::DoubleColon,
                        access_hint: match kind {
                            MemberKind::Method => MemberAccessHint::MethodCall,
                            _ => MemberAccessHint::PropertyAccess,
                        },
                    };
                    if let Some(loc) =
                        self.resolve_member_definition_with(uri, content, position, &mctx)
                    {
                        return Some(vec![loc]);
                    }
                }

                let fqn = ctx.resolve_name_at(name, cursor_offset);
                let mut candidates = vec![fqn];
                if name.contains('\\') && !candidates.iter().any(|c| c == name) {
                    candidates.push(name.to_string());
                }
                if !candidates.iter().any(|c| c == name) {
                    candidates.push(name.to_string());
                }
                self.resolve_function_definition(&candidates)
                    .map(|loc| vec![loc])
            }

            SymbolKind::ConstantReference { name, .. } => {
                let ctx = self.file_context(uri);
                let fqn = ctx.resolve_name_at(name, cursor_offset);
                let mut candidates = vec![fqn];
                if !candidates.iter().any(|c| c == name) {
                    candidates.push(name.to_string());
                }
                // Try class constant (Name::CONST) first — but the symbol
                // map records class constants as MemberAccess, so this path
                // handles standalone `define()` constants and bare constant
                // references only.
                self.resolve_constant_definition(&candidates)
                    .map(|loc| vec![loc])
            }

            SymbolKind::StringKey { kind, key, .. } => {
                if !self.string_key_kind_enabled(*kind) {
                    return None;
                }
                let locs = laravel::resolve_laravel_string_key(self, kind, key, uri);
                if locs.is_empty() { None } else { Some(locs) }
            }

            SymbolKind::LaravelMacroString { .. } => Some(vec![point_location(
                crate::util::parse_url(uri)?,
                crate::text_position::offset_to_position(content, cursor_offset as usize),
            )]),

            SymbolKind::CommandOwnParam { .. }
            | SymbolKind::Keyword
            | SymbolKind::CastType
            | SymbolKind::Comment => None,
        }
    }

    /// Return the declaration's own location for a symbol that has nowhere
    /// else to jump to.
    ///
    /// Editors detect "definition == current position" and offer Find
    /// References as a fallback (e.g. VS Code's
    /// `editor.gotoLocation.alternativeDefinitionCommand`).
    fn declaration_or_usages(
        &self,
        uri: &str,
        content: &str,
        cursor_offset: u32,
        name: &str,
    ) -> Option<Vec<Location>> {
        let parsed_uri = crate::util::parse_url(uri)?;
        Some(vec![Location {
            uri: parsed_uri,
            range: crate::text_position::byte_range_to_lsp_range(
                content,
                cursor_offset as usize,
                cursor_offset as usize + name.len(),
            ),
        }])
    }

    /// Resolve a `ClassReference` symbol to its definition.
    ///
    /// Tries same-file lookup (uri_classes_index), then cross-file via PSR-4.
    /// When `is_fqn` is `true`, the name is already fully-qualified
    /// (the original PHP source used a leading `\`) and should be used
    /// as-is without namespace resolution.
    pub(super) fn resolve_class_reference(
        &self,
        uri: &str,
        content: &str,
        name: &str,
        is_fqn: bool,
        cursor_offset: u32,
    ) -> Option<Location> {
        // In a docblock a template parameter in scope shadows a class of
        // the same name, so `@param T $x` under `@template T` names the
        // template even when a class `T` exists.  In code it is the class.
        if !is_fqn
            && crate::completion::source::comment_position::is_offset_inside_docblock(
                content,
                cursor_offset as usize,
            )
            && let Some(tpl_def) = self.lookup_template_def(uri, name, cursor_offset)
        {
            return template_def_location(uri, content, &tpl_def);
        }

        let candidates = if is_fqn {
            vec![name.to_string()]
        } else {
            vec![self.file_context(uri).resolve_name_at(name, cursor_offset)]
        };

        // Same-file lookup.
        for fqn in &candidates {
            if let Some(location) = self.find_definition_in_uri_classes_index(fqn, content, uri) {
                return Some(location);
            }
        }

        // Cross-file lookup via fqn_uri_index + uri_classes_index.
        //
        // Classes discovered during autoload scanning (opened files,
        // previously navigated-to vendor files) live in
        // fqn_uri_index (FQN → URI) and uri_classes_index (URI → [ClassInfo]).
        for fqn in &candidates {
            let target_uri = self.symbols.fqn_uri_index.read().get(fqn.as_str()).cloned();
            if let Some(ref target_uri) = target_uri
                && let Some(location) =
                    self.find_definition_in_uri_classes_index_cross_file(fqn, target_uri)
            {
                return Some(location);
            }
        }

        // Cross-file via PSR-4: parse on demand and cache.
        // PSR-4 mappings only cover user code (from composer.json).
        // Vendor classes are resolved by the class index above.
        let workspace_root = self.workspace.workspace_root.read().clone();

        if let Some(workspace_root) = workspace_root {
            // Resolve the paths first: `resolve_class_in_file` parses, and
            // must not run under the mappings guard.
            let paths: Vec<_> = {
                let mappings = self.workspace.psr4_mappings.read();
                candidates
                    .iter()
                    .filter_map(|fqn| {
                        composer::resolve_class_path(&mappings, &workspace_root, fqn)
                            .map(|path| (path, fqn))
                    })
                    .collect()
            };
            for (file_path, fqn) in paths {
                if let Some(location) = self.resolve_class_in_file(&file_path, fqn) {
                    return Some(location);
                }
            }
        }

        // ── Template parameter fallback ─────────────────────────────────
        // If no class was found, the name might be a template parameter
        // (e.g. `TKey`, `TModel`) defined in a `@template` tag on the
        // enclosing class or method docblock.
        if let Some(tpl_def) = self.lookup_template_def(uri, name, cursor_offset) {
            return template_def_location(uri, content, &tpl_def);
        }

        None
    }

    /// Look up a template parameter definition for `name` at
    /// `cursor_offset` in the precomputed symbol map for `uri`.
    fn lookup_template_def(
        &self,
        uri: &str,
        name: &str,
        cursor_offset: u32,
    ) -> Option<crate::symbol_map::TemplateParamDef> {
        let map = self.symbol_map_for(uri)?;
        map.find_template_def(name, cursor_offset).cloned()
    }

    // ─── Word Extraction & FQN Resolution ───────────────────────────────────

    /// Resolve a class definition in a file on disk.
    ///
    /// This is the cross-file counterpart of [`find_definition_in_uri_classes_index`].
    /// It ensures the target file is parsed and cached in `uri_classes_index`, then
    /// uses the stored `keyword_offset` to produce a precise `Location`
    /// without text searching.
    pub(super) fn resolve_class_in_file(
        &self,
        file_path: &std::path::Path,
        fqn: &str,
    ) -> Option<Location> {
        let target_uri_string = crate::util::path_to_uri(file_path);

        // Ensure the file is parsed and cached.  If the file has
        // already been parsed (opened via `did_open`, loaded from
        // autoload files, or parsed in a previous cross-file jump),
        // skip re-parsing.
        let already_cached = self.parsed_uris.read().contains(&target_uri_string);

        if !already_cached {
            self.parse_and_cache_file(file_path);
        }

        // Use AST-based lookup (keyword_offset).
        self.find_definition_in_uri_classes_index_cross_file(fqn, &target_uri_string)
    }

    /// Resolve a class FQN to the [`Location`] of its declaration.
    ///
    /// Loads the class if it is not yet in the FQN → URI index, so a target
    /// discovered from a framework index (rather than from source the user has
    /// opened) still resolves.  Used by go-to-definition on symbols that name a
    /// class indirectly, such as an Eloquent morph alias.
    pub(crate) fn class_declaration_location(&self, fqn: &str) -> Option<Location> {
        let indexed = self.symbols.fqn_uri_index.read().get(fqn).cloned();
        let target_uri = match indexed {
            Some(uri) => uri,
            None => {
                self.find_or_load_class(fqn);
                let loaded = self.symbols.fqn_uri_index.read().get(fqn).cloned();
                loaded?
            }
        };
        self.find_definition_in_uri_classes_index_cross_file(fqn, &target_uri)
    }

    /// Like [`find_definition_in_uri_classes_index`] but for cross-file jumps where
    /// we know the target file's URI (not the current file).
    ///
    /// Reads the file content and class list from the caches, finds the
    /// matching `ClassInfo`, and returns a `Location` using the stored
    /// `keyword_offset`.
    fn find_definition_in_uri_classes_index_cross_file(
        &self,
        fqn: &str,
        target_uri: &str,
    ) -> Option<Location> {
        if crate::stubs::is_embedded_stub_uri(target_uri) {
            return None;
        }
        // Look up classes from uri_classes_index first, then fall back
        // to fqn_class_index and disk.  Each lock is dropped before
        // calling parse_and_cache_file (which takes write locks).
        let cached = self
            .symbols
            .uri_classes_index
            .read()
            .get(target_uri)
            .cloned();
        let indexed = || self.symbols.fqn_class_index.read().get(fqn).cloned();
        let classes = match cached {
            Some(cached) => cached,
            None => match indexed() {
                Some(cls) => vec![cls],
                None => self.parse_and_cache_uri(target_uri)?,
            },
        };

        let class_info = find_class_by_fqn(&classes, fqn)?;
        let content = self.get_file_content(target_uri)?;
        class_keyword_location(class_info, target_uri, &content)
    }

    /// Try to find the definition of a class in the current file by checking
    /// the uri_classes_index.
    pub(super) fn find_definition_in_uri_classes_index(
        &self,
        fqn: &str,
        content: &str,
        uri: &str,
    ) -> Option<Location> {
        let classes = self.symbols.uri_classes_index.read().get(uri).cloned()?;
        class_keyword_location(find_class_by_fqn(&classes, fqn)?, uri, content)
    }

    /// Resolve `self`, `static`, or `parent` keywords to a class definition.
    ///
    /// - `self` / `static` → jump to the enclosing class declaration.
    /// - `parent` → jump to the parent class declaration (from `extends`).
    fn resolve_self_static_parent(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ssp_kind: SelfStaticParentKind,
    ) -> Option<Location> {
        let cursor_offset = position_to_offset(content, position);

        let current_class = self.class_at_offset(uri, cursor_offset)?;

        if matches!(
            ssp_kind,
            SelfStaticParentKind::Self_ | SelfStaticParentKind::Static | SelfStaticParentKind::This
        ) {
            // For `$this`, check `@param-closure-this` override first:
            // when the cursor is inside a closure whose enclosing call
            // site declares `@param-closure-this`, jump to the
            // overridden class definition instead of the lexical class.
            if ssp_kind == SelfStaticParentKind::This
                && let Some(override_cls) =
                    self.resolve_closure_this_override(uri, content, cursor_offset)
            {
                let fqn = override_cls.fqn();
                if let Some(loc) =
                    self.resolve_class_reference(uri, content, &fqn, true, cursor_offset)
                {
                    return Some(loc);
                }
            }

            // Jump to the enclosing class definition in the current file.
            return class_keyword_location(&current_class, uri, content);
        }

        // The parser has already resolved `parent_class` to an FQN.
        let parent_name = current_class.parent_class?;
        self.resolve_class_reference(uri, content, &parent_name, true, cursor_offset)
    }
}

/// The class named `fqn` in a file's class list.
///
/// Matches on the full FQN so two namespaces declaring the same short
/// name in one file never shadow each other.
fn find_class_by_fqn<'a>(classes: &'a [Arc<ClassInfo>], fqn: &str) -> Option<&'a ClassInfo> {
    // Class names are case-insensitive, so `new pdo` names `PDO`.
    let sn = short_name(fqn);
    classes
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(sn) && c.fqn().eq_ignore_ascii_case(fqn))
        .map(|c| &**c)
}

/// The location of a `@template` parameter's name in its declaring tag.
fn template_def_location(
    uri: &str,
    content: &str,
    tpl_def: &crate::symbol_map::TemplateParamDef,
) -> Option<Location> {
    let target_uri = crate::util::parse_url(uri)?;
    let start = tpl_def.name_offset as usize;
    Some(Location {
        uri: target_uri,
        range: crate::text_position::byte_range_to_lsp_range(
            content,
            start,
            start + tpl_def.name.len(),
        ),
    })
}
