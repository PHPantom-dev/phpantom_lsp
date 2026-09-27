use mago_span::HasSpan;

use super::*;

// ─── Class-like extractors ──────────────────────────────────────────────────

pub(super) fn extract_from_class<'a>(class: &'a Class<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Class name — declaration site, not a reference.
    let name = crate::atom::atom_bytes(class.name.value);
    ctx.spans.push(SymbolSpan {
        start: class.name.span.start.offset,
        end: class.name.span.end.offset,
        kind: SymbolKind::ClassDeclaration { name },
    });

    extract_from_attribute_lists(&class.attribute_lists, ctx, 0);

    if let Some(ref extends) = class.extends {
        for ident in extends.types.iter() {
            let raw = bytes_to_str(ident.value()).to_string();
            ctx.spans.push(class_ref_span_ctx(
                ident.span().start.offset,
                ident.span().end.offset,
                &raw,
                ClassRefContext::ExtendsClass,
            ));
        }
    }

    if let Some(ref implements) = class.implements {
        for ident in implements.types.iter() {
            let raw = bytes_to_str(ident.value()).to_string();
            ctx.spans.push(class_ref_span_ctx(
                ident.span().start.offset,
                ident.span().end.offset,
                &raw,
                ClassRefContext::Implements,
            ));
        }
    }

    let mut covers_default_class = None;
    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, class)
    {
        let found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        covers_default_class = found.covers_default_class;
        let scope_end = class.right_brace.end.offset;
        for (name, name_offset, bound, variance) in found.templates {
            ctx.template_defs.push(TemplateParamDef {
                name_offset,
                name,
                bound,
                variance,
                scope_start: doc_offset,
                scope_end,
            });
        }
        ctx.template_defs.extend(found.method_templates);
    }

    // Whether this class is (syntactically) an Artisan console command, so
    // `$this->call(...)` / `$this->callSilently(...)` inside its members can
    // be recognised as command-name references without false-positives in
    // ordinary classes (where `->call()` is a very common method name).
    // Detection is deliberately conservative: the direct `extends` clause
    // must name a `Command`-suffixed class, or the class must carry an
    // `#[AsCommand]` attribute.
    let prev_in_console_command = ctx.in_console_command;
    ctx.in_console_command = class_is_console_command(class);
    // Whether this class is (syntactically) a mailable, so `$this->view(…)`
    // and the `new Content(view: …)` its `content()` returns can be
    // recognised as view names.  `view`, `text`, and `Content` are all far
    // too plain to key on outside that context.
    let prev_in_mailable = ctx.in_mailable;
    ctx.in_mailable = class_extends_named(class, b"Mailable");
    let prev_covers_default_class = ctx.covers_default_class;
    ctx.covers_default_class = covers_default_class;

    for member in class.members.iter() {
        extract_from_class_member(member, ctx);
    }

    ctx.covers_default_class = prev_covers_default_class;
    ctx.in_mailable = prev_in_mailable;
    ctx.in_console_command = prev_in_console_command;
}

/// Whether `class`'s direct `extends` clause names a class whose short name
/// is `short`.
fn class_extends_named(class: &Class<'_>, short: &[u8]) -> bool {
    class.extends.as_ref().is_some_and(|ext| {
        ext.types.iter().any(|ty| {
            let value = ty.value();
            let tail = match value.iter().rposition(|&b| b == b'\\') {
                Some(idx) => &value[idx + 1..],
                None => value,
            };
            tail.eq_ignore_ascii_case(short)
        })
    })
}

/// Whether `class` is (syntactically) an Artisan console command: it
/// `extends` a `Command`-suffixed class or carries an `#[AsCommand]`
/// attribute.
fn class_is_console_command(class: &Class<'_>) -> bool {
    let has_as_command = class
        .attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
        .any(|attr| {
            let name = attr.name.value();
            let short = match name.iter().rposition(|&b| b == b'\\') {
                Some(idx) => &name[idx + 1..],
                None => name,
            };
            short == b"AsCommand"
        });
    if has_as_command {
        return true;
    }
    class
        .extends
        .as_ref()
        .map(|ext| {
            ext.types.iter().any(|ty| {
                let v = ty.value();
                let short = match v.iter().rposition(|&b| b == b'\\') {
                    Some(idx) => &v[idx + 1..],
                    None => v,
                };
                short.ends_with(b"Command")
            })
        })
        .unwrap_or(false)
}

pub(super) fn extract_from_interface<'a>(iface: &'a Interface<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Interface name — declaration site, not a reference.
    let name = crate::atom::atom_bytes(iface.name.value);
    ctx.spans.push(SymbolSpan {
        start: iface.name.span.start.offset,
        end: iface.name.span.end.offset,
        kind: SymbolKind::ClassDeclaration { name },
    });

    extract_from_attribute_lists(&iface.attribute_lists, ctx, 0);

    if let Some(ref extends) = iface.extends {
        for ident in extends.types.iter() {
            let raw = bytes_to_str(ident.value()).to_string();
            ctx.spans.push(class_ref_span_ctx(
                ident.span().start.offset,
                ident.span().end.offset,
                &raw,
                ClassRefContext::ExtendsInterface,
            ));
        }
    }

    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, iface)
    {
        let found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        let scope_end = iface.right_brace.end.offset;
        for (name, name_offset, bound, variance) in found.templates {
            ctx.template_defs.push(TemplateParamDef {
                name_offset,
                name,
                bound,
                variance,
                scope_start: doc_offset,
                scope_end,
            });
        }
        ctx.template_defs.extend(found.method_templates);
    }

    for member in iface.members.iter() {
        extract_from_class_member(member, ctx);
    }
}

