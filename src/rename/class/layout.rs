//! Where a class lives: the `namespace` statement that declares it in
//! its file, and the file itself, which a rename or move carries along
//! to the path [`psr4_path_for_class`] gives the new name.

use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::composer::psr4_path_for_class;
use crate::text_position::offset_to_position;

use super::siblings::{SiblingImport, build_sibling_import_edits};

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

impl Backend {
    /// Check whether renaming a class should also rename the file.
    ///
    /// Returns the old and new file URIs as `(old_uri, new_uri)` when:
    /// 1. The client supports file rename operations.
    /// 2. The definition file's basename (without `.php`) matches the
    ///    old class short name.
    /// 3. The file contains exactly one class/interface/trait/enum
    ///    declaration.
    pub(super) fn should_rename_file(
        &self,
        old_fqn: &str,
        new_short_name: &str,
    ) -> Option<(Url, Url)> {
        if !self.supports_file_rename.load(Ordering::Acquire) {
            return None;
        }

        let old_short = crate::util::short_name(old_fqn);

        let def_uri_str = self.symbols.fqn_uri_index.read().get(old_fqn).cloned()?;

        let def_url = Url::parse(&def_uri_str).ok()?;
        let def_path = def_url.to_file_path().ok()?;

        let stem = def_path.file_stem()?.to_str()?;
        if stem != old_short {
            return None;
        }

        let classes = self.get_classes_for_uri(&def_uri_str)?;
        if classes.len() != 1 {
            return None;
        }

        let mut new_path = def_path.clone();
        new_path.set_file_name(format!("{}.php", new_short_name));

        let new_url = Url::from_file_path(&new_path).ok()?;

        Some((def_url, new_url))
    }

    /// Compute the file move for a class being moved to a new FQN.
    ///
    /// Returns `Some((old_uri, new_uri))` when the file can be moved
    /// to match the new PSR-4 location.
    pub(super) fn compute_class_file_move(
        &self,
        old_fqn: &str,
        new_fqn: &str,
    ) -> Option<(Url, Url)> {
        if !self.supports_file_rename.load(Ordering::Acquire) {
            return None;
        }

        let def_uri_str = self.symbols.fqn_uri_index.read().get(old_fqn).cloned()?;
        let old_url = Url::parse(&def_uri_str).ok()?;

        let workspace_root = self.workspace_root().read().clone()?;
        let mappings = self.psr4_mappings().read().clone();

        let new_path = psr4_path_for_class(&mappings, &workspace_root, new_fqn)?;
        let new_url = Url::from_file_path(&new_path).ok()?;

        if old_url == new_url {
            return None;
        }

        // A `RenameFile` onto a path that is already there is destructive
        // in every editor that honours it. `build_class_move_edit`
        // refuses the move before reaching this point, so a path that
        // still exists here holds something PSR-4 does not account for.
        if new_path.exists() {
            return None;
        }

        Some((old_url, new_url))
    }

    /// Why a class cannot move to `new_fqn`, or `None` when the
    /// destination is free.
    ///
    /// A class already declared under that name is the blocking case:
    /// the move would leave two declarations claiming it, and every
    /// reference the rename rewrites would then name whichever one the
    /// autoloader reaches first. The PSR-4 destination file is checked
    /// too, since a file can sit there without the index having a class
    /// for it.
    pub(super) fn class_move_conflict(&self, old_fqn: &str, new_fqn: &str) -> Option<String> {
        if let Some((declared, uri)) = self
            .symbols
            .fqn_uri_index
            .read()
            .get_key_value(new_fqn)
            .map(|(k, v)| (k.to_string(), v.clone()))
            && !declared.eq_ignore_ascii_case(old_fqn)
        {
            return Some(format!(
                "Cannot rename to `{}`: a class with that name is already declared in {}.",
                declared,
                display_uri(&uri)
            ));
        }

        let workspace_root = self.workspace_root().read().clone()?;
        let mappings = self.psr4_mappings().read().clone();
        let new_path = psr4_path_for_class(&mappings, &workspace_root, new_fqn)?;

        let old_path = self
            .symbols
            .fqn_uri_index
            .read()
            .get(old_fqn)
            .and_then(|u| Url::parse(u).ok())
            .and_then(|u| u.to_file_path().ok());

        if new_path.exists() && old_path.as_deref() != Some(new_path.as_path()) {
            return Some(format!(
                "Cannot rename to `{}`: {} already exists.",
                new_fqn,
                new_path.display()
            ));
        }

        None
    }
}

/// A file URI rendered as a plain path for a user-facing message.
pub(super) fn display_uri(uri: &str) -> String {
    Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| uri.to_string())
}

/// The edit that adds a `namespace` line to a file that had none.
///
/// The `namespace` line and the first import would be inserted at the
/// same offset, and two edits sharing one offset land in whichever order
/// the client applies them.  Writing both as one edit fixes the order.
pub(super) fn insert_namespace_edit(
    content: &str,
    ns: &str,
    siblings: &[SiblingImport],
) -> TextEdit {
    let insert_line = crate::text_scan::header_insert_line(content);
    let mut new_text = format!("namespace {};\n\n", ns);
    for import in siblings {
        new_text.push_str(&import.statement);
        new_text.push('\n');
    }
    if !siblings.is_empty() {
        new_text.push('\n');
    }
    let at = Position {
        line: insert_line,
        character: 0,
    };
    TextEdit {
        range: Range { start: at, end: at },
        new_text,
    }
}

/// The edits that take a file's `namespace` statement out when the class
/// moves into the global namespace.
///
/// `Ok(None)` when the statement is not one the move knows how to remove.
pub(super) fn remove_namespace_edits(
    old_fqn: &str,
    file_uri_str: &str,
    content: &str,
    span_start: usize,
    span_end: usize,
    siblings: &[SiblingImport],
) -> Result<Option<Vec<TextEdit>>, String> {
    match namespace_statement(content, span_start, span_end) {
        NamespaceStatement::Statement {
            range,
            absorbed_blank_line,
        } => {
            let use_block = crate::completion::use_edit::analyze_use_block(content);
            // With no import block to sort into, a sibling import lands on
            // the line the removal takes away.  Writing both as one edit
            // keeps them off each other.
            let inline_siblings = use_block.existing.is_empty() && !siblings.is_empty();
            let mut new_text = String::new();
            if inline_siblings {
                for import in siblings {
                    new_text.push_str(&import.statement);
                    new_text.push('\n');
                }
                if absorbed_blank_line {
                    new_text.push('\n');
                }
            }
            let mut edits = vec![TextEdit {
                range: Range {
                    start: offset_to_position(content, range.start),
                    end: offset_to_position(content, range.end),
                },
                new_text,
            }];
            if !inline_siblings {
                edits.extend(build_sibling_import_edits(content, siblings));
            }
            Ok(Some(edits))
        }
        NamespaceStatement::Block => Err(format!(
            "Cannot move `{}` into the global namespace: {} writes its namespace as a \
             brace block, which the move would have to unwrap.",
            old_fqn,
            display_uri(file_uri_str)
        )),
        NamespaceStatement::Unrecognized => Ok(None),
    }
}
