use mago_span::HasSpan;
use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use super::*;

mod anonymous_class;
mod assignment;
mod call_sites;
mod calls;
mod closures;
mod constructs;
mod member_access;

use anonymous_class::*;
use assignment::*;
use calls::*;
use closures::*;
use constructs::*;
use member_access::*;

use call_sites::emit_call_site;
pub(in crate::symbol_map::extraction) use call_sites::emit_partial_call_site;

// ─── Expression extractor ───────────────────────────────────────────────────

pub(super) fn extract_variable_symbol_spans<'a>(
    var: &'a Variable<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match var {
        Variable::Direct(dv) => {
            let raw = bytes_to_str(dv.name);
            if raw == "$this" {
                ctx.spans.push(SymbolSpan {
                    start: dv.span.start.offset,
                    end: dv.span.end.offset,
                    kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::This),
                });
            } else {
                let name = raw.strip_prefix('$').unwrap_or(raw).to_string();
                ctx.spans.push(SymbolSpan {
                    start: dv.span.start.offset,
                    end: dv.span.end.offset,
                    kind: SymbolKind::Variable { name },
                });
            }
        }
        Variable::Indirect(iv) => extract_from_expression(iv.expression, ctx, scope_start),
        Variable::Nested(nv) => extract_variable_symbol_spans(nv.variable, ctx, scope_start),
    }
}

