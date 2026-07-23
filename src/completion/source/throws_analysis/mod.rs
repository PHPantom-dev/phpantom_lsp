//! Throws analysis: scanning, catch-block filtering, and uncaught detection.
//!
//! This module provides a complete throws-analysis pipeline used by both
//! `phpdoc.rs` (for `@throws` tag completion) and `catch_completion.rs`
//! (for catch-clause exception suggestions).
//!
//! - [`scanning`] — low-level scanning primitives shared by every caller:
//!   `throw` statement/expression scanning, `@throws` propagation, and
//!   method return-type / `@throws` lookup.
//! - [`catch`] — `try/catch` block scanning and `throw $variable`
//!   resolution through the enclosing catch clause.
//! - [`cross_file`] — high-level uncaught-throws analysis, including
//!   cross-file `@throws` propagation via [`ThrowsContext`].
//!
//! Callers that only need type names can map `ThrowInfo::type_name`;
//! callers that need offset information (e.g. for catch-block filtering)
//! use the full `ThrowInfo` struct.

mod catch;
mod cross_file;
mod scanning;

pub(crate) use cross_file::*;
pub(crate) use scanning::*;

// ─── Import Helpers ─────────────────────────────────────────────────────────

/// Check whether a `use` statement for the given FQN already exists in
/// the file content.
pub(in crate::completion) fn has_use_import(content: &str, fqn: &str) -> bool {
    let target = format!("use {};", fqn);
    let target_with_alias = format!("use {} as", fqn); // alias import
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == target || trimmed.starts_with(&target_with_alias) {
            return true;
        }
        // Handle group imports: `use Foo\{Bar, Baz};`
        // Check if the FQN's namespace prefix is used in a group import
        // that includes the short name.
        if let Some(ns_sep) = fqn.rfind('\\') {
            let ns_prefix = &fqn[..ns_sep];
            let short = &fqn[ns_sep + 1..];
            let group_prefix = format!("use {}\\{{", ns_prefix);
            if trimmed.starts_with(&group_prefix) {
                // Check if short name is in the brace list
                if let Some(brace_start) = trimmed.find('{')
                    && let Some(brace_end) = trimmed.find('}')
                {
                    let names = &trimmed[brace_start + 1..brace_end];
                    if names.split(',').any(|n| n.trim() == short) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
#[path = "../throws_analysis_tests.rs"]
mod tests;
