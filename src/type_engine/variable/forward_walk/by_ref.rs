//! By-reference parameters and callable arguments.
//!
//! Seeds the variables a call writes through a `&$ref` parameter, and the
//! ones a callback's parameters bind when the callee invokes it before
//! returning.

use super::*;

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::{atom, bytes_to_str};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::call_resolution::OutParamCallee;
use crate::types::{CallableInvocation, ClassInfo, ParameterInfo, ResolvedType};

pub(crate) fn process_by_ref_closure_captures<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match expr {
        Expression::Call(call) => {
            // The receiver runs before the arguments, and a chained call
            // (`$db->connect()->transaction(...)`) hides closure arguments
            // inside its receiver expression, so recurse into it first.
            match call {
                Call::Method(mc) => process_by_ref_closure_captures(mc.object, scope, ctx),
                Call::NullSafeMethod(mc) => process_by_ref_closure_captures(mc.object, scope, ctx),
                _ => {}
            }

            // `(function () use (&$x) { ... })()` runs the closure right
            // here, so its final variable state replaces the outer one.
            if let Call::Function(fc) = call
                && let Expression::Closure(closure) = crate::parser::unwrap_parens(fc.function)
            {
                process_by_ref_closure_capture(closure, scope, ctx, true, true);
            }

            let args = match call {
                Call::Function(fc) => &fc.argument_list,
                Call::Method(mc) => &mc.argument_list,
                Call::NullSafeMethod(mc) => &mc.argument_list,
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            let mut next_positional = 0usize;
            for arg in args.arguments.iter() {
                let (arg_expr, selector) = arg_expr_and_selector(arg, &mut next_positional);
                if let Expression::Closure(closure) = arg_expr {
                    // Asking the callee is the expensive half, so it is only
                    // asked about a closure with a capture for the answer to
                    // apply to.
                    let certain = captures_by_reference(closure)
                        && call_invokes_arg_immediately(call, &selector, scope, ctx);
                    process_by_ref_closure_capture(closure, scope, ctx, certain, false);
                } else {
                    process_by_ref_closure_captures(arg_expr, scope, ctx);
                }
            }
        }
        // A closure that is defined but not provably invoked (stored in a
        // variable, passed somewhere opaque) may still run any time later,
        // so the types it assigns are unioned into the captured variables.
        Expression::Closure(closure) => {
            process_by_ref_closure_capture(closure, scope, ctx, false, false);
        }
        // `new Wrapper(function () use (&$x) { … })` hands the closure to an
        // object that invokes it later (or never), which is the
        // widen-don't-replace case the `Closure` arm handles — the point is
        // that the capture is seen at all instead of the mutation going
        // missing.  Same for a closure inside an array literal.
        Expression::Instantiation(instantiation) => {
            if let Some(ref args) = instantiation.argument_list {
                for arg in args.arguments.iter() {
                    process_by_ref_closure_captures(arg.value(), scope, ctx);
                }
            }
        }
        Expression::Array(_) | Expression::LegacyArray(_) => {
            let elements =
                crate::parser::array_literal_elements(expr).expect("an array literal has elements");
            for elem in elements.iter() {
                if let Some(value) = crate::parser::array_element_value(elem) {
                    process_by_ref_closure_captures(value, scope, ctx);
                }
            }
        }
        Expression::Parenthesized(inner) => {
            process_by_ref_closure_captures(inner.expression, scope, ctx);
        }
        Expression::Assignment(assignment) => {
            process_by_ref_closure_captures(assignment.rhs, scope, ctx);
        }
        _ => {}
    }
}

