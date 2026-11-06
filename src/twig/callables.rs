//! Twig filters and functions: the ones extensions declare, and the places
//! templates apply and call them.
//!
//! An extension declares a filter with `new TwigFilter('price', …)`, or an
//! `#[AsTwigFilter('price')]` method, and a function the same way.  The
//! project's own declarations are read with its symbol maps (see
//! `symbol_map::extraction::twig`), which record the PHP callable behind
//! each.  Twig's own, and the ones of the packages the project installs
//! (Symfony's bridge, Twig's extras), are declared the same way by classes
//! under `vendor/`, which the workspace index does not hold: the classes
//! named like an extension are read from disk once, and kept until the
//! dependencies change.
//!
//! A template applies a filter with `|price` (and in an `{% apply %}` tag)
//! and calls a function with `icon(…)`, read as text like its template names
//! (see [`super::templates`]).  A macro the template defines or imports is
//! called the same way, and is not a function.

use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::Location;

use crate::Backend;
use crate::atom::Atom;
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan, TwigCallableTarget};
use crate::symfony::translations::{for_each_twig_block, skip_whitespace, string_end};

/// The class Twig's own filters and functions are declared by.  Until it is
/// read, a name nothing declares may be one of Twig's.
const CORE_EXTENSION: &str = "Twig\\Extension\\CoreExtension";

/// The words of Twig's expression language that a `(` may follow without
/// making them a function call.
const OPERATOR_WORDS: [&str; 15] = [
    "and", "or", "not", "in", "is", "xor", "matches", "with", "starts", "ends", "has", "some",
    "every", "as", "if",
];

/// One filter or function an extension declares.
#[derive(Debug, Clone)]
pub(crate) struct TwigCallable {
    pub(crate) kind: StringKeyKind,
    pub(crate) name: Atom,
    /// The file that declares it, and the byte range of the name there.
    pub(crate) uri: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) target: TwigCallableTarget,
}

/// The filters and functions installed packages declare.
#[derive(Debug, Default)]
pub(crate) struct VendorCallables {
    callables: Vec<TwigCallable>,
    /// Whether Twig's own extension was among them.
    has_core: bool,
}

/// Every filter and function the project can use: its own first, then the
/// ones its packages declare.
#[derive(Debug, Default)]
pub(crate) struct TwigCallables {
    callables: Vec<TwigCallable>,
    has_core: bool,
}

impl TwigCallables {
    /// The declarations of the `kind` callable `name`: the ones of that very
    /// name, or else the patterns (`*_path`) it matches.
    pub(crate) fn matching(&self, kind: StringKeyKind, name: &str) -> Vec<&TwigCallable> {
        let of_kind = || self.callables.iter().filter(move |c| c.kind == kind);
        let exact: Vec<&TwigCallable> = of_kind().filter(|c| &*c.name == name).collect();
        if !exact.is_empty() {
            return exact;
        }
        of_kind()
            .filter(|c| c.name.contains('*') && matches_pattern(&c.name, name))
            .collect()
    }

    /// Whether anything declares the `kind` callable `name`, or `None` when
    /// that cannot be told: Twig's own extension was not found, so the
    /// packages were not read.
    pub(crate) fn declares(&self, kind: StringKeyKind, name: &str) -> Option<bool> {
        self.has_core.then(|| !self.matching(kind, name).is_empty())
    }

