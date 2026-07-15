/// Type narrowing for variable resolution.
///
/// This module contains the logic for narrowing a variable's type based on
/// runtime checks that appear before the cursor position.  Supported
/// patterns include:
///
///   - `if ($var instanceof ClassName)` — narrows inside the then-body
///   - `if (!$var instanceof ClassName)` — narrows inside the else-body
///   - `is_a($var, ClassName::class)` — equivalent to instanceof
///   - `get_class($var) === ClassName::class` — exact class identity check
///   - `$var::class === ClassName::class` — exact class identity check
///   - `assert($var instanceof ClassName)` — unconditional narrowing
///   - `@phpstan-assert` / `@psalm-assert` — custom type guard functions
///   - `match(true) { $var instanceof Foo => … }` — match-arm narrowing
///   - `$var instanceof Foo ? $var->method() : …` — ternary narrowing
///   - `$var instanceof Foo && $var->method()` — inline `&&` narrowing
///     (the RHS of `&&` sees the narrowed type from the LHS)
///   - `!$var instanceof Foo || $var->method()` — inline `||`
///     short-circuit narrowing (the RHS of `||` sees the *inverse* of
///     the LHS, so `$var` is `Foo` where the right operand executes)
///   - Guard clauses: `if (!$var instanceof Foo) { return; }` — narrows
///     after the if block when the body unconditionally exits via
///     `return`, `throw`, `continue`, or `break`.
///   - `in_array($var, $haystack, true)` — narrows `$var` to the
///     haystack's element type when the third argument is `true`.
///   - `is_array($var)` — narrows to only the array-like members of a
///     union type, preserving generic element types from PHPDoc.
///   - `is_string($var)`, `is_int($var)`, `is_bool($var)`, etc. —
///     narrows to the corresponding scalar type.
use std::sync::Arc;

use crate::atom::{Atom, bytes_to_str};
use crate::php_type::PhpType;
use crate::types::{AssertionKind, ClassInfo, ParameterInfo, ResolvedType, TypeAssertion};

use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::conditional::extract_class_string_from_expr;
use crate::completion::resolver::VarResolutionCtx;

/// Resolve the `class_type` inside an `InstanceofExtraction` to its FQN.
///
/// When the extractor returns a short class name (e.g. `Foo`), the
/// `class_loader` may know the fully-qualified name (`App\Foo`).
/// Resolving early ensures that downstream comparisons (e.g.
/// `out.contains(&cls_type)`) and `ResolvedType` hints carry the FQN
/// rather than the short name.
fn resolve_extraction_to_fqn(
    extraction: &mut InstanceofExtraction,
    class_loader: &dyn Fn(&str) -> Option<std::sync::Arc<ClassInfo>>,
) {
    if let PhpType::Named(ref name) = extraction.class_type {
        let resolved = crate::util::resolve_name_via_loader(name, class_loader);
        if resolved != *name {
            extraction.class_type = PhpType::Named(resolved);
        }
    }
}

/// Resolve a list of `PhpType` values into a deduplicated `Vec<ClassInfo>`.
///
/// This is a shared helper for the compound instanceof/assert narrowing
/// patterns that produce a union of classes from multiple branches.
pub(crate) fn resolve_class_names_to_union(
    classes: &[PhpType],
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ClassInfo> {
    let mut union = Vec::new();
    for ty in classes {
        let resolved = super::resolution::type_hint_to_classes_typed(
            ty,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        );
        for arc_cls in resolved {
            let cls = Arc::unwrap_or_clone(arc_cls);
            if !union.iter().any(|c: &ClassInfo| c.name == cls.name) {
                union.push(cls);
            }
        }
    }
    union
}

/// Convert an AST expression to a subject key string for narrowing comparison.
///
/// Handles:
/// - `$var` → `"$var"`
/// - `$this->prop` → `"$this->prop"`
/// - `$this?->prop` → `"$this->prop"` (null-safe normalised)
///
/// Returns `None` for expressions that are not supported as narrowing subjects.
pub(in crate::completion) fn expr_to_subject_key(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => Some(bytes_to_str(dv.name).to_string()),
        Expression::Access(Access::Property(pa)) => {
            let obj = expr_to_subject_key(pa.object)?;
            if let ClassLikeMemberSelector::Identifier(ident) = &pa.property {
                Some(format!("{}->{}", obj, bytes_to_str(ident.value)))
            } else {
                None
            }
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            let obj = expr_to_subject_key(pa.object)?;
            if let ClassLikeMemberSelector::Identifier(ident) = &pa.property {
                Some(format!("{}->{}", obj, bytes_to_str(ident.value)))
            } else {
                None
            }
        }
        Expression::ArrayAccess(aa) => {
            let base = expr_to_subject_key(aa.array)?;
            let key = array_access_key_as_string(aa)?;
            Some(format!("{}[\"{}\"]", base, key))
        }
        // See through parentheses so `($x instanceof Foo)` and grouped
        // subjects resolve to the same key as the bare form.
        Expression::Parenthesized(inner) => expr_to_subject_key(inner.expression),
        // Inline assignment as a subject: `($node = expr()) instanceof Foo`
        // narrows the assigned variable, so key on the assignment target.
        Expression::Assignment(assign) => expr_to_subject_key(assign.lhs),
        _ => None,
    }
}

