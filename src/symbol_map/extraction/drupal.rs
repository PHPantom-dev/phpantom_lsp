//! The entity types a Drupal entity class declares, by attribute or by the
//! annotation older code uses, the route names Drupal's static and
//! constructor calls read, the libraries a render array attaches, and the
//! methods and functions a form or render array's callbacks name (see
//! [`crate::drupal`]).

use super::*;
use crate::symbol_map::extraction::symfony::{
    RouteDeclaration, RouteName, is_route_attribute, push_route_spans, route_declarations,
};
use crate::symbol_map::{
    DeclaredParameter, DrupalRouteSite, EntityTypeSite, ModuleGuard, PluginClassSite,
    PluginDeclarationSite, PluginManagerSite, ReceiverClass, StaticPluginSite, StringKeyKind,
    TypedReceiverSite,
};
use crate::virtual_members::laravel::helpers::resolved_string_literal;
use mago_syntax::cst::sequence::TokenSeparatedSequence;

const CONTENT_STORAGE: &str = "Drupal\\Core\\Entity\\Sql\\SqlContentEntityStorage";
const CONFIG_STORAGE: &str = "Drupal\\Core\\Config\\Entity\\ConfigEntityStorage";

/// An entity type attribute and the annotation older code writes instead,
/// with the storage a declaration that names none gets.
struct EntityTypeKind {
    attribute: &'static str,
    /// The annotation's opening, matched by short name, the way Drupal's
    /// annotation discovery reads it.
    annotation: &'static str,
    default_storage: Option<&'static str>,
}

const ENTITY_TYPE_KINDS: [EntityTypeKind; 3] = [
    EntityTypeKind {
        attribute: "Drupal\\Core\\Entity\\Attribute\\ContentEntityType",
        annotation: "@ContentEntityType(",
        default_storage: Some(CONTENT_STORAGE),
    },
    EntityTypeKind {
        attribute: "Drupal\\Core\\Entity\\Attribute\\ConfigEntityType",
        annotation: "@ConfigEntityType(",
        default_storage: Some(CONFIG_STORAGE),
    },
    EntityTypeKind {
        attribute: "Drupal\\Core\\Entity\\Attribute\\EntityType",
        annotation: "@EntityType(",
        default_storage: None,
    },
];

/// Whether `attribute`, a resolved class name, is one of the attributes
/// that declare an entity type.
pub(crate) fn is_entity_type_attribute(attribute: &str) -> bool {
    let attribute = attribute.trim_start_matches('\\');
    ENTITY_TYPE_KINDS
        .iter()
        .any(|kind| kind.attribute.eq_ignore_ascii_case(attribute))
}

/// Record the entity type `class` declares: a span over its ID, and the
/// class and storage the ID stands for, and a span over the permission its
/// `admin_permission` names.
pub(in crate::symbol_map::extraction) fn emit_drupal_entity_type_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let mut found = Vec::new();
    let mut admin_permissions = Vec::new();
    for kind in &ENTITY_TYPE_KINDS {
        for attribute in attributes_named(&class.attribute_lists, kind.attribute, ctx) {
            let mut id = None;
            let mut storage = kind.default_storage.map(str::to_string);
            for (parameter, value) in attribute_arguments(attribute, &["id"]) {
                match parameter {
                    "id" => id = resolved_string_literal(value, ctx.content),
                    "handlers" => {
                        if let Some(named) = handler_class(value, "storage", ctx) {
                            storage = Some(named);
                        }
                    }
                    "admin_permission" => {
                        push_laravel_string_span(
                            StringKeyKind::DrupalPermission,
                            false,
                            false,
                            value,
                            ctx.content,
                            &mut admin_permissions,
                        );
                    }
                    _ => {}
                }
            }
            if let Some((key, start, end)) = id.filter(|(key, _, _)| !key.is_empty()) {
                found.push((key.to_string(), start as u32, end as u32, storage));
            }
        }
    }
    if found.is_empty()
        && let Some((doc, offset)) = get_docblock_text_with_offset(ctx.trivias, ctx.content, class)
        && doc.contains("EntityType(")
    {
        found.extend(annotation_entity_type(doc, offset));
        if !found.is_empty() {
            admin_permissions.extend(annotation_admin_permission(doc, offset));
        }
    }
    if found.is_empty() {
        return;
    }
    ctx.spans.extend(admin_permissions);
    let Some(fqn) = ctx
        .resolved_name_at(class.name.span.start.offset)
        .map(|fqn| crate::atom::atom(fqn.trim_start_matches('\\')))
    else {
        return;
    };
    for (key, start, end, storage) in found {
        ctx.spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::DrupalEntityType,
                key,
                is_write: true,
                is_optional: false,
            },
        });
        ctx.entity_types.push(EntityTypeSite {
            start,
            class: fqn,
            storage: storage.map(|storage| crate::atom::atom(&storage)),
        });
    }
}

/// A Drupal plugin attribute and the annotation older code writes instead.
struct PluginMarker {
    attribute: &'static str,
    /// The annotation's opening, matched by short name, the way Drupal's
    /// annotation discovery reads it.
    annotation: &'static str,
}

const RENDER_ELEMENT_MARKERS: [PluginMarker; 2] = [
    PluginMarker {
        attribute: "Drupal\\Core\\Render\\Attribute\\RenderElement",
        annotation: "@RenderElement(",
    },
    PluginMarker {
        attribute: "Drupal\\Core\\Render\\Attribute\\FormElement",
        annotation: "@FormElement(",
    },
];

/// Record the render element `class` declares: a span over its ID, and
/// the class beside it.
pub(in crate::symbol_map::extraction) fn emit_drupal_render_element_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    emit_drupal_plugin_spans(
        class,
        &RENDER_ELEMENT_MARKERS,
        StringKeyKind::DrupalRenderElement,
        ctx,
    );
}

/// Record the plugin ID `class` declares by one of the `markers`, as a key
/// of `kind`, with the class it is written on.  An attribute wins over an
/// annotation, as in Drupal's discovery.
fn emit_drupal_plugin_spans(
    class: &Class<'_>,
    markers: &[PluginMarker],
    kind: StringKeyKind,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let mut found = Vec::new();
    for marker in markers {
        for attribute in attributes_named(&class.attribute_lists, marker.attribute, ctx) {
            let id = attribute_arguments(attribute, &["id"])
                .into_iter()
                .find(|(parameter, _)| *parameter == "id")
                .and_then(|(_, value)| resolved_string_literal(value, ctx.content));
            if let Some((key, start, end)) = id {
                found.push((key.to_string(), start as u32, end as u32));
            }
        }
    }
    if found.is_empty()
        && let Some((doc, offset)) = get_docblock_text_with_offset(ctx.trivias, ctx.content, class)
        && let Some((start, end)) = markers.iter().find_map(|marker| {
            let at = doc.find(marker.annotation)?;
            annotation_plugin_id(doc, at + marker.annotation.len())
        })
    {
        found.push((
            doc[start..end].to_string(),
            offset + start as u32,
            offset + end as u32,
        ));
    }
    found.retain(|(key, _, _)| !key.is_empty());
    if found.is_empty() {
        return;
    }
    let Some(fqn) = ctx
        .resolved_name_at(class.name.span.start.offset)
        .map(|fqn| crate::atom::atom(fqn.trim_start_matches('\\')))
    else {
        return;
    };
    for (key, start, end) in found {
        ctx.spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::StringKey {
                kind,
                key,
                is_write: true,
                is_optional: false,
            },
        });
        ctx.drupal_plugin_classes
            .push(PluginClassSite { start, class: fqn });
    }
}

/// The range of the ID a plugin annotation whose arguments start at `from`
/// in `doc` declares: its first argument when that is a string
/// (`@RenderElement("details")`), else its `id = "…"`.
fn annotation_plugin_id(doc: &str, from: usize) -> Option<(usize, usize)> {
    let rest = doc.get(from..)?;
    let first = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '*');
    if let Some(quoted) = first.strip_prefix('"') {
        let start = doc.len() - quoted.len();
        return Some((start, start + quoted.find('"')?));
    }
    annotation_entries(doc, from)
        .into_iter()
        .find(|entry| entry.path.is_empty() && entry.key == "id")
        .map(|entry| (entry.value_start, entry.value_end))
}

/// Record the ID each attribute of `class`, and the first annotation of its
/// docblock, give it, when the class is in a `Drupal\` namespace: the
/// plugins among them are told apart later, by the plugin types the
/// project's plugin managers declare (see [`crate::drupal::plugins`]).
pub(in crate::symbol_map::extraction) fn emit_drupal_plugin_declarations(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let Some(fqn) = ctx
        .resolved_name_at(class.name.span.start.offset)
        .map(|fqn| fqn.trim_start_matches('\\'))
        .filter(|fqn| fqn.starts_with("Drupal\\"))
        .map(crate::atom::atom)
    else {
        return;
    };
    for attribute in class
        .attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
    {
        // Plugin attributes take the ID first, by name or by place.  One
        // that is no string, such as a class constant, is recorded empty:
        // its plugin type then has a plugin whose ID is not known.
        let Some(value) = attribute_arguments(attribute, &["id"])
            .into_iter()
            .find(|(parameter, _)| *parameter == "id")
            .map(|(_, value)| value)
        else {
            continue;
        };
        let (id, start, end) = match resolved_string_literal(value, ctx.content) {
            Some((id, start, end)) if !id.is_empty() => (id, start, end),
            Some(_) => continue,
            None => {
                let span = value.span();
                ("", span.start.offset as usize, span.end.offset as usize)
            }
        };
        let written = bytes_to_str(attribute.name.value());
        let marker = ctx
            .resolved_name_at(attribute.name.span().start.offset)
            .unwrap_or(written)
            .trim_start_matches('\\');
        ctx.drupal_plugins.push(PluginDeclarationSite {
            start: start as u32,
            end: end as u32,
            id: id.to_string(),
            class: fqn,
            marker: crate::atom::atom(marker),
            is_annotation: false,
            doc_start: start as u32,
            doc_end: end as u32,
        });
    }
    let Some((doc, offset)) = get_docblock_text_with_offset(ctx.trivias, ctx.content, class) else {
        return;
    };
    if let Some((name, (start, end))) = doc_annotations(doc)
        .find_map(|(name, from)| Some((name, annotation_plugin_id(doc, from)?)))
        .filter(|(_, (start, end))| start < end)
    {
        ctx.drupal_plugins.push(PluginDeclarationSite {
            start: offset + start as u32,
            end: offset + end as u32,
            id: doc[start..end].to_string(),
            class: fqn,
            marker: crate::atom::atom(name),
            is_annotation: true,
            doc_start: offset,
            doc_end: offset + doc.len() as u32,
        });
    }
}

/// The annotations that open a line of the docblock `doc`: each one's name
/// as written, and the offset just past its `(`.
fn doc_annotations(doc: &str) -> impl Iterator<Item = (&str, usize)> {
    doc.match_indices('@').filter_map(move |(at, _)| {
        let line_start = doc[..at].rfind('\n').map_or(0, |newline| newline + 1);
        if !doc[line_start..at]
            .chars()
            .all(|c| c.is_whitespace() || c == '*' || c == '/')
        {
            return None;
        }
        let rest = &doc[at + 1..];
        let length = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\\'))
            .unwrap_or(rest.len());
        let name = &rest[..length];
        let opens = rest[length..].starts_with('(');
        let capitalised = name
            .trim_start_matches('\\')
            .starts_with(|c: char| c.is_ascii_uppercase());
        (opens && capitalised).then_some((name, at + 1 + length + 1))
    })
}

/// Record the plugin type a plugin manager's constructor declares by the
/// call it makes to its parent's: `parent::__construct('Plugin/Block',
/// $namespaces, $module_handler, BlockPluginInterface::class, Block::class,
/// 'Drupal\Core\Block\Annotation\Block')`.  Whether the parent is
/// `DefaultPluginManager` is told later, when the types are read.
pub(in crate::symbol_map::extraction) fn emit_drupal_plugin_manager(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let Some(body) = class.members.iter().find_map(|member| match member {
        ClassLikeMember::Method(method)
            if bytes_to_str(method.name.value).eq_ignore_ascii_case("__construct") =>
        {
            match &method.body {
                MethodBody::Concrete(body) => Some(body),
                MethodBody::Abstract(_) => None,
            }
        }
        _ => None,
    }) else {
        return;
    };
    const PARAMETERS: [&str; 6] = [
        "subdir",
        "namespaces",
        "module_handler",
        "plugin_interface",
        "plugin_definition_attribute_name",
        "plugin_definition_annotation_name",
    ];
    let Some(arguments) = body.statements.iter().find_map(|statement| {
        let Statement::Expression(statement) = statement else {
            return None;
        };
        let Expression::Call(Call::StaticMethod(call)) = statement.expression else {
            return None;
        };
        let ClassLikeMemberSelector::Identifier(method) = &call.method else {
            return None;
        };
        (matches!(call.class, Expression::Parent(_))
            && bytes_to_str(method.value).eq_ignore_ascii_case("__construct"))
        .then_some(&call.argument_list)
    }) else {
        return;
    };
    let argument = |wanted: usize| {
        arguments
            .arguments
            .iter()
            .enumerate()
            .find_map(|(index, argument)| match argument {
                Argument::Positional(positional) if index == wanted => Some(positional.value),
                Argument::Named(named) if bytes_to_str(named.name.value) == PARAMETERS[wanted] => {
                    Some(named.value)
                }
                _ => None,
            })
    };
    let Some((subdir, _, _)) = argument(0)
        .and_then(|value| resolved_string_literal(value, ctx.content))
        .filter(|(subdir, _, _)| !subdir.is_empty())
    else {
        return;
    };
    let Some(fqn) = ctx
        .resolved_name_at(class.name.span.start.offset)
        .map(|fqn| crate::atom::atom(fqn.trim_start_matches('\\')))
    else {
        return;
    };
    let class_at = |index: usize| {
        argument(index)
            .and_then(|value| message_class(value, ctx))
            .map(|name| crate::atom::atom(&name))
    };
    let discovery = read_manager_discovery(class, ctx);
    let site = PluginManagerSite {
        class: fqn,
        subdir: subdir.to_string(),
        fifth: class_at(4),
        sixth: class_at(5),
        sets_discovery: discovery.sets_discovery,
        decorates_discovery: discovery.decorates,
        static_plugins: discovery.static_plugins,
    };
    ctx.drupal_plugin_managers.push(site);
}

/// The decorators a manager's `getDiscovery()` may wrap its parent's
/// discovery in without finding plugins no class or [`StaticPluginSite`]
/// declares: derivatives are read from their base plugin's ID, and a static
/// decorator's plugins from the `setDefinition()` calls the class makes.
const ADDITIVE_DISCOVERY_DECORATORS: [&str; 3] = [
    "Drupal\\Component\\Plugin\\Discovery\\StaticDiscoveryDecorator",
    "Drupal\\Component\\Plugin\\Discovery\\DerivativeDiscoveryDecorator",
    "Drupal\\Core\\Plugin\\Discovery\\ContainerDerivativeDiscoveryDecorator",
];

/// How a plugin manager class finds its plugins, beyond what its parent's
/// discovery reads.
#[derive(Default)]
struct ManagerDiscovery {
    /// It may find plugins no class or `setDefinition()` call declares.
    sets_discovery: bool,
    /// It declares a `getDiscovery()` that only decorates its parent's.
    decorates: bool,
    static_plugins: Vec<StaticPluginSite>,
}

