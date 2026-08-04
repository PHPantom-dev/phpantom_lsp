//! Watched-file change application.
//!
//! Applies a `workspace/didChangeWatchedFiles` batch to the symbol
//! indexes on a blocking thread.

use std::path::PathBuf;

use tower_lsp::lsp_types::*;

use crate::Backend;

impl Backend {
    /// Apply a `workspace/didChangeWatchedFiles` batch to the indexes.
    ///
    /// Returns `true` if any PHP file or composer change was acted on (so the
    /// caller can ask the editor to re-pull diagnostics).  Runs entirely on a
    /// blocking thread; it parses no files on the async runtime.
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
        let mut schema_full_rebuild = false;
        let mut migration_changes: Vec<(PathBuf, FileChangeType)> = Vec::new();
        let mut php_changes: Vec<(String, PathBuf, FileChangeType)> = Vec::new();
        let is_laravel = self.resolved_class_cache.read().is_laravel();
        {
            let open = self.open_files.read();
            let parsed = self.parsed_uris.read();
            let laravel_config = self.config().laravel;
            for change in &params.changes {
                let path_str = change.uri.path();
                if path_str.ends_with("/composer.json") || path_str.ends_with("/composer.lock") {
                    composer_changed = true;
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
                        migration_changes.push((file_path, change.typ));
                    } else {
                        schema_full_rebuild = true;
                    }
                    continue;
                }
                if !path_str.ends_with(".php") {
                    continue;
                }

                // Open files are already tracked via did_open/did_change.
                let uri_str = change.uri.to_string();
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
            && !composer_changed
            && !schema_full_rebuild
            && migration_changes.is_empty()
        {
            return false;
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
            *self.laravel_aliases.write() = None;
            self.member_completion_cache.lock().clear();
        }

        if composer_changed {
            tracing::info!("PHPantom: composer files changed, rescanning vendor");
            self.rescan_composer_indexes(root);
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

        let backend = Backend::new_test();
        backend.resolved_class_cache.write().set_laravel(false);
        let params = DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Url::from_file_path(&schema).unwrap(),
                typ: FileChangeType::CREATED,
            }],
        };

        assert!(!backend.apply_watched_file_changes(&params, dir.path()));
    }
}