/// Extract a literal key from an array access expression.
///
/// Returns the key string for `$a["test"]`, `$a['test']`, and `$a[0]`
/// (integer indices are stringified, matching PHP's integer/string key
/// coercion so `$a[0]` and `$a["0"]` narrow the same subject).  Returns
/// `None` for non-literal keys like `$a[$i]`.
pub(in crate::completion) fn array_access_key_as_string(
    aa: &mago_syntax::cst::ArrayAccess<'_>,
) -> Option<String> {
    use mago_syntax::cst::Literal;
    match aa.index {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the unquoted content; fall back to stripping
            // quotes from `raw`.
            let key = s
                .value
                .map(|v| bytes_to_str(v).to_string())
                .unwrap_or_else(|| {
                    let raw_str = bytes_to_str(s.raw);
                    crate::util::unquote_php_string(raw_str)
                        .unwrap_or(raw_str)
                        .to_string()
                });
            Some(key)
        }
        Expression::Literal(Literal::Integer(i)) => {
            // PHP normalises integer-like keys, so `$a[0]` narrows the
            // same subject as `$a["0"]`.  Prefer the parsed value; fall
            // back to the raw token when it overflowed.
            i.value
                .map(|v| v.to_string())
                .or_else(|| Some(bytes_to_str(i.raw).to_string()))
        }
        _ => None,
    }
}

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
    let narrowed: Vec<ClassInfo> = super::resolution::type_hint_to_classes_typed(
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
    let excluded: Vec<ClassInfo> = super::resolution::type_hint_to_classes_typed(
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

/// Detect a class-string narrowing guard on `var_name`:
///
///   - `is_a($var, ClassName::class, true)` — the `allow_string` third
///     argument lets `$var` be a class-string as well as an object, so
///     a string-typed `$var` narrows to `class-string<ClassName>`
///     rather than an instance of `ClassName`.
///   - `class_exists($var)`, `interface_exists($var)`, `enum_exists($var)`,
///     `trait_exists($var)` — confirms `$var` names *some* declared
///     class-like, narrowing a string to the generic `class-string`
///     (the target class is not known statically).
///
/// Returns `Some((target, negated))` where `target` is `Some(name)` for
/// `is_a()` with a resolvable second argument, or `None` for the generic
/// `*_exists()` forms.  `negated` is `true` when the guard is wrapped in
/// `!`.
pub(in crate::completion) fn try_extract_class_string_guard(
    expr: &Expression<'_>,
    var_name: &str,
) -> Option<(Option<String>, bool)> {
    match expr {
        Expression::Parenthesized(inner) => {
            try_extract_class_string_guard(inner.expression, var_name)
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            try_extract_class_string_guard(prefix.operand, var_name)
                .map(|(target, negated)| (target, !negated))
        }
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()),
                _ => return None,
            };
            let args: Vec<_> = func_call.argument_list.arguments.iter().collect();
            match func_name {
                "is_a" => {
                    if args.len() < 3 {
                        return None;
                    }
                    if expr_to_subject_key(argument_value(args[0])).as_deref() != Some(var_name) {
                        return None;
                    }
                    if !argument_value(args[2]).is_true() {
                        return None;
                    }
                    let target = extract_class_string_from_expr(argument_value(args[1]));
                    Some((target, false))
                }
                "class_exists" | "interface_exists" | "enum_exists" | "trait_exists" => {
                    if args.is_empty() {
                        return None;
                    }
                    if expr_to_subject_key(argument_value(args[0])).as_deref() != Some(var_name) {
                        return None;
                    }
                    Some((None, false))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Detect a member-existence guard on `var_name`:
///
///   - `property_exists($var, 'name')` — proves `$var` has a property
///     called `name` in the branch where the guard is true.  PHPStan
///     models this as an `object&hasProperty(name)` intersection.
///   - `method_exists($var, 'name')` — same for a method called `name`.
///   - `isset($var->name)` — proves `$var` has a property called `name`
///     (and that it is non-null) in the branch where the guard is true.
///     PHPStan treats this as an existence proof for the guarded access.
///
/// Only literal member names are recognised — a dynamic name proves the
/// existence of *some* member but not which one, so nothing can be added
/// to the type.
///
/// Returns `Some((member_name, is_method, negated))`; `negated` is `true`
/// when the guard is wrapped in `!`.
pub(in crate::completion) fn try_extract_member_exists_guard(
    expr: &Expression<'_>,
    var_name: &str,
) -> Option<(String, bool, bool)> {
    match expr {
        Expression::Parenthesized(inner) => {
            try_extract_member_exists_guard(inner.expression, var_name)
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            try_extract_member_exists_guard(prefix.operand, var_name)
                .map(|(name, is_method, negated)| (name, is_method, !negated))
        }
        // `isset($var->name)` proves the property exists on `$var`.  An
        // `isset()` may carry several arguments; the first whose subject
        // is `var_name` and whose member name is a literal identifier
        // proves that member.  Only direct property access on `var_name`
        // counts (a chained `$var->a->b` proves nothing about `$var`).
        Expression::Construct(Construct::Isset(isset)) => {
            for value in isset.values.iter() {
                let (object, property) = match value {
                    Expression::Access(Access::Property(pa)) => (pa.object, &pa.property),
                    Expression::Access(Access::NullSafeProperty(pa)) => (pa.object, &pa.property),
                    _ => continue,
                };
                if expr_to_subject_key(object).as_deref() != Some(var_name) {
                    continue;
                }
                if let ClassLikeMemberSelector::Identifier(ident) = property {
                    return Some((bytes_to_str(ident.value).to_string(), false, false));
                }
            }
            None
        }
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()),
                _ => return None,
            };
            let is_method = match func_name {
                "property_exists" => false,
                "method_exists" => true,
                _ => return None,
            };
            let args: Vec<_> = func_call.argument_list.arguments.iter().collect();
            if args.len() < 2 {
                return None;
            }
            if expr_to_subject_key(argument_value(args[0])).as_deref() != Some(var_name) {
                return None;
            }
            let member = string_literal_value(argument_value(args[1]))?;
            Some((member, is_method, false))
        }
        _ => None,
    }
}

/// Extract the unquoted value of a string literal expression.
///
/// Returns `None` for anything that is not a plain string literal
/// (interpolated strings, concatenations, variables, ...).
fn string_literal_value(expr: &Expression<'_>) -> Option<String> {
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
fn argument_value<'b>(arg: &'b Argument<'b>) -> &'b Expression<'b> {
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

/// Resolved assertion metadata extracted from a function call or static
/// method call expression.
///
/// Produced by [`extract_call_assertions`] so that callers can apply
/// narrowing logic uniformly regardless of whether the call is
/// `myFunc($x)` or `Assert::check($x)`.
struct CallAssertionInfo<'a> {
    /// The `@phpstan-assert` / `@psalm-assert` annotations on the callee.
    assertions: &'a [TypeAssertion],
    /// The callee's parameter list (used to map assertion `$param` names
    /// to positional argument indices).
    parameters: &'a [ParameterInfo],
    /// The call-site argument list.
    argument_list: &'a ArgumentList<'a>,
    /// Template parameter names from the callee's `@template` tags.
    template_params: &'a [Atom],
    /// Template parameter → parameter name bindings (e.g. `("T", "$class")`).
    template_bindings: &'a [(Atom, Atom)],
}

/// Try to extract assertion metadata from a call expression.
///
/// Handles two call forms:
///   - `Call::Function(func_call)` — standalone function call, resolved
///     through `ctx.function_loader`.
///   - `Call::StaticMethod(static_call)` — static method call like
///     `Assert::instanceOf(…)`, resolved through `ctx.class_loader`.
///
/// Returns `None` when the call is not one of these forms, or when the
/// callee cannot be resolved.
fn extract_call_assertions<'a>(
    call: &'a Call<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<CallAssertionInfo<'a>> {
    match call {
        Call::Function(func_call) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return None,
            };
            let func_info = ctx.function_loader()?(&func_name)?;
            if func_info.type_assertions.is_empty() {
                return None;
            }
            // SAFETY: We leak the FunctionInfo to get a stable reference.
            // This is acceptable because narrowing runs once per completion
            // request and the allocation is small.
            let func_info = Box::leak(Box::new(func_info));
            Some(CallAssertionInfo {
                assertions: &func_info.type_assertions,
                parameters: &func_info.parameters,
                argument_list: &func_call.argument_list,
                template_params: &func_info.template_params,
                template_bindings: &func_info.template_bindings,
            })
        }
        Call::StaticMethod(static_call) => {
            let method_name = match &static_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
                _ => return None,
            };
            let class_info = resolve_static_receiver_class(static_call.class, ctx)?;
            build_method_assertion_info(&class_info, method_name, &static_call.argument_list, ctx)
        }
        Call::Method(method_call) => {
            let method_name = match &method_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
                _ => return None,
            };
            let class_info = resolve_instance_receiver_class(method_call.object, ctx)?;
            build_method_assertion_info(&class_info, method_name, &method_call.argument_list, ctx)
        }
        Call::NullSafeMethod(method_call) => {
            let method_name = match &method_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
                _ => return None,
            };
            let class_info = resolve_instance_receiver_class(method_call.object, ctx)?;
            build_method_assertion_info(&class_info, method_name, &method_call.argument_list, ctx)
        }
    }
}

