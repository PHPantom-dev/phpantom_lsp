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
//! - `type_engine` — The shared type-resolution engine: subject extraction,
//!   subject-to-`ClassInfo` resolution, call/return-type resolution, and
//!   variable type inference, consumed by completion, diagnostics, hover,
//!   definition, and signature help
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
//!   - `code_actions::replace_fqcn` — Import qualified classes, functions,
//!     and constants and replace their file-local usages with short names
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
//!   - `docblock::type_strings` — type utilities (`split_type_token`)
//!   - `docblock::shapes` — PHPStan array shape parsing
//!     (`parse_array_shape`, `extract_array_shape_value_type`) and object shape
//!     parsing (`parse_object_shape`, `extract_object_shape_property_type`,
//!     `is_object_shape`)

// Use mimalloc on Linux, where the system allocator is 4-6x slower for
// our parallel, allocation-heavy workload. Defined in the library
// rather than the binary so every artifact built from this crate uses
// the same allocator: the language server, and also the benchmark and
// test harnesses, which link the library but not `main.rs`. The Linux
// binaries we ship are musl; covering glibc too keeps a local dev build
// representative of them (see the dependency note in Cargo.toml).
#[cfg(all(feature = "mimalloc", target_os = "linux", not(feature = "mem-audit")))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// The `mem-audit` feature replaces the global allocator with a counting
// wrapper that delegates to whichever allocator this build would
// otherwise use, so the audit reports live bytes for the real
// allocator. Never enabled in a shipped build (see Cargo.toml).
#[cfg(feature = "mem-audit")]
#[global_allocator]
static GLOBAL: mem_audit::CountingAlloc = mem_audit::CountingAlloc;

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tower_lsp::Client;
use tower_lsp::lsp_types::CompletionItem;

use backend::external_tool_worker::ExternalToolWorker;
use backend::laravel::{LaravelStringKeyBuildLocks, LaravelStringKeyCache};
use backend::requests::WholeFileCoalesce;
use ci_map::CiMap;
use symbol_index::SymbolIndex;
use workspace_env::WorkspaceEnv;

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

/// What the last `workspace/didChangeWatchedFiles` registration pushed to
/// the client was built from, so the next one can tell whether anything
/// it watches has moved. See [`Backend::registered_watcher_state`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WatchedFileInputs {
    /// The `[indexing] extensions` set, one watcher each.
    pub(crate) extra_extensions: Vec<String>,
    /// Whether the project was classified as Laravel, which adds the
    /// schema watchers.
    pub(crate) is_laravel: bool,
    /// Directory symlinks the index reached through, one relative-pattern
    /// watcher each. Empty when the client cannot match a relative
    /// pattern, since then there is nothing to ask it for.
    pub(crate) followed_links: Vec<std::path::PathBuf>,
}

/// `None` until the first registration.
pub(crate) type WatchedFileRegistrationState = Option<WatchedFileInputs>;

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

