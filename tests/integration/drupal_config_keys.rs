//! Drupal's configuration keys: the keys a configuration object's schema
//! declares, read by the object's `get()`, `set()`, and `clear()`, the key
//! of a `#config_target`, and a `ConfigTarget`'s property path.

use crate::common::{completion_response, hover_text_at, offset_position, response_labels};
use crate::drupal_services::{definitions_at, uri_for, workspace_in};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// Just enough of core's configuration API, where Composer would put core.
const STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal.php",
        "<?php\nclass Drupal {\n    public static function service(string $id): object {}\n    /**\n     * @return \\Drupal\\Core\\Config\\ImmutableConfig\n     */\n    public static function config($name) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Config/ConfigBase.php",
        "<?php\nnamespace Drupal\\Core\\Config;\nabstract class ConfigBase {\n    public function get($key = '') {}\n    public function set($key, $value) { return $this; }\n    public function clear($key) { return $this; }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Config/Config.php",
        "<?php\nnamespace Drupal\\Core\\Config;\nclass Config extends ConfigBase {\n    public function save() { return $this; }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Config/ImmutableConfig.php",
        "<?php\nnamespace Drupal\\Core\\Config;\nclass ImmutableConfig extends Config {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Config/ConfigFactoryInterface.php",
        "<?php\nnamespace Drupal\\Core\\Config;\ninterface ConfigFactoryInterface {\n    /**\n     * @return \\Drupal\\Core\\Config\\ImmutableConfig\n     */\n    public function get($name);\n    /**\n     * @return \\Drupal\\Core\\Config\\Config\n     */\n    public function getEditable($name);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Form/FormBase.php",
        "<?php\nnamespace Drupal\\Core\\Form;\nabstract class FormBase {\n    /**\n     * @return \\Drupal\\Core\\Config\\ImmutableConfig\n     */\n    protected function config($name) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Form/ConfigFormBaseTrait.php",
        "<?php\nnamespace Drupal\\Core\\Form;\ntrait ConfigFormBaseTrait {\n    /**\n     * @return \\Drupal\\Core\\Config\\Config|\\Drupal\\Core\\Config\\ImmutableConfig\n     */\n    protected function config($name) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Form/ConfigFormBase.php",
        "<?php\nnamespace Drupal\\Core\\Form;\nabstract class ConfigFormBase extends FormBase {\n    use ConfigFormBaseTrait;\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Form/ConfigTarget.php",
        "<?php\nnamespace Drupal\\Core\\Form;\nfinal class ConfigTarget {\n    public function __construct(public readonly string $configName, string|array $propertyPath) {}\n}\n",
    ),
];

const CORE: &[(&str, &str)] = &[
    (".git/HEAD", "ref: refs/heads/main\n"),
    (".gitignore", "/web/core/\n"),
    (
        "web/core/config/schema/core.data_types.schema.yml",
        "config_object:\n  type: mapping\n  mapping:\n    _core:\n      type: _core_config_info\n    langcode:\n      type: langcode\nmail:\n  type: mapping\n  mapping:\n    subject:\n      type: label\n    body:\n      type: text\n",
    ),
    (
        "web/core/modules/node/config/schema/node.schema.yml",
        "node.type.*:\n  type: config_entity\n  label: 'Content type'\n  mapping:\n    name:\n      type: label\n",
    ),
];

const SCHEMA_PATH: &str = "web/modules/custom/acme/config/schema/acme.schema.yml";
const SCHEMA: &str = "acme.settings:\n  type: config_object\n  label: 'Acme settings'\n  mapping:\n    notify_email:\n      type: email\n      label: 'Notification email'\n    notifications:\n      type: mapping\n      mapping:\n        email:\n          type: boolean\n    message:\n      type: mail\n    loopy:\n      type: acme.loop\n    dynamic:\n      type: acme.[%parent.type]\nacme.loop:\n  type: acme.loop_b\nacme.loop_b:\n  type: acme.loop\n";

const FORM_PATH: &str = "web/modules/custom/acme/src/Form/SettingsForm.php";

fn site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in CORE {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    dir
}

async fn site_backend(php: &str) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let files: Vec<(&str, &str)> = STUBS
        .iter()
        .copied()
        .chain([(SCHEMA_PATH, SCHEMA), (FORM_PATH, php)])
        .collect();
    workspace_in(site(), &files).await
}

/// The labels completion offers at `marker` (`|`) in `template`, which is
/// opened without the marker.
async fn offered_at(template: &str) -> Vec<String> {
    let at = template.find('|').expect("marker");
    let php = template.replacen('|', "", 1);
    let (backend, dir) = site_backend(&php).await;
    let position = offset_position(&php, at);
    response_labels(
        completion_response(
            &backend,
            &uri_for(&dir, FORM_PATH),
            position.line,
            position.character,
            None,
        )
        .await,
    )
}

