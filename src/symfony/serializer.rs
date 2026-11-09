//! Symfony serializer groups.
//!
//! A group is declared by each member put in it: a `#[Groups]` attribute on
//! a property, a method, or a class, and the `groups` of an attribute in a
//! YAML or XML serializer mapping.  It is read by the `groups` of a
//! serialization context: the one a serializer's or a normalizer's
//! `serialize()`, `deserialize()`, `normalize()`, or `denormalize()`, or a
//! controller's `json()`, is given, and the ones `#[Context]`,
//! `#[MapRequestPayload]`, and `#[MapQueryString]` hold.  A group nothing
//! declares selects nothing, which is no error, so a read is never
//! reported.
//!
//! The mappings are read as text, by indentation in YAML and by tag in XML.

use mago_syntax::cst::*;

use crate::symbol_map::{ReceiverClass, StringKeyKind, SymbolKind, SymbolSpan};

/// Whether `uri` is a serializer mapping: a `serializer.*` or
/// `serialization.*` file, or one in a `serializer` or `serialization`
/// directory, under a `config` directory (the application's
/// `config/serializer/`, or a bundle's `Resources/config/serialization.xml`).
pub(crate) fn is_serializer_mapping_uri(uri: &str) -> bool {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    let mut segments = path.split('/').peekable();
    let mut in_config = false;
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            return in_config
                && (segment.starts_with("serializer.") || segment.starts_with("serialization."));
        }
        if in_config && (segment == "serializer" || segment == "serialization") {
            return true;
        }
        in_config |= segment == "config";
    }
    false
}

/// An attribute whose arguments name serializer groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupsAttribute {
    /// `#[Groups]`, which puts what it is on in its groups.
    Groups,
    /// `#[Context]`, whose contexts and the groups it applies in.
    Context,
    /// `#[MapRequestPayload]`, whose `serializationContext` is its second
    /// parameter.
    MapRequestPayload,
    /// `#[MapQueryString]`, whose `serializationContext` is its first.
    MapQueryString,
}

/// The attribute `class`, as resolved, is, when its arguments name groups.
pub(crate) fn groups_attribute(class: &str) -> Option<GroupsAttribute> {
    let class = class.trim_start_matches('\\');
    let is = |name: &str| class.eq_ignore_ascii_case(name);
    if is("Symfony\\Component\\Serializer\\Attribute\\Groups")
        || is("Symfony\\Component\\Serializer\\Annotation\\Groups")
    {
        Some(GroupsAttribute::Groups)
    } else if is("Symfony\\Component\\Serializer\\Attribute\\Context")
        || is("Symfony\\Component\\Serializer\\Annotation\\Context")
    {
        Some(GroupsAttribute::Context)
    } else if is("Symfony\\Component\\HttpKernel\\Attribute\\MapRequestPayload") {
        Some(GroupsAttribute::MapRequestPayload)
    } else if is("Symfony\\Component\\HttpKernel\\Attribute\\MapQueryString") {
        Some(GroupsAttribute::MapQueryString)
    } else {
        None
    }
}

