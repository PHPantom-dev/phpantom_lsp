use super::*;

/// The current default Laravel `config/auth.php`, trimmed to the parts the
/// auth-user model resolver traverses.
const DEFAULT_AUTH_CONFIG: &str = r#"<?php
return [
    'defaults' => [
        'guard' => env('AUTH_GUARD', 'web'),
    ],
    'guards' => [
        'web' => [
            'driver' => 'session',
            'provider' => 'users',
        ],
        'api' => [
            'driver' => 'token',
            'provider' => 'admins',
        ],
    ],
    'providers' => [
        'users' => [
            'driver' => 'eloquent',
            'model' => env('AUTH_MODEL', App\Models\User::class),
        ],
        'admins' => [
            'driver' => 'eloquent',
            'model' => App\Models\Admin::class,
        ],
    ],
];
"#;

#[test]
fn navigates_scalar_string() {
    let tree = parse_config_tree("<?php return ['a' => ['b' => 'hello']];").unwrap();
    assert_eq!(
        tree.value_at(&["a", "b"]),
        Some(&ConfigValue::Str("hello".to_string()))
    );
}

#[test]
fn classifies_fqn_class_constant() {
    let tree = parse_config_tree("<?php return ['model' => App\\Models\\User::class];").unwrap();
    assert_eq!(
        tree.value_at(&["model"]),
        Some(&ConfigValue::ClassString("App\\Models\\User".to_string()))
    );
}

#[test]
fn classifies_short_class_constant() {
    let tree = parse_config_tree("<?php return ['model' => User::class];").unwrap();
    assert_eq!(
        tree.value_at(&["model"]),
        Some(&ConfigValue::ClassString("User".to_string()))
    );
}

#[test]
fn resolves_short_class_constant_via_use_statement() {
    let tree = parse_config_tree(
        "<?php use App\\Models\\AdminUser; return ['model' => AdminUser::class];",
    )
    .unwrap();
    assert_eq!(
        tree.value_at(&["model"]),
        Some(&ConfigValue::ClassString(
            "App\\Models\\AdminUser".to_string()
        ))
    );
}

#[test]
fn resolves_aliased_class_constant_via_use_statement() {
    let tree = parse_config_tree(
        "<?php use App\\Models\\AdminUser as Model; return ['model' => Model::class];",
    )
    .unwrap();
    assert_eq!(
        tree.value_at(&["model"]),
        Some(&ConfigValue::ClassString(
            "App\\Models\\AdminUser".to_string()
        ))
    );
}

#[test]
fn resolves_imported_prefix_in_class_constant() {
    let tree =
        parse_config_tree("<?php use App\\Models; return ['model' => Models\\AdminUser::class];")
            .unwrap();
    assert_eq!(
        tree.value_at(&["model"]),
        Some(&ConfigValue::ClassString(
            "App\\Models\\AdminUser".to_string()
        ))
    );
}

#[test]
fn leading_backslash_class_constant_ignores_imports() {
    let tree =
        parse_config_tree("<?php use Other\\User; return ['model' => \\App\\Models\\User::class];")
            .unwrap();
    assert_eq!(
        tree.value_at(&["model"]),
        Some(&ConfigValue::ClassString("App\\Models\\User".to_string()))
    );
}

#[test]
fn classifies_env_with_class_default() {
    let tree =
        parse_config_tree("<?php return ['model' => env('AUTH_MODEL', App\\Models\\User::class)];")
            .unwrap();
    let value = tree.value_at(&["model"]).unwrap();
    assert_eq!(
        value,
        &ConfigValue::EnvDefault(Box::new(ConfigValue::ClassString(
            "App\\Models\\User".to_string()
        )))
    );
    // Anchors on the default, but records the possible override.
    let (classes, dynamic) = value.as_classes();
    assert_eq!(classes, vec!["App\\Models\\User".to_string()]);
    assert!(dynamic);
}

#[test]
fn bare_env_is_dynamic() {
    let tree = parse_config_tree("<?php return ['model' => env('AUTH_MODEL')];").unwrap();
    assert_eq!(tree.value_at(&["model"]), Some(&ConfigValue::Dynamic));
    let (classes, dynamic) = tree.value_at(&["model"]).unwrap().as_classes();
    assert!(classes.is_empty());
    assert!(dynamic);
}

#[test]
fn ternary_of_class_constants_is_one_of() {
    let tree = parse_config_tree(
        "<?php return ['model' => env('is_admin') ? User::class : Admin::class];",
    )
    .unwrap();
    let value = tree.value_at(&["model"]).unwrap();
    // Both arms are literals; the condition is irrelevant.
    let (classes, dynamic) = value.as_classes();
    assert_eq!(classes, vec!["User".to_string(), "Admin".to_string()]);
    // Exhaustive over literals — no runtime-unknowable branch.
    assert!(!dynamic);
}

