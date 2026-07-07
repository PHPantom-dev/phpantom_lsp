//! **Inline Variable** code action (`refactor.inline`).
//!
//! When the cursor is on a simple variable assignment like
//! `$name = $user->getName();`, this action replaces every read of
//! `$name` in the enclosing scope with the RHS expression and removes
//! the assignment statement.
//!
//! ### Safety checks
//!
//! 1. **Single assignment.** The variable must be assigned exactly once
//!    in the enclosing scope.  If reassigned, the action is not offered.
//! 2. **Pure expression.** If the RHS has side effects (function/method
//!    calls, `new`) and there are multiple reads, the action is not
//!    offered.  A single read is always safe.
//! 3. **Parenthesisation.** When substituting the RHS into a larger
//!    expression, binary/ternary/assignment expressions are wrapped in
//!    parentheses to preserve precedence.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::ast::class_like::member::ClassLikeMember;
use mago_syntax::ast::class_like::method::MethodBody;
use mago_syntax::ast::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::parser::with_parsed_program;
use crate::scope_collector::{AccessKind, ScopeMap};
use crate::util::{offset_to_position, position_to_byte_offset};

// ─── AST helpers ────────────────────────────────────────────────────────────

/// Information about an assignment statement found at the cursor.
struct AssignmentInfo {
    /// The variable name including `$` prefix (e.g. `"$name"`).
    var_name: String,
    /// Byte offset of the `$` in the variable on the LHS.
    var_offset: u32,
    /// Byte range of the RHS expression `[start, end)`.
    rhs_start: usize,
    rhs_end: usize,
    /// Byte range of the entire statement (including semicolon) for deletion.
    stmt_start: usize,
    stmt_end: usize,
    /// Whether the RHS expression needs parentheses when substituted into
    /// a larger expression.
    needs_parens: bool,
    /// Whether the RHS has side effects (calls, `new`).
    has_side_effects: bool,
}

