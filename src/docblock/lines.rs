//! Helpers for reading one line of a docblock as source text.

/// Strip the docblock delimiters from a single trimmed source line so the
/// tag text it carries can be matched.
///
/// Each delimiter is optional and removed independently: `/**`, a trailing
/// `*/`, and the leading `*` of a continuation line.  Stripping them
/// independently is what makes a tag written on the same line as the
/// opening `/**` of a *multi-line* docblock readable — chaining
/// `strip_suffix("*/").unwrap_or(trimmed)` onto the prefix strip would
/// restore the `/**` whenever the line does not also close the block.
pub(crate) fn strip_docblock_line_delimiters(trimmed: &str) -> &str {
    let inner = trimmed.strip_prefix("/**").unwrap_or(trimmed);
    let inner = inner.strip_suffix("*/").unwrap_or(inner);
    inner.trim().trim_start_matches('*').trim()
}

/// The byte offset and length of the first `@var` tag in `line`, in any of
/// its spellings (`@var`, `@phpstan-var`, `@psalm-var`).
pub(crate) fn find_var_tag(line: &str) -> Option<(usize, usize)> {
    ["@phpstan-var", "@psalm-var", "@var"]
        .iter()
        .find_map(|tag| line.find(tag).map(|pos| (pos, tag.len())))
}
