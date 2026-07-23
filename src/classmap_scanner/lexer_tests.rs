use super::*;

// ── find_classes unit tests ──────────────────────────────────────

#[test]
fn simple_class() {
    let content = b"<?php\nclass Foo {}";
    assert_eq!(find_classes(content), vec!["Foo"]);
}

#[test]
fn namespaced_class() {
    let content = b"<?php\nnamespace App\\Models;\nclass User {}";
    assert_eq!(find_classes(content), vec!["App\\Models\\User"]);
}

#[test]
fn multiple_declarations() {
    let content = br"<?php
namespace App;

class Foo {}
interface Bar {}
trait Baz {}
enum Status {}
";
    assert_eq!(
        find_classes(content),
        vec!["App\\Foo", "App\\Bar", "App\\Baz", "App\\Status"]
    );
}

#[test]
fn class_in_comment_ignored() {
    let content = br"<?php
// class Fake {}
/* class AlsoFake {} */
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn class_in_string_ignored() {
    let content = br#"<?php
$x = "class Fake {}";
$y = 'class AlsoFake {}';
class Real {}
"#;
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn no_classes() {
    let content = b"<?php\necho 'hello';";
    assert!(find_classes(content).is_empty());
}

#[test]
fn enum_with_type() {
    let content = b"<?php\nenum Status: int { case Active = 1; }";
    assert_eq!(find_classes(content), vec!["Status"]);
}

#[test]
fn class_constant_not_treated_as_declaration() {
    let content = b"<?php\n$x = SomeClass::class;\nclass Real {}";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn php_attribute() {
    let content = br"<?php
#[Attribute]
class MyAttribute {}
";
    assert_eq!(find_classes(content), vec!["MyAttribute"]);
}

#[test]
fn heredoc() {
    let content = br"<?php
$x = <<<EOT
class Fake {}
EOT;
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn nowdoc() {
    let content = br"<?php
$x = <<<'EOT'
class Fake {}
EOT;
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn property_access_class_ignored() {
    let content = br"<?php
namespace Foo;
if ($node->class instanceof Name) {
}
";
    assert!(find_classes(content).is_empty());
}

#[test]
fn nullsafe_property_access_class_ignored() {
    let content = br"<?php
namespace Foo;
if ($node?->class instanceof Name) {
}
";
    assert!(find_classes(content).is_empty());
}

#[test]
fn real_class_not_affected_by_property_access() {
    let content = br"<?php
namespace Foo;
class Real {}
if ($node->class instanceof Name) {
}
";
    assert_eq!(find_classes(content), vec!["Foo\\Real"]);
}

#[test]
fn anonymous_class_ignored() {
    let content = br"<?php
$x = new class extends Foo {};
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn anonymous_class_implements_ignored() {
    let content = br"<?php
$x = new class implements Bar {};
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn hash_comment_not_confused_with_attribute() {
    let content = br"<?php
# This is a comment with class keyword
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn multiple_namespaces() {
    let content = br"<?php
namespace First;
class A {}
namespace Second;
class B {}
";
    assert_eq!(find_classes(content), vec!["First\\A", "Second\\B"]);
}

#[test]
fn global_namespace_after_named() {
    // namespace; with no name resets to global
    let content = br"<?php
namespace Foo;
class A {}
namespace;
class B {}
";
    // When `namespace;` is encountered with no name, the namespace
    // becomes empty (global).
    assert_eq!(find_classes(content), vec!["Foo\\A", "B"]);
}

#[test]
fn escaped_string_does_not_leak() {
    let content = br#"<?php
$x = "escaped \" class Fake {}";
class Real {}
"#;
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn escaped_single_quote_string_does_not_leak() {
    let content = br"<?php
$x = 'escaped \' class Fake {}';
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn block_comment_with_star() {
    let content = br"<?php
/**
 * class Fake {}
 */
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

#[test]
fn empty_content() {
    assert!(find_classes(b"").is_empty());
}

#[test]
fn no_keyword_quick_rejection() {
    let content = b"<?php\necho 'hello world';";
    assert!(find_classes(content).is_empty());
}

#[test]
fn flexible_heredoc_php73() {
    // PHP 7.3+ allows the closing identifier to be indented
    let content = br"<?php
$x = <<<EOT
    class Fake {}
    EOT;
class Real {}
";
    assert_eq!(find_classes(content), vec!["Real"]);
}

// ── find_symbols unit tests ─────────────────────────────────────

#[test]
fn symbols_simple_function() {
    let content = b"<?php\nfunction helper(): void {}";
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["helper"]);
    assert!(result.classes.is_empty());
    assert!(result.constants.is_empty());
}

#[test]
fn symbols_namespaced_function() {
    let content = b"<?php\nnamespace App\\Helpers;\nfunction helper(): void {}";
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["App\\Helpers\\helper"]);
}

#[test]
fn symbols_closure_not_captured() {
    let content = b"<?php\n$fn = function () { return 1; };";
    let result = find_symbols(content);
    assert!(result.functions.is_empty());
}

#[test]
fn use_function_not_captured() {
    let content =
        b"<?php\nnamespace App\\Cache;\nuse function is_array;\nuse function array_map;\n";
    let result = find_symbols(content);
    assert!(
        result.functions.is_empty(),
        "use function statements should not appear as functions: {:?}",
        result.functions
    );
}

#[test]
fn use_const_not_captured() {
    let content = b"<?php\nnamespace App\\Config;\nuse const PHP_EOL;\n";
    let result = find_symbols(content);
    assert!(
        result.constants.is_empty(),
        "use const statements should not appear as constants: {:?}",
        result.constants
    );
}

#[test]
fn symbols_method_not_captured() {
    let content = br"<?php
class Foo {
    public function bar(): void {}
}
";
    let result = find_symbols(content);
    assert_eq!(result.classes, vec!["Foo"]);
    assert!(
        result.functions.is_empty(),
        "methods should not appear as functions: {:?}",
        result.functions
    );
}

#[test]
fn symbols_define_single_quote() {
    let content = b"<?php\ndefine('MY_CONST', 42);";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["MY_CONST"]);
}

#[test]
fn symbols_define_double_quote() {
    let content = b"<?php\ndefine(\"APP_VERSION\", '1.0');";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["APP_VERSION"]);
}

#[test]
fn symbols_top_level_const() {
    let content = b"<?php\nconst FOO = 'bar';";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["FOO"]);
}

#[test]
fn symbols_namespaced_const() {
    let content = b"<?php\nnamespace App;\nconst VERSION = '1.0';";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["App\\VERSION"]);
}

#[test]
fn symbols_class_const_not_captured() {
    let content = br"<?php
class Config {
    const MAX = 100;
    public function foo(): void {}
}
";
    let result = find_symbols(content);
    assert_eq!(result.classes, vec!["Config"]);
    assert!(
        result.constants.is_empty(),
        "class constants should not be captured: {:?}",
        result.constants
    );
    assert!(
        result.functions.is_empty(),
        "methods should not be captured: {:?}",
        result.functions
    );
}

#[test]
fn symbols_mixed_file() {
    let content = br#"<?php
namespace App\Utils;

class Helper {}
interface Renderable {}

function formatDate(): string { return ''; }
function parseJson(): array { return []; }

define('APP_NAME', 'MyApp');
const DEBUG = true;
"#;
    let result = find_symbols(content);
    assert_eq!(
        result.classes,
        vec!["App\\Utils\\Helper", "App\\Utils\\Renderable"]
    );
    assert_eq!(
        result.functions,
        vec!["App\\Utils\\formatDate", "App\\Utils\\parseJson"]
    );
    assert!(
        result.constants.contains(&"APP_NAME".to_string()),
        "should find define(): {:?}",
        result.constants
    );
    assert!(
        result.constants.contains(&"App\\Utils\\DEBUG".to_string()),
        "should find namespaced const: {:?}",
        result.constants
    );
}

#[test]
fn symbols_function_in_comment_ignored() {
    let content = b"<?php\n// function notReal(): void {}\nfunction real(): void {}";
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["real"]);
}

#[test]
fn symbols_function_named_int() {
    let content = br#"<?php
declare(strict_types=1);
namespace Psl\Type;
function int(): TypeInterface
{
    static $instance = new Internal\IntType();
    return $instance;
}
"#;
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["Psl\\Type\\int"]);
}

#[test]
fn symbols_define_in_string_ignored() {
    let content = b"<?php\n$s = \"define('NOT_REAL', 1);\";";
    let result = find_symbols(content);
    assert!(result.constants.is_empty());
}

#[test]
fn symbols_braced_namespace() {
    let content = br"<?php
namespace Foo {
    class A {}
    function helper(): void {}
    const BAR = 1;
}
namespace Baz {
    class B {}
    function other(): void {}
}
";
    let result = find_symbols(content);
    assert_eq!(result.classes, vec!["Foo\\A", "Baz\\B"]);
    assert_eq!(result.functions, vec!["Foo\\helper", "Baz\\other"]);
    assert_eq!(result.constants, vec!["Foo\\BAR"]);
}

#[test]
fn symbols_function_with_parenthesized_return() {
    // Ensure `function` keyword followed by `(` is treated as closure.
    let content = b"<?php\n$f = function(int $x): int { return $x; };";
    let result = find_symbols(content);
    assert!(result.functions.is_empty());
}

#[test]
fn symbols_define_in_block_comment_ignored() {
    let content = b"<?php\n/* define('NOPE', 1); */\ndefine('YES', 2);";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["YES"]);
}

#[test]
fn symbols_empty_content() {
    let result = find_symbols(b"");
    assert!(result.classes.is_empty());
    assert!(result.functions.is_empty());
    assert!(result.constants.is_empty());
}

#[test]
fn symbols_no_php_symbols() {
    let result = find_symbols(b"<?php\n$x = 1 + 2;\necho $x;");
    assert!(result.classes.is_empty());
    assert!(result.functions.is_empty());
    assert!(result.constants.is_empty());
}

#[test]
fn symbols_heredoc_skipped() {
    let content = br#"<?php
$s = <<<EOT
function fakeFunc(): void {}
define('FAKE', 1);
class FakeClass {}
EOT;
function realFunc(): void {}
"#;
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["realFunc"]);
    assert!(result.classes.is_empty());
    assert!(result.constants.is_empty());
}
