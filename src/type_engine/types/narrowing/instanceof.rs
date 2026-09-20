//! `instanceof`-style narrowing: extraction of `instanceof`, `is_a`,
//! `get_class` / `::class` identity checks, and the compound `&&` / `||`
//! instanceof forms, plus application onto candidate class lists.

use std::sync::Arc;

use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::php_type::{PhpType, TypeKind};
use crate::types::ClassInfo;

use mago_syntax::cst::*;

use super::super::conditional::extract_class_string_from_expr;
use crate::type_engine::resolver::VarResolutionCtx;

use super::*;

/// How the entries a narrowing call left in `results` relate to each
/// other, so that callers turning them into a `PhpType` pick the right
/// composite.
///
/// A caller that joins the entries as alternatives when they are really
/// an intersection judges the value's compatibility with a parameter
/// naming one member against the *other* member too, and rejects it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::type_engine) enum NarrowedShape {
    /// The check said nothing about this subject here (it names another
    /// variable, the cursor is outside the body it guards, or it only
    /// ruled a class out), so whatever shape `results` already had still
    /// stands.
    NotApplied,
    /// `results` holds alternatives: the value is exactly one of them.
    Union,
    /// `results` holds classes the value satisfies *simultaneously* —
    /// `apply_instanceof_inclusion` merged in a class the subject does
    /// not nominally implement (a mock or dynamic proxy that is both at
    /// once), so the entries describe one value rather than a choice.
    Intersection,
}

impl NarrowedShape {
    /// Fold this outcome into a walk-wide "the result is an
    /// intersection" flag.
    ///
    /// A walk applies many checks in source order and each one that
    /// applies replaces the previous conclusion, so only
    /// [`Self::NotApplied`] leaves the flag as it was.
    pub(in crate::type_engine) fn record(self, is_intersection: &mut bool) {
        match self {
            Self::NotApplied => {}
            Self::Union => *is_intersection = false,
            Self::Intersection => *is_intersection = true,
        }
    }
}

/// Check if `condition` is `$var instanceof ClassName` (possibly
/// parenthesised or negated) where the variable matches `ctx.var_name`.
///
/// If the cursor falls inside `body_span`:
///   - positive match → narrow `results` to only the instanceof class
///   - negated match (`!($var instanceof ClassName)`) → *exclude* the
///     class from the current candidates
pub(in crate::type_engine) fn try_apply_instanceof_narrowing(
    condition: &Expression<'_>,
    body_span: mago_span::Span,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) -> NarrowedShape {
    if ctx.cursor_offset < body_span.start.offset || ctx.cursor_offset > body_span.end.offset {
        return NarrowedShape::NotApplied;
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
        if union.is_empty() {
            return NarrowedShape::NotApplied;
        }
        results.clear();
        *results = union;
        return NarrowedShape::Union;
    }

    // ── Compound AND: `$x instanceof A && $x instanceof B` ─────────
    // Both branches must hold, so the value is every matched class at
    // once — an intersection, not a choice between them.
    if let Some(classes) = try_extract_compound_and_instanceof(condition, ctx.var_name)
        && !classes.is_empty()
    {
        let union = resolve_class_names_to_union(&classes, ctx);
        if union.is_empty() {
            return NarrowedShape::NotApplied;
        }
        let multiple = union.len() > 1;
        results.clear();
        *results = union;
        return if multiple {
            NarrowedShape::Intersection
        } else {
            NarrowedShape::Union
        };
    }

    if let Some(mut extraction) = try_extract_instanceof_with_negation(condition, ctx.var_name) {
        resolve_extraction_to_fqn(&mut extraction, ctx.class_loader);
        if extraction.negated {
            apply_instanceof_exclusion(&extraction.class_type, ctx, results);
            // Ruling one class out leaves the remaining entries relating
            // to each other exactly as they did before.
            NarrowedShape::NotApplied
        } else {
            instanceof_inclusion_shape(&extraction, ctx, results)
        }
    } else {
        NarrowedShape::NotApplied
    }
}

/// Apply a positive instanceof check and report the shape it left.
///
/// `apply_instanceof_inclusion` only grows a single starting class into
/// two entries via its "keep both" branch — every other path clears and
/// replaces — so growth is an unambiguous signal that the merge was an
/// intersection (see that function's doc comment).
fn instanceof_inclusion_shape(
    extraction: &InstanceofExtraction,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) -> NarrowedShape {
    let before = results.len();
    apply_instanceof_inclusion(&extraction.class_type, extraction.exact, ctx, results);
    if before <= 1 && results.len() > before {
        NarrowedShape::Intersection
    } else {
        NarrowedShape::Union
    }
}

