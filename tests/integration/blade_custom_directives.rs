//! Directives a project's own service providers register.
//!
//! `Blade::directive('datetime', …)` and `Blade::if('admin', …)` declare
//! directives Blade knows nothing about until the provider runs. Until the
//! provider scan reads them the preprocessor masks every use of them, which
//! costs the template both the type-checking of the argument it passes and
//! any completion of the directive's name.

#[cfg(test)]
mod tests {
    use crate::common::{
        LARAVEL_APP_COMPOSER, completion_response, create_psr4_workspace, open_document, open_php,
        template_diagnostics,
    };
    use phpantom_lsp::Backend;
    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::gen_lsp_types::*;

    const PROVIDERS_PHP: &str =
        "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n";

    const PROVIDER: &str = r#"<?php
namespace App\Providers;

use Illuminate\Support\Facades\Blade;

class AppServiceProvider
{
    public function boot(): void
    {
        Blade::directive('datetime', function ($expression) {
            return "<?php echo ($expression)->format('Y-m-d'); ?>";
        });

        Blade::if('admin', function () {
            return true;
        });
    }
}
"#;

    const POST_PHP: &str = r#"<?php
namespace App;

class Post
{
    public string $title = '';

    public function publishedAt(): string
    {
        return '';
    }
}
"#;

    const TEMPLATE: &str = "resources/views/page.blade.php";

    /// A workspace whose provider registers one `Blade::directive()` and one
    /// `Blade::if()`, with `page.blade.php` holding `template`.
    async fn start(template: &str) -> (Backend, tempfile::TempDir, Uri) {
        let (backend, dir) = create_psr4_workspace(
            LARAVEL_APP_COMPOSER,
            &[
                ("bootstrap/providers.php", PROVIDERS_PHP),
                ("app/Providers/AppServiceProvider.php", PROVIDER),
                ("app/Post.php", POST_PHP),
                (TEMPLATE, template),
            ],
        );
        backend.initialized(InitializedParams {}).await;

        let uri = Uri::from_file_path(dir.path().join(TEMPLATE)).unwrap();
        open_document(&backend, &uri, "blade", template).await;
        (backend, dir, uri)
    }

    /// The registered directive's argument is real PHP, so a member the
    /// class it names does not have is reported the same as anywhere else.
    #[tokio::test]
    async fn a_registered_directives_argument_is_type_checked() {
        let (backend, _dir, uri) = start(
            "@php($post = new \\App\\Post())\n\
             <p>@datetime($post->publishedAt())</p>\n",
        )
        .await;
        assert!(
            template_diagnostics(&backend, &uri).is_empty(),
            "a correct argument reports nothing: {:?}",
            template_diagnostics(&backend, &uri)
        );

        let (backend, _dir, uri) = start(
            "@php($post = new \\App\\Post())\n\
             <p>@datetime($post->noSuchMethod())</p>\n",
        )
        .await;
        let reported = template_diagnostics(&backend, &uri);
        assert!(
            reported.iter().any(|m| m.contains("noSuchMethod")),
            "the argument's unknown member should be reported: {reported:?}"
        );
    }

    /// `Blade::if('admin')` gives the template `@admin`, `@unlessadmin`,
    /// `@elseadmin` and `@endadmin`, and the block they form has to come out
    /// of the preprocessor as balanced PHP or the whole template stops
    /// parsing.
    #[tokio::test]
    async fn a_registered_condition_family_is_balanced() {
        let (backend, _dir, uri) = start(
            "@admin\n\
             <p>admin</p>\n\
             @elseadmin\n\
             <p>other</p>\n\
             @endadmin\n\
             @unlessadmin\n\
             <p>nobody</p>\n\
             @endadmin\n",
        )
        .await;
        let reported = template_diagnostics(&backend, &uri);
        assert!(
            reported.is_empty(),
            "the condition family must not break the template: {reported:?}"
        );

        // What follows the block is still analysed, so the condition did not
        // swallow the rest of the template on its way to a closing paren.
        let (backend, _dir, uri) = start(
            "@admin\n\
             <p>admin</p>\n\
             @endadmin\n\
             @php($post = new \\App\\Post())\n\
             @php($post->noSuchMethod())\n",
        )
        .await;
        let reported = template_diagnostics(&backend, &uri);
        assert!(
            reported.iter().any(|m| m.contains("noSuchMethod")),
            "the template after the block is still PHP: {reported:?}"
        );
    }

    /// A registered directive is offered when the name is being typed, so it
    /// is as discoverable as Blade's own.
    #[tokio::test]
    async fn registered_directives_are_offered_as_completions() {
        let (backend, _dir, uri) = start("<div>\n@\n</div>\n").await;
        let response = completion_response(&backend, &uri, 1, 1, None)
            .await
            .expect("directive completion should answer");
        let items = match response {
            CompletionResponse::CompletionItemList(items) => items,
            CompletionResponse::CompletionList(list) => list.items,
        };
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();

        for expected in [
            "@datetime",
            "@admin",
            "@unlessadmin",
            "@elseadmin",
            "@endadmin",
        ] {
            assert!(
                labels.contains(&expected),
                "{expected} should be offered: {labels:?}"
            );
        }
        // Blade's own directives are still there alongside them.
        assert!(
            labels.contains(&"@if"),
            "core directives remain: {labels:?}"
        );
    }

    /// A directive registered now applies now: the templates already
    /// preprocessed are re-read against the set the edit produced, which is
    /// also what makes the startup order work (providers are scanned after
    /// the workspace index has preprocessed every template).
    #[tokio::test]
    async fn registering_a_directive_re_reads_the_open_templates() {
        let template = "@php($post = new \\App\\Post())\n\
                        <p>@money($post->noSuchMethod())</p>\n";
        let (backend, dir, uri) = start(template).await;
        assert!(
            template_diagnostics(&backend, &uri).is_empty(),
            "an unregistered @money masks its argument: {:?}",
            template_diagnostics(&backend, &uri)
        );

        let provider_path = dir.path().join("app/Providers/AppServiceProvider.php");
        let provider_uri = Uri::from_file_path(&provider_path).unwrap();
        let edited = PROVIDER.replace(
            "Blade::if('admin'",
            "Blade::directive('money', fn ($e) => $e);\n\n        Blade::if('admin'",
        );
        open_php(&backend, &provider_uri, PROVIDER).await;
        backend
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    text_document_identifier: TextDocumentIdentifier { uri: provider_uri },
                    version: 2,
                },
                content_changes: vec![
                    TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                        TextDocumentContentChangeWholeDocument { text: edited },
                    ),
                ],
            })
            .await;

        let reported = template_diagnostics(&backend, &uri);
        assert!(
            reported.iter().any(|m| m.contains("noSuchMethod")),
            "the newly registered @money should type-check its argument: {reported:?}"
        );
    }

    /// Nothing registered `@notregistered`, so it is still inert markup: the
    /// masking is what keeps a template using an unknown directive from
    /// being read as PHP that isn't there.
    #[tokio::test]
    async fn an_unregistered_directive_is_still_masked() {
        let (backend, _dir, uri) = start("<p>@notregistered($undefinedVariable)</p>\n").await;
        let reported = template_diagnostics(&backend, &uri);
        assert!(
            reported.is_empty(),
            "an unregistered directive's text is not PHP: {reported:?}"
        );
    }
}
