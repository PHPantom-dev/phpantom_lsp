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
        "Symfony\\Bundle\\WebProfilerBundle\\": "vendor/symfony/web-profiler-bundle/",
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
        "<?php\nnamespace Symfony\\Component\\Routing\\Loader\\Configurator;\nclass RoutingConfigurator {\n    public function add(string $name, string|array $path): RouteConfigurator {}\n    public function collection(string $name = ''): CollectionConfigurator {}\n    public function import(string|array $resource, ?string $type = null): ImportConfigurator {}\n}\n",
    ),
    (
        "stubs/routing/Loader/Configurator/CollectionConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\Routing\\Loader\\Configurator;\nclass CollectionConfigurator {\n    public function add(string $name, string|array $path): RouteConfigurator {}\n    public function collection(string $name = ''): self {}\n}\n",
    ),
    (
        "stubs/routing/Loader/Configurator/RouteConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\Routing\\Loader\\Configurator;\nclass RouteConfigurator {\n    public function add(string $name, string|array $path): RouteConfigurator {}\n    public function controller(string $controller): static {}\n}\n",
    ),
    (
        "stubs/routing/Loader/Configurator/ImportConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\Routing\\Loader\\Configurator;\nclass ImportConfigurator {\n    public function namePrefix(string $namePrefix): static {}\n    public function prefix(string|array $prefix): static {}\n}\n",
    ),
    (
        "stubs/routing/Annotation/Route.php",
        "<?php\nnamespace Symfony\\Component\\Routing\\Annotation;\nclass Route {}\n",
    ),
    // A bundle that is never opened: its routing files are read from disk.
    (
        "vendor/symfony/web-profiler-bundle/WebProfilerBundle.php",
        "<?php\nnamespace Symfony\\Bundle\\WebProfilerBundle;\nclass WebProfilerBundle {}\n",
    ),
    (
        "vendor/symfony/web-profiler-bundle/Resources/config/routing/wdt.xml",
        "<?xml version=\"1.0\"?>\n<routes>\n    <import resource=\"wdt.php\" type=\"php\"/>\n</routes>\n",
    ),
    (
        "vendor/symfony/web-profiler-bundle/Resources/config/routing/wdt.php",
        "<?php\nuse Symfony\\Component\\Routing\\Loader\\Configurator\\RoutingConfigurator;\n\nreturn static function (RoutingConfigurator $routes): void {\n    $routes->add('_wdt', '/{token}');\n};\n",
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

/// The `invalid_symfony_route` messages a slow-diagnostics pass reports
/// for `content`, in order.
fn route_problems(backend: &Backend, uri: &Uri, content: &str) -> Vec<String> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code)) if code == "invalid_symfony_route"
            )
        })
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect()
}

#[tokio::test]
async fn undeclared_routes_are_reported() {
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

    // `app_check_check` is the name Symfony gives the unnamed route above,
    // and no bundle this application imports routes from declares
    // `_profiler`.
    assert_eq!(
        route_problems(&backend, &consumer_uri, consumer),
        vec![
            "Unknown route: 'app_missing'".to_string(),
            "Unknown route: '_profiler'".to_string(),
        ]
    );
}

/// An application whose routes are put together by the imports that load
/// them: a name prefix on an attribute import, on an XML file it imports,
/// and on a configurator's `import()`, configurator collections, `@Route`
/// annotations, a bundle's routing files, and loaders whose routes cannot be
/// read.
const COMPOSED_ROUTES_YAML: &str = "controllers:
    resource:
        path: ../src/Controller/
        namespace: App\\Controller
    type: attribute

admin:
    resource: ../src/Admin/
    type: attribute
    name_prefix: admin_

shop: { resource: shop.xml, name_prefix: 'shop_' }

when@dev:
    web_profiler_wdt:
        resource: '@WebProfilerBundle/Resources/config/routing/wdt.xml'
        prefix: /_wdt

_security_logout:
    resource: security.route_loader.logout
    type: service

api:
    resource: .
    type: api_platform
    name_prefix: api_
";

const SHOP_XML: &str = r#"<?xml version="1.0"?>
<routes>
    <route id="cart" path="/cart"/>
    <import resource="../src/Shop/" type="attribute" name-prefix="catalog_"/>
</routes>
"#;

const COMPOSED_ROUTES_PHP: &str = r#"<?php
use Symfony\Component\Routing\Loader\Configurator\RoutingConfigurator;

return static function (RoutingConfigurator $routes): void {
    $routes->collection('blog_')
        ->add('index', '/blog')
        ->add('show', '/blog/{slug}');
    $archive = $routes->collection('archive_');
    $archive->add('list', '/archive');
    $routes->import('../src/Legacy/', 'annotation')->namePrefix('old_');
};
"#;

const ADMIN_PHP: &str = r#"<?php
namespace App\Admin;

use Symfony\Component\Routing\Attribute\Route;

final class PostController
{
    #[Route('/admin/post', name: 'post_edit')]
    public function edit(): void {}
}
"#;

