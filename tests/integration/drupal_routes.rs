//! Drupal's route names: the keys a routing file declares, read by the
//! calls that build a URL or a redirect to a route.

use crate::common::{completion_response, hover_text_at, response_labels};
use crate::drupal_services::{
    definitions_at, position_in, references_at, uri_for, workspace, workspace_in,
};

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
