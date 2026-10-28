//! Literal sites whose enclosing syntax may declare Laravel validation rules.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::completion::source::code_context::CodeContext;

use super::super::helpers::{extract_string_literal_including_empty, resolved_string_literal};
use super::super::validation_rules::{RulesArray, rules_from_program_range};

/// Which part of a validation declaration the literal belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SiteKind {
    Rule,
    MessageKey,
    AttributeKey,
    InValue,
}

/// The expression or declaration whose type must confirm a candidate site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ValidationOwner {
    /// An instance call; `None` identifies an explicitly named `rules` argument.
    Method {
        receiver_start: usize,
        receiver_end: usize,
        method: String,
        rules_argument: Option<usize>,
    },
    /// A static call, with its class name still in the source file's terms.
    Static {
        class: String,
        offset: u32,
        method: String,
    },
    /// An array returned directly by a class method.
    RulesMethod { offset: u32, method: String },
}

/// A sibling field and the source expression defining its validation rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValidationField {
    /// The field's literal name, including any dot or wildcard segments.
    pub name: String,
    /// Start of the field name inside its quotes.
    pub start: usize,
    /// End of the field name inside its quotes.
    pub end: usize,
    /// Start of the rule expression, including quotes or brackets.
    pub value_start: usize,
    /// End of the rule expression.
    pub value_end: usize,
}

/// One literal to confirm through the receiver or enclosing class's type.
#[derive(Clone, Debug)]
pub(crate) struct ValidationSite {
    /// Start of the literal's contents, excluding the opening quote.
    pub start: usize,
    /// End of the literal's contents, excluding the closing quote.
    pub end: usize,
    /// The literal's role within its declaration.
    pub kind: SiteKind,
    /// The field this rule belongs to, indexed into `fields`.
    pub field: Option<usize>,
    /// Field declarations shared by every literal in the same rules array.
    pub fields: Arc<[ValidationField]>,
    /// Whether every sibling field name was statically recoverable.
    pub fields_complete: bool,
    /// The receiver or enclosing method to confirm.
    pub owner: Arc<ValidationOwner>,
    /// Start of the surrounding rules expression.
    pub array_start: usize,
    /// End of the surrounding rules expression.
    pub array_end: usize,
    /// Whether this is a whole positional rule in an array of rules.
    pub array_rule: bool,
    /// The written class and its offset for a `Rule::in()` candidate.
    pub in_rule_class: Option<(String, u32)>,
    /// Rules recovered from the same repaired AST when an enum value is unfinished.
    pub recovered_rules: Option<Arc<RulesArray>>,
}

/// The byte range of one rule and its optional parameter suffix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuleToken {
    /// Start of the rule name.
    pub start: usize,
    /// End of the rule, including its parameters.
    pub end: usize,
    /// End of the rule name, before the colon or trailing whitespace.
    pub name_end: usize,
    /// Start of the parameters, immediately after the colon.
    pub parameters_start: Option<usize>,
}

/// Collect candidates from syntax; callers confirm their framework types.
pub(crate) fn collect(content: &str) -> Vec<ValidationSite> {
    crate::parser::with_parsed_program(content, "validation_string_sites", |program, content| {
        let mut sites = Vec::new();
        visit(Node::Program(program), content, &mut sites);
        sites.sort_unstable_by_key(|site| site.start);
        sites
    })
}

