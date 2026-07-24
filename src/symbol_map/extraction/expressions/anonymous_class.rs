use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::*;

// ─── Anonymous class ─────────────────────────────────────────────────────────
// `new class(...) extends Foo implements Bar { ... }`

pub(super) fn extract_anonymous_class_expr<'a>(
    anon: &'a AnonymousClass<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
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
