//! Symfony translation keys and domains, from the YAML and XLIFF catalogues
//! that declare them to the PHP and Twig that translate them.

use crate::common::{
    completion_response, create_psr4_workspace, hover_on, offset_position, open_document,
    response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Symfony\\Component\\Translation\\": "stubs/translation/",
        "Symfony\\Contracts\\Translation\\": "stubs/translation-contracts/"
    } }
}"#;

/// Just enough of the translator for the type engine to tell it from any
/// other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/translation-contracts/TranslatorInterface.php",
        "<?php\nnamespace Symfony\\Contracts\\Translation;\ninterface TranslatorInterface {\n    public function trans(string $id, array $parameters = [], ?string $domain = null, ?string $locale = null): string;\n}\n",
    ),
    (
        "stubs/translation/TranslatableMessage.php",
        "<?php\nnamespace Symfony\\Component\\Translation;\nclass TranslatableMessage {\n    public function __construct(string $message, array $parameters = [], ?string $domain = null) {}\n}\n",
    ),
];

const MESSAGES_YAML: &str = "demo:
    title: Demo
    card: Card
";

const ADMIN_YAML: &str = "demo:
    title: Admin demo
";

const VALIDATORS_XLF: &str = r#"<?xml version="1.0"?>
<xliff version="1.2">
  <file source-language="en" datatype="plaintext" original="file.ext">
    <body>
      <trans-unit id="1" resname="user.invalid_email">
        <source>user.invalid_email</source>
      </trans-unit>
    </body>
  </file>
</xliff>
"#;

const CONTROLLER_PHP: &str = r#"<?php
namespace App\Controller;

use Symfony\Component\Translation\TranslatableMessage;
use Symfony\Contracts\Translation\TranslatorInterface;
use function Symfony\Component\Translation\t;

final class DemoController
{
    public function __construct(private TranslatorInterface $translator) {}

    public function index(\ArrayObject $other, string $domain): void
    {
        $this->translator->trans('demo.title');
        $this->translator->trans('demo.title', [], 'admin');
        $this->translator->trans('user.invalid_email', domain: 'validators');
        $this->translator->trans('demo.card', [], $domain);
        $other->offsetGet('demo.title');
        new TranslatableMessage('demo.card');
        t('demo.title', [], 'admin');
    }
}
"#;

const TEMPLATE_TWIG: &str = "{# 'demo.title'|trans #}
<h1>{{ 'demo.title'|trans }}</h1>
<p>{{ 'demo.title'|trans({}, 'admin') }}</p>
";

/// A workspace holding the stubs and `files`, every one of them open.
async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = STUBS.iter().chain(files).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in files {
        let language = if path.ends_with(".php") {
            "php"
        } else if path.ends_with(".xlf") {
            "xml"
        } else if path.ends_with(".twig") {
            "twig"
        } else {
            "yaml"
        };
        open_document(&backend, &uri_for(&dir, path), language, content).await;
    }
    (backend, dir)
}

/// The catalogues every test translates against.
const CATALOGUES: &[(&str, &str)] = &[
    ("translations/messages.en.yaml", MESSAGES_YAML),
    ("translations/messages.fr.yaml", MESSAGES_YAML),
    ("translations/admin.en.yaml", ADMIN_YAML),
    ("translations/validators.en.xlf", VALIDATORS_XLF),
];

async fn workspace_with_catalogues(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = CATALOGUES.iter().chain(files).copied().collect();
    workspace(&all).await
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
    sorted(
        locations
            .into_iter()
            .map(|location| (location.uri, location.range.start.line))
            .collect(),
    )
}

