//! Drupal's configuration names: the configuration objects an extension
//! ships default configuration for or describes in its schema, read by
//! `\Drupal::config()`, the config factory, the `config()` shortcut, a
//! config form's editable names, and a form element's `#config_target`.

use crate::common::{
    completion_response, hover_text_at, initialize_with_options, offset_position, response_labels,
};
use crate::drupal_services::{definitions_at, position_in, references_at, uri_for, workspace_in};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// Just enough of core's configuration API, where Composer would put core.
const STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal.php",
        "<?php\nclass Drupal {\n    public static function service(string $id): object {}\n    public static function config($name) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Config/ConfigFactoryInterface.php",
        "<?php\nnamespace Drupal\\Core\\Config;\ninterface ConfigFactoryInterface {\n    public function get($name);\n    public function getEditable($name);\n    public function loadMultiple(array $names);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Controller/ControllerBase.php",
        "<?php\nnamespace Drupal\\Core\\Controller;\nabstract class ControllerBase {\n    protected function config($name) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Form/FormBase.php",
        "<?php\nnamespace Drupal\\Core\\Form;\nabstract class FormBase {\n    protected function config($name) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Form/ConfigFormBase.php",
        "<?php\nnamespace Drupal\\Core\\Form;\nabstract class ConfigFormBase extends FormBase {\n    abstract protected function getEditableConfigNames();\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Form/ConfigTarget.php",
        "<?php\nnamespace Drupal\\Core\\Form;\nfinal class ConfigTarget {\n    public function __construct(public readonly string $configName, string|array $propertyPath) {}\n}\n",
    ),
];

/// Core's configuration, which a site's `.gitignore` leaves out, and a test
/// module's beside it.
const CORE: &[(&str, &str)] = &[
    (".git/HEAD", "ref: refs/heads/main\n"),
    (".gitignore", "/web/core/\n"),
    (
        "web/core/config/schema/core.data_types.schema.yml",
        "config_object:\n  type: mapping\ntheme_settings:\n  type: config_object\n",
    ),
    (
        "web/core/modules/system/config/schema/system.schema.yml",
        "system.site:\n  type: config_object\n  label: 'Site information'\nsystem.menu.*:\n  type: config_entity\n  label: 'Menu'\nsystem.theme.global:\n  type: theme_settings\nblock.settings.system_menu_block:*:\n  type: config_object\n",
    ),
    (
        "web/core/modules/system/config/install/system.site.yml",
        "name: Drupal\n",
    ),
    (
        "web/core/modules/system/config/install/system.menu.main.yml",
        "id: main\n",
    ),
    (
        "web/core/modules/system/tests/modules/config_test/config/install/config_test.system.yml",
        "foo: bar\n",
    ),
    (
        "web/core/modules/system/tests/modules/config_test/config/schema/config_test.schema.yml",
        "config_test.types:\n  type: config_object\n",
    ),
];

const DEFAULT_PATH: &str = "web/modules/custom/acme/config/install/acme.settings.yml";
const DEFAULT: &str = "notify: ''\n";
const SCHEMA_PATH: &str = "web/modules/custom/acme/config/schema/acme.schema.yml";
const SCHEMA: &str = "acme.settings:\n  type: config_object\n  label: 'Acme settings'\nacme.limits:\n  type: config_object\n";

const FORM: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Config\ConfigFactoryInterface;
use Drupal\Core\Form\ConfigFormBase;
use Drupal\Core\Form\ConfigTarget;

final class SettingsForm extends ConfigFormBase {
    public function __construct(private readonly ConfigFactoryInterface $factory) {}

    protected function getEditableConfigNames(): array {
        return ['acme.settings'];
    }

    public function buildForm(array $form): array {
        \Drupal::config('acme.settings');
        $this->factory->get('acme.settings');
        $this->factory->getEditable('acme.settings');
        $this->factory->loadMultiple(['acme.settings', 'system.site']);
        $this->config('acme.settings');
        $form['notify'] = ['#type' => 'email', '#config_target' => 'acme.settings:notify'];
        $form['other'] = ['#config_target' => new ConfigTarget('acme.settings', 'other')];
        return $form;
    }
}
"#;

const FORM_PATH: &str = "web/modules/custom/acme/src/Form/SettingsForm.php";

const CONTROLLER: &str = r#"<?php
namespace Drupal\acme\Controller;

use Drupal\Core\Controller\ControllerBase;

