//! Where a class lives on disk and in its file: the PSR-4 path its FQN
//! maps to and the `namespace` statement that declares it.

use std::path::{Path, PathBuf};

/// Compute the PSR-4 file path for a given namespace + class name.
pub(super) fn compute_psr4_path(
    mappings: &[crate::composer::Psr4Mapping],
    workspace_root: &Path,
    namespace: Option<&str>,
    class_name: &str,
) -> Option<PathBuf> {
    let fqn = match namespace {
        Some(ns) => format!("{}\\{}", ns, class_name),
        None => class_name.to_string(),
    };

    for mapping in mappings {
        let relative = if mapping.prefix.is_empty() {
            Some(fqn.as_str())
        } else {
            fqn.strip_prefix(&mapping.prefix)
        };

        if let Some(relative_class) = relative {
            let relative_path = relative_class.replace('\\', "/");
            let file_path = workspace_root
                .join(&mapping.base_path)
                .join(format!("{}.php", relative_path));
            return Some(file_path);
        }
    }

    None
}

/// What the source around a `namespace` name turns out to be, once the
/// move needs to take the whole declaration away rather than rewrite
/// the name in place.
pub(super) enum NamespaceStatement {
    /// A `namespace Foo;` statement occupying this byte range.
    Statement {
        range: std::ops::Range<usize>,
        /// Whether the range swallowed the blank line that followed the
        /// declaration, so anything written in its place has to supply
        /// that separation itself.
        absorbed_blank_line: bool,
    },
    /// `namespace Foo { … }`.  Removing the declaration means unwrapping
    /// the block it opens, which the move does not do.
    Block,
    /// Neither shape: the source does not read the way the symbol map
    /// says it does.
    Unrecognized,
}

/// The `namespace` statement whose name occupies `name_start..name_end`.
///
/// The span the symbol map records covers the name alone, which is all
/// a rename needs.  Removing the declaration takes the keyword before it
/// and the `;` after it as well, plus the line they sit on so the file
/// is not left with a stray blank.
pub(super) fn namespace_statement(
    content: &str,
    name_start: usize,
    name_end: usize,
) -> NamespaceStatement {
    const KEYWORD: &str = "namespace";
    let bytes = content.as_bytes();

    let mut keyword_end = name_start;
    while keyword_end > 0 && bytes[keyword_end - 1].is_ascii_whitespace() {
        keyword_end -= 1;
    }
    let Some(keyword_start) = keyword_end.checked_sub(KEYWORD.len()) else {
        return NamespaceStatement::Unrecognized;
    };
    if !content.is_char_boundary(keyword_start)
        || !content[keyword_start..keyword_end].eq_ignore_ascii_case(KEYWORD)
    {
        return NamespaceStatement::Unrecognized;
    }

    let mut end = name_end;
    while bytes.get(end).is_some_and(u8::is_ascii_whitespace) {
        end += 1;
    }
    match bytes.get(end) {
        Some(b';') => end += 1,
        Some(b'{') => return NamespaceStatement::Block,
        _ => return NamespaceStatement::Unrecognized,
    }

    let line_start = content[..keyword_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let mut start = keyword_start;
    while start > line_start && bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }

    let mut absorbed_blank_line = false;
    if start == line_start {
        let line_end = skip_blanks(bytes, end);
        if bytes.get(line_end) == Some(&b'\n') {
            end = line_end + 1;
            // Removing the line would otherwise leave the blank above
            // the declaration and the blank below it stacked.
            let next_line_end = skip_blanks(bytes, end);
            if ends_with_blank_line(&content[..start]) && bytes.get(next_line_end) == Some(&b'\n') {
                end = next_line_end + 1;
                absorbed_blank_line = true;
            }
        }
    }

    NamespaceStatement::Statement {
        range: start..end,
        absorbed_blank_line,
    }
}

/// Whether `text` ends on a line that holds nothing, so appending to it
/// would leave a blank line above.
fn ends_with_blank_line(text: &str) -> bool {
    let text = text.strip_suffix('\n').unwrap_or(text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    text.ends_with('\n')
}

/// The offset of the first byte at or after `from` that is not
/// horizontal whitespace.
fn skip_blanks(bytes: &[u8], from: usize) -> usize {
    let mut cursor = from;
    while bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
    {
        cursor += 1;
    }
    cursor
}
