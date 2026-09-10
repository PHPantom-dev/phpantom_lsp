use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

fn get_folding_ranges(php: &str) -> Vec<FoldingRange> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    backend.handle_folding_range(uri, php).unwrap_or_default()
}

fn has_range(ranges: &[FoldingRange], start_line: u32, end_line: u32) -> bool {
    ranges
        .iter()
        .any(|r| r.start_line == start_line && r.end_line == end_line)
}

fn has_comment_range(ranges: &[FoldingRange], start_line: u32, end_line: u32) -> bool {
    ranges.iter().any(|r| {
        r.start_line == start_line
            && r.end_line == end_line
            && r.kind == Some(FoldingRangeKind::Comment)
    })
}

// ─── Basic cases ────────────────────────────────────────────────────────────

#[test]
fn empty_file_returns_empty() {
    let ranges = get_folding_ranges("<?php\n");
    assert!(ranges.is_empty());
}

#[test]
fn class_body_produces_range() {
    let php = r#"<?php
class Foo {
    public $bar;
    public $baz;
}
"#;
    let ranges = get_folding_ranges(php);
    // class body { on line 1, } on line 4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected class body range (1..4), got: {ranges:?}"
    );
}

#[test]
fn method_body_produces_range() {
    let php = r#"<?php
class Foo {
    public function bar() {
        return 1;
    }
}
"#;
    let ranges = get_folding_ranges(php);
    // method body { on line 2, } on line 4
    assert!(
        has_range(&ranges, 2, 4),
        "Expected method body range (2..4), got: {ranges:?}"
    );
}

#[test]
fn function_body_produces_range() {
    let php = r#"<?php
function hello() {
    echo "hello";
    echo "world";
}
"#;
    let ranges = get_folding_ranges(php);
    // function body { on line 1, } on line 4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected function body range (1..4), got: {ranges:?}"
    );
}

#[test]
fn nested_class_and_method_produce_two_ranges() {
    let php = r#"<?php
class Outer {
    public function inner() {
        return 42;
    }
}
"#;
    let ranges = get_folding_ranges(php);
    // class body: line 1..5
    assert!(
        has_range(&ranges, 1, 5),
        "Expected class body range (1..5), got: {ranges:?}"
    );
    // method body: line 2..4
    assert!(
        has_range(&ranges, 2, 4),
        "Expected method body range (2..4), got: {ranges:?}"
    );
}

// ─── Comments ───────────────────────────────────────────────────────────────

#[test]
fn doc_comment_produces_comment_range() {
    let php = r#"<?php
/**
 * This is a doc comment
 * spanning multiple lines.
 */
function foo() {}
"#;
    let ranges = get_folding_ranges(php);
    // doc comment starts line 1, ends line 4
    assert!(
        has_comment_range(&ranges, 1, 4),
        "Expected doc comment range (1..4), got: {ranges:?}"
    );
}

#[test]
fn consecutive_single_line_comments_produce_comment_range() {
    let php = r#"<?php
// line one
// line two
// line three
function foo() {}
"#;
    let ranges = get_folding_ranges(php);
    // Three consecutive // comments on lines 1, 2, 3
    assert!(
        has_comment_range(&ranges, 1, 3),
        "Expected consecutive comment range (1..3), got: {ranges:?}"
    );
}

#[test]
fn multi_line_block_comment_produces_comment_range() {
    let php = r#"<?php
/* This is
   a block
   comment */
function foo() {}
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_comment_range(&ranges, 1, 3),
        "Expected block comment range (1..3), got: {ranges:?}"
    );
}

// ─── Control flow ───────────────────────────────────────────────────────────

#[test]
fn if_else_blocks_produce_ranges() {
    let php = r#"<?php
if ($x) {
    echo "a";
} else {
    echo "b";
}
"#;
    let ranges = get_folding_ranges(php);
    // if block { on line 1, } on line 3
    assert!(
        has_range(&ranges, 1, 3),
        "Expected if block range (1..3), got: {ranges:?}"
    );
    // else block { on line 3, } on line 5
    assert!(
        has_range(&ranges, 3, 5),
        "Expected else block range (3..5), got: {ranges:?}"
    );
}