pub(super) fn extract_from_trait<'a>(trait_def: &'a Trait<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Trait name — declaration site, not a reference.
    let name = crate::atom::atom_bytes(trait_def.name.value);
    ctx.spans.push(SymbolSpan {
        start: trait_def.name.span.start.offset,
        end: trait_def.name.span.end.offset,
        kind: SymbolKind::ClassDeclaration { name },
    });

    extract_from_attribute_lists(&trait_def.attribute_lists, ctx, 0);

    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, trait_def)
    {
        let found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        let scope_end = trait_def.right_brace.end.offset;
        for (name, name_offset, bound, variance) in found.templates {
            ctx.template_defs.push(TemplateParamDef {
                name_offset,
                name,
                bound,
                variance,
                scope_start: doc_offset,
                scope_end,
            });
        }
        ctx.template_defs.extend(found.method_templates);
    }

    for member in trait_def.members.iter() {
        extract_from_class_member(member, ctx);
    }
}

pub(super) fn extract_from_enum<'a>(enum_def: &'a Enum<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Enum name — declaration site, not a reference.
    let name = crate::atom::atom_bytes(enum_def.name.value);
    ctx.spans.push(SymbolSpan {
        start: enum_def.name.span.start.offset,
        end: enum_def.name.span.end.offset,
        kind: SymbolKind::ClassDeclaration { name },
    });

    extract_from_attribute_lists(&enum_def.attribute_lists, ctx, 0);

    if let Some(ref implements) = enum_def.implements {
        for ident in implements.types.iter() {
            let raw = bytes_to_str(ident.value()).to_string();
            ctx.spans.push(class_ref_span_ctx(
                ident.span().start.offset,
                ident.span().end.offset,
                &raw,
                ClassRefContext::Implements,
            ));
        }
    }

    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, enum_def)
    {
        let found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        let scope_end = enum_def.right_brace.end.offset;
        for (name, name_offset, bound, variance) in found.templates {
            ctx.template_defs.push(TemplateParamDef {
                name_offset,
                name,
                bound,
                variance,
                scope_start: doc_offset,
                scope_end,
            });
        }
        ctx.template_defs.extend(found.method_templates);
    }

    for member in enum_def.members.iter() {
        extract_from_class_member(member, ctx);
    }
}

// ─── Class member extractors ────────────────────────────────────────────────

/// Extract symbols from PHP 8 attribute lists (`#[Attr(...)]`).
///
/// Emits a `ClassReference` for the attribute class name and recurses
/// into argument expressions.
pub(super) fn extract_from_attribute_lists<'a>(
    attribute_lists: &mago_syntax::cst::sequence::Sequence<
        'a,
        mago_syntax::cst::attribute::AttributeList<'a>,
    >,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            // The attribute name (e.g. `\Illuminate\...\CollectedBy`).
            let raw = bytes_to_str(attr.name.value()).to_string();
            ctx.spans.push(class_ref_span_ctx(
                attr.name.span().start.offset,
                attr.name.span().end.offset,
                &raw,
                ClassRefContext::Attribute,
            ));

            // Attribute arguments — also emit a CallSite so that
            // signature help and named parameter completion work
            // inside `#[Attr(...)]` just like `new Attr(...)`.
            if let Some(ref arg_list) = attr.argument_list {
                extract_from_partial_arguments(&arg_list.arguments, ctx, scope_start);
                let class_name = raw.trim_start_matches('\\');
                if !class_name.is_empty() {
                    emit_partial_call_site(
                        format!("new {}", class_name),
                        arg_list,
                        &mut ctx.call_sites,
                        &mut ctx.untyped_closure_sites,
                    );
                }

                // Laravel container attributes: #[Config('key')],
                // #[Database('conn')], #[Cache('store')], etc. →
                // emit a config-key or named-resource span so hover,
                // go-to-definition, and diagnostics work on the name.
                //
                // Semantic names resolve imports and aliases exactly. The
                // syntax-only fallback caches its import check per file.
                let semantic_class_name = ctx
                    .resolved_name_at(attr.name.span().start.offset)
                    .unwrap_or(class_name);
                if let Some(attribute) = resolve_laravel_container_attr(
                    semantic_class_name,
                    ctx.resolved_names.is_none(),
                    &mut ctx.has_laravel_container_attrs,
                    ctx.content,
                ) {
                    match attribute {
                        LaravelContainerAttribute::Resource(trigger) => {
                            try_emit_laravel_config_resource_span_partial_for_parameter(
                                trigger.kind,
                                trigger.access,
                                arg_list,
                                trigger.argument,
                                ctx.content,
                                &mut ctx.spans,
                            );
                        }
                        LaravelContainerAttribute::Config => {
                            try_emit_laravel_string_span_partial_for_parameter(
                                crate::symbol_map::LaravelStringKind::Config,
                                arg_list,
                                "key",
                                ctx.content,
                                &mut ctx.spans,
                            );
                        }
                    }
                }

                // `#[RedirectToRoute('login')]` on a form request names the
                // route a failed validation bounces back to.
                if is_laravel_redirect_route_attr(
                    class_name,
                    &mut ctx.has_laravel_http_attrs,
                    ctx.content,
                ) {
                    try_emit_laravel_string_span_partial_for_parameter(
                        crate::symbol_map::LaravelStringKind::Route,
                        arg_list,
                        "route",
                        ctx.content,
                        &mut ctx.spans,
                    );
                }

                // PHPUnit coverage attributes: #[CoversMethod(Foo::class,
                // 'bar')], #[CoversFunction('baz')] — the target is a
                // string literal, so nothing else makes it navigable.
                try_emit_coverage_attribute_spans(
                    class_name,
                    arg_list,
                    &mut ctx.has_phpunit_attrs,
                    ctx.content,
                    &mut ctx.spans,
                );
            }
        }
    }
}

