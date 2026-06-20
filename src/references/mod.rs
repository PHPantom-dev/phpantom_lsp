//! Find References (`textDocument/references`).
//!
//! When the user invokes "Find All References" on a symbol, the LSP
//! collects every occurrence of that symbol across the project.
//!
//! **Same-file references** are answered from the precomputed
//! [`SymbolMap`] — we iterate all spans and collect those that match
//! the symbol under the cursor.
//!
//! **Cross-file references** iterate every `SymbolMap` stored in
//! `self.symbol_maps` (one per opened / parsed file).  For files that
//! are in the workspace but have not been opened yet, we lazily parse
//! them on demand (via the fqn_uri_index, PSR-4, and workspace scan).
//!
//! **Variable references** (including `$this`) are strictly scoped to
//! the enclosing function / method / closure body within the current
//! file.
//!
//! **Member references** (methods, properties, constants) are filtered
//! by the class hierarchy of the target member.  When the user triggers
//! "Find References" on `MyClass::save()`, only accesses where the
//! subject resolves to a class in the same inheritance tree are returned.
//! Accesses on unrelated classes that happen to have a member with the
//! same name are excluded.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::Backend;
use crate::symbol_map::{ClassRefContext, SelfStaticParentKind, SymbolKind, SymbolMap, VarDefKind};
use crate::types::ClassInfo;
use crate::util::{
    build_fqn, collect_php_files_gitignore, find_class_at_offset, offset_to_position,
    position_to_offset, push_unique_location, strip_fqn_prefix,
};
use crate::virtual_members::laravel;

impl Backend {
    /// Entry point for `textDocument/references`.
    ///
    /// Returns all locations where the symbol under the cursor is
    /// referenced.  When `include_declaration` is true the declaration
    /// site itself is included in the results.
    pub fn find_references(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        let start_total = std::time::Instant::now();
        tracing::info!(
            "Find References: starting at {} line {} char {}",
            uri,
            position.line,
            position.character
        );

        // Consult the precomputed symbol map for the current file
        // (retries one byte earlier for end-of-token edge cases).
        let symbol = self.lookup_symbol_at_position(uri, content, position);

        // When the cursor is on a symbol span, dispatch by kind.
        if let Some(ref sym) = symbol {
            tracing::info!(
                "Find References: found symbol kind {:?} at offset {}",
                sym.kind,
                sym.start
            );
            let locations = self.dispatch_symbol_references(
                &sym.kind,
                uri,
                content,
                sym.start,
                include_declaration,
            );
            tracing::info!(
                "Find References: total time for {:?}: {:?}",
                sym.kind,
                start_total.elapsed()
            );
            if !locations.is_empty() {
                return Some(locations);
            }
        }

        // Fallback for declaration sites in config/*.php
        let start_laravel = std::time::Instant::now();
        if let Some(locations) =
            laravel::find_config_references(self, uri, content, position, include_declaration)
        {
            tracing::info!(
                "Find References: found Laravel config references in {:?}",
                start_laravel.elapsed()
            );
            tracing::info!(
                "Find References: total time (fallback path): {:?}",
                start_total.elapsed()
            );
            return Some(locations);
        }

        tracing::info!(
            "Find References: no references found in {:?}",
            start_total.elapsed()
        );
        None
    }

    /// Dispatch a symbol-map hit to the appropriate reference finder.
    fn dispatch_symbol_references(
        &self,
        kind: &SymbolKind,
        uri: &str,
        content: &str,
        span_start: u32,
        include_declaration: bool,
    ) -> Vec<Location> {
        match kind {
            SymbolKind::Variable { name } => {
                // Property declarations use Variable spans (so GTD can
                // jump to the type hint), but Find References should
                // search for member accesses, not local variable uses.
                if let Some(crate::symbol_map::VarDefKind::Property) =
                    self.lookup_var_def_kind_at(uri, name, span_start)
                {
                    // Properties are never static in the Variable span
                    // context ($this->prop).  Static properties use
                    // MemberAccess spans at their usage sites with
                    // is_static=true, but the declaration-site Variable
                    // span doesn't encode static-ness.  Check the
                    // uri_classes_index to determine the correct flag.
                    let is_static = self
                        .get_classes_for_uri(uri)
                        .iter()
                        .flat_map(|classes| classes.iter())
                        .flat_map(|c| c.properties.iter())
                        .any(|p| {
                            let p_name = p.name.strip_prefix('$').unwrap_or(&p.name);
                            p_name == name && p.is_static
                        });

                    // Resolve the enclosing class to scope the search.
                    let hierarchy =
                        self.resolve_member_declaration_hierarchy(uri, span_start, name, is_static);
                    return self.find_member_references(
                        name,
                        is_static,
                        include_declaration,
                        hierarchy.as_ref(),
                    );
                }
                self.find_variable_references(uri, content, name, span_start, include_declaration)
            }
            SymbolKind::ClassReference { name, is_fqn, .. } => {
                let ctx = self.file_context(uri);
                let fqn = if *is_fqn {
                    name.clone()
                } else {
                    ctx.resolve_name_at(name, span_start)
                };
                self.find_class_references(&fqn, include_declaration)
            }
            SymbolKind::ClassDeclaration { name } => {
                let ctx = self.file_context(uri);
                let fqn = build_fqn(name, ctx.namespace.as_deref());
                self.find_class_references(&fqn, include_declaration)
            }
            SymbolKind::MemberAccess {
                subject_text,
                member_name,
                is_static,
                ..
            } => {
                // Resolve the subject to determine the class hierarchy
                // so we only return references on related classes.
                let hierarchy = self.resolve_member_access_hierarchy(
                    uri,
                    subject_text,
                    *is_static,
                    span_start,
                    member_name,
                );

                // Constructors are not invoked through member accesses
                // (`$obj->__construct()`); they are invoked through
                // `new ClassName(...)`.  An explicit `parent::__construct()`
                // call still lands here, so route to the constructor finder
                // seeded with the subject's resolved class(es).
                if is_constructor_name(member_name) {
                    let seeds = self
                        .get_file_content(uri)
                        .map(|content| {
                            self.resolve_subject_to_fqns(
                                subject_text,
                                *is_static,
                                &self.file_context(uri),
                                span_start,
                                &content,
                            )
                        })
                        .unwrap_or_default();
                    return self.find_constructor_references(&seeds, include_declaration);
                }

                self.find_member_references(
                    member_name,
                    *is_static,
                    include_declaration,
                    hierarchy.as_ref(),
                )
            }
            SymbolKind::FunctionCall { name, .. } => {
                let ctx = self.file_context(uri);
                let fqn = ctx.resolve_name_at(name, span_start);
                self.find_function_references(&fqn, name, include_declaration)
            }
            SymbolKind::ConstantReference { name } => {
                self.find_constant_references(name, include_declaration)
            }
            SymbolKind::MemberDeclaration { name, is_static } => {
                // A constructor declaration's "references" are the
                // `new ClassName(...)` instantiation sites (and `#[...]`
                // attribute usages), not `->__construct()` member accesses
                // (which don't exist in normal PHP code).
                if is_constructor_name(name) {
                    let ctx = self.file_context(uri);
                    let seeds: Vec<String> =
                        crate::util::find_class_at_offset(&ctx.classes, span_start)
                            .map(|cc| vec![build_fqn(&cc.name, ctx.namespace.as_deref())])
                            .unwrap_or_default();
                    return self.find_constructor_references(&seeds, include_declaration);
                }

                // Resolve the enclosing class to scope the search.
                let hierarchy =
                    self.resolve_member_declaration_hierarchy(uri, span_start, name, *is_static);
                self.find_member_references(
                    name,
                    *is_static,
                    include_declaration,
                    hierarchy.as_ref(),
                )
            }
            SymbolKind::SelfStaticParent(ssp_kind) => {
                // `$this` is a file-local variable, not a cross-file class search.
                if *ssp_kind == SelfStaticParentKind::This {
                    return self.find_this_references(
                        uri,
                        content,
                        span_start,
                        include_declaration,
                    );
                }

                // For real self/static/parent keywords, resolve to the class FQN.
                let ctx = self.file_context(uri);
                let current_class = crate::util::find_class_at_offset(&ctx.classes, span_start);
                let fqn = match ssp_kind {
                    SelfStaticParentKind::Parent => {
                        current_class.and_then(|cc| cc.parent_class.map(|a| a.to_string()))
                    }
                    _ => current_class.map(|cc| build_fqn(&cc.name, ctx.namespace.as_deref())),
                };
                if let Some(fqn) = fqn {
                    self.find_class_references(&fqn, include_declaration)
                } else {
                    Vec::new()
                }
            }

            SymbolKind::NamespaceDeclaration { .. } => Vec::new(),

            SymbolKind::LaravelStringKey { kind, key } => {
                let snapshot = self.user_file_symbol_maps();
                laravel::find_laravel_string_key_references(
                    self,
                    kind,
                    key,
                    &snapshot,
                    include_declaration,
                )
            }

            SymbolKind::Keyword | SymbolKind::CastType | SymbolKind::Comment => Vec::new(),
        }
    }

