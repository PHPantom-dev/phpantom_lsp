//! **Convert switch to match** code action (`refactor.rewrite`).
//!
//! Converts a `switch` statement to a `match` expression when the
//! conversion is safe (PHP 8.0+).
//!
//! The action is offered when:
//! - Every case body is a single expression statement (assignment to the
//!   same variable, or a `return`), optionally followed by `break`.
//! - No case body falls through to the next without `break`/`return`/`throw`.
//! - The switch subject is a simple expression.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::cursor_context::innermost_at;
use crate::code_actions::indent_of_line_at;
use crate::text_position::position_to_byte_offset;

impl Backend {
    /// Collect "Convert to match expression" code actions for switch
    /// statements at the cursor position.
    pub(crate) fn collect_convert_switch_to_match_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let php_version = self.php_version();
        if php_version < crate::types::PhpVersion::new(8, 0) {
            return;
        }

        let doc_uri: Url = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let cursor_offset = position_to_byte_offset(content, params.range.start) as u32;

        let found = crate::parser::with_parsed_program(
            content,
            "convert_switch_to_match",
            |program, content| convertible_switch_at(program, cursor_offset, content),
        );
        let Some((span, replacement)) = found else {
            return;
        };

        out.push(crate::code_actions::rewrite_action(
            "Convert to match expression".to_string(),
            doc_uri,
            content,
            (span.start.offset as usize, span.end.offset as usize),
            replacement,
            false,
        ));
    }
}

/// The "mode" detected from analyzing all switch arms.
#[derive(Debug, Clone, PartialEq)]
enum SwitchMode {
    /// All arms return a value: `return expr;`
    Return,
    /// All arms assign to the same variable: `$var = expr;`
    Assignment(String),
    /// A throw expression — compatible with any mode.
    Throw,
}

/// A single converted match arm.
struct MatchArm {
    /// The condition(s) — `None` for `default`.
    conditions: Option<Vec<String>>,
    /// The RHS expression text.
    body: String,
}

/// The innermost `switch` under `cursor` that can become a `match`, with
/// its span and the replacement text.
fn convertible_switch_at(
    program: &Program<'_>,
    cursor: u32,
    content: &str,
) -> Option<(mago_span::Span, String)> {
    innermost_at(program, cursor, |node| match node {
        Node::Switch(switch) => Some((switch.span(), try_convert_switch(switch, content)?)),
        _ => None,
    })
}

/// Attempt to convert a switch statement to a match expression.
/// Returns the replacement text if conversion is safe.
fn try_convert_switch(sw: &control_flow::switch::Switch<'_>, content: &str) -> Option<String> {
    let cases = sw.body.cases();
    if cases.is_empty() {
        return None;
    }

    let mut arms: Vec<MatchArm> = Vec::new();
    let mut mode: Option<SwitchMode> = None;
    // Track fall-through: consecutive cases with empty bodies share conditions.
    let mut pending_conditions: Vec<String> = Vec::new();

    for case in cases.iter() {
        let statements = case.statements();
        let is_default = case.is_default();
        let condition_text = if is_default {
            None
        } else {
            Some(source_text(content, case.expression().unwrap().span()).to_string())
        };

        // Empty body means fall-through to next case.
        if statements.is_empty() {
            if let Some(cond) = condition_text {
                pending_conditions.push(cond);
            } else {
                // `default:` with empty body followed by more cases — unusual but
                // means default falls through. Not safe to convert.
                return None;
            }
            continue;
        }

        // Collect the meaningful statements (ignoring trailing break).
        let (body_stmts, _has_break) = strip_trailing_break(statements);

        // Must have exactly one meaningful statement.
        if body_stmts.len() != 1 {
            return None;
        }

        let stmt = &body_stmts[0];

        let (arm_mode, arm_expr) = classify_arm_statement(stmt, content)?;

        // Verify consistency with previous arms.
        match (&mode, &arm_mode) {
            (None, SwitchMode::Throw) => {
                // Don't set mode from a throw-only arm; wait for a real mode.
            }
            (None, _) => mode = Some(arm_mode.clone()),
            (Some(_), SwitchMode::Throw) => {
                // Throw is compatible with any mode.
            }
            (Some(SwitchMode::Throw), _) => {
                // Previous was only throws; adopt this arm's mode.
                mode = Some(arm_mode.clone());
            }
            (Some(existing), _) => {
                if *existing != arm_mode {
                    return None;
                }
            }
        }

        // Build the conditions list for this arm.
        let conditions = if is_default {
            if !pending_conditions.is_empty() {
                // Fall-through into default — not safe.
                return None;
            }
            None
        } else {
            let mut conds = std::mem::take(&mut pending_conditions);
            conds.push(condition_text.unwrap());
            Some(conds)
        };

        arms.push(MatchArm {
            conditions,
            body: arm_expr,
        });
    }

    // If there are pending conditions at the end (fall-through without a body),
    // that's not convertible.
    if !pending_conditions.is_empty() {
        return None;
    }

    let mode = match mode {
        Some(SwitchMode::Throw) | None => return None,
        Some(m) => m,
    };

    let switch_start = sw.span().start.offset as usize;
    let indent = indent_of_line_at(content, switch_start);

    // Build the match expression.
    let subject = source_text(content, sw.expression.span());
    let mut result = String::new();

    match &mode {
        SwitchMode::Return | SwitchMode::Throw => {
            result.push_str("return match (");
            result.push_str(subject);
            result.push_str(") {\n");
        }
        SwitchMode::Assignment(var) => {
            result.push_str(var);
            result.push_str(" = match (");
            result.push_str(subject);
            result.push_str(") {\n");
        }
    }

    for arm in &arms {
        result.push_str(indent);
        result.push_str("    ");
        match &arm.conditions {
            Some(conds) => result.push_str(&conds.join(", ")),
            None => result.push_str("default"),
        }
        result.push_str(" => ");
        result.push_str(&arm.body);
        result.push_str(",\n");
    }

    result.push_str(indent);
    result.push_str("};");

    Some(result)
}

