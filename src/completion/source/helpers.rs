/// Source-text scanning helpers that are **not** part of the deprecated
/// `extract_raw_type_from_assignment_text` pipeline.
///
/// These functions perform lightweight, targeted scans of raw PHP source
/// text for patterns that the AST-based walker cannot (or need not)
/// handle:
///
/// - **`extract_new_expression_class`** — parse `new ClassName(…)` from
///   a text fragment.
/// - **`extract_function_return_from_source`** — find a function's
///   `@return` type by scanning backward for its docblock.
/// - **`extract_closure_return_type_from_assignment`** — find a
///   closure/arrow-function's native return type hint from its
///   assignment.
/// - **`extract_first_class_callable_return_type`** — resolve the
///   return type of a first-class callable assignment like
///   `$fn = strlen(...)` or `$fn = $obj->method(...)`.
/// - **`try_chained_array_access_with_candidates`** /
///   **`walk_array_segments_and_resolve`** — walk bracket segments on
///   candidate `PhpType` values to resolve array access chains.
///
/// All functions in this module are free functions (not methods on
/// `Backend`).  Cross-module dependencies that previously used `Self::`
/// are called via their canonical module paths.
use std::sync::Arc;

use crate::docblock;
use crate::php_type::PhpType;
use crate::types::{BracketSegment, ClassInfo};
use crate::util::find_semicolon_balanced;

use crate::completion::resolver::ResolutionCtx;

// ─── Source-text helpers ────────────────────────────────────────────────────

pub(in crate::completion) use crate::subject_expr::parse_new_expression_class as extract_new_expression_class;

/// Search backward in `content` for a function definition matching
/// `func_name` and extract its `@return` type from the docblock.
pub(in crate::completion) fn extract_function_return_from_source(
    func_name: &str,
    content: &str,
) -> Option<PhpType> {
    // Look for `function funcName(` in the source.
    let pattern = format!("function {}(", func_name);
    let func_pos = content.find(&pattern)?;

    // Search backward from the function definition for a docblock.
    let before = content.get(..func_pos)?;
    let trimmed = before.trim_end();
    if !trimmed.ends_with("*/") {
        return None;
    }
    let open_pos = trimmed.rfind("/**")?;
    let docblock = &trimmed[open_pos..];

    docblock::extract_return_type(docblock)
}

