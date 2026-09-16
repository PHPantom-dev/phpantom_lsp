//! What a service provider puts in a template's scope without the template
//! or its caller saying so.
//!
//! `View::share('key', $value)` adds a variable to the data *every* template
//! renders with, and `View::composer('view.name', …)` adds one to the data of
//! the views it targets, either from the `$view->with(…)` calls of an inline
//! closure or from the `compose()` body of a composer class. A template that
//! reads one of those has nothing to resolve it against, so the registrations
//! are scanned alongside the rest of a provider's resources.
//!
//! Only the names and the *location* of each value expression are recorded
//! here. The types come from resolving those expressions through the shared
//! pipeline (see [`crate::blade::shared_vars`]), which needs the class index
//! this scan does not have.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::const_eval::{Scope, const_string};
use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;
use crate::symbol_map::extraction::laravel::is_laravel_container_expr;

/// A variable a service provider puts into template scope: its name, and
/// where the expression that gives it a value sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedViewVar {
    /// The variable name, without `$`.
    pub name: String,
    /// The file holding the value expression.
    pub file: PathBuf,
    /// Byte offset where the value expression starts in `file`.
    pub offset: u32,
}

/// A `View::composer(views, handler)` registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ViewComposer {
    /// The view name patterns it targets, as written (`profile`,
    /// `partials.*`, `*`).
    pub views: Vec<String>,
    /// The variables an inline closure's `$view->with(…)` calls declare.
    pub inline: Vec<SharedViewVar>,
    /// The composer class whose `compose()` body declares them, when the
    /// registration names a class instead of writing a closure.
    pub class: Option<String>,
}

/// What one call on the view factory registers.
pub(crate) enum ViewDataRegistration {
    /// `View::share(…)`: variables every template sees.
    Shared(Vec<SharedViewVar>),
    /// `View::composer(…)` / `View::composers(…)`.
    Composers(Vec<ViewComposer>),
}

/// What a call on Laravel's view factory registers, or `None` for any other
/// expression.
///
/// Both ways of reaching the factory are covered: the `View` facade, and the
/// container entry a provider resolves (`$this->app['view']`, `app('view')`,
/// `view()`).
pub(crate) fn view_data_registration(
    expr: &Expression<'_>,
    content: &str,
    file_path: &Path,
    scope: &Scope,
    resolved: &OwnedResolvedNames,
) -> Option<ViewDataRegistration> {
    let (method, argument_list) = match expr {
        Expression::Call(Call::StaticMethod(sc)) => {
            let ClassLikeMemberSelector::Identifier(method) = &sc.method else {
                return None;
            };
            if !is_view_facade(sc.class) {
                return None;
            }
            (method.value, &sc.argument_list)
        }
        Expression::Call(Call::Method(mc)) => {
            let ClassLikeMemberSelector::Identifier(method) = &mc.method else {
                return None;
            };
            if !is_view_factory_expr(mc.object) {
                return None;
            }
            (method.value, &mc.argument_list)
        }
        _ => return None,
    };

    if method.eq_ignore_ascii_case(b"share") {
        let mut vars = Vec::new();
        collect_data_vars(argument_list, content, scope, file_path, &mut vars);
        return (!vars.is_empty()).then_some(ViewDataRegistration::Shared(vars));
    }

    if method.eq_ignore_ascii_case(b"composer") {
        let mut args = argument_list.arguments.iter();
        let views = view_patterns(args.next()?.value(), content, scope);
        let handler = args.next()?.value();
        let composer = composer_for_handler(views, handler, content, scope, file_path, resolved)?;
        return Some(ViewDataRegistration::Composers(vec![composer]));
    }

    if method.eq_ignore_ascii_case(b"composers") {
        let composers = composers_map(argument_list, content, scope, file_path, resolved);
        return (!composers.is_empty()).then_some(ViewDataRegistration::Composers(composers));
    }

    None
}

/// The variables a composer class's `compose()` body declares through
/// `$view->with(…)`.
///
/// Laravel calls `compose(View $view)` (or, for an invokable composer,
/// `__invoke`) with the view being rendered, so what the body writes against
/// that parameter is what the targeted templates see.
pub(crate) fn composer_class_vars(content: &str, file_path: &Path) -> Vec<SharedViewVar> {
    let scope = Scope::default();
    let mut vars = Vec::new();
    crate::parser::with_parsed_program(content, "laravel_view_composer", |program, content| {
        for_each_method(program, &mut |method| {
            if !method.name.value.eq_ignore_ascii_case(b"compose")
                && !method.name.value.eq_ignore_ascii_case(b"__invoke")
            {
                return;
            }
            let Some(parameter) = method.parameter_list.parameters.iter().next() else {
                return;
            };
            let MethodBody::Concrete(body) = &method.body else {
                return;
            };
            let view = parameter.variable.name;
            super::helpers::walk_block_expressions(body, &mut |expr| {
                record_with_call(expr, view, content, &scope, file_path, &mut vars);
                ControlFlow::Continue(())
            });
        });
    });
    vars
}

