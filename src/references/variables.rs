//! Variable and `$this` reference finders.
//!
//! Variables are file-local and scope-local; `$this` is scoped to the
//! enclosing class body.  Both walk the current file's [`SymbolMap`]
//! rather than the cross-file snapshot used by the other finders.

use super::*;

use std::collections::HashMap;

use tower_lsp::lsp_types::{Location, Range};

use crate::symbol_map::{SelfStaticParentKind, SymbolKind, VarDefKind};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::ClassInfo;

impl Backend {
    /// Find all references to a variable within its enclosing scope.
    ///
    /// Variables are file-local and scope-local — a `$user` in method A
    /// must not match `$user` in method B.
    pub(super) fn find_variable_references(
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
            Err(e) => {
                tracing::warn!("find-references: could not parse URI {uri:?}: {e}");
                return locations;
            }
        };

        // Build the set of reachable scopes: the primary (declaring)
        // scope plus every nested closure/arrow-function scope that
        // can see the variable (via explicit `use` or implicit arrow
        // capture) without being shadowed.
        let reachable_scopes = Self::collect_capture_scopes(symbol_map, var_name, scope_start);

        for span in &symbol_map.spans {
            let name = match &span.kind {
                SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => name,
                _ => continue,
            };
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
                if let SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } =
                    &s.kind
                {
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
    pub(super) fn find_this_references(
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
        let current_class = crate::class_lookup::find_class_at_offset(&ctx_classes, cursor_offset);
        let (class_start, class_end) = match current_class {
            Some(cc) => (cc.start_offset, cc.end_offset),
            None => return locations,
        };

        let parsed_uri = match Url::parse(uri) {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!("find-references: could not parse URI {uri:?}: {e}");
                return locations;
            }
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
}
