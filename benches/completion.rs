//! Performance benchmarks for PHPantom's core operations.
//!
//! Run with: `cargo bench`
//!
//! All benchmarks use **warm-start** measurement: backend creation and file
//! parsing happen before the measurement loop, so only the actual LSP
//! operation (completion, hover, go-to-definition) is timed.  This matches
//! Phpactor's `@BeforeMethods({"setUp"})` approach for direct comparison.
//!
//! A single `bench_cold_start` function measures the full pipeline from
//! backend creation through file parsing to a completed request, tracking
//! startup regression separately.
//!
//! The diagnostic benchmarks use the same fixture files as PHPactor's
//! `DiagnosticsBench` (from `lib/WorseReflection/Tests/Benchmarks/`) for
//! a direct 1:1 comparison.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Build a tokio runtime for async benchmarks.
fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Open a file on the backend.
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

/// Fire a completion request at the given position.
async fn fire_completion(backend: &Backend, uri: &Url, line: u32, character: u32) {
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let _ = black_box(backend.completion(params).await);
}

/// Fire a hover request at the given position.
async fn fire_hover(backend: &Backend, uri: &Url, line: u32, character: u32) {
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let _ = black_box(backend.hover(params).await);
}

/// Fire a go-to-definition request at the given position.
async fn fire_definition(backend: &Backend, uri: &Url, line: u32, character: u32) {
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let _ = black_box(backend.goto_definition(params).await);
}

// ─── PHP source generators ──────────────────────────────────────────────────

/// Generate a PHP file with a deep inheritance chain.
///
/// Creates `depth` classes where each extends the previous, each with
/// `methods_per_class` methods. The final line triggers `->` completion
/// on an instance of the deepest class.
fn generate_deep_inheritance(depth: usize, methods_per_class: usize) -> String {
    let mut src = String::from("<?php\n");

    for i in 0..depth {
        let class_name = format!("Level{i}");
        let extends = if i > 0 {
            format!(" extends Level{}", i - 1)
        } else {
            String::new()
        };
        src.push_str(&format!("class {class_name}{extends} {{\n"));
        for m in 0..methods_per_class {
            src.push_str(&format!(
                "    public function method_{i}_{m}(): void {{}}\n"
            ));
        }
        src.push_str("}\n");
    }

    let last = depth.saturating_sub(1);
    src.push_str(&format!("$obj = new Level{last}();\n"));
    src.push_str("$obj->\n");
    src
}

/// Generate a PHP file with many classes (simulating a large classmap).
fn generate_many_classes(count: usize) -> String {
    let mut src = String::from("<?php\n");
    for i in 0..count {
        src.push_str(&format!(
            "class Cls{i} {{ public function m{i}(): void {{}} }}\n"
        ));
    }
    src.push_str(&format!("$x = new Cls{}();\n", count.saturating_sub(1)));
    src.push_str("$x->\n");
    src
}

/// Generate a PHP file with generic classes, traits, and docblocks.
fn generate_complex_generics() -> String {
    r#"<?php
/** @template T */
class Collection {
    /** @return T */
    public function first() {}
    /** @return T[] */
    public function all(): array {}
    public function count(): int {}
    public function isEmpty(): bool {}
}

class Product {
    public function getName(): string {}
    public function getPrice(): float {}
    public function getCategory(): string {}
}

/** @extends Collection<Product> */
class ProductCollection extends Collection {
    public function cheapest(): Product {}
}

trait HasTimestamps {
    public function getCreatedAt(): string {}
    public function getUpdatedAt(): string {}
}

/** @mixin HasTimestamps */
class Audit {
    public function getAuditor(): string {}
}

/**
 * @method string getLabel()
 * @property int $priority
 */
class Task {
    public function complete(): void {}
}

$pc = new ProductCollection();
$pc->first()->
"#
    .to_string()
}

