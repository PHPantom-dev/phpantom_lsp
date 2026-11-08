//! The names Symfony takes as strings in PHP: service IDs, container
//! parameters, route names, translation keys and domains, Twig templates,
//! the configuration keys a bundle's schema declares, the data-class
//! properties a form type's fields map to, event names and the listener
//! methods that subscribe to them, Messenger bus names, and serializer
//! groups.  The Messenger handlers a class registers are recorded beside
//! them.
//!
//! The configurator functions (`service('app.mailer')`, `param('app.name')`),
//! the `#[Autowire]`, `#[Route]`, `#[Template]`, `#[AsEventListener]`,
//! `#[AsMessageHandler]`, and serializer `#[Groups]` and `#[Context]`
//! attributes, `TranslatableMessage`, `BusNameStamp`,
//! and `t()` are recognised by their resolved name.
//! A method call names one only on the right receiver: `get()` on a
//! container, `set()` on a configurator, `register()` on a container
//! builder, `generate()` on a URL generator,
//! `trans()` on a translator, `render()` on a controller or Twig,
//! `dispatch()` on an event dispatcher.  Those names are far too common to
//! claim on sight, and the receiver's spelling does not settle what it is
//! (`$container` may be anything), so each call is recorded as a
//! [`TypedReceiverSite`] for the type engine to confirm (see
//! [`symfony_receiver_method`]).

use super::*;
use crate::symbol_map::{
    ContainerValue, ContainerValueSite, MessengerHandlerSite, ReceiverClass, ReceiverFrameworks,
    RouteImport, SYMFONY_CONTAINER_CONFIGURATOR_NS, SYMFONY_ROUTING_CONFIGURATOR,
    ServiceDecoration, StringKeyKind,
};
use crate::symfony::translations::{DEFAULT_DOMAIN, is_domain_name, translation_key};
use crate::virtual_members::laravel::helpers::{resolved_string_literal, string_literal_at_range};

const CONFIGURATOR_NS: &str = SYMFONY_CONTAINER_CONFIGURATOR_NS;
const DEFINITION: &str = "Symfony\\Component\\DependencyInjection\\Definition";
const CHILD_DEFINITION: &str = "Symfony\\Component\\DependencyInjection\\ChildDefinition";
const AUTOWIRE_ATTRIBUTE: &str = "Symfony\\Component\\DependencyInjection\\Attribute\\Autowire";
pub(in crate::symbol_map::extraction) const TEMPLATE_ATTRIBUTE: &str =
    "Symfony\\Bridge\\Twig\\Attribute\\Template";
const TRANSLATABLE_MESSAGE: &str = "Symfony\\Component\\Translation\\TranslatableMessage";
const TRANSLATE_FUNCTION: &str = "Symfony\\Component\\Translation\\t";
const AS_EVENT_LISTENER: &str = "Symfony\\Component\\EventDispatcher\\Attribute\\AsEventListener";
const AS_MESSAGE_HANDLER: &str = "Symfony\\Component\\Messenger\\Attribute\\AsMessageHandler";
const BUS_NAME_STAMP: &str = "Symfony\\Component\\Messenger\\Stamp\\BusNameStamp";
/// `AsEventListener`'s constructor parameters, in order.
const EVENT_LISTENER_PARAMETERS: &[&str] = &["event", "method", "priority", "dispatcher"];
/// `AsMessageHandler`'s constructor parameters, in order.
const MESSAGE_HANDLER_PARAMETERS: &[&str] = &[
    "bus",
    "fromTransport",
    "handles",
    "method",
    "priority",
    "sign",
];
const ROUTE_ATTRIBUTES: [&str; 2] = [
    "Symfony\\Component\\Routing\\Attribute\\Route",
    "Symfony\\Component\\Routing\\Annotation\\Route",
];
/// The classes an older application's `@Route` docblock annotation is
/// imported from.
const ROUTE_ANNOTATIONS: [&str; 2] = [
    "Symfony\\Component\\Routing\\Annotation\\Route",
    "Sensio\\Bundle\\FrameworkExtraBundle\\Configuration\\Route",
];

/// What a method call names in its positional argument `index`, when its
/// receiver turns out to be the right class: the receiver it needs, and
/// whether the call declares the name rather than reading it.
///
/// Shared by the symbol map and completion, so the two cannot disagree
/// about which calls name what.  Kept allocation-free: it runs on every
/// method call in every file of a Symfony project.
pub(crate) fn symfony_receiver_method(
    member_name: &str,
    index: usize,
) -> Option<(ReceiverClass, bool)> {
    // Every name below starts with one of these, so most method calls are
    // turned away on a single byte.
    if !matches!(
        member_name.as_bytes().first().map(u8::to_ascii_lowercase),
        Some(b'g' | b'h' | b's' | b'a' | b'd' | b'r' | b't' | b'p' | b'f' | b'c' | b'l')
    ) {
        return None;
    }
    let is = |name: &str| member_name.eq_ignore_ascii_case(name);
    match index {
        0 if is("get") || is("has") => Some((ReceiverClass::ContainerLookup, false)),
        0 if is("register") || is("autowire") || is("setDefinition") || is("setAlias") => {
            Some((ReceiverClass::ContainerBuilder, true))
        }
        1 if is("setAlias") => Some((ReceiverClass::ContainerBuilder, false)),
        // Symfony's container builder and Drupal's entity type manager both
        // have these two, each for its own kind of name.
        0 if is("getDefinition") || is("hasDefinition") => {
            Some((ReceiverClass::DefinitionLookup, false))
        }
        0 if is("findDefinition")
            || is("removeDefinition")
            || is("getAlias")
            || is("hasAlias")
            || is("removeAlias") =>
        {
            Some((ReceiverClass::ContainerBuilder, false))
        }
        0 if is("generateUrl") || is("redirectToRoute") => {
            Some((ReceiverClass::ControllerRouting, false))
        }
        0 if is("generate") || is("generateAbsolute") || is("generateFromRoute") => {
            Some((ReceiverClass::UrlGenerator, false))
        }
        0 if is("setRedirect") => Some((ReceiverClass::FormRedirect, false)),
        0 if is("redirect") => Some((ReceiverClass::RouteRedirect, false)),
        0 if is("checkNamedRoute") => Some((ReceiverClass::AccessCheck, false)),
        0 if is("getRouteByName") => Some((ReceiverClass::RouteLookup, false)),
        3 if is("generateFromCurrent") => Some((ReceiverClass::UrlGenerator, false)),
        0 if is("add") => Some((ReceiverClass::RoutingConfigurator, true)),
        0 if is("getParameter") || is("hasParameter") => {
            Some((ReceiverClass::ParameterHolder, false))
        }
        0 if is("setParameter") => Some((ReceiverClass::ParameterHolder, true)),
        0 if is("set") => Some((ReceiverClass::ConfiguratorSet, true)),
        // `alias('app.mailer', 'app.smtp_mailer')` declares the first name
        // as another one for the second, a service or a route.
        0 if is("alias") => Some((ReceiverClass::ConfiguratorAlias, true)),
        1 if is("alias") => Some((ReceiverClass::ConfiguratorAlias, false)),
        0 if is("decorate") || is("parent") => Some((ReceiverClass::ServiceConfigurator, false)),
        0 if is("trans") => Some((ReceiverClass::Translator, false)),
        2 if is("trans") => Some((ReceiverClass::TranslatorDomain, false)),
        0 if is("render")
            || is("renderView")
            || is("renderBlock")
            || is("renderBlockView")
            || is("stream")
            || is("display") =>
        {
            Some((ReceiverClass::TemplateRenderer, false))
        }
        0 if is("addGlobal") => Some((ReceiverClass::TwigGlobals, true)),
        0 if is("htmlTemplate") || is("textTemplate") => {
            Some((ReceiverClass::TemplatedEmail, false))
        }
        1 if is("dispatch") => Some((ReceiverClass::EventDispatcher, false)),
        0 if is("getStorage")
            || is("getAccessControlHandler")
            || is("getViewBuilder")
            || is("getListBuilder")
            || is("getFormObject")
            || is("getRouteProviders")
            || is("hasHandler")
            || is("getHandler") =>
        {
            Some((ReceiverClass::EntityTypeManager, false))
        }
        0 if is("hasPermission") => Some((ReceiverClass::PermissionCheck, false)),
        0 if is("createInstance") => Some((ReceiverClass::PluginManager, false)),
        0 if is("addConstraint") => Some((ReceiverClass::ConstraintTarget, false)),
        0 if is("getEditable") || is("loadMultiple") => Some((ReceiverClass::ConfigFactory, false)),
        0 if is("config") => Some((ReceiverClass::ConfigShortcut, false)),
        0 if is("grantPermission") || is("revokePermission") => {
            Some((ReceiverClass::RolePermissions, false))
        }
        0 if is("addListener") => Some((ReceiverClass::EventListeners, true)),
        0 if is("removeListener")
            || is("hasListeners")
            || is("getListeners")
            || is("getListenerPriority") =>
        {
            Some((ReceiverClass::EventListeners, false))
        }
        _ => None,
    }
}

/// Record the service IDs and parameter names a method call may name, for
/// the type engine to confirm against its receiver, and what a configurator
/// call declares one as.
pub(in crate::symbol_map::extraction) fn record_symfony_receiver_sites(
    object: &Expression<'_>,
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.service_container && !ctx.twig && !ctx.yii {
        return;
    }
    // Only Drupal's config factory has a method starting with `l` the
    // table names, so elsewhere the one byte still turns those away.
    if !ctx.drupal && matches!(member_name.as_bytes().first(), Some(b'l' | b'L')) {
        return;
    }
    if ctx.symfony && member_name.eq_ignore_ascii_case("trans") {
        record_translator_sites(argument_list, ctx);
        return;
    }
    if ctx.symfony && member_name.eq_ignore_ascii_case("class") {
        record_configured_class(object, argument_list, ctx);
        return;
    }
    if ctx.symfony && member_name.eq_ignore_ascii_case("import") {
        record_route_import(object, argument_list, ctx);
        return;
    }
    if ctx.symfony && member_name.eq_ignore_ascii_case("namePrefix") {
        record_route_name_prefix(object, argument_list, ctx);
        return;
    }
    if member_name.eq_ignore_ascii_case("context") {
        record_twig_email_context(object, argument_list, ctx);
        return;
    }
    if ctx.symfony {
        record_serializer_context_sites(member_name, argument_list, ctx);
        record_configurator_placeholders(member_name, argument_list, ctx);
        if member_name.eq_ignore_ascii_case("parent") {
            record_configured_parent(object, argument_list, ctx);
        } else if member_name.eq_ignore_ascii_case("decorate") {
            record_configured_decoration(object, argument_list, ctx);
        }
    }
    // A Yii `generateFromCurrent()` takes its fallback route fourth.
    for (index, argument) in argument_list.arguments.iter().take(4).enumerate() {
        let Argument::Positional(argument) = argument else {
            break;
        };
        let Some((receiver, is_write)) = symfony_receiver_method(member_name, index) else {
            continue;
        };
        // Outside Symfony, only the calls of the framework the project uses
        // are left: Twig's own `render()`, Yii's URL generator, or a Drupal
        // container lookup.  Only Symfony declares services and parameters
        // in PHP; in Drupal a `setParameter()` is far more often a
        // validator's or a query's than the container's.
        let frameworks = ReceiverFrameworks {
            laravel: false,
            service_container: if is_write {
                ctx.symfony
            } else {
                ctx.service_container
            },
            symfony: ctx.symfony,
            twig: ctx.twig,
            yii: ctx.yii,
            drupal: ctx.drupal,
        };
        if !receiver.names_any(|kind| frameworks.enables(kind)) {
            continue;
        }
        let namespaces = receiver.configurator_namespaces();
        if !namespaces.is_empty()
            && !namespaces
                .iter()
                .any(|namespace| uses_configurators(namespace, ctx))
        {
            continue;
        }
        let Some((key, start, end)) = resolved_string_literal(argument.value, ctx.content) else {
            // A parameter set under a name built at runtime declares every
            // name its literal start begins, which the pattern stands for.
            if receiver == ReceiverClass::ParameterHolder && is_write {
                let span = argument.value.span();
                ctx.typed_receiver_sites.push(TypedReceiverSite {
                    start: span.start.offset,
                    end: span.end.offset,
                    key: format!("{}*", literal_name_prefix(argument.value, ctx.content)),
                    is_write,
                    is_optional: false,
                    guards: 0,
                    receiver,
                });
            }
            continue;
        };
        // Nothing but a Symfony container sets a parameter in the
        // application's own `app.` namespace, so the call declares it
        // whether or not its receiver's type is known.
        if receiver == ReceiverClass::ParameterHolder && is_write && key.starts_with("app.") {
            ctx.spans.push(SymbolSpan {
                start: start as u32,
                end: end as u32,
                kind: SymbolKind::StringKey {
                    kind: StringKeyKind::ContainerParameter,
                    key: key.to_string(),
                    is_write,
                    is_optional: false,
                },
            });
            record_declared_value(receiver, member_name, start as u32, argument_list, ctx);
            continue;
        }
        // `render('…')` is common enough elsewhere that only a string that
        // looks like a template is worth asking the type engine about.
        if key.is_empty()
            || (receiver.names_twig_templates()
                && !crate::twig::templates::looks_like_template_name(key))
            || (receiver == ReceiverClass::TwigGlobals
                && !crate::twig::variables::is_twig_variable_name(key))
        {
            continue;
        }
        // A route added or aliased on a collection takes the collection's
        // name prefix.  The route an alias reads takes it only when the
        // collection declares that route, which is not told here, so under a
        // prefix it is left unread.
        let key = if receiver.declares_routes() {
            match route_name_prefix(object, ctx) {
                Some(prefix) if is_write => format!("{prefix}{key}"),
                Some(prefix) if prefix.is_empty() => key.to_string(),
                Some(_) => continue,
                None => {
                    if is_write {
                        record_unreadable_routes(String::new(), ctx);
                    }
                    continue;
                }
            }
        } else {
            key.to_string()
        };
        // `has()` and `hasDefinition()` ask whether a name exists, and a
        // `get()` told what to do with an invalid reference, or a plugin
        // manager's `getDefinition()` told not to throw, may be told to do
        // without one.
        let is_optional = match receiver {
            ReceiverClass::ContainerLookup => {
                member_name.eq_ignore_ascii_case("has") || argument_list.arguments.len() > 1
            }
            ReceiverClass::DefinitionLookup => {
                member_name.eq_ignore_ascii_case("hasDefinition")
                    || argument_list.arguments.len() > 1
            }
            _ => false,
        };
        ctx.typed_receiver_sites.push(TypedReceiverSite {
            start: start as u32,
            end: end as u32,
            key,
            is_write,
            is_optional,
            guards: 0,
            receiver,
        });
        if !is_write {
            note_own_receiver_lookup(receiver, object, start as u32, ctx);
        }
        if receiver.names_twig_templates() {
            record_twig_render_context(receiver, object, member_name, argument_list, start, ctx);
        }
        if is_write {
            record_declared_value(receiver, member_name, start as u32, argument_list, ctx);
        }
    }
}

