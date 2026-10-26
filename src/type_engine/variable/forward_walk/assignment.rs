use super::*;

use mago_span::HasSpan;

use crate::atom::{Atom, atom, bytes_to_str};
use crate::class_lookup::Fqn;
use crate::php_type::{LiteralValue, PhpType, TypeKind};
use crate::type_engine::type_ops::condition_facts;
use crate::types::{ClassInfo, ResolvedType};

use super::super::rhs_resolution::{
    assignment_value, coalesce_assign_value, compound_assignment_result,
};

/// Identifies which callee parameter a call argument fills.
///
/// Positional arguments bind by their ordinal position; named arguments
/// (`foo(callback: ...)`) bind by the declared parameter name and may
/// appear out of their natural position, so they must be resolved by
/// name rather than by their slot in the argument list.
pub(crate) enum ArgSelector {
    Position(usize),
    Name(String),
}

/// Process increment/decrement expressions (`$a++`, `++$a`, `$a--`, `--$a`).
///
/// For numeric types (int, float), the base type is preserved.
/// Numeric literals and refined numeric types are widened because the
/// operation changes the value and may invalidate the refinement.
/// For numeric strings, the result becomes `int|float`.
/// For general strings, PHP increments alphabetically (stays string), while
/// decrementing a known non-numeric string is a no-op and stays exact.
/// Incrementing `null` produces `1`, while decrementing it leaves `null`
/// unchanged.
#[derive(Clone, Copy)]
pub(super) enum IncrementDecrementKind {
    Increment,
    Decrement,
}

pub(super) fn type_after_increment_decrement(
    ty: &PhpType,
    operation: IncrementDecrementKind,
) -> PhpType {
    match ty.kind() {
        TypeKind::Union(members) => {
            let mut transformed = Vec::with_capacity(members.len());
            for member in members {
                let member = type_after_increment_decrement(member, operation);
                for alternative in member.union_members() {
                    if !transformed.iter().any(|existing| existing == alternative) {
                        transformed.push(alternative.clone());
                    }
                }
            }
            if transformed.is_empty() {
                ty.clone()
            } else {
                PhpType::union(transformed)
            }
        }
        TypeKind::Nullable(inner) => {
            let inner = type_after_increment_decrement(inner, operation);
            match operation {
                IncrementDecrementKind::Increment => {
                    let mut alternatives = vec![PhpType::int()];
                    for alternative in inner.union_members() {
                        if !alternatives.iter().any(|existing| existing == alternative) {
                            alternatives.push(alternative.clone());
                        }
                    }
                    if alternatives.len() == 1 {
                        alternatives.into_iter().next().unwrap()
                    } else {
                        PhpType::union(alternatives)
                    }
                }
                IncrementDecrementKind::Decrement => {
                    if matches!(inner.kind(), TypeKind::Union(_)) {
                        let mut alternatives: Vec<PhpType> =
                            inner.union_members().into_iter().cloned().collect();
                        alternatives.push(PhpType::null());
                        PhpType::union(alternatives)
                    } else {
                        PhpType::nullable(inner)
                    }
                }
            }
        }
        _ if ty.is_null() => match operation {
            IncrementDecrementKind::Increment => PhpType::int(),
            IncrementDecrementKind::Decrement => PhpType::null(),
        },
        _ if ty.is_named_ci("numeric")
            || ty.is_named("number")
            || ty.is_named_ci("numeric-string")
            || ty.is_subtype_of(&PhpType::named(atom("numeric-string"))) =>
        {
            PhpType::union(vec![PhpType::int(), PhpType::float()])
        }
        _ if ty.is_int_subtype() => PhpType::int(),
        _ if ty.is_float_subtype() => PhpType::float(),
        TypeKind::Literal(value) if matches!(&**value, LiteralValue::String(_)) => {
            match operation {
                IncrementDecrementKind::Increment => PhpType::string(),
                IncrementDecrementKind::Decrement => ty.clone(),
            }
        }
        // A broad string may be numeric at runtime. PHP converts numeric
        // strings to int or float for both operators; non-numeric strings
        // remain strings (apart from the deprecated increment behaviour).
        _ if ty.is_string_subtype() => {
            PhpType::union(vec![PhpType::int(), PhpType::float(), PhpType::string()])
        }
        _ => ty.clone(),
    }
}

/// Get the byte offset of an expression (used for cursor comparisons).
pub(crate) fn stmt_offset(expr: &Expression<'_>) -> u32 {
    expr.span().start.offset
}

