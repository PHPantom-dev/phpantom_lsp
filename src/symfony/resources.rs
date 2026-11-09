//! Service IDs and container parameters in Symfony's YAML and XML
//! configuration.
//!
//! The scan reads the configuration as text, without a YAML or XML parser:
//! a key under `services:` or `parameters:` declares one, and an `@id`
//! reference, a `%name%` placeholder, or one of the XML attributes that
//! name a service reads one.  Each becomes a
//! [`StringKey`](SymbolKind::StringKey) span in the file's resource symbol
//! map, which every string-key feature already reads.
//!
//! A service ID spelled as a class name is left to the class reference the
//! resource symbol map already records over the same text.
//!
//! A declaration also records what it gives the name, when the text says:
//! a service's `class` or the service an alias points at, and the value a
//! parameter is set to (see [`super::container`]).

use crate::symbol_map::{
    ContainerValue, ContainerValueSite, ServiceDecoration, StringKeyKind, SymbolKind, SymbolSpan,
};

/// Whether `uri` is a file Symfony loads container configuration from: a
/// YAML or XML file under a `config` directory (the application's
/// `config/`, or a bundle's `Resources/config/`).
///
/// Other YAML files have keys of the same name with other meanings: a
/// `docker-compose.yml` has `services:` too.
pub(crate) fn is_container_config_uri(uri: &str) -> bool {
    uri.split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .split('/')
        .any(|segment| segment == "config")
}

/// What one YAML or XML configuration file declares.
#[derive(Default)]
pub(crate) struct ContainerDeclarations {
    /// The service-ID and container-parameter spans, sorted by offset.
    pub spans: Vec<SymbolSpan>,
    /// What the declarations give the names they declare, sorted the same
    /// way.
    pub values: Vec<ContainerValueSite>,
    /// The services that decorate another, in source order.
    pub decorations: Vec<ServiceDecoration>,
}

/// What one YAML or XML configuration file declares.
pub(crate) fn container_declarations(uri: &str, content: &str) -> ContainerDeclarations {
    let mut out = ContainerDeclarations::default();
    match super::config_format(uri) {
        Some(super::ConfigFormat::Yaml) => scan_yaml(content, &mut out),
        Some(super::ConfigFormat::Xml) => scan_xml(content, &mut out),
        None => {}
    }
    out.spans.sort_by_key(|span| (span.start, span.end));
    out.spans
        .dedup_by(|b, a| a.start == b.start && a.end == b.end);
    out.values.sort_by_key(|site| site.start);
    out
}

fn value_site(start: usize, value: ContainerValue) -> ContainerValueSite {
    ContainerValueSite {
        start: start as u32,
        value,
    }
}

fn key_span(kind: StringKeyKind, key: &str, start: usize, is_write: bool) -> SymbolSpan {
    SymbolSpan {
        start: start as u32,
        end: (start + key.len()) as u32,
        kind: SymbolKind::StringKey {
            kind,
            key: key.to_string(),
            is_write,
            is_optional: false,
        },
    }
}

/// Whether `name` can be a service ID or parameter name, and is not a class
/// name (which the class reference over the same text answers for).
fn is_container_key(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

// ─── YAML ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum YamlSection {
    Services,
    Parameters,
}

struct OpenSection {
    kind: YamlSection,
    indent: usize,
    /// The indentation of the section's own entries, from the first one.
    child_indent: Option<usize>,
}

fn yaml_section(trimmed: &str) -> Option<YamlSection> {
    let key = trimmed.strip_suffix(':')?;
    let key = key
        .strip_prefix(['\'', '"'])
        .and_then(|key| key.strip_suffix(['\'', '"']))
        .unwrap_or(key);
    match key {
        "services" => Some(YamlSection::Services),
        "parameters" => Some(YamlSection::Parameters),
        _ => None,
    }
}

/// A declaration whose value is written on the lines nested under it: a
/// service's `class:`, `alias:`, or `decorates:`, or a parameter's block of
/// entries.
struct OpenDeclaration {
    /// Where the declared name starts.
    start: usize,
    kind: YamlSection,
    /// The indentation of the service's own keys, from the first one.
    indent: Option<usize>,
    /// The service's ID, a class name when it is spelled as one.
    id: crate::atom::Atom,
    /// Whether the name is a span of its own, which what the declaration
    /// gives is keyed by; a service ID spelled as a class is not.
    keyed: bool,
}