pub(super) fn extract_from_class_member<'a>(
    member: &'a ClassLikeMember<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    match member {
        ClassLikeMember::Method(method) => {
            extract_from_method(method, ctx);
        }
        ClassLikeMember::Property(property) => {
            extract_from_property(property, ctx);
        }
        ClassLikeMember::Constant(constant) => {
            extract_from_class_constant(constant, ctx);
        }
        ClassLikeMember::TraitUse(trait_use) => {
            // Process the docblock attached to the trait use statement
            // so that `@use Trait<TModel>` generic args get spans.
            if let Some((doc_text, doc_offset)) =
                get_docblock_text_with_offset(ctx.trivias, ctx.content, trait_use)
            {
                let _found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
            }

            for ident in trait_use.trait_names.iter() {
                let raw = bytes_to_str(ident.value()).to_string();
                ctx.spans.push(class_ref_span_ctx(
                    ident.span().start.offset,
                    ident.span().end.offset,
                    &raw,
                    ClassRefContext::TraitUse,
                ));
            }

            // Extract symbols from trait use adaptations (`{ ... }` block)
            // so that go-to-definition works on method names and trait
            // references inside `as` alias and `insteadof` declarations.
            if let TraitUseSpecification::Concrete(spec) = &trait_use.specification {
                // Collect trait names from the `use` list so we can use the
                // first one as a fallback subject for unqualified method
                // references (e.g. `method as alias` without `Trait::method`).
                let first_trait_name: Option<String> = trait_use
                    .trait_names
                    .iter()
                    .next()
                    .map(|id| bytes_to_str(id.value()).to_string());

                for adaptation in spec.adaptations.iter() {
                    match adaptation {
                        TraitUseAdaptation::Alias(alias_adapt) => {
                            extract_from_trait_alias_adaptation(
                                alias_adapt,
                                first_trait_name.as_deref(),
                                ctx,
                            );
                        }
                        TraitUseAdaptation::Precedence(prec) => {
                            extract_from_trait_precedence_adaptation(prec, ctx);
                        }
                    }
                }
            }
        }
        ClassLikeMember::EnumCase(enum_case) => {
            extract_from_attribute_lists(&enum_case.attribute_lists, ctx, 0);

            // Enum case name — declaration site span for find-references,
            // rename, and document-highlights.  Enum cases are accessed
            // statically (`self::Issue`, `TaskType::Issue`).
            let case_name_ident = enum_case.item.name();
            ctx.spans.push(SymbolSpan {
                start: case_name_ident.span.start.offset,
                end: case_name_ident.span.end.offset,
                kind: SymbolKind::MemberDeclaration {
                    name: crate::atom::atom_bytes(case_name_ident.value),
                    is_static: true,
                },
            });

            // Enum case values (backed enums).
            if let EnumCaseItem::Backed(backed) = &enum_case.item {
                extract_from_expression(backed.value, ctx, 0);
            }
        }
    }
}

