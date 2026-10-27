//! Methods, and the properties a constructor promotes from its
//! parameters.

use mago_syntax::cst::class_like::method::{Method, MethodBody};
use mago_syntax::cst::*;

use crate::atom::{Atom, AtomMap, atom, atom_bytes, bytes_to_str};
use crate::docblock;
use crate::parser::signature::{Signature, extract_signature, template_bounds_in_scope};
use crate::parser::{DocblockCtx, extract_hint_type, extract_visibility, merge_deprecation_info};
use crate::php_type::PhpType;
use crate::types::*;
use crate::virtual_members::laravel::{has_scope_attribute, relationship_return_type};

use super::resolve_written_class;

/// The [`MethodInfo`] of a method declaration, or `None` when the target
/// PHP version excludes it.  A constructor's promoted parameters are
/// pushed onto `properties` as they are found.
pub(super) fn extract_method<'a>(
    method: &'a Method<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
    class_template_params: &[Atom],
    class_template_param_bounds: &AtomMap<PhpType>,
    properties: &mut Vec<PropertyInfo>,
) -> Option<MethodInfo> {
    let is_constructor = method.name.value == b"__construct".as_slice();
    let mut signature = extract_signature(
        method,
        &method.attribute_lists,
        &method.parameter_list,
        method.return_type_hint.as_ref(),
        doc_ctx,
        if is_constructor {
            class_template_params
        } else {
            &[]
        },
        class_template_param_bounds,
    )?;
    if is_constructor {
        push_promoted_properties(
            method,
            doc_ctx,
            class_template_params,
            class_template_param_bounds,
            &mut signature,
            properties,
        );
    }
    let Signature {
        mut parameters,
        native_return_type,
        return_type,
        conditional_return,
        docblock: method_docblock_info,
        template_params: method_template_params,
        template_param_bounds: method_template_param_bounds,
        template_param_defaults: method_template_param_defaults,
        template_bindings: method_template_bindings,
    } = signature;

    let depr_info = merge_deprecation_info(
        method_docblock_info
            .as_ref()
            .and_then(docblock::extract_deprecation_message_from_info),
        &method.attribute_lists,
        doc_ctx,
    );

    // An Eloquent relationship's related model comes from the body
    // (`$this->hasMany(Post::class)`) when the declared type does not name
    // it.
    let return_type = relationship_return_type(return_type, method, doc_ctx);

    // Merge `@param` docblock types into parameter type hints so that
    // callable signatures like `callable(User): void` are preserved.  This
    // mirrors the promoted-property logic used for constructor parameters.
    if let Some(ref info) = method_docblock_info {
        docblock::merge_param_docblock_into_parameters(
            info,
            &mut parameters,
            &template_bounds_in_scope(
                class_template_param_bounds,
                // A constructor binding the class's templates takes on
                // their list; that is not shadowing.
                if method_template_params.as_slice() == class_template_params {
                    &[]
                } else {
                    &method_template_params
                },
                &method_template_param_bounds,
            ),
        );
    }

    // A docblock `@param` merge above may have overwritten `type_hint` with
    // a non-nullable docblock type. Re-fold null for parameters whose
    // default value is `null`.
    for param in &mut parameters {
        param.apply_null_default(|name| {
            method_template_params
                .iter()
                .chain(class_template_params)
                .any(|t| t == name)
        });
    }

    // Populate per-parameter descriptions from `@param` tags.
    if let Some(ref info) = method_docblock_info {
        for param in &mut parameters {
            param.description = docblock::extract_param_description_from_info(info, &param.name);
        }
    }

    let reads_func_args = match &method.body {
        MethodBody::Concrete(block) => {
            crate::parser::functions::body_reads_func_args(block.statements.iter())
        }
        MethodBody::Abstract(_) => false,
    };

    let info = method_docblock_info.as_ref();
    Some(MethodInfo {
        name: atom_bytes(method.name.value),
        name_offset: method.name.span.start.offset,
        parameters: parameters.into(),
        native_return_type,
        return_type,
        description: info.and_then(crate::hover::extract_description_from_info),
        is_static: method.modifiers.iter().any(|m| m.is_static()),
        visibility: extract_visibility(method.modifiers.iter()),
        has_scope_attribute: has_scope_attribute(method),
        is_abstract: method.is_abstract(),
        is_final: method.modifiers.iter().any(|m| m.is_final()),
        is_virtual: false,
        is_macro: false,
        is_inferred_return: false,
        // `@pure` promises the call changes nothing, which is what lets a
        // check recorded about the receiver survive it; `@impure` is the
        // opposite promise, for a call whose return type does not already
        // give it away.
        is_pure: info.is_some_and(docblock::declares_pure),
        is_impure: info.is_some_and(docblock::declares_impure),
        reads_func_args,
        is_no_discard: crate::parser::has_no_discard_attribute(&method.attribute_lists, doc_ctx),
        extras: MethodExtras {
            return_description: info.and_then(docblock::extract_return_description_from_info),
            links: info
                .map(docblock::extract_link_urls_from_info)
                .unwrap_or_default(),
            see_refs: info
                .map(docblock::extract_see_refs_from_info)
                .unwrap_or_default(),
            conditional_return,
            deprecation_message: depr_info.message,
            deprecated_replacement: depr_info.replacement,
            not_deprecated: info.is_some_and(|i| {
                i.first_tag_by_kind(docblock::TagKind::NotDeprecated)
                    .is_some()
            }),
            template_params: method_template_params,
            template_param_bounds: method_template_param_bounds,
            template_bindings: method_template_bindings,
            template_param_defaults: method_template_param_defaults,
            // `@phpstan-assert` / `@psalm-assert` tags, so that the
            // narrowing engine can apply type guards from static method
            // calls like `Assert::instanceOf($v, Foo::class)`.
            type_assertions: info
                .map(docblock::extract_type_assertions_from_info)
                .unwrap_or_default(),
            // `@throws` tags, so that cross-file throws propagation can
            // look up which exceptions a method declares without needing
            // access to the source text.
            throws: info
                .map(docblock::extract_throws_tags_from_info)
                .unwrap_or_default(),
            if_this_is: info.and_then(docblock::extract_if_this_is_type_from_info),
            // `@psalm-this-out` / `@phpstan-self-out`, so the forward
            // walker can re-bind the receiver's template arguments after a
            // call, the way an assignment re-binds a variable's type.
            self_out: info.and_then(docblock::extract_self_out_type_from_info),
        }
        .into_option(),
    })
}

