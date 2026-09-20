//! Render sites whose receiver only a *type* settles.
//!
//! The symbol map decides what is a view name syntactically, so the
//! render sites it records as [`crate::symbol_map::SymbolKind::LaravelStringKey`]
//! spans are the ones a receiver's spelling settles: `$this->view(…)`
//! inside a mailable, the `View` facade, and the factory reached through a
//! bare `view()` helper call. A constructor-injected `Factory $views`
//! behind `$this->views->make('page')`, or a mailable held in a local
//! (`$mail = new OrderShipped(); $mail->view('emails.shipped')`), names a
//! template just as surely, but nothing about how it is written says so.
//!
//! [`crate::symbol_map::extract_symbol_map`] cannot tell: it runs during
//! `update_ast`, before the file's own classes are resolved, and a forward
//! walk per method call would be paid on every keystroke. It records the
//! candidate sites instead (see [`ViewReceiverSite`]), and this module
//! answers them lazily — once per file, cached until the file is re-parsed
//! — by asking the shared type engine what the receiver is.
//!
//! The answer is a set of extra view spans, indistinguishable from the ones
//! the symbol map records, that every consumer of view keys reads alongside
//! the map's own.

use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::*;

use crate::Backend;
use crate::parser::with_parsed_program;
use crate::symbol_map::{SymbolMap, SymbolSpan, ViewReceiverSite};
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::types::ClassInfo;

/// One file's confirmed extra view spans, and the source length they were
/// derived from.
///
/// The length is the same staleness signal [`SymbolMap::matches_source`]
/// uses: an insertion or removal since the spans were computed moves every
/// offset in them, so they have to be recomputed rather than reused.
pub(crate) type TypedReceiverSpans = (u32, Arc<Vec<SymbolSpan>>);

impl Backend {
    /// The view-name spans of `uri` whose render site is recognised by the
    /// receiver's type rather than its spelling.
    ///
    /// Empty — without any work at all — for a file whose symbol map
    /// recorded no candidate sites, which is every file that does not call
    /// a view factory or a mailable through a typed receiver.
    ///
    /// Takes the file's symbol map rather than looking it up, because every
    /// caller is already reading the map's own view spans and merging these
    /// into them.
    pub(crate) fn typed_receiver_view_spans_for(
        &self,
        uri: &str,
        map: &SymbolMap,
    ) -> Arc<Vec<SymbolSpan>> {
        if map.view_receiver_sites.is_empty() {
            return empty_spans();
        }
        if let Some((source_len, spans)) = self.typed_receiver_view_spans_cache.read().get(uri)
            && *source_len == map.source_len
        {
            return Arc::clone(spans);
        }

        let Some(content) = self.effective_content(uri) else {
            return empty_spans();
        };
        // The recorded offsets index the text the map was built from; a
        // buffer that has grown or shrunk since is a different text.
        if !map.matches_source(&content) {
            return empty_spans();
        }

        let spans =
            Arc::new(self.confirm_view_receiver_sites(uri, &content, &map.view_receiver_sites));
        self.typed_receiver_view_spans_cache
            .write()
            .insert(uri.to_string(), (map.source_len, Arc::clone(&spans)));
        spans
    }

    /// Drop the confirmed spans of one file, so the next reader recomputes
    /// them against the file as it now reads.
    pub(crate) fn evict_typed_receiver_view_spans(&self, uri: &str) {
        // Files with no candidate sites never get an entry, so the common
        // case is a miss on a map that is almost always empty.
        if self.typed_receiver_view_spans_cache.read().is_empty() {
            return;
        }
        self.typed_receiver_view_spans_cache.write().remove(uri);
    }

    /// The source every offset in a file's symbol map indexes: a Blade
    /// template's virtual PHP, and any other file's own text.
    fn effective_content(&self, uri: &str) -> Option<Arc<String>> {
        self.blade_virtual_php_arc(uri)
            .or_else(|| self.get_file_content_arc(uri))
    }

