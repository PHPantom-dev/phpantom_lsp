//! What a Laravel string key call site looks like, read from the raw
//! buffer.
//!
//! Detection is textual rather than AST-based: it runs on a buffer that is
//! mid-edit, where the call being typed usually does not parse yet.  The
//! symbol map decides the same question for a *complete* file, and the two
//! are kept in step by hand.

use tower_lsp::lsp_types::Position;

use std::cell::OnceCell;

use crate::completion::source::code_context::{CodeContext, Operator};
use crate::symbol_map::LaravelStringKind;
use crate::symbol_map::extraction::laravel::{
    MORPH_TYPES_ARG, MethodReceiver, argument_takes_name_list, facade_static_call_arg_kind,
    helper_function_accepts_name_list, helper_function_first_arg_kind, instance_method_arg_kind,
    is_morph_map_registration_method, is_morph_types_query_method,
};
use crate::text_position::position_to_offset;
use crate::text_scan::{CodeOffsets, is_escaped, scan_ident_backward, skip_php_trivia};

pub(super) struct LaravelStringKeyContext<'a> {
    pub(super) kind: LaravelStringKind,
    pub(super) prefix: &'a str,
    /// Byte offset of the string content start (right after the opening quote).
    pub(super) content_start_offset: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StringArgumentShape {
    Scalar,
    /// An unkeyed entry can still become a key while the user types.
    ArrayElement,
    ArrayKey,
    ArrayValue,
}

/// Where a string literal sits among the arguments of the call it is
/// passed to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ArgumentSlot<'a> {
    /// The positional argument at this index.
    Positional(usize),
    /// The argument passed by this name.
    Named(&'a str),
}

impl ArgumentSlot<'_> {
    /// Whether this is the first positional argument, or the one named
    /// `name`.
    pub(super) fn is_first_or_named(self, name: &str) -> bool {
        self.matches_parameter(0, name)
    }

    /// Match PHP's positional or named spelling of a parameter.
    fn matches_parameter(self, index: usize, name: &str) -> bool {
        match self {
            Self::Positional(position) => position == index,
            Self::Named(named) => named == name,
        }
    }
}

pub(super) struct StringArgumentContext<'a> {
    pub(super) callable: &'a str,
    /// Method-call operator and the end of its receiver, ignoring comments.
    pub(super) operator: Option<Operator>,
    pub(super) slot: ArgumentSlot<'a>,
    pub(super) shape: StringArgumentShape,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PhpLexState {
    Code,
    SingleQuoted,
    DoubleQuoted,
    LineComment,
    BlockComment,
}

/// Whether the literal is followed by an associative-array arrow.
pub(super) fn string_literal_is_array_key(content: &str, cursor: usize, quote: u8) -> bool {
    let bytes = content.as_bytes();
    let mut index = cursor;
    while index < bytes.len() {
        if bytes[index] == quote && !is_escaped(bytes, index) {
            let after_literal = skip_php_trivia(bytes, index + 1);
            return content[after_literal..].starts_with("=>");
        }
        index += 1;
    }
    false
}

impl StringArgumentShape {
    fn is_array_value(self) -> bool {
        matches!(self, Self::ArrayElement | Self::ArrayValue)
    }

    fn is_array_key(self) -> bool {
        matches!(self, Self::ArrayElement | Self::ArrayKey)
    }
}

