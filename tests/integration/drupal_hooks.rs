//! Drupal's hooks: the `hook_*()` functions of `*.api.php` files that
//! document them, and the implementations that `#[Hook]` attributes and
//! procedural function names make.

use crate::common::hover_text_at;
use crate::drupal_services::{
    definitions_at, position_in, references_at, uri_for, workspace_in, workspace_opened_early,
};

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
    workspace_in(drupal_dir(extra), open).await
}

/// The directory of [`drupal`]'s workspace, before `open` joins it.
fn drupal_dir(extra: &[(&str, &str)]) -> tempfile::TempDir {
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
    dir
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
    assert!(
        hover.contains("Perform alterations before a form is rendered."),
        "{hover}"
    );
    assert!(hover.contains("function hook_form_alter("), "{hover}");
    assert!(hover.contains("form.api.php"), "{hover}");

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

const INVOKER_STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Extension/ModuleHandlerInterface.php",
        "<?php\nnamespace Drupal\\Core\\Extension;\ninterface ModuleHandlerInterface {\n    public function invokeAll($hook, array $args = []);\n    public function invoke($module, $hook, array $args = []);\n    public function alter($type, &$data, &$context1 = NULL, &$context2 = NULL);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Theme/ThemeManagerInterface.php",
        "<?php\nnamespace Drupal\\Core\\Theme;\ninterface ThemeManagerInterface {\n    public function alter($type, &$data, &$context1 = NULL, &$context2 = NULL);\n}\n",
    ),
];

const INVOKED_API: &str = "<?php\nfunction hook_cron() {\n}\nfunction hook_form_alter(&$form) {\n}\nfunction hook_help($route_name) {\n}\nfunction hook_install() {\n}\n";
const INVOKED_API_PATH: &str = "web/core/core.api.php";

const INVOKED_MODULE: &str = "<?php\nfunction acme_cron() {\n}\nfunction acme_form_alter(&$form) {\n}\nfunction acme_help($route_name) {\n}\n";
const INVOKED_INSTALL: &str = "<?php\nfunction acme_install() {\n}\n";
const INVOKED_THEME: &str = "<?php\nfunction olivero_form_alter(&$form) {\n}\n";

const INVOKER: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Extension\ModuleHandlerInterface;
use Drupal\Core\Theme\ThemeManagerInterface;

final class Invoker {
    public function __construct(private ModuleHandlerInterface $modules, private ThemeManagerInterface $themes) {}

    public function run(array $form): void {
        $this->modules->invokeAll('cron');
        $this->modules->alter('form', $form);
        $this->themes->alter(['form', 'other'], $form);
        $this->modules->invoke('acme', 'help');
        $this->modules->invoke('other', 'help');
        $this->modules->invoke('acme', 'install');
    }
}
"#;
const INVOKER_PATH: &str = "web/modules/custom/acme/src/Invoker.php";

/// An invocation leads to the implementations of its group and extension
/// that Drupal collects, an implementation to the invocations that reach
/// it, and both to the documentation.
#[tokio::test]
async fn invocations_and_implementations_lead_to_each_other() {
    let (backend, dir) = drupal(
        &[
            INVOKER_STUBS[0],
            INVOKER_STUBS[1],
            (INVOKED_API_PATH, INVOKED_API),
            ("web/core/themes/olivero/olivero.info.yml", "type: theme\n"),
        ],
        &[
            ("web/modules/custom/acme/acme.module", INVOKED_MODULE),
            ("web/modules/custom/acme/acme.install", INVOKED_INSTALL),
            ("web/core/themes/olivero/olivero.theme", INVOKED_THEME),
            (INVOKER_PATH, INVOKER),
        ],
    )
    .await;
    let php = uri_for(&dir, INVOKER_PATH);
    let api = uri_for(&dir, INVOKED_API_PATH);
    let module = uri_for(&dir, "web/modules/custom/acme/acme.module");
    let theme = uri_for(&dir, "web/core/themes/olivero/olivero.theme");
    references_at(&backend, &php, position_in(INVOKER, "'cron'", 2)).await;

    let at = |needle: &str| {
        let inside = needle.rfind('\'').unwrap() - 1;
        position_in(INVOKER, needle, inside)
    };
    let lines = |found: Vec<tower_lsp_server::gen_lsp_types::Location>| {
        found
            .into_iter()
            .map(|location| (location.uri, location.range.start.line))
            .collect::<Vec<_>>()
    };
    for (needle, expected) in [
        (
            "invokeAll('cron'",
            vec![(module.clone(), 1), (api.clone(), 1)],
        ),
        (
            "modules->alter('form'",
            vec![(module.clone(), 3), (api.clone(), 3)],
        ),
        (
            "themes->alter(['form'",
            vec![(theme.clone(), 1), (api.clone(), 3)],
        ),
        (
            "invoke('acme', 'help'",
            vec![(module.clone(), 5), (api.clone(), 5)],
        ),
        ("invoke('other', 'help'", vec![(api.clone(), 5)]),
        ("invoke('acme', 'install'", vec![(api.clone(), 7)]),
    ] {
        let found = lines(definitions_at(&backend, &php, at(needle)).await);
        assert_eq!(found, expected, "{needle}");
    }

    let found = lines(
        definitions_at(
            &backend,
            &module,
            position_in(INVOKED_MODULE, "acme_cron", 6),
        )
        .await,
    );
    assert_eq!(found, vec![(php, 10), (api, 1)]);
}

