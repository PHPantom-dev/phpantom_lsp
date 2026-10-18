//! Route names (`route('users.index')`, `Route::is('admin.*')`, the
//! redirector and URL generator) and the controller methods a route group's
//! `->controller(X::class)` makes plain strings name.

use super::*;
use crate::virtual_members::laravel::helpers::resolved_string_literal;

/// Whether `object` is one of the bare helper calls whose result names a
/// route in the methods it receives: `redirect()` (the redirector),
/// `url()` (the URL generator), and `response()` (the response factory).
pub(in crate::symbol_map::extraction) fn is_route_name_helper_receiver(
    object: &Expression<'_>,
) -> bool {
    is_bare_helper_call(object, "redirect")
        || is_bare_helper_call(object, "url")
        || is_bare_helper_call(object, "response")
}

/// Whether `member_name` takes the name of the route it points at as its
/// first argument.
///
/// The redirector and the URL generator share the `route()` /
/// `signedRoute()` / `temporarySignedRoute()` family; the response factory's
/// `redirectToRoute()` is the same thing under another name.  Every caller
/// checks its receiver first, so the lowercasing here is paid only for the
/// handful of calls that could be one of these.
pub(crate) fn is_route_name_method(member_name: &str) -> bool {
    matches!(
        member_name.to_ascii_lowercase().as_str(),
        "route" | "signedroute" | "temporarysignedroute" | "redirecttoroute"
    )
}

/// Whether a static call on one of Laravel's URL-building facades names a
/// route in its first argument.
pub(in crate::symbol_map::extraction) fn is_route_name_facade_call(
    clean_subject: &str,
    member_name: &str,
) -> bool {
    ["URL", "Redirect", "Response"]
        .iter()
        .any(|facade| matches_laravel_facade(clean_subject, facade))
        && is_route_name_method(member_name)
}

/// Whether `member_name` is the request's "is the current route named …?"
/// predicate, whose every argument is a route-name pattern.
///
/// Laravel spells this check `routeIs()` precisely to keep it apart from
/// `is()`, which matches URIs, so the name carries the whole meaning and no
/// receiver has to vouch for it.
pub(crate) fn is_request_route_pattern_method(member_name: &str) -> bool {
    member_name.eq_ignore_ascii_case("routeIs")
}

/// The `Route` facade's spellings of the same check.
///
/// `Route::is()` reads route names where `$request->is()` reads URIs, so this
/// one is only recognised on the facade.
pub(crate) fn is_route_facade_pattern_method(member_name: &str) -> bool {
    member_name.eq_ignore_ascii_case("is") || member_name.eq_ignore_ascii_case("currentRouteNamed")
}

/// Whether an instance-method chain roots at the `Route` facade, as
/// `Route::get(…)->can('update', 'post')` does.
///
/// The `can()` there names an ability, but its second argument is a *route
/// parameter* name rather than a model, so no model subject is recorded.
pub(in crate::symbol_map::extraction) fn chain_roots_at_route_facade(
    expr: &Expression<'_>,
    resolved_names: Option<&crate::names::OwnedResolvedNames>,
) -> bool {
    chain_roots_at_route_facade_inner(
        expr,
        crate::symbol_map::laravel_resources::FACADE_CHAIN_DEPTH,
        resolved_names,
    )
}

/// Return the real global class that can suppress a fluent chain rooted at
/// Laravel's optional `Route` alias. Imported/FQN facade roots are
/// unambiguous and therefore have no dependency.
pub(in crate::symbol_map::extraction) fn route_facade_root_dependency(
    expr: &Expression<'_>,
    resolved_names: &crate::names::OwnedResolvedNames,
) -> Option<crate::symbol_map::LaravelStringDependency> {
    route_facade_root_dependency_inner(
        expr,
        crate::symbol_map::laravel_resources::FACADE_CHAIN_DEPTH,
        resolved_names,
    )
}

fn route_facade_root_dependency_inner(
    expr: &Expression<'_>,
    depth: usize,
    resolved_names: &crate::names::OwnedResolvedNames,
) -> Option<crate::symbol_map::LaravelStringDependency> {
    if depth == 0 {
        return None;
    }
    match expr {
        Expression::Call(Call::Method(call)) => {
            route_facade_root_dependency_inner(call.object, depth - 1, resolved_names)
        }
        Expression::Call(Call::StaticMethod(call)) => {
            let name = resolved_names.get(call.class.span().start.offset)?;
            crate::symbol_map::LaravelStringDependency::root_facade(name)
        }
        _ => None,
    }
}

fn chain_roots_at_route_facade_inner(
    expr: &Expression<'_>,
    depth: usize,
    resolved_names: Option<&crate::names::OwnedResolvedNames>,
) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expression::Call(Call::Method(call)) => {
            chain_roots_at_route_facade_inner(call.object, depth - 1, resolved_names)
        }
        Expression::Call(Call::StaticMethod(call)) => {
            if let Some(names) = resolved_names {
                names
                    .get(call.class.span().start.offset)
                    .is_some_and(|name| matches_laravel_facade(name, "Route"))
            } else {
                matches_laravel_facade(strip_fqn_prefix(&expr_to_subject_text(call.class)), "Route")
            }
        }
        _ => false,
    }
}

/// HTTP method names used in Laravel route definitions.
///
/// `Route::get(…)`, `Route::post(…)`, etc.  `match` is excluded because
/// its action argument is in a different position (3rd, not 2nd).
pub(crate) const ROUTE_HTTP_METHODS: &[&str] =
    &["get", "post", "put", "patch", "delete", "options", "any"];

