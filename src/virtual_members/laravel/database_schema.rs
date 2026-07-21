use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use sqlparser::ast::{
    ColumnOption, DataType, Expr, GeneratedExpressionMode, ObjectNamePart,
    Statement as SqlStatement, Value,
};
use sqlparser::dialect::{GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

use crate::config::{LaravelConfig, LaravelMigrationsConfig, LaravelSchemaConfig};
use crate::php_type::PhpType;
use crate::types::DatabaseColumnSource;
use crate::virtual_members::laravel::config_values::{ConfigNode, parse_config_tree};

#[derive(Debug, Clone, Default)]
pub struct SchemaIndex {
    pub default_connection: Option<String>,
    pub connection_drivers: HashMap<String, String>,
    tables: HashMap<(String, String), SchemaTable>,
    blueprint_macros: HashMap<String, String>,
    base_tables: HashMap<(String, String), SchemaTable>,
    cached_plan: MigrationPlan,
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

#[derive(Debug, Clone, Default)]
struct DatabaseConfig {
    default_connection: Option<String>,
    connection_drivers: HashMap<String, String>,
}

fn read_database_config(workspace_root: &Path) -> Option<DatabaseConfig> {
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

fn discover_schema_files(
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

fn discover_migration_files(
    workspace_root: &Path,
    config: &LaravelMigrationsConfig,
) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if config.paths.is_empty() {
        collect_default_migration_files(workspace_root, workspace_root, &mut files)?;
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

fn load_migration_plan(
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

fn apply_migration_plan(index: &mut SchemaIndex, plan: &MigrationPlan) {
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

fn sort_migration_plan(files: &mut [MigrationFile]) {
    files.sort_by(|left, right| {
        left.basename
            .cmp(&right.basename)
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn migration_basename(path: &Path) -> String {
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

fn collect_default_migration_files(
    workspace_root: &Path,
    path: &Path,
    files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    if !path.is_dir() || is_vendor_path(workspace_root, path) {
        return Ok(());
    }
    if is_database_migrations_dir(path) {
        return collect_configured_migration_files(path, files);
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_default_migration_files(workspace_root, &path, files)?;
        }
    }
    Ok(())
}

fn schema_paths(config: &LaravelSchemaConfig) -> Vec<String> {
    if config.paths.is_empty() {
        vec!["database/schema".to_string()]
    } else {
        config.paths.clone()
    }
}

fn is_schema_sql_file(path: &Path) -> bool {
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

fn connection_name_from_schema_path(path: &Path) -> &str {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("default");
    stem.strip_suffix("-schema").unwrap_or(stem)
}

fn resolve_path(workspace_root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

#[cfg(test)]
pub fn parse_schema_dump(connection: &str, sql: &str) -> Vec<SchemaTable> {
    parse_schema_dump_with_driver(connection, None, sql)
}

pub fn parse_schema_dump_with_driver(
    connection: &str,
    driver: Option<&str>,
    sql: &str,
) -> Vec<SchemaTable> {
    let mut tables = Vec::new();
    let dialect = dialect_for_driver(driver);
    let sql = strip_psql_meta_commands(sql);
    for create_table in extract_create_table_statements(&sql) {
        let (create_table, requires_generic_dialect) =
            normalize_postgres_generated_columns(driver, create_table);
        let parsed = if requires_generic_dialect {
            Parser::parse_sql(&GenericDialect {}, &create_table)
        } else {
            Parser::parse_sql(dialect.as_ref(), &create_table)
        };
        let statements = match parsed {
            Ok(statements) => statements,
            Err(_) => continue,
        };
        for statement in statements {
            if let Some(table) = table_from_statement(connection, statement) {
                tables.push(table);
            }
        }
    }
    tables
}

fn normalize_postgres_generated_columns<'a>(
    driver: Option<&str>,
    sql: &'a str,
) -> (Cow<'a, str>, bool) {
    // Work around sqlparser-rs requiring STORED for PostgreSQL generated columns.
    // PostgreSQL 18 defaults omitted generated-column modes to VIRTUAL.
    // Remove this once https://github.com/apache/datafusion-sqlparser-rs/issues/2407 is fixed.
    let driver = driver.unwrap_or_default().to_ascii_lowercase();
    if !(driver.is_empty() || matches!(driver.as_str(), "pgsql" | "postgres" | "postgresql"))
        || find_ascii_case_insensitive(sql.as_bytes(), b"generated always as", 0).is_none()
    {
        return (Cow::Borrowed(sql), false);
    }

    let mut output = String::with_capacity(sql.len());
    let mut inserted_virtual_mode = false;
    let mut offset = 0usize;
    while let Some(start) =
        find_ascii_case_insensitive(sql.as_bytes(), b"generated always as", offset)
    {
        let Some(open_paren) = sql[start..].find('(').map(|rel| start + rel) else {
            break;
        };
        let Some(close_paren) = find_matching_paren(sql, open_paren) else {
            break;
        };
        let after_expression = close_paren + 1;
        output.push_str(&sql[offset..after_expression]);
        if !generated_clause_has_explicit_mode(&sql[after_expression..]) {
            output.push_str(" VIRTUAL");
            inserted_virtual_mode = true;
        }
        offset = after_expression;
    }
    output.push_str(&sql[offset..]);
    (Cow::Owned(output), inserted_virtual_mode)
}

fn generated_clause_has_explicit_mode(rest: &str) -> bool {
    let trimmed = rest.trim_start();
    trimmed
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("stored"))
        || trimmed
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("virtual"))
}

fn find_matching_paren(sql: &str, open_paren: usize) -> Option<usize> {
    let mut offset = open_paren;
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut dollar_quote: Option<String> = None;

    while offset < sql.len() {
        let rest = &sql[offset..];
        if let Some(tag) = &dollar_quote {
            if rest.starts_with(tag) {
                offset += tag.len();
                dollar_quote = None;
            } else {
                offset += rest.chars().next()?.len_utf8();
            }
            continue;
        }
        if let Some(q) = quote {
            let ch = rest.chars().next()?;
            offset += ch.len_utf8();
            if ch == q {
                quote = None;
            }
            continue;
        }
        if let Some(tag) = read_dollar_quote_tag(rest) {
            offset += tag.len();
            dollar_quote = Some(tag);
            continue;
        }

        let ch = rest.chars().next()?;
        if ch == '\'' || ch == '"' || ch == '`' {
            quote = Some(ch);
        } else if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(offset);
            }
        }
        offset += ch.len_utf8();
    }
    None
}

fn strip_psql_meta_commands(sql: &str) -> Cow<'_, str> {
    if !sql.lines().any(|line| line.trim_start().starts_with('\\')) {
        return Cow::Borrowed(sql);
    }

    let mut sanitized = String::with_capacity(sql.len());
    for line in sql.lines() {
        if line.trim_start().starts_with('\\') {
            continue;
        }
        sanitized.push_str(line);
        sanitized.push('\n');
    }
    Cow::Owned(sanitized)
}

fn dialect_for_driver(driver: Option<&str>) -> Box<dyn sqlparser::dialect::Dialect> {
    match driver.unwrap_or_default().to_ascii_lowercase().as_str() {
        "pgsql" | "postgres" | "postgresql" => Box::new(PostgreSqlDialect {}),
        "mysql" | "mariadb" => Box::new(MySqlDialect {}),
        "sqlite" => Box::new(SQLiteDialect {}),
        _ => Box::new(GenericDialect {}),
    }
}

fn table_from_statement(connection: &str, statement: SqlStatement) -> Option<SchemaTable> {
    let SqlStatement::CreateTable(create_table) = statement else {
        return None;
    };
    let name = object_name_part_value(create_table.name.0.last()?)?.to_string();
    let columns = create_table
        .columns
        .into_iter()
        .map(|column| {
            let database_type = database_type_name(&column.data_type);
            let nullable = !column.options.iter().any(|option| {
                matches!(
                    option.option,
                    ColumnOption::NotNull | ColumnOption::PrimaryKey(_)
                )
            });
            let default = column
                .options
                .iter()
                .find_map(|option| match &option.option {
                    ColumnOption::Default(expr) => Some(default_expr_to_string(expr)),
                    _ => None,
                });
            let (generated_expression, generated_mode) = column
                .options
                .iter()
                .find_map(|option| match &option.option {
                    ColumnOption::Generated {
                        generation_expr,
                        generation_expr_mode,
                        ..
                    } => Some((
                        generation_expr.as_ref().map(default_expr_to_string),
                        generation_expr_mode.as_ref().map(generated_mode_name),
                    )),
                    _ => None,
                })
                .unwrap_or((None, None));
            let php_type = database_type_to_php_type(&database_type, nullable);
            SchemaColumn {
                name: column.name.value,
                database_type,
                nullable,
                default,
                generated_expression,
                generated_mode,
                php_type,
            }
        })
        .collect();
    Some(SchemaTable::new(connection.to_string(), name, columns))
}

fn database_type_name(data_type: &DataType) -> String {
    data_type.to_string()
}

fn object_name_part_value(part: &ObjectNamePart) -> Option<&str> {
    match part {
        ObjectNamePart::Identifier(ident) => Some(ident.value.as_str()),
        _ => None,
    }
}

fn default_expr_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Value(value) => match &value.value {
            Value::SingleQuotedString(value) => format!("'{}'", value),
            Value::DoubleQuotedString(value) => format!("\"{}\"", value),
            Value::Number(value, _) => value.clone(),
            Value::Boolean(value) => value.to_string(),
            Value::Null => "null".to_string(),
            _ => expr.to_string(),
        },
        _ => expr.to_string(),
    }
}

fn generated_mode_name(mode: &GeneratedExpressionMode) -> String {
    match mode {
        GeneratedExpressionMode::Stored => "stored".to_string(),
        GeneratedExpressionMode::Virtual => "virtual".to_string(),
    }
}

fn extract_create_table_statements(sql: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut offset = 0usize;
    const CREATE_TABLE: &[u8] = b"create table";

    while offset < sql.len() {
        let Some(start) = find_ascii_case_insensitive(sql.as_bytes(), CREATE_TABLE, offset) else {
            break;
        };
        let Some(end) = find_statement_end(sql, start) else {
            break;
        };
        if let Some(statement) = sql.get(start..end) {
            statements.push(statement.trim());
        }
        offset = end;
    }

    statements
}

fn find_ascii_case_insensitive(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start >= haystack.len() || needle.len() > haystack.len() {
        return None;
    }
    let last_start = haystack.len().saturating_sub(needle.len());
    for index in start..=last_start {
        if haystack[index..index + needle.len()].eq_ignore_ascii_case(needle) {
            return Some(index);
        }
    }
    None
}

fn find_statement_end(sql: &str, start: usize) -> Option<usize> {
    let mut offset = start;
    let mut quote: Option<char> = None;
    let mut dollar_quote: Option<String> = None;

    while offset < sql.len() {
        let rest = &sql[offset..];

        if let Some(tag) = &dollar_quote {
            if rest.starts_with(tag) {
                offset += tag.len();
                dollar_quote = None;
                continue;
            }
            let ch = rest.chars().next()?;
            offset += ch.len_utf8();
            continue;
        }

        if let Some(q) = quote {
            let ch = rest.chars().next()?;
            offset += ch.len_utf8();
            if ch == q {
                quote = None;
            }
            continue;
        }

        if rest.starts_with("--") {
            offset = rest
                .find('\n')
                .map(|rel| offset + rel + 1)
                .unwrap_or(sql.len());
            continue;
        }
        if rest.starts_with("/*") {
            offset = rest
                .find("*/")
                .map(|rel| offset + rel + 2)
                .unwrap_or(sql.len());
            continue;
        }
        if let Some(tag) = read_dollar_quote_tag(rest) {
            offset += tag.len();
            dollar_quote = Some(tag);
            continue;
        }

        let ch = rest.chars().next()?;
        offset += ch.len_utf8();
        if ch == '\'' || ch == '"' || ch == '`' {
            quote = Some(ch);
        } else if ch == ';' {
            return Some(offset);
        }
    }

    Some(sql.len())
}

fn read_dollar_quote_tag(s: &str) -> Option<String> {
    if !s.starts_with('$') {
        return None;
    }
    let end = s[1..].find('$')? + 1;
    let tag = &s[..=end];
    if tag[1..tag.len() - 1]
        .chars()
        .all(|c| c == '_' || c.is_ascii_alphanumeric())
    {
        Some(tag.to_string())
    } else {
        None
    }
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
        PhpType::Union(vec![ty, PhpType::null()]).simplified()
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

fn apply_migration_file(index: &mut SchemaIndex, content: &str) {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"migration.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    for stmt in program.statements.iter() {
        apply_migration_stmt(index, stmt, content, None);
    }
}

fn apply_migration_stmt(
    index: &mut SchemaIndex,
    stmt: &Statement<'_>,
    content: &str,
    inherited_connection: Option<&str>,
) {
    match stmt {
        Statement::Namespace(ns) => {
            for stmt in ns.statements().iter() {
                apply_migration_stmt(index, stmt, content, inherited_connection);
            }
        }
        Statement::Class(class) => {
            apply_migration_class(index, class, content, inherited_connection)
        }
        Statement::Return(ret) => {
            if let Some(Expression::AnonymousClass(class)) = ret.value {
                apply_anonymous_migration_class(index, class, content, inherited_connection);
            }
        }
        _ => {}
    }
}

fn apply_migration_class(
    index: &mut SchemaIndex,
    class: &class_like::Class<'_>,
    content: &str,
    inherited_connection: Option<&str>,
) {
    let connection = migration_class_connection(class, content)
        .or_else(|| inherited_connection.map(str::to_string))
        .or_else(|| index.default_connection.clone())
        .unwrap_or_else(|| "default".to_string());
    apply_migration_members(index, class.members.iter(), content, &connection);
}

fn apply_anonymous_migration_class(
    index: &mut SchemaIndex,
    class: &AnonymousClass<'_>,
    content: &str,
    inherited_connection: Option<&str>,
) {
    let connection = migration_anonymous_class_connection(class, content)
        .or_else(|| inherited_connection.map(str::to_string))
        .or_else(|| index.default_connection.clone())
        .unwrap_or_else(|| "default".to_string());
    apply_migration_members(index, class.members.iter(), content, &connection);
}

fn apply_migration_members<'a>(
    index: &mut SchemaIndex,
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    content: &str,
    connection: &str,
) {
    for member in members {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let method_name = crate::atom::bytes_to_str(method.name.value);
        if method_name.eq_ignore_ascii_case("down") {
            continue;
        }
        if let MethodBody::Concrete(body) = &method.body {
            for stmt in body.statements.iter() {
                apply_migration_method_stmt(index, stmt, content, connection);
            }
        }
    }
}

fn migration_anonymous_class_connection(
    class: &AnonymousClass<'_>,
    content: &str,
) -> Option<String> {
    for member in class.members.iter() {
        if let Some(connection) = migration_connection_property(member, content) {
            return Some(connection);
        }
    }
    None
}

fn migration_class_connection(class: &class_like::Class<'_>, content: &str) -> Option<String> {
    for member in class.members.iter() {
        if let Some(connection) = migration_connection_property(member, content) {
            return Some(connection);
        }
    }
    None
}

fn migration_connection_property(member: &ClassLikeMember<'_>, content: &str) -> Option<String> {
    let ClassLikeMember::Property(Property::Plain(prop)) = member else {
        return None;
    };
    for item in prop.items.iter() {
        let PropertyItem::Concrete(concrete) = item else {
            continue;
        };
        if crate::atom::bytes_to_str(concrete.variable.name).trim_start_matches('$') != "connection"
        {
            continue;
        }
        if let Some(value) = string_literal_value(concrete.value, content) {
            return Some(value);
        }
    }
    None
}

fn apply_migration_method_stmt(
    index: &mut SchemaIndex,
    stmt: &Statement<'_>,
    content: &str,
    connection: &str,
) {
    match stmt {
        Statement::Expression(expr) => {
            apply_migration_expr(index, expr.expression, content, connection)
        }
        Statement::If(if_stmt) => {
            for stmt in if_stmt.body.statements() {
                apply_migration_method_stmt(index, stmt, content, connection);
            }
            for stmts in if_stmt.body.else_if_statements() {
                for stmt in stmts {
                    apply_migration_method_stmt(index, stmt, content, connection);
                }
            }
            if let Some(stmts) = if_stmt.body.else_statements() {
                for stmt in stmts {
                    apply_migration_method_stmt(index, stmt, content, connection);
                }
            }
        }
        Statement::Block(block) => {
            for stmt in block.statements.iter() {
                apply_migration_method_stmt(index, stmt, content, connection);
            }
        }
        _ => {}
    }
}

fn apply_migration_expr(
    index: &mut SchemaIndex,
    expr: &Expression<'_>,
    content: &str,
    connection: &str,
) {
    match expr {
        Expression::Call(Call::StaticMethod(call)) => {
            if let Some((method, args, call_connection)) =
                schema_static_call(call, content, connection)
            {
                apply_schema_call(index, &call_connection, method, args, content);
            }
        }
        Expression::Call(Call::Method(call)) => {
            if let Some((method, args, call_connection)) =
                schema_method_call(call, content, connection)
            {
                apply_schema_call(index, &call_connection, method, args, content);
            } else {
                apply_migration_expr(index, call.object, content, connection);
            }
        }
        _ => {}
    }
}

fn schema_static_call<'a>(
    call: &'a call::StaticMethodCall<'a>,
    _content: &str,
    connection: &str,
) -> Option<(&'a str, &'a ArgumentList<'a>, String)> {
    if !is_schema_class(call.class) {
        return None;
    }
    let ClassLikeMemberSelector::Identifier(method) = &call.method else {
        return None;
    };
    let method = crate::atom::bytes_to_str(method.value);
    if matches!(method, "connection" | "setConnection") {
        return None;
    }
    Some((method, &call.argument_list, connection.to_string()))
}

fn schema_method_call<'a>(
    call: &'a call::MethodCall<'a>,
    content: &str,
    fallback_connection: &str,
) -> Option<(&'a str, &'a ArgumentList<'a>, String)> {
    let ClassLikeMemberSelector::Identifier(method) = &call.method else {
        return None;
    };
    let Expression::Call(Call::StaticMethod(static_call)) = call.object else {
        return None;
    };
    if !is_schema_class(static_call.class) {
        return None;
    }
    let ClassLikeMemberSelector::Identifier(connection_method) = &static_call.method else {
        return None;
    };
    let connection_method = crate::atom::bytes_to_str(connection_method.value);
    if !matches!(connection_method, "connection" | "setConnection") {
        return None;
    }
    let connection = static_call
        .argument_list
        .arguments
        .iter()
        .next()
        .and_then(|arg| string_literal_value(arg.value(), content))
        .unwrap_or_else(|| fallback_connection.to_string());
    Some((
        crate::atom::bytes_to_str(method.value),
        &call.argument_list,
        connection,
    ))
}

fn is_schema_class(expr: &Expression<'_>) -> bool {
    let Expression::Identifier(identifier) = expr else {
        return false;
    };
    let name = crate::atom::bytes_to_str(identifier.value());
    matches!(
        name.trim_start_matches('\\'),
        "Schema" | "Illuminate\\Support\\Facades\\Schema"
    )
}

fn apply_schema_call(
    index: &mut SchemaIndex,
    connection: &str,
    method: &str,
    args: &ArgumentList<'_>,
    content: &str,
) {
    match method {
        "create" | "table" => {
            let Some(table_name) = args
                .arguments
                .iter()
                .next()
                .and_then(|arg| string_literal_value(arg.value(), content))
            else {
                return;
            };
            if method == "create" {
                index.insert_table(SchemaTable::new(
                    connection.to_string(),
                    table_name.clone(),
                    Vec::new(),
                ));
            }
            for arg in args.arguments.iter().skip(1) {
                if let Expression::Closure(closure) = arg.value() {
                    process_blueprint_closure(index, connection, &table_name, closure, content);
                    break;
                }
            }
        }
        "drop" | "dropIfExists" => {
            if let Some(table_name) = args
                .arguments
                .iter()
                .next()
                .and_then(|arg| string_literal_value(arg.value(), content))
            {
                index.drop_table(connection, &table_name);
            }
        }
        "rename" => {
            let strings = string_args_from_arguments(args, content);
            if strings.len() >= 2 {
                index.rename_table(connection, &strings[0], &strings[1]);
            }
        }
        _ => {}
    }
}

fn process_blueprint_closure(
    index: &mut SchemaIndex,
    connection: &str,
    table_name: &str,
    closure: &function_like::closure::Closure<'_>,
    content: &str,
) {
    for stmt in closure.body.statements.iter() {
        process_blueprint_stmt(index, connection, table_name, stmt, content);
    }
}

fn process_blueprint_stmt(
    index: &mut SchemaIndex,
    connection: &str,
    table_name: &str,
    stmt: &Statement<'_>,
    content: &str,
) {
    match stmt {
        Statement::Expression(expr) => {
            if let Expression::Call(Call::Method(call)) = expr.expression {
                process_blueprint_call(index, connection, table_name, call, content);
            }
        }
        Statement::If(if_stmt) => {
            for stmt in if_stmt.body.statements() {
                process_blueprint_stmt(index, connection, table_name, stmt, content);
            }
            for stmts in if_stmt.body.else_if_statements() {
                for stmt in stmts {
                    process_blueprint_stmt(index, connection, table_name, stmt, content);
                }
            }
            if let Some(stmts) = if_stmt.body.else_statements() {
                for stmt in stmts {
                    process_blueprint_stmt(index, connection, table_name, stmt, content);
                }
            }
        }
        _ => {}
    }
}

fn process_blueprint_call(
    index: &mut SchemaIndex,
    connection: &str,
    table_name: &str,
    call: &call::MethodCall<'_>,
    content: &str,
) {
    let root = root_method_call(call);
    let ClassLikeMemberSelector::Identifier(method) = &root.method else {
        return;
    };
    let method = crate::atom::bytes_to_str(method.value);
    let chain = node_text(call, content).unwrap_or_default();
    let args = node_text(&root.argument_list, content).unwrap_or_default();
    if method == "after" {
        for arg in root.argument_list.arguments.iter() {
            if let Expression::Closure(closure) = arg.value() {
                process_blueprint_closure(index, connection, table_name, closure, content);
            }
        }
        return;
    }
    if let Some(closure_text) = index.blueprint_macros.get(method).cloned() {
        expand_blueprint_macro(index, connection, table_name, &closure_text);
        return;
    }
    apply_blueprint_statement(index, connection, table_name, method, args, chain);
}

fn expand_blueprint_macro(
    index: &mut SchemaIndex,
    connection: &str,
    table_name: &str,
    closure_text: &str,
) {
    let synthetic = format!("<?php $__fn = {closure_text};");
    let arena = LocalArena::new();
    let file_id = FileId::new(b"macro.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, synthetic.as_bytes());

    for stmt in program.statements.iter() {
        if let Statement::Expression(expr) = stmt {
            let assign_rhs = match expr.expression {
                Expression::Assignment(op) => Some(op.rhs),
                _ => None,
            };
            if let Some(Expression::Closure(closure)) = assign_rhs {
                let saved = std::mem::take(&mut index.blueprint_macros);
                for body_stmt in closure.body.statements.iter() {
                    process_blueprint_stmt(index, connection, table_name, body_stmt, &synthetic);
                }
                index.blueprint_macros = saved;
                return;
            }
        }
    }
}

fn root_method_call<'a>(call: &'a call::MethodCall<'a>) -> &'a call::MethodCall<'a> {
    let mut root = call;
    while let Expression::Call(Call::Method(parent)) = root.object {
        root = parent;
    }
    root
}

fn string_literal_value(expr: &Expression<'_>, content: &str) -> Option<String> {
    let Expression::Literal(literal::Literal::String(s)) = expr else {
        return None;
    };
    if let Some(value) = s.value {
        return Some(crate::atom::bytes_to_str(value).to_string());
    }
    let start = s.span.start.offset as usize + 1;
    let end = s.span.end.offset as usize - 1;
    if start <= end && end <= content.len() {
        Some(content[start..end].to_string())
    } else {
        None
    }
}

fn string_args_from_arguments(args: &ArgumentList<'_>, content: &str) -> Vec<String> {
    args.arguments
        .iter()
        .filter_map(|arg| string_literal_value(arg.value(), content))
        .collect()
}

fn node_text<'a>(node: &impl HasSpan, content: &'a str) -> Option<&'a str> {
    let span = node.span();
    let start = span.start.offset as usize;
    let end = span.end.offset as usize;
    content.get(start..end)
}