/// Result of [`try_process_inline_var_override`].
pub(crate) enum VarOverrideResult {
    /// No `@var` docblock found.
    None,
    /// A `@var Type $varName` block (with explicit variable name) was
    /// applied.  The caller should re-record the scope snapshot so that
    /// lookups within the same statement see the updated types.
    NamedVar,
    /// A `@var Type` block (without variable name) was applied to the
    /// assignment LHS.  The caller must NOT re-record the snapshot
    /// because the LHS variable should not be visible in the RHS.
    NoVar,
}

/// Extract the native type of an RHS expression using the current scope.
///
/// Used by [`try_process_inline_var_override`] to determine whether a
/// `@var` override should be blocked by a scalar native type.
///
/// This delegates to [`super::super::resolution::extract_native_type_from_rhs`]
/// via a `VarResolutionCtx` that has scope-based variable resolution.
/// That function already handles method calls, function calls, static
/// calls, casts, literals, and other patterns — including extracting
/// scalar return types from method signatures.
pub(crate) fn resolve_rhs_native_type(
    rhs: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let scope_resolver = scope.snapshot_resolver();
    let var_ctx =
        ctx.var_ctx_for_with_scope("$__rhs_check", 0, &scope_resolver, Some(scope.proofs()));
    super::super::resolution::extract_native_type_from_rhs(rhs, &var_ctx)
}

/// Resolve a [`PhpType`] to a complete `Vec<ResolvedType>` with
/// `class_info` populated when possible.  Falls back to a
/// type-string-only entry for scalars and unresolvable types.
///
/// The type comes from a docblock the walker just read out of the source,
/// so its class names are still spelled the way the author wrote them.
/// They are qualified against the enclosing namespace first, matching how
/// PHP reads the same spelling and how the parser already resolved the
/// `@param`/`@return` tags these types get compared against.
pub(crate) fn resolve_type_to_resolved_types(
    php_type: &PhpType,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    ctx.resolved_types_for(resolve_var_docblock_type(php_type, ctx))
}

/// The class names of a `@var` type read out of the source, qualified as
/// [`resolve_type_to_resolved_types`] does.
pub(crate) fn resolve_var_docblock_type(php_type: &PhpType, ctx: &ForwardWalkCtx<'_>) -> PhpType {
    // The declaration's templates are marked before names are qualified,
    // while `T` is still spelled the way the `@template` tag spells it.
    let marked = ctx
        .template_markers
        .as_ref()
        .map(|m| php_type.substitute(m));
    ctx.written_names()
        .resolve_type(marked.as_ref().unwrap_or(php_type))
}