const PATTERN_API: &str = "<?php\nfunction hook_form_FORM_ID_alter(&$form, $form_state, $form_id) {\n}\nfunction hook_ENTITY_TYPE_presave($entity) {\n}\n";
const PATTERN_MODULE: &str = "<?php\nfunction acme_form_user_login_form_alter(&$form, $form_state, $form_id) {\n}\nfunction acme_node_presave($node) {\n}\nfunction acme_widget_presave($widget) {\n}\n";

/// A hook named with parts filled in links to the documented pattern it
/// matches, and the pattern lists the hooks it stands for.
#[tokio::test]
async fn computed_hook_names_reach_their_documented_pattern() {
    let node = "<?php\nnamespace Drupal\\node\\Entity;\n/**\n * @ContentEntityType(\n *   id = \"node\",\n * )\n */\nclass Node {}\n";
    let (backend, dir) = drupal(
        &[
            ("web/core/lib/Drupal/Core/Form/form.api.php", PATTERN_API),
            ("web/core/modules/node/src/Entity/Node.php", node),
        ],
        &[("web/modules/custom/acme/acme.module", PATTERN_MODULE)],
    )
    .await;
    let module = uri_for(&dir, "web/modules/custom/acme/acme.module");
    let api = uri_for(&dir, "web/core/lib/Drupal/Core/Form/form.api.php");
    references_at(
        &backend,
        &module,
        position_in(PATTERN_MODULE, "acme_node", 6),
    )
    .await;

    for (needle, line) in [
        ("acme_form_user_login_form_alter", 1),
        ("acme_node_presave", 3),
    ] {
        let found = definitions_at(&backend, &module, position_in(PATTERN_MODULE, needle, 6)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(
            (&found[0].uri, found[0].range.start.line),
            (&api, line),
            "{needle}"
        );
    }
    // `widget` is no entity type, so the function is a helper and leads to
    // nothing but itself.
    let found = definitions_at(
        &backend,
        &module,
        position_in(PATTERN_MODULE, "acme_widget_presave", 6),
    )
    .await;
    assert!(
        found.iter().all(|location| location.uri == module),
        "{found:?}"
    );

    let found = references_at(
        &backend,
        &api,
        position_in(PATTERN_API, "ENTITY_TYPE_presave", 1),
    )
    .await;
    assert_eq!(found, vec![(api, 3), (module, 3)], "{found:?}");
}

/// A documented pattern lists the hooks an installed core implements, whose
/// files the workspace index keeps no parse of.
#[tokio::test]
async fn a_pattern_lists_the_hooks_an_installed_core_implements() {
    let node = "<?php\nnamespace Drupal\\node\\Entity;\n/**\n * @ContentEntityType(\n *   id = \"node\",\n * )\n */\nclass Node {}\n";
    let node_module = "<?php\nfunction node_node_presave($node) {\n}\n";
    let node_module_path = "web/core/modules/node/node.module";
    let (backend, dir) = drupal(
        &[
            INSTALLED_CORE,
            ("web/core/lib/Drupal/Core/Form/form.api.php", PATTERN_API),
            ("web/core/modules/node/node.info.yml", "type: module\n"),
            ("web/core/modules/node/src/Entity/Node.php", node),
            (node_module_path, node_module),
        ],
        &[],
    )
    .await;
    let api = uri_for(&dir, "web/core/lib/Drupal/Core/Form/form.api.php");

    let found = references_at(
        &backend,
        &api,
        position_in(PATTERN_API, "ENTITY_TYPE_presave", 1),
    )
    .await;
    assert_eq!(
        found,
        vec![(api, 3), (uri_for(&dir, node_module_path), 1)],
        "{found:?}"
    );
}

/// Hooks complete where an implementation is named and where a hook name is
/// written, an alter call's without its `_alter`.
#[tokio::test]
async fn hooks_complete_with_their_documented_signature() {
    let module = "<?php\n\nfunction acme_fo\n";
    let hooks = HOOKS.replace("#[Hook('form_alter')]", "#[Hook('')]");
    let invoker = INVOKER
        .replace("alter('form', $form)", "alter('', $form)")
        .replace("invokeAll('cron')", "invokeAll('')");
    let (backend, dir) = drupal(
        &[
            (FORM_API_PATH, FORM_API),
            HOOK_ATTRIBUTE,
            INVOKER_STUBS[0],
            INVOKER_STUBS[1],
            (
                "web/core/lib/Drupal/Core/Form/FormStateInterface.php",
                "<?php\nnamespace Drupal\\Core\\Form;\ninterface FormStateInterface {}\n",
            ),
        ],
        &[
            ("web/modules/custom/acme/acme.module", module),
            (HOOKS_PATH, hooks.as_str()),
            (INVOKER_PATH, invoker.as_str()),
        ],
    )
    .await;
    let module_uri = uri_for(&dir, "web/modules/custom/acme/acme.module");
    references_at(&backend, &module_uri, position_in(module, "acme_fo", 1)).await;

    let position = position_in(module, "acme_fo\n", 7);
    let response = crate::common::completion_response(
        &backend,
        &module_uri,
        position.line,
        position.character,
        None,
    )
    .await;
    let items = match response {
        Some(tower_lsp_server::gen_lsp_types::CompletionResponse::CompletionItemList(items)) => {
            items
        }
        Some(tower_lsp_server::gen_lsp_types::CompletionResponse::CompletionList(list)) => {
            list.items
        }
        None => Vec::new(),
    };
    let form_alter = items
        .iter()
        .find(|item| item.label == "acme_form_alter")
        .expect("acme_form_alter offered");
    let Some(tower_lsp_server::gen_lsp_types::CompletionItemTextEdit::TextEdit(edit)) =
        &form_alter.text_edit
    else {
        panic!("{form_alter:?}");
    };
    assert_eq!(
        edit.new_text,
        "acme_form_alter(&\\$form, Drupal\\\\Core\\\\Form\\\\FormStateInterface \\$form_state, \\$form_id) {\n\t$1\n}"
    );

    for (path, content, needle, inside, expected) in [
        (HOOKS_PATH, hooks.as_str(), "#[Hook('')]", 8, "form_alter"),
        (INVOKER_PATH, invoker.as_str(), "invokeAll('')", 11, "cron"),
        (
            INVOKER_PATH,
            invoker.as_str(),
            "modules->alter(''",
            16,
            "form",
        ),
    ] {
        let position = position_in(content, needle, inside);
        let labels = crate::common::response_labels(
            crate::common::completion_response(
                &backend,
                &uri_for(&dir, path),
                position.line,
                position.character,
                None,
            )
            .await,
        );
        assert!(
            labels.contains(&expected.to_string()),
            "{needle}: {labels:?}"
        );
    }
}

/// The completion labels right after `typed` in `acme.module`, whose
/// content is `module`, once the documented hooks are known.
async fn module_completion_labels(module: &str, typed: &str) -> Vec<String> {
    let (backend, dir) = drupal(
        &[(FORM_API_PATH, FORM_API)],
        &[("web/modules/custom/acme/acme.module", module)],
    )
    .await;
    let module_uri = uri_for(&dir, "web/modules/custom/acme/acme.module");
    // The same warm-up as the test above, on the first `acme_` name, so
    // the hooks are known.
    references_at(&backend, &module_uri, position_in(module, "acme_", 1)).await;
    let position = position_in(module, typed, typed.len());
    crate::common::response_labels(
        crate::common::completion_response(
            &backend,
            &module_uri,
            position.line,
            position.character,
            None,
        )
        .await,
    )
}

/// A method without a modifier in a `.module` file is not a hook
/// implementation, so its name gets the class-body completion instead: the
/// parent's method it can override.
#[tokio::test]
async fn a_method_in_a_module_file_is_not_offered_hooks() {
    let module = "<?php\n\nclass AcmeBase {\n    public function acme_format(): void {}\n}\n\nclass AcmeHelper extends AcmeBase {\n    function acme_fo\n}\n";
    let labels = module_completion_labels(module, "    function acme_fo").await;
    assert!(
        !labels.iter().any(|label| label == "acme_form_alter"),
        "{labels:?}"
    );
    assert!(
        labels.iter().any(|label| label.starts_with("acme_format(")),
        "{labels:?}"
    );
}

/// A closure on a line of its own inside a call's arguments is not a hook
/// implementation either.
#[tokio::test]
async fn a_closure_in_a_module_file_is_not_offered_hooks() {
    let module =
        "<?php\n\nfunction acme_cron() {}\n\n$callbacks = array_map(\n    function \n, []);\n";
    let labels = module_completion_labels(module, "    function ").await;
    assert!(
        !labels.iter().any(|label| label == "acme_form_alter"),
        "{labels:?}"
    );
}

/// An implementation that leaves a parameter untyped reads it as the hook's
/// documentation types it, unless it types it itself.
#[tokio::test]
async fn untyped_hook_parameters_take_the_documented_types() {
    let api = "<?php\n/**\n * @param array $form\n * @param \\Drupal\\Core\\Form\\FormStateInterface $form_state\n * @param string $form_id\n */\nfunction hook_form_alter(&$form, \\Drupal\\Core\\Form\\FormStateInterface $form_state, $form_id) {\n}\nfunction hook_form_FORM_ID_alter(&$form, \\Drupal\\Core\\Form\\FormStateInterface $form_state, $form_id) {\n}\n";
    let module = "<?php\nfunction acme_form_alter(&$form, $form_state, $form_id) {\n    $form_state;\n}\nfunction acme_form_user_login_alter(&$form, $form_state, $form_id) {\n    $form_state;\n}\n/**\n * @param \\stdClass $form_state\n */\nfunction acme_form_node_form_alter(&$form, $form_state, $form_id) {\n    $form_state;\n}\n";
    let hooks = "<?php\nnamespace Drupal\\acme\\Hook;\n\nuse Drupal\\Core\\Hook\\Attribute\\Hook;\n\nfinal class FormHooks {\n    #[Hook('form_alter')]\n    public function formAlter(array &$form, $form_state): void {\n        $form_state;\n    }\n}\n";
    let (backend, dir) = drupal(
        &[
            (FORM_API_PATH, api),
            HOOK_ATTRIBUTE,
            (
                "web/core/lib/Drupal/Core/Form/FormStateInterface.php",
                "<?php\nnamespace Drupal\\Core\\Form;\ninterface FormStateInterface {}\n",
            ),
        ],
        &[
            ("web/modules/custom/acme/acme.module", module),
            ("web/modules/custom/acme/src/Hook/FormHooks.php", hooks),
        ],
    )
    .await;
    let module_uri = uri_for(&dir, "web/modules/custom/acme/acme.module");
    references_at(
        &backend,
        &module_uri,
        position_in(module, "acme_form_alter", 6),
    )
    .await;

    let hover = |uri: tower_lsp_server::gen_lsp_types::Uri, content: &'static str, nth: usize| {
        let offset = content.match_indices("$form_state;").nth(nth).unwrap().0 + 2;
        let position = crate::common::offset_position(content, offset);
        (uri, position)
    };
    for (uri, content, nth, expected) in [
        (module_uri.clone(), module, 0, "FormStateInterface"),
        (module_uri.clone(), module, 1, "FormStateInterface"),
        (module_uri.clone(), module, 2, "stdClass"),
        (
            uri_for(&dir, "web/modules/custom/acme/src/Hook/FormHooks.php"),
            hooks,
            0,
            "FormStateInterface",
        ),
    ] {
        let (uri, position) = hover(uri, content, nth);
        let text = hover_text_at(&backend, &uri, position.line, position.character)
            .await
            .unwrap_or_default();
        assert!(text.contains(expected), "{nth}: {text}");
    }
}

