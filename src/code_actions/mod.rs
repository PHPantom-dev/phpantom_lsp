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
//!   A bulk variant does every qualified symbol in the namespace at once.
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
//! - **Create missing view** — when a `view('name')`-style call names a
//!   template that resolves to nothing on disk (the `invalid_laravel_view`
//!   diagnostic), offer to create the `.blade.php` file under the
//!   project's configured view root (or the matching package namespace's
//!   own view directory) and open it.
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
mod create_missing_view;
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

mod docblock_edit;
mod helpers;

use tower_lsp::lsp_types::*;

use crate::Backend;

pub(crate) use docblock_edit::{DocblockAbove, find_docblock_above_line};
pub(crate) use helpers::{
    CodeActionData, create_file_edit, detect_indent_from_members, find_identical_occurrences,
    indent_of_line_at, indent_unit, make_code_action_data, single_edit, single_file_edit,
};

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

        // ── Create missing view ─────────────────────────────────────────
        self.collect_create_missing_view_actions(uri, content, params, &mut actions);

        // Every collector plans its edits against the PHP a template lowers
        // to; the editor applies them to the template itself.
        for action in &mut actions {
            if let CodeActionOrCommand::CodeAction(CodeAction {
                edit: Some(edit), ..
            }) = action
            {
                self.translate_workspace_edit(edit);
            }
        }

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

        // A template's action was planned against its virtual PHP, so the
        // resolve reads the same text.
        let Some(content) = self.analysable_content(&data.uri) else {
            return (action, None);
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

        if let Some(mut edit) = result {
            self.translate_workspace_edit(&mut edit);
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
