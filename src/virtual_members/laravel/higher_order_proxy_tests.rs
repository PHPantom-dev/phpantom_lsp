//! Unit tests for higher-order collection proxy tagging and the result
//! types of each proxied collection method.

use std::sync::Arc;

use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::PhpType;
use crate::test_fixtures::{make_class, make_method, make_property, no_loader};
use crate::types::{ClassInfo, ELOQUENT_COLLECTION_FQN, Visibility};

use super::*;

const SUPPORT_COLLECTION: &str = "Illuminate\\Support\\Collection";
const ELOQUENT_MODEL: &str = "Illuminate\\Database\\Eloquent\\Model";
const USER: &str = "App\\User";

/// A collection carrying the framework's untagged proxy properties.
fn collection_with_proxy_properties(fqn: &str, names: &[&str]) -> ClassInfo {
    let mut class = make_class(fqn);
    class.properties = names
        .iter()
        .map(|name| {
            Arc::new(make_property(
                name,
                Some("Illuminate\\Support\\HigherOrderCollectionProxy<TKey, TValue>"),
            ))
        })
        .collect::<Vec<_>>()
        .into();
    class
}

fn property_type(class: &ClassInfo, name: &str) -> String {
    class
        .properties
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no property `{name}`"))
        .type_hint
        .as_ref()
        .expect("property has no type")
        .to_string()
}

// ─── Tagging ────────────────────────────────────────────────────────────────

#[test]
fn tagging_records_the_method_name_and_owning_collection() {
    let mut class = collection_with_proxy_properties(SUPPORT_COLLECTION, &["map", "filter"]);

    tag_higher_order_proxy_properties(&mut class, SUPPORT_COLLECTION);

    assert_eq!(
        property_type(&class, "map"),
        "Illuminate\\Support\\HigherOrderCollectionProxy<TKey, TValue, 'map', \
         Illuminate\\Support\\Collection>"
    );
    assert_eq!(
        property_type(&class, "filter"),
        "Illuminate\\Support\\HigherOrderCollectionProxy<TKey, TValue, 'filter', \
         Illuminate\\Support\\Collection>"
    );
}

/// A subclass inherits the parent's already-tagged properties, and must
/// rewrite the collection to itself — that is what keeps a proxied `filter`
/// on the subclass rather than degrading it to the parent.
#[test]
fn tagging_rewrites_an_inherited_tag_to_the_subclass() {
    let mut parent = collection_with_proxy_properties(SUPPORT_COLLECTION, &["filter"]);
    tag_higher_order_proxy_properties(&mut parent, SUPPORT_COLLECTION);

    let mut child = parent.clone();
    child.name = crate::atom::atom(ELOQUENT_COLLECTION_FQN);
    tag_higher_order_proxy_properties(&mut child, ELOQUENT_COLLECTION_FQN);

    assert_eq!(
        property_type(&child, "filter"),
        "Illuminate\\Support\\HigherOrderCollectionProxy<TKey, TValue, 'filter', \
         Illuminate\\Database\\Eloquent\\Collection>"
    );
}

#[test]
fn tagging_is_idempotent() {
    let mut class = collection_with_proxy_properties(SUPPORT_COLLECTION, &["map"]);
    tag_higher_order_proxy_properties(&mut class, SUPPORT_COLLECTION);
    let once = property_type(&class, "map");

    tag_higher_order_proxy_properties(&mut class, SUPPORT_COLLECTION);

    assert_eq!(property_type(&class, "map"), once);
}

#[test]
fn tagging_leaves_other_properties_alone() {
    let mut class = collection_with_proxy_properties(SUPPORT_COLLECTION, &["map"]);
    class.properties.push(Arc::new(make_property(
        "items",
        Some("array<TKey, TValue>"),
    )));

    tag_higher_order_proxy_properties(&mut class, SUPPORT_COLLECTION);

    assert_eq!(property_type(&class, "items"), "array<TKey, TValue>");
}

/// A short-name annotation (the framework imports the proxy rather than
/// writing it out) is recognised, and the short name is preserved so the
/// type still resolves through the declaring file's imports.
#[test]
fn tagging_accepts_a_short_proxy_name() {
    let mut class = make_class(SUPPORT_COLLECTION);
    class.properties = vec![Arc::new(make_property(
        "map",
        Some("HigherOrderCollectionProxy<TKey, TValue>"),
    ))]
    .into();

    tag_higher_order_proxy_properties(&mut class, SUPPORT_COLLECTION);

    assert_eq!(
        property_type(&class, "map"),
        "HigherOrderCollectionProxy<TKey, TValue, 'map', Illuminate\\Support\\Collection>"
    );
}

