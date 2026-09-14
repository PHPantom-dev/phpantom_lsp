//! Tests for Laravel's path helpers: `base_path('routes/web.php')` and friends.
//!
//! Each helper anchors its argument to a conventional directory under the
//! project root, so the file it names can be navigated to without booting the
//! application.  Directories are deliberately not navigable — an editor asked
//! to open a folder as a document reports an error — but they do complete, so
//! the next segment of the path can be typed against a real listing.

use crate::common::{create_psr4_workspace, goto_definition_at, position_of};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

/// A composer file with no Laravel dependency, so the helper names mean
/// whatever the project made them mean.
const PLAIN_COMPOSER_JSON: &str = r#"{
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const WORKSPACE_FILES: [(&str, &str); 4] = [
    ("routes/web.php", "<?php // routes\n"),
    ("config/app.php", "<?php return [];\n"),
    ("resources/views/welcome.blade.php", "<h1>Welcome</h1>\n"),
    ("public/index.php", "<?php // front controller\n"),
];

async fn workspace(
    composer_json: &str,
    consumer: &str,
) -> (phpantom_lsp::Backend, tempfile::TempDir, Url) {
    let mut files: Vec<(&str, &str)> = WORKSPACE_FILES.to_vec();
    files.push(("src/Paths.php", consumer));
    let (backend, dir) = create_psr4_workspace(composer_json, &files);
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Paths.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: consumer.to_string(),
            },
        })
        .await;
    (backend, dir, uri)
}

/// Go-to-definition at the first occurrence of `marker` in `consumer`.
async fn definition_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    consumer: &str,
    marker: &str,
) -> Option<GotoDefinitionResponse> {
    let position = position_of(consumer, marker);
    goto_definition_at(backend, uri, position.line, position.character).await
}

/// The single target of a go-to-definition response.
fn single_target(response: &GotoDefinitionResponse) -> Url {
    match response {
        GotoDefinitionResponse::Scalar(location) => location.uri.clone(),
        GotoDefinitionResponse::Array(locations) => {
            assert_eq!(locations.len(), 1, "expected one target: {locations:?}");
            locations[0].uri.clone()
        }
        GotoDefinitionResponse::Link(links) => {
            assert_eq!(links.len(), 1, "expected one target: {links:?}");
            links[0].target_uri.clone()
        }
    }
}

// ─── Go-to-definition ───────────────────────────────────────────────────────

#[tokio::test]
async fn every_helper_resolves_against_its_own_directory() {
    const CONSUMER: &str = "\
<?php
namespace App;

class Paths {
    public function demo(): void {
        base_path('routes/web.php');
        config_path('app.php');
        resource_path('views/welcome.blade.php');
        public_path('index.php');
    }
}
";
    let (backend, dir, uri) = workspace(COMPOSER_JSON, CONSUMER).await;

    for (marker, expected) in [
        ("routes/web.php", "routes/web.php"),
        ("app.php'", "config/app.php"),
        ("views/welcome", "resources/views/welcome.blade.php"),
        ("index.php", "public/index.php"),
    ] {
        let response = definition_at(&backend, &uri, CONSUMER, marker)
            .await
            .unwrap_or_else(|| panic!("{marker} should resolve"));
        assert_eq!(
            single_target(&response),
            Url::from_file_path(dir.path().join(expected)).unwrap(),
            "{marker} should land in {expected}"
        );
    }
}

#[tokio::test]
async fn a_missing_file_is_not_navigable() {
    const CONSUMER: &str = "\
<?php
namespace App;

class Paths {
    public function demo(): void {
        base_path('routes/console.php');
    }
}
";
    let (backend, _dir, uri) = workspace(COMPOSER_JSON, CONSUMER).await;

    let response = definition_at(&backend, &uri, CONSUMER, "routes/console.php").await;
    assert!(
        response.is_none_or(|r| matches!(&r, GotoDefinitionResponse::Array(l) if l.is_empty())),
        "nothing is there to open"
    );
}

#[tokio::test]
async fn a_directory_is_not_navigable() {
    const CONSUMER: &str = "\
<?php
namespace App;

class Paths {
    public function demo(): void {
        base_path('routes');
    }
}
";
    let (backend, _dir, uri) = workspace(COMPOSER_JSON, CONSUMER).await;

    let response = definition_at(&backend, &uri, CONSUMER, "'routes'").await;
    assert!(
        response.is_none_or(|r| matches!(&r, GotoDefinitionResponse::Array(l) if l.is_empty())),
        "an editor cannot open a folder as a document"
    );
}

