//! **Convert to string interpolation** code action (`refactor.rewrite`).
//!
//! Rewrites a string concatenation that mixes literal text with simple
//! variable expressions into a single double-quoted interpolated string:
//!
//! ```php
//! $greeting = 'Hello ' . $name . ', welcome!';  // before
//! $greeting = "Hello {$name}, welcome!";        // after
//! ```
//!
//! Every operand has to be either a plain string literal or an expression
//! that starts with `$` and is legal inside `{…}` (variables, property
//! reads, array reads, method calls); anything else leaves the chain alone.
//! Numeric and boolean operands are deliberately excluded: they read no
//! better interpolated, and `true`/`false` would render as `1`/`""`.
//!
//! Interpolated parts always use the curly form, so a literal that runs
//! straight into a variable (`$name . 'x'`) cannot silently become a
//! reference to a different variable.
//!
//! The action is only offered where the concatenation is the whole
//! expression of a statement, the right-hand side of an assignment, an
//! `echo`/`print` operand, a `return` value, an arrow-function body, or a
//! `match` arm.  Inside a call argument the rewrite is a matter of taste,
//! so it stays out of the way there.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::text_position::position_to_byte_offset;

impl Backend {
    /// Collect "Convert to string interpolation" code actions for the
    /// concatenation at the cursor position.
    pub(crate) fn collect_convert_to_interpolation_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        let doc_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let cursor = position_to_byte_offset(content, params.range.start) as u32;

        let best = crate::parser::with_parsed_program(
            content,
            "convert_to_interpolation",
            |program, content| {
                let mut best: Option<(u32, u32, String)> = None;
                find_concat(Node::Program(program), false, cursor, content, &mut best);
                best
            },
        );

        let (start, end, replacement) = match best {
            Some(b) => b,
            None => return,
        };

        out.push(crate::code_actions::rewrite_action(
            "Convert to string interpolation".to_string(),
            doc_uri,
            content,
            (start as usize, end as usize),
            replacement,
            false,
        ));
    }
}

// ─── AST walking ────────────────────────────────────────────────────────────

/// Descend into the subtree containing the cursor looking for a convertible
/// concatenation.
///
/// `in_statement_position` says whether this node sits somewhere the rewrite
/// is offered (see the module docs).  Pruning to the subtree that contains
/// `cursor` keeps the walk linear in the depth of the cursor rather than the
/// size of the file, and the generic [`Node::visit_children`] descent means
/// no syntax node has to be enumerated to be reached.
fn find_concat(
    node: Node<'_, '_>,
    in_statement_position: bool,
    cursor: u32,
    content: &str,
    best: &mut Option<(u32, u32, String)>,
) {
    let span = node.span();
    if cursor < span.start.offset || cursor > span.end.offset {
        return;
    }

    if in_statement_position
        && let Node::Binary(binary) = node
        && binary.operator.is_concatenation()
        && let Some(replacement) = try_convert_concat(binary, content)
    {
        // Prefer the innermost match, and stop here: the operands of a
        // convertible chain are never themselves convertible chains.
        let size = span.end.offset - span.start.offset;
        if best.as_ref().is_none_or(|(s, e, _)| size < e - s) {
            *best = Some((span.start.offset, span.end.offset, replacement));
        }
        return;
    }

    let child_position = if is_transparent(&node) {
        in_statement_position
    } else {
        opens_statement_position(&node)
    };

    node.visit_children(|child| find_concat(child, child_position, cursor, content, best));
}

/// Nodes that merely wrap their child and so pass the current position
/// through unchanged.
fn is_transparent(node: &Node<'_, '_>) -> bool {
    matches!(node, Node::Expression(_) | Node::Parenthesized(_))
}

/// Nodes whose children hold an expression in a position where converting a
/// concatenation is offered.
fn opens_statement_position(node: &Node<'_, '_>) -> bool {
    matches!(
        node,
        Node::ExpressionStatement(_)
            | Node::Assignment(_)
            | Node::Return(_)
            | Node::Echo(_)
            | Node::EchoTag(_)
            | Node::PrintConstruct(_)
            | Node::ArrowFunction(_)
            | Node::MatchExpressionArm(_)
            | Node::MatchDefaultArm(_)
    )
}

