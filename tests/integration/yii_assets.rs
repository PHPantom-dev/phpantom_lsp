//! Yii asset bundles: each file a bundle's `$css` and `$js` list navigates
//! to the file under the bundle's `$sourcePath` (or `$basePath`), completes
//! from the files there, and is reported when no file is there.  An entry
//! that is a URL is left alone.
use crate::common::{
    completion_response, hover_response, hover_text, initialize_with_options, message_text,
    offset_position, open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/aliases": "^3.0",
        "yiisoft/assets": "^5.0",
        "yiisoft/view": "^12.0"
    },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const ALIASES_PHP: &str = r#"<?php
return [
    '@root' => dirname(__DIR__, 2),
    '@assets' => '@root/public/assets',
    '@assetsSource' => '@root/assets',
];
"#;

const PARAMS_PHP: &str = r#"<?php
return [
    'yiisoft/aliases' => [
        'aliases' => require __DIR__ . '/aliases.php',
    ],
];
"#;

const MAIN_ASSET_PHP: &str = r#"<?php
namespace App\Asset;

use Yiisoft\Assets\AssetBundle;

final class MainAsset extends AssetBundle
{
    public ?string $basePath = '@assets/main';
    public ?string $sourcePath = '@assetsSource/main';

    public array $css = [
        'site.css',
        ['css/print.css', 'media' => 'print'],
        'theme' => 'missing.css',
        'https://cdn.example.com/x.css',
        '//cdn.example.com/y.css',
        '/favicon.css',
    ];

    public array $js = [
        'app.js',
    ];
}
"#;

/// A bundle whose files are already in place under its `$basePath`, built
/// from `__DIR__`.
const LOCAL_ASSET_PHP: &str = r#"<?php
namespace App\Asset;

final class LocalAsset extends \Yiisoft\Assets\AssetBundle
{
    public ?string $basePath = __DIR__ . '/dist';
    public array $js = ['local.js', 'gone.js'];
}
"#;

/// Nothing a bundle could be checked against: the directory it publishes
/// to before it is published, a CDN, and a class that is no bundle.
const UNJUDGED_PHP: &str = r#"<?php
namespace App\Asset;

use Yiisoft\Assets\AssetBundle;

final class PublishedAsset extends AssetBundle
{
    public ?string $basePath = '@assets/published';
    public array $css = ['published.css'];
}

final class CdnAsset extends AssetBundle
{
    public bool $cdn = true;
    public ?string $sourcePath = '@assetsSource/main';
    public array $css = ['cdn.css'];
}

final class NotABundle
{
    public ?string $sourcePath = '@assetsSource/main';
    public array $css = ['other.css'];
}
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("config/common/aliases.php", ALIASES_PHP),
        ("config/common/params.php", PARAMS_PHP),
        ("src/Asset/MainAsset.php", MAIN_ASSET_PHP),
        ("src/Asset/LocalAsset.php", LOCAL_ASSET_PHP),
        ("src/Asset/Unjudged.php", UNJUDGED_PHP),
        ("assets/main/site.css", "body {}\n"),
        ("assets/main/css/print.css", "body {}\n"),
        ("assets/main/app.js", "\n"),
        ("src/Asset/dist/local.js", "\n"),
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
    let root = dunce::canonicalize(dir.path()).unwrap();
    initialize_with_options(&backend, &root, None).await;
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
    let path = dunce::canonicalize(dir.path()).unwrap().join(rel);
    Uri::from_file_path(path).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<Uri> {
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
        Some(DefinitionResponse::Definition(Definition::Location(location))) => {
            vec![location.uri]
        }
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => {
            locations.into_iter().map(|location| location.uri).collect()
        }
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    }
}

/// The `invalid_yii_asset` messages reported on `file`.
fn reported(backend: &Backend, dir: &tempfile::TempDir, file: &str) -> Vec<String> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(
        uri_for(dir, file).as_str(),
        content_of(file),
        &mut diagnostics,
    );
    diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code)) if code == "invalid_yii_asset"
            )
        })
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect()
}

