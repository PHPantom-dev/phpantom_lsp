use mago_span::HasSpan;

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
        // A mailable's `content()` returns
        // `new Content(view: 'emails.shipped', with: [...])`, whose `view:`,
        // `html:`, `text:`, and `markdown:` arguments each name a template.
        // `Content` is too plain a class name to key on alone, so the short
        // spelling is only read inside a mailable.
        let clean_class = strip_fqn_prefix(&class_text);
        if clean_class.eq_ignore_ascii_case("Illuminate\\Mail\\Mailables\\Content")
            || (ctx.in_mailable && clean_class.eq_ignore_ascii_case("Content"))
        {
            try_emit_mailable_content_view_spans(args, ctx.content, &mut ctx.spans);
        }
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

// ─── Property hook invocation: `parent::$prop::get()` ───────────────────────

/// Extract `parent::$prop::get()` / `parent::$prop::set($v)`, the PHP 8.4
/// spelling for calling a property's overridden hook directly.
///
/// It parses as a static method call on a static property access, but
/// neither half is static: `$prop` is an *instance* property of the parent,
/// and `get`/`set` names its hook rather than a method. Reading it
/// literally reports both as missing members of the parent. The navigable
/// parts are the class and the property, so those get spans and the
/// accessor gets none — the same way `Foo::class` leaves `class` alone.
///
/// PHP defines this syntax for `parent` and nothing else, so any other
/// class reference is a genuine static property holding a class name
/// followed by a genuine static call (`Registry::$instance::get('x')`) and
/// must be left to the ordinary extraction path.
///
/// Returns whether the call had this shape and was extracted here.
fn extract_property_hook_call<'a>(
    static_call: &'a StaticMethodCall<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) -> bool {
    let ClassLikeMemberSelector::Identifier(accessor) = &static_call.method else {
        return false;
    };
    if !accessor.value.eq_ignore_ascii_case(b"get") && !accessor.value.eq_ignore_ascii_case(b"set")
    {
        return false;
    }
    let Expression::Access(access) = static_call.class else {
        return false;
    };
    let Access::StaticProperty(property_access) = access else {
        return false;
    };
    if !matches!(property_access.class, Expression::Parent(_)) {
        return false;
    }
    let Variable::Direct(variable) = &property_access.property else {
        return false;
    };

    let class_span = property_access.class.span();
    let subject_text = SubjectText::new(
        expr_to_subject_text(property_access.class),
        class_span.start.offset,
        class_span.end.offset,
        ctx.content,
    );
    emit_class_expr_span(property_access.class, ctx, scope_start);

    let property_name = {
        let s = bytes_to_str(variable.name);
        crate::atom::atom(s.strip_prefix('$').unwrap_or(s))
    };
    ctx.spans.push(SymbolSpan {
        start: variable.span.start.offset,
        end: variable.span.end.offset,
        kind: SymbolKind::MemberAccess {
            subject_text,
            member_name: property_name,
            is_static: false,
            is_method_call: false,
            docblock_ref: DocblockMemberRef::No,
            is_array_callable: false,
            is_nullsafe: false,
        },
    });

    extract_from_arguments(&static_call.argument_list.arguments, ctx, scope_start);
    true
}

/// How many raw source bytes the first decoded character of a string
/// literal's body consumes.
///
/// Only `\\` and the literal's own quote character collapse two raw bytes
/// into one decoded byte; every other leading byte (including a lone `\`
/// PHP does not recognise as an escape) maps one-to-one.
fn leading_char_raw_len(raw: &str, quote: u8) -> u32 {
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'\\' && (bytes[1] == b'\\' || bytes[1] == quote) {
        2
    } else {
        1
    }
}

