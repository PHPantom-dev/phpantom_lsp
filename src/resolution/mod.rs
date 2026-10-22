//! Cross-file class, function, and constant resolution.
//!
//! This module contains the heavyweight name-resolution logic that every
//! feature shares: completion, hover, go-to-definition, diagnostics, and
//! the type engine.
//!
//! # Resolution pipeline
//!
//! ## Class resolution ([`Backend::find_or_load_class`](crate::Backend::find_or_load_class))
//!
//! - **Phase 0, already-parsed files** — O(1) FQN lookup in the parsed
//!   classes
//! - **Phase 0.5, built-in names** — a global-namespace name the embedded
//!   stubs know resolves to the stub, ahead of any polyfill in vendor
//! - **Phase 1, class index** — direct FQN → URI lookup (covers non-PSR-4
//!   classes and Composer classmap entries)
//! - **Phase 2, PSR-4 resolution** — convert namespace to file path and
//!   parse
//! - **Phase 3, embedded stubs** — built-in PHP classes/interfaces bundled
//!   in the binary (e.g. `UnitEnum`, `BackedEnum`, `Iterator`)
//!
//! ## Function resolution ([`Backend::find_or_load_function`](crate::Backend::find_or_load_function))
//!
//! - **Phase 1, `global_functions`** — user code + already-cached stubs
//! - **Phase 1.5, autoload function index** — parse the file the
//!   byte-level scan found the function in
//! - **Phase 1.75, autoload files** — parse the remaining autoload files,
//!   for functions declared inside a `function_exists()` guard
//! - **Phase 2, embedded stubs** — built-in PHP functions from
//!   phpstorm-stubs
//!
//! ## Constant resolution ([`Backend::lookup_global_constant`](crate::Backend::lookup_global_constant))
//!
//!   The same order as functions: parsed defines, the autoload constant
//!   index, the remaining autoload files, then the embedded stubs.  The
//!   first three are [`Backend::find_or_load_define`](crate::Backend::find_or_load_define),
//!   which go-to-definition shares.
//!
//! ## Name resolution ([`Backend::class_loader_with`](crate::Backend::class_loader_with), [`Backend::resolve_function_name`](crate::Backend::resolve_function_name))
//!
//!   These take a raw name as it appears in source code and resolve it to
//!   a concrete `ClassInfo` or `FunctionInfo` using the file's `use`
//!   statement mappings and namespace context.  They handle:
//!
//!   - Fully-qualified names (`\PDO`, `\Couchbase\Cluster`)
//!   - Unqualified names resolved via the import table or current namespace
//!   - Qualified names with alias expansion and namespace prefixing
//!
//!   A name the engine already resolved goes through
//!   [`Backend::resolved_class_loader_with`](crate::Backend::resolved_class_loader_with) instead.
//!
//! The work is split by job:
//!
//! - [`classes`]: [`Backend::find_or_load_class`](crate::Backend::find_or_load_class) and linking a loaded
//!   class.
//! - [`lazy_parse`]: parsing a file, stub, or phar entry on first lookup.
//! - [`inflight`]: making concurrent parses of one file wait for the
//!   first.
//! - [`functions`]: [`Backend::find_or_load_function`](crate::Backend::find_or_load_function) and name lookups
//!   against the index.
//! - [`constants`]: the constant counterpart,
//!   [`Backend::lookup_global_constant`](crate::Backend::lookup_global_constant).
//! - [`loaders`]: the loader closures handed to the type engine.

mod classes;
mod constants;
mod functions;
mod inflight;
mod lazy_parse;
mod loaders;

pub(crate) use classes::{ClassLoadHooks, needs_linking};
pub(crate) use constants::{DefineSearch, unversioned_php_version_constant_type};
pub(crate) use inflight::ParseInflight;
pub(crate) use loaders::FileLoaders;

#[cfg(test)]
mod stub_patch_consistency_tests;
#[cfg(test)]
mod tests;
