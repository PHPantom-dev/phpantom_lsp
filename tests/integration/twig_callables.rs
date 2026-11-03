//! Twig filters and functions: from a template's `|price` and `icon()` to the
//! PHP behind them, in the project's extensions and in Twig's own.
use crate::common::{
    completion_response, hover_text, initialize_with_options, message_text, offset_position,
    open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "twig/twig": "^3.4" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const INSTALLED: &str = r#"{ "packages": [ {
    "name": "twig/twig",
    "install-path": "../twig/twig",
    "autoload": { "psr-4": { "Twig\\": "src/" } }
} ] }"#;

const CORE_EXTENSION: &str = r#"<?php
namespace Twig\Extension;

use Twig\TwigFilter;
use Twig\TwigFunction;

final class CoreExtension extends AbstractExtension
{
    public function getFilters(): array
    {
        return [
            new TwigFilter('upper', [self::class, 'upper']),
            new TwigFilter('raw', null),
        ];
    }

    public function getFunctions(): array
    {
        return [
            new TwigFunction('parent', null),
            new TwigFunction('range', 'range'),
        ];
    }

    /**
     * Converts a string to uppercase.
     */
    public static function upper(string $string): string
    {
        return strtoupper($string);
    }
}
"#;

const APP_EXTENSION: &str = r#"<?php
namespace App\Twig;

use Twig\Attribute\AsTwigFunction;
use Twig\Extension\AbstractExtension;
use Twig\TwigFilter;

final class AppExtension extends AbstractExtension
{
    public function getFilters(): array
    {
        return [new TwigFilter('price', [$this, 'formatPrice'])];
    }

    public function formatPrice(float $amount, string $currency = 'EUR'): string
    {
        return '';
    }

    #[AsTwigFunction('icon')]
    public function icon(string $name): string
    {
        return '';
    }
}
"#;

const BASE: &str = "<main>{% block body %}{% endblock %}</main>\n";
const PAGE: &str = "{% extends 'base.html.twig' %}
{% from 'macros.html.twig' import card %}
{% block body %}{{ parent() }}
{{ 9.5|price('DKK')|upper }}{{ html|raw }}
{% for i in range(1, 3) %}{{ icon('star') }}{{ card(i) }}{% endfor %}
{{ total|prise }}{{ ikon('x') }}
{% endblock %}
";
const MACROS: &str = "{% macro card(i) %}{{ i }}{% endmacro %}\n";

const PROJECT: &[(&str, &str)] = &[
    ("composer.json", COMPOSER),
    ("src/Twig/AppExtension.php", APP_EXTENSION),
    ("templates/base.html.twig", BASE),
    ("templates/page.html.twig", PAGE),
    ("templates/macros.html.twig", MACROS),
];

const VENDOR: &[(&str, &str)] = &[
    ("vendor/composer/installed.json", INSTALLED),
    (
        "vendor/twig/twig/src/Extension/CoreExtension.php",
        CORE_EXTENSION,
    ),
];

/// A session over `files`, with the page open.
async fn workspace(files: &[&[(&str, &str)]]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files.iter().copied().flatten() {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    open_document(&backend, &page_uri(&dir), "twig", PAGE).await;
    (backend, dir)
}

fn page_uri(dir: &tempfile::TempDir) -> Uri {
    Uri::from_file_path(dir.path().join("templates/page.html.twig")).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

/// The line `needle` is first found on.
fn line_of(content: &str, needle: &str) -> u32 {
    position_in(content, needle, 0).line
}

fn text_document_position(uri: &Uri, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position,
    }
}

fn named(locations: Vec<Location>) -> Vec<(String, u32)> {
    let mut found: Vec<(String, u32)> = locations
        .into_iter()
        .map(|location| {
            let path = location.uri.path().to_string();
            let file = path.rsplit('/').next().unwrap().to_string();
            (file, location.range.start.line)
        })
        .collect();
    found.sort();
    found
}

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<(String, u32)> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: text_document_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    named(match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    })
}

async fn hover(backend: &Backend, uri: &Uri, position: Position) -> String {
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: text_document_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("a hover");
    hover_text(&hover).to_string()
}

/// The Twig filter and function problems a pass reports for the page.
fn problems(backend: &Backend, uri: &Uri) -> Vec<(String, u32)> {
    let mut diagnostics = Vec::new();
    backend.collect_template_diagnostics(uri.as_str(), PAGE, &mut diagnostics);
    diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code))
                    if code == "invalid_twig_filter" || code == "invalid_twig_function"
            )
        })
        .map(|diagnostic| {
            (
                message_text(&diagnostic.message).to_string(),
                diagnostic.range.start.line,
            )
        })
        .collect()
}

