use crate::common::{
    create_psr4_workspace, create_psr4_workspace_with_stubs, create_test_backend,
    create_test_backend_with_exception_stubs, create_test_backend_with_stubs,
};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// A minimal global SPL `Iterator` stub: it deliberately does NOT declare
/// `accept()`, so any diagnostic flagging `accept` proves the instance was
/// wrongly resolved to the global stub instead of the project class.
static ITERATOR_STUB: &str = "<?php\ninterface Iterator { public function next(): void; }\n";

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
fn no_diagnostic_when_class_has_magic_call() {
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
    assert!(
        diags.is_empty(),
        "Methods dispatched through __call are valid and must not be flagged, got: {:?}",
        diags
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
fn no_diagnostic_when_class_has_magic_call_static() {
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
    assert!(
        diags.is_empty(),
        "Static methods dispatched through __callStatic are valid and must not be flagged, got: {:?}",
        diags
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
fn no_diagnostic_when_parent_has_magic_call() {
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
    assert!(
        diags.is_empty(),
        "Method inherited through a parent's __call is valid and must not be flagged, got: {:?}",
        diags
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

/// An `assertInstanceOf`-style `@phpstan-assert` on an array-index subject
/// (`assertInstanceOf(X::class, $arr['k'])`) must narrow that index so a
/// following member access on it resolves.  Without keying narrowing by the
/// printed subject expression, `$constants['C']->getImage()` was reported as
/// an unresolved member access.
#[test]
fn assert_instanceof_narrows_array_index_subject() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class ASTNode {
    public function getImage(): string { return ''; }
}

class BaseAssert
{
    /**
     * @template ExpectedType of object
     * @param class-string<ExpectedType> $expected
     * @phpstan-assert =ExpectedType $actual
     */
    public static function assertInstanceOf(string $expected, object $actual): void {}
}

class MyTest extends BaseAssert
{
    /**
     * @param array<string, mixed> $constants
     */
    public function testIt(array $constants): void
    {
        static::assertInstanceOf(ASTNode::class, $constants['C']);
        $constants['C']->getImage();
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut diags);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("could not be resolved")),
        "assertInstanceOf should narrow $constants['C'] to ASTNode, got: {diags:?}",
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("getImage")),
        "getImage is a valid method on ASTNode, got: {diags:?}",
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
fn scope_on_standalone_bare_builder_param_not_flagged_chain_continues() {
    // When a function parameter is typed as bare `Builder` (no callable
    // inference context), scope methods cannot be verified statically.
    // Because `Builder` defines `__call`, the call is dispatched through
    // it at runtime and must not be flagged (matching PHPStan). The chain
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

    // Scope method is NOT flagged — Builder defines __call, so the call
    // is valid and dynamically dispatched even without knowing the model.
    assert!(
        !diags.iter().any(|d| d.message.contains("whereIsLuxury")),
        "Scope method dispatched through Builder's __call must not be flagged, got: {:?}",
        diags
    );

    // Chain continues — known methods after the scope call
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

/// A variable assigned by list-destructuring from an unresolvable RHS
/// (e.g. a bare `array` parameter) must still be narrowable by a later
/// `assertInstanceOf`.  Previously the destructured variables were never
/// recorded in scope when the RHS type could not be resolved, so the
/// assert narrowing loop skipped them and `$type->getImage()` produced a
/// bogus `type of '$type' could not be resolved` diagnostic.
#[test]
fn assert_narrows_variable_destructured_from_unresolvable_rhs() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Wanted {
    public function getImage(): string { return ''; }
}
class BaseAssert {
    /**
     * @template ExpectedType of object
     * @param class-string<ExpectedType> $expected
     * @phpstan-assert ExpectedType $actual
     */
    public static function assertInstanceOf(string $expected, object $actual): void {}
}
class ParserTest extends BaseAssert {
    public function testDestructure(array $declarations): void {
        [$type, $variable] = $declarations[0];
        static::assertInstanceOf(Wanted::class, $type);
        $type->getImage();
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut diags);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("could not be resolved") && d.message.contains("getImage")),
        "assertInstanceOf must narrow a list-destructured variable from an \
         unresolvable RHS, got: {diags:?}",
    );
}

/// A variable holding a `::class` literal (e.g. `$cls = Wanted::class;`)
/// used as the class-string argument to `assertInstanceOf` must narrow
/// the subject the same way the inlined `Wanted::class` literal does.
#[test]
fn assert_narrows_via_variable_class_string_argument() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Wanted {
    public function getImage(): string { return ''; }
}
class BaseAssert {
    /**
     * @template ExpectedType of object
     * @param class-string<ExpectedType> $expected
     * @phpstan-assert ExpectedType $actual
     */
    public static function assertInstanceOf(string $expected, object $actual): void {}
}
class ParserTest extends BaseAssert {
    public function testDestructure(array $declarations): void {
        $expectedTypeClass = Wanted::class;
        [$type, $variable] = $declarations[0];
        static::assertInstanceOf($expectedTypeClass, $type);
        $type->getImage();
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut diags);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("could not be resolved") && d.message.contains("getImage")),
        "assertInstanceOf must narrow via a variable holding a ::class literal, got: {diags:?}",
    );
}

/// A variable holding a `::class` literal that is assigned *inside a braced
/// block* (e.g. a `foreach (...) { $cls = Wanted::class; ... }` body) must
/// still resolve for `assertInstanceOf` narrowing.  This is the PHPUnit
/// data-provider loop shape: assign the expected class in the loop body, then
/// assert the subject against it and call a method on the narrowed subject.
#[test]
fn assert_narrows_via_variable_class_string_assigned_in_foreach_body() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Wanted {
    public function getImage(): string { return ''; }
}
class BaseAssert {
    /**
     * @template ExpectedType of object
     * @param class-string<ExpectedType> $expected
     * @phpstan-assert =ExpectedType $actual
     */
    public static function assertInstanceOf(string $expected, mixed $actual): void {}
}
class ParserTest extends BaseAssert {
    public function testTypedProperties(array $declarations, array $items): void {
        foreach ($items as $index => $expected) {
            $expectedTypeClass = $expected[2] ?? Wanted::class;
            [$type, $variable] = $declarations[$index];
            static::assertInstanceOf(
                $expectedTypeClass,
                $type,
                "message"
            );
            $type->getImage();
        }
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut diags);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("could not be resolved") && d.message.contains("getImage")),
        "assertInstanceOf must narrow via a variable assigned a ::class literal inside \
         a foreach body, got: {diags:?}",
    );
}

/// The expected-class argument to `assertInstanceOf` may be list-destructured
/// out of a foreach source array (the PHPUnit data-provider loop shape):
/// `[$a, $b, $expectedTypeClass] = $expected;` where `$expected` iterates
/// `$items = [[..., ..., Wanted::class]]`.  The destructured variable holds a
/// `class-string<Wanted>` value, so the assert must narrow the subject to
/// `Wanted` and calling a method on it must not emit an unresolved-member
/// diagnostic.
#[test]
fn assert_narrows_via_variable_class_string_list_destructured_from_foreach() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Wanted {
    public function getImage(): string { return ''; }
}
class BaseAssert {
    /**
     * @template ExpectedType of object
     * @param class-string<ExpectedType> $expected
     * @phpstan-assert =ExpectedType $actual
     */
    public static function assertInstanceOf(string $expected, mixed $actual): void {}
}
class ParserTest extends BaseAssert {
    public function testTypedProperties(array $declarations): void {
        $items = [
            ['null|int|float', '$number', Wanted::class],
        ];
        foreach ($items as $index => $expected) {
            [$expectedType, $expectedVariable, $expectedTypeClass] = $expected;
            [$type, $variable] = $declarations[$index];
            static::assertInstanceOf(
                $expectedTypeClass,
                $type,
                "message"
            );
            $type->getImage();
        }
    }
}
"#;
    backend.update_ast(uri, text);
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut diags);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("could not be resolved") && d.message.contains("getImage")),
        "assertInstanceOf must narrow via a variable list-destructured from a foreach \
         source array, got: {diags:?}",
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

// ═══════════════════════════════════════════════════════════════════════════
// Same-namespace class wins over a global stub of the same short name
// ═══════════════════════════════════════════════════════════════════════════

/// `new Iterator()` inside `namespace App\Input` must resolve to the
/// project's `App\Input\Iterator`, not the global SPL `\Iterator` stub, so
/// members declared on the project class are recognised.  PHP resolves an
/// unqualified class reference against the current namespace before falling
/// back to the global scope.
#[test]
fn new_same_namespace_class_wins_over_global_stub() {
    let composer_json = r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#;
    let project_iterator = "<?php\nnamespace App\\Input;\nclass Iterator {\n    public function accept(): bool { return true; }\n}\n";

    let (backend, _dir) = create_psr4_workspace_with_stubs(
        composer_json,
        &[("src/Input/Iterator.php", project_iterator)],
        &[("Iterator", ITERATOR_STUB)],
    );

    let uri = "file:///consumer.php";
    let text = "<?php\nnamespace App\\Input;\nclass Consumer {\n    public function run(): void {\n        $it = new Iterator();\n        $it->accept();\n    }\n}\n";

    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("accept")),
        "`new Iterator()` must resolve to the same-namespace App\\Input\\Iterator, \
         so `accept()` is a known method, got: {diags:?}"
    );
}

/// The guard against regressing global resolution: when a namespace has no
/// class of the given short name, `new Iterator()` must still resolve to the
/// global SPL stub, so a member the stub does not declare is flagged.
#[test]
fn new_falls_back_to_global_stub_when_no_same_namespace_class() {
    let composer_json = r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#;

    // No project `App\Other\Iterator` exists, so the global stub is correct.
    let (backend, _dir) =
        create_psr4_workspace_with_stubs(composer_json, &[], &[("Iterator", ITERATOR_STUB)]);

    let uri = "file:///consumer.php";
    let text = "<?php\nnamespace App\\Other;\nclass Consumer {\n    public function run(): void {\n        $it = new Iterator();\n        $it->accept();\n    }\n}\n";

    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.iter().any(|d| d.message.contains("accept")),
        "`new Iterator()` with no same-namespace class must resolve to the \
         global stub, which has no `accept()`, got: {diags:?}"
    );
}

// ─── Array callables are data, not member accesses ──────────────────────────

/// A `[Class::class, 'method']` pair nested in a returned array is plain
/// data (a `list<list<string>>`), not a callable, so its second element
/// must not be validated as a static method.
#[test]
fn array_of_class_string_pairs_is_not_validated_as_callables() {
    let backend = create_test_backend();
    let uri = "file:///pairs.php";
    let text = concat!(
        "<?php\n",
        "class Chart {}\n",
        "class Report {}\n",
        "class Registry {\n",
        "    public function names(): array {\n",
        "        return [\n",
        "            [Chart::class, 'svg'],\n",
        "            [Report::class, 'xml'],\n",
        "        ];\n",
        "    }\n",
        "}\n",
    );

    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "data pairs like [Chart::class, 'svg'] must not be validated as method calls, got: {diags:?}"
    );
}

/// A `[$var, 'method']` pair passed as the data argument of `array_filter`
/// (whose first parameter is the array, not the callback) must not be
/// validated as an instance method call on the variable.
#[test]
fn array_data_argument_to_builtin_is_not_validated_as_callable() {
    let backend = create_test_backend();
    let uri = "file:///data_arg.php";
    let text = concat!(
        "<?php\n",
        "function build(string $prefix): array {\n",
        "    return array_filter([$prefix, 'match']);\n",
        "}\n",
    );

    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "a data array passed to array_filter must not be validated as a method call, got: {diags:?}"
    );
}

// ─── Mockery verification chains resolve through the real return type ──────

/// `Mockery\LegacyMockInterface::shouldHaveReceived()` is declared
/// `@return self`, but the concrete mock always builds a
/// `Mockery\VerificationDirector`. Honouring the declared `self` sends
/// the chained `->with()` call back to the mock interface (which has no
/// `with()`), producing a false-positive unknown-member diagnostic.
#[test]
fn mockery_should_have_received_chain_resolves_to_verification_director() {
    let backend = create_test_backend();
    let uri = "file:///mockery_verification.php";
    let text = r#"<?php
namespace Mockery {
    interface LegacyMockInterface {
        /** @return self */
        public function shouldHaveReceived($method, $args = null);
    }
    interface MockInterface extends LegacyMockInterface {}
    class VerificationDirector {
        public function with(...$args): self { return $this; }
        public function once(): self { return $this; }
    }
}
namespace App {
    class ProductCacheService {}

    class TestBase {
        /**
         * @param string $abstract
         * @return \Mockery\MockInterface
         */
        protected function mock($abstract) {}
    }

    class ExampleTest extends TestBase {
        public function test(): void {
            $service = $this->mock(ProductCacheService::class);
            $service->shouldHaveReceived('store')->with([10, 20], [])->once();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("with")),
        "shouldHaveReceived()->with(...) must resolve through VerificationDirector, got: {diags:?}"
    );
}

#[test]
fn this_in_anonymous_class_resolves_to_anon_not_outer() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Test
{
    public function make(): object
    {
        return new class (5) {
            public function __construct(private readonly int $value) {}

            public function get(): int
            {
                return $this->value;
            }
        };
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("value")),
        "$this->value inside the anonymous class must resolve to the anon class, got: {diags:?}"
    );
}

#[test]
fn this_method_call_in_anonymous_class_resolves_to_anon() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // `$this->helper()` inside the anonymous class must resolve against
    // the anonymous class, and a member missing on the anonymous class
    // (but present on the outer class) must still be flagged.
    let text = r#"<?php
class Outer
{
    public function outerOnly(): void {}

    public function make(): object
    {
        return new class {
            public function helper(): int { return 1; }

            public function run(): void
            {
                $this->helper();
                $this->outerOnly();
            }
        };
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("helper")),
        "$this->helper() must resolve on the anonymous class, got: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("outerOnly")),
        "$this->outerOnly() (defined only on Outer) must be flagged inside the anon class, got: {diags:?}"
    );
}

#[test]
fn this_after_anonymous_class_still_resolves_to_outer() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // After the anonymous class body, `$this` must return to the outer
    // class so its own members still resolve.
    let text = r#"<?php