/// Resolve the receiver class of a static method call (the `X` in
/// `X::method()`) to a loaded [`ClassInfo`].
///
/// Handles class-name identifiers (including subclass names), `self`,
/// `static`, and `parent`.  The returned class is the raw parsed class;
/// callers resolve inheritance separately so that methods declared on an
/// ancestor (e.g. PHPUnit's `Assert::assertInstanceOf`) are found.
fn resolve_static_receiver_class(
    class_expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    match class_expr {
        Expression::Identifier(ident) => {
            let name = bytes_to_str(ident.value());
            let fqn = crate::util::resolve_name_via_loader(name, ctx.class_loader);
            (ctx.class_loader)(&fqn).or_else(|| (ctx.class_loader)(name))
        }
        Expression::Self_(_) | Expression::Static(_) => (ctx.class_loader)(&ctx.current_class.name),
        Expression::Parent(_) => {
            let parent = ctx.current_class.parent_class.as_ref()?;
            (ctx.class_loader)(parent)
        }
        _ => None,
    }
}

/// Resolve the receiver class of an instance method call (the `$x` in
/// `$x->method()`) to a loaded [`ClassInfo`].
///
/// `$this` resolves to the enclosing class.  Other variables are resolved
/// through the forward walker's scope so that, for example,
/// `$test->assertInstanceOf(...)` narrows correctly.
fn resolve_instance_receiver_class(
    object_expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    let Expression::Variable(Variable::Direct(dv)) = object_expr else {
        return None;
    };
    // Variable names carry the leading `$` (e.g. `$this`, `$obj`).
    let name = bytes_to_str(dv.name);
    if name == "$this" {
        return (ctx.class_loader)(&ctx.current_class.name);
    }
    let resolver = ctx.scope_var_resolver?;
    let first = resolver(name).into_iter().next()?;
    (ctx.class_loader)(&first.type_string.to_string())
}

/// Build [`CallAssertionInfo`] for a method call once the receiver class
/// has been resolved.
///
/// Walks the receiver's trait and parent chain (using raw class loads) so
/// that assertion annotations declared on an ancestor are found — e.g.
/// PHPUnit's `assertInstanceOf`, declared on the base `Assert` class and
/// called through a `TestCase` subclass.  Returns `None` when no
/// reachable definition of the method carries assertions.
///
/// A full inheritance merge is deliberately avoided here: this runs inside
/// the forward walker while the enclosing class may itself be mid-resolution,
/// and `resolve_class_fully` would write a partial result into the shared
/// resolved-class cache, corrupting later member lookups.
fn build_method_assertion_info<'a>(
    class: &ClassInfo,
    method_name: &str,
    argument_list: &'a ArgumentList<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<CallAssertionInfo<'a>> {
    let method =
        find_assertion_method_in_chain(class, method_name, ctx.class_loader, &mut Vec::new(), 0)?;
    // Leak MethodInfo to get a stable reference for the duration of this
    // narrowing pass.
    let method = Box::leak(Box::new(method));
    Some(CallAssertionInfo {
        assertions: &method.type_assertions,
        parameters: &method.parameters,
        argument_list,
        template_params: &method.template_params,
        template_bindings: &method.template_bindings,
    })
}

/// Find the definition of `method_name` that carries `@phpstan-assert`
/// metadata, searching the class's own methods, its traits, and its parent
/// chain (in PHP resolution order).  Uses raw class loads only, so it never
/// mutates the shared resolved-class cache.
///
/// Returns an owned clone of the first matching method that has non-empty
/// `type_assertions`.  A `visited` set and `depth` bound guard against
/// cyclic hierarchies.
pub(in crate::completion) fn find_assertion_method_in_chain(
    class: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    visited: &mut Vec<Atom>,
    depth: usize,
) -> Option<crate::types::MethodInfo> {
    if depth > 15 {
        return None;
    }
    let fqn = class.fqn();
    if visited.contains(&fqn) {
        return None;
    }
    visited.push(fqn);

    // Own methods first: the most-derived definition wins.  A derived
    // override with its own assertions takes precedence; an override with
    // no docblock falls through so an ancestor's assertions can apply
    // (matching how inheritance propagates assertion metadata).
    if let Some(method) = class
        .methods
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(method_name))
        && !method.type_assertions.is_empty()
    {
        return Some(method.as_ref().clone());
    }

    // Traits mixed into this class.
    for trait_name in &class.used_traits {
        if let Some(trait_class) = class_loader(trait_name)
            && let Some(method) = find_assertion_method_in_chain(
                &trait_class,
                method_name,
                class_loader,
                visited,
                depth + 1,
            )
        {
            return Some(method);
        }
    }

    // Parent class chain.
    if let Some(parent) = class.parent_class.as_ref()
        && let Some(parent_class) = class_loader(parent)
        && let Some(method) = find_assertion_method_in_chain(
            &parent_class,
            method_name,
            class_loader,
            visited,
            depth + 1,
        )
    {
        return Some(method);
    }

    None
}

/// Apply narrowing from `@phpstan-assert` / `@psalm-assert` annotations
/// on a function or static method called as a standalone expression statement.
///
/// Only `AssertionKind::Always` assertions are applied here — the
/// `IfTrue` / `IfFalse` variants are handled by
/// `try_apply_assert_condition_narrowing`.
///
/// Map a bare scalar / pseudo-type to the type-guard kind that narrows it.
///
/// So `@phpstan-assert string $x` (PHPUnit's `assertIsString`) narrows like
/// `is_string($x)`, and its negation excludes `string`.  Returns `None` for
/// class names and for pseudo-types without a corresponding guard —
/// `iterable`, `resource`, and `null` (the last handled separately by the
/// not-null path) — so those fall through to the class-based narrowing.
fn scalar_assert_guard_kind(ty: &PhpType) -> Option<TypeGuardKind> {
    match ty {
        PhpType::Array(_) | PhpType::ArrayShape(_) => Some(TypeGuardKind::Array),
        PhpType::Generic(name, _) if crate::php_type::is_array_like_name(name) => {
            // `iterable` is array-like by name but has no `is_iterable` guard
            // kind, so it must not map to the array guard.
            (!name.eq_ignore_ascii_case("iterable")).then_some(TypeGuardKind::Array)
        }
        PhpType::Named(n) => match n.to_ascii_lowercase().as_str() {
            "array" | "list" | "non-empty-array" | "non-empty-list" => Some(TypeGuardKind::Array),
            "string" => Some(TypeGuardKind::String),
            "int" | "integer" => Some(TypeGuardKind::Int),
            "float" | "double" => Some(TypeGuardKind::Float),
            "bool" | "boolean" => Some(TypeGuardKind::Bool),
            "object" => Some(TypeGuardKind::Object),
            "numeric" => Some(TypeGuardKind::Numeric),
            "callable" => Some(TypeGuardKind::Callable),
            "scalar" => Some(TypeGuardKind::Scalar),
            _ => None,
        },
        _ => None,
    }
}

