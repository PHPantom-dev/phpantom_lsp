//! Translation keys and domains in Symfony's catalogues and Twig templates.
//!
//! A message is looked up in one domain, so a translation key is only the
//! same key as another in the same domain: `title` in `messages` and
//! `title` in `admin` are unrelated.  A [`StringKeyKind::SymfonyTranslation`]
//! span stores both, joined by [`translation_key`], and every reader splits
//! them again with [`split_translation_key`].
//!
//! Like the container configuration (see [`super::resources`]), catalogues
//! and templates are read as text.  A catalogue is a file under a
//! `translations/` directory named `<domain>.<locale>.<format>`, whose
//! domain is its name's first part.  A YAML catalogue declares every leaf of
//! its nested keys, dotted together, and an XLIFF one every unit, by its
//! `resname` (`name` in XLIFF 2) or else its source text.  A template reads
//! a key through the `trans` filter, in the domain the filter names or
//! else the template's `trans_default_domain`.

use super::resources::{yaml_content_before_comment, yaml_mapping_key};
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan};

/// The domain a message is looked up in when the call names none.
pub(crate) const DEFAULT_DOMAIN: &str = "messages";

/// Separates the domain from the message in a translation key.  A domain is
/// a file name's first part, which never holds it.
const DOMAIN_SEPARATOR: char = '\u{1f}';

/// The key a [`StringKeyKind::SymfonyTranslation`] span stores for `message`
/// in `domain`.
pub(crate) fn translation_key(domain: &str, message: &str) -> String {
    let mut key = String::with_capacity(domain.len() + 1 + message.len());
    key.push_str(domain);
    key.push(DOMAIN_SEPARATOR);
    key.push_str(message);
    key
}

/// The domain and the message a translation key was made from.
pub(crate) fn split_translation_key(key: &str) -> (&str, &str) {
    key.split_once(DOMAIN_SEPARATOR)
        .unwrap_or((DEFAULT_DOMAIN, key))
}

/// Whether `domain` can name a catalogue: what a file name's first part
/// holds.
pub(crate) fn is_domain_name(domain: &str) -> bool {
    !domain.is_empty()
        && domain
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

/// Whether `message` can be a translation key.  A key may be a whole
/// sentence, but not one spread over several lines.
fn is_message(message: &str) -> bool {
    !message.trim().is_empty() && !message.contains(['\n', '\r', DOMAIN_SEPARATOR])
}

/// The directories and the file name of `uri`, without a query or
/// fragment.
fn file_name(uri: &str) -> Option<(&str, &str)> {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    let (directories, name) = path.rsplit_once('/')?;
    Some((directories, name))
}

/// Whether `uri` is an XLIFF document.
pub(crate) fn is_xliff_uri(uri: &str) -> bool {
    file_name(uri).is_some_and(|(_, name)| {
        let name = name.to_ascii_lowercase();
        name.ends_with(".xlf") || name.ends_with(".xliff")
    })
}

/// The domain the catalogue `uri` holds messages for, when it is one Symfony
/// loads: a YAML or XLIFF file under a `translations/` directory, named
/// `<domain>.<locale>.<format>`.  An ICU catalogue (`messages+intl-icu`)
/// belongs to the domain it is named after.
pub(crate) fn catalogue_domain(uri: &str) -> Option<&str> {
    let (directories, name) = file_name(uri)?;
    if !directories
        .split('/')
        .any(|segment| segment == "translations")
    {
        return None;
    }
    let (rest, format) = name.rsplit_once('.')?;
    if !matches!(
        format.to_ascii_lowercase().as_str(),
        "yaml" | "yml" | "xlf" | "xliff"
    ) {
        return None;
    }
    let (domain, _locale) = rest.rsplit_once('.')?;
    let domain = ["+intl-icu", "%2Bintl-icu", "%2bintl-icu"]
        .iter()
        .find_map(|suffix| domain.strip_suffix(suffix))
        .unwrap_or(domain);
    is_domain_name(domain).then_some(domain)
}

/// The translation-key spans of one catalogue, sorted by offset.
pub(crate) fn catalogue_spans(uri: &str, content: &str) -> Vec<SymbolSpan> {
    let Some(domain) = catalogue_domain(uri) else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    if is_xliff_uri(uri) {
        scan_xliff(content, domain, &mut spans);
    } else {
        scan_yaml(content, domain, &mut spans);
    }
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

fn translation_span(
    domain: &str,
    message: &str,
    start: usize,
    end: usize,
    is_write: bool,
) -> SymbolSpan {
    SymbolSpan {
        start: start as u32,
        end: end as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::SymfonyTranslation,
            key: translation_key(domain, message),
            is_write,
            is_optional: false,
        },
    }
}

fn domain_span(domain: &str, start: usize) -> SymbolSpan {
    SymbolSpan {
        start: start as u32,
        end: (start + domain.len()) as u32,
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::TranslationDomain,
            key: domain.to_string(),
            is_write: false,
            is_optional: false,
        },
    }
}

