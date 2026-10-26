//! Twig template names, from the templates a Symfony project holds to the
//! controllers, emails, and other templates that name them.
use crate::common::message_text;

use crate::common::{
    completion_response, create_psr4_workspace, find_action, get_code_actions_in_range,
    offset_position, open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Symfony\\Bundle\\FrameworkBundle\\": "stubs/framework-bundle/",
        "Symfony\\Bridge\\Twig\\": "stubs/twig-bridge/",
        "Twig\\": "stubs/twig/"
    } }
}"#;

/// Just enough of the framework for the type engine to tell what renders a
/// template from any other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/framework-bundle/Controller/AbstractController.php",
        "<?php\nnamespace Symfony\\Bundle\\FrameworkBundle\\Controller;\nabstract class AbstractController {\n    protected function render(string $view, array $parameters = []): object {}\n    protected function renderView(string $view, array $parameters = []): string {}\n}\n",
    ),
    (
        "stubs/twig-bridge/Attribute/Template.php",
        "<?php\nnamespace Symfony\\Bridge\\Twig\\Attribute;\n#[\\Attribute]\nclass Template {\n    public function __construct(public string $template, public ?array $vars = null) {}\n}\n",
    ),
    (
        "stubs/twig-bridge/Mime/TemplatedEmail.php",
        "<?php\nnamespace Symfony\\Bridge\\Twig\\Mime;\nclass TemplatedEmail {\n    public function htmlTemplate(?string $template): static {}\n    public function textTemplate(?string $template): static {}\n}\n",
    ),
    (
        "stubs/twig/Environment.php",
        "<?php\nnamespace Twig;\nclass Environment {\n    public function render($name, array $context = []): string {}\n}\n",
    ),
];

const BASE_TWIG: &str = "<main>{% block body %}{% endblock %}</main>\n";
const CARD_TWIG: &str = "<article>Card</article>\n";
const PAGE_TWIG: &str = "{% extends 'base.html.twig' %}
{% block body %}
  {% include 'partials/card.html.twig' %}
{% endblock %}
";

const CONTROLLER_PHP: &str = r#"<?php
namespace App\Controller;

use Symfony\Bridge\Twig\Attribute\Template;
use Symfony\Bridge\Twig\Mime\TemplatedEmail;
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Twig\Environment;

final class PageController extends AbstractController
{
    public function show(Environment $twig, \ArrayObject $other): object
    {
        $twig->render('partials/card.html.twig');
        $other->offsetGet('page.html.twig');
        (new TemplatedEmail())->htmlTemplate('partials/card.html.twig');
        return $this->render('page.html.twig');
    }

    #[Template('page.html.twig')]
    public function attribute(): array
    {
        return [];
    }
}
"#;

