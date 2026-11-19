//! Collection return types selected by literal property paths.

use crate::atom::{Atom, atom};
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::key_path::{self, KeyPathRules};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::{BackedEnumType, ClassInfo, ClassLikeKind, ELOQUENT_COLLECTION_FQN};
use crate::util::unescape_php_string_literal_cow;

const COLLECTION: &str = "Illuminate\\Support\\Collection";
const LAZY_COLLECTION: &str = "Illuminate\\Support\\LazyCollection";

/// A collection method whose return type can be refined by literal properties.
#[derive(Clone, Copy)]
pub(crate) enum ProjectionMethod {
    /// Select the keys of the original items.
    KeyBy,
    /// Group the original items by selected keys.
    GroupBy,
    /// Select new values and, optionally, keys.
    Pluck,
}

impl ProjectionMethod {
    /// Classify the call once, before resolving any of its receiver classes.
    #[inline]
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("keyBy") {
            Some(Self::KeyBy)
        } else if name.eq_ignore_ascii_case("groupBy") {
            Some(Self::GroupBy)
        } else if name.eq_ignore_ascii_case("pluck") {
            Some(Self::Pluck)
        } else {
            None
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::KeyBy => "keyBy",
            Self::GroupBy => "groupBy",
            Self::Pluck => "pluck",
        }
    }
}

/// Refine a collection call whose literal arguments name item properties.
/// Unknown selectors and callbacks keep the method's declared inference.
pub(crate) fn collection_property_return(
    owner: &ClassInfo,
    receiver: Option<&PhpType>,
    projection: ProjectionMethod,
    args: &[&str],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let loader = ctx.file.class_loader;
    let owner_name = owner.fqn();
    let lazy = match owner_name.as_str() {
        COLLECTION | ELOQUENT_COLLECTION_FQN => false,
        LAZY_COLLECTION => true,
        _ if is_subtype_of(owner, LAZY_COLLECTION, loader) => true,
        _ if is_subtype_of(owner, COLLECTION, loader) => false,
        _ => return None,
    };
    let base = if lazy { LAZY_COLLECTION } else { COLLECTION };
    let bare_receiver;
    let receiver = match receiver {
        Some(receiver) => receiver,
        None => {
            bare_receiver = PhpType::named(owner_name);
            &bare_receiver
        }
    };
    let inherited_types;
    let types = if let TypeKind::Generic(generic) = receiver.kind()
        && (generic.name.eq_ignore_ascii_case(base)
            || (owner.template_params.is_empty() && generic.args.len() == 2))
    {
        // A static<TKey, TValue> result can rebind an extends-only subclass.
        // Its declaration still has the original keys; the receiver has the new ones.
        generic.args.as_slice()
    } else {
        inherited_types =
            crate::inheritance::extract_generic_args_from_ancestor(receiver, base, loader)?;
        inherited_types.as_slice()
    };
    let [key, value] = types else {
        return None;
    };
    let resolved;
    let method = match owner.get_method_ci(projection.name()) {
        Some(method) => method,
        None => {
            resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
                owner,
                loader,
                ctx.resolved_class_cache,
            );
            resolved.get_method_ci(projection.name())?
        }
    };
    let [selector, second] =
        crate::call_args::bind_text_args_to_param_prefix(&method.parameters, args);
    let selector = selector?;
    let property = |path: &str| property_path_type(value, path, ctx).unwrap_or_else(PhpType::mixed);
    let is_eloquent = || {
        !lazy
            && owner_name != COLLECTION
            && (owner_name == ELOQUENT_COLLECTION_FQN
                || is_subtype_of(owner, ELOQUENT_COLLECTION_FQN, loader))
    };
    match projection {
        ProjectionMethod::KeyBy => {
            let path = unescape_php_string_literal_cow(selector)?;
            let key = collection_key_type(&property(&path), false, lazy, true, loader);
            Some(PhpType::generic_atom(owner_name, vec![key, value.clone()]))
        }
        ProjectionMethod::GroupBy => {
            let single_key;
            let multiple_keys;
            let keys = if let Some(path) = unescape_php_string_literal_cow(selector) {
                single_key = collection_key_type(&property(&path), true, false, true, loader);
                std::slice::from_ref(&single_key)
            } else {
                let inner = selector
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .or_else(|| {
                        selector
                            .strip_prefix("array(")
                            .and_then(|s| s.strip_suffix(')'))
                    })?;
                let entries = crate::text_scan::split_top_level_commas(inner);
                let paths = entries
                    .iter()
                    .filter(|entry| !entry.trim().is_empty())
                    .map(|entry| {
                        let entry = crate::text_scan::split_top_level_arrow(entry)
                            .map_or(*entry, |(_, value)| value);
                        unescape_php_string_literal_cow(entry.trim())
                    })
                    .collect::<Option<Vec<_>>>()?;
                multiple_keys = paths
                    .iter()
                    .map(|path| collection_key_type(&property(path), true, false, true, loader))
                    .collect::<Vec<_>>();
                multiple_keys.as_slice()
            };
            if keys.is_empty() {
                return None;
            }
            let inner_key = match second.map(str::trim) {
                None => PhpType::int(),
                Some(text) if text.eq_ignore_ascii_case("false") || text == "0" => PhpType::int(),
                Some(text) if text.eq_ignore_ascii_case("true") || text == "1" => key.clone(),
                _ => PhpType::union(vec![key.clone(), PhpType::int()]),
            };
            Some(grouped_collection(
                owner_name,
                inner_key,
                value.clone(),
                keys,
                lazy,
                keys.len() > 1 && is_eloquent(),
            ))
        }
        ProjectionMethod::Pluck => {
            let projected = if selector.eq_ignore_ascii_case("null") {
                value.clone()
            } else {
                property(&unescape_php_string_literal_cow(selector)?)
            };
            let key = match second {
                None => PhpType::int(),
                Some(text) if text.eq_ignore_ascii_case("null") => PhpType::int(),
                Some(text) => collection_key_type(
                    &property(&unescape_php_string_literal_cow(text)?),
                    false,
                    lazy,
                    false,
                    loader,
                ),
            };
            let collection = if is_eloquent() {
                atom(COLLECTION)
            } else {
                owner_name
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
    let mut result = PhpType::generic_atom(
        if keys.is_empty() { collection } else { eager },
        vec![inner_key, value],
    );
    for (depth, key) in keys.iter().rev().enumerate() {
        let owner = if lazy && depth + 1 == keys.len() {
            collection
        } else if eloquent && depth > 0 {
            atom(COLLECTION)
        } else {
            eager
        };
        result = PhpType::generic_atom(owner, vec![key.clone(), result]);
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
        // Generators yield a null key as-is; an array key casts it to "".
        return if lazy {
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

/// The value `data_get()` reaches along `path` in an item of type `value`.
fn property_path_type(value: &PhpType, path: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    // data_get() treats these segments as operators (or escapes of them),
    // not as property names.
    if path
        .split('.')
        .any(|segment| matches!(segment, "*" | "{first}" | "{last}") || segment.starts_with('\\'))
    {
        return None;
    }
    let rules = KeyPathRules {
        properties: true,
        method_calls: false,
    };
    let reached = key_path::path_type(value, path.split('.'), rules, ctx)?;
    Some(if reached.may_miss {
        reached.ty.or_null()
    } else {
        reached.ty
    })
}

#[cfg(test)]
#[path = "collection_projection_tests.rs"]
mod tests;
