//! The database table an Eloquent model maps to.

use crate::types::ClassInfo;
use crate::virtual_members::ResolvedClassCache;

use super::database_schema::SchemaTable;
use super::helpers::camel_to_snake;

/// The table a model maps to: its declared `$table`, or Eloquent's snake-case
/// plural of the class's short name.
///
/// Returns `None` when the model overrides `getTable()`, since the table is
/// then computed at runtime.  Used by the morph-map scanner to derive the
/// aliases of a `Relation::morphMap([Post::class, …])` list registration, which
/// Laravel keys by table name.
pub(crate) fn model_table_name(class: &ClassInfo) -> Option<String> {
    let laravel = class.laravel()?;
    if let Some(table) = laravel.table_name.clone() {
        return Some(table);
    }
    if laravel.has_get_table_method {
        return None;
    }
    Some(default_table_name(&class.name))
}

fn model_connection_and_table(
    class: &ClassInfo,
    cache: Option<&ResolvedClassCache>,
) -> Option<(String, String)> {
    let laravel = class.laravel()?;
    let cache_read = cache?.read();
    let schema = cache_read.laravel().schema_index();
    let connection = if let Some(connection) = laravel.connection_name.clone() {
        connection
    } else if laravel.has_get_connection_name_method {
        return None;
    } else {
        schema.default_connection.clone()?
    };
    let table = if let Some(table) = laravel.table_name.clone() {
        table
    } else if laravel.has_get_table_method {
        return None;
    } else {
        default_table_name(&class.name)
    };
    Some((connection, table))
}

pub(super) fn model_schema_table(
    class: &ClassInfo,
    cache: Option<&ResolvedClassCache>,
) -> Option<SchemaTable> {
    let (connection, table) = model_connection_and_table(class, cache)?;
    cache?
        .read()
        .laravel()
        .schema_index()
        .table(&connection, &table)
        .cloned()
}

fn default_table_name(class_name: &str) -> String {
    let short = class_name
        .rsplit('\\')
        .next()
        .unwrap_or(class_name)
        .rsplit('/')
        .next()
        .unwrap_or(class_name);
    pluralize_snake_table_name(&camel_to_snake(short))
}

fn pluralize_snake_table_name(name: &str) -> String {
    if let Some((prefix, last)) = name.rsplit_once('_') {
        return format!("{}_{}", prefix, pluralize_english_word(last));
    }
    pluralize_english_word(name)
}

fn pluralize_english_word(word: &str) -> String {
    if word.ends_with('y')
        && !matches!(word.chars().rev().nth(1), Some('a' | 'e' | 'i' | 'o' | 'u'))
    {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with('s')
        || word.ends_with('x')
        || word.ends_with('z')
        || word.ends_with("ch")
        || word.ends_with("sh")
    {
        format!("{}es", word)
    } else {
        format!("{}s", word)
    }
}
