//! Blade parity tests: each test runs the same assertion against both a
//! `.blade.php` file and its hand-translated PHP equivalent.  When a Blade
//! test fails but the PHP equivalent passes, the issue is in the
//! preprocessor or source-map translation.  When both fail, the issue is
//! in the shared resolution engine.

#[cfg(test)]
mod tests {
    use crate::common::{create_test_backend, open_document, open_php};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    // ── Scaffolding classes shared across all tests ─────────────────────

    const BLOG_POST_PHP: &str = r#"<?php namespace App\Models;
class BlogPost {
    public \Carbon\Carbon $created_at;
    public function getTitle(): string { return ''; }
    public function getSlug(): string { return ''; }
}"#;

    const BLOG_AUTHOR_PHP: &str =
        r#"<?php namespace App\Models; class BlogAuthor { public string $name = ''; }"#;

    const CARBON_PHP: &str = r#"<?php namespace Carbon;
class Carbon {
    public function diffForHumans(): string { return ''; }
}"#;

    async fn open_scaffolding(backend: &phpantom_lsp::Backend) {
        for (uri, text) in [
            ("file:///app/Models/BlogPost.php", BLOG_POST_PHP),
            ("file:///app/Models/BlogAuthor.php", BLOG_AUTHOR_PHP),
            ("file:///vendor/Carbon.php", CARBON_PHP),
        ] {
            open_php(backend, &Url::parse(uri).unwrap(), text).await;
        }
    }

    // ── The Blade template under test ───────────────────────────────────

    const BLADE_TEMPLATE: &str = r#"{{-- PHPantom Laravel Demo: Blog Post Published Email --}}
{{-- Demonstrates variable completion inside included partials --}}
@php
/**
 * @var \App\Models\BlogPost $post
 * @var \App\Models\BlogAuthor $author
 */
@endphp

<div>
    <h2>{{ __('messages.welcome') }}</h2>
    <p>{{ $author->name }}</p>
    <h3>{{ $post->getTitle() }}</h3>
    <p>{{ $post->getSlug() }}</p>
    <p>{{ $post->created_at->diffForHumans() }}</p>
    <p>{{ config('app.name') }} {{ date('Y') }}</p>
</div>
"#;

    // ── Hand-translated PHP equivalent (no @bladestan-signature) ─────────

    const PHP_EQUIVALENT: &str = r#"<?php
/**
 * @var \App\Models\BlogPost $post
 * @var \App\Models\BlogAuthor $author
 */
echo e( __('messages.welcome') );
echo e( $author->name );
echo e( $post->getTitle() );
echo e( $post->getSlug() );
echo e( $post->created_at->diffForHumans() );
echo e( config('app.name') ); echo e( date('Y') );
"#;

    // ── Helper: open either the Blade or PHP file ───────────────────────

    async fn open_blade(backend: &phpantom_lsp::Backend) -> String {
        let uri = "file:///resources/views/email.blade.php";
        open_document(backend, &Url::parse(uri).unwrap(), "blade", BLADE_TEMPLATE).await;
        uri.to_string()
    }

    async fn open_php_equivalent(backend: &phpantom_lsp::Backend) -> String {
        let uri = "file:///equivalent.php";
        open_php(backend, &Url::parse(uri).unwrap(), PHP_EQUIVALENT).await;
        uri.to_string()
    }

    // ── Helper: hover at a position and return the hover text ───────────

