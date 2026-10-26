//! Twig template names: the places in templates that name another one.
//!
//! A template declares the names it is loaded by (see [`super::roots`])
//! itself, with empty [`StringKey`](SymbolKind::StringKey) spans at its
//! start, so a name resolves to the file and the files are found among the
//! declarations like any other key's.
//!
//! A template names others in its `extends`, `include`, `embed`, `use`,
//! `import`, `from`, and `form_theme` tags and its `include()` and
//! `source()` calls, read as text like its route names (see
//! [`crate::symfony::routes`]).  PHP names one through symbol-map
//! extraction.

use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan};
use crate::symfony::translations::{for_each_twig_block, skip_whitespace, string_end};

/// The tags whose expression is a template name.
const TEMPLATE_TAGS: [&str; 7] = [
    "extends",
    "include",
    "embed",
    "use",
    "import",
    "from",
    "form_theme",
];

/// Whether `name` looks like a file a loader could find: a path ending in
/// an extension, with nothing in it a template name never holds.  Which
/// extensions count is the project's (see
/// [`TwigRoots::is_template_name`](super::roots::TwigRoots::is_template_name)).
pub(crate) fn looks_like_template_name(name: &str) -> bool {
    name.rsplit('/')
        .next()
        .and_then(|file| file.rsplit_once('.'))
        .is_some_and(|(stem, extension)| {
            !stem.is_empty()
                && !extension.is_empty()
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        && !name
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || matches!(byte, b'\\' | b'{' | b'}'))
}

/// The plain names (see [`super::roots::plain_relative`]) a template's
/// tags and calls reference, which say where the template lives.
pub(crate) fn template_references(content: &str) -> Vec<String> {
    let mut names: Vec<String> = twig_template_spans(content, |name| {
        looks_like_template_name(name) && super::roots::plain_relative(name).is_some()
    })
    .into_iter()
    .filter_map(|span| match span.kind {
        SymbolKind::StringKey { key, .. } => Some(key),
        _ => None,
    })
    .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// The spans a template declares its `names` with.
pub(crate) fn template_declaration_spans(names: Vec<String>) -> Vec<SymbolSpan> {
    names
        .into_iter()
        .map(|name| template_span(name, 0, 0, true, false))
        .collect()
}

fn template_span(
    key: String,
    start: usize,
    end: usize,
    is_write: bool,
    is_optional: bool,
) -> SymbolSpan {
    SymbolSpan {
        start: start as u32,
        end: end as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::TwigTemplate,
            key,
            is_write,
            is_optional,
        },
    }
}

/// The template names a template's tags and `include()` and `source()`
/// calls read, sorted by offset, keeping the strings `is_name` takes for a
/// template name.
pub(crate) fn twig_template_spans(
    content: &str,
    is_name: impl Fn(&str) -> bool,
) -> Vec<SymbolSpan> {
    let bytes = content.as_bytes();
    let mut spans = Vec::new();
    for_each_twig_block(content, |body, end| {
        if bytes[body - 1] == b'%'
            && let Some((tag, after)) = template_tag(content, body, end)
        {
            if tag == "form_theme" {
                // Every theme the tag lists is applied, after the form it
                // themes.
                push_every_literal(content, after, end, &is_name, &mut spans);
            } else {
                // A name that may be missing, or a list of candidates of
                // which the first that exists is used, does without any
                // one of them existing.
                let optional = matches!(tag, "include" | "embed")
                    && content[after..end].contains("ignore missing");
                push_expression_literals(content, after, end, optional, &is_name, &mut spans);
            }
            return;
        }
        let mut cursor = body;
        while let Some(relative) = memchr::memchr(b'(', &bytes[cursor..end]) {
            let open = cursor + relative;
            cursor = open + 1;
            if template_function_before(&content[body..open]).is_some()
                && let Some(after) =
                    push_expression_literals(content, open + 1, end, false, &is_name, &mut spans)
            {
                cursor = after;
            }
        }
    });
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

/// The template tag a `{% … %}` block whose body spans `body..end` opens
/// with, and where its name ends.
fn template_tag(content: &str, body: usize, end: usize) -> Option<(&'static str, usize)> {
    let bytes = content.as_bytes();
    // `{%-` and `{%~` trim the whitespace around the tag.
    let mut start = body;
    if matches!(bytes.get(start), Some(b'-' | b'~')) {
        start += 1;
    }
    let start = skip_whitespace(bytes, start, end);
    let name_end = start
        + bytes[start..end]
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
            .count();
    let tag = TEMPLATE_TAGS
        .into_iter()
        .find(|tag| *tag == &content[start..name_end])?;
    let continues = bytes
        .get(name_end)
        .is_some_and(|byte| !(byte.is_ascii_whitespace() || matches!(byte, b'(' | b'\'' | b'"')));
    (!continues).then_some((tag, name_end))
}

/// The `include` or `source` function whose name `before` ends with, as
/// long as it is not a method of something (`block.include(`).
fn template_function_before(before: &str) -> Option<&str> {
    let trimmed = before.trim_end();
    let name_start = trimmed
        .bytes()
        .rposition(|byte| !(byte.is_ascii_alphanumeric() || byte == b'_'))
        .map_or(0, |index| index + 1);
    let name = &trimmed[name_start..];
    let preceded_by_dot = trimmed[..name_start].ends_with('.');
    (matches!(name, "include" | "source") && !preceded_by_dot).then_some(name)
}

/// Push the template names the expression starting at `start` spells: a
/// string, or a list of strings.  Returns where the expression's names
/// end, when it spells any.
fn push_expression_literals(
    content: &str,
    start: usize,
    end: usize,
    optional: bool,
    is_name: &impl Fn(&str) -> bool,
    spans: &mut Vec<SymbolSpan>,
) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut cursor = skip_whitespace(bytes, start, end);
    // `{% include('x.html.twig') %}` parenthesises its expression.
    while cursor < end && bytes[cursor] == b'(' {
        cursor = skip_whitespace(bytes, cursor + 1, end);
    }
    if cursor >= end {
        return None;
    }
    match bytes[cursor] {
        quote @ (b'\'' | b'"') => {
            let close = string_end(bytes, cursor + 1, quote).filter(|close| *close <= end)?;
            push_literal(content, cursor + 1, close, optional, is_name, spans);
            Some(close + 1)
        }
        b'[' => {
            cursor += 1;
            loop {
                cursor = skip_whitespace(bytes, cursor, end);
                let Some(quote @ (b'\'' | b'"')) = bytes.get(cursor).copied() else {
                    break;
                };
                let close = string_end(bytes, cursor + 1, quote).filter(|close| *close <= end)?;
                push_literal(content, cursor + 1, close, true, is_name, spans);
                cursor = skip_whitespace(bytes, close + 1, end);
                if bytes.get(cursor) != Some(&b',') {
                    break;
                }
                cursor += 1;
            }
            Some(cursor)
        }
        _ => None,
    }
}

/// Push every template name a string between `start` and `end` holds.
fn push_every_literal(
    content: &str,
    start: usize,
    end: usize,
    is_name: &impl Fn(&str) -> bool,
    spans: &mut Vec<SymbolSpan>,
) {
    let bytes = content.as_bytes();
    let mut cursor = start;
    while cursor < end {
        let quote = bytes[cursor];
        if !matches!(quote, b'\'' | b'"') {
            cursor += 1;
            continue;
        }
        let Some(close) = string_end(bytes, cursor + 1, quote).filter(|close| *close <= end) else {
            return;
        };
        push_literal(content, cursor + 1, close, false, is_name, spans);
        cursor = close + 1;
    }
}

fn push_literal(
    content: &str,
    start: usize,
    end: usize,
    optional: bool,
    is_name: &impl Fn(&str) -> bool,
    spans: &mut Vec<SymbolSpan>,
) {
    let name = &content[start..end];
    if is_name(name) {
        spans.push(template_span(name.to_string(), start, end, false, optional));
    }
}

/// Where the template name the cursor is typing in a template starts: the
/// cursor is in a string that is the expression of a template tag, or the
/// first argument of `include()` or `source()`.
pub(crate) fn twig_template_name_start(content: &str, cursor: usize) -> Option<usize> {
    let before = content.get(..cursor)?;
    let name_start = before
        .bytes()
        .rposition(|byte| {
            !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'/' | b'@'))
        })
        .map_or(0, |index| index + 1);
    let quote_at = name_start.checked_sub(1)?;
    if !matches!(before.as_bytes()[quote_at], b'\'' | b'"') {
        return None;
    }
    let head = before[..quote_at].trim_end();
    let head = head.strip_suffix('[').map_or(head, str::trim_end);
    if let Some(before_paren) = head.strip_suffix('(')
        && template_function_before(before_paren).is_some()
    {
        return Some(name_start);
    }

    let tag_open = before[..quote_at].rfind("{%")?;
    if before[tag_open..quote_at].contains("%}") {
        return None;
    }
    let (tag, after) = template_tag(content, tag_open + 2, quote_at)?;
    let expression = before[after..quote_at]
        .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '(' || c == '[');
    (tag == "form_theme" || expression.is_empty()).then_some(name_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(found: &[SymbolSpan], content: &str) -> Vec<(String, bool)> {
        found
            .iter()
            .map(|span| {
                let SymbolKind::StringKey {
                    key, is_optional, ..
                } = &span.kind
                else {
                    unreachable!()
                };
                assert_eq!(
                    &content[span.start as usize..span.end as usize],
                    key,
                    "span must cover the name"
                );
                (key.clone(), *is_optional)
            })
            .collect()
    }

    fn twig_name(name: &str) -> bool {
        looks_like_template_name(name) && name.ends_with(".twig")
    }

    #[test]
    fn tags_and_functions_name_templates() {
        let twig = "{# {% include 'commented.html.twig' %} #}\n{% extends 'base.html.twig' %}\n{%- include \"partials/card.html.twig\" with {title: 'x.html.twig'} only -%}\n{% embed ['a.html.twig', 'b.html.twig'] %}{% endembed %}\n{% include 'maybe.html.twig' ignore missing %}\n{% use 'blocks.html.twig' %}{% import 'macros.html.twig' as m %}{% from '@Acme/forms.html.twig' import field %}\n{% form_theme form 'theme_a.html.twig' 'theme_b.html.twig' %}\n{{ include('inc.html.twig', {x: 1}) }}{{ source(\"raw.html.twig\") }}{{ block.include('no.html.twig') }}\n{% include template_name %}{% set title = 'plain.html.twig' %}<a href='html.twig'>\n";
        assert_eq!(
            names(&twig_template_spans(twig, twig_name), twig),
            vec![
                ("base.html.twig".to_string(), false),
                ("partials/card.html.twig".to_string(), false),
                ("a.html.twig".to_string(), true),
                ("b.html.twig".to_string(), true),
                ("maybe.html.twig".to_string(), true),
                ("blocks.html.twig".to_string(), false),
                ("macros.html.twig".to_string(), false),
                ("@Acme/forms.html.twig".to_string(), false),
                ("theme_a.html.twig".to_string(), false),
                ("theme_b.html.twig".to_string(), false),
                ("inc.html.twig".to_string(), false),
                ("raw.html.twig".to_string(), false),
            ]
        );
    }

    #[test]
    fn text_outside_ascii_inside_a_block_is_stepped_over() {
        let twig = "{{ přehled|upper }}{% include 'a.html.twig' %}";
        assert_eq!(
            names(&twig_template_spans(twig, twig_name), twig),
            vec![("a.html.twig".to_string(), false)]
        );
    }

    #[test]
    fn references_are_the_plain_names_a_template_loads() {
        let twig = "{% extends \"index.html\" %}{% include 'menu.html' with {'menu': x} %}{% include '@admin/x.html' %}{% include '../up.html' %}{% include 'menu.html' %}{% set y = 'z.html' %}{{ include('partials/a.html.twig') }}";
        assert_eq!(
            template_references(twig),
            vec![
                "index.html".to_string(),
                "menu.html".to_string(),
                "partials/a.html.twig".to_string(),
            ]
        );
    }

    #[test]
    fn completion_starts_after_the_quote() {
        let at = |twig: &str| twig_template_name_start(twig, twig.len());
        assert_eq!(at("{% extends 'ba"), Some(12));
        assert_eq!(at("{% include ['a.html.twig', 'b"), None);
        assert_eq!(at("{% include ['pa"), Some(13));
        assert_eq!(at("{{ include(\"partials/"), Some(12));
        assert_eq!(at("{% form_theme form 'th"), Some(20));
        assert_eq!(at("{% set x = 'ba"), None);
        assert_eq!(at("{{ path('ba"), None);
        assert_eq!(at("{% extends 'x' %} 'ba"), None);
    }
}
