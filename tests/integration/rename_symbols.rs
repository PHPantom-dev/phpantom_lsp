//! Rename of free functions and global constants, the imports that name
//! them, and the guards that refuse a rename outright.

use crate::common::{
    apply_edits, create_test_backend, edits_for_uri, line_char_of, open_php, prepare_rename, rename,
};
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

// ─── Non-Renameable Symbols ─────────────────────────────────────────────────

#[tokio::test]
async fn prepare_rename_rejects_this() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {\n",
        "        $this->baz();\n",
        "    }\n",
        "    public function baz(): void {}\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // `$this` should not be renameable.
    let response = prepare_rename(&backend, &uri, 3, 9).await;
    assert!(response.is_none(), "$this should not be renameable");
}

#[tokio::test]
async fn prepare_rename_rejects_self() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public static function create(): self {\n",
        "        return new self();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // `self` keyword on line 3 should not be renameable.
    let response = prepare_rename(&backend, &uri, 3, 20).await;
    assert!(response.is_none(), "self keyword should not be renameable");
}

#[tokio::test]
async fn prepare_rename_rejects_static() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public static function create(): static {\n",
        "        return new static();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 3, 22).await;
    assert!(
        response.is_none(),
        "static keyword should not be renameable"
    );
}

#[tokio::test]
async fn prepare_rename_rejects_parent() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function hello(): void {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    public function hello(): void {\n",
        "        parent::hello();\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let response = prepare_rename(&backend, &uri, 6, 10).await;
    assert!(
        response.is_none(),
        "parent keyword should not be renameable"
    );
}

// ─── Function Rename ────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function helper(): void {}\n",
        "function demo(): void {\n",
        "    helper();\n",
        "    helper();\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 3, 6, "utility").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for function rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    // declaration (L1) + 2 call sites (L3, L4) = at least 3.
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for helper, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "utility");
    }
}

// ─── Constant Rename ────────────────────────────────────────────────────────

#[tokio::test]
async fn rename_class_constant() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Status {\n",
        "    const ACTIVE = 1;\n",
        "}\n",
        "function demo(): void {\n",
        "    echo Status::ACTIVE;\n",
        "    $x = Status::ACTIVE;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 5, 19, "ENABLED").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for constant rename"
    );

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    assert!(
        file_edits.len() >= 3,
        "Expected at least 3 edits for ACTIVE, got {}",
        file_edits.len()
    );

    for te in &file_edits {
        assert_eq!(te.new_text, "ENABLED");
    }
}

// ─── Function imports and aliases ───────────────────────────────────────────

#[tokio::test]
async fn rename_function_rewrites_only_the_last_segment_of_an_import() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/import.php").unwrap();
    let text = "<?php
namespace Foo;

function bar(): void {}

namespace App;

use function Foo\\bar;

bar();
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "function bar(): void");
    let edit = rename(&backend, &uri, line, character + 9, "baz")
        .await
        .expect("a function declaration should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("use function Foo\\baz;"),
        "the import keeps the namespace it names the function under, got: {result}"
    );
    assert!(
        result.contains("function baz(): void"),
        "the declaration takes the new name, got: {result}"
    );
    assert!(
        result.contains("baz();"),
        "an unaliased call takes the new name, got: {result}"
    );
}

#[tokio::test]
async fn rename_function_leaves_an_explicit_alias_alone() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/alias.php").unwrap();
    let text = "<?php
namespace Foo;

function bar(): void {}

namespace App;

use function Foo\\bar as quux;

quux();
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "function bar(): void");
    let edit = rename(&backend, &uri, line, character + 9, "baz")
        .await
        .expect("a function declaration should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("use function Foo\\baz as quux;"),
        "the import target moves to the new name, the alias does not, got: {result}"
    );
    assert!(
        result.contains("quux();"),
        "the alias still names the function, so its call sites must not move, got: {result}"
    );
    assert!(
        !result.contains("baz();"),
        "rewriting the aliased call would break a file that compiles, got: {result}"
    );
}

// ─── Constant imports and declarations ──────────────────────────────────────

#[tokio::test]
async fn rename_global_constant_reaches_its_import_and_uses() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/const.php").unwrap();
    let text = "<?php