/// Read a literal's immediate call argument from the shared lexical scan.
/// Nested arrays and expressions are deliberately not treated as direct keys.
pub(super) fn string_argument_context<'a>(
    content: &'a str,
    code: &CodeContext<'_>,
    cursor: usize,
) -> Option<StringArgumentContext<'a>> {
    let (quote_pos, quote) = code.open_string?;
    let inner = code.open_brackets.last()?;
    let before_inner = &content[..inner.code_before];
    let token_start = crate::text_scan::scan_ident_backward(content.as_bytes(), inner.code_before);
    let is_array = inner.byte == b'['
        || (inner.byte == b'('
            && inner.callee_operator.is_none()
            && before_inner[token_start..].eq_ignore_ascii_case("array"));
    let (call, value_start, shape) = if is_array {
        let call = code.open_brackets.iter().rev().nth(1)?;
        let element_start = skip_php_trivia(content.as_bytes(), inner.item_start);
        let shape = if element_start == quote_pos {
            if string_literal_is_array_key(content, cursor, quote as u8) {
                StringArgumentShape::ArrayKey
            } else {
                StringArgumentShape::ArrayElement
            }
        } else if code.code_before.ends_with("=>") && !inner.item_has_arrow_function {
            StringArgumentShape::ArrayValue
        } else {
            return None;
        };
        let value_start = if inner.byte == b'[' {
            inner.offset
        } else {
            token_start
        };
        (call, value_start, shape)
    } else {
        (inner, quote_pos, StringArgumentShape::Scalar)
    };
    if call.byte != b'(' {
        return None;
    }

    let argument_start = skip_php_trivia(content.as_bytes(), call.item_start);
    let named_argument = if argument_start == value_start {
        None
    } else {
        let bytes = content.as_bytes();
        let mut name_end = argument_start;
        while name_end < value_start
            && (bytes[name_end].is_ascii_alphanumeric()
                || bytes[name_end] == b'_'
                || bytes[name_end] >= 0x80)
        {
            name_end += 1;
        }
        let colon = skip_php_trivia(content.as_bytes(), name_end);
        if name_end == argument_start
            || bytes.get(colon) != Some(&b':')
            || skip_php_trivia(content.as_bytes(), colon + 1) != value_start
        {
            return None;
        }
        Some(&content[argument_start..name_end])
    };
    Some(StringArgumentContext {
        callable: &content[..call.code_before],
        operator: call.callee_operator,
        slot: named_argument.map_or(ArgumentSlot::Positional(call.commas), ArgumentSlot::Named),
        shape,
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
                let Some(members) = clause.get(open + 1..close) else {
                    continue;
                };
                if let Some(target) = members.split(',').find_map(|item| {
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
    let bytes = &content.as_bytes()[..end.min(content.len())];
    CodeOffsets::new(bytes)
        .filter(|&index| bytes[index..].starts_with(b"#["))
        .last()
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
            if byte == active && !is_escaped(bytes, index) {
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
    let mut offsets = CodeOffsets::new(bytes);

    while let Some(index) = offsets.next() {
        let rest = &bytes[index..];
        let open_or_close_tag = if rest
            .get(..5)
            .is_some_and(|tag| tag.eq_ignore_ascii_case(b"<?php"))
        {
            5
        } else if rest.starts_with(b"<?=") {
            3
        } else if rest.starts_with(b"?>") {
            2
        } else {
            0
        };
        if open_or_close_tag > 0 {
            boundary = index + open_or_close_tag;
            offsets.skip_to(boundary);
            continue;
        }
        match bytes[index] {
            b'(' => round += 1,
            b')' if round > 0 => round -= 1,
            b'[' => square += 1,
            b']' if square > 0 => square -= 1,
            b';' | b'{' | b'}' if round == 0 && square == 0 => boundary = index + 1,
            _ => {}
        }
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
                if byte == b'\'' && !is_escaped(bytes, index) {
                    state = PhpLexState::Code;
                }
                index += 1;
                continue;
            }
            PhpLexState::DoubleQuoted => {
                if byte == b'"' && !is_escaped(bytes, index) {
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
    let code = crate::completion::source::code_context::code_context_at(
        content,
        position_to_offset(content, position) as usize,
    )?;
    detect_laravel_string_key_context_inner(content, position, &code, None, None, None)
}

pub(super) fn detect_laravel_string_key_context_inner<'a>(
    content: &'a str,
    position: Position,
    code: &CodeContext<'_>,
    resolved_names: Option<&'a crate::names::OwnedResolvedNames>,
    indexed_function_exists: Option<&dyn Fn(&str) -> bool>,
    indexed_class_exists: Option<&dyn Fn(&str) -> bool>,
) -> Option<LaravelStringKeyContext<'a>> {
    let cursor_offset = position_to_offset(content, position) as usize;
    if cursor_offset == 0 || cursor_offset > content.len() {
        return None;
    }

    let (quote_pos, _) = code.open_string?;
    let literal = &content[quote_pos + 1..cursor_offset];
    let literal_start = quote_pos + 1;
    let argument = string_argument_context(content, code, cursor_offset)?;
    let before_paren = argument.callable;

    // ── Extract the function/method name ────────────────────────────
    let bp_bytes = before_paren.as_bytes();
    let mut name_start = bp_bytes.len();
    while name_start > 0
        && (bp_bytes[name_start - 1].is_ascii_alphanumeric() || bp_bytes[name_start - 1] == b'_')
    {
        name_start -= 1;
    }
    if name_start == bp_bytes.len() {
        return None;
    }

    let call = StringCall {
        content,
        before_paren,
        name_start,
        name: &before_paren[name_start..],
        literal,
        argument,
        resolved_names,
        indexed_function_exists,
        indexed_class_exists,
    };
    let before_name = before_paren[..name_start].trim_end();
    let key = if call.argument.operator.is_some() && is_morph_types_query_method(call.name) {
        (call
            .argument
            .slot
            .matches_parameter(MORPH_TYPES_ARG, "types")
            && (call.argument.shape == StringArgumentShape::Scalar
                || call.argument.shape.is_array_value()))
        .then_some(StringKey::Whole(LaravelStringKind::MorphAlias))?
    } else if let Some(operator) = call.argument.operator {
        let receiver = &content[..operator.code_before];
        if operator.is_static {
            static_call_key(&call, receiver)?
        } else {
            instance_call_key(&call, receiver, before_name)?
        }
    } else if let Some(attr_start) = attribute_class_start(before_paren, name_start) {
        StringKey::Whole(attribute_key(&call, attr_start)?)
    } else {
        StringKey::Whole(function_call_key(&call)?)
    };

    let (kind, prefix, content_start_offset) = match key {
        StringKey::Whole(kind) => (kind, literal, literal_start),
        StringKey::Middleware {
            kind,
            prefix,
            start,
        } => (kind, prefix, literal_start + start),
    };
    Some(LaravelStringKeyContext {
        kind,
        prefix,
        content_start_offset,
    })
}

/// The call a string argument is passed to, as the text before the string
/// spells it.
struct StringCall<'a, 'f> {
    content: &'a str,
    /// The text before the call's `(`.
    before_paren: &'a str,
    /// Where the function or method name starts in `before_paren`.
    name_start: usize,
    /// The function or method name.
    name: &'a str,
    /// The literal typed so far, up to the cursor.
    literal: &'a str,
    argument: StringArgumentContext<'a>,
    resolved_names: Option<&'a crate::names::OwnedResolvedNames>,
    indexed_function_exists: Option<&'f dyn Fn(&str) -> bool>,
    indexed_class_exists: Option<&'f dyn Fn(&str) -> bool>,
}

impl StringCall<'_, '_> {
    /// Whether the string can name a `kind` in the shape it was written in:
    /// as a scalar, or as a bare entry of a list the call takes (a value
    /// under `'key' =>` is not one).
    fn shape_fits(&self, kind: &LaravelStringKind) -> bool {
        match self.argument.shape {
            StringArgumentShape::Scalar => true,
            StringArgumentShape::ArrayElement => argument_takes_name_list(kind, self.name),
            StringArgumentShape::ArrayKey | StringArgumentShape::ArrayValue => false,
        }
    }
}

/// What a string argument names, and which part of the literal is the key
/// being typed.
enum StringKey<'a> {
    /// The whole literal typed so far is the key.
    Whole(LaravelStringKind),
    /// The key is the part of a middleware string after its `alias:`.
    Middleware {
        kind: LaravelStringKind,
        prefix: &'a str,
        start: usize,
    },
}

/// The middleware key a `->middleware('auth:…')` string is typing.
fn middleware_key(literal: &str) -> Option<StringKey<'_>> {
    let (kind, prefix, start) = middleware_completion_context(literal)?;
    Some(StringKey::Middleware {
        kind,
        prefix,
        start,
    })
}

/// A string inside a PHP attribute's arguments: `#[Config('app.name')]`,
/// grouped attributes, and fully-qualified container attributes.
fn attribute_key(call: &StringCall<'_, '_>, attr_start: usize) -> Option<LaravelStringKind> {
    if call.argument.shape != StringArgumentShape::Scalar {
        return None;
    }
    let written_class = &call.before_paren[attr_start..];
    let reference = semantic_class_reference(written_class, attr_start, call.resolved_names);
    let slot = call.argument.slot;
    if resolve_known_class_reference(
        call.content,
        reference,
        "Illuminate\\Container\\Attributes",
        &["Config"],
        false,
        call.indexed_class_exists,
    )
    .is_some()
    {
        slot.is_first_or_named("key")
            .then_some(LaravelStringKind::Config)
    } else if let Some(trigger) =
        config_resource_attribute_trigger(call.content, reference, call.indexed_class_exists)
    {
        slot.is_first_or_named(trigger.argument)
            .then_some(LaravelStringKind::ConfigResource(trigger.kind))
    } else if resolve_known_class_reference(
        call.content,
        reference,
        "Illuminate\\Foundation\\Http\\Attributes",
        &["RedirectToRoute"],
        false,
        call.indexed_class_exists,
    )
    .is_some()
    {
        (slot == ArgumentSlot::Positional(0)).then_some(LaravelStringKind::Route)
    } else {
        None
    }
}

/// A string passed to a static call: `Config::get('…')`,
/// `Storage::disk('…')`, `Route::view('/about', '…')`.
fn static_call_key<'a>(call: &StringCall<'a, '_>, before_colons: &'a str) -> Option<StringKey<'a>> {
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
    let reference = semantic_class_reference(written_class, cls_start, call.resolved_names);
    let slot = call.argument.slot;

    if is_morph_map_registration_method(call.name)
        && resolve_known_class_reference(
            call.content,
            reference,
            "Illuminate\\Database\\Eloquent\\Relations",
            &["Relation"],
            true,
            call.indexed_class_exists,
        )
        .is_some()
    {
        return (slot.is_first_or_named("map") && call.argument.shape.is_array_key())
            .then_some(StringKey::Whole(LaravelStringKind::MorphAlias));
    }
    if call.name.eq_ignore_ascii_case("set")
        && slot.is_first_or_named("key")
        && call.argument.shape.is_array_key()
        && is_laravel_facade_reference(call.content, reference, "Config", call.indexed_class_exists)
    {
        return Some(StringKey::Whole(LaravelStringKind::Config));
    }
    if let Some(trigger) = config_resource_static_trigger(
        call.content,
        reference,
        call.name,
        call.indexed_class_exists,
    ) {
        let shape_fits = match call.argument.shape {
            StringArgumentShape::Scalar => trigger.shape.accepts_scalar(),
            StringArgumentShape::ArrayElement | StringArgumentShape::ArrayValue => {
                trigger.shape.accepts_array()
            }
            StringArgumentShape::ArrayKey => false,
        };
        return (slot.is_first_or_named(trigger.argument) && shape_fits).then_some(
            StringKey::Whole(LaravelStringKind::ConfigResource(trigger.kind)),
        );
    }
    if call.name.eq_ignore_ascii_case("middleware")
        && slot.is_first_or_named("middleware")
        && call.argument.shape != StringArgumentShape::ArrayKey
        && is_laravel_facade_reference(call.content, reference, "Route", call.indexed_class_exists)
    {
        return middleware_key(call.literal);
    }

    // Classified by the class as written, the way the symbol map classifies
    // the finished call. Resource triggers resolve semantic aliases exactly;
    // an unrelated `Vendor\Config as Foo` must not make `Foo::get()` read as
    // Laravel Config.
    let ArgumentSlot::Positional(index) = slot else {
        return None;
    };
    let clean_class = written_class.trim_start_matches('\\');
    let kind = facade_static_call_arg_kind(clean_class, call.name, index)?;
    call.shape_fits(&kind).then_some(StringKey::Whole(kind))
}

