//! Raw-text scanning of `<x-…>` component tags in Blade source: the
//! attributes each call site passes, the lowest-priority variable source
//! in the declaration chain documented in [`super::signature`].
//!
//! Component tags are HTML syntax, not PHP, so they cannot be read from
//! the mago AST the way a `view()` call site can (see
//! [`super::call_site_inference`]). The virtual PHP the preprocessor
//! emits only carries a bound attribute's *expression* forward, as a
//! `blade_bound_attr_directive(...)` call; the tag name and any plain
//! string attribute never appear in it at all. This module scans the
//! original Blade source directly instead.

use std::borrow::Cow;
use std::ops::Range;

use crate::php_type::PhpType;

use super::pairing::{self, Pair, Pairing, Stray, Token};
use super::signature::mask_inert_regions;

/// One `<x-…>` tag occurrence whose tag name matched one of the requested
/// component names.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct ComponentTagCall {
    /// Plain (non-bound) attributes, already typed from their literal
    /// text: `(camelCase name, type)`.
    pub(crate) literal: Vec<(String, PhpType)>,
    /// Bound attributes (`:name="expr"` / the `:$var` shorthand):
    /// `(camelCase name, index into the file's
    /// `blade_bound_attr_directive(...)` call sequence)`.
    ///
    /// The preprocessor emits exactly one `blade_bound_attr_directive` call
    /// per bound attribute that is not consumed as a component call's
    /// argument, on every HTML tag in the file, in document order — the
    /// same order this scan counts them in — so the index correlates the
    /// two without needing to translate byte offsets between the Blade
    /// source and the virtual PHP. That marker is exclusive to bound
    /// attributes, so a `@class`/`@json`/other directive sharing the
    /// generic `blade_directive` marker elsewhere in the file cannot shift
    /// this sequence out of sync.
    pub(crate) bound: Vec<(String, usize)>,
}

/// The prefixes a component tag is written under, longest first so
/// neither shadows the other.
const TAG_PREFIXES: [&str; 2] = ["<livewire:", "<x-"];

/// Which index answers a component tag's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagKind {
    Blade,
    Livewire,
}

impl TagKind {
    /// The opening a tag of this kind is written with.
    pub(crate) fn opening(self) -> &'static str {
        match self {
            TagKind::Blade => "<x-",
            TagKind::Livewire => "<livewire:",
        }
    }

    /// The prefix a tag of this kind carries before the component name,
    /// without the `<`: `x-`, `livewire:`.
    pub(crate) fn prefix(self) -> &'static str {
        &self.opening()[1..]
    }
}

/// Which part of a component tag the cursor sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagCursor {
    /// The tag's own name, whether finished or still being typed.
    Name,
    /// An attribute name on a tag whose name is already written.
    Attribute,
}

/// The component tag a cursor sits in, and what it is writing there.
#[derive(Debug, PartialEq)]
pub(crate) struct TagContext {
    pub(crate) kind: TagKind,
    /// The tag name as written, without the opening. Empty for a tag
    /// whose name has not been typed yet.
    pub(crate) name: String,
    /// Byte offset the token under the cursor starts at, so an edit
    /// replaces what is already typed rather than appending to it.
    pub(crate) token_start: usize,
    pub(crate) cursor: TagCursor,
}

/// The component tag `offset` sits in, or `None` when it sits anywhere
/// else.
///
/// Reads the raw Blade buffer rather than the virtual PHP the rest of the
/// pipeline works from, for the same reason directive-name completion does
/// (`super::directive_completion`): a component tag is HTML, and the
/// virtual PHP carries neither the tag name nor a plain attribute's name
/// anywhere near where they are written.
pub(crate) fn tag_context_at(content: &str, offset: usize) -> Option<TagContext> {
    let before = content.get(..offset)?;
    // The nearest opening before the cursor is the tag it could be inside;
    // a closing tag is spelled `</x-…`, so it never matches.
    let (start, kind) = [TagKind::Blade, TagKind::Livewire]
        .into_iter()
        .filter_map(|kind| before.rfind(kind.opening()).map(|at| (at, kind)))
        .max_by_key(|(at, _)| *at)?;
    // A tag written inside a comment, a `@php` block, or an echo is text
    // rather than markup, and names no component.
    if !super::directive_completion::is_html_position(content, start) {
        return None;
    }

    let bytes = content.as_bytes();
    let name_start = start + kind.opening().len();
    let mut name_end = name_start;
    while name_end < bytes.len() && is_tag_name_char(bytes[name_end] as char) {
        name_end += 1;
    }
    let name = content[name_start..name_end].to_string();
    if offset <= name_end {
        return Some(TagContext {
            kind,
            name,
            token_start: name_start,
            cursor: TagCursor::Name,
        });
    }

    // Past the name, so the cursor is in the attribute list — unless the
    // tag closed before reaching it, in which case the opening this scan
    // started from is not the cursor's tag at all.
    let mut quote: Option<u8> = None;
    for &byte in &bytes[name_end..offset] {
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            // Both `>` and the `/` of a self-closing tag end it here.
            None if byte == b'>' => return None,
            None => {}
        }
    }
    // Inside an attribute's value the cursor is writing PHP or text, not
    // an attribute name.
    if quote.is_some() {
        return None;
    }

    let mut token_start = offset;
    while token_start > name_end && is_attr_name_char(bytes[token_start - 1] as char) {
        token_start -= 1;
    }
    // An attribute name follows whitespace. Anything else before it means
    // the cursor is in the middle of an unquoted value (`type=dan`) or of
    // the tag name itself.
    if !bytes[token_start - 1].is_ascii_whitespace() {
        return None;
    }
    Some(TagContext {
        kind,
        name,
        token_start,
        cursor: TagCursor::Attribute,
    })
}

/// Every distinct component tag referenced by an occurrence in `content`,
/// with the prefix it was written under (`x-alert`, `livewire:counter`).
///
/// Closing tags are skipped: an opening tag is what names a component, and
/// a self-closing one has no closing tag to find it by.
pub(crate) fn referenced_tags(content: &str) -> Vec<String> {
    if !TAG_PREFIXES.iter().any(|p| content.contains(p)) {
        return Vec::new();
    }
    let masked = mask_inert_regions(content, true);
    let bytes = masked.as_bytes();
    let mut tags = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let Some(prefix) = TAG_PREFIXES
            .iter()
            .find(|prefix| bytes[i..].starts_with(prefix.as_bytes()))
        else {
            i += 1;
            continue;
        };
        let name_start = i + prefix.len();
        let mut j = name_start;
        while j < bytes.len() && is_tag_name_char(bytes[j] as char) {
            j += 1;
        }
        if j > name_start {
            let tag = format!("{}{}", &prefix[1..], &masked[name_start..j]);
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        i = j.max(name_start);
    }
    tags
}