namespace Foo;

const BAR = 1;

namespace App;

use const Foo\\BAR;

echo BAR;
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "const BAR = 1;");
    let edit = rename(&backend, &uri, line, character + 6, "QUX")
        .await
        .expect("a global constant declaration should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("const QUX = 1;"),
        "the declaration takes the new name, got: {result}"
    );
    assert!(
        result.contains("use const Foo\\QUX;"),
        "the import keeps the namespace it names the constant under, got: {result}"
    );
    assert!(
        result.contains("echo QUX;"),
        "a use of the constant takes the new name, got: {result}"
    );
}

#[tokio::test]
async fn rename_global_constant_leaves_a_same_named_class_constant_alone() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/const_collision.php").unwrap();
    let text = "<?php
namespace App;

const BAR = 1;

class Holder
{
    public const BAR = 2;
}

echo BAR;
echo Holder::BAR;
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "const BAR = 1;");
    let edit = rename(&backend, &uri, line, character + 6, "QUX")
        .await
        .expect("a global constant declaration should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("const QUX = 1;"),
        "the global declaration takes the new name, got: {result}"
    );
    assert!(
        result.contains("echo QUX;"),
        "the global constant's use takes the new name, got: {result}"
    );
    assert!(
        result.contains("public const BAR = 2;"),
        "an unrelated class constant of the same short name must not rename, got: {result}"
    );
    assert!(
        result.contains("echo Holder::BAR;"),
        "the class constant's own use must not rename, got: {result}"
    );
}

#[tokio::test]
async fn rename_namespaced_constant_leaves_a_sibling_namespace_constant_alone() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/const_namespace_collision.php").unwrap();
    let text = "<?php
namespace A;

const VERSION = '1';

echo VERSION;

namespace B;

const VERSION = '2';

echo VERSION;
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "const VERSION = '1';");
    let edit = rename(&backend, &uri, line, character + 6, "RELEASE")
        .await
        .expect("a namespaced constant declaration should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("namespace A;\n\nconst RELEASE = '1';"),
        "the declaration in A takes the new name, got: {result}"
    );
    assert!(
        result.contains("namespace B;\n\nconst VERSION = '2';"),
        "the unrelated declaration in B must not rename, got: {result}"
    );
    assert!(
        result.contains("echo VERSION;\n"),
        "B's own unqualified use of its own VERSION must not rename, got: {result}"
    );
}

#[tokio::test]
async fn rename_namespaced_function_leaves_a_sibling_namespace_function_alone() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/function_namespace_collision.php").unwrap();
    let text = "<?php
namespace A;

function version(): string { return '1'; }

namespace B;

function version(): string { return '2'; }
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "function version(): string { return '1'; }");
    let edit = rename(&backend, &uri, line, character + 9, "release")
        .await
        .expect("a namespaced function declaration should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("namespace A;\n\nfunction release(): string { return '1'; }"),
        "the declaration in A takes the new name, got: {result}"
    );
    assert!(
        result.contains("namespace B;\n\nfunction version(): string { return '2'; }"),
        "the unrelated declaration in B must not rename, got: {result}"
    );
}

#[tokio::test]
async fn rename_global_constant_reaches_an_unqualified_use_inside_a_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/const_global_fallback.php").unwrap();
    let text = "<?php
const VERSION = '1';

namespace App;

echo VERSION;
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "const VERSION = '1';");
    let edit = rename(&backend, &uri, line, character + 6, "RELEASE")
        .await
        .expect("a global constant declaration should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("const RELEASE = '1';"),
        "the global declaration takes the new name, got: {result}"
    );
    assert!(
        result.contains("echo RELEASE;"),
        "the unqualified use inside App falls back to the global constant, got: {result}"
    );
}

#[tokio::test]
async fn rename_constant_leaves_an_explicit_alias_alone() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/const_alias.php").unwrap();
    let text = "<?php
namespace Foo;

const BAR = 1;

namespace App;

use const Foo\\BAR as QUUX;

