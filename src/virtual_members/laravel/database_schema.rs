use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sqlparser::ast::{
    ColumnOption, DataType, Expr, GeneratedExpressionMode, ObjectNamePart, Statement, Value,
};
use sqlparser::dialect::{GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

use crate::config::LaravelSchemaConfig;
use crate::php_type::PhpType;
use crate::types::DatabaseColumnSource;
use crate::virtual_members::laravel::config_values::{ConfigNode, parse_config_tree};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SchemaIndex {
    pub default_connection: Option<String>,
    pub connection_drivers: HashMap<String, String>,
    tables: HashMap<(String, String), SchemaTable>,
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

impl SchemaIndex {
    #[cfg(test)]
    pub fn from_tables(default_connection: Option<String>, tables: Vec<SchemaTable>) -> Self {
        let mut index = Self {
            default_connection,
            connection_drivers: HashMap::new(),
            tables: HashMap::new(),
        };
        for table in tables {
            index.insert_table(table);
        }
        index
    }

    pub fn watched_path_affects_schema(
        workspace_root: &Path,
        config: &LaravelSchemaConfig,
        path: &Path,
    ) -> bool {
        if path == workspace_root.join("config/database.php")
            || path == workspace_root.join(".phpantom.toml")
        {
            return true;
        }

        if !is_schema_sql_file(path) {
            return false;
        }

        for configured in schema_paths(config) {
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
}

pub fn load_schema_index(
    workspace_root: &Path,
    config: &LaravelSchemaConfig,
) -> std::io::Result<SchemaIndex> {
    if !config.enabled() {
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
    };

    let schema_files = discover_schema_files(workspace_root, config)?;

    for path in schema_files {
        let connection = connection_name_from_schema_path(&path);
        let content = std::fs::read_to_string(&path)?;
        let driver = index.connection_drivers.get(connection).map(String::as_str);
        for mut table in parse_schema_dump_with_driver(connection, driver, &content) {
            table.connection = connection.to_string();
            index.insert_table(table);
        }
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

fn schema_paths(config: &LaravelSchemaConfig) -> Vec<String> {
    if config.paths.is_empty() {
        vec!["database/schema".to_string()]
    } else {
        config.paths.clone()
    }
}

fn is_schema_sql_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("sql")
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

fn table_from_statement(connection: &str, statement: Statement) -> Option<SchemaTable> {
    let Statement::CreateTable(create_table) = statement else {
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
    let ty = if base.contains("bigint")
        || base.contains("int")
        || base == "serial"
        || base == "bigserial"
        || base == "smallserial"
        || base == "year"
    {
        PhpType::int()
    } else if base.contains("double")
        || base.contains("float")
        || base.contains("real")
        || base.contains("decimal")
        || base.contains("numeric")
    {
        PhpType::float()
    } else if base.contains("bool") || base == "bit" {
        PhpType::bool()
    } else if base.contains("json") || base.ends_with("[]") || base == "array" {
        PhpType::array()
    } else {
        PhpType::string()
    };
    if nullable {
        PhpType::Union(vec![ty, PhpType::null()]).simplified()
    } else {
        ty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let index = load_schema_index(dir.path(), &LaravelSchemaConfig::default()).unwrap();
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
        let default_config = LaravelSchemaConfig::default();

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

        let custom_config = LaravelSchemaConfig {
            paths: vec!["extra/schema.sql".to_string(), "tenant/schema".to_string()],
            ..LaravelSchemaConfig::default()
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
    fn loads_checked_in_schema_dump_fixtures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema_dumps");
        let index = load_schema_index(&root, &LaravelSchemaConfig::default()).unwrap();

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