/// Every distinct tag name referenced by an `<x-…>` occurrence in
/// `content`. Used in the reverse direction from [`scan_component_tag_calls`]:
/// given a file that was just edited, which components does it call?
pub(crate) fn referenced_component_tags(content: &str) -> Vec<String> {
    referenced_tags(content)
        .into_iter()
        .filter_map(|tag| tag.strip_prefix("x-").map(str::to_string))
        .collect()
}

/// The `<x-…>` openings that could name one of `tag_names`, for the
/// cheap rejection test [`may_contain_component_tag`] applies.
///
/// Built once per component rather than per candidate file: a bulk
/// refresh pass tests one component's tags against every Blade file in
/// the workspace, and [`scan_component_tag_calls`] masks the whole file
/// before it can answer, which is far more than a rejection needs.
pub(crate) fn component_tag_needles(tag_names: &[String]) -> Vec<String> {
    tag_names.iter().map(|name| format!("<x-{name}")).collect()
}

/// Whether `content` is worth handing to [`scan_component_tag_calls`].
///
/// Conservative in the direction that matters: masking only ever removes
/// tags (a `<x-…>` inside a comment or a `@php` block), and a needle hit
/// on a longer tag name (`<x-card` for the needle `<x-car`) is settled by
/// the real scan, so a `true` here can still scan to nothing while a
/// `false` cannot hide a call.
pub(crate) fn may_contain_component_tag(content: &str, needles: &[String]) -> bool {
    content.contains("<x-")
        && needles
            .iter()
            .any(|needle| content.contains(needle.as_str()))
}

/// Scan `content` for `<x-…>` occurrences whose tag name (after the `x-`
/// prefix) is one of `tag_names`, and collect the attributes each passes.
///
/// Every bound attribute on *any* tag in the file is counted — not just a
/// matching one — because the preprocessor's `blade_bound_attr_directive`
/// call sequence includes them all; skipping a non-matching tag's bound
/// attributes here would desynchronise this scan's count against that
/// sequence.
///
/// `arguments` is the same partition the preprocessor applied to this
/// file: a bound attribute naming a parameter of the call its tag makes
/// is that call's argument, not a `blade_bound_attr_directive` of its own,
/// so it is not in the sequence to be counted. Both sides read the tag's
/// target from one place (a template's
/// [`crate::blade::call_site_inference::BladeScope`]), so the two cannot
/// disagree about which attributes are arguments.
pub(crate) fn scan_component_tag_calls(
    content: &str,
    tag_names: &[String],
    arguments: &dyn Fn(&str) -> Option<Vec<String>>,
) -> Vec<ComponentTagCall> {
    if tag_names.is_empty() || !content.contains("<x-") {
        return Vec::new();
    }
    let masked = mask_inert_regions(content, true);
    let bytes = masked.as_bytes();
    let mut results = Vec::new();
    let mut bound_index = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        match bytes.get(i + 1) {
            Some(b'/') => {
                // A closing tag has no attributes to scan.
                i = find_byte(&masked, i, b'>').map_or(bytes.len(), |end| end + 1);
                continue;
            }
            Some(c) if c.is_ascii_alphabetic() => {}
            _ => {
                i += 1;
                continue;
            }
        }
        let name_start = i + 1;
        let mut j = name_start;
        while j < bytes.len() && is_tag_name_char(bytes[j] as char) {
            j += 1;
        }
        let tag_name = &masked[name_start..j];
        let is_match = tag_name
            .strip_prefix("x-")
            .is_some_and(|bare| tag_names.iter().any(|n| n == bare));
        let consumed = arguments(tag_name).unwrap_or_default();
        let (end, call) = scan_tag_attributes(&masked, j, &consumed, &mut bound_index);
        if is_match {
            results.push(call);
        }
        i = end;
    }
    results
}

/// One component tag written in a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TagSpan {
    pub(crate) kind: TagKind,
    /// The component the tag names, without the opening (`alert`,
    /// `counter`).
    pub(crate) name: String,
    /// The name's own bytes in the raw Blade source.
    pub(crate) name_span: Range<usize>,
    /// The opening tag through its matching closing tag, or the opening
    /// tag alone when it is self-closing or never closed.
    pub(crate) span: Range<usize>,
    /// Whether a matching closing tag was found, so [`Self::span`] covers
    /// a body rather than the opening tag on its own.
    pub(crate) closed: bool,
}

/// A closing tag, `</x-…>` or `</livewire:…>`.
struct TagCloser {
    kind: TagKind,
    /// The name's own bytes, indexing the masked source it was read from.
    name: Range<usize>,
    /// The whole closing tag, `</` through `>`.
    span: Range<usize>,
}

/// The closing prefixes a component tag is written under, the closing
/// counterparts of [`TAG_PREFIXES`].
const CLOSING_TAG_PREFIXES: [&str; 2] = ["</livewire:", "</x-"];

/// The component tags of a template, read once and paired up.
struct PairedTags<'a> {
    /// The source with its inert regions blanked, which the closers'
    /// name ranges index.
    masked: Cow<'a, str>,
    /// Tags that open nothing: `<x-alert />`.
    self_closing: Vec<TagSpan>,
    /// Every other tag, paired with [`pairing::pair`].
    pairing: Pairing<TagSpan, TagCloser>,
}

/// Read every component tag in `content` and pair the opening tags with
/// their closers through [`super::pairing`], the same walk the directive
/// check uses, so a closing tag that does not match the innermost open
/// tag does to it what Blade does: ends it.
///
/// A closer matches by name, with the one exception Blade's compiler
/// carves out for named slots ([`closer_matches`]). `None` when the
/// template holds no tag at all; a lone stray closer counts, since it is
/// one of the shapes [`tag_imbalances`] reports.
fn pair_tags(content: &str) -> Option<PairedTags<'_>> {
    if !TAG_PREFIXES
        .iter()
        .chain(CLOSING_TAG_PREFIXES.iter())
        .any(|prefix| content.contains(prefix))
    {
        return None;
    }
    let masked = mask_inert_regions(content, true);
    let bytes = masked.as_bytes();

    let mut self_closing: Vec<TagSpan> = Vec::new();
    let mut tokens: Vec<Token<TagSpan, TagCloser>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        // Closing tag: `</x-…>` or `</livewire:…>`.
        if bytes.get(i + 1) == Some(&b'/') {
            let Some(prefix) = TAG_PREFIXES
                .iter()
                .find(|prefix| masked[i + 2..].starts_with(&prefix[1..]))
            else {
                i += 1;
                continue;
            };
            let name_start = i + 2 + (prefix.len() - 1);
            let mut j = name_start;
            while j < bytes.len() && is_tag_name_char(bytes[j] as char) {
                j += 1;
            }
            let Some(close) = find_byte(&masked, j, b'>') else {
                break;
            };
            tokens.push(Token::Close(TagCloser {
                kind: tag_kind_of(prefix),
                name: name_start..j,
                span: i..close + 1,
            }));
            i = close + 1;
            continue;
        }

        // Opening tag: `<x-…>` or `<livewire:…>`.
        let Some(prefix) = TAG_PREFIXES
            .iter()
            .find(|prefix| masked[i..].starts_with(**prefix))
        else {
            i += 1;
            continue;
        };
        let name_start = i + prefix.len();
        let mut j = name_start;
        while j < bytes.len() && is_tag_name_char(bytes[j] as char) {
            j += 1;
        }
        if j == name_start {
            i += 1;
            continue;
        }
        let lexed = lex_tag_attributes(&masked, j);
        let tag = TagSpan {
            kind: tag_kind_of(prefix),
            name: masked[name_start..j].to_string(),
            name_span: name_start..j,
            span: i..lexed.end,
            closed: false,
        };
        if lexed.self_closing {
            self_closing.push(tag);
        } else {
            tokens.push(Token::Open(tag));
        }
        i = lexed.end;
    }

    let pairing = pairing::pair(tokens, |open: &TagSpan, closer: &TagCloser| {
        closer_matches(open.kind, &open.name, &masked[closer.name.clone()])
    });
    Some(PairedTags {
        masked,
        self_closing,
        pairing,
    })
}