/// Find the literal being edited, completing its syntax only when needed.
pub(crate) fn at_cursor(
    content: &str,
    cursor: usize,
    code: &CodeContext<'_>,
) -> Option<ValidationSite> {
    if let Some(site) = collect(content)
        .into_iter()
        .find(|site| site.start <= cursor && cursor <= site.end)
    {
        return Some(site);
    }
    let (quote_start, quote) = code.open_string?;
    // Recovery must not turn an existing array key into a positional rule.
    let after_quote = crate::text_scan::skip_string_forward(content.as_bytes(), quote_start);
    let after_quote = crate::text_scan::skip_php_trivia(content.as_bytes(), after_quote);
    if content
        .as_bytes()
        .get(after_quote..)
        .is_some_and(|tail| tail.starts_with(b"=>"))
    {
        return None;
    }
    // Keep the source up to the cursor so every recovered span still indexes
    // the original file. The shared lexical scan supplies the missing closers.
    for after_literal in ["", " => null"] {
        let mut repaired = content.get(..cursor)?.to_string();
        repaired.push(quote);
        repaired.push_str(after_literal);
        close_syntax(&mut repaired, code);
        let recovered = crate::parser::with_parsed_program(
            &repaired,
            "validation_string_recovery",
            |program, source| {
                let mut sites = Vec::new();
                visit(Node::Program(program), source, &mut sites);
                let mut site = sites
                    .into_iter()
                    .find(|site| site.start == quote_start + 1 && site.end == cursor)?;
                let needs_enum = site.kind == SiteKind::InValue
                    || (site.kind == SiteKind::Rule
                        && token_at(&site, source, cursor).is_some_and(|token| {
                            literal_value(&site, source, token.start, token.name_end).is_some_and(
                                |name| {
                                    super::rule_matches(&name, "in")
                                        || super::rule_matches(&name, "not_in")
                                },
                            )
                        }));
                if needs_enum {
                    site.recovered_rules =
                        rules_from_program_range(program, source, site.array_start, site.array_end)
                            .map(Arc::new);
                }
                Some(site)
            },
        );
        if let Some(mut site) = recovered {
            site.array_end = site.array_end.min(content.len());
            for field in Arc::make_mut(&mut site.fields) {
                field.value_end = field.value_end.min(content.len());
            }
            return Some(site);
        }
    }
    None
}

fn close_syntax(repaired: &mut String, code: &CodeContext<'_>) {
    let mut needs_semicolon = true;
    for bracket in code.open_brackets.iter().rev() {
        if bracket.byte == b'{' {
            if needs_semicolon {
                repaired.push(';');
            }
            repaired.push('}');
            needs_semicolon = false;
        } else {
            repaired.push(if bracket.byte == b'(' { ')' } else { ']' });
            needs_semicolon = true;
        }
    }
    if needs_semicolon {
        repaired.push(';');
    }
}

fn visit(node: Node<'_, '_>, content: &str, sites: &mut Vec<ValidationSite>) {
    match node {
        Node::MethodCall(call) => collect_method_call(
            call.object,
            &call.method,
            &call.argument_list,
            content,
            sites,
        ),
        Node::NullSafeMethodCall(call) => collect_method_call(
            call.object,
            &call.method,
            &call.argument_list,
            content,
            sites,
        ),
        Node::StaticMethodCall(call) => {
            if let ClassLikeMemberSelector::Identifier(method) = &call.method
                && method.value.eq_ignore_ascii_case(b"make")
                && let Expression::Identifier(class) = call.class
                && let Some((rules, _)) = argument(&call.argument_list, "rules", 1)
            {
                collect_rules(
                    rules,
                    Arc::new(ValidationOwner::Static {
                        class: bytes_to_str(class.value()).to_string(),
                        offset: class.span().start.offset,
                        method: bytes_to_str(method.value).to_string(),
                    }),
                    content,
                    sites,
                );
            }
        }
        Node::Method(method) => {
            let name = method.name.value;
            let kind = if name.eq_ignore_ascii_case(b"rules") {
                Some(SiteKind::Rule)
            } else if name.eq_ignore_ascii_case(b"messages") {
                Some(SiteKind::MessageKey)
            } else if name.eq_ignore_ascii_case(b"attributes") {
                Some(SiteKind::AttributeKey)
            } else {
                None
            };
            if let Some(kind) = kind
                && let MethodBody::Concrete(block) = &method.body
            {
                let owner = Arc::new(ValidationOwner::RulesMethod {
                    offset: method.name.span.start.offset,
                    method: bytes_to_str(method.name.value).to_string(),
                });
                collect_returns(Node::Block(block), kind, &owner, content, sites);
            }
        }
        _ => {}
    }
    node.visit_children(|child| visit(child, content, sites));
}

