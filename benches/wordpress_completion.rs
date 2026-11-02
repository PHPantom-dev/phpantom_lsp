use criterion::{Criterion, criterion_group, criterion_main};
use phpantom_lsp::Backend;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// Files of the project's own plugin, each firing `HOOKS` hooks and
/// listening to the hooks the file before it fires.
const FILES: usize = 40;
const HOOKS: usize = 10;
/// Plugins Composer installs, each with `PLUGIN_FILES` files firing
/// `HOOKS` hooks apiece.
const PLUGINS: usize = 10;
const PLUGIN_FILES: usize = 5;

/// The slice of `php-stubs/wordpress-stubs` the stub fixtures run against,
/// read relative to the repository root the benchmarks run from.
const STUBS: &str = "tests/wordpress_stubs/stubs/wordpress-stubs.php";

const COMPOSER: &str = r#"{
    "autoload": { "psr-4": { "Acme\\Shop\\": "src/" } }
}"#;

const CONFIG: &str = "[stubs]\npaths = [\"stubs\"]\n";

const CONSUMER_PHP: &str = r#"<?php
namespace Acme\Shop;

final class Consumer
{
    public function register(): void
    {
        add_action('acme_shop_1_');
        apply_filters('acme_shop_1_');
        add_action('acme_shop_7_3', [$this, 'handle']);
        add_action('vendor_plugin_3_');
    }

    public function handle(): void
    {
        global $wpdb;
        $wpdb->get_;
        $post = get_post(1);
        $post->post_;
    }
}
"#;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .thread_stack_size(phpantom_lsp::PARSE_WORKER_STACK_SIZE)
        .build()
        .unwrap()
}

/// The files of a project: its own plugin and the plugins Composer
/// installs beside the stubs.
fn generate_project() -> Vec<(String, String)> {
    let mut files = Vec::new();

    for f in 0..FILES {
        let mut php = format!(
            "<?php\nnamespace Acme\\Shop;\n\nfinal class Module{f}\n{{\n    public function boot(object $order): void\n    {{\n"
        );
        for h in 0..HOOKS {
            let _ = writeln!(
                php,
                "        do_action('acme_shop_{f}_{h}', $order);\n        $total = apply_filters('acme_shop_{f}_{h}_total', 0, $order);"
            );
        }
        if f > 0 {
            let previous = f - 1;
            for h in 0..HOOKS {
                let _ = writeln!(
                    php,
                    "        add_action('acme_shop_{previous}_{h}', [$this, 'listen']);"
                );
            }
        }
        php.push_str("    }\n\n    public function listen(): void {}\n}\n");
        files.push((format!("src/Module{f}.php"), php));
    }
    files.push(("src/Consumer.php".to_string(), CONSUMER_PHP.to_string()));

    let mut packages = Vec::new();
    for p in 0..PLUGINS {
        packages.push(format!(
            "    {{ \"name\": \"wpackagist-plugin/vendor-plugin-{p}\", \"type\": \"wordpress-plugin\", \"install-path\": \"../wpackagist-plugin/vendor-plugin-{p}\" }}"
        ));
        for f in 0..PLUGIN_FILES {
            let mut php = String::from("<?php\n");
            for h in 0..HOOKS {
                let _ = writeln!(
                    php,
                    "do_action('vendor_plugin_{p}_{f}_{h}');\napply_filters(\"vendor_plugin_{p}_{f}_{h}_{{$type}}\", $value);"
                );
            }
            files.push((
                format!("vendor/wpackagist-plugin/vendor-plugin-{p}/includes/file{f}.php"),
                php,
            ));
        }
    }
    files.push((
        "vendor/composer/installed.json".to_string(),
        format!("{{ \"packages\": [\n{}\n] }}\n", packages.join(",\n")),
    ));
    files.push(("composer.json".to_string(), COMPOSER.to_string()));
    files.push((".phpantom.toml".to_string(), CONFIG.to_string()));
    files
}

