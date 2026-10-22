use super::*;
use crate::atom::atom;
use crate::class_lookup::Fqn;
use crate::test_fixtures::{make_class, make_method, make_method_with_params, make_param};
use crate::types::{ClassInfo, ParameterInfo, Visibility};
use crate::virtual_members::laravel::ELOQUENT_MODEL_FQN;
use std::sync::Arc;

/// Helper: create a `MethodInfo` with `has_scope_attribute = true`.
fn make_scope_attr_method(name: &str, return_type: Option<&str>) -> MethodInfo {
    MethodInfo {
        has_scope_attribute: true,
        visibility: Visibility::Protected,
        ..make_method(name, return_type)
    }
}

/// Helper: create a `MethodInfo` with `has_scope_attribute = true`
/// and custom parameters.
fn make_scope_attr_method_with_params(
    name: &str,
    return_type: Option<&str>,
    params: Vec<ParameterInfo>,
) -> MethodInfo {
    MethodInfo {
        has_scope_attribute: true,
        visibility: Visibility::Protected,
        ..make_method_with_params(name, return_type, params)
    }
}

// ── public #[Scope] excluded ────────────────────────────────────────

#[test]
fn public_scope_attribute_not_treated_as_scope() {
    // Laravel silently ignores #[Scope] on public methods.
    let method = MethodInfo {
        has_scope_attribute: true,
        visibility: Visibility::Public,
        ..make_method("notAScope", Some("void"))
    };
    assert!(!is_scope_method(&method));
}

#[test]
fn protected_scope_attribute_treated_as_scope() {
    let method = make_scope_attr_method("completed", Some("void"));
    assert!(is_scope_method(&method));
}

// ── is_scope_method ─────────────────────────────────────────────────

#[test]
fn scope_method_detected() {
    let method = make_method("scopeActive", Some("void"));
    assert!(is_scope_method(&method));
}

#[test]
fn scope_method_multi_word() {
    let method = make_method("scopeRecentlyVerified", Some("void"));
    assert!(is_scope_method(&method));
}

#[test]
fn not_a_scope_bare_scope_name() {
    // "scope" alone with no suffix is not a scope
    let method = make_method("scope", Some("void"));
    assert!(!is_scope_method(&method));
}

#[test]
fn not_a_scope_different_prefix() {
    let method = make_method("getActive", Some("void"));
    assert!(!is_scope_method(&method));
}

#[test]
fn not_a_scope_lowercase_prefix() {
    // Must be exactly "scope" not "Scope"
    let method = make_method("ScopeActive", Some("void"));
    assert!(!is_scope_method(&method));
}

// ── scope_name ──────────────────────────────────────────────────────

#[test]
fn scope_name_simple() {
    assert_eq!(scope_name("scopeActive"), "active");
}

#[test]
fn scope_name_multi_word() {
    assert_eq!(scope_name("scopeRecentlyVerified"), "recentlyVerified");
}

#[test]
fn scope_name_single_char() {
    assert_eq!(scope_name("scopeA"), "a");
}

#[test]
fn scope_name_already_lowercase() {
    assert_eq!(scope_name("scopeactive"), "active");
}

// ── scope_return_type ───────────────────────────────────────────────

#[test]
fn scope_return_type_void_defaults() {
    let method = make_method("scopeActive", Some("void"));
    assert_eq!(
        scope_return_type(&method),
        PhpType::generic(
            "Illuminate\\Database\\Eloquent\\Builder",
            vec![PhpType::static_()],
        )
    );
}

#[test]
fn scope_return_type_none_defaults() {
    let method = make_method("scopeActive", None);
    assert_eq!(
        scope_return_type(&method),
        PhpType::generic(
            "Illuminate\\Database\\Eloquent\\Builder",
            vec![PhpType::static_()],
        )
    );
}

#[test]
fn scope_return_type_explicit() {
    let method = make_method(
        "scopeActive",
        Some("Illuminate\\Database\\Eloquent\\Builder<static>"),
    );
    assert_eq!(
        scope_return_type(&method),
        PhpType::generic(
            "Illuminate\\Database\\Eloquent\\Builder",
            vec![PhpType::static_()],
        )
    );
}

