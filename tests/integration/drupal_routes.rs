//! Drupal's route names: the keys a routing file declares, read by the
//! calls that build a URL or a redirect to a route.

use crate::common::{completion_response, hover_text_at, open_document, response_labels};
use crate::drupal_services::{
    definitions_at, position_in, references_at, uri_for, workspace, workspace_in,
};
use tower_lsp_server::gen_lsp_types::Position;

/// Just enough of core's routing API, where Composer would put core.
const ROUTING_STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Core/Url.php",
        "<?php\nnamespace Drupal\\Core;\nclass Url {\n    public function __construct($route_name, $route_parameters = [], $options = []) {}\n    public static function fromRoute($route_name, $route_parameters = [], $options = []) { return new static($route_name); }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Link.php",
        "<?php\nnamespace Drupal\\Core;\nclass Link {\n    public static function createFromRoute($text, $route_name, $route_parameters = [], $options = []) { return new static(); }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Form/FormStateInterface.php",
        "<?php\nnamespace Drupal\\Core\\Form;\ninterface FormStateInterface {\n    public function setRedirect($route_name, array $route_parameters = [], array $options = []);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Controller/ControllerBase.php",
        "<?php\nnamespace Drupal\\Core\\Controller;\nabstract class ControllerBase {\n    protected function redirect($route_name, array $route_parameters = [], array $options = [], $status = 302) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Routing/UrlGeneratorInterface.php",
        "<?php\nnamespace Drupal\\Core\\Routing;\ninterface UrlGeneratorInterface {\n    public function generateFromRoute(string $name, array $parameters = [], array $options = [], bool $collect_bubbleable_metadata = FALSE);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Access/AccessManagerInterface.php",
        "<?php\nnamespace Drupal\\Core\\Access;\ninterface AccessManagerInterface {\n    public function checkNamedRoute($route_name, array $parameters = [], $account = NULL, $return_as_object = FALSE);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Routing/RouteProviderInterface.php",
        "<?php\nnamespace Drupal\\Core\\Routing;\ninterface RouteProviderInterface {\n    public function getRouteByName($name);\n}\n",
    ),
];

const ROUTING: &str = "acme.greeting:\n  path: '/acme/greeting/{name}'\n  defaults:\n    _controller: '\\Drupal\\acme\\Controller\\GreetingController::greet'\n  requirements:\n    _permission: 'access content'\n\nroute_callbacks:\n  - '\\Drupal\\acme\\Routing\\Routes::routes'\n";

const ROUTING_PATH: &str = "web/modules/custom/acme/acme.routing.yml";

const CONSUMER: &str = r#"<?php
namespace Drupal\acme\Controller;

use Drupal\Core\Access\AccessManagerInterface;
use Drupal\Core\Controller\ControllerBase;
use Drupal\Core\Form\FormStateInterface;
use Drupal\Core\Link;
use Drupal\Core\Routing\RouteProviderInterface;
use Drupal\Core\Routing\UrlGeneratorInterface;
use Drupal\Core\Url;

final class GreetingController extends ControllerBase {
    public function __construct(
        private UrlGeneratorInterface $urls,
        private AccessManagerInterface $access,
        private RouteProviderInterface $routes,
    ) {}

    public function run(FormStateInterface $form_state, Redirect $other): void {
        Url::fromRoute('acme.greeting');
        new Url('acme.greeting');
        Link::createFromRoute('Hello', 'acme.greeting');
        $form_state->setRedirect('acme.greeting');
        $this->redirect('acme.greeting');
        $this->urls->generateFromRoute('acme.greeting');
        $this->access->checkNamedRoute('acme.greeting');
        $this->routes->getRouteByName('acme.greeting');
        $other->setRedirect('acme.greeting');
    }
}

final class Redirect {
    public function setRedirect(string $path): void {}
}
"#;

const CONSUMER_PATH: &str = "web/modules/custom/acme/src/Controller/GreetingController.php";

fn files<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    ROUTING_STUBS
        .iter()
        .copied()
        .chain(extra.iter().copied())
        .collect()
}

