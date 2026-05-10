//! Performance benchmarks for Find References.
//!
//! Run with: `cargo bench --bench references`

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

/// Generate a large project with `num_classes` classes.
/// Half of them extend a base class.
fn generate_large_project(num_classes: usize) -> Vec<(String, String)> {
    let mut files = Vec::new();

    // Base class
    files.push((
        "file:///Base.php".to_string(),
        "<?php class Base { public function targetMethod() {} }".to_string(),
    ));

    // Many descendants
    for i in 0..num_classes {
        let content = if i % 2 == 0 {
            format!(
                "<?php class Class{} extends Base {{ public function other() {{ $this->targetMethod(); }} }}",
                i
            )
        } else {
            format!("<?php class Class{} {{ public function other() {{ }} }}", i)
        };
        files.push((format!("file:///Class{}.php", i), content));
    }

    files
}

fn bench_references(c: &mut Criterion) {
    let mut group = c.benchmark_group("find_references");

    for size in [50, 150].iter() {
        group.bench_with_input(
            BenchmarkId::new("hierarchy_scan", size),
            size,
            |b, &size| {
                let backend = Backend::new_headless();
                let files = generate_large_project(size);

                // Initial indexing (warm up)
                for (uri, content) in &files {
                    backend.update_ast(uri, content);
                }

                let base_content = "<?php class Base { public function targetMethod() {} }";
                let pos = Position {
                    line: 0,
                    character: 40,
                }; // On 'targetMethod'

                b.iter(|| {
                    let _ = black_box(backend.find_references(
                        "file:///Base.php",
                        base_content,
                        pos,
                        false,
                    ));
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_references);
criterion_main!(benches);
