use criterion::{Criterion, criterion_group, criterion_main};
use phpantom_lsp::Backend;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// Controllers in the generated application, each rendering `ACTIONS`
/// views from a directory of its own, and route groups, each declaring
/// `ACTIONS` routes with an action class per route.
const SECTIONS: usize = 30;
const ACTIONS: usize = 10;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/active-record": "^1.1",
        "yiisoft/router": "^4.0",
        "yiisoft/yii-view-renderer": "^7.4",
        "yiisoft/view": "^12.2"
    },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Yiisoft\\ActiveRecord\\": "stubs/active-record/",
        "Yiisoft\\Db\\": "stubs/db/",
        "Yiisoft\\FormModel\\": "stubs/form-model/",
        "Yiisoft\\Hydrator\\": "stubs/hydrator/",
        "Yiisoft\\Router\\": "stubs/router/",
        "Yiisoft\\Translator\\": "stubs/translator/",
        "Yiisoft\\Yii\\View\\Renderer\\": "stubs/yii-view-renderer/",
        "Yiisoft\\View\\": "stubs/view/"
    } },
    "extra": { "config-plugin": {
        "params": "config/common/params.php",
        "di": "config/di/*.php",
        "routes": "config/routes.php"
    } }
}"#;

/// Just enough of the view layer, the router, and Active Record for the
/// type engine to tell a renderer, a view, a URL generator, a record, and a
/// query from any other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/active-record/ActiveRecordInterface.php",
        "<?php\nnamespace Yiisoft\\ActiveRecord;\ninterface ActiveRecordInterface {\n    public static function query(self|string|null $modelClass = null): ActiveQueryInterface;\n    public function get(string $propertyName): mixed;\n    public function relationQuery(string $name): ActiveQueryInterface;\n    public function hasOne(self|string $modelClass, array $link): ActiveQueryInterface;\n    public function hasMany(self|string $modelClass, array $link): ActiveQueryInterface;\n}\n",
    ),
    (
        "stubs/active-record/ActiveRecord.php",
        "<?php\nnamespace Yiisoft\\ActiveRecord;\nclass ActiveRecord implements ActiveRecordInterface {\n    public static function query(ActiveRecordInterface|string|null $modelClass = null): ActiveQueryInterface {}\n    public function get(string $propertyName): mixed {}\n    public function relationQuery(string $name): ActiveQueryInterface {}\n    public function hasOne(ActiveRecordInterface|string $modelClass, array $link): ActiveQueryInterface {}\n    public function hasMany(ActiveRecordInterface|string $modelClass, array $link): ActiveQueryInterface {}\n}\n",
    ),
    (
        "stubs/active-record/ActiveQueryInterface.php",
        "<?php\nnamespace Yiisoft\\ActiveRecord;\ninterface ActiveQueryInterface extends \\Yiisoft\\Db\\Query\\QueryInterface {\n    public function with(array|string ...$with): static;\n    public function one(): array|ActiveRecordInterface|null;\n}\n",
    ),
    (
        "stubs/db/Query/QueryInterface.php",
        "<?php\nnamespace Yiisoft\\Db\\Query;\ninterface QueryInterface {\n    public function from(array|string $tables): static;\n    public function where(array|string|null $condition, array $params = []): static;\n}\n",
    ),
    (
        "stubs/yii-view-renderer/WebViewRenderer.php",
        "<?php\nnamespace Yiisoft\\Yii\\View\\Renderer;\nfinal class WebViewRenderer {\n    public function withViewPath(string $viewPath): self {}\n    public function render(string $view, array $parameters = []): object {}\n}\n",
    ),
    (
        "stubs/view/ViewInterface.php",
        "<?php\nnamespace Yiisoft\\View;\ninterface ViewInterface {\n    public function render(string $view, array $parameters = []): string;\n}\n",
    ),
    (
        "stubs/view/WebView.php",
        "<?php\nnamespace Yiisoft\\View;\nfinal class WebView implements ViewInterface {\n    public function render(string $view, array $parameters = []): string {}\n}\n",
    ),
    (
        "config/common/aliases.php",
        "<?php\nreturn [\n    '@root' => dirname(__DIR__, 2),\n    '@views' => '@root/resources/views',\n];\n",
    ),
    (
        "config/common/params.php",
        "<?php\nreturn [\n    'yiisoft/aliases' => ['aliases' => require __DIR__ . '/aliases.php'],\n    'yiisoft/db-migration' => ['sourceNamespaces' => ['App\\\\Migration']],\n];\n",
    ),
    (
        "stubs/router/Route.php",
        "<?php\nnamespace Yiisoft\\Router;\nfinal class Route {\n    public static function get(string $pattern): self {}\n    public function name(string $name): self {}\n    public function action(array|callable|string $middlewareDefinition): self {}\n}\n",
    ),
    (
        "stubs/router/Group.php",
        "<?php\nnamespace Yiisoft\\Router;\nfinal class Group {\n    public static function create(?string $prefix = null): self {}\n    public function routes(self|Route ...$routes): self {}\n    public function namePrefix(string $namePrefix): self {}\n}\n",
    ),
    (
        "stubs/router/UrlGeneratorInterface.php",
        "<?php\nnamespace Yiisoft\\Router;\ninterface UrlGeneratorInterface {\n    public function generate(string $name, array $arguments = [], array $queryParameters = []): string;\n}\n",
    ),
    (
        "stubs/translator/TranslatorInterface.php",
        "<?php\nnamespace Yiisoft\\Translator;\ninterface TranslatorInterface {\n    public function translate(string $id, array $parameters = [], ?string $category = null, ?string $locale = null): string;\n}\n",
    ),
    (
        "config/di/translation.php",
        "<?php\nreturn [\n    'translation.app' => static fn () => new \\Yiisoft\\Translator\\Message\\Php\\MessageSource(dirname(__DIR__, 2) . '/resources/messages'),\n];\n",
    ),
    (
        "stubs/form-model/FormModelInterface.php",
        "<?php\nnamespace Yiisoft\\FormModel;\ninterface FormModelInterface {}\n",
    ),
    (
        "stubs/form-model/FormModel.php",
        "<?php\nnamespace Yiisoft\\FormModel;\nabstract class FormModel implements FormModelInterface {}\n",
    ),
    (
        "stubs/form-model/Field.php",
        "<?php\nnamespace Yiisoft\\FormModel;\nclass Field {\n    final public static function text(FormModelInterface $formModel, string $property, array $config = []): string {}\n}\n",
    ),
    (
        "stubs/hydrator/Hydrator.php",
        "<?php\nnamespace Yiisoft\\Hydrator;\nfinal class Hydrator {\n    public function hydrate(object $object, array|ArrayData $data = []): void {}\n}\n",
    ),
    (
        "stubs/hydrator/ArrayData.php",
        "<?php\nnamespace Yiisoft\\Hydrator;\nfinal class ArrayData {\n    public function __construct(array $data = [], array $map = [], bool $strict = false) {}\n}\n",
    ),
    (
        "stubs/router/HydratorAttribute/RouteArgument.php",
        "<?php\nnamespace Yiisoft\\Router\\HydratorAttribute;\n#[\\Attribute]\nfinal class RouteArgument {\n    public function __construct(private readonly ?string $name = null) {}\n}\n",
    ),
];

