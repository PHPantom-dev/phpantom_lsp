use super::*;
use crate::atom::atom;

/// Helper: create a minimal MethodInfo for testing signature_eq.
fn method(name: &str) -> MethodInfo {
    MethodInfo::virtual_method(name, Some("void"))
}

/// Helper: create a minimal PropertyInfo for testing signature_eq.
fn prop(name: &str, type_hint: &str) -> PropertyInfo {
    PropertyInfo::virtual_property(name, Some(type_hint))
}

/// Helper: create a minimal ConstantInfo for testing signature_eq.
fn constant(name: &str) -> ConstantInfo {
    ConstantInfo {
        name: crate::atom::atom(name),
        name_offset: 0,
        type_hint: Some(PhpType::parse("string")),
        visibility: Visibility::Public,
        deprecation_message: None,
        deprecated_replacement: None,
        see_refs: Vec::new(),
        description: None,
        is_enum_case: false,
        enum_value: None,
        value: Some("'hello'".to_string()),
        is_virtual: false,
    }
}

/// Helper: create a minimal ParameterInfo for testing signature_eq.
fn param(name: &str, type_hint: &str) -> ParameterInfo {
    ParameterInfo {
        name: crate::atom::atom(name),
        is_required: true,
        type_hint: Some(PhpType::parse(type_hint)),
        native_type_hint: None,
        description: None,
        default_value: None,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    }
}

// ── ParameterInfo::signature_eq ─────────────────────────────────

#[test]
fn param_signature_eq_identical() {
    let a = param("$x", "int");
    let b = param("$x", "int");
    assert!(a.signature_eq(&b));
}

#[test]
fn param_signature_eq_different_name() {
    let a = param("$x", "int");
    let b = param("$y", "int");
    assert!(!a.signature_eq(&b));
}

#[test]
fn param_signature_eq_different_type() {
    let a = param("$x", "int");
    let b = param("$x", "string");
    assert!(!a.signature_eq(&b));
}

#[test]
fn param_signature_eq_different_variadic() {
    let a = param("$x", "int");
    let mut b = param("$x", "int");
    b.is_variadic = true;
    assert!(!a.signature_eq(&b));
}

#[test]
fn param_signature_eq_different_reference() {
    let a = param("$x", "int");
    let mut b = param("$x", "int");
    b.is_reference = true;
    assert!(!a.signature_eq(&b));
}

#[test]
fn param_signature_eq_different_default() {
    let a = param("$x", "int");
    let mut b = param("$x", "int");
    b.default_value = Some("42".to_string());
    b.is_required = false;
    assert!(!a.signature_eq(&b));
}

#[test]
fn param_signature_eq_ignores_description() {
    let mut a = param("$x", "int");
    let mut b = param("$x", "int");
    a.description = Some("First param".to_string());
    b.description = Some("Different description".to_string());
    assert!(a.signature_eq(&b));
}

// ── MethodInfo::signature_eq ────────────────────────────────────

#[test]
fn method_signature_eq_identical() {
    let a = method("foo");
    let b = method("foo");
    assert!(a.signature_eq(&b));
}

#[test]
fn method_signature_eq_different_name() {
    let a = method("foo");
    let b = method("bar");
    assert!(!a.signature_eq(&b));
}

#[test]
fn method_signature_eq_different_return_type() {
    let a = MethodInfo::virtual_method("foo", Some("int"));
    let b = MethodInfo::virtual_method("foo", Some("string"));
    assert!(!a.signature_eq(&b));
}

#[test]
fn method_signature_eq_different_visibility() {
    let a = method("foo");
    let mut b = method("foo");
    b.visibility = Visibility::Protected;
    assert!(!a.signature_eq(&b));
}

#[test]
fn method_signature_eq_different_static() {
    let a = method("foo");
    let mut b = method("foo");
    b.is_static = true;
    assert!(!a.signature_eq(&b));
}

#[test]
fn method_signature_eq_different_deprecation() {
    let a = method("foo");
    let mut b = method("foo");
    b.deprecation_message = Some("Use bar() instead".to_string());
    assert!(!a.signature_eq(&b));
}