#[tokio::test]
async fn a_route_name_goes_to_its_routing_file_and_lists_its_readers() {
    let (backend, dir) = workspace(&files(&[
        (ROUTING_PATH, ROUTING),
        (CONSUMER_PATH, CONSUMER),
    ]))
    .await;
    let php = uri_for(&dir, CONSUMER_PATH);
    let yaml = uri_for(&dir, ROUTING_PATH);

    let found = definitions_at(
        &backend,
        &php,
        position_in(CONSUMER, "fromRoute('acme.greeting')", 12),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((&found[0].uri, found[0].range.start.line), (&yaml, 0));

    let found = references_at(&backend, &yaml, position_in(ROUTING, "acme.greeting", 2)).await;
    // Every reader but the redirect entity's own `setRedirect()`.
    let mut expected: Vec<_> = (19..=26).map(|line| (php.clone(), line)).collect();
    expected.push((yaml, 0));
    expected.sort();
    assert_eq!(found, expected);

    let position = position_in(CONSUMER, "getRouteByName('acme.greeting')", 18);
    let hover = hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Route"), "{hover}");
}

/// Core sits in a folder the project's `.gitignore` leaves out, and its
/// test modules declare routes no real site has.
#[tokio::test]
async fn core_routes_complete_and_test_modules_do_not() {
    let consumer = CONSUMER
        .replace("Url::fromRoute('acme.greeting')", "Url::fromRoute('')")
        .replace("'Hello', 'acme.greeting'", "'Hello', ''")
        .replace("setRedirect('acme.greeting')", "setRedirect('')");
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in [
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        (
            "web/core/modules/system/system.routing.yml",
            "'<front>':\n  path: '/<front>'\nsystem.admin_content:\n  path: '/admin/content'\n",
        ),
        (
            "web/core/modules/system/tests/modules/router_test/router_test.routing.yml",
            "router_test.1:\n  path: '/router_test/test1'\n",
        ),
        (
            "web/core/modules/node/node.links.action.yml",
            "node.add_page:\n  route_name: node.add_page\n  appears_on:\n    - system.admin_content\n",
        ),
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_in(
        dir,
        &files(&[(ROUTING_PATH, ROUTING), (CONSUMER_PATH, consumer.as_str())]),
    )
    .await;
    let php = uri_for(&dir, CONSUMER_PATH);
    // Finding references indexes the workspace, core's routing files
    // included.
    references_at(
        &backend,
        &php,
        position_in(&consumer, "new Url('acme.greeting')", 10),
    )
    .await;
    let system = uri_for(&dir, "web/core/modules/system/system.routing.yml");
    let found = references_at(&backend, &system, Position::new(2, 3)).await;
    assert!(
        found.contains(&(
            uri_for(&dir, "web/core/modules/node/node.links.action.yml"),
            3
        )),
        "{found:?}"
    );

    for (needle, inside) in [
        ("fromRoute('')", 11),
        ("'Hello', ''", 10),
        ("$form_state->setRedirect('')", 26),
    ] {
        let position = position_in(&consumer, needle, inside);
        let labels = response_labels(
            completion_response(&backend, &php, position.line, position.character, None).await,
        );
        for declared in ["acme.greeting", "<front>", "system.admin_content"] {
            assert!(
                labels.contains(&declared.to_string()),
                "{needle}: {labels:?}"
            );
        }
        assert!(!labels.contains(&"router_test.1".to_string()), "{labels:?}");
        assert!(
            !labels.contains(&"route_callbacks".to_string()),
            "{labels:?}"
        );
    }
}

/// `route_name:` names a route like the positional argument does.
#[tokio::test]
async fn a_named_route_argument_is_read() {
    let consumer = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Link;
use Drupal\Core\Url;

final class Links {
    public function run(): void {
        Url::fromRoute(route_name: 'acme.greeting');
        Link::createFromRoute(text: 'Hi', route_name: 'acme.greeting');
    }
}
"#;
    let path = "web/modules/custom/acme/src/Links.php";
    let (backend, dir) = workspace(&files(&[(ROUTING_PATH, ROUTING), (path, consumer)])).await;
    let php = uri_for(&dir, path);
    let yaml = uri_for(&dir, ROUTING_PATH);
    let found = references_at(&backend, &yaml, position_in(ROUTING, "acme.greeting", 2)).await;
    assert_eq!(found, vec![(yaml, 0), (php.clone(), 8), (php, 9)]);
}

const TASKS: &str = "acme.greeting_tab:\n  route_name: acme.greeting\n  base_route: 'acme.greeting'\n  parent_id: acme.greeting\n  title: 'Greeting'\n";
const TASKS_PATH: &str = "web/modules/custom/acme/acme.links.task.yml";
const ACTIONS: &str = "acme.add:\n  route_name: \"acme.greeting\"\n  appears_on:\n    - acme.greeting\n    - 'acme.greeting' # the list\n  parent: acme.greeting\nacme.flow:\n  route_name: acme.greeting\n  appears_on: [acme.greeting, 'acme.greeting']\n";
const ACTIONS_PATH: &str = "web/modules/custom/acme/acme.links.action.yml";

/// Links files place routes by name: `route_name`, `base_route`, and the
/// items of `appears_on`, but not the menu link or task a `parent` names.
#[tokio::test]
async fn links_files_read_route_names() {
    let (backend, dir) = workspace(&files(&[
        (ROUTING_PATH, ROUTING),
        (TASKS_PATH, TASKS),
        (ACTIONS_PATH, ACTIONS),
    ]))
    .await;
    let yaml = uri_for(&dir, ROUTING_PATH);
    let tasks = uri_for(&dir, TASKS_PATH);
    let actions = uri_for(&dir, ACTIONS_PATH);

    let found = definitions_at(&backend, &tasks, position_in(TASKS, "acme.greeting\n", 3)).await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((&found[0].uri, found[0].range.start.line), (&yaml, 0));

    let found = references_at(&backend, &yaml, position_in(ROUTING, "acme.greeting", 2)).await;
    let mut expected = vec![
        (yaml, 0),
        (tasks.clone(), 1),
        (tasks, 2),
        (actions.clone(), 1),
        (actions.clone(), 3),
        (actions.clone(), 4),
        (actions.clone(), 7),
        (actions.clone(), 8),
        (actions, 8),
    ];
    expected.sort();
    assert_eq!(found, expected);
}

#[tokio::test]
async fn links_files_complete_route_names() {
    for (content, needle, inside) in [
        ("acme.tab:\n  route_name: acme.gr\n", "acme.gr\n", 7),
        ("acme.tab:\n  base_route: 'acme.\n", "'acme.", 6),
        ("acme.add:\n  appears_on:\n    - \n", "- \n", 2),
    ] {
        let (backend, dir) =
            workspace(&files(&[(ROUTING_PATH, ROUTING), (TASKS_PATH, content)])).await;
        let position = position_in(content, needle, inside);
        let labels = response_labels(
            completion_response(
                &backend,
                &uri_for(&dir, TASKS_PATH),
                position.line,
                position.character,
                None,
            )
            .await,
        );
        assert!(
            labels.contains(&"acme.greeting".to_string()),
            "{content}: {labels:?}"
        );
    }
}

const ROUTE_ATTRIBUTE: (&str, &str) = (
    "web/core/lib/Symfony/Routing/Route.php",
    "<?php\nnamespace Symfony\\Component\\Routing\\Attribute;\n#[\\Attribute]\nclass Route {\n    public function __construct(?string $path = null, ?string $name = null) {}\n}\n",
);

const ATTRIBUTE_CONTROLLER: &str = r#"<?php
namespace Drupal\acme\Controller;

use Symfony\Component\Routing\Attribute\Route;

#[Route(path: '/notes', name: 'acme.notes.')]
final class NoteController {
    #[Route(path: '/list', name: 'list')]
    public function list(): array { return []; }

    #[Route(path: '/unnamed')]
    public function unnamed(): array { return []; }
}

#[Route('/about', name: 'acme.about')]
final class AboutController {
    public function __invoke(): array { return []; }
}
"#;

const ATTRIBUTE_PATH: &str = "web/modules/custom/acme/src/Controller/NoteController.php";

const NOT_A_CONTROLLER: &str = r#"<?php
namespace Drupal\acme\Form;

use Symfony\Component\Routing\Attribute\Route;

final class Elsewhere {
    #[Route(path: '/elsewhere', name: 'acme.elsewhere')]
    public function page(): array { return []; }
}
"#;

const TEMPLATE: &str =
    "<a href=\"{{ path('acme.notes.list') }}\">{{ url('acme.about') }}</a>\n{{ path('') }}\n";
const TEMPLATE_PATH: &str = "web/modules/custom/acme/templates/acme-notes.html.twig";

/// Drupal reads `#[Route]` on the classes of an extension's `Controller`
/// namespace, and its Twig's `path()` and `url()` read route names.
#[tokio::test]
async fn route_attributes_declare_and_templates_read() {
    let (backend, dir) = workspace(&files(&[
        ROUTE_ATTRIBUTE,
        (ATTRIBUTE_PATH, ATTRIBUTE_CONTROLLER),
        (
            "web/modules/custom/acme/src/Form/Elsewhere.php",
            NOT_A_CONTROLLER,
        ),
        (TEMPLATE_PATH, TEMPLATE),
    ]))
    .await;
    let php = uri_for(&dir, ATTRIBUTE_PATH);
    let twig = uri_for(&dir, TEMPLATE_PATH);

    for (needle, line) in [("path('acme.notes.list')", 7), ("url('acme.about')", 14)] {
        let found = definitions_at(
            &backend,
            &twig,
            position_in(TEMPLATE, needle, needle.find('\'').unwrap() + 2),
        )
        .await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(
            (&found[0].uri, found[0].range.start.line),
            (&php, line),
            "{needle}"
        );
    }

    let position = position_in(TEMPLATE, "path('')", 6);
    let labels = response_labels(
        completion_response(&backend, &twig, position.line, position.character, None).await,
    );
    let mut routes: Vec<_> = labels
        .iter()
        .filter(|label| label.starts_with("acme."))
        .collect();
    routes.sort();
    assert_eq!(routes, ["acme.about", "acme.notes.list"]);
}

const BUILDER: &str = r#"<?php
namespace Drupal\acme\Routing;

use Symfony\Component\Routing\Route;

final class Routes {
    public function routes(): array {
        $routes = [];
        $routes['acme.built'] = new Route('/acme/built');
        return $routes;
    }
}
"#;

const BUILDER_PATH: &str = "web/modules/custom/acme/src/Routing/Routes.php";

/// A route a PHP route builder adds under a name written out is declared
/// there.
#[tokio::test]
async fn a_route_builder_declares_the_routes_it_names() {
    let consumer = CONSUMER.replace(
        "Url::fromRoute('acme.greeting')",
        "Url::fromRoute('acme.built')",
    );
    let (backend, dir) = workspace(&files(&[
        ("web/core/lib/Symfony/Routing/RouteClass.php", "<?php\nnamespace Symfony\\Component\\Routing;\nclass Route {\n    public function __construct(string $path) {}\n}\n"),
        (ROUTING_PATH, ROUTING),
        (BUILDER_PATH, BUILDER),
        (CONSUMER_PATH, consumer.as_str()),
    ]))
    .await;
    let found = definitions_at(
        &backend,
        &uri_for(&dir, CONSUMER_PATH),
        position_in(&consumer, "fromRoute('acme.built')", 12),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (&found[0].uri, found[0].range.start.line),
        (&uri_for(&dir, BUILDER_PATH), 8)
    );
}

const VALUES_ROUTING: &str = "acme.note_edit:\n  path: '/note/{acme_note}/edit'\n  defaults:\n    _entity_form: acme_note.edit\n    _controller: acme.greeter:greet\n    _title_callback: '\\Drupal\\acme\\Greeter::greet'\n";

/// A route's defaults name the entity type of an entity form, the service
/// of a `service:method` controller, and the method of a `Class::method`.
#[tokio::test]
async fn routing_defaults_navigate_to_entity_types_services_and_methods() {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in [
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        (
            "web/core/modules/acme_core/acme_core.routing.yml",
            VALUES_ROUTING,
        ),
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let note = "<?php\nnamespace Drupal\\acme\\Entity;\nuse Drupal\\Core\\Entity\\Attribute\\ContentEntityType;\n#[ContentEntityType(id: 'acme_note')]\nclass Note {}\n";
    let (backend, dir) = workspace_in(
        dir,
        &files(&[
            (
                "web/modules/custom/acme/acme.services.yml",
                "services:\n  acme.greeter:\n    class: Drupal\\acme\\Greeter\n",
            ),
            ("web/modules/custom/acme/src/Entity/Note.php", note),
            (
                "web/core/modules/acme_core/acme_core.routing.yml",
                VALUES_ROUTING,
            ),
        ]),
    )
    .await;
    let yaml = uri_for(&dir, "web/core/modules/acme_core/acme_core.routing.yml");
    for (needle, inside, target, line) in [
        (
            "acme_note.edit",
            2,
            "web/modules/custom/acme/src/Entity/Note.php",
            3,
        ),
        (
            "acme.greeter:greet",
            2,
            "web/modules/custom/acme/acme.services.yml",
            1,
        ),
        (
            "Greeter::greet'",
            10,
            "web/modules/custom/acme/src/Greeter.php",
            3,
        ),
    ] {
        let found =
            definitions_at(&backend, &yaml, position_in(VALUES_ROUTING, needle, inside)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(
            (&found[0].uri, found[0].range.start.line),
            (&uri_for(&dir, target), line),
            "{needle}"
        );
    }
}

const SERVICE_HANDLERS: &str = "acme.greet:\n  path: '/greet'\n  defaults:\n    _controller: 'acme.greeter:greet'\n    _title_callback: acme.titles:title\n";

const TITLES: &str = "<?php\nnamespace Drupal\\acme;\nclass Titles {\n    public function title(): string { return ''; }\n    public function subtitle(): string { return ''; }\n    protected function secret(): string { return ''; }\n    public static function make(): static { return new static(); }\n    public function __construct() {}\n}\n";

const HANDLER_SERVICES: &str = "services:\n  acme.greeter:\n    class: Drupal\\acme\\Greeter\n  acme.titles:\n    class: Drupal\\acme\\Titles\n";

/// The method of a `service:method` controller or title callback is a
/// method of the service's class: it navigates there, counts among the
/// method's references, and completes from the class's public methods.
#[tokio::test]
async fn service_handler_methods_navigate_reference_and_complete() {
    let routing_path = "web/modules/custom/acme/acme.routing.yml";
    let titles_path = "web/modules/custom/acme/src/Titles.php";
    let (backend, dir) = workspace(&files(&[
        (
            "web/modules/custom/acme/acme.services.yml",
            HANDLER_SERVICES,
        ),
        (titles_path, TITLES),
        (routing_path, SERVICE_HANDLERS),
    ]))
    .await;
    let yaml = uri_for(&dir, routing_path);
    let greeter = uri_for(&dir, "web/modules/custom/acme/src/Greeter.php");
    let titles = uri_for(&dir, titles_path);

    for (needle, inside, target) in [("greeter:greet", 10, &greeter), ("title\n", 2, &titles)] {
        let found = definitions_at(
            &backend,
            &yaml,
            position_in(SERVICE_HANDLERS, needle, inside),
        )
        .await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!((&found[0].uri, found[0].range.start.line), (target, 3));
    }
    // The service ID still goes to its declaration.
    let found = definitions_at(
        &backend,
        &yaml,
        position_in(SERVICE_HANDLERS, "acme.titles", 2),
    )
    .await;
    assert_eq!(
        found
            .iter()
            .map(|location| location.range.start.line)
            .collect::<Vec<_>>(),
        [3]
    );

    let found = references_at(&backend, &titles, position_in(TITLES, "title()", 2)).await;
    assert_eq!(found, vec![(yaml.clone(), 4), (titles.clone(), 3)]);
    let found = references_at(&backend, &yaml, position_in(SERVICE_HANDLERS, "title\n", 2)).await;
    assert_eq!(found, vec![(yaml.clone(), 4), (titles, 3)]);

    let typed = SERVICE_HANDLERS.replace("acme.titles:title", "acme.titles:t");
    for (content, needle, inside, expected) in [
        (typed.as_str(), "titles:t\n", 8, vec!["title"]),
        (
            "acme.greet:\n  path: '/greet'\n  defaults:\n    _title_callback: 'acme.titles:'\n",
            "titles:'",
            7,
            vec!["subtitle", "title"],
        ),
    ] {
        let position = position_in(content, needle, inside);
        open_document(&backend, &yaml, "yaml", content).await;
        let mut labels = response_labels(
            completion_response(&backend, &yaml, position.line, position.character, None).await,
        );
        labels.sort();
        assert_eq!(labels, expected, "{needle}");
    }
}

/// The unknown-route messages the diagnostics of `path` report.
async fn unknown_routes(files_in: &[(&str, &str)], path: &str) -> Vec<String> {
    let (backend, dir) = workspace(&files(files_in)).await;
    let content = files_in
        .iter()
        .find(|(file, _)| *file == path)
        .map(|(_, content)| *content)
        .unwrap();
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri_for(&dir, path).as_str(), content, &mut diagnostics);
    diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(tower_lsp_server::gen_lsp_types::Code::String(code))
                    if code == "invalid_drupal_route"
            )
        })
        .map(|diagnostic| crate::common::message_text(&diagnostic.message).to_string())
        .collect()
}

