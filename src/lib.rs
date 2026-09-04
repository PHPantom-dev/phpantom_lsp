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
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use parking_lot::{Mutex, RwLock};
use tower_lsp::Client;
use tower_lsp::lsp_types::{CompletionItem, FileChangeType};

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

/// The `[indexing] extensions` set and Laravel classification last pushed
/// to the client as a `workspace/didChangeWatchedFiles` registration:
/// `(extra_extensions, is_laravel)`. `None` until the first registration.
/// See [`Backend::registered_watcher_state`].
pub(crate) type WatchedFileRegistrationState = Option<(Vec<String>, bool)>;

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
mod formatting;
mod highlight;
mod hover;
mod indexing;
pub(crate) mod inheritance;
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
mod parser;
pub(crate) mod phar;
pub mod php_type;
mod phpcs;
mod phpstan;
pub(crate) mod phpstan_ignore;
pub(crate) mod process;
pub mod progress;
mod reference_counts;
mod reference_index;
mod references;
mod rename;
mod resolution;
pub(crate) mod return_collection;
pub(crate) mod scope_collector;
mod selection_range;
#[cfg(not(target_arch = "wasm32"))]
pub mod self_update;
mod semantic_tokens;
mod server;
mod signature_help;
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
/// - `type_engine::resolver` — `resolve_target_classes` and type-resolution helpers
/// - `completion::builder` — module-level `build_completion_items`, `build_method_label`
/// - [`composer`] — PSR-4 autoload mapping and class file resolution
/// - `server` — `impl LanguageServer` (initialize, completion, did_open, …)
/// - `resolution` — `find_or_load_class`, `find_or_load_function`, `resolve_class_name`,
///   `resolve_function_name`
/// - `inheritance` — `resolve_class_with_inheritance` (base resolution), trait/parent merging
/// - `virtual_members` — `resolve_class_fully` (base resolution + virtual member providers),
///   `VirtualMemberProvider` trait, merge logic, provider registry
/// - `type_engine::subject_extraction` — Shared subject extraction helpers for `->`, `?->`, `::` operators
/// - `util` — module-level `position_to_offset`, `find_class_at_offset`,
///   `find_class_by_name`, plus `log`, `get_classes_for_uri`
/// - `definition` — `resolve_definition`, member resolution, function resolution
/// - `diagnostics` — `publish_diagnostics_for_file`, `clear_diagnostics_for_file`,
///   `collect_deprecated_diagnostics`, `collect_unused_import_diagnostics`,
///   `collect_unknown_class_diagnostics`,
///   `collect_unknown_member_diagnostics` (includes unresolved-member-access logic)
#[derive(Default)]
pub(crate) struct LaravelStringKeyCache {
    /// Named routes with the URI each was registered with, plus any group
    /// prefixes whose full set of children is unknowable.  Shared behind an
    /// `Arc` because both the name list and the parameter names of one route
    /// are read from it, and cloning the whole table per read would be waste.
    pub routes: Option<std::sync::Arc<crate::virtual_members::laravel::RouteDiscovery>>,
    pub config_keys: Option<Vec<String>>,
    pub view_names: Option<Vec<String>>,
    pub trans_keys: Option<Vec<String>>,
    /// Every translation key mapped to whether it names a group (nested
    /// array) rather than a scalar entry.  Shared behind an `Arc` for the
    /// same reason as `routes`: consumers look up one key per call and
    /// cloning the whole map per lookup would be waste.
    pub trans_key_shapes: Option<std::sync::Arc<HashMap<String, bool>>>,
    /// The Blade templates and component classes the project ships, keyed
    /// by the names Laravel addresses them under.  Shared behind an `Arc`
    /// because consumers look up a single name in one of its three maps and
    /// cloning the whole index per lookup would be waste.
    pub blade_discovery: Option<std::sync::Arc<crate::blade::discovery::BladeDiscovery>>,
    /// The section and stack names every template of the project writes,
    /// with what each one extends and includes.  Shared behind an `Arc`
    /// because an edit updates the entry of the one template that changed
    /// rather than replacing the whole index.
    pub blade_blocks: Option<std::sync::Arc<crate::blade::block_index::BladeBlockIndex>>,
    pub config_trees: Option<
        Vec<(
            String,
            crate::virtual_members::laravel::config_values::ConfigNode,
        )>,
    >,
    /// The variables service providers share into every template, and the
    /// ones their view composers add to the templates they target, with each
    /// value expression already resolved to a type.  Shared behind an `Arc`
    /// because every Blade template reads the whole set to pick the groups
    /// that reach it.
    pub shared_view_vars: Option<std::sync::Arc<Vec<crate::blade::shared_vars::SharedVarGroup>>>,
    /// Every authorization ability the project knows: `Gate::define()`
    /// registrations plus the methods of every policy class.
    pub gate_abilities: Option<Vec<String>>,
}

/// Compute-once guards for the entries of [`LaravelStringKeyCache`].
///
/// Every enumeration behind that cache walks the workspace from disk.
/// A plain check-then-fill cache stampedes under the parallel
/// diagnostic pass: all workers miss the empty slot in the same
/// instant, and each one repeats the identical walk while the others
/// queue behind the workspace index lock. Holding the matching guard
/// across the build means one worker walks and the rest wait once,
/// then read the filled slot.
///
/// One guard per slot rather than one shared guard: the enumerations
/// are independent, and a shared guard would let a worker that needs
/// view names wait out an unrelated route-name build.
#[derive(Default)]
pub(crate) struct LaravelStringKeyBuildLocks {
    pub routes: parking_lot::Mutex<()>,
    pub config_keys: parking_lot::Mutex<()>,
    pub view_names: parking_lot::Mutex<()>,
    pub trans_keys: parking_lot::Mutex<()>,
    pub trans_key_shapes: parking_lot::Mutex<()>,
    pub config_trees: parking_lot::Mutex<()>,
    pub blade_discovery: parking_lot::Mutex<()>,
    pub blade_blocks: parking_lot::Mutex<()>,
    pub shared_view_vars: parking_lot::Mutex<()>,
    pub gate_abilities: parking_lot::Mutex<()>,
}

