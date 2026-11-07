use crate::common::{collect_diagnostics_with, message_text};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

/// `backend` with the diagnostic switched on.
fn enabled(backend: Backend) -> Backend {
    let mut cfg = backend.config();
    cfg.diagnostics.unused_parameters = Some(true);
    backend.set_config(cfg);
    backend
}

/// The messages of the unused-parameter diagnostics `php` gets, each
/// checked for its code, severity and tag.
fn collect(php: &str) -> Vec<String> {
    collect_diagnostics_with(
        &enabled(Backend::new_test()),
        php,
        Backend::collect_unused_parameter_diagnostics,
    )
    .iter()
    .map(|d| {
        assert_eq!(d.code, Some(Code::String("unused_parameter".to_string())));
        assert_eq!(d.severity, Some(DiagnosticSeverity::Hint));
        assert_eq!(d.tags, Some(vec![DiagnosticTag::Unnecessary]));
        message_text(&d.message).to_string()
    })
    .collect()
}

fn assert_none(php: &str) {
    let diags = collect(php);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

#[test]
fn off_by_default() {
    let diags = collect_diagnostics_with(
        &Backend::new_test(),
        "<?php\nfunction f($a) { return 1; }\n",
        Backend::collect_unused_parameter_diagnostics,
    );
    assert!(diags.is_empty(), "{diags:?}");
}

// ─── Functions ──────────────────────────────────────────────────────────────

#[test]
fn function_parameter_never_read_is_reported() {
    let php = r#"<?php
function greet(string $name, bool $loud) {
    return "Hello $name";
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$loud'"]);
}

#[test]
fn diagnostic_sits_on_the_parameter_name() {
    let php = "<?php\nfunction f($used, $unused) { return $used; }\n";
    let diags = collect_diagnostics_with(
        &enabled(Backend::new_test()),
        php,
        Backend::collect_unused_parameter_diagnostics,
    );
    assert_eq!(diags.len(), 1);
    let col = php.lines().nth(1).unwrap().find("$unused").unwrap() as u32;
    assert_eq!(diags[0].range.start, Position::new(1, col));
    assert_eq!(
        diags[0].range.end,
        Position::new(1, col + "$unused".len() as u32)
    );
}

#[test]
fn every_trailing_unused_parameter_is_reported() {
    let php = r#"<?php
function f($a, $b, $c) {
    return $a;
}
"#;
    let mut diags = collect(php);
    diags.sort();
    assert_eq!(
        diags,
        vec!["Unused parameter '$b'", "Unused parameter '$c'"]
    );
}

#[test]
fn unused_parameter_before_a_used_one_is_not_reported() {
    // `$a` holds `$b`'s position, so callers cannot drop it.
    assert_none(
        r#"<?php
function f($a, $b) {
    return $b;
}
"#,
    );
}

#[test]
fn underscore_parameters_are_skipped_without_ending_the_run() {
    let php = r#"<?php
function f($a, $b, $_c) {
    return $a;
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$b'"]);
}

#[test]
fn parameter_used_in_various_positions_is_not_reported() {
    assert_none(
        r#"<?php
function f($a, $b, $c, $d, $e, $g) {
    $a++;
    $x = $b->foo;
    $c .= 'x';
    echo "{$d}";
    if (isset($e)) {}
    return [$x, $g];
}
"#,
    );
}

#[test]
fn parameter_only_overwritten_is_reported() {
    let php = r#"<?php
function f($a) {
    $a = 1;
    return $a;
}
"#;
    // The read is of the new value, but that is still a read of `$a`.
    assert_none(php);
    let php = r#"<?php
function f($a) {
    $a = 1;
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$a'"]);
}

#[test]
fn by_reference_parameter_written_is_used() {
    assert_none(
        r#"<?php
function fill(array &$out) {
    $out = [1, 2];
}
"#,
    );
}

#[test]
fn by_reference_parameter_never_touched_is_reported() {
    assert_eq!(
        collect("<?php\nfunction fill(array &$out) {}\n"),
        vec!["Unused parameter '$out'"]
    );
}

#[test]
fn variadic_parameter_never_read_is_reported() {
    assert_eq!(
        collect("<?php\nfunction f(...$rest) { return 1; }\n"),
        vec!["Unused parameter '$rest'"]
    );
}

#[test]
fn func_get_args_reads_every_parameter() {
    assert_none(
        r#"<?php
function f($a, $b) {
    return func_get_args();
}
"#,
    );
}

#[test]
fn compact_reads_the_named_parameter() {
    assert_none(
        r#"<?php
function f($a, $b) {
    return compact('a', 'b');
}
"#,
    );
}

#[test]
fn compact_naming_another_variable_does_not_hide_the_parameter() {
    let php = r#"<?php
function f($a) {
    $x = 1;
    return compact('x');
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$a'"]);
}

#[test]
fn get_defined_vars_reads_every_parameter() {
    assert_none(
        r#"<?php
function f($a) {
    return get_defined_vars();
}
"#,
    );
}

#[test]
fn variable_variable_reads_every_parameter() {
    assert_none(
        r#"<?php
function f($a, $name) {
    return $$name;
}
"#,
    );
}

#[test]
fn include_reads_every_parameter() {
    assert_none(
        r#"<?php
function render($view, $data) {
    include $view;
}
"#,
    );
}

#[test]
fn include_inside_a_nested_closure_does_not_read_the_parameter() {
    let php = r#"<?php
function f($a) {
    return function () { include 'x.php'; };
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$a'"]);
}

// ─── Closures and arrow functions ───────────────────────────────────────────

#[test]
fn closure_parameter_never_read_is_reported() {
    let php = r#"<?php
function f() {
    return array_map(function ($value, $key) { return $value; }, [], []);
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$key'"]);
}

#[test]
fn arrow_function_parameter_never_read_is_reported() {
    let php = "<?php\n$f = fn($a, $b) => $a;\n";
    assert_eq!(collect(php), vec!["Unused parameter '$b'"]);
}

#[test]
fn closure_at_top_level_is_checked() {
    let php = "<?php\nRoute::get('/', function ($request) { return 'ok'; });\n";
    assert_eq!(collect(php), vec!["Unused parameter '$request'"]);
}

#[test]
fn parameter_captured_by_a_closure_is_used() {
    assert_none(
        r#"<?php
function f($a) {
    return function () use ($a) { return 1; };
}
"#,
    );
}

#[test]
fn parameter_read_inside_a_nested_arrow_function_is_used() {
    assert_none(
        r#"<?php
function f($a) {
    return fn() => $a;
}
"#,
    );
}

#[test]
fn read_inside_a_nested_closure_is_not_the_outer_parameter() {
    // The closure has its own scope: its `$a` is not the function's.
    let php = r#"<?php
function f($a) {
    return function () { return $a; };
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$a'"]);
}

#[test]
fn shadowing_arrow_function_parameter_is_not_the_outer_parameter() {
    let php = r#"<?php
function f($a) {
    return fn($a) => $a;
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$a'"]);
}

// ─── Methods ────────────────────────────────────────────────────────────────

#[test]
fn private_method_parameter_is_reported() {
    let php = r#"<?php
class A {
    private function f($a, $b) { return $a; }
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$b'"]);
}

#[test]
fn final_method_and_final_class_parameters_are_reported() {
    let php = r#"<?php
class A {
    final public function f($a) { return 1; }
}
final class B {
    public function g($b) { return 1; }
}
"#;
    let mut diags = collect(php);
    diags.sort();
    assert_eq!(
        diags,
        vec!["Unused parameter '$a'", "Unused parameter '$b'"]
    );
}

#[test]
fn enum_method_parameter_is_reported() {
    // An enum implements `UnitEnum`, which has to load to show it does
    // not declare the method.
    let php = r#"<?php
enum E {
    case X;
    public function h($c) { return 1; }
}
"#;
    let diags = collect_diagnostics_with(
        &enabled(Backend::new_test_with_full_stubs()),
        php,
        Backend::collect_unused_parameter_diagnostics,
    );
    let messages: Vec<&str> = diags.iter().map(|d| message_text(&d.message)).collect();
    assert_eq!(messages, vec!["Unused parameter '$c'"]);
}

#[test]
fn anonymous_class_method_parameter_is_reported() {
    let php = r#"<?php
$x = new class {
    public function f($a) { return 1; }
};
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$a'"]);
}

#[test]
fn overridable_method_parameter_is_not_reported() {
    // A subclass may override the method and read the parameter.
    assert_none(
        r#"<?php
class A {
    public function f($a) { return 1; }
    protected function g($b) {}
}
trait T {
    public function h($c) { return 1; }
}
"#,
    );
}

#[test]
fn method_implementing_an_interface_is_not_reported() {
    assert_none(
        r#"<?php
interface Handler { public function handle($event, $context); }
final class H implements Handler {
    public function handle($event, $context) { return 1; }
}
"#,
    );
}

#[test]
fn method_overriding_a_parent_method_is_not_reported() {
    assert_none(
        r#"<?php
class Base { public function f($a) { return $a; } }
final class Child extends Base {
    public function f($a) { return 1; }
}
"#,
    );
}

#[test]
fn private_method_implementing_an_abstract_trait_method_is_not_reported() {
    assert_none(
        r#"<?php
trait T { abstract private function f($a); }
class A {
    use T;
    private function f($a) { return 1; }
}
"#,
    );
}

#[test]
fn final_method_with_an_unloadable_parent_is_not_reported() {
    // The missing parent may well declare the method.
    assert_none(
        r#"<?php
final class A extends Missing {
    public function f($a) { return 1; }
}
"#,
    );
}

#[test]
fn magic_method_parameters_are_not_reported() {
    assert_none(
        r#"<?php
final class A {
    public function __get($name) { return 1; }
    public function __call($name, $arguments) { return 1; }
    public function __set($name, $value) {}
}
"#,
    );
}

#[test]
fn invoke_is_judged_like_any_method() {
    let php = r#"<?php
final class A {
    public function __invoke($a) { return 1; }
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$a'"]);
}

// ─── Constructors ───────────────────────────────────────────────────────────

#[test]
fn constructor_parameter_never_read_is_reported() {
    // A subclass constructor replaces this one rather than sharing its
    // parameters, so even a non-final class's constructor is judged.
    let php = r#"<?php
class A {
    private $a;
    public function __construct($a, $b) { $this->a = $a; }
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$b'"]);
}

#[test]
fn constructor_replacing_a_concrete_parent_constructor_is_judged() {
    let php = r#"<?php
class Base { public function __construct($a) {} }
class Child extends Base {
    public function __construct($a, $b) { parent::__construct($a); }
}
"#;
    // `Base::__construct` never reads its own `$a` either.
    let mut diags = collect(php);
    diags.sort();
    assert_eq!(
        diags,
        vec!["Unused parameter '$a'", "Unused parameter '$b'"]
    );
}

#[test]
fn constructor_implementing_an_abstract_constructor_is_not_reported() {
    assert_none(
        r#"<?php
abstract class Base { abstract public function __construct($a); }
class Child extends Base {
    public function __construct($a) {}
}
"#,
    );
}

#[test]
fn promoted_constructor_parameters_are_used() {
    assert_none(
        r#"<?php
class A {
    public function __construct(private int $a, public readonly string $b) {}
}
"#,
    );
}

#[test]
fn promoted_parameter_ends_the_trailing_run() {
    let php = r#"<?php
class A {
    public function __construct($a, private int $b, $c) {}
}
"#;
    assert_eq!(collect(php), vec!["Unused parameter '$c'"]);
}

// ─── Bodiless declarations ──────────────────────────────────────────────────

#[test]
fn abstract_and_interface_methods_are_not_reported() {
    assert_none(
        r#"<?php
interface I { public function f($a); }
abstract class A { abstract protected function g($b); }
"#,
    );
}
