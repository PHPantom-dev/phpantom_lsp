/// Array literal inference and array function helpers.
///
/// These are utility helpers that support the forward-walking variable
/// resolver in [`super::forward_walk`] and the foreach/destructuring
/// resolution module.
use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::array_func_rules::{ArrayFuncArgs, array_func_element_type, array_func_raw_type};

use crate::atom::{atom, bytes_to_str};
use crate::docblock;
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;

use crate::type_engine::resolver::VarResolutionCtx;
use crate::types::ResolvedType;

/// Infer the raw PHPStan-style type for an array literal (`[…]` or
/// `array(…)`) from its keys and value expressions.
pub(in crate::type_engine) fn infer_array_literal_raw_type<'b>(
    elements: impl Iterator<Item = &'b ArrayElement<'b>>,
    ctx: &VarResolutionCtx<'_>,
    nested: bool,
) -> Option<PhpType> {
    // Maximum number of positional entries to record as a tuple-style
    // shape. Beyond this the array is almost certainly a homogeneous
    // collection rather than a fixed-arity tuple, so it is widened to
    // `list<T>` to avoid unbounded shape growth.
    const MAX_POSITIONAL_SHAPE_LEN: usize = 32;

    let mut types: Vec<PhpType> = Vec::new();
    let mut positional: Vec<PhpType> = Vec::new();
    let mut has_string_keys = false;
    let mut saw_spread = false;
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
                let resolved = infer_element_type(v.value, ctx);
                // A positional shape must keep one entry per element to
                // preserve arity, so an unresolvable element becomes
                // `mixed`. The `list<T>` fallback keeps its original
                // behaviour of ignoring unresolvable elements.
                positional.push(resolved.clone().unwrap_or_else(PhpType::mixed));
                if let Some(t) = resolved
                    && !types.contains(&t)
                {
                    types.push(t);
                }
            }
            ArrayElement::Variadic(v) => {
                // Spread: `...$other` — try to resolve iterable element type.
                saw_spread = true;
                if let Some(raw) = super::foreach_resolution::resolve_expression_type(v.value, ctx)
                    && let Some(elem) = raw
                        .iterable_element_type()
                        .map(|element| element.widen_scalar_literals())
                    && !types.contains(&elem)
                {
                    types.push(elem);
                }
            }
            ArrayElement::Missing(_) => {}
        }
    }

    if has_string_keys && !shape_entries.is_empty() {
        return Some(PhpType::array_shape(shape_entries));
    }

    if types.is_empty() {
        return None;
    }

    // Nested value-only literal with a fixed set of elements: record it
    // as a positional (tuple-style) array shape so that integer-literal
    // indexing (`$pair[1]`) resolves the element at that position and
    // out-of-bounds indices are known to be absent. A spread element or
    // an over-long literal makes the arity indeterminate, so those widen
    // to `list<T>` instead.
    //
    // A top-level literal (one assigned or returned directly) is
    // generalized to `list<T>` because that is what most consumers
    // expect for a freshly constructed array (return-type inference,
    // push tracking via `$arr[] = …`, and hover). Nested literals keep
    // their precise arity because they are typically fixed tuples read
    // back by position.
    if nested
        && !saw_spread
        && !positional.is_empty()
        && positional.len() <= MAX_POSITIONAL_SHAPE_LEN
    {
        let entries = positional
            .into_iter()
            .map(|value_type| crate::php_type::ShapeEntry {
                key: None,
                value_type,
                optional: false,
            })
            .collect();
        return Some(PhpType::array_shape(entries));
    }

    let elem_type = if types.len() == 1 {
        types.into_iter().next().unwrap()
    } else {
        PhpType::union(types)
    };
    Some(PhpType::list(elem_type))
}

/// Extract a string representation of an array key expression.
fn extract_array_key_text<'b>(key: &'b Expression<'b>) -> String {
    match key {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the unquoted content; fall back to unquoting `raw`.
            s.value
                .map(|v| bytes_to_str(v).to_string())
                .unwrap_or_else(|| {
                    crate::text_scan::unquote_php_string(bytes_to_str(s.raw))
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
    infer_element_type_precise(value, ctx).map(|ty| ty.widen_scalar_literals())
}

/// Resolve an array element before the collection storage boundary widens it.
fn infer_element_type_precise<'b>(
    value: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    match value {
        // ── Nested array literals ──
        Expression::Array(arr) => infer_array_literal_raw_type(arr.elements.iter(), ctx, true)
            .or_else(|| Some(PhpType::array())),
        Expression::LegacyArray(arr) => {
            infer_array_literal_raw_type(arr.elements.iter(), ctx, true)
                .or_else(|| Some(PhpType::array()))
        }
        // ── Object instantiation ──
        Expression::Instantiation(inst) => match inst.class {
            Expression::Identifier(ident) => {
                let name = bytes_to_str(ident.value()).to_string();
                let fqn = crate::util::resolve_source_class_name(
                    &name,
                    ctx.current_class.file_namespace.as_deref(),
                    ctx.class_loader,
                );
                Some(PhpType::named(atom(&fqn)))
            }
            Expression::Self_(_) => Some(PhpType::named(atom(ctx.current_class.name.as_ref()))),
            Expression::Static(_) => Some(PhpType::named(atom(ctx.current_class.name.as_ref()))),
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
            crate::type_engine::variable::resolution::resolve_variable_php_type(
                &var_text,
                ctx.content,
                offset as u32,
                current_class,
                ctx.all_classes,
                ctx.class_loader,
                ctx.backend,
                crate::type_engine::resolver::Loaders::with_function(ctx.function_loader()),
            )
        }
        // ── Parenthesized ──
        Expression::Parenthesized(p) => infer_element_type_precise(p.expression, ctx),
        // ── Property access, method calls on objects, etc. ──
        // Delegate to the unified pipeline which resolves property
        // type hints and method return types through the class
        // hierarchy.
        _ => super::foreach_resolution::resolve_expression_type(value, ctx),
    }
}

/// [`ArrayFuncArgs`] over a parsed argument list.
struct AstArrayFuncArgs<'a, 'ast, 'ctx> {
    args: &'a ArgumentList<'ast>,
    ctx: &'a VarResolutionCtx<'ctx>,
}

