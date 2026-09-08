//! Imports declared inside a Blade template.
//!
//! Laravel compiles a template's `@php` / `<?php` regions into the top
//! level of the generated view file, so a `use` written in one (or the
//! `@use` directive) imports for the whole template. Each spelling must
//! feed the file's use-map, or every short name that depends on it fails
//! to resolve and the failure cascades into every variable typed from it.

#[cfg(test)]
mod tests {
    use crate::common::{create_psr4_workspace, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    const COMPOSER: &str = r#"{"autoload": {"psr-4": {"App\\": "app/"}}}"#;

    const HELPER: &str = "<?php\nnamespace App\\Helpers;\n\
class CurrencyHelper {\n\
    public static function formatPrice(int $value): string { return ''; }\n\
    public static function make(): self { return new self(); }\n\
    public function label(): string { return ''; }\n\
}\n";

    /// Open a template and return its diagnostics, minus the ones caused
    /// by the test workspace having no Laravel installed.
    async fn diagnose(view: &str) -> Vec<String> {
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Helpers/CurrencyHelper.php", HELPER),
                ("resources/views/page.blade.php", view),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        open_document(&backend, &uri, "blade", view).await;
        let virtual_php = backend
            .blade_virtual_php(uri.as_str())
            .expect("blade virtual content");
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        diags
            .into_iter()
            .map(|d| d.message)
            .filter(|m| !m.contains("Function 'e' not found"))
            .collect()
    }

    #[tokio::test]
    async fn import_in_a_php_directive_block_resolves() {
        let diags = diagnose(
            "@php\nuse App\\Helpers\\CurrencyHelper;\n@endphp\n\
             {{ CurrencyHelper::formatPrice(1) }}\n",
        )
        .await;
        assert!(diags.is_empty(), "the import must resolve: {:?}", diags);
    }

    #[tokio::test]
    async fn import_in_a_raw_php_block_resolves() {
        let diags = diagnose(
            "<?php\nuse App\\Helpers\\CurrencyHelper;\n?>\n\
             {{ CurrencyHelper::formatPrice(1) }}\n",
        )
        .await;
        assert!(diags.is_empty(), "the import must resolve: {:?}", diags);
    }

    #[tokio::test]
    async fn import_via_the_use_directive_resolves() {
        let diags = diagnose(
            "@use('App\\Helpers\\CurrencyHelper')\n{{ CurrencyHelper::formatPrice(1) }}\n",
        )
        .await;
        assert!(diags.is_empty(), "the import must resolve: {:?}", diags);
    }

    /// The point of the use-map entry: a variable assigned from the
    /// imported class carries its type through the rest of the template.
    #[tokio::test]
    async fn a_variable_typed_from_an_imported_class_keeps_its_type() {
        let diags = diagnose(
            "@php\nuse App\\Helpers\\CurrencyHelper;\n\
             $money = CurrencyHelper::make();\n@endphp\n\
             {{ $money->label() }}\n{{ $money->noSuchMethod() }}\n",
        )
        .await;
        assert_eq!(
            diags,
            vec!["Method 'noSuchMethod' not found on class 'App\\Helpers\\CurrencyHelper'"],
            "$money must resolve to the imported class"
        );
    }

    /// Without an import the short name is genuinely unknown, and still
    /// reported as such.
    #[tokio::test]
    async fn an_unimported_short_name_is_still_unknown() {
        let diags = diagnose("{{ CurrencyHelper::formatPrice(1) }}\n").await;
        assert_eq!(diags, vec!["Class 'CurrencyHelper' not found"]);
    }

    /// A `use` inside a template must not shift the mapping between Blade
    /// and virtual-PHP positions: hover on a later line has to land on the
    /// expression the cursor is actually over.
    #[tokio::test]
    async fn hoisting_an_import_keeps_positions_aligned() {
        let view = "@use('App\\Helpers\\CurrencyHelper')\n\
                    <p>text</p>\n\
                    {{ CurrencyHelper::formatPrice(1) }}\n";
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Helpers/CurrencyHelper.php", HELPER),
                ("resources/views/page.blade.php", view),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        open_document(&backend, &uri, "blade", view).await;

        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    // `formatPrice` on the third Blade line.
                    position: Position {
                        line: 2,
                        character: 25,
                    },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap();
        let text = match hover.map(|h| h.contents) {
            Some(HoverContents::Markup(m)) => m.value,
            _ => String::new(),
        };
        assert!(
            text.contains("formatPrice"),
            "hover must still land on the method under the cursor, got: {}",
            text
        );
    }

    /// The unused-import check sees uses that live in the template body,
    /// including ones that only appear in a `@var` docblock.
    #[tokio::test]
    async fn an_import_used_only_in_the_template_body_is_not_reported_unused() {
        for view in [
            "@php\nuse App\\Helpers\\CurrencyHelper;\n@endphp\n{{ CurrencyHelper::formatPrice(1) }}\n",
            "@php\nuse App\\Helpers\\CurrencyHelper;\n/** @var CurrencyHelper $h */\n@endphp\n{{ $h->label() }}\n",
            "@use('App\\Helpers\\CurrencyHelper')\n{{ CurrencyHelper::formatPrice(1) }}\n",
        ] {
            let (backend, _dir) = create_psr4_workspace(
                COMPOSER,
                &[
                    ("app/Helpers/CurrencyHelper.php", HELPER),
                    ("resources/views/page.blade.php", view),
                ],
            );
            let root = backend.workspace_root().read().clone().unwrap();
            let uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
            open_document(&backend, &uri, "blade", view).await;
            let virtual_php = backend.blade_virtual_php(uri.as_str()).unwrap();
            let mut diags = Vec::new();
            backend.collect_unused_import_diagnostics(uri.as_str(), &virtual_php, &mut diags);
            assert!(
                diags.is_empty(),
                "import is used in {:?}, got: {:?}",
                view,
                diags.iter().map(|d| &d.message).collect::<Vec<_>>()
            );
        }
    }

    /// An import nothing references is still reported, at its Blade line.
    #[tokio::test]
    async fn a_genuinely_unused_import_is_reported() {
        let view = "@php\nuse App\\Helpers\\CurrencyHelper;\n@endphp\n<p>nothing</p>\n";
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Helpers/CurrencyHelper.php", HELPER),
                ("resources/views/page.blade.php", view),
            ],
        );
        let root = backend.workspace_root().read().clone().unwrap();
        let uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
        open_document(&backend, &uri, "blade", view).await;
        let virtual_php = backend.blade_virtual_php(uri.as_str()).unwrap();
        let mut diags = Vec::new();
        backend.collect_unused_import_diagnostics(uri.as_str(), &virtual_php, &mut diags);
        assert_eq!(diags.len(), 1, "{:?}", diags);
        assert!(diags[0].message.contains("CurrencyHelper"));
        assert_eq!(diags[0].range.start.line, 1, "reported on the Blade line");
    }
}
