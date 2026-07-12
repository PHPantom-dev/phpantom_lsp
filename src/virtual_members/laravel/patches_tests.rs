use crate::atom::atom;
use std::sync::Arc;

use crate::php_type::PhpType;
use crate::test_fixtures::make_class;
use crate::types::MethodInfo;
use crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN;

use super::{
    CACHE_FACADE_FQN, CONDITIONABLE_FQN, DB_CONNECTION_FQN, DB_FACADE_FQN, apply_laravel_patches,
};

/// Create a method with a parsed `PhpType` return type.
///
/// Uses `MethodInfo::virtual_method` as the base and replaces the
/// return type with the given `PhpType` value.
fn make_method_typed(name: &str, return_type: Option<PhpType>) -> MethodInfo {
    MethodInfo {
        return_type,
        ..MethodInfo::virtual_method(name, None)
    }
}

// ─── Eloquent Builder __call patch ──────────────────────────────────────────

#[test]
fn builder_call_mixed_becomes_static() {
    let mut class = make_class(ELOQUENT_BUILDER_FQN);
    class.methods = vec![
        Arc::new(make_method_typed("__call", Some(PhpType::mixed()))),
        Arc::new(make_method_typed("__callStatic", Some(PhpType::mixed()))),
    ]
    .into();

    apply_laravel_patches(&mut class, ELOQUENT_BUILDER_FQN);

    for method in class.methods.iter() {
        if method.name == "__call" || method.name == "__callStatic" {
            assert_eq!(
                method.return_type.as_ref().unwrap().to_string(),
                "static",
                "{} return type should be patched to static",
                method.name
            );
        }
    }
}

#[test]
fn builder_call_non_mixed_is_not_patched() {
    let mut class = make_class(ELOQUENT_BUILDER_FQN);
    class.methods = vec![Arc::new(make_method_typed("__call", Some(PhpType::void())))].into();

    apply_laravel_patches(&mut class, ELOQUENT_BUILDER_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "void",
        "__call with non-mixed return should not be patched"
    );
}

#[test]
fn builder_call_no_return_type_is_not_patched() {
    let mut class = make_class(ELOQUENT_BUILDER_FQN);
    class.methods = vec![Arc::new(make_method_typed("__call", None))].into();

    apply_laravel_patches(&mut class, ELOQUENT_BUILDER_FQN);

    assert!(
        class.methods.iter().next().unwrap().return_type.is_none(),
        "__call with no return type should remain None"
    );
}

// ─── Conditionable when/unless patch ────────────────────────────────────────

#[test]
fn conditionable_when_union_with_template_becomes_this() {
    let mut class = make_class(CONDITIONABLE_FQN);
    let return_type = PhpType::Union(vec![
        PhpType::Named("$this".to_string()),
        PhpType::Named("TWhenReturnType".to_string()),
    ]);
    class.methods = vec![Arc::new(make_method_typed("when", Some(return_type)))].into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "$this"
    );
}

#[test]
fn conditionable_unless_union_with_template_becomes_this() {
    let mut class = make_class(CONDITIONABLE_FQN);
    let return_type = PhpType::Union(vec![
        PhpType::Named("$this".to_string()),
        PhpType::Named("TUnlessReturnType".to_string()),
    ]);
    class.methods = vec![Arc::new(make_method_typed("unless", Some(return_type)))].into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "$this"
    );
}

#[test]
fn conditionable_bare_template_return_becomes_this() {
    let mut class = make_class(CONDITIONABLE_FQN);
    let return_type = PhpType::Named("TWhenReturnType".to_string());
    class.methods = vec![Arc::new(make_method_typed("when", Some(return_type)))].into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "$this"
    );
}

#[test]
fn conditionable_static_union_with_template_becomes_this() {
    let mut class = make_class(CONDITIONABLE_FQN);
    let return_type = PhpType::Union(vec![
        PhpType::static_(),
        PhpType::Named("TWhenReturnType".to_string()),
    ]);
    class.methods = vec![Arc::new(make_method_typed("when", Some(return_type)))].into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "$this"
    );
}