    /// Find all references to a variable within its enclosing scope.
    ///
    /// Variables are file-local and scope-local — a `$user` in method A
    /// must not match `$user` in method B.
    fn find_variable_references(
        &self,
        uri: &str,
        content: &str,
        var_name: &str,
        cursor_offset: u32,
        include_declaration: bool,
    ) -> Vec<Location> {
        let mut locations = Vec::new();

        let maps = self.symbol_maps.read();
        let symbol_map = match maps.get(uri) {
            Some(m) => m,
            None => return locations,
        };

        // Determine the effective scope for this variable.
        //
        // `find_variable_scope` handles the tricky cases where the
        // cursor is on a parameter (physically before the `{`) or on
        // a docblock `@param $var` mention, returning the body scope
        // those tokens logically belong to.
        //
        // We then walk upward from the initial scope to the nearest
        // declaring scope for the variable (stopping at Parameter,
        // Assignment, Foreach, etc. but skipping ClosureCapture so
        // that uses inside explicit-capturing closures still see their
        // outer declaration). This makes rename and find-references
        // work correctly when invoked from deep inside nested arrows
        // or closures.
        let mut scope_start = symbol_map.find_variable_scope(var_name, cursor_offset);
        {
            let mut decl = scope_start;
            let mut cur = scope_start;
            while cur != 0 {
                let has_def = symbol_map.var_defs.iter().any(|d| {
                    d.name == var_name
                        && d.scope_start == cur
                        && !matches!(
                            d.kind,
                            VarDefKind::ClosureCapture
                                | VarDefKind::Unset
                                | VarDefKind::CompoundAssignment
                                | VarDefKind::DocblockVar
                                | VarDefKind::Property
                        )
                });
                if has_def {
                    decl = cur;
                    break;
                }
                let parent = symbol_map.find_enclosing_scope(cur.saturating_sub(1));
                if parent == cur {
                    break;
                }
                cur = parent;
            }
            scope_start = decl;
        }
        let parsed_uri = match Url::parse(uri) {
            Ok(u) => u,
            Err(_) => return locations,
        };

        // Build the set of reachable scopes: the primary (declaring)
        // scope plus every nested closure/arrow-function scope that
        // can see the variable (via explicit `use` or implicit arrow
        // capture) without being shadowed.
        let reachable_scopes = Self::collect_capture_scopes(symbol_map, var_name, scope_start);

        for span in &symbol_map.spans {
            if let SymbolKind::Variable { name } = &span.kind {
                if name != var_name {
                    continue;
                }
                // Check that this variable is in a reachable scope.
                let span_scope = symbol_map.find_variable_scope(name, span.start);
                if !reachable_scopes.contains(&span_scope) {
                    continue;
                }
                // Optionally skip declaration sites.
                if !include_declaration && symbol_map.var_def_kind_at(name, span.start).is_some() {
                    continue;
                }
                let start = offset_to_position(content, span.start as usize);
                let end = offset_to_position(content, span.end as usize);
                locations.push(Location {
                    uri: parsed_uri.clone(),
                    range: Range { start, end },
                });
            }
        }

        // Also include var_def sites if include_declaration is set,
        // since some definition tokens (parameters, foreach bindings)
        // may not have a corresponding Variable span in the spans vec
        // with the exact same offset.
        if include_declaration {
            let mut seen_offsets: HashSet<u32> = locations
                .iter()
                .map(|loc| position_to_offset(content, loc.range.start))
                .collect();

            for def in &symbol_map.var_defs {
                if def.name == var_name
                    && reachable_scopes.contains(&def.scope_start)
                    && seen_offsets.insert(def.offset)
                {
                    let start = offset_to_position(content, def.offset as usize);
                    // The token is `$` + name.
                    let end_offset = def.offset as usize + 1 + def.name.len();
                    let end = offset_to_position(content, end_offset);
                    locations.push(Location {
                        uri: parsed_uri.clone(),
                        range: Range { start, end },
                    });
                }
            }
        }

        // Sort by position for stable output.
        locations.sort_by(|a, b| {
            a.range
                .start
                .line
                .cmp(&b.range.start.line)
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        locations
    }

    /// Collect all scopes reachable from `root_scope` for `var_name`
    /// through closure `use` captures and implicit arrow-function captures.
    ///
    /// Returns a set containing `root_scope` plus every nested
    /// closure/arrow scope that captures the variable without shadowing
    /// it with a new parameter of the same name.
    fn collect_capture_scopes(
        symbol_map: &SymbolMap,
        var_name: &str,
        root_scope: u32,
    ) -> HashSet<u32> {
        let mut reachable = HashSet::new();
        reachable.insert(root_scope);
        let scope_ends: HashMap<u32, u32> = symbol_map.scopes.iter().cloned().collect();
        fn has_usage(
            symbol_map: &SymbolMap,
            var_name: &str,
            scope_start: u32,
            scope_ends: &HashMap<u32, u32>,
        ) -> bool {
            symbol_map.spans.iter().any(|s| {
                if let SymbolKind::Variable { name } = &s.kind {
                    name == var_name
                        && scope_ends
                            .get(&scope_start)
                            .is_some_and(|&e| s.start >= scope_start && s.start <= e)
                } else {
                    false
                }
            })
        }

        // Explicit closure captures: `function () use ($var) { … }`
        // These have VarDefKind::ClosureCapture with scope_start
        // pointing to the closure body.
        for def in &symbol_map.var_defs {
            if def.name != var_name || def.kind != VarDefKind::ClosureCapture {
                continue;
            }
            // The `use ($var)` token sits physically in the outer scope.
            // Check if the outer scope is already reachable.
            let outer_scope = symbol_map.find_enclosing_scope(def.offset);
            if reachable.contains(&outer_scope) {
                reachable.insert(def.scope_start);
            }
        }

        // Implicit arrow-function captures: `fn () => $var`
        // Arrow functions have a scope entry but no ClosureCapture def.
        // A variable is implicitly captured if:
        //   1. The arrow scope is directly nested in a reachable scope.
        //   2. There is no parameter with the same name in the arrow scope.
        //
        // Note: the caller (find_variable_references) has already
        // normalized the incoming root_scope to the actual declaring
        // scope by walking ancestors.  This lets us start from the
        // correct root whether the request originated on a declaration
        // or deep inside nested arrows/closures.
        for &(scope_start, _scope_end) in &symbol_map.scopes {
            if reachable.contains(&scope_start) {
                continue; // Already reachable, skip.
            }
            // Find the parent scope of this scope.
            let parent = symbol_map.find_enclosing_scope(scope_start.saturating_sub(1));
            if !reachable.contains(&parent) {
                continue;
            }
            // Check if this is an arrow function scope (no ClosureCapture
            // or Parameter def that would indicate a closure with `use`).
            // Arrow scopes don't have braces; their scope_start is the
            // arrow function expression's start offset.
            //
            // Skip if there's a parameter with the same name (shadowed).
            let has_shadowing_param = symbol_map.var_defs.iter().any(|d| {
                d.name == var_name
                    && d.scope_start == scope_start
                    && d.kind == VarDefKind::Parameter
            });
            if has_shadowing_param {
                continue;
            }
            // Check if this scope actually uses the variable (has a
            // Variable span in it).  Only include it if the variable
            // appears there to avoid false positives with unrelated
            // nested functions.
            //
            // has_usage uses lexical containment (usage offset lies
            // inside the scope's byte range) rather than checking
            // whether find_variable_scope reports exactly this scope.
            // This is required to correctly handle chains of nested
            // arrows (`fn()=>fn()=> $var`) where the usage's innermost
            // scope is deeper than the intermediate arrow.
            //
            // We also still need to check: is this scope a closure body
            // (not an arrow function)?  Closures create new variable
            // scopes and require explicit `use` — if there's no
            // ClosureCapture def for this scope, the variable is NOT
            // available inside a regular closure.  We only auto-include
            // arrow function scopes.
            //
            // Heuristic: if there's any ClosureCapture or Parameter def
            // for *any* variable scoped to this scope_start, and there's
            // no ClosureCapture for *our* variable, this is likely a
            // closure that didn't capture our variable — skip it.
            let is_closure_scope = symbol_map
                .var_defs
                .iter()
                .any(|d| d.scope_start == scope_start && d.kind == VarDefKind::ClosureCapture);
            if is_closure_scope {
                // It's a closure scope.  Our variable is not in the `use`
                // list (we already handled ClosureCapture above), so the
                // variable is not available here.
                continue;
            }
            // This is an arrow function scope or similar transparent
            // scope.  The variable is implicitly captured.
            if has_usage(symbol_map, var_name, scope_start, &scope_ends) {
                reachable.insert(scope_start);
            }
        }

        // Recurse: newly added scopes may themselves contain nested
        // closures/arrows that capture the same variable.
        // Fixed-point iteration until no new scopes are added.
        let mut prev_len = 0;
        while reachable.len() != prev_len {
            prev_len = reachable.len();
            let current = reachable.clone();

            for def in &symbol_map.var_defs {
                if def.name != var_name || def.kind != VarDefKind::ClosureCapture {
                    continue;
                }
                if reachable.contains(&def.scope_start) {
                    continue;
                }
                let outer_scope = symbol_map.find_enclosing_scope(def.offset);
                if reachable.contains(&outer_scope) {
                    reachable.insert(def.scope_start);
                }
            }

            for &(scope_start, _scope_end) in &symbol_map.scopes {
                if reachable.contains(&scope_start) {
                    continue;
                }
                let parent = symbol_map.find_enclosing_scope(scope_start.saturating_sub(1));
                if !current.contains(&parent) {
                    continue;
                }
                let has_shadowing_param = symbol_map.var_defs.iter().any(|d| {
                    d.name == var_name
                        && d.scope_start == scope_start
                        && d.kind == VarDefKind::Parameter
                });
                if has_shadowing_param {
                    continue;
                }
                let is_closure_scope = symbol_map
                    .var_defs
                    .iter()
                    .any(|d| d.scope_start == scope_start && d.kind == VarDefKind::ClosureCapture);
                if is_closure_scope {
                    continue;
                }
                if has_usage(symbol_map, var_name, scope_start, &scope_ends) {
                    reachable.insert(scope_start);
                }
            }
        }

        reachable
    }

    /// Find all references to `$this` within the enclosing class body.
    ///
    /// `$this` is scoped to the enclosing class — it must not match
    /// `$this` in a different class or top-level function.  Unlike
    /// regular variables, `$this` is **not** scoped to the enclosing
    /// method: `$this` in method A and `$this` in method B inside the
    /// same class both refer to the same object, so they should all
    /// appear in the results.
    fn find_this_references(
        &self,
        uri: &str,
        content: &str,
        cursor_offset: u32,
        include_declaration: bool,
    ) -> Vec<Location> {
        let _ = include_declaration; // $this has no "declaration site"
        let mut locations = Vec::new();

        let maps = self.symbol_maps.read();
        let symbol_map = match maps.get(uri) {
            Some(m) => m,
            None => return locations,
        };

        // Determine the class body the cursor is in.
        let ctx_classes: Vec<Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let current_class = crate::util::find_class_at_offset(&ctx_classes, cursor_offset);
        let (class_start, class_end) = match current_class {
            Some(cc) => (cc.start_offset, cc.end_offset),
            None => return locations,
        };

        let parsed_uri = match Url::parse(uri) {
            Ok(u) => u,
            Err(_) => return locations,
        };

        for span in &symbol_map.spans {
            // Only consider spans within the same class body.
            if span.start < class_start || span.start > class_end {
                continue;
            }

            let is_this = matches!(
                &span.kind,
                SymbolKind::SelfStaticParent(SelfStaticParentKind::This)
            );

            if is_this {
                let start = offset_to_position(content, span.start as usize);
                let end = offset_to_position(content, span.end as usize);
                locations.push(Location {
                    uri: parsed_uri.clone(),
                    range: Range { start, end },
                });
            }
        }

        locations.sort_by(|a, b| {
            a.range
                .start
                .line
                .cmp(&b.range.start.line)
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        locations
    }

    /// Snapshot all symbol maps for user (non-vendor, non-stub) files.
    ///
    /// Ensures the workspace is indexed first, then returns a cloned
    /// snapshot of every symbol map whose URI does not fall under the
    /// vendor directory or the internal stub scheme.  All four cross-file
    /// reference scanners use this to restrict results to user code.
    pub(crate) fn user_file_symbol_maps(&self) -> Vec<(String, Arc<SymbolMap>)> {
        self.ensure_workspace_indexed();

        let vendor_prefixes = self.vendor_uri_prefixes.lock().clone();

        let maps = self.symbol_maps.read();
        maps.iter()
            .filter(|(uri, _)| {
                !uri.starts_with("phpantom-stub://")
                    && !uri.starts_with("phpantom-stub-fn://")
                    && !vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str()))
            })
            .map(|(uri, map)| (uri.clone(), Arc::clone(map)))
            .collect()
    }

    /// Find all references to a class/interface/trait/enum across all files.
    ///
    /// Matches `ClassReference` spans whose resolved FQN equals `target_fqn`,
    /// and optionally `ClassDeclaration` spans at the declaration site.
    fn find_class_references(&self, target_fqn: &str, include_declaration: bool) -> Vec<Location> {
        let mut locations = Vec::new();

        // Normalise: strip leading backslash if present.
        let target = strip_fqn_prefix(target_fqn);
        let target_short = crate::util::short_name(target);

        // Snapshot user-file symbol maps (excludes vendor and stubs).
        let snapshot = self.user_file_symbol_maps();

        for (file_uri, symbol_map) in &snapshot {
            // Prefer mago-names resolved_names for FQN resolution (byte-offset
            // based, applies PHP's full name resolution rules).  Falls back to
            // the legacy use_map lazily for identifiers not tracked by
            // mago-names (e.g. docblock-sourced references).
            let resolved_names = self.resolved_names.read().get(file_uri).cloned();
            let file_namespace = self.first_file_namespace(file_uri);
            let file_use_map = std::cell::OnceCell::new();

            // First pass: resolved-name check to avoid unnecessary content work.
            // Aliased imports (`use Foo as Bar; new Bar`) must still reach the
            // full matching loop, because the textual span name is the alias.
            let has_potential_match = symbol_map.spans.iter().any(|span| match &span.kind {
                SymbolKind::ClassReference { name, .. } => {
                    if crate::util::short_name(name) == target_short {
                        true
                    } else {
                        let resolved = if let Some(fqn) =
                            resolved_names.as_ref().and_then(|rn| rn.get(span.start))
                        {
                            fqn.to_string()
                        } else {
                            let use_map = file_use_map.get_or_init(|| {
                                self.file_imports
                                    .read()
                                    .get(file_uri)
                                    .cloned()
                                    .unwrap_or_default()
                            });
                            Self::resolve_to_fqn(name, use_map, &file_namespace)
                        };
                        class_names_match(strip_fqn_prefix(&resolved), target, target_short)
                    }
                }
                SymbolKind::ClassDeclaration { name } => {
                    include_declaration && name == target_short
                }
                SymbolKind::SelfStaticParent(ssp_kind) => *ssp_kind != SelfStaticParentKind::This,
                _ => false,
            });

            if !has_potential_match {
                continue;
            }

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };

            // Lazily load file content only if we find a true FQN match.
            let mut file_content: Option<Arc<String>> = None;

            for span in &symbol_map.spans {
                let matched = match &span.kind {
                    SymbolKind::ClassReference { name, is_fqn, .. } => {
                        let resolved = if *is_fqn {
                            name.clone()
                        } else if let Some(fqn) =
                            resolved_names.as_ref().and_then(|rn| rn.get(span.start))
                        {
                            fqn.to_string()
                        } else {
                            let use_map = file_use_map.get_or_init(|| {
                                self.file_imports
                                    .read()
                                    .get(file_uri)
                                    .cloned()
                                    .unwrap_or_default()
                            });
                            Self::resolve_to_fqn(name, use_map, &file_namespace)
                        };
                        class_names_match(strip_fqn_prefix(&resolved), target, target_short)
                    }
                    SymbolKind::ClassDeclaration { name } if include_declaration => {
                        if name != target_short {
                            false
                        } else {
                            let fqn = build_fqn(name, file_namespace.as_deref());
                            class_names_match(&fqn, target, target_short)
                        }
                    }
                    SymbolKind::SelfStaticParent(ssp_kind)
                        if *ssp_kind != SelfStaticParentKind::This =>
                    {
                        if let Some(fqn) = self.resolve_keyword_to_fqn(
                            ssp_kind,
                            file_uri,
                            &file_namespace,
                            span.start,
                        ) {
                            class_names_match(&fqn, target, target_short)
                        } else {
                            false
                        }
                    }
                    _ => false,
                };

                if matched {
                    if file_content.is_none() {
                        file_content = self.get_file_content_arc(file_uri);
                    }
                    if let Some(ref content) = file_content {
                        let start = offset_to_position(content, span.start as usize);
                        let end = offset_to_position(content, span.end as usize);
                        locations.push(Location {
                            uri: parsed_uri.clone(),
                            range: Range { start, end },
                        });
                    }
                }
            }
        }

        // Sort: by URI, then by position.
        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        locations.dedup();
        locations
    }

    /// Find all references to a constructor (`__construct`).
    ///
    /// Unlike ordinary methods, constructors are not invoked through
    /// member-access syntax (`$obj->__construct()`); the call sites are
    /// `new ClassName(...)` instantiation expressions plus explicit
    /// `parent::__construct()` / `self::__construct()` style calls.
    ///
    /// `owner_fqns` are the class(es) that declare the constructor under
    /// the cursor.  A `new SubClass()` expression only invokes this
    /// constructor when `SubClass` inherits it (i.e. does not declare its
    /// own), so the search scope is expanded to inheriting descendants and
    /// pruned at overriding ones (see
    /// [`Self::collect_constructor_hierarchy`]).
    fn find_constructor_references(
        &self,
        owner_fqns: &[String],
        include_declaration: bool,
    ) -> Vec<Location> {
        if owner_fqns.is_empty() {
            return Vec::new();
        }

        // Expand the owners to the set of classes whose instantiation
        // invokes this same constructor (inheriting descendants), pruning
        // at descendants that override it.
        let scoped = self.collect_constructor_hierarchy(owner_fqns);
        if scoped.is_empty() {
            return Vec::new();
        }

        let mut locations = Vec::new();
        let snapshot = self.user_file_symbol_maps();

        for (file_uri, symbol_map) in &snapshot {
            let resolved_names = self.resolved_names.read().get(file_uri).cloned();
            let file_namespace = self.first_file_namespace(file_uri);
            let file_use_map = std::cell::OnceCell::new();
            let file_ctx = std::cell::OnceCell::new();

            let Some(parsed_uri) = Url::parse(file_uri).ok() else {
                continue;
            };

            let mut file_content: Option<Arc<String>> = None;

            for span in &symbol_map.spans {
                let matched = match &span.kind {
                    // `new ClassName(...)` carries `ClassRefContext::New`;
                    // `#[ClassName(...)]` attribute usages carry
                    // `ClassRefContext::Attribute`.  Both invoke the
                    // constructor.
                    SymbolKind::ClassReference {
                        name,
                        is_fqn,
                        context: ClassRefContext::New | ClassRefContext::Attribute,
                    } => {
                        let resolved = if *is_fqn {
                            name
                        } else if let Some(fqn) =
                            resolved_names.as_ref().and_then(|rn| rn.get(span.start))
                        {
                            fqn
                        } else {
                            let use_map = file_use_map.get_or_init(|| {
                                self.file_imports
                                    .read()
                                    .get(file_uri)
                                    .cloned()
                                    .unwrap_or_default()
                            });
                            &Self::resolve_to_fqn(name, use_map, &file_namespace)
                        };
                        scoped.contains(&normalize_fqn(strip_fqn_prefix(resolved)))
                    }
                    // Explicit constructor delegation written as
                    // `parent::__construct()`, `self::__construct()`, or
                    // `Foo::__construct()` lands here.  Resolve the subject
                    // class and keep the call when it falls within the
                    // constructor's owning hierarchy.
                    SymbolKind::MemberAccess {
                        subject_text,
                        member_name,
                        is_static,
                        ..
                    } if is_constructor_name(member_name) => {
                        if file_content.is_none() {
                            file_content = self.get_file_content_arc(file_uri);
                        }
                        match &file_content {
                            Some(content) => {
                                let ctx = file_ctx.get_or_init(|| self.file_context(file_uri));
                                self.resolve_subject_to_fqns(
                                    subject_text,
                                    *is_static,
                                    ctx,
                                    span.start,
                                    content,
                                )
                                .iter()
                                .any(|fqn| scoped.contains(&normalize_fqn(strip_fqn_prefix(fqn))))
                            }
                            None => false,
                        }
                    }
                    _ => false,
                };

                if matched {
                    if file_content.is_none() {
                        file_content = self.get_file_content_arc(file_uri);
                    }
                    if let Some(content) = &file_content {
                        let start = offset_to_position(content, span.start as usize);
                        let end = offset_to_position(content, span.end as usize);
                        push_unique_location(&mut locations, &parsed_uri, start, end);
                    }
                }
            }

            // Optionally include the constructor declaration site(s).
            if include_declaration && let Some(classes) = self.get_classes_for_uri(file_uri) {
                for class in &classes {
                    let class_fqn = normalize_fqn(&class.fqn()).to_string();
                    if !scoped.contains(&class_fqn) {
                        continue;
                    }

                    for method in class.methods.iter() {
                        if is_constructor_name(&method.name) && method.name_offset != 0 {
                            if file_content.is_none() {
                                file_content = self.get_file_content_arc(file_uri);
                            }
                            let Some(content) = &file_content else {
                                break;
                            };
                            let offset = method.name_offset as usize;
                            let start = offset_to_position(content, offset);
                            let end = offset_to_position(content, offset + method.name.len());
                            push_unique_location(&mut locations, &parsed_uri, start, end);
                        }
                    }
                }
            }
        }

        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });
        locations.dedup();
        locations
    }

    /// Expand the constructor owner class(es) into the full set of classes
    /// whose instantiation (`new X(...)`) invokes the same constructor.
    ///
    /// Starting from `owner_fqns` (the class(es) that declare the
    /// constructor under the cursor), walk down the inheritance tree and
    /// include every descendant that does *not* declare its own
    /// constructor (those inherit the owner's), pruning the walk at any
    /// descendant that overrides it.
    fn collect_constructor_hierarchy(&self, owner_fqns: &[String]) -> HashSet<String> {
        let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };
        let declares_ctor = |fqn: &str| -> bool {
            class_loader(fqn)
                .map(|c| c.methods.iter().any(|m| is_constructor_name(&m.name)))
                .unwrap_or(false)
        };

        let owners: Vec<String> = owner_fqns.iter().map(|f| normalize_fqn(f)).collect();
        let mut result: HashSet<String> = owners.iter().cloned().collect();

        // Walk down from each owner, including inheriting descendants and
        // pruning at overrides.
        let gti = self.gti_index.read();
        let mut queue: std::collections::VecDeque<String> = owners.iter().cloned().collect();
        let mut seen: HashSet<String> = owners.iter().cloned().collect();
        while let Some(fqn) = queue.pop_front() {
            if let Some(descendants) = gti.get(&fqn) {
                for desc in descendants {
                    let normalized = normalize_fqn(desc).to_string();
                    if !seen.insert(normalized.clone()) {
                        continue;
                    }
                    // A descendant that declares its own constructor uses a
                    // different constructor — exclude it and stop walking
                    // past it.
                    if declares_ctor(&normalized) {
                        continue;
                    }
                    result.insert(normalized.clone());
                    queue.push_back(normalized);
                }
            }
        }

        result
    }

    /// Find all references to a member (method, property, or constant)
    /// across all files.
    ///
    /// When `hierarchy` is `Some`, only references where the subject
    /// resolves to a class in the given set of FQNs are returned.  When
    /// the subject cannot be resolved (e.g. a complex expression or an
    /// untyped variable), the reference is conservatively included.
    ///
    /// When `hierarchy` is `None`, all references with a matching member
    /// name and static-ness are returned (the v1 behaviour, kept as a
    /// fallback when the target class cannot be determined).
    fn find_member_references(
        &self,
        target_member: &str,
        target_is_static: bool,
        include_declaration: bool,
        hierarchy: Option<&HashSet<String>>,
    ) -> Vec<Location> {
        let mut locations = Vec::new();

        let snapshot = self.user_file_symbol_maps();

        for (file_uri, symbol_map) in &snapshot {
            // First pass: name-only check to avoid unnecessary work.
            // When a hierarchy is present (e.g. Laravel), we allow static mismatch.
            let has_potential_match = symbol_map.spans.iter().any(|span| match &span.kind {
                SymbolKind::MemberAccess {
                    member_name,
                    is_static,
                    ..
                } if member_name == target_member => {
                    hierarchy.is_some() || *is_static == target_is_static
                }
                SymbolKind::MemberDeclaration { name, is_static }
                    if include_declaration && name == target_member =>
                {
                    hierarchy.is_some() || *is_static == target_is_static
                }
                _ => false,
            });

            // Special check for property declarations in ClassInfo (represented as Variable spans)
            let mut check_ast_map = false;
            if !has_potential_match
                && include_declaration
                && let Some(classes) = self.get_classes_for_uri(file_uri)
            {
                for class in &classes {
                    for prop in &class.properties {
                        let prop_name = prop.name.strip_prefix('$').unwrap_or(&prop.name);
                        let target_name = target_member.strip_prefix('$').unwrap_or(target_member);
                        if prop_name == target_name && prop.is_static == target_is_static {
                            check_ast_map = true;
                            break;
                        }
                    }
                    if check_ast_map {
                        break;
                    }
                }
            }

            if !has_potential_match && !check_ast_map {
                continue;
            }

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let mut file_content: Option<Arc<String>> = None;

            // Lazily resolved file context — only computed when we need
            // to check a candidate's subject against the hierarchy.
            let file_ctx_cell: std::cell::OnceCell<crate::types::FileContext> =
                std::cell::OnceCell::new();

            for span in &symbol_map.spans {
                match &span.kind {
                    SymbolKind::MemberAccess {
                        subject_text,
                        member_name,
                        is_static,
                        ..
                    } if member_name == target_member => {
                        // For Laravel custom builders, we allow static-ness mismatch
                        // (Model::active() is static, UserBuilder->active() is instance).
                        if *is_static != target_is_static {
                            // Only allow mismatch if we have a hierarchy to verify
                            // that they are indeed related (one is Model, one is Builder).
                            if hierarchy.is_none() {
                                continue;
                            }
                        }

                        // Check if the subject belongs to the target hierarchy.
                        if let Some(hier) = hierarchy {
                            if file_content.is_none() {
                                file_content = self.get_file_content_arc(file_uri);
                            }
                            let Some(ref content) = file_content else {
                                break;
                            };

                            let ctx = file_ctx_cell.get_or_init(|| self.file_context(file_uri));
                            let subject_fqns = self.resolve_subject_to_fqns(
                                subject_text,
                                *is_static,
                                ctx,
                                span.start,
                                content,
                            );
                            if !subject_fqns.is_empty()
                                && !subject_fqns.iter().any(|fqn| hier.contains(fqn))
                            {
                                // Subject resolved but none of the resolved
                                // classes are in the target hierarchy — skip.
                                continue;
                            }
                            // If subject_fqns is empty, we couldn't resolve
                            // the subject — include conservatively.
                        }

                        if file_content.is_none() {
                            file_content = self.get_file_content_arc(file_uri);
                        }
                        let Some(ref content) = file_content else {
                            break;
                        };

                        let start = offset_to_position(content, span.start as usize);
                        let end = offset_to_position(content, span.end as usize);
                        locations.push(Location {
                            uri: parsed_uri.clone(),
                            range: Range { start, end },
                        });
                    }
                    SymbolKind::MemberDeclaration { name, is_static }
                        if include_declaration && name == target_member =>
                    {
                        if *is_static != target_is_static && hierarchy.is_none() {
                            continue;
                        }

                        // Check if the enclosing class is in the hierarchy.
                        if let Some(hier) = hierarchy {
                            let ctx = file_ctx_cell.get_or_init(|| self.file_context(file_uri));
                            let enclosing =
                                find_class_at_offset(&ctx.classes, span.start).or_else(|| {
                                    // Docblock MemberDeclaration spans are before the
                                    // opening brace; fall back to the nearest class.
                                    ctx.classes
                                        .iter()
                                        .map(|c| c.as_ref())
                                        .filter(|c| {
                                            c.keyword_offset > 0 && span.start < c.start_offset
                                        })
                                        .min_by_key(|c| c.start_offset)
                                });
                            if let Some(enclosing) = enclosing {
                                let fqn = enclosing.fqn().to_string();
                                if !hier.contains(&fqn) {
                                    continue;
                                }
                            }
                        }

                        if file_content.is_none() {
                            file_content = self.get_file_content_arc(file_uri);
                        }
                        let Some(ref content) = file_content else {
                            break;
                        };

                        let start = offset_to_position(content, span.start as usize);
                        let end = offset_to_position(content, span.end as usize);
                        locations.push(Location {
                            uri: parsed_uri.clone(),
                            range: Range { start, end },
                        });
                    }
                    _ => {}
                }
            }

            // Property declarations use Variable spans (not
            // MemberDeclaration) because GTD relies on the Variable
            // kind to jump to the type hint.  Scan the uri_classes_index to
            // pick up property declaration sites.
            if include_declaration && let Some(classes) = self.get_classes_for_uri(file_uri) {
                for class in &classes {
                    // Filter by hierarchy when available.
                    if let Some(hier) = hierarchy {
                        let class_fqn = class.fqn().to_string();
                        if !hier.contains(&class_fqn) {
                            continue;
                        }
                    }

                    for prop in &class.properties {
                        let prop_name = prop.name.strip_prefix('$').unwrap_or(&prop.name);
                        let target_name = target_member.strip_prefix('$').unwrap_or(target_member);
                        if prop_name == target_name
                            && prop.is_static == target_is_static
                            && prop.name_offset != 0
                        {
                            if file_content.is_none() {
                                file_content = self.get_file_content_arc(file_uri);
                            }
                            let Some(ref content) = file_content else {
                                break;
                            };

                            // `name_offset` points at the `$` sigil while
                            // `prop.name` excludes it, so the range must span
                            // the `$` plus the name (`$name`, not `$nam`).
                            let offset = prop.name_offset;
                            let start = offset_to_position(content, offset as usize);
                            let end =
                                offset_to_position(content, offset as usize + 1 + prop.name.len());
                            push_unique_location(&mut locations, &parsed_uri, start, end);
                        }
                    }
                }
            }
        }

        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        locations
    }

    /// Find all references to a function across all files.
    fn find_function_references(
        &self,
        target_fqn: &str,
        target_short: &str,
        include_declaration: bool,
    ) -> Vec<Location> {
        let mut locations = Vec::new();

        // Input boundary: callers may pass FQNs with a leading `\`.
        let target = strip_fqn_prefix(target_fqn);

        let snapshot = self.user_file_symbol_maps();

        for (file_uri, symbol_map) in &snapshot {
            // Prefer mago-names resolved_names; lazy-load use_map only
            // when an offset is not tracked (e.g. docblock references).
            let resolved_names = self.resolved_names.read().get(file_uri).cloned();
            let file_namespace = self.first_file_namespace(file_uri);
            let file_use_map = std::cell::OnceCell::new();

            // First pass: resolved-name check. Function imports can be aliased
            // (`use function Foo\bar as baz; baz()`), so the call-site text
            // alone is not enough to decide whether this file can match.
            let has_potential_match = symbol_map.spans.iter().any(|span| {
                if let SymbolKind::FunctionCall { name, .. } = &span.kind {
                    if name == target_short {
                        true
                    } else {
                        let resolved = if let Some(fqn) =
                            resolved_names.as_ref().and_then(|rn| rn.get(span.start))
                        {
                            fqn.to_string()
                        } else {
                            let use_map = file_use_map.get_or_init(|| {
                                self.file_imports
                                    .read()
                                    .get(file_uri)
                                    .cloned()
                                    .unwrap_or_default()
                            });
                            Self::resolve_to_fqn(name, use_map, &file_namespace)
                        };
                        let resolved_normalized = strip_fqn_prefix(&resolved);
                        resolved_normalized == target
                            || crate::util::short_name(resolved_normalized) == target_short
                    }
                } else {
                    false
                }
            });

            if !has_potential_match {
                continue;
            }

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let mut file_content: Option<Arc<String>> = None;

            for span in &symbol_map.spans {
                if let SymbolKind::FunctionCall {
                    name,
                    is_definition,
                } = &span.kind
                {
                    if *is_definition && !include_declaration {
                        continue;
                    }

                    let resolved = if let Some(fqn) =
                        resolved_names.as_ref().and_then(|rn| rn.get(span.start))
                    {
                        fqn.to_string()
                    } else {
                        let use_map = file_use_map.get_or_init(|| {
                            self.file_imports
                                .read()
                                .get(file_uri)
                                .cloned()
                                .unwrap_or_default()
                        });
                        Self::resolve_to_fqn(name, use_map, &file_namespace)
                    };

                    // Input boundary: resolve_to_fqn may return a leading `\`.
                    let resolved_normalized = strip_fqn_prefix(&resolved);
                    if resolved_normalized == target
                        || crate::util::short_name(resolved_normalized) == target_short
                    {
                        if file_content.is_none() {
                            file_content = self.get_file_content_arc(file_uri);
                        }
                        if let Some(ref content) = file_content {
                            let start = offset_to_position(content, span.start as usize);
                            let end = offset_to_position(content, span.end as usize);
                            locations.push(Location {
                                uri: parsed_uri.clone(),
                                range: Range { start, end },
                            });
                        }
                    }
                }
            }
        }

        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        locations
    }

    /// Find all references to a constant across all files.
    fn find_constant_references(
        &self,
        target_name: &str,
        include_declaration: bool,
    ) -> Vec<Location> {
        let mut locations = Vec::new();

        let snapshot = self.user_file_symbol_maps();

        for (file_uri, symbol_map) in &snapshot {
            // First pass: name-only check.
            let has_potential_match = symbol_map.spans.iter().any(|span| match &span.kind {
                SymbolKind::ConstantReference { name } => name == target_name,
                SymbolKind::MemberDeclaration { name, is_static }
                    if include_declaration && name == target_name && *is_static =>
                {
                    true
                }
                _ => false,
            });

            if !has_potential_match {
                continue;
            }

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let mut file_content: Option<Arc<String>> = None;

            for span in &symbol_map.spans {
                let matched = match &span.kind {
                    SymbolKind::ConstantReference { name } => name == target_name,
                    SymbolKind::MemberDeclaration { name, is_static }
                        if include_declaration && name == target_name && *is_static =>
                    {
                        true
                    }
                    _ => false,
                };

                if matched {
                    if file_content.is_none() {
                        file_content = self.get_file_content_arc(file_uri);
                    }
                    if let Some(ref content) = file_content {
                        let start = offset_to_position(content, span.start as usize);
                        let end = offset_to_position(content, span.end as usize);
                        push_unique_location(&mut locations, &parsed_uri, start, end);
                    }
                }
            }
        }

        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        locations
    }

    fn resolve_keyword_to_fqn(
        &self,
        ssp_kind: &SelfStaticParentKind,
        uri: &str,
        namespace: &Option<String>,
        offset: u32,
    ) -> Option<String> {
        let classes: Vec<Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        let current_class = crate::util::find_class_at_offset(&classes, offset)?;

        match ssp_kind {
            SelfStaticParentKind::Parent => current_class.parent_class.map(|a| a.to_string()),
            _ => {
                // self / static → current class FQN
                Some(build_fqn(&current_class.name, namespace.as_deref()))
            }
        }
    }

    // ─── Class hierarchy resolution for member references ───────────────────

    /// Resolve the class hierarchy for a `MemberAccess` subject.
    ///
    /// Returns `Some(set_of_fqns)` when the subject can be resolved to at
    /// least one class, or `None` when resolution fails entirely.
    fn resolve_member_access_hierarchy(
        &self,
        uri: &str,
        subject_text: &str,
        is_static: bool,
        span_start: u32,
        member_name: &str,
    ) -> Option<HashSet<String>> {
        let ctx = self.file_context(uri);
        let content = self.get_file_content(uri)?;
        let fqns =
            self.resolve_subject_to_fqns(subject_text, is_static, &ctx, span_start, &content);
        if fqns.is_empty() {
            return None;
        }
        Some(self.collect_hierarchy_for_fqns(&fqns, Some(member_name), is_static))
    }

    /// Resolve the class hierarchy for a `MemberDeclaration` at a given offset.
    ///
    /// Finds the enclosing class and builds the hierarchy set from it.
    fn resolve_member_declaration_hierarchy(
        &self,
        uri: &str,
        offset: u32,
        member_name: &str,
        is_static: bool,
    ) -> Option<HashSet<String>> {
        let classes: Vec<Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let current_class = find_class_at_offset(&classes, offset).or_else(|| {
            // Fallback: offset may be in a class docblock (before the opening
            // brace).  Find the nearest class whose body starts past the
            // offset, meaning its docblock region likely contains the offset.
            classes
                .iter()
                .map(|c| c.as_ref())
                .filter(|c| c.keyword_offset > 0 && offset < c.start_offset)
                .min_by_key(|c| c.start_offset)
        })?;
        let fqn = current_class.fqn().to_string();
        Some(self.collect_hierarchy_for_fqns(&[fqn], Some(member_name), is_static))
    }

    /// Resolve a member access subject to zero or more class FQNs.
    ///
    /// This is a lightweight resolution path used during reference scanning.
    /// It handles the common cases (`self`, `static`, `$this`, `parent`,
    /// Resolve a member-access subject to the FQN(s) of its type(s)
    /// using the shared subject resolution utility.
    fn resolve_subject_to_fqns(
        &self,
        subject_text: &str,
        is_static: bool,
        ctx: &crate::types::FileContext,
        access_offset: u32,
        content: &str,
    ) -> Vec<String> {
        let class_loader = self.class_loader(ctx);
        let function_loader = self.function_loader(ctx);
        let use_map = &ctx.use_map;
        let namespace = &ctx.namespace;
        let resolution_ctx = crate::subject_resolution::SubjectResolutionCtx {
            local_classes: &ctx.classes,
            use_map,
            namespace,
            content,
            class_loader: &class_loader,
            function_loader: &function_loader,
        };

        match crate::subject_resolution::resolve_subject_type(
            subject_text,
            is_static,
            access_offset,
            &resolution_ctx,
        ) {
            Some(php_type) => php_type
                .top_level_class_names()
                .into_iter()
                .map(|n| {
                    let normalized = normalize_fqn(&n);
                    // top_level_class_names() may return short names
                    // (e.g. "BlogAuthor" instead of
                    // "App\Models\BlogAuthor").  Resolve them through
                    // the file's use-map and namespace so they match
                    // the FQNs used in the hierarchy set.
                    if normalized.contains('\\') {
                        normalized.to_string()
                    } else {
                        normalize_fqn(&Self::resolve_to_fqn(&normalized, use_map, namespace))
                            .to_string()
                    }
                })
                .collect(),
            None => Vec::new(),
        }
    }

    /// Collect the full class hierarchy (ancestors and descendants) for
    /// a set of starting FQNs.
    ///
    /// The result includes:
    /// - The starting FQNs themselves
    /// - All ancestor FQNs (parent chain, interfaces, traits)
    /// - All descendant FQNs (classes that extend/implement any class in
    ///   the hierarchy)
    fn collect_hierarchy_for_fqns(
        &self,
        seed_fqns: &[String],
        member_name: Option<&str>,
        is_static: bool,
    ) -> HashSet<String> {
        let mut hierarchy = HashSet::new();
        let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };

        // Insert the seeds.
        for fqn in seed_fqns {
            hierarchy.insert(normalize_fqn(fqn).to_string());
        }

        // Walk up: collect all ancestors for each seed.
        let seeds: Vec<String> = hierarchy.iter().cloned().collect();
        for fqn in seeds {
            self.collect_ancestors(&fqn, &class_loader, &mut hierarchy);
        }

        // Bridge Laravel Models and their Custom Builders.
        // If a class in the hierarchy is a Model with a custom builder,
        // add that builder to the hierarchy.
        let mut extensions = Vec::new();
        for fqn in &hierarchy {
            if let Some(cls) = class_loader(fqn)
                && let Some(builder_fqn) = cls
                    .laravel()
                    .and_then(|l| l.custom_builder.as_ref())
                    .and_then(|b| b.base_name())
            {
                extensions.push(normalize_fqn(builder_fqn).to_string());
            }
        }
        for ext_fqn in extensions {
            if hierarchy.insert(ext_fqn.clone()) {
                self.collect_ancestors(&ext_fqn, &class_loader, &mut hierarchy);
            }
        }

        // Bridge Laravel Builders back to their Models.
        // If a class in the hierarchy is a custom builder, find which models
        // use it and add them to the hierarchy.
        let mut model_seeds = Vec::new();
        {
            let class_index = self.fqn_class_index.read();
            for (class_fqn, class_info) in class_index.iter() {
                if let Some(laravel) = class_info.laravel() {
                    if let Some(normalized) = laravel
                        .custom_builder
                        .as_ref()
                        .and_then(|b| b.base_name())
                        .map(normalize_fqn)
                    {
                        if hierarchy.contains(normalized.as_str()) {
                            model_seeds.push(class_fqn.clone());
                        }
                    } else if hierarchy
                        .contains(crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN)
                    {
                        // All models use the base Eloquent Builder by default.
                        model_seeds.push(class_fqn.clone());
                    }
                }
            }
        }
        for model_fqn in model_seeds {
            if hierarchy.insert(normalize_fqn(&model_fqn).to_string()) {
                self.collect_ancestors(&model_fqn, &class_loader, &mut hierarchy);
            }
        }

        // Walk down: collect all descendants using the global GTI index.
        // We only walk down from a class if it actually defines the member
        // (or if no member name was provided).
        let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        if let Some(name) = member_name {
            for fqn in &hierarchy {
                if self.defines_member(fqn, name, is_static, &class_loader) {
                    queue.push_back(fqn.clone());
                }
            }
        } else {
            queue.extend(hierarchy.iter().cloned());
        }

        let gti = self.gti_index.read();
        while let Some(fqn) = queue.pop_front() {
            if let Some(descendants) = gti.get(&fqn) {
                for desc in descendants {
                    let normalized = normalize_fqn(desc).to_string();
                    if hierarchy.insert(normalized.clone()) {
                        queue.push_back(normalized);
                    }
                }
            }
        }

        hierarchy
    }

    fn defines_member(
        &self,
        fqn: &str,
        name: &str,
        is_static: bool,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool {
        let Some(cls) = class_loader(fqn) else {
            return false;
        };

        // Real methods
        if cls
            .methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(name) && m.is_static == is_static)
        {
            return true;
        }

        // Laravel forwarded methods
        if let Some(laravel) = cls.laravel() {
            if let Some(builder_cls) = laravel
                .custom_builder
                .as_ref()
                .and_then(|b| b.base_name())
                .and_then(class_loader)
            {
                // Forwarded methods are instance methods on the builder
                // but called statically on the model.
                if builder_cls
                    .methods
                    .iter()
                    .any(|m| m.name.eq_ignore_ascii_case(name) && (!is_static || !m.is_static))
                {
                    return true;
                }
            }
            // Standard builder forwarding
            if class_loader(crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN)
                .filter(|bc| {
                    bc.methods
                        .iter()
                        .any(|m| m.name.eq_ignore_ascii_case(name) && (!is_static || !m.is_static))
                })
                .is_some()
            {
                return true;
            }
        }

        false
    }

    /// Walk up the inheritance chain and collect all ancestor FQNs.
    fn collect_ancestors(
        &self,
        fqn: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        hierarchy: &mut HashSet<String>,
    ) {
        let cls = match class_loader(fqn) {
            Some(c) => c,
            None => return,
        };

        // Parent class chain.
        if let Some(ref parent) = cls.parent_class {
            let parent_fqn = normalize_fqn(parent);
            if hierarchy.insert(parent_fqn.clone()) {
                self.collect_ancestors(&parent_fqn, class_loader, hierarchy);
            }
        }

        // Interfaces.
        for iface in &cls.interfaces {
            let iface_fqn = normalize_fqn(iface);
            if hierarchy.insert(iface_fqn.clone()) {
                self.collect_ancestors(&iface_fqn, class_loader, hierarchy);
            }
        }

        // Used traits.
        for trait_name in &cls.used_traits {
            let trait_fqn = normalize_fqn(trait_name);
            if hierarchy.insert(trait_fqn.clone()) {
                self.collect_ancestors(&trait_fqn, class_loader, hierarchy);
            }
        }

        // Mixins.
        for mixin in &cls.mixins {
            let mixin_fqn = normalize_fqn(mixin);
            if hierarchy.insert(mixin_fqn.clone()) {
                self.collect_ancestors(&mixin_fqn, class_loader, hierarchy);
            }
        }
    }

    /// Ensure all workspace PHP files have been parsed and have symbol maps.
    ///
    /// This lazily parses files that are in the workspace directory but
    /// have not been opened or indexed yet.  It also covers files known
    /// via the fqn_uri_index.  The vendor directory (read from
    /// skipped during the filesystem walk.
    pub(crate) fn ensure_workspace_indexed(&self) {
        let start = std::time::Instant::now();
        // Collect URIs that already have symbol maps.
        let existing_uris: HashSet<String> = self.symbol_maps.read().keys().cloned().collect();

        // Build the vendor URI prefixes so we can skip vendor files in
        // Phase 1 (fqn_uri_index may contain vendor URIs from prior
        // resolution, but we only need symbol maps for user files).
        let vendor_prefixes = self.vendor_uri_prefixes.lock().clone();

        // ── Phase 1: fqn_uri_index files (user only) ─────────────────────
        let index_uris: Vec<String> = self.fqn_uri_index.read().values().cloned().collect();

        let phase1_uris: Vec<&String> = index_uris
            .iter()
            .filter(|uri| {
                !existing_uris.contains(*uri)
                    && !vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str()))
                    && !uri.starts_with("phpantom-stub://")
                    && !uri.starts_with("phpantom-stub-fn://")
            })
            .collect();

        if !phase1_uris.is_empty() {
            tracing::info!(
                "ensure_workspace_indexed: Phase 1 parsing {} files",
                phase1_uris.len()
            );
            self.parse_files_parallel(
                phase1_uris
                    .iter()
                    .map(|uri| (uri.as_str(), None::<&str>))
                    .collect(),
            );
        }

        // ── Phase 2: workspace directory scan ───────────────────────────
        //
        // Even after the initial scan, repeat the walk so newly-created PHP
        // files that are not open in the editor can still be discovered.
        // The existing-URI filter below keeps this cheap by parsing only files
        // that are not already in `symbol_maps`.
        let has_scanned_workspace = self
            .workspace_indexed
            .load(std::sync::atomic::Ordering::Relaxed);

        let workspace_root = self.workspace_root.read().clone();

        if let Some(root) = workspace_root {
            let vendor_dir_paths = self.vendor_dir_paths.lock().clone();

            // Re-read existing URIs after phase 1 may have added more.
            let existing_uris: HashSet<String> = self.symbol_maps.read().keys().cloned().collect();

            let walk_start = std::time::Instant::now();
            let php_files = collect_php_files_gitignore(&root, &vendor_dir_paths);
            tracing::info!(
                "ensure_workspace_indexed: Phase 2 {} found {} PHP files in {:?}",
                if has_scanned_workspace {
                    "refresh disk walk"
                } else {
                    "disk walk"
                },
                php_files.len(),
                walk_start.elapsed()
            );

            let phase2_work: Vec<(String, PathBuf)> = php_files
                .into_iter()
                .filter_map(|path| {
                    let uri = crate::util::path_to_uri(&path);
                    if existing_uris.contains(&uri) {
                        None
                    } else {
                        Some((uri, path))
                    }
                })
                .collect();

            if !phase2_work.is_empty() {
                tracing::info!(
                    "ensure_workspace_indexed: Phase 2 parsing {} files",
                    phase2_work.len()
                );
                self.parse_paths_parallel(&phase2_work);
            }
            self.workspace_indexed
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        tracing::info!("ensure_workspace_indexed: total time {:?}", start.elapsed());
    }

    /// Parse a batch of files in parallel using OS threads.
    ///
    /// Each entry is `(uri, optional_content)`.  When `content` is `None`,
    /// the file is loaded via [`get_file_content`].  The expensive parsing
    /// step runs without any locks held; only the brief map insertions at
    /// the end of [`update_ast`] acquire write locks.
    ///
    /// Uses [`std::thread::scope`] for structured concurrency so that all
    /// spawned threads are guaranteed to finish before this method returns.
    /// The thread count is capped at the number of available CPU cores.
    fn parse_files_parallel(&self, files: Vec<(&str, Option<&str>)>) {
        if files.is_empty() {
            return;
        }

        // For very small batches, avoid thread overhead.
        if files.len() <= 2 {
            for (uri, content) in &files {
                if let Some(c) = content {
                    self.update_ast(uri, c);
                } else if let Some(c) = self.get_file_content(uri) {
                    self.update_ast(uri, &c);
                }
            }
            return;
        }

        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(files.len());

        let chunks: Vec<Vec<(&str, Option<&str>)>> = {
            let chunk_size = files.len().div_ceil(n_threads);
            files.chunks(chunk_size).map(|c| c.to_vec()).collect()
        };

        // Use a 16 MB stack per thread.  The default 8 MB can overflow
        // when parsing deeply-nested PHP files (e.g. WordPress
        // admin-bar.php) because `extract_symbol_map` recurses through
        // the full AST via `extract_from_expression` /
        // `extract_from_statement`.  Stack overflows are fatal
        // (abort, not panic) so `catch_unwind` cannot save us.
        const PARSE_STACK_SIZE: usize = 16 * 1024 * 1024;

        std::thread::scope(|s| {
            for chunk in &chunks {
                let handle = std::thread::Builder::new()
                    .stack_size(PARSE_STACK_SIZE)
                    .spawn_scoped(s, move || {
                        for (uri, content) in chunk {
                            if let Some(c) = content {
                                self.update_ast(uri, c);
                            } else if let Some(c) = self.get_file_content(uri) {
                                self.update_ast(uri, &c);
                            }
                        }
                    });
                if let Err(e) = handle {
                    tracing::error!("failed to spawn parse thread: {e}");
                }
            }
        });
    }

    /// Parse a batch of files from disk paths in parallel.
    ///
    /// Each entry is `(uri, path)`.  The file is read from disk and
    /// parsed in a worker thread.  Uses [`std::thread::scope`] for
    /// structured concurrency.
    pub(crate) fn parse_paths_parallel(&self, files: &[(String, PathBuf)]) {
        if files.is_empty() {
            return;
        }

        // For very small batches, avoid thread overhead.
        if files.len() <= 2 {
            for (uri, path) in files {
                if let Ok(content) = std::fs::read_to_string(path) {
                    self.update_ast(uri, &content);
                }
            }
            return;
        }

        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(files.len());

        let chunks: Vec<&[(String, PathBuf)]> = {
            let chunk_size = files.len().div_ceil(n_threads);
            files.chunks(chunk_size).collect()
        };

        const PARSE_STACK_SIZE: usize = 16 * 1024 * 1024;

        std::thread::scope(|s| {
            for chunk in &chunks {
                let handle = std::thread::Builder::new()
                    .stack_size(PARSE_STACK_SIZE)
                    .spawn_scoped(s, move || {
                        for (uri, path) in *chunk {
                            if let Ok(content) = std::fs::read_to_string(path) {
                                self.update_ast(uri, &content);
                            }
                        }
                    });
                if let Err(e) = handle {
                    tracing::error!("failed to spawn parse thread: {e}");
                }
            }
        });
    }
}

