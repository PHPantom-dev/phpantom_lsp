use super::*;

// ─── Return statement analysis ──────────────────────────────────────────────

/// Analyse `return` statements within the selected range and determine
/// the extraction strategy.
///
/// The returned `ReturnStrategy` tells the code generator how to handle
/// early returns in the extracted code:
/// - `None` — no returns in the selection.
/// - `TrailingReturn` — last statement is `return`, call site uses
///   `return extracted(…)`.
/// - `VoidGuards` / `UniformGuards` / `SentinelNull` — guard-clause
///   patterns that can be safely extracted with special call sites.
/// - `Unsafe` — cannot safely extract.
///
/// `return_value_count` is the number of variables modified inside the
/// selection that are read after it (the scope classifier's
/// `return_values.len()`).  Most guard strategies are rejected when
/// this is non-zero, except `NullGuardWithValue` which handles exactly
/// one return value with all-null guards.
pub(crate) fn analyse_returns(
    content: &str,
    start: usize,
    end: usize,
    return_value_count: usize,
) -> ReturnStrategy {
    crate::parser::with_parsed_program(content, "extract_function", |program, content| {
        let body_stmts = find_enclosing_body_statements(&program.statements, start as u32);

        // Collect the statements that fall inside the selection.
        let selected: Vec<&Statement<'_>> = body_stmts
            .iter()
            .filter(|stmt| {
                let span = stmt.span();
                let s = span.start.offset as usize;
                let e = span.end.offset as usize;
                s >= start && e <= end
            })
            .copied()
            .collect();

        if selected.is_empty() {
            return Some(ReturnStrategy::None);
        }

        // Check whether the last selected statement is a `return`.
        let has_trailing_return = matches!(selected.last(), Some(Statement::Return(_)));

        // Check whether any statement in the selection contains a return
        // (at any nesting level).
        let any_return = selected.iter().any(|s| selection_stmt_contains_return(s));

        if !any_return {
            return Some(ReturnStrategy::None);
        }

        // When the selection ends with `return`, the call site is
        // `return extracted(…)`, so every return path inside the
        // extracted function propagates correctly.
        if has_trailing_return {
            return Some(ReturnStrategy::TrailingReturn);
        }

        // The selection contains returns but does NOT end with one.
        // Try to find a guard-clause strategy.
        Some(classify_guard_returns(
            content,
            &selected,
            return_value_count,
        ))
    })
    .unwrap_or(ReturnStrategy::None)
}

/// Check whether a statement is or contains a `return` at any depth.
pub(crate) fn selection_stmt_contains_return(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Return(_) => true,
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => {
                selection_stmt_contains_return(body.statement)
                    || body
                        .else_if_clauses
                        .iter()
                        .any(|c| selection_stmt_contains_return(c.statement))
                    || body
                        .else_clause
                        .as_ref()
                        .is_some_and(|c| selection_stmt_contains_return(c.statement))
            }
            IfBody::ColonDelimited(body) => {
                body.statements
                    .iter()
                    .any(|s| selection_stmt_contains_return(s))
                    || body.else_if_clauses.iter().any(|c| {
                        c.statements
                            .iter()
                            .any(|s| selection_stmt_contains_return(s))
                    })
                    || body.else_clause.as_ref().is_some_and(|c| {
                        c.statements
                            .iter()
                            .any(|s| selection_stmt_contains_return(s))
                    })
            }
        },
        Statement::Foreach(f) => match &f.body {
            ForeachBody::Statement(s) => selection_stmt_contains_return(s),
            ForeachBody::ColonDelimited(b) => b
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s)),
        },
        Statement::While(w) => match &w.body {
            WhileBody::Statement(s) => selection_stmt_contains_return(s),
            WhileBody::ColonDelimited(b) => b
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s)),
        },
        Statement::DoWhile(dw) => selection_stmt_contains_return(dw.statement),
        Statement::For(f) => match &f.body {
            ForBody::Statement(s) => selection_stmt_contains_return(s),
            ForBody::ColonDelimited(b) => b
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s)),
        },
        Statement::Switch(sw) => sw.body.cases().iter().any(|c| match c {
            SwitchCase::Expression(e) => e
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s)),
            SwitchCase::Default(d) => d
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s)),
        }),
        Statement::Try(t) => {
            t.block
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s))
                || t.catch_clauses.iter().any(|c| {
                    c.block
                        .statements
                        .iter()
                        .any(|s| selection_stmt_contains_return(s))
                })
                || t.finally_clause.as_ref().is_some_and(|f| {
                    f.block
                        .statements
                        .iter()
                        .any(|s| selection_stmt_contains_return(s))
                })
        }
        Statement::Block(b) => b
            .statements
            .iter()
            .any(|s| selection_stmt_contains_return(s)),
        _ => false,
    }
}

