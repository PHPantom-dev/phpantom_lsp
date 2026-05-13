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

// ── replace_eloquent_collection ─────────────────────────────────

#[test]
fn replace_eloquent_collection_in_return_type() {
    let result = replace_eloquent_collection_typed(
        &PhpType::parse("Illuminate\\Database\\Eloquent\\Collection<int, App\\Models\\User>"),
        "App\\Collections\\UserCollection",
    )
    .to_string();
    assert_eq!(
        result,
        "App\\Collections\\UserCollection<int, App\\Models\\User>"
    );
}

#[test]
fn replace_eloquent_collection_preserves_other_types() {
    let result = replace_eloquent_collection_typed(
        &PhpType::parse("Illuminate\\Support\\Collection<int, string>"),
        "App\\Collections\\UserCollection",
    )
    .to_string();
    assert_eq!(result, "Illuminate\\Support\\Collection<int, string>");
}

#[test]
fn replace_eloquent_collection_in_union() {
    let result = replace_eloquent_collection_typed(
        &PhpType::parse("Illuminate\\Database\\Eloquent\\Collection<int, App\\Models\\User>|null"),
        "App\\Collections\\UserCollection",
    )
    .to_string();
    assert_eq!(
        result,
        "App\\Collections\\UserCollection<int, App\\Models\\User>|null"
    );
}

// ── build_builder_forwarded_methods ─────────────────────────────

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
    // 1 real + 3 synthesized (query, newQuery, newModelQuery)
    assert_eq!(result.len(), 4);
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
    assert_eq!(result.len(), 4);
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
    assert_eq!(result.len(), 4);
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
    assert_eq!(result.len(), 4);
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
    assert_eq!(result.len(), 4);
    assert_eq!(
        result[0].return_type_str().as_deref(),
        Some("App\\Models\\User|null"),
        "TModel should map to the concrete model class"
    );
}

#[test]
fn custom_builder_forwarding_maps_parent_tmodel_to_concrete_class() {
    let builder = make_builder(vec![make_method("first", Some("TModel|null"))]);
    let mut custom_builder = make_class("App\\Models\\UserBuilder");
    custom_builder.parent_class = Some(atom(ELOQUENT_BUILDER_FQN));
    let mut user = make_class("App\\Models\\User");
    user.laravel = Some(Box::new(crate::types::LaravelMetadata {
        custom_builder: Some(PhpType::Named("App\\Models\\UserBuilder".to_string())),
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
    assert_eq!(result.len(), 4);
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
    assert_eq!(result.len(), 4);
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
        4,
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
    assert_eq!(result.len(), 4, "Only public methods should be forwarded");
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
        4,
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
        4,
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
    assert_eq!(result.len(), 4);
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
    assert_eq!(result.len(), 4);
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
    assert_eq!(result.len(), 7);
    let names: Vec<&str> = result.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"where"));
    assert!(names.contains(&"orderBy"));
    assert!(names.contains(&"get"));
    assert!(names.contains(&"first"));
    assert!(result.iter().all(|m| m.is_static));
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
    assert_eq!(result.len(), 4);
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
    assert_eq!(result.len(), 5);
    assert_eq!(result[0].return_type_str().as_deref(), Some("string"));
    assert_eq!(result[1].return_type_str().as_deref(), Some("bool"));
}
