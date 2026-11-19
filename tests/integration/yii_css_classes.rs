//! CSS class names in a Yii HTML tag's `class()` and `addClass()`: each
//! name in the string navigates to the selectors of the project's
//! stylesheets that declare it, completes from them, and shows them on
//! hover.  The receiver has to be a tag.
use crate::common::{
    completion_response, hover_response, hover_text, initialize_with_options, offset_position,
    open_document, response_items,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/html": "^3.0",
        "yiisoft/view": "^12.0"
    },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Yiisoft\\Html\\": "stubs/html/"
    } }
}"#;

const TAG_PHP: &str = r#"<?php
namespace Yiisoft\Html\Tag\Base;

abstract class Tag
{
    final public function class(\BackedEnum|string|null ...$class): static { return $this; }
    final public function addClass(\BackedEnum|string|null ...$class): static { return $this; }
}
"#;

const DIV_PHP: &str = r#"<?php
namespace Yiisoft\Html\Tag;

final class Div extends \Yiisoft\Html\Tag\Base\Tag {}
"#;

const HTML_PHP: &str = r#"<?php
namespace Yiisoft\Html;

final class Html
{
    public static function div(string $content = ''): Tag\Div { return new Tag\Div(); }
}
"#;

const SITE_CSS: &str = "/* .old { } */\n.card,\n.card-body {\n    margin: .5em;\n}\n";

const THEME_SCSS: &str = ".card {\n    &.active { color: red; }\n}\n";

const VIEW_PHP: &str = r#"<?php
use Yiisoft\Html\Html;

echo Html::div('Hi')->class('card card-body')->addClass('active', 'missing');

final class Other
{
    public function class(string $name): self { return $this; }
}

echo (new Other())->class('card');
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("stubs/html/Tag/Base/Tag.php", TAG_PHP),
        ("stubs/html/Tag/Div.php", DIV_PHP),
        ("stubs/html/Html.php", HTML_PHP),
        ("assets/site.css", SITE_CSS),
        ("assets/theme.scss", THEME_SCSS),
        ("node_modules/lib/lib.css", ".from-a-package { }\n"),
        ("resources/views/view.php", VIEW_PHP),
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
    let uri = uri_for(&dir, "resources/views/view.php");
    open_document(&backend, &uri, "php", VIEW_PHP).await;
    (backend, dir, uri)
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn at(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dir.path().join(rel)).unwrap()
}

fn text_document_position(uri: &Uri, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position,
    }
}

/// The file and the line of each definition.
async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32, u32)> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: text_document_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let locations = match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    };
    locations
        .into_iter()
        .map(|location| {
            (
                location.uri,
                location.range.start.line,
                location.range.start.character,
            )
        })
        .collect()
}

#[tokio::test]
async fn each_class_name_goes_to_the_selectors_that_declare_it() {
    let (backend, dir, uri) = workspace().await;
    let site = uri_for(&dir, "assets/site.css");
    let theme = uri_for(&dir, "assets/theme.scss");

    let mut found = definitions(&backend, &uri, at(VIEW_PHP, "card card-body", 2)).await;
    found.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    assert_eq!(found, vec![(site.clone(), 1, 1), (theme.clone(), 0, 1)]);

    let found = definitions(&backend, &uri, at(VIEW_PHP, "card-body'", 6)).await;
    assert_eq!(found, vec![(site, 2, 1)]);

    let found = definitions(&backend, &uri, at(VIEW_PHP, "active'", 2)).await;
    assert_eq!(found, vec![(theme, 1, 6)]);

    // A class nothing declares, and a `class()` on something that is no
    // tag, name nothing.
    let found = definitions(&backend, &uri, at(VIEW_PHP, "missing'", 2)).await;
    assert!(found.is_empty(), "{found:?}");
    let found = definitions(&backend, &uri, at(VIEW_PHP, "class('card')", 9)).await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn class_names_complete_one_at_a_time() {
    let (backend, dir, _) = workspace().await;
    let content = "<?php\nuse Yiisoft\\Html\\Html;\n\necho Html::div()->class('card c');\necho (new \\stdClass())->class('');\n";
    let uri = uri_for(&dir, "resources/views/draft.php");
    open_document(&backend, &uri, "php", content).await;

    let position = at(content, "card c'", 6);
    let items = response_items(
        completion_response(&backend, &uri, position.line, position.character, None).await,
    )
    .expect("completions");
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    // The name already written is not offered again, and a package's
    // stylesheet is not the project's.
    assert_eq!(labels, vec!["active", "card-body"]);
    let Some(CompletionItemTextEdit::TextEdit(edit)) = &items[0].text_edit else {
        panic!("expected a text edit");
    };
    assert_eq!(edit.range.start, at(content, "c'", 0));

    let position = at(content, "class('')", 7);
    let labels: Vec<String> = response_items(
        completion_response(&backend, &uri, position.line, position.character, None).await,
    )
    .unwrap_or_default()
    .into_iter()
    .map(|item| item.label)
    .collect();
    assert!(!labels.iter().any(|label| label == "card"), "{labels:?}");
}

#[tokio::test]
async fn hovering_a_class_name_shows_its_stylesheets() {
    let (backend, _dir, uri) = workspace().await;
    let position = at(VIEW_PHP, "card-body'", 3);
    let hover = hover_response(&backend, &uri, position.line, position.character)
        .await
        .expect("a hover");
    let markup = hover_text(&hover);
    assert!(markup.contains("**CSS class** `card-body`"), "{markup}");
    assert!(markup.contains("Declared in `assets/site.css`"), "{markup}");

    let position = at(VIEW_PHP, "missing'", 3);
    let hover = hover_response(&backend, &uri, position.line, position.character)
        .await
        .expect("a hover");
    assert!(
        hover_text(&hover).contains("Not declared in the project's stylesheets"),
        "{}",
        hover_text(&hover)
    );
}

#[tokio::test]
async fn references_list_the_uses_and_the_selectors() {
    let (backend, dir, uri) = workspace().await;
    let references = backend
        .references(ReferenceParams {
            text_document_position_params: text_document_position(
                &uri,
                at(VIEW_PHP, "card-body'", 3),
            ),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default();
    let mut found: Vec<(String, u32)> = references
        .iter()
        .map(|location| (location.uri.to_string(), location.range.start.line))
        .collect();
    found.sort();
    assert_eq!(
        found,
        vec![
            (uri_for(&dir, "assets/site.css").to_string(), 2),
            (uri.to_string(), 3),
        ]
    );
}
