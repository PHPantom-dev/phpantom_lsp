//! Higher-order collection proxies (`$users->map->name`).
//!
//! Laravel collections expose a set of magic properties — `map`, `filter`,
//! `each`, `sum`, … — that return a
//! `Illuminate\Support\HigherOrderCollectionProxy`.  Reading a property or
//! calling a method on that proxy runs the proxied collection method with a
//! closure that performs the access on every item:
//!
//! ```php
//! $users->map->email;           // Collection<int, string>
//! $users->filter->isVerified(); // Collection<int, User>
//! $users->sum->score;           // int
//! ```
//!
//! Resolving this needs two pieces of information that the framework's own
//! `@property-read HigherOrderCollectionProxy<TKey, TValue> $map` annotation
//! throws away: **which** collection method is being proxied, and **which**
//! collection class the proxy came from (`filter` returns `static`, so an
//! Eloquent collection must stay an Eloquent collection).
//!
//! Both are recovered by tagging the proxy type with two extra generic
//! arguments when the collection class is resolved:
//!
//! ```text
//! HigherOrderCollectionProxy<TKey, TValue, 'map', Illuminate\Support\Collection>
//! ```
//!
//! [`tag_higher_order_proxy_properties`] adds the tag (it runs as a Laravel
//! class patch, so the tag is written before generic substitution and
//! `TKey`/`TValue` are substituted along with everything else).  Once the
//! proxy type is resolved with concrete arguments,
//! [`inject_higher_order_proxy_members`] reads the tag back and grafts every
//! public member of the value type onto the proxy, rewriting each member's
//! type to whatever the proxied collection method returns for it.
//!
//! Neither extra argument can collide with a template parameter, so the
//! framework's own `TKey` / `TValue` keep their positions and are
//! substituted exactly as before.

use std::sync::{Arc, LazyLock};

use crate::atom::{Atom, AtomMap, atom};
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of, is_subtype_of_named};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{
    ClassInfo, ELOQUENT_COLLECTION_FQN, MethodExtras, MethodInfo, PropertyInfo, Visibility,
};
use crate::util::short_name;
use crate::virtual_members::{ResolvedClassCache, resolve_class_fully_with_type_args};

/// Fully-qualified name of the proxy object returned by the magic
/// collection properties.
pub(crate) const HIGHER_ORDER_COLLECTION_PROXY_FQN: &str =
    "Illuminate\\Support\\HigherOrderCollectionProxy";

/// Short name of the proxy, used to recognise the framework's
/// `@property-read` annotations without depending on how the docblock
/// spelled the class.
const HIGHER_ORDER_COLLECTION_PROXY_SHORT: &str = "HigherOrderCollectionProxy";

/// The base collection every `Enumerable` degrades to when the proxied
/// method can no longer return `static`.
static SUPPORT_COLLECTION_FQN: LazyLock<Atom> =
    LazyLock::new(|| atom("Illuminate\\Support\\Collection"));

/// The `array-key` pseudo-type, held rather than rebuilt: constructing a
/// type interns it, which costs a hash and a shard lock.
static ARRAY_KEY: LazyLock<PhpType> = LazyLock::new(|| PhpType::named(atom("array-key")));

/// Number of generic arguments a tagged proxy type carries.
const TAGGED_ARG_COUNT: usize = 4;

// ─── Tagging ────────────────────────────────────────────────────────────────

/// Record the proxied method name and the owning collection class in the
/// generic arguments of every `HigherOrderCollectionProxy` property.
///
/// Runs from [`apply_laravel_patches`](super::patches::apply_laravel_patches)
/// for every resolved class, so a subclass re-tags the properties it
/// inherited from its parent with its *own* FQN.  That is what keeps
/// `$eloquentCollection->filter->isActive()` on the Eloquent collection
/// instead of degrading it to the parent `Support\Collection`.
pub(crate) fn tag_higher_order_proxy_properties(class: &mut ClassInfo, fqn: &str) {
    if !class
        .properties
        .iter()
        .any(|p| untagged_proxy(p, fqn).is_some())
    {
        return;
    }

    let owner = PhpType::named(atom(fqn));
    for property in class.properties.make_mut() {
        let Some(tagged) = tagged_hint(property, fqn, &owner) else {
            continue;
        };
        Arc::make_mut(property).type_hint = Some(tagged);
    }
}

