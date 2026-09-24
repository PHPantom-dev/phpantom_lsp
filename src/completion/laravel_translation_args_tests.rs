use super::*;
use crate::completion::source::code_context::code_context_at;
use crate::test_fixtures::make_backend;

fn complete(source: &str) -> Option<Vec<CompletionItem>> {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("lang/en")).unwrap();
    std::fs::create_dir_all(dir.path().join("resources/lang/fr")).unwrap();
    std::fs::write(
        dir.path().join("lang/en/messages.php"),
        "<?php return ['hello' => 'Hello :name :NAME :Name :count', 'dynamic' => env('KEY')];",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("resources/lang/fr.json"),
        r#"{"Welcome":"Bonjour :name et :ami"}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("lang/de.json"), "{}").unwrap();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    let cursor = source.find('|').unwrap();
    let content = source.replacen('|', "", 1);
    let code = code_context_at(&content, cursor)?;
    let ctx = FileContext {
        use_map: backend.parse_use_statements(&content),
        classes: Vec::new(),
        namespace: None,
        namespace_spans: None,
        resolved_names: None,
    };
    match backend.try_translation_argument_completion(
        &content,
        offset_to_position(&content, cursor),
        &code,
        &ctx,
    )? {
        CompletionResponse::Array(items) => Some(items),
        _ => panic!("expected array"),
    }
}

fn labels(source: &str) -> Vec<String> {
    complete(source)
        .unwrap_or_else(|| panic!("no completion: {source}"))
        .into_iter()
        .map(|item| item.label)
        .collect()
}

#[test]
fn translation_argument_locales_positional_named_and_incomplete() {
    for call in [
        "__('messages.hello', [], '|')",
        "trans('messages.hello', [], '|')",
        "trans_choice('messages.hello', 2, [], '|')",
        "Lang::get('messages.hello', [], '|')",
        "Lang::choice('messages.hello', 2, [], '|')",
        "Lang::hasForLocale('messages.hello', '|')",
        "Lang::has('messages.hello', '|')",
        "__(locale: '|', key: 'messages.hello')",
        "trans_choice(locale: '|', number: 2, key: 'messages.hello')",
        "Lang::hasForLocale(locale: '|', key: 'messages.hello')",
        "__('messages.hello', [], '|",
        "__(locale: '|",
        "\\__('x', locale: '|')",
        "\\Illuminate\\Support\\Facades\\Lang::get('x', locale: '|')",
    ] {
        assert_eq!(
            labels(&format!("<?php {call}")),
            ["de", "en", "fr"],
            "{call}"
        );
    }
    assert_eq!(
        labels(
            "<?php use Illuminate\\Support\\Facades\\Lang as L; L::get(locale: 'f|', key: 'x');"
        ),
        ["fr"]
    );
    assert_eq!(
        labels("<?php __('x', locale: 'unknown|');"),
        Vec::<String>::new()
    );
}

#[test]
fn translation_argument_placeholders_follow_bound_replacement_array() {
    for call in [
        "__('messages.hello', ['|'])",
        "trans('messages.hello', ['|'])",
        "trans_choice('messages.hello', 2, ['|'])",
        "Lang::get('messages.hello', ['|'])",
        "Lang::choice('messages.hello', 2, ['|'])",
        "__(replace: ['|'], key: 'messages.hello')",
        "__('messages.hello', [ /* don't ( */ '|'])",
        "__('messages.hello', ['|",
    ] {
        assert_eq!(
            labels(&format!("<?php {call}")),
            ["count", "name"],
            "{call}"
        );
    }
    assert_eq!(
        labels("<?php __('messages.hello', ['name' => ['x'], '|' => 1]);"),
        ["count"]
    );
    assert_eq!(
        labels("<?php __('messages.hello', ['|' => 1, 'count' => 2]);"),
        ["name"]
    );
    assert_eq!(labels("<?php __('Welcome', ['|']);"), ["ami", "name"]);
    assert!(labels("<?php __('messages.dynamic', ['|']);").is_empty());
    let items = complete("<?php __('messages.hello', ['n|me' => 1]);").unwrap();
    assert_eq!(items.len(), 1);
    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("edit")
    };
    assert_eq!(edit.new_text, "name");
    assert_eq!(edit.range.end.character - edit.range.start.character, 3);
}

#[test]
fn translation_argument_completion_ignores_other_calls_and_array_values() {
    for source in [
        "<?php $x = '|';",
        "<?php ['|'];",
        "<?php __('|');",
        "<?php foo(locale: '|');",
        "<?php Foo::get(locale: '|');",
        "<?php $lang->get(locale: '|');",
        "<?php App\\trans('x', [], '|');",
        "<?php use App\\Lang; Lang::get(locale: '|');",
        "<?php __('messages.hello', ['name' => '|']);",
        "<?php __('messages.hello', ['nested' => ['|']]);",
        "<?php __('messages.hello', wrong: '|');",
        "<?php __('missing', ['|']);",
        "<?php __($dynamic, ['|']);",
        "<?php Lang::has('x', ['|']);",
        "<?php __('messages.hello', [], ['|']);",
        "<?php __('messages.hello', replace: ['name'=>'x' . '|']);",
    ] {
        assert!(complete(source).is_none(), "{source}");
    }
}

#[test]
fn translation_argument_completion_handles_nested_calls_and_unknown_replacements() {
    assert_eq!(
        labels("<?php __('messages.hello', ['|' => foo()]);"),
        ["count", "name"]
    );
    assert_eq!(
        labels("<?php __('messages.hello', $replacements, locale: '|');"),
        ["de", "en", "fr"]
    );
    assert_eq!(
        labels("<?php __('messages.hello', [...$replacements, '|']);"),
        ["count", "name"]
    );
}