impl LaravelStringKeyCache {
    fn invalidate_for_uri(&mut self, uri: &str, content: &str) {
        // Abilities come from `Gate::define()` calls and from the methods of
        // every policy class, so an edit to either invalidates the set.  The
        // token is `Gate::` rather than `Gate` so an unrelated `Gateway` does
        // not throw the enumeration away on every keystroke.
        if uri.ends_with("Policy.php")
            || memchr::memmem::find(content.as_bytes(), b"Gate::").is_some()
        {
            self.gate_abilities = None;
        }
        // A Folio page registers a route without ever touching `routes/`, so
        // its own edits have to invalidate the route cache too — gated on
        // the page mentioning `Folio` at all (its `name()` import or a
        // fully-qualified call), the same way the `Gate::` check above
        // avoids paying for every unrelated Blade edit.  `bootstrap/app.php`
        // is where a Folio mount is most commonly registered
        // (`withRouting(pages: ...)`), and it is not a service provider, so
        // it needs its own trigger rather than riding along with one.
        if uri.contains("/routes/")
            || uri.ends_with("/bootstrap/app.php")
            || (uri.ends_with(".blade.php")
                && memchr::memmem::find(content.as_bytes(), b"Folio").is_some())
        {
            self.routes = None;
        }
        if uri.contains("/config/") {
            self.config_keys = None;
            self.config_trees = None;
        }
        // View roots are configurable via `config/view.php`, so a Blade
        // file may live outside `resources/views/`. Invalidate on any
        // Blade file, any path containing a `views/` directory, and on
        // `config/view.php` itself (which changes the set of roots).
        if uri.ends_with(".blade.php")
            || uri.contains("/views/")
            || uri.contains("/config/view.php")
        {
            self.view_names = None;
        }
        if uri.contains("/lang/") || uri.contains("/resources/lang/") {
            self.trans_keys = None;
            self.trans_key_shapes = None;
        }
    }
}

/// Shared state for one external diagnostic tool's dedicated background
/// worker (PHPStan, PHPCS, Mago lint, Mago analyze).
///
/// Each worker runs as its own task so that a slow external process never
/// blocks native diagnostics or the other external tools. At most one
/// process per tool runs at a time; if the user edits while it is running,
/// `pending_uri` is overwritten and the worker picks up the newest file
/// once the current run finishes (older requests are superseded, not
/// queued — these tools are too slow to queue).
pub(crate) struct ExternalToolWorker {
    /// Wakes the worker task when a new URI is pending.
    pub(crate) notify: Arc<tokio::sync::Notify>,
    /// The single file URI the worker should analyse next.
    pub(crate) pending_uri: Arc<Mutex<Option<String>>>,
    /// Last-published diagnostics per file URI, merged into fast/slow
    /// diagnostic publishes so results stay visible between tool runs.
    pub(crate) last_diags: Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>>,
    /// Per-URI write counter, bumped every time this worker stores a
    /// single-file result in [`last_diags`].
    ///
    /// A project-wide run of the same tool snapshots this map before it
    /// starts and compares each entry again just before writing, so its
    /// scan-time results can never clobber a fresher single-file result
    /// that landed while the project-wide run was still in flight.
    generations: Arc<Mutex<HashMap<String, u64>>>,
}

