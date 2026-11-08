//! The Twig filters and functions a PHP file declares: a
//! `new TwigFilter('price', [$this, 'formatPrice'])` or `new TwigFunction(…)`
//! anywhere (an extension's `getFilters()`, or an `addFilter()` on the
//! environment), and an `#[AsTwigFilter('price')]` or `#[AsTwigFunction]`
//! method.
//!
//! Each declares its name with a [`StringKey`](SymbolKind::StringKey) span,
//! and records the callable behind it as a [`TwigCallableSite`].

use super::*;
use crate::symbol_map::{StringKeyKind, TwigCallableSite, TwigCallableTarget};
use crate::virtual_members::laravel::helpers::resolved_string_literal;

const TWIG_FILTER: &str = "Twig\\TwigFilter";
const TWIG_FUNCTION: &str = "Twig\\TwigFunction";
const AS_TWIG_FILTER: &str = "Twig\\Attribute\\AsTwigFilter";
const AS_TWIG_FUNCTION: &str = "Twig\\Attribute\\AsTwigFunction";

/// Whether `name` is one a Twig callable can be declared under: a name, or
/// a pattern of one whose `*` stands for any part (`*_path`).
fn is_callable_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'*'))
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'*'))
}

/// The class whose members are being extracted, fully qualified.
fn enclosing_class<'a>(ctx: &ExtractionCtx<'a>) -> Option<&'a str> {
    ctx.class_name_offset
        .and_then(|offset| ctx.resolved_name_at(offset))
        .map(|fqn| fqn.trim_start_matches('\\'))
}

/// Record the filter or function a `new TwigFilter(…)` or
/// `new TwigFunction(…)` declares.
pub(in crate::symbol_map::extraction) fn emit_twig_callable_instantiation(
    class: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.twig {
        return;
    }
    let Expression::Identifier(identifier) = class else {
        return;
    };
    let written = bytes_to_str(identifier.value());
    let resolved = ctx
        .resolved_name_at(identifier.span().start.offset)
        .unwrap_or(written)
        .trim_start_matches('\\');
    let kind = if resolved.eq_ignore_ascii_case(TWIG_FILTER) {
        StringKeyKind::TwigFilter
    } else if resolved.eq_ignore_ascii_case(TWIG_FUNCTION) {
        StringKeyKind::TwigFunction
    } else {
        return;
    };
    let (mut name, mut callable) = (None, None);
    for (index, argument) in argument_list.arguments.iter().enumerate() {
        let (parameter, value) = match argument {
            Argument::Positional(positional) => match index {
                0 => ("name", positional.value),
                1 => ("callable", positional.value),
                _ => continue,
            },
            Argument::Named(named) => (bytes_to_str(named.name.value), named.value),
        };
        match parameter {
            "name" => name = Some(value),
            "callable" => callable = Some(value),
            _ => {}
        }
    }
    let target = callable.map_or(TwigCallableTarget::Inline, |callable| {
        callable_target(callable, ctx)
    });
    if let Some(name) = name {
        push_declaration(kind, name, target, ctx);
    }
}

/// Record the filters and functions a class's `#[AsTwigFilter]` and
/// `#[AsTwigFunction]` methods declare.
pub(in crate::symbol_map::extraction) fn emit_twig_callable_attributes(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.twig {
        return;
    }
    let Some(owner) = ctx
        .resolved_name_at(class.name.span.start.offset)
        .map(|fqn| crate::atom::atom(fqn.trim_start_matches('\\')))
    else {
        return;
    };
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        for (attribute, kind) in [
            (AS_TWIG_FILTER, StringKeyKind::TwigFilter),
            (AS_TWIG_FUNCTION, StringKeyKind::TwigFunction),
        ] {
            let names: Vec<(String, usize, usize)> =
                attributes_named(&method.attribute_lists, attribute, ctx)
                    .flat_map(|attribute| attribute_arguments(attribute, &["name"]))
                    .filter(|(parameter, _)| *parameter == "name")
                    .filter_map(|(_, value)| resolved_string_literal(value, ctx.content))
                    .map(|(name, start, end)| (name.to_string(), start, end))
                    .collect();
            for (name, start, end) in names {
                let target = TwigCallableTarget::Method {
                    class: owner,
                    method: crate::atom::atom_bytes(method.name.value),
                };
                push_named(kind, &name, start, end, target, ctx);
            }
        }
    }
}

