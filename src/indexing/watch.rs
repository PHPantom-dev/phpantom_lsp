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

use tower_lsp_server::gen_lsp_types::*;

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

/// A watched-file batch sorted by the work it calls for.
#[derive(Default)]
struct WatchedChangeBatch {
    /// The `composer.json` / `composer.lock` files the batch touched.
    composer_changes: Vec<PathBuf>,
    translations_changed: bool,
    config_changed: bool,
    schema_full_rebuild: bool,
    migration_changes: Vec<(PathBuf, FileChangeType)>,
    php_changes: Vec<(String, PathBuf, FileChangeType)>,
    resource_changes: Vec<(String, PathBuf, FileChangeType)>,
    /// The directories whose cached `Pest.php` lookup the batch made stale.
    pest_dirs: Vec<String>,
    /// Whether a file under a stub path changed.
    stubs_changed: bool,
}

impl WatchedChangeBatch {
    /// Whether no event in the batch needs acting on.
    fn is_empty(&self) -> bool {
        self.php_changes.is_empty()
            && self.resource_changes.is_empty()
            && self.composer_changes.is_empty()
            && !self.translations_changed
            && !self.config_changed
            && !self.schema_full_rebuild
            && self.migration_changes.is_empty()
            && self.pest_dirs.is_empty()
            && !self.stubs_changed
    }
}

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
        let is_laravel = self.resolved_class_cache.read().laravel().is_enabled();
        let changes = self.spell_changes_as_indexed(&params.changes);
        let mut batch = self.classify_watched_changes(&changes, root, is_laravel);
        if batch.is_empty() {
            return false;
        }

        if batch.translations_changed {
            self.laravel.string_key_cache.write().translations = None;
        }

        if batch.config_changed {
            tracing::info!("PHPantom: .phpantom.toml changed, reloading configuration");
            self.reload_config(root);
            // Schema/migration settings live in the same file, and the
            // cheapest correct response to "something in here changed" is
            // the same full rebuild a config/database.php or schema file
            // change already triggers below.
            if is_laravel {
                batch.schema_full_rebuild = true;
            }
        }

        for dir in &batch.pest_dirs {
            self.forget_pest_dir(dir);
        }

        if !batch.php_changes.is_empty() {
            self.reindex_watched_php_files(&batch.php_changes);
        }

        if batch.stubs_changed {
            tracing::info!("PHPantom: stub files changed, rebuilding the stub index");
            self.refresh_stub_paths();
        }

        if !batch.composer_changes.is_empty() {
            tracing::info!("PHPantom: composer files changed, rescanning vendor");
            self.rescan_composer_indexes(root, &batch.composer_changes);
        }

        if !batch.resource_changes.is_empty() {
            self.reindex_watched_resource_files(&batch.resource_changes);
        }

        if batch.schema_full_rebuild {
            tracing::info!("PHPantom: Laravel schema files changed, reloading schema index");
            self.reload_laravel_schema_index(root);
        } else if !batch.migration_changes.is_empty() {
            tracing::info!(
                "PHPantom: {} migration file(s) changed, incremental schema update",
                batch.migration_changes.len()
            );
            self.update_laravel_migrations(&batch.migration_changes);
        }

        true
    }

    /// Sort a watched-file batch into the work each event calls for,
    /// dropping the events that need none (see
    /// [`apply_watched_file_changes`](Self::apply_watched_file_changes)).
    fn classify_watched_changes(
        &self,
        changes: &[FileEvent],
        root: &std::path::Path,
        is_laravel: bool,
    ) -> WatchedChangeBatch {
        let mut batch = WatchedChangeBatch::default();
        let mut migration_discovery =
            crate::virtual_members::laravel::database_schema::MigrationDiscovery::default();
        let config_path = root.join(crate::config::CONFIG_FILE_NAME);
        // The index guards are taken per check, not across the loop:
        // migration discovery walks directories.
        let is_open = |uri: &str| self.open_files.read().contains_key(uri);
        let is_twig = self.is_twig_project();
        let laravel_config = self.config().laravel;
        let filters = self.index_filters();
        let translations = self.laravel.string_key_cache.read().translations.clone();
        let stubs = self.external_stubs();
        for change in changes {
            let path_str = change.uri.path();
            if path_str.ends_with("/composer.json") || path_str.ends_with("/composer.lock") {
                if let Some(path) = crate::util::url_to_path(&change.uri) {
                    batch.composer_changes.push(path);
                }
                continue;
            }
            if crate::util::url_to_path(&change.uri).is_some_and(|p| p == config_path) {
                batch.config_changed = true;
                continue;
            }
            // Test closures are typed from the `Pest.php` above them, read
            // once per directory.  Only one a lookup has read (or found
            // missing) has a cached answer to drop, which keeps the
            // whole-workspace report of a refocus from doing anything for
            // the rest.  The file still goes on to be indexed as PHP below.
            if path_str.ends_with("/Pest.php") {
                let uri_str = change.uri.to_string();
                let dir = self
                    .looked_up_pest_dir(&uri_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        let canonical =
                            crate::util::path_to_uri(&crate::util::url_to_path(&change.uri)?);
                        self.looked_up_pest_dir(&canonical).map(str::to_owned)
                    });
                if let Some(dir) = dir {
                    batch.pest_dirs.push(dir);
                }
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
                && let Some(file_path) = crate::util::url_to_path(&change.uri)
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
                    if change.kind == FileChangeType::Deleted
                        || migration_discovery.is_discoverable(
                            root,
                            &laravel_config.migrations,
                            &file_path,
                        )
                    {
                        batch.migration_changes.push((file_path, change.kind));
                    }
                } else {
                    batch.schema_full_rebuild = true;
                }
                continue;
            }
            let uri_str = change.uri.to_string();
            if is_laravel
                && !is_open(&uri_str)
                && (path_str.contains("/lang/")
                    || translations
                        .as_ref()
                        .is_some_and(|catalog| catalog.contains_uri(&uri_str)))
                && crate::util::url_to_path(&change.uri)
                    .is_some_and(|path| !filters.is_excluded_path(&path, false))
            {
                batch.translations_changed = true;
            }
            // A file that can be a template is followed even while it is
            // none, since what it references may make it one.
            let is_twig_candidate = is_twig && crate::twig::roots::is_candidate_uri(path_str);
            if is_twig_candidate || self.is_resource_uri(&uri_str) {
                if is_open(&uri_str) {
                    continue;
                }
                let Some(file_path) = crate::util::url_to_path(&change.uri) else {
                    continue;
                };
                // An excluded path is invisible to the workspace walk,
                // so its events are dropped here too.
                if filters.is_excluded_path(&file_path, false) {
                    continue;
                }
                if change.kind == FileChangeType::Changed && !is_twig_candidate {
                    let canonical_uri = crate::util::path_to_uri(&file_path);
                    if self.symbol_map_for(&uri_str).is_none()
                        && self.symbol_map_for(&canonical_uri).is_none()
                    {
                        continue;
                    }
                }
                batch
                    .resource_changes
                    .push((uri_str, file_path, change.kind));
                continue;
            }
            let is_php = std::path::Path::new(path_str)
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| filters.is_php_extension(ext));
            if !is_php {
                continue;
            }

            let Some(file_path) = crate::util::url_to_path(&change.uri) else {
                continue;
            };

            // Any change under a stub path can add or remove a name the
            // stub index holds, parsed or not, open or not.  Rebuilding the
            // index drops what was parsed from the stub files as well.
            if stubs.contains_path(&file_path) {
                batch.stubs_changed = true;
                continue;
            }

            self.note_yii_config_change(&uri_str);

            // Open files are already tracked via did_open/did_change.
            if is_open(&uri_str) {
                continue;
            }

            // Excluded paths are invisible to indexing; skip their
            // events the way the workspace scanners skip the files.
            if filters.is_excluded_path(&file_path, false) {
                continue;
            }

            if change.kind == FileChangeType::Changed {
                // `parsed_uris` records the editor URI for open files and
                // the canonical `file://` URI for lazily loaded ones;
                // check both spellings.
                let canonical_uri = crate::util::path_to_uri(&file_path);
                let parsed = self.parsed_uris.read();
                let loaded = parsed.contains(&uri_str) || parsed.contains(canonical_uri.as_str());
                drop(parsed);
                if !loaded {
                    continue;
                }
            }

            batch.php_changes.push((uri_str, file_path, change.kind));
        }
        batch
    }

    /// Re-index the PHP files of a watched batch and drop the
    /// class-derived caches they may have made stale.
    fn reindex_watched_php_files(&self, php_changes: &[(String, PathBuf, FileChangeType)]) {
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
        if self.reindex_files_batch(php_changes) || touches_config {
            self.clear_not_found_caches();
            self.clear_resolved_class_cache();
            self.laravel.clear_config_facts();
            self.member_completion_cache.lock().clear();
        }
    }

    /// Refresh the reference index for the YAML, XML, and Twig files of a
    /// watched batch.
    fn reindex_watched_resource_files(
        &self,
        resource_changes: &[(String, PathBuf, FileChangeType)],
    ) {
        tracing::info!(
            "PHPantom: {} watched resource file(s) changed on disk, refreshing references",
            resource_changes.len()
        );
        for (uri, path, change_type) in resource_changes {
            if *change_type == FileChangeType::Deleted {
                self.clear_file_maps(uri);
                self.forget_twig_source(uri);
            } else if let Ok(content) = std::fs::read_to_string(path) {
                self.update_resource_symbol_index(uri, &content);
            }
        }
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
        // below can tell which way the discovery inputs moved.
        let previous_inputs = self.discovery_inputs();
        let was_diagnosing = self.diagnostics_enabled();

        match crate::config::load_config_from(root, self.workspace.global_config_path.as_deref()) {
            Ok(cfg) => self.set_config(cfg),
            Err(e) => {
                self.log_detached(
                    MessageType::Warning,
                    format!("Failed to reload .phpantom.toml: {}", e),
                );
                return;
            }
        }

        // Resolved classes and completions may depend on config-driven
        // behaviour (e.g. `report-magic-properties`), so both must be
        // recomputed against the new settings rather than served stale.
        self.clear_resolved_class_cache();
        self.member_completion_cache.lock().clear();

        // Switching workspace diagnostics on or off is the one setting
        // whose consumer has already run (or is still running) by the
        // time a reload lands, so both directions need handling here
        // rather than merely being read the next time something asks.
        self.start_workspace_diagnostics_on_reload();
        self.stop_workspace_diagnostics_on_reload();

        // Open files keep what they were last shown until something
        // re-diagnoses them, so switching diagnostics off has to clear
        // them now and switching them back on has to fill them in.
        if self.diagnostics_enabled() != was_diagnosing {
            let open: Vec<String> = self.open_files.read().keys().cloned().collect();
            self.request_full_slow_pass(&open);
            for uri in open {
                self.schedule_diagnostics(uri);
            }
        }

        // An `[indexing] extensions` entry (e.g. Drupal's `.module`)
        // added to the config after startup needs its own watcher
        // registered, or the client never reports its file events and
        // the index keeps serving the last full scan.
        self.reregister_watched_files_if_changed();

        // An edit to `[indexing] exclude`, `extensions` or `include`
        // changes what belongs in the index, not merely what the next scan
        // will see.
        self.reconcile_index_for_input_change(&previous_inputs);
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
            if let Some(path) = crate::util::url_to_path(&change.uri)
                && let Some(as_indexed) = links.to_link_spelling(&path)
                && let Some(uri) = crate::util::path_to_url(&as_indexed)
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
        let is_laravel = self.resolved_class_cache.read().laravel().is_enabled();
        let is_symfony = self.is_symfony_project();
        let is_twig = self.is_twig_project();

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
        if is_symfony {
            patterns.push(("**/*.{xlf,xliff}".to_string(), watch_all));
        }
        if is_twig {
            patterns.push(("**/*.{twig,html,htm}".to_string(), watch_all));
        }
        if is_laravel {
            patterns.extend([
                ("**/*.json".to_string(), watch_all),
                ("**/*.sql".to_string(), watch_all),
                ("**/config/database.php".to_string(), watch_all),
            ]);
        }

        let mut watchers: Vec<FileSystemWatcher> = patterns
            .iter()
            .map(|(pattern, kind)| FileSystemWatcher {
                glob_pattern: GlobPattern::Pattern(pattern.clone()),
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
            let Some(base) = crate::util::path_to_url(link) else {
                continue;
            };
            watchers.extend(patterns.iter().map(|(pattern, kind)| FileSystemWatcher {
                glob_pattern: GlobPattern::RelativePattern(RelativePattern {
                    base_uri: BaseUri::Uri(base.clone()),
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
                is_symfony,
                is_twig,
                followed_links,
            },
        )
    }

    /// The `workspace/didChangeWatchedFiles` registration `initialized`
    /// sends, or `None` for a client that did not opt in to dynamic
    /// registration of watchers
    /// ([`Self::supports_watched_files_dynamic_registration`]).
    ///
    /// Records what it was built from, so
    /// [`Self::reregister_watched_files_if_changed`] knows what the client
    /// was last told.
    pub(crate) fn initial_watched_file_registration(&self) -> Option<Registration> {
        if !self.caps().watched_files_dynamic_registration {
            return None;
        }
        let (registration, inputs) = self.build_watched_file_registration();
        *self.registered_watcher_state.write() = Some(inputs);
        Some(registration)
    }

    /// The followed links worth putting in a registration: none unless the
    /// client said it can match a pattern against a base URI, since a
    /// client that cannot would either ignore the watcher or fail to parse
    /// the registration that carries it, taking the workspace watchers
    /// down with it.
    fn watchable_followed_links(&self) -> Vec<std::path::PathBuf> {
        if !self.caps().relative_pattern_watchers {
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
    ///
    /// A client that did not opt in to dynamic registration of watchers
    /// has none to keep current, and is never sent one.
    pub(crate) fn reregister_watched_files_if_changed(&self) {
        if !self.caps().watched_files_dynamic_registration {
            return;
        }
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
        let backend = self.clone_for_blocking();
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
    /// [`RelativePattern`](tower_lsp_server::gen_lsp_types::RelativePattern) base is
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
#[path = "watch_tests.rs"]
mod tests;