/// A documented hint the `*.api.php` file imports is completed as the
/// class it names there, written the way the implementing file reaches it.
#[tokio::test]
async fn completed_signatures_read_hints_with_the_api_files_imports() {
    let api = "<?php\nuse Drupal\\Core\\Block\\BlockPluginInterface;\n\nfunction hook_block_view_alter(array &$build, BlockPluginInterface $block) {\n}\n";
    let plain = "<?php\n\nfunction acme_bl\n";
    let imported = "<?php\nuse Drupal\\Core\\Block\\BlockPluginInterface;\n\nfunction acme_bl\n";
    for (module, expected) in [
        (
            plain,
            "acme_block_view_alter(array &\\$build, Drupal\\\\Core\\\\Block\\\\BlockPluginInterface \\$block) {\n\t$1\n}",
        ),
        (
            imported,
            "acme_block_view_alter(array &\\$build, BlockPluginInterface \\$block) {\n\t$1\n}",
        ),
    ] {
        let (backend, dir) = drupal(
            &[
                ("web/core/modules/block/block.api.php", api),
                (
                    "web/core/lib/Drupal/Core/Block/BlockPluginInterface.php",
                    "<?php\nnamespace Drupal\\Core\\Block;\ninterface BlockPluginInterface {}\n",
                ),
            ],
            &[("web/modules/custom/acme/acme.module", module)],
        )
        .await;
        let module_uri = uri_for(&dir, "web/modules/custom/acme/acme.module");
        references_at(&backend, &module_uri, position_in(module, "acme_bl", 1)).await;
        let position = position_in(module, "acme_bl\n", 7);
        let items = match crate::common::completion_response(
            &backend,
            &module_uri,
            position.line,
            position.character,
            None,
        )
        .await
        {
            Some(tower_lsp_server::gen_lsp_types::CompletionResponse::CompletionItemList(
                items,
            )) => items,
            Some(tower_lsp_server::gen_lsp_types::CompletionResponse::CompletionList(list)) => {
                list.items
            }
            None => Vec::new(),
        };
        let item = items
            .iter()
            .find(|item| item.label == "acme_block_view_alter")
            .expect("acme_block_view_alter offered");
        let Some(tower_lsp_server::gen_lsp_types::CompletionItemTextEdit::TextEdit(edit)) =
            &item.text_edit
        else {
            panic!("{item:?}");
        };
        assert_eq!(edit.new_text, expected);
    }
}

