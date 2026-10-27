use criterion::{Criterion, criterion_group, criterion_main};
use phpantom_lsp::Backend;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// Controllers in the generated application, each rendering `ACTIONS`
/// views from a directory of its own.
const CONTROLLERS: usize = 30;
const ACTIONS: usize = 10;

const COMPOSER: &str = r#"{
    "require": { "yiisoft/yii-view-renderer": "^7.4", "yiisoft/view": "^12.2" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Yiisoft\\Yii\\View\\Renderer\\": "stubs/yii-view-renderer/",
        "Yiisoft\\View\\": "stubs/view/"
    } }
}"#;

/// Just enough of the view layer for the type engine to tell a renderer
/// and a view from any other receiver.
const STUBS: &[(&str, &str)] = &[
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
        "<?php\nreturn [\n    'yiisoft/aliases' => ['aliases' => require __DIR__ . '/aliases.php'],\n];\n",
    ),
];

const CONSUMER_PHP: &str = r#"<?php
namespace App\Web\Consumer;

use Yiisoft\Yii\View\Renderer\WebViewRenderer;

final class ConsumerController
{
    private WebViewRenderer $viewRenderer;

    public function __construct(WebViewRenderer $viewRenderer)
    {
        $this->viewRenderer = $viewRenderer->withViewPath('@views/section1');
    }

    public function show(): object
    {
        $this->viewRenderer->render(__DIR__ . '/');
        return $this->viewRenderer->render('');
    }
}
"#;

const CONSUMER_TEMPLATE: &str =
    "<?php\n/** @var \\Yiisoft\\View\\WebView $this */\necho $this->render('');\n";

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// The files of a mid-sized Yii application: controllers that render the
/// views of a directory each, the views, and a partial each view renders.
fn generate_application() -> Vec<(String, String)> {
    let mut files = Vec::new();

    for c in 0..CONTROLLERS {
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
                "<?php\n/** @var \\Yiisoft\\View\\WebView $this */\necho $this->render('_partial');\n"
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
        CONSUMER_PHP.to_string(),
    ));
    files.push((
        "src/Web/Consumer/template.php".to_string(),
        "<p></p>\n".to_string(),
    ));
    files.push((
        "resources/views/section1/consumer.php".to_string(),
        CONSUMER_TEMPLATE.to_string(),
    ));
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
    let php_uri =
        Uri::from_file_path(dir.path().join("src/Web/Consumer/ConsumerController.php")).unwrap();
    let template_uri =
        Uri::from_file_path(dir.path().join("resources/views/section1/consumer.php")).unwrap();

    // (benchmark, document, its text, the literal to complete inside, a
    // name the completion must offer)
    let cases = [
        (
            "view_name_php",
            &php_uri,
            CONSUMER_PHP,
            "render(''",
            "action7",
        ),
        ("view_path_php", &php_uri, CONSUMER_PHP, "'/'", "/template"),
        (
            "view_name_template",
            &template_uri,
            CONSUMER_TEMPLATE,
            "render(''",
            "_partial",
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
