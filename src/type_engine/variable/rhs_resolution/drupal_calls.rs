//! Drupal calls in an assignment whose return type a Drupal declaration
//! decides (see [`crate::drupal::types`]).

use mago_syntax::cst::*;

use crate::types::{ClassInfo, ResolvedType};

use crate::type_engine::resolver::VarResolutionCtx;

/// The string literal that binds a call's first parameter, named `name`,
/// positional or named.
fn literal_first<'c>(
    argument_list: &'c ArgumentList<'_>,
    name: &str,
    content: &'c str,
) -> Option<&'c str> {
    let argument = crate::call_args::first_param_arg(argument_list, name)?;
    crate::virtual_members::laravel::helpers::resolved_string_literal(argument.value(), content)
        .map(|(key, _, _)| key)
}

/// What `\Drupal::service('id')` returns: the class the ID is built from.
pub(super) fn try_resolve_drupal_service_type(
    owner_fqn: &str,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if !method_name.eq_ignore_ascii_case("service") {
        return None;
    }
    let backend = ctx.file.backend?;
    let id = literal_first(argument_list, "id", ctx.file.content)?;
    let classes = crate::drupal::types::service_classes(
        backend,
        owner_fqn,
        method_name,
        id,
        ctx.file.class_loader,
    )?;
    Some(ResolvedType::from_classes(classes))
}

/// What `getStorage('node')` on an entity type manager returns.
pub(super) fn try_resolve_entity_storage_type(
    owner: &ClassInfo,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if !method_name.eq_ignore_ascii_case("getStorage") {
        return None;
    }
    let backend = ctx
        .file
        .backend
        .filter(|backend| backend.is_drupal_project())?;
    let id = literal_first(argument_list, "entity_type_id", ctx.file.content)?;
    let storages =
        crate::drupal::types::storage_type(backend, owner, method_name, id, ctx.file.class_loader)?;
    // Each storage keeps its own tag, so a load on it knows its entity.
    Some(
        storages
            .into_iter()
            .flat_map(|(class, ty)| ResolvedType::from_classes_with_hint(vec![class], ty))
            .collect(),
    )
}

/// What a load on a tagged entity storage returns.
pub(super) fn try_resolve_entity_load_type(
    method_name: &str,
    receiver: &[ResolvedType],
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    ctx.file
        .backend
        .filter(|backend| backend.is_drupal_project())?;
    let ty = crate::drupal::types::load_type(method_name, receiver, ctx.file.class_loader)?;
    Some(
        crate::type_engine::type_ops::resolution::resolved_types_for_hint(
            ty,
            "",
            ctx.file.all_classes,
            ctx.file.class_loader,
        ),
    )
}
