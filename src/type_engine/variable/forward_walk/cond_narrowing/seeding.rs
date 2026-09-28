use super::*;

/// Extract variable names referenced in instanceof / is_a / get_class
/// conditions.  This catches variables that are not yet in scope but
/// are used in guard clauses like `if (!$x instanceof Foo) { return; }`.
pub(crate) fn collect_condition_var_names(expr: &Expression<'_>) -> Vec<String> {
    let mut names = Vec::new();
    collect_condition_var_names_inner(expr, &mut names);
    names
}

/// Collect every variable a condition reads, in source order.
///
/// Unlike [`collect_condition_var_names`], which only picks out the subjects
/// of `instanceof`-shaped checks, this is the full set of candidates the
/// narrowing pipeline should consider — the equivalent of the `scope.locals`
/// key list `apply_condition_narrowing` walks when it has a live scope.
pub(crate) fn collect_condition_subject_vars(expr: &Expression<'_>, out: &mut Vec<String>) {
    let push = |name: String, out: &mut Vec<String>| {
        if !out.contains(&name) {
            out.push(name);
        }
    };
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            push(bytes_to_str(dv.name).to_string(), out);
        }
        Expression::Parenthesized(inner) => collect_condition_subject_vars(inner.expression, out),
        Expression::UnaryPrefix(unary) => collect_condition_subject_vars(unary.operand, out),
        Expression::UnaryPostfix(unary) => collect_condition_subject_vars(unary.operand, out),
        Expression::Binary(bin) => {
            collect_condition_subject_vars(bin.lhs, out);
            collect_condition_subject_vars(bin.rhs, out);
        }
        Expression::Assignment(assignment) => {
            collect_condition_subject_vars(assignment.lhs, out);
            collect_condition_subject_vars(assignment.rhs, out);
        }
        Expression::Conditional(conditional) => {
            collect_condition_subject_vars(conditional.condition, out);
            if let Some(then) = conditional.then {
                collect_condition_subject_vars(then, out);
            }
            collect_condition_subject_vars(conditional.r#else, out);
        }
        Expression::Call(call) => {
            let args = match call {
                Call::Function(fc) => {
                    collect_condition_subject_vars(fc.function, out);
                    &fc.argument_list
                }
                Call::Method(mc) => {
                    collect_condition_subject_vars(mc.object, out);
                    &mc.argument_list
                }
                Call::NullSafeMethod(mc) => {
                    collect_condition_subject_vars(mc.object, out);
                    &mc.argument_list
                }
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for arg in args.arguments.iter() {
                collect_condition_subject_vars(arg.value(), out);
            }
        }
        Expression::Access(Access::Property(pa)) => collect_condition_subject_vars(pa.object, out),
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_condition_subject_vars(pa.object, out);
        }
        Expression::ArrayAccess(aa) => {
            collect_condition_subject_vars(aa.array, out);
            collect_condition_subject_vars(aa.index, out);
        }
        Expression::Construct(Construct::Isset(isset)) => {
            for value in isset.values.iter() {
                collect_condition_subject_vars(value, out);
            }
        }
        Expression::Construct(Construct::Empty(empty)) => {
            collect_condition_subject_vars(empty.value, out);
        }
        _ => {}
    }
}

/// Whether a scope key is a synthetic key for an expression
/// (`$this->cache`, `$row["id"]`, `mb_strpos($s, $m)`) rather than a
/// plain variable.
pub(crate) fn is_synthetic_key(key: &str) -> bool {
    narrowing::is_member_path_key(key) || narrowing::is_call_key(key)
}

/// Remove synthetic property/array access keys from the scope.
/// Called after loop merges and other scope transitions where
/// condition-based narrowing no longer holds.
pub(crate) fn strip_synthetic_property_keys(scope: &mut ScopeState) {
    scope.locals.retain(|key, _| !is_synthetic_key(key));
    // A check on a property path is narrowing too, so a boolean that
    // stands for one is dropped alongside the key it describes.
    scope.assertions.retain(|_, checks| {
        checks.retain(|c| !is_synthetic_key(&c.subject));
        !checks.is_empty()
    });
}

/// Keep only the synthetic property/array access keys that *every*
/// surviving path out of a branching statement established a type for.
///
/// A key that only some paths carry is narrowing (or an assignment)
/// that holds inside one branch and says nothing about the others, so
/// the merged union would be an unsound claim about the program point
/// after the statement. A key every path carries is a genuine join:
/// each branch contributed its own truth, so the union is exactly the
/// type the property can have once the branches reconverge. That is
/// what makes the lazy-initialisation idiom resolve — the then-branch
/// assigns the concrete type and the implicit else path narrows to it
/// via the negated condition, so both agree.
pub(crate) fn retain_synthetic_keys_common_to_all(
    scope: &mut ScopeState,
    surviving: &[&ScopeState],
) {
    scope.locals.retain(|key, _| {
        !is_synthetic_key(key) || surviving.iter().all(|s| s.locals.contains_key(key))
    });
}

