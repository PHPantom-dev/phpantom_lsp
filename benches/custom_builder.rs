use criterion::{Criterion, black_box, criterion_group, criterion_main};
use phpantom_lsp::Backend;
use std::collections::HashMap;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

async fn setup_laravel_backend() -> Backend {
    let mut stubs = HashMap::new();
    stubs.insert("Illuminate\\Database\\Eloquent\\Model", "<?php namespace Illuminate\\Database\\Eloquent; class Model { public static function query(): CustomBuilder {} }");
    stubs.insert("Illuminate\\Database\\Eloquent\\Builder", "<?php namespace Illuminate\\Database\\Eloquent; class Builder { public function where($column): self {} }");

    let mut user_src = String::from(
        "<?php namespace App\\Models; class User extends \\Illuminate\\Database\\Eloquent\\Model {\n",
    );
    for i in 0..200 {
        user_src.push_str(&format!(
            "    public function scopeActive{}($query) {{}}\n",
            i
        ));
        user_src.push_str(&format!("    public $prop{};\n", i));
    }
    user_src.push('}');
    stubs.insert("App\\Models\\User", leak_str(user_src));

    // Deep inheritance for the builder
    for i in 0..10 {
        let parent = if i == 0 {
            "Illuminate\\Database\\Eloquent\\Builder"
        } else {
            leak_str(format!("App\\QueryBuilders\\BaseBuilder{}", i - 1))
        };
        let current = leak_str(format!("App\\QueryBuilders\\BaseBuilder{}", i));
        let src = leak_str(format!(
            "<?php namespace App\\QueryBuilders; class BaseBuilder{} extends {} {{ public function m{}(): void {{}} }}",
            i, parent, i
        ));
        stubs.insert(current, src);
    }

    stubs.insert("App\\QueryBuilders\\CustomBuilder", "<?php namespace App\\QueryBuilders; use App\\QueryBuilders\\BaseBuilder9; class CustomBuilder extends BaseBuilder9 { public function customMethod(): void {} }");

    Backend::new_test_with_stubs(stubs)
}

async fn open_file(backend: &Backend, uri_str: &str, content: &str) -> Url {
    let uri = Url::parse(uri_str).unwrap();
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: content.to_string(),
        },
    };
    backend.did_open(params).await;
    uri
}

fn generate_source() -> String {
    r#"<?php
namespace App\Models;
use App\QueryBuilders\CustomBuilder;

/** @var CustomBuilder<\App\Models\User> $query */
$query->wher
"#
    .to_string()
}

fn bench_custom_builder_completion(c: &mut Criterion) {
    let runtime = rt();
    let backend = runtime.block_on(setup_laravel_backend());
    let source = generate_source();
    let uri = runtime.block_on(open_file(&backend, "file:///app/Models/User.php", &source));
    let lines: Vec<&str> = source.lines().collect();
    let line = lines.len() as u32 - 1;
    let last_line = lines.last().unwrap();
    let col = last_line.len() as u32;

    let mut group = c.benchmark_group("custom_builder_completion");

    group.bench_function("custom_builder_deep_inheritance", |b| {
        b.iter(|| {
            runtime.block_on(async {
                backend.clear_completion_cache();
                let params = CompletionParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri: uri.clone() },
                        position: Position {
                            line,
                            character: col,
                        },
                    },
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                    context: Some(CompletionContext {
                        trigger_kind: CompletionTriggerKind::INVOKED,
                        trigger_character: None,
                    }),
                };
                let _ = black_box(backend.completion(params).await);
            })
        })
    });

    group.finish();
}

criterion_group!(benches, bench_custom_builder_completion);
criterion_main!(benches);
