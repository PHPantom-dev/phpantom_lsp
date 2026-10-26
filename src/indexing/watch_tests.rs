use super::*;

#[test]
fn non_laravel_projects_ignore_schema_watch_changes() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("database/schema/default-schema.sql");
    std::fs::create_dir_all(schema.parent().unwrap()).unwrap();
    std::fs::write(&schema, "CREATE TABLE users (id bigint);").unwrap();

    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Uri::from_file_path(&schema).unwrap(),
            kind: FileChangeType::Created,
        }],
    };

    let backend = Backend::new_test();
    backend
        .resolved_class_cache
        .write()
        .laravel_mut()
        .set_enabled(false);
    assert!(!backend.apply_watched_file_changes(&params, dir.path()));

    // The gate is on the project type alone: the same event in a
    // Laravel workspace still rebuilds the schema index.
    let backend = Backend::new_test();
    backend
        .resolved_class_cache
        .write()
        .laravel_mut()
        .set_enabled(true);
    assert!(backend.apply_watched_file_changes(&params, dir.path()));
}

/// Seed one entry into the resolved-class cache so a test can tell a
/// targeted invalidation from a wholesale clear.
fn seed_resolved_class(backend: &Backend) {
    backend.update_ast("file:///seed.php", "<?php class Seeded {}");
    let info = backend
        .symbols
        .fqn_class_index
        .read()
        .get("Seeded")
        .cloned()
        .expect("the seed class should be indexed");
    backend
        .resolved_class_cache
        .write()
        .insert((crate::atom::Atom::from("Seeded"), Vec::new()), info);
}

fn created_event(path: &std::path::Path) -> DidChangeWatchedFilesParams {
    DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Uri::from_file_path(path).unwrap(),
            kind: FileChangeType::Created,
        }],
    }
}

/// A template's lowered PHP and source map are recorded for every
/// indexed template, not only the open ones, so a watched deletion has
/// to drop them the way `did_close` does.
#[test]
fn deleting_a_template_drops_its_lowered_php_and_source_map() {
    let dir = tempfile::tempdir().unwrap();
    let template = dir.path().join("resources/views/welcome.blade.php");
    let url = Uri::from_file_path(&template).unwrap();
    let uri = url.to_string();

    let backend = Backend::new_test();
    backend.update_ast(&uri, "<div>{{ $name }}</div>\n");
    assert!(backend.blade_virtual_php_arc(&uri).is_some());
    assert!(backend.blade_source_map(&uri).is_some());

    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: url,
            kind: FileChangeType::Deleted,
        }],
    };
    backend.apply_watched_file_changes(&params, dir.path());

    assert!(backend.blade_virtual_php_arc(&uri).is_none());
    assert!(backend.blade_source_map(&uri).is_none());
}

/// The registrations a provider contributes are replaced when the file
/// is parsed, which a file deleted from disk never is again, so the
/// deletion itself has to take them away.
#[test]
fn deleting_a_provider_drops_its_gate_and_macro_registrations() {
    let dir = tempfile::tempdir().unwrap();
    let provider = dir.path().join("app/Providers/AuthServiceProvider.php");
    let url = Uri::from_file_path(&provider).unwrap();
    let uri = url.to_string();

    let backend = Backend::new_test();
    backend
        .resolved_class_cache
        .write()
        .laravel_mut()
        .set_enabled(true);
    backend.update_ast(
        &uri,
        "<?php\n\
         namespace App\\Providers;\n\
         use Illuminate\\Support\\Facades\\Gate;\n\
         use Illuminate\\Support\\Str;\n\
         class AuthServiceProvider {\n\
             public function boot(): void {\n\
                 Gate::define('edit-post', fn ($user) => true);\n\
                 Str::macro('shout', fn (string $s): string => strtoupper($s));\n\
             }\n\
         }\n",
    );
    assert!(
        backend
            .laravel
            .gates
            .read()
            .definition("edit-post")
            .is_some()
    );
    assert!(backend.laravel.macros.read().files.has_uri(&uri));

    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: url,
            kind: FileChangeType::Deleted,
        }],
    };
    backend.apply_watched_file_changes(&params, dir.path());

    assert!(
        backend
            .laravel
            .gates
            .read()
            .definition("edit-post")
            .is_none()
    );
    assert!(!backend.laravel.gates.read().files.has_uri(&uri));
    assert!(!backend.laravel.macros.read().files.has_uri(&uri));
}