#[test]
fn conditionable_self_return_is_not_patched() {
    let mut class = make_class(CONDITIONABLE_FQN);
    let return_type = PhpType::Named("$this".to_string());
    class.methods = vec![Arc::new(make_method_typed("when", Some(return_type)))].into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "$this",
        "Already $this should not be changed"
    );
}

#[test]
fn conditionable_static_return_is_not_patched() {
    let mut class = make_class(CONDITIONABLE_FQN);
    let return_type = PhpType::static_();
    class.methods = vec![Arc::new(make_method_typed("when", Some(return_type)))].into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "static",
        "static-only return should not be changed"
    );
}

#[test]
fn conditionable_no_return_type_is_not_patched() {
    let mut class = make_class(CONDITIONABLE_FQN);
    class.methods = vec![Arc::new(make_method_typed("when", None))].into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert!(
        class.methods.iter().next().unwrap().return_type.is_none(),
        "when with no return type should remain None"
    );
}

#[test]
fn conditionable_other_method_is_not_patched() {
    let mut class = make_class(CONDITIONABLE_FQN);
    let return_type = PhpType::Union(vec![
        PhpType::Named("$this".to_string()),
        PhpType::Named("TWhenReturnType".to_string()),
    ]);
    class.methods = vec![Arc::new(make_method_typed(
        "doSomething",
        Some(return_type.clone()),
    ))]
    .into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        return_type.to_string(),
        "Non-when/unless methods should not be patched"
    );
}

// ─── Conditionable patch applied via trait usage ────────────────────────────

#[test]
fn class_using_conditionable_trait_gets_when_patched() {
    let mut class = make_class("App\\Services\\Pipeline");
    class.used_traits = vec![atom(CONDITIONABLE_FQN)];
    let return_type = PhpType::Union(vec![
        PhpType::Named("$this".to_string()),
        PhpType::Named("TWhenReturnType".to_string()),
    ]);
    class.methods = vec![Arc::new(make_method_typed("when", Some(return_type)))].into();

    apply_laravel_patches(&mut class, "App\\Services\\Pipeline");

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "$this"
    );
}

#[test]
fn class_using_conditionable_short_name_gets_patched() {
    let mut class = make_class("App\\Services\\Pipeline");
    class.used_traits = vec![atom("Conditionable")];
    let return_type = PhpType::Union(vec![
        PhpType::Named("$this".to_string()),
        PhpType::Named("TWhenReturnType".to_string()),
    ]);
    class.methods = vec![Arc::new(make_method_typed("when", Some(return_type)))].into();

    apply_laravel_patches(&mut class, "App\\Services\\Pipeline");

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "$this"
    );
}

#[test]
fn class_without_conditionable_is_not_patched() {
    let mut class = make_class("App\\Services\\Pipeline");
    class.used_traits = vec![atom("SomeTrait")];
    let return_type = PhpType::Union(vec![
        PhpType::Named("$this".to_string()),
        PhpType::Named("TWhenReturnType".to_string()),
    ]);
    class.methods = vec![Arc::new(make_method_typed(
        "when",
        Some(return_type.clone()),
    ))]
    .into();

    apply_laravel_patches(&mut class, "App\\Services\\Pipeline");

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        return_type.to_string(),
        "Non-Conditionable classes should not be patched"
    );
}

// ─── Builder gets both patches ──────────────────────────────────────────────

#[test]
fn builder_gets_both_call_and_when_patches() {
    let mut class = make_class(ELOQUENT_BUILDER_FQN);
    class.methods = vec![
        Arc::new(make_method_typed("__call", Some(PhpType::mixed()))),
        Arc::new(make_method_typed(
            "when",
            Some(PhpType::Union(vec![
                PhpType::Named("$this".to_string()),
                PhpType::Named("TWhenReturnType".to_string()),
            ])),
        )),
        Arc::new(make_method_typed(
            "unless",
            Some(PhpType::Union(vec![
                PhpType::Named("$this".to_string()),
                PhpType::Named("TUnlessReturnType".to_string()),
            ])),
        )),
    ]
    .into();

    apply_laravel_patches(&mut class, ELOQUENT_BUILDER_FQN);

    let methods: Vec<_> = class.methods.iter().collect();
    assert_eq!(
        methods[0].return_type.as_ref().unwrap().to_string(),
        "static"
    );
    assert_eq!(
        methods[1].return_type.as_ref().unwrap().to_string(),
        "$this"
    );
    assert_eq!(
        methods[2].return_type.as_ref().unwrap().to_string(),
        "$this"
    );
}

