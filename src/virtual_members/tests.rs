use super::*;
use crate::atom::atom;
use crate::php_type::PhpType;
use crate::test_fixtures::{make_class, make_method, make_property};
use crate::types::{ClassLikeKind, ConstantInfo, Visibility};
use std::sync::Arc;

// ── VirtualMembers tests ────────────────────────────────────────────

#[test]
fn virtual_members_is_empty() {
    let vm = VirtualMembers {
        methods: Vec::new(),
        properties: Vec::new(),
        constants: Vec::new(),
    };
    assert!(vm.is_empty());
}

#[test]
fn virtual_members_not_empty_with_method() {
    let vm = VirtualMembers {
        methods: vec![Arc::new(make_method("foo", Some("string")))],
        properties: Vec::new(),
        constants: Vec::new(),
    };
    assert!(!vm.is_empty());
}

#[test]
fn virtual_members_not_empty_with_property() {
    let vm = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("bar", Some("int"))],
        constants: Vec::new(),
    };
    assert!(!vm.is_empty());
}

#[test]
fn virtual_members_not_empty_with_constant() {
    let vm = VirtualMembers {
        methods: Vec::new(),
        properties: Vec::new(),
        constants: vec![ConstantInfo {
            name: crate::atom::atom("FOO"),
            name_offset: 0,
            type_hint: None,
            visibility: Visibility::Public,
            deprecation_message: None,
            deprecated_replacement: None,
            see_refs: Vec::new(),
            description: None,
            is_enum_case: false,
            enum_value: None,
            value: None,
            is_virtual: true,
        }],
    };
    assert!(!vm.is_empty());
}

// ── merge_virtual_members tests ─────────────────────────────────────

#[test]
fn merge_adds_new_methods() {
    let mut class = make_class("Foo");
    class
        .methods
        .push(Arc::new(make_method("existing", Some("string"))));

    let virtual_members = VirtualMembers {
        methods: vec![Arc::new(make_method("new_method", Some("int")))],
        properties: Vec::new(),
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.methods.len(), 2);
    assert!(class.methods.iter().any(|m| m.name == "existing"));
    assert!(class.methods.iter().any(|m| m.name == "new_method"));
}