    /// The names of every `kind` callable, sorted, leaving out patterns.
    pub(crate) fn names(&self, kind: StringKeyKind) -> Vec<String> {
        let mut names: Vec<String> = self
            .callables
            .iter()
            .filter(|c| c.kind == kind && !c.name.contains('*'))
            .map(|c| c.name.to_string())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

/// Whether `name` fits `pattern`, each of whose `*`s stands for a part of
/// one or more characters.
fn matches_pattern(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let Some(first) = parts.next() else {
        return false;
    };
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    for (index, part) in parts.iter().enumerate() {
        let is_last = index + 1 == parts.len();
        if is_last {
            return rest.len() > part.len() && rest.ends_with(part);
        }
        // The `*` before this part takes at least one character.
        let Some(found) = rest.get(1..).and_then(|after| after.find(part)) else {
            return false;
        };
        rest = &rest[1 + found + part.len()..];
    }
    rest.is_empty()
}

/// Whether the class `fqn` is named like a Twig extension.
fn is_extension_name(fqn: &str) -> bool {
    let short = fqn.rsplit('\\').next().unwrap_or(fqn);
    short.len() >= "Extension".len()
        && short[short.len() - "Extension".len()..].eq_ignore_ascii_case("Extension")
}

/// The filters and functions the PHP file at `uri` declares, read from
/// `content`.
fn read_extension_file(uri: &str, content: &str) -> Vec<TwigCallable> {
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(uri.as_bytes());
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let resolver = mago_names::resolver::NameResolver::new(&arena);
    let names = crate::names::OwnedResolvedNames::from_resolved(&resolver.resolve(program));
    let map = crate::symbol_map::extraction::extract_symbol_map_for_file(
        program,
        content,
        &names,
        crate::symbol_map::extraction::Frameworks {
            symfony: false,
            service_container: false,
            drupal: false,
            twig: true,
        },
        None,
        crate::drupal::hooks::DrupalFile::Other,
    );
    map.twig_callables
        .into_iter()
        .map(|site| TwigCallable {
            kind: site.kind,
            name: site.name,
            uri: uri.to_string(),
            start: site.start,
            end: site.end,
            target: site.target,
        })
        .collect()
}

impl Backend {
    /// The filters and functions the installed packages declare.
    fn vendor_twig_callables(&self) -> Arc<VendorCallables> {
        if let Some(known) = self.twig.vendor_callables.read().as_ref() {
            return Arc::clone(known);
        }
        let prefixes = self.workspace.vendor_uri_prefixes.lock().clone();
        let mut uris: Vec<(String, bool)> = self
            .symbols
            .fqn_uri_index
            .read()
            .iter()
            .filter(|(fqn, uri)| {
                is_extension_name(fqn)
                    && prefixes
                        .iter()
                        .any(|prefix| uri.starts_with(prefix.as_str()))
            })
            .map(|(fqn, uri)| (uri.clone(), fqn.eq_ignore_ascii_case(CORE_EXTENSION)))
            .collect();
        uris.sort_unstable();
        uris.dedup_by(|b, a| {
            a.1 |= b.1;
            a.0 == b.0
        });
        let finder = memchr::memmem::Finder::new(b"TwigF");
        let mut found = VendorCallables::default();
        for (uri, is_core) in uris {
            let Some(content) =
                crate::util::uri_to_path(&uri).and_then(|path| std::fs::read_to_string(path).ok())
            else {
                continue;
            };
            if finder.find(content.as_bytes()).is_none() {
                continue;
            }
            let declared = read_extension_file(&uri, &content);
            found.has_core |= is_core && !declared.is_empty();
            found.callables.extend(declared);
        }
        let found = Arc::new(found);
        *self.twig.vendor_callables.write() = Some(Arc::clone(&found));
        found
    }

    /// Forget what the installed packages declare, once they changed.
    pub(crate) fn forget_vendor_twig_callables(&self) {
        *self.twig.vendor_callables.write() = None;
    }

    /// Every filter and function the project can use.
    ///
    /// Must not run under the file-state read lock: the project's own
    /// declarations are in files only the workspace walk may have read,
    /// which it waits for.
    pub(crate) fn twig_callables(&self) -> Arc<TwigCallables> {
        self.ensure_workspace_index_ready_for_request();
        let vendor = self.vendor_twig_callables();
        let mut callables = Vec::new();
        self.visit_declaring_symbol_maps(|uri, map| {
            callables.extend(map.twig_callables.iter().map(|site| TwigCallable {
                kind: site.kind,
                name: site.name,
                uri: uri.to_string(),
                start: site.start,
                end: site.end,
                target: site.target.clone(),
            }));
        });
        callables.extend(vendor.callables.iter().cloned());
        Arc::new(TwigCallables {
            callables,
            has_core: vendor.has_core,
        })
    }

    /// The names of every `kind` callable the project can use, sorted.
    pub(crate) fn twig_callable_names(&self, kind: StringKeyKind) -> Vec<String> {
        self.twig_callables().names(kind)
    }

    /// Where the `kind` callable `name` is: the PHP method or function
    /// behind it, or the declaration itself when it runs a closure or
    /// nothing that can be found.
    pub(crate) fn twig_callable_definitions(
        &self,
        kind: StringKeyKind,
        name: &str,
    ) -> Vec<Location> {
        let callables = self.twig_callables();
        let mut locations: Vec<Location> = Vec::new();
        for callable in callables.matching(kind, name) {
            let location = match &callable.target {
                TwigCallableTarget::Method { class, method } => {
                    self.class_member_declaration_location(class, method, false)
                }
                TwigCallableTarget::Function(function) => {
                    self.resolve_function_definition(&[function.to_string()])
                }
                TwigCallableTarget::Inline => None,
            }
            .or_else(|| self.twig_callable_declaration(callable));
            if let Some(location) = location
                && !locations.contains(&location)
            {
                locations.push(location);
            }
        }
        locations
    }

    /// Where `callable` is declared: the name in its `new TwigFilter(…)` or
    /// attribute.
    pub(crate) fn twig_callable_declaration(&self, callable: &TwigCallable) -> Option<Location> {
        let content = self.get_file_content_arc(&callable.uri).or_else(|| {
            crate::util::uri_to_path(&callable.uri)
                .and_then(|path| std::fs::read_to_string(path).ok())
                .map(Arc::new)
        })?;
        Some(Location {
            uri: crate::util::parse_url(&callable.uri)?,
            range: crate::text_position::byte_range_to_lsp_range(
                &content,
                callable.start as usize,
                callable.end as usize,
            ),
        })
    }
}

// ─── Templates ──────────────────────────────────────────────────────────────

pub(super) fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

pub(super) fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The identifier that starts at `start`, and where it ends.
pub(super) fn word_at(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    if start >= end || !is_identifier_start(bytes[start]) {
        return None;
    }
    Some(
        start
            + bytes[start..end]
                .iter()
                .take_while(|byte| is_identifier_byte(**byte))
                .count(),
    )
}

/// The tag a `{% … %}` block whose body spans `body..end` opens with, and
/// where its name ends.
pub(super) fn tag_at(content: &str, body: usize, end: usize) -> Option<(&str, usize)> {
    let bytes = content.as_bytes();
    let mut start = body;
    // `{%-` and `{%~` trim the whitespace around the tag.
    if matches!(bytes.get(start), Some(b'-' | b'~')) {
        start += 1;
    }
    let start = skip_whitespace(bytes, start, end);
    let name_end = word_at(bytes, start, end)?;
    Some((&content[start..name_end], name_end))
}

/// The tags whose body is written out as it stands rather than read.
pub(super) fn opens_raw_text(tag: &str) -> Option<&'static str> {
    match tag {
        "verbatim" => Some("endverbatim"),
        "raw" => Some("endraw"),
        _ => None,
    }
}

/// The names a template's macros go by: the ones it defines with
/// `{% macro %}` and the ones it imports with `{% from … import %}`, under
/// the name the import gives them.
fn macro_names(content: &str) -> HashSet<&str> {
    let bytes = content.as_bytes();
    let mut names = HashSet::new();
    for_each_twig_block(content, |body, end| {
        if bytes[body - 1] != b'%' {
            return;
        }
        let Some((tag, after)) = tag_at(content, body, end) else {
            return;
        };
        match tag {
            "macro" => {
                let start = skip_whitespace(bytes, after, end);
                if let Some(name_end) = word_at(bytes, start, end) {
                    names.insert(&content[start..name_end]);
                }
            }
            "from" => {
                let Some(import) = content[after..end].find(" import ") else {
                    return;
                };
                let list = &content[after + import + " import ".len()..end];
                let list = list.trim_end_matches(['-', '~']);
                for entry in list.split(',') {
                    let mut words = entry.split_whitespace();
                    match (words.next(), words.next(), words.next()) {
                        (Some(_), Some("as"), Some(alias)) => names.insert(alias),
                        (Some(name), None, None) => names.insert(name),
                        _ => false,
                    };
                }
            }
            _ => {}
        }
    });
    names
}

/// The filters a template applies and the functions it calls, sorted by
/// offset.
pub(crate) fn twig_callable_spans(content: &str) -> Vec<SymbolSpan> {
    if !content.contains("{{") && !content.contains("{%") {
        return Vec::new();
    }
    let bytes = content.as_bytes();
    let macros = macro_names(content);
    let mut spans = Vec::new();
    let mut raw_until: Option<&'static str> = None;
    for_each_twig_block(content, |body, end| {
        let tag = (bytes[body - 1] == b'%')
            .then(|| tag_at(content, body, end))
            .flatten();
        if let Some(close) = raw_until {
            if tag.is_some_and(|(tag, _)| tag == close) {
                raw_until = None;
            }
            return;
        }
        let (start, filter_first) = match tag {
            Some((tag, after)) => {
                if let Some(close) = opens_raw_text(tag) {
                    raw_until = Some(close);
                    return;
                }
                match tag {
                    // A macro's name and arguments, the names an import
                    // brings in, the names a guard tests for, and a cache
                    // tag's `ttl()` and `tags()` are the tag's own syntax.
                    "macro" | "from" | "import" | "guard" | "cache" => return,
                    // `{% apply upper|escape %}` applies `upper` too.
                    "apply" | "filter" => (after, true),
                    _ => (after, false),
                }
            }
            None => (body, false),
        };
        scan_expression(content, start, end, filter_first, &macros, &mut spans);
    });
    spans
}

/// Push the filters and functions the expression between `start` and `end`
/// names.  `filter_first` says that its first name is a filter, as an
/// `apply` tag's is.
fn scan_expression(
    content: &str,
    start: usize,
    end: usize,
    filter_first: bool,
    macros: &HashSet<&str>,
    spans: &mut Vec<SymbolSpan>,
) {
    let bytes = content.as_bytes();
    let push = |kind: StringKeyKind, start: usize, end: usize, spans: &mut Vec<SymbolSpan>| {
        spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                kind,
                key: content[start..end].to_string(),
                is_write: false,
                is_optional: false,
            },
        });
    };
    let mut cursor = start;
    let mut expect_filter = filter_first;
    while cursor < end {
        let byte = bytes[cursor];
        if matches!(byte, b'\'' | b'"') {
            match string_end(bytes, cursor + 1, byte) {
                Some(close) => cursor = close + 1,
                None => return,
            }
            continue;
        }
        if byte == b'|' {
            expect_filter = true;
            cursor += 1;
            continue;
        }
        if !is_identifier_start(byte) {
            if !byte.is_ascii_whitespace() {
                expect_filter = false;
            }
            cursor += 1;
            continue;
        }
        let word_end = word_at(bytes, cursor, end).unwrap_or(cursor + 1);
        let word = &content[cursor..word_end];
        if expect_filter {
            expect_filter = false;
            push(StringKeyKind::TwigFilter, cursor, word_end, spans);
            cursor = word_end;
            continue;
        }
        // An attribute or method of something (`user.name()`), but not the
        // bound of a range (`1..max(3)`).
        let is_member = cursor > start
            && bytes[cursor - 1] == b'.'
            && !(cursor > start + 1 && bytes[cursor - 2] == b'.');
        if word == "is" {
            cursor = skip_test(bytes, word_end, end);
            continue;
        }
        if !is_member
            && bytes.get(word_end) == Some(&b'(')
            && !OPERATOR_WORDS.contains(&word)
            && !macros.contains(word)
        {
            push(StringKeyKind::TwigFunction, cursor, word_end, spans);
        }
        cursor = word_end;
    }
}

