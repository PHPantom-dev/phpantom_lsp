//! Drupal's theme hooks: the names a `hook_theme()` implementation
//! declares, the templates that draw them, and the render arrays that name
//! them in `#theme` and `#theme_wrappers`.

use crate::common::{completion_response, hover_text_at, response_labels};
use crate::drupal_services::{definitions_at, position_in, uri_for, workspace_in};
use tower_lsp_server::gen_lsp_types::*;

const HOOK_ATTRIBUTE: (&str, &str) = (
    "web/core/lib/Drupal/Core/Hook/Attribute/Hook.php",
    "<?php\nnamespace Drupal\\Core\\Hook\\Attribute;\n#[\\Attribute]\nclass Hook {\n    public function __construct(public string $hook, public string $method = '', public ?string $module = NULL) {}\n}\n",
);

const COMMON_ELEMENTS: &str = r#"<?php
namespace Drupal\Core\Theme;

class ThemeCommonElements {
  public static function commonElements(): array {
    return [
      'item_list' => [
        'variables' => [
          'items' => [],
          'title' => '',
          'list_type' => 'ul',
        ],
      ],
      'table' => [
        'variables' => [
          'header' => NULL,
          'rows' => NULL,
        ],
      ],
    ];
  }
}
"#;
const COMMON_ELEMENTS_PATH: &str = "web/core/lib/Drupal/Core/Theme/ThemeCommonElements.php";

const SYSTEM_HOOKS: &str = r#"<?php
namespace Drupal\system\Hook;

use Drupal\Core\Hook\Attribute\Hook;
use Drupal\Core\Theme\ThemeCommonElements;

class SystemThemeHooks {
  #[Hook('theme')]
  public function theme(): array {
    $themeCommonElements = ThemeCommonElements::commonElements();
    $systemTheme = [
      'block__system_branding_block' => [
        'render element' => 'elements',
        'base hook' => 'block',
      ],
      'confirm_form' => [
        'render element' => 'form',
      ],
    ];
    return array_merge($themeCommonElements, $systemTheme);
  }
}
"#;
const SYSTEM_HOOKS_PATH: &str = "web/core/modules/system/src/Hook/SystemThemeHooks.php";

const ACME_HOOKS: &str = r#"<?php
namespace Drupal\acme\Hook;

use Drupal\Core\Hook\Attribute\Hook;

final class AcmeHooks {
  #[Hook('theme')]
  public function theme(): array {
    $hooks = [
      'acme_greeting' => [
        'variables' => [
          'name' => NULL,
          'greeting' => NULL,
        ],
      ],
      'acme_form' => [
        'render element' => 'form',
      ],
    ];
    return $hooks + [
      'acme_panel' => [
        'variables' => ['label' => NULL],
        'template' => 'panel',
        'path' => 'modules/custom/acme/layouts',
      ],
    ];
  }
}
"#;
const ACME_HOOKS_PATH: &str = "web/modules/custom/acme/src/Hook/AcmeHooks.php";

const EXAMPLE_MODULE: &str = r#"<?php

function example_theme($existing, $type, $theme, $path) {
  return [
    'example_box' => [
      'variables' => ['label' => NULL],
    ],
  ];
}
"#;
const EXAMPLE_MODULE_PATH: &str = "web/modules/custom/example/example.module";

const GREETING_TEMPLATE_PATH: &str = "web/modules/custom/acme/templates/acme-greeting.html.twig";
const ITEM_LIST_TEMPLATE_PATH: &str = "web/core/modules/system/templates/item-list.html.twig";
const PANEL_TEMPLATE_PATH: &str = "web/modules/custom/acme/layouts/panel.html.twig";
const OVERRIDE_PATH: &str = "web/themes/custom/olivo/templates/content/acme-greeting.html.twig";
const SUGGESTION_PATH: &str = "web/themes/custom/olivo/templates/acme-greeting--compact.html.twig";

