use crate::common::create_psr4_workspace;
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

async fn open_resource(backend: &Backend, uri: Url, language_id: &str, content: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: language_id.to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;
}

fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    let offset = content.find(needle).expect("needle should exist") + inside;
    let prefix = &content[..offset];
    Position::new(
        prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
        prefix
            .rsplit_once('\n')
            .map_or(prefix.len(), |(_, line)| line.len()) as u32,
    )
}

async fn definition_at(
    backend: &Backend,
    uri: Url,
    content: &str,
    needle: &str,
    inside: usize,
) -> Option<GotoDefinitionResponse> {
    backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: position_in(content, needle, inside),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("definition request should succeed")
}

#[tokio::test]
async fn navigates_php_classes_from_arbitrary_yaml_keys_and_values() {
    let php = "<?php\nnamespace App\\UseCase;\nclass DeletePlaylist {}\n";
    let yaml = concat!(
        "App\\UseCase\\DeletePlaylist:\n",
        "  arbitrary-name: App\\UseCase\\DeletePlaylist\n",
        "  x-usecase: App\\UseCase\\DeletePlaylist\n",
    );
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/UseCase/DeletePlaylist.php", php),
            ("schema/paths/playlists.yaml", yaml),
        ],
    );
    let yaml_uri = Url::from_file_path(dir.path().join("schema/paths/playlists.yaml")).unwrap();
    open_resource(&backend, yaml_uri.clone(), "yaml", yaml).await;

    for (needle, inside) in [
        ("App\\UseCase\\DeletePlaylist:", 18),
        ("arbitrary-name: App\\UseCase\\DeletePlaylist", 27),
        ("x-usecase: App\\UseCase\\DeletePlaylist", 22),
    ] {
        let result = definition_at(&backend, yaml_uri.clone(), yaml, needle, inside)
            .await
            .expect("class should resolve from YAML");
        let GotoDefinitionResponse::Scalar(location) = result else {
            panic!("expected one class definition");
        };
        assert!(
            location
                .uri
                .path()
                .ends_with("/src/UseCase/DeletePlaylist.php")
        );
        assert_eq!(location.range.start.line, 2);
    }
}

#[tokio::test]
async fn navigates_php_classes_from_xml_attributes_and_text() {
    let php = "<?php\nnamespace App\\Handler;\nclass Run {}\n";
    let xml = r#"<root handler="App\Handler\Run"><target>App\Handler\Run</target></root>"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[("src/Handler/Run.php", php), ("config/arbitrary.xml", xml)],
    );
    let xml_uri = Url::from_file_path(dir.path().join("config/arbitrary.xml")).unwrap();
    open_resource(&backend, xml_uri.clone(), "xml", xml).await;

    for needle in ["handler=\"App\\Handler\\Run", ">App\\Handler\\Run"] {
        let result = definition_at(&backend, xml_uri.clone(), xml, needle, needle.len() - 2)
            .await
            .expect("class should resolve from XML");
        let GotoDefinitionResponse::Scalar(location) = result else {
            panic!("expected one class definition");
        };
        assert!(location.uri.path().ends_with("/src/Handler/Run.php"));
    }
}

#[tokio::test]
async fn navigates_class_members_and_yaml_escaped_class_names() {
    let php = concat!(
        "<?php\n",
        "namespace App\\Handler;\n",
        "class Run {\n",
        "    public function handle(): void {}\n",
        "}\n",
    );
    let yaml = r#"callback: "App\\Handler\\Run::handle""#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[("src/Handler/Run.php", php), ("config/callbacks.yml", yaml)],
    );
    let yaml_uri = Url::from_file_path(dir.path().join("config/callbacks.yml")).unwrap();
    open_resource(&backend, yaml_uri.clone(), "yaml", yaml).await;

    let class_result = definition_at(&backend, yaml_uri.clone(), yaml, "App", 1)
        .await
        .expect("escaped class should resolve");
    let GotoDefinitionResponse::Scalar(class_location) = class_result else {
        panic!("expected one class definition");
    };
    assert_eq!(class_location.range.start.line, 2);

    let member_result = definition_at(&backend, yaml_uri, yaml, "handle", 2)
        .await
        .expect("class member should resolve");
    let GotoDefinitionResponse::Scalar(member_location) = member_result else {
        panic!("expected one member definition");
    };
    assert_eq!(member_location.range.start.line, 3);
}

