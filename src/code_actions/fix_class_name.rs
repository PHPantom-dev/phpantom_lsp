use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::diagnostics::class_name_mismatch::class_name_mismatch_diagnostic;
use crate::util::offset_to_position;

impl Backend {
    pub(crate) fn collect_fix_class_name_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let file_path = match Url::parse(uri).ok().and_then(|u| u.to_file_path().ok()) {
            Some(p) => p,
            None => return,
        };
        let file_stem = match file_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => return,
        };

        let Some(diag) = class_name_mismatch_diagnostic(self, uri, content) else {
            return;
        };

        let classes = self.parse_php(content);
        if classes.len() != 1 {
            return;
        }
        let class = &classes[0];
        let range = diag.range;

        let cursor_line = params.range.start.line;
        let decl_line = offset_to_position(content, class.keyword_offset as usize).line;
        if cursor_line != range.start.line && cursor_line != decl_line {
            return;
        }

        let edit = TextEdit {
            range,
            new_text: file_stem.clone(),
        };

        let title = format!("Fix class name to `{}`", file_stem);

        let mut changes = std::collections::HashMap::new();
        changes.insert(Url::parse(uri).unwrap(), vec![edit]);

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diag]),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            is_preferred: Some(true),
            ..Default::default()
        }));
    }
}
