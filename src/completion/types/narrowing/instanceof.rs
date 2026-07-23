//! `instanceof`-style narrowing: extraction of `instanceof`, `is_a`,
//! `get_class` / `::class` identity checks, and the compound `&&` / `||`
//! instanceof forms, plus application onto candidate class lists.

use std::sync::Arc;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::types::ClassInfo;

use mago_syntax::cst::*;

use super::super::conditional::extract_class_string_from_expr;
use crate::completion::resolver::VarResolutionCtx;

use super::*;

/// Check if `condition` is `$var instanceof ClassName` (possibly
/// parenthesised or negated) where the variable matches `ctx.var_name`.
///
/// If the cursor falls inside `body_span`:
///   - positive match → narrow `results` to only the instanceof class
///   - negated match (`!($var instanceof ClassName)`) → *exclude* the
///     class from the current candidates
pub(in crate::completion) fn try_apply_instanceof_narrowing(
    condition: &Expression<'_>,
    body_span: mago_span::Span,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    if ctx.cursor_offset < body_span.start.offset || ctx.cursor_offset > body_span.end.offset {
        return;
    }

    // ── Compound OR: `$x instanceof A || $x instanceof B` ──────────
    // Each branch that matches adds its class to the results (union).
    // This also handles untyped variables: if `results` is empty and
    // both branches match, the variable becomes `A|B`.
    //
    // We resolve all classes first and then replace `results` in one
    // shot, because `apply_instanceof_inclusion` clears results on
    // each call (correct for single-class narrowing, but wrong when
    // building a union from multiple OR branches).
    if let Some(classes) = try_extract_compound_or_instanceof(condition, ctx.var_name)
        && !classes.is_empty()
    {
        let union = resolve_class_names_to_union(&classes, ctx);
        if !union.is_empty() {
            results.clear();
            *results = union;
        }
        return;
    }

    // ── Compound AND: `$x instanceof A && $x instanceof B` ─────────
    // Both branches must hold, so each narrows further.  In practice
    // this means the variable is the intersection.  Since PHPantom
    // uses union-completion semantics, we add all matched classes.
    if let Some(classes) = try_extract_compound_and_instanceof(condition, ctx.var_name)
        && !classes.is_empty()
    {
        let union = resolve_class_names_to_union(&classes, ctx);
        if !union.is_empty() {
            results.clear();
            *results = union;
        }
        return;
    }

    if let Some(mut extraction) = try_extract_instanceof_with_negation(condition, ctx.var_name) {
        resolve_extraction_to_fqn(&mut extraction, ctx.class_loader);
        if extraction.negated {
            apply_instanceof_exclusion(&extraction.class_type, ctx, results);
        } else {
            apply_instanceof_inclusion(&extraction.class_type, extraction.exact, ctx, results);
        }
    }
}

/// Inverse of `try_apply_instanceof_narrowing` — used for the `else`
/// branch of an `if ($var instanceof ClassName)` check.
///
/// A positive instanceof in the condition means the variable is NOT
/// that class inside the else body (→ exclude), and vice-versa for a
/// negated condition (→ include only that class).
pub(in crate::completion) fn try_apply_instanceof_narrowing_inverse(
    condition: &Expression<'_>,
    body_span: mago_span::Span,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    if ctx.cursor_offset < body_span.start.offset || ctx.cursor_offset > body_span.end.offset {
        return;
    }

    // ── Compound OR inverse: after `if ($x instanceof A || $x instanceof B) { exit; }` ──
    // In the else branch, $x is neither A nor B → exclude both.
    if let Some(classes) = try_extract_compound_or_instanceof(condition, ctx.var_name)
        && !classes.is_empty()
    {
        for cls_type in &classes {
            apply_instanceof_exclusion(cls_type, ctx, results);
        }
        return;
    }

    // ── Compound AND inverse: after `if ($x instanceof A && $x instanceof B) { exit; }` ──
    // In the else branch, at least one doesn't hold.  Since we can't
    // precisely model "not (A and B)", we don't narrow.  Fall through.

    if let Some(mut extraction) = try_extract_instanceof_with_negation(condition, ctx.var_name) {
        resolve_extraction_to_fqn(&mut extraction, ctx.class_loader);
        // Flip the polarity: positive condition → exclude in else,
        // negated condition → include in else.
        if extraction.negated {
            apply_instanceof_inclusion(&extraction.class_type, extraction.exact, ctx, results);
        } else {
            apply_instanceof_exclusion(&extraction.class_type, ctx, results);
        }
    }
}