/// The files of a site whose `.gitignore` hides core, as a Composer install
/// of Drupal does: core's system module and its common elements, the
/// modules `acme` and `example`, and the theme `olivo`.
fn site() -> Vec<(&'static str, &'static str)> {
    vec![
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        HOOK_ATTRIBUTE,
        (COMMON_ELEMENTS_PATH, COMMON_ELEMENTS),
        (
            "web/core/modules/system/system.info.yml",
            "name: System\ntype: module\n",
        ),
        (SYSTEM_HOOKS_PATH, SYSTEM_HOOKS),
        (ITEM_LIST_TEMPLATE_PATH, "<ul>{{ items }}</ul>\n"),
        (
            "web/modules/custom/acme/acme.info.yml",
            "name: Acme\ntype: module\n",
        ),
        (ACME_HOOKS_PATH, ACME_HOOKS),
        (GREETING_TEMPLATE_PATH, "<p>{{ greeting }}</p>\n"),
        (PANEL_TEMPLATE_PATH, "<div>{{ label }}</div>\n"),
        (
            "web/modules/custom/example/example.info.yml",
            "name: Example\ntype: module\n",
        ),
        (EXAMPLE_MODULE_PATH, EXAMPLE_MODULE),
        (
            "web/themes/custom/olivo/olivo.info.yml",
            "name: Olivo\ntype: theme\n",
        ),
        (OVERRIDE_PATH, "<p class=\"olivo\">{{ greeting }}</p>\n"),
        (SUGGESTION_PATH, "<small>{{ greeting }}</small>\n"),
    ]
}

/// An initialized site holding [`site`] on disk and `open` open.
async fn drupal(open: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    drupal_with(&[], open).await
}

/// [`drupal`] with `extra` on disk as well.
async fn drupal_with(
    extra: &[(&str, &str)],
    open: &[(&str, &str)],
) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in site().into_iter().chain(extra.iter().copied()) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    workspace_in(dir, open).await
}

const CONTROLLER: &str = r#"<?php
namespace Drupal\acme\Controller;

final class GreetingController {
  public function greet(string $name): array {
    $build = [
      '#theme' => 'acme_greeting',
      '#name' => $name,
      '#greeting' => 'Hello',
    ];
    $build['list'] = [
      '#theme' => ['acme_greeting__compact', 'item_list'],
      '#items' => [],
    ];
    $build['form'] = [
      '#theme' => 'acme_form',
      '#theme_wrappers' => ['acme_panel'],
    ];
    $build['box']['#theme'] = 'example_box';
    $build['compact'] = ['#theme' => 'acme_greeting__compact', '#name' => $name];
    return $build;
  }
}
"#;
const CONTROLLER_PATH: &str = "web/modules/custom/acme/src/Controller/GreetingController.php";