/// Extract symbol spans from a trait `as` alias adaptation.
///
/// For `TraitA::method as alias`:
///   - `TraitA` gets a `ClassReference` span
///   - `method` gets a `MemberAccess` span (subject = `TraitA`, static call)
///   - `alias` gets a `MemberAccess` span (subject = `self`) so that
///     `resolve_trait_alias` maps it back to the original method
///
/// For unqualified `method as alias`:
///   - `method` gets a `MemberAccess` span using the first trait in the
///     `use` list as the subject (or `self` as fallback)
///   - `alias` gets a `MemberAccess` span (subject = `self`)
pub(super) fn extract_from_trait_alias_adaptation<'a>(
    alias_adapt: &'a TraitUseAliasAdaptation<'a>,
    first_trait_name: Option<&str>,
    ctx: &mut ExtractionCtx<'a>,
) {
    match &alias_adapt.method_reference {
        TraitUseMethodReference::Absolute(abs) => {
            let trait_raw = bytes_to_str(abs.trait_name.value()).to_string();
            ctx.spans.push(class_ref_span(
                abs.trait_name.span().start.offset,
                abs.trait_name.span().end.offset,
                &trait_raw,
            ));
            let method_name = crate::atom::atom_bytes(abs.method_name.value);
            let trait_span = abs.trait_name.span();
            ctx.spans.push(SymbolSpan {
                start: abs.method_name.span.start.offset,
                end: abs.method_name.span.end.offset,
                kind: SymbolKind::MemberAccess {
                    subject_text: SubjectText::new(
                        trait_raw,
                        trait_span.start.offset,
                        trait_span.end.offset,
                        ctx.content,
                    ),
                    member_name: method_name,
                    is_static: true,
                    is_method_call: true,
                    docblock_ref: DocblockMemberRef::No,
                    is_array_callable: false,
                    is_nullsafe: false,
                },
            });
        }
        TraitUseMethodReference::Identifier(ident) => {
            let subject = first_trait_name.unwrap_or("self").to_string();
            let method_name = crate::atom::atom_bytes(ident.value);
            ctx.spans.push(SymbolSpan {
                start: ident.span.start.offset,
                end: ident.span.end.offset,
                kind: SymbolKind::MemberAccess {
                    subject_text: SubjectText::owned(subject),
                    member_name: method_name,
                    is_static: true,
                    is_method_call: true,
                    docblock_ref: DocblockMemberRef::No,
                    is_array_callable: false,
                    is_nullsafe: false,
                },
            });
        }
    }

    // Using `self` as the subject so that `resolve_trait_alias` on
    // the owning class maps the alias back to the original method.
    if let Some(ref alias_ident) = alias_adapt.alias {
        let alias_name = crate::atom::atom_bytes(alias_ident.value);
        ctx.spans.push(SymbolSpan {
            start: alias_ident.span.start.offset,
            end: alias_ident.span.end.offset,
            kind: SymbolKind::MemberAccess {
                subject_text: SubjectText::owned("self".to_string()),
                member_name: alias_name,
                is_static: true,
                is_method_call: true,
                docblock_ref: DocblockMemberRef::No,
                is_array_callable: false,
                is_nullsafe: false,
            },
        });
    }
}

/// Extract symbol spans from a trait `insteadof` precedence adaptation.
///
/// For `TraitA::method insteadof TraitB, TraitC`:
///   - `TraitA` gets a `ClassReference` span
///   - `method` gets a `MemberAccess` span (subject = `TraitA`, static call)
///   - `TraitB` and `TraitC` each get a `ClassReference` span
pub(super) fn extract_from_trait_precedence_adaptation<'a>(
    prec: &'a TraitUsePrecedenceAdaptation<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    let trait_raw = bytes_to_str(prec.method_reference.trait_name.value()).to_string();
    ctx.spans.push(class_ref_span(
        prec.method_reference.trait_name.span().start.offset,
        prec.method_reference.trait_name.span().end.offset,
        &trait_raw,
    ));

    let method_name = crate::atom::atom_bytes(prec.method_reference.method_name.value);
    let trait_span = prec.method_reference.trait_name.span();
    ctx.spans.push(SymbolSpan {
        start: prec.method_reference.method_name.span.start.offset,
        end: prec.method_reference.method_name.span.end.offset,
        kind: SymbolKind::MemberAccess {
            subject_text: SubjectText::new(
                trait_raw,
                trait_span.start.offset,
                trait_span.end.offset,
                ctx.content,
            ),
            member_name: method_name,
            is_static: true,
            is_method_call: true,
            docblock_ref: DocblockMemberRef::No,
            is_array_callable: false,
            is_nullsafe: false,
        },
    });

    for ident in prec.trait_names.iter() {
        let raw = bytes_to_str(ident.value()).to_string();
        ctx.spans.push(class_ref_span(
            ident.span().start.offset,
            ident.span().end.offset,
            &raw,
        ));
    }
}