/// Replace `results` with only the resolved classes for `cls_name`.
/// Narrow `results` to include only classes matching `cls_name`.
///
/// When `exact` is `false` (the common `instanceof` / `is_a()` case),
/// existing results that are already subtypes of the narrowing class are
/// kept as-is because they are more specific and already satisfy the
/// check.  For example, if results = `[Zoo]` and we narrow to
/// `ZooBase`, `Zoo extends ZooBase` means `Zoo` is already more specific
/// so it is preserved.
///
/// When `exact` is `true` (`get_class($x) === Foo::class` or
/// `$x::class === Foo::class`), the variable is narrowed to exactly
/// that class regardless of the current results.
///
/// Always returns `true`: every path through this function reaches a
/// definite conclusion about the variable's type (including the
/// unresolvable-target case, which definitely concludes "untyped").
/// Callers feeding the result through [`ResolvedType::apply_narrowing`]
/// use this to drop leftover non-class entries (e.g. `mixed`) that the
/// instanceof check has proven cannot hold, even when the narrowed
/// class was already present in the pre-narrowing union.
pub(in crate::completion) fn apply_instanceof_inclusion(
    ty: &PhpType,
    exact: bool,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) -> bool {
    let narrowed: Vec<ClassInfo> = super::super::resolution::type_hint_to_classes_typed(
        ty,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    )
    .into_iter()
    .map(Arc::unwrap_or_clone)
    .collect();
    if narrowed.is_empty() {
        // The instanceof target class could not be resolved (e.g. it
        // lives inside a phar that we cannot index).  The developer
        // wrote an explicit instanceof guard, so they clearly expect
        // the variable to have that type in this branch.  Rather than
        // keeping the un-narrowed type (which would cause false-
        // positive "unknown member" diagnostics for members that only
        // exist on the unresolvable subclass), clear the results so
        // the variable appears untyped.  Untyped subjects are
        // suppressed by the diagnostic engine, eliminating the false
        // positives without losing any information we actually had.
        results.clear();
        return true;
    }

    // For non-exact checks (instanceof / is_a), keep existing results
    // that are already subtypes of the narrowing class.  For example,
    // if results = [Zoo] and we narrow to ZooBase, Zoo extends ZooBase
    // so Zoo is already more specific — keep it.
    if !exact {
        let already_subtypes: Vec<ClassInfo> = results
            .iter()
            .filter(|r| {
                narrowed
                    .iter()
                    .any(|n| crate::util::is_subtype_of_names(&r.fqn(), &n.fqn(), ctx.class_loader))
            })
            .cloned()
            .collect();

        if !already_subtypes.is_empty() {
            // All kept results are already subtypes of the narrowing
            // class, so the instanceof check is satisfied without
            // widening.
            *results = already_subtypes;
            return true;
        }
    }

    // When the narrowed class is a subtype of (i.e. more specific than)
    // an existing result, replace with the narrowed type.  For example,
    // results = [Animal] narrowed to Dog (Dog extends Animal) → [Dog].
    if !exact {
        let narrowed_is_more_specific = narrowed.iter().any(|n| {
            results
                .iter()
                .any(|r| crate::util::is_subtype_of_names(&n.fqn(), &r.fqn(), ctx.class_loader))
        });

        if !narrowed_is_more_specific && results.len() == 1 {
            // Neither direction holds — the types are unrelated.
            // This only makes sense as an intersection when the
            // variable has a single definite type (not a union from
            // conditional branches) and at least one side is an
            // interface, because a concrete object can implement an
            // interface without it appearing in the declared class
            // hierarchy (e.g. mock objects, dynamic proxies).
            //
            // When `results` is a union (len > 1) the instanceof
            // filters the union rather than intersecting, so we fall
            // through to the replacement path below.
            let any_interface = narrowed
                .iter()
                .chain(results.iter())
                .any(|c| c.kind == crate::types::ClassLikeKind::Interface);

            if any_interface {
                // Keep both (intersection semantics) so that members
                // from all types are available.
                for cls in narrowed {
                    if !results.iter().any(|c| c.fqn() == cls.fqn()) {
                        results.push(cls);
                    }
                }
                return true;
            }
        }
    }

    // Exact identity check, or narrowed type is more specific —
    // replace with the narrowed type.
    results.clear();
    for cls in narrowed {
        if !results.iter().any(|c| c.name == cls.name) {
            results.push(cls);
        }
    }
    true
}