/// Read how the plugin manager `class` sets its discovery: each assignment
/// to `$this->discovery`, the `getDiscovery()` it declares, and the plugins
/// it declares by `setDefinition('id', [...])`.
///
/// A `getDiscovery()` that starts from `parent::getDiscovery()`, wraps it in
/// [`ADDITIVE_DISCOVERY_DECORATORS`] only, and returns `$this->discovery`
/// still finds every plugin the default discovery does, plus the ones a
/// `setDefinition()` with a literal ID adds, as core's constraint manager
/// does for Symfony's constraints.  Any other shape may find plugins
/// nothing here declares.
fn read_manager_discovery(class: &Class<'_>, ctx: &ExtractionCtx<'_>) -> ManagerDiscovery {
    use mago_syntax::cst::Node;
    let is_this_discovery = |expr: &Expression<'_>| {
        matches!(expr, Expression::Access(Access::Property(access))
            if matches!(access.object, Expression::Variable(Variable::Direct(variable)) if variable.name == b"$this")
                && matches!(&access.property, ClassLikeMemberSelector::Identifier(name) if name.value == b"discovery"))
    };
    let is_parent_discovery = |expr: &Expression<'_>| {
        matches!(expr, Expression::Call(Call::StaticMethod(call))
            if matches!(call.class, Expression::Parent(_))
                && matches!(&call.method, ClassLikeMemberSelector::Identifier(name)
                    if bytes_to_str(name.value).eq_ignore_ascii_case("getDiscovery")))
    };
    let is_additive_decorator = |expr: &Expression<'_>| {
        let Expression::Instantiation(instantiation) = expr else {
            return false;
        };
        let Expression::Identifier(identifier) = instantiation.class else {
            return false;
        };
        let written = bytes_to_str(identifier.value());
        let resolved = ctx
            .resolved_name_at(identifier.span().start.offset)
            .unwrap_or(written)
            .trim_start_matches('\\');
        ADDITIVE_DISCOVERY_DECORATORS
            .iter()
            .any(|decorator| decorator.eq_ignore_ascii_case(resolved))
            && instantiation
                .argument_list
                .as_ref()
                .and_then(|arguments| arguments.arguments.first())
                .is_some_and(|first| is_this_discovery(first.value()))
    };

    let mut discovery = ManagerDiscovery::default();
    let mut starts_from_parent = false;
    let mut declares_get_discovery = false;
    crate::parser::for_each_concrete_method(class.members.as_slice(), |method, body| {
        let is_get_discovery = bytes_to_str(method.name.value).eq_ignore_ascii_case("getDiscovery");
        declares_get_discovery |= is_get_discovery;
        crate::parser::for_each_node_with_ancestors(Node::Block(body), |node, _| match node {
            Node::Assignment(assignment) if is_this_discovery(assignment.lhs) => {
                if !is_get_discovery || !assignment.operator.is_assign() {
                    discovery.sets_discovery = true;
                } else if is_parent_discovery(assignment.rhs) {
                    starts_from_parent = true;
                } else if !is_additive_decorator(assignment.rhs) {
                    discovery.sets_discovery = true;
                }
            }
            Node::Return(statement) if is_get_discovery => {
                if !statement.value.is_some_and(is_this_discovery) {
                    discovery.sets_discovery = true;
                }
            }
            Node::MethodCall(call)
                if matches!(&call.method, ClassLikeMemberSelector::Identifier(name)
                    if bytes_to_str(name.value).eq_ignore_ascii_case("setDefinition")) =>
            {
                let mut arguments = call.argument_list.arguments.iter();
                let id = arguments
                    .next()
                    .and_then(|first| resolved_string_literal(first.value(), ctx.content))
                    .filter(|(id, _, _)| !id.is_empty());
                // A plugin is its ID and its class: one whose either is
                // unreadable leaves what the manager finds unknown.
                let class = arguments
                    .next()
                    .and_then(|definition| {
                        crate::parser::array_literal_elements(definition.value())
                    })
                    .and_then(|entries| {
                        entries.iter().find_map(|entry| match entry {
                            ArrayElement::KeyValue(entry)
                                if resolved_string_literal(entry.key, ctx.content)
                                    .is_some_and(|(key, _, _)| key == "class") =>
                            {
                                message_class(entry.value, ctx)
                            }
                            _ => None,
                        })
                    });
                match (id, class) {
                    (Some((id, _, _)), Some(class)) => {
                        discovery.static_plugins.push(StaticPluginSite {
                            id: id.to_string(),
                            class: crate::atom::atom(&class),
                        });
                    }
                    _ => discovery.sets_discovery = true,
                }
            }
            _ => {}
        });
    });
    if declares_get_discovery && !starts_from_parent {
        discovery.sets_discovery = true;
    }
    discovery.decorates = declares_get_discovery && !discovery.sets_discovery;
    if discovery.sets_discovery {
        // What the class finds is unknown anyway.
        discovery.static_plugins.clear();
    }
    discovery
}

/// Record the plugin IDs a method call names by its arguments rather than
/// by its receiver alone: the widget or formatter the `type` of
/// `setDisplayOptions('form' | 'view', [...])` names, for the type engine to
/// confirm the receiver is a field definition.
pub(in crate::symbol_map::extraction) fn record_drupal_plugin_sites(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal || !member_name.eq_ignore_ascii_case("setDisplayOptions") {
        return;
    }
    let mut arguments = argument_list.arguments.iter();
    let (Some(Argument::Positional(display)), Some(Argument::Positional(options))) =
        (arguments.next(), arguments.next())
    else {
        return;
    };
    let Some(manager) = resolved_string_literal(display.value, ctx.content)
        .and_then(|(display, _, _)| crate::drupal::plugins::display_manager(display))
    else {
        return;
    };
    let Some(entries) = crate::parser::array_literal_elements(options.value) else {
        return;
    };
    for entry in entries.iter() {
        let ArrayElement::KeyValue(entry) = entry else {
            continue;
        };
        if !resolved_string_literal(entry.key, ctx.content).is_some_and(|(key, _, _)| key == "type")
        {
            continue;
        }
        if let Some((id, start, end)) = resolved_string_literal(entry.value, ctx.content) {
            ctx.typed_receiver_sites.push(TypedReceiverSite {
                start: start as u32,
                end: end as u32,
                key: crate::drupal::plugins::plugin_key(manager, id),
                is_write: false,
                is_optional: false,
                guards: 0,
                receiver: ReceiverClass::FieldDisplayOptions,
            });
        }
    }
}

/// Record the field type `BaseFieldDefinition::create('string')` names.
pub(in crate::symbol_map::extraction) fn emit_drupal_static_plugin_span(
    class: &str,
    method: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let Some(manager) = crate::drupal::plugins::static_call_manager(class, method) else {
        return;
    };
    let Some((id, start, end)) = crate::call_args::first_param_arg(argument_list, "type")
        .and_then(|argument| resolved_string_literal(argument.value(), ctx.content))
    else {
        return;
    };
    ctx.spans.push(SymbolSpan {
        start: start as u32,
        end: end as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::DrupalPlugin,
            key: crate::drupal::plugins::plugin_key(manager, id),
            is_write: false,
            is_optional: crate::drupal::plugins::may_catch_missing_plugin(ctx.content),
        },
    });
}

/// Record the render element name a `'#type' => 'x'` entry of an array
/// literal reads.
pub(in crate::symbol_map::extraction) fn emit_drupal_render_element_reads(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal_type_keys {
        return;
    }
    for element in elements.iter() {
        if let ArrayElement::KeyValue(entry) = element {
            push_render_element_read(entry.key, entry.value, ctx);
        }
    }
}

/// Record the render element name a `$form['x']['#type'] = 'x'`
/// assignment reads.
pub(in crate::symbol_map::extraction) fn emit_drupal_render_element_assignment(
    assign: &Assignment<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal_type_keys || !assign.operator.is_assign() {
        return;
    }
    if let Expression::ArrayAccess(access) = assign.lhs {
        push_render_element_read(access.index, assign.rhs, ctx);
    }
}

/// Record `value` as the render element name it reads when `key` is the
/// literal `#type` and `value` a string literal.
fn push_render_element_read(
    key: &Expression<'_>,
    value: &Expression<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !resolved_string_literal(key, ctx.content).is_some_and(|(key, _, _)| key == "#type") {
        return;
    }
    let Some((name, start, end)) = resolved_string_literal(value, ctx.content) else {
        return;
    };
    ctx.spans.push(SymbolSpan {
        start: start as u32,
        end: end as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::DrupalRenderElement,
            key: name.to_string(),
            is_write: false,
            is_optional: false,
        },
    });
}

/// Record the routes the `#[Route]` attributes of a controller declare, the
/// way Drupal's attribute route discovery reads them.
///
/// Drupal looks for them in the classes under an extension's `Controller`
/// namespace.  A route on a method is named by its `name:`, after the name
/// of the class's own first `#[Route]`; one without a name is rejected.  A
/// class with no routes on its methods and an `__invoke()` is a single
/// action, and its own `#[Route]`s are its routes.  A method with exactly
/// one route is also reachable as `Class::method`, and a single-action
/// class with one route as `Class` and `Class::__invoke`: those aliases
/// have no text of their own, so they are kept beside the spans.
///
/// Drupal skips an abstract class but reads the routes its subclasses
/// inherit, so an abstract controller's routes are declared where they are
/// written, under its own prefix, and without aliases, whose class name
/// would be the subclass's.
///
/// Each placeholder of a route's path declares a parameter of the route,
/// the class route's path included, which comes before every method
/// route's.  A route is kept beside the spans with the method it
/// dispatches to, for its parameters to be upcast by (see
/// [`DrupalRouteSite`]); an abstract controller's are not, since that
/// method is the subclass's.
pub(in crate::symbol_map::extraction) fn emit_drupal_route_attribute_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let Some(fqn) = ctx
        .resolved_name_at(class.name.span.start.offset)
        .map(|fqn| fqn.trim_start_matches('\\'))
        .filter(|fqn| is_controller_class(fqn))
    else {
        return;
    };
    let is_abstract = class.modifiers.contains_abstract();
    // Drupal's discovery reads attributes only, never `@Route` annotations.
    let class_routes = route_declarations(&class.attribute_lists, None, class, ctx);
    let class_details = route_details(&class.attribute_lists, ctx);
    let mut spans: Vec<SymbolSpan> = class_details
        .iter()
        .flat_map(|details| details.permissions.iter().cloned())
        .collect();
    let prefix = match class_routes.first().map(|route| &route.name) {
        Some(RouteName::Literal(name, _, _)) => name.as_str(),
        _ => "",
    };
    let is_named = |route: &&RouteDeclaration| matches!(route.name, RouteName::Literal(..));
    let mut sites = Vec::new();
    let mut aliases = Vec::new();
    let mut route_count = 0usize;
    let mut any_method_route = false;
    let mut invokable = false;
    let mut fqcn_alias = false;
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let method_name = bytes_to_str(method.name.value);
        let is_invoke = method_name.eq_ignore_ascii_case("__invoke");
        invokable |= is_invoke;
        let routes = route_declarations(&method.attribute_lists, None, class, ctx);
        let details = route_details(&method.attribute_lists, ctx);
        spans.extend(
            details
                .iter()
                .flat_map(|details| details.permissions.iter().cloned()),
        );
        any_method_route |= !routes.is_empty();
        let named = routes.iter().filter(is_named).count();
        route_count += named;
        fqcn_alias |= is_invoke && named > 0;
        if named == 1 {
            aliases.push(format!("{fqn}::{method_name}"));
        }
        for (route, own) in routes.iter().zip(&details) {
            let RouteName::Literal(name, _, _) = &route.name else {
                continue;
            };
            push_route_spans(
                route,
                prefix,
                None,
                StringKeyKind::DrupalRoute,
                &[],
                &mut spans,
            );
            sites.extend(attribute_route(
                &format!("{prefix}{name}"),
                class_details.first(),
                own,
                format!("{fqn}::{method_name}"),
                &mut spans,
            ));
        }
    }
    if !any_method_route && invokable && !is_abstract {
        for (route, own) in class_routes.iter().zip(&class_details) {
            let RouteName::Literal(name, _, _) = &route.name else {
                continue;
            };
            route_count += 1;
            fqcn_alias = true;
            push_route_spans(route, "", None, StringKeyKind::DrupalRoute, &[], &mut spans);
            sites.extend(attribute_route(
                name,
                None,
                own,
                format!("{fqn}::__invoke"),
                &mut spans,
            ));
        }
    }
    if fqcn_alias && route_count == 1 {
        aliases.push(fqn.to_string());
        aliases.push(format!("{fqn}::__invoke"));
    }
    if !is_abstract {
        for alias in aliases {
            if !ctx.drupal_route_aliases.contains(&alias) {
                ctx.drupal_route_aliases.push(alias);
            }
        }
        ctx.drupal_routes.extend(sites);
    }
    ctx.spans.extend(spans);
}

/// What a `#[Route]` gives besides its name: its path, as written with the
/// offset of its text, and the defaults and options Drupal upcasts the
/// route's parameters by.
#[derive(Default)]
struct RouteDetails {
    path: Option<(String, u32)>,
    /// Whether the path is written as something other than a literal.
    path_unreadable: bool,
    /// The parameters `options` declares, or `None` when it has no
    /// `parameters`.
    parameters: Option<Vec<DeclaredParameter>>,
    /// The `defaults` keys that are not Drupal's own, with whether each is
    /// `NULL`.
    defaults: Vec<(String, bool)>,
    title_callback: Option<String>,
    entity_view: Option<String>,
    entity_form: Option<String>,
    /// The spans of the permissions its `_permission` requirement names.
    permissions: Vec<SymbolSpan>,
}

/// The details of each `#[Route]` among `attribute_lists`, in the order
/// [`route_declarations`] reads them.
fn route_details(
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
    ctx: &ExtractionCtx<'_>,
) -> Vec<RouteDetails> {
    let content = ctx.content;
    let literal = |value: &Expression<'_>| {
        resolved_string_literal(value, content).map(|(text, _, _)| text.to_string())
    };
    let mut found = Vec::new();
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
        let mut details = RouteDetails::default();
        for (parameter, value) in attribute_arguments(
            attribute,
            &["path", "name", "requirements", "options", "defaults"],
        ) {
            match parameter {
                "path" => match resolved_string_literal(value, content) {
                    Some((path, start, _)) => details.path = Some((path.to_string(), start as u32)),
                    None => details.path_unreadable = true,
                },
                "options" => {
                    details.parameters = array_entry(value, "parameters", content)
                        .map(|parameters| declared_parameters(parameters, content));
                }
                "defaults" => {
                    let default = |key: &str| array_entry(value, key, content).and_then(literal);
                    details.title_callback = default("_title_callback")
                        .and_then(|callback| crate::drupal::routing::handler_of(&callback));
                    details.entity_view = default("_entity_view")
                        .and_then(|view| crate::drupal::routing::entity_type_of(&view));
                    details.entity_form = default("_entity_form")
                        .and_then(|form| crate::drupal::routing::entity_type_of(&form));
                    details.defaults = default_values(value, content);
                }
                "requirements" => {
                    push_requirements_permissions(value, content, &mut details.permissions);
                }
                _ => {}
            }
        }
        found.push(details);
    }
    found
}

/// The route an attribute declares under `name`, dispatching to
/// `controller`, after the class route `globals`: the spans of the
/// parameters its path declares, pushed to `spans`, and the route, unless
/// its path is not all written out.
///
/// Drupal puts the class route's path in front of the route's, and lets the
/// route's own defaults and options replace the class route's key by key.
fn attribute_route(
    name: &str,
    globals: Option<&RouteDetails>,
    own: &RouteDetails,
    controller: String,
    spans: &mut Vec<SymbolSpan>,
) -> Option<DrupalRouteSite> {
    let mut path = String::new();
    let mut readable = true;
    for details in globals.into_iter().chain([own]) {
        readable &= !details.path_unreadable;
        let Some((written, start)) = &details.path else {
            continue;
        };
        for (from, to) in crate::drupal::routing::path_parameters(written) {
            spans.push(SymbolSpan {
                start: start + from as u32,
                end: start + to as u32,
                kind: SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalRouteParameter,
                    key: crate::drupal::routing::route_parameter_key(name, &written[from..to]),
                    is_write: true,
                    is_optional: false,
                },
            });
        }
        path.push_str(written);
    }
    if !readable {
        return None;
    }
    let entity_view = own
        .entity_view
        .clone()
        .or_else(|| globals.and_then(|globals| globals.entity_view.clone()));
    let entity_form = own
        .entity_form
        .clone()
        .or_else(|| globals.and_then(|globals| globals.entity_form.clone()));
    let title_callback = own
        .title_callback
        .clone()
        .or_else(|| globals.and_then(|globals| globals.title_callback.clone()));
    let mut defaults = own.defaults.clone();
    for (name, is_null) in globals.iter().flat_map(|globals| &globals.defaults) {
        if !defaults.iter().any(|(own, _)| own == name) {
            defaults.push((name.clone(), *is_null));
        }
    }
    Some(DrupalRouteSite {
        name: name.to_string(),
        path,
        controller: Some(controller),
        title_callback,
        parameters: own
            .parameters
            .clone()
            .or_else(|| globals.and_then(|globals| globals.parameters.clone()))
            .unwrap_or_default(),
        defaults,
        entity_type: entity_view.or(entity_form),
    })
}

/// The value an array literal gives the string key `key`.
fn array_entry<'a, 'arena>(
    array: &'a Expression<'arena>,
    key: &str,
    content: &str,
) -> Option<&'a Expression<'arena>> {
    crate::parser::array_literal_elements(array)?
        .iter()
        .find_map(|element| match element {
            ArrayElement::KeyValue(entry) => resolved_string_literal(entry.key, content)
                .filter(|(written, _, _)| *written == key)
                .map(|_| entry.value),
            _ => None,
        })
}

