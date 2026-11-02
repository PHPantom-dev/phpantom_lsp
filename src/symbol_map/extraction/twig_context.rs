//! The variables a PHP file hands Twig templates, and the globals it
//! declares for every template.
//!
//! A render's context is recorded as a [`TwigContextSite`] keyed by the
//! template name it renders: the keys of a `render('…', ['post' => $post])`
//! array or a `compact('post')`, the `context()` of a templated email that
//! names its templates in the same chain, and what a `#[Template]` action
//! returns, or else the arguments it is called with.
//!
//! A global is declared with a [`TwigVariable`](StringKeyKind::TwigVariable)
//! span: the name an `addGlobal()` on the environment takes (a typed
//! receiver site, since only the receiver's type tells), and the keys of an
//! extension's `getGlobals()`.

use super::*;
use crate::symbol_map::{
    ReceiverClass, StringKeyKind, TwigContextSite, TwigContextValue, TwigContextVar,
};
use crate::twig::variables::is_twig_variable_name;
use crate::virtual_members::laravel::helpers::resolved_string_literal;

const GLOBALS_INTERFACE: &str = "Twig\\Extension\\GlobalsInterface";

/// The positional index and the parameter names of the context a
/// `member_name` call on a template renderer takes.
fn context_argument(member_name: &str) -> Option<(usize, &'static [&'static str])> {
    let is = |name: &str| member_name.eq_ignore_ascii_case(name);
    if is("render") || is("renderView") || is("stream") || is("display") {
        Some((1, &["parameters", "context"]))
    } else if is("renderBlock") || is("renderBlockView") {
        Some((2, &["parameters", "context"]))
    } else {
        None
    }
}

/// The argument a call passes at `index` or by one of `names`.
fn argument_at<'e, 'a>(
    argument_list: &'e ArgumentList<'a>,
    index: usize,
    names: &[&str],
) -> Option<&'e Expression<'a>> {
    argument_list
        .arguments
        .iter()
        .enumerate()
        .find_map(|(position, argument)| match argument {
            Argument::Positional(positional) if position == index => Some(positional.value),
            Argument::Named(named) if names.contains(&bytes_to_str(named.name.value)) => {
                Some(named.value)
            }
            _ => None,
        })
}

/// What a context expression hands a template: the string keys of an array
/// literal with their values, the names a `compact()` copies, or else the
/// expression itself, whose type alone tells its names.
struct Context {
    vars: Vec<TwigContextVar>,
    shape: Option<(u32, u32)>,
}

fn context_of(expression: &Expression<'_>, ctx: &ExtractionCtx<'_>) -> Context {
    let mut vars = Vec::new();
    let mut push = |text: &str, start: usize, end: usize, value: TwigContextValue| {
        if is_twig_variable_name(text) {
            vars.push(TwigContextVar {
                name: crate::atom::atom(text),
                start: start as u32,
                end: end as u32,
                value,
            });
        }
    };
    if let Some(elements) = crate::parser::array_literal_elements(expression) {
        for element in elements.iter() {
            if let ArrayElement::KeyValue(entry) = element
                && let Some((key, start, end)) = resolved_string_literal(entry.key, ctx.content)
            {
                let span = entry.value.span();
                let value = TwigContextValue::Expression {
                    start: span.start.offset,
                    end: span.end.offset,
                };
                push(key, start, end, value);
            }
        }
    } else if let Expression::Call(Call::Function(call)) = expression
        && let Expression::Identifier(identifier) = call.function
        && bytes_to_str(identifier.value())
            .trim_start_matches('\\')
            .eq_ignore_ascii_case("compact")
    {
        let value = TwigContextValue::Variable {
            offset: call.span().start.offset,
        };
        for argument in call.argument_list.arguments.iter() {
            if let Argument::Positional(positional) = argument
                && let Some((name, start, end)) =
                    resolved_string_literal(positional.value, ctx.content)
            {
                push(name, start, end, value);
            }
        }
    } else if !matches!(expression, Expression::Literal(_)) {
        let span = expression.span();
        return Context {
            vars,
            shape: Some((span.start.offset, span.end.offset)),
        };
    }
    Context { vars, shape: None }
}

fn push_context(template_start: usize, context: Context, ctx: &mut ExtractionCtx<'_>) {
    if !context.vars.is_empty() || context.shape.is_some() {
        ctx.twig_contexts.push(TwigContextSite {
            template_start: template_start as u32,
            vars: context.vars,
            shape: context.shape,
        });
    }
}

/// The method calls of the chain `expression` ends with, outermost first.
fn chain_calls<'e, 'a>(mut expression: &'e Expression<'a>) -> Vec<&'e MethodCall<'a>> {
    let mut calls = Vec::new();
    loop {
        match expression {
            Expression::Call(Call::Method(call)) => {
                calls.push(call);
                expression = call.object;
            }
            Expression::Parenthesized(inner) => expression = inner.expression,
            _ => return calls,
        }
    }
}

