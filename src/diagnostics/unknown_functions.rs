//! Unknown function diagnostics.
//!
//! Walk the precomputed [`SymbolMap`] for a file and flag every
//! `FunctionCall` span (that is not a definition) where the function
//! cannot be resolved through any of PHPantom's resolution phases
//! (use-map → namespace-qualified → global_functions → stubs →
//! autoload files).
//!
//! Diagnostics use `Severity::Error` because calling a function that
//! does not exist crashes at runtime with "Call to undefined function".
//!
//! Suppression rules:
//! - Function *definitions* are skipped (`is_definition: true`).
//! - Calls on `use` statement lines are skipped (import declarations).
//! - PHP built-in language constructs that look like function calls
//!   (`isset`, `unset`, `empty`, `eval`, `exit`, `die`, `list`,
//!   `print`, `echo`, `include`, `require`, etc.) are skipped.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolKind;

use super::helpers::{
    compute_existence_guards, compute_use_line_ranges, is_offset_in_ranges, make_diagnostic,
};

/// Diagnostic code used for unknown-function diagnostics.
pub(crate) const UNKNOWN_FUNCTION_CODE: &str = "unknown_function";

/// PHP language constructs that syntactically look like function calls
/// but are not actual functions and should never be flagged.
const LANGUAGE_CONSTRUCTS: &[&str] = &[
    "isset",
    "unset",
    "empty",
    "eval",
    "exit",
    "die",
    "list",
    "print",
    "echo",
    "include",
    "include_once",
    "require",
    "require_once",
    "array",
    "compact",
    "extract",
    "assert",
    "function_exists",
    "class_exists",
    "method_exists",
    "property_exists",
    "defined",
];

impl Backend {
    /// Collect unknown-function diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_unknown_function_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        // ── Gather context under locks ──────────────────────────────────
        let symbol_map = {
            let maps = self.symbol_maps.read();
            match maps.get(uri) {
                Some(sm) => sm.clone(),
                None => return,
            }
        };

        let file_use_map: HashMap<String, String> = self.file_use_map(uri);

        let file_namespace: Option<String> = self.first_file_namespace(uri);

        // ── Compute byte ranges of `use` statement lines ────────────────
        let use_line_ranges = compute_use_line_ranges(content);

        // ── Compute existence guards ────────────────────────────────────
        let existence_guards = compute_existence_guards(content);

        // ── Collect local function definition names ─────────────────────
        // Functions defined in the same file are always resolvable even
        // before they appear in global_functions (hoisting).  Collect
        // both short names and FQN forms.
        let local_function_names: Vec<String> = symbol_map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::FunctionCall {
                    name,
                    is_definition: true,
                } => {
                    let mut names = vec![name.clone()];
                    if let Some(ref ns) = file_namespace {
                        names.push(format!("{}\\{}", ns, name));
                    }
                    Some(names)
                }
                _ => None,
            })
            .flatten()
            .collect();

        // ── Walk every symbol span ──────────────────────────────────────
        for span in &symbol_map.spans {
            let name = match &span.kind {
                SymbolKind::FunctionCall {
                    name,
                    is_definition: false,
                } => name,
                _ => continue,
            };

            // Skip spans on `use` statement lines.
            if is_offset_in_ranges(span.start, &use_line_ranges) {
                continue;
            }

            // Skip PHP language constructs.
            if LANGUAGE_CONSTRUCTS
                .iter()
                .any(|&c| c.eq_ignore_ascii_case(name))
            {
                continue;
            }

            // Skip names that match a local function definition.
            if local_function_names.iter().any(|n| n == name) {
                continue;
            }

            // ── Attempt resolution through all phases ───────────────────
            if self
                .resolve_function_name(name, &file_use_map, &file_namespace)
                .is_some()
            {
                continue;
            }

            // ── Skip functions guarded by function_exists() ──────────────
            if existence_guards.is_function_guarded(name, span.start) {
                continue;
            }

            // ── Function is unresolved — emit diagnostic ────────────────
            let range = match self.offset_range_to_lsp_range(
                uri,
                content,
                span.start as usize,
                span.end as usize,
            ) {
                Some(r) => r,
                None => continue,
            };

            let message = format!("Function '{}' not found", name);

            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::ERROR,
                UNKNOWN_FUNCTION_CODE,
                message,
            ));
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a test backend, open a file, and collect
    /// unknown-function diagnostics.
    fn collect(php: &str) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_unknown_function_diagnostics(uri, php, &mut out);
        out
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
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_unknown_function_diagnostics(uri, php, &mut out);
        out
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
}
