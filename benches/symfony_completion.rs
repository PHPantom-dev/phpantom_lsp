use criterion::{Criterion, criterion_group, criterion_main};
use phpantom_lsp::Backend;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// Controllers in the generated application, each declaring `ACTIONS`
/// routes and rendering one template per route.
const CONTROLLERS: usize = 30;
const ACTIONS: usize = 10;
const YAML_ROUTES: usize = 100;
const SERVICES: usize = 200;
/// Catalogue sections, each holding `KEYS` translation keys, in each of
/// two locales.
const SECTIONS: usize = 60;
const KEYS: usize = 20;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Psr\\Container\\": "stubs/psr-container/",
        "Symfony\\Bundle\\FrameworkBundle\\": "stubs/framework-bundle/",
        "Symfony\\Component\\Routing\\": "stubs/routing/",
        "Symfony\\Contracts\\Translation\\": "stubs/translation-contracts/"
    } }
}"#;

/// Just enough of the framework for the type engine to tell a container,
/// a translator, and a controller from any other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/psr-container/ContainerInterface.php",
        "<?php\nnamespace Psr\\Container;\ninterface ContainerInterface {\n    public function get(string $id): mixed;\n    public function has(string $id): bool;\n}\n",
    ),
    (
        "stubs/framework-bundle/Controller/AbstractController.php",
        "<?php\nnamespace Symfony\\Bundle\\FrameworkBundle\\Controller;\nabstract class AbstractController {\n    protected function generateUrl(string $route, array $parameters = []): string {}\n    protected function redirectToRoute(string $route, array $parameters = []): object {}\n    protected function render(string $view, array $parameters = []): object {}\n}\n",
    ),
    (
        "stubs/routing/Attribute/Route.php",
        "<?php\nnamespace Symfony\\Component\\Routing\\Attribute;\n#[\\Attribute]\nclass Route {\n    public function __construct(string|array|null $path = null, ?string $name = null, string|array $alias = []) {}\n}\n",
    ),
    (
        "stubs/translation-contracts/TranslatorInterface.php",
        "<?php\nnamespace Symfony\\Contracts\\Translation;\ninterface TranslatorInterface {\n    public function trans(string $id, array $parameters = [], ?string $domain = null, ?string $locale = null): string;\n}\n",
    ),
];

const CONSUMER_PHP: &str = r#"<?php
namespace App\Controller;

use Psr\Container\ContainerInterface;
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Contracts\Translation\TranslatorInterface;

final class ConsumerController extends AbstractController
{
    public function show(ContainerInterface $container, TranslatorInterface $translator): object
    {
        $container->get('app.service1');
        $translator->trans('section1.');
        $this->generateUrl('app_section1_');
        return $this->render('section1/');
    }
}
"#;

const CONSUMER_TWIG: &str = "<a href=\"{{ path('app_section1_') }}\">{{ 'section1.'|trans }}</a>\n";

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// The files of a mid-sized Symfony application: attribute and YAML
/// routes, services, a catalogue in two locales, and a template per route.
fn generate_application() -> Vec<(String, String)> {
    let mut files = Vec::new();

    for c in 0..CONTROLLERS {
        let mut php = format!(
            "<?php\nnamespace App\\Controller;\n\nuse Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController;\nuse Symfony\\Component\\Routing\\Attribute\\Route;\n\n#[Route('/section{c}', name: 'app_section{c}_')]\nfinal class Section{c}Controller extends AbstractController\n{{\n"
        );
        for a in 0..ACTIONS {
            let _ = write!(
                php,
                "    #[Route('/action{a}', name: 'action{a}')]\n    public function action{a}(): object\n    {{\n        return $this->render('section{c}/action{a}.html.twig');\n    }}\n\n"
            );
            files.push((
                format!("templates/section{c}/action{a}.html.twig"),
                format!(
                    "{{% extends 'base.html.twig' %}}\n{{% block body %}}\n<a href=\"{{{{ path('app_section{c}_action{a}') }}}}\">{{{{ 'section{c}.key{a}'|trans }}}}</a>\n{{% endblock %}}\n"
                ),
            ));
        }
        php.push_str("}\n");
        files.push((format!("src/Controller/Section{c}Controller.php"), php));
    }
    files.push((
        "templates/base.html.twig".to_string(),
        "<html><body>{% block body %}{% endblock %}</body></html>\n".to_string(),
    ));

    let mut routes =
        String::from("controllers:\n    resource: ../src/Controller/\n    type: attribute\n");
    for r in 0..YAML_ROUTES {
        let _ = write!(
            routes,
            "\napp_legacy{r}:\n    path: /legacy/{r}\n    controller: App\\Controller\\Section0Controller::action0\n"
        );
    }
    files.push(("config/routes.yaml".to_string(), routes));

    let mut services = String::from("services:\n");
    for s in 0..SERVICES {
        let _ = writeln!(
            services,
            "    app.service{s}:\n        class: App\\Service\\Service{s}"
        );
    }
    files.push(("config/services.yaml".to_string(), services));

    for locale in ["en", "da"] {
        let mut catalogue = String::new();
        for s in 0..SECTIONS {
            let _ = writeln!(catalogue, "section{s}:");
            for k in 0..KEYS {
                let _ = writeln!(catalogue, "    key{k}: Text {s} {k} {locale}");
            }
        }
        files.push((format!("translations/messages.{locale}.yaml"), catalogue));
    }

    files.push((
        "src/Controller/ConsumerController.php".to_string(),
        CONSUMER_PHP.to_string(),
    ));
    files.push((
        "templates/consumer.html.twig".to_string(),
        CONSUMER_TWIG.to_string(),
    ));
    files
}