/// Scalar and pseudo-type assertions (PHPUnit's `assertIsString`,
/// `assertIsObject`, `assertIsArray`, and their negations) name no class, so
/// they cannot be narrowed through `apply_instanceof_*`.  When one is
/// detected, `*type_guard` is set to `(kind, exclude)` and the caller applies
/// [`apply_type_guard_inclusion`] / [`apply_type_guard_exclusion`] on the full
/// resolved types instead, matching how the corresponding `is_*()` guard
/// narrows.  The same channel carries the `object` fallback for a template
/// assertion whose bound `class-string` argument could not be resolved
/// (e.g. `assertInstanceOf($variableClass, $x)`): the subject is still known
/// to be an object, so it is narrowed to `object` rather than cleared.
///
/// Returns `true` when a definite (inclusion-style) narrowing was
/// applied to `results` — see [`ResolvedType::apply_narrowing`]. The
/// scalar/pseudo-type and template-deferral branches signal through
/// `type_guard` instead and do not affect `results` here, so they
/// contribute `false`.
pub(in crate::completion) fn try_apply_custom_assert_narrowing(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
    type_guard: &mut Option<(TypeGuardKind, bool)>,
) -> bool {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let call = match expr {
        Expression::Call(c) => c,
        _ => return false,
    };
    let info = match extract_call_assertions(call, ctx) {
        Some(info) => info,
        None => return false,
    };
    let mut definite = false;
    for assertion in info.assertions {
        if assertion.kind != AssertionKind::Always {
            continue;
        }
        if let Some(arg_var) =
            find_assertion_arg_variable(info.argument_list, &assertion.param_name, info.parameters)
            && arg_var == ctx.var_name
        {
            // Resolve the asserted type.  When the type is a template
            // parameter (e.g. `ExpectedType` from `@phpstan-assert
            // ExpectedType $actual`), substitute it using the call-site
            // argument bound via `class-string<T>`.
            let effective_type =
                resolve_assertion_template_type(&assertion.asserted_type, &info, ctx);

            // The substitution failed when the effective type is still a
            // template parameter — the bound `class-string` argument was a
            // variable whose concrete class could not be determined.  A
            // positive assertion still guarantees the subject is an object,
            // so defer to the caller's `object` narrowing instead of
            // clearing the subject's prior type.
            if !assertion.negated
                && matches!(&effective_type, PhpType::Named(n) if info.template_params.iter().any(|t| t == n))
            {
                *type_guard = Some((TypeGuardKind::Object, false));
                continue;
            }

            // Scalar / pseudo-type assertions (`assertIsString`,
            // `assertIsObject`, `assertIsArray`, and their `assertIsNot*`
            // negations) are type guards, not class narrowings.  The named
            // pseudo-type resolves to no class, so `apply_instanceof_inclusion`
            // would clear the subject and `apply_instanceof_exclusion` would
            // exclude nothing.  Route them through the type-guard machinery.
            if let Some(kind) = scalar_assert_guard_kind(&effective_type) {
                *type_guard = Some((kind, assertion.negated));
                continue;
            }

            if assertion.negated {
                apply_instanceof_exclusion(&effective_type, ctx, results);
            } else {
                definite |= apply_instanceof_inclusion(&effective_type, false, ctx, results);
            }
        }
    }
    definite
}

/// Collect argument expressions that an assert-style call proves to be
/// `true` or `false` by re-exporting an inner condition.
///
/// PHPUnit's `assertTrue()` carries `@phpstan-assert true $condition` and
/// `assertFalse()` carries `@phpstan-assert false $condition` (the
/// `@psalm-assert` spelling is treated identically).  When the matching
/// argument is itself a boolean condition expression (e.g.
/// `property_exists($model, 'value')`), asserting that it is `true` /
/// `false` is equivalent to entering an `if` guarded by that condition.
///
/// Returns each such argument expression paired with the polarity the
/// assertion proves: `true` means the expression is proven true (apply
/// truthy condition narrowing), `false` means proven false (apply the
/// inverse).  The caller feeds each expression into the standard
/// condition-narrowing pipeline so every guard form (`instanceof`,
/// `is_*`, `property_exists`, null checks, …) is honoured uniformly.
pub(in crate::completion) fn collect_assert_reexport_conditions<'a>(
    expr: &'a Expression<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<(&'a Expression<'a>, bool)> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let Expression::Call(call) = expr else {
        return Vec::new();
    };
    let Some(info) = extract_call_assertions(call, ctx) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for assertion in info.assertions {
        if assertion.kind != AssertionKind::Always {
            continue;
        }
        // Only a bare `true` / `false` literal assertion re-exports a
        // condition.  `@phpstan-assert true $c` (negated `!true` ⇒ false)
        // proves the argument true; `@phpstan-assert false $c` proves it
        // false.
        let asserts_true = if assertion.asserted_type.is_true() {
            !assertion.negated
        } else if assertion.asserted_type.is_false() {
            assertion.negated
        } else {
            continue;
        };
        if let Some(arg_expr) =
            assertion_arg_expression(info.argument_list, &assertion.param_name, info.parameters)
        {
            out.push((arg_expr, asserts_true));
        }
    }
    out
}

/// Return the call-site argument expression bound to `param_name`.
///
/// Unlike [`find_assertion_arg_variable`], which reduces the argument to a
/// subject key (and so discards non-subject expressions like nested
/// calls), this returns the raw expression so the caller can treat it as a
/// re-exported condition.
fn assertion_arg_expression<'a>(
    argument_list: &'a ArgumentList<'a>,
    param_name: &str,
    parameters: &[crate::types::ParameterInfo],
) -> Option<&'a Expression<'a>> {
    let param_idx = parameters.iter().position(|p| p.name == param_name)?;
    let arg = argument_list.arguments.iter().nth(param_idx)?;
    Some(match arg {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    })
}

