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

/// The observers a model's `#[ObservedBy]` attributes register and the
/// resources its `#[UseResource]` and `#[UseResourceCollection]` name, as
/// written.  `None` when it names none of them.
pub(super) fn extract_related_class_attributes(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    content: &str,
) -> Option<Box<crate::types::ModelRelatedClasses>> {
    let mut related = crate::types::ModelRelatedClasses::default();
    for attr in attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
    {
        if last_segment(attr.name.value()) != b"ObservedBy" {
            continue;
        }
        let Some(argument) = attr
            .argument_list
            .as_ref()
            .and_then(|list| list.arguments.first())
        else {
            continue;
        };
        let span = argument.span();
        let Some(text) = content.get(span.start.offset as usize..span.end.offset as usize) else {
            continue;
        };
        // `#[ObservedBy(A::class)]` or `#[ObservedBy([A::class, B::class])]`.
        let list = text
            .trim()
            .strip_prefix('[')
            .and_then(|inner| inner.strip_suffix(']'))
            .unwrap_or(text);
        related.observers.extend(
            list.split(',')
                .filter_map(|item| item.trim().strip_suffix("::class"))
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string),
        );
    }
    related.resource = extract_class_constant_attribute(attribute_lists, content, b"UseResource");
    related.resource_collection =
        extract_class_constant_attribute(attribute_lists, content, b"UseResourceCollection");
    (related != crate::types::ModelRelatedClasses::default()).then(|| Box::new(related))
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
            if !fqns.iter().any(|fqn| attr_fqn.eq_ignore_ascii_case(fqn)) {
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
    match doc_ctx {
        Some(ctx) => crate::names::resolve_to_fqn(name, &ctx.use_map, &ctx.namespace),
        None => name.trim_start_matches('\\').to_string(),
    }
}

/// The [`model_declaration`] flag of the Eloquent column attribute whose
/// short name is `name`.
fn column_attribute_flag(name: &str) -> Option<u16> {
    use model_declaration::*;
    [
        ("Fillable", FILLABLE_ATTRIBUTE),
        ("Guarded", GUARDED_ATTRIBUTE),
        ("Hidden", HIDDEN_ATTRIBUTE),
        ("Visible", VISIBLE_ATTRIBUTE),
        ("Appends", APPENDS_ATTRIBUTE),
        ("Unguarded", UNGUARDED_ATTRIBUTE),
    ]
    .into_iter()
    .find_map(|(target, flag)| name.eq_ignore_ascii_case(target).then_some(flag))
}

/// Whether the attribute written as `name` can be one of Eloquent's column
/// attributes, checked without building its FQN: its last segment names
/// one, or it is an import alias whose target does.
fn may_be_column_attribute(name: &str, doc_ctx: Option<&DocblockCtx<'_>>) -> bool {
    fn short(name: &str) -> &str {
        name.rsplit('\\').next().unwrap_or(name)
    }
    column_attribute_flag(short(name)).is_some()
        || (!name.contains('\\')
            && doc_ctx
                .and_then(|ctx| crate::names::lookup_import(&ctx.use_map, name))
                .is_some_and(|target| column_attribute_flag(short(target)).is_some()))
}

/// Read the literal column lists declared by Eloquent's class attributes.
/// Attribute flags stay separate from property flags: each inherits from its
/// own nearest declaration before Laravel combines the two values.
pub(super) fn extract_column_attributes(
    attribute_lists: &Sequence<'_, attribute::AttributeList<'_>>,
    doc_ctx: Option<&DocblockCtx<'_>>,
    names: &mut Vec<String>,
    sources: &mut Vec<u16>,
) -> u16 {
    use model_declaration::*;
    let mut declared = 0;
    for attribute in attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
    {
        let written = bytes_to_str(attribute.name.value());
        // Every class is read for model metadata, so skip the FQN for the
        // attributes that cannot be one of these.
        if !may_be_column_attribute(written, doc_ctx) {
            continue;
        }
        let fqn = resolve_attribute_fqn(written, doc_ctx);
        let Some((namespace, name)) = fqn.rsplit_once('\\') else {
            continue;
        };
        if !namespace.eq_ignore_ascii_case("Illuminate\\Database\\Eloquent\\Attributes") {
            continue;
        }
        let Some(flag) = column_attribute_flag(name) else {
            continue;
        };
        // Laravel uses the first matching attribute, including an empty or
        // dynamically computed declaration that hides an ancestor's list.
        if declared & flag != 0 {
            continue;
        }
        declared |= flag;
        if flag == UNGUARDED_ATTRIBUTE {
            continue;
        }
        let Some(arguments) = &attribute.argument_list else {
            continue;
        };
        let mut arguments = arguments.arguments.iter();
        let Some(argument::PartialArgument::Positional(first)) = arguments.next() else {
            continue;
        };
        if first.ellipsis.is_some() {
            continue;
        }
        let elements = match first.value {
            Expression::Array(array) => Some(&array.elements),
            Expression::LegacyArray(array) => Some(&array.elements),
            Expression::Literal(literal::Literal::String(_)) => None,
            _ => continue,
        };
        let mut push = |value: &Expression<'_>| {
            let Expression::Literal(literal::Literal::String(value)) = value else {
                return;
            };
            let Some(name) = value
                .value
                .and_then(crate::atom::literal_bytes_to_str)
                .filter(|name| !name.is_empty())
            else {
                return;
            };
            if let Some(index) = names.iter().position(|existing| existing == name) {
                sources[index] |= flag;
            } else {
                names.push(name.to_string());
                sources.push(flag);
            }
        };
        if let Some(elements) = elements {
            for element in elements.iter() {
                match element {
                    ArrayElement::Value(value) => push(value.value),
                    ArrayElement::KeyValue(value) => push(value.value),
                    _ => {}
                }
            }
        } else {
            push(first.value);
            for argument in arguments {
                match argument {
                    argument::PartialArgument::Positional(value) if value.ellipsis.is_none() => {
                        push(value.value)
                    }
                    argument::PartialArgument::Named(value) => push(value.value),
                    _ => {}
                }
            }
        }
    }
    declared
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

#[cfg(test)]
mod tests {
    use super::resolve_attribute_fqn;
    use crate::parser::DocblockCtx;

    #[test]
    fn model_column_attributes_resolve_names_without_namespace_context() {
        assert_eq!(resolve_attribute_fqn("\\Fillable", None), "Fillable");
        let context = DocblockCtx {
            trivias: &[],
            content: "",
            php_version: None,
            use_map: Default::default(),
            namespace: None,
        };
        assert_eq!(
            resolve_attribute_fqn("namespace\\Fillable", Some(&context)),
            "Fillable"
        );
    }
}
