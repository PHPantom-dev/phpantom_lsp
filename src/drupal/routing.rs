//! What a Drupal routing file declares besides route names: the
//! parameters each route's path declares, what Drupal upcasts them by (see
//! [`crate::symbol_map::DrupalRouteSite`]), and the names the route
//! defaults read: the entity type an `_entity_form`, `_entity_list`, or
//! `_entity_view` works on, and the service a `service:method` controller
//! or title callback calls.
//!
//! A parameter only means something under its route, so its span's key
//! names the route as well as the parameter (see [`route_parameter_key`]).

use crate::symbol_map::{
    DeclaredParameter, DrupalRouteSite, StringKeyKind, SymbolKind, SymbolSpan,
};

/// The key of a [`StringKeyKind::DrupalRouteParameter`] span: the
/// parameter `name` under the route `route`.
pub(crate) fn route_parameter_key(route: &str, name: &str) -> String {
    format!("{route}#{name}")
}

/// The route and the parameter name of a route parameter's key.
pub(crate) fn split_route_parameter_key(key: &str) -> (&str, &str) {
    key.rsplit_once('#').unwrap_or(("", key))
}

/// The byte range of the name of each placeholder of a route path, the way
/// Symfony's route compiler, which Drupal's extends, reads them: a name of
/// word characters that does not start with a digit, after an optional `!`
/// and before an optional inline requirement (`{id<\d+>}`) or default
/// (`{page?1}`).
pub(crate) fn path_parameters(path: &str) -> Vec<(usize, usize)> {
    let bytes = path.as_bytes();
    let mut found = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = memchr::memchr(b'{', &bytes[cursor..]) {
        let mut start = cursor + relative + 1;
        if bytes.get(start) == Some(&b'!') {
            start += 1;
        }
        let mut end = start;
        while bytes
            .get(end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte >= 0x80)
        {
            end += 1;
        }
        cursor = end;
        if end == start || bytes[start].is_ascii_digit() {
            continue;
        }
        let mut after = end;
        if bytes.get(after) == Some(&b'<') {
            let Some(close) = memchr::memchr(b'>', &bytes[after..]) else {
                continue;
            };
            after += close + 1;
        }
        if bytes.get(after) == Some(&b'?') {
            let Some(close) = memchr::memchr(b'}', &bytes[after..]) else {
                continue;
            };
            after += close;
        }
        if bytes.get(after) == Some(&b'}') {
            found.push((start, end));
            cursor = after + 1;
        }
    }
    found
}

/// The names of the placeholders a route path declares, in order.
pub(crate) fn path_parameter_names(path: &str) -> impl Iterator<Item = &str> {
    path_parameters(path)
        .into_iter()
        .map(move |(start, end)| &path[start..end])
}

/// The spans of the route parameters a Twig template's `path()` and
/// `url()` pass: the keys of the hash after each route name `routes`
/// holds, as `{{ path('entity.node.canonical', {node: 1, 'mode': 'x'}) }}`
/// writes them.
pub(crate) fn twig_parameter_spans(content: &str, routes: &[SymbolSpan]) -> Vec<SymbolSpan> {
    let bytes = content.as_bytes();
    let skip_space = |mut at: usize| {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        at
    };
    let mut spans = Vec::new();
    for route in routes {
        let SymbolKind::StringKey {
            kind: StringKeyKind::DrupalRoute,
            key: name,
            ..
        } = &route.kind
        else {
            continue;
        };
        // Past the route name's closing quote, a comma and a hash.
        let comma = skip_space(route.end as usize + 1);
        if bytes.get(comma) != Some(&b',') {
            continue;
        }
        let open = skip_space(comma + 1);
        if bytes.get(open) != Some(&b'{') {
            continue;
        }
        let mut at = open + 1;
        let mut depth = 1usize;
        let mut wants_key = true;
        while at < bytes.len() && depth > 0 {
            if depth == 1 && wants_key {
                at = skip_space(at);
                let (start, end) = match bytes.get(at) {
                    Some(quote @ (b'\'' | b'"')) => {
                        let Some(len) = memchr::memchr(*quote, &bytes[at + 1..]) else {
                            break;
                        };
                        (at + 1, at + 1 + len)
                    }
                    Some(byte) if byte.is_ascii_alphabetic() || *byte == b'_' => {
                        let len = bytes[at..]
                            .iter()
                            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
                            .count();
                        (at, at + len)
                    }
                    // A computed key, or the end of the hash.
                    _ => break,
                };
                let after = if bytes[at] == b'\'' || bytes[at] == b'"' {
                    end + 1
                } else {
                    end
                };
                let colon = skip_space(after);
                if bytes.get(colon) != Some(&b':') || start == end {
                    break;
                }
                spans.push(SymbolSpan {
                    start: start as u32,
                    end: end as u32,
                    kind: SymbolKind::StringKey {
                        kind: StringKeyKind::DrupalRouteParameter,
                        key: route_parameter_key(name, &content[start..end]),
                        is_write: false,
                        is_optional: false,
                    },
                });
                wants_key = false;
                at = colon + 1;
                continue;
            }
            match bytes[at] {
                quote @ (b'\'' | b'"') => {
                    at = memchr::memchr(quote, &bytes[at + 1..])
                        .map_or(bytes.len(), |len| at + len + 2);
                    continue;
                }
                b'{' | b'[' | b'(' => depth += 1,
                b'}' | b']' | b')' => depth -= 1,
                b',' if depth == 1 => wants_key = true,
                _ => {}
            }
            at += 1;
        }
    }
    spans
}

