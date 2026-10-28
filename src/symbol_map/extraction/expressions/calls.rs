use mago_span::HasSpan;

use super::*;

// ─── Instantiation: `new Foo(...)` ──────────────────────────────────────────

pub(super) fn extract_instantiation_expr<'a>(
    inst: &'a Instantiation<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    emit_keyword(&inst.new, ctx);
    emit_class_expr_span(inst.class, ClassRefContext::New, ctx, scope_start);
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
        emit_symfony_instantiation_spans(inst.class, args, ctx);
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

    let subject_text = subject_text_of(property_access.class, ctx.content);
    emit_class_expr_span(
        property_access.class,
        ClassRefContext::Other,
        ctx,
        scope_start,
    );

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
    record_morph_column_call(call, ctx.content, &mut ctx.morph_column_sites);
    match call {
        Call::Function(func_call) => extract_function_call(func_call, ctx, scope_start),
        Call::Method(method_call) => extract_method_call(
            method_call.object,
            &method_call.method,
            &method_call.argument_list,
            false,
            ctx,
            scope_start,
            visit_receiver,
        ),
        Call::NullSafeMethod(method_call) => extract_method_call(
            method_call.object,
            &method_call.method,
            &method_call.argument_list,
            true,
            ctx,
            scope_start,
            visit_receiver,
        ),
        Call::StaticMethod(static_call) => {
            extract_static_method_call(static_call, ctx, scope_start)
        }
    }
}

/// `clone($object, ['prop' => …])` writes each listed property on the
/// copy just as `$copy->prop = …` would, so each literal key gets the
/// property-access span that assignment would: navigation, references
/// and rename reach it, and the member diagnostics check it.
fn emit_clone_with_spans(func_call: &FunctionCall<'_>, ctx: &mut ExtractionCtx<'_>) {
    let Some((object, writes)) = crate::parser::clone_with_writes(func_call, ctx.content) else {
        return;
    };
    if writes.is_empty() {
        return;
    }
    let subject_text = expr_to_subject_text(object);
    if subject_text.is_empty() {
        return;
    }
    let object_span = object.span();
    for write in writes {
        ctx.spans.push(SymbolSpan {
            start: write.start,
            end: write.end,
            kind: SymbolKind::MemberAccess {
                subject_text: SubjectText::new(
                    subject_text.clone(),
                    object_span.start.offset,
                    object_span.end.offset,
                    ctx.content,
                ),
                member_name: crate::atom::atom(write.name),
                is_static: false,
                is_method_call: false,
                docblock_ref: DocblockMemberRef::No,
                is_array_callable: false,
                is_nullsafe: false,
            },
        });
    }
}

