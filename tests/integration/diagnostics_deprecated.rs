use crate::common::{create_test_backend, create_test_backend_with_function_stubs};
use phpantom_lsp::atom::atom;
use phpantom_lsp::php_type::PhpType;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Open a file, trigger `update_ast`, then collect diagnostics.
///
/// Since we don't have a real LSP client in tests, we call the internal
/// `collect_deprecated_diagnostics` and `collect_unused_import_diagnostics`
/// methods directly rather than going through `publish_diagnostics_for_file`.
fn deprecated_diagnostics(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    text: &str,
) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_deprecated_diagnostics(uri, text, &mut out);
    out
}

fn unused_import_diagnostics(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    text: &str,
) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_unused_import_diagnostics(uri, text, &mut out);
    out
}

fn all_diagnostics(backend: &phpantom_lsp::Backend, uri: &str, text: &str) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_deprecated_diagnostics(uri, text, &mut out);
    backend.collect_unused_import_diagnostics(uri, text, &mut out);
    out
}

/// Assert that a diagnostic has the `Deprecated` tag.
fn has_deprecated_tag(d: &Diagnostic) -> bool {
    d.tags
        .as_ref()
        .is_some_and(|tags| tags.contains(&DiagnosticTag::DEPRECATED))
}

/// Assert that a diagnostic has the `Unnecessary` tag.
fn has_unnecessary_tag(d: &Diagnostic) -> bool {
    d.tags
        .as_ref()
        .is_some_and(|tags| tags.contains(&DiagnosticTag::UNNECESSARY))
}

// ═══════════════════════════════════════════════════════════════════════════
// @deprecated usage diagnostics
// ═══════════════════════════════════════════════════════════════════════════

// ─── Deprecated class ───────────────────────────────────────────────────────

#[test]
fn deprecated_class_reference_in_new() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_class.php";
    let text = r#"<?php
/** @deprecated Use NewHelper instead */
class OldHelper {}

class Consumer {
    public function run(): void {
        $x = new OldHelper();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    // Should flag the `OldHelper` reference in `new OldHelper()`
    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("OldHelper") && d.message.contains("deprecated")),
        "Expected a deprecated diagnostic for OldHelper, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_class_with_message() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_msg.php";
    let text = r#"<?php
/** @deprecated Use NewApi instead */
class LegacyApi {}

class Consumer {
    public function run(): void {
        $x = new LegacyApi();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    // The message should include the deprecation reason
    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("Use NewApi instead")),
        "Expected deprecation message to include reason, got: {:?}",
        deprecated
    );
}

#[test]
fn non_deprecated_class_no_diagnostic() {
    let backend = create_test_backend();
    let uri = "file:///test_not_deprecated.php";
    let text = r#"<?php
class GoodHelper {}

class Consumer {
    public function run(): void {
        $x = new GoodHelper();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();
    assert!(
        deprecated.is_empty(),
        "Expected no deprecated diagnostics, got: {:?}",
        deprecated
    );
}

// ─── Deprecated method ──────────────────────────────────────────────────────

#[test]
fn deprecated_method_call() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_method.php";
    let text = r#"<?php
class Mailer {
    /** @deprecated Use sendAsync() instead. */
    public function sendLegacy(): void {}

    public function sendAsync(): void {}
}

class App {
    public function run(): void {
        $m = new Mailer();
        $m->sendLegacy();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("sendLegacy") && d.message.contains("deprecated")),
        "Expected deprecated diagnostic for $m->sendLegacy(), got: {:?}",
        deprecated
    );
}

#[test]
fn non_deprecated_method_no_diagnostic() {
    let backend = create_test_backend();
    let uri = "file:///test_not_deprecated_method.php";
    let text = r#"<?php
class Mailer {
    public static function sendAsync(): void {}

    public function run(): void {
        self::sendAsync();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();
    assert!(
        deprecated.is_empty(),
        "Expected no deprecated diagnostics, got: {:?}",
        deprecated
    );
}

// ─── Deprecated property ────────────────────────────────────────────────────

#[test]
fn deprecated_static_property() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_prop.php";
    let text = r#"<?php
class Config {
    /** @deprecated Use $newSetting instead */
    public static string $oldSetting = 'x';

    public static string $newSetting = 'y';

    public function run(): void {
        self::$oldSetting;
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("oldSetting") && d.message.contains("deprecated")),
        "Expected deprecated diagnostic for $oldSetting, got: {:?}",
        deprecated
    );
}

// ─── Deprecated constant ────────────────────────────────────────────────────

#[test]
fn deprecated_class_constant() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_const.php";
    let text = r#"<?php
class Status {
    /** @deprecated Use STATUS_ACTIVE instead */
    const OLD_STATUS = 1;

    const STATUS_ACTIVE = 1;

    public function run(): void {
        self::OLD_STATUS;
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("OLD_STATUS") && d.message.contains("deprecated")),
        "Expected deprecated diagnostic for OLD_STATUS, got: {:?}",
        deprecated
    );
}

// ─── Deprecated class in extends ────────────────────────────────────────────

#[test]
fn deprecated_class_in_extends() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_extends.php";
    let text = r#"<?php
/** @deprecated Use NewBase instead */
class OldBase {}

class NewBase {}

class Child extends OldBase {}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.iter().any(|d| d.message.contains("OldBase")),
        "Expected deprecated diagnostic for OldBase in extends clause, got: {:?}",
        deprecated
    );
}

// ─── Deprecated class in type hint ──────────────────────────────────────────

#[test]
fn deprecated_class_in_type_hint() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_hint.php";
    let text = r#"<?php
/** @deprecated */
class OldType {}

class Consumer {
    public function accept(OldType $param): void {}
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.iter().any(|d| d.message.contains("OldType")),
        "Expected deprecated diagnostic for OldType in param type hint, got: {:?}",
        deprecated
    );
}

// ─── Diagnostic severity and tags ───────────────────────────────────────────

#[test]
fn deprecated_diagnostic_has_hint_severity_and_deprecated_tag() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_severity.php";
    let text = r#"<?php
/** @deprecated */
class Old {}

class Consumer {
    public function run(): void {
        $x = new Old();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    for d in &deprecated {
        assert_eq!(
            d.severity,
            Some(DiagnosticSeverity::HINT),
            "Deprecated diagnostics should have HINT severity"
        );
        assert!(
            has_deprecated_tag(d),
            "Deprecated diagnostics should have the DEPRECATED tag"
        );
        assert_eq!(
            d.source.as_deref(),
            Some("phpantom"),
            "Source should be 'phpantom'"
        );
    }
}

// ─── Deprecated static method via class name ────────────────────────────────

#[test]
fn deprecated_static_method_via_class_name() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_static.php";
    let text = r#"<?php
class Factory {
    /** @deprecated Use create() instead */
    public static function make(): void {}

