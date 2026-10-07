//! What a Laravel string key call site looks like, read from the raw
//! buffer.
//!
//! Detection is textual rather than AST-based: it runs on a buffer that is
//! mid-edit, where the call being typed usually does not parse yet.  The
//! symbol map decides the same question for a *complete* file, and the two
//! are kept in step by hand.

use tower_lsp::lsp_types::Position;

use crate::symbol_map::LaravelStringKind;
use crate::text_position::position_to_offset;

pub(super) struct LaravelStringKeyContext<'a> {
    pub(super) kind: LaravelStringKind,
    pub(super) prefix: &'a str,
    /// Byte offset of the string content start (right after the opening quote).
    pub(super) content_start_offset: usize,
    /// When set, the key is a sub-key under this config path prefix.
    /// For example, `#[Database('mysql')]` sets this to `"database.connections."`
    /// so completion filters to `database.connections.*` keys and strips the
    /// prefix, showing just `mysql`, `sqlite`, etc.
    pub(super) config_sub_prefix: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StringArgumentShape {
    Scalar,
    ArrayValue,
}

pub(super) struct StringArgumentContext<'a> {
    pub(super) callable: &'a str,
    pub(super) named_argument: Option<&'a str>,
    pub(super) shape: StringArgumentShape,
}

#[inline]
pub(super) fn is_unescaped(bytes: &[u8], index: usize) -> bool {
    let mut before = index;
    while before > 0 && bytes[before - 1] == b'\\' {
        before -= 1;
    }
    (index - before).is_multiple_of(2)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PhpLexState {
    Code,
    SingleQuoted,
    DoubleQuoted,
    LineComment,
    BlockComment,
}

/// Find the unmatched call parenthesis enclosing a named argument.
pub(super) fn enclosing_call_open_paren(content: &str) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut quote = None;
    let mut index = bytes.len();

    while index > 0 {
        index -= 1;
        let byte = bytes[index];
        if let Some(active_quote) = quote {
            if byte == active_quote && is_unescaped(bytes, index) {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b')' => parens += 1,
            b'(' if parens > 0 => parens -= 1,
            b'(' if brackets == 0 && braces == 0 => return Some(index),
            b']' => brackets += 1,
            b'[' if brackets > 0 => brackets -= 1,
            b'}' => braces += 1,
            b'{' if braces > 0 => braces -= 1,
            b';' if parens == 0 && brackets == 0 && braces == 0 => return None,
            _ => {}
        }
    }

    None
}

/// Return the callable before a scalar first argument or a named argument.
pub(super) fn callable_before_scalar_argument(before_value: &str) -> Option<(&str, Option<&str>)> {
    let before_value = before_value.trim_end();
    if let Some(callable) = before_value.strip_suffix('(') {
        return Some((callable.trim_end(), None));
    }

    let colon = before_value.rfind(':')?;
    if !before_value[colon + 1..].trim().is_empty() {
        return None;
    }
    let before_label = before_value[..colon].trim_end();
    // Scanned by byte, the way PHP's lexer reads a label: every byte from
    // 0x80 up is a label byte, so a non-ASCII name (`prénom:`) is read
    // whole and the start always lands on a character boundary.
    let label_start = before_label
        .bytes()
        .rposition(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80))
        .map_or(0, |index| index + 1);
    if label_start == before_label.len() {
        return None;
    }
    let argument = &before_label[label_start..];
    let before_argument = before_label[..label_start].trim_end();
    let open_paren = enclosing_call_open_paren(before_argument)?;
    Some((before_argument[..open_paren].trim_end(), Some(argument)))
}

/// Return the callable owning an array that directly contains this literal.
pub(super) fn callable_before_array_argument(before_quote: &str) -> Option<(&str, Option<&str>)> {
    let bytes = before_quote.as_bytes();
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut string_quote = None;
    let mut index = bytes.len();

    while index > 0 {
        index -= 1;
        let byte = bytes[index];
        if let Some(quote) = string_quote {
            if byte == quote && is_unescaped(bytes, index) {
                string_quote = None;
            }
            continue;
        }

        match byte {
            b'\'' | b'"' => string_quote = Some(byte),
            b']' if paren_depth == 0 && brace_depth == 0 => bracket_depth += 1,
            b'[' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                return callable_before_scalar_argument(before_quote[..index].trim_end());
            }
            b'[' if paren_depth == 0 && brace_depth == 0 => bracket_depth -= 1,
            b')' => paren_depth += 1,
            b'(' if paren_depth > 0 => paren_depth -= 1,
            b'(' if bracket_depth == 0 && brace_depth == 0 => {
                let before_open = before_quote[..index].trim_end();
                let mut token_start = before_open.len();
                let token_bytes = before_open.as_bytes();
                while token_start > 0
                    && (token_bytes[token_start - 1].is_ascii_alphanumeric()
                        || token_bytes[token_start - 1] == b'_')
                {
                    token_start -= 1;
                }
                if before_open[token_start..].eq_ignore_ascii_case("array") {
                    return callable_before_scalar_argument(before_open[..token_start].trim_end());
                }
                return None;
            }
            b'}' => brace_depth += 1,
            b'{' if brace_depth > 0 => brace_depth -= 1,
            b'{' if bracket_depth == 0 && paren_depth == 0 => return None,
            b';' if bracket_depth == 0 && paren_depth == 0 && brace_depth == 0 => return None,
            _ => {}
        }
    }

    None
}

