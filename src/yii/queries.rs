//! `yiisoft/db` queries: the tables and columns a query names as strings.
//!
//! ```php
//! $query->from(Table::POST . ' p')
//!     ->leftJoin('user u', 'u.id = p.author_id')
//!     ->where(['p.status' => PostStatus::Published])
//!     ->orderBy(['p.publication_date' => SORT_DESC]);
//! ```
//!
//! `from()` and the joins name tables, as a string with an optional alias
//! (`'post p'`, `'post AS p'`), as an array keyed by alias
//! (`['p' => 'post']`), or as a class constant that holds the name, with
//! the alias appended (`Table::POST . ' p'`).  The conditions, the order,
//! the grouping, and the selection name columns, as array keys or values or
//! as a list in a string (`'id, name DESC'`), each qualified by an alias or
//! a table the query names, or by none.  A query for an Active Record reads
//! from the record's table unless it sets its own `FROM`.
//!
//! The tables and their columns are the ones the project's migrations
//! create (see [`super::migrations`]).  Symbol-map extraction
//! (`symbol_map/extraction/yii_queries.rs`) reads a chain of calls whole,
//! and keeps the tables it names as a [`YiiQuerySite`] and each name as a
//! [`YiiQueryName`], since the extractor cannot see types; this resolves the
//! chain's receiver and keeps only a query.
//!
//! The tables a query names in another statement than the one that names a
//! column are not followed, so such a column is read against the record of
//! an Active Record query, or not at all.
//!
//! [`YiiQuerySite`]: crate::symbol_map::YiiQuerySite
//! [`YiiQueryName`]: crate::symbol_map::YiiQueryName

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Hover, Location};

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::symbol_map::{SymbolMap, YiiQueryName, YiiQueryNameKind, YiiQuerySite, YiiTableName};
use crate::type_engine::resolver::{ResolutionCtx, resolve_target_classes};
use crate::types::AccessKind;

use super::active_record::{is_query, query_model, record_table};
use super::migrations::{Column, Schema, bare_table_name};

/// The interface every `yiisoft/db` query implements, Active Record
/// queries included.
const QUERY_INTERFACE: &str = "Yiisoft\\Db\\Query\\QueryInterface";

/// What an argument of a query method names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryArgument {
    /// The tables `from()` reads from, which replace the `FROM`.
    From,
    /// The table a join joins.
    JoinTable,
    /// A join's `ON` condition, whose array keys and values both name
    /// columns.
    JoinOn,
    /// A condition: the keys of a hash (`['status' => 1]`), or the column
    /// an operator compares (`['>', 'rating', 3]`).
    Condition,
    /// An order: the keys of an array, or a list in a string.
    Order,
    /// A list of columns: the values of an array, or a list in a string.
    Columns,
    /// A single column.
    Column,
    /// The alias an Active Record query gives its record's table.
    Alias,
}

/// What the argument at `index` of a call to the query method `method`
/// names.  The receiver still has to be a query for it to name anything.
pub(crate) fn query_argument(method: &str, index: usize) -> Option<QueryArgument> {
    const CONDITIONS: &[&str] = &[
        "where",
        "andWhere",
        "orWhere",
        "filterWhere",
        "andFilterWhere",
        "orFilterWhere",
        "having",
        "andHaving",
        "orHaving",
        "filterHaving",
    ];
    const JOINS: &[&str] = &["innerJoin", "leftJoin", "rightJoin"];
    let is = |name: &str| name.eq_ignore_ascii_case(method);
    let any = |names: &[&str]| names.iter().any(|name| is(name));
    if is("join") {
        return match index {
            1 => Some(QueryArgument::JoinTable),
            2 => Some(QueryArgument::JoinOn),
            _ => None,
        };
    }
    if any(JOINS) {
        return match index {
            0 => Some(QueryArgument::JoinTable),
            1 => Some(QueryArgument::JoinOn),
            _ => None,
        };
    }
    if index != 0 {
        return None;
    }
    if is("from") {
        Some(QueryArgument::From)
    } else if any(CONDITIONS) {
        Some(QueryArgument::Condition)
    } else if any(&["orderBy", "addOrderBy"]) {
        Some(QueryArgument::Order)
    } else if any(&["select", "addSelect", "groupBy", "addGroupBy"]) {
        Some(QueryArgument::Columns)
    } else if any(&[
        "indexBy",
        "andFilterCompare",
        "sum",
        "average",
        "min",
        "max",
    ]) {
        Some(QueryArgument::Column)
    } else if is("alias") {
        Some(QueryArgument::Alias)
    } else {
        None
    }
}

/// Whether the query method `method` takes any argument that names a table
/// or a column.
pub(crate) fn takes_names(method: &str) -> bool {
    query_argument(method, 0).is_some() || query_argument(method, 1).is_some()
}