echo QUUX;
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "const BAR = 1;");
    let edit = rename(&backend, &uri, line, character + 6, "QUX")
        .await
        .expect("a global constant declaration should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("use const Foo\\QUX as QUUX;"),
        "the import target moves to the new name, the alias does not, got: {result}"
    );
    assert!(
        result.contains("echo QUUX;"),
        "the alias still names the constant, so its uses must not move, got: {result}"
    );
}

#[tokio::test]
async fn constant_references_agree_from_the_import_and_from_a_use() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/const_refs.php").unwrap();
    let text = "<?php
namespace Foo;

const BAR = 1;

namespace App;

use const Foo\\BAR;

echo BAR;
";

    open_php(&backend, &uri, text).await;

    let from_import = {
        let (line, character) = line_char_of(text, "use const Foo\\BAR;");
        rename(&backend, &uri, line, character + 14, "QUX")
            .await
            .map(|e| apply_edits(text, &edits_for_uri(&e, &uri)))
    };
    let from_use = {
        let (line, character) = line_char_of(text, "echo BAR;");
        rename(&backend, &uri, line, character + 5, "QUX")
            .await
            .map(|e| apply_edits(text, &edits_for_uri(&e, &uri)))
    };

    assert_eq!(
        from_import, from_use,
        "the import and a use name one constant, so both must rewrite the same places"
    );
    assert!(
        from_use.is_some_and(|r| r.contains("const QUX = 1;")),
        "either starting point must reach the declaration"
    );
}

#[tokio::test]
async fn rename_from_a_use_rewrites_the_define_that_declares_the_constant() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/define_use.php").unwrap();
    let text = "<?php
define('FOO', 1);

echo FOO;
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "echo FOO;");
    let edit = rename(&backend, &uri, line, character + 5, "BAR")
        .await
        .expect("a use of a define()-declared constant should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("define('BAR', 1);"),
        "the define() call must take the new name or the constant is left undefined, got: {result}"
    );
    assert!(
        result.contains("echo BAR;"),
        "the use takes the new name, got: {result}"
    );
}

#[tokio::test]
async fn rename_from_a_define_call_rewrites_the_constants_uses() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/define_decl.php").unwrap();
    let text = "<?php
define('FOO', 1);

echo FOO;
echo 'FOO';
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "define('FOO', 1);");
    let edit = rename(&backend, &uri, line, character + 8, "BAR")
        .await
        .expect("the name in a define() call should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("define('BAR', 1);"),
        "the declaration takes the new name, got: {result}"
    );
    assert!(
        result.contains("echo BAR;"),
        "a use of the constant takes the new name, got: {result}"
    );
    assert!(
        result.contains("echo 'FOO';"),
        "an unrelated string of the same text must not rename, got: {result}"
    );
}

#[tokio::test]
async fn rename_of_a_define_leaves_a_same_named_class_constant_alone() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/define_collision.php").unwrap();
    let text = "<?php
define('FOO', 1);

class Holder
{
    public const FOO = 2;
}

echo FOO;
echo Holder::FOO;
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "define('FOO', 1);");
    let edit = rename(&backend, &uri, line, character + 8, "BAR")
        .await
        .expect("the name in a define() call should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("define('BAR', 1);"),
        "the declaration takes the new name, got: {result}"
    );
    assert!(
        result.contains("echo BAR;"),
        "the global constant's use takes the new name, got: {result}"
    );
    assert!(
        result.contains("public const FOO = 2;"),
        "an unrelated class constant of the same short name must not rename, got: {result}"
    );
}

#[tokio::test]
async fn rename_of_a_constant_rewrites_defined_and_constant_calls() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/defined_constant_calls.php").unwrap();
    let text = "<?php
define('FOO', 1);

if (defined('FOO')) {
    echo constant('FOO');
}
";

    open_php(&backend, &uri, text).await;

    let (line, character) = line_char_of(text, "define('FOO', 1);");
    let edit = rename(&backend, &uri, line, character + 8, "BAR")
        .await
        .expect("the name in a define() call should rename");
    let result = apply_edits(text, &edits_for_uri(&edit, &uri));

    assert!(
        result.contains("define('BAR', 1);"),
        "the declaration takes the new name, got: {result}"
    );
    assert!(
        result.contains("defined('BAR')"),
        "the defined() guard takes the new name, got: {result}"
    );
    assert!(
        result.contains("constant('BAR')"),
        "the constant() read takes the new name, got: {result}"
    );
}

