//! The routes Yii's router is configured with, and the route arguments a
//! handler reads.
//!
//! A route is a chain on `Yiisoft\Router\Route` (`Route::get('/blog/{slug}')
//! ->action([PostController::class, 'view'])->name('blog/post')`), usually
//! one of the entries a `Group`'s `routes()` takes, and the group prefixes
//! the name of every route in it with its `namePrefix()` and the path with
//! the prefix its `create()` takes.  A chain is read once, from its
//! outermost call, with the prefixes of the groups around it.  Its name is
//! declared as a [`StringKeyKind::YiiRoute`] span, and its path and handler
//! are kept beside it (see [`RouteSite`]).
//!
//! A URL generator's `generate()` reads a route name only on the right
//! receiver, so it is recorded with the Symfony calls for the type engine
//! to confirm (see [`symfony_receiver_method`]).

use super::*;
use crate::symbol_map::{RoutePlaceholder, RouteSite, StringKeyKind};
use crate::virtual_members::laravel::helpers::{
    extract_string_literal_including_empty, resolved_string_literal,
};
use crate::yii::routes::{
    handler_key, is_route_argument_attribute, path_placeholders, route_argument_key,
};

const ROUTE: &str = "Yiisoft\\Router\\Route";
const GROUP: &str = "Yiisoft\\Router\\Group";

/// What the groups around a route add to it.
#[derive(Clone, Default)]
struct Prefixes {
    /// The name prefix, or `None` when one is not a literal.
    name: Option<String>,
    /// The path prefix, or `None` when one is not a literal.
    path: Option<String>,
    /// The placeholders the path prefixes spell out.
    placeholders: Vec<RoutePlaceholder>,
}

impl Prefixes {
    fn root() -> Self {
        Self {
            name: Some(String::new()),
            path: Some(String::new()),
            placeholders: Vec::new(),
        }
    }
}

