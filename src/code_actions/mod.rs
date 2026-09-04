//! Code actions — `textDocument/codeAction` handler.
//!
//! This module provides code actions for PHP files:
//!
//! - **Import class** — when the cursor is on an unresolved class name,
//!   offer to add a `use` statement for matching classes found in the
//!   class index and stubs.  Also offers a bulk "Import all
//!   missing classes" action when two or more unresolved names exist in
//!   the file, importing the best candidate for each in one step.
//! - **Remove unused import** — when the cursor is on (or a diagnostic
//!   overlaps with) an unused `use` statement, offer to remove it.
//!   Also offers a bulk "Remove all unused imports" action.
//! - **Sort use statements** — when the cursor is on a `use` import
//!   line and the block isn't already sorted, offer to re-sort it
//!   alphabetically within each blank-line-separated group and import
//!   kind (plain `use` / `use function` / `use const`).
//! - **Import qualified symbol** — replace qualified class, function, or
//!   constant usages with an import and short name, aliasing conflicts.
//! - **Implement missing methods** — when the cursor is inside a
//!   concrete class that extends an abstract class or implements an
//!   interface with unimplemented methods, offer to generate stubs.
//! - **Replace deprecated call** — when the cursor is on a deprecated
//!   function or method call that has a `#[Deprecated(replacement: "...")]`
//!   template, offer to rewrite the call to the suggested replacement.
//! - **PHPStan quickfixes** — a family of code actions that respond to
//!   PHPStan diagnostics.  See the [`phpstan`] submodule for details.
//! - **Change visibility** — when the cursor is on a method, property,
//!   constant, or promoted constructor parameter with an explicit
//!   visibility modifier, offer to change it to each alternative
//!   (`public` ↔ `protected` ↔ `private`).
//! - **Update docblock** — when the cursor is on a function or method
//!   whose existing docblock's `@param`/`@return` tags don't match the
//!   signature, offer to patch the docblock (add missing params, remove
//!   stale ones, reorder, fix contradicted types, remove redundant
//!   `@return void`).
//! - **Promote constructor parameter** — when the cursor is on a
//!   constructor parameter that has a matching property declaration and
//!   `$this->name = $name;` assignment, offer to convert it into a
//!   constructor-promoted property.
//! - **Generate constructor** — when the cursor is inside a class that
//!   has non-static properties but no `__construct` method, offer to
//!   generate a constructor that accepts each qualifying property as a
//!   parameter and assigns it.
//! - **Generate getter/setter** — when the cursor is on a property
//!   declaration, offer to generate `getX()` / `setX()` accessor
//!   methods (or `isX()` for `bool` properties).  Readonly properties
//!   only get a getter.  Static properties generate static methods.
//! - **Generate property hooks** — when the cursor is on a property
//!   declaration (PHP 8.4+), offer to generate `get` and/or `set`
//!   hooks inline on the property.  Static properties are skipped.
//!   Readonly properties only get a `get` hook.  Interface properties
//!   generate abstract hook signatures without bodies.
//! - **Simplify with null coalescing / null-safe operator** — when the
//!   cursor is on a ternary expression that can be simplified, offer
//!   to rewrite it.  Supported patterns: `isset($x) ? $x : $d` →
//!   `$x ?? $d`, `$x !== null ? $x : $d` → `$x ?? $d`, `$x === null
//!   ? $d : $x` → `$x ?? $d`, `$x !== null ? $x->foo() : null` →
//!   `$x?->foo()` (PHP 8.0+).
//! - **Convert to string interpolation** — when the cursor is on a string
//!   concatenation that mixes literal text with simple variable
//!   expressions, offer to rewrite it as a single double-quoted
//!   interpolated string (`'Hello ' . $name` → `"Hello {$name}"`).
//! - **Extract constant** — when the user selects a literal expression
//!   (string, integer, float, or boolean) inside a class body, offer to
//!   extract it into a class constant.  The literal is replaced with
//!   `self::CONSTANT_NAME` and a new constant declaration is inserted at
//!   the top of the class (after any existing constants).  Offers both
//!   single-occurrence and all-occurrences variants when duplicates exist.
//!
//! ## Deferred edit computation (`codeAction/resolve`)
//!
//! Expensive code actions (PHPStan quickfixes, extract function/method,
//! extract variable, extract constant, inline variable) use a two-phase
//! model:
//!
//! 1. **Phase 1** (`textDocument/codeAction`): Return lightweight
//!    `CodeAction` objects with a `data` field but **no `edit`**.
//! 2. **Phase 2** (`codeAction/resolve`): When the user picks an
//!    action, the editor sends it back and the server fills in `edit`.
//!
//! This avoids computing workspace edits on every cursor movement.
//! For PHPStan quickfixes, resolve also eagerly clears the matched
//! diagnostic from the cache and pushes updated diagnostics.