#[tokio::test]
async fn rename_function_rewrites_a_call_spelled_in_another_case() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///helpers.php").unwrap();
    let uri_b = Url::parse("file:///main.php").unwrap();
    let text_a = concat!(
        "<?php\n",                      // L0
        "function helper(): void {}\n", // L1
    );
    let text_b = concat!(
        "<?php\n",                   // L0
        "namespace App;\n",          // L1
        "function demo(): void {\n", // L2
        "    HELPER();\n",           // L3
        "    helper();\n",           // L4
        "}\n",                       // L5
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    let edit = rename(&backend, &uri_a, 1, 10, "utility")
        .await
        .expect("expected a workspace edit for the function rename");

    // Leaving HELPER() behind would leave the file calling a function
    // that no longer exists.
    let updated = apply_edits(text_b, &edits_for_uri(&edit, &uri_b));
    assert!(
        !updated.contains("HELPER()") && updated.matches("utility()").count() == 2,
        "both spellings should be rewritten:\n{updated}"
    );
}

#[tokio::test]
async fn rename_function_can_start_from_a_fully_qualified_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                     // L0
        "namespace Support;\n",        // L1
        "function shout(): void {}\n", // L2
        "namespace App;\n",            // L3
        "function demo(): void {\n",   // L4
        "    \\Support\\shout();\n",   // L5
        "}\n",                         // L6
    );

    open_php(&backend, &uri, text).await;

    let prepared = prepare_rename(&backend, &uri, 5, 15).await;
    assert!(
        prepared.is_some(),
        "prepare-rename should accept a fully-qualified call site"
    );

    let edit = rename(&backend, &uri, 5, 15, "yell")
        .await
        .expect("expected a workspace edit from the fully-qualified call site");
    let updated = apply_edits(text, &edits_for_uri(&edit, &uri));
    assert!(
        updated.contains("function yell(): void {}") && updated.contains("\\Support\\yell();"),
        "the declaration and the qualified call should both move:\n{updated}"
    );
}

// ─── Cross-file Rename ─────────────────────────────────────────────────────

#[tokio::test]
async fn rename_class_cross_file() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function speak(): string { return ''; }\n",
        "}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "function demo(Animal $a): void {\n",
        "    $obj = new Animal();\n",
        "}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Rename from file a (declaration).
    let edit = rename(&backend, &uri_a, 1, 7, "Creature").await;
    assert!(
        edit.is_some(),
        "Expected a workspace edit for cross-file class rename"
    );

    let edit = edit.unwrap();
    let edits_a = edits_for_uri(&edit, &uri_a);
    let edits_b = edits_for_uri(&edit, &uri_b);

    assert!(
        !edits_a.is_empty(),
        "Expected edits in file a (declaration)"
    );
    assert!(!edits_b.is_empty(), "Expected edits in file b (references)");

    for te in edits_a.iter().chain(edits_b.iter()) {
        assert_eq!(te.new_text, "Creature");
    }
}

#[tokio::test]
async fn a_rename_keeps_an_indented_imports_indentation() {
    // A `use` inside a braced `namespace {}` block is indented, and the
    // import is rewritten as a whole statement rather than name by name
    // (an alias can appear or disappear).  Taking the whole line along
    // with it flattened the import against the left margin.
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "namespace App\\Old {\n",
        "    class Widget {}\n",
        "}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "namespace App\\Consumer {\n",
        "    use App\\Old\\Widget;\n",
        "\n",
        "    function demo(): void { new Widget(); }\n",
        "}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    let (line, character) = line_char_of(text_a, "Widget");
    let edit = rename(&backend, &uri_a, line, character, "Gadget")
        .await
        .expect("expected an edit");

    let result = apply_edits(text_b, &edits_for_uri(&edit, &uri_b));
    assert!(
        result.contains("    use App\\Old\\Gadget;"),
        "the import has to keep its indentation:\n{result}"
    );
}

#[tokio::test]
async fn rename_method_cross_file() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",
        "class Printer {\n",
        "    public function print(): void {}\n",
        "}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $p = new Printer();\n",
        "    $p->print();\n",
        "}\n",
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Rename from the call site in file b.
    let edit = rename(&backend, &uri_b, 3, 9, "output").await;
    assert!(edit.is_some());

    let edit = edit.unwrap();
    let edits_a = edits_for_uri(&edit, &uri_a);
    let edits_b = edits_for_uri(&edit, &uri_b);

    assert!(
        !edits_a.is_empty(),
        "Expected edits in file a (declaration)"
    );
    assert!(!edits_b.is_empty(), "Expected edits in file b (call site)");

    for te in edits_a.iter().chain(edits_b.iter()) {
        assert_eq!(te.new_text, "output");
    }
}

