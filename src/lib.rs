//! PHPantom — a fast, lightweight PHP language server.
//!
//! Diagnostics are debounced: each `did_change` bumps a per-file version
//! counter and spawns a delayed task. The task only publishes if its
//! version still matches (i.e. no newer edit arrived in the meantime).
//!
//! This crate is organised into the following modules:
//!
//! - [`types`] — Data structures for extracted PHP information (classes, methods, functions, etc.)
//! - `parser` — PHP parsing and AST extraction using mago_syntax
//! - [`completion`] — Completion logic (target extraction, type resolution, item building,
//!   and the top-level completion request handler)
//! - [`composer`] — Composer autoload (PSR-4, classmap) parsing and class-to-file resolution
//! - `server` — The LSP `LanguageServer` trait implementation (thin wrapper that delegates
//!   to feature-specific modules)
//! - `util` — Utility helpers (position conversion, class lookup, logging)
//! - `hover` — Hover support (`textDocument/hover`). Resolves the symbol under the
//!   cursor and returns type information, method signatures, and docblock descriptions
//! - `signature_help` — Signature help (`textDocument/signatureHelp`). Shows parameter
//!   hints while typing function/method arguments, with active-parameter tracking
//! - `definition` — Go-to-definition support for classes, members, and functions
//! - `inheritance` — Base class inheritance resolution. Merges members from parent
//!   classes and traits into a unified `ClassInfo`
//! - `virtual_members` — Virtual member provider abstraction. Defines the
//!   [`VirtualMemberProvider`](virtual_members::VirtualMemberProvider) trait and
//!   merge logic for members synthesized from `@method`/`@property` tags,
//!   `@mixin` classes, and framework-specific patterns (e.g. Laravel)
//! - `resolution` — Class and function lookup / name resolution (multi-phase:
//!   fqn_uri_index → PSR-4 → stubs)
//! - `subject_extraction` — Shared helpers for extracting the left-hand side of
//!   `->`, `?->`, and `::` access operators (used by both completion and definition)
//! - `highlight` — Document highlighting (`textDocument/documentHighlight`).
//!   When the cursor lands on a symbol, returns all other occurrences in the
//!   current file so the editor can highlight them.  Uses the precomputed
//!   `SymbolMap` with no additional parsing.  Variables are scoped to their
//!   enclosing function/closure; class names, members, functions, and constants
//!   are file-global.
//! - `semantic_tokens` — Semantic tokens (`textDocument/semanticTokens/full`).
//!   Type-aware syntax highlighting that goes beyond TextMate grammars.
//!   Maps `SymbolMap` spans to LSP semantic token types (class, interface,
//!   enum, method, property, parameter, variable, function, constant) with
//!   modifiers (declaration, static, readonly, deprecated, abstract).
//!   Resolves `ClassReference` spans to distinguish classes from interfaces,
//!   enums, and traits.  Template parameter names from `@template` tags are
//!   emitted as `typeParameter` tokens.
//! - `code_actions` — Code actions (`textDocument/codeAction`). Provides:
//!   - `code_actions::import_class` — Import class quick-fix (add a `use`
//!     statement for unresolved class names)
//!   - `code_actions::remove_unused_import` — Remove unused import quick-fix
//!     (delete individual or all unused `use` statements)
//!   - `code_actions::generate_constructor` — Generate a constructor from
//!     non-static properties
//!   - `code_actions::generate_getter_setter` — Generate `getX()`/`setX()`
//!     accessor methods (or `isX()` for `bool` properties) from a property
//!     declaration
//! - [`diagnostics`] — Diagnostic collection and delivery.  Native
//!   diagnostics prefer the pull model (`textDocument/diagnostic`, LSP
//!   3.17) whenever the client supports it.  Push diagnostics
//!   (`textDocument/publishDiagnostics`) are a compatibility fallback for
//!   clients that do not support pull diagnostics; the server should not
//!   mix both native delivery models for the same client.
//!   Currently implemented providers:
//!   - `diagnostics::deprecated` — `@deprecated` usage diagnostics (strikethrough
//!     via `DiagnosticTag::Deprecated` on references to deprecated symbols)
//!   - `diagnostics::unused_imports` — unused `use` dimming
//!     (`DiagnosticTag::Unnecessary` on imports with no references in the file)
//!   - `diagnostics::unknown_classes` — unknown class diagnostics
//!     (`Severity::Warning` on `ClassReference` spans that cannot be resolved
//!     through any resolution phase)
//!   - `diagnostics::unresolved_member_access` — opt-in diagnostic
//!     (`Severity::Hint` on `MemberAccess` spans where the subject type
//!     cannot be resolved at all; enabled via `[diagnostics]
//!     unresolved-member-access = true` in `.phpantom.toml`)
//! - [`docblock`] — PHPDoc block parsing, split into submodules:
//!   - `docblock::tags` — tag extraction (`@return`, `@var`, `@property`, `@method`,
//!     `@mixin`, `@deprecated`, `@phpstan-assert`, docblock text retrieval)
//!   - `docblock::conditional` — PHPStan conditional return type parsing
//!   - `docblock::types` — type utilities (`split_type_token`),
//!     PHPStan array shape parsing
//!     (`parse_array_shape`, `extract_array_shape_value_type`), and object shape
//!     parsing (`parse_object_shape`, `extract_object_shape_property_type`,
//!     `is_object_shape`)

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use parking_lot::{Mutex, RwLock};
use tower_lsp::Client;
use tower_lsp::lsp_types::{CompletionItem, FileChangeType};

use ci_map::{CiMap, CiSet};

/// A single parse error entry: `(message, start_byte_offset, end_byte_offset)`.
///
/// Stored per file in [`Backend::parse_errors`] during `update_ast` and
/// consumed by the syntax-error diagnostic collector.
pub(crate) type ParseErrorEntry = (String, u32, u32);

/// The standalone-function FQNs and `define()`/`const` names a single file
/// contributed to the global symbol maps on its most recent parse:
/// `(function_fqns, define_names)`.  Stored per URI in
/// [`Backend::uri_globals_index`] so a re-parse can evict what an edit removed.
pub(crate) type UriGlobals = (Vec<String>, Vec<String>);

// ─── Module declarations ────────────────────────────────────────────────────

/// Maximum number of LSP requests the tower-lsp transport processes
/// concurrently.
///
/// tower-lsp defaults to 4, which is far too low for real editors: they fire a
/// large request barrage on every keystroke (completion, a resolve per visible
/// item, diagnostics, code lens, semantic tokens, …). With a limit of 4, that
/// barrage fills tower-lsp's internal task queue, which blocks the message-read
/// loop so it can no longer even receive `$/cancelRequest` — the server stops
/// responding to everything until the backlog drains. Raising the limit lets
/// the barrage (and the cancellations that supersede stale requests) flow, so
/// cheap requests stay instant while typing. Heavy handlers run on the blocking
/// thread pool, so real CPU parallelism is bounded there; this only governs how
/// many requests may be in flight.
pub const LSP_CONCURRENCY: usize = 128;

/// Stack size for threads that parse or walk PHP ASTs.
///
/// The `mago-syntax` parser is recursive descent: it recurses once per
/// expression-nesting level (bounded by its own `MAX_RECURSION_DEPTH`,
/// but that bound is calibrated for an 8 MB stack). Our AST consumers
/// (`extract_symbol_map`, the forward walker, the diagnostic collectors)
/// are likewise recursive and walk the same depth. A deeply nested
/// expression — common in generated/bundled code such as WordPress'
/// bundled getID3 library, whose codec tables nest hundreds of levels
/// deep — exhausts the 2 MB default that spawned threads receive, and a
/// stack overflow aborts the whole process (it is a `SIGSEGV`, not a
/// catchable panic).
///
/// Rust only gives the *main* thread the 8 MB OS default; threads
/// spawned via `std::thread` and the Tokio runtime default to 2 MB. Any
/// thread that reaches [`update_ast`](Backend::update_ast) or the type
/// resolver must therefore set this explicitly. 8 MB matches the stack
/// `mago` itself uses for the same parser.
pub const PARSE_WORKER_STACK_SIZE: usize = 8 * 1024 * 1024;

pub mod analyse;
pub mod atom;
pub mod blade;
pub(crate) mod call_args;
pub mod ci_map;
pub mod classmap_scanner;
mod code_actions;
mod code_lens;
pub mod completion;
pub mod composer;
pub mod config;
mod definition;
pub mod diagnostics;
pub mod docblock;
mod document_links;
mod document_symbols;
pub mod fix;
mod folding;
mod formatting;
mod highlight;
mod hover;
pub(crate) mod inheritance;
mod inlay_hints;
mod linked_editing;
mod mago;
pub(crate) mod names;
mod parser;
pub(crate) mod phar;
pub mod php_type;
mod phpcs;
mod phpstan;
mod reference_index;
mod references;
mod rename;
mod resolution;
pub(crate) mod scope_collector;
mod selection_range;
pub mod self_update;
mod semantic_tokens;
mod server;
mod signature_help;
pub mod stub_patches;
pub mod stubs;
pub mod subject_expr;
pub(crate) mod subject_extraction;
pub(crate) mod subject_resolution;
pub(crate) mod symbol_map;
pub(crate) mod toposort;
mod type_hierarchy;
pub mod types;
mod util;
pub(crate) mod virtual_members;
mod workspace_symbols;

#[cfg(test)]
pub mod test_fixtures;

// ─── Re-exports ─────────────────────────────────────────────────────────────

// Re-export public types so that dependents (tests, main) can import them
// from the crate root, e.g. `use phpantom_lsp::{Backend, AccessKind}`.
pub use completion::target::extract_completion_target;
pub use types::{AccessKind, ClassInfo, DefineInfo, FunctionInfo, NamespaceSpan, Visibility};
pub use virtual_members::resolve_class_fully;

// ─── Backend ────────────────────────────────────────────────────────────────

/// The main LSP backend that holds all server state.
///
/// Method implementations are spread across several modules:
/// - `parser` — `parse_php`, `update_ast`, and module-level AST extraction helpers
///   (`extract_hint_type`, `extract_parameters`, `extract_visibility`, `extract_property_info`)
/// - `completion::handler` — Top-level completion request orchestration
/// - `completion::target` — module-level `extract_completion_target`
/// - `completion::resolver` — `resolve_target_classes` and type-resolution helpers
/// - `completion::builder` — module-level `build_completion_items`, `build_method_label`
/// - [`composer`] — PSR-4 autoload mapping and class file resolution
/// - `server` — `impl LanguageServer` (initialize, completion, did_open, …)
/// - `resolution` — `find_or_load_class`, `find_or_load_function`, `resolve_class_name`,
///   `resolve_function_name`
/// - `inheritance` — `resolve_class_with_inheritance` (base resolution), trait/parent merging
/// - `virtual_members` — `resolve_class_fully` (base resolution + virtual member providers),
///   `VirtualMemberProvider` trait, merge logic, provider registry
/// - `subject_extraction` — Shared subject extraction helpers for `->`, `?->`, `::` operators
/// - `util` — module-level `position_to_offset`, `find_class_at_offset`,
///   `find_class_by_name`, plus `log`, `get_classes_for_uri`
/// - `definition` — `resolve_definition`, member resolution, function resolution
/// - `diagnostics` — `publish_diagnostics_for_file`, `clear_diagnostics_for_file`,
///   `collect_deprecated_diagnostics`, `collect_unused_import_diagnostics`,
///   `collect_unknown_class_diagnostics`,
///   `collect_unknown_member_diagnostics` (includes unresolved-member-access logic)
#[derive(Default)]
pub(crate) struct LaravelStringKeyCache {
    pub route_names: Option<Vec<String>>,
    pub config_keys: Option<Vec<String>>,
    pub view_names: Option<Vec<String>>,
    pub trans_keys: Option<Vec<String>>,
}

