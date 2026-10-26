//! Symfony validation mappings in YAML and XML: the properties a class's
//! constraints are declared on, and the constraints by their short names.
//!
//! A mapped property is a reference to the class's property, so it becomes
//! a member access on the class in the file's resource symbol map, which
//! every member feature already reads.  A constraint written by its short
//! name (`NotBlank`) is one of the Validator's own, so it becomes a class
//! reference to `Symfony\Component\Validator\Constraints\NotBlank`.  One
//! written by its class name is already the class reference every resource
//! file records.
//!
//! The scan reads the mapping as text, by indentation in YAML and by tag in
//! XML.  The constraints an option of another constraint holds (`All`'s,
//! `Sequentially`'s) are left alone in YAML, where telling them from an
//! option's other values would take reading the constraint's options.

use crate::atom::atom;
use crate::symbol_map::{ClassRefContext, DocblockMemberRef, SubjectText, SymbolKind, SymbolSpan};

const CONSTRAINTS_NS: &str = "Symfony\\Component\\Validator\\Constraints\\";

/// Whether `uri` is a validation mapping: a `validation.*` file or one in a
/// `validator` or `validation` directory, under a `config` directory (the
/// application's `config/validator/`, or a bundle's
/// `Resources/config/validation.yaml`).
pub(crate) fn is_validation_mapping_uri(uri: &str) -> bool {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    let mut segments = path.split('/').peekable();
    let mut in_config = false;
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            return in_config && segment.starts_with("validation.");
        }
        if in_config && (segment == "validator" || segment == "validation") {
            return true;
        }
        in_config |= segment == "config";
    }
    false
}

/// The property and constraint spans of one validation mapping.
pub(crate) fn validation_spans(uri: &str, content: &str) -> Vec<SymbolSpan> {
    let path = uri
        .split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .to_ascii_lowercase();
    let path = path.strip_suffix(".dist").unwrap_or(&path);
    let mut spans = Vec::new();
    if path.ends_with(".yaml") || path.ends_with(".yml") {
        scan_yaml(content, &mut spans);
    } else if path.ends_with(".xml") && content.contains("<constraint-mapping") {
        scan_xml(content, &mut spans);
    }
    spans
}

fn property_span(class: &str, property: &str, start: usize) -> SymbolSpan {
    SymbolSpan {
        start: start as u32,
        end: (start + property.len()) as u32,
        kind: SymbolKind::MemberAccess {
            subject_text: SubjectText::owned(class.to_string()),
            member_name: atom(property),
            is_static: false,
            is_method_call: false,
            docblock_ref: DocblockMemberRef::No,
            is_array_callable: false,
            is_nullsafe: false,
        },
    }
}

/// The class reference a constraint written by its short name makes.
fn constraint_span(name: &str, start: usize) -> Option<SymbolSpan> {
    let is_short_name = name
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_uppercase())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    is_short_name.then(|| SymbolSpan {
        start: start as u32,
        end: (start + name.len()) as u32,
        kind: SymbolKind::ClassReference {
            name: atom(&format!("{CONSTRAINTS_NS}{name}")),
            is_fqn: true,
            context: ClassRefContext::Other,
        },
    })
}

fn is_identifier(name: &str) -> bool {
    name.bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

// ─── YAML ───────────────────────────────────────────────────────────────────

/// Where in a class's mapping a line is.
enum YamlPlace {
    /// Directly in the class's mapping.
    Class,
    /// In `properties:` or `getters:`, whose own keys are members.
    Members {
        indent: usize,
        is_property: bool,
        member_indent: Option<usize>,
    },
    /// In one member's constraint list, or in the class's `constraints:`.
    Constraints {
        indent: usize,
        item_indent: Option<usize>,
    },
}

fn scan_yaml(content: &str, spans: &mut Vec<SymbolSpan>) {
    let mut class: Option<String> = None;
    // The sections a line is nested in, innermost last.
    let mut places: Vec<YamlPlace> = Vec::new();
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = crate::symfony::resources::yaml_content_before_comment(line);
        let trimmed = semantic.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = crate::framework::leading_spaces(semantic);

        if indent == 0 {
            places.clear();
            class = crate::symfony::resources::yaml_mapping_key(semantic, line_start).and_then(
                |(raw_key, _)| {
                    let (key, _) = crate::framework::strip_yaml_quotes(raw_key);
                    let class = crate::framework::normalize_framework_fqn(key);
                    class.contains('\\').then_some(class)
                },
            );
            if class.is_some() {
                places.push(YamlPlace::Class);
            }
            continue;
        }
        let Some(class) = &class else {
            continue;
        };
        while places.len() > 1
            && match places.last() {
                Some(YamlPlace::Members { indent: open, .. })
                | Some(YamlPlace::Constraints { indent: open, .. }) => indent <= *open,
                _ => false,
            }
        {
            places.pop();
        }

        match places.last_mut() {
            Some(YamlPlace::Class) => {
                let Some((raw_key, _)) =
                    crate::symfony::resources::yaml_mapping_key(semantic, line_start)
                else {
                    continue;
                };
                match crate::framework::strip_yaml_quotes(raw_key).0 {
                    section @ ("properties" | "getters") => places.push(YamlPlace::Members {
                        indent,
                        is_property: section == "properties",
                        member_indent: None,
                    }),
                    "constraints" => places.push(YamlPlace::Constraints {
                        indent,
                        item_indent: None,
                    }),
                    _ => {}
                }
            }
            Some(YamlPlace::Members {
                is_property,
                member_indent,
                ..
            }) => {
                if *member_indent.get_or_insert(indent) != indent {
                    continue;
                }
                let Some((raw_key, key_start)) =
                    crate::symfony::resources::yaml_mapping_key(semantic, line_start)
                else {
                    continue;
                };
                let (member, (quote, _)) = crate::framework::strip_yaml_quotes(raw_key);
                // A getter is mapped by its property-like name, which the
                // `get`, `is`, or `has` method it reads is named after
                // rather than spelled as.
                if *is_property && is_identifier(member) {
                    spans.push(property_span(class, member, key_start + quote));
                }
                places.push(YamlPlace::Constraints {
                    indent,
                    item_indent: None,
                });
            }
            Some(YamlPlace::Constraints { item_indent, .. }) => {
                if !trimmed.starts_with('-') || *item_indent.get_or_insert(indent) != indent {
                    continue;
                }
                let item = trimmed[1..].trim_start();
                let name = item
                    .split_once(':')
                    .map_or(item, |(name, _)| name)
                    .trim_end();
                let (name, (quote, _)) = crate::framework::strip_yaml_quotes(name);
                let start = line_start + (semantic.len() - semantic.trim_start().len()) + 1;
                let start = start + (trimmed[1..].len() - item.len()) + quote;
                spans.extend(constraint_span(name, start));
            }
            None => {}
        }
    }
}