/// A running Laravel application compiles Blade templates into
/// storage/framework/views and rewrites the bootstrap/cache manifests
/// on every request it serves; those events must not reach the
/// indexes at all, or editor responsiveness is chained to the
/// application's request traffic.
#[test]
fn laravel_cache_artifact_events_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let view = dir.path().join("storage/framework/views/ab12cd.php");
    let manifest = dir.path().join("bootstrap/cache/packages.php");
    for path in [&view, &manifest] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "<?php class FromArtifact {}").unwrap();
    }

    let backend = Backend::new_test();
    backend
        .resolved_class_cache
        .write()
        .laravel_mut()
        .set_enabled(true);
    seed_resolved_class(&backend);

    for path in [&view, &manifest] {
        assert!(
            !backend.apply_watched_file_changes(&created_event(path), dir.path()),
            "an event under {} should be dropped entirely",
            path.display()
        );
    }
    assert_eq!(
        backend.resolved_class_cache.read().len(),
        1,
        "artifact events must not disturb the resolved-class cache"
    );
}

/// A watched file with no declarations in it (a generated artifact, a
/// routes file) cannot change what any class resolves to, so the
/// resolved-class cache survives its events instead of being cleared
/// and re-derived from scratch on the next completion.
#[test]
fn declaration_free_events_keep_the_resolved_class_cache() {
    let dir = tempfile::tempdir().unwrap();
    let routes = dir.path().join("routes/web.php");
    std::fs::create_dir_all(routes.parent().unwrap()).unwrap();
    std::fs::write(&routes, "<?php echo 'no declarations here';").unwrap();

    let backend = Backend::new_test();
    seed_resolved_class(&backend);

    assert!(
        backend.apply_watched_file_changes(&created_event(&routes), dir.path()),
        "the file is still reindexed and refreshes are still requested"
    );
    assert_eq!(
        backend.resolved_class_cache.read().len(),
        1,
        "a declaration-free batch must not clear the resolved-class cache"
    );
}

/// The counterpart: a file that does declare a class keeps the
/// conservative full clear, since anything already resolved may have
/// been waiting on (or referencing) that class.
#[test]
fn class_declaring_events_still_clear_the_resolved_class_cache() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("app/NewModel.php");
    std::fs::create_dir_all(model.parent().unwrap()).unwrap();
    std::fs::write(&model, "<?php namespace App; class NewModel {}").unwrap();

    let backend = Backend::new_test();
    seed_resolved_class(&backend);

    assert!(backend.apply_watched_file_changes(&created_event(&model), dir.path()));
    assert_eq!(
        backend.resolved_class_cache.read().len(),
        0,
        "a new class declaration must invalidate resolved classes"
    );
}

/// A function-only file written to disk declares no class, so the
/// class caches survive, but a function looked up and missed before
/// the file existed must resolve afterwards.
#[test]
fn function_only_file_events_retire_function_misses() {
    let dir = tempfile::tempdir().unwrap();
    let helpers = dir.path().join("helpers.php");

    let backend = Backend::new_test();
    assert!(backend.find_or_load_function(&["myHelper"]).is_none());

    std::fs::write(&helpers, "<?php function myHelper() {}").unwrap();
    backend.apply_watched_file_changes(&created_event(&helpers), dir.path());

    assert!(
        backend.find_or_load_function(&["myHelper"]).is_some(),
        "the recorded miss must not outlive the declaring file's creation"
    );
}

/// Laravel config files declare nothing but feed class resolution
/// (aliases, the auth model, storage disks), so a change under a
/// `config` directory keeps the conservative full clear.
#[test]
fn config_file_events_still_clear_the_resolved_class_cache() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config/app.php");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "<?php return ['aliases' => []];").unwrap();

    let backend = Backend::new_test();
    seed_resolved_class(&backend);

    assert!(backend.apply_watched_file_changes(&created_event(&config), dir.path()));
    assert_eq!(
        backend.resolved_class_cache.read().len(),
        0,
        "a config change must invalidate resolved classes"
    );
}

