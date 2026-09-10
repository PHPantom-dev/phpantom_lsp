use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, php, &mut out);
    retain_visibility(out)
}

fn retain_visibility(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics.retain(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "invalid_member_access"))
    });
    diagnostics
}

fn messages(php: &str) -> Vec<String> {
    collect(php).into_iter().map(|d| d.message).collect()
}

/// Every diagnostic the file produces, whatever its code — used to prove
/// that a rejected access is reported *as* an access violation and does
/// not also surface as an unknown member.
fn all_codes(php: &str) -> Vec<String> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, php, &mut out);
    out.into_iter()
        .filter_map(|d| match d.code {
            Some(NumberOrString::String(s)) => Some(s),
            _ => None,
        })
        .collect()
}

// ─── Private members from outside ───────────────────────────────────────────

#[test]
fn a_private_property_read_from_top_level_code_is_flagged() {
    let php = r#"<?php
class Account {
    private string $pin = '0000';
}

$account = new Account();
echo $account->pin;
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("private property") && msgs[0].contains("Account::$pin"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn a_private_method_called_from_another_class_is_flagged() {
    let php = r#"<?php
class Account {
    private function rotate(): void {}
}

class Teller {
    public function run(Account $account): void
    {
        $account->rotate();
    }
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Cannot call private method") && msgs[0].contains("Account::rotate()"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn a_private_class_constant_read_from_outside_is_flagged() {
    let php = r#"<?php
class Account {
    private const SECRET = 'x';
}

echo Account::SECRET;
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("private constant") && msgs[0].contains("Account::SECRET"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn a_private_static_property_read_from_outside_is_flagged() {
    let php = r#"<?php
class Account {
    private static int $count = 0;
}

echo Account::$count;
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("private static property"),
        "unexpected message: {}",
        msgs[0]
    );
}

// ─── Private members from inside ────────────────────────────────────────────

#[test]
fn a_private_member_reached_through_this_is_fine() {
    let php = r#"<?php
class Account {
    private string $pin = '0000';

    private function rotate(): void {}

    public function run(): void
    {
        echo $this->pin;
        $this->rotate();
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "the declaring class may reach its own members: {:?}",
        messages(php)
    );
}

#[test]
fn a_private_member_of_another_instance_of_the_same_class_is_fine() {
    let php = r#"<?php
class Account {
    private string $pin = '0000';

    public function sameAs(Account $other): bool
    {
        return $this->pin === $other->pin;
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "privacy in PHP is per class, not per object: {:?}",
        messages(php)
    );
}

#[test]
fn a_private_member_of_a_parent_is_flagged_in_the_child() {
    let php = r#"<?php
class Base {
    private string $secret = 'x';
}

class Child extends Base {
    public function run(): void
    {
        echo $this->secret;
    }
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("private property") && msgs[0].contains("Base::$secret"),
        "the message should name the declaring class: {}",
        msgs[0]
    );
}

#[test]
fn a_private_parent_member_is_not_also_reported_as_an_unknown_member() {
    let php = r#"<?php
class Base {
    private string $secret = 'x';
}

class Child extends Base {
    public function run(): void
    {
        echo $this->secret;
    }
}
"#;
    let codes = all_codes(php);
    assert!(
        codes.contains(&"invalid_member_access".to_string()),
        "expected an access violation, got {codes:?}"
    );
    assert!(
        !codes.contains(&"unknown_member".to_string()),
        "a private parent member exists; it must not be reported as missing: {codes:?}"
    );
}

// ─── Protected members ──────────────────────────────────────────────────────

#[test]
fn a_protected_property_read_from_top_level_code_is_flagged() {
    let php = r#"<?php
class Account {
    protected string $pin = '0000';
}

$account = new Account();
echo $account->pin;
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("protected property") && msgs[0].contains("or its subclasses"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn a_protected_member_reached_from_a_subclass_is_fine() {
    let php = r#"<?php
class Base {
    protected string $shared = 'x';

    protected function helper(): void {}
}

class Child extends Base {
    public function run(): void
    {
        echo $this->shared;
        $this->helper();
        parent::helper();
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "a subclass may reach protected members: {:?}",
        messages(php)
    );
}

#[test]
fn a_protected_member_of_a_sibling_class_is_fine() {
    let php = r#"<?php
class Base {
    protected string $shared = 'x';
}

class Left extends Base {
    public function read(Right $right): string
    {
        return $right->shared;
    }
}

class Right extends Base {}
"#;
    assert!(
        messages(php).is_empty(),
        "the member is declared on the shared parent, so both branches see it: {:?}",
        messages(php)
    );
}

#[test]
fn a_protected_member_declared_on_a_sibling_itself_is_flagged() {
    let php = r#"<?php
class Base {}

class Left extends Base {
    public function read(Right $right): string
    {
        return $right->own;
    }
}

class Right extends Base {
    protected string $own = 'x';
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Right::$own"),
        "unexpected message: {}",
        msgs[0]
    );
}

// ─── Static and self-referencing access ─────────────────────────────────────

#[test]
fn self_and_static_access_to_own_private_members_is_fine() {
    let php = r#"<?php
class Account {
    private const SECRET = 'x';

    private static int $count = 0;

    private static function bump(): void {}

    public static function run(): void
    {
        echo self::SECRET;
        echo static::$count;
        self::bump();
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "a class may reach its own members through self and static: {:?}",
        messages(php)
    );
}

#[test]
fn a_protected_static_method_called_through_parent_is_fine() {
    let php = r#"<?php
class Base {
    protected static function make(): void {}
}

class Child extends Base {
    public static function run(): void
    {
        parent::make();
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "parent:: is the canonical way to reach a protected parent member: {:?}",
        messages(php)
    );
}

// ─── Suppression ────────────────────────────────────────────────────────────

#[test]
fn a_class_with_get_is_left_alone() {
    let php = r#"<?php
class Bag {
    private string $hidden = 'x';

    public function __get(string $name): mixed
    {
        return $this->$name;
    }
}

$bag = new Bag();
echo $bag->hidden;
"#;
    assert!(
        messages(php).is_empty(),
        "__get answers for an unreadable property instead of erroring: {:?}",
        messages(php)
    );
}

#[test]
fn a_class_with_call_is_left_alone() {
    let php = r#"<?php
class Proxy {
    private function hidden(): void {}

    public function __call(string $name, array $args): mixed
    {
        return null;
    }
}

$proxy = new Proxy();
$proxy->hidden();
"#;
    assert!(
        messages(php).is_empty(),
        "__call answers for an unreachable method instead of erroring: {:?}",
        messages(php)
    );
}

#[test]
fn a_public_member_is_never_flagged() {
    let php = r#"<?php
class Account {
    public string $name = 'x';

    public function rename(): void {}
}

$account = new Account();
echo $account->name;
$account->rename();
"#;
    assert!(
        messages(php).is_empty(),
        "public members are always reachable: {:?}",
        messages(php)
    );
}

#[test]
fn a_docblock_reference_to_a_private_member_is_left_alone() {
    let php = r#"<?php
class Account {
    private string $pin = '0000';

    /**
     * @see Account::$pin
     */
    public function run(): void {}
}
"#;
    assert!(
        messages(php).is_empty(),
        "a @see tag documents a member, it does not read one: {:?}",
        messages(php)
    );
}

// ─── Traits ─────────────────────────────────────────────────────────────────

#[test]
fn a_private_trait_member_is_reachable_from_the_using_class() {
    let php = r#"<?php
trait Signs {
    private function sign(): string
    {
        return 'x';
    }
}

class Letter {
    use Signs;

    public function run(): string
    {
        return $this->sign();
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "PHP flattens trait members into the using class: {:?}",
        messages(php)
    );
}

#[test]
fn a_private_trait_member_is_flagged_from_outside_the_using_class() {
    let php = r#"<?php
trait Signs {
    private function sign(): string
    {
        return 'x';
    }
}

class Letter {
    use Signs;
}

$letter = new Letter();
$letter->sign();
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Letter::sign()"),
        "the host class owns a flattened trait member: {}",
        msgs[0]
    );
}

#[test]
fn a_trait_body_is_never_judged() {
    let php = r#"<?php
class Account {
    private string $pin = '0000';
}

trait Reads {
    public function read(Account $account): string
    {
        return $account->pin;
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "a trait's host class is unknown, so no visibility answer is trustworthy: {:?}",
        messages(php)
    );
}

// ─── Trait adaptations ──────────────────────────────────────────────────────
// The `use` clause can rewrite what a trait declares, so the visibility that
// counts is the one the assembled class ends up with, not the one written in
// the trait.

#[test]
fn a_trait_method_made_private_by_the_use_clause_is_flagged_from_outside() {
    let php = r#"<?php
trait Signs {
    public function run(): string
    {
        return 'x';
    }
}

class Letter {
    use Signs { run as private; }
}

$letter = new Letter();
$letter->run();
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("private method") && msgs[0].contains("Letter::run()"),
        "the use clause, not the trait, decides the visibility: {}",
        msgs[0]
    );
}

#[test]
fn a_trait_method_aliased_to_private_keeps_the_public_original_reachable() {
    let php = r#"<?php
trait Signs {
    public function run(): string
    {
        return 'x';
    }
}

class Letter {
    use Signs { run as private hidden; }
}

$letter = new Letter();
$letter->run();
"#;
    assert!(
        messages(php).is_empty(),
        "aliasing adds a name, it does not remove the original: {:?}",
        messages(php)
    );
}

#[test]
fn a_private_trait_alias_is_flagged_from_outside() {
    let php = r#"<?php
trait Signs {
    public function run(): string
    {
        return 'x';
    }
}

class Letter {
    use Signs { run as private hidden; }
}

$letter = new Letter();
$letter->hidden();
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Letter::hidden()"),
        "unexpected message: {}",
        msgs[0]
    );
}

#[test]
fn insteadof_decides_which_declaration_the_visibility_comes_from() {
    let php = r#"<?php
trait Loud {
    public function speak(): string
    {
        return 'loud';
    }
}

trait Quiet {
    private function speak(): string
    {
        return 'quiet';
    }
}

class Speaker {
    use Loud, Quiet {
        Loud::speak insteadof Quiet;
    }
}

$speaker = new Speaker();
$speaker->speak();
"#;
    assert!(
        messages(php).is_empty(),
        "the public declaration won the conflict, so the call is legal: {:?}",
        messages(php)
    );
}

// A `use` adaptation names a declaration in order to re-scope or select
// it.  Symbol extraction records those identifiers as `Trait::method()`
// so go-to-definition and rename land on them, which makes the clause
// look like a static call on the trait — it is not one, and no
// visibility rule applies to it.

#[test]
fn a_use_clause_widening_a_protected_trait_method_is_not_an_access() {
    let php = r#"<?php
trait Mocks {
    protected function mockThing(): void {}
    protected function other(): void {}
}

class Host {
    use Mocks {
        mockThing as public;
        other as public renamed;
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "the clause makes the members reachable, it does not reach them: {:?}",
        messages(php)
    );
}

#[test]
fn an_insteadof_clause_naming_a_protected_trait_method_is_not_an_access() {
    let php = r#"<?php
trait Left {
    protected function pick(): void {}
}

trait Right {
    protected function pick(): void {}
}

class Host {
    use Left, Right {
        Left::pick insteadof Right;
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "the clause picks a declaration, it does not call one: {:?}",
        messages(php)
    );
}

#[test]
fn a_protected_member_a_use_clause_widened_is_reachable_from_outside() {
    let php = r#"<?php
trait Mocks {
    protected function mockThing(): void {}
}

class Host {
    use Mocks { mockThing as public; }
}

$host = new Host();
$host->mockThing();
"#;
    assert!(
        messages(php).is_empty(),
        "the use clause, not the trait, decides the visibility: {:?}",
        messages(php)
    );
}

#[test]
fn a_protected_member_introduced_through_a_nested_trait_is_reachable_from_a_sibling() {
    let php = r#"<?php
trait Inner { protected function ping(): void {} }
trait Outer { use Inner; }

class Base { use Outer; }
class Mid extends Base {
    protected function ping(): void {}
}

class Other extends Base {
    public function go(Mid $m): void
    {
        $m->ping();
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "Base introduced the member through the trait it composes, so every \
         branch below Base reaches it however near the redeclaration sits: {:?}",
        messages(php)
    );
}

// ─── Deeper hierarchies ─────────────────────────────────────────────────────

#[test]
fn a_protected_member_introduced_two_levels_up_is_reachable_from_a_cousin() {
    let php = r#"<?php
class Root {
    protected string $shared = 'x';
}

class Middle extends Root {}

class Leaf extends Middle {}

class Cousin extends Root {
    public function read(Leaf $leaf): string
    {
        return $leaf->shared;
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "the member was introduced on Root, which Cousin also descends from: {:?}",
        messages(php)
    );
}

// ─── More magic handlers ────────────────────────────────────────────────────

// PHP would fatal on the read below — `__unset` does not answer for one.
// The span the check runs on records that a property was accessed, not
// whether it was read, written, or unset, so the exact handler cannot be
// required and any of the four stands the check down.
#[test]
fn any_property_handler_stands_the_check_down_for_now() {
    let php = r#"<?php
class Bag {
    private string $hidden = 'x';

    public function __unset(string $name): void {}
}

$bag = new Bag();
echo $bag->hidden;
"#;
    assert!(
        messages(php).is_empty(),
        "a property handler anywhere on the class stands the check down: {:?}",
        messages(php)
    );
}

#[test]
fn a_magic_handler_inherited_from_a_parent_still_counts() {
    let php = r#"<?php
class Base {
    public function __get(string $name): mixed
    {
        return null;
    }
}

class Bag extends Base {
    private string $hidden = 'x';
}

$bag = new Bag();
echo $bag->hidden;
"#;
    assert!(
        messages(php).is_empty(),
        "__get is inherited, so it answers here too: {:?}",
        messages(php)
    );
}

// ─── Anonymous classes ──────────────────────────────────────────────────────

#[test]
fn an_anonymous_class_may_reach_its_own_private_members() {
    let php = r#"<?php
$worker = new class {
    private string $token = 'x';

    public function run(): string
    {
        return $this->token;
    }
};
"#;
    assert!(
        messages(php).is_empty(),
        "an anonymous class is still a class: {:?}",
        messages(php)
    );
}

#[test]
fn a_top_level_message_names_the_class_that_introduced_the_member() {
    let php = r#"<?php
class Base {
    protected string $token = 'x';
}

class Child extends Base {}

echo (new Child())->token;
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Base::$token") && msgs[0].contains("outside Base"),
        "protected is spelled in terms of the class that introduced it, so \
         naming the receiver would state a rule PHP does not follow: {}",
        msgs[0]
    );
}

// ─── Provenance ─────────────────────────────────────────────────────────────
// The assembled class says what a class has, never who declared it, so
// every case below turns on finding the real declaring class.

#[test]
fn a_private_trait_member_used_by_a_parent_is_flagged_in_the_child() {
    let php = r#"<?php
trait Hidden {
    private function hidden(): void {}
}

class HiddenParent {
    use Hidden;
}

class HiddenChild extends HiddenParent {
    public function run(): void
    {
        $this->hidden();
    }
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("HiddenParent::hidden()"),
        "a parent's trait member belongs to the parent's scope, not the child's: {}",
        msgs[0]
    );
}

#[test]
fn a_private_trait_member_used_by_a_parent_is_reachable_in_that_parent() {
    let php = r#"<?php
trait Hidden {
    private function hidden(): void {}
}

class HiddenParent {
    use Hidden;

    public function run(): void
    {
        $this->hidden();
    }
}

class HiddenChild extends HiddenParent {}
"#;
    assert!(
        messages(php).is_empty(),
        "the class that uses the trait may reach what it imported: {:?}",
        messages(php)
    );
}

#[test]
fn a_nearer_protected_declaration_shadows_a_private_namesake_further_up() {
    let php = r#"<?php
class Root {
    private function probe(): void {}
}

class Owner extends Root {
    protected function probe(): void {}
}

class Receiver extends Owner {}

class Cousin extends Root {
    public function run(Receiver $receiver): void
    {
        $receiver->probe();
    }
}
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Owner::probe()"),
        "the introducer is Owner; Root's private namesake is a different member: {}",
        msgs[0]
    );
}

#[test]
fn a_class_inheriting_a_magic_handler_and_declaring_a_private_member_is_left_alone() {
    let php = r#"<?php
class Base {
    public function __call(string $name, array $args): mixed
    {
        return null;
    }
}

class Bag extends Base {
    private function hidden(): void {}
}

$bag = new Bag();
$bag->hidden();
"#;
    assert!(
        messages(php).is_empty(),
        "__call is inherited and answers for the unreachable method: {:?}",
        messages(php)
    );
}

#[test]
fn a_private_static_property_message_keeps_its_dollar() {
    let php = r#"<?php
class Counter {
    private static int $count = 0;
}

echo Counter::$count;
"#;
    let msgs = messages(php);
    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("Counter::$count"),
        "extraction strips the $, so the message has to put it back: {}",
        msgs[0]
    );
}

#[test]
fn a_closure_bound_to_another_class_is_judged_against_that_class() {
    let php = r#"<?php
class DateBase {
    protected static function build(): static
    {
        return new static();
    }

    /**
     * @param-closure-this static $macro
     */
    public static function macro(string $name, callable $macro): void {}
}

class Provider {
    public function boot(): void
    {
        DateBase::macro('made', function (): DateBase {
            return self::build();
        });
    }
}
"#;
    assert!(
        messages(php).is_empty(),
        "the closure is bound to DateBase, so self:: is inside DateBase however \
         the surrounding class is spelled: {:?}",
        messages(php)
    );
}

// ─── Cross-file ─────────────────────────────────────────────────────────────

#[test]
fn a_private_member_is_flagged_across_files() {
    let composer_json = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/"
        }
    }
}"#;
    let (backend, _tmp) = create_psr4_workspace(
        composer_json,
        &[(
            "src/Account.php",
            r#"<?php
namespace App;

class Account {
    private string $pin = '0000';
}
"#,
        )],
    );

    let uri = "file:///teller.php";
    let php = r#"<?php
namespace App;

class Teller {
    public function run(Account $account): string
    {
        return $account->pin;
    }
}
"#;
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, php, &mut out);
    let msgs: Vec<String> = retain_visibility(out)
        .into_iter()
        .map(|d| d.message)
        .collect();

    assert_eq!(msgs.len(), 1, "expected one diagnostic, got {msgs:?}");
    assert!(
        msgs[0].contains("App\\Account::$pin"),
        "the message should carry the FQN: {}",
        msgs[0]
    );
}

// ─── Diagnostic shape ───────────────────────────────────────────────────────

#[test]
fn the_diagnostic_has_the_expected_code_severity_and_source() {
    let php = r#"<?php
class Account {
    private string $pin = '0000';
}

$account = new Account();
echo $account->pin;
"#;
    let diags = collect(php);
    assert_eq!(diags.len(), 1, "expected one diagnostic");
    assert_eq!(
        diags[0].severity,
        Some(DiagnosticSeverity::ERROR),
        "an inaccessible member is a fatal error at runtime"
    );
    assert_eq!(diags[0].source.as_deref(), Some("phpantom"));
    assert!(matches!(
        diags[0].code,
        Some(NumberOrString::String(ref s)) if s == "invalid_member_access"
    ));
}