// ─── Whitespace / No Symbol ─────────────────────────────────────────────────

#[tokio::test]
async fn prepare_rename_on_whitespace_returns_none() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "\n", "function demo(): void {}\n",);

    open_php(&backend, &uri, text).await;

    // Line 1 is blank.
    let response = prepare_rename(&backend, &uri, 1, 0).await;
    assert!(response.is_none(), "Expected no rename on whitespace");
}

#[tokio::test]
async fn rename_on_whitespace_returns_none() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "\n", "function demo(): void {}\n",);

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 1, 0, "anything").await;
    assert!(edit.is_none(), "Expected no edit on whitespace");
}

// ─── Result Correctness ─────────────────────────────────────────────────────

#[tokio::test]
async fn rename_variable_produces_valid_php() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $a = 1;\n",
        "    $b = $a + 2;\n",
        "    echo $a;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let edit = rename(&backend, &uri, 2, 5, "$z").await;
    assert!(edit.is_some());

    let file_edits = edits_for_uri(&edit.unwrap(), &uri);
    let result = apply_edits(text, &file_edits);

    // The renamed variable should appear as `$z` everywhere.
    assert!(result.contains("$z = 1;"), "Declaration not renamed");
    assert!(result.contains("$b = $z + 2;"), "RHS usage not renamed");
    assert!(result.contains("echo $z;"), "Echo usage not renamed");
    // And the old name should be gone.
    assert!(!result.contains("$a"), "Old variable name still present");
}

// ─── Stale Symbol Map ───────────────────────────────────────────────────────

/// Replace a file's buffer without refreshing its symbol map.
///
/// This is the state a request lands in between a `didChange` and the
/// background parse that follows it: `open_files` already holds the new
/// text, `symbol_maps` still describes the old.
fn set_buffer_without_reparsing(backend: &Backend, uri: &Url, text: &str) {
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(text.to_string()));
}

/// A map built from older text must never become `TextEdit`s.
///
/// Inserting a line above the symbol shifts every offset after it, so
/// converting them against the newer buffer yields ranges over unrelated
/// code.  Rename is an explicit action, but the edits it returns are
/// applied wholesale, so the response has to be dropped instead.
#[tokio::test]
async fn rename_returns_none_for_stale_symbol_map() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///stale.php").unwrap();
    let before = concat!(
        "<?php\n",
        "function demo($store): string {\n",
        "    $created = $store->get('k');\n",
        "    return is_numeric($created) ? $created : 'no';\n",
        "}\n",
    );
    // The user types a docblock above the assignment.
    let after = before.replace("    $created = $store", "    /** */\n    $created = $store");

    open_php(&backend, &uri, before).await;
    set_buffer_without_reparsing(&backend, &uri, &after);

    let (line, character) = line_char_of(&after, "$created = ");
    assert!(
        rename(&backend, &uri, line, character + 1, "$updated")
            .await
            .is_none(),
        "stale map must not produce edits"
    );
    assert!(
        prepare_rename(&backend, &uri, line, character + 1)
            .await
            .is_none(),
        "stale map must not produce a prepare-rename range"
    );

    // Once the background parse lands, rename works again.
    backend.update_ast(uri.as_str(), &after);
    let edit = rename(&backend, &uri, line, character + 1, "$updated")
        .await
        .expect("fresh map should rename $created");
    let result = apply_edits(&after, &edits_for_uri(&edit, &uri));
    assert!(!result.contains("$created"), "{result}");
}

