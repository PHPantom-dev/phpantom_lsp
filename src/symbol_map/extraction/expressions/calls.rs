use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::*;

// ─── Instantiation: `new Foo(...)` ──────────────────────────────────────────

pub(super) fn extract_instantiation_expr<'a>(
    inst: &'a Instantiation<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
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

// ─── Function / method / static calls ───────────────────────────────────────

pub(super) fn extract_call_expr<'a>(
    call: &'a Call<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match call {
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
                    || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Config"))
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
    }
}

// ─── First-class callable / partial application ─────────────────────────────
// `strlen(...)`, `$obj->method(...)`, `Class::method(...)`

pub(super) fn extract_partial_application_expr<'a>(
    partial: &'a PartialApplication<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match partial {
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
    }
}