fn apply_blueprint_statement(
    index: &mut SchemaIndex,
    connection: &str,
    table_name: &str,
    method: &str,
    args: &str,
    chain: &str,
) {
    let nullable = chain_contains_nullable(chain);
    match method {
        "dropColumn" | "removeColumn" => {
            let table = index.get_or_create_table(connection, table_name);
            for column in string_args(args) {
                table.drop_column(&column);
            }
        }
        "renameColumn" => {
            let strings = string_args(args);
            if strings.len() >= 2 {
                let table = index.get_or_create_table(connection, table_name);
                table.rename_column(&strings[0], &strings[1]);
            }
        }
        "rename" => {
            if let Some(new_name) = first_string_arg(args) {
                index.rename_table(connection, table_name, &new_name);
            }
        }
        "timestamps" | "timestampsTz" | "nullableTimestamps" | "nullableTimestampsTz" => {
            let table = index.get_or_create_table(connection, table_name);
            table.set_column(migration_column("created_at", "TIMESTAMP", true));
            table.set_column(migration_column("updated_at", "TIMESTAMP", true));
        }
        "dropTimestamps" | "dropTimestampsTz" => {
            let table = index.get_or_create_table(connection, table_name);
            table.drop_column("created_at");
            table.drop_column("updated_at");
        }
        "softDeletes" | "softDeletesTz" | "softDeletesDatetime" => {
            let column = first_string_arg(args).unwrap_or_else(|| "deleted_at".to_string());
            let table = index.get_or_create_table(connection, table_name);
            table.set_column(migration_column(&column, "TIMESTAMP", true));
        }
        "dropSoftDeletes" | "dropSoftDeletesTz" => {
            let column = first_string_arg(args).unwrap_or_else(|| "deleted_at".to_string());
            let table = index.get_or_create_table(connection, table_name);
            table.drop_column(&column);
        }
        "rememberToken" => {
            let table = index.get_or_create_table(connection, table_name);
            table.set_column(migration_column_with_chain(
                "remember_token",
                "VARCHAR",
                nullable,
                chain,
            ))
        }
        "dropRememberToken" => {
            let table = index.get_or_create_table(connection, table_name);
            table.drop_column("remember_token")
        }
        "morphs" | "nullableMorphs" => {
            if let Some(prefix) = first_string_arg(args) {
                let nullable = nullable || method == "nullableMorphs";
                let table = index.get_or_create_table(connection, table_name);
                table.set_column(migration_column(
                    &format!("{prefix}_type"),
                    "VARCHAR",
                    nullable,
                ));
                table.set_column(migration_column(
                    &format!("{prefix}_id"),
                    "BIGINT",
                    nullable,
                ));
            }
        }
        "uuidMorphs" | "nullableUuidMorphs" => {
            if let Some(prefix) = first_string_arg(args) {
                let nullable = nullable || method == "nullableUuidMorphs";
                let table = index.get_or_create_table(connection, table_name);
                table.set_column(migration_column(
                    &format!("{prefix}_type"),
                    "VARCHAR",
                    nullable,
                ));
                table.set_column(migration_column(&format!("{prefix}_id"), "UUID", nullable));
            }
        }
        "dropMorphs" => {
            if let Some(prefix) = first_string_arg(args) {
                let table = index.get_or_create_table(connection, table_name);
                table.drop_column(&format!("{prefix}_type"));
                table.drop_column(&format!("{prefix}_id"));
            }
        }
        "addColumn" => {
            let strings = string_args(args);
            if strings.len() >= 2 {
                let database_type = migration_database_type(&strings[0]);
                let table = index.get_or_create_table(connection, table_name);
                table.set_column(migration_column_with_chain(
                    &strings[1],
                    database_type,
                    nullable,
                    chain,
                ));
            }
        }
        _ => {
            if let Some((column, database_type)) =
                migration_column_from_blueprint_method(method, args)
            {
                let table = index.get_or_create_table(connection, table_name);
                table.set_column(migration_column_with_chain(
                    &column,
                    database_type,
                    nullable,
                    chain,
                ));
            }
        }
    }
}

