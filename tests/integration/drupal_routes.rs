//! Drupal's route names: the keys a routing file declares, read by the
//! calls that build a URL or a redirect to a route.

use crate::common::{completion_response, hover_text_at, response_labels};
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
            "Unknown route: 'acme.greting'",
            "Unknown route: 'Drupal\\acme\\Routing\\Routes::routes'",
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
            "Unknown route: 'entity'",
            "Unknown route: 'entity.nod'",
            "Unknown route: 'view'",
            "Unknown route: 'view.content'",
        ]
    );
}

/// Test code passes invented route names to mocks, and a project whose
/// routing files were not read has nothing to judge against.
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
}
