//! Route names in Symfony's routing YAML and XML, and in Twig templates.
//!
//! Like the container configuration (see [`super::resources`]), the files
//! are read as text: a top-level key of a routing YAML file with a `path:`
//! declares a route, and so does a `<route id="…">` in a routing XML file.
//! A route that is an `alias:` of another reads the other one's name, and
//! Twig's `path('…')` and `url('…')` read one too.  Each becomes a
//! [`StringKey`](SymbolKind::StringKey) span in the file's resource symbol
//! map.

use super::resources::{yaml_content_before_comment, yaml_mapping_key};
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan};

/// Whether `uri` is a file Symfony loads routes from: `config/routes.yaml`,
/// a file under `config/routes/`, or one under a bundle's
/// `config/routing/`.
pub(crate) fn is_routing_config_uri(uri: &str) -> bool {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    let mut segments = path.split('/');
    while let Some(segment) = segments.next() {
        if segment != "config" {
            continue;
        }
        let Some(next) = segments.next() else {
            return false;
        };
        let lower = next.to_ascii_lowercase();
        let lower = lower.strip_suffix(".dist").unwrap_or(&lower);
        if matches!(lower, "routes" | "routing")
            || lower
                .strip_prefix("routes.")
                .is_some_and(|ext| matches!(ext, "yaml" | "yml" | "xml"))
        {
            return true;
        }
    }
    false
}

/// Whether `uri` is a Twig template.
pub(crate) fn is_twig_uri(uri: &str) -> bool {
    uri.split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .to_ascii_lowercase()
        .ends_with(".twig")
}

/// The route-name spans of one routing YAML or XML file, sorted by offset.
pub(crate) fn routing_key_spans(uri: &str, content: &str) -> Vec<SymbolSpan> {
    let path = uri
        .split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .to_ascii_lowercase();
    let path = path.strip_suffix(".dist").unwrap_or(&path);
    let mut spans = Vec::new();
    if path.ends_with(".yaml") || path.ends_with(".yml") {
        scan_yaml(content, &mut spans);
    } else if path.ends_with(".xml") {
        scan_xml(content, &mut spans);
    }
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

fn route_span(key: &str, start: usize, is_write: bool) -> SymbolSpan {
    SymbolSpan {
        start: start as u32,
        end: (start + key.len()) as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::SymfonyRoute,
            key: key.to_string(),
            is_write,
            is_optional: false,
        },
    }
}

/// Whether `name` can be a route name.  Symfony allows more, but these are
/// the names anyone writes, and a quoted key with spaces in it is not one.
fn is_route_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

// ─── YAML ───────────────────────────────────────────────────────────────────

/// One `key: value` line of a routing YAML file.
struct YamlLine<'a> {
    indent: usize,
    key: &'a str,
    /// Absolute offset of the key, quotes excluded.
    key_start: usize,
    /// The text after the colon, and its absolute offset.
    value: &'a str,
    value_start: usize,
}

fn yaml_lines(content: &str) -> Vec<YamlLine<'_>> {
    let mut lines = Vec::new();
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = yaml_content_before_comment(line);
        if semantic.trim().is_empty() || semantic.trim_start().starts_with('-') {
            continue;
        }
        let Some((raw_key, raw_start)) = yaml_mapping_key(semantic, line_start) else {
            continue;
        };
        let (key, (quote, _)) = crate::framework::strip_yaml_quotes(raw_key);
        let colon = raw_start - line_start + raw_key.len();
        let after = semantic[colon..]
            .find(':')
            .map_or(semantic.len(), |n| colon + n + 1);
        lines.push(YamlLine {
            indent: crate::framework::leading_spaces(semantic),
            key,
            key_start: raw_start + quote,
            value: &semantic[after..],
            value_start: line_start + after,
        });
    }
    lines
}

/// The route declarations of a routing YAML file: every top-level key, and
/// every key under a top-level `when@env:`, whose mapping has a `path:` or
/// an `alias:` rather than a `resource:` (which imports other routes).
fn scan_yaml(content: &str, spans: &mut Vec<SymbolSpan>) {
    let lines = yaml_lines(content);
    // The indentation of the entries of the `when@env:` section the scan is
    // in, from its first one.
    let mut env_section: Option<Option<usize>> = None;
    for (index, line) in lines.iter().enumerate() {
        if line.indent == 0 {
            if line.key.starts_with("when@") {
                env_section = Some(None);
                continue;
            }
            env_section = None;
        } else {
            let Some(child_indent) = env_section.as_mut() else {
                continue;
            };
            if *child_indent.get_or_insert(line.indent) != line.indent {
                continue;
            }
        }
        scan_yaml_route(line, &lines[index + 1..], spans);
    }
}

