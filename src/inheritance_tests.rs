use super::*;
use crate::atom::{AtomMap, atom};
use crate::php_type::PhpType;
use crate::types::ClassLikeKind;

/// Helper to build a `HashMap<String, PhpType>` from `(&str, &str)` pairs.
fn make_subs(pairs: &[(&str, &str)]) -> HashMap<String, PhpType> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), PhpType::parse(v)))
        .collect()
}

#[test]
fn test_apply_substitution_direct() {
    let subs = make_subs(&[("TValue", "Language"), ("TKey", "int")]);

    assert_eq!(apply_substitution("TValue", &subs), "Language");
    assert_eq!(apply_substitution("TKey", &subs), "int");
    assert_eq!(apply_substitution("string", &subs), "string");
}

#[test]
fn test_apply_substitution_nullable() {
    let subs = make_subs(&[("TValue", "Language")]);

    assert_eq!(apply_substitution("?TValue", &subs), "?Language");
}

#[test]
fn test_apply_substitution_union() {
    let subs = make_subs(&[("TValue", "Language")]);

    assert_eq!(apply_substitution("TValue|null", &subs), "Language|null");
    assert_eq!(
        apply_substitution("TValue|string", &subs),
        "Language|string"
    );
}

#[test]
fn test_apply_substitution_intersection() {
    let subs = make_subs(&[("TValue", "Language")]);

    assert_eq!(
        apply_substitution("TValue&Countable", &subs),
        "Language&Countable"
    );
}

#[test]
fn test_apply_substitution_generic() {
    let subs = make_subs(&[("TKey", "int"), ("TValue", "Language")]);

    assert_eq!(
        apply_substitution("array<TKey, TValue>", &subs),
        "array<int, Language>"
    );
}

#[test]
fn test_apply_substitution_nested_generic() {
    let subs = make_subs(&[("TValue", "User")]);

    assert_eq!(
        apply_substitution("Collection<int, list<TValue>>", &subs),
        "Collection<int, list<User>>"
    );
}

#[test]
fn test_apply_substitution_array_shorthand() {
    let subs = make_subs(&[("TValue", "User")]);

    assert_eq!(apply_substitution("TValue[]", &subs), "array<User>");
}

#[test]
fn test_apply_substitution_no_match() {
    let subs = make_subs(&[("TValue", "User")]);

    assert_eq!(apply_substitution("string", &subs), "string");
    assert_eq!(apply_substitution("void", &subs), "void");
    assert_eq!(apply_substitution("$this", &subs), "$this");
}

#[test]
fn test_apply_substitution_complex_union_with_generic() {
    let subs = make_subs(&[("TKey", "int"), ("TValue", "User")]);

    assert_eq!(
        apply_substitution("array<TKey, TValue>|null", &subs),
        "array<int, User>|null"
    );
}

#[test]
fn test_apply_substitution_dnf_parens() {
    let subs = make_subs(&[("T", "User")]);

    // Standalone `(A&B)` normalizes to `A&B` — the parentheses are
    // only semantically meaningful inside a union like `(A&B)|C`.
    assert_eq!(apply_substitution("(T&Countable)", &subs), "User&Countable");
}

#[test]
fn test_apply_substitution_callable_params() {
    let subs = make_subs(&[("TValue", "User")]);

    assert_eq!(
        apply_substitution("callable(TValue): void", &subs),
        "callable(User): void"
    );
}

#[test]
fn test_apply_substitution_callable_multiple_params() {
    let subs = make_subs(&[("TKey", "int"), ("TValue", "User")]);

    assert_eq!(
        apply_substitution("callable(TKey, TValue): mixed", &subs),
        "callable(int, User): mixed"
    );
}

#[test]
fn test_apply_substitution_callable_return_type() {
    let subs = make_subs(&[("TValue", "Order")]);

    assert_eq!(
        apply_substitution("callable(string): TValue", &subs),
        "callable(string): Order"
    );
}

#[test]
fn test_apply_substitution_closure_syntax() {
    let subs = make_subs(&[("TValue", "Product")]);

    assert_eq!(
        apply_substitution("Closure(TValue): bool", &subs),
        "Closure(Product): bool"
    );
}

