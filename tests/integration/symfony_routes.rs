//! Symfony route names, from their declarations in `#[Route]` attributes,
//! routing YAML and XML, and PHP routing configurators to the PHP and Twig
//! that generate their URLs.
use crate::common::message_text;

use crate::common::{
    completion_response, create_psr4_workspace, offset_position, open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Symfony\\Bundle\\FrameworkBundle\\": "stubs/framework-bundle/",
        "Symfony\\Component\\Routing\\": "stubs/routing/"
    } }
}"#;

/// Just enough of the router for the type engine to tell a URL generator
/// and a controller from any other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/framework-bundle/Controller/AbstractController.php",
        "<?php\nnamespace Symfony\\Bundle\\FrameworkBundle\\Controller;\nabstract class AbstractController {\n    protected function generateUrl(string $route, array $parameters = []): string {}\n    protected function redirectToRoute(string $route, array $parameters = []): object {}\n}\n",
    ),
    (
        "stubs/routing/Attribute/Route.php",
        "<?php\nnamespace Symfony\\Component\\Routing\\Attribute;\n#[\\Attribute]\nclass Route {\n    public function __construct(string|array|null $path = null, ?string $name = null, string|array $alias = []) {}\n}\n",
    ),
    (
        "stubs/routing/Generator/UrlGeneratorInterface.php",
        "<?php\nnamespace Symfony\\Component\\Routing\\Generator;\ninterface UrlGeneratorInterface {\n    public function generate(string $name, array $parameters = []): string;\n}\n",
    ),
    (
        "stubs/routing/Loader/Configurator/RoutingConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\Routing\\Loader\\Configurator;\nclass RoutingConfigurator {\n    public function add(string $name, string|array $path): object {}\n}\n",
    ),
];

const CONTROLLER_PHP: &str = r#"<?php
namespace App\Controller;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\Routing\Attribute\Route;
use Symfony\Component\Routing\Generator\UrlGeneratorInterface;

#[Route('/admin/post', name: 'admin_post_')]
final class PostController extends AbstractController
{
    #[Route('/', name: 'index', alias: ['admin_posts'])]
    public function index(): object
    {
        return $this->redirectToRoute('app_home');
    }

    #[Route('/{id}')]
    public function show(UrlGeneratorInterface $urls, \ArrayObject $other): string
    {
        $other->offsetGet('admin_post_index');
        return $urls->generate('admin_post_index');
    }
}
"#;

const INVOKABLE_PHP: &str = r#"<?php
namespace App\Controller;

use Symfony\Component\Routing\Attribute\Route;

#[Route('/about', name: 'app_about')]
final class AboutController
{
    public function __invoke(): void {}
}
"#;

const ROUTES_YAML: &str = "app_home:
    path: /
    controller: App\\Controller\\HomeController::index

app_old_home:
    alias: app_home

controllers:
    resource: ../src/Controller/
    type: attribute
";

const ROUTES_PHP: &str = r#"<?php
use Symfony\Component\Routing\Loader\Configurator\RoutingConfigurator;

return static function (RoutingConfigurator $routes, \ArrayObject $other): void {
    $routes->add('app_legacy', '/legacy');
    $other->offsetSet('not_a_route', '/x');
};
"#;

const TEMPLATE_TWIG: &str = "{# path('commented_out') #}
<a href=\"{{ path('app_home') }}\">Home</a>
<a href=\"{{ url('admin_post_index') }}\">Posts</a>
";

/// A workspace holding the stubs and `files`, every one of them open.
async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = STUBS.iter().chain(files).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in files {
        let language = if path.ends_with(".php") {
            "php"
        } else if path.ends_with(".xml") {
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
async fn a_generated_url_goes_to_the_route_attribute_with_its_class_prefix() {
    let (backend, dir) = workspace(&[("src/Controller/PostController.php", CONTROLLER_PHP)]).await;
    let controller = uri_for(&dir, "src/Controller/PostController.php");

    // `index` under the class's `admin_post_` prefix.
    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "generate('admin_post_index')", 12),
    )
    .await;
    assert_eq!(found, vec![(controller.clone(), 10)]);

    // `offsetGet()` on something that is not a URL generator names nothing.
    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "offsetGet('admin_post_index')", 12),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn references_reach_yaml_twig_and_php() {
    let (backend, dir) = workspace(&[
        ("config/routes.yaml", ROUTES_YAML),
        ("src/Controller/PostController.php", CONTROLLER_PHP),
        ("templates/home.html.twig", TEMPLATE_TWIG),
    ])
    .await;
    let yaml = uri_for(&dir, "config/routes.yaml");
    let controller = uri_for(&dir, "src/Controller/PostController.php");
    let twig = uri_for(&dir, "templates/home.html.twig");

    let found = references(&backend, &yaml, position_in(ROUTES_YAML, "app_home:", 2)).await;
    assert_eq!(
        found,
        sorted(vec![
            (controller.clone(), 13),
            (twig.clone(), 1),
            (yaml.clone(), 0),
            (yaml.clone(), 5),
        ])
    );

    // From the template, to the attribute it names.
    let found = definitions(
        &backend,
        &twig,
        position_in(TEMPLATE_TWIG, "url('admin_post_index')", 6),
    )
    .await;
    assert_eq!(found, vec![(controller.clone(), 10)]);

    // The alias names the route above it.
    let found = definitions(
        &backend,
        &yaml,
        position_in(ROUTES_YAML, "alias: app_home", 9),
    )
    .await;
    assert_eq!(found, vec![(yaml.clone(), 0)]);
}

