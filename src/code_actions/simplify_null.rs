//! Simplify with null coalescing / null-safe operator.
//!
//! Offers code actions to simplify common nullable patterns:
//!
//! - `isset($x) ? $x : $default` → `$x ?? $default`
//! - `$x !== null ? $x : $default` → `$x ?? $default`
//! - `$x === null ? $default : $x` → `$x ?? $default`
//! - `$x !== null ? $x->foo() : null` → `$x?->foo()` (PHP 8.0+)
//! - `$x !== null ? $x->foo : null` → `$x?->foo` (PHP 8.0+)
//!
//! **Code action kind:** `refactor.rewrite`.
//!
//! Only ternary expressions are handled (not if-statement patterns).

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::code_actions::cursor_context::innermost_at;
use crate::text_position::position_to_byte_offset;
use crate::types::PhpVersion;

// ─── Detection types ────────────────────────────────────────────────────────

/// A detected simplification opportunity.
enum Simplification {
    /// The ternary can be replaced with `$x ?? $default`.
    NullCoalescing {
        /// Source text of the left-hand side of `??`.
        lhs: String,
        /// Source text of the right-hand side of `??`.
        rhs: String,
    },
    /// The ternary can be replaced with `$x?->member` or `$x?->method()`.
    NullSafe {
        /// The full replacement text (e.g. `$x?->foo()` or `$x?->bar`).
        replacement: String,
    },
}

impl Backend {
    /// Collect "Simplify with null coalescing" code actions for ternary
    /// expressions at the cursor position.
    pub(crate) fn collect_simplify_null_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        let doc_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let cursor_offset = position_to_byte_offset(content, params.range.start) as u32;

        let php_version = self.php_version();

        let found =
            crate::parser::with_parsed_program(content, "simplify_null", |program, content| {
                simplifiable_ternary_at(program, cursor_offset, content, php_version)
            });
        let Some((span, simplification)) = found else {
            return;
        };

        let (title, replacement) = match &simplification {
            Simplification::NullCoalescing { lhs, rhs } => {
                ("Simplify to ??".to_string(), format!("{lhs} ?? {rhs}"))
            }
            Simplification::NullSafe { replacement } => {
                ("Simplify to ?->".to_string(), replacement.clone())
            }
        };

        out.push(crate::code_actions::rewrite_action(
            title,
            doc_uri,
            content,
            (span.start.offset as usize, span.end.offset as usize),
            replacement,
            true,
        ));
    }
}

/// The innermost ternary under `cursor` that `??` or `?->` can replace,
/// with its span.
fn simplifiable_ternary_at(
    program: &Program<'_>,
    cursor: u32,
    content: &str,
    php_version: PhpVersion,
) -> Option<(mago_span::Span, Simplification)> {
    innermost_at(program, cursor, |node| match node {
        Node::Conditional(ternary) => Some((
            ternary.span(),
            try_simplify_ternary(ternary, content, php_version)?,
        )),
        _ => None,
    })
}

// ─── Pattern matching ───────────────────────────────────────────────────────