#[test]
fn test_apply_substitution_callable_empty_params() {
    let subs = make_subs(&[("TValue", "User")]);

    assert_eq!(
        apply_substitution("callable(): TValue", &subs),
        "callable(): User"
    );
}

#[test]
fn test_apply_substitution_callable_no_match() {
    let subs = make_subs(&[("TValue", "User")]);

    // No template params inside callable — returned unchanged.
    assert_eq!(
        apply_substitution("callable(string): void", &subs),
        "callable(string): void"
    );
}

#[test]
fn test_apply_substitution_callable_generic_param() {
    let subs = make_subs(&[("TValue", "User")]);

    assert_eq!(
        apply_substitution("callable(Collection<int, TValue>): void", &subs),
        "callable(Collection<int, User>): void"
    );
}

#[test]
fn test_apply_substitution_fqn_closure() {
    let subs = make_subs(&[("TValue", "Item")]);

    assert_eq!(
        apply_substitution("Closure(TValue): void", &subs),
        "Closure(Item): void"
    );
}

#[test]
fn test_build_substitution_map_basic() {
    let child = ClassInfo {
        name: crate::atom::atom("LanguageCollection"),
        parent_class: Some(atom("Collection")),
        is_final: true,
        extends_generics: vec![(
            atom("Collection"),
            vec![PhpType::parse("int"), PhpType::parse("Language")],
        )],
        ..ClassInfo::default()
    };

    let parent = ClassInfo {
        name: crate::atom::atom("Collection"),
        template_params: vec![atom("TKey"), atom("TValue")],
        ..ClassInfo::default()
    };

    let subs = build_substitution_map(&child, &parent, &Default::default());
    assert_eq!(subs.get("TKey").unwrap().to_string(), "int");
    assert_eq!(subs.get("TValue").unwrap().to_string(), "Language");
}

#[test]
fn test_build_substitution_map_chained() {
    // Simulates: C extends B<Foo>, B extends A<T>, A has @template U
    // When resolving A's methods for C, active_subs = {T => Foo}
    // B's @extends A<T> should resolve to A<Foo>, giving {U => Foo}

    let current_b = ClassInfo {
        name: crate::atom::atom("B"),
        parent_class: Some(atom("A")),
        template_params: vec![atom("T")],
        extends_generics: vec![(atom("A"), vec![PhpType::parse("T")])],
        ..ClassInfo::default()
    };

    let parent_a = ClassInfo {
        name: crate::atom::atom("A"),
        template_params: vec![atom("U")],
        ..ClassInfo::default()
    };

    let active = make_subs(&[("T", "Foo")]);

    let subs = build_substitution_map(&current_b, &parent_a, &active);
    assert_eq!(subs.get("U").unwrap().to_string(), "Foo");
}

#[test]
fn test_short_name() {
    use crate::util::short_name;
    assert_eq!(short_name("Collection"), "Collection");
    assert_eq!(short_name("Illuminate\\Support\\Collection"), "Collection");
    assert_eq!(short_name("\\Collection"), "Collection");
}

#[test]
fn test_apply_substitution_array_shape() {
    let subs = make_subs(&[("T", "User")]);

    assert_eq!(
        apply_substitution("array{data: T, items: list<T>}", &subs),
        "array{data: User, items: list<User>}"
    );
}

#[test]
fn test_apply_substitution_object_shape() {
    let subs = make_subs(&[("T", "User")]);

    assert_eq!(
        apply_substitution("object{name: T}", &subs),
        "object{name: User}"
    );
}

#[test]
fn test_apply_substitution_array_shape_nested() {
    let subs = make_subs(&[("T", "Foo")]);

    assert_eq!(
        apply_substitution("array{inner: array{val: T}}", &subs),
        "array{inner: array{val: Foo}}"
    );
}

#[test]
fn test_apply_substitution_shape_in_union() {
    let subs = make_subs(&[("T", "User")]);

    assert_eq!(
        apply_substitution("array{data: T}|null", &subs),
        "array{data: User}|null"
    );
}

#[test]
fn test_apply_substitution_shape_no_key() {
    let subs = make_subs(&[("T", "User")]);

    assert_eq!(
        apply_substitution("array{T, string}", &subs),
        "array{User, string}"
    );
}

