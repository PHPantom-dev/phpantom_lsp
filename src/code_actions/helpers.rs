//! Helpers every code action shares: the `WorkspaceEdit` builders, the
//! indentation probes, the opaque `data` payload of a deferred action, the
//! test for which symbol-map spans a request covers, and the occurrence
//! search the extract actions use.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::sequence::Sequence;
use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::*;

use crate::symbol_map::SymbolSpan;

// ─── Shared edit builders ─────────────────────────────────────────────────────

/// Build a [`WorkspaceEdit`] that applies a set of text edits to a single file.
///
/// Nearly every code action produces edits for exactly one document.  This
/// wraps the `changes` map construction so handlers don't each open-code the
/// `document_changes: None` / `change_annotations: None` boilerplate.
pub(crate) fn single_file_edit(uri: Url, edits: Vec<TextEdit>) -> WorkspaceEdit {
    let mut changes = HashMap::new();
    changes.insert(uri, edits);
    multi_file_edit(changes)
}

/// Build a [`WorkspaceEdit`] that applies text edits to any number of
/// files, none of which is created, renamed, or deleted.
pub(crate) fn multi_file_edit(changes: HashMap<Url, Vec<TextEdit>>) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    }
}

/// The edit that replaces one occurrence of a variable at `offset` with
/// `replacement`, or `None` when `content` no longer spells `var_name`
/// there.
///
/// Every occurrence-rewriting action (inlining a variable, promoting one
/// to a property) collects offsets from a pre-computed scan and only
/// turns each into an edit once it re-reads the live text at that
/// position, since the scan and the edit can be separated by other edits
/// changing the file in between.
pub(crate) fn occurrence_replacement_edit(
    content: &str,
    offset: usize,
    var_name: &str,
    replacement: &str,
) -> Option<TextEdit> {
    let end = offset + var_name.len();
    if end > content.len() || content[offset..end] != *var_name {
        return None;
    }
    Some(TextEdit {
        range: crate::text_position::byte_range_to_lsp_range(content, offset, end),
        new_text: replacement.to_string(),
    })
}

/// Sort text edits into document order.
///
/// An occurrence-rewriting action collects one edit per reference as it
/// finds them, which need not be in source order (a static-property
/// reference documented ahead of the declaration it promotes, say); most
/// editors apply edits as given rather than sorting them first.
pub(crate) fn sort_edits_by_position(edits: &mut [TextEdit]) {
    edits.sort_by(|a, b| {
        a.range
            .start
            .line
            .cmp(&b.range.start.line)
            .then(a.range.start.character.cmp(&b.range.start.character))
    });
}

/// Build a [`WorkspaceEdit`] that applies a single text edit to one file.
///
/// Convenience wrapper over [`single_file_edit`] for the common case of one
/// range → one replacement string.
pub(crate) fn single_edit(uri: Url, range: Range, new_text: String) -> WorkspaceEdit {
    single_file_edit(uri, vec![TextEdit { range, new_text }])
}

/// Build a `WorkspaceEdit` whose `document_changes` carries `ops` (file
/// creations, renames, deletions) followed by one `TextDocumentEdit` per
/// entry of `edits`.
///
/// A resource operation only travels in `document_changes`, so the text
/// edits that accompany one go in the same list rather than in `changes`.
/// Offer an edit built this way only when the client has advertised the
/// operations it carries (`Backend::supports_file_create`,
/// `Backend::supports_file_rename`); one that has not ignores the whole
/// edit.
pub(crate) fn document_changes_edit(
    ops: impl IntoIterator<Item = ResourceOp>,
    edits: impl IntoIterator<Item = (Url, Vec<TextEdit>)>,
) -> WorkspaceEdit {
    let operations = ops
        .into_iter()
        .map(DocumentChangeOperation::Op)
        .chain(edits.into_iter().map(|(uri, edits)| {
            DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            })
        }))
        .collect();
    WorkspaceEdit {
        changes: None,
        document_changes: Some(DocumentChanges::Operations(operations)),
        change_annotations: None,
    }
}

/// Build a `WorkspaceEdit` that creates `uri` holding `content`, followed
/// by `edits` to files that already exist.
pub(crate) fn create_file_edit(
    uri: Url,
    content: String,
    edits: Vec<(Url, Vec<TextEdit>)>,
) -> WorkspaceEdit {
    let create = ResourceOp::Create(CreateFile {
        uri: uri.clone(),
        options: Some(CreateFileOptions {
            overwrite: Some(false),
            ignore_if_exists: Some(true),
        }),
        annotation_id: None,
    });
    // An empty file is complete once created; only content needs an edit.
    let content_edit = (!content.is_empty()).then(|| {
        (
            uri,
            vec![TextEdit {
                range: Range::default(),
                new_text: content,
            }],
        )
    });
    document_changes_edit([create], content_edit.into_iter().chain(edits))
}

