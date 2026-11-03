//! Named rate limiters discovered from registered providers and their helpers.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::names::OwnedResolvedNames;
use crate::text_position::offset_to_position;

use super::helpers::resolved_string_literal;

/// The facade's resolved name, shared by discovery and symbol extraction.
pub(crate) const RATE_LIMITER_FACADE: &str = "Illuminate\\Support\\Facades\\RateLimiter";

/// Whether a resolved constructor names Laravel's queue rate limiting middleware.
pub(crate) fn is_rate_limited_class(fqn: &str) -> bool {
    fqn.eq_ignore_ascii_case("Illuminate\\Queue\\Middleware\\RateLimited")
        || fqn.eq_ignore_ascii_case("Illuminate\\Queue\\Middleware\\RateLimitedWithRedis")
}

/// Whether a throttle payload can name a limiter rather than inline limits.
/// Multiple parameters select the numeric overload; `guest|authenticated`
/// is Laravel's older spelling for separate limits.
pub(crate) fn is_named_throttle_parameter(parameters: &str) -> bool {
    !parameters.contains([',', '|']) && !crate::php_type::is_php_numeric_string(parameters)
}

/// The first positional argument, or the argument named `name`, of a registration.
pub(crate) fn registration_name<'a>(args: &ArgumentList<'a>) -> Option<&'a Expression<'a>> {
    args.arguments.iter().find_map(|arg| match arg {
        Argument::Named(named) if named.name.value == b"name" => Some(named.value),
        Argument::Positional(positional) => Some(positional.value),
        _ => None,
    })
}

/// A cached snapshot of names and their declarations, sorted in the same order.
#[derive(Default)]
pub(crate) struct RateLimiterCatalog {
    /// Registered names, shared without copying on completion requests.
    pub names: Arc<[String]>,
    /// Whether all discovered registration sources were readable and literal.
    pub complete: bool,
    locations: Vec<Location>,
    sources: HashSet<String>,
}

impl RateLimiterCatalog {
    /// The declaration of a registered name.
    pub(crate) fn location(&self, name: &str) -> Option<&Location> {
        self.names
            .binary_search_by(|candidate| candidate.as_str().cmp(name))
            .ok()
            .map(|index| &self.locations[index])
    }

    /// Whether an edit changes a source this snapshot depends on.
    pub(crate) fn reads(&self, uri: &str) -> bool {
        self.sources.contains(uri)
    }
}

impl Backend {
    /// Provider-defined rate limiters, rebuilt only after a relevant edit.
    pub(crate) fn cached_rate_limiters(&self) -> Arc<RateLimiterCatalog> {
        self.cached_laravel_enumeration(
            &self.laravel.string_key_build_locks.rate_limiters,
            |cache| cache.rate_limiters.clone(),
            |cache, catalog| cache.rate_limiters = Some(catalog),
            || Arc::new(discover(self)),
        )
    }
}