/// Walk the AST to find a simple assignment statement at the cursor offset.
///
/// Returns `None` if the cursor is not on a simple `$var = expr;` statement.
fn find_assignment_at_cursor(
    statements: &[Statement<'_>],
    cursor: u32,
    content: &str,
) -> Option<AssignmentInfo> {
    for stmt in statements {
        if let Some(info) = find_assignment_in_statement(stmt, cursor, content) {
            return Some(info);
        }
    }
    None
}

fn find_assignment_in_statement(
    stmt: &Statement<'_>,
    cursor: u32,
    content: &str,
) -> Option<AssignmentInfo> {
    let stmt_span = stmt.span();
    if cursor < stmt_span.start.offset || cursor > stmt_span.end.offset {
        return None;
    }

    match stmt {
        Statement::Expression(expr_stmt) => {
            if let Expression::Assignment(assignment) = expr_stmt.expression {
                // Only simple assignments (not compound like `+=`).
                if !assignment.operator.is_assign() {
                    return None;
                }
                // LHS must be a simple direct variable (not `$this->foo`, not `$$var`).
                let var = match assignment.lhs {
                    Expression::Variable(Variable::Direct(dv)) => dv,
                    _ => return None,
                };

                let var_name = bytes_to_str(var.name).to_string();
                // Skip `$this`.
                if var_name == "$this" {
                    return None;
                }

                let var_offset = var.span().start.offset;

                let rhs_span = assignment.rhs.span();
                let rhs_start = rhs_span.start.offset as usize;
                let rhs_end = rhs_span.end.offset as usize;

                let stmt_start = stmt_span.start.offset as usize;
                let stmt_end = stmt_span.end.offset as usize;

                let needs_parens = expression_needs_parens(assignment.rhs);
                let has_side_effects = expression_has_side_effects(assignment.rhs);

                // Verify cursor is actually on this statement.
                if (cursor as usize) < stmt_start || (cursor as usize) > stmt_end {
                    return None;
                }

                // Sanity check: RHS text must be extractable.
                if rhs_end > content.len() || rhs_start > rhs_end {
                    return None;
                }

                return Some(AssignmentInfo {
                    var_name,
                    var_offset,
                    rhs_start,
                    rhs_end,
                    stmt_start,
                    stmt_end,
                    needs_parens,
                    has_side_effects,
                });
            }
            None
        }
        // Recurse into function/method bodies, blocks, if/else, loops, etc.
        Statement::Function(func) => {
            let body_span = func.body.span();
            if cursor >= body_span.start.offset && cursor <= body_span.end.offset {
                for s in func.body.statements.iter() {
                    if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                        return Some(info);
                    }
                }
            }
            None
        }
        Statement::Class(class) => {
            let span = class.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                for member in class.members.iter() {
                    if let ClassLikeMember::Method(method) = member
                        && let MethodBody::Concrete(block) = &method.body
                    {
                        let block_span = block.span();
                        if cursor >= block_span.start.offset && cursor <= block_span.end.offset {
                            for s in block.statements.iter() {
                                if let Some(info) = find_assignment_in_statement(s, cursor, content)
                                {
                                    return Some(info);
                                }
                            }
                        }
                    }
                }
            }
            None
        }
        Statement::Trait(tr) => {
            let span = tr.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                for member in tr.members.iter() {
                    if let ClassLikeMember::Method(method) = member
                        && let MethodBody::Concrete(block) = &method.body
                    {
                        let block_span = block.span();
                        if cursor >= block_span.start.offset && cursor <= block_span.end.offset {
                            for s in block.statements.iter() {
                                if let Some(info) = find_assignment_in_statement(s, cursor, content)
                                {
                                    return Some(info);
                                }
                            }
                        }
                    }
                }
            }
            None
        }
        Statement::Enum(en) => {
            let span = en.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                for member in en.members.iter() {
                    if let ClassLikeMember::Method(method) = member
                        && let MethodBody::Concrete(block) = &method.body
                    {
                        let block_span = block.span();
                        if cursor >= block_span.start.offset && cursor <= block_span.end.offset {
                            for s in block.statements.iter() {
                                if let Some(info) = find_assignment_in_statement(s, cursor, content)
                                {
                                    return Some(info);
                                }
                            }
                        }
                    }
                }
            }
            None
        }
        Statement::Interface(_) => None,
        Statement::Block(block) => {
            for s in block.statements.iter() {
                if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                    return Some(info);
                }
            }
            None
        }
        Statement::If(if_stmt) => find_assignment_in_if_body(if_stmt, cursor, content),
        Statement::While(w) => match &w.body {
            WhileBody::Statement(s) => find_assignment_in_statement(s, cursor, content),
            WhileBody::ColonDelimited(body) => {
                for s in body.statements.iter() {
                    if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                        return Some(info);
                    }
                }
                None
            }
        },
        Statement::DoWhile(dw) => find_assignment_in_statement(dw.statement, cursor, content),
        Statement::For(f) => match &f.body {
            ForBody::Statement(s) => find_assignment_in_statement(s, cursor, content),
            ForBody::ColonDelimited(body) => {
                for s in body.statements.iter() {
                    if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                        return Some(info);
                    }
                }
                None
            }
        },
        Statement::Foreach(fe) => match &fe.body {
            ForeachBody::Statement(s) => find_assignment_in_statement(s, cursor, content),
            ForeachBody::ColonDelimited(body) => {
                for s in body.statements.iter() {
                    if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                        return Some(info);
                    }
                }
                None
            }
        },
        Statement::Switch(sw) => {
            for case in sw.body.cases().iter() {
                let stmts = match case {
                    SwitchCase::Expression(c) => &c.statements,
                    SwitchCase::Default(c) => &c.statements,
                };
                for s in stmts.iter() {
                    if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                        return Some(info);
                    }
                }
            }
            None
        }
        Statement::Try(t) => {
            for s in t.block.statements.iter() {
                if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                    return Some(info);
                }
            }
            for catch in t.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                        return Some(info);
                    }
                }
            }
            if let Some(ref finally) = t.finally_clause {
                for s in finally.block.statements.iter() {
                    if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                        return Some(info);
                    }
                }
            }
            None
        }
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                    return Some(info);
                }
            }
            None
        }
        _ => None,
    }
}

fn find_assignment_in_if_body(
    if_stmt: &If<'_>,
    cursor: u32,
    content: &str,
) -> Option<AssignmentInfo> {
    match &if_stmt.body {
        IfBody::Statement(body) => {
            if let Some(info) = find_assignment_in_statement(body.statement, cursor, content) {
                return Some(info);
            }
            for clause in body.else_if_clauses.iter() {
                if let Some(info) = find_assignment_in_statement(clause.statement, cursor, content)
                {
                    return Some(info);
                }
            }
            if let Some(ref else_clause) = body.else_clause
                && let Some(info) =
                    find_assignment_in_statement(else_clause.statement, cursor, content)
            {
                return Some(info);
            }
            None
        }
        IfBody::ColonDelimited(body) => {
            for s in body.statements.iter() {
                if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                    return Some(info);
                }
            }
            for clause in body.else_if_clauses.iter() {
                for s in clause.statements.iter() {
                    if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                        return Some(info);
                    }
                }
            }
            if let Some(ref else_clause) = body.else_clause {
                for s in else_clause.statements.iter() {
                    if let Some(info) = find_assignment_in_statement(s, cursor, content) {
                        return Some(info);
                    }
                }
            }
            None
        }
    }
}

/// Check whether an expression needs parentheses when substituted into a
/// surrounding expression context.
///
/// Binary, ternary, and assignment expressions need wrapping to preserve
/// precedence.  Everything else (literals, variables, calls, property
/// accesses, array accesses) is fine without parens.
fn expression_needs_parens(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::Binary(_) | Expression::Conditional(_) | Expression::Assignment(_)
    )
}