/// The method a `_controller`, `_form`, or `_title_callback` value names,
/// as [`DrupalRouteSite::controller`] spells it: `Class::method` as it is,
/// and a class alone as its `__invoke()`.  A `service:method` is `None`.
pub(crate) fn handler_of(value: &str) -> Option<String> {
    let value = value.trim_start_matches('\\');
    if value.is_empty() {
        return None;
    }
    if value.contains("::") {
        Some(value.to_string())
    } else if value.contains(':') {
        None
    } else {
        Some(format!("{value}::__invoke"))
    }
}

/// One `key: value` line of a routing or links file.
pub(super) struct Line<'a> {
    pub(super) indent: usize,
    pub(super) key: &'a str,
    /// Absolute offset of [`Self::key`], quotes excluded.
    pub(super) key_start: usize,
    pub(super) value: &'a str,
    /// Absolute offset of [`Self::value`].
    pub(super) value_start: usize,
}

/// The `key: value` lines of a YAML file, comments and list items left
/// out.
pub(super) fn lines(content: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = crate::symfony::resources::yaml_content_before_comment(line);
        let indent = crate::framework::leading_spaces(semantic);
        let text = &semantic[indent..];
        if text.is_empty() || text.starts_with('-') {
            continue;
        }
        let Some(colon) = text.find(':') else {
            continue;
        };
        let (key, (quote, _)) = crate::framework::strip_yaml_quotes(text[..colon].trim_end());
        lines.push(Line {
            indent,
            key,
            key_start: line_start + indent + quote,
            value: &text[colon + 1..],
            value_start: line_start + indent + colon + 1,
        });
    }
    lines
}

/// The entries of the mapping `block` holds, each with the lines nested
/// under it.
pub(super) fn entries<'b, 'a>(block: &'b [Line<'a>]) -> Vec<(&'b Line<'a>, &'b [Line<'a>])> {
    let Some(first) = block.first() else {
        return Vec::new();
    };
    let indent = first.indent;
    let mut found = Vec::new();
    for (at, line) in block.iter().enumerate() {
        if line.indent != indent {
            continue;
        }
        let nested = block[at + 1..]
            .iter()
            .take_while(|nested| nested.indent > indent)
            .count();
        found.push((line, &block[at + 1..at + 1 + nested]));
    }
    found
}

/// A YAML scalar written at `start`, without its quotes and the space
/// around it, with the absolute offset of its text.
pub(super) fn scalar(value: &str, start: usize) -> (String, usize) {
    let trimmed = value.trim_start();
    let leading = value.len() - trimmed.len();
    let (text, (quote, _)) = crate::framework::strip_yaml_quotes(trimmed.trim_end());
    // A double-quoted scalar escapes its backslashes.
    let text = if trimmed.starts_with('"') {
        text.replace("\\\\", "\\")
    } else {
        text.to_string()
    };
    (text, start + leading + quote)
}

