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

/// The `client/registerCapability` id used for the
/// `workspace/didChangeWatchedFiles` registration built by
/// [`Backend::build_watched_file_registration`], shared with
/// [`Backend::reregister_watched_files_if_changed`] so it can unregister
/// the same capability it is about to replace.
const WATCHED_FILES_REGISTRATION_ID: &str = "workspace/didChangeWatchedFiles";

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
        let changes = self.spell_changes_as_indexed(&params.changes);
        {
            let open = self.open_files.read();
            let parsed = self.parsed_uris.read();
            let indexed = self.symbol_maps.read();
            let laravel_config = self.config().laravel;
            let filters = self.index_filters();
            for change in changes.iter() {
                let path_str = change.uri.path();
                if path_str.ends_with("/composer.json") || path_str.ends_with("/composer.lock") {
                    composer_changed = true;
                    continue;
                }
                if change.uri.to_file_path().is_ok_and(|p| p == config_path) {
                    config_changed = true;
                    continue;
                }
                // A running Laravel application churns its own cache
                // artifacts: every request can compile a Blade template
                // into storage/framework/views, and artisan rewrites the
                // manifests in bootstrap/cache.  None of it is authored
                // code and the workspace walkers never index those
                // directories (Laravel ships .gitignore files inside
                // them), so reacting to their events would chain the
                // editor's responsiveness to the application's request
                // traffic.
                if is_laravel
                    && (path_str.contains("/storage/framework/")
                        || path_str.contains("/bootstrap/cache/"))
                {
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
                    // An excluded path is invisible to the workspace walk,
                    // so its events are dropped here too.
                    if filters.is_excluded_path(&file_path, false) {
                        continue;
                    }
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
                let is_php = std::path::Path::new(path_str)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| filters.is_php_extension(ext));
                if !is_php {
                    continue;
                }

                // Open files are already tracked via did_open/did_change.
                if open.contains_key(&uri_str) {
                    continue;
                }
                let Ok(file_path) = change.uri.to_file_path() else {
                    continue;
                };

                // Excluded paths are invisible to indexing; skip their
                // events the way the workspace scanners skip the files.
                if filters.is_excluded_path(&file_path, false) {
                    continue;
                }

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
            // A class that was previously "not found" may now exist, and
            // resolved class info / member completions may be stale for a
            // class whose file changed.  A batch that touched no class
            // declaration at all (a generated artifact, a routes file)
            // cannot invalidate any of these class-derived caches, and
            // dropping them anyway makes the next completion re-resolve
            // the whole project: on a large one that is the difference
            // between an instant answer and a multi-second stall every
            // time something writes a PHP file the editor watches.
            //
            // Laravel config files are declaration-free but still feed
            // class resolution (`config/app.php` aliases, the auth model,
            // storage disks), so anything under a `config` directory
            // keeps the conservative full clear.
            let touches_config = php_changes.iter().any(|(_, path, _)| {
                path.components().any(|c| {
                    c.as_os_str()
                        .to_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("config"))
                })
            });
            if self.reindex_files_batch(&php_changes) || touches_config {
                self.clear_class_not_found_cache();
                self.resolved_class_cache.write().clear();
                self.auth_user_type_cache.write().clear();
                *self.storage_disk_type_cache.write() = None;
                self.laravel_aliases.invalidate();
                self.member_completion_cache.lock().clear();
            }
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
        // Captured before the reload replaces them, so the reconciliation
        // below can tell which way the file filters moved.
        let previous_filters = self.index_filters();

        match crate::config::load_config_from(root, self.workspace.global_config_path.as_deref()) {
            Ok(cfg) => self.set_config(cfg),
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

        // An `[indexing] extensions` entry (e.g. Drupal's `.module`)
        // added to the config after startup needs its own watcher
        // registered, or the client never reports its file events and
        // the index keeps serving the last full scan.
        self.reregister_watched_files_if_changed();

        // An edit to `[indexing] exclude` or `extensions` changes what
        // belongs in the index, not merely what the next scan will see.
        self.reconcile_index_for_filter_change(&previous_filters);
    }

    /// Rewrite the paths in a watched-file batch into the spellings the
    /// index holds.
    ///
    /// A directory reached through a symlink is indexed under the link's
    /// spelling, but the watcher asking for its events is based at the link
    /// and a client is free to resolve that base before it starts watching.
    /// Such a client reports `/opt/kdhelp/Help.php` where the index holds
    /// `<root>/kdhelp/Help.php`, and every lookup below (open files, parsed
    /// URIs, the symbol maps, the exclude filters) would miss. Clients that
    /// do not resolve it report the link spelling already and pass through
    /// untouched.
    ///
    /// Borrows the batch unchanged when the project has no followed links,
    /// which is all but a handful of projects and every project before its
    /// first walk.
    fn spell_changes_as_indexed<'a>(
        &self,
        changes: &'a [FileEvent],
    ) -> std::borrow::Cow<'a, [FileEvent]> {
        let links = self.followed_links();
        if links.is_empty() {
            return std::borrow::Cow::Borrowed(changes);
        }
        let mut spelled = changes.to_vec();
        for change in &mut spelled {
            if let Ok(path) = change.uri.to_file_path()
                && let Some(as_indexed) = links.to_link_spelling(&path)
                && let Ok(uri) = Url::from_file_path(as_indexed)
            {
                change.uri = uri;
            }
        }
        std::borrow::Cow::Owned(spelled)
    }

    /// Build the `workspace/didChangeWatchedFiles` registration for the
    /// current `[indexing] extensions` config, Laravel classification, and
    /// the directory symlinks the index has reached through.
    ///
    /// Shared by `initialized`'s first registration and
    /// [`Self::reregister_watched_files_if_changed`] so a live
    /// `.phpantom.toml` reload, or a walk that discovers a link, advertises
    /// the same watcher set a fresh session would have started with.
    /// Returns the registration alongside the inputs it was built from, so
    /// the caller can record what was actually registered.
    pub(crate) fn build_watched_file_registration(
        &self,
    ) -> (Registration, crate::WatchedFileInputs) {
        let index_filters = self.index_filters();
        let extra_extensions = index_filters.extra_extensions().to_vec();
        let is_laravel = self.resolved_class_cache.read().is_laravel();

        // Create, change, and delete: what every watcher but the two
        // Composer ones asks for.
        let watch_all = WatchKind::Create | WatchKind::Change | WatchKind::Delete;

        // Patterns the workspace folders are watched with. Each one is
        // repeated per followed link below, based at the link, because a
        // workspace-relative pattern never reaches through a symlink.
        let mut patterns: Vec<(String, WatchKind)> = extra_extensions
            .iter()
            .map(|ext| (format!("**/*.{ext}"), watch_all))
            .collect();
        patterns.extend([
            ("**/*.php".to_string(), watch_all),
            ("**/*.{yaml,yml,xml}".to_string(), watch_all),
            ("**/*.{yaml,yml,xml}.dist".to_string(), watch_all),
            ("**/composer.json".to_string(), WatchKind::Change),
            ("**/composer.lock".to_string(), WatchKind::Change),
            ("**/.phpantom.toml".to_string(), watch_all),
        ]);
        if is_laravel {
            patterns.extend([
                ("**/*.sql".to_string(), watch_all),
                ("**/config/database.php".to_string(), watch_all),
            ]);
        }

        let mut watchers: Vec<FileSystemWatcher> = patterns
            .iter()
            .map(|(pattern, kind)| FileSystemWatcher {
                glob_pattern: GlobPattern::String(pattern.clone()),
                kind: Some(*kind),
            })
            .collect();

        // A tree reached through a symlink is indexed under the link's
        // spelling but sits outside every workspace folder on disk, so the
        // patterns above never match a file in it. Asking for it by base
        // URI is the only way to hear about a `git pull` into a linked
        // framework, or a file created there by another tool.
        let followed_links = self.watchable_followed_links();
        for link in &followed_links {
            let Ok(base) = Url::from_file_path(link) else {
                continue;
            };
            watchers.extend(patterns.iter().map(|(pattern, kind)| FileSystemWatcher {
                glob_pattern: GlobPattern::Relative(RelativePattern {
                    base_uri: OneOf::Right(base.clone()),
                    pattern: pattern.clone(),
                }),
                kind: Some(*kind),
            }));
        }

        let registration = Registration {
            id: WATCHED_FILES_REGISTRATION_ID.to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: Some(
                serde_json::to_value(DidChangeWatchedFilesRegistrationOptions { watchers })
                    .unwrap(),
            ),
        };

        (
            registration,
            crate::WatchedFileInputs {
                extra_extensions,
                is_laravel,
                followed_links,
            },
        )
    }

    /// The followed links worth putting in a registration: none unless the
    /// client said it can match a pattern against a base URI, since a
    /// client that cannot would either ignore the watcher or fail to parse
    /// the registration that carries it, taking the workspace watchers
    /// down with it.
    fn watchable_followed_links(&self) -> Vec<std::path::PathBuf> {
        if !self
            .supports_relative_pattern_watchers
            .load(Ordering::Acquire)
        {
            return Vec::new();
        }
        self.followed_links().link_paths()
    }

    /// Re-push the `workspace/didChangeWatchedFiles` registration when
    /// `[indexing] extensions`, the Laravel classification, or the set of
    /// followed symlinks has changed since the last registration.
    ///
    /// Guarded on an actual change so an unrelated config edit does not
    /// churn the client's watcher list. Before `initialized` performs the
    /// first registration, this only records the desired state instead of
    /// racing that initial `register_capability` call.
    pub(crate) fn reregister_watched_files_if_changed(&self) {
        let (registration, inputs) = self.build_watched_file_registration();

        let mut state = self.registered_watcher_state.write();
        let Some(previous) = state.clone() else {
            *state = Some(inputs);
            return;
        };
        if previous == inputs {
            return;
        }
        *state = Some(inputs);
        drop(state);

        if self.client.is_none() {
            return;
        }
        // `reload_config` is synchronous, and runs on a blocking thread
        // for a watched-file batch and on the runtime for the global
        // config poller.  Both carry the runtime context; a unit test
        // calling it directly does not, and has nothing to (un)register.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let backend = self.clone_for_diagnostic_worker();
        runtime.spawn(async move {
            let Some(client) = &backend.client else {
                return;
            };
            let _ = client
                .unregister_capability(vec![Unregistration {
                    id: WATCHED_FILES_REGISTRATION_ID.to_string(),
                    method: "workspace/didChangeWatchedFiles".to_string(),
                }])
                .await;
            let _ = client.register_capability(vec![registration]).await;
        });
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
                uri: Url::from_file_path(path).unwrap(),
                typ: FileChangeType::CREATED,
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
        let url = Url::from_file_path(&template).unwrap();
        let uri = url.to_string();

        let backend = Backend::new_test();
        backend.update_ast(&uri, "<div>{{ $name }}</div>\n");
        assert!(backend.blade_virtual_content.read().contains_key(&uri));
        assert!(backend.blade_source_maps.read().contains_key(&uri));

        let params = DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: url,
                typ: FileChangeType::DELETED,
            }],
        };
        backend.apply_watched_file_changes(&params, dir.path());

        assert!(!backend.blade_virtual_content.read().contains_key(&uri));
        assert!(!backend.blade_source_maps.read().contains_key(&uri));
    }

    /// The registrations a provider contributes are replaced when the file
    /// is parsed, which a file deleted from disk never is again, so the
    /// deletion itself has to take them away.
    #[test]
    fn deleting_a_provider_drops_its_gate_and_macro_registrations() {
        let dir = tempfile::tempdir().unwrap();
        let provider = dir.path().join("app/Providers/AuthServiceProvider.php");
        let url = Url::from_file_path(&provider).unwrap();
        let uri = url.to_string();

        let backend = Backend::new_test();
        backend.resolved_class_cache.write().set_laravel(true);
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
                .laravel_gates
                .read()
                .definition("edit-post")
                .is_some()
        );
        assert!(backend.laravel_macros.read().files.has_uri(&uri));

        let params = DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: url,
                typ: FileChangeType::DELETED,
            }],
        };
        backend.apply_watched_file_changes(&params, dir.path());

        assert!(
            backend
                .laravel_gates
                .read()
                .definition("edit-post")
                .is_none()
        );
        assert!(!backend.laravel_gates.read().files.has_uri(&uri));
        assert!(!backend.laravel_macros.read().files.has_uri(&uri));
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
        backend.resolved_class_cache.write().set_laravel(true);
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

    /// Adding an `[indexing] extensions` entry used to need a restart
    /// before its files were watched: `initialized` only ever registered
    /// the watcher list once, and a live `.phpantom.toml` reload never
    /// revisited it. A reload must now record the new extension so the
    /// (client-side) watcher registration can be refreshed to match.
    #[test]
    fn reload_config_updates_the_registered_watcher_state_for_a_new_extension() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Backend::new_test();
        *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());

        // Simulate the registration `initialized` performs at startup,
        // before any `.phpantom.toml` extensions were configured.
        backend.reregister_watched_files_if_changed();
        assert_eq!(
            backend.registered_watcher_state.read().clone(),
            Some(crate::WatchedFileInputs {
                extra_extensions: Vec::new(),
                is_laravel: true,
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
        let backend = Backend::new_test();
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
        backend
            .supports_relative_pattern_watchers
            .store(true, Ordering::Release);

        // The walk is what discovers the link, exactly as it does at
        // startup.
        let files = crate::references::collect_php_files_gitignore(
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
        crate::references::collect_php_files_gitignore(
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
        crate::references::collect_php_files_gitignore(
            &root,
            &[],
            &backend.index_filters(),
            Some(backend.followed_links()),
        );

        let params = DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Url::from_file_path(real.join("src/Help.php")).unwrap(),
                typ: FileChangeType::CREATED,
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
        let backend = Backend::new_test();
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
                followed_links: Vec::new(),
            }),
            "a client-forwarded extension must be watched"
        );
    }
}
