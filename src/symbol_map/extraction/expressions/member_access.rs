use mago_syntax::cst::*;

use super::*;

// ─── Property / constant access ─────────────────────────────────────────────

pub(super) fn extract_access_expr<'a>(
    access: &'a Access<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
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