fn scan_yaml(content: &str, out: &mut ContainerDeclarations) {
    let ContainerDeclarations {
        spans,
        values,
        decorations,
    } = out;
    let mut section: Option<OpenSection> = None;
    let mut declaration: Option<OpenDeclaration> = None;
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = yaml_content_before_comment(line);
        let trimmed = semantic.trim();
        if trimmed.is_empty() {
            continue;
        }
        // A placeholder means the same thing anywhere in the configuration:
        // `secret: '%env(APP_SECRET)%'` under `framework:` reads one too.
        scan_parameter_placeholders(semantic, line_start, spans);

        let indent = crate::framework::leading_spaces(semantic);
        let child_indent = section.as_ref().and_then(|open| open.child_indent);
        if let Some(open) = declaration.take() {
            let nested = child_indent.is_some_and(|child| indent > child)
                && section
                    .as_ref()
                    .is_some_and(|section| indent > section.indent);
            match (open.kind, nested) {
                // A parameter with nothing after its colon is the block of
                // entries nested under it, or null when nothing is.
                (YamlSection::Parameters, nested) => {
                    let ty = if nested {
                        crate::php_type::PhpType::array()
                    } else {
                        crate::php_type::PhpType::null()
                    };
                    values.push(value_site(open.start, ContainerValue::Type(ty)));
                }
                (YamlSection::Services, true) => {
                    declaration = Some(service_property(
                        open,
                        semantic,
                        line_start,
                        indent,
                        ServiceOutput {
                            spans: &mut *spans,
                            values: &mut *values,
                            decorations: &mut *decorations,
                        },
                    ));
                }
                (YamlSection::Services, false) => {}
            }
        }

        if let Some(kind) = yaml_section(trimmed) {
            section = Some(OpenSection {
                kind,
                indent,
                child_indent: None,
            });
            continue;
        }
        if section.as_ref().is_some_and(|open| indent <= open.indent) {
            section = None;
        }
        let Some(open) = section.as_mut() else {
            continue;
        };
        let child_indent = *open.child_indent.get_or_insert(indent);

        if indent == child_indent
            && !trimmed.starts_with('-')
            && let Some((raw_key, key_start)) = yaml_mapping_key(semantic, line_start)
        {
            let (key, (quote, _)) = crate::framework::strip_yaml_quotes(raw_key);
            // `_defaults` and `_instanceof` configure the section rather
            // than declare a service.
            let declares = match open.kind {
                YamlSection::Services => !key.starts_with('_'),
                YamlSection::Parameters => true,
            };
            let keyed = is_container_key(key);
            let id = match open.kind {
                _ if !declares => None,
                _ if keyed => Some(crate::atom::atom(key)),
                YamlSection::Services => yaml_class_name(key),
                YamlSection::Parameters => None,
            };
            if let Some(id) = id {
                let start = key_start + quote;
                if keyed {
                    let kind = match open.kind {
                        YamlSection::Services => StringKeyKind::ServiceId,
                        YamlSection::Parameters => StringKeyKind::ContainerParameter,
                    };
                    spans.push(key_span(kind, key, start, true));
                }
                let after_colon = &semantic[key_start - line_start + raw_key.len()..];
                let inline = after_colon
                    .trim_start()
                    .strip_prefix(':')
                    .unwrap_or("")
                    .trim();
                let open = OpenDeclaration {
                    start,
                    kind: open.kind,
                    indent: None,
                    id,
                    keyed,
                };
                let inline_start = line_start + offset_within(semantic, inline);
                declaration = declared_inline(
                    open,
                    inline,
                    inline_start,
                    ServiceOutput {
                        spans: &mut *spans,
                        values: &mut *values,
                        decorations: &mut *decorations,
                    },
                );
            }
        }
        if open.kind == YamlSection::Services {
            scan_service_references(semantic, line_start, spans);
        }
    }
    if let Some(open) = declaration
        && open.kind == YamlSection::Parameters
    {
        values.push(value_site(
            open.start,
            ContainerValue::Type(crate::php_type::PhpType::null()),
        ));
    }
}

/// Where what a service declaration's properties say is recorded.
struct ServiceOutput<'a> {
    spans: &'a mut Vec<SymbolSpan>,
    values: &'a mut Vec<ContainerValueSite>,
    decorations: &'a mut Vec<ServiceDecoration>,
}

