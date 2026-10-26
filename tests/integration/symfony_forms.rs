//! Symfony form fields and validation mappings, which name a class's
//! properties, and bundle configuration keys, from a bundle's `TreeBuilder`
//! schema to the YAML under `config/packages/` that sets them.

use crate::common::{
    completion_response, create_psr4_workspace, hover_text_at, offset_position, open_document,
    rename, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Symfony\\Component\\Validator\\": "stubs/validator/"
    } }
}"#;

const STUBS: &[(&str, &str)] = &[(
    "stubs/validator/Constraints/NotBlank.php",
    "<?php\nnamespace Symfony\\Component\\Validator\\Constraints;\nclass NotBlank {}\n",
)];

const USER_PHP: &str = r#"<?php
namespace App\Entity;

class User
{
    public function __construct(
        private string $email,
        private string $displayName,
    ) {}

    public function email(): string
    {
        return $this->email;
    }
}
"#;

/// [`USER_PHP`] without a method that shares the property's name, which
/// member references do not tell apart yet.
const USER_WITH_GETTER_PHP: &str = r#"<?php
namespace App\Entity;

class User
{
    public function __construct(
        private string $email,
        private string $displayName,
    ) {}

    public function getEmail(): string
    {
        return $this->email;
    }
}
"#;

const FORM_PHP: &str = r#"<?php
namespace App\Form;

use App\Entity\User;
use Symfony\Component\Form\AbstractType;
use Symfony\Component\Form\FormBuilderInterface;
use Symfony\Component\OptionsResolver\OptionsResolver;

final class RegistrationType extends AbstractType
{
    public function buildForm(FormBuilderInterface $builder, array $options): void
    {
        $builder
            ->add('email')
            ->add('agreeTerms', null, ['mapped' => false])
            ->add('');
    }

    public function configureOptions(OptionsResolver $resolver): void
    {
        $resolver->setDefaults(['data_class' => User::class]);
    }
}
"#;

const VALIDATION_YAML: &str = "App\\Entity\\User:
    properties:
        email:
            - NotBlank: ~
        displayName:
            - Length: { min: 2 }
";

const VALIDATION_XML: &str = r#"<?xml version="1.0"?>
<constraint-mapping>
    <class name="App\Entity\User">
        <property name="email">
            <constraint name="NotBlank"/>
        </property>
    </class>
</constraint-mapping>
"#;

const CONFIGURATION_PHP: &str = r#"<?php
namespace App\DependencyInjection;

use Symfony\Component\Config\Definition\Builder\TreeBuilder;

final class Configuration
{
    public function getConfigTreeBuilder(): TreeBuilder
    {
        $treeBuilder = new TreeBuilder('acme_demo');
        $treeBuilder->getRootNode()
            ->children()
                ->scalarNode('api_key')->end()
                ->arrayNode('mailer')
                    ->children()
                        ->scalarNode('dsn')->end()
                        ->integerNode('port')->end()
                    ->end()
                ->end()
            ->end();

        return $treeBuilder;
    }
}
"#;

/// Line 4 is a blank line indented under `mailer:`.
const PACKAGE_YAML: &str = "acme_demo:\n    api_key: local\n    mailer:\n        dsn: smtp://localhost\n        \nframework:\n    secret: x\n";

/// A workspace holding the stubs and `files`, every one of them open.
async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = STUBS.iter().chain(files).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in files {
        let language = if path.ends_with(".php") {
            "php"
        } else if path.ends_with(".xml") {
            "xml"
        } else {
            "yaml"
        };
        open_document(&backend, &uri_for(&dir, path), language, content).await;
    }
    (backend, dir)
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dir.path().join(rel)).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32)> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let locations = match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    };
    locations
        .into_iter()
        .map(|location| (location.uri, location.range.start.line))
        .collect()
}