fn method_named(call: &MethodCall<'_>, name: &str) -> bool {
    matches!(&call.method, ClassLikeMemberSelector::Identifier(method) if bytes_to_str(method.value).eq_ignore_ascii_case(name))
}

/// Record the context a call on a template renderer hands the template
/// whose name starts at `template_start`: the context argument of a
/// `render()`, or for a templated email's `htmlTemplate()` and
/// `textTemplate()` the `context()` earlier in the same chain.
pub(in crate::symbol_map::extraction) fn record_twig_render_context(
    receiver: ReceiverClass,
    object: &Expression<'_>,
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    template_start: usize,
    ctx: &mut ExtractionCtx<'_>,
) {
    let contexts: Vec<Context> = match receiver {
        ReceiverClass::TemplateRenderer => {
            let Some((index, names)) = context_argument(member_name) else {
                return;
            };
            match argument_at(argument_list, index, names) {
                Some(context) => vec![context_of(context, ctx)],
                None => return,
            }
        }
        ReceiverClass::TemplatedEmail => chain_calls(object)
            .into_iter()
            .filter(|call| method_named(call, "context"))
            .filter_map(|call| argument_at(&call.argument_list, 0, &["context"]))
            .map(|context| context_of(context, ctx))
            .collect(),
        _ => return,
    };
    for context in contexts {
        push_context(template_start, context, ctx);
    }
}

/// Record the context a templated email's `context([…])` hands the
/// templates its chain named before it: `->htmlTemplate('…')->context([…])`.
pub(in crate::symbol_map::extraction) fn record_twig_email_context(
    object: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.twig {
        return;
    }
    let Some(context) = argument_at(argument_list, 0, &["context"]) else {
        return;
    };
    let templates: Vec<usize> = chain_calls(object)
        .into_iter()
        .filter(|call| method_named(call, "htmlTemplate") || method_named(call, "textTemplate"))
        .filter_map(|call| argument_at(&call.argument_list, 0, &["template"]))
        .filter_map(|name| resolved_string_literal(name, ctx.content).map(|(_, start, _)| start))
        .collect();
    if templates.is_empty() {
        return;
    }
    for template_start in templates {
        let context = context_of(context, ctx);
        push_context(template_start, context, ctx);
    }
}

/// Record the context a `#[Template]` action hands its template: the keys
/// of the arrays it returns, or, when it returns none, its arguments (those
/// the attribute's `vars` names, when it names any).
pub(in crate::symbol_map::extraction) fn record_template_action_context(
    method: &Method<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.symfony {
        return;
    }
    let Some(attribute) = attributes_named(&method.attribute_lists, TEMPLATE_ATTRIBUTE, ctx).next()
    else {
        return;
    };
    let arguments = attribute_arguments(attribute, &["template", "vars", "stream", "block"]);
    let Some(template_start) = arguments
        .iter()
        .find(|(parameter, _)| *parameter == "template")
        .and_then(|(_, value)| resolved_string_literal(value, ctx.content))
        .map(|(_, start, _)| start)
    else {
        return;
    };
    let MethodBody::Concrete(body) = &method.body else {
        return;
    };
    let mut returns = Vec::new();
    crate::return_collection::collect_returns(body.statements.iter(), &mut returns);
    let values: Vec<&Expression<'_>> = returns
        .iter()
        .filter_map(|(value, ..)| *value)
        .filter(|value| !matches!(value, Expression::Literal(Literal::Null(_))))
        .collect();
    // Symfony renders the arguments only when the action returns nothing.
    if !values.is_empty() {
        for value in values {
            let context = context_of(value, ctx);
            push_context(template_start, context, ctx);
        }
        return;
    }
    // An argument is read as it stands where the body starts.
    let argument = TwigContextValue::Variable {
        offset: body.left_brace.end.offset,
    };
    let named = arguments
        .iter()
        .find(|(parameter, _)| *parameter == "vars")
        .map(|(_, value)| *value);
    let vars = match named {
        Some(list) => crate::parser::array_literal_elements(list)
            .into_iter()
            .flat_map(|elements| elements.iter())
            .filter_map(|element| match element {
                ArrayElement::Value(value) => resolved_string_literal(value.value, ctx.content),
                _ => None,
            })
            .filter(|(name, _, _)| is_twig_variable_name(name))
            .map(|(name, start, end)| TwigContextVar {
                name: crate::atom::atom(name),
                start: start as u32,
                end: end as u32,
                value: argument,
            })
            .collect(),
        None => method
            .parameter_list
            .parameters
            .iter()
            .map(|parameter| {
                let span = parameter.variable.span;
                TwigContextVar {
                    name: crate::atom::atom(
                        bytes_to_str(parameter.variable.name).trim_start_matches('$'),
                    ),
                    // The name, without its `$`.
                    start: span.start.offset + 1,
                    end: span.end.offset,
                    value: argument,
                }
            })
            .collect(),
    };
    push_context(template_start, Context { vars, shape: None }, ctx);
}