/// Record what a declaration written on its key's own line, as `inline`
/// starting at `inline_start`, gives the name, or open the declaration
/// when it is written on the lines nested under it.
fn declared_inline(
    open: OpenDeclaration,
    inline: &str,
    inline_start: usize,
    mut out: ServiceOutput<'_>,
) -> Option<OpenDeclaration> {
    match open.kind {
        YamlSection::Parameters if inline.is_empty() => return Some(open),
        YamlSection::Parameters => {
            if let Some(value) = super::container::yaml_scalar_value(inline) {
                out.values.push(value_site(open.start, value));
            }
        }
        YamlSection::Services if inline.is_empty() || inline == "~" => return Some(open),
        YamlSection::Services => {
            if let Some(body) = inline.strip_prefix('{') {
                let body = body.strip_suffix('}').unwrap_or(body);
                for pair in body.split(',') {
                    if let Some((key, value)) = pair.split_once(':') {
                        let value = value.trim();
                        let value_start = inline_start + offset_within(inline, value);
                        push_service_property(&open, key.trim(), value, value_start, &mut out);
                    }
                }
            } else if open.keyed
                && let Some(target) = service_reference(inline)
            {
                out.values.push(value_site(
                    open.start,
                    ContainerValue::Alias(crate::atom::atom(target)),
                ));
            }
        }
    }
    None
}

/// Read one line nested under a service declaration, starting at
/// `line_start`, recording the `class:`, `alias:`, or `decorates:` at the
/// service's own level.
fn service_property(
    mut open: OpenDeclaration,
    line: &str,
    line_start: usize,
    indent: usize,
    mut out: ServiceOutput<'_>,
) -> OpenDeclaration {
    let at = *open.indent.get_or_insert(indent);
    if indent == at
        && let Some((key, value)) = line.trim().split_once(':')
    {
        let value = value.trim();
        let value_start = line_start + offset_within(line, value);
        push_service_property(&open, key.trim(), value, value_start, &mut out);
    }
    open
}

/// Record a service's `class`, `alias`, or `decorates` property, whose
/// value starts at `value_start`.
fn push_service_property(
    open: &OpenDeclaration,
    key: &str,
    raw_value: &str,
    value_start: usize,
    out: &mut ServiceOutput<'_>,
) {
    let (key, _) = crate::framework::strip_yaml_quotes(key);
    let (value, (quote, _)) = crate::framework::strip_yaml_quotes(raw_value);
    match key {
        "class" if open.keyed && !value.is_empty() => {
            let class = value.strip_prefix('\\').unwrap_or(value);
            out.values.push(value_site(
                open.start,
                ContainerValue::Class(crate::atom::atom(&class.replace("\\\\", "\\"))),
            ));
        }
        "alias" if open.keyed && is_container_key(value) => {
            out.values.push(value_site(
                open.start,
                ContainerValue::Alias(crate::atom::atom(value)),
            ));
        }
        "decorates" => {
            let decorated = if is_container_key(value) {
                out.spans.push(key_span(
                    StringKeyKind::ServiceId,
                    value,
                    value_start + quote,
                    false,
                ));
                crate::atom::atom(value)
            } else if let Some(class) = yaml_class_name(value) {
                class
            } else {
                return;
            };
            out.decorations.push(ServiceDecoration {
                decorator: open.id,
                decorated,
            });
        }
        _ => {}
    }
}

/// The class a YAML service ID or `decorates:` value spelled as one names,
/// without the leading `\`.  A double-quoted value escapes each `\`.
fn yaml_class_name(text: &str) -> Option<crate::atom::Atom> {
    let unescaped;
    let text = if text.contains("\\\\") {
        unescaped = text.replace("\\\\", "\\");
        unescaped.as_str()
    } else {
        text
    };
    let class = text.strip_prefix('\\').unwrap_or(text);
    (class.contains('\\') && super::container::is_class_name(class))
        .then(|| crate::atom::atom(class))
}

/// The service an XML `id` or `decorates` attribute names, a class name
/// without the leading `\` when it is spelled as one.
fn xml_service_id(text: &str) -> Option<crate::atom::Atom> {
    if is_container_key(text) {
        return Some(crate::atom::atom(text));
    }
    let class = text.strip_prefix('\\').unwrap_or(text);
    (class.contains('\\') && super::container::is_class_name(class))
        .then(|| crate::atom::atom(class))
}