    /// Resolve the receiver of every candidate site and keep the spans of
    /// the ones that turn out to be renders.
    fn confirm_view_receiver_sites(
        &self,
        uri: &str,
        content: &str,
        sites: &[ViewReceiverSite],
    ) -> Vec<SymbolSpan> {
        let by_offset: HashMap<u32, &ViewReceiverSite> =
            sites.iter().map(|site| (site.start, site)).collect();

        let file_ctx = self.file_context(uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        let function_loader_cl = |name: &str, offset: u32| function_loader(name, offset);

        with_parsed_program(content, "blade_typed_receiver", |program, content| {
            let mut calls: Vec<ReceiverCall<'_, '_, '_>> = Vec::new();
            let walker = ReceiverCallWalker { sites: &by_offset };
            let mut ctx = CollectCtx { calls: &mut calls };
            for stmt in program.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&walker, stmt, &mut ctx);
            }

            let default_class = ClassInfo::default();
            let mut confirmed = Vec::new();
            for call in calls {
                let offset = call.receiver.span().start.offset;
                let current_class =
                    crate::class_lookup::find_class_at_offset(&file_ctx.classes, offset)
                        .unwrap_or(&default_class);
                let var_ctx = VarResolutionCtx {
                    var_name: "",
                    top_level_scope: None,
                    current_class,
                    all_classes: &file_ctx.classes,
                    content,
                    // The receiver is resolved where the call is written, so
                    // a local reassigned earlier reads as it does there.
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
                let Some(ty) =
                    crate::type_engine::variable::foreach_resolution::resolve_expression_type(
                        call.receiver,
                        &var_ctx,
                    )
                else {
                    continue;
                };
                for site in call.sites {
                    if site.receiver.fqns().iter().any(|fqn| {
                        crate::class_lookup::is_subtype_of_named(&ty, fqn, &class_loader)
                    }) {
                        confirmed.push(site.to_span());
                    }
                }
            }
            confirmed.sort_by_key(|span| span.start);
            confirmed
        })
    }
}

/// The shared empty answer, so the overwhelmingly common "this file has no
/// candidate sites" case costs a refcount bump rather than an allocation.
fn empty_spans() -> Arc<Vec<SymbolSpan>> {
    static EMPTY: std::sync::OnceLock<Arc<Vec<SymbolSpan>>> = std::sync::OnceLock::new();
    Arc::clone(EMPTY.get_or_init(|| Arc::new(Vec::new())))
}

/// One method call that names at least one candidate template, paired with
/// the receiver whose type decides whether it renders.
struct ReceiverCall<'ast, 'arena, 'sites> {
    receiver: &'ast Expression<'arena>,
    sites: Vec<&'sites ViewReceiverSite>,
}

struct CollectCtx<'w, 'ast, 'arena, 'sites> {
    calls: &'w mut Vec<ReceiverCall<'ast, 'arena, 'sites>>,
}

/// Finds the method calls the candidate offsets belong to.
///
/// A candidate is keyed by the offset of its view-name string's contents,
/// which is unique in the file, so a call owns exactly the candidates its
/// own argument list spells.
struct ReceiverCallWalker<'a, 'sites> {
    sites: &'a HashMap<u32, &'sites ViewReceiverSite>,
}

impl<'sites> ReceiverCallWalker<'_, 'sites> {
    /// The candidates one argument holds: the string itself, or the entries
    /// of the array a `first(['a', 'b'])` names.
    fn matching_sites(&self, expr: &Expression<'_>, out: &mut Vec<&'sites ViewReceiverSite>) {
        match expr {
            Expression::Literal(Literal::String(s)) => {
                if let Some(site) = self.sites.get(&(s.span.start.offset + 1)) {
                    out.push(site);
                }
            }
            Expression::Array(array) => {
                for element in array.elements.iter() {
                    if let ArrayElement::Value(value) = element {
                        self.matching_sites(value.value, out);
                    }
                }
            }
            Expression::LegacyArray(array) => {
                for element in array.elements.iter() {
                    if let ArrayElement::Value(value) = element {
                        self.matching_sites(value.value, out);
                    }
                }
            }
            _ => {}
        }
    }
}

impl<'ast, 'arena, 'w, 'sites>
    mago_syntax::walker::Walker<'ast, 'arena, CollectCtx<'w, 'ast, 'arena, 'sites>>
    for ReceiverCallWalker<'_, 'sites>
{
    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena, 'sites>,
    ) {
        let mut sites = Vec::new();
        for argument in node.argument_list.arguments.iter() {
            self.matching_sites(argument.value(), &mut sites);
        }
        if !sites.is_empty() {
            ctx.calls.push(ReceiverCall {
                receiver: node.object,
                sites,
            });
        }
    }
}