/// What a router or a Twig environment the file builds itself (`new
/// UrlGenerator($routes, …)`, `new Environment($loader)`) looks names up
/// in: the routes and templates the file gives it, not the application's.
/// The guard bit of the kind of name, when `class` is one of them.
fn own_names_of(class: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> u8 {
    const ROUTERS: &[&str] = &[
        "Symfony\\Component\\Routing\\Router",
        "Symfony\\Component\\Routing\\Generator\\UrlGenerator",
        "Symfony\\Component\\Routing\\Generator\\CompiledUrlGenerator",
    ];
    const TWIG_ENVIRONMENT: &str = "Twig\\Environment";
    let Expression::Identifier(identifier) = class else {
        return 0;
    };
    let written = bytes_to_str(identifier.value());
    let resolved = ctx
        .resolved_name_at(identifier.span().start.offset)
        .unwrap_or(written)
        .trim_start_matches('\\');
    if ROUTERS
        .iter()
        .any(|router| resolved.eq_ignore_ascii_case(router))
    {
        crate::symbol_map::LookupGuards::ROUTE
    } else if resolved.eq_ignore_ascii_case(TWIG_ENVIRONMENT) {
        crate::symbol_map::LookupGuards::TEMPLATE
    } else {
        0
    }
}

/// Note a variable or property assigned a router or Twig environment the
/// file builds itself (see [`own_names_of`]).
pub(in crate::symbol_map::extraction) fn note_own_receiver_assignment(
    assign: &Assignment<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if (!ctx.symfony && !ctx.twig) || !assign.operator.is_assign() {
        return;
    }
    let Expression::Instantiation(instantiation) = assign.rhs else {
        return;
    };
    let names = own_names_of(instantiation.class, ctx);
    if names == 0 {
        return;
    }
    let receiver = expr_to_subject_text(assign.lhs);
    if !receiver.is_empty() {
        ctx.own_receivers.push((receiver, names));
    }
}

/// Note a lookup whose `receiver` may be a router or Twig environment the
/// file builds itself, for [`apply_lookup_guards`] to tell once the whole
/// file is read.  A controller's own `render()` and `generateUrl()` never
/// are.
fn note_own_receiver_lookup(
    receiver: ReceiverClass,
    object: &Expression<'_>,
    start: u32,
    ctx: &mut ExtractionCtx<'_>,
) {
    let kind = match receiver {
        ReceiverClass::UrlGenerator => crate::symbol_map::LookupGuards::ROUTE,
        ReceiverClass::TemplateRenderer => crate::symbol_map::LookupGuards::TEMPLATE,
        _ => return,
    };
    let mut object = object;
    while let Expression::Parenthesized(inner) = object {
        object = inner.expression;
    }
    match object {
        Expression::Variable(Variable::Direct(variable)) if variable.name == b"$this" => {}
        // `(new UrlGenerator($routes, $context))->generate('x')`.
        Expression::Instantiation(instantiation) => {
            if own_names_of(instantiation.class, ctx) & kind != 0
                && let Some(site) = ctx
                    .typed_receiver_sites
                    .iter_mut()
                    .rev()
                    .find(|site| site.start == start)
            {
                site.guards |= kind;
            }
        }
        _ => {
            let text = expr_to_subject_text(object);
            if !text.is_empty() {
                ctx.own_receiver_lookups.push((start, text, kind));
            }
        }
    }
}

/// Note a `try` whose `catch` copes with a name its lookups miss: one that
/// catches the exception the lookup throws, or one of its parents.
pub(in crate::symbol_map::extraction) fn note_lookup_guards(
    try_stmt: &Try<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony && !ctx.twig {
        return;
    }
    let guards = try_stmt
        .catch_clauses
        .iter()
        .flat_map(|catch| hint_classes(&catch.hint, ctx))
        .fold(0, |guards, caught| {
            guards | crate::symbol_map::LookupGuards::caught_by(&caught)
        });
    if guards != 0 {
        let block = try_stmt.block.span();
        ctx.lookup_guards
            .push((block.start.offset, block.end.offset, guards));
    }
}

/// Mark the lookups the file is written to do without as such: the ones
/// inside a guarding `try` (see [`note_lookup_guards`]), and the ones on a
/// router or Twig environment the file builds itself.
pub(in crate::symbol_map::extraction) fn apply_lookup_guards(ctx: &mut ExtractionCtx<'_>) {
    let own_receivers = std::mem::take(&mut ctx.own_receivers);
    let own_lookups = std::mem::take(&mut ctx.own_receiver_lookups);
    if ctx.lookup_guards.is_empty() && own_receivers.is_empty() {
        return;
    }
    let guards_at = |offset: u32| {
        ctx.lookup_guards
            .iter()
            .filter(|(start, end, _)| *start <= offset && offset < *end)
            .fold(0, |guards, (_, _, guard)| guards | guard)
    };
    for site in ctx.typed_receiver_sites.iter_mut() {
        if site.is_write {
            continue;
        }
        site.guards |= guards_at(site.start);
        site.guards |= own_lookups
            .iter()
            .filter(|(start, receiver, kind)| {
                *start == site.start
                    && own_receivers
                        .iter()
                        .any(|(own, names)| own == receiver && names & kind != 0)
            })
            .fold(0, |guards, (_, _, kind)| guards | kind);
    }
    if ctx.lookup_guards.is_empty() {
        return;
    }
    for span in ctx.spans.iter_mut() {
        if let SymbolKind::StringKey {
            kind,
            is_write: false,
            is_optional,
            ..
        } = &mut span.kind
            && kind.guard_bit() != 0
            && guards_at(span.start) & kind.guard_bit() != 0
        {
            *is_optional = true;
        }
    }
}

/// The kernel class an application's kernel extends.
const KERNEL: &str = "Symfony\\Component\\HttpKernel\\Kernel";
/// The interface every Twig loader implements.
const TWIG_LOADER_INTERFACE: &str = "Twig\\Loader\\LoaderInterface";

/// The literal text a name built at runtime starts with: the leftmost
/// literal of a concatenation (`'app.' . $key`), or nothing.
fn literal_name_prefix<'a>(value: &Expression<'a>, content: &str) -> &'a str {
    match value {
        Expression::Binary(binary) if binary.operator.is_concatenation() => {
            literal_name_prefix(binary.lhs, content)
        }
        Expression::Parenthesized(inner) => literal_name_prefix(inner.expression, content),
        _ => resolved_string_literal(value, content).map_or("", |(text, _, _)| text),
    }
}

/// What a class that is part of the framework's plumbing says about the
/// names the project has, beyond what it declares outright.
///
/// A Twig loader of the project's own can serve any template.  A kernel
/// that overrides `loadRoutes()` loads routes some other way than the
/// routing files read (see [`crate::symfony::route_world`]), and the
/// parameters a kernel's `getKernelParameters()` adds are declared there.
pub(in crate::symbol_map::extraction) fn note_symfony_framework_class(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony && !ctx.twig {
        return;
    }
    let names = |ident: &Identifier<'_>, name: &str| {
        let written = bytes_to_str(ident.value());
        ctx.resolved_name_at(ident.span().start.offset)
            .unwrap_or(written)
            .trim_start_matches('\\')
            .eq_ignore_ascii_case(name)
    };
    let parent = class
        .extends
        .as_ref()
        .and_then(|extends| extends.types.first());
    let in_twig_loader_namespace = |ident: &Identifier<'_>| {
        let written = bytes_to_str(ident.value());
        let resolved = ctx
            .resolved_name_at(ident.span().start.offset)
            .unwrap_or(written)
            .trim_start_matches('\\');
        resolved
            .get(.."Twig\\Loader\\".len())
            .is_some_and(|namespace| namespace.eq_ignore_ascii_case("Twig\\Loader\\"))
    };
    let is_loader = parent.is_some_and(in_twig_loader_namespace)
        || class.implements.as_ref().is_some_and(|implements| {
            implements
                .types
                .iter()
                .any(|ident| names(ident, TWIG_LOADER_INTERFACE))
        });
    let is_kernel = parent.is_some_and(|parent| names(parent, KERNEL));
    if is_loader {
        ctx.declares_twig_loader = true;
    }
    if !ctx.symfony || !is_kernel {
        return;
    }
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let name = bytes_to_str(method.name.value);
        if name.eq_ignore_ascii_case("loadRoutes") {
            record_unreadable_routes(String::new(), ctx);
        } else if name.eq_ignore_ascii_case("getKernelParameters")
            && let MethodBody::Concrete(body) = &method.body
        {
            push_kernel_parameter_spans(body, ctx);
        }
    }
}

/// Declare the parameters a kernel's `getKernelParameters()` adds: the keys
/// of the arrays it builds (not of the arrays inside them), and the
/// offsets it sets.  A key built at runtime is declared as the pattern its
/// literal start makes (see [`literal_name_prefix`]).
fn push_kernel_parameter_spans(body: &Block<'_>, ctx: &mut ExtractionCtx<'_>) {
    let content = ctx.content;
    let mut spans = Vec::new();
    let mut nested: Vec<u32> = Vec::new();
    let mut declare = |key: &Expression<'_>| {
        let (name, start, end) = match resolved_string_literal(key, content) {
            Some((name, start, end)) => (name.to_string(), start as u32, end as u32),
            None => {
                let span = key.span();
                (
                    format!("{}*", literal_name_prefix(key, content)),
                    span.start.offset,
                    span.end.offset,
                )
            }
        };
        spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::ContainerParameter,
                key: name,
                is_write: true,
                is_optional: false,
            },
        });
    };
    crate::virtual_members::laravel::walk_block_expressions(body, &mut |expr| {
        match expr {
            Expression::Array(array) => {
                if nested.contains(&array.span().start.offset) {
                    return std::ops::ControlFlow::Continue(());
                }
                for element in array.elements.iter() {
                    if let ArrayElement::KeyValue(entry) = element {
                        declare(entry.key);
                        if let Expression::Array(inner) = entry.value {
                            nested.push(inner.span().start.offset);
                        }
                    }
                }
            }
            Expression::Assignment(assign) if assign.operator.is_assign() => {
                if let Expression::ArrayAccess(access) = assign.lhs {
                    declare(access.index);
                }
            }
            _ => {}
        }
        std::ops::ControlFlow::Continue(())
    });
    ctx.spans.extend(spans);
}

/// Record what a declaring call says its name is, keyed by the name's site
/// at `start`: the class of a configurator's `set('app.mailer',
/// Mailer::class)` or a builder's `register('app.mailer', Mailer::class)`,
/// the target of an `alias()` or `setAlias()`, the definition a
/// `setDefinition()` is given, and the value of a parameter.
fn record_declared_value(
    receiver: ReceiverClass,
    member_name: &str,
    start: u32,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(Argument::Positional(value)) = argument_list.arguments.get(1) else {
        return;
    };
    let alias = |value: &Expression<'_>| {
        resolved_string_literal(value, ctx.content)
            .filter(|(target, _, _)| !target.is_empty())
            .map(|(target, _, _)| ContainerValue::Alias(crate::atom::atom(target)))
    };
    let value = match receiver {
        ReceiverClass::ConfiguratorSet | ReceiverClass::ParameterHolder => {
            configured_value(value.value, ctx)
        }
        ReceiverClass::ConfiguratorAlias => alias(value.value),
        ReceiverClass::ContainerBuilder if member_name.eq_ignore_ascii_case("setAlias") => {
            alias(value.value)
        }
        ReceiverClass::ContainerBuilder if member_name.eq_ignore_ascii_case("setDefinition") => {
            defined_value(value.value, ctx)
        }
        ReceiverClass::ContainerBuilder => configured_value(value.value, ctx)
            .filter(|value| matches!(value, ContainerValue::Class(_))),
        _ => None,
    };
    if let Some(value) = value {
        ctx.container_values
            .push(ContainerValueSite { start, value });
    }
}

