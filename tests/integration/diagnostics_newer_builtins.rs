use crate::common::{collect_diagnostics_with, create_test_backend_with_full_stubs, message_text};
use phpantom_lsp::Backend;
use phpantom_lsp::types::PhpVersion;
use std::str::FromStr;
use tower_lsp_server::gen_lsp_types::*;

fn backend_for(version: PhpVersion) -> Backend {
    let backend = create_test_backend_with_full_stubs();
    backend.set_php_version(version);
    backend
}

fn collect_on(backend: &Backend, php: &str) -> Vec<String> {
    collect_diagnostics_with(backend, php, Backend::collect_newer_builtin_diagnostics)
        .iter()
        .map(|d| {
            assert_eq!(d.code, Some(Code::String("requires_newer_php".to_string())));
            assert_eq!(d.severity, Some(DiagnosticSeverity::Error));
            message_text(&d.message).to_string()
        })
        .collect()
}

fn collect_74(php: &str) -> Vec<String> {
    collect_on(&backend_for(PhpVersion::new(7, 4)), php)
}

#[test]
fn function_added_later_is_reported() {
    let php = r#"<?php
function f(string $haystack): bool {
    return str_contains($haystack, 'x');
}
"#;
    assert_eq!(
        collect_74(php),
        ["Function str_contains() requires PHP 8.0, the project targets PHP 7.4"]
    );
}

#[test]
fn function_the_target_has_is_not_reported() {
    let php = r#"<?php
function f(string $haystack): bool {
    return str_contains($haystack, 'x');
}
"#;
    assert!(collect_on(&backend_for(PhpVersion::new(8, 0)), php).is_empty());
}

#[test]
fn function_in_a_namespace_falls_back_to_the_global_one() {
    let php = r#"<?php
namespace App;
function f(array $items): mixed {
    return array_find($items, fn($item) => $item > 1);
}
"#;
    assert_eq!(
        collect_on(&backend_for(PhpVersion::new(8, 3)), php),
        ["Function array_find() requires PHP 8.4, the project targets PHP 8.3"]
    );
}

#[test]
fn call_still_resolves_to_the_stub() {
    let backend = backend_for(PhpVersion::new(7, 4));
    let php = r#"<?php
function f(string $haystack): void {
    $found = str_contains($haystack, 'x');
}
"#;
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut unknown = Vec::new();
    backend.collect_unknown_function_diagnostics(uri, php, &mut unknown);
    assert!(unknown.is_empty(), "{unknown:?}");
}

#[test]
fn class_added_later_is_reported() {
    let php = r#"<?php
function f(): object {
    return new WeakMap();
}
"#;
    assert_eq!(
        collect_74(php),
        ["Class WeakMap requires PHP 8.0, the project targets PHP 7.4"]
    );
}

#[test]
fn class_in_a_docblock_or_an_import_is_not_reported() {
    let php = r#"<?php
namespace App;
use WeakMap;
/** @param WeakMap $map */
function f($map): void {}
"#;
    assert!(collect_74(php).is_empty());
}

#[test]
fn class_that_is_not_loaded_is_not_reported() {
    let php = r#"<?php
class Model {
    #[\ReturnTypeWillChange]
    public function jsonSerialize() { return []; }
}
function f(object $value): bool {
    try {
        return $value instanceof \Stringable || \WeakMap::class === get_class($value);
    } catch (\ValueError $e) {
        return false;
    }
}
"#;
    assert!(collect_74(php).is_empty(), "{:?}", collect_74(php));
}

#[test]
fn method_added_later_is_reported() {
    let php = r#"<?php
function f(DateTimeInterface $date): DateTime {
    return DateTime::createFromInterface($date);
}
"#;
    assert_eq!(
        collect_74(php),
        ["Method DateTime::createFromInterface() requires PHP 8.0, the project targets PHP 7.4"]
    );
}

#[test]
fn method_on_a_variable_is_reported() {
    let php = r#"<?php
function f(): void {
    $date = new DateTimeImmutable();
    $date->setMicrosecond(5);
}
"#;
    assert_eq!(
        collect_on(&backend_for(PhpVersion::new(8, 3)), php),
        [
            "Method DateTimeImmutable::setMicrosecond() requires PHP 8.4, the project targets PHP 8.3"
        ]
    );
}

#[test]
fn global_constant_added_later_is_reported() {
    let php = r#"<?php
function f(): float {
    return PHP_FLOAT_EPSILON;
}
"#;
    assert_eq!(
        collect_on(&backend_for(PhpVersion::new(7, 1)), php),
        ["Constant PHP_FLOAT_EPSILON requires PHP 7.2, the project targets PHP 7.1"]
    );
}

#[test]
fn guarded_uses_are_not_reported() {
    let php = r#"<?php
function f(string $haystack): bool {
    if (function_exists('str_contains')) {
        return str_contains($haystack, 'x');
    }
    if (PHP_VERSION_ID >= 80000) {
        return str_contains($haystack, 'x');
    } else {
        return str_contains($haystack, 'y');
    }
}
function g(DateTimeInterface $date): ?object {
    if (version_compare(PHP_VERSION, '8.0.0', '<')) {
        return null;
    }
    $copy = DateTime::createFromInterface($date);
    return class_exists('WeakMap') ? new WeakMap() : null;
}
function h(DateTimeInterface $date): ?DateTime {
    return method_exists(DateTime::class, 'createFromInterface')
        ? DateTime::createFromInterface($date)
        : null;
}
function i(): float {
    return defined('PHP_FLOAT_EPSILON') ? PHP_FLOAT_EPSILON : 0.0;
}
function j(string $haystack): bool {
    return \PHP_VERSION_ID >= 80000 && str_contains($haystack, 'x');
}
"#;
    assert!(collect_74(php).is_empty(), "{:?}", collect_74(php));
}

