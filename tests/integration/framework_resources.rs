use crate::common::create_psr4_workspace;
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

async fn open_doc(backend: &Backend, uri: Url, language_id: &str, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: language_id.to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Url {
    Url::from_file_path(dir.path().join(rel)).unwrap()
}

fn edit_texts_for_uri(edit: &WorkspaceEdit, uri: &Url) -> Vec<String> {
    edit.changes
        .as_ref()
        .and_then(|changes| changes.get(uri))
        .map(|edits| edits.iter().map(|edit| edit.new_text.clone()).collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn symfony_yaml_service_class_goes_to_php_definition() {
    let service_php = "<?php\nnamespace App\\Service;\nclass Mailer {}\n";
    let services_yaml =
        "services:\n  App\\Service\\Mailer:\n    arguments: ['@App\\Service\\Mailer']\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Service/Mailer.php", service_php),
            ("config/services.yaml", services_yaml),
        ],
    );

    let service_uri = uri_for(&dir, "src/Service/Mailer.php");
    let yaml_uri = uri_for(&dir, "config/services.yaml");
    open_doc(&backend, service_uri.clone(), "php", service_php).await;
    open_doc(&backend, yaml_uri.clone(), "yaml", services_yaml).await;

    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: yaml_uri },
                position: Position::new(1, 15),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("YAML service class should resolve to PHP class");

    let GotoDefinitionResponse::Scalar(location) = result else {
        panic!("expected a single definition location");
    };
    assert!(
        location.uri.path().ends_with("/src/Service/Mailer.php"),
        "expected Mailer.php, got {}",
        location.uri
    );
}

#[tokio::test]
async fn class_references_include_symfony_and_doctrine_yaml_xml() {
    let user_php = "<?php\nnamespace App\\Entity;\nclass User {}\n";
    let repo_php = "<?php\nnamespace App\\Repository;\nclass UserRepository {}\n";
    let services_yaml = "services:\n  app.user_service:\n    class: App\\Entity\\User\n";
    let doctrine_yaml =
        "App\\Entity\\User:\n  type: entity\n  repositoryClass: App\\Repository\\UserRepository\n";
    let doctrine_xml = r#"<doctrine-mapping>
  <entity name="App\Entity\User" repository-class="App\Repository\UserRepository" />
</doctrine-mapping>
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Entity/User.php", user_php),
            ("src/Repository/UserRepository.php", repo_php),
            ("config/services.yaml", services_yaml),
            ("config/doctrine/User.orm.yaml", doctrine_yaml),
            ("config/doctrine/User.orm.xml", doctrine_xml),
        ],
    );

    let user_uri = uri_for(&dir, "src/Entity/User.php");
    open_doc(&backend, user_uri.clone(), "php", user_php).await;
    open_doc(
        &backend,
        uri_for(&dir, "src/Repository/UserRepository.php"),
        "php",
        repo_php,
    )
    .await;
    open_doc(
        &backend,
        uri_for(&dir, "config/services.yaml"),
        "yaml",
        services_yaml,
    )
    .await;
    open_doc(
        &backend,
        uri_for(&dir, "config/doctrine/User.orm.yaml"),
        "yaml",
        doctrine_yaml,
    )
    .await;
    open_doc(
        &backend,
        uri_for(&dir, "config/doctrine/User.orm.xml"),
        "xml",
        doctrine_xml,
    )
    .await;

    let refs = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: user_uri },
                position: Position::new(2, 7),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .expect("class references should include framework resources");

    let paths: Vec<String> = refs.iter().map(|loc| loc.uri.path().to_string()).collect();
    assert!(
        paths.iter().any(|p| p.ends_with("/config/services.yaml")),
        "expected services.yaml reference, got {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|p| p.ends_with("/config/doctrine/User.orm.yaml")),
        "expected Doctrine YAML reference, got {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|p| p.ends_with("/config/doctrine/User.orm.xml")),
        "expected Doctrine XML reference, got {paths:?}"
    );
}

#[tokio::test]
async fn class_rename_updates_symfony_and_doctrine_resources() {
    let user_php = "<?php\nnamespace App\\Entity;\nclass User {}\n";
    let services_yaml = "services:\n  app.user_service:\n    class: App\\Entity\\User\n";
    let doctrine_xml = r#"<doctrine-mapping>
  <entity name="App\Entity\User" />
</doctrine-mapping>
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Entity/User.php", user_php),
            ("config/services.yaml", services_yaml),
            ("config/doctrine/User.orm.xml", doctrine_xml),
        ],
    );

    let user_uri = uri_for(&dir, "src/Entity/User.php");
    let yaml_uri = uri_for(&dir, "config/services.yaml");
    let xml_uri = uri_for(&dir, "config/doctrine/User.orm.xml");
    open_doc(&backend, user_uri.clone(), "php", user_php).await;
    open_doc(&backend, yaml_uri.clone(), "yaml", services_yaml).await;
    open_doc(&backend, xml_uri.clone(), "xml", doctrine_xml).await;

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: user_uri },
                position: Position::new(2, 7),
            },
            new_name: "Customer".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("class rename should produce edits");

    assert!(
        edit_texts_for_uri(&edit, &yaml_uri)
            .iter()
            .any(|text| text == "App\\Entity\\Customer"),
        "expected services.yaml class edit, got {:?}",
        edit_texts_for_uri(&edit, &yaml_uri)
    );
    assert!(
        edit_texts_for_uri(&edit, &xml_uri)
            .iter()
            .any(|text| text == "App\\Entity\\Customer"),
        "expected Doctrine XML class edit, got {:?}",
        edit_texts_for_uri(&edit, &xml_uri)
    );
}

