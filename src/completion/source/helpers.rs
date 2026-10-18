//! Source-text scanning helpers for completion and type resolution.
//!
//! These functions perform lightweight, targeted scans of raw PHP source
//! text for patterns that the AST-based walker cannot (or need not)
//! handle:
//!
//! - **`extract_new_expression_class`** — parse `new ClassName(…)` from
//!   a text fragment.
//! - **`extract_closure_return_type_from_text`** — find a
//!   closure/arrow-function's native return type hint from its own
//!   source text.
//! - **`resolve_first_class_callable_return_type`** — resolve the
//!   return type of a first-class callable expression like
//!   `strlen(...)` or `$obj->method(...)`.
//! - **`try_chained_array_access_with_candidates`** /
//!   **`walk_array_segments`** — walk bracket segments on candidate
//!   `PhpType` values to resolve array access chains.
//!
//! All functions in this module are free functions, not methods on
//! `Backend`.

use crate::atom::atom;
use std::sync::Arc;

use crate::completion::source::code_context::CodeContext;
use crate::php_type::{PhpType, TypeKind};
use crate::text_scan::{
    CodeOffsets, is_escaped, keyword_starts_at, split_top_level_arrow, split_top_level_commas,
};
use crate::types::{BracketSegment, ClassInfo};

use crate::type_engine::resolver::ResolutionCtx;

// ─── Source-text helpers ────────────────────────────────────────────────────

pub(crate) use crate::type_engine::subject_expr::parse_new_expression_class as extract_new_expression_class;

/// A call whose argument array holds the key the cursor is typing.
///
/// Recovered from `foo('name', ['key|' => …])`-shaped source text by
/// [`enclosing_array_key_call`].
pub(crate) struct ArrayKeyCall<'a> {
    /// The call's first argument, which must be a string literal — the name
    /// the keys belong to (an Artisan command, a route, …).
    pub name: String,
    /// The called function or member name, lowercased.
    pub callee: String,
    /// The source text before the callee, trimmed of trailing whitespace.
    /// Ends with `::` for a static call and `->` / `?->` for an instance
    /// call; callers inspect it to identify the receiver.
    pub before_callee: &'a str,
}

/// Resolve the enclosing `foo('name', [ '|' => … ])` call from the lexical
/// context of the array key being typed.
///
/// The key's array must be the innermost bracket left open at the cursor and
/// must itself sit directly inside a call's argument list; the callee and the
/// call's first argument are then read from the text before that `(`.  The
/// array need not follow the name directly, so the parameters of
/// `temporarySignedRoute('users.show', $expiration, ['|' => 1])` are found as
/// well.  Returns `None` when the surrounding text is not that shape.
pub(crate) fn enclosing_array_key_call<'a>(
    content: &'a str,
    code: &CodeContext<'_>,
) -> Option<ArrayKeyCall<'a>> {
    let (_, paren_open) = code.nested_pair(b'[', b'(')?;

    let name = first_string_argument(content, paren_open)?;
    let (callee, before_callee) = split_trailing_ident(content[..paren_open].trim_end());
    if callee.is_empty() {
        return None;
    }

    Some(ArrayKeyCall {
        name,
        callee: callee.to_ascii_lowercase(),
        before_callee: before_callee.trim_end(),
    })
}

/// The value of a call's first argument when it is a plain string literal,
/// given the offset of the `(` that opens the argument list.
fn first_string_argument(content: &str, paren_open: usize) -> Option<String> {
    let rest = content.get(paren_open + 1..)?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let inner = rest.get(1..)?;
    // An escaped quote (`route('it\'s')`) does not close the literal, so the
    // scan skips it the way `scan_back_to_opener` does.  The value keeps its
    // escapes, matching how the names it is looked up against are recorded.
    let bytes = inner.as_bytes();
    let end = (0..bytes.len()).find(|&i| bytes[i] == quote as u8 && !is_escaped(bytes, i))?;
    Some(inner[..end].to_string())
}

/// Split off a trailing PHP identifier (`[A-Za-z0-9_]+`) from `s`, returning
/// `(identifier, remainder_before_it)`.
pub(crate) fn split_trailing_ident(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    let mut start = bytes.len();
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    (&s[start..], &s[..start])
}