pub(super) fn extract_from_method<'a>(method: &'a Method<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Method name — declaration site span for find-references and rename.
    let is_static = method.modifiers.iter().any(|m| m.is_static());
    ctx.spans.push(SymbolSpan {
        start: method.name.span.start.offset,
        end: method.name.span.end.offset,
        kind: SymbolKind::MemberDeclaration {
            name: crate::atom::atom_bytes(method.name.value),
            is_static,
        },
    });

    extract_from_attribute_lists(&method.attribute_lists, ctx, 0);

    // Docblock on the method.  We extract type spans and template params
    // now, but defer `@param $var` variable spans until after we know
    // `method_scope_start` (the body's opening-brace offset).
    let method_docblock = get_docblock_text_with_offset(ctx.trivias, ctx.content, method);
    let mut docblock_params: Vec<(String, u32)> = Vec::new();
    if let Some((doc_text, doc_offset)) = method_docblock {
        let covers_default_class = ctx.covers_default_class;
        let found = extract_docblock_symbols_covering(
            doc_text,
            doc_offset,
            &mut ctx.spans,
            covers_default_class,
        );
        docblock_params = found.param_vars;
        // Method-level template params: scope extends from the docblock to
        // the end of the method body (or the end of the docblock for
        // abstract methods without a body).
        let scope_end = if let MethodBody::Concrete(body) = &method.body {
            body.right_brace.end.offset
        } else {
            // Abstract / interface method — scope is just the docblock + signature.
            // Use the method span end as a reasonable bound.
            method.span().end.offset
        };
        for (name, name_offset, bound, variance) in found.templates {
            ctx.template_defs.push(TemplateParamDef {
                name_offset,
                name,
                bound,
                variance,
                scope_start: doc_offset,
                scope_end,
            });
        }
    }

    // Determine scope_start for this method body.
    let method_scope_start = if let MethodBody::Concrete(body) = &method.body {
        let s = body.left_brace.start.offset;
        let e = body.right_brace.end.offset;
        ctx.scopes.push((s, e));
        if is_static {
            ctx.static_method_scopes.push((s, e));
        } else {
            ctx.instance_method_scopes.push((s, e));
        }
        s
    } else {
        0
    };

    // Emit Variable spans and VarDefSite markers for `@param $varName`
    // tokens in the docblock so that rename and find-references cover
    // them.  The VarDefSite with `DocblockParam` kind lets
    // `find_variable_scope` map the pre-body offset to the correct
    // function body scope.
    {
        for (name, file_offset) in docblock_params {
            let end = file_offset + 1 + name.len() as u32;
            ctx.spans.push(SymbolSpan {
                start: file_offset,
                end,
                kind: SymbolKind::Variable {
                    name: crate::atom::atom(&name),
                },
            });
            ctx.var_defs.push(VarDefSite {
                offset: file_offset,
                name,
                kind: VarDefKind::DocblockParam,
                scope_start: method_scope_start,
                effective_from: file_offset,
                nesting_depth: ctx.cond_nesting_depth,
                block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
            });
        }
    }

    // Parameter type hints, variable spans, and variable definition sites.
    for param in method.parameter_list.parameters.iter() {
        extract_from_attribute_lists(&param.attribute_lists, ctx, 0);
        if let Some(ref hint) = param.hint {
            extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
        }
        // Docblock attached to the parameter itself (e.g. promoted
        // constructor properties with `/** @var list<Subscription> */`).
        if let Some((doc_text, doc_offset)) =
            get_docblock_text_with_offset(ctx.trivias, ctx.content, param)
        {
            let _found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        }
        let name = {
            let s = bytes_to_str(param.variable.name);
            s.strip_prefix('$').unwrap_or(s).to_string()
        };
        let param_offset = param.variable.span.start.offset;
        // Emit a Variable span so the symbol map covers the parameter
        // token itself (needed for GTD-from-parameter-to-type-hint).
        ctx.spans.push(SymbolSpan {
            start: param_offset,
            end: param.variable.span.end.offset,
            kind: SymbolKind::Variable {
                name: crate::atom::atom(&name),
            },
        });
        ctx.var_defs.push(VarDefSite {
            offset: param_offset,
            name,
            kind: VarDefKind::Parameter,
            scope_start: method_scope_start,
            effective_from: param_offset,
            nesting_depth: ctx.cond_nesting_depth,
            block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
        });
        if let Some(ref default) = param.default_value {
            extract_from_expression(default.value, ctx, method_scope_start);
        }
        // A constructor-promoted property may declare hooks of its own,
        // which read like any other hook body but live out here in the
        // parameter list rather than in a `Property::Hooked` member.
        if let Some(hooks) = &param.hooks {
            for hook in hooks.hooks.iter() {
                extract_from_hook(hook, ctx);
            }
        }
    }

    if let Some(ref return_type) = method.return_type_hint {
        extract_from_hint_ctx(&return_type.hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }

    if let MethodBody::Concrete(body) = &method.body {
        for stmt in body.statements.iter() {
            extract_from_statement(stmt, ctx, method_scope_start);
        }
    }
}

/// Extract docblock symbols from an inline `/** @var ... */` comment
/// attached to a body-level statement (expression, return, echo, etc.).
///
/// These comments are stored as trivia preceding the statement token.
/// Unlike class/method docblocks, inline `@var` annotations don't define
/// template parameters — we only care about the type spans they contain.
pub(super) fn extract_inline_docblock(
    node: &impl HasSpan,
    ctx: &mut ExtractionCtx<'_>,
    scope_start: u32,
) {
    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, node)
    {
        let found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);

        // Emit VarDefSite entries for `@var Type $varName` in inline docblocks.
        for (name, file_offset) in found.var_vars {
            let name_len = name.len() as u32 + 1; // +1 for the `$` prefix
            ctx.spans.push(SymbolSpan {
                start: file_offset,
                end: file_offset + name_len,
                kind: SymbolKind::Variable {
                    name: crate::atom::atom(&name),
                },
            });
            ctx.var_defs.push(VarDefSite {
                offset: file_offset,
                name,
                kind: VarDefKind::DocblockVar,
                scope_start,
                effective_from: file_offset,
                nesting_depth: ctx.cond_nesting_depth,
                block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
            });
        }
    }
}