/// The labels completion offers at `position` of the open file `uri`.
async fn labels_at(backend: &phpantom_lsp::Backend, uri: &Uri, position: Position) -> Vec<String> {
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

/// Completion is the first request: the hooks of core's closed files are
/// read without a references search having indexed the workspace first.
#[tokio::test]
async fn theme_hook_names_complete_in_theme_and_theme_wrappers() {
    let (backend, dir) = drupal(&[(CONTROLLER_PATH, CONTROLLER)]).await;
    let php = uri_for(&dir, CONTROLLER_PATH);
    for (needle, inside) in [
        ("'#theme' => 'acme_greeting'", 13),
        ("'acme_greeting__compact', 'item_list'", 1),
        ("['acme_panel']", 2),
        ("['#theme'] = 'example_box'", 14),
    ] {
        let labels = labels_at(&backend, &php, position_in(CONTROLLER, needle, inside)).await;
        for expected in [
            "acme_greeting",
            "acme_form",
            "acme_panel",
            "example_box",
            "item_list",
            "table",
            "confirm_form",
        ] {
            assert!(
                labels.contains(&expected.to_string()),
                "{needle}: {expected} in {labels:?}"
            );
        }
    }
}

/// The files and lines go-to-definition lists at `needle`, `inside` bytes
/// in.
async fn definitions(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    needle: &str,
    inside: usize,
) -> Vec<(String, u32)> {
    let php = uri_for(dir, CONTROLLER_PATH);
    locations(backend, dir, &php, position_in(CONTROLLER, needle, inside)).await
}

/// The files, relative to `dir`, and lines go-to-definition lists at
/// `position` of the open file `uri`.
async fn locations(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    uri: &Uri,
    position: Position,
) -> Vec<(String, u32)> {
    let root = Uri::from_file_path(dir.path()).unwrap().to_string();
    definitions_at(backend, uri, position)
        .await
        .into_iter()
        .map(|location| {
            let uri = location.uri.to_string();
            let relative = uri
                .strip_prefix(&root)
                .unwrap_or(&uri)
                .trim_start_matches('/');
            (relative.to_string(), location.range.start.line)
        })
        .collect()
}

#[tokio::test]
async fn a_theme_hook_leads_to_its_entry_and_its_templates() {
    let (backend, dir) = drupal(&[(CONTROLLER_PATH, CONTROLLER)]).await;
    let line_of = |content: &str, needle: &str| {
        content[..content.find(needle).unwrap()]
            .matches('\n')
            .count() as u32
    };
    let greeting_entry = (
        ACME_HOOKS_PATH.to_string(),
        line_of(ACME_HOOKS, "'acme_greeting' =>"),
    );
    assert_eq!(
        definitions(&backend, &dir, "'#theme' => 'acme_greeting'", 14).await,
        vec![
            greeting_entry.clone(),
            (GREETING_TEMPLATE_PATH.to_string(), 0),
            (OVERRIDE_PATH.to_string(), 0),
        ]
    );
    // A suggestion leads to the hook it falls back to, and to a theme's
    // template for the suggestion itself.
    assert_eq!(
        definitions(&backend, &dir, "'acme_greeting__compact', 'item_list'", 2).await,
        vec![
            greeting_entry,
            (GREETING_TEMPLATE_PATH.to_string(), 0),
            (SUGGESTION_PATH.to_string(), 0),
        ]
    );
    // Core's hooks and templates sit in a folder `.gitignore` hides.
    assert_eq!(
        definitions(&backend, &dir, "'item_list']", 2).await,
        vec![
            (
                COMMON_ELEMENTS_PATH.to_string(),
                line_of(COMMON_ELEMENTS, "'item_list'")
            ),
            (ITEM_LIST_TEMPLATE_PATH.to_string(), 0),
        ]
    );
    // A literal `path` and a `template` place the template.
    assert_eq!(
        definitions(&backend, &dir, "['acme_panel']", 3).await,
        vec![
            (
                ACME_HOOKS_PATH.to_string(),
                line_of(ACME_HOOKS, "'acme_panel'")
            ),
            (PANEL_TEMPLATE_PATH.to_string(), 0),
        ]
    );
    // A procedural `hook_theme()` declares hooks too.
    assert_eq!(
        definitions(&backend, &dir, "= 'example_box'", 4).await,
        vec![(
            EXAMPLE_MODULE_PATH.to_string(),
            line_of(EXAMPLE_MODULE, "'example_box'")
        )]
    );
}

#[tokio::test]
async fn a_theme_hook_hover_shows_its_extension_template_and_variables() {
    let (backend, dir) = drupal(&[(CONTROLLER_PATH, CONTROLLER)]).await;
    let php = uri_for(&dir, CONTROLLER_PATH);
    let hover = |needle: &'static str, inside: usize| {
        let position = position_in(CONTROLLER, needle, inside);
        let backend = &backend;
        let php = php.clone();
        async move {
            hover_text_at(backend, &php, position.line, position.character)
                .await
                .unwrap_or_default()
        }
    };
    let text = hover("'#theme' => 'acme_greeting'", 14).await;
    for expected in [
        "Theme hook",
        "Declared by `acme`",
        "acme-greeting.html.twig",
        "`name`, `greeting`",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
    let text = hover("'acme_greeting__compact', 'item_list'", 2).await;
    assert!(text.contains("Suggestion of `acme_greeting`"), "{text}");
    let text = hover("'item_list']", 2).await;
    assert!(text.contains("Declared by `system`"), "{text}");
    assert!(text.contains("item-list.html.twig"), "{text}");
    let text = hover("'#theme' => 'acme_form'", 14).await;
    assert!(text.contains("Render element `form`"), "{text}");
}

#[tokio::test]
async fn a_theme_hooks_variables_complete_and_navigate_as_hash_keys() {
    let (backend, dir) = drupal(&[(CONTROLLER_PATH, CONTROLLER)]).await;
    let php = uri_for(&dir, CONTROLLER_PATH);
    let labels = labels_at(&backend, &php, position_in(CONTROLLER, "'#greeting' =>", 1)).await;
    assert_eq!(labels, ["#name", "#greeting"]);
    let labels = labels_at(
        &backend,
        &php,
        position_in(CONTROLLER, "'#name' => $name]", 3),
    )
    .await;
    assert_eq!(labels, ["#name"]);
    // A hook with a render element has no variables to pass.
    let labels = labels_at(
        &backend,
        &php,
        position_in(CONTROLLER, "'#theme_wrappers'", 1),
    )
    .await;
    assert!(labels.is_empty(), "{labels:?}");

    let line_of = |needle: &str| {
        ACME_HOOKS[..ACME_HOOKS.find(needle).unwrap()]
            .matches('\n')
            .count() as u32
    };
    assert_eq!(
        definitions(&backend, &dir, "'#greeting' =>", 3).await,
        vec![(ACME_HOOKS_PATH.to_string(), line_of("'greeting' =>"))]
    );
    assert_eq!(
        definitions(&backend, &dir, "'#name' => $name]", 3).await,
        vec![(ACME_HOOKS_PATH.to_string(), line_of("'name' =>"))]
    );
    let position = position_in(CONTROLLER, "'#greeting' =>", 3);
    let text = hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(text.contains("Theme hook `acme_greeting`"), "{text}");
}

/// A site whose system module merges `drupal_common_theme()`, as it did
/// before Drupal 11.1, from a `theme.inc` holding `theme_inc`.
async fn common_theme_site(theme_inc: &str) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let system_hooks = "<?php\nnamespace Drupal\\system\\Hook;\n\nuse Drupal\\Core\\Hook\\Attribute\\Hook;\n\nclass SystemThemeHooks {\n  #[Hook('theme')]\n  public function theme(): array {\n    return ['confirm_form' => ['render element' => 'form']];\n  }\n}\n";
    let system_module = "<?php\n\nfunction system_theme() {\n  return array_merge(drupal_common_theme(), [\n    'system_box' => ['variables' => []],\n  ]);\n}\n";
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in site().into_iter().chain([
        (SYSTEM_HOOKS_PATH, system_hooks),
        ("web/core/modules/system/system.module", system_module),
        ("web/core/includes/theme.inc", theme_inc),
    ]) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    workspace_in(dir, &[(CONTROLLER_PATH, CONTROLLER)]).await
}