/// An implementation of a hook documented for every entity type is passed
/// the entity class of the type its name fills in, where the documentation
/// names an entity interface that class implements; any other parameter
/// keeps its documented type.
#[tokio::test]
async fn entity_type_hooks_take_the_entity_class_of_their_type() {
    let api = "<?php\nfunction hook_ENTITY_TYPE_presave(\\Drupal\\Core\\Entity\\EntityInterface $entity) {\n}\nfunction hook_ENTITY_TYPE_access(\\Drupal\\Core\\Entity\\EntityInterface $entity, $operation, \\Drupal\\Core\\Session\\AccountInterface $account) {\n}\n";
    let module = "<?php\nfunction acme_node_presave($node) {\n    $node;\n}\nfunction acme_user_access($user, $operation, $account) {\n    $user;\n    $account;\n}\n";
    let (backend, dir) = drupal(
        &[
            ("web/core/lib/Drupal/Core/Entity/entity.api.php", api),
            (
                "web/core/lib/Drupal/Core/Entity/EntityInterface.php",
                "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityInterface {}\n",
            ),
            (
                "web/core/lib/Drupal/Core/Session/AccountInterface.php",
                "<?php\nnamespace Drupal\\Core\\Session;\ninterface AccountInterface {}\n",
            ),
            (
                "web/core/modules/node/src/Entity/Node.php",
                "<?php\nnamespace Drupal\\node\\Entity;\n/**\n * @ContentEntityType(\n *   id = \"node\",\n * )\n */\nclass Node implements \\Drupal\\Core\\Entity\\EntityInterface {}\n",
            ),
            (
                "web/core/modules/user/src/Entity/User.php",
                "<?php\nnamespace Drupal\\user\\Entity;\n/**\n * @ContentEntityType(\n *   id = \"user\",\n * )\n */\nclass User implements \\Drupal\\Core\\Entity\\EntityInterface, \\Drupal\\Core\\Session\\AccountInterface {}\n",
            ),
        ],
        &[("web/modules/custom/acme/acme.module", module)],
    )
    .await;
    let module_uri = uri_for(&dir, "web/modules/custom/acme/acme.module");
    references_at(
        &backend,
        &module_uri,
        position_in(module, "acme_node_presave", 6),
    )
    .await;
    for (needle, expected, unexpected) in [
        ("$node;", "Node", "EntityInterface"),
        ("$user;", "User", "EntityInterface"),
        ("$account;", "AccountInterface", "User"),
    ] {
        let position = position_in(module, needle, 2);
        let text = hover_text_at(&backend, &module_uri, position.line, position.character)
            .await
            .unwrap_or_default();
        assert!(
            text.contains(expected) && !text.contains(unexpected),
            "{needle}: {text}"
        );
    }
}

