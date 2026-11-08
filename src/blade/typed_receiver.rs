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
//!
//! A class of the project's own can stand in front of a receiver: a URL
//! generator wrapped in a class whose `generate(string $name)` passes the
//! name on to the real one.  A call on such a class names a key in the
//! argument its method passes on, which is found by reading the method (see
//! [`Backend::typed_receiver_key_kind`]).

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::*;

use crate::Backend;
use crate::class_lookup::ResolvedClassLoader;
use crate::parser::with_parsed_program;
use crate::php_type::PhpType;
use crate::symbol_map::{ReceiverClass, StringKeyKind, SymbolMap, SymbolSpan, TypedReceiverSite};

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
            note_partial_answer();
            return empty_spans();
        };
        // The recorded offsets index the text the map was built from; a
        // buffer that has grown or shrunk since is a different text.
        if !map.matches_source(&content) {
            note_partial_answer();
            return empty_spans();
        }

        // Confirming a site resolves its receiver, which can read the
        // declarations a configurator file makes (a `get('app.mailer')` is
        // typed by them), and so come back here for the file still being
        // confirmed.  That file's declarations are not known yet: it reads
        // as declaring nothing, and the partial answer is not cached.
        let Some(_confirming) = ConfirmingGuard::enter(uri) else {
            note_partial_answer();
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
        self.template_virtual_php_arc(uri)
            .or_else(|| self.get_file_content_arc(uri))
    }

    /// Resolve the receiver of every candidate site and keep the spans of
    /// the ones that turn out to be renders.
    pub(crate) fn confirm_typed_receiver_sites(
        &self,
        uri: &str,
        content: &str,
        sites: &[TypedReceiverSite],
    ) -> Vec<SymbolSpan> {
        // The spans are cached per file, and a caller working on a file of
        // another Composer project (a `\Drupal::service()` lookup reading a
        // Symfony app's configurator) would otherwise resolve this file's
        // receivers against that project's classes.
        let _project = self.enter_project_scope(uri);
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();
        let _cache_guard =
            crate::virtual_members::with_active_resolved_class_cache(&self.resolved_class_cache);
        let mut by_offset: Vec<&TypedReceiverSite> = sites.iter().collect();
        by_offset.sort_by_key(|site| site.start);

        let file_ctx = self.file_context(uri);
        let loaders = self.file_loaders(&file_ctx);
        let _parse_guard = crate::parser::with_parse_cache(content);
        let in_drupal_site = self.is_in_drupal_web_root(uri);
        let catches_missing_plugin =
            in_drupal_site && crate::drupal::plugins::may_catch_missing_plugin(content);

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
                for (site, index) in call.sites {
                    if let Some(kind) = self.typed_receiver_key_kind(
                        &ty,
                        call.method,
                        index,
                        site.receiver,
                        loaders.resolved_class_loader(),
                    ) {
                        if kind == StringKeyKind::DrupalConfigKey {
                            // The key is one of the object's whose name its
                            // type carries.
                            let editable = site.receiver == ReceiverClass::EditableConfigKeys;
                            for name in self.drupal_config_names_of(
                                &ty,
                                editable,
                                loaders.resolved_class_loader(),
                            ) {
                                let mut span = site.to_span(kind);
                                if let crate::symbol_map::SymbolKind::StringKey { key, .. } =
                                    &mut span.kind
                                {
                                    *key = crate::drupal::config_keys::config_key(&name, &site.key);
                                }
                                confirmed.push(span);
                            }
                            continue;
                        }
                        if kind == StringKeyKind::DrupalPlugin && site.receiver.bare_plugin_key() {
                            // The ID is one of the manager's the receiver
                            // is, or the one the call always looks it up in.
                            let manager = if site.receiver == ReceiverClass::ConstraintTarget {
                                Some(crate::atom::atom(crate::drupal::plugins::CONSTRAINTS))
                            } else {
                                self.drupal_plugin_manager_of(
                                    &ty.top_level_class_names(),
                                    loaders.resolved_class_loader(),
                                )
                            };
                            if let Some(manager) = manager {
                                let mut span = site.to_span(kind);
                                if let crate::symbol_map::SymbolKind::StringKey {
                                    key,
                                    is_optional,
                                    ..
                                } = &mut span.kind
                                {
                                    *key = crate::drupal::plugins::plugin_key(&manager, &site.key);
                                    // A file that may catch a missing
                                    // plugin's exception does without the
                                    // plugin, and so does a manager that
                                    // looks its plugins up its own way.
                                    *is_optional |= catches_missing_plugin
                                        || self
                                            .drupal_plugin_index()
                                            .looks_up_own_way(&manager, call.method);
                                }
                                confirmed.push(span);
                            }
                            continue;
                        }
                        let mut span = site.to_span(kind);
                        // A Drupal lookup on a plain PSR container may ask
                        // any other container, whose services are not the
                        // site's.
                        if kind == StringKeyKind::ServiceId
                            && site.receiver == ReceiverClass::ContainerLookup
                            && in_drupal_site
                            && !crate::drupal::services::SERVICE_CONTAINERS.iter().any(
                                |container| {
                                    crate::class_lookup::is_subtype_of_named(
                                        &ty,
                                        container,
                                        loaders.resolved_class_loader(),
                                    )
                                },
                            )
                            && let crate::symbol_map::SymbolKind::StringKey { is_optional, .. } =
                                &mut span.kind
                        {
                            *is_optional = true;
                        }
                        confirmed.push(span);
                    }
                }
            }
            confirmed.sort_by_key(|span| span.start);
            confirmed
        })
    }

    /// What a call of `method` on a receiver of type `ty` names in its
    /// argument `index`, when its receiver has to be a `receiver`: the kind
    /// of the first target the type is a subtype of, or the kind a method
    /// of the project's own passes the argument on to such a receiver as.
    pub(crate) fn typed_receiver_key_kind(
        &self,
        ty: &PhpType,
        method: &str,
        index: usize,
        receiver: ReceiverClass,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<StringKeyKind> {
        // A Drupal target is not worth a subtype check outside Drupal: a
        // Symfony project's `get()` is never asked about the config factory.
        if let Some((_, kind)) = receiver.targets().iter().find(|(fqn, kind)| {
            (kind.framework() != crate::symbol_map::KeyFramework::Drupal
                || self.is_drupal_project())
                && crate::class_lookup::is_subtype_of_named(ty, fqn, class_loader)
        }) {
            return Some(*kind);
        }
        // A class that uses core's `ConfigFormBaseTrait` has its `config()`,
        // and a trait is no type a target can name.
        if receiver == ReceiverClass::ConfigShortcut && self.is_drupal_project() {
            return ty
                .top_level_class_names()
                .iter()
                .filter_map(|class| class_loader(crate::class_lookup::Fqn::new(class)))
                .any(|class| crate::drupal::config::uses_config_form_trait(&class, class_loader))
                .then_some(StringKeyKind::DrupalConfigName);
        }
        if !receiver.follows_wrappers() {
            return None;
        }
        ty.top_level_class_names()
            .iter()
            .find_map(|class| self.forwarded_key_kind(class, method, index, receiver))
    }

    /// The kind of key the method `method` of `class` passes its parameter
    /// `index` on as: the argument of a call on a `receiver` (or on another
    /// such method) that the parameter is passed to as it is.
    ///
    /// Only a class of the project's own is read, and a method that comes
    /// back to itself through the classes it passes the name on to names
    /// nothing.
    fn forwarded_key_kind(
        &self,
        class: &str,
        method: &str,
        index: usize,
        receiver: ReceiverClass,
    ) -> Option<StringKeyKind> {
        let class = class.trim_start_matches('\\');
        let (declaring, method_info) = self.declaring_method(class, method)?;
        let uri = self.class_declaration_uri(&declaring)?;
        if self
            .workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
        {
            return None;
        }
        let _visiting = ForwardingGuard::enter(format!(
            "{}::{}#{}",
            declaring.to_ascii_lowercase(),
            method.to_ascii_lowercase(),
            index
        ))?;
        let content = self.get_file_content_arc(&uri)?;
        let file_ctx = self.file_context(&uri);
        let loaders = self.file_loaders(&file_ctx);
        let _parse_guard = crate::parser::with_parse_cache(&content);
        with_parsed_program(&content, "forwarded_key_kind", |program, content| {
            let method = find_method(program.statements.iter(), method_info.name_offset)?;
            let parameter = method.parameter_list.parameters.get(index)?;
            let MethodBody::Concrete(body) = &method.body else {
                return None;
            };
            let finder = ForwardFinder {
                parameter: parameter.variable.name,
                receiver,
            };
            let mut found = Vec::new();
            for statement in body.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&finder, statement, &mut found);
            }
            found.into_iter().find_map(|(object, callee, argument)| {
                let var_ctx = loaders.var_ctx_at(content, object.span().start.offset);
                let ty = crate::type_engine::variable::iterable_elements::resolve_expression_type(
                    object, &var_ctx,
                )?;
                self.typed_receiver_key_kind(
                    &ty,
                    callee,
                    argument,
                    receiver,
                    loaders.resolved_class_loader(),
                )
            })
        })
    }

    /// The class that declares the method `method` that `class` has, by
    /// walking up from `class` through its parents, with the method.
    fn declaring_method(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(String, Arc<crate::types::MethodInfo>)> {
        let mut seen = std::collections::HashSet::new();
        let mut current = class.to_string();
        while seen.insert(current.to_ascii_lowercase()) {
            let info = self.find_or_load_class(&current)?;
            if let Some(found) = info
                .methods
                .iter()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(method))
            {
                return Some((current, Arc::clone(found)));
            }
            current = info.parent_class.as_ref()?.to_string();
        }
        None
    }
}

