//! Drupal's hooks: the `hook_*()` functions of `*.api.php` files that
//! document them, and the implementations that `#[Hook]` attributes and
//! procedural function names make.

use crate::common::hover_text_at;
use crate::drupal_services::{definitions_at, position_in, references_at, uri_for, workspace_in};

const FORM_API: &str = "<?php\n/**\n * Perform alterations before a form is rendered.\n */\nfunction hook_form_alter(&$form, \\Drupal\\Core\\Form\\FormStateInterface $form_state, $form_id) {\n}\n\nfunction hook_cron() {\n}\n";
const FORM_API_PATH: &str = "web/core/lib/Drupal/Core/Form/form.api.php";

const HOOK_ATTRIBUTE: (&str, &str) = (
    "web/core/lib/Drupal/Core/Hook/Attribute/Hook.php",
    "<?php\nnamespace Drupal\\Core\\Hook\\Attribute;\n#[\\Attribute]\nclass Hook {\n    public function __construct(public string $hook, public string $method = '', public ?string $module = NULL) {}\n}\n",
);

const HOOKS: &str = r#"<?php
namespace Drupal\acme\Hook;

use Drupal\Core\Hook\Attribute\Hook;

final class AcmeHooks {
    #[Hook('form_alter')]
    public function formAlter(array &$form): void {}

    #[Hook(hook: 'cron', module: 'acme_extra')]
    public function cron(): void {}
}

#[Hook('cron')]
final class CronHook {
    public function __invoke(): void {}
}
"#;
const HOOKS_PATH: &str = "web/modules/custom/acme/src/Hook/AcmeHooks.php";

const NOT_A_HOOK_CLASS: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Hook\Attribute\Hook;

final class Elsewhere {
    #[Hook('cron')]
    public function cron(): void {}
}
"#;

/// A workspace whose `.gitignore` leaves core out, as a Composer install of
/// Drupal does, holding `extra` on disk and `open` open.
async fn drupal(
    extra: &[(&str, &str)],
    open: &[(&str, &str)],
) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in [
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        (
            "web/modules/custom/acme/acme.info.yml",
            "name: Acme\ntype: module\ncore_version_requirement: ^11\n",
        ),
    ]
    .iter()
    .chain(extra)
    {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    workspace_in(dir, open).await
}

#[tokio::test]
async fn a_hook_attribute_goes_to_the_documentation() {
    let (backend, dir) = drupal(
        &[(FORM_API_PATH, FORM_API), HOOK_ATTRIBUTE],
        &[
            (HOOKS_PATH, HOOKS),
            (
                "web/modules/custom/acme/src/Form/Elsewhere.php",
                NOT_A_HOOK_CLASS,
            ),
        ],
    )
    .await;
    let php = uri_for(&dir, HOOKS_PATH);
    let api = uri_for(&dir, FORM_API_PATH);
    // Finding references indexes the workspace, core's `*.api.php` included.
    references_at(&backend, &php, position_in(HOOKS, "'cron'", 2)).await;

    for needle in ["'form_alter'", "hook: 'cron'"] {
        let inside = needle.find('\'').unwrap() + 2;
        let found = definitions_at(&backend, &php, position_in(HOOKS, needle, inside)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(found[0].uri, api, "{needle}");
    }

    let position = position_in(HOOKS, "'form_alter'", 2);
    let hover = hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Hook"), "{hover}");

    // From the documentation: the implementations, not the class outside
    // `src/Hook`.
    let found = references_at(&backend, &api, position_in(FORM_API, "cron", 1)).await;
    assert_eq!(
        found,
        vec![(api, 7), (php.clone(), 9), (php, 13)],
        "{found:?}"
    );
}