#[tokio::test]
async fn invokable_controllers_xml_and_php_configurators_declare_routes() {
    let routes_xml = r#"<?xml version="1.0"?>
<routes>
    <route id="app_xml" path="/xml"/>
</routes>
"#;
    let consumer = r#"<?php
namespace App\Controller;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;

final class LinkController extends AbstractController
{
    public function links(): void
    {
        $this->generateUrl('app_about');
        $this->generateUrl('app_xml');
        $this->generateUrl('app_legacy');
    }
}
"#;
    let (backend, dir) = workspace(&[
        ("config/routes.xml", routes_xml),
        ("config/routes.php", ROUTES_PHP),
        ("src/Controller/AboutController.php", INVOKABLE_PHP),
        ("src/Controller/LinkController.php", consumer),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/Controller/LinkController.php");

    for (needle, file, line) in [
        ("'app_about'", "src/Controller/AboutController.php", 5),
        ("'app_xml'", "config/routes.xml", 2),
        ("'app_legacy'", "config/routes.php", 4),
    ] {
        let found = definitions(&backend, &consumer_uri, position_in(consumer, needle, 3)).await;
        assert_eq!(found, vec![(uri_for(&dir, file), line)], "for {needle}");
    }
}

#[tokio::test]
async fn route_names_complete_in_php_and_twig() {
    let consumer = r#"<?php
use Symfony\Component\Routing\Generator\UrlGeneratorInterface;

function link(UrlGeneratorInterface $urls, \ArrayObject $other): void {
    $urls->generate('app_');
    $other->offsetGet('app_');
}
"#;
    let twig = "{{ path('app_') }}\n";
    let (backend, dir) = workspace(&[
        ("config/routes.yaml", ROUTES_YAML),
        ("src/consumer.php", consumer),
        ("templates/link.html.twig", twig),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/consumer.php");
    let twig_uri = uri_for(&dir, "templates/link.html.twig");

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "generate('app_'", 14),
    )
    .await;
    assert_eq!(
        labels,
        vec!["app_home".to_string(), "app_old_home".to_string()]
    );

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "offsetGet('app_'", 15),
    )
    .await;
    assert!(!labels.contains(&"app_home".to_string()), "{labels:?}");

    let labels = completion_labels(&backend, &twig_uri, position_in(twig, "'app_'", 5)).await;
    assert_eq!(
        labels,
        vec!["app_home".to_string(), "app_old_home".to_string()]
    );
}

#[tokio::test]
async fn only_undeclared_application_routes_are_reported() {
    let consumer = r#"<?php
namespace App\Controller;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\Routing\Attribute\Route;

final class CheckController extends AbstractController
{
    #[Route('/check')]
    public function check(): void
    {
        $this->generateUrl('app_home');
        $this->generateUrl('app_missing');
        $this->generateUrl('_profiler');
        $this->generateUrl('app_check_check');
        $this->generateUrl('admin_posts');
    }
}
"#;
    let (backend, dir) = workspace(&[
        ("config/routes.yaml", ROUTES_YAML),
        ("src/Controller/PostController.php", CONTROLLER_PHP),
        ("src/Controller/CheckController.php", consumer),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/Controller/CheckController.php");

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(consumer_uri.as_str(), consumer, &mut diagnostics);
    let reported: Vec<String> = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code)) if code == "invalid_symfony_route"
            )
        })
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect();
    // `app_check_check` is the name Symfony gives the unnamed route above.
    assert_eq!(reported, vec!["Unknown route: 'app_missing'".to_string()]);
}