const CHECKED: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Url;

final class Checked {
    public function run(): void {
        Url::fromRoute('acme.greeting');
        Url::fromRoute('acme.greting');
        Url::fromRoute('acme.dyn_42');
        Url::fromRoute('acme.built');
        Url::fromRoute('Drupal\acme\Controller\AboutController');
        Url::fromRoute('Drupal\acme\Controller\NoteController::list');
        Url::fromRoute('Drupal\acme\Routing\Routes::routes');
    }
}
"#;

const DYNAMIC_BUILDER: &str = r#"<?php
namespace Drupal\acme\Routing;

use Symfony\Component\Routing\Route;

final class Routes {
    public function routes(string $id): array {
        $routes['acme.built'] = new Route('/built');
        $routes["acme.dyn_$id"] = new Route('/dyn');
        return $routes;
    }
}
"#;

/// A route name nothing declares is reported, unless it has the shape of a
/// name a route builder builds from parts.
#[tokio::test]
async fn an_undeclared_route_name_is_reported() {
    let path = "web/modules/custom/acme/src/Checked.php";
    let reported = unknown_routes(
        &[
            ROUTE_ATTRIBUTE,
            (ATTRIBUTE_PATH, ATTRIBUTE_CONTROLLER),
            (ROUTING_PATH, ROUTING),
            (BUILDER_PATH, DYNAMIC_BUILDER),
            (path, CHECKED),
        ],
        path,
    )
    .await;
    // A route builder's own method is no route: only a controller method
    // with one `#[Route]`, or a single-action controller, is aliased by its
    // class name.
    assert_eq!(
        reported,
        [
            "Unknown route 'acme.greting'",
            "Unknown route 'Drupal\\acme\\Routing\\Routes::routes'",
        ]
    );
}

const ENTITY_ROUTE_PROVIDER: &str = r#"<?php
namespace Drupal\Core\Entity\Routing;

use Symfony\Component\Routing\Route;
use Symfony\Component\Routing\RouteCollection;

