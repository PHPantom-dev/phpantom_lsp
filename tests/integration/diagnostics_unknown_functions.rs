#[cfg(test)]
mod tests {
    use crate::common::collect_diagnostics_with;
    use std::collections::HashMap;

    use phpantom_lsp::Backend;
    use tower_lsp::lsp_types::*;

    /// Helper: create a test backend, open a file, and collect
    /// unknown-function diagnostics.
    fn collect(php: &str) -> Vec<Diagnostic> {
        collect_diagnostics_with(
            &Backend::new_test(),
            php,
            Backend::collect_unknown_function_diagnostics,
        )
    }

    /// Helper that includes a minimal stub function index so that
    /// built-in functions like `strlen` are resolvable.
    fn collect_with_stubs(php: &str) -> Vec<Diagnostic> {
        let stub_fn_index: HashMap<&'static str, &'static str> = HashMap::from([
            (
                "strlen",
                "<?php\n/** @return int */\nfunction strlen(string $string): int {}\n",
            ),
            (
                "array_map",
                "<?php\nfunction array_map(?callable $callback, array $array, array ...$arrays): array {}\n",
            ),
        ]);
        let backend =
            Backend::new_test_with_all_stubs(HashMap::new(), stub_fn_index, HashMap::new());
        collect_diagnostics_with(&backend, php, Backend::collect_unknown_function_diagnostics)
    }