class Outer
{
    public function outerProp(): void {}

    public function make(): void
    {
        $x = new class {
            public function inner(): void {}
        };
        $this->outerProp();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("outerProp")),
        "$this->outerProp() after the anon class must resolve on Outer, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// property_exists() / method_exists() narrowing
// ═══════════════════════════════════════════════════════════════════════════

/// Inside `if (property_exists($x, 'Name'))`, accessing `$x->Name` must
/// not be flagged — the guard proves the (dynamically populated) property
/// exists.  PHPStan models this as `object&hasProperty(Name)`.
#[test]
fn property_exists_guard_allows_property_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Response {
    public int $code = 0;
}
function check(Response $response): void {
    if (property_exists($response, 'MerchantErrorMessage')) {
        if ($response->MerchantErrorMessage && is_string($response->MerchantErrorMessage)) {
            throw new \RuntimeException($response->MerchantErrorMessage);
        }
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("MerchantErrorMessage")),
        "property_exists guard must allow the guarded property, got: {diags:?}"
    );
}

/// A `&&` chain of two property_exists guards proves both properties
/// (the api-php AltaPay pattern).
#[test]
fn property_exists_and_chain_allows_both_properties() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Response {
    public int $code = 0;
}
function check(Response $response): void {
    if (property_exists($response, 'CardHolderErrorMessage') && property_exists($response, 'CardHolderMessageMustBeShown')) {
        if ($response->CardHolderMessageMustBeShown && is_string($response->CardHolderErrorMessage)) {
            throw new \RuntimeException($response->CardHolderErrorMessage);
        }
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("CardHolder")),
        "chained property_exists guards must allow both properties, got: {diags:?}"
    );
}

/// Without the guard the unknown property is still flagged — the
/// narrowing must not leak outside the guarded branch.
#[test]
fn property_access_outside_property_exists_guard_still_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Response {
    public int $code = 0;
}
function check(Response $response): void {
    if (property_exists($response, 'MerchantErrorMessage')) {
        echo 'ok';
    }
    echo $response->MerchantErrorMessage;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("MerchantErrorMessage")),
        "access after the guarded branch must still be flagged, got: {diags:?}"
    );
}

/// A property other than the guarded one is still flagged inside the
/// branch.
#[test]
fn property_exists_guard_only_proves_the_guarded_name() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Response {
    public int $code = 0;
}
function check(Response $response): void {
    if (property_exists($response, 'MerchantErrorMessage')) {
        echo $response->SomethingElse;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.iter().any(|d| d.message.contains("SomethingElse")),
        "unguarded property inside the branch must still be flagged, got: {diags:?}"
    );
}

/// The else branch of a positive guard proves nothing — the member is
/// absent there, so access is still flagged.
#[test]
fn property_exists_else_branch_still_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Response {
    public int $code = 0;
}
function check(Response $response): void {
    if (property_exists($response, 'MerchantErrorMessage')) {
        echo 'ok';
    } else {
        echo $response->MerchantErrorMessage;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("MerchantErrorMessage")),
        "else branch of property_exists must still flag the property, got: {diags:?}"
    );
}

/// After a guard clause `if (!property_exists($x, 'p')) { return; }`,
/// the property is known to exist.
#[test]
fn negated_property_exists_guard_clause_allows_access_after() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Response {
    public int $code = 0;
}
function check(Response $response): void {
    if (!property_exists($response, 'MerchantErrorMessage')) {
        return;
    }
    echo $response->MerchantErrorMessage;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("MerchantErrorMessage")),
        "negated property_exists guard clause must allow access after it, got: {diags:?}"
    );
}

/// `method_exists($x, 'name')` proves the method inside the branch.
#[test]
fn method_exists_guard_allows_method_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Handler {
    public function run(): void {}
}
function check(Handler $handler): void {
    if (method_exists($handler, 'customHook')) {
        $handler->customHook();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("customHook")),
        "method_exists guard must allow the guarded method, got: {diags:?}"
    );
}

/// A dynamic (non-literal) member name proves the existence of *some*
/// member but not which one — nothing is added, and other accesses stay
/// flagged.
#[test]
fn property_exists_with_dynamic_name_adds_nothing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Response {
    public int $code = 0;
}
function check(Response $response, string $name): void {
    if (property_exists($response, $name)) {
        echo $response->MerchantErrorMessage;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("MerchantErrorMessage")),
        "dynamic property_exists must not prove arbitrary properties, got: {diags:?}"
    );
}

/// PHPUnit's `assertTrue()` carries `@phpstan-assert true $condition`, so
/// `assertTrue(property_exists($x, 'p'))` re-exports the inner condition
/// exactly like `if (property_exists($x, 'p'))`: the guarded property must
/// not be flagged afterwards.
#[test]
fn assert_true_property_exists_reexports_the_guard() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class TestCase {
    /** @phpstan-assert true $condition */
    public static function assertTrue(mixed $condition): void {}
}
class Response {
    public int $code = 0;
}
class ResponseTest extends TestCase {
    public function check(Response $response): void {
        self::assertTrue(property_exists($response, 'MerchantErrorMessage'));
        echo $response->MerchantErrorMessage;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("MerchantErrorMessage")),
        "assertTrue(property_exists(...)) must prove the property, got: {diags:?}"
    );
}

/// The `@psalm-assert` spelling of the re-export tag is handled the same
/// as `@phpstan-assert`.
#[test]
fn assert_true_property_exists_reexports_the_guard_psalm_notation() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class TestCase {
    /** @psalm-assert true $condition */
    public static function assertTrue(mixed $condition): void {}
}
class Response {
    public int $code = 0;
}
class ResponseTest extends TestCase {
    public function check(Response $response): void {
        self::assertTrue(property_exists($response, 'MerchantErrorMessage'));
        echo $response->MerchantErrorMessage;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("MerchantErrorMessage")),
        "@psalm-assert true must re-export like @phpstan-assert, got: {diags:?}"
    );
}

/// The re-exported proof is scoped to the assertion: without it, the
/// unknown property is still flagged.  This guards against the narrowing
/// leaking to every property on the class.
#[test]
fn assert_true_property_exists_only_proves_the_guarded_name() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class TestCase {
    /** @phpstan-assert true $condition */
    public static function assertTrue(mixed $condition): void {}
}
class Response {
    public int $code = 0;
}
class ResponseTest extends TestCase {
    public function check(Response $response): void {
        self::assertTrue(property_exists($response, 'MerchantErrorMessage'));
        echo $response->CardHolderErrorMessage;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("CardHolderErrorMessage")),
        "only the guarded property is proven, got: {diags:?}"
    );
}

/// The full backoffice pattern: `viewData()` returns `mixed`,
/// `assertIsObject()` (a `@psalm-assert object` guard) narrows it to
/// `object`, and `assertTrue(property_exists(...))` re-exports the
/// property proof.  Member access on the result must not be flagged as an
/// unresolved type.
#[test]
fn assert_is_object_then_assert_true_property_exists() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class TestCase {
    /** @phpstan-assert object $actual */
    public static function assertIsObject(mixed $actual): void {}
    /** @phpstan-assert true $condition */
    public static function assertTrue(mixed $condition): void {}
}
class ControllerTest extends TestCase {
    public function testEdit(): void {
        $model = $this->viewData();
        self::assertIsObject($model);
        self::assertTrue(property_exists($model, 'value'));
        echo $model->value;
    }
    public function viewData(): mixed { return null; }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("$model")),
        "assertIsObject + assertTrue(property_exists) must resolve the subject, got: {diags:?}"
    );
}

/// `isset($obj->prop)` in an `if` proves the property exists inside the
/// branch, exactly like `property_exists($obj, 'prop')`.
#[test]
fn isset_property_guard_allows_property_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class CanApply {
    public int $id = 0;
}
function probeIsset(CanApply $item): int {
    if (isset($item->salesCampaignGroupId)) {
        return $item->salesCampaignGroupId;
    }
    return 0;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("salesCampaignGroupId")),
        "isset($item->prop) guard must allow the guarded property, got: {diags:?}"
    );
}

/// The property guarded by `isset` must still be flagged outside the
/// branch — the proof does not leak past the `if`.
#[test]
fn property_access_outside_isset_guard_still_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class CanApply {
    public int $id = 0;
}
function probeIsset(CanApply $item): int {
    if (isset($item->salesCampaignGroupId)) {
        echo 'ok';
    }
    return $item->salesCampaignGroupId;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("salesCampaignGroupId")),
        "access after the isset branch must still be flagged, got: {diags:?}"
    );
}

/// `isset($obj->prop)` in a ternary condition proves the property inside
/// the then-branch.
#[test]
fn isset_property_ternary_allows_property_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class CanApply {
    public int $id = 0;
}
function probeTernary(CanApply $item): mixed {
    return isset($item->qty) ? $item->qty : 1;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("qty")),
        "isset($item->qty) ternary must allow the guarded property, got: {diags:?}"
    );
}

/// `property_exists($obj, 'prop')` in a ternary condition proves the
/// property inside the then-branch, exactly like the `if` form.
#[test]
fn property_exists_ternary_allows_property_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class CanApply {
    public int $id = 0;
}
function probeTernary(CanApply $item): mixed {
    return property_exists($item, 'qty') ? $item->qty : 1;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("qty")),
        "property_exists ternary must allow the guarded property, got: {diags:?}"
    );
}

/// The else-branch of a `property_exists` ternary proves nothing — the
/// guarded property is still flagged there.
#[test]
fn property_exists_ternary_else_branch_still_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class CanApply {
    public int $id = 0;
}
function probeTernary(CanApply $item): mixed {
    return property_exists($item, 'qty') ? 1 : $item->qty;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.iter().any(|d| d.message.contains("qty")),
        "else-branch of a property_exists ternary must still flag the property, got: {diags:?}"
    );
}

/// `assertFalse()` carries `@phpstan-assert false $condition`, so
/// `assertFalse(is_string($x))` re-exports the inverse of the guard —
/// after it, a `string|Foo` union is narrowed to `Foo`.
#[test]
fn assert_false_reexports_the_inverse_guard() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class TestCase {
    /** @phpstan-assert false $condition */
    public static function assertFalse(mixed $condition): void {}
}
class Widget {
    public function render(): string { return ''; }
}
class WidgetTest extends TestCase {
    public function check(string|Widget $value): void {
        self::assertFalse(is_string($value));
        echo $value->render();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("render")),
        "assertFalse(is_string($value)) must narrow away the string branch, got: {diags:?}"
    );
}

/// PHPUnit's `assertIs*` / `assertIsNot*` family (`@phpstan-assert <scalar>`)
/// narrows a `string|Widget` union like the matching `is_*()` guard: a
/// positive object/array assertion keeps or drops the class member, and the
/// `assertIsNot*` negations drop or keep it symmetrically.  The scalar
/// pseudo-types name no class, so they must route through the type-guard
/// machinery rather than being treated as class narrowings.
#[test]
fn phpunit_scalar_type_asserts_narrow_a_union() {
    let stubs = r#"
class TestCase {
    /** @phpstan-assert object $actual */
    public static function assertIsObject(mixed $actual): void {}
    /** @phpstan-assert !object $actual */
    public static function assertIsNotObject(mixed $actual): void {}
    /** @phpstan-assert string $actual */
    public static function assertIsString(mixed $actual): void {}
    /** @phpstan-assert !string $actual */
    public static function assertIsNotString(mixed $actual): void {}
    /** @phpstan-assert array<mixed> $actual */
    public static function assertIsArray(mixed $actual): void {}
}
class Widget { public function render(): string { return ''; } }
"#;
    // (assertion body, whether `render()` should be flagged as invalid).
    // When Widget is dropped, `$x` narrows to `string`, so `render()` is a
    // scalar-member-access error rather than an unknown member — hence the
    // test inspects all slow diagnostics, not only `unknown_member`.
    let cases: [(&str, bool); 5] = [
        // Keeps Widget → render valid.
        ("self::assertIsObject($x);", false),
        ("self::assertIsNotString($x);", false),
        // Drops Widget → render on the surviving `string`, flagged.
        ("self::assertIsNotObject($x);", true),
        ("self::assertIsString($x);", true),
        ("self::assertIsArray($x);", true),
    ];
    for (body, should_flag) in cases {
        let backend = create_test_backend();
        {
            let mut cfg = backend.config();
            cfg.diagnostics.unresolved_member_access = Some(true);
            backend.set_config(cfg);
        }
        let uri = "file:///test.php";
        let text = format!(
            "<?php\n{stubs}\nclass T extends TestCase {{ public function check(string|Widget $x): void {{ {body} $x->render(); }} }}\n"
        );
        backend.update_ast(uri, &text);
        let mut diags = Vec::new();
        backend.collect_slow_diagnostics(uri, &text, &mut diags);
        let flagged = diags.iter().any(|d| d.message.contains("render"));
        assert_eq!(
            flagged, should_flag,
            "case `{body}` expected render flagged={should_flag}, got {flagged}: {diags:?}"
        );
    }
}

/// Indexing a call result inline (`call(...)[0]->member`) must resolve
/// the element type of the return so member access on it is checked.
///
/// A method declared `@return T[]` with a `class-string<T>` argument
/// binds `T` from the call-site argument, so
/// `$a->findChildrenOfType(Attr::class)[0]` is an `Attr`: a real method
/// on `Attr` must not be flagged, but an unknown one must be.
#[test]
fn inline_indexed_template_call_resolves_element_member() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Node {
    public function getParent(): ?Node { return null; }
}
class Attr extends Node {
    public function attrName(): string { return ''; }
}
class Holder {
    /**
     * @template T of Node
     * @param class-string<T> $type
     * @return T[]
     */
    public function findChildrenOfType(string $type): array { return []; }
}
function run(Holder $a): void {
    $a->findChildrenOfType(Attr::class)[0]->attrName();
    $a->findChildrenOfType(Attr::class)[0]->bogusMethod();
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("attrName")),
        "real method on the inferred element type must not be flagged, got: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("bogusMethod")),
        "unknown method on the inferred element type must be flagged, got: {diags:?}"
    );
}

