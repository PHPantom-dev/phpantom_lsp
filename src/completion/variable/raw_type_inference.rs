/// Array literal inference, array function helpers, and generator yield
/// reverse-inference.
///
/// These are utility helpers that support the forward-walking variable
/// resolver in [`super::forward_walk`] and the foreach/destructuring
/// resolution module.
use mago_span::HasSpan;
use mago_syntax::ast::*;

use super::{ARRAY_ELEMENT_FUNCS, ARRAY_PRESERVING_FUNCS};

use crate::atom::bytes_to_str;
use crate::docblock;
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;

use crate::completion::resolver::VarResolutionCtx;
use crate::types::ResolvedType;

/// Infer the raw PHPStan-style type string for an array literal
/// (`[…]` or `array(…)`) by examining its keys and resolving value
/// elements by resolving each value expression.
pub(in crate::completion) fn infer_array_literal_raw_type<'b>(
    elements: impl Iterator<Item = &'b ArrayElement<'b>>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let mut types: Vec<PhpType> = Vec::new();
    let mut has_string_keys = false;
    let mut shape_entries: Vec<crate::php_type::ShapeEntry> = Vec::new();

    for elem in elements {
        match elem {
            ArrayElement::KeyValue(kv) => {
                has_string_keys = true;
                let key_text = extract_array_key_text(kv.key);
                let value_type = infer_element_type(kv.value, ctx).unwrap_or_else(PhpType::mixed);
                shape_entries.push(crate::php_type::ShapeEntry {
                    key: Some(key_text),
                    value_type,
                    optional: false,
                });
            }
            ArrayElement::Value(v) => {
                if let Some(t) = infer_element_type(v.value, ctx)
                    && !types.contains(&t)
                {
                    types.push(t);
                }
            }
            ArrayElement::Variadic(v) => {
                // Spread: `...$other` — try to resolve iterable element type.
                if let Some(raw) = super::foreach_resolution::resolve_expression_type(v.value, ctx)
                    && let Some(elem) = raw.extract_value_type(true).cloned()
                    && !types.contains(&elem)
                {
                    types.push(elem);
                }
            }
            ArrayElement::Missing(_) => {}
        }
    }

    if has_string_keys && !shape_entries.is_empty() {
        return Some(PhpType::ArrayShape(shape_entries));
    }

    if types.is_empty() {
        return None;
    }

    let elem_type = if types.len() == 1 {
        types.into_iter().next().unwrap()
    } else {
        PhpType::Union(types)
    };
    Some(PhpType::list(elem_type))
}

/// Extract a string representation of an array key expression.
fn extract_array_key_text<'b>(key: &'b Expression<'b>) -> String {
    match key {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the unquoted content; fall back to `raw` trimmed.
            s.value
                .map(|v| bytes_to_str(v).to_string())
                .unwrap_or_else(|| {
                    crate::util::unquote_php_string(bytes_to_str(s.raw))
                        .unwrap_or(bytes_to_str(s.raw))
                        .to_string()
                })
        }
        Expression::Literal(Literal::Integer(i)) => bytes_to_str(i.raw).to_string(),
        _ => PhpType::mixed().to_string(),
    }
}

