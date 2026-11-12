use criterion::{Criterion, criterion_group, criterion_main};
use phpantom_lsp::Backend;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// Custom modules in the generated site, each declaring `SERVICES`
/// services, `ROUTES` routes, `ENTITY_TYPES` entity types, `PERMISSIONS`
/// permissions, `CONFIGS` configuration objects, with their default
/// configuration and schema, `ELEMENTS` render elements, `BLOCKS` block
/// plugins, `THEME_HOOKS` theme hooks, and `LIBRARIES` libraries,
/// documenting `HOOKS` hooks, and implementing the hooks the module before
/// it documents.
const MODULES: usize = 30;
const SERVICES: usize = 10;
const ROUTES: usize = 10;
const ENTITY_TYPES: usize = 3;
const PERMISSIONS: usize = 10;
const CONFIGS: usize = 10;
const ELEMENTS: usize = 3;
const BLOCKS: usize = 3;
const HOOKS: usize = 10;
const THEME_HOOKS: usize = 5;
const LIBRARIES: usize = 10;

const COMPOSER: &str = r#"{
    "require": { "drupal/core-recommended": "^11" },
    "extra": { "drupal-scaffold": { "locations": { "web-root": "web/" } } }
}"#;

/// Just enough of core, where Composer puts it, for the type engine to
/// tell a container, the entity type manager, the module handler, an
/// account, a URL, and the block manager from any other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Component/Plugin/PluginManagerInterface.php",
        "<?php\nnamespace Drupal\\Component\\Plugin;\ninterface PluginManagerInterface {\n    public function createInstance($plugin_id, array $configuration = []);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Component/Plugin/Attribute/AttributeInterface.php",
        "<?php\nnamespace Drupal\\Component\\Plugin\\Attribute;\ninterface AttributeInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Plugin/DefaultPluginManager.php",
        "<?php\nnamespace Drupal\\Core\\Plugin;\nclass DefaultPluginManager implements \\Drupal\\Component\\Plugin\\PluginManagerInterface {\n    public function __construct($subdir, $namespaces, $module_handler, $plugin_interface = NULL, $plugin_definition_attribute_name = NULL, $plugin_definition_annotation_name = NULL) {}\n    public function createInstance($plugin_id, array $configuration = []) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Block/Attribute/Block.php",
        "<?php\nnamespace Drupal\\Core\\Block\\Attribute;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nclass Block implements \\Drupal\\Component\\Plugin\\Attribute\\AttributeInterface {\n    public function __construct(public readonly string $id) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Block/BlockManagerInterface.php",
        "<?php\nnamespace Drupal\\Core\\Block;\ninterface BlockManagerInterface extends \\Drupal\\Component\\Plugin\\PluginManagerInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Block/BlockManager.php",
        "<?php\nnamespace Drupal\\Core\\Block;\nuse Drupal\\Core\\Block\\Attribute\\Block;\nclass BlockManager extends \\Drupal\\Core\\Plugin\\DefaultPluginManager implements BlockManagerInterface {\n    public function __construct($namespaces, $module_handler) {\n        parent::__construct('Plugin/Block', $namespaces, $module_handler, NULL, Block::class, 'Drupal\\Core\\Block\\Annotation\\Block');\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal.php",
        "<?php\nclass Drupal {\n    public static function service(string $id): object {}\n    public static function hasService(string $id): bool {}\n    public static function config($name) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Url.php",
        "<?php\nnamespace Drupal\\Core;\nclass Url {\n    public function __construct($route_name, $route_parameters = [], $options = []) {}\n    public static function fromRoute($route_name, $route_parameters = [], $options = []) { return new static($route_name); }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/EntityTypeManagerInterface.php",
        "<?php\nnamespace Drupal\\Core\\Entity;\ninterface EntityTypeManagerInterface {\n    public function getStorage(string $entity_type_id): object;\n    public function getDefinition(string $entity_type_id, bool $exception_on_invalid = true): ?object;\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Entity/Attribute/ContentEntityType.php",
        "<?php\nnamespace Drupal\\Core\\Entity\\Attribute;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nclass ContentEntityType {\n    public function __construct(public readonly string $id) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Render/Attribute/RenderElement.php",
        "<?php\nnamespace Drupal\\Core\\Render\\Attribute;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nclass RenderElement {\n    public function __construct(public readonly string $id) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Extension/ModuleHandlerInterface.php",
        "<?php\nnamespace Drupal\\Core\\Extension;\ninterface ModuleHandlerInterface {\n    public function invokeAll($hook, array $args = []);\n    public function invoke($module, $hook, array $args = []);\n    public function alter($type, &$data, &$context1 = NULL, &$context2 = NULL);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Session/AccountInterface.php",
        "<?php\nnamespace Drupal\\Core\\Session;\ninterface AccountInterface {\n    public function hasPermission(string $permission);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Hook/Attribute/Hook.php",
        "<?php\nnamespace Drupal\\Core\\Hook\\Attribute;\n#[\\Attribute]\nclass Hook {\n    public function __construct(public string $hook, public string $method = '', public ?string $module = NULL) {}\n}\n",
    ),
];