#[test]
fn test_apply_substitution_to_method_modifies_return_and_params() {
    let subs = make_subs(&[("TValue", "Language"), ("TKey", "int")]);

    let mut method = MethodInfo {
        name: crate::atom::atom("first"),
        name_offset: 0,
        parameters: vec![crate::types::ParameterInfo {
            name: crate::atom::atom("$key"),
            is_required: false,
            type_hint: Some(PhpType::parse("TKey")),
            native_type_hint: Some(PhpType::parse("TKey")),
            description: None,
            default_value: None,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        }],
        return_type: Some(PhpType::parse("TValue")),
        native_return_type: None,
        description: None,
        return_description: None,
        links: Vec::new(),
        see_refs: Vec::new(),
        is_static: false,
        visibility: Visibility::Public,
        conditional_return: None,
        deprecation_message: None,
        deprecated_replacement: None,
        template_params: Vec::new(),
        template_param_bounds: Default::default(),
        template_bindings: Vec::new(),
        has_scope_attribute: false,
        is_abstract: false,
        is_virtual: false,
        is_macro: false,
        is_inferred_return: false,
        type_assertions: Vec::new(),
        throws: Vec::new(),
        if_this_is: None,
    };

    apply_substitution_to_method(&mut method, &subs);

    assert_eq!(method.return_type.as_ref().unwrap().to_string(), "Language");
    assert_eq!(
        method.parameters[0].type_hint.as_ref().unwrap().to_string(),
        "int"
    );
}

/// Verify that `@extends Parent<ConcreteType>` propagates template args
/// through the parent's `@use Trait<TValue>` generics so that trait
/// methods have their template params substituted with concrete types.
///
/// This covers the pattern:
///
/// ```php
/// /** @template TKey @template TValue */
/// /** @use EnumerableMethods<TKey, TValue> */
/// class DataCollection { use EnumerableMethods; }
///
/// /** @extends DataCollection<int, DeliveryOption> */
/// class DeliveryOptionCollection extends DataCollection {}
/// ```
///
/// `DeliveryOptionCollection->first()` should return `DeliveryOption`,
/// not the raw template param `TValue`.
#[test]
fn test_extends_generics_propagate_through_parent_use_generics() {
    use std::sync::Arc;

    // The trait: has @template TKey, TValue and a method returning TValue.
    let trait_info = ClassInfo {
        name: crate::atom::atom("EnumerableMethods"),
        kind: ClassLikeKind::Trait,
        template_params: vec![atom("TKey"), atom("TValue")],
        methods: vec![Arc::new(MethodInfo {
            name: crate::atom::atom("first"),
            name_offset: 0,
            parameters: vec![],
            return_type: Some(PhpType::parse("TValue")),
            native_return_type: None,
            description: None,
            return_description: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            is_static: false,
            visibility: Visibility::Public,
            conditional_return: None,
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: Vec::new(),
            template_param_bounds: Default::default(),
            template_bindings: Vec::new(),
            has_scope_attribute: false,
            is_abstract: false,
            is_virtual: false,
            is_macro: false,
            is_inferred_return: false,
            type_assertions: Vec::new(),
            throws: Vec::new(),
            if_this_is: None,
        })]
        .into(),
        ..ClassInfo::default()
    };

    // The parent class: has @template TKey, TValue and @use EnumerableMethods<TKey, TValue>.
    let parent_class = ClassInfo {
        name: crate::atom::atom("DataCollection"),
        template_params: vec![atom("TKey"), atom("TValue")],
        used_traits: vec![atom("EnumerableMethods")],
        use_generics: vec![(
            atom("EnumerableMethods"),
            vec![PhpType::parse("TKey"), PhpType::parse("TValue")],
        )],
        ..ClassInfo::default()
    };

    // The child class: @extends DataCollection<int, DeliveryOption>
    let child_class = ClassInfo {
        name: crate::atom::atom("DeliveryOptionCollection"),
        parent_class: Some(atom("DataCollection")),
        extends_generics: vec![(
            atom("DataCollection"),
            vec![PhpType::parse("int"), PhpType::parse("DeliveryOption")],
        )],
        is_final: true,
        ..ClassInfo::default()
    };

    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "DataCollection" => Some(Arc::new(parent_class.clone())),
            "EnumerableMethods" => Some(Arc::new(trait_info.clone())),
            _ => None,
        }
    };

    let resolved = resolve_class_with_inheritance(&child_class, &class_loader);

    let first_method = resolved
        .methods
        .iter()
        .find(|m| m.name == "first")
        .expect("first() method should be inherited from trait via parent");

    assert_eq!(
        first_method.return_type.as_ref().unwrap().to_string(),
        "DeliveryOption",
        "first() return type should be substituted from TValue to DeliveryOption"
    );
}