/// A backend over the generated project written to a temporary
/// directory, initialized the way an editor starts it, with the stubs as
/// its stub path and the consumer open.
async fn setup_wordpress_backend() -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let stubs = fs::read_to_string(STUBS).unwrap_or_else(|e| panic!("{STUBS}: {e}"));
    let files = generate_project();
    let all = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .chain([("stubs/wordpress-stubs.php", stubs.as_str())]);
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
    // background index that publishes the project's hooks.
    backend.ensure_workspace_indexed();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: consumer_uri(&dir),
                language_id: "php".to_string().into(),
                version: 1,
                text: CONSUMER_PHP.to_string(),
            },
        })
        .await;
    (backend, dir)
}

fn consumer_uri(dir: &tempfile::TempDir) -> Uri {
    Uri::from_file_path(dir.path().join("src/Consumer.php")).unwrap()
}

/// The position just before the last character of `needle`'s first
/// occurrence in the consumer.
fn position_before_end_of(needle: &str) -> Position {
    let offset = CONSUMER_PHP.find(needle).expect("needle should exist") + needle.len() - 1;
    let before = &CONSUMER_PHP[..offset];
    Position {
        line: before.matches('\n').count() as u32,
        character: before.rsplit('\n').next().unwrap().len() as u32,
    }
}

fn completion_params(uri: &Uri, position: Position) -> CompletionParams {
    CompletionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
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

fn bench_wordpress(c: &mut Criterion) {
    let runtime = rt();
    let (backend, dir) = runtime.block_on(setup_wordpress_backend());
    let uri = consumer_uri(&dir);

    // (benchmark, the text ending where the cursor goes, a label the
    // completion must offer). The cursor sits before the needle's last
    // character: the closing quote, or the `;` after a member prefix.
    let cases = [
        (
            "hook_name_add_action",
            "add_action('acme_shop_1_'",
            "acme_shop_1_7",
        ),
        (
            "hook_name_apply_filters",
            "apply_filters('acme_shop_1_'",
            "acme_shop_1_7_total",
        ),
        ("stub_member_global_wpdb", "$wpdb->get_;", "get_results"),
        ("stub_member_get_post", "$post->post_;", "post_title"),
    ];

    // The installed plugins' hooks are among the names every hook
    // completion collects.
    let offered = labels(
        runtime
            .block_on(backend.completion(completion_params(
                &uri,
                position_before_end_of("add_action('vendor_plugin_3_'"),
            )))
            .unwrap(),
    );
    assert!(
        offered.iter().any(|label| label == "vendor_plugin_3_2_4"),
        "installed plugin hooks: {offered:?}"
    );

    let mut group = c.benchmark_group("wordpress_completion");
    for (name, needle, expected) in cases {
        let params = completion_params(&uri, position_before_end_of(needle));
        let request = || {
            runtime
                .block_on(backend.completion(params.clone()))
                .unwrap()
        };

        let offered = labels(request());
        assert!(
            // A method's label carries its signature.
            offered
                .iter()
                .any(|label| label.split('(').next() == Some(expected)),
            "{name}: expected {expected} among {offered:?}"
        );

        group.bench_function(name, |b| b.iter(|| black_box(request())));
    }

    let firing = Uri::from_file_path(dir.path().join("src/Module7.php")).unwrap();
    let params = DefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: position_before_end_of("add_action('acme_shop_7_3'"),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let request = || {
        runtime
            .block_on(backend.goto_definition(params.clone()))
            .unwrap()
    };
    let found = match request() {
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        other => panic!("hook_definition: unexpected {other:?}"),
    };
    assert!(
        found.iter().any(|location| location.uri == firing),
        "hook_definition: expected {firing:?} among {found:?}"
    );
    group.bench_function("hook_definition", |b| b.iter(|| black_box(request())));
    group.finish();
}

criterion_group!(benches, bench_wordpress);
criterion_main!(benches);
