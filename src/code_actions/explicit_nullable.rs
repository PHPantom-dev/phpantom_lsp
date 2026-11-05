//! "Make parameter explicitly nullable" code action.
//!
//! PHP 8.4 deprecates the implicit-nullable form `function f(Foo $x =
//! null)`, where the `null` default silently widens the declared type.
//! This quickfix writes the `null` into the type itself: `?Foo` for a
//! single type, `int|string|null` for a union.
//!
//! It is offered on any such parameter under the cursor, and for each
//! PHPStan `parameter.implicitlyNullable` diagnostic the request overlaps,
//! which it attaches.  An intersection type is left alone: PHP rejects a
//! `null` default on one, so there is nothing implicit to make explicit,
//! and PHPStan does not report it either.
//!
//! **Code action kind:** `quickfix`.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::code_actions::cursor_context::innermost_at;
use crate::code_actions::phpstan::phpstan_identifier;
use crate::parser::param_default_is_null;
use crate::types::PhpVersion;

/// PHPStan's identifier for the deprecation this action fixes.
const IMPLICITLY_NULLABLE_ID: &str = "parameter.implicitlyNullable";

/// The edit that makes a parameter's `null` explicit.
#[derive(Debug, PartialEq, Eq)]
struct NullableFix {
    /// Byte offset the text goes in at.
    offset: usize,
    /// `"?"` before a single type, `"|null"` after a union.
    text: &'static str,
}

impl Backend {
    /// Collect "Make parameter explicitly nullable" code actions.
    pub(crate) fn collect_explicit_nullable_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        let Ok(doc_uri) = uri.parse::<Uri>() else {
            return;
        };

        let phpstan_diags: Vec<Diagnostic> = self
            .phpstan_diags_overlapping(uri, &params.range)
            .into_iter()
            .filter(|diag| phpstan_identifier(diag) == Some(IMPLICITLY_NULLABLE_ID))
            .collect();

        let cursor =
            crate::text_position::position_to_byte_offset(content, params.range.start) as u32;
        let php_version = self.php_version();

        let found =
            crate::parser::with_parsed_program(content, "explicit_nullable", |program, content| {
                let mut found: Vec<(String, NullableFix, Vec<Diagnostic>)> = Vec::new();
                let mut push = |name: &str, fix: NullableFix, diag: Option<&Diagnostic>| {
                    if let Some(existing) = found.iter_mut().find(|(_, f, _)| *f == fix) {
                        existing.2.extend(diag.cloned());
                    } else {
                        found.push((name.to_string(), fix, diag.into_iter().cloned().collect()));
                    }
                };

                if let Some((name, fix)) = innermost_at(program, cursor, |node| match node {
                    Node::FunctionLikeParameter(param) => Some(param),
                    _ => None,
                })
                .and_then(|param| {
                    Some((
                        bytes_to_str(param.variable.name),
                        explicit_nullable_fix(param, php_version)?,
                    ))
                }) {
                    push(name, fix, None);
                }

                for diag in &phpstan_diags {
                    let Some(param) = reported_parameter(program, content, diag) else {
                        continue;
                    };
                    if let Some(fix) = explicit_nullable_fix(param, php_version) {
                        push(bytes_to_str(param.variable.name), fix, Some(diag));
                    }
                }
                found
            });