fn collect_method_call(
    receiver: &Expression<'_>,
    selector: &ClassLikeMemberSelector<'_>,
    arguments: &ArgumentList<'_>,
    content: &str,
    sites: &mut Vec<ValidationSite>,
) {
    let ClassLikeMemberSelector::Identifier(method) = selector else {
        return;
    };
    let name = bytes_to_str(method.value);
    let positions: &[usize] = if name.eq_ignore_ascii_case("validate") {
        &[0, 1]
    } else if name.eq_ignore_ascii_case("validateWithBag") {
        &[1, 2]
    } else if name.eq_ignore_ascii_case("sometimes") {
        &[1]
    } else {
        return;
    };
    for &position in positions {
        let Some((rules, rules_argument)) = argument(arguments, "rules", position) else {
            continue;
        };
        let owner = Arc::new(ValidationOwner::Method {
            receiver_start: receiver.span().start.offset as usize,
            receiver_end: receiver.span().end.offset as usize,
            method: name.to_string(),
            rules_argument,
        });
        if name.eq_ignore_ascii_case("sometimes") {
            if let Some((attributes, _)) = argument(arguments, "attribute", 0) {
                let mut fields = Vec::new();
                collect_attribute_names(attributes, rules, content, &mut fields);
                let fields: Arc<[ValidationField]> = fields.into();
                let group = RuleGroup::new(rules, owner, fields, false);
                collect_rule_value(
                    rules,
                    (!group.fields.is_empty()).then_some(0),
                    false,
                    &group,
                    content,
                    sites,
                );
            }
        } else {
            collect_rules(rules, owner, content, sites);
        }
        if rules_argument.is_none() {
            break;
        }
    }
}

fn argument<'ast, 'arena>(
    arguments: &'ast ArgumentList<'arena>,
    name: &str,
    position: usize,
) -> Option<(&'ast Expression<'arena>, Option<usize>)> {
    if let Some(named) = arguments.arguments.iter().find_map(|arg| match arg {
        Argument::Named(named) if bytes_to_str(named.name.value) == name => Some(named),
        _ => None,
    }) {
        return Some((named.value, None));
    }
    match arguments.arguments.iter().nth(position)? {
        Argument::Positional(positional) if positional.ellipsis.is_none() => {
            Some((positional.value, Some(position)))
        }
        _ => None,
    }
}

fn collect_returns(
    node: Node<'_, '_>,
    kind: SiteKind,
    owner: &Arc<ValidationOwner>,
    content: &str,
    sites: &mut Vec<ValidationSite>,
) {
    match node {
        Node::Closure(_)
        | Node::ArrowFunction(_)
        | Node::Function(_)
        | Node::Class(_)
        | Node::AnonymousClass(_) => return,
        Node::Return(ret) => {
            if let Some(value) = ret.value {
                if kind == SiteKind::Rule {
                    collect_rules(value, Arc::clone(owner), content, sites);
                } else {
                    let group = RuleGroup::new(value, Arc::clone(owner), Arc::from([]), false);
                    visit_array_entries(value, &mut |element| {
                        if let ArrayElement::KeyValue(entry) = element {
                            push_literal(
                                entry.key,
                                LiteralRole::new(kind, None),
                                &group,
                                content,
                                sites,
                            );
                        }
                    });
                }
            }
            return;
        }
        _ => {}
    }
    node.visit_children(|child| collect_returns(child, kind, owner, content, sites));
}

struct RuleGroup {
    owner: Arc<ValidationOwner>,
    fields: Arc<[ValidationField]>,
    fields_complete: bool,
    start: usize,
    end: usize,
}

impl RuleGroup {
    fn new(
        expression: &Expression<'_>,
        owner: Arc<ValidationOwner>,
        fields: Arc<[ValidationField]>,
        fields_complete: bool,
    ) -> Self {
        Self {
            owner,
            fields,
            fields_complete,
            start: expression.span().start.offset as usize,
            end: expression.span().end.offset as usize,
        }
    }
}