fn discover(backend: &Backend) -> RateLimiterCatalog {
    let mut catalog = RateLimiterCatalog::default();
    let mut definitions = BTreeMap::new();
    let mut readable = false;
    let mut complete = true;
    let mut helpers = Vec::new();
    // Laravel's default `RateLimiter` root alias reaches the facade unless
    // the project declares a global class of that name.
    let global_alias = !backend.has_indexed_class("RateLimiter");
    for fqn in backend.laravel_providers().fqns() {
        let Some(uri) = backend.resolve_class_uri(fqn) else {
            complete = false;
            continue;
        };
        // A provider runs the methods it inherits from an application (or
        // its own package's) base class and the traits it uses, so those
        // files are read the same way as the provider itself.
        let mut provider_files = vec![uri.clone()];
        let mut ancestors = vec![fqn.to_string()];
        let mut seen_ancestors = HashSet::new();
        while let Some(class) = ancestors.pop() {
            if !seen_ancestors.insert(class.clone()) {
                continue;
            }
            let Some(info) = backend.find_or_load_class(&class) else {
                continue;
            };
            for ancestor in info.parent_class.iter().chain(&info.used_traits) {
                if let Some(ancestor_uri) = backend.resolve_class_uri(ancestor)
                    && backend.is_macro_helper_uri_allowed(&uri, &ancestor_uri)
                {
                    provider_files.push(ancestor_uri);
                    ancestors.push(ancestor.to_string());
                }
            }
        }
        for (index, file) in provider_files.into_iter().enumerate() {
            if !catalog.sources.insert(file.clone()) {
                continue;
            }
            let Some(content) = backend.get_file_content(&file) else {
                complete = false;
                continue;
            };
            if index == 0 {
                readable = true;
            }
            complete &= scan(&file, &content, global_alias, &mut definitions);
            // Match macro discovery: one level of helpers, restricted to the
            // application or the provider's own package, without a vendor walk.
            for helper in super::parse_provider_referenced_classes(&content) {
                if let Some(helper_uri) = backend.resolve_class_uri(&helper)
                    && backend.is_macro_helper_uri_allowed(&uri, &helper_uri)
                {
                    helpers.push(helper_uri);
                }
            }
        }
    }
    helpers.sort_unstable();
    for uri in helpers {
        if !catalog.sources.insert(uri.clone()) {
            continue;
        }
        if let Some(content) = backend.get_file_content(&uri) {
            complete &= scan(&uri, &content, global_alias, &mut definitions);
        } else {
            complete = false;
        }
    }
    // Limiters are also registered while the application boots
    // (`bootstrap/app.php`'s `booted()` and `withRouting(then:)` callbacks)
    // and from route files, which the route scan already knows.
    let mut boot_files = Vec::new();
    if let Some(root) = backend.workspace.workspace_root.read().clone() {
        boot_files.push(crate::util::path_to_uri(&root.join("bootstrap/app.php")));
    }
    boot_files.extend(
        backend
            .cached_routes()
            .sources
            .iter()
            .map(|path| crate::util::path_to_uri(path)),
    );
    for uri in boot_files {
        if catalog.sources.contains(&uri) {
            continue;
        }
        // A missing boot file registers nothing; it is still recorded so
        // that creating it drops the snapshot.
        if let Some(content) = backend.get_file_content(&uri) {
            complete &= scan(&uri, &content, global_alias, &mut definitions);
        }
        catalog.sources.insert(uri);
    }
    catalog.complete = readable && complete;
    let (names, locations): (Vec<_>, Vec<_>) = definitions.into_iter().unzip();
    catalog.names = names.into();
    catalog.locations = locations;
    catalog
}

/// Reject files without a `::for()` or `->for()` call before parsing,
/// including aliased facades, PHP's case-insensitive methods, and comments
/// between tokens.
pub(crate) fn may_register_rate_limiter(content: &str) -> bool {
    let bytes = content.as_bytes();
    memchr::memmem::find_iter(bytes, b"::")
        .chain(memchr::memmem::find_iter(bytes, b"->"))
        .any(|offset| {
            let start = crate::text_scan::skip_php_trivia(bytes, offset + 2);
            bytes
                .get(start..start + 3)
                .is_some_and(|name| name.eq_ignore_ascii_case(b"for"))
                && bytes.get(crate::text_scan::skip_php_trivia(bytes, start + 3)) == Some(&b'(')
        })
}

/// The callback a registration passes: the second positional argument or
/// the one named `callback`.
fn registration_callback<'a>(args: &ArgumentList<'a>) -> Option<&'a Expression<'a>> {
    args.arguments
        .iter()
        .filter(|arg| matches!(arg, Argument::Positional(_)))
        .nth(1)
        .or_else(|| {
            args.arguments.iter().find(
                |arg| matches!(arg, Argument::Named(named) if named.name.value == b"callback"),
            )
        })
        .map(|arg| arg.value())
}