/// The method whose name starts at `name_offset`, among `statements` and
/// the namespaces and classes they declare.
fn find_method<'p, 'a>(
    statements: impl Iterator<Item = &'p Statement<'a>>,
    name_offset: u32,
) -> Option<&'p class_like::method::Method<'a>>
where
    'a: 'p,
{
    for statement in statements {
        let members = match statement {
            Statement::Namespace(namespace) => {
                if let Some(method) = find_method(namespace.statements().iter(), name_offset) {
                    return Some(method);
                }
                continue;
            }
            Statement::Class(class) => &class.members,
            Statement::Trait(class) => &class.members,
            Statement::Enum(class) => &class.members,
            _ => continue,
        };
        for member in members.iter() {
            if let ClassLikeMember::Method(method) = member
                && method.name.span.start.offset == name_offset
            {
                return Some(method);
            }
        }
    }
    None
}

/// Finds the calls a method passes its parameter on to as an argument that
/// names a key on the right receiver: each call's receiver, its method, and
/// the argument's position.
struct ForwardFinder<'p> {
    parameter: &'p [u8],
    receiver: ReceiverClass,
}

type Forwarded<'ast, 'arena> = Vec<(&'ast Expression<'arena>, &'ast str, usize)>;

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Forwarded<'ast, 'arena>>
    for ForwardFinder<'_>
{
    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        found: &mut Forwarded<'ast, 'arena>,
    ) {
        let ClassLikeMemberSelector::Identifier(name) = &node.method else {
            return;
        };
        let callee = crate::atom::bytes_to_str(name.value);
        for (index, argument) in node.argument_list.arguments.iter().enumerate() {
            let Argument::Positional(argument) = argument else {
                break;
            };
            let passes_parameter = matches!(
                argument.value,
                Expression::Variable(Variable::Direct(variable)) if variable.name == self.parameter
            );
            if passes_parameter
                && crate::symbol_map::extraction::symfony_receiver_method(callee, index)
                    .is_some_and(|(receiver, _)| receiver == self.receiver)
            {
                found.push((node.object, callee, index));
            }
        }
    }
}

