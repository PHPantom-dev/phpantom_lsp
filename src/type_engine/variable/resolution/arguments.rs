//! What a call does with its arguments: the nth positional argument, an
//! argument's raw type, and the native type a call on an assignment's
//! right-hand side returns.

use super::*;

use mago_span::HasSpan;

use crate::atom::{atom, bytes_to_str};
use crate::php_type::PhpType;
use crate::type_engine::resolver::VarResolutionCtx;
use crate::types::ResolvedType;

/// Extract the "native" return-type string from the RHS of an assignment
/// expression, without resolving it to `ClassInfo`.
///
/// This is used by [`try_inline_var_override`] to feed
/// [`docblock::resolve_effective_type`] with the same kind of parsed
/// `PhpType` that `@return` override checking uses.
///
/// Returns `None` when the native type cannot be determined (the
/// caller should treat this as "unknown", which lets the docblock type
/// win unconditionally).
pub(crate) fn extract_native_type_from_rhs<'b>(
    rhs: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    match rhs {
        // `new ClassName(…)` → the class name.
        Expression::Instantiation(inst) => match inst.class {
            Expression::Identifier(ident) => {
                let fqn = ctx
                    .written_names()
                    .resolve_class(bytes_to_str(ident.value()));
                Some(PhpType::named(atom(&fqn)))
            }
            Expression::Self_(_) => Some(PhpType::named(ctx.current_class.fqn())),
            Expression::Static(_) => Some(PhpType::named(ctx.current_class.fqn())),
            _ => None,
        },
        // Function / method calls → look up the return type.
        Expression::Call(call) => match call {
            Call::Function(func_call) => {
                let func_name = match func_call.function {
                    Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                    _ => None,
                };
                let func_name_offset = func_call.function.span().start.offset;
                func_name.and_then(|name| {
                    ctx.function_loader()
                        .and_then(|fl| fl(&name, func_name_offset))
                        .and_then(|fi| fi.return_type.clone())
                })
            }
            Call::Method(method_call) => {
                if let Expression::Variable(Variable::Direct(dv)) = method_call.object
                    && dv.name == b"$this"
                    && let ClassLikeMemberSelector::Identifier(ident) = &method_call.method
                {
                    let method_name = bytes_to_str(ident.value).to_string();
                    let current = ctx.current_class.fqn();
                    crate::class_lookup::find_class_by_fqn(ctx.all_classes, &current).and_then(
                        |cls| {
                            cls.get_method(&method_name)
                                .and_then(|m| m.return_type.clone())
                        },
                    )
                } else {
                    None
                }
            }
            Call::StaticMethod(static_call) => {
                let class_name = match static_call.class {
                    Expression::Self_(_) | Expression::Static(_) => {
                        Some(ctx.current_class.fqn().to_string())
                    }
                    Expression::Identifier(ident) => Some(
                        ctx.written_names()
                            .resolve_class(bytes_to_str(ident.value())),
                    ),
                    _ => None,
                };
                if let Some(cls_name) = class_name
                    && let ClassLikeMemberSelector::Identifier(ident) = &static_call.method
                {
                    let method_name = bytes_to_str(ident.value).to_string();
                    let owner = crate::class_lookup::find_class_by_fqn(ctx.all_classes, &cls_name)
                        .cloned()
                        .or_else(|| (ctx.class_loader)(&cls_name));
                    owner.and_then(|o| {
                        o.get_method(&method_name)
                            .and_then(|m| m.return_type.clone())
                    })
                } else {
                    None
                }
            }
            _ => None,
        },
        // First-class callable syntax, closure literals, and arrow
        // functions always produce a Closure. When we can infer the
        // body return type, preserve it in a typed `Closure(): T`.
        Expression::PartialApplication(_)
        | Expression::Closure(_)
        | Expression::ArrowFunction(_) => Some(
            super::super::rhs_resolution::infer_closure_literal_type(rhs, ctx),
        ),
        _ => None,
    }
}