fn migration_column_from_blueprint_method<'a>(
    method: &'a str,
    args: &str,
) -> Option<(String, &'a str)> {
    let column = first_string_arg(args).or_else(|| default_column_name(method))?;
    Some((column, migration_database_type(method)))
}

fn default_column_name(method: &str) -> Option<String> {
    match method {
        "id" => Some("id".to_string()),
        "uuid" => Some("uuid".to_string()),
        "ulid" => Some("ulid".to_string()),
        "ipAddress" => Some("ip_address".to_string()),
        "macAddress" => Some("mac_address".to_string()),
        _ => None,
    }
}

fn migration_database_type(method: &str) -> &'static str {
    match method.to_ascii_lowercase().as_str() {
        "bigincrements" | "biginteger" | "foreignid" | "foreignidfor" | "id"
        | "unsignedbiginteger" => "BIGINT",
        "increments" | "integer" | "integerincrements" | "unsignedinteger" => "INTEGER",
        "mediumincrements" | "mediuminteger" | "unsignedmediuminteger" => "MEDIUMINT",
        "smallincrements" | "smallinteger" | "unsignedsmallinteger" => "SMALLINT",
        "tinyincrements" | "tinyinteger" | "unsignedtinyinteger" => "TINYINT",
        "boolean" => "BOOLEAN",
        "decimal" | "unsigneddecimal" => "DECIMAL",
        "double" => "DOUBLE",
        "float" => "FLOAT",
        "uuid" | "foreignuuid" => "UUID",
        "ulid" | "foreignulid" => "CHAR(26)",
        "ipaddress" | "macaddress" => "VARCHAR",
        "json" => "JSON",
        "jsonb" => "JSONB",
        "date" => "DATE",
        "datetime" | "datetimetz" => "DATETIME",
        "time" | "timetz" => "TIME",
        "timestamp" | "timestamptz" | "timestampstz" | "softdeletes" | "softdeletestz" => {
            "TIMESTAMP"
        }
        "year" => "YEAR",
        "binary" | "mediumbinary" | "largebinary" => "BLOB",
        "computed" => "MIXED",
        "enum" | "set" => "VARCHAR",
        "morphs" | "nullablemorphs" | "uuidmorphs" | "nullableuuidmorphs" => "MIXED",
        "string" | "char" | "tinytext" => "VARCHAR",
        "text" | "mediumtext" | "longtext" => "TEXT",
        _ => "TEXT",
    }
}