/// Seed a synthetic scope entry for a compound key (property access
/// or array access) if it isn't already present.  Simple variable
/// names (no `->` or `["`) are skipped since they are already tracked.
pub(crate) fn seed_synthetic_key_if_needed(
    key: &str,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Only seed compound keys (property access, array access, or a
    // static property).
    if !narrowing::is_member_path_key(key) {
        return;
    }
    // A call key that carries arguments cannot be re-resolved from its
    // text — the arguments decide the return type.  Those are seeded from
    // the expression they were built from, by
    // [`seed_call_subject_keys`].
    if narrowing::is_call_key_with_arguments(key) {
        return;
    }
    if scope.contains(key) {
        return;
    }

    let types = resolve_synthetic_key_type(key, scope, ctx);
    scope.set(key, types);
}

/// Seed property/array-access subject keys that appear as arguments to a
/// call expression into the scope.
///
/// Used for assertion narrowing on non-variable subjects, e.g.
/// `assertInstanceOf(X::class, $view->component)` or a `@phpstan-assert`
/// helper invoked on `$arg->value`.  Each argument that resolves to a
/// compound subject key (property path or array access) is seeded with
/// its current type so the assertion narrowing loop can narrow it.
pub(crate) fn seed_assert_arg_subject_keys(
    expr: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let Expression::Call(call) = expr else {
        return;
    };
    let argument_list = match call {
        Call::Function(fc) => &fc.argument_list,
        Call::Method(mc) => &mc.argument_list,
        Call::NullSafeMethod(mc) => &mc.argument_list,
        Call::StaticMethod(sc) => &sc.argument_list,
    };
    for arg in argument_list.arguments.iter() {
        let arg_expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        if let Some(key) = narrowing::expr_to_subject_key(arg_expr)
            && narrowing::is_member_path_key(&key)
        {
            seed_synthetic_key_if_needed(&key, scope, ctx);
        }
        // An argument that is itself a check (`assert($items[0] instanceof
        // Foo)`) carries its subject one level further down, so it is
        // seeded the same way an `if` condition's subject is.
        seed_property_keys_into_scope(arg_expr, scope, ctx);
    }

    // A tag can also name a path the call site never spells out —
    // `@phpstan-assert bool $this->resolved` on a method with no
    // arguments at all — so the tags themselves are read for subjects to
    // seed, not just the arguments.
    let snapshot = scope.locals.clone();
    let resolver =
        |vn: &str| -> Vec<ResolvedType> { snapshot.get(&atom(vn)).cloned().unwrap_or_default() };
    let var_ctx = build_var_ctx("", ctx, &resolver);
    let Some(info) = narrowing::extract_call_assertions(call, &var_ctx) else {
        return;
    };
    let keys: Vec<String> = info
        .assertions
        .iter()
        .filter_map(|assertion| narrowing::assertion_subject_key(&assertion.param_name, &info))
        .filter(|key| narrowing::is_member_path_key(key))
        .collect();
    for key in keys {
        seed_synthetic_key_if_needed(&key, scope, ctx);
    }
}

/// Collect property access keys (e.g. `$a->foo`) from conditions that
/// contain type guards or instanceof checks on property accesses.
/// These keys are injected into the scope so that narrowing applies.
pub(crate) fn collect_condition_property_keys(expr: &Expression<'_>) -> Vec<String> {
    let mut keys = Vec::new();
    collect_condition_property_keys_inner(expr, &mut keys);
    keys
}

pub(crate) fn collect_condition_property_keys_inner(expr: &Expression<'_>, keys: &mut Vec<String>) {
    match expr {
        // instanceof: `$a->foo instanceof Foo` or `$row["page"] instanceof Foo`
        Expression::Binary(bin) if bin.operator.is_instanceof() => {
            if let Some(key) = narrowing::expr_to_subject_key(bin.lhs)
                && narrowing::is_member_path_key(&key)
                && !keys.contains(&key)
            {
                keys.push(key);
            }
        }
        // Class identity: `get_class($a->foo) === Foo::class`, and the
        // `$a->foo::class` spelling of the same question, on either side
        // of the comparison.
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::Identical(_)
                    | BinaryOperator::Equal(_)
                    | BinaryOperator::NotIdentical(_)
                    | BinaryOperator::NotEqual(_)
            ) =>
        {
            for side in [bin.lhs, bin.rhs] {
                if let Some(key) = narrowing::class_identity_subject_key(side)
                    && narrowing::is_member_path_key(&key)
                    && !keys.contains(&key)
                {
                    keys.push(key);
                }
            }
        }
        // Negation: `!is_string($a->foo)`, `!($a->foo instanceof Foo)`
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            collect_condition_property_keys_inner(prefix.operand, keys);
        }
        Expression::Parenthesized(p) => {
            collect_condition_property_keys_inner(p.expression, keys);
        }
        // Logical connectives
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_)
                    | BinaryOperator::LowAnd(_)
                    | BinaryOperator::Or(_)
                    | BinaryOperator::LowOr(_)
            ) =>
        {
            collect_condition_property_keys_inner(bin.lhs, keys);
            collect_condition_property_keys_inner(bin.rhs, keys);
        }
        // Type guard functions: `is_string($a->foo)`, `is_int($a->foo)`, etc.
        Expression::Call(Call::Function(func_call)) => {
            if let Expression::Identifier(ident) = func_call.function {
                let func_name = bytes_to_str(ident.value());
                let is_type_guard = matches!(
                    func_name,
                    "is_array"
                        | "is_string"
                        | "is_int"
                        | "is_integer"
                        | "is_long"
                        | "is_float"
                        | "is_double"
                        | "is_real"
                        | "is_bool"
                        | "is_object"
                        | "is_numeric"
                        | "is_callable"
                        | "is_null"
                        | "is_scalar"
                        | "is_a"
                        | "class_exists"
                        | "interface_exists"
                        | "enum_exists"
                        | "trait_exists"
                        // A strict `in_array` proves its needle is one of
                        // the haystack's elements, so the needle is a
                        // subject the branch narrows like any other.
                        | "in_array"
                );
                if is_type_guard && let Some(first_arg) = func_call.argument_list.arguments.first()
                {
                    let arg_expr = match first_arg {
                        Argument::Positional(pos) => pos.value,
                        Argument::Named(named) => named.value,
                    };
                    if let Some(key) = narrowing::expr_to_subject_key(arg_expr)
                        && narrowing::is_member_path_key(&key)
                        && !keys.contains(&key)
                    {
                        keys.push(key);
                    }
                }
            }
        }
        // A bare truthy test names its subject and nothing else:
        // `$article->alt ? $article->alt : $article->title` and
        // `if ($row->id)` both prove the path is truthy, and no operator
        // is present for the arms above to match on.
        Expression::Access(
            Access::Property(_) | Access::NullSafeProperty(_) | Access::StaticProperty(_),
        )
        | Expression::ArrayAccess(_) => {
            if let Some(key) = narrowing::expr_to_subject_key(expr)
                && narrowing::is_member_path_key(&key)
                && !keys.contains(&key)
            {
                keys.push(key);
            }
        }
        _ => {}
    }
}