#[test]
fn switch_body_produces_range() {
    let php = r#"<?php
switch ($x) {
    case 1:
        break;
    default:
        break;
}
"#;
    let ranges = get_folding_ranges(php);
    // switch body { on line 1, } on line 6
    assert!(
        has_range(&ranges, 1, 6),
        "Expected switch body range (1..6), got: {ranges:?}"
    );
}

#[test]
fn try_catch_finally_produce_ranges() {
    let php = r#"<?php
try {
    foo();
} catch (Exception $e) {
    bar();
} finally {
    baz();
}
"#;
    let ranges = get_folding_ranges(php);
    // try block: line 1..3
    assert!(
        has_range(&ranges, 1, 3),
        "Expected try block range (1..3), got: {ranges:?}"
    );
    // catch block: line 3..5
    assert!(
        has_range(&ranges, 3, 5),
        "Expected catch block range (3..5), got: {ranges:?}"
    );
    // finally block: line 5..7
    assert!(
        has_range(&ranges, 5, 7),
        "Expected finally block range (5..7), got: {ranges:?}"
    );
}

// ─── Loops ──────────────────────────────────────────────────────────────────

#[test]
fn for_loop_body_produces_range() {
    let php = r#"<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 3),
        "Expected for loop body range (1..3), got: {ranges:?}"
    );
}

#[test]
fn foreach_loop_body_produces_range() {
    let php = r#"<?php
foreach ($items as $item) {
    echo $item;
}
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 3),
        "Expected foreach loop body range (1..3), got: {ranges:?}"
    );
}

#[test]
fn while_loop_body_produces_range() {
    let php = r#"<?php
while ($x > 0) {
    $x--;
}
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 3),
        "Expected while loop body range (1..3), got: {ranges:?}"
    );
}

#[test]
fn do_while_body_produces_range() {
    let php = r#"<?php
do {
    $x++;
} while ($x < 10);
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 3),
        "Expected do-while body range (1..3), got: {ranges:?}"
    );
}

// ─── Arrays ─────────────────────────────────────────────────────────────────

#[test]
fn multi_line_array_produces_range() {
    let php = r#"<?php
$arr = [
    'a',
    'b',
    'c',
];
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 5),
        "Expected array range (1..5), got: {ranges:?}"
    );
}

#[test]
fn single_line_array_no_range() {
    let php = "<?php\n$arr = ['a', 'b', 'c'];\n";
    let ranges = get_folding_ranges(php);
    // The array is single-line, so no folding range should be emitted for it.
    // There might still be no ranges at all.
    for r in &ranges {
        // Any range that exists should not be single-line and not be an
        // array on line 1.
        assert!(
            r.start_line != r.end_line,
            "Single-line range should have been filtered: {r:?}"
        );
    }
}

// ─── Enums ──────────────────────────────────────────────────────────────────

#[test]
fn enum_body_produces_range() {
    let php = r#"<?php
enum Color {
    case Red;
    case Green;
    case Blue;
}
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 5),
        "Expected enum body range (1..5), got: {ranges:?}"
    );
}

#[test]
fn backed_enum_produces_range() {
    let php = r#"<?php
enum Status: string {
    case Active = 'active';
    case Inactive = 'inactive';
}
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 4),
        "Expected backed enum body range (1..4), got: {ranges:?}"
    );
}

// ─── Interfaces and traits ──────────────────────────────────────────────────

#[test]
fn interface_body_produces_range() {
    let php = r#"<?php
interface Foo {
    public function bar(): void;
    public function baz(): int;
}
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 4),
        "Expected interface body range (1..4), got: {ranges:?}"
    );
}

#[test]
fn trait_body_produces_range() {
    let php = r#"<?php
trait Foo {
    public function bar() {
        return 1;
    }
}
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 5),
        "Expected trait body range (1..5), got: {ranges:?}"
    );
}

// ─── Closures ───────────────────────────────────────────────────────────────

#[test]
fn multi_line_closure_produces_range() {
    let php = r#"<?php
$fn = function ($x) {
    return $x + 1;
};
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 3),
        "Expected closure body range (1..3), got: {ranges:?}"
    );
}

#[test]
fn single_line_closure_no_range() {
    let php = "<?php\n$fn = function ($x) { return $x + 1; };\n";
    let ranges = get_folding_ranges(php);
    for r in &ranges {
        assert!(
            r.start_line != r.end_line,
            "Single-line range should have been filtered: {r:?}"
        );
    }
}

