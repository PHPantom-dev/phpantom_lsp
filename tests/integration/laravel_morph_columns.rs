//! Morph-map aliases used as values of a model's actual polymorphic type columns.

use crate::common::{create_psr4_workspace, open_php};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^12.0" },
    "autoload": { "psr-4": { "App\\": "app/", "Illuminate\\Database\\Eloquent\\": "framework/" } }
}"#;

const COMMENT: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\MorphTo;
class Comment extends Model {
    public function commentable(): MorphTo { return $this->morphTo(); }
}
"#;

const ATTACHMENT: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Relations\MorphTo;
class Attachment extends Comment {
    use HasSubject;
    public function imageOwner(): MorphTo { return $this->morphTo(); }
    public function owner(): MorphTo {
        return $this->morphTo(type: 'owner_kind', name: 'asset');
    }
    public function renamed(): MorphTo { return $this->morphTo('media'); }
}
"#;

const SUBJECT: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Relations\MorphTo;
trait HasSubject {
    public function subject(): MorphTo {
        return $this->morphTo('subject', 'subject_kind');
    }
}
"#;

const OTHER: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class Other extends Model {
    public string $commentable_type;
    public string $owner_kind;
}
"#;

fn provider(enforced: bool) -> String {
    let method = if enforced {
        "enforceMorphMap"
    } else {
        "morphMap"
    };
    format!(
        r#"<?php
namespace App\Providers;
use App\Models\Post;
use App\Models\Video;
use Illuminate\Database\Eloquent\Relations\Relation;
use Illuminate\Support\ServiceProvider;
class AppServiceProvider extends ServiceProvider {{
    public function boot(): void {{
        Relation::{method}(['post' => Post::class, 'video' => Video::class]);
    }}
}}
"#
    )
}

fn consumer(body: &str) -> String {
    format!(
        r#"<?php
namespace App;
use App\Models\Comment;
use App\Models\Attachment;
use App\Models\Other;
use Illuminate\Database\Eloquent\Builder;
class Consumer {{
    /** @param Builder<Comment> $query */
    public function run(Comment $comment, Attachment $attachment, Other $other, Builder $query, OtherQuery $ordinary, \Illuminate\Database\Query\Builder $sql): void {{
        {body}
    }}
}}
"#
    )
}

async fn workspace(source: &str, enforced: bool) -> (Backend, tempfile::TempDir, Url) {
    let registration = provider(enforced);
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("framework/Model.php", super::laravel_morph_map::MODEL_PHP),
            (
                "framework/Builder.php",
                super::laravel_morph_map::BUILDER_PHP,
            ),
            (
                "bootstrap/providers.php",
                "<?php return [App\\Providers\\AppServiceProvider::class];",
            ),
            ("app/Providers/AppServiceProvider.php", &registration),
            ("app/Models/Comment.php", COMMENT),
            ("app/Models/Attachment.php", ATTACHMENT),
            ("app/Models/HasSubject.php", SUBJECT),
            ("app/Models/Other.php", OTHER),
            (
                "app/OtherQuery.php",
                "<?php namespace App; class OtherQuery { public string $commentable_type; public function where(string $column, string $value): self { return $this; } }",
            ),
            (
                "app/Models/Post.php",
                "<?php namespace App\\Models; class Post extends \\Illuminate\\Database\\Eloquent\\Model {}",
            ),
            (
                "app/Models/Video.php",
                "<?php namespace App\\Models; class Video extends \\Illuminate\\Database\\Eloquent\\Model {}",
            ),
            (
                "app/ColdConsumer.php",
                r#"<?php
namespace App;
use App\Models\Comment;
class ColdConsumer {
    public function run(Comment $comment): void {
        Comment::where('commentable_type', 'post');
        if ($comment->commentable_type === 'post') {}
        Comment::whereHasMorph('commentable', ['post']);
    }
}
"#,
            ),
            ("app/Consumer.php", source),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/Consumer.php")).unwrap();
    open_php(&backend, &uri, source).await;
    (backend, dir, uri)
}