/// Normalise a class FQN: strip leading `\` if present.
fn normalize_fqn(fqn: &str) -> String {
    strip_fqn_prefix(fqn).to_string()
}

/// Whether a member name is the PHP constructor (`__construct`).
///
/// PHP method names are case-insensitive, so `__CONSTRUCT` matches too.
fn is_constructor_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("__construct")
}

/// Check whether a resolved class name matches the target FQN.
///
/// Two names match if their fully-qualified forms are equal, or if both
/// are unqualified and their short names match.
fn class_names_match(resolved: &str, target: &str, target_short: &str) -> bool {
    if resolved == target {
        return true;
    }
    // When neither name is qualified, compare short names.
    if !resolved.contains('\\') && !target.contains('\\') {
        return resolved == target_short;
    }
    // When the resolved name is unqualified but the target is
    // namespace-qualified, the resolved name might be a short-name
    // reference to the target class (e.g. `Request` referencing
    // `Illuminate\Http\Request` via a `use` import that was not
    // tracked in the resolved-names map).  Accept the match only
    // when the short names agree.
    //
    // The reverse (resolved is qualified, target is unqualified) is
    // NOT accepted: `App\Helper` is a different class from a global
    // `Helper`, so matching by short name alone would produce false
    // positives.
    if !resolved.contains('\\') && target.contains('\\') {
        return resolved == target_short;
    }
    false
}

#[cfg(test)]
mod tests;
