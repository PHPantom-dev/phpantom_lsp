#[cfg(test)]
mod tests {
    use phpantom_lsp::Backend;
    use tower_lsp::lsp_types::*;

    /// Helper: parse a file and collect unknown-class diagnostics.
    fn collect(backend: &Backend, uri: &str, content: &str) -> Vec<Diagnostic> {
        backend.update_ast(uri, content);
        let mut out = Vec::new();
        backend.collect_unknown_class_diagnostics(uri, content, &mut out);
        out
    }

    /// PHP class names are case-insensitive (B25): `new stdclass()` and
    /// `extends \pdo` refer to the built-in classes and must not be
    /// flagged as unknown.
    #[test]
    fn no_false_positive_for_differently_cased_class() {
        let mut stubs = std::collections::HashMap::new();
        stubs.insert("stdClass", "<?php class stdClass {}");
        let backend = Backend::new_test_with_stubs(stubs);

        let uri = "file:///test.php";
        let content = r#"<?php
$a = new stdclass();
$b = new STDCLASS();
"#;

        let diags = collect(&backend, uri, content);
        assert!(
            diags.is_empty(),
            "Expected no unknown-class diagnostics for differently-cased stdClass, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    /// When a vendor class exists in the classmap,
    /// `collect_unknown_class_diagnostics` must NOT flag it as unknown.
    /// This simulates the IDE scenario where the classmap is loaded
    /// during init and then a file referencing vendor classes is opened.
    #[test]
    fn no_false_positive_for_classmap_vendor_class() {
        let dir = tempfile::tempdir().expect("tempdir");

        // Write a vendor class file that the classmap points to.
        let vendor_class_path = dir.path().join("vendor/filament/src/Panel.php");
        std::fs::create_dir_all(vendor_class_path.parent().unwrap()).unwrap();
        std::fs::write(
            &vendor_class_path,
            r#"<?php
namespace Filament;

class Panel {
    public function default(): static { return $this; }
}
"#,
        )
        .unwrap();

        // Create a backend with workspace root and classmap entry.
        let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
        backend.fqn_uri_index().write().insert(
            "Filament\\Panel".to_string(),
            format!("file://{}", vendor_class_path.display()),
        );

        // Open a file that uses the vendor class via a use-import.
        let uri = "file:///test.php";
        let content = r#"<?php
namespace App\Providers;

use Filament\Panel;

class MyProvider {
    public function panel(Panel $panel): Panel
    {
        return $panel->default();
    }
}
"#;

        let diags = collect(&backend, uri, content);
        let unknown_class_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("not found"))
            .collect();

        assert!(
            unknown_class_diags.is_empty(),
            "Expected no unknown-class diagnostics for classmap vendor classes, got: {:?}",
            unknown_class_diags
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    // ── Basic detection ─────────────────────────────────────────────────

    #[test]
    fn flags_unknown_class_in_new_expression() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nnamespace App;\n\nnew UnknownThing();\n";

        let diags = collect(&backend, uri, content);
        assert!(
            diags.iter().any(|d| d.message.contains("UnknownThing")),
            "expected diagnostic for UnknownThing, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn flags_unknown_class_in_type_hint() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nnamespace App;\n\nfunction foo(MissingClass $x): void {}\n";

        let diags = collect(&backend, uri, content);
        assert!(
            diags.iter().any(|d| d.message.contains("MissingClass")),
            "expected diagnostic for MissingClass, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn flags_unknown_fqn_class() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nnew \\Some\\Missing\\FqnClass();\n";

        let diags = collect(&backend, uri, content);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("Some\\Missing\\FqnClass")),
            "expected diagnostic for FqnClass, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ── No false positives ──────────────────────────────────────────────

    #[test]
    fn no_diagnostic_for_local_class() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nnamespace App;\n\nclass Foo {}\n\nnew Foo();\n";

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("Foo")),
            "should not flag local class Foo, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_imported_class() {
        let backend = Backend::new_test();

        // Register the dependency class in a separate file so that
        // find_or_load_class can resolve it via the fqn_uri_index + uri_classes_index.
        let dep_uri = "file:///vendor/laravel/Request.php";
        let dep_content = "<?php\nnamespace Illuminate\\Http;\n\nclass Request {}\n";
        backend.update_ast(dep_uri, dep_content);
        {
            let mut idx = backend.fqn_uri_index().write();
            idx.insert("Illuminate\\Http\\Request".to_string(), dep_uri.to_string());
        }

        let uri = "file:///test.php";
        let content = "<?php\nnamespace App;\n\nuse Illuminate\\Http\\Request;\n\nnew Request();\n";

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("Request")),
            "should not flag imported class Request, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_self_static_parent() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "class Base {}\n",
            "class Child extends Base {\n",
            "    public function foo(): self { return $this; }\n",
            "    public function bar(): static { return $this; }\n",
            "    public function baz(): void { parent::baz(); }\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| {
                d.message.contains("'self'")
                    || d.message.contains("'static'")
                    || d.message.contains("'parent'")
            }),
            "should not flag self/static/parent, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_stub_class() {
        use std::collections::HashMap;