#[tokio::test]
async fn drupal_common_theme_declares_cores_common_hooks() {
    // Drupal 10 returns the hooks from the function itself.
    let theme_inc = "<?php\n\nfunction drupal_common_theme() {\n  return [\n    'links' => [\n      'variables' => ['links' => [], 'heading' => []],\n    ],\n    'html' => ['render element' => 'html'],\n  ];\n}\n";
    let (backend, dir) = common_theme_site(theme_inc).await;
    let php = uri_for(&dir, CONTROLLER_PATH);
    let at = position_in(CONTROLLER, "'#theme' => 'acme_greeting'", 13);
    let labels = labels_at(&backend, &php, at).await;
    for expected in ["links", "html", "system_box", "confirm_form"] {
        assert!(
            labels.contains(&expected.to_string()),
            "{expected} in {labels:?}"
        );
    }
    assert!(!labels.contains(&"item_list".to_string()), "{labels:?}");

    // Drupal 11.1 hands them on to `ThemeCommonElements`.
    let theme_inc = "<?php\n\nuse Drupal\\Core\\Theme\\ThemeCommonElements;\n\nfunction drupal_common_theme(): array {\n  return ThemeCommonElements::commonElements();\n}\n";
    let (backend, dir) = common_theme_site(theme_inc).await;
    let php = uri_for(&dir, CONTROLLER_PATH);
    let labels = labels_at(&backend, &php, at).await;
    for expected in ["item_list", "table", "system_box"] {
        assert!(
            labels.contains(&expected.to_string()),
            "{expected} in {labels:?}"
        );
    }
}

