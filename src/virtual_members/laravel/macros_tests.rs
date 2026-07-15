//! Unit tests for the Laravel macro registration extractor.

use super::*;

#[test]
fn extracts_closure_macro_with_signature() {
    let content = r#"<?php
namespace App\Providers;
use Illuminate\Support\Collection;
class AppServiceProvider {
    public function boot(): void {
        Collection::macro('sumPrices', function (string $field): float {
            return 0.0;
        });
    }
}
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    let reg = &regs[0];
    assert_eq!(reg.target, "Illuminate\\Support\\Collection");
    assert_eq!(reg.method.name.as_str(), "sumPrices");
    assert_eq!(reg.method.parameters.len(), 1);
    assert_eq!(reg.method.parameters[0].name.as_str(), "$field");
    assert_eq!(
        reg.method.return_type.as_ref().map(|t| t.to_string()),
        Some("float".to_string())
    );
}

#[test]
fn extracts_arrow_function_macro() {
    let content = r#"<?php
use Illuminate\Support\Str;
Str::macro('shout', fn (string $s): string => strtoupper($s));
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Support\\Str");
    assert_eq!(regs[0].method.name.as_str(), "shout");
    assert_eq!(regs[0].method.parameters.len(), 1);
}

#[test]
fn resolves_target_through_use_statement() {
    // `Response` is imported, so the bare name resolves to the FQN.
    let content = r#"<?php
namespace App\Providers;
use Illuminate\Support\Facades\Response;
Response::macro('caps', function () { return 1; });
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Support\\Facades\\Response");
}

#[test]
fn skips_non_literal_name() {
    let content = r#"<?php
use Illuminate\Support\Collection;
Collection::macro($dynamicName, function () {});
"#;
    assert!(extract_macro_registrations(content, None).is_empty());
}

#[test]
fn skips_non_closure_second_argument() {
    let content = r#"<?php
use Illuminate\Support\Collection;
Collection::macro('viaCallable', 'someFunction');
"#;
    assert!(extract_macro_registrations(content, None).is_empty());
}

#[test]
fn skips_relative_self_target() {
    let content = r#"<?php
class Widget {
    public static function register(): void {
        self::macro('x', function () {});
    }
}
"#;
    assert!(extract_macro_registrations(content, None).is_empty());
}

#[test]
fn no_macro_substring_is_cheap_empty() {
    let content = "<?php class Foo { public function bar() {} }";
    assert!(extract_macro_registrations(content, None).is_empty());
}

#[test]
fn index_stores_static_and_instance_variants() {
    let content = r#"<?php
use Illuminate\Support\Collection;
Collection::macro('doubled', function (): int { return 2; });
"#;
    let regs = extract_macro_registrations(content, None);
    let mut index = LaravelMacroIndex::default();
    index.set_file("file:///provider.php".to_string(), regs);
    index.rebuild();

    let methods = index
        .get("Illuminate\\Support\\Collection")
        .expect("target should be indexed");
    assert_eq!(methods.len(), 2, "should store static + instance variants");
    assert!(methods.iter().any(|m| m.is_static));
    assert!(methods.iter().any(|m| !m.is_static));
    assert!(methods.iter().all(|m| m.name.as_str() == "doubled"));
}

#[test]
fn index_removes_file_contributions_when_emptied() {
    let content = r#"<?php
use Illuminate\Support\Collection;
Collection::macro('temp', function () {});
"#;
    let uri = "file:///provider.php".to_string();
    let mut index = LaravelMacroIndex::default();
    index.set_file(uri.clone(), extract_macro_registrations(content, None));
    index.rebuild();
    assert!(!index.is_empty());

    // File edited to remove the macro.
    index.set_file(uri, Vec::new());
    index.rebuild();
    assert!(index.is_empty());
}