/// Process assignment expressions, updating the scope.
pub(crate) fn process_assignment_expr<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // `($a = expr);` is a parenthesized assignment statement — written by
    // hand, or produced by the Blade preprocessor for `@php($a = expr)`.
    if let Expression::Assignment(assignment) = crate::parser::unwrap_parens(expr) {
        // An assignment buried in the value runs before the target here is
        // written, and the rest of the value reads what it wrote:
        // `$ok = ($x = $map[$key])->truthy();`.  A right-hand side that
        // *is* an assignment is left to the chain handling below, which
        // knows shapes (destructuring, indexed writes) this does not.
        let expr_types = if matches!(
            crate::parser::unwrap_parens(assignment.rhs),
            Expression::Assignment(_)
        ) {
            ExprTypes::default()
        } else {
            process_expr(assignment.rhs, scope, ctx)
        };

        if !assignment.operator.is_assign() {
            // Compound assignment: $x op= expr.
            // The type depends on the operator.
            process_compound_assignment(assignment, scope, ctx);
            return;
        }

        // Chain assignments: `$a = $b = expr` — the RHS is itself an
        // assignment expression, possibly parenthesized (`$a = ($b =
        // expr)`).  Process it first so that the inner variable (`$b`)
        // gets its type before we resolve the outer one.
        if matches!(
            crate::parser::unwrap_parens(assignment.rhs),
            Expression::Assignment(_)
        ) {
            process_assignment_expr(assignment.rhs, scope, ctx);
        }

        // Array destructuring: `[$a, $b] = …` / `list($a, $b) = …`
        if matches!(assignment.lhs, Expression::Array(_) | Expression::List(_)) {
            process_destructuring_assignment(assignment, scope, ctx);
            return;
        }

        // Array key assignment: `$var['key'] = expr;`
        if let Expression::ArrayAccess(array_access) = assignment.lhs {
            process_array_key_assignment(array_access, assignment, scope, ctx);
            return;
        }

        // Array push: `$var[] = expr;` and `$var['a'][$i][] = expr;`
        if let Expression::ArrayAppend(array_append) = assignment.lhs {
            process_array_append(array_append, assignment, scope, ctx);
            return;
        }

        // Property assignment: `$var->prop = expr;` (and null-safe
        // `$var?->prop = expr;`).  Record the assigned type under the
        // property-path key (e.g. `$settings->cache`) so that a later
        // read of that path resolves through the assignment rather than
        // the declaring class's declared property hints.  This is what
        // lets nested object property chains resolve, most notably on
        // `stdClass` which has no declared properties:
        //
        //     $s = new stdClass();
        //     $s->cache = new stdClass();
        //     $s->cache->ttl = 1;   // `$s->cache` now resolves to stdClass
        //
        // The key contains `->`, so it is treated as a synthetic
        // narrowing entry and stripped at loop boundaries — matching the
        // conservative behaviour of condition-based property narrowing.
        // A static property (`self::$repo = …`) is recorded the same way:
        // it is a member path with a declared type, and the lazy-init
        // idiom writes it in exactly the shape this branch handles.
        if matches!(
            assignment.lhs,
            Expression::Access(
                Access::Property(_) | Access::NullSafeProperty(_) | Access::StaticProperty(_)
            )
        ) {
            // Skip when the cursor is inside the RHS so that lookups
            // within the RHS see the pre-assignment state.
            let rhs_span = assignment.rhs.span();
            if ctx.cursor_offset >= rhs_span.start.offset
                && ctx.cursor_offset <= rhs_span.end.offset
            {
                return;
            }
            if let Some(key) = condition_facts::expr_to_subject_key(assignment.lhs) {
                // A write that dispatches to `__set` is opaque: the
                // magic setter may transform, reroute, or drop the
                // value, and a later read goes through `__get`, which
                // decides what comes back.  Drop whatever was known
                // about the path instead of recording the written type.
                if property_write_dispatches_to_magic_set(assignment.lhs, scope, ctx) {
                    scope.remove(&key);
                    scope.invalidate_dependent_keys(&key);
                    return;
                }
                let mut rhs_types =
                    resolve_rhs_with_types(assignment.rhs, scope, ctx, Some(&expr_types));
                adopt_declared_type_args(
                    assignment.lhs,
                    assignment.rhs,
                    &mut rhs_types,
                    scope,
                    ctx,
                );
                if rhs_types.is_empty() {
                    // The right-hand side did not resolve. Unlike a plain
                    // variable (`set_unknown`), a property's correct
                    // fallback is its *declared* type, not "unknown", so
                    // drop the key entirely rather than blanking it — a
                    // blank entry would leave the pre-write `instanceof`
                    // narrowing looking current instead of falling
                    // through to the declared type.
                    scope.remove(&key);
                } else {
                    scope.invalidate_proofs(&key);
                    scope.set(&key, rhs_types);
                }
            }
            return;
        }

        // Simple variable assignment: `$var = expr;`
        let lhs_name = match assignment.lhs {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => return,
        };

        // When the cursor is inside the RHS of this assignment, skip
        // storing the new type so that variable lookups within the RHS
        // see the pre-assignment type.  E.g. in `$request = new Bar(
        // name: $request->)`, the cursor on `$request->` should see
        // the old `Foo` type, not the new `Bar` type.
        let rhs_span = assignment.rhs.span();
        let cursor_in_rhs =
            ctx.cursor_offset >= rhs_span.start.offset && ctx.cursor_offset <= rhs_span.end.offset;
        if cursor_in_rhs {
            return;
        }

        let rhs_types = resolve_rhs_with_types(assignment.rhs, scope, ctx, Some(&expr_types));
        // `$show = $limit !== null && …` makes `$show` stand for what the
        // expression proves, read off the scope the value was computed in.
        let condition_proofs =
            condition_implications(&lhs_name, assignment.rhs, &rhs_types, scope, ctx);
        let ternary_proofs = ternary_arm_implications(&lhs_name, assignment.rhs, scope, ctx);
        // Reassigning the variable replaces its object identity, so any
        // property/array-access key rooted at it (seeded by an earlier
        // assignment or condition narrowing) is now stale.  Drop them
        // after resolving the RHS, so `$x = $x->foo` still reads the old
        // key while resolving.
        scope.invalidate_dependent_keys(&lhs_name);
        scope.invalidate_proofs(&lhs_name);
        // `$cb = function () { $this->stop(); };` — work out now what
        // invoking this closure does to its captures, so a later
        // `call_user_func($cb)` can apply it without the closure's body in
        // view.  `invalidate_proofs` above already dropped whatever was
        // recorded for a closure `$cb` held before this assignment.
        if let Expression::Closure(closure) = crate::parser::unwrap_parens(assignment.rhs) {
            let effects = closure_literal_capture_effects(closure, scope, ctx);
            scope.set_closure_capture_effects(&lhs_name, effects);
        }
        if !rhs_types.is_empty() {
            scope.set(&lhs_name, rhs_types);
        } else if !scope.get(&lhs_name).is_empty()
            && rhs_fails_on_resolved_receiver(assignment.rhs, scope)
        {
            // The right-hand side did not resolve, so nothing is known
            // about what the variable now holds — but the value it held
            // before the assignment is gone either way. Keeping the old
            // type is what made `$acc = $acc->merge($x)` (with `$x`
            // unresolved) report a member access on the `null` that
            // `$acc` was initialised with.
            //
            // Flagging it as unresolved is what keeps the loss local: a
            // join with a path that still knows the type takes that
            // path's answer, so `$acc = $acc->missing()` inside a loop
            // reports the missing member rather than turning `$acc`
            // unknown for the rest of the body.
            scope.set_unknown(&lhs_name);
        } else {
            // Nothing was known about the variable beforehand, or the
            // failure came from somewhere the answer was already "could
            // be anything". Neither is a type this walk lost, so the
            // entry is the plain unknown a join treats as top.
            scope.set_untyped(&lhs_name);
        }
        // `$isHtml = $raw instanceof HtmlString` makes `$isHtml` stand
        // for the check, so testing it later narrows `$raw`.
        record_assertion_variable(&lhs_name, assignment.rhs, scope, ctx);
        if !condition_proofs.is_empty() {
            scope
                .implied_narrowings
                .update(atom(&lhs_name), |proofs| proofs.extend(condition_proofs));
        }
        if let Some((implied, non_null)) = ternary_proofs {
            if !implied.is_empty() {
                scope
                    .implied_narrowings
                    .update(atom(&lhs_name), |proofs| proofs.extend(implied));
            }
            if !non_null.is_empty() {
                scope
                    .non_null_implications
                    .update(atom(&lhs_name), |keys| keys.extend(non_null));
            }
        }
        // `$period = $agreement?->latestPeriod()` makes `$period`'s null
        // stand for `$agreement`'s, so ruling out one rules out the other.
        record_nullsafe_origin(&lhs_name, assignment.rhs, scope);
        // `$ok = preg_match('/…/', $s, $m)` makes `$ok` stand for the
        // match's outcome, so testing it later narrows `$m`.
        record_preg_outcome(&lhs_name, assignment.rhs, scope, ctx);
    } else {
        // The expression assigns nothing at its root but may still assign
        // inside itself: `return ($x = $map[$key])->truthy();`.
        process_expr(expr, scope, ctx);
    }
}

