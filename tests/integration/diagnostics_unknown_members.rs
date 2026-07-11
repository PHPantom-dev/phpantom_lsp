use crate::common::{
    create_psr4_workspace, create_test_backend, create_test_backend_with_exception_stubs,
};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helpers for scope-cache-enabled diagnostics ────────────────────────────

/// Open a file, run full slow diagnostics (which activates the diagnostic
/// scope cache and the forward walker), then filter to unknown_member
/// diagnostics only.  This exercises the forward walker's diagnostic path
/// instead of the backward scanner.
fn unknown_member_diagnostics_with_scope_cache(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    text: &str,
) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut out);
    // Keep only unknown_member diagnostics (the code we're testing).
    out.retain(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
    });
    out
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Open a file, trigger `update_ast`, then collect unknown-member diagnostics.
fn unknown_member_diagnostics(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    text: &str,
) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut out);
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Startup race: resolved-class cache poisoned before indexing completes
// ═══════════════════════════════════════════════════════════════════════════

/// A method inherited from a vendor base class must not be flagged as
/// unknown once indexing has finished, even if the child class was first
/// resolved (by an early hover/completion/diagnostic request) while the
/// vendor parent was not yet in the index.
///
/// Reproduces the reported Symfony controller bug: `redirectToRoute` (an
/// inherited method on `AbstractController`) was flagged `unknown_member`
/// by diagnostics while hover resolved it correctly.  The cause was the
/// resolved-class cache caching a base-only merge of the child (parent
/// unresolvable mid-indexing) and never invalidating it.  The diagnostic
/// path reads that merged cache; hover walks the parent chain live, which
/// is why hover was unaffected.
#[tokio::test]
async fn inherited_member_not_flagged_after_indexing_completes() {
    // The parent lives in `vendor/` and is therefore only discoverable
    // through the vendor scan that runs during `initialized()` — exactly
    // like a framework base class.  It is NOT in the user's PSR-4 map, so
    // it cannot be resolved before indexing.
    let composer_json = r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#;
    let installed_json = r#"{"packages": [{
        "name": "acme/framework",
        "version": "1.0.0",
        "install-path": "../acme/framework",
        "autoload": {"psr-4": {"Acme\\Framework\\": ""}}
    }]}"#;
    let base = "<?php\nnamespace Acme\\Framework;\nclass BaseController {\n    public function redirectToRoute(string $route): void {}\n}\n";

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("vendor/acme/framework/BaseController.php", base),
            ("vendor/composer/installed.json", installed_json),
        ],
    );

    let uri = "file:///child.php";
    let text = "<?php\nnamespace App;\nuse Acme\\Framework\\BaseController;\nclass BlogController extends BaseController {\n    public function index(): void {\n        $this->redirectToRoute('home');\n    }\n}\n";

    // ── Pre-indexing request poisons the cache ──────────────────────
    // Resolving the child here cannot find the vendor parent, so the
    // merged child is cached without its inherited members.
    backend.update_ast(uri, text);
    let mut pre = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut pre);
    assert!(
        pre.iter().any(|d| d.message.contains("redirectToRoute")),
        "setup precondition: the inherited method should be flagged \
         while the vendor parent is not yet indexed, got: {pre:?}"
    );

    // ── Indexing completes ──────────────────────────────────────────
    // `initialized()` scans the vendor package (indexing the parent)
    // and must invalidate the poisoned merged-class cache.
    backend.initialized(InitializedParams {}).await;

    // ── The same diagnostic pass must now resolve the inherited method ──
    // Note: we deliberately do NOT re-run `update_ast` here — re-parsing
    // the file would evict the cached merge on its own and mask the bug.
    let mut post = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut post);
    assert!(
        !post.iter().any(|d| d.message.contains("redirectToRoute")),
        "inherited method must resolve once the vendor parent is indexed, got: {post:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Basic detection — instance methods
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_unknown_instance_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {}
}

class Consumer {
    public function run(): void {
        $f = new Foo();
        $f->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("not found")),
        "Expected unknown method diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_existing_instance_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {}
}

class Consumer {
    public function run(): void {
        $f = new Foo();
        $f->bar();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for existing method, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Basic detection — instance properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_unknown_instance_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public string $name = '';
}

class Consumer {
    public function run(): void {
        $f = new Foo();
        $f->missing;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("missing") && d.message.contains("not found")),
        "Expected unknown property diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_existing_instance_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public string $name = '';
}

class Consumer {
    public function run(): void {
        $f = new Foo();
        $f->name;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for existing property, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Basic detection — static methods
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_unknown_static_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public static function bar(): void {}
}

Foo::nonexistent();
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("not found")),
        "Expected unknown static method diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_existing_static_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public static function bar(): void {}
}

Foo::bar();
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for existing static method, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Basic detection — constants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_unknown_class_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    const BAR = 1;
}

$x = Foo::MISSING;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("MISSING") && d.message.contains("not found")),
        "Expected unknown constant diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_existing_class_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    const BAR = 1;
}

$x = Foo::BAR;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for existing constant, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Static properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_existing_static_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Config {
    public static string $appName = 'test';
}

$name = Config::$appName;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for existing static property, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ::class magic constant
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_class_keyword() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {}

$name = Foo::class;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for ::class, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Magic method suppression
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_when_class_has_magic_call_but_chain_continues() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Magic {
    public function __call(string $name, array $args): mixed {}
}

class Consumer {
    public function run(): void {
        $m = new Magic();
        $m->anything();
        $m->whatever();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        2,
        "Should flag unknown methods even when __call exists, got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("anything"),
        "First diagnostic should mention 'anything', got: {}",
        diags[0].message
    );
    assert!(
        diags[1].message.contains("whatever"),
        "Second diagnostic should mention 'whatever', got: {}",
        diags[1].message
    );
}

#[test]
fn no_diagnostic_when_class_has_magic_get() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class DynProps {
    public function __get(string $name): mixed {}
}

class Consumer {
    public function run(): void {
        $d = new DynProps();
        $d->anything;
        $d->whatever;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected when __get exists, got: {:?}",
        diags
    );
}

#[test]
fn diagnostic_when_class_has_magic_call_static_but_chain_continues() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class StaticMagic {
    public static function __callStatic(string $name, array $args): mixed {}
}

StaticMagic::anything();
StaticMagic::whatever();
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        2,
        "Should flag unknown static methods even when __callStatic exists, got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("anything"),
        "First diagnostic should mention 'anything', got: {}",
        diags[0].message
    );
    assert!(
        diags[1].message.contains("whatever"),
        "Second diagnostic should mention 'whatever', got: {}",
        diags[1].message
    );
}

#[test]
fn magic_call_does_not_suppress_property_diagnostics() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Magic {
    public function __call(string $name, array $args): mixed {}
}