fn position_after(source: &str, prefix: &str) -> Position {
    let end = source
        .find(prefix)
        .unwrap_or_else(|| panic!("missing {prefix}"))
        + prefix.len();
    let before = &source[..end];
    Position::new(
        before.bytes().filter(|byte| *byte == b'\n').count() as u32,
        before.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
    )
}

async fn labels(backend: &Backend, uri: &Url, position: Position) -> Vec<String> {
    let response = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    match response {
        Some(CompletionResponse::Array(items)) => {
            items.into_iter().map(|item| item.label).collect()
        }
        Some(CompletionResponse::List(list)) => {
            list.items.into_iter().map(|item| item.label).collect()
        }
        None => Vec::new(),
    }
}

async fn hover(backend: &Backend, uri: &Url, position: Position) -> Option<String> {
    let result = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()?;
    match result.contents {
        HoverContents::Markup(markup) => Some(markup.value),
        contents => Some(format!("{contents:?}")),
    }
}

fn morph_diagnostics(backend: &Backend, uri: &Url, source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    diagnostics
        .into_iter()
        .filter(|diagnostic| {
            matches!(&diagnostic.code, Some(NumberOrString::String(code)) if code == "invalid_laravel_morph_alias")
        })
        .collect()
}

#[tokio::test]
async fn model_and_typed_builder_where_values_complete_and_hover() {
    let statements = [
        "Comment::where('commentable_type', 'post');",
        "Comment::query()->where('commentable_type', '=', 'post');",
        "$query->where('commentable_type', '!=', 'post');",
        "$comment->orWhere('commentable_type', '<>', 'post');",
        "$query->orWhere(column: 'commentable_type', operator: 'post');",
        "Comment::where(value: 'post', operator: '=', column: 'commentable_type');",
        "$query->whereIn('commentable_type', ['post', 'video']);",
        "$query->orWhereIn('commentable_type', array('post'));",
        "Comment::whereNotIn('commentable_type', ['post']);",
        "$query->orWhereNotIn(values: ['post'], column: 'commentable_type');",
    ];
    let source = consumer(&statements.join("\n"));
    let (backend, _dir, uri) = workspace(&source, false).await;
    for statement in statements {
        let prefix = &statement[..statement.find("'post").unwrap() + 3];
        let position = position_after(&source, prefix);
        assert_eq!(
            labels(&backend, &uri, position).await,
            ["post"],
            "{statement}"
        );
        let text = hover(&backend, &uri, position).await.expect(statement);
        assert!(text.contains("App\\Models\\Post"), "{statement}: {text}");
    }
}

#[tokio::test]
async fn property_equality_aliases_work_in_both_directions_and_with_nullsafe_access() {
    let statements = [
        "if ($comment->commentable_type === 'post') {}",
        "if ($comment->commentable_type == 'post') {}",
        "if ($comment->commentable_type !== 'post') {}",
        "if ($comment->commentable_type != 'post') {}",
        "if ('post' === $comment->commentable_type) {}",
        "if ('post' == $comment->commentable_type) {}",
        "if ('post' !== $comment->commentable_type) {}",
        "if ('post' != $comment->commentable_type) {}",
        "if ($comment?->commentable_type === 'post') {}",
        "if ('post' !== $comment?->commentable_type) {}",
    ];
    let source = consumer(&statements.join("\n"));
    let (backend, _dir, uri) = workspace(&source, false).await;
    for statement in statements {
        let start = source.find(statement).unwrap();
        let prefix = &source[..start + statement.find("'post").unwrap() + 3];
        let position = position_after(&source, prefix);
        assert_eq!(
            labels(&backend, &uri, position).await,
            ["post"],
            "{statement}"
        );
        assert!(
            hover(&backend, &uri, position)
                .await
                .unwrap()
                .contains("App\\Models\\Post"),
            "{statement}"
        );
    }
}