/// Report whether a call expression carries an unconditional not-null
/// assertion (`@phpstan-assert !null $param`, e.g. PHPUnit's
/// `assertNotNull`) whose argument resolves to `ctx.var_name`.
///
/// The class-based [`apply_instanceof_exclusion`] cannot remove the `null`
/// pseudo-type (it isn't a class), so callers use this to strip `null` from
/// a subject's [`ResolvedType`] list directly.  Returns `true` when such an
/// assertion applies to the current subject.
pub(in crate::completion) fn call_asserts_not_null(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> bool {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let Expression::Call(call) = expr else {
        return false;
    };
    let Some(info) = extract_call_assertions(call, ctx) else {
        return false;
    };
    info.assertions.iter().any(|assertion| {
        assertion.kind == AssertionKind::Always
            && assertion.negated
            && assertion.asserted_type.is_null()
            && find_assertion_arg_variable(
                info.argument_list,
                &assertion.param_name,
                info.parameters,
            )
            .as_deref()
                == Some(ctx.var_name)
    })
}

/// If `asserted_type` is a template parameter name, resolve it to a
/// concrete type using the call-site arguments and template bindings.
///
/// For example, given:
///   `@template ExpectedType of object`
///   `@param class-string<ExpectedType> $expected`
///   `@phpstan-assert ExpectedType $actual`
///   Call: `Assert::assertFoobar(Foobar::class, $obj)`
///
/// The asserted type `ExpectedType` is resolved to `Foobar` by:
///   1. Finding `ExpectedType` in `template_params`
///   2. Looking up its binding: `("ExpectedType", "$expected")`
///   3. Finding positional index of `$expected` in `parameters`
///   4. Reading the call-site argument at that index: `Foobar::class`
///   5. Extracting the class name `Foobar`
///
/// Returns the original type unchanged when it is not a template param
/// or when the concrete type cannot be determined.
fn resolve_assertion_template_type(
    asserted_type: &PhpType,
    info: &CallAssertionInfo<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> PhpType {
    // Check if the asserted type is a template parameter.
    let tpl_name = match asserted_type {
        PhpType::Named(n) if info.template_params.iter().any(|t| t == n) => n.as_str(),
        _ => return asserted_type.clone(),
    };

    // Find the parameter name that binds this template param.
    let bound_param = info
        .template_bindings
        .iter()
        .find(|(tpl, _)| tpl == tpl_name)
        .map(|(_, param)| param.as_str());

    let bound_param = match bound_param {
        Some(p) => p,
        None => return asserted_type.clone(),
    };

    // Find the positional index of that parameter.
    let param_idx = match info.parameters.iter().position(|p| p.name == bound_param) {
        Some(idx) => idx,
        None => return asserted_type.clone(),
    };

    // Get the call-site argument at that position.
    let arg_expr = match info.argument_list.arguments.iter().nth(param_idx) {
        Some(Argument::Positional(pos)) => pos.value,
        Some(Argument::Named(named)) => named.value,
        None => return asserted_type.clone(),
    };

    // Try to extract a class name from the argument expression.
    if let Some(class_name) = extract_class_string_from_expr(arg_expr) {
        let fqn = crate::util::resolve_name_via_loader(&class_name, ctx.class_loader);
        return PhpType::Named(fqn);
    }

    if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
        let var_name = bytes_to_str(dv.name).to_string();

        // Prefer the shared forward walker's tracked type for the variable.
        // When the walker is driving this narrowing it has already processed
        // the statements leading up to the assert, so a variable holding a
        // `class-string<Wanted>` value (whether assigned directly, via
        // null-coalesce, or list-destructured out of a foreach source array)
        // is in scope with that type.  Reusing it keeps class-string-value
        // resolution on the single shared pipeline instead of a parallel
        // special-purpose walk that only recognizes direct assignments.
        if let Some(scope_resolver) = ctx.scope_var_resolver {
            for resolved in scope_resolver(&var_name) {
                if let Some(PhpType::Named(name)) = resolved.type_string.unwrap_class_string_inner()
                {
                    return PhpType::Named(name.clone());
                }
            }
        }

        // Fall back to the class-string resolver for consumers without a live
        // forward-walk scope (e.g. a completion request resolving the subject
        // directly).  Resolve it at the argument's own offset rather than
        // `ctx.cursor_offset`: the latter is `u32::MAX` during whole-method
        // diagnostics walks, which defeats the class-body detection in
        // `resolve_class_string_targets` (its `cursor <= class_end` bound never
        // holds), and using the call site is more precise anyway (a later
        // reassignment of the variable must not fold back into the assertion).
        let targets =
            crate::completion::variable::class_string_resolution::resolve_class_string_targets(
                &var_name,
                ctx.current_class,
                ctx.all_classes,
                ctx.content,
                arg_expr.span().start.offset,
                ctx.class_loader,
            );
        if let Some(first) = targets.into_iter().next() {
            return PhpType::Named(first.name.to_string());
        }
    }

    asserted_type.clone()
}

/// Unwrap parentheses and a single `!` prefix from a condition,
/// returning `(inner_expr, negated)`.
pub(in crate::completion) fn unwrap_condition_negation<'b>(
    expr: &'b Expression<'b>,
) -> (&'b Expression<'b>, bool) {
    match expr {
        Expression::Parenthesized(inner) => unwrap_condition_negation(inner.expression),
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            let (inner, already_negated) = unwrap_condition_negation(prefix.operand);
            (inner, !already_negated)
        }
        _ => (expr, false),
    }
}

/// Given a function's argument list and a parameter name (with `$`
/// prefix), find the subject key passed at that parameter's position.
///
/// Returns the subject key for a direct variable (`$var`), a property
/// path (`$arg->value`), or an array access (`$stmts["0"]`) so that
/// assertion narrowing applies to non-variable subjects, not just plain
/// variables.
pub(in crate::completion) fn find_assertion_arg_variable(
    argument_list: &ArgumentList<'_>,
    param_name: &str,
    parameters: &[crate::types::ParameterInfo],
) -> Option<String> {
    // Find the parameter index
    let param_idx = parameters.iter().position(|p| p.name == param_name)?;

    // Get the argument at that position
    let arg = argument_list.arguments.iter().nth(param_idx)?;
    let arg_expr = match arg {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };

    expr_to_subject_key(arg_expr)
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

/// Check whether a statement unconditionally exits the current scope.
///
/// A statement unconditionally exits if every code path through it
/// ends with `return`, `throw`, `continue`, or `break`.  This is used
/// to detect guard clause patterns like:
///
/// ```text
/// if (!$var instanceof Foo) {
///     return;
/// }
/// // $var is Foo here
/// ```
pub(in crate::completion) fn statement_unconditionally_exits(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Return(_) => true,
        Statement::Continue(_) => true,
        Statement::Break(_) => true,
        // `throw new …;` is parsed as an expression statement
        // containing a Throw expression.
        Statement::Expression(es) => matches!(
            es.expression,
            Expression::Throw(_)
                | Expression::Construct(mago_syntax::cst::Construct::Exit(_))
                | Expression::Construct(mago_syntax::cst::Construct::Die(_))
        ),
        // A block exits if its last statement exits.
        Statement::Block(block) => block
            .statements
            .last()
            .is_some_and(statement_unconditionally_exits),
        // An if/else exits if ALL branches exist and ALL exit.
        Statement::If(if_stmt) => if_body_unconditionally_exits(&if_stmt.body),
        _ => false,
    }
}

