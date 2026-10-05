/// Completion-related modules.
///
/// This sub-module groups all completion logic:
///
/// The subject-to-type resolution engine (subject extraction, `resolver`,
/// `call_resolution`, `types`, and variable type resolution) lives in the
/// shared `crate::type_engine` module, not here.
///
/// ## Top-level modules
///
/// - **handler**: Top-level completion request orchestration
/// - **target**: Extracting the completion target (access operator and subject)
/// - **builder**: Building LSP `CompletionItem`s from resolved class info
/// - **resolve**: `completionItem/resolve` — lazily filling in documentation
///   for the highlighted item
/// - **named_args**: Named argument completion inside function/method call parens
/// - **array_callable**: Method name completion inside array callable strings
///   (`[Class::class, '` → suggest class methods)
/// - **array_shape**: Array shape key completion (`$arr['` → suggest known keys)
///   and raw variable type resolution for array shape value chaining
/// - **eloquent_string**: Eloquent relation dot-notation and column name string
///   completion inside method arguments like `with('`, `where('`, etc.
/// - **command_params**: Artisan command parameter completion (own
///   `argument()`/`option()` calls and `Artisan::call()` parameter arrays)
/// - **laravel_paths**: Path-segment completion inside Laravel's path helpers
///   (`base_path('|')`, `resource_path('|')`, …)
/// - **laravel_request_keys**: Request input field name completion inside
///   `$request->input('`, `->has('`, `$request['`, etc., driven by the
///   validation rules in scope
/// - **laravel_route_controller**: Controller method name completion inside
///   `Route::patch('path', '|')` within a `->controller()->group()` block
/// - **laravel_route_params**: Route parameter name completion driven by the
///   named route's URI (`route('users.show', ['|' => 1])`)
/// - **laravel_string_keys**: Route/config/view/translation key completion
///   (`route('|')`, `config('|')`, `view('|')`, `__('|')`)
/// - **use_edit**: Use-statement insertion and conflict analysis
///
/// ## Sub-grouped modules
///
/// ### `variable/` — Variable-name completion
///
/// - **completion**: Variable name completions and scope collection
///
/// ### `context/` — Context-specific completion
///
/// - **catch_completion**: Smart exception type completion inside `catch()` clauses
/// - **class_completion**: Class name completions (class, interface, trait, enum)
/// - **constant_completion**: Global constant name completions
/// - **function_completion**: Standalone function name completions
/// - **namespace_completion**: Namespace declaration completions
/// - **type_hint_completion**: Type completion inside function/method parameter lists,
///   return types, and property declarations (offers native PHP types + class names)
///
/// ### `phpdoc/` — PHPDoc completion
///
/// - **mod** (phpdoc): PHPDoc tag completion inside `/** … */` blocks
/// - **context**: PHPDoc context detection and symbol info extraction
///   (`DocblockContext`, `SymbolInfo`, `detect_context`, `extract_symbol_info`,
///   `detect_docblock_typing_position`, `extract_phpdoc_prefix`)
///
/// ### `source/` — Source analysis
///
/// - **comment_position**: Comment and docblock position detection (`is_inside_docblock`,
///   `is_inside_non_doc_comment`, `position_to_byte_offset`)
/// - **helpers**: Source-text scanning helpers (closure return types,
///   first-class callable resolution, `new` expression parsing, array access)
/// - **throws_analysis**: Throws analysis pipeline (throw scanning, catch-block filtering,
///   uncaught detection, method `@throws` / return-type lookup, import helpers)
///   used by both phpdoc and catch_completion
///
/// Class inheritance merging (traits, mixins, parent chain) lives in the
/// top-level `crate::inheritance` module since it is shared infrastructure
/// used by completion, definition, and future features (hover, references).
// ─── Top-level modules ──────────────────────────────────────────────────────
pub(crate) mod array_callable;
pub mod array_shape;
pub(crate) mod builder;
pub(crate) mod command_params;
pub(crate) mod eloquent_string;
pub(crate) mod handler;
pub(crate) mod laravel_paths;
pub(crate) mod laravel_request_keys;
pub(crate) mod laravel_route_controller;
pub(crate) mod laravel_route_params;
pub(crate) mod laravel_string_keys;
pub mod named_args;
pub(crate) mod resolve;
pub(crate) mod symfony;
pub(crate) mod target;
pub(crate) mod use_edit;

// ─── Sub-grouped modules ───────────────────────────────────────────────────

pub(crate) mod context;
pub mod phpdoc;
pub(crate) mod source;
pub(crate) mod variable;

// ─── Backward-compatible re-exports ─────────────────────────────────────────
//
// These re-exports preserve existing import paths throughout the codebase.
// Code that uses `crate::completion::comment_position` (etc.) continues to
// compile without changes.

// source/
pub use source::comment_position;

// context/
pub(crate) use context::catch_completion;
pub(crate) use context::class_completion;
pub(crate) use context::keyword_completion;
pub(crate) use context::type_hint_completion;