#[test]
fn scope_return_type_custom() {
    let method = make_method("scopeActive", Some("\\App\\Builders\\UserBuilder"));
    assert_eq!(
        scope_return_type(&method),
        PhpType::named(atom("\\App\\Builders\\UserBuilder"))
    );
}

// ── build_scope_methods (convention) ─────────────────────────────────

#[test]
fn build_scope_methods_strips_query_param() {
    let method = make_method_with_params(
        "scopeActive",
        Some("void"),
        vec![make_param(
            "$query",
            Some("\\Illuminate\\Database\\Eloquent\\Builder"),
            true,
        )],
    );

    let [instance, static_m] = build_scope_methods(&method);
    assert!(instance.parameters.is_empty());
    assert!(static_m.parameters.is_empty());
}

#[test]
fn build_scope_methods_preserves_extra_params() {
    let method = make_method_with_params(
        "scopeOfType",
        Some("void"),
        vec![
            make_param(
                "$query",
                Some("\\Illuminate\\Database\\Eloquent\\Builder"),
                true,
            ),
            make_param("$type", Some("string"), true),
            make_param("$strict", Some("bool"), false),
        ],
    );

    let [instance, static_m] = build_scope_methods(&method);
    assert_eq!(instance.parameters.len(), 2);
    assert_eq!(instance.parameters[0].name, "$type");
    assert!(instance.parameters[0].is_required);
    assert_eq!(instance.parameters[1].name, "$strict");
    assert!(!instance.parameters[1].is_required);

    assert_eq!(static_m.parameters.len(), 2);
    assert_eq!(static_m.parameters[0].name, "$type");
    assert_eq!(static_m.parameters[1].name, "$strict");
}

#[test]
fn build_scope_methods_creates_instance_and_static() {
    let method = make_method("scopeActive", Some("void"));
    let [instance, static_m] = build_scope_methods(&method);

    assert_eq!(instance.name, "active");
    assert!(!instance.is_static);
    assert_eq!(instance.visibility, Visibility::Public);

    assert_eq!(static_m.name, "active");
    assert!(static_m.is_static);
    assert_eq!(static_m.visibility, Visibility::Public);
}

#[test]
fn build_scope_methods_default_return_type() {
    let method = make_method("scopeActive", None);
    let [instance, static_m] = build_scope_methods(&method);

    assert_eq!(
        instance.return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<static>")
    );
    assert_eq!(
        static_m.return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<static>")
    );
}

#[test]
fn build_scope_methods_void_return_type() {
    let method = make_method("scopeActive", Some("void"));
    let [instance, _] = build_scope_methods(&method);

    assert_eq!(
        instance.return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<static>")
    );
}

#[test]
fn build_scope_methods_with_no_params() {
    // Scope method without any parameters (unusual but valid)
    let method = make_method("scopeActive", Some("void"));
    let [instance, static_m] = build_scope_methods(&method);

    assert!(instance.parameters.is_empty());
    assert!(static_m.parameters.is_empty());
}

#[test]
fn build_scope_methods_preserves_deprecated() {
    let mut method = make_method("scopeOld", Some("void"));
    method.extras_mut().deprecation_message = Some("Use scopeNew() instead".into());

    let [instance, static_m] = build_scope_methods(&method);
    assert!(instance.deprecation_message().is_some());
    assert!(static_m.deprecation_message().is_some());
}

// ── is_attribute_scope ──────────────────────────────────────────────

#[test]
fn scope_attribute_detected() {
    let method = make_scope_attr_method("active", Some("void"));
    assert!(is_scope_method(&method));
}

#[test]
fn scope_attribute_multi_word() {
    let method = make_scope_attr_method("recentlyVerified", Some("void"));
    assert!(is_scope_method(&method));
}

