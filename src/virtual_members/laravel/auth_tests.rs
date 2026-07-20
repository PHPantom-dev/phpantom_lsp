use super::super::config_values::parse_config_tree;
use super::*;

/// A fake compatibility filter: `User` and `Admin` (short or FQN) are known
/// `Authenticatable` subtypes; anything else is rejected.
fn model_resolver(name: &str) -> Option<String> {
    match name {
        "App\\Models\\User" | "User" => Some("App\\Models\\User".to_string()),
        "App\\Models\\Admin" | "Admin" => Some("App\\Models\\Admin".to_string()),
        _ => None,
    }
}

fn named(fqn: &str) -> PhpType {
    PhpType::Named(fqn.to_string())
}

fn contract() -> PhpType {
    PhpType::Named(AUTHENTICATABLE_FQN.to_string())
}

/// Resolve with no known implementors, so an uncertain branch falls back to
/// the abstract `Authenticatable` contract.
fn resolve(config: &str, guard: Option<&str>) -> Option<PhpType> {
    let tree = parse_config_tree(config).unwrap();
    resolve_auth_user_model(&tree, guard, &model_resolver, &Vec::new)
}

/// Resolve with a fixed set of concrete implementors, so an uncertain branch
/// raises the floor to those classes instead of the abstract contract.
fn resolve_with_impls(config: &str, guard: Option<&str>, impls: &[&str]) -> Option<PhpType> {
    let tree = parse_config_tree(config).unwrap();
    let floor = || impls.iter().map(|s| s.to_string()).collect();
    resolve_auth_user_model(&tree, guard, &model_resolver, &floor)
}

/// A fresh single-guard install: `env('AUTH_MODEL', User::class)` → the model
/// is env-overridable, so we anchor on `User` but widen to the contract.
#[test]
fn single_guard_env_model_widens_to_contract() {
    let config = r#"<?php return [
        'defaults' => ['guard' => 'web'],
        'guards' => ['web' => ['provider' => 'users']],
        'providers' => ['users' => ['model' => env('AUTH_MODEL', App\Models\User::class)]],
    ];"#;
    assert_eq!(
        resolve(config, None),
        Some(PhpType::Union(vec![named("App\\Models\\User"), contract()]))
    );
}

/// A hard-literal model with a hard guard and provider: fully certain, so no
/// contract floor is added.
#[test]
fn hard_literal_model_is_precise() {
    let config = r#"<?php return [
        'defaults' => ['guard' => 'web'],
        'guards' => ['web' => ['provider' => 'users']],
        'providers' => ['users' => ['model' => App\Models\User::class]],
    ];"#;
    assert_eq!(resolve(config, None), Some(named("App\\Models\\User")));
}

/// An env-overridable default guard fans out to every configured guard, and
/// widens to the contract because the guard choice is uncertain.
#[test]
fn env_default_guard_fans_out_all_guards() {
    let config = r#"<?php return [
        'defaults' => ['guard' => env('AUTH_GUARD', 'web')],
        'guards' => [
            'web' => ['provider' => 'users'],
            'api' => ['provider' => 'admins'],
        ],
        'providers' => [
            'users' => ['model' => App\Models\User::class],
            'admins' => ['model' => App\Models\Admin::class],
        ],
    ];"#;
    assert_eq!(
        resolve(config, None),
        Some(PhpType::Union(vec![
            named("App\\Models\\User"),
            named("App\\Models\\Admin"),
            contract(),
        ]))
    );
}

/// An explicit guard argument with a hard provider and model resolves to a
/// single precise type with no floor.
#[test]
fn explicit_guard_is_precise() {
    let config = r#"<?php return [
        'defaults' => ['guard' => env('AUTH_GUARD', 'web')],
        'guards' => [
            'web' => ['provider' => 'users'],
            'api' => ['provider' => 'admins'],
        ],
        'providers' => [
            'users' => ['model' => App\Models\User::class],
            'admins' => ['model' => App\Models\Admin::class],
        ],
    ];"#;
    assert_eq!(
        resolve(config, Some("api")),
        Some(named("App\\Models\\Admin"))
    );
}