/// Generate a PHP file with narrowing and control flow.
fn generate_narrowing_scenario() -> String {
    r#"<?php
class Dog {
    public function bark(): void {}
    public function fetch(): void {}
}
class Cat {
    public function meow(): void {}
    public function purr(): void {}
}
class Fish {
    public function swim(): void {}
}

function handle(Dog|Cat|Fish $animal): void {
    if ($animal instanceof Fish) {
        return;
    }
    if ($animal instanceof Dog) {
        $animal->
    }
}
"#
    .to_string()
}

/// Generate a large PHP file (~500 lines) with mixed content.
fn generate_large_file(line_count: usize) -> String {
    let mut src = String::from("<?php\n");
    let classes_needed = line_count / 10;
    for i in 0..classes_needed {
        src.push_str(&format!("class Big{i} {{\n"));
        src.push_str(&format!("    public string $prop{i};\n"));
        src.push_str(&format!(
            "    /** @return Big{i} */\n    public function self{i}(): self {{}}\n"
        ));
        src.push_str(&format!(
            "    public function method{i}a(int $x): void {{}}\n"
        ));
        src.push_str(&format!(
            "    public function method{i}b(string $s): string {{}}\n"
        ));
        src.push_str(&format!("    private function internal{i}(): void {{}}\n"));
        src.push_str("}\n\n");
    }
    src
}

/// Generate a PHP class body with `count` local variables, ending with a
/// bare `$` to trigger variable-name completion.
fn generate_many_variables(count: usize) -> String {
    let mut src =
        String::from("<?php\nclass LongVarExample {\n    public function doWork(): void {\n");
    for i in 0..count {
        let name = format!("var_{i}");
        let value = match i % 6 {
            0 => "1".to_string(),
            1 => "\"hello\"".to_string(),
            2 => "new \\stdClass()".to_string(),
            3 => "[]".to_string(),
            4 => "true".to_string(),
            5 => format!("{}.{}", i, i * 2),
            _ => unreachable!(),
        };
        src.push_str(&format!("        ${name} = {value};\n"));
    }
    src.push_str("        $\n    }\n}\n");
    src
}

// ─── Cold-start benchmark ──────────────────────────────────────────────────
//
// Measures the full pipeline from `Backend::new_headless()` (which builds
// the stub indices) through file parsing to a completed request that
// triggers stub resolution (via `\DateTime`).  All other benchmarks are
// warm-start, so the difference between cold and warm numbers gives you
// the startup cost implicitly.

const COLD_START_SOURCE: &str = r#"<?php
class Greeter {
    public function hello(): \DateTime {}
}
$g = new Greeter();
$g->hello()->
"#;

const SIMPLE_CLASS_SOURCE: &str = r#"<?php
class Greeter {
    public function hello(): string {}
    public function goodbye(): string {}
}
$g = new Greeter();
$g->
"#;

fn bench_cold_start(c: &mut Criterion) {
    let runtime = rt();

    c.bench_function("cold_start_completion", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let backend = Backend::new_headless();
                let uri = open_file(&backend, "file:///cold_start.php", COLD_START_SOURCE).await;
                fire_completion(&backend, &uri, 5, 14).await;
            })
        })
    });
}

// ─── Completion benchmarks ─────────────────────────────────────────────────

fn bench_completion_simple(c: &mut Criterion) {
    let runtime = rt();
    let backend = Backend::new_test();
    let uri = runtime.block_on(open_file(
        &backend,
        "file:///bench_simple.php",
        SIMPLE_CLASS_SOURCE,
    ));

    c.bench_function("completion_simple_class", |b| {
        b.iter(|| runtime.block_on(fire_completion(&backend, &uri, 5, 4)))
    });
}

