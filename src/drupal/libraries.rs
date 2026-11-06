//! Drupal's libraries: the sets of CSS and JavaScript files an extension
//! declares, one per top-level key of `<extension>.libraries.yml`, named
//! `<extension>/<key>`.
//!
//! A library is read by the `dependencies` of another library, the
//! `libraries` a theme's info file attaches to every page, a template's
//! `attach_library()`, and the `library` list of a render array's
//! `#attached` (see `symbol_map::extraction::drupal`).  Libraries a module
//! builds in code, through `hook_library_info_build()`, are not read.

use super::routing::{entries, lines, scalar};
use crate::Backend;
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan};

/// Whether `name` has the shape of a library name: an extension and a
/// key, joined by a slash.
pub(crate) fn is_library_name(name: &str) -> bool {
    name.split_once('/')
        .is_some_and(|(extension, key)| !extension.is_empty() && !key.is_empty())
        && !name.bytes().any(|byte| {
            byte.is_ascii_whitespace()
                || matches!(byte, b'\'' | b'"' | b'{' | b'}' | b'[' | b']' | b',')
        })
}

/// The extension whose libraries file `path` (a path or URI) is: the name
/// before `.libraries.yml`.
pub(crate) fn libraries_file_extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".libraries.yml")
        .filter(|extension| !extension.is_empty())
}

/// A library span over `name`, written at `start`.
fn library_span(name: &str, start: usize, is_write: bool) -> SymbolSpan {
    SymbolSpan {
        start: start as u32,
        end: (start + name.len()) as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::DrupalLibrary,
            key: name.to_string(),
            is_write,
            is_optional: false,
        },
    }
}

/// Push the library `raw`, a YAML scalar starting at `start`, reads.
fn push_library(raw: &str, start: usize, spans: &mut Vec<SymbolSpan>) {
    let trimmed = raw.trim_start();
    let leading = raw.len() - trimmed.len();
    let (name, (quote, _)) = crate::framework::strip_yaml_quotes(trimmed.trim_end());
    if is_library_name(name) {
        spans.push(library_span(name, start + leading + quote, false));
    }
}

/// Push each library of an `[a, 'b']` flow sequence starting at `start`.
fn push_flow_libraries(raw: &str, start: usize, spans: &mut Vec<SymbolSpan>) {
    let Some(open) = raw.find('[') else {
        return;
    };
    let close = raw.rfind(']').unwrap_or(raw.len()).max(open + 1);
    let mut item_start = open + 1;
    for item in raw[open + 1..close].split(',') {
        push_library(item, start + item_start, spans);
        item_start += item.len() + 1;
    }
}

/// The library spans of the libraries file of `extension`: each top-level
/// key declares `<extension>/<key>`, and each item of a library's
/// `dependencies` reads a library.  Sorted by offset.
pub(crate) fn libraries_file_spans(extension: &str, content: &str) -> Vec<SymbolSpan> {
    let mut spans = Vec::new();
    // The indentation of the keys of the library the scan is in.
    let mut child_indent: Option<usize> = None;
    // The indentation of the `dependencies:` key whose list the scan is in.
    let mut dependencies: Option<usize> = None;
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = crate::symfony::resources::yaml_content_before_comment(line);
        if semantic.trim().is_empty() {
            continue;
        }
        let indent = crate::framework::leading_spaces(semantic);
        let text = &semantic[indent..];
        // YAML lets a block list's items sit at its key's indentation.
        if let Some(list) = dependencies {
            if indent >= list
                && let Some(item) = text.strip_prefix('-')
            {
                push_library(item, line_start + indent + 1, &mut spans);
                continue;
            }
            dependencies = None;
        }
        let Some(colon) = text.find(':') else {
            continue;
        };
        let (key, (quote, _)) = crate::framework::strip_yaml_quotes(text[..colon].trim_end());
        if indent == 0 {
            child_indent = None;
            let name = format!("{extension}/{key}");
            if !key.is_empty() && is_library_name(&name) {
                spans.push(SymbolSpan {
                    end: (line_start + quote + key.len()) as u32,
                    ..library_span(&name, line_start + quote, true)
                });
            }
            continue;
        }
        let child = *child_indent.get_or_insert(indent);
        if indent == child && key == "dependencies" {
            let value = &text[colon + 1..];
            if value.trim_start().starts_with('[') {
                push_flow_libraries(value, line_start + indent + colon + 1, &mut spans);
            } else {
                dependencies = Some(indent);
            }
        }
    }
    spans
}