// ─── Conversion ─────────────────────────────────────────────────────────────

/// Build the interpolated string that replaces `binary`, or `None` when the
/// chain does not qualify.
fn try_convert_concat(binary: &Binary<'_>, content: &str) -> Option<String> {
    let span = binary.span();
    let mut result = String::with_capacity((span.end.offset - span.start.offset) as usize + 2);
    result.push('"');

    let mut literals = 0usize;
    let mut expressions = 0usize;

    for operand in flatten_concat(binary) {
        if let Expression::Literal(literal) = operand {
            // Numbers, `true`/`false` and `null` are left as concatenation.
            let Literal::String(string) = literal else {
                return None;
            };
            push_escaped(&mut result, string.value?)?;
            literals += 1;
            continue;
        }

        if !is_interpolatable(operand) {
            return None;
        }

        let operand_span = operand.span();
        let text = &content[operand_span.start.offset as usize..operand_span.end.offset as usize];
        // A `"` would end the string early, a brace would confuse the
        // interpolation delimiters, and a newline would split the rewritten
        // expression across lines.
        if text.contains(['"', '{', '}', '\n', '\r']) {
            return None;
        }
        result.push('{');
        result.push_str(text);
        result.push('}');
        expressions += 1;
    }

    // A chain of only literals or only variables gains nothing.
    if literals == 0 || expressions == 0 {
        return None;
    }

    result.push('"');
    Some(result)
}

/// Flatten a left-associative `.` chain into its operands, left to right.
///
/// Walks the left spine iteratively: a chain built from thousands of pieces
/// (generated code does this) would otherwise recurse once per operand.
fn flatten_concat<'arena>(binary: &Binary<'arena>) -> Vec<&'arena Expression<'arena>> {
    let mut operands = vec![binary.rhs];
    let mut current = binary.lhs;
    while let Expression::Binary(inner) = current {
        if !inner.operator.is_concatenation() {
            break;
        }
        operands.push(inner.rhs);
        current = inner.lhs;
    }
    operands.push(current);
    operands.reverse();
    operands
}

/// Whether `expr` can be dropped into `{…}` inside a double-quoted string.
///
/// PHP only interpolates a braced expression when the character after the
/// brace is `$`, so the chain has to bottom out at a plain variable.  Member
/// names must be plain identifiers; a variable member name (`$obj->{$name}`)
/// would nest braces inside the interpolation.
fn is_interpolatable(expr: &Expression<'_>) -> bool {
    let mut current = expr;
    loop {
        current = match current {
            Expression::Variable(Variable::Direct(_)) => return true,
            Expression::ArrayAccess(access) => access.array,
            Expression::Access(Access::Property(access)) if is_plain_selector(&access.property) => {
                access.object
            }
            Expression::Access(Access::NullSafeProperty(access))
                if is_plain_selector(&access.property) =>
            {
                access.object
            }
            Expression::Call(Call::Method(call)) if is_plain_selector(&call.method) => call.object,
            Expression::Call(Call::NullSafeMethod(call)) if is_plain_selector(&call.method) => {
                call.object
            }
            _ => return false,
        };
    }
}

fn is_plain_selector(selector: &ClassLikeMemberSelector<'_>) -> bool {
    matches!(selector, ClassLikeMemberSelector::Identifier(_))
}