fn form(body: &str) -> String {
    format!(
        r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Config\ConfigFactoryInterface;
use Drupal\Core\Config\ImmutableConfig;
use Drupal\Core\Form\ConfigFormBase;
use Drupal\Core\Form\ConfigTarget;

final class SettingsForm extends ConfigFormBase {{
    private readonly ImmutableConfig $settings;

    public function __construct(private readonly ConfigFactoryInterface $factory) {{
        $this->settings = $factory->get('acme.settings');
    }}

    public function run(): void {{
        {body}
    }}
}}
"#
    )
}

const TOP_LEVEL: [&str; 7] = [
    "_core",
    "dynamic",
    "langcode",
    "loopy",
    "message",
    "notifications",
    "notify_email",
];

#[tokio::test]
async fn keys_complete_after_every_way_of_reading_the_object() {
    for body in [
        "\\Drupal::config('acme.settings')->get('|');",
        "$config = \\Drupal::config('acme.settings');\n        $config->get('|');",
        "$this->factory->get('acme.settings')->get('|');",
        "$this->settings->get('|');",
        "$this->config('acme.settings')->get('|');",
        "$this->factory->getEditable('acme.settings')->set('|', 1);",
        "$config = $this->config('acme.settings');\n        $config->clear('|');",
        "$x = ['#config_target' => 'acme.settings:|'];",
        "new ConfigTarget('acme.settings', '|');",
        "new ConfigTarget('acme.settings', ['notify_email', '|']);",
    ] {
        let mut labels = offered_at(&form(body)).await;
        labels.sort();
        assert_eq!(labels, TOP_LEVEL, "{body}");
    }
}

#[tokio::test]
async fn nested_keys_complete_a_level_at_a_time() {
    for (body, expected) in [
        (
            "\\Drupal::config('acme.settings')->get('notifications.|');",
            &["email"][..],
        ),
        // A named type's keys.
        (
            "\\Drupal::config('acme.settings')->get('message.|');",
            &["body", "subject"][..],
        ),
        // A type that comes back to itself, and a placeholder, have none.
        (
            "\\Drupal::config('acme.settings')->get('loopy.|');",
            &[][..],
        ),
        (
            "\\Drupal::config('acme.settings')->get('dynamic.|');",
            &[][..],
        ),
        // A configuration entity's name finds the wildcard entry.
        // A configuration entity's name finds the wildcard entry, whose
        // `config_entity` type this schema does not declare.
        (
            "\\Drupal::config('node.type.article')->get('|');",
            &["name"][..],
        ),
        (
            "$x = ['#config_target' => 'acme.settings:notifications.|'];",
            &["email"][..],
        ),
    ] {
        let mut labels = offered_at(&form(body)).await;
        labels.sort();
        assert_eq!(labels, expected, "{body}");
    }
}

/// A class that uses `ConfigFormBaseTrait` without being a form reads the
/// object its `config()` names, as a config form does.
#[tokio::test]
async fn a_class_using_the_config_form_trait_carries_the_name() {
    let template = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\ConfigFormBaseTrait;

final class ListBuilder {
    use ConfigFormBaseTrait;

    public function run(): void {
        $this->config('acme.settings')->get('|');
    }
}
"#;
    let mut labels = offered_at(template).await;
    labels.sort();
    assert_eq!(labels, TOP_LEVEL);
}