// ─── XML ────────────────────────────────────────────────────────────────────

fn scan_xml(content: &str, spans: &mut Vec<SymbolSpan>) {
    let bytes = content.as_bytes();
    let mut class: Option<String> = None;
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
        cursor = tag_end;

        let (closing, body) = match tag[1..].strip_prefix('/') {
            Some(body) => (true, body),
            None => (false, &tag[1..]),
        };
        let name = &body[..body
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .unwrap_or(body.len())];
        let attribute = |names: &[&str]| crate::framework::xml_attr_value(tag, tag_start, names);
        match (name, closing) {
            ("class", false) => {
                class = attribute(&["name"])
                    .map(|(name, _, _)| crate::framework::normalize_framework_fqn(&name));
            }
            ("class", true) => class = None,
            ("property", false) => {
                if let Some(class) = &class
                    && let Some((property, start, _)) = attribute(&["name"])
                    && is_identifier(&property)
                {
                    spans.push(property_span(class, &property, start));
                }
            }
            ("constraint", false) => {
                if class.is_some()
                    && let Some((constraint, start, _)) = attribute(&["name"])
                {
                    spans.extend(constraint_span(&constraint, start));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(uri: &str, content: &str) -> Vec<(String, String)> {
        validation_spans(uri, content)
            .into_iter()
            .map(|span| {
                let text = content[span.start as usize..span.end as usize].to_string();
                let what = match span.kind {
                    SymbolKind::MemberAccess {
                        subject_text: SubjectText::Owned(class),
                        member_name,
                        ..
                    } => format!("{class}::${member_name}"),
                    SymbolKind::ClassReference { name, .. } => name.to_string(),
                    other => panic!("unexpected span {other:?}"),
                };
                (what, text)
            })
            .collect()
    }

    #[test]
    fn yaml_maps_properties_and_constraints() {
        let yaml = "App\\Entity\\User:\n    constraints:\n        - Callback: validate\n    properties:\n        email:\n            - NotBlank: ~\n            - Email:\n                message: x\n                groups:\n                    - Default\n        'displayName':\n            - App\\Validator\\Custom: ~\n            - Length: { min: 2 }\n    getters:\n        passwordSafe:\n            - IsTrue: ~\n";
        let found = found("file:///p/config/validator/User.yaml", yaml);
        let c = |name: &str| format!("{CONSTRAINTS_NS}{name}");
        assert_eq!(
            found
                .iter()
                .map(|(what, _)| what.clone())
                .collect::<Vec<_>>(),
            vec![
                c("Callback"),
                "App\\Entity\\User::$email".to_string(),
                c("NotBlank"),
                c("Email"),
                "App\\Entity\\User::$displayName".to_string(),
                c("Length"),
                c("IsTrue"),
            ]
        );
        for (what, text) in &found {
            assert!(
                what.ends_with(text.as_str()) || what.ends_with(&format!("${text}")),
                "{what} covers {text}"
            );
        }
    }

    #[test]
    fn xml_maps_properties_and_constraints() {
        let xml = r#"<?xml version="1.0"?>
<constraint-mapping>
    <class name="App\Entity\User">
        <constraint name="Callback">validate</constraint>
        <property name="email">
            <constraint name="NotBlank"/>
            <constraint name="App\Validator\Custom"/>
        </property>
    </class>
    <!-- <property name="commented"/> -->
</constraint-mapping>
"#;
        let found = found("file:///p/config/validator/User.xml", xml);
        assert_eq!(
            found,
            vec![
                (format!("{CONSTRAINTS_NS}Callback"), "Callback".to_string()),
                ("App\\Entity\\User::$email".to_string(), "email".to_string()),
                (format!("{CONSTRAINTS_NS}NotBlank"), "NotBlank".to_string()),
            ]
        );
    }

    #[test]
    fn mappings_are_recognised_by_their_location() {
        assert!(is_validation_mapping_uri(
            "file:///p/config/validator/User.yaml"
        ));
        assert!(is_validation_mapping_uri(
            "file:///p/src/Resources/config/validation.xml"
        ));
        assert!(is_validation_mapping_uri(
            "file:///p/src/Resources/config/validation/User.yaml"
        ));
        assert!(!is_validation_mapping_uri("file:///p/config/services.yaml"));
        assert!(!is_validation_mapping_uri("file:///p/validator/User.yaml"));
    }
}