#[test]
fn short_ternary_uses_condition_as_then() {
    let tree = parse_config_tree("<?php return ['model' => User::class ?: Admin::class];").unwrap();
    let (classes, _) = tree.value_at(&["model"]).unwrap().as_classes();
    assert_eq!(classes, vec!["User".to_string(), "Admin".to_string()]);
}

#[test]
fn as_strings_flattens_and_flags_env_override() {
    let value = ConfigValue::EnvDefault(Box::new(ConfigValue::Str("web".to_string())));
    let (strings, dynamic) = value.as_strings();
    assert_eq!(strings, vec!["web".to_string()]);
    assert!(dynamic);
}

#[test]
fn child_keys_enumerates_for_fan_out() {
    let tree = parse_config_tree(DEFAULT_AUTH_CONFIG).unwrap();
    let guards = tree.get(&["guards"]).unwrap().child_keys();
    assert_eq!(guards, vec!["web".to_string(), "api".to_string()]);
    let providers = tree.get(&["providers"]).unwrap().child_keys();
    assert_eq!(providers, vec!["users".to_string(), "admins".to_string()]);
}

#[test]
fn default_laravel_auth_config_traversal() {
    let tree = parse_config_tree(DEFAULT_AUTH_CONFIG).unwrap();

    // defaults.guard → env('AUTH_GUARD', 'web')
    let (guards, guard_dynamic) = tree.value_at(&["defaults", "guard"]).unwrap().as_strings();
    assert_eq!(guards, vec!["web".to_string()]);
    assert!(guard_dynamic);

    // guards.web.provider → 'users'
    let (providers, provider_dynamic) = tree
        .value_at(&["guards", "web", "provider"])
        .unwrap()
        .as_strings();
    assert_eq!(providers, vec!["users".to_string()]);
    assert!(!provider_dynamic);

    // providers.users.model → env('AUTH_MODEL', App\Models\User::class)
    let (models, model_dynamic) = tree
        .value_at(&["providers", "users", "model"])
        .unwrap()
        .as_classes();
    assert_eq!(models, vec!["App\\Models\\User".to_string()]);
    assert!(model_dynamic);

    // The api guard's provider resolves to a hard literal model.
    let (admin_model, admin_dynamic) = tree
        .value_at(&["providers", "admins", "model"])
        .unwrap()
        .as_classes();
    assert_eq!(admin_model, vec!["App\\Models\\Admin".to_string()]);
    assert!(!admin_dynamic);
}

#[test]
fn variable_return_pattern() {
    let content = "<?php\n$config = ['model' => User::class];\nreturn $config;";
    let tree = parse_config_tree(content).unwrap();
    assert_eq!(
        tree.value_at(&["model"]),
        Some(&ConfigValue::ClassString("User".to_string()))
    );
}

#[test]
fn missing_path_returns_none() {
    let tree = parse_config_tree(DEFAULT_AUTH_CONFIG).unwrap();
    assert_eq!(tree.value_at(&["providers", "nope", "model"]), None);
    assert_eq!(tree.get(&["guards", "web", "provider", "deeper"]), None);
}

#[test]
fn a_keyless_array_is_a_list_whose_positions_are_not_keys() {
    let tree = parse_config_tree("<?php return ['handlers' => [A::class, B::class]];").unwrap();
    assert!(matches!(tree.get(&["handlers"]), Some(ConfigNode::List(items)) if items.len() == 2));
    let mut keys = Vec::new();
    tree.collect_keys("pipeline", &mut keys);
    assert_eq!(keys, vec!["pipeline.handlers".to_string()]);
}

#[test]
fn a_duplicate_key_keeps_the_last_value() {
    let tree = parse_config_tree("<?php return ['a' => 1, 'a' => 'two'];").unwrap();
    assert_eq!(
        tree.value_at(&["a"]),
        Some(&ConfigValue::Str("two".to_string()))
    );
}

#[test]
fn array_merge_lets_a_later_argument_replace_a_key_whole() {
    let content = "<?php return array_merge(['a' => ['x' => 1], 'b' => 1], ['a' => ['y' => 2]]);";
    let tree = parse_config_tree(content).unwrap();
    let mut keys = Vec::new();
    tree.collect_keys("app", &mut keys);
    keys.sort();
    assert_eq!(keys, vec!["app.a", "app.a.y", "app.b"]);
}

#[test]
fn a_group_merged_beneath_is_replaced_whole_unless_it_is_mergeable() {
    let mut app =
        parse_config_tree("<?php return ['connections' => ['mysql' => ['port' => 1]], 'redis' => ['client' => 'predis']];")
            .unwrap();
    let framework = parse_config_tree(
        "<?php return ['default' => 'sqlite', 'connections' => ['sqlite' => [], 'mysql' => ['host' => 'h']], 'redis' => ['options' => []]];",
    )
    .unwrap();
    app.merge_beneath(framework, &["connections"]);
    let mut keys = Vec::new();
    app.collect_keys("database", &mut keys);
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "database.connections",
            "database.connections.mysql",
            "database.connections.mysql.port",
            "database.connections.sqlite",
            "database.default",
            "database.redis",
            "database.redis.client",
        ]
    );
}
