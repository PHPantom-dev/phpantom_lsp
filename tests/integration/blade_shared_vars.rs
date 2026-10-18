//! `View::share()` and view composers put variables in a template's scope
//! that nothing in the template or its call sites mentions.
//!
//! A shared variable reaches every template; a composer's reaches only the
//! views its registration targets. Both are resolved from the expression the
//! provider (or the composer class) writes, so the template gets a real type
//! rather than `mixed`.

#[cfg(test)]
mod tests {
    use crate::common::{
        USER_MODEL_STUB, blade_undefined_variables, create_psr4_workspace, markup_hover_at,
        open_blade_template, open_initialized_blade_template, with_code,
    };

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
            ("app/Models/User.php", USER_MODEL_STUB),
            ("app/Support/Menu.php", MENU_CLASS),
            (
                "vendor/illuminate/Foundation/Application.php",
                APPLICATION_CLASS,
            ),
        ];
        files.extend_from_slice(templates);
        create_psr4_workspace(COMPOSER_JSON, &files)
    }

    /// A shared variable reaches a template no caller and no composer
    /// mentions, typed from the expression the provider shares.
    #[tokio::test]
    async fn a_shared_variable_reaches_every_template() {
        let (backend, _dir) =
            workspace(&[("resources/views/about.blade.php", "{{ $siteName }}\n")]);
        let uri =
            open_initialized_blade_template(&backend, "resources/views/about.blade.php").await;

        assert!(
            markup_hover_at(&backend, &uri, 0, 7)
                .await
                .contains("string"),
            "the shared value's own type must reach the template"
        );
        assert!(
            blade_undefined_variables(&backend, &uri).is_empty(),
            "a shared variable is defined: {:?}",
            blade_undefined_variables(&backend, &uri)
        );
    }

    /// A shared object keeps its class, so its members resolve in the
    /// template.
    #[tokio::test]
    async fn a_shared_object_keeps_its_class() {
        let (backend, _dir) =
            workspace(&[("resources/views/about.blade.php", "{{ $menu->items() }}\n")]);
        let uri =
            open_initialized_blade_template(&backend, "resources/views/about.blade.php").await;

        let hover = markup_hover_at(&backend, &uri, 0, 5).await;
        assert!(
            hover.contains("App\\Support") && hover.contains("Menu"),
            "the container form of share() must resolve too, got: {hover}"
        );
    }

    /// A composer class's `$view->with()` only reaches the views the
    /// registration targets.
    #[tokio::test]
    async fn a_composer_reaches_only_the_views_it_targets() {
        let (backend, _dir) = workspace(&[
            ("resources/views/profile.blade.php", "{{ $user->email }}\n"),
            ("resources/views/about.blade.php", "{{ $user->email }}\n"),
        ]);
        let profile =
            open_initialized_blade_template(&backend, "resources/views/profile.blade.php").await;

        let hover = markup_hover_at(&backend, &profile, 0, 5).await;
        assert!(
            hover.contains("App\\Models") && hover.contains("User"),
            "the composer's own expression types the variable, got: {hover}"
        );
        assert!(
            blade_undefined_variables(&backend, &profile).is_empty(),
            "the targeted view has the composer's data: {:?}",
            blade_undefined_variables(&backend, &profile)
        );

        let about = open_blade_template(&backend, "resources/views/about.blade.php").await;
        assert!(
            blade_undefined_variables(&backend, &about)
                .iter()
                .any(|m| m.contains("$user")),
            "a view the composer does not target never receives its data"
        );
    }

    /// A wildcard registration covers every view below its prefix.
    #[tokio::test]
    async fn a_wildcard_composer_covers_the_views_below_it() {
        let (backend, _dir) = workspace(&[(
            "resources/views/partials/header.blade.php",
            "{{ $breadcrumbs->items() }}\n",
        )]);
        let uri =
            open_initialized_blade_template(&backend, "resources/views/partials/header.blade.php")
                .await;

        let hover = markup_hover_at(&backend, &uri, 0, 5).await;
        assert!(
            hover.contains("App\\Support") && hover.contains("Menu"),
            "an inline closure composer types its data too, got: {hover}"
        );
    }

    /// A translation or a config value a composer passes reaches the
    /// template typed by what its key names, the way the same call reads in
    /// the provider itself.
    #[tokio::test]
    async fn a_composed_translation_or_config_value_is_what_its_key_names() {
        const PROVIDER: &str = r#"<?php
namespace App\Providers;

use Illuminate\Support\Facades\View;

class ViewServiceProvider
{
    public function boot(): void
    {
        View::composer('emails.*', function ($view) {
            $view->with('footer', __('messages.welcome'));
            $view->with('limit', config('mail.limit'));
        });
    }
}
"#;
        const HELPERS: &str = "<?php\n/** @return string|array|null */\nfunction __($key = null, $replace = [], $locale = null) { return null; }\n";
        const AUTOLOAD_FILES: &str = "<?php\n$vendorDir = dirname(__DIR__);\nreturn array(\n    'helpers' => $vendorDir . '/illuminate/Foundation/helpers.php',\n);\n";
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER_JSON,
            &[
                ("bootstrap/providers.php", PROVIDERS_PHP),
                ("app/Providers/ViewServiceProvider.php", PROVIDER),
                (
                    "vendor/illuminate/Foundation/Application.php",
                    APPLICATION_CLASS,
                ),
                ("vendor/illuminate/Foundation/helpers.php", HELPERS),
                ("vendor/composer/autoload_files.php", AUTOLOAD_FILES),
                (
                    "lang/en/messages.php",
                    "<?php\nreturn ['welcome' => 'Welcome'];\n",
                ),
                ("config/mail.php", "<?php\nreturn ['limit' => 25];\n"),
                (
                    "resources/views/emails/welcome.blade.php",
                    "{{ $footer }}\n{{ $limit }}\n",
                ),
            ],
        );
        let uri =
            open_initialized_blade_template(&backend, "resources/views/emails/welcome.blade.php")
                .await;

        let footer = markup_hover_at(&backend, &uri, 0, 5).await;
        assert!(
            footer.contains("string") && !footer.contains("array"),
            "the key names a single line, got: {footer}"
        );
        let limit = markup_hover_at(&backend, &uri, 1, 5).await;
        assert!(
            limit.contains("int"),
            "the key names an integer, got: {limit}"
        );
    }

    /// A translation whose key is only known at runtime may be a line or a
    /// group, and the provider itself may pass it on wherever a string is
    /// expected. The template it reaches accepts it the same way, while a
    /// value that is never a string is still reported.
    #[tokio::test]
    async fn a_composed_translation_with_a_runtime_key_is_accepted_where_a_string_is() {
        const PROVIDER: &str = r#"<?php
namespace App\Providers;

use Illuminate\Support\Facades\View;

class ViewServiceProvider
{
    private string $footerKey = 'messages.welcome';

    public function boot(): void
    {
        View::composer('emails.*', function ($view) {
            $view->with('footer', __($this->footerKey));
            $view->with('tags', ['a', 'b']);
        });
    }
}
"#;
        const HELPERS: &str = "<?php\n\
            /** @return ($key is null ? null : array|string) */\n\
            function __($key = null, $replace = [], $locale = null): string|array|null { return null; }\n\
            function e(string|int|float|null $value, $doubleEncode = true): string { return ''; }\n";
        const AUTOLOAD_FILES: &str = "<?php\n$vendorDir = dirname(__DIR__);\nreturn array(\n    'helpers' => $vendorDir . '/illuminate/Foundation/helpers.php',\n);\n";
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER_JSON,
            &[
                ("bootstrap/providers.php", PROVIDERS_PHP),
                ("app/Providers/ViewServiceProvider.php", PROVIDER),
                (
                    "vendor/illuminate/Foundation/Application.php",
                    APPLICATION_CLASS,
                ),
                ("vendor/illuminate/Foundation/helpers.php", HELPERS),
                ("vendor/composer/autoload_files.php", AUTOLOAD_FILES),
                (
                    "lang/en/messages.php",
                    "<?php\nreturn ['welcome' => 'Welcome'];\n",
                ),
                (
                    "resources/views/emails/welcome.blade.php",
                    "{{ $footer }}\n{{ $tags }}\n",
                ),
            ],
        );
        let uri =
            open_initialized_blade_template(&backend, "resources/views/emails/welcome.blade.php")
                .await;

        let footer = markup_hover_at(&backend, &uri, 0, 5).await;
        assert!(
            footer.contains("string") && footer.contains("array"),
            "a runtime key may name a line or a group, got: {footer}"
        );
        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        let reported: Vec<u32> = with_code(diags, "type_mismatch_argument")
            .iter()
            .map(|d| d.range.start.line)
            .collect();
        assert_eq!(
            reported,
            vec![1],
            "only echoing the array is reported, not the translation"
        );
    }

    /// A template's own `@var` declaration is closer to every use than an
    /// injected one, so the author still decides the type.
    #[tokio::test]
    async fn a_template_declaration_wins_over_a_shared_variable() {
        let (backend, _dir) = workspace(&[(
            "resources/views/about.blade.php",
            "@php\n/** @var \\App\\Models\\User $siteName */\n@endphp\n{{ $siteName->email }}\n",
        )]);
        let uri =
            open_initialized_blade_template(&backend, "resources/views/about.blade.php").await;

        let hover = markup_hover_at(&backend, &uri, 3, 5).await;
        assert!(
            hover.contains("User"),
            "the template's own annotation decides the type, got: {hover}"
        );
    }
}