#[tokio::test]
async fn empty_literals_and_relation_declared_custom_columns_complete() {
    let statements = [
        "Comment::where('commentable_type', '');",
        "if ($comment->commentable_type === '') {}",
        "if ('' === $comment->commentable_type) {}",
        "Attachment::where('commentable_type', '');",
        "Attachment::where('image_owner_type', '');",
        "Attachment::where('owner_kind', '');",
        "Attachment::where('media_type', '');",
        "Attachment::where('subject_kind', '');",
        "if ($attachment->owner_kind === '') {}",
        "$query->whereIn('commentable_type', ['']);",
    ];
    let source = consumer(&statements.join("\n"));
    let (backend, _dir, uri) = workspace(&source, false).await;
    for statement in statements {
        let prefix = &statement[..statement.find("''").unwrap() + 1];
        let mut values = labels(&backend, &uri, position_after(&source, prefix)).await;
        values.sort_unstable();
        assert_eq!(values, ["post", "video"], "{statement}");
    }
}

#[tokio::test]
async fn lookalike_columns_receivers_and_non_literal_expressions_are_not_aliases() {
    let statements = [
        "Other::where('commentable_type', 'post');",
        "Comment::where('owner_kind', 'post');",
        "Attachment::where('owner_type', 'post');",
        "Attachment::where('imageOwner_type', 'post');",
        "Comment::where('status_type', 'post');",
        "$query->where('commentable_type', 'like', 'post');",
        "$query->where('commentable_type', '>', 'post');",
        "$query->where('commentable_type', '<=', 'post');",
        "$query->where('commentable_type', $operator, 'post');",
        "$query->where($column, 'post');",
        "$query->where('commentable_' . 'type', 'post');",
        "$query->where('commentable_type', 'post' . $suffix);",
        "$query->whereRaw('commentable_type', 'post');",
        "$query->whereIn('commentable_type', [['post']]);",
        "\\Illuminate\\Support\\Facades\\DB::table('comments')->where('commentable_type', 'post');",
        "$unknown->where('commentable_type', 'post');",
        "$ordinary->where('commentable_type', 'post');",
        "$sql->where('commentable_type', 'post');",
        "if ($other->commentable_type === 'post') {}",
        "if ($ordinary->commentable_type === 'post') {}",
        "if ($comment->owner_kind === 'post') {}",
        "if ($comment->commentable_type > 'post') {}",
        "if ($comment->commentable_type === 'post' . $suffix) {}",
        "if ($comment->{$column} === 'post') {}",
        "// $query->where('commentable_type', 'post');",
        "$example = \"$query->where('commentable_type', 'post')\";",
    ];
    let source = consumer(&statements.join("\n"));
    let (backend, _dir, uri) = workspace(&source, true).await;
    for statement in statements {
        let start = source.find(statement).unwrap();
        let prefix = &source[..start + statement.find("'post").unwrap() + 3];
        let position = position_after(&source, prefix);
        let values = labels(&backend, &uri, position).await;
        assert!(
            !values
                .iter()
                .any(|value| value == "post" || value == "video"),
            "{statement}: {values:?}"
        );
        assert!(
            hover(&backend, &uri, position)
                .await
                .is_none_or(|text| !text.contains("Morph type")),
            "{statement}"
        );
    }
    let unknowns = source.replace("'post'", "'missing'");
    open_php(&backend, &uri, &unknowns).await;
    assert!(morph_diagnostics(&backend, &uri, &unknowns).is_empty());
}