#[test]
fn method_signature_eq_different_params() {
    let mut a = method("foo");
    a.parameters = vec![param("$x", "int")];
    let mut b = method("foo");
    b.parameters = vec![param("$x", "string")];
    assert!(!a.signature_eq(&b));
}

#[test]
fn method_signature_eq_different_param_count() {
    let mut a = method("foo");
    a.parameters = vec![param("$x", "int")];
    let mut b = method("foo");
    b.parameters = vec![param("$x", "int"), param("$y", "string")];
    assert!(!a.signature_eq(&b));
}

#[test]
fn method_signature_eq_ignores_name_offset() {
    let mut a = method("foo");
    a.name_offset = 100;
    let mut b = method("foo");
    b.name_offset = 200;
    assert!(a.signature_eq(&b));
}

#[test]
fn method_signature_eq_detects_description_change() {
    let mut a = method("foo");
    a.description = Some("Does stuff".to_string());
    let mut b = method("foo");
    b.description = Some("Different description".to_string());
    assert!(
        !a.signature_eq(&b),
        "Description changes must break signature_eq"
    );
}

#[test]
fn method_signature_eq_detects_return_description_change() {
    let mut a = method("foo");
    a.return_description = Some("The result".to_string());
    let mut b = method("foo");
    b.return_description = None;
    assert!(
        !a.signature_eq(&b),
        "Return description changes must break signature_eq"
    );
}

#[test]
fn method_signature_eq_detects_link_change() {
    let mut a = method("foo");
    a.links = vec!["https://example.com".to_string()];
    let b = method("foo");
    assert!(!a.signature_eq(&b), "Link changes must break signature_eq");
}

#[test]
fn method_signature_eq_detects_template_change() {
    let mut a = method("foo");
    a.template_params = vec![atom("T")];
    let b = method("foo");
    assert!(!a.signature_eq(&b));
}

#[test]
fn method_signature_eq_detects_conditional_return() {
    let mut a = method("foo");
    a.conditional_return = Some(PhpType::int());
    let b = method("foo");
    assert!(!a.signature_eq(&b));
}

#[test]
fn method_signature_eq_detects_scope_attribute() {
    let mut a = method("foo");
    a.has_scope_attribute = true;
    let b = method("foo");
    assert!(!a.signature_eq(&b));
}

#[test]
fn method_signature_eq_detects_abstract_change() {
    let mut a = method("foo");
    a.is_abstract = true;
    let b = method("foo");
    assert!(!a.signature_eq(&b));
}

// ── PropertyInfo::signature_eq ──────────────────────────────────

#[test]
fn prop_signature_eq_identical() {
    let a = prop("name", "string");
    let b = prop("name", "string");
    assert!(a.signature_eq(&b));
}

#[test]
fn prop_signature_eq_different_name() {
    let a = prop("name", "string");
    let b = prop("email", "string");
    assert!(!a.signature_eq(&b));
}

#[test]
fn prop_signature_eq_different_type() {
    let a = prop("name", "string");
    let b = prop("name", "int");
    assert!(!a.signature_eq(&b));
}

#[test]
fn prop_signature_eq_different_visibility() {
    let a = prop("name", "string");
    let mut b = prop("name", "string");
    b.visibility = Visibility::Private;
    assert!(!a.signature_eq(&b));
}

#[test]
fn prop_signature_eq_different_static() {
    let a = prop("name", "string");
    let mut b = prop("name", "string");
    b.is_static = true;
    assert!(!a.signature_eq(&b));
}

#[test]
fn prop_signature_eq_ignores_name_offset() {
    let mut a = prop("name", "string");
    a.name_offset = 10;
    let mut b = prop("name", "string");
    b.name_offset = 200;
    assert!(a.signature_eq(&b));
}

#[test]
fn prop_signature_eq_detects_description_change() {
    let mut a = prop("name", "string");
    a.description = Some("The user's name".to_string());
    let b = prop("name", "string");
    assert!(
        !a.signature_eq(&b),
        "Property description changes must break signature_eq"
    );
}

