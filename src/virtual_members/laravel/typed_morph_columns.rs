//! Confirm morph-column alias candidates through the shared type engine.

use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::{Expression, Node};

use crate::Backend;
use crate::php_type::{PhpType, TypeKind};
use crate::symbol_map::{MorphColumnReceiver, MorphColumnSite, SymbolMap, SymbolSpan};
use crate::type_engine::resolver::{Loaders, ResolutionCtx, VarResolutionCtx};
use crate::types::ClassInfo;

/// Confirmed spans with their class-index generation and immutable source map.
pub(crate) type MorphColumnSpans = (u64, Arc<SymbolMap>, Arc<Vec<SymbolSpan>>);

impl Backend {
    /// Resolve a file's candidate morph aliases once, retaining only comparisons
    /// against columns declared by the receiver model's `morphTo()` relations.
    pub(crate) fn morph_column_spans_for(
        &self,
        uri: &str,
        map: &Arc<SymbolMap>,
    ) -> Arc<Vec<SymbolSpan>> {
        if map.morph_column_sites.is_empty() {
            return empty_spans();
        }
        let content = self
            .blade_virtual_content
            .read()
            .get(uri)
            .map(|content| Arc::new(content.clone()))
            .or_else(|| self.get_file_content_arc(uri));
        let Some(content) = content.filter(|content| map.matches_source(content)) else {
            return empty_spans();
        };
        // A same-length alias edit can arrive before the background parse.
        if map.morph_column_sites.iter().any(|site| {
            content.get(site.start as usize..site.end as usize) != Some(site.key.as_str())
        }) {
            return empty_spans();
        }
        // Capture before resolving: a concurrent model edit must also retire
        // an answer that finishes computing after that edit invalidated caches.
        let generation = self.symbols.class_lookup_generation();
        if let Some((cached_generation, source, spans)) =
            self.morph_column_spans_cache.read().get(uri)
            && *cached_generation == generation
            && Arc::ptr_eq(source, map)
        {
            return Arc::clone(spans);
        }
        let spans = Arc::new(self.confirm_morph_columns(uri, &content, &map.morph_column_sites));
        self.morph_column_spans_cache.write().insert(
            uri.to_string(),
            (generation, Arc::clone(map), Arc::clone(&spans)),
        );
        spans
    }