/// Whether a static call's class expression names the `View` facade.
fn is_view_facade(class: &Expression<'_>) -> bool {
    let Expression::Identifier(ident) = class else {
        return false;
    };
    let subject = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
    subject.eq_ignore_ascii_case("View")
}

/// Whether an expression hands back Laravel's view factory: the container
/// entry a provider resolves (`$this->app['view']`, `$this->app->make('view')`,
/// `app('view')`) or the `view()` helper called with no arguments.
fn is_view_factory_expr(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::ArrayAccess(access) => {
            is_laravel_container_expr(access.array) && is_view_key(access.index)
        }
        Expression::Call(Call::Function(fc)) => {
            let Expression::Identifier(ident) = fc.function else {
                return false;
            };
            let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
            let mut args = fc.argument_list.arguments.iter();
            match args.next() {
                // `app('view')`, and nothing else the helper is handed.
                Some(first) => name.eq_ignore_ascii_case("app") && is_view_key(first.value()),
                // `view()` with no name renders nothing; it is the factory.
                None => name.eq_ignore_ascii_case("view"),
            }
        }
        Expression::Call(Call::Method(mc)) => {
            let ClassLikeMemberSelector::Identifier(method) = &mc.method else {
                return false;
            };
            method.value.eq_ignore_ascii_case(b"make")
                && is_laravel_container_expr(mc.object)
                && mc
                    .argument_list
                    .arguments
                    .iter()
                    .next()
                    .is_some_and(|arg| is_view_key(arg.value()))
        }
        _ => false,
    }
}

/// Whether an expression is the literal container key `'view'`.
fn is_view_key(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Literal(literal::Literal::String(s))
        if s.value.is_some_and(|value| value.eq_ignore_ascii_case(b"view")))
}

/// The variables a `share(…)` / `with(…)` argument list declares: either a
/// `('key', $value)` pair or a `['key' => $value, …]` array.
fn collect_data_vars(
    argument_list: &ArgumentList<'_>,
    content: &str,
    scope: &Scope,
    file: &Path,
    out: &mut Vec<SharedViewVar>,
) {
    let mut args = argument_list.arguments.iter();
    let Some(first) = args.next() else {
        return;
    };
    let mut push = |name: String, value: &Expression<'_>| {
        out.push(SharedViewVar {
            name,
            file: file.to_path_buf(),
            offset: value.span().start.offset,
        });
    };

    if let Some(value) = args.next() {
        if let Some(name) = data_key(first.value(), content, scope) {
            push(name, value.value());
        }
        return;
    }

    let elements = match first.value() {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return,
    };
    for element in elements.iter() {
        if let ArrayElement::KeyValue(kv) = element
            && let Some(name) = data_key(kv.key, content, scope)
        {
            push(name, kv.value);
        }
    }
}

/// A data key that a template could read as a variable. A key holding
/// anything else (`'user.name'`, a runtime value) names no variable, so it is
/// skipped rather than declared under a name no template can write.
fn data_key(expr: &Expression<'_>, content: &str, scope: &Scope) -> Option<String> {
    let key = const_string(expr, content, scope)?;
    let mut chars = key.chars();
    let leads = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    let rest = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    (leads && rest).then_some(key)
}

/// The view name patterns a composer registration targets: one name, or the
/// array of names a single registration covers.
fn view_patterns(expr: &Expression<'_>, content: &str, scope: &Scope) -> Vec<String> {
    if let Some(single) = const_string(expr, content, scope) {
        return vec![single];
    }
    let elements = match expr {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return Vec::new(),
    };
    elements
        .iter()
        .filter_map(|element| match element {
            ArrayElement::Value(value) => const_string(value.value, content, scope),
            _ => None,
        })
        .collect()
}

/// The composer a registration's handler describes: the `$view->with(…)`
/// calls of an inline closure, or the class whose `compose()` body holds
/// them.
///
/// `None` when the handler is neither (an array callable, a `Class@method`
/// string): there is no body to read and inventing variables from a name
/// alone would declare things the template never receives.
fn composer_for_handler(
    views: Vec<String>,
    handler: &Expression<'_>,
    content: &str,
    scope: &Scope,
    file: &Path,
    resolved: &OwnedResolvedNames,
) -> Option<ViewComposer> {
    if views.is_empty() {
        return None;
    }

    if let Some(view) = handler_parameter(handler) {
        let mut inline = Vec::new();
        super::helpers::walk_expression_tree(handler, &mut |expr| {
            record_with_call(expr, view, content, scope, file, &mut inline);
            ControlFlow::Continue(())
        });
        return (!inline.is_empty()).then_some(ViewComposer {
            views,
            inline,
            class: None,
        });
    }

    Some(ViewComposer {
        views,
        inline: Vec::new(),
        class: Some(composer_class_name(handler, content, scope, resolved)?),
    })
}