/// Deleting one of two files that declare the same class hands the
/// name to the surviving file.  The purge used to drop every index
/// entry the deleted file owned, so a class shipped in two variants
/// behind a `class_exists` guard became unresolvable.
#[test]
fn deleting_one_of_two_declaring_files_keeps_the_class() {
    let backend = Backend::new_test();

    backend.update_ast(
        "file:///a_rich.php",
        "<?php namespace Vendor; class Variant { public function rich() {} }",
    );
    backend.update_ast(
        "file:///b_bare.php",
        "<?php namespace Vendor; class Variant { public function bare() {} }",
    );

    backend.reindex_files_batch(&[(
        "file:///a_rich.php".to_string(),
        PathBuf::from("/a_rich.php"),
        FileChangeType::Deleted,
    )]);

    assert_eq!(
        backend
            .symbols
            .fqn_uri_index
            .read()
            .get("Vendor\\Variant")
            .cloned(),
        Some("file:///b_bare.php".to_string()),
        "the surviving file should take over the name"
    );
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .get("Vendor\\Variant")
            .is_some_and(|cls| cls.methods.iter().any(|m| m.name == "bare")),
        "the class index must describe the surviving declaration"
    );
}

/// ...and the name still goes away once the last declaring file is
/// deleted.
#[test]
fn deleting_the_last_declaring_file_drops_the_class() {
    let backend = Backend::new_test();

    backend.update_ast(
        "file:///a_rich.php",
        "<?php namespace Vendor; class Variant { public function rich() {} }",
    );
    backend.update_ast(
        "file:///b_bare.php",
        "<?php namespace Vendor; class Variant { public function bare() {} }",
    );

    backend.reindex_files_batch(&[
        (
            "file:///a_rich.php".to_string(),
            PathBuf::from("/a_rich.php"),
            FileChangeType::Deleted,
        ),
        (
            "file:///b_bare.php".to_string(),
            PathBuf::from("/b_bare.php"),
            FileChangeType::Deleted,
        ),
    ]);

    assert!(
        backend
            .symbols
            .fqn_uri_index
            .read()
            .get("Vendor\\Variant")
            .is_none(),
        "no file declares Vendor\\Variant any more"
    );
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .get("Vendor\\Variant")
            .is_none(),
        "the class index must not outlive the last declaration"
    );
}

/// A non-Laravel project used to never reload its own `.phpantom.toml`
/// on change: the reload was piggybacked on the Laravel schema watcher,
/// which only fires for Laravel projects.
#[test]
fn non_laravel_project_reloads_its_own_config_on_change() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(crate::config::CONFIG_FILE_NAME);
    std::fs::write(&config_path, "[diagnostics]\nextra-arguments = true\n").unwrap();

    let backend = Backend::new_test();
    backend
        .resolved_class_cache
        .write()
        .laravel_mut()
        .set_enabled(false);
    assert!(!backend.config().diagnostics.extra_arguments_enabled());

    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Uri::from_file_path(&config_path).unwrap(),
            kind: FileChangeType::Changed,
        }],
    };
    assert!(backend.apply_watched_file_changes(&params, dir.path()));
    assert!(backend.config().diagnostics.extra_arguments_enabled());
}

/// `reload_config` writes through the `Arc<Mutex<Config>>` shared by
/// every `Backend` clone (the blocking-task and background-worker
/// clones created via `clone_for_blocking`), not just the
/// clone it was called on. Before `config` moved behind an `Arc`, a
/// reload performed on one of those clones (as every reload always is)
/// was invisible to the original `Backend` answering LSP requests.
#[test]
fn reload_config_is_visible_on_every_clone() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(crate::config::CONFIG_FILE_NAME),
        "[diagnostics]\nextra-arguments = true\n",
    )
    .unwrap();

    let backend = Backend::new_test();
    let clone = backend.clone_for_blocking();
    assert!(!backend.config().diagnostics.extra_arguments_enabled());

    clone.reload_config(dir.path());

    assert!(
        backend.config().diagnostics.extra_arguments_enabled(),
        "a reload on a clone must be visible on the original Backend"
    );
}