// ─── Match expression ───────────────────────────────────────────────────────

#[test]
fn match_expression_produces_range() {
    let php = r#"<?php
$result = match ($x) {
    1 => 'one',
    2 => 'two',
    default => 'other',
};
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 5),
        "Expected match expression range (1..5), got: {ranges:?}"
    );
}

// ─── Argument and parameter lists ───────────────────────────────────────────

#[test]
fn multi_line_argument_list_produces_range() {
    let php = r#"<?php
foo(
    $a,
    $b,
    $c
);
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 5),
        "Expected argument list range (1..5), got: {ranges:?}"
    );
}

#[test]
fn multi_line_parameter_list_produces_range() {
    let php = r#"<?php
function foo(
    int $a,
    string $b,
    bool $c
) {
    return $a;
}
"#;
    let ranges = get_folding_ranges(php);
    // Parameter list: line 1..5
    assert!(
        has_range(&ranges, 1, 5),
        "Expected parameter list range (1..5), got: {ranges:?}"
    );
    // Function body: line 5..7
    assert!(
        has_range(&ranges, 5, 7),
        "Expected function body range (5..7), got: {ranges:?}"
    );
}

// ─── Single-line constructs ─────────────────────────────────────────────────

#[test]
fn single_line_constructs_produce_no_range() {
    let php = "<?php\nif (true) { echo 1; }\n";
    let ranges = get_folding_ranges(php);
    for r in &ranges {
        assert!(
            r.start_line != r.end_line,
            "Single-line range should have been filtered: {r:?}"
        );
    }
}

// ─── Namespace ──────────────────────────────────────────────────────────────

#[test]
fn brace_delimited_namespace_produces_range() {
    let php = r#"<?php
namespace App {
    class Foo {
    }
}
"#;
    let ranges = get_folding_ranges(php);
    // namespace body: line 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected namespace body range (1..4), got: {ranges:?}"
    );
}

// ─── Sorting and deduplication ──────────────────────────────────────────────

#[test]
fn ranges_are_sorted_by_start_line() {
    let php = r#"<?php
function a() {
    return 1;
}
function b() {
    return 2;
}
"#;
    let ranges = get_folding_ranges(php);
    for w in ranges.windows(2) {
        assert!(
            w[0].start_line <= w[1].start_line,
            "Ranges not sorted: {:?} came before {:?}",
            w[0],
            w[1]
        );
    }
}

// ─── Complex nesting ────────────────────────────────────────────────────────

#[test]
fn complex_nesting_produces_all_expected_ranges() {
    let php = r#"<?php
class Service {
    /**
     * Handle the request.
     */
    public function handle() {
        if ($this->check()) {
            foreach ($this->items() as $item) {
                try {
                    $item->process();
                } catch (\Exception $e) {
                    log($e);
                }
            }
        }
    }
}
"#;
    let ranges = get_folding_ranges(php);
    // class body
    assert!(has_range(&ranges, 1, 16), "class body: {ranges:?}");
    // doc comment
    assert!(has_comment_range(&ranges, 2, 4), "doc comment: {ranges:?}");
    // method body
    assert!(has_range(&ranges, 5, 15), "method body: {ranges:?}");
    // if block
    assert!(has_range(&ranges, 6, 14), "if block: {ranges:?}");
    // foreach block
    assert!(has_range(&ranges, 7, 13), "foreach block: {ranges:?}");
    // try block
    assert!(has_range(&ranges, 8, 10), "try block: {ranges:?}");
    // catch block
    assert!(has_range(&ranges, 10, 12), "catch block: {ranges:?}");
}

// ─── Anonymous class ────────────────────────────────────────────────────────

#[test]
fn anonymous_class_body_produces_range() {
    let php = r#"<?php
$obj = new class {
    public function foo() {
        return 1;
    }
};
"#;
    let ranges = get_folding_ranges(php);
    // anonymous class body: line 1..5
    assert!(
        has_range(&ranges, 1, 5),
        "Expected anonymous class body range (1..5), got: {ranges:?}"
    );
    // method body: line 2..4
    assert!(
        has_range(&ranges, 2, 4),
        "Expected method body range (2..4), got: {ranges:?}"
    );
}

// ─── Legacy array ───────────────────────────────────────────────────────────

