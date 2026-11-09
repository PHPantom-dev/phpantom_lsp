//! The HTML and PHP boundaries of a Tempest view. Ranges always refer to
//! the original buffer, including while a tag or expression is incomplete.

use std::ops::Range;

use crate::text_scan::{find, skip_php_comment, skip_string_forward};

#[derive(Debug)]
pub(super) struct Attribute<'a> {
    pub(super) name: &'a str,
    pub(super) span: Range<usize>,
    pub(super) value: Option<Range<usize>>,
}

#[derive(Debug)]
pub(super) struct Tag<'a> {
    pub(super) name: &'a str,
    pub(super) name_span: Range<usize>,
    pub(super) span: Range<usize>,
    pub(super) closing: bool,
    pub(super) self_closing: bool,
    pub(super) attributes: Vec<Attribute<'a>>,
}

#[derive(Debug)]
pub(super) enum Token<'a> {
    Tag(Tag<'a>),
    Php { code: Range<usize>, echo: bool },
}

/// Read the constructs that carry PHP or component names. Plain HTML,
/// escaped attributes and comments do not become PHP symbols.
pub(super) fn tokens(source: &str) -> Vec<Token<'_>> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        let comment = if rest.starts_with(b"{{--") {
            Some((4, b"--}}".as_slice()))
        } else if rest.starts_with(b"<!--") {
            Some((4, b"-->".as_slice()))
        } else if rest.starts_with(b"<?xml") {
            Some((5, b"?>".as_slice()))
        } else {
            None
        };
        if let Some((len, close)) = comment {
            i = find(bytes, i + len, bytes.len(), close).map_or(bytes.len(), |at| at + close.len());
            continue;
        }
        if let Some((token, end)) = php_at(source, i) {
            out.push(token);
            i = end;
            continue;
        }
        if rest.starts_with(b"<") {
            let closing = bytes.get(i + 1) == Some(&b'/');
            let start = i + 1 + usize::from(closing);
            if bytes.get(start).is_some_and(u8::is_ascii_alphabetic) {
                let tag = tag_at(source, i, start, closing);
                i = tag.span.end;
                out.push(Token::Tag(tag));
                continue;
            }
        }
        i += 1;
    }
    out
}

fn php_at(source: &str, start: usize) -> Option<(Token<'_>, usize)> {
    let bytes = source.as_bytes();
    let rest = &bytes[start..];
    let (open, close, echo) = if rest.starts_with(b"<?php") {
        (5, b"?>".as_slice(), false)
    } else if rest.starts_with(b"<?=") {
        (3, b"?>".as_slice(), true)
    } else if rest.starts_with(b"{{") && !rest.starts_with(b"{{--") {
        (2, b"}}".as_slice(), true)
    } else if rest.starts_with(b"{!!") {
        (3, b"!!}".as_slice(), true)
    } else {
        return None;
    };
    let code_start = start + open;
    let end = php_end(bytes, code_start, close);
    let span_end = if bytes[end..].starts_with(close) {
        end + close.len()
    } else {
        end
    };
    Some((
        Token::Php {
            code: code_start..end,
            echo,
        },
        span_end,
    ))
}

fn php_end(bytes: &[u8], mut i: usize, close: &[u8]) -> usize {
    while i < bytes.len() {
        if bytes[i..].starts_with(close) {
            break;
        }
        if matches!(bytes[i], b'\'' | b'"' | b'`') {
            i = skip_string_forward(bytes, i).min(bytes.len());
        } else if bytes[i..].starts_with(b"<<<") {
            i = crate::parser::skip_heredoc(bytes, i);
        } else if let Some(end) = skip_php_comment(bytes, i) {
            // A PHP closing tag also terminates a line comment.
            i = if close == b"?>" && !bytes[i..].starts_with(b"/*") {
                find(bytes, i, end, close).unwrap_or(end)
            } else {
                end
            };
        } else {
            i += 1;
        }
    }
    i
}

pub(super) fn last_php_token(bytes: &[u8]) -> Option<u8> {
    let mut last = None;
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'\'' | b'"' | b'`') {
            last = Some(bytes[i]);
            i = skip_string_forward(bytes, i);
        } else if bytes[i..].starts_with(b"<<<") {
            last = Some(b'\'');
            i = crate::parser::skip_heredoc(bytes, i);
        } else if let Some(end) = skip_php_comment(bytes, i) {
            i = end;
        } else {
            if !bytes[i].is_ascii_whitespace() {
                last = Some(bytes[i]);
            }
            i += 1;
        }
    }
    last
}

fn tag_at(source: &str, start: usize, name_start: usize, closing: bool) -> Tag<'_> {
    let bytes = source.as_bytes();
    let mut i = name_start;
    while bytes.get(i).is_some_and(|b| is_name(*b)) {
        i += 1;
    }
    let name_span = name_start..i;
    let name = &source[name_span.clone()];
    let mut attributes = Vec::new();
    let mut self_closing = is_void(name);
    while i < bytes.len() {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'>') {
            i += 1;
            break;
        }
        if bytes.get(i..i + 2) == Some(b"/>") {
            i += 2;
            self_closing = true;
            break;
        }
        if bytes.get(i..i + 2) == Some(b"<?")
            && let Some((_, end)) = php_at(source, i)
        {
            attributes.push(Attribute {
                name: "",
                span: i..end,
                value: Some(i..end),
            });
            i = end;
            continue;
        }
        if i == bytes.len() || bytes[i] == b'<' {
            break;
        }
        let attr_start = i;
        while bytes
            .get(i)
            .is_some_and(|b| !b.is_ascii_whitespace() && !matches!(b, b'=' | b'>' | b'/' | b'<'))
        {
            i += 1;
        }
        if i == attr_start {
            i += 1;
            continue;
        }
        let attr_name = &source[attr_start..i];
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let mut value = None;
        if bytes.get(i) == Some(&b'=') {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            if let Some(&quote @ (b'\'' | b'"')) = bytes.get(i) {
                i += 1;
                let value_start = i;
                // HTML quotes are not escaped with backslashes. PHP in
                // a normal attribute, however, owns its own quotes.
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i..].starts_with(b"<?")
                        && let Some((_, end)) = php_at(source, i)
                    {
                        i = end;
                    } else {
                        i += 1;
                    }
                }
                value = Some(value_start..i);
                if i < bytes.len() {
                    i += 1;
                }
            } else {
                let value_start = i;
                while bytes
                    .get(i)
                    .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'>')
                {
                    i += 1;
                }
                value = Some(value_start..i);
            }
        }
        attributes.push(Attribute {
            name: attr_name,
            span: attr_start..i,
            value,
        });
    }
    Tag {
        name,
        name_span,
        span: start..i,
        closing,
        self_closing,
        attributes,
    }
}

fn is_name(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':')
}

fn is_void(name: &str) -> bool {
    [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ]
    .iter()
    .any(|void| name.eq_ignore_ascii_case(void))
}