mod change_visibility;
mod convert_switch_to_match;
mod convert_to_arrow_function;
mod convert_to_closure;
mod convert_to_instance_variable;
mod convert_to_interpolation;
pub(crate) mod cursor_context;
mod extract_constant;
mod extract_function;
mod extract_interface;
mod extract_variable;
mod fix_class_case;
mod fix_class_name;
mod fix_namespace;
mod generate_constructor;
mod generate_getter_setter;
mod generate_property_hooks;
pub(crate) mod implement_methods;
mod import_class;
mod inline_variable;
mod mago;
mod naming;
pub(crate) mod phpstan;
mod promote_constructor_param;
mod remove_unused_import;
pub(crate) use remove_unused_import::{build_line_deletion_edit, cursor_on_use_import_line};
mod replace_deprecated;
mod replace_fqcn;
mod simplify_null;
mod sort_use_statements;
mod update_docblock;

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::sequence::Sequence;
use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::*;

use crate::Backend;

mod docblock_edit;
pub(crate) use docblock_edit::{DocblockAbove, find_docblock_above_line};

// ─── Shared edit builders ─────────────────────────────────────────────────────

/// Build a [`WorkspaceEdit`] that applies a set of text edits to a single file.
///
/// Nearly every code action produces edits for exactly one document.  This
/// wraps the `changes` map construction so handlers don't each open-code the
/// `document_changes: None` / `change_annotations: None` boilerplate.
pub(crate) fn single_file_edit(uri: Url, edits: Vec<TextEdit>) -> WorkspaceEdit {
    let mut changes = HashMap::new();
    changes.insert(uri, edits);
    WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    }
}

/// Build a [`WorkspaceEdit`] that applies a single text edit to one file.
///
/// Convenience wrapper over [`single_file_edit`] for the common case of one
/// range → one replacement string.
pub(crate) fn single_edit(uri: Url, range: Range, new_text: String) -> WorkspaceEdit {
    single_file_edit(uri, vec![TextEdit { range, new_text }])
}

// ─── Indentation helpers ──────────────────────────────────────────────────────

/// Return the leading whitespace of the line containing `offset`.
///
/// This is the raw indentation of that line, without adding an extra level.
pub(crate) fn indent_of_line_at(content: &str, offset: usize) -> String {
    let before = &content[..offset.min(content.len())];
    let line_start = before.rfind('\n').map_or(0, |p| p + 1);
    content[line_start..offset.min(content.len())]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect()
}

/// Detect the file's indentation unit (a tab, two spaces, or four spaces).
///
/// Scans lines for the first indented line and infers the convention,
/// defaulting to four spaces when nothing indented is found.
pub(crate) fn indent_unit(content: &str) -> &'static str {
    for line in content.lines() {
        if line.starts_with('\t') {
            return "\t";
        }
        let spaces: usize = line.chars().take_while(|c| *c == ' ').count();
        if spaces >= 2 {
            if spaces.is_multiple_of(4) {
                return "    ";
            }
            return "  ";
        }
    }
    "    "
}

// ─── Shared helpers ─────────────────────────────────────────────────────────

/// Detect indentation from the first class member's position in the source.
///
/// Looks at the line containing the first member to determine the
/// indent string.  Falls back to four spaces.
pub(super) fn detect_indent_from_members<'a>(
    members: &Sequence<'a, ClassLikeMember<'a>>,
    content: &str,
) -> String {
    if let Some(first) = members.first() {
        let offset = first.span().start.offset as usize;
        let line_start = content[..offset]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        let line_prefix = &content[line_start..offset];
        let indent: String = line_prefix
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        if !indent.is_empty() {
            return indent;
        }
    }

    // Fallback: four spaces.
    "    ".to_string()
}

// ─── Resolve data ───────────────────────────────────────────────────────────

/// Opaque data attached to a `CodeAction` for deferred edit computation.
///
/// Serialized into the `data` field of `CodeAction` during Phase 1.
/// Deserialized in the `codeAction/resolve` handler (Phase 2) to
/// recompute the workspace edit on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CodeActionData {
    /// Identifies which action this is (e.g. `"phpstan.addThrows"`,
    /// `"refactor.extractFunction"`).
    pub action_kind: String,
    /// The file URI the action applies to.
    pub uri: String,
    /// The cursor/selection range from the original `codeAction` request.
    pub range: Range,
    /// Action-specific context needed to recompute the edit.
    ///
    /// For PHPStan actions this carries the diagnostic message,
    /// identifier, and line number.  For refactoring actions it
    /// carries whatever lightweight context avoids a full re-scan.
    #[serde(default)]
    pub extra: serde_json::Value,
}

