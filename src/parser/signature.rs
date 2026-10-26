//! The signature extraction shared by standalone functions and methods.
//!
//! Both declarations carry the same pieces: version-gating stub
//! attributes, a parameter list, a return type hint that
//! `#[LanguageLevelTypeAware]` and `#[ArrayShape]` can refine, and a
//! docblock with `@return`, conditional return and `@template` tags.

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{Atom, AtomMap, atom};
use crate::docblock::{self, DocblockInfo};
use crate::php_type::PhpType;
use crate::types::ParameterInfo;

use super::{
    DocblockCtx, apply_array_shape_override, extract_hint_type, extract_language_level_type,
    extract_parameters, is_available_for_version, is_removed_for_version,
};

/// The pieces of a function or method signature computed by
/// [`extract_signature`].
pub(super) struct Signature {
    pub parameters: Vec<ParameterInfo>,
    /// The native return hint, or the `#[LanguageLevelTypeAware]` type
    /// that replaces it for the target PHP version.
    pub native_return_type: Option<PhpType>,
    /// The native return type refined by `@return` and `#[ArrayShape]`.
    pub return_type: Option<PhpType>,
    pub conditional_return: Option<PhpType>,
    /// The docblock's parse, for the extraction the caller does itself.
    pub docblock: Option<DocblockInfo>,
    pub template_params: Vec<Atom>,
    pub template_param_bounds: AtomMap<PhpType>,
    pub template_param_defaults: Box<[(Atom, PhpType)]>,
    pub template_bindings: Vec<(Atom, Atom)>,
}

