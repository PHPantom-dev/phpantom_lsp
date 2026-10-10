//! By-reference parameters and callable arguments.
//!
//! Seeds the variables a call writes through a `&$ref` parameter, and the
//! ones a callback's parameters bind when the callee invokes it before
//! returning.

use super::*;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;
use mago_syntax::walker::Walker;

use crate::atom::{atom, bytes_to_str};
use crate::parser::with_parsed_program;
use crate::php_type::{PhpType, ShapeEntry, TypeKind};
use crate::type_engine::call_resolution::{
    OutParamCallee, effective_out_type, resolve_out_type_for_call,
};
use crate::types::ResolvedType;

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
                    let certain = call_invokes_arg_immediately(call, &selector, scope, ctx);
                    process_by_ref_closure_capture(closure, scope, ctx, certain, false);
                } else {
                    process_by_ref_closure_captures(arg_expr, scope, ctx);
                }
            }

            // `$write()` on a variable still holding the closure literal it
            // was assigned runs that body here, after the arguments, just
            // as the immediately invoked closure above does.
            if let Call::Function(fc) = call
                && let Expression::Variable(Variable::Direct(var)) =
                    crate::parser::unwrap_parens(fc.function)
            {
                invoke_closure_literal_held_by(bytes_to_str(var.name), scope, ctx);
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

/// Run the by-reference capturing closure literal `var_name` holds, if
/// [`ScopeState::closure_literal_offset`] says it holds one.
///
/// The scope only keeps the closure's offset, since it outlives the
/// syntax tree being walked, so the closure is found again in the parsed
/// file.  Not finding it leaves the captures as the definition widened
/// them.
fn invoke_closure_literal_held_by(
    var_name: &str,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let Some(offset) = scope.closure_literal_offset(var_name) else {
        return;
    };
    with_parsed_program(ctx.content, "invoke_closure_literal", |program, _| {
        let mut found = None;
        for stmt in program.statements.iter() {
            Walker::walk_statement(&ClosureAtOffset(offset), stmt, &mut found);
            if found.is_some() {
                break;
            }
        }
        if let Some(closure) = found {
            process_by_ref_closure_capture(closure, scope, ctx, true, true);
        }
    });
}

/// Finds the closure literal starting at an offset.
struct ClosureAtOffset(u32);

impl<'ast, 'arena> Walker<'ast, 'arena, Option<&'ast Closure<'arena>>> for ClosureAtOffset {
    fn walk_in_closure(
        &self,
        closure: &'ast Closure<'arena>,
        found: &mut Option<&'ast Closure<'arena>>,
    ) {
        if closure.span().start.offset == self.0 {
            *found = Some(closure);
        }
    }
}

/// Whether a call provably invokes its callable argument before
/// returning, so a by-ref capture's final state can *replace* the outer
/// variable rather than widen it.
///
/// Follows PHPStan's defaults: function callable parameters are
/// immediate unless tagged `@param-later-invoked-callable`; method
/// callable parameters are later-invoked unless tagged
/// `@param-immediately-invoked-callable`.  An unresolvable callee or
/// receiver is not proof either way, so it answers `false` and the
/// caller falls back to widening.
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
            function_invokes_callable_arg_immediately(bytes_to_str(ident.value()), selector, ctx)
        }
        Call::Method(mc) => {
            let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
                return false;
            };
            let receiver_names = receiver_class_names(mc.object, scope, ctx);
            !receiver_names.is_empty()
                && method_invokes_callable_arg_immediately(
                    &receiver_names,
                    bytes_to_str(ident.value),
                    selector,
                    ctx,
                )
        }
        Call::NullSafeMethod(mc) => {
            let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
                return false;
            };
            let receiver_names = receiver_class_names(mc.object, scope, ctx);
            !receiver_names.is_empty()
                && method_invokes_callable_arg_immediately(
                    &receiver_names,
                    bytes_to_str(ident.value),
                    selector,
                    ctx,
                )
        }
        Call::StaticMethod(sc) => {
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return false;
            };
            let receiver_names = static_receiver_class_names(sc.class, ctx);
            !receiver_names.is_empty()
                && method_invokes_callable_arg_immediately(
                    &receiver_names,
                    bytes_to_str(ident.value),
                    selector,
                    ctx,
                )
        }
    }
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