fn collect_rules(
    expression: &Expression<'_>,
    owner: Arc<ValidationOwner>,
    content: &str,
    sites: &mut Vec<ValidationSite>,
) {
    let mut fields = Vec::new();
    let mut keys_complete = true;
    let arrays_complete = visit_array_entries(expression, &mut |element| {
        if let ArrayElement::KeyValue(entry) = element
            && let Some((name, start, end)) = resolved_string_literal(entry.key, content)
        {
            fields.push(ValidationField {
                name: name.to_string(),
                start,
                end,
                value_start: entry.value.span().start.offset as usize,
                value_end: entry.value.span().end.offset as usize,
            });
        } else {
            keys_complete = false;
        }
    });
    // PHP retains the last value assigned to each string key. Keep the fields
    // in source order so the second walk can match them without another map.
    let keep: Vec<_> = {
        let mut seen = HashSet::new();
        fields
            .iter()
            .rev()
            .map(|field| seen.insert(field.name.as_str()))
            .collect()
    };
    let mut keep = keep.into_iter().rev();
    fields.retain(|_| keep.next().unwrap_or(false));
    let group = RuleGroup::new(
        expression,
        owner,
        fields.into(),
        keys_complete && arrays_complete,
    );
    let mut field_index = 0;
    visit_array_entries(expression, &mut |element| {
        if let ArrayElement::KeyValue(entry) = element {
            let field = group
                .fields
                .get(field_index)
                .filter(|field| field.value_start == entry.value.span().start.offset as usize)
                .map(|_| {
                    let field = field_index;
                    field_index += 1;
                    field
                });
            if field.is_none() && resolved_string_literal(entry.key, content).is_some() {
                return;
            }
            collect_rule_value(entry.value, field, false, &group, content, sites);
        }
    });
}

fn visit_array_entries<'ast, 'arena>(
    expression: &'ast Expression<'arena>,
    visitor: &mut impl FnMut(&'ast ArrayElement<'arena>),
) -> bool {
    match expression {
        Expression::Array(array) => {
            array.elements.iter().for_each(visitor);
            true
        }
        Expression::LegacyArray(array) => {
            array.elements.iter().for_each(visitor);
            true
        }
        Expression::Parenthesized(parenthesized) => {
            visit_array_entries(parenthesized.expression, visitor)
        }
        Expression::Call(Call::Function(call)) if matches!(call.function, Expression::Identifier(name) if name.value().eq_ignore_ascii_case(b"array_merge") || name.value().eq_ignore_ascii_case(b"\\array_merge")) =>
        {
            let mut complete = true;
            for arg in call.argument_list.arguments.iter() {
                complete &= visit_array_entries(arg.value(), visitor);
            }
            complete
        }
        _ => false,
    }
}

fn array_value<'ast, 'arena>(
    element: &'ast ArrayElement<'arena>,
) -> Option<&'ast Expression<'arena>> {
    match element {
        ArrayElement::Value(value) => Some(value.value),
        ArrayElement::KeyValue(value) => Some(value.value),
        _ => None,
    }
}

fn collect_attribute_names(
    expression: &Expression<'_>,
    rules: &Expression<'_>,
    content: &str,
    fields: &mut Vec<ValidationField>,
) {
    if let Some((name, start, end)) = resolved_string_literal(expression, content) {
        fields.push(ValidationField {
            name: name.to_string(),
            start,
            end,
            value_start: rules.span().start.offset as usize,
            value_end: rules.span().end.offset as usize,
        });
    } else {
        visit_array_entries(expression, &mut |element| {
            if let Some(value) = array_value(element) {
                collect_attribute_names(value, rules, content, fields);
            }
        });
    }
}