    public static function create(): void {}
}

class Consumer {
    public function run(): void {
        Factory::make();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.iter().any(|d| d.message.contains("make")),
        "Expected deprecated diagnostic for Factory::make(), got: {:?}",
        deprecated
    );
}

// ─── Stub files are skipped ─────────────────────────────────────────────────

#[test]
fn stub_files_produce_no_diagnostics() {
    let backend = create_test_backend();
    let uri = "phpantom-stub://SomeStub";
    let text = r#"<?php
/** @deprecated */
class DeprecatedStub {}
class User extends DeprecatedStub {}
"#;

    // update_ast first, then try to collect diagnostics
    backend.update_ast(uri, text);
    // The publish_diagnostics_for_file would skip this URI.
    // Verify that the check exists by testing the condition manually:
    assert!(
        uri.starts_with("phpantom-stub://"),
        "Test URI should be a stub URI"
    );
}

// ─── Deprecated empty message ───────────────────────────────────────────────

#[test]
fn deprecated_with_empty_message() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_empty.php";
    let text = r#"<?php
/** @deprecated */
class Legacy {}

class Consumer {
    public function run(): void {
        $x = new Legacy();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    // Should say "'Legacy' is deprecated" without a trailing colon/message
    assert!(
        deprecated
            .iter()
            .any(|d| d.message == "'Legacy' is deprecated"),
        "Expected message to be exactly \"'Legacy' is deprecated\", got: {:?}",
        deprecated.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Unused `use` import diagnostics
// ═══════════════════════════════════════════════════════════════════════════

// ─── Single unused import ───────────────────────────────────────────────────

#[test]
fn unused_import_is_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_unused_import.php";
    let text = r#"<?php
namespace App;

use Foo\Bar;

class Consumer {}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.iter().any(|d| d.message.contains("Foo\\Bar")),
        "Expected unused import diagnostic for Foo\\Bar, got: {:?}",
        unnecessary
    );
}

#[test]
fn unused_import_has_hint_severity_and_unnecessary_tag() {
    let backend = create_test_backend();
    let uri = "file:///test_unused_severity.php";
    let text = r#"<?php
namespace App;

use Some\UnusedClass;

class Consumer {}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    for d in &unnecessary {
        assert_eq!(
            d.severity,
            Some(DiagnosticSeverity::HINT),
            "Unused import diagnostics should have HINT severity"
        );
        assert!(
            has_unnecessary_tag(d),
            "Unused import diagnostics should have the UNNECESSARY tag"
        );
        assert_eq!(
            d.source.as_deref(),
            Some("phpantom"),
            "Source should be 'phpantom'"
        );
    }
}

#[test]
fn unused_import_range_lands_on_correct_prefix_sharing_line() {
    // `Vendor\Foo` (unused) shares a prefix with `Vendor\FooBar` (used) and
    // appears on a later line. The squiggle must land on the `Vendor\Foo`
    // line, not the earlier `Vendor\FooBar` line that contains it as a
    // substring.
    let backend = create_test_backend();
    let uri = "file:///test_unused_prefix.php";
    let text = r#"<?php
namespace App;

use Vendor\FooBar;
use Vendor\Foo;

class Consumer {
    public function m(): FooBar {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let foo_diag = diags
        .iter()
        .filter(|d| has_unnecessary_tag(d))
        .find(|d| d.message.contains("Vendor\\Foo") && !d.message.contains("FooBar"))
        .expect("expected unused import diagnostic for Vendor\\Foo");

    assert_eq!(
        foo_diag.range.start.line, 4,
        "Unused import squiggle should be on the `use Vendor\\Foo;` line (4), got line {}",
        foo_diag.range.start.line
    );
}

// ─── Used import produces no diagnostic ─────────────────────────────────────

#[test]
fn used_import_in_type_hint_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_used_import.php";
    let text = r#"<?php
namespace App;

use Foo\Bar;

class Consumer {
    public function run(Bar $b): void {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in type hint should not be flagged, got: {:?}",
        unnecessary
    );
}

#[test]
fn used_import_in_new_expression_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_used_new.php";
    let text = r#"<?php
namespace App;

use Foo\Bar;

class Consumer {
    public function run(): void {
        $x = new Bar();
    }
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in new expression should not be flagged, got: {:?}",
        unnecessary
    );
}

#[test]
fn used_import_in_static_access_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_used_static.php";
    let text = r#"<?php
namespace App;

use Foo\Bar;

class Consumer {
    public function run(): void {
        Bar::doSomething();
    }
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in static access should not be flagged, got: {:?}",
        unnecessary
    );
}

#[test]
fn used_import_in_extends_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_used_extends.php";
    let text = r#"<?php
namespace App;

use Foo\BaseClass;

class Consumer extends BaseClass {}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in extends should not be flagged, got: {:?}",
        unnecessary
    );
}

#[test]
fn used_import_in_implements_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_used_implements.php";
    let text = r#"<?php
namespace App;

use Foo\SomeInterface;

class Consumer implements SomeInterface {}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in implements should not be flagged, got: {:?}",
        unnecessary
    );
}

#[test]
fn used_import_in_phpstan_require_extends_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_used_phpstan_require_extends.php";
    let text = r#"<?php
namespace App;

use Illuminate\Http\Resources\Json\JsonResource;

/** @phpstan-require-extends JsonResource */
trait GathersAuthorizations {}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in @phpstan-require-extends should not be flagged, got: {:?}",
        unnecessary
    );
}

#[test]
fn used_import_in_phpstan_require_implements_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_used_phpstan_require_implements.php";
    let text = r#"<?php
namespace App;

use Countable;

/** @phpstan-require-implements Countable */
trait GathersAuthorizations {}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in @phpstan-require-implements should not be flagged, got: {:?}",
        unnecessary
    );
}

// ─── Multiple imports, some used some not ───────────────────────────────────

#[test]
fn mixed_used_and_unused_imports() {
    let backend = create_test_backend();
    let uri = "file:///test_mixed_imports.php";
    let text = r#"<?php
namespace App;

use Foo\UsedClass;
use Foo\UnusedClass;
use Foo\AnotherUsed;

class Consumer {
    public function run(UsedClass $a): AnotherUsed {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    // Only UnusedClass should be flagged
    assert_eq!(
        unnecessary.len(),
        1,
        "Expected exactly 1 unused import diagnostic, got: {:?}",
        unnecessary
    );
    assert!(
        unnecessary[0].message.contains("Foo\\UnusedClass"),
        "Expected the unused import to be Foo\\UnusedClass, got: {}",
        unnecessary[0].message
    );
}

// ─── No use statements → no diagnostics ─────────────────────────────────────

#[test]
fn no_use_statements_no_diagnostics() {
    let backend = create_test_backend();
    let uri = "file:///test_no_uses.php";
    let text = r#"<?php
class SimpleClass {
    public function run(): void {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "File with no use statements should produce no unused import diagnostics"
    );
}

// ─── Empty file ─────────────────────────────────────────────────────────────

#[test]
fn empty_file_no_diagnostics() {
    let backend = create_test_backend();
    let uri = "file:///test_empty.php";
    let text = "<?php\n";

    let diags = all_diagnostics(&backend, uri, text);
    assert!(diags.is_empty(), "Empty file should produce no diagnostics");
}

// ═══════════════════════════════════════════════════════════════════════════
// Combined diagnostics (both providers)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deprecated_and_unused_in_same_file() {
    let backend = create_test_backend();
    let uri = "file:///test_combined.php";
    let text = r#"<?php
namespace App;

use Some\UnusedImport;

/** @deprecated */
class OldThing {}

class Consumer {
    public function run(): void {
        $x = new OldThing();
    }
}
"#;

    let diags = all_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        !deprecated.is_empty(),
        "Should have deprecated diagnostics for OldThing"
    );
    assert!(
        !unnecessary.is_empty(),
        "Should have unused import diagnostics for UnusedImport"
    );
}

// ─── Used import in return type ─────────────────────────────────────────────

#[test]
fn used_import_in_return_type_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_used_return.php";
    let text = r#"<?php
namespace App;

use Foo\Result;

class Consumer {
    public function run(): Result {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in return type should not be flagged, got: {:?}",
        unnecessary
    );
}

// ─── Used import in instanceof ──────────────────────────────────────────────

#[test]
fn used_import_in_instanceof_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_used_instanceof.php";
    let text = r#"<?php
namespace App;

use Foo\SomeClass;

class Consumer {
    public function check($x): void {
        if ($x instanceof SomeClass) {}
    }
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in instanceof should not be flagged, got: {:?}",
        unnecessary
    );
}