/// Find the callee parameter that a call argument fills, honouring both
/// positional and named binding.
pub(crate) fn select_param<'p>(
    parameters: impl Iterator<Item = &'p FunctionLikeParameter<'p>>,
    selector: &ArgSelector,
) -> Option<&'p FunctionLikeParameter<'p>> {
    match selector {
        ArgSelector::Position(idx) => parameters.into_iter().nth(*idx),
        ArgSelector::Name(name) => parameters
            .into_iter()
            .find(|param| bytes_to_str(param.variable.name).trim_start_matches('$') == name),
    }
}

/// Builtin functions PHPStan's own stubs tag `@param-later-invoked-callable`
/// (`stubs/core.stub` upstream): the callback is stashed away to run later,
/// rather than run before the call returns.
///
/// Every other builtin that takes a callable — `call_user_func`,
/// `call_user_func_array`, `array_map`, `usort`, and the rest — runs it
/// immediately, which is the function-parameter default
/// [`function_invokes_callable_arg_immediately`] falls back to below for a
/// callee the function loader can still resolve even though it is not
/// declared in the current file.
const LATER_INVOKED_CALLABLE_FUNCTIONS: &[&str] = &[
    "pcntl_signal",
    "set_error_handler",
    "set_exception_handler",
    "spl_autoload_register",
    "register_shutdown_function",
    "header_register_callback",
    "register_tick_function",
];

pub(crate) fn function_invokes_callable_arg_immediately(
    func_name: &str,
    selector: &ArgSelector,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let func_name = crate::util::strip_fqn_prefix(func_name);
    with_parsed_program(
        ctx.content,
        "function_invokes_callable_arg",
        |program, _| {
            let mut stmts = Vec::new();
            flatten_namespaced_statements(program.statements.iter(), &mut stmts);
            let declared = stmts.into_iter().find_map(|stmt| {
                if let Statement::Function(func) = stmt
                    && bytes_to_str(func.name.value).eq_ignore_ascii_case(func_name)
                {
                    Some(func)
                } else {
                    None
                }
            });
            let Some(func) = declared else {
                // Not declared in this file: a builtin still has a
                // signature the function loader can find (backed by the
                // embedded stubs), and only a callee that genuinely
                // resolves is one whose immediacy has a documented
                // default to fall back to. A name that resolves to
                // nothing gives no signal either way, the same as an
                // unresolvable receiver — `outer(inner(fn () use (&$x)
                // {...}))` for undefined `outer`/`inner` stays uncertain.
                return ctx
                    .loaders
                    .function_loader
                    .is_some_and(|loader| loader(func_name, 0).is_some())
                    && !LATER_INVOKED_CALLABLE_FUNCTIONS
                        .iter()
                        .any(|later_invoked| later_invoked.eq_ignore_ascii_case(func_name));
            };
            let Some(param) = select_param(func.parameter_list.parameters.iter(), selector) else {
                return false;
            };
            !node_param_has_invocation_tag(
                func.name.span.start.offset as usize,
                ctx.content,
                bytes_to_str(param.variable.name),
                "param-later-invoked-callable",
            )
        },
    )
}

pub(crate) fn method_invokes_callable_arg_immediately(
    receiver_names: &[String],
    method_name: &str,
    selector: &ArgSelector,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    with_parsed_program(ctx.content, "method_invokes_callable_arg", |program, _| {
        let mut stmts = Vec::new();
        flatten_namespaced_statements(program.statements.iter(), &mut stmts);
        stmts.into_iter().any(|stmt| {
            let members = match stmt {
                Statement::Class(class)
                    if class_name_matches_receiver(class.name.value, receiver_names) =>
                {
                    Some(class.members.iter())
                }
                _ => None,
            };

            let Some(members) = members else {
                return false;
            };

            members.into_iter().any(|member| {
                if let ClassLikeMember::Method(method) = member
                    && bytes_to_str(method.name.value).eq_ignore_ascii_case(method_name)
                {
                    let Some(param) =
                        select_param(method.parameter_list.parameters.iter(), selector)
                    else {
                        return false;
                    };
                    return node_param_has_invocation_tag(
                        method.name.span.start.offset as usize,
                        ctx.content,
                        bytes_to_str(param.variable.name),
                        "param-immediately-invoked-callable",
                    );
                }
                false
            })
        })
    })
}