/// Check whether an `if` body (including all branches) unconditionally
/// exits.  This requires:
///   - The then-body exits, AND
///   - All elseif bodies exit, AND
///   - An else clause exists and exits.
fn if_body_unconditionally_exits(body: &IfBody<'_>) -> bool {
    match body {
        IfBody::Statement(stmt_body) => {
            // Then-body must exit
            if !statement_unconditionally_exits(stmt_body.statement) {
                return false;
            }
            // All elseif bodies must exit
            if !stmt_body
                .else_if_clauses
                .iter()
                .all(|ei| statement_unconditionally_exits(ei.statement))
            {
                return false;
            }
            // Else must exist and exit
            stmt_body
                .else_clause
                .as_ref()
                .is_some_and(|ec| statement_unconditionally_exits(ec.statement))
        }
        IfBody::ColonDelimited(colon_body) => {
            // Then-body: last statement must exit
            if !colon_body
                .statements
                .last()
                .is_some_and(statement_unconditionally_exits)
            {
                return false;
            }
            // All elseif bodies must exit
            if !colon_body.else_if_clauses.iter().all(|ei| {
                ei.statements
                    .last()
                    .is_some_and(statement_unconditionally_exits)
            }) {
                return false;
            }
            // Else must exist and exit
            colon_body.else_clause.as_ref().is_some_and(|ec| {
                ec.statements
                    .last()
                    .is_some_and(statement_unconditionally_exits)
            })
        }
    }
}

/// Check whether an `if` body's then-branch unconditionally exits.
/// Used for guard clause detection where we only need the then-body
/// to exit (no else clause required).
fn then_body_unconditionally_exits(body: &IfBody<'_>) -> bool {
    match body {
        IfBody::Statement(stmt_body) => statement_unconditionally_exits(stmt_body.statement),
        IfBody::ColonDelimited(colon_body) => colon_body
            .statements
            .last()
            .is_some_and(statement_unconditionally_exits),
    }
}

/// Apply guard clause narrowing after an `if` statement whose
/// then-body unconditionally exits (return/throw/continue/break)
/// and which has no else/elseif clauses.
///
/// When a guard clause like:
/// ```text
/// if (!$var instanceof Foo) { return; }
/// ```
/// appears before the cursor, the code after it can only be reached
/// when the condition was *false* — so we apply the inverse narrowing.
///
/// This handles:
///   - `instanceof` / `is_a()` / `get_class()` / `::class` checks
///   - `@phpstan-assert-if-true` / `@phpstan-assert-if-false` guards
pub(in crate::completion) fn apply_guard_clause_narrowing(
    if_stmt: &If<'_>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    // Only applies when the then-body exits and there are no
    // elseif/else branches (simple guard clause pattern).
    if !then_body_unconditionally_exits(&if_stmt.body) {
        return;
    }
    if if_stmt.body.has_else_clause() || if_stmt.body.has_else_if_clauses() {
        return;
    }

    // ── Compound OR guard clause ────────────────────────────────────
    // `if ($x instanceof A || $x instanceof B) { return; }`
    // After the if, $x is neither A nor B → exclude both.
    if let Some(classes) = try_extract_compound_or_instanceof(if_stmt.condition, ctx.var_name)
        && !classes.is_empty()
    {
        for cls_type in &classes {
            apply_instanceof_exclusion(cls_type, ctx, results);
        }
        return;
    }

    // ── Compound negated AND guard clause ───────────────────────────
    // `if (!$x instanceof A && !$x instanceof B) { return; }`
    // The then-body exits when $x is neither A nor B.  After the if,
    // the condition was false, so $x IS instanceof A or B → include both.
    if let Some(classes) =
        try_extract_compound_negated_and_instanceof(if_stmt.condition, ctx.var_name)
        && !classes.is_empty()
    {
        let union = resolve_class_names_to_union(&classes, ctx);
        if !union.is_empty() {
            results.clear();
            *results = union;
        }
        return;
    }

    // ── Heterogeneous OR guard clause ───────────────────────────────
    // `if (!$a instanceof A || !$a->b instanceof B) { return; }`
    // De Morgan: after the guard every disjunct's negation holds, so
    // each disjunct narrows its own subject.  Apply the guard-inverse
    // for whichever disjunct is an instanceof on the current subject
    // (`ctx.var_name`).  This complements the same-subject compound OR
    // handler above, which returns early when it matches.
    {
        let operands = collect_or_operands(if_stmt.condition);
        if operands.len() > 1 {
            let mut narrowed = false;
            for operand in &operands {
                if let Some(mut extraction) =
                    try_extract_instanceof_with_negation(operand, ctx.var_name)
                {
                    resolve_extraction_to_fqn(&mut extraction, ctx.class_loader);
                    // Positive disjunct → excluded after the guard;
                    // negated disjunct → included after the guard.
                    if extraction.negated {
                        apply_instanceof_inclusion(
                            &extraction.class_type,
                            extraction.exact,
                            ctx,
                            results,
                        );
                    } else {
                        apply_instanceof_exclusion(&extraction.class_type, ctx, results);
                    }
                    narrowed = true;
                }
            }
            if narrowed {
                return;
            }
        }
    }

    // ── instanceof / is_a / get_class / ::class narrowing ──
    // The then-body exits, so subsequent code is the "else" — apply
    // the inverse of the condition.
    if let Some(mut extraction) =
        try_extract_instanceof_with_negation(if_stmt.condition, ctx.var_name)
    {
        resolve_extraction_to_fqn(&mut extraction, ctx.class_loader);
        // Positive instanceof + exit → exclude after (var is NOT that class)
        // Negated instanceof + exit → include after (var IS that class)
        if extraction.negated {
            apply_instanceof_inclusion(&extraction.class_type, extraction.exact, ctx, results);
        } else {
            apply_instanceof_exclusion(&extraction.class_type, ctx, results);
        }
    }

    // ── @phpstan-assert-if-true / @phpstan-assert-if-false ──
    // When a function or static method with assert-if-true/false is the
    // condition and the then-body exits, the code after runs when the
    // callee returned the opposite boolean — apply the inverse narrowing.
    let (func_call_expr, condition_negated) = unwrap_condition_negation(if_stmt.condition);

    if let Expression::Call(call) = func_call_expr
        && let Some(info) = extract_call_assertions(call, ctx)
    {
        // The then-body exits, so we're in the "else" conceptually.
        // inverted=true, same logic as try_apply_assert_condition_narrowing
        let function_returned_true = condition_negated;

        for assertion in info.assertions {
            let applies_positively = match assertion.kind {
                AssertionKind::IfTrue => function_returned_true,
                AssertionKind::IfFalse => !function_returned_true,
                AssertionKind::Always => continue,
            };

            if let Some(arg_var) = find_assertion_arg_variable(
                info.argument_list,
                &assertion.param_name,
                info.parameters,
            ) && arg_var == ctx.var_name
            {
                let should_exclude = assertion.negated ^ !applies_positively;
                if should_exclude {
                    apply_instanceof_exclusion(&assertion.asserted_type, ctx, results);
                } else {
                    apply_instanceof_inclusion(&assertion.asserted_type, false, ctx, results);
                }
            }
        }
    }
}

// ── Compound instanceof helpers ─────────────────────────────────