async fn references(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32)> {
    let mut found: Vec<(Uri, u32)> = backend
        .references(ReferenceParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .map(|location| (location.uri, location.range.start.line))
        .collect();
    found.sort();
    found
}

fn sorted(mut locations: Vec<(Uri, u32)>) -> Vec<(Uri, u32)> {
    locations.sort();
    locations
}

async fn completion_labels(backend: &Backend, uri: &Uri, position: Position) -> Vec<String> {
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

#[tokio::test]
async fn validation_mappings_go_to_the_property_and_the_constraint() {
    let (backend, dir) = workspace(&[
        ("src/Entity/User.php", USER_PHP),
        ("config/validator/User.yaml", VALIDATION_YAML),
        ("config/validator/User.xml", VALIDATION_XML),
    ])
    .await;
    let user = uri_for(&dir, "src/Entity/User.php");
    let yaml = uri_for(&dir, "config/validator/User.yaml");
    let xml = uri_for(&dir, "config/validator/User.xml");
    let not_blank = uri_for(&dir, "stubs/validator/Constraints/NotBlank.php");

    // The property, not the `email()` method of the same name.
    let found = definitions(&backend, &yaml, position_in(VALIDATION_YAML, "email:", 2)).await;
    assert_eq!(found, vec![(user.clone(), 6)]);
    let found = definitions(&backend, &xml, position_in(VALIDATION_XML, "\"email\"", 2)).await;
    assert_eq!(found, vec![(user.clone(), 6)]);

    let found = definitions(&backend, &yaml, position_in(VALIDATION_YAML, "NotBlank", 2)).await;
    assert_eq!(found, vec![(not_blank.clone(), 2)]);
    let found = definitions(&backend, &xml, position_in(VALIDATION_XML, "NotBlank", 2)).await;
    assert_eq!(found, vec![(not_blank, 2)]);
}

#[tokio::test]
async fn property_references_reach_forms_and_validation_mappings() {
    let (backend, dir) = workspace(&[
        ("src/Entity/User.php", USER_WITH_GETTER_PHP),
        ("src/Form/RegistrationType.php", FORM_PHP),
        ("config/validator/User.yaml", VALIDATION_YAML),
        ("config/validator/User.xml", VALIDATION_XML),
    ])
    .await;
    let user = uri_for(&dir, "src/Entity/User.php");
    let form = uri_for(&dir, "src/Form/RegistrationType.php");
    let yaml = uri_for(&dir, "config/validator/User.yaml");
    let xml = uri_for(&dir, "config/validator/User.xml");

    let expected = sorted(vec![
        (user.clone(), 6),
        (user.clone(), 12),
        (form.clone(), 13),
        (yaml.clone(), 2),
        (xml.clone(), 3),
    ]);
    let found = references(
        &backend,
        &user,
        position_in(USER_WITH_GETTER_PHP, "$email", 2),
    )
    .await;
    assert_eq!(found, expected);
    let found = references(&backend, &yaml, position_in(VALIDATION_YAML, "email:", 2)).await;
    assert_eq!(found, expected);

    // A form field navigates to the data-class property it maps to.
    let found = definitions(&backend, &form, position_in(FORM_PHP, "'email'", 2)).await;
    assert_eq!(found, vec![(user.clone(), 6)]);
    // An unmapped field names no property.
    let found = definitions(&backend, &form, position_in(FORM_PHP, "'agreeTerms'", 2)).await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn renaming_a_property_rewrites_its_validation_mappings_and_form_fields() {
    let (backend, dir) = workspace(&[
        ("src/Entity/User.php", USER_WITH_GETTER_PHP),
        ("src/Form/RegistrationType.php", FORM_PHP),
        ("config/validator/User.yaml", VALIDATION_YAML),
    ])
    .await;
    let user = uri_for(&dir, "src/Entity/User.php");
    let form = uri_for(&dir, "src/Form/RegistrationType.php");
    let yaml = uri_for(&dir, "config/validator/User.yaml");

    let position = position_in(VALIDATION_YAML, "email:", 2);
    let edit = rename(&backend, &yaml, position.line, position.character, "mail")
        .await
        .expect("the rename should produce edits");
    let changes = edit.changes.expect("a change map");
    let lines = |uri: &Uri| {
        let mut lines: Vec<u32> = changes
            .get(uri)
            .into_iter()
            .flatten()
            .inspect(|edit| assert_eq!(edit.new_text.trim_start_matches('$'), "mail"))
            .map(|edit| edit.range.start.line)
            .collect();
        lines.sort_unstable();
        lines
    };
    assert_eq!(lines(&yaml), vec![2]);
    assert_eq!(lines(&form), vec![13]);
    assert_eq!(lines(&user), vec![6, 12]);
}

#[tokio::test]
async fn form_fields_complete_from_the_data_class() {
    let (backend, dir) = workspace(&[
        ("src/Entity/User.php", USER_PHP),
        ("src/Form/RegistrationType.php", FORM_PHP),
    ])
    .await;
    let form = uri_for(&dir, "src/Form/RegistrationType.php");

    let labels = completion_labels(&backend, &form, position_in(FORM_PHP, "add('')", 5)).await;
    assert_eq!(labels, vec!["displayName", "email"]);

    let position = position_in(FORM_PHP, "'email'", 2);
    let hover = hover_text_at(&backend, &form, position.line, position.character)
        .await
        .expect("a hover");
    assert!(hover.contains("$email"), "{hover}");
}

#[tokio::test]
async fn configuration_keys_navigate_between_the_schema_and_the_yaml() {
    let (backend, dir) = workspace(&[
        (
            "src/DependencyInjection/Configuration.php",
            CONFIGURATION_PHP,
        ),
        ("config/packages/acme_demo.yaml", PACKAGE_YAML),
    ])
    .await;
    let configuration = uri_for(&dir, "src/DependencyInjection/Configuration.php");
    let yaml = uri_for(&dir, "config/packages/acme_demo.yaml");

    let found = definitions(&backend, &yaml, position_in(PACKAGE_YAML, "dsn:", 1)).await;
    assert_eq!(found, vec![(configuration.clone(), 15)]);
    let found = definitions(&backend, &yaml, position_in(PACKAGE_YAML, "acme_demo:", 1)).await;
    assert_eq!(found, vec![(configuration.clone(), 9)]);
    // An installed bundle's key is not one the project declares.
    let found = definitions(&backend, &yaml, position_in(PACKAGE_YAML, "secret:", 1)).await;
    assert!(found.is_empty(), "{found:?}");

    let found = references(
        &backend,
        &configuration,
        position_in(CONFIGURATION_PHP, "'dsn'", 2),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![(configuration.clone(), 15), (yaml.clone(), 3)])
    );

    let hover = hover_text_at(&backend, &yaml, 3, 9).await.expect("a hover");
    assert!(hover.contains("acme_demo.mailer.dsn"), "{hover}");
    assert!(
        hover.contains("src/DependencyInjection/Configuration.php"),
        "{hover}"
    );
}

#[tokio::test]
async fn configuration_keys_complete_under_their_parent() {
    let (backend, dir) = workspace(&[
        (
            "src/DependencyInjection/Configuration.php",
            CONFIGURATION_PHP,
        ),
        ("config/packages/acme_demo.yaml", PACKAGE_YAML),
    ])
    .await;
    let yaml = uri_for(&dir, "config/packages/acme_demo.yaml");

    let labels = completion_labels(&backend, &yaml, Position::new(4, 8)).await;
    assert_eq!(labels, vec!["dsn", "port"]);
    let labels = completion_labels(&backend, &yaml, Position::new(1, 6)).await;
    assert_eq!(labels, vec!["api_key"]);
}
