//! Finding the function or method whose docblock holds the cursor, and
//! reading its signature and its existing tags.

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::code_actions::cursor_context::{CursorContext, MemberContext};
use crate::code_actions::indent_of_line_at;
use crate::docblock::TagKind;
use crate::docblock::parser::{DocblockInfo, parse_docblock_for_tags};
use crate::docblock::type_strings::split_type_token;
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;
use crate::text_position::offset_to_position;

use super::{DocParam, DocReturn, FunctionWithDocblock, SigParam};

/// Use the shared `CursorContext` to find the function/method at the cursor
/// position, then check for an existing docblock.
pub(super) fn find_function_with_docblock_from_context<'a>(
    ctx: &CursorContext<'a>,
    statements: &'a Sequence<'a, Statement<'a>>,
    trivia: &[Trivia<'a>],
    content: &str,
    cursor: u32,
) -> Option<FunctionWithDocblock> {
    let method_at = |method: &Method<'a>| {
        function_docblock_at(
            cursor,
            method,
            &method.parameter_list,
            method.return_type_hint.as_ref(),
            trivia,
            content,
        )
    };
    match ctx {
        CursorContext::InClassLike {
            member,
            all_members,
            ..
        } => {
            if let MemberContext::Method(method, _in_body) = member
                && let Some(info) = method_at(method)
            {
                return Some(info);
            }
            // The cursor may be inside the docblock trivia that precedes
            // a method.  Docblocks live outside the method's AST span, so
            // `find_cursor_context` reports `MemberContext::None`.  Scan
            // all members to find a method whose preceding docblock
            // contains the cursor.
            if matches!(member, MemberContext::None) {
                return all_members.iter().find_map(|m| match m {
                    ClassLikeMember::Method(method) => method_at(method),
                    _ => None,
                });
            }
            None
        }
        CursorContext::InFunction(func, _in_body) => {
            standalone_function_at(cursor, func, trivia, content)
        }
        CursorContext::None => {
            // The cursor may be inside a docblock that precedes a
            // top-level (or namespace-level) standalone function.
            // Docblocks are trivia, so `find_cursor_context` returns
            // `None` when the cursor sits before the function's AST
            // span.  Scan all top-level statements to find a match.
            find_standalone_function_by_docblock(statements, trivia, content, cursor)
        }
    }
}

/// Scan top-level (and namespace-level) statements for a standalone
/// function whose preceding docblock contains the cursor.
fn find_standalone_function_by_docblock<'a>(
    statements: &'a Sequence<'a, Statement<'a>>,
    trivia: &[Trivia<'a>],
    content: &str,
    cursor: u32,
) -> Option<FunctionWithDocblock> {
    statements.iter().find_map(|stmt| match stmt {
        Statement::Function(func) => standalone_function_at(cursor, func, trivia, content),
        Statement::Namespace(ns) => ns.statements().iter().find_map(|s| match s {
            Statement::Function(func) => standalone_function_at(cursor, func, trivia, content),
            _ => None,
        }),
        _ => None,
    })
}

/// [`function_docblock_at`] for a standalone function.
fn standalone_function_at<'a>(
    cursor: u32,
    func: &Function<'a>,
    trivia: &[Trivia<'a>],
    content: &str,
) -> Option<FunctionWithDocblock> {
    function_docblock_at(
        cursor,
        func,
        &func.parameter_list,
        func.return_type_hint.as_ref(),
        trivia,
        content,
    )
}

/// The function-like `node` with its docblock, when the cursor is inside
/// the docblock that immediately precedes it.
fn function_docblock_at<'a>(
    cursor: u32,
    node: &impl HasSpan,
    param_list: &function_like::parameter::FunctionLikeParameterList<'a>,
    return_type_hint: Option<&function_like::r#return::FunctionLikeReturnTypeHint<'a>>,
    trivia: &[Trivia<'a>],
    content: &str,
) -> Option<FunctionWithDocblock> {
    // Uses the canonical trivia-based locator from symbol_map::docblock.
    let (docblock_text, docblock_start) =
        crate::symbol_map::docblock::get_docblock_text_with_offset(trivia, content, node)?;
    if cursor < docblock_start || cursor >= node.span().start.offset {
        return None;
    }
    Some(build_info_for_function_like(
        docblock_text,
        docblock_start as usize,
        node.span().start.offset,
        param_list,
        return_type_hint,
        content,
    ))
}