class DefaultHtmlRouteProvider {
    public function getRoutes($entity_type) {
        $collection = new RouteCollection();
        $entity_type_id = $entity_type->id();
        if ($canonical_route = $this->getCanonicalRoute($entity_type)) {
            $collection->add("entity.{$entity_type_id}.canonical", $canonical_route);
        }
        if ($edit_route = $this->getEditFormRoute($entity_type)) {
            $collection->add("entity.{$entity_type_id}.edit_form", $edit_route);
        }
        return $collection;
    }

    protected function getCanonicalRoute($entity_type): ?Route {
        return new Route('/canonical');
    }

    protected function getEditFormRoute($entity_type): ?Route {
        return new Route('/edit');
    }
}
"#;

const ENTITY_AND_VIEWS: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Url;

final class EntityAndViews {
    public function run(): void {
        Url::fromRoute('entity');
        Url::fromRoute('entity.nod');
        Url::fromRoute('entity.node.canonical');
        Url::fromRoute('entity.node.edit_form');
        Url::fromRoute('entity.user.canonical');
        Url::fromRoute('view');
        Url::fromRoute('view.content');
        Url::fromRoute('view.content.page_1');
        Url::fromRoute('view.frontpage.page_1');
    }
}
"#;

/// The routes core's entity route providers build from parts are read from
/// the providers, which the `.gitignore` leaves out, and a Views route has
/// the shape `view.<view_id>.<display_id>`: a name of neither shape is
/// reported.
#[tokio::test]
async fn entity_and_views_routes_are_judged_by_their_shape() {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in [
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        (
            "web/core/lib/Drupal/Core/Entity/Routing/DefaultHtmlRouteProvider.php",
            ENTITY_ROUTE_PROVIDER,
        ),
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let path = "web/modules/custom/acme/src/EntityAndViews.php";
    let (backend, dir) = workspace_in(
        dir,
        &files(&[(ROUTING_PATH, ROUTING), (path, ENTITY_AND_VIEWS)]),
    )
    .await;
    let php = uri_for(&dir, path);
    // Finding references indexes the workspace, core's route providers
    // included.
    references_at(
        &backend,
        &php,
        position_in(ENTITY_AND_VIEWS, "'entity.nod'", 3),
    )
    .await;
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(php.as_str(), ENTITY_AND_VIEWS, &mut diagnostics);
    let reported: Vec<&str> = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(tower_lsp_server::gen_lsp_types::Code::String(code))
                    if code == "invalid_drupal_route"
            )
        })
        .map(|diagnostic| crate::common::message_text(&diagnostic.message))
        .collect();
    assert_eq!(
        reported,
        [
            "Unknown route 'entity'",
            "Unknown route 'entity.nod'",
            "Unknown route 'view'",
            "Unknown route 'view.content'",
        ]
    );
}

/// Test code passes invented route names to mocks, `*.api.php` examples
/// name invented routes, and a project whose routing files were not read
/// has nothing to judge against.
#[tokio::test]
async fn test_code_and_unread_projects_are_not_judged() {
    let test_path = "web/modules/custom/acme/tests/src/Unit/CheckedTest.php";
    assert!(
        unknown_routes(&[(ROUTING_PATH, ROUTING), (test_path, CHECKED)], test_path)
            .await
            .is_empty()
    );
    let path = "web/modules/custom/acme/src/Checked.php";
    assert!(unknown_routes(&[(path, CHECKED)], path).await.is_empty());
    let api_path = "web/modules/custom/acme/acme.api.php";
    assert!(
        unknown_routes(&[(ROUTING_PATH, ROUTING), (api_path, CHECKED)], api_path)
            .await
            .is_empty()
    );
}

const PARAMETERS: &str = r#"<?php
namespace Drupal\acme\Controller;

use Drupal\Core\Access\AccessManagerInterface;
use Drupal\Core\Controller\ControllerBase;
use Drupal\Core\Form\FormStateInterface;
use Drupal\Core\Link;
use Drupal\Core\Routing\UrlGeneratorInterface;
use Drupal\Core\Url;

final class Parameters extends ControllerBase {
    public function __construct(
        private UrlGeneratorInterface $urls,
        private AccessManagerInterface $access,
    ) {}

    public function run(FormStateInterface $form_state, Redirect $other): void {
        Url::fromRoute('acme.greeting', ['name' => 'World']);
        new Url('acme.greeting', ['name' => 'World']);
        Link::createFromRoute('Hello', 'acme.greeting', ['name' => 'World']);
        $form_state->setRedirect('acme.greeting', ['name' => 'World']);
        $this->redirect('acme.greeting', ['name' => 'World']);
        $this->urls->generateFromRoute('acme.greeting', ['name' => 'World']);
        $this->access->checkNamedRoute('acme.greeting', ['name' => 'World']);
        $other->setRedirect('acme.greeting', ['name' => 'World']);
    }
}

final class Redirect {
    public function setRedirect(string $path, array $query = []): void {}
}
"#;

const PARAMETERS_PATH: &str = "web/modules/custom/acme/src/Controller/Parameters.php";

const MENU: &str = "acme.greeting:\n  title: 'Greeting'\n  route_parameters:\n    name: 'World'\n  route_name: acme.greeting\n";
const MENU_PATH: &str = "web/modules/custom/acme/acme.links.menu.yml";

/// A key of the parameters a route call or a links file passes goes to
/// the placeholder of the route's path, which lists every one of them.
#[tokio::test]
async fn route_parameters_go_to_the_path_that_declares_them() {
    let (backend, dir) = workspace(&files(&[
        (ROUTING_PATH, ROUTING),
        (PARAMETERS_PATH, PARAMETERS),
        (MENU_PATH, MENU),
    ]))
    .await;
    let php = uri_for(&dir, PARAMETERS_PATH);
    let yaml = uri_for(&dir, ROUTING_PATH);
    let menu = uri_for(&dir, MENU_PATH);

    let found = definitions_at(
        &backend,
        &php,
        position_in(PARAMETERS, "fromRoute('acme.greeting', ['name'", 30),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (&found[0].uri, found[0].range.start),
        (&yaml, Position::new(1, 25))
    );
    let found = definitions_at(&backend, &menu, Position::new(3, 5)).await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((&found[0].uri, found[0].range.start.line), (&yaml, 1));

    let found = references_at(&backend, &yaml, Position::new(1, 26)).await;
    // Every call but the other class's `setRedirect()`.
    let mut expected: Vec<_> = (17..=23).map(|line| (php.clone(), line)).collect();
    expected.push((yaml, 1));
    expected.push((menu, 3));
    expected.sort();
    assert_eq!(found, expected);

    let position = position_in(PARAMETERS, "fromRoute('acme.greeting', ['name'", 30);
    let hover = hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Route parameter"), "{hover}");
    assert!(hover.contains("/acme/greeting/{name}"), "{hover}");
}

/// The keys of the parameters complete from the route's path, in every
/// call that takes a route name and its parameters.
#[tokio::test]
async fn route_parameters_complete_from_the_path() {
    let consumer = PARAMETERS.replace("['name' => 'World']", "['' => 'World']");
    let (backend, dir) = workspace(&files(&[
        (ROUTING_PATH, ROUTING),
        (PARAMETERS_PATH, consumer.as_str()),
    ]))
    .await;
    let php = uri_for(&dir, PARAMETERS_PATH);
    for (needle, inside) in [
        ("fromRoute('acme.greeting', [''", 29),
        ("new Url('acme.greeting', [''", 27),
        ("'Hello', 'acme.greeting', [''", 28),
        ("$form_state->setRedirect('acme.greeting', [''", 44),
        ("$this->redirect('acme.greeting', [''", 35),
        ("generateFromRoute('acme.greeting', [''", 37),
        ("checkNamedRoute('acme.greeting', [''", 35),
    ] {
        let position = position_in(&consumer, needle, inside);
        let labels = response_labels(
            completion_response(&backend, &php, position.line, position.character, None).await,
        );
        assert_eq!(labels, ["name"], "{needle}");
    }
    let position = position_in(&consumer, "$other->setRedirect('acme.greeting', [''", 39);
    let labels = response_labels(
        completion_response(&backend, &php, position.line, position.character, None).await,
    );
    assert!(!labels.contains(&"name".to_string()), "{labels:?}");
}