/// The routes a routing file declares, in source order, and the spans of
/// the parameters their paths declare.
///
/// A route is a top-level key whose mapping has a `path:`; the
/// `route_callbacks:` list names the classes that build more.
pub(crate) fn routing_routes(content: &str) -> (Vec<SymbolSpan>, Vec<DrupalRouteSite>) {
    let lines = lines(content);
    let mut spans = Vec::new();
    let mut routes = Vec::new();
    for (index, entry) in lines.iter().enumerate() {
        if entry.indent != 0
            || entry.key == "route_callbacks"
            || !crate::symfony::routes::is_drupal_route_name(entry.key)
        {
            continue;
        }
        let block_len = lines[index + 1..]
            .iter()
            .take_while(|line| line.indent > 0)
            .count();
        let block = &lines[index + 1..index + 1 + block_len];
        let mut path = None;
        let mut defaults: &[Line<'_>] = &[];
        let mut options: &[Line<'_>] = &[];
        for (child, nested) in entries(block) {
            match child.key {
                "path" => path = Some(scalar(child.value, child.value_start)),
                "defaults" => defaults = nested,
                "options" => options = nested,
                _ => {}
            }
        }
        let Some((path, path_start)) = path else {
            continue;
        };
        for (start, end) in path_parameters(&path) {
            spans.push(SymbolSpan {
                start: (path_start + start) as u32,
                end: (path_start + end) as u32,
                kind: SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalRouteParameter,
                    key: route_parameter_key(entry.key, &path[start..end]),
                    is_write: true,
                    is_optional: false,
                },
            });
        }

        let mut controller = None;
        let mut form = None;
        let mut title_callback = None;
        let mut entity_view = None;
        let mut entity_form = None;
        let mut values = Vec::new();
        for (default, _) in entries(defaults) {
            let (value, _) = scalar(default.value, default.value_start);
            if !default.key.starts_with('_') {
                // Read as written: a quoted `'~'` or `''` is a string.
                values.push((default.key.to_string(), is_yaml_null(default.value.trim())));
            }
            match default.key {
                "_controller" => controller = handler_of(&value),
                "_form" => {
                    form = (!value.is_empty() && !value.contains(':'))
                        .then(|| format!("{}::buildForm", value.trim_start_matches('\\')));
                }
                "_title_callback" => title_callback = handler_of(&value),
                "_entity_view" => entity_view = entity_type_of(&value),
                "_entity_form" => entity_form = entity_type_of(&value),
                _ => {}
            }
        }
        let mut parameters = Vec::new();
        for (option, nested) in entries(options) {
            if option.key != "parameters" {
                continue;
            }
            for (parameter, declared) in entries(nested) {
                let settings = entries(declared);
                parameters.push(DeclaredParameter {
                    name: parameter.key.to_string(),
                    ty: settings
                        .iter()
                        .find(|(setting, _)| setting.key == "type")
                        .map(|(ty, _)| scalar(ty.value, ty.value_start).0),
                    converted_otherwise: settings
                        .iter()
                        .any(|(setting, _)| matches!(setting.key, "converter" | "tempstore")),
                });
            }
        }
        routes.push(DrupalRouteSite {
            name: entry.key.to_string(),
            path,
            controller: form.or(controller),
            title_callback,
            parameters,
            defaults: values,
            entity_type: entity_view.or(entity_form),
        });
    }
    (spans, routes)
}

/// Whether a YAML scalar, as written with its quotes, is `null`.
fn is_yaml_null(value: &str) -> bool {
    matches!(value, "" | "~" | "null" | "Null" | "NULL")
}

/// The entity type an `_entity_view` or `_entity_form` value names: the
/// part before its first `.`.
pub(crate) fn entity_type_of(value: &str) -> Option<String> {
    value
        .split('.')
        .next()
        .filter(|id| is_plain_name(id))
        .map(str::to_string)
}

/// The entity type, service, and permission spans of a routing file, in
/// source order.
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
        if key == "_permission" {
            spans.extend(crate::drupal::permissions::requirement_spans(
                value,
                value_start,
            ));
            continue;
        }
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
        let yaml = "a:\n  path: '/a'\n  defaults:\n    _entity_form: 'node.edit'\n    _entity_view: node.full\n    _entity_list: node_type\n    _controller: acme.greeter:greet\n    _title_callback: '\\Drupal\\acme\\Title::get'\n  requirements:\n    _entity_access: node.update\n    _permission: 'access content+administer acme'\n";
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
                ("access content", StringKeyKind::DrupalPermission),
                ("administer acme", StringKeyKind::DrupalPermission),
            ]
        );
    }

    #[test]
    fn twig_hashes_pass_the_parameters_of_their_route() {
        let twig = "{{ path('acme.note', {acme_note: note.id, 'mode': {a: 1}|first, other: x(1, 2)}) }}\n{{ url('acme.note') }}{{ path('acme.note', vars) }}";
        let routes = crate::symfony::routes::twig_route_spans(twig, StringKeyKind::DrupalRoute);
        let found: Vec<(&str, String)> = twig_parameter_spans(twig, &routes)
            .iter()
            .map(|span| {
                let SymbolKind::StringKey { key, .. } = &span.kind else {
                    unreachable!()
                };
                (&twig[span.start as usize..span.end as usize], key.clone())
            })
            .collect();
        assert_eq!(
            found,
            [
                ("acme_note", "acme.note#acme_note".to_string()),
                ("mode", "acme.note#mode".to_string()),
                ("other", "acme.note#other".to_string()),
            ]
        );
    }

    fn names(path: &str) -> Vec<&str> {
        path_parameter_names(path).collect()
    }

    #[test]
    fn path_parameters_are_read_the_way_symfony_compiles_them() {
        assert_eq!(
            names("/node/{node}/revisions/{node_revision}/view"),
            ["node", "node_revision"]
        );
        assert_eq!(
            names("/a/{id<\\d+>}/{page?1}/{!slug}"),
            ["id", "page", "slug"]
        );
        assert!(names("/a/{1x}/{}/{ x }/{open").is_empty());
    }

    #[test]
    fn routes_keep_their_path_handlers_and_upcasting() {
        let yaml = "acme.note:\n  path: '/note/{acme_note}/{mode}'\n  defaults:\n    _controller: '\\Drupal\\acme\\Controller\\NoteController::view'\n    _title_callback: 'Drupal\\acme\\Controller\\NoteController::title'\n    _entity_view: 'acme_note.full'\n    mode: ~\n  options:\n    parameters:\n      acme_note:\n        type: entity:acme_note\n      mode:\n        converter: x\nacme.form:\n  path: /form/{node}\n  defaults:\n    _controller: 'acme.runner:run'\n    _form: '\\Drupal\\acme\\Form\\NoteForm'\nacme.invoke:\n  path: /invoke\n  defaults:\n    _controller: \"\\\\Drupal\\\\acme\\\\Controller\\\\Invoked\"\n    _title_callback: acme.titles:title\nnot_a_route:\n  defaults: {}\nroute_callbacks:\n  - '\\Drupal\\acme\\Routing\\Routes::routes'\n";
        let (spans, routes) = routing_routes(yaml);
        let declared: Vec<(&str, String)> = spans
            .iter()
            .map(|span| {
                let SymbolKind::StringKey { key, is_write, .. } = &span.kind else {
                    unreachable!()
                };
                assert!(is_write);
                (&yaml[span.start as usize..span.end as usize], key.clone())
            })
            .collect();
        assert_eq!(
            declared,
            [
                ("acme_note", "acme.note#acme_note".to_string()),
                ("mode", "acme.note#mode".to_string()),
                ("node", "acme.form#node".to_string()),
            ]
        );
        let note = DrupalRouteSite {
            name: "acme.note".to_string(),
            path: "/note/{acme_note}/{mode}".to_string(),
            controller: Some("Drupal\\acme\\Controller\\NoteController::view".to_string()),
            title_callback: Some("Drupal\\acme\\Controller\\NoteController::title".to_string()),
            parameters: vec![
                DeclaredParameter {
                    name: "acme_note".to_string(),
                    ty: Some("entity:acme_note".to_string()),
                    converted_otherwise: false,
                },
                DeclaredParameter {
                    name: "mode".to_string(),
                    ty: None,
                    converted_otherwise: true,
                },
            ],
            defaults: vec![("mode".to_string(), true)],
            entity_type: Some("acme_note".to_string()),
        };
        assert_eq!(routes[0], note);
        assert_eq!(
            routes[1].controller.as_deref(),
            Some("Drupal\\acme\\Form\\NoteForm::buildForm")
        );
        assert_eq!(
            routes[2].controller.as_deref(),
            Some("Drupal\\acme\\Controller\\Invoked::__invoke")
        );
        assert_eq!(routes[2].title_callback, None);
        assert_eq!(routes.len(), 3);
    }
}