/// Whether a call provably invokes its callable argument before
/// returning, so a by-ref capture's final state can *replace* the outer
/// variable rather than widen it.
///
/// Reads the callee's `@param-immediately-invoked-callable` /
/// `@param-later-invoked-callable` tag, wherever the callee is declared,
/// and otherwise follows PHPStan's defaults: a function runs what it is
/// handed through a parameter typed as a callable immediately, and a
/// method keeps it for later.  An unresolvable callee or receiver is not
/// proof either way, so it answers `false` and the caller falls back to
/// widening.
pub(crate) fn call_invokes_arg_immediately(
    call: &Call<'_>,
    selector: &ArgSelector,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    match call {
        Call::Function(fc) => {
            let Expression::Identifier(ident) = fc.function else {
                return false;
            };
            // A name that resolves to nothing gives no signal either way,
            // the same as an unresolvable receiver: `outer(inner(fn () use
            // (&$x) {...}))` for undefined `outer`/`inner` stays uncertain.
            ctx.loaders
                .function_loader
                .and_then(|load| load(bytes_to_str(ident.value()), fc.function.span().start.offset))
                .is_some_and(|function| {
                    param_runs_callable_immediately(&function.parameters, selector, true)
                })
        }
        Call::Method(MethodCall { object, method, .. })
        | Call::NullSafeMethod(NullSafeMethodCall { object, method, .. }) => {
            let ClassLikeMemberSelector::Identifier(ident) = method else {
                return false;
            };
            every_class_runs_callable_immediately(
                receiver_classes(object, scope, ctx)
                    .iter()
                    .map(|class| &**class),
                bytes_to_str(ident.value),
                selector,
                ctx,
            )
        }
        Call::StaticMethod(sc) => {
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return false;
            };
            every_class_runs_callable_immediately(
                static_receiver_classes(sc.class, scope, ctx)
                    .iter()
                    .map(|class| &**class),
                bytes_to_str(ident.value),
                selector,
                ctx,
            )
        }
    }
}

/// The classes an instance call's receiver may hold.
///
/// `$this` is read from the scope like any variable, since a closure the
/// callee binds (`Closure::call()`, `@param-closure-this`) holds a `$this`
/// of another class; only where the scope holds none is it the enclosing
/// class.  Any other receiver (a property, a chained call, a new object)
/// is resolved against the scope the way the call's own type is.
fn receiver_classes(
    object: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<Arc<ClassInfo>> {
    let object = crate::parser::unwrap_parens(object);
    if let Expression::Variable(Variable::Direct(dv)) = object
        && dv.name == b"$this"
        && scope.get("$this").is_empty()
    {
        if ctx.current_class.name.is_empty() {
            return Vec::new();
        }
        return super::super::closure_resolution::find_owner_by_name(
            &ctx.current_class.fqn(),
            ctx.file.all_classes,
            ctx.file.class_loader,
        )
        .into_iter()
        .collect();
    }
    ResolvedType::into_arced_classes(super::assignment::resolve_rhs_with_scope(
        object, scope, ctx,
    ))
}

/// The classes the class expression of a static call or `new` names
/// (`Foo::make()`, `$class::make()`, `new $class`), read against the
/// scope.
pub(super) fn static_receiver_classes(
    class_expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<Arc<ClassInfo>> {
    let scope_resolver = |name: &str| scope.get(name).to_vec();
    let var_ctx = ctx.var_ctx_for_with_scope(
        "",
        class_expr.span().start.offset,
        &scope_resolver,
        Some(scope.proofs()),
    );
    super::super::closure_resolution::resolve_static_receiver_classes(
        class_expr,
        &var_ctx.as_resolution_ctx(),
    )
}

/// Whether `method_name` runs the callable passed as the `selector`
/// argument before returning on each of `classes`, of which there has to
/// be at least one.  Each class the receiver may hold has to promise it,
/// or the call may land on one that keeps the callable for later.
fn every_class_runs_callable_immediately<'c>(
    classes: impl Iterator<Item = &'c ClassInfo>,
    method_name: &str,
    selector: &ArgSelector,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let mut classes = classes.peekable();
    classes.peek().is_some()
        && classes.all(|class| method_runs_callable_immediately(class, method_name, selector, ctx))
}

/// Whether `method_name`, as `class` declares or inherits it, runs the
/// callable passed as the `selector` argument before returning.
fn method_runs_callable_immediately(
    class: &ClassInfo,
    method_name: &str,
    selector: &ArgSelector,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        class,
        ctx.file.class_loader,
        ctx.resolved_class_cache,
    );
    merged
        .get_method(method_name)
        .is_some_and(|method| param_runs_callable_immediately(&method.parameters, selector, false))
}