#[tokio::test]
async fn outside_laravel_the_names_mean_nothing() {
    const CONSUMER: &str = "\
<?php
namespace App;

class Paths {
    public function demo(): void {
        base_path('routes/web.php');
    }
}
";
    let (backend, _dir, uri) = workspace(PLAIN_COMPOSER_JSON, CONSUMER).await;

    let response = definition_at(&backend, &uri, CONSUMER, "routes/web.php").await;
    assert!(
        response.is_none_or(|r| matches!(&r, GotoDefinitionResponse::Array(l) if l.is_empty())),
        "a project without Laravel defines its own base_path()"
    );
}

// ─── Document links ─────────────────────────────────────────────────────────

#[tokio::test]
async fn helper_arguments_become_document_links() {
    const CONSUMER: &str = "\
<?php
namespace App;

class Paths {
    public function demo(): void {
        base_path('routes/web.php');
        base_path('routes');
        base_path('routes/console.php');
    }
}
";
    let (backend, dir, uri) = workspace(COMPOSER_JSON, CONSUMER).await;

    let links = backend
        .handle_document_link(uri.as_str(), CONSUMER)
        .unwrap_or_default();
    let targets: Vec<String> = links
        .iter()
        .filter_map(|link| link.target.as_ref().map(|url| url.to_string()))
        .collect();

    assert_eq!(
        targets,
        vec![
            Url::from_file_path(dir.path().join("routes/web.php"))
                .unwrap()
                .to_string()
        ],
        "only the argument naming an existing file is linked"
    );

    // The link covers the string's contents, not its quotes.
    let line = CONSUMER
        .lines()
        .position(|l| l.contains("routes/web.php"))
        .unwrap() as u32;
    assert_eq!(links[0].range.start.line, line);
    let column = CONSUMER
        .lines()
        .nth(line as usize)
        .unwrap()
        .find("routes/web.php")
        .unwrap() as u32;
    assert_eq!(links[0].range.start.character, column);
}

// ─── Completion ─────────────────────────────────────────────────────────────

/// Complete at the end of `marker` in `consumer`.
async fn completion_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    consumer: &str,
    marker: &str,
) -> Vec<CompletionItem> {
    let offset = consumer.find(marker).expect("marker is in the source") + marker.len();
    let line = consumer[..offset].matches('\n').count() as u32;
    let line_start = consumer[..offset].rfind('\n').map_or(0, |idx| idx + 1);
    let character = (offset - line_start) as u32;

    let response = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    match response {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    }
}

#[tokio::test]
async fn an_empty_argument_lists_the_helper_directory() {
    const CONSUMER: &str = "\
<?php
namespace App;

class Paths {
    public function demo(): void {
        $path = resource_path('');
    }
}
";
    let (backend, _dir, uri) = workspace(COMPOSER_JSON, CONSUMER).await;

    let items = completion_at(&backend, &uri, CONSUMER, "resource_path('").await;
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, vec!["views"], "resources/ holds only views/");
    assert_eq!(items[0].kind, Some(CompletionItemKind::FOLDER));

    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("expected a text edit");
    };
    assert_eq!(
        edit.new_text, "views/",
        "a directory keeps its separator so the next segment follows on"
    );
}

#[tokio::test]
async fn a_typed_segment_lists_that_directory() {
    const CONSUMER: &str = "\
<?php
namespace App;

class Paths {
    public function demo(): void {
        $path = resource_path('views/wel');
    }
}
";
    let (backend, _dir, uri) = workspace(COMPOSER_JSON, CONSUMER).await;

    let items = completion_at(&backend, &uri, CONSUMER, "views/wel").await;
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, vec!["welcome.blade.php"]);
    assert_eq!(items[0].kind, Some(CompletionItemKind::FILE));

    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("expected a text edit");
    };
    assert_eq!(
        edit.new_text, "views/welcome.blade.php",
        "the edit replaces the whole literal, segments included"
    );
}

#[tokio::test]
async fn a_prefix_that_matches_nothing_offers_nothing() {
    const CONSUMER: &str = "\
<?php
namespace App;

class Paths {
    public function demo(): void {
        $path = config_path('zz');
    }
}
";
    let (backend, _dir, uri) = workspace(COMPOSER_JSON, CONSUMER).await;

    let items = completion_at(&backend, &uri, CONSUMER, "config_path('zz").await;
    assert!(
        items.iter().all(|item| item.label != "app.php"),
        "config/ has no entry starting with zz"
    );
}