async fn references(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32)> {
    let found: Vec<(Uri, u32)> = backend
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
    sorted(found)
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
async fn a_translated_key_goes_to_its_entry_in_every_locale_of_its_domain() {
    let (backend, dir) =
        workspace_with_catalogues(&[("src/Controller/DemoController.php", CONTROLLER_PHP)]).await;
    let controller = uri_for(&dir, "src/Controller/DemoController.php");
    let messages_en = uri_for(&dir, "translations/messages.en.yaml");
    let messages_fr = uri_for(&dir, "translations/messages.fr.yaml");
    let admin = uri_for(&dir, "translations/admin.en.yaml");
    let validators = uri_for(&dir, "translations/validators.en.xlf");

    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "trans('demo.title')", 8),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![(messages_en.clone(), 1), (messages_fr.clone(), 1)])
    );

    // The same key in another domain is another key.
    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "trans('demo.title', [], 'admin')", 8),
    )
    .await;
    assert_eq!(found, vec![(admin.clone(), 1)]);

    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "trans('user.invalid_email'", 8),
    )
    .await;
    assert_eq!(found, vec![(validators.clone(), 4)]);

    // A domain goes to the catalogues named after it.
    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "domain: 'validators'", 10),
    )
    .await;
    assert_eq!(found, vec![(validators.clone(), 0)]);

    // A translatable message and `t()` name keys without a translator.
    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "TranslatableMessage('demo.card')", 22),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![(messages_en.clone(), 2), (messages_fr.clone(), 2)])
    );
    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "t('demo.title', [], 'admin')", 4),
    )
    .await;
    assert_eq!(found, vec![(admin.clone(), 1)]);

    // Neither a domain that is not a literal, nor a receiver that is not a
    // translator, says where a key is.
    for needle in ["trans('demo.card', [], $domain)", "offsetGet('demo.title')"] {
        let inside = needle.find('\'').unwrap() + 2;
        let found = definitions(
            &backend,
            &controller,
            position_in(CONTROLLER_PHP, needle, inside),
        )
        .await;
        assert!(found.is_empty(), "{needle}: {found:?}");
    }
}

#[tokio::test]
async fn references_reach_catalogues_php_and_twig_in_one_domain() {
    let (backend, dir) = workspace_with_catalogues(&[
        ("src/Controller/DemoController.php", CONTROLLER_PHP),
        ("templates/demo.html.twig", TEMPLATE_TWIG),
    ])
    .await;
    let controller = uri_for(&dir, "src/Controller/DemoController.php");
    let twig = uri_for(&dir, "templates/demo.html.twig");
    let messages_en = uri_for(&dir, "translations/messages.en.yaml");
    let messages_fr = uri_for(&dir, "translations/messages.fr.yaml");
    let admin = uri_for(&dir, "translations/admin.en.yaml");

    let found = references(
        &backend,
        &messages_en,
        position_in(MESSAGES_YAML, "title", 1),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![
            (controller.clone(), 13),
            (twig.clone(), 1),
            (messages_en.clone(), 1),
            (messages_fr.clone(), 1),
        ])
    );

    let found = references(
        &backend,
        &twig,
        position_in(TEMPLATE_TWIG, "'demo.title'|trans({}", 2),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![
            (controller.clone(), 14),
            (controller.clone(), 19),
            (twig.clone(), 2),
            (admin.clone(), 1),
        ])
    );
}

#[tokio::test]
async fn translation_hover_names_the_domain_and_the_catalogue() {
    let (backend, dir) =
        workspace_with_catalogues(&[("src/Controller/DemoController.php", CONTROLLER_PHP)]).await;
    let controller = uri_for(&dir, "src/Controller/DemoController.php");

    let hover = hover_on(
        &backend,
        &controller,
        CONTROLLER_PHP,
        "demo.title', [], 'admin'",
    )
    .await;
    assert!(hover.contains("**Translation** `demo.title`"), "{hover}");
    assert!(hover.contains("Domain `admin`"), "{hover}");
    assert!(hover.contains("translations/admin.en.yaml"), "{hover}");
}

#[tokio::test]
async fn keys_complete_from_their_domain_in_php_and_twig() {
    let consumer = r#"<?php
use Symfony\Contracts\Translation\TranslatorInterface;

function label(TranslatorInterface $translator, \ArrayObject $other): void {
    $translator->trans('demo.');
    $translator->trans('demo.', [], 'admin');
    $translator->trans('x', [], 'a');
    $other->offsetGet('demo.');
}
"#;
    let twig = "{{ 'demo.'|trans }}\n";
    let (backend, dir) = workspace_with_catalogues(&[
        ("src/consumer.php", consumer),
        ("templates/label.html.twig", twig),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/consumer.php");
    let twig_uri = uri_for(&dir, "templates/label.html.twig");

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "trans('demo.')", 12),
    )
    .await;
    assert_eq!(
        labels,
        vec!["demo.card".to_string(), "demo.title".to_string()]
    );

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "trans('demo.', [], 'admin')", 12),
    )
    .await;
    assert_eq!(labels, vec!["demo.title".to_string()]);

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "[], 'a')", 6),
    )
    .await;
    assert_eq!(labels, vec!["admin".to_string()]);

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "offsetGet('demo.')", 15),
    )
    .await;
    assert!(!labels.contains(&"demo.title".to_string()), "{labels:?}");

    let labels = completion_labels(&backend, &twig_uri, position_in(twig, "'demo.'", 6)).await;
    assert_eq!(
        labels,
        vec!["demo.card".to_string(), "demo.title".to_string()]
    );
}