/// What `$x = C ? A : B` proves about the condition's subjects once `$x`
/// is later shown to hold one arm's value.
///
/// Only one arm runs, so the assignment is the same as writing `$x` in
/// each branch of an `if`/`else` on `C`, and the join of those two
/// branches is what learns the proofs:
///
/// ```php
/// $problems = $importer instanceof Sheet ? $importer->getProblems() : [];
/// if ($problems !== []) { /* $importer is a Sheet here */ }
/// ```
///
/// Returns the implied narrowings and non-null implications the join
/// recorded for `lhs_name`, or `None` when `rhs` is not a full ternary or
/// either arm is unreachable or untyped.
fn ternary_arm_implications<'b>(
    lhs_name: &str,
    rhs: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<(Vec<ImpliedNarrowing>, Vec<Atom>)> {
    let Expression::Conditional(conditional) = crate::parser::unwrap_parens(rhs) else {
        return None;
    };
    let then_expr = conditional.then?;
    let mut before = scope.clone();
    let condition = process_condition(conditional.condition, &mut before, ctx);
    let arm = |arm_scope: &ScopeState, expr: &'b Expression<'b>| {
        if arm_scope.unreachable {
            return None;
        }
        let types = resolve_rhs_with_types(expr, arm_scope, ctx, None);
        if types.is_empty() {
            return None;
        }
        let mut arm_scope = arm_scope.clone();
        arm_scope.invalidate_dependent_keys(lhs_name);
        arm_scope.invalidate_proofs(lhs_name);
        arm_scope.set(lhs_name, types);
        Some(arm_scope)
    };
    let mut joined = arm(condition.truthy(ctx), then_expr)?;
    let else_scope = arm(condition.falsey(ctx), conditional.r#else)?;
    joined.merge_branch(&else_scope);
    let key = atom(lhs_name);
    let implied = joined
        .implied_narrowings
        .get(&key)
        .cloned()
        .unwrap_or_default();
    let non_null = joined
        .non_null_implications
        .get(&key)
        .cloned()
        .unwrap_or_default();
    Some((implied, non_null))
}

/// Give a `new X()` stored in a property the template arguments the
/// property declares, when the call itself bound none of them.
///
/// Nothing in `new \SplObjectStorage()` says what the storage will hold, so
/// its templates fall back to their bounds (`SplObjectStorage<object,
/// mixed>`); stored in a property declared `SplObjectStorage<DateTime,
/// null>` it is the object that declaration describes (PHPStan infers the
/// same).  A call that did bind a template keeps what it bound, and so does
/// an object of a class other than the declared one.
fn adopt_declared_type_args(
    lhs: &Expression<'_>,
    rhs: &Expression<'_>,
    rhs_types: &mut [ResolvedType],
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if !matches!(
        crate::parser::unwrap_parens(rhs),
        Expression::Instantiation(_)
    ) {
        return;
    }
    let [rt] = rhs_types else {
        return;
    };
    let Some(cls) = rt.class_info.as_ref() else {
        return;
    };
    let TypeKind::Generic(created) = rt.type_string.kind() else {
        return;
    };
    let unbound = cls.template_params.len() == created.args.len()
        && cls
            .template_params
            .iter()
            .zip(created.args.iter())
            .all(|(name, arg)| {
                let fallback = cls
                    .template_param_defaults
                    .get(name)
                    .or_else(|| cls.template_param_bounds.get(name))
                    .cloned()
                    .unwrap_or_else(PhpType::mixed);
                *arg == fallback
            });
    if !unbound {
        return;
    }
    let (object, prop_name) = match lhs {
        Expression::Access(Access::Property(pa)) => (pa.object, &pa.property),
        Expression::Access(Access::NullSafeProperty(pa)) => (pa.object, &pa.property),
        _ => return,
    };
    let ClassLikeMemberSelector::Identifier(ident) = prop_name else {
        return;
    };
    let prop_name = bytes_to_str(ident.value);
    let declared = match object {
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
            crate::inheritance::resolve_property_type_hint(
                ctx.current_class,
                prop_name,
                ctx.file.class_loader,
            )
        }
        _ => ResolvedType::into_arced_classes(resolve_rhs_with_scope(object, scope, ctx))
            .iter()
            .find_map(|owner| {
                crate::inheritance::resolve_property_type_hint(
                    owner,
                    prop_name,
                    ctx.file.class_loader,
                )
            }),
    };
    let Some(declared) = declared else {
        return;
    };
    let declared = declared.non_null_type().unwrap_or(declared);
    if let TypeKind::Generic(g) = declared.kind()
        && g.args.len() == created.args.len()
        && crate::util::short_name(&g.name).eq_ignore_ascii_case(&cls.name)
        && (ctx.file.class_loader)(Fqn::new(&g.name)).is_some_and(|c| c.fqn() == cls.fqn())
    {
        rt.type_string = declared;
    }
}