pub(crate) fn node_param_has_invocation_tag(
    node_start: usize,
    content: &str,
    param_name: &str,
    tag_name: &str,
) -> bool {
    let Some(docblock) = preceding_docblock_text(content, node_start) else {
        return false;
    };
    docblock.lines().any(|line| {
        let line = line
            .trim()
            .trim_start_matches("/**")
            .trim_start_matches('*')
            .trim_end_matches("*/")
            .trim();
        line.starts_with(&format!("@{tag_name}"))
            && line
                .split_whitespace()
                .any(|part| part.trim_matches(',') == param_name)
    })
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
        let return_frame = push_return_frame();
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

    let return_frame = push_return_frame();
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
    if extract_call_arg_variables(expr).is_empty() && !is_extract_call(expr) {
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

/// Whether `expr` is a call to `extract()`.
fn is_extract_call(expr: &Expression<'_>) -> bool {
    let Expression::Call(Call::Function(call)) = expr else {
        return false;
    };
    let Expression::Identifier(ident) = call.function else {
        return false;
    };
    bytes_to_str(ident.value())
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("extract")
}

/// How `extract()` treats a key, from its `flags` argument (with the
/// `EXTR_REFS` bit, which changes no types, masked off).
#[derive(Clone, Copy, PartialEq)]
enum ExtractMode {
    Overwrite,
    Skip,
    PrefixSame,
    PrefixAll,
    PrefixInvalid,
    PrefixIfExists,
    IfExists,
}

impl ExtractMode {
    fn from_flags(flags: i64) -> Option<Self> {
        Some(match flags & !256 {
            0 => Self::Overwrite,
            1 => Self::Skip,
            2 => Self::PrefixSame,
            3 => Self::PrefixAll,
            4 => Self::PrefixInvalid,
            5 => Self::PrefixIfExists,
            6 => Self::IfExists,
            _ => return None,
        })
    }

    fn is_prefixed(self) -> bool {
        matches!(
            self,
            Self::PrefixSame | Self::PrefixAll | Self::PrefixInvalid | Self::PrefixIfExists
        )
    }

    /// The local a key is written to, if any.  `exists` says whether a
    /// local of that name is in scope, which is what the conditional
    /// modes test.
    fn target(self, key: &str, prefix: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
        let prefixed = || format!("{prefix}_{key}");
        let name = match self {
            Self::Overwrite => key.to_string(),
            Self::Skip if exists(key) => return None,
            Self::Skip => key.to_string(),
            Self::PrefixSame if is_extractable_variable_name(key) && exists(key) => prefixed(),
            Self::PrefixSame => key.to_string(),
            Self::PrefixAll => prefixed(),
            Self::PrefixInvalid if is_extractable_variable_name(key) => key.to_string(),
            Self::PrefixInvalid => prefixed(),
            Self::PrefixIfExists if exists(key) => prefixed(),
            Self::IfExists if exists(key) => key.to_string(),
            Self::PrefixIfExists | Self::IfExists => return None,
        };
        is_extractable_variable_name(&name).then_some(name)
    }

    /// Whether a call whose keys are not known may write the local `name`.
    fn may_write_any(self, name: &str, prefix: &str) -> bool {
        match self {
            Self::Skip => false,
            // A key that collides is written under the prefix, and one that
            // does not names a variable that is not there yet.
            Self::PrefixSame | Self::PrefixAll | Self::PrefixIfExists => name
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('_')),
            Self::Overwrite | Self::PrefixInvalid | Self::IfExists => true,
        }
    }
}

/// The value of an `extract()` flags argument: `EXTR_*` constants and
/// integer literals, combined with `|`.
fn extract_flags_value(expr: &Expression<'_>) -> Option<i64> {
    match expr {
        Expression::Parenthesized(inner) => extract_flags_value(inner.expression),
        Expression::Binary(binary) if matches!(binary.operator, BinaryOperator::BitwiseOr(_)) => {
            Some(extract_flags_value(binary.lhs)? | extract_flags_value(binary.rhs)?)
        }
        Expression::Literal(Literal::Integer(int)) => int.value.map(|v| v as i64),
        Expression::ConstantAccess(access) => {
            let name = bytes_to_str(access.name.value());
            Some(match name.rsplit('\\').next().unwrap_or(name) {
                "EXTR_OVERWRITE" => 0,
                "EXTR_SKIP" => 1,
                "EXTR_PREFIX_SAME" => 2,
                "EXTR_PREFIX_ALL" => 3,
                "EXTR_PREFIX_INVALID" => 4,
                "EXTR_PREFIX_IF_EXISTS" => 5,
                "EXTR_IF_EXISTS" => 6,
                "EXTR_REFS" => 256,
                _ => return None,
            })
        }
        _ => None,
    }
}

/// Define the locals an `extract()` call writes.
///
/// An array shape writes one local per key, under the name and on the
/// condition its flags say, so a required key replaces whatever the
/// variable held and an optional one may.  An array whose keys are not
/// known may write any local the flags allow with anything, so each of
/// those widens to `mixed`.  Returns whether the call was an `extract()`.
fn process_extract_call<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    if !is_extract_call(expr) {
        return false;
    }
    let Expression::Call(Call::Function(call)) = expr else {
        return false;
    };
    let mut array_arg = None;
    let mut flags_arg = None;
    let mut prefix_arg = None;
    let mut unpacked = false;
    let mut next_positional = 0;
    for arg in call.argument_list.arguments.iter() {
        if let Argument::Positional(pos) = arg
            && pos.ellipsis.is_some()
        {
            unpacked = true;
        }
        let (value, selector) = arg_expr_and_selector(arg, &mut next_positional);
        let slot = match selector {
            ArgSelector::Position(0) => &mut array_arg,
            ArgSelector::Position(1) => &mut flags_arg,
            ArgSelector::Position(2) => &mut prefix_arg,
            ArgSelector::Name(name) if name == "array" => &mut array_arg,
            ArgSelector::Name(name) if name == "flags" => &mut flags_arg,
            ArgSelector::Name(name) if name == "prefix" => &mut prefix_arg,
            _ => continue,
        };
        *slot = Some(value);
    }
    let Some(array_arg) = array_arg else {
        return true;
    };
    let mode = match flags_arg {
        None => Some(ExtractMode::Overwrite),
        Some(flags) => extract_flags_value(flags).and_then(ExtractMode::from_flags),
    };
    let prefix = prefix_arg.and_then(crate::type_engine::types::narrowing::string_literal_value);
    let mode = match mode {
        Some(mode) if !unpacked && (prefix.is_some() || !mode.is_prefixed()) => mode,
        _ => {
            widen_extractable_locals(scope, |_| true);
            return true;
        }
    };
    let prefix = prefix.as_deref().unwrap_or("");

    let types = super::assignment::resolve_rhs_with_scope(array_arg, scope, ctx);
    let array = ResolvedType::types_joined(&types);
    // Each alternative of a union of shapes writes its own keys, so a key
    // the others lack is only possibly written.
    let shapes: Option<Vec<&[ShapeEntry]>> = match array.kind() {
        _ if types.is_empty() => None,
        TypeKind::ArrayShape(entries) => Some(vec![&entries[..]]),
        TypeKind::Union(members) => members
            .iter()
            .map(|m| match m.kind() {
                TypeKind::ArrayShape(entries) => Some(&entries[..]),
                _ => None,
            })
            .collect(),
        _ => None,
    };
    let Some(shapes) = shapes else {
        widen_extractable_locals(scope, |name| mode.may_write_any(name, prefix));
        return true;
    };
    let exists = |name: &str| scope.contains(&format!("${name}"));
    // (name, value, optional, alternatives writing it), in key order.
    let mut written: Vec<(String, Vec<PhpType>, bool, usize)> = Vec::new();
    for entries in &shapes {
        for (position, entry) in entries.iter().enumerate() {
            let key = match entry.key.as_deref() {
                Some(key) => key.trim_matches(|c| c == '\'' || c == '"').to_string(),
                None => position.to_string(),
            };
            let Some(name) = mode.target(&key, prefix, exists) else {
                continue;
            };
            match written.iter_mut().find(|(n, ..)| *n == name) {
                Some((_, values, optional, count)) => {
                    values.push(entry.value_type.clone());
                    *optional |= entry.optional;
                    *count += 1;
                }
                None => written.push((name, vec![entry.value_type.clone()], entry.optional, 1)),
            }
        }
    }
    for (name, mut values, optional, count) in written {
        let var_name = format!("${name}");
        if optional || count < shapes.len() {
            let existing = scope.get(&var_name);
            if !existing.is_empty() {
                values.insert(0, ResolvedType::types_joined(existing));
            }
        }
        scope.invalidate_dependent_keys(&var_name);
        scope.invalidate_proofs(&var_name);
        scope.set(
            &var_name,
            vec![ResolvedType::from_type_string(PhpType::union(values))],
        );
    }
    true
}