/// Each parameter an `options` array's `parameters` declares, with the
/// `type` it gives and whether it names its own converter.
fn declared_parameters(parameters: &Expression<'_>, content: &str) -> Vec<DeclaredParameter> {
    let Some(entries) = crate::parser::array_literal_elements(parameters) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|element| {
            let ArrayElement::KeyValue(entry) = element else {
                return None;
            };
            let (name, _, _) = resolved_string_literal(entry.key, content)?;
            let ty = array_entry(entry.value, "type", content)
                .and_then(|ty| resolved_string_literal(ty, content))
                .map(|(ty, _, _)| ty.to_string());
            let converted_otherwise = ["converter", "tempstore"]
                .iter()
                .any(|key| array_entry(entry.value, key, content).is_some());
            Some(DeclaredParameter {
                name: name.to_string(),
                ty,
                converted_otherwise,
            })
        })
        .collect()
}

/// The keys of a `defaults` array that are not Drupal's own, with whether
/// each is `NULL`.
fn default_values(defaults: &Expression<'_>, content: &str) -> Vec<(String, bool)> {
    let Some(entries) = crate::parser::array_literal_elements(defaults) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|element| {
            let ArrayElement::KeyValue(entry) = element else {
                return None;
            };
            let (name, _, _) = resolved_string_literal(entry.key, content)?;
            (!name.starts_with('_')).then(|| {
                (
                    name.to_string(),
                    matches!(entry.value, Expression::Literal(Literal::Null(_))),
                )
            })
        })
        .collect()
}

/// Whether `fqn` is a class under an extension's (or core's) `Controller`
/// namespace: `Drupal\<extension>\Controller\…`.
fn is_controller_class(fqn: &str) -> bool {
    let mut segments = fqn.trim_start_matches('\\').split('\\');
    segments.next() == Some("Drupal")
        && segments.next().is_some()
        && segments.next() == Some("Controller")
        && segments.next().is_some()
}

/// What a route builder names the route it adds: a name written out, or
/// the shape of one built from parts.
enum BuiltName {
    Literal(String, u32, u32),
    Pattern(String),
}

/// Record the routes a route builder adds: a class or trait whose file
/// uses Symfony's routing component, as the classes a routing file's
/// `route_callbacks` name, route subscribers, entity route providers, and
/// the traits they share do, or core's entity routing, as an entity route
/// provider that extends core's does.  `$collection->add(NAME, $route)`,
/// `$routes[NAME] = new Route(…)` (or a variable holding one), and an
/// array literal's `NAME => new Route(…)` add the route `NAME`: written
/// out, it is declared; built from parts, its shape is kept as a pattern.
/// A shape that does not start with written-out text stands for any route
/// under the extension's own prefix, which is where Drupal's own builders
/// put such routes.
pub(in crate::symbol_map::extraction) fn emit_drupal_route_builder_spans(
    members: &Sequence<'_, ClassLikeMember<'_>>,
    name_offset: u32,
    ctx: &mut ExtractionCtx<'_>,
) {
    // An entity route provider that extends core's renames the routes it
    // inherits with `$routes->add('node.add', $route)`, and needs no
    // Symfony import to do it.
    if !ctx.drupal
        || !(ctx.content.contains("Symfony\\Component\\Routing\\")
            || ctx.content.contains("Drupal\\Core\\Entity\\Routing\\"))
    {
        return;
    }
    let mut found = BuilderSites::default();
    for member in members.iter() {
        if let ClassLikeMember::Method(method) = member {
            mago_syntax::walker::Walker::walk_method(&BuilderWalker, method, &mut found);
        }
    }
    let is_route_class = |class: &Expression<'_>| {
        let Expression::Identifier(identifier) = class else {
            return false;
        };
        ctx.resolved_name_at(identifier.span().start.offset)
            .unwrap_or_else(|| bytes_to_str(identifier.value()))
            .trim_start_matches('\\')
            .eq_ignore_ascii_case("Symfony\\Component\\Routing\\Route")
    };
    let route_variables: Vec<&[u8]> = found
        .route_variables
        .iter()
        .filter(|(_, class)| is_route_class(class))
        .map(|(name, _)| *name)
        .collect();
    let holds_route = |value: &Expression<'_>| match value {
        Expression::Instantiation(instantiation) => is_route_class(instantiation.class),
        Expression::Variable(Variable::Direct(variable)) => {
            route_variables.contains(&variable.name)
        }
        _ => false,
    };
    // A route collection's `add()` takes the route second: a `new Route`,
    // or a variable or a call holding one (`static::getEntryPointRoute()`),
    // a parameter included, which the file alone cannot type.  Any other
    // `add()` adds something else.
    let mut names: Vec<&Expression<'_>> = found
        .added
        .into_iter()
        .filter(|(_, route)| {
            holds_route(route)
                || matches!(
                    route,
                    Expression::Variable(Variable::Direct(_)) | Expression::Call(_)
                )
        })
        .map(|(name, _)| name)
        .collect();
    names.extend(
        found
            .keyed
            .into_iter()
            .filter(|(_, value)| holds_route(value))
            .map(|(name, _)| name),
    );
    let extension = ctx.resolved_name_at(name_offset).and_then(extension_of);
    for name in names {
        match built_name(name, ctx.content) {
            BuiltName::Literal(key, start, end) => ctx.spans.push(SymbolSpan {
                start,
                end,
                kind: SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalRoute,
                    key,
                    is_write: true,
                    is_optional: false,
                },
            }),
            BuiltName::Pattern(pattern) => {
                let pattern = if pattern.starts_with('*') {
                    let Some(extension) = extension else {
                        continue;
                    };
                    format!("{extension}.*")
                } else {
                    pattern
                };
                if !ctx.drupal_route_patterns.contains(&pattern) {
                    ctx.drupal_route_patterns.push(pattern);
                }
            }
        }
    }
}

/// The calls, assignments, and array entries of a route builder's methods
/// that may add a route.
#[derive(Default)]
struct BuilderSites<'ast, 'arena> {
    /// The first two arguments of each `->add()` with two or more.
    added: Vec<(&'ast Expression<'arena>, &'ast Expression<'arena>)>,
    /// The key and the value of each `$routes[KEY] = VALUE` and each
    /// `KEY => VALUE` array entry.
    keyed: Vec<(&'ast Expression<'arena>, &'ast Expression<'arena>)>,
    /// Each variable assigned a `new Class(…)`, with the class.
    route_variables: Vec<(&'arena [u8], &'ast Expression<'arena>)>,
}

struct BuilderWalker;

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, BuilderSites<'ast, 'arena>>
    for BuilderWalker
{
    fn walk_in_method_call(
        &self,
        call: &'ast MethodCall<'arena>,
        found: &mut BuilderSites<'ast, 'arena>,
    ) {
        let ClassLikeMemberSelector::Identifier(name) = &call.method else {
            return;
        };
        let arguments = &call.argument_list.arguments;
        if name.value.eq_ignore_ascii_case(b"add")
            && let (Some(Argument::Positional(first)), Some(Argument::Positional(second))) =
                (arguments.first(), arguments.get(1))
        {
            found.added.push((first.value, second.value));
        }
    }

    fn walk_in_assignment(
        &self,
        assignment: &'ast Assignment<'arena>,
        found: &mut BuilderSites<'ast, 'arena>,
    ) {
        match (assignment.lhs, assignment.rhs) {
            (Expression::ArrayAccess(access), value) => found.keyed.push((access.index, value)),
            (
                Expression::Variable(Variable::Direct(variable)),
                Expression::Instantiation(instantiation),
            ) => found
                .route_variables
                .push((variable.name, instantiation.class)),
            _ => {}
        }
    }

    fn walk_in_array(&self, array: &'ast Array<'arena>, found: &mut BuilderSites<'ast, 'arena>) {
        push_keyed_entries(array.elements.iter(), found);
    }

    fn walk_in_legacy_array(
        &self,
        array: &'ast LegacyArray<'arena>,
        found: &mut BuilderSites<'ast, 'arena>,
    ) {
        push_keyed_entries(array.elements.iter(), found);
    }
}

/// Record each `KEY => VALUE` entry of an array literal.
fn push_keyed_entries<'ast, 'arena>(
    elements: impl Iterator<Item = &'ast ArrayElement<'arena>>,
    found: &mut BuilderSites<'ast, 'arena>,
) where
    'arena: 'ast,
{
    for element in elements {
        if let ArrayElement::KeyValue(entry) = element {
            found.keyed.push((entry.key, entry.value));
        }
    }
}

/// The name a route builder gives a route: written out, or the shape of
/// the parts it is built from.
fn built_name(expression: &Expression<'_>, content: &str) -> BuiltName {
    if let Some((key, start, end)) = resolved_string_literal(expression, content) {
        return BuiltName::Literal(key.to_string(), start as u32, end as u32);
    }
    BuiltName::Pattern(name_shape(expression))
}

/// The shape of the name `expression` builds: its text where it is a
/// string, `*` where it is anything else (`search.view_*`).
pub(crate) fn name_shape(expression: &Expression<'_>) -> String {
    let mut pattern = String::new();
    push_name_parts(expression, &mut pattern);
    pattern
}

/// Append the shape of `expression` to `pattern`: its text where it is a
/// string, `*` where it is anything else.
fn push_name_parts(expression: &Expression<'_>, pattern: &mut String) {
    let unknown = |pattern: &mut String| {
        if !pattern.ends_with('*') {
            pattern.push('*');
        }
    };
    match expression {
        Expression::Literal(Literal::String(string)) => match string.value {
            Some(value) => pattern.push_str(&String::from_utf8_lossy(value)),
            None => unknown(pattern),
        },
        Expression::CompositeString(CompositeString::Interpolated(string)) => {
            for part in string.parts.iter() {
                match part {
                    StringPart::Literal(literal) => match literal.value {
                        Some(value) => pattern.push_str(&String::from_utf8_lossy(value)),
                        None => pattern.push_str(&String::from_utf8_lossy(literal.raw)),
                    },
                    _ => unknown(pattern),
                }
            }
        }
        Expression::Binary(binary) if binary.operator.is_concatenation() => {
            push_name_parts(binary.lhs, pattern);
            push_name_parts(binary.rhs, pattern);
        }
        Expression::Parenthesized(inner) => push_name_parts(inner.expression, pattern),
        _ => unknown(pattern),
    }
}

/// The extension a class of `Drupal\<extension>\…` belongs to, other than
/// core's own `Drupal\Core` and `Drupal\Component`.
fn extension_of(fqn: &str) -> Option<&str> {
    let mut segments = fqn.trim_start_matches('\\').split('\\');
    let extension = (segments.next() == Some("Drupal")).then(|| segments.next())??;
    (!matches!(extension, "Core" | "Component") && segments.next().is_some()).then_some(extension)
}

/// Record the theme hooks a render array literal names: its `#theme`, a
/// string or a list of names, and its `#theme_wrappers`, a list of names or
/// of names keying their own variables.  When `#theme` is one name, every
/// other `#` key may pass one of the hook's variables, which is decided
/// when it is looked up, and the array is a render of the hook's templates
/// (a [`TwigContextSite`](crate::symbol_map::TwigContextSite) keyed by the
/// `#theme` value's span) passing those keys without the `#`.
pub(in crate::symbol_map::extraction) fn emit_drupal_render_array_spans(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal_theme_keys {
        return;
    }
    let mut hook = None;
    let mut keys = Vec::new();
    for element in elements.iter() {
        let ArrayElement::KeyValue(entry) = element else {
            continue;
        };
        let Some((key, start, end)) = resolved_string_literal(entry.key, ctx.content) else {
            continue;
        };
        match key {
            "#theme" => {
                hook = resolved_string_literal(entry.value, ctx.content);
                push_theme_hook_spans(entry.value, false, ctx);
            }
            "#theme_wrappers" => push_theme_hook_spans(entry.value, true, ctx),
            _ if key.len() > 1 && key.starts_with('#') && !is_render_api_key(key) => {
                let span = entry.value.span();
                keys.push((key, start, end, (span.start.offset, span.end.offset)));
            }
            _ => {}
        }
    }
    let Some((hook, hook_start, _)) = hook else {
        return;
    };
    let vars: Vec<crate::symbol_map::TwigContextVar> = keys
        .iter()
        .filter(|(key, ..)| crate::twig::variables::is_twig_variable_name(&key[1..]))
        .map(
            |(key, start, end, (value_start, value_end))| crate::symbol_map::TwigContextVar {
                name: crate::atom::atom(&key[1..]),
                start: *start as u32 + 1,
                end: *end as u32,
                value: crate::symbol_map::TwigContextValue::Expression {
                    start: *value_start,
                    end: *value_end,
                },
            },
        )
        .collect();
    if !vars.is_empty() {
        ctx.twig_contexts.push(crate::symbol_map::TwigContextSite {
            template_start: hook_start as u32,
            vars,
            shape: None,
        });
    }
    for (key, start, end, _) in keys {
        ctx.spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::DrupalThemeHookVariable,
                key: crate::drupal::theme_hooks::theme_variable_key(hook, &key[1..]),
                is_write: false,
                is_optional: false,
            },
        });
    }
}

/// The render array keys Drupal's renderer reads itself, which no theme
/// hook declares as a variable, sorted.
const RENDER_API_KEYS: [&str; 23] = [
    "#access",
    "#access_callback",
    "#after_build",
    "#allowed_tags",
    "#attached",
    "#cache",
    "#cache_properties",
    "#create_placeholder",
    "#defaults_loaded",
    "#lazy_builder",
    "#markup",
    "#plain_text",
    "#post_render",
    "#pre_render",
    "#prefix",
    "#printed",
    "#process",
    "#render_children",
    "#sorted",
    "#states",
    "#suffix",
    "#type",
    "#weight",
];

fn is_render_api_key(key: &str) -> bool {
    RENDER_API_KEYS.binary_search(&key).is_ok()
}

/// Record the theme hooks `$build['#theme'] = …` and
/// `$build['#theme_wrappers'] = …` name.
pub(in crate::symbol_map::extraction) fn emit_drupal_render_array_assignment(
    assignment: &Assignment<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal_theme_keys || !assignment.operator.is_assign() {
        return;
    }
    let Expression::ArrayAccess(access) = assignment.lhs else {
        return;
    };
    match resolved_string_literal(access.index, ctx.content) {
        Some(("#theme", _, _)) => push_theme_hook_spans(assignment.rhs, false, ctx),
        Some(("#theme_wrappers", _, _)) => push_theme_hook_spans(assignment.rhs, true, ctx),
        _ => {}
    }
}

/// Record the theme hook names `value` holds: a string, or the strings of a
/// list, and for `#theme_wrappers` the keys of a keyed list.
fn push_theme_hook_spans(value: &Expression<'_>, wrappers: bool, ctx: &mut ExtractionCtx<'_>) {
    let push = |name: &Expression<'_>, ctx: &mut ExtractionCtx<'_>| {
        if let Some((key, start, end)) = resolved_string_literal(name, ctx.content) {
            ctx.spans.push(SymbolSpan {
                start: start as u32,
                end: end as u32,
                kind: SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalThemeHook,
                    key: key.to_string(),
                    is_write: false,
                    is_optional: false,
                },
            });
        }
    };
    let Some(elements) = crate::parser::array_literal_elements(value) else {
        push(value, ctx);
        return;
    };
    for element in elements.iter() {
        match element {
            ArrayElement::Value(item) => push(item.value, ctx),
            ArrayElement::KeyValue(entry) if wrappers => push(entry.key, ctx),
            _ => {}
        }
    }
}

const HOOK_ATTRIBUTE: &str = "Drupal\\Core\\Hook\\Attribute\\Hook";

/// Record the hook a `hook_<name>()` function of an `*.api.php` file
/// documents.  The span covers the name after `hook_`, so the function's
/// own span still answers on `hook_`.
pub(in crate::symbol_map::extraction) fn emit_drupal_hook_doc_span(
    function: &Function<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal || ctx.drupal_file != crate::drupal::hooks::DrupalFile::Api {
        return;
    }
    let name = bytes_to_str(function.name.value);
    let Some(hook) = name.strip_prefix("hook_").filter(|hook| !hook.is_empty()) else {
        return;
    };
    let start = function.name.span.start.offset + 5;
    ctx.spans.push(SymbolSpan {
        start,
        end: start + hook.len() as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::DrupalHook,
            key: hook.to_string(),
            is_write: true,
            is_optional: false,
        },
    });
}

/// The attributes that keep Drupal's hook collector from reading a
/// procedural function: it implements its hook through a hook class.
const LEGACY_HOOK_ATTRIBUTES: [&str; 3] = [
    "Drupal\\Core\\Hook\\Attribute\\LegacyHook",
    "Drupal\\Core\\Hook\\Attribute\\LegacyModuleImplementsAlter",
    "Drupal\\Core\\Hook\\Attribute\\LegacyRequirementsHook",
];
const SCAN_STOP_ATTRIBUTE: &str = "Drupal\\Core\\Hook\\Attribute\\ProceduralHookScanStop";