#[test]
fn merge_adds_new_properties() {
    let mut class = make_class("Foo");
    class
        .properties
        .push(Arc::new(make_property("existing", Some("string"))));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("new_prop", Some("int"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 2);
    assert!(class.properties.iter().any(|p| p.name == "existing"));
    assert!(class.properties.iter().any(|p| p.name == "new_prop"));
}

#[test]
fn merge_does_not_overwrite_existing_method() {
    let mut class = make_class("Foo");
    class
        .methods
        .push(Arc::new(make_method("doStuff", Some("string"))));

    let virtual_members = VirtualMembers {
        methods: vec![Arc::new(make_method("doStuff", Some("int")))],
        properties: Vec::new(),
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.methods.len(), 1);
    assert_eq!(
        class.methods[0].return_type,
        Some(PhpType::parse("string")),
        "existing method should not be overwritten"
    );
}

#[test]
fn merge_allows_same_name_methods_with_different_staticness() {
    let mut class = make_class("Foo");
    // Existing instance method
    class
        .methods
        .push(Arc::new(make_method("active", Some("string"))));

    // Virtual: one instance (should be blocked) and one static (should be added)
    let mut static_method = make_method("active", Some("Builder"));
    static_method.is_static = true;

    let virtual_members = VirtualMembers {
        methods: vec![
            Arc::new(make_method("active", Some("int"))),
            Arc::new(static_method),
        ],
        properties: Vec::new(),
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.methods.len(), 2, "instance + static should coexist");
    let instance = class
        .methods
        .iter()
        .find(|m| m.name == "active" && !m.is_static)
        .unwrap();
    assert_eq!(
        instance.return_type,
        Some(PhpType::parse("string")),
        "existing instance method should not be overwritten"
    );
    let static_m = class
        .methods
        .iter()
        .find(|m| m.name == "active" && m.is_static)
        .unwrap();
    assert_eq!(
        static_m.return_type,
        Some(PhpType::parse("Builder")),
        "static variant should be added alongside instance"
    );
}

#[test]
fn merge_replaces_scope_attribute_method_with_virtual() {
    let mut class = make_class("Foo");
    let mut original = make_method("active", Some("void"));
    original.has_scope_attribute = true;
    original.visibility = Visibility::Protected;
    class.methods.push(Arc::new(original));

    let mut virtual_scope = make_method("active", Some("Builder<static>"));
    virtual_scope.visibility = Visibility::Public;

    let virtual_members = VirtualMembers {
        methods: vec![Arc::new(virtual_scope)],
        properties: Vec::new(),
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.methods.len(), 1);
    assert_eq!(
        class.methods[0].return_type,
        Some(PhpType::parse("Builder<static>")),
        "#[Scope] original should be replaced by virtual scope method"
    );
    assert_eq!(
        class.methods[0].visibility,
        Visibility::Public,
        "replacement should be public"
    );
}

#[test]
fn merge_does_not_replace_non_scope_attribute_method() {
    let mut class = make_class("Foo");
    let mut original = make_method("active", Some("string"));
    original.has_scope_attribute = false;
    class.methods.push(Arc::new(original));

    let virtual_members = VirtualMembers {
        methods: vec![Arc::new(make_method("active", Some("int")))],
        properties: Vec::new(),
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.methods.len(), 1);
    assert_eq!(
        class.methods[0].return_type,
        Some(PhpType::parse("string")),
        "non-#[Scope] method should not be replaced"
    );
}

#[test]
fn merge_replaces_scope_attribute_and_adds_static_variant() {
    let mut class = make_class("Foo");
    let mut original = make_method("active", Some("void"));
    original.has_scope_attribute = true;
    original.visibility = Visibility::Protected;
    class.methods.push(Arc::new(original));

    let mut virtual_instance = make_method("active", Some("Builder<static>"));
    virtual_instance.visibility = Visibility::Public;

    let mut virtual_static = make_method("active", Some("Builder<static>"));
    virtual_static.is_static = true;
    virtual_static.visibility = Visibility::Public;

    let virtual_members = VirtualMembers {
        methods: vec![Arc::new(virtual_instance), Arc::new(virtual_static)],
        properties: Vec::new(),
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(
        class.methods.len(),
        2,
        "replaced instance + new static should coexist"
    );
    let instance = class
        .methods
        .iter()
        .find(|m| m.name == "active" && !m.is_static)
        .unwrap();
    assert_eq!(
        instance.return_type,
        Some(PhpType::parse("Builder<static>")),
        "instance should be the virtual replacement"
    );
    assert_eq!(instance.visibility, Visibility::Public);
    let static_m = class
        .methods
        .iter()
        .find(|m| m.name == "active" && m.is_static)
        .unwrap();
    assert_eq!(
        static_m.return_type,
        Some(PhpType::parse("Builder<static>")),
        "static variant should be added"
    );
}

#[test]
fn merge_blocks_same_name_same_staticness() {
    let mut class = make_class("Foo");
    let mut existing = make_method("active", Some("string"));
    existing.is_static = true;
    class.methods.push(Arc::new(existing));

    let mut virtual_static = make_method("active", Some("int"));
    virtual_static.is_static = true;

    let virtual_members = VirtualMembers {
        methods: vec![Arc::new(virtual_static)],
        properties: Vec::new(),
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.methods.len(), 1);
    assert_eq!(
        class.methods[0].return_type,
        Some(PhpType::parse("string")),
        "existing static method should not be overwritten by virtual static"
    );
}

#[test]
fn merge_does_not_overwrite_existing_property() {
    let mut class = make_class("Foo");
    class
        .properties
        .push(Arc::new(make_property("value", Some("string"))));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("value", Some("int"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("string")),
        "existing property should not be overwritten"
    );
}

#[test]
fn merge_replaces_mixed_property_with_specific_type() {
    let mut class = make_class("Foo");
    class
        .properties
        .push(Arc::new(make_property("vat", Some("mixed"))));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("vat", Some("Decimal"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("Decimal")),
        "mixed-typed property should be replaced by a more specific type"
    );
}

#[test]
fn merge_replaces_untyped_property_with_specific_type() {
    let mut class = make_class("Foo");
    class.properties.push(Arc::new(make_property("vat", None)));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("vat", Some("Decimal"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("Decimal")),
        "untyped property should be replaced by a more specific type"
    );
}

#[test]
fn merge_does_not_replace_mixed_with_mixed() {
    let mut class = make_class("Foo");
    class
        .properties
        .push(Arc::new(make_property("col", Some("mixed"))));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("col", Some("mixed"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(class.properties[0].type_hint, Some(PhpType::parse("mixed")));
}

#[test]
fn merge_does_not_replace_specific_type_with_mixed() {
    let mut class = make_class("Foo");
    class
        .properties
        .push(Arc::new(make_property("col", Some("string"))));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("col", Some("mixed"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("string")),
        "specific type should not be overwritten by mixed"
    );
}

#[test]
fn merge_generic_type_beats_bare_type() {
    let mut class = make_class("Foo");
    class
        .properties
        .push(Arc::new(make_property("tags", Some("array"))));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("tags", Some("array<string>"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("array<string>")),
        "generic type should replace bare type of the same base"
    );
}

#[test]
fn merge_bare_type_beats_mixed() {
    let mut class = make_class("Foo");
    class
        .properties
        .push(Arc::new(make_property("tags", Some("mixed"))));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("tags", Some("array"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("array")),
        "bare type should replace mixed"
    );
}

#[test]
fn merge_bare_type_does_not_replace_generic() {
    let mut class = make_class("Foo");
    class
        .properties
        .push(Arc::new(make_property("tags", Some("array<int>"))));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("tags", Some("array"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("array<int>")),
        "bare type should not replace a more specific generic type"
    );
}

#[test]
fn merge_same_specificity_preserves_first_writer() {
    let mut class = make_class("Foo");
    class
        .properties
        .push(Arc::new(make_property("tags", Some("array<int>"))));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("tags", Some("array<string>"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("array<int>")),
        "equal specificity should preserve the first writer, not merge or replace"
    );
}

#[test]
fn merge_mixed_does_not_replace_bare_type() {
    let mut class = make_class("Foo");
    class
        .properties
        .push(Arc::new(make_property("tags", Some("array"))));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("tags", Some("mixed"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("array")),
        "mixed should not replace a bare type"
    );
}

#[test]
fn merge_native_type_hint_beats_untyped_virtual() {
    // A property with no type_hint but a native_type_hint should score
    // higher than an untyped virtual property (specificity 0).
    let mut class = make_class("Foo");
    class.properties.push(Arc::new(PropertyInfo {
        native_type_hint: Some(PhpType::parse("string")),
        type_hint: None,
        ..PropertyInfo::virtual_property("name", None)
    }));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("name", None)], // no type at all
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0]
            .native_type_hint
            .as_ref()
            .map(|t| t.to_string())
            .as_deref(),
        Some("string"),
        "property with native type hint should not be overwritten by untyped virtual"
    );
}

#[test]
fn merge_native_type_hint_beats_mixed_virtual() {
    // A property whose type_hint is None but native_type_hint is "int"
    // should not be replaced by a virtual property typed "mixed".
    let mut class = make_class("Foo");
    class.properties.push(Arc::new(PropertyInfo {
        native_type_hint: Some(PhpType::parse("int")),
        type_hint: None,
        ..PropertyInfo::virtual_property("code", None)
    }));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("code", Some("mixed"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0]
            .native_type_hint
            .as_ref()
            .map(|t| t.to_string())
            .as_deref(),
        Some("int"),
        "property with native type hint should not be overwritten by mixed virtual"
    );
}

#[test]
fn merge_specific_virtual_beats_native_only() {
    // A virtual property with a specific type_hint ("Decimal") should
    // replace a property that only has a native_type_hint ("string")
    // because docblock specificity (1) equals native specificity (1)
    // and the incoming property does not win on a tie.
    // Actually: both score 1, so the existing property is preserved
    // (first-writer-wins at the same tier).
    let mut class = make_class("Foo");
    class.properties.push(Arc::new(PropertyInfo {
        native_type_hint: Some(PhpType::parse("string")),
        type_hint: None,
        ..PropertyInfo::virtual_property("col", None)
    }));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("col", Some("string"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    // Same specificity (both 1) → first writer wins
    assert_eq!(
        class.properties[0]
            .native_type_hint
            .as_ref()
            .map(|t| t.to_string())
            .as_deref(),
        Some("string"),
        "equal specificity should preserve the existing property"
    );
}

#[test]
fn merge_generic_virtual_beats_native_bare() {
    // A virtual property with a generic type ("array<string>", score 2)
    // should replace a property that only has a native bare type
    // ("array", score 1 via native fallback).
    let mut class = make_class("Foo");
    class.properties.push(Arc::new(PropertyInfo {
        native_type_hint: Some(PhpType::parse("array")),
        type_hint: None,
        ..PropertyInfo::virtual_property("tags", None)
    }));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![make_property("tags", Some("array<string>"))],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("array<string>")),
        "generic virtual type should replace a property with only a bare native type"
    );
}

#[test]
fn merge_docblock_tag_overrides_schema_column_at_equal_specificity() {
    // A trait/class `@property Carbon $published_at` declares the runtime
    // attribute type; a schema-derived `string|null` for the same column
    // only knows the storage type.  The explicit tag must win even though
    // both score the same specificity.
    let mut class = make_class("App\\Models\\Event");
    class.properties.push(Arc::new(PropertyInfo {
        source: Some(PropertySource::DatabaseColumn {
            column: crate::types::DatabaseColumnSource {
                connection: "primary".to_string(),
                table: "events".to_string(),
                column: "published_at".to_string(),
                database_type: "TIMESTAMP".to_string(),
                nullable: true,
                default: None,
                generated_expression: None,
                generated_mode: None,
            },
            attribute_default: None,
            mutator: None,
        }),
        ..PropertyInfo::virtual_property("published_at", Some("string|null"))
    }));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![PropertyInfo {
            source: Some(PropertySource::DocblockTag),
            ..PropertyInfo::virtual_property("published_at", Some("Illuminate\\Support\\Carbon"))
        }],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("Illuminate\\Support\\Carbon")),
        "@property tag should override the schema-derived column type"
    );
}