/// Whether the query method `method` joins tables it does not name: an
/// Active Record query's `joinWith()` joins the tables of relations.
pub(crate) fn joins_unnamed_tables(method: &str) -> bool {
    method.eq_ignore_ascii_case("joinWith") || method.eq_ignore_ascii_case("innerJoinWith")
}

// ─── Text ──────────────────────────────────────────────────────────────────

/// Whether `name` is a plain SQL identifier.
pub(crate) fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A table as `from()` and the joins write it in a string: `post`,
/// `{{%post}}`, `post p`, or `post AS p`.  The table's name without the
/// prefix marker, the offset it starts at in `text`, and the alias.
/// `None` for a subquery, an expression, or a table in another schema.
pub(crate) fn table_text(text: &str) -> Option<(usize, &str, Option<&str>)> {
    if text.contains('(') {
        return None;
    }
    let mut words = text.split_whitespace();
    let table = words.next()?;
    let alias = match (words.next(), words.next(), words.next()) {
        (None, _, _) => None,
        (Some(alias), None, _) => Some(alias),
        (Some(r#as), Some(alias), None) if r#as.eq_ignore_ascii_case("as") => Some(alias),
        _ => return None,
    };
    if alias.is_some_and(|alias| !is_name(alias)) {
        return None;
    }
    let bare = bare_table_name(table);
    if !is_name(bare) {
        return None;
    }
    let at = table.as_ptr() as usize - text.as_ptr() as usize + table.find(bare)?;
    Some((at, bare, alias))
}

/// A column as a query writes it: `status`, or `p.status` qualified by an
/// alias or a table.  The qualifier, and the offset the column's own name
/// starts at in `text`.
pub(crate) fn column_text(text: &str) -> Option<(Option<&str>, usize, &str)> {
    match text.split_once('.') {
        Some((qualifier, name)) => (is_name(qualifier) && is_name(name)).then_some((
            Some(qualifier),
            qualifier.len() + 1,
            name,
        )),
        None => is_name(text).then_some((None, 0, text)),
    }
}

/// The columns a list in a string names (`'id, p.name DESC'`): the first
/// word of each item, where it is a column, and the offset it starts at.
pub(crate) fn column_list(text: &str) -> Vec<(usize, &str)> {
    let mut columns = Vec::new();
    let mut offset = 0;
    for item in text.split(',') {
        let trimmed = item.trim_start();
        let word = trimmed.split_whitespace().next().unwrap_or("");
        if column_text(word).is_some() {
            columns.push((offset + item.len() - trimmed.len(), word));
        }
        offset += item.len() + 1;
    }
    columns
}

/// The qualified columns (`p.author_id`) a fragment of SQL names, as a
/// join's `ON` or a condition writes it: each with the offset it starts
/// at.  An unqualified name cannot be told from a keyword, and is left
/// out, and so is anything in quotes.
pub(crate) fn qualified_columns(sql: &str) -> Vec<(usize, &str)> {
    let bytes = sql.as_bytes();
    let mut columns = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'\'' || byte == b'"' || byte == b'`' {
            i += 1;
            while i < bytes.len() && bytes[i] != byte {
                i += 1;
            }
            i += 1;
            continue;
        }
        if !(byte.is_ascii_alphanumeric() || byte == b'_') {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'.'))
        {
            i += 1;
        }
        let word = &sql[start..i];
        let after = sql[i..].trim_start();
        let parameter = start > 0 && bytes[start - 1] == b':';
        if !parameter
            && !after.starts_with('(')
            && word.contains('.')
            && column_text(word).is_some()
        {
            columns.push((start, word));
        }
    }
    columns
}

// ─── Lookup ────────────────────────────────────────────────────────────────

/// The tables a query reads from and joins, as far as they are known.
#[derive(Debug, Default)]
pub(crate) struct QueryTables {
    /// Each table's name, without the prefix marker, and its alias.
    tables: Vec<(String, Option<Atom>)>,
    /// Whether the query may read from a table not listed here.
    open: bool,
}

/// What a column a query names is.
pub(crate) enum ColumnLookup<'s> {
    /// A column of `table`.
    Found { table: &'s str, column: &'s Column },
    /// A column of none of the tables it may belong to, which are all known
    /// whole.
    Missing { tables: Vec<String> },
    /// A name that cannot be told, either way.
    Unknown,
}

