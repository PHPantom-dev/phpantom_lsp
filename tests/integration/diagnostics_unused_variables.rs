#[cfg(test)]
mod tests {
    use phpantom_lsp::Backend;
    use phpantom_lsp::types::PhpVersion;
    use tower_lsp::lsp_types::*;

    /// Helper: create a test backend, open a file, and collect
    /// unused-variable diagnostics.
    fn collect(php: &str) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_unused_variable_diagnostics(uri, php, &mut out);
        out
    }

    /// Helper: same as `collect` but with a specific PHP version.
    fn collect_with_version(php: &str, version: PhpVersion) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        backend.set_php_version(version);
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_unused_variable_diagnostics(uri, php, &mut out);
        out
    }

    // ═══════════════════════════════════════════════════════════════
    // PHP version gating for catch variables
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn skips_catch_variable_on_php7() {
        // Before PHP 8.0, catch variables are mandatory syntax —
        // there is no way to omit them, so flagging is pure noise.
        let diags = collect_with_version(
            r#"<?php
function foo() {
    try {
        doSomething();
    } catch (Exception $e) {
    }
}
"#,
            PhpVersion::new(7, 4),
        );
        assert!(
            diags.is_empty(),
            "catch variable should not be flagged on PHP 7.x: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn flags_catch_variable_on_php8() {
        let diags = collect_with_version(
            r#"<?php
function foo() {
    try {
        doSomething();
    } catch (Exception $e) {
    }
}
"#,
            PhpVersion::new(8, 0),
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$e"));
    }

    // ═══════════════════════════════════════════════════════════════
    // Basic cases
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn flags_unused_variable_in_function() {
        let diags = collect(
            r#"<?php
function foo() {
    $x = 1;
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$x"));
        assert!(diags[0].message.contains("Unused variable"));
    }

    #[test]
    fn no_diagnostic_when_variable_is_read() {
        let diags = collect(
            r#"<?php
function foo() {
    $x = 1;
    echo $x;
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn no_diagnostic_when_variable_is_used_in_dynamic_property_access() {
        let diags = collect(
            r#"<?php
function foo(object $message, string $type) {
    $attribute = strtolower($type);
    return $message->{$attribute};
}
"#,
        );
        assert!(
            diags.is_empty(),
            "dynamic property selector should count as a read"
        );
    }

    #[test]
    fn no_diagnostic_when_variable_is_used_as_dynamic_method_name() {
        let diags = collect(
            r#"<?php
function foo(object $response, string $value, bool $cond) {
    $assertion = $cond ? 'assertSee' : 'assertDontSee';
    $response->{$assertion}($value);
}
"#,
        );
        assert!(
            diags.is_empty(),
            "braced dynamic method-name selector should count as a read, got: {diags:?}"
        );
    }

    #[test]
    fn no_diagnostic_when_variable_is_used_as_nullsafe_dynamic_method_name() {
        let diags = collect(
            r#"<?php
function foo(?object $response, string $value, string $method) {
    $response?->{$method}($value);
}
"#,
        );
        assert!(
            diags.is_empty(),
            "null-safe dynamic method-name selector should count as a read, got: {diags:?}"
        );
    }

    #[test]
    fn no_diagnostic_when_variable_is_used_as_static_dynamic_method_name() {
        let diags = collect(
            r#"<?php
function foo(string $value, string $method) {
    Cls::{$method}($value);
}
"#,
        );
        assert!(
            diags.is_empty(),
            "static dynamic method-name selector should count as a read, got: {diags:?}"
        );
    }

    #[test]
    fn skips_unused_parameter() {
        // Parameters are intentionally not flagged until suppression
        // support is available — callbacks, interface implementations,
        // and framework conventions often require specific signatures.
        let diags = collect(
            r#"<?php
function foo($x) {
    return 1;
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn no_diagnostic_for_used_parameter() {
        let diags = collect(
            r#"<?php
function foo($x) {
    return $x;
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn no_diagnostic_for_underscore_prefix() {
        let diags = collect(
            r#"<?php
function foo($_unused) {
    $_ = 1;
    $_skip = 2;
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn no_diagnostic_for_this() {
        let diags = collect(
            r#"<?php
class Foo {
    public function bar() {
        $x = $this->value;
        echo $x;
    }
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn flags_unused_in_method() {
        let diags = collect(
            r#"<?php
class Foo {
    public function bar() {
        $unused = 42;
    }
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$unused"));
    }

    #[test]
    fn no_diagnostic_for_global_scope() {
        let diags = collect(
            r#"<?php
$x = 1;
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn no_diagnostic_for_variable_read_in_arrow_function() {
        let diags = collect(
            r#"<?php
function foo() {
    $x = 1;
    $fn = fn() => $x;
    echo $fn;
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn no_diagnostic_for_variable_captured_by_closure() {
        let diags = collect(
            r#"<?php
function foo() {
    $x = 1;
    $fn = function() use ($x) {
        echo $x;
    };
    echo $fn;
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn flags_unused_foreach_binding() {
        let diags = collect(
            r#"<?php
function foo($items) {
    foreach ($items as $key => $value) {
        echo $value;
    }
}
"#,
        );
        // $key is unused
        assert!(diags.iter().any(|d| d.message.contains("$key")));
    }

    #[test]
    fn no_diagnostic_for_byref_out_param() {
        let diags = collect(
            r#"<?php
function test(string $domain): bool {
    $dummy = [];
    return getmxrr($domain, $dummy);
}
"#,
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("$dummy")),
            "Got unexpected diagnostic for $dummy: {:?}",
            diags
        );
    }

    #[test]
    fn no_diagnostic_for_foreach_by_reference_binding() {
        let diags = collect(
            r#"<?php
function test() {
    $values = [1, 2, 3];
    foreach ($values as &$value) {
        $value = 4;
    }
    var_dump($values);
}
"#,
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("$value")),
            "Got unexpected diagnostic for $value: {:?}",
            diags
        );
    }

    #[test]
    fn no_diagnostic_for_underscore_foreach_key() {
        let diags = collect(
            r#"<?php
function foo($items) {
    foreach ($items as $_ => $value) {
        echo $value;
    }
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn diagnostic_has_correct_code_and_tags() {
        let diags = collect(
            r#"<?php
function foo() {
    $x = 1;
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("unused_variable".to_string()))
        );
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(diags[0].tags, Some(vec![DiagnosticTag::UNNECESSARY]));
        assert_eq!(diags[0].source, Some("phpantom".to_string()));
    }

    #[test]
    fn no_diagnostic_for_compound_assignment_read() {
        let diags = collect(
            r#"<?php
function foo() {
    $x = 0;
    $x += 1;
    echo $x;
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn flags_multiple_unused_variables() {
        let diags = collect(
            r#"<?php
function foo() {
    $a = 1;
    $b = 2;
    $c = 3;
    echo $c;
}
"#,
        );
        assert_eq!(diags.len(), 2);
        let msgs: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(msgs.iter().any(|m| m.contains("$a")));
        assert!(msgs.iter().any(|m| m.contains("$b")));
    }

    #[test]
    fn no_diagnostic_for_superglobals() {
        let diags = collect(
            r#"<?php
function foo() {
    $x = $_GET['id'];
    echo $x;
}
"#,
        );
        assert!(diags.is_empty());
    }

    // ═══════════════════════════════════════════════════════════════
    // Catch variables
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn flags_unused_catch_variable() {
        let diags = collect(
            r#"<?php
function foo() {
    try {
        something();
    } catch (\Exception $e) {
        log("error");
    }
}
"#,
        );
        assert!(diags.iter().any(|d| d.message.contains("$e")));
    }

    #[test]
    fn no_diagnostic_for_used_catch_variable() {
        let diags = collect(
            r#"<?php
function foo() {
    try {
        something();
    } catch (\Exception $e) {
        log($e->getMessage());
    }
}
"#,
        );
        assert!(!diags.iter().any(|d| d.message.contains("$e")));
    }

    #[test]
    fn no_duplicate_diagnostic_for_catch_variable() {
        let diags = collect(
            r#"<?php
function foo() {
    try {
        something();
    } catch (\Exception $e) {
        log("error");
    }
}
"#,
        );
        let e_diags: Vec<_> = diags.iter().filter(|d| d.message.contains("$e")).collect();
        assert_eq!(
            e_diags.len(),
            1,
            "should have exactly one diagnostic for $e, got: {:?}",
            e_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // Constructor promotion
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_promoted_constructor_parameter() {
        let diags = collect(
            r#"<?php
class Address {
    public function __construct(
        public readonly string $street,
        public readonly string $city,
        public readonly string $country_code,
    ) {}
}
"#,
        );
        assert!(
            diags.is_empty(),
            "promoted constructor parameters should not be flagged: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_promoted_mixed_visibility() {
        let diags = collect(
            r#"<?php
class Foo {
    public function __construct(
        private string $name,
        protected int $age,
        public bool $active = true,
    ) {}
}
"#,
        );
        assert!(
            diags.is_empty(),
            "promoted parameters with any visibility should not be flagged: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn skips_non_promoted_constructor_parameter() {
        // Unused (non-promoted) constructor parameters are intentionally
        // not flagged: reporting unused parameters is only useful once
        // users have a way to suppress the warning on parameters they
        // must keep for interface or signature compatibility.
        let diags = collect(
            r#"<?php
class Foo {
    public function __construct(string $name) {}
}
"#,
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn mixed_promoted_and_non_promoted() {
        // Non-promoted parameter $unused is still a parameter, so it
        // should not be flagged until suppression support exists.
        let diags = collect(
            r#"<?php
class Foo {
    public function __construct(
        private string $name,
        string $unused,
    ) {}
}
"#,
        );
        assert!(diags.is_empty());
    }

    // ═══════════════════════════════════════════════════════════════
    // Closure callback parameters
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_closure_callback_parameter_used() {
        let diags = collect(
            r#"<?php
function foo() {
    $result = array_map(function ($item) {
        return $item * 2;
    }, [1, 2, 3]);
    echo $result;
}
"#,
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("$item")),
            "closure param used in body should not be flagged: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_closure_callback_parameter_method_call() {
        let diags = collect(
            r#"<?php
function foo($query) {
    $query->where(function ($q) {
        $q->where('active', true);
    });
}
"#,
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("$q")),
            "closure param used for method call should not be flagged: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_join_clause_callback() {
        let diags = collect(
            r#"<?php
class Repo {
    public function getItems() {
        return $this->model
            ->leftJoin('other', function ($join) {
                $join->on('a.id', '=', 'b.a_id')
                    ->where('b.active', true);
            })
            ->get();
    }
}
"#,
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("$join")),
            "join closure param should not be flagged: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn skips_unused_closure_parameter() {
        // Closure parameters are skipped for the same reason as
        // regular parameters — no suppression support yet.
        let diags = collect(
            r#"<?php
function foo() {
    $result = array_map(function ($item) {
        return 42;
    }, [1, 2, 3]);
    echo $result;
}
"#,
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("$item")),
            "closure params should not be flagged without suppression support: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_false_positive_for_closure_param_in_outer_scope() {
        // The parent function should NOT flag $q as its own unused var.
        let diags = collect(
            r#"<?php
function foo($query) {
    $query->where(function ($q) {
        $q->where('active', true);
    });
}
"#,
        );
        // $q should not appear in any diagnostic
        assert!(
            !diags.iter().any(|d| d.message.contains("$q")),
            "closure param should not leak to outer scope: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_by_reference_capture_written_in_closure() {
        // A variable captured by reference and written inside the
        // closure is not unused: the write propagates to the outer
        // scope through the reference.
        let diags = collect(
            r#"<?php
function foo() {
    $lastId = null;
    $fn = function () use (&$lastId): void { $lastId = 5; };
    $fn();
    return $lastId;
}
"#,
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("$lastId")),
            "by-reference capture should not be flagged unused: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_by_reference_capture_only_written() {
        // Even when the outer variable is never read after the closure,
        // the by-reference capture counts as a use (conservatively).
        let diags = collect(
            r#"<?php
function foo(array $items) {
    $total = 0;
    array_walk($items, function ($item) use (&$total): void {
        $total += $item;
    });
    echo $total;
}
"#,
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("$total")),
            "by-reference capture accumulator should not be flagged: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn still_flags_by_value_capture_reassigned_but_unread() {
        // A by-value capture reassigned inside the closure but never
        // read there is a genuine dead write — the reassignment does
        // not escape the closure, so it should still be flagged.
        let diags = collect(
            r#"<?php
function foo() {
    $x = 1;
    $fn = function () use ($x): void { $x = 5; };
    $fn();
}
"#,
        );
        assert!(
            diags.iter().any(|d| d.message.contains("$x")),
            "dead by-value capture write should still be flagged: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // List destructuring
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_list_destructured_used_variable() {
        let diags = collect(
            r#"<?php
function foo() {
    [$fileId, $filePath] = upload();
    echo $filePath;
}
"#,
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("$filePath")),
            "used list-destructured variable should not be flagged: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn flags_unused_list_destructured_variable() {
        let diags = collect(
            r#"<?php
function foo() {
    [$fileId, $filePath] = upload();
    echo $filePath;
}
"#,
        );
        assert!(
            diags.iter().any(|d| d.message.contains("$fileId")),
            "unused list-destructured variable should be flagged: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // Arrow function parameters
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_false_positive_for_arrow_fn_param_in_outer_scope() {
        let diags = collect(
            r#"<?php
function foo() {
    $fn = fn($x) => $x * 2;
    echo $fn;
}
"#,
        );
        // Only $fn should not be flagged; $x is used inside the arrow fn.
        // $x should not appear as an outer-scope unused variable.
        let x_diags: Vec<_> = diags.iter().filter(|d| d.message.contains("$x")).collect();
        assert!(
            x_diags.is_empty(),
            "arrow fn param should not leak to outer scope: {:?}",
            x_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_duplicate_for_nested_catch_same_variable_name() {
        // Two catch blocks using the same variable name should each
        // produce at most one diagnostic, not three.
        let diags = collect(
            r#"<?php
function foo() {
    try {
        try {
            doSomething();
        } catch (DuplicateOrder $exception) {
        }
    } catch (Throwable $exception) {
    }
    return true;
}
"#,
        );
        let exception_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("$exception"))
            .collect();
        assert_eq!(
            exception_diags.len(),
            2,
            "expected exactly 2 diagnostics for $exception (one per catch), got {}: {:?}",
            exception_diags.len(),
            exception_diags
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
        // Each diagnostic should be on a different line.
        assert_ne!(
            exception_diags[0].range.start.line, exception_diags[1].range.start.line,
            "diagnostics should be on different lines"
        );
    }

    #[test]
    fn compact_suppresses_unused_variable() {
        let diags = collect(
            r#"<?php
function foo() {
    $breadcrumb = 'home';
    $unused = 'x';
    return view('page', compact('breadcrumb'));
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$unused"));
    }

    #[test]
    fn compact_with_array_argument_suppresses_unused_variables() {
        let diags = collect(
            r#"<?php
function foo() {
    $activeEvents = 'a';
    $showDefault = true;
    $unused = 'x';
    return compact([
        'activeEvents',
        'showDefault',
    ]);
}
"#,
        );
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(diags[0].message.contains("$unused"));
    }

    #[test]
    fn compact_with_nested_array_argument_suppresses_unused_variables() {
        let diags = collect(
            r#"<?php
function foo() {
    $a = 1;
    $b = 2;
    $c = 3;
    return compact('a', ['b', ['c']]);
}
"#,
        );
        assert_eq!(diags.len(), 0, "got: {diags:?}");
    }

    #[test]
    fn compact_in_method_suppresses_unused_variable() {
        let diags = collect(
            r#"<?php
class Ctrl {
    public function show() {
        $brand = 'x';
        $series = 'y';
        return view('page', compact('brand', 'series'));
    }
}
"#,
        );
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn get_defined_vars_suppresses_all_unused_in_function() {
        let diags = collect(
            r#"<?php
function foo() {
    $a = 1;
    $b = 2;
    $c = 3;
    return get_defined_vars();
}
"#,
        );
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn get_defined_vars_suppresses_all_unused_in_method() {
        let diags = collect(
            r#"<?php
class Ctrl {
    public function show() {
        $x = 1;
        $y = 2;
        var_dump(get_defined_vars());
    }
}
"#,
        );
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn get_defined_vars_does_not_suppress_nested_closure_unused_variables() {
        let diags = collect(
            r#"<?php
function foo() {
    $outer = 1;
    get_defined_vars();

    $fn = function () {
        $inner = 2;
    };

    echo $fn;
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$inner"));
    }

    #[test]
    fn get_defined_vars_inside_array_expression_suppresses_outer_unused_variables() {
        let diags = collect(
            r#"<?php
function foo() {
    $a = 1;
    $b = 2;
    return ['vars' => get_defined_vars()];
}
"#,
        );
        assert_eq!(diags.len(), 0);
    }
}
