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

const MODULE: &str = r#"<?php

use Drupal\Core\Hook\Attribute\LegacyHook;
use Drupal\Core\Hook\Attribute\ProceduralHookScanStop;

function acme_form_alter(&$form, $form_state, $form_id) {
}

#[LegacyHook]
function acme_cron() {
}

function acme_helper() {
}

#[ProceduralHookScanStop]
function acme_stop() {
}

function acme_late_cron() {
}
"#;
const MODULE_PATH: &str = "web/modules/custom/acme/acme.module";

const ATTRIBUTES: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Hook/Attribute/LegacyHook.php",
        "<?php\nnamespace Drupal\\Core\\Hook\\Attribute;\n#[\\Attribute]\nclass LegacyHook {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Hook/Attribute/ProceduralHookScanStop.php",
        "<?php\nnamespace Drupal\\Core\\Hook\\Attribute;\n#[\\Attribute]\nclass ProceduralHookScanStop {}\n",
    ),
];

/// A function of a `.module` file named after its module implements the
/// hook the rest of its name says, as Drupal's hook collector reads it.
#[tokio::test]
async fn a_procedural_function_implements_the_hook_its_name_says() {
    let (backend, dir) = drupal(
        &[(FORM_API_PATH, FORM_API), ATTRIBUTES[0], ATTRIBUTES[1]],
        &[(MODULE_PATH, MODULE)],
    )
    .await;
    let php = uri_for(&dir, MODULE_PATH);
    let api = uri_for(&dir, FORM_API_PATH);
    references_at(&backend, &php, position_in(MODULE, "acme_helper", 6)).await;

    let found = definitions_at(&backend, &php, position_in(MODULE, "acme_form_alter", 6)).await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((&found[0].uri, found[0].range.start.line), (&api, 4));

    // The module part of the name is still the function's: its references
    // are the function's own.
    let found = references_at(&backend, &php, position_in(MODULE, "acme_form_alter", 1)).await;
    assert_eq!(found, vec![(php.clone(), 5)], "{found:?}");

    // A legacy function and anything after the scan stop implement nothing.
    let found = references_at(&backend, &api, position_in(FORM_API, "cron", 1)).await;
    assert_eq!(found, vec![(api.clone(), 7)], "{found:?}");
    let found = references_at(&backend, &api, position_in(FORM_API, "form_alter", 1)).await;
    assert_eq!(found, vec![(api, 4), (php, 5)], "{found:?}");
}

/// Install files, themes, and another extension's name all implement
/// hooks, by the extensions the `*.info.yml` files declare.
#[tokio::test]
async fn install_files_themes_and_other_extensions_implement_hooks() {
    let api = "<?php\nfunction hook_install() {\n}\nfunction hook_preprocess_HOOK(&$variables) {\n}\nfunction hook_cron() {\n}\n";
    let api_path = "web/core/lib/Drupal/Core/Extension/module.api.php";
    let install = "<?php\nfunction acme_install() {\n}\n";
    let theme = "<?php\nfunction olivero_preprocess_HOOK(&$variables) {\n}\n";
    let other = "<?php\nfunction acme_extra_cron() {\n}\nfunction node_cron() {\n}\n";
    let (backend, dir) = drupal(
        &[
            (api_path, api),
            ("web/core/themes/olivero/olivero.info.yml", "type: theme\n"),
            ("web/core/modules/node/node.info.yml", "type: module\n"),
            (
                "web/modules/custom/acme_extra/acme_extra.info.yml",
                "type: module\n",
            ),
        ],
        &[
            ("web/modules/custom/acme/acme.install", install),
            ("web/core/themes/olivero/olivero.theme", theme),
            (
                "web/modules/custom/acme_extra/includes/acme_extra.inc",
                other,
            ),
        ],
    )
    .await;
    let api_uri = uri_for(&dir, api_path);
    let inc = uri_for(
        &dir,
        "web/modules/custom/acme_extra/includes/acme_extra.inc",
    );
    references_at(&backend, &inc, position_in(other, "node_cron", 6)).await;

    for (path, content, needle, line) in [
        (
            "web/modules/custom/acme/acme.install",
            install,
            "acme_install",
            1,
        ),
        (
            "web/core/themes/olivero/olivero.theme",
            theme,
            "olivero_preprocess_HOOK",
            3,
        ),
    ] {
        let at = needle.find('_').unwrap() + 2;
        let found = definitions_at(
            &backend,
            &uri_for(&dir, path),
            position_in(content, needle, at),
        )
        .await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(
            (&found[0].uri, found[0].range.start.line),
            (&api_uri, line),
            "{needle}"
        );
    }

    // `acme_extra_cron` belongs to the file's own extension, `node_cron` to
    // another module of its group.
    let found = references_at(&backend, &api_uri, position_in(api, "cron", 1)).await;
    assert_eq!(
        found,
        vec![(api_uri, 5), (inc.clone(), 1), (inc, 3)],
        "{found:?}"
    );
}