/// A string passed to an instance method call: `redirect()->route('…')`,
/// `$user->can('…')`, `view()->renderWhen($ok, '…')`.
///
/// `receiver` is the text before the call's `->`, and `before_name` the
/// same text with the `->` itself.
fn instance_call_key<'a>(
    call: &StringCall<'a, '_>,
    receiver: &'a str,
    before_name: &'a str,
) -> Option<StringKey<'a>> {
    let receiver = TextReceiver {
        call,
        receiver,
        before_name,
        chain_start: OnceCell::new(),
    };
    let slot = call.argument.slot;
    if call.name.eq_ignore_ascii_case("middleware")
        && slot.is_first_or_named("middleware")
        && call.argument.shape != StringArgumentShape::ArrayKey
        && receiver.roots_at_route()
    {
        return middleware_key(call.literal);
    }
    let ArgumentSlot::Positional(index) = slot else {
        return None;
    };
    let kind = instance_method_arg_kind(call.name, index, &receiver)?;
    call.shape_fits(&kind).then_some(StringKey::Whole(kind))
}

/// A string passed to a function: `route('…')`, `config('…')`, and the
/// markers Blade directives lower to.
fn function_call_key(call: &StringCall<'_, '_>) -> Option<LaravelStringKind> {
    let slot = call.argument.slot;
    let config_array_key = call.name.eq_ignore_ascii_case("config")
        && slot.is_first_or_named("key")
        && call.argument.shape.is_array_key();
    if !config_array_key
        && call.argument.shape != StringArgumentShape::Scalar
        && (call.argument.shape != StringArgumentShape::ArrayElement
            || !helper_function_accepts_name_list(call.name))
    {
        return None;
    }
    let bp_bytes = call.before_paren.as_bytes();
    let mut callable_start = call.name_start;
    while callable_start > 0
        && (bp_bytes[callable_start - 1].is_ascii_alphanumeric()
            || matches!(bp_bytes[callable_start - 1], b'_' | b'\\'))
    {
        callable_start -= 1;
    }
    let written_function = &call.before_paren[callable_start..];
    if let Some(trigger) = crate::symbol_map::laravel_resources::auth_helper_trigger(
        call.content,
        written_function,
        callable_start as u32,
        call.resolved_names,
        call.indexed_function_exists,
    ) {
        slot.is_first_or_named(trigger.argument)
            .then_some(LaravelStringKind::ConfigResource(trigger.kind))
    } else if config_array_key {
        Some(LaravelStringKind::Config)
    } else if slot == ArgumentSlot::Positional(0) {
        helper_function_first_arg_kind(call.name)
    } else {
        None
    }
}