fn bench_completion_deep_inheritance(c: &mut Criterion) {
    let runtime = rt();
    let source_5 = generate_deep_inheritance(5, 3);
    let source_10 = generate_deep_inheritance(10, 3);
    let source_20 = generate_deep_inheritance(20, 3);

    let mut group = c.benchmark_group("completion_inheritance_depth");

    {
        let backend = Backend::new_test();
        let uri = runtime.block_on(open_file(&backend, "file:///bench_depth5.php", &source_5));
        let line = source_5.lines().count() as u32 - 1;
        group.bench_function("depth_5", |b| {
            b.iter(|| runtime.block_on(fire_completion(&backend, &uri, line, 6)))
        });
    }

    {
        let backend = Backend::new_test();
        let uri = runtime.block_on(open_file(&backend, "file:///bench_depth10.php", &source_10));
        let line = source_10.lines().count() as u32 - 1;
        group.bench_function("depth_10", |b| {
            b.iter(|| runtime.block_on(fire_completion(&backend, &uri, line, 6)))
        });
    }

    {
        let backend = Backend::new_test();
        let uri = runtime.block_on(open_file(&backend, "file:///bench_depth20.php", &source_20));
        let line = source_20.lines().count() as u32 - 1;
        group.bench_function("depth_20", |b| {
            b.iter(|| runtime.block_on(fire_completion(&backend, &uri, line, 6)))
        });
    }

    group.finish();
}

fn bench_completion_many_classes(c: &mut Criterion) {
    let runtime = rt();
    let source_100 = generate_many_classes(100);
    let source_500 = generate_many_classes(500);
    let source_1000 = generate_many_classes(1000);

    let mut group = c.benchmark_group("completion_classmap_size");

    {
        let backend = Backend::new_test();
        let uri = runtime.block_on(open_file(&backend, "file:///bench_cls100.php", &source_100));
        let line = source_100.lines().count() as u32 - 1;
        group.bench_function("100_classes", |b| {
            b.iter(|| runtime.block_on(fire_completion(&backend, &uri, line, 4)))
        });
    }

    {
        let backend = Backend::new_test();
        let uri = runtime.block_on(open_file(&backend, "file:///bench_cls500.php", &source_500));
        let line = source_500.lines().count() as u32 - 1;
        group.bench_function("500_classes", |b| {
            b.iter(|| runtime.block_on(fire_completion(&backend, &uri, line, 4)))
        });
    }

    {
        let backend = Backend::new_test();
        let uri = runtime.block_on(open_file(
            &backend,
            "file:///bench_cls1000.php",
            &source_1000,
        ));
        let line = source_1000.lines().count() as u32 - 1;
        group.bench_function("1000_classes", |b| {
            b.iter(|| runtime.block_on(fire_completion(&backend, &uri, line, 4)))
        });
    }

    group.finish();
}

fn bench_completion_generics(c: &mut Criterion) {
    let runtime = rt();
    let source = generate_complex_generics();
    let backend = Backend::new_test();
    let uri = runtime.block_on(open_file(&backend, "file:///bench_generics.php", &source));
    let line = source.lines().count() as u32 - 1;

    c.bench_function("completion_generics_and_mixins", |b| {
        b.iter(|| runtime.block_on(fire_completion(&backend, &uri, line, 14)))
    });
}

fn bench_completion_narrowing(c: &mut Criterion) {
    let runtime = rt();
    let source = generate_narrowing_scenario();
    let backend = Backend::new_test();
    let uri = runtime.block_on(open_file(&backend, "file:///bench_narrow.php", &source));

    c.bench_function("completion_with_narrowing", |b| {
        // The cursor is on the `$animal->` line inside the Dog branch
        b.iter(|| runtime.block_on(fire_completion(&backend, &uri, 19, 18)))
    });
}

fn bench_completion_chained_methods(c: &mut Criterion) {
    let runtime = rt();
    let source = r#"<?php
class Builder {
    public function a(): self {}
    public function b(): self {}
    public function c(): self {}
    public function d(): self {}
    public function e(): self {}
    public function finish(): string {}
}
$b = new Builder();
$b->a()->b()->c()->d()->e()->
"#;
    let backend = Backend::new_test();
    let uri = runtime.block_on(open_file(&backend, "file:///bench_chain.php", source));

    c.bench_function("completion_5_method_chain", |b| {
        b.iter(|| runtime.block_on(fire_completion(&backend, &uri, 11, 30)))
    });
}

