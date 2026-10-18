//! The container bindings a provider makes under a string key, and whether
//! it replaces where translation strings come from.

use std::ops::ControlFlow;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::super::const_eval::const_string;
use super::extract::ProviderFile;
use super::{Alias, Binding, BindingSite, ProviderResources};
use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;
use crate::symbol_map::extraction::laravel::is_laravel_container_expr;

/// Container keys whose binding decides where translation strings come from.
const TRANSLATION_BINDINGS: [&str; 2] = ["translator", "translation.loader"];

/// The classes Laravel's own `TranslationServiceProvider` binds those keys
/// to.  A factory that builds anything else reads its lines from somewhere
/// other than the `lang/` directories we scan.
const FILE_TRANSLATION_CLASSES: [&str; 2] = ["FileLoader", "Translator"];

/// Container methods that put a new value behind a key.
const BINDING_METHODS: &[&[u8]] = &[
    b"bind",
    b"bindif",
    b"singleton",
    b"singletonif",
    b"scoped",
    b"scopedif",
    b"instance",
    b"extend",
];

/// Record a container registration made on the application
/// (`$this->app->alias(…)`, `->bind(…)`, `->singleton(…)`, …), and report
/// whether the call was one.
pub(super) fn record_container_call(
    file: &ProviderFile<'_, '_>,
    method: &[u8],
    call: &MethodCall<'_>,
    resources: &mut ProviderResources,
) -> bool {
    if !is_laravel_container_expr(call.object) {
        return false;
    }
    let mut args = call.argument_list.arguments.iter();

    // `$this->app->alias(Concrete::class, 'key')` gives an existing
    // binding another name.  The arguments read the other way round from
    // `bind()`: the key is the second one, and the first names what it
    // stands for.
    if method == b"alias" {
        let (Some(target), Some(key_arg)) = (args.next(), args.next()) else {
            return false;
        };
        let Some(key) = alias_key(file, key_arg.value()) else {
            return false;
        };
        let site = BindingSite {
            path: file.path.to_path_buf(),
            offset: key_offset(key_arg.value()),
        };
        match binding_concrete(Some(target.value()), file.resolved) {
            Some(concrete) => {
                resources.bindings.insert(
                    key,
                    Binding {
                        class: concrete,
                        site,
                        provider: Arc::clone(file.provider),
                    },
                );
            }
            // The target is another string key, whose own binding may not
            // have been scanned yet.
            None => {
                if let Some(aliased) = const_string(target.value(), file.content, file.scope) {
                    resources.aliases.insert(
                        key,
                        Alias {
                            target: aliased,
                            site,
                            provider: Arc::clone(file.provider),
                        },
                    );
                }
            }
        }
        return true;
    }

    if !BINDING_METHODS.contains(&method) {
        return false;
    }
    let Some(key_arg) = args.next() else {
        return false;
    };
    let Some(key) = const_string(key_arg.value(), file.content, file.scope) else {
        return false;
    };
    let factory = args.next().map(|a| a.value());

    // A translation binding decides where the strings come from, on top
    // of naming a class: anything but Laravel's own file-based pair
    // reads its lines from a source we cannot enumerate.  `extend`
    // decorates whatever is already bound, so even a file-based wrapper
    // adds lines from somewhere else.
    if TRANSLATION_BINDINGS.contains(&key.as_str())
        && (method == b"extend" || !builds_file_translator(factory))
    {
        resources.custom_translation_loader = true;
    }

    // `extend` wraps whatever the key already holds; which class comes
    // out depends on the binding it decorates, so only the calls that
    // *replace* the value tell us the concrete type.
    if method != b"extend"
        && let Some(concrete) = binding_concrete(factory, file.resolved)
    {
        resources.bindings.insert(
            key,
            Binding {
                class: concrete,
                site: BindingSite {
                    path: file.path.to_path_buf(),
                    offset: key_offset(key_arg.value()),
                },
                provider: Arc::clone(file.provider),
            },
        );
    }
    true
}