/// Widen to `mixed` every local an `extract()` of unknown keys may have
/// written, as `may_write` (given the name without its `$`) allows.
fn widen_extractable_locals(scope: &mut ScopeState, may_write: impl Fn(&str) -> bool) {
    let names: Vec<String> = scope
        .locals
        .keys()
        .filter_map(|key| key.strip_prefix('$'))
        .filter(|name| is_extractable_variable_name(name) && may_write(name))
        .map(|name| format!("${name}"))
        .collect();
    for var_name in names {
        scope.invalidate_dependent_keys(&var_name);
        scope.invalidate_proofs(&var_name);
        scope.set(
            &var_name,
            vec![ResolvedType::from_type_string(PhpType::mixed())],
        );
    }
}

/// Whether `extract()` turns an array key into a local: a valid PHP
/// variable name other than `this`, which it refuses to overwrite.
fn is_extractable_variable_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic() || first >= 0x80)
        && bytes.all(|b| b == b'_' || b.is_ascii_alphanumeric() || b >= 0x80)
        && name != "this"
}

/// Give the variables a call passes by reference the types its parameters
/// declare for them.
fn apply_by_ref_parameter_types<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // When a function call passes a variable to a parameter declared
    // as `Type &$param`, the variable acquires that type after the call.
    //
    // We need to check both variables already in scope AND variables
    // that appear as arguments but don't exist in scope yet (e.g.
    // `$matches` in `preg_match($pattern, $subject, $matches)`).
    //
    // Phase 1: use the existing `try_apply_pass_by_reference_type`
    // infrastructure for variables already in scope (works for class
    // types like `Type &$param`).
    let scope_resolver = scope.snapshot_resolver();

    // Only a variable passed directly as an argument of this call can be
    // written through a by-reference parameter, so those are the only
    // candidates, whether or not they are in scope yet.  Visiting every
    // local instead would redo the callee lookup once per variable and
    // make a long body cost statements × locals.
    let mut arg_var_names: Vec<String> = Vec::new();
    for arg_var in extract_call_arg_variables(expr) {
        if !arg_var_names.contains(&arg_var) {
            arg_var_names.push(arg_var);
        }
    }

    for var_name in arg_var_names {
        let var_ctx = ctx.var_ctx_for_with_scope(
            &var_name,
            ctx.cursor_offset,
            &scope_resolver,
            Some(scope.proofs()),
        );
        let before = scope.get(&var_name).to_vec();
        let mut results = before.clone();
        super::super::resolution::try_apply_pass_by_reference_type(
            expr,
            &var_ctx,
            &mut results,
            false,
        );
        if resolved_types_differ(&results, &before) {
            scope.set(&var_name, results);
        }
    }

    // Phase 2: for variables NOT yet in scope that are passed to
    // pass-by-reference parameters with primitive type hints (e.g.
    // `array &$matches` in `preg_match`), store the type hint
    // directly.  `try_apply_pass_by_reference_type` only produces
    // results for class-based type hints; primitive types like
    // `array`, `int`, `string` return empty from
    // `type_hint_to_classes_typed` and are missed.
    seed_pass_by_ref_primitives(expr, scope, ctx);
}