/// What a `new Definition(Mailer::class)` or `new ChildDefinition('app.base')`
/// handed to `setDefinition()` builds the service from.
fn defined_value(value: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Option<ContainerValue> {
    let Expression::Instantiation(instantiation) = value else {
        return None;
    };
    let Expression::Identifier(identifier) = instantiation.class else {
        return None;
    };
    let written = bytes_to_str(identifier.value());
    let resolved = ctx
        .resolved_name_at(identifier.span().start.offset)
        .unwrap_or(written)
        .trim_start_matches('\\');
    let Some(Argument::Positional(first)) = instantiation.argument_list.as_ref()?.arguments.first()
    else {
        return None;
    };
    if resolved.eq_ignore_ascii_case(DEFINITION) {
        configured_value(first.value, ctx).filter(|value| matches!(value, ContainerValue::Class(_)))
    } else if resolved.eq_ignore_ascii_case(CHILD_DEFINITION) {
        resolved_string_literal(first.value, ctx.content)
            .filter(|(parent, _, _)| !parent.is_empty())
            .map(|(parent, _, _)| ContainerValue::Parent(crate::atom::atom(parent)))
    } else {
        None
    }
}

/// The site of the ID a configurator chain's `set()` names, when `object`
/// is that chain: the configurator methods in between (`->args()`,
/// `->tag()`) return the same service's configurator.
fn chained_service_start(object: &Expression<'_>, ctx: &mut ExtractionCtx<'_>) -> Option<u32> {
    let id = chained_service_id(object)?;
    let (_, start, _) = resolved_string_literal(id, ctx.content)?;
    uses_configurators(CONFIGURATOR_NS, ctx).then_some(start as u32)
}

/// The ID argument of the `set()` a configurator chain starts from, when
/// `object` is that chain.
fn chained_service_id<'a, 'arena>(
    object: &'a Expression<'arena>,
) -> Option<&'a Expression<'arena>> {
    let mut link = object;
    let set = loop {
        let Expression::Call(Call::Method(call)) = link else {
            return None;
        };
        if let ClassLikeMemberSelector::Identifier(name) = &call.method
            && name.value.eq_ignore_ascii_case(b"set")
        {
            break call;
        }
        link = call.object;
    };
    let Some(Argument::Positional(id)) = set.argument_list.arguments.first() else {
        return None;
    };
    Some(id.value)
}

/// Record the class a configurator's `->set('app.mailer')->class(Mailer::class)`
/// builds the service from, keyed by the ID the chain's `set()` names.
fn record_configured_class(
    object: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(Argument::Positional(class)) = argument_list.arguments.first() else {
        return;
    };
    let Some(start) = chained_service_start(object, ctx) else {
        return;
    };
    if let Some(value @ ContainerValue::Class(_)) = configured_value(class.value, ctx) {
        ctx.container_values
            .push(ContainerValueSite { start, value });
    }
}

/// Record the service a configurator's `->set('app.cache')->parent('app.base')`
/// inherits its definition from, keyed by the ID the chain's `set()` names.
fn record_configured_parent(
    object: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(Argument::Positional(parent)) = argument_list.arguments.first() else {
        return;
    };
    let Some((parent, _, _)) = resolved_string_literal(parent.value, ctx.content)
        .filter(|(parent, _, _)| !parent.is_empty())
    else {
        return;
    };
    let parent = crate::atom::atom(parent);
    let Some(start) = chained_service_start(object, ctx) else {
        return;
    };
    ctx.container_values.push(ContainerValueSite {
        start,
        value: ContainerValue::Parent(parent),
    });
}

/// Record that the service a configurator's `->set('app.cached')` chain
/// declares decorates the one its `->decorate('app.mailer')` names.  Either
/// ID may be a class, as `Mailer::class`.
fn record_configured_decoration(
    object: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(Argument::Positional(decorated)) = argument_list.arguments.first() else {
        return;
    };
    let Some(decorator) = chained_service_id(object) else {
        return;
    };
    let service_id =
        |value: &Expression<'_>, ctx: &ExtractionCtx<'_>| match configured_value(value, ctx) {
            Some(ContainerValue::Class(class)) if !class.starts_with('%') => Some(class),
            _ => resolved_string_literal(value, ctx.content)
                .filter(|(id, _, _)| !id.is_empty())
                .map(|(id, _, _)| crate::atom::atom(id)),
        };
    let (Some(decorator), Some(decorated)) =
        (service_id(decorator, ctx), service_id(decorated.value, ctx))
    else {
        return;
    };
    if uses_configurators(CONFIGURATOR_NS, ctx) {
        ctx.service_decorations.push(ServiceDecoration {
            decorator,
            decorated,
        });
    }
}

/// Whether a configurator's `member_name` call may hold `%name%`
/// placeholders in its arguments from `from` on: a service's arguments,
/// bindings, method calls, properties, and factory, and the value a
/// `set()` gives a parameter or the class it gives a service.
fn placeholder_arguments_from(member_name: &str) -> Option<usize> {
    let is = |name: &str| member_name.eq_ignore_ascii_case(name);
    if is("set") {
        Some(1)
    } else if is("args")
        || is("arg")
        || is("bind")
        || is("call")
        || is("property")
        || is("factory")
        || is("configurator")
    {
        Some(0)
    } else {
        None
    }
}

/// Record the `%name%` placeholders in the strings a configurator call
/// passes, for the type engine to confirm the receiver is a configurator.
fn record_configurator_placeholders(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(from) = placeholder_arguments_from(member_name) else {
        return;
    };
    let mut found = Vec::new();
    for argument in argument_list.arguments.iter().skip(from) {
        placeholder_strings(argument.value(), &mut found);
    }
    if found.is_empty() || !uses_configurators(CONFIGURATOR_NS, ctx) {
        return;
    }
    for (start, end) in found {
        let Some(text) = ctx.content.get(start as usize..end as usize) else {
            continue;
        };
        crate::symfony::resources::for_each_parameter_placeholder(text, |name, offset| {
            let start = start + offset as u32;
            ctx.typed_receiver_sites.push(TypedReceiverSite {
                start,
                end: start + name.len() as u32,
                key: name.to_string(),
                is_write: false,
                is_optional: false,
                guards: 0,
                receiver: ReceiverClass::ConfiguratorArgument,
            });
        });
    }
}

/// The contents of each string literal holding a `%` in `value`, and in
/// the arrays it builds, as byte ranges inside the quotes.
fn placeholder_strings(value: &Expression<'_>, found: &mut Vec<(u32, u32)>) {
    let elements = match value {
        Expression::Literal(Literal::String(string)) => {
            if string.raw.contains(&b'%') {
                found.push((string.span.start.offset + 1, string.span.end.offset - 1));
            }
            return;
        }
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return,
    };
    for element in elements.iter() {
        match element {
            ArrayElement::Value(element) => placeholder_strings(element.value, found),
            ArrayElement::KeyValue(element) => placeholder_strings(element.value, found),
            _ => {}
        }
    }
}

/// Note a parameter typed as Symfony's routing configurator, whose
/// `import()` loads routes from elsewhere: the one a routing configuration
/// file's closure takes, or a kernel's `configureRoutes()`.  The kernel's
/// container configurator has an `import()` of its own, so the receiver of
/// one is what tells the two apart.
pub(in crate::symbol_map::extraction) fn note_routing_configurator_parameter(
    param: &FunctionLikeParameter<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    let Some(Hint::Identifier(hint)) = &param.hint else {
        return;
    };
    let resolved = ctx
        .resolved_name_at(hint.span().start.offset)
        .unwrap_or_else(|| bytes_to_str(hint.value()));
    if resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(SYMFONY_ROUTING_CONFIGURATOR)
    {
        ctx.routing_configurator_params
            .push(bare_variable_name(param.variable.name));
    }
}

/// Track what a variable assigned a routing configurator's collection
/// prefixes the routes added on it with.
pub(in crate::symbol_map::extraction) fn note_route_configurator_assignment(
    assign: &Assignment<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    // Only a file that takes a routing configurator builds routes on one.
    if ctx.routing_configurator_params.is_empty() || !assign.operator.is_assign() {
        return;
    }
    let Expression::Variable(Variable::Direct(variable)) = assign.lhs else {
        return;
    };
    let name = bytes_to_str(variable.name);
    let name = name.strip_prefix('$').unwrap_or(name);
    let prefix = match assign.rhs {
        Expression::Call(Call::Method(_))
        | Expression::Variable(_)
        | Expression::Parenthesized(_) => Some(route_name_prefix(assign.rhs, ctx)),
        _ => None,
    };
    ctx.route_prefix_vars.retain(|(tracked, _)| tracked != name);
    // A configurator without a prefix is what an untracked variable reads
    // as anyway.
    if let Some(prefix) = prefix.filter(|prefix| prefix.as_deref() != Some("")) {
        ctx.route_prefix_vars.push((name.to_string(), prefix));
    }
}

/// The name prefix the routes added on the routing configurator `receiver`
/// take: the names of the `collection()` calls along its chain, after the
/// prefix of the variable it starts from.  `None` when one of them is not a
/// literal.
fn route_name_prefix(receiver: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Option<String> {
    let mut segments = Vec::new();
    let mut link = receiver;
    let root = loop {
        match link {
            Expression::Call(Call::Method(call)) => {
                if let ClassLikeMemberSelector::Identifier(method) = &call.method
                    && method.value.eq_ignore_ascii_case(b"collection")
                    && let Some(argument) = call.argument_list.arguments.first()
                {
                    let (name, _, _) = resolved_string_literal(argument.value(), ctx.content)?;
                    segments.push(name);
                }
                link = call.object;
            }
            Expression::Parenthesized(inner) => link = inner.expression,
            other => break other,
        }
    };
    let mut prefix = match root {
        Expression::Variable(Variable::Direct(variable)) => {
            let name = bytes_to_str(variable.name);
            let name = name.strip_prefix('$').unwrap_or(name);
            ctx.route_prefix_vars
                .iter()
                .rev()
                .find(|(tracked, _)| tracked == name)
                .map_or(Some(String::new()), |(_, prefix)| prefix.clone())?
        }
        _ => String::new(),
    };
    for segment in segments.iter().rev() {
        prefix.push_str(segment);
    }
    Some(prefix)
}

/// Record that the file declares routes under `name_prefix` whose names
/// cannot be read.
fn record_unreadable_routes(name_prefix: String, ctx: &mut ExtractionCtx<'_>) {
    let import = RouteImport {
        resource: None,
        loader: None,
        name_prefix,
    };
    if !ctx.route_imports.iter().any(|(_, known)| *known == import) {
        // No `import()` call starts at offset 0, so no `namePrefix()`
        // finds this one.
        ctx.route_imports.push((0, import));
    }
}

/// Record the routes a routing configurator's `import()` loads.
fn record_route_import(
    object: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Expression::Variable(Variable::Direct(variable)) = object else {
        return;
    };
    let name = bytes_to_str(variable.name);
    let name = name.strip_prefix('$').unwrap_or(name);
    if !ctx
        .routing_configurator_params
        .iter()
        .any(|param| param == name)
    {
        return;
    }
    let mut resource = None;
    let mut loader = None;
    for (index, argument) in argument_list.arguments.iter().enumerate() {
        let parameter = match argument {
            Argument::Named(named) => bytes_to_str(named.name.value),
            Argument::Positional(_) if index == 0 => "resource",
            Argument::Positional(_) if index == 1 => "type",
            Argument::Positional(_) => continue,
        };
        match parameter {
            "resource" => resource = import_resource(argument.value(), ctx.content),
            "type" => {
                loader = resolved_string_literal(argument.value(), ctx.content)
                    .map(|(loader, _, _)| loader.to_string());
            }
            _ => {}
        }
    }
    ctx.route_imports.push((
        argument_list.span().start.offset,
        RouteImport {
            resource,
            loader,
            name_prefix: String::new(),
        },
    ));
}

/// What an `import()` loads: a literal, or a path built from `__DIR__`,
/// which is the importing file's own directory and so reads as relative to
/// it.
fn import_resource(value: &Expression<'_>, content: &str) -> Option<String> {
    if let Some((resource, _, _)) = resolved_string_literal(value, content) {
        return Some(resource.to_string());
    }
    crate::document_links::try_evaluate_path_expr(value, std::path::Path::new("."))
}

/// Put a `namePrefix()` in front of the names of the routes the `import()`
/// it is chained to loads.
fn record_route_name_prefix(
    object: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if ctx.routing_configurator_params.is_empty() {
        return;
    }
    let prefix = argument_list
        .arguments
        .first()
        .and_then(|argument| resolved_string_literal(argument.value(), ctx.content))
        .map(|(prefix, _, _)| prefix.to_string());
    let mut link = object;
    let import = loop {
        match link {
            Expression::Call(Call::Method(call)) => {
                if let ClassLikeMemberSelector::Identifier(method) = &call.method
                    && method.value.eq_ignore_ascii_case(b"import")
                {
                    break Some(call.argument_list.span().start.offset);
                }
                link = call.object;
            }
            Expression::Parenthesized(inner) => link = inner.expression,
            _ => break None,
        }
    };
    let import = import.and_then(|offset| {
        ctx.route_imports
            .iter_mut()
            .find(|(start, _)| *start == offset)
            .map(|(_, import)| import)
    });
    match (import, prefix) {
        // A later prefix goes in front of an earlier one.
        (Some(import), Some(prefix)) => import.name_prefix.insert_str(0, &prefix),
        (Some(import), None) => {
            import.resource = None;
            import.name_prefix.clear();
        }
        // Imported somewhere this cannot follow, so whichever routes it
        // renames are unknown.
        (None, _) => record_unreadable_routes(String::new(), ctx),
    }
}

/// What a configurator argument gives a service or parameter as: a class
/// (`Mailer::class`, or a string spelled as one), or a literal's type.
fn configured_value(value: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Option<ContainerValue> {
    use crate::php_type::PhpType;
    if let Some(identifier) = crate::parser::class_fetch_identifier(value) {
        let written = bytes_to_str(identifier.value());
        let resolved = ctx
            .resolved_name_at(identifier.span().start.offset)
            .unwrap_or(written);
        return Some(ContainerValue::Class(crate::atom::atom(
            resolved.trim_start_matches('\\'),
        )));
    }
    let ty = match value {
        Expression::Literal(Literal::String(_)) => {
            let (text, _, _) = resolved_string_literal(value, ctx.content)?;
            return Some(crate::symfony::container::string_value(text));
        }
        Expression::Literal(Literal::Integer(_)) => PhpType::int(),
        Expression::Literal(Literal::Float(_)) => PhpType::float(),
        Expression::Literal(Literal::True(_) | Literal::False(_)) => PhpType::bool(),
        Expression::Literal(Literal::Null(_)) => PhpType::null(),
        Expression::Array(_) | Expression::LegacyArray(_) => PhpType::array(),
        _ => return None,
    };
    Some(ContainerValue::Type(ty))
}

/// How a call that translates a message names its domain.
enum DomainArgument<'e, 'a> {
    /// Not at all, or with a literal `null`: the default domain applies.
    Default,
    /// With a string literal.
    Literal(&'e Expression<'a>),
    /// With something else, so which domain the message is in is unknown.
    Unknown,
}

/// The arguments of a call that translates a message: the message, under
/// the parameter name `message_parameter` when it is passed by name, and
/// the domain.
fn translation_arguments<'e, 'a>(
    argument_list: &'e ArgumentList<'a>,
    message_parameter: &str,
) -> (Option<&'e Expression<'a>>, DomainArgument<'e, 'a>) {
    let mut message = None;
    let mut domain = DomainArgument::Default;
    for (index, argument) in argument_list.arguments.iter().enumerate() {
        let (parameter, value) = match argument {
            Argument::Named(named) => (bytes_to_str(named.name.value), named.value),
            Argument::Positional(positional) => match index {
                0 => (message_parameter, positional.value),
                2 => ("domain", positional.value),
                _ => continue,
            },
        };
        if parameter == message_parameter {
            message = Some(value);
        } else if parameter == "domain" {
            domain = match value {
                Expression::Literal(Literal::Null(_)) => DomainArgument::Default,
                Expression::Literal(Literal::String(_)) => DomainArgument::Literal(value),
                _ => DomainArgument::Unknown,
            };
        }
    }
    (message, domain)
}

/// The domain a call translates in, and the range of the literal naming
/// it, or `None` when that cannot be told.
fn translation_domain<'c>(
    domain: DomainArgument<'_, '_>,
    content: &'c str,
) -> Option<(&'c str, Option<(usize, usize)>)> {
    match domain {
        DomainArgument::Default => Some((DEFAULT_DOMAIN, None)),
        DomainArgument::Literal(literal) => {
            let (name, start, end) = string_literal_at_range(literal, content)?;
            is_domain_name(name).then_some((name, Some((start as usize, end as usize))))
        }
        DomainArgument::Unknown => None,
    }
}

/// Record the translation key and domain a `trans()` call names, for the
/// type engine to confirm that its receiver is a translator.  The key is
/// recorded in the domain the call names, so a call whose domain is not a
/// literal records none.
fn record_translator_sites(argument_list: &ArgumentList<'_>, ctx: &mut ExtractionCtx<'_>) {
    let (message, domain) = translation_arguments(argument_list, "id");
    let Some((domain, domain_range)) = translation_domain(domain, ctx.content) else {
        return;
    };
    if let Some((message, start, end)) =
        message.and_then(|message| resolved_string_literal(message, ctx.content))
        && !message.trim().is_empty()
    {
        ctx.typed_receiver_sites.push(TypedReceiverSite {
            start: start as u32,
            end: end as u32,
            key: translation_key(domain, message),
            is_write: false,
            is_optional: false,
            guards: 0,
            receiver: ReceiverClass::Translator,
        });
    }
    if let Some((start, end)) = domain_range {
        ctx.typed_receiver_sites.push(TypedReceiverSite {
            start: start as u32,
            end: end as u32,
            key: domain.to_string(),
            is_write: false,
            is_optional: false,
            guards: 0,
            receiver: ReceiverClass::TranslatorDomain,
        });
    }
}

/// Emit the translation key and domain a message translated later names:
/// `new TranslatableMessage('key', [], 'domain')` and `t('key', [], 'domain')`.
fn push_translatable_spans(argument_list: &ArgumentList<'_>, ctx: &mut ExtractionCtx<'_>) {
    let (message, domain) = translation_arguments(argument_list, "message");
    let Some((domain, domain_range)) = translation_domain(domain, ctx.content) else {
        return;
    };
    if let Some((message, start, end)) =
        message.and_then(|message| resolved_string_literal(message, ctx.content))
        && !message.trim().is_empty()
    {
        ctx.spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::SymfonyTranslation,
                key: translation_key(domain, message),
                is_write: false,
                is_optional: false,
            },
        });
    }
    if let Some((start, end)) = domain_range {
        ctx.spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::TranslationDomain,
                key: domain.to_string(),
                is_write: false,
                is_optional: false,
            },
        });
    }
}

/// Whether `resolved` names Symfony's `TranslatableMessage`.
pub(crate) fn is_translatable_message(resolved: &str) -> bool {
    resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(TRANSLATABLE_MESSAGE)
}

/// Whether `resolved` names Symfony's `t()` function.
pub(crate) fn is_translate_function(resolved: &str) -> bool {
    resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(TRANSLATE_FUNCTION)
}