#[test]
fn tagging_fills_in_missing_key_and_value_arguments() {
    let mut class = make_class(SUPPORT_COLLECTION);
    class.properties = vec![Arc::new(make_property(
        "map",
        Some("HigherOrderCollectionProxy<TValue>"),
    ))]
    .into();

    tag_higher_order_proxy_properties(&mut class, SUPPORT_COLLECTION);

    assert_eq!(
        property_type(&class, "map"),
        "HigherOrderCollectionProxy<TValue, mixed, 'map', Illuminate\\Support\\Collection>"
    );
}

// ─── Result types ───────────────────────────────────────────────────────────

/// Resolve `$collection-><method>-><member>` where the member has type
/// `member`, against a plain `Support\Collection<int, User>`.
fn result_type(method: &str, member: &str) -> String {
    context(SUPPORT_COLLECTION, method, &no_loader)
        .result_type(Some(&PhpType::parse(member)))
        .to_string()
}

fn context<'a>(
    collection_fqn: &str,
    method: &str,
    class_loader: &'a ResolvedClassLoader<'a>,
) -> ProxyContext<'a> {
    static KEY: std::sync::LazyLock<PhpType> = std::sync::LazyLock::new(PhpType::int);
    ProxyContext::new(
        &KEY,
        &PhpType::named(crate::atom::atom(USER)),
        method,
        crate::atom::atom(collection_fqn),
        class_loader,
    )
}

#[test]
fn map_collects_the_member_type() {
    assert_eq!(
        result_type("map", "string"),
        "Illuminate\\Support\\Collection<int, string>"
    );
}

#[test]
fn flat_map_unwraps_one_level_and_rekeys() {
    assert_eq!(
        result_type("flatMap", "array<string>"),
        "Illuminate\\Support\\Collection<array-key, string>"
    );
}

#[test]
fn flat_map_falls_back_to_mixed_for_a_non_iterable_member() {
    assert_eq!(
        result_type("flatMap", "string"),
        "Illuminate\\Support\\Collection<array-key, mixed>"
    );
}

#[test]
fn filtering_methods_return_the_collection_unchanged() {
    for method in [
        "each",
        "filter",
        "reject",
        "skipUntil",
        "skipWhile",
        "sortBy",
        "sortByDesc",
        "takeUntil",
        "takeWhile",
        "unique",
        "unless",
        "until",
        "when",
    ] {
        assert_eq!(
            result_type(method, "bool"),
            "Illuminate\\Support\\Collection<int, App\\User>",
            "for `{method}`"
        );
    }
}

#[test]
fn key_by_moves_the_member_into_the_key() {
    assert_eq!(
        result_type("keyBy", "string"),
        "Illuminate\\Support\\Collection<array-key, App\\User>"
    );
}

#[test]
fn grouping_methods_nest_the_collection() {
    assert_eq!(
        result_type("groupBy", "string"),
        "Illuminate\\Support\\Collection<array-key, Illuminate\\Support\\Collection<int, App\\User>>"
    );
    assert_eq!(
        result_type("partition", "bool"),
        "Illuminate\\Support\\Collection<int, Illuminate\\Support\\Collection<int, App\\User>>"
    );
}

#[test]
fn first_and_last_are_nullable_items() {
    assert_eq!(result_type("first", "bool"), "?App\\User");
    assert_eq!(result_type("last", "bool"), "?App\\User");
}

#[test]
fn predicates_return_bool() {
    for method in [
        "contains",
        "doesntContain",
        "every",
        "hasMany",
        "hasSole",
        "some",
    ] {
        assert_eq!(result_type(method, "string"), "bool", "for `{method}`");
    }
}

#[test]
fn aggregates_are_numbers() {
    assert_eq!(result_type("avg", "int"), "int|float|null");
    assert_eq!(result_type("average", "int"), "int|float|null");
    assert_eq!(result_type("percentage", "bool"), "?float");
    assert_eq!(result_type("sum", "int"), "int");
    assert_eq!(result_type("sum", "float"), "float");
}

/// `sum` reduces from `0`, so it returns a number whatever the member is
/// and never returns `null` — a nullable numeric column (`?float`) totals
/// to a plain `float`, not to `?float`.
#[test]
fn sum_is_never_null_and_always_a_number() {
    assert_eq!(result_type("sum", "?float"), "float");
    assert_eq!(result_type("sum", "?int"), "int");
    assert_eq!(result_type("sum", "int|float|null"), "int|float");
    assert_eq!(result_type("sum", "string"), "int|float");
    assert_eq!(result_type("sum", "mixed"), "int|float");
}