/// Remove the resolved classes for `ty` from `results`.
///
/// Always returns `false`: exclusion only rules out one possibility and
/// never concludes the variable's full type, so leftover non-class
/// entries (e.g. `mixed`) that [`ResolvedType::apply_narrowing`] tracks
/// separately must survive.
pub(in crate::completion) fn apply_instanceof_exclusion(
    ty: &PhpType,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) -> bool {
    let excluded: Vec<ClassInfo> = super::super::resolution::type_hint_to_classes_typed(
        ty,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    )
    .into_iter()
    .map(Arc::unwrap_or_clone)
    .collect();
    if !excluded.is_empty() {
        results.retain(|r| !excluded.iter().any(|e| e.name == r.name));
    }
    false
}

/// If `expr` is `$var instanceof ClassName` and the variable name
/// matches `var_name`, return the class name.
///
/// Handles parenthesised expressions recursively so that
/// `($var instanceof Foo)` also works.
pub(in crate::completion) fn try_extract_instanceof<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<PhpType> {
    match expr {
        Expression::Parenthesized(inner) => try_extract_instanceof(inner.expression, var_name),
        Expression::Binary(bin) if bin.operator.is_instanceof() => {
            // LHS must be our variable or property access
            let lhs_name = expr_to_subject_key(bin.lhs)?;
            if lhs_name != var_name {
                return None;
            }
            // RHS is the class name
            match bin.rhs {
                Expression::Identifier(ident) => {
                    Some(PhpType::Named(bytes_to_str(ident.value()).to_string()))
                }
                Expression::Self_(_) => Some(PhpType::Named("self".to_string())),
                Expression::Static(_) => Some(PhpType::Named("static".to_string())),
                Expression::Parent(_) => Some(PhpType::Named("parent".to_string())),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Like `try_extract_instanceof` but also detects negation.
///
/// Returns `Some((class_name, negated))` where `negated` is `true`
/// when the expression is `!($var instanceof ClassName)` or
/// `!$var instanceof ClassName` (PHP precedence: `instanceof` binds
/// tighter than `!`, so both forms are equivalent).
///
/// Also handles:
///   - `is_a($var, ClassName::class)` — treated as equivalent to instanceof
///   - `get_class($var) === ClassName::class` or `==` — exact class match
///   - `$var::class === ClassName::class` or `==` — exact class match
///
/// Handles arbitrary parenthesisation.
/// Result of extracting an instanceof-style check from an expression.
///
/// - `class_name`: the class being checked against
/// - `negated`: `true` when the check is negated (e.g. `!($x instanceof Foo)`)
/// - `exact`: `true` for exact class identity checks (`get_class($x) === Foo::class`,
///   `$x::class === Foo::class`) where subclasses should NOT be preserved.
///   `false` for `instanceof` / `is_a()` checks where a more-specific subtype
///   in the current results should be kept.
pub(in crate::completion) struct InstanceofExtraction {
    /// The narrowed type (e.g. `PhpType::Named("ClassName".into())`).
    pub class_type: PhpType,
    pub negated: bool,
    pub exact: bool,
}

pub(in crate::completion) fn try_extract_instanceof_with_negation<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<InstanceofExtraction> {
    match expr {
        Expression::Parenthesized(inner) => {
            try_extract_instanceof_with_negation(inner.expression, var_name)
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            // `!expr` — recurse so that `!!expr` (double negation) and
            // deeper chains like `!!!expr` are handled correctly: each
            // `!` flips the negation flag.
            try_extract_instanceof_with_negation(prefix.operand, var_name).map(|mut e| {
                e.negated = !e.negated;
                e
            })
        }
        _ => {
            try_extract_instanceof(expr, var_name)
                .map(|cls_type| InstanceofExtraction {
                    class_type: cls_type,
                    negated: false,
                    exact: false,
                })
                .or_else(|| {
                    // `is_a($var, ClassName::class)` — equivalent to instanceof
                    try_extract_is_a(expr, var_name).map(|cls_type| InstanceofExtraction {
                        class_type: cls_type,
                        negated: false,
                        exact: false,
                    })
                })
                .or_else(|| {
                    // `get_class($var) === ClassName::class` or
                    // `$var::class === ClassName::class` — exact class match
                    try_extract_class_identity_check(expr, var_name).map(|(cls_type, neg)| {
                        InstanceofExtraction {
                            class_type: cls_type,
                            negated: neg,
                            exact: true,
                        }
                    })
                })
        }
    }
}

/// Detect `is_a($var, ClassName::class)` — semantically equivalent to
/// `$var instanceof ClassName`.
///
/// Returns the class name if the pattern matches.
fn try_extract_is_a<'b>(expr: &'b Expression<'b>, var_name: &str) -> Option<PhpType> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if let Expression::Call(Call::Function(func_call)) = expr {
        let func_name = match func_call.function {
            Expression::Identifier(ident) => bytes_to_str(ident.value()),
            _ => return None,
        };
        if func_name != "is_a" {
            return None;
        }
        let args: Vec<_> = func_call.argument_list.arguments.iter().collect();
        if args.len() < 2 {
            return None;
        }
        // First argument must be our variable
        let first_expr = match &args[0] {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        let first_var = match first_expr {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => return None,
        };
        if first_var != var_name {
            return None;
        }
        // Second argument should be ClassName::class
        let second_expr = match &args[1] {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        extract_class_string_from_expr(second_expr).map(PhpType::Named)
    } else {
        None
    }
}

/// Extract the unquoted value of a string literal expression.
///
/// Returns `None` for anything that is not a plain string literal
/// (interpolated strings, concatenations, variables, ...).
pub(in crate::completion) fn string_literal_value(expr: &Expression<'_>) -> Option<String> {
    use mago_syntax::cst::Literal;
    match expr {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the unquoted content; fall back to stripping
            // quotes from `raw`.
            Some(
                s.value
                    .map(|v| bytes_to_str(v).to_string())
                    .unwrap_or_else(|| {
                        let raw_str = bytes_to_str(s.raw);
                        crate::util::unquote_php_string(raw_str)
                            .unwrap_or(raw_str)
                            .to_string()
                    }),
            )
        }
        _ => None,
    }
}

/// Extract the value expression from a positional or named argument.
pub(in crate::completion) fn argument_value<'b>(arg: &'b Argument<'b>) -> &'b Expression<'b> {
    match arg {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    }
}

/// Detect `get_class($var) === ClassName::class` (or `==`) and
/// `$var::class === ClassName::class` (or `==`).
///
/// Returns `Some((class_name, negated))` where `negated` is `true`
/// for `!==` and `!=` operators.
fn try_extract_class_identity_check<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<(PhpType, bool)> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if let Expression::Binary(bin) = expr {
        let negated = match &bin.operator {
            BinaryOperator::Identical(_) | BinaryOperator::Equal(_) => false,
            BinaryOperator::NotIdentical(_) | BinaryOperator::NotEqual(_) => true,
            _ => return None,
        };
        // Try both orders: class-check == ClassName::class and
        // ClassName::class == class-check
        if let Some(cls) = match_class_identity_pair(bin.lhs, bin.rhs, var_name) {
            return Some((cls, negated));
        }
        if let Some(cls) = match_class_identity_pair(bin.rhs, bin.lhs, var_name) {
            return Some((cls, negated));
        }
    }
    None
}

/// Helper for `try_extract_class_identity_check`.
///
/// Checks if `lhs` is a class-identity expression for `var_name`
/// (`get_class($var)` or `$var::class`) and `rhs` is a
/// `ClassName::class` constant.
fn match_class_identity_pair<'b>(
    lhs: &'b Expression<'b>,
    rhs: &'b Expression<'b>,
    var_name: &str,
) -> Option<PhpType> {
    let is_class_of_var =
        is_get_class_of_var(lhs, var_name) || is_var_class_constant(lhs, var_name);
    if !is_class_of_var {
        return None;
    }
    extract_class_string_from_expr(rhs).map(PhpType::Named)
}

