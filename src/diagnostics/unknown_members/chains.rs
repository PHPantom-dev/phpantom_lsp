//! Chain error propagation: once a member access is flagged as broken,
//! the later links of the same fluent chain are not reported, because
//! their failure is a direct consequence of the first break.

use super::super::subject_cache::ScopeKey;
use super::access::MemberAccessSite;

/// The chains broken so far in one pass.  Each prefix is keyed by the
/// scope it was flagged in: the same text in another function, or at the
/// top level of another `namespace` block, can be a different value, and
/// that access has to be judged on its own.
#[derive(Default)]
pub(super) struct BrokenChains {
    prefixes: Vec<(ScopeKey, String)>,
}

impl BrokenChains {
    /// Record `access` as the place a chain broke.
    pub(super) fn record(&mut self, scope: ScopeKey, access: &MemberAccessSite) {
        self.prefixes.push((
            scope,
            broken_chain_prefix(
                access.subject_text,
                access.member_name,
                access.is_static,
                access.is_method_call,
            ),
        ));
    }

    /// Whether `subject_text` is downstream of a broken chain expression
    /// previously flagged in the same `scope`.  The original broken prefix
    /// propagates to all further links naturally (it is a prefix of every
    /// subsequent subject).
    ///
    /// Normalises null-safe operators (`?->` → `->`) so that chains mixing
    /// `->` and `?->` are handled correctly.
    pub(super) fn is_downstream(&self, subject_text: &str, scope: ScopeKey) -> bool {
        if self.prefixes.is_empty() {
            return false;
        }
        let normalized = subject_text.replace("?->", "->");
        self.prefixes.iter().any(|(prefix_scope, prefix)| {
            if *prefix_scope != scope {
                return false;
            }
            if prefix.ends_with('(') {
                // Method-call prefix: `starts_with` is sufficient because
                // the trailing `(` prevents name-prefix ambiguity.
                normalized.starts_with(prefix.as_str())
            } else {
                // Property prefix: the subject must equal the prefix or
                // the prefix must be followed by a chain operator to avoid
                // matching longer property names (e.g. `value` matching
                // `value_extra`).
                if normalized == *prefix {
                    return true;
                }
                if !normalized.starts_with(prefix.as_str()) {
                    return false;
                }
                let rest = &normalized[prefix.len()..];
                rest.starts_with("->") || rest.starts_with("::") || rest.starts_with('[')
            }
        })
    }
}

/// Build the "broken chain prefix" for a flagged member access.
///
/// The prefix is constructed so that downstream `subject_text` values
/// (produced by `expr_to_subject_text`) will start with this prefix.
///
/// For method calls the prefix ends with `(` — this prevents ambiguity
/// with similarly-named methods (e.g. `callHome(` vs `callHomeLate(`).
/// For property accesses the prefix is the bare expression;
/// [`BrokenChains::is_downstream`] checks for a chain-operator boundary
/// after the prefix to avoid false matches with longer property names
/// (e.g. `value` vs `value_extra`).
fn broken_chain_prefix(
    subject_text: &str,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
) -> String {
    let normalized = subject_text.replace("?->", "->");
    let operator = if is_static { "::" } else { "->" };
    if is_method_call {
        // Trailing `(` ensures "callHome(" does not match "callHomeLate(".
        format!("{}{}{}{}", normalized, operator, member_name, "(")
    } else {
        format!("{}{}{}", normalized, operator, member_name)
    }
}
