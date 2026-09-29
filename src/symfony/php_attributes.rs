//! Small source scanner for PHP method attributes.

use crate::text_scan::find_matching_forward;

#[derive(Clone, Copy)]
pub(super) struct AttributeCall {
    pub name_start: usize,
    pub name_end: usize,
    pub args: Option<(usize, usize)>,
    pub group_end: usize,
}

#[derive(Clone, Copy)]
pub(super) struct PhpArgument<'a> {
    pub name: Option<&'a str>,
    pub value_start: usize,
    pub value_end: usize,
}

pub(super) fn attribute_calls(content: &str) -> Vec<AttributeCall> {
    let mut calls = Vec::new();
    let mut search = 0usize;
    while let Some(relative) = content[search..].find("#[") {
        let bracket = search + relative + 1;
        let Some(group_close) = find_matching_forward(content, bracket, b'[', b']') else {
            break;
        };
        for (start, end) in split_top_level(content, bracket + 1, group_close) {
            let Some((segment_start, segment_end)) = trim_range(content, start, end) else {
                continue;
            };
            let mut name_end = segment_start;
            while content
                .as_bytes()
                .get(name_end)
                .is_some_and(|byte| is_php_name(*byte))
            {
                name_end += 1;
            }
            if name_end == segment_start {
                continue;
            }
            let mut cursor = name_end;
            skip_whitespace(content.as_bytes(), &mut cursor);
            let args = if cursor < segment_end && content.as_bytes()[cursor] == b'(' {
                find_matching_forward(content, cursor, b'(', b')')
                    .filter(|close| *close < segment_end)
                    .map(|close| (cursor + 1, close))
            } else {
                None
            };
            calls.push(AttributeCall {
                name_start: segment_start,
                name_end,
                args,
                group_end: group_close + 1,
            });
        }
        search = group_close + 1;
    }
    calls
}

pub(super) fn method_after_attribute(content: &str, group_end: usize) -> Option<(usize, usize)> {
    let bytes = content.as_bytes();
    let limit = (group_end + 8192).min(content.len());
    let relative = content[group_end..limit].find("function")?;
    let function = group_end + relative;
    if bytes
        .get(function.wrapping_sub(1))
        .is_some_and(|byte| is_php_identifier(*byte))
        || bytes
            .get(function + "function".len())
            .is_some_and(|byte| is_php_identifier(*byte))
    {
        return None;
    }
    let mut start = function + "function".len();
    skip_whitespace(bytes, &mut start);
    if bytes.get(start) == Some(&b'&') {
        start += 1;
        skip_whitespace(bytes, &mut start);
    }
    let mut end = start;
    while bytes.get(end).is_some_and(|byte| is_php_identifier(*byte)) {
        end += 1;
    }
    (end > start).then_some((start, end))
}

pub(super) fn php_arguments(content: &str, start: usize, end: usize) -> Vec<PhpArgument<'_>> {
    split_top_level(content, start, end)
        .into_iter()
        .filter_map(|(start, end)| {
            let (start, end) = trim_range(content, start, end)?;
            if let Some(colon) = top_level_colon(content, start, end)
                && let Some((name_start, name_end)) = trim_range(content, start, colon)
                && content[name_start..name_end]
                    .bytes()
                    .enumerate()
                    .all(|(index, byte)| {
                        if index == 0 {
                            byte == b'_' || byte.is_ascii_alphabetic()
                        } else {
                            is_php_identifier(byte)
                        }
                    })
            {
                let (value_start, value_end) = trim_range(content, colon + 1, end)?;
                return Some(PhpArgument {
                    name: Some(&content[name_start..name_end]),
                    value_start,
                    value_end,
                });
            }
            Some(PhpArgument {
                name: None,
                value_start: start,
                value_end: end,
            })
        })
        .collect()
}

pub(super) fn configured_argument<'a>(
    arguments: &'a [PhpArgument<'a>],
    name: Option<&str>,
    position: Option<usize>,
) -> Option<PhpArgument<'a>> {
    name.and_then(|name| {
        arguments
            .iter()
            .copied()
            .find(|argument| argument.name == Some(name))
    })
    .or_else(|| {
        position.and_then(|position| {
            arguments
                .iter()
                .filter(|argument| argument.name.is_none())
                .nth(position)
                .copied()
        })
    })
}

pub(super) fn argument_value<'a>(content: &'a str, argument: PhpArgument<'_>) -> &'a str {
    &content[argument.value_start..argument.value_end]
}

pub(super) fn string_argument(
    content: &str,
    argument: PhpArgument<'_>,
) -> Option<(String, usize, usize)> {
    let raw = argument_value(content, argument);
    let value = crate::text_scan::decode_php_string_literal(raw)?.into_owned();
    Some((
        value,
        argument.value_start + 1,
        argument.value_end.saturating_sub(1),
    ))
}

pub(super) fn split_top_level(content: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut ranges = Vec::new();
    let mut segment_start = start;
    let mut cursor = start;
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;
    let mut brace_depth = 0u32;
    while cursor < end {
        match bytes[cursor] {
            b'\'' | b'"' => {
                cursor = crate::text_scan::skip_string_forward(bytes, cursor).min(end);
                continue;
            }
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                ranges.push((segment_start, cursor));
                segment_start = cursor + 1;
            }
            _ => {}
        }
        cursor += 1;
    }
    ranges.push((segment_start, end));
    ranges
}

fn top_level_colon(content: &str, start: usize, end: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut cursor = start;
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;
    let mut brace_depth = 0u32;
    while cursor < end {
        match bytes[cursor] {
            b'\'' | b'"' => {
                cursor = crate::text_scan::skip_string_forward(bytes, cursor).min(end);
                continue;
            }
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b':' if paren_depth == 0
                && bracket_depth == 0
                && brace_depth == 0
                && bytes.get(cursor.wrapping_sub(1)) != Some(&b':')
                && bytes.get(cursor + 1) != Some(&b':') =>
            {
                return Some(cursor);
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn trim_range(content: &str, mut start: usize, mut end: usize) -> Option<(usize, usize)> {
    let bytes = content.as_bytes();
    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    (start < end).then_some((start, end))
}

pub(super) fn skip_whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes
        .get(*cursor)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        *cursor += 1;
    }
}

pub(super) fn is_php_identifier(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric() || byte >= 0x80
}

pub(super) fn is_php_name(byte: u8) -> bool {
    is_php_identifier(byte) || byte == b'\\'
}