/// Tune the global allocator for a language-server workload.
///
/// Only does anything on Linux, where mimalloc is the global allocator
/// (the system malloc is 4-6x slower for our parallel,
/// allocation-heavy indexing). Two adjustments keep mimalloc's
/// resident memory close to the live working set:
///
/// * **Purge delay.** The Rust mimalloc build retains freed pages
///   effectively indefinitely by default, so the tens of megabytes of
///   transient parse data freed during an indexing burst stay resident
///   while the server then sits idle. A small purge delay makes each
///   thread return pages to the OS shortly after they fall idle, at no
///   measurable throughput cost (pages are decommitted once, after the
///   burst, not churned on the hot path).
///
/// * **Transparent huge pages off for this process.** On distros where
///   THP is `always` (RHEL and derivatives most notably), the kernel
///   backs the allocator's arenas with 2 MiB pages; partially-freed
///   huge pages cannot be returned piecemeal and khugepaged re-collapses
///   ranges behind the purger's back, so resident memory stays tens of
///   MiB above the working set no matter how eagerly mimalloc purges.
///   mimalloc would issue this same prctl itself if its `allow_thp`
///   option were 0, but it reads that option during pre-`main` heap
///   initialization, before we run, so we make the call ourselves.
///
/// Must be called at the very start of `main`, before the async runtime
/// spawns any threads. No-op when mimalloc is not the active allocator.
pub fn configure_allocator() {
    #[cfg(all(feature = "mimalloc", target_os = "linux"))]
    unsafe {
        // `mi_option_purge_delay` is index 15 in mimalloc v3's option
        // enum (see c_src/mimalloc/v3/include/mimalloc.h in libmimalloc-sys),
        // which is the major version the crate compiles by default. The
        // enum layout differs between major versions, so guard on the
        // runtime version (v3 reports as 3xxxx): if a dependency bump
        // switches the bundled major version, we skip tuning and fall
        // back to the default rather than writing an unrelated option.
        const MI_OPTION_PURGE_DELAY_V3: std::os::raw::c_int = 15;
        let version = libmimalloc_sys::mi_version();
        if (30000..40000).contains(&version) {
            libmimalloc_sys::mi_option_set(MI_OPTION_PURGE_DELAY_V3, 10);
        }

        // Process-scoped THP opt-out (does not touch system settings).
        // Ignore the result: on kernels without THP support the prctl
        // fails and there is nothing to opt out of.
        let _ = libc::prctl(libc::PR_SET_THP_DISABLE, 1, 0, 0, 0);
    }
}

pub mod analyse;
pub mod atom;
mod backend;
pub mod benevolent_builtins;
pub mod blade;
pub(crate) mod call_args;
mod call_hierarchy;
pub mod ci_map;
pub(crate) mod class_loader_memo;
pub(crate) mod class_lookup;
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
pub mod format_cli;
pub mod formatting;
mod highlight;
mod hover;
mod indexing;
pub(crate) mod inheritance;
pub mod init_wizard;
mod inlay_hints;
/// LSP JSON-RPC dispatch for the wasm build, which has no tower-lsp transport.
/// Kept free of any target-specific code so the marshalling in `wasm_wasi` is
/// the only thing a future non-WASI wasm target would have to replace.
#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
mod lsp_dispatch;
mod mago;
#[cfg(feature = "mem-audit")]
mod mem_audit;
pub mod move_cli;
pub(crate) mod names;
mod parallel;
mod parser;
pub(crate) mod pest;
pub(crate) mod phar;
pub mod php_type;
mod phpcs;
mod phpmd;
mod phpstan;
pub(crate) mod phpstan_ignore;
pub(crate) mod process;
pub mod progress;
mod reference_counts;
mod reference_index;
mod references;
mod rename;
mod resolution;
pub(crate) mod resolution_deps;
mod resource_navigation;
pub(crate) mod return_collection;
pub(crate) mod scope_collector;
mod selection_range;
#[cfg(not(target_arch = "wasm32"))]
pub mod self_update;
#[cfg(feature = "semantic-export")]
pub mod semantic_export;
mod semantic_tokens;
mod server;
mod signature_help;
mod signature_render;
pub mod stub_patches;
pub mod stubs;
mod symbol_index;
pub(crate) mod symbol_map;
pub(crate) mod text_position;
pub(crate) mod text_scan;
pub(crate) mod toposort;
pub mod type_engine;
mod type_hierarchy;
pub mod types;
mod util;
pub(crate) mod virtual_members;
/// The exported wasm entry points. WASI only: the bare `wasm32-unknown-unknown`
/// target miscompiles the completion path (see `docs/wasm.md`).
#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
mod wasm_wasi;
mod workspace_env;
mod workspace_symbols;

#[cfg(test)]
pub mod test_fixtures;

// ─── Re-exports ─────────────────────────────────────────────────────────────

// Re-export public types so that dependents (tests, main) can import them
// from the crate root, e.g. `use phpantom_lsp::{Backend, AccessKind}`.
pub use backend::package_origin::ClassCompletionOrigin;
pub use completion::target::extract_completion_target;
pub use types::{AccessKind, ClassInfo, DefineInfo, FunctionInfo, NamespaceSpan, Visibility};
pub use virtual_members::resolve_class_fully;

