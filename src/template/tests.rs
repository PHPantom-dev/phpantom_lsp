use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::text_position::byte_range_to_lsp_range;

fn template(backend: &Backend, name: &str, source: &str) -> Uri {
    let uri = crate::util::path_to_uri(&std::env::temp_dir().join(name));
    backend
        .open_files
        .write()
        .insert(uri.clone(), Arc::new(source.to_string()));
    backend.update_ast(&uri, source);
    crate::util::parse_url(&uri).unwrap()
}

#[test]
fn insert_replace_completion_translates_both_ranges_and_rejects_generated_text() {
    for (name, source) in [
        ("completion.blade.php", "<p>📚 {{ $example }}</p>"),
        ("completion.view.php", "<p>📚 {{ $example }}</p>"),
    ] {
        let backend = Backend::new_test();
        let uri = template(&backend, name, source);
        let start = source.find("$example").unwrap();
        let insert = byte_range_to_lsp_range(source, start, start + 3);
        let replace = byte_range_to_lsp_range(source, start, start + 8);
        let virtual_insert = backend.translate_template_range_to_php(uri.as_str(), insert);
        let virtual_replace = backend.translate_template_range_to_php(uri.as_str(), replace);
        let completion = |insert, replace| CompletionItem {
            label: "$example".to_string(),
            text_edit: Some(CompletionItemTextEdit::InsertReplaceEdit(
                InsertReplaceEdit {
                    new_text: "$example".to_string(),
                    insert,
                    replace,
                },
            )),
            ..Default::default()
        };
        let response = backend.translate_completion_response(
            uri.as_str(),
            CompletionResponse::CompletionItemList(vec![
                completion(virtual_insert, virtual_replace),
                completion(Range::default(), virtual_replace),
                completion(virtual_insert, Range::default()),
            ]),
        );
        assert_eq!(
            response,
            CompletionResponse::CompletionItemList(vec![completion(insert, replace)])
        );
    }
}

#[test]
fn action_edits_read_other_templates_and_reject_unavailable_or_stale_lowerings() {
    for suffix in ["blade.php", "view.php"] {
        let backend = Backend::new_test();
        let request_uri = template(&backend, &format!("request.{suffix}"), "<p>Request</p>");
        let source = "<p>📚 {{ $example }}</p>";
        let target_uri = template(&backend, &format!("target.{suffix}"), source);
        let start = source.find("$example").unwrap();
        let range = byte_range_to_lsp_range(source, start, start + 8);
        let virtual_range = backend.translate_template_range_to_php(target_uri.as_str(), range);
        let request = backend.template_text(request_uri.as_str()).unwrap();
        let action = || WorkspaceEdit {
            changes: Some(std::collections::HashMap::from([(
                target_uri.clone(),
                vec![TextEdit {
                    range: virtual_range,
                    new_text: "$renamed".to_string(),
                }],
            )])),
            ..Default::default()
        };
        let mut edit = action();
        assert!(backend.translate_action_edit(&mut edit, Some(&request), false));
        assert_eq!(edit.changes.unwrap()[&target_uri][0].range, range);

        backend.open_files.write().insert(
            target_uri.to_string(),
            Arc::new(format!("{source}\nchanged")),
        );
        assert!(!backend.translate_action_edit(&mut action(), Some(&request), false));

        let unknown = crate::util::parse_url(&crate::util::path_to_uri(
            &std::env::temp_dir().join(format!("unopened.{suffix}")),
        ))
        .unwrap();
        let mut edit = action();
        let edits = edit.changes.as_mut().unwrap().remove(&target_uri).unwrap();
        edit.changes.as_mut().unwrap().insert(unknown, edits);
        assert!(!backend.translate_action_edit(&mut edit, Some(&request), false));
    }
}