fn language_id(path: &str) -> &'static str {
    if path.ends_with(".php") {
        "php"
    } else if path.ends_with(".twig") {
        "twig"
    } else {
        "yaml"
    }
}

/// A backend over the generated application written to a temporary
/// directory, with every file open.
async fn setup_symfony_backend() -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("composer.json"), COMPOSER).unwrap();
    let files: Vec<(String, String)> = STUBS
        .iter()
        .map(|(path, content)| (path.to_string(), content.to_string()))
        .chain(generate_application())
        .collect();
    for (path, content) in &files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, content).unwrap();
    }

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);
    for (path, content) in &files {
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: Uri::from_file_path(dir.path().join(path)).unwrap(),
                    language_id: language_id(path).to_string().into(),
                    version: 1,
                    text: content.clone(),
                },
            })
            .await;
    }
    (backend, dir)
}

/// Completion params with the cursor just before the closing quote of
/// `needle`'s first occurrence in `content`.
fn params_at(uri: &Uri, content: &str, needle: &str) -> CompletionParams {
    let offset = content.find(needle).expect("needle should exist") + needle.len() - 1;
    let before = &content[..offset];
    let line = before.matches('\n').count() as u32;
    let character = before.rsplit('\n').next().unwrap().len() as u32;
    CompletionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: Some(CompletionContext {
            trigger_kind: CompletionTriggerKind::Invoked,
            trigger_character: None,
        }),
    }
}

fn labels(response: Option<CompletionResponse>) -> Vec<String> {
    let items = match response {
        Some(CompletionResponse::CompletionItemList(items)) => items,
        Some(CompletionResponse::CompletionList(list)) => list.items,
        None => Vec::new(),
    };
    items.into_iter().map(|item| item.label).collect()
}

fn bench_symfony_name_completion(c: &mut Criterion) {
    let runtime = rt();
    let (backend, dir) = runtime.block_on(setup_symfony_backend());
    let php_uri =
        Uri::from_file_path(dir.path().join("src/Controller/ConsumerController.php")).unwrap();
    let twig_uri = Uri::from_file_path(dir.path().join("templates/consumer.html.twig")).unwrap();

    // (benchmark, document, its text, the literal to complete inside, a
    // name the completion must offer)
    let cases = [
        (
            "service_id_php",
            &php_uri,
            CONSUMER_PHP,
            "get('app.service1'",
            "app.service17",
        ),
        (
            "translation_key_php",
            &php_uri,
            CONSUMER_PHP,
            "trans('section1.'",
            "section1.key7",
        ),
        (
            "route_name_php",
            &php_uri,
            CONSUMER_PHP,
            "generateUrl('app_section1_'",
            "app_section1_action7",
        ),
        (
            "template_name_php",
            &php_uri,
            CONSUMER_PHP,
            "render('section1/'",
            "section1/action7.html.twig",
        ),
        (
            "route_name_twig",
            &twig_uri,
            CONSUMER_TWIG,
            "path('app_section1_'",
            "app_section1_action7",
        ),
    ];

    let mut group = c.benchmark_group("symfony_completion");
    for (name, uri, content, needle, expected) in cases {
        let params = params_at(uri, content, needle);
        let request = || {
            runtime
                .block_on(backend.completion(params.clone()))
                .unwrap()
        };

        let offered = labels(request());
        assert!(
            offered.iter().any(|label| label == expected),
            "{name}: expected {expected} among {offered:?}"
        );

        group.bench_function(name, |b| b.iter(|| black_box(request())));
    }
    group.finish();
}

criterion_group!(benches, bench_symfony_name_completion);
criterion_main!(benches);