class Consumer {
    public function run(): void {
        $m = new Magic();
        $m->missingProp;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    // __call only handles method calls, not property access.
    // Without __get, property access should still be flagged.
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("missingProp") && d.message.contains("not found")),
        "Expected unknown property diagnostic even with __call (no __get), got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Inherited magic methods
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_when_parent_has_magic_call_but_chain_continues() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Base {
    public function __call(string $name, array $args): mixed {}
}

class Child extends Base {}

class Consumer {
    public function run(): void {
        $c = new Child();
        $c->anything();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        1,
        "Should flag unknown method even when parent has __call, got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("anything"),
        "Diagnostic should mention 'anything', got: {}",
        diags[0].message
    );
}

#[test]
fn no_diagnostic_when_trait_has_magic_get() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait DynamicProperties {
    public function __get(string $name): mixed {}
}

class Widget {
    use DynamicProperties;
}

class Consumer {
    public function run(): void {
        $w = new Widget();
        $w->anything;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected when trait provides __get, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Inheritance — methods, properties, constants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_inherited_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Base {
    public function baseMethod(): void {}
}

class Child extends Base {}

class Consumer {
    public function run(): void {
        $c = new Child();
        $c->baseMethod();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for inherited method, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_inherited_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Base {
    public string $baseProp = '';
}

class Child extends Base {}

class Consumer {
    public function run(): void {
        $c = new Child();
        $c->baseProp;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for inherited property, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_inherited_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Base {
    const BASE_CONST = 42;
}

class Child extends Base {}

$x = Child::BASE_CONST;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for inherited constant, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Trait members
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_trait_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait Greetable {
    public function greet(): string { return 'hello'; }
}

class Greeter {
    use Greetable;
}

class Consumer {
    public function run(): void {
        $g = new Greeter();
        $g->greet();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for trait method, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_trait_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait HasName {
    public string $name = '';
}

class User {
    use HasName;
}

class Consumer {
    public function run(): void {
        $u = new User();
        $u->name;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for trait property, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Virtual members (@method / @property / @mixin)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_phpdoc_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @method string getName()
 * @method void setName(string $name)
 */
class VirtualClass {}

class Consumer {
    public function run(): void {
        $v = new VirtualClass();
        $v->getName();
        $v->setName('test');
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for @method virtual member, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_phpdoc_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @property string $name
 * @property-read int $id
 */
class VirtualClass {
    public function __get(string $name): mixed {}
}

class Consumer {
    public function run(): void {
        $v = new VirtualClass();
        $v->name;
        $v->id;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for @property virtual member, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_mixin_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Helper {
    public function doHelp(): void {}
}

/**
 * @mixin Helper
 */
class Service {}

class Consumer {
    public function run(): void {
        $s = new Service();
        $s->doHelp();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for @mixin method, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// $this / self / static / parent contexts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_unknown_method_on_this() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {
        $this->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("not found")),
        "Expected unknown method diagnostic for $this->nonexistent(), got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_this_existing_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {}

    public function baz(): void {
        $this->bar();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for $this->bar(), got: {:?}",
        diags
    );
}

#[test]
fn flags_unknown_method_on_self() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {
        self::nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("not found")),
        "Expected unknown method diagnostic for self::nonexistent(), got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_self_existing_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public static function bar(): void {}

    public function baz(): void {
        self::bar();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for self::bar(), got: {:?}",
        diags
    );
}

#[test]
fn flags_unknown_method_on_static() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {
        static::nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("not found")),
        "Expected unknown method diagnostic for static::nonexistent(), got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_parent_existing_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Base {
    public function parentMethod(): void {}
}

class Child extends Base {
    public function childMethod(): void {
        parent::parentMethod();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for parent::parentMethod(), got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Case-insensitive method matching
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn method_matching_is_case_insensitive() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function getData(): void {}
}

class Consumer {
    public function run(): void {
        $f = new Foo();
        $f->getdata();
        $f->GETDATA();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "PHP methods are case-insensitive, no diagnostic expected, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Multiple unknown members in one file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_multiple_unknown_members() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function known(): void {}
}

class Consumer {
    public function run(): void {
        $f = new Foo();
        $f->unknown1();
        $f->known();
        $f->unknown2();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        2,
        "Expected exactly 2 diagnostics, got: {:?}",
        diags
    );
    assert!(diags.iter().any(|d| d.message.contains("unknown1")));
    assert!(diags.iter().any(|d| d.message.contains("unknown2")));
}

// ═══════════════════════════════════════════════════════════════════════════
// Unresolvable subject — no false positives
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_when_subject_unresolvable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function getUnknown(): mixed { return null; }

$x = getUnknown();
$x->whatever();
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected when subject type is unresolvable, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_when_class_not_found() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
UnknownClass::method();
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    // The class itself is unknown — that's a different diagnostic
    // (unknown_classes). We should not also flag the member.
    assert!(
        diags.is_empty(),
        "No member diagnostic expected when the class itself is unknown, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Enum cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_enum_case() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
enum Color {
    case Red;
    case Green;
    case Blue;
}

$c = Color::Red;
$d = Color::Green;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for enum case access, got: {:?}",
        diags
    );
}

#[test]
fn flags_unknown_enum_case() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
enum Color {
    case Red;
    case Green;
    case Blue;
}

$c = Color::Purple;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("Purple") && d.message.contains("not found")),
        "Expected unknown member diagnostic for Color::Purple, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_backed_enum_case() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
enum Status: string {
    case Active = 'active';
    case Inactive = 'inactive';
}

$s = Status::Active;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for backed enum case, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Parameter type hint resolution
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_unknown_method_via_parameter_type_hint() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Service {
    public function doWork(): void {}
}

class Handler {
    public function handle(Service $svc): void {
        $svc->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("not found")),
        "Expected unknown method diagnostic via parameter type, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_method_via_parameter_type_hint() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Service {
    public function doWork(): void {}
}

class Handler {
    public function handle(Service $svc): void {
        $svc->doWork();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for existing method via parameter, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_method_via_param_docblock_override() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Node {}

class FuncCall extends Node {
    public function isFirstClassCallable(): bool {}
}

class Handler {
    /**
     * @param FuncCall $node
     */
    public function handle(Node $node): void {
        $node->isFirstClassCallable();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for existing method via @param override, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Interface method access
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_interface_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Renderable {
    public function render(): string;
}

class View implements Renderable {
    public function render(): string { return ''; }
}

class Consumer {
    public function run(Renderable $r): void {
        $r->render();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for interface method, got: {:?}",
        diags
    );
}

#[test]
fn flags_unknown_method_on_interface() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Renderable {
    public function render(): string;
}

class Consumer {
    public function run(Renderable $r): void {
        $r->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("not found")),
        "Expected unknown method diagnostic on interface, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Diagnostic metadata
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_has_warning_severity() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {}

class Consumer {
    public function run(): void {
        $f = new Foo();
        $f->missing();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(!diags.is_empty(), "Expected at least one diagnostic");
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
}

#[test]
fn diagnostic_has_code_and_source() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {}

class Consumer {
    public function run(): void {
        $f = new Foo();
        $f->missing();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(!diags.is_empty(), "Expected at least one diagnostic");
    assert_eq!(
        diags[0].code,
        Some(NumberOrString::String("unknown_member".to_string()))
    );
    assert_eq!(diags[0].source, Some("phpantom".to_string()));
}

#[test]
fn diagnostic_message_includes_class_name() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class MyService {}

class Consumer {
    public function run(): void {
        $s = new MyService();
        $s->missing();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(!diags.is_empty(), "Expected at least one diagnostic");
    assert!(
        diags[0].message.contains("MyService"),
        "Diagnostic should mention the class name, got: {}",
        diags[0].message
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Constructor calls should not flag
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_constructor_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function __construct() {}
}

$f = new Foo();
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for constructor call, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Method return type chain resolution
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_method_chain_existing_members() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Builder {
    public function where(): Builder { return $this; }
    public function get(): array { return []; }
}

class Service {
    public function query(): Builder { return new Builder(); }
}

class Consumer {
    public function run(): void {
        $s = new Service();
        $s->query()->where();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for valid method chain, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-file resolution (PSR-4)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_unknown_member_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Service.php",
            r#"<?php
namespace App;

class Service {
    public function doWork(): void {}
}
"#,
        )],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\Service;

class Consumer {
    public function run(Service $svc): void {
        $svc->nonexistent();
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("not found")),
        "Expected unknown method diagnostic across files, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_existing_member_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Service.php",
            r#"<?php
namespace App;

class Service {
    public function doWork(): void {}
}
"#,
        )],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\Service;

class Consumer {
    public function run(Service $svc): void {
        $svc->doWork();
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        diags.is_empty(),
        "No diagnostics expected for existing member across files, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Mixed known and unknown in same access chain
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn only_flags_the_unknown_member_not_the_known() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {}
    public string $name = '';
}

class Consumer {
    public function run(): void {
        $f = new Foo();
        $f->bar();
        $f->name;
        $f->missing;
        $f->alsoMissing();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        2,
        "Expected exactly 2 diagnostics (missing, alsoMissing), got: {:?}",
        diags
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("'bar'")),
        "bar() should not be flagged"
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("'name'")),
        "name should not be flagged"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Abstract class members
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_abstract_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
abstract class Shape {
    abstract public function area(): float;
}

class Consumer {
    public function run(Shape $s): void {
        $s->area();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for abstract method, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Promoted constructor properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_promoted_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class User {
    public function __construct(
        public readonly string $name,
        public readonly string $email,
    ) {}
}

class Consumer {
    public function run(): void {
        $u = new User('John', 'john@example.com');
        $u->name;
        $u->email;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for promoted properties, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Visibility should not affect detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_private_method_on_this() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    private function secret(): void {}

    public function bar(): void {
        $this->secret();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    // We don't check visibility — the member exists, so no diagnostic.
    // Visibility violations are a different diagnostic (not implemented yet).
    assert!(
        diags.is_empty(),
        "No diagnostics expected for private method via $this, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_protected_method_on_this() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    protected function helper(): void {}

    public function bar(): void {
        $this->helper();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for protected method via $this, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Empty class produces diagnostic
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_method_on_empty_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Empty_ {}

class Consumer {
    public function run(): void {
        $e = new Empty_();
        $e->anything();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("anything") && d.message.contains("not found")),
        "Expected unknown method diagnostic on empty class, got: {:?}",
        diags
    );
}

#[test]
fn flags_property_on_empty_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Empty_ {}

class Consumer {
    public function run(): void {
        $e = new Empty_();
        $e->anything;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("anything") && d.message.contains("not found")),
        "Expected unknown property diagnostic on empty class, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Enum constant access (not a case)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_enum_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
enum Color {
    case Red;
    const DEFAULT = self::Red;
}

$x = Color::DEFAULT;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for enum constant, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Interface virtual members (@method on interface)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_interface_phpdoc_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @method string format()
 */
interface Formattable {}

class Widget implements Formattable {}

class Consumer {
    public function run(): void {
        $w = new Widget();
        $w->format();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for interface @method, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Self constant access
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_self_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    const MAX = 100;

    public function getMax(): int {
        return self::MAX;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for self::MAX, got: {:?}",
        diags
    );
}

#[test]
fn flags_unknown_self_constant() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    const MAX = 100;

    public function getMin(): int {
        return self::MIN;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("MIN") && d.message.contains("not found")),
        "Expected unknown constant diagnostic for self::MIN, got: {:?}",
        diags
    );
}

// ── stdClass suppression ────────────────────────────────────────────────

/// stdClass is a universal object container — any property access on it
/// should be silently accepted.
#[test]
fn no_diagnostic_for_property_on_stdclass() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
$obj = new \stdClass();
$obj->anything;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for property access on stdClass, got: {:?}",
        diags
    );
}

/// Method calls on stdClass should also be suppressed.
#[test]
fn no_diagnostic_for_method_on_stdclass() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
$obj = new \stdClass();
$obj->whatever();
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for method call on stdClass, got: {:?}",
        diags
    );
}

/// When stdClass appears as a branch in a union type, suppress diagnostics
/// for the entire union since the property could be on the stdClass branch.
#[test]
fn no_diagnostic_for_stdclass_in_union() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Strict {
    public function known(): void {}
}

/** @var Strict|\stdClass $obj */
$obj = new Strict();
$obj->unknown_prop;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected when any union branch is stdClass, got: {:?}",
        diags
    );
}

/// stdClass passed as a parameter type hint should suppress diagnostics.
#[test]
fn no_diagnostic_for_stdclass_parameter() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function process(\stdClass $obj): void {
    $obj->foo;
    $obj->bar;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for property access on stdClass parameter, got: {:?}",
        diags
    );
}

/// A method returning stdClass should suppress diagnostics on the result.
#[test]
fn no_diagnostic_for_stdclass_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Factory {
    public function create(): \stdClass {
        return new \stdClass();
    }
}
$f = new Factory();
$f->create()->name;
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for property access on stdClass return type, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Method return → array access: $c->items()[0]->getLabel()
// ═══════════════════════════════════════════════════════════════════════════

/// When a method returns `Item[]` and the caller indexes inline
/// (`$c->items()[0]->getLabel()`), the element type should resolve
/// and no false "cannot verify" warning should appear.
#[test]
fn no_diagnostic_for_method_return_array_access_bracket_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Item {
    public function getLabel(): string { return ''; }
}
class Collection {
    /** @return Item[] */
    public function items(): array { return []; }
}
class Consumer {
    public function run(): void {
        $c = new Collection();
        $c->items()[0]->getLabel();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("getLabel")),
        "No diagnostic expected for getLabel on Item resolved via method-return array access, got: {:?}",
        diags
    );
}

/// Same pattern with `array<int, Item>` generic return type.
#[test]
fn no_diagnostic_for_method_return_array_access_generic_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Item {
    public function getLabel(): string { return ''; }
}
class Collection {
    /** @return array<int, Item> */
    public function items(): array { return []; }
}
class Consumer {
    public function run(): void {
        $c = new Collection();
        $c->items()[0]->getLabel();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("getLabel")),
        "No diagnostic expected for getLabel on Item resolved via generic method-return array access, got: {:?}",
        diags
    );
}

/// Static method returning an array: `Collection::all()[0]->getLabel()`.
#[test]
fn no_diagnostic_for_function_return_type_resolved_cross_file() {
    // Regression test: standalone functions store return types as short
    // names from the declaring file.  After FQN resolution in update_ast,
    // consumers in other files should resolve the type correctly.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Clock.php",
            r#"<?php
namespace App;

interface Clock {
    public function subMinutes(int $value = 1): Clock;
}
"#,
        )],
    );

    // A helper file that imports Clock via `use` and returns the short name.
    let helpers_uri = "file:///helpers.php";
    let helpers = r#"<?php
use App\Clock;

function now(): Clock {
    // stub
}
"#;
    backend.update_ast(helpers_uri, helpers);

    // Consumer file does NOT import App\Clock — it relies on the
    // function's return type being resolved to FQN at parse time.
    let uri = "file:///test.php";
    let text = r#"<?php
class Consumer {
    public function run(): void {
        now()->subMinutes(5);
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("subMinutes")),
        "No diagnostic expected for subMinutes on function return type resolved via FQN, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_self_return_type_in_cross_file_chain() {
    // Regression test: when a cross-file class has a method returning
    // `HasMany<self, $this>` (or any generic with `self`), the `self`
    // keyword must resolve to the *declaring* class, not get looked up
    // via the consuming file's use-map.  Previously, `self` was resolved
    // using `class_info.name` (the short name "TariffCode") which the
    // consuming file's class_loader could not find because it doesn't
    // import TariffCode.  The fix passes the FQN as owning_class_name
    // and uses find_class_by_name in resolve_named_type.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/TariffCode.php",
                r#"<?php
namespace App;

class TariffCode {
    public string $code = '';

    /** @return self[] */
    public function children(): array { return []; }
}
"#,
            ),
            (
                "src/OrderProduct.php",
                r#"<?php
namespace App;

class OrderProduct {
    public function __construct(
        public readonly ?TariffCode $tariffCode = null,
    ) {}
}
"#,
            ),
        ],
    );

    // Consumer file does NOT import App\TariffCode.  The chain
    // $tariffCode->children()[0]->code must still resolve because
    // children() returns `self[]` where `self` = App\TariffCode.
    let uri = "file:///test.php";
    let text = r#"<?php
use App\OrderProduct;

class Consumer {
    public function run(OrderProduct $op): void {
        $tariffCode = $op->tariffCode;
        if ($tariffCode) {
            $first = $tariffCode->children()[0];
            $first->code;
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("code")),
        "No diagnostic expected for 'code' on self-referencing return type resolved cross-file, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_static_method_return_array_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Item {
    public function getLabel(): string { return ''; }
}
class Collection {
    /** @return Item[] */
    public static function all(): array { return []; }
}

function test(): void {
    Collection::all()[0]->getLabel();
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);
    assert!(
        !diags.iter().any(|d| d.message.contains("getLabel")),
        "No diagnostic expected for getLabel on Item resolved via static method-return array access, got: {:?}",
        diags
    );
}

/// `$app['config']->set(...)` where `Application implements ArrayAccess`
/// without concrete generic annotations should NOT resolve the bracket
/// access to `Application` itself.  With `unresolved-member-access`
/// enabled, it should emit a diagnostic saying the type could not be
/// resolved.
#[test]
fn array_access_on_array_access_class_emits_unresolved_diagnostic() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Application implements \ArrayAccess {
    public function offsetExists(mixed $offset): bool { return true; }
    public function offsetGet(mixed $offset): mixed { return null; }
    public function offsetSet(mixed $offset, mixed $value): void {}
    public function offsetUnset(mixed $offset): void {}

    public function useStoragePath(string $path): void {}
}

function test(Application $app): void {
    $app->useStoragePath('/tmp');
    $app['config']->set('logging.default', 'stderr');
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    // $app->useStoragePath() should NOT be flagged (valid method).
    assert!(
        !diags.iter().any(|d| d.message.contains("useStoragePath")),
        "useStoragePath is a valid method on Application, got: {diags:?}",
    );
    // $app['config']->set() should NOT say 'set' is missing on Application.
    assert!(
        !diags.iter().any(|d| d.message.contains("Application")),
        "should not report 'set' as missing on Application, got: {diags:?}",
    );
    // $app['config']->set() SHOULD flag that the subject type is unresolved.
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("set") && d.message.contains("could not be resolved")),
        "expected unresolved-member-access diagnostic for $app['config']->set(), got: {diags:?}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Assert narrowing boundary prevents stale diagnostic cache reuse
// ═══════════════════════════════════════════════════════════════════════════

/// When a variable is used in a member access *before* an
/// `assert($var instanceof X)` and then used again *after* the assert,
/// the diagnostic cache must not reuse the pre-assert resolution.
/// Without the assert-offset discriminator in the cache key, the second
/// access would reuse the cached pre-assert type and produce a false
/// positive "property not found" diagnostic.
///
/// This reproduces the real-world Mockery pattern: `mock()` returns
/// `MockInterface`, the test calls `->shouldReceive()` (valid on
/// `MockInterface`), then `assert($x instanceof ConcreteClass)` narrows
/// the type so that `->id` (a property on the concrete class) is valid.
#[test]
fn no_false_positive_after_assert_instanceof() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface MockInterface {
    public function shouldReceive(string $name): self;
}
class MolliePayment {
    public string $id = '';
    public function canBeRefunded(): bool { return true; }
}
class TestCase {
    protected function mock(string $class): MockInterface {}
}
class Test extends TestCase {
    public function test(): void {
        $x = $this->mock(MolliePayment::class);
        $x->shouldReceive('canBeRefunded');
        assert($x instanceof MolliePayment);
        echo $x->id;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("id")),
        "No diagnostic expected for 'id' after assert($x instanceof MolliePayment), got: {:?}",
        diags
    );
}

/// Verify that the pre-assert access is still correctly diagnosed when
/// the member does NOT exist on the pre-assert type.
#[test]
fn still_flags_unknown_member_before_assert_instanceof() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface MockInterface {
    public function shouldReceive(string $name): self;
}
class MolliePayment {
    public string $id = '';
    public function canBeRefunded(): bool { return true; }
}
class TestCase {
    protected function mock(string $class): MockInterface {}
}
class Test extends TestCase {
    public function test(): void {
        $x = $this->mock(MolliePayment::class);
        echo $x->id;
        assert($x instanceof MolliePayment);
        echo $x->id;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    // The first $x->id (before the assert) should be flagged because
    // $x is MockInterface and MockInterface has no 'id' property.
    let id_diags: Vec<_> = diags.iter().filter(|d| d.message.contains("id")).collect();
    assert_eq!(
        id_diags.len(),
        1,
        "Expected exactly one diagnostic for 'id' (the pre-assert access), got: {:?}",
        id_diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Static return type resolution to concrete subclass
// ═══════════════════════════════════════════════════════════════════════════

/// When a parent class declares `public static function first(): ?static`,
/// calling `ChildClass::first()` should resolve `static` to `ChildClass`,
/// not the parent. No false-positive diagnostics should be emitted for
/// members that exist on the child class.
#[test]
fn no_diagnostic_for_static_return_type_on_subclass_static_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Model {
    /** @return ?static */
    public static function first(): ?static { return null; }
    public function save(): bool { return true; }
}
class AdminUser extends Model {
    public function assignRole(string $role): void {}
}
class Seeder {
    public function run(): void {
        $admin = AdminUser::first();
        $admin->assignRole('admin');
        $admin->save();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected when static return type resolves to subclass, got: {:?}",
        diags
    );
}

/// Same scenario but with a bare `static` return (non-nullable).
#[test]
fn no_diagnostic_for_bare_static_return_type_on_subclass() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Builder {
    /** @return static */
    public static function create(): static { return new static(); }
    public function build(): void {}
}
class AppBuilder extends Builder {
    public function setDebug(): void {}
}
class Factory {
    public function make(): void {
        $b = AppBuilder::create();
        $b->setDebug();
        $b->build();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for bare static return on subclass, got: {:?}",
        diags
    );
}

/// Chained static method calls: `Product::query()->where('x')->get()`
/// where `query()` and `where()` both return `static`.
#[test]
fn no_diagnostic_for_static_return_chained_static_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Model {
    /** @return static */
    public static function query(): static { return new static(); }
    /** @return static */
    public function where(string $col): static { return $this; }
    public function get(): array { return []; }
}
class Product extends Model {
    public function applyDiscount(): void {}
}
class Controller {
    public function index(): void {
        $q = Product::query();
        $q->where('active');
        $q->applyDiscount();
        $q->get();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for chained static return calls, got: {:?}",
        diags
    );
}

/// Cross-file variant: parent with `?static` return lives in a separate
/// PSR-4 file. Accessing subclass-specific members after a static method
/// call should not produce false-positive diagnostics.
#[test]
fn no_diagnostic_for_static_return_type_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Model.php",
                r#"<?php
namespace App;

class Model {
    /** @return ?static */
    public static function first(): ?static { return null; }
    public function save(): bool { return true; }
}
"#,
            ),
            (
                "src/AdminUser.php",
                r#"<?php
namespace App;

class AdminUser extends Model {
    public function assignRole(string $role): void {}
}
"#,
            ),
        ],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\AdminUser;

class Seeder {
    public function run(): void {
        $admin = AdminUser::first();
        $admin->assignRole('admin');
        $admin->save();
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        diags.is_empty(),
        "No diagnostics expected when static return type resolves to subclass cross-file, got: {:?}",
        diags
    );
}

// ─── Eloquent relationship property diagnostics ────────────────────────

#[test]
fn no_diagnostic_for_relationship_property_on_model() {
    // When a model has a relationship method (e.g. translations() returning
    // HasMany<Translation>), the LaravelModelProvider synthesizes a virtual
    // property `$translations` typed as Collection<Translation>.  Accessing
    // this property should not produce a diagnostic.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Relations/HasMany.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;

/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class HasMany {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Collection.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TModel
 */
class Collection {
    /** @return TModel|null */
    public function first(): mixed { return null; }
}
"#,
            ),
            (
                "src/Translation.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class Translation extends Model {
    public string $locale;
}
"#,
            ),
            (
                "src/Category.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;

class Category extends Model {
    /** @return HasMany<Translation, $this> */
    public function translations(): HasMany { return $this->hasMany(Translation::class); }
}
"#,
            ),
        ],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\Category;

class Service {
    public function test(Category $cat): void {
        $items = $cat->translations;
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        !diags.iter().any(|d| d.message.contains("translations")),
        "Relationship property 'translations' should be resolved via LaravelModelProvider, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_has_one_relationship_property_on_model() {
    // HasOne relationship produces a virtual property typed as the related model.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Relations/HasOne.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;

/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class HasOne {}
"#,
            ),
            (
                "src/ImageFile.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class ImageFile extends Model {
    public string $path;
}
"#,
            ),
            (
                "src/Notification.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasOne;

class Notification extends Model {
    /** @return HasOne<ImageFile, $this> */
    public function imageFile(): HasOne { return $this->hasOne(ImageFile::class); }
}
"#,
            ),
        ],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\Notification;

class Handler {
    public function process(Notification $notif): void {
        $file = $notif->imageFile;
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        !diags.iter().any(|d| d.message.contains("imageFile")),
        "HasOne relationship property 'imageFile' should be resolved via LaravelModelProvider, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_this_relationship_property_inside_model() {
    // Accessing $this->translations inside the model itself (e.g. in a
    // method body) should resolve the virtual relationship property.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Relations/HasMany.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;

/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class HasMany {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Collection.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TModel
 */
class Collection {
    /** @return TModel|null */
    public function first(): mixed { return null; }
}
"#,
            ),
            (
                "src/Translation.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class Translation extends Model {
    public string $locale;
}
"#,
            ),
        ],
    );

    let uri = "file:///src/Category.php";
    let text = r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;

class Category extends Model {
    /** @return HasMany<Translation, $this> */
    public function translations(): HasMany { return $this->hasMany(Translation::class); }

    public function defaultTranslation(): ?Translation {
        return $this->translations->first();
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        !diags.iter().any(|d| d.message.contains("translations")),
        "Relationship property '$this->translations' should be resolved inside model, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_belongs_to_associate_method() {
    // Calling a relationship method WITH () returns the relationship object
    // (e.g. BelongsTo).  Methods like associate() should be found on it.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Relations/BelongsTo.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;

/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class BelongsTo {
    /** @return TDeclaringModel */
    public function associate(mixed $model): static { return $this; }
    public function dissociate(): static { return $this; }
}
"#,
            ),
            (
                "src/ParentModel.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class ParentModel extends Model {
    public string $name;
}
"#,
            ),
            (
                "src/ChildModel.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class ChildModel extends Model {
    /** @return BelongsTo<ParentModel, $this> */
    public function parent(): BelongsTo { return $this->belongsTo(ParentModel::class); }
}
"#,
            ),
        ],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\ChildModel;
use App\ParentModel;

class Service {
    public function link(ChildModel $child, ParentModel $parent): void {
        $child->parent()->associate($parent);
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        !diags.iter().any(|d| d.message.contains("associate")),
        "BelongsTo::associate() should be resolved on relationship method return type, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_belongs_to_with_covariant_this() {
    // When the return type uses `covariant $this` syntax
    // (e.g. BelongsTo<Category, covariant $this>), the type parser
    // should still resolve the BelongsTo class and find its methods.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Relations/BelongsTo.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;

/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class BelongsTo {
    /** @return TDeclaringModel */
    public function associate(mixed $model): static { return $this; }
    public function dissociate(): static { return $this; }
}
"#,
            ),
            (
                "src/Category.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class Category extends Model {}
"#,
            ),
            (
                "src/Translation.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class Translation extends Model {
    /** @return BelongsTo<Category, covariant $this> */
    public function category(): BelongsTo { return $this->belongsTo(Category::class); }
}
"#,
            ),
        ],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\Translation;
use App\Category;

class Service {
    public function link(Translation $trans, Category $cat): void {
        $trans->category()->associate($cat);
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        !diags.iter().any(|d| d.message.contains("associate")),
        "BelongsTo::associate() should be resolved even with 'covariant $this' syntax, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_relationship_property_inferred_from_body() {
    // When a relationship method has no @return annotation but the body
    // contains `$this->hasMany(Related::class)`, the parser infers the
    // return type and the LaravelModelProvider should synthesize a virtual
    // property from it.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Relations/HasMany.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;

/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class HasMany {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Collection.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TModel
 */
class Collection {}
"#,
            ),
            (
                "src/Comment.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class Comment extends Model {
    public string $body;
}
"#,
            ),
            (
                "src/Post.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class Post extends Model {
    public function comments() { return $this->hasMany(Comment::class); }
}
"#,
            ),
        ],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\Post;

class Handler {
    public function test(Post $post): void {
        $items = $post->comments;
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        !diags.iter().any(|d| d.message.contains("comments")),
        "Body-inferred relationship property 'comments' should be resolved, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_relationship_property_with_mixed_native_return() {
    // In real Laravel projects, relationship methods often declare `mixed`
    // as the native return type with the specific relationship type only
    // in the @return docblock.  The LaravelModelProvider must still
    // synthesize the virtual property from the docblock return type.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Relations/HasMany.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;

/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class HasMany {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Relations/HasOne.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;

/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class HasOne {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Relations/BelongsTo.php",
                r#"<?php
namespace Illuminate\Database\Eloquent\Relations;

/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class BelongsTo {
    /** @return TDeclaringModel */
    public function associate(mixed $model): static { return $this; }
    public function dissociate(): static { return $this; }
}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Collection.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TModel
 */
class Collection {
    /** @return TModel|null */
    public function first(): mixed { return null; }
}
"#,
            ),
            (
                "src/Translation.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class Translation extends Model {
    public string $locale;
}
"#,
            ),
            (
                "src/ImageFile.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class ImageFile extends Model {
    public string $path;
}
"#,
            ),
            (
                "src/Category.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class Category extends Model {
    public string $name;
}
"#,
            ),
            (
                "src/NotificationCategory.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;

class NotificationCategory extends Model {
    /**
     * @return HasMany<Translation, $this>
     */
    public function translations(): mixed { return $this->hasMany(Translation::class); }

    public function defaultTranslation(): mixed {
        return $this->translations->first();
    }
}
"#,
            ),
            (
                "src/NotificationObject.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasOne;

class NotificationObject extends Model {
    /**
     * @return HasOne<ImageFile, $this>
     */
    public function imageFile(): mixed { return $this->hasOne(ImageFile::class); }

    public function getImagePath(): mixed {
        return $this->imageFile->path;
    }
}
"#,
            ),
            (
                "src/TranslationModel.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class TranslationModel extends Model {
    /**
     * @return BelongsTo<Category, covariant $this>
     */
    public function category(): mixed { return $this->belongsTo(Category::class); }
}
"#,
            ),
        ],
    );

    // Test 1: $this->translations inside model (HasMany virtual property)
    let uri1 = "file:///src/NotificationCategory.php";
    let text1 = std::fs::read_to_string(_dir.path().join("src/NotificationCategory.php")).unwrap();
    backend.update_ast(uri1, &text1);
    let mut diags1 = Vec::new();
    backend.collect_unknown_member_diagnostics(uri1, &text1, &mut diags1);
    assert!(
        !diags1.iter().any(|d| d.message.contains("translations")),
        "HasMany relationship property '$this->translations' with mixed native return should resolve, got: {:?}",
        diags1
    );

    // Test 2: $this->imageFile inside model (HasOne virtual property)
    let uri2 = "file:///src/NotificationObject.php";
    let text2 = std::fs::read_to_string(_dir.path().join("src/NotificationObject.php")).unwrap();
    backend.update_ast(uri2, &text2);
    let mut diags2 = Vec::new();
    backend.collect_unknown_member_diagnostics(uri2, &text2, &mut diags2);
    assert!(
        !diags2.iter().any(|d| d.message.contains("imageFile")),
        "HasOne relationship property '$this->imageFile' with mixed native return should resolve, got: {:?}",
        diags2
    );

    // Test 3: $translation->category()->associate() (BelongsTo with covariant $this)
    let uri3 = "file:///consumer.php";
    let text3 = r#"<?php
use App\TranslationModel;
use App\Category;

class NotificationCategoryService {
    public function link(TranslationModel $translation, Category $cat): void {
        $translation->category()->associate($cat);
    }
}
"#;
    backend.update_ast(uri3, text3);
    let mut diags3 = Vec::new();
    backend.collect_unknown_member_diagnostics(uri3, text3, &mut diags3);
    assert!(
        !diags3.iter().any(|d| d.message.contains("associate")),
        "BelongsTo::associate() should be found when method returns mixed with @return BelongsTo<..., covariant $this>, got: {:?}",
        diags3
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// @mixin with template parameter resolved via property generic type
// ═══════════════════════════════════════════════════════════════════════════

/// When a class declares `@template TWraps` and `@mixin TWraps`, and a
/// property is typed as `Wrapper<ConcreteApi>`, calling methods from
/// `ConcreteApi` on the property should NOT produce unknown_member
/// diagnostics.  This is the Klaviyo SDK pattern.
#[test]
fn no_diagnostic_for_mixin_template_param_via_property_generic() {
    let backend = create_test_backend();

    let wrapper_uri = "file:///Subclient.php";
    let wrapper_text = r#"<?php
/**
 * @template TWraps of object
 * @mixin TWraps
 */
class Subclient {
    public function getApiInstance(): object {}
}
"#;

    let api_uri = "file:///EventsApi.php";
    let api_text = r#"<?php
class EventsApi {
    public function createEvent(array $body): array {}
    public function getEvents(string $filter): array {}
}
"#;

    let consumer_uri = "file:///KlaviyoClient.php";
    let consumer_text = r#"<?php
class KlaviyoClient {
    /** @var Subclient<EventsApi> */
    public $Events;

    function test() {
        $this->Events->createEvent([]);
        $this->Events->getEvents('filter');
        $this->Events->getApiInstance();
    }
}
"#;

    backend.update_ast(wrapper_uri, wrapper_text);
    backend.update_ast(api_uri, api_text);
    backend.update_ast(consumer_uri, consumer_text);

    let diags = unknown_member_diagnostics(&backend, consumer_uri, consumer_text);
    assert!(
        !diags.iter().any(|d| d.message.contains("createEvent")),
        "createEvent from mixin TWraps→EventsApi should not be flagged, got: {:?}",
        diags
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("getEvents")),
        "getEvents from mixin TWraps→EventsApi should not be flagged, got: {:?}",
        diags
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("getApiInstance")),
        "Own method getApiInstance should not be flagged, got: {:?}",
        diags
    );
}

/// Calling a method that does NOT exist on the concrete mixin target
/// should still be flagged as unknown_member.
#[test]
fn diagnostic_for_nonexistent_method_on_mixin_template_param() {
    let backend = create_test_backend();

    let wrapper_uri = "file:///Wrapper.php";
    let wrapper_text = r#"<?php
/**
 * @template T of object
 * @mixin T
 */
class Wrapper {}
"#;

    let api_uri = "file:///Api.php";
    let api_text = r#"<?php
class Api {
    public function realMethod(): void {}
}
"#;

    let consumer_uri = "file:///Consumer.php";
    let consumer_text = r#"<?php
class Consumer {
    /** @var Wrapper<Api> */
    public $api;

    function test() {
        $this->api->fakeMethod();
    }
}
"#;

    backend.update_ast(wrapper_uri, wrapper_text);
    backend.update_ast(api_uri, api_text);
    backend.update_ast(consumer_uri, consumer_text);

    let diags = unknown_member_diagnostics(&backend, consumer_uri, consumer_text);
    assert!(
        diags.iter().any(|d| d.message.contains("fakeMethod")),
        "fakeMethod does not exist on Api and should be flagged, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// @mixin with template parameter — namespaced (Klaviyo SDK pattern)
// ═══════════════════════════════════════════════════════════════════════════

/// Reproduces the exact Klaviyo SDK pattern with namespaces:
///   - `KlaviyoAPI\Subclient` has `@template TWraps of object` + `@mixin TWraps`
///   - `KlaviyoAPI\KlaviyoAPI` has `/** @var Subclient<EventsApi> */ public $Events;`
///   - A consumer calls `$this->getClient()->Events->createEvent([])`
///
/// The mixin template parameter must resolve through the `@var` generic
/// annotation even when all classes live in different namespaces.
#[test]
fn no_diagnostic_for_mixin_template_param_namespaced_klaviyo_pattern() {
    let backend = create_test_backend();

    let subclient_uri = "file:///vendor/klaviyo/Subclient.php";
    let subclient_text = r#"<?php
namespace KlaviyoAPI;

/**
 * @template TWraps of object
 * @mixin TWraps
 */
class Subclient {
    public function __call(string $name, array $args): mixed {}
}
"#;

    let events_api_uri = "file:///vendor/klaviyo/EventsApi.php";
    let events_api_text = r#"<?php
namespace KlaviyoAPI\API;

class EventsApi {
    public function createEvent(array $body): array {}
    public function getEvents(string $filter): array {}
}
"#;

    let profiles_api_uri = "file:///vendor/klaviyo/ProfilesApi.php";
    let profiles_api_text = r#"<?php
namespace KlaviyoAPI\API;

class ProfilesApi {
    public function getProfiles(?string $additional = null, ?array $fields = null, ?string $filter = null): array {}
    public function updateProfile(string $id, array $body): array {}
}
"#;

    let klaviyo_api_uri = "file:///vendor/klaviyo/KlaviyoAPI.php";
    let klaviyo_api_text = r#"<?php
namespace KlaviyoAPI;

use KlaviyoAPI\API\EventsApi;
use KlaviyoAPI\API\ProfilesApi;

class KlaviyoAPI {
    /** @var Subclient<EventsApi> */
    public $Events;
    /** @var Subclient<ProfilesApi> */
    public $Profiles;
}
"#;

    let service_uri = "file:///src/KlaviyoService.php";
    let service_text = r#"<?php
namespace App\Services;

use KlaviyoAPI\KlaviyoAPI;

class KlaviyoService {
    private ?KlaviyoAPI $client = null;

    private function getClient(): KlaviyoAPI
    {
        return $this->client;
    }

    public function testEvents(): void
    {
        $this->getClient()->Events->createEvent([]);
        $this->getClient()->Events->getEvents('filter');
    }

    public function testProfiles(): void
    {
        $this->getClient()->Profiles->getProfiles(null, ['email'], 'filter');
        $this->getClient()->Profiles->updateProfile('id123', []);
    }
}
"#;

    backend.update_ast(subclient_uri, subclient_text);
    backend.update_ast(events_api_uri, events_api_text);
    backend.update_ast(profiles_api_uri, profiles_api_text);
    backend.update_ast(klaviyo_api_uri, klaviyo_api_text);
    backend.update_ast(service_uri, service_text);

    let diags = unknown_member_diagnostics(&backend, service_uri, service_text);

    assert!(
        !diags.iter().any(|d| d.message.contains("createEvent")),
        "createEvent from mixin TWraps→EventsApi should not be flagged, got: {:?}",
        diags
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("getEvents")),
        "getEvents from mixin TWraps→EventsApi should not be flagged, got: {:?}",
        diags
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("getProfiles")),
        "getProfiles from mixin TWraps→ProfilesApi should not be flagged, got: {:?}",
        diags
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("updateProfile")),
        "updateProfile from mixin TWraps→ProfilesApi should not be flagged, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Scope methods not found on Builder in analyzer chains
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_scope_method_on_builder_in_static_chain() {
    // When a model has scope methods (e.g. scopeWhereIsLuxury), they should be
    // available on the Builder returned by static query methods like
    // whereHas().  The Builder-forwarded methods on the model substitute
    // `static` → `Builder<Model>`, and type_hint_to_classes_typed should
    // inject the model's scope methods onto that Builder.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Builder.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TModel of \Illuminate\Database\Eloquent\Model
 */
class Builder {
    /** @return static */
    public function where(string $column, mixed $operator = null, mixed $value = null): static { return $this; }
    /** @return static */
    public function whereHas(string $relation, ?\Closure $callback = null): static { return $this; }
    /** @return static */
    public function orderBy(string $column, string $direction = 'asc'): static { return $this; }
    /** @return \Illuminate\Database\Eloquent\Collection<int, TModel> */
    public function get(): Collection { return new Collection(); }
    /**
     * @template TValue
     * @param string $column
     * @return \Illuminate\Support\Collection<int, TValue>
     */
    public function pluck(string $column): \Illuminate\Support\Collection { return new \Illuminate\Support\Collection(); }
}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Collection.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TKey of array-key
 * @template TModel
 */
class Collection {
    /** @return TModel|null */
    public function first(): mixed { return null; }
}
"#,
            ),
            (
                "illuminate/Support/Collection.php",
                r#"<?php
namespace Illuminate\Support;

/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {
    /** @return array<TKey, TValue> */
    public function all(): array { return []; }
}
"#,
            ),
            (
                "src/Product.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;

class Product extends Model {
    public function scopeWhereIsLuxury(Builder $query): Builder { return $query->where('is_luxury', true); }
    public function scopeWhereIsDerma(Builder $query): Builder { return $query->where('is_derma', true); }
    public function scopeWhereIsProHairCare(Builder $query): Builder { return $query->where('is_pro_hair_care', true); }
}
"#,
            ),
        ],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\Product;

class ProductRepository {
    public function getFiltered(bool $onlyLuxury): void {
        $products = Product::whereHas('translations')
            ->whereIsLuxury()
            ->whereIsDerma()
            ->whereIsProHairCare()
            ->get();
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        !diags.iter().any(|d| d.message.contains("whereIsLuxury")),
        "Scope method 'whereIsLuxury' should be found on Builder<Product>, got: {:?}",
        diags
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("whereIsDerma")),
        "Scope method 'whereIsDerma' should be found on Builder<Product>, got: {:?}",
        diags
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("whereIsProHairCare")),
        "Scope method 'whereIsProHairCare' should be found on Builder<Product>, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_scope_method_after_wherehas_with_closure() {
    // Same as above but with a closure argument to whereHas, matching
    // the real-world pattern from EventRepository.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Builder.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TModel of \Illuminate\Database\Eloquent\Model
 */
class Builder {
    /** @return static */
    public function where(string $column, mixed $operator = null, mixed $value = null): static { return $this; }
    /**
     * @param  string  $relation
     * @param  (\Closure(\Illuminate\Database\Eloquent\Builder<TModel>): mixed)|null  $callback
     * @return static
     */
    public function whereHas(string $relation, ?\Closure $callback = null): static { return $this; }
    /**
     * @template TValue
     * @param string $column
     * @return \Illuminate\Support\Collection<int, TValue>
     */
    public function pluck(string $column): \Illuminate\Support\Collection { return new \Illuminate\Support\Collection(); }
}
"#,
            ),
            (
                "illuminate/Support/Collection.php",
                r#"<?php
namespace Illuminate\Support;

/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {
    /** @return array<TKey, TValue> */
    public function all(): array { return []; }
}
"#,
            ),
            (
                "src/Product.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;

class Product extends Model {
    public function scopeWhereIsBlackFriday(Builder $query): Builder { return $query->where('is_black_friday', true); }
    public function scopeWhereIsVisible(Builder $query): Builder { return $query->where('is_visible', true); }
}
"#,
            ),
        ],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\Product;
use Illuminate\Database\Eloquent\Builder;

class EventRepository {
    public function getProductIds(): array {
        $ids = Product::whereHas(
            'translations',
            fn(Builder $query): Builder => $query->where('lang_code', 'en')
        )
            ->whereIsBlackFriday()
            ->whereIsVisible()
            ->pluck('id')
            ->all();
        return $ids;
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("whereIsBlackFriday")),
        "Scope method 'whereIsBlackFriday' should be found on Builder<Product>, got: {:?}",
        diags
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("whereIsVisible")),
        "Scope method 'whereIsVisible' should be found on Builder<Product>, got: {:?}",
        diags
    );
    // pluck and all should also resolve without issues
    assert!(
        !diags.iter().any(|d| d.message.contains("pluck")),
        "pluck should be found on Builder after scope methods, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_scope_in_when_closure_with_callable_inference() {
    // When a closure parameter is typed as bare `Builder` but the
    // enclosing method's callable signature provides `$this`/`static`,
    // the inferred type is refined to `Builder<Product>` (a supertype
    // match with generic args).  Scope methods are then found on the
    // refined type and should NOT be flagged.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Builder.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TModel of \Illuminate\Database\Eloquent\Model
 */
class Builder {
    /** @return static */
    public function where(string $column, mixed $operator = null, mixed $value = null): static { return $this; }
    /** @return static */
    public function whereHas(string $relation, ?\Closure $callback = null): static { return $this; }
    /**
     * @param bool $value
     * @param callable(static): static $callback
     * @return static
     */
    public function when(bool $value, callable $callback): static { return $this; }
    /** @return \Illuminate\Database\Eloquent\Collection<int, TModel> */
    public function get(): Collection { return new Collection(); }

    /** @return mixed */
    public function __call(string $method, array $parameters): mixed { return null; }
}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Collection.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TKey of array-key
 * @template TModel
 */
class Collection {}
"#,
            ),
            (
                "src/Product.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;

class Product extends Model {
    public function scopeWhereIsLuxury(Builder $query): Builder { return $query->where('is_luxury', true); }
    public function scopeWhereIsDerma(Builder $query): Builder { return $query->where('is_derma', true); }
}
"#,
            ),
        ],
    );

    let uri = "file:///consumer.php";
    let text = r#"<?php