/// Classify a single arm statement. Returns the mode and the expression text.
fn classify_arm_statement<'a>(
    stmt: &'a Statement<'a>,
    content: &str,
) -> Option<(SwitchMode, String)> {
    match stmt {
        Statement::Return(ret) => {
            let value = ret.value?;
            let expr_text = source_text(content, value.span()).to_string();
            Some((SwitchMode::Return, expr_text))
        }
        Statement::Expression(expr_stmt) => match expr_stmt.expression {
            Expression::Assignment(assignment) => {
                if !assignment.operator.is_assign() {
                    return None;
                }
                let lhs_text = source_text(content, assignment.lhs.span()).to_string();
                let rhs_text = source_text(content, assignment.rhs.span()).to_string();
                Some((SwitchMode::Assignment(lhs_text), rhs_text))
            }
            Expression::Throw(_) => {
                let expr_text = source_text(content, expr_stmt.expression.span()).to_string();
                Some((SwitchMode::Throw, expr_text))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Strip a trailing `break;` (with no level) from a statement list.
/// Returns the remaining statements and whether a break was found.
fn strip_trailing_break<'a>(statements: &'a [Statement<'a>]) -> (&'a [Statement<'a>], bool) {
    if let Some(Statement::Break(brk)) = statements.last()
        && brk.level.is_none()
    {
        return (&statements[..statements.len() - 1], true);
    }
    (statements, false)
}

fn source_text(content: &str, span: mago_span::Span) -> &str {
    &content[span.start.offset as usize..span.end.offset as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The conversion offered with the cursor on the first `switch`.
    fn try_convert(php: &str) -> Option<String> {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let cursor = php.find("switch")? as u32;
        convertible_switch_at(program, cursor, php).map(|(_, replacement)| replacement)
    }

    #[test]
    fn simple_return() {
        let php = r#"<?php
switch ($x) {
    case 1:
        return 'one';
    case 2:
        return 'two';
    default:
        return 'other';
}"#;
        let result = try_convert(php).unwrap();
        assert!(result.contains("return match ("));
        assert!(result.contains("1 => 'one'"));
        assert!(result.contains("2 => 'two'"));
        assert!(result.contains("default => 'other'"));
    }

    #[test]
    fn simple_assignment() {
        let php = r#"<?php
switch ($status) {
    case 'active':
        $label = 'Active';
        break;
    case 'inactive':
        $label = 'Inactive';
        break;
    default:
        $label = 'Unknown';
        break;
}"#;
        let result = try_convert(php).unwrap();
        assert!(result.contains("$label = match ("));
        assert!(result.contains("'active' => 'Active'"));
        assert!(result.contains("'inactive' => 'Inactive'"));
        assert!(result.contains("default => 'Unknown'"));
    }

    #[test]
    fn fall_through_cases() {
        let php = r#"<?php
switch ($x) {
    case 1:
    case 2:
        return 'low';
    case 3:
        return 'high';
}"#;
        let result = try_convert(php).unwrap();
        assert!(result.contains("1, 2 => 'low'"));
        assert!(result.contains("3 => 'high'"));
    }

    #[test]
    fn rejected_multiple_statements() {
        let php = r#"<?php
switch ($x) {
    case 1:
        $a = 1;
        $b = 2;
        break;
}"#;
        assert!(try_convert(php).is_none());
    }

    #[test]
    fn rejected_different_assignment_targets() {
        let php = r#"<?php
switch ($x) {
    case 1:
        $a = 'one';
        break;
    case 2:
        $b = 'two';
        break;
}"#;
        assert!(try_convert(php).is_none());
    }

    #[test]
    fn rejected_mixed_return_and_assignment() {
        let php = r#"<?php
switch ($x) {
    case 1:
        return 'one';
    case 2:
        $result = 'two';
        break;
}"#;
        assert!(try_convert(php).is_none());
    }

    #[test]
    fn throw_in_arms() {
        let php = r#"<?php
switch ($x) {
    case 1:
        return 'one';
    case 2:
        throw new \Exception('bad');
}"#;
        let result = try_convert(php).unwrap();
        assert!(result.contains("return match ("));
        assert!(result.contains("1 => 'one'"));
        assert!(result.contains("2 => throw new \\Exception('bad')"));
    }

    #[test]
    fn switch_in_else_branch() {
        let php = r#"<?php
if ($y) {
    foo();
} else {
    switch ($x) {
        case 1:
            return 'one';
        default:
            return 'other';
    }
}"#;
        let result = try_convert(php).unwrap();
        assert!(result.contains("return match ("));
    }

    #[test]
    fn switch_in_closure_argument() {
        let php = r#"<?php
$items = array_map(function ($x) {
    switch ($x) {
        case 1:
            return 'one';
        default:
            return 'other';
    }
}, $list);"#;
        let result = try_convert(php).unwrap();
        assert!(result.contains("return match ("));
    }
}
