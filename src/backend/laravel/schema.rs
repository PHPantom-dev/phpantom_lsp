//! The database schema index: schema dumps and migrations, reloaded whole
//! or updated one migration at a time.

use std::path::PathBuf;

use tower_lsp::lsp_types::FileChangeType;

use crate::Backend;

impl Backend {
    pub(crate) fn reload_laravel_schema_index(&self, root: &std::path::Path) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }

        let laravel_config = self.config().laravel;
        let index = if laravel_config.schema.enabled() || laravel_config.migrations.enabled() {
            let bp_macros = self.laravel_macros.read().blueprint_macro_closures();
            match crate::virtual_members::laravel::database_schema::load_schema_index(
                root,
                &laravel_config,
                &bp_macros,
            ) {
                Ok(index) => index,
                Err(err) => {
                    tracing::warn!("Failed to reload Laravel schema dumps: {}", err);
                    return;
                }
            }
        } else {
            crate::virtual_members::laravel::database_schema::SchemaIndex::default()
        };

        self.resolved_class_cache
            .write()
            .set_schema_index(index.clone());
        *self.schema_index.write() = index;
        self.resolved_class_cache.write().clear();
        self.member_completion_cache.lock().clear();
    }

    pub(crate) fn update_laravel_migrations(&self, changes: &[(PathBuf, FileChangeType)]) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }

        let mut index = self.schema_index.write();
        let mut any_changed = false;
        for (path, change_type) in changes {
            if *change_type == FileChangeType::DELETED {
                if index.remove_migration_file(path) {
                    any_changed = true;
                }
            } else {
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        index.update_migration_file(path, content);
                        any_changed = true;
                    }
                    Err(err) => {
                        tracing::warn!("Failed to read migration file {}: {}", path.display(), err);
                    }
                }
            }
        }
        if any_changed {
            self.resolved_class_cache
                .write()
                .set_schema_index(index.clone());
            self.resolved_class_cache.write().clear();
            self.member_completion_cache.lock().clear();
        }
    }
}