/// Declare the name `name` spells, unless the file already declares one of
/// the kind by that name: an extension that builds a stand-in callable to
/// compile a call with (`new TwigFunction('block', fn … )` in Twig's own)
/// does not declare it again.
fn push_declaration(
    kind: StringKeyKind,
    name: &Expression<'_>,
    target: TwigCallableTarget,
    ctx: &mut ExtractionCtx<'_>,
) {
    if let Some((name, start, end)) = resolved_string_literal(name, ctx.content) {
        push_named(kind, name, start, end, target, ctx);
    }
}

fn push_named(
    kind: StringKeyKind,
    name: &str,
    start: usize,
    end: usize,
    target: TwigCallableTarget,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !is_callable_name(name)
        || ctx
            .twig_callables
            .iter()
            .any(|site| site.kind == kind && site.name == name)
    {
        return;
    }
    ctx.spans.push(SymbolSpan {
        start: start as u32,
        end: end as u32,
        kind: SymbolKind::StringKey {
            kind,
            key: name.to_string(),
            is_write: true,
            is_optional: false,
        },
    });
    ctx.twig_callables.push(TwigCallableSite {
        kind,
        name: crate::atom::atom(name),
        start: start as u32,
        end: end as u32,
        target,
    });
}

/// What a callable expression runs: `[$this, 'method']`,
/// `[Foo::class, 'method']`, `'Foo::method'`, `'function'`, or a first-class
/// callable.
fn callable_target(callable: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> TwigCallableTarget {
    let method = |class: Option<String>, method: &str| match class {
        Some(class) if !method.is_empty() => TwigCallableTarget::Method {
            class: crate::atom::atom(&class),
            method: crate::atom::atom(method),
        },
        _ => TwigCallableTarget::Inline,
    };
    match callable {
        Expression::Literal(Literal::String(_)) => {
            let Some((text, _, _)) = resolved_string_literal(callable, ctx.content) else {
                return TwigCallableTarget::Inline;
            };
            let text = text.trim_start_matches('\\');
            match text.split_once("::") {
                Some((class, name)) => method(Some(class.to_string()), name),
                None if !text.is_empty() => TwigCallableTarget::Function(crate::atom::atom(text)),
                None => TwigCallableTarget::Inline,
            }
        }
        Expression::Array(_) | Expression::LegacyArray(_) => {
            let Some(elements) = crate::parser::array_literal_elements(callable) else {
                return TwigCallableTarget::Inline;
            };
            let mut values = elements.iter().filter_map(|element| match element {
                ArrayElement::Value(value) => Some(value.value),
                _ => None,
            });
            let (Some(object), Some(name), None) = (values.next(), values.next(), values.next())
            else {
                return TwigCallableTarget::Inline;
            };
            let Some((name, _, _)) = resolved_string_literal(name, ctx.content) else {
                return TwigCallableTarget::Inline;
            };
            method(class_of(object, ctx), name)
        }
        Expression::PartialApplication(partial) => match partial {
            PartialApplication::Function(function) => match function.function {
                Expression::Identifier(identifier) => {
                    let written = bytes_to_str(identifier.value());
                    let resolved = ctx
                        .resolved_name_at(identifier.span().start.offset)
                        .unwrap_or(written);
                    TwigCallableTarget::Function(crate::atom::atom(
                        resolved.trim_start_matches('\\'),
                    ))
                }
                _ => TwigCallableTarget::Inline,
            },
            PartialApplication::Method(call) => match &call.method {
                ClassLikeMemberSelector::Identifier(name) if is_this(call.object) => method(
                    enclosing_class(ctx).map(str::to_string),
                    bytes_to_str(name.value),
                ),
                _ => TwigCallableTarget::Inline,
            },
            PartialApplication::StaticMethod(call) => match &call.method {
                ClassLikeMemberSelector::Identifier(name) => {
                    method(class_named(call.class, ctx), bytes_to_str(name.value))
                }
                _ => TwigCallableTarget::Inline,
            },
        },
        _ => TwigCallableTarget::Inline,
    }
}

fn is_this(expression: &Expression<'_>) -> bool {
    matches!(expression, Expression::Variable(Variable::Direct(variable)) if variable.name == b"$this")
}

/// The class the first element of an array callable calls: `$this`, or the
/// class a `Foo::class` or `self::class` fetches.
fn class_of(object: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Option<String> {
    if is_this(object) {
        return enclosing_class(ctx).map(str::to_string);
    }
    if let Some(class) = crate::parser::class_fetch_target(object) {
        return class_named(class, ctx);
    }
    let (text, _, _) = resolved_string_literal(object, ctx.content)?;
    Some(text.trim_start_matches('\\').to_string())
}

/// The class a class expression names: `self` or `static`, or a name.
fn class_named(class: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Option<String> {
    match class {
        Expression::Self_(_) | Expression::Static(_) => enclosing_class(ctx).map(str::to_string),
        Expression::Identifier(identifier) => {
            let written = bytes_to_str(identifier.value());
            let resolved = ctx
                .resolved_name_at(identifier.span().start.offset)
                .unwrap_or(written);
            Some(resolved.trim_start_matches('\\').to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_map::{KeyFramework, SymbolMap};

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
            crate::symbol_map::extraction::Frameworks {
                symfony: false,
                service_container: false,
                drupal: false,
                twig: true,
                doctrine: false,
                yii_params: false,
            },
            None,
        )
    }

    fn report(map: &SymbolMap, php: &str) -> Vec<String> {
        map.twig_callables
            .iter()
            .map(|site| {
                assert_eq!(&php[site.start as usize..site.end as usize], &*site.name);
                let kind = match site.kind {
                    StringKeyKind::TwigFilter => "filter",
                    _ => "function",
                };
                let target = match &site.target {
                    TwigCallableTarget::Method { class, method } => format!("{class}::{method}"),
                    TwigCallableTarget::Function(function) => format!("{function}()"),
                    TwigCallableTarget::Inline => "inline".to_string(),
                };
                format!("{kind} {} {target}", site.name)
            })
            .collect()
    }

    #[test]
    fn extensions_declare_filters_and_functions_with_their_callables() {
        let php = r#"<?php
namespace App\Twig;
use Twig\Extension\AbstractExtension;
use Twig\TwigFilter;
use Twig\TwigFunction;
use App\Twig\Runtime\IconRuntime;
final class AppExtension extends AbstractExtension {
    public function getFilters(): array {
        return [
            new TwigFilter('price', [$this, 'formatPrice']),
            new TwigFilter('upper', [self::class, 'upper'], ['needs_charset' => true]),
            new TwigFilter('abs', 'abs'),
            new TwigFilter('raw', null, ['is_safe' => ['all']]),
            new TwigFilter('slug', $this->slug(...)),
            new TwigFilter(name: 'md', callable: 'App\Markdown::render'),
            new TwigFilter('*_case', fn (string $case, string $value) => $value),
            new TwigFilter($dynamic, 'x'),
        ];
    }
    public function getFunctions(): array {
        return [new TwigFunction('icon', [IconRuntime::class, 'render']), new TwigFunction('now', now(...))];
    }
    public function compile(): void {
        new TwigFunction('icon', fn () => null);
    }
}
"#;
        let map = map_of(php);
        assert_eq!(
            report(&map, php),
            [
                "filter price App\\Twig\\AppExtension::formatPrice",
                "filter upper App\\Twig\\AppExtension::upper",
                "filter abs abs()",
                "filter raw inline",
                "filter slug App\\Twig\\AppExtension::slug",
                "filter md App\\Markdown::render",
                "filter *_case inline",
                "function icon App\\Twig\\Runtime\\IconRuntime::render",
                "function now App\\Twig\\now()",
            ]
        );
        let declared = map
            .spans
            .iter()
            .filter(|span| {
                matches!(
                    &span.kind,
                    SymbolKind::StringKey { kind, is_write: true, .. }
                        if kind.framework() == KeyFramework::Twig
                )
            })
            .count();
        assert_eq!(declared, 9);
    }

    #[test]
    fn attributes_declare_their_methods() {
        let php = r#"<?php
namespace App\Twig;
use Twig\Attribute\AsTwigFilter;
use Twig\Attribute\AsTwigFunction;
final class Formatting {
    #[AsTwigFilter('price')]
    public function formatPrice(float $amount): string { return ''; }
    #[AsTwigFunction(name: 'icon', isSafe: ['html'])]
    public static function icon(string $name): string { return ''; }
    #[\Twig\Attribute\AsTwigFilter('shout'), AsTwigFunction('shout')]
    public function shout(string $text): string { return ''; }
}
"#;
        let map = map_of(php);
        assert_eq!(
            report(&map, php),
            [
                "filter price App\\Twig\\Formatting::formatPrice",
                "function icon App\\Twig\\Formatting::icon",
                "filter shout App\\Twig\\Formatting::shout",
                "function shout App\\Twig\\Formatting::shout",
            ]
        );
    }

    #[test]
    fn other_classes_named_alike_declare_nothing() {
        let php = "<?php\nnamespace App;\nclass TwigFilter {}\nnew TwigFilter('price', 'x');\n";
        assert!(map_of(php).twig_callables.is_empty());
    }
}
