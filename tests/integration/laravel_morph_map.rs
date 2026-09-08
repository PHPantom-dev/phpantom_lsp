//! Tests for Eloquent morph-map alias support.
//!
//! An alias registered with `Relation::morphMap()` in a service provider is
//! surfaced wherever the alias appears as a string literal: hover names the
//! model it maps to, go-to-definition jumps to the registration and the model,
//! find-references links every usage, and — when the project calls
//! `enforceMorphMap()` — an unregistered alias is flagged.

use crate::common::{create_psr4_workspace, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const PROVIDERS_PHP: &str = "\
<?php
return [
    App\\Providers\\AppServiceProvider::class,
];
";

const POST_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Post extends Model {}
";

const VIDEO_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Video extends Model {}
";

const COMMENT_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Relations\\MorphTo;
class Comment extends Model
{
    public function commentable(): MorphTo
    {
        return $this->morphTo();
    }
}
";

/// A provider registering the map, optionally enforcing it.
fn provider(enforce: bool) -> String {
    let method = if enforce {
        "enforceMorphMap"
    } else {
        "morphMap"
    };
    format!(
        "\
<?php
namespace App\\Providers;
use App\\Models\\Post;
use App\\Models\\Video;
use Illuminate\\Database\\Eloquent\\Relations\\Relation;
use Illuminate\\Support\\ServiceProvider;
class AppServiceProvider extends ServiceProvider
{{
    public function boot(): void
    {{
        Relation::{method}([
            'post' => Post::class,
            'video' => Video::class,
        ]);
    }}
}}
"
    )
}

async fn open(backend: &phpantom_lsp::Backend, uri: &str, text: &str) {
    open_php(backend, &Url::parse(uri).unwrap(), text).await;
}

/// Position of the cursor immediately after the first occurrence of `needle`.
fn position_after(content: &str, needle: &str) -> Position {
    let idx = content.find(needle).expect("needle not found") + needle.len();
    let mut line = 0u32;
    let mut character = 0u32;
    for (i, ch) in content.char_indices() {
        if i == idx {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }
    Position { line, character }
}

/// Build a workspace with the provider, both models, and `src/Consumer.php`.
async fn workspace(
    enforce: bool,
    consumer: &str,
) -> (phpantom_lsp::Backend, tempfile::TempDir, String) {
    let provider_src = provider(enforce);
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("src/Providers/AppServiceProvider.php", &provider_src),
            ("src/Models/Post.php", POST_PHP),
            ("src/Models/Video.php", VIDEO_PHP),
            ("src/Models/Comment.php", COMMENT_PHP),
            ("src/Consumer.php", consumer),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Consumer.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, consumer).await;
    (backend, dir, uri)
}

async fn hover_at(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    position: Position,
) -> Option<String> {
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()?;
    match hover.contents {
        HoverContents::Markup(markup) => Some(markup.value),
        HoverContents::Scalar(MarkedString::String(s)) => Some(s),
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => Some(ls.value),
        HoverContents::Array(items) => Some(
            items
                .into_iter()
                .map(|item| match item {
                    MarkedString::String(s) => s,
                    MarkedString::LanguageString(ls) => ls.value,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    }
}

async fn definition_uris(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    position: Position,
) -> Vec<String> {
    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    match result {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc.uri.to_string()],
        Some(GotoDefinitionResponse::Array(locs)) => {
            locs.into_iter().map(|l| l.uri.to_string()).collect()
        }
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|l| l.target_uri.to_string())
            .collect(),
    }
}

fn morph_diagnostics(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| {
            matches!(&d.code, Some(NumberOrString::String(c)) if c == "invalid_laravel_morph_alias")
        })
        .collect()
}

const WHERE_HAS_MORPH_CONSUMER: &str = "\
<?php
namespace App;
use App\\Models\\Comment;
class Consumer {
    public function go(): void {
        Comment::whereHasMorph('commentable', ['post', 'video'])->get();
    }
}
";

#[tokio::test]
async fn hover_on_a_morph_alias_names_the_mapped_model() {
    let (backend, _dir, uri) = workspace(false, WHERE_HAS_MORPH_CONSUMER).await;

    let position = position_after(WHERE_HAS_MORPH_CONSUMER, "['po");
    let hover = hover_at(&backend, &uri, position)
        .await
        .expect("morph alias should hover");
    assert!(
        hover.contains("App\\Models\\Post"),
        "hover should name the mapped model, got: {hover}"
    );
    assert!(
        hover.contains("AppServiceProvider.php"),
        "hover should name the registering file, got: {hover}"
    );
}

#[tokio::test]
async fn morph_alias_resolves_to_its_registration_and_model() {
    let (backend, _dir, uri) = workspace(false, WHERE_HAS_MORPH_CONSUMER).await;

    let position = position_after(WHERE_HAS_MORPH_CONSUMER, "['po");
    let targets = definition_uris(&backend, &uri, position).await;
    assert!(
        targets
            .iter()
            .any(|t| t.ends_with("/Providers/AppServiceProvider.php")),
        "should offer the registration site, got {targets:?}"
    );
    assert!(
        targets.iter().any(|t| t.ends_with("/Models/Post.php")),
        "should offer the mapped model, got {targets:?}"
    );
}

#[tokio::test]
async fn unregistered_alias_is_flagged_only_when_the_map_is_enforced() {
    let consumer = "\
<?php
namespace App;
use App\\Models\\Comment;
class Consumer {
    public function go(): void {
        Comment::whereHasMorph('commentable', ['post', 'audio'])->get();
    }
}
";

    // Without `enforceMorphMap()` an unmapped model still morphs under its own
    // class name, so the set of valid `*_type` values is open.
    let (backend, _dir, uri) = workspace(false, consumer).await;
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    assert!(
        morph_diagnostics(&diags).is_empty(),
        "a non-enforced map must not flag anything, got {:?}",
        morph_diagnostics(&diags)
    );

    // With the map enforced, every morphable model must be mapped.
    let (backend, _dir, uri) = workspace(true, consumer).await;
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    let flagged = morph_diagnostics(&diags);
    assert_eq!(
        flagged.len(),
        1,
        "only the unregistered alias should be flagged, got {flagged:?}"
    );
    assert!(
        flagged[0].message.contains("audio"),
        "message should name the bad alias, got {:?}",
        flagged[0].message
    );
}

#[tokio::test]
async fn wildcard_and_class_name_types_are_not_treated_as_aliases() {
    let consumer = "\
<?php
namespace App;
use App\\Models\\Comment;
class Consumer {
    public function go(): void {
        Comment::whereHasMorph('commentable', '*')->get();
        Comment::whereHasMorph('commentable', ['App\\\\Models\\\\Post'])->get();
    }
}
";
    let (backend, _dir, uri) = workspace(true, consumer).await;
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    assert!(
        morph_diagnostics(&diags).is_empty(),
        "`'*'` and class-name strings are not aliases, got {:?}",
        morph_diagnostics(&diags)
    );
}

#[tokio::test]
async fn morph_alias_completes_in_get_morphed_model() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Database\\Eloquent\\Relations\\Relation;
class Consumer {
    public function go(): void {
        Relation::getMorphedModel('');
    }
}
";
    let (backend, _dir, uri) = workspace(false, consumer).await;

    let position = position_after(consumer, "getMorphedModel('");
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(&uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    let labels: Vec<String> = match result {
        Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
        Some(CompletionResponse::List(list)) => list.items.into_iter().map(|i| i.label).collect(),
        None => Vec::new(),
    };
    assert!(
        labels.contains(&"post".to_string()) && labels.contains(&"video".to_string()),
        "both registered aliases should complete, got {labels:?}"
    );
}

#[tokio::test]
async fn list_shorthand_registration_keys_the_map_by_table_name() {
    // `Relation::morphMap([Post::class])` derives each alias from the model's
    // table, so the aliases here are `posts` and `videos`.
    const LIST_PROVIDER: &str = "\
<?php
namespace App\\Providers;
use App\\Models\\Post;
use App\\Models\\Video;
use Illuminate\\Database\\Eloquent\\Relations\\Relation;
use Illuminate\\Support\\ServiceProvider;
class AppServiceProvider extends ServiceProvider
{
    public function boot(): void
    {
        Relation::enforceMorphMap([Post::class, Video::class]);
    }
}
";
    let consumer = "\
<?php
namespace App;
use App\\Models\\Comment;
class Consumer {
    public function go(): void {
        Comment::whereHasMorph('commentable', ['posts', 'post'])->get();
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("src/Providers/AppServiceProvider.php", LIST_PROVIDER),
            ("src/Models/Post.php", POST_PHP),
            ("src/Models/Video.php", VIDEO_PHP),
            ("src/Models/Comment.php", COMMENT_PHP),
            ("src/Consumer.php", consumer),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("src/Consumer.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    let flagged = morph_diagnostics(&diags);
    assert_eq!(
        flagged.len(),
        1,
        "`posts` is registered and `post` is not, got {flagged:?}"
    );
    assert!(
        flagged[0].message.contains("'post'"),
        "the singular spelling should be the one flagged, got {:?}",
        flagged[0].message
    );
}

#[tokio::test]
async fn find_references_links_usages_to_the_registration() {
    let (backend, dir, uri) = workspace(false, WHERE_HAS_MORPH_CONSUMER).await;
    let provider_src = provider(false);
    let provider_uri = Url::from_file_path(dir.path().join("src/Providers/AppServiceProvider.php"))
        .unwrap()
        .to_string();
    open(&backend, &provider_uri, &provider_src).await;

    // Search from the registration's own alias key.
    let position = position_after(&provider_src, "'po");
    let locations = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(&provider_uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default();

    let uris: Vec<String> = locations.iter().map(|l| l.uri.to_string()).collect();
    assert!(
        uris.iter().any(|u| u == &uri),
        "the `whereHasMorph` usage should be found, got {uris:?}"
    );
    assert!(
        uris.iter().any(|u| u == &provider_uri),
        "the registration should be found, got {uris:?}"
    );
}