/// Infer the type of a single array element value expression.
fn infer_element_type<'b>(
    value: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    match value {
        // ── Scalar literals ──
        Expression::Literal(Literal::String(_)) => Some(PhpType::string()),
        Expression::Literal(Literal::Integer(_)) => Some(PhpType::int()),
        Expression::Literal(Literal::Float(_)) => Some(PhpType::float()),
        Expression::Literal(Literal::True(_) | Literal::False(_)) => Some(PhpType::bool()),
        Expression::Literal(Literal::Null(_)) => Some(PhpType::null()),
        // ── Nested array literals ──
        Expression::Array(arr) => infer_array_literal_raw_type(arr.elements.iter(), ctx)
            .or_else(|| Some(PhpType::array())),
        Expression::LegacyArray(arr) => infer_array_literal_raw_type(arr.elements.iter(), ctx)
            .or_else(|| Some(PhpType::array())),
        // ── Object instantiation ──
        Expression::Instantiation(inst) => match inst.class {
            Expression::Identifier(ident) => {
                let name = bytes_to_str(ident.value()).to_string();
                let fqn = crate::util::resolve_name_via_loader(&name, ctx.class_loader);
                Some(PhpType::Named(fqn))
            }
            Expression::Self_(_) => Some(PhpType::Named(ctx.current_class.name.to_string())),
            Expression::Static(_) => Some(PhpType::Named(ctx.current_class.name.to_string())),
            _ => None,
        },
        Expression::Call(_) => {
            // Resolve call return type via the unified pipeline.
            super::foreach_resolution::resolve_expression_type(value, ctx)
        }
        Expression::Variable(Variable::Direct(dv)) => {
            let var_text = bytes_to_str(dv.name).to_string();
            let offset = value.span().start.offset as usize;
            // Try iterable docblock first (e.g. `@var list<User> $items`).
            if let Some(t) =
                docblock::find_iterable_raw_type_in_source(ctx.content, offset, &var_text)
            {
                return Some(crate::util::resolve_php_type_names(&t, ctx.class_loader));
            }
            // When a scope variable resolver is available (i.e. we are
            // inside the forward walker), read the variable's type
            // directly from the in-progress ScopeState instead of
            // calling the full resolution pipeline which would trigger
            // a recursive method-body walk.
            if let Some(resolver) = ctx.scope_var_resolver {
                let prefixed = if var_text.starts_with('$') {
                    var_text.clone()
                } else {
                    format!("${}", var_text)
                };
                let from_scope = resolver(&prefixed);
                if !from_scope.is_empty() {
                    return Some(crate::types::ResolvedType::types_joined(&from_scope));
                }
                return None;
            }
            // Fall back to the full variable type resolution pipeline
            // (parameter type hints, @param docblocks, assignments,
            // foreach bindings, etc.).  This handles cases like
            // `string $trackingUserId` where the variable is a scalar
            // parameter, not an iterable.
            let current_class = ctx
                .all_classes
                .iter()
                .find(|c| c.name == ctx.current_class.name)
                .map(|c| c.as_ref());
            crate::completion::variable::resolution::resolve_variable_php_type(
                &var_text,
                ctx.content,
                offset as u32,
                current_class,
                ctx.all_classes,
                ctx.class_loader,
                crate::completion::resolver::Loaders::with_function(ctx.function_loader()),
            )
        }
        // ── Parenthesized ──
        Expression::Parenthesized(p) => infer_element_type(p.expression, ctx),
        // ── Property access, method calls on objects, etc. ──
        // Delegate to the unified pipeline which resolves property
        // type hints and method return types through the class
        // hierarchy.
        _ => super::foreach_resolution::resolve_expression_type(value, ctx),
    }
}

/// For known array functions, resolve the **raw output type** string
/// (e.g. `"list<User>"`) from the input arguments.
///
/// Used by foreach and destructuring resolution so that iterating over
/// `array_filter(...)` etc. preserves element types.
pub(in crate::completion) fn resolve_array_func_raw_type(
    func_name: &str,
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    // Type-preserving functions: output array has same element type.
    if ARRAY_PRESERVING_FUNCS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(func_name))
    {
        let arr_expr = super::resolution::first_arg_expr(args)?;
        let raw = super::resolution::resolve_arg_raw_type(arr_expr, ctx)?;
        // If the raw type already has generic params, return it as-is
        // so downstream `PhpType::extract_value_type` can extract the
        // element type.  Otherwise it's a plain class name and we
        // can't infer element type.
        if raw.extract_value_type(true).is_some() {
            return Some(raw);
        }
    }

    // array_map: callback is first arg, array is second.
    // The callback's return type determines the output element type.
    if func_name.eq_ignore_ascii_case("array_map")
        && let Some(element_type) = extract_array_map_element_type(args, ctx)
    {
        return Some(PhpType::list(element_type));
    }

    // iterator_to_array: converts an iterator to an array, preserving
    // the value type.  `iterator_to_array($iter)` where `$iter` is
    // `Iterator<int, Foo>` produces `array<int, Foo>`.
    if func_name.eq_ignore_ascii_case("iterator_to_array") {
        let iter_expr = super::resolution::first_arg_expr(args)?;
        let raw = super::resolution::resolve_arg_raw_type(iter_expr, ctx)?;
        if raw.extract_value_type(true).is_some() {
            return Some(raw);
        }
    }

    // Element-extracting functions: wrap element type in list<> so
    // it can be used as an iterable raw type.
    if ARRAY_ELEMENT_FUNCS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(func_name))
    {
        let arr_expr = super::resolution::first_arg_expr(args)?;
        let raw = super::resolution::resolve_arg_raw_type(arr_expr, ctx)?;
        if raw.extract_value_type(true).is_some() {
            return Some(raw);
        }
    }

    None
}

/// For known array functions, resolve the **element type** string
/// (e.g. `"User"`) for the output.
///
/// Used by `resolve_rhs_expression` so that `$item = array_pop($users)`
/// resolves `$item` to `User`.  This only covers true element-extracting
/// functions (array_pop, current, etc.) that return a single element.
///
/// Array-producing functions like `array_map` and `iterator_to_array`
/// are handled exclusively by [`resolve_array_func_raw_type`] which
/// preserves the container type (e.g. `list<User>`).  Returning the
/// element type here would lose the array wrapper and break downstream
/// consumers that need to walk bracket segments (e.g. `$result[0]->`).
pub(in crate::completion) fn resolve_array_func_element_type(
    func_name: &str,
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    // Element-extracting functions: return the element type directly.
    if ARRAY_ELEMENT_FUNCS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(func_name))
    {
        let arr_expr = super::resolution::first_arg_expr(args)?;
        let raw = super::resolution::resolve_arg_raw_type(arr_expr, ctx)?;
        return raw.extract_value_type(true).cloned();
    }

    None
}