fn migration_column(name: &str, database_type: &str, nullable: bool) -> SchemaColumn {
    migration_column_with_chain(name, database_type, nullable, "")
}

fn migration_column_with_chain(
    name: &str,
    database_type: &str,
    nullable: bool,
    chain: &str,
) -> SchemaColumn {
    let (generated_expression, generated_mode) = generated_column_from_chain(chain);
    SchemaColumn {
        name: name.to_string(),
        database_type: database_type.to_string(),
        nullable,
        default: None,
        generated_expression,
        generated_mode,
        php_type: database_type_to_php_type(database_type, nullable),
    }
}

fn generated_column_from_chain(chain: &str) -> (Option<String>, Option<String>) {
    if let Some(expression) = chain_string_arg_after_method(chain, "virtualAs") {
        return (Some(expression), Some("virtual".to_string()));
    }
    if let Some(expression) = chain_string_arg_after_method(chain, "storedAs") {
        return (Some(expression), Some("stored".to_string()));
    }
    if let Some(expression) = chain_string_arg_after_method(chain, "computed") {
        return (Some(expression), None);
    }
    (None, None)
}

fn chain_string_arg_after_method(chain: &str, method: &str) -> Option<String> {
    let needle = format!("->{method}");
    let start = find_ascii_case_insensitive(chain.as_bytes(), needle.as_bytes(), 0)?;
    let open = skip_php_whitespace(chain, start + needle.len());
    if chain.as_bytes().get(open) != Some(&b'(') {
        return None;
    }
    let close = find_matching_paren(chain, open)?;
    first_string_arg(&chain[open + 1..close])
}