/// Whether a right-hand side that resolved to nothing failed on a member
/// of a class the walker did resolve.
///
/// This is the one shape where the failure is the walker's own and it
/// says so out loud: the receiver is a known class, the member is not on
/// it, and `unknown_member` is reported on this very line. Anything else
/// — a value that had no type to start with, a chain that already lost
/// the thread further up — is a failure inherited from somewhere the
/// answer was already "could be anything", and passing it on is all the
/// assignment does.
///
/// The receiver is read straight out of the scope rather than resolved,
/// so this costs nothing on a path that is already a dead end. It also
/// keeps the answer to the accumulator idiom the flag exists for —
/// `$acc = $acc->…`, whose receiver is the variable being written — and
/// leaves a longer chain alone, which is the right way round: the deeper
/// the chain, the likelier it is that what failed was some link of it
/// rather than the member on the end.
fn rhs_fails_on_resolved_receiver(rhs: &Expression<'_>, scope: &ScopeState) -> bool {
    let receiver = match rhs {
        Expression::Call(Call::Method(call)) => call.object,
        Expression::Call(Call::NullSafeMethod(call)) => call.object,
        Expression::Access(Access::Property(access)) => access.object,
        Expression::Access(Access::NullSafeProperty(access)) => access.object,
        _ => return false,
    };
    let Expression::Variable(Variable::Direct(var)) = receiver else {
        return false;
    };
    scope
        .get(bytes_to_str(var.name))
        .iter()
        .any(|rt| rt.class_info.is_some())
}