/// Push a [`PropertyInfo`] for each promoted constructor parameter (one
/// with a visibility modifier, e.g. `public`, `private`, `protected`).
///
/// When the constructor has a docblock, `@param` annotations can provide
/// a more specific type than the native hint (e.g. `@param list<User>
/// $users` vs native `array $users`); `resolve_effective_type()` picks the
/// winner.  A parameter's inline `@var` without a `@param` of its own also
/// types the parameter in `signature`, and binds the class templates it
/// names.
fn push_promoted_properties<'a>(
    method: &'a Method<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
    class_template_params: &[Atom],
    class_template_param_bounds: &AtomMap<PhpType>,
    signature: &mut Signature,
    properties: &mut Vec<PropertyInfo>,
) {
    for param in method.parameter_list.parameters.iter() {
        if !param.is_promoted_property() {
            continue;
        }
        let raw_name = bytes_to_str(param.variable.name).to_string();
        let prop_name = atom(raw_name.strip_prefix('$').unwrap_or(&raw_name));
        let saved_native_hint = param.hint.as_ref().map(|h| extract_hint_type(h));
        let prop_visibility = extract_visibility(param.modifiers.iter());
        let prop_readonly = param.modifiers.iter().any(|m| m.is_readonly());

        // Check for a docblock type override.
        //
        // 1. Inline `@var` on the parameter itself
        //    (e.g. `/** @var array<User> */ public array $users`).
        // 2. `@param` on the constructor docblock
        //    (e.g. `@param list<User> $users`).
        let inline_var_type = doc_ctx.and_then(|ctx| {
            let doc = docblock::get_docblock_text_for_node(ctx.trivias, ctx.content, param)?;
            docblock::extract_var_type(doc)
        });

        let type_hint = if let Some(ref var_type) = inline_var_type {
            let effective =
                docblock::resolve_effective_type_typed(saved_native_hint.as_ref(), Some(var_type));
            // Without a `@param` of its own, the `@var` is the parameter's
            // documented type too, so it binds class templates from the
            // argument the way `@param T $t` would.
            let has_param_tag = signature.docblock.as_ref().is_some_and(|info| {
                docblock::extract_param_raw_type_from_info(info, &raw_name).is_some()
            });
            if !has_param_tag {
                bind_promoted_var_type(
                    var_type,
                    effective.as_ref(),
                    &raw_name,
                    class_template_params,
                    &mut signature.parameters,
                    &mut signature.template_params,
                    &mut signature.template_bindings,
                );
            }
            effective
        } else if let Some(ref info) = signature.docblock {
            let parsed = docblock::extract_param_raw_type_from_info(info, &raw_name);
            docblock::resolve_effective_type_with_template_bounds(
                saved_native_hint.as_ref(),
                parsed.as_ref(),
                class_template_param_bounds,
            )
        } else {
            saved_native_hint.clone()
        };

        // When no type hint is available, infer from `new ClassName()`
        // default values (PHP 8.1+: `private $repo = new Repo()`).  Resolve
        // to FQN eagerly so downstream code does not need short-name
        // resolution logic.
        let type_hint = type_hint.or_else(|| {
            let dv = param.default_value.as_ref()?;
            if let Expression::Instantiation(inst) = dv.value
                && let Expression::Identifier(ident) = inst.class
            {
                let fqn = resolve_written_class(bytes_to_str(ident.value()), doc_ctx);
                return Some(PhpType::named(atom(&fqn)));
            }
            None
        });

        properties.push(PropertyInfo {
            name: prop_name,
            name_offset: param.variable.span.start.offset,
            native_type_hint: saved_native_hint,
            type_hint,
            description: None,
            is_static: false,
            is_readonly: prop_readonly,
            doc_readonly: DocReadonly::No,
            visibility: prop_visibility,
            deprecation_message: None,
            deprecated_replacement: None,
            see_refs: Vec::new(),
            is_virtual: false,
            source: None,
        });
    }
}