fn extract_function_call<'a>(
    func_call: &'a FunctionCall<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
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
            emit_clone_with_spans(func_call, ctx);
            ctx.spans.push(SymbolSpan {
                start: ident.span().start.offset,
                end: ident.span().end.offset,
                kind: SymbolKind::FunctionCall {
                    name: crate::atom::atom(name_clean),
                    is_definition: false,
                    is_docblock_reference: false,
                },
            });
            // `@yield`/`@section`/`@stack`/`@push` and their
            // helpers lower to markers of their own, so the
            // section or stack they name is extracted here.
            try_emit_blade_block_span(
                name_clean,
                &func_call.argument_list,
                ctx.content,
                &mut ctx.spans,
            );
            match helper_function_first_arg_kind(name_clean) {
                // The `config()` helper both reads and writes.
                Some(crate::symbol_map::StringKeyKind::Config) => {
                    try_emit_laravel_config_helper_spans(
                        &func_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                // `@can`, `@cannot` and `@canany` check the ability
                // against the arguments that follow it.
                Some(crate::symbol_map::StringKeyKind::GateAbility) => {
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
                // `@includeFirst` and its `@componentFirst` /
                // `@extendsFirst` siblings put their candidates in an
                // array literal, so the compiled directive names several
                // templates at once.
                Some(crate::symbol_map::StringKeyKind::View)
                    if helper_function_accepts_name_list(name_clean) =>
                {
                    try_emit_laravel_view_span_at(
                        0,
                        &func_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                Some(kind) => {
                    try_emit_laravel_string_span(
                        kind,
                        &func_call.argument_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
                None => {}
            }
            emit_symfony_function_call_span(
                ident.span().start.offset,
                &func_call.argument_list,
                ctx,
            );
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
                    .and_then(crate::symbol_map::StringKeyDependency::namespaced_auth)
                {
                    ctx.defer_string_key_spans_since(span_start, dependency);
                }
            }
        }
        _ => {
            extract_from_expression(func_call.function, ctx, scope_start);
        }
    }
    let func_text = invoked_callee_text(func_call.function);
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

/// Extract an instance method call, written with `->` or `?->`.
///
/// Both spell the same call to the same method, so everything a `->` call
/// names a `?->` call names too. The one difference is recorded on the
/// member span: `?->` short-circuits to `null` when its receiver is `null`
/// instead of touching the member.
fn extract_method_call<'a>(
    object: &'a Expression<'a>,
    method: &'a ClassLikeMemberSelector<'a>,
    argument_list: &'a ArgumentList<'a>,
    is_nullsafe: bool,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
    visit_receiver: bool,
) {
    let subject_text = expr_to_subject_text(object);
    if visit_receiver {
        extract_from_expression(object, ctx, scope_start);
    }

    if let ClassLikeMemberSelector::Identifier(ident) = method {
        let member_name = crate::atom::atom_bytes(ident.value);
        emit_laravel_method_call_spans(object, &member_name, argument_list, ctx);
        record_symfony_receiver_sites(object, &member_name, argument_list, ctx);
        record_yii_view_sites(object, &member_name, argument_list, ctx);
        // Recorded with `->` for both spellings so callable resolution
        // treats them alike.
        emit_call_site(
            format!("{}->{}", subject_text, member_name),
            argument_list,
            &mut ctx.call_sites,
            &mut ctx.untyped_closure_sites,
        );
        let object_span = object.span();
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
                is_nullsafe,
            },
        });
        // Laravel: if this is a ->group() call, check for
        // ->controller(X::class) in the chain and emit MemberAccess
        // spans for route method-name strings inside the closure.
        if ident.value.eq_ignore_ascii_case(b"group")
            && let Some(controller) = laravel_route_find_controller_in_chain(object)
        {
            for arg in argument_list.arguments.iter() {
                laravel_route_scan_group_body(
                    arg.value(),
                    &controller,
                    ctx.content,
                    &mut ctx.spans,
                );
            }
        }
    }
    extract_from_arguments(&argument_list.arguments, ctx, scope_start);
}

fn extract_static_method_call<'a>(
    static_call: &'a StaticMethodCall<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    if extract_property_hook_call(static_call, ctx, scope_start) {
        return;
    }

    let subject_text = expr_to_subject_text(static_call.class);
    emit_class_expr_span(static_call.class, ClassRefContext::Other, ctx, scope_start);

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
        emit_laravel_static_call_spans(
            clean_subject,
            semantic_subject,
            &member_name,
            &static_call.argument_list,
            ctx,
        );
    }
    extract_from_arguments(&static_call.argument_list.arguments, ctx, scope_start);
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
            let subject_text = subject_text_of(method_pa.object, ctx.content);
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
            let subject_text = subject_text_of(static_pa.class, ctx.content);
            emit_class_expr_span(static_pa.class, ClassRefContext::Other, ctx, scope_start);
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
    use crate::symbol_map::StringKeyDependency;

    #[test]
    fn route_can_span_waits_for_the_root_alias_shadow_check() {
        let php = "<?php\nRoute::get('/posts/{post}', $action)->can('update', 'post');\n";
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = OwnedResolvedNames::from_resolved(&resolver.resolve(program));

        let map = crate::symbol_map::extraction::extract_symbol_map_for_index(
            program, php, &names, true, true, None,
        );

        assert_eq!(map.conditional_string_key_spans.len(), 1);
        assert_eq!(
            Some(map.conditional_string_key_spans[0].dependency),
            StringKeyDependency::root_facade("Route")
        );
    }
}
