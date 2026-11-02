//! Quick-fix for a PHPUnit hook that skips the parent's: insert the
//! `parent::setUp()` (or `tearDown()`, …) call the `missing_parent_call`
//! diagnostic reports missing.
//!
//! A set-up hook calls the parent first, so the base class's fixtures
//! exist before the test's own; a tear-down hook calls it last.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::diagnostics::helpers::FileDiagnosticContext;
use crate::diagnostics::parent_calls::MissingParentCall;
use crate::text_position::{find_line_start, offset_to_position, ranges_overlap};

impl Backend {
    pub(crate) fn collect_parent_call_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        let Ok(doc_uri) = uri.parse::<Uri>() else {
            return;
        };
        let start = crate::text_position::position_to_offset(content, params.range.start);
        let end = crate::text_position::position_to_offset(content, params.range.end);
        let ctx = FileDiagnosticContext::gather(self, uri);
        // Only the hooks whose name the request is near are judged; the
        // exact overlap is checked on the LSP range below.
        let findings = self.missing_parent_calls(&ctx, content, |offset| {
            offset <= end && start <= offset + "tearDownAfterClass".len() as u32
        });

        for finding in &findings {
            let Some(diagnostic) = self.parent_call_diagnostic(uri, content, finding) else {
                continue;
            };
            let range = diagnostic.range;
            // A bare cursor touching either end of the name is on it.
            let touches = if params.range.start == params.range.end {
                range.start <= params.range.start && params.range.start <= range.end
            } else {
                ranges_overlap(&range, &params.range)
            };
            if !touches {
                continue;
            }
            out.push(CodeActionResponse::CodeAction(CodeAction {
                title: format!("Call parent::{}()", finding.name),
                kind: Some(CodeActionKind::QuickFix),
                diagnostics: Some(vec![diagnostic]),
                edit: Some(super::single_file_edit(
                    doc_uri.clone(),
                    vec![parent_call_edit(content, finding)],
                )),
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: None,
                tags: None,
            }));
        }
    }
}

/// The leading whitespace of the line holding `offset`.
fn line_indent(content: &str, offset: u32) -> &str {
    let line_start = find_line_start(content, (offset as usize).min(content.len()));
    let line = &content[line_start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// The edit that adds the parent call to the hook `finding` reports.
fn parent_call_edit(content: &str, finding: &MissingParentCall) -> TextEdit {
    let method_indent = line_indent(content, finding.name_offset);
    let call = format!("parent::{}();", finding.name);
    let after_open = finding.body_open as usize + 1;

    let (from, to, new_text) = match finding.statements {
        Some((first, last)) => {
            // Match the body's own indentation when its first statement
            // starts a line.
            let first_indent = line_indent(content, first);
            let inner = if find_line_start(content, first as usize) + first_indent.len()
                == first as usize
            {
                first_indent.to_owned()
            } else {
                format!("{method_indent}{}", super::indent_unit(content))
            };
            let at = if finding.calls_first() {
                after_open
            } else {
                last as usize
            };
            (at, at, format!("\n{inner}{call}"))
        }
        None => {
            let inner = format!("{method_indent}{}", super::indent_unit(content));
            let close = finding.body_close as usize;
            // Keep a comment that is all the body holds.
            if content[after_open..close].trim().is_empty() {
                (
                    after_open,
                    close,
                    format!("\n{inner}{call}\n{method_indent}"),
                )
            } else {
                (after_open, after_open, format!("\n{inner}{call}"))
            }
        }
    };
    TextEdit {
        range: Range {
            start: offset_to_position(content, from),
            end: offset_to_position(content, to),
        },
        new_text,
    }
}