// ─── YAML ───────────────────────────────────────────────────────────────────

/// The keys of a YAML catalogue.  A key with nothing after its colon and
/// more-indented lines below it is a group, whose children's keys are
/// dotted onto its own; any other key is a message.  The lines of a block
/// scalar (`key: |`) are its value, not keys of their own.
fn scan_yaml(content: &str, domain: &str, spans: &mut Vec<SymbolSpan>) {
    let lines = crate::framework::line_offsets(content);
    // The indentation and key of each group the scan is inside.
    let mut groups: Vec<(usize, &str)> = Vec::new();
    // Lines indented deeper than this belong to a block scalar.
    let mut block_scalar: Option<usize> = None;
    for (index, &(line_start, line)) in lines.iter().enumerate() {
        let semantic = yaml_content_before_comment(line);
        if semantic.trim().is_empty() {
            continue;
        }
        let indent = crate::framework::leading_spaces(semantic);
        if let Some(parent) = block_scalar {
            if indent > parent {
                continue;
            }
            block_scalar = None;
        }
        if semantic.trim_start().starts_with('-') {
            continue;
        }
        let Some((raw_key, raw_start)) = yaml_mapping_key(semantic, line_start) else {
            continue;
        };
        while groups.last().is_some_and(|(parent, _)| *parent >= indent) {
            groups.pop();
        }
        let (key, (quote, _)) = crate::framework::strip_yaml_quotes(raw_key);
        let colon = raw_start - line_start + raw_key.len();
        let value = semantic[colon..]
            .find(':')
            .map_or("", |n| semantic[colon + n + 1..].trim());

        if value.is_empty() {
            let has_children = lines[index + 1..]
                .iter()
                .map(|(_, next)| yaml_content_before_comment(next))
                .find(|next| !next.trim().is_empty())
                .is_some_and(|next| crate::framework::leading_spaces(next) > indent);
            if has_children {
                groups.push((indent, key));
                continue;
            }
        } else if value.starts_with(['|', '>']) {
            block_scalar = Some(indent);
        }

        let message = if groups.is_empty() {
            key.to_string()
        } else {
            let mut message = String::new();
            for (_, group) in &groups {
                message.push_str(group);
                message.push('.');
            }
            message.push_str(key);
            message
        };
        if is_message(&message) {
            let start = raw_start + quote;
            spans.push(translation_span(
                domain,
                &message,
                start,
                start + key.len(),
                true,
            ));
        }
    }
}

// ─── XLIFF ──────────────────────────────────────────────────────────────────

