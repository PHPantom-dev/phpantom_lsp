use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::*;

// ─── Class-like extractors ──────────────────────────────────────────────────

pub(super) fn extract_from_class<'a>(class: &'a Class<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Class name — declaration site, not a reference.
    let name = bytes_to_str(class.name.value).to_string();
    ctx.spans.push(SymbolSpan {
        start: class.name.span.start.offset,
        end: class.name.span.end.offset,
        kind: SymbolKind::ClassDeclaration { name },
    });

    // Attributes (PHP 8).
    extract_from_attribute_lists(&class.attribute_lists, ctx, 0);

    // Extends.
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

    // Implements.
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

    // Docblock.
    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, class)
    {
        let tpl_params = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        let scope_end = class.right_brace.end.offset;
        for (name, name_offset, bound, variance) in tpl_params {
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

    // Members.
    for member in class.members.iter() {
        extract_from_class_member(member, ctx);
    }
}

pub(super) fn extract_from_interface<'a>(iface: &'a Interface<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Interface name — declaration site, not a reference.
    let name = bytes_to_str(iface.name.value).to_string();
    ctx.spans.push(SymbolSpan {
        start: iface.name.span.start.offset,
        end: iface.name.span.end.offset,
        kind: SymbolKind::ClassDeclaration { name },
    });

    // Attributes (PHP 8).
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
        let tpl_params = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        let scope_end = iface.right_brace.end.offset;
        for (name, name_offset, bound, variance) in tpl_params {
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

    for member in iface.members.iter() {
        extract_from_class_member(member, ctx);
    }
}

pub(super) fn extract_from_trait<'a>(trait_def: &'a Trait<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Trait name — declaration site, not a reference.
    let name = bytes_to_str(trait_def.name.value).to_string();
    ctx.spans.push(SymbolSpan {
        start: trait_def.name.span.start.offset,
        end: trait_def.name.span.end.offset,
        kind: SymbolKind::ClassDeclaration { name },
    });

    // Attributes (PHP 8).
    extract_from_attribute_lists(&trait_def.attribute_lists, ctx, 0);

    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, trait_def)
    {
        let tpl_params = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        let scope_end = trait_def.right_brace.end.offset;
        for (name, name_offset, bound, variance) in tpl_params {
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

    for member in trait_def.members.iter() {
        extract_from_class_member(member, ctx);
    }
}

pub(super) fn extract_from_enum<'a>(enum_def: &'a Enum<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Enum name — declaration site, not a reference.
    let name = bytes_to_str(enum_def.name.value).to_string();
    ctx.spans.push(SymbolSpan {
        start: enum_def.name.span.start.offset,
        end: enum_def.name.span.end.offset,
        kind: SymbolKind::ClassDeclaration { name },
    });

    // Attributes (PHP 8).
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
        let tpl_params = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        let scope_end = enum_def.right_brace.end.offset;
        for (name, name_offset, bound, variance) in tpl_params {
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
                // emit a LaravelStringKey::Config span so hover,
                // go-to-definition, and diagnostics work on the key.
                //
                // FQN attributes match directly. Short names require
                // the file to import from the Illuminate namespace;
                // that check is cached once per file to avoid repeated
                // linear scans.
                if let Some(kind) = resolve_laravel_container_attr(
                    class_name,
                    &mut ctx.has_laravel_container_attrs,
                    ctx.content,
                ) {
                    try_emit_laravel_string_span_partial(
                        kind,
                        arg_list,
                        ctx.content,
                        &mut ctx.spans,
                    );
                }
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
                let _tpl = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
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
            // Attributes (PHP 8) on the enum case.
            extract_from_attribute_lists(&enum_case.attribute_lists, ctx, 0);

            // Enum case name — declaration site span for find-references,
            // rename, and document-highlights.  Enum cases are accessed
            // statically (`self::Issue`, `TaskType::Issue`).
            let case_name_ident = enum_case.item.name();
            ctx.spans.push(SymbolSpan {
                start: case_name_ident.span.start.offset,
                end: case_name_ident.span.end.offset,
                kind: SymbolKind::MemberDeclaration {
                    name: bytes_to_str(case_name_ident.value).to_string(),
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
            // Emit ClassReference for the trait name.
            let trait_raw = bytes_to_str(abs.trait_name.value()).to_string();
            ctx.spans.push(class_ref_span(
                abs.trait_name.span().start.offset,
                abs.trait_name.span().end.offset,
                &trait_raw,
            ));
            // Emit MemberAccess for the original method name.
            let method_name = bytes_to_str(abs.method_name.value).to_string();
            ctx.spans.push(SymbolSpan {
                start: abs.method_name.span.start.offset,
                end: abs.method_name.span.end.offset,
                kind: SymbolKind::MemberAccess {
                    subject_text: trait_raw,
                    member_name: method_name,
                    is_static: true,
                    is_method_call: true,
                    is_docblock_reference: false,
                    is_array_callable: false,
                },
            });
        }
        TraitUseMethodReference::Identifier(ident) => {
            // Unqualified reference: use the first trait name from the
            // `use` list, or fall back to `self`.
            let subject = first_trait_name.unwrap_or("self").to_string();
            let method_name = bytes_to_str(ident.value).to_string();
            ctx.spans.push(SymbolSpan {
                start: ident.span.start.offset,
                end: ident.span.end.offset,
                kind: SymbolKind::MemberAccess {
                    subject_text: subject,
                    member_name: method_name,
                    is_static: true,
                    is_method_call: true,
                    is_docblock_reference: false,
                    is_array_callable: false,
                },
            });
        }
    }

    // Emit MemberAccess for the alias name (the `as` target).
    // Using `self` as the subject so that `resolve_trait_alias` on
    // the owning class maps the alias back to the original method.
    if let Some(ref alias_ident) = alias_adapt.alias {
        let alias_name = bytes_to_str(alias_ident.value).to_string();
        ctx.spans.push(SymbolSpan {
            start: alias_ident.span.start.offset,
            end: alias_ident.span.end.offset,
            kind: SymbolKind::MemberAccess {
                subject_text: "self".to_string(),
                member_name: alias_name,
                is_static: true,
                is_method_call: true,
                is_docblock_reference: false,
                is_array_callable: false,
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
    // Emit ClassReference for the trait name in the method reference.
    let trait_raw = bytes_to_str(prec.method_reference.trait_name.value()).to_string();
    ctx.spans.push(class_ref_span(
        prec.method_reference.trait_name.span().start.offset,
        prec.method_reference.trait_name.span().end.offset,
        &trait_raw,
    ));

    // Emit MemberAccess for the method name.
    let method_name = bytes_to_str(prec.method_reference.method_name.value).to_string();
    ctx.spans.push(SymbolSpan {
        start: prec.method_reference.method_name.span.start.offset,
        end: prec.method_reference.method_name.span.end.offset,
        kind: SymbolKind::MemberAccess {
            subject_text: trait_raw,
            member_name: method_name,
            is_static: true,
            is_method_call: true,
            is_docblock_reference: false,
            is_array_callable: false,
        },
    });

    // Emit ClassReference for each `insteadof` trait name.
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
            name: bytes_to_str(method.name.value).to_string(),
            is_static,
        },
    });

    // Attributes (PHP 8) on the method.
    extract_from_attribute_lists(&method.attribute_lists, ctx, 0);

    // Docblock on the method.  We extract type spans and template params
    // now, but defer `@param $var` variable spans until after we know
    // `method_scope_start` (the body's opening-brace offset).
    let method_docblock = get_docblock_text_with_offset(ctx.trivias, ctx.content, method);
    if let Some((doc_text, doc_offset)) = method_docblock {
        let tpl_params = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
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
        for (name, name_offset, bound, variance) in tpl_params {
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
    if let Some((doc_text, doc_offset)) = method_docblock {
        for (name, file_offset) in extract_param_var_spans(doc_text, doc_offset) {
            let end = file_offset + 1 + name.len() as u32;
            ctx.spans.push(SymbolSpan {
                start: file_offset,
                end,
                kind: SymbolKind::Variable { name: name.clone() },
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
        // Attributes (PHP 8) on the parameter.
        extract_from_attribute_lists(&param.attribute_lists, ctx, 0);
        if let Some(ref hint) = param.hint {
            extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
        }
        // Docblock attached to the parameter itself (e.g. promoted
        // constructor properties with `/** @var list<Subscription> */`).
        if let Some((doc_text, doc_offset)) =
            get_docblock_text_with_offset(ctx.trivias, ctx.content, param)
        {
            let _tpl = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
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
            kind: SymbolKind::Variable { name: name.clone() },
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
    }

    // Return type hint.
    if let Some(ref return_type) = method.return_type_hint {
        extract_from_hint_ctx(&return_type.hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }

    // Method body.
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
        let _tpl = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);

        // Emit VarDefSite entries for `@var Type $varName` in inline docblocks.
        for (name, file_offset) in extract_var_docblock_var_spans(doc_text, doc_offset) {
            let name_len = name.len() as u32 + 1; // +1 for the `$` prefix
            ctx.spans.push(SymbolSpan {
                start: file_offset,
                end: file_offset + name_len,
                kind: SymbolKind::Variable { name: name.clone() },
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

pub(super) fn extract_from_property<'a>(property: &Property<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Attributes (PHP 8) on the property.
    match property {
        Property::Plain(plain) => extract_from_attribute_lists(&plain.attribute_lists, ctx, 0),
        Property::Hooked(hooked) => extract_from_attribute_lists(&hooked.attribute_lists, ctx, 0),
    }

    // Docblock.
    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, property)
    {
        // Property docblocks don't define template params, but we still
        // need to consume the return value.
        let _tpl = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
    }

    // Property type hint.
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
                    kind: SymbolKind::Variable { name: name.clone() },
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
                kind: SymbolKind::Variable { name: name.clone() },
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
        }
    }
}

pub(super) fn extract_from_class_constant<'a>(
    constant: &'a ClassLikeConstant<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    // Attributes (PHP 8) on the constant.
    extract_from_attribute_lists(&constant.attribute_lists, ctx, 0);

    // Constant name(s) — declaration site spans for find-references and rename.
    // Class constants are always accessed statically (Foo::CONST).
    for item in constant.items.iter() {
        ctx.spans.push(SymbolSpan {
            start: item.name.span.start.offset,
            end: item.name.span.end.offset,
            kind: SymbolKind::MemberDeclaration {
                name: bytes_to_str(item.name.value).to_string(),
                is_static: true,
            },
        });
    }

    // Docblock.
    if let Some((doc_text, doc_offset)) =
        get_docblock_text_with_offset(ctx.trivias, ctx.content, constant)
    {
        let _tpl = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
    }

    // Type hint on constant (PHP 8.3+).
    if let Some(ref hint) = constant.hint {
        extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }

    // Constant value expressions.
    for item in constant.items.iter() {
        extract_from_expression(item.value, ctx, 0);
    }
}

// ─── Function extractor ─────────────────────────────────────────────────────

pub(super) fn extract_from_function<'a>(func: &'a Function<'a>, ctx: &mut ExtractionCtx<'a>) {
    // Attributes (PHP 8) on the function.
    extract_from_attribute_lists(&func.attribute_lists, ctx, 0);

    // Function name as a navigable reference.
    let name = bytes_to_str(func.name.value).to_string();
    ctx.spans.push(SymbolSpan {
        start: func.name.span.start.offset,
        end: func.name.span.end.offset,
        kind: SymbolKind::FunctionCall {
            name,
            is_definition: true,
        },
    });

    // Docblock.  We extract type spans and template params now, but
    // defer `@param $var` variable spans until after we know
    // `func_scope_start` (the body's opening-brace offset).
    let func_docblock = get_docblock_text_with_offset(ctx.trivias, ctx.content, func);
    if let Some((doc_text, doc_offset)) = func_docblock {
        let tpl_params = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        let scope_end = func.body.right_brace.end.offset;
        for (name, name_offset, bound, variance) in tpl_params {
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
    if let Some((doc_text, doc_offset)) = func_docblock {
        for (name, file_offset) in extract_param_var_spans(doc_text, doc_offset) {
            let end = file_offset + 1 + name.len() as u32;
            ctx.spans.push(SymbolSpan {
                start: file_offset,
                end,
                kind: SymbolKind::Variable { name: name.clone() },
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
        // Attributes (PHP 8) on the parameter.
        extract_from_attribute_lists(&param.attribute_lists, ctx, 0);
        if let Some(ref hint) = param.hint {
            extract_from_hint_ctx(hint, &mut ctx.spans, ClassRefContext::TypeHint);
        }
        // Docblock attached to the parameter itself (e.g. `/** @var list<Foo> */`).
        if let Some((doc_text, doc_offset)) =
            get_docblock_text_with_offset(ctx.trivias, ctx.content, param)
        {
            let _tpl = extract_docblock_symbols(doc_text, doc_offset, &mut ctx.spans);
        }
        // Emit VarDefSite for each parameter.
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
                name: pname.clone(),
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

    // Return type hint.
    if let Some(ref return_type) = func.return_type_hint {
        extract_from_hint_ctx(&return_type.hint, &mut ctx.spans, ClassRefContext::TypeHint);
    }

    // Function body.
    for stmt in func.body.statements.iter() {
        extract_from_statement(stmt, ctx, func_scope_start);
    }
}

// ─── Use statement extractor ────────────────────────────────────────────────

pub(super) fn extract_from_use_statement(use_stmt: &Use<'_>, spans: &mut Vec<SymbolSpan>) {
    fn register_use_item(item: &UseItem<'_>, prefix: Option<&str>, spans: &mut Vec<SymbolSpan>) {
        let raw = bytes_to_str(item.name.value());
        let full = if let Some(prefix) = prefix {
            format!("{}\\{}", prefix, raw)
        } else {
            raw.to_string()
        };
        // Use statement names are always fully qualified (even without a
        // leading `\`), so force `is_fqn = true`.  `class_ref_span`
        // derives the flag from a leading `\` which use statements omit.
        let name = strip_fqn_prefix(&full).to_string();
        spans.push(SymbolSpan {
            start: item.name.span().start.offset,
            end: item.name.span().end.offset,
            kind: SymbolKind::ClassReference {
                name,
                is_fqn: true,
                context: ClassRefContext::UseImport,
            },
        });
    }

    match &use_stmt.items {
        UseItems::Sequence(seq) => {
            for use_item in seq.items.iter() {
                register_use_item(use_item, None, spans);
            }
        }
        UseItems::TypedSequence(typed_seq) => {
            // Only class imports (not function/const).
            if !typed_seq.r#type.is_function() && !typed_seq.r#type.is_const() {
                for use_item in typed_seq.items.iter() {
                    register_use_item(use_item, None, spans);
                }
            }
        }
        UseItems::TypedList(list) => {
            if !list.r#type.is_function() && !list.r#type.is_const() {
                let prefix = bytes_to_str(list.namespace.value());
                for use_item in list.items.iter() {
                    register_use_item(use_item, Some(prefix), spans);
                }
            }
        }
        UseItems::MixedList(list) => {
            let prefix = bytes_to_str(list.namespace.value());
            for use_item in list.items.iter() {
                // MixedList items are MaybeTypedUseItem — skip function/const.
                if let Some(ref typ) = use_item.r#type
                    && (typ.is_function() || typ.is_const())
                {
                    continue;
                }
                register_use_item(&use_item.item, Some(prefix), spans);
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
            if is_navigable_type(&name_clean) {
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