#[test]
fn prop_signature_eq_detects_deprecation() {
    let mut a = prop("name", "string");
    a.deprecation_message = Some("Use fullName".to_string());
    let b = prop("name", "string");
    assert!(!a.signature_eq(&b));
}

// ── ConstantInfo::signature_eq ──────────────────────────────────

#[test]
fn constant_signature_eq_identical() {
    let a = constant("MAX");
    let b = constant("MAX");
    assert!(a.signature_eq(&b));
}

#[test]
fn constant_signature_eq_different_name() {
    let a = constant("MAX");
    let b = constant("MIN");
    assert!(!a.signature_eq(&b));
}

#[test]
fn constant_signature_eq_different_value() {
    let a = constant("MAX");
    let mut b = constant("MAX");
    b.value = Some("'world'".to_string());
    assert!(!a.signature_eq(&b));
}

#[test]
fn constant_signature_eq_different_visibility() {
    let a = constant("MAX");
    let mut b = constant("MAX");
    b.visibility = Visibility::Protected;
    assert!(!a.signature_eq(&b));
}

#[test]
fn constant_signature_eq_ignores_name_offset() {
    let mut a = constant("MAX");
    a.name_offset = 50;
    let mut b = constant("MAX");
    b.name_offset = 300;
    assert!(a.signature_eq(&b));
}

#[test]
fn constant_signature_eq_ignores_description() {
    let mut a = constant("MAX");
    a.description = Some("Maximum value".to_string());
    let b = constant("MAX");
    assert!(a.signature_eq(&b));
}

#[test]
fn constant_signature_eq_detects_enum_case() {
    let a = constant("Active");
    let mut b = constant("Active");
    b.is_enum_case = true;
    assert!(!a.signature_eq(&b));
}

// ── ClassInfo::signature_eq ─────────────────────────────────────

#[test]
fn class_signature_eq_identical_empty() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        ..Default::default()
    };
    assert!(a.signature_eq(&b));
}

#[test]
fn class_signature_eq_different_name() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Bar"),
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_different_kind() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        kind: ClassLikeKind::Class,
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        kind: ClassLikeKind::Interface,
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_different_parent() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        parent_class: Some(crate::atom::atom("Base")),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        parent_class: Some(crate::atom::atom("OtherBase")),
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_different_interfaces() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        interfaces: vec![crate::atom::atom("Countable")],
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        interfaces: vec![],
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_ignores_offsets() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        start_offset: 100,
        end_offset: 500,
        keyword_offset: 95,
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        start_offset: 200,
        end_offset: 600,
        keyword_offset: 195,
        ..Default::default()
    };
    assert!(a.signature_eq(&b));
}

#[test]
fn class_signature_eq_ignores_link() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        links: vec!["https://example.com".to_string()],
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        links: vec![],
        ..Default::default()
    };
    assert!(a.signature_eq(&b));
}

#[test]
fn class_signature_eq_methods_order_insensitive() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        methods: vec![Arc::new(method("alpha")), Arc::new(method("beta"))].into(),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        methods: vec![Arc::new(method("beta")), Arc::new(method("alpha"))].into(),
        ..Default::default()
    };
    assert!(a.signature_eq(&b));
}

#[test]
fn class_signature_eq_methods_different_count() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        methods: vec![Arc::new(method("alpha"))].into(),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        methods: vec![Arc::new(method("alpha")), Arc::new(method("beta"))].into(),
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_methods_different_signature() {
    let mut m = method("foo");
    m.return_type = Some(PhpType::parse("int"));
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        methods: vec![Arc::new(m)].into(),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        methods: vec![Arc::new(method("foo"))].into(),
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_properties_order_insensitive() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        properties: vec![prop("x", "int"), prop("y", "string")].into(),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        properties: vec![prop("y", "string"), prop("x", "int")].into(),
        ..Default::default()
    };
    assert!(a.signature_eq(&b));
}