#[test]
fn multi_line_legacy_array_produces_range() {
    let php = r#"<?php
$arr = array(
    'a',
    'b',
    'c',
);
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 5),
        "Expected legacy array range (1..5), got: {ranges:?}"
    );
}

// ─── Arrow function ─────────────────────────────────────────────────────────

#[test]
fn arrow_function_spanning_multiple_lines() {
    let php = r#"<?php
$fn = fn($x) =>
    $x + 1;
"#;
    let ranges = get_folding_ranges(php);
    // Arrow function spans from line 1 (fn) to line 2 ($x + 1)
    assert!(
        has_range(&ranges, 1, 2),
        "Expected arrow function range (1..2), got: {ranges:?}"
    );
}

// ─── Elseif ─────────────────────────────────────────────────────────────────

#[test]
fn elseif_blocks_produce_ranges() {
    let php = r#"<?php
if ($x) {
    echo "a";
} elseif ($y) {
    echo "b";
} else {
    echo "c";
}
"#;
    let ranges = get_folding_ranges(php);
    // if block: { on line 1, } on line 3
    assert!(
        has_range(&ranges, 1, 3),
        "Expected if block range (1..3), got: {ranges:?}"
    );
    // elseif block: { on line 3, } on line 5
    assert!(
        has_range(&ranges, 3, 5),
        "Expected elseif block range (3..5), got: {ranges:?}"
    );
    // else block: { on line 5, } on line 7
    assert!(
        has_range(&ranges, 5, 7),
        "Expected else block range (5..7), got: {ranges:?}"
    );
}

// ─── Colon-delimited if/elseif/else ─────────────────────────────────────────

#[test]
fn colon_delimited_if_elseif_else() {
    let php = r#"<?php
if ($x):
    $a = [
        1,
        2,
    ];
elseif ($y):
    $b = [
        3,
        4,
    ];
else:
    $c = [
        5,
        6,
    ];
endif;
"#;
    let ranges = get_folding_ranges(php);
    // Array in if body: lines 2..5
    assert!(
        has_range(&ranges, 2, 5),
        "Expected array in if body range (2..5), got: {ranges:?}"
    );
    // Array in elseif body: lines 7..10
    assert!(
        has_range(&ranges, 7, 10),
        "Expected array in elseif body range (7..10), got: {ranges:?}"
    );
    // Array in else body: lines 12..15
    assert!(
        has_range(&ranges, 12, 15),
        "Expected array in else body range (12..15), got: {ranges:?}"
    );
}

// ─── Colon-delimited while ──────────────────────────────────────────────────

#[test]
fn colon_delimited_while() {
    let php = r#"<?php
while ($x > 0):
    $arr = [
        1,
        2,
    ];
endwhile;
"#;
    let ranges = get_folding_ranges(php);
    // Array inside colon-delimited while: lines 2..5
    assert!(
        has_range(&ranges, 2, 5),
        "Expected array in while body range (2..5), got: {ranges:?}"
    );
}

// ─── Colon-delimited for ────────────────────────────────────────────────────

#[test]
fn colon_delimited_for() {
    let php = r#"<?php
for ($i = 0; $i < 10; $i++):
    $arr = [
        $i,
        $i + 1,
    ];
endfor;
"#;
    let ranges = get_folding_ranges(php);
    // Array inside colon-delimited for: lines 2..5
    assert!(
        has_range(&ranges, 2, 5),
        "Expected array in for body range (2..5), got: {ranges:?}"
    );
}

// ─── Colon-delimited foreach ────────────────────────────────────────────────

#[test]
fn colon_delimited_foreach() {
    let php = r#"<?php
foreach ($items as $item):
    $arr = [
        $item,
        $item,
    ];
endforeach;
"#;
    let ranges = get_folding_ranges(php);
    // Array inside colon-delimited foreach: lines 2..5
    assert!(
        has_range(&ranges, 2, 5),
        "Expected array in foreach body range (2..5), got: {ranges:?}"
    );
}

// ─── Colon-delimited switch ─────────────────────────────────────────────────

#[test]
fn colon_delimited_switch() {
    let php = r#"<?php
switch ($x):
    case 1:
        $arr = [
            'one',
            'two',
        ];
        break;
    default:
        break;
endswitch;
"#;
    let ranges = get_folding_ranges(php);
    // Array inside colon-delimited switch case: lines 3..6
    assert!(
        has_range(&ranges, 3, 6),
        "Expected array in switch case range (3..6), got: {ranges:?}"
    );
}

