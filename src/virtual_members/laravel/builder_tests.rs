use super::*;
use crate::atom::atom;
use crate::php_type::PhpType;
use crate::test_fixtures::{
    make_class, make_method, make_method_with_params, make_param, no_loader,
};
use std::sync::Arc;

/// Helper: create a minimal Builder class with template params and methods.
fn make_builder(methods: Vec<MethodInfo>) -> ClassInfo {
    let mut builder = make_class(ELOQUENT_BUILDER_FQN);
    builder.template_params = vec![atom("TModel")];
    builder.methods = methods.into_iter().map(Arc::new).collect::<Vec<_>>().into();
    builder
}

// ── build_builder_forwarded_methods ─────────────────────────────

#[test]
fn model_query_factories_preserve_declaration_metadata() {
    let builder = Arc::new(make_builder(vec![]));
    let mut model = make_class("App\\Team");
    let mut original = make_method_with_params(
        "newQueryWithoutScopes",
        Some(ELOQUENT_BUILDER_FQN),
        vec![make_param("$connection", Some("string"), false)],
    );
    original.is_static = false;
    original.is_virtual = false;
    original.name_offset = 42;
    original.native_return_type = original.return_type.clone();
    model.methods.push(Arc::new(original));
    let loader = |name: &str| (name == ELOQUENT_BUILDER_FQN).then(|| Arc::clone(&builder));
    let methods = build_builder_forwarded_methods(&model, &loader, None);
    let factory = methods
        .iter()
        .find(|m| m.name == "newQueryWithoutScopes")
        .unwrap();
    assert!(!factory.is_static);
    assert!(!factory.is_virtual);
    assert_eq!(factory.name_offset, 42);
    assert_eq!(factory.parameters[0].name, "$connection");
    assert_eq!(
        factory.native_return_type,
        Some(PhpType::named(atom(ELOQUENT_BUILDER_FQN)))
    );
    assert_eq!(
        factory.return_type,
        Some(PhpType::generic(
            ELOQUENT_BUILDER_FQN,
            vec![PhpType::named(atom("App\\Team"))]
        ))
    );
    for name in ["query", "newQuery", "newModelQuery"] {
        assert_eq!(
            methods.iter().find(|m| m.name == name).unwrap().is_static,
            name == "query"
        );
    }
}

#[test]
fn builder_forwarding_returns_empty_when_builder_not_found() {
    let class = make_class("App\\Models\\User");
    let result = build_builder_forwarded_methods(&class, &no_loader, None);
    assert!(result.is_empty());
}

#[test]
fn builder_forwarding_converts_instance_to_static() {
    let mut builder = make_builder(vec![make_method("where", Some("static"))]);
    Arc::make_mut(&mut builder.methods.make_mut()[0]).is_static = false;

    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    // 1 real + 4 synthesized query factories
    assert_eq!(result.len(), 5);
    assert!(result[0].is_static, "Forwarded method should be static");
    assert_eq!(result[0].name, "where");
}

#[test]
fn builder_forwarding_maps_static_to_builder_self_type() {
    let builder = make_builder(vec![make_method("where", Some("static"))]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 5);
    assert_eq!(
        result[0].return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<App\\Models\\User>"),
        "static should map to Builder<ConcreteModel>"
    );
}

#[test]
fn builder_forwarding_maps_this_to_builder_self_type() {
    let builder = make_builder(vec![make_method("orderBy", Some("$this"))]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 5);
    assert_eq!(
        result[0].return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<App\\Models\\User>"),
        "$this should map to Builder<ConcreteModel>"
    );
}

#[test]
fn builder_forwarding_maps_self_to_builder_self_type() {
    let builder = make_builder(vec![make_method("limit", Some("self"))]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 5);
    assert_eq!(
        result[0].return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<App\\Models\\User>"),
        "self should map to Builder<ConcreteModel>"
    );
}

#[test]
fn builder_forwarding_maps_tmodel_to_concrete_class() {
    let builder = make_builder(vec![make_method("first", Some("TModel|null"))]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 5);
    assert_eq!(
        result[0].return_type_str().as_deref(),
        Some("App\\Models\\User|null"),
        "TModel should map to the concrete model class"
    );
}

