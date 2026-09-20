use mago_span::HasSpan;

use super::*;

// ─── Property / constant access ─────────────────────────────────────────────

pub(super) fn extract_access_expr<'a>(
    access: &'a Access<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match access {
        Access::Property(pa) => {
            let object_span = pa.object.span();
            let subject_text = SubjectText::new(
                expr_to_subject_text(pa.object),
                object_span.start.offset,
                object_span.end.offset,
                ctx.content,
            );
            extract_from_expression(pa.object, ctx, scope_start);

            match &pa.property {
                ClassLikeMemberSelector::Identifier(ident) => {
                    let member_name = crate::atom::atom_bytes(ident.value);
                    ctx.spans.push(SymbolSpan {
                        start: ident.span.start.offset,
                        end: ident.span.end.offset,
                        kind: SymbolKind::MemberAccess {
                            subject_text,
                            member_name,
                            is_static: false,
                            is_method_call: false,
                            docblock_ref: DocblockMemberRef::No,
                            is_array_callable: false,
                            is_nullsafe: false,
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
            let object_span = pa.object.span();
            let subject_text = SubjectText::new(
                expr_to_subject_text(pa.object),
                object_span.start.offset,
                object_span.end.offset,
                ctx.content,
            );
            extract_from_expression(pa.object, ctx, scope_start);

            match &pa.property {
                ClassLikeMemberSelector::Identifier(ident) => {
                    let member_name = crate::atom::atom_bytes(ident.value);
                    ctx.spans.push(SymbolSpan {
                        start: ident.span.start.offset,
                        end: ident.span.end.offset,
                        kind: SymbolKind::MemberAccess {
                            subject_text,
                            member_name,
                            is_static: false,
                            is_method_call: false,
                            docblock_ref: DocblockMemberRef::No,
                            is_array_callable: false,
                            is_nullsafe: true,
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
            let class_span = spa.class.span();
            let subject_text = SubjectText::new(
                expr_to_subject_text(spa.class),
                class_span.start.offset,
                class_span.end.offset,
                ctx.content,
            );
            emit_class_expr_span(spa.class, ctx, scope_start);

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
            let class_span = cca.class.span();
            let subject_text = SubjectText::new(
                expr_to_subject_text(cca.class),
                class_span.start.offset,
                class_span.end.offset,
                ctx.content,
            );
            emit_class_expr_span(cca.class, ctx, scope_start);

            if let ClassLikeConstantSelector::Identifier(ident) = &cca.constant {
                let const_name = bytes_to_str(ident.value);
                if const_name == "class" {
                    // `Foo::class` — the navigable part is `Foo`.
                } else {
                    ctx.spans.push(SymbolSpan {
                        start: ident.span.start.offset,
                        end: ident.span.end.offset,
                        kind: SymbolKind::MemberAccess {
                            subject_text,
                            member_name: crate::atom::atom(const_name),
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
}
