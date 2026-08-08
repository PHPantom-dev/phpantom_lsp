use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use super::*;
use crate::type_engine::subject_expr::{BracketSegment, SubjectExpr};

// ─── Expression to subject text ─────────────────────────────────────────────

/// Convert an AST expression to the subject text string that
/// `resolve_target_classes` expects.
///
/// This is a thin renderer over [`expr_to_subject_expr`]: the AST is first
/// lowered into a structured [`SubjectExpr`], which is then serialised by
/// [`SubjectExpr::to_subject_text`].  Keeping a single serialiser (the one on
/// `SubjectExpr`) prevents this AST path and the string-parse path
/// (`SubjectExpr::parse`) from drifting apart.  Expressions with no subject
/// representation (dynamic member names on nested chains, unsupported forms)
/// lower to `None` and render as the empty string.
pub(super) fn expr_to_subject_text(expr: &Expression<'_>) -> String {
    expr_to_subject_expr(expr)
        .map(|se| se.to_subject_text())
        .unwrap_or_default()
}

/// One link of a receiver spine: `->property` or `->method(args)`.
enum SpineLink<'a> {
    Property(&'a ClassLikeMemberSelector<'a>),
    Method(
        &'a ClassLikeMemberSelector<'a>,
        &'a TokenSeparatedSequence<'a, Argument<'a>>,
    ),
}

/// Lower an AST expression into a structured [`SubjectExpr`].
///
/// Returns `None` when the expression has no subject-text representation
/// (e.g. it is a form the type engine cannot resolve).  Call arguments are
/// serialised eagerly via [`format_all_call_args`] and stored as the raw
/// `args_text` of the resulting [`SubjectExpr::CallExpr`].
///
/// A receiver spine (`$base->a()->b->c()`) is a chain, not a tree: every
/// link's child is the link before it.  Lowering it from the outermost link
/// inward would cost a stack frame per link, and generated fluent code runs
/// long enough to exhaust any stack, so the spine is peeled into a `Vec`
/// and rebuilt outward from its base instead.
///
/// Note on null-safe access: `SubjectExpr` does not distinguish `?->` from
/// `->` (`SubjectExpr::parse` strips the `?` and the resolver normalises the
/// two), so null-safe property and method access lower to the same
/// `PropertyChain` / `MethodCall` shapes as their plain counterparts.
pub(super) fn expr_to_subject_expr<'a>(expr: &'a Expression<'a>) -> Option<SubjectExpr> {
    let mut links: Vec<SpineLink<'a>> = Vec::new();
    let mut current = expr;
    loop {
        match current {
            Expression::Access(Access::Property(pa)) => {
                links.push(SpineLink::Property(&pa.property));
                current = pa.object;
            }
            Expression::Access(Access::NullSafeProperty(pa)) => {
                links.push(SpineLink::Property(&pa.property));
                current = pa.object;
            }
            Expression::Call(Call::Method(mc)) => {
                links.push(SpineLink::Method(&mc.method, &mc.argument_list.arguments));
                current = mc.object;
            }
            Expression::Call(Call::NullSafeMethod(mc)) => {
                links.push(SpineLink::Method(&mc.method, &mc.argument_list.arguments));
                current = mc.object;
            }
            _ => break,
        }
    }

    let mut lowered = lower_spine_base(current)?;
    for link in links.iter().rev() {
        lowered = match link {
            SpineLink::Property(property) => lower_property(lowered, property),
            SpineLink::Method(method, args) => lower_method_call(lowered, method, args),
        };
    }
    Some(lowered)
}