fn bench_cross_file_completion(c: &mut Criterion) {
    let runtime = rt();
    let backend = Backend::new_test();
    runtime.block_on(open_file(
        &backend,
        "file:///bench_dep.php",
        r#"<?php
class Dependency {
    public function resolve(): string {}
    public function bind(): void {}
    public function singleton(): void {}
}
"#,
    ));
    let uri = runtime.block_on(open_file(
        &backend,
        "file:///bench_consumer.php",
        r#"<?php
class Consumer {
    public function work(Dependency $dep): void {
        $dep->
    }
}
"#,
    ));

    c.bench_function("completion_cross_file_type_hint", |b| {
        b.iter(|| runtime.block_on(fire_completion(&backend, &uri, 3, 14)))
    });
}

// ─── PHPactor reflection benchmarks ────────────────────────────────────────
//
// These use the same fixture files as PHPactor's reflection benchmarks
// (`lib/WorseReflection/Tests/Benchmarks/fixtures/`), ported to
// `benches/fixtures/`.  They exercise completion on large, real-world
// classes to measure type resolution and member enumeration performance.

/// Carbon: a single 839-line class with ~80 `@property` annotations and
/// many `@method` entries.  PHPactor's `CarbonReflectBench` reflects this
/// class and iterates its methods.  We trigger completion on an instance.
fn bench_completion_carbon(c: &mut Criterion) {
    let runtime = rt();
    let carbon_src = std::fs::read_to_string("benches/fixtures/reflection/carbon.php")
        .expect("carbon.php fixture missing");
    let wrapper = format!("{}\n$c = new \\Carbon\\Carbon();\n$c->\n", carbon_src);
    let cursor_line = wrapper.lines().count() as u32 - 2;

    let backend = Backend::new_test();
    let uri = runtime.block_on(open_file(&backend, "file:///bench_carbon.php", &wrapper));

    c.bench_function("completion_carbon_class", |b| {
        b.iter(|| runtime.block_on(fire_completion(&backend, &uri, cursor_line, 4)))
    });
}

/// Yii ActiveRecord hierarchy: 10 files, ~5 500 lines total, deep
/// inheritance chain with traits, interfaces, and `@property` annotations.
/// PHPactor's `YiiBench` reflects the leaf `Record` class and iterates
/// all members calling `inferredType()`.  We open all files and trigger
/// completion on a `Record` instance.
fn bench_completion_yii_hierarchy(c: &mut Criterion) {
    let runtime = rt();

    let yii_files: Vec<(&str, String)> = [
        "BaseObject",
        "Component",
        "Model",
        "BaseActiveRecord",
        "ActiveRecord",
        "Record",
        "ActiveRecordInterface",
        "Arrayable",
        "ArrayableTrait",
        "StaticInstanceInterface",
        "StaticInstanceTrait",
    ]
    .iter()
    .map(|name| {
        let path = format!("benches/fixtures/yii/{name}.php");
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let uri_name: &str = name;
        (uri_name, content)
    })
    .collect();

    let trigger = r#"<?php
namespace Phpactor\WorseReflection\Tests\Workspace;
$r = new Record();
$r->
"#;

    let backend = Backend::new_test();
    runtime.block_on(async {
        for (name, content) in &yii_files {
            let uri = format!("file:///yii/{name}.php");
            open_file(&backend, &uri, content).await;
        }
    });
    let uri = runtime.block_on(open_file(&backend, "file:///yii/trigger.php", trigger));

    c.bench_function("completion_yii_deep_hierarchy", |b| {
        b.iter(|| runtime.block_on(fire_completion(&backend, &uri, 3, 4)))
    });
}

/// Large-file completion: PHPactor's `Example1.php.test` — a 213-line
/// class with 19 use-imports, 7 methods, and many local variables.
/// Completion is triggered on an untyped `$foobar->` deep inside the class.
/// This measures how completion scales with file complexity.
///
/// Directly comparable to Phpactor's `ClassMemberCompletorBench` (long).
fn bench_completion_large_file(c: &mut Criterion) {
    let runtime = rt();
    let content = std::fs::read_to_string("benches/fixtures/completion/example1_long.php")
        .expect("example1_long.php fixture missing");

    let backend = Backend::new_test();
    let uri = runtime.block_on(open_file(&backend, "file:///bench_large.php", &content));

    // Cursor is on line 207 (0-based: 206), at `$foobar->` (col 24).
    c.bench_function("completion_large_file", |b| {
        b.iter(|| runtime.block_on(fire_completion(&backend, &uri, 206, 24)))
    });
}