#[test]
fn builder_forwarding_maps_tmodel_to_fqn_when_name_is_short() {
    let builder = make_builder(vec![
        make_method("where", Some("static")),
        make_method("firstOrFail", Some("TModel")),
    ]);
    let mut channel = make_class("Channel");
    channel.file_namespace = Some(atom("App\\Models"));

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&channel, &loader, None);
    let where_m = result.iter().find(|m| m.name == "where").unwrap();
    assert_eq!(
        where_m.return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<App\\Models\\Channel>"),
    );
    let first = result.iter().find(|m| m.name == "firstOrFail").unwrap();
    assert_eq!(
        first.return_type_str().as_deref(),
        Some("App\\Models\\Channel"),
    );
}

#[test]
fn custom_builder_forwarding_maps_parent_tmodel_to_concrete_class() {
    let builder = make_builder(vec![make_method("first", Some("TModel|null"))]);
    let mut custom_builder = make_class("App\\Models\\UserBuilder");
    custom_builder.parent_class = Some(atom(ELOQUENT_BUILDER_FQN));
    let mut user = make_class("App\\Models\\User");
    user.laravel = Some(Box::new(crate::types::LaravelMetadata {
        custom_builder: Some(PhpType::named(atom("App\\Models\\UserBuilder"))),
        ..Default::default()
    }));

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Models\\UserBuilder" {
            Some(Arc::new(custom_builder.clone()))
        } else if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    let first = result
        .iter()
        .find(|m| m.name == "first")
        .expect("first() should be inherited from the parent Builder");
    assert_eq!(
        first.return_type_str().as_deref(),
        Some("App\\Models\\User|null"),
        "Inherited parent Builder<TModel> methods should substitute TModel"
    );
}

#[test]
fn builder_forwarding_maps_generic_collection_return() {
    let builder = make_builder(vec![make_method(
        "get",
        Some("Illuminate\\Database\\Eloquent\\Collection<int, TModel>"),
    )]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 5);
    assert_eq!(
        result[0].return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Collection<int, App\\Models\\User>"),
        "Collection<int, TModel> should become Collection<int, User>"
    );
}

#[test]
fn builder_forwarding_maps_static_in_union() {
    let builder = make_builder(vec![make_method("whereNull", Some("static|null"))]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 5);
    assert_eq!(
        result[0].return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<App\\Models\\User>|null"),
        "static|null should become Builder<User>|null"
    );
}

#[test]
fn builder_forwarding_skips_magic_methods() {
    let builder = make_builder(vec![
        make_method("where", Some("static")),
        make_method("__construct", None),
        make_method("__call", Some("mixed")),
    ]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(
        result.len(),
        5,
        "Only non-magic methods should be forwarded"
    );
    assert_eq!(result[0].name, "where");
}

#[test]
fn builder_forwarding_skips_non_public_methods() {
    let mut builder = make_builder(vec![
        make_method("where", Some("static")),
        make_method("internalHelper", Some("void")),
    ]);
    Arc::make_mut(&mut builder.methods.make_mut()[1]).visibility = Visibility::Protected;
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 5, "Only public methods should be forwarded");
    assert_eq!(result[0].name, "where");
}

#[test]
fn builder_forwarding_skips_methods_already_on_model() {
    let builder = make_builder(vec![
        make_method("where", Some("static")),
        make_method("myMethod", Some("void")),
    ]);
    let mut user = make_class("App\\Models\\User");
    // The model has a static method named "myMethod" already.
    let mut existing = make_method("myMethod", Some("string"));
    existing.is_static = true;
    user.methods.push(Arc::new(existing));

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(
        result.len(),
        5,
        "Should skip 'myMethod' because the model already has it as static"
    );
    assert_eq!(result[0].name, "where");
}

#[test]
fn builder_forwarding_does_not_skip_instance_method_with_same_name() {
    // If the model has an instance method named "where", the static
    // forwarded Builder method should still appear since they differ
    // in staticness.
    let builder = make_builder(vec![make_method("where", Some("static"))]);
    let mut user = make_class("App\\Models\\User");
    let mut existing = make_method("where", Some("string"));
    existing.is_static = false;
    user.methods.push(Arc::new(existing));

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(
        result.len(),
        5,
        "Static forwarded method should be added even when an instance method with the same name exists"
    );
    assert!(result[0].is_static);
}

