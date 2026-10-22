use mago_span::HasSpan;

use crate::atom::{Atom, atom, bytes_to_str, last_segment};
use crate::parser::DocblockCtx;
use crate::php_type::PhpType;
use crate::types::MethodInfo;
use crate::virtual_members::laravel::cast_literals::extract_string_literal;

use super::*;

/// Extract the policy class name from a `#[UsePolicy(X::class)]` attribute.
pub(super) fn extract_use_policy_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
) -> Option<String> {
    extract_class_constant_attribute(attribute_lists, content, b"UsePolicy")
}

/// The `X` of the first `#[Name(X::class)]` attribute whose short name is
/// `attr_name`.
pub(super) fn extract_class_constant_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
    attr_name: &[u8],
) -> Option<String> {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            let short = last_segment(attr.name.value());
            if short != attr_name {
                continue;
            }
            let arg_list = attr.argument_list.as_ref()?;
            let first_arg = arg_list.arguments.first()?;
            let span = first_arg.span();
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            let text = content.get(start..end)?;
            let class_name = text.trim_end_matches("::class").trim();
            if !class_name.is_empty() {
                return Some(class_name.to_string());
            }
        }
    }
    None
}

fn extract_laravel_model_string_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
    doc_ctx: Option<&DocblockCtx<'_>>,
    fqns: &[&str],
) -> Option<String> {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            let attr_fqn = resolve_attribute_fqn(bytes_to_str(attr.name.value()), doc_ctx);
            if !fqns.iter().any(|fqn| attr_fqn == *fqn) {
                continue;
            }
            let arg_list = attr.argument_list.as_ref()?;
            let first_arg = arg_list.arguments.first()?;
            let span = first_arg.span();
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            let text = content.get(start..end)?.trim();
            if let Some(value) = extract_string_literal(text) {
                return Some(value);
            }
        }
    }
    None
}

fn resolve_attribute_fqn(name: &str, doc_ctx: Option<&DocblockCtx<'_>>) -> String {
    let name = name.trim_start_matches('\\');
    if name.contains('\\') {
        return name.to_string();
    }
    let Some(ctx) = doc_ctx else {
        return name.to_string();
    };
    if let Some(imported) = ctx.use_map.get(name) {
        return imported.trim_start_matches('\\').to_string();
    }
    if let Some(ns) = &ctx.namespace {
        return format!("{}\\{}", ns, name);
    }
    name.to_string()
}

pub(super) fn extract_laravel_connection_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> Option<String> {
    extract_laravel_model_string_attribute(
        attribute_lists,
        content,
        doc_ctx,
        &["Illuminate\\Database\\Eloquent\\Attributes\\Connection"],
    )
}

pub(super) fn extract_laravel_table_attribute(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> Option<String> {
    extract_laravel_model_string_attribute(
        attribute_lists,
        content,
        doc_ctx,
        &["Illuminate\\Database\\Eloquent\\Attributes\\Table"],
    )
}

/// Determine the custom builder class for an Eloquent model.
///
/// Checks three sources in priority order:
///
/// 1. A `newEloquentBuilder()` override with a concrete return type.
/// 2. `#[UseEloquentBuilder(CustomBuilder::class)]` on the class.
/// 3. `/** @use HasBuilder<CustomBuilder> */` in `use_generics`.
pub(super) fn extract_custom_builder(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    use_generics: &[(Atom, Vec<PhpType>)],
    methods: &[MethodInfo],
    content: &str,
) -> Option<PhpType> {
    extract_customisation(
        attribute_lists,
        use_generics,
        methods,
        content,
        b"UseEloquentBuilder",
        &["HasBuilder", "CustomizeQueryBuilder"],
        "newEloquentBuilder",
        &["Illuminate\\Database\\Eloquent\\Builder", "Builder"],
    )
}

/// Determine the custom collection class for an Eloquent model.
///
/// Checks the same three sources [`extract_custom_builder`] does, in the
/// same order: `newCollection()`, `#[CollectedBy(CustomCollection::class)]`,
/// and `/** @use HasCollection<CustomCollection> */`.
pub(super) fn extract_custom_collection(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    use_generics: &[(Atom, Vec<PhpType>)],
    methods: &[MethodInfo],
    content: &str,
) -> Option<PhpType> {
    extract_customisation(
        attribute_lists,
        use_generics,
        methods,
        content,
        b"CollectedBy",
        &["HasCollection"],
        "newCollection",
        &["Illuminate\\Database\\Eloquent\\Collection", "Collection"],
    )
}

/// The class a model substitutes for one of Eloquent's defaults, in the
/// three ways a model can name it.
///
/// `attr_name` is the `#[…(X::class)]` attribute, `traits` the generic
/// traits whose first argument names it, `factory_method` the `new…()`
/// override that returns it, and `defaults` the base classes that method
/// returns when nothing is customised.
#[allow(clippy::too_many_arguments)]
fn extract_customisation(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    use_generics: &[(Atom, Vec<PhpType>)],
    methods: &[MethodInfo],
    content: &str,
    attr_name: &[u8],
    traits: &[&str],
    factory_method: &str,
    defaults: &[&str],
) -> Option<PhpType> {
    if let Some(method) = methods.iter().find(|m| m.name == factory_method)
        && let Some(return_type) = method.return_type.as_ref()
        && let Some(base) = return_type.base_name()
        && !base.is_empty()
        && !defaults.contains(&base)
    {
        return Some(return_type.clone());
    }

    if let Some(name) = extract_class_constant_attribute(attribute_lists, content, attr_name) {
        return Some(PhpType::named(atom(&name)));
    }

    for (trait_name, args) in use_generics {
        let short = crate::util::short_name(trait_name);
        if traits.contains(&short) && !args.is_empty() {
            return Some(args[0].clone());
        }
    }

    None
}