/// Try to simplify a ternary expression into `??` or `?->`.
fn try_simplify_ternary(
    cond: &Conditional<'_>,
    content: &str,
    php_version: PhpVersion,
) -> Option<Simplification> {
    // Only handle full ternaries (not short ternary `$a ?: $b`).
    let then_expr = cond.then?;

    // Pattern 1: isset($x) ? $x : $default → $x ?? $default
    if let Some(s) = try_isset_coalescing(cond.condition, then_expr, cond.r#else, content) {
        return Some(s);
    }

    // Pattern 2: $x !== null ? $x : $default → $x ?? $default
    // Pattern 3: $x === null ? $default : $x → $x ?? $default
    // Pattern 4: $x !== null ? $x->foo() : null → $x?->foo()  (PHP 8.0+)
    // Pattern 5: $x !== null ? $x->foo : null → $x?->foo  (PHP 8.0+)
    if let Some(s) =
        try_null_comparison_simplify(cond.condition, then_expr, cond.r#else, content, php_version)
    {
        return Some(s);
    }

    None
}

/// Pattern: `isset($x) ? $x : $default` → `$x ?? $default`
///
/// Only matches when `isset()` has exactly one argument, and that
/// argument's source text matches the then-branch's source text.
fn try_isset_coalescing(
    condition: &Expression<'_>,
    then_expr: &Expression<'_>,
    else_expr: &Expression<'_>,
    content: &str,
) -> Option<Simplification> {
    let condition = crate::parser::unwrap_parens(condition);

    let isset = match condition {
        Expression::Construct(Construct::Isset(isset)) => isset,
        _ => return None,
    };

    let values: Vec<_> = isset.values.iter().collect();
    if values.len() != 1 {
        return None;
    }

    let isset_arg = values[0];
    let isset_arg_text = expr_source_text(isset_arg, content);
    let then_text = expr_source_text(then_expr, content);

    if isset_arg_text != then_text {
        return None;
    }

    let else_text = expr_source_text(else_expr, content);

    Some(Simplification::NullCoalescing {
        lhs: isset_arg_text.to_string(),
        rhs: else_text.to_string(),
    })
}

/// Patterns involving `$x !== null` / `$x === null` conditions.
fn try_null_comparison_simplify(
    condition: &Expression<'_>,
    then_expr: &Expression<'_>,
    else_expr: &Expression<'_>,
    content: &str,
    php_version: PhpVersion,
) -> Option<Simplification> {
    let condition = crate::parser::unwrap_parens(condition);

    let bin = match condition {
        Expression::Binary(bin) => bin,
        _ => return None,
    };

    let (is_not_null, subject_expr) = match &bin.operator {
        BinaryOperator::NotIdentical(_) => {
            // $x !== null → subject is whichever side is not null
            if is_null_literal(bin.rhs) {
                (true, bin.lhs)
            } else if is_null_literal(bin.lhs) {
                (true, bin.rhs)
            } else {
                return None;
            }
        }
        BinaryOperator::Identical(_) => {
            // $x === null → subject is whichever side is not null
            if is_null_literal(bin.rhs) {
                (false, bin.lhs)
            } else if is_null_literal(bin.lhs) {
                (false, bin.rhs)
            } else {
                return None;
            }
        }
        _ => return None,
    };

    let subject_text = expr_source_text(subject_expr, content);

    if is_not_null {
        // $x !== null ? THEN : ELSE
        let then_text = expr_source_text(then_expr, content);

        // Pattern: $x !== null ? $x : $default → $x ?? $default
        if then_text == subject_text {
            let else_text = expr_source_text(else_expr, content);
            return Some(Simplification::NullCoalescing {
                lhs: subject_text.to_string(),
                rhs: else_text.to_string(),
            });
        }

        // Pattern: $x !== null ? $x->foo() : null → $x?->foo()
        // Pattern: $x !== null ? $x->foo : null → $x?->foo
        if is_null_literal(else_expr)
            && php_version >= PhpVersion::new(8, 0)
            && let Some(replacement) = try_nullsafe_replacement(then_expr, subject_text, content)
        {
            return Some(Simplification::NullSafe { replacement });
        }
    } else {
        // $x === null ? THEN : ELSE
        let else_text = expr_source_text(else_expr, content);

        // Pattern: $x === null ? $default : $x → $x ?? $default
        if else_text == subject_text {
            let then_text = expr_source_text(then_expr, content);
            return Some(Simplification::NullCoalescing {
                lhs: subject_text.to_string(),
                rhs: then_text.to_string(),
            });
        }

        // Pattern: $x === null ? null : $x->foo() → $x?->foo()
        if is_null_literal(then_expr)
            && php_version >= PhpVersion::new(8, 0)
            && let Some(replacement) = try_nullsafe_replacement(else_expr, subject_text, content)
        {
            return Some(Simplification::NullSafe { replacement });
        }
    }

    None
}

/// Try to rewrite `$x->foo()` or `$x->foo` as `$x?->foo()` / `$x?->foo`,
/// given that `subject_text` is the text of `$x`.
///
/// Handles both property access (`$x->foo`) and method calls (`$x->foo()`),
/// including chained calls (`$x->foo()->bar()`), where only the `->`
/// immediately after the subject is converted to `?->`.
fn try_nullsafe_replacement(
    expr: &Expression<'_>,
    subject_text: &str,
    content: &str,
) -> Option<String> {
    match expr {
        Expression::Call(Call::Method(mc)) => {
            let object_text = expr_source_text(mc.object, content);
            if object_text == subject_text {
                let full_text = expr_source_text(expr, content);
                return Some(replace_arrow_after_subject(full_text, subject_text));
            }
            // Check for chains: $x->foo()->bar() where root object is $x
            if let Some(root) = find_root_object(mc.object, content)
                && root == subject_text
            {
                let full_text = expr_source_text(expr, content);
                return Some(replace_arrow_after_subject(full_text, subject_text));
            }
            None
        }
        Expression::Access(Access::Property(pa)) => {
            let object_text = expr_source_text(pa.object, content);
            if object_text == subject_text {
                let full_text = expr_source_text(expr, content);
                return Some(replace_arrow_after_subject(full_text, subject_text));
            }
            if let Some(root) = find_root_object(pa.object, content)
                && root == subject_text
            {
                let full_text = expr_source_text(expr, content);
                return Some(replace_arrow_after_subject(full_text, subject_text));
            }
            None
        }
        _ => None,
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Extract the source text of an expression by its span.
fn expr_source_text<'c>(expr: &Expression<'_>, content: &'c str) -> &'c str {
    let span = expr.span();
    let start = span.start.offset as usize;
    let end = span.end.offset as usize;
    if end <= content.len() {
        content[start..end].trim()
    } else {
        ""
    }
}

fn is_null_literal(expr: &Expression<'_>) -> bool {
    matches!(
        crate::parser::unwrap_parens(expr),
        Expression::Literal(Literal::Null(_))
    )
}

/// Find the root object of a member access chain.
///
/// E.g., for `$x->foo()->bar()->baz()`, returns the source text of `$x`.
fn find_root_object<'c>(expr: &Expression<'_>, content: &'c str) -> Option<&'c str> {
    match expr {
        Expression::Call(Call::Method(mc)) => Some(
            find_root_object(mc.object, content)
                .unwrap_or_else(|| expr_source_text(mc.object, content)),
        ),
        Expression::Access(Access::Property(pa)) => Some(
            find_root_object(pa.object, content)
                .unwrap_or_else(|| expr_source_text(pa.object, content)),
        ),
        Expression::Call(Call::NullSafeMethod(mc)) => Some(
            find_root_object(mc.object, content)
                .unwrap_or_else(|| expr_source_text(mc.object, content)),
        ),
        Expression::Access(Access::NullSafeProperty(pa)) => Some(
            find_root_object(pa.object, content)
                .unwrap_or_else(|| expr_source_text(pa.object, content)),
        ),
        _ => None,
    }
}

/// Replace the `->` that comes immediately after `subject` with `?->`.
///
/// For simple cases like `$x->foo()` with subject `$x`, the `->` right
/// after `$x` is converted.  For compound subjects like `$this->bar`,
/// the `->` that appears after the full subject text is converted, so
/// `$this->bar->getName()` becomes `$this->bar?->getName()` rather than
/// `$this?->bar->getName()`.
fn replace_arrow_after_subject(text: &str, subject: &str) -> String {
    if let Some(start) = text.find(subject) {
        let after_subject = start + subject.len();
        if let Some(arrow_offset) = text[after_subject..].find("->") {
            let pos = after_subject + arrow_offset;
            let mut result = String::with_capacity(text.len() + 1);
            result.push_str(&text[..pos]);
            result.push_str("?->");
            result.push_str(&text[pos + 2..]);
            return result;
        }
    }
    text.to_string()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse PHP code and find a simplification at the given byte offset.
    fn find_simplification(
        php: &str,
        offset: u32,
        php_version: PhpVersion,
    ) -> Option<(Simplification, u32, u32)> {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());

        simplifiable_ternary_at(program, offset, php, php_version)
            .map(|(span, s)| (s, span.start.offset, span.end.offset))
    }

    fn php80() -> PhpVersion {
        PhpVersion::new(8, 0)
    }

    fn php74() -> PhpVersion {
        PhpVersion::new(7, 4)
    }

    /// Helper to get the replacement text for a simplification.
    fn replacement_text(s: &Simplification) -> String {
        match s {
            Simplification::NullCoalescing { lhs, rhs } => format!("{lhs} ?? {rhs}"),
            Simplification::NullSafe { replacement } => replacement.clone(),
        }
    }

    // ── isset patterns ──────────────────────────────────────────────────

    #[test]
    fn isset_ternary_to_coalescing() {
        let php = "<?php $result = isset($x) ? $x : 'default';";
        let offset = php.find("isset").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    #[test]
    fn isset_ternary_array_access() {
        let php = "<?php $result = isset($data['key']) ? $data['key'] : null;";
        let offset = php.find("isset").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$data['key'] ?? null");
    }

    #[test]
    fn isset_ternary_no_match_when_then_differs() {
        let php = "<?php $result = isset($x) ? $y : 'default';";
        let offset = php.find("isset").unwrap() as u32;
        assert!(find_simplification(php, offset, php80()).is_none());
    }

    #[test]
    fn isset_multi_arg_not_simplified() {
        let php = "<?php $result = isset($x, $y) ? $x : 'default';";
        let offset = php.find("isset").unwrap() as u32;
        assert!(find_simplification(php, offset, php80()).is_none());
    }

    // ── $x !== null patterns ────────────────────────────────────────────

    #[test]
    fn not_identical_null_to_coalescing() {
        let php = "<?php $result = $x !== null ? $x : 'fallback';";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'fallback'");
    }

    #[test]
    fn null_not_identical_reversed_to_coalescing() {
        // null !== $x ? $x : 'fallback'
        let php = "<?php $result = null !== $x ? $x : 'fallback';";
        let offset = php.find("null !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'fallback'");
    }

    // ── $x === null patterns ────────────────────────────────────────────

    #[test]
    fn identical_null_to_coalescing() {
        let php = "<?php $result = $x === null ? 'default' : $x;";
        let offset = php.find("$x ===").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    #[test]
    fn null_identical_reversed_to_coalescing() {
        // null === $x ? 'default' : $x
        let php = "<?php $result = null === $x ? 'default' : $x;";
        let offset = php.find("null ===").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    // ── null-safe patterns ──────────────────────────────────────────────

    #[test]
    fn not_null_method_call_to_nullsafe() {
        let php = "<?php $result = $x !== null ? $x->getName() : null;";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x?->getName()");
    }

    #[test]
    fn not_null_property_access_to_nullsafe() {
        let php = "<?php $result = $x !== null ? $x->name : null;";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x?->name");
    }

    #[test]
    fn identical_null_else_method_to_nullsafe() {
        // $x === null ? null : $x->getName()
        let php = "<?php $result = $x === null ? null : $x->getName();";
        let offset = php.find("$x ===").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x?->getName()");
    }

    #[test]
    fn nullsafe_not_offered_on_php74() {
        let php = "<?php $result = $x !== null ? $x->getName() : null;";
        let offset = php.find("$x !==").unwrap() as u32;
        // On PHP 7.4, ?-> doesn't exist, so no simplification.
        assert!(find_simplification(php, offset, php74()).is_none());
    }

    #[test]
    fn coalescing_still_offered_on_php74() {
        let php = "<?php $result = $x !== null ? $x : 'default';";
        let offset = php.find("$x !==").unwrap() as u32;
        // ?? exists in PHP 7.0+, so this should still work.
        let (s, _, _) = find_simplification(php, offset, php74()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn no_simplification_for_unrelated_ternary() {
        let php = "<?php $result = $x > 0 ? 'positive' : 'negative';";
        let offset = php.find("$x >").unwrap() as u32;
        assert!(find_simplification(php, offset, php80()).is_none());
    }

    #[test]
    fn no_simplification_for_short_ternary() {
        let php = "<?php $result = $x ?: 'default';";
        let offset = php.find("$x ?:").unwrap() as u32;
        assert!(find_simplification(php, offset, php80()).is_none());
    }

    #[test]
    fn cursor_outside_ternary_no_match() {
        let php = "<?php $a = 1;\n$result = isset($x) ? $x : 'default';";
        // Cursor on `$a = 1`.
        let offset = php.find("$a").unwrap() as u32;
        assert!(find_simplification(php, offset, php80()).is_none());
    }

    #[test]
    fn nested_in_function() {
        let php = "<?php\nfunction foo() {\n    return isset($x) ? $x : 'default';\n}";
        let offset = php.find("isset").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    #[test]
    fn nested_in_class_method() {
        let php = "<?php\nclass Foo {\n    public function bar() {\n        return $x !== null ? $x : 'default';\n    }\n}";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    #[test]
    fn nested_in_arrow_function_argument() {
        let php = "<?php\n$names = array_map(fn($u) => $u !== null ? $u->name : null, $users);";
        let offset = php.find("$u !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$u?->name");
    }

    #[test]
    fn nested_in_match_arm_and_else_branch() {
        let php = "<?php\nif ($a) {\n} else {\n    $r = match ($k) { 1 => isset($x) ? $x : 0, default => 1 };\n}";
        let offset = php.find("isset").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 0");
    }

    #[test]
    fn parenthesized_condition() {
        let php = "<?php $result = ($x !== null) ? $x : 'default';";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    #[test]
    fn parenthesized_null() {
        let php = "<?php $result = $x !== (null) ? $x : 'default';";
        let offset = php.find("$x !==").unwrap() as u32;
        let (s, _, _) = find_simplification(php, offset, php80()).unwrap();
        assert_eq!(replacement_text(&s), "$x ?? 'default'");
    }

    // ── replace_first_arrow helper ──────────────────────────────────────

    #[test]
    fn replace_arrow_simple() {
        assert_eq!(replace_arrow_after_subject("$x->foo()", "$x"), "$x?->foo()");
    }

    #[test]
    fn replace_arrow_property() {
        assert_eq!(replace_arrow_after_subject("$x->name", "$x"), "$x?->name");
    }

    #[test]
    fn replace_arrow_chain() {
        assert_eq!(
            replace_arrow_after_subject("$x->foo()->bar()", "$x"),
            "$x?->foo()->bar()"
        );
    }

    #[test]
    fn replace_arrow_compound_subject() {
        assert_eq!(
            replace_arrow_after_subject("$this->bar->getName()", "$this->bar"),
            "$this->bar?->getName()"
        );
    }

    #[test]
    fn replace_arrow_no_arrow() {
        assert_eq!(replace_arrow_after_subject("$x", "$x"), "$x");
    }

    // ── is_null_literal helper ──────────────────────────────────────────

    #[test]
    fn null_literal_detected() {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php $x = null;";
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let mut found = false;
        for stmt in program.statements.iter() {
            if let Statement::Expression(expr_stmt) = stmt
                && let Expression::Assignment(assign) = &expr_stmt.expression
            {
                assert!(is_null_literal(assign.rhs));
                found = true;
            }
        }
        assert!(found, "expected to find an assignment with null RHS");
    }
}
