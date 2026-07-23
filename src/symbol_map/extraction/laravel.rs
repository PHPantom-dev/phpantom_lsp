use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use super::*;

/// Check whether an attribute class name refers to a Laravel container
/// attribute (`Config`, `Database`, `Cache`, `Log`, `Storage`, `Auth`,
/// `Authenticated`).  Returns the corresponding [`LaravelStringKind`] if
/// so — always `Config` since all container attributes resolve to config
/// sub-keys.
///
/// FQN names (containing `\`) are matched directly against
/// `Illuminate\Container\Attributes\*`.  Short names require the file to
/// import from that namespace; the result of that check is cached in
/// `import_cache` to avoid repeated linear scans of the file content.
pub(super) const LARAVEL_CONTAINER_ATTR_NS: &str = "Illuminate\\Container\\Attributes\\";
pub(super) const LARAVEL_CONTAINER_ATTR_NAMES: &[&str] = &[
    "Config",
    "Database",
    "DB",
    "Cache",
    "Log",
    "Storage",
    "Auth",
    "Authenticated",
];

pub(super) fn resolve_laravel_container_attr(
    class_name: &str,
    import_cache: &mut Option<bool>,
    content: &str,
) -> Option<crate::symbol_map::LaravelStringKind> {
    if class_name.contains('\\') {
        let stripped = class_name.strip_prefix(LARAVEL_CONTAINER_ATTR_NS)?;
        if LARAVEL_CONTAINER_ATTR_NAMES.contains(&stripped) {
            return Some(crate::symbol_map::LaravelStringKind::Config);
        }
        return None;
    }
    if !LARAVEL_CONTAINER_ATTR_NAMES.contains(&class_name) {
        return None;
    }
    let has_import = *import_cache
        .get_or_insert_with(|| content.contains("use Illuminate\\Container\\Attributes\\"));
    if has_import {
        Some(crate::symbol_map::LaravelStringKind::Config)
    } else {
        None
    }
}

/// If the first argument of `argument_list` is a non-empty, non-interpolated
/// string literal, push a [`SymbolKind::LaravelStringKey`] span covering the
/// string content (inside the quotes) onto `spans`.
///
/// Called by the `config()` function-call extractor and the
/// `Config::get()` / `Config::set()` static-call extractor so that
/// find-references and go-to-definition for Laravel config keys can use
/// the pre-built symbol map instead of re-parsing every file on demand.
pub(super) fn try_emit_laravel_string_span(
    kind: crate::symbol_map::LaravelStringKind,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return;
    };
    let Expression::Literal(literal::Literal::String(s)) = first_arg.value() else {
        return;
    };
    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let key = &content[inner_start as usize..inner_end as usize];
    if key.is_empty() {
        return;
    }

    if kind == crate::symbol_map::LaravelStringKind::Config && !key.contains('.') {
        // Require at least one dot: bare keys like 'app' are not valid config paths.
        return;
    }

    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::LaravelStringKey {
            kind,
            key: key.to_string(),
        },
    });
}

/// If the first argument of `argument_list` is a non-empty, non-interpolated
/// string literal, push a [`SymbolKind::LaravelStringKey`] span covering the
/// string content (inside the quotes) onto `spans`.
///
/// Called by the `config()` function-call extractor and the
/// `Config::get()` / `Config::set()` static-call extractor so that
/// find-references and go-to-definition for Laravel config keys can use
/// the pre-built symbol map instead of re-parsing every file on demand.
pub(super) fn try_emit_laravel_string_span_partial(
    kind: crate::symbol_map::LaravelStringKind,
    argument_list: &PartialArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return;
    };
    let Some(Expression::Literal(literal::Literal::String(s))) = first_arg.value() else {
        return;
    };
    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let key = &content[inner_start as usize..inner_end as usize];
    if key.is_empty() {
        return;
    }

    if kind == crate::symbol_map::LaravelStringKind::Config && !key.contains('.') {
        // Require at least one dot: bare keys like 'app' are not valid config paths.
        return;
    }

    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::LaravelStringKey {
            kind,
            key: key.to_string(),
        },
    });
}

/// If `argument_list` starts with a plain, non-empty string literal, push a
/// [`SymbolKind::LaravelMacroString`] span covering the string content.
pub(super) fn try_emit_laravel_macro_string_span(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return;
    };
    let Expression::Literal(literal::Literal::String(s)) = first_arg.value() else {
        return;
    };
    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let name = &content[inner_start as usize..inner_end as usize];
    if name.is_empty() {
        return;
    }
    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::LaravelMacroString {
            name: name.to_string(),
        },
    });
}

