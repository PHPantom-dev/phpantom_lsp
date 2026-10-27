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
use tower_lsp_server::Client;
use tower_lsp_server::gen_lsp_types::CompletionItem;

use backend::external_tool_worker::ExternalToolWorker;
use backend::requests::WholeFileCoalesce;
use ci_map::CiMap;
use symbol_index::SymbolIndex;
use workspace_env::WorkspaceEnv;

/// A single parse error entry: `(message, start_byte_offset, end_byte_offset)`.
///
/// Stored in each file's [`FileState`](file_state::FileState) during
/// `update_ast` and consumed by the syntax-error diagnostic collector.
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
    /// Whether the project was classified as Symfony, which adds the
    /// XLIFF catalogue watchers.
    pub(crate) is_symfony: bool,
    /// Whether the project uses Twig, which adds the template watchers.
    pub(crate) is_twig: bool,
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
mod file_state;
pub mod fix;
mod folding;
pub mod format_cli;
pub mod formatting;
mod framework;
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
pub mod stub_map;
pub mod stub_patches;
pub mod stubs;
mod symbol_index;
pub(crate) mod symbol_map;
mod symfony;
pub(crate) mod text_position;
pub(crate) mod text_scan;
pub(crate) mod toposort;
mod twig;
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
mod yii;

#[cfg(test)]
pub mod test_fixtures;

// ─── Re-exports ─────────────────────────────────────────────────────────────

// Re-export public types so that dependents (tests, main) can import them
// from the crate root, e.g. `use phpantom_lsp::{Backend, AccessKind}`.
pub use backend::package_origin::ClassCompletionOrigin;
pub use class_lookup::Fqn;
pub use completion::target::extract_completion_target;
pub use types::{AccessKind, ClassInfo, DefineInfo, FunctionInfo, NamespaceSpan, Visibility};
pub use virtual_members::resolve_class_fully;

// ─── Backend ────────────────────────────────────────────────────────────────

/// The main LSP backend.
///
/// Its behaviour is implemented in `impl Backend` blocks across the
/// feature modules; see the Module Layout section of
/// `docs/ARCHITECTURE.md` for where each lives.  The server state is a
/// [`BackendState`] every clone shares, reached through `Deref`, so
/// handing a request its own `Backend` (see
/// [`clone_for_blocking`](Backend::clone_for_blocking)) costs one
/// reference-count bump.
pub struct Backend {
    state: Arc<BackendState>,
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

impl std::ops::Deref for Backend {
    type Target = BackendState;