/// Carry a promoted constructor parameter's inline `@var` over to the
/// parameter itself, and record the class template params it names as
/// constructor template bindings.
///
/// Only called for a parameter the constructor's docblock has no `@param`
/// for; a `@param` already did both. Left alone when the constructor
/// declares templates of its own, whose bindings the class ones would mix
/// with.
fn bind_promoted_var_type(
    var_type: &PhpType,
    effective: Option<&PhpType>,
    param_name: &str,
    class_template_params: &[Atom],
    parameters: &mut [ParameterInfo],
    method_template_params: &mut Vec<Atom>,
    method_template_bindings: &mut Vec<(Atom, Atom)>,
) {
    if let Some(param) = parameters.iter_mut().find(|p| p.name == param_name) {
        param.type_hint = effective.cloned();
    }
    if class_template_params.is_empty()
        || !(method_template_params.is_empty()
            || method_template_params.as_slice() == class_template_params)
    {
        return;
    }
    let class_tpl_str: Vec<String> = class_template_params
        .iter()
        .map(|a| a.to_string())
        .collect();
    let mut bindings = Vec::new();
    docblock::templates::collect_template_bindings(
        var_type,
        &class_tpl_str,
        param_name,
        &mut bindings,
    );
    if bindings.is_empty() {
        return;
    }
    if method_template_params.is_empty() {
        *method_template_params = class_template_params.to_vec();
    }
    for (tpl, param) in bindings {
        let binding = (atom(&tpl), atom(&param));
        if !method_template_bindings.contains(&binding) {
            method_template_bindings.push(binding);
        }
    }
}