const CONSUMER_PATH: &str = "web/modules/custom/acme/src/Consumer.php";

const CONSUMER_PHP: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Block\BlockManagerInterface;
use Drupal\Core\Entity\EntityTypeManagerInterface;
use Drupal\Core\Extension\ModuleHandlerInterface;
use Drupal\Core\Hook\Attribute\Hook;
use Drupal\Core\Session\AccountInterface;
use Drupal\Core\Url;

final class Consumer
{
    public function __construct(
        private EntityTypeManagerInterface $manager,
        private ModuleHandlerInterface $modules,
        private AccountInterface $account,
        private BlockManagerInterface $blocks,
    ) {}

    #[Hook('mod1_')]
    public function run(): void
    {
        \Drupal::service('mod1.');
        Url::fromRoute('mod1.');
        $this->manager->getStorage('mod1_');
        $this->modules->invokeAll('mod1_');
        $this->account->hasPermission('administer mod1 ');
        \Drupal::config('mod1.');
        $build = ['#type' => 'mod1_'];
        $this->blocks->createInstance('mod1_');
        $themed = ['#theme' => 'mod1_'];
        $build = ['#attached' => ['library' => ['mod1/']]];
        $build = ['#submit' => ['mod1_']];
        $build = ['#pre_render' => ['Drupal\acme\Consumer::r']];
    }
}

function mod1_submit(): void {}
"#;

const MODULE_PATH: &str = "web/modules/custom/acme/acme.module";

const MODULE_PHP: &str = "<?php\n\nfunction acme_mod1_\n";

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .thread_stack_size(phpantom_lsp::PARSE_WORKER_STACK_SIZE)
        .build()
        .unwrap()
}

