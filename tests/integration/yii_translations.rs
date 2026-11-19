//! Yii translation message IDs: an ID a translator's `translate()` names
//! navigates to its entry in each locale's message file of the category it
//! is looked up in, completes from that category's IDs, shows the default
//! locale's message on hover, and is reported when no file declares it.
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
        "yiisoft/translator": "^3.0",
        "yiisoft/translator-message-php": "^1.1"
    },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Yiisoft\\Translator\\": "stubs/translator/"
    } },
    "extra": { "config-plugin": {
        "params": "config/params.php",
        "di": "config/di/*.php"
    } }
}"#;

const TRANSLATOR_PHP: &str = r#"<?php
namespace Yiisoft\Translator;

interface TranslatorInterface
{
    public function translate(string $id, array $parameters = [], ?string $category = null, ?string $locale = null): string;
    public function withDefaultCategory(string $category): static;
}
"#;

const PARAMS_PHP: &str = r#"<?php
return [
    'yiisoft/aliases' => [
        'aliases' => [
            '@root' => dirname(__DIR__),
            '@messages' => '@root/resources/messages',
        ],
    ],
    'yiisoft/translator' => [
        'locale' => 'en-US',
        'defaultCategory' => 'site',
    ],
];
"#;

const TRANSLATION_DI_PHP: &str = r#"<?php
use Yiisoft\Aliases\Aliases;
use Yiisoft\Translator\CategorySource;
use Yiisoft\Translator\Message\Php\MessageSource;

/** @var array $params */

return [
    'translation.site' => [
        'definition' => static function (Aliases $aliases) use ($params): CategorySource {
            return new CategorySource(
                $params['yiisoft/translator']['defaultCategory'],
                new MessageSource($aliases->get('@messages')),
            );
        },
        'tags' => ['translation.categorySource'],
    ],
];
"#;

const EXTRA_DI_PHP: &str = r#"<?php
return [
    'translation.extra' => [
        'definition' => static fn () => new \Yiisoft\Translator\CategorySource(
            'extra',
            new \Yiisoft\Translator\Message\Php\MessageSource(\dirname(__DIR__) . '/lang'),
        ),
        'tags' => ['translation.categorySource'],
    ],
];
"#;

const EN_SITE_PHP: &str = r#"<?php
return [
    'home.title' => 'Welcome',
    'home.intro' => 'Hello there',
];
"#;

const DE_SITE_PHP: &str = r#"<?php
return [
    'home.title' => 'Willkommen',
];
"#;

const EN_BLOG_PHP: &str = r#"<?php
return [
    'blog.title' => 'Blog',
    'blog.intro' => 'Our posts',
];
"#;

const EN_OPEN_PHP: &str = r#"<?php
return [
    ...require __DIR__ . '/shared.php',
    'open.known' => 'Known',
];
"#;

const EXTRA_PHP: &str = r#"<?php
return [
    'extra.key' => 'Extra',
];
"#;

const HOME_CONTROLLER_PHP: &str = r#"<?php
namespace App;

use Yiisoft\Translator\TranslatorInterface;

final class HomeController
{
    public function __construct(private TranslatorInterface $translator, private \ArrayObject $other) {}

    public function index(): array
    {
        return [
            $this->translator->translate('home.title'),
            $this->translator->translate('home.title', category: 'site'),
            $this->translator->translate('blog.title', [], 'blog'),
            $this->translator->withDefaultCategory('blog')->translate('blog.intro'),
            $this->translator->translate('extra.key', category: 'extra'),
            $this->translator->translate('home.titel'),
            $this->translator->translate('blog.nope', category: 'blog'),
            $this->translator->translate('Save changes'),
            $this->translator->translate('open.unknown', category: 'open'),
            $this->translator->translate('gettext.only', category: 'gettext'),
            $this->other->translate('home.other'),
        ];
    }
}
"#;

const BLOG_CONTROLLER_PHP: &str = r#"<?php
namespace App;

use Yiisoft\Translator\TranslatorInterface;

final class BlogController
{
    private TranslatorInterface $translator;

