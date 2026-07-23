//! Scalar and structural type guards (`is_string`, `is_array`, …),
//! class-string and member-existence guards, `in_array` element
//! narrowing, and guard-clause (early-return) narrowing.

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::types::{AssertionKind, ClassInfo, ResolvedType};

use mago_syntax::cst::*;

use super::super::conditional::extract_class_string_from_expr;
use crate::completion::resolver::VarResolutionCtx;

use super::*;

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
