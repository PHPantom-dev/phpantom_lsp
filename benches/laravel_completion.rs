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

async fn setup_laravel_backend() -> Backend {
    let mut stubs = HashMap::new();
    stubs.insert("Illuminate\\Database\\Eloquent\\Model", "<?php namespace Illuminate\\Database\\Eloquent; class Model { public static function query(): Builder {} }");
    stubs.insert("Illuminate\\Database\\Eloquent\\Builder", "<?php namespace Illuminate\\Database\\Eloquent; class Builder { public function where($column): self {} public function whereIn($column, $values): self {} public function orWhere($column): self {} }");

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

fn generate_laravel_model_source() -> String {
    r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;

/**
 * @property string $name
 * @property string $email
 */
class User extends Model {}

$user = new User();
$user->wher
"#
    .to_string()
}

fn bench_laravel_model_completion(c: &mut Criterion) {
    let runtime = rt();
    let backend = runtime.block_on(setup_laravel_backend());
    let source = generate_laravel_model_source();
    let uri = runtime.block_on(open_file(&backend, "file:///app/Models/User.php", &source));
    let lines: Vec<&str> = source.lines().collect();
    let line = lines.len() as u32 - 1;
    let last_line = lines.last().unwrap();
    let col = last_line.len() as u32; // After 'wher'

    let mut group = c.benchmark_group("laravel_completion");

    group.bench_function("model_where_prefix", |b| {
        b.iter(|| {
            runtime.block_on(async {
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

    // Simulate typing: Model::w -> Model::wh -> Model::whe -> Model::wher
    group.bench_function("model_typing_sequence", |b| {
        b.iter(|| {
            runtime.block_on(async {
                for i in 1..=4 {
                    let current_col = col - 4 + i;
                    let params = CompletionParams {
                        text_document_position: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri: uri.clone() },
                            position: Position {
                                line,
                                character: current_col,
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
                }
            })
        })
    });

    group.finish();
}

criterion_group!(benches, bench_laravel_model_completion);
criterion_main!(benches);