/// Where the test an `is` (or `is not`) ending at `start` applies ends:
/// its name, of one word or of two (`same as`, `divisible by`).
pub(super) fn skip_test(bytes: &[u8], start: usize, end: usize) -> usize {
    let next_word = |from: usize| {
        let word_start = skip_whitespace(bytes, from, end);
        word_at(bytes, word_start, end).map(|word_end| (word_start, word_end))
    };
    let Some((mut word_start, mut word_end)) = next_word(start) else {
        return start;
    };
    if &bytes[word_start..word_end] == b"not" {
        let Some(found) = next_word(word_end) else {
            return word_end;
        };
        (word_start, word_end) = found;
    }
    if matches!(&bytes[word_start..word_end], b"same" | b"divisible")
        && let Some((_, second_end)) = next_word(word_end)
    {
        return second_end;
    }
    word_end
}

/// Where the filter or function name the cursor is typing in a template
/// starts, and which of the two it is: a name after a `|` or an `apply`
/// tag is a filter, and one where an expression may start a function.
pub(crate) fn callable_name_start(content: &str, cursor: usize) -> Option<(usize, StringKeyKind)> {
    let before = content.get(..cursor)?;
    let bytes = before.as_bytes();
    let name_start = bytes
        .iter()
        .rposition(|byte| !is_identifier_byte(*byte))
        .map_or(0, |index| index + 1);
    if bytes.get(name_start).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let open = before.rfind("{{").max(before.rfind("{%"))?;
    if before[open..].contains("}}") || before[open..].contains("%}") {
        return None;
    }
    // Inside a string, the name is the string's.
    let body = open + 2;
    let mut scan = body;
    while scan < name_start {
        match bytes[scan] {
            quote @ (b'\'' | b'"') => match string_end(bytes, scan + 1, quote) {
                Some(close) if close < name_start => scan = close + 1,
                _ => return None,
            },
            _ => scan += 1,
        }
    }

    let head = before[body..name_start].trim_end();
    if head.ends_with('|') {
        return Some((name_start, StringKeyKind::TwigFilter));
    }
    if head.ends_with('.') {
        return None;
    }
    if bytes[open + 1] == b'%' {
        let tag_head = head.trim_start_matches(['-', '~']).trim_start();
        let (tag, rest) = tag_head
            .split_once(|c: char| c.is_ascii_whitespace())
            .unwrap_or((tag_head, ""));
        if tag_head.is_empty() {
            return None;
        }
        if matches!(tag, "apply" | "filter") && rest.trim().is_empty() {
            return Some((name_start, StringKeyKind::TwigFilter));
        }
        if matches!(
            tag,
            "macro" | "from" | "import" | "guard" | "block" | "endblock"
        ) {
            return None;
        }
    }
    // A test's name follows `is` or `is not`.
    let last_word = |text: &str| -> (usize, bool) {
        let word = text
            .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        (text.len() - word.len(), word == "is")
    };
    let (previous_start, after_is) = last_word(head);
    let names_test =
        after_is || (head.ends_with("not") && last_word(head[..previous_start].trim_end()).1);
    (!names_test).then_some((name_start, StringKeyKind::TwigFunction))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(content: &str) -> Vec<String> {
        twig_callable_spans(content)
            .into_iter()
            .map(|span| {
                let SymbolKind::StringKey { kind, key, .. } = span.kind else {
                    unreachable!()
                };
                assert_eq!(&content[span.start as usize..span.end as usize], key);
                let kind = match kind {
                    StringKeyKind::TwigFilter => "filter",
                    _ => "function",
                };
                format!("{kind} {key}")
            })
            .collect()
    }

    #[test]
    fn filters_and_functions_in_expressions_and_tags() {
        let twig = "{# {{ x|commented }} #}<p>|not_twig()</p>\n{{ amount|price('EUR') | upper }}\n{{ icon('x', {size: max(1, 2)}) }}{{ user.name() }}{{ 'a|b(c)'|trim }}\n{% for i in range(1, 3)|reverse %}{{ 1..max(4) }}{% endfor %}\n{% apply spaceless|escape %}x{% endapply %}{% if not(x) and (y) %}{% endif %}";
        assert_eq!(
            report(twig),
            [
                "filter price",
                "filter upper",
                "function icon",
                "function max",
                "filter trim",
                "function range",
                "filter reverse",
                "function max",
                "filter spaceless",
                "filter escape",
            ]
        );
    }

    #[test]
    fn tests_macros_and_raw_text_name_no_function() {
        let twig = "{% from 'forms.html.twig' import field, label as caption %}{% import 'm.html.twig' as m %}\n{% macro row(name, value = none) %}{% endmacro %}\n{{ field('x') }}{{ caption('y') }}{{ row('z') }}{{ m.input() }}{{ _self.row() }}\n{% if x is same as(y) or x is not divisible by(3) or x is defined %}{% endif %}\n{% verbatim %}{{ vue(thing) }}{% endverbatim %}{% cache 'k' ttl(300) tags('a') %}{% endcache %}{{ dump(x) }}";
        assert_eq!(report(twig), ["function dump"]);
    }

    #[test]
    fn patterns_match_by_their_literal_parts() {
        assert!(matches_pattern("*_path", "asset_path"));
        assert!(!matches_pattern("*_path", "_path"));
        assert!(matches_pattern("a*b*c", "axbyc"));
        assert!(!matches_pattern("a*b*c", "abc"));
        assert!(!matches_pattern("*_path", "asset_paths"));
    }

    #[test]
    fn completion_knows_filters_from_functions() {
        let at = |twig: &str| callable_name_start(twig, twig.len());
        assert_eq!(at("{{ x|pr"), Some((5, StringKeyKind::TwigFilter)));
        assert_eq!(at("{{ x | "), Some((7, StringKeyKind::TwigFilter)));
        assert_eq!(at("{% apply up"), Some((9, StringKeyKind::TwigFilter)));
        assert_eq!(at("{{ ic"), Some((3, StringKeyKind::TwigFunction)));
        assert_eq!(at("{% if ic"), Some((6, StringKeyKind::TwigFunction)));
        assert_eq!(at("{% i"), None);
        assert_eq!(at("{{ user.na"), None);
        assert_eq!(at("{{ 'x|pr"), None);
        assert_eq!(at("{% if x is de"), None);
        assert_eq!(at("{% if x is not de"), None);
        assert_eq!(at("{{ x }} ic"), None);
        assert_eq!(at("<p>ic"), None);
    }
}