/// Whether the parameter the `selector` argument fills runs the callable
/// it is handed before the call returns.
///
/// A parameter whose docblock does not say is read as PHPStan reads it:
/// immediately when it belongs to a function (`of_function`) and is typed
/// as a callable, and later otherwise, since a parameter that takes
/// anything (`mixed ...$args`) may just keep what it is given.  An
/// argument no parameter takes is not proof either way.
fn param_runs_callable_immediately(
    parameters: &[ParameterInfo],
    selector: &ArgSelector,
    of_function: bool,
) -> bool {
    let param = match selector {
        ArgSelector::Position(idx) => parameters
            .get(*idx)
            .or_else(|| parameters.last().filter(|param| param.is_variadic)),
        ArgSelector::Name(name) => parameters
            .iter()
            .find(|param| param.name.strip_prefix('$') == Some(name.as_str())),
    };
    param.is_some_and(|param| match param.callable_invocation {
        Some(invocation) => invocation == CallableInvocation::Immediate,
        None => of_function && param.type_hint.as_ref().is_some_and(admits_a_callable),
    })
}

/// Whether `ty` is a callable type, or a union with one among its members
/// (`?callable`, `Closure|string`).
fn admits_a_callable(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Union(members) => members.iter().any(PhpType::is_callable),
        _ => ty.is_callable(),
    }
}

/// Whether `closure` captures any variable by reference (`use (&$x)`).
fn captures_by_reference(closure: &Closure<'_>) -> bool {
    closure.use_clause.as_ref().is_some_and(|use_clause| {
        use_clause
            .variables
            .iter()
            .any(|use_var| use_var.ampersand.is_some())
    })
}

/// Extract a call argument's value expression and the selector that
/// identifies which parameter it fills. `next_positional` tracks the
/// running position of positional arguments (PHP requires positional
/// arguments to precede named ones, so this stays aligned with the
/// parameter list).
pub(crate) fn arg_expr_and_selector<'b>(
    arg: &'b Argument<'b>,
    next_positional: &mut usize,
) -> (&'b Expression<'b>, ArgSelector) {
    match arg {
        Argument::Positional(pos) => {
            let selector = ArgSelector::Position(*next_positional);
            *next_positional += 1;
            (pos.value, selector)
        }
        Argument::Named(named) => (
            named.value,
            ArgSelector::Name(bytes_to_str(named.name.value).to_string()),
        ),
    }
}