#[test]
fn test_apply_generic_args_right_aligns_single_arg_for_collection() {
    // When `Collection<SectionTranslation>` is written (1 arg) but
    // Collection has `@template TKey of array-key` and `@template TValue`,
    // the single arg should bind to TValue, not TKey.
    let collection = ClassInfo {
        name: crate::atom::atom("Collection"),
        template_params: vec![atom("TKey"), atom("TValue")],
        template_param_bounds: [(atom("TKey"), PhpType::parse("array-key"))]
            .into_iter()
            .collect::<AtomMap<_>>(),
        methods: vec![Arc::new(MethodInfo::virtual_method(
            "first",
            Some("TValue"),
        ))]
        .into(),
        ..ClassInfo::default()
    };

    let result = apply_generic_args(&collection, &[PhpType::parse("SectionTranslation")]);

    let first = result
        .methods
        .iter()
        .find(|m| m.name == "first")
        .expect("first() should exist");
    assert_eq!(
        first.return_type.as_ref().unwrap().to_string(),
        "SectionTranslation",
        "Single generic arg should bind to TValue (not TKey) when TKey has array-key bound"
    );
}

#[test]
fn test_apply_generic_args_no_right_align_when_all_args_provided() {
    // When both args are provided, positional mapping is used as-is.
    let collection = ClassInfo {
        name: crate::atom::atom("Collection"),
        template_params: vec![atom("TKey"), atom("TValue")],
        template_param_bounds: [(atom("TKey"), PhpType::parse("array-key"))]
            .into_iter()
            .collect::<AtomMap<_>>(),
        methods: vec![Arc::new(MethodInfo::virtual_method(
            "first",
            Some("TValue"),
        ))]
        .into(),
        ..ClassInfo::default()
    };

    let result = apply_generic_args(
        &collection,
        &[PhpType::parse("int"), PhpType::parse("User")],
    );

    let first = result
        .methods
        .iter()
        .find(|m| m.name == "first")
        .expect("first() should exist");
    assert_eq!(first.return_type.as_ref().unwrap().to_string(), "User",);
}

#[test]
fn test_apply_generic_args_no_right_align_without_key_bound() {
    // When the leading param has no key-like bound, positional mapping
    // is used even with fewer args.
    let cls = ClassInfo {
        name: crate::atom::atom("Pair"),
        template_params: vec![atom("TFirst"), atom("TSecond")],
        methods: vec![Arc::new(MethodInfo::virtual_method(
            "first",
            Some("TFirst"),
        ))]
        .into(),
        ..ClassInfo::default()
    };

    let result = apply_generic_args(&cls, &[PhpType::parse("Foo")]);

    let first = result
        .methods
        .iter()
        .find(|m| m.name == "first")
        .expect("first() should exist");
    assert_eq!(
        first.return_type.as_ref().unwrap().to_string(),
        "Foo",
        "Without key-like bound on leading param, single arg should bind positionally to TFirst"
    );
}

#[test]
fn test_apply_generic_args_right_align_with_int_bound() {
    // `int` is also a key-like bound.
    let cls = ClassInfo {
        name: crate::atom::atom("TypedList"),
        template_params: vec![atom("TKey"), atom("TValue")],
        template_param_bounds: [(atom("TKey"), PhpType::parse("int"))]
            .into_iter()
            .collect::<AtomMap<_>>(),
        methods: vec![Arc::new(MethodInfo::virtual_method("get", Some("TValue")))].into(),
        ..ClassInfo::default()
    };

    let result = apply_generic_args(&cls, &[PhpType::parse("Product")]);

    let get = result
        .methods
        .iter()
        .find(|m| m.name == "get")
        .expect("get() should exist");
    assert_eq!(get.return_type.as_ref().unwrap().to_string(), "Product",);
}