// ─── Declare statement ──────────────────────────────────────────────────────

#[test]
fn declare_brace_delimited_body() {
    let php = r#"<?php
declare(strict_types=1) {
    echo "a";
    echo "b";
}
"#;
    let ranges = get_folding_ranges(php);
    // Block range: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected declare block range (1..4), got: {ranges:?}"
    );
}

#[test]
fn declare_colon_delimited_body() {
    let php = r#"<?php
declare(strict_types=1):
    $arr = [
        1,
        2,
    ];
enddeclare;
"#;
    let ranges = get_folding_ranges(php);
    // Array inside colon-delimited declare: lines 2..5
    assert!(
        has_range(&ranges, 2, 5),
        "Expected array in declare body range (2..5), got: {ranges:?}"
    );
}

// ─── Constant declarations ──────────────────────────────────────────────────

#[test]
fn constant_declaration_with_array_value() {
    let php = r#"<?php
const FOO = [
    1,
    2,
    3,
];
"#;
    let ranges = get_folding_ranges(php);
    // Array in const value: lines 1..5
    assert!(
        has_range(&ranges, 1, 5),
        "Expected const array range (1..5), got: {ranges:?}"
    );
}

// ─── Unset ──────────────────────────────────────────────────────────────────

#[test]
fn unset_walks_expressions() {
    let php = r#"<?php
unset($arr[foo(
    1,
    2
)]);
"#;
    let ranges = get_folding_ranges(php);
    // The foo() call argument list spans lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected call arg list range in unset (1..4), got: {ranges:?}"
    );
}

// ─── Echo tag ───────────────────────────────────────────────────────────────

#[test]
fn echo_tag_walks_expressions() {
    let php = r#"<?= [
    1,
    2,
] ?>"#;
    let ranges = get_folding_ranges(php);
    // Array in echo tag: lines 0..3
    assert!(
        has_range(&ranges, 0, 3),
        "Expected echo tag array range (0..3), got: {ranges:?}"
    );
}

// ─── Return with multi-line expression ──────────────────────────────────────

#[test]
fn return_with_multi_line_expression() {
    let php = r#"<?php
function foo() {
    return [
        1,
        2,
        3,
    ];
}
"#;
    let ranges = get_folding_ranges(php);
    // Function body: lines 1..7
    assert!(
        has_range(&ranges, 1, 7),
        "Expected function body range (1..7), got: {ranges:?}"
    );
    // Array in return: lines 2..6
    assert!(
        has_range(&ranges, 2, 6),
        "Expected return array range (2..6), got: {ranges:?}"
    );
}

// ─── Trait use with concrete specification ───────────────────────────────────

#[test]
fn trait_use_concrete_specification() {
    let php = r#"<?php
class Foo {
    use Bar, Baz {
        Bar::hello as private;
        Baz::world insteadof Bar;
    }
}
"#;
    let ranges = get_folding_ranges(php);
    // Class body: lines 1..6
    assert!(
        has_range(&ranges, 1, 6),
        "Expected class body range (1..6), got: {ranges:?}"
    );
    // Trait use concrete braces: lines 2..5
    assert!(
        has_range(&ranges, 2, 5),
        "Expected trait use concrete range (2..5), got: {ranges:?}"
    );
}

// ─── Property hooks (PHP 8.4) ───────────────────────────────────────────────

#[test]
fn property_hooks_produce_ranges() {
    let php = r#"<?php
class Foo {
    public string $name {
        get {
            return $this->name;
        }
        set {
            $this->name = $value;
        }
    }
}
"#;
    let ranges = get_folding_ranges(php);
    // Class body: lines 1..10
    assert!(
        has_range(&ranges, 1, 10),
        "Expected class body range (1..10), got: {ranges:?}"
    );
    // Hook list braces: lines 2..9
    assert!(
        has_range(&ranges, 2, 9),
        "Expected hook list range (2..9), got: {ranges:?}"
    );
    // get body: lines 3..5
    assert!(
        has_range(&ranges, 3, 5),
        "Expected get hook body range (3..5), got: {ranges:?}"
    );
    // set body: lines 6..8
    assert!(
        has_range(&ranges, 6, 8),
        "Expected set hook body range (6..8), got: {ranges:?}"
    );
}