/// Which kind of tag one of [`TAG_PREFIXES`] opens.
fn tag_kind_of(prefix: &str) -> TagKind {
    if prefix == TagKind::Livewire.opening() {
        TagKind::Livewire
    } else {
        TagKind::Blade
    }
}

/// Every component tag in `content`, in document order:
/// `<x-…>`…`</x-…>` and `<livewire:…>`…`</livewire:…>`, self-closing and
/// unclosed ones included.
///
/// A tag ended by a closer that is not its own, or never ended at all,
/// stands for its opening tag alone rather than being paired with a
/// closer that is not its own; [`tag_imbalances`] is where those are
/// reported.
pub(crate) fn tag_spans(content: &str) -> Vec<TagSpan> {
    let Some(PairedTags {
        self_closing,
        pairing,
        ..
    }) = pair_tags(content)
    else {
        return Vec::new();
    };

    let mut out = self_closing;
    out.extend(
        pairing
            .pairs
            .into_iter()
            .map(|Pair { mut opener, closer }| {
                opener.span.end = closer.span.end;
                opener.closed = true;
                opener
            }),
    );
    out.extend(pairing.consumed);
    out.extend(pairing.open);
    out.sort_by(|a, b| {
        a.span
            .start
            .cmp(&b.span.start)
            .then(b.span.end.cmp(&a.span.end))
    });
    out
}

/// Whether a closing tag spelled `closer_name` ends an opener of kind
/// `kind` named `name`.
///
/// Every tag closes under its own name. A named slot (`<x-slot:title>`)
/// also accepts the bare `</x-slot>` used by the legacy
/// `<x-slot name="title">` form.
fn closer_matches(kind: TagKind, name: &str, closer_name: &str) -> bool {
    name == closer_name
        || (kind == TagKind::Blade && is_slot_tag_name(name) && closer_name == "slot")
}

/// A place a component tag's block structure does not add up: the same
/// three shapes [`super::balance::Imbalance`] reports for a directive,
/// since a tag's body is a block too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TagImbalance {
    /// A closing tag that closes something other than the tag it sits in.
    Mismatched {
        closer: Range<usize>,
        found_kind: TagKind,
        found: String,
        opener_kind: TagKind,
        opener: String,
        opener_span: Range<usize>,
    },
    /// A closing tag with no open tag to close.
    Unexpected {
        closer: Range<usize>,
        found_kind: TagKind,
        found: String,
    },
    /// A tag the template never closes.
    Unclosed {
        opener_span: Range<usize>,
        opener_kind: TagKind,
        opener: String,
    },
}

impl TagImbalance {
    /// The range the report is anchored on: the offending tag itself.
    pub(crate) fn span(&self) -> &Range<usize> {
        match self {
            TagImbalance::Mismatched { closer, .. } | TagImbalance::Unexpected { closer, .. } => {
                closer
            }
            TagImbalance::Unclosed { opener_span, .. } => opener_span,
        }
    }
}

/// Every place a component tag's block structure does not add up: a
/// closing tag for a component further out, a closing tag with nothing
/// open, or a tag the template never closes.
///
/// Read from the same walk as [`tag_spans`], so the two agree on what a
/// closer closes: a tag that comes back from `tag_spans` with its opening
/// tag alone as its extent is one of the tags reported here (or one a
/// reported closer ended on its way out). An opener is anchored on its
/// `<x-name` rather than the whole opening tag, so a long attribute list
/// does not light up.
pub(crate) fn tag_imbalances(content: &str) -> Vec<TagImbalance> {
    let Some(PairedTags {
        masked, pairing, ..
    }) = pair_tags(content)
    else {
        return Vec::new();
    };
    let found = |closer: &TagCloser| masked[closer.name.clone()].to_string();
    let anchor = |tag: &TagSpan| tag.span.start..tag.name_span.end;

    let mut imbalances: Vec<TagImbalance> = pairing
        .strays
        .into_iter()
        .map(|stray| match stray {
            Stray::Mismatched { closer, skipped } => TagImbalance::Mismatched {
                found: found(&closer),
                found_kind: closer.kind,
                closer: closer.span,
                opener_kind: skipped.kind,
                opener_span: anchor(&skipped),
                opener: skipped.name,
            },
            Stray::Unexpected { closer } => TagImbalance::Unexpected {
                found: found(&closer),
                found_kind: closer.kind,
                closer: closer.span,
            },
        })
        .collect();
    imbalances.extend(pairing.open.into_iter().map(|open| TagImbalance::Unclosed {
        opener_span: anchor(&open),
        opener_kind: open.kind,
        opener: open.name,
    }));

    imbalances.sort_by_key(|imbalance| imbalance.span().start);
    imbalances
}

fn find_byte(content: &str, from: usize, needle: u8) -> Option<usize> {
    content.as_bytes()[from..]
        .iter()
        .position(|&b| b == needle)
        .map(|pos| from + pos)
}

/// The characters a component tag name is spelled with. Dots separate
/// directories (`forms.input`), a double colon a package namespace
/// (`pkg::calendar`), and `<x-slot:title>` names a slot the same way.
pub(crate) fn is_tag_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | ':' | '_')
}

/// The characters an HTML attribute name is spelled with, which is a
/// wider set than a tag name's: `wire:model.live`, `x-on:keydown`, and
/// `@click` are all legal there.
pub(crate) fn is_attr_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | ':' | '_' | '@')
}