#[test]
fn class_signature_eq_constants_order_insensitive() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        constants: vec![constant("A"), constant("B")].into(),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        constants: vec![constant("B"), constant("A")].into(),
        ..Default::default()
    };
    assert!(a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_docblock_change() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        class_docblock: Some("/** @method void bar() */".to_string()),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        class_docblock: None,
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_template_change() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        template_params: vec![crate::atom::atom("T")],
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        template_params: vec![],
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_extends_generics_change() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        extends_generics: vec![(
            crate::atom::atom("Base"),
            vec![crate::php_type::PhpType::parse("int")],
        )],
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        extends_generics: vec![(
            crate::atom::atom("Base"),
            vec![crate::php_type::PhpType::parse("string")],
        )],
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_trait_change() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        used_traits: vec![crate::atom::atom("SomeTrait")],
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        used_traits: vec![],
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_final_change() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        is_final: true,
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        is_final: false,
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_abstract_change() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        is_abstract: true,
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        is_abstract: false,
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_deprecation_change() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        deprecation_message: Some("Use Bar".to_string()),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        deprecation_message: None,
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_backed_type_change() {
    let a = ClassInfo {
        name: crate::atom::atom("Status"),
        kind: ClassLikeKind::Enum,
        backed_type: Some(BackedEnumType::String),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Status"),
        kind: ClassLikeKind::Enum,
        backed_type: Some(BackedEnumType::Int),
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_laravel_metadata_change() {
    let mut a = ClassInfo {
        name: crate::atom::atom("User"),
        ..Default::default()
    };
    a.laravel_mut().custom_collection = Some(PhpType::Named("UserCollection".to_string()));

    let b = ClassInfo {
        name: crate::atom::atom("User"),
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_mixin_change() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        mixins: vec![crate::atom::atom("SomeClass")],
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        mixins: vec![],
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

#[test]
fn class_signature_eq_detects_namespace_change() {
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        file_namespace: Some(crate::atom::atom("App\\Models")),
        ..Default::default()
    };
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        file_namespace: Some(crate::atom::atom("App\\Services")),
        ..Default::default()
    };
    assert!(!a.signature_eq(&b));
}

/// Body-only changes (offsets shift, descriptions change) must not
/// Changing only byte offsets must NOT trigger eviction.
/// Descriptions and links DO trigger eviction (they affect hover).
#[test]
fn class_signature_eq_body_only_change() {
    let mut m_a = method("doWork");
    m_a.name_offset = 100;
    m_a.description = Some("Same description".to_string());
    m_a.return_description = Some("Same return desc".to_string());
    m_a.links = vec!["https://same.example.com".to_string()];
    let mut p_a = prop("name", "string");
    p_a.name_offset = 200;
    p_a.description = Some("Same prop desc".to_string());
    let mut c_a = constant("MAX");
    c_a.name_offset = 300;
    c_a.description = Some("Same const desc".to_string());

    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        start_offset: 10,
        end_offset: 500,
        keyword_offset: 5,
        methods: vec![Arc::new(m_a)].into(),
        properties: vec![p_a].into(),
        constants: vec![c_a].into(),
        links: vec!["https://same.example.com".to_string()],
        ..Default::default()
    };

    let mut m_b = method("doWork");
    m_b.name_offset = 150; // offset changed
    m_b.description = Some("Same description".to_string());
    m_b.return_description = Some("Same return desc".to_string());
    m_b.links = vec!["https://same.example.com".to_string()];
    let mut p_b = prop("name", "string");
    p_b.name_offset = 250; // offset changed
    p_b.description = Some("Same prop desc".to_string());
    let mut c_b = constant("MAX");
    c_b.name_offset = 350; // offset changed
    c_b.description = Some("Same const desc".to_string());

    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        start_offset: 15,
        end_offset: 510,
        keyword_offset: 10,
        methods: vec![Arc::new(m_b)].into(),
        properties: vec![p_b].into(),
        constants: vec![c_b].into(),
        links: vec!["https://same.example.com".to_string()],
        ..Default::default()
    };

    assert!(
        a.signature_eq(&b),
        "Offset-only changes must not break signature_eq"
    );
}

/// Changing descriptions or links MUST trigger eviction so that
/// hover shows updated content after cross-file edits.
#[test]
fn class_signature_eq_description_change_triggers_eviction() {
    let mut m_a = method("doWork");
    m_a.description = Some("Old description".to_string());
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        methods: vec![Arc::new(m_a)].into(),
        ..Default::default()
    };

    let mut m_b = method("doWork");
    m_b.description = Some("New description".to_string());
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        methods: vec![Arc::new(m_b)].into(),
        ..Default::default()
    };

    assert!(
        !a.signature_eq(&b),
        "Description changes must break signature_eq to invalidate hover cache"
    );
}

