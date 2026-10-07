use crate::common::{create_psr4_workspace, lsp_pos_to_offset, open_php};
use tower_lsp::lsp_types::*;

const COMPOSER: &str =
    r#"{"require":{"laravel/framework":"^13.0"},"autoload":{"psr-4":{"App\\":"src/"}}}"#;

fn translation_diagnostics(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    diagnostics.into_iter().filter(|diagnostic| matches!(&diagnostic.code, Some(NumberOrString::String(code)) if code == "invalid_laravel_trans")).collect()
}

fn actions(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
    diagnostics: Vec<Diagnostic>,
) -> Vec<CodeAction> {
    backend
        .handle_code_action(
            uri.as_str(),
            content,
            &CodeActionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                range: Range::new(Position::new(0, 0), Position::new(100, 0)),
                context: CodeActionContext {
                    diagnostics,
                    only: Some(vec![CodeActionKind::QUICKFIX]),
                    trigger_kind: None,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        )
        .into_iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action)
                if action.title.starts_with("Insert translation") =>
            {
                Some(action)
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn translation_insertion_quick_fix_edits_existing_groups_in_both_roots() {
    let source = "<?php\n__('messages.checkout.new');\n";
    let lang = "<?php\nreturn [\n    'checkout' => [\n        'existing' => 'Keep me', // comment\n    ],\n];\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/usage.php", source),
            ("lang/en/messages.php", lang),
            ("resources/lang/fr/messages.php", lang),
        ],
    );
    let uri = Url::from_file_path(dir.path().join("src/usage.php")).unwrap();
    open_php(&backend, &uri, source).await;
    let diagnostics = translation_diagnostics(&backend, &uri, source);
    assert_eq!(diagnostics.len(), 1);
    let fixes = actions(&backend, &uri, source, diagnostics.clone());
    assert_eq!(fixes.len(), 2);
    for fix in fixes {
        assert_eq!(fix.kind, Some(CodeActionKind::QUICKFIX));
        assert_eq!(fix.diagnostics, Some(diagnostics.clone()));
        let edit = fix.edit.unwrap();
        assert!(edit.document_changes.is_none(), "must not create files");
        let changes = edit.changes.unwrap();
        assert_eq!(changes.len(), 1);
        let (file_uri, mut edits) = changes.into_iter().next().unwrap();
        assert!(
            file_uri.path().ends_with("lang/en/messages.php")
                || file_uri.path().ends_with("resources/lang/fr/messages.php")
        );
        edits.sort_by_key(|edit| edit.range.start);
        let mut updated = lang.to_string();
        for edit in edits.into_iter().rev() {
            updated.replace_range(
                lsp_pos_to_offset(lang, edit.range.start)..lsp_pos_to_offset(lang, edit.range.end),
                &edit.new_text,
            );
        }
        assert!(updated.contains("'existing' => 'Keep me', // comment"));
        assert!(updated.contains("        'new' => '',\n"));
        open_php(&backend, &file_uri, &updated).await;
        assert!(translation_diagnostics(&backend, &uri, source).is_empty());
        assert!(
            actions(&backend, &uri, source, diagnostics.clone()).is_empty(),
            "stale diagnostic must not duplicate a key"
        );
    }
}

/// A key spelled with an escape that decodes to bytes no UTF-8 string can
/// hold (`"\x8b"`) is compared as bytes, so it is simply not the key being
/// added, and the new one still goes in.
#[tokio::test]
async fn translation_insertion_quick_fix_passes_over_an_undecodable_key() {
    let source = "<?php\n__('messages.new');\n";
    let lang = "<?php\nreturn [\n    'existing' => 'y',\n    \"\\x8b\" => 'x',\n];\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[("src/usage.php", source), ("lang/en/messages.php", lang)],
    );
    let uri = Url::from_file_path(dir.path().join("src/usage.php")).unwrap();
    open_php(&backend, &uri, source).await;
    let diagnostics = translation_diagnostics(&backend, &uri, source);
    assert_eq!(diagnostics.len(), 1);
    let fixes = actions(&backend, &uri, source, diagnostics);
    assert_eq!(fixes.len(), 1, "the group file still takes the new key");
    let changes = fixes[0].edit.as_ref().unwrap().changes.as_ref().unwrap();
    let edits = changes.values().next().unwrap();
    assert!(
        edits
            .iter()
            .any(|edit| edit.new_text.contains("'new' => ''")),
        "{edits:?}"
    );
}

/// An installed package's language files are Composer's, which rewrites
/// them on the next update, so the line goes into the application's
/// published copy under `lang/vendor/<namespace>/` and nowhere else.
#[tokio::test]
async fn translation_insertion_quick_fix_leaves_installed_packages_alone() {
    let composer = r#"{"require":{"laravel/framework":"^13.0"},"autoload":{"psr-4":{"App\\":"src/","Acme\\Billing\\":"vendor/acme/billing/src/"}}}"#;
    let provider = "<?php\nnamespace Acme\\Billing;\nclass BillingServiceProvider {\n    public function boot(): void {\n        $this->loadTranslationsFrom(__DIR__.'/../lang', 'billing');\n    }\n}\n";
    let lang = "<?php\nreturn [\n    'total' => 'Total',\n];\n";
    let source = "<?php\n__('billing::invoice.nope');\n";
    let (backend, dir, uri) = crate::common::create_initialized_psr4_workspace(
        composer,
        &[
            (
                "bootstrap/providers.php",
                "<?php\nreturn [\n    Acme\\Billing\\BillingServiceProvider::class,\n];\n",
            ),
            (
                "vendor/acme/billing/src/BillingServiceProvider.php",
                provider,
            ),
            ("vendor/acme/billing/lang/en/invoice.php", lang),
            ("lang/vendor/billing/en/invoice.php", lang),
            ("src/usage.php", source),
        ],
        "src/usage.php",
    )
    .await;
    let diagnostics = translation_diagnostics(&backend, &uri, source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");

    let targets: Vec<String> = actions(&backend, &uri, source, diagnostics)
        .into_iter()
        .flat_map(|fix| fix.edit.unwrap().changes.unwrap().into_keys())
        .map(|file| file.to_string())
        .collect();
    let published = Url::from_file_path(dir.path().join("lang/vendor/billing/en/invoice.php"))
        .unwrap()
        .to_string();
    assert_eq!(targets, vec![published]);
}

#[tokio::test]
async fn translation_insertion_quick_fix_requires_a_diagnostic_and_existing_safe_group() {
    let source = "<?php\n__('absent.new');\n__('Missing phrase');\n__('messages..invalid');\n__('messages.scalar.child');\n__('messages.new');";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/usage.php", source),
            (
                "resources/lang/en/messages.php",
                "<?php return ['scalar'=>'existing'];",
            ),
        ],
    );
    let uri = Url::from_file_path(dir.path().join("src/usage.php")).unwrap();
    open_php(&backend, &uri, source).await;
    assert!(actions(&backend, &uri, source, vec![]).is_empty());
    let diagnostics = translation_diagnostics(&backend, &uri, source);
    assert_eq!(diagnostics.len(), 5);
    let fixes = actions(&backend, &uri, source, diagnostics);
    assert_eq!(fixes.len(), 1);
    assert!(fixes[0].title.contains("messages.new"));
    assert!(!dir.path().join("resources/lang/en/absent.php").exists());
}
