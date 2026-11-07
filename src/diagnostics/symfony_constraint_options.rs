//! Options a constraint in a Symfony validation mapping does not take.
//!
//! The option keys a YAML or XML validation mapping gives a constraint are
//! checked against the options its class takes (see
//! [`Backend::constraint_options`]).  The Validator throws on an option the
//! constraint does not take, so an unknown key is an error.  A constraint
//! whose class or ancestors do not load is not judged.

use std::collections::HashMap;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::symfony::validation::{ConstraintOptions, constraint_option_keys};

use super::helpers::make_diagnostic;

/// Diagnostic code used for an option a constraint does not take.
pub(crate) const UNKNOWN_CONSTRAINT_OPTION_CODE: &str = "unknown_constraint_option";

impl Backend {
    /// Report each option key of a constraint in the validation mapping at
    /// `uri` that names no option the constraint takes.
    pub(crate) fn collect_constraint_option_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let mut options_of: HashMap<String, Option<ConstraintOptions>> = HashMap::new();
        for constraint in constraint_option_keys(uri, content) {
            if constraint.keys.is_empty() || !constraint.is_judged() {
                continue;
            }
            let Some(options) = options_of
                .entry(constraint.constraint.clone())
                .or_insert_with(|| self.constraint_options(&constraint.constraint))
            else {
                continue;
            };
            for &(start, end) in &constraint.keys {
                let name = &content[start..end];
                let is_plain = !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
                if !is_plain || options.accepts(name) {
                    continue;
                }
                let Some(range) = self.offset_range_to_lsp_range(uri, content, start, end) else {
                    continue;
                };
                let class = constraint.constraint.trim_start_matches('\\');
                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::Error,
                    UNKNOWN_CONSTRAINT_OPTION_CODE,
                    format!("Unknown option '{name}' for constraint {class}"),
                ));
            }
        }
    }
}