#[test]
fn merge_docblock_tag_does_not_override_cast_type() {
    // A `$casts` entry is real PHP code describing the runtime type, so
    // an equal-specificity @property tag does not displace it.
    let mut class = make_class("App\\Models\\Event");
    class.properties.push(Arc::new(PropertyInfo {
        source: Some(PropertySource::Cast {
            cast: "immutable_datetime".to_string(),
            column: None,
            attribute_default: None,
            mutator: None,
        }),
        ..PropertyInfo::virtual_property("published_at", Some("Carbon\\CarbonImmutable"))
    }));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![PropertyInfo {
            source: Some(PropertySource::DocblockTag),
            ..PropertyInfo::virtual_property("published_at", Some("Illuminate\\Support\\Carbon"))
        }],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("Carbon\\CarbonImmutable")),
        "a $casts-derived type should not be displaced by an equal-specificity @property tag"
    );
}

#[test]
fn merge_docblock_tag_does_not_override_real_property() {
    // Real declared properties always beat docblock tags.
    let mut class = make_class("Foo");
    class.properties.push(Arc::new(PropertyInfo {
        is_readonly: false,
        is_virtual: false,
        ..PropertyInfo::virtual_property("name", Some("string"))
    }));

    let virtual_members = VirtualMembers {
        methods: Vec::new(),
        properties: vec![PropertyInfo {
            source: Some(PropertySource::DocblockTag),
            ..PropertyInfo::virtual_property("name", Some("Stringable"))
        }],
        constants: Vec::new(),
    };

    merge_virtual_members(&mut class, virtual_members);

    assert_eq!(class.properties.len(), 1);
    assert_eq!(
        class.properties[0].type_hint,
        Some(PhpType::parse("string")),
        "a real declared property should not be displaced by a @property tag"
    );
}

#[test]
fn trait_property_tag_beats_schema_column_end_to_end() {
    // Full resolution pipeline: a model whose table schema types
    // `published_at` as `string|null` but whose used trait declares
    // `@property Carbon $published_at` resolves to Carbon (issue #306).
    use crate::virtual_members::laravel::database_schema::{SchemaIndex, parse_schema_dump};

    let mut publishes = make_class("App\\Models\\Concerns\\Publishes");
    publishes.kind = ClassLikeKind::Trait;
    publishes.set_class_docblock(Some(
        "/**\n * @property \\Illuminate\\Support\\Carbon $published_at\n */".to_string(),
    ));

    let mut event = make_class("App\\Models\\Event");
    event.parent_class = Some(atom("Illuminate\\Database\\Eloquent\\Model"));
    event.used_traits = vec![atom("App\\Models\\Concerns\\Publishes")];
    event.laravel_mut();

    let publishes_arc = Arc::new(publishes);
    let model_arc = Arc::new(make_class("Illuminate\\Database\\Eloquent\\Model"));
    let carbon_arc = Arc::new(make_class("Illuminate\\Support\\Carbon"));
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "App\\Models\\Concerns\\Publishes" => Some(Arc::clone(&publishes_arc)),
            "Illuminate\\Database\\Eloquent\\Model" => Some(Arc::clone(&model_arc)),
            "Illuminate\\Support\\Carbon" => Some(Arc::clone(&carbon_arc)),
            _ => None,
        }
    };

    let schema = SchemaIndex::from_tables(
        Some("primary".to_string()),
        parse_schema_dump(
            "primary",
            "CREATE TABLE events (id bigint NOT NULL, published_at timestamp);",
        ),
    );
    let cache = crate::virtual_members::new_resolved_class_cache();
    cache.write().set_schema_index(schema);

    let resolved =
        crate::virtual_members::resolve_class_fully_cached(&event, &class_loader, &cache);
    let prop = resolved
        .properties
        .iter()
        .find(|p| p.name == "published_at")
        .expect("published_at should exist");
    assert_eq!(
        prop.type_hint_str().as_deref(),
        Some("\\Illuminate\\Support\\Carbon"),
        "trait @property should beat the schema-derived column type"
    );
}

#[test]
fn merge_handles_empty_virtual_members() {
    let mut class = make_class("Foo");
    class
        .methods
        .push(Arc::new(make_method("foo", Some("void"))));
    class
        .properties
        .push(Arc::new(make_property("bar", Some("int"))));

    merge_virtual_members(
        &mut class,
        VirtualMembers {
            methods: Vec::new(),
            properties: Vec::new(),
            constants: Vec::new(),
        },
    );

    assert_eq!(class.methods.len(), 1);
    assert_eq!(class.properties.len(), 1);
}

// ── apply_virtual_members / provider priority tests ─────────────────

/// A test provider that always applies and contributes fixed members.
struct TestProvider {
    methods: Vec<Arc<MethodInfo>>,
    properties: Vec<PropertyInfo>,
}

