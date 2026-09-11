use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
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
    stubs.insert("Illuminate\\Database\\Eloquent\\Model", "<?php namespace Illuminate\\Database\\Eloquent; class Model { public static function query(): Builder {} public function morphTo(): Relations\\MorphTo {} }");
    stubs.insert("Illuminate\\Database\\Eloquent\\Builder", "<?php namespace Illuminate\\Database\\Eloquent; class Builder { public function where($column): self {} public function whereIn($column, $values): self {} public function orWhere($column): self {} }");
    stubs.insert("Illuminate\\Database\\Eloquent\\Relations\\Relation", "<?php namespace Illuminate\\Database\\Eloquent\\Relations; class Relation { public static function morphMap(array $map): void {} }");
    stubs.insert("Illuminate\\Database\\Eloquent\\Relations\\MorphTo", "<?php namespace Illuminate\\Database\\Eloquent\\Relations; class MorphTo extends Relation {}");

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

fn generate_morph_column_source(literal_count: usize) -> (String, Position, Range) {
    let mut source = String::from(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\MorphTo;
use Illuminate\Database\Eloquent\Relations\Relation;

class Post extends Model {}
class Comment extends Model {
    protected $table = 'comments';
    public function subject(): MorphTo { return $this->morphTo(); }
}

Relation::morphMap(['post' => Post::class]);
Comment::whereIn('comments.subject_type', ["#,
    );
    for _ in 1..literal_count {
        source.push_str("'post', ");
    }
    let alias_start = source.len() + 1;
    source.push_str("'po']);\n");

    let before_alias = &source[..alias_start];
    let line = before_alias.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let character = before_alias.rsplit('\n').next().unwrap().len() as u32;
    let start = Position::new(line, character);
    let end = Position::new(line, character + 2);
    (source, end, Range::new(start, end))
}

fn assert_morph_alias_completion(response: Option<CompletionResponse>, expected_range: Range) {
    let items = match response.expect("morph column completion must return a response") {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert_eq!(items.len(), 1, "expected only the registered morph alias");
    assert_eq!(items[0].label, "post");
    assert_eq!(items[0].kind, Some(CompletionItemKind::ENUM_MEMBER));
    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("morph alias completion must replace the literal contents");
    };
    assert_eq!(edit.range, expected_range);
    assert_eq!(edit.new_text, "post");
}

fn bench_morph_column_completion(c: &mut Criterion) {
    let runtime = rt();
    let mut group = c.benchmark_group("laravel_morph_column_completion");

    for literal_count in [1, 128] {
        let backend = runtime.block_on(setup_laravel_backend());
        let (source, position, range) = generate_morph_column_source(literal_count);
        let uri = runtime.block_on(open_file(
            &backend,
            &format!("file:///bench/morph_columns_{literal_count}.php"),
            &source,
        ));
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::INVOKED,
                trigger_character: None,
            }),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let request = || {
            runtime
                .block_on(backend.completion(params.clone()))
                .expect("morph column completion request failed")
        };

        backend.update_ast(uri.as_str(), &source);
        backend.clear_completion_cache();
        assert_morph_alias_completion(request(), range);
        assert_morph_alias_completion(request(), range);

        group.bench_function(BenchmarkId::new("cold_confirmation", literal_count), |b| {
            // Setups mutate shared cache state, so each must precede exactly
            // one timed request instead of being grouped into larger batches.
            b.iter_batched(
                || {
                    backend.update_ast(uri.as_str(), &source);
                    backend.clear_completion_cache();
                },
                |()| black_box(request()),
                BatchSize::PerIteration,
            );
        });

        group.bench_function(BenchmarkId::new("cached_request", literal_count), |b| {
            assert_morph_alias_completion(request(), range);
            b.iter(|| black_box(request()));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_laravel_model_completion,
    bench_morph_column_completion
);
criterion_main!(benches);