/// Resource arrays name values; an associative key is bookkeeping, not a disk.
pub(super) fn string_literal_is_array_key(content: &str, cursor: usize, quote: u8) -> bool {
    let bytes = content.as_bytes();
    let mut index = cursor;
    while index < bytes.len() {
        if bytes[index] == quote && is_unescaped(bytes, index) {
            let after_literal = skip_php_trivia_forward(content, index + 1);
            return content[after_literal..].starts_with("=>");
        }
        index += 1;
    }
    false
}

/// Skip whitespace and PHP comments without allocating or scanning beyond the
/// first real token. Comments are valid between an array key and its `=>`.
pub(super) fn skip_php_trivia_forward(content: &str, mut index: usize) -> usize {
    let bytes = content.as_bytes();
    loop {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        match bytes.get(index..index.saturating_add(2)) {
            Some(b"//") => {
                index += 2;
                while bytes.get(index).is_some_and(|byte| *byte != b'\n') {
                    index += 1;
                }
            }
            Some(b"/*") => {
                index += 2;
                while index < bytes.len() && bytes.get(index..index + 2) != Some(b"*/") {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            _ if bytes.get(index) == Some(&b'#') => {
                index += 1;
                while bytes.get(index).is_some_and(|byte| *byte != b'\n') {
                    index += 1;
                }
            }
            _ => return index,
        }
    }
}

pub(super) fn string_argument_context<'a>(
    content: &'a str,
    before_quote: &'a str,
    cursor: usize,
    quote: u8,
) -> Option<StringArgumentContext<'a>> {
    if let Some((callable, named_argument)) = callable_before_array_argument(before_quote) {
        if string_literal_is_array_key(content, cursor, quote) {
            return None;
        }
        return Some(StringArgumentContext {
            callable,
            named_argument,
            shape: StringArgumentShape::ArrayValue,
        });
    }

    let (callable, named_argument) = callable_before_scalar_argument(before_quote)?;
    Some(StringArgumentContext {
        callable,
        named_argument,
        shape: StringArgumentShape::Scalar,
    })
}

pub(super) fn imported_item_target(
    item: &str,
    group_prefix: Option<&str>,
    expected_namespace: &str,
    candidates: &[&'static str],
    referenced_name: &str,
) -> Option<Option<&'static str>> {
    let mut words = item.split_whitespace();
    let imported_name = words.next()?;
    let alias = match words.next() {
        Some(as_keyword) if as_keyword.eq_ignore_ascii_case("as") => Some(words.next()?),
        Some(_) => return None,
        None => None,
    };
    if words.next().is_some() {
        return None;
    }

    let imported_name = imported_name.trim_start_matches('\\');
    let local_name =
        alias.unwrap_or_else(|| imported_name.rsplit('\\').next().unwrap_or(imported_name));
    if !local_name.eq_ignore_ascii_case(referenced_name) {
        return None;
    }

    let target = if let Some(prefix) = group_prefix {
        prefix
            .trim_start_matches('\\')
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(expected_namespace)
            .then(|| {
                candidates
                    .iter()
                    .copied()
                    .find(|candidate| imported_name.eq_ignore_ascii_case(candidate))
            })
            .flatten()
    } else {
        let (namespace, short) = imported_name
            .rsplit_once('\\')
            .unwrap_or(("", imported_name));
        namespace
            .eq_ignore_ascii_case(expected_namespace)
            .then(|| {
                candidates
                    .iter()
                    .copied()
                    .find(|candidate| short.eq_ignore_ascii_case(candidate))
            })
            .flatten()
    };
    Some(target)
}

/// Resolve one spelling against a small, fixed set of framework classes.
///
/// `Some` identifies the matched short name. `None` covers both an unknown
/// spelling and an explicit import of an unrelated class under the same local
/// name, which is important for rejecting namespace-local facade homonyms.
#[derive(Clone, Copy)]
pub(super) struct ResolvedClassReference<'a> {
    pub(super) written: &'a str,
    pub(super) semantic: &'a str,
    pub(super) semantic_is_authoritative: bool,
}