/// A workspace holding the stubs and `files`, every one of them open.
async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = STUBS.iter().chain(files).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in files {
        let language = if path.ends_with(".php") {
            "php"
        } else {
            "twig"
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

async fn completion_labels(backend: &Backend, uri: &Uri, position: Position) -> Vec<String> {
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

fn page_workspace() -> [(&'static str, &'static str); 4] {
    [
        ("templates/base.html.twig", BASE_TWIG),
        ("templates/partials/card.html.twig", CARD_TWIG),
        ("templates/page.html.twig", PAGE_TWIG),
        ("src/Controller/PageController.php", CONTROLLER_PHP),
    ]
}

#[tokio::test]
async fn template_names_go_to_their_files() {
    let (backend, dir) = workspace(&page_workspace()).await;
    let controller = uri_for(&dir, "src/Controller/PageController.php");
    let page = uri_for(&dir, "templates/page.html.twig");
    let card = uri_for(&dir, "templates/partials/card.html.twig");
    let base = uri_for(&dir, "templates/base.html.twig");

    for (uri, content, needle, expected) in [
        (&controller, CONTROLLER_PHP, "render('page", &page),
        (&controller, CONTROLLER_PHP, "Template('page", &page),
        (&controller, CONTROLLER_PHP, "render('partials", &card),
        (&controller, CONTROLLER_PHP, "htmlTemplate('partials", &card),
        (&page, PAGE_TWIG, "extends 'base", &base),
        (&page, PAGE_TWIG, "include 'partials", &card),
    ] {
        let inside = needle.find('\'').unwrap() + 2;
        let found = definitions(&backend, uri, position_in(content, needle, inside)).await;
        assert_eq!(found, vec![(expected.clone(), 0)], "for {needle}");
    }

    // `offsetGet()` on something that renders nothing names nothing.
    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "offsetGet('page", 12),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn references_reach_php_and_twig() {
    let (backend, dir) = workspace(&page_workspace()).await;
    let controller = uri_for(&dir, "src/Controller/PageController.php");
    let page = uri_for(&dir, "templates/page.html.twig");
    let card = uri_for(&dir, "templates/partials/card.html.twig");

    let found = references(&backend, &page, position_in(PAGE_TWIG, "'partials/card", 3)).await;
    let mut expected = vec![
        (card.clone(), 0),
        (controller.clone(), 12),
        (controller.clone(), 14),
        (page.clone(), 2),
    ];
    expected.sort();
    assert_eq!(found, expected);
}

#[tokio::test]
async fn a_bundle_override_is_named_by_the_bundle_namespace() {
    let consumer = "{% include '@Acme/widget.html.twig' %}\n";
    let (backend, dir) = workspace(&[
        (
            "templates/bundles/AcmeBundle/widget.html.twig",
            "<div>Widget</div>\n",
        ),
        ("templates/consumer.html.twig", consumer),
    ])
    .await;
    let found = definitions(
        &backend,
        &uri_for(&dir, "templates/consumer.html.twig"),
        position_in(consumer, "@Acme", 3),
    )
    .await;
    assert_eq!(
        found,
        vec![(
            uri_for(&dir, "templates/bundles/AcmeBundle/widget.html.twig"),
            0
        )]
    );
}

#[tokio::test]
async fn template_names_complete_in_php_and_twig() {
    let consumer = r#"<?php
namespace App\Controller;

use Symfony\Bridge\Twig\Attribute\Template;
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;

final class ListController extends AbstractController
{
    #[Template('pa')]
    public function list(\ArrayObject $other): object
    {
        $other->offsetGet('pa');
        return $this->render('pa');
    }
}
"#;
    let twig = "{% include 'pa' %}\n";
    let (backend, dir) = workspace(&[
        ("templates/base.html.twig", BASE_TWIG),
        ("templates/partials/card.html.twig", CARD_TWIG),
        ("templates/page.html.twig", PAGE_TWIG),
        ("src/Controller/ListController.php", consumer),
        ("templates/list.html.twig", twig),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/Controller/ListController.php");
    let twig_uri = uri_for(&dir, "templates/list.html.twig");
    let expected = vec![
        "page.html.twig".to_string(),
        "partials/card.html.twig".to_string(),
    ];

    for needle in ["render('pa'", "Template('pa'"] {
        let inside = needle.len() - 1;
        let labels = completion_labels(
            &backend,
            &consumer_uri,
            position_in(consumer, needle, inside),
        )
        .await;
        assert_eq!(labels, expected, "for {needle}");
    }

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "offsetGet('pa'", 13),
    )
    .await;
    assert!(
        !labels.contains(&"page.html.twig".to_string()),
        "{labels:?}"
    );

    let labels = completion_labels(&backend, &twig_uri, position_in(twig, "'pa'", 3)).await;
    assert_eq!(labels, expected);
}

const MISSING_PHP: &str = r#"<?php
namespace App\Controller;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;

final class MissingController extends AbstractController
{
    public function show(): object
    {
        $this->renderView('page.html.twig');
        $this->renderView('@Vendor/external.html.twig');
        return $this->render('missing/page.html.twig');
    }
}
"#;

#[tokio::test]
async fn only_missing_project_templates_are_reported() {
    let (backend, dir) = workspace(&[
        ("templates/page.html.twig", PAGE_TWIG),
        ("src/Controller/MissingController.php", MISSING_PHP),
    ])
    .await;
    let uri = uri_for(&dir, "src/Controller/MissingController.php");

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), MISSING_PHP, &mut diagnostics);
    let reported: Vec<String> = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code)) if code == "invalid_symfony_template"
            )
        })
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect();
    assert_eq!(
        reported,
        vec!["Unknown template: 'missing/page.html.twig'".to_string()]
    );
}

#[tokio::test]
async fn a_missing_template_can_be_created() {
    let (backend, dir) = workspace(&[
        ("templates/page.html.twig", PAGE_TWIG),
        ("src/Controller/MissingController.php", MISSING_PHP),
    ])
    .await;
    let uri = uri_for(&dir, "src/Controller/MissingController.php");
    let line = position_in(MISSING_PHP, "missing/page", 0).line;

    let actions = get_code_actions_in_range(
        &backend,
        uri.as_str(),
        MISSING_PHP,
        Range::new(Position::new(line, 0), Position::new(line, 200)),
    );
    let action = find_action(&actions, "Create missing template 'missing/page.html.twig'")
        .expect("should offer to create the template");
    let Some(ops) = action
        .edit
        .as_ref()
        .and_then(|edit| edit.document_changes.as_ref())
    else {
        panic!("expected resource operations");
    };
    match &ops[0] {
        DocumentChange::CreateFile(create) => assert_eq!(
            create.uri.to_file_path().unwrap(),
            dir.path().join("templates/missing/page.html.twig")
        ),
        _ => panic!("expected a CreateFile operation"),
    }

    // A template that exists, and a bundle's, are not offered.
    for needle in ["'page.html.twig'", "@Vendor"] {
        let line = position_in(MISSING_PHP, needle, 0).line;
        let actions = get_code_actions_in_range(
            &backend,
            uri.as_str(),
            MISSING_PHP,
            Range::new(Position::new(line, 0), Position::new(line, 200)),
        );
        assert!(
            find_action(&actions, "Create missing template").is_none(),
            "for {needle}"
        );
    }
}

