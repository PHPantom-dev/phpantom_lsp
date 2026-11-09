//! Calls that return the value a key path reaches in their argument, typed
//! by the shared walk (see [`crate::type_engine::key_path`]).

use mago_syntax::cst::*;

use crate::types::ResolvedType;

use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast;

use super::resolve_rhs_expression;

/// What `ArrayHelper::getValueByPath($user, 'profile.city')` and its
/// siblings return (see [`crate::yii::array_helper`]).
pub(super) fn try_resolve_array_helper_type(
    owner_fqn: &str,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let method = crate::yii::array_helper::path_method(owner_fqn, method_name)?;
    let texts = extract_arg_texts_from_ast(argument_list, ctx.file.content);
    let args: Vec<&str> = texts.iter().map(String::as_str).collect();
    // Each argument is typed from its expression, which the text it is
    // read from is a slice of.
    let arg_type = |text: &str| {
        let at = args
            .iter()
            .position(|arg| arg.as_bytes().as_ptr_range().contains(&text.as_ptr()))?;
        let resolved = resolve_rhs_expression(argument_list.arguments.get(at)?.value(), ctx);
        (!resolved.is_empty()).then(|| ResolvedType::types_joined(&resolved))
    };
    let ty = crate::yii::array_helper::call_return_type(
        method,
        &args,
        &arg_type,
        &ctx.as_resolution_ctx(),
    )?;
    Some(
        crate::type_engine::type_resolution::resolved_types_for_hint(
            ty,
            "",
            ctx.file.all_classes,
            ctx.file.class_loader,
        ),
    )
}
