/// Goto-definition resolution — core entry points.
///
/// Given a cursor position in a PHP file this module:
///   1. Extracts the symbol (class / interface / trait / enum name) under the cursor.
///   2. Resolves it to a fully-qualified name using the file's `use` map and namespace.
///   3. Locates the file on disk via PSR-4 mappings.
///   4. Finds the exact line of the symbol's declaration inside that file.
///   5. Returns an LSP `Location` the editor can jump to.
///
/// Member-access resolution (methods, properties, constants via `->`, `?->`,
/// `::`) is handled by the sibling [`super::member`] module.
///
/// Variable definition resolution (`$var` → most recent assignment /
/// declaration) is handled by the sibling [`super::variable`] module.
use std::collections::HashMap;
use std::sync::Arc;

use crate::symbol_map::VarDefKind;
use tower_lsp::lsp_types::*;

use super::member::{MemberAccessHint, MemberDefinitionCtx};
use super::point_location;
use crate::Backend;
use crate::composer;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind};
use crate::types::{AccessKind, ClassInfo};
use crate::util::{find_class_at_offset, position_to_offset, short_name};
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
        if let Some(loc) =
            laravel::resolve_config_key_definition_fallback(self, uri, content, position)
        {
            return vec![loc];
        }

        // env() fallback: not yet indexed in the symbol map.
        laravel::resolve_env_definition(self, content, position)
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
        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
        map.lookup(offset).cloned()
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
        let offset = crate::util::position_to_offset(content, position);
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
        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
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
        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
        map.var_def_kind_at(var_name, cursor_offset).cloned()
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
        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
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
                        let parsed_uri = Url::parse(uri).ok()?;
                        let start =
                            crate::util::offset_to_position(content, cursor_offset as usize);
                        let end_offset = match kind {
                            SymbolKind::Variable { .. } => cursor_offset as usize + 1 + name.len(),
                            SymbolKind::CompactVariable { .. } => {
                                cursor_offset as usize + name.len()
                            }
                            _ => unreachable!(),
                        };
                        let end = crate::util::offset_to_position(content, end_offset);
                        return Some(vec![Location {
                            uri: parsed_uri,
                            range: Range { start, end },
                        }]);
                    }
                }

                if let Some(var_def) = self.lookup_var_definition(uri, name, cursor_offset) {
                    // Found a prior definition — jump there.
                    let token_end = var_def.offset + 1 + var_def.name.len() as u32;
                    let target_uri = Url::parse(uri).ok()?;
                    let start_pos =
                        crate::util::offset_to_position(content, var_def.offset as usize);
                    let end_pos = crate::util::offset_to_position(content, token_end as usize);
                    return Some(vec![Location {
                        uri: target_uri,
                        range: Range {
                            start: start_pos,
                            end: end_pos,
                        },
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
                let access_kind = if *is_static {
                    AccessKind::DoubleColon
                } else {
                    AccessKind::Arrow
                };
                let access_hint = if *is_method_call {
                    MemberAccessHint::MethodCall
                } else {
                    MemberAccessHint::PropertyAccess
                };
                let mctx = MemberDefinitionCtx {
                    member_name,
                    subject: subject_text,
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

            SymbolKind::ClassDeclaration { name }
            | SymbolKind::MemberDeclaration { name, .. }
            | SymbolKind::NamespaceDeclaration { name } => {
                // The cursor is on a declaration name.  Return the
                // symbol's own location so that editors can detect
                // "definition == current position" and fall back to
                // Find References (e.g. VS Code's
                // editor.gotoLocation.alternativeDefinitionCommand).
                let parsed_uri = Url::parse(uri).ok()?;
                let start = crate::util::offset_to_position(content, cursor_offset as usize);
                let end =
                    crate::util::offset_to_position(content, cursor_offset as usize + name.len());
                Some(vec![Location {
                    uri: parsed_uri,
                    range: Range { start, end },
                }])
            }

            SymbolKind::FunctionCall { name, .. } => {
                // Build FQN candidates: the resolved name, the raw name,
                // and (if namespaced) the namespace-qualified version.
                let ctx = self.file_context(uri);
                let fqn = ctx.resolve_name_at(name, cursor_offset);
                let mut candidates = vec![fqn];
                if name.contains('\\') && !candidates.contains(name) {
                    candidates.push(name.clone());
                }
                if !candidates.contains(name) {
                    candidates.push(name.clone());
                }
                self.resolve_function_definition(&candidates)
                    .map(|loc| vec![loc])
            }

            SymbolKind::ConstantReference { name } => {
                let ctx = self.file_context(uri);
                let fqn = ctx.resolve_name_at(name, cursor_offset);
                let mut candidates = vec![fqn];
                if !candidates.contains(name) {
                    candidates.push(name.clone());
                }
                // Try class constant (Name::CONST) first — but the symbol
                // map records class constants as MemberAccess, so this path
                // handles standalone `define()` constants and bare constant
                // references only.
                self.resolve_constant_definition(&candidates)
                    .map(|loc| vec![loc])
            }

            SymbolKind::LaravelStringKey { kind, key } => {
                let locs = laravel::resolve_laravel_string_key(self, kind, key);
                if locs.is_empty() { None } else { Some(locs) }
            }

            SymbolKind::Keyword | SymbolKind::CastType | SymbolKind::Comment => None,
        }
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
        let mut candidates = if is_fqn {
            // Already fully-qualified — use as-is.
            vec![name.to_string()]
        } else {
            let ctx = self.file_context(uri);
            let fqn = ctx.resolve_name_at(name, cursor_offset);
            let mut c = vec![fqn];
            if name.contains('\\') && !c.contains(&name.to_string()) {
                c.push(name.to_string());
            }
            c
        };
        // Always include the bare name as a last-resort candidate.
        if !candidates.contains(&name.to_string()) {
            candidates.push(name.to_string());
        }

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
            let target_uri = self.fqn_uri_index.read().get(fqn.as_str()).cloned();
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
        let workspace_root = self.workspace_root.read().clone();

        if let Some(workspace_root) = workspace_root {
            let mappings = self.psr4_mappings.read();
            for fqn in &candidates {
                if let Some(file_path) =
                    composer::resolve_class_path(&mappings, &workspace_root, fqn)
                    && let Some(location) = self.resolve_class_in_file(&file_path, fqn)
                {
                    return Some(location);
                }
            }
        }

        // ── Template parameter fallback ─────────────────────────────────
        // If no class was found, the name might be a template parameter
        // (e.g. `TKey`, `TModel`) defined in a `@template` tag on the
        // enclosing class or method docblock.
        if let Some(tpl_def) = self.lookup_template_def(uri, name, cursor_offset) {
            let target_uri = Url::parse(uri).ok()?;
            let start_pos = crate::util::offset_to_position(content, tpl_def.name_offset as usize);
            let end_pos = crate::util::offset_to_position(
                content,
                (tpl_def.name_offset + tpl_def.name.len() as u32) as usize,
            );
            return Some(Location {
                uri: target_uri,
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
            });
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
        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
        map.find_template_def(name, cursor_offset).cloned()
    }

    // ─── Constant Definition Resolution ─────────────────────────────────────

    /// Resolve a standalone constant to its `define('NAME', …)` call site.
    ///
    /// Checks `global_defines` (user-defined constants discovered from parsed
    /// files) for a matching constant name, reads the source file, and returns
    /// a `Location` pointing at the `define(` call.  When not found, checks
    /// the `autoload_constant_index` (populated by the full-scan for
    /// non-Composer projects) and lazily parses the defining file via
    /// `update_ast`.  Built-in constants from `stub_constant_index` are not
    /// navigable (they have no real file).
    fn resolve_constant_definition(&self, candidates: &[String]) -> Option<Location> {
        // ── Phase 1: Look up the constant in global_defines. ──
        let found = {
            let dmap = self.global_defines.read();
            let mut result = None;
            for candidate in candidates {
                if let Some(info) = dmap.get(candidate.as_str()) {
                    result = Some((info.file_uri.clone(), info.name_offset));
                    break;
                }
            }
            result
        };

        // ── Phase 1.5: Check autoload_constant_index (byte-level scan). ──
        // The lightweight `find_symbols` byte-level scan discovers
        // constant names at startup without a full AST parse, for both
        // non-Composer projects (workspace scan) and Composer projects
        // (autoload_files.php scan).  When a candidate matches, we
        // lazily call `update_ast` to get the complete `DefineInfo`
        // and re-check global_defines.
        let found = if found.is_some() {
            found
        } else {
            let idx = self.autoload_constant_index.read();
            let mut lazy_result = None;
            for candidate in candidates {
                if let Some(path) = idx.get(candidate.as_str()) {
                    let path = path.clone();
                    drop(idx);

                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let uri = crate::util::path_to_uri(&path);
                        self.update_ast(&uri, &content);

                        let dmap = self.global_defines.read();
                        for retry in candidates {
                            if let Some(info) = dmap.get(retry.as_str()) {
                                lazy_result = Some((info.file_uri.clone(), info.name_offset));
                                break;
                            }
                        }
                    }
                    break;
                }
            }
            lazy_result
        };

        // ── Phase 1.75: Last-resort lazy parse of known autoload files ──
        // The byte-level scanner misses constants inside conditional
        // blocks (e.g. `if (!defined(...))` guards).  As a safety net,
        // lazily parse each known autoload file via `update_ast` until
        // the constant is found.  Each file is parsed at most once:
        // subsequent lookups hit Phase 1 (`global_defines`).
        let found = if found.is_some() {
            found
        } else {
            let paths = self.autoload_file_paths.read().clone();
            let mut lazy_result = None;
            for path in &paths {
                let uri = crate::util::path_to_uri(path);
                if self.parsed_uris.read().contains(&uri) {
                    continue;
                }

                if let Ok(content) = std::fs::read_to_string(path) {
                    self.update_ast(&uri, &content);

                    let dmap = self.global_defines.read();
                    for candidate in candidates {
                        if let Some(info) = dmap.get(candidate.as_str()) {
                            lazy_result = Some((info.file_uri.clone(), info.name_offset));
                            break;
                        }
                    }
                    if lazy_result.is_some() {
                        break;
                    }
                }
            }
            lazy_result
        };

        let (file_uri, name_offset) = found?;

        // Read the file content (try open files first, then disk).
        let file_content = self.get_file_content(&file_uri)?;

        // Use the stored byte offset.  An offset of 0 means "not
        // available" — return None in that case (should not happen for
        // constants discovered via `update_ast` since the parser always
        // sets the offset).
        if name_offset == 0 {
            return None;
        }
        let position = crate::util::offset_to_position(&file_content, name_offset as usize);
        let parsed_uri = Url::parse(&file_uri).ok()?;

        Some(point_location(parsed_uri, position))
    }

    // ─── Function Definition Resolution ─────────────────────────────────────

    /// Try to resolve a standalone function name to its definition.
    ///
    /// Searches the `global_functions` map (populated from autoload files,
    /// opened/changed files, and cached stub functions) for any of the
    /// given candidate names.  If not found there, falls back to the
    /// embedded PHP stubs via `find_or_load_function` — which parses the
    /// stub lazily and caches it in `global_functions` for future lookups.
    ///
    /// When found, reads the source file and locates the `function name(`
    /// declaration line.  Stub functions (with `phpantom-stub-fn://` URIs)
    /// are not navigable so they are skipped for go-to-definition but
    /// still loaded into the cache for return-type resolution.
    fn resolve_function_definition(&self, candidates: &[String]) -> Option<Location> {
        // ── Step 1: Check global_functions (user code + cached stubs) ──
        let found = {
            let fmap = self.global_functions.read();
            let mut result = None;
            for candidate in candidates {
                if let Some((uri, info)) = fmap.get(candidate.as_str()) {
                    result = Some((uri.clone(), info.clone()));
                    break;
                }
            }
            result
        };

        // ── Step 2: Try embedded PHP stubs as fallback ──
        let (file_uri, func_info) = if let Some(pair) = found {
            pair
        } else {
            // Build &str candidates for find_or_load_function.
            let str_candidates: Vec<&str> = candidates.iter().map(|s| s.as_str()).collect();
            let loaded = self.find_or_load_function(&str_candidates)?;

            // After find_or_load_function, the function is cached in
            // global_functions.  Look it up to get the URI.
            let fmap = self.global_functions.read();
            let mut result = None;
            for candidate in candidates {
                if let Some((uri, info)) = fmap.get(candidate.as_str()) {
                    result = Some((uri.clone(), info.clone()));
                    break;
                }
            }
            result.unwrap_or_else(|| {
                // Fallback: use a synthetic URI with the loaded info.
                (format!("phpantom-stub-fn://{}", loaded.name), loaded)
            })
        };

        // Stub functions don't have real file locations — skip
        // go-to-definition for them (they're still useful for return-type
        // resolution via the function_loader).
        if file_uri.starts_with("phpantom-stub-fn://") {
            return None;
        }

        // Read the file content (try open files first, then disk).
        let file_content = self.get_file_content(&file_uri)?;

        // Use the stored byte offset.  A name_offset of 0 means "not
        // available" — return None in that case (should not happen for
        // user code since the parser always sets the offset).
        if func_info.name_offset == 0 {
            return None;
        }
        let position =
            crate::util::offset_to_position(&file_content, func_info.name_offset as usize);
        let parsed_uri = Url::parse(&file_uri).ok()?;

        Some(point_location(parsed_uri, position))
    }

    // ─── Word Extraction & FQN Resolution ───────────────────────────────────

    /// Resolve a short or partially-qualified name to a fully-qualified name
    /// using the file's `use` map and namespace context.
    ///
    /// This is a thin wrapper around [`crate::util::resolve_to_fqn`] kept
    /// for API compatibility with callers that use `Self::resolve_to_fqn`.
    pub fn resolve_to_fqn(
        name: &str,
        use_map: &HashMap<String, String>,
        namespace: &Option<String>,
    ) -> String {
        crate::util::resolve_to_fqn(name, use_map, namespace)
    }

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
        let sn = short_name(fqn);

        // Look up classes from uri_classes_index first, then fall back
        // to fqn_class_index and disk.  Each lock is dropped before
        // calling parse_and_cache_file (which takes write locks).
        let classes = if let Some(cached) = self.uri_classes_index.read().get(target_uri).cloned() {
            cached
        } else if let Some(cls) = self.fqn_class_index.read().get(fqn) {
            vec![Arc::clone(cls)]
        } else {
            let file_path = Url::parse(target_uri)
                .ok()
                .and_then(|u| u.to_file_path().ok())?;
            self.parse_and_cache_file(&file_path)?
        };

        // Match by short name + namespace, same logic as
        // `find_definition_in_uri_classes_index`.
        let class_info = classes.iter().find(|c| {
            if c.name != sn {
                return false;
            }
            c.fqn() == fqn
        })?;

        let content = self.get_file_content(target_uri)?;
        let parsed_uri = Url::parse(target_uri).ok()?;

        if class_info.keyword_offset == 0 {
            return None;
        }
        let position =
            crate::util::offset_to_position(&content, class_info.keyword_offset as usize);

        Some(point_location(parsed_uri, position))
    }

    /// Try to find the definition of a class in the current file by checking
    /// the uri_classes_index.
    pub(super) fn find_definition_in_uri_classes_index(
        &self,
        fqn: &str,
        content: &str,
        uri: &str,
    ) -> Option<Location> {
        let short_name = short_name(fqn);

        let classes = self.uri_classes_index.read().get(uri).cloned()?;

        let class_info = classes.iter().find(|c| {
            if c.name != short_name {
                return false;
            }
            // Build the FQN of this class in the current file and compare
            // against the requested FQN to avoid false matches when two
            // namespaces contain classes with the same short name.
            let class_fqn = match &c.file_namespace {
                Some(ns) if !ns.is_empty() => format!("{}\\{}", ns, c.name),
                _ => c.name.to_string(),
            };
            class_fqn == fqn
        })?;

        if class_info.keyword_offset == 0 {
            return None;
        }
        let position = crate::util::offset_to_position(content, class_info.keyword_offset as usize);

        // Build a file URI from the current URI string.
        let parsed_uri = Url::parse(uri).ok()?;

        Some(point_location(parsed_uri, position))
    }

    /// Find the position (line, character) of a class / interface / trait / enum
    /// declaration inside the given file content.
    ///
    /// Searches for patterns like:
    ///   `class ClassName`
    ///   `interface ClassName`
    ///   `trait ClassName`
    ///   `enum ClassName`
    ///   `abstract class ClassName`
    ///   `final class ClassName`
    ///   `readonly class ClassName`
    ///
    /// Returns the position of the keyword (`class`, `interface`, etc.) on
    /// the matching line.
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

        let classes: Vec<std::sync::Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        let current_class = find_class_at_offset(&classes, cursor_offset)?;

        if matches!(
            ssp_kind,
            SelfStaticParentKind::Self_ | SelfStaticParentKind::Static | SelfStaticParentKind::This
        ) {
            // Jump to the enclosing class definition in the current file.
            if current_class.keyword_offset == 0 {
                return None;
            }
            let target_position =
                crate::util::offset_to_position(content, current_class.keyword_offset as usize);
            let parsed_uri = Url::parse(uri).ok()?;
            return Some(point_location(parsed_uri, target_position));
        }

        // SelfStaticParentKind::Parent
        let parent_name = current_class.parent_class.as_ref()?;

        // Try to find the parent class in the current file first.
        // Use keyword_offset when available (the parent class is in the
        // same file's uri_classes_index entry).
        let parent_in_file = classes.iter().find(|c| c.name == *parent_name);
        let parent_pos = parent_in_file
            .filter(|pc| pc.keyword_offset > 0)
            .map(|pc| crate::util::offset_to_position(content, pc.keyword_offset as usize));
        if let Some(pos) = parent_pos {
            let parsed_uri = Url::parse(uri).ok()?;
            return Some(point_location(parsed_uri, pos));
        }

        // Resolve the parent class name to a FQN using use-map / namespace.
        let ctx = self.file_context(uri);

        let fqn = ctx.resolve_name_at(parent_name, cursor_offset);

        // Try fqn_uri_index / uri_classes_index lookup via find_class_file_content.
        if let Some((class_uri, class_content)) = self.find_class_file_content(&fqn, uri, content) {
            // Use keyword_offset from the uri_classes_index entry for the cross-file class.
            let cross_class = self.find_class_in_uri_classes_index(&fqn);
            if let Some(ref cc) = cross_class
                && cc.keyword_offset > 0
                && let Ok(parsed_uri) = Url::parse(&class_uri)
            {
                let pos =
                    crate::util::offset_to_position(&class_content, cc.keyword_offset as usize);
                return Some(point_location(parsed_uri, pos));
            }
        }

        // Try class index: direct FQN → URI lookup.
        {
            let candidates = [fqn.as_str(), parent_name.as_str()];
            for candidate in &candidates {
                if let Some(file_uri) = self.fqn_uri_index.read().get(candidate).cloned()
                    && let Some(file_path) = Url::parse(&file_uri)
                        .ok()
                        .and_then(|u| u.to_file_path().ok())
                    && let Some(location) = self.resolve_class_in_file(&file_path, candidate)
                {
                    return Some(location);
                }
            }
        }

        // Try PSR-4 resolution as a last resort.
        // PSR-4 mappings only cover user code (from composer.json).
        // Vendor classes are resolved by the class index above.
        let workspace_root = self.workspace_root.read().clone();

        if let Some(workspace_root) = workspace_root {
            let mappings = self.psr4_mappings.read();
            let candidates = [fqn.as_str(), parent_name.as_str()];
            for candidate in &candidates {
                if let Some(file_path) =
                    composer::resolve_class_path(&mappings, &workspace_root, candidate)
                    && let Some(location) = self.resolve_class_in_file(&file_path, candidate)
                {
                    return Some(location);
                }
            }
        }

        None
    }
}
