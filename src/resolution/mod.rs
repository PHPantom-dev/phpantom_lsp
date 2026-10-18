//! Cross-file class and function resolution.
//!
//! This module contains the heavyweight name-resolution logic that is
//! shared by the completion handler, definition resolver, and
//! named-argument resolution.
//!
//! # Resolution pipeline
//!
//! ## Class resolution ([`Backend::find_or_load_class`](crate::Backend::find_or_load_class))
//!
//!   0. **Class index** — direct FQN → URI lookup (covers non-PSR-4 classes
//!      and Composer classmap entries)
//!   1. **uri_classes_index scan** — search all already-parsed files by short name,
//!      with namespace verification when a qualified name is requested
//!   2. **PSR-4 resolution** — convert namespace to file path and parse
//!   3. **Embedded stubs** — built-in PHP classes/interfaces bundled in
//!      the binary (e.g. `UnitEnum`, `BackedEnum`, `Iterator`)
//!
//! ## Function resolution ([`Backend::find_or_load_function`](crate::Backend::find_or_load_function))
//!
//!   1. **global_functions** — user code + already-cached stubs
//!   2. **Embedded stubs** — built-in PHP functions from phpstorm-stubs
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
//! - [`loaders`]: the loader closures handed to the type engine.

mod classes;
mod functions;
mod inflight;
mod lazy_parse;
mod loaders;

pub(crate) use classes::needs_linking;
pub(crate) use inflight::ParseInflight;
pub(crate) use loaders::FileLoaders;

#[cfg(test)]
mod stub_patch_consistency_tests;
#[cfg(test)]
mod tests;
