use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use super::*;

// ─── Expression to subject text ─────────────────────────────────────────────

/// Convert an AST expression to the subject text string that
/// `resolve_target_classes` expects.
pub(super) fn expr_to_subject_text(expr: &Expression<'_>) -> String {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        Expression::Self_(_) => "self".to_string(),
        Expression::Static(_) => "static".to_string(),
        Expression::Parent(_) => "parent".to_string(),
        Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),

        Expression::Access(Access::Property(pa)) => {
            let obj = expr_to_subject_text(pa.object);
            if let ClassLikeMemberSelector::Identifier(ident) = &pa.property {
                format!("{}->{}", obj, bytes_to_str(ident.value))
            } else {
                obj
            }
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            let obj = expr_to_subject_text(pa.object);
            if let ClassLikeMemberSelector::Identifier(ident) = &pa.property {
                format!("{}?->{}", obj, bytes_to_str(ident.value))
            } else {
                obj
            }
        }
        Expression::Access(Access::StaticProperty(spa)) => {
            let class_text = expr_to_subject_text(spa.class);
            if let Variable::Direct(dv) = &spa.property {
                format!("{}::{}", class_text, bytes_to_str(dv.name))
            } else {
                class_text
            }
        }
        Expression::Access(Access::ClassConstant(cca)) => {
            let class_text = expr_to_subject_text(cca.class);
            match &cca.constant {
                ClassLikeConstantSelector::Identifier(ident) => {
                    format!("{}::{}", class_text, bytes_to_str(ident.value))
                }
                _ => class_text,
            }
        }

        Expression::Call(Call::Method(mc)) => {
            let obj = expr_to_subject_text(mc.object);
            if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                let args_text = format_all_call_args(&mc.argument_list.arguments);
                format!("{}->{}({})", obj, bytes_to_str(ident.value), args_text)
            } else {
                format!("{}->?()", obj)
            }
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            let obj = expr_to_subject_text(mc.object);
            if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                let args_text = format_all_call_args(&mc.argument_list.arguments);
                format!("{}?->{}({})", obj, bytes_to_str(ident.value), args_text)
            } else {
                format!("{}?->?()", obj)
            }
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            let class_text = expr_to_subject_text(sc.class);
            if let ClassLikeMemberSelector::Identifier(ident) = &sc.method {
                let args_text = format_all_call_args(&sc.argument_list.arguments);
                format!(
                    "{}::{}({})",
                    class_text,
                    bytes_to_str(ident.value),
                    args_text
                )
            } else {
                format!("{}::?()", class_text)
            }
        }
        Expression::Call(Call::Function(fc)) => {
            let func_text = expr_to_subject_text(fc.function);
            let args_text = format_all_call_args(&fc.argument_list.arguments);
            // When the callee is a parenthesized expression (e.g.
            // `($this->formatter)()`), wrap the inner text back in
            // parens so that `SubjectExpr::parse` sees
            // `($this->formatter)()` rather than `$this->formatter()`
            // (which would be parsed as a method call).
            if matches!(fc.function, Expression::Parenthesized(_)) {
                format!("({})({})", func_text, args_text)
            } else {
                format!("{}({})", func_text, args_text)
            }
        }

        Expression::Instantiation(inst) => expr_to_subject_text(inst.class),

        Expression::Parenthesized(paren) => expr_to_subject_text(paren.expression),

        // `clone $expr` preserves the type of the operand.
        Expression::Clone(clone) => expr_to_subject_text(clone.object),

        // Array literals: `[Foo::class, 'bar']` → `[Foo::class, 'bar']`.
        // We format elements we can represent and elide the rest so that
        // callers (especially conditional return-type resolution) can see
        // that an argument was provided and is not null.
        Expression::Array(array) => {
            let mut parts = Vec::new();
            for element in array.elements.iter() {
                match element {
                    mago_syntax::cst::ArrayElement::KeyValue(kv) => {
                        let val = expr_to_subject_text(kv.value);
                        if !val.is_empty() {
                            let key = expr_to_subject_text(kv.key);
                            if key.is_empty() {
                                parts.push(val);
                            } else {
                                parts.push(format!("{} => {}", key, val));
                            }
                        } else {
                            parts.push("...".to_string());
                        }
                    }
                    mago_syntax::cst::ArrayElement::Value(v) => {
                        let val = expr_to_subject_text(v.value);
                        if val.is_empty() {
                            parts.push("...".to_string());
                        } else {
                            parts.push(val);
                        }
                    }
                    mago_syntax::cst::ArrayElement::Variadic(v) => {
                        let val = expr_to_subject_text(v.value);
                        if val.is_empty() {
                            parts.push("...".to_string());
                        } else {
                            parts.push(format!("...{}", val));
                        }
                    }
                    mago_syntax::cst::ArrayElement::Missing(_) => {
                        parts.push("...".to_string());
                    }
                }
            }
            format!("[{}]", parts.join(", "))
        }

        // Ternary `$a ? $b : $c` and short ternary `$a ?: $b`.
        // For short ternary (`then` is None), the condition is the
        // preferred branch; for full ternary, use the `then` branch.
        // Either way we pick one branch so the type engine has
        // something to resolve rather than an empty string.
        Expression::Conditional(cond) => {
            let preferred = cond.then.unwrap_or(cond.condition);
            let text = expr_to_subject_text(preferred);
            if !text.is_empty() {
                return text;
            }
            // Fall back to the else branch.
            expr_to_subject_text(cond.r#else)
        }

        // Null coalesce `$a ?? $b` — LHS is the preferred non-null value.
        Expression::Binary(binary) if binary.operator.is_null_coalesce() => {
            let text = expr_to_subject_text(binary.lhs);
            if !text.is_empty() {
                return text;
            }
            expr_to_subject_text(binary.rhs)
        }

        Expression::ArrayAccess(access) => {
            let base = expr_to_subject_text(access.array);
            if base.is_empty() {
                return String::new();
            }
            // Preserve string keys for array-shape resolution;
            // collapse everything else to `[]` (generic element access),
            // matching the convention used by `extract_arrow_subject`.
            let bracket = match access.index {
                Expression::Literal(Literal::String(s)) => {
                    // `s.raw` includes surrounding quotes (e.g. `'key'`).
                    // Strip them to get the bare key, then re-wrap in
                    // single quotes for the subject format.
                    let raw_str = bytes_to_str(s.raw);
                    let inner = crate::util::unquote_php_string(raw_str).unwrap_or(raw_str);
                    format!("['{}']", inner)
                }
                // Preserve integer-literal indices so downstream
                // narrowing can address a specific element (e.g.
                // `$stmts[0]` after `if (! $stmts[0] instanceof Foo)`).
                Expression::Literal(Literal::Integer(i)) => {
                    let n = i
                        .value
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| bytes_to_str(i.raw).to_string());
                    format!("[{}]", n)
                }
                _ => "[]".to_string(),
            };
            format!("{}{}", base, bracket)
        }

        _ => String::new(),
    }
}