// ─── Return strategy ────────────────────────────────────────────────────────

/// How to handle return statements in the extracted code.
///
/// When the selection contains `return` statements that are NOT the last
/// statement, naive extraction would break control flow.  This enum
/// describes the strategy for preserving the caller's early-exit
/// semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReturnStrategy {
    /// No return statements in the selection.
    None,
    /// The last selected statement is a `return` — the call site becomes
    /// `return extracted(…)` and every return path propagates correctly.
    TrailingReturn,
    /// All returns are bare `return;` (void guards).  The extracted
    /// function returns `bool` (true = continue, false = exit early)
    /// and the call site is `if (!extracted(…)) return;`.
    VoidGuards,
    /// All returns return the same non-null literal value.  The
    /// extracted function returns `bool` and the call site is
    /// `if (!extracted(…)) return <value>;`.
    ///
    /// The string is the source text of the common return value.
    UniformGuards(String),
    /// Returns have different non-null values — use `null` as a
    /// sentinel for "no early exit."  The extracted function returns
    /// `?<type>` and the call site is:
    /// ```php
    /// $result = extracted(…);
    /// if ($result !== null) return $result;
    /// ```
    SentinelNull,
    /// All guard returns are `null` (or bare `return;`) and the
    /// selection also computes exactly one return value.  The extracted
    /// function returns the computed value on success or `null` when a
    /// guard fires.  The call site assigns the result and checks for
    /// null:
    /// ```php
    /// $var = extracted(…);
    /// if ($var === null) return null;  // or `return;` for void guards
    /// ```
    ///
    /// The `bool` flag is `true` when the original guards were bare
    /// `return;` (void).  In that case the body's `return;` statements
    /// are rewritten to `return null;`, and the call site uses bare
    /// `return;` instead of `return null;`.
    NullGuardWithValue(bool),
    /// Cannot safely extract (e.g. returns null, or modified variables
    /// are used after the selection).
    Unsafe,
}

/// Collect the source text of every `return` expression in the selected
/// statements.
///
/// Bare `return;` is represented as `None`.  `return expr;` yields
/// `Some("expr")` with the expression's source text.
pub(crate) fn collect_return_expressions<'a>(
    content: &'a str,
    stmts: &[&Statement<'_>],
) -> Vec<Option<&'a str>> {
    let mut out = Vec::new();
    for stmt in stmts {
        collect_returns_from_stmt(content, stmt, &mut out);
    }
    out
}

/// Recursively collect return expressions from a single statement.
pub(crate) fn collect_returns_from_stmt<'a>(
    content: &'a str,
    stmt: &Statement<'_>,
    out: &mut Vec<Option<&'a str>>,
) {
    match stmt {
        Statement::Return(ret) => {
            let expr_text = ret.value.as_ref().map(|expr| {
                let s = expr.span().start.offset as usize;
                let e = expr.span().end.offset as usize;
                content[s..e].trim()
            });
            out.push(expr_text);
        }
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => {
                collect_returns_from_stmt(content, body.statement, out);
                for c in &body.else_if_clauses {
                    collect_returns_from_stmt(content, c.statement, out);
                }
                if let Some(c) = &body.else_clause {
                    collect_returns_from_stmt(content, c.statement, out);
                }
            }
            IfBody::ColonDelimited(body) => {
                for s in &body.statements {
                    collect_returns_from_stmt(content, s, out);
                }
                for c in &body.else_if_clauses {
                    for s in &c.statements {
                        collect_returns_from_stmt(content, s, out);
                    }
                }
                if let Some(c) = &body.else_clause {
                    for s in &c.statements {
                        collect_returns_from_stmt(content, s, out);
                    }
                }
            }
        },
        Statement::Foreach(f) => match &f.body {
            ForeachBody::Statement(s) => collect_returns_from_stmt(content, s, out),
            ForeachBody::ColonDelimited(b) => {
                for s in &b.statements {
                    collect_returns_from_stmt(content, s, out);
                }
            }
        },
        Statement::While(w) => match &w.body {
            WhileBody::Statement(s) => collect_returns_from_stmt(content, s, out),
            WhileBody::ColonDelimited(b) => {
                for s in &b.statements {
                    collect_returns_from_stmt(content, s, out);
                }
            }
        },
        Statement::DoWhile(dw) => collect_returns_from_stmt(content, dw.statement, out),
        Statement::For(f) => match &f.body {
            ForBody::Statement(s) => collect_returns_from_stmt(content, s, out),
            ForBody::ColonDelimited(b) => {
                for s in &b.statements {
                    collect_returns_from_stmt(content, s, out);
                }
            }
        },
        Statement::Switch(sw) => {
            for c in sw.body.cases().iter() {
                let stmts = match c {
                    SwitchCase::Expression(e) => &e.statements,
                    SwitchCase::Default(d) => &d.statements,
                };
                for s in stmts.iter() {
                    collect_returns_from_stmt(content, s, out);
                }
            }
        }
        Statement::Try(t) => {
            for s in &t.block.statements {
                collect_returns_from_stmt(content, s, out);
            }
            for c in &t.catch_clauses {
                for s in &c.block.statements {
                    collect_returns_from_stmt(content, s, out);
                }
            }
            if let Some(f) = &t.finally_clause {
                for s in &f.block.statements {
                    collect_returns_from_stmt(content, s, out);
                }
            }
        }
        Statement::Block(b) => {
            for s in &b.statements {
                collect_returns_from_stmt(content, s, out);
            }
        }
        _ => {}
    }
}

