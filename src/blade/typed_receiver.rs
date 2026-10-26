//! String keys whose receiver only a *type* settles.
//!
//! The symbol map decides what is a string key syntactically, so the call
//! sites it records as [`crate::symbol_map::SymbolKind::StringKey`] spans
//! are the ones a receiver's spelling settles: `$this->view(…)` inside a
//! mailable, the `View` facade, and the factory reached through a bare
//! `view()` helper call. A constructor-injected `Factory $views` behind
//! `$this->views->make('page')`, a mailable held in a local
//! (`$mail = new OrderShipped(); $mail->view('emails.shipped')`), or a
//! Symfony `ContainerInterface $container` behind
//! `$container->get('app.mailer')` names a key just as surely, but nothing
//! about how it is written says so.
//!
//! [`crate::symbol_map::extract_symbol_map`] cannot tell: it runs during
//! `update_ast`, before the file's own classes are resolved, and a forward
//! walk per method call would be paid on every keystroke. It records the
//! candidate sites instead (see [`TypedReceiverSite`]), and this module
//! answers them lazily — once per file, cached until the file is re-parsed
//! — by asking the shared type engine what the receiver is.
//!
//! The answer is a set of extra string-key spans, indistinguishable from
//! the ones the symbol map records, that every consumer of those keys reads
//! alongside the map's own.

use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::*;

use crate::Backend;
use crate::parser::with_parsed_program;
use crate::symbol_map::{SymbolMap, SymbolSpan, TypedReceiverSite};

/// One file's confirmed extra string-key spans, and the source length they
/// were derived from.
///
/// The length is the same staleness signal [`SymbolMap::matches_source`]
/// uses: an insertion or removal since the spans were computed moves every
/// offset in them, so they have to be recomputed rather than reused.
pub(crate) type TypedReceiverSpans = (u32, Arc<Vec<SymbolSpan>>);

impl Backend {
    /// The string-key spans of `uri` whose call is recognised by the
    /// receiver's type rather than its spelling.
    ///
    /// Empty — without any work at all — for a file whose symbol map
    /// recorded no candidate sites, which is every file that does not call
    /// a view factory, a mailable, or a Symfony container through a typed
    /// receiver.
    ///
    /// Takes the file's symbol map rather than looking it up, because every
    /// caller is already reading the map's own spans and merging these into
    /// them.
    pub(crate) fn typed_receiver_spans_for(
        &self,
        uri: &str,
        map: &SymbolMap,
    ) -> Arc<Vec<SymbolSpan>> {
        if map.typed_receiver_sites.is_empty() {
            return empty_spans();
        }
        if let Some((source_len, spans)) = self.blade.typed_receiver_spans_cache.read().get(uri)
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

        // Confirming a site resolves its receiver, which can read the
        // declarations a configurator file makes (a `get('app.mailer')` is
        // typed by them), and so come back here for the file still being
        // confirmed.  That file's declarations are not known yet: it reads
        // as declaring nothing, and the partial answer is not cached.
        let Some(_confirming) = ConfirmingGuard::enter(uri) else {
            return empty_spans();
        };
        let spans =
            Arc::new(self.confirm_typed_receiver_sites(uri, &content, &map.typed_receiver_sites));
        self.blade
            .typed_receiver_spans_cache
            .write()
            .insert(uri.to_string(), (map.source_len, Arc::clone(&spans)));
        spans
    }

    /// Drop the confirmed spans of one file, so the next reader recomputes
    /// them against the file as it now reads.
    pub(crate) fn evict_typed_receiver_spans(&self, uri: &str) {
        // Files with no candidate sites never get an entry, so the common
        // case is a miss on a map that is almost always empty.
        if self.blade.typed_receiver_spans_cache.read().is_empty() {
            return;
        }
        self.blade.typed_receiver_spans_cache.write().remove(uri);
    }

    /// The source every offset in a file's symbol map indexes: a Blade
    /// template's virtual PHP, and any other file's own text.
    fn effective_content(&self, uri: &str) -> Option<Arc<String>> {
        self.blade_virtual_php_arc(uri)
            .or_else(|| self.get_file_content_arc(uri))
    }

