use super::*;
use crate::test_fixtures::make_backend;
use tower_lsp::lsp_types::Range;

#[test]
fn json_translation_declarations_preserve_source_ranges_and_values() {
    let content = r#"{
  "Greeting 😀": "Bonjour :name",
  "Quoted \"key\"": "Ligne\nSuivante",
  "Unicode \u00e9": "Café",
  "nested": {"ignored": true},
  "empty": null
}"#;
    let declarations = collect_json_trans_declarations(content);
    assert_eq!(declarations.len(), 5);
    for (declaration, expected_key, expected_source, value) in [
        (
            &declarations[0],
            "Greeting 😀",
            "Greeting 😀",
            Some("Bonjour :name"),
        ),
        (
            &declarations[1],
            "Quoted \"key\"",
            r#"Quoted \"key\""#,
            Some("Ligne\nSuivante"),
        ),
        (
            &declarations[2],
            "Unicode é",
            r"Unicode \u00e9",
            Some("Café"),
        ),
        (&declarations[3], "nested", "nested", None),
        (&declarations[4], "empty", "empty", None),
    ] {
        assert_eq!(declaration.key, expected_key);
        assert_eq!(
            &content[declaration.start..declaration.end],
            expected_source
        );
        assert_eq!(declaration.value.as_deref(), value);
        assert!(!declaration.is_group);
    }
}

#[test]
fn json_translation_declarations_reject_invalid_documents() {
    for content in [
        "",
        "[]",
        "null",
        "{",
        "{1: 2}",
        "{\"a\"}",
        "{\"a\":}",
        "{\"a\":\"bad\\escape\"}",
        "{\"a\":1 \"b\":2}",
        "{\"a\":1,}",
        "{\"a\":1} trailing",
        "{} trailing",
        "\u{a0}{}",
        "{\u{a0}\"key\":1}",
        "{\"key\":1}\u{a0}",
        "{}\u{a0}",
        "{\"a\":1",
    ] {
        assert!(
            collect_json_trans_declarations(content).is_empty(),
            "{content}"
        );
    }
    assert!(collect_json_trans_declarations(" \n{ } \r\n").is_empty());
    assert_eq!(
        collect_json_trans_declarations("{\"a\":1,\"a\":2}").len(),
        2
    );
}

#[test]
fn json_translation_references_ignore_values_and_unrelated_files() {
    let backend = make_backend();
    for uri in [
        "file:///project/lang/en.php",
        "file:///project/config/en.json",
        "invalid.json",
        "https://example.test/lang/en.json",
    ] {
        assert!(
            find_json_trans_references(&backend, uri, "{}", Position::new(0, 0), true).is_none()
        );
    }
    assert!(
        find_json_trans_references(
            &backend,
            "file:///project/lang/en.json",
            r#"{"key":"value"}"#,
            Position::new(0, 10),
            true,
        )
        .is_none()
    );
}

#[test]
fn json_translation_provider_paths_and_duplicate_keys_resolve() {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    let content = "{\"key\":\"first\",\n \"key\":\"last\"}";
    let path = dir.path().join("fr.json");
    std::fs::write(&path, content).unwrap();
    std::fs::write(dir.path().join("ignore.txt"), "{}").unwrap();
    std::fs::write(dir.path().join("invalid.json"), "{").unwrap();
    let resource = super::super::provider_resources::ProviderResource {
        path: dir.path().to_path_buf(),
        namespace: String::new(),
    };
    backend.laravel_provider_resources.write().trans_dirs = vec![resource.clone(), resource];
    let uri = Url::from_file_path(path).unwrap();
    let locations =
        find_json_trans_references(&backend, uri.as_str(), content, Position::new(0, 2), true)
            .unwrap();
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].uri, uri);
    assert_eq!(
        locations[0].range,
        Range::new(Position::new(1, 2), Position::new(1, 5))
    );
    assert!(backend.translation_definitions("missing").is_empty());
}