/// Resolve the type of a property access key (e.g. `$a->foo`) or an
/// argument-less call key (`$a->foo()`) from the current scope and seed
/// it into the scope as a synthetic entry.  This allows subsequent
/// narrowing functions to find and narrow those expressions, and it is
/// what keeps a check against a wide type from discarding a narrower
/// declared one: seeded with `StringExpr`, an `instanceof Expr` guard
/// intersects down to `StringExpr` rather than replacing it.
pub(crate) fn seed_property_keys_into_scope(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    seed_call_subject_keys(condition, scope, ctx);

    let keys = collect_condition_property_keys(condition);
    if keys.is_empty() {
        return;
    }
    for key in &keys {
        // An array-access subject (`$items[0]`) is keyed and narrowed the
        // same way a property is, so both go through the seeder that knows
        // how to read an element type out of the base variable's type.
        // `seed_synthetic_key_if_needed` skips a key already seeded (e.g.
        // from a prior elseif condition).
        seed_synthetic_key_if_needed(key, scope, ctx);
    }
}

/// Seed the scope with the current type of every call the condition
/// tests, keyed under the call's own written form.
///
/// This is the counterpart of [`seed_synthetic_key_if_needed`] for the
/// calls that seeder cannot answer: `mb_strpos($slug, $marker)` cannot be
/// re-resolved from its key text the way `$this->handle` can, because the
/// arguments are what decide the return type, and `currentUser()` has no
/// receiver path to walk.  Resolving them here, from the expression, puts
/// the un-narrowed type in scope so the guard that follows has something
/// to narrow — and every later occurrence of the same call text then reads
/// the narrowed entry instead of asking the callee what it returns.
pub(crate) fn seed_call_subject_keys(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match condition {
        Expression::Parenthesized(paren) => seed_call_subject_keys(paren.expression, scope, ctx),
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            seed_call_subject_keys(prefix.operand, scope, ctx)
        }
        Expression::Binary(bin) if bin.operator.is_instanceof() => {
            seed_call_subject(bin.lhs, scope, ctx)
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_)
                    | BinaryOperator::LowAnd(_)
                    | BinaryOperator::Or(_)
                    | BinaryOperator::LowOr(_)
            ) =>
        {
            seed_call_subject_keys(bin.lhs, scope, ctx);
            seed_call_subject_keys(bin.rhs, scope, ctx);
        }
        // A comparison names its subject on whichever side is not the
        // value being compared against, and the narrowing extractors read
        // both, so both are offered here too.
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::Identical(_)
                    | BinaryOperator::NotIdentical(_)
                    | BinaryOperator::Equal(_)
                    | BinaryOperator::NotEqual(_)
            ) =>
        {
            seed_call_subject(bin.lhs, scope, ctx);
            seed_call_subject(bin.rhs, scope, ctx);
        }
        // A bare truthy test on a call, and the argument of a type guard
        // or assertion helper written around one.
        Expression::Call(call) => {
            let argument_list = match call {
                Call::Function(fc) => &fc.argument_list,
                Call::Method(mc) => &mc.argument_list,
                Call::NullSafeMethod(mc) => &mc.argument_list,
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for argument in argument_list.arguments.iter() {
                seed_call_subject(narrowing::argument_value(argument), scope, ctx);
            }
            seed_call_subject(condition, scope, ctx);
        }
        Expression::Construct(Construct::Isset(isset)) => {
            for value in isset.values.iter() {
                seed_call_subject(value, scope, ctx);
            }
        }
        Expression::Construct(Construct::Empty(empty)) => {
            seed_call_subject(empty.value, scope, ctx)
        }
        _ => {}
    }
}