/// A parameter, by position and by name.
type Param = (usize, &'static str);

/// The strings an attribute's arguments name groups in, and whether they
/// declare them (`#[Groups]`) rather than read them.
pub(crate) fn attribute_groups<'e, 'a>(
    attribute: GroupsAttribute,
    arguments: &'e PartialArgumentList<'a>,
) -> (Vec<&'e Expression<'a>>, bool) {
    // Each parameter that holds groups or a context, by position and name.
    let (contexts, groups): (&[Param], Option<Param>) = match attribute {
        GroupsAttribute::Groups => (&[], Some((0, "groups"))),
        GroupsAttribute::Context => (
            &[
                (0, "context"),
                (1, "normalizationContext"),
                (2, "denormalizationContext"),
            ],
            Some((3, "groups")),
        ),
        GroupsAttribute::MapRequestPayload => (&[(1, "serializationContext")], None),
        GroupsAttribute::MapQueryString => (&[(0, "serializationContext")], None),
    };
    let mut found = Vec::new();
    for (position, argument) in arguments.arguments.iter().enumerate() {
        let (value, named) = match argument {
            PartialArgument::Positional(argument) if argument.ellipsis.is_none() => {
                (argument.value, None)
            }
            PartialArgument::Named(argument) => (argument.value, Some(argument.name.value)),
            _ => continue,
        };
        let is_param = |(index, name): (usize, &str)| match named {
            Some(named) => named == name.as_bytes(),
            None => index == position,
        };
        if groups.is_some_and(is_param) {
            found.extend(group_values(value));
        } else if contexts.iter().any(|param| is_param(*param)) {
            found.extend(context_groups(value));
        }
    }
    (found, attribute == GroupsAttribute::Groups)
}

/// The groups the context of a serializer call names, with the receiver the
/// call has to be on: a serializer's or a normalizer's `serialize()`,
/// `deserialize()`, `normalize()`, or `denormalize()`, or a controller's
/// `json()`.
pub(crate) fn call_context_groups<'e, 'a>(
    method: &str,
    arguments: &'e ArgumentList<'a>,
) -> Option<(ReceiverClass, Vec<&'e Expression<'a>>)> {
    let is = |name: &str| method.eq_ignore_ascii_case(name);
    let (receiver, index) = if is("serialize") || is("normalize") {
        (ReceiverClass::Serializer, 2)
    } else if is("deserialize") || is("denormalize") {
        (ReceiverClass::Serializer, 3)
    } else if is("json") {
        (ReceiverClass::ControllerJson, 3)
    } else {
        return None;
    };
    let context = arguments
        .arguments
        .iter()
        .enumerate()
        .find_map(|(position, argument)| match argument {
            Argument::Positional(argument) if argument.ellipsis.is_none() && position == index => {
                Some(argument.value)
            }
            Argument::Named(argument) if argument.name.value == b"context" => Some(argument.value),
            _ => None,
        })?;
    let groups = context_groups(context);
    (!groups.is_empty()).then_some((receiver, groups))
}

/// A string in PHP that names a group, with the receiver a call has to be
/// on for it to.
pub(crate) struct GroupString<'e, 'a> {
    pub start: u32,
    pub end: u32,
    pub receiver: Option<(&'e Expression<'a>, ReceiverClass)>,
}

/// Every string in `program` that names a group, read the way the symbol
/// map reads them.  `resolve` gives the class a name written at an offset
/// names.
pub(crate) fn group_strings<'e, 'a>(
    program: &'e Program<'a>,
    resolve: &(dyn Fn(&str, u32) -> String + Sync),
) -> Vec<GroupString<'e, 'a>> {
    let finder = GroupFinder { resolve };
    let mut found = Vec::new();
    for statement in program.statements.iter() {
        mago_syntax::walker::Walker::walk_statement(&finder, statement, &mut found);
    }
    found
}

struct GroupFinder<'r> {
    resolve: &'r (dyn Fn(&str, u32) -> String + Sync),
}

fn string_range(expr: &Expression<'_>) -> Option<(u32, u32)> {
    match expr {
        Expression::Literal(Literal::String(literal)) => Some((
            literal.span.start.offset + 1,
            literal.span.end.offset.saturating_sub(1),
        )),
        _ => None,
    }
}

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Vec<GroupString<'ast, 'arena>>>
    for GroupFinder<'_>
{
    fn walk_in_attribute(
        &self,
        node: &'ast Attribute<'arena>,
        found: &mut Vec<GroupString<'ast, 'arena>>,
    ) {
        let Some(arguments) = &node.argument_list else {
            return;
        };
        let written = crate::atom::bytes_to_str(node.name.value());
        let class = (self.resolve)(written, mago_span::HasSpan::span(&node.name).start.offset);
        let Some(attribute) = groups_attribute(&class) else {
            return;
        };
        for group in attribute_groups(attribute, arguments).0 {
            if let Some((start, end)) = string_range(group) {
                found.push(GroupString {
                    start,
                    end,
                    receiver: None,
                });
            }
        }
    }

    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        found: &mut Vec<GroupString<'ast, 'arena>>,
    ) {
        let ClassLikeMemberSelector::Identifier(name) = &node.method else {
            return;
        };
        let method = crate::atom::bytes_to_str(name.value);
        let Some((receiver, groups)) = call_context_groups(method, &node.argument_list) else {
            return;
        };
        for group in groups {
            if let Some((start, end)) = string_range(group) {
                found.push(GroupString {
                    start,
                    end,
                    receiver: Some((node.object, receiver)),
                });
            }
        }
    }
}

