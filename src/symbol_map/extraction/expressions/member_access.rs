use super::*;

// ─── Property / constant access ─────────────────────────────────────────────

pub(super) fn extract_access_expr<'a>(
    access: &'a Access<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match access {
        Access::Property(pa) => {
            extract_property_access(pa.object, &pa.property, false, ctx, scope_start)
        }
        Access::NullSafeProperty(pa) => {
            extract_property_access(pa.object, &pa.property, true, ctx, scope_start)
        }
        Access::StaticProperty(spa) => {
            let subject_text = subject_text_of(spa.class, ctx.content);
            emit_class_expr_span(spa.class, ClassRefContext::Other, ctx, scope_start);

            if let Variable::Direct(dv) = &spa.property {
                let prop_name = {
                    let s = bytes_to_str(dv.name);
                    crate::atom::atom(s.strip_prefix('$').unwrap_or(s))
                };
                ctx.spans.push(SymbolSpan {
                    start: dv.span.start.offset,
                    end: dv.span.end.offset,
                    kind: SymbolKind::MemberAccess {
                        subject_text,
                        member_name: prop_name,
                        is_static: true,
                        is_method_call: false,
                        docblock_ref: DocblockMemberRef::No,
                        is_array_callable: false,
                        is_nullsafe: false,
                    },
                });
            }
        }
        Access::ClassConstant(cca) => {
            let subject_text = subject_text_of(cca.class, ctx.content);
            emit_class_expr_span(cca.class, ClassRefContext::Other, ctx, scope_start);

            // In `Foo::class` the navigable part is `Foo`.
            if let ClassLikeConstantSelector::Identifier(ident) = &cca.constant
                && !crate::parser::is_class_fetch(cca)
            {
                ctx.spans.push(SymbolSpan {
                    start: ident.span.start.offset,
                    end: ident.span.end.offset,
                    kind: SymbolKind::MemberAccess {
                        subject_text,
                        member_name: crate::atom::atom(bytes_to_str(ident.value)),
                        is_static: true,
                        is_method_call: false,
                        docblock_ref: DocblockMemberRef::No,
                        is_array_callable: false,
                        is_nullsafe: false,
                    },
                });
            }
        }
    }
}

/// `$object->property` or `$object?->property`.
fn extract_property_access<'a>(
    object: &'a Expression<'a>,
    property: &'a ClassLikeMemberSelector<'a>,
    is_nullsafe: bool,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    let subject_text = subject_text_of(object, ctx.content);
    extract_from_expression(object, ctx, scope_start);

    match property {
        ClassLikeMemberSelector::Identifier(ident) => {
            ctx.spans.push(SymbolSpan {
                start: ident.span.start.offset,
                end: ident.span.end.offset,
                kind: SymbolKind::MemberAccess {
                    subject_text,
                    member_name: crate::atom::atom_bytes(ident.value),
                    is_static: false,
                    is_method_call: false,
                    docblock_ref: DocblockMemberRef::No,
                    is_array_callable: false,
                    is_nullsafe,
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