/// The two provider properties Laravel reads container registrations out of,
/// as an alternative to writing them in `register()`.
const BINDING_PROPERTIES: [&[u8]; 2] = [b"$bindings", b"$singletons"];

/// Record the string-keyed entries of a provider's `$bindings` /
/// `$singletons` arrays, descending through namespace and block statements to
/// find the class that declares them.
///
/// Only a string key is recorded: an entry keyed by `Contract::class` binds a
/// name that already resolves on its own, and retyping it to the concrete
/// would contradict the contract the application declared.
pub(super) fn scan_property_bindings(
    file: &ProviderFile<'_, '_>,
    stmt: &Statement<'_>,
    resources: &mut ProviderResources,
) {
    let members = match stmt {
        Statement::Class(class) => &class.members,
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                scan_property_bindings(file, inner, resources);
            }
            return;
        }
        Statement::Block(block) => {
            for inner in block.statements.iter() {
                scan_property_bindings(file, inner, resources);
            }
            return;
        }
        _ => return,
    };

    for member in members.iter() {
        let ClassLikeMember::Property(Property::Plain(prop)) = member else {
            continue;
        };
        for item in prop.items.iter() {
            let PropertyItem::Concrete(concrete) = item else {
                continue;
            };
            if !BINDING_PROPERTIES.contains(&concrete.variable.name) {
                continue;
            }
            let Expression::Array(array) = concrete.value else {
                continue;
            };
            for element in array.elements.iter() {
                let ArrayElement::KeyValue(kv) = element else {
                    continue;
                };
                let Some(key) = const_string(kv.key, file.content, file.scope) else {
                    continue;
                };
                // An integer key is Laravel's shorthand for binding the class
                // to itself, which needs no table.
                if key.parse::<i64>().is_ok() {
                    continue;
                }
                let Some(class) = binding_concrete(Some(kv.value), file.resolved) else {
                    continue;
                };
                resources.bindings.insert(
                    key,
                    Binding {
                        class,
                        site: BindingSite {
                            path: file.path.to_path_buf(),
                            offset: key_offset(kv.key),
                        },
                        provider: Arc::clone(file.provider),
                    },
                );
            }
        }
    }
}

/// Where go-to-definition lands for a registration's key: just inside the
/// quotes of a string literal, or the start of whatever else names it.
fn key_offset(expr: &Expression<'_>) -> u32 {
    match expr {
        Expression::Literal(literal::Literal::String(s)) => s.span.start.offset + 1,
        other => other.span().start.offset,
    }
}

/// The concrete class a container binding puts behind its key.
///
/// Covers the shapes a service provider writes: the class itself
/// (`bind('foo', Foo::class)`), a ready-made instance
/// (`instance('foo', new Foo())`), and the usual factory
/// (`singleton('foo', fn () => new Foo())` or its closure equivalent).
///
/// A factory that declares a return type is taken at its word, the way a
/// method's declared return type decides what a call to it yields: a factory
/// written `function (): Gateway { return new StripeGateway(); }` binds the
/// contract its author named, not the implementation behind it.  Only an
/// undeclared factory is read from its body, whose first `return` decides the
/// type.  A factory that declares nothing and hands back anything else (a
/// container lookup, a variable, a conditional) yields `None`: guessing there
/// would bind the key to a class the application never resolves.
fn binding_concrete(
    expr: Option<&Expression<'_>>,
    resolved: &OwnedResolvedNames,
) -> Option<String> {
    let expr = expr?;
    match expr {
        Expression::Instantiation(inst) => match inst.class {
            Expression::Identifier(id) => resolved.class_fqn(id),
            _ => None,
        },
        Expression::Access(Access::ClassConstant(_)) => resolved.class_fetch_fqn(expr),
        Expression::ArrowFunction(arrow) => {
            declared_factory_return(arrow.return_type_hint.as_ref(), resolved)
                .or_else(|| binding_concrete(Some(arrow.expression), resolved))
        }
        Expression::Closure(closure) => {
            declared_factory_return(closure.return_type_hint.as_ref(), resolved)
                .or_else(|| closure_binding_concrete(closure, resolved))
        }
        Expression::Parenthesized(inner) => binding_concrete(Some(inner.expression), resolved),
        _ => None,
    }
}

