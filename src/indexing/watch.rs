//! Watched-file change application.
//!
//! Applies a `workspace/didChangeWatchedFiles` batch to the symbol
//! indexes on a blocking thread. Also owns [`Backend::reload_config`] and
//! the background poller that watches the global config file, since the
//! client's file watcher only ever reports paths inside the workspace
//! (see [`Backend::global_config_watcher`]).

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tower_lsp::lsp_types::*;

use crate::Backend;

/// How often [`Backend::global_config_watcher`] stats the global config
/// file. A single `stat` call is cheap enough that sub-second polling
/// would still be negligible, but there is no reason to notice an edit
/// faster than a human can plausibly switch back to their editor.
const GLOBAL_CONFIG_POLL_INTERVAL: Duration = Duration::from_secs(2);

impl Backend {
    /// Apply a `workspace/didChangeWatchedFiles` batch to the indexes.
    ///
    /// Returns `true` if any PHP/resource file, composer file, or the
    /// project's own `.phpantom.toml` was acted on (so the caller can ask the
    /// editor to refresh affected features). Runs entirely on a blocking
    /// thread; it parses no files on the async runtime.
    ///
    /// Editors cannot watch the filesystem while the window is unfocused, so
    /// on refocus they resynchronise by reporting the *entire* workspace as
    /// "changed" in one notification (hundreds of KiB of events).  Almost
    /// none of those files actually changed, and most were never parsed:
    /// PHPantom loads class details lazily, holding only a name→file pointer
    /// in the discovery index until something resolves the class.  Re-reading
    /// and re-scanning every reported file from disk would do thousands of
    /// wasted syscalls on every refocus.
    ///
    /// So a plain content change is only acted on for files we have actually
    /// parsed (whose cached details would otherwise go stale).  Created and
    /// deleted files are always handled: a creation makes a new class
    /// discoverable, and a deletion must purge a now-dangling entry, both of
    /// which matter even for files we never loaded.
    pub(crate) fn apply_watched_file_changes(
        &self,
        params: &DidChangeWatchedFilesParams,
        root: &std::path::Path,
    ) -> bool {
        let mut composer_changed = false;
        let mut config_changed = false;
        let mut schema_full_rebuild = false;
        let mut migration_changes: Vec<(PathBuf, FileChangeType)> = Vec::new();
        let mut php_changes: Vec<(String, PathBuf, FileChangeType)> = Vec::new();
        let mut resource_changes: Vec<(String, PathBuf, FileChangeType)> = Vec::new();
        let mut migration_discovery =
            crate::virtual_members::laravel::database_schema::MigrationDiscovery::default();
        let is_laravel = self.resolved_class_cache.read().is_laravel();
        let config_path = root.join(crate::config::CONFIG_FILE_NAME);
        {
            let open = self.open_files.read();
            let parsed = self.parsed_uris.read();
            let indexed = self.symbol_maps.read();
            let laravel_config = self.config().laravel;
            for change in &params.changes {
                let path_str = change.uri.path();
                if path_str.ends_with("/composer.json") || path_str.ends_with("/composer.lock") {
                    composer_changed = true;
                    continue;
                }
                if change.uri.to_file_path().is_ok_and(|p| p == config_path) {
                    config_changed = true;
                    continue;
                }
                if is_laravel
                    && let Ok(file_path) = change.uri.to_file_path()
                    && crate::virtual_members::laravel::database_schema::SchemaIndex::watched_path_affects_schema(
                        root,
                        &laravel_config,
                        &file_path,
                    )
                {
                    if laravel_config.migrations.enabled()
                        && crate::virtual_members::laravel::database_schema::is_migration_php_file(
                            root,
                            &laravel_config.migrations,
                            &file_path,
                        )
                    {
                        // A deletion only ever removes a file the initial
                        // scan itself put in the plan, so it needs no
                        // discovery check -- and the file is gone from disk
                        // by now, so a walk could not find it anyway.
                        if change.typ == FileChangeType::DELETED
                            || migration_discovery.is_discoverable(
                                root,
                                &laravel_config.migrations,
                                &file_path,
                            )
                        {
                            migration_changes.push((file_path, change.typ));
                        }
                    } else {
                        schema_full_rebuild = true;
                    }
                    continue;
                }
                let uri_str = change.uri.to_string();
                if crate::resource_navigation::is_resource_document(path_str) {
                    if open.contains_key(&uri_str) {
                        continue;
                    }
                    let Ok(file_path) = change.uri.to_file_path() else {
                        continue;
                    };
                    if change.typ == FileChangeType::CHANGED {
                        let canonical_uri = crate::util::path_to_uri(&file_path);
                        if !indexed.contains_key(&uri_str)
                            && !indexed.contains_key(canonical_uri.as_str())
                        {
                            continue;
                        }
                    }
                    resource_changes.push((uri_str, file_path, change.typ));
                    continue;
                }
                if !path_str.ends_with(".php") {
                    continue;
                }

                // Open files are already tracked via did_open/did_change.
                if open.contains_key(&uri_str) {
                    continue;
                }
                let Ok(file_path) = change.uri.to_file_path() else {
                    continue;
                };

                if change.typ == FileChangeType::CHANGED {
                    // `parsed_uris` records the editor URI for open files and
                    // the canonical `file://` URI for lazily loaded ones;
                    // check both spellings.
                    let canonical_uri = crate::util::path_to_uri(&file_path);
                    let loaded =
                        parsed.contains(&uri_str) || parsed.contains(canonical_uri.as_str());
                    if !loaded {
                        continue;
                    }
                }

                php_changes.push((uri_str, file_path, change.typ));
            }
        }

        if php_changes.is_empty()
            && resource_changes.is_empty()
            && !composer_changed
            && !config_changed
            && !schema_full_rebuild
            && migration_changes.is_empty()
        {
            return false;
        }

        if config_changed {
            tracing::info!("PHPantom: .phpantom.toml changed, reloading configuration");
            self.reload_config(root);
            // Schema/migration settings live in the same file, and the
            // cheapest correct response to "something in here changed" is
            // the same full rebuild a config/database.php or schema file
            // change already triggers below.
            if is_laravel {
                schema_full_rebuild = true;
            }
        }

        if !php_changes.is_empty() {
            tracing::info!(
                "PHPantom: {} watched PHP file(s) changed on disk, refreshing indexes",
                php_changes.len()
            );
            self.reindex_files_batch(&php_changes);
            // A class that was previously "not found" may now exist, and
            // resolved class info / member completions may be stale for a
            // class whose file changed.
            self.clear_class_not_found_cache();
            self.resolved_class_cache.write().clear();
            self.auth_user_type_cache.write().clear();
            *self.storage_disk_type_cache.write() = None;
            *self.laravel_aliases.write() = None;
            self.member_completion_cache.lock().clear();
        }

        if composer_changed {
            tracing::info!("PHPantom: composer files changed, rescanning vendor");
            self.rescan_composer_indexes(root);
        }

        if !resource_changes.is_empty() {
            tracing::info!(
                "PHPantom: {} watched YAML/XML file(s) changed on disk, refreshing references",
                resource_changes.len()
            );
            for (uri, path, change_type) in &resource_changes {
                if *change_type == FileChangeType::DELETED {
                    self.clear_file_maps(uri);
                } else if let Ok(content) = std::fs::read_to_string(path) {
                    self.update_resource_symbol_index(uri, &content);
                }
            }
        }
        if schema_full_rebuild {
            tracing::info!("PHPantom: Laravel schema files changed, reloading schema index");
            self.reload_laravel_schema_index(root);
        } else if !migration_changes.is_empty() {
            tracing::info!(
                "PHPantom: {} migration file(s) changed, incremental schema update",
                migration_changes.len()
            );
            self.update_laravel_migrations(&migration_changes);
        }

        true
    }