final class SiteController extends ControllerBase {
    public function name(): void {
        $this->config('system.site');
        \Drupal::config('system.menu.main');
    }
}
"#;

const CONTROLLER_PATH: &str = "web/modules/custom/acme/src/Controller/SiteController.php";

fn site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in CORE {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    dir
}

fn files<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    STUBS.iter().copied().chain(extra.iter().copied()).collect()
}

async fn site_backend(extra: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    workspace_in(site(), &files(extra)).await
}

/// Each location as its file and line.
fn places(found: &[Location]) -> Vec<(Uri, u32)> {
    found
        .iter()
        .map(|location| (location.uri.clone(), location.range.start.line))
        .collect()
}

/// Every place that names a configuration object goes to its default
/// configuration file and its schema entry, a custom module's and core's
/// alike.
#[tokio::test]
async fn every_place_goes_to_the_default_file_and_the_schema() {
    let (backend, dir) = site_backend(&[
        (DEFAULT_PATH, DEFAULT),
        (SCHEMA_PATH, SCHEMA),
        (FORM_PATH, FORM),
        (CONTROLLER_PATH, CONTROLLER),
    ])
    .await;
    let acme = vec![
        (uri_for(&dir, DEFAULT_PATH), 0),
        (uri_for(&dir, SCHEMA_PATH), 0),
    ];
    let form = uri_for(&dir, FORM_PATH);
    let sites: Vec<usize> = FORM
        .match_indices("'acme.settings")
        .map(|(at, _)| at)
        .collect();
    assert_eq!(sites.len(), 8);
    for at in sites {
        let position = offset_position(FORM, at + 3);
        let found = definitions_at(&backend, &form, position).await;
        assert_eq!(places(&found), acme, "{position:?}");
    }

    let core = vec![
        (
            uri_for(
                &dir,
                "web/core/modules/system/config/install/system.site.yml",
            ),
            0,
        ),
        (
            uri_for(
                &dir,
                "web/core/modules/system/config/schema/system.schema.yml",
            ),
            0,
        ),
    ];
    let controller = uri_for(&dir, CONTROLLER_PATH);
    for (uri, content) in [(&form, FORM), (&controller, CONTROLLER)] {
        let position = position_in(content, "'system.site'", 3);
        let found = definitions_at(&backend, uri, position).await;
        assert_eq!(places(&found), core, "{content}");
    }

    // A configuration entity's name finds the wildcard entry for its kind.
    let found = definitions_at(
        &backend,
        &controller,
        position_in(CONTROLLER, "'system.menu.main'", 3),
    )
    .await;
    assert_eq!(
        places(&found),
        [
            (
                uri_for(
                    &dir,
                    "web/core/modules/system/config/install/system.menu.main.yml"
                ),
                0
            ),
            (
                uri_for(
                    &dir,
                    "web/core/modules/system/config/schema/system.schema.yml"
                ),
                3
            ),
        ]
    );

    let hover = hover_text_at(&backend, &form, 15, 26)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Acme settings"), "{hover}");
    assert!(hover.contains("acme.settings.yml"), "{hover}");
    assert!(hover.contains("acme.schema.yml"), "{hover}");

    // The references are every read and both declarations.
    let found = references_at(
        &backend,
        &form,
        offset_position(FORM, FORM.find("'acme.settings").unwrap() + 3),
    )
    .await;
    assert_eq!(found.len(), 10, "{found:?}");
}

#[tokio::test]
async fn names_complete_in_every_place_and_data_types_do_not() {
    let form = FORM.replace("acme.settings", "");
    let controller = CONTROLLER.replace("system.site", "");
    let (backend, dir) = site_backend(&[
        (DEFAULT_PATH, DEFAULT),
        (SCHEMA_PATH, SCHEMA),
        (FORM_PATH, form.as_str()),
        (CONTROLLER_PATH, controller.as_str()),
    ])
    .await;
    let mut places: Vec<(&str, &str, usize)> = form
        .match_indices("''")
        .map(|(at, _)| (FORM_PATH, form.as_str(), at + 1))
        .collect();
    // The `#config_target`'s name, before its key.
    places.push((
        FORM_PATH,
        form.as_str(),
        form.find("':notify'").unwrap() + 1,
    ));
    places.push((
        CONTROLLER_PATH,
        controller.as_str(),
        controller.find("''").unwrap() + 1,
    ));
    assert_eq!(places.len(), 9);
    for (path, content, at) in places {
        let position = offset_position(content, at);
        let labels = response_labels(
            completion_response(
                &backend,
                &uri_for(&dir, path),
                position.line,
                position.character,
                None,
            )
            .await,
        );
        for name in [
            "acme.settings",
            "acme.limits",
            "system.site",
            "system.menu.main",
            "system.theme.global",
        ] {
            assert!(
                labels.contains(&name.to_string()),
                "{path}:{position:?}: {labels:?}"
            );
        }
        for not_a_name in [
            "system.menu.*",
            "theme_settings",
            "config_object",
            "block.settings.system_menu_block:*",
            "config_test.system",
            "config_test.types",
        ] {
            assert!(
                !labels.contains(&not_a_name.to_string()),
                "{path}:{position:?}: {labels:?}"
            );
        }
    }
}

