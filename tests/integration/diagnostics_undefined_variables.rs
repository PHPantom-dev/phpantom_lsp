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