/// Short-file completion: Phpactor's `Short.php.test` adapted to valid PHP.
/// A tiny file with one class and a single `$foobar->` trigger.
/// This measures baseline completion latency with minimal file complexity.
///
/// Directly comparable to Phpactor's `ClassMemberCompletorBench` (short).
fn bench_completion_short_file(c: &mut Criterion) {
    let runtime = rt();
    let content = std::fs::read_to_string("benches/fixtures/completion/short.php")
        .expect("short.php fixture missing");

    let backend = Backend::new_test();
    let uri = runtime.block_on(open_file(&backend, "file:///bench_short.php", &content));

    // Cursor is on line 8 (0-based), at `$foobar->` (col 9).
    c.bench_function("completion_short_file", |b| {
        b.iter(|| runtime.block_on(fire_completion(&backend, &uri, 8, 9)))
    });
}

// ─── Variable completion benchmarks ────────────────────────────────────────
//
// These trigger completion on a bare `$` (variable-name completion) rather
// than `$var->` (member completion).  Directly comparable to Phpactor's
// `WorseLocalVariableCompletorBench` (short and long variants).

fn bench_variable_completion(c: &mut Criterion) {
    let runtime = rt();
    let mut group = c.benchmark_group("variable_completion");

    // Short: 4 lines, 3 variables.
    {
        let source = "<?php\n$alpha = 1;\n$beta = \"hello\";\n$gamma = new stdClass();\n$\n";
        let backend = Backend::new_test();
        let uri = runtime.block_on(open_file(&backend, "file:///bench_var_short.php", source));
        group.bench_function("short", |b| {
            // Cursor on line 4, col 1 (right after `$`).
            b.iter(|| runtime.block_on(fire_completion(&backend, &uri, 4, 1)))
        });
    }

    // Long: class with 50 variables then a bare `$`.
    {
        let long_src = generate_many_variables(50);
        // Cursor line: 3 header lines + 50 variable lines = line 53, col 9.
        let cursor_line = 3 + 50;
        let backend = Backend::new_test();
        let uri = runtime.block_on(open_file(&backend, "file:///bench_var_long.php", &long_src));
        group.bench_function("long", |b| {
            b.iter(|| runtime.block_on(fire_completion(&backend, &uri, cursor_line, 9)))
        });
    }

    group.finish();
}

// ─── Hover & go-to-definition benchmarks ───────────────────────────────────

fn bench_hover(c: &mut Criterion) {
    let runtime = rt();
    let source = r#"<?php
class HoverTarget {
    /**
     * Compute a result from the input.
     * @param string $input The raw input
     * @return int The computed value
     */
    public function compute(string $input): int {}
}
$ht = new HoverTarget();
$ht->compute('test');
"#;
    let backend = Backend::new_test();
    let uri = runtime.block_on(open_file(&backend, "file:///bench_hover.php", source));

    c.bench_function("hover_method_call", |b| {
        b.iter(|| runtime.block_on(fire_hover(&backend, &uri, 10, 7)))
    });
}

fn bench_definition(c: &mut Criterion) {
    let runtime = rt();
    let source = r#"<?php
class DefTarget {
    public function action(): void {}
}
$dt = new DefTarget();
$dt->action();
"#;
    let backend = Backend::new_test();
    let uri = runtime.block_on(open_file(&backend, "file:///bench_def.php", source));

    c.bench_function("goto_definition_method", |b| {
        b.iter(|| runtime.block_on(fire_definition(&backend, &uri, 5, 7)))
    });
}

// ─── Parse & reparse benchmarks ────────────────────────────────────────────

