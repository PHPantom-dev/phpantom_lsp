//! The names Symfony takes as strings in PHP: service IDs, container
//! parameters, route names, translation keys and domains, Twig templates,
//! the configuration keys a bundle's schema declares, the data-class
//! properties a form type's fields map to, event names and the listener
//! methods that subscribe to them, and Messenger bus names.  The Messenger
//! handlers a class registers are recorded beside them.
//!
//! The configurator functions (`service('app.mailer')`, `param('app.name')`),
//! the `#[Autowire]`, `#[Route]`, `#[Template]`, `#[AsEventListener]`, and
//! `#[AsMessageHandler]` attributes, `TranslatableMessage`, `BusNameStamp`,
//! and `t()` are recognised by their resolved name.
//! A method call names one only on the right receiver: `get()` on a
//! container, `set()` on a configurator, `generate()` on a URL generator,
//! `trans()` on a translator, `render()` on a controller or Twig,
//! `dispatch()` on an event dispatcher.  Those names are far too common to
//! claim on sight, and the receiver's spelling does not settle what it is
//! (`$container` may be anything), so each call is recorded as a
//! [`TypedReceiverSite`] for the type engine to confirm (see
//! [`symfony_receiver_method`]).

use super::*;
use crate::symbol_map::{
    ContainerValue, ContainerValueSite, MessengerHandlerSite, ReceiverClass,
    SYMFONY_CONTAINER_CONFIGURATOR_NS, StringKeyKind,
};
use crate::symfony::translations::{DEFAULT_DOMAIN, is_domain_name, translation_key};
use crate::virtual_members::laravel::helpers::{resolved_string_literal, string_literal_at_range};