impl LaravelStringKeyCache {
    fn invalidate_for_uri(&mut self, uri: &str) {
        if uri.contains("/routes/") {
            self.route_names = None;
        }
        if uri.contains("/config/") {
            self.config_keys = None;
        }
        if uri.contains("/resources/views/") {
            self.view_names = None;
        }
        if uri.contains("/lang/") || uri.contains("/resources/lang/") {
            self.trans_keys = None;
        }
    }
}

pub struct Backend {
    pub(crate) name: String,
    pub(crate) version: String,
    /// The name of the LSP client (IDE/editor) connected to this server.
    ///
    /// Populated from `InitializeParams.client_info.name` during the
    /// `initialize` handshake.  Used for quirks-mode adjustments when
    /// certain editors need non-standard behavior (e.g. Helix, Neovim).
    /// Empty string when the client does not report its identity.
    pub(crate) client_name: Mutex<String>,
    pub(crate) open_files: Arc<RwLock<HashMap<String, Arc<String>>>>,
    /// Maps a file URI to a list of ClassInfo extracted from that file.
    pub(crate) uri_classes_index: Arc<RwLock<HashMap<String, Vec<Arc<ClassInfo>>>>>,
    /// Per-file precomputed symbol location maps for O(log n) lookup.
    ///
    /// Built during `update_ast` by walking the AST and recording every
    /// navigable symbol occurrence (class references, member accesses,
    /// variables, function calls, etc.).  Consulted by `resolve_definition`
    /// to replace character-level backward-walking with a binary search.
    pub(crate) symbol_maps: Arc<RwLock<HashMap<String, Arc<symbol_map::SymbolMap>>>>,
    /// Cross-file candidate index for find-references.
    ///
    /// Maintained from each file's [`symbol_maps`] entry during parsing.
    /// It is deliberately coarse: reference scanners use it only to narrow
    /// candidate files, then run their existing semantic checks for aliases,
    /// inheritance, Laravel declarations, and `self/static/parent`.
    pub(crate) reference_index: reference_index::ReferenceIndex,
    /// Per-file parse errors from the Mago parser.
    ///
    /// Each entry is `(message, start_byte_offset, end_byte_offset)`.
    /// Populated during `update_ast` from `Program::errors` and consumed
    /// by the syntax-error diagnostic collector.  When the parser panics
    /// (caught by `catch_unwind`), a single "Parse failed" entry is
    /// stored instead.
    pub(crate) parse_errors: Arc<RwLock<HashMap<String, Vec<ParseErrorEntry>>>>,
    /// Per-URI locks for background `didChange` parses.
    ///
    /// `didChange` handlers offload parsing to blocking tasks.  Without a
    /// per-file lock, an older parse can finish after a newer edit and publish
    /// stale symbol state.  These locks serialize parse commits per URI; the
    /// handler also verifies that the captured text is still current before
    /// updating shared maps.
    pub(crate) did_change_parse_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// Coalescing state for expensive whole-file requests. See
    /// [`WholeFileCoalesce`] for why this exists.
    pub(crate) whole_file_coalesce: Arc<WholeFileCoalesce>,
    pub(crate) client: Option<Client>,
    /// Whether to update ASTs synchronously.  Used for testing.
    pub(crate) sync_ast_updates: bool,
    /// The root directory of the workspace (set during `initialize`).
    pub(crate) workspace_root: Arc<RwLock<Option<PathBuf>>>,
    /// PSR-4 autoload mappings parsed from `composer.json`.
    pub(crate) psr4_mappings: Arc<RwLock<Vec<composer::Psr4Mapping>>>,
    /// Maps a file URI to its `use` statement mappings (short name → fully qualified name).
    /// For example, `use Klarna\Rest\Resource;` produces `"Resource" → "Klarna\Rest\Resource"`.
    pub(crate) file_imports: Arc<RwLock<HashMap<String, HashMap<String, String>>>>,
    /// Per-file name resolution data produced by `mago-names`.
    ///
    /// Maps a file URI to an [`OwnedResolvedNames`](names::OwnedResolvedNames)
    /// that provides byte-offset → FQN lookups for every identifier in the
    /// file.  Populated during `update_ast_inner` for files that are open
    /// in the editor.  Not populated for vendor/stub files loaded via
    /// `parse_and_cache_content_versioned` (those files are never queried
    /// by byte offset).
    pub(crate) resolved_names: Arc<RwLock<HashMap<String, Arc<names::OwnedResolvedNames>>>>,
    /// Maps a file URI to the namespace blocks declared in it.
    ///
    /// Each entry is a list of [`NamespaceSpan`] covering the byte ranges of
    /// the namespace blocks in the file.  Single-namespace files have exactly
    /// one entry; multi-namespace files (using `namespace Foo { }` blocks)
    /// have one entry per block.
    pub(crate) file_namespaces: Arc<RwLock<HashMap<String, Vec<NamespaceSpan>>>>,
    /// Global function definitions indexed by function name (short name).
    ///
    /// The value is `(file_uri, FunctionInfo)` so we can jump to the definition.
    /// Populated from files listed in Composer's `autoload_files.php` at init
    /// time, and also from any opened/changed files that contain standalone
    /// function declarations.
    /// Function names are case-insensitive in PHP, so the map folds
    /// keys to lowercase while preserving the declared spelling.
    pub(crate) global_functions: Arc<RwLock<CiMap<(String, FunctionInfo)>>>,
    /// Global constants defined via `define('NAME', value)` calls or
    /// top-level `const NAME = value;` statements.
    ///
    /// Maps constant name → [`DefineInfo`] containing the file URI,
    /// byte offset of the definition, and the initializer value text.
    ///
    /// Populated from files listed in Composer's `autoload_files.php` at
    /// init time, and also from any opened/changed files that contain
    /// `define()` calls or `const` statements.  Used for constant name
    /// completions, hover (showing the value), and go-to-definition.
    pub(crate) global_defines: Arc<RwLock<HashMap<String, DefineInfo>>>,
    /// Per-URI record of the standalone-function FQNs and `define()`/`const`
    /// names contributed to [`global_functions`](Self::global_functions) and
    /// [`global_defines`](Self::global_defines) by the most recent parse of
    /// each file.
    ///
    /// Value is `(function_fqns, define_names)`.  On re-parse this lets
    /// [`update_ast`](Self::update_ast) evict the symbols an edit deleted or
    /// renamed in `O(old + new)` — a targeted eviction analogous to the
    /// `old_fqns` class eviction — instead of scanning the whole global maps
    /// on every keystroke.
    pub(crate) uri_globals_index: Arc<RwLock<HashMap<String, UriGlobals>>>,
    /// Autoload function index: function FQN → file path on disk.
    ///
    /// Populated by the lightweight `find_symbols` byte-level scan
    /// during initialization.  For non-Composer projects the full-scan
    /// walks all workspace files; for Composer projects it scans the
    /// files listed in `autoload_files.php` (and their `require_once`
    /// chains).  Maps standalone function names to the file that
    /// defines them so that [`find_or_load_function`] can lazily call
    /// `update_ast` on first access instead of eagerly parsing every
    /// file at startup.
    pub(crate) autoload_function_index: Arc<RwLock<CiMap<PathBuf>>>,
    /// Completion provenance for autoloaded function symbols.
    pub(crate) autoload_function_origin_index: Arc<RwLock<CiMap<ClassCompletionOrigin>>>,
    /// Autoload constant index: constant name → file path on disk.
    ///
    /// Populated alongside `autoload_function_index` by the
    /// `find_symbols` byte-level scan during initialization.  Maps
    /// `define()` constants and top-level `const` declarations to
    /// the file that defines them for lazy resolution via
    /// `update_ast` on first access.
    pub(crate) autoload_constant_index: Arc<RwLock<HashMap<String, PathBuf>>>,
    /// Completion provenance for autoloaded constant symbols.
    pub(crate) autoload_constant_origin_index: Arc<RwLock<HashMap<String, ClassCompletionOrigin>>>,
    /// Paths of all files discovered through Composer's
    /// `autoload_files.php` (and their `require_once` chains).
    ///
    /// The byte-level `find_symbols` scanner only discovers top-level
    /// function and constant declarations.  Functions wrapped in
    /// `if (! function_exists(...))` guards (common in Laravel
    /// helpers) are at brace depth 1 and are missed by the scanner.
    /// This list is the safety net: when `find_or_load_function` or
    /// `resolve_constant_definition` cannot find a symbol in any
    /// index or stubs, it lazily parses each of these files via
    /// `update_ast` until the symbol is found.  Each file is parsed
    /// at most once (subsequent lookups hit `global_functions` /
    /// `global_defines`).
    pub(crate) autoload_file_paths: Arc<RwLock<Vec<PathBuf>>>,
    /// Index of fully-qualified class names to file URIs.
    ///
    /// This allows reliable lookup of classes that don't follow PSR-4
    /// conventions, e.g. classes defined in files listed by Composer's
    /// `autoload_files.php`.  The key is the FQN (e.g.
    /// `"Laravel\\Foundation\\Application"`) and the value is the file URI
    /// where the class is defined.
    ///
    /// Populated from four sources:
    /// - `update_ast` (using the file's namespace + class short name)
    ///   whenever a file is opened or changed.
    /// - The `find_symbols` byte-level scan of Composer autoload files
    ///   during server initialization (so classes in autoload files are
    ///   discoverable by `find_or_load_class` without an eager AST parse).
    /// - The workspace full-scan for non-Composer projects.
    /// - Entries from Composer's `autoload_classmap.php` (merged during
    ///   server initialization).
    pub(crate) fqn_uri_index: Arc<RwLock<CiMap<String>>>,
    /// Completion provenance for fully-qualified class names.
    ///
    /// Used only for ranking class-name completion candidates.  Tracks
    /// whether a class comes from project code, core/stubs, an explicit
    /// Composer dependency, or a transitive vendor dependency.
    pub(crate) fqn_origin_index: Arc<RwLock<CiMap<ClassCompletionOrigin>>>,
    /// Secondary index mapping fully-qualified class names directly to
    /// their parsed `ClassInfo`.
    ///
    /// This turns every Phase 1 lookup in [`find_or_load_class`] into an
    /// O(1) hash lookup instead of scanning all files in `uri_classes_index`.
    /// Maintained alongside `fqn_uri_index` in `update_ast_inner` and
    /// `parse_and_cache_content_versioned`.
    pub(crate) fqn_class_index: Arc<RwLock<CiMap<Arc<ClassInfo>>>>,
    /// Negative-result cache for [`find_or_load_class`].
    ///
    /// Stores fully-qualified class names that have been looked up and
    /// confirmed not to exist in any resolution phase (fqn_class_index,
    /// fqn_uri_index, PSR-4, stubs).  Subsequent lookups for the same name
    /// short-circuit with `None` instead of repeating the full
    /// multi-phase search.
    ///
    /// Entries are removed when new classes are discovered (in
    /// `update_ast_inner` and `parse_and_cache_content_versioned`) so
    /// that a class which becomes available after lazy loading is not
    /// permanently suppressed.
    pub(crate) class_not_found_cache: Arc<RwLock<CiSet>>,
    /// Parsed phar archives keyed by the phar file's absolute path.
    ///
    /// Populated during Composer autoload scanning when a bootstrap file
    /// references a `.phar` archive (e.g. PHPStan's `bootstrap.php`).
    /// Used by [`parse_and_cache_file`](Self::parse_and_cache_file) to
    /// extract PHP source files from inside the archive when the
    /// fqn_uri_index contains a phar-based path (detected by a `!` separator,
    /// e.g. `/path/to/phpstan.phar!src/Type/Type.php`).
    pub(crate) phar_archives: Arc<RwLock<HashMap<PathBuf, phar::PharArchive>>>,
    /// Set of file URIs that have been fully parsed at least once.
    ///
    /// Used as a lightweight "has this file been parsed?" check by
    /// consumers that need to skip redundant re-parsing (e.g.
    /// `find_or_load_function`, `resolve_constant_definition`,
    /// `find_implementors`).  Populated in `update_ast_inner` and
    /// `parse_and_cache_content_versioned`.
    pub(crate) parsed_uris: Arc<RwLock<HashSet<String>>>,
    /// Set of file URIs currently being parsed by another thread.
    ///
    /// Used by [`parse_and_cache_file`](Self::parse_and_cache_file) to avoid
    /// redundant concurrent parses of the same file.  Before parsing, the URI
    /// is claimed; if another thread already holds the claim, the calling
    /// thread blocks until that parse completes and then reads the result
    /// from `uri_classes_index` instead of re-parsing.
    pub(crate) parse_inflight: Arc<resolution::ParseInflight>,
    /// Embedded PHP stubs for built-in classes/interfaces (e.g. `UnitEnum`,
    /// `BackedEnum`, `Iterator`, `Countable`, …).
    /// Maps class short name → raw PHP source code.
    ///
    /// Built once during construction via [`stubs::build_stub_class_index`].
    /// Filtered at startup via [`set_php_version`](Self::set_php_version) to
    /// remove stubs that do not exist in the target PHP version.
    /// Consulted by `find_or_load_class` as a final fallback after the
    /// `uri_classes_index` and PSR-4 resolution.  Stub files are parsed lazily on
    /// first access and cached in `uri_classes_index` under `phpantom-stub://` URIs.
    pub(crate) stub_index: RwLock<CiMap<&'static str>>,
    /// Cache of fully-resolved classes (inheritance + virtual members).
    ///
    /// Keyed by fully-qualified class name.  Populated lazily by
    /// [`resolve_class_fully_cached`](crate::virtual_members::resolve_class_fully_cached)
    /// and cleared whenever a file is re-parsed (`update_ast` /
    /// `parse_and_cache_content`) so that stale results never survive
    /// an edit.
    pub(crate) resolved_class_cache: virtual_members::ResolvedClassCache,
    /// Memoized authenticated-user model type, derived from `config/auth.php`.
    ///
    /// Keyed by guard name (an empty string denotes the default guard).
    /// Populated lazily when an auth-user access is resolved and cleared
    /// whenever files are re-parsed, so edits to `config/auth.php` take
    /// effect without a restart.
    pub(crate) auth_user_type_cache: Arc<RwLock<HashMap<String, Option<crate::php_type::PhpType>>>>,
    /// Memoized Laravel alias tables, parsed from the installed framework
    /// source (`registerCoreContainerAliases()`, `Facade::defaultAliases()`)
    /// and the project's `config/app.php`.
    ///
    /// `None` means "not yet computed"; an inner empty map means "computed and
    /// this project has no such aliases" (e.g. a non-Laravel project). Cleared
    /// whenever files are re-parsed so edits to `config/app.php` take effect
    /// without a restart.
    pub(crate) laravel_aliases: Arc<RwLock<Option<Arc<virtual_members::laravel::LaravelAliases>>>>,
    /// Laravel `Target::macro('name', closure)` registrations discovered from
    /// project source, keyed by the FQN of the class each macro attaches to.
    ///
    /// Built during `initialized` for Laravel projects and refreshed when a
    /// contributing file changes.  Consulted when a class is loaded so that
    /// macro methods appear in completion, hover, and signature help.  Empty
    /// for non-Laravel projects.
    pub(crate) laravel_macros: Arc<RwLock<virtual_members::laravel::LaravelMacroIndex>>,
    /// Fast gate for [`laravel_macros`](Self::laravel_macros): `true` only
    /// when the index holds at least one macro, so the hot class-load path
    /// skips the lock entirely for the common (no-macro) case.
    pub(crate) laravel_has_macros: Arc<std::sync::atomic::AtomicBool>,
    /// Laravel macro seed files (service providers plus the app's provider
    /// registration files), mapped to the class references each contributed
    /// at the last macro-index build.  An edit that changes a seed's
    /// references triggers a full index rebuild; every other edit takes the
    /// cheap single-file refresh path.
    pub(crate) laravel_macro_seeds: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Concrete date class selected by the project's `Date::use()` call.
    ///
    /// The outer option is `None` until startup discovery completes; the inner
    /// option is `None` when discovery found no project override.
    pub(crate) laravel_date_class: Arc<RwLock<Option<Option<String>>>>,
    /// URIs whose edits can change the configured date class: every registered
    /// service provider scanned by [`build_laravel_date_class`], plus the app's
    /// provider-registration files (which decide *which* providers are
    /// registered).  Populated by that scan and consulted by the single-file
    /// refresh so a `Date::use()` added, changed, or removed in one of these
    /// files re-runs the full scan, while an edit to any other file is ignored.
    ///
    /// [`build_laravel_date_class`]: crate::Backend::build_laravel_date_class
    pub(crate) laravel_date_seed_uris: Arc<RwLock<std::collections::HashSet<String>>>,
    /// Cached Laravel string key enumerations (route names, config keys,
    /// view names, translation keys).  `None` = not yet computed.
    /// Invalidated when a file in `routes/`, `config/`, `resources/views/`,
    /// or `lang/` is updated.
    pub(crate) laravel_provider_resources: Arc<RwLock<virtual_members::laravel::ProviderResources>>,
    pub(crate) laravel_string_key_cache: Arc<RwLock<LaravelStringKeyCache>>,
    /// Per-target member completion cache.
    ///
    /// Typing `$model->wh...` triggers a completion request for each
    /// keyword edit. The receiver and candidate member set are unchanged
    /// across those requests, so cache the unfiltered member list and let
    /// each request apply only its current prefix filter.
    pub(crate) member_completion_cache: Arc<Mutex<HashMap<String, Vec<CompletionItem>>>>,
    /// Global method store: `(class_fqn, method_name)` → `Arc<MethodInfo>`.
    ///
    /// Populated alongside `fqn_index` whenever classes are parsed or
    /// loaded.  Currently a read-only mirror of the methods already stored
    /// inside `ClassInfo.methods`; future phases will make this the
    /// authoritative source and shrink `ClassInfo.methods` to just names.
    pub(crate) method_store: types::MethodStore,
    /// Reverse inheritance index: parent FQN → list of child FQNs.
    ///
    /// For each class/interface/trait, maps the FQNs of its parents
    /// (parent_class, interfaces, used_traits) to the child's FQN.
    /// Used by `find_implementors` for O(1) lookup of direct children
    /// instead of scanning all `uri_classes_index` entries.
    ///
    /// Populated incrementally in `update_ast_inner` and
    /// `parse_and_cache_content_versioned` as files are parsed.
    pub(crate) gti_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Embedded PHP stubs for built-in functions (e.g. `array_map`,
    /// `str_contains`, …).  Maps function name → raw PHP source code.
    ///
    /// Built once during construction via [`stubs::build_stub_function_index`].
    /// Filtered at startup via [`set_php_version`](Self::set_php_version) to
    /// remove stubs that do not exist in the target PHP version.
    /// Can be consulted to resolve return types of built-in function calls.
    pub(crate) stub_function_index: RwLock<CiMap<&'static str>>,
    /// Embedded PHP stubs for built-in constants (e.g. `PHP_EOL`,
    /// `SORT_ASC`, …).  Maps constant name → raw PHP source code.
    ///
    /// Built once during construction via [`stubs::build_stub_constant_index`].
    /// Filtered at startup via [`set_php_version`](Self::set_php_version) to
    /// remove stubs that do not exist in the target PHP version.
    /// Can be consulted when resolving standalone constant references.
    pub(crate) stub_constant_index: RwLock<HashMap<&'static str, &'static str>>,
    /// The target PHP version used for version-aware stub filtering.
    ///
    /// Detected from `composer.json` (`require.php`) during server
    /// initialization.  When no version constraint is found, defaults
    /// to PHP 8.5.  Stub elements annotated with
    /// `#[PhpStormStubsElementAvailable]` are filtered against this
    /// version so that only the correct variant is presented.
    ///
    /// Wrapped in a `Mutex` so that `set_php_version` can be called
    /// during `initialized` (which receives `&self`, not `&mut self`).
    pub(crate) php_version: Mutex<types::PhpVersion>,
    // NOTE: php_version, vendor_uri_prefixes, vendor_dir_paths, config,
    // and diag_pending_uris use parking_lot::Mutex (not RwLock) because
    // they are rarely accessed or always written.
    /// `file://` URI prefixes for all known vendor directories, used to
    /// skip diagnostics, find references, and rename for vendor files.
    ///
    /// Built during `initialized` from the workspace root and
    /// `composer.json`'s `config.vendor-dir` (default `"vendor"`).
    /// Example: `["file:///home/user/project/vendor/"]`.
    ///
    /// In monorepo mode, contains one prefix per discovered subproject
    /// vendor directory.  When empty, vendor-skipping is disabled.
    pub(crate) vendor_uri_prefixes: Mutex<Vec<String>>,
    /// Absolute paths of all known vendor directories.
    ///
    /// Cached during `initialized` so that cross-file scans (find
    /// references, go-to-implementation) can skip vendor directories
    /// without re-reading `composer.json` on every request.
    ///
    /// In monorepo mode, contains one path per discovered subproject
    /// vendor directory.  For single-project workspaces, contains
    /// exactly one entry.
    pub(crate) vendor_dir_paths: Mutex<Vec<PathBuf>>,
    /// Canonical vendor package roots paired with completion provenance.
    ///
    /// Used to classify function/constant/class symbols by whether they
    /// originate from explicit or transitive Composer dependencies.
    pub(crate) vendor_package_origin_roots:
        Arc<RwLock<Vec<(PathBuf, ClassCompletionOrigin, String)>>>,
    /// Monotonically increasing version counter for diagnostic debouncing.
    ///
    /// Bumped on every `did_change`.  A background diagnostic task
    /// checks this counter after a quiet period and only publishes
    /// results when the counter hasn't moved, meaning the user
    /// stopped typing.
    pub(crate) diag_version: Arc<AtomicU64>,
    /// Notification handle used to wake the diagnostic worker task.
    ///
    /// [`schedule_diagnostics`](Self::schedule_diagnostics) calls
    /// `notify_one()` after bumping `diag_version`; the worker awaits
    /// `notified()` in its main loop.
    pub(crate) diag_notify: Arc<tokio::sync::Notify>,
    /// File URIs that need a diagnostic pass, set by
    /// [`schedule_diagnostics`](Self::schedule_diagnostics) and consumed
    /// by the diagnostic worker.  When a class signature changes, all
    /// open files are queued so that cross-file diagnostics (unknown
    /// member, unknown class, deprecated usage) are refreshed.
    ///
    /// Wrapped in `Arc` so the diagnostic worker task (spawned during
    /// `initialized`) shares the same slot as the main `Backend`.
    ///
    /// A `HashSet` deduplicates queued URIs in O(1); the worker drains
    /// the whole set on each wake, so insertion order does not matter.
    pub(crate) diag_pending_uris: Arc<Mutex<HashSet<String>>>,
    /// Last-published slow diagnostics (unknown classes, unknown members, etc.)
    /// per file URI.  Used by the two-phase diagnostic publisher: the fast
    /// phase merges fresh fast diagnostics with the previous slow diagnostics
    /// so the editor never shows a flicker where slow diagnostics disappear
    /// and then reappear.
    pub(crate) diag_last_slow: Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>>,
    /// Last-computed fast diagnostics (syntax errors, unused imports,
    /// unused variables) per file URI.  Used by `assemble_and_push` to
    /// merge fast results with other source caches without recomputing.
    pub(crate) diag_last_fast: Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>>,
    /// Notification handle used to wake the PHPStan worker task.
    ///
    /// The PHPStan worker is a dedicated background task, separate from
    /// the main diagnostic worker, because PHPStan is extremely slow
    /// and resource-intensive.  Running it in its own task ensures that
    /// native diagnostics (fast + slow phases) are never blocked by a
    /// PHPStan invocation that may take tens of seconds.
    ///
    /// At most one PHPStan process runs at a time.  If the user edits
    /// a file while PHPStan is running, the pending URI is updated and
    /// the worker picks it up after the current run finishes.
    pub(crate) phpstan_notify: Arc<tokio::sync::Notify>,
    /// The single file URI that the PHPStan worker should analyse next.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// edits rapidly, earlier requests are superseded.  This is
    /// intentional — PHPStan is too slow to queue up multiple files.
    pub(crate) phpstan_pending_uri: Arc<Mutex<Option<String>>>,
    /// Last-published PHPStan diagnostics per file URI.
    ///
    /// The fast and slow diagnostic phases merge these cached results
    /// into their publish calls so that PHPStan errors remain visible
    /// while the user edits (without waiting for a fresh PHPStan run).
    /// The PHPStan worker updates this cache after each successful run
    /// and triggers a re-publish of the affected file.
    pub(crate) phpstan_last_diags:
        Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>>,
    /// Notification handle used to wake the PHPCS worker task.
    ///
    /// The PHPCS worker is a dedicated background task, separate from
    /// the main diagnostic worker and the PHPStan worker, because PHPCS
    /// is an external process that can take several seconds.  Running it
    /// in its own task ensures that native diagnostics and PHPStan are
    /// never blocked.
    ///
    /// At most one PHPCS process runs at a time.  If the user edits
    /// a file while PHPCS is running, the pending URI is updated and
    /// the worker picks it up after the current run finishes.
    pub(crate) phpcs_notify: Arc<tokio::sync::Notify>,
    /// The single file URI that the PHPCS worker should analyse next.
    ///
    /// Only the most recent file is kept: if the user switches files or
    /// edits rapidly, earlier requests are superseded.  This is
    /// intentional — PHPCS is too slow to queue up multiple files.
    pub(crate) phpcs_pending_uri: Arc<Mutex<Option<String>>>,
    /// Last-published PHPCS diagnostics per file URI.
    ///
    /// The fast and slow diagnostic phases merge these cached results
    /// into their publish calls so that PHPCS errors remain visible
    /// while the user edits (without waiting for a fresh PHPCS run).
    /// The PHPCS worker updates this cache after each successful run
    /// and triggers a re-publish of the affected file.
    pub(crate) phpcs_last_diags: Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>>,
    /// Notification handle used to wake the Mago lint worker task.
    pub(crate) mago_lint_notify: Arc<tokio::sync::Notify>,
    /// The single file URI that the Mago lint worker should analyse next.
    pub(crate) mago_lint_pending_uri: Arc<Mutex<Option<String>>>,
    /// Last-published Mago lint diagnostics per file URI.
    pub(crate) mago_lint_last_diags:
        Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>>,
    /// Notification handle used to wake the Mago analyze worker task.
    pub(crate) mago_analyze_notify: Arc<tokio::sync::Notify>,
    /// The single file URI that the Mago analyze worker should analyse next.
    pub(crate) mago_analyze_pending_uri: Arc<Mutex<Option<String>>>,
    /// Last-published Mago analyze diagnostics per file URI.
    pub(crate) mago_analyze_last_diags:
        Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>>,
    /// Per-file `resultId` for pull diagnostics (`textDocument/diagnostic`).
    ///
    /// Maps file URI → monotonically increasing counter.  Bumped whenever
    /// the diagnostics for a file change (on every `did_change` or when
    /// PHPStan finishes).  The client sends the previous `resultId` back
    /// in the next pull request; if it matches, the server returns
    /// `Unchanged` instead of recomputing.
    pub(crate) diag_result_ids: Arc<Mutex<HashMap<String, u64>>>,
    /// Combined diagnostic cache for pull diagnostics.
    ///
    /// Stores the last-computed full diagnostic set (fast + slow + PHPStan + PHPCS)
    /// per file URI.  When the client pulls diagnostics, the server
    /// returns this cached set.  Updated by the background diagnostic
    /// worker after each pass and by the PHPStan worker after each run.
    pub(crate) diag_last_full: Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>>,
    /// Diagnostics to suppress from the next publish cycle.
    ///
    /// When a `codeAction/resolve` handler eagerly clears a diagnostic
    /// (e.g. an unused-import warning), it pushes the diagnostic here.
    /// The next `publish_diagnostics_for_file` call filters these out
    /// before sending to the client, then clears the set.  This lets
    /// the squiggly line disappear before the text edit is applied.
    pub(crate) diag_suppressed: Arc<Mutex<Vec<tower_lsp::lsp_types::Diagnostic>>>,
    /// Whether the client supports pull diagnostics.
    ///
    /// Set during `initialize` based on the client's
    /// `textDocument.diagnostic` capability.  When `true`, the server
    /// uses pull diagnostics (`textDocument/diagnostic`) as the primary
    /// path and sends `workspace/diagnostic/refresh` instead of
    /// `schedule_diagnostics_for_open_files`.  When `false`, the server
    /// falls back to the push model (`textDocument/publishDiagnostics`).
    pub(crate) supports_pull_diagnostics: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client supports file rename operations in workspace edits.
    ///
    /// Set during `initialize` based on the client's
    /// `workspace.workspaceEdit.resourceOperations` capability.  When `true`
    /// and a class rename matches PSR-4 naming (filename == class name),
    /// the rename response includes a `RenameFile` operation alongside the
    /// text edits so the file is renamed to match the new class name.
    pub(crate) supports_file_rename: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client supports server-initiated work-done progress.
    ///
    /// Set during `initialize` based on the client's
    /// `window.workDoneProgress` capability.  When `false`, the server
    /// must not send `window/workDoneProgress/create` requests because
    /// the client will not handle them, blocking the server indefinitely.
    pub(crate) supports_work_done_progress: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client supports dynamic registration for type hierarchy.
    pub(crate) supports_type_hierarchy_dynamic_registration: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client supports `workspace/semanticTokens/refresh`.
    ///
    /// Set during `initialize` based on the client's
    /// `workspace.semanticTokens.refreshSupport` capability.  When `true`,
    /// the server asks the client to re-pull semantic tokens after a
    /// background `didChange` parse commits a new symbol map — without
    /// this, editors keep showing tokens computed from the pre-edit
    /// symbol map until the next unrelated request.
    pub(crate) supports_semantic_tokens_refresh: Arc<std::sync::atomic::AtomicBool>,
    /// Shared flag set to `true` when the LSP `shutdown` request is
    /// received.  Background workers (diagnostic, PHPStan, PHPCS) check this
    /// flag on each iteration and exit their loops.  The PHPStan
    /// `run_command_with_timeout` poll loop also checks it so that a
    /// running child process is killed promptly instead of waiting up
    /// to 60 seconds.
    /// Set to `true` once `initialized` finishes indexing (PSR-4,
    /// classmap, stubs, vendor).  Background workers and the pull
    /// diagnostic handler check this flag before running diagnostics
    /// so that files opened during startup don't produce a flood of
    /// false-positive "class not found" / "function not found" errors.
    pub(crate) init_complete: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) shutdown_flag: Arc<std::sync::atomic::AtomicBool>,
    // NOTE: resolved_class_cache uses parking_lot::Mutex because it is
    // frequently written (cache stores) and RwLock read→write upgrades
    // are error-prone.
    /// Per-project configuration loaded from `.phpantom.toml`.
    ///
    /// Read once during `initialized` from the workspace root directory.
    /// When the file is missing or cannot be parsed, all settings use
    /// their defaults.  Wrapped in a `Mutex` so that `initialized`
    /// (which receives `&self`) can set it after loading the file.
    /// The diagnostic worker snapshots the value at spawn time.
    pub(crate) config: Mutex<config::Config>,
    /// Virtual PHP content generated from Blade files.
    pub(crate) blade_virtual_content: Arc<RwLock<HashMap<String, String>>>,
    /// Source maps from virtual PHP back to original Blade positions.
    pub(crate) blade_source_maps:
        Arc<RwLock<HashMap<String, crate::blade::source_map::BladeSourceMap>>>,
    /// URIs opened with `languageId == "blade"` that don't have a `.blade.php` extension.
    /// Allows editors to signal Blade files via languageId alone.
    pub(crate) blade_uris: Arc<RwLock<std::collections::HashSet<String>>>,
    /// Whether the workspace directory has been fully scanned for PHP files.
    ///
    /// Set to `true` after the first Phase 2 walk in `ensure_workspace_indexed`.
    /// Subsequent calls still re-walk the directory to discover newly created
    /// files, but the flag lets us log the difference between initial and
    /// refresh scans.
    pub(crate) workspace_indexed: Arc<std::sync::atomic::AtomicBool>,
    /// Serializes whole-workspace indexing so a foreground request does not
    /// duplicate the background full-index parse.
    pub(crate) workspace_index_lock: Arc<Mutex<()>>,
    /// Prevents duplicate background full-index tasks when initialization and
    /// a request both race to parse the whole workspace.
    pub(crate) full_index_in_progress: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClassCompletionOrigin {
    #[default]
    Project,
    CoreStub,
    VendorExplicit,
    VendorTransitive,
}

impl ClassCompletionOrigin {
    pub(crate) fn sort_tier(self) -> char {
        match self {
            Self::Project => '0',
            Self::CoreStub => '1',
            Self::VendorExplicit => '2',
            Self::VendorTransitive => '3',
        }
    }
}

/// Request-coalescing state for expensive whole-file requests (semantic
/// tokens, code lens, document symbols, folding, links).
///
/// Editors re-issue these on every keystroke and cancel the superseded ones,
/// but a `spawn_blocking` computation cannot be aborted once it starts, so a
/// fast typist piles up many full-file scans (hundreds of ms each) that all
/// run to completion and saturate every CPU core, starving the cheap
/// interactive requests (completion, hover) until the user gives up waiting.
///
/// This coalesces by `(kind, uri)`: a global sequence stamps each request,
/// a per-key async lock serialises computation so at most one runs per kind
/// per file, and any request that finds itself no longer the latest when it
/// acquires the lock short-circuits to the previous result instead of redoing
/// the scan. A burst of N requests therefore performs at most two scans (the
/// one already running plus the newest) rather than N.
#[derive(Default)]
pub(crate) struct WholeFileCoalesce {
    /// Monotonic request counter shared across all keys.
    seq: AtomicU64,
    /// Latest request sequence seen per `"{kind}\0{uri}"` key.
    latest: Mutex<HashMap<String, u64>>,
    /// Per-key serialisation lock (async, held across the blocking compute).
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Last successfully computed result per key, returned to superseded
    /// requests so the editor never briefly sees an empty result.
    last: Mutex<HashMap<String, Arc<dyn std::any::Any + Send + Sync>>>,
}

impl WholeFileCoalesce {
    /// Get (or create) the per-key async serialisation lock. Held across the
    /// blocking compute so at most one computation per key runs at a time.
    pub(crate) fn key_lock(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock();
        Arc::clone(
            locks
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    /// Stamp a request with the next sequence number and record it as the
    /// latest for `key`. Returns the stamped sequence.
    pub(crate) fn stamp(&self, key: &str) -> u64 {
        let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.latest.lock().insert(key.to_string(), seq);
        seq
    }

    /// Whether `seq` is still the latest request stamped for `key`.
    pub(crate) fn is_latest(&self, key: &str, seq: u64) -> bool {
        self.latest.lock().get(key).copied() == Some(seq)
    }

    /// Read the cached last result for `key`, if any.
    pub(crate) fn last_result(&self, key: &str) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.last.lock().get(key).cloned()
    }

    /// Store the last computed result for `key`.
    pub(crate) fn store_result(&self, key: &str, value: Arc<dyn std::any::Any + Send + Sync>) {
        self.last.lock().insert(key.to_string(), value);
    }
}

impl Backend {
    /// Shared defaults for all Backend constructors.
    ///
    /// Returns a `Backend` with no LSP client, empty maps, and the full
    /// embedded stub indices.  Each public constructor customises only the
    /// fields that differ.
    ///
    /// **Note:** This loads the full embedded stub indices (1,455 classes,
    /// 5,023 functions, 8,119 constants).  Test code should use
    /// [`test_defaults`] instead, which leaves stubs empty.
    fn defaults() -> Self {
        Self {
            name: "PHPantom".to_string(),
            version: env!("PHPANTOM_GIT_VERSION").to_string(),
            client_name: Mutex::new(String::new()),
            open_files: Arc::new(RwLock::new(HashMap::new())),
            uri_classes_index: Arc::new(RwLock::new(HashMap::new())),
            symbol_maps: Arc::new(RwLock::new(HashMap::new())),
            reference_index: reference_index::new_reference_index(),
            parse_errors: Arc::new(RwLock::new(HashMap::new())),
            did_change_parse_locks: Arc::new(Mutex::new(HashMap::new())),
            whole_file_coalesce: Arc::new(WholeFileCoalesce::default()),
            client: None,
            workspace_root: Arc::new(RwLock::new(None)),
            vendor_uri_prefixes: Mutex::new(Vec::new()),
            vendor_dir_paths: Mutex::new(Vec::new()),
            vendor_package_origin_roots: Arc::new(RwLock::new(Vec::new())),
            psr4_mappings: Arc::new(RwLock::new(Vec::new())),
            file_imports: Arc::new(RwLock::new(HashMap::new())),
            resolved_names: Arc::new(RwLock::new(HashMap::new())),
            file_namespaces: Arc::new(RwLock::new(HashMap::new())),
            global_functions: Arc::new(RwLock::new(CiMap::new())),
            global_defines: Arc::new(RwLock::new(HashMap::new())),
            uri_globals_index: Arc::new(RwLock::new(HashMap::new())),
            autoload_function_index: Arc::new(RwLock::new(CiMap::new())),
            autoload_function_origin_index: Arc::new(RwLock::new(CiMap::new())),
            autoload_constant_index: Arc::new(RwLock::new(HashMap::new())),
            autoload_constant_origin_index: Arc::new(RwLock::new(HashMap::new())),
            autoload_file_paths: Arc::new(RwLock::new(Vec::new())),
            fqn_uri_index: Arc::new(RwLock::new(CiMap::new())),
            fqn_origin_index: Arc::new(RwLock::new(CiMap::new())),
            fqn_class_index: Arc::new(RwLock::new(CiMap::new())),
            class_not_found_cache: Arc::new(RwLock::new(CiSet::new())),
            phar_archives: Arc::new(RwLock::new(HashMap::new())),
            parsed_uris: Arc::new(RwLock::new(HashSet::new())),
            parse_inflight: Arc::new(resolution::ParseInflight::new()),
            stub_index: RwLock::new(CiMap::from(stubs::build_stub_class_index())),
            stub_function_index: RwLock::new(CiMap::from(stubs::build_stub_function_index())),
            stub_constant_index: RwLock::new(stubs::build_stub_constant_index()),
            resolved_class_cache: virtual_members::new_resolved_class_cache(),
            auth_user_type_cache: Arc::new(RwLock::new(HashMap::new())),
            laravel_aliases: Arc::new(RwLock::new(None)),
            laravel_macros: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelMacroIndex::default(),
            )),
            laravel_has_macros: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_macro_seeds: Arc::new(RwLock::new(HashMap::new())),
            laravel_date_class: Arc::new(RwLock::new(None)),
            laravel_date_seed_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            laravel_provider_resources: Arc::new(RwLock::new(
                virtual_members::laravel::ProviderResources::default(),
            )),
            laravel_string_key_cache: Arc::new(RwLock::new(LaravelStringKeyCache::default())),
            member_completion_cache: Arc::new(Mutex::new(HashMap::new())),
            method_store: Arc::new(RwLock::new(HashMap::new())),
            gti_index: Arc::new(RwLock::new(HashMap::new())),
            php_version: Mutex::new(types::PhpVersion::default()),
            diag_version: Arc::new(AtomicU64::new(0)),
            diag_notify: Arc::new(tokio::sync::Notify::new()),
            diag_pending_uris: Arc::new(Mutex::new(HashSet::new())),
            diag_last_slow: Arc::new(Mutex::new(HashMap::new())),
            diag_last_fast: Arc::new(Mutex::new(HashMap::new())),
            phpstan_notify: Arc::new(tokio::sync::Notify::new()),
            phpstan_pending_uri: Arc::new(Mutex::new(None)),
            phpstan_last_diags: Arc::new(Mutex::new(HashMap::new())),
            phpcs_notify: Arc::new(tokio::sync::Notify::new()),
            phpcs_pending_uri: Arc::new(Mutex::new(None)),
            phpcs_last_diags: Arc::new(Mutex::new(HashMap::new())),
            mago_lint_notify: Arc::new(tokio::sync::Notify::new()),
            mago_lint_pending_uri: Arc::new(Mutex::new(None)),
            mago_lint_last_diags: Arc::new(Mutex::new(HashMap::new())),
            mago_analyze_notify: Arc::new(tokio::sync::Notify::new()),
            mago_analyze_pending_uri: Arc::new(Mutex::new(None)),
            mago_analyze_last_diags: Arc::new(Mutex::new(HashMap::new())),
            diag_result_ids: Arc::new(Mutex::new(HashMap::new())),
            diag_last_full: Arc::new(Mutex::new(HashMap::new())),