/// A hook implementation the editor opens before the workspace walk finds
/// the Drupal site takes the documented types once it does.
#[tokio::test]
async fn a_hook_file_opened_before_the_walk_ends_takes_the_documented_types() {
    let api = "<?php\n/**\n * @param \\Drupal\\Core\\Form\\FormStateInterface $form_state\n */\nfunction hook_form_alter(&$form, \\Drupal\\Core\\Form\\FormStateInterface $form_state, $form_id) {\n}\n";
    let module =
        "<?php\nfunction acme_form_alter(&$form, $form_state, $form_id) {\n    $form_state;\n}\n";
    let hooks = "<?php\nnamespace Drupal\\acme\\Hook;\n\nuse Drupal\\Core\\Hook\\Attribute\\Hook;\n\nfinal class FormHooks {\n    #[Hook('form_alter')]\n    public function formAlter(array &$form, $form_state): void {\n        $form_state;\n    }\n}\n";
    let module_path = "web/modules/custom/acme/acme.module";
    let hooks_path = "web/modules/custom/acme/src/Hook/FormHooks.php";
    let (backend, dir) = workspace_opened_early(
        drupal_dir(&[
            (FORM_API_PATH, api),
            HOOK_ATTRIBUTE,
            (
                "web/core/lib/Drupal/Core/Form/FormStateInterface.php",
                "<?php\nnamespace Drupal\\Core\\Form;\ninterface FormStateInterface {}\n",
            ),
        ]),
        &[(module_path, module), (hooks_path, hooks)],
    )
    .await;
    let module_uri = uri_for(&dir, module_path);
    // Finding references indexes the workspace, core's hook documentation
    // included.
    references_at(
        &backend,
        &module_uri,
        position_in(module, "acme_form_alter", 6),
    )
    .await;
    for (path, content) in [(module_path, module), (hooks_path, hooks)] {
        let position = position_in(content, "$form_state;", 2);
        let text = hover_text_at(
            &backend,
            &uri_for(&dir, path),
            position.line,
            position.character,
        )
        .await
        .unwrap_or_default();
        assert!(text.contains("FormStateInterface"), "{path}: {text}");
    }
}