/// Whether a guard's return value is safe to reproduce verbatim at the
/// call site.
///
/// `UniformGuards` re-emits the return expression at the call site
/// (`if (!extracted(…)) return <value>;`).  That is only correct when the
/// value is a side-effect-free literal or constant: it must not reference
/// a variable (which could be local to the selection and out of scope at
/// the call site) and must not be a call (which would run twice).  String
/// literals are allowed even though they may contain `(`.
pub(crate) fn is_reproducible_guard_value(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return false;
    }
    // Any variable reference is risky — it may be selection-local.
    if v.contains('$') {
        return false;
    }
    // Quoted string literals are fine (their contents are inert).
    let single = v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'');
    let double = v.len() >= 2 && v.starts_with('"') && v.ends_with('"');
    if single || double {
        return true;
    }
    // Numbers and constants (e.g. `42`, `Status::Bad`, `self::FOO`) are
    // fine; a `(` indicates a call, which may have side effects.
    !v.contains('(')
}

/// Classify the return strategy for a selection that contains return
/// statements but does NOT end with one.
///
/// This is called only when `has_unsafe_return` would have been `true`
/// under the old logic.  It inspects the actual return expressions to
/// decide whether a safe extraction pattern exists.
pub(crate) fn classify_guard_returns(
    content: &str,
    stmts: &[&Statement<'_>],
    return_value_count: usize,
) -> ReturnStrategy {
    let return_exprs = collect_return_expressions(content, stmts);
    if return_exprs.is_empty() {
        return ReturnStrategy::Unsafe;
    }

    // When the selection modifies variables that are used after it,
    // most guard strategies can't work — we'd need to return both
    // the sentinel and the modified variables.  The exception is
    // NullGuardWithValue: all guards return null (or bare return;),
    // exactly one return value, and the extracted function returns
    // the value or null.
    if return_value_count > 0 {
        if return_value_count != 1 {
            return ReturnStrategy::Unsafe;
        }
        // All bare `return;` → NullGuardWithValue(true) (void guards).
        if return_exprs.iter().all(|e| e.is_none()) {
            return ReturnStrategy::NullGuardWithValue(true);
        }
        // All `return null;` → NullGuardWithValue(false).
        if return_exprs.iter().any(|e| e.is_none()) {
            // Mix of bare and valued returns — can't handle.
            return ReturnStrategy::Unsafe;
        }
        let all_null = return_exprs
            .iter()
            .all(|e| e.unwrap().trim().eq_ignore_ascii_case("null"));
        if all_null {
            return ReturnStrategy::NullGuardWithValue(false);
        }
        return ReturnStrategy::Unsafe;
    }

    // Case 1: All returns are bare `return;` (void guards).
    if return_exprs.iter().all(|e| e.is_none()) {
        return ReturnStrategy::VoidGuards;
    }

    // If any return is bare but others aren't, we have a mix of void
    // and valued returns — can't handle this.
    if return_exprs.iter().any(|e| e.is_none()) {
        return ReturnStrategy::Unsafe;
    }

    // All returns have values.  Check if any returns null.
    let values: Vec<&str> = return_exprs.iter().map(|e| e.unwrap()).collect();
    let any_returns_null = values.iter().any(|v| {
        let lower = v.trim().to_lowercase();
        lower == "null"
    });

    // Case 2: All return the same value.
    let all_same = values.windows(2).all(|w| w[0].trim() == w[1].trim());
    if all_same {
        let value = values[0].trim().to_string();
        let lower = value.to_lowercase();
        // `true`/`false`/`null` are always safe to reproduce at the call
        // site: the extracted function returns bool and the call site
        // does `if (!extracted(…)) return <value>;`.
        if lower == "false" || lower == "true" || lower == "null" {
            return ReturnStrategy::UniformGuards(value);
        }
        // Other uniform values can only be reproduced at the call site
        // when they are side-effect-free literals or constants.  A value
        // that references a variable or a call (e.g. `redirect($url)`)
        // may depend on variables that are local to the selection and
        // therefore out of scope at the call site, or it may have side
        // effects that must not run twice.  Such a value is kept inside
        // the extracted function via the null sentinel below.
        if is_reproducible_guard_value(&value) {
            return ReturnStrategy::UniformGuards(value);
        }
        // Fall through to the null-sentinel strategy.
    }

    // Case 3: Different (or non-reproducible) values, none are null —
    // use null sentinel so the return expressions stay inside the
    // extracted function and propagate through `$result`.
    if !any_returns_null {
        return ReturnStrategy::SentinelNull;
    }

    // Different values including null — can't use null as sentinel
    // and can't use bool flag either.
    ReturnStrategy::Unsafe
}

/// Resolve the return type of the enclosing function/method at `offset`.
///
/// Extracts the native return type hint from the function signature.
/// Extract the parameter names of the enclosing function/method in
/// declaration order.  Used to sort extracted-function parameters so
/// they mirror the original signature.
pub(crate) fn resolve_enclosing_param_order(content: &str, offset: u32) -> Vec<String> {
    crate::parser::with_parsed_program(content, "extract_function", |program, _| {
        let ctx = find_cursor_context(&program.statements, offset);

        let param_list = match ctx {
            CursorContext::InClassLike { member, .. } => {
                if let MemberContext::Method(method, true) = member {
                    Some(&method.parameter_list)
                } else {
                    None
                }
            }
            CursorContext::InFunction(func, true) => Some(&func.parameter_list),
            _ => None,
        };

        match param_list {
            Some(pl) => pl
                .parameters
                .iter()
                .map(|p| bytes_to_str(p.variable.name).to_string())
                .collect(),
            None => Vec::new(),
        }
    })
}

/// Sort extracted-function parameters so that variables matching the
/// enclosing function's signature come first (in their original order),
/// followed by any other variables in classification order.
pub(crate) fn sort_params_by_enclosing_order(
    mut params: Vec<(String, PhpType, PhpType)>,
    enclosing_order: &[String],
) -> Vec<(String, PhpType, PhpType)> {
    if enclosing_order.is_empty() {
        return params;
    }
    params.sort_by(|a, b| {
        let idx_a = enclosing_order.iter().position(|n| *n == a.0);
        let idx_b = enclosing_order.iter().position(|n| *n == b.0);
        match (idx_a, idx_b) {
            // Both are signature params → preserve signature order.
            (Some(ia), Some(ib)) => ia.cmp(&ib),
            // Signature params come before non-signature variables.
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            // Neither is a signature param → preserve classification order.
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    params
}

pub(crate) fn resolve_enclosing_return_type(content: &str, offset: u32) -> PhpType {
    crate::parser::with_parsed_program(content, "extract_function", |program, _| {
        let ctx = find_cursor_context(&program.statements, offset);

        let ty = match ctx {
            CursorContext::InClassLike { member, .. } => {
                if let MemberContext::Method(method, true) = member {
                    method
                        .return_type_hint
                        .as_ref()
                        .map(|h| crate::parser::extract_hint_type(&h.hint))
                        .unwrap_or_else(PhpType::untyped)
                } else {
                    PhpType::untyped()
                }
            }
            CursorContext::InFunction(func, true) => func
                .return_type_hint
                .as_ref()
                .map(|h| crate::parser::extract_hint_type(&h.hint))
                .unwrap_or_else(PhpType::untyped),
            _ => PhpType::untyped(),
        };
        Some(ty)
    })
    .unwrap_or_else(PhpType::untyped)
}