/// Flatten a `||` / `or` chain into its leaf operands.
///
/// Parenthesised sub-chains are unwrapped; a non-`||` expression yields a
/// single-element vec.  Used by the guard-clause narrowing to apply the
/// De Morgan inverse to each disjunct's own subject.
fn collect_or_operands<'b>(expr: &'b Expression<'b>) -> Vec<&'b Expression<'b>> {
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
fn try_extract_compound_negated_and_instanceof<'b>(
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

// ── in_array strict-mode narrowing ───────────────────────────────

/// Extract the haystack expression from an
/// `in_array($needle, $haystack, true)` call where the needle
/// matches `var_name`.
///
/// Returns `Some(haystack_expr)` when:
///   - The function name is `in_array`
///   - The first argument is a simple `$variable` matching `var_name`
///   - There are at least 3 arguments and the third is the literal `true`
///
/// The caller is responsible for resolving the haystack expression's
/// iterable element type.
pub(in crate::completion) fn try_extract_in_array<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<&'b Expression<'b>> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let func_call = match expr {
        Expression::Call(Call::Function(fc)) => fc,
        _ => return None,
    };
    let name = match func_call.function {
        Expression::Identifier(ident) => bytes_to_str(ident.value()),
        _ => return None,
    };
    if name != "in_array" {
        return None;
    }
    let args: Vec<_> = func_call.argument_list.arguments.iter().collect();
    if args.len() < 3 {
        return None;
    }

    // Third argument must be the literal `true` (strict mode).
    let third_expr = match &args[2] {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    if !third_expr.is_true() {
        return None;
    }

    // First argument must be our variable.
    let first_expr = match &args[0] {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    let needle_var = match first_expr {
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        _ => return None,
    };
    if needle_var != var_name {
        return None;
    }

    // Second argument is the haystack expression.
    let second_expr = match &args[1] {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    Some(second_expr)
}

/// The category of a PHP type-checking function like `is_array`, `is_string`, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeGuardKind {
    Array,
    String,
    Int,
    Float,
    Bool,
    Object,
    Numeric,
    Callable,
    Null,
    Scalar,
}

/// Return the canonical `PhpType` that a type-guard narrows `mixed` to.
///
/// When a variable has type `mixed` and a type-guard like `is_object()`
/// succeeds, the variable should narrow to `object` (not stay `mixed`
/// and not become empty).  This function maps each guard kind to the
/// PHP type it asserts.
fn guard_kind_to_narrowed_type(kind: TypeGuardKind) -> PhpType {
    match kind {
        TypeGuardKind::Array => PhpType::array(),
        TypeGuardKind::String => PhpType::string(),
        TypeGuardKind::Int => PhpType::int(),
        TypeGuardKind::Float => PhpType::float(),
        TypeGuardKind::Bool => PhpType::bool(),
        TypeGuardKind::Object => PhpType::object(),
        TypeGuardKind::Numeric => PhpType::numeric(),
        TypeGuardKind::Callable => PhpType::callable(),
        TypeGuardKind::Null => PhpType::null(),
        TypeGuardKind::Scalar => PhpType::Union(vec![
            PhpType::int(),
            PhpType::float(),
            PhpType::string(),
            PhpType::bool(),
        ]),
    }
}

/// Try to extract a type-guard function call on a variable.
///
/// Matches `is_array($var)`, `is_string($var)`, etc. (with optional
/// parenthesisation and negation).
///
/// Returns `Some((kind, negated))` when the expression is a recognised
/// type-guard call on `var_name`.
pub(crate) fn try_extract_type_guard(
    expr: &Expression<'_>,
    var_name: &str,
) -> Option<(TypeGuardKind, bool)> {
    match expr {
        Expression::Parenthesized(inner) => try_extract_type_guard(inner.expression, var_name),
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            try_extract_type_guard(prefix.operand, var_name).map(|(kind, neg)| (kind, !neg))
        }
        Expression::Call(Call::Function(fc)) => {
            let func_name = match &fc.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()),
                _ => return None,
            };
            let kind = match func_name {
                "is_array" => TypeGuardKind::Array,
                "is_string" => TypeGuardKind::String,
                "is_int" | "is_integer" | "is_long" => TypeGuardKind::Int,
                "is_float" | "is_double" | "is_real" => TypeGuardKind::Float,
                "is_bool" => TypeGuardKind::Bool,
                "is_object" => TypeGuardKind::Object,
                "is_numeric" => TypeGuardKind::Numeric,
                "is_callable" => TypeGuardKind::Callable,
                "is_null" => TypeGuardKind::Null,
                "is_scalar" => TypeGuardKind::Scalar,
                _ => return None,
            };
            let args = &fc.argument_list.arguments;
            if args.len() != 1 {
                return None;
            }
            let arg_expr = match args.first() {
                Some(Argument::Positional(pos)) => pos.value,
                Some(Argument::Named(named)) => named.value,
                _ => return None,
            };
            let arg_name = expr_to_subject_key(arg_expr)?;
            if arg_name != var_name {
                return None;
            }
            Some((kind, false))
        }
        _ => None,
    }
}

/// Check whether a `PhpType` matches a given type-guard kind.
///
/// For `TypeGuardKind::Array`, returns `true` for array-like types
/// (`array`, `list<T>`, `T[]`, `array{…}`, `iterable`, etc.).
fn type_matches_guard(ty: &PhpType, kind: TypeGuardKind) -> bool {
    match kind {
        TypeGuardKind::Array => ty.is_array_like(),
        TypeGuardKind::String => ty.is_subtype_of(&PhpType::string()),
        TypeGuardKind::Int => ty.is_subtype_of(&PhpType::int()),
        // `is_float()` returns false for integers at runtime, so use
        // exact type identity instead of `is_subtype_of` (which treats
        // `int` as a subtype of `float` due to PHP's type coercion).
        TypeGuardKind::Float => matches!(ty, PhpType::Named(n) if {
            let lower = n.to_ascii_lowercase();
            lower == "float" || lower == "double" || lower == "real"
        }),
        TypeGuardKind::Bool => ty.is_subtype_of(&PhpType::bool()),
        TypeGuardKind::Numeric => ty.is_subtype_of(&PhpType::numeric()),
        TypeGuardKind::Callable => ty.is_callable(),
        TypeGuardKind::Object => ty.is_object_like(),
        TypeGuardKind::Null => ty.is_null(),
        TypeGuardKind::Scalar => {
            ty.is_subtype_of(&PhpType::string())
                || ty.is_subtype_of(&PhpType::int())
                || ty.is_subtype_of(&PhpType::float())
                || ty.is_subtype_of(&PhpType::bool())
        }
    }
}