impl VirtualMemberProvider for TestProvider {
    fn applies_to(
        &self,
        _class: &ClassInfo,
        _class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool {
        true
    }

    fn provide(
        &self,
        _class: &ClassInfo,
        _class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        _cache: Option<&ResolvedClassCache>,
    ) -> VirtualMembers {
        VirtualMembers {
            methods: self.methods.clone(),
            properties: self.properties.clone(),
            constants: Vec::new(),
        }
    }
}

/// A test provider that never applies.
struct NeverProvider;

impl VirtualMemberProvider for NeverProvider {
    fn applies_to(
        &self,
        _class: &ClassInfo,
        _class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool {
        false
    }

    fn provide(
        &self,
        _class: &ClassInfo,
        _class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        _cache: Option<&ResolvedClassCache>,
    ) -> VirtualMembers {
        panic!("provide should not be called when applies_to returns false")
    }
}

#[test]
fn apply_providers_in_priority_order() {
    let mut class = make_class("Foo");

    // Higher priority provider contributes "doStuff" returning "string"
    let high_priority = Box::new(TestProvider {
        methods: vec![Arc::new(make_method("doStuff", Some("string")))],
        properties: Vec::new(),
    }) as Box<dyn VirtualMemberProvider>;

    // Lower priority provider contributes "doStuff" returning "int"
    // (should be shadowed) and "other" returning "bool" (should be added)
    let low_priority = Box::new(TestProvider {
        methods: vec![
            Arc::new(make_method("doStuff", Some("int"))),
            Arc::new(make_method("other", Some("bool"))),
        ],
        properties: Vec::new(),
    }) as Box<dyn VirtualMemberProvider>;

    let providers: Vec<Box<dyn VirtualMemberProvider>> = vec![high_priority, low_priority];
    let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };

    apply_virtual_members(&mut class, &class_loader, &providers, None);

    assert_eq!(class.methods.len(), 2);

    let do_stuff = class.methods.iter().find(|m| m.name == "doStuff").unwrap();
    assert_eq!(
        do_stuff.return_type,
        Some(PhpType::parse("string")),
        "higher-priority provider should win"
    );

    let other = class.methods.iter().find(|m| m.name == "other").unwrap();
    assert_eq!(other.return_type, Some(PhpType::parse("bool")));
}

#[test]
fn apply_providers_skips_non_applicable() {
    let mut class = make_class("Foo");

    let providers: Vec<Box<dyn VirtualMemberProvider>> = vec![Box::new(NeverProvider)];
    let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };

    apply_virtual_members(&mut class, &class_loader, &providers, None);

    assert!(class.methods.is_empty());
    assert!(class.properties.is_empty());
}

#[test]
fn apply_providers_real_members_beat_virtual() {
    let mut class = make_class("Foo");
    class
        .methods
        .push(Arc::new(make_method("realMethod", Some("string"))));

    let provider = Box::new(TestProvider {
        methods: vec![Arc::new(make_method("realMethod", Some("int")))],
        properties: Vec::new(),
    }) as Box<dyn VirtualMemberProvider>;

    let providers: Vec<Box<dyn VirtualMemberProvider>> = vec![provider];
    let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };

    apply_virtual_members(&mut class, &class_loader, &providers, None);

    assert_eq!(class.methods.len(), 1);
    assert_eq!(
        class.methods[0].return_type,
        Some(PhpType::parse("string")),
        "real declared method should not be overwritten by virtual"
    );
}

#[test]
fn apply_providers_property_priority() {
    let mut class = make_class("Foo");

    let high_priority = Box::new(TestProvider {
        methods: Vec::new(),
        properties: vec![make_property("name", Some("string"))],
    }) as Box<dyn VirtualMemberProvider>;

    let low_priority = Box::new(TestProvider {
        methods: Vec::new(),
        properties: vec![
            make_property("name", Some("mixed")),
            make_property("email", Some("string")),
        ],
    }) as Box<dyn VirtualMemberProvider>;

    let providers: Vec<Box<dyn VirtualMemberProvider>> = vec![high_priority, low_priority];
    let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };

    apply_virtual_members(&mut class, &class_loader, &providers, None);

    assert_eq!(class.properties.len(), 2);

    let name = class.properties.iter().find(|p| p.name == "name").unwrap();
    assert_eq!(
        name.type_hint,
        Some(PhpType::parse("string")),
        "higher-priority provider property should win"
    );

    let email = class.properties.iter().find(|p| p.name == "email").unwrap();
    assert_eq!(email.type_hint, Some(PhpType::parse("string")));
}

/// When a higher-priority provider contributes a `mixed` property and
/// a lower-priority provider has a specific type, the specific type
/// should replace the `mixed` placeholder.
#[test]
fn apply_providers_low_priority_overrides_mixed_from_high_priority() {
    let mut class = make_class("Foo");

    // Simulates the Laravel model provider adding a column with type `mixed`
    // (e.g. from $fillable or an unresolvable cast).
    let high_priority = Box::new(TestProvider {
        methods: Vec::new(),
        properties: vec![make_property("vat", Some("mixed"))],
    }) as Box<dyn VirtualMemberProvider>;

    // Simulates the PHPDoc provider contributing `@property Decimal $vat`.
    let low_priority = Box::new(TestProvider {
        methods: Vec::new(),
        properties: vec![make_property("vat", Some("Decimal"))],
    }) as Box<dyn VirtualMemberProvider>;

    let providers: Vec<Box<dyn VirtualMemberProvider>> = vec![high_priority, low_priority];
    let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };

    apply_virtual_members(&mut class, &class_loader, &providers, None);

    assert_eq!(class.properties.len(), 1);

    let vat = class.properties.iter().find(|p| p.name == "vat").unwrap();
    assert_eq!(
        vat.type_hint,
        Some(PhpType::parse("Decimal")),
        "PHPDoc @property type should replace mixed from Laravel provider"
    );
}

/// When a higher-priority provider contributes a specific type,
/// a lower-priority provider must not replace it — even with another
/// specific type.
#[test]
fn apply_providers_low_priority_cannot_override_specific_from_high_priority() {
    let mut class = make_class("Foo");

    let high_priority = Box::new(TestProvider {
        methods: Vec::new(),
        properties: vec![make_property("is_admin", Some("bool"))],
    }) as Box<dyn VirtualMemberProvider>;

    let low_priority = Box::new(TestProvider {
        methods: Vec::new(),
        properties: vec![make_property("is_admin", Some("int"))],
    }) as Box<dyn VirtualMemberProvider>;

    let providers: Vec<Box<dyn VirtualMemberProvider>> = vec![high_priority, low_priority];
    let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };

    apply_virtual_members(&mut class, &class_loader, &providers, None);

    assert_eq!(class.properties.len(), 1);

    let prop = class
        .properties
        .iter()
        .find(|p| p.name == "is_admin")
        .unwrap();
    assert_eq!(
        prop.type_hint,
        Some(PhpType::parse("bool")),
        "higher-priority specific type should not be replaced"
    );
}