#[test]
fn guard_for_another_symbol_does_not_cover_the_call() {
    let php = r#"<?php
function f(string $haystack): bool {
    if (function_exists('str_starts_with')) {
        return str_contains($haystack, 'x');
    }
    return false;
}
"#;
    assert_eq!(
        collect_74(php),
        ["Function str_contains() requires PHP 8.0, the project targets PHP 7.4"]
    );
}

#[test]
fn polyfill_function_takes_precedence() {
    let backend = backend_for(PhpVersion::new(7, 4));
    backend.update_ast(
        "file:///vendor/symfony/polyfill-php80/bootstrap.php",
        r#"<?php
if (!function_exists('str_contains')) {
    function str_contains(?string $haystack, ?string $needle): bool { return false; }
}
"#,
    );
    let php = r#"<?php
function f(string $haystack): bool {
    return str_contains($haystack, 'x');
}
"#;
    assert!(collect_on(&backend, php).is_empty());
}

#[test]
fn polyfill_class_takes_precedence() {
    let backend = backend_for(PhpVersion::new(7, 4));
    backend.update_ast(
        "file:///vendor/symfony/polyfill-php80/Resources/stubs/Stringable.php",
        r#"<?php
if (\PHP_VERSION_ID < 80000) {
    interface Stringable { public function __toString(); }
}
"#,
    );
    let php = r#"<?php
function f(Stringable $value): string {
    return (string) $value;
}
"#;
    assert!(collect_on(&backend, php).is_empty());
}

#[test]
fn polyfill_constant_takes_precedence() {
    let backend = backend_for(PhpVersion::new(7, 1));
    backend.update_ast(
        "file:///vendor/symfony/polyfill-php72/bootstrap.php",
        r#"<?php
if (!defined('PHP_FLOAT_EPSILON')) {
    define('PHP_FLOAT_EPSILON', 2.2204460492503E-16);
}
"#,
    );
    let php = r#"<?php
function f(): float {
    return PHP_FLOAT_EPSILON;
}
"#;
    assert!(collect_on(&backend, php).is_empty());
}

#[test]
fn user_class_with_a_same_named_method_is_not_reported() {
    let php = r#"<?php
class Clock {
    public static function createFromInterface(DateTimeInterface $date): self { return new self(); }
}
function f(DateTimeInterface $date): Clock {
    return Clock::createFromInterface($date);
}
"#;
    assert!(collect_74(php).is_empty());
}

// ─── Completion ─────────────────────────────────────────────────────────────

async fn labels_at_end(backend: &Backend, text: &str) -> Vec<String> {
    let uri = Uri::from_str("file:///completion.php").unwrap();
    let line = text.lines().count() as u32 - 1;
    let character = text.lines().last().unwrap().len() as u32;
    crate::common::complete_labels_at(backend, &uri, text, line, character).await
}

#[tokio::test]
async fn newer_builtins_are_left_out_of_completion() {
    let old = backend_for(PhpVersion::new(7, 4));
    let new = backend_for(PhpVersion::new(8, 0));

    let function = "<?php\nstr_conta";
    assert!(
        !labels_at_end(&old, function)
            .await
            .iter()
            .any(|l| l.starts_with("str_contains"))
    );
    assert!(
        labels_at_end(&new, function)
            .await
            .iter()
            .any(|l| l.starts_with("str_contains"))
    );

    let class = "<?php\nnew WeakMa";
    assert!(
        !labels_at_end(&old, class)
            .await
            .iter()
            .any(|l| l == "WeakMap")
    );
    assert!(
        labels_at_end(&new, class)
            .await
            .iter()
            .any(|l| l == "WeakMap")
    );

    let old = backend_for(PhpVersion::new(7, 1));
    let new = backend_for(PhpVersion::new(7, 2));
    let constant = "<?php\n$x = PHP_FLOAT_EPS";
    assert!(
        !labels_at_end(&old, constant)
            .await
            .iter()
            .any(|l| l == "PHP_FLOAT_EPSILON")
    );
    assert!(
        labels_at_end(&new, constant)
            .await
            .iter()
            .any(|l| l == "PHP_FLOAT_EPSILON")
    );
}

#[test]
fn polyfill_constant_survives_its_stub_being_parsed() {
    let backend = backend_for(PhpVersion::new(8, 1));
    backend.update_ast(
        "file:///polyfill.php",
        "<?php\nif (!defined('T_PRIVATE_SET')) {\n    define('T_PRIVATE_SET', 42450);\n}\n",
    );
    // Reading another constant parses the stub file that declares both.
    let reader = "<?php\n$token = T_STRING;\n";
    assert!(crate::common::hover_at(&backend, "file:///reader.php", reader, 1, 10).is_some());
    let php = r#"<?php
function f(): int {
    return T_PRIVATE_SET;
}
"#;
    assert!(
        collect_on(&backend, php).is_empty(),
        "{:?}",
        collect_on(&backend, php)
    );
}