impl ArrayFuncArgs for AstArrayFuncArgs<'_, '_, '_> {
    fn arg_raw_type(&self, index: usize) -> Option<PhpType> {
        let expr = super::resolution::nth_arg_expr(self.args, index)?;
        super::resolution::resolve_arg_raw_type(expr, self.ctx)
    }

    fn is_false_literal(&self, index: usize) -> bool {
        matches!(
            super::resolution::nth_arg_expr(self.args, index),
            Some(Expression::Literal(Literal::False(_)))
        )
    }

    fn callback_declared_return_type(&self, index: usize) -> Option<PhpType> {
        match super::resolution::nth_arg_expr(self.args, index)? {
            Expression::Closure(closure) => closure
                .return_type_hint
                .as_ref()
                .map(|rth| extract_hint_type(&rth.hint)),
            Expression::ArrowFunction(arrow) => arrow
                .return_type_hint
                .as_ref()
                .map(|rth| extract_hint_type(&rth.hint)),
            _ => None,
        }
    }

    fn callback_inferred_return_type(&self, index: usize, param_type: &PhpType) -> Option<PhpType> {
        let expr = super::resolution::nth_arg_expr(self.args, index)?;
        infer_callback_return_type(expr, param_type, self.ctx)
    }
}

/// For known array-producing functions, resolve the **raw output type**
/// (e.g. `list<User>`) from the input arguments.
///
/// Used by foreach and destructuring resolution so that iterating over
/// `array_filter(...)` etc. preserves element types.  Element-extracting
/// functions are handled by [`resolve_array_func_element_type`], which the
/// caller consults first.
pub(in crate::type_engine) fn resolve_array_func_raw_type(
    func_name: &str,
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    array_func_raw_type(func_name, &AstArrayFuncArgs { args, ctx })
}

/// For known array functions, resolve the **element type**
/// (e.g. `User`) of the output.
///
/// Used by `resolve_rhs_expression` so that `$item = array_pop($users)`
/// resolves `$item` to `User`.
pub(in crate::type_engine) fn resolve_array_func_element_type(
    func_name: &str,
    args: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    array_func_element_type(func_name, &AstArrayFuncArgs { args, ctx })
}

/// Extract per-argument source text from a parsed `ArgumentList`.
///
/// Returns one `String` per argument by walking the AST nodes and
/// extracting their spans. This avoids serialising the argument list
/// to a flat string and then re-splitting with `split_text_args`.
pub(in crate::type_engine) fn extract_arg_texts_from_ast(
    argument_list: &mago_syntax::cst::ArgumentList<'_>,
    content: &str,
) -> Vec<String> {
    argument_list
        .arguments
        .iter()
        .map(|arg| {
            let value_span = match arg {
                mago_syntax::cst::argument::Argument::Positional(pos) => pos.value.span(),
                mago_syntax::cst::argument::Argument::Named(named) => named.value.span(),
            };
            let start = value_span.start.offset as usize;
            let end = value_span.end.offset as usize;
            let value = if end <= content.len() {
                &content[start..end]
            } else {
                ""
            };
            // Preserve the `name:` prefix for named arguments so that
            // downstream argument binding (`bind_text_args_to_params`) can
            // route them to the parameter they target rather than their
            // source-order slot. Without it, `f(b: 1, a: 2)` would bind `a`
            // to the value `1` and misresolve conditional return types and
            // template parameters that key on `a`.
            match arg {
                mago_syntax::cst::argument::Argument::Named(named) => {
                    let name = crate::atom::bytes_to_str(named.name.value);
                    format!("{name}: {value}")
                }
                mago_syntax::cst::argument::Argument::Positional(_) => value.to_string(),
            }
        })
        .collect()
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
        backend: ctx.backend,
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