// ─── Template param heuristic edge cases ────────────────────────────────────

#[test]
fn union_with_null_and_template_is_patched() {
    let mut class = make_class(CONDITIONABLE_FQN);
    let return_type = PhpType::Union(vec![
        PhpType::Named("$this".to_string()),
        PhpType::null(),
        PhpType::Named("TWhenReturnType".to_string()),
    ]);
    class.methods = vec![Arc::new(make_method_typed("when", Some(return_type)))].into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "$this"
    );
}

#[test]
fn union_of_only_self_types_is_not_patched() {
    let mut class = make_class(CONDITIONABLE_FQN);
    let return_type = PhpType::Union(vec![
        PhpType::Named("$this".to_string()),
        PhpType::static_(),
    ]);
    class.methods = vec![Arc::new(make_method_typed(
        "when",
        Some(return_type.clone()),
    ))]
    .into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        return_type.to_string(),
        "Union of self-like types should not be patched"
    );
}

// ─── DB select return type patches ──────────────────────────────────────────

#[test]
fn db_facade_select_bare_array_becomes_typed() {
    let mut class = make_class(DB_FACADE_FQN);
    class.methods = vec![Arc::new(make_method_typed(
        "select",
        Some(PhpType::array()),
    ))]
    .into();

    apply_laravel_patches(&mut class, DB_FACADE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "array<int, stdClass>",
        "select() bare array should become array<int, stdClass>"
    );
}

#[test]
fn db_facade_select_from_write_connection_becomes_typed() {
    let mut class = make_class(DB_FACADE_FQN);
    class.methods = vec![Arc::new(make_method_typed(
        "selectFromWriteConnection",
        Some(PhpType::array()),
    ))]
    .into();

    apply_laravel_patches(&mut class, DB_FACADE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "array<int, stdClass>",
    );
}

#[test]
fn db_facade_select_result_sets_becomes_typed() {
    let mut class = make_class(DB_FACADE_FQN);
    class.methods = vec![Arc::new(make_method_typed(
        "selectResultSets",
        Some(PhpType::array()),
    ))]
    .into();

    apply_laravel_patches(&mut class, DB_FACADE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "array<int, stdClass>",
    );
}

#[test]
fn db_facade_select_one_mixed_becomes_nullable_stdclass() {
    let mut class = make_class(DB_FACADE_FQN);
    class.methods = vec![Arc::new(make_method_typed(
        "selectOne",
        Some(PhpType::mixed()),
    ))]
    .into();

    apply_laravel_patches(&mut class, DB_FACADE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "?stdClass",
        "selectOne() mixed should become ?stdClass"
    );
}

#[test]
fn db_connection_select_bare_array_becomes_typed() {
    let mut class = make_class(DB_CONNECTION_FQN);
    class.methods = vec![Arc::new(make_method_typed(
        "select",
        Some(PhpType::array()),
    ))]
    .into();

    apply_laravel_patches(&mut class, DB_CONNECTION_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        "array<int, stdClass>",
        "Connection::select() bare array should become array<int, stdClass>"
    );
}

#[test]
fn db_select_non_array_return_is_not_patched() {
    let mut class = make_class(DB_FACADE_FQN);
    let original = PhpType::Generic(
        "array".to_string(),
        vec![PhpType::string(), PhpType::mixed()],
    );
    class.methods = vec![Arc::new(make_method_typed(
        "select",
        Some(original.clone()),
    ))]
    .into();

    apply_laravel_patches(&mut class, DB_FACADE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        original.to_string(),
        "select() with non-bare-array return type should not be patched"
    );
}