/// The `View::composers([Composer::class => 'view.name', …])` form, which
/// registers a whole table of composers at once.
fn composers_map(
    argument_list: &ArgumentList<'_>,
    content: &str,
    scope: &Scope,
    file: &Path,
    resolved: &OwnedResolvedNames,
) -> Vec<ViewComposer> {
    let Some(first) = argument_list.arguments.iter().next() else {
        return Vec::new();
    };
    let elements = match first.value() {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return Vec::new(),
    };
    elements
        .iter()
        .filter_map(|element| {
            let ArrayElement::KeyValue(kv) = element else {
                return None;
            };
            // The table reads the other way round from `composer()`: the
            // handler is the key and the views it covers are the value.
            composer_for_handler(
                view_patterns(kv.value, content, scope),
                kv.key,
                content,
                scope,
                file,
                resolved,
            )
        })
        .collect()
}

/// The view parameter of a closure handler (`function ($view) { … }`), or
/// `None` when the handler is not a closure at all.
fn handler_parameter<'arena>(handler: &Expression<'arena>) -> Option<&'arena [u8]> {
    let parameters = match handler {
        Expression::Closure(closure) => &closure.parameter_list,
        Expression::ArrowFunction(arrow) => &arrow.parameter_list,
        _ => return None,
    };
    Some(parameters.parameters.iter().next()?.variable.name)
}

/// The composer class a handler names, either as `Composer::class` or as a
/// plain class-name string.
fn composer_class_name(
    handler: &Expression<'_>,
    content: &str,
    scope: &Scope,
    resolved: &OwnedResolvedNames,
) -> Option<String> {
    if let Expression::Access(Access::ClassConstant(access)) = handler
        && matches!(
            &access.constant,
            ClassLikeConstantSelector::Identifier(constant)
                if constant.value.eq_ignore_ascii_case(b"class")
        )
        && let Expression::Identifier(ident) = access.class
    {
        if let Some(fqn) = resolved.get(ident.span().start.offset) {
            return Some(fqn.trim_start_matches('\\').to_string());
        }
        let raw = bytes_to_str(ident.value()).trim_start_matches('\\');
        return (!raw.is_empty()).then(|| raw.to_string());
    }

    // A single-quoted class name keeps its doubled separators in source.
    let name = const_string(handler, content, scope)?
        .replace("\\\\", "\\")
        .trim_start_matches('\\')
        .to_string();
    (!name.is_empty() && !name.contains('@')).then_some(name)
}

/// Record the variables one `$view->with(…)` call declares, when it is
/// written against the view parameter `view`.
///
/// The call may sit at the end of a chain (`$view->with('a', 1)->with('b',
/// 2)`), so the receiver spine is walked back to the variable it roots at.
fn record_with_call(
    expr: &Expression<'_>,
    view: &[u8],
    content: &str,
    scope: &Scope,
    file: &Path,
    out: &mut Vec<SharedViewVar>,
) {
    let Expression::Call(Call::Method(mc)) = expr else {
        return;
    };
    let ClassLikeMemberSelector::Identifier(method) = &mc.method else {
        return;
    };
    if !method.value.eq_ignore_ascii_case(b"with") || !chain_roots_at_variable(mc.object, view) {
        return;
    }
    collect_data_vars(&mc.argument_list, content, scope, file, out);
}

/// Whether the receiver spine of a method call roots at the variable `name`.
fn chain_roots_at_variable(mut expr: &Expression<'_>, name: &[u8]) -> bool {
    loop {
        match expr {
            Expression::Variable(Variable::Direct(dv)) => return dv.name == name,
            Expression::Call(Call::Method(mc)) => expr = mc.object,
            Expression::Call(Call::NullSafeMethod(mc)) => expr = mc.object,
            Expression::Parenthesized(p) => expr = p.expression,
            _ => return false,
        }
    }
}

/// Visit every method declared by a class-like in `program`.
fn for_each_method<'ast, 'arena>(
    program: &'ast Program<'arena>,
    visit: &mut impl FnMut(&'ast Method<'arena>),
) {
    fn walk<'ast, 'arena: 'ast>(
        statements: impl Iterator<Item = &'ast Statement<'arena>>,
        visit: &mut impl FnMut(&'ast Method<'arena>),
    ) {
        for statement in statements {
            let members = match statement {
                Statement::Namespace(ns) => {
                    walk(ns.statements().iter(), visit);
                    continue;
                }
                Statement::Class(class) => &class.members,
                Statement::Trait(r#trait) => &r#trait.members,
                _ => continue,
            };
            for member in members.iter() {
                if let ClassLikeMember::Method(method) = member {
                    visit(method);
                }
            }
        }
    }
    walk(program.statements.iter(), visit);
}

#[cfg(test)]
#[path = "view_data_tests.rs"]
mod tests;