/// An edit reads only the file it changed again, and a watched event on a
/// file whose text did not change reads nothing.
#[tokio::test]
async fn only_a_changed_implementation_is_read_again() {
    let (backend, dir) =
        drupal(&[(CONTROLLER_PATH, CONTROLLER), (ACME_HOOKS_PATH, ACME_HOOKS)]).await;
    let php = uri_for(&dir, CONTROLLER_PATH);
    let at = position_in(CONTROLLER, "'#theme' => 'acme_greeting'", 13);
    let has = |labels: &[String], name: &str| labels.iter().any(|label| label == name);
    assert!(has(&labels_at(&backend, &php, at).await, "example_box"));

    // A hook written to disk unannounced shows only once its file is read.
    let unannounced = EXAMPLE_MODULE.replace(
        "'example_box' =>",
        "'example_extra' => [],\n    'example_box' =>",
    );
    std::fs::write(dir.path().join(EXAMPLE_MODULE_PATH), &unannounced).unwrap();

    // The editor regaining focus reports an implementation it did not change.
    watched(&backend, &dir, SYSTEM_HOOKS_PATH, FileChangeType::Changed).await;
    let labels = labels_at(&backend, &php, at).await;
    assert!(!has(&labels, "example_extra"), "{labels:?}");

    // An edit to another implementation reads that one alone.
    let edited = ACME_HOOKS.replace(
        "'acme_form' =>",
        "'acme_banner' => [],\n      'acme_form' =>",
    );
    edit(&backend, &uri_for(&dir, ACME_HOOKS_PATH), &edited).await;
    let labels = labels_at(&backend, &php, at).await;
    assert!(has(&labels, "acme_banner"), "{labels:?}");
    assert!(has(&labels, "item_list"), "{labels:?}");
    assert!(!has(&labels, "example_extra"), "{labels:?}");

    // A change on disk reads them all.
    watched(&backend, &dir, EXAMPLE_MODULE_PATH, FileChangeType::Changed).await;
    let labels = labels_at(&backend, &php, at).await;
    assert!(has(&labels, "example_extra"), "{labels:?}");
    assert!(has(&labels, "acme_banner"), "{labels:?}");
}

/// Replace the whole text of the open document `uri`.
async fn edit(backend: &phpantom_lsp::Backend, uri: &Uri, text: &str) {
    use tower_lsp_server::LanguageServer;
    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                text_document_identifier: TextDocumentIdentifier { uri: uri.clone() },
                version: 2,
            },
            content_changes: vec![
                TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                    TextDocumentContentChangeWholeDocument {
                        text: text.to_string(),
                    },
                ),
            ],
        })
        .await;
}

/// Tell the backend that `path` under `dir` was created or deleted.
async fn watched(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    path: &str,
    kind: FileChangeType,
) {
    use tower_lsp_server::LanguageServer;
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: uri_for(dir, path),
                kind,
            }],
        })
        .await;
}

#[tokio::test]
async fn editing_an_implementation_or_a_template_refreshes_the_hooks() {
    let (backend, dir) =
        drupal(&[(CONTROLLER_PATH, CONTROLLER), (ACME_HOOKS_PATH, ACME_HOOKS)]).await;
    let php = uri_for(&dir, CONTROLLER_PATH);
    let at = position_in(CONTROLLER, "'#theme' => 'acme_greeting'", 13);
    assert!(
        !labels_at(&backend, &php, at)
            .await
            .contains(&"acme_banner".to_string())
    );

    // An edit to an open implementation.
    let edited = ACME_HOOKS.replace(
        "'acme_form' =>",
        "'acme_banner' => [],\n      'acme_form' =>",
    );
    edit(&backend, &uri_for(&dir, ACME_HOOKS_PATH), &edited).await;
    assert!(
        labels_at(&backend, &php, at)
            .await
            .contains(&"acme_banner".to_string())
    );

    // A new implementation written to disk.
    let extra_path = "web/modules/custom/acme/src/Hook/ExtraHooks.php";
    let extra = "<?php\nnamespace Drupal\\acme\\Hook;\n\nuse Drupal\\Core\\Hook\\Attribute\\Hook;\n\nfinal class ExtraHooks {\n  #[Hook('theme')]\n  public function theme(): array {\n    return ['acme_extra' => ['variables' => []]];\n  }\n}\n";
    std::fs::write(dir.path().join(extra_path), extra).unwrap();
    watched(&backend, &dir, extra_path, FileChangeType::Created).await;
    assert!(
        labels_at(&backend, &php, at)
            .await
            .contains(&"acme_extra".to_string())
    );

    // A theme's override deleted, and another created.
    std::fs::remove_file(dir.path().join(OVERRIDE_PATH)).unwrap();
    watched(&backend, &dir, OVERRIDE_PATH, FileChangeType::Deleted).await;
    let found = definitions(&backend, &dir, "'#theme' => 'acme_greeting'", 14).await;
    assert!(
        !found.iter().any(|(path, _)| path == OVERRIDE_PATH),
        "{found:?}"
    );
    let form_override = "web/themes/custom/olivo/templates/acme-form.html.twig";
    std::fs::write(dir.path().join(form_override), "{{ form }}\n").unwrap();
    watched(&backend, &dir, form_override, FileChangeType::Created).await;
    let found = definitions(&backend, &dir, "'#theme' => 'acme_form'", 14).await;
    assert!(
        found.iter().any(|(path, _)| path == form_override),
        "{found:?}"
    );
}