/// Whether `$obj->prop = …` writes through the subject class's `__set`
/// magic method instead of storing the value in a real property.
///
/// Returns `false` whenever no subject class resolves: without a class
/// there is no evidence of a magic setter, and the write is recorded as
/// before.
fn property_write_dispatches_to_magic_set(
    lhs: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let (object, selector) = match lhs {
        Expression::Access(Access::Property(pa)) => (pa.object, &pa.property),
        Expression::Access(Access::NullSafeProperty(pa)) => (pa.object, &pa.property),
        _ => return false,
    };
    let ClassLikeMemberSelector::Identifier(ident) = selector else {
        return false;
    };
    let prop_name = bytes_to_str(ident.value);
    let is_magic = |cls: &ClassInfo| {
        crate::virtual_members::property_write_is_magic(
            cls,
            prop_name,
            ctx.file.class_loader,
            ctx.resolved_class_cache,
        )
    };

    // `$this` and plain variables are answered from the walker's own
    // state, so the common write shapes cost no resolution.  A union
    // subject is magic as soon as one member routes the write through
    // `__set`: the recorded type would have no authority over what that
    // member's `__get` returns.
    let object = crate::parser::unwrap_parens(object);
    if let Expression::Variable(Variable::Direct(dv)) = object {
        let var_name = bytes_to_str(dv.name);
        if var_name == "$this" {
            return is_magic(ctx.current_class);
        }
        return scope
            .get(var_name)
            .iter()
            .filter_map(|rt| rt.class_info.as_deref())
            .any(is_magic);
    }
    resolve_rhs_with_scope(object, scope, ctx)
        .iter()
        .filter_map(|rt| rt.class_info.as_deref())
        .any(is_magic)
}

/// Process compound assignment operators (`+=`, `-=`, `/=`, `*=`, etc.).
///
/// The result type depends on the operator kind:
/// - `.=` → string
/// - `%=` → int
/// - `<<=`, `>>=`, `&=`, `|=`, `^=` → int
/// - `+=`, `-=`, `*=`, `/=`, `**=` → int|float
/// - `??=` → union of LHS non-null type and RHS type
pub(crate) fn process_compound_assignment<'b>(
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    use mago_syntax::cst::assignment::AssignmentOperator;

    let var_name = match assignment.lhs {
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        // `$totals[(string) $item] ??= 0;` stores the element's non-null
        // half or the fallback into the array, whatever the key is spelled
        // as: it is the same write `=` makes with that value.
        Expression::ArrayAccess(array_access)
            if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) =>
        {
            let combined = coalesce_assign_value(
                resolve_rhs_with_scope(assignment.lhs, scope, ctx),
                resolve_rhs_with_scope(assignment.rhs, scope, ctx),
            );
            super::array_assignment::process_array_key_write(array_access, combined, scope, ctx);
            return;
        }
        // `$this->regexp ??= $this->generate();` leaves the property
        // non-null just as surely as the same operator leaves a local
        // non-null, and the scope names a member path the same way it
        // names a local.  Only `??=` is routed this way: the arithmetic
        // operators below read the target's current type, which a member
        // path the scope has never seen does not have.
        _ if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) => {
            match crate::type_engine::type_ops::condition_facts::expr_to_subject_key(assignment.lhs)
            {
                Some(key) => key,
                None => return,
            }
        }
        // `$totals[$key]['count'] += $n;` writes the operator's result back
        // into the element, the same as spelling it out with `=` would.
        Expression::ArrayAccess(array_access) => {
            if let Some(result_type) = compound_assignment_result(
                &assignment.operator,
                || resolve_rhs_with_scope(assignment.lhs, scope, ctx),
                || resolve_rhs_with_scope(assignment.rhs, scope, ctx),
            ) {
                super::array_assignment::process_array_key_write(
                    array_access,
                    vec![ResolvedType::from_type_string(result_type)],
                    scope,
                    ctx,
                );
            }
            return;
        }
        _ => return,
    };
    if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) {
        // A member path the scope has not narrowed yet still has a
        // declared type, and `??=` only keeps that type's non-null half —
        // reading nothing there would drop every alternative the
        // declaration allows besides the fallback's.
        let lhs_types = match scope.get(&var_name) {
            [] => resolve_rhs_with_scope(assignment.lhs, scope, ctx),
            existing => existing.to_vec(),
        };
        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        let combined = coalesce_assign_value(lhs_types, rhs_types);
        if !combined.is_empty() {
            scope.set(&var_name, combined);
        } else if !scope.contains(&var_name) {
            scope.set_empty(&var_name);
        }
        return;
    }

    // `??=` was handled above and plain `=` never reaches here.
    let Some(result_type) = compound_assignment_result(
        &assignment.operator,
        || scope.get(&var_name).to_vec(),
        || resolve_rhs_with_scope(assignment.rhs, scope, ctx),
    ) else {
        return;
    };

    scope.set(&var_name, vec![ResolvedType::from_type_string(result_type)]);
}