use App\Product;
use Illuminate\Database\Eloquent\Builder;

class ProductRepository {
    public function getFiltered(bool $onlyLuxury, bool $onlyDerma): void {
        Product::whereHas('translations')
            ->when($onlyLuxury, fn(Builder $q): Builder => $q->whereIsLuxury())
            ->when($onlyDerma, fn(Builder $q): Builder => $q->whereIsDerma())
            ->get();
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    // The callable signature `callable(static)` on `when()` provides
    // `static` as the closure param type.  Since the receiver is
    // `Builder<Product>`, `static` resolves to `Builder<Product>`.
    // The explicit `Builder` type hint is a supertype, so the inferred
    // `Builder<Product>` is preferred — scope methods are found.
    assert!(
        !diags.iter().any(|d| d.message.contains("whereIsLuxury")),
        "Scope method should be found via callable param inference from when(), got: {:?}",
        diags
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("whereIsDerma")),
        "Scope method should be found via callable param inference from when(), got: {:?}",
        diags
    );

    // Known methods after the scope calls should also resolve.
    assert!(
        !diags.iter().any(|d| d.message.contains("get")),
        "Known method 'get' should resolve after scope calls, got: {:?}",
        diags
    );
    // No broken-chain / unresolved diagnostics downstream.
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("could not be resolved")),
        "Chain should not break, got: {:?}",
        diags
    );
}