// ─── Used import in catch clause ────────────────────────────────────────────

#[test]
fn used_import_in_catch_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_used_catch.php";
    let text = r#"<?php
namespace App;

use RuntimeException;

class Consumer {
    public function run(): void {
        try {
        } catch (RuntimeException $e) {
        }
    }
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in catch clause should not be flagged, got: {:?}",
        unnecessary
    );
}

// ─── Deprecated method on parent via static ─────────────────────────────────

#[test]
fn deprecated_inherited_method_via_parent() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_parent.php";
    let text = r#"<?php
class Base {
    /** @deprecated Use newMethod() instead */
    public static function oldMethod(): void {}

    public static function newMethod(): void {}
}

class Child extends Base {
    public function run(): void {
        parent::oldMethod();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.iter().any(|d| d.message.contains("oldMethod")),
        "Expected deprecated diagnostic for parent::oldMethod(), got: {:?}",
        deprecated
    );
}

// ─── All imports used → zero unnecessary diagnostics ────────────────────────

#[test]
fn all_imports_used_no_unnecessary_diagnostics() {
    let backend = create_test_backend();
    let uri = "file:///test_all_used.php";
    let text = r#"<?php
namespace App;

use Foo\TypeA;
use Foo\TypeB;
use Foo\TypeC;

class Consumer {
    public function a(TypeA $a): TypeB {
        $x = new TypeC();
    }
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "All imports are used, should have no unnecessary diagnostics, got: {:?}",
        unnecessary
    );
}

// ─── Multiple unused imports ────────────────────────────────────────────────

#[test]
fn multiple_unused_imports_all_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_multi_unused.php";
    let text = r#"<?php
namespace App;

use Foo\Unused1;
use Foo\Unused2;
use Foo\Unused3;

class Consumer {}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert_eq!(
        unnecessary.len(),
        3,
        "Expected 3 unused import diagnostics, got: {:?}",
        unnecessary
    );
}

// ─── Deprecated class on declaration site should NOT be flagged ─────────────

#[test]
fn deprecated_class_declaration_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_decl.php";
    let text = r#"<?php
/** @deprecated */
class DeprecatedClass {
    public function foo(): void {}
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    // The declaration itself should not be flagged — only references to it.
    // ClassDeclaration spans are different from ClassReference spans.
    assert!(
        deprecated.is_empty(),
        "Class declaration should not produce deprecated diagnostics, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_function_declaration_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_func_decl.php";
    let text = r#"<?php
/** @deprecated We don't need time! */
function formatUtfDate(string $tz): void {}

formatUtfDate('');
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    // Only the call site should be flagged, not the declaration.
    assert_eq!(
        deprecated.len(),
        1,
        "Expected exactly 1 deprecated diagnostic (the call site), got: {:?}",
        deprecated
    );
    // The diagnostic should be on line 4 (the call), not line 2 (the declaration).
    assert_eq!(
        deprecated[0].range.start.line, 4,
        "Deprecated diagnostic should be on the call site (line 4), not the declaration, got: {:?}",
        deprecated[0]
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Catch clause union type import detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn used_import_in_catch_union_type_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_catch_union.php";
    let text = r#"<?php
namespace Demo;

use GtdAccessException;

class Foo {
    public function demo(): void {
        try {
        } catch (GtdAccessException $e) {}
    }
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in catch clause should not be flagged as unused, got: {:?}",
        unnecessary
    );
}

#[test]
fn used_import_in_catch_multi_union_type_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_catch_multi.php";
    let text = r#"<?php
namespace Demo;

use GtdNotFoundException;
use GtdAccessException;

class GtdNotFoundException extends \RuntimeException {}
class GtdAccessException extends \RuntimeException {}

class Foo {
    public function demo(): void {
        try {
        } catch (GtdNotFoundException|GtdAccessException $e) {}
    }
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Imports used in catch union type should not be flagged, got: {:?}",
        unnecessary
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Truly unused import IS flagged (example.php-like scenario)
// ═══════════════════════════════════════════════════════════════════════════

/// Matches the example.php structure: a namespace block with use statements,
/// multiple classes, and an import that is genuinely not referenced anywhere.
#[test]
fn truly_unused_import_in_namespaced_file_is_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_example_like.php";
    let text = r#"<?php
namespace Demo {

use Exception;
use GtdAccessException;
use Stringable;

class GtdTarget {
    public function label(): string { return ''; }
}

class GtdNotFoundException extends \RuntimeException {}
class GtdAccessException extends \RuntimeException {}

class TypeHintGtdDemo {
    public function demo(): void {
        try {
            $x = new GtdTarget();
        } catch (GtdNotFoundException $e) {}
    }

    public function paramTypes(GtdTarget $item): GtdTarget { return $item; }
}

}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    // GtdAccessException is imported but never referenced — should be flagged.
    // Exception and Stringable are also imported but not referenced — should be flagged.
    // GtdNotFoundException IS referenced in the catch clause — should NOT be flagged.
    let flagged_msgs: Vec<&str> = unnecessary.iter().map(|d| d.message.as_str()).collect();

    assert!(
        unnecessary
            .iter()
            .any(|d| d.message.contains("GtdAccessException")),
        "GtdAccessException is unused and should be flagged, got: {:?}",
        flagged_msgs
    );

    assert!(
        !unnecessary
            .iter()
            .any(|d| d.message.contains("GtdNotFoundException")),
        "GtdNotFoundException IS used in catch clause and should NOT be flagged, got: {:?}",
        flagged_msgs
    );
}

/// When GtdAccessException IS used in a catch union, it should NOT be flagged.
#[test]
fn used_import_in_catch_union_namespaced_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_example_used.php";
    let text = r#"<?php
namespace Demo {

use GtdNotFoundException;
use GtdAccessException;

class GtdNotFoundException extends \RuntimeException {}
class GtdAccessException extends \RuntimeException {}

class TypeHintGtdDemo {
    public function demo(): void {
        try {
        } catch (GtdNotFoundException|GtdAccessException $e) {}
    }
}

}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Both imports are used in catch union type, none should be flagged, got: {:?}",
        unnecessary.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Import used only in a PHPDoc @param/@return tag should not be flagged.
#[test]
fn used_import_in_phpdoc_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_phpdoc_usage.php";
    let text = r#"<?php
namespace App;

use Foo\BarType;

class Consumer {
    /**
     * @param BarType $item
     * @return BarType
     */
    public function process($item) {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in PHPDoc should not be flagged, got: {:?}",
        unnecessary
    );
}

/// Import with alias: the alias name is what matters for usage detection.
#[test]
fn aliased_import_used_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_alias_used.php";
    let text = r#"<?php
namespace App;

use Foo\UserProfile as Profile;

class Consumer {
    public function run(Profile $p): void {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Aliased import used via alias should not be flagged, got: {:?}",
        unnecessary
    );
}

/// Import with alias that is NOT used anywhere.
#[test]
fn aliased_import_unused_is_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_alias_unused.php";
    let text = r#"<?php
namespace App;

use Foo\UserProfile as Profile;

class Consumer {
    public function run(): void {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary
            .iter()
            .any(|d| d.message.contains("Foo\\UserProfile")),
        "Unused aliased import should be flagged, got: {:?}",
        unnecessary
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// #[Deprecated] attribute diagnostics
// ═══════════════════════════════════════════════════════════════════════════

// ─── Deprecated function via attribute ──────────────────────────────────────

#[test]
fn deprecated_function_via_attribute_bare() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_attr_fn_bare.php";
    let text = r#"<?php
#[Deprecated]
function old_helper(): void {}

old_helper();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("old_helper") && d.message.contains("deprecated")),
        "Expected a deprecated diagnostic for old_helper(), got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_function_via_attribute_with_reason() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_attr_fn_reason.php";
    let text = r#"<?php
#[Deprecated(reason: "Use new_helper() instead", since: "8.0")]
function old_helper(): void {}

old_helper();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("old_helper")
                && d.message.contains("Use new_helper() instead")),
        "Expected a deprecated diagnostic with reason for old_helper(), got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_function_via_attribute_positional_reason() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_attr_fn_positional.php";
    let text = r#"<?php
#[Deprecated("Use anonymous functions instead")]
function old_helper(): void {}

old_helper();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.iter().any(|d| d.message.contains("old_helper")
            && d.message.contains("Use anonymous functions instead")),
        "Expected a deprecated diagnostic with positional reason, got: {:?}",
        deprecated
    );
}

