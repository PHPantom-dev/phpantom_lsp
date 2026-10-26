//! Built-in directives the installed Laravel does not have.
//!
//! Blade compiles an `@name` only when its compiler has a `compileName()`
//! method for it, and leaves any other as text. A `"@context"` key in a
//! JSON-LD block is therefore text on a Laravel that predates `@context`,
//! and reading it as the directive opens a block the template never closes.

#[cfg(test)]
mod tests {
    use crate::common::{
        completion_response, create_psr4_workspace, open_document, template_diagnostics,
    };
    use phpantom_lsp::Backend;
    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::gen_lsp_types::*;

    /// A Laravel application whose framework classes autoload from the
    /// vendor copy the test writes.
    const COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^8.0" },
    "autoload": { "psr-4": {
        "App\\": "app/",
        "Illuminate\\": "vendor/laravel/framework/src/Illuminate/"
    } }
}"#;

    /// A compiler with the conditionals and loops but none of the directives
    /// later Laravel versions added, `@context` among them.
    const BLADE_COMPILER: &str = r#"<?php
namespace Illuminate\View\Compilers;

class BladeCompiler
{
    use Concerns\CompilesConditionals;

    protected function compileForeach($expression) { return ''; }
    protected function compileEndforeach() { return ''; }
}
"#;

    const COMPILES_CONDITIONALS: &str = r#"<?php
namespace Illuminate\View\Compilers\Concerns;

trait CompilesConditionals
{
    protected function compileIf($expression) { return ''; }
    protected function compileElse() { return ''; }
    protected function compileEndif() { return ''; }
}
"#;

    const TEMPLATE: &str = "resources/views/page.blade.php";

    const JSON_LD: &str = "<script type=\"application/ld+json\">\n\
                           {\n    \"@context\": \"https://schema.org\",\n    \"@type\": \"BreadcrumbList\"\n}\n\
                           </script>\n\
                           @if (true)\n<p>shown</p>\n@endif\n";

    async fn start(template: &str) -> (Backend, tempfile::TempDir, Uri) {
        let (backend, dir) = create_psr4_workspace(
            COMPOSER,
            &[
                (
                    "vendor/laravel/framework/src/Illuminate/View/Compilers/BladeCompiler.php",
                    BLADE_COMPILER,
                ),
                (
                    "vendor/laravel/framework/src/Illuminate/View/Compilers/Concerns/CompilesConditionals.php",
                    COMPILES_CONDITIONALS,
                ),
                ("bootstrap/providers.php", "<?php\nreturn [];\n"),
                (TEMPLATE, template),
            ],
        );
        backend.initialized(InitializedParams {}).await;

        let uri = Uri::from_file_path(dir.path().join(TEMPLATE)).unwrap();
        open_document(&backend, &uri, "blade", template).await;
        (backend, dir, uri)
    }

    #[tokio::test]
    async fn a_directive_the_installed_compiler_lacks_is_text() {
        let (backend, _dir, uri) = start(JSON_LD).await;
        let reported = template_diagnostics(&backend, &uri);
        assert!(
            reported.is_empty(),
            "\"@context\" is text to this compiler: {reported:?}"
        );
    }

    /// The directives the compiler does have are still lowered, so a broken
    /// block of theirs is still reported.
    #[tokio::test]
    async fn a_directive_the_installed_compiler_has_is_still_read() {
        let (backend, _dir, uri) = start("@if (true)\n<p>shown</p>\n").await;
        let reported = template_diagnostics(&backend, &uri);
        assert!(!reported.is_empty(), "the unclosed @if should be reported");
    }

    #[tokio::test]
    async fn only_the_installed_directives_are_offered() {
        let (backend, _dir, uri) = start("<div>\n@\n</div>\n").await;
        let response = completion_response(&backend, &uri, 1, 1, None)
            .await
            .expect("directive completion should answer");
        let items = match response {
            CompletionResponse::CompletionItemList(items) => items,
            CompletionResponse::CompletionList(list) => list.items,
        };
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert!(labels.contains(&"@if"), "{labels:?}");
        assert!(labels.contains(&"@verbatim"), "{labels:?}");
        assert!(!labels.contains(&"@context"), "{labels:?}");
    }
}