#[test]
fn scope_attribute_without_convention_prefix() {
    // "active" doesn't start with "scope", but has_scope_attribute is true
    let method = make_scope_attr_method("active", Some("void"));
    assert!(is_scope_method(&method));
    assert!(is_attribute_scope(&method));
}

#[test]
fn scope_attribute_false_and_no_convention_not_scope() {
    let method = make_method("active", Some("void"));
    assert!(!is_scope_method(&method));
}

// ── scope_name_for ──────────────────────────────────────────────────

#[test]
fn scope_name_for_attribute_uses_own_name() {
    let method = make_scope_attr_method("active", Some("void"));
    assert_eq!(scope_name_for(&method), "active");
}

#[test]
fn scope_name_for_attribute_multi_word() {
    let method = make_scope_attr_method("recentlyVerified", Some("void"));
    assert_eq!(scope_name_for(&method), "recentlyVerified");
}

#[test]
fn scope_name_for_convention_strips_prefix() {
    let method = make_method("scopeActive", Some("void"));
    assert_eq!(scope_name_for(&method), "active");
}

#[test]
fn scope_name_for_attribute_with_scope_prefix_uses_own_name() {
    // A method named "scopeActive" with #[Scope] — the attribute
    // takes priority, so the name is used as-is.
    let method = make_scope_attr_method("scopeActive", Some("void"));
    assert_eq!(scope_name_for(&method), "scopeActive");
}

// ── build_scope_methods (attribute) ─────────────────────────────────

#[test]
fn build_scope_methods_attribute_keeps_name() {
    let method = make_scope_attr_method_with_params(
        "active",
        Some("void"),
        vec![make_param(
            "$query",
            Some("\\Illuminate\\Database\\Eloquent\\Builder"),
            true,
        )],
    );

    let [instance, static_m] = build_scope_methods(&method);
    assert_eq!(instance.name, "active");
    assert_eq!(static_m.name, "active");
}

#[test]
fn build_scope_methods_attribute_strips_query_param() {
    let method = make_scope_attr_method_with_params(
        "active",
        Some("void"),
        vec![make_param(
            "$query",
            Some("\\Illuminate\\Database\\Eloquent\\Builder"),
            true,
        )],
    );

    let [instance, static_m] = build_scope_methods(&method);
    assert!(instance.parameters.is_empty());
    assert!(static_m.parameters.is_empty());
}

#[test]
fn build_scope_methods_attribute_preserves_extra_params() {
    let method = make_scope_attr_method_with_params(
        "ofType",
        Some("void"),
        vec![
            make_param(
                "$query",
                Some("\\Illuminate\\Database\\Eloquent\\Builder"),
                true,
            ),
            make_param("$type", Some("string"), true),
        ],
    );

    let [instance, static_m] = build_scope_methods(&method);
    assert_eq!(instance.parameters.len(), 1);
    assert_eq!(instance.parameters[0].name, "$type");
    assert_eq!(static_m.parameters.len(), 1);
}

#[test]
fn build_scope_methods_attribute_default_return_type() {
    let method = make_scope_attr_method("active", None);
    let [instance, static_m] = build_scope_methods(&method);

    assert_eq!(
        instance.return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<static>")
    );
    assert_eq!(
        static_m.return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<static>")
    );
}

#[test]
fn build_scope_methods_attribute_void_defaults() {
    let method = make_scope_attr_method("active", Some("void"));
    let [instance, _] = build_scope_methods(&method);
    assert_eq!(
        instance.return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<static>")
    );
}

#[test]
fn build_scope_methods_attribute_creates_instance_and_static() {
    let method = make_scope_attr_method("active", Some("void"));
    let [instance, static_m] = build_scope_methods(&method);

    assert!(!instance.is_static);
    assert_eq!(instance.visibility, Visibility::Public);
    assert!(static_m.is_static);
    assert_eq!(static_m.visibility, Visibility::Public);
}