/// Drupal sends a parameter no path declares as the query string, so no
/// key is reported.
#[tokio::test]
async fn route_parameters_no_path_declares_are_not_reported() {
    let consumer = PARAMETERS.replace("['name' => 'World']", "['page' => 2]");
    let (backend, dir) = workspace(&files(&[
        (ROUTING_PATH, ROUTING),
        (PARAMETERS_PATH, consumer.as_str()),
    ]))
    .await;
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(
        uri_for(&dir, PARAMETERS_PATH).as_str(),
        &consumer,
        &mut diagnostics,
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !crate::common::message_text(&diagnostic.message).contains("page")),
        "{diagnostics:?}"
    );
}

const NOTE: &str = r#"<?php
namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityBase;

#[ContentEntityType(id: 'acme_note')]
class Note extends ContentEntityBase {}
"#;
const NOTE_PATH: &str = "web/modules/custom/acme/src/Entity/Note.php";
const ENTITY_INTERFACE: &str =
    "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityInterface {}\n";
const ENTITY_INTERFACE_PATH: &str = "web/core/lib/Drupal/Core/Entity/EntityInterface.php";
// As in core, a content entity is `\Traversable`, which PHP's stubs declare
// `iterable`.
const CONTENT_ENTITY_INTERFACE: &str = "<?php\nnamespace Drupal\\Core\\Entity;\ninterface ContentEntityInterface extends \\Traversable, EntityInterface {}\n";
const CONTENT_ENTITY_INTERFACE_PATH: &str =
    "web/core/lib/Drupal/Core/Entity/ContentEntityInterface.php";
/// PHP's own iteration interfaces, as its stubs declare them.
const PHP_ITERATION: &str = "<?php\ninterface Traversable extends iterable {}\ninterface IteratorAggregate extends Traversable {\n    public function getIterator(): Iterator;\n}\n";
const PHP_ITERATION_PATH: &str = "stubs/Traversable.php";
const CONTENT_ENTITY_BASE: &str = "<?php\nnamespace Drupal\\Core\\Entity;\nabstract class ContentEntityBase implements \\IteratorAggregate, ContentEntityInterface {\n    public function getIterator(): \\ArrayIterator { return new \\ArrayIterator([]); }\n}\n";
const CONTENT_ENTITY_BASE_PATH: &str = "web/core/lib/Drupal/Core/Entity/ContentEntityBase.php";
const USER_INTERFACE: &str = "<?php\nnamespace Drupal\\user;\ninterface UserInterface extends \\Drupal\\Core\\Entity\\ContentEntityInterface {}\n";
const USER_INTERFACE_PATH: &str = "web/core/modules/user/src/UserInterface.php";

const NOTE_ROUTING: &str = r#"acme.note:
  path: '/note/{acme_note}'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::view'
    _title_callback: '\Drupal\acme\Controller\PageController::title'
acme.note_edit:
  path: '/note/{note}/edit'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::edit'
  options:
    parameters:
      note:
        type: entity:acme_note
acme.note_raw:
  path: '/note/{acme_note}/raw'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::raw'
acme.note_page:
  path: '/note/{acme_note}/page'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::page'
    _entity_view: 'acme_note.full'
acme.note_blocked:
  path: '/note/{acme_note}/blocked'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::blocked'
acme.note_form:
  path: '/note/{acme_note}/form'
  defaults:
    _form: '\Drupal\acme\Form\NoteForm'
acme.note_delete:
  path: '/note/{acme_note}/delete'
  defaults:
    _entity_form: 'acme_note.delete'
    _title_callback: '\Drupal\acme\Controller\PageController::deleteTitle'
acme.note_tempstore:
  path: '/note/{note}/draft'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::draft'
  options:
    parameters:
      note:
        type: entity:acme_note
        tempstore: TRUE
acme.note_revision:
  path: '/note/{acme_note}/revisions/{acme_note_revision}'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::revision'
    _title_callback: '\Drupal\acme\Controller\PageController::revisionTitle'
    _entity_view: 'acme_note.full'
  options:
    parameters:
      acme_note_revision:
        type: entity_revision:acme_note
acme.note_latest:
  path: '/note/latest'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::latest'
    note: 1
  options:
    parameters:
      note:
        type: entity:acme_note
acme.note_missing:
  path: '/note/missing'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::missing'
  options:
    parameters:
      note:
        type: entity:acme_note
acme.note_optional:
  path: '/note/optional/{acme_note}'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::optional'
    acme_note: ~
acme.note_shared_a:
  path: '/note/a/{note}'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::shared'
  options:
    parameters:
      note:
        type: entity:acme_note
acme.note_shared_b:
  path: '/note/b/{note}'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::shared'
acme.note_content:
  path: '/note/{acme_note}/content'
  defaults:
    _controller: '\Drupal\acme\Controller\PageController::content'
"#;

const PAGE_CONTROLLER: &str = r#"<?php
namespace Drupal\acme\Controller;

use Drupal\Core\Entity\ContentEntityInterface;
use Drupal\Core\Entity\EntityInterface;
use Drupal\user\UserInterface;

final class PageController {
    public function view(EntityInterface $acme_note): array {
        $acme_note; // view
        return [];
    }

    public function title($acme_note): string {
        $acme_note; // title
        return '';
    }

    public function edit($note): array {
        $note; // edit
        return [];
    }

    public function raw($acme_note): array {
        $acme_note; // raw
        return [];
    }

    public function page($acme_note): array {
        $acme_note; // page
        return [];
    }

    public function blocked(UserInterface $acme_note, string $mode = 'full'): array {
        $acme_note; // blocked
        return [];
    }

    public function unrouted(EntityInterface $acme_note): array {
        $acme_note; // unrouted
        return [];
    }

    public function deleteTitle($acme_note): string {
        $acme_note; // deleteTitle
        return '';
    }

    public function draft($note): array {
        $note; // draft
        return [];
    }

    public function revision($acme_note_revision): array {
        return [];
    }

    public function revisionTitle($acme_note, $acme_note_revision): string {
        $acme_note; // revisionTitle
        $acme_note_revision; // revisionTitle revision
        return '';
    }

    public function latest($note): array {
        $note; // latest
        return [];
    }

    public function missing($note): array {
        $note; // missing
        return [];
    }

    public function optional(EntityInterface $acme_note): array {
        $acme_note; // optional
        return [];
    }

    public function shared($note): array {
        $note; // shared
        return [];
    }

    public function content(ContentEntityInterface $acme_note): array {
        $acme_note; // content
        return [];
    }
}
"#;
const PAGE_CONTROLLER_PATH: &str = "web/modules/custom/acme/src/Controller/PageController.php";

const NOTE_FORM: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Entity\EntityInterface;

final class NoteForm {
    public function buildForm(array $form, $form_state, ?EntityInterface $acme_note = NULL): array {
        $acme_note; // buildForm
        return $form;
    }
}
"#;
const NOTE_FORM_PATH: &str = "web/modules/custom/acme/src/Form/NoteForm.php";

const ATTRIBUTE_NOTE_CONTROLLER: &str = r#"<?php
namespace Drupal\acme\Controller;

use Drupal\Core\Entity\EntityInterface;
use Symfony\Component\Routing\Attribute\Route;

final class AttributeNoteController {
    #[Route(path: '/attribute/{acme_note}', name: 'acme.attribute_note')]
    public function show(EntityInterface $acme_note): array {
        $acme_note; // attribute
        return [];
    }
}
"#;
const ATTRIBUTE_NOTE_CONTROLLER_PATH: &str =
    "web/modules/custom/acme/src/Controller/AttributeNoteController.php";