    fn confirm_morph_columns(
        &self,
        uri: &str,
        content: &str,
        sites: &[MorphColumnSite],
    ) -> Vec<SymbolSpan> {
        let mut by_receiver: HashMap<(u32, u32), Vec<&MorphColumnSite>> = HashMap::new();
        for site in sites {
            by_receiver
                .entry((site.receiver_start, site.receiver_end))
                .or_default()
                .push(site);
        }
        let file_ctx = self.file_context(uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        let function_loader_cl = |name: &str, offset: u32| function_loader(name, offset);
        let default_class = ClassInfo::default();
        let mut confirmed = Vec::new();
        crate::parser::with_parsed_program(content, "morph_columns", |program, content| {
            visit_expressions(Node::Program(program), &mut |expr| {
                let span = expr.span();
                let Some(candidates) = by_receiver.get(&(span.start.offset, span.end.offset))
                else {
                    return;
                };
                let site = candidates[0];
                let offset = span.start.offset;
                let current_class =
                    crate::class_lookup::find_class_at_offset(&file_ctx.classes, offset)
                        .unwrap_or(&default_class);
                let var_ctx = VarResolutionCtx {
                    var_name: "",
                    top_level_scope: None,
                    current_class,
                    all_classes: &file_ctx.classes,
                    content,
                    cursor_offset: offset,
                    class_loader: &class_loader,
                    backend: Some(self),
                    loaders: Loaders::with_function(Some(&function_loader_cl)),
                    resolved_class_cache: Some(&self.resolved_class_cache),
                    enclosing_return_type: None,
                    branch_aware: false,
                    match_arm_narrowing: HashMap::new(),
                    scope_var_resolver: None,
                    scope_proofs: None,
                };
                let ty = if matches!(
                    expr,
                    Expression::Identifier(_)
                        | Expression::Self_(_)
                        | Expression::Static(_)
                        | Expression::Parent(_)
                ) && site.receiver_kind == MorphColumnReceiver::StaticQuery
                {
                    crate::type_engine::subject_resolution::resolve_subject_type(
                        &content[offset as usize..span.end.offset as usize],
                        true,
                        offset,
                        &crate::type_engine::subject_resolution::SubjectResolutionCtx {
                            local_classes: &file_ctx.classes,
                            use_map: &file_ctx.use_map,
                            namespace: file_ctx.namespace_at(offset),
                            content,
                            class_loader: &class_loader,
                            backend: Some(self),
                            function_loader: &function_loader_cl,
                        },
                    )
                } else {
                    crate::type_engine::variable::foreach_resolution::resolve_expression_type(
                        expr, &var_ctx,
                    )
                };
                let Some(ty) = ty else {
                    return;
                };
                // Each receiver's literals share its column and call kind.
                // A whereIn array needs only one column check.
                if has_morph_column(&ty, site, &var_ctx.as_resolution_ctx()) {
                    confirmed.extend(candidates.iter().map(|site| site.to_span()));
                }
            });
        });
        confirmed.sort_by_key(|span| span.start);
        confirmed
    }
}

fn has_morph_column(ty: &PhpType, site: &MorphColumnSite, ctx: &ResolutionCtx<'_>) -> bool {
    match ty.kind() {
        TypeKind::Nullable(inner) => return has_morph_column(inner, site, ctx),
        TypeKind::Union(members) => {
            let mut non_null = members.iter().filter(|ty| !ty.is_null());
            return non_null
                .next()
                .is_some_and(|ty| has_morph_column(ty, site, ctx))
                && non_null.all(|ty| has_morph_column(ty, site, ctx));
        }
        TypeKind::Intersection(members) => {
            return members.iter().any(|ty| has_morph_column(ty, site, ctx));
        }
        _ => {}
    }
    let Some(name) = ty.base_name() else {
        return false;
    };
    if crate::class_lookup::is_subtype_of_named(ty, super::ELOQUENT_MODEL_FQN, ctx.class_loader) {
        let Some(class) = (ctx.class_loader)(name) else {
            return false;
        };
        // Qualified columns only refer to this model when their table matches.
        let column = if let Some((table, column)) = site.column.rsplit_once('.') {
            if site.receiver_kind == MorphColumnReceiver::Model
                || !model_table_matches(&class, table, ctx.class_loader)
            {
                return false;
            }
            column
        } else {
            &site.column
        };
        return super::is_morph_type_column(&class, column, ctx.class_loader);
    }
    if site.receiver_kind != MorphColumnReceiver::Model
        && crate::class_lookup::is_subtype_of_named(
            ty,
            super::ELOQUENT_BUILDER_FQN,
            ctx.class_loader,
        )
        && let Some(model) =
            crate::type_engine::variable::rhs_resolution::extract_generic_arg_from_ancestor(
                ty,
                super::ELOQUENT_BUILDER_FQN,
                0,
                ctx,
            )
        && crate::class_lookup::is_subtype_of_named(
            &model,
            super::ELOQUENT_MODEL_FQN,
            ctx.class_loader,
        )
    {
        return has_morph_column(&model, site, ctx);
    }
    false
}

fn model_table_matches(
    class: &ClassInfo,
    table: &str,
    loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    let mut declared_match = None;
    if !collect_table_metadata(class, table, loader, &mut declared_match, 0) {
        return false;
    }
    declared_match.unwrap_or_else(|| super::model_table_name(class).as_deref() == Some(table))
}

// A custom getTable() can compute anything, even when a nearer subclass
// declares $table. Search the whole chain before trusting a literal table.
fn collect_table_metadata(
    class: &ClassInfo,
    requested: &str,
    loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    declared_match: &mut Option<bool>,
    depth: u32,
) -> bool {
    if depth > crate::types::MAX_INHERITANCE_DEPTH {
        return false;
    }
    if class.fqn().as_str() == super::ELOQUENT_MODEL_FQN {
        return true;
    }
    if class
        .get_method_ci("getTable")
        .is_some_and(|method| !method.is_abstract && !method.is_virtual)
    {
        return false;
    }
    if let Some(metadata) = class.laravel() {
        if metadata.has_get_table_method {
            return false;
        }
        if declared_match.is_none() {
            *declared_match = metadata
                .table_name
                .as_deref()
                .map(|table| table == requested);
        }
    }
    for name in &class.used_traits {
        if let Some(trait_info) = loader(name)
            && !collect_table_metadata(&trait_info, requested, loader, declared_match, depth + 1)
        {
            return false;
        }
    }
    class.parent_class.as_ref().is_none_or(|name| {
        name.as_str() == super::ELOQUENT_MODEL_FQN
            || loader(name).is_some_and(|parent| {
                collect_table_metadata(&parent, requested, loader, declared_match, depth + 1)
            })
    })
}

fn visit_expressions(node: Node<'_, '_>, visitor: &mut impl FnMut(&Expression<'_>)) {
    if let Node::Expression(expr) = node {
        visitor(expr);
    }
    node.visit_children(|child| visit_expressions(child, visitor));
}

fn empty_spans() -> Arc<Vec<SymbolSpan>> {
    static EMPTY: std::sync::OnceLock<Arc<Vec<SymbolSpan>>> = std::sync::OnceLock::new();
    Arc::clone(EMPTY.get_or_init(|| Arc::new(Vec::new())))
}

#[cfg(test)]
#[path = "typed_morph_columns_tests.rs"]
mod tests;