/// The variables completion offers inside the `{{ }}` of the template
/// `path` whose text is `template`, opened.
async fn template_variables(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    path: &str,
    template: &str,
) -> Vec<String> {
    let uri = uri_for(dir, path);
    crate::common::open_document(backend, &uri, "twig", template).await;
    labels_at(backend, &uri, position_in(template, "{{ }}", 3)).await
}

/// The variables Drupal adds to every template a theme hook draws.
const DEFAULT_VARIABLES: [&str; 11] = [
    "attributes",
    "title_attributes",
    "content_attributes",
    "title_prefix",
    "title_suffix",
    "db_is_active",
    "is_admin",
    "logged_in",
    "user",
    "directory",
    "theme_hook_original",
];

#[tokio::test]
async fn a_core_template_receives_its_hooks_variables_and_drupals_defaults() {
    let (backend, dir) = drupal(&[]).await;
    let labels =
        template_variables(&backend, &dir, ITEM_LIST_TEMPLATE_PATH, "<ul>{{ }}</ul>\n").await;
    for expected in ["items", "title", "list_type"]
        .into_iter()
        .chain(DEFAULT_VARIABLES)
    {
        assert!(
            labels.contains(&expected.to_string()),
            "{expected} in {labels:?}"
        );
    }
    assert!(!labels.contains(&"rows".to_string()), "{labels:?}");
}

#[tokio::test]
async fn a_default_template_and_an_override_receive_the_hooks_variables() {
    let (backend, dir) = drupal(&[(CONTROLLER_PATH, CONTROLLER)]).await;
    let line_of = |needle: &str| {
        ACME_HOOKS[..ACME_HOOKS.find(needle).unwrap()]
            .matches('\n')
            .count() as u32
    };
    let controller_line = CONTROLLER[..CONTROLLER.find("'#greeting'").unwrap()]
        .matches('\n')
        .count() as u32;
    for path in [GREETING_TEMPLATE_PATH, OVERRIDE_PATH] {
        let template = "<p>{{ }}</p>\n<p>{{ greeting }}</p>\n";
        let labels = template_variables(&backend, &dir, path, template).await;
        for expected in ["name", "greeting"].into_iter().chain(DEFAULT_VARIABLES) {
            assert!(
                labels.contains(&expected.to_string()),
                "{path}: {expected} in {labels:?}"
            );
        }
        // The declaration comes first, then the render arrays that pass it.
        let found = locations(
            &backend,
            &dir,
            &uri_for(&dir, path),
            position_in(template, "greeting }}", 1),
        )
        .await;
        assert_eq!(
            found,
            vec![
                (ACME_HOOKS_PATH.to_string(), line_of("'greeting' =>")),
                (CONTROLLER_PATH.to_string(), controller_line),
            ],
            "{path}"
        );
        let position = position_in(template, "greeting }}", 1);
        let text = hover_text_at(
            &backend,
            &uri_for(&dir, path),
            position.line,
            position.character,
        )
        .await
        .unwrap_or_default();
        assert!(
            text.contains("Declared by the theme hook `acme_greeting`"),
            "{path}: {text}"
        );
    }
}

#[tokio::test]
async fn a_suggestion_template_receives_what_the_suggestion_and_its_hook_pass() {
    let (backend, dir) = drupal(&[(CONTROLLER_PATH, CONTROLLER)]).await;
    let template = "<small>{{ }}</small>\n<small>{{ name }}</small>\n";
    let labels = template_variables(&backend, &dir, SUGGESTION_PATH, template).await;
    for expected in ["name", "greeting"].into_iter().chain(DEFAULT_VARIABLES) {
        assert!(
            labels.contains(&expected.to_string()),
            "{expected} in {labels:?}"
        );
    }
    let line_in = |content: &str, needle: &str| {
        content[..content.find(needle).unwrap()]
            .matches('\n')
            .count() as u32
    };
    let found = locations(
        &backend,
        &dir,
        &uri_for(&dir, SUGGESTION_PATH),
        position_in(template, "name }}", 1),
    )
    .await;
    assert_eq!(
        found,
        vec![
            (
                ACME_HOOKS_PATH.to_string(),
                line_in(ACME_HOOKS, "'name' =>")
            ),
            (
                CONTROLLER_PATH.to_string(),
                line_in(CONTROLLER, "'#name' => $name,")
            ),
            (
                CONTROLLER_PATH.to_string(),
                line_in(CONTROLLER, "'#name' => $name]")
            ),
        ]
    );
}