/// Narrow `results` to only the union members that match the given
/// type-guard kind.
///
/// For example, when `kind` is `Array` and the type string is
/// `null|list<Request>|Request`, the result is narrowed to
/// `list<Request>`.
pub(crate) fn apply_type_guard_inclusion(kind: TypeGuardKind, results: &mut Vec<ResolvedType>) {
    let had_types = !results.is_empty();
    for rt in results.iter_mut() {
        let filtered = filter_type_by_guard(&rt.type_string, kind, true);
        if let Some(narrowed) = filtered {
            rt.replace_type(narrowed);
        }
    }
    // Remove entries that became empty (no union member matched).
    results.retain(|rt| !rt.type_string.is_empty_sentinel());

    // When the guard's assertion fully contradicts every statically known
    // candidate — e.g. `is_object($file)` where `$file` was inferred as
    // plain `string` because upstream inference (a foreach over a custom
    // iterator) missed a possible member — trust the runtime check over
    // the incomplete static type instead of silently discarding all type
    // information.  Only fires when *every* entry was eliminated; a
    // single stale/duplicate entry among several valid ones is dropped
    // as before.
    if had_types && results.is_empty() {
        results.push(ResolvedType::from_type_string(guard_kind_to_narrowed_type(
            kind,
        )));
    }
}

/// Narrow `results` to only the union members that do NOT match the
/// given type-guard kind (inverse / else-body narrowing).
pub(crate) fn apply_type_guard_exclusion(kind: TypeGuardKind, results: &mut Vec<ResolvedType>) {
    for rt in results.iter_mut() {
        let filtered = filter_type_by_guard(&rt.type_string, kind, false);
        if let Some(narrowed) = filtered {
            rt.replace_type(narrowed);
        }
    }
    results.retain(|rt| !rt.type_string.is_empty_sentinel());
}

/// Filter a `PhpType` to keep only members that match (or don't match)
/// the given type-guard kind.
///
/// When `keep_matching` is `true`, keeps only members where
/// `type_matches_guard` returns `true` (then-body semantics).
/// When `false`, keeps only members where it returns `false`
/// (else-body semantics).
///
/// Returns `None` when no filtering is needed (non-union type that
/// already satisfies the predicate).  Returns `Some(Named("__empty"))`
/// when all members are filtered out.
fn filter_type_by_guard(ty: &PhpType, kind: TypeGuardKind, keep_matching: bool) -> Option<PhpType> {
    // Expand compound pseudo-types into their constituent unions so
    // that type guards can filter individual members.  For example,
    // `array-key` → `int|string`, so `is_string()` on `array-key`
    // correctly narrows to `string`.
    if let Some(expanded) = expand_pseudo_type_for_guard(ty) {
        return filter_type_by_guard(&expanded, kind, keep_matching);
    }

    // `is_numeric()` also returns true for numeric strings, not just
    // `int`/`float`.  Narrow string-like members to `numeric-string`
    // instead of dropping them or widening to bare `int|float`, so the
    // narrowed type stays a subtype of the original `string`.
    if kind == TypeGuardKind::Numeric && keep_matching {
        return Some(narrow_to_numeric_inclusive(ty));
    }

    match ty {
        PhpType::Union(members) => {
            let filtered: Vec<PhpType> = members
                .iter()
                .filter(|m| type_matches_guard(m, kind) == keep_matching)
                .cloned()
                .collect();
            if filtered.len() == members.len() {
                // Nothing was filtered out.
                None
            } else if filtered.is_empty() {
                Some(PhpType::empty_sentinel())
            } else if filtered.len() == 1 {
                Some(filtered.into_iter().next().unwrap())
            } else {
                Some(PhpType::Union(filtered))
            }
        }
        PhpType::Nullable(inner) => {
            // `?T` is `T|null`.  For `is_array`, null doesn't match,
            // so we keep only the inner type (if it matches) or only
            // null (if it doesn't).
            let inner_matches = type_matches_guard(inner, kind);
            let null_matches = type_matches_guard(&PhpType::null(), kind);
            match (
                inner_matches == keep_matching,
                null_matches == keep_matching,
            ) {
                (true, true) => None, // keep both → no change
                (true, false) => Some(inner.as_ref().clone()),
                (false, true) => Some(PhpType::null()),
                (false, false) => Some(PhpType::empty_sentinel()),
            }
        }
        other => {
            // `mixed` includes all types.  When narrowing in the
            // then-body (`keep_matching = true`), replace `mixed`
            // with the canonical type for the guard kind (e.g.
            // `is_object($mixed)` → `object`).  In the else-body
            // (`keep_matching = false`), `mixed` minus one kind is
            // still effectively `mixed`, so leave it unchanged.
            if other.is_mixed() {
                return if keep_matching {
                    Some(guard_kind_to_narrowed_type(kind))
                } else {
                    None // mixed minus one kind ≈ mixed
                };
            }
            // Non-union type: if it matches the predicate, keep it.
            if type_matches_guard(other, kind) == keep_matching {
                None // no change needed
            } else {
                Some(PhpType::empty_sentinel())
            }
        }
    }
}

/// Expand compound pseudo-types into unions of their constituent scalar
/// types so that type guard filtering can operate on individual members.
///
/// - `array-key` → `int|string`
/// - `scalar` → `int|float|string|bool`
/// - `numeric` / `number` → `int|float`
fn expand_pseudo_type_for_guard(ty: &PhpType) -> Option<PhpType> {
    let name = match ty {
        PhpType::Named(n) => n.to_ascii_lowercase(),
        _ => return None,
    };
    match name.as_str() {
        "array-key" => Some(PhpType::Union(vec![PhpType::int(), PhpType::string()])),
        "scalar" => Some(PhpType::Union(vec![
            PhpType::int(),
            PhpType::float(),
            PhpType::string(),
            PhpType::bool(),
        ])),
        "numeric" | "number" => Some(PhpType::Union(vec![PhpType::int(), PhpType::float()])),
        _ => None,
    }
}

/// Narrow a type to what `is_numeric()` guarantees, keeping string-like
/// members within `numeric-string` rather than widening them to `int|float`
/// or dropping them.
fn narrow_to_numeric_inclusive(ty: &PhpType) -> PhpType {
    match ty {
        PhpType::Union(members) => {
            let narrowed: Vec<PhpType> = members
                .iter()
                .filter_map(narrow_single_type_to_numeric)
                .collect();
            match narrowed.len() {
                0 => PhpType::empty_sentinel(),
                1 => narrowed.into_iter().next().unwrap(),
                _ => PhpType::Union(narrowed),
            }
        }
        // `null` never satisfies `is_numeric()`; narrow the inner type only.
        PhpType::Nullable(inner) => {
            narrow_single_type_to_numeric(inner).unwrap_or_else(PhpType::empty_sentinel)
        }
        other => narrow_single_type_to_numeric(other).unwrap_or_else(PhpType::empty_sentinel),
    }
}

/// Narrow a single (non-union) type to what `is_numeric()` guarantees.
/// Returns `None` when the type can never be numeric (e.g. an object).
fn narrow_single_type_to_numeric(ty: &PhpType) -> Option<PhpType> {
    if ty.is_mixed() {
        return Some(PhpType::Union(vec![
            PhpType::int(),
            PhpType::float(),
            PhpType::parse("numeric-string"),
        ]));
    }
    if type_matches_guard(ty, TypeGuardKind::Numeric) {
        return Some(ty.clone());
    }
    if ty.is_subtype_of(&PhpType::string()) {
        return Some(PhpType::parse("numeric-string"));
    }
    None
}