/// Append the decoded value of a string literal to `out`, escaped for a
/// double-quoted string.
///
/// The input is the literal's *value*, with escape sequences already
/// resolved, so `'a\nb'` (backslash, `n`) and `"a\nb"` (a newline) both come
/// back out meaning what they did before the rewrite.  Control characters
/// are written back as escape sequences to keep the result on one line.
/// Returns `None` for a value that is not valid UTF-8, which only a raw byte
/// escape can produce.
fn push_escaped(out: &mut String, value: &[u8]) -> Option<()> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    for ch in std::str::from_utf8(value).ok()?.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0b}' => out.push_str("\\v"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{1b}' => out.push_str("\\e"),
            _ if (ch as u32) < 0x20 || ch == '\u{7f}' => {
                let byte = ch as u32 as u8;
                out.push_str("\\x");
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0x0f) as usize] as char);
            }
            _ => out.push(ch),
        }
    }

    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Convert the first concatenation found anywhere in `php`, ignoring the
    /// position restrictions that the code-action entry point applies.
    fn convert(php: &str) -> Option<String> {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());

        let mut found = None;
        collect_first(Node::Program(program), php, &mut found);
        found
    }

    fn collect_first(node: Node<'_, '_>, content: &str, found: &mut Option<String>) {
        if found.is_some() {
            return;
        }
        if let Node::Binary(binary) = node
            && binary.operator.is_concatenation()
            && let Some(result) = try_convert_concat(binary, content)
        {
            *found = Some(result);
            return;
        }
        node.visit_children(|child| collect_first(child, content, found));
    }

    #[test]
    fn literal_and_variable() {
        assert_eq!(
            convert("<?php $g = 'Hello ' . $name . ', welcome!';").unwrap(),
            r#""Hello {$name}, welcome!""#
        );
    }

    #[test]
    fn method_call_uses_curly_syntax() {
        assert_eq!(
            convert(r#"<?php $m = "Total: " . $order->getTotal();"#).unwrap(),
            r#""Total: {$order->getTotal()}""#
        );
    }

    #[test]
    fn property_and_array_reads() {
        assert_eq!(
            convert("<?php $s = $user->name . ' <' . $row['email'] . '>';").unwrap(),
            r#""{$user->name} <{$row['email']}>""#
        );
    }

    #[test]
    fn double_quotes_and_dollars_are_escaped() {
        assert_eq!(
            convert(r#"<?php $s = 'say "hi" for $5: ' . $name;"#).unwrap(),
            r#""say \"hi\" for \$5: {$name}""#
        );
    }

    #[test]
    fn single_quoted_backslash_stays_literal() {
        // `'C:\path'` holds a single backslash; the rewrite must escape it so
        // the double-quoted form still holds one.
        assert_eq!(
            convert(r"<?php $s = 'C:\path\to' . $file;").unwrap(),
            r#""C:\\path\\to{$file}""#
        );
    }

    #[test]
    fn newlines_become_escape_sequences() {
        assert_eq!(
            convert("<?php $s = \"line\\n\" . $rest;").unwrap(),
            r#""line\n{$rest}""#
        );
    }

    #[test]
    fn rejected_when_all_literals() {
        assert!(convert("<?php $s = 'a' . 'b';").is_none());
    }

    #[test]
    fn rejected_when_all_variables() {
        assert!(convert("<?php $s = $a . $b;").is_none());
    }

    #[test]
    fn rejected_for_numeric_literal() {
        assert!(convert("<?php $s = 'n=' . 42 . $unit;").is_none());
    }

    #[test]
    fn rejected_for_boolean_literal() {
        assert!(convert("<?php $s = 'flag=' . true . $rest;").is_none());
    }

    #[test]
    fn rejected_for_function_call() {
        assert!(convert("<?php $s = 'n=' . count($items);").is_none());
    }

    #[test]
    fn rejected_for_static_access() {
        // `{Foo::BAR}` is not interpolated by PHP.
        assert!(convert("<?php $s = 'v=' . Config::VALUE;").is_none());
    }

    #[test]
    fn rejected_for_interpolated_string_operand() {
        // `"hi $a"` followed by literal text could merge the variable name
        // with the text that comes after it.
        assert!(convert(r#"<?php $s = "hi $a" . 'bc';"#).is_none());
    }

    #[test]
    fn rejected_for_double_quoted_argument() {
        assert!(convert(r#"<?php $s = 'v=' . $o->get("k");"#).is_none());
    }

    #[test]
    fn rejected_for_variable_property_name() {
        assert!(convert("<?php $s = 'v=' . $o->{$name};").is_none());
    }

    #[test]
    fn trailing_literal_keeps_variable_name_intact() {
        // Without the braces this would read as `$namex`.
        assert_eq!(convert("<?php $s = $name . 'x';").unwrap(), r#""{$name}x""#);
    }
}