    async fn hover_text(
        backend: &phpantom_lsp::Backend,
        uri: &str,
        line: u32,
        col: u32,
    ) -> Option<String> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position: Position {
                    line,
                    character: col,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let result = backend.hover(params).await.unwrap()?;
        match result.contents {
            HoverContents::Markup(m) => Some(m.value),
            HoverContents::Scalar(MarkedString::String(s)) => Some(s),
            HoverContents::Scalar(MarkedString::LanguageString(ls)) => Some(ls.value),
            HoverContents::Array(arr) => Some(
                arr.into_iter()
                    .map(|m| match m {
                        MarkedString::String(s) => s,
                        MarkedString::LanguageString(ls) => ls.value,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        }
    }

    // ── Helper: completion labels at a position ─────────────────────────

    async fn completion_labels(
        backend: &phpantom_lsp::Backend,
        uri: &str,
        line: u32,
        col: u32,
    ) -> Vec<String> {
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position: Position {
                    line,
                    character: col,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(">".to_string()),
            }),
        };
        let result = backend.completion(params).await.unwrap();
        match result {
            Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
            Some(CompletionResponse::List(list)) => {
                list.items.into_iter().map(|i| i.label).collect()
            }
            None => vec![],
        }
    }

    // ── Helper: GTD returns something ───────────────────────────────────

    async fn has_definition(
        backend: &phpantom_lsp::Backend,
        uri: &str,
        line: u32,
        col: u32,
    ) -> bool {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position: Position {
                    line,
                    character: col,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        backend.goto_definition(params).await.unwrap().is_some()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Tests: each one runs against both the Blade file and PHP equivalent
    // ═══════════════════════════════════════════════════════════════════════

    // ── Hover $author shows BlogAuthor type ─────────────────────────────

    #[tokio::test]
    async fn test_hover_author_php() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_php_equivalent(&backend).await;
        // "$author" in "echo e( $author->name );" — line 6, col 8
        let text = hover_text(&backend, &uri, 6, 8).await;
        assert!(
            text.as_ref().is_some_and(|t| t.contains("BlogAuthor")),
            "PHP: hover on $author should mention BlogAuthor, got: {:?}",
            text
        );
    }

    #[tokio::test]
    async fn test_hover_author_blade() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_blade(&backend).await;
        // "$author" in "<p>{{ $author->name }}</p>" — line 11, col 10
        let text = hover_text(&backend, &uri, 11, 10).await;
        assert!(
            text.as_ref().is_some_and(|t| t.contains("BlogAuthor")),
            "Blade: hover on $author should mention BlogAuthor, got: {:?}",
            text
        );
    }

    // ── Completion $author-> offers 'name' ──────────────────────────────

    #[tokio::test]
    async fn test_completion_author_php() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        // Use a version with cursor after ->
        let uri_str = "file:///equiv_completion.php";
        let text =
            "<?php\n/**\n * @var \\App\\Models\\BlogAuthor $author\n */\necho e( $author-> );";
        open_php(&backend, &Url::parse(uri_str).unwrap(), text).await;
        // "echo e( $author-> );" — line 4, col 17
        let labels = completion_labels(&backend, uri_str, 4, 17).await;
        assert!(
            labels.iter().any(|l| l == "name"),
            "PHP: $author-> should offer 'name', got: {:?}",
            labels
        );
    }