fn note_files() -> Vec<(&'static str, &'static str)> {
    files(&[
        (ENTITY_INTERFACE_PATH, ENTITY_INTERFACE),
        (CONTENT_ENTITY_INTERFACE_PATH, CONTENT_ENTITY_INTERFACE),
        (CONTENT_ENTITY_BASE_PATH, CONTENT_ENTITY_BASE),
        (PHP_ITERATION_PATH, PHP_ITERATION),
        (USER_INTERFACE_PATH, USER_INTERFACE),
        (NOTE_PATH, NOTE),
        ("web/modules/custom/acme/acme.routing.yml", NOTE_ROUTING),
        (PAGE_CONTROLLER_PATH, PAGE_CONTROLLER),
        (NOTE_FORM_PATH, NOTE_FORM),
        ROUTE_ATTRIBUTE,
        (ATTRIBUTE_NOTE_CONTROLLER_PATH, ATTRIBUTE_NOTE_CONTROLLER),
    ])
}

/// What hovering the variable just before `marker` in `content` shows.
async fn hover_before(
    backend: &phpantom_lsp::Backend,
    uri: &tower_lsp_server::gen_lsp_types::Uri,
    content: &str,
    marker: &str,
) -> String {
    let position = position_in(content, marker, 2);
    hover_text_at(backend, uri, position.line, position.character)
        .await
        .unwrap_or_default()
}

/// A controller's parameter is the entity Drupal loads for it only where
/// Drupal upcasts it: a type the route declares, a hint the entity class
/// fits, or the entity type an `_entity_view` or `_entity_form` names, for
/// a parameter the path or a default gives a value, and its title callback
/// receives the same.  A parameter another converter takes, one nothing
/// upcasts, and one a route reaching the method leaves alone keep their
/// declared type.
#[tokio::test]
async fn controller_parameters_are_the_entities_drupal_upcasts() {
    let (backend, dir) = workspace(&note_files()).await;
    let php = uri_for(&dir, PAGE_CONTROLLER_PATH);
    for (marker, expected) in [
        ("$acme_note; // view", "$acme_note = Note\n"),
        ("$acme_note; // title", "$acme_note = Note\n"),
        ("$note; // edit", "$note = Note\n"),
        ("$acme_note; // page", "$acme_note = Note\n"),
        ("$acme_note; // deleteTitle", "$acme_note = Note\n"),
        ("$acme_note; // revisionTitle", "$acme_note = Note\n"),
        (
            "$acme_note_revision; // revisionTitle revision",
            "$acme_note_revision = Note\n",
        ),
        ("$note; // latest", "$note = Note\n"),
        ("$acme_note; // content", "$acme_note = Note\n"),
        // A hint the entity class does not fit blocks upcasting, and a
        // method no route reaches keeps its hint.
        ("$acme_note; // blocked", "$acme_note = UserInterface\n"),
        ("$acme_note; // unrouted", "$acme_note = EntityInterface\n"),
    ] {
        let text = hover_before(&backend, &php, PAGE_CONTROLLER, marker).await;
        assert!(text.contains(expected), "{marker}: {text}");
    }
    // A route's `NULL` default leaves the parameter `NULL` when the path
    // gives it no value.
    let text = hover_before(&backend, &php, PAGE_CONTROLLER, "$acme_note; // optional").await;
    assert!(
        text.contains("$acme_note = ?Note\n") || text.contains("$acme_note = Note|null\n"),
        "{text}"
    );
    // Untyped and left alone: by nothing, by a tempstore's own converter,
    // by a route that gives it no value, or by one of the routes that
    // reach the method.
    for (marker, variable) in [
        ("$acme_note; // raw", "$acme_note"),
        ("$note; // draft", "$note"),
        ("$note; // missing", "$note"),
        ("$note; // shared", "$note"),
    ] {
        let text = hover_before(&backend, &php, PAGE_CONTROLLER, marker).await;
        assert!(
            text.contains(&format!("<?php\n{variable}\n```")),
            "{marker}: {text}"
        );
    }

    // A `_form` route upcasts the form's `buildForm()` parameters, which
    // default to `NULL`.
    let form = uri_for(&dir, NOTE_FORM_PATH);
    let text = hover_before(&backend, &form, NOTE_FORM, "$acme_note; // buildForm").await;
    assert!(
        text.contains("$acme_note = ?Note\n") || text.contains("$acme_note = Note|null\n"),
        "{text}"
    );
    // A `#[Route]` method is upcast like a routing file's controller.
    let attribute = uri_for(&dir, ATTRIBUTE_NOTE_CONTROLLER_PATH);
    let text = hover_before(
        &backend,
        &attribute,
        ATTRIBUTE_NOTE_CONTROLLER,
        "$acme_note; // attribute",
    )
    .await;
    assert!(text.contains("$acme_note = Note\n"), "{text}");
}

const TITLE_ROUTING: &str = r#"acme_extra.converted:
  path: '/extra/{acme_note}/converted'
  defaults:
    _controller: '\Drupal\acme_extra\Controller\TitleController::converted'
  options:
    parameters:
      acme_note:
        converter: acme_extra.note_converter
acme_extra.titled:
  path: '/extra/{acme_note}/titled'
  defaults:
    _controller: '\Drupal\acme_extra\Controller\TitleController::titledView'
    _title_callback: '\Drupal\acme_extra\Controller\TitleController::titledTitle'
acme_extra.declared:
  path: '/extra/{note}/declared'
  defaults:
    _controller: '\Drupal\acme_extra\Controller\TitleController::declaredView'
    _title_callback: '\Drupal\acme_extra\Controller\TitleController::declaredTitle'
  options:
    parameters:
      note:
        type: entity:acme_note
"#;
const TITLE_ROUTING_PATH: &str = "web/modules/custom/acme_extra/acme_extra.routing.yml";

const TITLE_CONTROLLER: &str = r#"<?php
namespace Drupal\acme_extra\Controller;

use Drupal\Core\Entity\EntityInterface;
use Drupal\user\UserInterface;

final class NoteDto {}

final class TitleController {
    public function converted(NoteDto $acme_note): array { return []; }

    public function titledView($acme_note): array { return []; }

    public function titledTitle(EntityInterface $acme_note): string { return ''; }

    public function declaredView($note): array { return []; }

    public function declaredTitle(UserInterface $note): string { return ''; }
}
"#;
const TITLE_CONTROLLER_PATH: &str =
    "web/modules/custom/acme_extra/src/Controller/TitleController.php";

/// The positions of the `invalid_drupal_route_parameter` warnings the
/// diagnostics of `content` at `path` report.
async fn blocked_upcasts(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    path: &str,
    content: &str,
) -> Vec<Position> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri_for(dir, path).as_str(), content, &mut diagnostics);
    diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(tower_lsp_server::gen_lsp_types::Code::String(code))
                    if code == "invalid_drupal_route_parameter"
            )
        })
        .map(|diagnostic| diagnostic.range.start)
        .collect()
}

/// A hint the entity class does not fit leaves Drupal passing the raw ID,
/// which is reported; an untyped parameter, one no route declares, and a
/// method no route reaches are not.
#[tokio::test]
async fn a_hint_that_blocks_upcasting_is_reported() {
    let (backend, dir) = workspace(&note_files()).await;
    assert_eq!(
        blocked_upcasts(&backend, &dir, PAGE_CONTROLLER_PATH, PAGE_CONTROLLER).await,
        [position_in(PAGE_CONTROLLER, "UserInterface $acme_note", 14)]
    );
}

/// A parameter with its own converter, and a title callback, are not
/// judged.
#[tokio::test]
async fn converters_and_title_callbacks_are_not_judged() {
    let mut files = note_files();
    files.extend([
        (TITLE_ROUTING_PATH, TITLE_ROUTING),
        (TITLE_CONTROLLER_PATH, TITLE_CONTROLLER),
    ]);
    let (backend, dir) = workspace(&files).await;
    assert_eq!(
        blocked_upcasts(&backend, &dir, TITLE_CONTROLLER_PATH, TITLE_CONTROLLER).await,
        Vec::<Position>::new()
    );
}