/// Rewrite a `void` call result to `null` before it is stored as a
/// variable's type.
///
/// PHP has no `void` value: a `void`-declared function implicitly returns
/// `null`, so a variable assigned from such a call holds `null`, not
/// `void`. This is scoped to the assignment funnel rather than the RHS
/// pipeline itself, so a call's resolved type still reads as `void`
/// wherever that distinction matters outside of a variable holding it
/// (e.g. the argument-type-mismatch diagnostic reports a `void` argument
/// as always wrong, even against a nullable parameter).
fn normalize_void_assignment(mut resolved: Vec<ResolvedType>) -> Vec<ResolvedType> {
    for rt in &mut resolved {
        if rt.type_string.is_void() {
            rt.type_string = PhpType::null();
            rt.class_info = None;
        }
    }
    resolved
}

/// Resolve the type of an RHS expression using the current scope.
///
/// This is the key integration point: instead of calling
/// `resolve_variable_types` (which would recurse), we build a
/// `VarResolutionCtx` that already has the answer for any variable
/// references in the RHS — the forward walker has already resolved
/// them.
///
/// We delegate to `resolve_rhs_expression` with a `VarResolutionCtx`
/// whose `scope_var_resolver` reads directly from the forward walker's
/// in-progress `ScopeState`.  For bare variable references in the RHS,
/// we intercept them and return the scope-based result directly.
pub(crate) fn resolve_rhs_with_scope<'b>(
    rhs: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    resolve_rhs_with_types(rhs, scope, ctx, None)
}

