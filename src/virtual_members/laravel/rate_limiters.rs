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
    for fqn in backend.laravel_providers().fqns() {
        let Some(uri) = backend.resolve_class_uri(fqn) else {
            complete = false;
            continue;
        };
        if !catalog.sources.insert(uri.clone()) {
            continue;
        }
        let Some(content) = backend.get_file_content(&uri) else {
            complete = false;
            continue;
        };
        readable = true;
        complete &= scan(&uri, &content, &mut definitions);
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
    helpers.sort_unstable();
    for uri in helpers {
        if !catalog.sources.insert(uri.clone()) {
            continue;
        }
        if let Some(content) = backend.get_file_content(&uri) {
            complete &= scan(&uri, &content, &mut definitions);
        } else {
            complete = false;
        }
    }
    catalog.complete = readable && complete;
    let (names, locations): (Vec<_>, Vec<_>) = definitions.into_iter().unzip();
    catalog.names = names.into();
    catalog.locations = locations;
    catalog
}

/// Reject files without a `::for()` call before parsing, including aliased
/// facades, PHP's case-insensitive methods, and comments between tokens.
pub(crate) fn may_register_rate_limiter(content: &str) -> bool {
    let bytes = content.as_bytes();
    memchr::memmem::find_iter(bytes, b"::").any(|offset| {
        let start = crate::text_scan::skip_php_trivia(bytes, offset + 2);
        bytes
            .get(start..start + 3)
            .is_some_and(|name| name.eq_ignore_ascii_case(b"for"))
            && bytes.get(crate::text_scan::skip_php_trivia(bytes, start + 3)) == Some(&b'(')
    })
}

/// Add literal declarations and report whether every registration was readable.
fn scan(uri: &str, content: &str, definitions: &mut BTreeMap<String, Location>) -> bool {
    if !may_register_rate_limiter(content) {
        return true;
    }
    crate::parser::with_parsed_program(content, "rate_limiters", |program, _| {
        let arena = mago_allocator::LocalArena::new();
        let resolved = NameResolver::new(&arena).resolve(program);
        let names = OwnedResolvedNames::from_resolved(&resolved);
        let mut complete = program.errors.is_empty();
        let mut pending = vec![Node::Program(program)];
        while let Some(node) = pending.pop() {
            if let Node::StaticMethodCall(call) = node
                && let Expression::Identifier(class) = call.class
                && let ClassLikeMemberSelector::Identifier(method) = &call.method
                && method.value.eq_ignore_ascii_case(b"for")
                && names
                    .get(class.span().start.offset)
                    .is_some_and(|name| name.eq_ignore_ascii_case(RATE_LIMITER_FACADE))
            {
                if let Some((name, start, end)) = registration_name(&call.argument_list)
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
        assert!(scan("file:///provider.php", source, &mut definitions));
        assert_eq!(
            definitions.keys().map(String::as_str).collect::<Vec<_>>(),
            ["api", "backup", "direct", "uploads"]
        );
        assert_eq!(definitions["api"].range.start.line, 3);
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
                !scan("file:///provider.php", &source, &mut definitions),
                "{source}"
            );
            assert!(definitions.is_empty());
        }
        let mut definitions = BTreeMap::new();
        assert!(!scan(
            "file:///provider.php",
            "<?php \\Illuminate\\Support\\Facades\\RateLimiter::for('api', fn () => null); broken syntax",
            &mut definitions
        ));
        assert!(!scan(
            "not a URI",
            "<?php \\Illuminate\\Support\\Facades\\RateLimiter::for('api', fn () => null);",
            &mut definitions
        ));
        assert!(scan(
            "file:///empty.php",
            "<?php class EmptyProvider {}",
            &mut definitions
        ));
    }
}