/// Switching diagnostics off in the config clears what open files were
/// shown, and switching them back on fills it in again, both without
/// waiting for the next edit: the reload queues every open file for the
/// diagnostic worker, with a full pass owed.
#[test]
fn reload_config_rediagnoses_open_files_when_diagnostics_are_toggled() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(crate::config::CONFIG_FILE_NAME);
    let backend = Backend::new_test();
    backend.init_complete.store(true, Ordering::Release);
    let uri = "file:///project/src/Open.php";
    backend
        .open_files
        .write()
        .insert(uri.to_string(), std::sync::Arc::new("<?php\n".to_string()));

    std::fs::write(&config_path, "[diagnostics]\nenabled = false\n").unwrap();
    backend.reload_config(dir.path());
    assert!(backend.diag.pending_uris.lock().contains(uri));
    assert!(backend.diag.full_slow_pass_requested.lock().contains(uri));

    backend.diag.pending_uris.lock().clear();
    backend.diag.full_slow_pass_requested.lock().clear();
    std::fs::write(
        &config_path,
        "[diagnostics]\nenabled = false\nworkspace = true\n",
    )
    .unwrap();
    backend.reload_config(dir.path());
    assert!(
        backend.diag.pending_uris.lock().is_empty(),
        "a reload that leaves diagnostics off has nothing to redo"
    );

    std::fs::write(&config_path, "[diagnostics]\n").unwrap();
    backend.reload_config(dir.path());
    assert!(backend.diag.pending_uris.lock().contains(uri));
}

/// A reload merges the configured global config file, not the one
/// belonging to whoever runs the process: point a backend at a global
/// file of our own and its settings must show up, while the default
/// test backend (which has no global layer at all) reloads the
/// project config on its own.
#[test]
fn reload_config_merges_the_configured_global_layer() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir
        .path()
        .join("global")
        .join(crate::config::CONFIG_FILE_NAME);
    std::fs::create_dir_all(global.parent().unwrap()).unwrap();
    std::fs::write(&global, "[diagnostics]\nextra-arguments = true\n").unwrap();

    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let isolated = Backend::new_test();
    isolated.reload_config(&project);
    assert!(
        !isolated.config().diagnostics.extra_arguments_enabled(),
        "a test backend must not pick up any global config"
    );

    let mut backend = Backend::new_test();
    backend.state_mut().workspace.global_config_path = Some(global);
    backend.reload_config(&project);
    assert!(backend.config().diagnostics.extra_arguments_enabled());
}

/// The compiled `[indexing]` filters are cached until the config is
/// replaced, so a reload has to drop them. Writing the new config
/// straight into the mutex leaves the old globs compiled and every
/// scan after the reload keeps using the settings the user just
/// changed away from.
#[test]
fn reload_config_recompiles_the_indexing_filters() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());

    // Compile and cache the filters under the empty default config.
    assert!(
        !backend
            .index_filters()
            .is_excluded_entry(&dir.path().join("generated"), true)
    );

    std::fs::write(
        dir.path().join(crate::config::CONFIG_FILE_NAME),
        "[indexing]\nexclude = [\"generated\"]\nextensions = [\"module\"]\n",
    )
    .unwrap();
    backend.reload_config(dir.path());

    let filters = backend.index_filters();
    assert!(
        filters.is_excluded_entry(&dir.path().join("generated"), true),
        "a reload must recompile the exclude globs"
    );
    assert_eq!(filters.extra_extensions(), &["module".to_string()]);
}

/// Recompiling the filters only governs the next scan. A live
/// `.phpantom.toml` edit also has to reconcile the index built under
/// the old ones, or the tree the user just excluded keeps answering
/// completion and workspace symbol search until restart.
#[test]
fn reload_config_evicts_newly_excluded_classes() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());

    let generated = dir.path().join("generated/Hidden.php");
    std::fs::create_dir_all(generated.parent().unwrap()).unwrap();
    std::fs::write(&generated, "<?php\nclass Hidden {}\n").unwrap();
    let uri = crate::util::path_to_uri(&generated);
    backend
        .symbols
        .with_class_declarations(|decls| decls.note_discovered("Hidden", uri));

    std::fs::write(
        dir.path().join(crate::config::CONFIG_FILE_NAME),
        "[indexing]\nexclude = [\"generated\"]\n",
    )
    .unwrap();
    backend.reload_config(dir.path());

    assert!(
        backend.symbols.fqn_uri_index.read().get("Hidden").is_none(),
        "a class under a newly excluded path must leave the index"
    );
}

