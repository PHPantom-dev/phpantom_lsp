use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::composer;
use crate::diagnostics::namespace_mismatch::{
    namespace_decl_from_content, namespace_mismatch_diagnostic,
};

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
        let file_path = match Url::parse(uri).ok().and_then(|u| u.to_file_path().ok()) {
            Some(p) => p,
            None => return,
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
            let insert_pos = find_namespace_insert_position(content);
            TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: format!("namespace {};\n\n", ns),
            }
        } else {
            return;
        };

        let expected_display = expected_ns.as_deref().unwrap_or("<global>");
        let title = format!("Fix namespace to `{}`", expected_display);

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

fn find_namespace_keyword_line(content: &str) -> Option<u32> {
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("namespace ") {
            return Some(i as u32);
        }
    }
    None
}

fn find_namespace_insert_position(content: &str) -> Position {
    let mut insert_line = 0u32;

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.starts_with("<?php") {
            insert_line = (i + 1) as u32;
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("declare(") || trimmed.starts_with("declare (") {
            insert_line = (i + 1) as u32;
            continue;
        }

        break;
    }

    Position {
        line: insert_line,
        character: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::find_namespace_insert_position;
    use tower_lsp::lsp_types::Position;

    #[test]
    fn inserts_after_php_open_tag_without_declare() {
        let content = "<?php\n\nclass Example {}\n";
        assert_eq!(
            find_namespace_insert_position(content),
            Position {
                line: 1,
                character: 0,
            }
        );
    }

    #[test]
    fn inserts_after_declare_statement() {
        let content = "<?php\n\ndeclare(strict_types=1);\n\nclass Example {}\n";
        assert_eq!(
            find_namespace_insert_position(content),
            Position {
                line: 3,
                character: 0,
            }
        );
    }

    #[test]
    fn inserts_after_multiple_declare_statements() {
        let content = "<?php\n\ndeclare(strict_types=1);\ndeclare(ticks=1);\n\nclass Example {}\n";
        assert_eq!(
            find_namespace_insert_position(content),
            Position {
                line: 4,
                character: 0,
            }
        );
    }
}