/// One call of a chain: the method's name and its arguments.
type Link<'e, 'a> = (&'e str, &'e ArgumentList<'a>);

/// Whether the file may define routes: the project uses Yii's router, and
/// the file names something from it.  Checked once per file.
fn may_define_routes(ctx: &mut ExtractionCtx<'_>) -> bool {
    if !ctx.yii {
        return false;
    }
    *ctx.names_yii_router
        .get_or_insert_with(|| ctx.content.contains("Yiisoft\\Router"))
}

/// Read the routes `chain`, the outermost call of a method-call chain,
/// defines, when it is a `Route` or `Group` chain the walk has not read
/// yet as part of a group.
pub(in crate::symbol_map::extraction) fn record_yii_route_chain(
    chain: &Expression<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !may_define_routes(ctx) {
        return;
    }
    let span = chain.span();
    let (start, end) = (span.start.offset, span.end.offset);
    if ctx
        .yii_read_chains
        .iter()
        .any(|&(read_start, read_end)| read_start <= start && end <= read_end)
    {
        return;
    }
    if record_chain(chain, &Prefixes::root(), ctx) {
        ctx.yii_read_chains.push((start, end));
    }
}

/// Read the chain whose outermost call is `object`, under `prefixes`.
/// Whether it is a `Route` or `Group` chain.
fn record_chain(
    mut object: &Expression<'_>,
    prefixes: &Prefixes,
    ctx: &mut ExtractionCtx<'_>,
) -> bool {
    let mut links: Vec<Link<'_, '_>> = Vec::new();
    let root = loop {
        match object {
            Expression::Call(Call::Method(call)) => {
                let ClassLikeMemberSelector::Identifier(name) = &call.method else {
                    return false;
                };
                links.push((bytes_to_str(name.value), &call.argument_list));
                object = call.object;
            }
            Expression::Call(Call::StaticMethod(call)) => break call,
            _ => return false,
        }
    };
    let ClassLikeMemberSelector::Identifier(method) = &root.method else {
        return false;
    };
    let Expression::Identifier(class) = root.class else {
        return false;
    };
    let written = bytes_to_str(class.value());
    let resolved = ctx
        .resolved_name_at(class.span().start.offset)
        .unwrap_or(written)
        .trim_start_matches('\\');
    // The chain was gathered outermost first.
    links.reverse();
    let method = bytes_to_str(method.value);
    if resolved.eq_ignore_ascii_case(ROUTE) {
        record_route(method, &root.argument_list, &links, prefixes, ctx);
        true
    } else if resolved.eq_ignore_ascii_case(GROUP) && method.eq_ignore_ascii_case("create") {
        record_group(&root.argument_list, &links, prefixes, ctx);
        true
    } else {
        false
    }
}

/// The value of a positional argument, or of the named one.
fn argument<'e, 'a>(
    arguments: &'e ArgumentList<'a>,
    index: usize,
    name: &str,
) -> Option<&'e Expression<'a>> {
    arguments
        .arguments
        .iter()
        .enumerate()
        .find_map(|(position, argument)| match argument {
            Argument::Positional(positional) if position == index => Some(positional.value),
            Argument::Named(named) if bytes_to_str(named.name.value) == name => Some(named.value),
            _ => None,
        })
}

/// A path written as `value`: its text, and the placeholders in it at
/// their offsets in the file.  `None` when it is not a literal.
fn literal_path(
    value: &Expression<'_>,
    ctx: &ExtractionCtx<'_>,
) -> Option<(String, Vec<RoutePlaceholder>)> {
    let (raw, start, _) = extract_string_literal_including_empty(value, ctx.content)?;
    let path = match resolved_string_literal(value, ctx.content) {
        Some((decoded, _, _)) => decoded.to_string(),
        None => raw.to_string(),
    };
    let placeholders = path_placeholders(raw)
        .into_iter()
        .map(|(name_start, name_end)| RoutePlaceholder {
            name: raw[name_start..name_end].to_string(),
            start: (start + name_start) as u32,
            end: (start + name_end) as u32,
        })
        .collect();
    Some((path, placeholders))
}

fn record_group(
    create: &ArgumentList<'_>,
    links: &[Link<'_, '_>],
    outer: &Prefixes,
    ctx: &mut ExtractionCtx<'_>,
) {
    let mut prefixes = outer.clone();
    match argument(create, 0, "prefix") {
        None | Some(Expression::Literal(Literal::Null(_))) => {}
        Some(value) => match (literal_path(value, ctx), &mut prefixes.path) {
            (Some((path, placeholders)), Some(prefix)) => {
                prefix.push_str(&path);
                prefixes.placeholders.extend(placeholders);
            }
            _ => prefixes.path = None,
        },
    }
    for (method, arguments) in links {
        if method.eq_ignore_ascii_case("namePrefix") {
            match (
                argument(arguments, 0, "namePrefix")
                    .and_then(|value| extract_string_literal_including_empty(value, ctx.content)),
                &mut prefixes.name,
            ) {
                (Some((name, _, _)), Some(prefix)) => prefix.push_str(name),
                _ => prefixes.name = None,
            }
        }
    }
    for (method, arguments) in links {
        if method.eq_ignore_ascii_case("routes") {
            for entry in arguments.arguments.iter() {
                if let Argument::Positional(entry) = entry {
                    record_chain(entry.value, &prefixes, ctx);
                }
            }
        }
    }
}

fn record_route(
    method: &str,
    arguments: &ArgumentList<'_>,
    links: &[Link<'_, '_>],
    prefixes: &Prefixes,
    ctx: &mut ExtractionCtx<'_>,
) {
    let pattern = if method.eq_ignore_ascii_case("methods") {
        argument(arguments, 1, "pattern")
    } else if ["get", "post", "put", "delete", "patch", "head", "options"]
        .iter()
        .any(|verb| method.eq_ignore_ascii_case(verb))
    {
        argument(arguments, 0, "pattern")
    } else {
        return;
    };
    let mut pattern = pattern.map(|value| literal_path(value, ctx));
    let mut name = None;
    let mut handler = None;
    for (method, arguments) in links {
        if method.eq_ignore_ascii_case("pattern") {
            pattern = argument(arguments, 0, "pattern").map(|value| literal_path(value, ctx));
        } else if method.eq_ignore_ascii_case("name") {
            name = argument(arguments, 0, "name")
                .and_then(|value| resolved_string_literal(value, ctx.content));
        } else if method.eq_ignore_ascii_case("action") {
            handler = argument(arguments, 0, "middlewareDefinition")
                .and_then(|value| route_handler(value, ctx));
        }
    }

    let name_start = match (name, &prefixes.name) {
        (Some((name, start, end)), Some(prefix)) => {
            ctx.spans.push(SymbolSpan {
                start: start as u32,
                end: end as u32,
                kind: SymbolKind::StringKey {
                    kind: StringKeyKind::YiiRoute,
                    key: format!("{prefix}{name}"),
                    is_write: true,
                    is_optional: false,
                },
            });
            Some(start as u32)
        }
        _ => None,
    };
    if name_start.is_none() && handler.is_none() {
        return;
    }
    let mut placeholders = prefixes.placeholders.clone();
    let path = match (pattern.flatten(), &prefixes.path) {
        (Some((path, own)), Some(prefix)) => {
            placeholders.extend(own);
            Some(format!("{prefix}{path}"))
        }
        (Some((_, own)), None) => {
            placeholders.extend(own);
            None
        }
        _ => None,
    };
    ctx.routes.push(RouteSite {
        name_start,
        path,
        placeholders,
        handler,
    });
}

/// The handler an `action()` names: the method of a `[Controller::class,
/// 'method']` pair, or an action class's `__invoke()`.
fn route_handler(value: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Option<String> {
    let class_name = |value: &Expression<'_>| {
        let identifier = crate::parser::class_fetch_identifier(value)?;
        let written = bytes_to_str(identifier.value());
        Some(
            ctx.resolved_name_at(identifier.span().start.offset)
                .unwrap_or(written)
                .to_string(),
        )
    };
    let elements = match value {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return Some(handler_key(&class_name(value)?, "__invoke")),
    };
    let mut values = elements.iter().map(|element| match element {
        ArrayElement::Value(value) => Some(value.value),
        _ => None,
    });
    let (Some(Some(class)), Some(Some(method)), None) =
        (values.next(), values.next(), values.next())
    else {
        return None;
    };
    let (method, _, _) = resolved_string_literal(method, ctx.content)?;
    Some(handler_key(&class_name(class)?, method))
}

/// Emit the route arguments the methods of `class` read with
/// `#[RouteArgument('id')]`, each keyed by the method as a handler.
pub(in crate::symbol_map::extraction) fn emit_yii_route_argument_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.yii {
        return;
    }
    let mut class_name = None;
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        for parameter in method.parameter_list.parameters.iter() {
            for attribute in parameter
                .attribute_lists
                .iter()
                .flat_map(|list| list.attributes.iter())
            {
                let written = bytes_to_str(attribute.name.value());
                let resolved = ctx
                    .resolved_name_at(attribute.name.span().start.offset)
                    .unwrap_or(written);
                if !is_route_argument_attribute(resolved) {
                    continue;
                }
                let Some(value) = attribute.argument_list.as_ref().and_then(|arguments| {
                    arguments
                        .arguments
                        .iter()
                        .enumerate()
                        .find_map(|(index, argument)| match argument {
                            PartialArgument::Positional(positional) if index == 0 => {
                                Some(positional.value)
                            }
                            PartialArgument::Named(named)
                                if bytes_to_str(named.name.value) == "name" =>
                            {
                                Some(named.value)
                            }
                            _ => None,
                        })
                }) else {
                    continue;
                };
                let Some((name, start, end)) = resolved_string_literal(value, ctx.content) else {
                    continue;
                };
                let Some(class_name) = class_name.get_or_insert_with(|| {
                    ctx.resolved_name_at(class.name.span.start.offset)
                        .map(str::to_string)
                }) else {
                    return;
                };
                let handler = handler_key(class_name, bytes_to_str(method.name.value));
                ctx.spans.push(SymbolSpan {
                    start: start as u32,
                    end: end as u32,
                    kind: SymbolKind::StringKey {
                        kind: StringKeyKind::YiiRouteArgument,
                        key: route_argument_key(&handler, name),
                        is_write: false,
                        is_optional: false,
                    },
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolMap};

    fn map_of(php: &str) -> SymbolMap {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = crate::names::OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        crate::symbol_map::extraction::extract_symbol_map_for_index(
            program,
            php,
            &names,
            false,
            false,
            Some(std::path::Path::new("/p")),
        )
    }

    fn keys(map: &SymbolMap, kind: StringKeyKind, php: &str) -> Vec<(String, String, bool)> {
        map.spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: span_kind,
                    key,
                    is_write,
                    ..
                } if *span_kind == kind => Some((
                    key.clone(),
                    php[span.start as usize..span.end as usize].to_string(),
                    *is_write,
                )),
                _ => None,
            })
            .collect()
    }

    const ROUTES: &str = r#"<?php
use App\Web;
use App\Web\Blog\PostController;
use Yiisoft\Router\Group;
use Yiisoft\Router\Route;

return [
    Route::get('/')->action(Web\HomePage\Action::class)->name('home'),
    Group::create('/blog')
        ->namePrefix('blog/')
        ->routes(
            Route::get('[/{page}]')->action([PostController::class, 'index'])->name('index'),
            Group::create('/{slug}')->routes(
                Route::methods(['GET', 'POST'], '/edit')
                    ->name('edit')
                    ->action([PostController::class, 'edit']),
            ),
            Route::get($dynamic)->name('dynamic'),
        ),
    Group::create()->namePrefix($prefix)->routes(Route::get('/x')->name('unknowable')),
    Route::post('/unnamed/{id}')->action(Web\Unnamed::class),
    Other::get('/no')->name('not-a-route'),
];
"#;

    #[test]
    fn routes_are_declared_with_the_prefixes_of_their_groups() {
        let map = map_of(ROUTES);
        assert_eq!(
            keys(&map, StringKeyKind::YiiRoute, ROUTES),
            vec![
                ("home".to_string(), "home".to_string(), true),
                ("blog/index".to_string(), "index".to_string(), true),
                ("blog/edit".to_string(), "edit".to_string(), true),
                ("blog/dynamic".to_string(), "dynamic".to_string(), true),
            ]
        );
        let routes: Vec<(Option<&str>, Option<&str>, Vec<&str>)> = map
            .routes
            .iter()
            .map(|route| {
                (
                    route.path.as_deref(),
                    route.handler.as_deref(),
                    route
                        .placeholders
                        .iter()
                        .map(|placeholder| {
                            assert_eq!(
                                &ROUTES[placeholder.start as usize..placeholder.end as usize],
                                placeholder.name
                            );
                            placeholder.name.as_str()
                        })
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            routes,
            vec![
                (
                    Some("/"),
                    Some("app\\web\\homepage\\action::__invoke"),
                    vec![]
                ),
                (
                    Some("/blog[/{page}]"),
                    Some("app\\web\\blog\\postcontroller::index"),
                    vec!["page"]
                ),
                (
                    Some("/blog/{slug}/edit"),
                    Some("app\\web\\blog\\postcontroller::edit"),
                    vec!["slug"]
                ),
                (None, None, vec![]),
                (
                    Some("/unnamed/{id}"),
                    Some("app\\web\\unnamed::__invoke"),
                    vec!["id"]
                ),
            ]
        );
    }

    #[test]
    fn route_arguments_are_keyed_by_their_handler() {
        let php = r#"<?php
namespace App\Web;

use Yiisoft\Router\HydratorAttribute\RouteArgument;

final class Action
{
    public function __invoke(#[RouteArgument('id')] int $id, #[Other('x')] $x): void {}
}
"#;
        assert_eq!(
            keys(&map_of(php), StringKeyKind::YiiRouteArgument, php),
            vec![(
                "app\\web\\action::__invoke#id".to_string(),
                "id".to_string(),
                false
            )]
        );
    }

    #[test]
    fn url_generator_calls_are_recorded_for_the_type_engine() {
        let php = "<?php\n$urls->generate('home', ['id' => 1]);\n$urls->generateAbsolute('blog/index');\n$urls->generateFromCurrent([], [], null, 'home');\n";
        let map = map_of(php);
        let sites: Vec<&str> = map
            .typed_receiver_sites
            .iter()
            .map(|site| site.key.as_str())
            .collect();
        assert_eq!(sites, vec!["home", "blog/index", "home"]);
    }
}