/// Changing a property description MUST trigger eviction.
#[test]
fn class_signature_eq_property_description_change_triggers_eviction() {
    let mut p_a = prop("name", "string");
    p_a.description = Some("Old prop desc".to_string());
    let a = ClassInfo {
        name: crate::atom::atom("Foo"),
        properties: vec![p_a].into(),
        ..Default::default()
    };

    let mut p_b = prop("name", "string");
    p_b.description = Some("New prop desc".to_string());
    let b = ClassInfo {
        name: crate::atom::atom("Foo"),
        properties: vec![p_b].into(),
        ..Default::default()
    };

    assert!(
        !a.signature_eq(&b),
        "Property description changes must break signature_eq"
    );
}

// ── ResolvedType helpers ────────────────────────────────────────

/// Helper: create a minimal ClassInfo with only a name.
fn class(name: &str) -> ClassInfo {
    ClassInfo {
        name: crate::atom::atom(name),
        ..Default::default()
    }
}

/// Helper: create a ClassInfo with a namespace.
fn class_with_ns(name: &str, ns: &str) -> ClassInfo {
    ClassInfo {
        name: crate::atom::atom(name),
        file_namespace: Some(crate::atom::atom(ns)),
        ..Default::default()
    }
}

// ── from_classes_with_hint: intersection ────────────────────────

#[test]
fn from_classes_with_hint_single_class_uses_hint() {
    let hint = PhpType::Named("Foo".to_owned());
    let result = ResolvedType::from_classes_with_hint(vec![Arc::new(class("Foo"))], hint.clone());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].type_string, hint);
    assert!(result[0].class_info.is_some());
}

#[test]
fn from_classes_with_hint_intersection_preserves_type() {
    let hint = PhpType::Intersection(vec![
        PhpType::Named("Countable".to_owned()),
        PhpType::Named("Serializable".to_owned()),
    ]);
    let classes = vec![
        Arc::new(class("Countable")),
        Arc::new(class("Serializable")),
    ];
    let result = ResolvedType::from_classes_with_hint(classes, hint.clone());
    assert_eq!(result.len(), 2);
    // Both entries carry the full intersection type.
    for rt in &result {
        assert_eq!(rt.type_string, hint);
        assert!(rt.class_info.is_some());
    }
}

#[test]
fn from_classes_with_hint_union_uses_class_names() {
    let hint = PhpType::Union(vec![
        PhpType::Named("Foo".to_owned()),
        PhpType::Named("Bar".to_owned()),
    ]);
    let classes = vec![Arc::new(class("Foo")), Arc::new(class("Bar"))];
    let result = ResolvedType::from_classes_with_hint(classes, hint);
    assert_eq!(result.len(), 2);
    // Union: each entry uses the class's own name (old behaviour).
    assert_eq!(result[0].type_string, PhpType::Named("Foo".to_owned()));
    assert_eq!(result[1].type_string, PhpType::Named("Bar".to_owned()));
}

// ── types_joined: intersection ──────────────────────────────────

#[test]
fn types_joined_single_entry() {
    let entries = vec![ResolvedType::from_type_string(PhpType::Named(
        "Foo".to_owned(),
    ))];
    assert_eq!(
        ResolvedType::types_joined(&entries),
        PhpType::Named("Foo".to_owned())
    );
}