/// The receiver of an instance method call, read from the text before its
/// `->`.  Each fact is worked out only when a method name asks for it.
struct TextReceiver<'a, 'c, 'f> {
    call: &'c StringCall<'a, 'f>,
    /// Everything before the `->`, back to the start of the file.
    receiver: &'a str,
    /// `receiver` with the call's own `->`, which counts as a link of the
    /// chain a facade root is looked for down.
    before_name: &'a str,
    /// Where the statement the receiver chain is part of starts, found the
    /// first time a facade root is asked about: only the text back to it is
    /// searched, since an unrelated `Gate::` far above would false-positive.
    chain_start: OnceCell<usize>,
}

impl TextReceiver<'_, '_, '_> {
    /// Whether the receiver chain starts at a static call on the Laravel
    /// facade `facade`.
    fn chain_roots_at(&self, facade: &'static str) -> bool {
        let start = *self
            .chain_start
            .get_or_init(|| receiver_chain_start(self.before_name));
        chain_starts_at_laravel_facade(
            self.call.content,
            &self.before_name[start..],
            start,
            self.call.resolved_names,
            facade,
            self.call.indexed_class_exists,
        )
    }
}

impl MethodReceiver for TextReceiver<'_, '_, '_> {
    fn is_this(&self) -> bool {
        self.receiver.ends_with("$this")
    }

    // The class the call is typed into may not parse yet, so `$this` reads
    // as a console command and a mailable alike: a stray suggestion in a
    // class that turns out to take neither costs less than a missing one.
    fn is_console_command(&self) -> bool {
        self.is_this()
    }

    fn is_mailable(&self) -> bool {
        self.is_this()
    }

    /// Mirrors the symbol map's rule: a name ending in `user`, or any
    /// `user()` call.
    fn is_user_like(&self) -> bool {
        let tail = self
            .receiver
            .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        tail.get(tail.len().saturating_sub(4)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case("user"))
            || self
                .receiver
                .get(self.receiver.len().saturating_sub(6)..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case("user()"))
    }

    fn roots_at_gate(&self) -> bool {
        self.chain_roots_at("Gate")
    }

    fn roots_at_route(&self) -> bool {
        self.chain_roots_at("Route")
    }

    fn is_route_name_helper(&self) -> bool {
        ["redirect", "url", "response"]
            .iter()
            .any(|helper| ends_with_bare_helper_call(self.receiver, helper))
    }

    fn is_view_factory(&self) -> bool {
        ends_with_bare_helper_call(self.receiver, "view")
    }

    fn is_config_repository(&self) -> bool {
        ends_with_bare_helper_call(self.receiver, "config")
    }

    fn is_container(&self) -> bool {
        ends_with_bare_helper_call(self.receiver, "app")
            || self.receiver.ends_with("$this->app")
            || self.receiver.ends_with("$app")
    }
}

/// Whether `text` ends with a call to the function `name` that passes no
/// arguments, `name()` or `\name()`, rather than a method of that name.
fn ends_with_bare_helper_call(text: &str, name: &str) -> bool {
    let Some(before_close) = text.strip_suffix(')') else {
        return false;
    };
    let Some(before_open) = before_close.trim_end().strip_suffix('(') else {
        return false;
    };
    let before_open = before_open.trim_end();
    let name_start = scan_ident_backward(before_open.as_bytes(), before_open.len());
    if !before_open[name_start..].eq_ignore_ascii_case(name) {
        return false;
    }
    let before_name = before_open[..name_start]
        .strip_suffix('\\')
        .unwrap_or(&before_open[..name_start])
        .trim_end();
    !(before_name.ends_with("->")
        || before_name.ends_with("::")
        || before_name.ends_with('$')
        || before_name.ends_with("new")
        || before_name.ends_with("function"))
}
