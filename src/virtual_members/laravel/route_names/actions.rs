use super::*;
use std::cmp::Ordering;

use mago_names::ResolvedNames;

use crate::atom::bytes_to_str;
use crate::virtual_members::laravel::const_eval::{Scope, const_string};

/// A route that dispatches to a controller method.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RouteAction {
    /// The controller's FQN, without a leading `\`.
    pub controller: String,
    /// The method the route calls, `__invoke` for an invokable controller.
    pub method: String,
    /// The HTTP verbs the route answers, upper-case and `|`-separated
    /// (`GET`, `PUT|PATCH`), or `ANY`.  Empty when not statically known.
    pub verbs: String,
    /// The URI, spelled as [`RouteEntry::uri`] spells it.  Empty when not
    /// statically known.
    pub uri: String,
    /// The route's name, when it has one that is statically known.
    pub name: Option<String>,
    /// Where the route is registered.
    pub declaration: RouteDeclaration,
}

/// Compare two class or method names the way PHP does: ASCII
/// case-insensitively.
pub(super) fn cmp_ignore_ascii_case(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(b.bytes().map(|byte| byte.to_ascii_lowercase()))
}

/// The controller method a route action names, as `(controller, method)`.
///
/// Laravel accepts `[Controller::class, 'method']`, an invokable
/// `Controller::class`, the `'Controller@method'` string, an array whose
/// `uses` key holds one of those, and inside a `->controller(X::class)` group
/// a bare method name, which `group_controller` supplies the class for.  A
/// closure, or anything not statically known, names no method.
pub(super) fn action_target(
    action: &Expression<'_>,
    group_controller: Option<&str>,
    content: &str,
    scope: &Scope,
    names: &ResolvedNames<'_>,
) -> Option<(String, String)> {
    if matches!(action, Expression::Array(_) | Expression::LegacyArray(_)) {
        let elements = resource_array_elements(action);
        for element in elements {
            if let ArrayElement::KeyValue(pair) = element
                && const_string(pair.key, content, scope).as_deref() == Some("uses")
            {
                return action_target(pair.value, group_controller, content, scope, names);
            }
        }
        let mut values = elements.iter().filter_map(|element| match element {
            ArrayElement::Value(value) => Some(value.value),
            _ => None,
        });
        let controller = class_name(values.next()?, content, scope, names)?;
        let method = const_string(values.next()?, content, scope)?;
        return is_method_name(&method).then_some((controller, method));
    }
    if let Some(controller) = class_fetch_fqn(action, names) {
        return Some((controller, "__invoke".to_string()));
    }
    let action = const_string(action, content, scope)?;
    let (controller, method) = match action.split_once('@') {
        Some((controller, method)) => (
            controller.trim_start_matches('\\').to_string(),
            method.to_string(),
        ),
        None => (group_controller?.to_string(), action),
    };
    (!controller.is_empty() && is_method_name(&method)).then_some((controller, method))
}

/// The controller a `->controller(X::class)` link, or the static
/// `Route::controller(X::class)` entry point, of the call chain `expr` names.
pub(super) fn chain_controller(
    mut expr: &Expression<'_>,
    content: &str,
    scope: &Scope,
    names: &ResolvedNames<'_>,
) -> Option<String> {
    loop {
        let (method, args, object) = match expr {
            Expression::Call(Call::Method(mc)) => (&mc.method, &mc.argument_list, Some(mc.object)),
            Expression::Call(Call::StaticMethod(sc)) => (&sc.method, &sc.argument_list, None),
            _ => return None,
        };
        if let ClassLikeMemberSelector::Identifier(ident) = method
            && ident.value.eq_ignore_ascii_case(b"controller")
        {
            let argument = args.arguments.iter().next()?;
            return class_name(argument.value(), content, scope, names);
        }
        expr = object?;
    }
}

/// The class a controller argument names: `X::class`, resolved through the
/// file's imports, or a string holding the class' full name.
pub(super) fn class_name(
    expr: &Expression<'_>,
    content: &str,
    scope: &Scope,
    names: &ResolvedNames<'_>,
) -> Option<String> {
    if let Some(fqn) = class_fetch_fqn(expr, names) {
        return Some(fqn);
    }
    let name = const_string(expr, content, scope)?;
    let name = name.trim_start_matches('\\');
    (!name.is_empty()).then(|| name.to_string())
}

fn class_fetch_fqn(expr: &Expression<'_>, names: &ResolvedNames<'_>) -> Option<String> {
    let ident = crate::parser::class_fetch_identifier(expr)?;
    let fqn = names
        .resolve(ident)
        .map_or_else(|| bytes_to_str(ident.value()), bytes_to_str)
        .trim_start_matches('\\');
    (!fqn.is_empty()).then(|| fqn.to_string())
}

fn is_method_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_' || first >= 0x80)
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80)
}

/// The verbs a registration call answers, upper-case and `|`-separated.
///
/// `method` is the registration method, lower-cased.  `Route::match()` lists
/// its verbs in its first argument; a list that is not statically known
/// leaves the verbs unknown.
pub(super) fn registration_verbs(
    method: &[u8],
    args: &ArgumentList<'_>,
    content: &str,
    scope: &Scope,
) -> String {
    match method {
        b"match" => {
            let Some(verbs) = args.arguments.iter().next().map(|arg| arg.value()) else {
                return String::new();
            };
            let listed: Vec<String> = match const_string(verbs, content, scope) {
                Some(verb) => vec![verb],
                None => resource_array_elements(verbs)
                    .iter()
                    .filter_map(|element| match element {
                        ArrayElement::Value(value) => const_string(value.value, content, scope),
                        _ => None,
                    })
                    .collect(),
            };
            listed.join("|").to_ascii_uppercase()
        }
        b"fallback" => "GET".to_string(),
        _ => bytes_to_str(method).to_ascii_uppercase(),
    }
}

/// The verbs Laravel's `ResourceRegistrar` registers each resource method
/// under.
pub(super) fn resource_verbs(suffix: &str) -> &'static str {
    match suffix {
        "store" => "POST",
        "update" => "PUT|PATCH",
        "destroy" => "DELETE",
        _ => "GET",
    }
}