/// Detect an array-callable literal — `[Class::class, 'method']` or
/// `[$object, 'method']` — and emit a [`SymbolKind::MemberAccess`] span over
/// the method-name string so that go-to-definition, references, and rename
/// treat it like a real `Class::method` / `$object->method` reference.
///
/// This is the shape Laravel routes use for controller actions
/// (`Route::get('/', [IndexPageController::class, 'indexPage'])`), but the
/// pattern is generic PHP (event listeners, `array_map`, etc.).
///
/// Only fires for a two-element array whose first element is a `::class`
/// constant or a variable and whose second element is a plain string literal.
pub(super) fn try_emit_array_callable_span(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    // Exactly two positional (value) elements: `[<callee>, '<method>']`.
    let mut values = elements.iter().filter_map(|el| match el {
        ArrayElement::Value(v) => Some(v.value),
        _ => None,
    });
    let (Some(first), Some(second), None) = (values.next(), values.next(), values.next()) else {
        return;
    };
    // The whole array must have been exactly those two elements (no
    // key/value or variadic entries mixed in).
    if elements.iter().count() != 2 {
        return;
    }

    // Second element must be a plain string literal — the method name.
    let Expression::Literal(literal::Literal::String(s)) = second else {
        return;
    };

    // First element determines the subject and access kind.
    let (subject_text, is_static) = match first {
        // `Class::class` — static-style access on the class name.
        Expression::Access(Access::ClassConstant(cca)) => {
            let is_class_const = matches!(
                &cca.constant,
                ClassLikeConstantSelector::Identifier(ident)
                    if bytes_to_str(ident.value).eq_ignore_ascii_case("class")
            );
            if !is_class_const {
                return;
            }
            (expr_to_subject_text(cca.class), true)
        }
        // `$this` / `$object` — instance-style access.
        Expression::Variable(Variable::Direct(dv)) => (bytes_to_str(dv.name).to_string(), false),
        _ => return,
    };
    if subject_text.is_empty() {
        return;
    }

    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let member_name = if let Some(value) = s.value {
        bytes_to_str(value)
    } else {
        &content[inner_start as usize..inner_end as usize]
    };
    // Guard against non-identifier strings (e.g. `[$a, 'some text']`).
    if member_name.is_empty()
        || !member_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        || member_name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    {
        return;
    }

    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::MemberAccess {
            subject_text,
            member_name: member_name.to_string(),
            is_static,
            is_method_call: true,
            is_docblock_reference: false,
            is_array_callable: true,
        },
    });
}

/// If `argument_list` belongs to a `compact()` call, emit one
/// [`SymbolKind::CompactVariable`] span per string-literal variable name,
/// including names inside (possibly nested) array-literal arguments.
pub(super) fn try_emit_compact_string_spans(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    for arg in argument_list.arguments.iter() {
        emit_compact_name_spans(arg.value(), content, spans);
    }
}

/// Emit [`SymbolKind::CompactVariable`] spans for one `compact()` argument:
/// a string literal names a variable directly, and an array literal is
/// descended into recursively so `compact(['a', ['b']])` covers both names.
pub(super) fn emit_compact_name_spans(
    expr: &Expression<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match expr {
        Expression::Literal(literal::Literal::String(s)) => {
            let inner_start = s.span.start.offset + 1;
            let inner_end = s.span.end.offset - 1;
            if inner_start >= inner_end || inner_end as usize > content.len() {
                return;
            }

            let name = if let Some(value) = s.value {
                bytes_to_str(value)
            } else {
                &content[inner_start as usize..inner_end as usize]
            };
            if name.is_empty() {
                return;
            }

            spans.push(SymbolSpan {
                start: inner_start,
                end: inner_end,
                kind: SymbolKind::CompactVariable {
                    name: name.to_string(),
                },
            });
        }
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                emit_compact_name_elem_spans(elem, content, spans);
            }
        }
        Expression::LegacyArray(arr) => {
            for elem in arr.elements.iter() {
                emit_compact_name_elem_spans(elem, content, spans);
            }
        }
        _ => {}
    }
}

