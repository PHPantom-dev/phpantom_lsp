use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::{LaravelMigrationsConfig, LaravelSchemaConfig};
use crate::virtual_members::laravel::config_values::{ConfigNode, parse_config_tree};

use super::*;

#[derive(Debug, Clone, Default)]
pub(super) struct DatabaseConfig {
    pub(super) default_connection: Option<String>,
    pub(super) connection_drivers: HashMap<String, String>,
}

pub(super) fn read_database_config(workspace_root: &Path) -> Option<DatabaseConfig> {
    let content = std::fs::read_to_string(workspace_root.join("config/database.php")).ok()?;
    let tree = parse_config_tree(&content)?;
    let default_connection = tree
        .value_at(&["default"])
        .and_then(|value| value.as_strings().0.into_iter().next());
    let mut connection_drivers = HashMap::new();
    if let Some(ConfigNode::Array(connections)) = tree.get(&["connections"]) {
        for (name, node) in connections {
            let Some(driver) = node
                .value_at(&["driver"])
                .and_then(|value| value.as_strings().0.into_iter().next())
            else {
                continue;
            };
            connection_drivers.insert(name.clone(), driver);
        }
    }
    Some(DatabaseConfig {
        default_connection,
        connection_drivers,
    })
}

pub(super) fn discover_schema_files(
    workspace_root: &Path,
    config: &LaravelSchemaConfig,
) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for configured in schema_paths(config) {
        let path = resolve_path(workspace_root, &configured);
        if path.is_file() {
            if is_schema_sql_file(&path) {
                files.push(path);
            }
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_schema_sql_file(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

pub(super) fn discover_migration_files(
    workspace_root: &Path,
    config: &LaravelMigrationsConfig,
) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if config.paths.is_empty() {
        collect_default_migration_files(workspace_root, &mut files)?;
    } else {
        for configured in &config.paths {
            collect_configured_migration_files(
                &resolve_path(workspace_root, configured),
                &mut files,
            )?;
        }
    }
    sort_migration_files(&mut files);
    files.dedup();
    Ok(files)
}

pub(super) fn load_migration_plan(
    workspace_root: &Path,
    config: &LaravelMigrationsConfig,
) -> std::io::Result<MigrationPlan> {
    let files = discover_migration_files(workspace_root, config)?
        .into_iter()
        .map(|path| {
            let basename = migration_basename(&path);
            let content = std::fs::read_to_string(&path)?;
            Ok(MigrationFile {
                path,
                basename,
                content,
            })
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    Ok(MigrationPlan { files })
}

pub(super) fn apply_migration_plan(index: &mut SchemaIndex, plan: &MigrationPlan) {
    for file in &plan.files {
        apply_migration_file(index, &file.content);
    }
}

fn sort_migration_files(files: &mut [PathBuf]) {
    files.sort_by(|left, right| {
        migration_basename(left)
            .cmp(&migration_basename(right))
            .then_with(|| left.cmp(right))
    });
}

pub(super) fn sort_migration_plan(files: &mut [MigrationFile]) {
    files.sort_by(|left, right| {
        left.basename
            .cmp(&right.basename)
            .then_with(|| left.path.cmp(&right.path))
    });
}

pub(super) fn migration_basename(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn collect_configured_migration_files(
    path: &Path,
    files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("php") {
            files.push(path.to_path_buf());
        }
        return Ok(());
    }
    if !path.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("php") {
            files.push(path);
        }
    }
    Ok(())
}

/// The workspace walk that finds `database/migrations/*.php` files when
/// no explicit migration paths are configured.
///
/// It starts at the workspace root and has to cross the whole project, so
/// it uses the `ignore` crate for the same reason the other workspace
/// walkers do (`classmap_scanner::collect_php_files_gitignore`):
/// generated and vendored trees are skipped rather than crawled, and
/// directory symlinks are not followed, so a link that points back at an
/// ancestor cannot spin the walk forever.  Entries that cannot be read
/// are skipped so one unreadable directory does not cost the project its
/// whole schema index.
///
/// `restrict_to`, when set, prunes everything outside that directory, so
/// the walk answers "would the full scan have reached this directory?"
/// without crossing the rest of the project.  Both callers share this
/// builder so the watcher cannot start accepting migrations the initial
/// scan skips.
fn migration_walk(workspace_root: &Path, restrict_to: Option<PathBuf>) -> ignore::Walk {
    let root = workspace_root.to_path_buf();
    ignore::WalkBuilder::new(workspace_root)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(true)
        .parents(true)
        .ignore(true)
        .follow_links(false)
        .filter_entry(move |entry| {
            let path = entry.path();
            if is_vendor_path(&root, path) {
                return false;
            }

            if let Some(target) = &restrict_to
                && !path.starts_with(target)
                && !target.starts_with(path)
            {
                return false;
            }

            // Migrations are read one level deep (as
            // `collect_configured_migration_files` does), so there is
            // nothing below a migrations directory worth descending into.
            !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !path.parent().is_some_and(is_database_migrations_dir)
        })
        .build()
}

fn is_walked_migration_file(entry: &ignore::DirEntry) -> bool {
    let path = entry.path();
    entry.file_type().is_some_and(|kind| kind.is_file())
        && path.parent().is_some_and(is_database_migrations_dir)
        && path.extension().and_then(|extension| extension.to_str()) == Some("php")
}

/// Find every `database/migrations/*.php` file in the workspace.
pub(super) fn collect_default_migration_files(
    workspace_root: &Path,
    files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in migration_walk(workspace_root, None).flatten() {
        if is_walked_migration_file(&entry) {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(())
}

/// Answers whether a watched migration file is one the initial scan would
/// also have found.
///
/// Without this the two paths disagree: the scan honours gitignore, hidden
/// and `.ignore` rules, while the watcher only looks at the path's shape,
/// so a migration in an ignored directory joined the schema index the
/// moment somebody edited it and vanished again on restart.
///
/// Each distinct migrations directory costs one pruned walk, so build one
/// of these per watched-file batch and reuse it across the batch.
#[derive(Default)]
pub struct MigrationDiscovery {
    directories: HashMap<PathBuf, Vec<PathBuf>>,
}

impl MigrationDiscovery {
    pub fn is_discoverable(
        &mut self,
        workspace_root: &Path,
        config: &LaravelMigrationsConfig,
        path: &Path,
    ) -> bool {
        // Configured paths are read with a plain `read_dir` on both the
        // scan and the watcher path, so those already agree.
        if !config.paths.is_empty() {
            return true;
        }
        let Some(directory) = path.parent() else {
            return false;
        };
        self.directories
            .entry(directory.to_path_buf())
            .or_insert_with(|| {
                migration_walk(workspace_root, Some(directory.to_path_buf()))
                    .flatten()
                    .filter(is_walked_migration_file)
                    .map(|entry| entry.path().to_path_buf())
                    .collect()
            })
            .iter()
            .any(|found| found == path)
    }
}

pub(super) fn schema_paths(config: &LaravelSchemaConfig) -> Vec<String> {
    if config.paths.is_empty() {
        vec!["database/schema".to_string()]
    } else {
        config.paths.clone()
    }
}

pub(super) fn is_schema_sql_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("sql" | "dump")
    )
}

pub fn is_migration_php_file(
    workspace_root: &Path,
    config: &LaravelMigrationsConfig,
    path: &Path,
) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("php") {
        return false;
    }
    if !config.paths.is_empty() {
        return config.paths.iter().any(|configured| {
            let configured = resolve_path(workspace_root, configured);
            path == configured || path.parent() == Some(configured.as_path())
        });
    }
    !is_vendor_path(workspace_root, path) && path.parent().is_some_and(is_database_migrations_dir)
}

fn is_database_migrations_dir(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("migrations")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some("database")
}

fn is_vendor_path(workspace_root: &Path, path: &Path) -> bool {
    path.strip_prefix(workspace_root)
        .ok()
        .is_some_and(|relative| {
            relative
                .components()
                .any(|part| part.as_os_str() == "vendor")
        })
}

pub(super) fn connection_name_from_schema_path(path: &Path) -> &str {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("default");
    stem.strip_suffix("-schema").unwrap_or(stem)
}

pub(super) fn resolve_path(workspace_root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}
