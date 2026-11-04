//! Collection return types selected by literal property paths.

use crate::atom::{Atom, atom};
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::{BackedEnumType, ClassInfo, ClassLikeKind, ELOQUENT_COLLECTION_FQN};

const COLLECTION: &str = "Illuminate\\Support\\Collection";
const LAZY_COLLECTION: &str = "Illuminate\\Support\\LazyCollection";

/// Whether a method can use a literal item property to refine its return type.
pub(crate) fn is_projection_method(name: &str) -> bool {
    name.eq_ignore_ascii_case("keyBy")
        || name.eq_ignore_ascii_case("groupBy")
        || name.eq_ignore_ascii_case("pluck")
}

/// Refine a collection call whose literal arguments name item properties.
/// Unknown selectors and callbacks keep the method's declared inference.
pub(crate) fn collection_property_return(
    owner: &ClassInfo,
    receiver: Option<&PhpType>,
    method_name: &str,
    args: &[&str],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let method_name = if method_name.eq_ignore_ascii_case("keyBy") {
        "keyBy"
    } else if method_name.eq_ignore_ascii_case("groupBy") {
        "groupBy"
    } else if method_name.eq_ignore_ascii_case("pluck") {
        "pluck"
    } else {
        return None;
    };
    let loader = ctx.file.class_loader;
    let lazy = is_subtype_of(owner, LAZY_COLLECTION, loader);
    if !lazy && !is_subtype_of(owner, COLLECTION, loader) {
        return None;
    }
    let base = if lazy { LAZY_COLLECTION } else { COLLECTION };
    let bare_receiver = PhpType::named(owner.fqn());
    let receiver = receiver.unwrap_or(&bare_receiver);
    let types = if owner.template_params.is_empty()
        && let TypeKind::Generic(generic) = receiver.kind()
        && generic.args.len() == 2
    {
        // A static<TKey, TValue> result can rebind an extends-only subclass.
        // Its declaration still has the original keys; the receiver has the new ones.
        generic.args.clone()
    } else {
        crate::inheritance::extract_generic_args_from_ancestor(receiver, base, loader)?
    };
    let [key, value] = types.as_slice() else {
        return None;
    };
    let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
        owner,
        loader,
        ctx.resolved_class_cache,
    );
    let method = resolved.get_method_ci(method_name)?;
    let bound = crate::call_args::bind_text_args_to_params(&method.parameters, args);
    let selector = bound.first()?.as_deref()?;
    let second = bound.get(1).and_then(Option::as_deref);
    let property = |path: &str| property_path_type(value, path, ctx).unwrap_or_else(PhpType::mixed);
    match method_name {
        "keyBy" => {
            let path = crate::util::unescape_php_string_literal(selector)?;
            let key = collection_key_type(&property(&path), false, lazy, true, loader);
            Some(PhpType::generic_atom(owner.fqn(), vec![key, value.clone()]))
        }
        "groupBy" => {
            let paths = if let Some(path) = crate::util::unescape_php_string_literal(selector) {
                vec![path]
            } else {
                let inner = selector
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .or_else(|| {
                        selector
                            .strip_prefix("array(")
                            .and_then(|s| s.strip_suffix(')'))
                    })?;
                crate::text_scan::split_top_level_commas(inner)
                    .iter()
                    .filter(|entry| !entry.trim().is_empty())
                    .map(|entry| {
                        let entry = crate::text_scan::split_top_level_arrow(entry)
                            .map_or(*entry, |(_, value)| value);
                        crate::util::unescape_php_string_literal(entry.trim())
                    })
                    .collect::<Option<Vec<_>>>()?
            };
            if paths.is_empty() {
                return None;
            }
            let inner_key = match second.map(str::trim) {
                None => PhpType::int(),
                Some(text) if text.eq_ignore_ascii_case("false") || text == "0" => PhpType::int(),
                Some(text) if text.eq_ignore_ascii_case("true") || text == "1" => key.clone(),
                _ => PhpType::union(vec![key.clone(), PhpType::int()]),
            };
            let keys = paths
                .iter()
                .map(|path| collection_key_type(&property(path), true, false, true, loader))
                .collect::<Vec<_>>();
            Some(grouped_collection(
                owner.fqn(),
                inner_key,
                value.clone(),
                &keys,
                lazy,
                is_subtype_of(owner, ELOQUENT_COLLECTION_FQN, loader),
            ))
        }
        _ => {
            let projected = if selector.eq_ignore_ascii_case("null") {
                value.clone()
            } else {
                property(&crate::util::unescape_php_string_literal(selector)?)
            };
            let key = match second {
                None => PhpType::int(),
                Some(text) if text.eq_ignore_ascii_case("null") => PhpType::int(),
                Some(text) => collection_key_type(
                    &property(&crate::util::unescape_php_string_literal(text)?),
                    false,
                    lazy,
                    false,
                    loader,
                ),
            };
            let collection = if is_subtype_of(owner, ELOQUENT_COLLECTION_FQN, loader) {
                atom(COLLECTION)
            } else {
                owner.fqn()
            };
            Some(PhpType::generic_atom(collection, vec![key, projected]))
        }
    }
}