/// The units of an XLIFF catalogue, by the name Symfony's loader gives
/// each: its `resname` (XLIFF 1.2) or `name` (XLIFF 2) attribute, else the
/// text of its first `<source>`.
fn scan_xliff(content: &str, domain: &str, spans: &mut Vec<SymbolSpan>) {
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
        let attribute = if is_tag(tag, "trans-unit") {
            "resname"
        } else if is_tag(tag, "unit") {
            "name"
        } else {
            continue;
        };
        let named = crate::framework::xml_attr_value(tag, tag_start, &[attribute])
            .filter(|(name, _, _)| !name.is_empty());
        let Some((message, start, end)) = named.or_else(|| first_source(content, tag_end)) else {
            continue;
        };
        if is_message(&message) {
            spans.push(translation_span(domain, &message, start, end, true));
        }
    }
}

/// Whether `tag` opens an element called `name`.
fn is_tag(tag: &str, name: &str) -> bool {
    tag.strip_prefix('<')
        .and_then(|rest| rest.strip_prefix(name))
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_whitespace() || c == '>'))
}

/// The text of the first `<source>` after a unit's opening tag, and where
/// it is, unless the unit ends first.
fn first_source(content: &str, unit_start: usize) -> Option<(String, usize, usize)> {
    let rest = &content[unit_start..];
    let source = rest.find("<source")?;
    if rest[..source].contains("</trans-unit>") || rest[..source].contains("</unit>") {
        return None;
    }
    let text_start = source + rest[source..].find('>')? + 1;
    let text_end = text_start + rest[text_start..].find("</source>")?;
    let raw = &rest[text_start..text_end];
    let text = raw.trim();
    let start = unit_start + text_start + (raw.len() - raw.trim_start().len());
    // An escaped name is not the text the span covers.
    (!text.contains('&') && !text.contains('<'))
        .then(|| (text.to_string(), start, start + text.len()))
}

// ─── Twig ───────────────────────────────────────────────────────────────────

/// The range of one string literal's contents inside a Twig tag or
/// expression.
struct TwigString {
    start: usize,
    end: usize,
}

/// Visit every `{{ … }}` and `{% … %}` block of a template, in order, with
/// its byte range.  Comments are skipped, and so is everything outside a
/// block, where a quote is only HTML.
pub(crate) fn for_each_twig_block(content: &str, mut visit: impl FnMut(usize, usize)) {
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    while let Some(relative) = memchr::memchr(b'{', &bytes[cursor..]) {
        let open = cursor + relative;
        let close = match bytes.get(open + 1) {
            Some(b'#') => "#}",
            Some(b'{') => "}}",
            Some(b'%') => "%}",
            _ => {
                cursor = open + 1;
                continue;
            }
        };
        let body = open + 2;
        let end = twig_block_end(content, body, close).unwrap_or(bytes.len());
        if close != "#}" {
            visit(body, end);
        }
        cursor = (end + 2).min(bytes.len());
    }
}

/// Where the block whose body starts at `body` closes with `close`, past
/// any string literal that holds the closing characters.
fn twig_block_end(content: &str, body: usize, close: &str) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut cursor = body;
    while cursor < bytes.len() {
        match bytes[cursor] {
            quote @ (b'\'' | b'"') if close != "#}" => {
                cursor = string_end(bytes, cursor + 1, quote)? + 1;
            }
            _ if bytes[cursor..].starts_with(close.as_bytes()) => return Some(cursor),
            _ => cursor += 1,
        }
    }
    None
}

/// The offset of the quote closing a string whose contents start at
/// `start`.
pub(crate) fn string_end(bytes: &[u8], start: usize, quote: u8) -> Option<usize> {
    let mut cursor = start;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            byte if byte == quote => return Some(cursor),
            _ => cursor += 1,
        }
    }
    None
}