const MEMO: &str = "<?php\nnamespace Drupal\\acme_check\\Entity;\n\nuse Drupal\\acme\\Entity\\Note;\n\nfinal class Memo extends Note {}\n";
const MEMO_PATH: &str = "web/modules/custom/acme_check/src/Entity/Memo.php";
const BROKEN: &str =
    "<?php\nnamespace Drupal\\acme_check\\Entity;\n\ninterface Broken extends Gone {}\n";
const BROKEN_PATH: &str = "web/modules/custom/acme_check/src/Entity/Broken.php";
const CHECK_ROUTING_PATH: &str = "web/modules/custom/acme_check/acme_check.routing.yml";
const CHECK_CONTROLLER_PATH: &str =
    "web/modules/custom/acme_check/src/Controller/CheckController.php";

/// `CheckController` in the `acme_check` module, with `methods`.
fn check_controller(methods: &str) -> String {
    format!(
        "<?php\nnamespace Drupal\\acme_check\\Controller;\n\nuse Drupal\\acme\\Entity\\Note;\nuse Drupal\\acme_check\\Entity\\Broken;\nuse Drupal\\acme_check\\Entity\\Memo;\nuse Drupal\\Core\\Entity\\EntityInterface;\nuse Drupal\\user\\UserInterface;\nuse Symfony\\Component\\Routing\\Attribute\\Route;\n\nfinal class CheckController {{\n{methods}}}\n"
    )
}

/// A workspace with the `acme_note` entity type, the `acme_check` module's
/// `routing` file, and `php` at `path`.
async fn check_workspace(
    routing: &str,
    path: &str,
    php: &str,
) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    workspace(&files(&[
        (ENTITY_INTERFACE_PATH, ENTITY_INTERFACE),
        (CONTENT_ENTITY_INTERFACE_PATH, CONTENT_ENTITY_INTERFACE),
        (CONTENT_ENTITY_BASE_PATH, CONTENT_ENTITY_BASE),
        (PHP_ITERATION_PATH, PHP_ITERATION),
        (USER_INTERFACE_PATH, USER_INTERFACE),
        (NOTE_PATH, NOTE),
        (MEMO_PATH, MEMO),
        (BROKEN_PATH, BROKEN),
        ROUTE_ATTRIBUTE,
        (CHECK_ROUTING_PATH, routing),
        (path, php),
    ]))
    .await
}

/// The warnings the diagnostics of `CheckController`, holding `methods`,
/// report with `routing` declaring its routes.
async fn check_warnings(routing: &str, methods: &str) -> (String, Vec<Position>) {
    let php = check_controller(methods);
    let (backend, dir) = check_workspace(routing, CHECK_CONTROLLER_PATH, &php).await;
    let found = blocked_upcasts(&backend, &dir, CHECK_CONTROLLER_PATH, &php).await;
    (php, found)
}

/// What hovering `$acme_note` before `marker` in `CheckController`,
/// holding `methods`, shows with `routing` declaring its routes.
async fn check_hover(routing: &str, methods: &str, marker: &str) -> String {
    let php = check_controller(methods);
    let (backend, dir) = check_workspace(routing, CHECK_CONTROLLER_PATH, &php).await;
    hover_before(
        &backend,
        &uri_for(&dir, CHECK_CONTROLLER_PATH),
        &php,
        marker,
    )
    .await
}

/// Whether `hover` shows `$acme_note` as a `Note` or `null`.
fn is_nullable_note(hover: &str) -> bool {
    hover.contains("$acme_note = ?Note\n") || hover.contains("$acme_note = Note|null\n")
}

/// A `Class::method` controller hinting an entity-type-named parameter
/// with an interface unrelated to the entity class gets the raw ID.
#[tokio::test]
async fn a_controller_hint_unrelated_to_the_entity_is_reported() {
    let routing = "acme_check.view:\n  path: '/check/{acme_note}'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::view'\n";
    let (php, found) = check_warnings(
        routing,
        "    public function view(UserInterface $acme_note): array { return []; }\n",
    )
    .await;
    assert_eq!(found, [position_in(&php, "UserInterface $acme_note", 14)]);
}

/// A `_form` class's `buildForm()` is reflected like a controller.
#[tokio::test]
async fn a_form_hint_unrelated_to_the_entity_is_reported() {
    let routing = "acme_check.form:\n  path: '/check/{acme_note}/form'\n  defaults:\n    _form: '\\Drupal\\acme_check\\Form\\CheckForm'\n";
    let path = "web/modules/custom/acme_check/src/Form/CheckForm.php";
    let php = "<?php\nnamespace Drupal\\acme_check\\Form;\n\nuse Drupal\\user\\UserInterface;\n\nfinal class CheckForm {\n    public function buildForm(array $form, $form_state, UserInterface $acme_note): array { return $form; }\n}\n";
    let (backend, dir) = check_workspace(routing, path, php).await;
    assert_eq!(
        blocked_upcasts(&backend, &dir, path, php).await,
        [position_in(php, "UserInterface $acme_note", 14)]
    );
}

/// A `#[Route]` method is reflected like a controller.
#[tokio::test]
async fn a_route_attribute_hint_unrelated_to_the_entity_is_reported() {
    let (php, found) = check_warnings(
        "",
        "    #[Route(path: '/check/{acme_note}/attribute', name: 'acme_check.attribute')]\n    public function attribute(UserInterface $acme_note): array { return []; }\n",
    )
    .await;
    assert_eq!(found, [position_in(&php, "UserInterface $acme_note", 14)]);
}

/// A declared `entity:` type converts the parameter as declared, a bundle
/// class hint included.
#[tokio::test]
async fn a_declared_parameter_with_a_bundle_class_hint_is_not_judged() {
    let routing = "acme_check.bundle:\n  path: '/check/{acme_note}/bundle'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::bundle'\n  options:\n    parameters:\n      acme_note:\n        type: entity:acme_note\n";
    let (_, found) = check_warnings(
        routing,
        "    public function bundle(Memo $acme_note): array { return []; }\n",
    )
    .await;
    assert_eq!(found, Vec::<Position>::new());
}

/// A declared `entity:` type converts the parameter as declared, whatever
/// its hint.
#[tokio::test]
async fn a_declared_parameter_with_an_unrelated_hint_is_not_judged() {
    let routing = "acme_check.unrelated:\n  path: '/check/{acme_note}/unrelated'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::unrelated'\n  options:\n    parameters:\n      acme_note:\n        type: entity:acme_note\n";
    let (_, found) = check_warnings(
        routing,
        "    public function unrelated(UserInterface $acme_note): array { return []; }\n",
    )
    .await;
    assert_eq!(found, Vec::<Position>::new());
}

/// A bundle class hint is not reported, since an alter hook can make it
/// the entity class.
#[tokio::test]
async fn a_bundle_class_hint_is_not_reported() {
    let routing = "acme_check.bundle:\n  path: '/check/{acme_note}/bundle'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::bundle'\n";
    let (_, found) = check_warnings(
        routing,
        "    public function bundle(Memo $acme_note): array { return []; }\n",
    )
    .await;
    assert_eq!(found, Vec::<Position>::new());
}

/// A docblock that names a bundle class of the entity says more than the
/// entity class Drupal's upcasting gives, and is kept.
#[tokio::test]
async fn a_bundle_class_docblock_wins_over_the_upcast_entity() {
    let routing = "acme_check.view:\n  path: '/check/{acme_note}'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::view'\n    _title_callback: '\\Drupal\\acme_check\\Controller\\CheckController::title'\n";
    let methods = "    /** @param Memo $acme_note */\n    public function view(EntityInterface $acme_note): array {\n        $acme_note; // view\n        return [];\n    }\n    /** @param Memo $acme_note */\n    public function title($acme_note): string {\n        $acme_note; // title\n        return '';\n    }\n";
    for marker in ["$acme_note; // view", "$acme_note; // title"] {
        let text = check_hover(routing, methods, marker).await;
        assert!(text.contains("$acme_note = Memo\n"), "{marker}: {text}");
    }
}