/// Split off a trailing class-name token (identifier plus `\` separators).
pub(crate) fn trailing_class_name(s: &str) -> &str {
    let s = s.trim_end();
    let bytes = s.as_bytes();
    let mut start = bytes.len();
    while start > 0
        && (bytes[start - 1].is_ascii_alphanumeric()
            || bytes[start - 1] == b'_'
            || bytes[start - 1] == b'\\')
    {
        start -= 1;
    }
    &s[start..]
}

/// Recognise a closure or arrow-function literal, skipping an optional
/// `static` modifier.
///
/// Returns whether the literal is an arrow function, together with the text
/// from the `fn`/`function` keyword onward, so callers can scan the
/// signature without tripping over the modifier.  `static fn ($x) => …` is
/// an ordinary closure literal; a caller that stops at the modifier reads
/// it as "not a closure" and silently binds no template from it.
pub(crate) fn closure_literal_parts(text: &str) -> Option<(bool, &str)> {
    let trimmed = text.trim();
    let trimmed = trimmed
        .strip_prefix("static")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .map(str::trim_start)
        .unwrap_or(trimmed);
    if trimmed.starts_with("fn")
        && trimmed
            .get(2..)
            .is_some_and(|rest| rest.trim_start().starts_with('('))
    {
        return Some((true, trimmed));
    }
    if trimmed.starts_with("function")
        && trimmed
            .get(8..)
            .is_some_and(|rest| rest.trim_start().starts_with('('))
    {
        return Some((false, trimmed));
    }
    None
}

/// Check whether text is a closure or arrow-function literal, optionally
/// prefixed with `static` — e.g. `fn($x) => …`, `function ($x) use (…) { … }`,
/// `static fn($x) => …`.
pub(crate) fn is_closure_like_text(text: &str) -> bool {
    closure_literal_parts(text).is_some()
}

/// The offsets of a parameter list's opening `(` and its matching `)`.
///
/// The first `(` of `text` opens the list; the scan then pairs brackets
/// so a default value written as a call (`fn ($x = f(1))`) does not close
/// it early. `None` when there is no `(` or it is never closed.
fn parameter_list_parens(text: &str) -> Option<(usize, usize)> {
    let open = text.find('(')?;
    let close = crate::text_scan::find_matching_forward(text, open, b'(', b')')?;
    Some((open, close))
}

