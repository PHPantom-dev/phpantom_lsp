//! Yii `config-plugin` entries in `composer.json`: group names complete
//! from the ones the installed packages and `configuration.php` use, paths
//! complete from the source directory, a `$group` goes to the group it
//! names and a path to its file, and a path with no file is reported.
use crate::common::{
    completion_response, initialize_with_options, message_text, offset_position, open_document,
    response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/config": "^1.6",
        "yiisoft/router": "^4.0"
    },
    "extra": {
        "config-plugin-file": "config/configuration.php",
        "config-plugin-options": {
            "source-directory": "config"
        },
        "config-plugin": {
            "params": "common/params.php",
            "di": "common/di/*.php",
            "di-web": ["$di", "web/di.php", "?web/local.php", "web/gone.php"]
        },
        "config-plugin-environments": {
            "dev": {
                "params": ["$params", "dev/params.php"]
            }
        }
    }
}"#;

const CONFIGURATION_PHP: &str = r#"<?php
return [
    'config-plugin' => [
        'routes' => 'common/routes.php',
        'bootstrap' => 'common/bootstrap.php',
    ],
];
"#;

const INSTALLED_JSON: &str = r#"{
    "packages": [
        {
            "name": "yiisoft/router",
            "install-path": "../yiisoft/router",
            "extra": {
                "config-plugin": {
                    "params": "params.php",
                    "di-console": "di-console.php"
                }
            }
        }
    ]
}"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("config/configuration.php", CONFIGURATION_PHP),
        ("config/common/params.php", "<?php\nreturn [];\n"),
        ("config/common/di/app.php", "<?php\nreturn [];\n"),
        ("config/common/di/blog.php", "<?php\nreturn [];\n"),
        ("config/web/di.php", "<?php\nreturn [];\n"),
        ("config/dev/params.php", "<?php\nreturn [];\n"),
        ("vendor/composer/installed.json", INSTALLED_JSON),
        ("vendor/yiisoft/router/composer.json", "{}"),
    ]
}

async fn workspace() -> (Backend, tempfile::TempDir, Uri) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files() {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    let uri = uri_for(&dir, "composer.json");
    open_document(&backend, &uri, "json", COMPOSER).await;
    (backend, dir, uri)
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dunce::canonicalize(dir.path()).unwrap().join(rel)).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<Location> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    }
}

/// The labels completion offers `inside` bytes into the first occurrence of
/// `needle` in `content`.
async fn labels_at(
    backend: &Backend,
    uri: &Uri,
    content: &str,
    needle: &str,
    inside: usize,
) -> Vec<String> {
    open_document(backend, uri, "json", content).await;
    let position = position_in(content, needle, inside);
    let mut labels = response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    );
    labels.sort();
    labels
}

#[tokio::test]
async fn a_group_reference_goes_to_the_group() {
    let (backend, _dir, uri) = workspace().await;
    let group = |quoted: &str| Range {
        start: position_in(COMPOSER, quoted, 1),
        end: position_in(COMPOSER, quoted, quoted.len() - 1),
    };
    assert_eq!(
        definitions(&backend, &uri, position_in(COMPOSER, "\"$di\"", 2)).await,
        vec![Location {
            uri: uri.clone(),
            range: group("\"di\""),
        }]
    );
    // An environment's `$params` is the base group of that name.
    assert_eq!(
        definitions(&backend, &uri, position_in(COMPOSER, "\"$params\"", 2)).await,
        vec![Location {
            uri: uri.clone(),
            range: group("\"params\""),
        }]
    );
}

#[tokio::test]
async fn a_path_goes_to_its_files() {
    let (backend, dir, uri) = workspace().await;
    let file = |rel: &str| Location {
        uri: uri_for(&dir, rel),
        range: Range::default(),
    };
    assert_eq!(
        definitions(&backend, &uri, position_in(COMPOSER, "web/di.php", 2)).await,
        vec![file("config/web/di.php")]
    );
    // A pattern goes to every file it matches.
    assert_eq!(
        definitions(&backend, &uri, position_in(COMPOSER, "common/di/*", 2)).await,
        vec![
            file("config/common/di/app.php"),
            file("config/common/di/blog.php")
        ]
    );
    assert!(
        definitions(&backend, &uri, position_in(COMPOSER, "web/gone.php", 2))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_path_with_no_file_is_reported() {
    let (backend, _dir, uri) = workspace().await;
    let mut diagnostics = Vec::new();
    backend.collect_resource_diagnostics(uri.as_str(), COMPOSER, &mut diagnostics);
    let reported: Vec<(&str, Range)> = diagnostics
        .iter()
        .filter(|d| matches!(&d.code, Some(Code::String(c)) if c == "invalid_yii_config_file"))
        .map(|d| (message_text(&d.message), d.range))
        .collect();
    // An optional file may be missing.
    assert_eq!(
        reported,
        vec![(
            "Unknown config file 'config/web/gone.php'",
            Range {
                start: position_in(COMPOSER, "web/gone.php", 0),
                end: position_in(COMPOSER, "web/gone.php", "web/gone.php".len()),
            }
        )]
    );
}

#[tokio::test]
async fn group_names_complete_from_the_ones_in_use() {
    let (backend, _dir, uri) = workspace().await;
    let content = COMPOSER.replace(
        "\"di\": \"common/di/*.php\",",
        "\"di\": \"common/di/*.php\",\n            \"\": [],",
    );
    // The groups declared beside it are taken already.
    assert_eq!(
        labels_at(&backend, &uri, &content, "\"\": [", 1).await,
        vec!["bootstrap", "di-console", "routes"]
    );
}

#[tokio::test]
async fn a_package_completes_the_groups_of_the_project_it_is_installed_in() {
    let (backend, dir, _) = workspace().await;
    let uri = uri_for(&dir, "vendor/yiisoft/router/composer.json");
    let content = r#"{
    "extra": {
        "config-plugin": {
            "params": "params.php",
            "": "di.php"
        }
    }
}"#;
    // The project's groups are `configuration.php`'s, which its
    // `composer.json` names in place of its own `config-plugin`.
    assert_eq!(
        labels_at(&backend, &uri, content, "\"\": \"", 1).await,
        vec!["bootstrap", "di-console", "routes"]
    );
}

#[tokio::test]
async fn group_references_complete_with_their_dollar() {
    let (backend, _dir, uri) = workspace().await;
    let content = COMPOSER.replace("[\"$di\",", "[\"$\",");
    assert_eq!(
        labels_at(&backend, &uri, &content, "[\"$", 3).await,
        vec!["$bootstrap", "$di", "$di-console", "$params", "$routes"]
    );
}

#[tokio::test]
async fn paths_complete_from_the_source_directory() {
    let (backend, _dir, uri) = workspace().await;
    let content = COMPOSER.replace("\"web/di.php\"", "\"?common/\"");
    assert_eq!(
        labels_at(&backend, &uri, &content, "\"?common/", 9).await,
        vec!["di", "params.php"]
    );
}