/// The files of a site with many custom modules: their services, routes,
/// entity types, render elements, documented hooks, hook implementations,
/// and theme hooks.
fn generate_site() -> Vec<(String, String)> {
    let mut files = Vec::new();

    for m in 0..MODULES {
        let dir = format!("web/modules/custom/mod{m}");
        files.push((
            format!("{dir}/mod{m}.info.yml"),
            format!("name: Module {m}\ntype: module\ncore_version_requirement: ^11\n"),
        ));

        let mut services = String::from("services:\n");
        for s in 0..SERVICES {
            let _ = writeln!(
                services,
                "  mod{m}.service{s}:\n    class: Drupal\\mod{m}\\Service{s}"
            );
        }
        files.push((format!("{dir}/mod{m}.services.yml"), services));

        let mut routing = String::new();
        for r in 0..ROUTES {
            let _ = writeln!(
                routing,
                "mod{m}.route{r}:\n  path: '/mod{m}/route{r}/{{node}}'\n  defaults:\n    _controller: '\\Drupal\\mod{m}\\Controller\\PageController::route{r}'\n  requirements:\n    _permission: 'access content'"
            );
        }
        files.push((format!("{dir}/mod{m}.routing.yml"), routing));

        let mut permissions = String::new();
        for p in 0..PERMISSIONS {
            let _ = writeln!(
                permissions,
                "administer mod{m} thing{p}:\n  title: 'Administer thing {p} of module {m}'"
            );
        }
        files.push((format!("{dir}/mod{m}.permissions.yml"), permissions));

        let mut schema = String::new();
        for c in 0..CONFIGS {
            files.push((
                format!("{dir}/config/install/mod{m}.setting{c}.yml"),
                "enabled: true\n".to_string(),
            ));
            let _ = writeln!(
                schema,
                "mod{m}.setting{c}:\n  type: config_object\n  label: 'Setting {c} of module {m}'"
            );
        }
        files.push((format!("{dir}/config/schema/mod{m}.schema.yml"), schema));

        let mut libraries = String::new();
        for l in 0..LIBRARIES {
            let _ = writeln!(
                libraries,
                "library{l}:\n  version: 1.x\n  js:\n    js/library{l}.js: {{}}\n  dependencies:\n    - core/drupal"
            );
        }
        files.push((format!("{dir}/mod{m}.libraries.yml"), libraries));

        for e in 0..ENTITY_TYPES {
            files.push((
                format!("{dir}/src/Entity/Entity{e}.php"),
                format!(
                    "<?php\nnamespace Drupal\\mod{m}\\Entity;\n\nuse Drupal\\Core\\Entity\\Attribute\\ContentEntityType;\n\n#[ContentEntityType(\n    id: 'mod{m}_entity{e}',\n)]\nclass Entity{e} {{}}\n"
                ),
            ));
        }

        for e in 0..ELEMENTS {
            files.push((
                format!("{dir}/src/Element/Element{e}.php"),
                format!(
                    "<?php\nnamespace Drupal\\mod{m}\\Element;\n\nuse Drupal\\Core\\Render\\Attribute\\RenderElement;\n\n#[RenderElement('mod{m}_element{e}')]\nclass Element{e} {{}}\n"
                ),
            ));
        }

        for b in 0..BLOCKS {
            files.push((
                format!("{dir}/src/Plugin/Block/Block{b}.php"),
                format!(
                    "<?php\nnamespace Drupal\\mod{m}\\Plugin\\Block;\n\nuse Drupal\\Core\\Block\\Attribute\\Block;\n\n#[Block(id: 'mod{m}_block{b}')]\nclass Block{b} {{}}\n"
                ),
            ));
        }

        let mut api = String::from("<?php\n");
        for h in 0..HOOKS {
            let _ = write!(
                api,
                "\n/**\n * Respond to event {h} of module {m}.\n */\nfunction hook_mod{m}_event{h}(array $context) {{\n}}\n"
            );
        }
        files.push((format!("{dir}/mod{m}.api.php"), api));

        let mut theme = format!(
            "<?php\nnamespace Drupal\\mod{m}\\Hook;\n\nuse Drupal\\Core\\Hook\\Attribute\\Hook;\n\nfinal class ThemeHooks {{\n    #[Hook('theme')]\n    public function theme(): array {{\n        return [\n"
        );
        for t in 0..THEME_HOOKS {
            let _ = writeln!(
                theme,
                "            'mod{m}_widget{t}' => ['variables' => ['label' => NULL, 'items' => []]],"
            );
        }
        theme.push_str("        ];\n    }\n}\n");
        files.push((format!("{dir}/src/Hook/ThemeHooks.php"), theme));

        if m > 0 {
            let previous = m - 1;
            let mut module = String::from("<?php\n");
            for h in 0..HOOKS {
                let _ = write!(
                    module,
                    "\nfunction mod{m}_mod{previous}_event{h}(array $context) {{\n}}\n"
                );
            }
            files.push((format!("{dir}/mod{m}.module"), module));
        }
    }

    files.push((
        "web/modules/custom/acme/acme.info.yml".to_string(),
        "name: Acme\ntype: module\ncore_version_requirement: ^11\n".to_string(),
    ));
    files.push((CONSUMER_PATH.to_string(), CONSUMER_PHP.to_string()));
    files.push((MODULE_PATH.to_string(), MODULE_PHP.to_string()));
    files
}

