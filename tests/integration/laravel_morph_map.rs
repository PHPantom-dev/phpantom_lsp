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
    "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\Database\\Eloquent\\": "framework/" } }
}"#;

pub(super) const MODEL_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
abstract class Model
{
    /** @return Builder<static> */
    public static function query() {}
    /** @return Builder<static> */
    public static function where($column, $operator = null, $value = null) {}
}
"#;

pub(super) const BUILDER_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
/** @template TModel of Model */
class Builder
{
    /** @return $this */
    public function where($column, $operator = null, $value = null) { return $this; }
}
"#;

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
    workspace_with_comment(enforce, consumer, COMMENT_PHP).await
}

async fn workspace_with_comment(
    enforce: bool,
    consumer: &str,
    comment: &str,
) -> (phpantom_lsp::Backend, tempfile::TempDir, String) {
    let provider_src = provider(enforce);
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("src/Providers/AppServiceProvider.php", &provider_src),
            ("src/Models/Post.php", POST_PHP),
            ("src/Models/Video.php", VIDEO_PHP),
            ("src/Models/Comment.php", comment),
            ("framework/Model.php", MODEL_PHP),
            ("framework/Builder.php", BUILDER_PHP),
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

#[tokio::test]
async fn morph_type_where_values_hover_on_models_and_typed_builders() {
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
use Illuminate\Database\Eloquent\Builder;
class Consumer {
    /** @param Builder<Comment> $query */
    public function go(Builder $query, Comment $comment): void {
        Comment::where('commentable_type', 'post');
        Comment::where('commentable_type', '=', 'post');
        $query->where('commentable_type', '!=', 'post');
        $comment->where('commentable_type', '<>', 'post');
        Comment::query()->where('commentable_type', 'post');
    }
}
"#;
    let (backend, _dir, uri) = workspace(false, consumer).await;

    for needle in [
        "Comment::where('commentable_type', 'po",
        "Comment::where('commentable_type', '=', 'po",
        "$query->where('commentable_type', '!=', 'po",
        "$comment->where('commentable_type', '<>', 'po",
        "Comment::query()->where('commentable_type', 'po",
    ] {
        let hover = hover_at(&backend, &uri, position_after(consumer, needle))
            .await
            .unwrap_or_else(|| panic!("morph alias should hover at {needle}"));
        assert!(
            hover.contains("App\\Models\\Post"),
            "alias should resolve to Post at {needle}, got {hover}"
        );
    }
}

#[tokio::test]
async fn morph_type_columns_resolve_builder_subclasses_and_inherited_table_names() {
    let comment = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\MorphTo;
class Comment extends Model {
    protected $table = 'comments';
    public function commentable(): MorphTo {
        return $this->morphTo();
    }
}
"#;
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Model;
/**
 * @template TRecord of Model
 * @extends Builder<TRecord>
 */
class CustomBuilder extends Builder {}
class SpecialComment extends Comment {}
class Consumer {
    /** @param CustomBuilder<Comment> $query */
    public function go(CustomBuilder $query): void {
        $query->where('commentable_type', 'post');
        $query->where('comments.commentable_type', 'post');
        SpecialComment::where('comments.commentable_type', 'post');
        $query->where('posts.commentable_type', 'post');
        SpecialComment::where('special_comments.commentable_type', 'post');
    }
}
"#;
    let (backend, _dir, uri) = workspace_with_comment(false, consumer, comment).await;

    for needle in [
        "$query->where('commentable_type', 'po",
        "$query->where('comments.commentable_type', 'po",
        "SpecialComment::where('comments.commentable_type', 'po",
    ] {
        let hover = hover_at(&backend, &uri, position_after(consumer, needle))
            .await
            .unwrap_or_else(|| panic!("inherited model metadata should resolve at {needle}"));
        assert!(hover.contains("App\\Models\\Post"), "got {hover}");
    }

    for needle in [
        "$query->where('posts.commentable_type', 'po",
        "SpecialComment::where('special_comments.commentable_type', 'po",
    ] {
        assert!(
            hover_at(&backend, &uri, position_after(consumer, needle))
                .await
                .is_none(),
            "only the inherited declared table may qualify the morph column at {needle}"
        );
    }
}

#[tokio::test]
async fn morph_type_queries_resolve_relative_class_names_inside_model_methods() {
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
class Consumer extends Comment {
    public function go(): void {
        self::where('commentable_type', 'post');
        static::where('commentable_type', 'post');
        parent::where('commentable_type', 'post');
        $this->where('commentable_type', 'post');
        $matches = $this->commentable_type === 'post';
    }
}
"#;
    let (backend, _dir, uri) = workspace(false, consumer).await;

    for needle in [
        "self::where('commentable_type', 'po",
        "static::where('commentable_type', 'po",
        "parent::where('commentable_type', 'po",
        "$this->where('commentable_type', 'po",
        "$this->commentable_type === 'po",
    ] {
        let hover = hover_at(&backend, &uri, position_after(consumer, needle))
            .await
            .unwrap_or_else(|| panic!("relative model receiver should resolve at {needle}"));
        assert!(hover.contains("App\\Models\\Post"), "got {hover}");
    }
}

#[tokio::test]
async fn morph_type_aliases_follow_the_current_type_after_local_reassignment() {
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
class Plain {
    public string $commentable_type = 'post';
    public function where(string $column, string $value): void {}
}
class Consumer {
    public function go(): void {
        $receiver = new Comment();
        $before = $receiver->commentable_type === 'post';
        $receiver->where('commentable_type', 'post');
        $receiver = new Plain();
        $after = $receiver->commentable_type === 'post';
        $receiver->where('commentable_type', 'video');
    }
}
"#;
    let (backend, _dir, uri) = workspace(false, consumer).await;

    for needle in [
        "$before = $receiver->commentable_type === 'po",
        "$receiver->where('commentable_type', 'po",
    ] {
        let hover = hover_at(&backend, &uri, position_after(consumer, needle))
            .await
            .unwrap_or_else(|| {
                panic!("model receiver should resolve before reassignment at {needle}")
            });
        assert!(hover.contains("App\\Models\\Post"), "got {hover}");
    }

    for needle in [
        "$after = $receiver->commentable_type === 'po",
        "$receiver->where('commentable_type', 'vi",
    ] {
        assert!(
            hover_at(&backend, &uri, position_after(consumer, needle))
                .await
                .is_none(),
            "non-model reassignment must stop morph alias recognition at {needle}"
        );
    }
}

#[tokio::test]
async fn morph_type_aliases_require_every_non_null_union_member_to_declare_the_column() {
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
use App\Models\Post;
interface Tagged {}
class SpecialComment extends Comment implements Tagged {}
class Consumer {
    public function go(
        ?Comment $nullable,
        Comment|SpecialComment|null $compatible,
        Comment|Post $incompatible,
        Comment&Tagged $intersection
    ): void {
        $optional = $nullable?->commentable_type === 'post';
        $nullable?->where('commentable_type', 'post');
        $union = $compatible?->commentable_type === 'post';
        $tagged = $intersection->commentable_type === 'post';
        $ambiguous = $incompatible->commentable_type === 'post';
        $incompatible->where('commentable_type', 'post');
    }
}
"#;
    let (backend, _dir, uri) = workspace(false, consumer).await;

    for needle in [
        "$nullable?->commentable_type === 'po",
        "$nullable?->where('commentable_type', 'po",
        "$compatible?->commentable_type === 'po",
        "$intersection->commentable_type === 'po",
    ] {
        let hover = hover_at(&backend, &uri, position_after(consumer, needle))
            .await
            .unwrap_or_else(|| panic!("known morph column should resolve at {needle}"));
        assert!(hover.contains("App\\Models\\Post"), "got {hover}");
    }

    for needle in [
        "$incompatible->commentable_type === 'po",
        "$incompatible->where('commentable_type', 'po",
    ] {
        assert!(
            hover_at(&backend, &uri, position_after(consumer, needle))
                .await
                .is_none(),
            "a union member without the morph relation must prevent recognition at {needle}"
        );
    }
}

#[tokio::test]
async fn morph_type_queries_resolve_function_returns_and_property_receivers() {
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
use Illuminate\Database\Eloquent\Builder;
/** @return Builder<Comment> */
function commentQuery(): Builder { return Comment::query(); }
class QueryHolder {
    /** @var Builder<Comment> */
    public Builder $query;
}
class Consumer {
    public function go(QueryHolder $holder): void {
        commentQuery()->where('commentable_type', 'post');
        $holder->query->where('commentable_type', 'post');
    }
}
"#;
    let (backend, _dir, uri) = workspace(false, consumer).await;

    for needle in [
        "commentQuery()->where('commentable_type', 'po",
        "$holder->query->where('commentable_type', 'po",
    ] {
        let hover = hover_at(&backend, &uri, position_after(consumer, needle))
            .await
            .unwrap_or_else(|| panic!("builder expression should retain its model at {needle}"));
        assert!(hover.contains("App\\Models\\Post"), "got {hover}");
    }
}

#[tokio::test]
async fn inherited_get_table_override_prevents_guessing_qualified_morph_columns() {
    let comment = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\MorphTo;
abstract class DynamicTableModel extends Model {
    public function getTable(): string { return 'comments_' . date('Y'); }
}
class Comment extends DynamicTableModel {
    protected $table = 'comments';
    public function commentable(): MorphTo {
        return $this->morphTo();
    }
}
"#;
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
class Consumer {
    public function go(Comment $comment): void {
        Comment::where('commentable_type', 'post');
        $comment->where('commentable_type', 'post');
        Comment::where('comments.commentable_type', 'post');
        $comment->where('comments.commentable_type', 'post');
    }
}
"#;
    let (backend, _dir, uri) = workspace_with_comment(false, consumer, comment).await;

    for needle in [
        "Comment::where('commentable_type', 'po",
        "$comment->where('commentable_type', 'po",
    ] {
        let hover = hover_at(&backend, &uri, position_after(consumer, needle))
            .await
            .unwrap_or_else(|| {
                panic!("declared unqualified morph column should resolve at {needle}")
            });
        assert!(hover.contains("App\\Models\\Post"), "got {hover}");
    }

    for needle in [
        "Comment::where('comments.commentable_type', 'po",
        "$comment->where('comments.commentable_type', 'po",
    ] {
        assert!(
            hover_at(&backend, &uri, position_after(consumer, needle))
                .await
                .is_none(),
            "inherited getTable() overrides the child's literal table at {needle}"
        );
    }
}

#[tokio::test]
async fn morph_type_property_comparisons_recognize_both_operand_orders() {
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
class Consumer {
    public function go(Comment $comment): void {
        $strict = $comment->commentable_type === 'post';
        $loose = $comment->commentable_type == 'post';
        $different = $comment->commentable_type !== 'post';
        $unequal = $comment->commentable_type != 'post';
        $alternate = $comment->commentable_type <> 'post';
        $reversed = 'post' === $comment->commentable_type;
        $reversedUnequal = 'post' != $comment->commentable_type;
    }
}
"#;
    let (backend, _dir, uri) = workspace(false, consumer).await;

    for needle in [
        "commentable_type === 'po",
        "commentable_type == 'po",
        "commentable_type !== 'po",
        "commentable_type != 'po",
        "commentable_type <> 'po",
        "$reversed = 'po",
        "$reversedUnequal = 'po",
    ] {
        let hover = hover_at(&backend, &uri, position_after(consumer, needle))
            .await
            .unwrap_or_else(|| panic!("comparison alias should hover at {needle}"));
        assert!(
            hover.contains("App\\Models\\Post"),
            "comparison should resolve to Post at {needle}, got {hover}"
        );
    }
}

#[tokio::test]
async fn morph_type_columns_follow_custom_names_inherited_methods_and_traits() {
    let comment = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\MorphTo;
trait HasOwner {
    public function ownedBy(): MorphTo {
        return $this->morphTo('owner');
    }
}
abstract class BaseComment extends Model {
    public function commentable(): MorphTo {
        return $this->morphTo('commentable', 'target_kind', 'target_id');
    }
}
class Comment extends BaseComment {
    use HasOwner;
    public function attachment(): MorphTo {
        return $this->morphTo(type: 'attachment_kind', name: 'attachment');
    }
}
"#;
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
class Consumer {
    public function go(Comment $comment): void {
        Comment::where('target_kind', 'post');
        $inherited = $comment->target_kind === 'post';
        Comment::where('owner_type', 'post');
        $trait = $comment->owner_type === 'post';
        Comment::where('attachment_kind', 'post');
        $named = $comment->attachment_kind === 'post';
        Comment::where('commentable_type', 'post');
        $oldDefault = $comment->commentable_type === 'post';
        Comment::where('owned_by_type', 'post');
    }
}
"#;
    let (backend, _dir, uri) = workspace_with_comment(false, consumer, comment).await;

    for needle in [
        "where('target_kind', 'po",
        "target_kind === 'po",
        "where('owner_type', 'po",
        "owner_type === 'po",
        "where('attachment_kind', 'po",
        "attachment_kind === 'po",
    ] {
        let hover = hover_at(&backend, &uri, position_after(consumer, needle))
            .await
            .unwrap_or_else(|| panic!("declared morph type column should resolve at {needle}"));
        assert!(hover.contains("App\\Models\\Post"), "got {hover}");
    }

    for needle in [
        "where('commentable_type', 'po",
        "commentable_type === 'po",
        "where('owned_by_type', 'po",
    ] {
        assert!(
            hover_at(&backend, &uri, position_after(consumer, needle))
                .await
                .is_none(),
            "explicit morphTo arguments must replace the inferred column at {needle}"
        );
    }
}

#[tokio::test]
async fn morph_type_columns_require_a_relation_on_an_eloquent_receiver() {
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
use App\Models\Post;
class Plain {
    public string $commentable_type;
    public function commentable() { return $this->morphTo(); }
    public function morphTo() {}
    public static function where($column, $operator = null, $value = null) {}
}
class Consumer {
    public function go(Comment $comment, Post $post, Plain $plain): void {
        Comment::where('document_type', 'post');
        $ordinary = $comment->document_type === 'post';
        Post::where('commentable_type', 'post');
        $otherModel = $post->commentable_type === 'post';
        Plain::where('commentable_type', 'post');
        $plain->where('commentable_type', 'post');
        $lookalike = $plain->commentable_type === 'post';
        Comment::where('commentable_type', 'like', 'post');
        $ordering = $comment->commentable_type > 'post';
    }
}
"#;
    let (backend, _dir, uri) = workspace(true, consumer).await;

    for needle in [
        "Comment::where('document_type', 'po",
        "document_type === 'po",
        "Post::where('commentable_type', 'po",
        "$post->commentable_type === 'po",
        "Plain::where('commentable_type', 'po",
        "$plain->where('commentable_type', 'po",
        "$plain->commentable_type === 'po",
        "where('commentable_type', 'like', 'po",
        "commentable_type > 'po",
    ] {
        assert!(
            hover_at(&backend, &uri, position_after(consumer, needle))
                .await
                .is_none(),
            "unrelated string must not be recognized as a morph alias at {needle}"
        );
        assert!(
            definition_uris(&backend, &uri, position_after(consumer, needle))
                .await
                .is_empty(),
            "unrelated string must not navigate to a morph registration at {needle}"
        );
    }
}

#[tokio::test]
async fn morph_type_column_diagnostics_flag_only_unregistered_comparison_values() {
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
class Consumer {
    public function go(Comment $comment): void {
        Comment::where('commentable_type', 'post');
        Comment::where('commentable_type', 'audio');
        Comment::where('commentable_type', '!=', 'missing');
        $bad = $comment->commentable_type === 'unknown';
        $reverse = 'unmapped' !== $comment->commentable_type;
        Comment::where('ordinary_type', 'unregistered');
        $ordinary = $comment->ordinary_type === 'unregistered';
        Comment::where('commentable_type', 'like', '%post%');
    }
}
"#;

    for enforce in [false, true] {
        let (backend, _dir, uri) = workspace(enforce, consumer).await;
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
        let flagged = morph_diagnostics(&diags);
        if enforce {
            let expected = ["'au", "'mi", "'un", "$reverse = 'un"];
            assert_eq!(flagged.len(), expected.len(), "got {flagged:?}");
            for needle in expected {
                let position = position_after(consumer, needle);
                assert!(
                    flagged.iter().any(|diagnostic| {
                        diagnostic.range.start <= position && position < diagnostic.range.end
                    }),
                    "unregistered alias at {needle} should be flagged, got {flagged:?}"
                );
            }
        } else {
            assert!(
                flagged.is_empty(),
                "open morph map should accept values: {flagged:?}"
            );
        }
    }
}

#[tokio::test]
async fn morph_type_column_navigation_and_references_share_the_registered_alias() {
    let consumer = r#"<?php
namespace App;
use App\Models\Comment;
class Consumer {
    public function go(Comment $comment): void {
        Comment::where('commentable_type', 'post');
        $matches = $comment->commentable_type === 'post';
        Comment::whereHasMorph('commentable', ['post']);
        Comment::where('ordinary_type', 'post');
    }
}
"#;
    let (backend, dir, uri) = workspace(false, consumer).await;
    let provider_src = provider(false);
    let provider_uri = Url::from_file_path(dir.path().join("src/Providers/AppServiceProvider.php"))
        .unwrap()
        .to_string();
    open(&backend, &provider_uri, &provider_src).await;

    let usages = [
        "where('commentable_type', 'po",
        "commentable_type === 'po",
        "['po",
    ];
    for needle in &usages[..2] {
        let targets = definition_uris(&backend, &uri, position_after(consumer, needle)).await;
        assert!(
            targets.iter().any(|target| target == &provider_uri),
            "column alias should navigate to registration at {needle}, got {targets:?}"
        );
        assert!(
            targets
                .iter()
                .any(|target| target.ends_with("/Models/Post.php")),
            "column alias should navigate to mapped model at {needle}, got {targets:?}"
        );
    }

    for (source_uri, position) in [
        (&provider_uri, position_after(&provider_src, "'po")),
        (&uri, position_after(consumer, usages[0])),
        (&uri, position_after(consumer, usages[1])),
    ] {
        let locations = backend
            .references(ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::parse(source_uri).unwrap(),
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
        assert!(
            locations
                .iter()
                .any(|location| location.uri.as_str() == provider_uri),
            "references should include registration, got {locations:?}"
        );
        let usage_lines: Vec<u32> = locations
            .iter()
            .filter(|location| location.uri.as_str() == uri)
            .map(|location| location.range.start.line)
            .collect();
        assert_eq!(usage_lines.len(), usages.len(), "got {locations:?}");
        for needle in usages {
            assert!(
                usage_lines.contains(&position_after(consumer, needle).line),
                "references should include {needle}, got {locations:?}"
            );
        }
    }
}
