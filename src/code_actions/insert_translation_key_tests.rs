use super::*;
use crate::text_position::position_to_offset;

fn apply(content: &str, path: &str) -> String {
    let mut edits = insertion_edits(content, path).unwrap_or_else(|| panic!("no edits: {content}"));
    edits.sort_by_key(|edit| edit.range.start);
    let mut result = content.to_string();
    for edit in edits.into_iter().rev() {
        let start = position_to_offset(content, edit.range.start) as usize;
        let end = position_to_offset(content, edit.range.end) as usize;
        result.replace_range(start..end, &edit.new_text);
    }
    crate::parser::with_parsed_program(&result, "verify_translation_edit", |program, _| {
        assert!(program.errors.is_empty(), "{result}: {:?}", program.errors);
    });
    assert!(
        insertion_edits(&result, path).is_none(),
        "must not insert a duplicate"
    );
    result
}

#[test]
fn translation_insertion_preserves_siblings_comments_and_layout() {
    for (source, expected) in [
        ("<?php return [];", "<?php return ['new' => ''];"),
        (
            "<?php return ['old' => 'yes'];",
            "<?php return ['old' => 'yes', 'new' => ''];",
        ),
        (
            "<?php return ['old' => 'yes',];",
            "<?php return ['old' => 'yes', 'new' => '',];",
        ),
        (
            "<?php return [\n    'old' => 'yes' // keep me\n];",
            "<?php return [\n    'old' => 'yes', // keep me\n    'new' => '',\n];",
        ),
        (
            "<?php return [\r\n\t'old' => 'yes',\r\n];",
            "<?php return [\r\n\t'old' => 'yes',\r\n\t'new' => '',\r\n];",
        ),
        ("<?php return [\n];", "<?php return [\n    'new' => '',\n];"),
        ("<?php return array();", "<?php return array('new' => '');"),
        ("<?php return ([]);", "<?php return (['new' => '']);"),
    ] {
        assert_eq!(apply(source, "new"), expected);
    }
    let multiline = apply("<?php return [\n 'old' => 'yes'];", "new");
    assert!(multiline.contains("'old' => 'yes',\n 'new' => '',\n]"));
}

#[test]
fn translation_insertion_adds_nested_keys_without_overwriting_existing_values() {
    let result = apply(
        "<?php return ['checkout' => ['existing' => 'yes']];",
        "checkout.address.label",
    );
    assert_eq!(
        result,
        "<?php return ['checkout' => ['existing' => 'yes', 'address' => ['label' => '']]];"
    );
    assert_eq!(
        apply("<?php return [];", "it's.back\\slash"),
        "<?php return ['it\\'s' => ['back\\\\slash' => '']];"
    );
    for source in [
        "<?php return ['checkout' => 'scalar'];",
        "<?php return ['checkout' => []];",
        "<?php return ['checkout' => 'first', 'checkout' => 'last'];",
        "<?php return array_merge([], []);",
        "<?php return $values;",
        "<?php $x = [];",
        "<?php return [;",
        "<?php return ['value'];",
        "<?php return [...$values];",
        "<?php return [$dynamic => 'value'];",
        "<?php return [1 => 'value'];",
    ] {
        assert!(insertion_edits(source, "checkout").is_none(), "{source}");
    }
    assert!(insertion_edits("<?php return ['checkout' => 'scalar'];", "checkout.title").is_none());
}

#[test]
fn translation_insertion_ignores_stale_documents_ranges_and_deleted_files() {
    let backend = crate::test_fixtures::make_backend();
    let dir = tempfile::tempdir().unwrap();
    let uri = Url::from_file_path(dir.path().join("usage.php")).unwrap();
    let source = "<?php __('messages.first'); __('messages.second');";
    let range = Range::new(Position::new(0, 9), Position::new(0, 23));
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range,
        context: CodeActionContext {
            diagnostics: vec![Diagnostic {
                range,
                code: Some(NumberOrString::String("invalid_laravel_trans".to_string())),
                ..Default::default()
            }],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let mut out = Vec::new();
    backend.collect_insert_translation_key_actions(uri.as_str(), source, &params, &mut out);
    assert!(out.is_empty());
    backend.update_ast(uri.as_str(), source);
    std::fs::create_dir_all(dir.path().join("lang/en")).unwrap();
    let path = dir.path().join("lang/en/messages.php");
    std::fs::write(&path, "<?php return ['existing'=>'text'];").unwrap();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    assert!(!backend.cached_translations().files.is_empty());
    std::fs::remove_file(path).unwrap();
    backend.collect_insert_translation_key_actions(uri.as_str(), source, &params, &mut out);
    assert!(out.is_empty());
}