const FORM_BUILDER: &str = r#"<?php
namespace Drupal\Core\Form;

class FormBuilder {
    public function prepareForm($form_id, &$form, $form_state) {
        $hooks = ['form'];
        $hooks[] = 'form_' . $form_id;
        $this->moduleHandler->alter($hooks, $form, $form_state, $form_id);
        $this->themeManager->alter($hooks, $form, $form_state, $form_id);
    }
}
"#;

const STORAGES: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Entity/EntityStorageBase.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\nabstract class EntityStorageBase {\n    protected function invokeHook($hook, $entity) {\n        $this->moduleHandler()->invokeAll($this->entityTypeId . '_' . $hook, [$entity]);\n        $this->moduleHandler()->invokeAll('entity_' . $hook, [$entity]);\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Config/Entity/ConfigEntityStorage.php",
        "<?php\nnamespace Drupal\\Core\\Config\\Entity;\nclass ConfigEntityStorage extends \\Drupal\\Core\\Entity\\EntityStorageBase {\n    protected function invokeHook($hook, $entity) {\n        $this->moduleHandler()->invokeAll($this->entityTypeId . '_' . $hook, [$entity]);\n    }\n}\n",
    ),
    (
        "web/core/modules/node/src/Entity/Node.php",
        "<?php\nnamespace Drupal\\node\\Entity;\n/**\n * @ContentEntityType(\n *   id = \"node\",\n * )\n */\nclass Node {}\n",
    ),
];

