//! The entity types a Drupal entity class declares, by attribute or by the
//! annotation older code uses, and the route names Drupal's static and
//! constructor calls read (see [`crate::drupal`]).

use super::*;
use crate::symbol_map::extraction::symfony::{
    RouteDeclaration, RouteName, push_route_spans, route_declarations,
};
use crate::symbol_map::{EntityTypeSite, StringKeyKind};
use crate::virtual_members::laravel::helpers::resolved_string_literal;

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

/// Record the entity type `class` declares: a span over its ID, and the
/// class and storage the ID stands for.
pub(in crate::symbol_map::extraction) fn emit_drupal_entity_type_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.drupal {
        return;
    }
    let mut found = Vec::new();
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
    }
    if found.is_empty() {
        return;
    }
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
    let prefix = match class_routes.first().map(|route| &route.name) {
        Some(RouteName::Literal(name, _, _)) => name.as_str(),
        _ => "",
    };
    let is_named = |route: &&RouteDeclaration| matches!(route.name, RouteName::Literal(..));
    let mut spans = Vec::new();
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
        any_method_route |= !routes.is_empty();
        let named = routes.iter().filter(is_named).count();
        route_count += named;
        fqcn_alias |= is_invoke && named > 0;
        if named == 1 {
            aliases.push(format!("{fqn}::{method_name}"));
        }
        for route in routes.iter().filter(is_named) {
            push_route_spans(
                route,
                prefix,
                None,
                StringKeyKind::DrupalRoute,
                &[],
                &mut spans,
            );
        }
    }
    if !any_method_route && invokable && !is_abstract {
        for route in class_routes.iter().filter(is_named) {
            route_count += 1;
            fqcn_alias = true;
            push_route_spans(route, "", None, StringKeyKind::DrupalRoute, &[], &mut spans);
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
    }
    ctx.spans.extend(spans);
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
/// the traits they share do.  `$collection->add(NAME, $route)`,
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
    if !ctx.drupal || !ctx.content.contains("Symfony\\Component\\Routing\\") {
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
    let mut names: Vec<&Expression<'_>> = found.added;
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
    /// The first argument of each `->add()` with two or more arguments.
    added: Vec<&'ast Expression<'arena>>,
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
            && arguments.len() >= 2
            && let Some(Argument::Positional(first)) = arguments.first()
        {
            found.added.push(first.value);
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
    let mut pattern = String::new();
    push_name_parts(expression, &mut pattern);
    BuiltName::Pattern(pattern)
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

/// Record the route name a call reads in its positional argument `index`,
/// or in the `route_name:` argument that names it.
pub(in crate::symbol_map::extraction) fn emit_drupal_route_span(
    index: usize,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let route =
        argument_list
            .arguments
            .iter()
            .enumerate()
            .find_map(|(at, argument)| match argument {
                Argument::Positional(positional) if at == index => Some(positional.value),
                Argument::Named(named) if named.name.value == b"route_name" => Some(named.value),
                _ => None,
            });
    if let Some(route) = route {
        push_laravel_string_span(
            StringKeyKind::DrupalRoute,
            false,
            false,
            route,
            ctx.content,
            &mut ctx.spans,
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::OwnedResolvedNames;

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
        $collection->add("{$name}.bc", new Route('/y'));
        $collection->add($name, new Route('/z'));
        $routes['not_a_route'] = new \ArrayObject();
        $form->add('not_a_route_either');
        return $routes;
    }
}
"#;
        let (mut declared, mut patterns) = built(php);
        declared.sort();
        patterns.sort();
        assert_eq!(declared, ["search.literal", "search.view"]);
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

    #[test]
    fn a_file_without_symfony_routes_builds_none() {
        let php = "<?php\nnamespace Drupal\\acme;\nclass Cart {\n    public function run($items) { $items->add('acme.thing', 1); }\n}\n";
        assert_eq!(built(php), (Vec::new(), Vec::new()));
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
}
