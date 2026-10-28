use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mago_syntax::cst::*;

use crate::config::LaravelConfig;
use crate::php_type::PhpType;
use crate::types::DatabaseColumnSource;

mod discovery;
pub use discovery::*;
mod sql_dump;
pub use sql_dump::*;
mod migrations;
use migrations::*;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Default)]
pub struct SchemaIndex {
    pub default_connection: Option<String>,
    pub connection_drivers: HashMap<String, String>,
    tables: HashMap<(String, String), SchemaTable>,
    blueprint_macros: HashMap<String, String>,
    base_tables: HashMap<(String, String), SchemaTable>,
    cached_plan: MigrationPlan,
    dump_paths: Vec<PathBuf>,
}

impl PartialEq for SchemaIndex {
    fn eq(&self, other: &Self) -> bool {
        self.default_connection == other.default_connection
            && self.connection_drivers == other.connection_drivers
            && self.tables == other.tables
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaTable {
    pub connection: String,
    pub name: String,
    pub columns: Vec<SchemaColumn>,
    column_lookup: HashMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaColumn {
    pub name: String,
    pub database_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub generated_expression: Option<String>,
    pub generated_mode: Option<String>,
    pub php_type: PhpType,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MigrationPlan {
    files: Vec<MigrationFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MigrationFile {
    path: PathBuf,
    basename: String,
    content: String,
}

impl SchemaIndex {
    #[cfg(test)]
    pub fn from_tables(default_connection: Option<String>, tables: Vec<SchemaTable>) -> Self {
        let mut index = Self {
            default_connection,
            connection_drivers: HashMap::new(),
            tables: HashMap::new(),
            blueprint_macros: HashMap::new(),
            base_tables: HashMap::new(),
            cached_plan: MigrationPlan::default(),
            dump_paths: Vec::new(),
        };
        for table in tables {
            index.insert_table(table);
        }
        index
    }

    pub fn watched_path_affects_schema(
        workspace_root: &Path,
        config: &LaravelConfig,
        path: &Path,
    ) -> bool {
        if path == workspace_root.join("config/database.php")
            || path == workspace_root.join(".phpantom.toml")
        {
            return true;
        }

        if config.migrations.enabled()
            && is_migration_php_file(workspace_root, &config.migrations, path)
        {
            return true;
        }

        if !config.schema.enabled() {
            return false;
        }

        if !is_schema_sql_file(path) {
            return false;
        }

        for configured in schema_paths(&config.schema) {
            let schema_path = resolve_path(workspace_root, &configured);
            if schema_path.is_file() || is_schema_sql_file(&schema_path) {
                if path == schema_path {
                    return true;
                }
            } else if path.parent() == Some(schema_path.as_path()) {
                return true;
            }
        }

        false
    }

    pub fn table(&self, connection: &str, table: &str) -> Option<&SchemaTable> {
        self.tables
            .get(&(connection.to_ascii_lowercase(), table.to_ascii_lowercase()))
    }

    /// The tables declared by committed schema dumps and migrations.
    pub(crate) fn tables(&self) -> impl Iterator<Item = &SchemaTable> {
        self.tables.values()
    }

    /// Already-discovered migration files, in migration order, with cached source.
    pub(crate) fn migration_sources(&self) -> impl DoubleEndedIterator<Item = (&Path, &str)> {
        self.cached_plan
            .files
            .iter()
            .map(|file| (file.path.as_path(), file.content.as_str()))
    }

    /// Schema dump files already read for one connection.
    pub(crate) fn dump_paths_for<'a>(
        &'a self,
        connection: &'a str,
    ) -> impl Iterator<Item = &'a Path> {
        self.dump_paths
            .iter()
            .filter(move |path| {
                connection_name_from_schema_path(path).eq_ignore_ascii_case(connection)
            })
            .map(PathBuf::as_path)
    }

    #[cfg(test)]
    pub fn column_source(
        &self,
        connection: &str,
        table: &str,
        column: &str,
    ) -> Option<DatabaseColumnSource> {
        let table = self.table(connection, table)?;
        table.column_source(column)
    }

    fn insert_table(&mut self, table: SchemaTable) {
        self.tables.insert(
            (
                table.connection.to_ascii_lowercase(),
                table.name.to_ascii_lowercase(),
            ),
            table,
        );
    }

    fn get_or_create_table(&mut self, connection: &str, table: &str) -> &mut SchemaTable {
        self.tables
            .entry((connection.to_ascii_lowercase(), table.to_ascii_lowercase()))
            .or_insert_with(|| {
                SchemaTable::new(connection.to_string(), table.to_string(), Vec::new())
            })
    }

    fn drop_table(&mut self, connection: &str, table: &str) {
        self.tables
            .remove(&(connection.to_ascii_lowercase(), table.to_ascii_lowercase()));
    }

    fn rename_table(&mut self, connection: &str, old_name: &str, new_name: &str) {
        let Some(mut table) = self.tables.remove(&(
            connection.to_ascii_lowercase(),
            old_name.to_ascii_lowercase(),
        )) else {
            return;
        };
        table.name = new_name.to_string();
        table.rebuild_column_lookup();
        self.insert_table(table);
    }

    pub fn update_migration_file(&mut self, path: &Path, content: String) {
        let mut found = false;
        for file in &mut self.cached_plan.files {
            if file.path == path {
                file.content = content.clone();
                found = true;
                break;
            }
        }
        if !found {
            let basename = migration_basename(path);
            self.cached_plan.files.push(MigrationFile {
                path: path.to_path_buf(),
                basename,
                content,
            });
            sort_migration_plan(&mut self.cached_plan.files);
        }
        self.rebuild_from_plan();
    }

    pub fn remove_migration_file(&mut self, path: &Path) -> bool {
        let before = self.cached_plan.files.len();
        self.cached_plan.files.retain(|f| f.path != path);
        if self.cached_plan.files.len() == before {
            return false;
        }
        self.rebuild_from_plan();
        true
    }

    fn rebuild_from_plan(&mut self) {
        self.tables = self.base_tables.clone();
        let plan = std::mem::take(&mut self.cached_plan);
        for file in &plan.files {
            apply_migration_file(self, &file.content);
        }
        self.cached_plan = plan;
    }
}

impl SchemaTable {
    pub fn new(connection: String, name: String, columns: Vec<SchemaColumn>) -> Self {
        let mut table = Self {
            connection,
            name,
            columns,
            column_lookup: HashMap::new(),
        };
        table.rebuild_column_lookup();
        table
    }

    pub fn column(&self, name: &str) -> Option<&SchemaColumn> {
        self.column_lookup
            .get(&name.to_ascii_lowercase())
            .and_then(|index| self.columns.get(*index))
    }

    pub fn column_source(&self, name: &str) -> Option<DatabaseColumnSource> {
        let column = self.column(name)?;
        Some(DatabaseColumnSource {
            connection: self.connection.clone(),
            table: self.name.clone(),
            column: column.name.clone(),
            database_type: column.database_type.clone(),
            nullable: column.nullable,
            default: column.default.clone(),
            generated_expression: column.generated_expression.clone(),
            generated_mode: column.generated_mode.clone(),
        })
    }

    fn rebuild_column_lookup(&mut self) {
        self.column_lookup.clear();
        self.column_lookup.reserve(self.columns.len());
        for (index, column) in self.columns.iter().enumerate() {
            self.column_lookup
                .insert(column.name.to_ascii_lowercase(), index);
        }
    }

    fn set_column(&mut self, column: SchemaColumn) {
        if let Some(index) = self
            .column_lookup
            .get(&column.name.to_ascii_lowercase())
            .copied()
        {
            self.columns[index] = column;
        } else {
            self.columns.push(column);
        }
        self.rebuild_column_lookup();
    }

    fn drop_column(&mut self, name: &str) {
        let lower = name.to_ascii_lowercase();
        self.columns
            .retain(|column| column.name.to_ascii_lowercase() != lower);
        self.rebuild_column_lookup();
    }

    fn rename_column(&mut self, old_name: &str, new_name: &str) {
        if let Some(index) = self
            .column_lookup
            .get(&old_name.to_ascii_lowercase())
            .copied()
        {
            self.columns[index].name = new_name.to_string();
            self.rebuild_column_lookup();
        }
    }
}

pub fn load_schema_index(
    workspace_root: &Path,
    config: &LaravelConfig,
    blueprint_macros: &HashMap<String, String>,
) -> std::io::Result<SchemaIndex> {
    if !config.schema.enabled() && !config.migrations.enabled() {
        return Ok(SchemaIndex::default());
    }

    let database_config = read_database_config(workspace_root);
    let default_connection = database_config
        .as_ref()
        .and_then(|c| c.default_connection.clone());
    let connection_drivers = database_config
        .map(|c| c.connection_drivers)
        .unwrap_or_default();

    let mut index = SchemaIndex {
        default_connection,
        connection_drivers,
        tables: HashMap::new(),
        blueprint_macros: blueprint_macros.clone(),
        base_tables: HashMap::new(),
        cached_plan: MigrationPlan::default(),
        dump_paths: Vec::new(),
    };

    if config.schema.enabled() {
        let schema_files = discover_schema_files(workspace_root, &config.schema)?;

        for path in schema_files {
            let connection = connection_name_from_schema_path(&path);
            let content = std::fs::read_to_string(&path)?;
            let driver = index.connection_drivers.get(connection).map(String::as_str);
            for mut table in parse_schema_dump_with_driver(connection, driver, &content) {
                table.connection = connection.to_string();
                index.insert_table(table);
            }
            index.dump_paths.push(path);
        }
    }

    index.base_tables = index.tables.clone();

    if config.migrations.enabled() {
        let plan = load_migration_plan(workspace_root, &config.migrations)?;
        apply_migration_plan(&mut index, &plan);
        index.cached_plan = plan;
    }
    Ok(index)
}

/// Whether a Blueprint method creates a column rather than an index or constraint.
pub(crate) fn blueprint_declares_column(method: &str) -> bool {
    migration_database_type(method).is_some()
}

pub fn database_type_to_php_type(database_type: &str, nullable: bool) -> PhpType {
    let lower = database_type.to_ascii_lowercase();
    let base = lower
        .split('(')
        .next()
        .unwrap_or(&lower)
        .trim()
        .trim_matches('`')
        .trim_matches('"');
    // Postgres array types (`integer[]`, `text[]`, ...) must be matched
    // before the scalar element checks below: the element name would
    // otherwise fall into the `int`/`float`/`bool` branches (e.g.
    // `integer[]` contains `int`) and lose the array-ness entirely.
    let ty = if base.ends_with("[]") || base == "array" || base.contains("json") {
        PhpType::array()
    } else if is_boolean_database_type(base, &lower) {
        PhpType::bool()
    } else if is_integer_database_type(base) {
        PhpType::int()
    } else if base.contains("double")
        || base.contains("float")
        || base.contains("real")
        || base.contains("decimal")
        || base.contains("numeric")
    {
        PhpType::float()
    } else {
        PhpType::string()
    };
    if nullable {
        PhpType::union(vec![ty, PhpType::null()]).simplified()
    } else {
        ty
    }
}

/// Whether a base SQL type is a boolean.
///
/// MySQL exposes `bool`/`boolean` as aliases for `tinyint(1)`, and
/// Laravel's `$table->boolean()` emits exactly `tinyint(1)` in a schema
/// dump, so a one-width tinyint is treated as a bool.  Wider tinyints
/// (`tinyint(3)`, `tinyint(4)`, ...) remain integers.
fn is_boolean_database_type(base: &str, lower: &str) -> bool {
    base == "bool"
        || base == "boolean"
        || base == "bit"
        || (base == "tinyint" && lower.contains("(1)"))
}

/// Whether a base SQL type is an integer.
///
/// `interval` and the spatial `point` family embed the substring `int`
/// (e.g. `point` ends with `int`) but are not integers, so they are
/// excluded before the broad substring test that catches `int`,
/// `integer`, `bigint`, `int4`, and friends.
fn is_integer_database_type(base: &str) -> bool {
    if base == "interval" || base.contains("point") {
        return false;
    }
    base.contains("int")
        || base == "serial"
        || base == "bigserial"
        || base == "smallserial"
        || base == "year"
}