/// A suggestion no theme writes a template for is drawn with its hook's
/// templates, which take what its render arrays pass; one a theme writes a
/// template for passes nothing to them.
#[tokio::test]
async fn a_hooks_templates_receive_what_its_unclaimed_suggestions_pass() {
    let wide_path = "web/modules/custom/acme/src/Controller/WideController.php";
    let wide = "<?php\nnamespace Drupal\\acme\\Controller;\n\nfinal class WideController {\n  public function wide(string $who): array {\n    return ['#theme' => 'acme_greeting__wide', '#name' => $who];\n  }\n}\n";
    let (backend, dir) = drupal(&[(CONTROLLER_PATH, CONTROLLER), (wide_path, wide)]).await;
    let line_in = |content: &str, needle: &str| {
        content[..content.find(needle).unwrap()]
            .matches('\n')
            .count() as u32
    };
    let template = "<p>{{ name }}</p>\n";
    crate::common::open_document(
        &backend,
        &uri_for(&dir, GREETING_TEMPLATE_PATH),
        "twig",
        template,
    )
    .await;
    let found = locations(
        &backend,
        &dir,
        &uri_for(&dir, GREETING_TEMPLATE_PATH),
        position_in(template, "name }}", 1),
    )
    .await;
    assert_eq!(
        found,
        vec![
            (
                ACME_HOOKS_PATH.to_string(),
                line_in(ACME_HOOKS, "'name' =>")
            ),
            (
                CONTROLLER_PATH.to_string(),
                line_in(CONTROLLER, "'#name' => $name,")
            ),
            (wide_path.to_string(), line_in(wide, "'#name'")),
        ]
    );
}

#[tokio::test]
async fn a_render_element_template_receives_the_element() {
    let (backend, dir) = drupal(&[]).await;
    let path = "web/modules/custom/acme/templates/acme-form.html.twig";
    let template = "{{ }}\n{{ form }}\n";
    let labels = template_variables(&backend, &dir, path, template).await;
    for expected in ["form"].into_iter().chain(DEFAULT_VARIABLES) {
        assert!(
            labels.contains(&expected.to_string()),
            "{expected} in {labels:?}"
        );
    }
    let line = ACME_HOOKS[..ACME_HOOKS.find("'render element' => 'form'").unwrap()]
        .matches('\n')
        .count() as u32;
    assert_eq!(
        locations(
            &backend,
            &dir,
            &uri_for(&dir, path),
            position_in(template, "form }}", 1)
        )
        .await,
        vec![(ACME_HOOKS_PATH.to_string(), line)]
    );
}

const ATTRIBUTE: (&str, &str) = (
    "web/core/lib/Drupal/Core/Template/Attribute.php",
    "<?php\nnamespace Drupal\\Core\\Template;\nclass Attribute {\n    public function addClass(...$classes): static { return $this; }\n    public function setAttribute(string $attribute, mixed $value): static { return $this; }\n}\n",
);

const NOTES_INFO: (&str, &str) = (
    "web/modules/custom/notes/notes.info.yml",
    "name: Notes\ntype: module\n",
);

const NOTES_MODULE: (&str, &str) = (
    "web/modules/custom/notes/notes.module",
    "<?php\n\nfunction notes_theme() {\n  return [\n    'notes_teaser' => [\n      'variables' => ['note' => NULL, 'attributes' => []],\n    ],\n  ];\n}\n",
);

const NOTE: (&str, &str) = (
    "web/modules/custom/notes/src/Note.php",
    "<?php\nnamespace Drupal\\notes;\nclass Note {\n    public function label(): string { return ''; }\n    public function isPinned(): bool { return false; }\n}\n",
);

const NOTE_CONTROLLER: &str = r#"<?php
namespace Drupal\notes;

final class NoteController {
  public function show(Note $note): array {
    return [
      '#theme' => 'notes_teaser',
      '#note' => $note,
      '#attributes' => ['class' => ['teaser']],
      '#undeclared' => $note,
    ];
  }
}
"#;
const NOTE_CONTROLLER_PATH: &str = "web/modules/custom/notes/src/NoteController.php";

const TEASER_PATH: &str = "web/modules/custom/notes/templates/notes-teaser.html.twig";
const TEASER: &str =
    "<article{{ attributes. }}>\n{{ note. }}\n{{ note }} {{ title_attributes }}\n</article>\n";