// ─── Walking an existing edit ─────────────────────────────────────────────────

/// One file's text edits inside a [`WorkspaceEdit`], in whichever of the two
/// shapes carries them.
pub(crate) enum FileTextEdits<'a> {
    /// The edits of a `changes` entry.
    Plain(&'a mut Vec<TextEdit>),
    /// The edits of a `TextDocumentEdit`, each plain or annotated.
    Document(&'a mut Vec<OneOf<TextEdit, AnnotatedTextEdit>>),
}

impl FileTextEdits<'_> {
    /// Keep only the edits `keep` returns `true` for, handing it each edit's
    /// plain [`TextEdit`] (an annotated edit's inner one) to inspect or
    /// rewrite.
    pub(crate) fn retain_mut(&mut self, mut keep: impl FnMut(&mut TextEdit) -> bool) {
        match self {
            Self::Plain(edits) => edits.retain_mut(keep),
            Self::Document(edits) => edits.retain_mut(|edit| keep(plain_text_edit(edit))),
        }
    }

    /// Run `f` on every edit's plain [`TextEdit`].
    pub(crate) fn for_each_mut(&mut self, mut f: impl FnMut(&mut TextEdit)) {
        match self {
            Self::Plain(edits) => edits.iter_mut().for_each(f),
            Self::Document(edits) => edits.iter_mut().for_each(|edit| f(plain_text_edit(edit))),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Self::Plain(edits) => edits.is_empty(),
            Self::Document(edits) => edits.is_empty(),
        }
    }
}

fn plain_text_edit(edit: &mut OneOf<TextEdit, AnnotatedTextEdit>) -> &mut TextEdit {
    match edit {
        OneOf::Left(edit) => edit,
        OneOf::Right(edit) => &mut edit.text_edit,
    }
}

/// Call `visit` with every file's text edits in `edit` and the file they go
/// into, whether `changes` or `document_changes` carries them.
///
/// Returns whether `edit` also holds a resource operation (a file
/// creation, rename, or deletion), which carries no text edits to visit.
pub(crate) fn visit_file_text_edits(
    edit: &mut WorkspaceEdit,
    mut visit: impl FnMut(&Url, FileTextEdits<'_>),
) -> bool {
    if let Some(changes) = edit.changes.as_mut() {
        for (uri, edits) in changes.iter_mut() {
            visit(uri, FileTextEdits::Plain(edits));
        }
    }
    let mut has_resource_op = false;
    match edit.document_changes.as_mut() {
        Some(DocumentChanges::Edits(documents)) => {
            for document in documents {
                visit(
                    &document.text_document.uri,
                    FileTextEdits::Document(&mut document.edits),
                );
            }
        }
        Some(DocumentChanges::Operations(operations)) => {
            for operation in operations {
                match operation {
                    DocumentChangeOperation::Edit(document) => visit(
                        &document.text_document.uri,
                        FileTextEdits::Document(&mut document.edits),
                    ),
                    DocumentChangeOperation::Op(_) => has_resource_op = true,
                }
            }
        }
        None => {}
    }
    has_resource_op
}

// ─── Indentation helpers ──────────────────────────────────────────────────────

/// Return the leading whitespace of the line containing `offset`.
///
/// This is the raw indentation of that line, without adding an extra level.
pub(crate) fn indent_of_line_at(content: &str, offset: usize) -> &str {
    let offset = offset.min(content.len());
    let line = &content[crate::text_position::find_line_start(content, offset)..offset];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
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
        let indent = indent_of_line_at(content, first.span().start.offset as usize);
        if !indent.is_empty() {
            return indent.to_string();
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

/// A deferred `quickfix` for `diagnostics`: no edit, and a `data` payload
/// the resolve recomputes the edit from.  The payload carries `extra`'s
/// own fields plus the first diagnostic's message, line and code, as
/// `diagnostic_message`, `diagnostic_line` and `diagnostic_code`.
pub(crate) fn deferred_quickfix(
    title: String,
    action_kind: &str,
    uri: &str,
    range: &Range,
    diagnostics: Vec<Diagnostic>,
    is_preferred: bool,
    extra: serde_json::Value,
) -> CodeActionOrCommand {
    let mut extra = match extra {
        serde_json::Value::Object(fields) => fields,
        _ => serde_json::Map::new(),
    };
    if let Some(diag) = diagnostics.first() {
        let code = match &diag.code {
            Some(NumberOrString::String(code)) => code.as_str(),
            _ => "",
        };
        extra.insert("diagnostic_message".into(), diag.message.clone().into());
        extra.insert("diagnostic_line".into(), diag.range.start.line.into());
        extra.insert("diagnostic_code".into(), code.into());
    }
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(diagnostics),
        edit: None,
        command: None,
        is_preferred: Some(is_preferred),
        disabled: None,
        data: Some(make_code_action_data(
            action_kind,
            uri,
            range,
            serde_json::Value::Object(extra),
        )),
    })
}

/// A `refactor.rewrite` action that replaces the bytes `[start, end)` of
/// the document `uri` holding `content` with `replacement`.
pub(crate) fn rewrite_action(
    title: String,
    uri: Url,
    content: &str,
    (start, end): (usize, usize),
    replacement: String,
    is_preferred: bool,
) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        diagnostics: None,
        edit: Some(single_edit(
            uri,
            crate::text_position::byte_range_to_lsp_range(content, start, end),
            replacement,
        )),
        command: None,
        is_preferred: Some(is_preferred),
        disabled: None,
        data: None,
    })
}