#[test]
fn builder_forwarding_maps_parameter_types() {
    let builder = make_builder(vec![make_method_with_params(
        "find",
        Some("TModel|null"),
        vec![make_param("$id", Some("TModel"), true)],
    )]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 5);
    assert_eq!(
        result[0].parameters[0].type_hint_str().as_deref(),
        Some("App\\Models\\User"),
        "Parameter TModel should map to the concrete model class"
    );
}

#[test]
fn builder_forwarding_preserves_method_metadata() {
    let mut builder = make_builder(vec![make_method_with_params(
        "where",
        Some("static"),
        vec![
            make_param("$column", Some("string"), true),
            make_param("$value", Some("mixed"), false),
        ],
    )]);
    Arc::make_mut(&mut builder.methods.make_mut()[0]).deprecation_message =
        Some("Use whereNew() instead".into());

    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 5);
    assert!(
        result[0].deprecation_message.is_some(),
        "Deprecated flag should be preserved"
    );
    assert_eq!(result[0].parameters.len(), 2);
    assert_eq!(result[0].parameters[0].name, "$column");
    assert!(!result[0].parameters[1].is_required);
}

#[test]
fn builder_forwarding_multiple_methods() {
    let builder = make_builder(vec![
        make_method("where", Some("static")),
        make_method("orderBy", Some("static")),
        make_method(
            "get",
            Some("Illuminate\\Database\\Eloquent\\Collection<int, TModel>"),
        ),
        make_method("first", Some("TModel|null")),
    ]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 8);
    let names: Vec<&str> = result.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"where"));
    assert!(names.contains(&"orderBy"));
    assert!(names.contains(&"get"));
    assert!(names.contains(&"first"));
    assert!(result[..4].iter().all(|m| m.is_static));
}

#[test]
fn custom_builder_chain_terminates_on_cyclic_parents() {
    // `class A extends B; class B extends A; class MyBuilder extends A`.
    // The cycle is illegal PHP but the walk must still terminate instead of
    // hanging the request thread.
    let mut a = make_class("App\\A");
    a.parent_class = Some(atom("App\\B"));
    let mut b = make_class("App\\B");
    b.parent_class = Some(atom("App\\A"));
    let mut my_builder = make_class("App\\MyBuilder");
    my_builder.parent_class = Some(atom("App\\A"));

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "App\\A" => Some(Arc::new(a.clone())),
            "App\\B" => Some(Arc::new(b.clone())),
            "App\\MyBuilder" => Some(Arc::new(my_builder.clone())),
            _ => None,
        }
    };

    assert!(
        !custom_builder_chain_declares_method(&my_builder, "scopeFoo", &loader),
        "no class in the cyclic chain declares scopeFoo"
    );
}

#[test]
fn custom_builder_chain_finds_method_through_cyclic_parents() {
    // Same cycle, but the parent `A` genuinely declares the method. It must be
    // found before the cycle is detected.
    let mut a = make_class("App\\A");
    a.parent_class = Some(atom("App\\B"));
    a.methods.push(Arc::new(make_method("scopeFoo", None)));
    let mut b = make_class("App\\B");
    b.parent_class = Some(atom("App\\A"));
    let mut my_builder = make_class("App\\MyBuilder");
    my_builder.parent_class = Some(atom("App\\A"));

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "App\\A" => Some(Arc::new(a.clone())),
            "App\\B" => Some(Arc::new(b.clone())),
            "App\\MyBuilder" => Some(Arc::new(my_builder.clone())),
            _ => None,
        }
    };

    assert!(
        custom_builder_chain_declares_method(&my_builder, "scopeFoo", &loader),
        "scopeFoo is declared on the parent A"
    );
}

#[test]
fn builder_forwarding_with_no_return_type() {
    let builder = make_builder(vec![make_method("doSomething", None)]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 5);
    assert!(
        result[0].return_type.is_none(),
        "None return type should stay None"
    );
}

#[test]
fn builder_forwarding_preserves_non_template_return_types() {
    let builder = make_builder(vec![
        make_method("toSql", Some("string")),
        make_method("exists", Some("bool")),
    ]);
    let user = make_class("App\\Models\\User");

    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == ELOQUENT_BUILDER_FQN {
            Some(Arc::new(builder.clone()))
        } else {
            None
        }
    };

    let result = build_builder_forwarded_methods(&user, &loader, None);
    assert_eq!(result.len(), 6);
    assert_eq!(result[0].return_type_str().as_deref(), Some("string"));
    assert_eq!(result[1].return_type_str().as_deref(), Some("bool"));
}