/// An asset bundle whose `$css` entry is being typed, over a directory of
/// `ACTIONS` stylesheets per section.
const ASSET_CONSUMER_PHP: &str = r#"<?php
namespace App\Asset;

use Yiisoft\Assets\AssetBundle;

final class ConsumerAsset extends AssetBundle
{
    public ?string $sourcePath = '@root/assets';
    public array $css = ['section1/'];
}
"#;

/// The records' attributes and relations, named from a class of the
/// application, and the columns of a record that declares no properties.
const RECORD_CONSUMER_PHP: &str = r#"<?php
namespace App\Catalog;

final class Catalog
{
    public function read(Record1 $record): void
    {
        $record->get('');
        Record1::query()->with('')->one();
        Record1::query()->with('author.')->one();
    }

    public function review(Review $review): void
    {
        $review->get('');
        Review::query()->from('')->one();
        Review::query()->where(['' => 1])->one();
    }
}
"#;

const VIEW_CONSUMER_PHP: &str = r#"<?php
namespace App\Web\Consumer;

use Yiisoft\Yii\View\Renderer\WebViewRenderer;

final class ConsumerController
{
    private WebViewRenderer $viewRenderer;
    private string $logs = '@';

    public function __construct(WebViewRenderer $viewRenderer)
    {
        $this->viewRenderer = $viewRenderer->withViewPath('@views/section1');
    }