impl QueryTables {
    /// The tables a column qualified by `qualifier` (or by none) may be a
    /// column of, and whether those are all of them.
    fn candidates<'a>(&'a self, qualifier: Option<&str>) -> (Vec<&'a str>, bool) {
        match qualifier {
            Some(qualifier) => {
                let by_alias: Vec<&str> = self
                    .tables
                    .iter()
                    .filter(|(name, alias)| match alias {
                        Some(alias) => *alias == qualifier,
                        None => name == qualifier,
                    })
                    .map(|(name, _)| name.as_str())
                    .collect();
                let complete = !by_alias.is_empty();
                (by_alias, complete)
            }
            None => (
                self.tables.iter().map(|(name, _)| name.as_str()).collect(),
                !self.open,
            ),
        }
    }

    /// What the column `name`, qualified by `qualifier`, is in `schema`.
    pub(crate) fn column<'s>(
        &self,
        schema: &'s Schema,
        qualifier: Option<&str>,
        name: &str,
    ) -> ColumnLookup<'s> {
        let (mut candidates, complete) = self.candidates(qualifier);
        // A qualifier that is no alias or table of the query may be a table
        // it names in another statement: its columns are found, but a name
        // that is not one of them is not told to be wrong.
        let mut all_known = complete;
        if candidates.is_empty()
            && let Some(qualifier) = qualifier
        {
            candidates.push(qualifier);
            all_known = false;
        }
        all_known &= !candidates.is_empty();
        for candidate in &candidates {
            let Some((table_name, table)) = schema.table_entry(candidate) else {
                all_known = false;
                continue;
            };
            if let Some(column) = table.column(name) {
                return ColumnLookup::Found {
                    table: table_name,
                    column,
                };
            }
            all_known &= schema.columns_known(table);
        }
        if all_known {
            ColumnLookup::Missing {
                tables: candidates.iter().map(|name| name.to_string()).collect(),
            }
        } else {
            ColumnLookup::Unknown
        }
    }

    /// The columns a column qualified by `qualifier` (or by none) may be,
    /// each with its table.
    pub(crate) fn columns<'s>(
        &self,
        schema: &'s Schema,
        qualifier: Option<&str>,
    ) -> Vec<(&'s str, &'s Column)> {
        let (mut candidates, _) = self.candidates(qualifier);
        if candidates.is_empty()
            && let Some(qualifier) = qualifier
        {
            candidates.push(qualifier);
        }
        let mut columns = Vec::new();
        for candidate in candidates {
            if let Some((table_name, table)) = schema.table_entry(candidate) {
                columns.extend(table.columns().iter().map(|column| (table_name, column)));
            }
        }
        columns
    }
}

impl Backend {
    /// The tables the query `site` reads from and joins, or `None` when its
    /// receiver is not a query.
    pub(crate) fn yii_query_tables(
        &self,
        site: &YiiQuerySite,
        subject: &str,
        rctx: &ResolutionCtx<'_>,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<QueryTables> {
        let mut is_db_query = false;
        let mut record_tables: Vec<Option<String>> = Vec::new();
        for resolved in resolve_target_classes(subject, AccessKind::Arrow, rctx) {
            let Some(query) = resolved.class_info.as_ref() else {
                continue;
            };
            if !crate::class_lookup::is_subtype_of(query, QUERY_INTERFACE, class_loader) {
                continue;
            }
            is_db_query = true;
            if is_query(query, class_loader) {
                record_tables.push(
                    query_model(&resolved.type_string)
                        .and_then(|model| class_loader(Fqn::new(model.trim_start_matches('\\'))))
                        .and_then(|record| record_table(&record, class_loader)),
                );
            }
        }
        if !is_db_query {
            return None;
        }
        let mut tables = QueryTables {
            tables: Vec::new(),
            open: site.open,
        };
        if !site.sets_from {
            if record_tables.is_empty() {
                // A query whose `FROM` another statement sets.
                tables.open = true;
            }
            for table in record_tables {
                match table {
                    Some(table) => tables.tables.push((table, site.alias)),
                    None => tables.open = true,
                }
            }
        }
        for table in &site.tables {
            let name = match &table.name {
                YiiTableName::Literal(name) => Some(name.to_string()),
                YiiTableName::Constant { class, constant } => self
                    .yii_constant_string(class, constant)
                    .map(|found| bare_table_name(&found.value).to_string()),
            };
            match name {
                Some(name) => tables.tables.push((name, table.alias)),
                None => tables.open = true,
            }
        }
        Some(tables)
    }

    /// The query name the file `uri` writes at `offset`, with the map that
    /// holds it.
    fn yii_query_name_at(&self, uri: &str, offset: u32) -> Option<(Arc<SymbolMap>, YiiQueryName)> {
        let map = self.symbol_map_for(uri)?;
        let name = map
            .yii_query_names
            .iter()
            .find(|name| name.start <= offset && offset <= name.end)?
            .clone();
        Some((map, name))
    }