pub(super) fn extract_from_property<'a>(property: &'a Property<'a>, ctx: &mut ExtractionCtx<'a>) {
    match property {
        Property::Plain(plain) => extract_from_attribute_lists(&plain.attribute_lists, ctx, 0),
        Property::Hooked(hooked) => extract_from_attribute_lists(&hooked.attribute_lists, ctx, 0),
    }

    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, property)
    {
        // Property docblocks don't define template params, but we still
        // need to consume the return value.
        let _found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
    }

    if let Some(hint) = property.hint() {
        extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }

    // Property variable names and default value expressions.
    match property {
        Property::Plain(plain) => {
            for item in plain.items.iter() {
                let var = item.variable();
                let name = {
                    let s = bytes_to_str(var.name);
                    s.strip_prefix('$').unwrap_or(s).to_string()
                };
                let var_offset = var.span.start.offset;
                ctx.spans.push(SymbolSpan {
                    start: var_offset,
                    end: var.span.end.offset,
                    kind: SymbolKind::Variable {
                        name: crate::atom::atom(&name),
                    },
                });
                ctx.var_defs.push(VarDefSite {
                    offset: var_offset,
                    name,
                    kind: VarDefKind::Property,
                    scope_start: 0,
                    effective_from: var_offset,
                    nesting_depth: ctx.cond_nesting_depth,
                    block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
                });
                // Walk the default value expression so that class
                // references like `Foo::class` in property defaults
                // produce navigable spans.
                if let PropertyItem::Concrete(concrete) = item {
                    extract_from_expression(concrete.value, ctx, 0);
                }
            }
        }
        Property::Hooked(hooked) => {
            let var = hooked.item.variable();
            let name = {
                let s = bytes_to_str(var.name);
                s.strip_prefix('$').unwrap_or(s).to_string()
            };
            let var_offset = var.span.start.offset;
            ctx.spans.push(SymbolSpan {
                start: var_offset,
                end: var.span.end.offset,
                kind: SymbolKind::Variable {
                    name: crate::atom::atom(&name),
                },
            });
            ctx.var_defs.push(VarDefSite {
                offset: var_offset,
                name,
                kind: VarDefKind::Property,
                scope_start: 0,
                effective_from: var_offset,
                nesting_depth: ctx.cond_nesting_depth,
                block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
            });
            if let PropertyItem::Concrete(concrete) = &hooked.item {
                extract_from_expression(concrete.value, ctx, 0);
            }
            for hook in hooked.hook_list.hooks.iter() {
                extract_from_hook(hook, ctx);
            }
        }
    }
}

/// Extract a single `get`/`set` hook.
///
/// A hook body is a method body: it has its own variable scope, `$this`
/// is available inside it, and a `set` hook may declare the assigned
/// value as a parameter.  Both body spellings (`{ … }` and `=> expr;`)
/// carry all of that, so both are walked.
fn extract_from_hook<'a>(hook: &'a PropertyHook<'a>, ctx: &mut ExtractionCtx<'a>) {
    extract_from_attribute_lists(&hook.attribute_lists, ctx, 0);

    let PropertyHookBody::Concrete(body) = &hook.body else {
        return;
    };

    // A block hook scopes from its opening brace, an expression hook from
    // its arrow — the same rule as a method body, whose scope starts at
    // `{` and leaves the parameter list outside.
    let (scope_start, scope_end) = match body {
        PropertyHookConcreteBody::Block(block) => {
            (block.left_brace.start.offset, block.right_brace.end.offset)
        }
        PropertyHookConcreteBody::Expression(expr_body) => {
            (expr_body.arrow.start.offset, expr_body.semicolon.end.offset)
        }
    };
    ctx.scopes.push((scope_start, scope_end));
    // A property hook is never static, so `$this` is always in scope.
    ctx.instance_method_scopes.push((scope_start, scope_end));

    if let Some(params) = &hook.parameter_list {
        for param in params.parameters.iter() {
            extract_from_hook_parameter(param, ctx, scope_start);
        }
    }

    match body {
        PropertyHookConcreteBody::Block(block) => {
            for stmt in block.statements.iter() {
                extract_from_statement(stmt, ctx, scope_start);
            }
        }
        PropertyHookConcreteBody::Expression(expr_body) => {
            extract_from_expression(expr_body.expression, ctx, scope_start);
        }
    }
}

/// Extract the `$value` parameter a `set` hook declares, so it is
/// navigable, renameable, and offered by variable completion inside the
/// hook body.
fn extract_from_hook_parameter<'a>(
    param: &'a FunctionLikeParameter<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    extract_from_attribute_lists(&param.attribute_lists, ctx, 0);
    if let Some(ref hint) = param.hint {
        extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }

    let name = {
        let s = bytes_to_str(param.variable.name);
        s.strip_prefix('$').unwrap_or(s).to_string()
    };
    let offset = param.variable.span.start.offset;
    ctx.spans.push(SymbolSpan {
        start: offset,
        end: param.variable.span.end.offset,
        kind: SymbolKind::Variable {
            name: crate::atom::atom(&name),
        },
    });
    ctx.var_defs.push(VarDefSite {
        offset,
        name,
        kind: VarDefKind::Parameter,
        scope_start,
        effective_from: offset,
        nesting_depth: ctx.cond_nesting_depth,
        block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
    });

    if let Some(ref default) = param.default_value {
        extract_from_expression(default.value, ctx, scope_start);
    }
}

pub(super) fn extract_from_class_constant<'a>(
    constant: &'a ClassLikeConstant<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    extract_from_attribute_lists(&constant.attribute_lists, ctx, 0);

    // Constant name(s) — declaration site spans for find-references and rename.
    // Class constants are always accessed statically (Foo::CONST).
    for item in constant.items.iter() {
        ctx.spans.push(SymbolSpan {
            start: item.name.span.start.offset,
            end: item.name.span.end.offset,
            kind: SymbolKind::MemberDeclaration {
                name: crate::atom::atom_bytes(item.name.value),
                is_static: true,
            },
        });
    }

    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, constant)
    {
        let _found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
    }

    // Type hint on constant (PHP 8.3+).
    if let Some(ref hint) = constant.hint {
        extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }

    for item in constant.items.iter() {
        extract_from_expression(item.value, ctx, 0);
    }
}