    #[tokio::test]
    async fn test_completion_author_blade() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri_str = "file:///resources/views/comp.blade.php";
        let text = "@php\n/**\n * @var \\App\\Models\\BlogAuthor $author\n */\n@endphp\n\n<p>{{ $author-> }}</p>";
        open_document(&backend, &Url::parse(uri_str).unwrap(), "blade", text).await;
        // "<p>{{ $author-> }}</p>" — line 6, col 15
        let labels = completion_labels(&backend, uri_str, 6, 15).await;
        assert!(
            labels.iter().any(|l| l == "name"),
            "Blade: $author-> should offer 'name', got: {:?}",
            labels
        );
    }

    // ── GTD on ->name resolves to BlogAuthor::$name ─────────────────────

    #[tokio::test]
    async fn test_gtd_author_name_php() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_php_equivalent(&backend).await;
        // "name" in "echo e( $author->name );" — line 6, col 17
        let found = has_definition(&backend, &uri, 6, 17).await;
        assert!(found, "PHP: GTD on ->name should resolve");
    }

    #[tokio::test]
    async fn test_gtd_author_name_blade() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_blade(&backend).await;
        // "name" in "<p>{{ $author->name }}</p>" — line 11, col 19
        let found = has_definition(&backend, &uri, 11, 19).await;
        assert!(found, "Blade: GTD on ->name should resolve");
    }

    // ── Hover on $post shows BlogPost ───────────────────────────────────

    #[tokio::test]
    async fn test_hover_post_php() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_php_equivalent(&backend).await;
        // "$post" in "echo e( $post->getTitle() );" — line 7, col 8
        let text = hover_text(&backend, &uri, 7, 8).await;
        assert!(
            text.as_ref().is_some_and(|t| t.contains("BlogPost")),
            "PHP: hover on $post should mention BlogPost, got: {:?}",
            text
        );
    }

    #[tokio::test]
    async fn test_hover_post_blade() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_blade(&backend).await;
        // "$post" in "<h3>{{ $post->getTitle() }}</h3>" — line 12, col 11
        let text = hover_text(&backend, &uri, 12, 11).await;
        assert!(
            text.as_ref().is_some_and(|t| t.contains("BlogPost")),
            "Blade: hover on $post should mention BlogPost, got: {:?}",
            text
        );
    }

    // ── GTD on ->getTitle() resolves ────────────────────────────────────

    #[tokio::test]
    async fn test_gtd_get_title_php() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_php_equivalent(&backend).await;
        // "getTitle" in "echo e( $post->getTitle() );" — line 7, col 16
        let found = has_definition(&backend, &uri, 7, 16).await;
        assert!(found, "PHP: GTD on ->getTitle() should resolve");
    }

    #[tokio::test]
    async fn test_gtd_get_title_blade() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_blade(&backend).await;
        // "getTitle" in "<h3>{{ $post->getTitle() }}</h3>" — line 12, col 18
        let found = has_definition(&backend, &uri, 12, 18).await;
        assert!(found, "Blade: GTD on ->getTitle() should resolve");
    }

    // ── Chained access: $post->created_at->diffForHumans() ──────────────

    #[tokio::test]
    async fn test_hover_chained_php() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_php_equivalent(&backend).await;
        // "diffForHumans" in "echo e( $post->created_at->diffForHumans() );"
        // line 9, col 27
        let text = hover_text(&backend, &uri, 9, 27).await;
        assert!(
            text.as_ref()
                .is_some_and(|t| t.contains("diffForHumans") || t.contains("string")),
            "PHP: hover on ->diffForHumans() should show info, got: {:?}",
            text
        );
    }

    #[tokio::test]
    async fn test_hover_chained_blade() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_blade(&backend).await;
        // "diffForHumans" in "<p>{{ $post->created_at->diffForHumans() }}</p>"
        // line 14, col 30
        let text = hover_text(&backend, &uri, 14, 30).await;
        assert!(
            text.as_ref()
                .is_some_and(|t| t.contains("diffForHumans") || t.contains("string")),
            "Blade: hover on ->diffForHumans() should show info, got: {:?}",
            text
        );
    }

    // ── Foreach variable resolution ─────────────────────────────────────

    const AUTHOR_COLLECTION_PHP: &str = r#"<?php namespace App\Models;
use Illuminate\Database\Eloquent\Collection;
/**
 * @extends Collection<int, BlogAuthor>
 */
class AuthorCollection extends Collection {
    /** @return static */
    public function active(): static { return $this; }
    /** @return static */
    public function byName(): static { return $this; }
}"#;

    const BLOG_AUTHOR_FULL_PHP: &str = r#"<?php namespace App\Models;