#[test]
fn scope_on_standalone_bare_builder_param_flags_warning_chain_continues() {
    // When a function parameter is typed as bare `Builder` (no callable
    // inference context), scope methods cannot be verified statically.
    // They are flagged via MagicFallback (__call exists), but the chain
    // continues because Builder's __call return type is patched to
    // `static` during resolution.
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/", "Illuminate\\": "illuminate/" } } }"#,
        &[
            (
                "illuminate/Database/Eloquent/Model.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

class Model {}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Builder.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TModel of \Illuminate\Database\Eloquent\Model
 */
class Builder {
    /** @return static */
    public function where(string $column, mixed $operator = null, mixed $value = null): static { return $this; }
    /** @return static */
    public function orderBy(string $column, string $direction = 'asc'): static { return $this; }
    /** @return \Illuminate\Database\Eloquent\Collection<int, TModel> */
    public function get(): Collection { return new Collection(); }

    /** @return mixed */
    public function __call(string $method, array $parameters): mixed { return null; }
}
"#,
            ),
            (
                "illuminate/Database/Eloquent/Collection.php",
                r#"<?php
namespace Illuminate\Database\Eloquent;

/**
 * @template TKey of array-key
 * @template TModel
 */
class Collection {}
"#,
            ),
            (
                "src/Product.php",
                r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;

class Product extends Model {
    public function scopeWhereIsLuxury(Builder $query): Builder { return $query->where('is_luxury', true); }
}
"#,
            ),
        ],
    );

    let uri = "file:///consumer.php";
    // Standalone function parameter — no callable inference context.
    let text = r#"<?php
