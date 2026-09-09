use super::*;
use crate::test_fixtures::make_backend;
use tower_lsp::lsp_types::Url;

#[test]
fn translation_hover_lists_locales_values_and_links_from_both_roots() {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("lang/en")).unwrap();
    std::fs::create_dir_all(dir.path().join("resources/lang/fr")).unwrap();
    std::fs::write(
        dir.path().join("lang/en/messages.php"),
        "<?php return [\n'hello'=>'Hello :name', 'group'=>['child'=>'text'], 'empty'=>''];",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("resources/lang/fr/messages.php"),
        "<?php return [\n\n'hello'=>'Bonjour :name'];",
    )
    .unwrap();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    let detail = backend.translation_hover_detail("messages.hello");
    assert!(detail.contains("`en`: `Hello :name`"), "{detail}");
    assert!(detail.contains("`fr`: `Bonjour :name`"));
    assert!(detail.contains("[\u{60}lang/en/messages.php\u{60}]"));
    assert!(detail.contains("[\u{60}resources/lang/fr/messages.php\u{60}]"));
    assert!(detail.contains("messages.php#L2>"));
    assert!(detail.contains("messages.php#L3>"));
    assert!(detail.find("`en`").unwrap() < detail.find("`fr`").unwrap());
    let group = backend.translation_hover_detail("messages");
    assert!(group.contains("`en`"));
    assert!(group.contains("`fr`"));
    assert!(
        backend
            .translation_hover_detail("messages.group")
            .contains("Defined in")
    );
    assert_eq!(
        backend.translation_hover_detail("missing"),
        "Translation key"
    );
    assert!(
        backend
            .translation_hover_detail("messages.empty")
            .contains("`en`:")
    );
}

#[test]
fn translation_hover_links_custom_paths_and_escapes_markdown_values() {
    let uri = Url::parse("file:///vendor/package/translations/en.json").unwrap();
    let detail = locale_detail("en", &uri, 5, Some("Hello `name`"));
    assert!(detail.contains("`` Hello `name` ``"));
    assert!(detail.contains("[\u{60}/vendor/package/translations/en.json\u{60}](<file:///vendor/package/translations/en.json#L6>)"));
}