/// Inverse of `try_apply_instanceof_narrowing` — used for the `else`
/// branch of an `if ($var instanceof ClassName)` check.
///
/// A positive instanceof in the condition means the variable is NOT
/// that class inside the else body (→ exclude), and vice-versa for a
/// negated condition (→ include only that class).
pub(in crate::type_engine) fn try_apply_instanceof_narrowing_inverse(
    condition: &Expression<'_>,
    body_span: mago_span::Span,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) -> NarrowedShape {
    if ctx.cursor_offset < body_span.start.offset || ctx.cursor_offset > body_span.end.offset {
        return NarrowedShape::NotApplied;
    }

    // ── Compound OR inverse: after `if ($x instanceof A || $x instanceof B) { exit; }` ──
    // In the else branch, $x is neither A nor B → exclude both.
    if let Some(classes) = try_extract_compound_or_instanceof(condition, ctx.var_name)
        && !classes.is_empty()
    {
        for cls_type in &classes {
            apply_instanceof_exclusion(cls_type, ctx, results);
        }
        return NarrowedShape::NotApplied;
    }

    // ── Compound AND inverse: after `if ($x instanceof A && $x instanceof B) { exit; }` ──
    // In the else branch, at least one doesn't hold.  Since we can't
    // precisely model "not (A and B)", we don't narrow.  Fall through.

    if let Some(mut extraction) = try_extract_instanceof_with_negation(condition, ctx.var_name) {
        resolve_extraction_to_fqn(&mut extraction, ctx.class_loader);
        // Flip the polarity: positive condition → exclude in else,
        // negated condition → include in else.
        if extraction.negated {
            instanceof_inclusion_shape(&extraction, ctx, results)
        } else {
            apply_instanceof_exclusion(&extraction.class_type, ctx, results);
            NarrowedShape::NotApplied
        }
    } else {
        NarrowedShape::NotApplied
    }
}

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
pub(in crate::type_engine) fn apply_instanceof_inclusion(
    ty: &PhpType,
    exact: bool,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) -> bool {
    let narrowed: Vec<ClassInfo> = super::resolve::resolve_narrowing_target(ty, ctx)
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
                narrowed.iter().any(|n| {
                    crate::class_lookup::is_subtype_of_names(&r.fqn(), &n.fqn(), ctx.class_loader)
                })
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
            results.iter().any(|r| {
                crate::class_lookup::is_subtype_of_names(&n.fqn(), &r.fqn(), ctx.class_loader)
            })
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
        ClassInfo::push_unique(results, cls);
    }
    true
}

/// Remove the resolved classes for `ty` from `results`.
///
/// A failed check rules out the class it names *and every subtype of it*:
/// a value that is not an `Identifier` cannot be a `VarLikeIdentifier`
/// either, so the else branch of `instanceof Identifier` must drop both.
/// Comparing names alone left the subclass behind and reported the union
/// as still holding it.
///
/// Always returns `false`: exclusion only rules out one possibility and
/// never concludes the variable's full type, so leftover non-class
/// entries (e.g. `mixed`) that [`ResolvedType::apply_narrowing`] tracks
/// separately must survive.
pub(in crate::type_engine) fn apply_instanceof_exclusion(
    ty: &PhpType,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) -> bool {
    let excluded: Vec<ClassInfo> = super::resolve::resolve_narrowing_target(ty, ctx)
        .into_iter()
        .map(Arc::unwrap_or_clone)
        .collect();
    if !excluded.is_empty() {
        results.retain(|r| {
            !excluded.iter().any(|e| {
                e.name == r.name
                    || crate::class_lookup::is_subtype_of_names(
                        &r.fqn(),
                        &e.fqn(),
                        ctx.class_loader,
                    )
            })
        });
    }
    false
}