/// The libraries a theme's info file attaches to every page: the items of
/// its top-level `libraries` list.  Nothing for a module's or a profile's
/// info file, which has no such list.
pub(crate) fn info_file_spans(content: &str) -> Vec<SymbolSpan> {
    let mut spans = Vec::new();
    if !is_theme_info(content) {
        return spans;
    }
    let mut in_list = false;
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = crate::symfony::resources::yaml_content_before_comment(line);
        if semantic.trim().is_empty() {
            continue;
        }
        let indent = crate::framework::leading_spaces(semantic);
        let text = &semantic[indent..];
        if in_list {
            if let Some(item) = text.strip_prefix('-') {
                push_library(item, line_start + indent + 1, &mut spans);
                continue;
            }
            in_list = false;
        }
        if indent > 0 {
            continue;
        }
        let Some(value) = text.strip_prefix("libraries:") else {
            continue;
        };
        if value.trim_start().starts_with('[') {
            push_flow_libraries(value, line_start + "libraries:".len(), &mut spans);
        } else {
            in_list = true;
        }
    }
    spans
}

/// Whether an info file declares a theme.
fn is_theme_info(content: &str) -> bool {
    crate::framework::line_offsets(content)
        .iter()
        .any(|(_, line)| {
            line.strip_prefix("type:").is_some_and(|value| {
                let value = crate::symfony::resources::yaml_content_before_comment(value).trim();
                crate::framework::strip_yaml_quotes(value).0 == "theme"
            })
        })
}

/// Where the library name the cursor is typing in a libraries file starts:
/// an item of a library's `dependencies`.
pub(crate) fn libraries_file_name_start(content: &str, cursor: usize) -> Option<usize> {
    list_item_start(content, cursor, "dependencies", |indent| indent > 0)
}

/// Where the library name the cursor is typing in a theme's info file
/// starts: an item of its `libraries`.
pub(crate) fn info_file_name_start(content: &str, cursor: usize) -> Option<usize> {
    if !is_theme_info(content) {
        return None;
    }
    list_item_start(content, cursor, "libraries", |indent| indent == 0)
}

/// Where the item the cursor is typing starts, in a block or flow list
/// under a `key:` whose indentation `at_level` accepts.
fn list_item_start(
    content: &str,
    cursor: usize,
    key: &str,
    at_level: impl Fn(usize) -> bool,
) -> Option<usize> {
    let before = content.get(..cursor)?;
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let line = &before[line_start..];
    let name_start = line
        .bytes()
        .rposition(|byte| byte.is_ascii_whitespace() || matches!(byte, b'\'' | b'"' | b'[' | b','))
        .map_or(0, |index| index + 1);
    let opening = line[..name_start].trim_end_matches(['\'', '"']).trim_end();
    let indent = crate::framework::leading_spaces(opening);
    let opening = &opening[indent..];
    let in_list = if opening == "-" {
        list_key_above(&content[..line_start], indent)
            .is_some_and(|(key_indent, found)| found == key && at_level(key_indent))
    } else {
        // A flow list still open on the key's own line.
        opening
            .strip_prefix(key)
            .and_then(|rest| rest.trim_start().strip_prefix(':'))
            .is_some_and(|rest| rest.trim_start().starts_with('[') && !rest.contains(']'))
            && at_level(indent)
    };
    in_list.then_some(line_start + name_start)
}

/// The key whose block list an item indented by `indent`, after the text
/// `above`, belongs to, with the key's indentation: the first line above
/// it that is not an item of the same list, indented no deeper than the
/// item.
fn list_key_above(above: &str, indent: usize) -> Option<(usize, &str)> {
    let line = above
        .lines()
        .rev()
        .map(crate::symfony::resources::yaml_content_before_comment)
        .filter(|line| !line.trim().is_empty())
        .find(|line| {
            let line_indent = crate::framework::leading_spaces(line);
            line_indent < indent || (line_indent == indent && !line[line_indent..].starts_with('-'))
        })?;
    let line_indent = crate::framework::leading_spaces(line);
    let key = line[line_indent..].trim_end().strip_suffix(':')?;
    Some((line_indent, key))
}

/// The `version` the libraries file `content` gives the library `key`, as
/// written.
pub(crate) fn library_version(content: &str, key: &str) -> Option<String> {
    let lines = lines(content);
    let (_, library) = entries(&lines)
        .into_iter()
        .find(|(entry, _)| entry.indent == 0 && entry.key == key)?;
    entries(library)
        .into_iter()
        .find(|(child, _)| child.key == "version")
        .map(|(child, _)| scalar(child.value, child.value_start).0)
        .filter(|version| !version.is_empty())
}

// ─── Twig ───────────────────────────────────────────────────────────────────

/// The `attach_library` function a template calls with a library name.
const ATTACH_LIBRARY: &str = "attach_library";

