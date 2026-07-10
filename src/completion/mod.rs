/// Completion-related modules.
///
/// This sub-module groups all completion logic:
///
/// ## Top-level modules
///
/// - **handler**: Top-level completion request orchestration
/// - **target**: Extracting the completion target (access operator and subject)
/// - **resolver**: Resolving the subject to a concrete class type
/// - **call_resolution**: Call expression and callable target resolution (method
///   calls, static calls, function calls, constructor calls, signature help,
///   named-argument completion)
/// - **builder**: Building LSP `CompletionItem`s from resolved class info
/// - **named_args**: Named argument completion inside function/method call parens
/// - **array_callable**: Method name completion inside array callable strings
///   (`[Class::class, '` → suggest class methods)
/// - **array_shape**: Array shape key completion (`$arr['` → suggest known keys)
///   and raw variable type resolution for array shape value chaining
/// - **eloquent_string**: Eloquent relation dot-notation and column name string
///   completion inside method arguments like `with('`, `where('`, etc.
/// - **use_edit**: Use-statement insertion and conflict analysis
///
/// ## Sub-grouped modules
///
/// ### `variable/` — Variable resolution
///
/// - **resolution**: Variable type resolution via assignment scanning
/// - **completion**: Variable name completions and scope collection
/// - **rhs_resolution**: Right-hand-side expression resolution for variable
///   assignments (instantiation, array access, function/method/static calls,
///   property access, match, ternary, clone)
/// - **class_string_resolution**: Class-string variable resolution (`$cls = User::class`)
/// - **raw_type_inference**: Raw type inference for variable assignments (array shapes,
///   array functions, generator yields)
/// - **foreach_resolution**: Foreach value/key and array destructuring type resolution
/// - **closure_resolution**: Closure and arrow-function parameter resolution
///
/// ### `types/` — Type resolution
///
/// - **resolution**: Type-hint string to `ClassInfo` mapping (unions,
///   intersections, generics, type aliases, object shapes, property types)
/// - **narrowing**: instanceof / assert / custom type guard narrowing
/// - **conditional**: PHPStan conditional return type resolution at call sites
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
pub(crate) mod call_resolution;
pub(crate) mod eloquent_string;
pub(crate) mod handler;
pub(crate) mod laravel_route_controller;
pub mod named_args;
pub(crate) mod resolve;
pub(crate) mod resolver;
pub(crate) mod target;
pub(crate) mod use_edit;

// ─── Sub-grouped modules ───────────────────────────────────────────────────

pub(crate) mod context;
pub mod phpdoc;
pub(crate) mod source;
pub mod types;
pub(crate) mod variable;

// ─── Backward-compatible re-exports ─────────────────────────────────────────
//
// These re-exports preserve existing import paths throughout the codebase.
// Code that uses `crate::completion::comment_position` (etc.) continues to
// compile without changes.

// source/
pub use source::comment_position;

// types/
pub use types::conditional as conditional_resolution;
pub(crate) use types::resolution as type_resolution;

// context/
pub(crate) use context::catch_completion;
pub(crate) use context::class_completion;
pub(crate) use context::keyword_completion;
pub(crate) use context::type_hint_completion;
