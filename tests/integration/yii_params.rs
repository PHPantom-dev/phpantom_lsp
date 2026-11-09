//! Yii params: the keys a configuration file reads from the `$params`
//! `yiisoft/config` hands it navigate to the params file that declares
//! them, complete a level at a time, are reported when no params file
//! declares them, and read as the type of the value declared.
use crate::common::{
    completion_response, initialize_with_options, markup_hover_at, message_text, offset_position,
    open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/config": "^1.6",
        "yiisoft/router": "^4.0"
    },
    "autoload": { "psr-4": { "App\\": "src/" } },
    "extra": { "config-plugin-file": "config/configuration.php" }
}"#;

const CONFIGURATION_PHP: &str = r#"<?php
use App\Environment;

return [
    'config-plugin' => [
        'params' => 'common/params.php',
        'params-web' => ['$params', 'web/params.php'],
        'di' => 'common/di/*.php',
        'di-web' => ['$di', 'web/di/*.php'],
    ],
    'config-plugin-environments' => [
        Environment::DEV => [
            'params' => ['environments/dev/params.php'],
        ],
    ],
    'config-plugin-options' => [
        'source-directory' => 'config',
    ],
];
"#;

const PARAMS_PHP: &str = r#"<?php
return [
    'application' => require __DIR__ . '/application.php',
    'blog' => [
        'posts' => ['hello' => 'Hello'],
        'pageSize' => 5,
    ],
    'empty' => [],
];
"#;

const APPLICATION_PHP: &str = r#"<?php
return [
    'charset' => 'UTF-8',
    'name' => 'My Project',
];
"#;

const WEB_PARAMS_PHP: &str = r#"<?php
return [
    'blog' => [
        'pageSize' => 10,
    ],
];
"#;

const DEV_PARAMS_PHP: &str = r#"<?php
return [
    'traceLink' => 'phpstorm://open?url=file://{file}&line={line}',
];
"#;

const DI_PHP: &str = r#"<?php
/** @var array $params */

$size = $params['blog']['pageSize'];
$posts = $params['blog']['posts'];
$name = $params['application']['name'];

return [
    'name' => $params['application']['name'],
    'size' => $params['blog']['pageSize'],
    'typo' => $params['application']['nmae'],
    'missing' => $params['nothing'],
    'under' => $params['nothing']['deeper'],
    'open' => $params['empty']['anything'],
    'guarded' => $params['other'] ?? null,
    'checked' => isset($params['gone']) ? 1 : 0,
    'link' => static fn () => $params['traceLink'],
];
"#;

const CONTROLLER_PHP: &str = r#"<?php
namespace App;

$params = ['id' => 1];
echo $params['nothing'];
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("config/configuration.php", CONFIGURATION_PHP),
        ("config/common/params.php", PARAMS_PHP),
        ("config/common/application.php", APPLICATION_PHP),
        ("config/web/params.php", WEB_PARAMS_PHP),
        ("config/environments/dev/params.php", DEV_PARAMS_PHP),
        ("config/common/di/app.php", DI_PHP),
        ("src/controller.php", CONTROLLER_PHP),
    ]
}

/// A session over the workspace with `open` open.
async fn workspace(open: &[&str]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files() {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    for path in open {
        open_document(&backend, &uri_for(&dir, path), "php", content_of(path)).await;
    }
    (backend, dir)
}

fn content_of(path: &str) -> &'static str {
    files()
        .into_iter()
        .find(|(file, _)| *file == path)
        .unwrap()
        .1
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dir.path().join(rel)).unwrap()
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

/// The range of the text of the string `quoted` in `content`, inside its
/// quotes.
fn key_range(content: &str, quoted: &str) -> Range {
    Range {
        start: position_in(content, quoted, 1),
        end: position_in(content, quoted, quoted.len() - 1),
    }
}

/// The messages of the diagnostics with `code` the file at `rel` gets.
fn reported(backend: &Backend, dir: &tempfile::TempDir, rel: &str, code: &str) -> Vec<String> {
    let uri = uri_for(dir, rel);
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content_of(rel), &mut diagnostics);
    diagnostics
        .iter()
        .filter(|diagnostic| matches!(&diagnostic.code, Some(Code::String(found)) if found == code))
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect()
}