#[tokio::test]
async fn filters_and_functions_lead_to_the_php_behind_them() {
    let (backend, dir) = workspace(&[PROJECT, VENDOR]).await;
    let page = page_uri(&dir);

    for (needle, inside, expected) in [
        (
            "price(",
            2,
            (
                "AppExtension.php",
                line_of(APP_EXTENSION, "function formatPrice"),
            ),
        ),
        (
            "icon(",
            2,
            ("AppExtension.php", line_of(APP_EXTENSION, "function icon")),
        ),
        (
            "upper }}",
            2,
            (
                "CoreExtension.php",
                line_of(CORE_EXTENSION, "function upper"),
            ),
        ),
        // A filter Twig compiles itself has only its declaration.
        (
            "raw }}",
            1,
            ("CoreExtension.php", line_of(CORE_EXTENSION, "'raw'")),
        ),
        // `parent()` renders the block it overrides.
        ("parent()", 2, ("base.html.twig", 0)),
    ] {
        let found = definitions(&backend, &page, position_in(PAGE, needle, inside)).await;
        assert_eq!(
            found,
            vec![(expected.0.to_string(), expected.1)],
            "for {needle}"
        );
    }
}

#[tokio::test]
async fn hover_shows_the_signature_behind_a_filter() {
    let (backend, dir) = workspace(&[PROJECT, VENDOR]).await;
    let page = page_uri(&dir);

    let text = hover(&backend, &page, position_in(PAGE, "price(", 2)).await;
    assert!(text.contains("**Twig filter** `price`"), "{text}");
    assert!(
        text.contains(
            "public function formatPrice(float $amount, string $currency = 'EUR'): string"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "Runs `App\\Twig\\AppExtension::formatPrice()`, declared in `src/Twig/AppExtension.php`"
        ),
        "{text}"
    );

    let text = hover(&backend, &page, position_in(PAGE, "upper }}", 2)).await;
    assert!(text.contains("Converts a string to uppercase."), "{text}");
}

#[tokio::test]
async fn filters_and_functions_complete_apart() {
    let content = "{{ x| }}{% if  %}{% endif %}\n";
    let (backend, dir) = workspace(&[PROJECT, VENDOR]).await;
    let uri = Uri::from_file_path(dir.path().join("templates/new.html.twig")).unwrap();
    std::fs::write(dir.path().join("templates/new.html.twig"), content).unwrap();
    open_document(&backend, &uri, "twig", content).await;

    let labels = |position: Position| {
        let backend = &backend;
        let uri = uri.clone();
        async move {
            response_labels(
                completion_response(backend, &uri, position.line, position.character, None).await,
            )
        }
    };
    assert_eq!(
        labels(position_in(content, "| }}", 1)).await,
        ["price", "raw", "upper"]
    );
    // Where an expression starts, the variables every template has come
    // before the functions.
    assert_eq!(
        labels(position_in(content, "if  %}", 3)).await,
        ["_charset", "_context", "_self", "icon", "parent", "range"]
    );
}

#[tokio::test]
async fn references_reach_the_declaration_and_the_templates() {
    let (backend, dir) = workspace(&[PROJECT, VENDOR]).await;
    let page = page_uri(&dir);
    let locations = backend
        .references(ReferenceParams {
            text_document_position_params: text_document_position(
                &page,
                position_in(PAGE, "price(", 2),
            ),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default();
    assert_eq!(
        named(locations),
        vec![
            (
                "AppExtension.php".to_string(),
                line_of(APP_EXTENSION, "'price'")
            ),
            ("page.html.twig".to_string(), line_of(PAGE, "price(")),
        ]
    );
}

#[tokio::test]
async fn names_no_extension_declares_are_reported() {
    let (backend, dir) = workspace(&[PROJECT, VENDOR]).await;
    let line = line_of(PAGE, "prise");
    assert_eq!(
        problems(&backend, &page_uri(&dir)),
        vec![
            ("Unknown Twig filter: 'prise'".to_string(), line),
            ("Unknown Twig function: 'ikon'".to_string(), line),
        ]
    );
}

#[tokio::test]
async fn nothing_is_reported_before_twig_itself_is_read() {
    let (backend, dir) = workspace(&[PROJECT]).await;
    assert_eq!(problems(&backend, &page_uri(&dir)), vec![]);
}