/// Emit a [`SymbolKind::ConstantReference`] span for the constant a
/// string-literal argument names: the declaration `define('NAME', …)`
/// creates, or the use `defined('NAME')` / `constant('NAME')` asks about.
///
/// The name is a string literal rather than an identifier, so nothing else
/// in the extractor sees it. Without this span `define()` is not a
/// reference to its own constant (renaming from a use site rewrites every
/// use but leaves `define()` naming the old constant) and `defined()` /
/// `constant()` calls are not references at all (a rename silently leaves
/// them asking about the old name).
fn try_emit_constant_name_span(
    argument_list: &ArgumentList<'_>,
    content: &str,
    is_definition: bool,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first) = argument_list.arguments.first() else {
        return;
    };
    let Expression::Literal(Literal::String(literal)) = first.value() else {
        return;
    };

    // `Literal::String` is only ever single- or double-quoted, so the name
    // starts one byte past the opening quote.
    let inner_start = literal.span.start.offset + 1;
    let inner_end = literal.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let Some(raw) = content.get(inner_start as usize..inner_end as usize) else {
        return;
    };
    let Some(literal_text) =
        content.get(literal.span.start.offset as usize..literal.span.end.offset as usize)
    else {
        return;
    };
    let Some(value) = literal.value.and_then(literal_bytes_to_str) else {
        return;
    };

    // Rename rewrites whatever the span covers, so a name that only reaches
    // PHP through an escape this can't map back to source bytes (a hex,
    // octal, or unicode escape) must not claim to spell itself in the
    // source. `unescape_php_string_literal` resolves only `\\` and the
    // matching quote escape, leaving every other backslash sequence
    // literal, so agreement with the real decoded value means no other
    // escape is in play — the mismatch, if any, is confined to those two.
    if crate::util::unescape_php_string_literal(literal_text).as_deref() != Some(value) {
        return;
    }

    // `constant('Foo::BAR')` / `defined('Foo::BAR')` name a class constant,
    // not a global one; that member belongs to `MemberAccess`, which this
    // does not emit.
    if !is_definition && value.contains("::") {
        return;
    }

    // `define('\FOO', 1)` names the same constant `FOO` does, and the span
    // has to cover the name alone for the rename edit to leave the prefix.
    let name = strip_fqn_prefix(value);
    let start = if name.len() == value.len() {
        inner_start
    } else {
        inner_start + leading_char_raw_len(raw, literal_text.as_bytes()[0])
    };

    spans.push(SymbolSpan {
        start,
        end: inner_end,
        kind: SymbolKind::ConstantReference {
            name: crate::atom::atom(name),
            is_definition,
        },
    });
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
                    if name_clean.eq_ignore_ascii_case("define") {
                        try_emit_constant_name_span(
                            &func_call.argument_list,
                            ctx.content,
                            true,
                            &mut ctx.spans,
                        );
                    } else if name_clean.eq_ignore_ascii_case("defined")
                        || name_clean.eq_ignore_ascii_case("constant")
                    {
                        try_emit_constant_name_span(
                            &func_call.argument_list,
                            ctx.content,
                            false,
                            &mut ctx.spans,
                        );
                    }
                    ctx.spans.push(SymbolSpan {
                        start: ident.span().start.offset,
                        end: ident.span().end.offset,
                        kind: SymbolKind::FunctionCall {
                            name: crate::atom::atom(name_clean),
                            is_definition: false,
                            is_docblock_reference: false,
                        },
                    });
                    // `@includeFirst` and its `@componentFirst` /
                    // `@extendsFirst` siblings put their candidates in an
                    // array literal, so the compiled directive names several
                    // templates at once.
                    if name_clean.eq_ignore_ascii_case("blade_view_directive") {
                        try_emit_laravel_view_span_at(
                            0,
                            &func_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                    // `@yield`/`@section`/`@stack`/`@push` and their
                    // helpers lower to markers of their own, so the
                    // section or stack they name is extracted here.
                    try_emit_blade_block_span(
                        name_clean,
                        &func_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                    // Detect Laravel helper calls and emit a
                    // LaravelStringKey span for the first string arg.
                    // Uses if-else to short-circuit (most function calls
                    // won't match) and avoids to_ascii_lowercase() heap
                    // allocations.
                    // The `config()` helper is the one that both reads and
                    // writes, so it takes a path of its own rather than the
                    // read-only mapping below.
                    if name_clean.eq_ignore_ascii_case("config") {
                        try_emit_laravel_config_helper_spans(
                            &func_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                    let laravel_kind = if name_clean.eq_ignore_ascii_case("view")
                        || name_clean.eq_ignore_ascii_case("blade_each_directive")
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
                    } else if name_clean.eq_ignore_ascii_case("app")
                        || name_clean.eq_ignore_ascii_case("resolve")
                    {
                        Some(crate::symbol_map::LaravelStringKind::ContainerBinding)
                    } else if name_clean.eq_ignore_ascii_case("env") {
                        Some(crate::symbol_map::LaravelStringKind::Env)
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
                    if let Some(trigger) = crate::symbol_map::laravel_resources::auth_helper_trigger(
                        ctx.content,
                        name_clean,
                        ident.span().start.offset,
                        ctx.resolved_names,
                        None,
                    ) {
                        let span_start = ctx.spans.len();
                        try_emit_laravel_config_resource_span_for_parameter(
                            trigger.kind,
                            trigger.shape,
                            trigger.access,
                            &func_call.argument_list,
                            trigger.argument,
                            ctx.content,
                            &mut ctx.spans,
                        );
                        if let Some(dependency) = ctx
                            .resolved_name_at(ident.span().start.offset)
                            .and_then(crate::symbol_map::LaravelStringDependency::namespaced_auth)
                        {
                            ctx.defer_laravel_spans_since(span_start, dependency);
                        }
                    }
                    // The Blade preprocessor lowers `@can`/`@cannot`/`@canany`
                    // to this call so the ability string is extracted here
                    // like any other authorization check.
                    if name_clean == BLADE_CAN_DIRECTIVE {
                        try_emit_gate_ability_spans(
                            &func_call.argument_list,
                            0,
                            Some(1),
                            AbilityRole::Check,
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
                // `$this->app->singleton('payments', …)` registers a container
                // key and `app()->make('payments')` asks for one.  Gated on the
                // receiver: `make` and `bind` say nothing on their own.
                if is_laravel_container_expr(method_call.object) {
                    try_emit_container_key_span(
                        &member_name,
                        &method_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // `redirect()->route('users.index')`, `url()->signedRoute(…)`,
                // and `response()->redirectToRoute(…)` all name a route.
                // Keyed on the receiver: `route()` is a plain enough method
                // name that only the helper it hangs off says it names one.
                if is_route_name_helper_receiver(method_call.object)
                    && is_route_name_method(&member_name)
                {
                    try_emit_laravel_string_span(
                        crate::symbol_map::LaravelStringKind::Route,
                        &method_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // `$request->routeIs('admin.*', 'account.*')` asks whether the
                // current route is any of the names it lists.
                if is_request_route_pattern_method(&member_name) {
                    try_emit_laravel_string_spans_all(
                        crate::symbol_map::LaravelStringKind::Route,
                        &method_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // `$this->view('emails.shipped', [...])` in a mailable, and
                // the view factory's own methods reached through a bare
                // `view()` helper call.  Both are keyed on the receiver:
                // `view`, `make`, and `first` are far too common as method
                // names to recognise on their own.
                let mut named_syntactically = true;
                if ctx.in_mailable
                    && is_mailable_view_method(&member_name)
                    && matches!(method_call.object, Expression::Variable(Variable::Direct(v)) if v.name == b"$this")
                {
                    try_emit_laravel_string_span(
                        crate::symbol_map::LaravelStringKind::View,
                        &method_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                } else if is_laravel_view_factory_call(method_call.object)
                    && let Some(index) = view_factory_method_name_index(&member_name)
                {
                    try_emit_laravel_view_span_at(
                        index,
                        &method_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                } else if is_laravel_view_factory_call(method_call.object)
                    && is_render_each_method(&member_name)
                {
                    try_emit_render_each_view_spans(
                        &method_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                } else {
                    named_syntactically = false;
                }
                // Nothing the receiver is spelled as says it renders, so
                // whether this names a template at all is a question about
                // its type — recorded here and answered lazily.
                if !named_syntactically
                    && let Some((receiver, name_indices)) = view_receiver_method(&member_name)
                {
                    record_view_receiver_sites(
                        receiver,
                        name_indices,
                        &method_call.argument_list,
                        ctx.content,
                        &mut ctx.view_receiver_sites,
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
                        docblock_ref: DocblockMemberRef::No,
                        is_array_callable: false,
                        is_nullsafe: false,
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
                        docblock_ref: DocblockMemberRef::No,
                        is_array_callable: false,
                        is_nullsafe: false,
                    },
                });
            }
            extract_from_arguments(&method_call.argument_list.arguments, ctx, scope_start);
        }
        Call::StaticMethod(static_call) => {
            if extract_property_hook_call(static_call, ctx, scope_start) {
                return;
            }

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
                        docblock_ref: DocblockMemberRef::No,
                        is_array_callable: false,
                        is_nullsafe: false,
                    },
                });
                let clean_subject = strip_fqn_prefix(&subject_text);
                let semantic_subject = ctx
                    .resolved_name_at(class_span.start.offset)
                    .map(strip_fqn_prefix)
                    .unwrap_or(clean_subject);
                let laravel_span_start = ctx.spans.len();
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
                if let Some(trigger) = crate::symbol_map::laravel_resources::static_method_trigger(
                    semantic_subject,
                    &member_name,
                ) {
                    try_emit_laravel_config_resource_span_for_parameter(
                        trigger.kind,
                        trigger.shape,
                        trigger.access,
                        &static_call.argument_list,
                        trigger.argument,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // The `View` facade proxies the view factory, so every
                // factory method that names a template does so here too.
                if clean_subject.eq_ignore_ascii_case("View")
                    || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\View")
                {
                    if let Some(index) = view_factory_method_name_index(&member_name) {
                        try_emit_laravel_view_span_at(
                            index,
                            &static_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    } else if is_render_each_method(&member_name) {
                        try_emit_render_each_view_spans(
                            &static_call.argument_list,
                            ctx.content,
                            &mut ctx.spans,
                        );
                    }
                }
                // `Route::view('/about', 'pages.about', [...])` binds a URI
                // straight to a template, naming the view second;
                // `Response::view('pages.about', [...])` names it first.
                if (clean_subject.eq_ignore_ascii_case("Route")
                    || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Route"))
                    && member_name.eq_ignore_ascii_case("view")
                {
                    try_emit_laravel_string_span_at(
                        crate::symbol_map::LaravelStringKind::View,
                        1,
                        &static_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                if (clean_subject.eq_ignore_ascii_case("Response")
                    || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Response"))
                    && member_name.eq_ignore_ascii_case("view")
                {
                    try_emit_laravel_string_span(
                        crate::symbol_map::LaravelStringKind::View,
                        &static_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // `URL::signedRoute('orders.show')`,
                // `Redirect::route('home')`, `Response::redirectToRoute(…)`.
                if is_route_name_facade_call(clean_subject, &member_name) {
                    try_emit_laravel_string_span(
                        crate::symbol_map::LaravelStringKind::Route,
                        &static_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // `Route::is('admin.*')` / `Route::currentRouteNamed(…)` list
                // the names the current route is checked against.
                if (clean_subject.eq_ignore_ascii_case("Route")
                    || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Route"))
                    && is_route_facade_pattern_method(&member_name)
                {
                    try_emit_laravel_string_spans_all(
                        crate::symbol_map::LaravelStringKind::Route,
                        &static_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // `Env::get('APP_NAME')` reads the same variable the `env()`
                // helper it backs does.
                if (clean_subject.eq_ignore_ascii_case("Env")
                    || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Env"))
                    && matches!(
                        member_name.to_ascii_lowercase().as_str(),
                        "get" | "getorfail"
                    )
                {
                    try_emit_laravel_string_span(
                        crate::symbol_map::LaravelStringKind::Env,
                        &static_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                if (clean_subject.eq_ignore_ascii_case("Lang")
                    || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Lang"))
                    && matches!(
                        member_name.to_ascii_lowercase().as_str(),
                        "get" | "has" | "hasforlocale" | "choice"
                    )
                {
                    try_emit_laravel_string_span(
                        crate::symbol_map::LaravelStringKind::Trans,
                        &static_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // `App::make('payments')` and the registrations written
                // against the same facade.
                if clean_subject.eq_ignore_ascii_case("App")
                    || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\App")
                {
                    try_emit_container_key_span(
                        &member_name,
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
                } else if matches_laravel_facade(semantic_subject, "Route")
                    && member_name.eq_ignore_ascii_case("middleware")
                {
                    try_emit_can_middleware_spans(
                        &static_call.argument_list,
                        true,
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
                if let Some(dependency) = ctx.resolved_names.and_then(|_| {
                    crate::symbol_map::LaravelStringDependency::root_facade(semantic_subject)
                }) {
                    ctx.defer_laravel_spans_since(laravel_span_start, dependency);
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
            AbilityRole::Definition,
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
            gate_ability_role(member_name),
            ctx.content,
            &mut ctx.spans,
            &mut ctx.gate_subjects,
        );
    }
}

/// What a `Gate` method does with the ability it is handed.
///
/// `has()` is the framework's own "is this ability registered?" predicate, so
/// a name it does not find is the answer the call was written to get rather
/// than a typo.  The span is still emitted — completion, hover, and
/// go-to-definition all work inside it — but it is not judged.
fn gate_ability_role(member_name: &str) -> AbilityRole {
    if member_name.eq_ignore_ascii_case("has") {
        AbilityRole::Existence
    } else {
        AbilityRole::Check
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
                AbilityRole::Check,
                ctx.content,
                &mut ctx.spans,
                &mut ctx.gate_subjects,
            );
        }
        return;
    }

    let route_dependency = if is_middleware || is_can {
        ctx.resolved_names
            .and_then(|names| route_facade_root_dependency(object, names))
    } else {
        None
    };
    let route_receiver =
        (is_middleware || is_can) && chain_roots_at_route_facade(object, ctx.resolved_names);
    if is_middleware && (receiver_is_this || route_receiver) {
        let span_start = ctx.spans.len();
        try_emit_can_middleware_spans(argument_list, route_receiver, ctx.content, &mut ctx.spans);
        if let Some(dependency) = route_dependency {
            ctx.defer_laravel_spans_since(span_start, dependency);
        }
        return;
    }

    // A route registration's `->can('update', 'post')` names a route
    // parameter, not a model, in its second argument.
    if is_can && route_receiver {
        let span_start = ctx.spans.len();
        try_emit_gate_ability_spans(
            argument_list,
            0,
            None,
            AbilityRole::Check,
            ctx.content,
            &mut ctx.spans,
            &mut ctx.gate_subjects,
        );
        if let Some(dependency) = route_dependency {
            ctx.defer_laravel_spans_since(span_start, dependency);
        }
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
        gate_ability_role(member_name),
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
                        is_docblock_reference: false,
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
                        docblock_ref: DocblockMemberRef::No,
                        is_array_callable: false,
                        is_nullsafe: false,
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
                        docblock_ref: DocblockMemberRef::No,
                        is_array_callable: false,
                        is_nullsafe: false,
                    },
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::names::OwnedResolvedNames;
    use crate::symbol_map::LaravelStringDependency;

    #[test]
    fn route_can_span_waits_for_the_root_alias_shadow_check() {
        let php = "<?php\nRoute::get('/posts/{post}', $action)->can('update', 'post');\n";
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = OwnedResolvedNames::from_resolved(&resolver.resolve(program));

        let map = crate::symbol_map::extraction::extract_symbol_map_for_index(program, php, &names);

        assert_eq!(map.conditional_laravel_spans.len(), 1);
        assert_eq!(
            Some(map.conditional_laravel_spans[0].dependency),
            LaravelStringDependency::root_facade("Route")
        );
    }
}