#[test]
fn build_scope_methods_attribute_preserves_deprecated() {
    let mut method = make_scope_attr_method("old", Some("void"));
    method.extras_mut().deprecation_message = Some("Use new() instead".into());

    let [instance, static_m] = build_scope_methods(&method);
    assert!(instance.deprecation_message().is_some());
    assert!(static_m.deprecation_message().is_some());
}

// ── build_scope_methods_for_builder ─────────────────────────────────

#[test]
fn builder_scope_returns_empty_when_model_not_found() {
    let methods = build_scope_methods_for_builder("App\\Models\\Missing", &|_| None);
    assert!(methods.is_empty());
}

#[test]
fn builder_scope_returns_empty_for_non_model() {
    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name == "App\\Services\\Foo" {
            Some(Arc::new(make_class("App\\Services\\Foo")))
        } else {
            None
        }
    };
    let methods = build_scope_methods_for_builder("App\\Services\\Foo", &loader);
    assert!(methods.is_empty());
}

#[test]
fn builder_scope_extracts_scope_methods_as_instance() {
    let model_name = "App\\Models\\Brand";
    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name == "App\\Models\\Brand" {
            let mut m = make_class("Brand");
            m.file_namespace = Some(atom("App\\Models"));
            m.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
            m.methods.push(Arc::new(make_method_with_params(
                "scopeActive",
                Some("void"),
                vec![make_param("$query", Some("Builder"), true)],
            )));
            Some(Arc::new(m))
        } else if name == ELOQUENT_MODEL_FQN {
            Some(Arc::new(make_class(ELOQUENT_MODEL_FQN)))
        } else {
            None
        }
    };

    let methods = build_scope_methods_for_builder(model_name, &loader);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "active");
    assert!(!methods[0].is_static);
}

#[test]
fn builder_scope_substitutes_static_in_return_type() {
    let model_name = "App\\Models\\Brand";
    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name == "App\\Models\\Brand" {
            let mut m = make_class("Brand");
            m.file_namespace = Some(atom("App\\Models"));
            m.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
            m.methods.push(Arc::new(make_method_with_params(
                "scopeActive",
                Some("void"),
                vec![make_param("$query", Some("Builder"), true)],
            )));
            Some(Arc::new(m))
        } else if name == ELOQUENT_MODEL_FQN {
            Some(Arc::new(make_class(ELOQUENT_MODEL_FQN)))
        } else {
            None
        }
    };

    let methods = build_scope_methods_for_builder(model_name, &loader);
    assert_eq!(methods.len(), 1);
    // void defaults to Builder<static>, then static → App\Models\Brand
    assert_eq!(
        methods[0].return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<App\\Models\\Brand>")
    );
}

#[test]
fn builder_scope_strips_query_parameter() {
    let model_name = "App\\Models\\Brand";
    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name == "App\\Models\\Brand" {
            let mut m = make_class("Brand");
            m.file_namespace = Some(atom("App\\Models"));
            m.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
            m.methods.push(Arc::new(make_method_with_params(
                "scopeOfType",
                Some("void"),
                vec![
                    make_param("$query", Some("Builder"), true),
                    make_param("$type", Some("string"), true),
                ],
            )));
            Some(Arc::new(m))
        } else if name == ELOQUENT_MODEL_FQN {
            Some(Arc::new(make_class(ELOQUENT_MODEL_FQN)))
        } else {
            None
        }
    };

    let methods = build_scope_methods_for_builder(model_name, &loader);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].parameters.len(), 1);
    assert_eq!(methods[0].parameters[0].name, "$type");
}

#[test]
fn builder_scope_with_custom_return_type() {
    let model_name = "App\\Models\\Brand";
    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name == "App\\Models\\Brand" {
            let mut m = make_class("Brand");
            m.file_namespace = Some(atom("App\\Models"));
            m.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
            m.methods.push(Arc::new(make_method_with_params(
                "scopeActive",
                Some("\\App\\Builders\\BrandBuilder"),
                vec![make_param("$query", Some("Builder"), true)],
            )));
            Some(Arc::new(m))
        } else if name == ELOQUENT_MODEL_FQN {
            Some(Arc::new(make_class(ELOQUENT_MODEL_FQN)))
        } else {
            None
        }
    };

    let methods = build_scope_methods_for_builder(model_name, &loader);
    assert_eq!(methods.len(), 1);
    assert_eq!(
        methods[0].return_type_str().as_deref(),
        Some("\\App\\Builders\\BrandBuilder")
    );
}

