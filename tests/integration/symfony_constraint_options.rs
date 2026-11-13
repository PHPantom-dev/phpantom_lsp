//! Symfony constraint options in YAML and XML validation mappings: the keys
//! a mapping gives a constraint, checked against and completed from the
//! constructor parameters and public properties of the constraint's class.

use crate::common::{
    completion_response, create_psr4_workspace, offset_position, open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Symfony\\Component\\Validator\\": "stubs/validator/"
    } }
}"#;

const STUBS: &[(&str, &str)] = &[
    (
        "stubs/validator/Constraint.php",
        "<?php\nnamespace Symfony\\Component\\Validator;\nabstract class Constraint {\n    public mixed $payload;\n    public ?array $groups = null;\n    public function __construct(mixed $options = null, ?array $groups = null, mixed $payload = null) {}\n}\n",
    ),
    (
        "stubs/validator/Constraints/Length.php",
        "<?php\nnamespace Symfony\\Component\\Validator\\Constraints;\nuse Symfony\\Component\\Validator\\Constraint;\nclass Length extends Constraint {\n    public string $minMessage = '';\n    public ?int $min = null;\n    public ?int $max = null;\n    public function __construct(int|array|null $exactly = null, ?int $min = null, ?int $max = null, ?array $groups = null, mixed $payload = null, ?array $options = null) {}\n}\n",
    ),
    (
        "stubs/validator/Constraints/NotBlank.php",
        "<?php\nnamespace Symfony\\Component\\Validator\\Constraints;\nuse Symfony\\Component\\Validator\\Constraint;\nclass NotBlank extends Constraint {\n    public string $message = '';\n}\n",
    ),
];

/// A constraint of the project's own whose parent does not load, so its
/// options are not all known.
const LOOSE_PHP: &str =
    "<?php\nnamespace App\\Validator;\nclass Loose extends \\Missing\\Base {}\n";

/// A constraint whose constructor collects any named argument.
const OPEN_PHP: &str = "<?php\nnamespace App\\Validator;\nuse Symfony\\Component\\Validator\\Constraint;\nclass Open extends Constraint {\n    public function __construct(mixed ...$options) {}\n}\n";

// Escape the indented blank line's final space so whitespace cleanup preserves it.
const VALIDATION_YAML: &str = "App\\Entity\\User:
    constraints:
        - App\\Validator\\Loose: { anything: 1 }
        - App\\Validator\\Open: { whatever: 1 }
    properties:
        email:
            - NotBlank:
                message: Required
                mesage: Typo
                groups:
                    - Default
            - Length: { min: 2, mni: 3, value: 4, minMessage: 'a, b: c' }
        displayName:
            - Length:
                max: 50
               \x20
            - Length: { min: 1,  }
";

const VALIDATION_XML: &str = r#"<?xml version="1.0"?>
<constraint-mapping>
    <class name="App\Entity\User">
        <property name="email">
            <constraint name="Length">
                <option name="min">2</option>
                <option name="mxa">50</option>
                <option name="">1</option>
            </constraint>
        </property>
    </class>
</constraint-mapping>
"#;

async fn workspace() -> (Backend, tempfile::TempDir) {
    let files = [
        ("src/Validator/Loose.php", LOOSE_PHP),
        ("src/Validator/Open.php", OPEN_PHP),
        ("config/validator/User.yaml", VALIDATION_YAML),
        ("config/validator/User.xml", VALIDATION_XML),
    ];
    let all: Vec<(&str, &str)> = STUBS.iter().chain(&files).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in files {
        let language = match path.rsplit('.').next() {
            Some("php") => "php",
            Some("xml") => "xml",
            _ => "yaml",
        };
        open_document(&backend, &uri_for(&dir, path), language, content).await;
    }
    (backend, dir)
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dir.path().join(rel)).unwrap()
}

/// The unknown-option diagnostics reported in the mapping at `rel`, each
/// with the text it covers.
fn unknown_options(
    backend: &Backend,
    dir: &tempfile::TempDir,
    rel: &str,
    content: &str,
) -> Vec<(String, String)> {
    let mut diagnostics = Vec::new();
    backend.collect_resource_diagnostics(uri_for(dir, rel).as_str(), content, &mut diagnostics);
    let lines: Vec<&str> = content.lines().collect();
    diagnostics
        .into_iter()
        .filter(|d| {
            d.code
                == Some(Code::String(
                    "invalid_symfony_constraint_option".to_string(),
                ))
        })
        .map(|d| {
            let line = lines[d.range.start.line as usize];
            let covered =
                line[d.range.start.character as usize..d.range.end.character as usize].to_string();
            (crate::common::message_text(&d.message).to_string(), covered)
        })
        .collect()
}

async fn labels_at(backend: &Backend, uri: &Uri, content: &str, offset: usize) -> Vec<String> {
    let position = offset_position(content, offset);
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

#[tokio::test]
async fn options_a_constraint_does_not_take_are_reported() {
    let (backend, dir) = workspace().await;
    assert_eq!(
        unknown_options(
            &backend,
            &dir,
            "config/validator/User.yaml",
            VALIDATION_YAML
        ),
        vec![
            (
                "Unknown option 'mesage' for constraint Symfony\\Component\\Validator\\Constraints\\NotBlank".to_string(),
                "mesage".to_string()
            ),
            (
                "Unknown option 'mni' for constraint Symfony\\Component\\Validator\\Constraints\\Length".to_string(),
                "mni".to_string()
            ),
        ],
    );
    assert_eq!(
        unknown_options(&backend, &dir, "config/validator/User.xml", VALIDATION_XML),
        vec![(
            "Unknown option 'mxa' for constraint Symfony\\Component\\Validator\\Constraints\\Length".to_string(),
            "mxa".to_string()
        )],
    );
}

#[tokio::test]
async fn options_complete_in_a_flow_mapping() {
    let (backend, dir) = workspace().await;
    let uri = uri_for(&dir, "config/validator/User.yaml");
    let offset = VALIDATION_YAML.find("{ min: 1, ").unwrap() + "{ min: 1, ".len();
    // `min` is given already, and `options` is the deprecated catch-all.
    assert_eq!(
        labels_at(&backend, &uri, VALIDATION_YAML, offset).await,
        vec!["exactly", "max", "groups", "payload", "minMessage"],
    );
}

#[tokio::test]
async fn options_complete_in_a_block_mapping() {
    let (backend, dir) = workspace().await;
    let uri = uri_for(&dir, "config/validator/User.yaml");
    // The blank line indented under `max: 50`.
    let prefix = "max: 50\n                ";
    let offset = VALIDATION_YAML.find(prefix).unwrap() + prefix.len();
    assert_eq!(
        labels_at(&backend, &uri, VALIDATION_YAML, offset).await,
        vec!["exactly", "min", "groups", "payload", "minMessage"],
    );
    // A value is no key.
    let offset = VALIDATION_YAML.find("max: 50").unwrap() + "max: 5".len();
    assert!(
        labels_at(&backend, &uri, VALIDATION_YAML, offset)
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn options_complete_in_an_xml_option_name() {
    let (backend, dir) = workspace().await;
    let uri = uri_for(&dir, "config/validator/User.xml");
    let offset = VALIDATION_XML.find("name=\"\"").unwrap() + "name=\"".len();
    assert_eq!(
        labels_at(&backend, &uri, VALIDATION_XML, offset).await,
        vec!["exactly", "max", "groups", "payload", "minMessage"],
    );
}