/// When a higher-priority provider contributes a bare type (`array`) and
/// a lower-priority provider has a generic type (`array<string>`), the
/// more specific generic type should win.
#[test]
fn apply_providers_generic_from_low_priority_beats_bare_from_high_priority() {
    let mut class = make_class("Foo");

    // Simulates the Laravel model provider adding a cast column with
    // bare type `array` (from `$casts = ['tags' => 'array']`).
    let high_priority = Box::new(TestProvider {
        methods: Vec::new(),
        properties: vec![make_property("tags", Some("array"))],
    }) as Box<dyn VirtualMemberProvider>;

    // Simulates the PHPDoc provider contributing
    // `@property array<string> $tags`.
    let low_priority = Box::new(TestProvider {
        methods: Vec::new(),
        properties: vec![make_property("tags", Some("array<string>"))],
    }) as Box<dyn VirtualMemberProvider>;

    let providers: Vec<Box<dyn VirtualMemberProvider>> = vec![high_priority, low_priority];
    let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };

    apply_virtual_members(&mut class, &class_loader, &providers, None);

    assert_eq!(class.properties.len(), 1);

    let tags = class.properties.iter().find(|p| p.name == "tags").unwrap();
    assert_eq!(
        tags.type_hint,
        Some(PhpType::parse("array<string>")),
        "generic type from PHPDoc should replace bare type from Laravel provider"
    );
}

/// When both providers contribute the same specificity level, the
/// first writer (higher priority) wins.  We do NOT try to merge
/// `array<int>` and `array<string>` into `array<int|string>`.
#[test]
fn apply_providers_same_specificity_preserves_high_priority() {
    let mut class = make_class("Foo");

    let high_priority = Box::new(TestProvider {
        methods: Vec::new(),
        properties: vec![make_property("tags", Some("array<int>"))],
    }) as Box<dyn VirtualMemberProvider>;

    let low_priority = Box::new(TestProvider {
        methods: Vec::new(),
        properties: vec![make_property("tags", Some("array<string>"))],
    }) as Box<dyn VirtualMemberProvider>;

    let providers: Vec<Box<dyn VirtualMemberProvider>> = vec![high_priority, low_priority];
    let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };

    apply_virtual_members(&mut class, &class_loader, &providers, None);

    assert_eq!(class.properties.len(), 1);

    let tags = class.properties.iter().find(|p| p.name == "tags").unwrap();
    assert_eq!(
        tags.type_hint,
        Some(PhpType::parse("array<int>")),
        "equal specificity should preserve higher-priority provider, not merge types"
    );
}

#[test]
fn default_providers_has_laravel_and_phpdoc() {
    let providers = default_providers(true);
    assert_eq!(
        providers.len(),
        3,
        "should have LaravelModelProvider, LaravelFactoryProvider, and PHPDocProvider registered"
    );
}

#[test]
fn default_providers_omits_laravel_when_not_laravel() {
    let providers = default_providers(false);
    assert_eq!(
        providers.len(),
        1,
        "non-Laravel projects should only register the PHPDoc provider"
    );
}

// ── resolve_class_fully tests ───────────────────────────────────────

#[test]
fn resolve_class_fully_returns_same_as_base_when_no_providers() {
    // With no providers registered, resolve_class_fully should produce
    // the same result as resolve_class_with_inheritance.
    let mut class = make_class("Child");
    class
        .methods
        .push(Arc::new(make_method("childMethod", Some("void"))));
    class.parent_class = Some(atom("Parent"));

    let mut parent = make_class("Parent");
    parent
        .methods
        .push(Arc::new(make_method("parentMethod", Some("string"))));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Parent" {
            Some(Arc::new(parent.clone()))
        } else {
            None
        }
    };

    let base = crate::inheritance::resolve_class_with_inheritance(&class, &class_loader);
    let full = crate::virtual_members::resolve_class_fully(&class, &class_loader);

    assert_eq!(base.methods.len(), full.methods.len());
    assert_eq!(base.properties.len(), full.properties.len());
    for base_method in &base.methods {
        assert!(
            full.methods.iter().any(|m| m.name == base_method.name),
            "full resolution should contain all base methods"
        );
    }
}

// ── transformed-method interning tests ──────────────────────────────

#[test]
fn intern_transformed_method_shares_same_origin_and_transform() {
    let cache = new_resolved_class_cache();
    let _guard = with_active_resolved_class_cache(&cache);

    let origin = Arc::new(make_method("where", Some("Builder<TModel>")));
    let mut subs = std::collections::HashMap::new();
    subs.insert("TModel".to_string(), PhpType::parse("App\\Models\\User"));
    let fp = cache::TransformFingerprint::new(Some(&subs), None, 0);

    let first = cache::intern_transformed_method(&origin, fp, || {
        let mut m = (*origin).clone();
        m.return_type = Some(PhpType::parse("Builder<App\\Models\\User>"));
        m
    });
    let second = cache::intern_transformed_method(&origin, fp, || {
        panic!("second lookup must hit the interned value, not rebuild");
    });

    assert!(
        Arc::ptr_eq(&first, &second),
        "same (origin, transform) must share one allocation"
    );
    assert_eq!(
        first.return_type_str().as_deref(),
        Some("Builder<App\\Models\\User>")
    );
}

#[test]
fn intern_transformed_method_distinguishes_transforms() {
    let cache = new_resolved_class_cache();
    let _guard = with_active_resolved_class_cache(&cache);

    let origin = Arc::new(make_method("where", Some("Builder<TModel>")));
    let mut subs_user = std::collections::HashMap::new();
    subs_user.insert("TModel".to_string(), PhpType::parse("User"));
    let mut subs_order = std::collections::HashMap::new();
    subs_order.insert("TModel".to_string(), PhpType::parse("Order"));

    let fp_user = cache::TransformFingerprint::new(Some(&subs_user), None, 0);
    let fp_order = cache::TransformFingerprint::new(Some(&subs_order), None, 0);
    assert_ne!(fp_user, fp_order, "different subs must fingerprint apart");

    // Same subs but different flags must also fingerprint apart.
    let fp_flagged = cache::TransformFingerprint::new(Some(&subs_user), None, 1);
    assert_ne!(
        fp_user, fp_flagged,
        "flags must participate in the fingerprint"
    );

    let user = cache::intern_transformed_method(&origin, fp_user, || {
        let mut m = (*origin).clone();
        m.return_type = Some(PhpType::parse("Builder<User>"));
        m
    });
    let order = cache::intern_transformed_method(&origin, fp_order, || {
        let mut m = (*origin).clone();
        m.return_type = Some(PhpType::parse("Builder<Order>"));
        m
    });
    assert!(!Arc::ptr_eq(&user, &order));
    assert_eq!(user.return_type_str().as_deref(), Some("Builder<User>"));
    assert_eq!(order.return_type_str().as_deref(), Some("Builder<Order>"));
}

