//! The names a Drupal routing file's route defaults read besides route
//! names: the entity type an `_entity_form`, `_entity_list`, or
//! `_entity_view` works on, and the service a `service:method` controller
//! or title callback calls.

use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan};

/// The entity type and service spans of a routing file, in source order.
pub(crate) fn routing_value_spans(content: &str) -> Vec<SymbolSpan> {
    let mut spans = Vec::new();
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = crate::symfony::resources::yaml_content_before_comment(line);
        let indent = crate::framework::leading_spaces(semantic);
        if indent == 0 {
            continue;
        }
        let text = &semantic[indent..];
        let Some(colon) = text.find(':') else {
            continue;
        };
        let key = text[..colon].trim_end();
        let raw = &text[colon + 1..];
        let trimmed = raw.trim_start();
        let (value, (quote, _)) = crate::framework::strip_yaml_quotes(trimmed.trim_end());
        let value_start = line_start + indent + colon + 1 + (raw.len() - trimmed.len()) + quote;
        let found = match key {
            // `node.edit` is the form `edit` of the entity type `node`, and
            // `node.full` its view mode `full`.
            "_entity_form" | "_entity_view" | "_entity_list" => value
                .split('.')
                .next()
                .map(|id| (StringKeyKind::DrupalEntityType, id)),
            // `acme.greeter:greet` calls `greet()` on the service
            // `acme.greeter`; a `Class::method` names a class instead.
            "_controller" | "_title_callback" if !value.contains("::") => value
                .split_once(':')
                .map(|(service, _)| (StringKeyKind::ServiceId, service)),
            _ => None,
        };
        let Some((kind, name)) = found.filter(|(_, name)| is_plain_name(name)) else {
            continue;
        };
        spans.push(SymbolSpan {
            start: value_start as u32,
            end: (value_start + name.len()) as u32,
            kind: SymbolKind::StringKey {
                kind,
                key: name.to_string(),
                is_write: false,
                is_optional: false,
            },
        });
    }
    spans
}

/// Whether `name` can be an entity type ID or a service ID.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_and_service_defaults_read_their_names() {
        let yaml = "a:\n  path: '/a'\n  defaults:\n    _entity_form: 'node.edit'\n    _entity_view: node.full\n    _entity_list: node_type\n    _controller: acme.greeter:greet\n    _title_callback: '\\Drupal\\acme\\Title::get'\n  requirements:\n    _entity_access: node.update\n";
        let found: Vec<(&str, StringKeyKind)> = routing_value_spans(yaml)
            .iter()
            .map(|span| {
                let SymbolKind::StringKey { kind, .. } = span.kind else {
                    unreachable!()
                };
                (&yaml[span.start as usize..span.end as usize], kind)
            })
            .collect();
        assert_eq!(
            found,
            [
                ("node", StringKeyKind::DrupalEntityType),
                ("node", StringKeyKind::DrupalEntityType),
                ("node_type", StringKeyKind::DrupalEntityType),
                ("acme.greeter", StringKeyKind::ServiceId),
            ]
        );
    }
}
