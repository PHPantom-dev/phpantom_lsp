//! Symfony calls whose return type the service container decides: a
//! service fetched by its ID, a container parameter, and the result of the
//! one handler a query bus's `HandleTrait::handle()` sends a message to.

use std::sync::Arc;

use mago_syntax::cst::*;

use crate::class_lookup::Fqn;
use crate::php_type::PhpType;
use crate::symbol_map::StringKeyKind;
use crate::types::{ClassInfo, ResolvedType};

use crate::type_engine::resolver::VarResolutionCtx;

use super::resolve_rhs_expression;

const HANDLE_TRAIT: &str = "Symfony\\Component\\Messenger\\HandleTrait";

/// What a container lookup on `owner` returns, from the name its literal
/// first argument gives: the class a service ID is built from for `get()`
/// on a container, and the type of a parameter's value for `get()` on a
/// parameter bag or `getParameter()`.
///
/// `None` when the project does not declare what the name is, so the call
/// keeps the type the method declares.
pub(super) fn try_resolve_container_lookup_type(
    owner: &ClassInfo,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if !method_name.eq_ignore_ascii_case("get") && !method_name.eq_ignore_ascii_case("getParameter")
    {
        return None;
    }
    let backend = ctx
        .file
        .backend
        .filter(|backend| backend.is_symfony_project())?;
    let Some(Argument::Positional(argument)) = argument_list.arguments.first() else {
        return None;
    };
    let (key, _, _) = crate::virtual_members::laravel::helpers::resolved_string_literal(
        argument.value,
        ctx.file.content,
    )?;
    let (receiver, _) = crate::symbol_map::extraction::symfony_receiver_method(method_name, 0)?;
    let (_, kind) = receiver
        .targets()
        .iter()
        .find(|(fqn, _)| crate::class_lookup::is_subtype_of(owner, fqn, ctx.file.class_loader))?;
    match kind {
        StringKeyKind::ServiceId => {
            let classes: Vec<Arc<ClassInfo>> = backend
                .symfony_service_classes(key)
                .iter()
                .filter_map(|class| (ctx.file.class_loader)(Fqn::new(class)))
                .collect();
            (!classes.is_empty()).then(|| ResolvedType::from_classes(classes))
        }
        StringKeyKind::ContainerParameter => {
            let ty = backend.symfony_parameter_type(key)?;
            Some(vec![ResolvedType::from_type_string(ty)])
        }
        _ => None,
    }
}

/// What Messenger's `HandleTrait::handle()` returns for the message it is
/// given: what the message's handler returns, when exactly one handler is
/// registered for it.  `handle()` throws for a message with none or with
/// several, so those keep the declared `mixed`, as does a message whose
/// class is not known.
pub(super) fn try_resolve_handle_trait_type(
    owner: &ClassInfo,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if !method_name.eq_ignore_ascii_case("handle") {
        return None;
    }
    let backend = ctx
        .file
        .backend
        .filter(|backend| backend.is_symfony_project())?;
    if !handles_through_trait(owner, ctx) {
        return None;
    }
    let [Argument::Positional(message)] = argument_list.arguments.as_slice() else {
        return None;
    };
    let messages = ResolvedType::into_arced_classes(resolve_rhs_expression(message.value, ctx));
    if messages.is_empty() {
        return None;
    }

    let mut returns = Vec::with_capacity(messages.len());
    for message in &messages {
        let [handler] = backend.symfony_message_handlers(message).try_into().ok()?;
        let handler_class = (ctx.file.class_loader)(Fqn::new(&handler.handler))?;
        let (method, _) =
            crate::type_engine::type_ops::condition_facts::find_method_in_chain_where(
                &handler_class,
                &handler.method,
                ctx.file.class_loader,
                &|_| true,
                &mut Vec::new(),
                0,
            )?;
        let returned = method.return_type.as_ref()?;
        returns.push(returned.replace_self_bound(&handler.handler, None));
    }
    let ty = PhpType::union(returns);
    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &ty,
        "",
        ctx.file.all_classes,
        ctx.file.class_loader,
    );
    Some(if classes.is_empty() {
        vec![ResolvedType::from_type_string(ty)]
    } else {
        ResolvedType::from_classes_with_hint(classes, ty)
    })
}

/// Whether the `handle()` that `owner` has is the one `HandleTrait` gives
/// it, rather than one the class or a parent declares itself.
fn handles_through_trait(owner: &ClassInfo, ctx: &VarResolutionCtx<'_>) -> bool {
    let declares_handle = |class: &ClassInfo| {
        class
            .methods
            .iter()
            .any(|method| method.name.eq_ignore_ascii_case("handle"))
    };
    // The receiver is the class with its inherited members merged in, so
    // what it declares itself is read from the class as written.
    let Some(raw) = (ctx.file.class_loader)(Fqn::new(&owner.fqn())) else {
        return false;
    };
    if declares_handle(&raw) {
        return false;
    }
    crate::inheritance::find_declaring_ancestor(&raw, ctx.file.class_loader, &declares_handle)
        .is_some_and(|(name, _)| name.trim_start_matches('\\') == HANDLE_TRAIT)
}
