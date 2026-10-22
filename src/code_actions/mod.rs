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
mod import_all_classes;
mod import_class;
mod inline_variable;
mod insert_translation_key;
mod mago;
mod naming;
pub(crate) mod phpstan;
mod promote_constructor_param;
mod remove_unused_import;
mod replace_deprecated;
mod replace_fqcn;
mod simplify_null;
mod sort_use_statements;
mod update_docblock;
mod use_line_deletion;
pub(crate) use use_line_deletion::{
    build_line_deletion_edit, build_use_directive_deletion_edit, range_touches_use_import,
};

mod dispatch;
mod docblock_edit;
mod helpers;

pub(crate) use docblock_edit::{
    DocblockAbove, FunctionDocblock, find_docblock_above_line, function_docblock_at_line,
    render_docblock,
};
pub(crate) use helpers::{
    CodeActionData, FileTextEdits, create_file_edit, deferred_quickfix, detect_indent_from_members,
    document_changes_edit, extract_wants_all_occurrences, indent_of_line_at, indent_unit,
    make_code_action_data, multi_file_edit, occurrence_replacement_edit, push_extract_actions,
    replace_spans_edits, rewrite_action, selected_byte_range, selection_and_identical_occurrences,
    single_edit, single_file_edit, sort_edits_by_position, span_overlaps_request,
    visit_file_text_edits,
};