/// If `expr` is `$var instanceof <value>` — an `instanceof` whose
/// right-hand side is a value rather than a literal class name — return
/// that right-hand side and whether the check is negated.
///
/// PHP resolves `$x instanceof $class` against whatever `$class` holds:
/// a `class-string` names the class directly, and an object stands for
/// its own class.  Which it is takes a type resolution the extractors in
/// this module have no context for, so the expression is handed back for
/// the caller to resolve.
pub(in crate::type_engine) fn try_extract_dynamic_instanceof<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<(&'b Expression<'b>, bool)> {
    match expr {
        Expression::Parenthesized(inner) => {
            try_extract_dynamic_instanceof(inner.expression, var_name)
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            try_extract_dynamic_instanceof(prefix.operand, var_name)
                .map(|(rhs, negated)| (rhs, !negated))
        }
        Expression::Binary(bin) if bin.operator.is_instanceof() => {
            if expr_to_subject_key(bin.lhs).as_deref() != Some(var_name) {
                return None;
            }
            match bin.rhs {
                // A literal class name needs no resolution, and is what
                // `try_extract_instanceof` already reads.
                Expression::Identifier(_)
                | Expression::Self_(_)
                | Expression::Static(_)
                | Expression::Parent(_) => None,
                rhs => Some((rhs, false)),
            }
        }
        _ => None,
    }
}

/// If `expr` is `$var instanceof ClassName` and the variable name
/// matches `var_name`, return the class name.
///
/// Handles parenthesised expressions recursively so that
/// `($var instanceof Foo)` also works.
pub(in crate::type_engine) fn try_extract_instanceof<'b>(
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
                    Some(PhpType::named(atom(bytes_to_str(ident.value()))))
                }
                Expression::Self_(_) => Some(PhpType::named(atom("self"))),
                Expression::Static(_) => Some(PhpType::named(atom("static"))),
                Expression::Parent(_) => Some(PhpType::named(atom("parent"))),
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
/// - `allow_string`: `true` for `is_a($x, Foo::class, true)` — the third
///   argument means the check also passes when `$x` is a `class-string<Foo>`,
///   so a string alternative in the subject's current type must survive the
///   narrowing rather than being replaced by the checked class.
#[derive(Clone)]
pub(in crate::type_engine) struct InstanceofExtraction {
    /// The narrowed type (e.g. `PhpType::named(atom("ClassName"))`).
    pub class_type: PhpType,
    pub negated: bool,
    pub exact: bool,
    pub allow_string: bool,
}

pub(in crate::type_engine) fn try_extract_instanceof_with_negation<'b>(
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
                    allow_string: false,
                })
                .or_else(|| {
                    // `is_a($var, ClassName::class)` — equivalent to instanceof
                    try_extract_is_a(expr, var_name).map(|(cls_type, allow_string)| {
                        InstanceofExtraction {
                            class_type: cls_type,
                            negated: false,
                            exact: false,
                            allow_string,
                        }
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
                            allow_string: false,
                        }
                    })
                })
        }
    }
}

/// Detect `is_a($var, ClassName::class)` — semantically equivalent to
/// `$var instanceof ClassName`.
///
/// Returns the class name and whether the third argument (`$allow_string`)
/// is literally `true`: `is_a($x, Foo::class, true)` also passes when `$x`
/// is a `class-string<Foo>`, so a string alternative on the subject must
/// survive the narrowing rather than being replaced by `Foo` alone.
fn try_extract_is_a<'b>(expr: &'b Expression<'b>, var_name: &str) -> Option<(PhpType, bool)> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if let Expression::Call(Call::Function(func_call)) = expr {
        let func_name = match func_call.function {
            Expression::Identifier(ident) => bytes_to_str(ident.value()),
            _ => return None,
        };
        if !crate::util::strip_fqn_prefix(func_name).eq_ignore_ascii_case("is_a") {
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
        if expr_to_subject_key(first_expr).as_deref() != Some(var_name) {
            return None;
        }
        // Second argument should be ClassName::class
        let second_expr = match &args[1] {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        let allow_string = args.get(2).is_some_and(|arg| argument_value(arg).is_true());
        extract_class_string_from_expr(second_expr)
            .map(|n| (PhpType::named(atom(n.as_ref())), allow_string))
    } else {
        None
    }
}

/// Extract the unquoted value of a string literal expression.
///
/// Returns `None` for anything that is not a plain string literal
/// (interpolated strings, concatenations, variables, ...).
pub(in crate::type_engine) fn string_literal_value(expr: &Expression<'_>) -> Option<String> {
    use mago_syntax::cst::Literal;
    match expr {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the unquoted content; fall back to stripping
            // quotes from `raw`.
            match s.value {
                Some(bytes) => Some(literal_bytes_to_str(bytes)?.to_string()),
                None => {
                    let raw_str = bytes_to_str(s.raw);
                    Some(
                        crate::text_scan::unquote_php_string(raw_str)
                            .unwrap_or(raw_str)
                            .to_string(),
                    )
                }
            }
        }
        _ => None,
    }
}