impl Backend {
    /// Handle a `textDocument/codeAction` request.
    ///
    /// Returns a list of code actions applicable at the given range.
    /// Expensive actions return a lightweight stub with a [`CodeActionData`]
    /// `data` field and no `edit`; the edit is computed lazily in
    /// [`resolve_code_action`](Self::resolve_code_action).
    pub fn handle_code_action(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
    ) -> Vec<CodeActionOrCommand> {
        let mut actions = Vec::new();

        // Parse the file once and share the result across every collector
        // below.  Each collector resolves cursor context by walking the
        // AST via `with_parsed_program(content, …)`; without this guard
        // they would each re-parse the same file from scratch.
        let _parse_guard = crate::parser::with_parse_cache(content);

        // ── Import class ────────────────────────────────────────────────
        self.collect_import_class_actions(uri, content, params, &mut actions);

        // ── Import qualified symbol and shorten usages ─────────────────
        self.collect_replace_fqcn_actions(uri, content, params, &mut actions);

        // ── Import all missing classes (bulk) ───────────────────────────
        self.collect_import_all_classes_action(uri, content, params, &mut actions);

        // ── Remove unused imports ───────────────────────────────────────
        self.collect_remove_unused_import_actions(uri, content, params, &mut actions);

        // ── Sort use statements ─────────────────────────────────────────
        self.collect_sort_use_statements_action(uri, content, params, &mut actions);

        // ── Implement missing methods ───────────────────────────────────
        self.collect_implement_methods_actions(uri, content, params, &mut actions);

        // ── Replace deprecated call ─────────────────────────────────────
        self.collect_replace_deprecated_actions(uri, content, params, &mut actions);

        // ── PHPStan-specific quickfixes (deferred) ──────────────────────
        self.collect_phpstan_actions(uri, content, params, &mut actions);

        // ── Mago quick-fix code actions ─────────────────────────────────
        self.collect_mago_fix_actions(uri, content, params, &mut actions);

        // ── Change visibility ───────────────────────────────────────────
        self.collect_change_visibility_actions(uri, content, params, &mut actions);

        // ── Update docblock to match signature ──────────────────────────
        self.collect_update_docblock_actions(uri, content, params, &mut actions);

        // ── Promote constructor parameter ───────────────────────────────────
        self.collect_promote_constructor_param_actions(uri, content, params, &mut actions);

        // ── Generate constructor ────────────────────────────────────────────
        self.collect_generate_constructor_actions(uri, content, params, &mut actions);

        // ── Generate getter/setter ──────────────────────────────────────────
        self.collect_generate_getter_setter_actions(uri, content, params, &mut actions);

        // ── Generate property hooks (PHP 8.4+) ─────────────────────────────
        self.collect_generate_property_hook_actions(uri, content, params, &mut actions);

        // ── Extract constant (deferred) ─────────────────────────────────
        self.collect_extract_constant_actions(uri, content, params, &mut actions);

        // ── Extract variable (deferred) ─────────────────────────────────
        self.collect_extract_variable_actions(uri, content, params, &mut actions);

        // ── Extract function / method (deferred) ────────────────────────
        self.collect_extract_function_actions(uri, content, params, &mut actions);

        // ── Inline variable (deferred) ──────────────────────────────
        self.collect_inline_variable_actions(uri, content, params, &mut actions);

        // ── Convert to instance variable (deferred) ─────────────────
        self.collect_convert_to_instance_variable_actions(uri, content, params, &mut actions);

        // ── Simplify with null coalescing / null-safe operator ──────────
        self.collect_simplify_null_actions(uri, content, params, &mut actions);

        // ── Convert to arrow function / closure ─────────────────────────
        self.collect_convert_to_arrow_function_actions(uri, content, params, &mut actions);
        self.collect_convert_to_closure_actions(uri, content, params, &mut actions);

        // ── Convert switch to match expression ──────────────────────────
        self.collect_convert_switch_to_match_actions(uri, content, params, &mut actions);

        // ── Convert concatenation to string interpolation ───────────────
        self.collect_convert_to_interpolation_actions(uri, content, params, &mut actions);

        // ── Fix namespace (PSR-4 mismatch) ──────────────────────────────
        self.collect_fix_namespace_actions(uri, content, params, &mut actions);

        // ── Fix class name (filename mismatch) ──────────────────────────
        self.collect_fix_class_name_actions(uri, content, params, &mut actions);

        // ── Fix class-reference case (PSR-4 autoload safety) ────────────
        self.collect_fix_class_case_actions(uri, content, params, &mut actions);

        // ── Extract interface ────────────────────────────────────────────
        self.collect_extract_interface_actions(uri, content, params, &mut actions);

        actions
    }