/// Record the hook a global function of an extension's procedural file
/// implements, named `<extension>_<hook>` the way Drupal's hook collector
/// reads it: the file's own extension first, then every extension of its
/// group, longest first.  The span covers the hook after the extension, so
/// the function's own span still answers on the extension part.
pub(in crate::symbol_map::extraction) fn emit_drupal_procedural_hook_span(
    function: &Function<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let crate::drupal::hooks::DrupalFile::Procedural(procedural) = &ctx.drupal_file else {
        return;
    };
    if !ctx.drupal || ctx.drupal_hook_scan_stopped {
        return;
    }
    if attributes_named(&function.attribute_lists, SCAN_STOP_ATTRIBUTE, ctx)
        .next()
        .is_some()
    {
        ctx.drupal_hook_scan_stopped = true;
        return;
    }
    let is_legacy = LEGACY_HOOK_ATTRIBUTES.iter().any(|legacy| {
        attributes_named(&function.attribute_lists, legacy, ctx)
            .next()
            .is_some()
    });
    let namespaced = ctx
        .resolved_name_at(function.name.span.start.offset)
        .is_some_and(|fqn| fqn.trim_start_matches('\\').contains('\\'));
    if is_legacy || namespaced {
        return;
    }
    let name = bytes_to_str(function.name.value);
    let hook_of = |extension: &str| {
        name.strip_prefix(extension)?
            .strip_prefix('_')
            .filter(|hook| !hook.is_empty())
    };
    // Only the file's own extension names an update function or a post
    // update, which Drupal reads by the extension that owns the file.
    let found = if procedural.post_update {
        hook_of(&procedural.extension)
            .filter(|hook| hook.starts_with("post_update_"))
            .map(|hook| (procedural.extension.len(), hook))
    } else {
        hook_of(&procedural.extension)
            .map(|hook| (procedural.extension.len(), hook))
            .or_else(|| {
                procedural.group.iter().find_map(|extension| {
                    hook_of(extension)
                        .filter(|hook| !starts_with_update_number(hook))
                        .map(|hook| (extension.len(), hook))
                })
            })
    };
    let Some((prefix, hook)) = found else {
        return;
    };
    let start = function.name.span.start.offset + prefix as u32 + 1;
    let collected = !procedural.post_update
        && !crate::drupal::hooks::NOT_COLLECTED.contains(&hook)
        && !starts_with_update_number(hook)
        && !ends_with_update_number(name);
    let role = crate::symbol_map::HookRole::Implementation {
        extension: crate::atom::atom(&name[..prefix]),
        is_theme: Some(procedural.is_theme),
        collected,
    };
    let key = hook.to_string();
    ctx.drupal_hooks
        .push(crate::symbol_map::HookSite { start, role });
    ctx.spans.push(SymbolSpan {
        start,
        end: start + key.len() as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::DrupalHook,
            key,
            is_write: false,
            is_optional: false,
        },
    });
}

/// `update_` followed by digits only, at the end: an update function.
fn ends_with_update_number(name: &str) -> bool {
    let digits = name.trim_end_matches(|c: char| c.is_ascii_digit());
    digits.len() < name.len() && digits.ends_with("update_")
}

/// `update_` followed by a digit: an update function, which only the
/// extension that owns the file names.
fn starts_with_update_number(hook: &str) -> bool {
    hook.strip_prefix("update_")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// Record the hooks the `#[Hook]` attributes of a hook class implement:
/// one on a method implements the hook for that method, one on the class
/// for `__invoke()` or the `method:` it names.  Drupal discovers hook
/// classes under an extension's `src/Hook`, whose namespace is
/// `Drupal\<extension>\Hook`; a `module:` argument implements the hook for
/// that extension instead.
pub(in crate::symbol_map::extraction) fn emit_drupal_hook_attribute_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal || ctx.drupal_file == crate::drupal::hooks::DrupalFile::Test {
        return;
    }
    let Some(extension) = ctx
        .resolved_name_at(class.name.span.start.offset)
        .and_then(crate::drupal::hooks::hook_class_extension)
        .map(crate::atom::atom)
    else {
        return;
    };
    let methods = class.members.iter().filter_map(|member| match member {
        ClassLikeMember::Method(method) => Some(&method.attribute_lists),
        _ => None,
    });
    let mut found = Vec::new();
    for attribute_lists in std::iter::once(&class.attribute_lists).chain(methods) {
        for attribute in attributes_named(attribute_lists, HOOK_ATTRIBUTE, ctx) {
            let mut hook = None;
            let mut module = None;
            for (parameter, value) in attribute_arguments(attribute, &["hook", "method", "module"])
            {
                match parameter {
                    "hook" => hook = resolved_string_literal(value, ctx.content),
                    "module" => module = resolved_string_literal(value, ctx.content),
                    _ => {}
                }
            }
            if let Some((key, start, end)) = hook.filter(|(key, _, _)| !key.is_empty()) {
                let extension = module
                    .map(|(module, _, _)| crate::atom::atom(module))
                    .unwrap_or(extension);
                found.push((key.to_string(), start as u32, end as u32, extension));
            }
        }
    }
    for (key, start, end, extension) in found {
        ctx.drupal_hooks.push(crate::symbol_map::HookSite {
            start,
            role: crate::symbol_map::HookRole::Implementation {
                extension,
                is_theme: None,
                collected: true,
            },
        });
        ctx.spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::DrupalHook,
                key,
                is_write: false,
                is_optional: false,
            },
        });
    }
}

/// A method invoking hooks: where its hook name is, whether that name is
/// an alter type (`form` for `form_alter`), and where the extension it is
/// limited to is.
struct HookInvoker {
    method: &'static str,
    hook: usize,
    alter: bool,
    extension: Option<usize>,
}

const fn invoker(
    method: &'static str,
    hook: usize,
    alter: bool,
    extension: Option<usize>,
) -> HookInvoker {
    HookInvoker {
        method,
        hook,
        alter,
        extension,
    }
}

/// The module handler's invoking methods, which reach modules and
/// profiles.
const MODULE_INVOKERS: [HookInvoker; 7] = [
    invoker("invoke", 1, false, Some(0)),
    invoker("invokeAll", 0, false, None),
    invoker("invokeAllWith", 0, false, None),
    invoker("invokeDeprecated", 2, false, Some(1)),
    invoker("invokeAllDeprecated", 1, false, None),
    invoker("alter", 0, true, None),
    invoker("alterDeprecated", 1, true, None),
];

/// The theme manager's invoking methods, which reach themes.
const THEME_INVOKERS: [HookInvoker; 4] = [
    invoker("invoke", 1, false, Some(0)),
    invoker("invokeAllWith", 0, false, None),
    invoker("alter", 0, true, None),
    invoker("alterForTheme", 1, true, None),
];

/// What a call of `method` invokes in its positional argument `index`:
/// the receiver it needs, and whether the argument is an alter type.
pub(crate) fn hook_invocation(method: &str, index: usize) -> Option<(ReceiverClass, bool)> {
    let find = |invokers: &'static [HookInvoker]| {
        invokers
            .iter()
            .find(|invoker| invoker.method.eq_ignore_ascii_case(method) && invoker.hook == index)
    };
    match (find(&MODULE_INVOKERS), find(&THEME_INVOKERS)) {
        (Some(invoker), Some(_)) => Some((ReceiverClass::Hooks, invoker.alter)),
        (Some(invoker), None) => Some((ReceiverClass::ModuleHooks, invoker.alter)),
        (None, Some(invoker)) => Some((ReceiverClass::ThemeHooks, invoker.alter)),
        (None, None) => None,
    }
}

/// Record the hooks a call may invoke, for the type engine to confirm
/// against its receiver: a module handler's or a theme manager's.  An
/// alter type names the hook `<type>_alter`, and a list of them names one
/// hook each.
pub(in crate::symbol_map::extraction) fn record_drupal_hook_invocations(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal
        || ctx.drupal_file == crate::drupal::hooks::DrupalFile::Test
        || !matches!(
            member_name.as_bytes().first(),
            Some(b'i' | b'a' | b'I' | b'A')
        )
    {
        return;
    }
    let positional: Vec<&Expression<'_>> = argument_list
        .arguments
        .iter()
        .map_while(|argument| match argument {
            Argument::Positional(positional) => Some(positional.value),
            Argument::Named(_) => None,
        })
        .collect();
    let find = |invokers: &'static [HookInvoker]| {
        invokers
            .iter()
            .find(|invoker| invoker.method.eq_ignore_ascii_case(member_name))
    };
    // The methods both have take their arguments in the same places; only
    // the type engine tells which one a call is on.
    let (invoker, receiver) = match (find(&MODULE_INVOKERS), find(&THEME_INVOKERS)) {
        (Some(invoker), Some(_)) => (invoker, ReceiverClass::Hooks),
        (Some(invoker), None) => (invoker, ReceiverClass::ModuleHooks),
        (None, Some(invoker)) => (invoker, ReceiverClass::ThemeHooks),
        (None, None) => return,
    };
    let Some(hook) = positional.get(invoker.hook) else {
        return;
    };
    let extension = invoker
        .extension
        .and_then(|at| positional.get(at))
        .and_then(|value| resolved_string_literal(value, ctx.content))
        .map(|(extension, _, _)| crate::atom::atom(extension));
    let names: Vec<(String, usize, usize)> = match hook {
        Expression::Array(array) if invoker.alter => array
            .elements
            .iter()
            .filter_map(|element| match element {
                ArrayElement::Value(value) => resolved_string_literal(value.value, ctx.content)
                    .map(|(name, start, end)| (name.to_string(), start, end)),
                _ => None,
            })
            .collect(),
        _ => resolved_string_literal(hook, ctx.content)
            .map(|(name, start, end)| (name.to_string(), start, end))
            .into_iter()
            .collect(),
    };
    for (name, start, end) in names.into_iter().filter(|(name, _, _)| !name.is_empty()) {
        let key = if invoker.alter {
            format!("{name}_alter")
        } else {
            name
        };
        ctx.typed_receiver_sites.push(TypedReceiverSite {
            start: start as u32,
            end: end as u32,
            key,
            is_write: true,
            is_optional: false,
            guards: 0,
            receiver,
        });
        ctx.drupal_hooks.push(crate::symbol_map::HookSite {
            start: start as u32,
            role: crate::symbol_map::HookRole::Invocation { extension },
        });
    }
}

/// Record the invoking calls of the core methods that fire hooks under
/// computed names, with the shape of the name each builds (see
/// [`crate::drupal::computed_hooks`]).  A hook name held in a variable is
/// all `*`; an alter call adds `_alter`, and a list of alter types gives
/// one shape each.
pub(in crate::symbol_map::extraction) fn emit_drupal_computed_hook_sites(
    members: &Sequence<'_, ClassLikeMember<'_>>,
    name_offset: u32,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal || ctx.drupal_file == crate::drupal::hooks::DrupalFile::Test {
        return;
    }
    let Some(class) = ctx
        .resolved_name_at(name_offset)
        .filter(|class| crate::drupal::computed_hooks::is_computing_class(class))
        .map(str::to_string)
    else {
        return;
    };
    for member in members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let method_name = bytes_to_str(method.name.value);
        if !crate::drupal::computed_hooks::is_computing_method(&class, method_name) {
            continue;
        }
        let mut calls = Vec::new();
        mago_syntax::walker::Walker::walk_method(&InvokingCallWalker, method, &mut calls);
        for (receiver, callee, arguments) in calls {
            let positional: Vec<&Expression<'_>> = arguments
                .arguments
                .iter()
                .map_while(|argument| match argument {
                    Argument::Positional(positional) => Some(positional.value),
                    Argument::Named(_) => None,
                })
                .collect();
            let invoker = MODULE_INVOKERS
                .iter()
                .chain(THEME_INVOKERS.iter())
                .find(|invoker| invoker.method.eq_ignore_ascii_case(callee));
            let Some(invoker) = invoker else {
                continue;
            };
            let Some(hook) = positional.get(invoker.hook) else {
                continue;
            };
            // The receiver's spelling says which group a call reaches: the
            // theme manager calls its own `alterForTheme()`, and the module
            // handler and the theme manager are injected under their names.
            let receiver_text = expr_to_subject_text(receiver);
            let themes = if callee.eq_ignore_ascii_case("alterForTheme")
                || receiver_text.contains("themeManager")
                || (receiver_text == "$this"
                    && class
                        .trim_start_matches('\\')
                        .ends_with("Theme\\ThemeManager"))
            {
                Some(true)
            } else if receiver_text.contains("moduleHandler") {
                Some(false)
            } else {
                None
            };
            let parts: Vec<&Expression<'_>> = match hook {
                Expression::Array(array) => array
                    .elements
                    .iter()
                    .filter_map(|element| match element {
                        ArrayElement::Value(value) => Some(value.value),
                        _ => None,
                    })
                    .collect(),
                _ => vec![*hook],
            };
            for part in parts {
                let mut shape = String::new();
                push_name_parts(part, &mut shape);
                if invoker.alter {
                    shape.push_str("_alter");
                }
                let span = part.span();
                ctx.drupal_computed_hooks
                    .push(crate::symbol_map::ComputedHookSite {
                        method: crate::atom::atom(method_name),
                        start: span.start.offset,
                        end: span.end.offset,
                        shape,
                        alter: invoker.alter,
                        themes,
                    });
            }
        }
    }
}

/// The method calls of a method body.
struct InvokingCallWalker;

/// A method call: its receiver, its name, and its arguments.
type FoundCall<'ast, 'arena> = (
    &'ast Expression<'arena>,
    &'arena str,
    &'ast ArgumentList<'arena>,
);

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Vec<FoundCall<'ast, 'arena>>>
    for InvokingCallWalker
{
    fn walk_in_method_call(
        &self,
        call: &'ast MethodCall<'arena>,
        calls: &mut Vec<FoundCall<'ast, 'arena>>,
    ) {
        if let ClassLikeMemberSelector::Identifier(name) = &call.method {
            calls.push((call.object, bytes_to_str(name.value), &call.argument_list));
        }
    }
}

/// Record the route name `new Url('…')` reads.
pub(in crate::symbol_map::extraction) fn emit_drupal_url_spans(
    class: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let Expression::Identifier(identifier) = class else {
        return;
    };
    let resolved = ctx
        .resolved_name_at(identifier.span().start.offset)
        .unwrap_or_else(|| bytes_to_str(identifier.value()));
    if crate::drupal::is_url_class(resolved) {
        emit_drupal_route_span(0, argument_list, ctx);
    }
}

/// Record the permissions `new Route()` requires: the `_permission` of its
/// requirements array, the third argument.
pub(in crate::symbol_map::extraction) fn emit_drupal_route_permission_spans(
    class: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let Expression::Identifier(identifier) = class else {
        return;
    };
    let resolved = ctx
        .resolved_name_at(identifier.span().start.offset)
        .unwrap_or_else(|| bytes_to_str(identifier.value()));
    if resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(SYMFONY_ROUTE)
        && let Some(requirements) = argument_at(argument_list, 2, b"requirements")
    {
        push_requirements_permissions(requirements, ctx.content, &mut ctx.spans);
    }
}

const SYMFONY_ROUTE: &str = "Symfony\\Component\\Routing\\Route";

/// Record the permissions a method call reads: the `_permission` a route's
/// `setRequirement()` or `setRequirements()` sets.  The calls that check a
/// permission on an account or a role are recorded with the receiver they
/// need (see `symfony_receiver_method`).
pub(in crate::symbol_map::extraction) fn emit_drupal_requirement_permission_spans(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let is = |name: &str| member_name.eq_ignore_ascii_case(name);
    if !ctx.drupal || !(is("setRequirement") || is("setRequirements")) {
        return;
    }
    if is("setRequirement") {
        let is_permission = argument_at(argument_list, 0, b"key")
            .and_then(|key| resolved_string_literal(key, ctx.content))
            .is_some_and(|(key, _, _)| key == "_permission");
        if is_permission && let Some(value) = argument_at(argument_list, 1, b"regex") {
            push_requirement_permissions(value, ctx.content, &mut ctx.spans);
        }
    } else if let Some(requirements) = argument_at(argument_list, 0, b"requirements") {
        push_requirements_permissions(requirements, ctx.content, &mut ctx.spans);
    }
}

/// Record the permissions a static call reads: the second argument of
/// `AccessResult::allowedIfHasPermission()`, and each one in the array
/// `allowedIfHasPermissions()` takes second.
pub(in crate::symbol_map::extraction) fn emit_drupal_static_permission_spans(
    class: &str,
    method: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let Some(list) = crate::drupal::permissions::static_permission_call(class, method) else {
        return;
    };
    let Some(argument) = argument_at(argument_list, 1, b"permission")
        .or_else(|| argument_at(argument_list, 1, b"permissions"))
    else {
        return;
    };
    if list {
        push_permission_list(argument, ctx.content, &mut ctx.spans);
    } else {
        push_laravel_string_span(
            StringKeyKind::DrupalPermission,
            false,
            false,
            argument,
            ctx.content,
            &mut ctx.spans,
        );
    }
}