use Illuminate\Database\Eloquent\Builder;

function filterProducts(Builder $query): void {
    $query->whereIsLuxury()->orderBy('name')->get();
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, text, &mut diags);

    // Scope method IS flagged — no callable inference to refine the
    // bare Builder to Builder<Product>.
    assert!(
        diags.iter().any(|d| d.message.contains("whereIsLuxury")),
        "Scope method on standalone bare Builder param should be flagged, got: {:?}",
        diags
    );

    // Chain continues — known methods after the unknown scope call
    // should NOT be flagged because __call returns static.
    assert!(
        !diags.iter().any(|d| d.message.contains("orderBy")),
        "Known method 'orderBy' should resolve after __call fallback, got: {:?}",
        diags
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("get")),
        "Known method 'get' should resolve after __call fallback, got: {:?}",
        diags
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("could not be resolved")),
        "Chain should not break after __call fallback, got: {:?}",
        diags
    );
}

/// Cross-file variant: `Collection::reduce()` loaded via PSR-4 with
/// two method-level `@template` params and a `callable(TReduceInitial|TReduceReturnType, TValue, TKey): TReduceReturnType`
/// parameter.  The return type `TReduceReturnType` must be inferred from the
/// closure's return type annotation even when the Collection class lives in
/// a separate file.
#[test]
fn no_false_positive_on_reduce_two_tpl_cross_file() {
    let composer = r#"{"autoload":{"psr-4":{"App\\":"src/","Illuminate\\Support\\":"vendor/illuminate/support/src/"}}}"#;

    let collection_php = r#"<?php
namespace Illuminate\Support;

/**
 * @template TKey
 * @template TValue
 */
class Collection {
    /**
     * @template TReduceInitial
     * @template TReduceReturnType
     * @param callable(TReduceInitial|TReduceReturnType, TValue, TKey): TReduceReturnType $callback
     * @param TReduceInitial $initial
     * @return TReduceInitial|TReduceReturnType
     */
    public function reduce(callable $callback, mixed $initial = null): mixed {}
}
"#;

    let decimal_php = r#"<?php
namespace App;

class Decimal {
    public function add(Decimal $other): Decimal { return $this; }
    public function getValue(): string { return '0'; }
}
"#;

    let order_product_php = r#"<?php
namespace App;

class OrderProduct {
    public float $price;
}
"#;

    let service_php = r#"<?php
namespace App;

use Illuminate\Support\Collection;

class FlowService {
    public function test(): void {
        /** @var Collection<int, OrderProduct> $products */
        $products = new Collection();
        $products->reduce(fn(Decimal $c, OrderProduct $p): Decimal => $c->add($p->price), new Decimal('0'))->add(new Decimal('1'));
        $products->reduce(fn(Decimal $c, OrderProduct $p): Decimal => $c->add($p->price), new Decimal('0'))->getValue();
    }
}
"#;

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            (
                "vendor/illuminate/support/src/Collection.php",
                collection_php,
            ),
            ("src/Decimal.php", decimal_php),
            ("src/OrderProduct.php", order_product_php),
            ("src/FlowService.php", service_php),
        ],
    );

    let uri = &format!(
        "file://{}",
        _dir.path().join("src/FlowService.php").display()
    );
    let diags = unknown_member_diagnostics(&backend, uri, service_php);

    let chained_diags: Vec<_> = diags.iter().filter(|d| !d.message.contains("$c")).collect();
    assert!(
        !chained_diags.iter().any(|d| d.message.contains("add")),
        "reduce() should resolve TReduceReturnType=Decimal cross-file, chained 'add' should be known, got: {:?}",
        chained_diags
    );
    assert!(
        !chained_diags.iter().any(|d| d.message.contains("getValue")),
        "reduce() should resolve TReduceReturnType=Decimal cross-file, chained 'getValue' should be known, got: {:?}",
        chained_diags
    );
    assert!(
        !chained_diags
            .iter()
            .any(|d| d.message.contains("could not be resolved")),
        "reduce() return type should be fully resolved cross-file when chained, got: {:?}",
        chained_diags
    );
}

