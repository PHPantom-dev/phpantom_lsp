//! Source-text patching helpers used to recover a parseable AST when the
//! user is mid-keystroke (an unclosed call, a bare `->`/`?->` with
//! nothing after it).  Shared by [`super::member_access`] and
//! [`super::named_args`].

use tower_lsp::lsp_types::Position;

use crate::Backend;

impl Backend {
    /// Patch incomplete member-access expressions for parser recovery.
    ///
    /// When the cursor is right after `->` or `?->` and the line has no
    /// semicolon, the PHP parser may fail to recognise the enclosing
    /// statement (e.g. an arrow function body).  This inserts a dummy
    /// identifier and semicolon (`_x;`) at the cursor so the parser can
    /// recover the surrounding structure.
    pub(super) fn patch_incomplete_member_access(content: &str, position: Position) -> String {
        let line_idx = position.line as usize;
        let col = position.character as usize;
        let mut result = String::with_capacity(content.len() + 4);

        for (i, line) in content.lines().enumerate() {
            if i == line_idx {
                let byte_col = line
                    .char_indices()
                    .nth(col)
                    .map(|(idx, _)| idx)
                    .unwrap_or(line.len());
                // Only patch when the cursor is right after `->` or
                // `?->` with nothing meaningful following it.
                let before = &line[..byte_col];
                let after = line[byte_col..].trim();
                if (before.ends_with("->") || before.ends_with("?->")) && after.is_empty() {
                    result.push_str(before);
                    result.push_str("_x;");
                    result.push_str(&line[byte_col..]);
                } else {
                    result.push_str(line);
                }
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }

        if !content.ends_with('\n') && result.ends_with('\n') {
            result.pop();
        }

        result
    }

    /// Insert `);` at the given cursor position in `content`.
    ///
    /// This produces a patched version of the source that the parser can
    /// handle when the user is in the middle of typing a function call
    /// (e.g. `$this->greet(|` where the closing `)` hasn't been typed
    /// yet).  Closing the call expression lets the parser recover the
    /// surrounding class/function structure.
    pub(super) fn patch_content_at_cursor(content: &str, position: Position) -> String {
        let line_idx = position.line as usize;
        let col = position.character as usize;
        let mut result = String::with_capacity(content.len() + 2);

        for (i, line) in content.lines().enumerate() {
            if i == line_idx {
                // Insert `);` at the cursor column
                let byte_col = line
                    .char_indices()
                    .nth(col)
                    .map(|(idx, _)| idx)
                    .unwrap_or(line.len());
                result.push_str(&line[..byte_col]);
                result.push_str(");");
                result.push_str(&line[byte_col..]);
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }

        // Remove the trailing newline we may have added if the original
        // content did not end with one.
        if !content.ends_with('\n') && result.ends_with('\n') {
            result.pop();
        }

        result
    }
}