/// Build nested groups; `preserveKeys` only changes the original items' keys.
pub(super) fn grouped_collection(
    collection: Atom,
    inner_key: PhpType,
    value: PhpType,
    keys: &[PhpType],
    lazy: bool,
    eloquent: bool,
) -> PhpType {
    // LazyCollection delegates grouping to an eager collection. Eloquent's
    // recursive map produces a base collection when its values are groups.
    let eager = if lazy { atom(COLLECTION) } else { collection };
    let mut result = PhpType::generic_atom(eager, vec![inner_key, value]);
    for (depth, key) in keys.iter().rev().enumerate() {
        let owner = if eloquent && depth > 0 {
            atom(COLLECTION)
        } else {
            eager
        };
        result = PhpType::generic_atom(owner, vec![key.clone(), result]);
    }
    if lazy && let TypeKind::Generic(g) = result.kind() {
        return PhpType::generic_atom(collection, g.args.clone());
    }
    result
}

/// The key a collection stores after Laravel and PHP coerce the selected value.
pub(super) fn collection_key_type(
    value: &PhpType,
    group: bool,
    lazy: bool,
    enum_keys: bool,
    loader: &ResolvedClassLoader<'_>,
) -> PhpType {
    match value.kind() {
        TypeKind::Nullable(inner) => {
            return PhpType::union(vec![
                collection_key_type(inner, group, lazy, enum_keys, loader),
                collection_key_type(&PhpType::null(), group, lazy, enum_keys, loader),
            ]);
        }
        TypeKind::Union(members) => {
            return PhpType::union(
                members
                    .iter()
                    .map(|member| collection_key_type(member, group, lazy, enum_keys, loader))
                    .collect(),
            );
        }
        _ => {}
    }
    if group && value.is_array_like() {
        return value
            .iterable_element_type()
            .map_or_else(array_key, |element| {
                collection_key_type(&element, false, false, enum_keys, loader)
            });
    }
    if value.is_null() {
        return if lazy && !enum_keys {
            PhpType::null()
        } else {
            PhpType::literal_string_value("")
        };
    }
    if !lazy
        && let Some(text) = value
            .as_literal()
            .and_then(|literal| literal.string_content())
        && crate::php_type::is_decimal_int_array_key(&text)
    {
        return PhpType::literal_int(text.into_owned());
    }
    if value.is_int_subtype() || value.is_string_type() || value.is_string_subtype() {
        return value.clone();
    }
    if value.is_bool() || value.is_true() || value.is_false() || value.is_float_subtype() {
        return if lazy { value.clone() } else { PhpType::int() };
    }
    if let Some(name) = value.base_name()
        && let Some(class) = loader(Fqn::new(name))
    {
        if enum_keys && class.kind == ClassLikeKind::Enum {
            return match class.backed_type {
                Some(BackedEnumType::Int) => PhpType::int(),
                _ => PhpType::string(),
            };
        }
        if crate::inheritance::resolve_method_return_type(&class, "__toString", loader).is_some() {
            return PhpType::string();
        }
    }
    if lazy && !enum_keys {
        value.clone()
    } else {
        array_key()
    }
}

fn array_key() -> PhpType {
    static ARRAY_KEY: std::sync::LazyLock<PhpType> =
        std::sync::LazyLock::new(|| PhpType::named(atom("array-key")));
    ARRAY_KEY.clone()
}

#[cfg(test)]
#[path = "collection_projection_tests.rs"]
mod tests;

fn property_path_type(value: &PhpType, path: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let (head, tail) = path
        .split_once('.')
        .map_or((path, None), |(head, tail)| (head, Some(tail)));
    let next = match value.kind() {
        TypeKind::Nullable(inner) => {
            return property_path_type(inner, path, ctx).map(PhpType::or_null);
        }
        TypeKind::Union(members) => {
            return members
                .iter()
                .map(|member| property_path_type(member, path, ctx))
                .collect::<Option<Vec<_>>>()
                .map(PhpType::union);
        }
        _ if value.is_null() => return Some(PhpType::null()),
        TypeKind::ArrayShape(_) | TypeKind::ObjectShape(_) => {
            let entry = value.shape_entry(head)?;
            if entry.optional {
                entry.value_type.clone().or_null()
            } else {
                entry.value_type.clone()
            }
        }
        _ if value.is_array_like() => value.iterable_element_type()?.or_null(),
        _ => {
            let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                value,
                "",
                ctx.file.all_classes,
                ctx.file.class_loader,
            );
            let types = classes
                .iter()
                .filter_map(|class| {
                    crate::inheritance::resolve_property_type_hint(
                        class,
                        head,
                        ctx.file.class_loader,
                    )
                })
                .collect::<Vec<_>>();
            if types.is_empty() {
                return None;
            }
            PhpType::union(types)
        }
    };
    match tail {
        Some(tail) => property_path_type(&next, tail, ctx),
        None => Some(next),
    }
}