/// The callee an instance method call names, with the parameters the
/// out-param pass binds its arguments to.
///
/// The receiver has to be a variable whose class is already known: `$this`
/// names the enclosing class, and any other variable names whatever class
/// the scope recorded for it. A scalar is not a receiver, so a variable
/// holding one yields `None` rather than a class named `string`.
fn instance_method_callee<'b>(
    object: &'b Expression<'b>,
    selector: &ClassLikeMemberSelector<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<(
    crate::types::SharedVec<crate::types::ParameterInfo>,
    OutParamCallee,
)> {
    let ClassLikeMemberSelector::Identifier(ident) = selector else {
        return None;
    };
    let method_name = bytes_to_str(ident.value).to_string();

    let class_name = match object {
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
            ctx.current_class.name.to_string()
        }
        Expression::Variable(Variable::Direct(dv)) => {
            scope.get(bytes_to_str(dv.name)).iter().find_map(|rt| {
                let name = rt.type_string.base_name()?;
                (!crate::php_type::is_primitive_scalar_name(name)).then(|| name.to_string())
            })?
        }
        _ => return None,
    };

    resolved_method_callee(&class_name, &method_name, ctx)
}

/// Load `class_name`, merge everything it inherits, and look `method_name`
/// up on the result.
fn resolved_method_callee(
    class_name: &str,
    method_name: &str,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<(
    crate::types::SharedVec<crate::types::ParameterInfo>,
    OutParamCallee,
)> {
    let cls = (ctx.class_loader)(class_name)?;
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        &cls,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    let parameters = merged.get_method(method_name)?.parameters.clone();
    Some((
        parameters,
        OutParamCallee::Method(merged, atom(method_name)),
    ))
}

/// Whether `value_expr` is passed to the call via `...value_expr` (argument
/// unpacking), rather than as a direct positional or named argument.
fn arg_is_unpacked<'b>(arg_list: &ArgumentList<'b>, value_expr: &Expression<'b>) -> bool {
    arg_list.arguments.iter().any(|arg| {
        matches!(arg, Argument::Positional(pos) if pos.ellipsis.is_some() && std::ptr::eq(pos.value, value_expr))
    })
}