/// Record the permissions `user_role_grant_permissions()` and
/// `user_role_revoke_permissions()`, written as `written`, take in the
/// array they take second.
pub(in crate::symbol_map::extraction) fn emit_drupal_function_permission_spans(
    written: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal || !crate::drupal::permissions::is_role_permissions_function(written) {
        return;
    }
    if let Some(permissions) = argument_at(argument_list, 1, b"permissions") {
        push_permission_list(permissions, ctx.content, &mut ctx.spans);
    }
}

/// Push a span for each permission an array literal lists.
fn push_permission_list(list: &Expression<'_>, content: &str, spans: &mut Vec<SymbolSpan>) {
    let Some(elements) = crate::parser::array_literal_elements(list) else {
        return;
    };
    for element in elements.iter() {
        if let ArrayElement::Value(value) = element {
            push_laravel_string_span(
                StringKeyKind::DrupalPermission,
                false,
                false,
                value.value,
                content,
                spans,
            );
        }
    }
}

/// Push the spans of the permissions a requirements array's `_permission`
/// names.
fn push_requirements_permissions(
    requirements: &Expression<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    if let Some(value) = array_entry(requirements, "_permission", content) {
        push_requirement_permissions(value, content, spans);
    }
}

/// Push the spans of the permissions a `_permission` requirement written
/// as `value` names, each part of an `a+b` or `a,b` on its own.
fn push_requirement_permissions(
    value: &Expression<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    // A string with escapes reads differently from its text, whose
    // offsets the parts would not match.
    if let Some((text, start, end)) = resolved_string_literal(value, content)
        && text.len() == end - start
    {
        spans.extend(crate::drupal::permissions::requirement_spans(text, start));
    }
}

/// Record the configuration name `new ConfigTarget()` reads first, and the
/// keys of that object its property path, a key or a list of them, names
/// second.
pub(in crate::symbol_map::extraction) fn emit_drupal_config_target_spans(
    class: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let Expression::Identifier(identifier) = class else {
        return;
    };
    let resolved = ctx
        .resolved_name_at(identifier.span().start.offset)
        .unwrap_or_else(|| bytes_to_str(identifier.value()));
    if !crate::drupal::is_config_target_class(resolved) {
        return;
    }
    let Some(name) = argument_at(argument_list, 0, b"configName") else {
        return;
    };
    push_laravel_string_span(
        StringKeyKind::DrupalConfigName,
        false,
        false,
        name,
        ctx.content,
        &mut ctx.spans,
    );
    let Some((name, _, _)) = resolved_string_literal(name, ctx.content) else {
        return;
    };
    let Some(path) = argument_at(argument_list, 1, b"propertyPath") else {
        return;
    };
    let keys: Vec<&Expression<'_>> = match crate::parser::array_literal_elements(path) {
        Some(elements) => elements
            .iter()
            .filter_map(|element| match element {
                ArrayElement::Value(value) => Some(value.value),
                _ => None,
            })
            .collect(),
        None => vec![path],
    };
    for key in keys {
        if let Some((key, start, end)) =
            resolved_string_literal(key, ctx.content).filter(|(key, _, _)| !key.is_empty())
        {
            push_config_key_span(name, key, start, end, &mut ctx.spans);
        }
    }
}

/// Push the span of the key `key` of the configuration object `name`,
/// written from `start` to `end`.
fn push_config_key_span(
    name: &str,
    key: &str,
    start: usize,
    end: usize,
    spans: &mut Vec<SymbolSpan>,
) {
    spans.push(SymbolSpan {
        start: start as u32,
        end: end as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::DrupalConfigKey,
            key: crate::drupal::config_keys::config_key(name, key),
            is_write: false,
            is_optional: false,
        },
    });
}

/// Record the key an editable config object's `set()` or `clear()` reads,
/// for the type engine to confirm against its receiver, whose type carries
/// the object's name.  Its `get()` is recorded as a container lookup's,
/// which the config object is one of the targets of.
pub(in crate::symbol_map::extraction) fn record_drupal_config_key_sites(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    if !member_name.eq_ignore_ascii_case("set") && !member_name.eq_ignore_ascii_case("clear") {
        return;
    }
    let Some(Argument::Positional(key)) = argument_list.arguments.first() else {
        return;
    };
    if let Some((key, start, end)) =
        resolved_string_literal(key.value, ctx.content).filter(|(key, _, _)| !key.is_empty())
    {
        ctx.typed_receiver_sites.push(TypedReceiverSite {
            start: start as u32,
            end: end as u32,
            key: key.to_string(),
            is_write: false,
            is_optional: false,
            guards: 0,
            receiver: ReceiverClass::EditableConfigKeys,
        });
    }
}

/// Record the configuration name a form element's `#config_target` names
/// before its `:`, among the entries of an array literal.
pub(in crate::symbol_map::extraction) fn emit_drupal_config_target_entry_spans(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    for element in elements.iter() {
        let ArrayElement::KeyValue(entry) = element else {
            continue;
        };
        let is_target = resolved_string_literal(entry.key, ctx.content)
            .is_some_and(|(key, _, _)| key == "#config_target");
        if !is_target {
            continue;
        }
        if let Some((target, start, end)) = resolved_string_literal(entry.value, ctx.content)
            && target.len() == end - start
        {
            let (name, key) = target.split_once(':').unwrap_or((target, ""));
            if !name.is_empty() && !key.is_empty() {
                let key_start = start + name.len() + 1;
                push_config_key_span(name, key, key_start, end, &mut ctx.spans);
            }
            if !name.is_empty() {
                ctx.spans.push(SymbolSpan {
                    start: start as u32,
                    end: (start + name.len()) as u32,
                    kind: SymbolKind::StringKey {
                        kind: StringKeyKind::DrupalConfigName,
                        key: name.to_string(),
                        is_write: false,
                        is_optional: false,
                    },
                });
            }
        }
    }
}

/// Record each configuration name the array a config factory's
/// `loadMultiple()` takes lists, for the type engine to confirm against
/// its receiver.
pub(in crate::symbol_map::extraction) fn record_drupal_config_list_sites(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal || !member_name.eq_ignore_ascii_case("loadMultiple") {
        return;
    }
    let Some(Argument::Positional(names)) = argument_list.arguments.first() else {
        return;
    };
    let Some(elements) = crate::parser::array_literal_elements(names.value) else {
        return;
    };
    for element in elements.iter() {
        let ArrayElement::Value(value) = element else {
            continue;
        };
        if let Some((name, start, end)) = resolved_string_literal(value.value, ctx.content)
            .filter(|(name, _, _)| !name.is_empty())
        {
            ctx.typed_receiver_sites.push(TypedReceiverSite {
                start: start as u32,
                end: end as u32,
                key: name.to_string(),
                is_write: false,
                is_optional: false,
                guards: 0,
                receiver: ReceiverClass::ConfigFactory,
            });
        }
    }
}

/// Record the configuration names a config form's
/// `getEditableConfigNames()` returns in an array literal.
pub(in crate::symbol_map::extraction) fn emit_drupal_editable_config_spans(
    members: &Sequence<'_, ClassLikeMember<'_>>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    for member in members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        if !method
            .name
            .value
            .eq_ignore_ascii_case(b"getEditableConfigNames")
        {
            continue;
        }
        let mut returned = Vec::new();
        mago_syntax::walker::Walker::walk_method(&ReturnWalker, method, &mut returned);
        for value in returned {
            let Some(elements) = crate::parser::array_literal_elements(value) else {
                continue;
            };
            for element in elements.iter() {
                if let ArrayElement::Value(name) = element {
                    push_laravel_string_span(
                        StringKeyKind::DrupalConfigName,
                        false,
                        false,
                        name.value,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
            }
        }
    }
}

/// The values a method's `return` statements return.
struct ReturnWalker;

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Vec<&'ast Expression<'arena>>>
    for ReturnWalker
{
    fn walk_in_return(
        &self,
        statement: &'ast Return<'arena>,
        returned: &mut Vec<&'ast Expression<'arena>>,
    ) {
        if let Some(value) = statement.value {
            returned.push(value);
        }
    }
}

/// Record the route name a call reads in its positional argument `index`,
/// or in the `route_name:` argument that names it, and the parameters of
/// that route it passes in the argument after it, or in the
/// `route_parameters:` argument.
pub(in crate::symbol_map::extraction) fn emit_drupal_route_span(
    index: usize,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(route) = argument_at(argument_list, index, b"route_name") else {
        return;
    };
    push_laravel_string_span(
        StringKeyKind::DrupalRoute,
        false,
        false,
        route,
        ctx.content,
        &mut ctx.spans,
    );
    let Some((route, _, _)) = resolved_string_literal(route, ctx.content) else {
        return;
    };
    let Some(parameters) = argument_at(argument_list, index + 1, b"route_parameters") else {
        return;
    };
    for (key, start, end) in parameter_keys(parameters, ctx.content) {
        ctx.spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::DrupalRouteParameter,
                key: crate::drupal::routing::route_parameter_key(route, key),
                is_write: false,
                is_optional: false,
            },
        });
    }
}

/// Record the route parameters a call on a typed receiver passes after the
/// route name it reads (see [`crate::drupal::route_parameter_call`]), for
/// the type engine to confirm against its receiver like the name.
pub(in crate::symbol_map::extraction) fn record_drupal_route_parameter_sites(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let Some((receiver, parameter)) = crate::drupal::route_parameter_call(member_name) else {
        return;
    };
    // The route name is read only when it is written first, as the
    // receiver's own sites read it.
    let Some(Argument::Positional(route)) = argument_list.arguments.first() else {
        return;
    };
    let Some((route, _, _)) = resolved_string_literal(route.value, ctx.content) else {
        return;
    };
    let Some(parameters) = argument_at(argument_list, 1, parameter.as_bytes()) else {
        return;
    };
    for (key, start, end) in parameter_keys(parameters, ctx.content) {
        ctx.typed_receiver_sites.push(TypedReceiverSite {
            start,
            end,
            key: crate::drupal::routing::route_parameter_key(route, key),
            is_write: false,
            is_optional: false,
            guards: 0,
            receiver,
        });
    }
}

/// The argument a call passes its parameter `index` in: positional, or
/// named `name`.
pub(in crate::symbol_map::extraction) fn argument_at<'a, 'arena>(
    argument_list: &'a ArgumentList<'arena>,
    index: usize,
    name: &[u8],
) -> Option<&'a Expression<'arena>> {
    argument_list
        .arguments
        .iter()
        .enumerate()
        .find_map(|(at, argument)| match argument {
            Argument::Positional(positional) if at == index => Some(positional.value),
            Argument::Named(named) if named.name.value == name => Some(named.value),
            _ => None,
        })
}

/// The keys of an array literal of route parameters, or of a Yii URL
/// generator's route arguments, that are written out, each with the byte
/// range of its text.
pub(in crate::symbol_map::extraction) fn parameter_keys<'arena>(
    parameters: &Expression<'arena>,
    content: &str,
) -> Vec<(&'arena str, u32, u32)> {
    let Some(entries) = crate::parser::array_literal_elements(parameters) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| match entry {
            ArrayElement::KeyValue(entry) => resolved_string_literal(entry.key, content),
            _ => None,
        })
        .filter(|(key, _, _)| !key.is_empty())
        .map(|(key, start, end)| (key, start as u32, end as u32))
        .collect()
}

/// The class a `handlers` array gives the handler `name`, as a class
/// constant or a string.
fn handler_class(handlers: &Expression<'_>, name: &str, ctx: &ExtractionCtx<'_>) -> Option<String> {
    let entries = crate::parser::array_literal_elements(handlers)?;
    entries.iter().find_map(|entry| {
        let ArrayElement::KeyValue(entry) = entry else {
            return None;
        };
        let (key, _, _) = resolved_string_literal(entry.key, ctx.content)?;
        (key == name)
            .then(|| message_class(entry.value, ctx))
            .flatten()
    })
}

/// The entity type an `@ContentEntityType(…)`-family annotation in the
/// docblock `doc`, which starts at `offset` in the file, declares: its ID
/// with the ID's range in the file, and its storage.
fn annotation_entity_type(doc: &str, offset: u32) -> Option<(String, u32, u32, Option<String>)> {
    let (body, default_storage) = ENTITY_TYPE_KINDS.iter().find_map(|kind| {
        let at = doc.find(kind.annotation)?;
        Some((at + kind.annotation.len(), kind.default_storage))
    })?;
    let mut id = None;
    let mut storage = default_storage.map(str::to_string);
    for entry in annotation_entries(doc, body) {
        match (entry.path.as_slice(), entry.key) {
            ([], "id") => id = Some((entry.value_start, entry.value_end)),
            (["handlers"], "storage") => {
                storage = Some(
                    doc[entry.value_start..entry.value_end]
                        .trim_start_matches('\\')
                        .to_string(),
                );
            }
            _ => {}
        }
    }
    let (start, end) = id?;
    let key = &doc[start..end];
    (!key.is_empty()).then(|| {
        (
            key.to_string(),
            offset + start as u32,
            offset + end as u32,
            storage,
        )
    })
}

/// The span of the permission the `admin_permission` of an entity type
/// annotation in the docblock `doc`, which starts at `offset` in the file,
/// names.
fn annotation_admin_permission(doc: &str, offset: u32) -> Option<SymbolSpan> {
    let body = ENTITY_TYPE_KINDS
        .iter()
        .find_map(|kind| Some(doc.find(kind.annotation)? + kind.annotation.len()))?;
    let entry = annotation_entries(doc, body)
        .into_iter()
        .find(|entry| entry.path.is_empty() && entry.key == "admin_permission")?;
    let key = &doc[entry.value_start..entry.value_end];
    (!key.is_empty()).then(|| SymbolSpan {
        start: offset + entry.value_start as u32,
        end: offset + entry.value_end as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::DrupalPermission,
            key: key.to_string(),
            is_write: false,
            is_optional: false,
        },
    })
}

/// A `key = "value"` pair of an annotation, with the keys of the `{…}`
/// blocks around it.
struct AnnotationEntry<'d> {
    path: Vec<&'d str>,
    key: &'d str,
    value_start: usize,
    value_end: usize,
}

/// Every `key = "value"` pair of the annotation whose arguments start at
/// `from` in `doc`, up to its closing parenthesis.  Keys may be quoted.
fn annotation_entries(doc: &str, from: usize) -> Vec<AnnotationEntry<'_>> {
    let bytes = doc.as_bytes();
    let mut entries = Vec::new();
    let mut path: Vec<&str> = Vec::new();
    // The key read last, waiting for its `=`, and whether the `=` came.
    let mut key: Option<&str> = None;
    let mut assigned = false;
    let mut parens = 0usize;
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i + 1;
                let Some(len) = doc[start..].find('"') else {
                    break;
                };
                let end = start + len;
                if assigned {
                    if let Some(name) = key.take() {
                        entries.push(AnnotationEntry {
                            path: path.clone(),
                            key: name,
                            value_start: start,
                            value_end: end,
                        });
                    }
                    assigned = false;
                } else {
                    key = Some(&doc[start..end]);
                }
                i = end + 1;
                continue;
            }
            b'=' => assigned = key.is_some(),
            b'{' => {
                if assigned && let Some(name) = key.take() {
                    path.push(name);
                } else {
                    path.push("");
                }
                assigned = false;
            }
            b'}' => {
                path.pop();
                key = None;
                assigned = false;
            }
            // A nested annotation's own pairs (`@Translation(context = …)`)
            // are not the entity type's.
            b'(' => {
                parens += 1;
                path.push("(");
                key = None;
                assigned = false;
            }
            b')' => {
                if parens == 0 {
                    break;
                }
                parens -= 1;
                path.pop();
                key = None;
                assigned = false;
            }
            b',' => {
                key = None;
                assigned = false;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if !assigned {
                    key = Some(&doc[start..i]);
                } else {
                    // A bare value (`TRUE`, a constant): nothing to read.
                    key = None;
                    assigned = false;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    entries
}

// ─── Libraries ──────────────────────────────────────────────────────────────

/// The key a render array attaches its assets under.
const ATTACHED: &str = "#attached";

/// The key of the libraries among them.
const LIBRARY: &str = "library";

/// Record the libraries an array literal attaches: the `library` list of
/// its `'#attached'` entry.
pub(in crate::symbol_map::extraction) fn emit_drupal_attached_array_spans(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal_attached {
        return;
    }
    for element in elements.iter() {
        if let ArrayElement::KeyValue(entry) = element
            && is_literal_key(entry.key, ATTACHED, ctx.content)
        {
            push_attached_libraries(entry.value, ctx);
        }
    }
}

/// Record the libraries an assignment into a render array's `#attached`
/// attaches: `$build['#attached']['library'][] = '…'`, a whole `library`
/// list, or a whole `#attached` array.
pub(in crate::symbol_map::extraction) fn emit_drupal_attached_assignment_spans(
    assign: &Assignment<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal_attached || !assign.operator.is_assign() {
        return;
    }
    match assign.lhs {
        Expression::ArrayAppend(append)
            if accesses_keys(append.array, &[ATTACHED, LIBRARY], ctx.content) =>
        {
            push_library(assign.rhs, ctx);
        }
        lhs if accesses_keys(lhs, &[ATTACHED, LIBRARY], ctx.content) => {
            push_library_list(assign.rhs, ctx);
        }
        lhs if accesses_keys(lhs, &[ATTACHED], ctx.content) => {
            push_attached_libraries(assign.rhs, ctx);
        }
        _ => {}
    }
}

/// Whether `expression` is an array access whose last offsets are the
/// string literals `keys`, in order.
fn accesses_keys(mut expression: &Expression<'_>, keys: &[&str], content: &str) -> bool {
    for key in keys.iter().rev() {
        let Expression::ArrayAccess(access) = expression else {
            return false;
        };
        if !is_literal_key(access.index, key, content) {
            return false;
        }
        expression = access.array;
    }
    true
}

/// Whether `expression` is the string literal `key`.
fn is_literal_key(expression: &Expression<'_>, key: &str, content: &str) -> bool {
    resolved_string_literal(expression, content).is_some_and(|(found, _, _)| found == key)
}

/// Record the libraries of the `library` list of an `#attached` array
/// literal.
fn push_attached_libraries(attached: &Expression<'_>, ctx: &mut ExtractionCtx<'_>) {
    let Some(entries) = crate::parser::array_literal_elements(attached) else {
        return;
    };
    for entry in entries.iter() {
        if let ArrayElement::KeyValue(entry) = entry
            && is_literal_key(entry.key, LIBRARY, ctx.content)
        {
            push_library_list(entry.value, ctx);
        }
    }
}

/// Record the libraries of a list literal of them.
fn push_library_list(list: &Expression<'_>, ctx: &mut ExtractionCtx<'_>) {
    let Some(items) = crate::parser::array_literal_elements(list) else {
        return;
    };
    for item in items.iter() {
        if let ArrayElement::Value(item) = item {
            push_library(item.value, ctx);
        }
    }
}

/// Record the library a string literal names.
fn push_library(expression: &Expression<'_>, ctx: &mut ExtractionCtx<'_>) {
    let Some((name, start, end)) = resolved_string_literal(expression, ctx.content) else {
        return;
    };
    if crate::drupal::libraries::is_library_name(name) {
        ctx.spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::DrupalLibrary,
                key: name.to_string(),
                is_write: false,
                is_optional: false,
            },
        });
    }
}

