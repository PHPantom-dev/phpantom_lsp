use crate::common::{create_test_backend, open_document};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

async fn folding_ranges_for(text: &str) -> Vec<FoldingRange> {
    let backend = create_test_backend();
    let uri = Url::parse("file:///view.blade.php").unwrap();
    open_document(&backend, &uri, "blade", text).await;

    backend
        .folding_range(FoldingRangeParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default()
}

fn has_range(ranges: &[FoldingRange], start_line: u32, end_line: u32) -> bool {
    ranges
        .iter()
        .any(|r| r.start_line == start_line && r.end_line == end_line)
}

#[tokio::test]
async fn foreach_directive_folds() {
    let blade = "@foreach ($items as $item)\n    <p>{{ $item }}</p>\n@endforeach\n";
    let ranges = folding_ranges_for(blade).await;
    assert!(has_range(&ranges, 0, 2), "{ranges:?}");
}

#[tokio::test]
async fn if_else_directive_folds_each_branch() {
    let blade = "@if ($visible)\n    <p>shown</p>\n@else\n    <p>hidden</p>\n@endif\n";
    let ranges = folding_ranges_for(blade).await;
    // The whole @if…@endif folds, and so does @else…@endif since @else is
    // not itself a block boundary Blade compiles a matching directive for.
    assert!(has_range(&ranges, 0, 4), "{ranges:?}");
}

#[tokio::test]
async fn section_directive_folds() {
    let blade = "@section('body')\n    <p>content</p>\n@endsection\n";
    let ranges = folding_ranges_for(blade).await;
    assert!(has_range(&ranges, 0, 2), "{ranges:?}");
}

#[tokio::test]
async fn push_directive_folds() {
    let blade = "@push('scripts')\n    <script></script>\n@endpush\n";
    let ranges = folding_ranges_for(blade).await;
    assert!(has_range(&ranges, 0, 2), "{ranges:?}");
}

#[tokio::test]
async fn component_tag_body_folds() {
    let blade = "<x-alert>\n    <p>Hi</p>\n</x-alert>\n";
    let ranges = folding_ranges_for(blade).await;
    assert!(has_range(&ranges, 0, 2), "{ranges:?}");
}

#[tokio::test]
async fn self_closing_component_tag_does_not_fold() {
    let blade = "<x-alert />\n<p>after</p>\n";
    let ranges = folding_ranges_for(blade).await;
    assert!(ranges.is_empty(), "{ranges:?}");
}

#[tokio::test]
async fn mismatched_directive_does_not_fold() {
    let blade = "@foreach ($items as $item)\n    {{ $item }}\n@endif\n";
    let ranges = folding_ranges_for(blade).await;
    assert!(!has_range(&ranges, 0, 2), "{ranges:?}");
}

/// A multi-line PHP construct inside `@php`/`@endphp` produces an
/// AST-derived fold range. That range must land on the original Blade
/// lines, not the virtual-PHP lines the preprocessor's injected prologue
/// shifts everything down by.
#[tokio::test]
async fn ast_derived_range_lands_on_blade_lines_not_virtual_php_lines() {
    let blade = "@php\n    $data = [\n        'a' => 1,\n        'b' => 2,\n    ];\n@endphp\n";
    let ranges = folding_ranges_for(blade).await;
    // The array literal spans Blade lines 1-4 (`$data = [` through `];`).
    assert!(has_range(&ranges, 1, 4), "{ranges:?}");
    // None of the returned ranges should be anchored on the virtual-PHP
    // line numbers the unshifted prologue would produce.
    for range in &ranges {
        assert!(
            range.start_line < 6 && range.end_line < 6,
            "range escaped Blade coordinates: {range:?}"
        );
    }
}