/// Extract the value expression from a positional or named argument.
pub(in crate::type_engine) fn argument_value<'b>(arg: &'b Argument<'b>) -> &'b Expression<'b> {
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
    if class_identity_subject_key(lhs).as_deref() != Some(var_name) {
        return None;
    }
    extract_class_string_from_expr(rhs).map(|n| PhpType::named(atom(n.as_ref())))
}

/// The subject an expression asks the runtime class of, as a narrowing
/// key: `get_class($x)` and `$x::class` both name `$x`.
///
/// The two spellings are the same question, and either side of an
/// identity comparison can hold it, so the callers that need to know
/// *which* value a `=== Foo::class` test speaks about share this one
/// answer.
pub(in crate::type_engine) fn class_identity_subject_key(expr: &Expression<'_>) -> Option<String> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    match expr {
        Expression::Call(Call::Function(func_call)) => {
            let Expression::Identifier(ident) = func_call.function else {
                return None;
            };
            if !crate::util::strip_fqn_prefix(bytes_to_str(ident.value()))
                .eq_ignore_ascii_case("get_class")
            {
                return None;
            }
            let first_arg = func_call.argument_list.arguments.iter().next()?;
            expr_to_subject_key(match first_arg {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            })
        }
        Expression::Access(Access::ClassConstant(cca)) => {
            let ClassLikeConstantSelector::Identifier(ident) = &cca.constant else {
                return None;
            };
            if ident.value != b"class" {
                return None;
            }
            expr_to_subject_key(cca.class)
        }
        _ => None,
    }
}

/// Extract the variable a `match` subject of the form `$var::class`
/// tests, so the arms' `Foo::class` conditions can narrow it.
///
/// Returns `None` for `match (true)` (narrowed from the conditions
/// themselves) and for subjects that are not a plain variable's class,
/// such as `$this->kind::class`.  The name is returned without the `$`,
/// matching how the narrowing helpers take variable names.
pub(in crate::type_engine) fn match_class_subject_var<'b>(
    subject: &'b Expression<'b>,
) -> Option<&'b str> {
    let subject = match subject {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let Expression::Access(Access::ClassConstant(cca)) = subject else {
        return None;
    };
    let ClassLikeConstantSelector::Identifier(ident) = &cca.constant else {
        return None;
    };
    if ident.value != b"class" {
        return None;
    }
    match cca.class {
        Expression::Variable(Variable::Direct(dv)) => Some(bytes_to_str(dv.name)),
        _ => None,
    }
}

