//! The relations a Yii Active Record call names (see
//! [`crate::yii::active_record`]).
//!
//! `$post->relation('autor')` and `Post::query()->with('autor')` name a
//! relation the record does not declare, which the record throws for.  A
//! name is checked only against a record whose relations are all known: one
//! whose `relationQuery()` takes names this cannot read is left alone.

use tower_lsp_server::gen_lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::Backend;
use crate::diagnostics::helpers::{FileDiagnosticContext, make_diagnostic};
use crate::yii::active_record::record_relations;
use crate::yii::relations::relation_owners;

/// Diagnostic code for a relation an Active Record call names that the
/// record does not declare.
pub(crate) const INVALID_YII_RELATION_CODE: &str = "invalid_yii_relation";

impl Backend {
    /// Report the relations the file's Active Record calls name that the
    /// record they are a relation of does not declare.
    pub(crate) fn collect_yii_relation_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let map = &ctx.symbol_map;
        if map.yii_relations.is_empty() {
            return;
        }
        let Some(source) = map.source(content) else {
            return;
        };
        let loaders = self.file_loaders(&ctx.file);
        let class_loader = loaders.resolved_class_loader();
        for site in &map.yii_relations {
            if !super::incremental::reports_site(site.start) {
                continue;
            }
            let Some(name) = content.get(site.start as usize..site.end as usize) else {
                continue;
            };
            let rctx = loaders.resolution_ctx_at(content, site.start);
            let owners = relation_owners(
                site.subject.as_str(source),
                site.on_query,
                &site.path,
                &rctx,
                class_loader,
            );
            let Some(owner) = owners.iter().find(|owner| {
                record_relations(owner, class_loader)
                    .is_some_and(|relations| !relations.iter().any(|r| r.name == name))
            }) else {
                continue;
            };
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                site.start as usize,
                site.end as usize,
            ) else {
                continue;
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Warning,
                INVALID_YII_RELATION_CODE,
                format!(
                    "Relation '{name}' is not declared by record {}",
                    owner.fqn()
                ),
            ));
        }
    }
}
