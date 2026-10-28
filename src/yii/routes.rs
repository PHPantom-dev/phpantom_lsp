//! What a Yii route declares beyond its name: the placeholders its path
//! declares, and the handler whose `#[RouteArgument]`s read them.
//!
//! A route argument only means something under the routes that dispatch to
//! its handler, so its span's key names the handler as well as the argument
//! (see [`route_argument_key`]), and the routes are looked up by handler in
//! the route side tables of the files that declare them.

use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::symbol_map::{RouteSite, StringKeyKind, SymbolKind, SymbolMap};
use crate::text_position::LineIndex;

/// The attribute that hydrates a handler's parameter from a route argument.
pub(crate) const ROUTE_ARGUMENT_ATTRIBUTE: &str =
    "Yiisoft\\Router\\HydratorAttribute\\RouteArgument";

/// Whether `class_name`, as resolved, is Yii's `#[RouteArgument]`.
pub(crate) fn is_route_argument_attribute(class_name: &str) -> bool {
    class_name
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(ROUTE_ARGUMENT_ATTRIBUTE)
}

/// The byte range of the name of every placeholder in a route `path`:
/// `{id}`, `{id:\d+}`, and the ones inside an optional segment
/// (`/blog[/{page}]`) alike.  A pattern's own braces (`{year:\d{4}}`) are
/// skipped.
pub(crate) fn path_placeholders(path: &str) -> Vec<(usize, usize)> {
    let bytes = path.as_bytes();
    let mut found = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = memchr::memchr(b'{', &bytes[cursor..]) {
        let open = cursor + relative;
        let mut start = open + 1;
        while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
            start += 1;
        }
        let mut end = start;
        while bytes
            .get(end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            end += 1;
        }
        let mut after = end;
        while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
            after += 1;
        }
        let mut depth = 1usize;
        let mut close = open + 1;
        while close < bytes.len() && depth > 0 {
            match bytes[close] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            close += 1;
        }
        let names_one = end > start
            && (bytes[start].is_ascii_alphabetic() || bytes[start] == b'_')
            && matches!(bytes.get(after), Some(b'}' | b':'));
        if names_one && depth == 0 {
            found.push((start, end));
        }
        cursor = close;
    }
    found
}

/// The names of the placeholders a route `path` declares, in order.
pub(crate) fn path_placeholder_names(path: &str) -> Vec<&str> {
    path_placeholders(path)
        .into_iter()
        .map(|(start, end)| &path[start..end])
        .collect()
}

/// How a route's handler is spelled in a [`RouteSite`] and in a route
/// argument's key: the class and the method, compared without regard to
/// case as PHP compares them.
pub(crate) fn handler_key(class: &str, method: &str) -> String {
    let mut key = class.trim_start_matches('\\').to_ascii_lowercase();
    key.push_str("::");
    key.push_str(&method.to_ascii_lowercase());
    key
}

/// The key of a [`StringKeyKind::YiiRouteArgument`] span: the argument
/// `name` under the `handler` that reads it.
pub(crate) fn route_argument_key(handler: &str, name: &str) -> String {
    format!("{handler}#{name}")
}

/// The handler and the argument name of a route argument's key.
pub(crate) fn split_route_argument_key(key: &str) -> (&str, &str) {
    key.rsplit_once('#').unwrap_or(("", key))
}

/// The route name a route file's map declares at `start`.
fn declared_name_at(map: &SymbolMap, start: u32) -> Option<&str> {
    let first = map.spans.partition_point(|span| span.start < start);
    map.spans[first..]
        .iter()
        .take_while(|span| span.start == start)
        .find_map(|span| match &span.kind {
            SymbolKind::StringKey {
                kind: StringKeyKind::YiiRoute,
                key,
                is_write: true,
                ..
            } => Some(key.as_str()),
            _ => None,
        })
}