// ─── Backend ────────────────────────────────────────────────────────────────

/// The main LSP backend that holds all server state.
///
/// Its behaviour is implemented in `impl Backend` blocks across the
/// feature modules; see the Module Layout section of
/// `docs/ARCHITECTURE.md` for where each lives.
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
    /// Symbol discovery and lookup indexes (classes, functions, constants).
    pub(crate) symbols: SymbolIndex,
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
    /// Skip building [`reference_index`] from `update_ast`.
    ///
    /// Set by [`Backend::new_headless`] for the `analyze`/`fix` CLI
    /// subcommands, which parse every file but never issue a
    /// find-references, rename, or CodeLens request, so populating
    /// the index would be pure wasted CPU and short-lived allocation.
    pub(crate) skip_reference_index: bool,
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
    /// Workspace-level configuration and PSR-4/vendor location metadata.
    pub(crate) workspace: WorkspaceEnv,
    /// Maps a file URI to its `use` statement mappings (short name → fully qualified name).
    /// For example, `use Klarna\Rest\Resource;` produces `"Resource" → "Klarna\Rest\Resource"`.
    pub(crate) file_imports: Arc<RwLock<HashMap<String, HashMap<String, String>>>>,
    /// Maps a file URI to its `use function` and `use const` imports, kept
    /// apart from the class imports in `file_imports`.  Files without any
    /// have no entry.
    pub(crate) file_symbol_imports: Arc<RwLock<HashMap<String, types::SymbolImports>>>,
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
    /// Parsed phar archives keyed by the phar file's absolute path.
    ///
    /// Populated during Composer autoload scanning when a bootstrap file
    /// references a `.phar` archive (e.g. PHPStan's `bootstrap.php`).
    /// Used by [`parse_and_cache_uri`](Self::parse_and_cache_uri) to
    /// extract PHP source files from inside the archive when the
    /// fqn_uri_index holds a `phar://` URI (e.g.
    /// `phar:///path/to/phpstan.phar/src/Type/Type.php`).
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
    pub(crate) stub_index: Arc<RwLock<CiMap<&'static str>>>,
    /// Cache of fully-resolved classes (inheritance + virtual members).
    ///
    /// Keyed by fully-qualified class name.  Populated lazily by
    /// [`resolve_class_fully_cached`](crate::virtual_members::resolve_class_fully_cached)
    /// and cleared whenever a file is re-parsed (`update_ast` /
    /// `parse_and_cache_content`) so that stale results never survive
    /// an edit.
    ///
    /// Uses `parking_lot::Mutex` because it is frequently written (cache
    /// stores) and RwLock read→write upgrades are error-prone.
    pub(crate) resolved_class_cache: virtual_members::ResolvedClassCache,
    /// Memoized authenticated-user model type, derived from `config/auth.php`.
    ///
    /// Keyed by guard name (an empty string denotes the default guard).
    /// Populated lazily when an auth-user access is resolved and cleared
    /// whenever files are re-parsed, so edits to `config/auth.php` take
    /// effect without a restart.
    pub(crate) auth_user_type_cache: Arc<RwLock<HashMap<String, Option<crate::php_type::PhpType>>>>,
    /// Memoized concrete type every configured filesystem disk resolves to,
    /// derived from `config/filesystems.php` and
    /// [`laravel_storage_drivers`](Self::laravel_storage_drivers).
    ///
    /// `Storage`/`FilesystemManager`'s `drive()`/`disk()`/`cloud()`/`build()`
    /// declare only the `Filesystem`/`Cloud` contract; this is what they are
    /// refined to. The outer option is `None` until first computed, the inner
    /// one when at least one disk could not be classified (a dynamic driver
    /// name, or a custom driver with no discoverable registration). Cleared
    /// when a `config/` file or a `Storage::extend()` registration changes, so
    /// an edit takes effect without a restart.
    pub(crate) storage_disk_type_cache: Arc<RwLock<Option<Option<crate::php_type::PhpType>>>>,
    /// `Storage::extend('driver', closure)` registrations discovered from
    /// project and vendor service-provider source, keyed by driver name.
    ///
    /// Built alongside [`laravel_macros`](Self::laravel_macros) (both come
    /// from the same provider scan) and refreshed when a contributing file
    /// changes. Empty for non-Laravel projects.
    pub(crate) laravel_storage_drivers:
        Arc<RwLock<virtual_members::laravel::LaravelStorageDriverIndex>>,
    /// Memoized Laravel alias tables, parsed from the installed framework
    /// source (`registerCoreContainerAliases()`, `Facade::defaultAliases()`)
    /// and the project's `config/app.php`.
    ///
    /// `None` means "not yet computed"; an inner empty map means "computed and
    /// this project has no such aliases" (e.g. a non-Laravel project). Cleared
    /// whenever files are re-parsed so edits to `config/app.php` take effect
    /// without a restart.
    ///
    /// The same slot is shared into [`resolved_class_cache`](Self::resolved_class_cache)
    /// so the facade virtual member provider, which sees the cache but never
    /// the `Backend`, can map a container-binding accessor to its class.
    pub(crate) laravel_aliases: virtual_members::laravel::LaravelAliasSlot,
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
    /// Reverse index mapping a related-model FQN to the pivot accessors exposed
    /// when it is reached through a many-to-many relationship.
    /// Built lazily (and rebuilt when a pivot-bearing file changes) and
    /// consulted at class load; see [`virtual_members::laravel::pivots`].
    pub(crate) laravel_pivots: Arc<RwLock<virtual_members::laravel::LaravelPivotIndex>>,
    /// Fast gate for [`laravel_pivots`](Self::laravel_pivots): `true` only when
    /// the index holds at least one many-to-many target.
    pub(crate) laravel_has_pivots: Arc<std::sync::atomic::AtomicBool>,
    /// Whether [`laravel_pivots`](Self::laravel_pivots) needs rebuilding
    /// (a pivot-bearing file changed, or the index was never built). Starts
    /// `true` so the first class load builds it.
    pub(crate) laravel_pivots_dirty: Arc<std::sync::atomic::AtomicBool>,
    /// Index of Artisan console commands (project + vendor) keyed by command
    /// name.  Built during `initialized` for Laravel projects and refreshed
    /// when a command file changes.  Powers command-name completion,
    /// go-to-definition, hover, and unknown-name diagnostics for
    /// `Artisan::call('app:sync')` and friends.  Empty for non-Laravel
    /// projects.  See [`virtual_members::laravel::commands`].
    pub(crate) laravel_commands: Arc<RwLock<virtual_members::laravel::LaravelCommandIndex>>,
    /// Fast gate for [`laravel_commands`](Self::laravel_commands): `true` only
    /// when the index holds at least one command.
    pub(crate) laravel_has_commands: Arc<std::sync::atomic::AtomicBool>,
    /// Eloquent morph map (`Relation::morphMap()` /
    /// `Relation::enforceMorphMap()`) recovered from service-provider source,
    /// keyed by morph alias.  Built during `initialized` for Laravel projects
    /// and refreshed when a registering file changes.  Powers
    /// go-to-definition, hover, and find-references on the alias strings that
    /// appear in `*_type` columns and morph query arguments.  Empty for
    /// non-Laravel projects.  See [`virtual_members::laravel::morph_map`].
    pub(crate) laravel_morph_map: Arc<RwLock<virtual_members::laravel::LaravelMorphMapIndex>>,
    /// Authorization gate registrations (`Gate::define()` abilities and the
    /// model → policy map) recovered from service-provider source.  Built
    /// during `initialized` for Laravel projects and refreshed when a
    /// registering file changes.  Powers completion, hover, go-to-definition,
    /// and the unknown-ability diagnostic for the strings `Gate::allows()`,
    /// `$user->can()`, `$this->authorize()`, and `@can` check.  Empty for
    /// non-Laravel projects.  See [`virtual_members::laravel::gates`].
    pub(crate) laravel_gates: Arc<RwLock<virtual_members::laravel::LaravelGateIndex>>,
    /// Config keys the project declares at runtime rather than in a
    /// `config/` file (`Config::set()`, the array form of the `config()`
    /// helper, `Storage::fake()`), keyed by the file each write is written
    /// in so an edit that removes one takes the key with it.  A test that
    /// configures a disk in `setUp()` before exercising it is the common
    /// shape.  Empty for non-Laravel projects.
    pub(crate) laravel_runtime_config_keys: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Whether the workspace is an application rather than a library.
    ///
    /// A library's configuration is supplied by whatever application
    /// installs it, so the config keys it reads are declared in a file we
    /// never see and none of them can be judged.  See
    /// [`crate::composer::is_application_project`].  Starts `true` so a
    /// workspace with no `composer.json` to classify behaves as before.
    pub(crate) is_application: Arc<std::sync::atomic::AtomicBool>,
    /// Laravel macro seed files (service providers plus the app's provider
    /// registration files), mapped to the class references each contributed
    /// at the last macro-index build.  An edit that changes a seed's
    /// references triggers a full index rebuild; every other edit takes the
    /// cheap single-file refresh path.
    pub(crate) laravel_macro_seeds: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// URIs of the mixin-class files that `Macroable::mixin()` registrations
    /// pulled macros from at the last macro-index build.  A mixin class's
    /// methods live in a different file than the `::mixin(...)` call, and that
    /// file contains no `macro(`/`mixin(` token of its own, so the single-file
    /// refresh cannot see it as a contributor.  An edit to one of these files
    /// (e.g. adding a helper method) therefore triggers a full index rebuild.
    pub(crate) laravel_macro_mixin_uris: Arc<RwLock<std::collections::HashSet<String>>>,
    /// Concrete date class selected by the project's `Date::use()` call.
    ///
    /// The outer option is `None` until startup discovery completes; the inner
    /// option is `None` when discovery found no project override.
    pub(crate) laravel_date_class: Arc<RwLock<Option<Option<String>>>>,
    /// The `Pest.php` files of the project's test directories, read on
    /// first use and dropped when one is saved.  `None` until then.
    pub(crate) pest_configs: Arc<RwLock<Option<pest::PestConfigs>>>,
    /// The classes and traits each test file binds to itself with a
    /// `uses(…)` or `pest()->extends(…)` that has no `->in(…)`.  Only
    /// files that do are present.
    pub(crate) file_pest_uses: Arc<RwLock<HashMap<String, Arc<[String]>>>>,
    /// URIs whose edits can change the configured date class: every registered
    /// service provider scanned by [`build_laravel_date_class`], plus the app's
    /// provider-registration files (which decide *which* providers are
    /// registered).  Populated by that scan and consulted by the single-file
    /// refresh so a `Date::use()` added, changed, or removed in one of these
    /// files re-runs the full scan, while an edit to any other file is ignored.
    ///
    /// [`build_laravel_date_class`]: crate::Backend::build_laravel_date_class
    pub(crate) laravel_date_seed_uris: Arc<RwLock<std::collections::HashSet<String>>>,
    /// What the project's registered service providers register: container
    /// bindings, package config files, view and translation directories, route
    /// files, and Blade component namespaces, merged across every provider.
    ///
    /// Built by [`build_provider_resources`](crate::Backend::build_provider_resources)
    /// and republished by
    /// [`refresh_laravel_provider_resources`](crate::Backend::refresh_laravel_provider_resources)
    /// when a provider is edited.
    pub(crate) laravel_provider_resources: Arc<RwLock<virtual_members::laravel::ProviderResources>>,
    /// The per-provider scans the merged table above was built from, so an
    /// edit to one provider rebuilds the merge without re-reading the rest.
    pub(crate) laravel_provider_scans: Arc<RwLock<virtual_members::laravel::ProviderScans>>,
    /// The Blade directives the project's templates can write: the built-in
    /// ones the installed Blade compiler defines, and the ones its providers
    /// register, expanded from the merged table's `custom_directives` into
    /// every name a template can write.  Kept separate from the table
    /// because the Blade preprocessor reads it on every keystroke in a
    /// template and must not pay for the expansion each time.
    pub(crate) blade_directives: Arc<RwLock<blade::directives::BladeDirectives>>,
    /// Cached Laravel string key enumerations (route names, config keys,
    /// view names, translation keys).  `None` = not yet computed.
    /// Invalidated when a file in `routes/`, `config/`, `resources/views/`,
    /// or `lang/` is updated.
    pub(crate) laravel_string_key_cache: Arc<RwLock<LaravelStringKeyCache>>,
    /// Compute-once guards for `laravel_string_key_cache`; see
    /// [`LaravelStringKeyBuildLocks`].
    pub(crate) laravel_string_key_build_locks: Arc<LaravelStringKeyBuildLocks>,
    pub(crate) schema_index: Arc<RwLock<virtual_members::laravel::database_schema::SchemaIndex>>,
    /// Per-target member completion cache.
    ///
    /// Typing `$model->wh...` triggers a completion request for each
    /// keyword edit. The receiver and candidate member set are unchanged
    /// across those requests, so cache the unfiltered member list and let
    /// each request apply only its current prefix filter.
    pub(crate) member_completion_cache: Arc<Mutex<HashMap<String, Arc<Vec<CompletionItem>>>>>,
    /// Embedded PHP stubs for built-in functions (e.g. `array_map`,
    /// `str_contains`, …).  Maps function name → raw PHP source code.
    ///
    /// Built once during construction via [`stubs::build_stub_function_index`].
    /// Filtered at startup via [`set_php_version`](Self::set_php_version) to
    /// remove stubs that do not exist in the target PHP version.
    /// Can be consulted to resolve return types of built-in function calls.
    pub(crate) stub_function_index: Arc<RwLock<CiMap<&'static str>>>,
    /// Embedded PHP stubs for built-in constants (e.g. `PHP_EOL`,
    /// `SORT_ASC`, …).  Maps constant name → raw PHP source code.
    ///
    /// Built once during construction via [`stubs::build_stub_constant_index`].
    /// Filtered at startup via [`set_php_version`](Self::set_php_version) to
    /// remove stubs that do not exist in the target PHP version.
    /// Can be consulted when resolving standalone constant references.
    pub(crate) stub_constant_index: Arc<RwLock<HashMap<&'static str, &'static str>>>,
    /// Diagnostic debouncing state and the pull-model diagnostic caches.
    pub(crate) diag: crate::diagnostics::state::DiagnosticState,
    /// PHPStan's dedicated background worker state (extremely slow and
    /// resource-intensive, so it runs separately from native diagnostics).
    pub(crate) phpstan_tool: ExternalToolWorker,
    /// PHPCS's dedicated background worker state.
    pub(crate) phpcs_tool: ExternalToolWorker,
    /// PHPMD's dedicated background worker state.
    pub(crate) phpmd_tool: ExternalToolWorker,
    /// Mago lint's dedicated background worker state.
    pub(crate) mago_lint_tool: ExternalToolWorker,
    /// Mago analyze's dedicated background worker state.
    pub(crate) mago_analyze_tool: ExternalToolWorker,
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
    /// Whether the client supports file creation in workspace edits
    /// (`workspace.workspaceEdit.resourceOperations` includes `create`).
    /// The code actions that create a file (extract interface, create a
    /// missing view) are only offered when it does.
    pub(crate) supports_file_create: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client can resolve deferred code actions: it lists `data`
    /// support and `edit` in `codeAction.resolveSupport.properties`.  When it
    /// cannot, `handle_code_action` sends every edit up front.
    pub(crate) supports_code_action_resolve: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client supports server-initiated work-done progress.
    ///
    /// Set during `initialize` based on the client's
    /// `window.workDoneProgress` capability.  When `false`, the server
    /// must not send `window/workDoneProgress/create` requests because
    /// the client will not handle them, blocking the server indefinitely.
    pub(crate) supports_work_done_progress: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client supports dynamic registration for type hierarchy.
    pub(crate) supports_type_hierarchy_dynamic_registration: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client accepts dynamic registration of
    /// `workspace/didChangeWatchedFiles`
    /// (`workspace.didChangeWatchedFiles.dynamicRegistration`).
    ///
    /// The protocol has no static way to ask for file events, so this is
    /// the only way to hear about a change made outside the editor. A
    /// client that did not opt in is never sent the registration: it may
    /// not understand the request, and may never answer it.
    pub(crate) supports_watched_files_dynamic_registration: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client can match a watcher pattern against a base URI
    /// (`workspace.didChangeWatchedFiles.relativePatternSupport`).
    ///
    /// A plain `**/*.php` pattern is matched against the files of the
    /// workspace folders, which gives a client no reason to watch a
    /// directory living outside them, and nothing obliges it to traverse a
    /// symlink to find one. A relative pattern names the link outright,
    /// which is the only way in the protocol to ask for those events;
    /// without the capability, a tree reached through a link is indexed but
    /// not watched.
    pub(crate) supports_relative_pattern_watchers: Arc<std::sync::atomic::AtomicBool>,
    /// The `[indexing] extensions` set and Laravel classification last
    /// pushed to the client as a `workspace/didChangeWatchedFiles`
    /// registration. `None` until `initialized` performs the first
    /// registration, and for good when the client did not opt in to one
    /// ([`Backend::supports_watched_files_dynamic_registration`]).
    ///
    /// Compared on every config reload
    /// ([`indexing::watch::reregister_watched_files_if_changed`]) so a
    /// live `.phpantom.toml` edit that adds or removes an extension can
    /// push a fresh registration instead of requiring a restart.
    pub(crate) registered_watcher_state: Arc<RwLock<WatchedFileRegistrationState>>,
    /// Whether the client supports `window/showDocument`.
    ///
    /// Set during `initialize` based on the client's
    /// `window.showDocument.support` capability.  Code lens navigation
    /// asks the client to open the prototype's file through that request,
    /// so a client that does not opt in gets a lens command it can act on
    /// by itself instead (see `code_lens::build_code_lens_command`).
    pub(crate) supports_show_document: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client supports `workspace/semanticTokens/refresh`.
    ///
    /// Set during `initialize` based on the client's
    /// `workspace.semanticTokens.refreshSupport` capability.  When `true`,
    /// the server asks the client to re-pull semantic tokens after a
    /// background `didChange` parse commits a new symbol map — without
    /// this, editors keep showing tokens computed from the pre-edit
    /// symbol map until the next unrelated request.
    pub(crate) supports_semantic_tokens_refresh: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client supports `workspace/codeLens/refresh`.
    ///
    /// Exact member-reference locations are computed outside the CodeLens
    /// request.  Supporting clients re-pull once that bounded cache is warm,
    /// avoiding a burst of lazy resolve requests for every declaration.
    pub(crate) supports_code_lens_refresh: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client supports `workspace/inlayHint/refresh`.
    ///
    /// Set during `initialize` from the client's
    /// `workspace.inlayHint.refreshSupport` capability.  Hints resolve
    /// against the workspace index and a background parse, so without a
    /// refresh the editor keeps the ones it pulled before either was
    /// ready.
    pub(crate) supports_inlay_hint_refresh: Arc<std::sync::atomic::AtomicBool>,
    /// Exact member references behind the declaration CodeLens.
    pub(crate) member_ref_counts: Arc<reference_counts::MemberRefCounts>,
    /// Set to `true` once `initialized` finishes indexing (PSR-4,
    /// classmap, stubs, vendor).  Background workers and the pull
    /// diagnostic handler check this flag before running diagnostics
    /// so that files opened during startup don't produce a flood of
    /// false-positive "class not found" / "function not found" errors.
    pub(crate) init_complete: Arc<std::sync::atomic::AtomicBool>,
    /// Shared flag set to `true` when the LSP `shutdown` request is
    /// received.  Background workers (diagnostic, PHPStan, PHPCS) check this
    /// flag on each iteration and exit their loops.  The PHPStan
    /// `run_command_with_timeout` poll loop also checks it so that a
    /// running child process is killed promptly instead of waiting up
    /// to 60 seconds.
    pub(crate) shutdown_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Virtual PHP content generated from Blade files.
    ///
    /// Shared rather than owned: every request against a template reads
    /// this text, and a template's virtual PHP is several times the size
    /// of the template itself.
    pub(crate) blade_virtual_content: Arc<RwLock<HashMap<String, Arc<String>>>>,
    /// Source maps from virtual PHP back to original Blade positions.
    pub(crate) blade_source_maps:
        Arc<RwLock<HashMap<String, crate::blade::source_map::BladeSourceMap>>>,
    /// Held while a parse publishes a template's virtual PHP, source map,
    /// and symbol map, which live behind three separate locks.
    ///
    /// Several threads can re-parse the same template at once (the
    /// did-open and did-save call-site inference, the refresh a Find
    /// References request runs, the workspace index), each with its own
    /// lowering.  Publishing the three one after another without this
    /// could leave the virtual PHP of one parse next to the symbol map of
    /// another, and every offset in the map would then point at the wrong
    /// text until the template is parsed again.
    pub(crate) blade_publish_lock: Arc<Mutex<()>>,
    /// URIs opened with `languageId == "blade"` that don't have a `.blade.php` extension.
    /// Allows editors to signal Blade files via languageId alone.
    pub(crate) blade_uris: Arc<RwLock<std::collections::HashSet<String>>>,
    /// Per-template scope seeded into the virtual PHP on the last
    /// preprocess: the declared and call-site-inferred variables, and the
    /// class `$this` is bound to.  Lets re-inference passes skip templates
    /// whose scope is unchanged.
    pub(crate) blade_injected_vars:
        Arc<RwLock<HashMap<String, crate::blade::call_site_inference::BladeScope>>>,
    /// Per-file view spans whose render site only the receiver's *type*
    /// settles, computed on first use and dropped when the file is
    /// re-parsed (see [`crate::blade::typed_receiver`]).  Only files whose
    /// symbol map recorded a candidate site ever get an entry.
    pub(crate) typed_receiver_view_spans_cache:
        Arc<RwLock<HashMap<String, crate::blade::typed_receiver::TypedReceiverSpans>>>,
    /// Lazily confirmed morph-column aliases, invalidated when source or
    /// the model metadata used to recognize their columns changes.
    pub(crate) morph_column_spans_cache: Arc<
        RwLock<HashMap<String, virtual_members::laravel::typed_morph_columns::MorphColumnSpans>>,
    >,
    /// Whether the workspace directory has been fully scanned for PHP and
    /// resource files.
    ///
    /// Set to `true` after the initial `ensure_workspace_indexed` pass.
    /// Per-symbol consumers reuse that index, watched-file notifications
    /// update it incrementally, and an explicit reference search may refresh
    /// it once to discover filesystem changes the editor did not report.
    pub(crate) workspace_indexed: Arc<std::sync::atomic::AtomicBool>,
    /// Serializes whole-workspace indexing so a foreground request does not
    /// duplicate the background full-index parse.
    pub(crate) workspace_index_lock: Arc<Mutex<()>>,
    /// Prevents duplicate background full-index tasks when initialization and
    /// a request both race to parse the whole workspace.
    pub(crate) full_index_in_progress: Arc<std::sync::atomic::AtomicBool>,
    /// Latest `(percentage, message)` published by the indexing pass that
    /// currently holds `workspace_index_lock`, or `None` when no pass is
    /// running.
    ///
    /// Requests that need a complete index (find references, rename,
    /// go-to-implementation, Laravel string keys) block on that lock while
    /// the background full index runs. They mirror this status into their
    /// own progress token so the wait reads as "the workspace is still
    /// indexing" rather than a stalled request.
    pub(crate) workspace_index_status: Arc<Mutex<Option<(u32, String)>>>,
    /// Progress sink for the currently executing long-running request
    /// (go-to-implementation, find-references, type hierarchy).
    ///
    /// Handlers set this on their per-request clone before moving the
    /// work to the blocking pool, so the scan code deep inside the
    /// request can report per-file progress without threading a
    /// callback through every signature.  The shared `Backend`
    /// instance (and every fresh clone) keeps `None`.
    pub(crate) request_progress: Option<Arc<progress::ScanProgress>>,
}