#[tokio::test]
async fn an_entry_goes_to_its_file() {
    let (backend, dir) = workspace(&["src/Asset/MainAsset.php", "src/Asset/LocalAsset.php"]).await;
    for (file, needle, expected) in [
        (
            "src/Asset/MainAsset.php",
            "'site.css",
            "assets/main/site.css",
        ),
        (
            "src/Asset/MainAsset.php",
            "'css/print.css",
            "assets/main/css/print.css",
        ),
        ("src/Asset/MainAsset.php", "'app.js", "assets/main/app.js"),
        (
            "src/Asset/LocalAsset.php",
            "'local.js",
            "src/Asset/dist/local.js",
        ),
    ] {
        let uri = uri_for(&dir, file);
        let position = position_in(content_of(file), needle, 2);
        let found = definitions(&backend, &uri, position).await;
        assert_eq!(found, vec![uri_for(&dir, expected)], "for {needle}");
    }
    let uri = uri_for(&dir, "src/Asset/MainAsset.php");
    let position = position_in(MAIN_ASSET_PHP, "'https:", 2);
    assert!(definitions(&backend, &uri, position).await.is_empty());
}

#[tokio::test]
async fn an_entry_with_no_file_is_reported() {
    let (backend, dir) = workspace(&[
        "src/Asset/MainAsset.php",
        "src/Asset/LocalAsset.php",
        "src/Asset/Unjudged.php",
    ])
    .await;
    assert_eq!(
        reported(&backend, &dir, "src/Asset/MainAsset.php"),
        vec!["Unknown asset file 'assets/main/missing.css'".to_string()]
    );
    assert_eq!(
        reported(&backend, &dir, "src/Asset/LocalAsset.php"),
        vec!["Unknown asset file 'src/Asset/dist/gone.js'".to_string()]
    );
    assert!(reported(&backend, &dir, "src/Asset/Unjudged.php").is_empty());
}

#[tokio::test]
async fn entries_complete_from_the_bundle_directory() {
    let (backend, dir) = workspace(&[]).await;
    for (needle, content, expected) in [
        (
            "['",
            "<?php\nnamespace App\\Asset;\nuse Yiisoft\\Assets\\AssetBundle;\nfinal class Draft extends AssetBundle {\n    public ?string $sourcePath = '@assetsSource/main';\n    public array $css = [''];\n}\n",
            vec!["css", "app.js", "site.css"],
        ),
        (
            "['css/",
            "<?php\nnamespace App\\Asset;\nuse Yiisoft\\Assets\\AssetBundle;\nfinal class Draft extends AssetBundle {\n    public ?string $sourcePath = '@assetsSource/main';\n    public array $css = [['css/', 'media' => 'print']];\n}\n",
            vec!["print.css"],
        ),
    ] {
        let uri = uri_for(&dir, "src/Asset/Draft.php");
        open_document(&backend, &uri, "php", content).await;
        let position = position_in(content, needle, needle.len());
        let labels = response_labels(
            completion_response(&backend, &uri, position.line, position.character, None).await,
        );
        let expected: Vec<String> = expected.into_iter().map(str::to_string).collect();
        assert_eq!(labels, expected, "for {needle}");
    }
}

#[tokio::test]
async fn hovering_an_entry_shows_its_file() {
    let (backend, dir) = workspace(&["src/Asset/MainAsset.php"]).await;
    let uri = uri_for(&dir, "src/Asset/MainAsset.php");
    let position = position_in(MAIN_ASSET_PHP, "'site.css", 2);
    let hover = hover_response(&backend, &uri, position.line, position.character)
        .await
        .expect("a hover");
    let markup = hover_text(&hover);
    assert!(
        markup.contains("**Asset file** `assets/main/site.css`"),
        "{markup}"
    );
}