pub(super) fn resolve_known_class_reference(
    content: &str,
    reference: ResolvedClassReference<'_>,
    expected_namespace: &str,
    candidates: &[&'static str],
    allow_root_alias: bool,
    indexed_class_exists: Option<&dyn Fn(&str) -> bool>,
) -> Option<&'static str> {
    let is_root_qualified = reference.written.starts_with('\\');
    let class_name = reference.semantic.trim_start_matches('\\');
    if class_name.contains('\\') {
        let (namespace, short) = class_name.rsplit_once('\\')?;
        return namespace
            .eq_ignore_ascii_case(expected_namespace)
            .then(|| {
                candidates
                    .iter()
                    .copied()
                    .find(|candidate| short.eq_ignore_ascii_case(candidate))
            })
            .flatten();
    }

    if reference.semantic_is_authoritative {
        if !allow_root_alias {
            return None;
        }
        let candidate = candidates
            .iter()
            .copied()
            .find(|candidate| class_name.eq_ignore_ascii_case(candidate))?;
        return (!indexed_class_exists.is_some_and(|exists| exists(candidate)))
            .then_some(candidate);
    }

    if !is_root_qualified {
        for statement in content.split(';') {
            let mut line_offset = 0usize;
            let mut clause = None;
            for line in statement.split_inclusive('\n') {
                let trimmed = line.trim_start();
                if trimmed
                    .get(..4)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("use "))
                {
                    let leading = line.len() - trimmed.len();
                    clause = Some(statement[line_offset + leading + 4..].trim());
                    break;
                }
                line_offset += line.len();
            }
            let Some(clause) = clause else {
                continue;
            };

            if let Some(open) = clause.find('{') {
                let Some(close) = clause.rfind('}') else {
                    continue;
                };
                let prefix = clause[..open].trim();
                if let Some(target) = clause[open + 1..close].split(',').find_map(|item| {
                    imported_item_target(
                        item,
                        Some(prefix),
                        expected_namespace,
                        candidates,
                        class_name,
                    )
                }) {
                    return target;
                }
            } else if let Some(target) = clause.split(',').find_map(|item| {
                imported_item_target(item, None, expected_namespace, candidates, class_name)
            }) {
                return target;
            }
        }
    }

    if !allow_root_alias
        || (!is_root_qualified && crate::text_scan::source_declares_namespace(content))
    {
        return None;
    }
    let candidate = candidates
        .iter()
        .copied()
        .find(|candidate| class_name.eq_ignore_ascii_case(candidate))?;
    (!indexed_class_exists.is_some_and(|exists| exists(candidate))).then_some(candidate)
}

pub(super) fn config_resource_static_trigger(
    content: &str,
    reference: ResolvedClassReference<'_>,
    method: &str,
    indexed_class_exists: Option<&dyn Fn(&str) -> bool>,
) -> Option<crate::symbol_map::laravel_resources::ResourceTriggerMatch> {
    if !crate::symbol_map::laravel_resources::static_method_may_trigger(method) {
        return None;
    }

    let facade = resolve_known_class_reference(
        content,
        reference,
        "Illuminate\\Support\\Facades",
        crate::symbol_map::laravel_resources::RESOURCE_FACADES,
        true,
        indexed_class_exists,
    )?;
    crate::symbol_map::laravel_resources::static_method_trigger(facade, method)
}

pub(super) fn config_resource_attribute_trigger(
    content: &str,
    reference: ResolvedClassReference<'_>,
    indexed_class_exists: Option<&dyn Fn(&str) -> bool>,
) -> Option<crate::symbol_map::laravel_resources::ResourceTriggerMatch> {
    let attribute = resolve_known_class_reference(
        content,
        reference,
        "Illuminate\\Container\\Attributes",
        crate::symbol_map::laravel_resources::RESOURCE_ATTRIBUTES,
        false,
        indexed_class_exists,
    )?;
    crate::symbol_map::laravel_resources::attribute_trigger(attribute)
}

pub(super) fn is_laravel_facade_reference(
    content: &str,
    reference: ResolvedClassReference<'_>,
    facade: &'static str,
    indexed_class_exists: Option<&dyn Fn(&str) -> bool>,
) -> bool {
    resolve_known_class_reference(
        content,
        reference,
        "Illuminate\\Support\\Facades",
        &[facade],
        true,
        indexed_class_exists,
    )
    .is_some()
}

#[inline]
pub(super) fn semantic_class_reference<'a>(
    written: &'a str,
    offset: usize,
    resolved_names: Option<&'a crate::names::OwnedResolvedNames>,
) -> ResolvedClassReference<'a> {
    match resolved_names.and_then(|names| names.get(offset as u32)) {
        Some(name) => ResolvedClassReference {
            written,
            semantic: name,
            semantic_is_authoritative: true,
        },
        None => ResolvedClassReference {
            written,
            semantic: written,
            semantic_is_authoritative: false,
        },
    }
}