// ─── Method call with multi-line args ────────────────────────────────────────

#[test]
fn method_call_multi_line_args() {
    let php = r#"<?php
$obj->method(
    $a,
    $b,
    $c
);
"#;
    let ranges = get_folding_ranges(php);
    // Method call arg list: lines 1..5
    assert!(
        has_range(&ranges, 1, 5),
        "Expected method call arg list range (1..5), got: {ranges:?}"
    );
}

// ─── Null-safe method call with multi-line args ─────────────────────────────

#[test]
fn null_safe_method_call_multi_line_args() {
    let php = r#"<?php
$obj?->method(
    $a,
    $b,
    $c
);
"#;
    let ranges = get_folding_ranges(php);
    // Null-safe method call arg list: lines 1..5
    assert!(
        has_range(&ranges, 1, 5),
        "Expected null-safe method call arg list range (1..5), got: {ranges:?}"
    );
}

// ─── Static method call with multi-line args ────────────────────────────────

#[test]
fn static_method_call_multi_line_args() {
    let php = r#"<?php
Foo::bar(
    $a,
    $b,
    $c
);
"#;
    let ranges = get_folding_ranges(php);
    // Static method call arg list: lines 1..5
    assert!(
        has_range(&ranges, 1, 5),
        "Expected static method call arg list range (1..5), got: {ranges:?}"
    );
}

// ─── Instantiation with multi-line args ─────────────────────────────────────

#[test]
fn instantiation_multi_line_args() {
    let php = r#"<?php
$obj = new Foo(
    $a,
    $b,
    $c
);
"#;
    let ranges = get_folding_ranges(php);
    // Instantiation arg list: lines 1..5
    assert!(
        has_range(&ranges, 1, 5),
        "Expected instantiation arg list range (1..5), got: {ranges:?}"
    );
}

// ─── Anonymous class with args and members ──────────────────────────────────

#[test]
fn anonymous_class_with_args_and_members() {
    let php = r#"<?php
$obj = new class(
    $a,
    $b
) {
    public function foo() {
        return 1;
    }
};
"#;
    let ranges = get_folding_ranges(php);
    // Argument list: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected anonymous class arg list range (1..4), got: {ranges:?}"
    );
    // Anonymous class body braces: lines 4..8
    assert!(
        has_range(&ranges, 4, 8),
        "Expected anonymous class body range (4..8), got: {ranges:?}"
    );
    // Method body: lines 5..7
    assert!(
        has_range(&ranges, 5, 7),
        "Expected method body range (5..7), got: {ranges:?}"
    );
}

// ─── Match with default arm ─────────────────────────────────────────────────

#[test]
fn match_with_default_arm_expression() {
    let php = r#"<?php
$result = match ($x) {
    1 => 'one',
    default => [
        'fallback',
        'value',
    ],
};
"#;
    let ranges = get_folding_ranges(php);
    // Match braces: lines 1..7
    assert!(
        has_range(&ranges, 1, 7),
        "Expected match expression range (1..7), got: {ranges:?}"
    );
    // Array in default arm: lines 3..6
    assert!(
        has_range(&ranges, 3, 6),
        "Expected default arm array range (3..6), got: {ranges:?}"
    );
}

// ─── Array with nested closures ─────────────────────────────────────────────

#[test]
fn array_with_nested_closures() {
    let php = r#"<?php
$handlers = [
    'a' => function () {
        return 1;
    },
    'b' => function () {
        return 2;
    },
];
"#;
    let ranges = get_folding_ranges(php);
    // Outer array: lines 1..8
    assert!(
        has_range(&ranges, 1, 8),
        "Expected outer array range (1..8), got: {ranges:?}"
    );
    // First closure body: lines 2..4
    assert!(
        has_range(&ranges, 2, 4),
        "Expected first closure range (2..4), got: {ranges:?}"
    );
    // Second closure body: lines 5..7
    assert!(
        has_range(&ranges, 5, 7),
        "Expected second closure range (5..7), got: {ranges:?}"
    );
}

// ─── Binary and assignment with closure on RHS ──────────────────────────────

#[test]
fn binary_expression_with_multi_line_rhs() {
    let php = r#"<?php
$result = $a + foo(
    1,
    2
);
"#;
    let ranges = get_folding_ranges(php);
    // The foo() call arg list spans multiple lines: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected call arg list range in binary (1..4), got: {ranges:?}"
    );
}