/// The class a factory's own `: Concrete` return type names.
///
/// Nothing but a single class name counts: a union, an intersection, or a
/// built-in type does not name one class the key holds.  A nullable hint
/// (`?Concrete`) still names one, since a container that hands back `null`
/// for the key has nothing else it could be resolved as.
fn declared_factory_return(
    return_type_hint: Option<&FunctionLikeReturnTypeHint<'_>>,
    resolved: &OwnedResolvedNames,
) -> Option<String> {
    super::super::macros::resolve_hint_target_fqn(&return_type_hint?.hint, resolved)
}

/// The concrete class a closure factory's `return` hands back.
///
/// A factory often configures the instance before returning it
/// (`$translator = new DatabaseTranslator(…); $translator->setFallback(…);
/// return $translator;`), so a returned variable is followed back to the
/// last value assigned to it above the return.
fn closure_binding_concrete(
    closure: &Closure<'_>,
    resolved: &OwnedResolvedNames,
) -> Option<String> {
    let mut assignments: Vec<(&[u8], &Expression<'_>)> = Vec::new();
    for stmt in closure.body.statements.iter() {
        match stmt {
            Statement::Return(ret) => {
                if let Some(concrete) = binding_concrete(ret.value, resolved) {
                    return Some(concrete);
                }
                let Some(Expression::Variable(Variable::Direct(target))) = ret.value else {
                    return None;
                };
                return assignments
                    .iter()
                    .rev()
                    .find(|(name, _)| *name == target.name)
                    .and_then(|(_, rhs)| binding_concrete(Some(rhs), resolved));
            }
            Statement::Expression(expr_stmt) => {
                if let Expression::Assignment(assign) = expr_stmt.expression
                    && assign.operator.is_assign()
                    && let Expression::Variable(Variable::Direct(lhs)) = assign.lhs
                {
                    assignments.push((lhs.name, assign.rhs));
                }
            }
            _ => {}
        }
    }
    None
}

/// The container key an `alias()` argument names.
///
/// An alias is keyed either by a string (`alias(HubInterface::class,
/// 'sentry')`) or by a contract's name (`alias('sentry',
/// HubInterface::class)`), and both sides of the call accept both forms.
fn alias_key(file: &ProviderFile<'_, '_>, expr: &Expression<'_>) -> Option<String> {
    const_string(expr, file.content, file.scope).or_else(|| file.resolved.class_fetch_fqn(expr))
}

/// Whether a translation binding's factory builds Laravel's own file-based
/// translator, i.e. every class it names is one of `FILE_TRANSLATION_CLASSES`.
///
/// A factory that reaches for anything else has moved the lines out of the
/// `lang/` directories, and one that names no class at all (a container
/// lookup, a variable) says nothing either way, which is equally unknowable.
fn builds_file_translator(factory: Option<&Expression<'_>>) -> bool {
    let Some(factory) = factory else {
        return false;
    };

    let mut named_any = false;
    let mut all_file_based = true;
    super::super::expr_walker::walk_expression_tree(factory, &mut |expr| {
        if let Some(name) = instantiated_or_class_string(expr) {
            named_any = true;
            if !FILE_TRANSLATION_CLASSES
                .iter()
                .any(|known| crate::util::short_name(name).eq_ignore_ascii_case(known))
            {
                all_file_based = false;
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    });

    named_any && all_file_based
}

/// The class an expression names, either by instantiating it (`new X(…)`) or
/// by referring to it as a string (`X::class`).
///
/// An empty name means the expression names a class that is only known at
/// runtime (`new $loaderClass`), which no more resolves to Laravel's own
/// loader than an explicit replacement does.
fn instantiated_or_class_string<'arena>(expr: &Expression<'arena>) -> Option<&'arena str> {
    let class = match expr {
        Expression::Instantiation(inst) => inst.class,
        _ => crate::parser::class_fetch_target(expr)?,
    };
    match class {
        Expression::Identifier(id) => Some(bytes_to_str(id.value())),
        _ => Some(""),
    }
}