/// The strings that name a group in a serialization context: the `groups`
/// entry of the array, keyed by the string or by
/// `AbstractNormalizer::GROUPS`, holding a group or a list of them.
pub(crate) fn context_groups<'e, 'a>(context: &'e Expression<'a>) -> Vec<&'e Expression<'a>> {
    let elements = match context {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return Vec::new(),
    };
    let mut groups = Vec::new();
    for element in elements.iter() {
        if let ArrayElement::KeyValue(entry) = element
            && is_groups_key(entry.key)
        {
            groups.extend(group_values(entry.value));
        }
    }
    groups
}

fn is_groups_key(key: &Expression<'_>) -> bool {
    match key {
        Expression::Literal(Literal::String(literal)) => literal.value == Some(b"groups"),
        Expression::Access(Access::ClassConstant(access)) => matches!(
            &access.constant,
            ClassLikeConstantSelector::Identifier(name) if name.value == b"GROUPS"
        ),
        _ => false,
    }
}

/// The strings a group value is: one group, or a list of them.
pub(crate) fn group_values<'e, 'a>(value: &'e Expression<'a>) -> Vec<&'e Expression<'a>> {
    match value {
        Expression::Literal(Literal::String(_)) => vec![value],
        Expression::Array(array) => list_strings(&array.elements),
        Expression::LegacyArray(array) => list_strings(&array.elements),
        _ => Vec::new(),
    }
}

fn list_strings<'e, 'a>(
    elements: &'e TokenSeparatedSequence<'a, ArrayElement<'a>>,
) -> Vec<&'e Expression<'a>> {
    elements
        .iter()
        .filter_map(|element| match element {
            ArrayElement::Value(value)
                if matches!(value.value, Expression::Literal(Literal::String(_))) =>
            {
                Some(value.value)
            }
            _ => None,
        })
        .collect()
}

/// The group spans of one serializer mapping.
pub(crate) fn serializer_mapping_spans(uri: &str, content: &str) -> Vec<SymbolSpan> {
    let mut spans = Vec::new();
    match super::config_format(uri) {
        Some(super::ConfigFormat::Yaml) => scan_yaml(content, &mut spans),
        Some(super::ConfigFormat::Xml) if content.contains("<serializer") => {
            scan_xml(content, &mut spans);
        }
        _ => {}
    }
    spans
}

fn group_span(content: &str, start: usize, end: usize) -> Option<SymbolSpan> {
    let name = content.get(start..end)?;
    (!name.is_empty()).then(|| SymbolSpan {
        start: start as u32,
        end: end as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::SerializerGroup,
            key: name.to_string(),
            is_write: true,
            is_optional: false,
        },
    })
}

/// The span of one YAML scalar, `raw` starting at `start`, quoted or not.
fn push_yaml_scalar(content: &str, raw: &str, start: usize, spans: &mut Vec<SymbolSpan>) {
    let trimmed = raw.trim();
    let start = start + (raw.len() - raw.trim_start().len());
    let (name, (open, _)) = crate::framework::strip_yaml_quotes(trimmed);
    spans.extend(group_span(content, start + open, start + open + name.len()));
}