/// Add literal declarations and report whether every registration was readable.
///
/// The `RateLimiter` facade, and the root alias when `global_alias` holds,
/// are recognized by name. A `->for()` call may be a limiter the container
/// resolved (`$limiter->for(...)` in a `boot(RateLimiter $limiter)`), so one
/// that names a literal and passes a closure counts as a registration, and
/// any other leaves the set open.
fn scan(
    uri: &str,
    content: &str,
    global_alias: bool,
    definitions: &mut BTreeMap<String, Location>,
) -> bool {
    if !may_register_rate_limiter(content) {
        return true;
    }
    crate::parser::with_parsed_program(content, "rate_limiters", |program, _| {
        let arena = mago_allocator::LocalArena::new();
        let resolved = NameResolver::new(&arena).resolve(program);
        let names = OwnedResolvedNames::from_resolved(&resolved);
        let mut complete = program.errors.is_empty();
        let mut pending = vec![Node::Program(program)];
        let is_for = |method: &ClassLikeMemberSelector<'_>| {
            matches!(method, ClassLikeMemberSelector::Identifier(method)
                if method.value.eq_ignore_ascii_case(b"for"))
        };
        while let Some(node) = pending.pop() {
            let registration = match node {
                Node::StaticMethodCall(call)
                    if is_for(&call.method)
                        && matches!(call.class, Expression::Identifier(class)
                        if names.get(class.span().start.offset).is_some_and(|name| {
                            name.eq_ignore_ascii_case(RATE_LIMITER_FACADE)
                                || (global_alias && name.eq_ignore_ascii_case("RateLimiter"))
                        })) =>
                {
                    Some((&call.argument_list, false))
                }
                Node::MethodCall(call) if is_for(&call.method) => Some((&call.argument_list, true)),
                Node::NullSafeMethodCall(call) if is_for(&call.method) => {
                    Some((&call.argument_list, true))
                }
                _ => None,
            };
            if let Some((arguments, needs_callback)) = registration {
                let callback_ok = !needs_callback
                    || matches!(
                        registration_callback(arguments),
                        Some(
                            Expression::Closure(_)
                                | Expression::ArrowFunction(_)
                                | Expression::PartialApplication(_)
                        )
                    );
                if callback_ok
                    && let Some((name, start, end)) = registration_name(arguments)
                        .and_then(|expr| resolved_string_literal(expr, content))
                    && let Some(uri) = crate::util::parse_url(uri)
                {
                    definitions.insert(
                        name.to_string(),
                        Location::new(
                            uri,
                            Range::new(
                                offset_to_position(content, start),
                                offset_to_position(content, end),
                            ),
                        ),
                    );
                } else {
                    complete = false;
                }
            }
            let children_start = pending.len();
            node.visit_children(|child| pending.push(child));
            pending[children_start..].reverse();
        }
        complete
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiters_prefilter_recognizes_call_tokens() {
        for source in [
            "Limits::for('api', fn () => null);",
            "Limits :: /* method */ FOR /* args */ ('api', fn () => null);",
            "Limits:: // comment\n for ('api', fn () => null);",
            "$limiter->for('api', fn () => null);",
            "$limiter -> FOR ('api', fn () => null);",
        ] {
            assert!(may_register_rate_limiter(source), "{source}");
        }
        for source in [
            "",
            "for (;;) {}",
            "Limits::forget('api');",
            "Limits::for;",
            "Limits::",
            "Limits:: /* unfinished",
            "$factory->forUser($user);",
        ] {
            assert!(!may_register_rate_limiter(source), "{source}");
        }
    }

    #[test]
    fn rate_limiters_throttle_parameters_distinguish_names_and_limits() {
        for parameter in [
            "api",
            "uploads-v2",
            "",
            "NaN",
            "inf",
            "0x60",
            "1_000",
            "100uploads",
        ] {
            assert!(is_named_throttle_parameter(parameter), "{parameter}");
        }
        for parameter in [
            "60",
            "60,1",
            "60,1,prefix",
            "10|60",
            "60|member_limit",
            "5e2",
            " +10 ",
            ".5",
            "5.",
            "-1",
        ] {
            assert!(!is_named_throttle_parameter(parameter), "{parameter}");
        }
    }

    #[test]
    fn rate_limiters_scan_resolves_names_in_each_namespace() {
        let source = r#"<?php
namespace One {
    use Illuminate\Support\Facades\RateLimiter as Limits;
    Limits::for('api', fn () => null);
    Limits::for(callback: fn () => null, name: 'uploads');
}
namespace Two {
    use Illuminate\Support\Facades as Framework;
    Framework\RateLimiter::FOR('backup', fn () => null);
    RateLimiter::for('local', fn () => null);
    \RateLimiter::for('global', fn () => null);
    \Illuminate\Support\Facades\RateLimiter::for('direct', fn () => null);
    $limiter::for('dynamic-class', fn () => null);
    Framework\RateLimiter::{$method}('dynamic-method', fn () => null);
}
"#;
        let mut definitions = BTreeMap::new();
        assert!(scan("file:///provider.php", source, true, &mut definitions));
        assert_eq!(
            definitions.keys().map(String::as_str).collect::<Vec<_>>(),
            ["api", "backup", "direct", "global", "uploads"]
        );
        assert_eq!(definitions["api"].range.start.line, 3);
        // A project-declared global `RateLimiter` class shadows the alias.
        let mut definitions = BTreeMap::new();
        assert!(scan(
            "file:///provider.php",
            source,
            false,
            &mut definitions
        ));
        assert!(!definitions.contains_key("global"));
    }

    #[test]
    fn rate_limiters_scan_instance_registrations() {
        let source = r#"<?php
$limiter->for('injected', fn () => null);
app(RateLimiter::class)?->for(name: 'nullsafe', callback: function () {});
$limiter->for('method', $this->limit(...));
"#;
        let mut definitions = BTreeMap::new();
        assert!(scan("file:///provider.php", source, true, &mut definitions));
        assert_eq!(
            definitions.keys().map(String::as_str).collect::<Vec<_>>(),
            ["injected", "method", "nullsafe"]
        );
        for source in [
            "<?php $factory->for($user);",
            "<?php $factory->for('user', $state);",
            "<?php $limiter->for($name, fn () => null);",
        ] {
            let mut definitions = BTreeMap::new();
            assert!(
                !scan("file:///provider.php", source, true, &mut definitions),
                "{source}"
            );
            assert!(definitions.is_empty(), "{source}");
        }
    }

    #[test]
    fn rate_limiters_unknown_registrations_and_invalid_source_are_incomplete() {
        for arguments in [
            "$name, fn () => null",
            "'api' . $suffix, fn () => null",
            "name: $name",
            "callback: fn () => null",
            "",
            "'', fn () => null",
        ] {
            let source =
                format!("<?php \\Illuminate\\Support\\Facades\\RateLimiter::for({arguments});");
            let mut definitions = BTreeMap::new();
            assert!(
                !scan("file:///provider.php", &source, true, &mut definitions),
                "{source}"
            );
            assert!(definitions.is_empty());
        }
        let mut definitions = BTreeMap::new();
        assert!(!scan(
            "file:///provider.php",
            "<?php \\Illuminate\\Support\\Facades\\RateLimiter::for('api', fn () => null); broken syntax",
            true,
            &mut definitions
        ));
        assert!(!scan(
            "not a URI",
            "<?php \\Illuminate\\Support\\Facades\\RateLimiter::for('api', fn () => null);",
            true,
            &mut definitions
        ));
        assert!(scan(
            "file:///empty.php",
            "<?php class EmptyProvider {}",
            true,
            &mut definitions
        ));
    }
}