    /// The tables of the query `name` is written in, resolved at the name,
    /// or `None` when the chain is not a query.
    fn yii_query_tables_for(
        &self,
        uri: &str,
        content: &str,
        map: &SymbolMap,
        name: &YiiQueryName,
    ) -> Option<QueryTables> {
        let site = map.yii_queries.get(name.query as usize)?;
        let source = map.source(content)?;
        let ctx = self.file_context(uri);
        let loaders = self.file_loaders(&ctx);
        let rctx = loaders.resolution_ctx_at(content, name.start);
        self.yii_query_tables(
            site,
            site.subject.as_str(source),
            &rctx,
            loaders.resolved_class_loader(),
        )
    }

    /// Go to the migration that creates the table or column a query names
    /// at `offset`.
    pub(crate) fn yii_query_definition(
        &self,
        uri: &str,
        content: &str,
        offset: u32,
    ) -> Option<Location> {
        let (map, name) = self.yii_query_name_at(uri, offset)?;
        let tables = self.yii_query_tables_for(uri, content, &map, &name)?;
        let text = content.get(name.start as usize..name.end as usize)?;
        let schema = self.yii_schema();
        match &name.kind {
            YiiQueryNameKind::Table => schema.table(text)?.location(),
            YiiQueryNameKind::Column { qualifier } => {
                match tables.column(&schema, qualifier.as_deref(), text) {
                    ColumnLookup::Found { column, .. } => column.location(),
                    _ => None,
                }
            }
        }
    }

    /// The hover of the table or column a query names at `offset`.
    pub(crate) fn yii_query_hover(&self, uri: &str, content: &str, offset: u32) -> Option<Hover> {
        let (map, name) = self.yii_query_name_at(uri, offset)?;
        let tables = self.yii_query_tables_for(uri, content, &map, &name)?;
        let text = content.get(name.start as usize..name.end as usize)?;
        let schema = self.yii_schema();
        let root = self.workspace.workspace_root.read().clone();
        let value = match &name.kind {
            YiiQueryNameKind::Table => {
                let table = schema.table(text)?;
                let mut value = format!("Table `{text}`");
                if let Some(file) = table.file() {
                    value.push_str(&format!(
                        "\n\nCreated in `{}`",
                        super::migrations::display_path(file, root.as_deref())
                    ));
                }
                value
            }
            YiiQueryNameKind::Column { qualifier } => {
                match tables.column(&schema, qualifier.as_deref(), text) {
                    ColumnLookup::Found { table, column } => column.detail(table, root.as_deref()),
                    _ => return None,
                }
            }
        };
        let mut hover = crate::hover::formatting::make_hover(value);
        hover.range = Some(crate::text_position::byte_range_to_lsp_range(
            content,
            name.start as usize,
            name.end as usize,
        ));
        Some(hover)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_is_read_with_its_alias() {
        assert_eq!(table_text("post"), Some((0, "post", None)));
        assert_eq!(table_text("post p"), Some((0, "post", Some("p"))));
        assert_eq!(table_text(" {{%post}} AS p"), Some((4, "post", Some("p"))));
        assert_eq!(table_text("public.post"), None);
        assert_eq!(table_text("(SELECT 1) t"), None);
        assert_eq!(table_text("post p q"), None);
    }

    #[test]
    fn a_column_is_read_with_its_qualifier() {
        assert_eq!(column_text("status"), Some((None, 0, "status")));
        assert_eq!(column_text("p.status"), Some((Some("p"), 2, "status")));
        assert_eq!(column_text("p.*"), None);
        assert_eq!(column_text("COUNT(*)"), None);
    }

    #[test]
    fn a_list_names_the_first_word_of_each_item() {
        assert_eq!(
            column_list("id, p.name DESC,COUNT(*) AS c"),
            vec![(0, "id"), (4, "p.name")]
        );
    }

    #[test]
    fn sql_names_its_qualified_columns() {
        assert_eq!(
            qualified_columns("u.id = p.author_id AND p.title <> 'a.b' AND x.f(1) AND :p.q"),
            vec![(0, "u.id"), (7, "p.author_id"), (23, "p.title")]
        );
    }

    #[test]
    fn query_methods_name_their_arguments() {
        assert_eq!(query_argument("from", 0), Some(QueryArgument::From));
        assert_eq!(query_argument("join", 1), Some(QueryArgument::JoinTable));
        assert_eq!(query_argument("leftJoin", 1), Some(QueryArgument::JoinOn));
        assert_eq!(
            query_argument("andWhere", 0),
            Some(QueryArgument::Condition)
        );
        assert_eq!(query_argument("andWhere", 1), None);
        assert!(takes_names("join"));
        assert!(!takes_names("limit"));
    }
}