    /// Handle a `codeAction/resolve` request.
    ///
    /// The editor sends back a `CodeAction` that was previously returned
    /// by [`handle_code_action`](Self::handle_code_action) with a `data`
    /// field but no `edit`.  This method deserializes the data, computes
    /// the full workspace edit, and returns the completed action.
    ///
    /// For PHPStan quickfixes the matched diagnostic is also eagerly
    /// removed from the cache and updated diagnostics are returned via
    /// the `diagnostics_to_republish` output parameter.
    pub fn resolve_code_action(&self, mut action: CodeAction) -> (CodeAction, Option<String>) {
        let data_value = match &action.data {
            Some(v) => v.clone(),
            None => return (action, None),
        };

        let data: CodeActionData = match serde_json::from_value(data_value) {
            Ok(d) => d,
            Err(_) => return (action, None),
        };

        let content = match self.get_file_content(&data.uri) {
            Some(c) => c,
            None => return (action, None),
        };

        // Parse the file once and share it across the resolve handler below.
        // Resolving an extract action, for example, walks the AST several
        // times (scope map, return analysis, parameter order, return type);
        // without this guard each walk would re-parse the same file.
        let _parse_guard = crate::parser::with_parse_cache(&content);

        // Resolving an action can need types (an extracted function's
        // return type, a docblock's inferred `@return`).  This handler
        // fetches its own file content, so it activates the type-engine
        // resolvers itself rather than going through `with_file_content`.
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();

        let result = match data.action_kind.as_str() {
            // ── PHPStan quickfixes ──────────────────────────────────
            "phpstan.addThrows" => {
                let edit = self.resolve_add_throws(&data, &content);

                // Adding a @throws tag for an exception resolves the
                // diagnostic for *every* throw of that exception in
                // the same function/method body.  Expand the action's
                // diagnostic list so they all get cleared at once.
                if edit.is_some() {
                    self.expand_sibling_checked_exception_diags(&data, &content, &mut action);
                }

                edit
            }
            "phpstan.removeThrows" => self.resolve_remove_throws(&data, &content),
            "phpstan.addOverride" => self.resolve_add_override(&data, &content),
            "phpstan.addIgnore" => self.resolve_add_ignore(&data, &content),
            "phpstan.removeIgnore" => self.resolve_remove_ignore(&data, &content),
            "phpstan.removeOverride" => self.resolve_remove_override(&data, &content),
            "phpstan.addReturnTypeWillChange" => {
                self.resolve_add_return_type_will_change(&data, &content)
            }
            "phpstan.fixPhpDocType.update" | "phpstan.fixPhpDocType.remove" => {
                self.resolve_fix_phpdoc_type(&data, &content)
            }
            "phpstan.newStatic.addTag"
            | "phpstan.newStatic.finalClass"
            | "phpstan.newStatic.finalConstructor" => self.resolve_new_static(&data, &content),
            // ── Fix prefixed class name ─────────────────────────────
            "phpstan.fixPrefixedClass" => self.resolve_fix_prefixed_class(&data, &content),
            // ── Remove always-true assert() ─────────────────────────
            "phpstan.removeAssert" => self.resolve_remove_assert(&data, &content),
            // ── Fix return type ─────────────────────────────────────
            "phpstan.fixReturnType.stripExpr"
            | "phpstan.fixReturnType.changeTypeToActual"
            | "phpstan.fixReturnType.changeType"
            | "phpstan.fixReturnType.addType"
            | "phpstan.fixReturnType.updateReturnType" => {
                self.resolve_fix_return_type(&data, &content)
            }
            // ── Remove unused return type ────────────────────────────
            "phpstan.removeUnusedReturnType" => {
                self.resolve_remove_unused_return_type(&data, &content)
            }
            // ── Add iterable return type ────────────────────────────
            "phpstan.addIterableType" => self.resolve_add_iterable_type(&data, &content),
            // ── Remove unreachable statement ────────────────────────
            "phpstan.removeUnreachable" => self.resolve_remove_unreachable(&data, &content),
            // ── Change visibility (parent-aware) ────────────────────
            "refactor.changeVisibility" => self.resolve_change_visibility(&data, &content),
            // ── Unused import quickfixes ─────────────────────────────
            "quickfix.removeUnusedImport" | "quickfix.removeAllUnusedImports" => {
                self.resolve_remove_unused_import(&data, &content, action.diagnostics.as_deref())
            }
            // ── Refactoring actions ─────────────────────────────────
            "refactor.extractConstant" | "refactor.extractConstantAll" => {
                self.resolve_extract_constant(&data, &content)
            }
            "refactor.extractVariable" | "refactor.extractVariableAll" => {
                self.resolve_extract_variable(&data, &content)
            }
            // ── Import all missing classes ───────────────────────────────
            "source.importAllClasses" => self.resolve_import_all_classes(&data, &content),
            "refactor.extractFunction" => self.resolve_extract_function(&data, &content),
            "refactor.extractInterface" => self.resolve_extract_interface(&data, &content),
            "refactor.inlineVariable" => self.resolve_inline_variable(&data, &content),
            "refactor.extractInstanceVariable" => {
                self.resolve_convert_to_instance_variable(&data, &content)
            }
            _ => None,
        };

        if let Some(edit) = result {
            action.edit = Some(edit);
        }

        // Only clear diagnostics and republish when the resolve
        // actually produced an edit.  If the file changed between
        // Phase 1 and Phase 2 the resolve may return None, and we
        // must not remove a diagnostic that wasn't actually fixed.
        //
        // This applies to all quickfix actions that attach diagnostics
        // (PHPStan and unused-import alike).  The eager clear+republish
        // removes the squiggly line before the text edit is applied,
        // so the editor doesn't have to guess where to move it.
        let republish_uri = if let Some(ref diags) = action.diagnostics
            && !diags.is_empty()
            && action.edit.is_some()
        {
            if data.action_kind.starts_with("phpstan.")
                || data.action_kind == "refactor.changeVisibility"
            {
                // PHPStan diagnostics live in a separate cache.
                self.clear_phpstan_diagnostics_after_resolve(&data.uri, diags);
            }

            // Push all resolved diagnostics to the suppression list
            // so that `publish_diagnostics_for_file` filters them out.
            // This handles both PHPStan (cached) and native (recomputed)
            // diagnostics uniformly.
            {
                let mut suppressed = self.diag.suppressed.lock();
                suppressed.extend(diags.iter().cloned());
            }

            Some(data.uri.clone())
        } else {
            None
        };

        (action, republish_uri)
    }
}