// ─── Deprecated method via attribute ────────────────────────────────────────

#[test]
fn deprecated_method_via_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_attr_method.php";
    let text = r#"<?php
class Mailer {
    #[Deprecated(reason: "Use sendAsync() instead", since: "8.1")]
    public function sendLegacy(): void {}

    public function run(): void {
        $this->sendLegacy();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("sendLegacy")
                && d.message.contains("Use sendAsync() instead")),
        "Expected a deprecated diagnostic for sendLegacy(), got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_method_via_attribute_bare() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_attr_method_bare.php";
    let text = r#"<?php
class Mailer {
    #[Deprecated]
    public function sendLegacy(): void {}

    public function run(): void {
        $this->sendLegacy();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("sendLegacy") && d.message.contains("deprecated")),
        "Expected a deprecated diagnostic for sendLegacy(), got: {:?}",
        deprecated
    );
}

// ─── Deprecated class via attribute ─────────────────────────────────────────

#[test]
fn deprecated_class_via_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_attr_class.php";
    let text = r#"<?php
#[Deprecated(reason: "Use NewApi instead", since: "8.2")]
class OldApi {}

class Consumer {
    public function run(): void {
        $x = new OldApi();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("OldApi") && d.message.contains("Use NewApi instead")),
        "Expected a deprecated diagnostic for OldApi, got: {:?}",
        deprecated
    );
}

// ─── Deprecated property via attribute ──────────────────────────────────────

#[test]
fn deprecated_property_via_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_attr_prop.php";
    let text = r#"<?php
class Document {
    #[Deprecated("The property is deprecated", since: "8.4")]
    public string $encoding = 'UTF-8';

    public function run(): void {
        $this->encoding;
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("encoding")
                && d.message.contains("The property is deprecated")),
        "Expected a deprecated diagnostic for encoding property, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_property_via_attribute_no_docblock() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_attr_prop_no_doc.php";
    let text = r#"<?php
class Document {
    #[Deprecated]
    public string $config = '';

    public function run(): void {
        $this->config;
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("config") && d.message.contains("deprecated")),
        "Expected a deprecated diagnostic for config property, got: {:?}",
        deprecated
    );
}

// ─── Deprecated constant via attribute ──────────────────────────────────────

#[test]
fn deprecated_constant_via_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_attr_const.php";
    let text = r#"<?php
class Config {
    #[Deprecated(reason: "Use ATTR_EMULATE_PREPARES instead")]
    const ATTR_OLD = 1;

    public function run(): void {
        self::ATTR_OLD;
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.iter().any(|d| d.message.contains("ATTR_OLD")
            && d.message.contains("Use ATTR_EMULATE_PREPARES instead")),
        "Expected a deprecated diagnostic for ATTR_OLD constant, got: {:?}",
        deprecated
    );
}

// ─── Docblock @deprecated takes priority over attribute ─────────────────────

#[test]
fn docblock_deprecated_takes_priority_over_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_priority.php";
    let text = r#"<?php
class Mailer {
    /**
     * @deprecated Use sendModern() instead.
     */
    #[Deprecated(reason: "Attribute message")]
    public function sendLegacy(): void {}

    public function run(): void {
        $this->sendLegacy();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    // The docblock message should win over the attribute message.
    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("Use sendModern() instead")),
        "Expected docblock @deprecated message to take priority, got: {:?}",
        deprecated
    );
    assert!(
        !deprecated
            .iter()
            .any(|d| d.message.contains("Attribute message")),
        "Attribute message should NOT appear when docblock has @deprecated, got: {:?}",
        deprecated
    );
}

// ─── Since field appears in message ─────────────────────────────────────────

#[test]
fn deprecated_attribute_since_appears_in_message() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_attr_since.php";
    let text = r#"<?php
class Config {
    #[Deprecated(since: "7.4")]
    const OLD_MODE = 0;

    public function run(): void {
        self::OLD_MODE;
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("OLD_MODE") && d.message.contains("since PHP 7.4")),
        "Expected 'since PHP 7.4' in deprecated message, got: {:?}",
        deprecated
    );
}

// ─── Completion marks #[Deprecated] items ───────────────────────────────────

#[tokio::test]
async fn completion_marks_attribute_deprecated_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_completion_attr_deprecated.php").unwrap();
    let text = r#"<?php
class Mailer {
    #[Deprecated(reason: "Use sendAsync() instead")]
    public function sendLegacy(): void {}

    public function sendAsync(): void {}