/// An explicit guard whose model is env-overridable widens to the contract.
#[test]
fn explicit_guard_with_env_model_widens() {
    let config = r#"<?php return [
        'guards' => ['web' => ['provider' => 'users']],
        'providers' => ['users' => ['model' => env('AUTH_MODEL', App\Models\User::class)]],
    ];"#;
    assert_eq!(
        resolve(config, Some("web")),
        Some(PhpType::Union(vec![named("App\\Models\\User"), contract()]))
    );
}

/// A ternary of two literal models with an otherwise-hard chain resolves to
/// both, with no floor (exhaustive over literals).
#[test]
fn ternary_model_is_exhaustive() {
    let config = r#"<?php return [
        'defaults' => ['guard' => 'web'],
        'guards' => ['web' => ['provider' => 'users']],
        'providers' => ['users' => ['model' => something() ? App\Models\User::class : App\Models\Admin::class]],
    ];"#;
    assert_eq!(
        resolve(config, None),
        Some(PhpType::Union(vec![
            named("App\\Models\\User"),
            named("App\\Models\\Admin"),
        ]))
    );
}

/// A model value that is not an Authenticatable subtype is dropped by the
/// compatibility filter; with nothing left, resolution is untouched.
#[test]
fn non_model_value_is_dropped() {
    let config = r#"<?php return [
        'defaults' => ['guard' => 'web'],
        'guards' => ['web' => ['provider' => 'users']],
        'providers' => ['users' => ['model' => App\Support\NotAModel::class]],
    ];"#;
    assert_eq!(resolve(config, None), None);
}

/// A guard argument that does not exist in the config resolves nothing.
#[test]
fn unknown_guard_argument_resolves_nothing() {
    let config = r#"<?php return [
        'guards' => ['web' => ['provider' => 'users']],
        'providers' => ['users' => ['model' => App\Models\User::class]],
    ];"#;
    assert_eq!(resolve(config, Some("nonexistent")), None);
}

/// A bare `env()` model with no default is fully dynamic; nothing concrete
/// survives, so the declared contract is kept.
#[test]
fn bare_env_model_keeps_contract() {
    let config = r#"<?php return [
        'defaults' => ['guard' => 'web'],
        'guards' => ['web' => ['provider' => 'users']],
        'providers' => ['users' => ['model' => env('AUTH_MODEL')]],
    ];"#;
    assert_eq!(resolve(config, None), None);
}

/// With one concrete implementor, an uncertain branch raises the floor to that
/// class — the abstract contract collapses away and the union becomes the
/// single model.
#[test]
fn floor_raises_to_sole_implementor() {
    let config = r#"<?php return [
        'defaults' => ['guard' => 'web'],
        'guards' => ['web' => ['provider' => 'users']],
        'providers' => ['users' => ['model' => env('AUTH_MODEL', App\Models\User::class)]],
    ];"#;
    assert_eq!(
        resolve_with_impls(config, None, &["App\\Models\\User"]),
        Some(named("App\\Models\\User"))
    );
}

/// With several implementors, the floor raises to all of them (config-derived
/// model first), and the abstract contract does not appear.
#[test]
fn floor_raises_to_all_implementors() {
    let config = r#"<?php return [
        'defaults' => ['guard' => 'web'],
        'guards' => ['web' => ['provider' => 'users']],
        'providers' => ['users' => ['model' => env('AUTH_MODEL', App\Models\User::class)]],
    ];"#;
    assert_eq!(
        resolve_with_impls(config, None, &["App\\Models\\User", "App\\Models\\Admin"]),
        Some(PhpType::Union(vec![
            named("App\\Models\\User"),
            named("App\\Models\\Admin"),
        ]))
    );
}

/// When the config is fully unresolvable but implementors exist, the result is
/// the union of every implementor — the honest "one of these" floor.
#[test]
fn unresolvable_config_falls_back_to_implementors() {
    let config = r#"<?php return [
        'defaults' => ['guard' => 'web'],
        'guards' => ['web' => ['provider' => 'users']],
        'providers' => ['users' => ['model' => env('AUTH_MODEL')]],
    ];"#;
    assert_eq!(
        resolve_with_impls(config, None, &["App\\Models\\User", "App\\Models\\Admin"]),
        Some(PhpType::Union(vec![
            named("App\\Models\\User"),
            named("App\\Models\\Admin"),
        ]))
    );
}