    fn deref(&self) -> &BackendState {
        &self.state
    }
}

/// The server state every clone of a [`Backend`] shares.
pub struct BackendState {
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
    /// What the last parse of each file produced: its symbol map, parse
    /// errors, imports, namespaces, resolved names and template lowering,
    /// published and removed as one record (see [`file_state`]).
    pub(crate) files: Arc<RwLock<file_state::FileStates>>,
    /// Per-file Symfony/Doctrine YAML/XML references: the namespace-prefix
    /// service keys and resource paths the resource symbol maps do not
    /// record, and the class and member names in the spelling each file
    /// writes them, which rename has to keep.
    pub(crate) framework_references: framework::FrameworkReferenceIndex,
    /// Doctrine entity-to-repository pairs from YAML/XML mappings, keyed by
    /// source URI so a watched change replaces one file's entries.
    pub(crate) framework_doctrine_repositories: framework::DoctrineRepositoryIndex,
    /// Cross-file candidate index for find-references.
    ///
    /// Maintained from each file's symbol map (see [`files`](Self::files)) during parsing.
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
    /// Per-URI locks for background `didChange` parses.
    ///
    /// `didChange` handlers offload parsing to blocking tasks.  Without a
    /// per-file lock, an older parse can finish after a newer edit and publish
    /// stale symbol state.  These locks serialize parse commits per URI; the
    /// handler also verifies that the captured text is still current before
    /// updating shared maps.
    pub(crate) did_change_parse_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// The last open file `update_ast` parsed, kept for the diagnostic
    /// pass its edit schedules, keyed by URI.
    ///
    /// One slot rather than one per file: only the file being typed in is
    /// diagnosed right after its parse, and the parse of a large file
    /// holds tens of megabytes.
    pub(crate) parsed_for_diagnostics: Mutex<Option<(String, crate::parser::ParsedFile)>>,
    /// Coalescing state for expensive whole-file requests. See
    /// [`WholeFileCoalesce`] for why this exists.
    pub(crate) whole_file_coalesce: Arc<WholeFileCoalesce>,
    pub(crate) client: Option<Client>,
    /// Whether to update ASTs synchronously.  Used for testing.
    pub(crate) sync_ast_updates: bool,
    /// Workspace-level configuration and PSR-4/vendor location metadata.
    pub(crate) workspace: WorkspaceEnv,
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
    /// `find_or_load_function`, `find_or_load_define`,
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
    /// The project's Laravel indexes and caches.
    pub(crate) laravel: backend::laravel::LaravelState,
    /// The project's Blade templates, beyond each one's own lowering.
    pub(crate) blade: blade::state::BladeState,
    /// Whether the workspace is an application rather than a library.
    ///
    /// A library's configuration is supplied by whatever application
    /// installs it, so the config keys it reads are declared in a file we
    /// never see and none of them can be judged.  See
    /// [`crate::composer::is_application_project`].  Starts `true` so a
    /// workspace with no `composer.json` to classify behaves as before.
    pub(crate) is_application: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the project uses Doctrine, which the Doctrine entity and
    /// repository lenses are gated on.  See
    /// [`crate::composer::is_doctrine_project`].  Starts `true` so a
    /// workspace with no `composer.json` to classify behaves as before.
    pub(crate) is_doctrine: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the project builds a Symfony service container, which
    /// service IDs and container parameters are read in.  See
    /// [`crate::composer::is_symfony_project`].  Starts `true` so a
    /// workspace with no `composer.json` to classify behaves as before.
    pub(crate) is_symfony: Arc<std::sync::atomic::AtomicBool>,
    /// The files that declare Symfony service IDs or container parameters.
    pub(crate) symfony: symfony::SymfonyState,
    /// Whether the project uses Twig, which template names are read in.
    /// See [`crate::composer::is_twig_project`].  Starts `true` so a
    /// workspace with no `composer.json` to classify behaves as before.
    pub(crate) is_twig: Arc<std::sync::atomic::AtomicBool>,
    /// The project's Twig template files and the roots they live under.
    pub(crate) twig: twig::TwigState,
    /// Whether the project renders Yii views, which view names are read
    /// in.  See [`crate::composer::is_yii_project`].  Starts `true` so a
    /// workspace with no `composer.json` to classify behaves as before.
    pub(crate) is_yii: Arc<std::sync::atomic::AtomicBool>,
    /// The Yii path aliases the project's configuration declares.
    pub(crate) yii: yii::YiiState,
    /// The `Pest.php` files of the project's test directories, read on
    /// first use and dropped when one is saved.  `None` until then.
    pub(crate) pest_configs: Arc<RwLock<Option<pest::PestConfigs>>>,
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
    /// Where the stub paths (`[stubs]` in `.phpantom.toml` and the editor's
    /// stub directory) declare each name.  Consulted by the class,
    /// function and constant lookups ahead of everything else, since a stub
    /// path's declaration wins over every other.  Rebuilt whole when the
    /// paths change; see [`refresh_stub_paths`](Self::refresh_stub_paths).
    pub(crate) external_stubs: Arc<RwLock<Arc<indexing::stub_paths::ExternalStubIndex>>>,
    /// Whether `external_stubs` holds any stub path, read before its lock
    /// so the class and function lookups, which run millions of times in
    /// a diagnostic pass, pay one atomic load when there are none.
    pub(crate) has_external_stubs: Arc<std::sync::atomic::AtomicBool>,
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
    /// What the connected client supports, read from its `initialize`
    /// request (see [`ClientCaps`]).
    pub(crate) client_caps: Arc<RwLock<backend::ClientCaps>>,
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
    /// How far the workspace index has got.
    pub(crate) workspace_index: backend::WorkspaceIndexState,
}