#[test]
fn types_joined_intersection_entries_return_intersection() {
    let intersection = PhpType::Intersection(vec![
        PhpType::Named("Countable".to_owned()),
        PhpType::Named("Serializable".to_owned()),
    ]);
    let entries = vec![
        ResolvedType::from_both(intersection.clone(), class("Countable")),
        ResolvedType::from_both(intersection.clone(), class("Serializable")),
    ];
    let joined = ResolvedType::types_joined(&entries);
    assert_eq!(joined, intersection);
}

#[test]
fn types_joined_mixed_entries_return_union() {
    let entries = vec![
        ResolvedType::from_type_string(PhpType::Named("Foo".to_owned())),
        ResolvedType::from_type_string(PhpType::Named("Bar".to_owned())),
    ];
    let joined = ResolvedType::types_joined(&entries);
    assert_eq!(
        joined,
        PhpType::Union(vec![
            PhpType::Named("Foo".to_owned()),
            PhpType::Named("Bar".to_owned()),
        ])
    );
}

#[test]
fn types_joined_empty_returns_mixed() {
    let entries: Vec<ResolvedType> = vec![];
    assert_eq!(ResolvedType::types_joined(&entries), PhpType::mixed());
}

// ── strip_null ──────────────────────────────────────────────────

#[test]
fn strip_null_removes_nullable() {
    let mut rt = ResolvedType::from_both(
        PhpType::Nullable(Box::new(PhpType::Named("Foo".to_owned()))),
        class("Foo"),
    );
    rt.strip_null();
    assert_eq!(rt.type_string, PhpType::Named("Foo".to_owned()));
    assert!(rt.class_info.is_some());
}

#[test]
fn strip_null_no_op_when_not_nullable() {
    let mut rt = ResolvedType::from_both(PhpType::Named("Foo".to_owned()), class("Foo"));
    rt.strip_null();
    assert_eq!(rt.type_string, PhpType::Named("Foo".to_owned()));
    assert!(rt.class_info.is_some());
}

// ── replace_type ────────────────────────────────────────────────

#[test]
fn replace_type_keeps_class_info_when_matching() {
    let mut rt = ResolvedType::from_both(PhpType::Named("Foo".to_owned()), class("Foo"));
    rt.replace_type(PhpType::Named("Foo".to_owned()));
    assert_eq!(rt.type_string, PhpType::Named("Foo".to_owned()));
    assert!(rt.class_info.is_some());
}

#[test]
fn replace_type_clears_class_info_when_mismatched() {
    let mut rt = ResolvedType::from_both(PhpType::Named("Foo".to_owned()), class("Foo"));
    rt.replace_type(PhpType::Named("array".to_owned()));
    assert_eq!(rt.type_string, PhpType::Named("array".to_owned()));
    assert!(rt.class_info.is_none());
}

#[test]
fn replace_type_matches_fqn_with_leading_backslash() {
    let mut rt = ResolvedType::from_both(
        PhpType::Named("App\\Models\\User".to_owned()),
        class_with_ns("User", "App\\Models"),
    );
    rt.replace_type(PhpType::Named("\\App\\Models\\User".to_owned()));
    assert_eq!(
        rt.type_string,
        PhpType::Named("\\App\\Models\\User".to_owned())
    );
    assert!(
        rt.class_info.is_some(),
        "class_info should be preserved when FQN matches modulo leading backslash"
    );
}

#[test]
fn replace_type_matches_short_name() {
    let mut rt = ResolvedType::from_both(PhpType::Named("User".to_owned()), class("User"));
    rt.replace_type(PhpType::Named("User".to_owned()));
    assert!(rt.class_info.is_some());
}

#[test]
fn replace_type_clears_when_no_class_info() {
    let mut rt = ResolvedType::from_type_string(PhpType::Named("int".to_owned()));
    rt.replace_type(PhpType::Named("string".to_owned()));
    assert_eq!(rt.type_string, PhpType::Named("string".to_owned()));
    assert!(rt.class_info.is_none());
}

// ── FunctionInfo::signature_eq ──────────────────────────────────