/// Scan backward through `content` for a closure or arrow-function
/// literal assigned to `var_name` and extract the native return type
/// hint from the source text.
///
/// Handles:
/// - `$fn = function(…): ReturnType { … }`
/// - `$fn = function(…) use (…): ReturnType { … }`
/// - `$fn = fn(…): ReturnType => …`
///
/// Returns `None` if no closure/arrow-function assignment is found
/// or if there is no return type hint.
pub(in crate::completion) fn extract_closure_return_type_from_assignment(
    var_name: &str,
    content: &str,
    cursor_offset: u32,
) -> Option<PhpType> {
    let search_area = content.get(..cursor_offset as usize)?;

    // Look for `$fn = function` or `$fn = fn` assignment.
    let assign_prefix = format!("{} = ", var_name);
    let assign_pos = search_area.rfind(&assign_prefix)?;
    let rhs_start = assign_pos + assign_prefix.len();
    let rhs = search_area.get(rhs_start..)?.trim_start();

    // Match `function(…): ReturnType` or `fn(…): ReturnType => …`
    let is_closure = rhs.starts_with("function") && rhs[8..].trim_start().starts_with('(');
    let is_arrow = rhs.starts_with("fn") && rhs[2..].trim_start().starts_with('(');

    if !is_closure && !is_arrow {
        return None;
    }

    // Find the opening `(` of the parameter list.
    let paren_open = rhs.find('(')?;
    // Find the matching `)` by tracking depth.
    let mut depth = 0i32;
    let mut paren_close = None;
    for (i, c) in rhs[paren_open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    paren_close = Some(paren_open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let paren_close = paren_close?;

    // After `)`, look for `: ReturnType`.
    let after_paren = rhs.get(paren_close + 1..)?.trim_start();
    // For closures there may be a `use (…)` clause before the return type.
    let after_use = if after_paren.starts_with("use") {
        let use_paren = after_paren.find('(')?;
        let mut udepth = 0i32;
        let mut use_close = None;
        for (i, c) in after_paren[use_paren..].char_indices() {
            match c {
                '(' => udepth += 1,
                ')' => {
                    udepth -= 1;
                    if udepth == 0 {
                        use_close = Some(use_paren + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        after_paren.get(use_close? + 1..)?.trim_start()
    } else {
        after_paren
    };

    // Expect `: ReturnType`
    let after_colon = after_use.strip_prefix(':')?.trim_start();
    if after_colon.is_empty() {
        return None;
    }

    // Extract the return type token — stop at `{`, `=>`, or whitespace.
    let end = after_colon
        .find(|c: char| c == '{' || c == '=' || c.is_whitespace())
        .unwrap_or(after_colon.len());
    let ret_type = after_colon[..end].trim();
    if ret_type.is_empty() {
        return None;
    }

    Some(PhpType::parse(ret_type))
}

/// Extract the return type annotation from a closure or arrow-function
/// literal passed as a call-site argument.
///
/// Unlike [`extract_closure_return_type_from_assignment`], this operates
/// on the raw argument text (e.g. the text between the call's parentheses
/// for one argument), not on a `$var = …` assignment context.
///
/// Handles:
/// - `fn(…): ReturnType => …`
/// - `function(…): ReturnType { … }`
/// - `function(…) use (…): ReturnType { … }`
///
/// Returns `None` if the text is not a closure/arrow-function or if
/// there is no return type hint.
/// Check whether text is a closure or arrow-function literal, optionally
/// prefixed with `static` — e.g. `fn($x) => …`, `function ($x) use (…) { … }`,
/// `static fn($x) => …`.
pub(in crate::completion) fn is_closure_like_text(text: &str) -> bool {
    let trimmed = text.trim();
    let trimmed = trimmed
        .strip_prefix("static")
        .map(str::trim_start)
        .unwrap_or(trimmed);
    let is_arrow = trimmed.starts_with("fn")
        && trimmed
            .get(2..)
            .is_some_and(|rest| rest.trim_start().starts_with('('));
    let is_closure = trimmed.starts_with("function")
        && trimmed
            .get(8..)
            .is_some_and(|rest| rest.trim_start().starts_with('('));
    is_arrow || is_closure
}

pub(in crate::completion) fn extract_closure_return_type_from_text(text: &str) -> Option<PhpType> {
    let trimmed = text.trim();

    let is_arrow = trimmed.starts_with("fn")
        && trimmed
            .get(2..2 + 1)
            .is_some_and(|c| c.starts_with('(') || c.starts_with(' ') || c.starts_with('\t'));
    let is_closure = trimmed.starts_with("function")
        && trimmed
            .get(8..)
            .is_some_and(|rest| rest.trim_start().starts_with('('));

    if !is_arrow && !is_closure {
        return None;
    }

    // Find the opening `(` of the parameter list.
    let paren_open = trimmed.find('(')?;
    // Find the matching `)` by tracking depth.
    let mut depth = 0i32;
    let mut paren_close = None;
    for (i, c) in trimmed[paren_open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    paren_close = Some(paren_open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let paren_close = paren_close?;

    // After `)`, look for `: ReturnType`.
    let after_paren = trimmed.get(paren_close + 1..)?.trim_start();

    // For closures there may be a `use (…)` clause before the return type.
    let after_use = if after_paren.starts_with("use") {
        let use_paren = after_paren.find('(')?;
        let mut udepth = 0i32;
        let mut use_close = None;
        for (i, c) in after_paren[use_paren..].char_indices() {
            match c {
                '(' => udepth += 1,
                ')' => {
                    udepth -= 1;
                    if udepth == 0 {
                        use_close = Some(use_paren + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        after_paren.get(use_close? + 1..)?.trim_start()
    } else {
        after_paren
    };

    // Expect `: ReturnType`
    let after_colon = after_use.strip_prefix(':')?.trim_start();
    if after_colon.is_empty() {
        return None;
    }

    // Extract the return type token — stop at `{`, `=>`, or whitespace.
    let end = after_colon
        .find(|c: char| c == '{' || c == '=' || c.is_whitespace())
        .unwrap_or(after_colon.len());
    let ret_type = after_colon[..end].trim();
    if ret_type.is_empty() {
        return None;
    }

    Some(PhpType::parse(ret_type))
}

/// Infer a `Generator<TKey, TValue>` return type from yield expressions
/// in a closure or function literal that has no explicit return type.
///
/// Scans the closure body for `yield` statements (at the top brace
/// depth, not inside nested closures/functions) and infers:
/// - **Value type**: from PHP casts like `(string)`, literal types, or
///   falls back to `mixed`.
/// - **Key type**: `int` for bare `yield $expr`, or inferred from
///   `yield $key => $value`.
///
/// Returns `None` if the text doesn't contain `yield`.
pub(in crate::completion) fn infer_generator_type_from_closure_yields(
    text: &str,
) -> Option<PhpType> {
    let trimmed = text.trim();

    // Must be a closure or function literal.
    let is_arrow = trimmed.starts_with("fn")
        && trimmed
            .get(2..3)
            .is_some_and(|c| c.starts_with('(') || c.starts_with(' ') || c.starts_with('\t'));
    let is_closure = trimmed.starts_with("function")
        && trimmed
            .get(8..)
            .is_some_and(|rest| rest.trim_start().starts_with('('));

    if !is_arrow && !is_closure {
        return None;
    }

    // Find the opening `{` of the body.
    let body_start = trimmed.find('{')?;
    let body = &trimmed[body_start + 1..];

    // Scan for `yield` at any depth within the closure body,
    // but skip nested function/closure definitions.
    let mut depth = 0i32;
    // When inside a nested `function` closure, holds the outer brace depth
    // at which that closure was declared.  Yields/returns are ignored until
    // `depth` returns to this level.  `None` while scanning the outer body.
    let mut nested_fn_base: Option<i32> = None;
    let mut value_type: Option<PhpType> = None;
    let mut key_type: Option<PhpType> = None;
    let mut found_yield = false;
    let mut return_type: Option<PhpType> = None;
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let c = bytes[i];
        match c {
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    break; // end of closure body
                }
                depth -= 1;
                if nested_fn_base.is_some_and(|base| depth <= base) {
                    nested_fn_base = None;
                }
            }
            b'/' if i + 1 < len && bytes[i + 1] == b'/' => {
                // Skip line comments.
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'\'' | b'"' => {
                // Skip string literals.
                let quote = c;
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1; // skip escaped char
                    }
                    i += 1;
                }
            }
            // Detect nested `function` closures so yields inside them
            // (which belong to a different generator) are ignored until
            // brace depth returns to the declaration level.  Arrow `fn`
            // closures are single-expression and cannot contain `yield`,
            // so they need no special handling.
            b'f' if nested_fn_base.is_none()
                && body[i..].starts_with("function")
                && body
                    .as_bytes()
                    .get(i + 8)
                    .is_some_and(|b| !b.is_ascii_alphanumeric() && *b != b'_') =>
            {
                // Remember the depth at which the nested closure was
                // declared; its `{`/`}` are counted normally by the
                // braces arms, and we resume scanning once we return here.
                nested_fn_base = Some(depth);
            }
            b'y' if nested_fn_base.is_none()
                && body[i..].starts_with("yield")
                && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_')
                && i + 5 < len
                && !bytes[i + 5].is_ascii_alphanumeric()
                && bytes[i + 5] != b'_' =>
            {
                found_yield = true;
                let after_yield = body[i + 5..].trim_start();

                // Skip `yield from` — that delegates to another iterator.
                if after_yield.starts_with("from")
                    && after_yield
                        .as_bytes()
                        .get(4)
                        .is_some_and(|b| !b.is_ascii_alphanumeric() && *b != b'_')
                {
                    i += 5;
                    continue;
                }

                // Check for `yield $key => $value` vs bare `yield $value`.
                if let Some(semi_pos) = find_statement_end(after_yield) {
                    let yield_expr = after_yield[..semi_pos].trim();
                    if let Some(arrow_pos) = find_fat_arrow_outside_parens(yield_expr) {
                        // yield $key => $value
                        let key_text = yield_expr[..arrow_pos].trim();
                        let val_text = yield_expr[arrow_pos + 2..].trim();
                        if key_type.is_none() {
                            key_type = infer_type_from_simple_expr(key_text);
                        }
                        if value_type.is_none() {
                            value_type = infer_type_from_simple_expr(val_text);
                        }
                    } else {
                        // bare yield $value — key is int
                        if key_type.is_none() {
                            key_type = Some(PhpType::int());
                        }
                        if value_type.is_none() {
                            value_type = infer_type_from_simple_expr(yield_expr);
                        }
                    }
                }
            }
            b'r' if nested_fn_base.is_none()
                && body[i..].starts_with("return")
                && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_')
                && i + 6 < len
                && !bytes[i + 6].is_ascii_alphanumeric()
                && bytes[i + 6] != b'_' =>
            {
                let after_return = body[i + 6..].trim_start();
                if let Some(semi_pos) = find_statement_end(after_return) {
                    let return_expr = after_return[..semi_pos].trim();
                    let inferred = if return_expr.is_empty() {
                        PhpType::void()
                    } else {
                        infer_type_from_simple_expr(return_expr).unwrap_or_else(PhpType::mixed)
                    };
                    return_type = Some(match return_type.take() {
                        Some(existing) if existing.equivalent(&inferred) => existing,
                        Some(existing) => PhpType::Union(vec![existing, inferred]),
                        None => inferred,
                    });
                }
            }
            _ => {}
        }
        i += 1;
    }

    if !found_yield {
        return None;
    }

    let key = key_type.unwrap_or_else(PhpType::int);
    let value = value_type.unwrap_or_else(PhpType::mixed);
    let ret = return_type.unwrap_or_else(PhpType::void);

    Some(PhpType::Generic(
        "Generator".to_string(),
        vec![key, value, PhpType::mixed(), ret],
    ))
}

/// Extract the body expression *text* of a closure or arrow function that
/// carries no explicit return-type annotation, so the caller can resolve
/// it through the shared type resolver.
///
/// - Arrow function `fn(...) => EXPR` → returns `EXPR`.
/// - Closure `function(...) { …; return EXPR; … }` → returns the first
///   top-level `return` expression, skipping `return`s inside nested
///   closures.
///
/// This is a best-effort fallback used only when there is no
/// `: ReturnType` annotation and no `yield` (both handled by
/// [`extract_closure_return_type_from_text`] and
/// [`infer_generator_type_from_closure_yields`]).  Returns `None` when the
/// text is not a closure/arrow function or no returnable expression is
/// found.
pub(in crate::completion) fn extract_closure_body_expr_text(text: &str) -> Option<&str> {
    let trimmed = text.trim();

    let is_arrow = trimmed.starts_with("fn")
        && trimmed
            .get(2..3)
            .is_some_and(|c| c.starts_with('(') || c.starts_with(' ') || c.starts_with('\t'));
    let is_closure = trimmed.starts_with("function")
        && trimmed
            .get(8..)
            .is_some_and(|rest| rest.trim_start().starts_with('('));

    if !is_arrow && !is_closure {
        return None;
    }

    // Find the parameter list's closing `)` by matching depth.
    let paren_open = trimmed.find('(')?;
    let mut depth = 0i32;
    let mut paren_close = None;
    for (i, c) in trimmed[paren_open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    paren_close = Some(paren_open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let paren_close = paren_close?;

    if is_arrow {
        // The body is the single expression after the `=>` arrow.  The
        // first `=>` past the parameter list is the arrow operator (a
        // `=>` belonging to the body itself sits inside brackets/parens).
        let after = &trimmed[paren_close + 1..];
        let arrow = after.find("=>")?;
        let body = after[arrow + 2..]
            .trim()
            .trim_end_matches([';', ','])
            .trim();
        if body.is_empty() { None } else { Some(body) }
    } else {
        // The first top-level `return EXPR;` inside the closure body.
        let body_start = trimmed.find('{')?;
        find_first_return_expr(&trimmed[body_start + 1..])
    }
}

/// Find the first `return EXPR` at the closure body's own brace depth,
/// skipping `return`s inside nested `function` closures.  Returns the
/// expression text without the trailing `;`.
fn find_first_return_expr(body: &str) -> Option<&str> {
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut depth = 0i32;
    // When inside a nested `function` closure, holds the brace depth at
    // which it was declared; `return`s are ignored until we return there.
    let mut nested_fn_base: Option<i32> = None;
    let mut i = 0;

    while i < len {
        let c = bytes[i];
        match c {
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    break; // end of closure body
                }
                depth -= 1;
                if nested_fn_base.is_some_and(|base| depth <= base) {
                    nested_fn_base = None;
                }
            }
            b'/' if i + 1 < len && bytes[i + 1] == b'/' => {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'\'' | b'"' => {
                let quote = c;
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'f' if nested_fn_base.is_none()
                && body[i..].starts_with("function")
                && bytes
                    .get(i + 8)
                    .is_some_and(|b| !b.is_ascii_alphanumeric() && *b != b'_')
                && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_') =>
            {
                nested_fn_base = Some(depth);
            }
            b'r' if nested_fn_base.is_none()
                && body[i..].starts_with("return")
                && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_')
                && i + 6 < len
                && !bytes[i + 6].is_ascii_alphanumeric()
                && bytes[i + 6] != b'_' =>
            {
                let after_return = body[i + 6..].trim_start();
                let semi = find_statement_end(after_return)?;
                let expr = after_return[..semi].trim();
                return if expr.is_empty() { None } else { Some(expr) };
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Find the end of a statement (`;` or `}`) at brace/paren depth 0.
fn find_statement_end(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            ';' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Find `=>` outside parentheses/brackets in a yield expression.
fn find_fat_arrow_outside_parens(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let bytes = s.as_bytes();
    for i in 0..bytes.len().saturating_sub(1) {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'=' if depth == 0 && bytes[i + 1] == b'>' => return Some(i),
            _ => {}
        }
    }
    None
}

/// Infer a type from a simple expression (cast, literal, etc.).
fn infer_type_from_simple_expr(expr: &str) -> Option<PhpType> {
    let trimmed = expr.trim();

    // PHP cast: (string), (int), (float), (bool), (array), (object)
    if trimmed.starts_with('(')
        && let Some(close) = trimmed.find(')')
    {
        let cast = trimmed[1..close].trim().to_lowercase();
        match cast.as_str() {
            "string" => return Some(PhpType::string()),
            "int" | "integer" => return Some(PhpType::int()),
            "float" | "double" | "real" => return Some(PhpType::float()),
            "bool" | "boolean" => return Some(PhpType::bool()),
            "array" => return Some(PhpType::Named("array".to_string())),
            "object" => return Some(PhpType::Named("object".to_string())),
            _ => {} // might be a parenthesized expression, not a cast
        }
    }

    // String literal
    if (trimmed.starts_with('"') || trimmed.starts_with('\'')) && trimmed.len() >= 2 {
        return Some(PhpType::string());
    }

    // Numeric literal
    if trimmed.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
        if trimmed.contains('.') {
            return Some(PhpType::float());
        }
        return Some(PhpType::int());
    }

    // true / false / null
    match trimmed.to_lowercase().as_str() {
        "true" | "false" => return Some(PhpType::bool()),
        "null" => return Some(PhpType::null()),
        _ => {}
    }

    // Can't infer from text alone — caller may use resolve_arg_text_to_type.
    None
}

/// Extract the type annotation of the Nth parameter from a closure or
/// arrow-function literal.
///
/// Given `fn(User $u, int $count): void => ...` and `position = 0`,
/// returns `Some("User")`.  Given `position = 1`, returns `Some("int")`.
///
/// This is the contravariant counterpart of
/// [`extract_closure_return_type_from_text`]: when a docblock declares
/// `@param Closure(T): void $cb`, the template param `T` appears in the
/// callable's *parameter* list rather than its return type, so we need to
/// read the closure argument's parameter type hints to infer `T`.
///
/// Returns `None` if the text is not a closure/arrow-function, the
/// parameter at `position` does not exist, or the parameter has no type
/// hint.
pub(in crate::completion) fn extract_closure_param_type_from_text(
    text: &str,
    position: usize,
) -> Option<PhpType> {
    extract_closure_params_from_text(text)?
        .into_iter()
        .nth(position)?
        .1
}

/// Extract the parameter list of a closure or arrow-function literal as
/// `(name, type)` pairs, in declaration order.
///
/// Given `fn(Decimal $carry, $op) => ...`, returns
/// `[("$carry", Some(Decimal)), ("$op", None)]`.  The name includes the
/// leading `$` so entries can be matched against variable lookups
/// directly.  Untyped parameters carry `None` as their type.
///
/// Returns `None` when the text is not a closure/arrow-function literal.
pub(in crate::completion) fn extract_closure_params_from_text(
    text: &str,
) -> Option<Vec<(String, Option<PhpType>)>> {
    let trimmed = text.trim();

    let is_arrow = trimmed.starts_with("fn")
        && trimmed
            .get(2..2 + 1)
            .is_some_and(|c| c.starts_with('(') || c.starts_with(' ') || c.starts_with('\t'));
    let is_closure = trimmed.starts_with("function")
        && trimmed
            .get(8..)
            .is_some_and(|rest| rest.trim_start().starts_with('('));

    if !is_arrow && !is_closure {
        return None;
    }

    // Find the opening `(` of the parameter list.
    let paren_open = trimmed.find('(')?;
    // Find the matching `)` by tracking depth.
    let mut depth = 0i32;
    let mut paren_close = None;
    for (i, c) in trimmed[paren_open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    paren_close = Some(paren_open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let paren_close = paren_close?;

    // Extract the parameter list text between the parens.
    let params_text = trimmed.get(paren_open + 1..paren_close)?.trim();
    if params_text.is_empty() {
        return Some(vec![]);
    }

    // Split by commas at depth 0 (respecting nested parens/generics).
    let mut result = Vec::new();
    for param in split_params_at_depth_zero(params_text) {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }

        // A typed parameter looks like `TypeHint $name` or `?TypeHint $name`
        // or `TypeHint &$name` or `TypeHint ...$name`, optionally followed
        // by `= default`.  An untyped parameter is just `$name` (with the
        // same `&`/`...` and default variations).

        // The first `$` starts the variable name (a default value may
        // contain further `$`s, which come after the name).
        let Some(dollar) = param.find('$') else {
            continue;
        };
        let name: String = param[dollar..]
            .chars()
            .take_while(|c| *c == '$' || c.is_alphanumeric() || *c == '_')
            .collect();
        if name.len() <= 1 {
            continue;
        }

        let before_dollar = param[..dollar].trim_end();
        // Strip trailing `&` or `...` (pass-by-reference or variadic).
        let before_dollar = before_dollar
            .strip_suffix("...")
            .or_else(|| before_dollar.strip_suffix('&'))
            .unwrap_or(before_dollar)
            .trim_end();

        let ty = if before_dollar.is_empty() {
            None
        } else {
            Some(PhpType::parse(before_dollar))
        };
        result.push((name, ty));
    }

    Some(result)
}

/// Split a parameter list string by commas at depth zero, respecting
/// nested parentheses and angle brackets.
fn split_params_at_depth_zero(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => depth -= 1,
            ',' if depth == 0 => {
                result.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    result.push(&text[start..]);
    result
}

/// Resolve the return type of a first-class callable assigned to
/// `var_name`.
///
/// Scans backward for `$var_name = callable_expr(...)` and resolves
/// the underlying function or method's return type.  Handles:
///
/// - `$fn = strlen(...)` (standalone function)
/// - `$fn = $this->method(...)` (instance method)
/// - `$fn = $obj->method(...)` (instance method on resolved variable)
/// - `$fn = ClassName::method(...)` (static method)
/// - `$fn = self::method(...)` / `static::method(...)`
///
/// Returns `None` if no first-class callable assignment is found or
/// the return type cannot be determined.
pub(in crate::completion) fn extract_first_class_callable_return_type(
    var_name: &str,
    rctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let content = rctx.content;
    let cursor_offset = rctx.cursor_offset;
    let search_area = content.get(..cursor_offset as usize)?;

    // Look for `$fn = ` assignment.
    let assign_prefix = format!("{} = ", var_name);
    let assign_pos = search_area.rfind(&assign_prefix)?;
    let rhs_start = assign_pos + assign_prefix.len();

    // Extract the RHS up to the next `;`
    let remaining = &content[rhs_start..];
    let semi_pos = find_semicolon_balanced(remaining)?;
    let rhs_text = remaining[..semi_pos].trim();

    // Must end with `(...)` — the first-class callable marker.
    let callable_text = rhs_text.strip_suffix("(...)")?.trim_end();
    if callable_text.is_empty() {
        return None;
    }

    // Parse the callable text into a structured expression using the
    // main SubjectExpr pipeline, then resolve through the shared call
    // return type resolver.
    let callee_expr = crate::subject_expr::SubjectExpr::parse_callee(callable_text);

    // For method calls (instance and static), use the main pipeline
    // with a return type hint capture.
    match &callee_expr {
        crate::subject_expr::SubjectExpr::MethodCall { .. }
        | crate::subject_expr::SubjectExpr::StaticMethodCall { .. }
        | crate::subject_expr::SubjectExpr::NewExpr { .. } => {
            let mut return_type: Option<PhpType> = None;
            let classes = crate::Backend::resolve_call_return_types_expr_with_hint(
                &callee_expr,
                "",
                rctx,
                Some(&mut return_type),
            );
            // Prefer the captured PhpType hint (preserves generics and
            // scalar types).  Fall back to reconstructing from the
            // returned ClassInfo set.
            return_type.or_else(|| {
                if classes.is_empty() {
                    None
                } else if classes.len() == 1 {
                    Some(PhpType::Named(classes[0].fqn().to_string()))
                } else {
                    Some(PhpType::Union(
                        classes
                            .iter()
                            .map(|c| PhpType::Named(c.fqn().to_string()))
                            .collect(),
                    ))
                }
            })
        }
        crate::subject_expr::SubjectExpr::FunctionCall(func_name) => {
            let function_loader = rctx.function_loader?;
            let func_info = function_loader(func_name)?;
            func_info.return_type.clone()
        }
        // Variable callables ($fn = $otherFn(...)) are not handled
        // here; they would need forward walker resolution of the
        // source variable first.
        _ => None,
    }
}

/// Resolve a chained array access, trying each candidate raw type
/// in order until one succeeds through the full segment walk.
///
/// Each candidate `PhpType` is fed through
/// `walk_array_segments_and_resolve`.  The first that resolves
/// through the segment walk and, if it produces a non-empty
/// `ClassInfo` set, returned immediately.  Returns `None` when no
/// candidate succeeds.
pub(in crate::completion) fn try_chained_array_access_with_candidates<'a>(
    candidates: impl Iterator<Item = PhpType> + 'a,
    segments: &[BracketSegment],
    current_class: Option<&ClassInfo>,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Vec<Arc<ClassInfo>>> {
    let current_class_name = current_class.map(|c| c.name.as_str()).unwrap_or("");

    for candidate in candidates {
        if let Some(result) = walk_array_segments_and_resolve(
            &candidate,
            segments,
            current_class_name,
            all_classes,
            class_loader,
        ) {
            return Some(result);
        }
    }

    None
}

/// Walk bracket segments on a `PhpType`, then resolve the resulting
/// type to `ClassInfo`.
///
/// Returns `Some(classes)` when the full segment chain resolves
/// successfully, or `None` when a segment cannot be applied (e.g.
/// the array shape does not contain the requested key).
fn walk_array_segments_and_resolve(
    base_type: &PhpType,
    segments: &[BracketSegment],
    current_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Vec<Arc<ClassInfo>>> {
    // Expand type aliases before walking segments.  The raw type may
    // be an alias name like `UserData` that resolves to
    // `array{name: string, pen: Pen}`.  Without expansion the
    // segment walk would fail to extract shape values.
    let mut current = if let PhpType::Named(_) = base_type {
        if let Some(expanded) = crate::completion::type_resolution::resolve_type_alias_typed(
            base_type,
            current_class_name,
            all_classes,
            class_loader,
        ) {
            expanded
        } else {
            base_type.clone()
        }
    } else {
        base_type.clone()
    };

    for seg in segments {
        // Try pure-type extraction first (array shapes, generics).
        let extracted = match seg {
            BracketSegment::StringKey(key) | BracketSegment::IntKey(key) => current
                .shape_value_type(key)
                .or_else(|| current.extract_value_type(true))
                .cloned(),
            // A dynamic (non-literal) key can address any entry, so a
            // shape yields the union of its value types (via
            // `iterable_element_type`); generic arrays yield their
            // value type as before.
            BracketSegment::ElementAccess => current
                .extract_value_type(true)
                .cloned()
                .or_else(|| current.iterable_element_type()),
        };

        current = if let Some(t) = extracted {
            t
        } else {
            // Fallback: when the current type is a plain class name (e.g.
            // `Application`, `OpeningHours`), resolve the class and check
            // its iterable generics (`@extends`, `@implements`) for the
            // element type.  This handles bracket access on classes that
            // implement `ArrayAccess` with generic type parameters.
            let class_element = crate::completion::type_resolution::type_hint_to_classes_typed(
                &current,
                current_class_name,
                all_classes,
                class_loader,
            )
            .into_iter()
            .find_map(|cls| {
                let cache = crate::virtual_members::active_resolved_class_cache();
                let merged =
                    crate::virtual_members::resolve_class_fully_maybe_cached(&cls, class_loader, cache);
                crate::completion::variable::foreach_resolution::extract_iterable_element_type_from_class(
                    &merged,
                    class_loader,
                )
            });

            class_element?
        };

        // After each segment, the resulting type might itself be an
        // alias (e.g. a shape value defined as another alias).
        // Convert to string only for alias resolution.
        if let Some(expanded) = crate::completion::type_resolution::resolve_type_alias_typed(
            &current,
            current_class_name,
            all_classes,
            class_loader,
        ) {
            current = expanded;
        }
    }

    // Check whether the type has any class-like (non-scalar) component
    // worth resolving.
    if current.is_scalar() {
        return None;
    }

    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
        &current,
        current_class_name,
        all_classes,
        class_loader,
    );
    if classes.is_empty() {
        return None;
    }
    Some(classes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_fn_with_return_type() {
        let text = "fn(int $x): Decimal => $x";
        assert_eq!(
            extract_closure_return_type_from_text(text),
            Some(PhpType::parse("Decimal"))
        );
    }

    #[test]
    fn arrow_fn_without_return_type() {
        let text = "fn(int $x) => $x * 2";
        assert_eq!(extract_closure_return_type_from_text(text), None);
    }

    #[test]
    fn closure_with_return_type() {
        let text = "function(int $x): Decimal { return new Decimal($x); }";
        assert_eq!(
            extract_closure_return_type_from_text(text),
            Some(PhpType::parse("Decimal"))
        );
    }

    #[test]
    fn closure_with_use_and_return_type() {
        let text = "function(int $x) use ($y): Decimal { return new Decimal($x + $y); }";
        assert_eq!(
            extract_closure_return_type_from_text(text),
            Some(PhpType::parse("Decimal"))
        );
    }

    #[test]
    fn closure_without_return_type() {
        let text = "function(int $x) { return $x * 2; }";
        assert_eq!(extract_closure_return_type_from_text(text), None);
    }

    #[test]
    fn not_a_closure() {
        let text = "42 + $x";
        assert_eq!(extract_closure_return_type_from_text(text), None);
    }

    #[test]
    fn arrow_fn_fqn_return_type() {
        let text = "fn(int $x): \\App\\Models\\User => findUser($x)";
        assert_eq!(
            extract_closure_return_type_from_text(text),
            Some(PhpType::parse("\\App\\Models\\User"))
        );
    }

    #[test]
    fn arrow_fn_nullable_return_type() {
        let text = "fn(int $x): ?User => findUser($x)";
        assert_eq!(
            extract_closure_return_type_from_text(text),
            Some(PhpType::parse("?User"))
        );
    }

    #[test]
    fn closure_with_nested_parens_in_params() {
        let text = "function(array $a = []): Iterator { yield from $a; }";
        assert_eq!(
            extract_closure_return_type_from_text(text),
            Some(PhpType::parse("Iterator"))
        );
    }

    #[test]
    fn variable_is_not_a_closure() {
        let text = "$someVar";
        assert_eq!(extract_closure_return_type_from_text(text), None);
    }

    #[test]
    fn whitespace_around_text() {
        let text = "  fn(int $x): Result => ok($x)  ";
        assert_eq!(
            extract_closure_return_type_from_text(text),
            Some(PhpType::parse("Result"))
        );
    }

    // ── extract_closure_param_type_from_text tests ──────────────

    #[test]
    fn param_type_arrow_fn_first_param() {
        let text = "fn(User $u, int $count): void => doStuff($u, $count)";
        assert_eq!(
            extract_closure_param_type_from_text(text, 0),
            Some(PhpType::parse("User"))
        );
    }

    #[test]
    fn param_type_arrow_fn_second_param() {
        let text = "fn(User $u, int $count): void => doStuff($u, $count)";
        assert_eq!(
            extract_closure_param_type_from_text(text, 1),
            Some(PhpType::parse("int"))
        );
    }

    #[test]
    fn param_type_closure_first_param() {
        let text = "function(User $u, int $count): void { doStuff($u, $count); }";
        assert_eq!(
            extract_closure_param_type_from_text(text, 0),
            Some(PhpType::parse("User"))
        );
    }

    #[test]
    fn param_type_untyped_param() {
        let text = "fn($item) => $item->process()";
        assert_eq!(extract_closure_param_type_from_text(text, 0), None);
    }

    #[test]
    fn param_type_out_of_bounds() {
        let text = "fn(User $u): void => doSomething($u)";
        assert_eq!(extract_closure_param_type_from_text(text, 5), None);
    }

    #[test]
    fn param_type_nullable() {
        let text = "fn(?User $u): void => doStuff($u)";
        assert_eq!(
            extract_closure_param_type_from_text(text, 0),
            Some(PhpType::parse("?User"))
        );
    }

    #[test]
    fn param_type_fqn() {
        let text = "fn(\\App\\Models\\User $u): void => doStuff($u)";
        assert_eq!(
            extract_closure_param_type_from_text(text, 0),
            Some(PhpType::parse("\\App\\Models\\User"))
        );
    }

    #[test]
    fn param_type_by_reference() {
        let text = "fn(User &$u): void => doStuff($u)";
        assert_eq!(
            extract_closure_param_type_from_text(text, 0),
            Some(PhpType::parse("User"))
        );
    }

    #[test]
    fn param_type_variadic() {
        let text = "fn(User ...$users): void => doStuff($users)";
        assert_eq!(
            extract_closure_param_type_from_text(text, 0),
            Some(PhpType::parse("User"))
        );
    }

    #[test]
    fn param_type_not_a_closure() {
        let text = "new Decimal('0')";
        assert_eq!(extract_closure_param_type_from_text(text, 0), None);
    }

    #[test]
    fn param_type_empty_params() {
        let text = "fn(): void => null";
        assert_eq!(extract_closure_param_type_from_text(text, 0), None);
    }

    #[test]
    fn param_type_closure_with_use_clause() {
        let text = "function(User $u) use ($y): void { doStuff($u, $y); }";
        assert_eq!(
            extract_closure_param_type_from_text(text, 0),
            Some(PhpType::parse("User"))
        );
    }

    #[test]
    fn param_type_whitespace_around() {
        let text = "  fn( User $u , int $count ): void => doStuff($u, $count)  ";
        assert_eq!(
            extract_closure_param_type_from_text(text, 0),
            Some(PhpType::parse("User"))
        );
    }

    #[test]
    fn param_type_variable_is_not_a_closure() {
        let text = "$someVar";
        assert_eq!(extract_closure_param_type_from_text(text, 0), None);
    }

    #[test]
    fn param_type_mixed_typed_and_untyped() {
        let text = "fn(User $u, $count, string $label): void => doStuff($u, $count, $label)";
        assert_eq!(
            extract_closure_param_type_from_text(text, 0),
            Some(PhpType::parse("User"))
        );
        assert_eq!(extract_closure_param_type_from_text(text, 1), None);
        assert_eq!(
            extract_closure_param_type_from_text(text, 2),
            Some(PhpType::parse("string"))
        );
    }

    /// Extract the `<TKey, TValue>` args from an inferred `Generator` type.
    fn generator_args(text: &str) -> Vec<PhpType> {
        match infer_generator_type_from_closure_yields(text) {
            Some(PhpType::Generic(name, args)) if name == "Generator" => args,
            other => panic!("expected Generator<...>, got {other:?}"),
        }
    }

    #[test]
    fn bare_yield_infers_int_key_and_value_type() {
        let args = generator_args("function () { yield (string) $x; }");
        assert_eq!(args[0], PhpType::int());
        assert_eq!(args[1], PhpType::string());
    }

    #[test]
    fn keyed_yield_infers_key_and_value_types() {
        let args = generator_args("function () { yield 1 => 'a'; }");
        assert_eq!(args[0], PhpType::int());
        assert_eq!(args[1], PhpType::string());
    }

    #[test]
    fn not_a_generator_without_yield() {
        assert_eq!(
            infer_generator_type_from_closure_yields("function () { return 1; }"),
            None
        );
    }

    // ─── extract_closure_body_expr_text ─────────────────────────────────

    #[test]
    fn arrow_body_expr_extracted() {
        assert_eq!(
            extract_closure_body_expr_text("fn() => new Order()"),
            Some("new Order()")
        );
    }

    #[test]
    fn arrow_body_with_params_extracted() {
        assert_eq!(
            extract_closure_body_expr_text("fn($x, $y) => foo($x, $y)"),
            Some("foo($x, $y)")
        );
    }

    #[test]
    fn arrow_body_with_nested_arrow_takes_first_arrow() {
        // The first `=>` past the parameter list is the outer arrow.
        assert_eq!(
            extract_closure_body_expr_text("fn() => fn() => 5"),
            Some("fn() => 5")
        );
    }

    #[test]
    fn closure_return_expr_extracted() {
        assert_eq!(
            extract_closure_body_expr_text("function () { return new Order(); }"),
            Some("new Order()")
        );
    }

    #[test]
    fn closure_return_skips_nested_closure_return() {
        assert_eq!(
            extract_closure_body_expr_text(
                "function () { $f = function () { return 1; }; return new Order(); }"
            ),
            Some("new Order()")
        );
    }

    #[test]
    fn body_expr_none_for_non_closure() {
        assert_eq!(extract_closure_body_expr_text("$foo->bar()"), None);
        assert_eq!(extract_closure_body_expr_text("new Order()"), None);
    }

    /// A nested closure with no inner braces used to make the scan hit its
    /// `}` at depth 0 and stop early, missing the real yield that follows.
    #[test]
    fn nested_closure_without_inner_braces_does_not_end_scan_early() {
        let args =
            generator_args("function () { $f = function () { yield 1; }; yield (string) $x; }");
        assert_eq!(args[0], PhpType::int());
        assert_eq!(args[1], PhpType::string());
    }

    /// A nested closure containing inner braces used to leak its yields into
    /// the outer generator; the outer yield must win regardless of order.
    #[test]
    fn nested_closure_with_inner_braces_does_not_leak_yields() {
        let args = generator_args(
            "function () { \
                $f = function () { foreach ([] as $y) {} yield 1.5; }; \
                yield (string) $x; \
            }",
        );
        assert_eq!(args[0], PhpType::int());
        assert_eq!(args[1], PhpType::string());
    }
}
