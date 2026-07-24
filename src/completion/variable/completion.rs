/// Variable name completion.
///
/// This module handles building completion items for variable names (`$`
/// prefix) by collecting variable definitions from the precomputed
/// [`SymbolMap`] visible at the cursor position, respecting PHP scoping
/// rules (function, method, closure, and top-level scope).
use std::collections::HashSet;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolMap;
use crate::symbol_map::VarDefKind;
use crate::text_position::position_to_byte_offset;

impl Backend {
    /// PHP superglobal variable names (always available in any scope).
    const SUPERGLOBALS: &'static [&'static str] = &[
        "$_GET",
        "$_POST",
        "$_REQUEST",
        "$_SESSION",
        "$_COOKIE",
        "$_SERVER",
        "$_FILES",
        "$_ENV",
        "$GLOBALS",
        "$argc",
        "$argv",
    ];

    /// Maximum number of variable completions to return.
    const MAX_VARIABLE_COMPLETIONS: usize = 100;

    /// Extract the partial variable name (including `$`) that the user
    /// is currently typing at the given cursor position.
    ///
    /// Walks backward from the cursor through alphanumeric characters and
    /// underscores, then checks for a preceding `$`.  Returns `None` if
    /// no `$` is found or the result is just `"$"` with no identifier
    /// characters.
    ///
    /// Examples:
    ///   - `$us|`  → `Some("$us")`
    ///   - `$_SE|` → `Some("$_SE")`
    ///   - `$|`    → `Some("$")`  (bare dollar — show all variables)
    ///   - `foo|`  → `None`
    pub fn extract_partial_variable_name(content: &str, position: Position) -> Option<String> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return None;
        }

        // When the cursor is past the last line (editor can send this for
        // a trailing blank line after the final newline), treat it as the
        // end of the last line so variables defined earlier are still found.
        let (line, col) = if position.line as usize >= lines.len() {
            let last = lines[lines.len() - 1];
            (last, last.chars().count())
        } else {
            let l = lines[position.line as usize];
            (l, (position.character as usize).min(l.chars().count()))
        };

        let chars: Vec<char> = line.chars().collect();

        // Walk backwards through identifier characters
        let mut i = col;
        while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            i -= 1;
        }

        // Must be preceded by `$`
        if i == 0 || chars[i - 1] != '$' {
            return None;
        }
        // Include the `$`
        i -= 1;

        // If preceded by another `$` (e.g. `$$var` — variable variable),
        // skip for now.
        if i > 0 && chars[i - 1] == '$' {
            return None;
        }

        // If preceded by `->` or `::`, member completion handles this
        if i >= 2 && chars[i - 2] == '-' && chars[i - 1] == '>' {
            return None;
        }
        if i >= 2 && chars[i - 2] == ':' && chars[i - 1] == ':' {
            return None;
        }

        let partial: String = chars[i..col].iter().collect();
        // Must be at least `$`
        if partial.is_empty() {
            return None;
        }

        Some(partial)
    }

    /// Build completion items for variable names visible at the cursor.
    ///
    /// Consults the precomputed [`SymbolMap`] to find variable definitions
    /// in the correct scope (method body, function body, closure, or
    /// top-level code).  This ensures:
    ///   - Properties (`$this->name`) are NOT listed as variables.
    ///   - Method/function parameters only appear inside their body.
    ///   - `$this` only appears inside non-static methods.
    ///   - Variables from unrelated classes/methods are excluded.
    ///
    /// Additionally, PHP superglobals (`$_GET`, `$_POST`, …) are always
    /// offered.
    ///
    /// The prefix must include the `$` (e.g. `"$us"`).
    /// Returns `(items, is_incomplete)`.
    pub(crate) fn build_variable_completions(
        content: &str,
        prefix: &str,
        position: Position,
        symbol_map: Option<&SymbolMap>,
    ) -> (Vec<CompletionItem>, bool) {
        let prefix_lower = prefix.to_lowercase();
        let mut seen: HashSet<String> = HashSet::new();
        let mut items: Vec<CompletionItem> = Vec::new();

        let cursor_offset = position_to_byte_offset(content, position) as u32;

        // Compute the replacement range: from the start of the `$` prefix
        // to the cursor position.  Using `text_edit` with an explicit range
        // prevents the double-dollar problem in editors (Helix, Neovim)
        // that don't consider `$` part of a word boundary.
        let prefix_char_len = prefix.chars().count() as u32;
        let replace_range = Range {
            start: Position {
                line: position.line,
                character: position.character.saturating_sub(prefix_char_len),
            },
            end: position,
        };

        // ── 1. SymbolMap-based scope-aware variable collection ────────
        let scope_vars = collect_variables_from_symbol_map(symbol_map, cursor_offset);

        for var_name in &scope_vars {
            if !var_name.to_lowercase().starts_with(&prefix_lower) {
                continue;
            }
            if !seen.insert(var_name.clone()) {
                continue;
            }
            items.push(CompletionItem {
                label: var_name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some("variable".to_string()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: replace_range,
                    new_text: var_name.clone(),
                })),
                filter_text: Some(var_name.clone()),
                sort_text: Some(format!("0_{}", var_name.to_lowercase())),
                ..CompletionItem::default()
            });
        }

        // ── 2. PHP superglobals ─────────────────────────────────────
        for &name in Self::SUPERGLOBALS {
            if !name.to_lowercase().starts_with(&prefix_lower) {
                continue;
            }
            if !seen.insert(name.to_string()) {
                continue;
            }
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some("PHP superglobal".to_string()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: replace_range,
                    new_text: name.to_string(),
                })),
                filter_text: Some(name.to_string()),
                sort_text: Some(format!("z_{}", name.to_lowercase())),
                tags: Some(vec![CompletionItemTag::DEPRECATED]),
                ..CompletionItem::default()
            });
        }

        let is_incomplete = items.len() > Self::MAX_VARIABLE_COMPLETIONS;
        if is_incomplete {
            items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            items.truncate(Self::MAX_VARIABLE_COMPLETIONS);
        }

        (items, is_incomplete)
    }
}

