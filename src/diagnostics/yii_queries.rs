//! The tables and columns a `yiisoft/db` query names (see
//! [`crate::yii::queries`]).
//!
//! `$query->from('psot')` reads from a table no migration creates, and
//! `->where(['titel' => …])` compares a column none of the query's tables
//! has, either of which the database rejects.  A table is checked only when
//! the migrations name every table they create in a way that can be read,
//! and a column only against tables whose columns they say whole, and only
//! when the query names all the tables it may be a column of.

use tower_lsp_server::gen_lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::Backend;
use crate::diagnostics::helpers::{FileDiagnosticContext, make_diagnostic};
use crate::symbol_map::YiiQueryNameKind;
use crate::yii::queries::{ColumnLookup, QueryTables};

/// Diagnostic code for a table a query names that no migration creates.
pub(crate) const INVALID_YII_TABLE_CODE: &str = "invalid_yii_table";

/// Diagnostic code for a column a query names that none of its tables has.
pub(crate) const INVALID_YII_COLUMN_CODE: &str = "invalid_yii_column";

impl Backend {
    /// Report the tables and columns the file's queries name that the
    /// project's migrations do not create.
    pub(crate) fn collect_yii_query_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let map = &ctx.symbol_map;
        if map.yii_query_names.is_empty() {
            return;
        }
        let Some(source) = map.source(content) else {
            return;
        };
        let schema = self.yii_schema();
        let loaders = self.file_loaders(&ctx.file);
        let class_loader = loaders.resolved_class_loader();
        // The tables of each query, resolved once for all its names.
        let mut tables: Vec<Option<Option<QueryTables>>> = std::iter::repeat_with(|| None)
            .take(map.yii_queries.len())
            .collect();
        for name in &map.yii_query_names {
            if !super::incremental::reports_site(name.start) {
                continue;
            }
            let Some(site) = map.yii_queries.get(name.query as usize) else {
                continue;
            };
            let Some(text) = content.get(name.start as usize..name.end as usize) else {
                continue;
            };
            let resolved = tables[name.query as usize].get_or_insert_with(|| {
                let rctx = loaders.resolution_ctx_at(content, name.start);
                self.yii_query_tables(site, site.subject.as_str(source), &rctx, class_loader)
            });
            let Some(query_tables) = resolved.as_ref() else {
                continue;
            };
            let (code, message) = match &name.kind {
                YiiQueryNameKind::Table => {
                    if !schema.tables_known() || schema.table(text).is_some() {
                        continue;
                    }
                    (
                        INVALID_YII_TABLE_CODE,
                        format!("Table '{text}' is not created by any migration"),
                    )
                }
                YiiQueryNameKind::Column { qualifier } => {
                    let ColumnLookup::Missing { tables } =
                        query_tables.column(&schema, qualifier.as_deref(), text)
                    else {
                        continue;
                    };
                    let message = match tables.as_slice() {
                        [table] => format!("Column '{text}' is not a column of table '{table}'"),
                        _ => format!(
                            "Column '{text}' is not a column of any of the tables '{}'",
                            tables.join("', '")
                        ),
                    };
                    (INVALID_YII_COLUMN_CODE, message)
                }
            };
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                name.start as usize,
                name.end as usize,
            ) else {
                continue;
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Warning,
                code,
                message,
            ));
        }
    }
}