/// The up-to-date tag for `property`, or `None` when it is not a proxy
/// property or already carries the right tag.
///
/// Reading the tag and rewriting it are one step so that a property is
/// parsed once per pass: this runs for every property of every resolved
/// class.
fn tagged_hint(property: &PropertyInfo, owner_fqn: &str, owner: &PhpType) -> Option<PhpType> {
    let (name, args) = untagged_proxy(property, owner_fqn)?;
    let key = args.first().cloned().unwrap_or_else(|| ARRAY_KEY.clone());
    let value = args.get(1).cloned().unwrap_or_else(PhpType::mixed);
    Some(PhpType::generic_atom(
        name,
        vec![
            key,
            value,
            PhpType::literal_string_value(property.name.as_str()),
            owner.clone(),
        ],
    ))
}

/// The proxy class name and generic arguments of a proxy property that is
/// missing an up-to-date tag, or `None` for anything already tagged or not
/// a proxy at all.
fn untagged_proxy<'a>(
    property: &'a PropertyInfo,
    owner_fqn: &str,
) -> Option<(Atom, &'a [PhpType])> {
    let (name, args) = proxy_name_and_args(property.type_hint.as_ref())?;
    let is_current = args.len() == TAGGED_ARG_COUNT
        && literal_text(&args[2]) == Some(property.name.as_str())
        && args[3].base_name() == Some(owner_fqn);
    (!is_current).then_some((name, args))
}

/// Destructure a `HigherOrderCollectionProxy<…>` type hint into its
/// (possibly short) class name and generic arguments.
fn proxy_name_and_args(type_hint: Option<&PhpType>) -> Option<(Atom, &[PhpType])> {
    let TypeKind::Generic(generic) = type_hint?.kind() else {
        return None;
    };
    if short_name(&generic.name) != HIGHER_ORDER_COLLECTION_PROXY_SHORT {
        return None;
    }
    Some((generic.name, &generic.args))
}

/// The text of a string-literal type, or `None` for any other type.
///
/// Only used for the proxied method name, which never contains an escape
/// sequence, so the raw literal spelling is the content.
fn literal_text(ty: &PhpType) -> Option<&str> {
    match ty.as_literal()?.string_content()? {
        std::borrow::Cow::Borrowed(s) => Some(s),
        std::borrow::Cow::Owned(_) => None,
    }
}

// ─── Member injection ───────────────────────────────────────────────────────

/// How a proxied collection method's result depends on the member the
/// closure accesses.
///
/// Only a handful of the thirty-odd proxyable methods actually vary with
/// the member; the rest produce one type for the whole proxy, which is
/// built once when the context is created rather than once per grafted
/// member.
enum ProxiedResult {
    /// The same type whichever member is accessed — `filter` returns the
    /// collection, `contains` a `bool`, `first` a nullable item.
    Fixed(PhpType),
    /// The total of the member (`sum`).
    Summed,
    /// The member, or `null` for an empty collection (`min`, `max`).
    Extremum,
    /// A collection of the member (`map`).
    Mapped,
    /// A collection of the member's elements (`flatMap`).
    Flattened,
}

/// Everything the wrapping rules need about one proxy instantiation.
struct ProxyContext<'a> {
    /// The collection's key type (`TKey`).
    key: &'a PhpType,
    /// What the proxied method makes of each accessed member.
    result: ProxiedResult,
    /// The collection the proxy came from.
    collection: Atom,
    /// Whether that collection is an Eloquent one, and so cannot hold a
    /// mapped value that is not a model.  Resolved once: the check walks a
    /// parent chain and the answer is the same for every member.
    collection_is_eloquent: bool,
    class_loader: &'a ResolvedClassLoader<'a>,
}