/// Walk a fluent method chain backwards looking for `->controller(X::class)`.
///
/// Returns the class name (as written in source, e.g. `"WorkItemController"`)
/// if found, `None` otherwise.  Handles both instance-method chains
/// (`->prefix('…')->controller(X::class)`) and a static entry point
/// (`Route::controller(X::class)`).
pub(in crate::symbol_map::extraction) fn laravel_route_find_controller_in_chain(
    expr: &Expression<'_>,
) -> Option<String> {
    match expr {
        Expression::Call(Call::Method(mc)) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &mc.method
                && ident.value.eq_ignore_ascii_case(b"controller")
            {
                return laravel_route_extract_class_arg(&mc.argument_list);
            }
            laravel_route_find_controller_in_chain(mc.object)
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &sc.method
                && ident.value.eq_ignore_ascii_case(b"controller")
            {
                return laravel_route_extract_class_arg(&sc.argument_list);
            }
            None
        }
        _ => None,
    }
}

/// Extract the class name from the first argument when it is a `X::class`
/// constant access.  Returns the source text of `X` (e.g.
/// `"WorkItemResourceController"` or `"App\\Http\\Controllers\\Foo"`).
fn laravel_route_extract_class_arg(args: &ArgumentList<'_>) -> Option<String> {
    let first_arg = args.arguments.iter().next()?;
    let name = expr_to_subject_text(crate::parser::class_fetch_target(first_arg.value())?);
    (!name.is_empty()).then_some(name)
}

/// Walk the closure (or arrow function) body passed to `->group()`.
pub(in crate::symbol_map::extraction) fn laravel_route_scan_group_body(
    expr: &Expression<'_>,
    controller: &str,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match expr {
        Expression::Closure(closure) => {
            for stmt in closure.body.statements.iter() {
                laravel_route_scan_stmt(stmt, controller, content, spans);
            }
        }
        Expression::ArrowFunction(af) => {
            laravel_route_scan_expr(af.expression, controller, content, spans);
        }
        _ => {}
    }
}

/// Scan a statement for route definition calls with controller method strings.
fn laravel_route_scan_stmt(
    stmt: &Statement<'_>,
    controller: &str,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match stmt {
        Statement::Expression(e) => {
            laravel_route_scan_expr(e.expression, controller, content, spans);
        }
        Statement::Return(r) => {
            if let Some(v) = r.value {
                laravel_route_scan_expr(v, controller, content, spans);
            }
        }
        Statement::If(if_stmt) => {
            crate::parser::for_each_if_branch(if_stmt, |statements| {
                for s in statements {
                    laravel_route_scan_stmt(s, controller, content, spans);
                }
            });
        }
        Statement::Foreach(fe) => {
            for s in fe.body.statements() {
                laravel_route_scan_stmt(s, controller, content, spans);
            }
        }
        _ => {}
    }
}

/// Scan an expression for `Route::get(…)` / `Route::post(…)` / etc. and
/// emit a [`SymbolKind::MemberAccess`] span for the action string.
///
/// Also recurses into nested `->group()` calls that do **not** declare
/// their own `->controller()` (inheriting the parent controller), while
/// stopping at nested groups that **do** declare a new controller (those
/// are handled by their own extraction-time invocation).
fn laravel_route_scan_expr(
    expr: &Expression<'_>,
    controller: &str,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match expr {
        // Chained call: Route::patch('cancel', 'cancel')->name('cancel')
        Expression::Call(Call::Method(mc)) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &mc.method
                && ident.value.eq_ignore_ascii_case(b"group")
            {
                // Nested group — check for its own controller.
                if laravel_route_find_controller_in_chain(mc.object).is_some() {
                    return; // Own controller; handled separately.
                }
                // No new controller — inherit the parent's.
                for arg in mc.argument_list.arguments.iter() {
                    laravel_route_scan_group_body(arg.value(), controller, content, spans);
                }
                return;
            }
            // Walk the inner object (chain before ->name() etc.).
            laravel_route_scan_expr(mc.object, controller, content, spans);
        }
        // Route::patch('cancel', 'cancel')
        Expression::Call(Call::StaticMethod(sc)) => {
            let subject = expr_to_subject_text(sc.class);
            if !strip_fqn_prefix(&subject).eq_ignore_ascii_case("Route") {
                return;
            }
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return;
            };
            let method_lower = bytes_to_str(ident.value).to_ascii_lowercase();
            if !ROUTE_HTTP_METHODS.iter().any(|m| *m == method_lower) {
                return;
            }
            // Second argument is the controller method name.
            let mut args_iter = sc.argument_list.arguments.iter();
            let _uri_arg = args_iter.next(); // skip first (URI)
            let Some(action_arg) = args_iter.next() else {
                return;
            };
            let Some((method_name, inner_start, inner_end)) =
                resolved_string_literal(action_arg.value(), content)
            else {
                return;
            };
            // Must look like a valid PHP method name.
            if !is_identifier(method_name) {
                return;
            }

            spans.push(SymbolSpan {
                start: inner_start as u32,
                end: inner_end as u32,
                kind: SymbolKind::MemberAccess {
                    subject_text: SubjectText::owned(controller.to_string()),
                    member_name: crate::atom::atom(method_name),
                    is_static: true,
                    is_method_call: true,
                    docblock_ref: DocblockMemberRef::No,
                    is_array_callable: false,
                    is_nullsafe: false,
                },
            });
        }
        _ => {}
    }
}