fn chain_contains_nullable(chain: &str) -> bool {
    let Some(nullable) = find_ascii_case_insensitive(chain.as_bytes(), b"->nullable", 0) else {
        return false;
    };
    let after = skip_php_whitespace(chain, nullable + "->nullable".len());
    if chain.as_bytes().get(after) != Some(&b'(') {
        return true;
    }
    let Some(close) = find_matching_paren(chain, after) else {
        return true;
    };
    !chain[after + 1..close].trim().eq_ignore_ascii_case("false")
}

fn first_string_arg(args: &str) -> Option<String> {
    string_args(args).into_iter().next()
}

fn string_args(args: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while offset < args.len() {
        let rest = &args[offset..];
        let Some(relative_quote) = rest.find(['\'', '"']) else {
            break;
        };
        let quote = offset + relative_quote;
        if let Some((value, consumed)) = parse_php_string(&args[quote..]) {
            values.push(value);
            offset = quote + consumed;
        } else {
            offset = quote + 1;
        }
    }
    values
}

fn parse_php_string(input: &str) -> Option<(String, usize)> {
    let quote = input.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let mut value = String::new();
    let mut escaped = false;
    for (index, ch) in input[quote.len_utf8()..].char_indices() {
        let consumed = quote.len_utf8() + index + ch.len_utf8();
        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return Some((value, consumed));
        }
        value.push(ch);
    }
    None
}