/// A backend whose client accepted dynamic registration of the file
/// watchers, which is what makes any registration worth tracking.
fn backend_whose_client_accepts_watcher_registration() -> Backend {
    let backend = Backend::new_test();
    backend
        .client_caps
        .write()
        .watched_files_dynamic_registration = true;
    backend
}

/// A client that did not opt in to dynamic registration of watchers is
/// not asked to register any, and nothing is recorded as registered.
#[test]
fn no_watcher_registration_is_built_for_a_client_that_did_not_opt_in() {
    let backend = Backend::new_test();

    assert!(backend.initial_watched_file_registration().is_none());
    assert_eq!(backend.registered_watcher_state.read().clone(), None);
}

/// The counterpart: a client that opted in gets the registration, and
/// what it was built from is recorded for the next reload to compare.
#[test]
fn the_startup_watcher_registration_is_recorded_for_a_client_that_opted_in() {
    let backend = backend_whose_client_accepts_watcher_registration();

    let registration = backend
        .initial_watched_file_registration()
        .expect("a client that opted in should be asked to register its watchers");

    assert_eq!(registration.method, "workspace/didChangeWatchedFiles");
    assert_eq!(
        backend.registered_watcher_state.read().clone(),
        Some(crate::WatchedFileInputs {
            extra_extensions: Vec::new(),
            is_laravel: true,
            is_symfony: true,
            is_twig: true,
            followed_links: Vec::new(),
        })
    );
}

/// A reload that adds an extension has nothing to re-register for a
/// client that never took the first registration, so it must not start
/// tracking one.
#[test]
fn reload_config_does_not_register_watchers_for_a_client_that_did_not_opt_in() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());

    std::fs::write(
        dir.path().join(crate::config::CONFIG_FILE_NAME),
        "[indexing]\nextensions = [\"module\"]\n",
    )
    .unwrap();
    backend.reload_config(dir.path());

    assert_eq!(backend.registered_watcher_state.read().clone(), None);
}

/// Adding an `[indexing] extensions` entry used to need a restart
/// before its files were watched: `initialized` only ever registered
/// the watcher list once, and a live `.phpantom.toml` reload never
/// revisited it. A reload must now record the new extension so the
/// (client-side) watcher registration can be refreshed to match.
#[test]
fn reload_config_updates_the_registered_watcher_state_for_a_new_extension() {
    let dir = tempfile::tempdir().unwrap();
    let backend = backend_whose_client_accepts_watcher_registration();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());

    // Simulate the registration `initialized` performs at startup,
    // before any `.phpantom.toml` extensions were configured.
    backend.reregister_watched_files_if_changed();
    assert_eq!(
        backend.registered_watcher_state.read().clone(),
        Some(crate::WatchedFileInputs {
            extra_extensions: Vec::new(),
            is_laravel: true,
            is_symfony: true,
            is_twig: true,
            followed_links: Vec::new(),
        })
    );

    std::fs::write(
        dir.path().join(crate::config::CONFIG_FILE_NAME),
        "[indexing]\nextensions = [\"module\"]\n",
    )
    .unwrap();
    backend.reload_config(dir.path());

    assert_eq!(
        backend.registered_watcher_state.read().clone(),
        Some(crate::WatchedFileInputs {
            extra_extensions: vec!["module".to_string()],
            is_laravel: true,
            is_symfony: true,
            is_twig: true,
            followed_links: Vec::new(),
        }),
        "a reload that adds an extension must update the registered watcher state"
    );
}

/// An unrelated config edit that leaves `[indexing] extensions` and
/// the Laravel classification untouched must not appear as a change
/// to the registered watcher state.
#[test]
fn reload_config_leaves_the_registered_watcher_state_untouched_for_an_unrelated_edit() {
    let dir = tempfile::tempdir().unwrap();
    let backend = backend_whose_client_accepts_watcher_registration();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    backend.reregister_watched_files_if_changed();

    std::fs::write(
        dir.path().join(crate::config::CONFIG_FILE_NAME),
        "[diagnostics]\nextra-arguments = false\n",
    )
    .unwrap();
    backend.reload_config(dir.path());

    assert_eq!(
        backend.registered_watcher_state.read().clone(),
        Some(crate::WatchedFileInputs {
            extra_extensions: Vec::new(),
            is_laravel: true,
            is_symfony: true,
            is_twig: true,
            followed_links: Vec::new(),
        })
    );
}