/// Check if `expr` is `get_class($var)` where the variable matches.
fn is_get_class_of_var(expr: &Expression<'_>, var_name: &str) -> bool {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if let Expression::Call(Call::Function(func_call)) = expr {
        let func_name = match func_call.function {
            Expression::Identifier(ident) => bytes_to_str(ident.value()),
            _ => return false,
        };
        if func_name != "get_class" {
            return false;
        }
        if let Some(first_arg) = func_call.argument_list.arguments.iter().next() {
            let arg_expr = match first_arg {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            };
            if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
                return bytes_to_str(dv.name) == var_name;
            }
        }
    }
    false
}

/// Check if `expr` is `$var::class` where the variable matches.
fn is_var_class_constant(expr: &Expression<'_>, var_name: &str) -> bool {
    if let Expression::Access(Access::ClassConstant(cca)) = expr {
        // The class part must be our variable
        if let Expression::Variable(Variable::Direct(dv)) = cca.class {
            if bytes_to_str(dv.name) != var_name {
                return false;
            }
            // The constant selector must be `class`
            if let ClassLikeConstantSelector::Identifier(ident) = &cca.constant {
                return ident.value == b"class";
            }
        }
    }
    false
}

/// If `expr` is `assert($var instanceof ClassName)` (or the negated
/// form `assert(!$var instanceof ClassName)`), narrow or exclude
/// `results` accordingly.
///
/// Unlike `if`-based narrowing which is scoped to the block body,
/// `assert()` narrows unconditionally for all subsequent code in the
/// same scope — the statement being before the cursor is already
/// guaranteed by the caller.
///
/// Returns `true` when a definite (inclusion-style) narrowing was
/// applied — see [`ResolvedType::apply_narrowing`].
pub(in crate::completion) fn try_apply_assert_instanceof_narrowing(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) -> bool {
    // ── Compound OR inside assert: `assert($x instanceof A || $x instanceof B)` ──
    if let Some(classes) = try_extract_assert_compound_or_instanceof(expr, ctx.var_name)
        && !classes.is_empty()
    {
        let union = resolve_class_names_to_union(&classes, ctx);
        if !union.is_empty() {
            results.clear();
            *results = union;
            return true;
        }
        return false;
    }

    if let Some(mut extraction) = try_extract_assert_instanceof(expr, ctx.var_name) {
        resolve_extraction_to_fqn(&mut extraction, ctx.class_loader);
        return if extraction.negated {
            apply_instanceof_exclusion(&extraction.class_type, ctx, results)
        } else {
            apply_instanceof_inclusion(&extraction.class_type, extraction.exact, ctx, results)
        };
    }
    false
}

