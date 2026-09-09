use crate::common::{create_psr4_workspace, lsp_pos_to_offset, open_document, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {"laravel/framework": "^13.0"},
    "autoload": {"psr-4": {"App\\": "src/"}}
}"#;

fn position(content: &str, needle: &str) -> Position {
    let offset = content.find(needle).unwrap();
    let before = &content[..offset];
    Position::new(
        before.bytes().filter(|b| *b == b'\n').count() as u32,
        before.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
    )
}

async fn references(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    at: Position,
    include_declaration: bool,
) -> Vec<Location> {
    backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: at,
            },
            context: ReferenceContext {
                include_declaration,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default()
}

#[tokio::test]
async fn json_translation_definitions_and_references_use_exact_key_ranges() {
    let php = "<?php\n__('Greeting 😀');\ntrans('Greeting 😀');\n";
    let english = "{\n  \"Other\": \"Greeting 😀\",\n  \"Greeting 😀\": \"Hello\"\n}\n";
    let french = "{\n\n  \"Greeting 😀\": \"Bonjour\"\n}\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/usage.php", php),
            ("lang/en.json", english),
            ("resources/lang/fr.json", french),
        ],
    );
    let php_uri = Url::from_file_path(dir.path().join("src/usage.php")).unwrap();
    let en_uri = Url::from_file_path(dir.path().join("lang/en.json")).unwrap();
    let fr_uri = Url::from_file_path(dir.path().join("resources/lang/fr.json")).unwrap();
    open_php(&backend, &php_uri, php).await;
    let definitions = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: php_uri.clone(),
                },
                position: position(php, "Greeting"),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .expect("JSON definitions");
    let GotoDefinitionResponse::Array(definitions) = definitions else {
        panic!("expected locales")
    };
    assert_eq!(definitions.len(), 2);
    for definition in &definitions {
        assert!(definition.uri == en_uri || definition.uri == fr_uri);
        assert_eq!(definition.range.start, Position::new(2, 3));
        assert_eq!(definition.range.end, Position::new(2, 14));
    }
    for include_declaration in [false, true] {
        let from_php = references(
            &backend,
            &php_uri,
            position(php, "Greeting"),
            include_declaration,
        )
        .await;
        let from_json = references(
            &backend,
            &fr_uri,
            position(french, "Greeting"),
            include_declaration,
        )
        .await;
        assert_eq!(from_php.len(), if include_declaration { 4 } else { 2 });
        assert_eq!(from_json.len(), from_php.len());
        assert!(from_json.iter().all(|location| from_php.contains(location)));
    }
    assert!(
        references(&backend, &en_uri, position(english, "Other"), false)
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn json_translation_navigation_reads_unsaved_escaped_keys() {
    let php = r#"<?php __('Say "hello"');"#;
    let initial = "{\"old\":\"value\"}";
    let updated = r#"{
  "Say \"hello\"": "Bonjour"
}"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[("src/usage.php", php), ("lang/en.json", initial)],
    );
    let php_uri = Url::from_file_path(dir.path().join("src/usage.php")).unwrap();
    let json_uri = Url::from_file_path(dir.path().join("lang/en.json")).unwrap();
    open_php(&backend, &php_uri, php).await;
    open_document(&backend, &json_uri, "json", updated).await;
    let found = references(&backend, &json_uri, position(updated, "Say"), true).await;
    assert_eq!(found.len(), 2, "{found:?}");
    let declaration = found
        .iter()
        .find(|location| location.uri == json_uri)
        .unwrap();
    assert_eq!(
        &updated[lsp_pos_to_offset(updated, declaration.range.start)
            ..lsp_pos_to_offset(updated, declaration.range.end)],
        r#"Say \"hello\""#
    );
}

#[tokio::test]
async fn translation_argument_completion_uses_both_language_folders() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            (
                "lang/en/messages.php",
                "<?php return ['hello'=>'Hello :name :count'];",
            ),
            ("resources/lang/fr.json", r#"{"Welcome":"Bonjour :ami"}"#),
            ("src/usage.php", "<?php"),
        ],
    );
    let uri = Url::from_file_path(dir.path().join("src/usage.php")).unwrap();
    for (source, expected) in [
        ("<?php __('messages.hello', [], '|');", vec!["en", "fr"]),
        (
            "<?php use Illuminate\\Support\\Facades\\Lang as L; L::get(locale: '|', key: 'messages.hello');",
            vec!["en", "fr"],
        ),
        (
            "<?php trans_choice('messages.hello', 2, replace: ['name'=>'Ada', '|' => 1]);",
            vec!["count"],
        ),
        ("<?php __(replace: ['|'], key: 'Welcome');", vec!["ami"]),
    ] {
        let at = position(source, "|");
        let php = source.replace('|', "");
        open_php(&backend, &uri, &php).await;
        let response = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: at,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            })
            .await
            .unwrap()
            .unwrap();
        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            expected,
            "{source}"
        );
    }
}