/// An implementation of a computed hook reaches the call in core that
/// fires it: the module handler's for a module, and the storage that
/// stores its entity type.
#[tokio::test]
async fn computed_hooks_reach_the_core_call_that_fires_them() {
    computed_hooks_reach_the_core_call(&[]).await;
}

/// Composer's record of core installed into the web root, which makes the
/// workspace index let go of core's files once it has read them.
const INSTALLED_CORE: (&str, &str) = (
    "vendor/composer/installed.json",
    r#"{ "packages": [ { "name": "drupal/core", "type": "drupal-core", "install-path": "../../web/core" } ] }"#,
);

#[tokio::test]
async fn computed_hooks_reach_the_core_call_of_an_installed_core() {
    computed_hooks_reach_the_core_call(&[INSTALLED_CORE]).await;
}

async fn computed_hooks_reach_the_core_call(extra: &[(&str, &str)]) {
    let builder_path = "web/core/lib/Drupal/Core/Form/FormBuilder.php";
    let files: Vec<(&str, &str)> = [
        ("web/core/lib/Drupal/Core/Form/form.api.php", PATTERN_API),
        (builder_path, FORM_BUILDER),
        STORAGES[0],
        STORAGES[1],
        STORAGES[2],
    ]
    .into_iter()
    .chain(extra.iter().copied())
    .collect();
    let (backend, dir) = drupal(
        &files,
        &[("web/modules/custom/acme/acme.module", PATTERN_MODULE)],
    )
    .await;
    let module = uri_for(&dir, "web/modules/custom/acme/acme.module");
    references_at(
        &backend,
        &module,
        position_in(PATTERN_MODULE, "acme_node", 6),
    )
    .await;
    let lines = |found: Vec<tower_lsp_server::gen_lsp_types::Location>| {
        found
            .into_iter()
            .map(|location| (location.uri, location.range.start.line))
            .collect::<Vec<_>>()
    };
    let api = uri_for(&dir, "web/core/lib/Drupal/Core/Form/form.api.php");

    let found = lines(
        definitions_at(
            &backend,
            &module,
            position_in(PATTERN_MODULE, "acme_form_user_login_form_alter", 6),
        )
        .await,
    );
    assert_eq!(
        found,
        vec![(uri_for(&dir, builder_path), 7), (api.clone(), 1)]
    );

    let found = lines(
        definitions_at(
            &backend,
            &module,
            position_in(PATTERN_MODULE, "acme_node_presave", 6),
        )
        .await,
    );
    assert_eq!(
        found,
        vec![(uri_for(&dir, STORAGES[0].0), 4), (api, 3)],
        "only the content storage stores nodes"
    );
}

