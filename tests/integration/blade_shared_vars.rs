//! `View::share()` and view composers put variables in a template's scope
//! that nothing in the template or its call sites mentions.
//!
//! A shared variable reaches every template; a composer's reaches only the
//! views its registration targets. Both are resolved from the expression the
//! provider (or the composer class) writes, so the template gets a real type
//! rather than `mixed`.

#[cfg(test)]
mod tests {
    use crate::common::{create_psr4_workspace, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    const COMPOSER_JSON: &str = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": {
            "psr-4": {
                "App\\": "app/",
                "Illuminate\\": "vendor/illuminate/"
            }
        }
    }"#;

    const PROVIDERS_PHP: &str =
        "<?php\nreturn [\n    App\\Providers\\ViewServiceProvider::class,\n];\n";

    const VIEW_SERVICE_PROVIDER: &str = r#"<?php
namespace App\Providers;

use App\Support\Menu;
use App\View\Composers\ProfileComposer;
use Illuminate\Support\Facades\View;

class ViewServiceProvider
{
    public function boot(): void
    {
        View::share('siteName', $this->siteName());
        $this->app['view']->share('menu', new Menu());

        View::composer('profile', ProfileComposer::class);
        View::composer('partials.*', function ($view) {
            $view->with('breadcrumbs', new Menu());
        });
    }

    private function siteName(): string
    {
        return 'Acme';
    }
}
"#;

    const PROFILE_COMPOSER: &str = r#"<?php
namespace App\View\Composers;

use App\Models\User;

class ProfileComposer
{
    public function compose($view): void
    {
        $view->with('user', new User());
    }
}
"#;

    const USER_CLASS: &str =
        "<?php\nnamespace App\\Models;\nclass User { public string $email = ''; }\n";
    const MENU_CLASS: &str = "<?php\nnamespace App\\Support;\nclass Menu { public function items(): array { return []; } }\n";
    const APPLICATION_CLASS: &str =
        "<?php\nnamespace Illuminate\\Foundation;\nclass Application {}\n";

    fn workspace(templates: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
        let mut files = vec![
            ("bootstrap/providers.php", PROVIDERS_PHP),
            (
                "app/Providers/ViewServiceProvider.php",
                VIEW_SERVICE_PROVIDER,
            ),
            ("app/View/Composers/ProfileComposer.php", PROFILE_COMPOSER),
            ("app/Models/User.php", USER_CLASS),
            ("app/Support/Menu.php", MENU_CLASS),
            (
                "vendor/illuminate/Foundation/Application.php",
                APPLICATION_CLASS,
            ),
        ];
        files.extend_from_slice(templates);
        create_psr4_workspace(COMPOSER_JSON, &files)
    }

    /// Open a template and hand back its URI, with the provider scan already
    /// run so the registrations are known.
    async fn open_template(
        backend: &phpantom_lsp::Backend,
        dir: &tempfile::TempDir,
        relative: &str,
    ) -> Url {
        backend.initialized(InitializedParams {}).await;
        let path = dir.path().join(relative);
        let text = std::fs::read_to_string(&path).unwrap();
        let uri = Url::from_file_path(&path).unwrap();
        open_document(backend, &uri, "blade", &text).await;
        uri
    }

    async fn hover_text(
        backend: &phpantom_lsp::Backend,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> String {
        let result = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position { line, character },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap();
        match result {
            Some(Hover {
                contents: HoverContents::Markup(m),
                ..
            }) => m.value,
            other => panic!("expected markup hover, got {other:?}"),
        }
    }

    fn undefined_variables(backend: &phpantom_lsp::Backend, uri: &Url) -> Vec<String> {
        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_undefined_variable_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        diags
            .into_iter()
            .filter(|d| d.message.contains("Undefined variable"))
            .map(|d| d.message)
            .collect()
    }

    /// A shared variable reaches a template no caller and no composer
    /// mentions, typed from the expression the provider shares.
    #[tokio::test]
    async fn a_shared_variable_reaches_every_template() {
        let (backend, dir) = workspace(&[("resources/views/about.blade.php", "{{ $siteName }}\n")]);
        let uri = open_template(&backend, &dir, "resources/views/about.blade.php").await;

        assert!(
            hover_text(&backend, &uri, 0, 7).await.contains("string"),
            "the shared value's own type must reach the template"
        );
        assert!(
            undefined_variables(&backend, &uri).is_empty(),
            "a shared variable is defined: {:?}",
            undefined_variables(&backend, &uri)
        );
    }

    /// A shared object keeps its class, so its members resolve in the
    /// template.
    #[tokio::test]
    async fn a_shared_object_keeps_its_class() {
        let (backend, dir) =
            workspace(&[("resources/views/about.blade.php", "{{ $menu->items() }}\n")]);
        let uri = open_template(&backend, &dir, "resources/views/about.blade.php").await;

        let hover = hover_text(&backend, &uri, 0, 5).await;
        assert!(
            hover.contains("App\\Support") && hover.contains("Menu"),
            "the container form of share() must resolve too, got: {hover}"
        );
    }

    /// A composer class's `$view->with()` only reaches the views the
    /// registration targets.
    #[tokio::test]
    async fn a_composer_reaches_only_the_views_it_targets() {
        let (backend, dir) = workspace(&[
            ("resources/views/profile.blade.php", "{{ $user->email }}\n"),
            ("resources/views/about.blade.php", "{{ $user->email }}\n"),
        ]);
        let profile = open_template(&backend, &dir, "resources/views/profile.blade.php").await;

        let hover = hover_text(&backend, &profile, 0, 5).await;
        assert!(
            hover.contains("App\\Models") && hover.contains("User"),
            "the composer's own expression types the variable, got: {hover}"
        );
        assert!(
            undefined_variables(&backend, &profile).is_empty(),
            "the targeted view has the composer's data: {:?}",
            undefined_variables(&backend, &profile)
        );

        let about =
            Url::from_file_path(dir.path().join("resources/views/about.blade.php")).unwrap();
        let text =
            std::fs::read_to_string(dir.path().join("resources/views/about.blade.php")).unwrap();
        open_document(&backend, &about, "blade", &text).await;
        assert!(
            undefined_variables(&backend, &about)
                .iter()
                .any(|m| m.contains("$user")),
            "a view the composer does not target never receives its data"
        );
    }

    /// A wildcard registration covers every view below its prefix.
    #[tokio::test]
    async fn a_wildcard_composer_covers_the_views_below_it() {
        let (backend, dir) = workspace(&[(
            "resources/views/partials/header.blade.php",
            "{{ $breadcrumbs->items() }}\n",
        )]);
        let uri = open_template(&backend, &dir, "resources/views/partials/header.blade.php").await;

        let hover = hover_text(&backend, &uri, 0, 5).await;
        assert!(
            hover.contains("App\\Support") && hover.contains("Menu"),
            "an inline closure composer types its data too, got: {hover}"
        );
    }

    /// A template's own `@var` declaration is closer to every use than an
    /// injected one, so the author still decides the type.
    #[tokio::test]
    async fn a_template_declaration_wins_over_a_shared_variable() {
        let (backend, dir) = workspace(&[(
            "resources/views/about.blade.php",
            "@php\n/** @var \\App\\Models\\User $siteName */\n@endphp\n{{ $siteName->email }}\n",
        )]);
        let uri = open_template(&backend, &dir, "resources/views/about.blade.php").await;

        let hover = hover_text(&backend, &uri, 3, 5).await;
        assert!(
            hover.contains("User"),
            "the template's own annotation decides the type, got: {hover}"
        );
    }
}