class BlogAuthor {
    public string $name = '';
    public string $email = '';
    public ?string $role = null;
}"#;

    #[tokio::test]
    async fn test_foreach_var_php() {
        let backend = create_test_backend();
        for (uri, text) in [
            ("file:///app/Models/BlogAuthor.php", BLOG_AUTHOR_FULL_PHP),
            (
                "file:///app/Models/AuthorCollection.php",
                AUTHOR_COLLECTION_PHP,
            ),
        ] {
            open_php(&backend, &Url::parse(uri).unwrap(), text).await;
        }

        let php_uri = "file:///foreach_test.php";
        let php_text = r#"<?php
/** @var \App\Models\AuthorCollection $users */
foreach ($users->active()->byName() as $user) {
    echo $user->name;
}
"#;
        open_php(&backend, &Url::parse(php_uri).unwrap(), php_text).await;

        // Hover on $user (line 3, col 9)
        let text = hover_text(&backend, php_uri, 3, 9).await;
        assert!(
            text.as_ref().is_some_and(|t| t.contains("BlogAuthor")),
            "PHP: hover on $user in foreach should show BlogAuthor, got: {:?}",
            text
        );
    }

    #[tokio::test]
    async fn test_foreach_var_blade() {
        let backend = create_test_backend();
        for (uri, text) in [
            ("file:///app/Models/BlogAuthor.php", BLOG_AUTHOR_FULL_PHP),
            (
                "file:///app/Models/AuthorCollection.php",
                AUTHOR_COLLECTION_PHP,
            ),
        ] {
            open_php(&backend, &Url::parse(uri).unwrap(), text).await;
        }

        let blade_uri = "file:///views/users.blade.php";
        let blade_text = r#"@php
/**
 * @var \App\Models\AuthorCollection $users
 */
@endphp

@foreach($users->active()->byName() as $user)
    <p>{{ $user->name }}</p>
@endforeach
"#;
        open_document(
            &backend,
            &Url::parse(blade_uri).unwrap(),
            "blade",
            blade_text,
        )
        .await;

        // Hover on $user (line 7: "    <p>{{ $user->name }}</p>")
        let text = hover_text(&backend, blade_uri, 7, 14).await;
        assert!(
            text.as_ref().is_some_and(|t| t.contains("BlogAuthor")),
            "Blade: hover on $user in foreach should show BlogAuthor, got: {:?}",
            text
        );
    }

    #[tokio::test]
    async fn test_foreach_var_blade_with_bladestan_signature() {
        let backend = create_test_backend();
        for (uri, text) in [
            ("file:///app/Models/BlogAuthor.php", BLOG_AUTHOR_FULL_PHP),
            (
                "file:///app/Models/AuthorCollection.php",
                AUTHOR_COLLECTION_PHP,
            ),
        ] {
            open_php(&backend, &Url::parse(uri).unwrap(), text).await;
        }

        // This matches the exact real index.blade.php
        let blade_uri = "file:///views/admin/users/index.blade.php";
        let blade_text = r#"{{-- Demonstrates completion and navigation in nested Blade views --}}
@php
/**
 * @bladestan-signature
 * @var \App\Models\AuthorCollection $users
 */
@endphp

@extends('welcome')

@section('content')
    <h1>{{ __('messages.welcome') }} - Admin</h1>

    <table>
        <thead>
            <tr>
                <th>Name</th>
                <th>Email</th>
                <th>Role</th>
            </tr>
        </thead>
        <tbody>
            @foreach($users->active()->byName() as $user)
                <tr>
                    <td>{{ $user->name }}</td>
                    <td>{{ $user->email }}</td>
                    <td>{{ $user->role ?? 'N/A' }}</td>
                </tr>
            @endforeach
        </tbody>
    </table>

    @if($users->isEmpty())
        <p>{{ trans('pagination.next') }}</p>
    @endif
@endsection
"#;
        open_document(
            &backend,
            &Url::parse(blade_uri).unwrap(),
            "blade",
            blade_text,
        )
        .await;

        // Hover on $user on line 24 ("                    <td>{{ $user->name }}</td>")
        // $user starts at col 28
        let text = hover_text(&backend, blade_uri, 24, 30).await;
        assert!(
            text.as_ref().is_some_and(|t| t.contains("BlogAuthor")),
            "Blade(real file): hover on $user should show BlogAuthor, got: {:?}",
            text
        );
    }

    // ── Hover range is in Blade coordinate space ────────────────────────

    #[tokio::test]
    async fn test_hover_range_in_blade_space() {
        let backend = create_test_backend();
        open_scaffolding(&backend).await;
        let uri = open_blade(&backend).await;

        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(&uri).unwrap(),
                },
                position: Position {
                    line: 11,
                    character: 10,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let result = backend.hover(params).await.unwrap();
        assert!(result.is_some(), "Should return hover for $author");

        let hover = result.unwrap();
        if let Some(range) = hover.range {
            assert_eq!(
                range.start.line, 11,
                "Hover range start line should be in Blade space (11), got {}",
                range.start.line
            );
        }
    }
}