impl ExternalToolWorker {
    fn new() -> Self {
        Self {
            notify: Arc::new(tokio::sync::Notify::new()),
            pending_uri: Arc::new(Mutex::new(None)),
            last_diags: Arc::new(Mutex::new(HashMap::new())),
            generations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Store a single-file run's result and mark the URI as freshly
    /// written, superseding any project-wide run still in flight.
    pub(crate) fn store_file_result(
        &self,
        uri: &str,
        diags: Vec<tower_lsp::lsp_types::Diagnostic>,
    ) {
        let mut cache = self.last_diags.lock();
        cache.insert(uri.to_string(), diags);
        *self.generations.lock().entry(uri.to_string()).or_insert(0) += 1;
    }

    /// Snapshot the per-URI write counters, to be handed back to
    /// [`store_scan_result`](Self::store_scan_result) once a
    /// project-wide run of this tool finishes.
    pub(crate) fn generation_snapshot(&self) -> HashMap<String, u64> {
        self.generations.lock().clone()
    }

    /// Store a project-wide run's result for one URI, unless a
    /// single-file run has written to that URI since `snapshot` was
    /// taken.  Returns whether the write happened.
    ///
    /// Both locks are taken in the same order as `store_file_result`,
    /// which makes the check-then-write atomic against it.
    pub(crate) fn store_scan_result(
        &self,
        snapshot: &HashMap<String, u64>,
        uri: &str,
        diags: Vec<tower_lsp::lsp_types::Diagnostic>,
    ) -> bool {
        let mut cache = self.last_diags.lock();
        if self.generations.lock().get(uri).copied() != snapshot.get(uri).copied() {
            return false;
        }
        cache.insert(uri.to_string(), diags);
        true
    }

    /// Drop a URI's cached result and write counter (on `did_close`).
    pub(crate) fn forget(&self, uri: &str) {
        self.last_diags.lock().remove(uri);
        self.generations.lock().remove(uri);
    }
}

impl Clone for ExternalToolWorker {
    fn clone(&self) -> Self {
        Self {
            notify: Arc::clone(&self.notify),
            pending_uri: Arc::clone(&self.pending_uri),
            last_diags: Arc::clone(&self.last_diags),
            generations: Arc::clone(&self.generations),
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
    /// find-references, rename, or inlay-hints request, so populating
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
    /// Reverse index mapping a related-model FQN to the pivot type exposed on
    /// its `$pivot` attribute when reached through a many-to-many relationship.
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
    /// The Blade directives the project's providers register, expanded from
    /// the merged table's `custom_directives` into every name a template can
    /// write.  Kept separate from the table because the Blade preprocessor
    /// reads it on every keystroke in a template and must not pay for the
    /// expansion each time.
    pub(crate) blade_custom_directives: Arc<RwLock<blade::directives::CustomDirectives>>,
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
    pub(crate) member_completion_cache: Arc<Mutex<HashMap<String, Vec<CompletionItem>>>>,
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
    /// Whether the client supports server-initiated work-done progress.
    ///
    /// Set during `initialize` based on the client's
    /// `window.workDoneProgress` capability.  When `false`, the server
    /// must not send `window/workDoneProgress/create` requests because
    /// the client will not handle them, blocking the server indefinitely.
    pub(crate) supports_work_done_progress: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the client supports dynamic registration for type hierarchy.
    pub(crate) supports_type_hierarchy_dynamic_registration: Arc<std::sync::atomic::AtomicBool>,
    /// The `[indexing] extensions` set and Laravel classification last
    /// pushed to the client as a `workspace/didChangeWatchedFiles`
    /// registration. `None` until `initialized` performs the first
    /// registration.
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
    /// Whether the client supports `workspace/inlayHint/refresh`.
    ///
    /// Set during `initialize` from the client's
    /// `workspace.inlayHint.refreshSupport` capability.  The reference
    /// counts shown on declarations are computed in the background, so
    /// without a refresh the editor keeps the hints it pulled before they
    /// were ready.
    pub(crate) supports_inlay_hint_refresh: Arc<std::sync::atomic::AtomicBool>,
    /// Reference counts for member declarations, feeding the inlay hints.
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
    pub(crate) blade_virtual_content: Arc<RwLock<HashMap<String, String>>>,
    /// Source maps from virtual PHP back to original Blade positions.
    pub(crate) blade_source_maps:
        Arc<RwLock<HashMap<String, crate::blade::source_map::BladeSourceMap>>>,
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

/// The Laravel alias slot and the resolved-class cache that reads it.
///
/// The cache holds a handle to the same slot the `Backend` builds the alias
/// tables into, which is how the facade virtual member provider maps a
/// container-binding accessor to its concrete class: a provider is handed
/// the cache but never the `Backend`.
fn new_alias_slot_and_cache() -> (
    virtual_members::laravel::LaravelAliasSlot,
    virtual_members::ResolvedClassCache,
) {
    let slot = virtual_members::laravel::new_alias_slot();
    let cache = virtual_members::new_resolved_class_cache();
    cache.write().set_laravel_aliases(Arc::clone(&slot));
    (slot, cache)
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
        let (laravel_aliases, resolved_class_cache) = new_alias_slot_and_cache();
        Self {
            name: "PHPantom".to_string(),
            version: env!("PHPANTOM_GIT_VERSION").to_string(),
            client_name: Mutex::new(String::new()),
            open_files: Arc::new(RwLock::new(HashMap::new())),
            symbol_maps: Arc::new(RwLock::new(HashMap::new())),
            reference_index: reference_index::new_reference_index(),
            skip_reference_index: false,
            symbols: SymbolIndex::new(),
            workspace: WorkspaceEnv::new(),
            parse_errors: Arc::new(RwLock::new(HashMap::new())),
            did_change_parse_locks: Arc::new(Mutex::new(HashMap::new())),
            whole_file_coalesce: Arc::new(WholeFileCoalesce::default()),
            client: None,
            file_imports: Arc::new(RwLock::new(HashMap::new())),
            resolved_names: Arc::new(RwLock::new(HashMap::new())),
            file_namespaces: Arc::new(RwLock::new(HashMap::new())),
            phar_archives: Arc::new(RwLock::new(HashMap::new())),
            parsed_uris: Arc::new(RwLock::new(HashSet::new())),
            parse_inflight: Arc::new(resolution::ParseInflight::new()),
            stub_index: Arc::new(RwLock::new(CiMap::from(stubs::build_stub_class_index()))),
            stub_function_index: Arc::new(RwLock::new(CiMap::from(
                stubs::build_stub_function_index(),
            ))),
            stub_constant_index: Arc::new(RwLock::new(stubs::build_stub_constant_index())),
            resolved_class_cache,
            auth_user_type_cache: Arc::new(RwLock::new(HashMap::new())),
            storage_disk_type_cache: Arc::new(RwLock::new(None)),
            laravel_storage_drivers: Arc::new(RwLock::new(Default::default())),
            laravel_aliases,
            laravel_macros: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelMacroIndex::default(),
            )),
            laravel_has_macros: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_pivots: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelPivotIndex::default(),
            )),
            laravel_has_pivots: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_pivots_dirty: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            laravel_commands: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelCommandIndex::default(),
            )),
            laravel_has_commands: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_morph_map: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelMorphMapIndex::default(),
            )),
            laravel_gates: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelGateIndex::default(),
            )),
            laravel_macro_seeds: Arc::new(RwLock::new(HashMap::new())),
            laravel_macro_mixin_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            laravel_date_class: Arc::new(RwLock::new(None)),
            laravel_date_seed_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            laravel_provider_resources: Arc::new(RwLock::new(
                virtual_members::laravel::ProviderResources::default(),
            )),
            laravel_provider_scans: Arc::new(RwLock::new(
                virtual_members::laravel::ProviderScans::default(),
            )),
            blade_custom_directives: Arc::new(RwLock::new(
                blade::directives::CustomDirectives::default(),
            )),
            laravel_string_key_cache: Arc::new(RwLock::new(LaravelStringKeyCache::default())),
            laravel_string_key_build_locks: Arc::new(LaravelStringKeyBuildLocks::default()),
            schema_index: Arc::new(RwLock::new(
                virtual_members::laravel::database_schema::SchemaIndex::default(),
            )),
            member_completion_cache: Arc::new(Mutex::new(HashMap::new())),
            diag: crate::diagnostics::state::DiagnosticState::new(),
            phpstan_tool: ExternalToolWorker::new(),
            phpcs_tool: ExternalToolWorker::new(),
            mago_lint_tool: ExternalToolWorker::new(),
            mago_analyze_tool: ExternalToolWorker::new(),

            supports_pull_diagnostics: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_file_rename: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_work_done_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_type_hierarchy_dynamic_registration: Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ),
            registered_watcher_state: Arc::new(RwLock::new(None)),
            supports_show_document: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_semantic_tokens_refresh: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_inlay_hint_refresh: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            member_ref_counts: reference_counts::new_member_ref_counts(),
            init_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutdown_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            blade_virtual_content: Arc::new(RwLock::new(HashMap::new())),
            blade_source_maps: Arc::new(RwLock::new(HashMap::new())),
            blade_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            blade_injected_vars: Arc::new(RwLock::new(HashMap::new())),
            typed_receiver_view_spans_cache: Arc::new(RwLock::new(HashMap::new())),
            workspace_indexed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workspace_index_lock: Arc::new(Mutex::new(())),
            full_index_in_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workspace_index_status: Arc::new(Mutex::new(None)),
            request_progress: None,
            sync_ast_updates: false,
        }
    }

    /// Shared defaults for test Backend constructors.
    ///
    /// Identical to [`defaults`] but with **empty** stub indices, avoiding
    /// the cost of building three large `HashMap`s (14,597 entries total)
    /// that most tests never consult.  Tests that need specific stubs
    /// override the relevant fields after construction.
    ///
    /// The workspace environment is also isolated from the global
    /// `.phpantom.toml`, so a test asserts against the project config it
    /// writes itself rather than against the config directory of whoever
    /// happens to be running the suite.
    fn test_defaults() -> Self {
        let (laravel_aliases, resolved_class_cache) = new_alias_slot_and_cache();
        Self {
            name: "PHPantom".to_string(),
            version: env!("PHPANTOM_GIT_VERSION").to_string(),
            client_name: Mutex::new(String::new()),
            open_files: Arc::new(RwLock::new(HashMap::new())),
            symbol_maps: Arc::new(RwLock::new(HashMap::new())),
            reference_index: reference_index::new_reference_index(),
            skip_reference_index: false,
            symbols: SymbolIndex::new(),
            workspace: WorkspaceEnv::new_isolated(),
            parse_errors: Arc::new(RwLock::new(HashMap::new())),
            did_change_parse_locks: Arc::new(Mutex::new(HashMap::new())),
            whole_file_coalesce: Arc::new(WholeFileCoalesce::default()),
            client: None,
            file_imports: Arc::new(RwLock::new(HashMap::new())),
            resolved_names: Arc::new(RwLock::new(HashMap::new())),
            file_namespaces: Arc::new(RwLock::new(HashMap::new())),
            phar_archives: Arc::new(RwLock::new(HashMap::new())),
            parsed_uris: Arc::new(RwLock::new(HashSet::new())),
            parse_inflight: Arc::new(resolution::ParseInflight::new()),
            stub_index: Arc::new(RwLock::new(CiMap::new())),
            stub_function_index: Arc::new(RwLock::new(CiMap::new())),
            stub_constant_index: Arc::new(RwLock::new(HashMap::new())),
            resolved_class_cache,
            auth_user_type_cache: Arc::new(RwLock::new(HashMap::new())),
            storage_disk_type_cache: Arc::new(RwLock::new(None)),
            laravel_storage_drivers: Arc::new(RwLock::new(Default::default())),
            laravel_aliases,
            laravel_macros: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelMacroIndex::default(),
            )),
            laravel_has_macros: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_pivots: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelPivotIndex::default(),
            )),
            laravel_has_pivots: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_pivots_dirty: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            laravel_commands: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelCommandIndex::default(),
            )),
            laravel_has_commands: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            laravel_morph_map: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelMorphMapIndex::default(),
            )),
            laravel_gates: Arc::new(RwLock::new(
                virtual_members::laravel::LaravelGateIndex::default(),
            )),
            laravel_macro_seeds: Arc::new(RwLock::new(HashMap::new())),
            laravel_macro_mixin_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            laravel_date_class: Arc::new(RwLock::new(None)),
            laravel_date_seed_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            laravel_provider_resources: Arc::new(RwLock::new(
                virtual_members::laravel::ProviderResources::default(),
            )),
            laravel_provider_scans: Arc::new(RwLock::new(
                virtual_members::laravel::ProviderScans::default(),
            )),
            blade_custom_directives: Arc::new(RwLock::new(
                blade::directives::CustomDirectives::default(),
            )),
            laravel_string_key_cache: Arc::new(RwLock::new(LaravelStringKeyCache::default())),
            laravel_string_key_build_locks: Arc::new(LaravelStringKeyBuildLocks::default()),
            schema_index: Arc::new(RwLock::new(
                virtual_members::laravel::database_schema::SchemaIndex::default(),
            )),
            member_completion_cache: Arc::new(Mutex::new(HashMap::new())),
            diag: crate::diagnostics::state::DiagnosticState::new(),
            phpstan_tool: ExternalToolWorker::new(),
            phpcs_tool: ExternalToolWorker::new(),
            mago_lint_tool: ExternalToolWorker::new(),
            mago_analyze_tool: ExternalToolWorker::new(),
            supports_pull_diagnostics: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_file_rename: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_work_done_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_type_hierarchy_dynamic_registration: Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ),
            registered_watcher_state: Arc::new(RwLock::new(None)),
            supports_show_document: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_semantic_tokens_refresh: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            supports_inlay_hint_refresh: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            member_ref_counts: reference_counts::new_member_ref_counts(),
            init_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            shutdown_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            blade_virtual_content: Arc::new(RwLock::new(HashMap::new())),
            blade_source_maps: Arc::new(RwLock::new(HashMap::new())),
            blade_uris: Arc::new(RwLock::new(std::collections::HashSet::new())),
            blade_injected_vars: Arc::new(RwLock::new(HashMap::new())),
            typed_receiver_view_spans_cache: Arc::new(RwLock::new(HashMap::new())),
            workspace_indexed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workspace_index_lock: Arc::new(Mutex::new(())),
            full_index_in_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workspace_index_status: Arc::new(Mutex::new(None)),
            request_progress: None,
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
        Self {
            skip_reference_index: true,
            ..Self::defaults()
        }
    }

    /// Create a headless backend for refactoring commands that need the
    /// workspace-wide reference index.
    pub fn new_headless_refactoring() -> Self {
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
        let backend = Self {
            workspace: WorkspaceEnv::new_isolated(),
            ..Self::defaults()
        };
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
            stub_index: Arc::new(RwLock::new(CiMap::from(stub_index))),
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
            stub_index: Arc::new(RwLock::new(CiMap::from(stub_index))),
            stub_function_index: Arc::new(RwLock::new(CiMap::from(stub_function_index))),
            stub_constant_index: Arc::new(RwLock::new(stub_constant_index)),
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
            workspace: WorkspaceEnv {
                workspace_root: Arc::new(RwLock::new(Some(workspace_root))),
                psr4_mappings: Arc::new(RwLock::new(psr4_mappings)),
                ..WorkspaceEnv::new_isolated()
            },
            ..Self::test_defaults()
        }
    }

    // ── Public accessors for integration tests ──────────────────────────

    /// Borrow the workspace root mutex (used by integration tests to set a
    /// custom workspace directory).
    pub fn workspace_root(&self) -> &Arc<RwLock<Option<PathBuf>>> {
        &self.workspace.workspace_root
    }

    /// Borrow the global functions mutex (used by integration tests to
    /// inject user-defined functions or inspect the cache).
    pub fn global_functions(&self) -> &Arc<RwLock<CiMap<(String, FunctionInfo)>>> {
        &self.symbols.global_functions
    }

    /// The preprocessed virtual PHP for a Blade file (used by
    /// integration tests to diagnose the virtual content the way the
    /// live pipeline does).
    pub fn blade_virtual_php(&self, uri: &str) -> Option<String> {
        self.blade_virtual_content.read().get(uri).cloned()
    }

    /// Borrow the global defines mutex (used by integration tests to
    /// inject user-defined constants or inspect the cache).
    pub fn global_defines(&self) -> &Arc<RwLock<HashMap<String, DefineInfo>>> {
        &self.symbols.global_defines
    }

    /// Borrow the class index mutex (used by integration tests to
    /// populate discovered class entries).
    pub fn fqn_uri_index(&self) -> &Arc<RwLock<CiMap<String>>> {
        &self.symbols.fqn_uri_index
    }

    /// Borrow the FQN → ClassInfo index mutex (used by integration tests
    /// to populate class metadata for context-aware completion filtering).
    pub fn fqn_class_index(&self) -> &Arc<RwLock<CiMap<Arc<ClassInfo>>>> {
        &self.symbols.fqn_class_index
    }

    /// Borrow the PSR-4 mappings mutex (used by integration tests to
    /// configure autoload mappings).
    pub fn psr4_mappings(&self) -> &Arc<RwLock<Vec<composer::Psr4Mapping>>> {
        &self.workspace.psr4_mappings
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
        &self.symbols.autoload_function_index
    }

    pub fn autoload_function_origin_index(&self) -> &Arc<RwLock<CiMap<ClassCompletionOrigin>>> {
        &self.symbols.autoload_function_origin_index
    }

    /// Borrow the autoload constant index (used by integration tests to
    /// populate discovered constant entries for non-Composer projects).
    pub fn autoload_constant_index(&self) -> &Arc<RwLock<HashMap<String, PathBuf>>> {
        &self.symbols.autoload_constant_index
    }

    pub fn autoload_constant_origin_index(
        &self,
    ) -> &Arc<RwLock<HashMap<String, ClassCompletionOrigin>>> {
        &self.symbols.autoload_constant_origin_index
    }

    /// Borrow the autoload file paths list (used by integration tests
    /// to simulate Composer autoload file discovery).
    pub fn autoload_file_paths(&self) -> &Arc<RwLock<Vec<PathBuf>>> {
        &self.symbols.autoload_file_paths
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
        let vendor_paths = self.workspace.vendor_dir_paths.lock();
        let is_under_vendor = vendor_paths.iter().any(|vp| path.starts_with(vp));
        drop(vendor_paths);

        let roots = self.workspace.vendor_package_origin_roots.read();

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
                let ws = self.workspace.workspace_root.read();
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
        &self.phpstan_tool.last_diags
    }

    /// Borrow the PHPCS diagnostics cache (used by integration tests
    /// to inject PHPCS diagnostics without running PHPCS).
    pub fn phpcs_last_diags(
        &self,
    ) -> &Arc<Mutex<HashMap<String, Vec<tower_lsp::lsp_types::Diagnostic>>>> {
        &self.phpcs_tool.last_diags
    }

    /// Clear the member completion cache.
    pub fn clear_completion_cache(&self) {
        self.member_completion_cache.lock().clear();
    }

    /// Return the configured PHP version.
    pub fn php_version(&self) -> types::PhpVersion {
        *self.workspace.php_version.lock()
    }

    /// Populate the method store from a slice of classes.
    ///
    /// For each class, inserts every method under the key
    /// `(class_fqn, method.name)`.  Called from `update_ast_inner`
    /// and `parse_and_cache_content_versioned` after classes are parsed.
    pub(crate) fn populate_method_store(&self, classes: &[Arc<ClassInfo>]) {
        let mut store = self.symbols.method_store.write();
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
        let mut store = self.symbols.method_store.write();
        for fqn in fqns {
            store.retain(|k, _| k.0 != *fqn);
        }
    }

    /// Populate the GTI (go-to-implementation) reverse inheritance index
    /// for the given classes.  For each class, inserts the class's FQN
    /// into the child list of every parent (parent_class, interfaces,
    /// used_traits).
    pub(crate) fn populate_gti_index(&self, classes: &[Arc<ClassInfo>]) {
        let mut gti = self.symbols.gti_index.write();
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
        let mut gti = self.symbols.gti_index.write();
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

        // Drop every class declaration sourced from a changed file in one
        // pass, collecting the affected FQNs so the dependent caches can be
        // evicted without re-scanning.  A name a purged file shared with
        // another declaration is promoted to that one rather than dropped,
        // so deleting one of two files declaring a class does not make the
        // class unresolvable.
        let crate::symbol_index::WithdrawnClasses {
            dropped: dropped_fqns,
            promoted: promoted_fqns,
        } = self
            .symbols
            .with_class_declarations(|decls| decls.withdraw_uris(&uri_set));
        // These FQNs no longer resolve, and the promoted ones resolve
        // elsewhere; retire the memoised lookups.
        self.symbols.note_class_lookup_change();
        self.evict_methods_for_fqns(&dropped_fqns);
        self.evict_gti_for_fqns(&dropped_fqns);
        if !promoted_fqns.is_empty() {
            // The indexes derived from the class index still describe the
            // purged declaration, so rebuild them from the survivor.
            let winners: Vec<Arc<ClassInfo>> = {
                let fci = self.symbols.fqn_class_index.read();
                promoted_fqns
                    .iter()
                    .filter_map(|fqn| fci.get(fqn).map(Arc::clone))
                    .collect()
            };
            self.evict_methods_for_fqns(&promoted_fqns);
            self.evict_gti_for_fqns(&promoted_fqns);
            self.populate_method_store(&winners);
            self.populate_gti_index(&winners);
        }

        self.symbols
            .autoload_function_index
            .write()
            .retain(|_, v| !path_set.contains(v));
        self.symbols
            .autoload_constant_index
            .write()
            .retain(|_, v| !path_set.contains(v));
        self.withdraw_functions_for_uris(&uri_set);
        self.symbols
            .global_defines
            .write()
            .retain(|_, d| !uri_set.contains(d.file_uri.as_str()));

        // Per-URI keyed removals are cheap (no full scan).  Like the
        // retain-based purges above, clear both URI spellings: the editor
        // URI from the watcher event and the canonical `file://` URI the
        // background indexer stores.  Missing the canonical spelling would
        // leave a stale symbol map that also blocks re-parsing (the
        // workspace-index walk skips files that already have one).
        for (uri_str, path, _) in changes {
            let canonical_uri = crate::util::path_to_uri(path);
            let spellings = if canonical_uri == *uri_str {
                vec![uri_str.as_str()]
            } else {
                vec![uri_str.as_str(), canonical_uri.as_str()]
            };
            for uri in spellings {
                self.clear_file_maps(uri);
                self.symbols.uri_classes_index.write().remove(uri);
                self.parsed_uris.write().remove(uri);
                // The global_functions/global_defines entries for these URIs
                // were just retained out above; drop the per-URI tracking
                // record too so deleted files don't leave a stale entry
                // behind.  Created/changed files rebuild it when re-parsed
                // below.
                self.symbols.uri_globals_index.write().remove(uri);
            }
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
            self.symbols.with_class_declarations(|decls| {
                for fqn in classes {
                    decls.note_discovered(&fqn, uri_str.clone());
                }
            });

            let scan = crate::classmap_scanner::scan_file_full(path);
            {
                let mut fi = self.symbols.autoload_function_index.write();
                for fqn in scan.functions {
                    fi.or_insert_with(fqn, || path.clone());
                }
            }
            {
                let mut ci = self.symbols.autoload_constant_index.write();
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
            symbol_maps: Arc::clone(&self.symbol_maps),
            reference_index: Arc::clone(&self.reference_index),
            skip_reference_index: self.skip_reference_index,
            symbols: self.symbols.clone(),
            parse_errors: Arc::clone(&self.parse_errors),
            did_change_parse_locks: Arc::clone(&self.did_change_parse_locks),
            whole_file_coalesce: Arc::clone(&self.whole_file_coalesce),
            // RwLock fields are shared by Arc::clone — the diagnostic
            // worker reads them concurrently with the main Backend.
            client: self.client.clone(),
            file_imports: Arc::clone(&self.file_imports),
            resolved_names: Arc::clone(&self.resolved_names),
            file_namespaces: Arc::clone(&self.file_namespaces),
            phar_archives: Arc::clone(&self.phar_archives),
            parsed_uris: Arc::clone(&self.parsed_uris),
            parse_inflight: Arc::clone(&self.parse_inflight),
            stub_index: Arc::clone(&self.stub_index),
            resolved_class_cache: Arc::clone(&self.resolved_class_cache),
            auth_user_type_cache: Arc::clone(&self.auth_user_type_cache),
            storage_disk_type_cache: Arc::clone(&self.storage_disk_type_cache),
            laravel_storage_drivers: Arc::clone(&self.laravel_storage_drivers),
            laravel_aliases: Arc::clone(&self.laravel_aliases),
            laravel_macros: Arc::clone(&self.laravel_macros),
            laravel_has_macros: Arc::clone(&self.laravel_has_macros),
            laravel_pivots: Arc::clone(&self.laravel_pivots),
            laravel_has_pivots: Arc::clone(&self.laravel_has_pivots),
            laravel_pivots_dirty: Arc::clone(&self.laravel_pivots_dirty),
            laravel_commands: Arc::clone(&self.laravel_commands),
            laravel_has_commands: Arc::clone(&self.laravel_has_commands),
            laravel_morph_map: Arc::clone(&self.laravel_morph_map),
            laravel_gates: Arc::clone(&self.laravel_gates),
            laravel_macro_seeds: Arc::clone(&self.laravel_macro_seeds),
            laravel_macro_mixin_uris: Arc::clone(&self.laravel_macro_mixin_uris),
            laravel_date_class: Arc::clone(&self.laravel_date_class),
            laravel_date_seed_uris: Arc::clone(&self.laravel_date_seed_uris),
            laravel_provider_resources: Arc::clone(&self.laravel_provider_resources),
            laravel_provider_scans: Arc::clone(&self.laravel_provider_scans),
            blade_custom_directives: Arc::clone(&self.blade_custom_directives),
            laravel_string_key_cache: Arc::clone(&self.laravel_string_key_cache),
            laravel_string_key_build_locks: Arc::clone(&self.laravel_string_key_build_locks),
            schema_index: Arc::clone(&self.schema_index),
            member_completion_cache: Arc::clone(&self.member_completion_cache),
            stub_function_index: Arc::clone(&self.stub_function_index),
            stub_constant_index: Arc::clone(&self.stub_constant_index),
            diag: self.diag.clone(),
            workspace: self.workspace.clone(),
            phpstan_tool: self.phpstan_tool.clone(),
            phpcs_tool: self.phpcs_tool.clone(),
            mago_lint_tool: self.mago_lint_tool.clone(),
            mago_analyze_tool: self.mago_analyze_tool.clone(),
            supports_pull_diagnostics: Arc::clone(&self.supports_pull_diagnostics),
            supports_file_rename: Arc::clone(&self.supports_file_rename),
            supports_work_done_progress: Arc::clone(&self.supports_work_done_progress),
            supports_type_hierarchy_dynamic_registration: Arc::clone(
                &self.supports_type_hierarchy_dynamic_registration,
            ),
            registered_watcher_state: Arc::clone(&self.registered_watcher_state),
            supports_show_document: Arc::clone(&self.supports_show_document),
            supports_semantic_tokens_refresh: Arc::clone(&self.supports_semantic_tokens_refresh),
            supports_inlay_hint_refresh: Arc::clone(&self.supports_inlay_hint_refresh),
            member_ref_counts: Arc::clone(&self.member_ref_counts),
            init_complete: Arc::clone(&self.init_complete),
            shutdown_flag: Arc::clone(&self.shutdown_flag),
            blade_virtual_content: Arc::clone(&self.blade_virtual_content),
            blade_source_maps: Arc::clone(&self.blade_source_maps),
            blade_uris: Arc::clone(&self.blade_uris),
            blade_injected_vars: Arc::clone(&self.blade_injected_vars),
            typed_receiver_view_spans_cache: Arc::clone(&self.typed_receiver_view_spans_cache),
            workspace_indexed: Arc::clone(&self.workspace_indexed),
            workspace_index_lock: Arc::clone(&self.workspace_index_lock),
            full_index_in_progress: Arc::clone(&self.full_index_in_progress),
            workspace_index_status: Arc::clone(&self.workspace_index_status),
            // Deliberately not propagated: a request's progress sink is
            // attached explicitly to that request's own clone only.
            request_progress: None,
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
        self.workspace.config.lock().clone()
    }

    /// Replace the current configuration.
    ///
    /// Used when (re)loading `.phpantom.toml` and by integration tests
    /// to enable opt-in diagnostics like `unresolved-member-access`
    /// without needing a `.phpantom.toml` file. Resets the compiled
    /// `[indexing]` filters so the next scan sees the new settings.
    pub fn set_config(&self, config: config::Config) {
        *self.workspace.config.lock() = config;
        *self.workspace.index_filters.write() = None;
    }

    /// Return the compiled `[indexing]` exclude globs and extra PHP
    /// extensions, building them on first use from the union of the
    /// `.phpantom.toml` layer and the client-supplied layer.
    ///
    /// The two layers are unioned rather than one overriding the other:
    /// both name files that are not worth indexing, and a client cannot
    /// know what a project's config file already excludes. The compiled
    /// result is cached until [`set_config`](Self::set_config) or
    /// [`set_client_indexing_options`](Self::set_client_indexing_options)
    /// invalidates it, so glob compilation never runs on a per-file path.
    pub(crate) fn index_filters(&self) -> Arc<classmap_scanner::IndexFilters> {
        if let Some(filters) = self.workspace.index_filters.read().as_ref() {
            return Arc::clone(filters);
        }
        let root = self.workspace.workspace_root.read().clone();
        let indexing = self.config().indexing;
        let client = self.workspace.client_indexing.read();

        // The overwhelmingly common case is one layer or the other being
        // empty, so only pay for a merged allocation when both contribute.
        let compiled = if client.is_empty() {
            classmap_scanner::IndexFilters::compile(
                root.as_deref(),
                indexing.exclude(),
                indexing.extensions(),
            )
        } else {
            // The config file's patterns go last so a `!` re-include in
            // `.phpantom.toml` can still override an exclude the editor
            // forwarded: gitignore semantics give the last match priority.
            let exclude = [client.exclude.as_slice(), indexing.exclude()].concat();
            let extensions = [client.extensions.as_slice(), indexing.extensions()].concat();
            classmap_scanner::IndexFilters::compile(root.as_deref(), &exclude, &extensions)
        };
        drop(client);

        let compiled = Arc::new(compiled);
        *self.workspace.index_filters.write() = Some(Arc::clone(&compiled));
        compiled
    }

    /// Replace the client-supplied file filters.
    ///
    /// Called from the `initialize` handshake and again whenever a
    /// `workspace/didChangeConfiguration` arrives. Returns whether the
    /// filters actually changed, so the caller can skip the rescan and
    /// watcher churn a no-op settings push would otherwise cause (clients
    /// re-send their whole settings tree for edits to unrelated keys).
    pub fn set_client_indexing_options(&self, options: config::ClientIndexingOptions) -> bool {
        let mut current = self.workspace.client_indexing.write();
        if *current == options {
            return false;
        }
        *current = options;
        drop(current);
        *self.workspace.index_filters.write() = None;
        true
    }

    /// Record whether the client handles `window/showDocument`.
    ///
    /// Set from the `initialize` handshake; integration tests use it to
    /// exercise both code lens command shapes.
    pub fn set_supports_show_document(&self, supported: bool) {
        self.supports_show_document
            .store(supported, std::sync::atomic::Ordering::Release);
    }

    /// Set the PHP version (used by integration tests and during
    /// server initialization after reading `composer.json`).
    ///
    /// Also filters `stub_function_index`, `stub_index`, and
    /// `stub_constant_index` to remove entries that do not exist in
    /// the given PHP version.
    pub fn set_php_version(&self, version: types::PhpVersion) {
        *self.workspace.php_version.lock() = version;
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

    /// Translate a position from a virtual PHP file back to the original Blade
    /// file, or `None` when the position lies in the injected prologue.
    ///
    /// Use this instead of [`Self::translate_php_to_blade`] wherever the
    /// result becomes a text edit or a range the editor navigates to: the
    /// prologue has no template text behind it, and clamping a prologue
    /// position to `0:0` produces an edit at the very start of the template.
    pub(crate) fn try_translate_php_to_blade(
        &self,
        uri: &str,
        pos: tower_lsp::lsp_types::Position,
    ) -> Option<tower_lsp::lsp_types::Position> {
        match self.blade_source_maps.read().get(uri) {
            Some(map) => map.try_php_to_blade(pos),
            None => Some(pos),
        }
    }

    /// Translate a range from virtual PHP coordinates back to original Blade
    /// coordinates, dropping it when either end lies in the prologue.
    pub(crate) fn try_translate_blade_range(
        &self,
        uri: &str,
        range: tower_lsp::lsp_types::Range,
    ) -> Option<tower_lsp::lsp_types::Range> {
        Some(tower_lsp::lsp_types::Range {
            start: self.try_translate_php_to_blade(uri, range.start)?,
            end: self.try_translate_php_to_blade(uri, range.end)?,
        })
    }

    /// Translate one completion item's edit ranges from virtual PHP
    /// coordinates back to Blade, dropping the item if any range falls in
    /// the injected prologue.
    fn translate_completion_item(
        &self,
        uri: &str,
        mut item: tower_lsp::lsp_types::CompletionItem,
    ) -> Option<tower_lsp::lsp_types::CompletionItem> {
        use tower_lsp::lsp_types::CompletionTextEdit;

        if let Some(edit) = item.text_edit.take() {
            item.text_edit = Some(match edit {
                CompletionTextEdit::Edit(mut e) => {
                    e.range = self.try_translate_blade_range(uri, e.range)?;
                    CompletionTextEdit::Edit(e)
                }
                CompletionTextEdit::InsertAndReplace(mut e) => {
                    e.insert = self.try_translate_blade_range(uri, e.insert)?;
                    e.replace = self.try_translate_blade_range(uri, e.replace)?;
                    CompletionTextEdit::InsertAndReplace(e)
                }
            });
        }

        if let Some(edits) = item.additional_text_edits.take() {
            let mut translated = Vec::with_capacity(edits.len());
            for mut e in edits {
                e.range = self.try_translate_blade_range(uri, e.range)?;
                translated.push(e);
            }
            item.additional_text_edits = Some(translated);
        }

        Some(item)
    }

    /// Translate every completion item in a response from virtual PHP
    /// coordinates back to Blade, dropping items whose edits target the
    /// preprocessor's injected prologue rather than clamping them to the
    /// start of the template.
    pub(crate) fn translate_completion_response(
        &self,
        uri: &str,
        response: tower_lsp::lsp_types::CompletionResponse,
    ) -> tower_lsp::lsp_types::CompletionResponse {
        use tower_lsp::lsp_types::{CompletionList, CompletionResponse};

        match response {
            CompletionResponse::Array(items) => CompletionResponse::Array(
                items
                    .into_iter()
                    .filter_map(|item| self.translate_completion_item(uri, item))
                    .collect(),
            ),
            CompletionResponse::List(list) => CompletionResponse::List(CompletionList {
                is_incomplete: list.is_incomplete,
                items: list
                    .items
                    .into_iter()
                    .filter_map(|item| self.translate_completion_item(uri, item))
                    .collect(),
            }),
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

    /// Like [`Self::translate_location`], but drops a Blade location whose
    /// range falls inside the preprocessor's prologue.
    pub(crate) fn try_translate_location(
        &self,
        mut location: tower_lsp::lsp_types::Location,
    ) -> Option<tower_lsp::lsp_types::Location> {
        let uri_str = location.uri.to_string();
        if self.is_blade_file(&uri_str) {
            location.range = self.try_translate_blade_range(&uri_str, location.range)?;
        }
        Some(location)
    }

    /// Translate every text edit in a workspace edit from virtual PHP
    /// coordinates back to Blade, dropping the ones that target a template's
    /// injected prologue.
    ///
    /// Covers both `changes` and `document_changes`; a class rename that also
    /// renames its file uses the latter.
    pub(crate) fn translate_workspace_edit(&self, edit: &mut tower_lsp::lsp_types::WorkspaceEdit) {
        use tower_lsp::lsp_types::{DocumentChangeOperation, DocumentChanges};

        if let Some(changes) = &mut edit.changes {
            changes.retain(|uri, edits| {
                let uri_str = uri.to_string();
                if !self.is_blade_file(&uri_str) {
                    return true;
                }
                edits.retain_mut(
                    |e| match self.try_translate_blade_range(&uri_str, e.range) {
                        Some(range) => {
                            e.range = range;
                            true
                        }
                        None => false,
                    },
                );
                !edits.is_empty()
            });
        }

        match &mut edit.document_changes {
            Some(DocumentChanges::Edits(edits)) => {
                edits.retain_mut(|e| self.translate_text_document_edit(e));
            }
            Some(DocumentChanges::Operations(ops)) => ops.retain_mut(|op| match op {
                DocumentChangeOperation::Edit(e) => self.translate_text_document_edit(e),
                DocumentChangeOperation::Op(_) => true,
            }),
            None => {}
        }
    }

    /// Translate one document's edits back to Blade coordinates, returning
    /// `false` when every edit it held targeted the prologue.
    fn translate_text_document_edit(
        &self,
        edit: &mut tower_lsp::lsp_types::TextDocumentEdit,
    ) -> bool {
        use tower_lsp::lsp_types::OneOf;

        let uri_str = edit.text_document.uri.to_string();
        if !self.is_blade_file(&uri_str) {
            return true;
        }
        edit.edits.retain_mut(|e| {
            let range = match e {
                OneOf::Left(e) => &mut e.range,
                OneOf::Right(e) => &mut e.text_edit.range,
            };
            match self.try_translate_blade_range(&uri_str, *range) {
                Some(translated) => {
                    *range = translated;
                    true
                }
                None => false,
            }
        });
        !edit.edits.is_empty()
    }
}