#[tokio::test]
async fn symfony_route_controller_action_resolves_and_renames_method() {
    let controller_php = "<?php\nnamespace App\\Controller;\nclass HomeController {\n    public function index(): void {}\n}\n";
    let routes_yaml = "home:\n  path: /\n  controller: App\\Controller\\HomeController::index\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Controller/HomeController.php", controller_php),
            ("config/routes.yaml", routes_yaml),
        ],
    );

    let controller_uri = uri_for(&dir, "src/Controller/HomeController.php");
    let routes_uri = uri_for(&dir, "config/routes.yaml");
    open_doc(&backend, controller_uri.clone(), "php", controller_php).await;
    open_doc(&backend, routes_uri.clone(), "yaml", routes_yaml).await;

    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: routes_uri.clone(),
                },
                position: Position::new(2, 56),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("controller method should resolve");
    let GotoDefinitionResponse::Scalar(location) = definition else {
        panic!("expected a single method definition");
    };
    assert_eq!(location.range.start.line, 3);

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: routes_uri.clone(),
                },
                position: Position::new(2, 56),
            },
            new_name: "dashboard".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("controller action rename should produce edits");

    assert!(
        edit_texts_for_uri(&edit, &routes_uri)
            .iter()
            .any(|text| text == "dashboard"),
        "expected route controller action edit, got {:?}",
        edit_texts_for_uri(&edit, &routes_uri)
    );
    assert!(
        edit_texts_for_uri(&edit, &controller_uri)
            .iter()
            .any(|text| text == "dashboard"),
        "expected PHP method declaration edit, got {:?}",
        edit_texts_for_uri(&edit, &controller_uri)
    );
}

#[tokio::test]
async fn symfony_namespace_prefix_rename_updates_yaml_and_php_namespace() {
    let mailer_php = "<?php\nnamespace App\\Service;\nclass Mailer {}\n";
    let services_yaml = "services:\n  App\\Service\\:\n    resource: '../src/Service/'\n  App\\Service\\Mailer: ~\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Service/Mailer.php", mailer_php),
            ("config/services.yaml", services_yaml),
        ],
    );

    let mailer_uri = uri_for(&dir, "src/Service/Mailer.php");
    let yaml_uri = uri_for(&dir, "config/services.yaml");
    open_doc(&backend, mailer_uri.clone(), "php", mailer_php).await;
    open_doc(&backend, yaml_uri.clone(), "yaml", services_yaml).await;

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: yaml_uri.clone(),
                },
                position: Position::new(1, 8),
            },
            new_name: "Domain".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("namespace-prefix rename should produce edits");

    let yaml_edits = edit_texts_for_uri(&edit, &yaml_uri);
    assert!(
        yaml_edits.iter().any(|text| text == "App\\Domain\\"),
        "expected YAML namespace-prefix edit, got {yaml_edits:?}"
    );
    assert!(
        yaml_edits.iter().any(|text| text == "App\\Domain\\Mailer"),
        "expected YAML class-reference edit, got {yaml_edits:?}"
    );
    assert!(
        yaml_edits.iter().any(|text| text == "../src/Domain/"),
        "expected YAML resource path edit, got {yaml_edits:?}"
    );
    assert!(
        edit_texts_for_uri(&edit, &mailer_uri)
            .iter()
            .any(|text| text == "App\\Domain"),
        "expected PHP namespace declaration edit, got {:?}",
        edit_texts_for_uri(&edit, &mailer_uri)
    );
}

#[tokio::test]
async fn renaming_a_controller_action_from_php_updates_the_route() {
    let controller_php = "<?php\nnamespace App\\Controller;\nclass HomeController {\n    public function index(): void {}\n}\n";
    let routes_yaml = "home:\n  path: /\n  controller: App\\Controller\\HomeController::index\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Controller/HomeController.php", controller_php),
            ("config/routes.yaml", routes_yaml),
        ],
    );

    let controller_uri = uri_for(&dir, "src/Controller/HomeController.php");
    let routes_uri = uri_for(&dir, "config/routes.yaml");
    open_doc(&backend, controller_uri.clone(), "php", controller_php).await;
    open_doc(&backend, routes_uri.clone(), "yaml", routes_yaml).await;

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: controller_uri.clone(),
                },
                position: Position::new(3, 21),
            },
            new_name: "dashboard".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("method rename should produce edits");

    assert_eq!(edit_texts_for_uri(&edit, &routes_uri), vec!["dashboard"]);
    assert_eq!(
        edit_texts_for_uri(&edit, &controller_uri),
        vec!["dashboard"]
    );
}
