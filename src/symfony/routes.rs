//! Route names in Symfony's routing YAML and XML, and in Twig templates.
//!
//! Like the container configuration (see [`super::resources`]), the files
//! are read as text: a top-level key of a routing YAML file with a `path:`
//! declares a route, and so does a `<route id="…">` in a routing XML file.
//! A route that is an `alias:` of another reads the other one's name, and
//! Twig's `path('…')` and `url('…')` read one too.  Each becomes a
//! [`StringKey`](SymbolKind::StringKey) span in the file's resource symbol
//! map.  A key with a `resource:`, or an `<import>`, loads routes from
//! elsewhere, and becomes a [`RouteImport`] (see [`super::route_world`]).

use super::resources::{yaml_content_before_comment, yaml_mapping_key};
use crate::symbol_map::{RouteImport, StringKeyKind, SymbolKind, SymbolSpan};

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

/// What one routing YAML or XML file declares: its route-name spans,
/// sorted by offset, and the routes it imports, in source order.
pub(crate) fn routing_declarations(
    uri: &str,
    content: &str,
) -> (Vec<SymbolSpan>, Vec<RouteImport>) {
    let mut spans = Vec::new();
    let mut imports = Vec::new();
    match super::config_format(uri) {
        Some(super::ConfigFormat::Yaml) => {
            scan_yaml(content, SYMFONY_ROUTES, &mut spans, &mut imports);
        }
        Some(super::ConfigFormat::Xml) => scan_xml(content, &mut spans, &mut imports),
        None => {}
    }
    spans.sort_by_key(|span| (span.start, span.end));
    (spans, imports)
}