/// Offer a deferred extract action (`Extract {noun}`) for `range`, plus
/// an "all occurrences" variant when `needle` appears more than once in
/// the file.
///
/// The variants are told apart by the `all_occurrences` flag in their
/// data and by their kinds, `action_kind` and `{action_kind}All`.
pub(crate) fn push_extract_actions(
    out: &mut Vec<CodeActionOrCommand>,
    uri: &str,
    range: &Range,
    content: &str,
    needle: &str,
    noun: &str,
    action_kind: &str,
) {
    // A cheap text search, so the action list does not have to scope
    // the occurrences just to decide whether to offer the variant.
    let has_other_occurrences = content
        .find(needle)
        .is_some_and(|pos| content[pos + needle.len()..].contains(needle));

    let mut push = |title: String, kind: &str, all_occurrences: bool| {
        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::REFACTOR_EXTRACT),
            diagnostics: None,
            edit: None,
            command: None,
            is_preferred: Some(false),
            disabled: None,
            data: Some(make_code_action_data(
                kind,
                uri,
                range,
                serde_json::json!({ "all_occurrences": all_occurrences }),
            )),
        }));
    };
    if has_other_occurrences {
        push(
            format!("Extract {noun} (this occurrence)"),
            action_kind,
            false,
        );
        push(
            format!("Extract {noun} (all occurrences)"),
            &format!("{action_kind}All"),
            true,
        );
    } else {
        push(format!("Extract {noun}"), action_kind, false);
    }
}

/// Whether a deferred action from [`push_extract_actions`] asked for
/// every occurrence.
pub(crate) fn extract_wants_all_occurrences(data: &CodeActionData, all_kind: &str) -> bool {
    data.extra
        .get("all_occurrences")
        .and_then(|v| v.as_bool())
        .unwrap_or(data.action_kind == all_kind)
}

/// The byte range `[start, end)` that `range` selects in `content`, or
/// `None` for an empty selection or one that runs past the end.
pub(crate) fn selected_byte_range(content: &str, range: Range) -> Option<(usize, usize)> {
    let start = crate::text_position::position_to_byte_offset(content, range.start);
    let end = crate::text_position::position_to_byte_offset(content, range.end);
    (start < end && end <= content.len()).then_some((start, end))
}

/// Whether a code action requested over the byte range
/// `[request_start, request_end)` is about `span`: a bare cursor anywhere
/// from its first character up to its end, or a selection that intersects
/// it.
pub(crate) fn span_overlaps_request(
    span: &SymbolSpan,
    request_start: usize,
    request_end: usize,
) -> bool {
    let (start, end) = (span.start as usize, span.end as usize);
    if request_start == request_end {
        start <= request_start && request_start < end
    } else {
        start < request_end && end > request_start
    }
}

/// The selection `[start, end)` and every textually identical expression
/// within `[scope_start, scope_end)`, in source order.
///
/// Surrounding whitespace in the selection is ignored when matching.
pub(crate) fn selection_and_identical_occurrences(
    content: &str,
    start: usize,
    end: usize,
    scope_start: usize,
    scope_end: usize,
) -> Vec<(usize, usize)> {
    let selected = &content[start..end];
    let trimmed_start = start + (selected.len() - selected.trim_start().len());
    let trimmed_end = end - (selected.len() - selected.trim_end().len());
    let mut offsets = vec![(start, end)];
    offsets.extend(find_identical_occurrences(
        content,
        selected.trim(),
        trimmed_start,
        trimmed_end,
        scope_start,
        scope_end,
    ));
    offsets.sort_by_key(|&(s, _)| s);
    offsets
}

/// One edit per `(start, end)` byte span, replacing it with `replacement`.
pub(crate) fn replace_spans_edits<'a>(
    content: &'a str,
    spans: &'a [(usize, usize)],
    replacement: &'a str,
) -> impl Iterator<Item = TextEdit> + 'a {
    spans.iter().map(move |&(start, end)| TextEdit {
        range: crate::text_position::byte_range_to_lsp_range(content, start, end),
        new_text: replacement.to_string(),
    })
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
        search_from = search_from + pos + needle.len();
    }
    results
}