// ─── Scope-aware variable collector ─────────────────────────────────────────

/// Collect all variable names visible at `cursor_offset` from the
/// precomputed [`SymbolMap`].
///
/// Uses [`SymbolMap::find_enclosing_scope`] to determine the current
/// scope, then filters [`VarDefSite`] entries by matching
/// `scope_start` and `effective_from <= cursor_offset`.
///
/// The returned set contains variable names including the `$` prefix
/// (e.g. `"$user"`, `"$this"`).
fn collect_variables_from_symbol_map(
    symbol_map: Option<&SymbolMap>,
    cursor_offset: u32,
) -> HashSet<String> {
    let Some(map) = symbol_map else {
        return HashSet::new();
    };

    // Start with the innermost scope containing the cursor, then walk
    // outward through arrow function boundaries (which inherit the
    // parent scope) but stop at closure and function boundaries (which
    // isolate their scope).
    let mut scope = map.find_enclosing_scope(cursor_offset);
    let mut visible_scopes: Vec<u32> = Vec::new();
    loop {
        visible_scopes.push(scope);
        if scope == 0 {
            // Reached top-level scope.
            break;
        }
        // If this scope is an arrow function, walk up to the parent.
        if map.arrow_fn_scopes.contains(&scope) {
            // Find the parent scope: the next-innermost scope that
            // contains `scope` but is not `scope` itself.
            let mut parent: u32 = 0;
            for &(start, end) in &map.scopes {
                if start < scope && scope <= end && start > parent {
                    parent = start;
                }
            }
            scope = parent;
        } else {
            // Closure or function boundary — stop here.
            break;
        }
    }

    let mut vars = HashSet::new();

    for def in &map.var_defs {
        if visible_scopes.contains(&def.scope_start) && def.effective_from <= cursor_offset {
            vars.insert(format!("${}", def.name));
        }
    }

    // Remove variables whose most recent definition before cursor is an Unset.
    let mut to_remove = Vec::new();
    for var_name_with_dollar in &vars {
        let name_without_dollar = &var_name_with_dollar[1..];
        let last_def = map.var_defs.iter().rfind(|d| {
            d.name == name_without_dollar
                && visible_scopes.contains(&d.scope_start)
                && d.effective_from <= cursor_offset
        });
        if let Some(d) = last_def
            && d.kind == VarDefKind::Unset
            && (d.block_end == u32::MAX || cursor_offset <= d.block_end)
        {
            to_remove.push(var_name_with_dollar.clone());
        }
    }
    for name in to_remove {
        vars.remove(&name);
    }

    // Add `$this` when inside a non-static method.  In PHP, `$this`
    // is automatically available inside closures defined in instance
    // methods (since PHP 5.4), so we check ALL enclosing scopes.
    // If any enclosing scope is an instance method body, add `$this`.
    for &(start, end) in &map.instance_method_scopes {
        if start <= cursor_offset && cursor_offset <= end {
            vars.insert("$this".to_string());
            break;
        }
    }

    vars
}