/// Every place reaches a core name too.
#[tokio::test]
async fn every_place_goes_to_core_configuration() {
    let form = FORM.replace("acme.settings", "system.site");
    let (backend, dir) = site_backend(&[(FORM_PATH, form.as_str())]).await;
    let core = vec![
        (
            uri_for(
                &dir,
                "web/core/modules/system/config/install/system.site.yml",
            ),
            0,
        ),
        (
            uri_for(
                &dir,
                "web/core/modules/system/config/schema/system.schema.yml",
            ),
            0,
        ),
    ];
    let uri = uri_for(&dir, FORM_PATH);
    let sites: Vec<usize> = form
        .match_indices("'system.site")
        .map(|(at, _)| at)
        .collect();
    assert_eq!(sites.len(), 9);
    for at in sites {
        let position = offset_position(&form, at + 3);
        let found = definitions_at(&backend, &uri, position).await;
        assert_eq!(places(&found), core, "{position:?}");
    }
}

/// A test module's configuration counts while its file is open.
#[tokio::test]
async fn an_open_test_module_offers_its_names() {
    let controller = CONTROLLER.replace("system.site", "");
    let fixture =
        "web/core/modules/system/tests/modules/config_test/config/install/config_test.system.yml";
    let fixture_schema =
        "web/core/modules/system/tests/modules/config_test/config/schema/config_test.schema.yml";
    let (backend, dir) = site_backend(&[
        (CONTROLLER_PATH, controller.as_str()),
        (fixture, "foo: bar\n"),
        (
            fixture_schema,
            "config_test.types:\n  type: config_object\n",
        ),
    ])
    .await;
    let php = uri_for(&dir, CONTROLLER_PATH);
    let position = offset_position(&controller, controller.find("''").unwrap() + 1);
    let offered = || async {
        response_labels(
            completion_response(&backend, &php, position.line, position.character, None).await,
        )
    };
    let labels = offered().await;
    assert!(
        labels.contains(&"config_test.system".to_string()),
        "{labels:?}"
    );
    assert!(
        labels.contains(&"config_test.types".to_string()),
        "{labels:?}"
    );

    for path in [fixture, fixture_schema] {
        backend
            .did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_for(&dir, path),
                },
            })
            .await;
    }
    let labels = offered().await;
    assert!(
        !labels.contains(&"config_test.system".to_string()),
        "{labels:?}"
    );
    assert!(
        !labels.contains(&"config_test.types".to_string()),
        "{labels:?}"
    );
}

/// A class that uses core's `ConfigFormBaseTrait` without being a form,
/// as a list builder may, reads a name in its `config()` too.
#[tokio::test]
async fn a_class_using_the_config_form_trait_reads_names() {
    let trait_stub = "<?php\nnamespace Drupal\\Core\\Form;\ntrait ConfigFormBaseTrait {\n    protected function config($name) {}\n}\n";
    let builder = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Form\ConfigFormBaseTrait;

class BaseBuilder {
    use ConfigFormBaseTrait;
}

final class ListBuilder extends BaseBuilder {
    public function run(): void {
        $this->config('acme.settings');
    }
}
"#;
    let path = "web/modules/custom/acme/src/ListBuilder.php";
    let (backend, dir) = site_backend(&[
        (
            "web/core/lib/Drupal/Core/Form/ConfigFormBaseTrait.php",
            trait_stub,
        ),
        (DEFAULT_PATH, DEFAULT),
        (SCHEMA_PATH, SCHEMA),
        (path, builder),
    ])
    .await;
    let found = definitions_at(
        &backend,
        &uri_for(&dir, path),
        position_in(builder, "'acme.settings'", 3),
    )
    .await;
    assert_eq!(
        places(&found),
        [
            (uri_for(&dir, DEFAULT_PATH), 0),
            (uri_for(&dir, SCHEMA_PATH), 0)
        ]
    );
}