const CONFIGURATOR_NS: &str = SYMFONY_CONTAINER_CONFIGURATOR_NS;
const AUTOWIRE_ATTRIBUTE: &str = "Symfony\\Component\\DependencyInjection\\Attribute\\Autowire";
const TEMPLATE_ATTRIBUTE: &str = "Symfony\\Bridge\\Twig\\Attribute\\Template";
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
        Some(b'g' | b'h' | b's' | b'a' | b'd' | b'r' | b't')
    ) {
        return None;
    }
    let is = |name: &str| member_name.eq_ignore_ascii_case(name);
    match index {
        0 if is("get") || is("has") => Some((ReceiverClass::ContainerLookup, false)),
        0 if is("generateUrl") || is("redirectToRoute") => {
            Some((ReceiverClass::ControllerRouting, false))
        }
        0 if is("generate") => Some((ReceiverClass::UrlGenerator, false)),
        0 if is("add") => Some((ReceiverClass::RoutingConfigurator, true)),
        0 if is("getParameter") || is("hasParameter") => {
            Some((ReceiverClass::ParameterHolder, false))
        }
        0 if is("setParameter") => Some((ReceiverClass::ParameterHolder, true)),
        0 if is("set") => Some((ReceiverClass::ConfiguratorSet, true)),
        // `alias('app.mailer', 'app.smtp_mailer')` declares the first ID
        // as another name for the second.
        0 if is("alias") => Some((ReceiverClass::ServicesConfigurator, true)),
        1 if is("alias") => Some((ReceiverClass::ServicesConfigurator, false)),
        0 if is("decorate") => Some((ReceiverClass::ServiceConfigurator, false)),
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
        0 if is("htmlTemplate") || is("textTemplate") => {
            Some((ReceiverClass::TemplatedEmail, false))
        }
        1 if is("dispatch") => Some((ReceiverClass::EventDispatcher, false)),
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
    if !ctx.symfony && !ctx.twig {
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
    for (index, argument) in argument_list.arguments.iter().take(2).enumerate() {
        let Argument::Positional(argument) = argument else {
            break;
        };
        let Some((receiver, is_write)) = symfony_receiver_method(member_name, index) else {
            continue;
        };
        // Outside Symfony, Twig's own `render()` is the only call left.
        if !ctx.symfony && receiver != ReceiverClass::TemplateRenderer {
            continue;
        }
        if let Some(namespace) = receiver.configurator_namespace()
            && !uses_configurators(namespace, ctx)
        {
            continue;
        }
        let Some((key, start, end)) = resolved_string_literal(argument.value, ctx.content) else {
            continue;
        };
        // `render('…')` is common enough elsewhere that only a string that
        // looks like a template is worth asking the type engine about.
        if key.is_empty()
            || (receiver.names_twig_templates()
                && !crate::twig::templates::looks_like_template_name(key))
        {
            continue;
        }
        ctx.typed_receiver_sites.push(TypedReceiverSite {
            start: start as u32,
            end: end as u32,
            key: key.to_string(),
            is_write,
            is_optional: false,
            receiver,
        });
        if is_write {
            record_declared_value(receiver, start as u32, argument_list, ctx);
        }
    }
}

/// Record what a configurator's `set('app.mailer', Mailer::class)` or
/// `alias('app.mailer', 'app.smtp_mailer')` declares its name as, keyed
/// by the name's site at `start`.
fn record_declared_value(
    receiver: ReceiverClass,
    start: u32,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(Argument::Positional(value)) = argument_list.arguments.get(1) else {
        return;
    };
    let value = match receiver {
        ReceiverClass::ConfiguratorSet => configured_value(value.value, ctx),
        ReceiverClass::ServicesConfigurator => resolved_string_literal(value.value, ctx.content)
            .filter(|(target, _, _)| !target.is_empty())
            .map(|(target, _, _)| ContainerValue::Alias(crate::atom::atom(target))),
        _ => None,
    };
    if let Some(value) = value {
        ctx.container_values
            .push(ContainerValueSite { start, value });
    }
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
    // The configurator methods in between (`->args()`, `->tag()`) return
    // the same service's configurator.
    let mut link = object;
    let set = loop {
        let Expression::Call(Call::Method(call)) = link else {
            return;
        };
        if let ClassLikeMemberSelector::Identifier(name) = &call.method
            && name.value.eq_ignore_ascii_case(b"set")
        {
            break call;
        }
        link = call.object;
    };
    let Some(Argument::Positional(id)) = set.argument_list.arguments.first() else {
        return;
    };
    let Some((_, start, _)) = resolved_string_literal(id.value, ctx.content) else {
        return;
    };
    if !uses_configurators(CONFIGURATOR_NS, ctx) {
        return;
    }
    if let Some(value @ ContainerValue::Class(_)) = configured_value(class.value, ctx) {
        ctx.container_values.push(ContainerValueSite {
            start: start as u32,
            value,
        });
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
fn is_route_attribute(class_name: &str) -> bool {
    let class_name = class_name.trim_start_matches('\\');
    ROUTE_ATTRIBUTES
        .iter()
        .any(|route| class_name.eq_ignore_ascii_case(route))
}

/// The arguments of one `#[Route]` attribute that bear on its name.
#[derive(Default)]
struct RouteArguments<'e, 'a> {
    path: Option<&'e Expression<'a>>,
    name: Option<&'e Expression<'a>>,
    alias: Option<&'e Expression<'a>>,
}

/// The `#[Route]` attributes in `attribute_lists`, by their arguments.
fn route_attributes<'e, 'a>(
    attribute_lists: &'e Sequence<'a, AttributeList<'a>>,
    ctx: &ExtractionCtx<'_>,
) -> Vec<RouteArguments<'e, 'a>> {
    let mut routes = Vec::new();
    for attribute in attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
    {
        let written = bytes_to_str(attribute.name.value());
        let resolved = ctx
            .resolved_name_at(attribute.name.span().start.offset)
            .unwrap_or(written);
        if !is_route_attribute(resolved) {
            continue;
        }
        let mut route = RouteArguments::default();
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
                    "path" => route.path = Some(value),
                    "name" => route.name = Some(value),
                    "alias" => route.alias = Some(value),
                    _ => {}
                }
            }
        }
        routes.push(route);
    }
    routes
}

