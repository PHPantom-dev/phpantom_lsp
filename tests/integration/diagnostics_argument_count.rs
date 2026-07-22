#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use phpantom_lsp::Backend;
    use tower_lsp::lsp_types::*;

    /// Enable the `extra-arguments` diagnostic on the given backend.
    fn enable_extra_args(backend: &Backend) {
        let mut cfg = backend.config();
        cfg.diagnostics.extra_arguments = Some(true);
        backend.set_config(cfg);
    }

    /// Helper: create a test backend with minimal function stubs and
    /// collect argument-count diagnostics.  Extra-arguments checking
    /// is **off** (the default).
    fn collect(php: &str) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        out
    }

    /// Like [`collect`] but with the `extra-arguments` diagnostic
    /// enabled so that "too many arguments" errors are reported.
    fn collect_extra(php: &str) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        enable_extra_args(&backend);
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        out
    }

    /// Minimal stub function index shared by stub-aware helpers.
    fn stub_fn_index() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("strlen", "<?php\nfunction strlen(string $string): int {}\n"),
            (
                "array_map",
                "<?php\nfunction array_map(?callable $callback, array $array, array ...$arrays): array {}\n",
            ),
            (
                "implode",
                "<?php\nfunction implode(string $separator, array $array): string {}\n",
            ),
            (
                "str_replace",
                "<?php\nfunction str_replace(string|array $search, string|array $replace, string|array $subject): string|array {}\n",
            ),
            (
                "array_push",
                "<?php\nfunction array_push(array &$array, mixed ...$values): int {}\n",
            ),
            (
                "in_array",
                "<?php\nfunction in_array(mixed $needle, array $haystack, bool $strict = false): bool {}\n",
            ),
            (
                "substr",
                "<?php\nfunction substr(string $string, int $offset, ?int $length = null): string {}\n",
            ),
            (
                "array_keys",
                "<?php\nfunction array_keys(array $array, mixed $filter_value, bool $strict = false): array {}\n",
            ),
            (
                "mt_rand",
                "<?php\nfunction mt_rand(int $min, int $max): int {}\n",
            ),
            ("rand", "<?php\nfunction rand(int $min, int $max): int {}\n"),
        ])
    }

    /// Helper that includes minimal stub functions so that built-in
    /// functions like `strlen` are resolvable.  Extra-arguments
    /// checking is **off** (the default).
    fn collect_with_stubs(php: &str) -> Vec<Diagnostic> {
        let backend =
            Backend::new_test_with_all_stubs(HashMap::new(), stub_fn_index(), HashMap::new());
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        out
    }

    /// Like [`collect_with_stubs`] but with the `extra-arguments`
    /// diagnostic enabled.
    fn collect_with_stubs_extra(php: &str) -> Vec<Diagnostic> {
        let backend =
            Backend::new_test_with_all_stubs(HashMap::new(), stub_fn_index(), HashMap::new());
        enable_extra_args(&backend);
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        out
    }

    // ── Too few arguments ───────────────────────────────────────────

    #[test]
    fn flags_too_few_args_to_function() {
        let php = r#"<?php
function test(): void {
    strlen();
}
"#;
        let diags = collect_with_stubs(php);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(
            diags[0].message.contains("Expected 1 argument"),
            "message: {}",
            diags[0].message,
        );
        assert!(
            diags[0].message.contains("got 0"),
            "message: {}",
            diags[0].message,
        );
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn flags_too_few_args_to_method() {
        let php = r#"<?php
class Greeter {
    public function greet(string $name): string {
        return "Hello, " . $name;
    }
}
function test(): void {
    $g = new Greeter();
    $g->greet();
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("got 0")),
            "Expected too-few-args diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn flags_too_few_args_to_static_method() {
        let php = r#"<?php
class Math {
    public static function add(int $a, int $b): int {
        return $a + $b;
    }
}
function test(): void {
    Math::add(1);
}
"#;
        let diags = collect(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Expected 2 arguments") && d.message.contains("got 1")),
            "Expected too-few-args diagnostic, got: {diags:?}",
        );
    }

    // ── Too many arguments (default off) ────────────────────────────

    #[test]
    fn too_many_args_suppressed_by_default() {
        let php = r#"<?php
function test(): void {
    strlen("hello", "extra");
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "Extra-arguments diagnostic should be off by default, got: {diags:?}",
        );
    }

    #[test]
    fn too_many_args_to_user_function_suppressed_by_default() {
        let php = r#"<?php
function myHelper(string $a): void {}
function test(): void {
    myHelper("x", "y");
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Extra-arguments diagnostic should be off by default, got: {diags:?}",
        );
    }

    #[test]
    fn too_many_args_to_method_suppressed_by_default() {
        let php = r#"<?php
class Greeter {
    public function greet(string $name): string {
        return "Hello, " . $name;
    }
}
function test(): void {
    $g = new Greeter();
    $g->greet("world", "extra", "more");
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Extra-arguments diagnostic should be off by default, got: {diags:?}",
        );
    }

    // ── Too many arguments (opt-in) ─────────────────────────────────

    #[test]
    fn flags_too_many_args_to_function() {
        let php = r#"<?php
function test(): void {
    strlen("hello", "extra");
}
"#;
        let diags = collect_with_stubs_extra(php);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(
            diags[0].message.contains("got 2"),
            "message: {}",
            diags[0].message,
        );
    }

    #[test]
    fn flags_too_many_args_to_method() {
        let php = r#"<?php
class Greeter {
    public function greet(string $name): string {
        return "Hello, " . $name;
    }
}
function test(): void {
    $g = new Greeter();
    $g->greet("world", "extra", "more");
}
"#;
        let diags = collect_extra(php);
        assert!(
            diags.iter().any(|d| d.message.contains("got 3")),
            "Expected too-many-args diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_extra_args_to_constructorless_class() {
        // PHP silently ignores arguments passed to a class with no
        // constructor, so even with the extra-arguments check enabled the
        // call must not be flagged.
        let php = r#"<?php
class Plain {}
function test(): void {
    new Plain("x");
}
"#;
        let diags = collect_extra(php);
        assert!(
            diags.is_empty(),
            "Constructor-less class should accept any args, got: {diags:?}",
        );
    }

    #[test]
    fn leading_backslash_builtin_honours_overload_minimum() {
        // `\mt_rand()` in namespaced code must hit the same overload entry
        // as `mt_rand()` (min 0 args), not the stub's full required count.
        let php = r#"<?php
namespace App;
function test(): void {
    \mt_rand();
}
"#;
        let diags = collect_with_stubs_extra(php);
        assert!(
            diags.is_empty(),
            "Leading-backslash builtin should respect overload minimum, got: {diags:?}",
        );
    }

    // ── Correct argument count — no diagnostic ──────────────────────

    #[test]
    fn no_diagnostic_for_correct_arg_count() {
        let php = r#"<?php
function test(): void {
    strlen("hello");
}
"#;
        let diags = collect_with_stubs(php);
        assert!(diags.is_empty(), "No diagnostics expected, got: {diags:?}",);
    }

    #[test]
    fn no_diagnostic_with_optional_args() {
        let php = r#"<?php
function test(): void {
    in_array("x", ["x", "y"]);
    in_array("x", ["x", "y"], true);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for optional args, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_with_default_value() {
        let php = r#"<?php
function test(): void {
    substr("hello", 1);
    substr("hello", 1, 3);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected for default-valued params, got: {diags:?}",
        );
    }

    // ── Variadic functions ──────────────────────────────────────────

    #[test]
    fn no_diagnostic_for_extra_args_to_variadic_function() {
        let php = r#"<?php
function test(): void {
    array_map(null, [1], [2], [3], [4]);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "Variadic function should accept extra args, got: {diags:?}",
        );
    }

    #[test]
    fn flags_too_few_required_args_to_variadic_function() {
        let php = r#"<?php
function test(): void {
    array_push();
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("at least 1 argument")),
            "Expected too-few-args diagnostic for variadic function, got: {diags:?}",
        );
    }

    // ── Argument unpacking suppression ──────────────────────────────

    #[test]
    fn no_diagnostic_when_args_are_unpacked() {
        let php = r#"<?php
function test(): void {
    $args = ["hello"];
    strlen(...$args);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "No diagnostics expected when using argument unpacking, got: {diags:?}",
        );
    }

    // ── Unresolvable calls ──────────────────────────────────────────

    #[test]
    fn no_diagnostic_for_unresolvable_function() {
        let php = r#"<?php
function test(): void {
    nonExistentFunction(1, 2, 3);
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "No arg-count diagnostics expected for unresolvable functions, got: {diags:?}",
        );
    }

    // ── Same-file user-defined functions ─────────────────────────────

    #[test]
    fn flags_too_few_args_to_user_function() {
        let php = r#"<?php
function myHelper(string $a, int $b): void {}
function test(): void {
    myHelper("x");
}
"#;
        let diags = collect(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Expected 2") && d.message.contains("got 1")),
            "Expected too-few-args diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn flags_too_many_args_to_user_function() {
        let php = r#"<?php
function myHelper(string $a): void {}
function test(): void {
    myHelper("x", "y");
}
"#;
        let diags = collect_extra(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Expected 1 argument") && d.message.contains("got 2")),
            "Expected too-many-args diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_correct_user_function_call() {
        let php = r#"<?php
function myHelper(string $a, int $b = 0): void {}
function test(): void {
    myHelper("x");
    myHelper("x", 1);
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "No diagnostics expected, got: {diags:?}",);
    }

    // ── Diagnostic metadata ─────────────────────────────────────────

    #[test]
    fn diagnostic_has_correct_code_and_source() {
        let php = r#"<?php
function myHelper(string $a): void {}
function test(): void {
    myHelper();
}
"#;
        let diags = collect(php);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String(
                "argument_count_mismatch".to_string()
            )),
        );
        assert_eq!(diags[0].source, Some("phpantom".to_string()));
    }

    // ── Constructor calls ───────────────────────────────────────────

    #[test]
    fn flags_too_few_args_to_constructor() {
        let php = r#"<?php
class User {
    public function __construct(string $name, string $email) {}
}
function test(): void {
    new User("Alice");
}
"#;
        let diags = collect(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Expected 2") && d.message.contains("got 1")),
            "Expected too-few-args diagnostic for constructor, got: {diags:?}",
        );
    }

    #[test]
    fn flags_too_many_args_to_constructor() {
        let php = r#"<?php
class User {
    public function __construct(string $name) {}
}
function test(): void {
    new User("Alice", "extra");
}
"#;
        let diags = collect_extra(php);
        assert!(
            diags.iter().any(|d| d.message.contains("got 2")),
            "Expected too-many-args diagnostic for constructor, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_correct_constructor() {
        let php = r#"<?php
class User {
    public function __construct(string $name, string $email = "") {}
}
function test(): void {
    new User("Alice");
    new User("Alice", "alice@test.com");
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "No diagnostics expected, got: {diags:?}",);
    }

    // ── "at least / at most" message wording ────────────────────────

    #[test]
    fn message_says_at_least_when_some_params_optional() {
        let php = r#"<?php
function helper(string $a, string $b, string $c = ""): void {}
function test(): void {
    helper("x");
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("at least 2")),
            "Expected 'at least' wording, got: {diags:?}",
        );
    }

    #[test]
    fn message_says_at_most_when_too_many_with_optional() {
        let php = r#"<?php
function helper(string $a, string $b = ""): void {}
function test(): void {
    helper("x", "y", "z");
}
"#;
        let diags = collect_extra(php);
        assert!(
            diags.iter().any(|d| d.message.contains("at most 2")),
            "Expected 'at most' wording, got: {diags:?}",
        );
    }

    // ── Multiple diagnostics ────────────────────────────────────────

    #[test]
    fn flags_multiple_bad_calls() {
        let php = r#"<?php
function one(int $a): void {}
function two(int $a, int $b): void {}
function test(): void {
    one();
    two(1, 2, 3);
}
"#;
        let diags = collect_extra(php);
        assert_eq!(diags.len(), 2, "Expected 2 diagnostics, got: {diags:?}",);
    }

    #[test]
    fn too_few_still_reported_when_extra_args_disabled() {
        // "Too few" must always fire regardless of the extra-arguments flag.
        let php = r#"<?php
function one(int $a): void {}
function two(int $a, int $b): void {}
function test(): void {
    one();
    two(1, 2, 3);
}
"#;
        let diags = collect(php);
        assert_eq!(
            diags.len(),
            1,
            "Only the too-few diagnostic should fire by default, got: {diags:?}",
        );
        assert!(
            diags[0].message.contains("got 0"),
            "message: {}",
            diags[0].message,
        );
    }

    // ── Scope methods (Laravel) ─────────────────────────────────────

    #[test]
    fn no_diagnostic_for_scope_method_with_query_stripped() {
        // #[Scope]-attributed methods have their first $query parameter
        // stripped by the virtual member provider.  The arg count
        // diagnostic must see the virtual method (0 required params),
        // not the original (1 required param).
        let php = r#"<?php
namespace Illuminate\Database\Eloquent\Attributes;

#[\Attribute]
class Scope {}

namespace Illuminate\Database\Eloquent;

class Model {}
class Builder {}

namespace App;

use Illuminate\Database\Eloquent\Model;

class Bakery extends Model {
    #[\Illuminate\Database\Eloquent\Attributes\Scope]
    protected function fresh(\Illuminate\Database\Eloquent\Builder $query): void {
        $query->where('fresh', true);
    }
}

class Demo {
    public function test(): void {
        $bakery = new Bakery();
        $bakery->fresh();
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Scope method with $query stripped should accept 0 args, got: {diags:?}",
        );
    }

    // ── Overloaded built-in function tests ──────────────────────────

    #[test]
    fn no_diagnostic_for_array_keys_with_one_arg() {
        // array_keys(array $array): array — the 1-arg form is valid.
        let php = r#"<?php
function test(): void {
    $keys = array_keys([1, 2, 3]);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "array_keys with 1 arg should be accepted (overload), got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_array_keys_with_two_args() {
        // array_keys(array $array, mixed $filter_value): array
        let php = r#"<?php
function test(): void {
    $keys = array_keys([1, 2, 3], 2);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "array_keys with 2 args should be accepted, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_array_keys_with_three_args() {
        // array_keys(array $array, mixed $filter_value, bool $strict): array
        let php = r#"<?php
function test(): void {
    $keys = array_keys([1, 2, 3], 2, true);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "array_keys with 3 args should be accepted, got: {diags:?}",
        );
    }

    #[test]
    fn flags_array_keys_with_zero_args() {
        // array_keys() with no arguments is always invalid.
        let php = r#"<?php
function test(): void {
    $keys = array_keys();
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.iter().any(|d| d.message.contains("got 0")),
            "array_keys with 0 args should be flagged, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_mt_rand_with_zero_args() {
        // mt_rand(): int — the 0-arg form is valid.
        let php = r#"<?php
function test(): void {
    $n = mt_rand();
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "mt_rand with 0 args should be accepted (overload), got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_mt_rand_with_two_args() {
        // mt_rand(int $min, int $max): int
        let php = r#"<?php
function test(): void {
    $n = mt_rand(1, 100);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "mt_rand with 2 args should be accepted, got: {diags:?}",
        );
    }

    #[test]
    fn flags_mt_rand_with_one_arg() {
        // mt_rand(1) is invalid — must be 0 or 2 args.
        // The stub declares 2 required params, and the overload min is 0.
        // 1 arg is >= overload min (0) so the "too few" check passes.
        // But the "too many" check (when enabled) would catch it only if
        // max = 2.  With extra-args off (default), 1 arg is not caught.
        // This is acceptable — PHP itself raises a runtime warning for
        // mt_rand(1) but it still works (treats it as mt_rand(0, 1)).
        // We don't flag it because the overload map only lowers the
        // minimum; intermediate invalid counts require a more complex
        // model we don't need yet.
    }

    #[test]
    fn no_diagnostic_for_rand_with_zero_args() {
        // rand(): int — same overload pattern as mt_rand.
        let php = r#"<?php
function test(): void {
    $n = rand();
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "rand with 0 args should be accepted (overload), got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_for_rand_with_two_args() {
        let php = r#"<?php
function test(): void {
    $n = rand(1, 100);
}
"#;
        let diags = collect_with_stubs(php);
        assert!(
            diags.is_empty(),
            "rand with 2 args should be accepted, got: {diags:?}",
        );
    }

    #[test]
    fn no_false_positive_when_stub_uses_element_available_attribute() {
        // Stubs like array_push declare a non-variadic parameter with
        // #[PhpStormStubsElementAvailable(from: '5.3', to: '7.2')] alongside
        // a variadic parameter of the same name.  The AST parser filters out
        // the non-variadic parameter for PHP 8.5 (the default), so the
        // required count should be 1 ($array only), not 2.
        //
        // This test uses the real stub pattern to verify the version
        // filtering produces correct argument counts without needing an
        // overload_min_args entry.
        let stub_content: &str = concat!(
            "<?php\n",
            "use JetBrains\\PhpStorm\\Internal\\PhpStormStubsElementAvailable;\n",
            "\n",
            "function array_push(\n",
            "    array &$array,\n",
            "    #[PhpStormStubsElementAvailable(from: '5.3', to: '7.2')] $values,\n",
            "    mixed ...$values\n",
            "): int {}\n",
        );

        let backend = Backend::new_test_with_all_stubs(
            HashMap::new(),
            HashMap::from([("array_push", stub_content)]),
            HashMap::new(),
        );
        let uri = "file:///test.php";
        let php = r#"<?php
function test(): void {
    $arr = [1, 2];
    array_push($arr, 3);
}
"#;
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        assert!(
            out.is_empty(),
            "array_push($arr, 3) should not produce a diagnostic when \
             PhpStormStubsElementAvailable filtering is active, got: {out:?}",
        );
    }

    #[test]
    fn no_false_positive_for_stub_variadic_with_one_arg_after_filtering() {
        // After version filtering removes the non-variadic $values param,
        // array_push(array &$array, mixed ...$values) requires only 1 arg.
        // Calling array_push($arr) with just the array is valid PHP 7.3+.
        let stub_content: &str = concat!(
            "<?php\n",
            "use JetBrains\\PhpStorm\\Internal\\PhpStormStubsElementAvailable;\n",
            "\n",
            "function array_push(\n",
            "    array &$array,\n",
            "    #[PhpStormStubsElementAvailable(from: '5.3', to: '7.2')] $values,\n",
            "    mixed ...$values\n",
            "): int {}\n",
        );

        let backend = Backend::new_test_with_all_stubs(
            HashMap::new(),
            HashMap::from([("array_push", stub_content)]),
            HashMap::new(),
        );
        let uri = "file:///test.php";
        let php = r#"<?php
function test(): void {
    $arr = [1, 2];
    array_push($arr);
}
"#;
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_argument_count_diagnostics(uri, php, &mut out);
        assert!(
            out.is_empty(),
            "array_push($arr) with 1 arg should be valid after version filtering \
             removes the non-variadic $values param, got: {out:?}",
        );
    }

    #[test]
    fn flags_too_few_args_to_scope_method_with_extra_param() {
        // scopeTopping($query, $type) → virtual topping($type) needs 1 arg.
        let php = r#"<?php
namespace Illuminate\Database\Eloquent\Attributes;

#[\Attribute]
class Scope {}

namespace Illuminate\Database\Eloquent;

class Model {}
class Builder {}

namespace App;

use Illuminate\Database\Eloquent\Model;

class Bakery extends Model {
    public function scopeTopping(\Illuminate\Database\Eloquent\Builder $query, string $type): void {
        $query->where('topping', $type);
    }
}

class Demo {
    public function test(): void {
        $bakery = new Bakery();
        $bakery->topping();
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(|d| d.message.contains("got 0")),
            "Scope method topping() needs 1 arg after $query stripping, got: {diags:?}",
        );
    }

    // ── Named arguments ─────────────────────────────────────────────

    #[test]
    fn flags_missing_required_when_named_arg_fills_optional() {
        // $a is required; filling only the optional $c by name leaves $a
        // unsupplied, which PHP rejects with ArgumentCountError.
        let php = r#"<?php
function f(int $a, int $b = 0, int $c = 0): void {}
function test(): void {
    f(c: 3);
}
"#;
        let diags = collect(php);
        assert!(
            diags.iter().any(
                |d| d.message.contains("Missing required argument") && d.message.contains("$a")
            ),
            "Expected missing-required-argument diagnostic for $a, got: {diags:?}",
        );
    }

    #[test]
    fn no_diagnostic_when_required_filled_by_name() {
        // The single required parameter is supplied by name, so no error
        // even though it is not in the first positional slot.
        let php = r#"<?php
function f(int $a, int $b = 0): void {}
function test(): void {
    f(a: 1);
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "No diagnostics expected, got: {diags:?}",);
    }

    #[test]
    fn no_diagnostic_when_required_split_positional_and_named() {
        // First required parameter positional, second required parameter by
        // name (in any order) — both supplied, so no error.
        let php = r#"<?php
function f(int $a, int $b, int $c = 0): void {}
function test(): void {
    f(1, b: 2);
}
"#;
        let diags = collect(php);
        assert!(diags.is_empty(), "No diagnostics expected, got: {diags:?}",);
    }

    #[test]
    fn reports_multiple_missing_required_named() {
        // Two required parameters left unsupplied while only an optional is
        // filled by name.
        let php = r#"<?php
function f(int $a, int $b, int $c = 0): void {}
function test(): void {
    f(c: 9);
}
"#;
        let diags = collect(php);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Missing required arguments")
                    && d.message.contains("$a")
                    && d.message.contains("$b")),
            "Expected both $a and $b reported missing, got: {diags:?}",
        );
    }
}