/// The library names a template's `attach_library('…')` calls read, sorted
/// by offset.  Only a call inside `{{ }}` or `{% %}` reads one, and not in
/// a `{% verbatim %}` block.
pub(crate) fn twig_library_spans(content: &str) -> Vec<SymbolSpan> {
    let bytes = content.as_bytes();
    let mut spans = Vec::new();
    if memchr::memmem::find(bytes, ATTACH_LIBRARY.as_bytes()).is_none() {
        return spans;
    }
    let mut raw_until: Option<&'static str> = None;
    crate::symfony::translations::for_each_twig_block(content, |body, end| {
        let tag = (bytes[body - 1] == b'%')
            .then(|| crate::twig::callables::tag_at(content, body, end))
            .flatten();
        if let Some(close) = raw_until {
            if tag.is_some_and(|(tag, _)| tag == close) {
                raw_until = None;
            }
            return;
        }
        if let Some(close) = tag.and_then(|(tag, _)| crate::twig::callables::opens_raw_text(tag)) {
            raw_until = Some(close);
            return;
        }
        for found in memchr::memmem::find_iter(&bytes[body..end], ATTACH_LIBRARY) {
            let at = body + found;
            if !is_function_start(content, at) {
                continue;
            }
            let mut open = at + ATTACH_LIBRARY.len();
            while open < end && bytes[open].is_ascii_whitespace() {
                open += 1;
            }
            if open >= end || bytes[open] != b'(' {
                continue;
            }
            open += 1;
            while open < end && bytes[open].is_ascii_whitespace() {
                open += 1;
            }
            let Some(quote @ (b'\'' | b'"')) = bytes.get(open).copied().filter(|_| open < end)
            else {
                continue;
            };
            let start = open + 1;
            let Some(len) = memchr::memchr(quote, &bytes[start..end]) else {
                continue;
            };
            let name = &content[start..start + len];
            if is_library_name(name) {
                spans.push(library_span(name, start, false));
            }
        }
    });
    spans
}