/// Whether a resolved class is a proxy carrying the tag that
/// [`inject_higher_order_proxy_members`] needs.
///
/// Lets the generic resolver skip the copy-on-write it would otherwise pay
/// to hand this module a mutable class.
pub(crate) fn is_tagged_higher_order_proxy(fqn: &str, generic_args: &[PhpType]) -> bool {
    fqn == HIGHER_ORDER_COLLECTION_PROXY_FQN && generic_args.len() >= TAGGED_ARG_COUNT
}

/// Graft the value type's members onto a resolved
/// `HigherOrderCollectionProxy<TKey, TValue, 'method', Collection>`.
///
/// Every public instance property and method of the value type is added to
/// the proxy with its type rewritten to the proxied method's result, so
/// `$users->map->email` resolves to `Collection<int, string>` rather than
/// to `User::$email`'s own `string`.
///
/// Injected members *replace* same-named members already on the proxy: the
/// framework annotates the proxy `@mixin Enumerable<TKey, TValue>`, and a
/// value-type member of the same name always wins at runtime because
/// `__get` / `__call` never see the mixin.
pub(crate) fn inject_higher_order_proxy_members(
    result: &mut ClassInfo,
    generic_args: &[PhpType],
    class_loader: &ResolvedClassLoader<'_>,
    cache: Option<&ResolvedClassCache>,
) {
    if generic_args.len() < TAGGED_ARG_COUNT {
        return;
    }
    let (Some(method), Some(collection)) =
        (literal_text(&generic_args[2]), named_atom(&generic_args[3]))
    else {
        return;
    };
    let value = generic_args[1].unwrap_nullable();
    let Some(value_class) = load_value_class(value, class_loader, cache) else {
        return;
    };

    let ctx = ProxyContext::new(&generic_args[0], value, method, collection, class_loader);

    // Index the proxy's existing members once.  A value type can carry
    // hundreds of members, and a scan per member would make grafting them
    // quadratic in the size of the model.
    let mut property_slots: AtomMap<usize> = result
        .properties
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name, i))
        .collect();
    // Static methods are indexed too, so a grafted member *replaces* one
    // rather than pushing a second entry under the same name.  The proxy
    // picks up `Enumerable`'s statics (`make`, `wrap`, `empty`, …) through
    // its `@mixin`, and a value type with an instance method of that name
    // would otherwise leave the class holding both.
    let mut method_slots: AtomMap<usize> = result
        .methods
        .iter()
        .enumerate()
        .map(|(i, m)| (m.name, i))
        .collect();

    for property in value_class.properties.iter() {
        if property.is_static || property.visibility != Visibility::Public {
            continue;
        }
        let member = property
            .type_hint
            .as_ref()
            .or(property.native_type_hint.as_ref());
        let proxied = PropertyInfo {
            description: property.description.clone(),
            ..PropertyInfo::virtual_property_typed(&property.name, Some(&ctx.result_type(member)))
        };
        upsert_property(result, &mut property_slots, proxied);
    }

    for method_info in value_class.methods.iter() {
        if method_info.is_static
            || method_info.visibility != Visibility::Public
            || method_info.name.starts_with("__")
        {
            continue;
        }
        let member = method_info
            .return_type
            .as_ref()
            .or(method_info.native_return_type.as_ref());
        let proxied = MethodInfo {
            parameters: method_info.parameters.clone(),
            description: method_info.description.clone(),
            return_type: Some(ctx.result_type(member)),
            extras: MethodExtras {
                deprecation_message: method_info.deprecation_message().map(str::to_owned),
                ..Default::default()
            }
            .into_option(),
            ..MethodInfo::virtual_method(&method_info.name, None)
        };
        upsert_method(result, &mut method_slots, proxied);
    }
}