/// `EnumName::cases()[0]->member` must resolve to the enum instance so
/// member access is checked, because `cases()` returns a list of the
/// enum's own instances even though the stub declares it `: array`.
#[test]
fn inline_indexed_enum_cases_resolves_element_member() {
    let backend = create_test_backend_with_stubs();
    let uri = "file:///test.php";
    let text = r#"<?php
enum Priority: int
{
    case Low = 1;
    case High = 3;
}
function run(): void {
    echo Priority::cases()[0]->value;
    echo Priority::cases()[0]->bogusProp;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("'value'")),
        "backed-enum 'value' property must not be flagged, got: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("bogusProp")),
        "unknown property on the enum instance must be flagged, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// $this / self / static suppression inside traits
// ═══════════════════════════════════════════════════════════════════════════

/// `$this->` inside a trait method must resolve against the host class
/// that uses the trait, not the trait itself.
#[test]
fn no_diagnostic_for_this_member_access_inside_trait() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait LogsErrors {
    public function logError(): void {
        $this->model;
        $this->eventType;
    }
}

class ImportJob {
    use LogsErrors;
    public string $model = 'Product';
    public string $eventType = 'import';
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for $this-> inside trait, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_this_method_call_inside_trait() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait Cacheable {
    public function cache(): void {
        $this->getCacheKey();
    }
}

class Product {
    use Cacheable;
    public function getCacheKey(): string { return ''; }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for $this->method() inside trait, got: {:?}",
        diags
    );
}

/// `self::`/`static::` inside a trait can reference members declared on
/// the host class.
#[test]
fn no_diagnostic_for_self_static_inside_trait() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait HasDefaults {
    public static function create(): void {
        self::DEFAULT_NAME;
        static::factory();
    }
}

class User {
    use HasDefaults;
    const DEFAULT_NAME = 'admin';
    public static function factory(): void {}
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for self::/static:: inside trait, got: {:?}",
        diags
    );
}

/// Only `$this`/`self`/`static`/`parent` are suppressed inside traits; a
/// typed variable must still be diagnosed normally.
#[test]
fn variable_inside_trait_still_diagnosed() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {}
}

trait MyTrait {
    public function doStuff(Foo $x): void {
        $x->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("Foo")),
        "expected diagnostic for unknown method on typed variable inside trait, got: {:?}",
        diags
    );
}

/// `$this->` and `static::` inside a closure nested within a trait
/// method should be suppressed just like direct trait method bodies.
#[test]
fn no_diagnostic_for_this_inside_closure_in_trait() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait SalesInfoGlobalTrait {
    public function getSalesInfo(): void {
        $items = array_map(function ($item) {
            $this->model;
            $this->eventType;
            static::where();
            static::query();
        }, []);
    }
}

class SalesReport {
    use SalesInfoGlobalTrait;
    public string $model = 'Sale';
    public string $eventType = 'report';
    public static function where(): void {}
    public static function query(): void {}
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for $this/static:: inside closure in trait, got: {:?}",
        diags
    );
}

/// `$this->` inside an arrow function nested within a trait method.
#[test]
fn no_diagnostic_for_this_inside_arrow_fn_in_trait() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait FilterTrait {
    public function applyFilter(): void {
        $fn = fn() => $this->filterColumn;
    }
}

class Report {
    use FilterTrait;
    public string $filterColumn = 'status';
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for $this-> inside arrow fn in trait, got: {:?}",
        diags
    );
}

/// `static::where(...)->update(...)` inside a trait method: the subject
/// text for `update` is a chain rooted at `static`, so the suppression
/// must recognise the root keyword rather than require an exact match.
#[test]
fn no_diagnostic_for_chain_rooted_at_static_inside_trait() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait SalesInfoGlobalTrait {
    public function updateSalesInfo(): void {
        static::where('column', 'value')->update(['sales' => 1]);
    }
}

class SalesReport extends \Illuminate\Database\Eloquent\Model {
    use SalesInfoGlobalTrait;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for static::...->method() chain inside trait, got: {:?}",
        diags
    );
}

/// `$this->relation()->first()` inside a trait method: the subject text
/// for `first` is a chain rooted at `$this`.
#[test]
fn no_diagnostic_for_chain_rooted_at_this_inside_trait() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait HasRelation {
    public function loadRelation(): void {
        $this->items()->first();
    }
}

class Order {
    use HasRelation;
    /** @return \Illuminate\Database\Eloquent\Builder */
    public function items(): object { return new \stdClass(); }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for $this->...->method() chain inside trait, got: {:?}",
        diags
    );
}

/// `static::where(...)` inside a closure within a trait method.
#[test]
fn no_diagnostic_for_chain_rooted_at_static_inside_closure_in_trait() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait SalesInfoGlobalTrait {
    public function updateSalesInfo(): void {
        $items = array_map(function ($item) {
            static::where('col', 'val')->update(['x' => 1]);
        }, []);
    }
}

class SalesReport extends \Illuminate\Database\Eloquent\Model {
    use SalesInfoGlobalTrait;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for static:: chain inside closure in trait, got: {:?}",
        diags
    );
}

/// `self::create(...)` chain inside a trait.
#[test]
fn no_diagnostic_for_self_chain_inside_trait() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait Creatable {
    public function duplicate(): void {
        self::create(['name' => 'copy'])->save();
    }
}

class Product extends \Illuminate\Database\Eloquent\Model {
    use Creatable;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for self::...->method() chain inside trait, got: {:?}",
        diags
    );
}

/// Non-self-referencing subjects inside a trait must still be diagnosed;
/// the `$this`/`self`/`static`/`parent` suppression must not swallow them.
#[test]
fn variable_chain_inside_trait_still_diagnosed() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Widget {
    public function knownMethod(): void {}
}
trait BadTrait {
    public function doStuff(Widget $w): void {
        $w->nonExistentMethod();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonExistentMethod") && d.message.contains("not found")),
        "expected diagnostic for non-self-referencing subject inside trait, got: {:?}",
        diags
    );
}

/// A trait's own member accessed via a variable holding an anonymous
/// class instance that uses the trait.
#[test]
fn no_diagnostic_for_trait_method_on_anonymous_class_variable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
trait Greetable {
    public function greet(): string { return "hello"; }
}

function test(): void {
    $obj = new class {
        use Greetable;
    };
    $obj->greet();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for trait member on anonymous class variable, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// $this / self / parent edge cases
// ═══════════════════════════════════════════════════════════════════════════

/// `$this->` in one class must not be confused with `$this` in an
/// unrelated class earlier in the same file.
#[test]
fn no_diagnostic_for_this_in_second_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class First {
    public function a(): void {}
}
class Second {
    public function b(): void {
        $this->b();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for $this->b() in Second, got: {:?}",
        diags
    );
}

#[test]
fn flags_unknown_method_on_this_in_second_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class First {
    public function a(): void {}
}
class Second {
    public function b(): void {
        $this->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonexistent") && d.message.contains("not found")),
        "expected diagnostic for $this->nonexistent() scoped to Second, got: {:?}",
        diags
    );
}

/// `parent::` inside an anonymous class body resolves against the
/// anonymous class's own `extends` target.
#[test]
fn no_diagnostic_for_parent_in_anonymous_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Base {
    public function baseMethod(): void {}
}
class Outer {
    public function make(): void {
        $anon = new class extends Base {
            public function test(): void {
                parent::baseMethod();
            }
        };
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for parent::baseMethod() inside anonymous class, got: {:?}",
        diags
    );
}

/// A variable holding an anonymous class instance (assigned outside the
/// anonymous class body) must resolve members via the anonymous class's
/// `ClassInfo`, inheriting from its parent.
#[test]
fn no_diagnostic_for_method_on_anonymous_class_variable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Base {
    public function hello(): string { return "hi"; }
}

function test(): void {
    $model = new class extends Base {};
    $model->hello();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for method on anonymous class variable, got: {:?}",
        diags
    );
}

#[test]
fn flags_unknown_method_on_anonymous_class_variable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function test(): void {
    $obj = new class {
        public function known(): void {}
    };
    $obj->unknown();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("unknown") && d.message.contains("not found")),
        "expected unknown member diagnostic on anonymous class variable, got: {:?}",
        diags
    );
}

/// A `self::CONST` reference inside a class-level attribute sits before
/// the `class` keyword and the body braces, so the enclosing class must
/// be found via its declaration span (which includes the leading
/// attribute) rather than the body span.
#[test]
fn no_diagnostic_for_self_const_in_class_level_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
#[Route(name: self::ROUTE)]
class HealthCheckController
{
    public const string ROUTE = 'health-check';
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("ROUTE") || d.message.contains("could not be resolved")),
        "expected no self::ROUTE diagnostic, got: {:?}",
        diags
    );
}

/// A standalone multi-variable `@var` block inside a closure body
/// (without a following assignment) should declare types for untyped
/// closure parameters.
#[test]
fn no_diagnostic_for_standalone_var_docblock_in_closure() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class App {
    public function make(string $class): mixed { return new $class; }
}

class Foo {
    public function test(): void {
        $fn = function ($app, $params) {
            /**
             * @var App                      $app
             * @var array{indexName: string} $params
             */
            $app->make('Something');
        };
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics when @var declares closure param type, got: {:?}",
        diags
    );
}

/// The flip side: when `@var` resolves the type, unknown members should
/// still be flagged (proves the type was actually resolved).
#[test]
fn flags_unknown_member_with_standalone_var_docblock_in_closure() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class App {
    public function make(string $class): mixed { return new $class; }
}

