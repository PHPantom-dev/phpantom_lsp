//! Class and function names, by the rule for where each was read.
//!
//! A name written in a file (in code or in a docblock) follows PHP's
//! rule: the block's imports, else the current namespace, never a global
//! class.  A name the engine already resolved is the exact FQN it spells.
//! See "Source Names and Resolved Names" in `docs/ARCHITECTURE.md`.

mod docblock_type_names;
mod resolved;
mod written;

pub(crate) use docblock_type_names::{DocblockTypeNames, scan_declares_docblock_type_name};
pub use resolved::OwnedResolvedNames;
pub(crate) use written::{
    fill_generic_type_defaults, function_lookup_names, lookup_import, resolve_code_class_name,
    resolve_code_php_type_names, resolve_name_via_loader, resolve_php_type_names,
    resolve_source_php_type_names, resolve_to_fqn, shorten_class_fqn, shorten_from_fqn,
    shorten_type_for_file, written_name_region,
};