    /// Reload the merged project + global configuration from disk.
    ///
    /// Used both for a project's own `.phpantom.toml` (via
    /// [`apply_watched_file_changes`](Self::apply_watched_file_changes))
    /// and for the global config file (polled by the background watcher
    /// spawned in `initialized`), so either one takes effect immediately
    /// instead of requiring a restart. Always reloads both layers so the
    /// project one keeps overriding the global one no matter which file
    /// changed.
    ///
    /// `config` lives behind an `Arc` precisely so that a write made here
    /// on a cloned `Backend` (a blocking-task or background-worker clone)
    /// is visible to every other clone, including the long-lived one that
    /// answers LSP requests.
    pub(crate) fn reload_config(&self, root: &std::path::Path) {
        match crate::config::load_config_from(root, self.workspace.global_config_path.as_deref()) {
            Ok(cfg) => *self.workspace.config.lock() = cfg,
            Err(e) => {
                tracing::warn!("Failed to reload .phpantom.toml: {}", e);
                return;
            }
        }

        // Resolved classes and completions may depend on config-driven
        // behaviour (e.g. `report-magic-properties`), so both must be
        // recomputed against the new settings rather than served stale.
        self.resolved_class_cache.write().clear();
        self.member_completion_cache.lock().clear();

        // Switching workspace diagnostics on or off is the one setting
        // whose consumer has already run (or is still running) by the
        // time a reload lands, so both directions need handling here
        // rather than merely being read the next time something asks.
        self.start_workspace_diagnostics_on_reload();
        self.stop_workspace_diagnostics_on_reload();
    }