/// Seed one expression under its call key, when it has one and the scope
/// does not already carry it.
fn seed_call_subject(expr: &Expression<'_>, scope: &mut ScopeState, ctx: &ForwardWalkCtx<'_>) {
    let Some(key) = narrowing::expr_to_subject_key(expr) else {
        return;
    };
    if !seeds_from_expression(&key) || scope.contains(&key) {
        return;
    }
    let types = super::assignment::resolve_rhs_with_scope(expr, scope, ctx);
    scope.set(&key, types);
}

/// Whether a call key has to be seeded from the expression it was built
/// from rather than re-resolved from its own text by
/// [`resolve_synthetic_key_type`].
///
/// A key carrying arguments never re-resolves: the arguments are what
/// decide the return type, and the key text is all that survives. A bare
/// `currentUser()` or `Holder::make()` names everything needed to resolve
/// it, but the text-based resolver only knows how to walk a receiver path,
/// which neither has. What it does own is `$h->get()`, whose receiver the
/// walker's scope already holds, so that shape is left to it.
fn seeds_from_expression(key: &str) -> bool {
    narrowing::is_call_key(key)
        && (narrowing::is_call_key_with_arguments(key) || !narrowing::is_member_path_key(key))
}

pub(crate) fn collect_condition_var_names_inner(expr: &Expression<'_>, names: &mut Vec<String>) {
    match expr {
        Expression::Binary(bin) if bin.operator.is_instanceof() => {
            if let Expression::Variable(Variable::Direct(dv)) = bin.lhs {
                let name = bytes_to_str(dv.name).to_string();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            collect_condition_var_names_inner(prefix.operand, names);
        }
        Expression::Parenthesized(p) => {
            collect_condition_var_names_inner(p.expression, names);
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_)
                    | BinaryOperator::LowAnd(_)
                    | BinaryOperator::Or(_)
                    | BinaryOperator::LowOr(_)
            ) =>
        {
            collect_condition_var_names_inner(bin.lhs, names);
            collect_condition_var_names_inner(bin.rhs, names);
        }
        // is_a($var, ...) and get_class($var) === ...
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()),
                _ => return,
            };
            if matches!(
                crate::util::strip_fqn_prefix(func_name)
                    .to_ascii_lowercase()
                    .as_str(),
                "is_a"
                    | "get_class"
                    | "class_exists"
                    | "interface_exists"
                    | "enum_exists"
                    | "trait_exists"
            ) && let Some(first_arg) = func_call.argument_list.arguments.first()
            {
                let arg_expr = match first_arg {
                    Argument::Positional(pos) => pos.value,
                    Argument::Named(named) => named.value,
                };
                if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
                    let name = bytes_to_str(dv.name).to_string();
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
        }
        _ => {}
    }
}
