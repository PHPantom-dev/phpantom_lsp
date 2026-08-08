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
    extract_call(call, ctx, scope_start, true)
}

/// Extract everything [`extract_call_expr`] does *except* the receiver of an
/// instance method call.
///
/// Used when walking a method-call spine iteratively: the receiver of each
/// link has already been visited as part of the link below it, so visiting it
/// again here would reintroduce the per-link recursion.
pub(super) fn extract_call_expr_members<'a>(
    call: &'a Call<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    extract_call(call, ctx, scope_start, false)
}

fn extract_call<'a>(
    call: &'a Call<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
    visit_receiver: bool,
) {
    match call {
        Call::Function(func_call) => {
            match func_call.function {
                Expression::Identifier(ident) => {
                    let raw = bytes_to_str(ident.value());
                    let name_clean = strip_fqn_prefix(raw);
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
                            name: crate::atom::atom(name_clean),
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
                    // The Blade preprocessor lowers `@can`/`@cannot`/`@canany`
                    // to this call so the ability string is extracted here
                    // like any other authorization check.
                    if name_clean == BLADE_CAN_DIRECTIVE {
                        try_emit_gate_ability_spans(
                            &func_call.argument_list,
                            0,
                            Some(1),
                            false,
                            ctx.content,
                            &mut ctx.spans,
                            &mut ctx.gate_subjects,
                        );
                    }
                }
                _ => {
                    extract_from_expression(func_call.function, ctx, scope_start);
                }
            }
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
            if visit_receiver {
                extract_from_expression(method_call.object, ctx, scope_start);
            }

            if let ClassLikeMemberSelector::Identifier(ident) = &method_call.method {
                let member_name = crate::atom::atom_bytes(ident.value);
                if member_name.eq_ignore_ascii_case("macro") {
                    try_emit_laravel_macro_string_span(
                        &method_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                if is_laravel_config_repository_call(method_call.object, &member_name) {
                    try_emit_laravel_config_key_span(
                        &member_name,
                        &method_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // `$this->call('app:sync')` / `$this->callSilently('app:sync')`
                // inside a console command runs another Artisan command.
                // Gated on `in_console_command` because `->call()` is an
                // extremely common method name elsewhere (e.g.
                // `$this->call('GET', '/uri')` in HTTP tests).
                if ctx.in_console_command
                    && matches!(method_call.object, Expression::Variable(Variable::Direct(v)) if v.name == b"$this")
                {
                    match member_name.to_ascii_lowercase().as_str() {
                        "call" | "callsilently" => {
                            try_emit_laravel_string_span(
                                crate::symbol_map::LaravelStringKind::Command,
                                &method_call.argument_list,
                                ctx.content,
                                &mut ctx.spans,
                            );
                        }
                        "argument" | "hasargument" | "getargument" => {
                            try_emit_command_own_param_span(
                                false,
                                &method_call.argument_list,
                                ctx.content,
                                &mut ctx.spans,
                            );
                        }
                        "option" | "hasoption" | "getoption" => {
                            try_emit_command_own_param_span(
                                true,
                                &method_call.argument_list,
                                ctx.content,
                                &mut ctx.spans,
                            );
                        }
                        _ => {}
                    }
                }
                // Authorization abilities.  `->can()` and friends are named
                // too plainly to claim on the method name alone, so each
                // shape is gated on what its receiver reads as: the `Gate`
                // facade, a route registration, `$this` in a controller, or
                // something that plainly is the authenticated user.
                emit_gate_ability_spans_for_method(
                    method_call.object,
                    &member_name,
                    &method_call.argument_list,
                    ctx,
                );
                // `$query->whereHasMorph('commentable', ['post'], …)` resolves
                // each type through the morph map, so a literal there is an
                // alias.  Keyed on the method name alone: every method in the
                // family is Eloquent-specific enough that a same-named method
                // elsewhere is vanishingly unlikely.
                if is_morph_types_query_method(&member_name) {
                    try_emit_morph_type_spans(
                        &method_call.argument_list,
                        1,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                emit_call_site(
                    format!("{}->{}", subject_text, member_name),
                    &method_call.argument_list,
                    &mut ctx.call_sites,
                    &mut ctx.untyped_closure_sites,
                );
                let object_span = method_call.object.span();
                ctx.spans.push(SymbolSpan {
                    start: ident.span.start.offset,
                    end: ident.span.end.offset,
                    kind: SymbolKind::MemberAccess {
                        subject_text: SubjectText::new(
                            subject_text,
                            object_span.start.offset,
                            object_span.end.offset,
                            ctx.content,
                        ),
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
            if visit_receiver {
                extract_from_expression(method_call.object, ctx, scope_start);
            }

            if let ClassLikeMemberSelector::Identifier(ident) = &method_call.method {
                let member_name = crate::atom::atom_bytes(ident.value);
                if is_laravel_config_repository_call(method_call.object, &member_name) {
                    try_emit_laravel_config_key_span(
                        &member_name,
                        &method_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // Use `->` so resolve_callable handles it the same
                // as regular method calls.
                emit_call_site(
                    format!("{}->{}", subject_text, member_name),
                    &method_call.argument_list,
                    &mut ctx.call_sites,
                    &mut ctx.untyped_closure_sites,
                );
                let object_span = method_call.object.span();
                ctx.spans.push(SymbolSpan {
                    start: ident.span.start.offset,
                    end: ident.span.end.offset,
                    kind: SymbolKind::MemberAccess {
                        subject_text: SubjectText::new(
                            subject_text,
                            object_span.start.offset,
                            object_span.end.offset,
                            ctx.content,
                        ),
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
                let member_name = crate::atom::atom_bytes(ident.value);
                if member_name.eq_ignore_ascii_case("macro") {
                    try_emit_laravel_macro_string_span(
                        &static_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                emit_call_site(
                    format!("{}::{}", subject_text, member_name),
                    &static_call.argument_list,
                    &mut ctx.call_sites,
                    &mut ctx.untyped_closure_sites,
                );
                let class_span = static_call.class.span();
                ctx.spans.push(SymbolSpan {
                    start: ident.span.start.offset,
                    end: ident.span.end.offset,
                    kind: SymbolKind::MemberAccess {
                        subject_text: SubjectText::new(
                            subject_text.clone(),
                            class_span.start.offset,
                            class_span.end.offset,
                            ctx.content,
                        ),
                        member_name,
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
                    try_emit_laravel_config_key_span(
                        &member_name,
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
                // Artisan command names: `Artisan::call('app:sync')`,
                // `Artisan::queue('app:sync')`, `Schedule::command('app:sync')`.
                if is_artisan_command_static_call(clean_subject, &member_name) {
                    try_emit_laravel_string_span(
                        crate::symbol_map::LaravelStringKind::Command,
                        &static_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // Authorization abilities checked (or defined) through the
                // `Gate` facade, and the `can:ability,Model` middleware
                // parameter of a route registration.
                if is_gate_facade(clean_subject) {
                    emit_gate_facade_ability_spans(&member_name, &static_call.argument_list, ctx);
                } else if clean_subject.eq_ignore_ascii_case("Route")
                    && member_name.eq_ignore_ascii_case("middleware")
                {
                    try_emit_can_middleware_spans(
                        &static_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // Eloquent morph aliases: the keys of a
                // `Relation::morphMap(['post' => Post::class])` registration,
                // and the argument of `Relation::getMorphedModel('post')` /
                // `Model::getActualClassNameForMorph('post')`.
                if clean_subject.eq_ignore_ascii_case("Relation")
                    || clean_subject
                        .eq_ignore_ascii_case("Illuminate\\Database\\Eloquent\\Relations\\Relation")
                {
                    if is_morph_map_registration_method(&member_name) {
                        try_emit_morph_map_key_spans(
                            &static_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    } else if member_name.eq_ignore_ascii_case("getMorphedModel") {
                        try_emit_morph_type_spans(
                            &static_call.argument_list,
                            0,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                }
                if member_name.eq_ignore_ascii_case("getActualClassNameForMorph") {
                    try_emit_morph_type_spans(
                        &static_call.argument_list,
                        0,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // A model forwards static calls to its query builder, so
                // `Comment::whereHasMorph('commentable', ['post'])` is the same
                // call site as the instance form.
                if is_morph_types_query_method(&member_name) {
                    try_emit_morph_type_spans(
                        &static_call.argument_list,
                        1,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
            }
            extract_from_arguments(&static_call.argument_list.arguments, ctx, scope_start);
        }
    }
}

// ─── Authorization gate abilities ───────────────────────────────────────────

/// Emit ability spans for a `Gate::<method>(…)` static call.
///
/// `define()` declares the ability it names, so its span is marked as a write
/// and never judged against the known set.  Every other gate method reads one
/// (or a list), with the model — when the call names one — in the second
/// argument.
fn emit_gate_facade_ability_spans<'a>(
    member_name: &str,
    argument_list: &'a ArgumentList<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    if member_name.eq_ignore_ascii_case("define") {
        try_emit_gate_ability_spans(
            argument_list,
            0,
            None,
            true,
            ctx.content,
            &mut ctx.spans,
            &mut ctx.gate_subjects,
        );
        return;
    }
    if is_gate_check_method(member_name) {
        try_emit_gate_ability_spans(
            argument_list,
            0,
            Some(1),
            false,
            ctx.content,
            &mut ctx.spans,
            &mut ctx.gate_subjects,
        );
    }
}

/// Emit ability spans for an instance-method authorization check.
///
/// Recognises the four unambiguous shapes: a chain rooted at the `Gate`
/// facade, `$this->authorize()` / `$this->authorizeForUser()` from the
/// `AuthorizesRequests` trait, `->can()` on something that plainly is the
/// authenticated user, and a route registration's `->can()` /
/// `->middleware('can:…')`.
fn emit_gate_ability_spans_for_method<'a>(
    object: &'a Expression<'a>,
    member_name: &str,
    argument_list: &'a ArgumentList<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    // Every shape below is one of a handful of method names, and this runs for
    // every method call in every file, so reject on the name first — before
    // walking any receiver chain or touching the heap.
    let is_can = member_name.eq_ignore_ascii_case("can")
        || member_name.eq_ignore_ascii_case("cannot")
        || member_name.eq_ignore_ascii_case("canAny");
    let is_authorize_for_user = member_name.eq_ignore_ascii_case("authorizeForUser");
    let is_middleware = member_name.eq_ignore_ascii_case("middleware");
    if !is_can
        && !is_authorize_for_user
        && !is_middleware
        && !member_name.eq_ignore_ascii_case("authorize")
        && !is_gate_check_method(member_name)
    {
        return;
    }

    let receiver_is_this =
        matches!(object, Expression::Variable(Variable::Direct(v)) if v.name == b"$this");

    // `$this->authorizeForUser($user, 'update', $post)` puts the user first,
    // shifting the ability and model along by one.  It is a method of the
    // `AuthorizesRequests` trait, so `$this` is the only receiver it has —
    // anything else is a different method that happens to share the name.
    if is_authorize_for_user {
        if receiver_is_this {
            try_emit_gate_ability_spans(
                argument_list,
                1,
                Some(2),
                false,
                ctx.content,
                &mut ctx.spans,
                &mut ctx.gate_subjects,
            );
        }
        return;
    }

    if is_middleware && (receiver_is_this || chain_roots_at_route_facade(object)) {
        try_emit_can_middleware_spans(argument_list, ctx.content, &mut ctx.spans);
        return;
    }

    // A route registration's `->can('update', 'post')` names a route
    // parameter, not a model, in its second argument.
    if is_can && chain_roots_at_route_facade(object) {
        try_emit_gate_ability_spans(
            argument_list,
            0,
            None,
            false,
            ctx.content,
            &mut ctx.spans,
            &mut ctx.gate_subjects,
        );
        return;
    }

    let is_check = if is_can {
        receiver_is_user_like(object)
    } else if member_name.eq_ignore_ascii_case("authorize") {
        receiver_is_this || chain_roots_at_gate(object)
    } else {
        chain_roots_at_gate(object)
    };
    if !is_check {
        return;
    }
    try_emit_gate_ability_spans(
        argument_list,
        0,
        Some(1),
        false,
        ctx.content,
        &mut ctx.spans,
        &mut ctx.gate_subjects,
    );
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
                let raw = bytes_to_str(ident.value());
                let name_clean = strip_fqn_prefix(raw);
                ctx.spans.push(SymbolSpan {
                    start: ident.span().start.offset,
                    end: ident.span().end.offset,
                    kind: SymbolKind::FunctionCall {
                        name: crate::atom::atom(name_clean),
                        is_definition: false,
                    },
                });
            }
            _ => {
                extract_from_expression(func_pa.function, ctx, scope_start);
            }
        },
        PartialApplication::Method(method_pa) => {
            let object_span = method_pa.object.span();
            let subject_text = SubjectText::new(
                expr_to_subject_text(method_pa.object),
                object_span.start.offset,
                object_span.end.offset,
                ctx.content,
            );
            extract_from_expression(method_pa.object, ctx, scope_start);
            if let ClassLikeMemberSelector::Identifier(ident) = &method_pa.method {
                let member_name = crate::atom::atom_bytes(ident.value);
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
            let class_span = static_pa.class.span();
            let subject_text = SubjectText::new(
                expr_to_subject_text(static_pa.class),
                class_span.start.offset,
                class_span.end.offset,
                ctx.content,
            );
            emit_class_expr_span(static_pa.class, ctx, scope_start);
            if let ClassLikeMemberSelector::Identifier(ident) = &static_pa.method {
                let member_name = crate::atom::atom_bytes(ident.value);
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