/// One attribute of a component tag, as its attribute list spells it.
///
/// Ranges index the text the list was lexed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TagAttribute {
    /// The name, without a bound attribute's leading `:` and without the
    /// `$` of the `:$name` shorthand. A `::` escape keeps the colon it
    /// protects (`::class` is named `:class`).
    pub(crate) name: Range<usize>,
    /// `:name="…"` or `:$name`: the value is a PHP expression, not text.
    pub(crate) bound: bool,
    /// The `:$name` shorthand, which passes the variable it names.
    pub(crate) shorthand: bool,
    /// The value text, between the quotes when quoted, or `None` for a
    /// bare attribute (`disabled`).
    pub(crate) value: Option<Range<usize>>,
    /// Whether the value was written between `"` or `'`.
    pub(crate) quoted: bool,
}

/// A component tag's attribute list, from just past the tag name to the
/// `>` or `/>` that ends the opening tag.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TagAttributes {
    pub(crate) attributes: Vec<TagAttribute>,
    /// The offset just past the `>`/`/>`, or the end of the text when the
    /// tag never closes.
    pub(crate) end: usize,
    /// Whether the opening tag ended with `/>`.
    pub(crate) self_closing: bool,
    /// Whether the tag's `>` was found at all. A quoted value that never
    /// closes swallows the rest of the text, so the tag is unclosed too.
    pub(crate) closed: bool,
}

/// Lex the attribute list of a component tag whose name ends at `start`.
///
/// This is the one reading of a tag's attributes both the preprocessor
/// (which turns them into the arguments of the call the tag makes) and
/// the call-site scan (which turns them into the component template's
/// variables) work from, so the two cannot disagree about what a tag
/// passes. An attribute value may hold a `>` (`:items="$a > $b"`), so the
/// tag ends at the first `>` outside a quoted value, not the first one.
///
/// The lexer is tolerant of malformed markup in one direction only: a
/// byte that starts no attribute is stepped over, so a broken tag cannot
/// spin the scan, but nothing is guessed at.
pub(crate) fn lex_tag_attributes(text: &str, start: usize) -> TagAttributes {
    let bytes = text.as_bytes();
    let mut lexed = TagAttributes::default();
    let mut i = start;

    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        match bytes.get(i) {
            None => break,
            Some(b'>') => {
                i += 1;
                lexed.closed = true;
                break;
            }
            Some(b'/') if bytes.get(i + 1) == Some(&b'>') => {
                i += 2;
                lexed.closed = true;
                lexed.self_closing = true;
                break;
            }
            Some(b'/') => {
                i += 1;
                continue;
            }
            _ => {}
        }

        // `:$name` passes the variable it names.
        if bytes[i] == b':'
            && bytes.get(i + 1) == Some(&b'$')
            && bytes
                .get(i + 2)
                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
            let name_start = i + 2;
            let mut j = name_start;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            lexed.attributes.push(TagAttribute {
                name: name_start..j,
                bound: true,
                shorthand: true,
                value: None,
                quoted: false,
            });
            i = j;
            continue;
        }

        // A single leading `:` marks a bound attribute; `::` is an
        // escaped literal colon (e.g. `::class`), and the attribute name
        // drops just the escape, not the real colon it protects.
        let bound = bytes[i] == b':' && bytes.get(i + 1) != Some(&b':');
        let name_start = if bytes[i] == b':' { i + 1 } else { i };
        let mut j = name_start;
        while j < bytes.len() && is_attr_name_char(bytes[j] as char) {
            j += 1;
        }
        if j == name_start {
            // Not an attribute token (e.g. a stray `<`); skip one byte so
            // a malformed tag cannot spin this loop forever.
            i += 1;
            continue;
        }
        let name = name_start..j;
        i = j;

        if bytes.get(i) != Some(&b'=') {
            lexed.attributes.push(TagAttribute {
                name,
                bound,
                shorthand: false,
                value: None,
                quoted: false,
            });
            continue;
        }
        i += 1;

        let quote = bytes.get(i).copied().filter(|b| *b == b'"' || *b == b'\'');
        let value_start = i + usize::from(quote.is_some());
        let mut k = value_start;
        match quote {
            Some(quote) => {
                while k < bytes.len() && bytes[k] != quote {
                    k += 1;
                }
            }
            None => {
                while k < bytes.len() && !bytes[k].is_ascii_whitespace() && bytes[k] != b'>' {
                    k += 1;
                }
            }
        }
        lexed.attributes.push(TagAttribute {
            name,
            bound,
            shorthand: false,
            value: Some(value_start..k),
            quoted: quote.is_some(),
        });
        // Past the closing quote, when there is one to be past.
        i = if quote.is_some() {
            (k + 1).min(bytes.len())
        } else {
            k
        };
    }

    lexed.end = i;
    lexed
}

/// Read the attribute list of a tag starting right after its name, up to
/// (and past) the tag's closing `>` or self-closing `/>`. Returns the
/// offset just past the close, plus the literal and bound attributes
/// found; `bound_index` is threaded through and bumped for every bound
/// attribute encountered, matching or not, to stay in sync with the
/// file-wide `blade_bound_attr_directive` call count.
fn scan_tag_attributes(
    masked: &str,
    start: usize,
    consumed: &[String],
    bound_index: &mut usize,
) -> (usize, ComponentTagCall) {
    let lexed = lex_tag_attributes(masked, start);
    let mut call = ComponentTagCall::default();
    // A parameter can only be filled once, so a name an earlier attribute
    // of this tag already claimed is back to being ordinary markup — the
    // same rule the preprocessor applies on the emitting side.
    let mut unclaimed: Vec<&str> = consumed.iter().map(String::as_str).collect();
    let mut is_argument = |name: &str| match unclaimed.iter().position(|param| *param == name) {
        Some(index) => {
            unclaimed.remove(index);
            true
        }
        None => false,
    };

    for attr in &lexed.attributes {
        let written = &masked[attr.name.clone()];
        if attr.shorthand {
            // Bound, named after the variable. One the tag's own call
            // carries is not in the `blade_bound_attr_directive`
            // sequence at all.
            if !is_argument(written) {
                call.bound.push((written.to_string(), *bound_index));
                *bound_index += 1;
            }
            continue;
        }
        let name = camel_case_attr_name(written);
        let Some(value) = &attr.value else {
            // A bare attribute (`disabled`) is `true`. A bare *bound*
            // attribute (`:disabled`, no `=`) never reaches the
            // preprocessor's `blade_bound_attr_directive` emission (it
            // requires a quoted value), so there is nothing to correlate
            // for it.
            if !attr.bound {
                call.literal.push((name, PhpType::bool()));
            }
            continue;
        };
        if attr.bound {
            // An unquoted bound value is never recognised by the
            // preprocessor either; only a quoted one produced a
            // `blade_bound_attr_directive` call to correlate against.
            if attr.quoted && !is_argument(&name) {
                call.bound.push((name, *bound_index));
                *bound_index += 1;
            }
            continue;
        }
        let raw = &masked[value.clone()];
        let ty = if raw.contains("{{") || raw.contains("{!!") {
            // A literal attribute embedding a Blade echo is not a
            // constant string; fall back to a generic type rather
            // than reporting the raw `{{ $expr }}` text as the value.
            PhpType::string()
        } else {
            PhpType::literal_string_value(raw)
        };
        call.literal.push((name, ty));
    }

    (lexed.end, call)
}