#[test]
fn db_select_one_non_mixed_is_not_patched() {
    let mut class = make_class(DB_FACADE_FQN);
    let original = PhpType::object();
    class.methods = vec![Arc::new(make_method_typed(
        "selectOne",
        Some(original.clone()),
    ))]
    .into();

    apply_laravel_patches(&mut class, DB_FACADE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        original.to_string(),
        "selectOne() with non-mixed return type should not be patched"
    );
}

#[test]
fn db_other_method_is_not_patched() {
    let mut class = make_class(DB_FACADE_FQN);
    let original = PhpType::array();
    class.methods = vec![Arc::new(make_method_typed(
        "insert",
        Some(original.clone()),
    ))]
    .into();

    apply_laravel_patches(&mut class, DB_FACADE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        original.to_string(),
        "non-select methods should not be patched"
    );
}

#[test]
fn fqn_in_return_type_is_not_treated_as_template() {
    let mut class = make_class(CONDITIONABLE_FQN);
    let return_type = PhpType::Union(vec![
        PhpType::Named("$this".to_string()),
        PhpType::Named("App\\Models\\User".to_string()),
    ]);
    class.methods = vec![Arc::new(make_method_typed(
        "when",
        Some(return_type.clone()),
    ))]
    .into();

    apply_laravel_patches(&mut class, CONDITIONABLE_FQN);

    assert_eq!(
        class
            .methods
            .iter()
            .next()
            .unwrap()
            .return_type
            .as_ref()
            .unwrap()
            .to_string(),
        return_type.to_string(),
        "FQN types should not trigger the template heuristic"
    );
}

// ─── Cache facade generics restoration ──────────────────────────────────────

/// Build the `Cache` facade's generated `remember` shape: a `mixed`
/// return with a `$ttl` closure and the value-producing `$callback`.
fn make_cache_remember() -> MethodInfo {
    MethodInfo {
        return_type: Some(PhpType::mixed()),
        parameters: vec![
            crate::test_fixtures::make_param("$key", Some("string"), true),
            crate::test_fixtures::make_param("$ttl", Some("\\Closure|int|null"), true),
            crate::test_fixtures::make_param("$callback", Some("\\Closure"), true),
        ],
        ..MethodInfo::virtual_method("remember", None)
    }
}

#[test]
fn cache_facade_remember_gets_template() {
    let mut class = make_class(CACHE_FACADE_FQN);
    class.methods = vec![Arc::new(make_cache_remember())].into();

    apply_laravel_patches(&mut class, CACHE_FACADE_FQN);

    let method = class.methods.iter().next().unwrap();
    assert_eq!(
        method.return_type.as_ref().unwrap().to_string(),
        "TCacheValue",
        "return type should be the restored template"
    );
    assert_eq!(
        method.template_params,
        vec![atom("TCacheValue")],
        "the method-level template should be added"
    );
    assert_eq!(
        method.template_bindings,
        vec![(atom("TCacheValue"), atom("$callback"))],
        "TCacheValue should bind from the $callback closure"
    );
    // The callback param is retyped as a closure returning the template.
    let callback = method
        .parameters
        .iter()
        .find(|p| p.name.as_str() == "$callback")
        .unwrap();
    assert!(
        callback
            .type_hint
            .as_ref()
            .unwrap()
            .to_string()
            .contains("TCacheValue"),
        "the callback should return TCacheValue, got: {:?}",
        callback.type_hint
    );
}

#[test]
fn cache_facade_non_callback_method_untouched() {
    let mut class = make_class(CACHE_FACADE_FQN);
    // `get` returns `mixed` but has no `$callback` closure.
    class.methods = vec![Arc::new(MethodInfo {
        return_type: Some(PhpType::mixed()),
        parameters: vec![crate::test_fixtures::make_param(
            "$key",
            Some("string"),
            true,
        )],
        ..MethodInfo::virtual_method("get", None)
    })]
    .into();

    apply_laravel_patches(&mut class, CACHE_FACADE_FQN);

    let method = class.methods.iter().next().unwrap();
    assert_eq!(
        method.return_type.as_ref().unwrap().to_string(),
        "mixed",
        "methods without a $callback closure should be left as-is"
    );
    assert!(method.template_params.is_empty());
}
