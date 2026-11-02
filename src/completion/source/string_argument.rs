//! String positions and sibling literals in a completion candidate's call.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use super::code_context::{CodeContext, OpenBracket};
use crate::atom::{bytes_to_str, literal_bytes_to_str};

/// The string's position within its argument, without descending into expressions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StringArgumentPosition {
    /// A string passed directly to the call.
    Direct,
    /// The key of a direct argument array.
    ArrayKey,
    /// The value paired with a key in a direct argument array.
    ArrayValue,
    /// An unkeyed element, including a key whose arrow is not typed yet.
    ArrayElement,
}

/// The argument containing the string being completed.
#[derive(Debug)]
pub(crate) struct StringArgument {
    /// The argument's position in the call.
    pub(crate) index: usize,
    /// Its explicit parameter name, when passed by name.
    pub(crate) name: Option<String>,
    /// Total number of arguments, including those after the cursor.
    pub(crate) count: usize,
    /// Whether the first argument is an array literal.
    pub(crate) first_is_array: bool,
    /// Whether the string is an argument, key, or array value.
    pub(crate) position: StringArgumentPosition,
    /// Other literal keys in this argument's array.
    pub(crate) used_keys: Vec<String>,
    /// Other literal values in this argument's array.
    pub(crate) used_values: Vec<String>,
    /// Byte offset before the closing quote, or the cursor for an unfinished string.
    pub(crate) string_end: usize,
}

struct ArgumentVisitor {
    quote: usize,
}

fn literal<'a>(expression: &'a Expression<'_>) -> Option<&'a str> {
    match expression {
        Expression::Literal(Literal::String(string)) => string.value.and_then(literal_bytes_to_str),
        _ => None,
    }
}