/// A same-length edit leaves the byte count intact, so the map still looks
/// fresh.  Re-reading the text at each range is what catches it.
#[tokio::test]
async fn rename_returns_none_when_token_text_changed_in_place() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///overtyped.php").unwrap();
    let before = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): string { return 'x'; }\n",
        "}\n",
        "function demo(Widget $w): string { return $w->render(); }\n",
    );
    let after = before.replace("render", "encode");
    assert_eq!(before.len(), after.len());

    open_php(&backend, &uri, before).await;
    set_buffer_without_reparsing(&backend, &uri, &after);

    let (line, character) = line_char_of(&after, "$w->encode");
    assert!(
        rename(&backend, &uri, line, character + 5, "display")
            .await
            .is_none(),
        "overtyped token must not produce edits"
    );
}

/// Cross-file renames read one map per file, so freshness has to be
/// checked against *that* file's text, not the buffer the request arrived
/// on.  Here the file under the cursor is up to date and the file holding
/// the references is not.
#[tokio::test]
async fn rename_returns_none_when_a_referencing_file_is_stale() {
    let backend = create_test_backend();
    let decl_uri = Url::parse("file:///decl.php").unwrap();
    let usage_uri = Url::parse("file:///usage.php").unwrap();

    let decl_text = concat!("<?php\n", "class Widget {}\n");
    let usage_before = concat!(
        "<?php\n",
        "function demo(): Widget { return new Widget(); }\n",
    );
    let usage_after = usage_before.replace("<?php\n", "<?php\n// a new comment line\n");

    open_php(&backend, &decl_uri, decl_text).await;
    open_php(&backend, &usage_uri, usage_before).await;
    set_buffer_without_reparsing(&backend, &usage_uri, &usage_after);

    let (line, character) = line_char_of(decl_text, "class Widget");
    assert!(
        rename(&backend, &decl_uri, line, character + 6, "Gadget")
            .await
            .is_none(),
        "a stale referencing file must drop the whole rename"
    );

    backend.update_ast(usage_uri.as_str(), &usage_after);
    let edit = rename(&backend, &decl_uri, line, character + 6, "Gadget")
        .await
        .expect("fresh maps should rename Widget");
    let result = apply_edits(&usage_after, &edits_for_uri(&edit, &usage_uri));
    assert!(!result.contains("Widget"), "{result}");
}

/// Namespace rename walks every workspace file and rewrites inline FQN
/// references from their symbol-map spans, so one stale file is enough to
/// make the result inconsistent.  A half-renamed namespace does not
/// compile, so the whole edit is dropped.
#[tokio::test]
async fn rename_namespace_returns_none_when_a_file_is_stale() {
    let backend = create_test_backend();
    let decl_uri = Url::parse("file:///ns_decl.php").unwrap();
    let usage_uri = Url::parse("file:///ns_usage.php").unwrap();

    let decl_text = concat!("<?php\n", "namespace App\\Old;\n", "class Foo {}\n");
    let usage_before = concat!(
        "<?php\n",
        "function demo(): \\App\\Old\\Foo { return new \\App\\Old\\Foo(); }\n",
    );
    let usage_after = usage_before.replace("<?php\n", "<?php\n// a new comment line\n");

    open_php(&backend, &decl_uri, decl_text).await;
    open_php(&backend, &usage_uri, usage_before).await;
    set_buffer_without_reparsing(&backend, &usage_uri, &usage_after);

    let (line, character) = line_char_of(decl_text, "namespace App\\Old;");
    assert!(
        rename(&backend, &decl_uri, line, character + 14, "New")
            .await
            .is_none(),
        "a stale file must drop the whole namespace rename"
    );

    backend.update_ast(usage_uri.as_str(), &usage_after);
    let edit = rename(&backend, &decl_uri, line, character + 14, "New")
        .await
        .expect("fresh maps should rename the namespace");
    let result = apply_edits(&usage_after, &edits_for_uri(&edit, &usage_uri));
    assert!(result.contains("App\\New\\Foo"), "{result}");
    assert!(!result.contains("App\\Old"), "{result}");
}