    public function show(): object
    {
        $this->viewRenderer->render(__DIR__ . '/');
        return $this->viewRenderer->render('');
    }

    public function parameters(): object
    {
        return $this->viewRenderer->render('action7', ['p']);
    }
}
"#;

/// A class of the project's own in front of the URL generator, the way
/// `yiisoft/demo-blog` builds its URLs.
const WRAPPER_PHP: &str = r#"<?php
namespace App\Shared;

use Yiisoft\Router\UrlGeneratorInterface;

final class UrlGenerator
{
    public function __construct(private UrlGeneratorInterface $generator) {}

    public function generate(string $name, array $arguments = []): string
    {
        return $this->generator->generate($name, $arguments);
    }
}
"#;

const CONSUMER_TEMPLATE: &str =
    "<?php\n/** @var \\Yiisoft\\View\\WebView $this */\necho $this->render('');\n";

const ROUTE_CONSUMER_PHP: &str = r#"<?php
namespace App\Web;

use App\Shared\UrlGenerator;
use Yiisoft\Router\HydratorAttribute\RouteArgument;
use Yiisoft\Router\UrlGeneratorInterface;

final class ConsumerAction
{
    public function __construct(private UrlGeneratorInterface $urls, private UrlGenerator $wrapped) {}

    public function __invoke(#[RouteArgument('')] string $slug): string
    {
        $this->urls->generate('section1/');
        $this->wrapped->generate('section1/');
        return $this->urls->generate('section1/action7', ['']);
    }
}
"#;

/// A DI configuration file binding the URL generator to the wrapper, the
/// way `yiisoft/demo-blog` binds its own.
const DI_CONSUMER_PHP: &str = r#"<?php
use App\Shared\UrlGenerator;
use Yiisoft\Router\UrlGeneratorInterface;

return [
    UrlGeneratorInterface::class => [
        'class' => UrlGenerator::class,
        '__construct()' => ['g' => 1],
        'gen' => [],
    ],
    'aliases' => $params['yiisoft/aliases']['a'],
];
"#;

/// A class translating a message whose ID is being typed, among the
/// `SECTIONS * ACTIONS` IDs the default category's message files declare.
const TRANSLATION_CONSUMER_PHP: &str = r#"<?php
namespace App\Web;

use Yiisoft\Translator\TranslatorInterface;

final class ConsumerTitle
{
    public function __construct(private TranslatorInterface $translator) {}

