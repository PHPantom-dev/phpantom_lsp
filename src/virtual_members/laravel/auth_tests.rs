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
    PhpType::named(atom(fqn))
}

fn contract() -> PhpType {
    PhpType::named(atom(AUTHENTICATABLE_FQN))
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
        Some(PhpType::union(vec![named("App\\Models\\User"), contract()]))
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
        Some(PhpType::union(vec![
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
        Some(PhpType::union(vec![named("App\\Models\\User"), contract()]))
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
        Some(PhpType::union(vec![
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
        Some(PhpType::union(vec![
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
        Some(PhpType::union(vec![
            named("App\\Models\\User"),
            named("App\\Models\\Admin"),
        ]))
    );
}

/// A backend whose `config/auth.php` names `App\Models\User`, kept alive by the
/// returned directory, and a loader that knows that one class.
fn backend_configuring_user_model() -> (
    Backend,
    tempfile::TempDir,
    impl Fn(Fqn<'_>) -> Option<Arc<ClassInfo>> + Sync,
) {
    backend_with_auth_model(r"'App\Models\User'")
}

/// A backend whose `config/auth.php` gives `model_expr` as the model, kept
/// alive by the returned directory, and a loader that knows `App\Models\User`.
fn backend_with_auth_model(
    model_expr: &str,
) -> (
    Backend,
    tempfile::TempDir,
    impl Fn(Fqn<'_>) -> Option<Arc<ClassInfo>> + Sync,
) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("config")).unwrap();
    std::fs::write(
        dir.path().join("config/auth.php"),
        format!(
            "<?php return [
                'defaults' => ['guard' => 'web'],
                'guards' => ['web' => ['provider' => 'users']],
                'providers' => ['users' => ['model' => {model_expr}]],
            ];"
        ),
    )
    .unwrap();
    let backend = crate::test_fixtures::make_backend();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());

    let mut user = crate::test_fixtures::make_class("User");
    user.file_namespace = Some(atom("App\\Models"));
    user.interfaces.push(atom(AUTHENTICATABLE_FQN));
    let user = Arc::new(user);
    let load_user =
        move |name: Fqn<'_>| (name.as_str() == "App\\Models\\User").then(|| user.clone());
    (backend, dir, load_user)
}

/// A thread that asks for the auth model while another is computing it gets
/// the model.  Seeing "no model" here would leave `user()` unrefined in
/// whatever class that thread caches, depending only on thread timing.
#[test]
fn a_thread_asking_during_the_computation_gets_the_model() {
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicBool, Ordering};

    let (backend, _dir, load_user) = backend_configuring_user_model();

    let computing = Barrier::new(2);
    let paused = AtomicBool::new(false);
    let pausing_loader = |name: Fqn<'_>| {
        if !paused.swap(true, Ordering::Relaxed) {
            computing.wait();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        load_user(name)
    };

    let during = std::thread::scope(|scope| {
        scope.spawn(|| resolve_auth_user_type(&backend, None, &pausing_loader));
        computing.wait();
        resolve_auth_user_type(&backend, None, &load_user)
    });

    assert_eq!(during, Some(named("App\\Models\\User")));
}

/// A thread that asks while another is computing the model does not wait for
/// that computation.  The computing thread's loads can wait on whatever the
/// asking thread holds (the workspace index pass, a class resolution claim),
/// so waiting here could deadlock both.
#[test]
fn a_thread_asking_during_the_computation_does_not_wait_for_it() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Barrier, Mutex, mpsc};

    let (backend, _dir, load_user) = backend_configuring_user_model();

    let computing = Barrier::new(2);
    let paused = AtomicBool::new(false);
    let (answered_tx, answered_rx) = mpsc::channel::<()>();
    let answered_rx = Mutex::new(answered_rx);
    let waiting_loader = |name: Fqn<'_>| {
        if !paused.swap(true, Ordering::Relaxed) {
            computing.wait();
            // Stands in for a load that waits on something the other thread
            // holds until that thread has its answer.
            let _ = answered_rx
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10));
        }
        load_user(name)
    };

    let (during, computed, answered_in_time) = std::thread::scope(|scope| {
        let computing_thread =
            scope.spawn(|| resolve_auth_user_type(&backend, None, &waiting_loader));
        computing.wait();
        let started = std::time::Instant::now();
        let during = resolve_auth_user_type(&backend, None, &load_user);
        let answered_in_time = started.elapsed() < std::time::Duration::from_secs(5);
        answered_tx.send(()).unwrap();
        (during, computing_thread.join().unwrap(), answered_in_time)
    });

    assert!(answered_in_time, "waited for the other computation");
    assert_eq!(during, Some(named("App\\Models\\User")));
    assert_eq!(computed, during);
}

/// The loads a computation triggers ask for the model too, and must get the
/// raw class rather than recurse.
#[test]
fn the_computing_thread_asking_again_gets_no_model() {
    let (backend, _dir, load_user) = backend_configuring_user_model();

    let during = std::cell::OnceCell::new();
    let reentrant_loader = |name: Fqn<'_>| {
        during.get_or_init(|| resolve_auth_user_type(&backend, None, &load_user));
        load_user(name)
    };

    let computed = resolve_auth_user_type(&backend, None, &reentrant_loader);

    assert_eq!(during.get(), Some(&None));
    assert_eq!(computed, Some(named("App\\Models\\User")));
}

/// A model the config does not pin down widens to the project's implementors
/// without waiting for the workspace index.  The index pass loads the auth
/// entry points itself, so a computation waiting on it never finished.
#[test]
fn the_implementor_floor_does_not_wait_for_the_workspace_index() {
    let (backend, _dir, load_user) = backend_with_auth_model("runtime_model()");
    let indexing = backend.workspace_index.lock.lock();

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let finished = std::thread::scope(|scope| {
        scope.spawn(|| {
            let _ = done_tx.send(resolve_auth_user_type(&backend, None, &load_user));
        });
        let finished = done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .is_ok();
        drop(indexing);
        finished
    });

    assert!(finished, "waited for the workspace index");
}