/// A site with the module `notes`, whose controller passes a `Note` to
/// its `notes_teaser` template, open.
async fn notes_site() -> (phpantom_lsp::Backend, tempfile::TempDir, Uri) {
    let (backend, dir) = drupal_with(
        &[
            ATTRIBUTE,
            NOTES_INFO,
            NOTES_MODULE,
            NOTE,
            (TEASER_PATH, TEASER),
            (
                "web/modules/custom/notes/notes.routing.yml",
                "notes.show:\n  path: '/notes'\n",
            ),
        ],
        &[(NOTE_CONTROLLER_PATH, NOTE_CONTROLLER)],
    )
    .await;
    let teaser = uri_for(&dir, TEASER_PATH);
    crate::common::open_document(&backend, &teaser, "twig", TEASER).await;
    (backend, dir, teaser)
}

#[tokio::test]
async fn a_variable_holds_what_the_render_arrays_pass() {
    let (backend, dir, teaser) = notes_site().await;
    let position = position_in(TEASER, "note. }}", 5);
    let labels = labels_at(&backend, &teaser, position).await;
    for expected in ["label", "pinned"] {
        assert!(
            labels.contains(&expected.to_string()),
            "{expected} in {labels:?}"
        );
    }
    let position = position_in(TEASER, "note }}", 1);
    let text = hover_text_at(&backend, &teaser, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(text.contains("`Drupal\\notes\\Note`"), "{text}");
    assert!(
        text.contains("Passed by `web/modules/custom/notes/src/NoteController.php`"),
        "{text}"
    );
    // A key the hook does not declare is not handed to its template.
    let labels = template_variables(&backend, &dir, TEASER_PATH, "{{ }}\n").await;
    assert!(!labels.contains(&"undeclared".to_string()), "{labels:?}");
}

#[tokio::test]
async fn the_attributes_variables_hold_a_template_attribute() {
    let (backend, _dir, teaser) = notes_site().await;
    // Declared by the hook and passed as an array, but Drupal turns it into
    // an `Attribute` before the template sees it.
    let labels = labels_at(&backend, &teaser, position_in(TEASER, "attributes. }}", 11)).await;
    for expected in ["addClass", "setAttribute"] {
        assert!(
            labels.contains(&expected.to_string()),
            "{expected} in {labels:?}"
        );
    }
    let position = position_in(TEASER, "title_attributes }}", 1);
    let text = hover_text_at(&backend, &teaser, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(
        text.contains("`Drupal\\Core\\Template\\Attribute`"),
        "{text}"
    );
}

#[tokio::test]
async fn a_template_no_theme_hook_claims_receives_nothing_from_drupal() {
    let (backend, dir) = drupal(&[(CONTROLLER_PATH, CONTROLLER)]).await;
    let labels = template_variables(
        &backend,
        &dir,
        "web/modules/custom/acme/templates/acme-orphan.html.twig",
        "{{ }}\n",
    )
    .await;
    assert_eq!(labels, ["_charset", "_context", "_self"]);
}

#[tokio::test]
async fn a_template_reports_no_unknown_variables() {
    let (backend, _dir, teaser) = notes_site().await;
    // The route name nothing declares shows the template is judged.
    let body =
        "{{ note.label }} {{ nowhere }} {{ attributes.nothing }} {{ path('notes.nowhere') }}\n";
    crate::common::open_document(&backend, &teaser, "twig", body).await;
    let mut diagnostics = Vec::new();
    backend.collect_resource_diagnostics(teaser.as_str(), body, &mut diagnostics);
    let reported: Vec<&str> = diagnostics
        .iter()
        .map(|diagnostic| crate::common::message_text(&diagnostic.message))
        .collect();
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(reported[0].contains("notes.nowhere"), "{reported:?}");
}

#[tokio::test]
async fn render_arrays_outside_drupal_name_no_theme_hooks() {
    let backend = crate::common::create_test_backend();
    let uri = Uri::from_file_path(std::env::temp_dir().join("plain/src/Page.php")).unwrap();
    let php = "<?php\n$build = ['#theme' => '', '#name' => 'x'];\n";
    let labels = crate::common::complete_labels_at(&backend, &uri, php, 1, 23).await;
    assert!(labels.is_empty(), "{labels:?}");
    let hover = hover_text_at(&backend, &uri, 1, 28).await;
    assert!(hover.is_none(), "{hover:?}");
}
