//! Helpers every code action shares: the `WorkspaceEdit` builders, the
//! indentation probes, the opaque `data` payload of a deferred action, and
//! the occurrence search the extract actions use.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::sequence::Sequence;
use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::*;

// ─── Shared edit builders ─────────────────────────────────────────────────────

/// Build a [`WorkspaceEdit`] that applies a set of text edits to a single file.
///
/// Nearly every code action produces edits for exactly one document.  This
/// wraps the `changes` map construction so handlers don't each open-code the
/// `document_changes: None` / `change_annotations: None` boilerplate.
pub(crate) fn single_file_edit(uri: Url, edits: Vec<TextEdit>) -> WorkspaceEdit {
    let mut changes = HashMap::new();
    changes.insert(uri, edits);
    WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    }
}

/// Build a [`WorkspaceEdit`] that applies a single text edit to one file.
///
/// Convenience wrapper over [`single_file_edit`] for the common case of one
/// range → one replacement string.
pub(crate) fn single_edit(uri: Url, range: Range, new_text: String) -> WorkspaceEdit {
    single_file_edit(uri, vec![TextEdit { range, new_text }])
}

/// Build a `WorkspaceEdit` that creates `uri` holding `content`, followed
/// by `edits` to files that already exist.
///
/// A file creation is a resource operation, which only `document_changes`
/// can carry, so the accompanying edits travel in the same list rather than
/// in `changes`. Offer an action built this way only when the client has
/// advertised `create` among its resource operations
/// (`Backend::supports_file_create`); one that has not ignores the whole
/// edit.
pub(crate) fn create_file_edit(
    uri: Url,
    content: String,
    edits: Vec<(Url, Vec<TextEdit>)>,
) -> WorkspaceEdit {
    let mut operations = vec![DocumentChangeOperation::Op(ResourceOp::Create(
        CreateFile {
            uri: uri.clone(),
            options: Some(CreateFileOptions {
                overwrite: Some(false),
                ignore_if_exists: Some(true),
            }),
            annotation_id: None,
        },
    ))];
    // An empty file is complete once created; only content needs an edit.
    if !content.is_empty() {
        operations.push(DocumentChangeOperation::Edit(TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
            edits: vec![OneOf::Left(TextEdit {
                range: Range::default(),
                new_text: content,
            })],
        }));
    }
    operations.extend(edits.into_iter().map(|(uri, edits)| {
        DocumentChangeOperation::Edit(TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
            edits: edits.into_iter().map(OneOf::Left).collect(),
        })
    }));
    WorkspaceEdit {
        changes: None,
        document_changes: Some(DocumentChanges::Operations(operations)),
        change_annotations: None,
    }
}

// ─── Indentation helpers ──────────────────────────────────────────────────────

/// Return the leading whitespace of the line containing `offset`.
///
/// This is the raw indentation of that line, without adding an extra level.
pub(crate) fn indent_of_line_at(content: &str, offset: usize) -> String {
    let before = &content[..offset.min(content.len())];
    let line_start = before.rfind('\n').map_or(0, |p| p + 1);
    content[line_start..offset.min(content.len())]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect()
}

/// Detect the file's indentation unit (a tab, two spaces, or four spaces).
///
/// Scans lines for the first indented line and infers the convention,
/// defaulting to four spaces when nothing indented is found.
pub(crate) fn indent_unit(content: &str) -> &'static str {
    for line in content.lines() {
        if line.starts_with('\t') {
            return "\t";
        }
        let spaces: usize = line.chars().take_while(|c| *c == ' ').count();
        if spaces >= 2 {
            if spaces.is_multiple_of(4) {
                return "    ";
            }
            return "  ";
        }
    }
    "    "
}

// ─── Shared helpers ─────────────────────────────────────────────────────────

/// Detect indentation from the first class member's position in the source.
///
/// Looks at the line containing the first member to determine the
/// indent string.  Falls back to four spaces.
pub(crate) fn detect_indent_from_members<'a>(
    members: &Sequence<'a, ClassLikeMember<'a>>,
    content: &str,
) -> String {
    if let Some(first) = members.first() {
        let offset = first.span().start.offset as usize;
        let line_start = content[..offset]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        let line_prefix = &content[line_start..offset];
        let indent: String = line_prefix
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        if !indent.is_empty() {
            return indent;
        }
    }

    // Fallback: four spaces.
    "    ".to_string()
}

// ─── Resolve data ───────────────────────────────────────────────────────────

/// Opaque data attached to a `CodeAction` for deferred edit computation.
///
/// Serialized into the `data` field of `CodeAction` during Phase 1.
/// Deserialized in the `codeAction/resolve` handler (Phase 2) to
/// recompute the workspace edit on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CodeActionData {
    /// Identifies which action this is (e.g. `"phpstan.addThrows"`,
    /// `"refactor.extractFunction"`).
    pub action_kind: String,
    /// The file URI the action applies to.
    pub uri: String,
    /// The cursor/selection range from the original `codeAction` request.
    pub range: Range,
    /// Action-specific context needed to recompute the edit.
    ///
    /// For PHPStan actions this carries the diagnostic message,
    /// identifier, and line number.  For refactoring actions it
    /// carries whatever lightweight context avoids a full re-scan.
    #[serde(default)]
    pub extra: serde_json::Value,
}

/// Build a [`CodeActionData`] value and serialize it to JSON.
pub(crate) fn make_code_action_data(
    action_kind: &str,
    uri: &str,
    range: &Range,
    extra: serde_json::Value,
) -> serde_json::Value {
    serde_json::to_value(CodeActionData {
        action_kind: action_kind.to_string(),
        uri: uri.to_string(),
        range: *range,
        extra,
    })
    .unwrap_or_default()
}

/// Find all occurrences of `needle` in `content` within the byte range
/// `[scope_start, scope_end)` that are textually identical to the selected
/// expression, excluding the original selection `[sel_start, sel_end)`.
///
/// Returns `(start, end)` byte offset pairs. Word boundaries are checked
/// so that substrings of longer identifiers are not matched.
pub(crate) fn find_identical_occurrences(
    content: &str,
    needle: &str,
    sel_start: usize,
    sel_end: usize,
    scope_start: usize,
    scope_end: usize,
) -> Vec<(usize, usize)> {
    if needle.is_empty() || scope_start >= scope_end || scope_end > content.len() {
        return Vec::new();
    }
    let haystack = &content[scope_start..scope_end];
    let mut results = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = haystack[search_from..].find(needle) {
        let abs_start = scope_start + search_from + pos;
        let abs_end = abs_start + needle.len();
        // Skip the original selection.
        if abs_start != sel_start || abs_end != sel_end {
            // Check word boundaries to avoid matching substrings.
            let before_ok = abs_start == 0
                || !content.as_bytes()[abs_start - 1].is_ascii_alphanumeric()
                    && content.as_bytes()[abs_start - 1] != b'_'
                    && content.as_bytes()[abs_start - 1] != b'$';
            let after_ok = abs_end >= content.len()
                || !content.as_bytes()[abs_end].is_ascii_alphanumeric()
                    && content.as_bytes()[abs_end] != b'_';
            if before_ok && after_ok {
                results.push((abs_start, abs_end));
            }
        }
        search_from = search_from + pos + 1;
    }
    results
}