/// If `expr` is `assert($var instanceof ClassName)` (or the negated
/// form), return `Some((class_name, negated))`.
///
/// Supports parenthesised inner expressions and the function name
/// `assert`.
fn try_extract_assert_instanceof<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<InstanceofExtraction> {
    // Unwrap parenthesised wrapper on the whole expression
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if let Expression::Call(Call::Function(func_call)) = expr {
        let func_name_raw = match func_call.function {
            Expression::Identifier(ident) => bytes_to_str(ident.value()),
            _ => return None,
        };
        let func_name = func_name_raw.strip_prefix('\\').unwrap_or(func_name_raw);
        if !func_name.eq_ignore_ascii_case("assert") {
            return None;
        }
        // The first argument should be the instanceof expression
        // (possibly negated), or is_a / class-identity check
        if let Some(first_arg) = func_call.argument_list.arguments.iter().next() {
            let arg_expr = match first_arg {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            };
            return try_extract_instanceof_with_negation(arg_expr, var_name);
        }
    }
    None
}

/// Extract compound OR instanceof class names from inside an `assert()` call.
///
/// For `assert($x instanceof A || $x instanceof B)`, returns
/// `Some(["A", "B"])`.  Returns `None` if the expression is not an
/// `assert()` call whose argument is a compound OR of instanceof checks.
fn try_extract_assert_compound_or_instanceof<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<Vec<PhpType>> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if let Expression::Call(Call::Function(func_call)) = expr {
        let func_name_raw = match func_call.function {
            Expression::Identifier(ident) => bytes_to_str(ident.value()),
            _ => return None,
        };
        let func_name = func_name_raw.strip_prefix('\\').unwrap_or(func_name_raw);
        if !func_name.eq_ignore_ascii_case("assert") {
            return None;
        }
        if let Some(first_arg) = func_call.argument_list.arguments.iter().next() {
            let arg_expr = match first_arg {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            };
            return try_extract_compound_or_instanceof(arg_expr, var_name);
        }
    }
    None
}