#[tokio::test]
async fn a_params_key_goes_to_the_entry_that_declares_it() {
    let (backend, dir) = workspace(&["config/common/di/app.php"]).await;
    let di = uri_for(&dir, "config/common/di/app.php");
    let at = |needle: &str, inside: usize| position_in(DI_PHP, needle, inside);

    // A key a required file declares.
    assert_eq!(
        definitions(&backend, &di, at("['name'],\n    'size'", 3)).await,
        vec![Location {
            uri: uri_for(&dir, "config/common/application.php"),
            range: key_range(APPLICATION_PHP, "'name'"),
        }]
    );
    // A group each params file adds keys to, and a key the web params
    // override.
    assert_eq!(
        definitions(&backend, &di, at("['blog']['pageSize'],", 3)).await,
        vec![
            Location {
                uri: uri_for(&dir, "config/common/params.php"),
                range: key_range(PARAMS_PHP, "'blog'"),
            },
            Location {
                uri: uri_for(&dir, "config/web/params.php"),
                range: key_range(WEB_PARAMS_PHP, "'blog'"),
            },
        ]
    );
    let blog = WEB_PARAMS_PHP.find("'blog'").unwrap();
    let page_size = blog + WEB_PARAMS_PHP[blog..].find("'pageSize'").unwrap();
    assert_eq!(
        definitions(&backend, &di, at("['pageSize'],", 3)).await,
        vec![Location {
            uri: uri_for(&dir, "config/web/params.php"),
            range: Range {
                start: offset_position(WEB_PARAMS_PHP, page_size + 1),
                end: offset_position(WEB_PARAMS_PHP, page_size + "'pageSize".len()),
            },
        }]
    );
    // A key only an environment declares.
    assert_eq!(
        definitions(&backend, &di, at("['traceLink']", 3)).await,
        vec![Location {
            uri: uri_for(&dir, "config/environments/dev/params.php"),
            range: key_range(DEV_PARAMS_PHP, "'traceLink'"),
        }]
    );
}

#[tokio::test]
async fn a_key_no_params_file_declares_is_reported() {
    let (backend, dir) = workspace(&["config/common/di/app.php", "src/controller.php"]).await;
    assert_eq!(
        reported(
            &backend,
            &dir,
            "config/common/di/app.php",
            "invalid_yii_param"
        ),
        vec![
            "Unknown params key ['application']['nmae']".to_string(),
            "Unknown params key ['nothing']".to_string(),
            // The key under it is not judged.
            "Unknown params key ['nothing']".to_string(),
        ]
    );
    // A `$params` no configuration file is handed is any other variable.
    assert!(reported(&backend, &dir, "src/controller.php", "invalid_yii_param").is_empty());
}

#[tokio::test]
async fn params_keys_complete_a_level_at_a_time() {
    let (backend, dir) = workspace(&[]).await;
    let uri = uri_for(&dir, "config/common/di/app.php");
    let content =
        "<?php\nreturn [\n    'a' => $params[''],\n    'b' => $params['application']['c'],\n];\n";
    open_document(&backend, &uri, "php", content).await;
    for (needle, expected) in [
        (
            "$params['",
            vec!["application", "blog", "empty", "traceLink"],
        ),
        ("['application']['", vec!["charset", "name"]),
    ] {
        let position = position_in(content, needle, needle.len());
        let mut labels = response_labels(
            completion_response(&backend, &uri, position.line, position.character, None).await,
        );
        labels.sort();
        assert_eq!(labels, expected, "for {needle}");
    }
}

#[tokio::test]
async fn hovering_a_params_key_shows_its_value() {
    let (backend, dir) = workspace(&["config/common/di/app.php"]).await;
    let uri = uri_for(&dir, "config/common/di/app.php");
    let position = position_in(DI_PHP, "['name'],\n    'size'", 3);
    let markup = markup_hover_at(&backend, &uri, position.line, position.character).await;
    assert!(
        markup.contains("**Param** `['application']['name']`"),
        "{markup}"
    );
    assert!(markup.contains("`string`"), "{markup}");
    assert!(
        markup.contains("Declared in `config/common/application.php`"),
        "{markup}"
    );
}

#[tokio::test]
async fn a_params_read_is_typed_as_the_declared_value() {
    let (backend, dir) = workspace(&["config/common/di/app.php"]).await;
    let uri = uri_for(&dir, "config/common/di/app.php");
    for (variable, expected) in [
        ("$size", "int"),
        ("$name", "string"),
        ("$posts", "array{hello: string}"),
    ] {
        let position = position_in(DI_PHP, variable, 1);
        let markup = markup_hover_at(&backend, &uri, position.line, position.character).await;
        assert!(
            markup.contains(expected),
            "{variable} should be {expected}: {markup}"
        );
    }
}