/// Emit spans for one element of an array passed to `compact()`. Keys are
/// ignored; values are variable names or nested arrays.
pub(super) fn emit_compact_name_elem_spans(
    elem: &ArrayElement<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match elem {
        ArrayElement::KeyValue(kv) => emit_compact_name_spans(kv.value, content, spans),
        ArrayElement::Value(v) => emit_compact_name_spans(v.value, content, spans),
        ArrayElement::Variadic(s) => emit_compact_name_spans(s.value, content, spans),
        ArrayElement::Missing(_) => {}
    }
}

/// Returns `true` if `name` is a method on Laravel's `Repository` config contract
/// that accepts a config key as its first argument.
pub(super) fn is_config_repository_method(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "has"
            | "get"
            | "string"
            | "integer"
            | "float"
            | "boolean"
            | "array"
            | "collection"
            | "set"
            | "prepend"
            | "push"
    )
}

/// Returns `true` if `object` is a `config()` (or `\config()`) helper call and
/// `member_name` is a config-key-accepting method, e.g. `config()->get('app.name')`.
pub(super) fn is_laravel_config_repository_call(
    object: &Expression<'_>,
    member_name: &str,
) -> bool {
    if !is_config_repository_method(member_name) {
        return false;
    }

    match object {
        Expression::Call(Call::Function(func_call)) => match func_call.function {
            Expression::Identifier(ident) => {
                strip_fqn_prefix(bytes_to_str(ident.value())).eq_ignore_ascii_case("config")
            }
            _ => false,
        },
        _ => false,
    }
}

// ─── Laravel route controller method spans ──────────────────────────────────

/// HTTP method names used in Laravel route definitions.
///
/// `Route::get(…)`, `Route::post(…)`, etc.  `match` is excluded because
/// its action argument is in a different position (3rd, not 2nd).
pub(super) const ROUTE_HTTP_METHODS: &[&str] =
    &["get", "post", "put", "patch", "delete", "options", "any"];

/// Walk a fluent method chain backwards looking for `->controller(X::class)`.
///
/// Returns the class name (as written in source, e.g. `"WorkItemController"`)
/// if found, `None` otherwise.  Handles both instance-method chains
/// (`->prefix('…')->controller(X::class)`) and a static entry point
/// (`Route::controller(X::class)`).
pub(super) fn laravel_route_find_controller_in_chain(expr: &Expression<'_>) -> Option<String> {
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
pub(super) fn laravel_route_extract_class_arg(args: &ArgumentList<'_>) -> Option<String> {
    let first_arg = args.arguments.iter().next()?;
    if let Expression::Access(Access::ClassConstant(cca)) = first_arg.value() {
        let is_class = matches!(
            &cca.constant,
            ClassLikeConstantSelector::Identifier(ident)
                if bytes_to_str(ident.value).eq_ignore_ascii_case("class")
        );
        if is_class {
            let name = expr_to_subject_text(cca.class);
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Walk the closure (or arrow function) body passed to `->group()`.
pub(super) fn laravel_route_scan_group_body(
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
pub(super) fn laravel_route_scan_stmt(
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
            for s in if_stmt.body.statements() {
                laravel_route_scan_stmt(s, controller, content, spans);
            }
            for stmts in if_stmt.body.else_if_statements() {
                for s in stmts {
                    laravel_route_scan_stmt(s, controller, content, spans);
                }
            }
            if let Some(else_stmts) = if_stmt.body.else_statements() {
                for s in else_stmts {
                    laravel_route_scan_stmt(s, controller, content, spans);
                }
            }
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
pub(super) fn laravel_route_scan_expr(
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
            let Expression::Literal(literal::Literal::String(s)) = action_arg.value() else {
                return;
            };

            let inner_start = s.span.start.offset + 1;
            let inner_end = s.span.end.offset - 1;
            if inner_start >= inner_end || inner_end as usize > content.len() {
                return;
            }
            let method_name = if let Some(value) = s.value {
                bytes_to_str(value)
            } else {
                &content[inner_start as usize..inner_end as usize]
            };
            // Must look like a valid PHP method name.
            if method_name.is_empty()
                || !method_name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                || method_name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
            {
                return;
            }

            spans.push(SymbolSpan {
                start: inner_start,
                end: inner_end,
                kind: SymbolKind::MemberAccess {
                    subject_text: controller.to_string(),
                    member_name: method_name.to_string(),
                    is_static: true,
                    is_method_call: true,
                    is_docblock_reference: false,
                    is_array_callable: false,
                },
            });
        }
        _ => {}
    }
}