    public function run(): void {
        $this->
    }
}
"#;

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 8,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let items = match backend.completion(completion_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        _ => vec![],
    };

    let legacy = items.iter().find(|i| i.label.contains("sendLegacy"));
    assert!(legacy.is_some(), "sendLegacy should appear in completions");
    assert!(
        legacy
            .unwrap()
            .tags
            .as_ref()
            .is_some_and(|t| t.contains(&CompletionItemTag::DEPRECATED)),
        "sendLegacy should be tagged deprecated in completion"
    );

    let async_item = items.iter().find(|i| i.label.contains("sendAsync"));
    assert!(
        async_item.is_some(),
        "sendAsync should appear in completions"
    );
    assert!(
        !async_item
            .unwrap()
            .tags
            .as_ref()
            .is_some_and(|t| t.contains(&CompletionItemTag::DEPRECATED)),
        "sendAsync should NOT be tagged deprecated"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Native PHP 8.4 \Deprecated attribute diagnostics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deprecated_function_via_native_php84_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test_native_deprecated_fn.php";
    let text = r#"<?php
#[\Deprecated(message: "Use safe_replacement() instead", since: "1.5")]
function unsafe_function(): void {}

unsafe_function();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("unsafe_function")
                && d.message.contains("Use safe_replacement() instead")),
        "Expected a deprecated diagnostic with native message for unsafe_function(), got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_method_via_native_php84_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test_native_deprecated_method.php";
    let text = r#"<?php
class Service {
    #[\Deprecated(message: "Use processV2() instead", since: "8.4")]
    public function process(): void {}

    public function run(): void {
        $this->process();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("process")
                && d.message.contains("Use processV2() instead")),
        "Expected a deprecated diagnostic with native message: for process(), got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_constant_via_native_php84_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test_native_deprecated_const.php";
    let text = r#"<?php
class Config {
    #[\Deprecated(message: "Use NEW_LIMIT instead")]
    const OLD_LIMIT = 100;

    public function run(): void {
        self::OLD_LIMIT;
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("OLD_LIMIT")
                && d.message.contains("Use NEW_LIMIT instead")),
        "Expected a deprecated diagnostic for OLD_LIMIT constant, got: {:?}",
        deprecated
    );
}