/// Extract the return type annotation from a closure or arrow-function
/// literal, given its own source text (e.g. the closure's span text, or
/// the text between a call's parentheses for one argument).
///
/// Handles:
/// - `fn(…): ReturnType => …`
/// - `function(…): ReturnType { … }`
/// - `function(…) use (…): ReturnType { … }`
///
/// Returns `None` if the text is not a closure/arrow-function or if
/// there is no return type hint.
pub(crate) fn extract_closure_return_type_from_text(text: &str) -> Option<PhpType> {
    let (_, trimmed) = closure_literal_parts(text)?;

    let (_, paren_close) = parameter_list_parens(trimmed)?;

    // After `)`, look for `: ReturnType`.
    let after_paren = trimmed.get(paren_close + 1..)?.trim_start();

    // For closures there may be a `use (…)` clause before the return type.
    let after_use = if after_paren.starts_with("use") {
        let (_, use_close) = parameter_list_parens(after_paren)?;
        after_paren.get(use_close + 1..)?.trim_start()
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
pub(crate) fn infer_generator_type_from_closure_yields(text: &str) -> Option<PhpType> {
    let (_, trimmed) = closure_literal_parts(text)?;

    // Find the opening `{` of the body.
    let body_start = trimmed.find('{')?;
    let body = &trimmed[body_start + 1..];

    let mut value_type: Option<PhpType> = None;
    let mut key_type: Option<PhpType> = None;
    let mut found_yield = false;
    let mut return_type: Option<PhpType> = None;

    for (keyword, after) in own_body_keywords(body) {
        let rest = body[after..].trim_start();
        match keyword {
            BodyKeyword::Yield => {
                found_yield = true;
                // `yield from` delegates to another iterator.
                if keyword_starts_at(rest.as_bytes(), 0, b"from") {
                    continue;
                }
                let Some(end) = find_statement_end(rest) else {
                    continue;
                };
                let yield_expr = rest[..end].trim();
                let (key, value) = match split_top_level_arrow(yield_expr) {
                    Some((key_text, value_text)) => {
                        (infer_type_from_simple_expr(key_text), value_text)
                    }
                    // A bare `yield $value` is keyed by position.
                    None => (Some(PhpType::int()), yield_expr),
                };
                if key_type.is_none() {
                    key_type = key;
                }
                if value_type.is_none() {
                    value_type = infer_type_from_simple_expr(value);
                }
            }
            BodyKeyword::Return => {
                let Some(end) = find_statement_end(rest) else {
                    continue;
                };
                let return_expr = rest[..end].trim();
                let inferred = if return_expr.is_empty() {
                    PhpType::void()
                } else {
                    infer_type_from_simple_expr(return_expr).unwrap_or_else(PhpType::mixed)
                };
                return_type = Some(match return_type.take() {
                    Some(existing) if existing.equivalent(&inferred) => existing,
                    Some(existing) => PhpType::union(vec![existing, inferred]),
                    None => inferred,
                });
            }
        }
    }

    if !found_yield {
        return None;
    }

    let key = key_type.unwrap_or_else(PhpType::int);
    let value = value_type.unwrap_or_else(PhpType::mixed);
    let ret = return_type.unwrap_or_else(PhpType::void);

    Some(PhpType::generic(
        "Generator",
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
pub(crate) fn extract_closure_body_expr_text(text: &str) -> Option<&str> {
    let (is_arrow, trimmed) = closure_literal_parts(text)?;

    let (_, paren_close) = parameter_list_parens(trimmed)?;

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

/// Find the first `return EXPR` the closure body owns (see
/// [`own_body_keywords`]).  Returns the expression text without the
/// trailing `;`.
fn find_first_return_expr(body: &str) -> Option<&str> {
    let (_, after) =
        own_body_keywords(body).find(|(keyword, _)| *keyword == BodyKeyword::Return)?;
    let rest = body[after..].trim_start();
    let expr = rest[..find_statement_end(rest)?].trim();
    if expr.is_empty() { None } else { Some(expr) }
}

/// A statement keyword found by [`own_body_keywords`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyKeyword {
    Yield,
    Return,
}

/// The `yield` and `return` keywords a function body owns, each with the
/// offset just past it, given the text just past the body's opening `{`.
///
/// Quotes and comments are skipped, and so is everything inside a nested
/// `function` closure, whose keywords belong to it.  Arrow `fn` closures
/// are single-expression and cannot hold either keyword.  The scan ends
/// at the body's closing `}`.
fn own_body_keywords(body: &str) -> impl Iterator<Item = (BodyKeyword, usize)> + '_ {
    let bytes = body.as_bytes();
    let mut offsets = CodeOffsets::new(bytes);
    let mut depth = 0u32;
    // While inside a nested `function` closure, the brace depth it was
    // declared at; scanning for keywords resumes once the depth is back.
    let mut nested_fn_base: Option<u32> = None;
    let keyword_at = |i: usize, keyword: &[u8]| {
        keyword_starts_at(bytes, i, keyword) && (i == 0 || bytes[i - 1] != b'$')
    };
    std::iter::from_fn(move || {
        while let Some(i) = offsets.next() {
            let found = match bytes[i] {
                b'{' => {
                    depth += 1;
                    continue;
                }
                b'}' => {
                    depth = depth.checked_sub(1)?;
                    if nested_fn_base.is_some_and(|base| depth <= base) {
                        nested_fn_base = None;
                    }
                    continue;
                }
                _ if nested_fn_base.is_some() => continue,
                b'f' if keyword_at(i, b"function") => {
                    nested_fn_base = Some(depth);
                    continue;
                }
                b'y' if keyword_at(i, b"yield") => (BodyKeyword::Yield, i + 5),
                b'r' if keyword_at(i, b"return") => (BodyKeyword::Return, i + 6),
                _ => continue,
            };
            offsets.skip_to(found.1);
            return Some(found);
        }
        None
    })
    .fuse()
}

/// Find the end of a statement: the first `;`, or the closer of an
/// enclosing bracket, outside quotes, comments and nested brackets.
fn find_statement_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0u32;
    for i in CodeOffsets::new(bytes) {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                let Some(outer) = depth.checked_sub(1) else {
                    return Some(i);
                };
                depth = outer;
            }
            b';' if depth == 0 => return Some(i),
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
            "array" => return Some(PhpType::named(atom("array"))),
            "object" => return Some(PhpType::named(atom("object"))),
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
pub(crate) fn extract_closure_param_type_from_text(text: &str, position: usize) -> Option<PhpType> {
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
pub(crate) fn extract_closure_params_from_text(
    text: &str,
) -> Option<Vec<(String, Option<PhpType>)>> {
    let (_, trimmed) = closure_literal_parts(text)?;

    let (paren_open, paren_close) = parameter_list_parens(trimmed)?;

    // Extract the parameter list text between the parens.
    let params_text = trimmed.get(paren_open + 1..paren_close)?.trim();
    if params_text.is_empty() {
        return Some(vec![]);
    }

    let mut result = Vec::new();
    for param in split_top_level_commas(params_text) {
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

/// Resolve the return type of a first-class callable expression, given
/// its own source text (e.g. `"strlen(...)"`, `"$this->method(...)"`,
/// `"ClassName::method(...)"`).
///
/// Handles:
///
/// - `strlen(...)` (standalone function)
/// - `$this->method(...)` (instance method)
/// - `$obj->method(...)` (instance method on resolved variable)
/// - `ClassName::method(...)` (static method)
/// - `self::method(...)` / `static::method(...)`
/// - `$obj->$name(...)` / `ClassName::$name(...)` (member name held in a
///   variable with a known literal string value)
/// - `$obj(...)` (an object whose class declares `__invoke()`)
///
/// Returns `None` if the text is not a recognised callable form or the
/// return type cannot be determined.
pub(crate) fn resolve_first_class_callable_return_type(
    callable_text: &str,
    rctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    // `SubjectExpr::parse_callee` strips the trailing `(...)` marker
    // itself, so the full callable text (including it) can be passed
    // straight through.
    let callee_expr = crate::type_engine::subject_expr::SubjectExpr::parse_callee(callable_text);

    // For method calls (instance and static), use the main pipeline
    // with a return type hint capture.  A bare variable callable
    // (`$test(...)`) goes through the same pipeline: the variable
    // invocation arm resolves the variable's own type and, for an
    // object holding `__invoke()`, that method's return type — which
    // covers `$test(...)` on an invokable object the same way it
    // already covers `$fn = $otherFn(...)` on a closure variable.
    match &callee_expr {
        crate::type_engine::subject_expr::SubjectExpr::MethodCall { .. }
        | crate::type_engine::subject_expr::SubjectExpr::StaticMethodCall { .. }
        | crate::type_engine::subject_expr::SubjectExpr::NewExpr { .. }
        | crate::type_engine::subject_expr::SubjectExpr::Variable(_) => {
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
                    Some(PhpType::named(classes[0].fqn()))
                } else {
                    Some(PhpType::union(
                        classes.iter().map(|c| PhpType::named(c.fqn())).collect(),
                    ))
                }
            })
        }
        crate::type_engine::subject_expr::SubjectExpr::FunctionCall(func_name) => {
            let function_loader = rctx.function_loader?;
            let func_info = function_loader(func_name, 0)?;
            func_info.return_type.clone()
        }
        _ => None,
    }
}

/// Resolve a chained array access, trying each candidate raw type
/// in order until one succeeds through the full segment walk.
///
/// Each candidate `PhpType` is fed through [`walk_array_segments`]. The
/// first whose segment walk succeeds is returned as-is, whether or not
/// it resolves to a class — a shape value that turns out to be a scalar
/// (`array{message: string}['message']` → `string`) is just as valid an
/// answer as a class-backed one, and the caller decides how to package
/// it (`ResolvedType::from_classes_with_hint` vs `from_type_string`).
/// Returns `None` when no candidate's segment walk succeeds.
pub(crate) fn try_chained_array_access_with_candidates<'a>(
    candidates: impl Iterator<Item = PhpType> + 'a,
    segments: &[BracketSegment],
    current_class: Option<&ClassInfo>,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    let current_class_fqn = current_class.map(ClassInfo::fqn);
    let current_class_name = current_class_fqn.as_deref().unwrap_or("");

    for candidate in candidates {
        if let Some(result) = walk_array_segments(
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

/// Walk bracket segments on a `PhpType`, narrowing it at each step.
///
/// Returns `Some(type)` when the full segment chain resolves
/// successfully (whatever the resulting type turns out to be — a
/// class, a scalar, or anything else), or `None` when a segment
/// cannot be applied at all (e.g. the array shape does not contain
/// the requested key).
fn walk_array_segments(
    base_type: &PhpType,
    segments: &[BracketSegment],
    current_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    // Expand type aliases before walking segments.  The raw type may
    // be an alias name like `UserData` that resolves to
    // `array{name: string, pen: Pen}`.  Without expansion the
    // segment walk would fail to extract shape values.
    let mut current = if let TypeKind::Named(_) = base_type.kind() {
        if let Some(expanded) = crate::type_engine::types::aliases::resolve_type_alias_typed(
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
            BracketSegment::ComputedIndex(_) | BracketSegment::ElementAccess => current
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
            let class_element = crate::type_engine::type_resolution::type_hint_to_classes_typed(
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
                crate::type_engine::variable::foreach_resolution::extract_iterable_element_type_from_class(
                    &merged,
                    class_loader,
                )
            });

            class_element?
        };

        // After each segment, the resulting type might itself be an
        // alias (e.g. a shape value defined as another alias).
        if let Some(expanded) = crate::type_engine::types::aliases::resolve_type_alias_typed(
            &current,
            current_class_name,
            all_classes,
            class_loader,
        ) {
            current = expanded;
        }
    }

    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resolve the call around the empty array key in `content`.
    fn array_key_call(content: &str) -> Option<ArrayKeyCall<'_>> {
        let quote_pos = content.rfind("''").unwrap();
        let code = crate::completion::source::code_context::code_context_at(content, quote_pos)?;
        enclosing_array_key_call(content, &code)
    }

    /// The name is read up to the literal's real closing quote, so an escaped
    /// quote inside it does not truncate the value.
    #[test]
    fn call_name_keeps_an_escaped_quote() {
        let call = array_key_call("route('it\\'s', ['' => 1]);").expect("a route() call");
        assert_eq!(call.name, "it\\'s");
        assert_eq!(call.callee, "route");
    }

    #[test]
    fn call_name_reads_a_plain_first_argument() {
        let call = array_key_call("route('users.show', ['' => 1]);").expect("a route() call");
        assert_eq!(call.name, "users.show");
    }

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
        let inferred = infer_generator_type_from_closure_yields(text);
        match inferred.as_ref().map(PhpType::kind) {
            Some(TypeKind::Generic(g)) if g.name == "Generator" => g.args.clone(),
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
    fn yield_scan_skips_quotes_and_comments() {
        // A `=>` inside the yielded string is not a key separator, and a
        // `;` inside it does not end the statement.
        let args = generator_args("function () { yield 'a => b; c'; }");
        assert_eq!(args[0], PhpType::int());
        assert_eq!(args[1], PhpType::string());

        // `yield` in a `#` or block comment is not a yield.
        assert_eq!(
            infer_generator_type_from_closure_yields(
                "function () {\n # yield 1;\n /* yield 2; */ return 1; }"
            ),
            None
        );
    }

    #[test]
    fn yielded_arrow_function_is_not_a_keyed_yield() {
        let args = generator_args("function () { yield fn($x) => $x; }");
        assert_eq!(args[0], PhpType::int());
    }

    #[test]
    fn param_default_with_comma_in_string_stays_one_param() {
        assert_eq!(
            extract_closure_param_type_from_text("fn(string $sep = ',', int $n) => 1", 1),
            Some(PhpType::parse("int"))
        );
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