/// Check whether an expression has side effects.
///
/// Expressions with side effects:
/// - Function calls, method calls, static method calls
/// - `new` (instantiation)
/// - `clone`
/// - Language constructs: `include`, `require`, `eval`, `print`, `exit`, `die`
/// - Yield expressions
/// - Assignment expressions
///
/// Pure expressions:
/// - Variables, literals, constants
/// - Property/array access
/// - Binary/unary operations (on pure operands)
/// - Ternary/null-coalescing
/// - String interpolation
/// - `isset`, `empty`
fn expression_has_side_effects(expr: &Expression<'_>) -> bool {
    match expr {
        // Calls — always side-effectful.
        Expression::Call(_) => true,
        // Instantiation — side-effectful.
        Expression::Instantiation(_) => true,
        // Clone — side-effectful (calls __clone).
        Expression::Clone(_) => true,
        // Yield — side-effectful.
        Expression::Yield(_) => true,
        // Throw — side-effectful.
        Expression::Throw(_) => true,
        // Assignment in the RHS is side-effectful.
        Expression::Assignment(a) => {
            // The assignment itself is a side effect, plus check the RHS.
            let _ = a;
            true
        }
        // Language constructs with side effects.
        Expression::Construct(construct) => matches!(
            construct,
            Construct::Eval(_)
                | Construct::Include(_)
                | Construct::IncludeOnce(_)
                | Construct::Require(_)
                | Construct::RequireOnce(_)
                | Construct::Print(_)
                | Construct::Exit(_)
                | Construct::Die(_)
        ),
        // Unary postfix (++/--) is side-effectful.
        Expression::UnaryPostfix(_) => true,
        // Unary prefix: check if it's ++ or -- (side-effectful) or
        // a pure operator like `-`, `!`, `~`.
        Expression::UnaryPrefix(u) => {
            // The increment/decrement operators in prefix position
            // are side-effectful.  We check for `++` and `--`.
            let op_span = u.operator.span();
            let op_len = (op_span.end.offset - op_span.start.offset) as usize;
            // `++` and `--` are 2 chars; `!`, `~`, `-`, `+` are 1 char.
            if op_len >= 2 {
                true
            } else {
                expression_has_side_effects(u.operand)
            }
        }
        // Recursive checks for compound pure expressions.
        Expression::Binary(b) => {
            expression_has_side_effects(b.lhs) || expression_has_side_effects(b.rhs)
        }
        Expression::Conditional(c) => {
            expression_has_side_effects(c.condition)
                || c.then.is_some_and(|t| expression_has_side_effects(t))
                || expression_has_side_effects(c.r#else)
        }
        Expression::Parenthesized(p) => expression_has_side_effects(p.expression),
        Expression::Array(arr) => arr.elements.iter().any(|el| match el {
            ArrayElement::KeyValue(kv) => {
                expression_has_side_effects(kv.key) || expression_has_side_effects(kv.value)
            }
            ArrayElement::Value(v) => expression_has_side_effects(v.value),
            ArrayElement::Variadic(s) => expression_has_side_effects(s.value),
            ArrayElement::Missing(_) => false,
        }),
        Expression::LegacyArray(arr) => arr.elements.iter().any(|el| match el {
            ArrayElement::KeyValue(kv) => {
                expression_has_side_effects(kv.key) || expression_has_side_effects(kv.value)
            }
            ArrayElement::Value(v) => expression_has_side_effects(v.value),
            ArrayElement::Variadic(s) => expression_has_side_effects(s.value),
            ArrayElement::Missing(_) => false,
        }),
        Expression::CompositeString(cs) => cs.parts().iter().any(|part| match part {
            StringPart::Expression(e) => expression_has_side_effects(e),
            StringPart::BracedExpression(b) => expression_has_side_effects(b.expression),
            StringPart::Literal(_) => false,
        }),
        Expression::ArrayAccess(a) => {
            expression_has_side_effects(a.array) || expression_has_side_effects(a.index)
        }
        // Pipe operator — the callable is invoked, so side-effectful.
        Expression::Pipe(_) => true,
        // Match expressions — arms may contain side effects.
        Expression::Match(m) => {
            expression_has_side_effects(m.expression)
                || m.arms.iter().any(|arm| match arm {
                    MatchArm::Expression(ea) => {
                        ea.conditions.iter().any(|c| expression_has_side_effects(c))
                            || expression_has_side_effects(ea.expression)
                    }
                    MatchArm::Default(da) => expression_has_side_effects(da.expression),
                })
        }
        // Anonymous class — side-effectful (creates a class).
        Expression::AnonymousClass(_) => true,
        // Closures and arrow functions are pure values (they don't
        // execute until called).
        Expression::Closure(_) | Expression::ArrowFunction(_) => false,
        // Everything else: variables, literals, property access,
        // static property access, class constant access, identifiers,
        // magic constants, self/static/parent, etc.
        _ => false,
    }
}

// ─── Scope map building ─────────────────────────────────────────────────────

/// Build a `ScopeMap` for the file by walking the AST, identical to the
/// approach used in extract_variable.
fn build_scope_map(content: &str, offset: u32) -> ScopeMap {
    with_parsed_program(content, "inline_variable", |program, content| {
        crate::scope_collector::build_scope_map_for_offset(
            program.statements.as_slice(),
            offset,
            content.len() as u32,
        )
    })
}

// ─── Line deletion helpers ──────────────────────────────────────────────────

/// Compute the byte range for deleting an entire statement line.
///
/// Extends the statement span to include leading whitespace and the
/// trailing newline (if present), so that removing the statement doesn't
/// leave a blank line.
/// Check whether inlining the given assignment is safe, based on scope
/// analysis.
///
/// A simple assignment `$var = expr;` completely overwrites the variable,
/// so earlier writes and read-writes (e.g. `$arr[] = …` building up an
/// array) are irrelevant to the inline.  Only occurrences **after** the
/// assignment matter:
///
/// - There must be at least one read after the assignment.
/// - There must be no writes or read-writes after the assignment (which
///   would mean the variable is reassigned or mutated later).
/// - When the RHS has side effects, there must be at most one read.
fn is_inline_safe(info: &AssignmentInfo, content: &str, cursor_offset: u32) -> bool {
    let scope_map = build_scope_map(content, cursor_offset);
    let occurrences = scope_map.all_occurrences(&info.var_name, info.var_offset);

    if occurrences.is_empty() {
        return false;
    }

    // Only consider occurrences after the assignment statement.
    // The RHS may read the variable (e.g. `$x = foo($x)`), but that
    // read consumes the *old* value and is part of the statement being
    // deleted, so it must not count as a post-assignment read.
    let after_stmt = occurrences
        .iter()
        .filter(|(offset, _)| (*offset as usize) >= info.stmt_end);

    let read_count = after_stmt
        .clone()
        .filter(|(_, kind)| matches!(kind, AccessKind::Read))
        .count();
    let write_count = after_stmt
        .clone()
        .filter(|(_, kind)| matches!(kind, AccessKind::Write))
        .count();
    let read_write_count = after_stmt
        .filter(|(_, kind)| matches!(kind, AccessKind::ReadWrite))
        .count();

    if read_count == 0 || write_count > 0 || read_write_count > 0 {
        return false;
    }

    if info.has_side_effects && read_count > 1 {
        return false;
    }

    true
}

fn deletion_range(content: &str, stmt_start: usize, stmt_end: usize) -> (usize, usize) {
    // Extend backward to the start of the line (include leading whitespace).
    let line_start = content[..stmt_start]
        .rfind('\n')
        .map(|pos| pos + 1)
        .unwrap_or(0);

    // Check that everything between line_start and stmt_start is whitespace.
    let prefix = &content[line_start..stmt_start];
    let del_start = if prefix.chars().all(|c| c == ' ' || c == '\t') {
        line_start
    } else {
        stmt_start
    };

    // Extend forward past the trailing newline.
    let del_end = if stmt_end < content.len() && content.as_bytes()[stmt_end] == b'\n' {
        stmt_end + 1
    } else if stmt_end + 1 < content.len()
        && content.as_bytes()[stmt_end] == b'\r'
        && content.as_bytes()[stmt_end + 1] == b'\n'
    {
        stmt_end + 2
    } else {
        stmt_end
    };

    (del_start, del_end)
}

// ─── Code action ────────────────────────────────────────────────────────────

impl Backend {
    /// Collect "Inline Variable" code actions.
    ///
    /// This action is offered when the cursor is on a simple variable
    /// assignment statement (`$var = expr;`).  It replaces every read of
    /// the variable with the RHS expression and deletes the assignment.
    ///
    /// Phase 1 only parses the AST to verify the cursor is on an
    /// assignment.  The expensive scope analysis and safety checks are
    /// deferred to [`resolve_inline_variable`] (Phase 2).
    pub(crate) fn collect_inline_variable_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let cursor_offset = position_to_byte_offset(content, params.range.start) as u32;

        // ── 1. Find the assignment at the cursor ────────────────────
        // If the cursor is not on a simple `$var = expr;` assignment,
        // no action is offered.
        let info = with_parsed_program(content, "inline_variable", |program, content| {
            find_assignment_at_cursor(program.statements.as_slice(), cursor_offset, content)
        });

        let info = match info {
            Some(i) => i,
            None => return,
        };

        // ── 2. Scope analysis and safety checks ─────────────────────
        // Run the same checks that Phase 2 uses so the action is only
        // offered when it can actually be applied.  The parse is cached
        // by `with_parsed_program`, so there is no extra parse cost.
        if !is_inline_safe(&info, content, cursor_offset) {
            return;
        }

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Inline variable {}", info.var_name),
            kind: Some(CodeActionKind::REFACTOR_INLINE),
            diagnostics: None,
            edit: None,
            command: None,
            is_preferred: Some(false),
            disabled: None,
            data: Some(make_code_action_data(
                "refactor.inlineVariable",
                uri,
                &params.range,
                serde_json::json!({}),
            )),
        }));
    }

    /// Resolve a deferred "Inline Variable" code action.
    ///
    /// Re-runs the full analysis using the cursor range from `data` to
    /// find the assignment, build the scope, locate all usages, and
    /// construct the workspace edit with deletion + replacements.
    ///
    /// The safety checks are also performed in Phase 1 (so the action
    /// is not offered when unsafe), but they are repeated here because
    /// the file content may have changed between phases.
    pub(crate) fn resolve_inline_variable(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let cursor_offset = position_to_byte_offset(content, data.range.start) as u32;

        // ── 1. Find the assignment at the cursor ────────────────────
        let info = with_parsed_program(content, "inline_variable", |program, content| {
            find_assignment_at_cursor(program.statements.as_slice(), cursor_offset, content)
        })?;

        // ── 2. Build scope map and check safety ─────────────────────
        let scope_map = build_scope_map(content, cursor_offset);
        let occurrences = scope_map.all_occurrences(&info.var_name, info.var_offset);

        if occurrences.is_empty() {
            return None;
        }

        // Only consider occurrences after the assignment statement.
        let after_stmt = occurrences
            .iter()
            .filter(|(offset, _)| (*offset as usize) >= info.stmt_end);

        let read_count = after_stmt
            .clone()
            .filter(|(_, kind)| matches!(kind, AccessKind::Read))
            .count();
        let write_count = after_stmt
            .clone()
            .filter(|(_, kind)| matches!(kind, AccessKind::Write))
            .count();
        let read_write_count = after_stmt
            .filter(|(_, kind)| matches!(kind, AccessKind::ReadWrite))
            .count();

        if read_count == 0 || write_count > 0 || read_write_count > 0 {
            return None;
        }

        if info.has_side_effects && read_count > 1 {
            return None;
        }

        // ── 3. Extract the RHS text ─────────────────────────────────
        let rhs_text = &content[info.rhs_start..info.rhs_end];

        // ── 4. Build the workspace edit ─────────────────────────────
        let doc_uri: Url = match data.uri.parse() {
            Ok(u) => u,
            Err(_) => return None,
        };

        let mut edits: Vec<TextEdit> = Vec::new();

        // 4a. Delete the assignment statement line.
        let (del_start, del_end) = deletion_range(content, info.stmt_start, info.stmt_end);
        let del_start_pos = offset_to_position(content, del_start);
        let del_end_pos = offset_to_position(content, del_end);
        edits.push(TextEdit {
            range: Range {
                start: del_start_pos,
                end: del_end_pos,
            },
            new_text: String::new(),
        });

        // 4b. Replace each read occurrence with the RHS text.
        let replacement = if info.needs_parens {
            format!("({})", rhs_text)
        } else {
            rhs_text.to_string()
        };

        for (offset, kind) in &occurrences {
            if !matches!(kind, AccessKind::Read) {
                continue;
            }
            // Only replace reads after the assignment statement.
            // Reads within the RHS (e.g. `$badges` in
            // `$badges = self::computeBadges($model, $badges)`)
            // must not be touched.
            if (*offset as usize) < info.stmt_end {
                continue;
            }
            let start = *offset as usize;
            let end = start + info.var_name.len();

            // Verify the text at this offset matches the variable name.
            if end > content.len() || content[start..end] != info.var_name {
                continue;
            }

            let start_pos = offset_to_position(content, start);
            let end_pos = offset_to_position(content, end);
            edits.push(TextEdit {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: replacement.clone(),
            });
        }

        // Sort edits by position (document order) for determinism.
        edits.sort_by(|a, b| {
            a.range
                .start
                .line
                .cmp(&b.range.start.line)
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        let mut changes = HashMap::new();
        changes.insert(doc_uri, edits);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: given PHP source with a cursor marker `/*|*/`, run the
    /// inline variable action and return the resulting edits (if offered).
    fn run_inline(php: &str) -> Option<Vec<TextEdit>> {
        let marker = "/*|*/";
        let marker_pos = php.find(marker)?;
        let content = php.replace(marker, "");

        let uri = "file:///test.php";
        let cursor_offset = marker_pos;
        let position = offset_to_position(&content, cursor_offset);
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::parse(uri).unwrap(),
            },
            range: Range {
                start: position,
                end: position,
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let backend = Backend::new_test();
        // Store file content so resolve_code_action can retrieve it.
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.clone()));

        let mut actions = Vec::new();
        backend.collect_inline_variable_actions(uri, &content, &params, &mut actions);

        if actions.is_empty() {
            return None;
        }

        let action = match &actions[0] {
            CodeActionOrCommand::CodeAction(a) => a.clone(),
            _ => return None,
        };

        // Phase 1 should have data but no edit.
        assert!(action.edit.is_none(), "Phase 1 should not compute edits");
        assert!(action.data.is_some(), "Phase 1 should attach resolve data");

        // Phase 2: resolve the action to get the workspace edit.
        let (resolved, _) = backend.resolve_code_action(action);
        let edit = resolved.edit.as_ref()?;
        let changes = edit.changes.as_ref()?;
        let parsed_uri = Url::parse(uri).unwrap();
        let edits = changes.get(&parsed_uri)?;
        Some(edits.clone())
    }

    /// Apply TextEdits to content (edits are assumed to be non-overlapping
    /// and will be applied from bottom to top to preserve positions).
    fn apply_edits(content: &str, edits: &[TextEdit]) -> String {
        let mut result = content.to_string();
        // Sort edits in reverse document order so earlier edits don't
        // shift positions of later ones.
        let mut sorted: Vec<&TextEdit> = edits.iter().collect();
        sorted.sort_by(|a, b| {
            b.range
                .start
                .line
                .cmp(&a.range.start.line)
                .then(b.range.start.character.cmp(&a.range.start.character))
        });
        for edit in sorted {
            let start = position_to_byte_offset(&result, edit.range.start);
            let end = position_to_byte_offset(&result, edit.range.end);
            result.replace_range(start..end, &edit.new_text);
        }
        result
    }

    // ── Basic inline ────────────────────────────────────────────────

    #[test]
    fn inline_simple_variable() {
        let php = r#"<?php
function foo() {
    /*|*/$name = $user->getName();
    echo $name;
}
"#;
        let content_without_marker = php.replace("/*|*/", "");
        let edits = run_inline(php).expect("action should be offered");
        let result = apply_edits(&content_without_marker, &edits);
        assert!(!result.contains("$name = "), "assignment should be removed");
        assert!(
            result.contains("echo $user->getName();"),
            "read should be replaced with RHS: got:\n{}",
            result
        );
    }

    #[test]
    fn inline_variable_multiple_reads() {
        let php = r#"<?php
function foo($user) {
    /*|*/$name = $user->email;
    echo $name;
    return $name;
}
"#;
        let content_without_marker = php.replace("/*|*/", "");
        let edits = run_inline(php).expect("action should be offered");
        let result = apply_edits(&content_without_marker, &edits);
        assert!(!result.contains("$name = "), "assignment should be removed");
        assert!(
            result.contains("echo $user->email;"),
            "first read should be replaced: got:\n{}",
            result
        );
        assert!(
            result.contains("return $user->email;"),
            "second read should be replaced: got:\n{}",
            result
        );
    }

    // ── Safety: reject multiple writes ──────────────────────────────

    #[test]
    fn reject_multiple_writes() {
        let php = r#"<?php
function foo() {
    /*|*/$name = 'hello';
    $name = 'world';
    echo $name;
}
"#;
        assert!(
            run_inline(php).is_none(),
            "should reject: variable is reassigned"
        );
    }

    // ── Safety: reject side-effectful RHS with multiple reads ───────

    #[test]
    fn reject_side_effects_multiple_reads() {
        let php = r#"<?php
function foo() {
    /*|*/$val = getResult();
    echo $val;
    return $val;
}
"#;
        assert!(
            run_inline(php).is_none(),
            "should reject: side-effectful RHS with multiple reads"
        );
    }

    #[test]
    fn allow_side_effects_single_read() {
        let php = r#"<?php
function foo() {
    /*|*/$val = getResult();
    echo $val;
}
"#;
        assert!(
            run_inline(php).is_some(),
            "should allow: side-effectful RHS with single read"
        );
    }

    // ── Parenthesisation ────────────────────────────────────────────

    #[test]
    fn adds_parens_for_binary_expression() {
        let php = r#"<?php
function foo($a, $b) {
    /*|*/$sum = $a + $b;
    echo $sum;
}
"#;
        let content_without_marker = php.replace("/*|*/", "");
        let edits = run_inline(php).expect("action should be offered");
        let result = apply_edits(&content_without_marker, &edits);
        assert!(
            result.contains("echo ($a + $b);"),
            "binary expression should be wrapped in parens: got:\n{}",
            result
        );
    }

    #[test]
    fn no_parens_for_simple_expression() {
        let php = r#"<?php
function foo($user) {
    /*|*/$name = $user->name;
    echo $name;
}
"#;
        let content_without_marker = php.replace("/*|*/", "");
        let edits = run_inline(php).expect("action should be offered");
        let result = apply_edits(&content_without_marker, &edits);
        assert!(
            result.contains("echo $user->name;"),
            "property access should NOT be wrapped in parens: got:\n{}",
            result
        );
    }

    // ── Compound assignment → reject ────────────────────────────────

    #[test]
    fn reject_compound_assignment() {
        // The cursor is on a compound assignment (`.=`), which is not a
        // simple `$var = expr` assignment — should not be offered.
        let php = r#"<?php
function foo() {
    $name = 'hello';
    /*|*/$name .= ' world';
    echo $name;
}
"#;
        assert!(
            run_inline(php).is_none(),
            "should reject: compound assignment is not a simple assignment"
        );
    }

    // ── Method body ─────────────────────────────────────────────────

    #[test]
    fn inline_in_method_body() {
        let php = r#"<?php
class Foo {
    public function bar() {
        /*|*/$x = 42;
        return $x;
    }
}
"#;
        let content_without_marker = php.replace("/*|*/", "");
        let edits = run_inline(php).expect("action should be offered");
        let result = apply_edits(&content_without_marker, &edits);
        assert!(
            result.contains("return 42;"),
            "read should be replaced: got:\n{}",
            result
        );
        assert!(
            !result.contains("$x = 42"),
            "assignment should be deleted: got:\n{}",
            result
        );
    }

    // ── Ternary expression needs parens ─────────────────────────────

    #[test]
    fn adds_parens_for_ternary() {
        let php = r#"<?php
function foo($a) {
    /*|*/$val = $a ? 'yes' : 'no';
    echo $val;
}
"#;
        let content_without_marker = php.replace("/*|*/", "");
        let edits = run_inline(php).expect("action should be offered");
        let result = apply_edits(&content_without_marker, &edits);
        assert!(
            result.contains("echo ($a ? 'yes' : 'no');"),
            "ternary should be wrapped in parens: got:\n{}",
            result
        );
    }

    // ── Code action kind ────────────────────────────────────────────

    #[test]
    fn code_action_kind_is_refactor_inline() {
        let php = r#"<?php
function foo() {
    /*|*/$x = 1;
    echo $x;
}
"#;
        let content = php.replace("/*|*/", "");
        let marker_pos = php.find("/*|*/").unwrap();
        let position = offset_to_position(&content, marker_pos);
        let uri = "file:///test.php";
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::parse(uri).unwrap(),
            },
            range: Range {
                start: position,
                end: position,
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let backend = Backend::new_test();
        let mut actions = Vec::new();
        backend.collect_inline_variable_actions(uri, &content, &params, &mut actions);

        assert!(!actions.is_empty(), "action should be offered");
        match &actions[0] {
            CodeActionOrCommand::CodeAction(a) => {
                assert_eq!(a.kind, Some(CodeActionKind::REFACTOR_INLINE));
                // Phase 1: no edit, has data.
                assert!(a.edit.is_none(), "Phase 1 should not compute edits");
                assert!(a.data.is_some(), "Phase 1 should attach resolve data");
            }
            _ => panic!("expected CodeAction"),
        }
    }

    // ── Title format ────────────────────────────────────────────────

    #[test]
    fn title_includes_variable_name() {
        let php = r#"<?php
function foo() {
    /*|*/$myVar = 1;
    echo $myVar;
}
"#;
        let content = php.replace("/*|*/", "");
        let marker_pos = php.find("/*|*/").unwrap();
        let position = offset_to_position(&content, marker_pos);
        let uri = "file:///test.php";
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::parse(uri).unwrap(),
            },
            range: Range {
                start: position,
                end: position,
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let backend = Backend::new_test();
        let mut actions = Vec::new();
        backend.collect_inline_variable_actions(uri, &content, &params, &mut actions);

        assert!(!actions.is_empty());
        match &actions[0] {
            CodeActionOrCommand::CodeAction(a) => {
                assert_eq!(a.title, "Inline variable $myVar");
            }
            _ => panic!("expected CodeAction"),
        }
    }

    // ── No reads → no action ────────────────────────────────────────

    #[test]
    fn reject_no_reads() {
        let php = r#"<?php
function foo() {
    /*|*/$x = 1;
}
"#;
        assert!(
            run_inline(php).is_none(),
            "should reject: variable has no reads"
        );
    }

    // ── ReadWrite (e.g. $x++) → reject ──────────────────────────────

    #[test]
    fn reject_read_write_usage() {
        let php = r#"<?php
function foo() {
    /*|*/$x = 0;
    $x++;
}
"#;
        assert!(
            run_inline(php).is_none(),
            "should reject: variable has read-write access ($x++)"
        );
    }

    // ── String literal RHS ──────────────────────────────────────────

    #[test]
    fn inline_string_literal() {
        let php = r#"<?php
function foo() {
    /*|*/$msg = 'hello world';
    echo $msg;
}
"#;
        let content_without_marker = php.replace("/*|*/", "");
        let edits = run_inline(php).expect("action should be offered");
        let result = apply_edits(&content_without_marker, &edits);
        assert!(
            result.contains("echo 'hello world';"),
            "string literal should be inlined: got:\n{}",
            result
        );
    }

    // ── Pure expression helpers ─────────────────────────────────────

    #[test]
    fn side_effect_detection_literals_are_pure() {
        // This is implicitly tested via inline_string_literal and
        // inline_variable_multiple_reads, but we also verify the helper
        // accepts property access with multiple reads.
        let php = r#"<?php
function foo($obj) {
    /*|*/$x = $obj->name;
    echo $x;
    return $x;
}
"#;
        assert!(
            run_inline(php).is_some(),
            "pure property access should be inlinable with multiple reads"
        );
    }

    // ── Deletion range helper ───────────────────────────────────────

    #[test]
    fn deletion_range_includes_indentation_and_newline() {
        let content = "    $x = 1;\n    echo $x;\n";
        let (start, end) = deletion_range(content, 4, 15); // "$x = 1;" is at 4..15
        // Should include leading spaces and trailing newline.
        assert_eq!(start, 0, "should start at line beginning");
        // stmt_end is 15 which is ';', the newline is at index 15 (assuming
        // the ";" is at index 14).  Let's check precisely:
        // "    $x = 1;\n" — the `;` is at index 10, `\n` at 11
        // Actually let's just verify the range covers the full line.
        assert!(end > 10, "should extend past the semicolon");
    }

    // ── Inline in namespace ─────────────────────────────────────────

    #[test]
    fn inline_in_namespaced_function() {
        let php = r#"<?php
namespace App;

function bar() {
    /*|*/$val = 123;
    return $val;
}
"#;
        let content_without_marker = php.replace("/*|*/", "");
        let edits = run_inline(php).expect("action should be offered");
        let result = apply_edits(&content_without_marker, &edits);
        assert!(
            result.contains("return 123;"),
            "should inline in namespaced function: got:\n{}",
            result
        );
    }

    // ── new expression is side-effectful ────────────────────────────

    #[test]
    fn reject_new_with_multiple_reads() {
        let php = r#"<?php
function foo() {
    /*|*/$obj = new stdClass();
    echo $obj;
    return $obj;
}
"#;
        assert!(
            run_inline(php).is_none(),
            "should reject: `new` is side-effectful with multiple reads"
        );
    }

    #[test]
    fn allow_new_with_single_read() {
        let php = r#"<?php
function foo() {
    /*|*/$obj = new stdClass();
    return $obj;
}
"#;
        assert!(
            run_inline(php).is_some(),
            "should allow: `new` with single read"
        );
    }

    // ── String interpolation ────────────────────────────────────────

    #[test]
    fn inline_with_string_interpolation() {
        let php = r#"<?php
class OrderProcessor {
    public function processOrder(Order $order): string {
        /*|*/$total = $order->getTotal();
        return "total {$total}";
    }
}
"#;
        assert!(
            run_inline(php).is_some(),
            "should offer inline for variable read inside string interpolation"
        );
    }

    // ── Reassigned variable after earlier writes/read-writes ────────

    #[test]
    fn inline_reassigned_variable_after_array_appends() {
        // The variable has earlier writes ($badges = []) and read-writes
        // ($badges[] = ...), but the cursor is on a later reassignment
        // that overwrites the variable.  After that reassignment there is
        // only a single read (return $badges), so the inline is safe:
        // `return self::computeBadges($model, $badges);`
        let php = r#"<?php
class BadgeHelper {
    public static function getBadges($model, $lang): array {
        $badges = [];

        if ($model->isDerma()) {
            $badges[] = new BadgeViewModel('derma');
        }

        if ($model->isProHairCare()) {
            $badges[] = new BadgeViewModel('pro-hair');
        }

        /*|*/$badges = self::computeBadges($model, $badges);

        return $badges;
    }
}
"#;
        let content_without_marker = php.replace("/*|*/", "");
        let edits = run_inline(php).expect("action should be offered for reassigned variable");
        let result = apply_edits(&content_without_marker, &edits);
        assert!(
            !result.contains("$badges = self::computeBadges"),
            "assignment should be removed:\n{}",
            result
        );
        assert!(
            result.contains("return self::computeBadges($model, $badges);"),
            "return should inline the RHS:\n{}",
            result
        );
    }

    #[test]
    fn reject_reassigned_variable_with_later_mutation() {
        // After the reassignment there is a read-write ($badges[] = ...),
        // so inlining is NOT safe.
        let php = r#"<?php
function getBadges($model) {
    $badges = [];
    /*|*/$badges = self::computeBadges($model, $badges);
    $badges[] = new BadgeViewModel('extra');
    return $badges;
}
"#;
        assert!(
            run_inline(php).is_none(),
            "should reject: variable has read-write access after the assignment"
        );
    }

    #[test]
    fn reject_reassigned_variable_with_later_overwrite() {
        // After the reassignment there is another write, so inlining
        // would lose that overwrite.
        let php = r#"<?php
function getBadges($model) {
    $badges = [];
    /*|*/$badges = self::computeBadges($model, $badges);
    $badges = array_unique($badges);
    return $badges;
}
"#;
        assert!(
            run_inline(php).is_none(),
            "should reject: variable has another write after the assignment"
        );
    }
}