    public function __invoke(): string
    {
        return $this->translator->translate('section1.');
    }
}
"#;

/// A class rendering a field of a form model with `SECTIONS * ACTIONS`
/// properties, and filling the model through a hydration map, whose
/// property names are being typed.
const FORM_CONSUMER_PHP: &str = r#"<?php
namespace App\Web;

use Yiisoft\FormModel\Field;
use Yiisoft\Hydrator\ArrayData;
use Yiisoft\Hydrator\Hydrator;

final class ConsumerForm
{
    public function __invoke(Hydrator $hydrator, Form\LargeForm $form, array $data): string
    {
        $hydrator->hydrate($form, new ArrayData($data, ['section1' => 'name']));
        return Field::text($form, 'section1');
    }
}
"#;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// The files of a mid-sized Yii application: controllers that render the
/// views of a directory each, the views, and a partial each view renders,
/// and grouped routes, an action class per route that reads its route's
/// arguments, a wrapper around the URL generator, a DI definition that
/// binds it, and an Active Record per section, related to the next.
fn generate_application() -> Vec<(String, String)> {
    let mut files = Vec::new();

    for c in 0..SECTIONS {
        let mut php = format!(
            "<?php\nnamespace App\\Web\\Section{c};\n\nuse Yiisoft\\Yii\\View\\Renderer\\WebViewRenderer;\n\nfinal class Section{c}Controller\n{{\n    private WebViewRenderer $viewRenderer;\n\n    public function __construct(WebViewRenderer $viewRenderer)\n    {{\n        $this->viewRenderer = $viewRenderer->withViewPath('@views/section{c}');\n    }}\n\n"
        );
        for a in 0..ACTIONS {
            let _ = write!(
                php,
                "    public function action{a}(): object\n    {{\n        return $this->viewRenderer->render('action{a}');\n    }}\n\n"
            );
            files.push((
                format!("resources/views/section{c}/action{a}.php"),
                "<?php\n/**\n * @var \\Yiisoft\\View\\WebView $this\n * @var string $title\n * @var int $page\n */\necho $this->render('_partial');\n"
                    .to_string(),
            ));
        }
        php.push_str("}\n");
        files.push((format!("src/Web/Section{c}/Section{c}Controller.php"), php));
        files.push((
            format!("resources/views/section{c}/_partial.php"),
            "<p></p>\n".to_string(),
        ));
    }

    files.push((
        "src/Web/Consumer/ConsumerController.php".to_string(),
        VIEW_CONSUMER_PHP.to_string(),
    ));
    files.push((
        "src/Web/Consumer/template.php".to_string(),
        "<p></p>\n".to_string(),
    ));
    files.push((
        "resources/views/section1/consumer.php".to_string(),
        CONSUMER_TEMPLATE.to_string(),
    ));

    let mut routes = String::from(
        "<?php\n\nuse App\\Web;\nuse Yiisoft\\Router\\Group;\nuse Yiisoft\\Router\\Route;\n\nreturn [\n",
    );
    for s in 0..SECTIONS {
        let _ = writeln!(
            routes,
            "    Group::create('/section{s}')\n        ->namePrefix('section{s}/')\n        ->routes("
        );
        for a in 0..ACTIONS {
            let _ = writeln!(
                routes,
                "            Route::get('/action{a}/{{slug}}[/{{page}}]')->action(Web\\Section{s}\\Action{a}::class)->name('action{a}'),"
            );
            files.push((
                format!("src/Web/Section{s}/Action{a}.php"),
                format!(
                    "<?php\nnamespace App\\Web\\Section{s};\n\nuse Yiisoft\\Router\\HydratorAttribute\\RouteArgument;\n\nfinal class Action{a}\n{{\n    public function __invoke(#[RouteArgument('slug')] string $slug): void {{}}\n}}\n"
                ),
            ));
        }
        routes.push_str("        ),\n");
    }
    routes.push_str(
        "    Route::get('/consumer/{slug}')->action(Web\\ConsumerAction::class)->name('consumer'),\n];\n",
    );
    files.push(("config/routes.php".to_string(), routes));
    files.push((
        "src/Shared/UrlGenerator.php".to_string(),
        WRAPPER_PHP.to_string(),
    ));
    files.push((
        "src/Web/ConsumerAction.php".to_string(),
        ROUTE_CONSUMER_PHP.to_string(),
    ));
    files.push((
        "config/di/consumer.php".to_string(),
        DI_CONSUMER_PHP.to_string(),
    ));
    files.push((
        "src/Asset/ConsumerAsset.php".to_string(),
        ASSET_CONSUMER_PHP.to_string(),
    ));
    files.push((
        "src/Web/ConsumerTitle.php".to_string(),
        TRANSLATION_CONSUMER_PHP.to_string(),
    ));
    // A record per section, each related to the next.
    for s in 0..SECTIONS {
        let next = (s + 1) % SECTIONS;
        files.push((
            format!("src/Catalog/Record{s}.php"),
            format!(
                "<?php\nnamespace App\\Catalog;\n\nuse Yiisoft\\ActiveRecord\\ActiveQueryInterface;\nuse Yiisoft\\ActiveRecord\\ActiveRecord;\n\nfinal class Record{s} extends ActiveRecord\n{{\n    public int $id;\n    public string $title;\n    public int $author_id;\n\n    public function relationQuery(string $name): ActiveQueryInterface\n    {{\n        return match ($name) {{\n            'author' => $this->hasOne(Record{next}::class, ['id' => 'author_id']),\n            'posts' => $this->hasMany(Record{next}::class, ['author_id' => 'id']),\n            default => parent::relationQuery($name),\n        }};\n    }}\n}}\n"
            ),
        ));
    }
    files.push((
        "src/Catalog/Catalog.php".to_string(),
        RECORD_CONSUMER_PHP.to_string(),
    ));
    let mut form = String::from(
        "<?php\nnamespace App\\Web\\Form;\n\nfinal class LargeForm extends \\Yiisoft\\FormModel\\FormModel\n{\n",
    );
    for section in 0..SECTIONS {
        for action in 0..ACTIONS {
            let _ = writeln!(
                form,
                "    public string $section{section}Action{action} = '';"
            );
        }
    }
    form.push_str("}\n");
    files.push(("src/Web/Form/LargeForm.php".to_string(), form));
    files.push((
        "src/Web/ConsumerForm.php".to_string(),
        FORM_CONSUMER_PHP.to_string(),
    ));
    // A migration per section, each creating a table, and a record that
    // declares no properties, whose attributes are the columns of the table
    // the first migration creates and the last one adds to.
    files.push((
        "src/Catalog/Review.php".to_string(),
        "<?php\nnamespace App\\Catalog;\n\nuse Yiisoft\\ActiveRecord\\ActiveRecord;\n\n#[\\AllowDynamicProperties]\nfinal class Review extends ActiveRecord\n{\n}\n".to_string(),
    ));
    for s in 0..SECTIONS {
        let table = if s == 0 {
            "review".to_string()
        } else {
            format!("table{s}")
        };
        let mut php = format!(
            "<?php\nnamespace App\\Migration;\n\nfinal class M2511080000{s:02}Create\n{{\n    public function up(\\Yiisoft\\Db\\Migration\\MigrationBuilder $b): void\n    {{\n        $column = $b->columnBuilder();\n        $b->createTable('{{{{%{table}}}}}', [\n            'id' => $column::primaryKey(),\n"
        );
        for a in 0..ACTIONS {
            let _ = writeln!(
                php,
                "            'column{a}' => $column::string()->notNull(),"
            );
        }
        php.push_str("        ]);\n");
        if s == SECTIONS - 1 {
            php.push_str("        $b->addColumn('{{%review}}', 'rating', $column::integer());\n");
        }
        php.push_str("    }\n}\n");
        files.push((format!("src/Migration/M2511080000{s:02}Create.php"), php));
    }
    files
}

/// A backend over the generated application written to a temporary
/// directory, with every file open.
async fn setup_yii_backend() -> (Backend, tempfile::TempDir) {
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
    // The message files of the default category, which are never opened.
    let mut messages = String::from("<?php\nreturn [\n");
    for section in 0..SECTIONS {
        for action in 0..ACTIONS {
            let _ = writeln!(
                messages,
                "    'section{section}.action{action}' => 'Action {action}',"
            );
        }
    }
    messages.push_str("];\n");
    for locale in ["en", "de"] {
        let dir = dir.path().join(format!("resources/messages/{locale}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("app.php"), &messages).unwrap();
    }
    // The stylesheets the asset bundle lists, which are never opened.
    for section in 0..SECTIONS {
        let assets = dir.path().join(format!("assets/section{section}"));
        fs::create_dir_all(&assets).unwrap();
        for action in 0..ACTIONS {
            fs::write(assets.join(format!("action{action}.css")), "body {}\n").unwrap();
        }
    }

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);
    for (path, content) in &files {
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: Uri::from_file_path(dir.path().join(path)).unwrap(),
                    language_id: "php".to_string().into(),
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

fn bench_yii_name_completion(c: &mut Criterion) {
    let runtime = rt();
    let (backend, dir) = runtime.block_on(setup_yii_backend());
    let view_uri =
        Uri::from_file_path(dir.path().join("src/Web/Consumer/ConsumerController.php")).unwrap();
    let template_uri =
        Uri::from_file_path(dir.path().join("resources/views/section1/consumer.php")).unwrap();
    let route_uri = Uri::from_file_path(dir.path().join("src/Web/ConsumerAction.php")).unwrap();
    let di_uri = Uri::from_file_path(dir.path().join("config/di/consumer.php")).unwrap();
    let asset_uri = Uri::from_file_path(dir.path().join("src/Asset/ConsumerAsset.php")).unwrap();
    let translation_uri =
        Uri::from_file_path(dir.path().join("src/Web/ConsumerTitle.php")).unwrap();
    let record_uri = Uri::from_file_path(dir.path().join("src/Catalog/Catalog.php")).unwrap();
    let form_uri = Uri::from_file_path(dir.path().join("src/Web/ConsumerForm.php")).unwrap();

    // (benchmark, document, its text, the literal to complete inside, a
    // name the completion must offer)
    let cases = [
        (
            "view_name_php",
            &view_uri,
            VIEW_CONSUMER_PHP,
            "render(''",
            "action7",
        ),
        (
            "view_path_php",
            &view_uri,
            VIEW_CONSUMER_PHP,
            "'/'",
            "/template",
        ),
        (
            "view_name_template",
            &template_uri,
            CONSUMER_TEMPLATE,
            "render(''",
            "_partial",
        ),
        (
            "view_parameter",
            &view_uri,
            VIEW_CONSUMER_PHP,
            "render('action7', ['p'",
            "page",
        ),
        ("alias_name", &view_uri, VIEW_CONSUMER_PHP, "'@'", "@views"),
        (
            "asset_file",
            &asset_uri,
            ASSET_CONSUMER_PHP,
            "['section1/'",
            "action7.css",
        ),
        (
            "route_name",
            &route_uri,
            ROUTE_CONSUMER_PHP,
            "urls->generate('section1/'",
            "section1/action7",
        ),
        (
            "route_name_wrapper",
            &route_uri,
            ROUTE_CONSUMER_PHP,
            "wrapped->generate('section1/'",
            "section1/action7",
        ),
        (
            "route_parameter_key",
            &route_uri,
            ROUTE_CONSUMER_PHP,
            "[''",
            "page",
        ),
        (
            "route_argument",
            &route_uri,
            ROUTE_CONSUMER_PHP,
            "RouteArgument(''",
            "slug",
        ),
        (
            "definition_key",
            &di_uri,
            DI_CONSUMER_PHP,
            "'gen'",
            "generate()",
        ),
        (
            "definition_argument",
            &di_uri,
            DI_CONSUMER_PHP,
            "['g'",
            "generator",
        ),
        (
            "params_key",
            &di_uri,
            DI_CONSUMER_PHP,
            "['yiisoft/aliases']['a'",
            "aliases",
        ),
        (
            "message_id",
            &translation_uri,
            TRANSLATION_CONSUMER_PHP,
            "translate('section1.'",
            "section1.action7",
        ),
        (
            "record_attribute",
            &record_uri,
            RECORD_CONSUMER_PHP,
            "get(''",
            "title",
        ),
        (
            "record_column",
            &record_uri,
            RECORD_CONSUMER_PHP,
            "$review->get(''",
            "rating",
        ),
        (
            "query_table",
            &record_uri,
            RECORD_CONSUMER_PHP,
            "from(''",
            "table3",
        ),
        (
            "query_column",
            &record_uri,
            RECORD_CONSUMER_PHP,
            "where([''",
            "rating",
        ),
        (
            "record_relation",
            &record_uri,
            RECORD_CONSUMER_PHP,
            "with(''",
            "author",
        ),
        (
            "record_relation_path",
            &record_uri,
            RECORD_CONSUMER_PHP,
            "with('author.'",
            "posts",
        ),
        (
            "form_field",
            &form_uri,
            FORM_CONSUMER_PHP,
            "text($form, 'section1'",
            "section1Action7",
        ),
        (
            "hydration_map_key",
            &form_uri,
            FORM_CONSUMER_PHP,
            "['section1'",
            "section1Action7",
        ),
    ];

    let mut group = c.benchmark_group("yii_completion");
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

criterion_group!(benches, bench_yii_name_completion);
criterion_main!(benches);