// ── Compound instanceof helpers ─────────────────────────────────

/// Flatten a `||` / `or` chain into its leaf operands.
///
/// Parenthesised sub-chains are unwrapped; a non-`||` expression yields a
/// single-element vec.  Used by the guard-clause narrowing to apply the
/// De Morgan inverse to each disjunct's own subject.
pub(in crate::completion) fn collect_or_operands<'b>(
    expr: &'b Expression<'b>,
) -> Vec<&'b Expression<'b>> {
    fn walk<'b>(expr: &'b Expression<'b>, out: &mut Vec<&'b Expression<'b>>) {
        match expr {
            Expression::Parenthesized(inner) => walk(inner.expression, out),
            Expression::Binary(bin)
                if matches!(
                    bin.operator,
                    BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
                ) =>
            {
                walk(bin.lhs, out);
                walk(bin.rhs, out);
            }
            _ => out.push(expr),
        }
    }
    let mut out = Vec::new();
    walk(expr, &mut out);
    out
}

/// Extract all instanceof class names from a compound `||` condition.
///
/// For `$x instanceof A || $x instanceof B || $x instanceof C`,
/// returns `Some(["A", "B", "C"])`.  Returns `None` if the expression
/// is not a chain of `||`-connected instanceof checks on `var_name`.
pub(crate) fn try_extract_compound_or_instanceof<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<Vec<PhpType>> {
    match expr {
        Expression::Parenthesized(inner) => {
            try_extract_compound_or_instanceof(inner.expression, var_name)
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
            ) =>
        {
            let mut classes = Vec::new();
            collect_or_instanceof_classes(expr, var_name, &mut classes);
            if classes.is_empty() {
                None
            } else {
                Some(classes)
            }
        }
        _ => None,
    }
}