/// `min` / `max` reduce with no initial value, so an empty collection
/// yields `null` even when the member itself is not nullable.
#[test]
fn min_and_max_are_nullable_for_an_empty_collection() {
    assert_eq!(result_type("min", "int"), "?int");
    assert_eq!(result_type("max", "int"), "?int");
    assert_eq!(result_type("max", "?int"), "?int");
    assert_eq!(result_type("max", "string"), "?string");
}

/// A name that is not a known proxy contributes no type information rather
/// than an incorrect one.
#[test]
fn an_unrecognised_method_resolves_to_mixed() {
    assert_eq!(result_type("notAProxyMethod", "string"), "mixed");
}

// ─── Eloquent collection degradation ────────────────────────────────────────

fn eloquent_loader(name: Fqn<'_>) -> Option<Arc<ClassInfo>> {
    let name = name.as_str();
    let class = match name {
        ELOQUENT_COLLECTION_FQN => make_class(ELOQUENT_COLLECTION_FQN),
        ELOQUENT_MODEL => make_class(ELOQUENT_MODEL),
        USER => {
            let mut user = make_class(USER);
            user.parent_class = Some(crate::atom::atom(ELOQUENT_MODEL));
            user
        }
        _ => return None,
    };
    Some(Arc::new(class))
}

fn eloquent_context(method: &str) -> ProxyContext<'static> {
    context(ELOQUENT_COLLECTION_FQN, method, &eloquent_loader)
}

/// `Eloquent\Collection` is `@template TModel of Model`, so it cannot hold
/// the result of mapping to a scalar.
#[test]
fn mapping_an_eloquent_collection_to_a_scalar_degrades_to_the_base_collection() {
    assert_eq!(
        eloquent_context("map")
            .result_type(Some(&PhpType::string()))
            .to_string(),
        "Illuminate\\Support\\Collection<int, string>"
    );
}

#[test]
fn mapping_an_eloquent_collection_to_a_model_keeps_it() {
    assert_eq!(
        eloquent_context("map")
            .result_type(Some(&PhpType::named(crate::atom::atom(USER))))
            .to_string(),
        "Illuminate\\Database\\Eloquent\\Collection<int, App\\User>"
    );
}

/// Filtering never changes the value type, so the Eloquent collection is
/// kept even though the degradation rule exists for `map`.
#[test]
fn filtering_an_eloquent_collection_keeps_it() {
    assert_eq!(
        eloquent_context("filter")
            .result_type(Some(&PhpType::bool()))
            .to_string(),
        "Illuminate\\Database\\Eloquent\\Collection<int, App\\User>"
    );
}

// ─── Member injection ───────────────────────────────────────────────────────

fn proxy_loader(name: Fqn<'_>) -> Option<Arc<ClassInfo>> {
    let name = name.as_str();
    if name != USER {
        return None;
    }
    let mut user = make_class(USER);
    let mut hidden = make_property("secret", Some("string"));
    hidden.visibility = Visibility::Private;
    let mut shared = make_property("registry", Some("array"));
    shared.is_static = true;
    user.properties = vec![
        Arc::new(make_property("email", Some("string"))),
        Arc::new(hidden),
        Arc::new(shared),
    ]
    .into();
    user.methods = vec![
        Arc::new(make_method("isActive", Some("bool"))),
        Arc::new(make_method("__toString", Some("string"))),
    ]
    .into();
    Some(Arc::new(user))
}

/// The four generic arguments the tagger writes onto a proxy property.
fn tag_args(method: PhpType, collection: PhpType) -> Vec<PhpType> {
    vec![
        PhpType::int(),
        PhpType::named(crate::atom::atom(USER)),
        method,
        collection,
    ]
}

fn injected_proxy(method: &str) -> ClassInfo {
    let mut proxy = make_class(HIGHER_ORDER_COLLECTION_PROXY_FQN);
    let args = tag_args(
        PhpType::literal_string_value(method),
        PhpType::named(crate::atom::atom(SUPPORT_COLLECTION)),
    );
    inject_higher_order_proxy_members(&mut proxy, &args, &proxy_loader, None);
    proxy
}

