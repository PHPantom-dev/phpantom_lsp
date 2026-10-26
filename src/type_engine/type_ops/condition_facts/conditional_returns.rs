//! The `never` branches of a conditional return type: a call whose
//! conditional return type answers `never` for some argument types cannot
//! have returned with such an argument, which narrows the argument after
//! the call.

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::class_lookup::ResolvedClassLoader;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::VarResolutionCtx;
use crate::types::{ClassInfo, ParameterInfo, SharedVec};

use super::assert_annotations::{
    find_method_in_chain_where, resolve_instance_receiver_class, resolve_static_receiver_class,
};

/// What a call's conditional return type says about the arguments it was
/// given.
///
/// A branch that resolves to `never` cannot be taken, so an argument
/// value that would select it cannot have reached the call: the call
/// would not have returned.  That is how PHPStan reads `throw_unless()`
/// and its family, which declare their effect as
/// `($condition is false ? never : …)` rather than with an
/// `@phpstan-assert` tag.
pub(in crate::type_engine) struct CallReturnInfo<'a> {
    /// The callee's declared conditional return type, unevaluated.
    pub(in crate::type_engine) return_type: PhpType,
    /// The callee's parameter list, for mapping `$param` names to
    /// positional argument indices.
    pub(in crate::type_engine) parameters: SharedVec<ParameterInfo>,
    /// The call-site argument list.
    pub(in crate::type_engine) argument_list: &'a ArgumentList<'a>,
}

/// The callee facts behind a call whose declared return type is
/// conditional, or `None` when it is not one (which is nearly every
/// call, so this is the cheap early exit for the walker).
pub(in crate::type_engine) fn extract_conditional_return_call<'a>(
    call: &'a Call<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<CallReturnInfo<'a>> {
    match call {
        Call::Function(func_call) => {
            let Expression::Identifier(ident) = func_call.function else {
                return None;
            };
            let func_name = bytes_to_str(ident.value()).to_string();
            let offset = func_call.function.span().start.offset;
            let func_info = ctx.function_loader()?(&func_name, offset)?;
            let return_type = func_info.conditional_return?;
            names_a_conditional(&return_type).then_some(CallReturnInfo {
                return_type,
                parameters: func_info.parameters,
                argument_list: &func_call.argument_list,
            })
        }
        Call::StaticMethod(static_call) => {
            let ClassLikeMemberSelector::Identifier(ident) = &static_call.method else {
                return None;
            };
            let class = resolve_static_receiver_class(static_call.class, ctx)?;
            conditional_return_from_chain(
                &class,
                bytes_to_str(ident.value),
                &static_call.argument_list,
                ctx,
            )
        }
        // `->` and `?->` reach the same method; only the receiver's
        // nullability differs, which the conditional return does not read.
        Call::Method(MethodCall {
            object,
            method,
            argument_list,
            ..
        })
        | Call::NullSafeMethod(NullSafeMethodCall {
            object,
            method,
            argument_list,
            ..
        }) => instance_conditional_return(object, method, argument_list, ctx),
    }
}

/// The conditional return an instance method call carries, for either
/// arrow.
fn instance_conditional_return<'a>(
    object: &Expression<'_>,
    selector: &ClassLikeMemberSelector<'_>,
    argument_list: &'a ArgumentList<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<CallReturnInfo<'a>> {
    let ClassLikeMemberSelector::Identifier(ident) = selector else {
        return None;
    };
    let class = resolve_instance_receiver_class(object, ctx)?;
    conditional_return_from_chain(&class, bytes_to_str(ident.value), argument_list, ctx)
}

/// Find the definition of `method_name` whose return type is conditional,
/// searching the class's own methods, its traits, and its parent chain.
///
/// Uses the same raw class loads [`find_assertion_method_in_chain`] does,
/// for the same reason: this runs inside the forward walker, where a full
/// inheritance merge would write a partial result into the shared
/// resolved-class cache.
fn conditional_return_from_chain<'a>(
    class: &ClassInfo,
    method_name: &str,
    argument_list: &'a ArgumentList<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<CallReturnInfo<'a>> {
    let (method, _) = find_method_in_chain_where(
        class,
        method_name,
        ctx.file.class_loader,
        &|m| m.conditional_return().is_some_and(names_a_conditional),
        &mut Vec::new(),
        0,
    )?;
    Some(CallReturnInfo {
        return_type: method.extras.as_ref()?.conditional_return.clone()?,
        parameters: method.parameters.clone(),
        argument_list,
    })
}