            diag_suppressed: Arc::new(Mutex::new(Vec::new())),
            supports_pull_diagnostics: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_file_rename: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_work_done_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_type_hierarchy_dynamic_registration: Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ),
            supports_semantic_tokens_refresh: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            init_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutdown_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            config: Mutex::new(config::Config::default()),
            blade_virtual_content: Arc::new(RwLock::new(HashMap::new())),
            blade_source_maps: Arc::new(RwLock::new(HashMap::new())),
            blade_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            workspace_indexed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workspace_index_lock: Arc::new(Mutex::new(())),
            full_index_in_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            sync_ast_updates: false,
        }
    }

    /// Shared defaults for test Backend constructors.
    ///
    /// Identical to [`defaults`] but with **empty** stub indices, avoiding
    /// the cost of building three large `HashMap`s (14,597 entries total)
    /// that most tests never consult.  Tests that need specific stubs
    /// override the relevant fields after construction.
    fn test_defaults() -> Self {
        Self {
            name: "PHPantom".to_string(),
            version: env!("PHPANTOM_GIT_VERSION").to_string(),
            client_name: Mutex::new(String::new()),
            open_files: Arc::new(RwLock::new(HashMap::new())),
            uri_classes_index: Arc::new(RwLock::new(HashMap::new())),
            symbol_maps: Arc::new(RwLock::new(HashMap::new())),
            reference_index: reference_index::new_reference_index(),
            parse_errors: Arc::new(RwLock::new(HashMap::new())),
            did_change_parse_locks: Arc::new(Mutex::new(HashMap::new())),
            whole_file_coalesce: Arc::new(WholeFileCoalesce::default()),
            client: None,
            workspace_root: Arc::new(RwLock::new(None)),
            vendor_uri_prefixes: Mutex::new(Vec::new()),
            vendor_dir_paths: Mutex::new(Vec::new()),
            vendor_package_origin_roots: Arc::new(RwLock::new(Vec::new())),
            psr4_mappings: Arc::new(RwLock::new(Vec::new())),
            file_imports: Arc::new(RwLock::new(HashMap::new())),
            resolved_names: Arc::new(RwLock::new(HashMap::new())),
            file_namespaces: Arc::new(RwLock::new(HashMap::new())),
            global_functions: Arc::new(RwLock::new(CiMap::new())),
            global_defines: Arc::new(RwLock::new(HashMap::new())),
            uri_globals_index: Arc::new(RwLock::new(HashMap::new())),
            autoload_function_index: Arc::new(RwLock::new(CiMap::new())),
            autoload_function_origin_index: Arc::new(RwLock::new(CiMap::new())),
            autoload_constant_index: Arc::new(RwLock::new(HashMap::new())),
            autoload_constant_origin_index: Arc::new(RwLock::new(HashMap::new())),
            autoload_file_paths: Arc::new(RwLock::new(Vec::new())),
            fqn_uri_index: Arc::new(RwLock::new(CiMap::new())),
            fqn_origin_index: Arc::new(RwLock::new(CiMap::new())),
            fqn_class_index: Arc::new(RwLock::new(CiMap::new())),
            class_not_found_cache: Arc::new(RwLock::new(CiSet::new())),
            phar_archives: Arc::new(RwLock::new(HashMap::new())),
            parsed_uris: Arc::new(RwLock::new(HashSet::new())),
            parse_inflight: Arc::new(resolution::ParseInflight::new()),
            stub_index: RwLock::new(CiMap::new()),
            stub_function_index: RwLock::new(CiMap::new()),
            stub_constant_index: RwLock::new(HashMap::new()),
            resolved_class_cache: virtual_members::new_resolved_class_cache(),
            auth_user_type_cache: Arc::new(RwLock::new(HashMap::new())),
            laravel_aliases: Arc::new(RwLock::new(None)),
            laravel_macros: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelMacroIndex::default(),
            )),
            laravel_has_macros: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_macro_seeds: Arc::new(RwLock::new(HashMap::new())),
            laravel_date_class: Arc::new(RwLock::new(None)),
            laravel_date_seed_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            laravel_provider_resources: Arc::new(RwLock::new(
                virtual_members::laravel::ProviderResources::default(),
            )),
            laravel_string_key_cache: Arc::new(RwLock::new(LaravelStringKeyCache::default())),
            member_completion_cache: Arc::new(Mutex::new(HashMap::new())),
            method_store: Arc::new(RwLock::new(HashMap::new())),
            gti_index: Arc::new(RwLock::new(HashMap::new())),
            php_version: Mutex::new(types::PhpVersion::default()),
            diag_version: Arc::new(AtomicU64::new(0)),
            diag_notify: Arc::new(tokio::sync::Notify::new()),
            diag_pending_uris: Arc::new(Mutex::new(HashSet::new())),
            diag_last_slow: Arc::new(Mutex::new(HashMap::new())),
            diag_last_fast: Arc::new(Mutex::new(HashMap::new())),
            phpstan_notify: Arc::new(tokio::sync::Notify::new()),
            phpstan_pending_uri: Arc::new(Mutex::new(None)),
            phpstan_last_diags: Arc::new(Mutex::new(HashMap::new())),
            phpcs_notify: Arc::new(tokio::sync::Notify::new()),
            phpcs_pending_uri: Arc::new(Mutex::new(None)),
            phpcs_last_diags: Arc::new(Mutex::new(HashMap::new())),
            mago_lint_notify: Arc::new(tokio::sync::Notify::new()),
            mago_lint_pending_uri: Arc::new(Mutex::new(None)),
            mago_lint_last_diags: Arc::new(Mutex::new(HashMap::new())),
            mago_analyze_notify: Arc::new(tokio::sync::Notify::new()),
            mago_analyze_pending_uri: Arc::new(Mutex::new(None)),
            mago_analyze_last_diags: Arc::new(Mutex::new(HashMap::new())),
            diag_result_ids: Arc::new(Mutex::new(HashMap::new())),
            diag_last_full: Arc::new(Mutex::new(HashMap::new())),
            diag_suppressed: Arc::new(Mutex::new(Vec::new())),
            supports_pull_diagnostics: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_file_rename: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_work_done_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_type_hierarchy_dynamic_registration: Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ),
            supports_semantic_tokens_refresh: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            init_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutdown_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            config: Mutex::new(config::Config::default()),
            blade_virtual_content: Arc::new(RwLock::new(HashMap::new())),
            blade_source_maps: Arc::new(RwLock::new(HashMap::new())),
            blade_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            workspace_indexed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workspace_index_lock: Arc::new(Mutex::new(())),
            full_index_in_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            sync_ast_updates: true,
        }
    }

    /// Create a new `Backend` connected to an LSP client.
    pub fn new(client: Client) -> Self {
        Self {
            client: Some(client),
            ..Self::defaults()
        }
    }

    /// Create a `Backend` without an LSP client but with full embedded
    /// stub indices.
    ///
    /// Use this for headless / CLI operation (e.g. the `analyze` command)
    /// where there is no LSP client but the backend still needs access to
    /// the PHP standard library stubs.
    pub fn new_headless() -> Self {
        Self::defaults()
    }

    /// Create a `Backend` without an LSP client (for unit / integration tests).
    ///
    /// Uses empty stub indices for fast construction.  Tests that need
    /// specific stubs should use [`new_test_with_stubs`] or
    /// [`new_test_with_all_stubs`] instead.
    pub fn new_test() -> Self {
        virtual_members::phpdoc::clear_mixin_cache();
        Self::test_defaults()
    }

    /// Create a `Backend` for tests that need the full embedded stub
    /// indices (e.g. benchmarks, end-to-end tests exercising real PHP
    /// stdlib classes).
    ///
    /// This is significantly slower than [`new_test`] because it builds
    /// three large `HashMap`s from the embedded phpstorm-stubs.  Only
    /// use this when the test specifically exercises stub-backed
    /// behaviour.
    pub fn new_test_with_full_stubs() -> Self {
        virtual_members::phpdoc::clear_mixin_cache();
        let backend = Self::defaults();
        backend.set_php_version(backend.php_version());
        backend
    }

    /// Create a `Backend` for tests with custom stub class index.
    ///
    /// This allows tests to inject minimal stub content (e.g. `UnitEnum`,
    /// `BackedEnum`) without depending on `composer install` having been run.
    pub fn new_test_with_stubs(stub_index: HashMap<&'static str, &'static str>) -> Self {
        virtual_members::phpdoc::clear_mixin_cache();
        let backend = Self {
            stub_index: RwLock::new(CiMap::from(stub_index)),
            ..Self::test_defaults()
        };
        backend.set_php_version(backend.php_version());
        backend
    }

    /// Create a `Backend` for tests with custom class, function, and constant
    /// stub indices.
    ///
    /// This allows tests to inject minimal stub content so that they are
    /// fully self-contained and do not depend on `composer install`.
    pub fn new_test_with_all_stubs(
        stub_index: HashMap<&'static str, &'static str>,
        stub_function_index: HashMap<&'static str, &'static str>,
        stub_constant_index: HashMap<&'static str, &'static str>,
    ) -> Self {
        virtual_members::phpdoc::clear_mixin_cache();
        let backend = Self {
            stub_index: RwLock::new(CiMap::from(stub_index)),
            stub_function_index: RwLock::new(CiMap::from(stub_function_index)),
            stub_constant_index: RwLock::new(stub_constant_index),
            ..Self::test_defaults()
        };
        backend.set_php_version(backend.php_version());
        backend
    }

    /// Create a `Backend` for tests with a specific workspace root and PSR-4
    /// mappings pre-configured.
    pub fn new_test_with_workspace(
        workspace_root: PathBuf,
        psr4_mappings: Vec<composer::Psr4Mapping>,
    ) -> Self {
        virtual_members::phpdoc::clear_mixin_cache();
        Self {
            workspace_root: Arc::new(RwLock::new(Some(workspace_root))),
            psr4_mappings: Arc::new(RwLock::new(psr4_mappings)),
            ..Self::test_defaults()
        }
    }

    // ── Public accessors for integration tests ──────────────────────────

    /// Borrow the workspace root mutex (used by integration tests to set a
    /// custom workspace directory).
    pub fn workspace_root(&self) -> &Arc<RwLock<Option<PathBuf>>> {
        &self.workspace_root
    }

    /// Borrow the global functions mutex (used by integration tests to
    /// inject user-defined functions or inspect the cache).
    pub fn global_functions(&self) -> &Arc<RwLock<CiMap<(String, FunctionInfo)>>> {
        &self.global_functions
    }

    /// Borrow the global defines mutex (used by integration tests to
    /// inject user-defined constants or inspect the cache).
    pub fn global_defines(&self) -> &Arc<RwLock<HashMap<String, DefineInfo>>> {
        &self.global_defines
    }

    /// Borrow the class index mutex (used by integration tests to
    /// populate discovered class entries).
    pub fn fqn_uri_index(&self) -> &Arc<RwLock<CiMap<String>>> {
        &self.fqn_uri_index
    }

    /// Borrow the FQN → ClassInfo index mutex (used by integration tests
    /// to populate class metadata for context-aware completion filtering).
    pub fn fqn_class_index(&self) -> &Arc<RwLock<CiMap<Arc<ClassInfo>>>> {
        &self.fqn_class_index
    }

    /// Borrow the PSR-4 mappings mutex (used by integration tests to
    /// configure autoload mappings).
    pub fn psr4_mappings(&self) -> &Arc<RwLock<Vec<composer::Psr4Mapping>>> {
        &self.psr4_mappings
    }

    /// Borrow the configured Laravel date class (used by integration tests
    /// to verify that provider edits keep the `Date::use()` selection
    /// current).  The outer option is `None` until startup discovery runs;
    /// the inner option is `None` when no project override was found.
    pub fn laravel_date_class(&self) -> &Arc<RwLock<Option<Option<String>>>> {
        &self.laravel_date_class
    }

    /// Borrow the set of parsed file URIs (used by integration tests to
    /// mark a workspace file as already loaded, mirroring a lazy parse).
    pub fn parsed_uris(&self) -> &Arc<RwLock<HashSet<String>>> {
        &self.parsed_uris
    }

    /// Read the stub constant index (used by integration tests to
    /// verify built-in constants are present).
    pub fn stub_constant_index(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, HashMap<&'static str, &'static str>> {
        self.stub_constant_index.read()
    }

    pub fn stub_function_index_mut(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, CiMap<&'static str>> {
        self.stub_function_index.write()
    }

    /// Write-access the stub constant index (used by integration tests
    /// to inject test stub entries).
    pub fn stub_constant_index_mut(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, HashMap<&'static str, &'static str>> {
        self.stub_constant_index.write()
    }

    /// Borrow the autoload function index (used by integration tests to
    /// populate discovered function entries for non-Composer projects).
    pub fn autoload_function_index(&self) -> &Arc<RwLock<CiMap<PathBuf>>> {
        &self.autoload_function_index
    }

    pub fn autoload_function_origin_index(&self) -> &Arc<RwLock<CiMap<ClassCompletionOrigin>>> {
        &self.autoload_function_origin_index
    }

    /// Borrow the autoload constant index (used by integration tests to
    /// populate discovered constant entries for non-Composer projects).
    pub fn autoload_constant_index(&self) -> &Arc<RwLock<HashMap<String, PathBuf>>> {
        &self.autoload_constant_index
    }

    pub fn autoload_constant_origin_index(
        &self,
    ) -> &Arc<RwLock<HashMap<String, ClassCompletionOrigin>>> {
        &self.autoload_constant_origin_index
    }

    /// Borrow the autoload file paths list (used by integration tests
    /// to simulate Composer autoload file discovery).
    pub fn autoload_file_paths(&self) -> &Arc<RwLock<Vec<PathBuf>>> {
        &self.autoload_file_paths
    }

    /// Borrow the open files map (used by integration tests to inject
    /// file content without going through the LSP `didOpen` path).
    pub fn open_files(&self) -> &Arc<RwLock<HashMap<String, Arc<String>>>> {
        &self.open_files
    }

    pub(crate) fn completion_origin_for_uri(&self, uri: &str) -> ClassCompletionOrigin {
        self.package_info_for_uri(uri).0
    }

    /// Return the completion origin **and** the Composer package name
    /// (e.g. `"laravel/framework"`) for the given file path.
    ///
    /// Returns `(ClassCompletionOrigin::Project, None)` for project files,
    /// `(ClassCompletionOrigin::CoreStub, None)` for stubs, and
    /// `(origin, Some(package_name))` for vendor files.
    pub(crate) fn package_info_for_path(
        &self,
        path: &Path,
    ) -> (ClassCompletionOrigin, Option<String>) {
        let vendor_paths = self.vendor_dir_paths.lock();
        let is_under_vendor = vendor_paths.iter().any(|vp| path.starts_with(vp));
        drop(vendor_paths);

        let roots = self.vendor_package_origin_roots.read();

        if is_under_vendor {
            for (root, origin, pkg_name) in roots.iter() {
                if path.starts_with(root) {
                    return (*origin, Some(pkg_name.clone()));
                }
            }
            return (ClassCompletionOrigin::VendorTransitive, None);
        }

        // Path is outside vendor/.  This is usually project code, but
        // it can also be a symlinked path-repository package whose
        // canonical path resolved outside vendor/.  Check whether any
        // vendor package root matches.  If so, treat it as project code
        // only when the root is inside the workspace (a local module);
        // otherwise show the package provenance.
        for (root, origin, pkg_name) in roots.iter() {
            if path.starts_with(root) {
                let ws = self.workspace_root.read();
                if let Some(ref workspace) = *ws {
                    let canonical_ws = workspace
                        .canonicalize()
                        .unwrap_or_else(|_| workspace.clone());
                    if root.starts_with(&canonical_ws) {
                        return (ClassCompletionOrigin::Project, None);
                    }
                }
                return (*origin, Some(pkg_name.clone()));
            }
        }

        (ClassCompletionOrigin::Project, None)
    }

    /// Return the completion origin **and** the Composer package name
    /// for the given file URI.
    pub(crate) fn package_info_for_uri(
        &self,
        uri: &str,
    ) -> (ClassCompletionOrigin, Option<String>) {
        if uri.starts_with("phpantom-stub://") || uri.starts_with("phpantom-stub-fn://") {
            return (ClassCompletionOrigin::CoreStub, None);
        }
        if let Ok(url) = tower_lsp::lsp_types::Url::parse(uri)
            && let Ok(path) = url.to_file_path()
        {
            return self.package_info_for_path(&path);
        }
        (ClassCompletionOrigin::Project, None)
    }

    /// Borrow the PHPStan diagnostics cache (used by integration tests
    /// to inject PHPStan diagnostics without running PHPStan).
    pub fn phpstan_last_diags(
        &self,
    ) -> &Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>> {
        &self.phpstan_last_diags
    }

    /// Borrow the PHPCS diagnostics cache (used by integration tests
    /// to inject PHPCS diagnostics without running PHPCS).
    pub fn phpcs_last_diags(
        &self,
    ) -> &Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>> {
        &self.phpcs_last_diags
    }

    /// Clear the member completion cache.
    pub fn clear_completion_cache(&self) {
        self.member_completion_cache.lock().clear();
    }

    /// Return the configured PHP version.
    pub fn php_version(&self) -> types::PhpVersion {
        *self.php_version.lock()
    }

    /// Populate the method store from a slice of classes.
    ///
    /// For each class, inserts every method under the key
    /// `(class_fqn, method.name)`.  Called from `update_ast_inner`
    /// and `parse_and_cache_content_versioned` after classes are parsed.
    pub(crate) fn populate_method_store(&self, classes: &[Arc<ClassInfo>]) {
        let mut store = self.method_store.write();
        for cls in classes {
            let fqn = cls.fqn().to_string();
            for method in &cls.methods {
                let key = (fqn.clone(), method.name.to_string());
                store.insert(key, Arc::clone(method));
            }
        }
    }

    /// Remove all method store entries whose class FQN matches any of
    /// the given FQNs.
    ///
    /// Called before re-populating after a file re-parse so that renamed
    /// or deleted methods do not linger.
    pub(crate) fn evict_methods_for_fqns(&self, fqns: &[String]) {
        if fqns.is_empty() {
            return;
        }
        let mut store = self.method_store.write();
        for fqn in fqns {
            store.retain(|k, _| k.0 != *fqn);
        }
    }

    /// Populate the GTI (go-to-implementation) reverse inheritance index
    /// for the given classes.  For each class, inserts the class's FQN
    /// into the child list of every parent (parent_class, interfaces,
    /// used_traits).
    pub(crate) fn populate_gti_index(&self, classes: &[Arc<ClassInfo>]) {
        let mut gti = self.gti_index.write();
        for cls in classes {
            if cls.name.starts_with("__anonymous@") {
                continue;
            }
            let child_fqn = cls.fqn().to_string();

            if let Some(ref parent) = cls.parent_class {
                let parent_str = parent.to_string();
                let children = gti.entry(parent_str).or_default();
                if !children.contains(&child_fqn) {
                    children.push(child_fqn.clone());
                }
            }
            for iface in &cls.interfaces {
                let iface_str = iface.to_string();
                let children = gti.entry(iface_str).or_default();
                if !children.contains(&child_fqn) {
                    children.push(child_fqn.clone());
                }
            }
            for tr in &cls.used_traits {
                let tr_str = tr.to_string();
                let children = gti.entry(tr_str).or_default();
                if !children.contains(&child_fqn) {
                    children.push(child_fqn.clone());
                }
            }
        }
    }

    /// Remove all GTI entries where `child_fqn` appears as a child.
    /// Called before re-populating when a file is re-parsed.
    pub(crate) fn evict_gti_for_fqns(&self, fqns: &[String]) {
        if fqns.is_empty() {
            return;
        }
        let fqn_set: HashSet<&str> = fqns.iter().map(|s| s.as_str()).collect();
        let mut gti = self.gti_index.write();
        for children in gti.values_mut() {
            children.retain(|child| !fqn_set.contains(child.as_str()));
        }
        // Remove empty entries to avoid unbounded growth.
        gti.retain(|_, v| !v.is_empty());
    }

    /// Re-scan a batch of files from disk, refreshing their discovery-level
    /// index entries (FQN→URI, autoload functions/constants, globals).
    ///
    /// Used when files are created, changed, or deleted outside the editor
    /// (a git checkout, a `composer install`, an editor session resuming
    /// after idle).  Every index that references a changed file is purged
    /// first, or stale symbols linger: completion keeps suggesting a class
    /// whose file was removed, go-to-definition jumps into a deleted file,
    /// and so on.  Purging only some indexes (e.g. `fqn_uri_index` but not
    /// `method_store`) leaves the symptom alive in whichever feature reads
    /// the index that was missed.
    ///
    /// The purge of each discovery index is done once for the whole batch
    /// rather than once per file.  Each of those maps must be scanned in
    /// full to drop a file's entries, so handling a flood of watched-file
    /// events one at a time would be O(files × index size); a branch switch
    /// can emit thousands of events at once.  Batching makes the purge
    /// O(index size) regardless of how many files changed.
    ///
    /// A given FQN→URI entry may have been stored under either of two URI
    /// conventions depending on how it was created (the classmap scan
    /// stores [`crate::util::path_to_uri`] of the discovered path, while
    /// `update_ast` stores the editor's URI string), so values are matched
    /// against both spellings.  The full
    /// [`ClassInfo`](crate::types::ClassInfo) is re-parsed lazily on next
    /// access; this only restores the lightweight discovery indexes.
    ///
    /// `changes` is `(editor URI string, file path, change type)`.
    pub(crate) fn reindex_files_batch(&self, changes: &[(String, PathBuf, FileChangeType)]) {
        if changes.is_empty() {
            return;
        }

        // Index values are stored under either the editor URI or the
        // canonical `file://` URI, so match both variants.
        let mut uri_set: HashSet<String> = HashSet::new();
        let mut path_set: HashSet<PathBuf> = HashSet::new();
        for (uri_str, path, _) in changes {
            uri_set.insert(uri_str.clone());
            uri_set.insert(crate::util::path_to_uri(path));
            path_set.insert(path.clone());
        }

        // FQN → URI: drop every entry sourced from a changed file in one
        // pass, collecting the dropped FQNs so the dependent caches can be
        // evicted without re-scanning.
        let mut dropped_fqns: Vec<String> = Vec::new();
        {
            let mut idx = self.fqn_uri_index.write();
            idx.retain(|fqn, v| {
                if uri_set.contains(v.as_str()) {
                    dropped_fqns.push(fqn.to_owned());
                    false
                } else {
                    true
                }
            });
        }
        {
            let mut fci = self.fqn_class_index.write();
            for fqn in &dropped_fqns {
                fci.remove(fqn);
            }
        }
        self.evict_methods_for_fqns(&dropped_fqns);
        self.evict_gti_for_fqns(&dropped_fqns);

        self.autoload_function_index
            .write()
            .retain(|_, v| !path_set.contains(v));
        self.autoload_constant_index
            .write()
            .retain(|_, v| !path_set.contains(v));
        self.global_functions
            .write()
            .retain(|_, (u, _)| !uri_set.contains(u.as_str()));
        self.global_defines
            .write()
            .retain(|_, d| !uri_set.contains(d.file_uri.as_str()));

        // Per-URI keyed removals are cheap (no full scan).
        for (uri_str, _, _) in changes {
            self.clear_file_maps(uri_str);
            self.uri_classes_index.write().remove(uri_str);
            self.parsed_uris.write().remove(uri_str);
            // The global_functions/global_defines entries for these URIs were
            // just retained out above; drop the per-URI tracking record too so
            // deleted files don't leave a stale entry behind.  Created/changed
            // files rebuild it when re-parsed below.
            self.uri_globals_index.write().remove(uri_str);
        }

        // Re-add current symbols for created/changed files.  Deleted files
        // keep their entries purged.
        for (uri_str, path, change_type) in changes {
            if !matches!(
                *change_type,
                FileChangeType::CREATED | FileChangeType::CHANGED
            ) {
                continue;
            }

            let classes = crate::classmap_scanner::scan_file(path);
            {
                let mut idx = self.fqn_uri_index.write();
                for fqn in classes {
                    idx.insert(fqn, uri_str.clone());
                }
            }

            let scan = crate::classmap_scanner::scan_file_full(path);
            {
                let mut fi = self.autoload_function_index.write();
                for fqn in scan.functions {
                    fi.or_insert_with(fqn, || path.clone());
                }
            }
            {
                let mut ci = self.autoload_constant_index.write();
                for name in scan.constants {
                    ci.entry(name).or_insert_with(|| path.clone());
                }
            }
        }
    }

    /// Create a shallow clone of this `Backend` that shares every
    /// `Arc`-wrapped field with the original.
    ///
    /// Non-`Arc` fields (`php_version`, `vendor_uri_prefixes`,
    /// `vendor_dir_paths`) are snapshotted at call time.  The stub
    /// indices (`stub_index`, `stub_function_index`,
    /// `stub_constant_index`) are cloned (they are static `&str`
    /// maps, so this is cheap).
    ///
    /// Used by `initialized()` to build a `Backend` value that can be
    /// moved into the `tokio::spawn`-ed diagnostic worker task while
    /// still observing every mutation the "real" `Backend` makes to
    /// the shared `Arc<Mutex<…>>` maps.
    ///
    /// Also used by [`clone_for_blocking`](Self::clone_for_blocking).
    pub(crate) fn clone_for_diagnostic_worker(&self) -> Self {
        Self {
            name: self.name.clone(),
            version: self.version.clone(),
            client_name: Mutex::new(self.client_name.lock().clone()),
            open_files: Arc::clone(&self.open_files),
            uri_classes_index: Arc::clone(&self.uri_classes_index),
            symbol_maps: Arc::clone(&self.symbol_maps),
            reference_index: Arc::clone(&self.reference_index),
            parse_errors: Arc::clone(&self.parse_errors),
            did_change_parse_locks: Arc::clone(&self.did_change_parse_locks),
            whole_file_coalesce: Arc::clone(&self.whole_file_coalesce),
            // RwLock fields are shared by Arc::clone — the diagnostic
            // worker reads them concurrently with the main Backend.
            client: self.client.clone(),
            workspace_root: Arc::clone(&self.workspace_root),
            psr4_mappings: Arc::clone(&self.psr4_mappings),
            file_imports: Arc::clone(&self.file_imports),
            resolved_names: Arc::clone(&self.resolved_names),
            file_namespaces: Arc::clone(&self.file_namespaces),
            global_functions: Arc::clone(&self.global_functions),
            global_defines: Arc::clone(&self.global_defines),
            uri_globals_index: Arc::clone(&self.uri_globals_index),
            autoload_function_index: Arc::clone(&self.autoload_function_index),
            autoload_function_origin_index: Arc::clone(&self.autoload_function_origin_index),
            autoload_constant_index: Arc::clone(&self.autoload_constant_index),
            autoload_constant_origin_index: Arc::clone(&self.autoload_constant_origin_index),
            autoload_file_paths: Arc::clone(&self.autoload_file_paths),
            fqn_uri_index: Arc::clone(&self.fqn_uri_index),
            fqn_origin_index: Arc::clone(&self.fqn_origin_index),
            fqn_class_index: Arc::clone(&self.fqn_class_index),
            phar_archives: Arc::clone(&self.phar_archives),
            parsed_uris: Arc::clone(&self.parsed_uris),
            parse_inflight: Arc::clone(&self.parse_inflight),
            class_not_found_cache: Arc::clone(&self.class_not_found_cache),
            stub_index: RwLock::new(self.stub_index.read().clone()),
            resolved_class_cache: Arc::clone(&self.resolved_class_cache),
            auth_user_type_cache: Arc::clone(&self.auth_user_type_cache),
            laravel_aliases: Arc::clone(&self.laravel_aliases),
            laravel_macros: Arc::clone(&self.laravel_macros),
            laravel_has_macros: Arc::clone(&self.laravel_has_macros),
            laravel_macro_seeds: Arc::clone(&self.laravel_macro_seeds),
            laravel_date_class: Arc::clone(&self.laravel_date_class),
            laravel_date_seed_uris: Arc::clone(&self.laravel_date_seed_uris),
            laravel_provider_resources: Arc::clone(&self.laravel_provider_resources),
            laravel_string_key_cache: Arc::clone(&self.laravel_string_key_cache),
            member_completion_cache: Arc::clone(&self.member_completion_cache),
            method_store: Arc::clone(&self.method_store),
            gti_index: Arc::clone(&self.gti_index),
            stub_function_index: RwLock::new(self.stub_function_index.read().clone()),
            stub_constant_index: RwLock::new(self.stub_constant_index.read().clone()),
            php_version: Mutex::new(self.php_version()),
            vendor_uri_prefixes: Mutex::new(self.vendor_uri_prefixes.lock().clone()),
            vendor_dir_paths: Mutex::new(self.vendor_dir_paths.lock().clone()),
            vendor_package_origin_roots: Arc::clone(&self.vendor_package_origin_roots),
            diag_version: Arc::clone(&self.diag_version),
            diag_notify: Arc::clone(&self.diag_notify),
            diag_pending_uris: Arc::clone(&self.diag_pending_uris),
            diag_last_slow: Arc::clone(&self.diag_last_slow),
            diag_last_fast: Arc::clone(&self.diag_last_fast),
            phpstan_notify: Arc::clone(&self.phpstan_notify),
            phpstan_pending_uri: Arc::clone(&self.phpstan_pending_uri),
            phpstan_last_diags: Arc::clone(&self.phpstan_last_diags),
            phpcs_notify: Arc::clone(&self.phpcs_notify),
            phpcs_pending_uri: Arc::clone(&self.phpcs_pending_uri),
            phpcs_last_diags: Arc::clone(&self.phpcs_last_diags),
            mago_lint_notify: Arc::clone(&self.mago_lint_notify),
            mago_lint_pending_uri: Arc::clone(&self.mago_lint_pending_uri),
            mago_lint_last_diags: Arc::clone(&self.mago_lint_last_diags),
            mago_analyze_notify: Arc::clone(&self.mago_analyze_notify),
            mago_analyze_pending_uri: Arc::clone(&self.mago_analyze_pending_uri),
            mago_analyze_last_diags: Arc::clone(&self.mago_analyze_last_diags),
            diag_result_ids: Arc::clone(&self.diag_result_ids),
            diag_last_full: Arc::clone(&self.diag_last_full),

            diag_suppressed: Arc::clone(&self.diag_suppressed),
            supports_pull_diagnostics: Arc::clone(&self.supports_pull_diagnostics),
            supports_file_rename: Arc::clone(&self.supports_file_rename),
            supports_work_done_progress: Arc::clone(&self.supports_work_done_progress),
            supports_type_hierarchy_dynamic_registration: Arc::clone(
                &self.supports_type_hierarchy_dynamic_registration,
            ),
            supports_semantic_tokens_refresh: Arc::clone(&self.supports_semantic_tokens_refresh),
            init_complete: Arc::clone(&self.init_complete),
            shutdown_flag: Arc::clone(&self.shutdown_flag),
            config: Mutex::new(self.config.lock().clone()),
            blade_virtual_content: Arc::clone(&self.blade_virtual_content),
            blade_source_maps: Arc::clone(&self.blade_source_maps),
            blade_uris: Arc::clone(&self.blade_uris),
            workspace_indexed: Arc::clone(&self.workspace_indexed),
            workspace_index_lock: Arc::clone(&self.workspace_index_lock),
            full_index_in_progress: Arc::clone(&self.full_index_in_progress),
            sync_ast_updates: self.sync_ast_updates,
        }
    }

    /// Cheap clone that shares all `Arc`-wrapped state with the original.
    ///
    /// Used by LSP handlers (hover, definition, references, etc.) to move
    /// blocking sync work onto a `spawn_blocking` thread while keeping
    /// the async runtime free to process cancellations and other requests.
    pub(crate) fn clone_for_blocking(&self) -> Self {
        self.clone_for_diagnostic_worker()
    }

    /// Return the current project configuration.
    ///
    /// Returns a clone of the [`Config`](config::Config) loaded from
    /// `.phpantom.toml` (or the default config when the file is missing).
    pub fn config(&self) -> config::Config {
        self.config.lock().clone()
    }

    /// Replace the current configuration.
    ///
    /// Used by integration tests to enable opt-in diagnostics like
    /// `unresolved-member-access` without needing a `.phpantom.toml` file.
    pub fn set_config(&self, config: config::Config) {
        *self.config.lock() = config;
    }

    /// Set the PHP version (used by integration tests and during
    /// server initialization after reading `composer.json`).
    ///
    /// Also filters `stub_function_index`, `stub_index`, and
    /// `stub_constant_index` to remove entries that do not exist in
    /// the given PHP version.
    pub fn set_php_version(&self, version: types::PhpVersion) {
        *self.php_version.lock() = version;
        self.stub_function_index
            .write()
            .retain(|name, source| !stubs::is_stub_function_removed(source, name, version));
        self.stub_index
            .write()
            .retain(|name, source| !stubs::is_stub_class_removed(source, name, version));
        self.stub_constant_index
            .write()
            .retain(|name, source| !stubs::is_stub_constant_removed(source, name, version));
    }

    /// Check whether a URI refers to a Blade template file.
    /// Returns true if the URI ends with `.blade.php` OR was opened with `languageId == "blade"`.
    pub(crate) fn is_blade_file(&self, uri: &str) -> bool {
        crate::blade::is_blade_file(uri) || self.blade_uris.read().contains(uri)
    }

    /// Translate a position from an original Blade file to the virtual PHP file.
    pub(crate) fn translate_blade_to_php(
        &self,
        uri: &str,
        pos: tower_lsp::lsp_types::Position,
    ) -> tower_lsp::lsp_types::Position {
        if let Some(map) = self.blade_source_maps.read().get(uri) {
            map.blade_to_php(pos)
        } else {
            pos
        }
    }

    /// Translate a position from a virtual PHP file back to the original Blade file.
    pub(crate) fn translate_php_to_blade(
        &self,
        uri: &str,
        pos: tower_lsp::lsp_types::Position,
    ) -> tower_lsp::lsp_types::Position {
        if let Some(map) = self.blade_source_maps.read().get(uri) {
            map.php_to_blade(pos)
        } else {
            pos
        }
    }

    /// Translate a location from virtual PHP coordinates back to original Blade
    /// coordinates if the location points into a Blade file.
    pub(crate) fn translate_location(
        &self,
        mut location: tower_lsp::lsp_types::Location,
    ) -> tower_lsp::lsp_types::Location {
        let uri_str = location.uri.to_string();
        if self.is_blade_file(&uri_str) {
            location.range.start = self.translate_php_to_blade(&uri_str, location.range.start);
            location.range.end = self.translate_php_to_blade(&uri_str, location.range.end);
        }
        location
    }
}