#[test]
fn injection_grafts_public_members_with_proxied_types() {
    let proxy = injected_proxy("map");

    assert_eq!(
        property_type(&proxy, "email"),
        "Illuminate\\Support\\Collection<int, string>"
    );
    let is_active = proxy
        .methods
        .iter()
        .find(|m| m.name == "isActive")
        .expect("isActive was not grafted");
    assert_eq!(
        is_active.return_type.as_ref().unwrap().to_string(),
        "Illuminate\\Support\\Collection<int, bool>"
    );
}

/// `__get` / `__call` are how the proxy works, not members it forwards to.
#[test]
fn injection_skips_non_public_static_and_magic_members() {
    let proxy = injected_proxy("map");

    assert!(proxy.properties.iter().all(|p| p.name != "secret"));
    assert!(proxy.properties.iter().all(|p| p.name != "registry"));
    assert!(proxy.methods.iter().all(|m| m.name != "__toString"));
}

/// The proxy picks up `Enumerable`'s statics (`make`, `wrap`, `empty`, …)
/// through its `@mixin`, but has no such method at runtime — `__call`
/// reaches the value type's instance method instead.  The graft must
/// therefore replace the static rather than leave the class holding two
/// methods of the same name.
#[test]
fn a_grafted_member_replaces_a_same_named_static() {
    fn maker_loader(name: Fqn<'_>) -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name != USER {
            return None;
        }
        let mut user = make_class(USER);
        user.methods = vec![Arc::new(make_method("make", Some("bool")))].into();
        Some(Arc::new(user))
    }

    let mut proxy = make_class(HIGHER_ORDER_COLLECTION_PROXY_FQN);
    let mut inherited = make_method("make", Some("static"));
    inherited.is_static = true;
    proxy.methods = vec![Arc::new(inherited)].into();

    let args = tag_args(
        PhpType::literal_string_value("map"),
        PhpType::named(crate::atom::atom(SUPPORT_COLLECTION)),
    );
    inject_higher_order_proxy_members(&mut proxy, &args, &maker_loader, None);

    let makes: Vec<_> = proxy.methods.iter().filter(|m| m.name == "make").collect();
    assert_eq!(makes.len(), 1, "the static was duplicated, not replaced");
    assert!(!makes[0].is_static, "the graft is an instance method");
    assert_eq!(
        makes[0].return_type.as_ref().unwrap().to_string(),
        "Illuminate\\Support\\Collection<int, bool>"
    );
}

/// Without the two extra tag arguments there is nothing to resolve against,
/// so the proxy is left as the framework declared it.
#[test]
fn injection_is_a_no_op_for_an_untagged_proxy() {
    let mut proxy = make_class(HIGHER_ORDER_COLLECTION_PROXY_FQN);
    let args = vec![PhpType::int(), PhpType::named(crate::atom::atom(USER))];

    inject_higher_order_proxy_members(&mut proxy, &args, &proxy_loader, None);

    assert!(proxy.properties.is_empty());
    assert!(proxy.methods.is_empty());
}

/// An unresolvable value type leaves the proxy untouched rather than
/// producing members typed against nothing.
#[test]
fn injection_is_a_no_op_when_the_value_type_is_unknown() {
    let mut proxy = make_class(HIGHER_ORDER_COLLECTION_PROXY_FQN);
    let args = vec![
        PhpType::int(),
        PhpType::named(crate::atom::atom("App\\Missing")),
        PhpType::literal_string_value("map"),
        PhpType::named(crate::atom::atom(SUPPORT_COLLECTION)),
    ];

    inject_higher_order_proxy_members(&mut proxy, &args, &proxy_loader, None);

    assert!(proxy.properties.is_empty());
    assert!(proxy.methods.is_empty());
}

/// A hand-written `HigherOrderCollectionProxy<…>` annotation can carry four
/// arguments without carrying the *shapes* the tagger writes.  Neither a
/// method name that is not a literal nor a collection that is not a plain
/// class name says which method is proxied onto what, so the proxy is left
/// as it was declared.
#[test]
fn injection_is_a_no_op_for_a_malformed_tag() {
    let collection = PhpType::named(crate::atom::atom(SUPPORT_COLLECTION));

    let mut proxy = make_class(HIGHER_ORDER_COLLECTION_PROXY_FQN);
    let args = tag_args(PhpType::string(), collection);
    inject_higher_order_proxy_members(&mut proxy, &args, &proxy_loader, None);
    assert!(proxy.properties.is_empty(), "method name is not a literal");
    assert!(proxy.methods.is_empty(), "method name is not a literal");

    let mut proxy = make_class(HIGHER_ORDER_COLLECTION_PROXY_FQN);
    let args = tag_args(
        PhpType::literal_string_value("map"),
        PhpType::parse("Illuminate\\Support\\Collection<int, App\\User>"),
    );
    inject_higher_order_proxy_members(&mut proxy, &args, &proxy_loader, None);
    assert!(proxy.properties.is_empty(), "collection is not a bare name");
    assert!(proxy.methods.is_empty(), "collection is not a bare name");
}