/// Emit the spans a `new TranslatableMessage(…)` names.
pub(in crate::symbol_map::extraction) fn emit_symfony_instantiation_spans(
    class: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    let Expression::Identifier(identifier) = class else {
        return;
    };
    let written = bytes_to_str(identifier.value());
    let resolved = ctx
        .resolved_name_at(identifier.span().start.offset)
        .unwrap_or(written);
    if is_translatable_message(resolved) {
        push_translatable_spans(argument_list, ctx);
    } else if is_bus_name_stamp(resolved) {
        let bus = argument_list
            .arguments
            .iter()
            .find_map(|argument| match argument {
                Argument::Positional(positional) => Some(positional.value),
                Argument::Named(named) if named.name.value == b"busName" => Some(named.value),
                Argument::Named(_) => None,
            });
        if let Some(bus) = bus {
            push_laravel_string_span(
                StringKeyKind::MessengerBus,
                false,
                false,
                bus,
                ctx.content,
                &mut ctx.spans,
            );
        }
    }
}

/// Whether `resolved` names Messenger's `BusNameStamp`, which sends a
/// message down the bus its constructor names.
pub(crate) fn is_bus_name_stamp(resolved: &str) -> bool {
    resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(BUS_NAME_STAMP)
}

/// Whether `resolved` names Symfony's `#[AsEventListener]`.
pub(crate) fn is_event_listener_attribute(resolved: &str) -> bool {
    resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(AS_EVENT_LISTENER)
}

/// Whether `resolved` names Messenger's `#[AsMessageHandler]`.
pub(crate) fn is_message_handler_attribute(resolved: &str) -> bool {
    resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(AS_MESSAGE_HANDLER)
}

/// Whether the file names anything from one of Symfony's configurator
/// namespaces, which a receiver has to be a class of for `set()`,
/// `alias()`, `decorate()`, or `add()` to declare or name a service or a
/// route.  Those method names are common enough elsewhere
/// (`$session->set('cart', …)`, `$collection->add('x')`) that recording
/// every call for the type engine would make every such file one the
/// declared names are looked for in.  A file without resolved names is not
/// judged.
fn uses_configurators(namespace: &str, ctx: &mut ExtractionCtx<'_>) -> bool {
    let resolved_names = ctx.resolved_names;
    let slot = if namespace == CONFIGURATOR_NS {
        &mut ctx.uses_symfony_configurators
    } else {
        &mut ctx.uses_symfony_routing_configurators
    };
    *slot.get_or_insert_with(|| {
        resolved_names.is_none_or(|names| {
            names.iter().any(|(_, name, _)| {
                name.get(..namespace.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(namespace))
            })
        })
    })
}

/// The kind of name a PHP configurator function takes in its first
/// argument: `service('app.mailer')`, `service_closure('app.mailer')`, and
/// `param('app.sender')`.
pub(crate) fn configurator_function_kind(resolved: &str) -> Option<StringKeyKind> {
    let prefix = resolved.get(..CONFIGURATOR_NS.len())?;
    if !prefix.eq_ignore_ascii_case(CONFIGURATOR_NS) {
        return None;
    }
    let name = &resolved[CONFIGURATOR_NS.len()..];
    if name.eq_ignore_ascii_case("service") || name.eq_ignore_ascii_case("service_closure") {
        Some(StringKeyKind::ServiceId)
    } else if name.eq_ignore_ascii_case("param") {
        Some(StringKeyKind::ContainerParameter)
    } else {
        None
    }
}

/// Emit the span a configurator function call names, when the call
/// resolves to one.
pub(in crate::symbol_map::extraction) fn emit_symfony_function_call_span(
    name_offset: u32,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    let Some(resolved) = ctx.resolved_name_at(name_offset) else {
        return;
    };
    if is_translate_function(resolved) {
        push_translatable_spans(argument_list, ctx);
        return;
    }
    let Some(kind) = configurator_function_kind(resolved) else {
        return;
    };
    if let Some(Argument::Positional(argument)) = argument_list.arguments.iter().next() {
        push_laravel_string_span(
            kind,
            false,
            false,
            argument.value,
            ctx.content,
            &mut ctx.spans,
        );
    }
}

/// Whether `class_name`, as resolved, is Symfony's `#[Autowire]`.
pub(crate) fn is_autowire_attribute(class_name: &str) -> bool {
    class_name
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(AUTOWIRE_ATTRIBUTE)
}

/// Emit the spans an `#[Autowire]` attribute names: `service:` a service
/// ID, `param:` a parameter, and its value an `@id` service reference or
/// the `%name%` placeholders it is built from.
pub(in crate::symbol_map::extraction) fn emit_autowire_attribute_spans(
    argument_list: &PartialArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    for (index, argument) in argument_list.arguments.iter().enumerate() {
        let (parameter, value) = match argument {
            PartialArgument::Named(named) => (bytes_to_str(named.name.value), named.value),
            PartialArgument::Positional(positional) if index == 0 => ("value", positional.value),
            _ => continue,
        };
        match parameter {
            "service" => push_laravel_string_span(
                StringKeyKind::ServiceId,
                false,
                false,
                value,
                ctx.content,
                &mut ctx.spans,
            ),
            "param" => push_laravel_string_span(
                StringKeyKind::ContainerParameter,
                false,
                false,
                value,
                ctx.content,
                &mut ctx.spans,
            ),
            "value" => emit_autowire_value_spans(value, ctx.content, &mut ctx.spans),
            _ => {}
        }
    }
}

/// Emit the serializer groups an attribute's arguments name (see
/// [`crate::symfony::serializer`]).
pub(in crate::symbol_map::extraction) fn emit_serializer_group_spans(
    attribute: crate::symfony::serializer::GroupsAttribute,
    argument_list: &PartialArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let (groups, is_write) = crate::symfony::serializer::attribute_groups(attribute, argument_list);
    for group in groups {
        push_laravel_string_span(
            StringKeyKind::SerializerGroup,
            is_write,
            false,
            group,
            ctx.content,
            &mut ctx.spans,
        );
    }
}

/// Record the serializer groups the context of a serializer call names, for
/// the type engine to confirm against its receiver.
fn record_serializer_context_sites(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some((receiver, groups)) =
        crate::symfony::serializer::call_context_groups(member_name, argument_list)
    else {
        return;
    };
    for group in groups {
        if let Some((key, start, end)) = resolved_string_literal(group, ctx.content) {
            ctx.typed_receiver_sites.push(TypedReceiverSite {
                start: start as u32,
                end: end as u32,
                key: key.to_string(),
                is_write: false,
                is_optional: false,
                guards: 0,
                receiver,
            });
        }
    }
}

/// Whether `class_name`, as resolved, is Symfony's `#[Template]`.
pub(crate) fn is_template_attribute(class_name: &str) -> bool {
    class_name
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(TEMPLATE_ATTRIBUTE)
}

/// Emit the template a `#[Template]` attribute renders the action's result
/// with.
pub(in crate::symbol_map::extraction) fn emit_template_attribute_span(
    argument_list: &PartialArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    try_emit_laravel_string_span_partial_for_parameter(
        StringKeyKind::TwigTemplate,
        argument_list,
        "template",
        ctx.content,
        &mut ctx.spans,
    );
}

/// Whether `class_name`, as resolved, is Symfony's `#[Route]`, under its
/// current name or the `Annotation` one older applications import.
pub(in crate::symbol_map::extraction) fn is_route_attribute(class_name: &str) -> bool {
    let class_name = class_name.trim_start_matches('\\');
    ROUTE_ATTRIBUTES
        .iter()
        .any(|route| class_name.eq_ignore_ascii_case(route))
}

/// A route's name, as its declaration writes it.
pub(in crate::symbol_map::extraction) enum RouteName {
    /// No name: Symfony derives one from the class and the method.
    Absent,
    /// A literal, with the byte range of its text.
    Literal(String, usize, usize),
    /// Something other than a literal, such as a constant.
    Unreadable,
}

/// One `#[Route]` attribute or `@Route` annotation, by what bears on the
/// names it declares.
pub(in crate::symbol_map::extraction) struct RouteDeclaration {
    pub(in crate::symbol_map::extraction) name: RouteName,
    /// The byte range a derived name spans: the path, or the whole
    /// declaration when the path is not a literal.
    at: (usize, usize),
    /// The aliases, each with the byte range of its text.
    aliases: Vec<(String, usize, usize)>,
    /// The locales of a localized path (`path: ['en' => …, 'nl' => …]`),
    /// in order.  Empty for a path that is one string.
    locales: Vec<String>,
}

/// The locales a localized path (`['en' => '/contact', …]`) is keyed by.
fn route_locales(value: &Expression<'_>, content: &str) -> Vec<String> {
    let Expression::Array(array) = value else {
        return Vec::new();
    };
    array
        .elements
        .iter()
        .filter_map(|element| match element {
            ArrayElement::KeyValue(entry) => resolved_string_literal(entry.key, content),
            _ => None,
        })
        .map(|(locale, _, _)| locale.to_string())
        .collect()
}

/// The name a `#[Route]`'s `name:` gives: a literal, or a constant of
/// `class` itself (`self::ROUTE`) whose value is one, with the byte range of
/// the literal's text or of the whole access.
fn route_name_value(
    value: &Expression<'_>,
    class: &Class<'_>,
    ctx: &ExtractionCtx<'_>,
) -> Option<(String, usize, usize)> {
    if let Some((name, start, end)) = resolved_string_literal(value, ctx.content) {
        return Some((name.to_string(), start, end));
    }
    let Expression::Access(Access::ClassConstant(access)) = value else {
        return None;
    };
    let ClassLikeConstantSelector::Identifier(constant) = &access.constant else {
        return None;
    };
    let names_class = match access.class {
        Expression::Self_(_) | Expression::Static(_) => true,
        Expression::Identifier(identifier) => {
            let own = ctx.resolved_name_at(class.name.span.start.offset);
            let named = ctx.resolved_name_at(identifier.span().start.offset);
            own.zip(named).is_some_and(|(own, named)| {
                own.trim_start_matches('\\')
                    .eq_ignore_ascii_case(named.trim_start_matches('\\'))
            })
        }
        _ => false,
    };
    if !names_class {
        return None;
    }
    let item = class.members.iter().find_map(|member| {
        let ClassLikeMember::Constant(declaration) = member else {
            return None;
        };
        declaration
            .items
            .iter()
            .find(|item| item.name.value == constant.value)
    })?;
    let (name, _, _) = resolved_string_literal(item.value, ctx.content)?;
    // The whole access, since the constant's own name is a member access
    // span already, and of two spans over the same text only one is kept.
    let span = access.span();
    Some((
        name.to_string(),
        span.start.offset as usize,
        span.end.offset as usize,
    ))
}

/// The routes the `#[Route]` attributes in `attribute_lists` declare, and
/// the `@Route` annotations in `docblock` (its text and offset), on `class`
/// or one of its methods.
pub(in crate::symbol_map::extraction) fn route_declarations(
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
    docblock: Option<(&str, u32)>,
    class: &Class<'_>,
    ctx: &ExtractionCtx<'_>,
) -> Vec<RouteDeclaration> {
    let mut routes = Vec::new();
    for attribute in attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
    {
        let written = bytes_to_str(attribute.name.value());
        let name_span = attribute.name.span();
        let resolved = ctx
            .resolved_name_at(name_span.start.offset)
            .unwrap_or(written);
        if !is_route_attribute(resolved) {
            continue;
        }
        // The whole attribute: its class name is a span already.
        let attribute_span = attribute.span();
        let mut route = RouteDeclaration {
            name: RouteName::Absent,
            at: (
                attribute_span.start.offset as usize,
                attribute_span.end.offset as usize,
            ),
            aliases: Vec::new(),
            locales: Vec::new(),
        };
        if let Some(arguments) = &attribute.argument_list {
            for (index, argument) in arguments.arguments.iter().enumerate() {
                let (parameter, value) = match argument {
                    PartialArgument::Named(named) => (bytes_to_str(named.name.value), named.value),
                    PartialArgument::Positional(positional) if index == 0 => {
                        ("path", positional.value)
                    }
                    PartialArgument::Positional(positional) if index == 1 => {
                        ("name", positional.value)
                    }
                    _ => continue,
                };
                match parameter {
                    "path" => {
                        if let Some((_, start, end)) = resolved_string_literal(value, ctx.content) {
                            route.at = (start, end);
                        }
                        route.locales = route_locales(value, ctx.content);
                    }
                    "name" => {
                        route.name = match route_name_value(value, class, ctx) {
                            Some((name, start, end)) => RouteName::Literal(name, start, end),
                            None if matches!(value, Expression::Literal(Literal::Null(_))) => {
                                RouteName::Absent
                            }
                            None => RouteName::Unreadable,
                        };
                    }
                    "alias" => route.aliases = route_aliases(value, ctx.content),
                    _ => {}
                }
            }
        }
        routes.push(route);
    }
    if let Some((text, offset)) = docblock {
        routes.extend(annotation_routes(text, offset as usize));
    }
    routes
}

/// The aliases a `#[Route]`'s `alias:` declares: one name, or a list.
fn route_aliases(value: &Expression<'_>, content: &str) -> Vec<(String, usize, usize)> {
    let values: Vec<&Expression<'_>> = match value {
        Expression::Array(array) => array
            .elements
            .iter()
            .filter_map(|element| match element {
                ArrayElement::Value(value) => Some(value.value),
                _ => None,
            })
            .collect(),
        alias => vec![alias],
    };
    values
        .into_iter()
        .filter_map(|alias| resolved_string_literal(alias, content))
        .map(|(alias, start, end)| (alias.to_string(), start, end))
        .collect()
}

/// Whether the file imports the class of the `@Route` annotation, without
/// which a docblock's `@Route` is something else.
fn uses_route_annotations(ctx: &mut ExtractionCtx<'_>) -> bool {
    let content = ctx.content;
    *ctx.uses_route_annotations.get_or_insert_with(|| {
        ROUTE_ANNOTATIONS
            .iter()
            .any(|annotation| content.contains(annotation))
    })
}

/// The docblock of the declaration at `span`, and where it starts.
fn route_docblock<'a>(span: mago_span::Span, ctx: &ExtractionCtx<'a>) -> Option<(&'a str, u32)> {
    get_docblock_text_with_offset(ctx.trivias, ctx.content, &span)
}

/// What one argument of an annotation is: a string, by the byte range of
/// its text, a map (`{"en": "/contact", …}`), by its keys, or anything else.
enum AnnotationValue {
    Str(usize, usize),
    Map(Vec<String>),
    Other,
}

/// Where the next argument of an annotation starts, from `pos`: past the
/// whitespace and the `*` that continues a docblock line.
fn skip_annotation_blank(doc: &[u8], mut pos: usize) -> usize {
    while doc
        .get(pos)
        .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'*')
    {
        pos += 1;
    }
    pos
}

/// The annotation value starting at `pos`, and where it ends.
fn annotation_value(doc: &[u8], pos: usize) -> (AnnotationValue, usize) {
    let mut pos = skip_annotation_blank(doc, pos);
    if doc.get(pos) == Some(&b'"') {
        return match memchr::memchr(b'"', &doc[pos + 1..]) {
            Some(len) => (AnnotationValue::Str(pos + 1, pos + 1 + len), pos + len + 2),
            None => (AnnotationValue::Other, doc.len()),
        };
    }
    if doc.get(pos) == Some(&b'{') {
        return annotation_map(doc, pos);
    }
    let mut depth = 0usize;
    while let Some(&byte) = doc.get(pos) {
        match byte {
            b'"' => {
                pos = memchr::memchr(b'"', &doc[pos + 1..]).map_or(doc.len(), |len| pos + len + 1);
            }
            b'{' | b'(' => depth += 1,
            b'}' | b')' if depth > 0 => depth -= 1,
            b')' | b',' => break,
            _ => {}
        }
        pos += 1;
    }
    (AnnotationValue::Other, pos)
}

