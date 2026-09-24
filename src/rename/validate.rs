//! Buffer verification for the edit-producing rename paths.
//!
//! `symbol_maps` is refreshed on a background task after each `didChange`
//! and is left untouched entirely when a parse panics, so a request can
//! read a map whose byte offsets index an *older* version of the file.
//! Every other read-only feature degrades to a wrong hover or a missing
//! completion when that happens.  Rename turns those offsets into
//! `TextEdit`s the editor applies verbatim, so a stale offset silently
//! rewrites unrelated code.
//!
//! Two checks apply here:
//!
//! 1. **The symbol map must describe the text it is converted against.**
//!    Cross-file edits are checked per file, against *that* file's
//!    content, because the request only carries the buffer it arrived on.
//! 2. **Every range must still cover the token it claims to.** A
//!    same-length edit leaves `matches_source` happy but can still move a
//!    token, so the text at each range is re-read from the buffer.
//!
//! Both are all-or-nothing.  A rename that drops the occurrences it
//! cannot verify leaves the code half-renamed, which is worse than not
//! renaming at all; the user re-triggers rename once the background parse
//! lands.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::{Location, Range};

use crate::Backend;
use crate::symbol_map::{SymbolKind, SymbolSpan};
use crate::text_position::position_to_byte_offset;

/// What the source text at a reference range must still look like.
enum Expected<'a> {
    /// The exact identifier.  Variables and class members cannot be
    /// reached under a second name, so a reference that no longer spells
    /// this is a stale offset.
    Exact(&'a str),
    /// Any complete PHP name token.  Classes, functions, and constants
    /// can be imported under an alias (`use Foo as Bar;`), and a
    /// constructor's references are `new ClassName(...)` sites, so the
    /// text at a reference site is not predictable from the symbol name.
    /// All we can require is that the range still covers one whole name.
    Name,
    /// The full key, or one of its segments.  A Laravel string key is
    /// written out in full at its use sites (`config('app.name')`) and as
    /// a single segment at its declaration (`'name' => ...`).
    Key(&'a str),
}

impl Backend {
    /// Whether the symbol map for `uri` still describes `content`.
    ///
    /// A file with no map contributes no map-derived offsets, so it has
    /// nothing to invalidate; the per-range check below still applies to
    /// the locations it produced through other indices.
    pub(super) fn rename_map_matches(&self, uri: &str, content: &str) -> bool {
        self.symbol_maps
            .read()
            .get(uri)
            .is_none_or(|map| map.matches_source(content))
    }

    /// Verify every reference location before it is turned into an edit.
    ///
    /// Returns `false` as soon as one location fails, which drops the
    /// whole rename.
    pub(super) fn rename_locations_verified(
        &self,
        kind: &SymbolKind,
        locations: &[Location],
    ) -> bool {
        let expected = expected_text(kind);

        // Reference results are grouped by file only after this point, so
        // cache the content per URI rather than re-reading it (and, for a
        // file on disk, re-hitting the filesystem) once per occurrence.
        let mut contents: HashMap<&str, Option<Arc<String>>> = HashMap::new();

        for location in locations {
            let uri = location.uri.as_str();
            let content = contents
                .entry(uri)
                .or_insert_with(|| self.reference_file_content_arc(uri));
            let Some(content) = content.as_deref() else {
                return false;
            };
            if !self.rename_map_matches(uri, content) {
                return false;
            }
            if !range_matches(content, location.range, &expected) {
                return false;
            }
        }

        true
    }
}

/// Whether the source text under `span` still spells the name the symbol
/// map recorded for it.
///
/// This is the single-span counterpart of
/// [`Backend::rename_locations_verified`], for the cursor span itself:
/// prepare-rename hands its range straight to the editor, and the rename
/// dispatch decides which handler to run from the span's kind and name.
pub(super) fn span_spells_its_name(content: &str, span: &SymbolSpan) -> bool {
    let Some(name) = span_name(&span.kind) else {
        // Keywords and comments carry no name; nothing to compare, and
        // neither is renameable in the first place.
        return true;
    };
    let Some(text) = content.get(span.start as usize..span.end as usize) else {
        return false;
    };
    // Property and static-property spans include the `$` sigil, the
    // recorded name never does.  A fully-qualified reference (`\Foo\bar()`)
    // spans its leading `\` while the recorded name is normalised without
    // one, so neither side may carry it into the comparison.
    let text = text.strip_prefix('$').unwrap_or(text);
    let text = text.strip_prefix('\\').unwrap_or(text);
    let name = name.strip_prefix('\\').unwrap_or(name);
    text.eq_ignore_ascii_case(name)
}

/// The name a span's own source text should spell.
fn span_name(kind: &SymbolKind) -> Option<&str> {
    match kind {
        SymbolKind::ClassReference { name, .. }
        | SymbolKind::ClassDeclaration { name }
        | SymbolKind::Variable { name }
        | SymbolKind::CompactVariable { name }
        | SymbolKind::FunctionCall { name, .. }
        | SymbolKind::ConstantReference { name, .. }
        | SymbolKind::MemberDeclaration { name, .. }
        | SymbolKind::NamespaceDeclaration { name } => Some(name.as_str()),
        SymbolKind::MemberAccess { member_name, .. } => Some(member_name.as_str()),
        SymbolKind::LaravelMacroString { name } => Some(name),
        SymbolKind::LaravelStringKey { key, .. } => Some(key),
        SymbolKind::SelfStaticParent(_)
        | SymbolKind::CommandOwnParam { .. }
        | SymbolKind::Keyword
        | SymbolKind::CastType
        | SymbolKind::Comment => None,
    }
}

/// What the reference sites of `kind` must still spell.
fn expected_text(kind: &SymbolKind) -> Expected<'_> {
    match kind {
        SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => {
            Expected::Exact(name.as_str())
        }
        SymbolKind::MemberAccess { member_name, .. }
            if !crate::references::is_constructor_name(member_name) =>
        {
            Expected::Exact(member_name.as_str())
        }
        SymbolKind::MemberDeclaration { name, .. }
            if !crate::references::is_constructor_name(name) =>
        {
            Expected::Exact(name.as_str())
        }
        SymbolKind::LaravelMacroString { name } => Expected::Exact(name),
        SymbolKind::LaravelStringKey { key, .. } => Expected::Key(key),
        _ => Expected::Name,
    }
}