/// The class a `match ($x::class)` arm condition names, if any.
///
/// Every condition on an arm names one alternative the subject may be, so
/// an arm's conditions together describe a union.
pub(in crate::type_engine) fn class_match_condition_class(
    condition: &Expression<'_>,
) -> Option<PhpType> {
    let condition = match condition {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    extract_class_string_from_expr(condition).map(|n| PhpType::named(atom(n.as_ref())))
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
/// applied — see [`ResolvedType::apply_narrowing`].  `shape` reports how
/// the surviving entries relate to each other, which is a separate
/// question from whether the narrowing was definite: an `assert()` can
/// prove a mock is both its declared class and the asserted interface at
/// once, and a caller that joins those as alternatives gets the same
/// false positives as the `if`-based path does.
pub(in crate::type_engine) fn try_apply_assert_instanceof_narrowing(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
    shape: &mut NarrowedShape,
) -> bool {
    // ── Compound OR inside assert: `assert($x instanceof A || $x instanceof B)` ──
    if let Some(classes) = try_extract_assert_compound_or_instanceof(expr, ctx.var_name)
        && !classes.is_empty()
    {
        let union = resolve_class_names_to_union(&classes, ctx);
        if !union.is_empty() {
            results.clear();
            *results = union;
            *shape = NarrowedShape::Union;
            return true;
        }
        return false;
    }

    if let Some(mut extraction) = try_extract_assert_instanceof(expr, ctx.var_name) {
        resolve_extraction_to_fqn(&mut extraction, ctx.class_loader);
        return if extraction.negated {
            apply_instanceof_exclusion(&extraction.class_type, ctx, results)
        } else {
            *shape = instanceof_inclusion_shape(&extraction, ctx, results);
            true
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
pub(in crate::type_engine) fn collect_or_operands<'b>(
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
pub(in crate::type_engine) fn try_extract_compound_negated_and_instanceof<'b>(
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

/// Narrow `ty` to the values an `instanceof`-style check keeps (or, when
/// the check is negated, the ones it rejects).
///
/// Each member of the union answers on its own: one that already names a
/// subtype of the checked class is the more specific of the two and
/// stays as it is, one the checked class is a subtype of narrows to the
/// class, and one that is unrelated (or not an object at all) cannot
/// pass and is dropped.
///
/// Returns `None` when nothing can be said — no class loader, a class
/// name that does not resolve, a `self`/`static`/`parent` reference the
/// enclosing scope alone could resolve, or a check every member already
/// satisfies.
pub(in crate::type_engine) fn narrow_type_by_instanceof(
    ty: &PhpType,
    extraction: &InstanceofExtraction,
    class_loader: GuardClassLoader<'_>,
) -> Option<PhpType> {
    let loader = class_loader?;
    let class_name = extraction.class_type.class_name()?;
    if extraction.class_type.is_self_like() || class_name.eq_ignore_ascii_case("parent") {
        return None;
    }
    loader(class_name)?;

    let members = instanceof_union_members(ty);
    let kept: Vec<PhpType> = members
        .iter()
        .filter_map(|member| {
            if extraction.negated {
                (!instanceof_rules_out(member, extraction, class_name, loader))
                    .then(|| member.clone())
            } else {
                instanceof_member(member, extraction, class_name, loader)
            }
        })
        .collect();

    if kept.is_empty() || kept == members {
        return None;
    }
    // A lone survivor is that type, not a union of one: `PhpType::union`
    // only collapses a single member when it deduplicated to get there.
    match kept.len() {
        1 => kept.into_iter().next(),
        _ => Some(PhpType::union(kept)),
    }
}

/// The alternatives a value of `ty` can take, with `?T` spelled out as
/// the `T|null` it stands for so each half is judged separately.
fn instanceof_union_members(ty: &PhpType) -> Vec<PhpType> {
    match ty.kind() {
        TypeKind::Nullable(inner) => vec![inner.clone(), PhpType::null()],
        TypeKind::Union(members) => members.to_vec(),
        _ => vec![ty.clone()],
    }
}

/// The class a union member names, whether it was written bare (`Foo`) or
/// with type arguments (`Foo<T>`).
///
/// [`PhpType::class_name`] answers only for the bare spelling, so a
/// generic member would otherwise look like one that names no class at
/// all and be replaced by the checked class, dropping its arguments.
fn member_class_name(member: &PhpType) -> Option<&str> {
    member
        .base_name()
        .filter(|n| crate::php_type::is_class_like_name(n))
}

/// Whether a negated check rules a single union member out entirely.
///
/// `!($v instanceof Foo)` rejects every instance of `Foo`, subclasses
/// included. An exact identity check (`get_class($v) !== Foo::class`)
/// rejects only `Foo` itself: a subclass's `get_class()` names the
/// subclass, so it passes the comparison and survives.
fn instanceof_rules_out(
    member: &PhpType,
    extraction: &InstanceofExtraction,
    class_name: &str,
    loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    if !extraction.exact {
        return crate::class_lookup::is_subtype_of_named(member, class_name, loader);
    }
    let Some(member_name) = member_class_name(member) else {
        return false;
    };
    // The two names may be spelled differently (short vs qualified), so
    // compare what they resolve to rather than how they were written.
    match loader(member_name) {
        Some(cls) => loader(class_name).is_some_and(|checked| cls.fqn() == checked.fqn()),
        None => {
            member_name.eq_ignore_ascii_case(class_name.strip_prefix('\\').unwrap_or(class_name))
        }
    }
}

/// What a positive `instanceof` check leaves of a single union member,
/// or `None` when the member cannot pass it.
fn instanceof_member(
    member: &PhpType,
    extraction: &InstanceofExtraction,
    class_name: &str,
    loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    if !member.is_object_like() {
        // `null`, a scalar and an array all fail `instanceof` outright.
        return None;
    }
    // A member that names no class of its own (`object`, `mixed`) is
    // whatever the check proves.
    let Some(member_name) = member_class_name(member).filter(|n| loader(n).is_some()) else {
        return Some(extraction.class_type.clone());
    };
    // An exact identity check (`get_class($v) === Foo::class`) admits the
    // class itself and nothing below it, so a member naming a subclass
    // cannot pass.
    if extraction.exact {
        return crate::class_lookup::is_subtype_of_names(class_name, member_name, loader)
            .then(|| extraction.class_type.clone());
    }
    if crate::class_lookup::is_subtype_of_named(member, class_name, loader) {
        return Some(member.clone());
    }
    crate::class_lookup::is_subtype_of_names(class_name, member_name, loader)
        .then(|| extraction.class_type.clone())
}