/// The offset of `part`, a slice of `whole`, within it.
fn offset_within(whole: &str, part: &str) -> usize {
    part.as_ptr() as usize - whole.as_ptr() as usize
}

/// The service a `'@id'` value names, the whole of a service's declaration
/// when it is an alias.
fn service_reference(value: &str) -> Option<&str> {
    let (value, _) = crate::framework::strip_yaml_quotes(value);
    let id = value.strip_prefix('@')?;
    is_container_key(id).then_some(id)
}

/// The key of a `key: value` line and its absolute offset.
pub(super) fn yaml_mapping_key(line: &str, line_start: usize) -> Option<(&str, usize)> {
    let indent = crate::framework::leading_spaces(line);
    let rest = &line[indent..];
    let colon = rest.find(':')?;
    let raw_key = rest[..colon].trim_end();
    (!raw_key.is_empty()).then_some((raw_key, line_start + indent))
}

/// `line` without a trailing `# comment`, leaving a `#` inside quotes.
pub(crate) fn yaml_content_before_comment(line: &str) -> &str {
    let mut quote = None;
    for (index, byte) in line.bytes().enumerate() {
        match (quote, byte) {
            (None, b'\'' | b'"') => quote = Some(byte),
            (Some(open), _) if byte == open => quote = None,
            (None, b'#') if index == 0 || line.as_bytes()[index - 1].is_ascii_whitespace() => {
                return &line[..index];
            }
            _ => {}
        }
    }
    line
}