    #[test]
    fn flags_unknown_function_call() {
        let php = r#"<?php
function test(): void {
    doesntExist();
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("doesntExist")),
            "Expected unknown function diagnostic for doesntExist(), got: {:?}",
            diags,
        );
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn flags_unknown_function_with_args() {
        let php = r#"<?php
function test(): void {
    alsoFake(1, 2, 3);
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("alsoFake")),
            "Expected unknown function diagnostic for alsoFake(), got: {:?}",
            diags,
        );
    }

    #[test]
    fn flags_unknown_function_assigned_to_variable() {
        let php = r#"<?php
function test(): void {
    $result = noSuchFn();
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("noSuchFn")),
            "Expected unknown function diagnostic for noSuchFn(), got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_builtin_function() {
        let php = r#"<?php
function test(): void {
    $len = strlen("hello");
    $arr = array_map(fn($x) => $x, [1,2,3]);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for built-in functions, got: {:?}",
            diags,
        );
    }

    /// PHP function names are case-insensitive (B25): `STRLEN()` calls
    /// the built-in `strlen` and must not be flagged.
    #[test]
    fn no_diagnostic_for_differently_cased_builtin_function() {
        let php = r#"<?php
function test(): void {
    $len = STRLEN("hello");
    $arr = Array_Map(fn($x) => $x, [1,2,3]);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for differently-cased built-ins, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_language_constructs() {
        let php = r#"<?php
function test(): void {
    isset($x);
    unset($x);
    empty($x);
    eval('');
    exit(0);
    die(1);
    print("hello");
    assert(true);
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for language constructs, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_same_file_function() {
        let php = r#"<?php
function myHelper(): string {
    return "ok";
}
function test(): void {
    myHelper();
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for same-file function, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_function_definition_itself() {
        let php = r#"<?php
function myHelper(): string {
    return "ok";
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for function definitions, got: {:?}",
            diags,
        );
    }

    #[test]
    fn diagnostic_has_correct_code_and_source() {
        let php = r#"<?php
function test(): void {
    fakeFunc();
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("unknown_function".to_string())),
        );
        assert_eq!(diags[0].source, Some("phpantom".to_string()));
    }

    #[test]
    fn flags_multiple_unknown_functions() {
        let php = r#"<?php
function test(): void {
    fake1();
    fake2();
    fake3();
}
"#;
        let diags = collect(php);
        assert_eq!(
            diags.len(),
            3,
            "Expected 3 unknown function diagnostics, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_use_statement_lines() {
        // `use function` lines should not be flagged.
        let php = r#"<?php
use function Some\Namespace\myFunc;
function test(): void {
    strlen("ok");
}
"#;
        // Use stubs-free backend: `strlen` is unknown but we're testing
        // that the `use function` line itself is not flagged.  `strlen`
        // will be flagged — filter it out.
        let diags = collect(php);
        assert!(
            !diags.iter().any(|d| d.message.contains("myFunc")),
            "No diagnostic expected for function name on use-statement line, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_compact() {
        let php = r#"<?php
function test(): void {
    $a = 1;
    $b = 2;
    $result = compact('a', 'b');
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for compact(), got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_use_function_imported_call() {
        // Simulate the PHPUnit pattern: a namespaced function is defined
        // in one file and imported via `use function` in the consumer.
        let backend = Backend::new_test();

        // Define a namespaced function in another file.
        let def_uri = "file:///vendor/phpunit/Functions.php";
        let def_php = r#"<?php
namespace PHPUnit\Framework;

function assertSame(mixed $expected, mixed $actual, string $message = ''): void {}
"#;
        backend.update_ast(def_uri, def_php);

        // Consumer file uses `use function` to import it.
        let uri = "file:///tests/MyTest.php";
        let php = r#"<?php
namespace Tests\Unit;

use function PHPUnit\Framework\assertSame;

class MyTest {
    public function testSomething(): void {
        assertSame(1, 1);
    }
}
"#;
        backend.update_ast(uri, php);

        let mut out = Vec::new();
        backend.collect_unknown_function_diagnostics(uri, php, &mut out);
        assert!(
            out.is_empty(),
            "No diagnostics expected for use-function imported call, got: {:?}",
            out,
        );
    }

    #[test]
    fn no_diagnostic_for_use_function_imported_polyfill() {
        // Functions inside `if (!function_exists(...))` guards are
        // marked as polyfills but should still be resolvable when
        // they don't shadow a stub.
        let backend = Backend::new_test();

        let def_uri = "file:///vendor/phpunit/Functions.php";
        let def_php = r#"<?php
namespace PHPUnit\Framework;

if (!function_exists('PHPUnit\Framework\assertSame')) {
    function assertSame(mixed $expected, mixed $actual, string $message = ''): void {}
}
"#;
        backend.update_ast(def_uri, def_php);

        let uri = "file:///tests/MyTest.php";
        let php = r#"<?php
namespace Tests\Unit;

use function PHPUnit\Framework\assertSame;

class MyTest {
    public function testSomething(): void {
        assertSame(1, 1);
    }
}
"#;
        backend.update_ast(uri, php);

        let mut out = Vec::new();
        backend.collect_unknown_function_diagnostics(uri, php, &mut out);
        assert!(
            out.is_empty(),
            "No diagnostics expected for use-function imported polyfill, got: {:?}",
            out,
        );
    }

    #[test]
    fn no_diagnostic_for_use_function_importing_type_keyword_name() {
        // Functions whose name coincides with a PHP type keyword
        // (e.g. `int`, `string`, `bool`) must still be resolvable
        // when imported via `use function`.
        let backend = Backend::new_test();

        let def_uri = "file:///vendor/psl/Type/int.php";
        let def_php = r#"<?php
namespace Psl\Type;

function int(): TypeInterface {
    return new Internal\IntType();
}
"#;
        backend.update_ast(def_uri, def_php);

        let def_uri2 = "file:///vendor/psl/Type/vec.php";
        let def_php2 = r#"<?php
namespace Psl\Type;

function vec(TypeInterface $valueType): TypeInterface {
    return new Internal\VecType($valueType);
}
"#;
        backend.update_ast(def_uri2, def_php2);

        let uri = "file:///src/Test.php";
        let php = r#"<?php
namespace App;

use function Psl\Type\vec;
use function Psl\Type\int;

class Test {
    public function a(): void {
        vec(
            int()
        )->coerce(1);
    }
}
"#;
        backend.update_ast(uri, php);

        let mut out = Vec::new();
        backend.collect_unknown_function_diagnostics(uri, php, &mut out);
        assert!(
            out.is_empty(),
            "No diagnostics expected for use-function imported calls named after type keywords, got: {:?}",
            out,
        );
    }

    #[test]
    fn no_diagnostic_when_guarded_by_function_exists() {
        let php = r#"<?php
function test(): void {
    if (function_exists('maybe')) {
        maybe();
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for function guarded by function_exists(), got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_when_negated_function_exists_with_early_return() {
        let php = r#"<?php
function test(): void {
    if (!function_exists('maybe')) return;
    maybe();
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected after negated function_exists with early return, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_when_negated_function_exists_with_throw() {
        let php = r#"<?php
function test(): void {
    if (!function_exists('maybe')) {
        throw new \RuntimeException('missing');
    }
    maybe();
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected after negated function_exists with throw, got: {:?}",
            diags,
        );
    }

    #[test]
    fn still_flags_when_negated_without_early_exit() {
        // Negated check without early exit is a polyfill definition pattern,
        // should NOT suppress diagnostics for the function elsewhere.
        let php = r#"<?php
function test(): void {
    if (!function_exists('maybe')) {
        // just logging, no return/throw
        echo 'not found';
    }
    maybe();
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("maybe")),
            "Expected unknown function diagnostic for maybe() without early exit guard, got: {:?}",
            diags,
        );
    }

    // ─── Unqualified `@see` references ──────────────────────────────────

    #[test]
    fn no_diagnostic_for_see_naming_own_method_from_class_docblock() {
        let php = r#"<?php
/**
 * Decides what a test covers. {@see covers()} draws that line.
 */
final class QodanaChecker
{
    public function covers(int $case): bool { return true; }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "A class docblock naming one of its own methods is not a global function call, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_see_naming_own_method_from_method_docblock() {
        let php = r#"<?php
final class QodanaChecker
{
    /**
     * @see covers
     */
    public function report(): void {}

    public function covers(int $case): bool { return true; }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "A method docblock naming a sibling method is not a global function call, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_see_naming_inherited_method() {
        let php = r#"<?php
abstract class BaseChecker
{
    public function covers(int $case): bool { return true; }
}

/**
 * {@see covers()} is inherited, not global.
 */
final class QodanaChecker extends BaseChecker {}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "An inherited method reached through @see should resolve, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_see_naming_own_property() {
        let php = r#"<?php
/**
 * @see cases
 */
final class QodanaChecker
{
    public array $cases = [];
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "A property reached through @see should resolve, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_see_naming_nothing_in_scope() {
        let php = r#"<?php
/**
 * {@see covers()} names nothing.
 */
final class QodanaChecker
{
    public function report(): void {}
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "@see legally carries prose and naming suggestions; unresolvable targets are not errors, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_see_in_a_docblock_that_documents_no_class() {
        let php = r#"<?php
/**
 * {@see covers()} names nothing.
 */
function report(): void {}

final class QodanaChecker
{
    public function covers(int $case): bool { return true; }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "@see legally carries prose and naming suggestions; unresolvable targets are not errors, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_see_with_free_text_prose() {
        let php = r#"<?php
class Widget
{
    /**
     * @see the vendor manual "Widget Format v2" on page 3
     */
    public function encode(string $content): string
    {
        return $content;
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "@see with free-text prose should not produce diagnostics, got: {:?}",
            diags,
        );
    }

    #[test]
    fn still_flags_covers_tag_naming_a_test_class_method() {
        // PHPUnit spells "a member of this class" as `@covers ::name`; a
        // bare name is always a global function.
        let php = r#"<?php
final class QodanaCheckerTest
{
    /**
     * @covers covers
     */
    public function testCovers(): void {}

    public function covers(int $case): bool { return true; }
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("covers")),
            "@covers names a global function, not a method of the test class, got: {:?}",
            diags,
        );
    }

    /// A file may declare several `namespace` blocks. A call in the second
    /// block resolves against that block's namespace, so a function defined
    /// there (in another file) must not be reported as missing.
    #[test]
    fn function_from_second_namespace_block_resolves() {
        let backend = Backend::new_test();
        backend.update_ast(
            "file:///other.php",
            "<?php\nnamespace App;\nfunction helper(int $n): void {}\n",
        );
        let php = r#"<?php
namespace App\Other;

class Marker {}

namespace App;

function run(): void {
    helper(1);
    missingFn();
}
"#;
        backend.update_ast("file:///test.php", php);
        let mut diags = Vec::new();
        backend.collect_unknown_function_diagnostics("file:///test.php", php, &mut diags);
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages, vec!["Function 'missingFn' not found"]);
    }

    // ─── Qualified function names ───────────────────────────────────────

    #[test]
    fn qualified_name_resolves_through_a_class_import() {
        // PHP resolves the first segment of a qualified name against the
        // import table, so `Ip\isIpAllowed()` under `use Core\Ip;` means
        // `Core\Ip\isIpAllowed`.  The call and the declaration sit in
        // different namespace blocks, which is what makes the per-offset
        // resolution the only thing that can answer: a single file
        // namespace describes one block, not both.
        let php = r#"<?php
namespace Core\Ip;

function isIpAllowed(string $ip): bool { return true; }

namespace App;

use Core\Ip;

Ip\isIpAllowed('203.0.113.1');
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "`Ip\\isIpAllowed()` under `use Core\\Ip;` names a declared function, got: {:?}",
            diags,
        );
    }

    #[test]
    fn qualified_name_is_not_expanded_through_a_function_import() {
        // A `use function` import binds a function name, and PHP never
        // lets one prefix a qualified name: `Lib\helper()` here means
        // `App\Lib\helper`, which nothing declares.  The declaration of
        // `Vendor\factory\helper` is the point of the test -- a resolver
        // that expands the first segment through the shared import table
        // finds it and stays silent, which is exactly the wrong answer.
        let php = r#"<?php
namespace Vendor\factory;

function helper(): void {}

namespace App;

use function Vendor\factory as Lib;

Lib\helper();
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("helper")),
            "`Lib\\helper()` resolves to `App\\Lib\\helper`, which is undeclared, got: {:?}",
            diags,
        );
    }
}