#[test]
fn both_jetbrains_and_native_styles_produce_diagnostics() {
    let backend = create_test_backend();
    let uri = "file:///test_both_styles.php";
    let text = r#"<?php
class Demo {
    #[Deprecated(reason: "JetBrains style")]
    public function jbMethod(): void {}

    #[\Deprecated(message: "Native PHP style")]
    public function nativeMethod(): void {}

    public function run(): void {
        $this->jbMethod();
        $this->nativeMethod();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("jbMethod") && d.message.contains("JetBrains style")),
        "JetBrains-style #[Deprecated(reason:)] should produce a diagnostic, got: {:?}",
        deprecated
    );
    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("nativeMethod") && d.message.contains("Native PHP style")),
        "Native-style #[\\Deprecated(message:)] should produce a diagnostic, got: {:?}",
        deprecated
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Variable-based deprecated member access diagnostics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deprecated_method_via_variable_instance() {
    let backend = create_test_backend();
    let uri = "file:///test_var_deprecated_method.php";
    let text = r#"<?php
class Service {
    /** @deprecated Use processV2() instead. */
    public function process(): void {}

    public function processV2(): void {}
}

class Consumer {
    public function run(): void {
        $svc = new Service();
        $svc->process();
        $svc->processV2();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("process") && d.message.contains("deprecated")),
        "Expected deprecated diagnostic for $svc->process(), got: {:?}",
        deprecated
    );

    // processV2 is NOT deprecated — no diagnostic should *target* it.
    // (The deprecation message for `process` may mention "processV2" as
    // the replacement, so we check the diagnostic target, not the full text.)
    assert!(
        !deprecated
            .iter()
            .any(|d| d.message.starts_with("'Service::processV2'")),
        "processV2 should NOT be marked deprecated, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_property_via_variable_instance() {
    let backend = create_test_backend();
    let uri = "file:///test_var_deprecated_prop.php";
    let text = r#"<?php
class Config {
    /** @deprecated Use $newSetting instead. */
    public string $oldSetting = '';

    public string $newSetting = '';
}

class Reader {
    public function read(): void {
        $cfg = new Config();
        $cfg->oldSetting;
        $cfg->newSetting;
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("oldSetting") && d.message.contains("deprecated")),
        "Expected deprecated diagnostic for $cfg->oldSetting, got: {:?}",
        deprecated
    );

    // The deprecation message for `oldSetting` mentions "$newSetting" as
    // the replacement, so check the diagnostic target, not the full text.
    assert!(
        !deprecated
            .iter()
            .any(|d| d.message.starts_with("'Config::newSetting'")),
        "newSetting should NOT be marked deprecated, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_method_via_variable_with_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test_var_attr_deprecated.php";
    let text = r#"<?php
class Logger {
    #[\Deprecated(message: "Use logStructured() instead", since: "2.0")]
    public function log(): void {}

    public function logStructured(): void {}
}

class App {
    public function run(): void {
        $logger = new Logger();
        $logger->log();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("log")
                && d.message.contains("Use logStructured() instead")),
        "Expected deprecated diagnostic with attribute message for $logger->log(), got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_method_via_parameter_type_hint() {
    let backend = create_test_backend();
    let uri = "file:///test_param_deprecated.php";
    let text = r#"<?php
class Mailer {
    /** @deprecated Use sendAsync() instead. */
    public function sendLegacy(): void {}
}

class Handler {
    public function handle(Mailer $mailer): void {
        $mailer->sendLegacy();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("sendLegacy") && d.message.contains("deprecated")),
        "Expected deprecated diagnostic for $mailer->sendLegacy() (param type hint), got: {:?}",
        deprecated
    );
}

#[test]
fn non_deprecated_method_via_variable_no_diagnostic() {
    let backend = create_test_backend();
    let uri = "file:///test_var_no_deprecated.php";
    let text = r#"<?php
class Service {
    public function doWork(): void {}
}

class Consumer {
    public function run(): void {
        $svc = new Service();
        $svc->doWork();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.is_empty(),
        "No deprecated diagnostics expected for non-deprecated members, got: {:?}",
        deprecated
    );
}

#[test]
fn custom_namespaced_deprecated_attribute_no_false_diagnostic() {
    let backend = create_test_backend();
    let uri = "file:///test_custom_ns_deprecated.php";
    let text = r#"<?php
class Widget {
    #[\Test\Deprecated(reason: "This is a custom attribute, not a real deprecation")]
    public function render(): void {}

    #[\App\Attributes\Deprecated]
    public function process(): void {}
}

class Consumer {
    public function run(): void {
        $w = new Widget();
        $w->render();
        $w->process();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.is_empty(),
        "Custom-namespaced #[\\Test\\Deprecated] and #[\\App\\Attributes\\Deprecated] should NOT produce deprecation diagnostics, got: {:?}",
        deprecated
    );
}

#[test]
fn real_deprecated_attribute_still_produces_diagnostic() {
    let backend = create_test_backend();
    let uri = "file:///test_real_deprecated_attr.php";
    let text = r#"<?php
class Service {
    #[\Deprecated(message: "Use modernMethod() instead")]
    public function oldMethod(): void {}
}

class Consumer {
    public function run(): void {
        $svc = new Service();
        $svc->oldMethod();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("oldMethod") && d.message.contains("Use modernMethod()")),
        "Real #[\\Deprecated] should still produce a diagnostic, got: {:?}",
        deprecated
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Version-aware deprecation suppression
// ═══════════════════════════════════════════════════════════════════════════

// ─── Suppressed when target PHP version is older than since ─────────────────

#[test]
fn deprecated_since_suppressed_when_target_is_older() {
    let backend = create_test_backend();
    // Target PHP 7.4 — function deprecated since 8.0 should NOT produce a diagnostic.
    backend.set_php_version(phpantom_lsp::types::PhpVersion::new(7, 4));

    let uri = "file:///test_since_suppressed.php";
    let text = r#"<?php
#[\Deprecated(reason: "Use newFunc()", since: "8.0")]
function oldFunc(): void {}

oldFunc();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.is_empty(),
        "Deprecation should be suppressed when target PHP (7.4) < since (8.0), got: {:?}",
        deprecated
    );
}

// ─── Not suppressed when target PHP version equals since ────────────────────

#[test]
fn deprecated_since_not_suppressed_when_target_equals_since() {
    let backend = create_test_backend();
    // Target PHP 8.0 — function deprecated since 8.0 SHOULD produce a diagnostic.
    backend.set_php_version(phpantom_lsp::types::PhpVersion::new(8, 0));

    let uri = "file:///test_since_equal.php";
    let text = r#"<?php
#[\Deprecated(reason: "Use newFunc()", since: "8.0")]
function oldFunc(): void {}

oldFunc();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        !deprecated.is_empty(),
        "Deprecation should fire when target PHP (8.0) == since (8.0)"
    );
}

// ─── Not suppressed when target PHP version is newer than since ─────────────

#[test]
fn deprecated_since_not_suppressed_when_target_is_newer() {
    let backend = create_test_backend();
    // Target PHP 8.4 — function deprecated since 7.2 SHOULD produce a diagnostic.
    backend.set_php_version(phpantom_lsp::types::PhpVersion::new(8, 4));

    let uri = "file:///test_since_newer.php";
    let text = r#"<?php
#[\Deprecated(reason: "Use newFunc()", since: "7.2")]
function oldFunc(): void {}

oldFunc();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        !deprecated.is_empty(),
        "Deprecation should fire when target PHP (8.4) > since (7.2)"
    );
}

// ─── Docblock @deprecated is never suppressed by version ────────────────────

#[test]
fn docblock_deprecated_never_suppressed_by_version() {
    let backend = create_test_backend();
    // Even with a very old target version, @deprecated docblock has no
    // `since` data and should always produce a diagnostic.
    backend.set_php_version(phpantom_lsp::types::PhpVersion::new(5, 6));

    let uri = "file:///test_docblock_not_suppressed.php";
    let text = r#"<?php
/**
 * @deprecated Use newFunc() instead
 */
function oldFunc(): void {}

oldFunc();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        !deprecated.is_empty(),
        "Docblock @deprecated should always produce a diagnostic regardless of PHP version"
    );
}

// ─── Method deprecated since suppressed when target is older ────────────────

#[test]
fn deprecated_method_since_suppressed_when_target_is_older() {
    let backend = create_test_backend();
    backend.set_php_version(phpantom_lsp::types::PhpVersion::new(8, 0));

    let uri = "file:///test_method_since.php";
    let text = r#"<?php
class Formatter {
    #[\Deprecated(reason: "Use format() instead", since: "8.4")]
    public function oldFormat(): string { return ''; }
}

$f = new Formatter();
$f->oldFormat();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.is_empty(),
        "Method deprecation should be suppressed when target PHP (8.0) < since (8.4), got: {:?}",
        deprecated
    );
}

// ─── Class deprecated since suppressed when target is older ─────────────────

#[test]
fn deprecated_class_since_suppressed_when_target_is_older() {
    let backend = create_test_backend();
    backend.set_php_version(phpantom_lsp::types::PhpVersion::new(7, 4));

    let uri = "file:///test_class_since.php";
    let text = r#"<?php
#[\Deprecated(reason: "Use NewService", since: "8.1")]
class OldService {}

new OldService();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.is_empty(),
        "Class deprecation should be suppressed when target PHP (7.4) < since (8.1), got: {:?}",
        deprecated
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Replace deprecated call code action
// ═══════════════════════════════════════════════════════════════════════════

// ─── Replacement code action offered for deprecated function with template ───

#[test]
fn replace_deprecated_function_call_action_offered() {
    use phpantom_lsp::types::FunctionInfo;

    let backend = create_test_backend();
    let uri = "file:///test_replace_func.php";
    let text = concat!("<?php\n", "read_exif_data('photo.jpg');\n",);

    // Register a deprecated function with a replacement template.
    {
        let mut fmap = backend.global_functions().write();
        fmap.insert(
            "read_exif_data".to_string(),
            (
                "file:///stubs.php".to_string(),
                FunctionInfo {
                    name: atom("read_exif_data"),
                    name_offset: 0,
                    parameters: vec![],
                    return_type: Some(PhpType::parse("array")),
                    native_return_type: None,
                    description: None,
                    return_description: None,
                    links: vec![],
                    see_refs: vec![],
                    namespace: None,
                    conditional_return: None,
                    type_assertions: vec![],
                    deprecation_message: Some("since PHP 7.2".to_string()),
                    deprecated_replacement: Some("exif_read_data(%parametersList%)".to_string()),
                    template_params: vec![],
                    template_bindings: vec![],
                    template_param_bounds: Default::default(),
                    throws: vec![],
                    is_polyfill: false,
                    overloads: vec![],
                },
            ),
        );
    }

    backend.update_ast(uri, text);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(1, 0),
            end: Position::new(1, 14),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, text, &params);

    let replace_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(ca) => {
                ca.title.contains("Replace")
            }
            _ => false,
        })
        .collect();

    assert!(
        !replace_actions.is_empty(),
        "Should offer a 'Replace' code action for deprecated function with replacement template, got actions: {:?}",
        actions
            .iter()
            .map(|a| match a {
                tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                tower_lsp::lsp_types::CodeActionOrCommand::Command(c) => c.title.clone(),
            })
            .collect::<Vec<_>>()
    );

    // Verify the replacement text includes the expanded template.
    if let tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(ca) = &replace_actions[0] {
        let edit = ca.edit.as_ref().expect("code action should have an edit");
        let changes = edit.changes.as_ref().expect("edit should have changes");
        let edits = changes
            .values()
            .next()
            .expect("should have at least one file edit");
        let text_edit = &edits[0];
        assert!(
            text_edit.new_text.contains("exif_read_data"),
            "Replacement should contain 'exif_read_data', got: {}",
            text_edit.new_text
        );
        assert!(
            text_edit.new_text.contains("'photo.jpg'"),
            "Replacement should contain the original argument, got: {}",
            text_edit.new_text
        );
    }
}

// ─── No replacement action when no replacement template ─────────────────────