/// Lower the base of a receiver spine: everything except the property and
/// method links [`expr_to_subject_expr`] peels off itself.
fn lower_spine_base<'a>(expr: &'a Expression<'a>) -> Option<SubjectExpr> {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            Some(SubjectExpr::Variable(bytes_to_str(dv.name).to_string()))
        }
        Expression::Self_(_) => Some(SubjectExpr::SelfKw),
        Expression::Static(_) => Some(SubjectExpr::StaticKw),
        Expression::Parent(_) => Some(SubjectExpr::Parent),
        Expression::Identifier(ident) => Some(SubjectExpr::ClassName(
            bytes_to_str(ident.value()).to_string(),
        )),

        Expression::Access(Access::StaticProperty(spa)) => {
            let class = expr_to_subject_expr(spa.class)?;
            if let Variable::Direct(dv) = &spa.property {
                Some(SubjectExpr::StaticAccess {
                    class: class.to_subject_text(),
                    member: bytes_to_str(dv.name).to_string(),
                })
            } else {
                Some(class)
            }
        }
        Expression::Access(Access::ClassConstant(cca)) => {
            let class = expr_to_subject_expr(cca.class)?;
            match &cca.constant {
                ClassLikeConstantSelector::Identifier(ident) => Some(SubjectExpr::StaticAccess {
                    class: class.to_subject_text(),
                    member: bytes_to_str(ident.value).to_string(),
                }),
                _ => Some(class),
            }
        }

        Expression::Call(Call::StaticMethod(sc)) => {
            let class = expr_to_subject_expr(sc.class)?;
            let (method, args_text) = match &sc.method {
                ClassLikeMemberSelector::Identifier(ident) => (
                    bytes_to_str(ident.value).to_string(),
                    format_all_call_args(&sc.argument_list.arguments),
                ),
                _ => ("?".to_string(), String::new()),
            };
            Some(SubjectExpr::CallExpr {
                callee: Box::new(SubjectExpr::StaticMethodCall {
                    class: class.to_subject_text(),
                    method,
                }),
                args_text,
            })
        }
        Expression::Call(Call::Function(fc)) => {
            let callee = expr_to_subject_expr(fc.function)?;
            let args_text = format_all_call_args(&fc.argument_list.arguments);
            // `SubjectExpr::to_subject_text` wraps callees that are not
            // callable-by-name (property chains, array access, nested calls)
            // in parentheses, so `($this->formatter)()` round-trips through
            // `SubjectExpr::parse` as an invoke rather than a method call.
            Some(SubjectExpr::CallExpr {
                callee: Box::new(callee),
                args_text,
            })
        }

        // `new Foo(...)` as a subject lowers to just the class name: the
        // constructed instance resolves through its own class regardless
        // of the constructor arguments.
        Expression::Instantiation(inst) => expr_to_subject_expr(inst.class),

        Expression::Parenthesized(paren) => expr_to_subject_expr(paren.expression),

        // `clone $expr` preserves the type of the operand.
        Expression::Clone(clone) => expr_to_subject_expr(clone.object),

        // Array literals: `[Foo::class, 'bar']` → `[Foo::class, 'bar']`.
        // We format elements we can represent and elide the rest so that
        // callers (especially conditional return-type resolution) can see
        // that an argument was provided and is not null.
        Expression::Array(array) => {
            let mut parts = Vec::new();
            for element in array.elements.iter() {
                match element {
                    ArrayElement::KeyValue(kv) => {
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
                    ArrayElement::Value(v) => {
                        let val = expr_to_subject_text(v.value);
                        if val.is_empty() {
                            parts.push("...".to_string());
                        } else {
                            parts.push(val);
                        }
                    }
                    ArrayElement::Variadic(v) => {
                        let val = expr_to_subject_text(v.value);
                        if val.is_empty() {
                            parts.push("...".to_string());
                        } else {
                            parts.push(format!("...{}", val));
                        }
                    }
                    ArrayElement::Missing(_) => {
                        parts.push("...".to_string());
                    }
                }
            }
            Some(SubjectExpr::InlineArray {
                elements: parts,
                index_segments: Vec::new(),
            })
        }

        // Ternary `$a ? $b : $c` and short ternary `$a ?: $b`.
        // For short ternary (`then` is None), the condition is the
        // preferred branch; for full ternary, use the `then` branch.
        // Either way we pick one branch so the type engine has
        // something to resolve rather than an empty string.
        Expression::Conditional(cond) => {
            let preferred = cond.then.unwrap_or(cond.condition);
            match expr_to_subject_expr(preferred) {
                Some(se) if !se.to_subject_text().is_empty() => Some(se),
                // Fall back to the else branch.
                _ => expr_to_subject_expr(cond.r#else),
            }
        }

        // Null coalesce `$a ?? $b` — LHS is the preferred non-null value.
        Expression::Binary(binary) if binary.operator.is_null_coalesce() => {
            match expr_to_subject_expr(binary.lhs) {
                Some(se) if !se.to_subject_text().is_empty() => Some(se),
                _ => expr_to_subject_expr(binary.rhs),
            }
        }

        Expression::ArrayAccess(access) => {
            let base = expr_to_subject_expr(access.array)?;
            if base.to_subject_text().is_empty() {
                return None;
            }
            // Preserve string keys for array-shape resolution and integer
            // indices for positional narrowing; collapse everything else to
            // generic element access (`[]`), matching the convention used by
            // `extract_arrow_subject`.
            let segment = match access.index {
                Expression::Literal(Literal::String(s)) => {
                    // `s.raw` includes surrounding quotes (e.g. `'key'`).
                    let raw_str = bytes_to_str(s.raw);
                    let inner = crate::text_scan::unquote_php_string(raw_str).unwrap_or(raw_str);
                    BracketSegment::StringKey(inner.to_string())
                }
                Expression::Literal(Literal::Integer(i)) => {
                    let n = i
                        .value
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| bytes_to_str(i.raw).to_string());
                    BracketSegment::IntKey(n)
                }
                _ => BracketSegment::ElementAccess,
            };
            Some(SubjectExpr::ArrayAccess {
                base: Box::new(base),
                segments: vec![segment],
            })
        }

        _ => None,
    }
}