/// Cross-file test modelling the real Laravel structure: `Collection` uses
/// a trait `EnumeratesValues` (which defines `reduce()` with
/// `@return TReduceReturnType`) and implements an interface `Enumerable`
/// (which declares `reduce()` with `@return TReduceInitial|TReduceReturnType`).
/// The inheritance merger might pick up the interface's union return type,
/// so the template substitution must handle both template params in the
/// return type union.
///
/// Regression test for template inference through trait + interface + collection reduce.
#[test]
fn no_false_positive_on_reduce_trait_interface_pattern() {
    let composer = r#"{"autoload":{"psr-4":{"App\\":"src/","Illuminate\\Support\\":"vendor/illuminate/support/src/"}}}"#;

    let enumerable_php = r#"<?php
namespace Illuminate\Support;

/**
 * @template TKey
 * @template TValue
 */
interface Enumerable {
    /**
     * @template TReduceInitial
     * @template TReduceReturnType
     * @param callable(TReduceInitial|TReduceReturnType, TValue, TKey): TReduceReturnType $callback
     * @param TReduceInitial $initial
     * @return TReduceInitial|TReduceReturnType
     */
    public function reduce(callable $callback, $initial = null);
}
"#;

    let trait_php = r#"<?php
namespace Illuminate\Support;

trait EnumeratesValues {
    /**
     * @template TReduceInitial
     * @template TReduceReturnType
     * @param callable(TReduceInitial|TReduceReturnType, TValue, TKey): TReduceReturnType $callback
     * @param TReduceInitial $initial
     * @return TReduceReturnType
     */
    public function reduce(callable $callback, $initial = null)
    {
        $result = $initial;
        foreach ($this as $key => $value) {
            $result = $callback($result, $value, $key);
        }
        return $result;
    }
}
"#;

    let collection_php = r#"<?php
namespace Illuminate\Support;

/**
 * @template TKey
 * @template TValue
 * @implements Enumerable<TKey, TValue>
 */
class Collection implements Enumerable {
    use EnumeratesValues;
}
"#;

    let decimal_php = r#"<?php
namespace App;

class Decimal {
    public function add(Decimal $other): Decimal { return $this; }
    public function getValue(): string { return '0'; }
}
"#;

    let order_product_php = r#"<?php
namespace App;

class OrderProduct {
    public float $price;
}
"#;

    let service_php = r#"<?php
namespace App;

use Illuminate\Support\Collection;

class FlowService {
    public function test(): void {
        /** @var Collection<int, OrderProduct> $products */
        $products = new Collection();
        $products->reduce(fn(Decimal $c, OrderProduct $p): Decimal => $c->add($p->price), new Decimal('0'))->add(new Decimal('1'));
        $products->reduce(fn(Decimal $c, OrderProduct $p): Decimal => $c->add($p->price), new Decimal('0'))->getValue();
    }
}
"#;

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            (
                "vendor/illuminate/support/src/Enumerable.php",
                enumerable_php,
            ),
            (
                "vendor/illuminate/support/src/EnumeratesValues.php",
                trait_php,
            ),
            (
                "vendor/illuminate/support/src/Collection.php",
                collection_php,
            ),
            ("src/Decimal.php", decimal_php),
            ("src/OrderProduct.php", order_product_php),
            ("src/FlowService.php", service_php),
        ],
    );

    let uri = &format!(
        "file://{}",
        _dir.path().join("src/FlowService.php").display()
    );
    let diags = unknown_member_diagnostics(&backend, uri, service_php);

    let chained_diags: Vec<_> = diags.iter().filter(|d| !d.message.contains("$c")).collect();
    assert!(
        !chained_diags.iter().any(|d| d.message.contains("add")),
        "reduce() via trait+interface should resolve TReduceReturnType=Decimal, chained 'add' should be known, got: {:?}",
        chained_diags
    );
    assert!(
        !chained_diags.iter().any(|d| d.message.contains("getValue")),
        "reduce() via trait+interface should resolve TReduceReturnType=Decimal, chained 'getValue' should be known, got: {:?}",
        chained_diags
    );
    assert!(
        !chained_diags
            .iter()
            .any(|d| d.message.contains("could not be resolved")),
        "reduce() return type via trait+interface should be fully resolved when chained, got: {:?}",
        chained_diags
    );
}

/// `Collection::reduce()` with two method-level `@template` params
/// (`TReduceInitial`, `TReduceReturnType`) and a callable whose first
/// parameter is the union `TReduceInitial|TReduceReturnType`.  The
/// return type is `TReduceReturnType` which should be inferred from
/// the closure's return type annotation.  Chaining `.add()` on the
/// result must not produce a diagnostic.
///
/// Regression test for reduce with two template parameters.
#[test]
fn no_false_positive_on_reduce_with_two_template_params() {
    let backend = create_test_backend();
    let uri = "file:///test_reduce_two_tpl.php";
    let text = r#"<?php
class Decimal {
    public function add(Decimal $other): Decimal { return $this; }
    public function getValue(): string { return '0'; }
}

class OrderProduct {
    public float $price;
}

/**
 * @template TKey
 * @template TValue
 */
class Collection {
    /**
     * @template TReduceInitial
     * @template TReduceReturnType
     * @param callable(TReduceInitial|TReduceReturnType, TValue, TKey): TReduceReturnType $callback
     * @param TReduceInitial $initial
     * @return TReduceReturnType
     */
    public function reduce(callable $callback, mixed $initial = null): mixed {}
}

class FlowService {
    public function test(): void {
        /** @var Collection<int, OrderProduct> $products */
        $products = new Collection();
        $products->reduce(fn(Decimal $c, OrderProduct $p): Decimal => $c->add($p->price), new Decimal('0'))->add(new Decimal('1'));
        $products->reduce(fn(Decimal $c, OrderProduct $p): Decimal => $c->add($p->price), new Decimal('0'))->getValue();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    // Filter out diagnostics for the inner `$c->add($p->price)` inside
    // the closure — we only care about the chained call after reduce().
    let chained_diags: Vec<_> = diags.iter().filter(|d| !d.message.contains("$c")).collect();
    assert!(
        !chained_diags.iter().any(|d| d.message.contains("add")),
        "reduce() should resolve TReduceReturnType=Decimal, chained 'add' should be known, got: {:?}",
        chained_diags
    );
    assert!(
        !chained_diags.iter().any(|d| d.message.contains("getValue")),
        "reduce() should resolve TReduceReturnType=Decimal, chained 'getValue' should be known, got: {:?}",
        chained_diags
    );
    assert!(
        !chained_diags
            .iter()
            .any(|d| d.message.contains("could not be resolved")),
        "reduce() return type should be fully resolved when chained, got: {:?}",
        chained_diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Forward walker scope cache — assert instanceof narrowing
// ═══════════════════════════════════════════════════════════════════════════

/// `assert($param instanceof self)` inside a method should narrow the
/// parameter from the base class to the enclosing class.  When the
/// diagnostic scope cache is active, the forward walker must apply this
/// narrowing so that members of the subclass are found.
#[test]
fn scope_cache_assert_instanceof_self_narrows_parameter() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class BaseCatalogFeature {
    public function baseMethod(): void {}
}
class SpecificFeature extends BaseCatalogFeature {
    public function specificMethod(): void {}
    public function isBetterThanOther(BaseCatalogFeature $feature): bool {
        assert($feature instanceof self);
        return $feature->specificMethod() !== null;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("specificMethod")),
        "No diagnostic expected for 'specificMethod' after assert($feature instanceof self), got: {:?}",
        diags
    );
}

/// Same pattern but with a named class instead of `self`.
#[test]
fn scope_cache_assert_instanceof_named_class_narrows_parameter() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Animal {
    public function breathe(): void {}
}
class Dog extends Animal {
    public function bark(): void {}
}
class Handler {
    public function handle(Animal $pet): void {
        assert($pet instanceof Dog);
        $pet->bark();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("bark")),
        "No diagnostic expected for 'bark' after assert($pet instanceof Dog), got: {:?}",
        diags
    );
}

/// Assert narrowing should apply to body-assigned variables too, not
/// just parameters.
#[test]
fn scope_cache_assert_instanceof_narrows_assigned_variable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Renderable {
    public function render(): string;
}
class HtmlWidget implements Renderable {
    public function render(): string { return ''; }
    public function toHtml(): string { return ''; }
}
class Consumer {
    public function run(Renderable $r): void {
        $widget = $r;
        assert($widget instanceof HtmlWidget);
        $widget->toHtml();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("toHtml")),
        "No diagnostic expected for 'toHtml' after assert instanceof, got: {:?}",
        diags
    );
}

/// A generic `@phpstan-assert` on a static method declared on a base
/// class must narrow when called through a subclass via `$this->`,
/// `static::`, and `self::` (the PHPUnit `assertInstanceOf` shape).
/// Previously the metadata was only found when the call named the
/// declaring class directly, producing false `unresolved-member-access`
/// positives across PHPUnit-based test suites.
#[test]
fn scope_cache_phpstan_assert_inherited_narrows_via_this_static_self() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Node {
    public function getName(): string { return ''; }
}
class BaseAssert {
    /**
     * @template ExpectedType of object
     * @param class-string<ExpectedType> $expected
     * @phpstan-assert ExpectedType $actual
     */
    public static function assertInstanceOf(string $expected, object $actual): void {}
}
class NodeTest extends BaseAssert {
    public function testThis(mixed $v): void {
        $this->assertInstanceOf(Node::class, $v);
        $v->getName();
    }
    public function testStatic(mixed $v): void {
        static::assertInstanceOf(Node::class, $v);
        $v->getName();
    }
    public function testSelf(mixed $v): void {
        self::assertInstanceOf(Node::class, $v);
        $v->getName();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("getName")),
        "No diagnostic expected for 'getName' after inherited assertInstanceOf via \
         $this->/static::/self::, got: {diags:?}",
    );
}

/// An exact-type assertion `@phpstan-assert =Type $x` must not emit a
/// bogus `Class '=Type' not found` diagnostic on the docblock.
#[test]
fn exact_type_assertion_prefix_does_not_emit_unknown_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foobar {
    public function fooMethod(): void {}
}
class Asserter {
    /**
     * @phpstan-assert =Foobar $actual
     */
    public function assertIsFoobar(object $actual): void {}
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut diags);
    assert!(
        !diags.iter().any(|d| d.message.contains("=Foobar")),
        "No unknown-class diagnostic expected for the `=` exact-type prefix, got: {diags:?}",
    );
}

/// Members accessed BEFORE the assert should still be diagnosed when
/// they don't exist on the pre-assert type.
#[test]
fn scope_cache_still_flags_unknown_member_before_assert() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Base {
    public function baseMethod(): void {}
}
class Child extends Base {
    public function childMethod(): void {}
}
class Handler {
    public function handle(Base $item): void {
        $item->childMethod();
        assert($item instanceof Child);
        $item->childMethod();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    let child_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("childMethod"))
        .collect();
    assert_eq!(
        child_diags.len(),
        1,
        "Expected exactly 1 diagnostic for 'childMethod' (the pre-assert access), got: {:?}",
        child_diags
    );
}

/// Verify that `assert($x instanceof self)` inside a `final` class
/// with modifiers (which shifts the class span) resolves correctly.
#[test]
fn scope_cache_assert_instanceof_self_final_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class BaseFoo {
    public function baseOp(): void {}
}
final class ConcreteFoo extends BaseFoo {
    public function concreteOp(): void {}
    public function compare(BaseFoo $other): bool {
        assert($other instanceof self);
        return $other->concreteOp() !== null;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("concreteOp")),
        "No diagnostic expected for 'concreteOp' after assert instanceof self in final class, got: {:?}",
        diags
    );
}

/// `assert(!$x instanceof Foo)` — negated instanceof should exclude the
/// type, not include it.
#[test]
fn scope_cache_assert_negated_instanceof_excludes_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Alpha {
    public function alphaMethod(): void {}
}
class Beta extends Alpha {
    public function betaMethod(): void {}
}
class Tester {
    public function run(): void {
        $x = random_int(0,1) ? new Alpha() : new Beta();
        assert(!$x instanceof Beta);
        $x->alphaMethod();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("alphaMethod")),
        "No diagnostic expected for 'alphaMethod' after assert negated instanceof, got: {:?}",
        diags
    );
}

/// Instanceof narrowing inside an `if` condition should narrow the
/// variable in the then-branch for the scope cache path.
#[test]
fn scope_cache_if_instanceof_narrows_in_then_branch() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Shape {
    public function area(): float { return 0.0; }
}
class Circle extends Shape {
    public function radius(): float { return 1.0; }
}
class Renderer {
    public function draw(Shape $s): void {
        if ($s instanceof Circle) {
            $s->radius();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("radius")),
        "No diagnostic expected for 'radius' inside if-instanceof branch, got: {:?}",
        diags
    );
}