/// Extract per-argument source text from a parsed `ArgumentList`.
///
/// Returns one `String` per argument by walking the AST nodes and
/// extracting their spans. This avoids serialising the argument list
/// to a flat string and then re-splitting with `split_text_args`.
pub(in crate::completion) fn extract_arg_texts_from_ast(
    argument_list: &mago_syntax::ast::ArgumentList<'_>,
    content: &str,
) -> Vec<String> {
    argument_list
        .arguments
        .iter()
        .map(|arg| {
            let span = match arg {
                mago_syntax::ast::argument::Argument::Positional(pos) => pos.value.span(),
                mago_syntax::ast::argument::Argument::Named(named) => named.value.span(),
            };
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            if end <= content.len() {
                content[start..end].to_string()
            } else {
                String::new()
            }
        })
        .collect()
}

/// Extract the output element type for `array_map($callback, $array)`.
///
/// Strategy:
/// 1. If the callback (first arg) is a closure/arrow function with a
///    return type hint, use that.
/// 2. Otherwise, fall back to the **input array's** element type
///    (assumes the callback preserves type, which is a reasonable
///    default when no return type is declared).
fn extract_array_map_element_type(
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let callback_expr = super::resolution::first_arg_expr(args)?;

    // Try to get the callback's return type hint.
    let return_hint = match callback_expr {
        Expression::Closure(closure) => closure
            .return_type_hint
            .as_ref()
            .map(|rth| extract_hint_type(&rth.hint)),
        Expression::ArrowFunction(arrow) => arrow
            .return_type_hint
            .as_ref()
            .map(|rth| extract_hint_type(&rth.hint)),
        _ => None,
    };

    if let Some(ref parsed) = return_hint
        && !parsed.is_untyped()
    {
        return return_hint;
    }

    // No explicit return type — try to infer it from the callback body
    // by resolving the body expression with the callback parameter
    // seeded to the input array's element type.
    let arr_expr = super::resolution::nth_arg_expr(args, 1)?;
    let input_raw = super::resolution::resolve_arg_raw_type(arr_expr, ctx)?;
    let input_element = input_raw.extract_value_type(true)?.clone();

    if let Some(inferred) = infer_callback_return_type(callback_expr, &input_element, ctx) {
        return Some(inferred);
    }

    // Final fallback: use the input array's element type.
    Some(input_element)
}

/// Infer the return type of a callback (arrow function or closure) by
/// resolving its body expression with the first parameter seeded to
/// `param_type`.
///
/// For arrow functions: resolves `arrow.expression` directly.
/// For closures: finds the first `return` statement and resolves its
/// expression.
fn infer_callback_return_type(
    callback_expr: &Expression<'_>,
    param_type: &PhpType,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    let (param_name, body_expr) = match callback_expr {
        Expression::ArrowFunction(arrow) => {
            let param = arrow.parameter_list.parameters.first()?;
            let name = bytes_to_str(param.variable.name).to_string();
            (name, arrow.expression)
        }
        Expression::Closure(closure) => {
            let param = closure.parameter_list.parameters.first()?;
            let name = bytes_to_str(param.variable.name).to_string();
            // Find the first return statement's expression.
            let ret_expr = closure.body.statements.iter().find_map(|stmt| {
                if let Statement::Return(ret) = stmt {
                    ret.value.as_ref()
                } else {
                    None
                }
            })?;
            (name, *ret_expr)
        }
        _ => return None,
    };

    // Build a scope resolver that maps the callback parameter to the
    // input element type.  Include ClassInfo when available so that
    // property access resolution can find the class members.
    let resolved_param = if let Some(class_name) = param_type.base_name() {
        if let Some(cls) = (ctx.class_loader)(class_name) {
            vec![ResolvedType::from_both(param_type.clone(), (*cls).clone())]
        } else {
            vec![ResolvedType::from_type_string(param_type.clone())]
        }
    } else {
        vec![ResolvedType::from_type_string(param_type.clone())]
    };
    let scope_resolver = move |var: &str| -> Vec<ResolvedType> {
        if var == param_name {
            resolved_param.clone()
        } else {
            vec![]
        }
    };

    // Create a synthetic context with the scope resolver.
    let body_offset = body_expr.span().start.offset;
    let infer_ctx = VarResolutionCtx {
        var_name: "",
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: body_offset,
        class_loader: ctx.class_loader,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: None,
        top_level_scope: None,
        branch_aware: false,
        match_arm_narrowing: std::collections::HashMap::new(),
        scope_var_resolver: Some(&scope_resolver),
    };

    super::foreach_resolution::resolve_expression_type(body_expr, &infer_ctx)
}