/// Declare the globals an extension's `getGlobals()` returns the keys of.
pub(in crate::symbol_map::extraction) fn emit_twig_global_declarations(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.twig {
        return;
    }
    let implements_globals = class.implements.as_ref().is_some_and(|implements| {
        implements.types.iter().any(|identifier| {
            ctx.resolved_name_at(identifier.span().start.offset)
                .unwrap_or(bytes_to_str(identifier.value()))
                .trim_start_matches('\\')
                .eq_ignore_ascii_case(GLOBALS_INTERFACE)
        })
    });
    if !implements_globals {
        return;
    }
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        if !method.name.value.eq_ignore_ascii_case(b"getGlobals") {
            continue;
        }
        let MethodBody::Concrete(body) = &method.body else {
            continue;
        };
        let mut returns = Vec::new();
        crate::return_collection::collect_returns(body.statements.iter(), &mut returns);
        for value in returns.iter().filter_map(|(value, ..)| *value) {
            for var in context_of(value, ctx).vars {
                ctx.spans.push(SymbolSpan {
                    start: var.start,
                    end: var.end,
                    kind: SymbolKind::StringKey {
                        kind: StringKeyKind::TwigVariable,
                        key: var.name.to_string(),
                        is_write: true,
                        is_optional: false,
                    },
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::symbol_map::{SymbolKind, SymbolMap};

    fn map_of(php: &str, symfony: bool) -> SymbolMap {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = crate::names::OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        crate::symbol_map::extract_symbol_map_for_index(program, php, &names, symfony, true, None)
    }

    /// Each context as the template it renders, then the names it passes.
    fn contexts(map: &SymbolMap, php: &str) -> Vec<String> {
        map.twig_contexts
            .iter()
            .map(|site| {
                let start = site.template_start as usize;
                let end = start + php[start..].find(['\'', '"']).unwrap();
                let names: Vec<String> = site
                    .vars
                    .iter()
                    .map(|var| {
                        assert_eq!(&php[var.start as usize..var.end as usize], &*var.name);
                        var.name.to_string()
                    })
                    .collect();
                let shape = site.shape.map_or(String::new(), |(start, end)| {
                    format!(" + {}", &php[start as usize..end as usize])
                });
                format!("{}: {}{shape}", &php[start..end], names.join(", "))
            })
            .collect()
    }

    #[test]
    fn renders_record_the_keys_they_pass() {
        let php = r#"<?php
use Symfony\Bridge\Twig\Mime\TemplatedEmail;
class C {
    function f($twig, $post) {
        $twig->render('post/show.html.twig', ['post' => $post, 'title' => 'x', 0 => 'y', $k => 1]);
        $twig->renderView('post/list.html.twig', compact('post'));
        $twig->renderBlock('post/show.html.twig', 'body', parameters: ['block' => 1]);
        $twig->render('post/none.html.twig');
        $twig->render('post/params.html.twig', $params);
        (new TemplatedEmail())->htmlTemplate('mail/a.html.twig')->context(['user' => 1]);
        (new TemplatedEmail())->context(['code' => 1])->textTemplate('mail/b.txt.twig');
    }
}
"#;
        let map = map_of(php, true);
        assert_eq!(
            contexts(&map, php),
            [
                "post/show.html.twig: post, title",
                "post/list.html.twig: post",
                "post/show.html.twig: block",
                "post/params.html.twig:  + $params",
                "mail/a.html.twig: user",
                "mail/b.txt.twig: code",
            ]
        );
    }

    #[test]
    fn template_actions_pass_what_they_return_or_their_arguments() {
        let php = r#"<?php
use Symfony\Bridge\Twig\Attribute\Template;
class C {
    #[Template('a.html.twig')]
    function a($post) {
        if ($post) { return ['post' => $post]; }
        return ['missing' => true];
    }
    #[Template('b.html.twig')]
    function b(int $id, string $slug): void {}
    #[Template('c.html.twig', vars: ['slug'])]
    function c(int $id, string $slug) {}
}
"#;
        let map = map_of(php, true);
        assert_eq!(
            contexts(&map, php),
            [
                "a.html.twig: post",
                "a.html.twig: missing",
                "b.html.twig: id, slug",
                "c.html.twig: slug",
            ]
        );
    }

    #[test]
    fn extensions_and_environments_declare_globals() {
        let php = r#"<?php
use Twig\Extension\AbstractExtension;
use Twig\Extension\GlobalsInterface;
class Globals extends AbstractExtension implements GlobalsInterface {
    public function getGlobals(): array { return ['site_name' => 'x', 'not a name' => 1]; }
}
class NotAnExtension {
    public function getGlobals(): array { return ['ignored' => 1]; }
}
function boot($twig) { $twig->addGlobal('ga_tracking', 'UA-1'); }
"#;
        let map = map_of(php, false);
        let declared: Vec<&str> = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: crate::symbol_map::StringKeyKind::TwigVariable,
                    key,
                    is_write: true,
                    ..
                } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(declared, ["site_name"]);
        let sites: Vec<(&str, bool)> = map
            .typed_receiver_sites
            .iter()
            .map(|site| (site.key.as_str(), site.is_write))
            .collect();
        assert_eq!(sites, [("ga_tracking", true)]);
    }
}