        let mut stubs = HashMap::new();
        stubs.insert(
            "Exception",
            "<?php\nclass Exception {\n    public function getMessage(): string {}\n}\n",
        );
        let backend = Backend::new_test_with_stubs(stubs);
        let uri = "file:///test.php";
        let content = "<?php\nnew \\Exception();\n";

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("Exception")),
            "should not flag stub class Exception, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_same_namespace_class() {
        let backend = Backend::new_test();
        let uri_dep = "file:///dep.php";
        let content_dep = "<?php\nnamespace App;\n\nclass Helper {}\n";
        backend.update_ast(uri_dep, content_dep);

        // Register in fqn_uri_index so same-namespace lookup works.
        {
            let mut idx = backend.fqn_uri_index().write();
            idx.insert("App\\Helper".to_string(), uri_dep.to_string());
        }

        let uri = "file:///test.php";
        let content = "<?php\nnamespace App;\n\nnew Helper();\n";

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("Helper")),
            "should not flag same-namespace class Helper, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ── Diagnostic metadata ─────────────────────────────────────────────

    #[test]
    fn diagnostic_has_warning_severity() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nnamespace App;\n\nnew Ghost();\n";

        let diags = collect(&backend, uri, content);
        let ghost_diag = diags
            .iter()
            .find(|d| d.message.contains("Ghost"))
            .expect("should have diagnostic for Ghost");
        assert_eq!(ghost_diag.severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn diagnostic_has_code_and_source() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nnamespace App;\n\nnew Ghost();\n";

        let diags = collect(&backend, uri, content);
        let ghost_diag = diags
            .iter()
            .find(|d| d.message.contains("Ghost"))
            .expect("should have diagnostic for Ghost");
        assert_eq!(
            ghost_diag.code,
            Some(NumberOrString::String("unknown_class".to_string()))
        );
        assert_eq!(ghost_diag.source, Some("phpantom".to_string()));
    }

    #[test]
    fn diagnostic_range_covers_class_name() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        // "<?php\nnamespace App;\n\nnew Ghost();\n"
        //  line 3: "new Ghost();"
        //  "new " = 4 chars, "Ghost" starts at col 4, ends at col 9
        let content = "<?php\nnamespace App;\n\nnew Ghost();\n";

        let diags = collect(&backend, uri, content);
        let ghost_diag = diags
            .iter()
            .find(|d| d.message.contains("Ghost"))
            .expect("should have diagnostic for Ghost");

        // The range should be on line 3 and cover "Ghost" (5 chars).
        assert_eq!(ghost_diag.range.start.line, 3);
        assert_eq!(ghost_diag.range.end.line, 3);
        let width = ghost_diag.range.end.character - ghost_diag.range.start.character;
        assert_eq!(width, 5, "range should cover 'Ghost' (5 chars)");
    }

    // ── No diagnostic for global class without namespace ────────────────

    #[test]
    fn no_diagnostic_for_global_class_without_namespace() {
        let backend = Backend::new_test();
        let uri_dep = "file:///dep.php";
        let content_dep = "<?php\nclass GlobalHelper {}\n";
        backend.update_ast(uri_dep, content_dep);

        {
            let mut idx = backend.fqn_uri_index().write();
            idx.insert("GlobalHelper".to_string(), uri_dep.to_string());
        }

        let uri = "file:///test.php";
        let content = "<?php\nnew GlobalHelper();\n";

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("GlobalHelper")),
            "should not flag global class without namespace, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ── Template parameters ─────────────────────────────────────────

    #[test]
    fn no_diagnostic_for_template_parameter() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "/**\n",
            " * @template TValue\n",
            " * @template TKey\n",
            " */\n",
            "class Collection {\n",
            "    /**\n",
            "     * @param callable(TValue, TKey): mixed $callback\n",
            "     * @return TValue\n",
            "     */\n",
            "    public function first(callable $callback): mixed { return null; }\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("TValue")),
            "should not flag @template param TValue, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("TKey")),
            "should not flag @template param TKey, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_method_level_template() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Util {\n",
            "    /**\n",
            "     * @template T\n",
            "     * @param T $value\n",
            "     * @return T\n",
            "     */\n",
            "    public function identity(mixed $value): mixed { return $value; }\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("'T'")),
            "should not flag method-level @template param T, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ── Multiple unknown classes in one file ────────────────────────────

    #[test]
    fn flags_multiple_unknown_classes() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nnamespace App;\n\nnew Alpha();\nnew Beta();\n";

        let diags = collect(&backend, uri, content);
        assert!(
            diags.iter().any(|d| d.message.contains("Alpha")),
            "expected diagnostic for Alpha"
        );
        assert!(
            diags.iter().any(|d| d.message.contains("Beta")),
            "expected diagnostic for Beta"
        );
    }

    // ── Type alias suppression ──────────────────────────────────────

    #[test]
    fn no_diagnostic_for_phpstan_type_alias() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "/**\n",
            " * @phpstan-type UserData array{name: string, email: string}\n",
            " * @phpstan-type StatusInfo array{code: int, label: string}\n",
            " */\n",
            "class TypeAliasDemo {\n",
            "    /** @return UserData */\n",
            "    public function getData(): array { return []; }\n",
            "\n",
            "    /** @return StatusInfo */\n",
            "    public function getStatus(): array { return []; }\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("UserData")),
            "should not flag @phpstan-type alias UserData, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("StatusInfo")),
            "should not flag @phpstan-type alias StatusInfo, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_imported_type_alias() {
        let backend = Backend::new_test();

        // Source class with the alias definition.
        let dep_uri = "file:///dep.php";
        let dep_content = concat!(
            "<?php\n",
            "namespace Lib;\n",
            "\n",
            "/**\n",
            " * @phpstan-type Score int<0, 100>\n",
            " */\n",
            "class Scoring {}\n",
        );
        backend.update_ast(dep_uri, dep_content);
        {
            let mut idx = backend.fqn_uri_index().write();
            idx.insert("Lib\\Scoring".to_string(), dep_uri.to_string());
        }

        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "use Lib\\Scoring;\n",
            "\n",
            "/**\n",
            " * @phpstan-import-type Score from Scoring\n",
            " */\n",
            "class Consumer {\n",
            "    /** @return Score */\n",
            "    public function getScore(): int { return 42; }\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("Score")),
            "should not flag @phpstan-import-type alias Score, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_imported_type_alias_with_two_leading_spaces() {
        // The `@phpstan-import-type` tag is written with two spaces after
        // the asterisk (the style found in real vendor code).  The tag
        // must still register the alias so the `@param` reference to it is
        // not flagged as an unknown class.
        let backend = Backend::new_test();

        let dep_uri = "file:///dep.php";
        let dep_content = concat!(
            "<?php\n",
            "namespace Lib;\n",
            "\n",
            "/**\n",
            " * @phpstan-type Score int<0, 100>\n",
            " */\n",
            "class Scoring {}\n",
        );
        backend.update_ast(dep_uri, dep_content);
        {
            let mut idx = backend.fqn_uri_index().write();
            idx.insert("Lib\\Scoring".to_string(), dep_uri.to_string());
        }

        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "use Lib\\Scoring;\n",
            "\n",
            "/**\n",
            " *  @phpstan-import-type Score from Scoring\n",
            " */\n",
            "class Consumer {\n",
            "    /** @param Score $score */\n",
            "    public function setScore(int $score): void {}\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("Score")),
            "should not flag two-space @phpstan-import-type alias Score, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ── Attribute suppression ───────────────────────────────────────

    #[test]
    fn no_diagnostic_for_attribute_class() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "#[\\JetBrains\\PhpStorm\\Deprecated(reason: 'Use newMethod()', since: '8.1')]\n",
            "function oldFunction(): void {}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("JetBrains")),
            "should not flag attribute class, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_attribute_on_method() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Demo {\n",
            "    #[\\SomeVendor\\CustomAttr]\n",
            "    public function annotated(): void {}\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("SomeVendor") || d.message.contains("CustomAttr")),
            "should not flag attribute on method, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ── Docblock description text suppression ───────────────────────

    #[test]
    fn no_diagnostic_for_tag_in_description_text() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Demo {\n",
            "    /**\n",
            "     * Caught exceptions are filtered out of @throws suggestions.\n",
            "     *\n",
            "     * @throws \\RuntimeException\n",
            "     */\n",
            "    public function risky(): void {}\n",
            "\n",
            "    /**\n",
            "     * Called method's @throws propagate to the caller.\n",
            "     */\n",
            "    public function delegated(): void {}\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("suggestions")),
            "should not flag 'suggestions' from description text, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("propagate")),
            "should not flag 'propagate' from description text, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_emdash_after_tag_in_description() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Demo {\n",
            "    /**\n",
            "     * Broken multi-line @return \u{2014} base `static` is recovered.\n",
            "     */\n",
            "    public function broken(): void {}\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains('\u{2014}')),
            "should not flag em-dash from description text, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_string_literal_in_conditional_return() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Mapper {\n",
            "    /**\n",
            "     * @return ($signature is \"foo\" ? Pen : Marker)\n",
            "     */\n",
            "    public function map(string $signature): Pen|Marker {\n",
            "        return new Pen();\n",
            "    }\n",
            "}\n",
            "class Pen {}\n",
            "class Marker {}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("\"foo\"")),
            "should not flag string literal '\"foo\"' as unknown class, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_single_quoted_literal_in_conditional_return() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Mapper {\n",
            "    /**\n",
            "     * @return ($sig is 'bar' ? Pen : Marker)\n",
            "     */\n",
            "    public function map(string $sig): Pen|Marker {\n",
            "        return new Pen();\n",
            "    }\n",
            "}\n",
            "class Pen {}\n",
            "class Marker {}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("'bar'")),
            "should not flag single-quoted literal as unknown class, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_numeric_literal_in_conditional_return() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Mapper {\n",
            "    /**\n",
            "     * @return ($count is 0 ? EmptyList : FullList)\n",
            "     */\n",
            "    public function get(int $count): EmptyList|FullList {\n",
            "        return new EmptyList();\n",
            "    }\n",
            "}\n",
            "class EmptyList {}\n",
            "class FullList {}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("0")),
            "should not flag numeric literal as unknown class, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_covariant_variance_annotation() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Collection {}\n",
            "class Customer {}\n",
            "class Contact {}\n",
            "\n",
            "class Repo {\n",
            "    /**\n",
            "     * @return Collection<int, covariant array{customer: Customer, contact: Contact|null}>\n",
            "     */\n",
            "    public function getAll(): Collection {\n",
            "        return new Collection();\n",
            "    }\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("covariant")),
            "should not flag 'covariant array' as unknown class, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_contravariant_variance_annotation() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Handler {}\n",
            "\n",
            "class Processor {\n",
            "    /**\n",
            "     * @param Consumer<contravariant Handler> $consumer\n",
            "     */\n",
            "    public function run($consumer): void {}\n",
            "}\n",
            "class Consumer {}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("contravariant")),
            "should not flag 'contravariant Handler' as unknown class, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_false_positive_for_by_reference_param() {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Sorter {\n",
            "    /** @param array<int> &$data */\n",
            "    public function sort(array &$data, string $direction): void {}\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("$data")),
            "by-reference @param &$data must not be flagged as unknown class, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_false_positive_for_namespaced_constant() {
        // Standalone namespaced constant access (e.g. `\PHPStan\PHP_VERSION_ID`)
        // is a ConstantAccess in the parser, not a class reference.  It must
        // not produce an "unknown class" diagnostic.
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App\\Console;\n",
            "\n",
            "function check(): int {\n",
            "    return \\PHPStan\\PHP_VERSION_ID;\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("PHPStan")),
            "namespaced constant \\PHPStan\\PHP_VERSION_ID must not be flagged as unknown class, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_false_positive_for_star_wildcard_in_generic() {
        // PHPStan `*` wildcards in generic positions (e.g.
        // `Relation<TRelatedModel, *, *>`) must not cause the entire
        // type string to be reported as an unknown class.
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Relation {}\n",
            "\n",
            "class Foo {\n",
            "    /**\n",
            "     * @param Relation<string, *, *>|string \\$relation\n",
            "     * @return void\n",
            "     */\n",
            "    public function bar($relation): void {}\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        // The `Relation` class is defined locally — no diagnostic expected.
        // Before the fix, the entire `Relation<string, *, *>|string` was
        // emitted as a single ClassReference and flagged as unknown.
        assert!(
            diags.is_empty(),
            "Star wildcards in generic positions must not cause false unknown_class diagnostics, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ── No-namespace file tests ─────────────────────────────────────────

    #[test]
    fn diagnostic_when_namespaced_class_in_uri_classes_index() {
        // Reproduces issue #59: when `Carbon\Carbon` is already parsed
        // and in the uri_classes_index, `find_or_load_class("Carbon")` must NOT
        // match it — the bare name is a global-scope lookup.  Without
        // the fix the no-namespace fallback at step 3 resolves the bare
        // name to the namespaced class, suppressing the diagnostic.
        let backend = Backend::new_test();

        // Parse the dependency so Carbon\Carbon is in the uri_classes_index.
        let uri_dep = "file:///vendor/carbon.php";
        let content_dep = "<?php\nnamespace Carbon;\n\nclass Carbon {}\n";
        backend.update_ast(uri_dep, content_dep);
        {
            let mut idx = backend.fqn_uri_index().write();
            idx.insert("Carbon\\Carbon".to_string(), uri_dep.to_string());
        }

        let uri = "file:///test.php";
        let content = "<?php\n\nfunction () {\n    return Carbon::now();\n};\n";

        let diags = collect(&backend, uri, content);
        assert!(
            diags.iter().any(|d| d.message.contains("Carbon")),
            "expected unknown-class diagnostic for Carbon even when Carbon\\Carbon is in uri_classes_index, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn diagnostic_for_unknown_class_in_no_namespace_file() {
        // In a file without a namespace, an unresolved class name should
        // still produce a diagnostic.
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\n\nnew Request();\n";

        let diags = collect(&backend, uri, content);
        assert!(
            diags.iter().any(|d| d.message.contains("Request")),
            "expected unknown-class diagnostic for Request in no-namespace file, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn diagnostic_for_unknown_static_class_in_no_namespace_file() {
        // Reproduces issue #59: `Carbon::now()` in a file without a
        // namespace should emit a diagnostic for unresolved `Carbon`.
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\n\nfunction () {\n    return Carbon::now();\n};\n";

        let diags = collect(&backend, uri, content);
        assert!(
            diags.iter().any(|d| d.message.contains("Carbon")),
            "expected unknown-class diagnostic for Carbon in no-namespace file, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_imported_class_in_no_namespace_file() {
        // A `use` statement in a no-namespace file should suppress the
        // diagnostic, just like in a namespaced file.
        let backend = Backend::new_test();

        // Register the class so it can be found.
        let uri_dep = "file:///carbon.php";
        let content_dep = "<?php\nnamespace Carbon;\n\nclass Carbon {}\n";
        backend.update_ast(uri_dep, content_dep);
        {
            let mut idx = backend.fqn_uri_index().write();
            idx.insert("Carbon\\Carbon".to_string(), uri_dep.to_string());
        }

        let uri = "file:///test.php";
        let content =
            "<?php\n\nuse Carbon\\Carbon;\n\nfunction () {\n    return Carbon::now();\n};\n";

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("Carbon")),
            "should not flag imported Carbon class, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_multiline_grouped_imports() {
        let backend = Backend::new_test();

        let uri_dep = "file:///services.php";
        let content_dep = concat!(
            "<?php\n",
            "namespace Project\\CatalogIndex\\Services\\Validation;\n",
            "class ComplementsScenarioValidation {}\n",
            "class ProductByModelValidation {}\n",
        );
        backend.update_ast(uri_dep, content_dep);
        {
            let mut idx = backend.fqn_uri_index().write();
            idx.insert(
                "Project\\CatalogIndex\\Services\\Validation\\ComplementsScenarioValidation"
                    .to_string(),
                uri_dep.to_string(),
            );
            idx.insert(
                "Project\\CatalogIndex\\Services\\Validation\\ProductByModelValidation".to_string(),
                uri_dep.to_string(),
            );
        }

        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace Project\\CatalogIndex\\Controllers;\n\n",
            "use Project\\CatalogIndex\\Services\\Validation\\{\n",
            "    ComplementsScenarioValidation,\n",
            "    ProductByModelValidation\n",
            "};\n\n",
            "class ProductController {\n",
            "    public function test(ComplementsScenarioValidation $a): ProductByModelValidation {\n",
            "        return new ProductByModelValidation();\n",
            "    }\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("ComplementsScenarioValidation")
                    || d.message.contains("ProductByModelValidation")),
            "should not flag classes imported via multiline grouped use, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_diagnostic_for_global_class_via_fqn_uri_index_lazy_load() {
        // A global-namespace class (like Mockery) that is discovered by
        // `scan_autoload_files` and placed in fqn_uri_index — but NOT yet
        // parsed into uri_classes_index — should be lazily loaded via Phase 0 of
        // find_or_load_class and suppress the diagnostic.
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let dep_path = dir.path().join("Mockery.php");
        std::fs::write(&dep_path, "<?php\nclass Mockery {}\n").expect("failed to write temp file");
        let dep_uri = format!("file://{}", dep_path.display());

        let backend = Backend::new_test();

        // Only populate fqn_uri_index (simulating scan_autoload_files).
        // Do NOT call update_ast for the dependency — it must be lazily
        // parsed by find_or_load_class Phase 0.
        {
            let mut idx = backend.fqn_uri_index().write();
            idx.insert("Mockery".to_string(), dep_uri);
        }

        let uri = "file:///test.php";
        let content = concat!(
            "<?php\n",
            "namespace Tests\\Feature;\n",
            "\n",
            "use Mockery;\n",
            "\n",
            "class ApiTest {\n",
            "    public function test(): void {\n",
            "        Mockery::mock();\n",
            "    }\n",
            "}\n",
        );

        let diags = collect(&backend, uri, content);
        assert!(
            !diags.iter().any(|d| d.message.contains("Mockery")),
            "should not flag Mockery resolved via fqn_uri_index lazy load, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