pub(crate) fn skip_whitespace(bytes: &[u8], mut cursor: usize, end: usize) -> usize {
    while cursor < end && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

/// Whether `trans` is the filter name at `cursor`, and where it ends.
fn trans_filter_at(content: &str, cursor: usize) -> Option<usize> {
    let rest = content.get(cursor..)?;
    let after = rest.strip_prefix("trans")?;
    let continues = after
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    (!continues).then_some(cursor + "trans".len())
}

/// The domain literal of a `trans` filter's arguments, written from
/// `open` (the `(`): its second positional argument, or `domain:` (or
/// `domain =`) by name.
fn filter_domain(content: &str, open: usize, end: usize) -> Option<TwigString> {
    let bytes = content.as_bytes();
    let mut cursor = open + 1;
    let mut depth = 0u32;
    let mut index = 0usize;
    let mut argument_start = cursor;
    while cursor < end {
        match bytes[cursor] {
            quote @ (b'\'' | b'"') => {
                let close = string_end(bytes, cursor + 1, quote)?;
                if depth == 0 {
                    let before = content[argument_start..cursor].trim();
                    let named = before
                        .strip_prefix("domain")
                        .map(str::trim_start)
                        .is_some_and(|rest| rest == ":" || rest == "=");
                    if named || (index == 1 && before.is_empty()) {
                        return Some(TwigString {
                            start: cursor + 1,
                            end: close,
                        });
                    }
                }
                cursor = close + 1;
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' if depth == 0 => return None,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                index += 1;
                argument_start = cursor + 1;
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

/// The translation keys and domains a template names, sorted by offset:
/// a string the `trans` filter is applied to, the domain the filter names,
/// and the domain of a `trans_default_domain` tag, which every filter after
/// it that names none translates in.
pub(crate) fn twig_translation_spans(content: &str) -> Vec<SymbolSpan> {
    let bytes = content.as_bytes();
    let mut spans = Vec::new();
    let mut default_domain: Option<&str> = None;
    for_each_twig_block(content, |body, end| {
        if let Some(domain) = default_domain_tag(content, body, end) {
            let start = domain.as_ptr() as usize - content.as_ptr() as usize;
            default_domain = Some(domain);
            spans.push(domain_span(domain, start));
            return;
        }
        let mut cursor = body;
        while cursor < end {
            let quote = bytes[cursor];
            if !matches!(quote, b'\'' | b'"') {
                cursor += 1;
                continue;
            }
            let Some(close) = string_end(bytes, cursor + 1, quote) else {
                return;
            };
            let literal = TwigString {
                start: cursor + 1,
                end: close,
            };
            cursor = close + 1;
            let pipe = skip_whitespace(bytes, cursor, end);
            if bytes.get(pipe) != Some(&b'|') {
                continue;
            }
            let name = skip_whitespace(bytes, pipe + 1, end);
            let Some(filter_end) = trans_filter_at(content, name) else {
                continue;
            };
            let open = skip_whitespace(bytes, filter_end, end);
            let named_domain = (bytes.get(open) == Some(&b'('))
                .then(|| filter_domain(content, open, end))
                .flatten()
                .filter(|domain| is_domain_name(&content[domain.start..domain.end]));
            let domain = match &named_domain {
                Some(domain) => &content[domain.start..domain.end],
                None => default_domain.unwrap_or(DEFAULT_DOMAIN),
            };
            let message = &content[literal.start..literal.end];
            // A key with an escape in it is not the text the span covers.
            if is_message(message) && !message.contains('\\') {
                spans.push(translation_span(
                    domain,
                    message,
                    literal.start,
                    literal.end,
                    false,
                ));
            }
            if let Some(named) = named_domain {
                spans.push(domain_span(domain, named.start));
            }
            cursor = filter_end;
        }
    });
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

/// The domain of a `{% trans_default_domain '…' %}` tag whose body spans
/// `body..end`, when it is one.
fn default_domain_tag(content: &str, body: usize, end: usize) -> Option<&str> {
    let bytes = content.as_bytes();
    let tag = content[body..end].trim_start();
    let rest = tag.strip_prefix("trans_default_domain")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let quote = skip_whitespace(bytes, end - rest.len(), end);
    let quote_byte @ (b'\'' | b'"') = *bytes.get(quote)? else {
        return None;
    };
    let close = string_end(bytes, quote + 1, quote_byte)?;
    let domain = &content[quote + 1..close];
    (close <= end && is_domain_name(domain)).then_some(domain)
}

/// The domain a template's `trans` filter translates a string at `offset`
/// in when it names none: that of the last `trans_default_domain` before
/// it.
fn twig_default_domain_at(content: &str, offset: usize) -> Option<&str> {
    let mut domain = None;
    for_each_twig_block(&content[..offset], |body, end| {
        if let Some(found) = default_domain_tag(content, body, end) {
            domain = Some(found);
        }
    });
    domain
}

/// Where the translation key the cursor is typing in a template starts,
/// and the domain it is looked up in: the cursor is in a string the `trans`
/// filter is applied to.
pub(crate) fn twig_translation_completion(content: &str, cursor: usize) -> Option<(usize, String)> {
    let bytes = content.as_bytes();
    let line_start = content[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let before = &content[line_start..cursor];
    let quote_at = line_start + before.rfind(['\'', '"'])?;
    let quote = bytes[quote_at];
    // The quote has to open the string, not close one.
    if before[..quote_at - line_start]
        .bytes()
        .filter(|byte| *byte == quote)
        .count()
        % 2
        != 0
    {
        return None;
    }
    let close = string_end(bytes, quote_at + 1, quote)?;
    if content[cursor..close].contains('\n') {
        return None;
    }
    let line_end = content[close..]
        .find('\n')
        .map_or(content.len(), |index| close + index);
    let pipe = skip_whitespace(bytes, close + 1, line_end);
    if bytes.get(pipe) != Some(&b'|') {
        return None;
    }
    let name = skip_whitespace(bytes, pipe + 1, line_end);
    let filter_end = trans_filter_at(content, name)?;
    let open = skip_whitespace(bytes, filter_end, line_end);
    let domain = (bytes.get(open) == Some(&b'('))
        .then(|| filter_domain(content, open, line_end))
        .flatten()
        .map(|domain| content[domain.start..domain.end].to_string())
        .filter(|domain| is_domain_name(domain))
        .or_else(|| twig_default_domain_at(content, quote_at).map(str::to_string))
        .unwrap_or_else(|| DEFAULT_DOMAIN.to_string());
    Some((quote_at + 1, domain))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(found: &[SymbolSpan], content: &str) -> Vec<(String, String, bool)> {
        found
            .iter()
            .map(|span| {
                let SymbolKind::StringKey {
                    kind,
                    key,
                    is_write,
                    ..
                } = &span.kind
                else {
                    unreachable!()
                };
                let text = content[span.start as usize..span.end as usize].to_string();
                let key = if *kind == StringKeyKind::SymfonyTranslation {
                    let (domain, message) = split_translation_key(key);
                    format!("{domain}/{message}")
                } else {
                    format!("domain {key}")
                };
                (key, text, *is_write)
            })
            .collect()
    }

    #[test]
    fn catalogues_are_named_by_domain_and_locale() {
        assert_eq!(
            catalogue_domain("file:///p/translations/messages.en.yaml"),
            Some("messages")
        );
        assert_eq!(
            catalogue_domain("file:///p/translations/admin.fr_FR.xlf"),
            Some("admin")
        );
        assert_eq!(
            catalogue_domain("file:///p/translations/messages+intl-icu.en.yaml"),
            Some("messages")
        );
        assert_eq!(
            catalogue_domain("file:///p/src/Resources/translations/app.blog.de.yml"),
            Some("app.blog")
        );
        assert_eq!(
            catalogue_domain("file:///p/translations/messages.yaml"),
            None
        );
        assert_eq!(catalogue_domain("file:///p/config/messages.en.yaml"), None);
        assert_eq!(
            catalogue_domain("file:///p/translations/messages.en.php"),
            None
        );
    }

    #[test]
    fn yaml_catalogues_declare_their_flattened_leaves() {
        let yaml = "# greeting\ndemo:\n    title: Symfony\n    nested:\n        deep: 'Deep'\n    empty:\n'Hello world': Bonjour\nblock: |\n    not: a key\n    still: text\nafter: x\n";
        let found = catalogue_spans("file:///p/translations/messages.en.yaml", yaml);
        assert_eq!(
            keys(&found, yaml),
            vec![
                ("messages/demo.title".to_string(), "title".to_string(), true),
                (
                    "messages/demo.nested.deep".to_string(),
                    "deep".to_string(),
                    true
                ),
                ("messages/demo.empty".to_string(), "empty".to_string(), true),
                (
                    "messages/Hello world".to_string(),
                    "Hello world".to_string(),
                    true
                ),
                ("messages/block".to_string(), "block".to_string(), true),
                ("messages/after".to_string(), "after".to_string(), true),
            ]
        );
    }

    #[test]
    fn xliff_catalogues_declare_their_units() {
        let xliff = r#"<?xml version="1.0"?>
<xliff version="1.2">
  <file source-language="en" datatype="plaintext" original="file.ext">
    <body>
      <!-- <trans-unit id="x" resname="commented"> -->
      <trans-unit id="1" resname="user.invalid_email">
        <source>user.invalid_email</source>
      </trans-unit>
      <trans-unit id="2">
        <source> Hello world </source>
        <target>Bonjour</target>
      </trans-unit>
      <trans-unit id="3"><target>no source</target></trans-unit>
      <unit id="4" name="v2.name"><segment><source>x</source></segment></unit>
    </body>
  </file>
</xliff>
"#;
        let found = catalogue_spans("file:///p/translations/validators.en.xlf", xliff);
        assert_eq!(
            keys(&found, xliff),
            vec![
                (
                    "validators/user.invalid_email".to_string(),
                    "user.invalid_email".to_string(),
                    true
                ),
                (
                    "validators/Hello world".to_string(),
                    "Hello world".to_string(),
                    true
                ),
                (
                    "validators/v2.name".to_string(),
                    "v2.name".to_string(),
                    true
                ),
            ]
        );
    }

    #[test]
    fn twig_reads_the_trans_filter_in_its_domain() {
        let twig = "{# 'commented'|trans #}\n<p title=\"don't\">{{ 'demo.title'|trans }}</p>\n{{ 'admin.title' | trans({'%n%': 1}, 'admin') }}\n{% trans_default_domain 'forms' %}\n{{ \"form.label\"|trans }}{{ 'named'|trans(domain: 'other') }}{{ 'plain'|upper }}{{ 'x'|translate }}\n";
        assert_eq!(
            keys(&twig_translation_spans(twig), twig),
            vec![
                (
                    "messages/demo.title".to_string(),
                    "demo.title".to_string(),
                    false
                ),
                (
                    "admin/admin.title".to_string(),
                    "admin.title".to_string(),
                    false
                ),
                ("domain admin".to_string(), "admin".to_string(), false),
                ("domain forms".to_string(), "forms".to_string(), false),
                (
                    "forms/form.label".to_string(),
                    "form.label".to_string(),
                    false
                ),
                ("other/named".to_string(), "named".to_string(), false),
                ("domain other".to_string(), "other".to_string(), false),
            ]
        );
    }

    #[test]
    fn twig_completion_needs_the_trans_filter() {
        let twig = "{{ 'demo.ti'|trans }}";
        assert_eq!(
            twig_translation_completion(twig, 11),
            Some((4, "messages".to_string()))
        );
        let twig =
            "{% trans_default_domain 'admin' %}\n{{ 'x'|trans }}{{ 'y'|trans({}, 'forms') }}";
        let cursor = twig.find("'x").unwrap() + 2;
        assert_eq!(
            twig_translation_completion(twig, cursor),
            Some((cursor - 1, "admin".to_string()))
        );
        let cursor = twig.find("'y").unwrap() + 2;
        assert_eq!(
            twig_translation_completion(twig, cursor),
            Some((cursor - 1, "forms".to_string()))
        );
        let twig = "{{ 'demo.ti'|upper }}";
        assert_eq!(twig_translation_completion(twig, 11), None);
    }
}