/// Find the last syntactic PHP attribute opener before `end`.
/// Attribute-looking text inside strings and comments is deliberately ignored.
pub(super) fn last_attribute_open_before(content: &str, end: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let end = end.min(bytes.len());
    let mut state = PhpLexState::Code;
    let mut last = None;
    let mut index = 0usize;

    while index < end {
        let byte = bytes[index];
        match state {
            PhpLexState::Code => match byte {
                b'\'' => {
                    state = PhpLexState::SingleQuoted;
                    index += 1;
                }
                b'"' => {
                    state = PhpLexState::DoubleQuoted;
                    index += 1;
                }
                b'/' if bytes.get(index + 1) == Some(&b'/') => {
                    state = PhpLexState::LineComment;
                    index += 2;
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    state = PhpLexState::BlockComment;
                    index += 2;
                }
                b'#' if bytes.get(index + 1) == Some(&b'[') => {
                    last = Some(index);
                    index += 2;
                }
                b'#' => {
                    state = PhpLexState::LineComment;
                    index += 1;
                }
                _ => index += 1,
            },
            PhpLexState::SingleQuoted => {
                if byte == b'\'' && is_unescaped(bytes, index) {
                    state = PhpLexState::Code;
                }
                index += 1;
            }
            PhpLexState::DoubleQuoted => {
                if byte == b'"' && is_unescaped(bytes, index) {
                    state = PhpLexState::Code;
                }
                index += 1;
            }
            PhpLexState::LineComment => {
                if byte == b'\n' || byte == b'\r' {
                    state = PhpLexState::Code;
                }
                index += 1;
            }
            PhpLexState::BlockComment => {
                if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    state = PhpLexState::Code;
                    index += 2;
                } else {
                    index += 1;
                }
            }
        }
    }

    last
}

/// Start of the class name for the attribute call ending at `name_start`.
///
/// Attribute groups may contain earlier attributes and arbitrary balanced
/// argument expressions. Only a class that starts a top-level group element
/// is accepted; a lookalike call nested inside another attribute is not.
pub(super) fn attribute_class_start(before_paren: &str, name_start: usize) -> Option<usize> {
    let bytes = before_paren.as_bytes();
    let mut class_start = name_start;
    while class_start > 0
        && (bytes[class_start - 1].is_ascii_alphanumeric()
            || matches!(bytes[class_start - 1], b'_' | b'\\'))
    {
        class_start -= 1;
    }

    if !matches!(
        before_paren[..class_start].trim_end().as_bytes().last(),
        Some(b'[' | b',')
    ) {
        return None;
    }

    let open = last_attribute_open_before(before_paren, class_start)?;
    let between = &before_paren[open + 2..class_start];
    let bytes = between.as_bytes();
    let mut round = 0usize;
    let mut square = 0usize;
    let mut curly = 0usize;
    let mut quote = None;
    let mut element_start = 0usize;

    for (index, byte) in bytes.iter().copied().enumerate() {
        if let Some(active) = quote {
            if byte == active && is_unescaped(bytes, index) {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => round += 1,
            b')' if round > 0 => round -= 1,
            b'[' => square += 1,
            b']' if square > 0 => square -= 1,
            b']' => return None,
            b'{' => curly += 1,
            b'}' if curly > 0 => curly -= 1,
            b',' if round == 0 && square == 0 && curly == 0 => element_start = index + 1,
            _ => {}
        }
    }

    (quote.is_none()
        && round == 0
        && square == 0
        && curly == 0
        && between[element_start..].trim().is_empty())
    .then_some(class_start)
}

/// Find the top-level statement boundary before a fluent receiver chain.
/// Newlines are ordinary PHP whitespace, while semicolons and braces inside
/// balanced calls/closures belong to the receiver expression itself.
pub(super) fn receiver_chain_start(prefix: &str) -> usize {
    let bytes = prefix.as_bytes();
    let mut boundary = 0usize;
    let mut round = 0usize;
    let mut square = 0usize;
    let mut state = PhpLexState::Code;
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];
        match state {
            PhpLexState::Code => {
                if bytes
                    .get(index..index.saturating_add(5))
                    .is_some_and(|tag| tag.eq_ignore_ascii_case(b"<?php"))
                {
                    boundary = index + 5;
                    index += 5;
                    continue;
                }
                if bytes.get(index..index.saturating_add(3)) == Some(b"<?=") {
                    boundary = index + 3;
                    index += 3;
                    continue;
                }
                if bytes.get(index..index.saturating_add(2)) == Some(b"?>") {
                    boundary = index + 2;
                    index += 2;
                    continue;
                }
                match byte {
                    b'\'' => state = PhpLexState::SingleQuoted,
                    b'"' => state = PhpLexState::DoubleQuoted,
                    b'/' if bytes.get(index + 1) == Some(&b'/') => {
                        state = PhpLexState::LineComment;
                        index += 1;
                    }
                    b'/' if bytes.get(index + 1) == Some(&b'*') => {
                        state = PhpLexState::BlockComment;
                        index += 1;
                    }
                    b'#' if bytes.get(index + 1) != Some(&b'[') => {
                        state = PhpLexState::LineComment;
                    }
                    b'(' => round += 1,
                    b')' if round > 0 => round -= 1,
                    b'[' => square += 1,
                    b']' if square > 0 => square -= 1,
                    b';' | b'{' | b'}' if round == 0 && square == 0 => boundary = index + 1,
                    _ => {}
                }
            }
            PhpLexState::SingleQuoted => {
                if byte == b'\'' && is_unescaped(bytes, index) {
                    state = PhpLexState::Code;
                }
            }
            PhpLexState::DoubleQuoted => {
                if byte == b'"' && is_unescaped(bytes, index) {
                    state = PhpLexState::Code;
                }
            }
            PhpLexState::LineComment => {
                if byte == b'\n' || byte == b'\r' {
                    state = PhpLexState::Code;
                }
            }
            PhpLexState::BlockComment => {
                if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    state = PhpLexState::Code;
                    index += 1;
                }
            }
        }
        index += 1;
    }

    boundary
}