/// A tree reached through a symlink sits outside every workspace
/// folder, so the plain `**/*.php` watchers never cover it. Without a
/// watcher based at the link, a file created in the linked tree by
/// another tool stays invisible until the window is reloaded.
#[test]
fn a_followed_link_gets_its_own_relative_pattern_watcher() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    let real = dir.path().join("framework");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("Help.php"), "<?php\n").unwrap();

    let link = root.join("kdhelp");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real, &link).unwrap();

    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(root.clone());
    backend.client_caps.write().relative_pattern_watchers = true;

    // The walk is what discovers the link, exactly as it does at
    // startup.
    let files = crate::classmap_scanner::collect_php_files_gitignore(
        &root,
        &[],
        &backend.index_filters(),
        Some(backend.followed_links()),
    );
    assert!(
        files.iter().any(|p| p.starts_with(&link)),
        "the walk must index through the link first: {files:?}"
    );

    let (_, inputs) = backend.build_watched_file_registration();
    assert_eq!(
        inputs.followed_links,
        vec![link],
        "the link the index reached through must be watched"
    );
}

/// The relative-pattern watchers are dropped for a client that never
/// said it could match one. A client that does not understand the
/// object form of a glob pattern can fail to parse the registration
/// carrying it, which would take the workspace watchers down with it.
#[test]
fn a_client_without_relative_patterns_gets_no_link_watchers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    let real = dir.path().join("framework");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("Help.php"), "<?php\n").unwrap();

    let link = root.join("kdhelp");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real, &link).unwrap();

    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(root.clone());
    crate::classmap_scanner::collect_php_files_gitignore(
        &root,
        &[],
        &backend.index_filters(),
        Some(backend.followed_links()),
    );

    let (_, inputs) = backend.build_watched_file_registration();
    assert!(
        inputs.followed_links.is_empty(),
        "a client that cannot match a relative pattern must get none"
    );
}

/// A client is free to resolve the base URI it was handed before it
/// starts watching, and then reports the real path behind the link.
/// The index holds the link spelling, so every lookup in the batch
/// handler would miss unless the path is spelled back first.
#[test]
fn a_real_path_event_is_respelled_through_the_link() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    let real = dir.path().join("framework");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(real.join("src")).unwrap();
    std::fs::write(real.join("src/Help.php"), "<?php\nclass Help {}").unwrap();

    let link = root.join("kdhelp");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real, &link).unwrap();

    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(root.clone());
    crate::classmap_scanner::collect_php_files_gitignore(
        &root,
        &[],
        &backend.index_filters(),
        Some(backend.followed_links()),
    );

    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            // Resolve system temp-directory symlinks as well as the watched link.
            uri: Uri::from_file_path(real.join("src/Help.php").canonicalize().unwrap()).unwrap(),
            kind: FileChangeType::Created,
        }],
    };
    backend.apply_watched_file_changes(&params, &root);

    let indexed = backend.symbols.fqn_uri_index.read();
    let uri = indexed
        .get("Help")
        .unwrap_or_else(|| panic!("a created file must reach the index: {indexed:?}"))
        .clone();
    drop(indexed);
    assert!(
        uri.contains("/kdhelp/"),
        "the index must keep the link spelling the walk produced, got {uri}"
    );
}

/// An extension the *editor* forwards needs its watcher registered
/// just as much as one written into `.phpantom.toml`. Without it the
/// client reports no events for those files and the index keeps
/// serving the last full scan.
#[test]
fn a_client_extension_update_updates_the_registered_watcher_state() {
    let dir = tempfile::tempdir().unwrap();
    let backend = backend_whose_client_accepts_watcher_registration();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    backend.reregister_watched_files_if_changed();

    backend.set_client_indexing_options(crate::config::ClientIndexingOptions {
        exclude: Vec::new(),
        extensions: vec!["module".to_string()],
    });
    backend.reregister_watched_files_if_changed();

    assert_eq!(
        backend.registered_watcher_state.read().clone(),
        Some(crate::WatchedFileInputs {
            extra_extensions: vec!["module".to_string()],
            is_laravel: true,
            is_symfony: true,
            is_twig: true,
            followed_links: Vec::new(),
        }),
        "a client-forwarded extension must be watched"
    );
}
