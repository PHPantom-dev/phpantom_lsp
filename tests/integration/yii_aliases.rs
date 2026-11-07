//! Yii path aliases: the `@name` any string starts with navigates to the
//! configuration that declares it, completes from the declared names, and
//! the rest of the string links to the file or directory it names.
use crate::common::{
    completion_response, hover_response, hover_text, initialize_with_options, offset_position,
    open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/aliases": "^3.0",
        "yiisoft/yii-view-renderer": "^7.0"
    },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const ALIASES_PHP: &str = r#"<?php
return [
    '@root' => dirname(__DIR__, 2),
    '@views' => '@root/resources/views',
    '@runtime' => '@root/runtime',
    '@baseUrl' => '/',
];
"#;

const PARAMS_PHP: &str = r#"<?php
return [
    'yiisoft/aliases' => [
        'aliases' => require __DIR__ . '/aliases.php',
    ],
];
"#;

const ASSET_PHP: &str = r#"<?php
namespace App\Web;

final class Asset
{
    public ?string $sourcePath = '@views/blog';
    public ?string $card = '@views/blog/_card.php';
    public ?string $missing = '@runtime/missing';
    public ?string $url = '@baseUrl/favicon.svg';
    public ?string $plain = '@views';

    public function logs(object $aliases): string
    {
        return $aliases->get('@runtime/logs');
    }
}
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("config/common/aliases.php", ALIASES_PHP),
        ("config/common/params.php", PARAMS_PHP),
        ("src/Web/Asset.php", ASSET_PHP),
        ("resources/views/blog/_card.php", "<article></article>\n"),
        ("runtime/logs/.gitkeep", ""),
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
    Uri::from_file_path(dunce::canonicalize(dir.path()).unwrap().join(rel)).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

fn text_document_position(uri: &Uri, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position,
    }
}

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<Location> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: text_document_position(uri, position),
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

async fn references(
    backend: &Backend,
    uri: &Uri,
    position: Position,
    include_declaration: bool,
) -> Vec<(String, Position)> {
    let mut found: Vec<(String, Position)> = backend
        .references(ReferenceParams {
            text_document_position_params: text_document_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .map(|location| (location.uri.to_string(), location.range.start))
        .collect();
    found.sort_by_key(|(uri, position)| (uri.clone(), position.line, position.character));
    found
}

#[tokio::test]
async fn an_alias_in_any_string_goes_to_its_declaration() {
    let (backend, dir) = workspace(&["src/Web/Asset.php", "config/common/aliases.php"]).await;
    let aliases = uri_for(&dir, "config/common/aliases.php");
    for (file, needle, alias) in [
        ("src/Web/Asset.php", "'@views/blog'", "'@views'"),
        ("src/Web/Asset.php", "'@runtime/logs'", "'@runtime'"),
        ("src/Web/Asset.php", "'@views'", "'@views'"),
        ("config/common/aliases.php", "'@root/runtime'", "'@root'"),
    ] {
        let found = definitions(
            &backend,
            &uri_for(&dir, file),
            position_in(content_of(file), needle, 3),
        )
        .await;
        let start = position_in(ALIASES_PHP, alias, 1);
        let end = position_in(ALIASES_PHP, alias, alias.len() - 1);
        assert_eq!(
            found,
            vec![Location {
                uri: aliases.clone(),
                range: Range { start, end },
            }],
            "for {needle} in {file}"
        );
    }

    // The path after the alias is not the alias.
    let found = definitions(
        &backend,
        &uri_for(&dir, "src/Web/Asset.php"),
        position_in(ASSET_PHP, "'@views/blog'", 9),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn an_alias_lists_the_strings_that_name_it() {
    let (backend, dir) = workspace(&["src/Web/Asset.php"]).await;
    let asset = uri_for(&dir, "src/Web/Asset.php").to_string();
    let aliases = uri_for(&dir, "config/common/aliases.php").to_string();
    let at = |file: &str, content: &str, needle: &str| {
        (file.to_string(), position_in(content, needle, 1))
    };

    let uri = uri_for(&dir, "src/Web/Asset.php");
    let position = position_in(ASSET_PHP, "'@runtime/logs'", 2);
    assert_eq!(
        references(&backend, &uri, position, true).await,
        vec![
            at(&aliases, ALIASES_PHP, "'@runtime'"),
            at(&asset, ASSET_PHP, "'@runtime/missing'"),
            at(&asset, ASSET_PHP, "'@runtime/logs'"),
        ]
    );
    assert_eq!(
        references(&backend, &uri, position, false).await,
        vec![
            at(&asset, ASSET_PHP, "'@runtime/missing'"),
            at(&asset, ASSET_PHP, "'@runtime/logs'"),
        ]
    );
}

#[tokio::test]
async fn alias_names_complete_in_any_string() {
    let (backend, dir) = workspace(&[]).await;
    let content = "<?php\nfinal class Paths {\n    public string $path = '@';\n    public function a(object $aliases): string { return $aliases->get('@vi'); }\n}\n";
    let uri = uri_for(&dir, "src/Paths.php");
    open_document(&backend, &uri, "php", content).await;
    for needle in ["'@", "get('@vi"] {
        let position = position_in(content, needle, needle.len());
        let labels = response_labels(
            completion_response(&backend, &uri, position.line, position.character, None).await,
        );
        assert_eq!(
            labels,
            vec!["@baseUrl", "@root", "@runtime", "@views"],
            "for {needle}"
        );
    }
}

#[tokio::test]
async fn hovering_an_alias_shows_its_path() {
    let (backend, dir) = workspace(&["src/Web/Asset.php"]).await;
    let uri = uri_for(&dir, "src/Web/Asset.php");
    let position = position_in(ASSET_PHP, "'@views/blog'", 2);
    let hover = hover_response(&backend, &uri, position.line, position.character)
        .await
        .expect("a hover");
    let markup = hover_text(&hover);
    assert!(markup.contains("**Alias** `@views`"), "{markup}");
    assert!(
        markup.contains("`@root/resources/views`, which is `resources/views`"),
        "{markup}"
    );
}

#[tokio::test]
async fn the_path_after_an_alias_links_to_what_it_names() {
    let (backend, dir) = workspace(&["src/Web/Asset.php"]).await;
    let root = dunce::canonicalize(dir.path()).unwrap();
    let uri = uri_for(&dir, "src/Web/Asset.php");
    let links = backend
        .handle_document_link(uri.as_str(), ASSET_PHP)
        .unwrap_or_default();
    let found: Vec<(Position, Position, Uri)> = links
        .into_iter()
        .map(|link| (link.range.start, link.range.end, link.target.unwrap()))
        .collect();
    let range = |needle: &str| {
        (
            position_in(ASSET_PHP, needle, 0),
            position_in(ASSET_PHP, needle, needle.len() - 1),
        )
    };
    let link = |needle: &str, target: &str| {
        let (start, end) = range(needle);
        (start, end, Uri::from_file_path(root.join(target)).unwrap())
    };
    assert_eq!(
        found,
        vec![
            link("/blog'", "resources/views/blog"),
            link("/blog/_card.php'", "resources/views/blog/_card.php"),
            link("/logs'", "runtime/logs"),
        ]
    );
}
