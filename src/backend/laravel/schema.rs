//! The database schema index: schema dumps and migrations, reloaded whole
//! or updated one migration at a time.

use std::path::PathBuf;

use tower_lsp_server::gen_lsp_types::{FileChangeType, MessageType};

use crate::Backend;

impl Backend {
    pub(crate) fn reload_laravel_schema_index(&self, root: &std::path::Path) {
        if !self.resolved_class_cache.read().laravel().is_enabled() {
            return;
        }

        let laravel_config = self.config().laravel;
        let index = if laravel_config.schema.enabled() || laravel_config.migrations.enabled() {
            let bp_macros = self.laravel.macros.read().blueprint_macro_closures();
            match crate::virtual_members::laravel::database_schema::load_schema_index(
                root,
                &laravel_config,
                &bp_macros,
            ) {
                Ok(index) => index,
                Err(err) => {
                    self.log_detached(
                        MessageType::Warning,
                        format!("Failed to reload Laravel schema dumps: {}", err),
                    );
                    return;
                }
            }
        } else {
            crate::virtual_members::laravel::database_schema::SchemaIndex::default()
        };

        self.resolved_class_cache
            .write()
            .laravel_mut()
            .set_schema_index(index.clone());
        *self.laravel.schema_index.write() = index;
        self.clear_resolved_class_cache();
        self.member_completion_cache.lock().clear();
    }

    pub(crate) fn update_laravel_migrations(&self, changes: &[(PathBuf, FileChangeType)]) {
        if !self.resolved_class_cache.read().laravel().is_enabled() {
            return;
        }

        // Read every changed migration before taking the index guard, so
        // lookups are not held up behind the file IO.
        // A file that cannot be read is left as the index has it.
        let mut contents: Vec<(&PathBuf, Option<String>)> = Vec::with_capacity(changes.len());
        for (path, change_type) in changes {
            let content = if *change_type == FileChangeType::Deleted {
                None
            } else {
                match std::fs::read_to_string(path) {
                    Ok(content) => Some(content),
                    Err(err) => {
                        self.log_detached(
                            MessageType::Warning,
                            format!("Failed to read migration file {}: {}", path.display(), err),
                        );
                        continue;
                    }
                }
            };
            contents.push((path, content));
        }

        let updated = {
            let mut index = self.laravel.schema_index.write();
            let mut any_changed = false;
            for (path, content) in contents {
                match content {
                    Some(content) => {
                        index.update_migration_file(path, content);
                        any_changed = true;
                    }
                    None => any_changed |= index.remove_migration_file(path),
                }
            }
            any_changed.then(|| index.clone())
        };
        if let Some(index) = updated {
            self.resolved_class_cache
                .write()
                .laravel_mut()
                .set_schema_index(index);
            self.clear_resolved_class_cache();
            self.member_completion_cache.lock().clear();
        }
    }
}