pub(super) fn middleware_completion_context(
    prefix: &str,
) -> Option<(LaravelStringKind, &str, usize)> {
    let colon = prefix.find(':')?;
    let alias = &prefix[..=colon];
    if alias != "auth:" {
        return None;
    }
    let resource = crate::symbol_map::laravel_resources::middleware_resource(alias)?;

    let payload = &prefix[colon + 1..];
    let raw_current = payload
        .rsplit_once(',')
        .map_or(payload, |(_, current)| current);
    let current = raw_current.trim_start();
    let start = prefix.len().saturating_sub(raw_current.len());
    Some((LaravelStringKind::ConfigResource(resource), current, start))
}

pub(super) fn is_gate_check_method(method: &str) -> bool {
    match method.len() {
        3 => method.eq_ignore_ascii_case("any") || method.eq_ignore_ascii_case("has"),
        4 => method.eq_ignore_ascii_case("none"),
        5 => method.eq_ignore_ascii_case("check"),
        6 => method.eq_ignore_ascii_case("allows") || method.eq_ignore_ascii_case("denies"),
        7 => method.eq_ignore_ascii_case("inspect"),
        _ => false,
    }
}

pub(super) fn chain_starts_at_laravel_facade(
    content: &str,
    chain: &str,
    chain_offset: usize,
    resolved_names: Option<&crate::names::OwnedResolvedNames>,
    facade: &'static str,
    indexed_class_exists: Option<&dyn Fn(&str) -> bool>,
) -> bool {
    let bytes = chain.as_bytes();
    let mut class_start = 0usize;
    while class_start < bytes.len() && bytes[class_start].is_ascii_whitespace() {
        class_start += 1;
    }
    let mut class_end = class_start;
    while class_end < bytes.len()
        && (bytes[class_end].is_ascii_alphanumeric() || matches!(bytes[class_end], b'_' | b'\\'))
    {
        class_end += 1;
    }
    if class_start == class_end {
        return false;
    }
    let mut colons = class_end;
    while colons < bytes.len() && bytes[colons].is_ascii_whitespace() {
        colons += 1;
    }
    if bytes.get(colons..colons + 2) != Some(b"::") {
        return false;
    }

    let written = &chain[class_start..class_end];
    let reference = semantic_class_reference(written, chain_offset + class_start, resolved_names);
    is_laravel_facade_reference(content, reference, facade, indexed_class_exists)
        && is_method_chain_suffix(&chain[colons + 2..])
}

/// Whether everything after a top-level `Facade::` token remains on the same
/// receiver spine. Nested arguments may contain arbitrary PHP; at top level
/// only identifiers, calls, and static/instance chain operators are valid.
pub(super) fn is_method_chain_suffix(suffix: &str) -> bool {
    let bytes = suffix.as_bytes();
    let mut round = 0usize;
    let mut square = 0usize;
    let mut curly = 0usize;
    let mut state = PhpLexState::Code;
    let mut instance_links = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        match state {
            PhpLexState::SingleQuoted => {
                if byte == b'\'' && is_unescaped(bytes, index) {
                    state = PhpLexState::Code;
                }
                index += 1;
                continue;
            }
            PhpLexState::DoubleQuoted => {
                if byte == b'"' && is_unescaped(bytes, index) {
                    state = PhpLexState::Code;
                }
                index += 1;
                continue;
            }
            PhpLexState::LineComment => {
                if byte == b'\n' || byte == b'\r' {
                    state = PhpLexState::Code;
                }
                index += 1;
                continue;
            }
            PhpLexState::BlockComment => {
                if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    state = PhpLexState::Code;
                    index += 2;
                } else {
                    index += 1;
                }
                continue;
            }
            PhpLexState::Code => {}
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            state = PhpLexState::LineComment;
            index += 2;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            state = PhpLexState::BlockComment;
            index += 2;
            continue;
        }
        if byte == b'#' && bytes.get(index + 1) != Some(&b'[') {
            state = PhpLexState::LineComment;
            index += 1;
            continue;
        }
        if round > 0 || square > 0 || curly > 0 {
            match byte {
                b'\'' => state = PhpLexState::SingleQuoted,
                b'"' => state = PhpLexState::DoubleQuoted,
                b'(' => round += 1,
                b')' if round > 0 => round -= 1,
                b'[' => square += 1,
                b']' if square > 0 => square -= 1,
                b'{' => curly += 1,
                b'}' if curly > 0 => curly -= 1,
                _ => {}
            }
            index += 1;
            continue;
        }

        match byte {
            b if b.is_ascii_alphanumeric() || matches!(b, b'_' | b' ' | b'\t' | b'\r' | b'\n') => {
                index += 1;
            }
            b'(' => {
                round = 1;
                index += 1;
            }
            b'[' => {
                square = 1;
                index += 1;
            }
            b'{' => {
                curly = 1;
                index += 1;
            }
            b':' if bytes.get(index + 1) == Some(&b':') => index += 2,
            b'-' if bytes.get(index + 1) == Some(&b'>') => {
                instance_links += 1;
                if instance_links > crate::symbol_map::laravel_resources::FACADE_CHAIN_DEPTH {
                    return false;
                }
                index += 2;
            }
            b'?' if bytes.get(index + 1) == Some(&b'-') && bytes.get(index + 2) == Some(&b'>') => {
                instance_links += 1;
                if instance_links > crate::symbol_map::laravel_resources::FACADE_CHAIN_DEPTH {
                    return false;
                }
                index += 3;
            }
            _ => return false,
        }
    }
    matches!(state, PhpLexState::Code | PhpLexState::LineComment)
        && round == 0
        && square == 0
        && curly == 0
}