/// Build a `FunctionWithDocblock` from a function-like AST node and the
/// docblock above it.
fn build_info_for_function_like<'a>(
    docblock_text: &str,
    docblock_start: usize,
    decl_offset: u32,
    param_list: &function_like::parameter::FunctionLikeParameterList<'a>,
    return_type_hint: Option<&function_like::r#return::FunctionLikeReturnTypeHint<'a>>,
    content: &str,
) -> FunctionWithDocblock {
    let docblock_end = docblock_start + docblock_text.len();
    let docblock_text = docblock_text.to_string();

    let sig_params: Vec<SigParam> = param_list
        .parameters
        .iter()
        .map(|p| {
            let name = bytes_to_str(p.variable.name).to_string();
            let type_hint = p.hint.as_ref().map(|h| extract_hint_type(h));
            let is_variadic = p.ellipsis.is_some();
            SigParam {
                name,
                type_hint,
                is_variadic,
            }
        })
        .collect();

    let sig_return = return_type_hint.map(|rth| extract_hint_type(&rth.hint));

    // Parse existing docblock tags with a single parse pass.
    let docblock_info = parse_docblock_for_tags(&docblock_text);
    let doc_params = docblock_info
        .as_ref()
        .map(parse_doc_params_from_info)
        .unwrap_or_default();
    let doc_return = docblock_info.as_ref().and_then(parse_doc_return_from_info);
    let doc_throws = docblock_info
        .as_ref()
        .map(parse_doc_throws_from_info)
        .unwrap_or_default();

    let indent = indent_of_line_at(content, docblock_start).to_string();

    // Compute LSP position for throws analysis.
    let docblock_position = offset_to_position(content, docblock_start);

    FunctionWithDocblock {
        docblock_start,
        docblock_end,
        docblock_text,
        sig_params,
        sig_return,
        doc_params,
        doc_return,
        doc_throws,
        indent,
        docblock_position,
        decl_offset,
    }
}

// ── Docblock parsing ────────────────────────────────────────────────────────

/// Parse all `@param` tags from a pre-parsed [`DocblockInfo`].
pub(super) fn parse_doc_params_from_info(info: &DocblockInfo) -> Vec<DocParam> {
    let mut results = Vec::new();

    for tag in info.tags_by_kind(TagKind::Param) {
        let rest = tag.description.trim();
        if rest.is_empty() {
            continue;
        }

        // When the first token starts with `$` (or `...$` for variadic),
        // there is no type — the token is the parameter name directly.
        let first_token = rest.split_whitespace().next().unwrap_or("");
        let is_name_first = first_token.starts_with('$') || first_token.starts_with("...$");

        let (type_str, name_token, after_params) = if is_name_first {
            ("", first_token, &rest[first_token.len()..])
        } else {
            let (type_str, remainder) = split_type_token(rest);
            let remainder = remainder.trim_start();

            let name_token = remainder.split_whitespace().next().unwrap_or("");
            let after_params = remainder.get(name_token.len()..).unwrap_or("");
            (type_str, name_token, after_params)
        };

        if name_token.is_empty() || (!name_token.contains('$')) {
            continue;
        }

        let name = name_token.to_string();

        // Continuation lines are joined with \n; collapse to spaces
        // for the description to match the old behaviour.
        let description = after_params
            .trim()
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ");

        results.push(DocParam {
            type_parsed: PhpType::parse(type_str),
            type_str_raw: type_str.to_string(),
            name,
            description,
        });
    }

    results
}

/// Parse the `@return` tag from a pre-parsed [`DocblockInfo`].
fn parse_doc_return_from_info(info: &DocblockInfo) -> Option<DocReturn> {
    for tag in info.tags_by_kind(TagKind::Return) {
        let rest = tag.description.trim();
        if rest.is_empty() {
            continue;
        }

        // Skip conditional return types.
        if rest.starts_with('(') {
            continue;
        }

        let (type_str, remainder) = split_type_token(rest);
        let description = remainder.trim().to_string();

        return Some(DocReturn {
            type_parsed: PhpType::parse(type_str),
            description,
        });
    }

    None
}

/// Parse `@throws` tags from a pre-parsed [`DocblockInfo`], returning
/// the exception type names.
fn parse_doc_throws_from_info(info: &DocblockInfo) -> Vec<String> {
    let mut results = Vec::new();
    for tag in info.tags_by_kind(TagKind::Throws) {
        let rest = tag.description.trim();
        if let Some(type_name) = rest.split_whitespace().next()
            && !type_name.is_empty()
        {
            results.push(type_name.to_string());
        }
    }
    results
}