impl Backend {
    /// The paths of the routes the project names `name`, sorted.
    pub(crate) fn yii_route_paths(&self, name: &str) -> Vec<String> {
        let mut paths = Vec::new();
        self.visit_declaring_symbol_maps(|_, map| {
            for route in &map.routes {
                if let (Some(start), Some(path)) = (route.name_start, &route.path)
                    && declared_name_at(map, start) == Some(name)
                {
                    paths.push(path.clone());
                }
            }
        });
        paths.sort_unstable();
        paths.dedup();
        paths
    }

    /// Visit every route that dispatches to `handler`, with the file that
    /// declares it.
    fn visit_yii_handler_routes(&self, handler: &str, mut visit: impl FnMut(&str, &RouteSite)) {
        self.visit_declaring_symbol_maps(|uri, map| {
            for route in &map.routes {
                if route.handler.as_deref() == Some(handler) {
                    visit(uri, route);
                }
            }
        });
    }

    /// The paths of the routes that dispatch to `handler`, sorted.
    pub(crate) fn yii_handler_paths(&self, handler: &str) -> Vec<String> {
        let mut paths = Vec::new();
        self.visit_yii_handler_routes(handler, |_, route| {
            paths.extend(route.path.clone());
        });
        paths.sort_unstable();
        paths.dedup();
        paths
    }

    /// The arguments the routes that dispatch to `handler` declare, sorted,
    /// or `None` when no route the project declares dispatches to it, so
    /// that nothing can be said about what it may read.
    ///
    /// A route whose path is not all written out declares arguments that
    /// cannot be listed, which is also `None`.
    pub(crate) fn yii_handler_arguments(&self, handler: &str) -> Option<Vec<String>> {
        let mut any = false;
        let mut open = false;
        let mut names = Vec::new();
        self.visit_yii_handler_routes(handler, |_, route| {
            any = true;
            match &route.path {
                Some(path) => {
                    names.extend(path_placeholder_names(path).into_iter().map(str::to_string))
                }
                None => open = true,
            }
        });
        if !any || open {
            return None;
        }
        names.sort_unstable();
        names.dedup();
        Some(names)
    }

    /// Where the paths of the routes that dispatch to a route argument's
    /// handler declare it: each `{name}` its key names.
    pub(crate) fn yii_route_argument_locations(&self, key: &str) -> Vec<Location> {
        let (handler, name) = split_route_argument_key(key);
        let mut found: Vec<(String, u32, u32)> = Vec::new();
        self.visit_yii_handler_routes(handler, |uri, route| {
            for placeholder in &route.placeholders {
                if placeholder.name == name {
                    found.push((uri.to_string(), placeholder.start, placeholder.end));
                }
            }
        });
        found.sort_unstable();
        found.dedup();

        let mut locations = Vec::with_capacity(found.len());
        for group in found.chunk_by(|a, b| a.0 == b.0) {
            let uri = &group[0].0;
            let (Some(parsed), Some(content)) =
                (crate::util::parse_url(uri), self.get_file_content_arc(uri))
            else {
                continue;
            };
            let lines = LineIndex::new(&content);
            for (_, start, end) in group {
                locations.push(Location {
                    uri: parsed.clone(),
                    range: Range {
                        start: lines.position(*start as usize),
                        end: lines.position(*end as usize),
                    },
                });
            }
        }
        locations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_read_with_patterns_and_optional_segments() {
        assert_eq!(
            path_placeholder_names("/blog/{slug}[/{page:\\d+}]"),
            vec!["slug", "page"]
        );
        assert_eq!(
            path_placeholder_names("/archive/{year:\\d{4}}/{ month }"),
            vec!["year", "month"]
        );
        assert!(path_placeholder_names("/static/{1}/{}").is_empty());
        let path = "/users/{id}";
        let (start, end) = path_placeholders(path)[0];
        assert_eq!(&path[start..end], "id");
    }

    #[test]
    fn a_route_argument_key_names_its_handler() {
        let handler = handler_key("\\App\\Web\\PostController", "View");
        assert_eq!(handler, "app\\web\\postcontroller::view");
        let key = route_argument_key(&handler, "slug");
        assert_eq!(split_route_argument_key(&key), (handler.as_str(), "slug"));
    }
}