/// Detect if the cursor is inside a supported string argument of a Laravel
/// helper or facade call. Returns the key kind and the prefix typed so far.
#[cfg(test)]
pub(super) fn detect_laravel_string_key_context(
    content: &str,
    position: Position,
) -> Option<LaravelStringKeyContext<'_>> {
    detect_laravel_string_key_context_inner(content, position, None, None, None)
}

pub(super) fn detect_laravel_string_key_context_inner<'a>(
    content: &'a str,
    position: Position,
    resolved_names: Option<&'a crate::names::OwnedResolvedNames>,
    indexed_function_exists: Option<&dyn Fn(&str) -> bool>,
    indexed_class_exists: Option<&dyn Fn(&str) -> bool>,
) -> Option<LaravelStringKeyContext<'a>> {
    let cursor_offset = position_to_offset(content, position) as usize;
    let bytes = content.as_bytes();

    if cursor_offset == 0 || cursor_offset > bytes.len() {
        return None;
    }

    // ── Find the opening quote before the cursor ────────────────────
    let mut quote_pos = None;
    let mut i = cursor_offset;
    while i > 0 {
        i -= 1;
        let ch = bytes[i];
        if (ch == b'\'' || ch == b'"') && is_unescaped(bytes, i) {
            quote_pos = Some(i);
            break;
        }
        if ch == b'\n' {
            return None;
        }
    }
    let quote_pos = quote_pos?;
    let mut prefix = &content[quote_pos + 1..cursor_offset];
    let mut content_start_offset = quote_pos + 1;

    // ── Locate the call argument that owns this string ─────────────
    let before_quote = content[..quote_pos].trim_end();
    let argument = string_argument_context(content, before_quote, cursor_offset, bytes[quote_pos])?;
    let before_paren = argument.callable;

    // ── Extract the function/method name ────────────────────────────
    let bp_bytes = before_paren.as_bytes();
    let name_end = bp_bytes.len();
    let mut name_start = name_end;
    while name_start > 0
        && (bp_bytes[name_start - 1].is_ascii_alphanumeric() || bp_bytes[name_start - 1] == b'_')
    {
        name_start -= 1;
    }
    if name_start == name_end {
        return None;
    }
    let func_name = &before_paren[name_start..name_end];

    // ── Check for static method syntax (Config::get, etc.) ──────────
    let before_name = &before_paren[..name_start];
    let is_static = before_name.trim_end().ends_with("::");

    // Check for instance method call (->route() or ?->route())
    let trimmed_before = before_name.trim_end();
    let is_instance_method = trimmed_before.ends_with("->") || trimmed_before.ends_with("?->");

    // Check for PHP attribute syntax: #[Config('key')], grouped attributes,
    // and fully-qualified container attributes.
    let current_attribute_class_start = (!is_static && !is_instance_method)
        .then(|| attribute_class_start(before_paren, name_start))
        .flatten();
    let is_attribute = current_attribute_class_start.is_some();

    let kind = if is_attribute {
        if argument.shape != StringArgumentShape::Scalar {
            return None;
        }
        let attr_start = current_attribute_class_start?;
        let written_class = &before_paren[attr_start..];
        let reference = semantic_class_reference(written_class, attr_start, resolved_names);
        if resolve_known_class_reference(
            content,
            reference,
            "Illuminate\\Container\\Attributes",
            &["Config"],
            false,
            indexed_class_exists,
        )
        .is_some()
        {
            argument
                .named_argument
                .is_none_or(|name| name == "key")
                .then_some(LaravelStringKind::Config)
        } else if let Some(trigger) =
            config_resource_attribute_trigger(content, reference, indexed_class_exists)
        {
            argument
                .named_argument
                .is_none_or(|name| name == trigger.argument)
                .then_some(LaravelStringKind::ConfigResource(trigger.kind))
        } else if resolve_known_class_reference(
            content,
            reference,
            "Illuminate\\Foundation\\Http\\Attributes",
            &["RedirectToRoute"],
            false,
            indexed_class_exists,
        )
        .is_some()
        {
            argument
                .named_argument
                .is_none()
                .then_some(LaravelStringKind::Route)
        } else {
            None
        }
    } else if is_static {
        let before_colons = &trimmed_before[..trimmed_before.len() - 2].trim_end();
        let bc_bytes = before_colons.as_bytes();
        let mut cls_start = bc_bytes.len();
        while cls_start > 0
            && (bc_bytes[cls_start - 1].is_ascii_alphanumeric()
                || bc_bytes[cls_start - 1] == b'_'
                || bc_bytes[cls_start - 1] == b'\\')
        {
            cls_start -= 1;
        }
        let written_class = &before_colons[cls_start..];
        let reference = semantic_class_reference(written_class, cls_start, resolved_names);
        // Preserve the pre-existing legacy facade behavior. New resource
        // triggers above resolve semantic aliases exactly; feeding an
        // unrelated `Vendor\Config as Foo` target into the legacy short-name
        // table would otherwise misclassify `Foo::get()` as Laravel Config.
        let short = written_class.rsplit('\\').next().unwrap_or(written_class);

        let fn_lower = func_name.to_ascii_lowercase();
        let short_lower = short.to_ascii_lowercase();
        let legacy_accepts_array = matches!(
            (short_lower.as_str(), fn_lower.as_str()),
            ("config", "getmany") | ("route", "is" | "currentroutenamed")
        );

        if let Some(trigger) =
            config_resource_static_trigger(content, reference, func_name, indexed_class_exists)
        {
            if argument
                .named_argument
                .is_some_and(|name| name != trigger.argument)
                || (argument.shape == StringArgumentShape::ArrayValue
                    && !trigger.shape.accepts_array())
                || (argument.shape == StringArgumentShape::Scalar
                    && !trigger.shape.accepts_scalar())
            {
                return None;
            }
            Some(LaravelStringKind::ConfigResource(trigger.kind))
        } else if func_name.eq_ignore_ascii_case("middleware")
            && argument
                .named_argument
                .is_none_or(|name| name == "middleware")
            && is_laravel_facade_reference(content, reference, "Route", indexed_class_exists)
        {
            let (middleware_kind, middleware_prefix, relative_start) =
                middleware_completion_context(prefix)?;
            prefix = middleware_prefix;
            content_start_offset += relative_start;
            Some(middleware_kind)
        } else if argument.named_argument.is_some()
            || (argument.shape != StringArgumentShape::Scalar
                && (!legacy_accepts_array || !before_quote.trim_end().ends_with('[')))
        {
            None
        } else {
            match (short_lower.as_str(), fn_lower.as_str()) {
                (
                    "config",
                    "get" | "getmany" | "set" | "has" | "boolean" | "array" | "collection"
                    | "prepend" | "push",
                ) => Some(LaravelStringKind::Config),
                ("view", "make" | "exists") => Some(LaravelStringKind::View),
                ("lang", "get" | "has" | "hasforlocale" | "choice") => {
                    Some(LaravelStringKind::Trans)
                }
                // Route names reached through the URL-building facades, and the
                // "is the current route named …?" predicates.
                (
                    "url" | "redirect" | "response",
                    "route" | "signedroute" | "temporarysignedroute" | "redirecttoroute",
                ) => Some(LaravelStringKind::Route),
                ("route", "is" | "currentroutenamed") => Some(LaravelStringKind::Route),
                ("env", "get" | "getorfail") => Some(LaravelStringKind::Env),
                // Artisan command names.
                ("artisan", "call" | "queue") => Some(LaravelStringKind::Command),
                ("schedule", "command") => Some(LaravelStringKind::Command),
                // Eloquent morph aliases.
                ("relation", "getmorphedmodel") => Some(LaravelStringKind::MorphAlias),
                ("model", "getactualclassnameformorph") => Some(LaravelStringKind::MorphAlias),
                // Authorization abilities checked through the Gate facade.
                (
                    "gate",
                    "allows" | "denies" | "check" | "any" | "none" | "authorize" | "inspect"
                    | "has" | "define",
                ) => Some(LaravelStringKind::GateAbility),
                _ => None,
            }
        }
    } else if is_instance_method {
        let receiver = trimmed_before
            .trim_end_matches("?->")
            .trim_end_matches("->")
            .trim_end();
        // Whether the receiver is `$this` (used to scope command-running
        // methods, whose names are too generic to match on any object).
        let receiver_is_this = receiver.ends_with("$this");
        // Whether the receiver plainly reads as the authenticated user,
        // which is what makes `->can('…')` an authorization check rather
        // than a same-named method on an unrelated object.  Mirrors the
        // symbol-map rule that decides which `can()` calls get a span.
        let receiver_is_user_like = {
            let tail = receiver
                .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("");
            tail.get(tail.len().saturating_sub(4)..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case("user"))
                || receiver
                    .get(receiver.len().saturating_sub(6)..)
                    .is_some_and(|suffix| suffix.eq_ignore_ascii_case("user()"))
        };
        // A chain that starts at the `Gate` facade
        // (`Gate::forUser($user)->allows('…')`) or at a route registration
        // (`Route::get(…)->can('…')`) is an authorization check whatever the
        // rest of the chain looks like.  Only the text back to the start of
        // the statement is searched — `trimmed_before` is the whole file
        // prefix, and an unrelated `Gate::` far above would false-positive.
        let is_middleware = func_name.eq_ignore_ascii_case("middleware");
        let is_can = func_name.eq_ignore_ascii_case("can")
            || func_name.eq_ignore_ascii_case("cannot")
            || func_name.eq_ignore_ascii_case("canAny");
        let needs_route_root = is_middleware || is_can;
        let needs_gate_root = is_can
            || func_name.eq_ignore_ascii_case("authorize")
            || is_gate_check_method(func_name);
        let (chain_start, chain_text) = if needs_route_root || needs_gate_root {
            let start = receiver_chain_start(trimmed_before);
            (start, &trimmed_before[start..])
        } else {
            (0, "")
        };
        let chain_starts_at_gate = needs_gate_root
            && chain_starts_at_laravel_facade(
                content,
                chain_text,
                chain_start,
                resolved_names,
                "Gate",
                indexed_class_exists,
            );
        let chain_starts_at_route = needs_route_root
            && chain_starts_at_laravel_facade(
                content,
                chain_text,
                chain_start,
                resolved_names,
                "Route",
                indexed_class_exists,
            );

        if is_middleware
            && chain_starts_at_route
            && argument
                .named_argument
                .is_none_or(|name| name == "middleware")
        {
            let (middleware_kind, middleware_prefix, relative_start) =
                middleware_completion_context(prefix)?;
            prefix = middleware_prefix;
            content_start_offset += relative_start;
            Some(middleware_kind)
        } else {
            if argument.named_argument.is_some() || argument.shape != StringArgumentShape::Scalar {
                return None;
            }
            if matches!(
                func_name.to_ascii_lowercase().as_str(),
                "route" | "signedroute" | "temporarysignedroute" | "redirecttoroute" | "routeis"
            ) {
                Some(LaravelStringKind::Route)
                // `$this->call('cmd')` / `$this->callSilently('cmd')` inside a
                // console command run another Artisan command.  Restricted to a
                // `$this` receiver because `->call()` is a common method name.
            } else if receiver_is_this
                && (func_name.eq_ignore_ascii_case("call")
                    || func_name.eq_ignore_ascii_case("callSilently"))
            {
                Some(LaravelStringKind::Command)
                // `$this->authorize('update', $post)` in a controller.
            } else if (func_name.eq_ignore_ascii_case("authorize")
                && (receiver_is_this || chain_starts_at_gate))
                // `$user->can('update', $post)`.
                || (is_can
                    && (receiver_is_user_like || chain_starts_at_route || chain_starts_at_gate))
                || (is_gate_check_method(func_name) && chain_starts_at_gate)
            {
                Some(LaravelStringKind::GateAbility)
            } else {
                None
            }
        }
    } else {
        let fn_lower = func_name.to_ascii_lowercase();
        // The Blade preprocessor lowers `@includeFirst`/`@componentFirst`/
        // `@extendsFirst` and `@canany` to markers that name their candidates
        // inside an array literal rather than as a plain first argument.
        let accepts_array = matches!(
            fn_lower.as_str(),
            "blade_view_directive" | "blade_can_directive"
        );
        if argument.shape != StringArgumentShape::Scalar
            && (!accepts_array || !before_quote.trim_end().ends_with('['))
        {
            return None;
        }
        let mut callable_start = name_start;
        while callable_start > 0
            && (bp_bytes[callable_start - 1].is_ascii_alphanumeric()
                || matches!(bp_bytes[callable_start - 1], b'_' | b'\\'))
        {
            callable_start -= 1;
        }
        let written_function = &before_paren[callable_start..name_end];
        if let Some(trigger) = crate::symbol_map::laravel_resources::auth_helper_trigger(
            content,
            written_function,
            callable_start as u32,
            resolved_names,
            indexed_function_exists,
        ) {
            argument
                .named_argument
                .is_none_or(|name| name == trigger.argument)
                .then_some(LaravelStringKind::ConfigResource(trigger.kind))
        } else if argument.named_argument.is_some() {
            None
        } else {
            match fn_lower.as_str() {
                "route" | "to_route" => Some(LaravelStringKind::Route),
                "config" => Some(LaravelStringKind::Config),
                "view" | "blade_view_directive" | "blade_each_directive" => {
                    Some(LaravelStringKind::View)
                }
                "__" | "trans" | "trans_choice" => Some(LaravelStringKind::Trans),
                "env" => Some(LaravelStringKind::Env),
                // The Blade preprocessor lowers `@can`/`@cannot`/`@canany` to
                // this call, so completion inside the directive works too.
                "blade_can_directive" => Some(LaravelStringKind::GateAbility),
                _ => None,
            }
        }
    };

    let kind = kind?;
    let config_sub_prefix = match &kind {
        LaravelStringKind::ConfigResource(resource) => {
            Some(crate::symbol_map::laravel_resources::descriptor(*resource).config_prefix)
        }
        _ => None,
    };

    Some(LaravelStringKeyContext {
        kind,
        prefix,
        content_start_offset,
        config_sub_prefix,
    })
}
