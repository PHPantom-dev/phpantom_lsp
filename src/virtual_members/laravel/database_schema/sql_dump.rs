use std::borrow::Cow;

use sqlparser::ast::{
    ColumnOption, DataType, Expr, GeneratedExpressionMode, ObjectNamePart,
    Statement as SqlStatement, Value,
};
use sqlparser::dialect::{GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

use super::*;

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

pub(super) fn find_matching_paren(sql: &str, open_paren: usize) -> Option<usize> {
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

pub(super) fn find_ascii_case_insensitive(
    haystack: &[u8],
    needle: &[u8],
    start: usize,
) -> Option<usize> {
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