/// Emit the route names a class's `#[Route]` attributes declare.
///
/// A route on a method is named by its `name:`, after the name of the
/// class's own first `#[Route]`, which prefixes every route in it.  A route
/// with no name is given the one Symfony derives from the class and the
/// method, spanning its path.  A class with no routes on its methods and an
/// `__invoke()` is a single action, and its own `#[Route]`s are its routes.
/// An alias is declared as it is written, without the prefix.
pub(in crate::symbol_map::extraction) fn emit_symfony_route_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    let class_routes = route_attributes(&class.attribute_lists, ctx);
    // Derived names are spelled from the class an action is loaded with,
    // which an abstract one never is.
    let derive = (!class.modifiers.contains_abstract())
        .then(|| ctx.resolved_name_at(class.name.span.start.offset))
        .flatten();
    let mut spans = Vec::new();

    let prefix = class_routes
        .first()
        .and_then(|route| route.name)
        .and_then(|name| resolved_string_literal(name, ctx.content))
        .map_or("", |(name, _, _)| name);
    let mut any_method_route = false;
    let mut invoke = None;
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let method_name = bytes_to_str(method.name.value);
        if method_name.eq_ignore_ascii_case("__invoke") {
            invoke = Some(method_name);
        }
        let routes = route_attributes(&method.attribute_lists, ctx);
        any_method_route |= !routes.is_empty();
        push_action_route_spans(
            &routes,
            prefix,
            derive,
            method_name,
            ctx.content,
            &mut spans,
        );
    }
    if !any_method_route && let Some(invoke) = invoke {
        push_action_route_spans(&class_routes, "", derive, invoke, ctx.content, &mut spans);
    }
    ctx.spans.extend(spans);
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
fn attributes_named<'e, 'a>(
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
fn attribute_arguments<'e, 'a>(
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
fn message_class(handles: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Option<String> {
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

/// The spans the `#[Route]`s of one action declare.  `derive_from` is the
/// class to derive a name from for a route that has none; Symfony numbers
/// the unnamed routes of one action from the second one on.
fn push_action_route_spans(
    routes: &[RouteArguments<'_, '_>],
    prefix: &str,
    derive_from: Option<&str>,
    method: &str,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let mut unnamed = 0;
    for route in routes {
        let derived = match (route.name, derive_from) {
            (None, Some(class)) => {
                unnamed += 1;
                Some(default_route_name(class, method, unnamed - 1))
            }
            _ => None,
        };
        push_route_spans(route, prefix, derived, content, spans);
    }
}

/// The spans one `#[Route]` declares: its name, or the `derived` one over
/// its path, and its aliases.
fn push_route_spans(
    route: &RouteArguments<'_, '_>,
    prefix: &str,
    derived: Option<String>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let declare = |spans: &mut Vec<SymbolSpan>, key: String, start: usize, end: usize| {
        spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::SymfonyRoute,
                key,
                is_write: true,
                is_optional: false,
            },
        });
    };
    match route.name {
        Some(name) => {
            if let Some((name, start, end)) = resolved_string_literal(name, content) {
                declare(spans, format!("{prefix}{name}"), start, end);
            }
        }
        None => {
            if let Some(derived) = derived
                && let Some((_, start, end)) = route
                    .path
                    .and_then(|path| resolved_string_literal(path, content))
            {
                declare(spans, format!("{prefix}{derived}"), start, end);
            }
        }
    }
    let aliases: Vec<&Expression<'_>> = match route.alias {
        Some(Expression::Array(array)) => array
            .elements
            .iter()
            .filter_map(|element| match element {
                ArrayElement::Value(value) => Some(value.value),
                _ => None,
            })
            .collect(),
        Some(alias) => vec![alias],
        None => Vec::new(),
    };
    for alias in aliases {
        if let Some((alias, start, end)) = resolved_string_literal(alias, content) {
            declare(spans, alias.to_string(), start, end);
        }
    }
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
    use crate::symbol_map::{ReceiverClass, SymbolMap};

    fn map_of(php: &str) -> SymbolMap {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        crate::symbol_map::extraction::extract_symbol_map_for_index(
            program, php, &names, true, true, None,
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
                ("app.alias", ReceiverClass::ServicesConfigurator, true),
                ("app.mailer", ReceiverClass::ServicesConfigurator, false),
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

    fn declared_routes(map: &SymbolMap, php: &str) -> Vec<(String, String)> {
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
            program, php, &names, false, false, None,
        );
        assert!(map.typed_receiver_sites.is_empty());
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
            program, php, &names, false, true, None,
        );
        assert_eq!(
            receivers(&map),
            vec![("index.html", ReceiverClass::TemplateRenderer, false)]
        );
    }
}