/// The `use (&$x)` variables a closure captures by reference, in
/// declaration order.
pub(crate) fn by_ref_captured_names(closure: &Closure<'_>) -> Vec<String> {
    closure
        .use_clause
        .as_ref()
        .map(|use_clause| {
            use_clause
                .variables
                .iter()
                .filter(|use_var| use_var.ampersand.is_some())
                .map(|use_var| bytes_to_str(use_var.variable.name).to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Refine a closure's `use (&$x)` entry types to account for the body
/// reassigning them.
///
/// The closure may run any number of times relative to whatever point is
/// being resolved (before, after, or several times around it), so a read
/// anywhere in the body — including its very first statement — must see
/// not just what the capture held before the closure literal, but also
/// whatever a previous run of the body could have left it holding:
///
/// ```php
/// $a = 0;
/// $cb = function () use (&$a): void {
///     $a; // 0|'s' — a previous run may already have assigned 's'
///     $a = 's';
/// };
/// ```
///
/// This is exactly the loop fixed-point problem
/// (`walk_loop_body_to_fixed_point`) with the closure body standing in
/// for a loop body of unknown trip count: re-walk a cursor-suppressed
/// copy of the body, union each capture's exit types back into its entry,
/// and repeat until nothing new appears. Capped by the body's own
/// assignment-dependency depth, exactly as a loop body is.
pub(crate) fn seed_by_ref_capture_fixed_point<'b>(
    closure: &'b Closure<'b>,
    closure_scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    captured: &[String],
) {
    if captured.is_empty() {
        return;
    }
    let body_stmts: Vec<&Statement<'_>> = closure.body.statements.iter().collect();
    let depth = assignment_map_depth(&body_stmts);
    if depth <= 1 {
        return;
    }

    let discovery_ctx = ctx.with_cursor_offset(u32::MAX);
    let seed_scope = closure_scope.clone();
    let mut entry_scope = seed_scope.clone();

    for _ in 0..depth.saturating_sub(1) {
        let mut probe = entry_scope.clone();
        let return_frame = push_return_frame(discovery_ctx.flow);
        walk_body_forward(body_stmts.iter().copied(), &mut probe, &discovery_ctx);
        if let Some(returned) = return_frame.finish() {
            probe.merge_branch(&returned);
        }

        let mut next_entry = seed_scope.clone();
        let mut changed = false;
        for var_name in captured {
            let mut combined = seed_scope.get(var_name).to_vec();
            ResolvedType::extend_unique(&mut combined, probe.get(var_name).to_vec());
            if resolved_types_differ(&combined, entry_scope.get(var_name)) {
                changed = true;
            }
            next_entry.set(var_name, combined);
        }
        entry_scope = next_entry;
        if !changed {
            break;
        }
    }

    *closure_scope = entry_scope;
}

/// Walk a closure body and propagate the types it assigns to `use (&$x)`
/// captures back into the outer scope.
///
/// `invoked_immediately` decides how: when the closure provably runs
/// before the call returns, the closure's final state *replaces* the
/// outer variable; otherwise the closure may run zero or more times at
/// any later point, so the assigned types are *unioned* with the outer
/// types (mirroring PHPStan, which widens by-ref captures even for
/// closures that are merely defined).
///
/// `runs_once` is narrower: only a closure called where it is written
/// runs exactly once.  One handed to a call that invokes it immediately
/// (`array_map`, `array_walk`) may still run once per element, so its body
/// is walked the way a loop body is and an append inside it builds the
/// collection instead of a one-entry shape.
pub(crate) fn process_by_ref_closure_capture<'b>(
    closure: &'b Closure<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    invoked_immediately: bool,
    runs_once: bool,
) {
    let captured = by_ref_captured_names(closure);
    if captured.is_empty() {
        return;
    }

    let full_ctx = ctx
        .with_cursor_offset(u32::MAX)
        .with_in_loop(ctx.in_loop || !runs_once);
    let mut closure_scope = ScopeState::new();

    seed_closure_captures(&mut closure_scope, scope, closure.use_clause.as_ref());

    seed_closure_params(
        &mut closure_scope,
        &closure.parameter_list,
        closure.span().start.offset,
        &[],
        &full_ctx,
    );

    seed_by_ref_capture_fixed_point(closure, &mut closure_scope, &full_ctx, &captured);

    let return_frame = push_return_frame(full_ctx.flow);
    walk_body_forward(
        closure.body.statements.iter(),
        &mut closure_scope,
        &full_ctx,
    );
    // Every `return` in the body is an exit of the closure just as much as
    // falling off its end is, and a capture written on a returning path is
    // still written.  `walk_body_forward` leaves only the fall-through
    // state behind, so the returning paths are folded back in here.
    if let Some(returned) = return_frame.finish() {
        closure_scope.merge_branch(&returned);
    }

    for var_name in captured {
        scope.invalidate_dependent_keys(&var_name);
        scope.invalidate_proofs(&var_name);
        let types = closure_scope.get(&var_name).to_vec();
        if !types.is_empty() {
            if invoked_immediately {
                scope.set(&var_name, types);
            } else {
                let mut combined = scope.get(&var_name).to_vec();
                ResolvedType::extend_unique(&mut combined, types);
                scope.set(&var_name, combined);
            }
        } else if closure_scope.contains(&var_name) {
            scope.set_empty(&var_name);
        }
    }
}

/// Write what the call `expr` hands back through the variables it passes
/// by reference, once its arguments have been evaluated.
///
/// The expression walk calls this at every call it evaluates, so a write
/// lands where the call happens: `preg_match('/(a)/', $s, $m) && $m[1]`
/// reads the `$m` the call filled, and in `$file = end($file);` the
/// assignment that follows the call is what `$file` ends up holding.
pub(crate) fn process_pass_by_ref<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Only a variable passed as an argument can be written through a
    // by-reference parameter, so a call without one is left alone before
    // anything is looked up.  `extract()` is the exception: it writes
    // locals named by its argument's keys, whatever the argument is.
    if !passes_a_variable(expr) && !is_extract_call(expr) {
        return;
    }
    if !super::array_assignment::process_array_push_call(expr, scope, ctx)
        && !super::array_assignment::process_array_cursor_call(expr, scope, ctx)
        && !super::array_assignment::process_array_sort_call(expr, scope)
        && !process_extract_call(expr, scope, ctx)
    {
        apply_by_ref_parameter_types(expr, scope, ctx);
    }
}

/// A call that can write through by-reference parameters: the arguments it
/// passes, the parameters they bind to, and the callee those belong to,
/// which an out type read from the callee's body or `@param-out` needs.
pub(super) struct ByRefCall<'b> {
    pub(super) arguments: &'b ArgumentList<'b>,
    pub(super) parameters: crate::types::SharedVec<ParameterInfo>,
    pub(super) callee: OutParamCallee,
}