/// `set()` and `clear()` throw on an `ImmutableConfig`, so nothing is
/// offered there.
#[tokio::test]
async fn an_immutable_object_offers_no_key_to_write() {
    for body in [
        "\\Drupal::config('acme.settings')->set('|', 1);",
        "$this->settings->clear('|');",
    ] {
        let labels = offered_at(&form(body)).await;
        assert!(
            !labels.iter().any(|label| label == "notify_email"),
            "{body}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn a_key_goes_to_its_schema_line() {
    let body = "\\Drupal::config('acme.settings')->get('notifications.email');\n        $this->settings->get('notify_email');\n        $this->factory->getEditable('acme.settings')->set('notify_email', 1);\n        $x = ['#config_target' => 'acme.settings:notify_email'];\n        new ConfigTarget('acme.settings', 'notify_email');\n        \\Drupal::config('acme.settings')->get('message.subject');";
    let php = form(body);
    let (backend, dir) = site_backend(&php).await;
    let uri = uri_for(&dir, FORM_PATH);
    let schema = uri_for(&dir, SCHEMA_PATH);
    let data_types = uri_for(&dir, "web/core/config/schema/core.data_types.schema.yml");
    let line_of =
        |needle: &str| SCHEMA[..SCHEMA.find(needle).unwrap()].matches('\n').count() as u32;
    let cases = [
        (
            "'notifications.email'",
            16,
            (&schema, line_of("        email:")),
        ),
        ("'notify_email'", 2, (&schema, line_of("    notify_email:"))),
        (
            "set('notify_email'",
            6,
            (&schema, line_of("    notify_email:")),
        ),
        (
            "settings:notify_email'",
            10,
            (&schema, line_of("    notify_email:")),
        ),
        (
            "'acme.settings', 'notify_email'",
            19,
            (&schema, line_of("    notify_email:")),
        ),
        ("'message.subject'", 10, (&data_types, 10)),
    ];
    for (needle, inside, (expected_uri, expected_line)) in cases {
        let at = php.find(needle).unwrap() + inside;
        let found = definitions_at(&backend, &uri, offset_position(&php, at)).await;
        assert_eq!(
            found
                .iter()
                .map(|location| (&location.uri, location.range.start.line))
                .collect::<Vec<_>>(),
            [(expected_uri, expected_line)],
            "{needle}"
        );
    }
}

/// The references of a key reach the reads a config object's type
/// confirms in other files, from either end.
#[tokio::test]
async fn key_references_reach_typed_reads_in_other_files() {
    let php = form("$x = ['#config_target' => 'acme.settings:notify_email'];");
    let reader_path = "web/modules/custom/acme/src/Reader.php";
    let reader = "<?php\nnamespace Drupal\\acme;\n\nfinal class Reader {\n    public function run(): void {\n        \\Drupal::config('acme.settings')->get('notify_email');\n        \\Drupal::config('other.settings')->get('notify_email');\n    }\n}\n";
    let files: Vec<(&str, &str)> = STUBS
        .iter()
        .copied()
        .chain([
            (SCHEMA_PATH, SCHEMA),
            (FORM_PATH, php.as_str()),
            (reader_path, reader),
        ])
        .collect();
    let (backend, dir) = workspace_in(site(), &files).await;
    let form_uri = uri_for(&dir, FORM_PATH);
    let reader_uri = uri_for(&dir, reader_path);
    let at = php.find("settings:notify_email").unwrap() + 12;
    let from_form =
        crate::drupal_services::references_at(&backend, &form_uri, offset_position(&php, at)).await;
    let at = reader.find("get('notify_email')").unwrap() + 6;
    let from_reader =
        crate::drupal_services::references_at(&backend, &reader_uri, offset_position(reader, at))
            .await;
    for found in [&from_form, &from_reader] {
        // The other object's key of the same name is not one of them.
        assert_eq!(
            found
                .iter()
                .filter(|(uri, _)| *uri == reader_uri)
                .collect::<Vec<_>>(),
            [&(reader_uri.clone(), 5)],
            "{found:?}"
        );
        assert!(found.iter().any(|(uri, _)| *uri == form_uri), "{found:?}");
    }
}

/// The object's type carries its name, which hover shows.
#[tokio::test]
async fn hover_shows_the_name_an_object_carries() {
    let php = form("$config = \\Drupal::config('acme.settings');\n        $config;");
    let (backend, dir) = site_backend(&php).await;
    let position = offset_position(&php, php.find("$config;").unwrap() + 2);
    let hover = hover_text_at(
        &backend,
        &uri_for(&dir, FORM_PATH),
        position.line,
        position.character,
    )
    .await
    .unwrap_or_default();
    assert!(
        hover.contains("ImmutableConfig<'acme.settings'>"),
        "{hover}"
    );
}

/// Editing the schema on disk drops the keys read from it.
#[tokio::test]
async fn an_edited_schema_offers_its_new_keys() {
    let template = form("\\Drupal::config('acme.settings')->get('|');");
    let at = template.find('|').unwrap();
    let php = template.replacen('|', "", 1);
    let dir = site();
    let schema_path = dir.path().join(SCHEMA_PATH);
    std::fs::create_dir_all(schema_path.parent().unwrap()).unwrap();
    std::fs::write(&schema_path, SCHEMA).unwrap();
    let files: Vec<(&str, &str)> = STUBS
        .iter()
        .copied()
        .chain([(FORM_PATH, php.as_str())])
        .collect();
    let (backend, dir) = workspace_in(dir, &files).await;
    let uri = uri_for(&dir, FORM_PATH);
    let position = offset_position(&php, at);
    let offered = || async {
        response_labels(
            completion_response(&backend, &uri, position.line, position.character, None).await,
        )
    };
    assert!(offered().await.contains(&"notify_email".to_string()));

    std::fs::write(
        &schema_path,
        SCHEMA.replace(
            "    notify_email:",
            "    added_key:\n      type: string\n    notify_email:",
        ),
    )
    .unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: uri_for(&dir, SCHEMA_PATH),
                kind: FileChangeType::Changed,
            }],
        })
        .await;
    let labels = offered().await;
    assert!(labels.contains(&"added_key".to_string()), "{labels:?}");
}

/// The name rides on the type and changes nothing else about it: the
/// object keeps its members, and passes and returns as the class its
/// method declares.
#[tokio::test]
async fn a_tagged_object_is_still_its_class() {
    let body = "$config = \\Drupal::config('acme.settings');\n        $config->|";
    let labels = offered_at(&form(body)).await;
    assert!(
        labels.iter().any(|label| label.starts_with("save")),
        "{labels:?}"
    );

    let php = form(
        "$this->takes(\\Drupal::config('acme.settings'));\n        $this->takes($this->settings);\n    }\n\n    private function takes(ImmutableConfig $config): ImmutableConfig {\n        return $this->factory->get('acme.settings');",
    );
    let (backend, dir) = site_backend(&php).await;
    let uri = uri_for(&dir, FORM_PATH);
    backend.update_ast(uri.as_str(), &php);
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), &php, &mut diagnostics);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