fn scan_yaml(content: &str, spans: &mut Vec<SymbolSpan>) {
    // The indentation of a `groups:` whose list follows on the next lines.
    let mut open_list: Option<usize> = None;
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = super::resources::yaml_content_before_comment(line);
        let trimmed = semantic.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = crate::framework::leading_spaces(semantic);
        if let Some(key_indent) = open_list {
            if indent >= key_indent && trimmed.starts_with('-') {
                let item = &semantic[indent + 1..];
                push_yaml_scalar(content, item, line_start + indent + 1, spans);
                continue;
            }
            open_list = None;
        }
        if indent == 0 {
            continue;
        }
        let Some((raw_key, _)) = super::resources::yaml_mapping_key(semantic, line_start) else {
            continue;
        };
        if crate::framework::strip_yaml_quotes(raw_key).0 != "groups" {
            continue;
        }
        let Some(colon) = semantic[indent..].find(':') else {
            continue;
        };
        let value_start = indent + colon + 1;
        let value = &semantic[value_start..];
        let value_trimmed = value.trim();
        if value_trimmed.is_empty() {
            open_list = Some(indent);
        } else if let Some(inner) = value_trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            let inner_start =
                line_start + value_start + (value.len() - value.trim_start().len()) + 1;
            let mut offset = 0;
            for item in inner.split(',') {
                push_yaml_scalar(content, item, inner_start + offset, spans);
                offset += item.len() + 1;
            }
        } else {
            push_yaml_scalar(content, value, line_start + value_start, spans);
        }
    }
}

fn scan_xml(content: &str, spans: &mut Vec<SymbolSpan>) {
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    while let Some(relative) = memchr::memchr(b'<', &bytes[cursor..]) {
        let tag_start = cursor + relative;
        if content[tag_start..].starts_with("<!--") {
            cursor = content[tag_start..]
                .find("-->")
                .map_or(bytes.len(), |end| tag_start + end + 3);
            continue;
        }
        let Some(tag_len) = memchr::memchr(b'>', &bytes[tag_start..]) else {
            break;
        };
        let tag_end = tag_start + tag_len + 1;
        cursor = tag_end;
        let tag = &content[tag_start..tag_end];
        let name_end = tag[1..]
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .map_or(tag.len(), |end| end + 1);
        if &tag[1..name_end] != "group" || tag.ends_with("/>") {
            continue;
        }
        let Some(text_len) = memchr::memchr(b'<', &bytes[tag_end..]) else {
            break;
        };
        let text = &content[tag_end..tag_end + text_len];
        let start = tag_end + (text.len() - text.trim_start().len());
        spans.extend(group_span(content, start, start + text.trim().len()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups(uri: &str, content: &str) -> Vec<String> {
        serializer_mapping_spans(uri, content)
            .into_iter()
            .map(|span| content[span.start as usize..span.end as usize].to_string())
            .collect()
    }

    #[test]
    fn yaml_groups_in_flow_block_and_scalar_form() {
        let yaml = "App\\Entity\\User:\n    attributes:\n        email:\n            groups: ['user:read', user:write]\n        name:\n            groups:\n                - 'user:read'\n                - admin # a comment\n        id:\n            groups: summary\n";
        assert_eq!(
            groups("file:///p/config/serializer/User.yaml", yaml),
            vec!["user:read", "user:write", "user:read", "admin", "summary"]
        );
    }

    #[test]
    fn xml_groups() {
        let xml = r#"<?xml version="1.0"?>
<serializer>
    <class name="App\Entity\User">
        <attribute name="email">
            <group>user:read</group>
            <!-- <group>commented</group> -->
            <group> admin </group>
        </attribute>
    </class>
</serializer>
"#;
        assert_eq!(
            groups("file:///p/config/serializer/User.xml", xml),
            vec!["user:read", "admin"]
        );
    }

    #[test]
    fn mappings_are_recognised_by_their_location() {
        assert!(is_serializer_mapping_uri(
            "file:///p/config/serializer/User.yaml"
        ));
        assert!(is_serializer_mapping_uri(
            "file:///p/src/Resources/config/serialization.xml"
        ));
        assert!(!is_serializer_mapping_uri("file:///p/config/services.yaml"));
        assert!(!is_serializer_mapping_uri("file:///p/serializer/User.yaml"));
    }
}