/// Add a property access (`base->property` or `base?->property`) to an
/// already-lowered base.  A non-identifier selector (dynamic property name)
/// drops to the base expression, matching the original serialiser.
fn lower_property(base: SubjectExpr, property: &ClassLikeMemberSelector<'_>) -> SubjectExpr {
    if let ClassLikeMemberSelector::Identifier(ident) = property {
        SubjectExpr::PropertyChain {
            base: Box::new(base),
            property: bytes_to_str(ident.value).to_string(),
        }
    } else {
        base
    }
}

/// Add an instance method call (`base->method(args)` or the null-safe form)
/// to an already-lowered base.  A dynamic method name lowers to a `?` method
/// with no arguments, since the type engine cannot resolve it either way.
fn lower_method_call(
    base: SubjectExpr,
    method: &ClassLikeMemberSelector<'_>,
    args: &TokenSeparatedSequence<'_, Argument<'_>>,
) -> SubjectExpr {
    let (method, args_text) = match method {
        ClassLikeMemberSelector::Identifier(ident) => (
            bytes_to_str(ident.value).to_string(),
            format_all_call_args(args),
        ),
        _ => ("?".to_string(), String::new()),
    };
    SubjectExpr::CallExpr {
        callee: Box::new(SubjectExpr::MethodCall {
            base: Box::new(base),
            method,
        }),
        args_text,
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

/// Longest closure body kept verbatim in an argument's subject text.
///
/// A callback with no return-type annotation can only bind a
/// `callable(): T` template through its body, so the body has to survive
/// into the text.  Real callbacks are short (`fn ($p) => $p->slug`); this
/// bound keeps a pathological one from bloating the symbol map, at the
/// cost of losing the binding for that call.
const MAX_INLINE_CLOSURE_BODY: usize = 160;

/// Format a single argument expression to text.
///
/// For closures and arrow functions the parameter list and return type
/// annotation are always preserved, since template inference reads them.
/// The body is replaced with a placeholder (`=> ...` or `{ ... }`) unless
/// the callback has no return-type annotation, in which case the body is
/// the only thing left that can bind the callback's return template.
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
            match &arrow.return_type_hint {
                Some(rth) => format!(
                    "fn({}): {} => ...",
                    params,
                    crate::parser::extract_hint_type(&rth.hint)
                ),
                None => format!(
                    "fn({}) => {}",
                    params,
                    inline_closure_body(format_arg_expr(arrow.expression))
                ),
            }
        }
        // Closure: function(Type $a, Type $b): ReturnType { … }
        Expression::Closure(closure) => {
            let params = format_callable_params(&closure.parameter_list);
            match &closure.return_type_hint {
                Some(rth) => format!(
                    "function({}): {} {{ ... }}",
                    params,
                    crate::parser::extract_hint_type(&rth.hint)
                ),
                None => match first_return_expr(closure.body.statements.as_slice()) {
                    Some(value) => format!(
                        "function({}) {{ return {}; }}",
                        params,
                        inline_closure_body(format_arg_expr(value))
                    ),
                    None => format!("function({}) {{ ... }}", params),
                },
            }
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

/// Keep a rendered closure body only while it stays short, falling back to
/// the `...` placeholder otherwise.
fn inline_closure_body(body: String) -> String {
    if body.len() > MAX_INLINE_CLOSURE_BODY {
        "...".to_string()
    } else {
        body
    }
}

/// The value of a closure body's first top-level `return`.
///
/// Returns inside nested blocks are skipped: the point is to recover the
/// single-expression bodies callbacks are usually written as, not to
/// reconstruct control flow.
fn first_return_expr<'a>(statements: &'a [Statement<'a>]) -> Option<&'a Expression<'a>> {
    statements.iter().find_map(|stmt| match stmt {
        Statement::Return(ret) => ret.value,
        _ => None,
    })
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
