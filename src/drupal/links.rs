//! The route names a Drupal links file reads.
//!
//! A menu, task, action, or contextual links file declares one link per
//! top-level key, and places it by route: the link's `route_name`, the
//! `base_route` a task is grouped under, and the routes an action
//! `appears_on`.  A `parent` or `parent_id` names another link, not a
//! route.  The keys of a link's `route_parameters` are parameters of the
//! route its `route_name` names.

use super::routing::{entries, lines, route_parameter_key, scalar};
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan};
use crate::symfony::routes::is_drupal_route_name;

/// The keys of a link whose value is a route name.
fn names_route(key: &str) -> bool {
    matches!(key, "route_name" | "base_route")
}

/// The route-name spans of a links file, sorted by offset.
pub(crate) fn links_route_spans(content: &str) -> Vec<SymbolSpan> {
    let mut spans = Vec::new();
    // The indentation of the `appears_on:` key whose list the scan is in.
    let mut appears_on: Option<usize> = None;
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = crate::symfony::resources::yaml_content_before_comment(line);
        if semantic.trim().is_empty() {
            continue;
        }
        let indent = crate::framework::leading_spaces(semantic);
        let text = &semantic[indent..];
        // YAML lets a block list's items sit at its key's indentation.
        if let Some(list) = appears_on {
            if indent >= list
                && let Some(item) = text.strip_prefix('-')
            {
                let item_start = line_start + indent + 1;
                push_route(item, item_start, &mut spans);
                continue;
            }
            appears_on = None;
        }
        if indent == 0 {
            continue;
        }
        let Some(colon) = text.find(':') else {
            continue;
        };
        let key = text[..colon].trim_end();
        let value = &text[colon + 1..];
        let value_start = line_start + indent + colon + 1;
        if names_route(key) {
            push_route(value, value_start, &mut spans);
        } else if key == "appears_on" {
            match value.trim().strip_prefix('[') {
                Some(_) => push_flow_routes(value, value_start, &mut spans),
                None => appears_on = Some(indent),
            }
        }
    }
    spans
}

/// The spans of the route parameters a links file passes: each key of a
/// link's `route_parameters`, as a parameter of its `route_name`, which
/// may come before or after it.
pub(crate) fn links_parameter_spans(content: &str) -> Vec<SymbolSpan> {
    let lines = lines(content);
    let mut spans = Vec::new();
    for (_, link) in entries(&lines) {
        let mut route = None;
        let mut keys: Vec<(String, usize)> = Vec::new();
        for (child, nested) in entries(link) {
            match child.key {
                "route_name" => route = Some(scalar(child.value, child.value_start).0),
                "route_parameters" if nested.is_empty() => {
                    keys.extend(flow_keys(child.value, child.value_start));
                }
                "route_parameters" => keys.extend(
                    entries(nested)
                        .into_iter()
                        .map(|(parameter, _)| (parameter.key.to_string(), parameter.key_start)),
                ),
                _ => {}
            }
        }
        let Some(route) = route.filter(|route| is_drupal_route_name(route)) else {
            continue;
        };
        for (key, start) in keys {
            spans.push(SymbolSpan {
                start: start as u32,
                end: (start + key.len()) as u32,
                kind: SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalRouteParameter,
                    key: route_parameter_key(&route, &key),
                    is_write: false,
                    is_optional: false,
                },
            });
        }
    }
    spans
}

/// The keys of a `{ a: 1, 'b': 2 }` flow mapping written at `start`, each
/// with the absolute offset of its text.
fn flow_keys(raw: &str, start: usize) -> Vec<(String, usize)> {
    let Some(open) = raw.find('{') else {
        return Vec::new();
    };
    let close = raw.rfind('}').unwrap_or(raw.len()).max(open + 1);
    let mut keys = Vec::new();
    let mut pair_start = open + 1;
    for pair in raw[open + 1..close].split(',') {
        if let Some(colon) = pair.find(':') {
            let written = pair[..colon].trim();
            let leading = pair[..colon].len() - pair[..colon].trim_start().len();
            let (key, (quote, _)) = crate::framework::strip_yaml_quotes(written);
            if !key.is_empty() {
                keys.push((key.to_string(), start + pair_start + leading + quote));
            }
        }
        pair_start += pair.len() + 1;
    }
    keys
}

/// Push the route `raw`, a YAML scalar starting at `start`, names.
fn push_route(raw: &str, start: usize, spans: &mut Vec<SymbolSpan>) {
    let trimmed = raw.trim_start();
    let leading = raw.len() - trimmed.len();
    let (name, (quote, _)) = crate::framework::strip_yaml_quotes(trimmed.trim_end());
    if !is_drupal_route_name(name) {
        return;
    }
    let name_start = start + leading + quote;
    spans.push(SymbolSpan {
        start: name_start as u32,
        end: (name_start + name.len()) as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::DrupalRoute,
            key: name.to_string(),
            is_write: false,
            is_optional: false,
        },
    });
}