#[tokio::test]
async fn resource_classes_feed_find_references_and_code_lens() {
    let php = concat!("<?php\n", "namespace App\\Domain;\n", "class Widget {}\n",);
    let yaml = concat!(
        "primary: App\\Domain\\Widget\n",
        "fallback: App\\Domain\\Widget\n",
    );
    let xml = r#"<item class="App\Domain\Widget" />"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Domain/Widget.php", php),
            ("config/widgets.yaml", yaml),
            ("config/widgets.xml", xml),
        ],
    );
    let php_uri = Url::from_file_path(dir.path().join("src/Domain/Widget.php")).unwrap();
    open_resource(&backend, php_uri.clone(), "php", php).await;

    let references = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: php_uri.clone(),
                },
                position: Position::new(2, 8),
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("reference request should succeed")
        .expect("resource references should be found");
    assert_eq!(references.len(), 3);
    assert_eq!(
        references
            .iter()
            .filter(|location| location.uri.path().ends_with("/config/widgets.yaml"))
            .count(),
        2
    );
    assert_eq!(
        references
            .iter()
            .filter(|location| location.uri.path().ends_with("/config/widgets.xml"))
            .count(),
        1
    );

    let lenses = backend
        .code_lens(CodeLensParams {
            text_document: TextDocumentIdentifier { uri: php_uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code lens request should succeed")
        .expect("class reference lens should be present");
    let lens = lenses
        .into_iter()
        .find(|lens| lens.range.start.line == 2 && lens.data.is_some())
        .expect("class declaration should have an unresolved reference lens");
    let resolved = backend
        .code_lens_resolve(lens)
        .await
        .expect("class reference lens should resolve");
    assert_eq!(
        resolved
            .command
            .as_ref()
            .map(|command| command.title.as_str()),
        Some("3 references")
    );
}

#[tokio::test]
async fn resource_class_members_feed_find_references_and_code_lens() {
    let php = concat!(
        "<?php\n",
        "namespace App\\Handler;\n",
        "class Run {\n",
        "    public function handle(): void {}\n",
        "}\n",
    );
    let yaml = "callback: App\\Handler\\Run::handle\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Handler/Run.php", php),
            ("config/callbacks.yaml", yaml),
        ],
    );
    let php_uri = Url::from_file_path(dir.path().join("src/Handler/Run.php")).unwrap();
    open_resource(&backend, php_uri.clone(), "php", php).await;

    let references = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: php_uri.clone(),
                },
                position: Position::new(3, 22),
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("reference request should succeed")
        .expect("resource member reference should be found");
    assert_eq!(references.len(), 1);
    assert!(references[0].uri.path().ends_with("/config/callbacks.yaml"));

    let lenses = backend
        .code_lens(CodeLensParams {
            text_document: TextDocumentIdentifier { uri: php_uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code lens request should succeed")
        .expect("member reference lens should be present");
    let lens = lenses
        .into_iter()
        .find(|lens| {
            lens.range.start.line == 3
                && lens.data.as_ref().is_some_and(|data| {
                    data.get("kind").and_then(serde_json::Value::as_str)
                        == Some("phpMemberReferences")
                })
        })
        .expect("method declaration should have an unresolved reference lens");
    let resolved = backend
        .code_lens_resolve(lens)
        .await
        .expect("member reference lens should resolve");
    assert_eq!(
        resolved
            .command
            .as_ref()
            .map(|command| command.title.as_str()),
        Some("1 reference")
    );
}

#[tokio::test]
async fn unknown_and_unqualified_names_do_not_navigate() {
    let yaml = "short: DeletePlaylist\nunknown: App\\Missing\\DeletePlaylist\n";
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("config/classes.yaml", yaml)]);
    let yaml_uri = Url::from_file_path(dir.path().join("config/classes.yaml")).unwrap();
    open_resource(&backend, yaml_uri.clone(), "yaml", yaml).await;

    assert!(
        definition_at(&backend, yaml_uri.clone(), yaml, "DeletePlaylist", 2)
            .await
            .is_none()
    );
    assert!(
        definition_at(&backend, yaml_uri, yaml, "App\\Missing", 4)
            .await
            .is_none()
    );
}