const SHOP_PHP: &str = r#"<?php
namespace App\Shop;

use Symfony\Component\Routing\Attribute\Route;

final class ProductController
{
    #[Route('/product', name: 'product')]
    public function show(): void {}
}
"#;

const LEGACY_PHP: &str = r#"<?php
namespace App\Legacy;

use Symfony\Component\Routing\Annotation\Route;

/**
 * @Route("/legacy", name="legacy_")
 */
final class LegacyController
{
    /**
     * @Route("/page", name="page", methods={"GET"})
     */
    public function page(): void {}
}
"#;

const COMPOSED_CONSUMER: &str = r#"<?php
namespace App\Controller;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;

final class LinkController extends AbstractController
{
    public function links(): void
    {
        $this->generateUrl('admin_post_edit');
        $this->generateUrl('post_edit');
        $this->generateUrl('shop_cart');
        $this->generateUrl('cart');
        $this->generateUrl('shop_catalog_product');
        $this->generateUrl('blog_index');
        $this->generateUrl('blog_show');
        $this->generateUrl('archive_list');
        $this->generateUrl('old_legacy_page');
        $this->generateUrl('_wdt');
        $this->generateUrl('_logout_main');
        $this->generateUrl('api_anything');
        $this->generateUrl('app_missing');
    }
}
"#;

async fn composed_workspace() -> (Backend, tempfile::TempDir) {
    workspace(&[
        ("config/routes.yaml", COMPOSED_ROUTES_YAML),
        ("config/routes.php", COMPOSED_ROUTES_PHP),
        ("config/shop.xml", SHOP_XML),
        ("src/Admin/PostController.php", ADMIN_PHP),
        ("src/Shop/ProductController.php", SHOP_PHP),
        ("src/Legacy/LegacyController.php", LEGACY_PHP),
        ("src/Controller/LinkController.php", COMPOSED_CONSUMER),
    ])
    .await
}

#[tokio::test]
async fn names_the_imports_compose_are_declared() {
    let (backend, dir) = composed_workspace().await;
    let consumer = uri_for(&dir, "src/Controller/LinkController.php");

    // Only the names the imports do not give are reported; the security
    // bundle's logout routes and the custom loader's are not read.
    assert_eq!(
        route_problems(&backend, &consumer, COMPOSED_CONSUMER),
        vec![
            "Unknown route: 'post_edit'".to_string(),
            "Unknown route: 'cart'".to_string(),
            "Unknown route: 'app_missing'".to_string(),
        ]
    );
}

#[tokio::test]
async fn a_composed_name_goes_to_its_declaration() {
    let (backend, dir) = composed_workspace().await;
    let consumer = uri_for(&dir, "src/Controller/LinkController.php");

    for (needle, file, line) in [
        ("'admin_post_edit'", "src/Admin/PostController.php", 7),
        ("'shop_cart'", "config/shop.xml", 2),
        (
            "'shop_catalog_product'",
            "src/Shop/ProductController.php",
            7,
        ),
        ("'blog_show'", "config/routes.php", 6),
        ("'archive_list'", "config/routes.php", 8),
        ("'old_legacy_page'", "src/Legacy/LegacyController.php", 11),
        (
            "'_wdt'",
            "vendor/symfony/web-profiler-bundle/Resources/config/routing/wdt.php",
            4,
        ),
    ] {
        let found = definitions(
            &backend,
            &consumer,
            position_in(COMPOSED_CONSUMER, needle, 2),
        )
        .await;
        assert_eq!(found, vec![(uri_for(&dir, file), line)], "for {needle}");
    }

    // References include the declaration the name is composed from.
    let found = references(
        &backend,
        &consumer,
        position_in(COMPOSED_CONSUMER, "'admin_post_edit'", 2),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![
            (uri_for(&dir, "src/Admin/PostController.php"), 7),
            (consumer.clone(), 9),
        ])
    );
}

#[tokio::test]
async fn composed_names_complete() {
    let consumer = r#"<?php
use Symfony\Component\Routing\Generator\UrlGeneratorInterface;

function link(UrlGeneratorInterface $urls): void {
    $urls->generate('');
}
"#;
    let (backend, dir) = workspace(&[
        ("config/routes.yaml", COMPOSED_ROUTES_YAML),
        ("config/routes.php", COMPOSED_ROUTES_PHP),
        ("config/shop.xml", SHOP_XML),
        ("src/Admin/PostController.php", ADMIN_PHP),
        ("src/Shop/ProductController.php", SHOP_PHP),
        ("src/Legacy/LegacyController.php", LEGACY_PHP),
        ("src/consumer.php", consumer),
    ])
    .await;
    let labels = completion_labels(
        &backend,
        &uri_for(&dir, "src/consumer.php"),
        position_in(consumer, "generate('')", 10),
    )
    .await;
    assert_eq!(
        labels,
        [
            "_wdt",
            "admin_post_edit",
            "archive_list",
            "blog_index",
            "blog_show",
            "old_legacy_page",
            "shop_cart",
            "shop_catalog_product",
        ]
        .map(String::from)
    );
}