/// A Twig template or a YAML file belongs to another extension: PHPantom
/// answers there for the names it reads, and leaves formatting,
/// highlighting, outline, folding, and the rest to that extension.
#[tokio::test]
async fn resource_documents_answer_only_for_the_names_they_hold() {
    const SERVICES_YAML: &str =
        "services:\n    app.mailer:\n        class: App\\Controller\\PageController\n";
    let (backend, dir) = workspace(&page_workspace()).await;
    let services = uri_for(&dir, "config/services.yaml");
    open_document(&backend, &services, "yaml", SERVICES_YAML).await;
    let page = uri_for(&dir, "templates/page.html.twig");
    let controller = uri_for(&dir, "src/Controller/PageController.php");

    // What the server owns there still answers.
    let found = definitions(&backend, &page, position_in(PAGE_TWIG, "extends 'base", 10)).await;
    assert_eq!(found, vec![(uri_for(&dir, "templates/base.html.twig"), 0)]);

    for (uri, content) in [(&page, PAGE_TWIG), (&services, SERVICES_YAML)] {
        let document = TextDocumentIdentifier { uri: uri.clone() };
        let at = TextDocumentPositionParams {
            text_document: document.clone(),
            position: Position::new(1, 3),
        };
        let whole = Range::new(Position::new(0, 0), offset_position(content, content.len()));
        let options = FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            ..FormattingOptions::default()
        };

        let semantic_tokens = backend
            .semantic_tokens_full(SemanticTokensParams {
                text_document: document.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap();
        assert!(semantic_tokens.is_none(), "semantic tokens in {uri:?}");

        let formatting = backend
            .formatting(DocumentFormattingParams {
                text_document: document.clone(),
                options,
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap();
        assert!(formatting.is_none(), "formatting in {uri:?}");

        let range_formatting = backend
            .range_formatting(DocumentRangeFormattingParams {
                text_document: document.clone(),
                range: whole,
                options,
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap();
        assert!(range_formatting.is_none(), "range formatting in {uri:?}");

        let folding = backend
            .folding_range(FoldingRangeParams {
                text_document: document.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap();
        assert!(folding.is_none(), "folding in {uri:?}");

        let symbols = backend
            .document_symbol(DocumentSymbolParams {
                text_document: document.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap();
        assert!(symbols.is_none(), "document symbols in {uri:?}");

        let hints = backend
            .inlay_hint(InlayHintParams {
                text_document: document.clone(),
                range: whole,
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap();
        assert!(hints.is_none(), "inlay hints in {uri:?}");

        let lenses = backend
            .code_lens(CodeLensParams {
                text_document: document.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap();
        assert!(lenses.is_none(), "code lenses in {uri:?}");

        let links = backend
            .document_link(DocumentLinkParams {
                text_document: document.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap();
        assert!(links.is_none(), "document links in {uri:?}");

        let selection = backend
            .selection_range(SelectionRangeParams {
                text_document: document.clone(),
                positions: vec![at.position],
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap();
        assert!(selection.is_none(), "selection ranges in {uri:?}");

        let signature = backend
            .signature_help(SignatureHelpParams {
                text_document_position_params: at.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                context: None,
            })
            .await
            .unwrap();
        assert!(signature.is_none(), "signature help in {uri:?}");

        let highlights = backend
            .document_highlight(DocumentHighlightParams {
                text_document_position_params: at.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap();
        assert!(highlights.is_none(), "highlights in {uri:?}");

        let actions = backend
            .code_action(CodeActionParams {
                text_document: document,
                range: whole,
                context: CodeActionContext::default(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap();
        assert!(actions.is_none(), "code actions in {uri:?}: {actions:?}");
    }

    // A PHP file is still highlighted and folded.
    let php = TextDocumentIdentifier {
        uri: controller.clone(),
    };
    let semantic_tokens = backend
        .semantic_tokens_full(SemanticTokensParams {
            text_document: php.clone(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    assert!(semantic_tokens.is_some());
    let folding = backend
        .folding_range(FoldingRangeParams {
            text_document: php,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    assert!(folding.is_some());
}