/// Whether the text `range` covers in `content` is still the token the
/// rename expects to replace.
fn range_matches(content: &str, range: Range, expected: &Expected) -> bool {
    let start = position_to_byte_offset(content, range.start);
    let end = position_to_byte_offset(content, range.end);
    if start >= end {
        return false;
    }
    let Some(text) = content.get(start..end) else {
        return false;
    };

    // The range must start and end on a token boundary, catching a
    // shifted offset that lands part-way into a longer identifier.
    let before_ok = content
        .get(..start)
        .and_then(|s| s.chars().next_back())
        .is_none_or(|c| !is_name_char(c));
    let after_ok = content
        .get(end..)
        .and_then(|s| s.chars().next())
        .is_none_or(|c| !is_name_char(c));
    if !before_ok || !after_ok {
        return false;
    }

    match expected {
        Expected::Exact(name) => {
            let token = text.strip_prefix('$').unwrap_or(text);
            token.eq_ignore_ascii_case(name)
        }
        Expected::Name => is_name_token(text),
        Expected::Key(key) => {
            // A view name is stored in its canonical dotted spelling, so
            // the source may still read `partials/card` for `partials.card`.
            text.eq_ignore_ascii_case(key)
                || crate::virtual_members::laravel::canonical_view_name(text)
                    .eq_ignore_ascii_case(key)
                || key.split(['.', '/']).any(|segment| segment == text)
        }
    }
}

/// Whether `new_name` is something a symbol of `kind` can be renamed to.
///
/// Only PHP names are checked.  A Laravel string key is free text (it may
/// hold dots, dashes, or slashes), so it is left to its own rename path.
pub(super) fn is_valid_new_name(kind: &SymbolKind, new_name: &str) -> bool {
    match kind {
        SymbolKind::Variable { .. }
        | SymbolKind::CompactVariable { .. }
        | SymbolKind::MemberAccess { .. }
        | SymbolKind::MemberDeclaration { .. } => {
            let bare = new_name.strip_prefix('$').unwrap_or(new_name);
            !bare.contains('\\') && bare != "this" && is_name_token(bare)
        }
        SymbolKind::ClassReference { .. }
        | SymbolKind::ClassDeclaration { .. }
        | SymbolKind::FunctionCall { .. }
        | SymbolKind::ConstantReference { .. }
        | SymbolKind::NamespaceDeclaration { .. } => {
            !new_name.starts_with('$') && is_name_token(new_name)
        }
        _ => true,
    }
}

/// Whether `text` is one complete PHP name: `$var`, `Foo`, `Ns\Foo`, or
/// `\Ns\Foo`.
fn is_name_token(text: &str) -> bool {
    let body = text.strip_prefix('$').unwrap_or(text);
    let body = body.strip_prefix('\\').unwrap_or(body);
    !body.is_empty()
        && body.split('\\').all(|segment| {
            !segment.is_empty()
                && !segment.starts_with(|c: char| c.is_ascii_digit())
                && segment.chars().all(is_name_char)
        })
}

/// Whether `c` can appear inside a PHP identifier.  PHP allows every byte
/// from `0x80` up, which is why this is not just `is_ascii_alphanumeric`.
fn is_name_char(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric() || (c as u32) >= 0x80
}