#[test]
fn intern_transformed_method_ignores_dead_origin_entries() {
    let cache = new_resolved_class_cache();
    let _guard = with_active_resolved_class_cache(&cache);

    let origin_a = Arc::new(make_method("first", Some("TValue")));
    let origin_b = Arc::new(make_method("first", Some("TValue")));
    let fp = cache::TransformFingerprint::new(None, Some("Target"), 0);

    let a = cache::intern_transformed_method(&origin_a, fp, || (*origin_a).clone());
    // A different origin with the same fingerprint must not be served
    // origin A's entry, even though the fingerprints collide by design.
    let b = cache::intern_transformed_method(&origin_b, fp, || {
        let mut m = (*origin_b).clone();
        m.is_static = true;
        m
    });
    assert!(
        !Arc::ptr_eq(&a, &b),
        "distinct origins must not share entries"
    );
    assert!(b.is_static);
}

#[test]
fn intern_transformed_method_without_active_cache_builds_directly() {
    // No guard: builds must run every time but still produce the value.
    let origin = Arc::new(make_method("where", Some("Builder<TModel>")));
    let fp = cache::TransformFingerprint::new(None, None, 0);
    let one = cache::intern_transformed_method(&origin, fp, || (*origin).clone());
    let two = cache::intern_transformed_method(&origin, fp, || (*origin).clone());
    assert!(
        !Arc::ptr_eq(&one, &two),
        "no cache active: nothing is interned"
    );
}

// ── evict_fqn / depends_on_any tests ────────────────────────────────

/// Helper: create a bare cache store for eviction tests.
/// `evict_fqn` operates on the inner [`cache::ResolvedCacheInner`], not
/// the `Arc<Mutex<…>>` wrapper used at runtime.
fn make_cache() -> cache::ResolvedCacheInner {
    cache::ResolvedCacheInner::default()
}

#[test]
fn resolve_class_fully_with_type_args_caches_specialisation() {
    let mut collection = make_class("Collection");
    collection.template_params.push(atom("TValue"));
    collection
        .methods
        .push(Arc::new(make_method("first", Some("TValue"))));

    let cache = new_resolved_class_cache();
    let class_loader = |_name: &str| None;
    let args = vec![PhpType::parse("User")];

    let first = resolve_class_fully_with_type_args(&collection, &class_loader, Some(&cache), &args);
    let second =
        resolve_class_fully_with_type_args(&collection, &class_loader, Some(&cache), &args);

    assert!(
        Arc::ptr_eq(&first, &second),
        "same generic specialisation should come from cache"
    );
    assert_eq!(
        first.get_method("first").and_then(|m| m.return_type_str()),
        Some("User".to_string())
    );

    let map = cache.read();
    assert!(
        map.contains_key(&(atom("Collection"), Vec::new())),
        "base resolved class should be cached separately"
    );
    assert!(
        map.contains_key(&(atom("Collection"), vec!["User".to_string()])),
        "generic specialisation should be cached by type arguments"
    );
}

#[test]
fn base_resolution_shared_across_distinct_specialisations() {
    // The point of the two-stage split: the expensive base resolution
    // (inheritance/trait/virtual-member merge) runs once per class, no
    // matter how many distinct generic instantiations are requested. A
    // parent that the inheritance walk must load lets us count whether
    // the base merge re-ran: on a cached base hit the parent is not
    // loaded again.
    let mut base = make_class("Base");
    base.methods
        .push(Arc::new(make_method("save", Some("bool"))));

    let mut collection = make_class("Collection");
    collection.template_params.push(atom("TValue"));
    collection.parent_class = Some(atom("Base"));
    collection
        .methods
        .push(Arc::new(make_method("first", Some("TValue"))));

    let base_arc = Arc::new(base);
    let collection_arc = Arc::new(collection);
    let base_loads = std::cell::Cell::new(0u32);
    let class_loader = |name: &str| -> Option<Arc<crate::types::ClassInfo>> {
        match name {
            "Base" => {
                base_loads.set(base_loads.get() + 1);
                Some(Arc::clone(&base_arc))
            }
            "Collection" => Some(Arc::clone(&collection_arc)),
            _ => None,
        }
    };

    let cache = new_resolved_class_cache();

    // First specialisation triggers the one-and-only base resolution.
    let user = resolve_class_fully_with_type_args(
        &collection_arc,
        &class_loader,
        Some(&cache),
        &[PhpType::parse("User")],
    );
    let after_first = base_loads.get();
    assert!(after_first > 0, "base resolution must load the parent");

    // A different specialisation must reuse the cached base, loading the
    // parent zero additional times.
    let order = resolve_class_fully_with_type_args(
        &collection_arc,
        &class_loader,
        Some(&cache),
        &[PhpType::parse("Order")],
    );
    assert_eq!(
        base_loads.get(),
        after_first,
        "distinct generic specialisation must not re-run base resolution"
    );

    // Each specialisation still substitutes its own type argument.
    assert_eq!(
        user.get_method("first").and_then(|m| m.return_type_str()),
        Some("User".to_string())
    );
    assert_eq!(
        order.get_method("first").and_then(|m| m.return_type_str()),
        Some("Order".to_string())
    );

    let map = cache.read();
    assert!(map.contains_key(&(atom("Collection"), Vec::new())));
    assert!(map.contains_key(&(atom("Collection"), vec!["User".to_string()])));
    assert!(map.contains_key(&(atom("Collection"), vec!["Order".to_string()])));
}

#[test]
fn evict_removes_direct_match() {
    let mut cache = make_cache();
    let cls = make_class("App\\Models\\User");
    cache.insert(
        ("App\\Models\\User".to_string().into(), Vec::new()),
        Arc::new(cls),
    );

    evict_fqn(&mut cache, "App\\Models\\User");
    assert!(cache.is_empty(), "direct match should be evicted");
}

#[test]
fn evict_transitively_removes_child_class() {
    let mut cache = make_cache();

    let parent = make_class("Model");
    cache.insert(
        ("App\\Models\\Model".to_string().into(), Vec::new()),
        Arc::new(parent),
    );

    let mut child = make_class("User");
    child.parent_class = Some(atom("App\\Models\\Model"));
    cache.insert(
        ("App\\Models\\User".to_string().into(), Vec::new()),
        Arc::new(child),
    );

    evict_fqn(&mut cache, "App\\Models\\Model");
    assert!(cache.is_empty(), "both parent and child should be evicted");
}