/// The interned name of a plain named type, or `None` for anything else.
///
/// Only ever reads a tag this module wrote, so the name is already a bare
/// FQN with no leading separator to strip.
fn named_atom(ty: &PhpType) -> Option<Atom> {
    match ty.kind() {
        TypeKind::Named(name) => Some(*name),
        _ => None,
    }
}

/// Fully resolve the collection's value type so its members can be read.
fn load_value_class(
    value: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
    cache: Option<&ResolvedClassCache>,
) -> Option<Arc<ClassInfo>> {
    let raw = class_loader(Fqn::new(value.base_name()?))?;
    let type_args: &[PhpType] = match value.kind() {
        TypeKind::Generic(generic) => &generic.args,
        _ => &[],
    };
    Some(resolve_class_fully_with_type_args(
        &raw,
        class_loader,
        cache,
        type_args,
    ))
}

fn upsert_property(class: &mut ClassInfo, slots: &mut AtomMap<usize>, property: PropertyInfo) {
    match slots.get(&property.name) {
        Some(&idx) => class.properties.make_mut()[idx] = Arc::new(property),
        None => {
            slots.insert(property.name, class.properties.len());
            class.properties.push(Arc::new(property));
        }
    }
}

fn upsert_method(class: &mut ClassInfo, slots: &mut AtomMap<usize>, method: MethodInfo) {
    match slots.get(&method.name) {
        Some(&idx) => class.methods.make_mut()[idx] = Arc::new(method),
        None => {
            slots.insert(method.name, class.methods.len());
            class.methods.push(Arc::new(method));
        }
    }
}

// ─── Result types ───────────────────────────────────────────────────────────

impl<'a> ProxyContext<'a> {
    /// Work out, once for the whole proxy, what the proxied method makes of
    /// an accessed member.
    fn new(
        key: &'a PhpType,
        value: &PhpType,
        method: &str,
        collection: Atom,
        class_loader: &'a ResolvedClassLoader<'a>,
    ) -> Self {
        let result = plan_result(method, key, value, collection);
        // Only the two shapes that build a collection around the member can
        // outgrow an Eloquent collection's `TModel of Model` bound, so the
        // parent-chain walk is worth paying for only then.
        let collection_is_eloquent =
            matches!(result, ProxiedResult::Mapped | ProxiedResult::Flattened)
                && class_loader(Fqn::new(collection.as_str()))
                    .is_some_and(|cls| is_subtype_of(&cls, ELOQUENT_COLLECTION_FQN, class_loader));

        Self {
            key,
            result,
            collection,
            collection_is_eloquent,
            class_loader,
        }
    }

    /// The type `$collection->{$method}(fn ($item) => …)` produces when the
    /// closure returns `member`.
    fn result_type(&self, member: Option<&PhpType>) -> PhpType {
        match &self.result {
            ProxiedResult::Fixed(ty) => ty.clone(),
            ProxiedResult::Summed => summed_type(member),
            ProxiedResult::Extremum => member.cloned().unwrap_or_else(PhpType::mixed).or_null(),
            ProxiedResult::Mapped => {
                let mapped = member.cloned().unwrap_or_else(PhpType::mixed);
                PhpType::generic_atom(
                    self.mapped_collection(&mapped),
                    vec![self.key.clone(), mapped],
                )
            }
            ProxiedResult::Flattened => {
                let element = member
                    .and_then(PhpType::iterable_element_type)
                    .unwrap_or_else(PhpType::mixed);
                PhpType::generic_atom(
                    self.mapped_collection(&element),
                    vec![ARRAY_KEY.clone(), element],
                )
            }
        }
    }