fn collect_rule_value(
    expression: &Expression<'_>,
    field: Option<usize>,
    array_rule: bool,
    group: &RuleGroup,
    content: &str,
    sites: &mut Vec<ValidationSite>,
) {
    match expression {
        Expression::Array(_) | Expression::LegacyArray(_) => {
            if !array_rule {
                visit_array_entries(expression, &mut |element| {
                    if let Some(value) = array_value(element) {
                        collect_rule_value(value, field, true, group, content, sites);
                    }
                });
            }
        }
        Expression::Parenthesized(parenthesized) => collect_rule_value(
            parenthesized.expression,
            field,
            array_rule,
            group,
            content,
            sites,
        ),
        Expression::Call(Call::StaticMethod(call)) => {
            if let ClassLikeMemberSelector::Identifier(method) = &call.method
                && method.value.eq_ignore_ascii_case(b"in")
                && let Expression::Identifier(class) = call.class
            {
                let name = bytes_to_str(class.value());
                for argument in call.argument_list.arguments.iter() {
                    let value = argument.value();
                    let class = Some((name.to_string(), class.span().start.offset));
                    if matches!(value, Expression::Array(_) | Expression::LegacyArray(_)) {
                        visit_array_entries(value, &mut |element| {
                            if let Some(value) = array_value(element) {
                                let role = LiteralRole {
                                    in_rule_class: class.clone(),
                                    ..LiteralRole::new(SiteKind::InValue, field)
                                };
                                push_literal(value, role, group, content, sites);
                            }
                        });
                    } else {
                        let role = LiteralRole {
                            in_rule_class: class,
                            ..LiteralRole::new(SiteKind::InValue, field)
                        };
                        push_literal(value, role, group, content, sites);
                    }
                }
            }
        }
        _ => {
            let role = LiteralRole {
                array_rule,
                ..LiteralRole::new(SiteKind::Rule, field)
            };
            push_literal(expression, role, group, content, sites);
        }
    }
}

struct LiteralRole {
    kind: SiteKind,
    field: Option<usize>,
    array_rule: bool,
    in_rule_class: Option<(String, u32)>,
}

impl LiteralRole {
    fn new(kind: SiteKind, field: Option<usize>) -> Self {
        Self {
            kind,
            field,
            array_rule: false,
            in_rule_class: None,
        }
    }
}

fn push_literal(
    expression: &Expression<'_>,
    role: LiteralRole,
    group: &RuleGroup,
    content: &str,
    sites: &mut Vec<ValidationSite>,
) {
    if let Some((_, start, end)) = extract_string_literal_including_empty(expression, content) {
        sites.push(ValidationSite {
            start,
            end,
            kind: role.kind,
            field: role.field,
            fields: Arc::clone(&group.fields),
            fields_complete: group.fields_complete,
            owner: Arc::clone(&group.owner),
            array_start: group.start,
            array_end: group.end,
            array_rule: role.array_rule,
            in_rule_class: role.in_rule_class,
            recovered_rules: None,
        });
    }
}

/// Split scalar rule strings the way Laravel does, leaving array items whole.
pub(crate) fn rule_tokens(site: &ValidationSite, content: &str) -> Vec<RuleToken> {
    let Some(decoded) = decoded_literal(site, content, site.start, site.end) else {
        return Vec::new();
    };
    let value = decoded.value.as_ref();
    let mut tokens = Vec::new();
    let mut start = 0;
    loop {
        let remaining = &value[start..];
        let length = if site.array_rule {
            remaining.len()
        } else {
            remaining.find('|').unwrap_or(remaining.len())
        };
        let raw = &remaining[..length];
        let trimmed = raw.trim_start();
        let token_start = start + raw.len() - raw.trim_start().len();
        let colon = trimmed.find(':');
        let name = colon.map_or(trimmed, |colon| &trimmed[..colon]).trim_end();
        tokens.push(RuleToken {
            start: decoded.source_offset(token_start),
            end: decoded.source_offset(
                token_start
                    + if colon.is_some() {
                        trimmed.len()
                    } else {
                        name.len()
                    },
            ),
            name_end: decoded.source_offset(token_start + name.len()),
            parameters_start: colon.map(|colon| decoded.source_offset(token_start + colon + 1)),
        });
        start += length;
        if start == value.len() {
            break;
        }
        start += 1;
    }
    tokens
}

