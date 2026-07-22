use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Open a file, trigger `update_ast`, then collect undefined-variable diagnostics.
fn undefined_var_diagnostics(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_undefined_variable_diagnostics(uri, php, &mut out);
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Basic detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_undefined_variable_in_echo() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    echo $nmae;
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$nmae"));
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
}

#[test]
fn flags_undefined_in_expression() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $x = $y + 1;
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$y"));
}

#[test]
fn flags_multiple_undefined_variables() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    echo $a;
    echo $b;
    echo $c;
}
"#,
    );
    assert_eq!(diags.len(), 3);
}

#[test]
fn diagnostic_has_correct_code_and_source() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    echo $x;
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(
        diags[0].code,
        Some(NumberOrString::String("unknown_variable".to_string())),
    );
    assert_eq!(diags[0].source, Some("phpantom".to_string()));
}

#[test]
fn flags_undefined_in_return() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): string {
    return $missing;
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$missing"));
}

#[test]
fn flags_undefined_in_function_argument() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    strlen($notDefined);
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$notDefined"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Defined variables — no diagnostic expected
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_assigned_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $name = "Alice";
    echo $name;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_parameter() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(string $name): void {
    echo $name;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_foreach_key_and_value() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(array $items): void {
    foreach ($items as $key => $value) {
        echo $key . ': ' . $value;
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_catch_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    try {
        throw new \RuntimeException('oops');
    } catch (\Exception $e) {
        echo $e->getMessage();
    }
}
"#,
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("$e")),
        "Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_global_statement() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    global $config;
    echo $config;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_static_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    static $count = 0;
    $count++;
    echo $count;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_list_destructuring() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(array $pair): void {
    [$first, $second] = $pair;
    echo $first;
    echo $second;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_compound_assignment() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $x = 0;
    $x += 5;
    echo $x;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_branch_assignment() {
    // Phase 1 conservative: any assignment anywhere in the function counts.
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(bool $flag): void {
    if ($flag) {
        $result = "yes";
    }
    echo $result;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_for_loop_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    for ($i = 0; $i < 10; $i++) {
        echo $i;
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Superglobals
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_superglobals() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    echo $_GET['key'];
    echo $_POST['key'];
    echo $_SERVER['REQUEST_URI'];
    echo $_SESSION['user'];
    echo $_COOKIE['token'];
    echo $_FILES['upload'];
    echo $_ENV['APP_ENV'];
    echo $_REQUEST['data'];
    echo $GLOBALS['x'];
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_argc_argv() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    echo $argc;
    echo $argv[0];
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// $this
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_this_in_instance_method() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    private string $name = '';

    public function bar(): string {
        return $this->name;
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_this_in_static_method() {
    // $this in static methods is a separate concern; we skip it entirely.
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    public static function bar(): void {
        echo $this;
    }
}
"#,
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("$this")),
        "Got: {:?}",
        diags,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Suppression: isset / empty
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_inside_isset() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    if (isset($maybe)) {
        // $maybe is guarded by isset — the read inside isset is OK.
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_inside_empty() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    if (empty($value)) {
        return;
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_isset_with_array_access() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    if (isset($data['key'])) {
        echo "found";
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Suppression: compact
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_compact_referenced_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): array {
    $name = "Alice";
    $age = 30;
    return compact('name', 'age');
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Suppression: extract
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_when_extract_is_used() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(array $data): void {
    extract($data);
    echo $name;
    echo $age;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Suppression: variable variables ($$)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_when_variable_variables_present() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $varName = 'hello';
    $$varName = 'world';
    echo $unknown;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Suppression: @ error control operator
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_error_suppressed_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    echo @$undefined;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Suppression: @var inline annotation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_var_annotated_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    /** @var string $name */
    echo $name;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn var_annotation_does_not_leak_into_other_function() {
    // A `/** @var ... $name */` in one function must not suppress an
    // undefined `$name` in a different function in the same file.
    let diags = undefined_var_diagnostics(
        r#"<?php
function annotated(): void {
    /** @var string $name */
    echo $name;
}

function other(): void {
    echo $name;
}
"#,
    );
    assert!(
        diags.iter().any(|d| d.message.contains("$name")),
        "Expected undefined $name in other(), got: {:?}",
        diags
    );
    assert_eq!(
        diags.len(),
        1,
        "Only other()'s $name should be flagged, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Closures
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_closure_use_captured_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $x = 42;
    $fn = function() use ($x) {
        echo $x;
    };
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn flags_undefined_in_closure_without_use_capture() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $outer = 42;
    $fn = function() {
        echo $outer;
    };
}
"#,
    );
    assert!(
        diags.iter().any(|d| d.message.contains("$outer")),
        "Expected undefined $outer in closure, got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_closure_parameter() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $fn = function(string $name) {
        echo $name;
    };
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Arrow functions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_arrow_function_implicit_capture() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $multiplier = 2;
    $fn = fn(int $n) => $n * $multiplier;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_arrow_function_parameter() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $fn = fn(int $n) => $n * 2;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Class methods
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_undefined_in_method() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    public function bar(): void {
        echo $undefined;
    }
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$undefined"));
}

#[test]
fn no_diagnostic_for_method_parameter() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    public function bar(string $name): void {
        echo $name;
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Static property access
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_self_static_property() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    private static ?self $instance = null;

    public static function getInstance(): self {
        return self::$instance ?? throw new \RuntimeException;
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_static_static_property() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    protected static array $items = [];

    public static function add(string $item): void {
        static::$items[] = $item;
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_classname_static_property() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Config {
    public static bool $debug = false;
}

class App {
    public function boot(): void {
        if (Config::$debug) {
            echo "debug mode";
        }
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn flags_undefined_in_dynamic_static_property() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    public static function get(): mixed {
        return self::$$prop;
    }
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$prop"));
}

#[test]
fn no_diagnostic_for_defined_dynamic_static_property() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    public static function get(string $prop): mixed {
        return self::$$prop;
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn flags_undefined_in_indirect_static_property() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    public static function get(): mixed {
        return self::${'prop_' . $suffix};
    }
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$suffix"));
}

#[test]
fn no_diagnostic_for_defined_indirect_static_property() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    public static function get(string $suffix): mixed {
        return self::${'prop_' . $suffix};
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Traits and enums
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_undefined_in_trait_method() {
    let diags = undefined_var_diagnostics(
        r#"<?php
trait MyTrait {
    public function foo(): void {
        echo $undefined;
    }
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$undefined"));
}

#[test]
fn flags_undefined_in_enum_method() {
    let diags = undefined_var_diagnostics(
        r#"<?php
enum Status {
    case Active;

    public function label(): string {
        return $undefined;
    }
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$undefined"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Top-level code (should NOT diagnose)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_top_level_code() {
    let diags = undefined_var_diagnostics(
        r#"<?php
echo $undefined;
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Namespaced code
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_undefined_in_namespaced_function() {
    let diags = undefined_var_diagnostics(
        r#"<?php
namespace App;

function test(): void {
    echo $undefined;
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$undefined"));
}

#[test]
fn flags_undefined_in_namespaced_class() {
    let diags = undefined_var_diagnostics(
        r#"<?php
namespace App;

class Foo {
    public function bar(): void {
        echo $undefined;
    }
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$undefined"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Unset — should not flag
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_unset_of_defined_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $x = 1;
    unset($x);
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Reference parameters
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_reference_parameter() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(array &$items): void {
    $items[] = 'new';
    echo count($items);
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Match expression
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_match_subject() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(int $status): string {
    return match($status) {
        1 => 'active',
        2 => 'inactive',
        default => 'unknown',
    };
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Yield
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_undefined_in_yield() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): \Generator {
    yield $undefined;
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$undefined"));
}

#[test]
fn no_diagnostic_for_defined_in_yield() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): \Generator {
    $x = 42;
    yield $x;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Multiple scopes in one file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn each_function_has_its_own_scope() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function foo(): void {
    $a = 1;
    echo $a;
}

function bar(): void {
    echo $a;
}
"#,
    );
    // $a is defined in foo() but not in bar().
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$a"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Ternary / null coalescing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_undefined_in_ternary() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): string {
    return $maybeUndefined ? 'yes' : 'no';
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$maybeUndefined"));
}

#[test]
fn no_diagnostic_for_defined_in_ternary() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(bool $flag): string {
    return $flag ? 'yes' : 'no';
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// String interpolation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_undefined_in_string_interpolation() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    echo "Hello $name";
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$name"));
}

#[test]
fn no_diagnostic_for_defined_in_interpolation() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $name = "World";
    echo "Hello $name";
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Switch statement
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_switch_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(int $code): string {
    switch ($code) {
        case 200:
            $msg = 'OK';
            break;
        default:
            $msg = 'Error';
    }
    return $msg;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// While / do-while
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_while_loop_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $i = 0;
    while ($i < 10) {
        echo $i;
        $i++;
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// Diagnostic range accuracy
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_range_covers_variable_name() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    echo $undefinedVar;
}
"#,
    );
    assert_eq!(diags.len(), 1);
    // "$undefinedVar" is 13 chars; check that the range covers exactly that.
    let range = diags[0].range;
    assert_eq!(range.start.line, 2);
    assert_eq!(range.end.line, 2);
    let col_span = range.end.character - range.start.character;
    assert_eq!(
        col_span, 13,
        "Range should cover '$undefinedVar' (13 chars)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Braced namespace
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_undefined_in_braced_namespace() {
    let diags = undefined_var_diagnostics(
        r#"<?php
namespace App {
    function test(): void {
        echo $undefined;
    }
}
"#,
    );
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("$undefined"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Array element assignment defines the variable
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_array_access_assignment() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $b['a'] = 'hello';
    echo $b;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Array access assignment should define the variable. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_array_append_assignment() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $items[] = 'hello';
    echo $items;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Array append assignment should define the variable. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_nested_array_access_assignment() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $b['a']['a'] = 'a';
    echo $b;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Nested array access assignment should define the variable. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_deeply_nested_array_access_assignment() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $config['db']['host']['primary'] = 'localhost';
    echo $config;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Deeply nested array access assignment should define the variable. Got: {:?}",
        diags,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Postfix increment of a defined variable
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_postfix_increment_of_defined_var() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $x = 0;
    $x++;
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// isset() guards the read inside it, but does not itself define the variable
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_undefined_variable_after_isset_guard() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    if (isset($x)) {
        echo $x;
    }
}
"#,
    );
    // $x inside isset() should not be flagged, but $x is never assigned, so
    // the echo inside the if body is still a read of an undefined variable.
    assert_eq!(diags.len(), 1, "Got: {:?}", diags);
    assert!(diags[0].message.contains("$x"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Nested closures and arrow functions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_undefined_in_closure_without_capture_nested() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $outer = function () {
        $local = 42;
        $inner = function () {
            echo $local;
        };
    };
}
"#,
    );
    assert_eq!(
        diags.len(),
        1,
        "Closure without use() should not see parent closure variables. Got: {:?}",
        diags,
    );
    assert!(diags[0].message.contains("$local"));
}

#[test]
fn no_diagnostic_for_nested_closure_use_captures() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $outer = function () {
        $brandIds = [1, 2, 3];
        $typeIds = [4, 5, 6];

        $inner = function () use ($brandIds, $typeIds) {
            return [$brandIds[0], $typeIds[0]];
        };

        return $inner();
    };
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Nested closure use() captures should be visible. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_arrow_fn_capturing_closure_variable() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $callback = function (array $ids) {
        $sortMap = array_flip($ids);
        return array_map(fn($item) => $sortMap[$item], $ids);
    };
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Arrow fn should see variables from enclosing closure. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_arrow_fn_in_closure_in_method() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Foo {
    public function run(): void {
        $this->process(function (array $products, array $ids) {
            $sortMap = array_flip($ids);
            return $products->sortBy(fn($product) => $sortMap[$product->id]);
        });
    }
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Arrow fn in closure in method should see closure variables. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_deeply_nested_arrow_functions() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $a = 1;
    $f = function () use ($a) {
        $b = 2;
        $g = fn() => fn() => $a + $b;
        return $g;
    };
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Deeply nested arrow fns should see all ancestor variables. Got: {:?}",
        diags,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Try/catch variable visibility
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_function_param_used_in_catch() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function capture(string $payment, float $amount): void {
    try {
        doSomething($amount);
    } catch (\Exception $e) {
        echo $payment;
        echo $amount;
        echo $e->getMessage();
    }
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Function parameters should be visible inside catch blocks. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_outer_variable_used_in_catch() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $client = getClient();
    $token = 'abc';
    try {
        $client->send($token);
    } catch (\RuntimeException $e) {
        log($client, $token, $e->getMessage());
    }
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Variables assigned before try should be visible inside catch blocks. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_try_assigned_variable_used_in_catch() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    try {
        $response = fetchData();
    } catch (\Exception $e) {
        if (isset($response)) {
            echo $response;
        }
    }
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Variables assigned in try block should be visible in catch (guarded by isset). Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_catch_inside_closure() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $handler = function (string $payment) {
        $client = getClient();
        try {
            $response = $client->send($payment);
        } catch (\Exception $e) {
            log($payment, $client, $e->getMessage());
        }
    };
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Catch inside closure should see closure variables. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_variable_assigned_in_try_used_in_catch_inside_closure() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $handler = function () {
        $fullFilePath = '/tmp/test.jpg';
        try {
            process($fullFilePath);
        } catch (\Throwable $e) {
            fallback($fullFilePath);
        }
    };
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Variable assigned before try should be visible in catch inside closure. Got: {:?}",
        diags,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// By-reference foreach binding
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_foreach_by_reference_binding() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $values = [1, 2, 3];
    foreach ($values as &$value) {
        $value = 4;
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

#[test]
fn no_diagnostic_for_foreach_by_reference_key_value_binding() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(array $items): void {
    foreach ($items as $key => &$value) {
        echo $key;
        $value = 'modified';
    }
}
"#,
    );
    assert!(diags.is_empty(), "Got: {:?}", diags);
}

// ═══════════════════════════════════════════════════════════════════════════
// By-reference out-parameters (built-in functions)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_preg_match_out_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(string $input): ?string {
    if (preg_match('/(\d+)/', $input, $match) === 1) {
        return $match[1];
    }
    return null;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "preg_match out-param $match should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_fqn_preg_match() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(string $input): ?string {
    if (\preg_match('/(\d+)/', $input, $match) === 1) {
        return $match[1];
    }
    return null;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "FQN \\preg_match out-param should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_preg_match_all_out_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(string $text): array {
    preg_match_all('/\w+/', $text, $matches);
    return $matches[0];
}
"#,
    );
    assert!(
        diags.is_empty(),
        "preg_match_all out-param $matches should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_parse_str_out_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(string $query): string {
    parse_str($query, $data);
    return $data['key'] ?? '';
}
"#,
    );
    assert!(
        diags.is_empty(),
        "parse_str out-param $data should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_mb_parse_str_out_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(string $input): array {
    mb_parse_str($input, $result);
    return $result;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "mb_parse_str out-param $result should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_curl_multi_exec_out_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test($mh): int {
    curl_multi_exec($mh, $running);
    return $running;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "curl_multi_exec out-param $running should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_fsockopen_out_params() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    $fp = fsockopen('example.com', 80, $errno, $errstr);
    echo $errno . $errstr;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "fsockopen out-params $errno/$errstr should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_openssl_sign_out_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(string $data, $key): string {
    openssl_sign($data, $signature, $key);
    return $signature;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "openssl_sign out-param $signature should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_getimagesize_out_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(string $file): array {
    $info = getimagesize($file, $imageinfo);
    return $imageinfo;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "getimagesize out-param $imageinfo should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_headers_sent_out_params() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    headers_sent($file, $line);
    echo $file . ':' . $line;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "headers_sent out-params $file/$line should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_pcntl_wait_out_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(): void {
    pcntl_wait($status);
    echo $status;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "pcntl_wait out-param $status should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_dns_get_mx_out_params() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test(string $host): void {
    dns_get_mx($host, $mxhosts, $weights);
    var_dump($mxhosts, $weights);
}
"#,
    );
    assert!(
        diags.is_empty(),
        "dns_get_mx out-params should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_flock_out_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function test($fp): void {
    flock($fp, LOCK_EX, $wouldblock);
    echo $wouldblock;
}
"#,
    );
    assert!(
        diags.is_empty(),
        "flock out-param $wouldblock should be treated as defined. Got: {:?}",
        diags,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// By-reference out-parameters via resolver (user-defined functions, static
// methods, constructors, instance methods)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_user_defined_function_byref_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function myFunc(string $input, array &$output): void {
    $output = [$input];
}
function test(string $val): void {
    myFunc($val, $result);
    echo $result[0];
}
"#,
    );
    assert!(
        diags.is_empty(),
        "User-defined function by-ref $result should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_fqn_user_defined_function_byref_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
namespace App;
function transform(string $in, array &$out): void {
    $out = [$in];
}
function test(): void {
    \App\transform('hello', $result);
    echo $result[0];
}
"#,
    );
    assert!(
        diags.is_empty(),
        "FQN user-defined function by-ref $result should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_static_method_byref_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Validator {
    public static function validate(string $input, array &$errors): bool {
        $errors = [];
        return true;
    }
}
function test(string $data): void {
    Validator::validate($data, $errors);
    var_dump($errors);
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Static method by-ref $errors should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_constructor_byref_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Parser {
    public function __construct(string $input, array &$warnings) {
        $warnings = [];
    }
}
function test(string $src): void {
    $p = new Parser($src, $warnings);
    var_dump($warnings);
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Constructor by-ref $warnings should be treated as defined. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_this_method_byref_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class Svc {
    private function init(?string &$out): void {
        $out = 'ready';
    }
    public function demo(): void {
        $this->init($result);
        echo $result;
    }
}
"#,
    );
    assert!(
        diags.is_empty(),
        "$this->method() with by-ref param should not flag $result. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_new_instance_method_byref_param() {
    // Regression: by-ref out-params on instance methods via `new A()->…`
    // must define the variable, same as free functions / preg_match.
    let diags = undefined_var_diagnostics(
        r#"<?php
class A {
    public function dosmth(?string &$y, ?string &$x) {
        $x = "";
    }
    public function x() {
        return true;
    }
}
class B {
    public function create()
    {
        $y = "";
        new A()->dosmth($y, $foo);
        echo $foo;
    }
}
"#,
    );
    assert!(
        diags.is_empty(),
        "new A()->method() with by-ref param should not flag $foo. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_parenthesized_new_instance_method_byref_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
class A {
    public function fill(array &$out): void {
        $out = [1];
    }
}
function test(): void {
    (new A())->fill($result);
    echo $result[0];
}
"#,
    );
    assert!(
        diags.is_empty(),
        "(new A())->method() with by-ref param should not flag $result. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_namespaced_unqualified_function_byref_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
namespace App;
function initItem(?string &$out): void {
    $out = 'hello';
}
class Svc {
    public function demo(): void {
        initItem($result);
        echo $result;
    }
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Unqualified call to namespaced function with by-ref param should not flag $result. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_namespaced_static_method_byref_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
namespace App;
class Factory {
    public static function create(?string &$out): void {
        $out = 'hello';
    }
}
class Svc {
    public function demo(): void {
        Factory::create($result);
        echo $result;
    }
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Static method with by-ref param in namespace should not flag $result. Got: {:?}",
        diags,
    );
}

#[test]
fn no_diagnostic_for_namespaced_constructor_byref_param() {
    let diags = undefined_var_diagnostics(
        r#"<?php
namespace App;
class Builder {
    public function __construct(?string &$out) {
        $out = 'built';
    }
}
class Svc {
    public function demo(): void {
        new Builder($result);
        echo $result;
    }
}
"#,
    );
    assert!(
        diags.is_empty(),
        "Constructor with by-ref param in namespace should not flag $result. Got: {:?}",
        diags,
    );
}

#[test]
fn diagnostic_still_fires_for_truly_undefined_after_non_byref_call() {
    let diags = undefined_var_diagnostics(
        r#"<?php
function noRefs(string $a): void {}
function test(): void {
    noRefs('hello');
    echo $undefined;
}
"#,
    );
    assert_eq!(
        diags.len(),
        1,
        "Should flag $undefined even when resolver is active. Got: {:?}",
        diags,
    );
    assert!(
        diags[0].message.contains("$undefined"),
        "Diagnostic should be for $undefined",
    );
}
