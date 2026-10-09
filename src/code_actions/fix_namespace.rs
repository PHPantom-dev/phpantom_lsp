use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::composer;
use crate::diagnostics::namespace_mismatch::{
    namespace_decl_from_content, namespace_mismatch_diagnostic,
};
use crate::text_position::offset_to_position;
use crate::text_scan::HeaderEnd;

use super::single_file_edit;

impl Backend {
    pub(crate) fn collect_fix_namespace_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let workspace_root = match self.workspace_root().read().clone() {
            Some(r) => r,
            None => return,
        };
        let Ok(url) = Url::parse(uri) else {
            return;
        };
        let Ok(file_path) = url.to_file_path() else {
            return;
        };
        let mappings = self.psr4_mappings().read().clone();
        if mappings.is_empty() {
            return;
        }
        let Some(diag) = namespace_mismatch_diagnostic(self, uri, content) else {
            return;
        };
        let (expected_ns, _) =
            match composer::resolve_namespace_from_path(&mappings, &workspace_root, &file_path) {
                Some(r) => r,
                None => return,
            };
        let (actual_ns, edit_range) = match namespace_decl_from_content(content) {
            Some(v) => v,
            None => return,
        };
        let ns_line = edit_range.start.line;

        let cursor_line = params.range.start.line;
        let ns_decl_line = find_namespace_keyword_line(content);
        let target_line = ns_decl_line.unwrap_or(ns_line);

        if cursor_line != target_line && cursor_line != ns_line {
            return;
        }

        let edit = if actual_ns.is_some() {
            TextEdit {
                range: edit_range,
                new_text: expected_ns.clone().unwrap_or_default(),
            }
        } else if let Some(ref ns) = expected_ns {
            insert_namespace_edit(content, ns)
        } else {
            return;
        };

        let expected_display = expected_ns.as_deref().unwrap_or("<global>");
        let title = format!("Fix namespace to `{}`", expected_display);

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diag]),
            edit: Some(single_file_edit(url, vec![edit])),
            is_preferred: Some(true),
            ..Default::default()
        }));
    }
}

fn find_namespace_keyword_line(content: &str) -> Option<u32> {
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("namespace ") {
            return Some(i as u32);
        }
    }
    None
}

/// The edit that writes a `namespace` statement into a file that has
/// none, just past its header.
fn insert_namespace_edit(content: &str, ns: &str) -> TextEdit {
    let (at, new_text) = match crate::text_scan::header_end(content) {
        HeaderEnd::Line(line) => (
            Position { line, character: 0 },
            format!("namespace {ns};\n\n"),
        ),
        // Code follows the header on its line, so the line below is past
        // it: the statement goes between the two, on that line.
        HeaderEnd::Inline(end) => (
            offset_to_position(content, end),
            format!(" namespace {ns};"),
        ),
    };
    TextEdit {
        range: Range { start: at, end: at },
        new_text,
    }
}

#[cfg(test)]
mod tests {
    use super::insert_namespace_edit;
    use tower_lsp::lsp_types::Position;

    fn insertion(content: &str) -> (Position, String) {
        let edit = insert_namespace_edit(content, "App");
        assert_eq!(edit.range.start, edit.range.end);
        (edit.range.start, edit.new_text)
    }

    fn own_line(line: u32) -> (Position, String) {
        (
            Position { line, character: 0 },
            "namespace App;\n\n".to_string(),
        )
    }

    fn inline_at(line: u32, character: u32) -> (Position, String) {
        (Position { line, character }, " namespace App;".to_string())
    }

    #[test]
    fn inserts_after_php_open_tag_without_declare() {
        assert_eq!(insertion("<?php\n\nclass Example {}\n"), own_line(1));
    }

    #[test]
    fn inserts_after_declare_statement() {
        let content = "<?php\n\ndeclare(strict_types=1);\n\nclass Example {}\n";
        assert_eq!(insertion(content), own_line(3));
    }

    #[test]
    fn inserts_after_multiple_declare_statements() {
        let content = "<?php\n\ndeclare(strict_types=1);\ndeclare(ticks=1);\n\nclass Example {}\n";
        assert_eq!(insertion(content), own_line(4));
    }

    #[test]
    fn inserts_after_a_declare_a_comment_precedes() {
        let content = "<?php\n// License header\ndeclare(strict_types=1);\n\nclass Example {}\n";
        assert_eq!(insertion(content), own_line(3));
    }

    #[test]
    fn inserts_before_the_docblock_of_the_first_class() {
        let content = "<?php\n\n/** An example. */\nclass Example {}\n";
        assert_eq!(insertion(content), own_line(1));
    }

    #[test]
    fn inserts_inline_when_code_shares_the_open_tag_line() {
        assert_eq!(insertion("<?php class Example {}\n"), inline_at(0, 5));
    }

    #[test]
    fn inserts_inline_after_a_declare_code_shares_a_line_with() {
        let content = "<?php\ndeclare(strict_types=1); class Example {}\n";
        assert_eq!(insertion(content), inline_at(1, 24));
        let content = "<?php declare(strict_types=1); class Example {}\n";
        assert_eq!(insertion(content), inline_at(0, 30));
    }

    #[test]
    fn a_declare_block_is_not_header() {
        let content = "<?php declare(ticks=1) { tick(); }\n";
        assert_eq!(insertion(content), inline_at(0, 5));
    }
}