/// The same short names, of classes that are not Drupal's, read nothing.
#[tokio::test]
async fn look_alike_classes_read_no_name() {
    let consumer = r#"<?php
namespace Drupal\acme;

final class Drupal {
    public static function config($name) {}
}

final class ConfigTarget {
    public function __construct(string $name) {}
}

final class Registry {
    public function get(string $name) {}
    public function config(string $name) {}
}

final class Elsewhere {
    public function run(Registry $registry): void {
        Drupal::config('acme.settings');
        new ConfigTarget('acme.settings');
        $registry->get('acme.settings');
        $registry->config('acme.settings');
        $x = ['#config_target' => 1];
    }
}
"#;
    let path = "web/modules/custom/acme/src/Elsewhere.php";
    let (backend, dir) = site_backend(&[
        (DEFAULT_PATH, DEFAULT),
        (SCHEMA_PATH, SCHEMA),
        (path, consumer),
    ])
    .await;
    let uri = uri_for(&dir, path);
    for (at, _) in consumer.match_indices("'acme.settings'") {
        let found = definitions_at(&backend, &uri, offset_position(consumer, at + 3)).await;
        assert!(found.is_empty(), "{at}: {found:?}");
    }
}

/// A project that is not a Drupal site offers no configuration name.
#[tokio::test]
async fn a_project_that_is_not_drupal_reads_no_name() {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        (
            "composer.json",
            r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        ),
        ("src/config/install/acme.settings.yml", "notify: ''\n"),
        (
            "src/Check.php",
            "<?php\nnamespace App;\nfinal class Check {\n    public function run(): void {\n        \\Drupal::config('');\n    }\n}\n",
        ),
    ];
    for (path, content) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let backend = phpantom_lsp::Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    let uri = uri_for(&dir, "src/Check.php");
    crate::common::open_document(&backend, &uri, "php", files[2].1).await;
    let labels = response_labels(completion_response(&backend, &uri, 4, 25, None).await);
    assert!(!labels.contains(&"acme.settings".to_string()), "{labels:?}");
}

async fn notify(backend: &phpantom_lsp::Backend, uri: &Uri, kind: FileChangeType) {
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: uri.clone(),
                kind,
            }],
        })
        .await;
}

/// Adding or deleting a default configuration file, or editing a schema
/// file, on disk refreshes the names offered.
#[tokio::test]
async fn files_changed_on_disk_refresh_the_names() {
    let controller = CONTROLLER.replace("system.site", "");
    let dir = site();
    let schema = dir.path().join(SCHEMA_PATH);
    std::fs::create_dir_all(schema.parent().unwrap()).unwrap();
    std::fs::write(&schema, SCHEMA).unwrap();
    let (backend, dir) = workspace_in(dir, &files(&[(CONTROLLER_PATH, controller.as_str())])).await;
    let php = uri_for(&dir, CONTROLLER_PATH);
    let position = offset_position(&controller, controller.find("''").unwrap() + 1);
    let offered = || async {
        response_labels(
            completion_response(&backend, &php, position.line, position.character, None).await,
        )
    };
    assert!(offered().await.contains(&"acme.limits".to_string()));
    assert!(!offered().await.contains(&"acme.extra".to_string()));

    let extra = dir
        .path()
        .join("web/modules/custom/acme/config/optional/acme.extra.yml");
    std::fs::create_dir_all(extra.parent().unwrap()).unwrap();
    std::fs::write(&extra, "a: b\n").unwrap();
    let extra_uri = Uri::from_file_path(&extra).unwrap();
    notify(&backend, &extra_uri, FileChangeType::Created).await;
    assert!(offered().await.contains(&"acme.extra".to_string()));

    std::fs::remove_file(&extra).unwrap();
    notify(&backend, &extra_uri, FileChangeType::Deleted).await;
    assert!(!offered().await.contains(&"acme.extra".to_string()));

    std::fs::write(
        &schema,
        format!("{SCHEMA}acme.added:\n  type: config_object\n"),
    )
    .unwrap();
    notify(
        &backend,
        &uri_for(&dir, SCHEMA_PATH),
        FileChangeType::Changed,
    )
    .await;
    let labels = offered().await;
    assert!(labels.contains(&"acme.added".to_string()), "{labels:?}");
}