/// Whether the `attach_library` at `at` is the function itself, not part
/// of a longer name or a method of something.
fn is_function_start(content: &str, at: usize) -> bool {
    !content[..at]
        .bytes()
        .next_back()
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
        && !content[at + ATTACH_LIBRARY.len()..]
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Where the library name a template's `attach_library('` the cursor is
/// in starts, when the name typed so far runs up to `cursor`.
pub(crate) fn twig_library_name_start(content: &str, cursor: usize) -> Option<usize> {
    let before = content.get(..cursor)?;
    let name_start = before
        .bytes()
        .rposition(|byte| {
            !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'/'))
        })
        .map_or(0, |index| index + 1);
    let before_name = before[..name_start].strip_suffix(['\'', '"'])?;
    let before_paren = before_name.trim_end().strip_suffix('(')?.trim_end();
    let function = before_paren.strip_suffix(ATTACH_LIBRARY)?;
    is_function_start(content, function.len()).then_some(name_start)
}

impl Backend {
    /// What the hover of the library `name` says: the libraries file that
    /// declares it, and the version it gives the library.
    pub(crate) fn drupal_library_detail(&self, name: &str) -> String {
        let declared = self.symfony_declaration_locations(StringKeyKind::DrupalLibrary, name);
        let Some(location) = declared.first() else {
            return "Not declared in the project's libraries files".to_string();
        };
        let uri = location.uri.as_str();
        let mut detail = match self.workspace_relative_path(uri) {
            Some(path) => format!("Declared in `{}`", path),
            None => "Declared in a libraries file".to_string(),
        };
        let key = name.split_once('/').map_or(name, |(_, key)| key);
        if let Some(version) = self
            .get_file_content(uri)
            .and_then(|content| library_version(&content, key))
        {
            detail.push_str(&format!("\n\nVersion `{}`", version));
        }
        detail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each span as the text it covers, its key, and whether it declares.
    fn found<'a>(spans: &[SymbolSpan], content: &'a str) -> Vec<(&'a str, String, bool)> {
        spans
            .iter()
            .map(|span| {
                let SymbolKind::StringKey { key, is_write, .. } = &span.kind else {
                    unreachable!()
                };
                (
                    &content[span.start as usize..span.end as usize],
                    key.clone(),
                    *is_write,
                )
            })
            .collect()
    }

    #[test]
    fn top_level_keys_declare_and_dependencies_read() {
        let yaml = "# Libraries.\ngreeting:\n  version: 1.x\n  css:\n    component:\n      css/greeting.css: {}\n  dependencies:\n    - core/drupal\n    - 'core/once' # comment\n'quoted.one':\n  js:\n    js/one.js: { attributes: { dependencies: not/read } }\n  dependencies:\n  - acme/greeting\n  version: VERSION\nflow:\n  dependencies: [core/drupal, 'acme/greeting']\n";
        assert_eq!(
            found(&libraries_file_spans("acme", yaml), yaml),
            [
                ("greeting", "acme/greeting".to_string(), true),
                ("core/drupal", "core/drupal".to_string(), false),
                ("core/once", "core/once".to_string(), false),
                ("quoted.one", "acme/quoted.one".to_string(), true),
                ("acme/greeting", "acme/greeting".to_string(), false),
                ("flow", "acme/flow".to_string(), true),
                ("core/drupal", "core/drupal".to_string(), false),
                ("acme/greeting", "acme/greeting".to_string(), false),
            ]
        );
    }

    #[test]
    fn the_extension_is_the_file_name() {
        assert_eq!(
            libraries_file_extension("file:///s/web/core/core.libraries.yml"),
            Some("core")
        );
        assert_eq!(
            libraries_file_extension("/s/web/themes/custom/x/x.libraries.yml"),
            Some("x")
        );
        assert_eq!(libraries_file_extension("/s/.libraries.yml"), None);
        assert_eq!(libraries_file_extension("/s/x.routing.yml"), None);
    }

    #[test]
    fn a_themes_info_file_attaches_libraries() {
        let yaml = "name: Acme Theme\ntype: theme\nlibraries:\n  - acme_theme/global\n  - \"core/drupal\"\ndependencies:\n  - acme:acme\nlibraries-extend:\n  core/drupal:\n    - acme_theme/extra\n";
        assert_eq!(
            found(&info_file_spans(yaml), yaml),
            [
                ("acme_theme/global", "acme_theme/global".to_string(), false),
                ("core/drupal", "core/drupal".to_string(), false),
            ]
        );
        let flow = "type: 'theme'\nlibraries: [acme_theme/global]\n";
        assert_eq!(found(&info_file_spans(flow), flow).len(), 1);
        let module = "type: module\nlibraries:\n  - acme/greeting\n";
        assert!(info_file_spans(module).is_empty());
    }

    #[test]
    fn only_twig_code_attaches_libraries() {
        let twig = "<p>attach_library('in/html')</p>\n{% verbatim %}{{ attach_library('in/verbatim') }}{% endverbatim %}\n{% do attach_library('acme/tag') %}{{ attach_library('acme/print') }}\n";
        assert_eq!(
            found(&twig_library_spans(twig), twig),
            [
                ("acme/tag", "acme/tag".to_string(), false),
                ("acme/print", "acme/print".to_string(), false),
            ]
        );
    }

    #[test]
    fn completion_starts_in_library_lists_only() {
        let yaml = "a:\n  dependencies:\n    - core/dr";
        assert_eq!(
            libraries_file_name_start(yaml, yaml.len()),
            Some(yaml.len() - 7)
        );
        let yaml = "a:\n  dependencies:\n  - '";
        assert_eq!(
            libraries_file_name_start(yaml, yaml.len()),
            Some(yaml.len())
        );
        let yaml = "a:\n  dependencies: [core/drupal, co";
        assert_eq!(
            libraries_file_name_start(yaml, yaml.len()),
            Some(yaml.len() - 2)
        );
        let yaml = "a:\n  js:\n    - x";
        assert_eq!(libraries_file_name_start(yaml, yaml.len()), None);
        let yaml = "dependencies:\n  - x";
        assert_eq!(libraries_file_name_start(yaml, yaml.len()), None);

        let info = "type: theme\nlibraries:\n  - acme";
        assert_eq!(info_file_name_start(info, info.len()), Some(info.len() - 4));
        let info = "type: theme\ndependencies:\n  - acme";
        assert_eq!(info_file_name_start(info, info.len()), None);
        let info = "type: module\nlibraries:\n  - acme";
        assert_eq!(info_file_name_start(info, info.len()), None);
    }

    #[test]
    fn the_version_is_read_from_the_librarys_block() {
        let yaml = "one:\n  js: {}\ntwo:\n  css:\n    version: no\n  version: \"1.0.1\"\n";
        assert_eq!(library_version(yaml, "two").as_deref(), Some("1.0.1"));
        assert_eq!(library_version(yaml, "one"), None);
    }

    #[test]
    fn templates_attach_libraries() {
        let twig = "{{ attach_library('acme/greeting') }}\n{# attach_library('in/comment') #}\n{{ attach_library( \"core/drupal\" ) }}{{ x.attach_library('no/method') }}{{ my_attach_library('no/other') }}\n";
        assert_eq!(
            found(&twig_library_spans(twig), twig),
            [
                ("acme/greeting", "acme/greeting".to_string(), false),
                ("core/drupal", "core/drupal".to_string(), false),
            ]
        );
        let twig = "{{ attach_library('acme/gr";
        assert_eq!(
            twig_library_name_start(twig, twig.len()),
            Some(twig.len() - 7)
        );
        let twig = "{{ path('acme";
        assert_eq!(twig_library_name_start(twig, twig.len()), None);
        let twig = "{{ x.attach_library('acme";
        assert_eq!(twig_library_name_start(twig, twig.len()), None);
    }
}