#[test]
fn builder_scope_preserves_deprecated() {
    let model_name = "App\\Models\\Brand";
    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name == "App\\Models\\Brand" {
            let mut m = make_class("Brand");
            m.file_namespace = Some(atom("App\\Models"));
            m.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
            let mut scope = make_method("scopeOld", Some("void"));
            scope.extras_mut().deprecation_message = Some("Use scopeNew() instead".into());
            m.methods.push(Arc::new(scope));
            Some(Arc::new(m))
        } else if name == ELOQUENT_MODEL_FQN {
            Some(Arc::new(make_class(ELOQUENT_MODEL_FQN)))
        } else {
            None
        }
    };

    let methods = build_scope_methods_for_builder(model_name, &loader);
    assert_eq!(methods.len(), 1);
    assert!(methods[0].deprecation_message().is_some());
}

// ── #[Scope] attribute: build_scope_methods_for_builder ─────────────

#[test]
fn builder_scope_attribute_extracts_scope_methods_as_instance() {
    let model_name = "App\\Models\\Brand";
    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name == "App\\Models\\Brand" {
            let mut m = make_class("Brand");
            m.file_namespace = Some(atom("App\\Models"));
            m.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
            m.methods.push(Arc::new(make_scope_attr_method_with_params(
                "active",
                Some("void"),
                vec![make_param("$query", Some("Builder"), true)],
            )));
            Some(Arc::new(m))
        } else if name == ELOQUENT_MODEL_FQN {
            Some(Arc::new(make_class(ELOQUENT_MODEL_FQN)))
        } else {
            None
        }
    };

    let methods = build_scope_methods_for_builder(model_name, &loader);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "active");
    assert!(!methods[0].is_static);
}

#[test]
fn builder_scope_attribute_strips_query_parameter() {
    let model_name = "App\\Models\\Brand";
    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name == "App\\Models\\Brand" {
            let mut m = make_class("Brand");
            m.file_namespace = Some(atom("App\\Models"));
            m.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
            m.methods.push(Arc::new(make_scope_attr_method_with_params(
                "ofType",
                Some("void"),
                vec![
                    make_param("$query", Some("Builder"), true),
                    make_param("$type", Some("string"), true),
                ],
            )));
            Some(Arc::new(m))
        } else if name == ELOQUENT_MODEL_FQN {
            Some(Arc::new(make_class(ELOQUENT_MODEL_FQN)))
        } else {
            None
        }
    };

    let methods = build_scope_methods_for_builder(model_name, &loader);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].parameters.len(), 1);
    assert_eq!(methods[0].parameters[0].name, "$type");
}

#[test]
fn builder_scope_attribute_substitutes_static_in_return_type() {
    let model_name = "App\\Models\\Brand";
    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name == "App\\Models\\Brand" {
            let mut m = make_class("Brand");
            m.file_namespace = Some(atom("App\\Models"));
            m.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
            m.methods.push(Arc::new(make_scope_attr_method_with_params(
                "active",
                Some("void"),
                vec![make_param("$query", Some("Builder"), true)],
            )));
            Some(Arc::new(m))
        } else if name == ELOQUENT_MODEL_FQN {
            Some(Arc::new(make_class(ELOQUENT_MODEL_FQN)))
        } else {
            None
        }
    };

    let methods = build_scope_methods_for_builder(model_name, &loader);
    assert_eq!(methods.len(), 1);
    // void defaults to Builder<static>, then static → App\Models\Brand
    assert_eq!(
        methods[0].return_type_str().as_deref(),
        Some("Illuminate\\Database\\Eloquent\\Builder<App\\Models\\Brand>")
    );
}