#[test]
fn evict_transitively_removes_model_referencing_cast_class() {
    let mut cache = make_cache();

    let cast_class = make_class("DecimalCast");
    cache.insert(
        ("App\\Casts\\DecimalCast".to_string().into(), Vec::new()),
        Arc::new(cast_class),
    );

    let mut model = make_class("Setting");
    model.laravel_mut().casts_definitions =
        vec![("vat".to_string(), "App\\Casts\\DecimalCast".to_string())];
    cache.insert(
        ("App\\Models\\Setting".to_string().into(), Vec::new()),
        Arc::new(model),
    );

    // Evict the cast class — the model should be transitively evicted.
    evict_fqn(&mut cache, "App\\Casts\\DecimalCast");
    assert!(
        cache.is_empty(),
        "model referencing cast class via $casts should be transitively evicted"
    );
}

#[test]
fn evict_cast_class_with_colon_argument_transitively_removes_model() {
    let mut cache = make_cache();

    let cast_class = make_class("DecimalCast");
    cache.insert(
        ("App\\Casts\\DecimalCast".to_string().into(), Vec::new()),
        Arc::new(cast_class),
    );

    let mut model = make_class("Setting");
    // Cast type has a `:argument` suffix like `DecimalCast:8:2`.
    model.laravel_mut().casts_definitions =
        vec![("vat".to_string(), "App\\Casts\\DecimalCast:8:2".to_string())];
    cache.insert(
        ("App\\Models\\Setting".to_string().into(), Vec::new()),
        Arc::new(model),
    );

    evict_fqn(&mut cache, "App\\Casts\\DecimalCast");
    assert!(
        cache.is_empty(),
        "cast type with :argument suffix should still trigger transitive eviction"
    );
}

#[test]
fn evict_cast_class_matched_by_short_name() {
    let mut cache = make_cache();

    let cast_class = make_class("DecimalCast");
    cache.insert(
        ("App\\Casts\\DecimalCast".to_string().into(), Vec::new()),
        Arc::new(cast_class),
    );

    let mut model = make_class("Setting");
    // The model references the cast class by short name (same-file scenario).
    model.laravel_mut().casts_definitions = vec![("vat".to_string(), "DecimalCast".to_string())];
    cache.insert(
        ("App\\Models\\Setting".to_string().into(), Vec::new()),
        Arc::new(model),
    );

    evict_fqn(&mut cache, "App\\Casts\\DecimalCast");
    assert!(
        cache.is_empty(),
        "short-name cast reference should match FQN eviction"
    );
}

#[test]
fn evict_cast_class_canonical() {
    let mut cache = make_cache();

    let cast_class = make_class("DecimalCast");
    cache.insert(
        ("App\\Casts\\DecimalCast".to_string().into(), Vec::new()),
        Arc::new(cast_class),
    );

    let mut model = make_class("Setting");
    // Cast values are canonical (no leading `\`) after ingestion normalization.
    model.laravel_mut().casts_definitions =
        vec![("vat".to_string(), "App\\Casts\\DecimalCast".to_string())];
    cache.insert(
        ("App\\Models\\Setting".to_string().into(), Vec::new()),
        Arc::new(model),
    );

    evict_fqn(&mut cache, "App\\Casts\\DecimalCast");
    assert!(
        cache.is_empty(),
        "canonical cast type should trigger transitive eviction"
    );
}

#[test]
fn evict_builtin_cast_does_not_affect_model() {
    let mut cache = make_cache();

    let mut model = make_class("Setting");
    model.laravel_mut().casts_definitions = vec![
        ("is_active".to_string(), "boolean".to_string()),
        ("created_at".to_string(), "datetime".to_string()),
    ];
    cache.insert(
        ("App\\Models\\Setting".to_string().into(), Vec::new()),
        Arc::new(model),
    );

    // Evicting a random class should not affect the model since
    // its casts only reference built-in types.
    evict_fqn(&mut cache, "App\\Something\\Unrelated");
    assert_eq!(
        cache.len(),
        1,
        "model with only built-in casts should not be evicted"
    );
}

#[test]
fn evict_cast_class_chains_through_model_to_child() {
    let mut cache = make_cache();

    let cast_class = make_class("DecimalCast");
    cache.insert(
        ("App\\Casts\\DecimalCast".to_string().into(), Vec::new()),
        Arc::new(cast_class),
    );

    let mut model = make_class("Setting");
    model.laravel_mut().casts_definitions =
        vec![("vat".to_string(), "App\\Casts\\DecimalCast".to_string())];
    cache.insert(
        ("App\\Models\\Setting".to_string().into(), Vec::new()),
        Arc::new(model),
    );

    let mut child = make_class("AdvancedSetting");
    child.parent_class = Some(atom("App\\Models\\Setting"));
    cache.insert(
        (
            "App\\Models\\AdvancedSetting".to_string().into(),
            Vec::new(),
        ),
        Arc::new(child),
    );

    // Evicting the cast class should evict the model (via casts_definitions),
    // and the child (via parent_class → model).
    evict_fqn(&mut cache, "App\\Casts\\DecimalCast");
    assert!(
        cache.is_empty(),
        "cast eviction should chain: cast → model (via casts) → child (via parent)"
    );
}

#[test]
fn evict_removes_all_generic_variants_of_fqn() {
    let mut cache = make_cache();

    let cls = make_class("App\\Support\\Collection");
    // The same class cached under two distinct generic instantiations.
    cache.insert(
        ("App\\Support\\Collection".to_string().into(), Vec::new()),
        Arc::new(cls.clone()),
    );
    cache.insert(
        (
            "App\\Support\\Collection".to_string().into(),
            vec!["App\\Models\\User".to_string()],
        ),
        Arc::new(cls),
    );

    let evicted = evict_fqn(&mut cache, "App\\Support\\Collection");
    assert!(
        cache.is_empty(),
        "every generic-arg variant of the FQN should be evicted"
    );
    assert_eq!(evicted, vec!["App\\Support\\Collection".to_string()]);
}

#[test]
fn evict_returns_empty_when_nothing_matches() {
    let mut cache = make_cache();

    let mut child = make_class("User");
    child.parent_class = Some(atom("App\\Models\\Model"));
    cache.insert(
        ("App\\Models\\User".to_string().into(), Vec::new()),
        Arc::new(child),
    );

    // An FQN that is neither cached nor depended upon yields no eviction.
    let evicted = evict_fqn(&mut cache, "App\\Unrelated\\Thing");
    assert!(evicted.is_empty(), "unmatched FQN should evict nothing");
    assert_eq!(cache.len(), 1, "unrelated entry should remain cached");
}