/// Format all arguments of a call expression as a comma-separated string.
///
/// Each argument is serialized to a text representation that preserves
/// enough information for downstream consumers:
/// - Conditional return-type resolution needs the first argument value
///   (`Foo::class`, string literals, `null`, etc.)
/// - Template parameter inference needs closure/arrow-function signatures
///   (parameter types and return type) and constructor calls (`new Foo()`)
///
/// When an argument cannot be represented, it is emitted as `...` so that
/// positional indices remain correct for template binding resolution.
pub(super) fn format_all_call_args(args: &TokenSeparatedSequence<'_, Argument<'_>>) -> String {
    let mut parts = Vec::new();
    for arg in args.iter() {
        let arg_expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        let text = format_arg_expr(arg_expr);
        parts.push(text);
    }
    // Trim trailing `...` placeholders beyond the first argument so
    // that multi-arg calls like `method(Foo::class, ...)` don't grow
    // a long tail of placeholders, but a single unknown argument still
    // produces `func(...)` rather than `func()` (which would look like
    // a no-arg call and break conditional return-type resolution).
    while parts.len() > 1 && parts.last().is_some_and(|p| p == "...") {
        parts.pop();
    }
    parts.join(", ")
}

/// Format a single argument expression to text.
///
/// Handles the same cases as the old `format_first_class_arg` plus
/// closure and arrow-function expressions.  For closures the full body
/// is replaced with a placeholder (`=> ...` or `{ ... }`) to keep the
/// subject text compact while preserving parameter types and return
/// type annotations that template inference depends on.
pub(super) fn format_arg_expr(expr: &Expression<'_>) -> String {
    match expr {
        // Foo::class
        Expression::Access(Access::ClassConstant(cca)) => {
            if let ClassLikeConstantSelector::Identifier(ident) = &cca.constant
                && ident.value == b"class"
            {
                let class_text = expr_to_subject_text(cca.class);
                return format!("{}::class", class_text);
            }
            "...".to_string()
        }
        // String literals: 'web', "guard"
        Expression::Literal(Literal::String(lit_str)) => bytes_to_str(lit_str.raw).to_string(),
        // Integer literals: 0, 42
        Expression::Literal(Literal::Integer(lit_int)) => bytes_to_str(lit_int.raw).to_string(),
        // Float literals: 3.14
        Expression::Literal(Literal::Float(lit_float)) => bytes_to_str(lit_float.raw).to_string(),
        // null
        Expression::Literal(Literal::Null(_)) => "null".to_string(),
        // true
        Expression::Literal(Literal::True(_)) => "true".to_string(),
        // false
        Expression::Literal(Literal::False(_)) => "false".to_string(),
        // $variable
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        // new ClassName(…) → "new ClassName()"
        Expression::Instantiation(inst) => {
            let class_text = expr_to_subject_text(inst.class);
            if class_text.is_empty() {
                "...".to_string()
            } else {
                format!("new {}()", class_text)
            }
        }
        // Arrow function: fn(Type $a, Type $b): ReturnType => …
        // Serialize the signature so template inference can extract
        // parameter types and the return type annotation.
        Expression::ArrowFunction(arrow) => {
            let params = format_callable_params(&arrow.parameter_list);
            let ret = arrow
                .return_type_hint
                .as_ref()
                .map(|rth| format!(": {}", crate::parser::extract_hint_type(&rth.hint)))
                .unwrap_or_default();
            format!("fn({}){} => ...", params, ret)
        }
        // Closure: function(Type $a, Type $b): ReturnType { … }
        Expression::Closure(closure) => {
            let params = format_callable_params(&closure.parameter_list);
            let ret = closure
                .return_type_hint
                .as_ref()
                .map(|rth| format!(": {}", crate::parser::extract_hint_type(&rth.hint)))
                .unwrap_or_default();
            format!("function({}){} {{ ... }}", params, ret)
        }
        // Any other expression — delegate to the general subject text
        // formatter.  Falls back to `...` when it can't be represented.
        _ => {
            let text = expr_to_subject_text(expr);
            if text.is_empty() {
                "...".to_string()
            } else {
                text
            }
        }
    }
}