/// Helper: create a minimal FunctionInfo for testing signature_eq.
fn func(name: &str) -> FunctionInfo {
    FunctionInfo {
        name: atom(name),
        name_offset: 0,
        parameters: Vec::new(),
        return_type: None,
        native_return_type: None,
        description: None,
        return_description: None,
        links: Vec::new(),
        see_refs: Vec::new(),
        namespace: None,
        conditional_return: None,
        type_assertions: Vec::new(),
        deprecation_message: None,
        deprecated_replacement: None,
        template_params: Vec::new(),
        template_bindings: Vec::new(),
        template_param_bounds: Default::default(),
        throws: Vec::new(),
        is_polyfill: false,
        overloads: Vec::new(),
    }
}

#[test]
fn func_signature_eq_identical() {
    let a = func("bar");
    let b = func("bar");
    assert!(a.signature_eq(&b));
}

#[test]
fn func_signature_eq_different_name() {
    let a = func("bar");
    let b = func("baz");
    assert!(!a.signature_eq(&b));
}

#[test]
fn func_signature_eq_different_return_type() {
    let mut a = func("bar");
    a.return_type = Some(PhpType::parse("int"));
    let mut b = func("bar");
    b.return_type = Some(PhpType::parse("string"));
    assert!(!a.signature_eq(&b));
}

#[test]
fn func_signature_eq_different_native_return_type() {
    let mut a = func("bar");
    a.native_return_type = Some(PhpType::parse("int"));
    let b = func("bar");
    assert!(!a.signature_eq(&b));
}

#[test]
fn func_signature_eq_different_param_type() {
    let mut a = func("bar");
    a.parameters = vec![param("$x", "null")];
    let mut b = func("bar");
    b.parameters = vec![param("$x", "string")];
    assert!(!a.signature_eq(&b));
}

#[test]
fn func_signature_eq_different_param_count() {
    let mut a = func("bar");
    a.parameters = vec![param("$x", "int")];
    let mut b = func("bar");
    b.parameters = vec![param("$x", "int"), param("$y", "string")];
    assert!(!a.signature_eq(&b));
}

#[test]
fn func_signature_eq_different_deprecation() {
    let mut a = func("bar");
    a.deprecation_message = Some("Use baz() instead".to_string());
    let b = func("bar");
    assert!(!a.signature_eq(&b));
}

#[test]
fn func_signature_eq_different_template_params() {
    let mut a = func("bar");
    a.template_params = vec![atom("T")];
    let b = func("bar");
    assert!(!a.signature_eq(&b));
}

#[test]
fn func_signature_eq_different_throws() {
    let mut a = func("bar");
    a.throws = vec![PhpType::parse("RuntimeException")];
    let b = func("bar");
    assert!(!a.signature_eq(&b));
}

#[test]
fn func_signature_eq_different_namespace() {
    let mut a = func("bar");
    a.namespace = Some("App".to_string());
    let b = func("bar");
    assert!(!a.signature_eq(&b));
}

#[test]
fn func_signature_eq_different_conditional_return() {
    let mut a = func("bar");
    a.conditional_return = Some(PhpType::int());
    let b = func("bar");
    assert!(!a.signature_eq(&b));
}

#[test]
fn func_signature_eq_ignores_name_offset() {
    let mut a = func("bar");
    a.name_offset = 100;
    let mut b = func("bar");
    b.name_offset = 200;
    assert!(a.signature_eq(&b));
}

#[test]
fn func_signature_eq_ignores_description() {
    let mut a = func("bar");
    a.description = Some("Does things".to_string());
    let mut b = func("bar");
    b.description = Some("Different description".to_string());
    assert!(a.signature_eq(&b));
}

#[test]
fn func_signature_eq_ignores_links() {
    let mut a = func("bar");
    a.links = vec!["https://example.com".to_string()];
    let b = func("bar");
    assert!(a.signature_eq(&b));
}

#[test]
fn func_signature_eq_ignores_is_polyfill() {
    let mut a = func("bar");
    a.is_polyfill = true;
    let b = func("bar");
    assert!(a.signature_eq(&b));
}