/// A title callback is never reported, nor reflected by core.
#[tokio::test]
async fn a_title_callback_is_not_reported() {
    let routing = "acme_check.titled:\n  path: '/check/{acme_note}/titled'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::view'\n    _title_callback: '\\Drupal\\acme_check\\Controller\\CheckController::title'\n";
    let (_, found) = check_warnings(
        routing,
        "    public function view($acme_note): array { return []; }\n    public function title(UserInterface $acme_note): string { return ''; }\n",
    )
    .await;
    assert_eq!(found, Vec::<Position>::new());
}

/// A title callback on a route whose controller is a service's method is
/// not reported either.
#[tokio::test]
async fn a_title_callback_beside_a_service_controller_is_not_reported() {
    let routing = "acme_check.service:\n  path: '/check/{acme_note}/service'\n  defaults:\n    _controller: 'acme_check.pages:show'\n    _title_callback: '\\Drupal\\acme_check\\Controller\\CheckController::title'\n";
    let (_, found) = check_warnings(
        routing,
        "    public function title(UserInterface $acme_note): string { return ''; }\n",
    )
    .await;
    assert_eq!(found, Vec::<Position>::new());
}

/// A function controller is not judged.
#[tokio::test]
async fn a_function_controller_is_not_judged() {
    let routing = "acme_check.function:\n  path: '/check/{acme_note}/function'\n  defaults:\n    _controller: 'acme_check_page'\n";
    let path = "web/modules/custom/acme_check/acme_check.module";
    let php = "<?php\n\nuse Drupal\\user\\UserInterface;\n\nfunction acme_check_page(UserInterface $acme_note): array { return []; }\n";
    let (backend, dir) = check_workspace(routing, path, php).await;
    assert_eq!(
        blocked_upcasts(&backend, &dir, path, php).await,
        Vec::<Position>::new()
    );
}

/// A route whose controller class is not found gives its title callback
/// no verdict.
#[tokio::test]
async fn a_route_whose_controller_is_not_found_is_not_judged() {
    let routing = "acme_check.missing:\n  path: '/check/{acme_note}/missing'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\Missing::view'\n    _title_callback: '\\Drupal\\acme_check\\Controller\\CheckController::title'\n";
    let (_, found) = check_warnings(
        routing,
        "    public function title(UserInterface $acme_note): string { return ''; }\n",
    )
    .await;
    assert_eq!(found, Vec::<Position>::new());
}

/// A hint whose ancestors are not all found is not judged.
#[tokio::test]
async fn a_hint_with_missing_ancestors_is_not_judged() {
    let routing = "acme_check.broken:\n  path: '/check/{acme_note}/broken'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::broken'\n";
    let (_, found) = check_warnings(
        routing,
        "    public function broken(Broken $acme_note): array { return []; }\n",
    )
    .await;
    assert_eq!(found, Vec::<Position>::new());
}

/// A `NULL` default the path does not override is passed as it is.
#[tokio::test]
async fn a_null_default_outside_the_path_is_not_judged() {
    let routing = "acme_check.null:\n  path: '/check/null'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::view'\n    acme_note: NULL\n";
    let (_, found) = check_warnings(
        routing,
        "    public function view(UserInterface $acme_note): array { return []; }\n",
    )
    .await;
    assert_eq!(found, Vec::<Position>::new());
}

const CHECK_VIEW_ROUTING: &str = "acme_check.view:\n  path: '/check/{acme_note}'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::view'\n";

/// A nullable hint keeps `null` in the upcast type.
#[tokio::test]
async fn an_upcast_parameter_with_a_nullable_hint_may_be_null() {
    let text = check_hover(
        CHECK_VIEW_ROUTING,
        "    public function view(?Note $acme_note): array {\n        $acme_note; // view\n        return [];\n    }\n",
        "$acme_note; // view",
    )
    .await;
    assert!(is_nullable_note(&text), "{text}");
}

/// A parameter defaulting to `NULL` keeps `null` in the upcast type.
#[tokio::test]
async fn an_upcast_parameter_defaulting_to_null_may_be_null() {
    let text = check_hover(
        CHECK_VIEW_ROUTING,
        "    public function view(Note $acme_note = NULL): array {\n        $acme_note; // view\n        return [];\n    }\n",
        "$acme_note; // view",
    )
    .await;
    assert!(is_nullable_note(&text), "{text}");
}

/// A routing file's `NULL` default for a path parameter keeps `null` in
/// the upcast type.
#[tokio::test]
async fn an_upcast_parameter_a_route_defaults_to_null_may_be_null() {
    let routing = "acme_check.view:\n  path: '/check/{acme_note}'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::view'\n    acme_note: NULL\n";
    let text = check_hover(
        routing,
        "    public function view(EntityInterface $acme_note): array {\n        $acme_note; // view\n        return [];\n    }\n",
        "$acme_note; // view",
    )
    .await;
    assert!(is_nullable_note(&text), "{text}");
}

/// A `#[Route]`'s `NULL` default keeps `null` in the upcast type.
#[tokio::test]
async fn an_upcast_parameter_a_route_attribute_defaults_to_null_may_be_null() {
    let text = check_hover(
        "",
        "    #[Route(path: '/check/{acme_note}', name: 'acme_check.view', defaults: ['acme_note' => NULL])]\n    public function view(EntityInterface $acme_note): array {\n        $acme_note; // view\n        return [];\n    }\n",
        "$acme_note; // view",
    )
    .await;
    assert!(is_nullable_note(&text), "{text}");
}

/// A quoted `'~'`, `'null'`, `"null"` or `''` default is a string, not
/// `NULL`.
#[tokio::test]
async fn a_quoted_null_default_is_a_string() {
    for value in ["'~'", "'null'", "\"null\"", "''"] {
        let routing = format!(
            "acme_check.view:\n  path: '/check/{{acme_note}}'\n  defaults:\n    _controller: '\\Drupal\\acme_check\\Controller\\CheckController::view'\n    acme_note: {value}\n"
        );
        let text = check_hover(
            &routing,
            "    public function view(EntityInterface $acme_note): array {\n        $acme_note; // view\n        return [];\n    }\n",
            "$acme_note; // view",
        )
        .await;
        assert!(text.contains("$acme_note = Note\n"), "{value}: {text}");
    }
}

const BOOK_CONTROLLER: &str = r#"<?php
namespace Drupal\acme\Controller;

use Symfony\Component\Routing\Attribute\Route;

#[Route(path: '/books/{shelf}', name: 'acme.books.')]
final class BookController {
    #[Route(path: '/{book}', name: 'view')]
    public function view(string $shelf, string $book): array { return []; }
}
"#;
const BOOK_CONTROLLER_PATH: &str = "web/modules/custom/acme/src/Controller/BookController.php";
const BOOK_TEMPLATE: &str = "{{ path('acme.books.view', {shelf: 'a', 'book': 1}) }}\n";
const BOOK_TEMPLATE_PATH: &str = "web/modules/custom/acme/templates/acme-books.html.twig";

/// A `#[Route]`'s path declares its parameters, the class route's
/// included, and a template's `path()` passes them in its hash.
#[tokio::test]
async fn attribute_route_parameters_are_read_by_templates() {
    let (backend, dir) = workspace(&files(&[
        ROUTE_ATTRIBUTE,
        (BOOK_CONTROLLER_PATH, BOOK_CONTROLLER),
        (BOOK_TEMPLATE_PATH, BOOK_TEMPLATE),
    ]))
    .await;
    let php = uri_for(&dir, BOOK_CONTROLLER_PATH);
    let twig = uri_for(&dir, BOOK_TEMPLATE_PATH);
    for (needle, line) in [("shelf: ", 5), ("'book'", 7)] {
        let found = definitions_at(&backend, &twig, position_in(BOOK_TEMPLATE, needle, 2)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(
            (&found[0].uri, found[0].range.start.line),
            (&php, line),
            "{needle}"
        );
    }
}