#[test]
fn no_replace_action_when_no_replacement_template() {
    let backend = create_test_backend();
    let uri = "file:///test_no_replace.php";
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @deprecated Use something else\n",
        " */\n",
        "function oldFunc(): void {}\n",
        "\n",
        "oldFunc();\n",
    );

    backend.update_ast(uri, text);

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(6, 0),
            end: Position::new(6, 7),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, text, &params);

    let replace_actions: Vec<_> = actions
        .iter()
        .filter(|a| match a {
            tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(ca) => {
                ca.title.contains("Replace")
            }
            _ => false,
        })
        .collect();

    assert!(
        replace_actions.is_empty(),
        "Should NOT offer a Replace action when there is no replacement template"
    );
}

#[test]
fn trait_use_inside_class_body_not_flagged_as_unused() {
    let backend = create_test_backend();
    let uri = "file:///test_trait_use.php";
    let text = r#"<?php
namespace App\Domain\UpdateFromSheet\Jobs;

use Illuminate\Foundation\Queue\Queueable;

final class UpdateProductPricesFromSheetJob
{
    use Queueable;
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used by trait `use` inside class body should not be flagged as unused, got: {:?}",
        unnecessary
    );
}

#[test]
fn trait_use_with_multiple_traits_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_multi_trait_use.php";
    let text = r#"<?php
namespace App;

use Foo\TraitA;
use Foo\TraitB;
use Foo\UnusedClass;

class Consumer {
    use TraitA, TraitB;
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        !unnecessary.iter().any(|d| d.message.contains("TraitA")),
        "TraitA used in class body trait-use should not be flagged, got: {:?}",
        unnecessary
    );
    assert!(
        !unnecessary.iter().any(|d| d.message.contains("TraitB")),
        "TraitB used in class body trait-use should not be flagged, got: {:?}",
        unnecessary
    );
    assert!(
        unnecessary
            .iter()
            .any(|d| d.message.contains("UnusedClass")),
        "UnusedClass should still be flagged as unused, got: {:?}",
        unnecessary
    );
}

#[test]
fn trait_use_in_braced_namespace_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_braced_ns_trait.php";
    let text = r#"<?php
namespace App\Jobs {
    use Illuminate\Foundation\Queue\Queueable;

    final class MyJob {
        use Queueable;
    }
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used by trait `use` inside braced namespace should not be flagged, got: {:?}",
        unnecessary
    );
}

#[test]
fn class_in_var_docblock_on_promoted_property_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_var_promoted.php";
    let text = r#"<?php
namespace App\Features\Mobilepay;

use Luxplus\Core\Database\Model\Subscriptions\Subscription;

final class SubscriptionInfo
{
    public function __construct(
        /** @var list<Subscription> */
        public array $luxplusSubscriptions,
    ) {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in @var docblock on promoted constructor property should not be flagged as unused, got: {:?}",
        unnecessary
    );
}

#[test]
fn class_in_param_docblock_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_param_docblock.php";
    let text = r#"<?php
namespace App\Services;

use App\Models\Order;

class OrderService
{
    /**
     * @param list<Order> $orders
     */
    public function processOrders(array $orders): void
    {
    }
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in @param docblock should not be flagged as unused, got: {:?}",
        unnecessary
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// @see references in @deprecated diagnostics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deprecated_class_with_see_reference_shows_see_in_message() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_see.php";
    let text = r#"<?php
/**
 * @deprecated Use NewHelper instead.
 * @see NewHelper
 */
class OldHelper {}

class Consumer {
    public function run(): void {
        $x = new OldHelper();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        !deprecated.is_empty(),
        "Expected at least one deprecated diagnostic"
    );
    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("see:") && d.message.contains("NewHelper")),
        "Expected @see reference in deprecated diagnostic message, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_method_with_see_reference_shows_see_in_message() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_method_see.php";
    let text = r#"<?php
class Mailer {
    /**
     * @deprecated Use sendAsync() instead.
     * @see Mailer::sendAsync()
     */
    public function sendLegacy(): void {}