/// The annotation map or list whose `{` is at `pos`, by the keys of its
/// entries (`"en": …`, `"en" = …`, or `en = …`), and where it ends.  A list
/// has no keys.
fn annotation_map(doc: &[u8], mut pos: usize) -> (AnnotationValue, usize) {
    let mut keys = Vec::new();
    let mut depth = 0usize;
    // Whether the entry of the map itself that starts next can be a key.
    let mut at_entry = false;
    while let Some(&byte) = doc.get(pos) {
        let key = match byte {
            b'"' => {
                let Some(len) = memchr::memchr(b'"', &doc[pos + 1..]) else {
                    break;
                };
                let key = (pos + 1, pos + 1 + len);
                pos += len + 2;
                Some(key)
            }
            b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                let start = pos;
                pos += doc[pos..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
                    .count();
                Some((start, pos))
            }
            _ => None,
        };
        if let Some((start, end)) = key {
            let after = skip_annotation_blank(doc, pos);
            if depth == 1 && at_entry && matches!(doc.get(after), Some(b':' | b'=')) {
                // Cut at a quote or an ASCII name, so always UTF-8.
                if let Ok(key) = std::str::from_utf8(&doc[start..end]) {
                    keys.push(key.to_string());
                }
            }
            at_entry = false;
            continue;
        }
        match byte {
            b'{' | b'(' => {
                depth += 1;
                at_entry = depth == 1;
            }
            b'}' | b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return (AnnotationValue::Map(keys), pos + 1);
                }
            }
            b',' if depth == 1 => at_entry = true,
            byte if byte.is_ascii_whitespace() || byte == b'*' => {}
            _ => at_entry = false,
        }
        pos += 1;
    }
    (AnnotationValue::Map(keys), doc.len())
}

/// The `@Route(…)` annotations of a docblock that starts at `offset`.
fn annotation_routes(doc: &str, offset: usize) -> Vec<RouteDeclaration> {
    const TAG: &str = "@Route";
    let bytes = doc.as_bytes();
    let mut routes = Vec::new();
    let mut cursor = 0;
    while let Some(found) = doc[cursor..].find(TAG) {
        let tag = cursor + found;
        cursor = tag + TAG.len();
        let mut pos = skip_annotation_blank(bytes, cursor);
        if bytes.get(pos) != Some(&b'(') {
            continue;
        }
        pos += 1;
        let mut route = RouteDeclaration {
            name: RouteName::Absent,
            at: (offset + tag, offset + cursor),
            aliases: Vec::new(),
            locales: Vec::new(),
        };
        let mut index = 0;
        loop {
            pos = skip_annotation_blank(bytes, pos);
            match bytes.get(pos) {
                None => break,
                Some(b')') => {
                    pos += 1;
                    break;
                }
                Some(b',') => {
                    index += 1;
                    pos += 1;
                    continue;
                }
                Some(_) => {}
            }
            // `name="…"`, `name: "…"`, or a positional value.
            let ident_end = pos
                + bytes[pos..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
                    .count();
            let after = skip_annotation_blank(bytes, ident_end);
            let parameter = if ident_end > pos && matches!(bytes.get(after), Some(b'=' | b':')) {
                let parameter = &doc[pos..ident_end];
                pos = after + 1;
                parameter
            } else if index == 0 {
                "path"
            } else {
                ""
            };
            let (value, next) = annotation_value(bytes, pos);
            pos = next;
            match (parameter, value) {
                ("path" | "value", AnnotationValue::Str(start, end)) => {
                    route.at = (offset + start, offset + end);
                }
                ("path" | "value", AnnotationValue::Map(locales)) => route.locales = locales,
                ("name", AnnotationValue::Str(start, end)) => {
                    route.name = RouteName::Literal(
                        doc[start..end].to_string(),
                        offset + start,
                        offset + end,
                    );
                }
                ("name", AnnotationValue::Other) => route.name = RouteName::Unreadable,
                _ => {}
            }
        }
        cursor = cursor.max(pos);
        routes.push(route);
    }
    routes
}

/// Emit the route names a class's `#[Route]` attributes and `@Route`
/// annotations declare.
///
/// A route on a method is named by its `name:`, after the name of the
/// class's own first route, which prefixes every route in it.  A route with
/// no name is given the one Symfony derives from the class and the method,
/// spanning its path.  A class with no routes on its methods and an
/// `__invoke()` is a single action, and its own routes are its routes.  An
/// alias is declared as it is written, without the prefix.  A name that is
/// not a literal is recorded as one that cannot be read.
///
/// Symfony names a route more ways than its declaration writes.  A
/// localized path (`['en' => '/contact', …]`, on the route or the class's
/// prefix) adds a route per locale, `contact.en`; the base name still
/// generates the current locale's.  An action with exactly one route also
/// answers to `App\Controller\Foo::bar`, and a class whose only route is
/// its `__invoke()` to `App\Controller\Foo` and `App\Controller\Foo::__invoke`.
pub(in crate::symbol_map::extraction) fn emit_symfony_route_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    let annotations = uses_route_annotations(ctx);
    let class_doc = if annotations {
        route_docblock(class.span(), ctx)
    } else {
        None
    };
    let class_routes = route_declarations(&class.attribute_lists, class_doc, class, ctx);
    // Derived names are spelled from the class an action is loaded with,
    // which an abstract one never is.
    let derive = (!class.modifiers.contains_abstract())
        .then(|| ctx.resolved_name_at(class.name.span.start.offset))
        .flatten()
        .map(|class| class.trim_start_matches('\\'));
    let mut spans = Vec::new();

    let prefix = match class_routes.first().map(|route| &route.name) {
        Some(RouteName::Literal(name, _, _)) => name.as_str(),
        // Every route in the class takes a prefix that cannot be read.
        Some(RouteName::Unreadable) => {
            record_unreadable_routes(String::new(), ctx);
            return;
        }
        _ => "",
    };
    let class_locales: &[String] = class_routes
        .first()
        .map_or(&[], |route| route.locales.as_slice());
    let mut any_method_route = false;
    let mut unreadable = false;
    let mut invoke = None;
    // The routes the class adds, and where the last one is declared.
    let mut count = 0;
    let mut last_at = (0, 0);
    let mut routes_invoke = false;
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let method_name = bytes_to_str(method.name.value);
        let is_invoke = method_name.eq_ignore_ascii_case("__invoke");
        if is_invoke {
            invoke = Some(method_name);
        }
        let method_doc = if annotations {
            route_docblock(method.span(), ctx)
        } else {
            None
        };
        let routes = route_declarations(&method.attribute_lists, method_doc, class, ctx);
        if routes.is_empty() {
            continue;
        }
        any_method_route = true;
        routes_invoke |= is_invoke;
        let action = push_action_route_spans(
            &routes,
            prefix,
            class_locales,
            derive,
            method_name,
            &mut spans,
        );
        unreadable |= action.unreadable;
        if action.count == 1
            && let Some(class) = derive
        {
            declare_route(
                &mut spans,
                StringKeyKind::SymfonyRoute,
                format!("{class}::{method_name}"),
                action.at,
            );
        }
        count += action.count;
        last_at = action.at;
    }
    if unreadable {
        record_unreadable_routes(prefix.to_string(), ctx);
    }
    if !any_method_route
        && let Some(invoke) = invoke
        && !class_routes.is_empty()
    {
        let action = push_action_route_spans(&class_routes, "", &[], derive, invoke, &mut spans);
        if action.unreadable {
            record_unreadable_routes(String::new(), ctx);
        }
        routes_invoke = true;
        count = action.count;
        last_at = action.at;
    }
    if routes_invoke
        && count == 1
        && let Some(class) = derive
    {
        declare_route(
            &mut spans,
            StringKeyKind::SymfonyRoute,
            class.to_string(),
            last_at,
        );
        declare_route(
            &mut spans,
            StringKeyKind::SymfonyRoute,
            format!("{class}::__invoke"),
            last_at,
        );
    }
    ctx.spans.extend(spans);
}

/// Declare the route `key`, a `kind` key, over the byte range `at`.
fn declare_route(
    spans: &mut Vec<SymbolSpan>,
    kind: StringKeyKind,
    key: String,
    at: (usize, usize),
) {
    spans.push(SymbolSpan {
        start: at.0 as u32,
        end: at.1 as u32,
        kind: SymbolKind::StringKey {
            kind,
            key,
            is_write: true,
            is_optional: false,
        },
    });
}

/// Emit the fields a form type maps to its data class, as accesses to the
/// data class's properties (see [`crate::symfony::forms`]).
pub(in crate::symbol_map::extraction) fn emit_symfony_form_field_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    let resolve = |written: &str, offset: u32| {
        ctx.resolved_name_at(offset)
            .map_or_else(|| written.to_string(), str::to_string)
    };
    let Some(form) = crate::symfony::forms::form_fields(class, &resolve) else {
        return;
    };
    let (written, offset) = form.data_class;
    // Rooted, so the class is not read again against this file's
    // namespace and imports.
    let data_class = format!("\\{}", resolve(written, offset).trim_start_matches('\\'));
    for (start, end) in form.fields {
        let field = &ctx.content[start as usize..end as usize];
        if !is_identifier(field) {
            continue;
        }
        ctx.spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::MemberAccess {
                subject_text: SubjectText::owned(data_class.clone()),
                member_name: crate::atom::atom(field),
                is_static: false,
                is_method_call: false,
                docblock_ref: DocblockMemberRef::PropertyPath,
                is_array_callable: false,
                is_nullsafe: false,
            },
        });
    }
}

/// Emit the configuration keys a bundle's `getConfigTreeBuilder()`
/// declares (see [`crate::symfony::config_tree`]).
pub(in crate::symbol_map::extraction) fn emit_symfony_config_tree_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    let resolve = |written: &str, offset: u32| {
        ctx.resolved_name_at(offset)
            .map_or_else(|| written.to_string(), str::to_string)
    };
    let keys = crate::symfony::config_tree::tree_builder_keys(class, &resolve);
    ctx.spans
        .extend(keys.into_iter().map(|(key, start, end)| SymbolSpan {
            start,
            end,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::SymfonyConfigKey,
                key,
                is_write: true,
                is_optional: false,
            },
        }));
}

/// The attributes among `attribute_lists` that resolve to `class_name`.
pub(in crate::symbol_map::extraction) fn attributes_named<'e, 'a>(
    attribute_lists: &'e Sequence<'a, AttributeList<'a>>,
    class_name: &'e str,
    ctx: &'e ExtractionCtx<'_>,
) -> impl Iterator<Item = &'e Attribute<'a>> {
    attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
        .filter(move |attribute| {
            let written = bytes_to_str(attribute.name.value());
            ctx.resolved_name_at(attribute.name.span().start.offset)
                .unwrap_or(written)
                .trim_start_matches('\\')
                .eq_ignore_ascii_case(class_name)
        })
}

/// The arguments of `attribute`, each with the constructor parameter it is
/// passed to: a named one by its name, a positional one by its place in
/// `parameters`.
pub(in crate::symbol_map::extraction) fn attribute_arguments<'e, 'a>(
    attribute: &'e Attribute<'a>,
    parameters: &[&'static str],
) -> Vec<(&'e str, &'e Expression<'a>)> {
    let Some(arguments) = &attribute.argument_list else {
        return Vec::new();
    };
    arguments
        .arguments
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| match argument {
            PartialArgument::Named(named) => Some((bytes_to_str(named.name.value), named.value)),
            PartialArgument::Positional(positional) => parameters
                .get(index)
                .map(|parameter| (*parameter, positional.value)),
            _ => None,
        })
        .collect()
}

/// Push a span for the method of the class `subject` a string names, as
/// the listener method an event subscription or a handler's `method:`
/// gives.  Symfony calls it on an instance of the class.
fn push_own_method_span(
    subject: &str,
    method: &Expression<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some((name, start, end)) = resolved_string_literal(method, content) else {
        return;
    };
    if !is_identifier(name) {
        return;
    }
    spans.push(SymbolSpan {
        start: start as u32,
        end: end as u32,
        kind: SymbolKind::MemberAccess {
            subject_text: SubjectText::owned(subject.to_string()),
            member_name: crate::atom::atom(name),
            is_static: false,
            is_method_call: true,
            docblock_ref: DocblockMemberRef::No,
            is_array_callable: false,
            is_nullsafe: false,
        },
    });
}

/// Emit the event names a class's listeners subscribe to and the methods
/// they name: an `#[AsEventListener]` on the class or one of its methods,
/// and the event-to-method map a subscriber's `getSubscribedEvents()`
/// returns.  Each subscription declares the event name.
pub(in crate::symbol_map::extraction) fn emit_symfony_listener_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    // Rooted, so the class is not read again against the file's namespace
    // and imports.
    let subject = ctx
        .resolved_name_at(class.name.span.start.offset)
        .map(|fqn| format!("\\{}", fqn.trim_start_matches('\\')));
    let mut spans = Vec::new();
    for attribute in attributes_named(&class.attribute_lists, AS_EVENT_LISTENER, ctx) {
        for (parameter, value) in attribute_arguments(attribute, EVENT_LISTENER_PARAMETERS) {
            match parameter {
                "event" => push_event_span(value, ctx.content, &mut spans),
                "method" => {
                    if let Some(subject) = &subject {
                        push_own_method_span(subject, value, ctx.content, &mut spans);
                    }
                }
                _ => {}
            }
        }
    }
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        // On a method, the method is the listener.
        for attribute in attributes_named(&method.attribute_lists, AS_EVENT_LISTENER, ctx) {
            for (parameter, value) in attribute_arguments(attribute, EVENT_LISTENER_PARAMETERS) {
                if parameter == "event" {
                    push_event_span(value, ctx.content, &mut spans);
                }
            }
        }
        if method.modifiers.contains_static()
            && method
                .name
                .value
                .eq_ignore_ascii_case(b"getSubscribedEvents")
            && let MethodBody::Concrete(body) = &method.body
        {
            for statement in body.statements.iter() {
                if let Statement::Return(statement) = statement
                    && let Some(events) = statement.value
                {
                    push_subscribed_event_spans(
                        events,
                        subject.as_deref(),
                        ctx.content,
                        &mut spans,
                    );
                }
            }
        }
    }
    ctx.spans.extend(spans);
}

/// Push the event name a listener subscribes to, when it is a string.
fn push_event_span(event: &Expression<'_>, content: &str, spans: &mut Vec<SymbolSpan>) {
    push_laravel_string_span(
        StringKeyKind::SymfonyEvent,
        true,
        false,
        event,
        content,
        spans,
    );
}

/// Push the spans of the map a `getSubscribedEvents()` returns: each key
/// an event name, each value the method that listens to it, alone, with a
/// priority (`['onEvent', 10]`), or as a list of either.
fn push_subscribed_event_spans(
    events: &Expression<'_>,
    subject: Option<&str>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(entries) = crate::parser::array_literal_elements(events) else {
        return;
    };
    for entry in entries.iter() {
        let ArrayElement::KeyValue(entry) = entry else {
            continue;
        };
        push_event_span(entry.key, content, spans);
        let Some(subject) = subject else {
            continue;
        };
        let Some(listeners) = crate::parser::array_literal_elements(entry.value) else {
            push_own_method_span(subject, entry.value, content, spans);
            continue;
        };
        let mut values = listeners.iter().filter_map(|element| match element {
            ArrayElement::Value(value) => Some(value.value),
            _ => None,
        });
        match values.next() {
            // `['onEvent', 10]`: the method, then its priority.
            Some(first @ Expression::Literal(Literal::String(_))) => {
                push_own_method_span(subject, first, content, spans);
            }
            // `[['onEvent', 10], ['onOther']]`: one listener per entry.
            Some(first) => {
                for listener in std::iter::once(first).chain(values) {
                    if let Some(method) = crate::parser::array_literal_elements(listener)
                        .and_then(|listener| listener.iter().next())
                        .and_then(|element| match element {
                            ArrayElement::Value(value) => Some(value.value),
                            _ => None,
                        })
                    {
                        push_own_method_span(subject, method, content, spans);
                    }
                }
            }
            None => {}
        }
    }
}