#[test]
fn assignment_with_closure_rhs() {
    let php = r#"<?php
$fn = function () {
    return true;
};
"#;
    let ranges = get_folding_ranges(php);
    // Closure body: lines 1..3
    assert!(
        has_range(&ranges, 1, 3),
        "Expected closure body range (1..3), got: {ranges:?}"
    );
}

// ─── Conditional ternary ────────────────────────────────────────────────────

#[test]
fn conditional_ternary_with_multi_line_expressions() {
    let php = r#"<?php
$result = $condition
    ? [
        1,
        2,
    ]
    : [
        3,
        4,
    ];
"#;
    let ranges = get_folding_ranges(php);
    // Then-branch array: lines 2..5
    assert!(
        has_range(&ranges, 2, 5),
        "Expected then-branch array range (2..5), got: {ranges:?}"
    );
    // Else-branch array: lines 6..9
    assert!(
        has_range(&ranges, 6, 9),
        "Expected else-branch array range (6..9), got: {ranges:?}"
    );
}

// ─── Yield expressions ──────────────────────────────────────────────────────

#[test]
fn yield_value_expression() {
    let php = r#"<?php
function gen() {
    yield [
        1,
        2,
    ];
}
"#;
    let ranges = get_folding_ranges(php);
    // Function body: lines 1..6
    assert!(
        has_range(&ranges, 1, 6),
        "Expected generator function body range (1..6), got: {ranges:?}"
    );
    // Yield value array: lines 2..5
    assert!(
        has_range(&ranges, 2, 5),
        "Expected yield value array range (2..5), got: {ranges:?}"
    );
}

#[test]
fn yield_pair_expression() {
    let php = r#"<?php
function gen() {
    yield 'key' => [
        3,
        4,
    ];
}
"#;
    let ranges = get_folding_ranges(php);
    // Yield pair value array: lines 2..5
    assert!(
        has_range(&ranges, 2, 5),
        "Expected yield pair value array range (2..5), got: {ranges:?}"
    );
}

#[test]
fn yield_from_expression() {
    let php = r#"<?php
function gen() {
    yield from foo(
        1,
        2
    );
}
"#;
    let ranges = get_folding_ranges(php);
    // foo() call arg list inside yield from: lines 2..5
    assert!(
        has_range(&ranges, 2, 5),
        "Expected yield from call arg list range (2..5), got: {ranges:?}"
    );
}

// ─── Throw expression ───────────────────────────────────────────────────────

#[test]
fn throw_expression_with_instantiation() {
    let php = r#"<?php
throw new Exception(
    'error',
    500
);
"#;
    let ranges = get_folding_ranges(php);
    // Instantiation arg list inside throw: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected throw instantiation arg list range (1..4), got: {ranges:?}"
    );
}

// ─── Clone expression ───────────────────────────────────────────────────────

#[test]
fn clone_expression_walks_inner() {
    let php = r#"<?php
$b = clone new Foo(
    1,
    2
);
"#;
    let ranges = get_folding_ranges(php);
    // Instantiation arg list inside clone: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected clone instantiation arg list range (1..4), got: {ranges:?}"
    );
}

// ─── List destructuring ────────────────────────────────────────────────────

#[test]
fn list_destructuring() {
    let php = r#"<?php
list($a, $b) = [
    1,
    2,
];
"#;
    let ranges = get_folding_ranges(php);
    // Array on RHS: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected list RHS array range (1..4), got: {ranges:?}"
    );
}

// ─── Construct expressions ──────────────────────────────────────────────────

#[test]
fn construct_print_walks_expression() {
    let php = r#"<?php
print implode(",", [
    1,
    2,
    3,
]);
"#;
    let ranges = get_folding_ranges(php);
    // The implode() call arg list: lines 1..5
    assert!(
        has_range(&ranges, 1, 5),
        "Expected print call arg list range (1..5), got: {ranges:?}"
    );
}

#[test]
fn construct_exit_walks_arguments() {
    let php = r#"<?php
exit(foo(
    1,
    2
));
"#;
    let ranges = get_folding_ranges(php);
    // foo() call arg list inside exit: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected exit call arg list range (1..4), got: {ranges:?}"
    );
}