// ─── Callbacks ──────────────────────────────────────────────────────────────

/// The keys of a form or render array that hold a list of callbacks.
const CALLBACK_LIST_KEYS: [&str; 8] = [
    "#after_build",
    "#element_validate",
    "#entity_builders",
    "#post_render",
    "#pre_render",
    "#process",
    "#submit",
    "#validate",
];

/// The keys that hold one callback.
const CALLBACK_KEYS: [&str; 2] = ["#access_callback", "#value_callback"];

/// The key whose list holds its callback first, then the arguments.
const LAZY_BUILDER: &str = "#lazy_builder";

/// The key whose array holds its callback under [`AJAX_CALLBACK`].
const AJAX: &str = "#ajax";
const AJAX_CALLBACK: &str = "callback";

/// Whether `content` quotes a key Drupal reads a callback under, found in
/// one pass over its `#`s.
pub(in crate::symbol_map::extraction) fn has_callback_key(content: &str) -> bool {
    let bytes = content.as_bytes();
    memchr::memchr_iter(b'#', bytes).any(|at| {
        at > 0 && matches!(bytes[at - 1], b'\'' | b'"') && {
            let rest = &bytes[at..];
            CALLBACK_LIST_KEYS
                .iter()
                .chain(&CALLBACK_KEYS)
                .chain([&LAZY_BUILDER, &AJAX])
                .any(|key| {
                    rest.starts_with(key.as_bytes())
                        && matches!(rest.get(key.len()), Some(b'\'' | b'"'))
                })
        }
    })
}

/// Record the callbacks an array literal's entries name under the keys
/// Drupal reads callbacks from.
pub(in crate::symbol_map::extraction) fn emit_drupal_callback_spans(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal_callback_keys {
        return;
    }
    for element in elements.iter() {
        if let ArrayElement::KeyValue(entry) = element
            && let Some((key, _, _)) = resolved_string_literal(entry.key, ctx.content)
        {
            push_keyed_callbacks(key, entry.value, ctx);
        }
    }
}

/// Record the callbacks an assignment into a form or render array names:
/// `$form['#submit'][] = …`, a whole list or callback under its key, and
/// `$form['x']['#ajax']['callback'] = …`.
pub(in crate::symbol_map::extraction) fn emit_drupal_callback_assignment(
    assign: &Assignment<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal_callback_keys || !assign.operator.is_assign() {
        return;
    }
    match assign.lhs {
        Expression::ArrayAppend(append) => {
            if let Expression::ArrayAccess(access) = append.array
                && let Some((key, _, _)) = resolved_string_literal(access.index, ctx.content)
                && CALLBACK_LIST_KEYS.contains(&key)
            {
                push_callback(assign.rhs, ctx);
            }
        }
        Expression::ArrayAccess(access) => {
            let Some((key, _, _)) = resolved_string_literal(access.index, ctx.content) else {
                return;
            };
            if key == AJAX_CALLBACK {
                if accesses_keys(access.array, &[AJAX], ctx.content) {
                    push_callback(assign.rhs, ctx);
                }
            } else {
                push_keyed_callbacks(key, assign.rhs, ctx);
            }
        }
        _ => {}
    }
}

/// Record the callbacks `value`, written under `key`, names.
fn push_keyed_callbacks(key: &str, value: &Expression<'_>, ctx: &mut ExtractionCtx<'_>) {
    if CALLBACK_KEYS.contains(&key) {
        push_callback(value, ctx);
        return;
    }
    let lazy_builder = key == LAZY_BUILDER;
    if !lazy_builder && key != AJAX && !CALLBACK_LIST_KEYS.contains(&key) {
        return;
    }
    let Some(entries) = crate::parser::array_literal_elements(value) else {
        return;
    };
    for entry in entries.iter() {
        match entry {
            ArrayElement::Value(item) if key != AJAX => push_callback(item.value, ctx),
            ArrayElement::KeyValue(entry)
                if key == AJAX && is_literal_key(entry.key, AJAX_CALLBACK, ctx.content) =>
            {
                push_callback(entry.value, ctx);
            }
            _ => {}
        }
        if lazy_builder {
            return;
        }
    }
}

/// Record what one callback names: a method of the form for `'::method'`,
/// a method of the class for `'Class::method'` or a class name joined to
/// `'::method'`, and a function for a name without a colon.  An array
/// callable is read wherever it is written, and `'service:method'` names a
/// service rather than a class.
fn push_callback(callback: &Expression<'_>, ctx: &mut ExtractionCtx<'_>) {
    if let Expression::Binary(binary) = callback
        && binary.operator.is_concatenation()
    {
        push_joined_callback(binary.lhs, binary.rhs, ctx);
        return;
    }
    let Some((value, start, end)) = resolved_string_literal(callback, ctx.content) else {
        return;
    };
    if let Some((class, method)) = value.rsplit_once("::") {
        if !is_identifier(method) || !ctx.content[..end].ends_with(method) {
            return;
        }
        let method_start = (end - method.len()) as u32;
        if class.is_empty() {
            push_callback_method(
                SubjectText::owned("$this".to_string()),
                false,
                DocblockMemberRef::FormCallback,
                method,
                method_start,
                ctx,
            );
            return;
        }
        // A class name in a string is a runtime name, which is fully
        // qualified whether or not it says so.
        let class = class.strip_prefix('\\').unwrap_or(class);
        if is_qualified_name(class) {
            push_callback_method(
                SubjectText::owned(format!("\\{class}")),
                true,
                DocblockMemberRef::No,
                method,
                method_start,
                ctx,
            );
        }
    } else if !value.contains(':') {
        let name = value.strip_prefix('\\').unwrap_or(value);
        if is_qualified_name(name) {
            ctx.spans.push(SymbolSpan {
                start: start as u32,
                end: end as u32,
                kind: SymbolKind::FunctionCall {
                    name: crate::atom::atom(&format!("\\{name}")),
                    is_definition: false,
                    is_docblock_reference: false,
                    is_callback: true,
                },
            });
        }
    }
}

/// Record the method `X::class . '::method'`, `static::class . '::method'`,
/// `self::class . '::method'` or `__CLASS__ . '::method'` names.  The
/// `'::method'` half is never read on its own.
fn push_joined_callback(
    class: &Expression<'_>,
    method: &Expression<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some((value, _, end)) = resolved_string_literal(method, ctx.content) else {
        return;
    };
    let Some(name) = value
        .strip_prefix("::")
        .filter(|name| is_identifier(name) && ctx.content[..end].ends_with(name))
    else {
        return;
    };
    let subject = match class {
        Expression::Access(Access::ClassConstant(fetch))
            if crate::parser::is_class_fetch(fetch) =>
        {
            let span = fetch.class.span();
            SubjectText::new(
                expr_to_subject_text(fetch.class),
                span.start.offset,
                span.end.offset,
                ctx.content,
            )
        }
        Expression::MagicConstant(MagicConstant::Class(_)) => {
            SubjectText::owned("self".to_string())
        }
        _ => return,
    };
    push_callback_method(
        subject,
        true,
        DocblockMemberRef::No,
        name,
        (end - name.len()) as u32,
        ctx,
    );
}

/// Record the method `name`, at `start`, of `subject` that a callback
/// names, read like the method of an array callable.
fn push_callback_method(
    subject: SubjectText,
    is_static: bool,
    docblock_ref: DocblockMemberRef,
    name: &str,
    start: u32,
    ctx: &mut ExtractionCtx<'_>,
) {
    ctx.spans.push(SymbolSpan {
        start,
        end: start + name.len() as u32,
        kind: SymbolKind::MemberAccess {
            subject_text: subject,
            member_name: crate::atom::atom(name),
            is_static,
            is_method_call: true,
            docblock_ref,
            is_array_callable: true,
            is_nullsafe: false,
        },
    });
}

/// Whether `name` is a class or function name, its namespace included.
fn is_qualified_name(name: &str) -> bool {
    name.split('\\').all(is_identifier)
}

/// The code `program` runs only when a module is installed (see
/// [`ModuleGuard`]): the branch an `if` takes when a `moduleExists('m')`
/// it requires holds, and the rest of a block after an `if` that leaves
/// it when one is false.
pub(in crate::symbol_map::extraction) fn module_guards(
    program: &Program<'_>,
    content: &str,
) -> Vec<ModuleGuard> {
    use mago_syntax::cst::Node;
    let mut guards = Vec::new();
    let guard_list = |statements: &[Statement<'_>], end: u32, guards: &mut Vec<ModuleGuard>| {
        for statement in statements {
            let Statement::If(if_stmt) = statement else {
                continue;
            };
            let then: Vec<&Statement<'_>> = if_stmt.body.statements().iter().collect();
            let (Some(first), Some(last)) = (then.first(), then.last()) else {
                continue;
            };
            let mut required = Vec::new();
            collect_required_modules(if_stmt.condition, true, content, &mut required);
            for module in required {
                guards.push(ModuleGuard {
                    start: first.span().start.offset,
                    end: last.span().end.offset,
                    module,
                });
            }
            // `if (!moduleExists('m')) { return; }` with nothing else to
            // run guards what follows it.
            let has_else = !if_stmt.body.else_if_statements().is_empty()
                || if_stmt.body.else_statements().is_some();
            let leaves = then.iter().any(|statement| {
                crate::type_engine::type_ops::condition_facts::statement_leaves_block(
                    statement,
                    &|_| false,
                )
            });
            if !has_else && leaves {
                let mut missing = Vec::new();
                collect_required_modules(if_stmt.condition, false, content, &mut missing);
                let start = if_stmt.span().end.offset;
                for module in missing {
                    guards.push(ModuleGuard { start, end, module });
                }
            }
        }
    };
    guard_list(
        program.statements.as_slice(),
        program.span().end.offset,
        &mut guards,
    );
    crate::parser::for_each_node_with_ancestors(Node::Program(program), |node, _| {
        if let Node::Block(block) = node {
            guard_list(
                block.statements.as_slice(),
                block.span().end.offset,
                &mut guards,
            );
        }
    });
    guards
}