#[tokio::test]
async fn comparisons_diagnose_only_unknown_aliases_under_an_enforced_map() {
    let source = consumer(
        r#"
Comment::where('commentable_type', 'post');
Comment::where('commentable_type', 'missing-query');
$query->whereIn('commentable_type', ['video', 'missing-array']);
if ($comment->commentable_type !== 'missing-property') {}
if ('missing-reversed' === $comment->commentable_type) {}
Comment::where('commentable_type', 'App\\Models\\Post');
Other::where('commentable_type', 'ordinary');
"#,
    );
    for enforced in [false, true] {
        let (backend, _dir, uri) = workspace(&source, enforced).await;
        let diagnostics = morph_diagnostics(&backend, &uri, &source);
        assert_eq!(
            diagnostics.len(),
            if enforced { 4 } else { 0 },
            "{diagnostics:?}"
        );
        if enforced {
            for missing in [
                "missing-query",
                "missing-array",
                "missing-property",
                "missing-reversed",
            ] {
                assert!(
                    diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic.message.contains(missing)),
                    "{diagnostics:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn alias_navigation_links_model_registration_and_cold_cross_file_usages() {
    let source = consumer("Comment::where('commentable_type', 'post');");
    let (backend, dir, uri) = workspace(&source, false).await;
    let position = position_after(&source, "'commentable_type', 'po");
    let response = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("alias definition");
    let destinations: Vec<Url> = match response {
        GotoDefinitionResponse::Scalar(location) => vec![location.uri],
        GotoDefinitionResponse::Array(locations) => {
            locations.into_iter().map(|location| location.uri).collect()
        }
        GotoDefinitionResponse::Link(links) => {
            links.into_iter().map(|link| link.target_uri).collect()
        }
    };
    for suffix in ["/Providers/AppServiceProvider.php", "/Models/Post.php"] {
        assert!(
            destinations
                .iter()
                .any(|target| target.path().ends_with(suffix)),
            "{destinations:?}"
        );
    }
    let registration_uri =
        Url::from_file_path(dir.path().join("app/Providers/AppServiceProvider.php")).unwrap();
    let cold_uri = Url::from_file_path(dir.path().join("app/ColdConsumer.php")).unwrap();
    let references = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
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
    assert_eq!(
        references
            .iter()
            .filter(|location| location.uri == cold_uri)
            .count(),
        3,
        "{references:?}"
    );
    assert_eq!(
        references
            .iter()
            .filter(|location| location.uri == uri)
            .count(),
        1,
        "{references:?}"
    );
    assert!(
        references
            .iter()
            .any(|location| location.uri == registration_uri),
        "{references:?}"
    );
}

#[tokio::test]
async fn same_length_model_edits_refresh_cross_file_column_aliases() {
    let source = consumer(
        "Comment::where('commentable_type', 'post');\nComment::where('mentionable_type', 'post');",
    );
    let (backend, dir, uri) = workspace(&source, false).await;
    let old_position = position_after(&source, "'commentable_type', 'po");
    let new_position = position_after(&source, "'mentionable_type', 'po");
    assert_eq!(labels(&backend, &uri, old_position).await, ["post"]);
    assert!(
        !labels(&backend, &uri, new_position)
            .await
            .iter()
            .any(|label| label == "post")
    );

    let model_uri = Url::from_file_path(dir.path().join("app/Models/Comment.php")).unwrap();
    open_php(&backend, &model_uri, COMMENT).await;
    let edited = COMMENT.replace("commentable", "mentionable");
    assert_eq!(edited.len(), COMMENT.len());
    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: model_uri,
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: edited,
            }],
        })
        .await;
    assert!(
        !labels(&backend, &uri, old_position)
            .await
            .iter()
            .any(|label| label == "post")
    );
    assert_eq!(labels(&backend, &uri, new_position).await, ["post"]);
    assert!(hover(&backend, &uri, old_position).await.is_none());
    assert!(
        hover(&backend, &uri, new_position)
            .await
            .unwrap()
            .contains("App\\Models\\Post")
    );
}

#[tokio::test]
async fn morph_aliases_resolve_a_builder_subclass_with_the_same_short_name() {
    let source = r#"<?php
namespace App;
use Illuminate\Database\Eloquent\Builder as EloquentBuilder;
/**
 * @template TUnused
 * @template TRecord of \Illuminate\Database\Eloquent\Model
 * @extends EloquentBuilder<TRecord>
 */
class Builder extends EloquentBuilder {}
/** @extends EloquentBuilder<\App\Models\Comment> */
class FixedBuilder extends EloquentBuilder {}
class Consumer {
    /** @param Builder<int, \App\Models\Comment> $query */
    public function run(Builder $query, FixedBuilder $fixed): void {
        $query->where('commentable_type', 'post');
        $fixed->where('commentable_type', 'post');
    }
}
"#;
    let (backend, _dir, uri) = workspace(source, false).await;
    let position = position_after(source, "'commentable_type', 'po");
    assert_eq!(labels(&backend, &uri, position).await, ["post"]);
    assert!(
        hover(&backend, &uri, position)
            .await
            .unwrap()
            .contains("App\\Models\\Post")
    );
    assert_eq!(
        labels(
            &backend,
            &uri,
            position_after(source, "$fixed->where('commentable_type', 'po")
        )
        .await,
        ["post"]
    );
}