/// Record the Messenger handlers a class's `#[AsMessageHandler]`
/// attributes register, and emit the bus each one names and the method a
/// class-level one names.
///
/// A class-level attribute registers the method its `method:` names,
/// `__invoke()` when it names none; one on a method registers that method.
/// The message is the attribute's `handles:`, else each class the method's
/// first parameter is typed with.
pub(in crate::symbol_map::extraction) fn emit_messenger_handler_sites(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    let Some(handler) = ctx
        .resolved_name_at(class.name.span.start.offset)
        .map(|fqn| fqn.trim_start_matches('\\'))
    else {
        return;
    };
    let subject = format!("\\{handler}");
    let method_named = |name: &[u8]| {
        class.members.iter().find_map(|member| match member {
            ClassLikeMember::Method(method) if method.name.value.eq_ignore_ascii_case(name) => {
                Some(method)
            }
            _ => None,
        })
    };

    let mut spans = Vec::new();
    let mut sites = Vec::new();
    let mut register = |attribute: &Attribute<'_>,
                        method: Option<&Method<'_>>,
                        method_name: &str,
                        spans: &mut Vec<SymbolSpan>| {
        let mut handles = None;
        for (parameter, value) in attribute_arguments(attribute, MESSAGE_HANDLER_PARAMETERS) {
            match parameter {
                "bus" => push_laravel_string_span(
                    StringKeyKind::MessengerBus,
                    false,
                    false,
                    value,
                    ctx.content,
                    spans,
                ),
                "handles" => handles = Some(value),
                _ => {}
            }
        }
        let messages = match handles {
            Some(handles) => message_class(handles, ctx).into_iter().collect(),
            None => method
                .and_then(|method| method.parameter_list.parameters.iter().next())
                .and_then(|parameter| parameter.hint.as_ref())
                .map(|hint| hint_classes(hint, ctx))
                .unwrap_or_default(),
        };
        for message in messages {
            sites.push(MessengerHandlerSite {
                handler: crate::atom::atom(handler),
                method: crate::atom::atom(method_name),
                message: crate::atom::atom(&message),
            });
        }
    };

    for attribute in attributes_named(&class.attribute_lists, AS_MESSAGE_HANDLER, ctx) {
        let named = attribute_arguments(attribute, MESSAGE_HANDLER_PARAMETERS)
            .into_iter()
            .find(|(parameter, _)| *parameter == "method")
            .map(|(_, value)| value);
        if let Some(named) = named {
            push_own_method_span(&subject, named, ctx.content, &mut spans);
        }
        let method_name = named
            .and_then(|named| resolved_string_literal(named, ctx.content))
            .map_or("__invoke", |(name, _, _)| name);
        register(
            attribute,
            method_named(method_name.as_bytes()),
            method_name,
            &mut spans,
        );
    }
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        for attribute in attributes_named(&method.attribute_lists, AS_MESSAGE_HANDLER, ctx) {
            register(
                attribute,
                Some(method),
                bytes_to_str(method.name.value),
                &mut spans,
            );
        }
    }
    ctx.spans.extend(spans);
    ctx.messenger_handlers.extend(sites);
}