impl<'a> Walker<'a, 'a, Option<StringArgument>> for ArgumentVisitor {
    fn walk_in_argument_list(&self, list: &'a ArgumentList<'a>, out: &mut Option<StringArgument>) {
        // Every fragment starts with `<?php f(`. Nested calls belong to a
        // different expression and must not supply keys for this argument.
        if list.left_parenthesis.start.offset != 7 {
            return;
        }
        for (index, argument) in list.arguments.iter().enumerate() {
            if argument.is_unpacked() {
                continue;
            }
            let value = argument.value();
            let span = value.span();
            if !(span.start.offset as usize <= self.quote && self.quote < span.end.offset as usize)
            {
                continue;
            }
            let mut result = StringArgument {
                index,
                name: match argument {
                    Argument::Named(named) => Some(bytes_to_str(named.name.value).to_string()),
                    Argument::Positional(_) => None,
                },
                count: list.arguments.len(),
                first_is_array: list.arguments.first().is_some_and(|argument| {
                    crate::parser::array_literal_elements(argument.value()).is_some()
                }),
                position: StringArgumentPosition::Direct,
                used_keys: Vec::new(),
                used_values: Vec::new(),
                string_end: 0,
            };
            let mut record = |expression: &Expression<'_>, position| {
                if literal(expression).is_some()
                    && expression.span().start.offset as usize == self.quote
                {
                    result.position = position;
                    result.string_end = expression.span().end.offset as usize - 1;
                    true
                } else {
                    false
                }
            };
            if let Some(elements) = crate::parser::array_literal_elements(value) {
                for element in elements.iter() {
                    match element {
                        ArrayElement::KeyValue(entry) => {
                            if !record(entry.key, StringArgumentPosition::ArrayKey)
                                && let Some(key) = literal(entry.key)
                            {
                                result.used_keys.push(key.to_string());
                            }
                            if !record(entry.value, StringArgumentPosition::ArrayValue)
                                && let Some(value) = literal(entry.value)
                            {
                                result.used_values.push(value.to_string());
                            }
                        }
                        ArrayElement::Value(entry) => {
                            if !record(entry.value, StringArgumentPosition::ArrayElement)
                                && let Some(value) = literal(entry.value)
                            {
                                result.used_values.push(value.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                record(value, StringArgumentPosition::Direct);
            }
            if result.string_end != 0 {
                *out = Some(result);
            }
        }
    }
}

/// Inspect a recognized call's string argument using only that call's source.
///
/// A complete call preserves siblings after the cursor. While typing an
/// unfinished call, close its prefix using the already scanned bracket stack;
/// unrelated syntax errors elsewhere in the file cannot prevent completion.
pub(crate) fn string_argument(
    content: &str,
    cursor: usize,
    code: &CodeContext<'_>,
    paren: &OpenBracket,
) -> Option<StringArgument> {
    let (quote, quote_char) = code.open_string?;
    let close = super::code_context::closing_call_paren(content, paren.offset);
    let mut fragment = String::from("<?php f");
    if let Some(close) = close {
        fragment.push_str(&content[paren.offset..=close]);
    } else {
        fragment.push_str(&content[paren.offset..cursor]);
        fragment.push(quote_char);
        for bracket in code.open_brackets.iter().rev() {
            fragment.push(match bracket.byte {
                b'(' => ')',
                b'[' => ']',
                _ => '}',
            });
            if bracket.offset == paren.offset {
                break;
            }
        }
    }
    fragment.push(';');
    crate::parser::with_parsed_program(&fragment, "completion_argument", |program, _| {
        let mut result = None;
        ArgumentVisitor {
            quote: quote.checked_sub(paren.offset)? + 7,
        }
        .walk_program(program, &mut result);
        let mut result = result?;
        result.string_end = if close.is_some() {
            result.string_end.checked_sub(7)? + paren.offset
        } else {
            cursor
        };
        Some(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::source::code_context::code_context_at;

    fn argument_at(source: &str) -> Option<StringArgument> {
        let cursor = source.find('|').unwrap();
        let content = source.replacen('|', "", 1);
        let code = code_context_at(&content, cursor)?;
        let paren = code
            .open_brackets
            .iter()
            .find(|bracket| bracket.byte == b'(')?;
        string_argument(&content, cursor, &code, paren)
    }

    #[test]
    fn array_siblings_are_scoped_to_the_current_argument() {
        let arg = argument_at("f(other: ['ignored' => 1], values: array('name' => 'Ada', 'em|ail' => 'a', 'age' => 2, ...$extra, $key => 'dynamic', 'tail'))").unwrap();
        assert_eq!(arg.index, 1);
        assert_eq!(arg.name.as_deref(), Some("values"));
        assert_eq!(arg.count, 2);
        assert_eq!(arg.position, StringArgumentPosition::ArrayKey);
        assert_eq!(arg.used_keys, ["name", "age"]);
        assert_eq!(arg.used_values, ["Ada", "a", "dynamic", "tail"]);
        assert_eq!(
            arg.string_end,
            "f(other: ['ignored' => 1], values: array('name' => 'Ada', 'email".len()
        );
    }

    #[test]
    fn distinguishes_direct_arguments_array_values_and_unfinished_keys() {
        for (source, position) in [
            ("f(column: 'na|me')", StringArgumentPosition::Direct),
            (
                "f(['name' => 'va|lue'])",
                StringArgumentPosition::ArrayValue,
            ),
            ("f(['na|'])", StringArgumentPosition::ArrayElement),
            ("f(array('na|", StringArgumentPosition::ArrayElement),
        ] {
            let arg = argument_at(source).unwrap();
            assert_eq!(arg.position, position, "{source}");
        }
    }

    #[test]
    fn rejects_nested_and_computed_strings() {
        for source in [
            "f(['outer' => ['na|' => 1]])",
            "f(['name' . 'su|' => 1])",
            "f(fn () => 'na|')",
            "f(g('na|'))",
            "f(...'na|')",
            "f([...['na|' => 1]])",
            "f([function () { return 'na|",
            "f(/* 'na|') */)",
        ] {
            assert!(argument_at(source).is_none(), "{source}");
        }
    }
}