/// Recursively walk a tree of `||` binary expressions, collecting
/// instanceof class names for `var_name`.
fn collect_or_instanceof_classes<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
    out: &mut Vec<PhpType>,
) {
    match expr {
        Expression::Parenthesized(inner) => {
            collect_or_instanceof_classes(inner.expression, var_name, out);
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
            ) =>
        {
            collect_or_instanceof_classes(bin.lhs, var_name, out);
            collect_or_instanceof_classes(bin.rhs, var_name, out);
        }
        _ => {
            if let Some(cls_type) = try_extract_instanceof(expr, var_name)
                && !out.contains(&cls_type)
            {
                out.push(cls_type);
            }
        }
    }
}

/// Extract all instanceof class names from a compound `&&` condition.
///
/// For `$x instanceof A && $x instanceof B`, returns `Some(["A", "B"])`.
/// Returns `None` if the expression is not a chain of `&&`-connected
/// instanceof checks on `var_name`.
fn try_extract_compound_and_instanceof<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<Vec<PhpType>> {
    match expr {
        Expression::Parenthesized(inner) => {
            try_extract_compound_and_instanceof(inner.expression, var_name)
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
            ) =>
        {
            let mut classes = Vec::new();
            collect_and_instanceof_classes(expr, var_name, &mut classes);
            if classes.is_empty() {
                None
            } else {
                Some(classes)
            }
        }
        _ => None,
    }
}

/// Recursively walk a tree of `&&` binary expressions, collecting
/// instanceof class names for `var_name`.
fn collect_and_instanceof_classes<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
    out: &mut Vec<PhpType>,
) {
    match expr {
        Expression::Parenthesized(inner) => {
            collect_and_instanceof_classes(inner.expression, var_name, out);
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
            ) =>
        {
            collect_and_instanceof_classes(bin.lhs, var_name, out);
            collect_and_instanceof_classes(bin.rhs, var_name, out);
        }
        _ => {
            if let Some(cls_type) = try_extract_instanceof(expr, var_name)
                && !out.contains(&cls_type)
            {
                out.push(cls_type);
            }
        }
    }
}

/// Detect a compound `&&` of negated `instanceof` checks for `var_name`.
///
/// Matches patterns like `!$x instanceof A && !$x instanceof B`.
/// Returns the list of class names when every leaf of the `&&` tree is
/// a negated instanceof for the same variable.  Returns `None` when the
/// pattern does not match.
pub(in crate::completion) fn try_extract_compound_negated_and_instanceof<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<Vec<PhpType>> {
    match expr {
        Expression::Parenthesized(inner) => {
            try_extract_compound_negated_and_instanceof(inner.expression, var_name)
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
            ) =>
        {
            let mut classes = Vec::new();
            if collect_negated_and_instanceof_classes(expr, var_name, &mut classes)
                && !classes.is_empty()
            {
                Some(classes)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Recursively walk a tree of `&&` binary expressions, collecting
/// instanceof class names from negated instanceof leaves.
///
/// Returns `true` when every leaf successfully matched `!$var instanceof Class`.
fn collect_negated_and_instanceof_classes<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
    out: &mut Vec<PhpType>,
) -> bool {
    match expr {
        Expression::Parenthesized(inner) => {
            collect_negated_and_instanceof_classes(inner.expression, var_name, out)
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
            ) =>
        {
            collect_negated_and_instanceof_classes(bin.lhs, var_name, out)
                && collect_negated_and_instanceof_classes(bin.rhs, var_name, out)
        }
        _ => {
            // Each leaf must be a negated instanceof for the target variable.
            if let Some(extraction) = try_extract_instanceof_with_negation(expr, var_name)
                && extraction.negated
            {
                if !out.contains(&extraction.class_type) {
                    out.push(extraction.class_type);
                }
                true
            } else {
                false
            }
        }
    }
}