/// [`resolve_rhs_with_scope`], reading the sub-expression types
/// [`process_expr`] recorded where they ran instead of re-deriving them
/// from `scope`, which holds what the expression's writes left behind.
pub(crate) fn resolve_rhs_with_types<'b>(
    rhs: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    expr_types: Option<&ExprTypes>,
) -> Vec<ResolvedType> {
    if let Some(recorded) = expr_types.and_then(|types| types.get(rhs)) {
        return recorded.to_vec();
    }

    // An assignment used as a value (`$a = $b = expr`, `$a = ($x /= 2)`,
    // `$x = $cache[$k] ??= expensive()`) evaluates to what it leaves in
    // its target.  Its operands are resolved through here rather than the
    // RHS pipeline, so `$a = $b = Foo::class` keeps the class-string this
    // function gives `Foo::class`.  The assignment may be parenthesized
    // (`$a = ($b = expr)`), so unwrap before matching.
    if let Expression::Assignment(assignment) = crate::parser::unwrap_parens(rhs) {
        return assignment_value(
            &assignment.operator,
            || resolve_rhs_with_types(assignment.lhs, scope, ctx, expr_types),
            || resolve_rhs_with_types(assignment.rhs, scope, ctx, expr_types),
        );
    }

    // For bare variable references, read directly from scope.
    // This is the O(1) path that replaces the recursive backward scan.
    if let Expression::Variable(Variable::Direct(dv)) = rhs {
        let var_name = bytes_to_str(dv.name).to_string();
        let from_scope = scope.get(&var_name);
        if !from_scope.is_empty() {
            return from_scope.to_vec();
        }
        // Variable not in scope — fall through to rhs_resolution which
        // handles some special patterns.
    }

    // ── Foo::class → class-string<Foo> ──────────────────────────
    // `Foo::class` is parsed as `Access::ClassConstant` with the
    // identifier `class`.  resolve_rhs_expression doesn't return a
    // useful type for this (it looks for a constant named "class"
    // on the class and finds nothing).  Handle it here so that
    // subsequent `new $var` can resolve the class-string.
    if let Some(class) = crate::parser::class_fetch_target(rhs) {
        let class_name = crate::class_lookup::class_expression_name(
            class,
            ctx.current_class,
            ctx.file.all_classes,
            ctx.file.source_class_loader,
            ctx.file.source_imports,
        )
        .filter(|name| !name.is_empty());
        if let Some(name) = class_name {
            let resolved_name = name.strip_prefix('\\').unwrap_or(&name);
            // Resolve the class so we can store a proper ResolvedType
            // with class_info.  This allows `new $var` to work.
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &PhpType::named(atom(resolved_name)),
                &ctx.current_class.fqn(),
                ctx.file.all_classes,
                ctx.file.class_loader,
            );
            // The identifier is spelled as the source writes it, which for a
            // name reached through a namespace import (`Support\Pen` behind
            // `use App\Support;`) is neither the FQCN nor resolvable once the
            // class-string is read somewhere else.  Prefer the resolved class.
            let class_string_type = PhpType::class_string(Some(match classes.first() {
                Some(cls) => PhpType::named(cls.fqn()),
                None => PhpType::named(atom(resolved_name)),
            }));
            if !classes.is_empty() {
                return ResolvedType::from_classes_with_hint(classes, class_string_type);
            }
            // Even if we can't resolve the class, return a type-string-only result
            // so the variable is non-empty in scope.
            return vec![ResolvedType::from_type_string(class_string_type)];
        }
    }

    // ── Fast paths for expressions whose type is known structurally ──
    // These avoid the full resolve_rhs_expression round-trip for
    // common patterns where the result type depends only on the
    // expression kind, not on the operand types.

    // Type casts (`(int) $x`), `!`, `~`, and the signs `-`/`+`.
    if let Expression::UnaryPrefix(prefix) = rhs
        && let Some(ty) = super::super::rhs_resolution::unary_prefix_result_type(
            &prefix.operator,
            prefix.operand,
            || resolve_rhs_with_types(prefix.operand, scope, ctx, expr_types),
            ctx.file.class_loader,
            ctx.resolved_class_cache,
        )
    {
        return vec![ResolvedType::from_type_string(ty)];
    }

    // For all other expressions, delegate to the existing RHS resolver
    // with a scope-based variable resolver injected.  When
    // `resolve_rhs_expression` (or its sub-functions like
    // `resolve_rhs_method_call_inner`, `resolve_rhs_property_access`)
    // need to resolve a variable's type, they call `resolve_var_types`
    // which checks `scope_var_resolver` first.  This reads directly
    // from the forward walker's in-progress `ScopeState`, bypassing
    // `resolve_variable_types` entirely.
    let rhs_offset = rhs.span().start.offset;
    let dummy_var = "$__rhs";
    let scope_locals = &scope.locals;
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_locals
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let scope_contains = |var_name: &str| -> bool { scope_locals.contains_key(&atom(var_name)) };
    let var_ctx = crate::type_engine::resolver::VarResolutionCtx {
        scope_contains_resolver: Some(&scope_contains),
        expr_types,
        ..ctx.var_ctx_for_with_scope(dummy_var, rhs_offset, &scope_resolver, Some(scope.proofs()))
    };

    let result = super::super::rhs_resolution::resolve_rhs_expression(rhs, &var_ctx);
    if !result.is_empty() {
        return normalize_void_assignment(result);
    }

    // ── Structural fallbacks ────────────────────────────────────
    // When resolve_rhs_expression returns empty, infer the type
    // purely from the expression structure.  These only fire as a
    // last resort so they never override a more precise result.

    // Unwrap parenthesized expressions for structural inference.
    let rhs = crate::parser::unwrap_parens(rhs);

    // Composite strings are not scalar literals and may not be handled by the
    // canonical literal resolver. Exact scalar literals have no fallback here:
    // reintroducing broad int/string/float types would silently undo its
    // precision whenever resolution regressed or hit a recursion guard.
    if matches!(rhs, Expression::CompositeString(_)) {
        return vec![ResolvedType::from_type_string(PhpType::string())];
    }

    result
}

/// Seed PHP superglobals (`$_SERVER`, `$_GET`, `$_POST`, etc.) into the
/// scope as `array` so that accesses on them resolve correctly.
/// PHP makes these available in every scope without
/// an explicit `global` declaration.
pub(crate) fn seed_superglobals(scope: &mut ScopeState) {
    let array_type = vec![ResolvedType::from_type_string(PhpType::named(atom(
        "array",
    )))];
    for name in crate::scope_collector::AUTO_GLOBALS {
        scope.set(name, array_type.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_decrement_preserves_only_values_that_cannot_change() {
        let text = PhpType::literal_string_raw("'abc'");

        assert_eq!(
            type_after_increment_decrement(&text, IncrementDecrementKind::Increment),
            PhpType::string()
        );
        assert_eq!(
            type_after_increment_decrement(&text, IncrementDecrementKind::Decrement),
            text
        );
        assert_eq!(
            type_after_increment_decrement(
                &PhpType::named(atom("number")),
                IncrementDecrementKind::Increment
            ),
            PhpType::union(vec![PhpType::int(), PhpType::float()])
        );
        assert_eq!(
            type_after_increment_decrement(
                &PhpType::named(atom("Number")),
                IncrementDecrementKind::Increment
            ),
            PhpType::named(atom("Number"))
        );
    }
}