/// Each `@id` service reference in a line of the `services:` section.
///
/// The `@` has to open a value, so the domain of an e-mail address in a
/// string is not read as one.  `@?id` and `@!id` reference a service
/// optionally or lazily, `@@` escapes a literal `@`, and `@=` starts an
/// expression.
fn scan_service_references(text: &str, absolute_start: usize, spans: &mut Vec<SymbolSpan>) {
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    while let Some(relative) = memchr::memchr(b'@', &bytes[cursor..]) {
        let at = cursor + relative;
        cursor = at + 1;
        if at > 0
            && !matches!(
                bytes[at - 1],
                b' ' | b'\'' | b'"' | b'[' | b',' | b'{' | b'\t'
            )
        {
            continue;
        }
        let mut start = at + 1;
        if matches!(bytes.get(start), Some(b'?' | b'!')) {
            start += 1;
        }
        let mut end = start;
        while bytes
            .get(end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        {
            end += 1;
        }
        // A trailing `.` ends a sentence, not an ID.
        while end > start && bytes[end - 1] == b'.' {
            end -= 1;
        }
        cursor = end.max(cursor);
        // A name running on into `\` is a class name.
        if end == start || bytes.get(end) == Some(&b'\\') {
            continue;
        }
        spans.push(key_span(
            StringKeyKind::ServiceId,
            &text[start..end],
            absolute_start + start,
            false,
        ));
    }
}

/// Each `%name%` parameter placeholder in `text`.
fn scan_parameter_placeholders(text: &str, absolute_start: usize, spans: &mut Vec<SymbolSpan>) {
    for_each_parameter_placeholder(text, |name, start| {
        spans.push(key_span(
            StringKeyKind::ContainerParameter,
            name,
            absolute_start + start,
            false,
        ));
    });
}

/// Call `found` with each `%name%` parameter placeholder in `text` and the
/// offset of its name.  `%%` is an escaped percent sign, and `%env(…)%`
/// reads an environment variable.
pub(crate) fn for_each_parameter_placeholder(text: &str, mut found: impl FnMut(&str, usize)) {
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    while let Some(relative) = memchr::memchr(b'%', &bytes[cursor..]) {
        let open = cursor + relative;
        let Some(close_relative) = memchr::memchr(b'%', &bytes[open + 1..]) else {
            break;
        };
        let close = open + 1 + close_relative;
        let name = &text[open + 1..close];
        if is_container_key(name) {
            found(name, open + 1);
            cursor = close + 1;
        } else {
            // `50% of the 100%` pairs the wrong signs: try the closing one
            // as an opening one instead.
            cursor = if name.is_empty() { close + 1 } else { close };
        }
    }
}

// ─── XML ────────────────────────────────────────────────────────────────────

fn scan_xml(content: &str, out: &mut ContainerDeclarations) {
    let ContainerDeclarations {
        spans,
        values,
        decorations,
    } = out;
    if !content.contains("<service")
        && !content.contains("<parameter")
        && !content.contains("<argument")
    {
        return;
    }

    let bytes = content.as_bytes();
    // How deep inside `<parameter>` elements the scan is: only a parameter
    // directly under `<parameters>` declares one, and a nested one is an
    // entry of a collection.
    let mut parameter_depth = 0usize;
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
        let tag = &content[tag_start..tag_end];
        let text_end = memchr::memchr(b'<', &bytes[tag_end..]).map_or(bytes.len(), |n| tag_end + n);
        cursor = tag_end;

        let (closing, body) = match tag[1..].strip_prefix('/') {
            Some(body) => (true, body),
            None => (false, &tag[1..]),
        };
        let name = &body[..body
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .unwrap_or(body.len())];
        if closing {
            if name == "parameter" {
                parameter_depth = parameter_depth.saturating_sub(1);
            }
            continue;
        }
        let self_closing = tag.ends_with("/>");
        if name == "parameter" {
            if parameter_depth == 0
                && let Some((key, start, _)) =
                    crate::framework::xml_attr_value(tag, tag_start, &["key"])
                && is_container_key(&key)
            {
                spans.push(key_span(
                    StringKeyKind::ContainerParameter,
                    &key,
                    start,
                    true,
                ));
                let type_attribute = crate::framework::xml_attr_value(tag, tag_start, &["type"]);
                let text = if self_closing {
                    ""
                } else {
                    &content[tag_end..text_end]
                };
                if let Some(value) = super::container::xml_parameter_value(
                    text,
                    type_attribute.as_ref().map(|(value, _, _)| value.as_str()),
                ) {
                    values.push(value_site(start, value));
                }
            }
            if !self_closing {
                parameter_depth += 1;
            }
        }

        let attribute = |names: &[&str]| crate::framework::xml_attr_value(tag, tag_start, names);
        let service = |found: Option<(String, usize, usize)>, is_write, spans: &mut Vec<_>| {
            if let Some((id, start, _)) = found
                && is_container_key(&id)
            {
                spans.push(key_span(StringKeyKind::ServiceId, &id, start, is_write));
            }
        };
        match name {
            "service" => {
                if let Some((id, start, _)) = attribute(&["id"])
                    && is_container_key(&id)
                {
                    if let Some((class, _, _)) = attribute(&["class"]) {
                        let class = class.strip_prefix('\\').unwrap_or(&class);
                        values.push(value_site(
                            start,
                            ContainerValue::Class(crate::atom::atom(class)),
                        ));
                    } else if let Some((target, _, _)) = attribute(&["alias"])
                        && is_container_key(&target)
                    {
                        values.push(value_site(
                            start,
                            ContainerValue::Alias(crate::atom::atom(&target)),
                        ));
                    }
                }
                if let Some((decorated, _, _)) = attribute(&["decorates"])
                    && let Some((id, _, _)) = attribute(&["id"])
                    && let Some(decorator) = xml_service_id(&id)
                    && let Some(decorated) = xml_service_id(&decorated)
                {
                    decorations.push(ServiceDecoration {
                        decorator,
                        decorated,
                    });
                }
                service(attribute(&["id"]), true, spans);
                for name in ["alias", "decorates", "parent"] {
                    service(attribute(&[name]), false, spans);
                }
            }
            "argument" | "property" => {
                if attribute(&["type"]).is_some_and(|(value, _, _)| value == "service") {
                    service(attribute(&["id", "service"]), false, spans);
                }
            }
            "factory" | "configurator" => service(attribute(&["service"]), false, spans),
            _ => {}
        }

        scan_parameter_placeholders(tag, tag_start, spans);
        if !self_closing {
            scan_parameter_placeholders(&content[tag_end..text_end], tag_end, spans);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(uri: &str, content: &str) -> Vec<(StringKeyKind, String, bool, String)> {
        container_declarations(uri, content)
            .spans
            .into_iter()
            .map(|span| {
                let SymbolKind::StringKey {
                    kind,
                    key,
                    is_write,
                    ..
                } = span.kind
                else {
                    unreachable!()
                };
                let text = content[span.start as usize..span.end as usize].to_string();
                (kind, key, is_write, text)
            })
            .collect()
    }

    fn named(found: &[(StringKeyKind, String, bool, String)]) -> Vec<(StringKeyKind, &str, bool)> {
        found
            .iter()
            .inspect(|(_, key, _, text)| assert_eq!(key, text, "span must cover the key"))
            .map(|(kind, key, is_write, _)| (*kind, key.as_str(), *is_write))
            .collect()
    }

    use StringKeyKind::{ContainerParameter as Param, ServiceId as Service};

    #[test]
    fn yaml_declares_services_and_parameters_and_reads_references() {
        let yaml = "parameters:\n    app.sender: 'PHPantom'\n    'app.quoted': x\nservices:\n    _defaults:\n        autowire: true\n    App\\Service\\Mailer: ~\n    app.mailer:\n        class: App\\Service\\Mailer\n        arguments: ['@app.transport', '%app.sender%', 'me@example.com']\n        calls:\n            - [setLogger, ['@?logger']]\n    app.mailer_alias: '@app.mailer' # the alias\n";
        let found = keys("file:///p/config/services.yaml", yaml);
        assert_eq!(
            named(&found),
            vec![
                (Param, "app.sender", true),
                (Param, "app.quoted", true),
                (Service, "app.mailer", true),
                (Service, "app.transport", false),
                (Param, "app.sender", false),
                (Service, "logger", false),
                (Service, "app.mailer_alias", true),
                (Service, "app.mailer", false),
            ]
        );
    }

    #[test]
    fn yaml_outside_a_container_section_reads_placeholders_only() {
        let yaml = "framework:\n    secret: '%env(APP_SECRET)%'\n    cache_dir: '%kernel.cache_dir%/app'\n    ratio: '50% of 100%'\n    escaped: '%%literal%%'\n";
        let found = keys("file:///p/config/packages/framework.yaml", yaml);
        assert_eq!(named(&found), vec![(Param, "kernel.cache_dir", false)]);
    }

    #[test]
    fn yaml_nested_sections_end_where_their_indentation_does() {
        let yaml = "when@dev:\n    services:\n        app.debug_mailer: ~\n    parameters:\n        app.debug: true\nother:\n    app.not_a_service: ~\n";
        let found = keys("file:///p/config/services.yaml", yaml);
        assert_eq!(
            named(&found),
            vec![
                (Service, "app.debug_mailer", true),
                (Param, "app.debug", true)
            ]
        );
    }

    #[test]
    fn xml_declares_services_and_parameters_and_reads_references() {
        let xml = r#"<?xml version="1.0"?>
<container>
  <parameters>
    <parameter key="app.sender">PHPantom</parameter>
    <parameter key="app.locales" type="collection">
      <parameter key="en">English</parameter>
    </parameter>
  </parameters>
  <!-- <service id="app.commented"/> -->
  <services>
    <service id="app.mailer" class="App\Service\Mailer">
      <argument type="service" id="app.transport"/>
      <argument>%app.sender%</argument>
      <factory service="app.factory" method="create"/>
    </service>
    <service id="app.mailer_alias" alias="app.mailer"/>
    <service id="App\Service\Other"/>
  </services>
</container>
"#;
        let found = keys("file:///p/config/services.xml", xml);
        assert_eq!(
            named(&found),
            vec![
                (Param, "app.sender", true),
                (Param, "app.locales", true),
                (Service, "app.mailer", true),
                (Service, "app.transport", false),
                (Param, "app.sender", false),
                (Service, "app.factory", false),
                (Service, "app.mailer_alias", true),
                (Service, "app.mailer", false),
            ]
        );
    }

    /// What each declaration gives, by the name it declares.
    fn values(uri: &str, content: &str) -> Vec<(String, String)> {
        let ContainerDeclarations { spans, values, .. } = container_declarations(uri, content);
        values
            .into_iter()
            .map(|site| {
                let span = spans
                    .iter()
                    .find(|span| span.start == site.start)
                    .expect("a value belongs to a declaration");
                let name = content[span.start as usize..span.end as usize].to_string();
                let value = match site.value {
                    ContainerValue::Class(class) => format!("class {class}"),
                    ContainerValue::Alias(id) => format!("alias {id}"),
                    ContainerValue::Parent(id) => format!("parent {id}"),
                    ContainerValue::Type(ty) => ty.to_string(),
                };
                (name, value)
            })
            .collect()
    }

    fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
        expected
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn yaml_declarations_give_classes_aliases_and_values() {
        let yaml = "parameters:\n    app.name: PHPantom\n    app.port: 25\n    app.list:\n        - a\n    app.empty:\n    app.class: 'App\\Mailer'\nservices:\n    app.mailer:\n        arguments: ['@app.transport']\n        class: App\\Mailer\n    app.alias: '@app.mailer'\n    app.long_alias:\n        alias: app.mailer\n    app.flow: { class: App\\Flow, public: true }\n    app.bare: ~\nwhen@dev:\n    parameters:\n        app.last:\n";
        assert_eq!(
            values("file:///p/config/services.yaml", yaml),
            pairs(&[
                ("app.name", "string"),
                ("app.port", "int"),
                ("app.list", "array"),
                ("app.empty", "null"),
                ("app.class", "class App\\Mailer"),
                ("app.mailer", "class App\\Mailer"),
                ("app.alias", "alias app.mailer"),
                ("app.long_alias", "alias app.mailer"),
                ("app.flow", "class App\\Flow"),
                ("app.last", "null"),
            ])
        );
    }

    #[test]
    fn xml_declarations_give_classes_aliases_and_values() {
        let xml = r#"<container>
  <parameters>
    <parameter key="app.count">3</parameter>
    <parameter key="app.label" type="string">3</parameter>
    <parameter key="app.list" type="collection">
      <parameter>a</parameter>
    </parameter>
  </parameters>
  <services>
    <service id="app.mailer" class="App\Mailer"/>
    <service id="app.alias" alias="app.mailer"/>
  </services>
</container>
"#;
        assert_eq!(
            values("file:///p/config/services.xml", xml),
            pairs(&[
                ("app.count", "int"),
                ("app.label", "string"),
                ("app.list", "array"),
                ("app.mailer", "class App\\Mailer"),
                ("app.alias", "alias app.mailer"),
            ])
        );
    }

    fn decorations(uri: &str, content: &str) -> Vec<(String, String)> {
        container_declarations(uri, content)
            .decorations
            .into_iter()
            .map(|found| (found.decorator.to_string(), found.decorated.to_string()))
            .collect()
    }

    #[test]
    fn yaml_and_xml_services_record_what_they_decorate() {
        let yaml = "services:\n    app.cached:\n        class: App\\Cached\n        decorates: app.mailer\n    App\\Logging:\n        decorates: 'App\\Mailer'\n    \"App\\\\Quoted\": { decorates: \"App\\\\Mailer\" }\n    app.plain:\n        class: App\\Plain\n";
        assert_eq!(
            decorations("file:///p/config/services.yaml", yaml),
            pairs(&[
                ("app.cached", "app.mailer"),
                ("App\\Logging", "App\\Mailer"),
                ("App\\Quoted", "App\\Mailer"),
            ])
        );
        // The decorated ID is a reference to the service it names, and a
        // service ID spelled as a class gives nothing a span would key.
        assert_eq!(
            named(&keys("file:///p/config/services.yaml", yaml)),
            vec![
                (Service, "app.cached", true),
                (Service, "app.mailer", false),
                (Service, "app.plain", true),
            ]
        );
        assert_eq!(
            values("file:///p/config/services.yaml", yaml),
            pairs(&[
                ("app.cached", "class App\\Cached"),
                ("app.plain", "class App\\Plain")
            ])
        );

        let xml = r#"<container><services>
    <service id="app.cached" class="App\Cached" decorates="app.mailer"/>
    <service id="App\Logging" decorates="App\Mailer"/>
</services></container>
"#;
        assert_eq!(
            decorations("file:///p/config/services.xml", xml),
            pairs(&[
                ("app.cached", "app.mailer"),
                ("App\\Logging", "App\\Mailer")
            ])
        );
    }

    #[test]
    fn only_configuration_directories_hold_container_configuration() {
        assert!(is_container_config_uri("file:///p/config/services.yaml"));
        assert!(is_container_config_uri(
            "file:///p/src/Resources/config/services.xml"
        ));
        assert!(!is_container_config_uri("file:///p/docker-compose.yml"));
    }
}