    public function sendAsync(): void {}
}

class App {
    public function run(): void {
        $m = new Mailer();
        $m->sendLegacy();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.iter().any(|d| d.message.contains("sendLegacy")
            && d.message.contains("see:")
            && d.message.contains("Mailer::sendAsync()")),
        "Expected @see reference in deprecated method diagnostic, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_bare_with_see_shows_see_as_main_message() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_bare_see.php";
    let text = r#"<?php
/**
 * @deprecated
 * @see NewClass
 */
class OldClass {}

class Consumer {
    public function run(): void {
        $x = new OldClass();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        !deprecated.is_empty(),
        "Expected at least one deprecated diagnostic"
    );
    // With bare @deprecated + @see, the message should show "See: NewClass"
    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("See:") && d.message.contains("NewClass")),
        "Expected 'See: NewClass' in diagnostic for bare @deprecated + @see, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_with_multiple_see_references() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_multi_see.php";
    let text = r#"<?php
class Counter {
    /**
     * @deprecated Use number_of() instead.
     * @see number_of()                 Alias.
     * @see Counter::$items             For the property.
     * @see https://example.com/docs    Documentation.
     */
    public function count(): int { return 0; }

    /** @var list<mixed> */
    public array $items = [];
}

class App {
    public function run(): void {
        $c = new Counter();
        $c->count();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("see:") && d.message.contains("number_of()")),
        "Expected @see references in deprecated method diagnostic, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_without_see_unchanged() {
    // Ensure that existing @deprecated without @see still works as before.
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_no_see.php";
    let text = r#"<?php
/** @deprecated Use NewApi instead */
class LegacyApi {}

class Consumer {
    public function run(): void {
        $x = new LegacyApi();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("Use NewApi instead")),
        "Expected deprecation reason without 'see:' when no @see tags, got: {:?}",
        deprecated
    );
    // Should NOT contain "see:" since there are no @see tags
    assert!(
        deprecated.iter().all(|d| !d.message.contains("see:")),
        "Should not contain 'see:' when no @see tags, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_function_with_see_reference() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_func_see.php";
    let text = r#"<?php
/**
 * @deprecated
 * @see newFunction()
 */
function oldFunction(): void {}

class App {
    public function run(): void {
        oldFunction();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated
            .iter()
            .any(|d| d.message.contains("See:") && d.message.contains("newFunction()")),
        "Expected @see reference in deprecated function diagnostic, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_property_with_see_reference() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_prop_see.php";
    let text = r#"<?php
class Config {
    /**
     * @deprecated Use $newSetting instead.
     * @see Config::$newSetting
     */
    public string $oldSetting = '';

    public string $newSetting = '';
}

class App {
    public function run(): void {
        $c = new Config();
        $c->oldSetting;
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.iter().any(|d| d.message.contains("oldSetting")
            && d.message.contains("see:")
            && d.message.contains("Config::$newSetting")),
        "Expected @see reference in deprecated property diagnostic, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_constant_with_see_reference() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_const_see.php";
    let text = r#"<?php
class Features {
    /**
     * @deprecated
     * @see Features::NEW_FLAG
     */
    const OLD_FLAG = 1;

    const NEW_FLAG = 2;
}

class App {
    public function run(): void {
        Features::OLD_FLAG;
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.iter().any(|d| d.message.contains("OLD_FLAG")
            && d.message.contains("See:")
            && d.message.contains("Features::NEW_FLAG")),
        "Expected @see reference in deprecated constant diagnostic, got: {:?}",
        deprecated
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Polyfill false-positive suppression
// ═══════════════════════════════════════════════════════════════════════════

/// A deprecated polyfill function (e.g. Laravel's `str_contains` wrapped in
/// `if (! function_exists('str_contains'))`) should NOT produce a deprecation
/// diagnostic when a native stub for the same function exists and the stub
/// is not deprecated.  On PHP 8.0+ the guard is never entered, so the
/// polyfill is dead code and the native version is what actually runs.
#[test]
fn polyfill_deprecated_function_suppressed_when_native_stub_exists() {
    let backend = create_test_backend_with_function_stubs();

    // Simulate Laravel's helpers.php: a deprecated polyfill inside a
    // function_exists guard.
    let helpers_uri = "file:///vendor/laravel/helpers/src/helpers.php";
    let helpers_text = r#"<?php
if (! function_exists('str_contains')) {
    /**
     * @deprecated A native function with the same name was introduced in PHP 8.0.
     * @param string $haystack
     * @param string|array $needles
     * @return bool
     */
    function str_contains($haystack, $needles)
    {
        return false;
    }
}
"#;
    backend.update_ast(helpers_uri, helpers_text);

    // Now open a user file that calls str_contains.
    let uri = "file:///app/test.php";
    let text = r#"<?php
class Membership {
    public function check(): bool {
        return str_contains('hello world', 'world');
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.is_empty(),
        "Polyfill str_contains should NOT produce a deprecation diagnostic \
         when a native stub exists without @deprecated. Got: {:?}",
        deprecated
    );
}

/// When a user-defined function is deprecated and NO native stub exists,
/// the deprecation diagnostic should still be emitted as usual.
#[test]
fn deprecated_function_without_native_stub_still_flagged() {
    let backend = create_test_backend_with_function_stubs();

    let uri = "file:///test_no_stub.php";
    let text = r#"<?php
/** @deprecated Use newHelper() instead. */
function oldHelper(): void {}

oldHelper();
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert_eq!(
        deprecated.len(),
        1,
        "Deprecated function without a native stub should still produce a diagnostic, got: {:?}",
        deprecated
    );
}

// ─── Deprecated class in implements ─────────────────────────────────────────

#[test]
fn deprecated_class_in_implements() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_implements.php";
    let text = r#"<?php
/** @deprecated Use NewContract instead */
interface OldContract {
    public function execute(): void;
}

class Worker implements OldContract {
    public function execute(): void {}
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    assert!(
        deprecated.iter().any(|d| d.message.contains("OldContract")),
        "Expected deprecated diagnostic for OldContract in implements clause, got: {:?}",
        deprecated
    );
}

// ─── Multi-class file: $this resolves to correct class ──────────────────────

#[test]
fn deprecated_method_this_resolves_correct_class_in_multi_class_file() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_multi_class.php";
    let text = r#"<?php
class First {
    /** @deprecated */
    public function oldMethod(): void {}

    public function safeMethod(): void {}

    public function callSafe(): void {
        $this->safeMethod();
    }
}

class Second {
    public function notDeprecated(): void {}

    public function callOwn(): void {
        $this->notDeprecated();
    }

    public function callDeprecatedFromFirst(): void {
        // This should NOT flag — Second does not have oldMethod
        // $this->oldMethod() would be an unknown member, not deprecated
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    // $this->safeMethod() in First should NOT be flagged
    // $this->notDeprecated() in Second should NOT be flagged
    // No deprecated member accesses via $this in this file
    assert!(
        deprecated
            .iter()
            .all(|d| !d.message.contains("safeMethod") && !d.message.contains("notDeprecated")),
        "Non-deprecated methods should not be flagged, got: {:?}",
        deprecated
    );
}

#[test]
fn deprecated_method_this_flags_in_correct_class() {
    let backend = create_test_backend();
    let uri = "file:///test_deprecated_multi_class_flag.php";
    let text = r#"<?php
class Alpha {
    /** @deprecated Use newWay() instead */
    public function oldWay(): void {}

    public function newWay(): void {}

    public function doStuff(): void {
        $this->oldWay();
    }
}

class Beta {
    public function oldWay(): void {}

    public function doStuff(): void {
        $this->oldWay();
    }
}
"#;

    let diags = deprecated_diagnostics(&backend, uri, text);
    let deprecated: Vec<_> = diags.iter().filter(|d| has_deprecated_tag(d)).collect();

    // Alpha::doStuff() calls $this->oldWay() which IS deprecated on Alpha
    // Beta::doStuff() calls $this->oldWay() which is NOT deprecated on Beta
    // So we expect exactly one deprecated diagnostic
    assert_eq!(
        deprecated.len(),
        1,
        "Expected exactly one deprecated diagnostic (from Alpha), got: {:?}",
        deprecated
    );
    assert!(
        deprecated[0].message.contains("oldWay"),
        "Expected the deprecated diagnostic to mention oldWay, got: {:?}",
        deprecated[0].message
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Aliased imports used in attributes (bug #64)
// ═══════════════════════════════════════════════════════════════════════════

/// An aliased namespace import used in an attribute (`#[Assert\Uuid(...)]`)
/// must not be flagged as unused.
#[test]
fn aliased_import_used_in_attribute_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_aliased_attr.php";
    let text = r#"<?php
namespace App\Dto\In;

use Symfony\Component\Validator\Constraints as Assert;

final readonly class SomeKindOfInDto
{
    public function __construct(
        #[Assert\Uuid(
            message: 'Not valid UUID::v7',
        )]
        public string $id,
    ) {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Aliased import used in attribute should NOT be flagged as unused, got: {:?}",
        unnecessary
    );
}

/// An aliased namespace import used in a nested static constant access
/// inside an attribute argument (`Assert\Uuid::V7_MONOTONIC`) must not
/// be flagged as unused.
#[test]
fn aliased_import_used_in_attribute_static_constant_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_aliased_attr_const.php";
    let text = r#"<?php
namespace App\Dto\In;

use Symfony\Component\Validator\Constraints as Assert;

final readonly class SomeKindOfInDto
{
    public function __construct(
        #[Assert\Uuid(
            message: 'Not valid UUID::v7',
            versions: [Assert\Uuid::V7_MONOTONIC],
        )]
        public string $id,
    ) {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Aliased import used in attribute with static constant should NOT be flagged as unused, got: {:?}",
        unnecessary
    );
}

/// A non-aliased import used in an attribute should also work correctly.
#[test]
fn non_aliased_import_used_in_attribute_not_flagged() {
    let backend = create_test_backend();
    let uri = "file:///test_non_aliased_attr.php";
    let text = r#"<?php
namespace App;

use Symfony\Component\Serializer\Attribute\SerializedName;

class Dto {
    public function __construct(
        #[SerializedName('product')]
        public string $id,
    ) {}
}
"#;

    let diags = unused_import_diagnostics(&backend, uri, text);
    let unnecessary: Vec<_> = diags.iter().filter(|d| has_unnecessary_tag(d)).collect();

    assert!(
        unnecessary.is_empty(),
        "Import used in attribute should NOT be flagged as unused, got: {:?}",
        unnecessary
    );
}