/// Short-circuit narrowing: the right operand of `||` executes only
/// when the left is false, so `!$s instanceof Circle || $s->radius()`
/// sees `$s` narrowed to `Circle` in the right operand.
#[test]
fn scope_cache_or_short_circuit_narrows_right_operand() {
    let backend = create_test_backend();
    let uri = "file:///test_or_shortcircuit.php";
    let text = r#"<?php
class Shape {
    public function area(): float { return 0.0; }
}
class Circle extends Shape {
    public function radius(): float { return 1.0; }
}
class Renderer {
    public function draw(Shape $s): void {
        if (!$s instanceof Circle || $s->radius() > 0.0) {
            return;
        }
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("radius")),
        "No diagnostic expected for 'radius' in the right operand of ||, got: {:?}",
        diags
    );
}

/// Short-circuit narrowing mirror: the right operand of `&&` executes
/// only when the left is true, so `$s instanceof Circle && $s->radius()`
/// sees `$s` narrowed to `Circle` in the right operand.
#[test]
fn scope_cache_and_short_circuit_narrows_right_operand() {
    let backend = create_test_backend();
    let uri = "file:///test_and_shortcircuit.php";
    let text = r#"<?php
class Shape {
    public function area(): float { return 0.0; }
}
class Circle extends Shape {
    public function radius(): float { return 1.0; }
}
class Renderer {
    public function draw(Shape $s): void {
        if ($s instanceof Circle && $s->radius() > 0.0) {
            return;
        }
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("radius")),
        "No diagnostic expected for 'radius' in the right operand of &&, got: {:?}",
        diags
    );
}