// ── Array function type preservation helpers ─────────────────────────

/// Extract the nth positional argument expression (0-based).
pub(in crate::type_engine) fn nth_arg_expr<'b>(
    args: &'b ArgumentList<'b>,
    n: usize,
) -> Option<&'b Expression<'b>> {
    args.arguments.iter().nth(n).map(|arg| match arg {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    })
}

/// The types the forward walker knows for `$var_name` at `offset`,
/// through whichever of its two scope channels is live.
///
/// Argument-type resolution reaches for this before its backward
/// `@var`/`@param` text scan, because the two answer different
/// questions: the scan describes the variable at its *annotation*, while
/// the walker's scope describes it at the point of *use*, carrying
/// whatever the guards in between proved.  `array_slice($cached, …)`
/// inside `if ($cached !== null)` has to slice the non-null half, not the
/// `null|list<…>` written above the assignment.
///
/// The two channels are the same information reaching different
/// consumers: `scope_var_resolver` is the in-progress `ScopeState` the
/// walker threads down its own call tree, and the diagnostic scope cache
/// is the per-statement recording of it a diagnostic pass leaves behind
/// for the consumers the walker does not drive directly (the return-type
/// check among them).  Both are O(1)/O(log N) lookups.
pub(in crate::type_engine) fn walker_scope_types(
    var_name: &str,
    offset: u32,
    scope_var_resolver: crate::type_engine::resolver::ScopeVarResolverFn<'_>,
) -> Vec<ResolvedType> {
    let prefixed = if var_name.starts_with('$') {
        var_name.to_string()
    } else {
        format!("${}", var_name)
    };
    if let Some(resolver) = scope_var_resolver {
        let from_scope = resolver(&prefixed);
        if !from_scope.is_empty() {
            return from_scope;
        }
    }
    if super::super::forward_walk::is_diagnostic_scope_active()
        && !super::super::forward_walk::is_building_scopes()
        && let Some(types) = super::super::forward_walk::lookup_diagnostic_scope(&prefixed, offset)
    {
        return types;
    }
    Vec::new()
}

/// Resolve the raw iterable type of an argument expression.
///
/// Handles `$variable` (via the walker's scope, then a resolution of the
/// variable) and delegates to `resolve_expression_type` for method calls, property
/// access, etc.
pub(in crate::type_engine) fn resolve_arg_raw_type<'b>(
    arg_expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
        let var_text = bytes_to_str(dv.name).to_string();
        let offset = arg_expr.span().start.offset as usize;
        // The scope the forward walker recorded for this position, from
        // whichever of its two channels is live.
        let from_walker = walker_scope_types(&var_text, offset as u32, ctx.scope_var_resolver);

        // Resolve the variable's type to extract the raw iterable type.
        // This handles cases like `$users = $this->getUsers();
        // array_pop($users)` where `$users` has no `@var` annotation but was
        // assigned from a method returning `list<User>`.
        //
        // Only reached when the walker's scope came up empty, and only
        // without a scope_var_resolver: falling through to
        // resolve_variable_types while the forward walker is active would
        // re-enter it, causing infinite recursion on patterns like
        // `$a['k'] = f($a['k'])`.
        let resolved = if !from_walker.is_empty() || ctx.scope_var_resolver.is_some() {
            from_walker
        } else {
            resolve_variable_types(ctx.var_query(&var_text, offset as u32))
        };
        let from_scope = (!resolved.is_empty())
            .then(|| crate::types::ResolvedType::types_joined(&resolved))
            // `skip_scalar` stays off: a `list<int>` carries an element type
            // every bit as usable as a `list<User>`.
            .filter(|joined| joined.extract_value_type(false).is_some());

        if from_scope.is_some() {
            return from_scope;
        }
    }
    // Fall back to the unified pipeline (method calls, etc.)
    super::super::foreach_resolution::resolve_expression_type(arg_expr, ctx)
}
