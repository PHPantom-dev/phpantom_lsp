//! A child template and the layout it `@extends` render from one data
//! array, so whatever the layout declares is in the child's scope too.

#[cfg(test)]
mod tests {
    use crate::common::{
        blade_undefined_variables, create_psr4_workspace, markup_hover_at, open_blade_template,
    };
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    const COMPOSER: &str = r#"{"autoload": {"psr-4": {"App\\": "app/"}}}"#;

    const USER_CLASS: &str =
        "<?php\nnamespace App\\Models;\nclass User { public string $email = ''; }\n";
    const ADMIN_CLASS: &str =
        "<?php\nnamespace App\\Models;\nclass Admin extends User { public string $role = ''; }\n";

    const APP_LAYOUT: &str = "@php\n\
        /**\n\
         * @bladestan-signature\n\
         * @var string $title\n\
         * @var \\App\\Models\\User $user\n\
         */\n\
        @endphp\n\
        <title>{{ $title }}</title>\n\
        @yield('body')\n";

    fn workspace(templates: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
        let mut files = vec![
            ("app/Models/User.php", USER_CLASS),
            ("app/Models/Admin.php", ADMIN_CLASS),
        ];
        files.extend_from_slice(templates);
        create_psr4_workspace(COMPOSER, &files)
    }

    async fn open_template(
        backend: &phpantom_lsp::Backend,
        dir: &tempfile::TempDir,
        relative: &str,
    ) -> Url {
        backend.initialized(InitializedParams {}).await;
        open_blade_template(backend, dir.path(), relative).await
    }

    /// What the layout declares, the child that extends it receives:
    /// Laravel renders both from the same data array.
    #[tokio::test]
    async fn a_layouts_declarations_reach_the_child() {
        let (backend, dir) = workspace(&[
            ("resources/views/layouts/app.blade.php", APP_LAYOUT),
            (
                "resources/views/profile.blade.php",
                "@extends('layouts.app')\n\
                 @section('body')\n\
                 <h1>{{ $user->email }}</h1>\n\
                 <p>{{ $title }}</p>\n\
                 @endsection\n",
            ),
        ]);
        let uri = open_template(&backend, &dir, "resources/views/profile.blade.php").await;

        let hover = markup_hover_at(&backend, &uri, 2, 8).await;
        assert!(
            hover.contains("App\\Models") && hover.contains("User"),
            "the layout's declared class must reach the child, got: {hover}"
        );
        assert!(
            markup_hover_at(&backend, &uri, 3, 8)
                .await
                .contains("string"),
            "the layout's scalar declaration must reach the child too"
        );
        assert!(
            blade_undefined_variables(&backend, &uri).is_empty(),
            "a layout-declared variable is defined in the child: {:?}",
            blade_undefined_variables(&backend, &uri)
        );
    }

    /// The chain is walked all the way up, so a name only the grandparent
    /// layout declares still reaches the child.
    #[tokio::test]
    async fn the_whole_layout_chain_contributes() {
        let (backend, dir) = workspace(&[
            ("resources/views/layouts/app.blade.php", APP_LAYOUT),
            (
                "resources/views/layouts/admin.blade.php",
                "@extends('layouts.app')\n\
                 @php\n\
                 /** @var \\App\\Models\\Admin $admin */\n\
                 @endphp\n\
                 @yield('body')\n",
            ),
            (
                "resources/views/dashboard.blade.php",
                "@extends('layouts.admin')\n\
                 @section('body')\n\
                 <h1>{{ $admin->role }}</h1>\n\
                 <p>{{ $title }}</p>\n\
                 @endsection\n",
            ),
        ]);
        let uri = open_template(&backend, &dir, "resources/views/dashboard.blade.php").await;

        let hover = markup_hover_at(&backend, &uri, 2, 8).await;
        assert!(
            hover.contains("Admin"),
            "the nearest layout's own declaration must reach the child, got: {hover}"
        );
        assert!(
            markup_hover_at(&backend, &uri, 3, 8)
                .await
                .contains("string"),
            "a grandparent layout's declaration must reach the child too"
        );
        assert!(
            blade_undefined_variables(&backend, &uri).is_empty(),
            "every layout in the chain declares into the child: {:?}",
            blade_undefined_variables(&backend, &uri)
        );
    }

    /// The child may narrow a name its layout declares: its own signature
    /// is closer to the body, so its type is the one that stands.
    #[tokio::test]
    async fn the_child_narrows_a_name_its_layout_declares() {
        let (backend, dir) = workspace(&[
            ("resources/views/layouts/app.blade.php", APP_LAYOUT),
            (
                "resources/views/console.blade.php",
                "@extends('layouts.app')\n\
                 @php\n\
                 /** @var \\App\\Models\\Admin $user */\n\
                 @endphp\n\
                 <h1>{{ $user->role }}</h1>\n\
                 <p>{{ $title }}</p>\n",
            ),
        ]);
        let uri = open_template(&backend, &dir, "resources/views/console.blade.php").await;

        let hover = markup_hover_at(&backend, &uri, 4, 8).await;
        assert!(
            hover.contains("Admin"),
            "the child's own declaration must win over the layout's, got: {hover}"
        );
        assert!(
            blade_undefined_variables(&backend, &uri).is_empty(),
            "narrowing must not lose the layout's other names: {:?}",
            blade_undefined_variables(&backend, &uri)
        );
    }

    /// A layout named by an expression names no file that can be read, so
    /// nothing is invented for the child.
    #[tokio::test]
    async fn a_dynamic_extends_declares_nothing() {
        let (backend, dir) = workspace(&[
            ("resources/views/layouts/app.blade.php", APP_LAYOUT),
            (
                "resources/views/themed.blade.php",
                "@extends($layout)\n<p>{{ $title }}</p>\n",
            ),
        ]);
        let uri = open_template(&backend, &dir, "resources/views/themed.blade.php").await;

        assert!(
            blade_undefined_variables(&backend, &uri)
                .iter()
                .any(|message| message.contains("title")),
            "a dynamic layout name must not put the layout's variables in scope"
        );
    }

    /// A chain that loops back on itself terminates with what it found
    /// rather than walking forever.
    #[tokio::test]
    async fn a_layout_cycle_terminates() {
        let (backend, dir) = workspace(&[
            (
                "resources/views/loops/one.blade.php",
                "@extends('loops.two')\n\
                 @php\n\
                 /** @var string $one */\n\
                 @endphp\n\
                 <p>{{ $one }}{{ $two }}</p>\n",
            ),
            (
                "resources/views/loops/two.blade.php",
                "@extends('loops.one')\n\
                 @php\n\
                 /** @var int $two */\n\
                 @endphp\n\
                 @yield('body')\n",
            ),
        ]);
        let uri = open_template(&backend, &dir, "resources/views/loops/one.blade.php").await;

        assert!(
            markup_hover_at(&backend, &uri, 4, 8)
                .await
                .contains("string"),
            "the template's own declaration still stands"
        );
        assert!(
            blade_undefined_variables(&backend, &uri).is_empty(),
            "the other side of the cycle still declares into this one: {:?}",
            blade_undefined_variables(&backend, &uri)
        );
    }
}
