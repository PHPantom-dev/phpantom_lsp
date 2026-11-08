//! What a relation an Active Record call names is a relation of, and where
//! it is declared (see [`super::active_record`]).

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Location, Position};

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::definition::point_location;
use crate::symbol_map::{SymbolMap, YiiRelationSite};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::type_engine::resolver::{ResolutionCtx, resolve_target_classes};
use crate::types::{AccessKind, ClassInfo};

use super::active_record::{
    Relation, RelationSite, is_query, is_record, query_model, record_relations, relation_path_owner,
};

/// The records a relation written against `subject` is a relation of:
/// the records among the classes the receiver resolves to, or, when
/// `on_query`, the records the queries among them are for, followed
/// along `path`.
pub(crate) fn relation_owners(
    subject: &str,
    on_query: bool,
    path: &str,
    rctx: &ResolutionCtx<'_>,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    let mut owners: Vec<Arc<ClassInfo>> = Vec::new();
    for resolved in resolve_target_classes(subject, AccessKind::Arrow, rctx) {
        let record = if on_query {
            if !resolved
                .class_info
                .as_ref()
                .is_some_and(|query| is_query(query, class_loader))
            {
                continue;
            }
            query_model(&resolved.type_string)
                .and_then(|model| class_loader(Fqn::new(model.trim_start_matches('\\'))))
        } else {
            resolved.class_info
        };
        let Some(record) = record.filter(|record| is_record(record, class_loader)) else {
            continue;
        };
        let Some(owner) = relation_path_owner(record, path, class_loader) else {
            continue;
        };
        if !owners.iter().any(|known| known.fqn() == owner.fqn()) {
            owners.push(owner);
        }
    }
    owners
}

impl Backend {
    /// The records the relation `site` of the file `uri` names a relation
    /// of.
    pub(crate) fn yii_relation_owners(
        &self,
        uri: &str,
        content: &str,
        map: &SymbolMap,
        site: &YiiRelationSite,
    ) -> Vec<Arc<ClassInfo>> {
        let Some(source) = map.source(content) else {
            return Vec::new();
        };
        let ctx = self.file_context(uri);
        let loaders = self.file_loaders(&ctx);
        let rctx = loaders.resolution_ctx_at(content, site.start);
        relation_owners(
            site.subject.as_str(source),
            site.on_query,
            &site.path,
            &rctx,
            loaders.resolved_class_loader(),
        )
    }

    /// Go to the declaration of the relation an Active Record call names
    /// at `position`.
    pub(crate) fn yii_relation_definition(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Location> {
        let map = self.symbol_map_for(uri)?;
        let offset = position_to_offset(content, position);
        let site = map
            .yii_relations
            .iter()
            .find(|site| site.start <= offset && offset <= site.end)?;
        let name = content.get(site.start as usize..site.end as usize)?;
        let ctx = self.file_context(uri);
        let class_loader = self.resolved_class_loader_with(&ctx.classes);
        self.yii_relation_owners(uri, content, &map, site)
            .iter()
            .find_map(|owner| {
                let relations = record_relations(owner, &class_loader)?;
                let relation = relations.iter().find(|relation| relation.name == name)?;
                self.yii_relation_location(relation, uri, content, &class_loader)
            })
    }

    /// Where `relation` is declared.
    pub(crate) fn yii_relation_location(
        &self,
        relation: &Relation,
        uri: &str,
        content: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<Location> {
        let (class_uri, class_content) =
            self.find_class_file_content(&relation.declaring_class, uri, content)?;
        let offset = match &relation.site {
            RelationSite::Arm(offset) => *offset,
            RelationSite::Getter(method) => {
                class_loader(Fqn::new(&relation.declaring_class))?
                    .methods
                    .iter()
                    .find(|candidate| candidate.name == *method)?
                    .name_offset
            }
        };
        Some(point_location(
            crate::util::parse_url(&class_uri)?,
            offset_to_position(&class_content, offset as usize),
        ))
    }
}