fn skip_php_whitespace(input: &str, mut offset: usize) -> usize {
    while let Some(ch) = input[offset..].chars().next() {
        if !ch.is_whitespace() {
            break;
        }
        offset += ch.len_utf8();
        if offset >= input.len() {
            break;
        }
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_type_to_php_type_maps_arrays_before_scalar_elements() {
        // Postgres array types must resolve to `array`, not the scalar
        // element type embedded in the name.
        assert_eq!(
            database_type_to_php_type("integer[]", false),
            PhpType::array()
        );
        assert_eq!(
            database_type_to_php_type("bigint[]", false),
            PhpType::array()
        );
        assert_eq!(
            database_type_to_php_type("numeric[]", false),
            PhpType::array()
        );
        assert_eq!(
            database_type_to_php_type("boolean[]", false),
            PhpType::array()
        );
        assert_eq!(database_type_to_php_type("text[]", false), PhpType::array());
    }

    #[test]
    fn database_type_to_php_type_does_not_treat_interval_as_int() {
        // `interval` embeds "int" but is a string type.
        assert_eq!(
            database_type_to_php_type("interval", false),
            PhpType::string()
        );
    }

    #[test]
    fn database_type_to_php_type_treats_tinyint_one_as_bool() {
        // `$table->boolean()` renders as `tinyint(1)` in a MySQL dump.
        assert_eq!(
            database_type_to_php_type("tinyint(1)", false),
            PhpType::bool()
        );
        // Wider tinyints stay integers.
        assert_eq!(
            database_type_to_php_type("tinyint(3)", false),
            PhpType::int()
        );
        assert_eq!(database_type_to_php_type("tinyint", false), PhpType::int());
        // Plain integer types are unaffected.
        assert_eq!(database_type_to_php_type("bigint", false), PhpType::int());
    }

    #[test]
    fn parses_postgres_create_table() {
        let sql = r#"
            CREATE TABLE public.users (
                id bigserial PRIMARY KEY,
                email character varying(255) NOT NULL,
                settings jsonb,
                active boolean DEFAULT false NOT NULL,
                CONSTRAINT users_email_unique UNIQUE (email)
            );
        "#;
        let tables = parse_schema_dump("pgsql", sql);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "users");
        assert_eq!(tables[0].columns[0].name, "id");
        assert_eq!(tables[0].columns[1].name, "email");
        assert_eq!(
            tables[0].columns[1].database_type.to_ascii_lowercase(),
            "character varying(255)"
        );
        assert!(!tables[0].columns[1].nullable);
        assert_eq!(
            tables[0].columns[2].php_type,
            PhpType::Union(vec![PhpType::array(), PhpType::null()]).simplified()
        );
    }

    #[test]
    fn parses_postgres_dump_with_restrict_and_generated_column() {
        let sql = r#"
            -- PostgreSQL database dump
            \restrict abc123

            CREATE TABLE public.users (
                id integer NOT NULL,
                first_name text NOT NULL,
                last_name text NOT NULL,
                name character varying(255) GENERATED ALWAYS AS (((first_name || ' '::text) || last_name)) NOT NULL
            );

            \unrestrict abc123
        "#;

        let tables = parse_schema_dump("pgsql", sql);
        assert_eq!(tables.len(), 1);
        let name = tables[0].column("name").expect("generated column");
        assert_eq!(
            name.database_type.to_ascii_lowercase(),
            "character varying(255)"
        );
        assert!(!name.nullable);
        assert_eq!(name.generated_mode.as_deref(), Some("virtual"));
        assert_eq!(
            name.generated_expression.as_deref(),
            Some("((first_name || ' '::TEXT) || last_name)")
        );
    }

    #[test]
    fn keeps_connections_separate() {
        let mut index = SchemaIndex::default();
        for table in parse_schema_dump("mysql", "CREATE TABLE users (id int NOT NULL);") {
            index.insert_table(table);
        }
        for table in parse_schema_dump("analytics", "CREATE TABLE users (uuid varchar(36));") {
            index.insert_table(table);
        }
        assert!(index.column_source("mysql", "users", "id").is_some());
        assert!(index.column_source("analytics", "users", "uuid").is_some());
        assert!(index.column_source("mysql", "users", "uuid").is_none());
    }

    #[test]
    fn parses_final_statement_without_trailing_semicolon() {
        let tables = parse_schema_dump(
            "primary",
            "CREATE TABLE users (id bigint NOT NULL, email varchar(255))",
        );
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "users");
        assert_eq!(tables[0].columns.len(), 2);
    }

    #[test]
    fn loads_laravel_schema_directory_and_database_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::create_dir_all(dir.path().join("database/schema")).unwrap();
        std::fs::write(
            dir.path().join("config/database.php"),
            r#"<?php return [
                'default' => env('DB_CONNECTION', 'primary'),
                'connections' => [
                    'primary' => ['driver' => 'pgsql'],
                    'analytics' => ['driver' => 'mysql'],
                ],
            ];"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("database/schema/primary-schema.sql"),
            "CREATE TABLE users (id bigint NOT NULL, email varchar(255));",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("database/schema/analytics-schema.sql"),
            "CREATE TABLE users (uuid varchar(36) NOT NULL);",
        )
        .unwrap();

        let index =
            load_schema_index(dir.path(), &LaravelConfig::default(), &HashMap::new()).unwrap();
        assert_eq!(index.default_connection.as_deref(), Some("primary"));
        assert_eq!(
            index.connection_drivers.get("primary").map(String::as_str),
            Some("pgsql")
        );
        assert!(index.column_source("primary", "users", "email").is_some());
        assert!(index.column_source("analytics", "users", "uuid").is_some());
    }

    #[test]
    fn watched_paths_detect_schema_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let default_config = LaravelConfig::default();

        assert!(SchemaIndex::watched_path_affects_schema(
            root,
            &default_config,
            &root.join("database/schema/primary-schema.sql"),
        ));
        assert!(SchemaIndex::watched_path_affects_schema(
            root,
            &default_config,
            &root.join("config/database.php"),
        ));
        assert!(SchemaIndex::watched_path_affects_schema(
            root,
            &default_config,
            &root.join(".phpantom.toml"),
        ));
        assert!(!SchemaIndex::watched_path_affects_schema(
            root,
            &default_config,
            &root.join("storage/debug.sql"),
        ));

        let custom_config = LaravelConfig {
            schema: LaravelSchemaConfig {
                paths: vec!["extra/schema.sql".to_string(), "tenant/schema".to_string()],
                ..LaravelSchemaConfig::default()
            },
            ..LaravelConfig::default()
        };
        assert!(SchemaIndex::watched_path_affects_schema(
            root,
            &custom_config,
            &root.join("extra/schema.sql"),
        ));
        assert!(SchemaIndex::watched_path_affects_schema(
            root,
            &custom_config,
            &root.join("tenant/schema/eagle-schema.sql"),
        ));
        assert!(!SchemaIndex::watched_path_affects_schema(
            root,
            &custom_config,
            &root.join("database/schema/primary-schema.sql"),
        ));
    }

    #[test]
    fn applies_laravel_migrations_over_schema_dump() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("database/schema")).unwrap();
        std::fs::create_dir_all(root.join("database/migrations")).unwrap();
        std::fs::write(
            root.join("database/schema/default-schema.sql"),
            "CREATE TABLE users (id bigint NOT NULL, old_name varchar(255), removed_at timestamp);",
        )
        .unwrap();
        std::fs::write(
            root.join("database/migrations/2024_01_01_000000_update_users.php"),
            r#"<?php
            return new class {
                public function up(): void
                {
                    Schema::table('users', function (Blueprint $table): void {
                        $table->string('email')->nullable(false);
                        $table->renameColumn('old_name', 'name');
                        $table->dropColumn('removed_at');
                    });
                }
            };
            "#,
        )
        .unwrap();

        let index = load_schema_index(root, &LaravelConfig::default(), &HashMap::new()).unwrap();
        assert!(index.column_source("default", "users", "id").is_some());
        assert!(index.column_source("default", "users", "email").is_some());
        assert!(index.column_source("default", "users", "name").is_some());
        assert!(
            index
                .column_source("default", "users", "old_name")
                .is_none()
        );
        assert!(
            index
                .column_source("default", "users", "removed_at")
                .is_none()
        );
    }

    #[test]
    fn discovers_direct_non_vendor_migrations_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("database/migrations/archive")).unwrap();
        std::fs::create_dir_all(root.join("modules/blog/database/migrations")).unwrap();
        std::fs::create_dir_all(root.join("vendor/package/database/migrations")).unwrap();
        std::fs::write(
            root.join("database/migrations/2024_01_01_000000_create_posts.php"),
            r#"<?php
            return new class {
                public function up(): void
                {
                    Schema::create('posts', function (Blueprint $table): void {
                        $table->id();
                        $table->string('title');
                    });
                }
            };
            "#,
        )
        .unwrap();
        std::fs::write(
            root.join("database/migrations/archive/2024_01_02_000000_create_archived.php"),
            r#"<?php return new class { public function up(): void { Schema::create('archived_posts', function (Blueprint $table): void { $table->id(); }); } };"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules/blog/database/migrations/2024_01_03_000000_create_comments.php"),
            r#"<?php return new class { public function up(): void { Schema::create('comments', function (Blueprint $table): void { $table->id(); }); } };"#,
        )
        .unwrap();
        std::fs::write(
            root.join("vendor/package/database/migrations/2024_01_04_000000_create_vendor.php"),
            r#"<?php return new class { public function up(): void { Schema::create('vendor_rows', function (Blueprint $table): void { $table->id(); }); } };"#,
        )
        .unwrap();

        let index = load_schema_index(root, &LaravelConfig::default(), &HashMap::new()).unwrap();
        assert!(index.column_source("default", "posts", "title").is_some());
        assert!(index.column_source("default", "comments", "id").is_some());
        assert!(
            index
                .column_source("default", "archived_posts", "id")
                .is_none()
        );
        assert!(
            index
                .column_source("default", "vendor_rows", "id")
                .is_none()
        );
    }

    #[test]
    fn applies_migrations_by_basename_across_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("database/migrations")).unwrap();
        std::fs::create_dir_all(root.join("modules/blog/database/migrations")).unwrap();
        std::fs::write(
            root.join("modules/blog/database/migrations/2024_01_01_000000_create_users.php"),
            r#"<?php
            return new class {
                public function up(): void
                {
                    Schema::create('users', function (Blueprint $table): void {
                        $table->id();
                        $table->string('name');
                    });
                }
            };
            "#,
        )
        .unwrap();
        std::fs::write(
            root.join("database/migrations/2024_02_01_000000_drop_name.php"),
            r#"<?php
            return new class {
                public function up(): void
                {
                    Schema::table('users', function (Blueprint $table): void {
                        $table->dropColumn('name');
                    });
                }
            };
            "#,
        )
        .unwrap();

        let index = load_schema_index(root, &LaravelConfig::default(), &HashMap::new()).unwrap();
        assert!(index.column_source("default", "users", "id").is_some());
        assert!(index.column_source("default", "users", "name").is_none());
    }

    #[test]
    fn applies_migration_table_drop_rename_and_connections() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("database/migrations")).unwrap();
        std::fs::write(
            root.join("database/migrations/2024_01_01_000000_connections.php"),
            r#"<?php
            return new class {
                protected $connection = 'tenant';

                public function up(): void
                {
                    Schema::create('drafts', function (Blueprint $table): void {
                        $table->id();
                        $table->boolean('published')->nullable();
                    });
                    Schema::rename('drafts', 'articles');
                    Schema::connection('analytics')->create('events', function (Blueprint $table): void {
                        $table->uuid('uuid');
                    });
                    Schema::connection('analytics')->dropIfExists('temporary_events');
                }
            };
            "#,
        )
        .unwrap();

        let index = load_schema_index(root, &LaravelConfig::default(), &HashMap::new()).unwrap();
        assert!(index.column_source("tenant", "drafts", "id").is_none());
        assert!(
            index
                .column_source("tenant", "articles", "published")
                .is_some()
        );
        assert!(index.column_source("analytics", "events", "uuid").is_some());
        assert!(index.column_source("tenant", "events", "uuid").is_none());
    }

    #[test]
    fn loads_checked_in_laravel_migration_fixtures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/laravel_migrations");
        let index = load_schema_index(&root, &LaravelConfig::default(), &HashMap::new()).unwrap();

        assert_schema_column(&index, "default", "users", "id", "BIGINT", false, None);
        assert_schema_column(&index, "default", "users", "name", "VARCHAR", false, None);
        assert_schema_column(&index, "default", "users", "email", "VARCHAR", true, None);
        assert!(
            index
                .column_source("default", "users", "address1")
                .is_none()
        );
        assert_schema_column(
            &index,
            "default",
            "users",
            "created_at",
            "TIMESTAMP",
            true,
            None,
        );
        assert_schema_column(
            &index,
            "default",
            "users",
            "updated_at",
            "TIMESTAMP",
            true,
            None,
        );
        assert_schema_column(
            &index,
            "default",
            "users",
            "deleted_at",
            "TIMESTAMP",
            true,
            None,
        );
        assert_schema_column(
            &index,
            "default",
            "users",
            "ip_address",
            "VARCHAR",
            false,
            None,
        );
        assert_schema_column(
            &index,
            "default",
            "users",
            "custom_mac_address",
            "VARCHAR",
            false,
            None,
        );
        assert_schema_column(&index, "default", "users", "uuid", "UUID", false, None);
        assert_schema_column(
            &index,
            "default",
            "users",
            "custom_ulid",
            "CHAR(26)",
            false,
            None,
        );

        let display_name = index
            .column_source("default", "users", "display_name")
            .expect("users.display_name should be generated");
        assert_eq!(display_name.generated_mode.as_deref(), Some("virtual"));
        assert_eq!(
            display_name.generated_expression.as_deref(),
            Some("concat(name, ' <', email, '>')")
        );

        let email_hash = index
            .column_source("default", "users", "email_hash")
            .expect("users.email_hash should be generated");
        assert_eq!(email_hash.generated_mode.as_deref(), Some("stored"));
        assert_eq!(
            email_hash.generated_expression.as_deref(),
            Some("md5(email)")
        );

        assert_schema_column(
            &index, "billing", "invoices", "total", "DECIMAL", false, None,
        );
        assert_schema_column(
            &index, "billing", "invoices", "metadata", "JSON", true, None,
        );
        assert_schema_column(
            &index,
            "analytics",
            "events",
            "occurred_at",
            "TIMESTAMP",
            true,
            None,
        );
    }

    #[test]
    fn incremental_migration_update_replays_from_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("database/schema")).unwrap();
        std::fs::create_dir_all(root.join("database/migrations")).unwrap();
        std::fs::write(
            root.join("database/schema/default-schema.sql"),
            "CREATE TABLE users (id bigint NOT NULL, email varchar(255));",
        )
        .unwrap();
        std::fs::write(
            root.join("database/migrations/2024_01_01_000000_add_name.php"),
            r#"<?php return new class { public function up(): void { Schema::table('users', function (Blueprint $table): void { $table->string('name'); }); } };"#,
        )
        .unwrap();

        let mut index =
            load_schema_index(root, &LaravelConfig::default(), &HashMap::new()).unwrap();
        assert!(index.column_source("default", "users", "id").is_some());
        assert!(index.column_source("default", "users", "email").is_some());
        assert!(index.column_source("default", "users", "name").is_some());
        assert!(index.column_source("default", "users", "age").is_none());

        index.update_migration_file(
            &root.join("database/migrations/2024_01_01_000000_add_name.php"),
            r#"<?php return new class { public function up(): void { Schema::table('users', function (Blueprint $table): void { $table->string('name'); $table->integer('age'); }); } };"#.to_string(),
        );
        assert!(index.column_source("default", "users", "name").is_some());
        assert!(index.column_source("default", "users", "age").is_some());
        assert!(index.column_source("default", "users", "email").is_some());

        let new_migration = root.join("database/migrations/2024_02_01_000000_drop_email.php");
        index.update_migration_file(
            &new_migration,
            r#"<?php return new class { public function up(): void { Schema::table('users', function (Blueprint $table): void { $table->dropColumn('email'); }); } };"#.to_string(),
        );
        assert!(index.column_source("default", "users", "email").is_none());
        assert!(index.column_source("default", "users", "name").is_some());

        index.remove_migration_file(&new_migration);
        assert!(index.column_source("default", "users", "email").is_some());
    }

    #[test]
    fn expands_blueprint_macros_during_migration() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("database/migrations")).unwrap();
        std::fs::write(
            root.join("database/migrations/2024_01_01_000000_create_orders.php"),
            r#"<?php
            return new class {
                public function up(): void
                {
                    Schema::create('orders', function (Blueprint $table): void {
                        $table->id();
                        $table->auditColumns();
                        $table->string('status');
                    });
                }
            };
            "#,
        )
        .unwrap();

        let mut macros = HashMap::new();
        macros.insert(
            "auditColumns".to_string(),
            r#"function () {
                $this->unsignedBigInteger('created_by')->nullable();
                $this->unsignedBigInteger('updated_by')->nullable();
                $this->timestamps();
            }"#
            .to_string(),
        );

        let index = load_schema_index(root, &LaravelConfig::default(), &macros).unwrap();
        assert_schema_column(&index, "default", "orders", "id", "BIGINT", false, None);
        assert_schema_column(
            &index, "default", "orders", "status", "VARCHAR", false, None,
        );
        assert_schema_column(
            &index,
            "default",
            "orders",
            "created_by",
            "BIGINT",
            true,
            None,
        );
        assert_schema_column(
            &index,
            "default",
            "orders",
            "updated_by",
            "BIGINT",
            true,
            None,
        );
        assert_schema_column(
            &index,
            "default",
            "orders",
            "created_at",
            "TIMESTAMP",
            true,
            None,
        );
        assert_schema_column(
            &index,
            "default",
            "orders",
            "updated_at",
            "TIMESTAMP",
            true,
            None,
        );
    }

    #[test]
    fn loads_checked_in_schema_dump_fixtures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema_dumps");
        let index = load_schema_index(&root, &LaravelConfig::default(), &HashMap::new()).unwrap();

        assert_eq!(index.default_connection.as_deref(), Some("primary"));
        assert_eq!(
            index.connection_drivers.get("primary").map(String::as_str),
            Some("pgsql")
        );
        assert_eq!(
            index
                .connection_drivers
                .get("analytics")
                .map(String::as_str),
            Some("mysql")
        );
        assert_eq!(
            index.connection_drivers.get("archive").map(String::as_str),
            Some("sqlite")
        );
        assert_eq!(
            index
                .connection_drivers
                .get("pgsql_types")
                .map(String::as_str),
            Some("pgsql")
        );
        assert_eq!(
            index
                .connection_drivers
                .get("mysql_types")
                .map(String::as_str),
            Some("mysql")
        );
        assert_eq!(
            index
                .connection_drivers
                .get("sqlite_types")
                .map(String::as_str),
            Some("sqlite")
        );

        let primary_email = index
            .column_source("primary", "users", "email")
            .expect("primary users.email should be loaded");
        assert_eq!(
            primary_email.database_type.to_ascii_lowercase(),
            "character varying(255)"
        );
        assert!(!primary_email.nullable);
        assert_eq!(primary_email.default, None);

        let primary_display_name = index
            .column_source("primary", "users", "display_name")
            .expect("primary users.display_name should be loaded");
        assert_eq!(
            primary_display_name.database_type.to_ascii_lowercase(),
            "text"
        );
        assert!(primary_display_name.nullable);
        assert_eq!(primary_display_name.default.as_deref(), Some("'Guest'"));

        let analytics_email = index
            .column_source("analytics", "users", "email")
            .expect("analytics users.email should be loaded independently");
        assert_eq!(
            analytics_email.database_type.to_ascii_lowercase(),
            "varchar(255)"
        );
        assert!(!analytics_email.nullable);

        let analytics_score = index
            .column_source("analytics", "users", "score")
            .expect("analytics users.score should be loaded");
        assert_eq!(
            analytics_score.database_type.to_ascii_lowercase(),
            "decimal(8,2)"
        );
        assert_eq!(analytics_score.default.as_deref(), Some("0.00"));

        let archive_archived_at = index
            .column_source("archive", "users", "archived_at")
            .expect("archive users.archived_at should be loaded");
        assert_eq!(
            archive_archived_at.database_type.to_ascii_lowercase(),
            "text"
        );
        assert_eq!(archive_archived_at.default.as_deref(), Some("null"));

        assert!(index.column_source("primary", "users", "score").is_none());
        assert!(
            index
                .column_source("analytics", "users", "display_name")
                .is_none()
        );

        assert_schema_column(
            &index,
            "pgsql_types",
            "pg_type_samples",
            "created_at",
            "TIMESTAMP WITHOUT TIME ZONE",
            false,
            Some("now()"),
        );
        assert_schema_column(
            &index,
            "pgsql_types",
            "pg_type_samples",
            "published_at",
            "TIMESTAMP WITH TIME ZONE",
            true,
            None,
        );
        assert_schema_column(
            &index,
            "pgsql_types",
            "pg_type_samples",
            "settings",
            "JSONB",
            true,
            Some("'{}'::jsonb"),
        );
        assert_schema_column(
            &index,
            "pgsql_types",
            "pg_type_samples",
            "tags",
            "TEXT[]",
            true,
            None,
        );
        assert_schema_column(
            &index,
            "mysql_types",
            "mysql_type_samples",
            "created_at",
            "TIMESTAMP",
            true,
            Some("CURRENT_TIMESTAMP"),
        );
        assert_schema_column(
            &index,
            "mysql_types",
            "mysql_type_samples",
            "payload",
            "JSON",
            true,
            Some("null"),
        );
        assert_schema_column(
            &index,
            "sqlite_types",
            "sqlite_type_samples",
            "created_at",
            "DATETIME",
            true,
            Some("current_timestamp"),
        );

        assert_eq!(
            index
                .table("pgsql_types", "pg_type_samples")
                .unwrap()
                .columns
                .len(),
            26
        );
        assert!(
            index
                .table("mysql_types", "mysql_secondary_samples")
                .is_some()
        );
        assert!(
            index
                .table("sqlite_types", "sqlite_secondary_samples")
                .is_some()
        );
    }

    fn assert_schema_column(
        index: &SchemaIndex,
        connection: &str,
        table: &str,
        column: &str,
        database_type: &str,
        nullable: bool,
        default: Option<&str>,
    ) {
        let source = index
            .column_source(connection, table, column)
            .unwrap_or_else(|| panic!("missing {connection}.{table}.{column}"));
        assert_eq!(
            source.database_type.to_ascii_lowercase(),
            database_type.to_ascii_lowercase(),
            "{connection}.{table}.{column} database type"
        );
        assert_eq!(
            source.nullable, nullable,
            "{connection}.{table}.{column} nullability"
        );
        assert_eq!(
            source.default.as_deref().map(str::to_ascii_lowercase),
            default.map(str::to_ascii_lowercase),
            "{connection}.{table}.{column} default"
        );
    }
}