/// The value type is resolved with its own generic arguments applied, so a
/// collection of a parameterised class proxies the *substituted* member
/// type rather than the template parameter's name.
#[test]
fn injection_applies_the_value_types_generic_arguments() {
    fn box_loader(name: Fqn<'_>) -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name != "App\\Box" {
            return None;
        }
        let mut boxed = make_class("App\\Box");
        boxed.template_params = vec![crate::atom::atom("T")];
        boxed.properties = vec![Arc::new(make_property("item", Some("T")))].into();
        boxed.methods = vec![Arc::new(make_method("unwrap", Some("T")))].into();
        Some(Arc::new(boxed))
    }

    let mut proxy = make_class(HIGHER_ORDER_COLLECTION_PROXY_FQN);
    let args = vec![
        PhpType::int(),
        PhpType::parse("App\\Box<string>"),
        PhpType::literal_string_value("map"),
        PhpType::named(crate::atom::atom(SUPPORT_COLLECTION)),
    ];

    inject_higher_order_proxy_members(&mut proxy, &args, &box_loader, None);

    assert_eq!(
        property_type(&proxy, "item"),
        "Illuminate\\Support\\Collection<int, string>"
    );
    let unwrap = proxy
        .methods
        .iter()
        .find(|m| m.name == "unwrap")
        .expect("unwrap was not grafted");
    assert_eq!(
        unwrap.return_type.as_ref().unwrap().to_string(),
        "Illuminate\\Support\\Collection<int, string>"
    );
}

/// The proxy is annotated `@mixin Enumerable`, so it already carries members
/// named after collection methods.  A value-type member of the same name
/// replaces it — at runtime `__get` / `__call` fire first and never reach
/// the mixin.
#[test]
fn injection_replaces_same_named_members_already_on_the_proxy() {
    let mut proxy = make_class(HIGHER_ORDER_COLLECTION_PROXY_FQN);
    proxy.properties = vec![Arc::new(make_property("email", Some("array")))].into();
    proxy.methods = vec![Arc::new(make_method("isActive", Some("int")))].into();
    let args = tag_args(
        PhpType::literal_string_value("map"),
        PhpType::named(crate::atom::atom(SUPPORT_COLLECTION)),
    );

    inject_higher_order_proxy_members(&mut proxy, &args, &proxy_loader, None);

    assert_eq!(
        proxy
            .properties
            .iter()
            .filter(|p| p.name == "email")
            .count(),
        1,
        "the grafted property should replace, not duplicate"
    );
    assert_eq!(
        property_type(&proxy, "email"),
        "Illuminate\\Support\\Collection<int, string>"
    );

    let is_active: Vec<_> = proxy
        .methods
        .iter()
        .filter(|m| m.name == "isActive")
        .collect();
    assert_eq!(
        is_active.len(),
        1,
        "the grafted method should replace, not duplicate"
    );
    assert_eq!(
        is_active[0].return_type.as_ref().unwrap().to_string(),
        "Illuminate\\Support\\Collection<int, bool>"
    );
}

/// `Eloquent\Collection` overrides `partition()` with an explicit
/// `->toBase()`, so only that one drops to the base collection.  `groupBy()`
/// is not overridden and keeps `Support\Collection`'s `static<…, static<…>>`
/// annotation, so its outer collection stays Eloquent.
#[test]
fn grouping_an_eloquent_collection_keeps_it_but_partitioning_does_not() {
    assert_eq!(
        eloquent_context("groupBy")
            .result_type(Some(&PhpType::string()))
            .to_string(),
        "Illuminate\\Database\\Eloquent\\Collection<array-key, \
         Illuminate\\Database\\Eloquent\\Collection<int, App\\User>>"
    );
    assert_eq!(
        eloquent_context("partition")
            .result_type(Some(&PhpType::bool()))
            .to_string(),
        "Illuminate\\Support\\Collection<int, \
         Illuminate\\Database\\Eloquent\\Collection<int, App\\User>>"
    );
}