    public function __construct(TranslatorInterface $translator)
    {
        $this->translator = $translator->withDefaultCategory('blog');
    }

    public function index(): string
    {
        return $this->translator->translate('blog.title') . $this->translator->translate('blog.missing');
    }
}
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("stubs/translator/TranslatorInterface.php", TRANSLATOR_PHP),
        ("config/params.php", PARAMS_PHP),
        ("config/di/translation.php", TRANSLATION_DI_PHP),
        ("config/di/extra.php", EXTRA_DI_PHP),
        ("resources/messages/en/site.php", EN_SITE_PHP),
        ("resources/messages/de/site.php", DE_SITE_PHP),
        ("resources/messages/en/blog.php", EN_BLOG_PHP),
        ("resources/messages/en/open.php", EN_OPEN_PHP),
        ("config/lang/en/extra.php", EXTRA_PHP),
        ("src/HomeController.php", HOME_CONTROLLER_PHP),
        ("src/BlogController.php", BLOG_CONTROLLER_PHP),
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
    let path = dir.path().join(rel);
    Uri::from_file_path(dunce::canonicalize(&path).unwrap_or(path)).unwrap()
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
    let mut locations = match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    };
    locations.sort_by(|a, b| a.uri.as_str().cmp(b.uri.as_str()));
    locations
}