/// A backend over the generated site written to a temporary directory,
/// initialized the way an editor starts it, with core left out by the
/// site's `.gitignore` and the consumers open.
async fn setup_drupal_backend() -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let files = generate_site();
    let all = STUBS
        .iter()
        .copied()
        .chain([
            ("composer.json", COMPOSER),
            (".git/HEAD", "ref: refs/heads/main\n"),
            (".gitignore", "/web/core/\n"),
        ])
        .chain(
            files
                .iter()
                .map(|(path, content)| (path.as_str(), content.as_str())),
        );
    for (path, content) in all {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, content).unwrap();
    }

    let backend = Backend::new_test();
    #[allow(deprecated)] // `root_uri` is what this generation of clients sends
    let params = InitializeParams {
        root_uri: Some(Uri::from_file_path(dir.path()).unwrap()),
        ..Default::default()
    };
    backend.initialize(params).await.unwrap();
    backend.initialized(InitializedParams {}).await;
    // A test backend has no client, so `initialized` leaves out the
    // background index that reads core's and the modules' declarations.
    backend.ensure_workspace_indexed();
    for (path, content) in [(CONSUMER_PATH, CONSUMER_PHP), (MODULE_PATH, MODULE_PHP)] {
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri_for(&dir, path),
                    language_id: "php".to_string().into(),
                    version: 1,
                    text: content.to_string(),
                },
            })
            .await;
    }
    (backend, dir)
}

fn uri_for(dir: &tempfile::TempDir, path: &str) -> Uri {
    Uri::from_file_path(dir.path().join(path)).unwrap()
}

/// Completion params with the cursor just before the last character of
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

fn bench_drupal_name_completion(c: &mut Criterion) {
    let runtime = rt();
    let (backend, dir) = runtime.block_on(setup_drupal_backend());
    let consumer = uri_for(&dir, CONSUMER_PATH);
    let module = uri_for(&dir, MODULE_PATH);

    // (benchmark, document, its text, the text ending one character past
    // the cursor, a name the completion must offer)
    let cases = [
        (
            "service_id",
            &consumer,
            CONSUMER_PHP,
            "service('mod1.'",
            "mod1.service7",
        ),
        (
            "route_name",
            &consumer,
            CONSUMER_PHP,
            "fromRoute('mod1.'",
            "mod1.route7",
        ),
        (
            "entity_type_id",
            &consumer,
            CONSUMER_PHP,
            "getStorage('mod1_'",
            "mod1_entity2",
        ),
        (
            "permission_name",
            &consumer,
            CONSUMER_PHP,
            "hasPermission('administer mod1 '",
            "administer mod1 thing7",
        ),
        (
            "config_name",
            &consumer,
            CONSUMER_PHP,
            "config('mod1.'",
            "mod1.setting7",
        ),
        (
            "render_element",
            &consumer,
            CONSUMER_PHP,
            "'#type' => 'mod1_'",
            "mod1_element2",
        ),
        (
            "plugin_id",
            &consumer,
            CONSUMER_PHP,
            "createInstance('mod1_'",
            "mod1_block2",
        ),
        (
            "hook_name_invoke_all",
            &consumer,
            CONSUMER_PHP,
            "invokeAll('mod1_'",
            "mod1_event7",
        ),
        (
            "hook_name_attribute",
            &consumer,
            CONSUMER_PHP,
            "#[Hook('mod1_'",
            "mod1_event7",
        ),
        (
            "theme_hook_name",
            &consumer,
            CONSUMER_PHP,
            "'#theme' => 'mod1_'",
            "mod1_widget4",
        ),
        (
            "library_name",
            &consumer,
            CONSUMER_PHP,
            "['library' => ['mod1/'",
            "mod1/library7",
        ),
        (
            "callback_function",
            &consumer,
            CONSUMER_PHP,
            "['#submit' => ['mod1_'",
            "Drupal\\acme\\mod1_submit",
        ),
        (
            "callback_method",
            &consumer,
            CONSUMER_PHP,
            "['#pre_render' => ['Drupal\\acme\\Consumer::r'",
            "run",
        ),
        (
            "hook_implementation",
            &module,
            MODULE_PHP,
            "function acme_mod1_\n",
            "acme_mod1_event7",
        ),
    ];

    let mut group = c.benchmark_group("drupal_completion");
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

criterion_group!(benches, bench_drupal_name_completion);
criterion_main!(benches);
