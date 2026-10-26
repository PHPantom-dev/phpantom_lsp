use crate::common::{collect_diagnostics_with, message_text};
use phpantom_lsp::Backend;
use phpantom_lsp::types::PhpVersion;
use tower_lsp_server::gen_lsp_types::*;

fn collect_on(backend: &Backend, php: &str) -> Vec<String> {
    collect_diagnostics_with(
        backend,
        php,
        Backend::collect_override_attribute_diagnostics,
    )
    .iter()
    .map(|d| {
        assert_eq!(
            d.code,
            Some(Code::String("invalid_override_attribute".to_string()))
        );
        assert_eq!(d.severity, Some(DiagnosticSeverity::Error));
        message_text(&d.message).to_string()
    })
    .collect()
}

fn collect(php: &str) -> Vec<String> {
    collect_on(&Backend::new_test(), php)
}

#[test]
fn property_without_parent_property_is_reported() {
    let php = r#"<?php
class ParentBox { public int $value = 1; }

class Child extends ParentBox
{
    #[\Override]
    public int $value = 2;

    #[\Override]
    public int $other = 3;
}
"#;
    assert_eq!(
        collect(php),
        vec!["Child::$other has #[\\Override] attribute, but no matching parent property exists"]
    );
}

#[test]
fn method_without_parent_method_is_reported() {
    let php = r#"<?php
class Foo { public function test(): void {} }

class Bar extends Foo
{
    #[\Override]
    public function test(): void {}

    #[\Override]
    public function test2(): void {}
}
"#;
    assert_eq!(
        collect(php),
        vec!["Bar::test2() has #[\\Override] attribute, but no matching parent method exists"]
    );
}

#[test]
fn diagnostic_covers_the_member_name() {
    let php = "<?php\nclass A {\n    #[\\Override]\n    public function f(): void {}\n}\n";
    let diags = collect_diagnostics_with(
        &Backend::new_test(),
        php,
        Backend::collect_override_attribute_diagnostics,
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].range.start, Position::new(3, 20));
    assert_eq!(diags[0].range.end, Position::new(3, 21));
}

#[test]
fn method_names_match_case_insensitively() {
    let php = r#"<?php
class P { public function Foo(): void {} }
class C extends P { #[\Override] public function foo(): void {} }
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn private_parent_members_are_not_overridden() {
    let php = r#"<?php
class P {
    private function f(): void {}
    private $a;
}
class C extends P {
    #[\Override] public function f(): void {}
    #[\Override] public $a;
}
"#;
    assert_eq!(collect(php).len(), 2);
}

#[test]
fn concrete_parent_constructor_is_not_overridden() {
    let php = r#"<?php
class P { public function __construct() {} }
class C extends P { #[\Override] public function __construct() {} }

abstract class AbstractP { abstract public function __construct(); }
class D extends AbstractP { #[\Override] public function __construct() {} }

interface HasCtor { public function __construct(); }
class Q implements HasCtor { public function __construct() {} }
class E extends Q { #[\Override] public function __construct() {} }
"#;
    assert_eq!(
        collect(php),
        vec!["C::__construct() has #[\\Override] attribute, but no matching parent method exists"]
    );
}

#[test]
fn own_trait_members_are_not_overridden_unless_abstract() {
    let php = r#"<?php
trait Concrete { public function f(): void {} public $a; }
class C { use Concrete; #[\Override] public function f(): void {} #[\Override] public $a; }

trait Inner { abstract public function g(): void; }
trait Outer { use Inner; }
class D { use Outer; #[\Override] public function g(): void {} }
"#;
    assert_eq!(collect(php).len(), 2);
}

#[test]
fn parent_trait_members_are_overridden() {
    let php = r#"<?php
trait T { public function f(): void {} public $a; private function p(): void {} }
class P { use T; }
class C extends P {
    #[\Override] public function f(): void {}
    #[\Override] public $a;
    #[\Override] public function p(): void {}
}
"#;
    assert_eq!(
        collect(php),
        vec!["C::p() has #[\\Override] attribute, but no matching parent method exists"]
    );
}

#[test]
fn interface_members_are_overridden() {
    let php = r#"<?php
interface I { public function f(): void; public $a { get; } }
interface J extends I { #[\Override] public function f(): void; }
interface K { #[\Override] public function g(): void; }
class Base implements J {}
abstract class C extends Base { #[\Override] public function f(): void {} #[\Override] public $a; }
"#;
    assert_eq!(
        collect(php),
        vec!["K::g() has #[\\Override] attribute, but no matching parent method exists"]
    );
}

#[test]
fn enum_methods_are_checked() {
    let php = r#"<?php
interface I { public function f(): void; }
enum E: string implements I {
    case A = 'a';
    #[\Override] public function f(): void {}
    #[\Override] public function h(): void {}
}
"#;
    assert_eq!(
        collect_on(&Backend::new_test_with_full_stubs(), php),
        vec!["E::h() has #[\\Override] attribute, but no matching parent method exists"]
    );
}

#[test]
fn promoted_and_hooked_properties_are_checked() {
    let php = r#"<?php
class P { public $a; }
class C extends P {
    public function __construct(#[\Override] public $a, #[\Override] public $b) {}
    #[\Override] public $c { get => 1; }
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "C::$b has #[\\Override] attribute, but no matching parent property exists",
            "C::$c has #[\\Override] attribute, but no matching parent property exists",
        ]
    );
}

#[test]
fn every_property_in_a_declaration_is_checked() {
    let php = r#"<?php
class P { public $a; }
class C extends P { #[\Override] public $a, $b; }
"#;
    assert_eq!(
        collect(php),
        vec!["C::$b has #[\\Override] attribute, but no matching parent property exists"]
    );
}

#[test]
fn anonymous_classes_are_checked() {
    let php = r#"<?php
function make() {
    return new class { #[\Override] public function f(): void {} };
}
"#;
    assert_eq!(collect(php).len(), 1);
}

#[test]
fn unqualified_override_in_a_namespace_is_another_attribute() {
    let php = r#"<?php
namespace App;
class C { #[Override] public function f(): void {} }
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn imported_and_lowercase_override_is_checked() {
    let php = r#"<?php
namespace App;
use Override;
class C { #[Override] public function f(): void {} #[\override] public function g(): void {} }
"#;
    assert_eq!(collect(php).len(), 2);
}

#[test]
fn trait_members_are_checked_where_used() {
    let php = r#"<?php
trait T { #[\Override] public function f(): void {} }
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn unknown_ancestors_are_assumed_to_declare_the_member() {
    let php = r#"<?php
class C extends Missing { #[\Override] public function f(): void {} }
class D implements MissingInterface { #[\Override] public function f(): void {} }
class E { use MissingTrait; #[\Override] public function f(): void {} }
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn virtual_parent_members_are_not_overridden() {
    let php = r#"<?php
/**
 * @method void f()
 * @property int $a
 */
class P {}
class C extends P { #[\Override] public function f(): void {} #[\Override] public $a; }
"#;
    assert_eq!(collect(php).len(), 2);
}

#[test]
fn checks_follow_the_php_version() {
    let php = r#"<?php
class C { #[\Override] public function f(): void {} #[\Override] public $a; }
"#;
    let backend = Backend::new_test();
    backend.set_php_version(PhpVersion::new(8, 4));
    assert_eq!(
        collect_on(&backend, php),
        vec!["C::f() has #[\\Override] attribute, but no matching parent method exists"]
    );
    backend.set_php_version(PhpVersion::new(8, 2));
    assert!(collect_on(&backend, php).is_empty());
}