/// Give the variables a call passes by reference the types its parameters
/// leave in them.
fn apply_by_ref_parameter_types<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // `preg_match`'s `$matches` is typed from the pattern, which needs no
    // callee, and it has no other by-reference parameter beside it, so
    // nothing below is left to do once the pattern has typed it.
    if seed_preg_matches(expr, scope, ctx) {
        return;
    }
    if let Some(call) = by_ref_call(expr, scope, ctx) {
        seed_by_ref_parameters(expr, &call, scope, ctx);
    }
}

/// The call `expr` makes, for the by-reference passes: a function, or a
/// method or constructor on whichever class the receiver names first that
/// declares it.
fn by_ref_call<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<ByRefCall<'b>> {
    match expr {
        Expression::Call(Call::Function(call)) => {
            let Expression::Identifier(ident) = call.function else {
                return None;
            };
            let function_loader = ctx.loaders.function_loader?;
            let function = function_loader(
                bytes_to_str(ident.value()),
                call.function.span().start.offset,
            )?;
            Some(ByRefCall {
                arguments: &call.argument_list,
                parameters: function.parameters.clone(),
                callee: OutParamCallee::Function(Box::new(function)),
            })
        }
        Expression::Call(Call::Method(call)) => named_method_call(
            &call.argument_list,
            &call.method,
            &receiver_classes(call.object, scope, ctx),
            ctx,
        ),
        Expression::Call(Call::NullSafeMethod(call)) => named_method_call(
            &call.argument_list,
            &call.method,
            &receiver_classes(call.object, scope, ctx),
            ctx,
        ),
        Expression::Call(Call::StaticMethod(call)) => named_method_call(
            &call.argument_list,
            &call.method,
            &static_receiver_classes(call.class, scope, ctx),
            ctx,
        ),
        Expression::Instantiation(new) => method_call_on(
            new.argument_list.as_ref()?,
            "__construct",
            &static_receiver_classes(new.class, scope, ctx),
            ctx,
        ),
        _ => None,
    }
}

/// The method a call's `selector` names on the first of `classes` that has
/// it, with the arguments the call passes it.
fn named_method_call<'b>(
    arguments: &'b ArgumentList<'b>,
    selector: &ClassLikeMemberSelector<'_>,
    classes: &[Arc<ClassInfo>],
    ctx: &ForwardWalkCtx<'_>,
) -> Option<ByRefCall<'b>> {
    let ClassLikeMemberSelector::Identifier(ident) = selector else {
        return None;
    };
    method_call_on(arguments, bytes_to_str(ident.value), classes, ctx)
}

/// `method_name` on the first of `classes` that has it, once everything the
/// class inherits is merged in, with the arguments the call passes it.
fn method_call_on<'b>(
    arguments: &'b ArgumentList<'b>,
    method_name: &str,
    classes: &[Arc<ClassInfo>],
    ctx: &ForwardWalkCtx<'_>,
) -> Option<ByRefCall<'b>> {
    classes.iter().find_map(|class| {
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            class,
            ctx.file.class_loader,
            ctx.resolved_class_cache,
        );
        let parameters = merged.get_method(method_name)?.parameters.clone();
        Some(ByRefCall {
            arguments,
            parameters,
            callee: OutParamCallee::Method(merged, atom(method_name)),
        })
    })
}

/// Whether the call `expr` passes a plain variable as one of its arguments
/// (`$matches` in `preg_match($pattern, $subject, $matches)`), the only
/// kind of argument a by-reference parameter can write through.
fn passes_a_variable(expr: &Expression<'_>) -> bool {
    let arg_list = match expr {
        Expression::Call(Call::Function(fc)) => &fc.argument_list,
        Expression::Call(Call::Method(mc)) => &mc.argument_list,
        Expression::Call(Call::NullSafeMethod(mc)) => &mc.argument_list,
        Expression::Call(Call::StaticMethod(sc)) => &sc.argument_list,
        Expression::Instantiation(inst) => match &inst.argument_list {
            Some(al) => al,
            None => return false,
        },
        _ => return false,
    };
    arg_list
        .arguments
        .iter()
        .any(|arg| matches!(arg.value(), Expression::Variable(Variable::Direct(_))))
}