#[test]
fn construct_die_walks_arguments() {
    let php = r#"<?php
die(foo(
    1,
    2
));
"#;
    let ranges = get_folding_ranges(php);
    // foo() call arg list inside die: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected die call arg list range (1..4), got: {ranges:?}"
    );
}

#[test]
fn construct_isset_walks_values() {
    let php = r#"<?php
$x = isset(foo(
    1,
    2
));
"#;
    let ranges = get_folding_ranges(php);
    // foo() call arg list inside isset: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected isset call arg list range (1..4), got: {ranges:?}"
    );
}

#[test]
fn construct_empty_walks_value() {
    let php = r#"<?php
$x = empty(foo(
    1,
    2
));
"#;
    let ranges = get_folding_ranges(php);
    // foo() call arg list inside empty: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected empty call arg list range (1..4), got: {ranges:?}"
    );
}

#[test]
fn construct_eval_walks_value() {
    let php = r#"<?php
eval(implode("\n", [
    '$a = 1;',
    '$b = 2;',
]));
"#;
    let ranges = get_folding_ranges(php);
    // implode() call arg list inside eval: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected eval call arg list range (1..4), got: {ranges:?}"
    );
}

#[test]
fn construct_include_walks_value() {
    let php = r#"<?php
include foo(
    'a',
    'b'
);
"#;
    let ranges = get_folding_ranges(php);
    // foo() call arg list: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected include call arg list range (1..4), got: {ranges:?}"
    );
}

#[test]
fn construct_include_once_walks_value() {
    let php = r#"<?php
include_once foo(
    'a',
    'b'
);
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 4),
        "Expected include_once call arg list range (1..4), got: {ranges:?}"
    );
}

#[test]
fn construct_require_walks_value() {
    let php = r#"<?php
require foo(
    'a',
    'b'
);
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 4),
        "Expected require call arg list range (1..4), got: {ranges:?}"
    );
}

#[test]
fn construct_require_once_walks_value() {
    let php = r#"<?php
require_once foo(
    'a',
    'b'
);
"#;
    let ranges = get_folding_ranges(php);
    assert!(
        has_range(&ranges, 1, 4),
        "Expected require_once call arg list range (1..4), got: {ranges:?}"
    );
}

// ─── Composite string ───────────────────────────────────────────────────────

#[test]
fn composite_string_walks_embedded_expressions() {
    let php = r#"<?php
$str = "value: {$arr[foo(
    1,
    2
)]}";
"#;
    let ranges = get_folding_ranges(php);
    // foo() call arg list inside interpolated string: lines 1..4
    assert!(
        has_range(&ranges, 1, 4),
        "Expected composite string call arg list range (1..4), got: {ranges:?}"
    );
}

// ─── Pipe expression ────────────────────────────────────────────────────────

#[test]
fn pipe_expression_walks_both_sides() {
    let php = r#"<?php
$result = [
    1,
    2,
    3,
] |> array_sum(...);
"#;
    let ranges = get_folding_ranges(php);
    // If the parser supports pipe, the input array spans lines 1..5
    assert!(
        has_range(&ranges, 1, 5),
        "Expected pipe input array range (1..5), got: {ranges:?}"
    );
}

// ─── Non-adjacent single-line comments produce separate groups ──────────────

#[test]
fn non_adjacent_single_line_comments_produce_separate_groups() {
    let php = r#"<?php
// group one
// group one continued

// group two
// group two continued
function foo() {}
"#;
    let ranges = get_folding_ranges(php);
    // Group 1: lines 1..2
    assert!(
        has_comment_range(&ranges, 1, 2),
        "Expected first comment group range (1..2), got: {ranges:?}"
    );
    // Group 2: lines 4..5
    assert!(
        has_comment_range(&ranges, 4, 5),
        "Expected second comment group range (4..5), got: {ranges:?}"
    );
}

// ─── Hash-style comments ────────────────────────────────────────────────────

#[test]
fn hash_style_comments_produce_comment_range() {
    let php = r#"<?php
# comment one
# comment two
# comment three
function foo() {}
"#;
    let ranges = get_folding_ranges(php);
    // Three consecutive # comments on lines 1, 2, 3
    assert!(
        has_comment_range(&ranges, 1, 3),
        "Expected hash comment group range (1..3), got: {ranges:?}"
    );
}