#[test]
fn reverse_index_is_cleaned_up_after_eviction() {
    let mut cache = make_cache();

    let parent = make_class("Model");
    cache.insert(
        ("App\\Models\\Model".to_string().into(), Vec::new()),
        Arc::new(parent),
    );

    let mut child = make_class("User");
    child.parent_class = Some(atom("App\\Models\\Model"));
    cache.insert(
        ("App\\Models\\User".to_string().into(), Vec::new()),
        Arc::new(child),
    );

    // Evict the parent; both entries go.
    evict_fqn(&mut cache, "App\\Models\\Model");
    assert!(cache.is_empty());

    // A fresh, unrelated class that happens to share the parent's FQN
    // must not be evicted by a stale reverse-dependency edge.
    let standalone = make_class("App\\Models\\Model");
    cache.insert(
        ("App\\Models\\Model".to_string().into(), Vec::new()),
        Arc::new(standalone),
    );
    let evicted = evict_fqn(&mut cache, "App\\Models\\User");
    assert!(
        evicted.is_empty(),
        "evicting the removed child must not touch the new unrelated entry"
    );
    assert_eq!(cache.len(), 1, "the standalone entry should survive");
}

// ── Interface-extends-interface transitive member merging ────────────

#[test]
fn resolve_class_fully_merges_transitive_interface_constants() {
    // Simulates the Carbon scenario:
    //   interface UnitValue       { const JANUARY = 1; }
    //   interface JsonSerializable { function jsonSerialize(): mixed; }
    //   interface DateTimeInterface { function format(string $f): string; }
    //   interface CarbonInterface extends DateTimeInterface, JsonSerializable, UnitValue {}
    //   class Carbon implements CarbonInterface {}
    //
    // Before the fix, only DateTimeInterface (the first extended
    // interface, stored in `parent_class`) was merged.  Members from
    // JsonSerializable and UnitValue were lost.

    let mut unit_value = make_class("Carbon\\Constants\\UnitValue");
    unit_value.kind = ClassLikeKind::Interface;
    unit_value.constants.push(Arc::new(ConstantInfo {
        name: crate::atom::atom("JANUARY"),
        name_offset: 0,
        type_hint: Some(PhpType::parse("int")),
        visibility: Visibility::Public,
        deprecation_message: None,
        deprecated_replacement: None,
        see_refs: Vec::new(),
        description: None,
        is_enum_case: false,
        enum_value: None,
        value: Some("1".to_string()),
        is_virtual: false,
    }));

    let mut json_serializable = make_class("JsonSerializable");
    json_serializable.kind = ClassLikeKind::Interface;
    json_serializable
        .methods
        .push(Arc::new(make_method("jsonSerialize", Some("mixed"))));

    let mut datetime_iface = make_class("DateTimeInterface");
    datetime_iface.kind = ClassLikeKind::Interface;
    datetime_iface
        .methods
        .push(Arc::new(make_method("format", Some("string"))));

    let mut carbon_iface = make_class("CarbonInterface");
    carbon_iface.kind = ClassLikeKind::Interface;
    carbon_iface.parent_class = Some(atom("DateTimeInterface"));
    carbon_iface.interfaces = vec![
        atom("DateTimeInterface"),
        atom("JsonSerializable"),
        atom("Carbon\\Constants\\UnitValue"),
    ];

    let mut carbon = make_class("Carbon");
    carbon.interfaces = vec![atom("CarbonInterface")];

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "CarbonInterface" => Some(Arc::new(carbon_iface.clone())),
            "DateTimeInterface" => Some(Arc::new(datetime_iface.clone())),
            "JsonSerializable" => Some(Arc::new(json_serializable.clone())),
            "Carbon\\Constants\\UnitValue" => Some(Arc::new(unit_value.clone())),
            _ => None,
        }
    };

    let resolved = crate::virtual_members::resolve_class_fully(&carbon, &class_loader);

    // DateTimeInterface::format — merged via CarbonInterface's parent_class chain
    assert!(
        resolved.methods.iter().any(|m| m.name == "format"),
        "should have DateTimeInterface::format"
    );

    // JsonSerializable::jsonSerialize — merged via transitive interface collection
    assert!(
        resolved.methods.iter().any(|m| m.name == "jsonSerialize"),
        "should have JsonSerializable::jsonSerialize (2nd extended interface)"
    );

    // UnitValue::JANUARY — merged via transitive interface collection
    assert!(
        resolved.constants.iter().any(|c| c.name == "JANUARY"),
        "should have UnitValue::JANUARY constant (3rd extended interface)"
    );
}

// ── Resolution is independent of cache warmth ───────────────────────
//
// Whether a dependency happens to be resolved before or after the class
// that uses it must not change the class's members: eager population
// fans out over several workers, so the order they reach a class in
// varies between runs, and a warmth-sensitive merge made the same file
// report different diagnostics each time.

/// An implementing class picks up the members its interface contributes
/// whether or not that interface was resolved (and cached) first.
///
/// `Illuminate\Contracts\View\View` gets its concrete class bound as a
/// mixin while it is being fully resolved, so a merge that skipped the
/// full resolution when the cache was cold saw the bare contract.
#[test]
fn interface_merge_matches_with_and_without_a_warm_cache() {
    let mut contract = make_class("Illuminate\\Contracts\\View\\View");
    contract.kind = ClassLikeKind::Interface;

    let mut concrete = make_class("Illuminate\\View\\View");
    concrete
        .methods
        .push(Arc::new(make_method("render", Some("string"))));

    let mut view = make_class("App\\Views\\Profile");
    view.interfaces = vec![atom("Illuminate\\Contracts\\View\\View")];

    let contract_arc = Arc::new(contract);
    let concrete_arc = Arc::new(concrete);
    let view_arc = Arc::new(view);
    let class_loader = {
        let contract_arc = Arc::clone(&contract_arc);
        let concrete_arc = Arc::clone(&concrete_arc);
        let view_arc = Arc::clone(&view_arc);
        move |name: &str| -> Option<Arc<ClassInfo>> {
            match name {
                "Illuminate\\Contracts\\View\\View" => Some(Arc::clone(&contract_arc)),
                "Illuminate\\View\\View" => Some(Arc::clone(&concrete_arc)),
                "App\\Views\\Profile" => Some(Arc::clone(&view_arc)),
                _ => None,
            }
        }
    };

    let cold = new_resolved_class_cache();
    let from_cold = resolve_class_fully_cached(&view_arc, &class_loader, &cold);

    let warm = new_resolved_class_cache();
    resolve_class_fully_cached(&contract_arc, &class_loader, &warm);
    let from_warm = resolve_class_fully_cached(&view_arc, &class_loader, &warm);

    let names = |class: &ClassInfo| -> Vec<String> {
        let mut names: Vec<String> = class.methods.iter().map(|m| m.name.to_string()).collect();
        names.sort();
        names
    };
    assert_eq!(
        names(&from_cold),
        names(&from_warm),
        "resolving the interface first must not change the implementing class"
    );
    assert!(
        from_cold.methods.iter().any(|m| m.name == "render"),
        "the contract's bound concrete should come through either way"
    );
}