/// A function named after its module that no documentation and no
/// invocation backs is a helper: its whole name is the function's.  Hover
/// on an implementation names its documentation, a pattern's included, and
/// a hook matching two patterns is documented by the one that writes out
/// more of it.
#[tokio::test]
async fn helpers_stay_functions_and_hooks_reach_their_most_specific_documentation() {
    let api = "<?php\nfunction hook_theme_suggestions_HOOK(array $variables) {\n}\nfunction hook_theme_suggestions_HOOK_alter(array &$suggestions, array $variables, $hook) {\n}\nfunction hook_cron() {\n}\n";
    let api_path = "web/core/lib/Drupal/Core/Render/theme.api.php";
    let module = "<?php\nfunction acme_helper() {\n}\nfunction acme_cron() {\n    acme_helper();\n}\nfunction acme_theme_suggestions_node_alter(array &$suggestions, $variables, $hook) {\n}\n";
    let (backend, dir) = drupal(
        &[(api_path, api)],
        &[("web/modules/custom/acme/acme.module", module)],
    )
    .await;
    let php = uri_for(&dir, "web/modules/custom/acme/acme.module");
    let api_uri = uri_for(&dir, api_path);
    references_at(&backend, &php, position_in(module, "acme_cron", 6)).await;

    let found = references_at(&backend, &php, position_in(module, "acme_helper()", 7)).await;
    assert_eq!(found, vec![(php.clone(), 1), (php.clone(), 4)], "{found:?}");

    let found = definitions_at(
        &backend,
        &php,
        position_in(module, "acme_theme_suggestions_node_alter", 6),
    )
    .await;
    let lines: Vec<_> = found
        .iter()
        .map(|l| (l.uri.clone(), l.range.start.line))
        .collect();
    assert_eq!(lines, vec![(api_uri, 3)]);

    let position = position_in(module, "acme_theme_suggestions_node_alter", 7);
    let hover = hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("theme.api.php"), "{hover}");
}

/// Only a function or a method implements a hook: a property hook's
/// parameter keeps no type when a hook implementation follows it.
#[tokio::test]
async fn a_property_hook_is_no_hook_implementation() {
    let api = "<?php\n/**\n * @param array $form\n */\nfunction hook_form_alter(&$form, $form_state, $form_id) {\n}\n";
    let module = "<?php\nclass AcmeCounter {\n    public int $count {\n        set($value) {\n            $value;\n            $this->count = 1;\n        }\n    }\n}\nfunction acme_form_alter(&$form, $form_state, $form_id) {\n}\n";
    let (backend, dir) = drupal(
        &[(FORM_API_PATH, api)],
        &[("web/modules/custom/acme/acme.module", module)],
    )
    .await;
    let php = uri_for(&dir, "web/modules/custom/acme/acme.module");
    references_at(&backend, &php, position_in(module, "acme_form_alter", 6)).await;
    let position = position_in(module, "$value;", 2);
    let hover = hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(!hover.contains("array"), "{hover}");
}

/// Hover anywhere on a procedural implementation's name shows the
/// documentation of the hook it implements, as a function's hover would,
/// with the file that documents it.
#[tokio::test]
async fn hover_on_an_implementation_shows_the_hooks_documentation() {
    let api = "<?php\n/**\n * Register a module or theme's theme implementations.\n *\n * @param array $existing\n *   An array of existing implementations.\n *\n * @return array\n *   An associative array of information about theme implementations.\n */\nfunction hook_theme($existing, $type, $theme, $path) {\n}\n";
    let api_path = "web/core/lib/Drupal/Core/Render/theme.api.php";
    let module = "<?php\nfunction acme_theme($existing, $type, $theme, $path) {\n    return [];\n}\nfunction acme_helper() {\n}\n";
    let (backend, dir) = drupal(
        &[(api_path, api)],
        &[("web/modules/custom/acme/acme.module", module)],
    )
    .await;
    let php = uri_for(&dir, "web/modules/custom/acme/acme.module");
    references_at(&backend, &php, position_in(module, "acme_theme", 6)).await;

    for at in [7, 1] {
        let position = position_in(module, "acme_theme", at);
        let hover = hover_text_at(&backend, &php, position.line, position.character)
            .await
            .unwrap_or_default();
        for expected in [
            "**Hook** `theme`",
            "Register a module or theme's theme implementations.",
            "$existing",
            "An associative array of information about theme implementations.",
            "function hook_theme($existing, $type, $theme, $path)",
            "theme.api.php",
        ] {
            assert!(hover.contains(expected), "{at}: {expected}: {hover}");
        }
    }

    // A helper named after its module is no implementation.
    let position = position_in(module, "acme_helper", 1);
    let hover = hover_text_at(&backend, &php, position.line, position.character).await;
    assert!(hover.is_none(), "{hover:?}");
}
