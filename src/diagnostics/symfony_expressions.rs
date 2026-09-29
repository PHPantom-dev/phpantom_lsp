//! Diagnostics for configured Symfony ExpressionLanguage strings.

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};

use super::helpers::make_diagnostic;
use super::unknown_members::UNKNOWN_MEMBER_CODE;
use crate::Backend;

impl Backend {
    pub(super) fn collect_symfony_expression_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        for problem in self.symfony_expression_problems(uri, content) {
            let kind = if problem.is_method {
                "Method"
            } else {
                "Property"
            };
            let message = if problem.classes.len() == 1 {
                format!(
                    "{} '{}' not found on class '{}'",
                    kind, problem.member, problem.classes[0]
                )
            } else {
                format!(
                    "{} '{}' not found on any of the {} possible types ({})",
                    kind,
                    problem.member,
                    problem.classes.len(),
                    problem.classes.join(", ")
                )
            };
            out.push(make_diagnostic(
                Range::new(
                    crate::text_position::offset_to_position(content, problem.start),
                    crate::text_position::offset_to_position(content, problem.end),
                ),
                DiagnosticSeverity::WARNING,
                UNKNOWN_MEMBER_CODE,
                message,
            ));
        }
    }
}