/// Build a [`CodeActionData`] value and serialize it to JSON.
pub(crate) fn make_code_action_data(
    action_kind: &str,
    uri: &str,
    range: &Range,
    extra: serde_json::Value,
) -> serde_json::Value {
    serde_json::to_value(CodeActionData {
        action_kind: action_kind.to_string(),
        uri: uri.to_string(),
        range: *range,
        extra,
    })
    .unwrap_or_default()
}

/// Find all occurrences of `needle` in `content` within the byte range
/// `[scope_start, scope_end)` that are textually identical to the selected
/// expression, excluding the original selection `[sel_start, sel_end)`.
///
/// Returns `(start, end)` byte offset pairs. Word boundaries are checked
/// so that substrings of longer identifiers are not matched.
pub(crate) fn find_identical_occurrences(
    content: &str,
    needle: &str,
    sel_start: usize,
    sel_end: usize,
    scope_start: usize,
    scope_end: usize,
) -> Vec<(usize, usize)> {
    if needle.is_empty() || scope_start >= scope_end || scope_end > content.len() {
        return Vec::new();
    }
    let haystack = &content[scope_start..scope_end];
    let mut results = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = haystack[search_from..].find(needle) {
        let abs_start = scope_start + search_from + pos;
        let abs_end = abs_start + needle.len();
        // Skip the original selection.
        if abs_start != sel_start || abs_end != sel_end {
            // Check word boundaries to avoid matching substrings.
            let before_ok = abs_start == 0
                || !content.as_bytes()[abs_start - 1].is_ascii_alphanumeric()
                    && content.as_bytes()[abs_start - 1] != b'_'
                    && content.as_bytes()[abs_start - 1] != b'$';
            let after_ok = abs_end >= content.len()
                || !content.as_bytes()[abs_end].is_ascii_alphanumeric()
                    && content.as_bytes()[abs_end] != b'_';
            if before_ok && after_ok {
                results.push((abs_start, abs_end));
            }
        }
        search_from = search_from + pos + 1;
    }
    results
}