/// Add to `modules` each module whose `moduleExists('m')` must be `holds`
/// for `condition` to be true: the call itself, or under a `!`, an `&&`,
/// or, for a call that must be false, an `||`.
fn collect_required_modules(
    condition: &Expression<'_>,
    holds: bool,
    content: &str,
    modules: &mut Vec<String>,
) {
    match condition {
        Expression::Parenthesized(inner) => {
            collect_required_modules(inner.expression, holds, content, modules)
        }
        Expression::UnaryPrefix(unary) if matches!(unary.operator, UnaryPrefixOperator::Not(_)) => {
            collect_required_modules(unary.operand, !holds, content, modules)
        }
        Expression::Binary(binary)
            if (holds
                && matches!(
                    binary.operator,
                    BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
                ))
                || (!holds
                    && matches!(
                        binary.operator,
                        BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
                    )) =>
        {
            collect_required_modules(binary.lhs, holds, content, modules);
            collect_required_modules(binary.rhs, holds, content, modules);
        }
        Expression::Call(Call::Method(call)) if holds => {
            let ClassLikeMemberSelector::Identifier(name) = &call.method else {
                return;
            };
            if !bytes_to_str(name.value).eq_ignore_ascii_case("moduleExists") {
                return;
            }
            if let Some((module, _, _)) = call
                .argument_list
                .arguments
                .first()
                .and_then(|argument| resolved_string_literal(argument.value(), content))
                && !module.is_empty()
            {
                modules.push(module.to_string());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::OwnedResolvedNames;

    /// Every invoking method of the module handler and the theme manager,
    /// at the argument it names a hook in, reaches a receiver that offers
    /// that class's hooks, whichever other table also has the method.
    #[test]
    fn every_hook_invoker_reaches_its_drupal_class() {
        for (invokers, class) in [
            (
                &MODULE_INVOKERS[..],
                "Drupal\\Core\\Extension\\ModuleHandlerInterface",
            ),
            (
                &THEME_INVOKERS[..],
                "Drupal\\Core\\Theme\\ThemeManagerInterface",
            ),
        ] {
            for invoker in invokers {
                let (receiver, alter) = hook_invocation(invoker.method, invoker.hook)
                    .unwrap_or_else(|| {
                        panic!("{}#{} names no receiver", invoker.method, invoker.hook)
                    });
                assert_eq!(alter, invoker.alter, "{}#{}", invoker.method, invoker.hook);
                assert!(
                    receiver
                        .targets()
                        .contains(&(class, StringKeyKind::DrupalHook)),
                    "{}#{}: {receiver:?} has no {class} target",
                    invoker.method,
                    invoker.hook
                );
            }
        }
    }

    /// Each method that checks or changes a permission reaches the classes
    /// that have it, and no other table claims the method first.
    #[test]
    fn every_permission_method_reaches_its_drupal_classes() {
        for (method, classes) in [
            (
                "hasPermission",
                &[
                    "Drupal\\Core\\Session\\AccountInterface",
                    "Drupal\\user\\RoleInterface",
                    "Drupal\\Core\\Session\\PermissionCheckerInterface",
                    "Drupal\\Core\\Session\\CalculatedPermissionsItemInterface",
                ][..],
            ),
            ("grantPermission", &["Drupal\\user\\RoleInterface"][..]),
            ("revokePermission", &["Drupal\\user\\RoleInterface"][..]),
        ] {
            let (receiver, is_write) =
                crate::symbol_map::extraction::symfony_receiver_method(method, 0)
                    .unwrap_or_else(|| panic!("{method} names no permission"));
            assert!(!is_write, "{method}");
            for class in classes {
                assert!(
                    receiver
                        .targets()
                        .contains(&(*class, StringKeyKind::DrupalPermission)),
                    "{method}: {receiver:?} has no {class} target"
                );
            }
            assert!(hook_invocation(method, 0).is_none(), "{method}");
            assert!(
                crate::drupal::route_parameter_call(method).is_none(),
                "{method}"
            );
        }
    }

    /// Each method that reads a configuration name reaches the classes that
    /// have it, and no other table claims the method first.
    #[test]
    fn every_config_method_reaches_its_drupal_classes() {
        for (method, classes) in [
            ("get", &["Drupal\\Core\\Config\\ConfigFactoryInterface"][..]),
            (
                "getEditable",
                &["Drupal\\Core\\Config\\ConfigFactoryInterface"][..],
            ),
            (
                "loadMultiple",
                &["Drupal\\Core\\Config\\ConfigFactoryInterface"][..],
            ),
            (
                "config",
                &[
                    "Drupal\\Core\\Controller\\ControllerBase",
                    "Drupal\\Core\\Form\\FormBase",
                ][..],
            ),
        ] {
            let (receiver, is_write) =
                crate::symbol_map::extraction::symfony_receiver_method(method, 0)
                    .unwrap_or_else(|| panic!("{method} names no configuration"));
            assert!(!is_write, "{method}");
            for class in classes {
                assert!(
                    receiver
                        .targets()
                        .contains(&(*class, StringKeyKind::DrupalConfigName)),
                    "{method}: {receiver:?} has no {class} target"
                );
            }
            assert!(hook_invocation(method, 0).is_none(), "{method}");
            assert!(
                crate::drupal::route_parameter_call(method).is_none(),
                "{method}"
            );
        }
    }

    /// Each configuration name a static call, a `ConfigTarget`, a
    /// `#config_target`, or a config form's editable names reads is a span
    /// over its own text, and the names a `loadMultiple()` lists are left
    /// for the type engine.
    #[test]
    fn config_name_spans_cover_each_name() {
        let php = r#"<?php
namespace Drupal\acme\Form;
use Drupal\Core\Form\ConfigTarget;
class SettingsForm {
    protected function getEditableConfigNames(): array {
        $unused = ['not.this'];
        return ['acme.a', 'acme.b'];
    }
    public function run($factory): void {
        \Drupal::config('acme.c');
        new ConfigTarget('acme.d', 'key');
        $form = ['#config_target' => 'acme.e:key', '#title' => 'acme.not'];
        $factory->loadMultiple(['acme.f', 'acme.g']);
    }
}
"#;
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
                drupal: true,
                twig: false,
                doctrine: false,
                yii_params: false,
                tempest: false,
            },
            None,
        );
        let found: Vec<&str> = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalConfigName,
                    key,
                    ..
                } => {
                    let text = &php[span.start as usize..span.end as usize];
                    assert_eq!(key, text);
                    Some(text)
                }
                _ => None,
            })
            .collect();
        assert_eq!(found, ["acme.a", "acme.b", "acme.c", "acme.d", "acme.e"]);
        let listed: Vec<&str> = map
            .typed_receiver_sites
            .iter()
            .filter(|site| site.receiver == ReceiverClass::ConfigFactory)
            .map(|site| site.key.as_str())
            .collect();
        assert_eq!(listed, ["acme.f", "acme.g"]);
    }

    /// A configuration key written beside its object's name is a span over
    /// its own text, keyed by both, and one a config object reads is left
    /// for the type engine, which knows the object.
    #[test]
    fn config_key_spans_cover_each_key() {
        let php = r#"<?php
namespace Drupal\acme\Form;
use Drupal\Core\Form\ConfigTarget;
class SettingsForm {
    public function run($config): void {
        $form = ['#config_target' => 'acme.a:notify.email'];
        new ConfigTarget('acme.b', 'one');
        new ConfigTarget('acme.c', ['two', 'three']);
        $config->get('four');
        $config->set('five', 1);
        $config->clear('six');
        $config->other('seven');
    }
}
"#;
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
                drupal: true,
                twig: false,
                doctrine: false,
                yii_params: false,
                tempest: false,
            },
            None,
        );
        let found: Vec<(&str, &str)> = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalConfigKey,
                    key,
                    ..
                } => Some((&php[span.start as usize..span.end as usize], key.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            found,
            [
                ("notify.email", "acme.a#notify.email"),
                ("one", "acme.b#one"),
                ("two", "acme.c#two"),
                ("three", "acme.c#three"),
            ]
        );
        let sites: Vec<(&str, ReceiverClass)> = map
            .typed_receiver_sites
            .iter()
            .filter(|site| {
                site.receiver
                    .targets()
                    .iter()
                    .any(|(_, kind)| *kind == StringKeyKind::DrupalConfigKey)
            })
            .map(|site| (site.key.as_str(), site.receiver))
            .collect();
        assert_eq!(
            sites,
            [
                ("four", ReceiverClass::ContainerLookup),
                ("five", ReceiverClass::EditableConfigKeys),
                ("six", ReceiverClass::EditableConfigKeys),
            ]
        );
    }

    /// Each permission a requirement, a list, or an entity type names is a
    /// span over its own text.
    #[test]
    fn permission_spans_cover_each_name() {
        let php = r#"<?php
namespace Drupal\acme;
use Drupal\Core\Access\AccessResult;
use Drupal\Core\Entity\Attribute\ContentEntityType;
use Symfony\Component\Routing\Route;
#[ContentEntityType(id: 'acme_note', admin_permission: 'administer acme')]
class Note {
    public function run($account, $route): void {
        new Route('/a', [], ['_permission' => 'a+b']);
        new Route('/a', requirements: ['_permission' => 'c,d+e']);
        $route->setRequirement('_permission', 'f');
        $route->setRequirement('_format', 'json');
        AccessResult::allowedIfHasPermissions($account, ['g', 'h'], 'OR');
        AccessResult::allowedIfHasPermission($account, 'i');
        user_role_grant_permissions('editor', ['j']);
    }
}
"#;
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
                drupal: true,
                twig: false,
                doctrine: false,
                yii_params: false,
                tempest: false,
            },
            None,
        );
        let found: Vec<&str> = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalPermission,
                    key,
                    ..
                } => {
                    let text = &php[span.start as usize..span.end as usize];
                    assert_eq!(key, text);
                    Some(text)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            found,
            [
                "administer acme",
                "a",
                "b",
                "c",
                "d+e",
                "f",
                "g",
                "h",
                "i",
                "j"
            ]
        );
    }

    /// The entity type IDs, classes, and storages `php` declares.
    fn declared(php: &str) -> Vec<(String, String, Option<String>)> {
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
                drupal: true,
                twig: false,
                doctrine: false,
                yii_params: false,
                tempest: false,
            },
            None,
        );
        map.entity_types
            .iter()
            .map(|site| {
                let id = map
                    .spans
                    .iter()
                    .find_map(|span| match &span.kind {
                        SymbolKind::StringKey {
                            kind: StringKeyKind::DrupalEntityType,
                            key,
                            is_write: true,
                            ..
                        } if span.start == site.start => Some(key.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                (
                    id,
                    site.class.to_string(),
                    site.storage.as_ref().map(ToString::to_string),
                )
            })
            .collect()
    }

    /// The route names `php`'s route builders declare, and the patterns
    /// they keep.
    fn built(php: &str) -> (Vec<String>, Vec<String>) {
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
                drupal: true,
                twig: false,
                doctrine: false,
                yii_params: false,
                tempest: false,
            },
            None,
        );
        let declared = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalRoute,
                    key,
                    is_write: true,
                    ..
                } => Some(key.clone()),
                _ => None,
            })
            .collect();
        (declared, map.drupal_route_patterns.clone())
    }

    #[test]
    fn route_builders_declare_names_and_keep_patterns() {
        let php = r#"<?php
namespace Drupal\search\Routing;
use Symfony\Component\Routing\Route;
use Symfony\Component\Routing\RouteCollection;
class SearchPageRoutes {
    public function routes(RouteCollection $collection, string $id, string $name): array {
        $routes['search.view'] = new Route('/search');
        $routes["search.view_$id"] = new Route('/search/' . $id);
        $routes['search.help_' . $id] = new Route('/help');
        $collection->add('search.literal', new Route('/x'));
        $collection->add('search.called', static::route($id));
        $collection->add("{$name}.bc", new Route('/y'));
        $collection->add($name, new Route('/z'));
        $routes['not_a_route'] = new \ArrayObject();
        $form->add('not_a_route_either');
        $bag->add('nor_this', 1);
        $bag->add($id . '.nor_this', [1]);
        return $routes;
    }
}
"#;
        let (mut declared, mut patterns) = built(php);
        declared.sort();
        patterns.sort();
        assert_eq!(declared, ["search.called", "search.literal", "search.view"]);
        // `"{$name}.bc"` and `$name` start with no written-out text, so
        // they stand for the extension's own routes.
        assert_eq!(patterns, ["search.*", "search.help_*", "search.view_*"]);
    }

    #[test]
    fn routes_held_in_variables_array_literals_and_traits_are_built() {
        let php = r#"<?php
namespace Drupal\layout_builder\Routing;
use Symfony\Component\Routing\{Route, RouteCollection};
trait LayoutRoutes {
    protected function build(RouteCollection $collection, string $prefix): array {
        $route = new Route('/x');
        $routes['layout_builder.held'] = $route;
        $collection->add("$prefix.view", $route);
        $options['not_a_route'] = $prefix;
        return ['layout_builder.literal' => new Route('/y'), 'not_either' => [1]];
    }
}
"#;
        let (mut declared, patterns) = built(php);
        declared.sort();
        assert_eq!(declared, ["layout_builder.held", "layout_builder.literal"]);
        assert_eq!(patterns, ["layout_builder.*"]);
    }

    /// The route names and aliases `php`'s controllers declare.
    fn attribute_routes(php: &str) -> (Vec<String>, Vec<String>) {
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
                drupal: true,
                twig: false,
                doctrine: false,
                yii_params: false,
                tempest: false,
            },
            None,
        );
        let mut declared: Vec<String> = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalRoute,
                    key,
                    is_write: true,
                    ..
                } => Some(key.clone()),
                _ => None,
            })
            .collect();
        declared.sort();
        let mut aliases = map.drupal_route_aliases.clone();
        aliases.sort();
        (declared, aliases)
    }

    #[test]
    fn controllers_alias_single_routes_by_class_and_method() {
        let php = r#"<?php
namespace Drupal\acme\Controller;
use Symfony\Component\Routing\Attribute\Route;
#[Route(path: '/notes', name: 'acme.notes.')]
final class Notes {
    #[Route(path: '/list', name: 'list')]
    public function list(): array { return []; }
    #[Route(path: '/a', name: 'a')]
    #[Route(path: '/b', name: 'b')]
    public function twice(): array { return []; }
    public function none(): array { return []; }
}
#[Route('/about', name: 'acme.about')]
final class About {
    public function __invoke(): array { return []; }
}
abstract class Base {
    #[Route(path: '/base', name: 'acme.base')]
    public function base(): array { return []; }
}
"#;
        let (declared, aliases) = attribute_routes(php);
        assert_eq!(
            declared,
            [
                "acme.about",
                "acme.base",
                "acme.notes.a",
                "acme.notes.b",
                "acme.notes.list"
            ]
        );
        assert_eq!(
            aliases,
            [
                "Drupal\\acme\\Controller\\About",
                "Drupal\\acme\\Controller\\About::__invoke",
                "Drupal\\acme\\Controller\\Notes::list",
            ]
        );
    }

    /// The route parameter keys `php`'s static calls pass, sorted, and the
    /// ones its method calls leave for the type engine to confirm, with the
    /// receiver they need.
    fn route_parameters(
        php: &str,
    ) -> (Vec<String>, Vec<(String, crate::symbol_map::ReceiverClass)>) {
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
                drupal: true,
                twig: false,
                doctrine: false,
                yii_params: false,
                tempest: false,
            },
            None,
        );
        let mut passed: Vec<String> = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalRouteParameter,
                    key,
                    is_write: false,
                    ..
                } => {
                    assert_eq!(
                        key.rsplit('#').next(),
                        php.get(span.start as usize..span.end as usize)
                    );
                    Some(key.clone())
                }
                _ => None,
            })
            .collect();
        passed.sort();
        let sites = map
            .typed_receiver_sites
            .iter()
            .filter(|site| {
                site.receiver
                    .targets()
                    .iter()
                    .any(|(_, kind)| *kind == StringKeyKind::DrupalRouteParameter)
            })
            .map(|site| (site.key.clone(), site.receiver))
            .collect();
        (passed, sites)
    }

    #[test]
    fn route_calls_pass_the_parameters_of_their_route() {
        let php = r#"<?php
namespace Drupal\acme;
use Drupal\Core\Link;
use Drupal\Core\Url;
final class Links {
    public function run($form_state, $urls, string $id): void {
        Url::fromRoute('acme.note', ['acme_note' => $id, 'mode' => 'full', 0 => 'x']);
        new Url('acme.note', route_parameters: ['acme_note' => 1]);
        Link::createFromRoute('Note', 'acme.note', ['acme_note' => 2]);
        Url::fromRoute($id, ['acme_note' => 3]);
        $form_state->setRedirect('acme.note', ['acme_note' => 4]);
        $urls->generateFromRoute('acme.note', parameters: ['mode' => 'teaser']);
        $form_state->setRedirect('acme.note', 'not_an_array');
        $form_state->other('acme.note', ['nope' => 1]);
    }
}
"#;
        let (passed, sites) = route_parameters(php);
        assert_eq!(
            passed,
            [
                "acme.note#acme_note",
                "acme.note#acme_note",
                "acme.note#acme_note",
                "acme.note#mode",
            ]
        );
        assert_eq!(
            sites,
            [
                (
                    "acme.note#acme_note".to_string(),
                    crate::symbol_map::ReceiverClass::FormRedirectParameters
                ),
                (
                    "acme.note#mode".to_string(),
                    crate::symbol_map::ReceiverClass::UrlGeneratorParameters
                ),
            ]
        );
    }

    /// A call's parameters are confirmed by a receiver of the same classes
    /// as its route name, so neither can be read without the other.
    #[test]
    fn route_parameter_receivers_reach_the_classes_of_the_route_name() {
        let classes = |receiver: crate::symbol_map::ReceiverClass, wanted: StringKeyKind| {
            receiver
                .targets()
                .iter()
                .filter(|(_, kind)| *kind == wanted)
                .map(|(class, _)| *class)
                .collect::<Vec<&str>>()
        };
        for method in [
            "setRedirect",
            "redirect",
            "generateFromRoute",
            "generate",
            "checkNamedRoute",
        ] {
            let (names, _) = crate::symbol_map::extraction::symfony_receiver_method(method, 0)
                .unwrap_or_else(|| panic!("{method} names no route"));
            let (parameters, _) = crate::drupal::route_parameter_call(method)
                .unwrap_or_else(|| panic!("{method} passes no parameters"));
            assert_eq!(
                classes(names, StringKeyKind::DrupalRoute),
                classes(parameters, StringKeyKind::DrupalRouteParameter),
                "{method}"
            );
        }
    }

    #[test]
    fn route_attributes_declare_their_parameters_and_routes() {
        let php = r#"<?php
namespace Drupal\acme\Controller;
use Symfony\Component\Routing\Attribute\Route;
#[Route(path: '/notes/{acme_note}', name: 'acme.notes.')]
final class Notes {
    #[Route(path: '/view/{mode}', name: 'view', options: ['parameters' => ['acme_note' => ['type' => 'entity:acme_note']]])]
    public function view($acme_note, string $mode): array { return []; }
}
#[Route('/about/{node}', name: 'acme.about', defaults: ['_entity_view' => 'node.full'])]
final class About {
    public function __invoke(): array { return []; }
}
"#;
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
                drupal: true,
                twig: false,
                doctrine: false,
                yii_params: false,
                tempest: false,
            },
            None,
        );
        let declared: Vec<(&str, &str)> = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalRouteParameter,
                    key,
                    is_write: true,
                    ..
                } => Some((&php[span.start as usize..span.end as usize], key.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            declared,
            [
                ("acme_note", "acme.notes.view#acme_note"),
                ("mode", "acme.notes.view#mode"),
                ("node", "acme.about#node"),
            ]
        );
        assert_eq!(
            map.drupal_routes,
            [
                DrupalRouteSite {
                    name: "acme.notes.view".to_string(),
                    path: "/notes/{acme_note}/view/{mode}".to_string(),
                    controller: Some("Drupal\\acme\\Controller\\Notes::view".to_string()),
                    title_callback: None,
                    parameters: vec![DeclaredParameter {
                        name: "acme_note".to_string(),
                        ty: Some("entity:acme_note".to_string()),
                        converted_otherwise: false,
                    }],
                    defaults: Vec::new(),
                    entity_type: None,
                },
                DrupalRouteSite {
                    name: "acme.about".to_string(),
                    path: "/about/{node}".to_string(),
                    controller: Some("Drupal\\acme\\Controller\\About::__invoke".to_string()),
                    title_callback: None,
                    parameters: Vec::new(),
                    defaults: Vec::new(),
                    entity_type: Some("node".to_string()),
                },
            ]
        );
    }

    #[test]
    fn an_entity_route_provider_declares_the_routes_it_renames() {
        let php = r#"<?php
namespace Drupal\node\Entity;
use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Entity\Routing\DefaultHtmlRouteProvider;
class NodeRouteProvider extends DefaultHtmlRouteProvider {
    public function getRoutes(EntityTypeInterface $entity_type) {
        $routes = parent::getRoutes($entity_type);
        $addFormRoute = $routes->get('entity.node.add_form');
        $routes->remove('entity.node.add_form');
        $routes->add('node.add', $addFormRoute);
        return $routes;
    }
}
"#;
        assert_eq!(built(php), (vec!["node.add".to_string()], Vec::new()));
    }

    #[test]
    fn a_file_without_symfony_routes_builds_none() {
        let php = "<?php\nnamespace Drupal\\acme;\nclass Cart {\n    public function run($items) { $items->add('acme.thing', 1); }\n}\n";
        assert_eq!(built(php), (Vec::new(), Vec::new()));
    }

    #[test]
    fn api_files_document_hooks_after_the_hook_prefix() {
        let php = "<?php\nfunction hook_form_alter(&$form) {}\nfunction other_thing() {}\n";
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        let extract = |file: crate::drupal::hooks::DrupalFile| {
            crate::symbol_map::extraction::extract_symbol_map_for_file(
                program,
                php,
                &names,
                crate::symbol_map::extraction::Frameworks {
                    symfony: false,
                    service_container: true,
                    drupal: true,
                    twig: false,
                    doctrine: false,
                    yii_params: false,
                    tempest: false,
                },
                None,
                file,
            )
        };
        let map = extract(crate::drupal::hooks::DrupalFile::Api);
        let name = php.find("hook_form_alter").unwrap() as u32;
        assert!(matches!(
            map.lookup(name).map(|span| &span.kind),
            Some(SymbolKind::FunctionCall { .. })
        ));
        assert!(matches!(
            map.lookup(name + 6).map(|span| &span.kind),
            Some(SymbolKind::StringKey { kind: StringKeyKind::DrupalHook, key, is_write: true, .. }) if key == "form_alter"
        ));
        let other = extract(crate::drupal::hooks::DrupalFile::Other);
        assert!(other.spans.iter().all(|span| !matches!(
            span.kind,
            SymbolKind::StringKey {
                kind: StringKeyKind::DrupalHook,
                ..
            }
        )));
    }

    #[test]
    fn an_attribute_declares_the_id_class_and_storage() {
        let php = "<?php\nnamespace Drupal\\node\\Entity;\nuse Drupal\\Core\\Entity\\Attribute\\ContentEntityType;\nuse Drupal\\node\\NodeStorage;\n#[ContentEntityType(\n  id: 'node',\n  handlers: ['storage' => NodeStorage::class, 'form' => ['default' => 'X']],\n)]\nclass Node {}\n";
        assert_eq!(
            declared(php),
            vec![(
                "node".to_string(),
                "Drupal\\node\\Entity\\Node".to_string(),
                Some("Drupal\\node\\NodeStorage".to_string())
            )]
        );
    }

    #[test]
    fn a_config_attribute_without_handlers_gets_config_storage() {
        let php = "<?php\nnamespace Drupal\\acme\\Entity;\nuse Drupal\\Core\\Entity\\Attribute\\ConfigEntityType;\n#[ConfigEntityType(id: 'acme_preset')]\nclass Preset {}\n";
        assert_eq!(
            declared(php),
            vec![(
                "acme_preset".to_string(),
                "Drupal\\acme\\Entity\\Preset".to_string(),
                Some(CONFIG_STORAGE.to_string())
            )]
        );
    }

    #[test]
    fn an_annotation_declares_too_and_another_attribute_does_not() {
        let php = "<?php\nnamespace Drupal\\flag\\Entity;\n/**\n * @ConfigEntityType(\n *   id = \"flag\",\n * )\n */\nclass Flag {}\n#[\\Other\\ContentEntityType(id: 'nope')]\nclass Other {}\n";
        assert_eq!(
            declared(php),
            vec![(
                "flag".to_string(),
                "Drupal\\flag\\Entity\\Flag".to_string(),
                Some(CONFIG_STORAGE.to_string())
            )]
        );
    }

    #[test]
    fn annotations_give_the_id_and_the_storage_handler() {
        let doc = "/**\n * @ContentEntityType(\n *   id = \"redirect\",\n *   label = @Translation(\"Redirect\"),\n *   handlers = {\n *     \"list_builder\" = \"Drupal\\redirect\\RedirectListBuilder\",\n *     \"storage\" = \"\\Drupal\\redirect\\RedirectStorage\",\n *     \"form\" = {\n *       \"id\" = \"nested\"\n *     }\n *   },\n *   entity_keys = {\n *     \"id\" = \"rid\"\n *   }\n * )\n */";
        let (id, start, end, storage) = annotation_entity_type(doc, 100).unwrap();
        assert_eq!(id, "redirect");
        assert_eq!(
            &doc[(start - 100) as usize..(end - 100) as usize],
            "redirect"
        );
        assert_eq!(
            storage.as_deref(),
            Some("Drupal\\redirect\\RedirectStorage")
        );
    }

    #[test]
    fn an_annotation_without_handlers_gets_its_kinds_storage() {
        let doc = "/**\n * @ConfigEntityType(id = \"flag\", label = @Translation(\"Flag\"))\n */";
        let (id, _, _, storage) = annotation_entity_type(doc, 0).unwrap();
        assert_eq!(id, "flag");
        assert_eq!(storage.as_deref(), Some(CONFIG_STORAGE));
    }

    #[test]
    fn a_nested_annotations_pairs_are_its_own() {
        let doc = "/**\n * @ContentEntityType(\n *   label = @Translation(\"Note\", id = \"other\"),\n *   id = \"note\",\n *   extra = @Bar(handlers = {\"storage\" = \"Wrong\"}),\n * )\n */";
        let (id, _, _, storage) = annotation_entity_type(doc, 0).unwrap();
        assert_eq!(id, "note");
        assert_eq!(storage.as_deref(), Some(CONTENT_STORAGE));
    }

    #[test]
    fn a_doc_comment_without_an_entity_type_declares_none() {
        assert!(annotation_entity_type("/** @see EntityType(id = \"x\") */", 0).is_none());
    }

    /// The symbol map of `php` in a Drupal project, or in another one.
    fn map_of(php: &str, drupal: bool) -> crate::symbol_map::SymbolMap {
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
                symfony: false,
                service_container: drupal,
                drupal,
                twig: false,
                doctrine: false,
                yii_params: false,
                tempest: false,
            },
            None,
        )
    }

    /// The render elements `php` declares, each with the ID's text and its
    /// class.
    fn elements(php: &str) -> Vec<(String, String, String)> {
        let map = map_of(php, true);
        map.drupal_plugin_classes
            .iter()
            .filter_map(|site| {
                let span = map.spans.iter().find(|span| {
                    span.start == site.start
                        && matches!(
                            span.kind,
                            SymbolKind::StringKey {
                                kind: StringKeyKind::DrupalRenderElement,
                                is_write: true,
                                ..
                            }
                        )
                })?;
                let SymbolKind::StringKey { key, .. } = &span.kind else {
                    return None;
                };
                Some((
                    key.clone(),
                    php[span.start as usize..span.end as usize].to_string(),
                    site.class.to_string(),
                ))
            })
            .collect()
    }

    /// The render element names `php` reads, in source order.
    fn element_reads(php: &str, drupal: bool) -> Vec<String> {
        map_of(php, drupal)
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalRenderElement,
                    key,
                    is_write: false,
                    ..
                } => {
                    assert_eq!(&php[span.start as usize..span.end as usize], key);
                    Some(key.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn render_and_form_element_attributes_declare_their_ids() {
        let php = "<?php\nnamespace Drupal\\acme\\Element;\nuse Drupal\\Core\\Render\\Attribute\\RenderElement;\nuse Drupal\\Core\\Render\\Attribute\\FormElement;\n#[RenderElement('acme_card')]\nclass Card {}\n#[FormElement(id: 'acme_input')]\nclass Input {}\n#[\\Drupal\\Core\\Render\\Attribute\\RenderElement('acme_badge')]\nclass Badge {}\n";
        assert_eq!(
            elements(php),
            vec![
                (
                    "acme_card".to_string(),
                    "acme_card".to_string(),
                    "Drupal\\acme\\Element\\Card".to_string()
                ),
                (
                    "acme_input".to_string(),
                    "acme_input".to_string(),
                    "Drupal\\acme\\Element\\Input".to_string()
                ),
                (
                    "acme_badge".to_string(),
                    "acme_badge".to_string(),
                    "Drupal\\acme\\Element\\Badge".to_string()
                ),
            ]
        );
    }

    #[test]
    fn element_annotations_declare_by_position_or_by_id() {
        let php = "<?php\nnamespace Drupal\\acme\\Element;\n/**\n * Provides a card.\n *\n * @RenderElement(\"acme_card\")\n */\nclass Card {}\n/**\n * @FormElement(id = \"acme_input\")\n */\nclass Input {}\n/**\n * @see RenderElement(\"nope\")\n */\nclass Nope {}\n";
        assert_eq!(
            elements(php),
            vec![
                (
                    "acme_card".to_string(),
                    "acme_card".to_string(),
                    "Drupal\\acme\\Element\\Card".to_string()
                ),
                (
                    "acme_input".to_string(),
                    "acme_input".to_string(),
                    "Drupal\\acme\\Element\\Input".to_string()
                ),
            ]
        );
    }

    /// An attribute is matched by the class its name resolves to, so a
    /// class of the same short name elsewhere declares nothing.
    #[test]
    fn another_render_element_attribute_declares_nothing() {
        let imported = "<?php\nnamespace Drupal\\acme\\Element;\nuse Acme\\Attribute\\RenderElement;\n#[RenderElement('acme_card')]\nclass Card {}\n";
        let unimported = "<?php\nnamespace Drupal\\acme\\Element;\n#[RenderElement('acme_card')]\nclass Card {}\n";
        assert!(elements(imported).is_empty());
        assert!(elements(unimported).is_empty());
    }

    #[test]
    fn an_element_with_attribute_and_annotation_declares_once() {
        let php = "<?php\nnamespace Drupal\\acme\\Element;\nuse Drupal\\Core\\Render\\Attribute\\RenderElement;\n/**\n * @RenderElement(\"acme_card\")\n */\n#[RenderElement('acme_card')]\nclass Card {}\n";
        assert_eq!(elements(php).len(), 1);
    }

    #[test]
    fn type_keys_of_render_arrays_read_element_names() {
        let php = "<?php\nfunction build(array $form, string $type): array {\n    $form['name']['#type'] = 'textfield';\n    $form[\"#type\"] = \"form\";\n    $form['other']['#type'] .= 'nope';\n    $form['dynamic']['#type'] = $type;\n    $form['type'] = 'not_an_element';\n    return [\n        '#type' => 'details',\n        'inner' => array('#type' => 'container', '#title' => 'not_an_element'),\n        '#theme' => 'not_an_element',\n        'computed' => ['#type' => $type],\n    ];\n}\n";
        assert_eq!(
            element_reads(php, true),
            ["textfield", "form", "details", "container"]
        );
        assert!(element_reads(php, false).is_empty());
    }

    /// The theme hook and variable spans `php` reads, as kind, key, and
    /// text, read as a Drupal project's file or not.
    fn render_array_reads(php: &str, drupal: bool) -> Vec<(StringKeyKind, String, String)> {
        map_of(php, drupal)
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey { kind, key, .. }
                    if matches!(
                        kind,
                        StringKeyKind::DrupalThemeHook | StringKeyKind::DrupalThemeHookVariable
                    ) =>
                {
                    Some((
                        *kind,
                        key.clone(),
                        php[span.start as usize..span.end as usize].to_string(),
                    ))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn render_arrays_name_theme_hooks_and_pass_their_variables() {
        let php = r#"<?php
$build = [
  '#theme' => 'acme_greeting',
  '#name' => $name,
  '#' => 1,
  'child' => ['#theme' => ['acme_box__wide', 'acme_box'], '#label' => 'x'],
  '#theme_wrappers' => ['container', 'acme_panel' => ['#title' => 'y']],
];
$build['more']['#theme'] = 'item_list';
"#;
        let hook = |key: &str| {
            (
                StringKeyKind::DrupalThemeHook,
                key.to_string(),
                key.to_string(),
            )
        };
        assert_eq!(
            render_array_reads(php, true),
            vec![
                hook("acme_greeting"),
                (
                    StringKeyKind::DrupalThemeHookVariable,
                    "acme_greeting#name".to_string(),
                    "#name".to_string()
                ),
                hook("acme_box__wide"),
                hook("acme_box"),
                hook("container"),
                hook("acme_panel"),
                hook("item_list"),
            ]
        );
        assert!(render_array_reads(php, false).is_empty());
    }

    /// A render array whose `#theme` is one name renders that hook's
    /// templates with its other `#` keys, without the `#`.
    #[test]
    fn a_theme_render_array_passes_its_hash_keys_to_the_hooks_templates() {
        let php = r#"<?php
$build = [
  '#theme' => 'acme_greeting',
  '#name' => $name,
  '#' => 1,
  '#not-a-name' => 2,
  '#cache' => ['max-age' => 0],
  '#weight' => 5,
  'child' => ['#theme' => ['acme_box__wide', 'acme_box'], '#label' => 'x'],
];
$build['more']['#theme'] = 'item_list';
"#;
        assert!(RENDER_API_KEYS.is_sorted());
        let map = map_of(php, true);
        // The renderer's own keys pass nothing.
        assert!(!map.spans.iter().any(|span| matches!(
            &span.kind,
            SymbolKind::StringKey { kind: StringKeyKind::DrupalThemeHookVariable, key, .. }
                if key.ends_with("#cache") || key.ends_with("#weight")
        )));
        let sites: Vec<(String, Vec<(String, String)>)> = map
            .twig_contexts
            .iter()
            .map(|site| {
                let start = site.template_start as usize;
                let hook = &php[start..start + php[start..].find('\'').unwrap()];
                let vars = site
                    .vars
                    .iter()
                    .map(|var| {
                        assert_eq!(&php[var.start as usize..var.end as usize], &*var.name);
                        let value = match var.value {
                            crate::symbol_map::TwigContextValue::Expression { start, end } => {
                                php[start as usize..end as usize].to_string()
                            }
                            other => format!("{other:?}"),
                        };
                        (var.name.to_string(), value)
                    })
                    .collect();
                (hook.to_string(), vars)
            })
            .collect();
        assert_eq!(
            sites,
            vec![(
                "acme_greeting".to_string(),
                vec![("name".to_string(), "$name".to_string())]
            )]
        );
        assert!(map_of(php, false).twig_contexts.is_empty());
    }

    #[test]
    fn a_callback_key_is_one_a_quote_opens_and_closes() {
        for (php, expected) in [
            ("$form['#submit'][] = 'x';", true),
            ("['#ajax' => []]", true),
            ("[\"#lazy_builder\" => []]", true),
            ("$form['#value_callback'] = 'x';", true),
            ("$form['#submitted'] = 1;", false),
            ("# submit, '#type' => 'x'", false),
            ("#[Hook('form_alter')]", false),
        ] {
            assert_eq!(has_callback_key(php), expected, "{php}");
        }
    }

    /// The text each callback span of `php` covers, with what it names:
    /// `subject::method` for a method and `function name` for a function.
    fn callbacks(php: &str, drupal: bool) -> Vec<(String, String)> {
        let map = map_of(php, drupal);
        let source = map.source(php).expect("the map's own source");
        map.spans
            .iter()
            .filter_map(|span| {
                let named = match &span.kind {
                    SymbolKind::MemberAccess {
                        subject_text,
                        member_name,
                        is_array_callable: true,
                        ..
                    } => format!("{}::{member_name}", subject_text.as_str(source)),
                    SymbolKind::FunctionCall {
                        name,
                        is_callback: true,
                        ..
                    } => format!("function {name}"),
                    _ => return None,
                };
                Some((
                    php[span.start as usize..span.end as usize].to_string(),
                    named,
                ))
            })
            .collect()
    }

    #[test]
    fn each_callback_shape_names_its_method_or_function() {
        let php = r#"<?php
namespace Acme;
$form['#submit'][] = '::save';
$form['#validate'] = ['\Acme\Helper::check', Helper::class . '::check', 'acme_check', 'acme.checker:check'];
$form['x']['#ajax']['callback'] = static::class . '::refresh';
$form['y'] = ['#ajax' => ['callback' => __CLASS__ . '::refresh'], 'callback' => '::nothing'];
$form['#lazy_builder'] = ['Acme\Builder::build', ['::argument']];
$form['#value_callback'] = self::class . '::value';
$form['#markup'] = '::nothing';
$joined = static::class . '::nothing';
"#;
        let expected = [
            ("save", "$this::save"),
            ("check", "\\Acme\\Helper::check"),
            ("check", "Helper::check"),
            ("acme_check", "function \\acme_check"),
            ("refresh", "static::refresh"),
            ("refresh", "self::refresh"),
            ("build", "\\Acme\\Builder::build"),
            ("value", "self::value"),
        ];
        let found = callbacks(php, true);
        assert_eq!(
            found
                .iter()
                .map(|(text, named)| (text.as_str(), named.as_str()))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(callbacks(php, false), Vec::new());
    }
}
