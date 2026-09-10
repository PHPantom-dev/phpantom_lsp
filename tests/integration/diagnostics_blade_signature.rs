//! A template's signature is checked against the layouts it renders
//! through: it may narrow what a layout declared, never widen it. A second
//! `@bladestan-signature` block is reported too, since only the first is
//! ever read.

#[cfg(test)]
mod tests {
    use crate::common::{create_psr4_workspace, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    const COMPOSER: &str = r#"{
        "require": { "laravel/framework": "^11.0" },
        "autoload": { "psr-4": { "App\\": "app/" } }
    }"#;

    const USER_CLASS: &str =
        "<?php\nnamespace App\\Models;\nclass User { public string $email = ''; }\n";
    const ADMIN_CLASS: &str =
        "<?php\nnamespace App\\Models;\nclass Admin extends User { public string $role = ''; }\n";

    /// A layout declaring `$title` (string) and `$user` (User).
    const APP_LAYOUT: &str = "@php\n\
        /**\n\
         * @bladestan-signature\n\
         * @var string $title\n\
         * @var \\App\\Models\\User $user\n\
         */\n\
        @endphp\n\
        <title>{{ $title }}</title>\n\
        @yield('body')\n";

    /// The signature diagnostics for one template, as `(code, message)`
    /// pairs in report order.
    async fn signature_diagnostics(
        templates: &[(&str, &str)],
        relative: &str,
    ) -> Vec<(String, String)> {
        let mut files = vec![
            ("app/Models/User.php", USER_CLASS),
            ("app/Models/Admin.php", ADMIN_CLASS),
        ];
        files.extend_from_slice(templates);
        let (backend, dir) = create_psr4_workspace(COMPOSER, &files);
        backend.initialized(InitializedParams {}).await;

        let path = dir.path().join(relative);
        let text = std::fs::read_to_string(&path).unwrap();
        let uri = Url::from_file_path(&path).unwrap();
        open_document(&backend, &uri, "blade", &text).await;

        let effective = backend.blade_virtual_php(uri.as_str()).unwrap_or(text);
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &effective, &mut diags);
        diags
            .into_iter()
            .filter_map(|d| match &d.code {
                Some(NumberOrString::String(code))
                    if code == "blade_signature_widens_layout"
                        || code == "duplicate_blade_signature"
                        || code == "invalid_laravel_view" =>
                {
                    Some((code.clone(), d.message))
                }
                _ => None,
            })
            .collect()
    }

    /// A layout asking for a `string` gets whatever the child was passed,
    /// so a child declaring `string|int` promises less than the layout
    /// needs.
    #[tokio::test]
    async fn a_declaration_that_widens_its_layouts_is_reported() {
        let reported = signature_diagnostics(
            &[
                ("resources/views/layouts/app.blade.php", APP_LAYOUT),
                (
                    "resources/views/profile.blade.php",
                    "@extends('layouts.app')\n\
                     @php\n\
                     /**\n\
                      * @bladestan-signature\n\
                      * @var string|int $title\n\
                      */\n\
                     @endphp\n\
                     <p>{{ $title }}</p>\n",
                ),
            ],
            "resources/views/profile.blade.php",
        )
        .await;

        assert_eq!(
            reported.len(),
            1,
            "the widened declaration is the one report: {reported:?}"
        );
        assert_eq!(reported[0].0, "blade_signature_widens_layout");
        assert!(
            reported[0].1.contains("$title")
                && reported[0].1.contains("layouts.app")
                && reported[0].1.contains("string"),
            "the report names the variable, the layout, and the type: {}",
            reported[0].1
        );
    }

    /// The child's own declaration is closer to its body, so narrowing is
    /// the whole point of redeclaring a layout's variable.
    #[tokio::test]
    async fn a_declaration_that_narrows_its_layouts_is_accepted() {
        let reported = signature_diagnostics(
            &[
                ("resources/views/layouts/app.blade.php", APP_LAYOUT),
                (
                    "resources/views/console.blade.php",
                    "@extends('layouts.app')\n\
                     @php\n\
                     /**\n\
                      * @bladestan-signature\n\
                      * @var string $title\n\
                      * @var \\App\\Models\\Admin $user\n\
                      */\n\
                     @endphp\n\
                     <p>{{ $user->role }}</p>\n",
                ),
            ],
            "resources/views/console.blade.php",
        )
        .await;

        assert!(
            reported.is_empty(),
            "a subclass narrows the layout's declaration: {reported:?}"
        );
    }

    /// A declaration with nothing in common with the layout's is not a
    /// narrowing at all.
    #[tokio::test]
    async fn an_incompatible_declaration_is_reported() {
        let reported = signature_diagnostics(
            &[
                ("resources/views/layouts/app.blade.php", APP_LAYOUT),
                (
                    "resources/views/report.blade.php",
                    "@extends('layouts.app')\n\
                     @php\n\
                     /**\n\
                      * @bladestan-signature\n\
                      * @var int $title\n\
                      */\n\
                     @endphp\n\
                     <p>{{ $title }}</p>\n",
                ),
            ],
            "resources/views/report.blade.php",
        )
        .await;

        assert_eq!(
            reported.len(),
            1,
            "an incompatible declaration is reported: {reported:?}"
        );
        assert_eq!(reported[0].0, "blade_signature_widens_layout");
    }

    /// The whole chain is checked, so a grandparent's declaration binds
    /// the child too.
    #[tokio::test]
    async fn a_grandparent_layouts_declaration_is_checked_too() {
        let reported = signature_diagnostics(
            &[
                ("resources/views/layouts/app.blade.php", APP_LAYOUT),
                (
                    "resources/views/layouts/admin.blade.php",
                    "@extends('layouts.app')\n@yield('body')\n",
                ),
                (
                    "resources/views/dashboard.blade.php",
                    "@extends('layouts.admin')\n\
                     @php\n\
                     /**\n\
                      * @bladestan-signature\n\
                      * @var ?string $title\n\
                      */\n\
                     @endphp\n\
                     <p>{{ $title }}</p>\n",
                ),
            ],
            "resources/views/dashboard.blade.php",
        )
        .await;

        assert_eq!(
            reported.len(),
            1,
            "a nullable widens the grandparent's string: {reported:?}"
        );
        assert!(
            reported[0].1.contains("layouts.app"),
            "the report names the layout that declared it: {}",
            reported[0].1
        );
    }

    /// A name the layout never declares is the child's own to type.
    #[tokio::test]
    async fn a_name_no_layout_declares_is_the_childs_own() {
        let reported = signature_diagnostics(
            &[
                ("resources/views/layouts/app.blade.php", APP_LAYOUT),
                (
                    "resources/views/orders.blade.php",
                    "@extends('layouts.app')\n\
                     @php\n\
                     /**\n\
                      * @bladestan-signature\n\
                      * @var int|null $count\n\
                      */\n\
                     @endphp\n\
                     <p>{{ $count }}</p>\n",
                ),
            ],
            "resources/views/orders.blade.php",
        )
        .await;

        assert!(
            reported.is_empty(),
            "nothing above declares $count: {reported:?}"
        );
    }

    /// A template has one contract, so everything after the first marked
    /// block is silently unread.
    #[tokio::test]
    async fn a_second_signature_block_is_reported() {
        let reported = signature_diagnostics(
            &[(
                "resources/views/duplicate.blade.php",
                "@php\n\
                 /**\n\
                  * @bladestan-signature\n\
                  * @var string $title\n\
                  */\n\
                 @endphp\n\
                 @php\n\
                 /**\n\
                  * @bladestan-signature\n\
                  * @var int $count\n\
                  */\n\
                 @endphp\n\
                 <p>{{ $title }}</p>\n",
            )],
            "resources/views/duplicate.blade.php",
        )
        .await;

        assert_eq!(
            reported.len(),
            1,
            "only the second block is reported: {reported:?}"
        );
        assert_eq!(reported[0].0, "duplicate_blade_signature");
    }

    /// One signature block, however many other docblocks the template
    /// carries, is not a duplicate.
    #[tokio::test]
    async fn a_single_signature_block_is_not_a_duplicate() {
        let reported = signature_diagnostics(
            &[(
                "resources/views/single.blade.php",
                "@php\n\
                 /**\n\
                  * @bladestan-signature\n\
                  * @var string $title\n\
                  */\n\
                 @endphp\n\
                 @php\n\
                 /** @var int $count */\n\
                 $count = 1;\n\
                 @endphp\n\
                 <p>{{ $title }}{{ $count }}</p>\n",
            )],
            "resources/views/single.blade.php",
        )
        .await;

        assert!(
            reported.is_empty(),
            "a plain docblock is not a second signature: {reported:?}"
        );
    }

    /// A signature parked in a comment is inert to Blade, so it is not the
    /// second one either.
    #[tokio::test]
    async fn a_commented_out_signature_block_is_not_a_duplicate() {
        let reported = signature_diagnostics(
            &[(
                "resources/views/commented.blade.php",
                "{{--\n\
                 @php\n\
                 /**\n\
                  * @bladestan-signature\n\
                  * @var int $stale\n\
                  */\n\
                 @endphp\n\
                 --}}\n\
                 @php\n\
                 /**\n\
                  * @bladestan-signature\n\
                  * @var string $title\n\
                  */\n\
                 @endphp\n\
                 <p>{{ $title }}</p>\n",
            )],
            "resources/views/commented.blade.php",
        )
        .await;

        assert!(
            reported.is_empty(),
            "a commented block declares nothing to duplicate: {reported:?}"
        );
    }

    /// A layout no view root holds contributes nothing to the child, so
    /// the template is reported rather than left to look complete.
    #[tokio::test]
    async fn a_layout_that_cannot_be_found_is_reported() {
        let reported = signature_diagnostics(
            &[(
                "resources/views/orphan.blade.php",
                "@extends('layouts.missing')\n<p>Hi</p>\n",
            )],
            "resources/views/orphan.blade.php",
        )
        .await;

        assert_eq!(
            reported.len(),
            1,
            "the unresolvable layout is reported once: {reported:?}"
        );
        assert_eq!(reported[0].0, "invalid_laravel_view");
        assert!(
            reported[0].1.contains("layouts.missing"),
            "the report names the layout: {}",
            reported[0].1
        );
    }
}