/// Declare `entry` as a route if its mapping makes it one, and read the
/// route an `alias:` names.  `rest` is every line after it.
fn scan_yaml_route(entry: &YamlLine<'_>, rest: &[YamlLine<'_>], spans: &mut Vec<SymbolSpan>) {
    if !is_route_name(entry.key) {
        return;
    }
    let mut is_route = false;
    let mut is_import = false;
    let mut alias = None;

    let inline = entry.value.trim();
    if let Some(mapping) = inline.strip_prefix('{') {
        for pair in mapping.trim_end_matches('}').split(',') {
            let Some((key, value)) = pair.split_once(':') else {
                continue;
            };
            match key.trim().trim_matches(['\'', '"']) {
                "path" => is_route = true,
                "resource" => is_import = true,
                "alias" => {
                    let offset = value.as_ptr() as usize - entry.value.as_ptr() as usize;
                    alias = Some((value, entry.value_start + offset));
                }
                _ => {}
            }
        }
    } else {
        let mut child_indent = None;
        for child in rest.iter().take_while(|child| child.indent > entry.indent) {
            if *child_indent.get_or_insert(child.indent) != child.indent {
                continue;
            }
            match child.key {
                "path" => is_route = true,
                "resource" => is_import = true,
                "alias" => alias = Some((child.value, child.value_start)),
                _ => {}
            }
        }
    }

    let alias = alias.and_then(|(value, value_start)| {
        let trimmed = value.trim_start();
        let leading = value.len() - trimmed.len();
        let (name, (quote, _)) = crate::framework::strip_yaml_quotes(trimmed.trim_end());
        is_route_name(name).then(|| (name, value_start + leading + quote))
    });
    if is_import || !(is_route || alias.is_some()) {
        return;
    }
    spans.push(route_span(entry.key, entry.key_start, true));
    if let Some((name, start)) = alias {
        spans.push(route_span(name, start, false));
    }
}

// ─── XML ────────────────────────────────────────────────────────────────────

/// The `<route id="…">` declarations of a routing XML file, and the route
/// each `alias="…"` names.
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
        let is_route = tag
            .strip_prefix("<route")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_whitespace()));
        if !is_route {
            continue;
        }
        for (attribute, is_write) in [("id", true), ("alias", false)] {
            if let Some((name, start, _)) =
                crate::framework::xml_attr_value(tag, tag_start, &[attribute])
                && is_route_name(&name)
            {
                spans.push(route_span(&name, start, is_write));
            }
        }
    }
}

// ─── Twig ───────────────────────────────────────────────────────────────────

/// The route names a Twig template's `path('…')` and `url('…')` calls
/// read, sorted by offset.
pub(crate) fn twig_route_spans(content: &str) -> Vec<SymbolSpan> {
    let bytes = content.as_bytes();
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let Some(relative) = memchr::memchr2(b'(', b'{', &bytes[cursor..]) else {
            break;
        };
        let at = cursor + relative;
        cursor = at + 1;
        // A comment says nothing.
        if bytes[at] == b'{' {
            if bytes.get(at + 1) == Some(&b'#') {
                cursor = content[at..]
                    .find("#}")
                    .map_or(bytes.len(), |end| at + end + 2);
            }
            continue;
        }
        if route_function_before(&content[..at]).is_none() {
            continue;
        }
        let mut open = at + 1;
        while bytes.get(open).is_some_and(u8::is_ascii_whitespace) {
            open += 1;
        }
        let Some(quote @ (b'\'' | b'"')) = bytes.get(open).copied() else {
            continue;
        };
        let start = open + 1;
        let Some(len) = memchr::memchr(quote, &bytes[start..]) else {
            break;
        };
        let name = &content[start..start + len];
        if is_route_name(name) {
            spans.push(route_span(name, start, false));
        }
        cursor = start + len + 1;
    }
    spans
}

/// The `path` or `url` function whose name `before` ends with, as long as
/// it is not a method of something (`app.request.path(`).
fn route_function_before(before: &str) -> Option<&str> {
    let trimmed = before.trim_end();
    let name_start = trimmed
        .bytes()
        .rposition(|byte| !(byte.is_ascii_alphanumeric() || byte == b'_'))
        .map_or(0, |index| index + 1);
    let name = &trimmed[name_start..];
    let preceded_by_dot = trimmed[..name_start].ends_with('.');
    (matches!(name, "path" | "url") && !preceded_by_dot).then_some(name)
}

