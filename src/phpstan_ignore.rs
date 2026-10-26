use std::ops::Range;

use tower_lsp_server::gen_lsp_types::Position;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IgnoreCodeSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IgnoreTagSpan {
    pub start: usize,
    pub end: usize,
}

/// The tag every per-identifier ignore comment starts with.
const TAG: &str = "@phpstan-ignore";

/// An `@phpstan-ignore` comment found on one line.  All offsets are byte
/// offsets into the line that was parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IgnoreComment {
    /// Where the `@` of the tag sits.
    pub tag_start: usize,
    /// `@phpstan-ignore-line` or `@phpstan-ignore-next-line`, which
    /// suppress everything and carry no identifier list.
    pub blanket: bool,
    /// The identifier list, from the start of its first entry to the end
    /// of its last one (reason included), stopping before a closing `*/`.
    /// An empty list sits right after the tag's trailing whitespace.
    pub list: Range<usize>,
    /// The comma-separated entries, split only at commas outside a
    /// parenthesized reason, so `id (legacy, see #12)` is one entry.  An
    /// entry may be empty (`id, ` while the next one is being typed).
    pub entries: Vec<IgnoreEntry>,
    /// Whether the list ends inside a reason whose `(` is not closed.
    pub unclosed: bool,
}

/// One entry of an `@phpstan-ignore` identifier list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IgnoreEntry {
    /// The whole entry, trimmed: the identifier and its reason, if any.
    pub span: Range<usize>,
    /// The identifier the entry starts with (empty when it has none).
    pub id: Range<usize>,
    /// Whether the entry carries a parenthesized reason.
    pub has_reason: bool,
}

impl IgnoreComment {
    /// The identifiers the comment lists, in order.
    pub(crate) fn ids<'a>(&self, line: &'a str) -> impl Iterator<Item = &'a str> {
        self.entries
            .iter()
            .filter(|entry| !entry.id.is_empty())
            .map(|entry| &line[entry.id.clone()])
    }
}

/// Parse the `@phpstan-ignore` comment on `line`, or `None` when the line
/// has none.
pub(crate) fn parse_ignore_comment(line: &str) -> Option<IgnoreComment> {
    let tag_start = line.find(TAG)?;
    let tag_end = tag_start + TAG.len();
    let after_tag = &line[tag_end..];
    if after_tag.starts_with("-line") || after_tag.starts_with("-next-line") {
        return Some(IgnoreComment {
            tag_start,
            blanket: true,
            list: tag_end..tag_end,
            entries: Vec::new(),
            unclosed: false,
        });
    }

    let region_end = tag_end + after_tag.find("*/").unwrap_or(after_tag.len());
    let mut entries = Vec::new();
    let mut depth = 0u32;
    let mut segment_start = tag_end;
    for (idx, ch) in line[tag_end..region_end].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                entries.push(parse_entry(line, segment_start..tag_end + idx));
                segment_start = tag_end + idx + 1;
            }
            _ => {}
        }
    }
    entries.push(parse_entry(line, segment_start..region_end));

    let list_start = entries.first().map_or(tag_end, |entry| entry.span.start);
    let list_end = entries.last().map_or(list_start, |entry| entry.span.end);
    Some(IgnoreComment {
        tag_start,
        blanket: false,
        list: list_start..list_end.max(list_start),
        entries,
        unclosed: depth > 0,
    })
}

/// Parse one comma-separated entry of an identifier list.
fn parse_entry(line: &str, segment: Range<usize>) -> IgnoreEntry {
    let text = &line[segment.clone()];
    let trimmed_start = text.trim_start();
    let start = segment.start + (text.len() - trimmed_start.len());
    let end = start + trimmed_start.trim_end().len();
    let entry = &line[start..end];
    let id_len = if entry.starts_with(is_identifier_start) {
        entry
            .find(|ch: char| !is_identifier_char(ch))
            .unwrap_or(entry.len())
    } else {
        0
    };
    IgnoreEntry {
        span: start..end,
        id: start..start + id_len,
        has_reason: entry.contains('('),
    }
}

pub(crate) fn phpstan_ignore_code_prefix(content: &str, position: Position) -> Option<String> {
    let offset = crate::text_position::position_to_offset(content, position) as usize;
    let offset = offset.min(content.len());
    let line_start = content[..offset]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let before_cursor = &content[line_start..offset];
    // The tag nearest the cursor is the one being completed.
    let tag_pos = before_cursor.rfind(TAG)?;
    let before_cursor = &before_cursor[tag_pos..];
    let comment = parse_ignore_comment(before_cursor)?;
    if comment.blanket || comment.unclosed {
        return None;
    }
    let last = comment.entries.last()?;
    if last.has_reason {
        return None;
    }

    let entry = &before_cursor[last.span.clone()];
    let prefix_len = entry
        .chars()
        .rev()
        .take_while(|ch| is_identifier_char(*ch))
        .map(char::len_utf8)
        .sum::<usize>();
    Some(entry[entry.len() - prefix_len..].to_string())
}

pub(crate) fn phpstan_ignore_code_spans(content: &str) -> Vec<IgnoreCodeSpan> {
    let mut spans = Vec::new();
    for (line_start, line) in crate::text_position::lines_with_offsets(content) {
        let Some(comment) = parse_ignore_comment(line) else {
            continue;
        };
        spans.extend(
            comment
                .entries
                .iter()
                .filter(|entry| !entry.id.is_empty())
                .map(|entry| IgnoreCodeSpan {
                    start: line_start + entry.id.start,
                    end: line_start + entry.id.end,
                }),
        );
    }
    spans
}

pub(crate) fn phpstan_ignore_tag_spans(content: &str) -> Vec<IgnoreTagSpan> {
    crate::text_position::lines_with_offsets(content)
        .filter_map(|(line_start, line)| {
            let comment = parse_ignore_comment(line)?;
            (!comment.blanket).then(|| IgnoreTagSpan {
                start: line_start + comment.tag_start,
                end: line_start + comment.tag_start + TAG.len(),
            })
        })
        .collect()
}

fn is_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')
}
