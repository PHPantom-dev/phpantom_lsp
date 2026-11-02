//! The hook names WordPress's hook functions take: the first argument of
//! `do_action()`, `apply_filters()`, `add_action()`, `add_filter()`, and the
//! rest of the family (see [`crate::wordpress::hooks`]).
//!
//! A name built at runtime, interpolated (`"save_post_{$post->post_type}"`)
//! or concatenated (`'save_post_' . $post->post_type`), is recorded as a
//! pattern: its literal parts, and each computed part as its source text in
//! braces. A name with no literal part at all names no hook in particular,
//! and is left out.

use super::*;
use crate::symbol_map::StringKeyKind;
use crate::virtual_members::laravel::helpers::resolved_string_literal;
use crate::wordpress::hooks::{HOOK_PARAMETER, HookRole, hook_function_role};

/// Emit the hook span of a call to the function written `written`, when it
/// is one of WordPress's hook functions.
///
/// A name qualified with a namespace of its own names some other function;
/// an unqualified one falls back to WordPress's global one.
pub(in crate::symbol_map::extraction) fn emit_wordpress_hook_span(
    written: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let name = written.strip_prefix('\\').unwrap_or(written);
    if name.contains('\\') {
        return;
    }
    let Some(role) = hook_function_role(name) else {
        return;
    };
    let Some(argument) = argument_expr_for_parameter(argument_list, HOOK_PARAMETER) else {
        return;
    };
    let Some((key, start, end)) = hook_name(argument, ctx.content) else {
        return;
    };
    ctx.spans.push(SymbolSpan {
        start,
        end,
        kind: SymbolKind::StringKey {
            key,
            kind: StringKeyKind::WordPressHook,
            is_write: role == HookRole::Fires,
            is_optional: false,
        },
    });
}

/// The hook name `expr` spells, and the byte range of the literal that
/// spells it: the whole string's contents for a literal name, its first
/// literal part for a pattern.
fn hook_name(expr: &Expression<'_>, content: &str) -> Option<(String, u32, u32)> {
    if let Some((value, start, end)) = resolved_string_literal(expr, content) {
        // A brace would read as a computed part.
        return (!value.contains('{')).then(|| (value.to_string(), start as u32, end as u32));
    }
    let mut pattern = Pattern::default();
    match expr {
        Expression::CompositeString(string @ CompositeString::Interpolated(_)) => {
            for part in string.parts().iter() {
                match part {
                    StringPart::Literal(literal) => {
                        let value = literal.value.and_then(literal_bytes_to_str)?;
                        let span = literal.span;
                        pattern.literal(value, span.start.offset, span.end.offset)?;
                    }
                    StringPart::Expression(expression) => {
                        pattern.computed(expression, content)?;
                    }
                    StringPart::BracedExpression(braced) => {
                        pattern.computed(braced.expression, content)?;
                    }
                }
            }
        }
        Expression::Binary(binary) if binary.operator.is_concatenation() => {
            // A concatenation nests to the left, so its operands are read
            // off the right of each link and come out last first.
            let mut operands = Vec::new();
            let mut current = expr;
            while let Expression::Binary(link) = current
                && link.operator.is_concatenation()
            {
                operands.push(link.rhs);
                current = link.lhs;
            }
            operands.push(current);
            for operand in operands.into_iter().rev() {
                match resolved_string_literal(operand, content) {
                    Some((value, start, end)) => {
                        pattern.literal(value, start as u32, end as u32)?;
                    }
                    None => pattern.computed(operand, content)?,
                }
            }
        }
        _ => return None,
    }
    let (start, end) = pattern.first_literal?;
    Some((pattern.key, start, end))
}

/// A pattern being built a part at a time.
#[derive(Default)]
struct Pattern {
    key: String,
    /// The byte range of the first non-empty literal part.
    first_literal: Option<(u32, u32)>,
}

impl Pattern {
    /// Append a literal part, whose source spans `start..end`. `None` when
    /// it holds a brace, which would read as a computed part.
    fn literal(&mut self, value: &str, start: u32, end: u32) -> Option<()> {
        if value.contains('{') {
            return None;
        }
        if !value.is_empty() && self.first_literal.is_none() {
            self.first_literal = Some((start, end));
        }
        self.key.push_str(value);
        Some(())
    }

    /// Append a computed part, written `expression`.
    fn computed(&mut self, expression: &Expression<'_>, content: &str) -> Option<()> {
        let span = expression.span();
        let source = content.get(span.start.offset as usize..span.end.offset as usize)?;
        self.key.push('{');
        self.key.push_str(source);
        self.key.push('}');
        Some(())
    }
}
