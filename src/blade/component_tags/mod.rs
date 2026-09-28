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
use crate::text_scan::find_byte;

use super::pairing::{self, Pair, Pairing, Stray, Token};
use super::signature::mask_inert_regions;

mod attributes;
mod spans;
#[cfg(test)]
mod tests;

pub(crate) use attributes::*;
pub(crate) use spans::*;

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
        let j = tag_name_end(bytes, name_start);
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
                i = find_byte(bytes, i, b'>').map_or(bytes.len(), |end| end + 1);
                continue;
            }
            Some(c) if c.is_ascii_alphabetic() => {}
            _ => {
                i += 1;
                continue;
            }
        }
        let name_start = i + 1;
        let j = tag_name_end(bytes, name_start);
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

/// The characters a component tag name is spelled with. Dots separate
/// directories (`forms.input`), a double colon a package namespace
/// (`pkg::calendar`), and `<x-slot:title>` names a slot the same way.
pub(crate) fn is_tag_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | ':' | '_')
}

/// The end of the tag-name run starting at `from`.
pub(crate) fn tag_name_end(bytes: &[u8], from: usize) -> usize {
    let mut j = from;
    while j < bytes.len() && is_tag_name_char(bytes[j] as char) {
        j += 1;
    }
    j
}

/// The characters an HTML attribute name is spelled with, which is a
/// wider set than a tag name's: `wire:model.live`, `x-on:keydown`, and
/// `@click` are all legal there.
pub(crate) fn is_attr_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | ':' | '_' | '@')
}