/// Format a callable's parameter list as a comma-separated string of
/// `Type $name` pairs, preserving type annotations for template inference.
pub(super) fn format_callable_params(params: &FunctionLikeParameterList<'_>) -> String {
    let mut parts = Vec::new();
    for param in params.parameters.iter() {
        let name = bytes_to_str(param.variable.name).to_string();
        let type_text = param
            .hint
            .as_ref()
            .map(|h| crate::parser::extract_hint_type(h).to_string());
        match type_text {
            Some(t) => parts.push(format!("{} {}", t, name)),
            None => parts.push(name),
        }
    }
    parts.join(", ")
}

/// Check whether `expr` is an `assert(… instanceof …)` call.
///
/// Returns `true` for patterns like:
/// - `assert($var instanceof Foo)`
/// - `assert($var instanceof Foo || $var instanceof Bar)`
///
/// This is intentionally loose — it does not check which variable is
/// being narrowed.  The diagnostic cache uses the result only to know
/// that *some* assert-instanceof boundary exists at this offset, which
/// is enough to split cache entries before vs after the assert.
pub(super) fn is_assert_instanceof(expr: &Expression<'_>) -> bool {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    if let Expression::Call(Call::Function(func_call)) = expr {
        let func_name = match func_call.function {
            Expression::Identifier(ident) => bytes_to_str(ident.value()),
            _ => return false,
        };
        let func_name = func_name.strip_prefix('\\').unwrap_or(func_name);
        if !func_name.eq_ignore_ascii_case("assert") {
            return false;
        }
        if let Some(first_arg) = func_call.argument_list.arguments.iter().next() {
            let arg_expr = match first_arg {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            };
            return arg_contains_instanceof(arg_expr);
        }
    }
    false
}
/// Recursively check whether an expression contains an `instanceof` operator.
pub(super) fn arg_contains_instanceof(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Parenthesized(inner) => arg_contains_instanceof(inner.expression),
        Expression::UnaryPrefix(prefix) => arg_contains_instanceof(prefix.operand),
        Expression::Binary(bin) => {
            if bin.operator.is_instanceof() {
                return true;
            }
            arg_contains_instanceof(bin.lhs) || arg_contains_instanceof(bin.rhs)
        }
        _ => false,
    }
}