/// Nested `&&` inside the right operand of `||`: the inner `&&` chain
/// narrows independently on top of the outer short-circuit context.
/// `$node !== $other || ($s instanceof Circle && !$s->radius())` narrows
/// `$s` to `Circle` for the `$s->radius()` access.
#[test]
fn scope_cache_and_chain_inside_or_narrows() {
    let backend = create_test_backend();
    let uri = "file:///test_and_in_or.php";
    let text = r#"<?php
class Shape {
    public function area(): float { return 0.0; }
}
class Circle extends Shape {
    public function radius(): float { return 1.0; }
}
class Renderer {
    public function draw(Shape $s, int $node, int $other): bool {
        return $node !== $other
            || ($s instanceof Circle && $s->radius() > 0.0);
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("radius")),
        "No diagnostic expected for 'radius' in nested && inside ||, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Forward walker scope cache — top-level code
// ═══════════════════════════════════════════════════════════════════════════

/// Variables assigned in top-level code (outside any function or class body)
/// should be tracked by the forward walker's scope cache so that member
/// accesses on those variables resolve without falling through to the
/// backward scanner.
#[test]
fn scope_cache_top_level_variable_assignment() {
    let backend = create_test_backend();
    let uri = "file:///test_top_level.php";
    let text = r#"<?php
class Logger {
    public function info(string $msg): void {}
    public function warning(string $msg): void {}
}

$logger = new Logger();
$logger->info('hello');
$logger->warning('watch out');
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for top-level $logger->info()/warning(), got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Top-level code with an if-statement should still track variable types
/// across branches so that member accesses after the if resolve correctly.
#[test]
fn scope_cache_top_level_if_then_access() {
    let backend = create_test_backend();
    let uri = "file:///test_top_level_if.php";
    let text = r#"<?php
class Config {
    public function get(string $key): string { return ''; }
}

$config = new Config();
if (true) {
    $config->get('key');
}
$config->get('other');
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for top-level $config->get(), got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Top-level foreach should bind the value variable so that member
/// accesses inside the loop body resolve from the scope cache.
#[test]
fn scope_cache_top_level_foreach() {
    let backend = create_test_backend();
    let uri = "file:///test_top_level_foreach.php";
    let text = r#"<?php
class Item {
    public function getName(): string { return ''; }
}

/** @var list<Item> $items */
$items = [];
foreach ($items as $item) {
    $item->getName();
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for top-level foreach $item->getName(), got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Forward walker scope cache — foreach over method call result
// ═══════════════════════════════════════════════════════════════════════════

/// When iterating over a method call result like `$this->getItems()`,
/// the forward walker should resolve the expression through the full
/// resolver pipeline (subject-based resolution) so that the foreach
/// value variable gets the correct element type.
#[test]
fn scope_cache_foreach_over_method_call_result() {
    let backend = create_test_backend();
    let uri = "file:///test_foreach_method.php";
    let text = r#"<?php
class Product {
    public function getTitle(): string { return ''; }
}
class Catalog {
    /** @return list<Product> */
    public function getProducts(): array { return []; }

    public function display(): void {
        foreach ($this->getProducts() as $product) {
            $product->getTitle();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member for $product->getTitle() in foreach over method call, got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Foreach over a static method call should also resolve the value
/// variable type through the subject pipeline.
#[test]
fn scope_cache_foreach_over_static_method_call() {
    let backend = create_test_backend();
    let uri = "file:///test_foreach_static.php";
    let text = r#"<?php
class User {
    public function getEmail(): string { return ''; }
}
class UserRepository {
    /** @return list<User> */
    public static function findAll(): array { return []; }
}
class Report {
    public function generate(): void {
        foreach (UserRepository::findAll() as $user) {
            $user->getEmail();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member for $user->getEmail() in foreach over static call, got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Forward walker scope cache — pass-by-ref in if-conditions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scope_cache_pass_by_ref_in_if_condition_preg_match() {
    let backend = create_test_backend();
    let uri = "file:///test_preg_match_if.php";
    let text = r#"<?php
class MatchResult {
    /** @return array<string> */
    public static function fromMatches(array $matches): self { return new self(); }
    public function getGroup(): string { return ''; }
}

class Parser {
    public function parse(string $input): ?MatchResult {
        if (preg_match('/(\d+)/', $input, $matches) === 1) {
            $result = MatchResult::fromMatches($matches);
            $result->getGroup();
        }
        return null;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Pass-by-ref $matches from preg_match in if-condition should be in scope. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn scope_cache_pass_by_ref_in_if_condition_with_comparison() {
    let backend = create_test_backend();
    let uri = "file:///test_preg_match_cmp.php";
    let text = r#"<?php
class Extractor {
    public function extract(string $text): ?int {
        if (preg_match_all('/\d+/', $text, $matches) >= 1) {
            return count($matches[0]);
        }
        return null;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Pass-by-ref $matches from preg_match_all in comparison condition should be in scope. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn scope_cache_pass_by_ref_in_while_condition() {
    let backend = create_test_backend();
    let uri = "file:///test_preg_match_while.php";
    let text = r#"<?php
class TokenCollector {
    /** @var list<string> */
    private array $tokens = [];
    public function collect(string $input): void {
        $offset = 0;
        while (preg_match('/\w+/', $input, $matches, 0, $offset) === 1) {
            $this->tokens[] = $matches[0];
            $offset += strlen($matches[0]);
        }
    }
    /** @return list<string> */
    public function getTokens(): array { return $this->tokens; }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Pass-by-ref $matches from preg_match in while-condition should be in scope. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn scope_cache_pass_by_ref_parse_str_expression_statement() {
    let backend = create_test_backend();
    let uri = "file:///test_parse_str.php";
    let text = r#"<?php
class QueryParser {
    public function parse(string $queryString): int {
        parse_str($queryString, $params);
        return count($params);
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Pass-by-ref $params from parse_str should be in scope. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Forward walker scope cache — superglobal seeding
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scope_cache_superglobal_server_in_function() {
    let backend = create_test_backend();
    let uri = "file:///test_superglobal.php";
    // $_SERVER is a superglobal — accessing it should not cause unknown
    // member diagnostics on variables assigned from it.
    let text = r#"<?php
class RequestInfo {
    public static function fromServer(string $key): self { return new self(); }
    public function getValue(): string { return ''; }
}

function getHost(): string {
    $host = $_SERVER['HTTP_HOST'] ?? 'localhost';
    return is_string($host) ? $host : 'localhost';
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Superglobal $_SERVER should be seeded in scope. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Forward walker scope cache — pass-by-ref on method/static calls
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scope_cache_pass_by_ref_method_call() {
    let backend = create_test_backend();
    let uri = "file:///test_pass_by_ref_method.php";
    let text = r#"<?php
class DataStore {
    /** @param array &$output */
    public function exportTo(string $key, array &$output): void {}
}

class Processor {
    public function run(DataStore $store): int {
        $store->exportTo('items', $results);
        return count($results);
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Pass-by-ref $results from method call should be in scope. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn scope_cache_pass_by_ref_static_method_call() {
    let backend = create_test_backend();
    let uri = "file:///test_pass_by_ref_static.php";
    let text = r#"<?php
class Registry {
    /** @param array &$entries */
    public static function dump(array &$entries): void {}
}

class Reporter {
    public function report(): int {
        Registry::dump($entries);
        return count($entries);
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Pass-by-ref $entries from static method call should be in scope. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// Forward walker scope cache: subject pipeline fallback for RHS resolution.

#[test]
fn scope_cache_method_call_rhs_via_subject_fallback() {
    let backend = create_test_backend();
    let uri = "file:///test_rhs_subject_method.php";
    let text = r#"<?php
class OrderItem {
    public function getProduct(): Product { return new Product(); }
}

class Product {
    public function getName(): string { return ''; }
}

class OrderProcessor {
    public function process(OrderItem $item): string {
        $product = $item->getProduct();
        return $product->getName();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Method call RHS should resolve via subject pipeline. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn scope_cache_static_call_rhs_via_subject_fallback() {
    let backend = create_test_backend();
    let uri = "file:///test_rhs_subject_static.php";
    let text = r#"<?php
class Config {
    public static function load(): Settings { return new Settings(); }
}

class Settings {
    public function getValue(): string { return ''; }
}

class App {
    public function run(): string {
        $settings = Config::load();
        return $settings->getValue();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Static call RHS should resolve via subject pipeline. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn scope_cache_chained_method_call_rhs_via_subject_fallback() {
    let backend = create_test_backend();
    let uri = "file:///test_rhs_subject_chain.php";
    let text = r#"<?php
class Connection {
    public function query(): QueryBuilder { return new QueryBuilder(); }
}

class QueryBuilder {
    public function where(string $col, string $val): self { return $this; }
    public function first(): ?Record { return null; }
}

class Record {
    public function getId(): int { return 0; }
}

class Repository {
    public function find(Connection $db): ?int {
        $builder = $db->query()->where('status', 'active');
        $record = $builder->first();
        if ($record !== null) {
            return $record->getId();
        }
        return null;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Chained method call RHS should resolve via subject pipeline. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// When a cross-file class has `@phpstan-assert-if-false self<true> $this`
/// on a method (e.g. `Decimal::isZero()`), a guard clause like
/// `if ($var->isZero()) { return null; }` triggers inverse assert
/// narrowing.  The `self` in the assertion type must resolve against
/// the *declaring* class (`Decimal`), not the *enclosing* class
/// (`Monetary`).  Previously, `self` was passed to
/// `apply_instanceof_inclusion` unresolved and the narrowing engine
/// resolved it against `current_class` (the enclosing class), replacing
/// the variable's type with the wrong class.
#[test]
fn scope_cache_phpstan_assert_if_false_self_resolves_against_declaring_class() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Decimal.php",
            r#"<?php
namespace App;

class Decimal {
    public function sub(Decimal $other): self { return $this; }
    public function div(Decimal $other): self { return $this; }
    public function mul(Decimal $other): self { return $this; }
    public function toFloat(): float { return 0.0; }

    /** @phpstan-assert-if-false self<true> $this */
    public function isZero(): bool { return false; }
}
"#,
        )],
    );

    // The guard clause `if ($denominator->isZero()) { return null; }`
    // triggers inverse @phpstan-assert-if-false narrowing on $denominator.
    // The assertion type `self<true>` must resolve to `Decimal<true>`,
    // not `Monetary<true>`.
    let uri = "file:///test_assert_self.php";
    let text = r#"<?php
use App\Decimal;

class Monetary {
    public function calcFraction(Decimal $net, Decimal $supplierPrice): ?float {
        $denominator = $net->mul($supplierPrice);
        if ($denominator->isZero()) {
            return null;
        }
        return $denominator->sub($supplierPrice)->div($denominator)->toFloat();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "phpstan-assert-if-false with `self` type should resolve against declaring class, not enclosing class. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Same pattern but without the guard clause — plain fluent chain on a
/// cross-file parameter with `self` return type.  Ensures the basic
/// resolution works even without assert narrowing.
#[test]
fn scope_cache_self_return_type_cross_file_fluent_chain() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Decimal.php",
            r#"<?php
namespace App;

class Decimal {
    public function sub(Decimal $other): self { return $this; }
    public function div(Decimal $other): self { return $this; }
    public function toFloat(): float { return 0.0; }
}
"#,
        )],
    );

    let uri = "file:///test_self_chain.php";
    let text = r#"<?php
use App\Decimal;

class Monetary {
    public function calcFraction(Decimal $denominator, Decimal $supplierPrice): float {
        return $denominator->sub($supplierPrice)->div($denominator)->toFloat();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "self return type on cross-file parameter should resolve correctly. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Same-name class in different namespace should not shadow parent (GH-87)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_false_positive_when_same_name_class_exists_in_namespace() {
    let backend = create_test_backend_with_exception_stubs();
    let uri = "file:///test.php";
    // Adding `Test\Exception` should not affect `MyException extends \Exception`.
    // The `\Exception` FQN explicitly refers to the global Exception class,
    // so `getMessage()` (inherited from global Exception) must still resolve.
    let text = r#"<?php
namespace Test;

class Exception extends \Exception {}

class MyException extends \Exception {}

class Consumer {
    public function run(): void {
        try {
            throw new MyException("foobards");
        } catch (MyException $e) {
            echo $e->getMessage();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "getMessage() is inherited from \\Exception — no diagnostic expected, got: {:?}",
        diags
    );
}

#[test]
fn no_false_positive_when_same_name_class_exists_in_namespace_scope_cache() {
    let backend = create_test_backend_with_exception_stubs();
    let uri = "file:///test.php";
    let text = r#"<?php
namespace Test;

class Exception extends \Exception {}

class MyException extends \Exception {}

class Consumer {
    public function run(): void {
        try {
            throw new MyException("foobards");
        } catch (MyException $e) {
            echo $e->getMessage();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "getMessage() is inherited from \\Exception — no diagnostic expected (scope cache path), got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Use-map import must take priority over global-namespace stub class
// ═══════════════════════════════════════════════════════════════════════════

/// When a file has `use Some\Namespaced\Event;` and calls `Event::listen()`,
/// the `@method static` on the imported class must be found — not shadowed
/// by a global-namespace stub class with the same short name (e.g. the PECL
/// `Event` extension stub).
#[test]
fn use_import_takes_priority_over_global_stub_with_same_short_name() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Facades/Event.php",
            r#"<?php
namespace App\Facades;

/**
 * @method static void listen(string $event, callable $listener)
 * @method static void dispatch(string $event)
 */
class Event {
    public static function __callStatic(string $name, array $arguments): mixed { return null; }
}
"#,
        )],
    );

    // Register a global-namespace class named "Event" (simulating a stub
    // like the PECL event extension) that does NOT have `listen`/`dispatch`.
    let stub_uri = "file:///stub_event.php";
    let stub_text = r#"<?php
class Event {
    public function fd(): int { return 0; }
}
"#;
    backend.update_ast(stub_uri, stub_text);

    // The user file imports the Facade and calls a @method static method.
    let uri = "file:///test.php";
    let text = r#"<?php
namespace App\Services;

use App\Facades\Event;

class MyService {
    public function run(): void {
        Event::listen('foo', function () {});
        Event::dispatch('bar');
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Use-imported Facade @method static should resolve, not shadow by global stub. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// merge_branch must not let `mixed` subsume narrowed class types
// ═══════════════════════════════════════════════════════════════════════════

/// After `assert($data instanceof \stdClass)`, inserting any `if` block
/// (even `if (true) {}`) before a member access must not cause `$data`
/// to lose its narrowed type.  The branch merge used to let `mixed`
/// (from the pre-narrowed scope) subsume `stdClass`.
#[test]
fn assert_instanceof_survives_if_block_merge() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Test {
    public function handle(string $raw): void {
        $data = json_decode($raw);
        assert($data instanceof \stdClass);

        if (true) {
        }

        if (!is_string($data->status)) {
            throw new \RuntimeException('bad');
        }
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "assert instanceof should survive branch merge; $data->status should resolve. Got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Conditional return types — redirect() helper
// ═══════════════════════════════════════════════════════════════════════════

/// `redirect($to)` has `@return ($to is null ? Redirector : RedirectResponse)`.
/// When called with a non-null argument (including string concatenation),
/// the return type must resolve to `RedirectResponse`, which carries `with()`
/// and `withErrors()`.  No `unknown_member` diagnostic should fire.
#[test]
fn redirect_with_concat_arg_resolves_to_redirect_response() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"/src/"}}}"#,
        &[
            (
                "helpers.php",
                r#"<?php
namespace {
    use Illuminate\Routing\Redirector;
    use Illuminate\Http\RedirectResponse;

    /**
     * @return ($to is null ? \Illuminate\Routing\Redirector : \Illuminate\Http\RedirectResponse)
     */
    function redirect(?string $to = null): Redirector|RedirectResponse
    {
        return new RedirectResponse();
    }
}
"#,
            ),
            (
                "src/Routing/Redirector.php",
                r#"<?php
namespace Illuminate\Routing;
class Redirector {}
"#,
            ),
            (
                "src/Http/RedirectResponse.php",
                r#"<?php
namespace Illuminate\Http;
class RedirectResponse {
    public function with(string $key, mixed $value = null): static {}
    public function withErrors(mixed $provider, string $key = 'default'): static {}
}
"#,
            ),
            (
                "src/Controller.php",
                r#"<?php
namespace App;
class Customer { public int $id = 0; }
class MyController {
    public function action(Customer $customer): void {
        // String concatenation arg — must resolve to RedirectResponse.
        redirect('/users/' . $customer->id . '#tab')->with('msg', 'ok');
        redirect('/users/' . $customer->id)->withErrors(['e']);
        // Assigned form works too (baseline sanity check).
        $r = redirect('/users/' . $customer->id);
        $r->with('msg', 'ok');
    }
}
"#,
            ),
        ],
    );

    let uri = "file:///src/Controller.php";
    let content =
        std::fs::read_to_string(std::path::Path::new(_dir.path()).join("src/Controller.php"))
            .unwrap();
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, &content);
    let with_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("with") || d.message.contains("withErrors"))
        .collect();
    assert!(
        with_diags.is_empty(),
        "redirect()->with()/withErrors() should resolve to RedirectResponse. Got: {:?}",
        with_diags
    );
}

// ─── Issue #168: instanceof narrowing must not leak into elseif body ────────

#[test]
fn no_false_unknown_member_in_elseif_after_instanceof() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let php = r#"<?php
class KnownDateLike {
    public function format(): string { return 'formatted'; }
}

function formatDateLike(object $value): string {
    if ($value instanceof KnownDateLike) {
        $value = $value->format();
    } elseif (is_callable([$value, 'getTime'])) {
        $value = (string) $value->getTime();
    }

    return (string) $value;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, php);
    assert!(
        diags.is_empty(),
        "instanceof narrowing from if-branch must not leak into elseif (issue #168): {:?}",
        diags
    );
}

// ─── Reassignment in if-branch must not leak into the elseif *condition* ────

#[test]
fn no_false_unknown_member_in_elseif_condition_after_reassign() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // The then-branch reassigns `$value` to a type without `format()`.  A
    // member access in the following elseif *condition* must resolve
    // `$value` against the clean pre-branch scope (where it is still
    // `HasFormat`), not the leaked then-branch type.  This exercises the
    // scope snapshot recorded at the elseif condition boundary — the body
    // snapshots recorded by the forward walker do not cover the condition.
    let php = r#"<?php
class HasFormat {
    public function format(): string { return 'formatted'; }
}
class NoFormat {
    public function other(): string { return 'other'; }
}

function test(HasFormat $value, bool $flag): string {
    if ($flag) {
        $value = new NoFormat();
    } elseif ($value->format() === 'x') {
        return 'a';
    }

    return 'b';
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, php);
    assert!(
        diags.is_empty(),
        "reassignment in if-branch must not leak into the elseif condition: {:?}",
        diags
    );
}

#[test]
fn enum_name_and_value_properties_are_known() {
    // Every enum exposes a readonly `name` property, and backed enums also
    // expose a `value` property. Neither should be flagged as unknown.
    let backend = create_test_backend();
    let uri = "file:///enum_props.php";
    let php = r#"<?php
enum Suit: string {
    case Hearts = 'H';
    case Spades = 'S';
}

enum Direction {
    case North;
    case South;
}

function backed(Suit $s): string {
    return $s->value . $s->name;
}

function pure(Direction $d): string {
    return $d->name;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, php);
    assert!(
        diags.is_empty(),
        "enum ->name and backed enum ->value must not be flagged unknown: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Ternary condition narrows property / method-call subjects
// ═══════════════════════════════════════════════════════════════════════════

/// `$this->node instanceof Foo ? $this->node->fooMethod() : null` must
/// narrow the `$this->node` property to `Foo` inside the then-branch, so
/// that a method only declared on `Foo` is not flagged as unknown on the
/// declared property type.
#[test]
fn ternary_instanceof_narrows_property_subject() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Node {}
class Artifact implements Node {
    public function getCompilationUnit(): string { return ''; }
}
class AbstractNode {
    private Node $node;
    public function getCompilationUnit(): ?string {
        return $this->node instanceof Artifact
            ? $this->node->getCompilationUnit()
            : null;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("getCompilationUnit")),
        "No diagnostic expected for 'getCompilationUnit' inside the ternary then-branch, got: {:?}",
        diags
    );
}

/// The type of a ternary whose then-branch narrows a property must be the
/// union of both branches, not just the else-branch.  Here the assigned
/// variable should be `string|null`, so a later truthy-guarded member
/// access must not report member access on `null`.
#[test]
fn ternary_property_narrowing_does_not_collapse_to_else_branch() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Node {}
class CompilationUnit {
    public function getFileName(): string { return ''; }
}
class Artifact implements Node {
    public function getCompilationUnit(): CompilationUnit { return new CompilationUnit(); }
}
class AbstractNode {
    private Node $node;
    public function getFileName(): ?string {
        $unit = $this->node instanceof Artifact
            ? $this->node->getCompilationUnit()
            : null;
        return $unit
            ? $unit->getFileName()
            : null;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Ternary result must be CompilationUnit|null (not just null), so 'getFileName' must not be flagged, got: {:?}",
        diags
    );
}

/// A nullable method-call subject inside a truthy ternary condition is
/// narrowed to its non-null type in the then-branch, so a member declared
/// on that type is not flagged.
#[test]
fn ternary_truthy_narrows_method_call_subject() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class User {
    public int $id = 1;
}
class Request {
    public function user(): ?User { return new User(); }
}
class Handler {
    public function handle(Request $request): ?int {
        return $request->user()
            ? $request->user()->id
            : null;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Nullable method-call subject narrowed to User in ternary then-branch; 'id' must not be flagged, got: {:?}",
        diags
    );
}

/// Guard against over-narrowing: a genuinely missing member inside the
/// ternary then-branch must still be flagged after narrowing.
#[test]
fn ternary_instanceof_still_flags_missing_member() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Node {}
class Artifact implements Node {
    public function realMethod(): string { return ''; }
}
class AbstractNode {
    private Node $node;
    public function run(): ?string {
        return $this->node instanceof Artifact
            ? $this->node->missingMethod()
            : null;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.iter().any(|d| d.message.contains("missingMethod")),
        "A missing method on the narrowed type must still be flagged, got: {:?}",
        diags
    );
}