pub(super) fn extract_from_expression<'a>(
    expr: &'a Expression<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match expr {
        // ── Variables ──
        Expression::Variable(Variable::Direct(dv)) => {
            let raw = bytes_to_str(dv.name);
            if raw == "$this" {
                // `$this` is semantically equivalent to `static` for
                // go-to-definition — resolve it to the enclosing class.
                ctx.spans.push(SymbolSpan {
                    start: dv.span.start.offset,
                    end: dv.span.end.offset,
                    kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::This),
                });
            } else {
                let name = raw.strip_prefix('$').unwrap_or(raw).to_string();
                ctx.spans.push(SymbolSpan {
                    start: dv.span.start.offset,
                    end: dv.span.end.offset,
                    kind: SymbolKind::Variable { name },
                });
            }
        }

        // ── self / static / parent keywords ──
        Expression::Self_(kw) => {
            ctx.spans.push(SymbolSpan {
                start: kw.span.start.offset,
                end: kw.span.end.offset,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Self_),
            });
        }
        Expression::Static(kw) => {
            ctx.spans.push(SymbolSpan {
                start: kw.span.start.offset,
                end: kw.span.end.offset,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Static),
            });
        }
        Expression::Parent(kw) => {
            ctx.spans.push(SymbolSpan {
                start: kw.span.start.offset,
                end: kw.span.end.offset,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Parent),
            });
        }

        // ── Identifiers (standalone class/constant references) ──
        Expression::Identifier(ident) => {
            let name = bytes_to_str(ident.value()).to_string();
            let name_clean = strip_fqn_prefix(&name).to_string();
            if is_navigable_type(&name_clean) {
                ctx.spans.push(class_ref_span(
                    ident.span().start.offset,
                    ident.span().end.offset,
                    &name,
                ));
            }
        }

        // ── Instantiation: `new Foo(...)` ──
        Expression::Instantiation(inst) => extract_instantiation_expr(inst, ctx, scope_start),

        // ── Function calls ──
        Expression::Call(call) => extract_call_expr(call, ctx, scope_start),

        // ── Property / constant access ──
        Expression::Access(access) => extract_access_expr(access, ctx, scope_start),

        // ── Assignment ──
        Expression::Assignment(assign) => extract_assignment_expr(assign, ctx, scope_start),

        // ── Binary operations ──
        Expression::Binary(bin) => {
            extract_from_expression(bin.lhs, ctx, scope_start);
            // Tag the RHS of `instanceof` with the Instanceof context.
            if bin.operator.is_instanceof() {
                if let Expression::Identifier(ident) = bin.rhs {
                    let raw = bytes_to_str(ident.value()).to_string();
                    ctx.spans.push(class_ref_span_ctx(
                        ident.span().start.offset,
                        ident.span().end.offset,
                        &raw,
                        ClassRefContext::Instanceof,
                    ));
                } else {
                    extract_from_expression(bin.rhs, ctx, scope_start);
                }
            } else {
                extract_from_expression(bin.rhs, ctx, scope_start);
            }
        }

        // ── Unary operations ──
        Expression::UnaryPrefix(un) => {
            if un.operator.is_cast() {
                let op_start = un.operator.span().start.offset;
                let raw = bytes_to_str(un.operator.as_bytes());
                if let Some(open) = raw.find('(')
                    && let Some(close) = raw.find(')')
                {
                    let inner = raw[open + 1..close].trim();
                    if !inner.is_empty() {
                        let inner_start_in_raw = raw.find(inner).unwrap_or(open + 1);
                        let type_start = op_start + inner_start_in_raw as u32;
                        let type_end = type_start + inner.len() as u32;
                        ctx.spans.push(SymbolSpan {
                            start: type_start,
                            end: type_end,
                            kind: SymbolKind::CastType,
                        });
                    }
                }
            }
            extract_from_expression(un.operand, ctx, scope_start);
        }
        Expression::UnaryPostfix(un) => {
            extract_from_expression(un.operand, ctx, scope_start);
        }

        // ── Parenthesized ──
        Expression::Parenthesized(paren) => {
            extract_from_expression(paren.expression, ctx, scope_start);
        }

        // ── Ternary ──
        Expression::Conditional(ternary) => {
            extract_from_expression(ternary.condition, ctx, scope_start);
            if let Some(then_branch) = ternary.then {
                extract_from_expression(then_branch, ctx, scope_start);
            }
            extract_from_expression(ternary.r#else, ctx, scope_start);
        }

        // ── Array ──
        Expression::Array(array) => {
            try_emit_array_callable_span(&array.elements, ctx.content, &mut ctx.spans);
            extract_from_array_elements(&array.elements, ctx, scope_start);
        }
        Expression::LegacyArray(array) => {
            try_emit_array_callable_span(&array.elements, ctx.content, &mut ctx.spans);
            extract_from_array_elements(&array.elements, ctx, scope_start);
        }
        Expression::List(list) => {
            extract_from_array_elements(&list.elements, ctx, scope_start);
        }

        // ── Array access ──
        Expression::ArrayAccess(access) => {
            extract_from_expression(access.array, ctx, scope_start);
            extract_from_expression(access.index, ctx, scope_start);
        }

        // ── Closures / arrow functions ──
        Expression::Closure(closure) => extract_closure_expr(closure, ctx),
        Expression::ArrowFunction(arrow) => extract_arrow_function_expr(arrow, ctx),

        // ── Match expression ──
        Expression::Match(match_expr) => {
            emit_keyword(&match_expr.r#match, ctx);
            extract_from_expression(match_expr.expression, ctx, scope_start);
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(arm) => {
                        for cond in arm.conditions.iter() {
                            extract_from_expression(cond, ctx, scope_start);
                        }
                        extract_from_expression(arm.expression, ctx, scope_start);
                    }
                    MatchArm::Default(arm) => {
                        emit_keyword(&arm.default, ctx);
                        extract_from_expression(arm.expression, ctx, scope_start);
                    }
                }
            }
        }

        // ── Throw expression (PHP 8) ──
        Expression::Throw(throw_expr) => {
            emit_keyword(&throw_expr.throw, ctx);
            extract_from_expression(throw_expr.exception, ctx, scope_start);
        }

        // ── Yield ──
        Expression::Yield(yield_expr) => match yield_expr {
            Yield::Value(yv) => {
                emit_keyword(&yv.r#yield, ctx);
                if let Some(value) = yv.value {
                    extract_from_expression(value, ctx, scope_start);
                }
            }
            Yield::Pair(yp) => {
                emit_keyword(&yp.r#yield, ctx);
                extract_from_expression(yp.key, ctx, scope_start);
                extract_from_expression(yp.value, ctx, scope_start);
            }
            Yield::From(yf) => {
                emit_keyword(&yf.r#yield, ctx);
                emit_keyword(&yf.from, ctx);
                extract_from_expression(yf.iterator, ctx, scope_start);
            }
        },

        // ── Clone ──
        Expression::Clone(clone) => {
            emit_keyword(&clone.clone, ctx);
            extract_from_expression(clone.object, ctx, scope_start);
        }

        // ── Anonymous class ──
        // `new class(...) extends Foo implements Bar { ... }`
        Expression::AnonymousClass(anon) => extract_anonymous_class_expr(anon, ctx, scope_start),

        // ── Language constructs ──
        // `isset($a, $b)`, `empty($x)`, `eval(...)`, `print(...)`,
        // `include ...`, `require ...`, `exit(...)`, `die(...)`
        Expression::Construct(construct) => extract_construct_expr(construct, ctx, scope_start),

        // ── Composite strings (interpolation) ──
        // `"Hello {$obj->method()}"`, heredocs, shell-exec backticks.
        Expression::CompositeString(composite) => {
            for part in composite.parts().iter() {
                match part {
                    StringPart::Expression(expr) => {
                        extract_from_expression(expr, ctx, scope_start);
                    }
                    StringPart::BracedExpression(braced) => {
                        extract_from_expression(braced.expression, ctx, scope_start);
                    }
                    StringPart::Literal(_) => {}
                }
            }
        }

        // ── Array append ──
        // `$arr[]` — the array expression is navigable.
        Expression::ArrayAppend(append) => {
            extract_from_expression(append.array, ctx, scope_start);
        }

        // ── Standalone constant access ──
        // `PHP_EOL`, `SORT_ASC`, `PHPStan\PHP_VERSION_ID`, etc.
        // The parser produces `ConstantAccess` for all standalone
        // constant references — including namespaced ones.  These are
        // never class names, so always emit `ConstantReference`.
        Expression::ConstantAccess(ca) => {
            let name = bytes_to_str(ca.name.value()).to_string();
            let name_clean = strip_fqn_prefix(&name).to_string();
            ctx.spans.push(SymbolSpan {
                start: ca.name.span().start.offset,
                end: ca.name.span().end.offset,
                kind: SymbolKind::ConstantReference { name: name_clean },
            });
        }

        // ── Pipe operator (PHP 8.5) ──
        // `$value |> transform(...)`
        Expression::Pipe(pipe) => {
            extract_from_expression(pipe.input, ctx, scope_start);
            extract_from_expression(pipe.callable, ctx, scope_start);
        }

        // ── First-class callable / partial application ──
        // `strlen(...)`, `$obj->method(...)`, `Class::method(...)`
        Expression::PartialApplication(partial) => {
            extract_partial_application_expr(partial, ctx, scope_start)
        }

        // Non-navigable expressions (literals, etc.) are intentionally ignored.
        _ => {}
    }
}

/// Collect variable definition sites from a destructuring pattern
/// (`[$a, $b] = ...` or `list($a, $b) = ...`).
pub(super) fn collect_destructuring_var_defs(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    var_defs: &mut Vec<VarDefSite>,
    scope_start: u32,
    kind: VarDefKind,
    effective_from: u32,
) {
    for element in elements.iter() {
        let value_expr = match element {
            ArrayElement::KeyValue(kv) => kv.value,
            ArrayElement::Value(val) => val.value,
            _ => continue,
        };
        match value_expr {
            Expression::Variable(Variable::Direct(dv)) => {
                let name = {
                    let s = bytes_to_str(dv.name);
                    s.strip_prefix('$').unwrap_or(s).to_string()
                };
                var_defs.push(VarDefSite {
                    offset: dv.span.start.offset,
                    name,
                    kind: kind.clone(),
                    scope_start,
                    effective_from,
                    nesting_depth: 0,
                    block_end: u32::MAX,
                });
            }
            // Nested destructuring: `[[$a, $b], $c] = ...`
            Expression::Array(arr) => {
                collect_destructuring_var_defs(
                    &arr.elements,
                    var_defs,
                    scope_start,
                    kind.clone(),
                    effective_from,
                );
            }
            Expression::List(list) => {
                collect_destructuring_var_defs(
                    &list.elements,
                    var_defs,
                    scope_start,
                    kind.clone(),
                    effective_from,
                );
            }
            _ => {}
        }
    }
}

// ─── Shared helpers ─────────────────────────────────────────────────────────

/// Walk an argument list and extract symbols from each argument expression.
pub(super) fn extract_from_arguments<'a>(
    args: &TokenSeparatedSequence<'a, Argument<'a>>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    for arg in args.iter() {
        let arg_expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        extract_from_expression(arg_expr, ctx, scope_start);
    }
}