#[tokio::test]
async fn navigates_class_constants_from_yaml() {
    let php = concat!(
        "<?php\n",
        "namespace App\\Handler;\n",
        "class Run {\n",
        "    public const MODE = 'sync';\n",
        "}\n",
    );
    let yaml = "mode: App\\Handler\\Run::MODE\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[("src/Handler/Run.php", php), ("config/modes.yaml", yaml)],
    );
    let yaml_uri = Url::from_file_path(dir.path().join("config/modes.yaml")).unwrap();
    open_resource(&backend, yaml_uri.clone(), "yaml", yaml).await;

    let result = definition_at(&backend, yaml_uri, yaml, "MODE", 2)
        .await
        .expect("class constant should resolve from YAML");
    let GotoDefinitionResponse::Scalar(location) = result else {
        panic!("expected one constant definition");
    };
    assert_eq!(location.range.start.line, 3);
}

/// A resource occurrence is a reference the user can be shown, but never an
/// edit target: the escaped `"App\\Widget"` spelling YAML quoting requires
/// is not a PHP name token, and rename drops every edit as soon as one
/// location fails verification.  Leaving them in turned a single escaped
/// name anywhere in the workspace into a silent no-op rename.
#[tokio::test]
async fn rename_ignores_resource_occurrences() {
    let backend = Backend::new_test();
    let php_uri = Url::parse("file:///test.php").unwrap();
    let php = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Widget {}\n",
        "function demo(Widget $w): void {}\n",
    );
    open_resource(&backend, php_uri.clone(), "php", php).await;

    let yaml_uri = Url::parse("file:///config/widgets.yaml").unwrap();
    let yaml = "plain: App\\Widget\nescaped: \"App\\\\Widget\"\n";
    open_resource(&backend, yaml_uri.clone(), "yaml", yaml).await;

    // Both spellings are found by Find References.
    let references = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: php_uri.clone(),
                },
                position: Position::new(2, 8),
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("reference request should succeed")
        .expect("resource references should be found");
    assert_eq!(
        references
            .iter()
            .filter(|location| location.uri == yaml_uri)
            .count(),
        2
    );

    // The rename still happens, and touches only the PHP file.
    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: php_uri.clone(),
                },
                position: Position::new(3, 16),
            },
            new_name: "Gadget".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("rename should not be refused")
        .expect("rename should produce an edit");
    let changes = edit.changes.expect("rename should carry changes");
    assert_eq!(changes.keys().collect::<Vec<_>>(), vec![&php_uri]);
    assert_eq!(changes[&php_uri].len(), 2);
}

#[tokio::test]
async fn rename_is_refused_from_inside_a_resource_file() {
    let backend = Backend::new_test();
    let php_uri = Url::parse("file:///test.php").unwrap();
    let php = "<?php\nnamespace App;\nclass Widget {}\n";
    open_resource(&backend, php_uri, "php", php).await;

    let yaml_uri = Url::parse("file:///config/widgets.yaml").unwrap();
    let yaml = "primary: App\\Widget\n";
    open_resource(&backend, yaml_uri.clone(), "yaml", yaml).await;

    assert!(
        backend
            .prepare_rename(TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: yaml_uri.clone()
                },
                position: Position::new(0, 13),
            })
            .await
            .expect("prepare rename should succeed")
            .is_none()
    );
    assert!(
        backend
            .rename(RenameParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: yaml_uri },
                    position: Position::new(0, 13),
                },
                new_name: "Gadget".to_string(),
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("rename should not be refused")
            .is_none()
    );
}