    /// Resolve the receiver of every candidate site and keep the spans of
    /// the ones that turn out to be renders.
    fn confirm_typed_receiver_sites(
        &self,
        uri: &str,
        content: &str,
        sites: &[TypedReceiverSite],
    ) -> Vec<SymbolSpan> {
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();
        let _cache_guard =
            crate::virtual_members::with_active_resolved_class_cache(&self.resolved_class_cache);
        let by_offset: HashMap<u32, &TypedReceiverSite> =
            sites.iter().map(|site| (site.start, site)).collect();

        let file_ctx = self.file_context(uri);
        let loaders = self.file_loaders(&file_ctx);
        let _parse_guard = crate::parser::with_parse_cache(content);

        with_parsed_program(content, "blade_typed_receiver", |program, content| {
            let mut calls: Vec<ReceiverCall<'_, '_, '_>> = Vec::new();
            let walker = ReceiverCallWalker { sites: &by_offset };
            let mut ctx = CollectCtx { calls: &mut calls };
            for stmt in program.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&walker, stmt, &mut ctx);
            }

            let mut confirmed = Vec::new();
            for call in calls {
                // The receiver is resolved where the call is written, so a
                // local reassigned earlier reads as it does there.
                let var_ctx = loaders.var_ctx_at(content, call.receiver.span().start.offset);
                let Some(ty) =
                    crate::type_engine::variable::iterable_elements::resolve_expression_type(
                        call.receiver,
                        &var_ctx,
                    )
                else {
                    continue;
                };
                for site in call.sites {
                    if let Some((_, kind)) = site.receiver.targets().iter().find(|(fqn, _)| {
                        crate::class_lookup::is_subtype_of_named(
                            &ty,
                            fqn,
                            loaders.resolved_class_loader(),
                        )
                    }) {
                        confirmed.push(site.to_span(*kind));
                    }
                }
            }
            confirmed.sort_by_key(|span| span.start);
            confirmed
        })
    }
}

thread_local! {
    /// The files whose sites are being confirmed on this thread.
    static CONFIRMING: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::default();
}

/// Marks one file as being confirmed for as long as it lives.
struct ConfirmingGuard {
    uri: String,
}

impl ConfirmingGuard {
    /// Mark `uri`, unless it is already being confirmed further up.
    fn enter(uri: &str) -> Option<Self> {
        CONFIRMING
            .with(|confirming| confirming.borrow_mut().insert(uri.to_string()))
            .then(|| Self {
                uri: uri.to_string(),
            })
    }
}

impl Drop for ConfirmingGuard {
    fn drop(&mut self) {
        CONFIRMING.with(|confirming| confirming.borrow_mut().remove(&self.uri));
    }
}

/// The shared empty answer, so the overwhelmingly common "this file has no
/// candidate sites" case costs a refcount bump rather than an allocation.
fn empty_spans() -> Arc<Vec<SymbolSpan>> {
    static EMPTY: std::sync::OnceLock<Arc<Vec<SymbolSpan>>> = std::sync::OnceLock::new();
    Arc::clone(EMPTY.get_or_init(|| Arc::new(Vec::new())))
}

/// One method call that names at least one candidate key, paired with the
/// receiver whose type decides what it names.
struct ReceiverCall<'ast, 'arena, 'sites> {
    receiver: &'ast Expression<'arena>,
    sites: Vec<&'sites TypedReceiverSite>,
}

struct CollectCtx<'w, 'ast, 'arena, 'sites> {
    calls: &'w mut Vec<ReceiverCall<'ast, 'arena, 'sites>>,
}

/// Finds the method calls the candidate offsets belong to.
///
/// A candidate is keyed by the offset of its string's contents, which is
/// unique in the file, so a call owns exactly the candidates its own
/// argument list spells.
struct ReceiverCallWalker<'a, 'sites> {
    sites: &'a HashMap<u32, &'sites TypedReceiverSite>,
}

impl<'sites> ReceiverCallWalker<'_, 'sites> {
    /// The candidates one argument holds: the string itself, or the entries
    /// of the array a `first(['a', 'b'])` names.
    fn matching_sites(&self, expr: &Expression<'_>, out: &mut Vec<&'sites TypedReceiverSite>) {
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