/// For each variable argument in a call expression that is passed to a
/// pass-by-reference parameter with a primitive type hint (e.g.
/// `array &$matches`), seed or refresh the variable in scope. Existing exact
/// values must be invalidated because the callee may assign any value allowed
/// by the parameter type. This complements [`process_pass_by_ref`] which
/// handles class-typed parameters via `try_apply_pass_by_reference_type`.
pub(crate) fn seed_pass_by_ref_primitives<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // `preg_match`'s `$matches` has no other by-reference parameter beside
    // it, so nothing below is left to do once the pattern has typed it.
    if seed_preg_matches(expr, scope, ctx) {
        return;
    }
    // Moving the internal pointer leaves the array's value as it was, so
    // the `array|object` hint describes nothing the call wrote.
    if super::array_assignment::is_array_pointer_call(expr) {
        return;
    }

    // Resolve the called function/method's parameters.
    let (arg_list, parameters, template_owner) = match expr {
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let func_name_offset = func_call.function.span().start.offset;
            let fl = match ctx.loaders.function_loader {
                Some(fl) => fl,
                None => return,
            };
            let func_info = match fl(&func_name, func_name_offset) {
                Some(fi) => fi,
                None => return,
            };
            let parameters = func_info.parameters.clone();
            (
                &func_call.argument_list,
                parameters,
                OutParamCallee::Function(Box::new(func_info)),
            )
        }
        Expression::Call(Call::Method(mc)) => {
            let Some((parameters, callee)) =
                instance_method_callee(mc.object, &mc.method, scope, ctx)
            else {
                return;
            };
            (&mc.argument_list, parameters, callee)
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            let Some((parameters, callee)) =
                instance_method_callee(mc.object, &mc.method, scope, ctx)
            else {
                return;
            };
            (&mc.argument_list, parameters, callee)
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            let method_name = match &sc.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let Some(class_name) = crate::class_lookup::class_expression_name(
                sc.class,
                ctx.current_class,
                ctx.all_classes,
                ctx.class_loader,
            ) else {
                return;
            };
            let Some((parameters, callee)) = resolved_method_callee(&class_name, &method_name, ctx)
            else {
                return;
            };
            (&sc.argument_list, parameters, callee)
        }
        _ => return,
    };

    // Bind arguments to parameters following PHP's rules so a named argument
    // seeds the parameter it actually targets, not the one at its ordinal
    // position in the call.
    let bound = crate::call_args::bind_args_to_params(&parameters, arg_list);

    // An out type written in the callee's own `@template` params
    // (`usort`'s `array<TKey, TValue> &$array`) describes the caller's
    // variable only once those params are bound, and this pass has no
    // binding for them. Applied raw it would replace a precise argument
    // type (`TargetClass<ContainerExtension>[]`) with an array of a name
    // nothing can resolve, so the variable is left as it was instead.
    let callee_templates = template_owner.template_params();

    for (param_index, (param, arg_expr)) in parameters.iter().zip(bound.iter()).enumerate() {
        let arg_expr = match arg_expr {
            Some(expr) => *expr,
            None => continue,
        };

        // Only handle direct variable arguments.
        let var_name = match arg_expr {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => continue,
        };

        // Check if the corresponding parameter is pass-by-reference.
        if !param.is_reference {
            continue;
        }

        // `example(...$z)` unpacks each element of `$z` into a separate
        // by-ref argument; the callee writes through the *elements*, not
        // through `$z` itself, so `$z` stays an array whose values take
        // the parameter's type rather than becoming that type directly.
        let is_spread = param.is_variadic && arg_is_unpacked(arg_list, arg_expr);

        let already_in_scope = !scope.get(&var_name).is_empty();
        let mut seeded = false;
        if let Some(out_hint) = effective_out_type(param, param_index, &template_owner, ctx.backend)
        {
            if out_hint.references_any_name(callee_templates) {
                continue;
            }
            // A PHPStan conditional out type (`@param-out ($arg is null ?
            // A&I : A) $arg`) is call-site-agnostic up to this point; only
            // this call's own arguments say which branch it actually takes.
            // Snapshotting clones the scope, so it is only paid for a
            // hint that actually has a condition to decide.
            let out_hint = if !matches!(out_hint.kind(), TypeKind::Conditional(_)) {
                out_hint
            } else {
                let scope_resolver = scope.snapshot_resolver();
                let var_ctx = ctx.var_ctx_for_with_scope(
                    "",
                    ctx.cursor_offset,
                    &scope_resolver,
                    Some(scope.proofs()),
                );
                let call_var_resolver =
                    super::super::resolution::build_var_resolver_from_ctx(&var_ctx);
                resolve_out_type_for_call(
                    out_hint,
                    &parameters,
                    &template_owner,
                    arg_list,
                    ctx.content,
                    &var_ctx.as_resolution_ctx(),
                    Some(&call_var_resolver),
                )
            };
            // A variadic parameter's stored PHPDoc type may describe the
            // collected argument array (`string[] &$values`), while each
            // call-site variable is one element of that collection. Native
            // element hints such as `string &...$values` are already scalar
            // and therefore pass through unchanged.
            let effective_hint = if param.is_variadic {
                out_hint
                    .iterable_element_type()
                    .unwrap_or_else(|| out_hint.clone())
            } else {
                out_hint
            };
            let primitive_hint = match effective_hint.kind() {
                TypeKind::Union(members) | TypeKind::Intersection(members) => {
                    !members.is_empty() && members.iter().all(PhpType::is_scalar)
                }
                _ => effective_hint.is_scalar(),
            };
            if primitive_hint && is_spread {
                // The existing array's own element type plays the same role
                // here that the whole variable's type plays below: kept
                // when it already agrees with the hint (minus literal
                // precision), replaced by the hint when it disagrees or is
                // unknown. The array's keys are untouched either way.
                let existing = scope.get(&var_name);
                let existing_joined =
                    (!existing.is_empty()).then(|| ResolvedType::types_joined(existing));
                let key_type = existing_joined
                    .as_ref()
                    .and_then(PhpType::iterable_key_type)
                    .unwrap_or_else(PhpType::int);
                let value_type = existing_joined
                    .as_ref()
                    .and_then(PhpType::iterable_element_type)
                    .filter(|value| !value.is_null())
                    .filter(|value| value.is_subtype_of(&effective_hint))
                    .map(|value| value.widen_scalar_literals())
                    .unwrap_or(effective_hint);
                scope.set(
                    &var_name,
                    vec![ResolvedType::from_type_string(PhpType::generic_array(
                        key_type, value_type,
                    ))],
                );
                seeded = true;
            } else if primitive_hint {
                // The callee may assign any value the parameter type allows,
                // so an exact value observed before the call is stale. What
                // the call cannot invalidate is precision the hint does not
                // contradict: `array_shift(array &$array)` says nothing about
                // the element type of a `Node[]` argument. Keep a value the
                // hint already covers, minus its literal precision, and fall
                // back to the hint only when the two genuinely disagree.
                let existing = scope.get(&var_name);
                let refined = (!existing.is_empty())
                    .then(|| ResolvedType::types_joined(existing))
                    // An empty array shape (`$matches = [];` before
                    // `preg_match_all(…, $matches)`) records no element
                    // precision, only that nothing had been written yet —
                    // which is exactly what the call invalidates. It is a
                    // subtype of every array hint, so without this it would
                    // survive the call and claim the result is still empty.
                    // A bare `null` (`$key = null;` before the call) says the
                    // same thing about a nullable out type, and keeping it
                    // would claim the callee wrote nothing at all.
                    .filter(|existing| !existing.shape_entries().is_some_and(<[_]>::is_empty))
                    .filter(|existing| !existing.is_null())
                    .filter(|existing| existing.is_subtype_of(&effective_hint))
                    .map(|existing| existing.widen_scalar_literals())
                    .unwrap_or(effective_hint);
                scope.set(&var_name, vec![ResolvedType::from_type_string(refined)]);
                seeded = true;
            }
        }
        if !seeded && !already_in_scope && param.type_hint.is_none() {
            // Untyped pass-by-reference parameters (e.g. `&$matches`
            // in `preg_match`, `&$result` in `parse_str`) are most
            // commonly arrays. Seed only new variables as `array`; an
            // existing value has no sounder replacement without a hint,
            // and neither has one the callee's body did not give up.
            scope.set(
                &var_name,
                vec![ResolvedType::from_type_string(PhpType::named(atom(
                    "array",
                )))],
            );
        }
    }
}

