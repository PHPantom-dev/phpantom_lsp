use mago_span::HasSpan;
use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use super::*;

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
        Expression::Instantiation(inst) => {
            emit_keyword(&inst.new, ctx);
            match inst.class {
                Expression::Identifier(ident) => {
                    let raw = bytes_to_str(ident.value()).to_string();
                    ctx.spans.push(class_ref_span_ctx(
                        ident.span().start.offset,
                        ident.span().end.offset,
                        &raw,
                        ClassRefContext::New,
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
                    extract_from_expression(inst.class, ctx, scope_start);
                }
            }
            if let Some(ref args) = inst.argument_list {
                // Emit call site for constructor: `new ClassName(...)`
                let class_text = expr_to_subject_text(inst.class);
                if !class_text.is_empty() {
                    emit_call_site(
                        format!("new {}", class_text),
                        args,
                        &mut ctx.call_sites,
                        &mut ctx.untyped_closure_sites,
                    );
                }
                extract_from_arguments(&args.arguments, ctx, scope_start);
            }
        }

        // ── Function calls ──
        Expression::Call(call) => match call {
            Call::Function(func_call) => {
                match func_call.function {
                    Expression::Identifier(ident) => {
                        let name = bytes_to_str(ident.value()).to_string();
                        let name_clean = strip_fqn_prefix(&name).to_string();
                        if name_clean.eq_ignore_ascii_case("compact") {
                            try_emit_compact_string_spans(
                                &func_call.argument_list,
                                ctx.content,
                                &mut ctx.spans,
                            );
                        }
                        ctx.spans.push(SymbolSpan {
                            start: ident.span().start.offset,
                            end: ident.span().end.offset,
                            kind: SymbolKind::FunctionCall {
                                name: name_clean.clone(),
                                is_definition: false,
                            },
                        });
                        // Detect Laravel helper calls and emit a
                        // LaravelStringKey span for the first string arg.
                        // Uses if-else to short-circuit (most function calls
                        // won't match) and avoids to_ascii_lowercase() heap
                        // allocations.
                        let laravel_kind = if name_clean.eq_ignore_ascii_case("config") {
                            Some(crate::symbol_map::LaravelStringKind::Config)
                        } else if name_clean.eq_ignore_ascii_case("view")
                            || name_clean.eq_ignore_ascii_case("blade_view_directive")
                        {
                            Some(crate::symbol_map::LaravelStringKind::View)
                        } else if name_clean.eq_ignore_ascii_case("route")
                            || name_clean.eq_ignore_ascii_case("to_route")
                        {
                            Some(crate::symbol_map::LaravelStringKind::Route)
                        } else if name_clean.eq_ignore_ascii_case("__")
                            || name_clean.eq_ignore_ascii_case("trans")
                            || name_clean.eq_ignore_ascii_case("trans_choice")
                        {
                            Some(crate::symbol_map::LaravelStringKind::Trans)
                        } else {
                            None
                        };
                        if let Some(kind) = laravel_kind {
                            try_emit_laravel_string_span(
                                kind,
                                &func_call.argument_list,
                                ctx.content,
                                &mut ctx.spans,
                            );
                        }
                    }
                    _ => {
                        extract_from_expression(func_call.function, ctx, scope_start);
                    }
                }
                // Emit call site for function call
                let func_text = expr_to_subject_text(func_call.function);
                if !func_text.is_empty() {
                    emit_call_site(
                        func_text,
                        &func_call.argument_list,
                        &mut ctx.call_sites,
                        &mut ctx.untyped_closure_sites,
                    );
                }
                extract_from_arguments(&func_call.argument_list.arguments, ctx, scope_start);
            }
            Call::Method(method_call) => {
                let subject_text = expr_to_subject_text(method_call.object);
                extract_from_expression(method_call.object, ctx, scope_start);

                if let ClassLikeMemberSelector::Identifier(ident) = &method_call.method {
                    let member_name = bytes_to_str(ident.value).to_string();
                    if member_name.eq_ignore_ascii_case("macro") {
                        try_emit_laravel_macro_string_span(
                            &method_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                    if is_laravel_config_repository_call(method_call.object, &member_name) {
                        try_emit_laravel_string_span(
                            crate::symbol_map::LaravelStringKind::Config,
                            &method_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                    // Emit call site for method call: `$subject->method(...)`
                    emit_call_site(
                        format!("{}->{}", subject_text, member_name),
                        &method_call.argument_list,
                        &mut ctx.call_sites,
                        &mut ctx.untyped_closure_sites,
                    );
                    ctx.spans.push(SymbolSpan {
                        start: ident.span.start.offset,
                        end: ident.span.end.offset,
                        kind: SymbolKind::MemberAccess {
                            subject_text,
                            member_name,
                            is_static: false,
                            is_method_call: true,
                            is_docblock_reference: false,
                            is_array_callable: false,
                        },
                    });
                    // Laravel: if this is a ->group() call, check for
                    // ->controller(X::class) in the chain and emit MemberAccess
                    // spans for route method-name strings inside the closure.
                    if ident.value.eq_ignore_ascii_case(b"group")
                        && let Some(controller) =
                            laravel_route_find_controller_in_chain(method_call.object)
                    {
                        for arg in method_call.argument_list.arguments.iter() {
                            laravel_route_scan_group_body(
                                arg.value(),
                                &controller,
                                ctx.content,
                                &mut ctx.spans,
                            );
                        }
                    }
                }
                extract_from_arguments(&method_call.argument_list.arguments, ctx, scope_start);
            }
            Call::NullSafeMethod(method_call) => {
                let subject_text = expr_to_subject_text(method_call.object);
                extract_from_expression(method_call.object, ctx, scope_start);

                if let ClassLikeMemberSelector::Identifier(ident) = &method_call.method {
                    let member_name = bytes_to_str(ident.value).to_string();
                    if is_laravel_config_repository_call(method_call.object, &member_name) {
                        try_emit_laravel_string_span(
                            crate::symbol_map::LaravelStringKind::Config,
                            &method_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                    // Emit call site for null-safe method call.
                    // Use `->` so resolve_callable handles it the same
                    // as regular method calls.
                    emit_call_site(
                        format!("{}->{}", subject_text, member_name),
                        &method_call.argument_list,
                        &mut ctx.call_sites,
                        &mut ctx.untyped_closure_sites,
                    );
                    ctx.spans.push(SymbolSpan {
                        start: ident.span.start.offset,
                        end: ident.span.end.offset,
                        kind: SymbolKind::MemberAccess {
                            subject_text,
                            member_name,
                            is_static: false,
                            is_method_call: true,
                            is_docblock_reference: false,
                            is_array_callable: false,
                        },
                    });
                }
                extract_from_arguments(&method_call.argument_list.arguments, ctx, scope_start);
            }
            Call::StaticMethod(static_call) => {
                let subject_text = expr_to_subject_text(static_call.class);
                emit_class_expr_span(static_call.class, ctx, scope_start);

                if let ClassLikeMemberSelector::Identifier(ident) = &static_call.method {
                    let member_name = bytes_to_str(ident.value).to_string();
                    if member_name.eq_ignore_ascii_case("macro") {
                        try_emit_laravel_macro_string_span(
                            &static_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                    // Emit call site for static method call: `Class::method(...)`
                    emit_call_site(
                        format!("{}::{}", subject_text, member_name),
                        &static_call.argument_list,
                        &mut ctx.call_sites,
                        &mut ctx.untyped_closure_sites,
                    );
                    ctx.spans.push(SymbolSpan {
                        start: ident.span.start.offset,
                        end: ident.span.end.offset,
                        kind: SymbolKind::MemberAccess {
                            subject_text: subject_text.clone(),
                            member_name: member_name.clone(),
                            is_static: true,
                            is_method_call: true,
                            is_docblock_reference: false,
                            is_array_callable: false,
                        },
                    });
                    let clean_subject = strip_fqn_prefix(&subject_text);
                    if (clean_subject.eq_ignore_ascii_case("Config")
                        || clean_subject
                            .eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Config"))
                        && is_config_repository_method(&member_name)
                    {
                        try_emit_laravel_string_span(
                            crate::symbol_map::LaravelStringKind::Config,
                            &static_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                    if (clean_subject.eq_ignore_ascii_case("View")
                        || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\View"))
                        && matches!(member_name.to_ascii_lowercase().as_str(), "make" | "exists")
                    {
                        try_emit_laravel_string_span(
                            crate::symbol_map::LaravelStringKind::View,
                            &static_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                    if (clean_subject.eq_ignore_ascii_case("Lang")
                        || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Lang"))
                        && matches!(
                            member_name.to_ascii_lowercase().as_str(),
                            "get" | "has" | "choice"
                        )
                    {
                        try_emit_laravel_string_span(
                            crate::symbol_map::LaravelStringKind::Trans,
                            &static_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                }
                extract_from_arguments(&static_call.argument_list.arguments, ctx, scope_start);
            }
        },

        // ── Property / constant access ──
        Expression::Access(access) => {
            match access {
                Access::Property(pa) => {
                    let subject_text = expr_to_subject_text(pa.object);
                    extract_from_expression(pa.object, ctx, scope_start);

                    match &pa.property {
                        ClassLikeMemberSelector::Identifier(ident) => {
                            let member_name = bytes_to_str(ident.value).to_string();
                            ctx.spans.push(SymbolSpan {
                                start: ident.span.start.offset,
                                end: ident.span.end.offset,
                                kind: SymbolKind::MemberAccess {
                                    subject_text,
                                    member_name,
                                    is_static: false,
                                    is_method_call: false,
                                    is_docblock_reference: false,
                                    is_array_callable: false,
                                },
                            });
                        }
                        ClassLikeMemberSelector::Variable(var) => {
                            extract_variable_symbol_spans(var, ctx, scope_start)
                        }
                        ClassLikeMemberSelector::Expression(selector) => {
                            extract_from_expression(selector.expression, ctx, scope_start)
                        }
                        ClassLikeMemberSelector::Missing(_) => {}
                    }
                }
                Access::NullSafeProperty(pa) => {
                    let subject_text = expr_to_subject_text(pa.object);
                    extract_from_expression(pa.object, ctx, scope_start);

                    match &pa.property {
                        ClassLikeMemberSelector::Identifier(ident) => {
                            let member_name = bytes_to_str(ident.value).to_string();
                            ctx.spans.push(SymbolSpan {
                                start: ident.span.start.offset,
                                end: ident.span.end.offset,
                                kind: SymbolKind::MemberAccess {
                                    subject_text,
                                    member_name,
                                    is_static: false,
                                    is_method_call: false,
                                    is_docblock_reference: false,
                                    is_array_callable: false,
                                },
                            });
                        }
                        ClassLikeMemberSelector::Variable(var) => {
                            extract_variable_symbol_spans(var, ctx, scope_start)
                        }
                        ClassLikeMemberSelector::Expression(selector) => {
                            extract_from_expression(selector.expression, ctx, scope_start)
                        }
                        ClassLikeMemberSelector::Missing(_) => {}
                    }
                }
                Access::StaticProperty(spa) => {
                    let subject_text = expr_to_subject_text(spa.class);
                    emit_class_expr_span(spa.class, ctx, scope_start);

                    if let Variable::Direct(dv) = &spa.property {
                        let prop_name = {
                            let s = bytes_to_str(dv.name);
                            s.strip_prefix('$').unwrap_or(s).to_string()
                        };
                        ctx.spans.push(SymbolSpan {
                            start: dv.span.start.offset,
                            end: dv.span.end.offset,
                            kind: SymbolKind::MemberAccess {
                                subject_text,
                                member_name: prop_name,
                                is_static: true,
                                is_method_call: false,
                                is_docblock_reference: false,
                                is_array_callable: false,
                            },
                        });
                    }
                }
                Access::ClassConstant(cca) => {
                    let subject_text = expr_to_subject_text(cca.class);
                    emit_class_expr_span(cca.class, ctx, scope_start);

                    if let ClassLikeConstantSelector::Identifier(ident) = &cca.constant {
                        let const_name = bytes_to_str(ident.value).to_string();
                        if const_name == "class" {
                            // `Foo::class` — the navigable part is `Foo`.
                        } else {
                            ctx.spans.push(SymbolSpan {
                                start: ident.span.start.offset,
                                end: ident.span.end.offset,
                                kind: SymbolKind::MemberAccess {
                                    subject_text,
                                    member_name: const_name,
                                    is_static: true,
                                    is_method_call: false,
                                    is_docblock_reference: false,
                                    is_array_callable: false,
                                },
                            });
                        }
                    }
                }
            }
        }

        // ── Assignment ──
        Expression::Assignment(assign) => {
            extract_from_expression(assign.lhs, ctx, scope_start);
            extract_from_expression(assign.rhs, ctx, scope_start);

            // The definition only becomes visible *after* the entire
            // assignment expression — the RHS still sees the previous
            // definition of the variable.
            let effective = assign.span().end.offset;

            // Emit VarDefSite for simple variable assignments: `$var = ...`
            match assign.lhs {
                Expression::Variable(Variable::Direct(dv)) => {
                    let name = {
                        let s = bytes_to_str(dv.name);
                        s.strip_prefix('$').unwrap_or(s).to_string()
                    };
                    let kind = if assign.operator.is_assign() {
                        VarDefKind::Assignment
                    } else {
                        VarDefKind::CompoundAssignment
                    };
                    ctx.var_defs.push(VarDefSite {
                        offset: dv.span.start.offset,
                        name,
                        kind,
                        scope_start,
                        effective_from: effective,
                        nesting_depth: ctx.cond_nesting_depth,
                        block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                    });
                }
                // Array destructuring: `[$a, $b] = ...`
                Expression::Array(arr) => {
                    collect_destructuring_var_defs(
                        &arr.elements,
                        &mut ctx.var_defs,
                        scope_start,
                        VarDefKind::ArrayDestructuring,
                        effective,
                    );
                }
                // List destructuring: `list($a, $b) = ...`
                Expression::List(list) => {
                    collect_destructuring_var_defs(
                        &list.elements,
                        &mut ctx.var_defs,
                        scope_start,
                        VarDefKind::ListDestructuring,
                        effective,
                    );
                }
                _ => {}
            }
        }

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
        Expression::Closure(closure) => {
            // Closure introduces a new scope.
            let closure_scope_start = closure.body.left_brace.start.offset;
            let closure_scope_end = closure.body.right_brace.end.offset;
            ctx.scopes.push((closure_scope_start, closure_scope_end));
            ctx.body_scopes
                .push((closure_scope_start, closure_scope_end));

            for param in closure.parameter_list.parameters.iter() {
                // Attributes (PHP 8) on the parameter.
                extract_from_attribute_lists(&param.attribute_lists, ctx, 0);
                if let Some(ref hint) = param.hint {
                    extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
                }
                let name = {
                    let s = bytes_to_str(param.variable.name);
                    s.strip_prefix('$').unwrap_or(s).to_string()
                };
                ctx.spans.push(SymbolSpan {
                    start: param.variable.span.start.offset,
                    end: param.variable.span.end.offset,
                    kind: SymbolKind::Variable { name: name.clone() },
                });
                // Emit VarDefSite for closure parameter.
                let cp_offset = param.variable.span.start.offset;
                ctx.var_defs.push(VarDefSite {
                    offset: cp_offset,
                    name,
                    kind: VarDefKind::Parameter,
                    scope_start: closure_scope_start,
                    effective_from: cp_offset,
                    nesting_depth: ctx.cond_nesting_depth,
                    block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                });
                if let Some(ref default) = param.default_value {
                    extract_from_expression(default.value, ctx, closure_scope_start);
                }
            }
            if let Some(ref use_clause) = closure.use_clause {
                for var in use_clause.variables.iter() {
                    let name = {
                        let s = bytes_to_str(var.variable.name);
                        s.strip_prefix('$').unwrap_or(s).to_string()
                    };
                    ctx.spans.push(SymbolSpan {
                        start: var.variable.span.start.offset,
                        end: var.variable.span.end.offset,
                        kind: SymbolKind::Variable { name: name.clone() },
                    });
                    // Emit VarDefSite so that GTD inside the closure body
                    // can find the captured variable.  The definition is
                    // scoped to the closure body and immediately visible.
                    let use_var_offset = var.variable.span.start.offset;
                    ctx.var_defs.push(VarDefSite {
                        offset: use_var_offset,
                        name,
                        kind: VarDefKind::ClosureCapture,
                        scope_start: closure_scope_start,
                        effective_from: use_var_offset,
                        nesting_depth: ctx.cond_nesting_depth,
                        block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                    });
                }
            }
            if let Some(ref return_type) = closure.return_type_hint {
                extract_from_hint_ctx(&return_type.hint, &mut ctx.spans, ClassRefContext::TypeHint);
            }
            for s in closure.body.statements.iter() {
                extract_from_statement(s, ctx, closure_scope_start);
            }
        }
        Expression::ArrowFunction(arrow) => {
            // Arrow functions introduce a new scope for their parameters.
            // They don't have braces, so use the span of the arrow function itself.
            let arrow_scope_start = arrow.span().start.offset;
            let arrow_scope_end = arrow.span().end.offset;
            ctx.scopes.push((arrow_scope_start, arrow_scope_end));
            ctx.arrow_fn_scopes.push(arrow_scope_start);
            // Body scope starts at `=>` for signature help suppression.
            ctx.body_scopes
                .push((arrow.arrow.start.offset, arrow_scope_end));

            for param in arrow.parameter_list.parameters.iter() {
                // Attributes (PHP 8) on the parameter.
                extract_from_attribute_lists(&param.attribute_lists, ctx, 0);
                if let Some(ref hint) = param.hint {
                    extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
                }
                let name = {
                    let s = bytes_to_str(param.variable.name);
                    s.strip_prefix('$').unwrap_or(s).to_string()
                };
                ctx.spans.push(SymbolSpan {
                    start: param.variable.span.start.offset,
                    end: param.variable.span.end.offset,
                    kind: SymbolKind::Variable { name: name.clone() },
                });
                // Emit VarDefSite for arrow function parameter.
                let ap_offset = param.variable.span.start.offset;
                ctx.var_defs.push(VarDefSite {
                    offset: ap_offset,
                    name,
                    kind: VarDefKind::Parameter,
                    scope_start: arrow_scope_start,
                    effective_from: ap_offset,
                    nesting_depth: ctx.cond_nesting_depth,
                    block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                });
                if let Some(ref default) = param.default_value {
                    extract_from_expression(default.value, ctx, arrow_scope_start);
                }
            }
            if let Some(ref return_type) = arrow.return_type_hint {
                extract_from_hint_ctx(&return_type.hint, &mut ctx.spans, ClassRefContext::TypeHint);
            }
            extract_from_expression(arrow.expression, ctx, arrow_scope_start);
        }

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
        Expression::AnonymousClass(anon) => {
            // Constructor arguments.
            if let Some(ref args) = anon.argument_list {
                extract_from_partial_arguments(&args.arguments, ctx, scope_start);
            }

            // Extends.
            if let Some(ref extends) = anon.extends {
                for ident in extends.types.iter() {
                    let raw = bytes_to_str(ident.value()).to_string();
                    ctx.spans.push(class_ref_span(
                        ident.span().start.offset,
                        ident.span().end.offset,
                        &raw,
                    ));
                }
            }

            // Implements.
            if let Some(ref implements) = anon.implements {
                for ident in implements.types.iter() {
                    let raw = bytes_to_str(ident.value()).to_string();
                    ctx.spans.push(class_ref_span(
                        ident.span().start.offset,
                        ident.span().end.offset,
                        &raw,
                    ));
                }
            }

            // Attributes on the anonymous class.
            extract_from_attribute_lists(&anon.attribute_lists, ctx, scope_start);

            // Docblock.
            if let Some((doc_text, doc_offset)) =
                get_docblock_text_with_offset(ctx.trivias, ctx.content, anon)
            {
                let _tpl = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
            }

            // Members.
            for member in anon.members.iter() {
                extract_from_class_member(member, ctx);
            }
        }

        // ── Language constructs ──
        // `isset($a, $b)`, `empty($x)`, `eval(...)`, `print(...)`,
        // `include ...`, `require ...`, `exit(...)`, `die(...)`
        Expression::Construct(construct) => match construct {
            Construct::Isset(isset) => {
                emit_keyword(&isset.isset, ctx);
                for val in isset.values.iter() {
                    extract_from_expression(val, ctx, scope_start);
                }
            }
            Construct::Empty(empty) => {
                emit_keyword(&empty.empty, ctx);
                extract_from_expression(empty.value, ctx, scope_start);
            }
            Construct::Eval(eval) => {
                emit_keyword(&eval.eval, ctx);
                extract_from_expression(eval.value, ctx, scope_start);
            }
            Construct::Include(inc) => {
                emit_keyword(&inc.include, ctx);
                extract_from_expression(inc.value, ctx, scope_start);
            }
            Construct::IncludeOnce(inc) => {
                emit_keyword(&inc.include_once, ctx);
                extract_from_expression(inc.value, ctx, scope_start);
            }
            Construct::Require(req) => {
                emit_keyword(&req.require, ctx);
                extract_from_expression(req.value, ctx, scope_start);
            }
            Construct::RequireOnce(req) => {
                emit_keyword(&req.require_once, ctx);
                extract_from_expression(req.value, ctx, scope_start);
            }
            Construct::Print(print) => {
                emit_keyword(&print.print, ctx);
                extract_from_expression(print.value, ctx, scope_start);
            }
            Construct::Exit(exit) => {
                emit_keyword(&exit.exit, ctx);
                if let Some(ref args) = exit.arguments {
                    extract_from_arguments(&args.arguments, ctx, scope_start);
                }
            }
            Construct::Die(die) => {
                emit_keyword(&die.die, ctx);
                if let Some(ref args) = die.arguments {
                    extract_from_arguments(&args.arguments, ctx, scope_start);
                }
            }
        },

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
        Expression::PartialApplication(partial) => match partial {
            PartialApplication::Function(func_pa) => match func_pa.function {
                Expression::Identifier(ident) => {
                    let name = bytes_to_str(ident.value()).to_string();
                    let name_clean = strip_fqn_prefix(&name).to_string();
                    ctx.spans.push(SymbolSpan {
                        start: ident.span().start.offset,
                        end: ident.span().end.offset,
                        kind: SymbolKind::FunctionCall {
                            name: name_clean,
                            is_definition: false,
                        },
                    });
                }
                _ => {
                    extract_from_expression(func_pa.function, ctx, scope_start);
                }
            },
            PartialApplication::Method(method_pa) => {
                let subject_text = expr_to_subject_text(method_pa.object);
                extract_from_expression(method_pa.object, ctx, scope_start);
                if let ClassLikeMemberSelector::Identifier(ident) = &method_pa.method {
                    let member_name = bytes_to_str(ident.value).to_string();
                    ctx.spans.push(SymbolSpan {
                        start: ident.span.start.offset,
                        end: ident.span.end.offset,
                        kind: SymbolKind::MemberAccess {
                            subject_text,
                            member_name,
                            is_static: false,
                            is_method_call: true,
                            is_docblock_reference: false,
                            is_array_callable: false,
                        },
                    });
                }
            }
            PartialApplication::StaticMethod(static_pa) => {
                let subject_text = expr_to_subject_text(static_pa.class);
                emit_class_expr_span(static_pa.class, ctx, scope_start);
                if let ClassLikeMemberSelector::Identifier(ident) = &static_pa.method {
                    let member_name = bytes_to_str(ident.value).to_string();
                    ctx.spans.push(SymbolSpan {
                        start: ident.span.start.offset,
                        end: ident.span.end.offset,
                        kind: SymbolKind::MemberAccess {
                            subject_text,
                            member_name,
                            is_static: true,
                            is_method_call: true,
                            is_docblock_reference: false,
                            is_array_callable: false,
                        },
                    });
                }
            }
        },

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

// ─── Call site emission ─────────────────────────────────────────────────────

/// Build and push a [`CallSite`] from an argument list and its call expression string.
pub(super) fn emit_call_site(
    call_expression: String,
    argument_list: &ArgumentList<'_>,
    call_sites: &mut Vec<CallSite>,
    untyped_closure_sites: &mut Vec<UntypedClosureSite>,
) {
    if call_expression.is_empty() {
        return;
    }
    let args_start = argument_list.left_parenthesis.end.offset;
    let args_end = argument_list.right_parenthesis.start.offset;
    let comma_offsets: Vec<u32> = argument_list
        .arguments
        .tokens
        .iter()
        .map(|t| t.start.offset)
        .collect();

    let arg_count = argument_list.arguments.len() as u32;

    // Collect the byte offset of each argument's start token and
    // track which arguments use named syntax (`name: value`).
    let mut arg_offsets = Vec::with_capacity(arg_count as usize);
    let mut named_arg_indices = Vec::new();
    let mut named_arg_names = Vec::new();
    let mut spread_arg_indices = Vec::new();
    for (i, arg) in argument_list.arguments.iter().enumerate() {
        match arg {
            Argument::Positional(pos) => {
                // If unpacking is used, the `...` token comes before the
                // value expression.  Use the ellipsis offset when present
                // so the hint appears before `...`.
                let offset = pos
                    .ellipsis
                    .as_ref()
                    .map(|e| e.start.offset)
                    .unwrap_or_else(|| pos.value.span().start.offset);
                arg_offsets.push(offset);
                if pos.ellipsis.is_some() {
                    spread_arg_indices.push(i as u32);
                }
            }
            Argument::Named(named) => {
                arg_offsets.push(named.name.span.start.offset);
                named_arg_indices.push(i as u32);
                named_arg_names.push(bytes_to_str(named.name.value).to_string());
            }
        }
    }

    // Detect argument unpacking (`...$args`).  Only positional
    // arguments can use the spread operator; the AST stores it as
    // `ellipsis: Some(Span)` on `PositionalArgument`.
    let has_unpacking = argument_list
        .arguments
        .iter()
        .any(|arg| matches!(arg, Argument::Positional(pos) if pos.ellipsis.is_some()));

    // Check arguments for closures/arrows with untyped parameters or
    // missing return types.
    for (arg_idx, arg) in argument_list.arguments.iter().enumerate() {
        let expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        collect_untyped_closure_site(expr, &call_expression, arg_idx, untyped_closure_sites);
    }

    call_sites.push(CallSite {
        args_start,
        args_end,
        call_expression,
        comma_offsets,
        arg_offsets,
        arg_count,
        has_unpacking,
        named_arg_indices,
        named_arg_names,
        spread_arg_indices,
    });
}

/// Build and push a [`CallSite`] from a partial argument list and its call expression string.
pub(super) fn emit_partial_call_site(
    call_expression: String,
    argument_list: &PartialArgumentList<'_>,
    call_sites: &mut Vec<CallSite>,
    untyped_closure_sites: &mut Vec<UntypedClosureSite>,
) {
    let args_start = argument_list.left_parenthesis.end.offset;
    let args_end = argument_list.right_parenthesis.start.offset;
    let comma_offsets = argument_list
        .arguments
        .tokens
        .iter()
        .map(|token| token.start.offset)
        .collect();
    let mut arg_offsets = Vec::with_capacity(argument_list.arguments.len());
    let mut named_arg_indices = Vec::new();
    let mut named_arg_names = Vec::new();
    let mut spread_arg_indices = Vec::new();

    for (index, argument) in argument_list.arguments.iter().enumerate() {
        match argument {
            PartialArgument::Positional(argument) => {
                let offset = argument
                    .ellipsis
                    .map(|span| span.start.offset)
                    .unwrap_or_else(|| argument.value.span().start.offset);
                arg_offsets.push(offset);
                if argument.ellipsis.is_some() {
                    spread_arg_indices.push(index as u32);
                }
                collect_untyped_closure_site(
                    argument.value,
                    &call_expression,
                    index,
                    untyped_closure_sites,
                );
            }
            PartialArgument::Named(argument) => {
                arg_offsets.push(argument.name.span.start.offset);
                named_arg_indices.push(index as u32);
                named_arg_names.push(bytes_to_str(argument.name.value).to_string());
                collect_untyped_closure_site(
                    argument.value,
                    &call_expression,
                    index,
                    untyped_closure_sites,
                );
            }
            PartialArgument::NamedPlaceholder(argument) => {
                arg_offsets.push(argument.name.span.start.offset);
                named_arg_indices.push(index as u32);
                named_arg_names.push(bytes_to_str(argument.name.value).to_string());
            }
            PartialArgument::Placeholder(argument) => arg_offsets.push(argument.span.start.offset),
            PartialArgument::VariadicPlaceholder(argument) => {
                arg_offsets.push(argument.span.start.offset)
            }
        }
    }

    call_sites.push(CallSite {
        args_start,
        args_end,
        call_expression,
        comma_offsets,
        arg_count: argument_list.arguments.len() as u32,
        has_unpacking: !spread_arg_indices.is_empty(),
        arg_offsets,
        named_arg_indices,
        named_arg_names,
        spread_arg_indices,
    });
}

/// If `expr` is a closure or arrow function, collect an [`UntypedClosureSite`]
/// with its untyped parameters and (optionally) its close-paren offset for a
/// return type hint.
pub(super) fn collect_untyped_closure_site(
    expr: &Expression<'_>,
    parent_call_expression: &str,
    arg_index: usize,
    out: &mut Vec<UntypedClosureSite>,
) {
    let (params, close_paren_offset, has_return_type) = match expr {
        Expression::Closure(c) => (
            &c.parameter_list.parameters,
            c.parameter_list.span().end.offset,
            c.return_type_hint.is_some(),
        ),
        Expression::ArrowFunction(a) => (
            &a.parameter_list.parameters,
            a.parameter_list.span().end.offset,
            a.return_type_hint.is_some(),
        ),
        _ => return,
    };

    let mut untyped_params = Vec::new();
    for (param_idx, param) in params.iter().enumerate() {
        if param.hint.is_none() {
            untyped_params.push((param_idx, param.variable.span.start.offset));
        }
    }

    // Only emit a site if there is something for inlay hints to show:
    // untyped parameters or a missing return type.
    if untyped_params.is_empty() && has_return_type {
        return;
    }

    out.push(UntypedClosureSite {
        parent_call_expression: parent_call_expression.to_string(),
        arg_index_in_parent: arg_index,
        close_paren_offset: if has_return_type {
            None
        } else {
            Some(close_paren_offset)
        },
        untyped_params,
    });
}