/// Whether a declared type has a conditional anywhere a `never` branch
/// could hide.
///
/// Only the conditional spine is walked: a conditional nested inside a
/// generic argument describes what the container holds, not whether the
/// call returns.
fn names_a_conditional(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Conditional(cond) => {
            cond.param.starts_with('$')
                || names_a_conditional(&cond.then_type)
                || names_a_conditional(&cond.else_type)
        }
        _ => false,
    }
}

/// The members of `arg_ty` that cannot have reached this call, because
/// the branch each of them selects in `return_type` is `never`.
///
/// `$dispatcher` typed `Dispatcher|null` handed to a
/// `($condition is false ? never : ($condition is non-empty-mixed ?
/// TValue : never))` returns `[null]`: the null member lands on a `never`
/// branch, so a run that gets past the call did not have one.
pub(in crate::type_engine) fn never_ruled_out_members(
    return_type: &PhpType,
    param_name: &str,
    arg_ty: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<PhpType> {
    let members = split_into_runtime_members(arg_ty);
    // A type with nothing to choose between says nothing: ruling its only
    // member out would leave the argument with no type at all, which is a
    // claim about unreachable code rather than about the value.
    if members.len() < 2 {
        return Vec::new();
    }
    let ruled_out: Vec<PhpType> = members
        .iter()
        .filter(|member| {
            evaluate_conditional_for(return_type, param_name, member, class_loader)
                .is_some_and(|result| result.is_never())
        })
        .cloned()
        .collect();
    // Every member ruled out means the call can never return at all, which
    // is a statement about the call site rather than about the argument.
    if ruled_out.len() == members.len() {
        return Vec::new();
    }
    ruled_out
}

/// The alternatives a value of this type can actually be at runtime.
///
/// `?Foo` is `Foo` or `null`, and a `bool` really is `true` or `false`:
/// splitting it is what lets `throw_unless($flag)` leave `true` behind.
pub(in crate::type_engine) fn split_into_runtime_members(ty: &PhpType) -> Vec<PhpType> {
    let mut out = Vec::new();
    for member in ty.union_members() {
        match member.kind() {
            TypeKind::Nullable(inner) => {
                out.extend(split_into_runtime_members(inner));
                out.push(PhpType::null());
            }
            TypeKind::Named(name) if name == "bool" || name == "boolean" => {
                out.push(PhpType::true_());
                out.push(PhpType::false_());
            }
            _ => out.push((*member).clone()),
        }
    }
    out.dedup();
    out
}

/// Walk the conditional spine of `ty` with `$param` bound to `arg_ty`,
/// returning the branch it settles on.
///
/// `None` when a condition cannot be decided for this member, or when the
/// spine tests a parameter other than the one being judged: either way
/// nothing about this member has been proven.
fn evaluate_conditional_for(
    ty: &PhpType,
    param_name: &str,
    arg_ty: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    let TypeKind::Conditional(cond) = ty.kind() else {
        return Some(ty.clone());
    };
    if cond.param != param_name {
        return None;
    }
    let mut taken = condition_result_for_member(arg_ty, &cond.condition, class_loader)?;
    if cond.negated {
        taken = !taken;
    }
    let branch = if taken {
        &cond.then_type
    } else {
        &cond.else_type
    };
    evaluate_conditional_for(branch, param_name, arg_ty, class_loader)
}

/// Decide `$param is <condition>` for one runtime member.
///
/// The shared conditional evaluator answers `is true` / `is false` only
/// for an argument already narrowed to one of them, because a plain
/// `bool` really may be either.  Here the members have already been split
/// apart, so a member that cannot be a bool at all settles the condition:
/// a `null` argument is not `false`, which is what sends
/// `throw_unless($x)` down its else branch instead of leaving it
/// undecided.
fn condition_result_for_member(
    member: &PhpType,
    condition: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<bool> {
    if condition.is_true() || condition.is_false() {
        if member.is_true() || member.is_false() {
            return Some(member.is_true() == condition.is_true());
        }
        return (!can_hold_a_bool(member)).then_some(false);
    }
    // `non-empty-mixed` is PHPStan's spelling of "any truthy value", which
    // is the condition the throw-helper family is written against.
    if let TypeKind::Named(name) = condition.kind()
        && name == "non-empty-mixed"
    {
        return member.truthiness();
    }
    super::super::conditional::condition_holds_for_type(member, condition, class_loader)
}

/// Whether a value of this type could be `true` or `false`.
fn can_hold_a_bool(ty: &PhpType) -> bool {
    if ty.is_mixed() || ty.is_untyped() {
        return true;
    }
    match ty.kind() {
        TypeKind::Named(name) => matches!(
            name.to_ascii_lowercase().as_str(),
            "bool" | "boolean" | "true" | "false" | "scalar"
        ),
        _ => false,
    }
}