/// Type `$matches` from the capture groups of the pattern a
/// `preg_match`/`preg_match_all` call passes.
///
/// The parameter is declared `?array &$matches`, and a bare `array` is all
/// the generic by-reference seeding above can offer. A literal pattern says
/// more: which keys the array has, and which of them a successful match may
/// leave out.
///
/// The call site is not the place that knows whether the match succeeded, so
/// what lands in the scope here is what the call leaves either way. A branch
/// guarded on the outcome narrows it down (see
/// [`apply_preg_match_narrowing`]).
///
/// Returns whether the variable was typed. A pattern the group walk refuses,
/// or a `$flags` argument that does not resolve to a constant, leaves the
/// call to the generic path.
fn seed_preg_matches<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let Some(call) = crate::type_engine::regex_shape::preg_call(expr) else {
        return false;
    };
    let Some(matched) = preg_matched_type(&call, scope, ctx) else {
        return false;
    };
    scope.set(
        call.matches_var,
        vec![ResolvedType::from_type_string(
            crate::type_engine::regex_shape::or_no_match(matched, call.matches_all),
        )],
    );
    true
}

/// The type a *successful* match leaves in the out-parameter of `call`.
///
/// `None` when the analysis refuses the call: a pattern whose group list the
/// walk cannot read, or a `$flags` argument that does not resolve to a
/// constant whose bits the shape analysis models.
pub(crate) fn preg_matched_type<'b>(
    call: &crate::type_engine::regex_shape::PregCall<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let flags = match call.flags {
        None => 0,
        Some(flags) => preg_flag_bits(flags, scope, ctx)?,
    };
    call.pattern
        .as_deref()
        .and_then(|pattern| {
            crate::type_engine::regex_shape::matches_type(pattern, flags, call.matches_all)
        })
        .or_else(|| crate::type_engine::regex_shape::opaque_matches_type(flags, call.matches_all))
}