/// Decode a selected part of a literal without changing its source range.
pub(crate) fn literal_value<'a>(
    site: &ValidationSite,
    content: &'a str,
    start: usize,
    end: usize,
) -> Option<Cow<'a, str>> {
    let raw = content.get(start..end)?;
    if !raw.contains('\\') {
        return Some(Cow::Borrowed(raw));
    }
    let quote = *content.as_bytes().get(site.start.checked_sub(1)?)? as char;
    let mut quoted = String::with_capacity(raw.len() + 2);
    quoted.push(quote);
    quoted.push_str(raw);
    quoted.push(quote);
    crate::text_scan::decode_php_string_literal(&quoted).map(|value| Cow::Owned(value.into_owned()))
}

/// Decoded text with offsets back into the original literal's source.
pub(super) struct DecodedLiteral<'a> {
    /// The value PHP passes to the validation parser.
    pub value: Cow<'a, str>,
    start: usize,
    offsets: Option<Vec<usize>>,
}

impl DecodedLiteral<'_> {
    /// Translate a decoded byte boundary into an absolute source offset.
    pub fn source_offset(&self, offset: usize) -> usize {
        self.start
            + self
                .offsets
                .as_ref()
                .map_or(offset, |offsets| offsets[offset])
    }
}

/// Build source offsets only for escaped literals; ordinary strings stay borrowed.
pub(super) fn decoded_literal<'a>(
    site: &ValidationSite,
    content: &'a str,
    start: usize,
    end: usize,
) -> Option<DecodedLiteral<'a>> {
    let value = literal_value(site, content, start, end)?;
    let raw = content.get(start..end)?;
    let offsets = if raw.contains('\\') {
        let double = content.as_bytes()[site.start - 1] == b'"';
        let mut offsets = Vec::with_capacity(value.len() + 1);
        let mut index = 0;
        while index < raw.len() {
            let (consumed, decoded) = escape_width(raw.as_bytes(), index, double);
            if consumed == decoded {
                offsets.extend(index..index + consumed);
            } else {
                offsets.extend(std::iter::repeat_n(index, decoded));
            }
            index += consumed;
        }
        offsets.push(raw.len());
        Some(offsets)
    } else {
        None
    };
    Some(DecodedLiteral {
        value,
        start,
        offsets,
    })
}

// Only the source widths are needed here. The shared PHP literal decoder above
// supplies the value and rejects malformed Unicode escapes before this runs.
fn escape_width(bytes: &[u8], index: usize, double: bool) -> (usize, usize) {
    if bytes[index] != b'\\' || index + 1 == bytes.len() {
        return (1, 1);
    }
    let next = bytes[index + 1];
    if !double {
        return (2, if matches!(next, b'\\' | b'\'') { 1 } else { 2 });
    }
    match next {
        b'\\' | b'"' | b'$' | b'n' | b'r' | b't' | b'v' | b'e' | b'f' => (2, 1),
        b'x' => {
            let digits = bytes[index + 2..]
                .iter()
                .take(2)
                .take_while(|byte| byte.is_ascii_hexdigit())
                .count();
            (2 + digits, if digits == 0 { 2 } else { 1 })
        }
        b'0'..=b'7' => {
            let digits = bytes[index + 1..]
                .iter()
                .take(3)
                .take_while(|&&byte| (b'0'..=b'7').contains(&byte))
                .count();
            (1 + digits, 1)
        }
        b'u' if bytes.get(index + 2) == Some(&b'{') => {
            let mut end = index + 3;
            let mut codepoint = 0;
            while bytes[end] != b'}' {
                codepoint = codepoint * 16
                    + (bytes[end] as char)
                        .to_digit(16)
                        .expect("the shared decoder validated this escape");
                end += 1;
            }
            (
                end + 1 - index,
                char::from_u32(codepoint)
                    .expect("the shared decoder validated this code point")
                    .len_utf8(),
            )
        }
        _ => (2, 2),
    }
}

/// The rule whose name or parameters contain the cursor.
pub(crate) fn token_at(site: &ValidationSite, content: &str, cursor: usize) -> Option<RuleToken> {
    rule_tokens(site, content)
        .into_iter()
        .find(|token| token.start <= cursor && cursor <= token.end)
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