/// The route-name spans of a Drupal routing file, sorted by offset.
pub(crate) fn drupal_routing_spans(content: &str) -> Vec<SymbolSpan> {
    let mut spans = Vec::new();
    let mut imports = Vec::new();
    scan_yaml(content, DRUPAL_ROUTES, &mut spans, &mut imports);
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

/// The kind of key a routing file declares, what a name of that kind may
/// look like, and whether the file can import other routes.
#[derive(Clone, Copy)]
struct RouteNaming {
    kind: StringKeyKind,
    is_name: fn(&str) -> bool,
    /// Whether a `resource:` imports routes: true for Symfony's routing
    /// files, false for Drupal's, which have no imports.
    reads_imports: bool,
}

const SYMFONY_ROUTES: RouteNaming = RouteNaming {
    kind: StringKeyKind::SymfonyRoute,
    is_name: is_route_name,
    reads_imports: true,
};

const DRUPAL_ROUTES: RouteNaming = RouteNaming {
    kind: StringKeyKind::DrupalRoute,
    is_name: is_drupal_route_name,
    reads_imports: false,
};

fn route_span(key: &str, start: usize, is_write: bool) -> SymbolSpan {
    route_span_of(StringKeyKind::SymfonyRoute, key, start, is_write)
}

fn route_span_of(kind: StringKeyKind, key: &str, start: usize, is_write: bool) -> SymbolSpan {
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

/// Whether `name` can be a route name.  Symfony allows more, but these are
/// the names anyone writes, and a quoted key with spaces in it is not one.
fn is_route_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

/// Whether `name` can be a Drupal route name.  Drupal's own routes include
/// `<front>` and `<none>`, and a route attribute's alias is a class name
/// (`Drupal\acme\Controller\Foo::bar`), so only what YAML or PHP could not
/// hold in a plain name is ruled out.
pub(crate) fn is_drupal_route_name(name: &str) -> bool {
    !name.is_empty()
        && !name
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || matches!(byte, b'\'' | b'"' | b'{' | b'}'))
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
/// an `alias:` rather than a `resource:`, which imports other routes.
fn scan_yaml(
    content: &str,
    naming: RouteNaming,
    spans: &mut Vec<SymbolSpan>,
    imports: &mut Vec<RouteImport>,
) {
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
        scan_yaml_route(line, &lines[index + 1..], naming, spans, imports);
    }
}

/// A YAML scalar, without its quotes and the space around it.
fn yaml_scalar(value: &str) -> &str {
    crate::framework::strip_yaml_quotes(value.trim()).0
}

/// Declare `entry` as a route if its mapping makes it one, and read the
/// route an `alias:` names, or the import a `resource:` makes it.  `rest`
/// is every line after it.
fn scan_yaml_route(
    entry: &YamlLine<'_>,
    rest: &[YamlLine<'_>],
    naming: RouteNaming,
    spans: &mut Vec<SymbolSpan>,
    imports: &mut Vec<RouteImport>,
) {
    if !(naming.is_name)(entry.key) {
        return;
    }
    let mut is_route = false;
    let mut import: Option<RouteImport> = None;
    let mut loader = None;
    let mut name_prefix = "";
    let mut alias = None;

    let inline = entry.value.trim();
    if let Some(mapping) = inline.strip_prefix('{') {
        for pair in mapping.trim_end_matches('}').split(',') {
            let Some((key, value)) = pair.split_once(':') else {
                continue;
            };
            match key.trim().trim_matches(['\'', '"']) {
                "path" => is_route = true,
                "resource" => import = Some(yaml_import(Some(yaml_scalar(value)))),
                "type" => loader = Some(yaml_scalar(value)),
                "name_prefix" => name_prefix = yaml_scalar(value),
                "alias" => {
                    let offset = value.as_ptr() as usize - entry.value.as_ptr() as usize;
                    alias = Some((value, entry.value_start + offset));
                }
                _ => {}
            }
        }
    } else {
        let mut child_indent = None;
        for (index, child) in rest
            .iter()
            .enumerate()
            .take_while(|(_, child)| child.indent > entry.indent)
        {
            if *child_indent.get_or_insert(child.indent) != child.indent {
                continue;
            }
            match child.key {
                "path" => is_route = true,
                "resource" => {
                    // `resource: { path: …, namespace: … }` written as a
                    // block.
                    let resource = match yaml_scalar(child.value) {
                        "" => rest[index + 1..]
                            .iter()
                            .take_while(|nested| nested.indent > child.indent)
                            .find(|nested| nested.key == "path")
                            .map(|nested| yaml_scalar(nested.value)),
                        resource => Some(resource),
                    };
                    import = Some(yaml_import(resource));
                }
                "type" => loader = Some(yaml_scalar(child.value)),
                "name_prefix" => name_prefix = yaml_scalar(child.value),
                "alias" => alias = Some((child.value, child.value_start)),
                _ => {}
            }
        }
    }

    if let Some(mut import) = import.filter(|_| naming.reads_imports) {
        import.loader = loader
            .filter(|loader| !loader.is_empty())
            .map(str::to_string);
        import.name_prefix = name_prefix.to_string();
        imports.push(import);
        return;
    }

    let alias = alias.and_then(|(value, value_start)| {
        let trimmed = value.trim_start();
        let leading = value.len() - trimmed.len();
        let (name, (quote, _)) = crate::framework::strip_yaml_quotes(trimmed.trim_end());
        (naming.is_name)(name).then(|| (name, value_start + leading + quote))
    });
    if !(is_route || alias.is_some()) {
        return;
    }
    spans.push(route_span_of(naming.kind, entry.key, entry.key_start, true));
    if let Some((name, start)) = alias {
        spans.push(route_span_of(naming.kind, name, start, false));
    }
}

/// The import a `resource:` makes, before its `type:` and `name_prefix:`
/// are read.  A resource written as a mapping (`{ path: …, namespace: … }`)
/// loads its `path`.
fn yaml_import(resource: Option<&str>) -> RouteImport {
    let resource = resource.and_then(|resource| match resource.strip_prefix('{') {
        Some(mapping) => mapping
            .trim_end_matches('}')
            .split(',')
            .filter_map(|pair| pair.split_once(':'))
            .find(|(key, _)| key.trim() == "path")
            .map(|(_, path)| yaml_scalar(path)),
        None => Some(resource),
    });
    RouteImport {
        resource: resource
            .filter(|resource| !resource.is_empty())
            .map(str::to_string),
        loader: None,
        name_prefix: String::new(),
    }
}

// ─── XML ────────────────────────────────────────────────────────────────────

/// The `<route id="…">` declarations of a routing XML file, the route each
/// `alias="…"` names, and its `<import>`s.
fn scan_xml(content: &str, spans: &mut Vec<SymbolSpan>, imports: &mut Vec<RouteImport>) {
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
        let is_element = |name: &str| {
            tag.strip_prefix(name)
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_whitespace()))
        };
        if is_element("<import") {
            let attribute = |name: &str| {
                crate::framework::xml_attr_value(tag, tag_start, &[name])
                    .map(|(value, _, _)| value)
                    .filter(|value| !value.is_empty())
            };
            imports.push(RouteImport {
                resource: attribute("resource"),
                loader: attribute("type"),
                name_prefix: attribute("name-prefix").unwrap_or_default(),
            });
            continue;
        }
        if !is_element("<route") {
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
/// read, as `kind` keys (Symfony's and Drupal's Twig both have the two
/// functions), sorted by offset.
pub(crate) fn twig_route_spans(content: &str, kind: StringKeyKind) -> Vec<SymbolSpan> {
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
        let is_name = match kind {
            StringKeyKind::DrupalRoute => is_drupal_route_name(name),
            _ => is_route_name(name),
        };
        if is_name {
            spans.push(route_span_of(kind, name, start, false));
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
        let (found, _) = routing_declarations("file:///p/config/routes.yaml", yaml);
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
    fn a_drupal_entry_with_a_resource_and_a_path_is_a_route() {
        let yaml = "acme.export:\n  path: '/acme/export'\n  resource: 'not an import'\n";
        assert_eq!(
            routes(&drupal_routing_spans(yaml), yaml),
            vec![("acme.export".to_string(), true)]
        );
    }

    #[test]
    fn drupal_routing_files_declare_their_keys_with_a_path() {
        let yaml = "'<front>':\n  path: '/<front>'\nacme.greeting:\n  path: '/acme/greeting/{name}'\n  defaults:\n    _controller: '\\Drupal\\acme\\Controller\\GreetingController::greet'\n\nroute_callbacks:\n  - '\\Drupal\\acme\\Routing\\Routes::routes'\n";
        assert_eq!(
            routes(&drupal_routing_spans(yaml), yaml),
            vec![
                ("<front>".to_string(), true),
                ("acme.greeting".to_string(), true),
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
        let (found, _) = routing_declarations("file:///p/config/routes.xml", xml);
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
    fn imports_are_read_with_their_loader_and_name_prefix() {
        let import =
            |resource: Option<&str>, loader: Option<&str>, name_prefix: &str| RouteImport {
                resource: resource.map(str::to_string),
                loader: loader.map(str::to_string),
                name_prefix: name_prefix.to_string(),
            };
        let yaml = "controllers:\n    resource:\n        path: ../src/Controller/\n        namespace: App\\Controller\n    type: attribute\n\nadmin:\n    resource: '../src/Admin/'\n    name_prefix: admin_\nshop: { resource: shop.xml, name_prefix: 'shop_' }\nflow:\n    resource: { path: ../src/Flow/, namespace: App\\Flow }\nwhen@dev:\n    _errors:\n        resource: '@FrameworkBundle/Resources/config/routing/errors.php'\n";
        let (spans, imports) = routing_declarations("file:///p/config/routes.yaml", yaml);
        assert!(spans.is_empty(), "{spans:?}");
        assert_eq!(
            imports,
            vec![
                import(Some("../src/Controller/"), Some("attribute"), ""),
                import(Some("../src/Admin/"), None, "admin_"),
                import(Some("shop.xml"), None, "shop_"),
                import(Some("../src/Flow/"), None, ""),
                import(
                    Some("@FrameworkBundle/Resources/config/routing/errors.php"),
                    None,
                    ""
                ),
            ]
        );

        let xml = r#"<routes><import resource="errors.php" type="php" name-prefix="x_"/><import resource="other.xml"/></routes>"#;
        let (_, imports) = routing_declarations("file:///p/config/routes.xml", xml);
        assert_eq!(
            imports,
            vec![
                import(Some("errors.php"), Some("php"), "x_"),
                import(Some("other.xml"), None, ""),
            ]
        );
    }

    #[test]
    fn twig_reads_path_and_url_calls() {
        let twig = "{# path('commented') #}\n<a href=\"{{ path('blog_index') }}\">{{ url( \"blog_post\", {slug: post.slug}) }}</a>\n{{ app.request.path('not_a_route') }}{{ asset_path('nor_this') }}\n";
        assert_eq!(
            routes(&twig_route_spans(twig, StringKeyKind::SymfonyRoute), twig),
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