        for (name, fix, diagnostics) in found {
            let position = crate::text_position::offset_to_position(content, fix.offset);
            out.push(CodeActionResponse::CodeAction(CodeAction {
                title: format!("Make {name} explicitly nullable"),
                kind: Some(CodeActionKind::QuickFix),
                diagnostics: (!diagnostics.is_empty()).then_some(diagnostics),
                edit: Some(crate::code_actions::single_edit(
                    doc_uri.clone(),
                    Range::new(position, position),
                    fix.text.to_string(),
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

/// The edit that makes `param`'s `null` default explicit in its type, or
/// `None` when the type already admits `null` or the parameter has no
/// type or no `null` default.
fn explicit_nullable_fix(
    param: &FunctionLikeParameter<'_>,
    php_version: PhpVersion,
) -> Option<NullableFix> {
    let hint = param.hint.as_ref()?;
    if !param_default_is_null(param) {
        return None;
    }
    match hint {
        // `null` is already part of these, and `?mixed` is a compile error.
        Hint::Nullable(_) | Hint::Null(_) | Hint::Mixed(_) => None,
        Hint::Union(_) => (!union_has_null(hint)).then(|| NullableFix {
            offset: hint.span().end.offset as usize,
            text: "|null",
        }),
        // PHP refuses a `null` default on an intersection outright, and
        // `void`, `never`, and `static` are not parameter types.
        Hint::Intersection(_)
        | Hint::Parenthesized(_)
        | Hint::Void(_)
        | Hint::Never(_)
        | Hint::Static(_) => None,
        // `?T` arrived in PHP 7.1.
        _ => (php_version >= PhpVersion::new(7, 1)).then(|| NullableFix {
            offset: hint.span().start.offset as usize,
            text: "?",
        }),
    }
}

/// Whether a union type names `null` among its members.
fn union_has_null(hint: &Hint<'_>) -> bool {
    match hint {
        Hint::Union(union) => union_has_null(union.left) || union_has_null(union.right),
        Hint::Null(_) => true,
        _ => false,
    }
}

/// The parameter a `parameter.implicitlyNullable` diagnostic reports:
/// the one its message names, on the diagnostic's line.
fn reported_parameter<'ast, 'arena>(
    program: &'ast Program<'arena>,
    content: &str,
    diag: &Diagnostic,
) -> Option<&'ast FunctionLikeParameter<'arena>> {
    let name = reported_parameter_name(crate::util::message_text(&diag.message))?;
    let line_start = crate::text_position::position_to_byte_offset(
        content,
        Position::new(diag.range.start.line, 0),
    );
    let line_end = crate::text_position::find_line_end(content, line_start);
    let line = content.get(line_start..line_end)?;

    let mut search = 0;
    while let Some(found) = line[search..].find(name) {
        let at = search + found;
        search = at + name.len();
        let boundary = line[search..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if !boundary {
            continue;
        }
        let param = innermost_at(program, (line_start + at) as u32, |node| match node {
            Node::FunctionLikeParameter(param) => Some(param),
            _ => None,
        });
        if let Some(param) = param
            && bytes_to_str(param.variable.name) == name
        {
            return Some(param);
        }
    }
    None
}

/// `$p` in PHPStan's "Deprecated in PHP 8.4: Parameter #1 $p (string) is
/// implicitly nullable via default value null."
fn reported_parameter_name(message: &str) -> Option<&str> {
    let rest = &message[message.find("Parameter #")?..];
    let rest = &rest[rest.find('$')?..];
    let end = rest[1..]
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(rest.len(), |end| end + 1);
    (end > 1).then(|| &rest[..end])
}

/// Whether a cached `parameter.implicitlyNullable` diagnostic no longer
/// applies: the parameter it names is gone from its line, or its type now
/// admits `null` (or its default is no longer `null`).
pub(crate) fn is_explicit_nullable_stale(content: &str, diag: &Diagnostic) -> bool {
    crate::parser::with_parsed_program(content, "explicit_nullable_stale", |program, content| {
        reported_parameter(program, content, diag)
            .is_none_or(|param| explicit_nullable_fix(param, PhpVersion::new(8, 4)).is_none())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_parameter_name_from_the_phpstan_message() {
        assert_eq!(
            reported_parameter_name(
                "Deprecated in PHP 8.4: Parameter #2 $user_id (int) is implicitly nullable via default value null."
            ),
            Some("$user_id")
        );
        assert_eq!(reported_parameter_name("Something else."), None);
    }
}