/// The entry of `quoted` in the message file at `rel`, inside its quotes.
fn entry(dir: &tempfile::TempDir, rel: &str, quoted: &str) -> Location {
    let content = content_of(rel);
    Location {
        uri: uri_for(dir, rel),
        range: Range {
            start: position_in(content, quoted, 1),
            end: position_in(content, quoted, quoted.len() - 1),
        },
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
async fn a_message_id_goes_to_its_entry_in_each_locale() {
    let (backend, dir) = workspace(&["src/HomeController.php", "src/BlogController.php"]).await;
    let home = uri_for(&dir, "src/HomeController.php");
    let at = |needle: &str| position_in(HOME_CONTROLLER_PHP, needle, 3);
    let site = vec![
        entry(&dir, "resources/messages/de/site.php", "'home.title'"),
        entry(&dir, "resources/messages/en/site.php", "'home.title'"),
    ];

    // The default category, which the params name.
    assert_eq!(
        definitions(&backend, &home, at("'home.title')")).await,
        site
    );
    // The default category, named.
    assert_eq!(
        definitions(&backend, &home, at("'home.title', category")).await,
        site
    );
    // A category passed by position.
    assert_eq!(
        definitions(&backend, &home, at("'blog.title', [], 'blog'")).await,
        vec![entry(
            &dir,
            "resources/messages/en/blog.php",
            "'blog.title'"
        )]
    );
    // A category the receiver chain gives.
    assert_eq!(
        definitions(&backend, &home, at("'blog.intro'")).await,
        vec![entry(
            &dir,
            "resources/messages/en/blog.php",
            "'blog.intro'"
        )]
    );
    // A message source whose directory is built from `__DIR__`.
    assert_eq!(
        definitions(&backend, &home, at("'extra.key'")).await,
        vec![entry(&dir, "config/lang/en/extra.php", "'extra.key'")]
    );
    // A receiver that is no translator names nothing.
    assert!(
        definitions(&backend, &home, at("'home.other'"))
            .await
            .is_empty()
    );

    // A category the class's `withDefaultCategory()` gives.
    let blog = uri_for(&dir, "src/BlogController.php");
    assert_eq!(
        definitions(
            &backend,
            &blog,
            position_in(BLOG_CONTROLLER_PHP, "'blog.title'", 3)
        )
        .await,
        vec![entry(
            &dir,
            "resources/messages/en/blog.php",
            "'blog.title'"
        )]
    );
}

#[tokio::test]
async fn an_id_no_message_file_declares_is_reported() {
    let (backend, dir) = workspace(&["src/HomeController.php", "src/BlogController.php"]).await;
    // An ID that is its own message, an ID in a category with a file that
    // cannot be read whole, and one in a category with no message file are
    // not judged.
    assert_eq!(
        reported(
            &backend,
            &dir,
            "src/HomeController.php",
            "invalid_yii_translation"
        ),
        vec![
            "Unknown message ID 'home.titel' in category 'site'".to_string(),
            "Unknown message ID 'blog.nope' in category 'blog'".to_string(),
        ]
    );
    assert_eq!(
        reported(
            &backend,
            &dir,
            "src/BlogController.php",
            "invalid_yii_translation"
        ),
        vec!["Unknown message ID 'blog.missing' in category 'blog'".to_string()]
    );
}

#[tokio::test]
async fn message_ids_complete_from_the_category() {
    let (backend, dir) = workspace(&[]).await;
    let uri = uri_for(&dir, "src/HomeController.php");
    let content = HOME_CONTROLLER_PHP.replace(
        "$this->translator->translate('home.title'),",
        "$this->translator->translate('home.'),\n            $this->translator->translate('blog.', category: 'blog'),",
    );
    open_document(&backend, &uri, "php", &content).await;
    for (needle, expected) in [
        ("translate('home.')", vec!["home.intro", "home.title"]),
        (
            "translate('blog.', category",
            vec!["blog.intro", "blog.title"],
        ),
    ] {
        let position = position_in(&content, needle, "translate('home.".len());
        let mut labels = response_labels(
            completion_response(&backend, &uri, position.line, position.character, None).await,
        );
        labels.sort();
        assert_eq!(labels, expected, "for {needle}");
    }
}

#[tokio::test]
async fn hovering_a_message_id_shows_the_default_locales_message() {
    let (backend, dir) = workspace(&["src/HomeController.php"]).await;
    let uri = uri_for(&dir, "src/HomeController.php");
    let position = position_in(HOME_CONTROLLER_PHP, "'home.title')", 3);
    let markup = markup_hover_at(&backend, &uri, position.line, position.character).await;
    assert!(markup.contains("**Translation** `home.title`"), "{markup}");
    assert!(markup.contains("Category `site`"), "{markup}");
    // `en-US` falls back to `en`.
    assert!(markup.contains("`en`: `Welcome`"), "{markup}");
    assert!(markup.contains("Declared for `de`, `en`"), "{markup}");
}

#[tokio::test]
async fn references_to_a_message_id_include_both_spellings_of_the_default_category() {
    let (backend, dir) = workspace(&["src/HomeController.php"]).await;
    let uri = uri_for(&dir, "src/HomeController.php");
    let position = position_in(HOME_CONTROLLER_PHP, "'home.title')", 3);
    let locations = backend
        .references(ReferenceParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: false,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default();
    let mut lines: Vec<u32> = locations
        .iter()
        .filter(|location| location.uri == uri)
        .map(|location| location.range.start.line)
        .collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        vec![
            position_in(HOME_CONTROLLER_PHP, "'home.title')", 0).line,
            position_in(HOME_CONTROLLER_PHP, "'home.title', category", 0).line,
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_message_directories_navigate_and_refresh_at_the_canonical_path() {
    let (backend, dir) = workspace(&[]).await;
    let source = dir.path().join("resources/messages");
    let target = dir.path().join("resources/catalogue");
    std::fs::rename(&source, &target).unwrap();
    std::os::unix::fs::symlink(&target, &source).unwrap();
    let home = uri_for(&dir, "src/HomeController.php");
    open_document(&backend, &home, "php", HOME_CONTROLLER_PHP).await;
    let position = position_in(HOME_CONTROLLER_PHP, "'home.title')", 3);
    assert_eq!(
        definitions(&backend, &home, position).await,
        vec![
            entry(&dir, "resources/messages/de/site.php", "'home.title'"),
            entry(&dir, "resources/messages/en/site.php", "'home.title'"),
        ]
    );

    let message = uri_for(&dir, "resources/messages/en/site.php");
    std::fs::write(
        target.join("en/site.php"),
        EN_SITE_PHP.replace("Welcome", "Welcome back"),
    )
    .unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: message,
                kind: FileChangeType::Changed,
            }],
        })
        .await;
    let hover = markup_hover_at(&backend, &home, position.line, position.character).await;
    assert!(hover.contains("Welcome back"), "{hover}");
}