// ─── Function extractor ─────────────────────────────────────────────────────

pub(super) fn extract_from_function<'a>(func: &'a Function<'a>, ctx: &mut ExtractionCtx<'a>) {
    extract_from_attribute_lists(&func.attribute_lists, ctx, 0);

    // Function name as a navigable reference.
    let name = crate::atom::atom_bytes(func.name.value);
    ctx.spans.push(SymbolSpan {
        start: func.name.span.start.offset,
        end: func.name.span.end.offset,
        kind: SymbolKind::FunctionCall {
            name,
            is_definition: true,
            is_docblock_reference: false,
        },
    });

    // Docblock.  We extract type spans and template params now, but
    // defer `@param $var` variable spans until after we know
    // `func_scope_start` (the body's opening-brace offset).
    let func_docblock = get_docblock_text_with_offset(ctx.trivias, ctx.content, func);
    let mut docblock_params: Vec<(String, u32)> = Vec::new();
    if let Some((doc_text, doc_offset)) = func_docblock {
        let found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        docblock_params = found.param_vars;
        let scope_end = func.body.right_brace.end.offset;
        for (name, name_offset, bound, variance) in found.templates {
            ctx.template_defs.push(TemplateParamDef {
                name_offset,
                name,
                bound,
                variance,
                scope_start: doc_offset,
                scope_end,
            });
        }
    }

    // Determine scope_start for this function body.
    let func_scope_start = func.body.left_brace.start.offset;
    let func_scope_end = func.body.right_brace.end.offset;
    ctx.scopes.push((func_scope_start, func_scope_end));

    // Emit Variable spans and VarDefSite markers for `@param $varName`
    // tokens in the docblock so that rename and find-references cover
    // them.  The VarDefSite with `DocblockParam` kind lets
    // `find_variable_scope` map the pre-body offset to the correct
    // function body scope.
    {
        for (name, file_offset) in docblock_params {
            let end = file_offset + 1 + name.len() as u32;
            ctx.spans.push(SymbolSpan {
                start: file_offset,
                end,
                kind: SymbolKind::Variable {
                    name: crate::atom::atom(&name),
                },
            });
            ctx.var_defs.push(VarDefSite {
                offset: file_offset,
                name,
                kind: VarDefKind::DocblockParam,
                scope_start: func_scope_start,
                effective_from: file_offset,
                nesting_depth: ctx.cond_nesting_depth,
                block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
            });
        }
    }

    // Parameter type hints, variable spans, and variable definition sites.
    for param in func.parameter_list.parameters.iter() {
        extract_from_attribute_lists(&param.attribute_lists, ctx, 0);
        if let Some(ref hint) = param.hint {
            extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
        }
        // Docblock attached to the parameter itself (e.g. `/** @var list<Foo> */`).
        if let Some((doc_text, doc_offset)) =
            get_docblock_text_with_offset(ctx.trivias, ctx.content, param)
        {
            let _found = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        }
        let pname = {
            let s = bytes_to_str(param.variable.name);
            s.strip_prefix('$').unwrap_or(s).to_string()
        };
        let param_offset = param.variable.span.start.offset;
        // Emit a Variable span so the symbol map covers the parameter
        // token itself (needed for GTD-from-parameter-to-type-hint).
        ctx.spans.push(SymbolSpan {
            start: param_offset,
            end: param.variable.span.end.offset,
            kind: SymbolKind::Variable {
                name: crate::atom::atom(&pname),
            },
        });
        ctx.var_defs.push(VarDefSite {
            offset: param_offset,
            name: pname,
            kind: VarDefKind::Parameter,
            scope_start: func_scope_start,
            effective_from: param_offset,
            nesting_depth: ctx.cond_nesting_depth,
            block_end: ctx.cond_block_end_stack.last().copied().unwrap_or(u32::MAX),
        });
        if let Some(ref default) = param.default_value {
            extract_from_expression(default.value, ctx, func_scope_start);
        }
    }

    if let Some(ref return_type) = func.return_type_hint {
        extract_from_hint_ctx(&return_type.hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }

    for stmt in func.body.statements.iter() {
        extract_from_statement(stmt, ctx, func_scope_start);
    }
}

// ─── Use statement extractor ────────────────────────────────────────────────

/// What a `use` item imports.
///
/// The three kinds live in separate PHP symbol tables, so each one has
/// to be indexed as the symbol it actually names — a `use function`
/// item indexed as a class reference resolves to nothing, which is what
/// leaves the import line unnavigable.
#[derive(Clone, Copy)]
enum UseItemKind {
    Class,
    Function,
    Constant,
}

impl UseItemKind {
    /// Read the kind off a `use` statement's type keyword.
    fn from_type(r#type: &UseType<'_>) -> Self {
        if r#type.is_function() {
            Self::Function
        } else if r#type.is_const() {
            Self::Constant
        } else {
            Self::Class
        }
    }
}