/// The named slots (`<x-slot:title>` or the legacy `<x-slot
/// name="title">`) written as a direct child of one of `tag_names`'s
/// component tags, anywhere in `content`.
///
/// A slot's receiving component is its *nearest* enclosing `<x-…>` tag,
/// the same scoping Blade's own compiler applies: `ComponentTagCompiler`
/// lowers `<x-slot…>`/`</x-slot>` to `@slot(...)`/`@endslot` in a pass
/// that runs before component tags are compiled, and the runtime
/// `slot()`/`endSlot()` pair (`Illuminate\View\Concerns\ManagesComponents`)
/// files the slot under whichever component is innermost on the render
/// stack at that point. A plain HTML tag between a component and its
/// slot does not change that (Blade never tracks HTML nesting), but
/// another component tag in between does: its own slots are its own.
///
/// A name is returned once per distinct value, regardless of how many
/// occurrences across the file (or however many times a caller repeats
/// the same slot name) declare it: a component template cannot know at
/// preprocessing time which specific occurrence it is rendering for, so
/// every name any occurrence could pass has to be declared.
pub(crate) fn scan_component_tag_slots(content: &str, tag_names: &[String]) -> Vec<String> {
    if tag_names.is_empty() || !content.contains("<x-slot") {
        return Vec::new();
    }
    let masked = mask_inert_regions(content, true);
    let bytes = masked.as_bytes();
    // Currently open `<x-…>` component tags, nearest last. `<x-slot…>`
    // itself is never pushed here: it is not a component boundary, so a
    // slot cannot receive another slot.
    let mut stack: Vec<(&str, bool)> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'/') {
            if !masked[i + 2..].starts_with("x-") {
                i += 1;
                continue;
            }
            let name_start = i + 2 + "x-".len();
            let mut j = name_start;
            while j < bytes.len() && is_tag_name_char(bytes[j] as char) {
                j += 1;
            }
            let Some(close) = find_byte(&masked, j, b'>') else {
                break;
            };
            let name = &masked[name_start..j];
            if !is_slot_tag_name(name)
                && let Some(pos) = stack.iter().rposition(|(open, _)| *open == name)
            {
                stack.truncate(pos);
            }
            i = close + 1;
            continue;
        }
        if !bytes.get(i + 1).is_some_and(|b| b.is_ascii_alphabetic()) {
            i += 1;
            continue;
        }
        if !masked[i + 1..].starts_with("x-") {
            i += 1;
            continue;
        }
        let name_start = i + 1 + "x-".len();
        let mut j = name_start;
        while j < bytes.len() && is_tag_name_char(bytes[j] as char) {
            j += 1;
        }
        let tag_name = &masked[name_start..j];
        let lexed = lex_tag_attributes(&masked, j);
        if is_slot_tag_name(tag_name) {
            if let Some((_, true)) = stack.last()
                && let Some(slot_name) = slot_tag_name(&masked, tag_name, &lexed)
                && !names.contains(&slot_name)
            {
                names.push(slot_name);
            }
        } else if lexed.closed && !lexed.self_closing {
            let is_match = tag_names.iter().any(|n| n == tag_name);
            stack.push((tag_name, is_match));
        }
        i = lexed.end;
    }
    names
}

/// Whether an `<x-…>` tag's bare name (after the `x-` prefix) opens a
/// slot: `slot` (the legacy `name="…"` form) or `slot:title` (the
/// inline form).
fn is_slot_tag_name(name: &str) -> bool {
    name == "slot" || name.starts_with("slot:")
}

/// The name a `<x-slot…>` tag declares, or `None` when neither the
/// inline form nor a `name="…"` attribute names one (a `:name="$expr"`
/// bound name is dynamic and cannot be resolved here).
///
/// The inline form wins when both are written, matching
/// `ComponentTagCompiler::compileSlots`'s `$matches['inlineName'] ?:
/// $matches['name']`. It is also the only one ever camel-cased: Blade's
/// `Str::camel` runs on the inline name when it contains a hyphen (a
/// PHP variable cannot spell one), but an attribute-form name is used
/// verbatim, hyphens and all, so a hyphenated one is written down as a
/// slot key `extract()` can never bind to a variable — the same fate an
/// unbindable component-tag attribute name has (the preprocessor's
/// `is_php_variable_name` skips declaring either).
fn slot_tag_name(masked: &str, tag_name: &str, lexed: &TagAttributes) -> Option<String> {
    if let Some(inline) = tag_name.strip_prefix("slot:") {
        return Some(if inline.contains('-') {
            camel_case_attr_name(inline)
        } else {
            inline.to_string()
        });
    }
    lexed.attributes.iter().find_map(|attr| {
        (!attr.bound && &masked[attr.name.clone()] == "name")
            .then(|| attr.value.clone())
            .flatten()
            .map(|value| masked[value].to_string())
    })
}

