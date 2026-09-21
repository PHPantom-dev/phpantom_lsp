//! Pairing the `<x-…>` opening and closing tags of a template: the span
//! each component occupies, for folding and the outline, and the tags
//! left unbalanced, for the diagnostic.

use super::*;

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
            let j = tag_name_end(bytes, name_start);
            let Some(close) = find_byte(bytes, j, b'>') else {
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
        let j = tag_name_end(bytes, name_start);
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
/// Every tag closes under its own name, with one exception Blade's own
/// compiler carves out: any `</x-slot…>` ends the open slot, whatever
/// name either side carries, so `<x-slot:title>` closes with the bare
/// `</x-slot>`, with `</x-slot:title>`, and even with a closer naming a
/// different slot (the compiler's `@endslot` rewrite never reads the
/// closing tag's name).
fn closer_matches(kind: TagKind, name: &str, closer_name: &str) -> bool {
    if kind == TagKind::Blade && is_slot_tag_name(name) {
        is_slot_tag_name(closer_name)
    } else {
        name == closer_name
    }
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