/// The flag mask a `preg_match` `$flags` argument holds.
///
/// Resolved through the shared pipeline so a named constant, a class
/// constant, and a variable holding one all read the same. Returns `None`
/// when the argument does not resolve to a single integer value, or to one
/// whose bits the shape analysis does not model — the result's shape depends
/// on the flags, so a mask that cannot be read is not one to guess at.
fn preg_flag_bits<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<i64> {
    let scope_resolver = scope.snapshot_resolver();
    let var_ctx =
        ctx.var_ctx_for_with_scope("", ctx.cursor_offset, &scope_resolver, Some(scope.proofs()));
    let flags =
        crate::type_engine::variable::foreach_resolution::resolve_expression_type(expr, &var_ctx)
            .as_ref()
            .and_then(crate::type_engine::types::const_fold::literal_int_value)?;
    crate::type_engine::regex_shape::flags_are_modelled(flags).then_some(flags)
}

/// Extract all `$variable` names that appear as direct arguments in a
/// call expression.  Used by [`process_pass_by_ref`] to discover
/// variables that may be introduced by pass-by-reference parameters
/// (e.g. `$matches` in `preg_match($pattern, $subject, $matches)`).
pub(crate) fn extract_call_arg_variables<'b>(expr: &'b Expression<'b>) -> Vec<String> {
    let arg_list = match expr {
        Expression::Call(Call::Function(fc)) => &fc.argument_list,
        Expression::Call(Call::Method(mc)) => &mc.argument_list,
        Expression::Call(Call::NullSafeMethod(mc)) => &mc.argument_list,
        Expression::Call(Call::StaticMethod(sc)) => &sc.argument_list,
        Expression::Instantiation(inst) => match &inst.argument_list {
            Some(al) => al,
            None => return vec![],
        },
        _ => return vec![],
    };
    let mut vars = Vec::new();
    for arg in arg_list.arguments.iter() {
        let arg_expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
            vars.push(bytes_to_str(dv.name).to_string());
        }
    }
    vars
}