    /// The collection class that holds `mapped` values.
    ///
    /// `Eloquent\Collection` is declared `@template TModel of Model`, so a
    /// map to anything that is not a model produces a base
    /// `Support\Collection` — exactly what `Eloquent\Collection::map()` does
    /// at runtime when the callback stops returning models.
    fn mapped_collection(&self, mapped: &PhpType) -> Atom {
        if self.collection_is_eloquent && !is_eloquent_model_type(mapped, self.class_loader) {
            return *SUPPORT_COLLECTION_FQN;
        }
        self.collection
    }
}

/// Classify a proxyable collection method, building the result type up
/// front for every method whose result does not depend on the member.
///
/// Follows what each method does at runtime: `static` for the filtering
/// family, `bool` for the predicates, a number for the aggregates.  The
/// framework annotates the aggregates `mixed`, so there is no annotation to
/// mirror for those — the types here are derived from the reductions the
/// methods actually perform.
fn plan_result(method: &str, key: &PhpType, value: &PhpType, collection: Atom) -> ProxiedResult {
    let same_collection = || PhpType::generic_atom(collection, vec![key.clone(), value.clone()]);

    let fixed = match method {
        // `static` — the collection, unchanged.
        "each" | "filter" | "reject" | "skipUntil" | "skipWhile" | "sortBy" | "sortByDesc"
        | "takeUntil" | "takeWhile" | "unique" | "unless" | "until" | "when" => same_collection(),

        // The member becomes the collection's value.
        "map" => return ProxiedResult::Mapped,
        "flatMap" => return ProxiedResult::Flattened,

        // The member becomes the collection's key.
        "keyBy" => PhpType::generic_atom(collection, vec![ARRAY_KEY.clone(), value.clone()]),

        // Collections of collections.  `groupBy` is annotated `static<…,
        // static<…>>` and `Eloquent\Collection` does not override it, so the
        // outer stays on the collection it came from.  `partition` *is*
        // overridden, with an explicit `->toBase()`, so only that one drops
        // to the base collection.
        "groupBy" => PhpType::generic_atom(collection, vec![ARRAY_KEY.clone(), same_collection()]),
        "partition" => PhpType::generic_atom(
            *SUPPORT_COLLECTION_FQN,
            vec![PhpType::int(), same_collection()],
        ),

        // Single item, or `null` when nothing matched.
        "first" | "last" => value.clone().or_null(),

        // Predicates.
        "contains" | "doesntContain" | "every" | "hasMany" | "hasSole" | "some" => PhpType::bool(),

        // Aggregates.
        "average" | "avg" => {
            PhpType::union(vec![PhpType::int(), PhpType::float(), PhpType::null()])
        }
        "percentage" => PhpType::float().or_null(),
        "sum" => return ProxiedResult::Summed,
        // `reject(null)->reduce(…)` with no initial value, so an empty
        // collection reduces to `null` however non-nullable the member is.
        "max" | "min" => return ProxiedResult::Extremum,

        _ => PhpType::mixed(),
    };
    ProxiedResult::Fixed(fixed)
}

/// The type `sum` produces over members of type `member`.
///
/// `sum` seeds its reduction with `0` and adds each member to it, so it
/// never yields `null` however nullable the member is, and a member PHP
/// cannot add as one specific numeric type still comes back as a number.
fn summed_type(member: Option<&PhpType>) -> PhpType {
    let Some(member) = member else {
        return numeric();
    };
    let non_null = member.non_null_type();
    let summed = non_null.as_ref().unwrap_or(member);
    if summed.is_int() || summed.is_float() {
        summed.clone()
    } else {
        numeric()
    }
}

/// `int|float`, what PHP's `+` yields for operands that are not already one
/// specific numeric type.
fn numeric() -> PhpType {
    PhpType::union(vec![PhpType::int(), PhpType::float()])
}

fn is_eloquent_model_type(ty: &PhpType, class_loader: &ResolvedClassLoader<'_>) -> bool {
    is_subtype_of_named(ty, super::ELOQUENT_MODEL_FQN, class_loader)
}

#[cfg(test)]
#[path = "higher_order_proxy_tests.rs"]
mod tests;
