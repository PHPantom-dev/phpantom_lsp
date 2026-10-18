//! Throws analysis: the exceptions a function lets escape, and the ones a
//! `try` block throws.
//!
//! - [`uncaught`] — the exceptions a function or method lets escape, read
//!   from its AST with receivers typed by the forward walker.  What `@throws`
//!   completion, docblock generation and "Update docblock" report.
//! - [`scanning`] — text scanners for catch-clause completion, which runs
//!   on a `try` body that may not parse yet: `throw` statements and
//!   expressions, `@throws` propagated from same-file calls, and inline
//!   `@throws` annotations.

mod scanning;
mod uncaught;

pub(crate) use scanning::*;
pub(crate) use uncaught::*;

// ─── Import Helpers ─────────────────────────────────────────────────────────

/// Check whether a `use` statement for the given FQN already exists in
/// the file content.
pub(in crate::completion) fn has_use_import(content: &str, fqn: &str) -> bool {
    let target = format!("use {};", fqn);
    let target_with_alias = format!("use {} as", fqn);
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