    /// Poll the global config file for changes and reload on edit.
    ///
    /// The global config lives outside every workspace, so it can never
    /// match a client-side `**/…` file watcher glob (workspace-relative by
    /// definition) and dynamic registration with an absolute
    /// [`RelativePattern`](tower_lsp::lsp_types::RelativePattern) base is
    /// unevenly supported across editors. Polling one file's mtime is a
    /// single `stat` call, cheap enough to just always do ourselves rather
    /// than depend on client capabilities.
    ///
    /// Runs for the lifetime of the session; exits once
    /// [`shutdown_flag`](Self) is set, same as the other background
    /// workers spawned in `initialized`.
    pub(crate) async fn global_config_watcher(&self, root: PathBuf) {
        let Some(path) = self.workspace.global_config_path.clone() else {
            return;
        };

        let mtime = |p: &std::path::Path| std::fs::metadata(p).ok().and_then(|m| m.modified().ok());
        let mut last_modified = mtime(&path);

        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }
            tokio::time::sleep(GLOBAL_CONFIG_POLL_INTERVAL).await;

            let modified = mtime(&path);
            if modified == last_modified {
                continue;
            }
            last_modified = modified;
            tracing::info!("PHPantom: global config changed, reloading configuration");
            self.reload_config(&root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_laravel_projects_ignore_schema_watch_changes() {
        let dir = tempfile::tempdir().unwrap();
        let schema = dir.path().join("database/schema/default-schema.sql");
        std::fs::create_dir_all(schema.parent().unwrap()).unwrap();
        std::fs::write(&schema, "CREATE TABLE users (id bigint);").unwrap();

        let params = DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Url::from_file_path(&schema).unwrap(),
                typ: FileChangeType::CREATED,
            }],
        };

        let backend = Backend::new_test();
        backend.resolved_class_cache.write().set_laravel(false);
        assert!(!backend.apply_watched_file_changes(&params, dir.path()));

        // The gate is on the project type alone: the same event in a
        // Laravel workspace still rebuilds the schema index.
        let backend = Backend::new_test();
        backend.resolved_class_cache.write().set_laravel(true);
        assert!(backend.apply_watched_file_changes(&params, dir.path()));
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
            FileChangeType::DELETED,
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
        assert!(
            backend
                .symbols
                .method_store
                .read()
                .contains_key(&("Vendor\\Variant".to_string(), "bare".to_string())),
            "the method store must follow the surviving declaration"
        );
        assert!(
            !backend
                .symbols
                .method_store
                .read()
                .contains_key(&("Vendor\\Variant".to_string(), "rich".to_string())),
            "the deleted file's members must be gone"
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
                FileChangeType::DELETED,
            ),
            (
                "file:///b_bare.php".to_string(),
                PathBuf::from("/b_bare.php"),
                FileChangeType::DELETED,
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
        backend.resolved_class_cache.write().set_laravel(false);
        assert!(!backend.config().diagnostics.extra_arguments_enabled());

        let params = DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Url::from_file_path(&config_path).unwrap(),
                typ: FileChangeType::CHANGED,
            }],
        };
        assert!(backend.apply_watched_file_changes(&params, dir.path()));
        assert!(backend.config().diagnostics.extra_arguments_enabled());
    }

    /// `reload_config` writes through the `Arc<Mutex<Config>>` shared by
    /// every `Backend` clone (the blocking-task and background-worker
    /// clones created via `clone_for_diagnostic_worker`), not just the
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
        let clone = backend.clone_for_diagnostic_worker();
        assert!(!backend.config().diagnostics.extra_arguments_enabled());

        clone.reload_config(dir.path());

        assert!(
            backend.config().diagnostics.extra_arguments_enabled(),
            "a reload on a clone must be visible on the original Backend"
        );
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
        backend.workspace.global_config_path = Some(global);
        backend.reload_config(&project);
        assert!(backend.config().diagnostics.extra_arguments_enabled());
    }
}