class Foo {
    public function test(): void {
        $fn = function ($app) {
            /** @var App $app */
            $app->nonExistentMethod();
        };
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonExistentMethod")),
        "expected unknown member diagnostic for nonExistentMethod, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Object shapes (@return object{...})
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_object_shape_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Factory {
    /**
     * @return object{name: string, age: int}
     */
    public function create(): object {
        return (object)['name' => 'test', 'age' => 1];
    }
}

class Consumer {
    public function test(): void {
        $factory = new Factory();
        $obj = $factory->create();
        echo $obj->name;
        echo $obj->age;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for object shape property, got: {:?}",
        diags
    );
}

#[test]
fn flags_unknown_property_on_object_shape() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Factory {
    /**
     * @return object{name: string, age: int}
     */
    public function create(): object {
        return (object)['name' => 'test', 'age' => 1];
    }
}

class Consumer {
    public function test(): void {
        $obj = (new Factory())->create();
        echo $obj->missing;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.iter().any(|d| d.message.contains("missing")),
        "expected diagnostic for missing property on object shape, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Union type member resolution
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_member_on_any_union_branch() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Cat {
    public function purr(): void {}
    public function eat(): void {}
}
class Dog {
    public function bark(): void {}
    public function eat(): void {}
}
class Shelter {
    /**
     * @return Cat|Dog
     */
    public function adopt(): Cat|Dog {
        return new Cat();
    }
}

class Test {
    public function run(): void {
        $shelter = new Shelter();
        $pet = $shelter->adopt();
        $pet->eat();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected when the member exists on every union branch, got: {:?}",
        diags
    );
}

#[test]
fn flags_member_missing_from_all_union_branches() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Cat {
    public function purr(): void {}
}
class Dog {
    public function bark(): void {}
}
class Shelter {
    /**
     * @return Cat|Dog
     */
    public function adopt(): Cat|Dog {
        return new Cat();
    }
}

class Test {
    public function run(): void {
        $shelter = new Shelter();
        $pet = $shelter->adopt();
        $pet->fly();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.iter().any(|d| d.message.contains("fly")),
        "expected diagnostic for member missing from all union branches, got: {:?}",
        diags
    );
}

#[test]
fn union_diagnostic_message_mentions_multiple_types() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Cat {
    public function purr(): void {}
}
class Dog {
    public function bark(): void {}
}
class Shelter {
    /**
     * @return Cat|Dog
     */
    public function adopt(): Cat|Dog {
        return new Cat();
    }
}

class Test {
    public function run(): void {
        $shelter = new Shelter();
        $pet = $shelter->adopt();
        $pet->fly();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let d = diags
        .iter()
        .find(|d| d.message.contains("fly"))
        .expect("expected diagnostic");
    assert!(
        d.message.contains("Cat") && d.message.contains("Dog"),
        "expected both union member types in message: {}",
        d.message
    );
}

/// When the subject is a union and any branch defines `__call`, the
/// access is dynamically dispatched through that branch at runtime, so
/// it must not be flagged.  This is the Mockery higher-order-message
/// pattern: a fluent method call on a mock return type must not warn.
#[test]
fn no_diagnostic_when_any_union_branch_has_magic_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Normal {
    public function known(): void {}
}
class Dynamic {
    public function __call(string $name, array $args): mixed { return null; }
}

class Test {
    /**
     * @return Normal|Dynamic
     */
    public function get(): Normal|Dynamic { return new Normal(); }

    public function run(): void {
        $x = $this->get();
        $x->anything();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Method dispatched through a union branch's __call must not be flagged, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// stdClass dynamic property chains
// ═══════════════════════════════════════════════════════════════════════════

/// A property assigned `new stdClass()` resolves to stdClass when read
/// again, so a further property access on it is not flagged.
#[test]
fn no_diagnostic_for_nested_stdclass_property_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function test(): void {
    $settings = new stdClass();
    $settings->cache = new stdClass();
    $settings->cache->ttl = 3600;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for nested stdClass property chain, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_deeply_nested_stdclass_property_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function test(): void {
    $root = new stdClass();
    $root->a = new stdClass();
    $root->a->b = new stdClass();
    $root->a->b->c = 1;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for deeply nested stdClass property chain, got: {:?}",
        diags
    );
}

/// Reassigning `$s` drops the stale `$s->cache` type, so `$s->cache`
/// resolves against the new object (a typed class here) rather than the
/// stdClass assigned before the reassignment.
#[test]
fn stdclass_property_key_invalidated_on_base_reassignment() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Holder { public ?Holder $cache = null; public int $ttl = 0; }
function test(): void {
    $s = new stdClass();
    $s->cache = new stdClass();
    $s = new Holder();
    echo $s->cache->ttl;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "$s->cache must resolve against Holder (not the stale stdClass), got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// PHPDoc property inheritance
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_phpdoc_property_on_child_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @property string $virtualProp
 */
class Base {
    public function __get(string $name): mixed { return null; }
}

class Child extends Base {}

function test(): void {
    $c = new Child();
    echo $c->virtualProp;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for @property inherited on child class, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_phpdoc_property_from_interface() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @property string $name
 */
interface HasName {}

class User implements HasName {
    public function __get(string $n): mixed { return null; }
}

function test(): void {
    $u = new User();
    echo $u->name;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for @property declared on an implemented interface, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// PHPDoc virtual members inside type-narrowing contexts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_phpdoc_members_inside_assert() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @method string getName()
 */
class Entity {
    public function __call(string $name, array $args): mixed { return null; }
}

class Base {}

class Test {
    public function run(Base $item): void {
        assert($item instanceof Entity);
        echo $item->getName();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for @method virtual member after assert instanceof, got: {:?}",
        diags
    );
}

/// `\assert($item instanceof Entity)` — the leading backslash is the
/// global-namespace FQN form.  It should narrow the variable type
/// identically to the unqualified `assert()`.
#[test]
fn no_diagnostic_for_fqn_assert_instanceof() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @method string getName()
 */
class Entity {
    public function __call(string $name, array $args): mixed { return null; }
}

class Base {}

class Test {
    public function run(Base $item): void {
        \assert($item instanceof Entity);
        echo $item->getName();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for FQN \\assert instanceof narrowing, got: {:?}",
        diags
    );
}

/// Combines FQN `\assert()` narrowing and interleaved
/// array-access/property-chain resolution.
#[test]
fn no_diagnostic_for_fqn_assert_with_interleaved_array_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class FormError {
    public function getMessage(): string { return ''; }
}

class FormChild {
    public function getName(): string { return ''; }
}

/** @var \Iterator<int, mixed> */
$errorIterator = new \ArrayIterator([]);
/** @var FormChild $child */
$child = new FormChild();
/** @var array<string, list<string>> */
$errors = [];

foreach ($errorIterator as $error) {
    \assert(
        $error instanceof FormError,
        'Error is not a FormError!',
    );
    $errors[$child->getName()][] = $error->getMessage();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for FQN \\assert with interleaved array access, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_phpdoc_members_after_instanceof_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @method string getName()
 */
class Entity {
    public function __call(string $name, array $args): mixed { return null; }
}

class Base {}

class Test {
    public function run(Base $item): void {
        if ($item instanceof Entity) {
            echo $item->getName();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for @method virtual member after if-instanceof narrowing, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Inline `&&` narrowing (instanceof as the LHS of `&&`)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_instanceof_and_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class QueryException extends \Exception {
    public array $errorInfo = [];
}

function test(\Throwable $e): void {
    $e instanceof QueryException && $e->errorInfo;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for && narrowing, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_instanceof_and_chain_in_catch() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class QueryException extends \Exception {
    public array $errorInfo = [];
}

function test(): void {
    try {
        throw new \Exception('fail');
    } catch (\Throwable $e) {
        $e instanceof QueryException && $e->errorInfo;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for && narrowing in catch, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_instanceof_and_chain_method_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class SpecialException extends \Exception {
    public function getDetail(): string { return ''; }
}

function test(\Throwable $e): void {
    $e instanceof SpecialException && $e->getDetail();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for && narrowing with method call, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_instanceof_and_chain_in_if_condition() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class QueryException extends \Exception {
    public array $errorInfo = [];
}

function test(\Throwable $e): void {
    if ($e instanceof QueryException && count($e->errorInfo) > 0) {
        echo 'has errors';
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for && narrowing in if condition, got: {:?}",
        diags
    );
}

/// Real-world repro: instanceof on the LHS of `&&` inside a `return`
/// statement.  The narrowing must propagate through the entire chained
/// `&&` even when wrapped in `return`.
#[test]
fn no_diagnostic_for_instanceof_and_chain_in_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class QueryException extends \Exception {
    public array $errorInfo = [];
}

trait UniqueConstraintViolation {
    protected function isUniqueConstraintViolation(\Throwable $exception): bool {
        return $exception instanceof QueryException
            && is_array($exception->errorInfo)
            && count($exception->errorInfo) >= 2
            && ($exception->errorInfo[0] ?? '') === '23000'
            && ($exception->errorInfo[1] ?? 0) === 1062;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for && narrowing in return, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_ternary_instanceof_in_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class SpecialException extends \Exception {
    public function getDetail(): string { return ''; }
}

function test(\Throwable $e): string {
    return $e instanceof SpecialException ? $e->getDetail() : 'unknown';
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for ternary instanceof in return, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_chained_and_instanceof() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class DetailedException extends \Exception {
    public string $detail = '';
    public string $context = '';
}

function test(\Throwable $e): void {
    $e instanceof DetailedException && $e->detail !== '' && $e->context !== '';
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for chained && narrowing, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Property chains through nested objects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_unknown_member_on_property_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public function known(): void {}
}
class Outer {
    public Inner $inner;
}

class Test {
    public function run(): void {
        $o = new Outer();
        $o->inner->missing();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.iter().any(|d| d.message.contains("missing")),
        "expected diagnostic for unknown member on property chain, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_valid_property_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public function known(): void {}
}
class Outer {
    public Inner $inner;
}

class Test {
    public function run(): void {
        $o = new Outer();
        $o->inner->known();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for valid property chain, got: {:?}",
        diags
    );
}

#[test]
fn flags_unknown_member_on_method_return_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public function known(): void {}
}
class Outer {
    public function getInner(): Inner { return new Inner(); }
}

function test(): void {
    $o = new Outer();
    $o->getInner()->missing();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.iter().any(|d| d.message.contains("missing")),
        "expected diagnostic for unknown member on method return chain, got: {:?}",
        diags
    );
}

#[test]
fn flags_unknown_member_on_virtual_property_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public function known(): void {}
}

/**
 * @property Inner $inner
 */
class Outer {
    public function __get(string $name): mixed { return null; }
}

function test(): void {
    $o = new Outer();
    $o->inner->missing();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.iter().any(|d| d.message.contains("missing")),
        "expected diagnostic for unknown member on virtual property chain, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Scalar member access (member access on int/string/bool subjects)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_member_access_on_scalar_property_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public int $value = 0;
}

class Test {
    public function run(): void {
        $foo = new Foo();
        $foo->value->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
    assert!(
        diags
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
        "expected ERROR severity for scalar access, got: {:?}",
        diags
    );
}

#[test]
fn flags_member_access_on_string_property_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public string $name = '';
}

class Test {
    public function run(): void {
        $foo = new Foo();
        $foo->name->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("string") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_member_access_on_scalar_method_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function getCount(): int { return 0; }
}

class Test {
    public function run(): void {
        $foo = new Foo();
        $foo->getCount()->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_method_call_on_scalar_method_return_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public function getValue(): string { return ''; }
}

class Middle {
    public function getInner(): Inner { return new Inner(); }
}

class Outer {
    public function getMiddle(): Middle { return new Middle(); }
}

class Test {
    public function run(): void {
        $o = new Outer();
        $o->getMiddle()->getInner()->getValue()->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("string") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_method_call_on_scalar_return_typed_param() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function getCount(): int { return 0; }
}
function test(Foo $foo): void {
    $foo->getCount()->nonexistent();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_scalar_access_on_static_method_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public static function getCount(): int { return 0; }
}
class Test {
    public function run(): void {
        Foo::getCount()->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_scalar_access_on_function_return_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function getNumber(): int { return 42; }
function test(): void {
    getNumber()->nonexistent();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_scalar_access_on_docblock_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    /**
     * @return string
     */
    public function getName() { return ''; }
}

class Test {
    public function run(): void {
        $foo = new Foo();
        $foo->getName()->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("string") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_scalar_access_on_static_return_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function getName(): string { return ''; }
}
class Test {
    public function run(): void {
        $foo = new Foo();
        $foo->getName()->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("string") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

/// Fluent chains that return `self`/`$this` never resolve to a scalar,
/// so no scalar-access diagnostic should ever fire on them.
#[test]
fn no_scalar_diagnostic_for_class_returning_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Builder {
    public function where(): self { return $this; }
    public function get(): self { return $this; }
}
function test(): void {
    $b = new Builder();
    $b->where()->get();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no scalar access diagnostic for class-returning chain, got: {:?}",
        diags
    );
}

#[test]
fn flags_scalar_access_on_function_returning_class_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function getName(): string { return ''; }
}
function createFoo(): Foo { return new Foo(); }
function test(): void {
    createFoo()->getName()->nonexistent();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("string") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_scalar_access_on_array_element_method_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Item {
    public function getLabel(): string { return ''; }
}

function test(): void {
    /** @var array<int, Item> $items */
    $items = [];
    $items[0]->getLabel()->nonexistent();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("string") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_scalar_access_on_deeper_method_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public function getValue(): int { return 42; }
}
class Outer {
    public function getInner(): Inner { return new Inner(); }
}
class Test {
    public function run(): void {
        $o = new Outer();
        $o->getInner()->getValue()->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_scalar_property_access_on_deeper_method_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public string $label = '';
}
class Outer {
    public function getInner(): Inner { return new Inner(); }
}
class Test {
    public function run(): void {
        $o = new Outer();
        $o->getInner()->label->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("string") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_member_access_on_virtual_scalar_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @property int $age
 * @property string $name
 */
class User {
    public function __get(string $name): mixed { return null; }
}

class Test {
    public function run(): void {
        $u = new User();
        $u->age->nonexistent();
        $u->name->nonexistent2();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for int virtual property, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_scalar_property_access_itself() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public int $count = 0;
}
function test(): void {
    $f = new Foo();
    echo $f->count;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "scalar property access itself should not be flagged, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Scalar member access on bare variables and typed parameters
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_member_access_on_bare_int_variable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function getCount(): int { return 0; }
}

class Test {
    public function run(): void {
        $foo = new Foo();
        $number = $foo->getCount();
        $number->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for bare int variable, got: {:?}",
        diags
    );
}

#[test]
fn flags_property_access_on_bare_string_variable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function getName(): string { return ''; }
}

class Test {
    public function run(): void {
        $foo = new Foo();
        $name = $foo->getName();
        $name->nonexistent;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("string") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for bare string variable, got: {:?}",
        diags
    );
}

#[test]
fn flags_method_access_on_bare_bool_variable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function isValid(): bool { return true; }
}

class Test {
    public function run(): void {
        $foo = new Foo();
        $valid = $foo->isValid();
        $valid->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("bool") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for bare bool variable, got: {:?}",
        diags
    );
}

#[test]
fn flags_member_access_on_scalar_function_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function getNumber(): int { return 42; }
class Test {
    public function run(): void {
        $n = getNumber();
        $n->nonexistent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for function return, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_bare_scalar_variable_without_member_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function test(): void {
    $n = 42;
    echo $n;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "bare scalar variable without member access should not produce diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn flags_member_access_on_scalar_typed_parameter() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function test(int $value): void {
    $value->nonexistent();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("int") && d.message.contains("nonexistent")),
        "expected scalar access diagnostic for typed parameter, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Unknown class parameter / return types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_member_access_on_unknown_class_parameter() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function test(NonExistentClass $obj): void {
    $obj->doSomething();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.iter().any(|d| {
            d.message.contains("doSomething") && d.message.contains("NonExistentClass")
        }),
        "expected diagnostic for unknown class parameter, got: {:?}",
        diags
    );
}

#[test]
fn flags_member_access_on_unknown_return_type_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
/** @return NonExistentClass */
function createObj() { return new stdClass; }
function test(): void {
    $obj = createObj();
    $obj->doSomething();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        !diags.is_empty(),
        "expected diagnostic for unknown return type, got: {:?}",
        diags
    );
}

#[test]
fn no_unknown_class_diagnostic_for_mixed_parameter() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function test(mixed $obj): void {
    $obj->doSomething();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for mixed parameter, got: {:?}",
        diags
    );
}

#[test]
fn no_unknown_class_diagnostic_for_class_string_parameter() {
    let backend = create_test_backend_with_stubs();
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @param class-string<BackedEnum> $enum
 */
function test(string $enum): void {
    $enum::from('test');
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for class-string parameter, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Array shape / type-alias object values
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_type_alias_array_shape_object_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Service {
    public function getName(): string { return ''; }
}

class Factory {
    /**
     * @return array{service: Service, name: string}
     */
    public function create(): array { return []; }
}

class Test {
    public function run(): void {
        $f = new Factory();
        $result = $f->create();
        $result['service']->getName();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for array shape object value, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_multiple_type_alias_object_values() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class UserService {
    public function findAll(): array { return []; }
}

class PostService {
    public function findRecent(): array { return []; }
}

class Container {
    /**
     * @return array{users: UserService, posts: PostService}
     */
    public function services(): array { return []; }
}

class Test {
    public function run(): void {
        $c = new Container();
        $services = $c->services();
        $services['users']->findAll();
        $services['posts']->findRecent();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for multiple array shape object values, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_inline_array_element_function_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Item {
    public function process(): void {}
}

function getItems(): array {
    /** @var Item[] */
    return [];
}

function test(): void {
    getItems()[0]->process();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for inline array element function call, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_when_member_exists_on_pre_resolved_base_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Builder {
    public function where(): self { return $this; }
    public function get(): array { return []; }
}
function test(): void {
    $b = new Builder();
    $b->where();
    $b->get();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for existing methods, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// @see tag references
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_see_tag_method_reference() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {}

    /**
     * @see Foo::bar()
     */
    public function test(): void {}
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for @see tag method reference, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_see_tag_constant_reference() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    const BAR = 1;

    /**
     * @see Foo::BAR
     */
    public function test(): void {}
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for @see tag constant reference, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_see_tag_hash_fragment_reference() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {}

    /**
     * @see Foo#bar
     */
    public function test(): void {}
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for @see tag hash-fragment reference, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_inline_see_tag_method_reference() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): void {}

    /**
     * This delegates to {@see Foo::bar()}.
     */
    public function test(): void {}
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for inline @see reference, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Namespaced stub class member / conditional $this return
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_namespaced_stub_class_member() {
    let mut stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    stubs.insert(
        "Ns\\StubClass",
        r#"<?php
namespace Ns;
class StubClass {
    public function stubMethod(): void {}
}
"#,
    );
    let backend = phpantom_lsp::Backend::new_test_with_stubs(stubs);
    let uri = "file:///test.php";
    let text = r#"<?php
use Ns\StubClass;

function test(StubClass $obj): void {
    $obj->stubMethod();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostic for namespaced stub class member, got: {:?}",
        diags
    );
}

#[test]
fn no_false_positive_on_conditional_this_return_in_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Builder {
    /**
     * @return $this
     */
    public function where(): static { return $this; }

    public function get(): array { return []; }
}
class Test {
    public function run(): void {
        $b = new Builder();
        $b->where()->get();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no false positive on conditional $this return chain, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-method / cross-function subject-cache scope isolation
// ═══════════════════════════════════════════════════════════════════════════

/// The subject resolution cache must be scoped to the enclosing method,
/// not just the enclosing class.  Two methods in the same class that
/// both use `$order->` must not share a cache entry when `$order` has a
/// completely different type in each method.
#[test]
fn no_false_positive_when_same_var_has_different_type_in_different_methods() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class OrderA {
    public function propOnA(): void {}
}
class OrderB {
    public function propOnB(): void {}
}
class Service {
    public function handleA(OrderA $order): void {
        $order->propOnA();
    }
    public function handleB(OrderB $order): void {
        $order->propOnB();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no false positives when same-named variable has different types \
         in different methods, got: {:?}",
        diags
    );
}

/// Same bug as the class-method variant, but with top-level functions
/// instead of methods.
#[test]
fn no_false_positive_same_var_different_type_top_level_functions() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Alpha {
    public function alphaMethod(): void {}
}
class Beta {
    public function betaMethod(): void {}
}
function first(Alpha $x): void {
    $x->alphaMethod();
}
function second(Beta $x): void {
    $x->betaMethod();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no false positives for same-named variable in different \
         top-level functions, got: {:?}",
        diags
    );
}

/// The flip side: a member that IS valid in one method must still be
/// flagged as unknown in another method where the variable has a
/// different type that lacks the member.
#[test]
fn flags_unknown_member_despite_valid_in_other_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class HasFoo {
    public function foo(): void {}
}
class NoFoo {
    public function bar(): void {}
}
class Service {
    public function a(HasFoo $x): void {
        $x->foo();
    }
    public function b(NoFoo $x): void {
        $x->foo();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("foo") && d.message.contains("NoFoo")),
        "expected diagnostic for foo() on NoFoo in method b(), got: {:?}",
        diags
    );
    let foo_diags: Vec<_> = diags.iter().filter(|d| d.message.contains("foo")).collect();
    assert_eq!(
        foo_diags.len(),
        1,
        "expected exactly one 'foo' diagnostic (in method b), got: {:?}",
        foo_diags
    );
}

/// When a method parameter is reassigned mid-body, subsequent accesses
/// must resolve against the new type, not the original parameter type.
#[test]
fn no_false_positive_when_parameter_is_reassigned() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class UploadedFile {
    public string $originalName;
}
class FileModel {
    public int $id;
    public string $name;
}
class Result {
    public function getFile(): FileModel { return new FileModel(); }
}
class FileUploadService {
    public function uploadFile(UploadedFile $file): void {
        $file->originalName;
        $result = new Result();
        $file = $result->getFile();
        $file->id;
        $file->name;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no false positives when parameter is reassigned mid-body, got: {:?}",
        diags
    );
}

/// The flip side: after reassignment, members from the NEW type that
/// don't exist should still be flagged.
#[test]
fn flags_unknown_member_after_reassignment() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class TypeA {
    public function onlyOnA(): void {}
}
class TypeB {
    public function onlyOnB(): void {}
}
class Service {
    public function process(TypeA $var): void {
        $var->onlyOnA();
        $var = new TypeB();
        $var->onlyOnA();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("onlyOnA") && d.message.contains("TypeB")),
        "expected diagnostic for onlyOnA() on TypeB after reassignment, got: {:?}",
        diags
    );
    let relevant: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("onlyOnA"))
        .collect();
    assert_eq!(
        relevant.len(),
        1,
        "expected exactly one 'onlyOnA' diagnostic (after reassignment), got: {:?}",
        relevant
    );
}

/// `$found = null; foreach (...) { $found = $pen; } $found->write()` must
/// not produce a scalar_member_access diagnostic when the foreach value
/// variable has a known type.
#[test]
fn no_false_positive_null_init_foreach_var_to_var_reassign() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Pen {
    public function write(): void {}
    public function color(): string { return ''; }
}
class Svc {
    /** @param list<Pen> $pens */
    public function find(array $pens): void {
        $found = null;
        foreach ($pens as $pen) {
            if ($pen->color() === 'blue') {
                $found = $pen;
            }
        }
        if ($found) {
            $found->write();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $found->write() after foreach reassign, got: {:?}",
        scalar_diags
    );
    let unknown_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("write"))
        .collect();
    assert!(
        unknown_diags.is_empty(),
        "should not flag unknown member 'write' on $found after foreach reassign, got: {:?}",
        unknown_diags
    );
}

/// `$valid = null; foreach (...) { if (...) { $valid = $item; break; } }`
/// `if (!$valid) { return ...; } $valid->details` must not produce a
/// scalar_member_access diagnostic.  The guard clause strips null from
/// the scope, leaving only the class type.  Exercises the forward-walker
/// scope cache.
#[test]
fn no_false_positive_null_init_foreach_guard_clause_early_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Mandate {
    public object $details;
    public function isInvalid(): bool { return false; }
}
class Client {
    /** @return mixed */
    public function getMandates(): mixed { return []; }
}
class Svc {
    public function check(): ?object {
        $client = new Client();
        $mandates = $client->getMandates();
        $validMandate = null;
        /** @var Mandate $mandate */
        foreach ($mandates as $mandate) {
            if (!$mandate->isInvalid()) {
                $validMandate = $mandate;
                break;
            }
        }

        if (!$validMandate) {
            return null;
        }

        $details = $validMandate->details;
        return $details;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $validMandate->details after guard clause, got: {:?}",
        scalar_diags
    );
}

/// `fn($x) => $x instanceof Foo && $x->method()` — an untyped arrow
/// function parameter narrowed by an earlier `&&` conjunct must be
/// visible to the member access in a later conjunct, when the
/// forward-walker scope cache is active.
#[test]
fn arrow_fn_param_narrowed_by_and_instanceof_scope_cache() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Collection {
    public function contains($x): bool { return true; }
}
class Svc {
    public function run(): void {
        $faq1 = 1;
        $cb = fn($faqs) => $faqs instanceof Collection && $faqs->contains($faq1);
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "arrow-fn param narrowed by `&&` instanceof should resolve, got: {:?}",
        diags
    );
}

/// Same scenario, but through the plain (non-scope-cache) diagnostic
/// path, to prove the narrowing works uniformly across both.
#[test]
fn arrow_fn_param_narrowed_by_and_instanceof_fresh_path() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Collection {
    public function contains($x): bool { return true; }
}
class Svc {
    public function run(): void {
        $faq1 = 1;
        $cb = fn($faqs) => $faqs instanceof Collection && $faqs->contains($faq1);
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "arrow-fn param narrowed by `&&` instanceof should resolve (fresh path), got: {:?}",
        diags
    );
}

/// Direct instantiation inside a foreach body (no var-to-var).
#[test]
fn no_false_positive_null_init_foreach_direct_reassign() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Transaction {
    public function commit(): void {}
}
class Svc {
    /** @param list<string> $items */
    public function process(array $items): void {
        $tx = null;
        foreach ($items as $item) {
            $tx = new Transaction();
        }
        if ($tx) {
            $tx->commit();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let bad_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("commit") || d.message.contains("null"))
        .collect();
    assert!(
        bad_diags.is_empty(),
        "should not flag commit() or scalar null after foreach reassign, got: {:?}",
        bad_diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Negative narrowing after early return (guard clauses)
// ═══════════════════════════════════════════════════════════════════════════

/// After `if ($value instanceof Stringable) { return; }`, the variable
/// should be narrowed to exclude Stringable.  Inside a subsequent
/// `if ($value instanceof BackedEnum)` block, `$value` must resolve to
/// BackedEnum (not Stringable).
#[test]
fn no_false_positive_after_guard_clause_excludes_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Stringable {
    public function __toString(): string;
}
interface BackedEnum {
    public readonly int|string $value;
}

class Svc {
    public static function toString(mixed $value): string
    {
        if ($value instanceof Stringable) {
            return $value->__toString();
        }
        if ($value instanceof BackedEnum) {
            $value = $value->value;
        }
        return '';
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let bad: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("value") && d.message.contains("Stringable"))
        .collect();
    assert!(
        bad.is_empty(),
        "should not flag 'value' on Stringable after guard clause excludes it, got: {:?}",
        bad
    );
}

/// Multiple sequential guard clauses should each exclude their type
/// from subsequent code.
#[test]
fn no_false_positive_sequential_instanceof_guards() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Alpha {
    public function alphaMethod(): void;
}
interface Beta {
    public function betaMethod(): void;
}
class Gamma {
    public function gammaMethod(): void {}
}

class Svc {
    public function test(Alpha|Beta|Gamma $x): void
    {
        if ($x instanceof Alpha) {
            return;
        }
        if ($x instanceof Beta) {
            return;
        }
        $x->gammaMethod();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let bad: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.message.contains("gammaMethod")
                && (d.message.contains("Alpha") || d.message.contains("Beta"))
        })
        .collect();
    assert!(
        bad.is_empty(),
        "should not flag gammaMethod after two guard clauses exclude Alpha and Beta, got: {:?}",
        bad
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// self:: / static:: enum case value/name access
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_self_enum_case_value() {
    let backend = create_test_backend_with_stubs();
    let uri = "file:///test.php";
    let text = r#"<?php
enum SizeUnit: string {
    case pcs = 'pcs';
    case pair = 'pair';
    case g = 'g';

    public function translation(): string {
        return self::pcs->value;
    }

    public static function units(): array {
        return [
            self::pcs->value,
            self::pair->value,
            self::g->value,
        ];
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for self::case->value, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_static_enum_case_value() {
    let backend = create_test_backend_with_stubs();
    let uri = "file:///test.php";
    let text = r#"<?php
enum Currency: string {
    case USD = 'usd';
    case EUR = 'eur';

    public static function defaults(): array {
        return [static::USD->value];
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for static::case->value, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_self_enum_case_name() {
    let backend = create_test_backend_with_stubs();
    let uri = "file:///test.php";
    let text = r#"<?php
enum Color: int {
    case Red = 1;
    case Blue = 2;

    public function label(): string {
        return self::Red->name;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "No diagnostics expected for self::case->name, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ArrayAccess-implementing collections and parent static return chains
// ═══════════════════════════════════════════════════════════════════════════

/// `$obj->prop['key']` where `prop` is a collection class with
/// `@extends DataCollection<string, Day>` should resolve the bracket
/// access to the element type.
#[test]
fn no_diagnostic_for_property_chain_array_access_on_collection() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Day {
    public string $from;
    public string $to;
}

/**
 * @template TKey of array-key
 * @template TValue
 * @implements \ArrayAccess<TKey, TValue>
 */
class DataCollection implements \ArrayAccess {
    /** @return TValue */
    public function offsetGet(mixed $offset): mixed {}
    public function offsetExists(mixed $offset): bool {}
    public function offsetSet(mixed $offset, mixed $value): void {}
    public function offsetUnset(mixed $offset): void {}
}

/**
 * @extends DataCollection<string, Day>
 */
class OpeningHours extends DataCollection {}

class ServicePoint {
    public ?OpeningHours $opening_hours;
}

function test(ServicePoint $sp): void {
    $day = $sp->opening_hours['monday'] ?? null;
    if ($day !== null) {
        $day->from;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for property chain array access on collection, got: {:?}",
        diags
    );
}

/// `parent::method()` should resolve the return type from the parent
/// class so that member access on the result works.
#[test]
fn no_diagnostic_for_parent_static_call_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Response {
    public function status(): int { return 200; }
    public function body(): string { return ''; }
}

class BaseConnector {
    protected function call(string $endpoint): Response
    {
        return new Response();
    }
}

class LoggedConnection extends BaseConnector {
    protected function call(string $endpoint): Response
    {
        $response = parent::call($endpoint);
        $response->status();
        $response->body();
        return $response;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for parent::call() return type chain, got: {:?}",
        diags
    );
}

/// Bracket access on a class implementing `ArrayAccess` without concrete
/// generic annotations should NOT resolve to the container class
/// itself, including when `ArrayAccess` is implemented on a parent
/// class rather than the concrete subclass.
#[test]
fn flags_member_on_array_access_subclass_without_generics() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
namespace Tests;

use ArrayAccess;

class Container2 implements ArrayAccess
{
    public function offsetExists($offset): bool
    {
        return false;
    }

    public function offsetGet($offset): mixed
    {
        return '';
    }

    public function offsetSet($offset, $value): void
    {
    }

    public function offsetUnset($offset): void
    {
    }
}

class Application2 extends Container2
{
}

class TestCase
{
    public function defineEnvironment(): void
    {
        $test4 = new Application2();
        $test4['config']->set('logging.channels.stack.channels', ['stderr']);
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("Application2")),
        "should not report 'set' as missing on Application2 — bracket access returns mixed, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Assignment inside `if`/`while` conditions
// ═══════════════════════════════════════════════════════════════════════════

/// Variables assigned inside `if` conditions should resolve in the body.
#[test]
fn assignment_in_if_condition_resolves_in_body() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class AdminUser {
    public function assignRole(string $role): void {}
    /** @return ?static */
    public static function first(): ?static { return new static(); }
}
function test(string $role): void {
    if ($admin = AdminUser::first()) {
        $admin->assignRole($role);
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let bad: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("assignRole") || d.message.contains("admin"))
        .collect();
    assert!(
        bad.is_empty(),
        "should resolve $admin from if-condition assignment, got: {:?}",
        bad
    );
}

/// Assignment inside comparison `if (($x = expr()) !== null)` should
/// resolve.
#[test]
fn assignment_in_if_condition_with_comparison() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Conn {
    public function query(string $sql): void {}
}
function getConn(): ?Conn { return new Conn(); }
function test(): void {
    if (($conn = getConn()) !== null) {
        $conn->query('SELECT 1');
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let bad: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("query") || d.message.contains("conn"))
        .collect();
    assert!(
        bad.is_empty(),
        "should resolve $conn from if-condition assignment with !== null, got: {:?}",
        bad
    );
}

/// Assignment in `while` condition should resolve in the loop body.
#[test]
fn assignment_in_while_condition_resolves_in_body() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Row {
    public function toArray(): array { return []; }
}
function nextRow(): ?Row { return new Row(); }
function test(): void {
    while ($row = nextRow()) {
        $row->toArray();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let bad: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("toArray") || d.message.contains("row"))
        .collect();
    assert!(
        bad.is_empty(),
        "should resolve $row from while-condition assignment, got: {:?}",
        bad
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Chain error propagation (broken links suppress downstream links)
// ═══════════════════════════════════════════════════════════════════════════

/// `$m->callHome()->callMom()->callDad()` — only `callHome` should be
/// flagged; `callMom` and `callDad` are downstream of the break.
#[test]
fn chain_propagation_flags_only_first_broken_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Machine {
    public function knownMethod(): self { return $this; }
}

function test(): void {
    $m = new Machine();
    $m->callHome()->callMom()->callDad();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (first broken link only), got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("callHome"),
        "expected diagnostic for callHome, got: {:?}",
        diags[0].message
    );
}

/// `$m->callHome(); $m->callMom();` — separate statements, both should
/// be flagged independently.
#[test]
fn chain_propagation_separate_statements_flag_both() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Machine {
    public function knownMethod(): self { return $this; }
}

function test(): void {
    $m = new Machine();
    $m->callHome();
    $m->callMom();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        2,
        "expected 2 diagnostics (separate statements), got: {:?}",
        diags
    );
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("callHome")),
        "expected callHome diagnostic"
    );
    assert!(
        messages.iter().any(|m| m.contains("callMom")),
        "expected callMom diagnostic"
    );
}

/// `$user->getAge()->value->deep` — only `->value` should be flagged
/// (scalar access on int); `->deep` is downstream of the scalar break.
#[test]
fn chain_propagation_scalar_suppresses_downstream() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class User {
    public function getAge(): int { return 30; }
}

function test(): void {
    $user = new User();
    $user->getAge()->value->deep;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (scalar access only), got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("int"),
        "expected scalar type 'int' in message, got: {:?}",
        diags[0].message
    );
}

/// `$o->getInner()->fakeMethod()->next()` — only `fakeMethod` should be
/// flagged; `next()` is downstream.
#[test]
fn chain_propagation_second_link_broken_suppresses_rest() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public function known(): void {}
}
class Outer {
    public function getInner(): Inner { return new Inner(); }
}

function test(): void {
    $o = new Outer();
    $o->getInner()->fakeMethod()->next()->deep();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (first broken link), got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("fakeMethod"),
        "expected diagnostic for fakeMethod, got: {:?}",
        diags[0].message
    );
}

/// `$o->getMiddle()->getInner()->getValue()->nonexistent()->another()` —
/// only `nonexistent()` should be flagged (scalar access on string).
#[test]
fn chain_propagation_scalar_method_return_suppresses_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public function getValue(): string { return ''; }
}

class Middle {
    public function getInner(): Inner { return new Inner(); }
}

class Outer {
    public function getMiddle(): Middle { return new Middle(); }
}

class Test {
    public function run(): void {
        $o = new Outer();
        $o->getMiddle()->getInner()->getValue()->nonexistent()->another();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (scalar access), got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("nonexistent"),
        "expected diagnostic for nonexistent, got: {:?}",
        diags[0].message
    );
}

/// A broken property `value` must not suppress a separate property
/// `value_extra` on the same subject (no accidental prefix matching).
#[test]
fn chain_propagation_property_does_not_match_longer_name() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public int $value = 0;
    public string $value_extra = '';
}

function test(): void {
    $f = new Foo();
    $f->value->nope;
    $f->value_extra->nope;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        2,
        "expected 2 diagnostics (value and value_extra are independent), got: {:?}",
        diags
    );
}

/// `Foo::create()->unknown()->next()` — only `unknown()` should be
/// flagged; `next()` is downstream.
#[test]
fn chain_propagation_static_method_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public static function create(): self { return new self(); }
    public function known(): self { return $this; }
}

function test(): void {
    Foo::create()->unknown()->next();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (first broken link), got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("unknown"),
        "expected diagnostic for unknown, got: {:?}",
        diags[0].message
    );
}

/// `$m?->callHome()?->callMom()` — only `callHome` should be flagged.
#[test]
fn chain_propagation_null_safe_operator() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Machine {
    public function knownMethod(): self { return $this; }
}

function test(?Machine $m): void {
    $m?->callHome()?->callMom();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (null-safe chain), got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("callHome"),
        "expected diagnostic for callHome, got: {:?}",
        diags[0].message
    );
}

/// `$this->unknownMethod()->next()` inside a class — only
/// `unknownMethod` should be flagged.
#[test]
fn chain_propagation_this_method_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function test(): void {
        $this->unknownMethod()->next()->deep();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic ($this chain), got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("unknownMethod"),
        "expected diagnostic for unknownMethod, got: {:?}",
        diags[0].message
    );
}

/// `$o->getInner()->label->nonexistent->deep` — only `->nonexistent`
/// should be flagged (scalar access on string from label).
#[test]
fn chain_propagation_property_chain_suppresses_downstream() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public string $label = '';
}
class Outer {
    public function getInner(): Inner { return new Inner(); }
}
class Test {
    public function run(): void {
        $o = new Outer();
        $o->getInner()->label->nonexistent->deep;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (scalar property access), got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("nonexistent") || diags[0].message.contains("string"),
        "expected diagnostic about scalar access on string, got: {:?}",
        diags[0].message
    );
}

/// `$o->getInner()::staticMissing()->next()` — only `staticMissing`
/// should be flagged.
#[test]
fn chain_propagation_mixed_arrow_and_static_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Inner {
    public function known(): void {}
}
class Outer {
    public function getInner(): Inner { return new Inner(); }
}

function test(): void {
    $o = new Outer();
    $o->getInner()::staticMissing()->next();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 diagnostic (first broken static link), got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("staticMissing"),
        "expected diagnostic for staticMissing, got: {:?}",
        diags[0].message
    );
}

/// Errors inside closure/arrow-function arguments are independent
/// expressions — they must NOT be suppressed by a broken link in the
/// outer chain.
#[test]
fn chain_propagation_does_not_suppress_errors_inside_closure_arguments() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Joe {
    public function where(callable $cb): self { return $this; }
}

class ShowThisError {
    public function valid(): void {}
}

function test(): void {
    $joe = new Joe();
    $showThisError = new ShowThisError();
    $joe::whereInvalid()->where(fn() => $showThisError->unknown())->hideMe()->hideMe();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("whereInvalid")),
        "expected diagnostic for whereInvalid (outer chain), got: {:?}",
        messages
    );
    assert!(
        messages.iter().any(|m| m.contains("unknown")),
        "expected diagnostic for unknown (inside closure), got: {:?}",
        messages
    );
    assert!(
        !messages.iter().any(|m| m.contains("hideMe")),
        "hideMe should be suppressed (downstream of whereInvalid), got: {:?}",
        messages
    );
    assert_eq!(
        diags.len(),
        2,
        "expected exactly 2 diagnostics (whereInvalid + unknown), got: {:?}",
        messages
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// `&&` short-circuit null narrowing (distinct from instanceof narrowing)
// ═══════════════════════════════════════════════════════════════════════════

/// `$lastPaidEnd !== null && $lastPaidEnd->diffInDays(…)` must not
/// produce a scalar_member_access diagnostic.  The `!== null` check on
/// the left side of `&&` should narrow away `null` for the right side.
#[test]
fn no_false_positive_and_short_circuit_null_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Carbon {
    public function diffInDays(Carbon $other): int { return 0; }
    public function startOfDay(): static { return $this; }
}
class Period {
    public Carbon $ending;
}
class Svc {
    /** @param list<Period> $periods */
    public function gaps(array $periods): void {
        $lastPaidEnd = null;
        $periodStart = new Carbon();
        foreach ($periods as $period) {
            if ($lastPaidEnd !== null && $lastPaidEnd->diffInDays($periodStart) > 0) {
                // should not report: Cannot access method 'diffInDays' on type 'null'
            }
            $lastPaidEnd = $period->ending->startOfDay();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $lastPaidEnd->diffInDays() after !== null guard in &&, got: {:?}",
        scalar_diags
    );
}

/// Variant: bare truthy check `$var && $var->method()`.
#[test]
fn no_false_positive_and_short_circuit_truthy_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Logger {
    public function log(string $msg): void {}
}
class Svc {
    public function run(): void {
        $logger = null;
        if (rand(0,1)) {
            $logger = new Logger();
        }
        $logger && $logger->log('hello');
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $logger->log() after truthy guard in &&, got: {:?}",
        scalar_diags
    );
}

/// Variant: chained `&&` with a null check as the first operand.
/// `$a !== null && $b !== null && $a->method()` — the null check for
/// `$a` is two levels up in the `&&` chain.
#[test]
fn no_false_positive_chained_and_null_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): int { return 0; }
}
class Svc {
    public function test(): void {
        $a = null;
        $b = null;
        if (rand(0,1)) { $a = new Foo(); }
        if (rand(0,1)) { $b = new Foo(); }
        if ($a !== null && $b !== null && $a->bar() > 0) {
            // both $a and $b are non-null here
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $a->bar() in chained && with null guards, got: {:?}",
        scalar_diags
    );
}

/// Variant: three null-init vars with a compound `&&` guard, member
/// access on the third var inside the if-body (not inside the
/// condition).
#[test]
fn no_false_positive_if_body_triple_null_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): int { return 0; }
    public function baz(): static { return $this; }
}
class Svc {
    public function test(): void {
        $x = null;
        $y = null;
        $z = null;
        if (rand(0,1)) { $x = new Foo(); }
        if (rand(0,1)) { $y = new Foo(); }
        if (rand(0,1)) { $z = new Foo(); }
        if ($x !== null && $y !== null && $z !== null && $x->baz()->bar() > 0) {
            $z->bar();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $z->bar() inside if-body after triple && null guard, got: {:?}",
        scalar_diags
    );
}

/// Variant: the null check in the if-condition narrows inside the
/// then-body for a *different* variable in the same `&&` chain.
#[test]
fn no_false_positive_if_body_null_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function bar(): int { return 0; }
}
class Svc {
    public function test(): void {
        $a = null;
        $b = null;
        if (rand(0,1)) { $a = new Foo(); }
        if (rand(0,1)) { $b = new Foo(); }
        if ($a !== null && $b !== null && $a->bar() > 0) {
            $b->bar();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $b->bar() inside if-body after && null guard, got: {:?}",
        scalar_diags
    );
}

/// Variant: `&&` inside a ternary condition in a `return` statement.
#[test]
fn no_false_positive_ternary_wrapped_and_null_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public function val(): int { return 0; }
}
class Svc {
    public function test(): int {
        $c = null;
        if (rand(0,1)) { $c = new Foo(); }
        return $c !== null && $c->val() > 5 ? 1 : 0;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $c->val() inside ternary-wrapped &&, got: {:?}",
        scalar_diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// __call chain continuation
// ═══════════════════════════════════════════════════════════════════════════

/// When a class defines `__call` with a typed return, the dispatched
/// method is valid PHP and must not be flagged.  Known methods after it
/// resolve through the `__call` return type.
#[test]
fn magic_call_chain_not_flagged_and_continues() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class AppleCart {
    public function getApples(): array { return []; }
}
class Builder {
    public function __call(string $name, array $args): static { return $this; }
    public function first(): AppleCart { return new AppleCart(); }
}
class Svc {
    public function run(): void {
        $b = new Builder();
        $b->doesntExist()->first()->getApples();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "doesntExist() is dispatched through __call (returns static), so nothing should be flagged, got: {:?}",
        diags
    );
}

/// Multiple `__call`-dispatched methods in a chain are all valid and
/// none should be flagged; known methods between and after them resolve
/// through the `__call` return type.
#[test]
fn magic_call_chain_multiple_dynamic_methods_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class AppleCart {
    public function getApples(): array { return []; }
}
class Builder {
    public function __call(string $name, array $args): static { return $this; }
    public function first(): AppleCart { return new AppleCart(); }
}
class Svc {
    public function run(): void {
        $b = new Builder();
        $b->doesntExist()->first()->getApples();
        $b->doesntExist()->alsoDoesntExist()->first()->getApples();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Dynamic methods dispatched through __call must not be flagged, got: {:?}",
        diags
    );
}

/// When `__call` returns a concrete type (not self/static), the
/// dispatched method is not flagged and the chain resolves to that type
/// afterwards.
#[test]
fn magic_call_concrete_return_continues_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Result {
    public function getData(): array { return []; }
}
class Proxy {
    public function __call(string $name, array $args): Result { return new Result(); }
}
class Svc {
    public function run(): void {
        $p = new Proxy();
        $p->anything()->getData();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "anything() dispatches through __call (returns Result), getData() resolves — nothing to flag, got: {:?}",
        diags
    );
}

/// When `__call` returns `mixed`, the dispatched method is still not
/// flagged.  The chain type becomes `mixed`, so downstream accesses are
/// simply unverifiable rather than flagged as unknown members.
#[test]
fn magic_call_mixed_return_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Loose {
    public function __call(string $name, array $args): mixed { return null; }
}
class Svc {
    public function run(): void {
        $l = new Loose();
        $l->unknown()->somethingElse();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        !diags.iter().any(|d| d.message.contains("unknown")),
        "unknown() is dispatched through __call and must not be flagged, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Variable reassignment tracking inside try/catch blocks
// ═══════════════════════════════════════════════════════════════════════════

/// When a variable is reassigned inside a `try` block, accesses after
/// the reassignment (still inside the try) should resolve against the
/// new type, not the original.
#[test]
fn no_false_positive_when_variable_reassigned_inside_try_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class AppCustomer {
    public function getName(): string { return ''; }
}
class MollieCustomer {
    public function createPayment(string $data): MolliePayment { return new MolliePayment(); }
}
class MolliePayment {
    public function getCheckoutUrl(): string { return ''; }
}
class MollieClient {
    public function getOrCreateCustomer(AppCustomer $c): MollieCustomer { return new MollieCustomer(); }
}
class Gateway {
    public function charge(AppCustomer $customer): void {
        $client = new MollieClient();
        try {
            $customer = $client->getOrCreateCustomer($customer);
            $molliePayment = $customer->createPayment('data');
            $url = $molliePayment->getCheckoutUrl();
        } catch (\Exception $e) {
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for reassigned variable inside try block, got: {:?}",
        diags
    );
}

/// The flip side: after reassignment inside a try block, members from
/// the OLD type that don't exist on the NEW type should be flagged.
#[test]
fn flags_unknown_member_after_reassignment_inside_try_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class OriginalType {
    public function onlyOnOriginal(): void {}
}
class ReplacementType {
    public function onlyOnReplacement(): void {}
}
class Service {
    public function process(OriginalType $var): void {
        try {
            $var = new ReplacementType();
            $var->onlyOnOriginal();
        } catch (\Exception $e) {
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("onlyOnOriginal") && d.message.contains("ReplacementType")),
        "expected diagnostic for onlyOnOriginal() on ReplacementType after reassignment in try, got: {:?}",
        diags
    );
}

/// After the try/catch block, the variable could be either the original
/// type (if the try threw before the reassignment) or the new type.
/// Both types' members should be accepted.
#[test]
fn try_block_reassignment_is_conditional_after_try() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class TypeA {
    public function methodA(): void {}
}
class TypeB {
    public function methodB(): void {}
}
class Svc {
    public function run(TypeA $var): void {
        try {
            $var = new TypeB();
        } catch (\Exception $e) {
        }
        $var->methodA();
        $var->methodB();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "after try/catch, both original and reassigned types should be accepted, got: {:?}",
        diags
    );
}

/// Variable reassignment inside a `catch` block should also be tracked.
#[test]
fn catch_block_variable_reassignment_tracked() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class ErrorResult {
    public function getErrorCode(): int { return 0; }
}
class SuccessResult {
    public function getData(): string { return ''; }
}
class Handler {
    public function handle(): void {
        $result = new SuccessResult();
        try {
            $result->getData();
        } catch (\Exception $e) {
            $result = new ErrorResult();
            $result->getErrorCode();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for reassigned variable inside catch block, got: {:?}",
        diags
    );
}

/// When a class extends `Collection<int, T>` via `@extends`, accessing
/// `$this->items` should yield `array<int, T>` with the generic
/// substitution applied, so that iterating it and calling a
/// generics-aware helper both resolve the element type.
#[test]
fn no_diagnostic_for_this_items_on_generic_collection_subclass() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
class Collection {
    /** @var array<TKey, TValue> */
    public array $items = [];

    /** @return TValue|null */
    public function first(): mixed { return null; }
}

class PurchaseFileProduct {
    public int $order_amount = 0;
    public string $name = '';
}

/**
 * @template TKey
 * @template TValue
 * @param array<TKey, TValue> $array
 * @param callable(TValue, TKey): bool $callback
 * @return bool
 */
function array_any(array $array, callable $callback): bool { return false; }

/**
 * @extends Collection<int, PurchaseFileProduct>
 */
final class PurchaseFileProductCollection extends Collection {
    public function hasIssues(): bool {
        return array_any($this->items, fn($item) => $item->order_amount > 0);
    }

    public function hasName(): bool {
        return array_any($this->items, fn($item) => $item->name !== '');
    }

    public function foreachWorks(): void {
        foreach ($this->items as $item) {
            $item->order_amount;
            $item->name;
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for $this->items on generic Collection subclass, got: {:?}",
        diags
    );
}

/// When a variable is assigned before a foreach, then reassigned inside
/// a try block nested inside the foreach body, the type should still
/// resolve for accesses after the reassignment (still inside the try).
#[test]
fn no_false_positive_when_variable_reassigned_inside_try_inside_foreach() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Decimal {
    public function sub(string $v): self { return new self(); }
    public function isZero(): bool { return true; }
    public function isNegative(): bool { return true; }
    public function isPositive(): bool { return true; }
    public function toFixed(int $places): string { return ''; }
}

/**
 * @property Decimal $amount
 * @property string $state
 */
class Payment {
}

/**
 * @property Decimal $amount
 */
class Order {
}

class CaptureException extends \Exception {}
class InvalidStateException extends \Exception {}
class CaptureService {
    public function captureReservedPayment(Payment $p, Decimal $amount): void {}
}

class OrderService {
    /** @param list<Payment> $payments */
    public function capture(Order $order, array $payments): void {
        $remaining = $order->amount;
        foreach ($payments as $payment) {
            if ($payment->state === 'paid') {
                $remaining = $remaining->sub('1');
            }
        }

        $svc = new CaptureService();
        foreach ($payments as $payment) {
            if ($payment->state !== 'reserved') {
                continue;
            }

            $toCapture = $remaining->isPositive() ? $payment->amount : $remaining;
            if ($toCapture->isZero() || $toCapture->isNegative()) {
                break;
            }

            try {
                $svc->captureReservedPayment($payment, $toCapture);
                $remaining = $remaining->sub('1');
            } catch (CaptureException|InvalidStateException $e) {
            }
        }

        if ($remaining->isPositive() && !$remaining->isZero()) {
            throw new \RuntimeException('remaining: ' . $remaining->toFixed(2));
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for variable reassigned inside try-inside-foreach, got: {:?}",
        diags
    );
}

/// Regression test for self-referential variable reassignment: when a
/// variable is reassigned inside a *nested* foreach via
/// `$result = $result->add(...)`, the forward walker must resolve the
/// outer foreach access correctly without a false "type could not be
/// resolved" diagnostic.
#[test]
fn no_false_positive_when_variable_reassigned_inside_nested_foreach() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Decimal {
    public function add(string $v): self { return new self(); }
    public function mul(string $v): self { return new self(); }
}

class Item {
    public Decimal $cost;
    public function isBundle(): bool { return false; }
    /** @return list<Item> */
    public function getChildren(): array { return []; }
}

class OrderService {
    /** @param list<Item> $items */
    public function calculateCost(array $items): Decimal {
        $zero = new Decimal();
        $result = $zero;
        foreach ($items as $item) {
            if ($item->isBundle()) {
                $children = $item->getChildren();
                foreach ($children as $child) {
                    $result = $result->add($child->cost->mul('1'));
                }

                continue;
            }

            $result = $result->add($item->cost->mul('1'));
        }

        return $result->mul('1');
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for variable reassigned inside nested foreach loops, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// `object` return/parameter type and `is_object()` guard clauses
// ═══════════════════════════════════════════════════════════════════════════

/// A call whose return type is `object` is the "any object" escape
/// hatch: property/method access on the result is always valid at
/// runtime, so no unresolved-member diagnostic should fire.
#[test]
fn no_diagnostic_for_object_return_type_member_access() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Repo {
    public function all(): object { return new \stdClass(); }
}
function test(Repo $r): void {
    $x = $r->all()->projects ?? [];
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for member access on object return type, got: {:?}",
        diags
    );
}

/// Adding nullability (`?object`) must not lose the `object` type and
/// leave the subject unresolvable.
#[test]
fn no_diagnostic_for_nullable_object_return_type_member_access() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Repo {
    public function all(): ?object { return new \stdClass(); }
}
function test(Repo $r): void {
    $x = $r->all()->projects ?? [];
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for member access on nullable object return type, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_after_is_object_guard() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
function test(mixed $data): void {
    if (is_object($data)) {
        echo $data->error_link;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after is_object() guard, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_after_is_object_guard_on_real_union() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Thing {
    public function bar(): void {}
}
function test(string|Thing $file): void {
    if (is_object($file)) {
        $file->bar();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after is_object() guard on a real union, got: {:?}",
        diags
    );
}

/// When upstream type inference produced a plain `string` type for a
/// variable that can, at runtime, also be an object, an `is_object()`
/// guard must still stop `scalar_member_access` on the guarded access —
/// trust the runtime check over the incomplete static type.
#[test]
fn no_scalar_member_access_after_is_object_guard_on_plain_string() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
function test(string $file): void {
    if (is_object($file)) {
        $file->getPathname();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $file->getPathname() inside is_object() guard, got: {:?}",
        scalar_diags
    );
}

#[test]
fn no_diagnostic_for_empty_on_missing_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Item {
    public string $name = '';
}

function test(Item $item): void {
    if (empty($item->maybeDynamic)) {
        echo 'ok';
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "empty() should suppress unknown-property diagnostics, got: {:?}",
        diags
    );
}

#[test]
fn no_scalar_member_access_for_isset_on_union_with_stdclass() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Item {
    public string $name = '';
}

function test(Item|\stdClass $item): void {
    if (isset($item->maybeDynamic)) {
        echo 'ok';
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "isset() on a union with stdClass should not flag the other union members, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_after_is_object_guard_with_negated_early_return() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
function test(mixed $data): void {
    if (!is_object($data)) {
        return;
    }
    echo $data->error_link;
    echo $data->something_else;
    $data->doStuff();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after negated is_object() early return, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_after_is_object_in_compound_and_condition() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
function test(mixed $data): void {
    if (is_object($data) && property_exists($data, 'error_link') && is_string($data->error_link)) {
        echo stripslashes($data->error_link);
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after is_object() in compound && condition, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_object_typed_parameter() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
function test(object $data): void {
    echo $data->name;
    $data->doStuff();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for object-typed parameter, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// class-string<T> static return type resolution / in_array guards
// ═══════════════════════════════════════════════════════════════════════════

/// When a parameter is typed `class-string<BackedEnum>` and we call
/// `$class::cases()`, the `static[]` return type should resolve to
/// `BackedEnum[]`, making foreach items typed as `BackedEnum` with
/// `->name` and `->value` available.
#[test]
fn no_diagnostic_for_class_string_static_return_in_foreach() {
    let backend = create_test_backend_with_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class OptionList {
    /**
     * @param class-string<BackedEnum> $class
     */
    public static function enum(BackedEnum $value, string $class, array $exclude = [], string $method = ''): void {
        foreach ($class::cases() as $item) {
            if (in_array($item, $exclude, true)) {
                continue;
            }

            $name = $method ? $item->{$method}() : $item->name;

            $val = $item->value;
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for class-string<BackedEnum> foreach item members, got: {:?}",
        diags
    );
}

/// `$class::from('foo')` returns `static` which should resolve to
/// `BackedEnum` when `$class` is `class-string<BackedEnum>`.
#[test]
fn no_diagnostic_for_class_string_static_return_chained() {
    let backend = create_test_backend_with_stubs();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Svc {
    /**
     * @param class-string<BackedEnum> $class
     */
    public function resolve(string $class): void {
        $result = $class::from('foo');
        $name = $result->name;
        $val  = $result->value;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for class-string<BackedEnum> static return chain, got: {:?}",
        diags
    );
}

/// When `in_array($item, $exclude, true)` is used as a guard clause
/// (`if (...) { continue; }`), the narrowing must NOT exclude the
/// variable's type when the haystack's element type matches the
/// variable's type — the check filters by value, not by type.
#[test]
fn in_array_guard_does_not_wipe_type_when_element_matches() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public string $name;
}

class Svc {
    /**
     * @param array<int, Foo> $exclude
     */
    public function run(Foo $item, array $exclude): void {
        if (in_array($item, $exclude, true)) {
            return;
        }
        $name = $item->name;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "in_array guard should not wipe variable type when element type matches, got: {:?}",
        diags
    );
}

/// When the variable is a union type and the haystack element type is
/// one of the union members, the guard clause SHOULD narrow: removing
/// one union member still leaves the other narrowed correctly.
#[test]
fn in_array_guard_still_narrows_union_type() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Foo {
    public string $fooName;
}
class Bar {
    public string $barName;
}

class Svc {
    /**
     * @param Foo|Bar $item
     * @param array<int, Foo> $fooList
     */
    public function run(object $item, array $fooList): void {
        if (in_array($item, $fooList, true)) {
            return;
        }
        $name = $item->barName;
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "in_array guard should still narrow union types, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Unresolvable instanceof target suppression
// ═══════════════════════════════════════════════════════════════════════════

/// When the instanceof target class cannot be resolved (e.g. it lives
/// in a phar), the ternary then-branch should not produce false-positive
/// diagnostics for members that only exist on the unresolvable subclass.
#[test]
fn no_diagnostic_when_instanceof_target_unresolvable_ternary() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Type {
    public function describe(): string;
}

class Test {
    /** @param Type $argType */
    public function run(Type $argType): void {
        $types = $argType instanceof UnionType ? $argType->getTypes() : [$argType];
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics when instanceof target is unresolvable (ternary), got: {:?}",
        diags
    );
}

/// Same scenario but with an if-body instead of a ternary.
#[test]
fn no_diagnostic_when_instanceof_target_unresolvable_if_body() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Type {
    public function describe(): string;
}

class Test {
    public function run(Type $argType): void {
        if ($argType instanceof UnionType) {
            $argType->getTypes();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics when instanceof target is unresolvable (if-body), got: {:?}",
        diags
    );
}

/// Same scenario but with `assert($var instanceof ...)`.
#[test]
fn no_diagnostic_when_instanceof_target_unresolvable_assert() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Type {
    public function describe(): string;
}

class Test {
    public function run(Type $argType): void {
        assert($argType instanceof UnionType);
        $argType->getTypes();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics when instanceof target is unresolvable (assert), got: {:?}",
        diags
    );
}

/// Same scenario but with inline `&&` narrowing.
#[test]
fn no_diagnostic_when_instanceof_target_unresolvable_and_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface Type {
    public function describe(): string;
}

function test(Type $t): void {
    $t instanceof UnionType && $t->getTypes();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics when instanceof target is unresolvable (&& chain), got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Regression: variable from a nullsafe method chain must still resolve
// ═══════════════════════════════════════════════════════════════════════════

/// A variable assigned from a method call chain must resolve correctly
/// for diagnostics.  This catches regressions where the diagnostic
/// outcome path diverges from completion/hover and incorrectly reports
/// the variable as untyped.
#[test]
fn no_unresolved_for_variable_assigned_from_method_chain() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class DebtCollection {
    public function isResolved(): bool { return false; }
}

class Order {
    public function getDebtCollection(): ?DebtCollection { return null; }
}

class Period {
    public function getOrder(): ?Order { return null; }
}

class Test {
    public function run(Period $period): void {
        $debt = $period->getOrder()?->getDebtCollection();
        if ($debt) {
            $debt->isResolved();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for variable assigned from method chain, got: {:?}",
        diags
    );
}

/// `$results[$i]->activities[$id]->extras` where `$results` is
/// `array<int, WeeklyResultDto>` and the property chain walks through
/// typed properties with array access in between.
#[test]
fn no_diagnostic_for_interleaved_array_access_property_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class ExtraPointsDto {
    public string $label;
}

class ActivityResultDto {
    /** @var list<ExtraPointsDto> */
    public array $extras = [];
    public int $activityId;
}

class WeeklyResultDto {
    /** @var array<int, ActivityResultDto> */
    public array $activities;
    public int $week;
}

function test(): void {
    /** @var array<int, WeeklyResultDto> */
    $results = [];

    $results[0]->activities[1]->extras[] = new ExtraPointsDto();
    $results[0]->activities[1]->activityId;
    $results[0]->week;
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for interleaved array-access property chain, got: {:?}",
        diags
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Property narrowing via guard clauses (property subject, not variable)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_false_positive_after_negated_instanceof_guard_on_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Dog {
    public function bark(): string { return ''; }
}
class Cat {
    public function purr(): string { return ''; }
}
class Svc {
    private Dog|Cat $pet;
    public function test(): void {
        if ($this->pet instanceof Dog) {
            $this->pet->bark();
        }
        if (!$this->pet instanceof Cat) {
            return;
        }
        $this->pet->purr();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after negated instanceof guard on property, got: {:?}",
        diags
    );
}

#[test]
fn no_false_positive_after_positive_instanceof_guard_on_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Dog {
    public function bark(): string { return ''; }
}
class Cat {
    public function purr(): string { return ''; }
}
class Svc {
    private Dog|Cat $pet;
    public function test(): void {
        if ($this->pet instanceof Cat) {
            return;
        }
        $this->pet->bark();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after positive instanceof guard excludes Cat on property, got: {:?}",
        diags
    );
}

#[test]
fn no_false_positive_after_assert_instanceof_on_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Dog {
    public function bark(): string { return ''; }
}
class Cat {
    public function purr(): string { return ''; }
}
class Svc {
    /** @var Dog|Cat|null */
    public $pet;
    public function test(): void {
        assert($this->pet instanceof Dog);
        $this->pet->bark();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after assert instanceof on property, got: {:?}",
        diags
    );
}

/// When a method has a conditional return type like
/// `($type is class-string<SomeInterface> ? ThenType : ElseType)`, and
/// the argument class does NOT implement `SomeInterface`, the analyser
/// should use the else-branch return type.
#[test]
fn no_false_positive_conditional_return_class_string_bound() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
interface FormInterface {
    public function submit(mixed $data): void;
    public function getData(): mixed;
}
interface FormFlowTypeInterface {}
interface FormFlowInterface {}
abstract class AbstractController {
    /**
     * @return ($type is class-string<FormFlowTypeInterface> ? FormFlowInterface : FormInterface)
     */
    protected function createForm(string $type, mixed $data = null, array $options = []): FormInterface {}
}
class ImageUploadFormType {}
class ImageController extends AbstractController {
    public function store(): void {
        $form = $this->createForm(ImageUploadFormType::class);
        $form->submit([]);
        $form->getData();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let unknown_diags: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.code == Some(NumberOrString::String("unknown_member".to_string()))
                || d.code == Some(NumberOrString::String("scalar_member_access".to_string()))
        })
        .collect();
    assert!(
        unknown_diags.is_empty(),
        "should not flag unknown members on $form when createForm conditional returns FormInterface, got: {:?}",
        unknown_diags
    );
}

/// Cross-file variant: the base class with the conditional return type
/// is in a separate file (simulating a vendor package).  This exercises
/// `conditional_return` being inherited through `resolve_class_fully`
/// when the method is defined in an ancestor loaded via the class
/// loader.
#[test]
fn no_false_positive_conditional_return_class_string_bound_cross_file() {
    let backend = create_test_backend();
    let base_php = r#"<?php
interface FormInterface {
    public function submit(mixed $data): void;
    public function getData(): mixed;
}
interface FormFlowTypeInterface {}
interface FormFlowInterface {}
abstract class AbstractController {
    /**
     * @return ($type is class-string<FormFlowTypeInterface> ? FormFlowInterface : FormInterface)
     */
    protected function createForm(string $type, mixed $data = null, array $options = []): FormInterface {}
}
"#;
    let controller_php = r#"<?php
class ImageUploadFormType {}
class ImageController extends AbstractController {
    public function store(): void {
        $form = $this->createForm(ImageUploadFormType::class);
        $form->submit([]);
        $form->getData();
    }
}
"#;
    // Index the base file first (simulates vendor classes).
    backend.update_ast("file:///base.php", base_php);
    // Then index the controller file.
    backend.update_ast("file:///controller.php", controller_php);

    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(
        "file:///controller.php",
        controller_php,
        &mut diags,
    );
    let unknown_diags: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.code == Some(NumberOrString::String("unknown_member".to_string()))
                || d.code == Some(NumberOrString::String("scalar_member_access".to_string()))
        })
        .collect();
    assert!(
        unknown_diags.is_empty(),
        "cross-file: should not flag unknown members on $form when createForm conditional returns FormInterface, got: {:?}",
        unknown_diags
    );
}

/// When two `array_map` calls in the same method use different closure
/// parameter names that happen to collide (e.g. `$row`), the second
/// closure's parameter type must come from its own type hint, not from
/// the first closure's `@param` docblock.
#[test]
fn no_false_positive_closure_param_scope_leak_between_array_maps() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Activity {
    public int $id = 0;
    public function toResponseObject(): string { return ''; }
}
class Repo {
    /**
     * @return list<array{activity: int, distance: int}>
     */
    public function getStats(): array { return []; }

    /**
     * @return list<Activity>
     */
    public function getActivities(): array { return []; }

    public function run(): void {
        $rows = $this->getStats();

        $ids = \array_map(
            /** @param array{activity: int, distance: int} $row */
            static fn(array $row): int => $row['activity'],
            $rows,
        );

        /** @var list<Activity> */
        $activities = $this->getActivities();

        $result = \array_map(
            static fn(Activity $row): string => $row->toResponseObject(),
            $activities,
        );
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "should not flag scalar_member_access on $row->toResponseObject() in second closure, got: {:?}",
        scalar_diags
    );
    let unknown_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("toResponseObject"))
        .collect();
    assert!(
        unknown_diags.is_empty(),
        "should not flag unknown member 'toResponseObject' on $row in second closure, got: {:?}",
        unknown_diags
    );
}

/// After `if (!$this->model instanceof Order) { return; }`,
/// `$this->model` is narrowed to `Order`.  A foreach over
/// `$this->model->items` should resolve the element type so that member
/// accesses on the loop variable don't fire.
#[test]
fn no_false_positive_foreach_over_narrowed_property_after_guard_clause() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Item {
    public function name(): string { return ''; }
}
class Order {
    /** @return Item[] */
    public function getItems(): array { return []; }
    /** @var Item[] */
    public array $items;
}
class Svc {
    private ?Order $model;
    public function test(): void {
        if (!$this->model instanceof Order) {
            return;
        }
        foreach ($this->model->items as $item) {
            $item->name();
        }
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics after guard clause narrowing on property in foreach, got: {:?}",
        diags
    );
}

#[test]
fn no_diagnostic_for_arbitrary_method_on_soap_client() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
function test(\SoapClient $client): void {
    $client->gettransactionlist(['foo' => 'bar']);
    $client->delete(123);
    $client->capture('abc');
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for arbitrary methods on SoapClient, got: {:?}",
        diags
    );
}

/// When a class extends `SoapClient`, it inherits `__call`, so any
/// method call on it (or on a value returned as `SoapClient`) is valid.
#[test]
fn no_diagnostic_for_arbitrary_method_on_soap_client_subclass() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class MyService extends \SoapClient {
    public function getConnection(): \SoapClient { return $this; }
}
function test(): void {
    $svc = new MyService('http://example.com?wsdl');
    $svc->getConnection()->customMethod();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for arbitrary methods on SoapClient subclass, got: {:?}",
        diags
    );
}

/// `$m->prop = null;` records the property as exactly `null`.  A
/// following not-null assertion (`@phpstan-assert !null`, e.g.
/// PHPUnit's `assertNotNull`) must strip that tracked `null` so the
/// subsequent member access is not flagged as scalar member access on
/// `null`.
#[test]
fn not_null_assert_strips_tracked_null_on_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let text = r#"<?php
class Clock {
    public function toString(): string { return ''; }
}
class Model {
    public ?Clock $at = null;
    public function save(): void {}
}
class Helper {
    /** @phpstan-assert !null $actual */
    public static function assertNotNull(mixed $actual): void {}
}
class Demo {
    public function run(Model $m): void {
        $m->at = null;
        $m->save();
        Helper::assertNotNull($m->at);
        echo $m->at->toString();
    }
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    let scalar_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("scalar_member_access".to_string())))
        .collect();
    assert!(
        scalar_diags.is_empty(),
        "assertNotNull should strip the tracked null, got: {:?}",
        scalar_diags
    );
}

/// A `@var array{First, Second}` annotation on an assignment must let
/// integer-indexed access resolve each positional entry to its own
/// type, so member access on `$pair[0]` / `$pair[1]` verifies against
/// the right class instead of reporting the subject type as unresolved.
#[test]
fn positional_shape_var_annotation_resolves_int_index_element() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let uri = "file:///test.php";
    let text = r#"<?php
class Label {
    public function labelOnly(): void {}
}
class Stmt {
    public function stmtOnly(): void {}
}
class Node {
    /** @return Node[] */
    public function getChildren(): array { return []; }
}
function test(Node $n): void {
    /** @var array{Label, Stmt} $pair */
    $pair = $n->getChildren();
    $pair[0]->labelOnly();
    $pair[1]->stmtOnly();
}
"#;
    let diags = unknown_member_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "positional shape entries should resolve to their own type, got: {:?}",
        diags
    );
}

/// The same positional-shape resolution must work for a multiline
/// `@var array{...}` block with a trailing comma, and each entry must
/// resolve to the correct class (so a method that only exists on the
/// wrong entry is still flagged).
#[test]
fn positional_shape_multiline_var_annotation_resolves_int_index_element() {
    let backend = create_test_backend();
    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }
    let php = r#"<?php
class Label {
    public function labelOnly(): void {}
}
class Stmt {
    public function stmtOnly(): void {}
}
class Node {
    /** @return Node[] */
    public function getChildren(): array { return []; }
}
function test(Node $n): void {
    /**
     * @var array{
     *     Label,
     *     Stmt,
     * } $pair
     */
    $pair = $n->getChildren();
    $pair[0]->labelOnly();
    $pair[1]->stmtOnly();
}
"#;
    let diags = unknown_member_diagnostics(&backend, "file:///test.php", php);
    assert!(
        diags.is_empty(),
        "multiline positional shape entries should resolve, got: {:?}",
        diags
    );

    // A method that only exists on the *other* entry must still be flagged,
    // proving each index resolves to its own distinct type.
    let php_wrong = php.replace("$pair[0]->labelOnly();", "$pair[0]->stmtOnly();");
    let diags_wrong = unknown_member_diagnostics(&backend, "file:///test2.php", &php_wrong);
    assert!(
        diags_wrong
            .iter()
            .any(|d| d.message.contains("stmtOnly") && d.message.contains("Label")),
        "calling Stmt's method on $pair[0] (a Label) should be flagged, got: {:?}",
        diags_wrong
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// self:: / static:: inside macro registration closures
// ═══════════════════════════════════════════════════════════════════════════

/// Inside a closure passed to a `macro()` registration (Laravel
/// `Macroable`, Carbon), the runtime binds the closure with the macro
/// target as its scope, so `self::` and `static::` refer to the target
/// class. Protected members like Carbon's `Mixin::this()` are therefore
/// accessible and must not be flagged.
#[tokio::test]
async fn self_and_static_in_macro_closure_resolve_to_macro_target() {
    let backend = create_test_backend();
    let text = concat!(
        "<?php\n",
        "class DateBase {\n",
        "    protected static function this(): static { return new static(); }\n",
        "    public function diffForHumans(): string { return ''; }\n",
        "    /**\n",
        "     * @param-closure-this static $macro\n",
        "     */\n",
        "    public static function macro(string $name, callable $macro): void {}\n",
        "}\n",
        "class Provider {\n",
        "    public function boot(): void {\n",
        "        DateBase::macro('diffFromYear', function (int $year): string {\n",
        "            return self::this()->diffForHumans()\n",
        "                . static::this()->diffForHumans();\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    let diags = unknown_member_diagnostics(&backend, "file:///test/macro_self.php", text);
    assert!(
        diags.is_empty(),
        "self::/static:: inside a macro closure should resolve to the macro target, got: {:?}",
        diags
    );
}

/// `self::` outside the macro closure still resolves to the lexically
/// enclosing class, so a method that only exists on the macro target is
/// flagged there.
#[tokio::test]
async fn self_outside_macro_closure_stays_lexical() {
    let backend = create_test_backend();
    let text = concat!(
        "<?php\n",
        "class DateBase {\n",
        "    protected static function this(): static { return new static(); }\n",
        "    /**\n",
        "     * @param-closure-this static $macro\n",
        "     */\n",
        "    public static function macro(string $name, callable $macro): void {}\n",
        "}\n",
        "class Provider {\n",
        "    public function boot(): void {\n",
        "        DateBase::macro('noop', function (): void {\n",
        "        });\n",
        "        self::this();\n",
        "    }\n",
        "}\n",
    );

    let diags = unknown_member_diagnostics(&backend, "file:///test/macro_self_leak.php", text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("this") && d.message.contains("Provider")),
        "self::this() outside the closure should be flagged on Provider, got: {:?}",
        diags
    );
}