pub(super) fn extract_from_use_statement(use_stmt: &Use<'_>, spans: &mut Vec<SymbolSpan>) {
    fn register_use_item(
        item: &UseItem<'_>,
        prefix: Option<&str>,
        item_kind: UseItemKind,
        spans: &mut Vec<SymbolSpan>,
    ) {
        let raw = bytes_to_str(item.name.value());
        let full = if let Some(prefix) = prefix {
            format!("{}\\{}", prefix, raw)
        } else {
            raw.to_string()
        };
        // Use statement names are always fully qualified (even without a
        // leading `\`), so force `is_fqn = true`.  `class_ref_span`
        // derives the flag from a leading `\` which use statements omit.
        let name = crate::atom::atom(strip_fqn_prefix(&full));
        let kind = match item_kind {
            UseItemKind::Class => SymbolKind::ClassReference {
                name,
                is_fqn: true,
                context: ClassRefContext::UseImport,
            },
            // Not a call site, but the same symbol a call site names, so
            // definition, hover, references and rename all reach it the
            // way they reach any other mention of the function.  The
            // diagnostics that would otherwise read this as a call skip
            // `use` statement lines already.
            UseItemKind::Function => SymbolKind::FunctionCall {
                name,
                is_definition: false,
                is_docblock_reference: false,
            },
            UseItemKind::Constant => SymbolKind::ConstantReference {
                name,
                is_definition: false,
            },
        };
        spans.push(SymbolSpan {
            start: item.name.span().start.offset,
            end: item.name.span().end.offset,
            kind,
        });
    }

    match &use_stmt.items {
        UseItems::Sequence(seq) => {
            for use_item in seq.items.iter() {
                register_use_item(use_item, None, UseItemKind::Class, spans);
            }
        }
        UseItems::TypedSequence(typed_seq) => {
            let item_kind = UseItemKind::from_type(&typed_seq.r#type);
            for use_item in typed_seq.items.iter() {
                register_use_item(use_item, None, item_kind, spans);
            }
        }
        UseItems::TypedList(list) => {
            let item_kind = UseItemKind::from_type(&list.r#type);
            let prefix = bytes_to_str(list.namespace.value());
            for use_item in list.items.iter() {
                register_use_item(use_item, Some(prefix), item_kind, spans);
            }
        }
        UseItems::MixedList(list) => {
            let prefix = bytes_to_str(list.namespace.value());
            for use_item in list.items.iter() {
                // `use Foo\{Bar, function baz, const QUX};` — an item
                // without its own type keyword is a class import.
                let item_kind = use_item
                    .r#type
                    .as_ref()
                    .map_or(UseItemKind::Class, UseItemKind::from_type);
                register_use_item(&use_item.item, Some(prefix), item_kind, spans);
            }
        }
    }
}

// ─── Type hint extractor ────────────────────────────────────────────────────

/// Extract navigable symbols from a type hint, tagging emitted
/// `ClassReference` spans with the given [`ClassRefContext`].
pub(super) fn extract_from_hint_ctx(
    hint: &Hint<'_>,
    spans: &mut Vec<SymbolSpan>,
    ref_ctx: ClassRefContext,
) {
    match hint {
        Hint::Identifier(ident) => {
            let raw = bytes_to_str(ident.value()).to_string();
            let name_clean = strip_fqn_prefix(&raw).to_string();
            // The parser gives every type PHP supports natively its own
            // `Hint` variant, so an identifier here is a class name as far
            // as PHP is concerned — including `resource`, `integer`, and
            // `number`, which mean something in a docblock and nothing in a
            // declaration ("`resource` is not a supported builtin type and
            // will be interpreted as a class name"). Reading them as the
            // class references they are is what gets them reported instead
            // of waved through by the docblock type vocabulary.
            if !crate::php_type::is_native_type_name(&name_clean) {
                spans.push(class_ref_span_ctx(
                    ident.span().start.offset,
                    ident.span().end.offset,
                    &raw,
                    ref_ctx,
                ));
            }
        }
        Hint::Nullable(nullable) => {
            extract_from_hint_ctx(nullable.hint, spans, ref_ctx);
        }
        Hint::Union(union) => {
            extract_from_hint_ctx(union.left, spans, ref_ctx);
            extract_from_hint_ctx(union.right, spans, ref_ctx);
        }
        Hint::Intersection(intersection) => {
            extract_from_hint_ctx(intersection.left, spans, ref_ctx);
            extract_from_hint_ctx(intersection.right, spans, ref_ctx);
        }
        Hint::Parenthesized(paren) => {
            extract_from_hint_ctx(paren.hint, spans, ref_ctx);
        }
        Hint::Self_(kw) => {
            spans.push(SymbolSpan {
                start: kw.span.start.offset,
                end: kw.span.end.offset,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Self_),
            });
        }
        Hint::Static(kw) => {
            spans.push(SymbolSpan {
                start: kw.span.start.offset,
                end: kw.span.end.offset,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Static),
            });
        }
        Hint::Parent(kw) => {
            spans.push(SymbolSpan {
                start: kw.span.start.offset,
                end: kw.span.end.offset,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Parent),
            });
        }
        // Scalar / built-in type hints are not navigable.
        _ => {}
    }
}