/// Convert a kebab-case attribute name to the camelCase variable name
/// Blade exposes it as (`Illuminate\Support\Str::camel`). A PHP variable
/// name cannot contain a hyphen, so only the camelCase form of a
/// hyphenated attribute is ever accessible inside the template.
pub(crate) fn camel_case_attr_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = false;
    for ch in name.chars() {
        if ch == '-' || ch == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Convert a camelCase name to the kebab-case attribute (or tag-name)
/// segment it is written as, matching `Illuminate\Support\Str::kebab`: a
/// delimiter goes before every capital that isn't the first character, and
/// existing separators are kept.
///
/// The inverse of [`camel_case_attr_name`] for the names Blade round-trips
/// (`hairAnalysis` ↔ `hair-analysis`), and the transform that turns a
/// component class's name into the tag that reaches it.
pub(crate) fn kebab_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.char_indices() {
        if ch.is_uppercase() && i > 0 {
            out.push('-');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(vars: &[(String, PhpType)]) -> Vec<&str> {
        vars.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// A project whose tags name no component the preprocessor could
    /// build, so no attribute of theirs is an argument.
    fn no_arguments(_tag: &str) -> Option<Vec<String>> {
        None
    }

    #[test]
    fn referenced_component_tags_collects_distinct_names() {
        let content = r#"<x-brand.boxes /><x-brand.boxes /><x-alert type="danger" />"#;
        let mut tags = referenced_component_tags(content);
        tags.sort();
        assert_eq!(tags, vec!["alert", "brand.boxes"]);
    }

    #[test]
    fn scans_a_literal_string_attribute() {
        let calls = scan_component_tag_calls(
            r#"<x-alert type="danger" />"#,
            &["alert".to_string()],
            &no_arguments,
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(names(&calls[0].literal), vec!["type"]);
        assert_eq!(
            calls[0].literal[0].1,
            PhpType::literal_string_value("danger")
        );
    }

    #[test]
    fn scans_a_bare_boolean_attribute() {
        let calls = scan_component_tag_calls(
            r#"<x-alert disabled />"#,
            &["alert".to_string()],
            &no_arguments,
        );
        assert_eq!(
            calls[0].literal,
            vec![("disabled".to_string(), PhpType::bool())]
        );
    }

    #[test]
    fn camel_cases_a_hyphenated_attribute_name() {
        let calls = scan_component_tag_calls(
            r#"<x-alert hair-analysis="x" />"#,
            &["alert".to_string()],
            &no_arguments,
        );
        assert_eq!(names(&calls[0].literal), vec!["hairAnalysis"]);
    }

    #[test]
    fn a_bound_attribute_is_indexed_in_document_order() {
        let calls = scan_component_tag_calls(
            r#"<x-alert :hairAnalysis="$model->hairAnalysis" />"#,
            &["alert".to_string()],
            &no_arguments,
        );
        assert_eq!(calls[0].bound, vec![("hairAnalysis".to_string(), 0)]);
    }

    #[test]
    fn a_non_matching_tags_bound_attribute_still_advances_the_index() {
        let calls = scan_component_tag_calls(
            r#"<div :class="$active"></div><x-alert :message="$msg" />"#,
            &["alert".to_string()],
            &no_arguments,
        );
        // The `<div>` binding is index 0; `alert`'s own binding must
        // therefore be index 1, or the caller correlates it against the
        // wrong `blade_bound_attr_directive` call.
        assert_eq!(calls[0].bound, vec![("message".to_string(), 1)]);
    }

    #[test]
    fn a_shorthand_bound_attribute_is_named_after_its_variable() {
        let calls = scan_component_tag_calls(
            r#"<x-alert :$message />"#,
            &["alert".to_string()],
            &no_arguments,
        );
        assert_eq!(calls[0].bound, vec![("message".to_string(), 0)]);
    }

    #[test]
    fn a_non_matching_tag_contributes_nothing() {
        let calls = scan_component_tag_calls(
            r#"<x-widget foo="bar" />"#,
            &["alert".to_string()],
            &no_arguments,
        );
        assert!(calls.is_empty());
    }

    #[test]
    fn an_echo_interpolated_literal_falls_back_to_a_generic_string() {
        let calls = scan_component_tag_calls(
            r#"<x-alert title="Hello {{ $name }}" />"#,
            &["alert".to_string()],
            &no_arguments,
        );
        assert_eq!(calls[0].literal[0].1, PhpType::string());
    }

    /// A utility class carrying an arbitrary value (`max-h-[80vh]`) puts
    /// brackets inside a quoted attribute value. The scan tracks quotes, so
    /// neither the attribute nor the rest of the tag is cut short by them.
    #[test]
    fn a_bracket_in_a_quoted_attribute_value_does_not_truncate_the_tag() {
        let calls = scan_component_tag_calls(
            r#"<x-modal class="max-h-[80vh]" title="Save" /><x-alert type="danger" />"#,
            &["modal".to_string(), "alert".to_string()],
            &no_arguments,
        );
        assert_eq!(calls.len(), 2, "both tags must be seen");
        assert_eq!(names(&calls[0].literal), vec!["class", "title"]);
        assert_eq!(
            calls[0].literal[0].1,
            PhpType::literal_string_value("max-h-[80vh]")
        );
        assert_eq!(names(&calls[1].literal), vec!["type"]);
    }

    #[test]
    fn a_component_tag_inside_a_comment_is_ignored() {
        let calls = scan_component_tag_calls(
            r#"{{-- <x-alert type="danger" /> --}}"#,
            &["alert".to_string()],
            &no_arguments,
        );
        assert!(calls.is_empty());
    }

    #[test]
    fn scans_an_inline_named_slot() {
        let names = scan_component_tag_slots(
            "<x-card><x-slot:title>Hello</x-slot></x-card>",
            &["card".to_string()],
        );
        assert_eq!(names, vec!["title"]);
    }

    #[test]
    fn scans_the_legacy_name_attribute_form() {
        let names = scan_component_tag_slots(
            r#"<x-card><x-slot name="title">Hello</x-slot></x-card>"#,
            &["card".to_string()],
        );
        assert_eq!(names, vec!["title"]);
    }

    #[test]
    fn camel_cases_a_hyphenated_inline_slot_name() {
        let names = scan_component_tag_slots(
            "<x-card><x-slot:hair-analysis>x</x-slot></x-card>",
            &["card".to_string()],
        );
        assert_eq!(names, vec!["hairAnalysis"]);
    }

    /// Blade's own `Str::camel` transform only fires on the inline form:
    /// a hyphenated `name="…"` attribute is used verbatim, which is not a
    /// legal PHP variable name and so is never actually bound.
    #[test]
    fn a_hyphenated_legacy_name_is_not_camel_cased() {
        let names = scan_component_tag_slots(
            r#"<x-card><x-slot name="hair-analysis">x</x-slot></x-card>"#,
            &["card".to_string()],
        );
        assert_eq!(names, vec!["hair-analysis"]);
    }

    /// A plain HTML tag between the component and its slot does not
    /// change which component receives the slot.
    #[test]
    fn html_nesting_does_not_block_slot_scoping() {
        let names = scan_component_tag_slots(
            "<x-card><div><x-slot:title>Hello</x-slot></div></x-card>",
            &["card".to_string()],
        );
        assert_eq!(names, vec!["title"]);
    }

    /// A slot inside a *different* nested component belongs to that
    /// component, not the outer one.
    #[test]
    fn a_nested_components_own_slot_is_not_the_outer_ones() {
        let names = scan_component_tag_slots(
            "<x-card><x-alert><x-slot:title>Hello</x-slot></x-alert></x-card>",
            &["card".to_string()],
        );
        assert!(names.is_empty());

        let names = scan_component_tag_slots(
            "<x-card><x-alert><x-slot:title>Hello</x-slot></x-alert></x-card>",
            &["alert".to_string()],
        );
        assert_eq!(names, vec!["title"]);
    }

    #[test]
    fn the_same_slot_name_is_reported_once() {
        let names = scan_component_tag_slots(
            "<x-card><x-slot:title>A</x-slot></x-card><x-card><x-slot:title>B</x-slot></x-card>",
            &["card".to_string()],
        );
        assert_eq!(names, vec!["title"]);
    }

    #[test]
    fn a_slot_outside_any_matching_tag_contributes_nothing() {
        let names = scan_component_tag_slots("<x-slot:title>Hello</x-slot>", &["card".to_string()]);
        assert!(names.is_empty());
    }

    /// The lexed attributes as `(name, bound, shorthand, value)` for
    /// readable assertions.
    fn lexed(tag: &str) -> Vec<(&str, bool, bool, Option<&str>)> {
        lex_tag_attributes(tag, 0)
            .attributes
            .into_iter()
            .map(|attr| {
                (
                    &tag[attr.name],
                    attr.bound,
                    attr.shorthand,
                    attr.value.map(|value| &tag[value]),
                )
            })
            .collect()
    }

    #[test]
    fn lexes_every_attribute_shape() {
        assert_eq!(
            lexed(r#" type="info" disabled :items="$a > $b" :$user data-x=1 ::class="a">"#),
            [
                ("type", false, false, Some("info")),
                ("disabled", false, false, None),
                ("items", true, false, Some("$a > $b")),
                ("user", true, true, None),
                ("data-x", false, false, Some("1")),
                (":class", false, false, Some("a")),
            ]
        );
    }

    #[test]
    fn the_tag_ends_at_the_first_close_outside_a_value() {
        let tag = r#" :when="$a > $b" /> tail"#;
        let lexed = lex_tag_attributes(tag, 0);
        assert!(lexed.closed);
        assert!(lexed.self_closing);
        assert_eq!(&tag[lexed.end..], " tail");
    }

    #[test]
    fn a_value_that_never_closes_leaves_the_tag_unclosed() {
        let lexed = lex_tag_attributes(r#" title="oops>"#, 0);
        assert!(!lexed.closed);
        assert_eq!(lexed.attributes.len(), 1);
    }

    #[test]
    fn kebab_matches_laravels_own_spelling() {
        assert_eq!(kebab_case("DatePicker"), "date-picker");
        assert_eq!(kebab_case("Alert"), "alert");
        assert_eq!(kebab_case("HTMLPurifier"), "h-t-m-l-purifier");
        assert_eq!(kebab_case("Create_Refund"), "create_-refund");
    }

    /// The cursor's context is taken at the `|` marker, which is stripped
    /// before the scan so the surrounding text is what the user typed.
    fn context_at(marked: &str) -> Option<TagContext> {
        let offset = marked.find('|').expect("no cursor marker");
        tag_context_at(&marked.replace('|', ""), offset)
    }

    fn name_context(marked: &str) -> Option<(TagKind, String, String)> {
        let offset = marked.find('|').expect("no cursor marker");
        let content = marked.replace('|', "");
        let ctx = tag_context_at(&content, offset)?;
        (ctx.cursor == TagCursor::Name).then(|| {
            (
                ctx.kind,
                ctx.name.clone(),
                content[ctx.token_start..offset].to_string(),
            )
        })
    }

    #[test]
    fn a_bare_opening_is_a_name_with_nothing_typed() {
        assert_eq!(
            name_context("<div><x-|"),
            Some((TagKind::Blade, String::new(), String::new()))
        );
    }

    #[test]
    fn a_partly_typed_name_carries_what_is_typed_so_far() {
        assert_eq!(
            name_context("<x-for|ms.input>"),
            Some((TagKind::Blade, "forms.input".to_string(), "for".to_string()))
        );
    }

    #[test]
    fn a_livewire_opening_is_its_own_kind() {
        assert_eq!(
            name_context("<livewire:coun|"),
            Some((TagKind::Livewire, "coun".to_string(), "coun".to_string()))
        );
    }

    #[test]
    fn a_cursor_after_the_tag_name_is_at_an_attribute() {
        let ctx = context_at("<x-alert |").expect("expected an attribute context");
        assert_eq!(ctx.cursor, TagCursor::Attribute);
        assert_eq!(ctx.name, "alert");
        assert_eq!(ctx.token_start, "<x-alert ".len());
    }

    #[test]
    fn a_partly_typed_attribute_starts_at_its_own_first_character() {
        let ctx = context_at(r#"<x-alert class="a" :mes|"#).expect("expected an attribute");
        assert_eq!(ctx.cursor, TagCursor::Attribute);
        assert_eq!(ctx.token_start, r#"<x-alert class="a" "#.len());
    }

    #[test]
    fn a_closed_tag_leaves_the_cursor_outside_it() {
        assert_eq!(context_at("<x-alert /> |"), None);
        assert_eq!(context_at("<x-alert>{{ $com|"), None);
    }

    /// An attribute value may hold a `>` (`:items=\"$a > $b\"`), so the
    /// scan has to read quotes rather than stop at the first one.
    #[test]
    fn a_greater_than_inside_a_value_does_not_close_the_tag() {
        let ctx = context_at(r#"<x-alert :items="$a > $b" |"#).expect("expected an attribute");
        assert_eq!(ctx.cursor, TagCursor::Attribute);
    }

    #[test]
    fn a_cursor_inside_an_attribute_value_is_not_writing_an_attribute() {
        assert_eq!(context_at(r#"<x-alert type="dan|"#), None);
        assert_eq!(context_at("<x-alert type=dan|"), None);
    }

    #[test]
    fn a_tag_written_inside_a_comment_names_nothing() {
        assert_eq!(context_at("{{-- <x-al| --}}"), None);
    }

    #[test]
    fn a_closing_tag_is_not_an_opening_one() {
        assert_eq!(context_at("<div></x-al|"), None);
    }

    /// The spans of the tags [`tag_spans`] found a body for, as
    /// `(start, end)` pairs, for readable assertions.
    fn tag_bodies(content: &str) -> Vec<(usize, usize)> {
        tag_spans(content)
            .into_iter()
            .filter(|tag| tag.closed)
            .map(|tag| (tag.span.start, tag.span.end))
            .collect()
    }

    #[test]
    fn a_component_tag_body_runs_from_open_to_close() {
        let blade = "<x-alert>\n<p>hi</p>\n</x-alert>\n";
        assert_eq!(tag_bodies(blade), [(0, blade.len() - 1)]);
    }

    #[test]
    fn a_self_closing_tag_has_no_body() {
        let blade = "<x-alert />\n<p>after</p>\n";
        assert!(tag_bodies(blade).is_empty());
        // It is still a tag, spanning itself alone.
        let tags = tag_spans(blade);
        assert_eq!(tags.len(), 1);
        assert_eq!(&blade[tags[0].span.clone()], "<x-alert />");
    }

    #[test]
    fn nested_component_tags_each_span_independently() {
        let blade = "<x-card>\n<x-alert>\n<p>hi</p>\n</x-alert>\n</x-card>\n";
        assert_eq!(tag_bodies(blade).len(), 2);
    }

    #[test]
    fn a_mismatched_closing_tag_closes_nothing() {
        assert!(tag_bodies("<x-alert>\n<p>hi</p>\n</x-card>\n").is_empty());
    }

    /// A stray closing tag ends the innermost open tag, the way Blade
    /// compiles it, so the tag's own closer later closes nothing.
    #[test]
    fn a_stray_closing_tag_ends_the_innermost_tag() {
        let blade = "<x-card>\n<x-alert>\n</x-foo>\n</x-alert>\n</x-card>\n";
        assert!(tag_bodies(blade).is_empty());
        let names: Vec<String> = tag_spans(blade).into_iter().map(|tag| tag.name).collect();
        assert_eq!(names, ["card", "alert"]);
    }

    #[test]
    fn an_unclosed_tag_spans_its_opening_tag_alone() {
        let blade = "<x-alert>\n<p>hi</p>\n";
        assert!(tag_bodies(blade).is_empty());
        let tags = tag_spans(blade);
        assert_eq!(tags.len(), 1);
        assert_eq!(&blade[tags[0].span.clone()], "<x-alert>");
    }

    #[test]
    fn a_livewire_tag_body_is_found() {
        let blade = "<livewire:counter>\n<p>slot</p>\n</livewire:counter>\n";
        assert_eq!(tag_bodies(blade), [(0, blade.len() - 1)]);
        let tags = tag_spans(blade);
        assert_eq!(tags[0].kind, TagKind::Livewire);
        assert_eq!(tags[0].name, "counter");
        assert_eq!(&blade[tags[0].name_span.clone()], "counter");
    }

    #[test]
    fn a_bracket_in_an_attribute_value_does_not_close_the_opening_tag_early() {
        let blade = "<x-alert :items=\"$a > $b\">\n<p>hi</p>\n</x-alert>\n";
        assert_eq!(tag_bodies(blade), [(0, blade.len() - 1)]);
    }

    #[test]
    fn tags_come_back_in_document_order() {
        let blade = "<x-card>\n<x-alert />\n</x-card>\n<x-note />\n";
        let names: Vec<String> = tag_spans(blade).into_iter().map(|tag| tag.name).collect();
        assert_eq!(names, ["card", "alert", "note"]);
    }

    /// A named inline slot may close with the bare `</x-slot>`.
    #[test]
    fn a_named_slot_closes_with_the_bare_tag() {
        let blade = "<x-card>\n<x-slot:title>\nHi\n</x-slot>\n</x-card>\n";
        assert_eq!(tag_bodies(blade).len(), 2);
    }

    #[test]
    fn a_named_slot_closes_with_its_own_name() {
        let blade = "<x-card>\n<x-slot:title>\nHi\n</x-slot:title>\n</x-card>\n";
        let tags = tag_spans(blade);
        assert_eq!(tags.len(), 2);
        assert!(tags.iter().all(|tag| tag.closed));
        assert_eq!(
            &blade[tags[1].span.clone()],
            "<x-slot:title>\nHi\n</x-slot:title>"
        );
    }

    /// The imbalances [`tag_imbalances`] finds, as short strings for
    /// readable assertions: `"mismatched </x-card>/<x-alert>"`,
    /// `"unexpected </x-card>"`, `"unclosed <x-alert>"`.
    fn tag_report(content: &str) -> Vec<String> {
        tag_imbalances(content)
            .into_iter()
            .map(|imbalance| match imbalance {
                TagImbalance::Mismatched { found, opener, .. } => {
                    format!("mismatched </x-{found}>/<x-{opener}>")
                }
                TagImbalance::Unexpected { found, .. } => format!("unexpected </x-{found}>"),
                TagImbalance::Unclosed { opener, .. } => format!("unclosed <x-{opener}>"),
            })
            .collect()
    }

    /// `<x-alert>` closed by `</x-card>` is a mismatched-tag diagnostic:
    /// the innermost open tag is what a wrongly-named closer actually
    /// ends.
    #[test]
    fn a_tag_closed_by_another_components_name_is_mismatched() {
        assert_eq!(
            tag_report("<x-alert>\n<p>hi</p>\n</x-card>\n"),
            ["mismatched </x-card>/<x-alert>"]
        );
    }

    /// `<x-alert>` with no closing tag anywhere is an unclosed-tag
    /// diagnostic.
    #[test]
    fn a_tag_with_no_closing_tag_is_unclosed() {
        assert_eq!(tag_report("<x-alert>\n<p>hi</p>\n"), ["unclosed <x-alert>"]);
    }

    /// A closing tag with nothing open at all closes nothing.
    #[test]
    fn a_closing_tag_with_nothing_open_is_unexpected() {
        assert_eq!(
            tag_report("<p>hi</p>\n</x-alert>\n"),
            ["unexpected </x-alert>"]
        );
    }

    /// Self-closing tags, and tags whose attributes hold a `>`, report
    /// nothing.
    #[test]
    fn self_closing_and_bracket_bearing_tags_report_nothing() {
        assert!(tag_report("<x-alert />\n<p>after</p>\n").is_empty());
        assert!(tag_report("<x-alert :items=\"$a > $b\">\n<p>hi</p>\n</x-alert>\n").is_empty());
    }

    /// A closer for a tag further out reports only the innermost tag it
    /// skipped past, the same as a directive closer that skips open
    /// blocks.
    #[test]
    fn a_closer_matching_a_shallower_tag_reports_what_it_skipped() {
        assert_eq!(
            tag_report("<x-card>\n<x-alert>\n<p>hi</p>\n</x-card>\n"),
            ["mismatched </x-card>/<x-alert>"]
        );
    }

    /// Properly nested and paired tags report nothing.
    #[test]
    fn balanced_tags_report_nothing() {
        assert!(
            tag_report(
                "<x-card>\n<x-alert>\n<p>hi</p>\n</x-alert>\n</x-card>\n<livewire:counter></livewire:counter>\n"
            )
            .is_empty()
        );
    }

    /// Both closing spellings of a named inline slot are balanced.
    #[test]
    fn a_properly_closed_named_slot_reports_nothing() {
        for closer in ["slot", "slot:title"] {
            let blade = format!("<x-card>\n<x-slot:title>\nHi\n</x-{closer}>\n</x-card>\n");
            assert!(tag_report(&blade).is_empty(), "{blade}");
        }
    }

    #[test]
    fn a_named_slot_closed_by_a_different_tag_is_mismatched() {
        for closer in ["slot:other", "alert"] {
            let blade = format!("<x-card>\n<x-slot:title>\nHi\n</x-{closer}>\n</x-card>\n");
            assert_eq!(
                tag_report(&blade),
                [format!("mismatched </x-{closer}>/<x-slot:title>")],
                "{blade}"
            );
        }
    }
}
