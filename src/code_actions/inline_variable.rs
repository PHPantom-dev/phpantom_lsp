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

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::parser::with_parsed_program;
use crate::scope_collector::{AccessKind, ScopeMap};
use crate::text_position::{offset_to_position, position_to_byte_offset};

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

/// Find the assignment at `cursor` inside whichever of a class-like's
/// methods contains it.
///
/// `own_span` gates the walk to class-likes the cursor is actually
/// inside, since a class declared later in the same file is otherwise
/// still visited and its (non-matching) methods scanned for nothing.
fn find_assignment_in_class_like<'a>(
    own_span: mago_span::Span,
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    cursor: u32,
    content: &str,
) -> Option<AssignmentInfo> {
    if cursor < own_span.start.offset || cursor > own_span.end.offset {
        return None;
    }
    let block = crate::util::find_enclosing_method_block_in_members(members, cursor)?;
    for s in block.statements.iter() {
        if let Some(info) = find_assignment_in_statement(s, cursor, content) {
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
            find_assignment_in_class_like(class.span(), class.members.iter(), cursor, content)
        }
        Statement::Trait(tr) => {
            find_assignment_in_class_like(tr.span(), tr.members.iter(), cursor, content)
        }
        Statement::Enum(en) => {
            find_assignment_in_class_like(en.span(), en.members.iter(), cursor, content)
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
            // The assignment itself is a side effect; the RHS need not be
            // inspected separately.
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

/// Compute the byte range for deleting an entire statement line.
///
/// Extends the statement span to include leading whitespace and the
/// trailing newline (if present), so that removing the statement doesn't
/// leave a blank line.
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
            if let Some(edit) = crate::code_actions::occurrence_replacement_edit(
                content,
                *offset as usize,
                &info.var_name,
                &replacement,
            ) {
                edits.push(edit);
            }
        }

        crate::code_actions::sort_edits_by_position(&mut edits);

        Some(crate::code_actions::single_file_edit(doc_uri, edits))
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Deletion range helper ───────────────────────────────────────

    #[test]
    fn deletion_range_includes_indentation_and_newline() {
        let content = "    $x = 1;\n    echo $x;\n";
        let (start, end) = deletion_range(content, 4, 15); // "$x = 1;" is at 4..15
        // Should include leading spaces and trailing newline.
        assert_eq!(start, 0, "should start at line beginning");
        assert!(end > 10, "should extend past the semicolon");
    }
}