/// Walk an argument list and extract symbols from each partial argument expression.
pub(super) fn extract_from_partial_arguments<'a>(
    args: &TokenSeparatedSequence<'a, PartialArgument<'a>>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    for arg in args.iter() {
        let arg_expr = match arg {
            PartialArgument::Positional(pos) => pos.value,
            PartialArgument::Named(named) => named.value,
            _ => continue,
        };
        extract_from_expression(arg_expr, ctx, scope_start);
    }
}

/// Walk array elements and extract symbols from each element expression.
pub(super) fn extract_from_array_elements<'a>(
    elements: &TokenSeparatedSequence<'a, ArrayElement<'a>>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    for element in elements.iter() {
        match element {
            ArrayElement::KeyValue(kv) => {
                extract_from_expression(kv.key, ctx, scope_start);
                extract_from_expression(kv.value, ctx, scope_start);
            }
            ArrayElement::Value(val) => {
                extract_from_expression(val.value, ctx, scope_start);
            }
            ArrayElement::Variadic(variadic) => {
                extract_from_expression(variadic.value, ctx, scope_start);
            }
            _ => {}
        }
    }
}

/// For the class part of a static call/property/constant access, emit
/// the appropriate span (ClassReference, SelfStaticParent, or recurse).
pub(super) fn emit_class_expr_span<'a>(
    expr: &'a Expression<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match expr {
        Expression::Identifier(ident) => {
            let raw = bytes_to_str(ident.value()).to_string();
            ctx.spans.push(class_ref_span(
                ident.span().start.offset,
                ident.span().end.offset,
                &raw,
            ));
        }
        Expression::Self_(kw) => {
            ctx.spans.push(SymbolSpan {
                start: kw.span.start.offset,
                end: kw.span.end.offset,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Self_),
            });
        }
        Expression::Static(kw) => {
            ctx.spans.push(SymbolSpan {
                start: kw.span.start.offset,
                end: kw.span.end.offset,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Static),
            });
        }
        Expression::Parent(kw) => {
            ctx.spans.push(SymbolSpan {
                start: kw.span.start.offset,
                end: kw.span.end.offset,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Parent),
            });
        }
        _ => {
            extract_from_expression(expr, ctx, scope_start);
        }
    }
}