thread_local! {
    /// The methods whose forwarding is being read on this thread, as
    /// `class::method#index`.
    static FORWARDING: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::default();
}

/// Marks one method as being read for as long as it lives.
struct ForwardingGuard {
    key: String,
}

impl ForwardingGuard {
    /// Mark `key`, unless it is already being read further up.
    fn enter(key: String) -> Option<Self> {
        FORWARDING
            .with(|reading| reading.borrow_mut().insert(key.clone()))
            .then_some(Self { key })
    }
}

impl Drop for ForwardingGuard {
    fn drop(&mut self) {
        FORWARDING.with(|reading| reading.borrow_mut().remove(&self.key));
    }
}

thread_local! {
    /// The files whose sites are being confirmed on this thread.
    static CONFIRMING: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::default();
}

thread_local! {
    /// How many times [`Backend::typed_receiver_spans_for`] has answered
    /// on this thread with spans it could not work out, see
    /// [`partial_answers`].
    static PARTIAL_ANSWERS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn note_partial_answer() {
    PARTIAL_ANSWERS.with(|count| count.set(count.get() + 1));
}

/// A count that moves whenever [`Backend::typed_receiver_spans_for`]
/// answers on this thread with no spans for a file it could not confirm
/// (one already being confirmed further up, or whose text no longer
/// matches its map) rather than the file's actual spans.  Whatever is
/// derived from the answers between two readings that differ is partial
/// and must not be cached.
pub(crate) fn partial_answers() -> u64 {
    PARTIAL_ANSWERS.with(std::cell::Cell::get)
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
/// receiver whose type decides what it names, and the method it calls.
struct ReceiverCall<'ast, 'arena, 'sites> {
    receiver: &'ast Expression<'arena>,
    method: &'ast str,
    /// Each candidate, with the position of the argument it is in.
    sites: Vec<(&'sites TypedReceiverSite, usize)>,
}

struct CollectCtx<'w, 'ast, 'arena, 'sites> {
    calls: &'w mut Vec<ReceiverCall<'ast, 'arena, 'sites>>,
}

/// Finds the method calls the candidate offsets belong to.
///
/// A candidate lies inside a string its call's argument list spells: the
/// whole of its contents, or a `%name%` placeholder among them.  Strings
/// do not overlap, so a call owns exactly the candidates inside its own.
/// A name built at runtime starts where the argument that builds it does.
struct ReceiverCallWalker<'a, 'sites> {
    /// Sorted by offset.
    sites: &'a [&'sites TypedReceiverSite],
}

impl<'sites> ReceiverCallWalker<'_, 'sites> {
    /// The candidates the argument at `index` holds: in the string itself,
    /// or in the entries of the array a `first(['a', 'b'])` or an
    /// `args(['%app.sender%'])` builds, their keys included, which name
    /// the parameters of a Drupal `setRedirect('route', ['node' => 1])`.
    fn matching_sites(
        &self,
        expr: &Expression<'_>,
        index: usize,
        out: &mut Vec<(&'sites TypedReceiverSite, usize)>,
    ) {
        let elements = match expr {
            Expression::Literal(Literal::String(s)) => {
                let (start, end) = (s.span.start.offset, s.span.end.offset);
                let from = self.sites.partition_point(|site| site.start <= start);
                out.extend(
                    self.sites[from..]
                        .iter()
                        .take_while(|site| site.start < end)
                        .map(|site| (*site, index)),
                );
                return;
            }
            Expression::Array(array) => &array.elements,
            Expression::LegacyArray(array) => &array.elements,
            // A name built at runtime is a candidate over the whole
            // expression that builds it.
            other => {
                let start = other.span().start.offset;
                let from = self.sites.partition_point(|site| site.start < start);
                out.extend(
                    self.sites[from..]
                        .iter()
                        .take_while(|site| site.start == start)
                        .map(|site| (*site, index)),
                );
                return;
            }
        };
        for element in elements.iter() {
            match element {
                ArrayElement::Value(value) => self.matching_sites(value.value, index, out),
                ArrayElement::KeyValue(value) => {
                    self.matching_sites(value.key, index, out);
                    self.matching_sites(value.value, index, out);
                }
                _ => {}
            }
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
        let ClassLikeMemberSelector::Identifier(method) = &node.method else {
            return;
        };
        let mut sites = Vec::new();
        for (index, argument) in node.argument_list.arguments.iter().enumerate() {
            self.matching_sites(argument.value(), index, &mut sites);
        }
        if !sites.is_empty() {
            ctx.calls.push(ReceiverCall {
                receiver: node.object,
                method: crate::atom::bytes_to_str(method.value),
                sites,
            });
        }
    }
}