fn bench_update_ast(c: &mut Criterion) {
    let source_small = generate_large_file(100);
    let source_medium = generate_large_file(500);
    let source_large = generate_large_file(2000);

    let mut group = c.benchmark_group("update_ast_parse_time");

    {
        let backend = Backend::new_test();
        group.bench_function("100_lines", |b| {
            b.iter(|| backend.update_ast("file:///bench_parse_100.php", black_box(&source_small)))
        });
    }

    {
        let backend = Backend::new_test();
        group.bench_function("500_lines", |b| {
            b.iter(|| backend.update_ast("file:///bench_parse_500.php", black_box(&source_medium)))
        });
    }

    {
        let backend = Backend::new_test();
        group.bench_function("2000_lines", |b| {
            b.iter(|| backend.update_ast("file:///bench_parse_2000.php", black_box(&source_large)))
        });
    }

    group.finish();
}

fn bench_reparse_after_edit(c: &mut Criterion) {
    let runtime = rt();
    let initial_source = generate_large_file(500);

    let backend = Backend::new_test();
    let uri = Url::parse("file:///bench_reparse.php").unwrap();
    runtime.block_on(async {
        let open_params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: initial_source.clone(),
            },
        };
        backend.did_open(open_params).await;
    });

    let mut edited = initial_source;
    edited.push_str("class ExtraClass { public function extra(): void {} }\n");
    let mut version = 2;

    c.bench_function("reparse_500_line_file", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let change_params = DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: edited.clone(),
                    }],
                };
                backend.did_change(change_params).await;
            });
            version += 1;
        })
    });
}

// ─── Diagnostic benchmarks ─────────────────────────────────────────────────
//
// These use the exact same fixture files as PHPactor's `DiagnosticsBench`
// (`lib/WorseReflection/Tests/Benchmarks/fixtures/diagnostics/*.test`),
// copied to `benches/fixtures/diagnostics/*.php`.  PHPactor runs its
// `MissingMemberProvider` (missing member detection) on each fixture; we
// run all four of our diagnostic providers (deprecated, unused imports,
// unknown classes, unknown members).

/// PHPactor diagnostic fixture files used for benchmarking.
/// Each file exercises class resolution (unknown-class provider) and/or
/// member resolution (unknown-member provider).
const DIAGNOSTIC_FIXTURES: &[&str] = &[
    "lots_of_new_generic_objects", // 66 lines — repeated `new` of a @template class
    "lots_of_new_objects",         // 62 lines — repeated `new` of a plain class
    "lots_of_missing_methods",     // ~1175 lines — many unresolved member accesses
    "method_chain",                // fluent chain of ->bang() calls
                                   // "phpstan" is excluded: ~5 000 lines with hundreds of unresolvable
                                   // vendor class refs that dominate the benchmark without exercising
                                   // our member-level diagnostics meaningfully.
];

fn bench_diagnostics_phpactor_fixtures(c: &mut Criterion) {
    let mut group = c.benchmark_group("diagnostics");
    for name in DIAGNOSTIC_FIXTURES {
        let path = format!("benches/fixtures/diagnostics/{name}.php");
        let content =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
        let uri = format!("file:///bench/{name}.php");

        group.bench_with_input(
            BenchmarkId::new("fixture", *name),
            &content,
            |b, content| {
                let backend = Backend::new_test_with_full_stubs();
                backend.update_ast(&uri, content);
                b.iter(|| {
                    let mut out = Vec::new();
                    backend.collect_slow_diagnostics(&uri, black_box(content), &mut out);
                    black_box(out)
                })
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_cold_start,
    bench_completion_simple,
    bench_completion_deep_inheritance,
    bench_completion_many_classes,
    bench_completion_generics,
    bench_completion_narrowing,
    bench_completion_chained_methods,
    bench_cross_file_completion,
    bench_completion_carbon,
    bench_completion_yii_hierarchy,
    bench_completion_large_file,
    bench_completion_short_file,
    bench_variable_completion,
    bench_hover,
    bench_definition,
    bench_update_ast,
    bench_reparse_after_edit,
    bench_diagnostics_phpactor_fixtures,
);
criterion_main!(benches);