/// Where the route name a Twig `path('` or `url('` the cursor is in starts,
/// when the name typed so far runs up to `cursor`.
pub(crate) fn twig_route_name_start(content: &str, cursor: usize) -> Option<usize> {
    let before = content.get(..cursor)?;
    let name_start = before
        .bytes()
        .rposition(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')))
        .map_or(0, |index| index + 1);
    let before_name = before[..name_start].strip_suffix(['\'', '"'])?;
    let before_paren = before_name.trim_end().strip_suffix('(')?;
    route_function_before(before_paren)?;
    Some(name_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routes(found: &[SymbolSpan], content: &str) -> Vec<(String, bool)> {
        found
            .iter()
            .map(|span| {
                let SymbolKind::StringKey { key, is_write, .. } = &span.kind else {
                    unreachable!()
                };
                assert_eq!(
                    &content[span.start as usize..span.end as usize],
                    key,
                    "span must cover the name"
                );
                (key.clone(), *is_write)
            })
            .collect()
    }

    #[test]
    fn yaml_declares_routes_and_reads_aliases() {
        let yaml = "# the home page\nhomepage:\n    path: /{_locale}\n    controller: App\\Controller\\HomeController\n\ncontrollers:\n    resource: routing.controllers\n    prefix: /{_locale}\n\n'quoted_route': { path: /quoted }\nold_home:\n    alias: homepage\ninline_alias: { alias: 'quoted_route' }\nwhen@dev:\n    _errors:\n        resource: '@FrameworkBundle/Resources/config/routing/errors.php'\n    app_debug:\n        path: /debug\n        defaults:\n            path: not-a-route\n";
        let found = routing_key_spans("file:///p/config/routes.yaml", yaml);
        assert_eq!(
            routes(&found, yaml),
            vec![
                ("homepage".to_string(), true),
                ("quoted_route".to_string(), true),
                ("old_home".to_string(), true),
                ("homepage".to_string(), false),
                ("inline_alias".to_string(), true),
                ("quoted_route".to_string(), false),
                ("app_debug".to_string(), true),
            ]
        );
    }

    #[test]
    fn xml_declares_routes_and_reads_aliases() {
        let xml = r#"<?xml version="1.0"?>
<routes>
    <!-- <route id="commented" path="/x"/> -->
    <route id="blog_list" path="/blog" controller="App\Controller\BlogController::list"/>
    <route id="blog_index" alias="blog_list"/>
    <import resource="../src/Controller/" type="attribute"/>
</routes>
"#;
        let found = routing_key_spans("file:///p/config/routes.xml", xml);
        assert_eq!(
            routes(&found, xml),
            vec![
                ("blog_list".to_string(), true),
                ("blog_index".to_string(), true),
                ("blog_list".to_string(), false),
            ]
        );
    }

    #[test]
    fn twig_reads_path_and_url_calls() {
        let twig = "{# path('commented') #}\n<a href=\"{{ path('blog_index') }}\">{{ url( \"blog_post\", {slug: post.slug}) }}</a>\n{{ app.request.path('not_a_route') }}{{ asset_path('nor_this') }}\n";
        assert_eq!(
            routes(&twig_route_spans(twig), twig),
            vec![
                ("blog_index".to_string(), false),
                ("blog_post".to_string(), false),
            ]
        );
    }

    #[test]
    fn twig_completion_starts_after_the_quote() {
        let twig = "{{ path('blog_in";
        assert_eq!(twig_route_name_start(twig, twig.len()), Some(9));
        let twig = "{{ url(\"";
        assert_eq!(twig_route_name_start(twig, twig.len()), Some(8));
        let twig = "{{ asset('x";
        assert_eq!(twig_route_name_start(twig, twig.len()), None);
    }

    #[test]
    fn only_routing_configuration_holds_routes() {
        assert!(is_routing_config_uri("file:///p/config/routes.yaml"));
        assert!(is_routing_config_uri("file:///p/config/routes/admin.yaml"));
        assert!(is_routing_config_uri(
            "file:///p/src/Resources/config/routing/blog.xml"
        ));
        assert!(!is_routing_config_uri("file:///p/config/services.yaml"));
        assert!(!is_routing_config_uri(
            "file:///p/config/packages/routing.yaml"
        ));
    }
}