/// Extract the signature of the function or method `node`, or `None` when
/// its `#[PhpStormStubsElementAvailable]` range or `@removed` tag excludes
/// it from the target PHP version.
///
/// `ctor_class_template_params` is the class's template list when `node`
/// is a constructor (empty otherwise): a constructor whose docblock binds
/// none of its own templates but binds the class's through `@param T $x`
/// takes on the class's list and those bindings.
///
/// `class_template_param_bounds` are the bounds of the enclosing class's
/// templates (empty for a standalone function), which `@return` naming one
/// of them is judged by against the native return hint.
pub(super) fn extract_signature<'a>(
    node: &impl HasSpan,
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    parameter_list: &FunctionLikeParameterList<'_>,
    return_type_hint: Option<&FunctionLikeReturnTypeHint<'_>>,
    doc_ctx: Option<&DocblockCtx<'a>>,
    ctor_class_template_params: &[Atom],
    class_template_param_bounds: &AtomMap<PhpType>,
) -> Option<Signature> {
    let php_version = doc_ctx.and_then(|ctx| ctx.php_version);
    if let Some(ctx) = doc_ctx
        && let Some(ver) = php_version
        && (!is_available_for_version(attribute_lists, ctx, ver)
            || is_removed_for_version(node, ctx, ver))
    {
        return None;
    }

    let parameters = extract_parameters(
        parameter_list,
        doc_ctx.map(|ctx| ctx.content),
        php_version,
        doc_ctx,
    );
    let raw_native_return_type = return_type_hint.map(|rth| extract_hint_type(&rth.hint));

    // A `#[LanguageLevelTypeAware]` override replaces the native type hint
    // with the version-appropriate type.
    let lang_level_return = if let Some(ctx) = doc_ctx
        && let Some(ver) = php_version
    {
        extract_language_level_type(attribute_lists, ctx, ver)
    } else {
        None
    };
    // The attribute is JetBrains' authoritative, version-resolved return
    // type.  When it is present the accompanying `@return` docblock is
    // legacy text (e.g. `resource|false` left over from before the
    // handle→object migration) and must not widen it back to the old value.
    let return_from_lang_level = lang_level_return.is_some();
    let native_return_type = lang_level_return.or(raw_native_return_type);

    let docblock_text = doc_ctx
        .and_then(|ctx| docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, node));
    let doc_info = docblock_text.and_then(docblock::parse_docblock_for_tags);

    let Some(info) = doc_info.as_ref() else {
        let return_type = finish_return_type(native_return_type.clone(), attribute_lists, doc_ctx);
        return Some(Signature {
            parameters,
            native_return_type,
            return_type,
            conditional_return: None,
            docblock: doc_info,
            template_params: Vec::new(),
            template_param_bounds: AtomMap::default(),
            template_param_defaults: Box::default(),
            template_bindings: Vec::new(),
        });
    };

    let conditional = docblock::extract_conditional_return_type_from_info(info);

    // `@template` params, their bounds and defaults, and the `@param`
    // bindings that substitute them at call sites.
    let params_full = docblock::extract_template_params_full_from_info(info);
    let mut tpl_names: Vec<String> = Vec::with_capacity(params_full.len());
    let mut template_param_bounds: AtomMap<PhpType> = AtomMap::default();
    let mut tpl_param_defaults: Vec<(Atom, PhpType)> = Vec::new();
    for (name, bound, _, default) in params_full {
        if let Some(bound) = bound {
            template_param_bounds.insert(atom(&name), bound);
        }
        if let Some(default) = default {
            tpl_param_defaults.push((atom(&name), default));
        }
        tpl_names.push(name);
    }
    let mut template_params: Vec<Atom> = tpl_names.iter().map(|n| atom(n)).collect();

    let effective = if return_from_lang_level {
        native_return_type.clone()
    } else {
        let parsed_doc_return = docblock::extract_return_type_from_info(info);
        docblock::resolve_effective_type_with_template_bounds(
            native_return_type.as_ref(),
            parsed_doc_return.as_ref(),
            &template_bounds_in_scope(
                class_template_param_bounds,
                &template_params,
                &template_param_bounds,
            ),
        )
    };
    let return_type = finish_return_type(effective, attribute_lists, doc_ctx);

    let mut template_bindings: Vec<(Atom, Atom)> = if tpl_names.is_empty() {
        Vec::new()
    } else {
        docblock::extract_template_param_bindings_from_info(info, &tpl_names)
            .into_iter()
            .map(|(a, b)| (atom(&a), atom(&b)))
            .collect()
    };

    // A constructor can bind the *class-level* template params through its
    // `@param` annotations:
    //   /** @template T */
    //   class Foo {
    //       /** @param T $bar */
    //       public function __construct($bar) {}
    //   }
    // where `T` is declared on the class but bound via `@param T $bar`.
    if template_bindings.is_empty() && !ctor_class_template_params.is_empty() {
        let class_tpl_names: Vec<String> = ctor_class_template_params
            .iter()
            .map(|a| a.to_string())
            .collect();
        let class_bindings =
            docblock::extract_template_param_bindings_from_info(info, &class_tpl_names);
        if !class_bindings.is_empty() {
            template_params = ctor_class_template_params.to_vec();
            template_bindings = class_bindings
                .into_iter()
                .map(|(a, b)| (atom(&a), atom(&b)))
                .collect();
            tpl_names = class_tpl_names;
        }
    }

    // Without an explicit conditional return type, synthesize one from the
    // `@template` annotations.  For example:
    //   @template T
    //   @param class-string<T> $class
    //   @return T
    // becomes a conditional that resolves T from the call-site argument
    // (e.g. find(User::class) → User).
    let conditional_return = conditional.or_else(|| {
        docblock::synthesize_template_conditional_from_info(
            info,
            &tpl_names,
            return_type.as_ref(),
            false,
        )
    });

    Some(Signature {
        parameters,
        native_return_type,
        return_type,
        conditional_return,
        docblock: doc_info,
        template_params,
        template_param_bounds,
        template_param_defaults: tpl_param_defaults.into_boxed_slice(),
        template_bindings,
    })
}

/// The `@template` bounds a method signature sees: the class's, with the
/// method's own templates shadowing any of the same name, bounded or not.
pub(super) fn template_bounds_in_scope<'b>(
    class_bounds: &'b AtomMap<PhpType>,
    method_params: &[Atom],
    method_bounds: &'b AtomMap<PhpType>,
) -> std::borrow::Cow<'b, AtomMap<PhpType>> {
    if class_bounds.is_empty() {
        return std::borrow::Cow::Borrowed(method_bounds);
    }
    if method_params.is_empty() {
        return std::borrow::Cow::Borrowed(class_bounds);
    }
    let mut merged = class_bounds.clone();
    merged.retain(|name, _| !method_params.contains(name));
    merged.extend(method_bounds.iter().map(|(n, b)| (*n, b.clone())));
    std::borrow::Cow::Owned(merged)
}

/// The effective return type with a bare `array` (or a nullable or union
/// containing it) given way to the more specific `#[ArrayShape]` type.
fn finish_return_type(
    effective: Option<PhpType>,
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> Option<PhpType> {
    match doc_ctx {
        Some(ctx) => effective.map(|ty| apply_array_shape_override(ty, attribute_lists, ctx)),
        None => effective,
    }
}