/// Push each route of an `[a, 'b']` flow sequence starting at `start`.
fn push_flow_routes(raw: &str, start: usize, spans: &mut Vec<SymbolSpan>) {
    let Some(open) = raw.find('[') else {
        return;
    };
    let close = raw.rfind(']').unwrap_or(raw.len());
    let mut item_start = open + 1;
    for item in raw[open + 1..close.max(open + 1)].split(',') {
        push_route(item, start + item_start, spans);
        item_start += item.len() + 1;
    }
}

/// Where the route name the cursor is typing in a links file starts: the
/// value of a `route_name:` or `base_route:`, or an item of an
/// `appears_on:` list.
pub(crate) fn links_route_name_start(content: &str, cursor: usize) -> Option<usize> {
    let before = content.get(..cursor)?;
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let line = &before[line_start..];
    let name_start = line
        .bytes()
        .rposition(|byte| byte.is_ascii_whitespace() || matches!(byte, b'\'' | b'"'))
        .map_or(0, |index| index + 1);
    let opening = line[..name_start].trim_end_matches(['\'', '"']).trim_end();
    let indent = crate::framework::leading_spaces(opening);
    let opening = &opening[indent..];
    let reads_route = match opening.strip_suffix(':') {
        Some(key) => indent > 0 && names_route(key),
        None => opening == "-" && in_appears_on(&content[..line_start], indent),
    };
    reads_route.then_some(line_start + name_start)
}

/// Whether a list item indented by `indent`, after the text `above`, is an
/// item of an `appears_on:` list: the first line above it that is not an
/// item of the same list is the key, indented no deeper than the item.
fn in_appears_on(above: &str, indent: usize) -> bool {
    above
        .lines()
        .rev()
        .map(crate::symfony::resources::yaml_content_before_comment)
        .filter(|line| !line.trim().is_empty())
        .find(|line| {
            let line_indent = crate::framework::leading_spaces(line);
            line_indent < indent || (line_indent == indent && !line[line_indent..].starts_with('-'))
        })
        .is_some_and(|line| line.trim() == "appears_on:")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routes(content: &str) -> Vec<&str> {
        links_route_spans(content)
            .iter()
            .map(|span| &content[span.start as usize..span.end as usize])
            .collect()
    }

    #[test]
    fn route_keys_and_appears_on_items_read_routes() {
        let yaml = "a:\n  route_name: one\n  base_route: 'two'\n  parent_id: no\n  appears_on:\n    - three\n    - \"four\" # comment\n  parent: no\nb:\n  appears_on: [five, 'six']\nc:\n  appears_on:\n  - seven\n  - eight\n  title: no\nroute_name: not_a_link\n";
        assert_eq!(
            routes(yaml),
            [
                "one", "two", "three", "four", "five", "six", "seven", "eight"
            ]
        );
    }

    #[test]
    fn route_parameters_are_keys_of_the_links_route() {
        let yaml = "a:\n  route_parameters:\n    name: 'World'\n    'id': 1\n  route_name: acme.greeting\nb:\n  route_name: acme.note\n  route_parameters: { acme_note: 1, 'mode': full }\nc:\n  route_parameters:\n    orphan: 1\n";
        let found: Vec<(&str, String)> = links_parameter_spans(yaml)
            .iter()
            .map(|span| {
                let SymbolKind::StringKey { key, .. } = &span.kind else {
                    unreachable!()
                };
                (&yaml[span.start as usize..span.end as usize], key.clone())
            })
            .collect();
        assert_eq!(
            found,
            [
                ("name", "acme.greeting#name".to_string()),
                ("id", "acme.greeting#id".to_string()),
                ("acme_note", "acme.note#acme_note".to_string()),
                ("mode", "acme.note#mode".to_string()),
            ]
        );
    }

    #[test]
    fn completion_starts_in_route_values_only() {
        let yaml = "a:\n  route_name: on";
        assert_eq!(
            links_route_name_start(yaml, yaml.len()),
            Some(yaml.len() - 2)
        );
        let yaml = "a:\n  appears_on:\n    - 'x";
        assert_eq!(
            links_route_name_start(yaml, yaml.len()),
            Some(yaml.len() - 1)
        );
        let yaml = "a:\n  parent: x";
        assert_eq!(links_route_name_start(yaml, yaml.len()), None);
        let yaml = "a:\n  menu_name:\n    - x";
        assert_eq!(links_route_name_start(yaml, yaml.len()), None);
        let yaml = "a:\n  appears_on:\n  - one\n  - tw";
        assert_eq!(
            links_route_name_start(yaml, yaml.len()),
            Some(yaml.len() - 2)
        );
    }
}