/// The message class a `handles:` names: `Foo::class`, or a class name
/// written out as a string.
pub(super) fn message_class(handles: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Option<String> {
    if let Some(identifier) = crate::parser::class_fetch_identifier(handles) {
        let written = bytes_to_str(identifier.value());
        let resolved = ctx
            .resolved_name_at(identifier.span().start.offset)
            .unwrap_or(written);
        return Some(resolved.trim_start_matches('\\').to_string());
    }
    let (name, _, _) = resolved_string_literal(handles, ctx.content)?;
    let name = name.trim_start_matches('\\');
    (!name.is_empty()).then(|| name.to_string())
}

/// The classes a parameter type names: the class itself, each class of a
/// union, and the class behind a `?`.
fn hint_classes(hint: &Hint<'_>, ctx: &ExtractionCtx<'_>) -> Vec<String> {
    match hint {
        Hint::Identifier(identifier) => {
            let written = bytes_to_str(identifier.value());
            let resolved = ctx
                .resolved_name_at(identifier.span().start.offset)
                .unwrap_or(written);
            vec![resolved.trim_start_matches('\\').to_string()]
        }
        Hint::Nullable(nullable) => hint_classes(nullable.hint, ctx),
        Hint::Parenthesized(inner) => hint_classes(inner.hint, ctx),
        Hint::Union(union) => {
            let mut classes = hint_classes(union.left, ctx);
            classes.extend(hint_classes(union.right, ctx));
            classes
        }
        _ => Vec::new(),
    }
}

/// What the routes of one action add.
struct ActionRoutes {
    /// Whether one of them has a name that cannot be read.
    unreadable: bool,
    /// How many routes they add: one per locale of a localized path.
    count: usize,
    /// Where the last of them is declared.
    at: (usize, usize),
}

/// The spans the routes of one action declare.  `derive_from` is the class
/// to derive a name from for a route that has none; Symfony numbers the
/// unnamed routes of one action from the second one on.  A route whose own
/// path is not localized takes the locales of the class's prefix,
/// `class_locales`.
fn push_action_route_spans(
    routes: &[RouteDeclaration],
    prefix: &str,
    class_locales: &[String],
    derive_from: Option<&str>,
    method: &str,
    spans: &mut Vec<SymbolSpan>,
) -> ActionRoutes {
    let mut unnamed = 0;
    let mut action = ActionRoutes {
        unreadable: false,
        count: 0,
        at: (0, 0),
    };
    for route in routes {
        let derived = match (&route.name, derive_from) {
            (RouteName::Absent, Some(class)) => {
                unnamed += 1;
                Some(default_route_name(class, method, unnamed - 1))
            }
            _ => None,
        };
        let locales = if route.locales.is_empty() {
            class_locales
        } else {
            &route.locales
        };
        action.unreadable |= push_route_spans(
            route,
            prefix,
            derived,
            StringKeyKind::SymfonyRoute,
            locales,
            spans,
        );
        action.count += locales.len().max(1);
        action.at = route.at;
    }
    action
}

/// The spans one route declares as `kind` keys: its name, or the `derived`
/// one, and its aliases, each under every one of its `locales`.  A localized route's own
/// name still generates the current locale's route; an alias's does not.
/// Whether its name cannot be read.
pub(in crate::symbol_map::extraction) fn push_route_spans(
    route: &RouteDeclaration,
    prefix: &str,
    derived: Option<String>,
    kind: StringKeyKind,
    locales: &[String],
    spans: &mut Vec<SymbolSpan>,
) -> bool {
    let localized = |spans: &mut Vec<SymbolSpan>, name: &str, at: (usize, usize)| {
        for locale in locales {
            declare_route(spans, kind, format!("{name}.{locale}"), at);
        }
    };
    let declare = |spans: &mut Vec<SymbolSpan>, name: String, at: (usize, usize)| {
        localized(spans, &name, at);
        declare_route(spans, kind, name, at);
    };
    let unreadable = match &route.name {
        RouteName::Literal(name, start, end) => {
            declare(spans, format!("{prefix}{name}"), (*start, *end));
            false
        }
        RouteName::Absent => {
            if let Some(derived) = derived {
                declare(spans, format!("{prefix}{derived}"), route.at);
            }
            false
        }
        RouteName::Unreadable => true,
    };
    for (alias, start, end) in &route.aliases {
        if locales.is_empty() {
            declare_route(spans, kind, alias.clone(), (*start, *end));
        } else {
            localized(spans, alias, (*start, *end));
        }
    }
    unreadable
}

/// The name Symfony's framework gives a route declared without one: the
/// class and the method, lowercased and joined with `_`, with `Controller`
/// and `Bundle` dropped from the namespace segments and an `Action` suffix
/// dropped from the method.  The `index`th unnamed route of one method
/// takes `_index` after it.
fn default_route_name(class: &str, method: &str, index: usize) -> String {
    let mut name = format!("{}_{method}", class.trim_start_matches('\\')).replace('\\', "_");
    name.make_ascii_lowercase();
    if index > 0 {
        name.push_str(&format!("_{index}"));
    }
    let mut name = name.replace("bundle_", "_").replace("controller_", "_");
    if method.ends_with("Action") || method.ends_with("_action") {
        let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let suffix_start = if digits > 0 && name[..name.len() - digits].ends_with('_') {
            name.len() - digits - 1
        } else {
            name.len()
        };
        if name[..suffix_start].ends_with("action") {
            name.replace_range(suffix_start - "action".len()..suffix_start, "");
        }
    }
    name.replace("__", "_")
}

/// The names an `#[Autowire]` value is written in terms of: `'@app.mailer'`
/// injects a service (`'@@'` escapes a literal `@`), and any other string
/// is resolved for `%name%` parameter placeholders.
fn emit_autowire_value_spans(value: &Expression<'_>, content: &str, spans: &mut Vec<SymbolSpan>) {
    // The raw source: a name has no escapes in it, so its offsets are the
    // same in the source as in the value.
    let Some((raw, start, _)) = string_literal_at_range(value, content) else {
        return;
    };
    let push = |spans: &mut Vec<SymbolSpan>, kind, name: &str, offset: usize| {
        spans.push(SymbolSpan {
            start: start + offset as u32,
            end: start + (offset + name.len()) as u32,
            kind: SymbolKind::StringKey {
                kind,
                key: name.to_string(),
                is_write: false,
                is_optional: false,
            },
        });
    };
    if let Some(id) = raw.strip_prefix('@') {
        if !id.is_empty() && !id.starts_with(['@', '=']) && !id.contains('\\') {
            push(spans, StringKeyKind::ServiceId, id, 1);
        }
        return;
    }
    let mut cursor = 0usize;
    while let Some(open) = raw[cursor..].find('%').map(|n| cursor + n) {
        let Some(close) = raw[open + 1..].find('%').map(|n| open + 1 + n) else {
            break;
        };
        let name = &raw[open + 1..close];
        if !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        {
            push(spans, StringKeyKind::ContainerParameter, name, open + 1);
            cursor = close + 1;
        } else {
            cursor = if name.is_empty() { close + 1 } else { close };
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::names::OwnedResolvedNames;
    use crate::symbol_map::{ReceiverClass, RouteImport, SymbolMap};

    fn map_of(php: &str) -> SymbolMap {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        crate::symbol_map::extraction::extract_symbol_map_for_index(
            program,
            php,
            &names,
            crate::symbol_map::extraction::Frameworks {
                symfony: true,
                service_container: true,
                drupal: false,
                twig: true,
                doctrine: false,
                yii_params: false,
            },
            None,
        )
    }

    fn receivers(map: &SymbolMap) -> Vec<(&str, ReceiverClass, bool)> {
        map.typed_receiver_sites
            .iter()
            .map(|site| (site.key.as_str(), site.receiver, site.is_write))
            .collect()
    }

    #[test]
    fn configurator_calls_are_recorded_only_where_configurators_are_named() {
        let session = "<?php\nfunction f($session, $container) {\n    $session->set('app.cart', 1);\n    $container->get('app.mailer');\n}\n";
        assert_eq!(
            receivers(&map_of(session)),
            vec![("app.mailer", ReceiverClass::ContainerLookup, false)]
        );

        let configurator = "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nreturn static function (ContainerConfigurator $c): void {\n    $c->services()->set('app.mailer')->decorate('app.base');\n    $c->services()->alias('app.alias', 'app.mailer');\n};\n";
        assert_eq!(
            receivers(&map_of(configurator)),
            vec![
                ("app.mailer", ReceiverClass::ConfiguratorSet, true),
                ("app.base", ReceiverClass::ServiceConfigurator, false),
                ("app.alias", ReceiverClass::ConfiguratorAlias, true),
                ("app.mailer", ReceiverClass::ConfiguratorAlias, false),
            ]
        );
    }

    /// Each `(method, argument)` Drupal code reads a name from through
    /// `symfony_receiver_method`, with the Drupal class and kind the
    /// receiver it gets must offer, so that a same-named arm placed before
    /// Drupal's takes nothing away from it.
    const DRUPAL_RECEIVER_METHODS: &[(&str, usize, &str, crate::symbol_map::StringKeyKind)] = &[
        ("getStorage", 0, ENTITY_TYPE_MANAGER, ENTITY_TYPE),
        ("getDefinition", 0, ENTITY_TYPE_MANAGER, ENTITY_TYPE),
        ("hasDefinition", 0, ENTITY_TYPE_MANAGER, ENTITY_TYPE),
        (
            "getAccessControlHandler",
            0,
            ENTITY_TYPE_MANAGER,
            ENTITY_TYPE,
        ),
        ("getViewBuilder", 0, ENTITY_TYPE_MANAGER, ENTITY_TYPE),
        ("getListBuilder", 0, ENTITY_TYPE_MANAGER, ENTITY_TYPE),
        ("getFormObject", 0, ENTITY_TYPE_MANAGER, ENTITY_TYPE),
        ("getRouteProviders", 0, ENTITY_TYPE_MANAGER, ENTITY_TYPE),
        ("hasHandler", 0, ENTITY_TYPE_MANAGER, ENTITY_TYPE),
        ("getHandler", 0, ENTITY_TYPE_MANAGER, ENTITY_TYPE),
        ("generate", 0, URL_GENERATOR, ROUTE),
        ("generateFromRoute", 0, URL_GENERATOR, ROUTE),
        ("setRedirect", 0, FORM_STATE, ROUTE),
        ("redirect", 0, CONTROLLER_BASE, ROUTE),
        ("redirect", 0, FORM_BASE, ROUTE),
        ("redirect", 0, ENTITY_CONTROLLER, ROUTE),
        ("checkNamedRoute", 0, ACCESS_MANAGER, ROUTE),
        ("getRouteByName", 0, ROUTE_PROVIDER, ROUTE),
    ];

    const ENTITY_TYPE_MANAGER: &str = "Drupal\\Core\\Entity\\EntityTypeManagerInterface";
    const ENTITY_TYPE: crate::symbol_map::StringKeyKind =
        crate::symbol_map::StringKeyKind::DrupalEntityType;
    const URL_GENERATOR: &str = "Drupal\\Core\\Routing\\UrlGeneratorInterface";
    const FORM_STATE: &str = "Drupal\\Core\\Form\\FormStateInterface";
    const CONTROLLER_BASE: &str = "Drupal\\Core\\Controller\\ControllerBase";
    const FORM_BASE: &str = "Drupal\\Core\\Form\\FormBase";
    const ENTITY_CONTROLLER: &str = "Drupal\\Core\\Entity\\Controller\\EntityController";
    const ACCESS_MANAGER: &str = "Drupal\\Core\\Access\\AccessManagerInterface";
    const ROUTE_PROVIDER: &str = "Drupal\\Core\\Routing\\RouteProviderInterface";
    const ROUTE: crate::symbol_map::StringKeyKind = crate::symbol_map::StringKeyKind::DrupalRoute;

    #[test]
    fn every_drupal_receiver_method_reaches_its_drupal_class() {
        for &(method, index, class, kind) in DRUPAL_RECEIVER_METHODS {
            let (receiver, _) =
                crate::symbol_map::extraction::symfony_receiver_method(method, index)
                    .unwrap_or_else(|| panic!("{method}#{index} names no receiver"));
            assert!(
                receiver.targets().contains(&(class, kind)),
                "{method}#{index}: {receiver:?} has no {class} target of {kind:?}"
            );
        }
    }

    #[test]
    fn a_container_builders_definition_lookups_name_a_service() {
        let php = "<?php\nfunction f($container) {\n    $container->getDefinition('app.a');\n    $container->hasDefinition('app.b');\n}\n";
        let map = map_of(php);
        assert_eq!(
            receivers(&map),
            vec![
                ("app.a", ReceiverClass::DefinitionLookup, false),
                ("app.b", ReceiverClass::DefinitionLookup, false),
            ]
        );
        // A receiver confirmed as a container builder names a service ID.
        let (_, kind) = ReceiverClass::DefinitionLookup
            .targets()
            .iter()
            .find(|(class, _)| {
                *class == "Symfony\\Component\\DependencyInjection\\ContainerBuilder"
            })
            .expect("the container builder is a target");
        let span = map.typed_receiver_sites[0].to_span(*kind);
        assert!(matches!(
            &span.kind,
            crate::symbol_map::SymbolKind::StringKey {
                kind: crate::symbol_map::StringKeyKind::ServiceId,
                key,
                is_write: false,
                ..
            } if key == "app.a"
        ));
    }

    #[test]
    fn builder_calls_parents_and_placeholders_are_recorded() {
        let builder = "<?php\nfunction f($container) {\n    $container->register('app.a', A::class);\n    $container->setAlias('app.b', 'app.a');\n    $container->findDefinition('app.a');\n}\n";
        assert_eq!(
            receivers(&map_of(builder)),
            vec![
                ("app.a", ReceiverClass::ContainerBuilder, true),
                ("app.b", ReceiverClass::ContainerBuilder, true),
                ("app.a", ReceiverClass::ContainerBuilder, false),
                ("app.a", ReceiverClass::ContainerBuilder, false),
            ]
        );

        let configurator = "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nreturn static function (ContainerConfigurator $c): void {\n    $c->services()->set('app.c')->parent('app.base')->args(['%app.x%/%app.y%', 'k' => ['50%'], '%%app.z%%']);\n    $c->parameters()->set('app.p', '%app.q%');\n};\n";
        assert_eq!(
            receivers(&map_of(configurator)),
            vec![
                ("app.c", ReceiverClass::ConfiguratorSet, true),
                ("app.base", ReceiverClass::ServiceConfigurator, false),
                ("app.x", ReceiverClass::ConfiguratorArgument, false),
                ("app.y", ReceiverClass::ConfiguratorArgument, false),
                ("app.p", ReceiverClass::ConfiguratorSet, true),
                ("app.q", ReceiverClass::ConfiguratorArgument, false),
            ]
        );
    }

    #[test]
    fn configurator_calls_record_what_they_declare() {
        use crate::symbol_map::ContainerValue;
        let php = "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nuse App\\Mailer;\nreturn static function (ContainerConfigurator $c): void {\n    $c->services()->set('app.mailer', Mailer::class);\n    $c->services()->set('app.smtp')->args([])->class('App\\\\Smtp');\n    $c->services()->alias('app.alias', 'app.mailer');\n    $c->parameters()->set('app.port', 25);\n    $c->parameters()->set('app.name', 'PHPantom');\n};\n";
        let map = map_of(php);
        let values: Vec<(&str, String)> = map
            .container_values
            .iter()
            .map(|site| {
                let key = map
                    .typed_receiver_sites
                    .iter()
                    .find(|receiver| receiver.start == site.start)
                    .map_or("", |receiver| receiver.key.as_str());
                let value = match &site.value {
                    ContainerValue::Class(class) => format!("class {class}"),
                    ContainerValue::Alias(id) => format!("alias {id}"),
                    ContainerValue::Parent(id) => format!("parent {id}"),
                    ContainerValue::Type(ty) => ty.to_string(),
                };
                (key, value)
            })
            .collect();
        assert_eq!(
            values,
            vec![
                ("app.mailer", "class App\\Mailer".to_string()),
                ("app.smtp", "class App\\Smtp".to_string()),
                ("app.alias", "alias app.mailer".to_string()),
                ("app.port", "int".to_string()),
                ("app.name", "string".to_string()),
            ]
        );
    }

    #[test]
    fn a_translation_is_recorded_in_the_domain_its_call_names() {
        let php = "<?php\nfunction f($translator, string $d) {\n    $translator->trans('a');\n    $translator->trans('b', [], 'admin');\n    $translator->trans(domain: null, id: 'c');\n    $translator->trans('d', [], $d);\n}\n";
        let sites: Vec<(String, ReceiverClass)> = map_of(php)
            .typed_receiver_sites
            .iter()
            .map(|site| (site.key.replace('\u{1f}', "/"), site.receiver))
            .collect();
        assert_eq!(
            sites,
            vec![
                ("messages/a".to_string(), ReceiverClass::Translator),
                ("admin/b".to_string(), ReceiverClass::Translator),
                ("admin".to_string(), ReceiverClass::TranslatorDomain),
                ("messages/c".to_string(), ReceiverClass::Translator),
            ]
        );
    }

    #[test]
    fn renders_are_recorded_only_for_template_names() {
        let php = "<?php\nuse Symfony\\Bridge\\Twig\\Attribute\\Template;\nclass C {\n    #[Template(template: 'attr.html.twig')]\n    function f($twig, $email) {\n        $twig->render('page.html.twig');\n        $twig->render('not-a-template');\n        $email->textTemplate('mail.txt.twig');\n    }\n}\n";
        let map = map_of(php);
        assert_eq!(
            receivers(&map),
            vec![
                ("page.html.twig", ReceiverClass::TemplateRenderer, false),
                ("mail.txt.twig", ReceiverClass::TemplatedEmail, false),
            ]
        );
        let attribute: Vec<&str> = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                crate::symbol_map::SymbolKind::StringKey {
                    kind: crate::symbol_map::StringKeyKind::TwigTemplate,
                    key,
                    ..
                } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(attribute, vec!["attr.html.twig"]);
    }

    /// The route names `map` declares and the text each spans, leaving out
    /// the ones Symfony names after the controller (see
    /// [`all_declared_routes`]).
    fn declared_routes(map: &SymbolMap, php: &str) -> Vec<(String, String)> {
        all_declared_routes(map, php)
            .into_iter()
            .filter(|(name, _)| !name.contains('\\'))
            .collect()
    }

    fn all_declared_routes(map: &SymbolMap, php: &str) -> Vec<(String, String)> {
        map.spans
            .iter()
            .filter_map(|span| match &span.kind {
                crate::symbol_map::SymbolKind::StringKey {
                    kind: crate::symbol_map::StringKeyKind::SymfonyRoute,
                    key,
                    is_write: true,
                    ..
                } => Some((
                    key.clone(),
                    php[span.start as usize..span.end as usize].to_string(),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn route_attributes_declare_prefixed_derived_and_aliased_names() {
        let php = "<?php\nnamespace App\\Controller;\nuse Symfony\\Component\\Routing\\Attribute\\Route;\n#[Route('/blog', name: 'blog_')]\nclass BlogController {\n    #[Route('/', name: 'index', alias: ['blog_home', 'home'])]\n    public function index() {}\n    #[Route('/a')]\n    #[Route('/b')]\n    public function showAction() {}\n}\n";
        assert_eq!(
            declared_routes(&map_of(php), php),
            vec![
                ("blog_index".to_string(), "index".to_string()),
                ("blog_home".to_string(), "blog_home".to_string()),
                ("home".to_string(), "home".to_string()),
                ("blog_app_blog_show".to_string(), "/a".to_string()),
                ("blog_app_blog_show_1".to_string(), "/b".to_string()),
            ]
        );
    }

    #[test]
    fn an_invokable_controller_is_routed_by_its_class_attribute() {
        let php = "<?php\nnamespace App\\Controller;\nuse Symfony\\Component\\Routing\\Annotation\\Route;\n#[Route('/about', name: 'about')]\nclass AboutController {\n    public function __invoke() {}\n}\n#[Route('/admin', name: 'admin_')]\nclass AdminController {\n    public function __invoke() {}\n    #[Route('/x', name: 'x')]\n    public function x() {}\n}\n";
        assert_eq!(
            declared_routes(&map_of(php), php),
            vec![
                ("about".to_string(), "about".to_string()),
                ("admin_x".to_string(), "x".to_string()),
            ]
        );
    }

    #[test]
    fn route_annotations_declare_prefixed_and_derived_names() {
        let php = "<?php\nnamespace App\\Controller;\nuse Symfony\\Component\\Routing\\Annotation\\Route;\n/**\n * @Route(\"/blog\", name=\"blog_\")\n */\nclass BlogController {\n    /**\n     * @Route(\n     *     \"/{slug}\",\n     *     requirements={\"slug\": \"[a-z,)]+\"},\n     *     name=\"post\"\n     * )\n     */\n    public function post() {}\n    /** @Route(path=\"/list\") @Router(name=\"not_a_route\") */\n    public function listAction() {}\n}\n";
        assert_eq!(
            declared_routes(&map_of(php), php),
            vec![
                ("blog_post".to_string(), "post".to_string()),
                ("blog_app_blog_list".to_string(), "/list".to_string()),
            ]
        );
        // Without the annotation's class, `@Route` is something else.
        let php = php.replace("Annotation\\Route", "Other\\Route");
        assert!(declared_routes(&map_of(&php), &php).is_empty());
    }

    #[test]
    fn a_route_name_that_is_not_a_literal_leaves_its_prefix_open() {
        let php = "<?php\nnamespace App\\Controller;\nuse Symfony\\Component\\Routing\\Attribute\\Route;\n#[Route('/admin', name: 'admin_')]\nclass AdminController {\n    public const B = 'b';\n    #[Route('/a', name: Other::NAME)]\n    public function a() {}\n    #[Route('/b', name: self::B)]\n    public function b() {}\n    #[Route('/c', name: AdminController::C)]\n    public function c() {}\n    const C = 'c';\n}\n";
        let map = map_of(php);
        // A constant of the class itself is read; another class's is not.
        assert_eq!(
            declared_routes(&map, php),
            vec![
                ("admin_b".to_string(), "self::B".to_string()),
                ("admin_c".to_string(), "AdminController::C".to_string()),
            ]
        );
        assert_eq!(
            map.route_imports,
            vec![RouteImport {
                resource: None,
                loader: None,
                name_prefix: "admin_".to_string(),
            }]
        );
    }

    #[test]
    fn an_action_with_one_route_is_named_after_its_controller_too() {
        let php = "<?php\nnamespace App\\Controller;\nuse Symfony\\Component\\Routing\\Attribute\\Route;\nclass BlogController {\n    #[Route('/', name: 'index')]\n    public function index() {}\n    #[Route('/a')]\n    #[Route('/b')]\n    public function show() {}\n    #[Route(path: ['en' => '/about', 'nl' => '/over'], name: 'about')]\n    public function about() {}\n}\n#[Route('/about', name: 'about_us')]\nclass AboutController {\n    public function __invoke() {}\n}\nabstract class BaseController {\n    #[Route('/x', name: 'x')]\n    public function x() {}\n}\n";
        // An action with two routes, or one localized into two, has no
        // controller name, and an abstract class is never loaded.
        assert_eq!(
            all_declared_routes(&map_of(php), php),
            vec![
                (
                    "App\\Controller\\BlogController::index".to_string(),
                    "/".to_string()
                ),
                ("index".to_string(), "index".to_string()),
                ("app_blog_show".to_string(), "/a".to_string()),
                ("app_blog_show_1".to_string(), "/b".to_string()),
                ("about.en".to_string(), "about".to_string()),
                ("about.nl".to_string(), "about".to_string()),
                ("about".to_string(), "about".to_string()),
                (
                    "App\\Controller\\AboutController".to_string(),
                    "/about".to_string()
                ),
                (
                    "App\\Controller\\AboutController::__invoke".to_string(),
                    "/about".to_string()
                ),
                ("about_us".to_string(), "about_us".to_string()),
                ("x".to_string(), "x".to_string()),
            ]
        );
    }

    #[test]
    fn a_localized_route_is_declared_per_locale() {
        let php = "<?php\nnamespace App\\Controller;\nuse Symfony\\Component\\Routing\\Attribute\\Route;\n#[Route(['en' => '/en', 'de' => '/de'], name: 'shop_')]\nclass ShopController {\n    #[Route('/cart', name: 'cart', alias: 'basket')]\n    public function cart() {}\n}\n#[Route(path: ['en' => '/contact', 'nl' => '/contact-nl'], name: 'contact')]\nclass ContactController {\n    public function __invoke() {}\n}\n";
        // The class's localized prefix localizes every route in it, and an
        // alias is only declared per locale.
        assert_eq!(
            declared_routes(&map_of(php), php),
            vec![
                ("shop_cart.en".to_string(), "cart".to_string()),
                ("shop_cart.de".to_string(), "cart".to_string()),
                ("shop_cart".to_string(), "cart".to_string()),
                ("basket.en".to_string(), "basket".to_string()),
                ("basket.de".to_string(), "basket".to_string()),
                ("contact.en".to_string(), "contact".to_string()),
                ("contact.nl".to_string(), "contact".to_string()),
                ("contact".to_string(), "contact".to_string()),
            ]
        );
    }

    #[test]
    fn a_localized_route_annotation_is_declared_per_locale() {
        let php = "<?php\nnamespace App\\Controller;\nuse Symfony\\Component\\Routing\\Annotation\\Route;\nclass ContactController {\n    /**\n     * @Route({\"en\": \"/contact\", \"nl\": \"/contact-nl\"}, name=\"contact\")\n     */\n    public function contact() {}\n    /**\n     * @Route(path={\n     *     en=\"/about\",\n     *     \"de\" = \"/{a}\"\n     * }, requirements={\"a\": \"x\"}, name=\"about\")\n     */\n    public function about() {}\n    /** @Route({\"/a\", \"/b\"}, name=\"list\") */\n    public function list() {}\n}\n";
        // A path written as a list is not localized.
        assert_eq!(
            declared_routes(&map_of(php), php),
            vec![
                ("contact.en".to_string(), "contact".to_string()),
                ("contact.nl".to_string(), "contact".to_string()),
                ("contact".to_string(), "contact".to_string()),
                ("about.en".to_string(), "about".to_string()),
                ("about.de".to_string(), "about".to_string()),
                ("about".to_string(), "about".to_string()),
                ("list".to_string(), "list".to_string()),
            ]
        );
    }

    /// The key and the guards of each lookup site of `map`.
    fn site_guards(map: &SymbolMap) -> Vec<(&str, u8)> {
        map.typed_receiver_sites
            .iter()
            .filter(|site| !site.is_write)
            .map(|site| (site.key.as_str(), site.guards))
            .collect()
    }

    #[test]
    fn a_catch_of_the_not_found_exception_guards_the_lookups_in_its_try() {
        use crate::symbol_map::LookupGuards;
        let php = "<?php\nnamespace App\\Controller;\nuse Symfony\\Component\\Routing\\Exception\\RouteNotFoundException;\nuse Twig\\Error\\LoaderError;\nfinal class Links {\n    public function links($urls, $twig, $container): void {\n        try {\n            $urls->generate('legacy');\n            $twig->render('legacy.html.twig');\n        } catch (RouteNotFoundException) {}\n        try {\n            $twig->render('maybe.html.twig');\n        } catch (\\RuntimeException|LoaderError $e) {}\n        try {\n            $container->getParameter('app.maybe');\n        } catch (\\Exception) {}\n        $urls->generate('outside');\n    }\n}\n";
        assert_eq!(
            site_guards(&map_of(php)),
            vec![
                ("legacy", LookupGuards::ROUTE),
                ("legacy.html.twig", LookupGuards::ROUTE),
                ("maybe.html.twig", LookupGuards::TEMPLATE),
                (
                    "app.maybe",
                    LookupGuards::ROUTE | LookupGuards::TEMPLATE | LookupGuards::PARAMETER
                ),
                ("outside", 0),
            ]
        );
    }

    #[test]
    fn lookups_on_a_router_or_twig_the_file_builds_are_its_own() {
        use crate::symbol_map::LookupGuards;
        let php = "<?php\nnamespace App\\Tests;\nuse Symfony\\Component\\Routing\\Generator\\UrlGenerator;\nuse Symfony\\Component\\Routing\\RouteCollection;\nuse Twig\\Environment;\nuse Twig\\Loader\\ArrayLoader;\nfinal class GeneratorTest {\n    public function test(Environment $app): void {\n        $generator->generate('before_it_is_built');\n        $generator = new UrlGenerator(new RouteCollection(), $context);\n        $generator->generate('local_only');\n        (new UrlGenerator($routes, $context))->generate('inline');\n        $this->twig->render('a.html.twig');\n        $app->render('app.html.twig');\n        $this->render('page.html.twig');\n    }\n    protected function setUp(): void {\n        $this->twig = new Environment(new ArrayLoader(['a.html.twig' => '']));\n    }\n}\n";
        // What a variable is assigned anywhere in the file is what it holds,
        // and an environment handed in, or a controller's own `render()`,
        // is the application's.
        assert_eq!(
            site_guards(&map_of(php)),
            vec![
                ("before_it_is_built", LookupGuards::ROUTE),
                ("local_only", LookupGuards::ROUTE),
                ("inline", LookupGuards::ROUTE),
                ("a.html.twig", LookupGuards::TEMPLATE),
                ("app.html.twig", 0),
                ("page.html.twig", 0),
            ]
        );
    }

    #[test]
    fn a_kernel_says_how_routes_and_parameters_are_loaded() {
        let php = "<?php\nnamespace App;\nuse Symfony\\Component\\HttpKernel\\Kernel as BaseKernel;\nclass Kernel extends BaseKernel {\n    public function loadRoutes($loader): object {}\n    protected function getKernelParameters(): array {\n        $parameters = parent::getKernelParameters() + ['app.from_kernel' => ['nested' => true]];\n        $parameters['app.set'] = 1;\n        $parameters['app.' . $this->name] = 2;\n        return $parameters;\n    }\n}\n";
        let map = map_of(php);
        assert_eq!(
            keys_of(&map, crate::symbol_map::StringKeyKind::ContainerParameter),
            vec![
                ("app.from_kernel".to_string(), true),
                ("app.set".to_string(), true),
                ("app.*".to_string(), true),
            ]
        );
        // Every route the kernel loads is one that cannot be read.
        assert_eq!(
            map.route_imports,
            vec![RouteImport {
                resource: None,
                loader: None,
                name_prefix: String::new(),
            }]
        );
        // A class that is no kernel says nothing.
        let php = php.replace("extends BaseKernel", "");
        let map = map_of(&php);
        assert!(map.route_imports.is_empty());
        assert!(keys_of(&map, crate::symbol_map::StringKeyKind::ContainerParameter).is_empty());
    }

    #[test]
    fn set_parameter_declares_what_it_can_read() {
        let php = "<?php\nnamespace App;\nfinal class Pass {\n    public function process($container, array $config): void {\n        $container->setParameter('app.untyped', true);\n        foreach ($config as $key => $value) {\n            $container->setParameter('app.' . $key, $value);\n        }\n    }\n}\n";
        let map = map_of(php);
        // A literal `app.` name is declared outright; one built at runtime
        // is left for the receiver's type to confirm, as a pattern.
        assert_eq!(
            keys_of(&map, crate::symbol_map::StringKeyKind::ContainerParameter),
            vec![("app.untyped".to_string(), true)]
        );
        assert_eq!(
            receivers(&map),
            vec![("app.*", ReceiverClass::ParameterHolder, true)]
        );
    }

    #[test]
    fn a_twig_loader_of_the_project_is_noted() {
        let loader = "<?php\nnamespace App\\Twig;\nfinal class DatabaseLoader implements \\Twig\\Loader\\LoaderInterface {}\n";
        assert!(map_of(loader).declares_twig_loader);
        let subclass = "<?php\nnamespace App\\Twig;\nuse Twig\\Loader\\FilesystemLoader;\nfinal class ThemeLoader extends FilesystemLoader {}\n";
        assert!(map_of(subclass).declares_twig_loader);
        let other =
            "<?php\nnamespace App\\Twig;\nfinal class Loader implements LoaderInterface {}\n";
        assert!(!map_of(other).declares_twig_loader);
    }

    #[test]
    fn an_invokable_controller_named_by_its_own_constant() {
        let php = "<?php\nnamespace App\\Controller;\nuse Symfony\\Component\\Routing\\Attribute\\Route;\n#[Route(\n    path: '/healthcheck',\n    name: self::ROUTE,\n)]\nfinal class HealthCheckController\n{\n    public const string ROUTE = 'health-check';\n\n    public function __invoke(): void {}\n}\n";
        let map = map_of(php);
        assert_eq!(
            declared_routes(&map, php),
            vec![("health-check".to_string(), "self::ROUTE".to_string())]
        );
        assert!(map.route_imports.is_empty());
    }

    #[test]
    fn configurator_collections_and_imports_are_recorded() {
        let php = "<?php\nuse Symfony\\Component\\Routing\\Loader\\Configurator\\RoutingConfigurator;\nreturn static function (RoutingConfigurator $routes, $container): void {\n    $routes->add('home', '/');\n    $routes->collection('blog_')->collection('post_')->add('show', '/x')->add('edit', '/y');\n    $archive = $routes->collection('archive_');\n    $archive->add('list', '/archive');\n    $archive = $routes;\n    $archive->add('top', '/top');\n    $routes->collection($name)->add('lost', '/lost');\n    $routes->import('../src/Admin/', 'attribute')->prefix('/admin')->namePrefix('admin_');\n    $routes->import(__DIR__.'/legacy.yaml');\n    $container->import('../config/packages/*.yaml');\n};\n";
        let map = map_of(php);
        let added: Vec<&str> = receivers(&map)
            .into_iter()
            .filter(|(_, receiver, is_write)| {
                *receiver == ReceiverClass::RoutingConfigurator && *is_write
            })
            .map(|(key, _, _)| key)
            .collect();
        assert_eq!(
            added,
            vec![
                "home",
                "blog_post_show",
                "blog_post_edit",
                "archive_list",
                "top"
            ]
        );
        let import =
            |resource: Option<&str>, loader: Option<&str>, name_prefix: &str| RouteImport {
                resource: resource.map(str::to_string),
                loader: loader.map(str::to_string),
                name_prefix: name_prefix.to_string(),
            };
        // The collection named by a variable leaves every name open, and
        // the container's `import()` loads no routes.
        assert_eq!(
            map.route_imports,
            vec![
                import(None, None, ""),
                import(Some("../src/Admin/"), Some("attribute"), "admin_"),
                import(Some("./legacy.yaml"), None, ""),
            ]
        );
    }

    #[test]
    fn derived_route_names_follow_the_framework() {
        assert_eq!(
            super::default_route_name("App\\Controller\\DemoController", "index", 0),
            "app_demo_index"
        );
        assert_eq!(
            super::default_route_name(
                "Acme\\BlogBundle\\Controller\\PostController",
                "listAction",
                0
            ),
            "acme_blog_post_list"
        );
        assert_eq!(
            super::default_route_name("App\\Controller\\DemoController", "listAction", 2),
            "app_demo_list_2"
        );
    }

    /// The string keys of `kind` a map holds, with whether each declares.
    fn keys_of(map: &SymbolMap, kind: crate::symbol_map::StringKeyKind) -> Vec<(String, bool)> {
        map.spans
            .iter()
            .filter_map(|span| match &span.kind {
                crate::symbol_map::SymbolKind::StringKey {
                    kind: span_kind,
                    key,
                    is_write,
                    ..
                } if *span_kind == kind => Some((key.clone(), *is_write)),
                _ => None,
            })
            .collect()
    }

    /// The methods a map's member accesses name, with their subject.
    fn methods(map: &SymbolMap, php: &str) -> Vec<(String, String)> {
        let source = map.source(php).expect("the map is built from `php`");
        map.spans
            .iter()
            .filter_map(|span| match &span.kind {
                crate::symbol_map::SymbolKind::MemberAccess {
                    subject_text,
                    member_name,
                    ..
                } => Some((
                    subject_text.as_str(source).to_string(),
                    member_name.to_string(),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn listeners_declare_their_events_and_name_their_methods() {
        let php = "<?php\nnamespace App\\Listener;\nuse Symfony\\Component\\EventDispatcher\\Attribute\\AsEventListener;\nuse Symfony\\Component\\EventDispatcher\\EventSubscriberInterface;\n#[AsEventListener(event: 'app.order.placed', method: 'onPlaced')]\n#[AsEventListener('app.order.shipped')]\nclass OrderListener implements EventSubscriberInterface {\n    #[AsEventListener(event: 'app.order.paid')]\n    public function onPaid(): void {}\n    public static function getSubscribedEvents(): array {\n        return [\n            'app.order.cancelled' => 'onCancelled',\n            'app.order.refunded' => ['onRefunded', 10],\n            'app.order.returned' => [['onReturned', 10], ['onRestocked']],\n            Other::NAME => 'onOther',\n        ];\n    }\n}\n";
        let map = map_of(php);
        let events: Vec<String> = keys_of(&map, crate::symbol_map::StringKeyKind::SymfonyEvent)
            .into_iter()
            .map(|(key, is_write)| {
                assert!(is_write, "a subscription declares its event");
                key
            })
            .collect();
        assert_eq!(
            events,
            vec![
                "app.order.placed",
                "app.order.shipped",
                "app.order.paid",
                "app.order.cancelled",
                "app.order.refunded",
                "app.order.returned",
            ]
        );
        let subject = "\\App\\Listener\\OrderListener".to_string();
        let named: Vec<String> = methods(&map, php)
            .into_iter()
            .filter(|(on, _)| *on == subject)
            .map(|(_, method)| method)
            .collect();
        assert_eq!(
            named,
            vec![
                "onPlaced",
                "onCancelled",
                "onRefunded",
                "onReturned",
                "onRestocked",
                "onOther",
            ]
        );
    }

    #[test]
    fn message_handlers_are_recorded_with_their_messages_and_buses() {
        let php = "<?php\nnamespace App\\Handler;\nuse App\\Message\\{PlaceOrder, CancelOrder, RefundOrder};\nuse Symfony\\Component\\Messenger\\Attribute\\AsMessageHandler;\nuse Symfony\\Component\\Messenger\\Stamp\\BusNameStamp;\n#[AsMessageHandler(bus: 'command.bus')]\nfinal class PlaceOrderHandler {\n    public function __invoke(PlaceOrder $message): void {\n        new BusNameStamp('query.bus');\n    }\n}\n#[AsMessageHandler(handles: RefundOrder::class, method: 'refund')]\nfinal class RefundHandler {\n    public function refund(object $message): void {}\n}\nfinal class OrderHandlers {\n    #[AsMessageHandler]\n    public function cancel(CancelOrder|PlaceOrder $message): void {}\n    public function notAHandler(PlaceOrder $message): void {}\n}\n";
        let map = map_of(php);
        let sites: Vec<(&str, &str, &str)> = map
            .messenger_handlers
            .iter()
            .map(|site| {
                (
                    site.handler.as_str(),
                    site.method.as_str(),
                    site.message.as_str(),
                )
            })
            .collect();
        assert_eq!(
            sites,
            vec![
                (
                    "App\\Handler\\PlaceOrderHandler",
                    "__invoke",
                    "App\\Message\\PlaceOrder"
                ),
                (
                    "App\\Handler\\RefundHandler",
                    "refund",
                    "App\\Message\\RefundOrder"
                ),
                (
                    "App\\Handler\\OrderHandlers",
                    "cancel",
                    "App\\Message\\CancelOrder"
                ),
                (
                    "App\\Handler\\OrderHandlers",
                    "cancel",
                    "App\\Message\\PlaceOrder"
                ),
            ]
        );
        assert_eq!(
            keys_of(&map, crate::symbol_map::StringKeyKind::MessengerBus),
            vec![
                ("command.bus".to_string(), false),
                ("query.bus".to_string(), false),
            ]
        );
        assert!(methods(&map, php).contains(&(
            "\\App\\Handler\\RefundHandler".to_string(),
            "refund".to_string()
        )));
    }

    #[test]
    fn dispatcher_calls_are_recorded_for_the_type_engine() {
        let php = "<?php\nfunction f($events) {\n    $events->dispatch($event, 'app.order.placed');\n    $events->addListener('app.order.paid', $listener);\n    $events->hasListeners('app.order.shipped');\n}\n";
        assert_eq!(
            receivers(&map_of(php)),
            vec![
                ("app.order.placed", ReceiverClass::EventDispatcher, false),
                ("app.order.paid", ReceiverClass::EventListeners, true),
                ("app.order.shipped", ReceiverClass::EventListeners, false),
            ]
        );
    }

    #[test]
    fn nothing_is_recorded_outside_a_symfony_project() {
        let php = "<?php\nfunction f($container) { $container->get('app.mailer'); }\n";
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        let map = crate::symbol_map::extraction::extract_symbol_map_for_index(
            program,
            php,
            &names,
            crate::symbol_map::extraction::Frameworks {
                symfony: false,
                service_container: false,
                drupal: false,
                twig: false,
                doctrine: false,
                yii_params: false,
            },
            None,
        );
        assert!(map.typed_receiver_sites.is_empty());
    }

    #[test]
    fn a_drupal_project_records_container_reads_but_no_declarations() {
        let php = "<?php\nfunction f($container, $violation) {\n    $container->get('acme.greeter');\n    $container->getParameter('acme.limit');\n    $violation->setParameter('@value', $value);\n}\n";
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        let map = crate::symbol_map::extraction::extract_symbol_map_for_index(
            program,
            php,
            &names,
            crate::symbol_map::extraction::Frameworks {
                symfony: false,
                service_container: true,
                drupal: false,
                twig: true,
                doctrine: false,
                yii_params: false,
            },
            None,
        );
        assert_eq!(
            receivers(&map),
            vec![
                ("acme.greeter", ReceiverClass::ContainerLookup, false),
                ("acme.limit", ReceiverClass::ParameterHolder, false),
            ]
        );
    }

    #[test]
    fn only_renders_are_recorded_in_a_twig_project() {
        let php = "<?php\nfunction f($container, $twig) {\n    $container->get('app.mailer');\n    $twig->render('index.html');\n}\n";
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        let map = crate::symbol_map::extraction::extract_symbol_map_for_index(
            program,
            php,
            &names,
            crate::symbol_map::extraction::Frameworks {
                symfony: false,
                service_container: false,
                drupal: false,
                twig: true,
                doctrine: false,
                yii_params: false,
            },
            None,
        );
        assert_eq!(
            receivers(&map),
            vec![("index.html", ReceiverClass::TemplateRenderer, false)]
        );
    }
}
