use crate::common::{create_test_backend, create_test_backend_with_full_stubs, open_php};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::{
    Location, PartialResultParams, Position, ReferenceContext, ReferenceParams,
    TextDocumentIdentifier, TextDocumentPositionParams, Url, WorkDoneProgressParams,
};

use std::sync::Arc;

/// Helper: send a find-references request and return the locations.
async fn references_at(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    include_declaration: bool,
) -> Vec<Location> {
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: ReferenceContext {
            include_declaration,
        },
    };

    backend
        .references(params)
        .await
        .unwrap()
        .unwrap_or_default()
}

/// Verify that `format!("file://{}", path.display())` and
/// `Url::from_file_path(path).to_string()` produce the same URI for
/// simple paths.  A mismatch here would cause `ensure_workspace_indexed`
/// to index the same file twice under different URI keys, producing
/// duplicate entries in Find References results.
#[test]
fn uri_format_consistency_simple_path() {
    use tower_lsp::lsp_types::Url;

    let path = std::path::Path::new("/home/user/project/src/Foo.php");
    let from_format = format!("file://{}", path.display());
    let from_url = Url::from_file_path(path).unwrap().to_string();
    eprintln!("format!: {}", from_format);
    eprintln!("Url:     {}", from_url);
    assert_eq!(
        from_format, from_url,
        "URI format mismatch for simple path — this would cause double entries in Find References"
    );
}

/// Paths with spaces: `Url::from_file_path` percent-encodes them but
/// `format!("file://{}",…)` does not.  If any code path uses the raw
/// format for a path containing spaces while another uses the Url type,
/// the same file ends up in `symbol_maps` under two different keys.
#[test]
fn uri_format_consistency_path_with_spaces() {
    use tower_lsp::lsp_types::Url;

    let path = std::path::Path::new("/home/user/My Project/src/Foo.php");
    let from_format = format!("file://{}", path.display());
    let from_url = Url::from_file_path(path).unwrap().to_string();
    eprintln!("format! (spaces): {}", from_format);
    eprintln!("Url     (spaces): {}", from_url);
    // This is expected to DIFFER — Url encodes the space as %20.
    // The point of this test is to document the divergence so that
    // any code producing URIs via format! is aware of the risk.
    if from_format != from_url {
        eprintln!(
            "WARNING: URI mismatch for path with spaces!\n  format!: {}\n  Url:     {}",
            from_format, from_url
        );
    }
    // Url produces percent-encoded form.
    assert!(
        from_url.contains("My%20Project"),
        "Url should percent-encode spaces: {}",
        from_url
    );
    // format! does NOT encode.
    assert!(
        from_format.contains("My Project"),
        "format! should leave spaces as-is: {}",
        from_format
    );
}

/// Paths with special characters that Url percent-encodes.
#[test]
fn uri_format_consistency_path_with_special_chars() {
    use tower_lsp::lsp_types::Url;

    let path = std::path::Path::new("/home/user/project[1]/src/Foo.php");
    let from_format = format!("file://{}", path.display());
    let from_url = Url::from_file_path(path).unwrap().to_string();
    eprintln!("format! (brackets): {}", from_format);
    eprintln!("Url     (brackets): {}", from_url);
    if from_format != from_url {
        eprintln!(
            "WARNING: URI mismatch for path with brackets!\n  format!: {}\n  Url:     {}",
            from_format, from_url
        );
    }
}

/// Paths with hash characters — Url treats `#` as a fragment delimiter.
#[test]
fn uri_format_consistency_path_with_hash() {
    use tower_lsp::lsp_types::Url;

    let path = std::path::Path::new("/home/user/project#2/src/Foo.php");
    let from_format = format!("file://{}", path.display());
    let from_url = Url::from_file_path(path).unwrap().to_string();
    eprintln!("format! (hash): {}", from_format);
    eprintln!("Url     (hash): {}", from_url);
    if from_format != from_url {
        eprintln!(
            "WARNING: URI mismatch for path with hash!\n  format!: {}\n  Url:     {}",
            from_format, from_url
        );
    }
}

/// Helper: open a file in the backend by calling update_ast directly
/// and storing the content in open_files so find_references can read it.
fn open_file(backend: &phpantom_lsp::Backend, uri: &str, content: &str) {
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), Arc::new(content.to_string()));
    backend.update_ast(uri, content);
}

/// Helper to assert no duplicate locations exist in the results.
fn assert_no_duplicates(results: &[tower_lsp::lsp_types::Location], label: &str) {
    let mut seen = std::collections::HashSet::new();
    for loc in results {
        let key = format!(
            "{}:{}:{}:{}:{}",
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character,
        );
        assert!(
            seen.insert(key.clone()),
            "Duplicate reference found ({}): {}",
            label,
            key
        );
    }
}

// ─── Class reference tests ──────────────────────────────────────────────────

#[test]
fn class_references_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_class.php";
    let content = r#"<?php

class Foo {
    public function bar(): void {}
}

class Baz {
    public function test(): void {
        $f = new Foo();
        $g = new Foo();
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `Foo` in the class declaration (line 2, col 6)
    let results = backend
        .find_references(uri, content, Position::new(2, 6), true)
        .expect("should find references");

    assert_no_duplicates(&results, "class_references");

    // We expect exactly 3: the declaration + 2 usages in `new Foo()`
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 declaration + 2 usages), got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn class_references_include_differently_cased_spellings() {
    // PHP resolves class names case-insensitively, so every spelling of
    // `Widget` below is a reference to the same class.
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_class_case.php";
    let content = r#"<?php

class Widget {}

class Uses {
    public function test(): void {
        $a = new WIDGET();
        $b = new Widget();
        $c = new widget();
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `Widget` in the class declaration (line 2, col 6).
    let results = backend
        .find_references(uri, content, Position::new(2, 6), true)
        .expect("should find references");

    assert_no_duplicates(&results, "class_references_case");
    assert_eq!(
        results.len(),
        4,
        "Expected the declaration plus all 3 spellings, got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn constructor_references_include_differently_cased_instantiations() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_ctor_case.php";
    let content = r#"<?php

class Widget {
    public function __construct(int $size) {}
}

class Uses {
    public function test(): void {
        $a = new WIDGET(1);
        $b = new Widget(2);
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `__construct` (line 3, col 20).
    let results = backend
        .find_references(uri, content, Position::new(3, 20), true)
        .expect("should find constructor references");

    assert_no_duplicates(&results, "constructor_references_case");
    assert_eq!(
        results.len(),
        3,
        "Expected the declaration plus both instantiations, got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn class_references_without_declaration_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_class_nodecl.php";
    let content = r#"<?php

class Foo {
    public function bar(): void {}
}

class Baz {
    public function test(): void {
        $f = new Foo();
        $g = new Foo();
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `Foo` in the class declaration (line 2, col 6), include_declaration = false
    let results = backend
        .find_references(uri, content, Position::new(2, 6), false)
        .expect("should find references");

    assert_no_duplicates(&results, "class_references_nodecl");

    // We expect exactly 2 usages in `new Foo()` (no declaration)
    assert_eq!(
        results.len(),
        2,
        "Expected 2 references (usages only), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Global constant reference tests ───────────────────────────────────────

#[test]
fn global_constant_references_include_declaration() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_global_const.php";
    let content = r#"<?php

const FOO = 1;

function test(): int {
    return FOO + FOO;
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `FOO` in the declaration (line 2, col 6)
    let results = backend
        .find_references(uri, content, Position::new(2, 6), true)
        .expect("should find references");

    assert_no_duplicates(&results, "global_constant_references");

    // 1 declaration + 2 usages
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 declaration + 2 usages), got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn global_constant_references_without_declaration_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_global_const_nodecl.php";
    let content = r#"<?php

const FOO = 1;

function test(): int {
    return FOO + FOO;
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `FOO` in the declaration (line 2, col 6), include_declaration = false
    let results = backend
        .find_references(uri, content, Position::new(2, 6), false)
        .expect("should find references");

    assert_no_duplicates(&results, "global_constant_references_nodecl");

    // 2 usages only, no declaration
    assert_eq!(
        results.len(),
        2,
        "Expected 2 references (usages only), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Member access tests ────────────────────────────────────────────────────

#[test]
fn array_callable_method_references() {
    // A method referenced both directly and via an array callable
    // (`[Foo::class, 'bar']`) should be found by find-references.
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_array_callable.php";
    let content = r#"<?php

class Foo {
    public function bar(): void {}
}

Route::get('/', [Foo::class, 'bar']);

$f = new Foo();
$f->bar();
"#;

    open_file(&backend, uri, content);

    // Cursor on `bar` in the method declaration (line 3).
    let results = backend
        .find_references(uri, content, Position::new(3, 21), true)
        .expect("should find references");

    assert_no_duplicates(&results, "array_callable_method_references");
    // 1 declaration + 1 array callable + 1 instance call.
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (declaration + array callable + call), got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn method_references_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_method.php";
    let content = r#"<?php

class Foo {
    public function bar(): void {}
}

class Baz {
    public function test(): void {
        $f = new Foo();
        $f->bar();
        $f->bar();
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `bar` in the method declaration (line 3, col 21)
    let results = backend
        .find_references(uri, content, Position::new(3, 21), true)
        .expect("should find references");

    assert_no_duplicates(&results, "method_references");

    // 1 declaration + 2 call sites
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 declaration + 2 calls), got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn method_references_without_declaration_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_method_nodecl.php";
    let content = r#"<?php

class Foo {
    public function bar(): void {}
}

class Baz {
    public function test(): void {
        $f = new Foo();
        $f->bar();
        $f->bar();
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `bar` in the method declaration (line 3, col 21)
    let results = backend
        .find_references(uri, content, Position::new(3, 21), false)
        .expect("should find references");

    assert_no_duplicates(&results, "method_references_nodecl");

    // 2 call sites only
    assert_eq!(
        results.len(),
        2,
        "Expected 2 references (calls only), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Property references ────────────────────────────────────────────────────

#[test]
fn property_references_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_prop.php";
    let content = r#"<?php

class Foo {
    public string $name = '';

    public function test(): void {
        $this->name = 'hello';
        echo $this->name;
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `name` in the property access (line 6, col 16)
    let results = backend
        .find_references(uri, content, Position::new(6, 16), true)
        .expect("should find references");

    assert_no_duplicates(&results, "property_references");

    // Expect: 1 declaration + 2 accesses
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 declaration + 2 accesses), got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn property_declaration_range_covers_full_name() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_prop_range.php";
    let content = r#"<?php

class Foo {
    public string $name = '';

    public function test(): void {
        echo $this->name;
    }
}
"#;

    open_file(&backend, uri, content);

    let results = backend
        .find_references(uri, content, Position::new(6, 21), true)
        .expect("should find references");

    // The declaration is the reference on the property declaration line.
    let decl = results
        .iter()
        .find(|loc| loc.range.start.line == 3)
        .expect("should include the property declaration");

    // `    public string $name` — the `$` is at column 18 and `$name`
    // spans five UTF-16 columns (18..23).
    assert_eq!(decl.range.start.character, 18, "range should start at `$`");
    assert_eq!(
        decl.range.end.character, 23,
        "range should cover the full `$name`, not `$nam`"
    );
}

#[test]
fn promoted_property_references_cascade() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_promoted_prop.php";
    let content = r#"<?php

class SomeService {
    public function __construct(
        private int $someField,
    ) {}

    public function handle(): int {
        return $this->someField;
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `someField` in the promoted constructor parameter (line 4).
    let results = backend
        .find_references(uri, content, Position::new(4, 22), true)
        .expect("should find references");

    assert_no_duplicates(&results, "promoted_property_references");

    // Expect: 1 declaration (the parameter) + 1 access ($this->someField).
    assert_eq!(
        results.len(),
        2,
        "Expected 2 references (1 declaration + 1 access), got {}: {:#?}",
        results.len(),
        results
    );

    // The access reference must land on the `$this->someField` usage, not
    // just the constructor's own local parameter uses.
    let access = results
        .iter()
        .find(|loc| loc.range.start.line == 8)
        .expect("should include the $this->someField access site");
    assert_eq!(access.range.start.character, 22);
}

// ─── Variable references ────────────────────────────────────────────────────

#[test]
fn variable_references_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_var.php";
    let content = r#"<?php

function test(): void {
    $foo = 1;
    $bar = $foo + 2;
    echo $foo;
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `$foo` at declaration (line 3, col 5)
    let results = backend
        .find_references(uri, content, Position::new(3, 5), true)
        .expect("should find references");

    assert_no_duplicates(&results, "variable_references");

    // 1 definition + 2 usages
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 definition + 2 usages), got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn variable_references_include_dynamic_property_selector() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_dynamic_selector.php";
    let content = r#"<?php

function test(object $message, string $type): void {
    $attribute = strtolower($type);
    if (empty($message->{$attribute})) {
        return;
    }
    echo $attribute;
}
"#;

    open_file(&backend, uri, content);

    let results = backend
        .find_references(uri, content, Position::new(3, 5), true)
        .expect("should find references");

    assert_no_duplicates(&results, "variable_references_dynamic_selector");
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (definition + dynamic selector + echo), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Static member references ───────────────────────────────────────────────

#[test]
fn static_method_references_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_static.php";
    let content = r#"<?php

class Foo {
    public static function create(): self {
        return new self();
    }
}

class Baz {
    public function test(): void {
        Foo::create();
        Foo::create();
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `create` in declaration (line 3, col 28)
    let results = backend
        .find_references(uri, content, Position::new(3, 28), true)
        .expect("should find references");

    assert_no_duplicates(&results, "static_method_references");

    // 1 declaration + 2 call sites
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 declaration + 2 calls), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Class constant references ──────────────────────────────────────────────

#[test]
fn class_constant_references_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_const.php";
    let content = r#"<?php

class Foo {
    const BAR = 42;

    public function test(): void {
        echo self::BAR;
        echo Foo::BAR;
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `BAR` in the constant declaration (line 3, col 10)
    let results = backend
        .find_references(uri, content, Position::new(3, 10), true)
        .expect("should find references");

    assert_no_duplicates(&results, "class_constant_references");

    // 1 declaration + 2 usages
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 declaration + 2 usages), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Function references ────────────────────────────────────────────────────

#[test]
fn function_references_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_func.php";
    let content = r#"<?php

function myHelper(): int {
    return 42;
}

function test(): void {
    $a = myHelper();
    $b = myHelper();
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `myHelper` at declaration (line 2, col 10)
    let results = backend
        .find_references(uri, content, Position::new(2, 10), true)
        .expect("should find references");

    assert_no_duplicates(&results, "function_references");

    // 1 declaration + 2 call sites
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 declaration + 2 calls), got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn function_references_include_aliased_import_usage() {
    let (backend, dir) = crate::common::create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/functions.php",
                r#"<?php
namespace Foo;

function bar(): void {}
"#,
            ),
            (
                "src/client.php",
                r#"<?php
namespace App;

use function Foo\bar as baz;

function run(): void {
    baz();
}
"#,
            ),
        ],
    );

    let functions_path = dir.path().join("src/functions.php");
    let client_path = dir.path().join("src/client.php");

    let functions_uri = format!("file://{}", functions_path.display());
    let client_uri = format!("file://{}", client_path.display());

    let functions_content = std::fs::read_to_string(&functions_path).unwrap();
    let client_content = std::fs::read_to_string(&client_path).unwrap();

    open_file(&backend, &functions_uri, &functions_content);
    open_file(&backend, &client_uri, &client_content);

    let results = backend
        .find_references(
            &functions_uri,
            &functions_content,
            Position::new(3, 9),
            true,
        )
        .expect("should find function references");

    assert_no_duplicates(&results, "function_alias_refs");
    assert!(
        results.iter().any(|loc| loc.uri.as_str() == client_uri),
        "Expected aliased baz() call in client.php, got {:#?}",
        results
    );
}

// ─── $this references ───────────────────────────────────────────────────────

#[test]
fn this_references_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_this.php";
    let content = r#"<?php

class Foo {
    public string $name = '';

    public function test(): void {
        $this->name = 'hello';
        echo $this->name;
        $this->doSomething();
    }

    public function doSomething(): void {}
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `$this` (line 6, col 9)
    let results = backend
        .find_references(uri, content, Position::new(6, 9), true)
        .expect("should find references");

    assert_no_duplicates(&results, "this_references");

    // 3 usages of $this in the method
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references ($this usages), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Cross-file references ──────────────────────────────────────────────────

#[test]
fn cross_file_class_references_no_duplicates() {
    let (backend, _dir) = crate::common::create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Foo.php",
                r#"<?php
namespace App;

class Foo {
    public function bar(): void {}
}
"#,
            ),
            (
                "src/Baz.php",
                r#"<?php
namespace App;

use App\Foo;

class Baz {
    public function test(): void {
        $f = new Foo();
        $f->bar();
    }
}
"#,
            ),
        ],
    );

    let foo_path = _dir.path().join("src/Foo.php");
    let baz_path = _dir.path().join("src/Baz.php");

    let foo_uri = format!("file://{}", foo_path.display());
    let baz_uri = format!("file://{}", baz_path.display());

    let foo_content = std::fs::read_to_string(&foo_path).unwrap();
    let baz_content = std::fs::read_to_string(&baz_path).unwrap();

    open_file(&backend, &foo_uri, &foo_content);
    open_file(&backend, &baz_uri, &baz_content);

    // Cursor on `Foo` in the class declaration (line 3, col 6)
    let results = backend
        .find_references(&foo_uri, &foo_content, Position::new(3, 6), true)
        .expect("should find references");

    assert_no_duplicates(&results, "cross_file_class_references");

    // Should have: declaration in Foo.php + use statement in Baz.php + new Foo() in Baz.php
    // No duplicates allowed
    assert!(
        results.len() >= 2,
        "Expected at least 2 cross-file references, got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn class_references_include_aliased_import_usage() {
    let (backend, dir) = crate::common::create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Models/User.php",
                r#"<?php
namespace App\Models;

class User {}
"#,
            ),
            (
                "src/Controller.php",
                r#"<?php
namespace App;

use App\Models\User as Account;

class Controller {
    public function show(): void {
        $user = new Account();
    }
}
"#,
            ),
        ],
    );

    let user_path = dir.path().join("src/Models/User.php");
    let controller_path = dir.path().join("src/Controller.php");

    let user_uri = format!("file://{}", user_path.display());
    let controller_uri = format!("file://{}", controller_path.display());

    let user_content = std::fs::read_to_string(&user_path).unwrap();
    let controller_content = std::fs::read_to_string(&controller_path).unwrap();

    open_file(&backend, &user_uri, &user_content);
    open_file(&backend, &controller_uri, &controller_content);

    let results = backend
        .find_references(&user_uri, &user_content, Position::new(3, 6), true)
        .expect("should find class references");

    assert_no_duplicates(&results, "class_alias_refs");
    assert!(
        results.iter().any(|loc| loc.uri.as_str() == controller_uri),
        "Expected aliased Account usage in Controller.php, got {:#?}",
        results
    );
}

#[test]
fn cross_file_method_references_no_duplicates() {
    let (backend, _dir) = crate::common::create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Foo.php",
                r#"<?php
namespace App;

class Foo {
    public function bar(): void {}
}
"#,
            ),
            (
                "src/Baz.php",
                r#"<?php
namespace App;

use App\Foo;

class Baz {
    public function test(): void {
        $f = new Foo();
        $f->bar();
    }
}
"#,
            ),
        ],
    );

    let foo_path = _dir.path().join("src/Foo.php");
    let baz_path = _dir.path().join("src/Baz.php");

    let foo_uri = format!("file://{}", foo_path.display());
    let baz_uri = format!("file://{}", baz_path.display());

    let foo_content = std::fs::read_to_string(&foo_path).unwrap();
    let baz_content = std::fs::read_to_string(&baz_path).unwrap();

    open_file(&backend, &foo_uri, &foo_content);
    open_file(&backend, &baz_uri, &baz_content);

    // Cursor on `bar` in the method declaration (line 4, col 21)
    let results = backend
        .find_references(&foo_uri, &foo_content, Position::new(4, 21), true)
        .expect("should find references");

    assert_no_duplicates(&results, "cross_file_method_references");

    // 1 declaration in Foo.php + 1 usage in Baz.php
    assert_eq!(
        results.len(),
        2,
        "Expected 2 references (1 declaration + 1 call), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Cursor on usage site (not declaration) ─────────────────────────────────

#[test]
fn references_from_usage_site_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_usage.php";
    let content = r#"<?php

class Foo {
    public function bar(): void {}
}

class Baz {
    public function test(): void {
        $f = new Foo();
        $f->bar();
        $f->bar();
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `bar` at a call site (line 9, col 13)
    let results = backend
        .find_references(uri, content, Position::new(9, 13), true)
        .expect("should find references");

    assert_no_duplicates(&results, "references_from_usage");

    // 1 declaration + 2 call sites
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 declaration + 2 calls), got {}: {:#?}",
        results.len(),
        results
    );
}

#[test]
fn references_from_new_keyword_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_new.php";
    let content = r#"<?php

class Foo {}

$a = new Foo();
$b = new Foo();
"#;

    open_file(&backend, uri, content);

    // Cursor on `Foo` in `new Foo()` (line 4, col 10)
    let results = backend
        .find_references(uri, content, Position::new(4, 10), true)
        .expect("should find references");

    assert_no_duplicates(&results, "references_from_new");

    // 1 declaration + 2 usages in `new Foo()`
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references, got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Class with type hints ──────────────────────────────────────────────────

#[test]
fn class_references_in_type_hints_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_hints.php";
    let content = r#"<?php

class Foo {}

class Bar {
    public Foo $prop;

    public function take(Foo $param): Foo {
        return $param;
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `Foo` class declaration (line 2, col 6)
    let results = backend
        .find_references(uri, content, Position::new(2, 6), true)
        .expect("should find references");

    assert_no_duplicates(&results, "class_references_in_type_hints");

    // 1 declaration + property type hint + param type hint + return type hint = 4
    assert_eq!(
        results.len(),
        4,
        "Expected 4 references (1 decl + 3 type hints), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Docblock type references ───────────────────────────────────────────────

#[test]
fn class_references_in_docblock_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_docblock.php";
    let content = r#"<?php

class Foo {}

class Bar {
    /**
     * @param Foo $param
     * @return Foo
     */
    public function take(Foo $param): Foo {
        return $param;
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `Foo` class declaration (line 2, col 6)
    let results = backend
        .find_references(uri, content, Position::new(2, 6), true)
        .expect("should find references");

    assert_no_duplicates(&results, "class_references_in_docblock");

    // 1 declaration + 2 docblock refs + 1 param hint + 1 return hint = 5
    // (docblock @param Foo and @return Foo are additional ClassReference spans)
    assert!(
        results.len() >= 3,
        "Expected at least 3 references, got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Inheritance chain references ───────────────────────────────────────────

#[test]
fn class_references_with_extends_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_extends.php";
    let content = r#"<?php

class Base {}

class Child extends Base {}

$b = new Base();
"#;

    open_file(&backend, uri, content);

    // Cursor on `Base` class declaration (line 2, col 6)
    let results = backend
        .find_references(uri, content, Position::new(2, 6), true)
        .expect("should find references");

    assert_no_duplicates(&results, "class_references_with_extends");

    // 1 declaration + `extends Base` + `new Base()` = 3
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 decl + extends + new), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Self/static/parent references ──────────────────────────────────────────

#[test]
fn self_references_no_duplicates() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_self.php";
    let content = r#"<?php

class Foo {
    public static function create(): self {
        return new self();
    }

    public function test(): void {
        $f = self::create();
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `self` keyword (line 3, col 38)
    let results = backend
        .find_references(uri, content, Position::new(3, 38), true)
        .expect("should find references");

    assert_no_duplicates(&results, "self_references");

    // Should find class declaration + self usages + any Foo references
    // Main check: no duplicates
    assert!(
        results.len() >= 2,
        "Expected at least 2 references, got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Debug helper: dump spans for investigation ─────────────────────────────

#[test]
fn debug_dump_symbol_spans_for_simple_class() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_debug_spans.php";
    let content = r#"<?php

class Foo {
    public function bar(): void {}
}

class Baz {
    public function test(): void {
        $f = new Foo();
        $f->bar();
        $f->bar();
    }
}
"#;

    open_file(&backend, uri, content);

    // Test class references
    let results = backend
        .find_references(uri, content, Position::new(2, 6), true)
        .unwrap_or_default();

    eprintln!("=== Class 'Foo' references (include_declaration=true) ===");
    for (i, loc) in results.iter().enumerate() {
        let line = loc.range.start.line;
        let col = loc.range.start.character;
        let end_col = loc.range.end.character;
        let source_line = content.lines().nth(line as usize).unwrap_or("");
        eprintln!(
            "  [{}] {}:{}:{}-{} | {:?}",
            i,
            loc.uri,
            line,
            col,
            end_col,
            source_line.trim()
        );
    }

    assert_no_duplicates(&results, "debug_class_refs");

    // Test method references
    let results = backend
        .find_references(uri, content, Position::new(3, 21), true)
        .unwrap_or_default();

    eprintln!("=== Method 'bar' references (include_declaration=true) ===");
    for (i, loc) in results.iter().enumerate() {
        let line = loc.range.start.line;
        let col = loc.range.start.character;
        let end_col = loc.range.end.character;
        let source_line = content.lines().nth(line as usize).unwrap_or("");
        eprintln!(
            "  [{}] {}:{}:{}-{} | {:?}",
            i,
            loc.uri,
            line,
            col,
            end_col,
            source_line.trim()
        );
    }

    assert_no_duplicates(&results, "debug_method_refs");
}

// ─── Async did_open tests (production path) ─────────────────────────────────
// These tests use the actual `did_open` LSP method to replicate production
// conditions exactly, including any async side effects.

#[tokio::test]
async fn async_did_open_class_references_no_duplicates() {
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::{DidOpenTextDocumentParams, TextDocumentItem, Url};

    let backend = create_test_backend();
    let uri = Url::parse("file:///tmp/test_async_refs.php").unwrap();
    let content = r#"<?php

class Foo {
    public function bar(): void {}
}

class Baz {
    public function test(): void {
        $f = new Foo();
        $f->bar();
        $f->bar();
    }
}
"#;

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    let uri_str = uri.to_string();

    // Class references
    let results = backend
        .find_references(&uri_str, content, Position::new(2, 6), true)
        .expect("should find class references");

    eprintln!("=== Async did_open: 'Foo' class references ===");
    for (i, loc) in results.iter().enumerate() {
        let line = loc.range.start.line;
        let col = loc.range.start.character;
        let src = content.lines().nth(line as usize).unwrap_or("");
        eprintln!("  [{}] L{}:{} | {}", i, line, col, src.trim());
    }
    assert_no_duplicates(&results, "async_class_refs");
    assert_eq!(
        results.len(),
        2,
        "Expected 2 class refs (1 decl + 1 new Foo), got {}: {:#?}",
        results.len(),
        results
    );

    // Method references
    let results = backend
        .find_references(&uri_str, content, Position::new(3, 21), true)
        .expect("should find method references");

    eprintln!("=== Async did_open: 'bar' method references ===");
    for (i, loc) in results.iter().enumerate() {
        let line = loc.range.start.line;
        let col = loc.range.start.character;
        let src = content.lines().nth(line as usize).unwrap_or("");
        eprintln!("  [{}] L{}:{} | {}", i, line, col, src.trim());
    }
    assert_no_duplicates(&results, "async_method_refs");
    assert_eq!(
        results.len(),
        3,
        "Expected 3 method refs (1 decl + 2 calls), got {}: {:#?}",
        results.len(),
        results
    );
}

#[tokio::test]
async fn async_did_open_cross_file_no_duplicates() {
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::{DidOpenTextDocumentParams, TextDocumentItem, Url};

    let (backend, dir) = crate::common::create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Product.php",
                r#"<?php
namespace App;

class Product {
    public function price(): int { return 0; }
}
"#,
            ),
            (
                "src/Basket.php",
                r#"<?php
namespace App;

use App\Product;

class Basket {
    public function addProduct(Product $p): void {
        $item = new Product();
        $item->price();
    }
}
"#,
            ),
        ],
    );

    let product_path = dir.path().join("src/Product.php");
    let basket_path = dir.path().join("src/Basket.php");

    let product_uri = Url::from_file_path(&product_path).unwrap();
    let basket_uri = Url::from_file_path(&basket_path).unwrap();

    let product_content = std::fs::read_to_string(&product_path).unwrap();
    let basket_content = std::fs::read_to_string(&basket_path).unwrap();

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: product_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: product_content.clone(),
            },
        })
        .await;

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: basket_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: basket_content.clone(),
            },
        })
        .await;

    let product_uri_str = product_uri.to_string();

    // Class references
    let results = backend
        .find_references(
            &product_uri_str,
            &product_content,
            Position::new(3, 6),
            true,
        )
        .expect("should find class references");

    eprintln!("=== Async cross-file: 'Product' class references ===");
    for (i, loc) in results.iter().enumerate() {
        eprintln!(
            "  [{}] {}:L{}:{}-{}:{}",
            i,
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character
        );
    }
    assert_no_duplicates(&results, "async_cross_file_class_refs");

    // Method references
    let results = backend
        .find_references(
            &product_uri_str,
            &product_content,
            Position::new(4, 21),
            true,
        )
        .expect("should find method references");

    eprintln!("=== Async cross-file: 'price' method references ===");
    for (i, loc) in results.iter().enumerate() {
        eprintln!(
            "  [{}] {}:L{}:{}-{}:{}",
            i,
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character
        );
    }
    assert_no_duplicates(&results, "async_cross_file_method_refs");
}

/// Test that opens only one file via did_open and relies on workspace
/// indexing to discover the second file. This is the most realistic
/// production scenario where URI format mismatches could occur.
#[tokio::test]
async fn async_did_open_one_file_workspace_discovers_other() {
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::{DidOpenTextDocumentParams, TextDocumentItem, Url};

    let (backend, dir) = crate::common::create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Widget.php",
                r#"<?php
namespace App;

class Widget {
    public function render(): string { return ''; }
}
"#,
            ),
            (
                "src/Dashboard.php",
                r#"<?php
namespace App;

use App\Widget;

class Dashboard {
    public function show(): void {
        $w = new Widget();
        $w->render();
        $w->render();
    }
}
"#,
            ),
        ],
    );

    // Only open Widget.php via did_open. Dashboard.php should be
    // discovered by ensure_workspace_indexed when find_references runs.
    let widget_path = dir.path().join("src/Widget.php");
    let widget_uri = Url::from_file_path(&widget_path).unwrap();
    let widget_content = std::fs::read_to_string(&widget_path).unwrap();

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: widget_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: widget_content.clone(),
            },
        })
        .await;

    let widget_uri_str = widget_uri.to_string();

    // Class references — triggers workspace indexing
    let results = backend
        .find_references(&widget_uri_str, &widget_content, Position::new(3, 6), true)
        .expect("should find class references");

    eprintln!("=== One-file async: 'Widget' class references ===");
    for (i, loc) in results.iter().enumerate() {
        eprintln!(
            "  [{}] {}:L{}:{}-{}:{}",
            i,
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character
        );
    }
    assert_no_duplicates(&results, "async_one_file_class_refs");

    // Method references
    let results = backend
        .find_references(&widget_uri_str, &widget_content, Position::new(4, 21), true)
        .expect("should find method references");

    eprintln!("=== One-file async: 'render' method references ===");
    for (i, loc) in results.iter().enumerate() {
        eprintln!(
            "  [{}] {}:L{}:{}-{}:{}",
            i,
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character
        );
    }
    assert_no_duplicates(&results, "async_one_file_method_refs");
}

// ─── Symbol map span dump test ──────────────────────────────────────────────

#[test]
fn debug_dump_all_spans_for_duplicate_detection() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_span_dump.php";
    let content = r#"<?php

namespace App;

use App\Order;

class Order {
    public string $name = '';
    public static int $count = 0;
    const STATUS_ACTIVE = 1;

    public function total(): int { return 0; }
    public static function create(): self { return new self(); }
}

class Service {
    public function process(Order $order): void {
        $o = new Order();
        $o->total();
        $o->total();
        $o->name;
        Order::create();
        Order::$count;
        Order::STATUS_ACTIVE;
        echo $o->name;
    }
}
"#;

    open_file(&backend, uri, content);

    // Read the symbol map directly and dump all spans
    let maps = backend.open_files(); // just to prove file is loaded
    assert!(
        maps.read().contains_key(uri),
        "file should be in open_files"
    );

    // Use find_references on various symbols and check for duplicates

    // Class "Order" declaration (line 6, col 6)
    let results = backend
        .find_references(uri, content, Position::new(6, 6), true)
        .unwrap_or_default();
    eprintln!("=== 'Order' class references ===");
    for (i, loc) in results.iter().enumerate() {
        let line = loc.range.start.line;
        let col = loc.range.start.character;
        let src = content.lines().nth(line as usize).unwrap_or("");
        eprintln!("  [{}] L{}:{} | {}", i, line, col, src.trim());
    }
    assert_no_duplicates(&results, "Order class");

    // Method "total" declaration (line 11, col 21)
    let results = backend
        .find_references(uri, content, Position::new(11, 21), true)
        .unwrap_or_default();
    eprintln!("=== 'total' method references ===");
    for (i, loc) in results.iter().enumerate() {
        let line = loc.range.start.line;
        let col = loc.range.start.character;
        let src = content.lines().nth(line as usize).unwrap_or("");
        eprintln!("  [{}] L{}:{} | {}", i, line, col, src.trim());
    }
    assert_no_duplicates(&results, "total method");

    // Property "name" access (line 21, col 13)
    let results = backend
        .find_references(uri, content, Position::new(21, 13), true)
        .unwrap_or_default();
    eprintln!("=== 'name' property references ===");
    for (i, loc) in results.iter().enumerate() {
        let line = loc.range.start.line;
        let col = loc.range.start.character;
        let src = content.lines().nth(line as usize).unwrap_or("");
        eprintln!("  [{}] L{}:{} | {}", i, line, col, src.trim());
    }
    assert_no_duplicates(&results, "name property");

    // Static method "create" (line 22, col 14)
    let results = backend
        .find_references(uri, content, Position::new(22, 14), true)
        .unwrap_or_default();
    eprintln!("=== 'create' static method references ===");
    for (i, loc) in results.iter().enumerate() {
        let line = loc.range.start.line;
        let col = loc.range.start.character;
        let src = content.lines().nth(line as usize).unwrap_or("");
        eprintln!("  [{}] L{}:{} | {}", i, line, col, src.trim());
    }
    assert_no_duplicates(&results, "create static method");

    // Constant "STATUS_ACTIVE" (line 24, col 16)
    let results = backend
        .find_references(uri, content, Position::new(24, 16), true)
        .unwrap_or_default();
    eprintln!("=== 'STATUS_ACTIVE' constant references ===");
    for (i, loc) in results.iter().enumerate() {
        let line = loc.range.start.line;
        let col = loc.range.start.character;
        let src = content.lines().nth(line as usize).unwrap_or("");
        eprintln!("  [{}] L{}:{} | {}", i, line, col, src.trim());
    }
    assert_no_duplicates(&results, "STATUS_ACTIVE constant");

    // Static property "$count" (line 23, col 12)
    let results = backend
        .find_references(uri, content, Position::new(23, 12), true)
        .unwrap_or_default();
    eprintln!("=== '$count' static property references ===");
    for (i, loc) in results.iter().enumerate() {
        let line = loc.range.start.line;
        let col = loc.range.start.character;
        let src = content.lines().nth(line as usize).unwrap_or("");
        eprintln!("  [{}] L{}:{} | {}", i, line, col, src.trim());
    }
    assert_no_duplicates(&results, "$count static property");
}

// ─── Workspace-indexed cross-file tests ─────────────────────────────────────
// These tests create real files on disk so that `ensure_workspace_indexed`
// (phase 2) discovers them, which is the path most likely to produce
// duplicate entries in production.

#[test]
fn workspace_indexed_class_references_no_duplicates() {
    let (backend, dir) = crate::common::create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Order.php",
                r#"<?php
namespace App;

class Order {
    public function total(): int { return 0; }
}
"#,
            ),
            (
                "src/Service.php",
                r#"<?php
namespace App;

use App\Order;

class Service {
    public function process(Order $order): void {
        $o = new Order();
        $o->total();
    }
}
"#,
            ),
            (
                "src/Controller.php",
                r#"<?php
namespace App;

use App\Order;

class Controller {
    public function index(): void {
        $order = new Order();
        $order->total();
    }
}
"#,
            ),
        ],
    );

    let order_path = dir.path().join("src/Order.php");
    let service_path = dir.path().join("src/Service.php");
    let controller_path = dir.path().join("src/Controller.php");

    let order_uri = format!("file://{}", order_path.display());
    let service_uri = format!("file://{}", service_path.display());
    let controller_uri = format!("file://{}", controller_path.display());

    let order_content = std::fs::read_to_string(&order_path).unwrap();
    let service_content = std::fs::read_to_string(&service_path).unwrap();
    let controller_content = std::fs::read_to_string(&controller_path).unwrap();

    open_file(&backend, &order_uri, &order_content);
    open_file(&backend, &service_uri, &service_content);
    open_file(&backend, &controller_uri, &controller_content);

    // ── Class references ────────────────────────────────────────────
    // Cursor on `Order` class declaration (line 3, col 6)
    let results = backend
        .find_references(&order_uri, &order_content, Position::new(3, 6), true)
        .expect("should find class references");

    eprintln!("=== Workspace-indexed 'Order' class references ===");
    for (i, loc) in results.iter().enumerate() {
        eprintln!(
            "  [{}] {}:{}:{}-{}:{}",
            i,
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character,
        );
    }

    assert_no_duplicates(&results, "workspace_class_refs");

    // ── Method references ───────────────────────────────────────────
    // Cursor on `total` method declaration (line 4, col 21)
    let results = backend
        .find_references(&order_uri, &order_content, Position::new(4, 21), true)
        .expect("should find method references");

    eprintln!("=== Workspace-indexed 'total' method references ===");
    for (i, loc) in results.iter().enumerate() {
        eprintln!(
            "  [{}] {}:{}:{}-{}:{}",
            i,
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character,
        );
    }

    assert_no_duplicates(&results, "workspace_method_refs");
}

/// This test opens only ONE file and relies on `ensure_workspace_indexed`
/// to discover the other files on disk.  This is the scenario most likely
/// to produce URI mismatches (and thus duplicate entries).
#[test]
fn workspace_indexed_only_one_file_opened_no_duplicates() {
    let (backend, dir) = crate::common::create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Item.php",
                r#"<?php
namespace App;

class Item {
    public function price(): int { return 0; }
}
"#,
            ),
            (
                "src/Cart.php",
                r#"<?php
namespace App;

use App\Item;

class Cart {
    public function addItem(Item $item): void {
        $i = new Item();
        $i->price();
    }
}
"#,
            ),
        ],
    );

    // Only open Item.php — Cart.php should be discovered by workspace scan.
    let item_path = dir.path().join("src/Item.php");
    let item_uri = format!("file://{}", item_path.display());
    let item_content = std::fs::read_to_string(&item_path).unwrap();

    open_file(&backend, &item_uri, &item_content);

    // ── Class references ────────────────────────────────────────────
    let results = backend
        .find_references(&item_uri, &item_content, Position::new(3, 6), true)
        .expect("should find class references");

    eprintln!("=== One-file-opened 'Item' class references ===");
    for (i, loc) in results.iter().enumerate() {
        eprintln!(
            "  [{}] {}:{}:{}-{}:{}",
            i,
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character,
        );
    }

    assert_no_duplicates(&results, "one_file_class_refs");

    // ── Method references ───────────────────────────────────────────
    let results = backend
        .find_references(&item_uri, &item_content, Position::new(4, 21), true)
        .expect("should find method references");

    eprintln!("=== One-file-opened 'price' method references ===");
    for (i, loc) in results.iter().enumerate() {
        eprintln!(
            "  [{}] {}:{}:{}-{}:{}",
            i,
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character,
        );
    }

    assert_no_duplicates(&results, "one_file_method_refs");
}

#[tokio::test]
async fn workspace_index_refreshes_after_new_file_is_added() {
    let (backend, dir) = crate::common::create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Item.php",
            r#"<?php
namespace App;

class Item {}
"#,
        )],
    );

    let item_path = dir.path().join("src/Item.php");
    let item_uri = format!("file://{}", item_path.display());
    let item_content = std::fs::read_to_string(&item_path).unwrap();

    open_file(&backend, &item_uri, &item_content);

    let item_url = Url::parse(&item_uri).unwrap();
    let initial_results = references_at(&backend, &item_url, 3, 6, true).await;
    assert!(
        !initial_results.is_empty(),
        "should find initial class references"
    );

    assert_no_duplicates(&initial_results, "workspace_refresh_initial_refs");

    let service_path = dir.path().join("src/Service.php");
    std::fs::write(
        &service_path,
        r#"<?php
namespace App;

use App\Item;

class Service {
    public function build(): void {
        $item = new Item();
    }
}
"#,
    )
    .expect("failed to write newly added PHP file");

    let service_uri = format!("file://{}", service_path.display());
    let refreshed_results = references_at(&backend, &item_url, 3, 6, true).await;

    assert_no_duplicates(&refreshed_results, "workspace_refresh_refs");
    assert!(
        refreshed_results
            .iter()
            .any(|loc| loc.uri.as_str() == service_uri),
        "Expected newly added Service.php to be discovered, got {:#?}",
        refreshed_results
    );
}

// ─── Nullable / union type member references ────────────────────────────────

/// Find references on a @property-read member via a nullable variable
/// should include the @property-read declaration and non-nullable usages.
#[test]
fn member_references_nullable_type_virtual_property() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_nullable_virtual.php";
    let content = r#"<?php

/**
 * @property-read string $displayName
 */
class Author {
    public function __get(string $name): mixed { return null; }

    /** @return static|null */
    public static function first(): ?static { return null; }
}

function test(): void {
    $found = Author::first();
    echo $found->displayName;

    $author = new Author();
    echo $author->displayName;
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `displayName` in `$found->displayName` (line 14)
    let results = backend
        .find_references(uri, content, Position::new(14, 18), true)
        .expect("should find references");

    assert_no_duplicates(&results, "nullable_virtual_prop_refs");

    for (i, loc) in results.iter().enumerate() {
        eprintln!(
            "  [{}] {}:{}:{}-{}:{}",
            i,
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character,
        );
    }

    // Expect: 1 @property-read declaration + 2 accesses ($found->displayName, $author->displayName)
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 @property-read declaration + 2 accesses), got {}: {:#?}",
        results.len(),
        results
    );
}

/// Cross-file find references on a @property-read member via a nullable
/// variable should include the declaration and non-nullable usages from other files.
#[test]
fn cross_file_member_references_nullable_virtual_property() {
    let (backend, _dir) = crate::common::create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Author.php",
                r#"<?php
namespace App;

/**
 * @property-read string $displayName
 */
class Author {
    public function __get(string $name): mixed { return null; }

    /** @return static|null */
    public static function first(): ?static { return null; }
}
"#,
            ),
            (
                "src/Service.php",
                r#"<?php
namespace App;

class Service {
    public function test(): void {
        $found = Author::first();
        echo $found->displayName;

        $author = new Author();
        echo $author->displayName;
    }
}
"#,
            ),
        ],
    );

    let author_path = _dir.path().join("src/Author.php");
    let service_path = _dir.path().join("src/Service.php");

    let author_uri = format!("file://{}", author_path.display());
    let service_uri = format!("file://{}", service_path.display());

    let author_content = std::fs::read_to_string(&author_path).unwrap();
    let service_content = std::fs::read_to_string(&service_path).unwrap();

    open_file(&backend, &author_uri, &author_content);
    open_file(&backend, &service_uri, &service_content);

    // Cursor on `displayName` in `$found->displayName` (line 6 in Service.php)
    let results = backend
        .find_references(&service_uri, &service_content, Position::new(6, 22), true)
        .expect("should find references");

    assert_no_duplicates(&results, "cross_file_nullable_virtual_prop_refs");

    for (i, loc) in results.iter().enumerate() {
        eprintln!(
            "  [{}] {}:{}:{}-{}:{}",
            i,
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character,
        );
    }

    // Expect: 1 @property-read declaration in Author.php + 2 accesses in Service.php
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (1 @property-read declaration + 2 accesses), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Constructor reference tests ────────────────────────────────────────────

/// Whether any returned location starts on the given zero-based line.
fn has_location_on_line(results: &[tower_lsp::lsp_types::Location], line: u32) -> bool {
    results.iter().any(|loc| loc.range.start.line == line)
}

/// Finding references to a base constructor includes the explicit
/// `parent::__construct()` delegation call alongside the `new` sites.
#[test]
fn constructor_references_include_parent_call() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_ctor_parent.php";
    let content = r#"<?php

class Base {
    public function __construct() {}
}

class Child extends Base {
    public function __construct() {
        parent::__construct();
    }
}

$a = new Base();
$b = new Child();
"#;

    open_file(&backend, uri, content);

    // Cursor on Base's `__construct` declaration (line 3).
    let results = backend
        .find_references(uri, content, Position::new(3, 22), true)
        .expect("should find references");

    assert_no_duplicates(&results, "constructor_references_include_parent_call");

    // Expected: the Base declaration (line 3), the `parent::__construct()`
    // call (line 8), and `new Base()` (line 12).  `new Child()` invokes
    // Child's own constructor, so it must NOT appear.
    assert!(
        has_location_on_line(&results, 3),
        "missing constructor declaration: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 8),
        "missing parent::__construct() call: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 12),
        "missing new Base() site: {results:#?}"
    );
    assert!(
        !has_location_on_line(&results, 13),
        "new Child() invokes Child's own constructor and must not appear: {results:#?}"
    );
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (declaration + parent call + new Base), got {}: {:#?}",
        results.len(),
        results
    );
}

/// Clicking on the `parent::__construct()` call itself lists the call
/// alongside the other references to that constructor (not just the
/// subject's instantiation sites).
#[test]
fn constructor_references_from_parent_call_site() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_ctor_callsite.php";
    let content = r#"<?php

class Base {
    public function __construct() {}
}

class Child extends Base {
    public function __construct() {
        parent::__construct();
    }
}

$a = new Base();
"#;

    open_file(&backend, uri, content);

    // Cursor on the `__construct` member name in `parent::__construct()`
    // (line 8).
    let results = backend
        .find_references(uri, content, Position::new(8, 18), true)
        .expect("should find references");

    assert_no_duplicates(&results, "constructor_references_from_parent_call_site");

    assert!(
        has_location_on_line(&results, 3),
        "missing constructor declaration: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 8),
        "the parent::__construct() call should list itself: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 12),
        "missing new Base() site: {results:#?}"
    );
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (declaration + parent call + new Base), got {}: {:#?}",
        results.len(),
        results
    );
}

/// A `parent::__construct()` call references the *parent's* constructor,
/// not the subclass's.  Finding references to the subclass constructor
/// must therefore exclude the delegation call.
#[test]
fn constructor_references_exclude_parent_call_for_subclass() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_ctor_subclass.php";
    let content = r#"<?php

class Base {
    public function __construct() {}
}

class Child extends Base {
    public function __construct() {
        parent::__construct();
    }
}

$a = new Base();
$b = new Child();
"#;

    open_file(&backend, uri, content);

    // Cursor on Child's own `__construct` declaration (line 7).
    let results = backend
        .find_references(uri, content, Position::new(7, 22), true)
        .expect("should find references");

    assert_no_duplicates(
        &results,
        "constructor_references_exclude_parent_call_for_subclass",
    );

    // Expected: Child's declaration (line 7) and `new Child()` (line 13).
    // The `parent::__construct()` call references Base's constructor, not
    // Child's, so it must NOT appear.
    assert!(
        has_location_on_line(&results, 7),
        "missing Child constructor declaration: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 13),
        "missing new Child() site: {results:#?}"
    );
    assert!(
        !has_location_on_line(&results, 8),
        "parent::__construct() references the parent constructor, not Child's: {results:#?}"
    );
    assert_eq!(
        results.len(),
        2,
        "Expected 2 references (declaration + new Child), got {}: {:#?}",
        results.len(),
        results
    );
}

/// Explicit `self::__construct()` and `Class::__construct()` forms are
/// resolved and listed as constructor references.
#[test]
fn constructor_references_include_self_and_named_calls() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_ctor_self_named.php";
    let content = r#"<?php

class Widget {
    public function __construct() {}

    public function rebuild(): void {
        self::__construct();
    }
}

function make(): void {
    Widget::__construct();
}

$w = new Widget();
"#;

    open_file(&backend, uri, content);

    // Cursor on Widget's `__construct` declaration (line 3).
    let results = backend
        .find_references(uri, content, Position::new(3, 22), true)
        .expect("should find references");

    assert_no_duplicates(
        &results,
        "constructor_references_include_self_and_named_calls",
    );

    assert!(
        has_location_on_line(&results, 3),
        "missing constructor declaration: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 6),
        "missing self::__construct() call: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 11),
        "missing Widget::__construct() call: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 14),
        "missing new Widget() site: {results:#?}"
    );
    assert_eq!(
        results.len(),
        4,
        "Expected 4 references (declaration + self + Widget + new), got {}: {:#?}",
        results.len(),
        results
    );
}

/// `new self()` and `new static()` instantiate through the `self`/`static`
/// keywords rather than a named `ClassReference`, and must be reported
/// alongside plain `new ClassName()` sites.
#[test]
fn constructor_references_include_new_self_and_static() {
    let backend = create_test_backend();
    let uri = "file:///tmp/test_refs_ctor_new_self_static.php";
    let content = r#"<?php

class ConfigPaths {
    public function __construct() {}

    public static function home(): ConfigPaths {
        return new self();
    }

    public static function viaStatic(): ConfigPaths {
        return new static();
    }
}
"#;

    open_file(&backend, uri, content);

    // Cursor on ConfigPaths's `__construct` declaration (line 3).
    let results = backend
        .find_references(uri, content, Position::new(3, 22), true)
        .expect("should find references");

    assert_no_duplicates(
        &results,
        "constructor_references_include_new_self_and_static",
    );

    assert!(
        has_location_on_line(&results, 3),
        "missing constructor declaration: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 6),
        "missing new self() call: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 10),
        "missing new static() call: {results:#?}"
    );
    assert_eq!(
        results.len(),
        3,
        "Expected 3 references (declaration + new self + new static), got {}: {:#?}",
        results.len(),
        results
    );
}

/// A constant read off a value a reflection-based accessor returned is a
/// real reference to it, even though nothing in the accessor's signature
/// says which class comes back: the class was decided by the arguments the
/// call passed, and reading the accessor's body recovers it.
///
/// `Accessor::fetchProperty()` is `Psy\Sudo::fetchProperty()` verbatim, and
/// `Psy\Shell::VERSION` read through it is the reference this used to miss.
#[test]
fn constant_read_through_a_reflection_accessor_is_a_reference() {
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///tmp/test_refs_reflection_accessor.php";
    let content = r#"<?php

class Shell {
    const VERSION = 'v1.0.0';
}

class Configuration {
    private ?Shell $shell = null;
}

class Accessor {
    /**
     * @return mixed Value of $object->property
     */
    public static function fetchProperty($object, string $property)
    {
        $prop = self::getProperty(new \ReflectionObject($object), $property);

        return $prop->getValue($object);
    }

    private static function getProperty(\ReflectionClass $refl, string $property): \ReflectionProperty
    {
        return $refl->getProperty($property);
    }
}

function probe(Configuration $config): void {
    $shell = Accessor::fetchProperty($config, 'shell');
    echo $shell::VERSION;
}
"#;

    open_file(&backend, uri, content);

    // Cursor on `VERSION` in the constant declaration (line 3).
    let results = backend
        .find_references(uri, content, Position::new(3, 10), true)
        .expect("should find references");

    assert_no_duplicates(&results, "reflection_accessor_constant_references");
    assert!(
        has_location_on_line(&results, 3),
        "missing the declaration: {results:#?}"
    );
    assert!(
        has_location_on_line(&results, 29),
        "missing the read through the accessor: {results:#?}"
    );
    assert_eq!(
        results.len(),
        2,
        "Expected 2 references (declaration + read), got {}: {:#?}",
        results.len(),
        results
    );
}

// ─── Variable References ────────────────────────────────────────────────────

#[tokio::test]
async fn test_variable_references_same_scope() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // L0
        "function demo(): void {\n",    // L1
        "    $user = new User();\n",    // L2
        "    $user->name = 'Alice';\n", // L3
        "    echo $user->name;\n",      // L4
        "}\n",                          // L5
    );

    open_php(&backend, &uri, text).await;

    // Click on $user at line 3
    let locs = references_at(&backend, &uri, 3, 5, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references to $user, got {}",
        locs.len()
    );
    // All references should be in the same file.
    for loc in &locs {
        assert_eq!(loc.uri, uri);
    }
}

#[tokio::test]
async fn test_variable_references_excludes_other_scope() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                    // L0
        "function alpha(): void {\n", // L1
        "    $x = 1;\n",              // L2
        "    echo $x;\n",             // L3
        "}\n",                        // L4
        "function beta(): void {\n",  // L5
        "    $x = 2;\n",              // L6
        "    echo $x;\n",             // L7
        "}\n",                        // L8
    );

    open_php(&backend, &uri, text).await;

    // References to $x in alpha() should NOT include $x in beta().
    let locs = references_at(&backend, &uri, 2, 5, true).await;
    for loc in &locs {
        assert!(
            loc.range.start.line <= 4,
            "Reference to $x in alpha() should not appear in beta() (line {})",
            loc.range.start.line
        );
    }
}

#[tokio::test]
async fn test_variable_references_exclude_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                   // L0
        "function demo(): void {\n", // L1
        "    $val = 42;\n",          // L2
        "    echo $val;\n",          // L3
        "    $val = 99;\n",          // L4
        "}\n",                       // L5
    );

    open_php(&backend, &uri, text).await;

    // include_declaration = false: should still include usage sites
    let locs_no_decl = references_at(&backend, &uri, 3, 10, false).await;
    let locs_with_decl = references_at(&backend, &uri, 3, 10, true).await;
    // With declaration should have at least as many as without.
    assert!(
        locs_with_decl.len() >= locs_no_decl.len(),
        "with_decl ({}) should be >= no_decl ({})",
        locs_with_decl.len(),
        locs_no_decl.len()
    );
}

#[tokio::test]
async fn test_variable_references_include_compact_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): array {\n",
        "    $user = 'alice';\n",
        "    return compact('user');\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let locs = references_at(&backend, &uri, 2, 6, true).await;
    assert!(
        locs.iter().any(|loc| {
            loc.range.start.line == 3
                && loc.range.start.character == 20
                && loc.range.end.character == 24
        }),
        "Expected compact('user') string contents to be included in variable references: {locs:?}"
    );
}

#[tokio::test]
async fn test_variable_references_include_compact_array_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo(): array {\n",
        "    $user = 'alice';\n",
        "    return compact(['user']);\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let locs = references_at(&backend, &uri, 2, 6, true).await;
    assert!(
        locs.iter().any(|loc| {
            loc.range.start.line == 3
                && loc.range.start.character == 21
                && loc.range.end.character == 25
        }),
        "Expected compact(['user']) string contents to be included in variable references: {locs:?}"
    );
}

// ─── Class References ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_class_references_same_file() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // L0
        "class Logger {\n",                             // L1
        "    public function info(): void {}\n",        // L2
        "}\n",                                          // L3
        "class Service {\n",                            // L4
        "    public function run(Logger $l): void {\n", // L5
        "        $x = new Logger();\n",                 // L6
        "    }\n",                                      // L7
        "}\n",                                          // L8
    );

    open_php(&backend, &uri, text).await;

    // Click on "Logger" on line 5 (type hint).
    let locs = references_at(&backend, &uri, 5, 27, true).await;
    // Should find: declaration (L1), type hint (L5), new (L6) = at least 3.
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to Logger, got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_class_references_exclude_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // L0
        "class Foo {}\n",                            // L1
        "class Bar {\n",                             // L2
        "    public function test(Foo $f): Foo {\n", // L3
        "        return new Foo();\n",               // L4
        "    }\n",                                   // L5
        "}\n",                                       // L6
    );

    open_php(&backend, &uri, text).await;

    // Without declaration: should not include the `class Foo` declaration site.
    let locs = references_at(&backend, &uri, 3, 25, false).await;
    for loc in &locs {
        // Line 1 is the declaration of class Foo.
        assert_ne!(
            loc.range.start.line, 1,
            "Should not include declaration site when include_declaration=false"
        );
    }

    // With declaration: should include line 1.
    let locs_decl = references_at(&backend, &uri, 3, 25, true).await;
    let has_decl = locs_decl.iter().any(|l| l.range.start.line == 1);
    assert!(
        has_decl,
        "Should include declaration site when include_declaration=true"
    );
}

#[tokio::test]
async fn test_class_declaration_finds_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                     // L0
        "class Widget {}\n",           // L1
        "function make(): Widget {\n", // L2
        "    return new Widget();\n",  // L3
        "}\n",                         // L4
    );

    open_php(&backend, &uri, text).await;

    // Click on "Widget" at the declaration (line 1).
    let locs = references_at(&backend, &uri, 1, 7, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references (decl + 2 usages), got {}",
        locs.len()
    );
}

// ─── Member Access References ───────────────────────────────────────────────

#[tokio::test]
async fn test_method_references_same_file() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // L0
        "class Repo {\n",                               // L1
        "    public function find(int $id): void {}\n", // L2
        "}\n",                                          // L3
        "class Controller {\n",                         // L4
        "    public function index(Repo $r): void {\n", // L5
        "        $r->find(1);\n",                       // L6
        "        $r->find(2);\n",                       // L7
        "    }\n",                                      // L8
        "}\n",                                          // L9
    );

    open_php(&backend, &uri, text).await;

    // Click on "find" at line 6 (method call).
    let locs = references_at(&backend, &uri, 6, 14, false).await;
    // Should find at least 2 call sites (L6, L7).
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to find(), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_method_references_include_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // L0
        "class Repo {\n",                        // L1
        "    public function save(): void {}\n", // L2
        "}\n",                                   // L3
        "function demo(Repo $r): void {\n",      // L4
        "    $r->save();\n",                     // L5
        "}\n",                                   // L6
    );

    open_php(&backend, &uri, text).await;

    // With declaration should also include the method definition on L2.
    let locs = references_at(&backend, &uri, 5, 10, true).await;
    let has_def = locs.iter().any(|l| l.range.start.line == 2);
    assert!(
        has_def,
        "Should include method declaration when include_declaration=true"
    );
}

#[tokio::test]
async fn test_static_method_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // L0
        "class Factory {\n",                              // L1
        "    public static function create(): void {}\n", // L2
        "}\n",                                            // L3
        "function demo(): void {\n",                      // L4
        "    Factory::create();\n",                       // L5
        "    Factory::create();\n",                       // L6
        "}\n",                                            // L7
    );

    open_php(&backend, &uri, text).await;

    // Click on "create" at line 5.
    let locs = references_at(&backend, &uri, 5, 15, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to create(), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_property_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                            // L0
        "class Config {\n",                   // L1
        "    public string $name = '';\n",    // L2
        "}\n",                                // L3
        "function demo(Config $c): void {\n", // L4
        "    echo $c->name;\n",               // L5
        "    $c->name = 'test';\n",           // L6
        "}\n",                                // L7
    );

    open_php(&backend, &uri, text).await;

    // Click on "name" at line 5 (property access).
    let locs = references_at(&backend, &uri, 5, 15, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to ->name, got {}",
        locs.len()
    );
}

// ─── Function Call References ───────────────────────────────────────────────

#[tokio::test]
async fn test_function_call_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // L0
        "function helper(): void {}\n", // L1
        "function main(): void {\n",    // L2
        "    helper();\n",              // L3
        "    helper();\n",              // L4
        "}\n",                          // L5
    );

    open_php(&backend, &uri, text).await;

    // Click on "helper" at line 3.
    let locs = references_at(&backend, &uri, 3, 6, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to helper(), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_function_references_include_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                // L0
        "function myFunc(): int { return 1; }\n", // L1
        "function demo(): void {\n",              // L2
        "    $x = myFunc();\n",                   // L3
        "}\n",                                    // L4
    );

    open_php(&backend, &uri, text).await;

    // With declaration should include the function definition on L1.
    let locs = references_at(&backend, &uri, 3, 11, true).await;
    let has_def = locs.iter().any(|l| l.range.start.line == 1);
    assert!(
        has_def,
        "Should include function declaration when include_declaration=true"
    );
}

// ─── Constant References ────────────────────────────────────────────────────

#[tokio::test]
async fn test_constant_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                    // L0
        "class Status {\n",           // L1
        "    const ACTIVE = 1;\n",    // L2
        "}\n",                        // L3
        "function demo(): void {\n",  // L4
        "    echo Status::ACTIVE;\n", // L5
        "    $x = Status::ACTIVE;\n", // L6
        "}\n",                        // L7
    );

    open_php(&backend, &uri, text).await;

    // Click on "ACTIVE" at line 5.
    let locs = references_at(&backend, &uri, 5, 20, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to ACTIVE, got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_define_constant_references_use_reference_index_snapshot() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///constants.php").unwrap();
    let text = concat!(
        "<?php\n",                        // L0
        "define('APP_FLAG', true);\n",    // L1
        "if (APP_FLAG) { echo 'on'; }\n"  // L2
    );

    open_php(&backend, &uri, text).await;

    let locs = references_at(&backend, &uri, 2, 4, false).await;
    assert!(
        locs.iter().any(|loc| loc.range.start.line == 2),
        "Expected bare APP_FLAG usage to be found through constant references, got {locs:?}"
    );
}

// ─── self / static / parent References ──────────────────────────────────────

#[tokio::test]
async fn test_self_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // L0
        "class Item {\n",                                // L1
        "    public static function create(): self {\n", // L2
        "        return new self();\n",                  // L3
        "    }\n",                                       // L4
        "}\n",                                           // L5
        "function demo(): void {\n",                     // L6
        "    $x = new Item();\n",                        // L7
        "}\n",                                           // L8
    );

    open_php(&backend, &uri, text).await;

    // Click on "self" at line 3.  This should resolve to class Item
    // and find references to Item across the file.
    let locs = references_at(&backend, &uri, 3, 20, true).await;
    assert!(
        !locs.is_empty(),
        "Expected references when clicking on self"
    );
}

// ─── Cross-File References ──────────────────────────────────────────────────

#[tokio::test]
async fn test_class_references_cross_file() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",           // L0
        "class Animal {}\n", // L1
    );
    let text_b = concat!(
        "<?php\n",                                       // L0
        "class Zoo {\n",                                 // L1
        "    public function add(Animal $a): void {}\n", // L2
        "    public function get(): Animal {\n",         // L3
        "        return new Animal();\n",                // L4
        "    }\n",                                       // L5
        "}\n",                                           // L6
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Find references to Animal from file a.
    let locs = references_at(&backend, &uri_a, 1, 7, true).await;
    // Should find references in both files.
    let in_a = locs.iter().filter(|l| l.uri == uri_a).count();
    let in_b = locs.iter().filter(|l| l.uri == uri_b).count();
    assert!(
        in_a >= 1,
        "Expected at least 1 reference in a.php, got {}",
        in_a
    );
    assert!(
        in_b >= 1,
        "Expected at least 1 reference in b.php, got {}",
        in_b
    );
}

#[tokio::test]
async fn test_member_references_cross_file() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                                // L0
        "class Printer {\n",                      // L1
        "    public function print(): void {}\n", // L2
        "}\n",                                    // L3
        "function useA(Printer $p): void {\n",    // L4
        "    $p->print();\n",                     // L5
        "}\n",                                    // L6
    );
    let text_b = concat!(
        "<?php\n",                             // L0
        "function useB(Printer $p): void {\n", // L1
        "    $p->print();\n",                  // L2
        "}\n",                                 // L3
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Find references to print() from file a.
    let locs = references_at(&backend, &uri_a, 5, 10, false).await;
    let in_a = locs.iter().filter(|l| l.uri == uri_a).count();
    let in_b = locs.iter().filter(|l| l.uri == uri_b).count();
    assert!(
        in_a >= 1,
        "Expected at least 1 reference in a.php, got {}",
        in_a
    );
    assert!(
        in_b >= 1,
        "Expected at least 1 reference in b.php, got {}",
        in_b
    );
}

// ─── Namespaced References ──────────────────────────────────────────────────

#[tokio::test]
async fn test_namespaced_class_references() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                  // L0
        "namespace App\\Models;\n", // L1
        "class User {}\n",          // L2
    );
    let text_b = concat!(
        "<?php\n",                              // L0
        "namespace App\\Services;\n",           // L1
        "use App\\Models\\User;\n",             // L2
        "class UserService {\n",                // L3
        "    public function find(): User {\n", // L4
        "        return new User();\n",         // L5
        "    }\n",                              // L6
        "}\n",                                  // L7
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Find references to App\Models\User from declaration in a.php.
    let locs = references_at(&backend, &uri_a, 2, 7, true).await;
    let in_b = locs.iter().filter(|l| l.uri == uri_b).count();
    assert!(
        in_b >= 1,
        "Expected at least 1 cross-file namespaced reference in b.php, got {}",
        in_b
    );
}

#[tokio::test]
async fn test_braced_namespace_resolves_receiver_against_its_own_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multi_ns.php").unwrap();

    let text = concat!(
        "<?php\n",                                    // L0
        "namespace Other {\n",                        // L1
        "    class Author {}\n",                      // L2
        "}\n",                                        // L3
        "namespace App {\n",                          // L4
        "    class Author {\n",                       // L5
        "        public static function make() {}\n", // L6
        "    }\n",                                    // L7
        "    function show() {\n",                    // L8
        "        Author::make();\n",                  // L9
        "    }\n",                                    // L10
        "}\n",                                        // L11
    );

    open_php(&backend, &uri, text).await;

    // `make` at L6 is declared on `App\Author`; the call at L9 sits in the
    // same `App` block, so it must be found even though `Other\Author` is
    // declared first in the file.
    let locs = references_at(&backend, &uri, 6, 31, false).await;
    assert!(
        locs.iter().any(|l| l.range.start.line == 9),
        "Expected a reference at L9 (Author::make() in the `App` namespace), got {:?}",
        locs
    );
}

// ─── Edge Cases ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_no_references_on_whitespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",        // L0
        "\n",             // L1
        "class Foo {}\n", // L2
    );

    open_php(&backend, &uri, text).await;

    // Click on empty line — should return None / empty.
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 0,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result = backend.references(params).await.unwrap();
    assert!(
        result.is_none() || result.as_ref().unwrap().is_empty(),
        "Expected no references on whitespace"
    );
}

#[tokio::test]
async fn test_variable_parameter_reference() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // L0
        "function greet(string $name): string {\n", // L1
        "    return 'Hello ' . $name;\n",           // L2
        "}\n",                                      // L3
    );

    open_php(&backend, &uri, text).await;

    // Click on $name at usage (line 2).
    let locs = references_at(&backend, &uri, 2, 25, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references (param + usage), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_results_sorted_by_position() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // L0
        "class X {}\n",                              // L1
        "function a(X $x): X { return new X(); }\n", // L2
        "function b(X $x): X { return new X(); }\n", // L3
    );

    open_php(&backend, &uri, text).await;

    let locs = references_at(&backend, &uri, 2, 12, true).await;
    // Verify results are sorted by line then character.
    for window in locs.windows(2) {
        let a = &window[0];
        let b = &window[1];
        let a_before_b = (a.uri.as_str(), a.range.start.line, a.range.start.character)
            <= (b.uri.as_str(), b.range.start.line, b.range.start.character);
        assert!(
            a_before_b,
            "Results should be sorted: {:?} should come before {:?}",
            a.range.start, b.range.start
        );
    }
}

#[tokio::test]
async fn test_class_extends_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                           // L0
        "class Base {}\n",                   // L1
        "class Child extends Base {}\n",     // L2
        "function demo(Base $b): void {}\n", // L3
    );

    open_php(&backend, &uri, text).await;

    // Find references to Base — should include extends clause and type hint.
    let locs = references_at(&backend, &uri, 1, 7, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references (decl + extends + param), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_interface_implements_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // L0
        "interface Loggable {}\n",                   // L1
        "class FileLogger implements Loggable {}\n", // L2
        "function log(Loggable $l): void {}\n",      // L3
    );

    open_php(&backend, &uri, text).await;

    let locs = references_at(&backend, &uri, 1, 12, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references (decl + implements + param), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_foreach_variable_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                           // L0
        "function demo(): void {\n",         // L1
        "    $items = [1, 2, 3];\n",         // L2
        "    foreach ($items as $item) {\n", // L3
        "        echo $item;\n",             // L4
        "        echo $item + 1;\n",         // L5
        "    }\n",                           // L6
        "}\n",                               // L7
    );

    open_php(&backend, &uri, text).await;

    // Click on $item at line 4.
    let locs = references_at(&backend, &uri, 4, 14, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to $item (foreach var + usages), got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_static_property_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // L0
        "class Counter {\n",                                // L1
        "    public static int $count = 0;\n",              // L2
        "    public static function increment(): void {\n", // L3
        "        self::$count++;\n",                        // L4
        "    }\n",                                          // L5
        "}\n",                                              // L6
        "function demo(): void {\n",                        // L7
        "    Counter::$count = 5;\n",                       // L8
        "}\n",                                              // L9
    );

    open_php(&backend, &uri, text).await;

    // Click on $count at line 4.
    let locs = references_at(&backend, &uri, 4, 16, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to static $count, got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_this_property_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // L0
        "class Person {\n",                                      // L1
        "    public string $email = '';\n",                      // L2
        "    public function setEmail(string $email): void {\n", // L3
        "        $this->email = $email;\n",                      // L4
        "    }\n",                                               // L5
        "    public function getEmail(): string {\n",            // L6
        "        return $this->email;\n",                        // L7
        "    }\n",                                               // L8
        "}\n",                                                   // L9
    );

    open_php(&backend, &uri, text).await;

    // Click on "email" at line 4 (property access via $this->).
    let locs = references_at(&backend, &uri, 4, 17, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to ->email, got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_multiple_files_function_references() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///helpers.php").unwrap();
    let uri_b = Url::parse("file:///main.php").unwrap();

    let text_a = concat!(
        "<?php\n",                                        // L0
        "function format_name(string $name): string {\n", // L1
        "    return ucfirst($name);\n",                   // L2
        "}\n",                                            // L3
    );
    let text_b = concat!(
        "<?php\n",                          // L0
        "function demo(): void {\n",        // L1
        "    $x = format_name('alice');\n", // L2
        "    $y = format_name('bob');\n",   // L3
        "}\n",                              // L4
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Find references to format_name from file b.
    let locs = references_at(&backend, &uri_b, 2, 11, false).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 call-site references across files, got {}",
        locs.len()
    );
}

// ─── $this References (file-local, not cross-file class search) ─────────────

#[tokio::test]
async fn test_this_is_file_local_not_cross_file() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                             // L0
        "class Foo {\n",                       // L1
        "    public function bar(): void {\n", // L2
        "        $this->baz();\n",             // L3
        "    }\n",                             // L4
        "}\n",                                 // L5
    );
    let text_b = concat!(
        "<?php\n",                         // L0
        "function demo(Foo $f): void {\n", // L1
        "    $f->baz();\n",                // L2
        "}\n",                             // L3
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Click on $this at line 3 of a.php.
    let locs = references_at(&backend, &uri_a, 3, 9, true).await;

    // All results must be in the same file — $this is not a cross-file
    // class reference.
    for loc in &locs {
        assert_eq!(
            loc.uri, uri_a,
            "$this references should stay within the current file, but found one in {}",
            loc.uri
        );
    }
}

#[tokio::test]
async fn test_this_references_within_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // L0
        "class Account {\n",                                // L1
        "    public string $name = '';\n",                  // L2
        "    public function getName(): string {\n",        // L3
        "        return $this->name;\n",                    // L4
        "    }\n",                                          // L5
        "    public function setName(string $n): void {\n", // L6
        "        $this->name = $n;\n",                      // L7
        "    }\n",                                          // L8
        "    public function self_ref(): self {\n",         // L9
        "        return $this;\n",                          // L10
        "    }\n",                                          // L11
        "}\n",                                              // L12
    );

    open_php(&backend, &uri, text).await;

    // Click on $this at line 4.
    let locs = references_at(&backend, &uri, 4, 16, true).await;
    // Should find at least 3 occurrences of $this (L4, L7, L10).
    assert!(
        locs.len() >= 3,
        "Expected at least 3 $this references in Account, got {}",
        locs.len()
    );
    for loc in &locs {
        assert_eq!(loc.uri, uri);
    }
}

#[tokio::test]
async fn test_this_scoped_to_enclosing_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                            // L0
        "class Alpha {\n",                    // L1
        "    public function go(): void {\n", // L2
        "        $this->run();\n",            // L3
        "    }\n",                            // L4
        "}\n",                                // L5
        "class Beta {\n",                     // L6
        "    public function go(): void {\n", // L7
        "        $this->run();\n",            // L8
        "    }\n",                            // L9
        "}\n",                                // L10
    );

    open_php(&backend, &uri, text).await;

    // Click on $this inside Alpha (line 3).
    let locs = references_at(&backend, &uri, 3, 9, true).await;
    // Should NOT include $this from Beta on line 8.
    for loc in &locs {
        assert!(
            loc.range.start.line < 5,
            "$this in Alpha should not include Beta's $this on line {}",
            loc.range.start.line
        );
    }
    assert!(!locs.is_empty(), "Should find at least one $this in Alpha");
}

// ─── Method Declaration Triggers Find References ────────────────────────────

#[tokio::test]
async fn test_method_declaration_triggers_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                              // L0
        "class Converter {\n",                                                  // L1
        "    public static function toListOfString(iterable $values): array\n", // L2
        "    {\n",                                                              // L3
        "        self::toListOfString($values);\n",                             // L4
        "    }\n",                                                              // L5
        "}\n",                                                                  // L6
    );

    open_php(&backend, &uri, text).await;

    // Click on the method NAME at the declaration site (line 2).
    // "    public static function toListOfString(..."
    // "toListOfString" starts at character 27.
    let locs = references_at(&backend, &uri, 2, 30, true).await;
    assert!(
        locs.len() >= 2,
        "Clicking on method declaration should find references; got {} locations",
        locs.len()
    );
    // Should include the call site on L4.
    let has_call = locs.iter().any(|l| l.range.start.line == 4);
    assert!(
        has_call,
        "Should include the self::toListOfString call on line 4"
    );
}

#[tokio::test]
async fn test_property_declaration_triggers_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // L0
        "class Box {\n",                        // L1
        "    public int $size = 0;\n",          // L2
        "    public function grow(): void {\n", // L3
        "        $this->size++;\n",             // L4
        "    }\n",                              // L5
        "}\n",                                  // L6
    );

    open_php(&backend, &uri, text).await;

    // Click on the property name at the declaration (line 2).
    // "    public int $size = 0;"
    // "$size" starts at character 15.
    let locs = references_at(&backend, &uri, 2, 16, true).await;
    assert!(
        locs.len() >= 2,
        "Clicking on property declaration should find references; got {} locations",
        locs.len()
    );
    let has_usage = locs.iter().any(|l| l.range.start.line == 4);
    assert!(has_usage, "Should include the $this->size usage on line 4");
}

#[tokio::test]
async fn test_constant_declaration_triggers_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // L0
        "class Limit {\n",                       // L1
        "    const MAX = 100;\n",                // L2
        "    public function check(): bool {\n", // L3
        "        return self::MAX > 0;\n",       // L4
        "    }\n",                               // L5
        "}\n",                                   // L6
    );

    open_php(&backend, &uri, text).await;

    // Click on the constant name at the declaration (line 2).
    // "    const MAX = 100;"
    // "MAX" starts at character 10.
    let locs = references_at(&backend, &uri, 2, 11, true).await;
    assert!(
        locs.len() >= 2,
        "Clicking on constant declaration should find references; got {} locations",
        locs.len()
    );
    let has_usage = locs.iter().any(|l| l.range.start.line == 4);
    assert!(has_usage, "Should include the self::MAX usage on line 4");
}

#[tokio::test]
async fn test_method_declaration_cross_file() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                                         // L0
        "class Formatter {\n",                             // L1
        "    public function format(string $s): string\n", // L2
        "    {\n",                                         // L3
        "        return $s;\n",                            // L4
        "    }\n",                                         // L5
        "}\n",                                             // L6
    );
    let text_b = concat!(
        "<?php\n",                               // L0
        "function demo(Formatter $f): void {\n", // L1
        "    $f->format('hello');\n",            // L2
        "}\n",                                   // L3
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Click on method name at the declaration in a.php (line 2).
    let locs = references_at(&backend, &uri_a, 2, 23, true).await;
    let in_b = locs.iter().filter(|l| l.uri == uri_b).count();
    assert!(
        in_b >= 1,
        "Method declaration should find cross-file call site; got {} in b.php",
        in_b
    );
}

// ─── Class-Aware Member Filtering ───────────────────────────────────────────

#[tokio::test]
async fn test_unrelated_class_same_method_excluded() {
    // Two unrelated classes with the same method name.  Find References
    // on one should NOT return results from the other.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                            // L0
        "class MyClass {\n",                                  // L1
        "    public function save(): void {}\n",              // L2
        "}\n",                                                // L3
        "class OtherClass {\n",                               // L4
        "    public function save(): void {}\n",              // L5
        "}\n",                                                // L6
        "function demo(MyClass $a, OtherClass $b): void {\n", // L7
        "    $a->save();\n",                                  // L8
        "    $b->save();\n",                                  // L9
        "}\n",                                                // L10
    );

    open_php(&backend, &uri, text).await;

    // Click on save() at L8 ($a->save(), where $a: MyClass).
    let locs = references_at(&backend, &uri, 8, 10, false).await;

    // Should include L8 ($a->save()) but NOT L9 ($b->save()).
    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&8),
        "Should find $a->save() on L8; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&9),
        "Should NOT find $b->save() on L9 (unrelated class); got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_unrelated_class_same_method_excluded_cross_file() {
    // Cross-file: two unrelated classes with the same method name.
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                               // L0
        "class MyClass {\n",                     // L1
        "    public function save(): void {}\n", // L2
        "}\n",                                   // L3
        "class OtherClass {\n",                  // L4
        "    public function save(): void {}\n", // L5
        "}\n",                                   // L6
    );
    let text_b = concat!(
        "<?php\n",                                         // L0
        "function useMyClass(MyClass $m): void {\n",       // L1
        "    $m->save();\n",                               // L2
        "}\n",                                             // L3
        "function useOtherClass(OtherClass $o): void {\n", // L4
        "    $o->save();\n",                               // L5
        "}\n",                                             // L6
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Find references to MyClass::save() from its declaration (L2 in a.php).
    // "save" starts at character 20 in "    public function save(): void {}"
    let locs = references_at(&backend, &uri_a, 2, 21, true).await;

    // b.php should have $m->save() (L2) but NOT $o->save() (L5).
    let b_lines: Vec<u32> = locs
        .iter()
        .filter(|l| l.uri == uri_b)
        .map(|l| l.range.start.line)
        .collect();
    assert!(
        b_lines.contains(&2),
        "Should find $m->save() on L2 of b.php; got lines: {:?}",
        b_lines
    );
    assert!(
        !b_lines.contains(&5),
        "Should NOT find $o->save() on L5 of b.php (unrelated class); got lines: {:?}",
        b_lines
    );

    // The declaration of OtherClass::save() (L5 in a.php) should also be excluded.
    let a_lines: Vec<u32> = locs
        .iter()
        .filter(|l| l.uri == uri_a)
        .map(|l| l.range.start.line)
        .collect();
    assert!(
        a_lines.contains(&2),
        "Should include MyClass::save() declaration on L2 of a.php; got: {:?}",
        a_lines
    );
    assert!(
        !a_lines.contains(&5),
        "Should NOT include OtherClass::save() declaration on L5 of a.php; got: {:?}",
        a_lines
    );
}

#[tokio::test]
async fn method_references_type_receivers_without_walking_unrelated_bodies() {
    // A candidate file holds the searched access in one body among
    // several.  Only that body gets its variable scopes built, so the
    // bodies around it must neither contribute their own `$item` to the
    // answer nor be needed for it.
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///a.php").unwrap();
    let uri_b = Url::parse("file:///b.php").unwrap();

    let text_a = concat!(
        "<?php\n",                               // L0
        "class Target {\n",                      // L1
        "    public function save(): void {}\n", // L2
        "}\n",                                   // L3
        "class Decoy {\n",                       // L4
        "    public function save(): void {}\n", // L5
        "    public function run(): void {}\n",  // L6
        "}\n",                                   // L7
    );
    let text_b = concat!(
        "<?php\n",                          // L0
        "class Holder {\n",                 // L1
        "    public function before() {\n", // L2
        "        $item = new Decoy();\n",   // L3
        "        $item->run();\n",          // L4
        "    }\n",                          // L5
        "    public function middle() {\n", // L6
        "        $item = new Target();\n",  // L7
        "        $item->save();\n",         // L8
        "        $item->save();\n",         // L9
        "    }\n",                          // L10
        "    public function after() {\n",  // L11
        "        $item = new Decoy();\n",   // L12
        "        $item->run();\n",          // L13
        "    }\n",                          // L14
        "}\n",                              // L15
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Find references to Target::save() from its declaration.
    let locs = references_at(&backend, &uri_a, 2, 21, false).await;

    let b_lines: Vec<u32> = locs
        .iter()
        .filter(|l| l.uri == uri_b)
        .map(|l| l.range.start.line)
        .collect();
    assert_eq!(
        b_lines,
        vec![8, 9],
        "Both `save()` calls in middle() should resolve to Target; got: {:?}",
        b_lines
    );

    // The same search for Decoy::save() finds nothing in b.php: the
    // `$item` the skipped bodies hold is a Decoy, but neither body
    // calls `save()` on it.
    let decoy_locs = references_at(&backend, &uri_a, 5, 21, false).await;
    assert!(
        !decoy_locs.iter().any(|l| l.uri == uri_b),
        "Decoy::save() has no call sites; got: {:?}",
        decoy_locs
    );
}

#[tokio::test]
async fn test_inherited_method_references_included() {
    // A child class inherits a method from its parent.  Find References
    // on the parent's method should include calls via the child.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // L0
        "class Base {\n",                             // L1
        "    public function save(): void {}\n",      // L2
        "}\n",                                        // L3
        "class Child extends Base {}\n",              // L4
        "function demo(Base $a, Child $b): void {\n", // L5
        "    $a->save();\n",                          // L6
        "    $b->save();\n",                          // L7
        "}\n",                                        // L8
    );

    open_php(&backend, &uri, text).await;

    // Click on save() at L6 ($a->save(), $a: Base).
    let locs = references_at(&backend, &uri, 6, 10, false).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&6),
        "Should find $a->save() on L6; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&7),
        "Should find $b->save() on L7 (Child extends Base); got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_interface_method_references_included() {
    // A class implements an interface.  Find References on the interface's
    // method should include calls via the implementing class.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // L0
        "interface Saveable {\n",                          // L1
        "    public function save(): void;\n",             // L2
        "}\n",                                             // L3
        "class Record implements Saveable {\n",            // L4
        "    public function save(): void {}\n",           // L5
        "}\n",                                             // L6
        "function demo(Saveable $s, Record $r): void {\n", // L7
        "    $s->save();\n",                               // L8
        "    $r->save();\n",                               // L9
        "}\n",                                             // L10
    );

    open_php(&backend, &uri, text).await;

    // Click on save() at L8 ($s->save(), $s: Saveable).
    let locs = references_at(&backend, &uri, 8, 10, false).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&8),
        "Should find $s->save() on L8; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&9),
        "Should find $r->save() on L9 (Record implements Saveable); got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_static_method_unrelated_class_excluded() {
    // Two unrelated classes with the same static method name.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // L0
        "class Alpha {\n",                                // L1
        "    public static function create(): void {}\n", // L2
        "}\n",                                            // L3
        "class Beta {\n",                                 // L4
        "    public static function create(): void {}\n", // L5
        "}\n",                                            // L6
        "function demo(): void {\n",                      // L7
        "    Alpha::create();\n",                         // L8
        "    Beta::create();\n",                          // L9
        "}\n",                                            // L10
    );

    open_php(&backend, &uri, text).await;

    // Click on create() at L8 (Alpha::create()).
    let locs = references_at(&backend, &uri, 8, 14, false).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&8),
        "Should find Alpha::create() on L8; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&9),
        "Should NOT find Beta::create() on L9 (unrelated class); got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_self_static_method_references_scoped() {
    // self:: and static:: calls should be scoped to the enclosing class.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // L0
        "class Foo {\n",                                 // L1
        "    public static function build(): void {}\n", // L2
        "    public function demo(): void {\n",          // L3
        "        self::build();\n",                      // L4
        "    }\n",                                       // L5
        "}\n",                                           // L6
        "class Bar {\n",                                 // L7
        "    public static function build(): void {}\n", // L8
        "    public function demo(): void {\n",          // L9
        "        self::build();\n",                      // L10
        "    }\n",                                       // L11
        "}\n",                                           // L12
    );

    open_php(&backend, &uri, text).await;

    // Click on build() at L4 (self::build() inside Foo).
    let locs = references_at(&backend, &uri, 4, 16, false).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&4),
        "Should find self::build() on L4 (inside Foo); got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&10),
        "Should NOT find self::build() on L10 (inside Bar, unrelated); got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_unresolvable_variable_excluded_when_member_scope_known() {
    // Once a member search has a resolved receiver scope, unresolved
    // receivers with the same member name should not be included. In large
    // projects, common methods such as `find` otherwise match unrelated
    // untyped services and repositories.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // L0
        "class MyClass {\n",                             // L1
        "    public function save(): void {}\n",         // L2
        "}\n",                                           // L3
        "function demo(MyClass $a, $unknown): void {\n", // L4
        "    $a->save();\n",                             // L5
        "    $unknown->save();\n",                       // L6
        "}\n",                                           // L7
    );

    open_php(&backend, &uri, text).await;

    // Click on save() at L5 ($a->save(), $a: MyClass).
    let locs = references_at(&backend, &uri, 5, 10, false).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&5),
        "Should find $a->save() on L5; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&6),
        "Should NOT include unresolved $unknown->save() on L6; got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_overridden_find_excludes_base_repository_and_unresolved_calls() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                          // L0
        "class ServiceEntityRepository {\n",                                // L1
        "    public function find(int $id): object {}\n",                   // L2
        "}\n",                                                              // L3
        "class NotificationRepository extends ServiceEntityRepository {\n", // L4
        "    public function find(int $id): object {}\n",                   // L5
        "}\n",                                                              // L6
        "class UserRepository extends ServiceEntityRepository {\n",         // L7
        "    public function find(int $id): object {}\n",                   // L8
        "}\n",                                                              // L9
        "function demo(NotificationRepository $notifications, ServiceEntityRepository $base, UserRepository $users, $managerRegistry, $unknown): void {\n", // L10
        "    $notifications->find(1);\n", // L11
        "    $base->find(2);\n",          // L12
        "    $users->find(3);\n",         // L13
        "    $notificationRepository = $managerRegistry->getManager()->getRepository(NotificationImpl::class);\n", // L14
        "    $notificationRepository->find(4);\n", // L15
        "    $unknown->find(5);\n",                // L16
        "}\n",                                     // L17
    );

    open_php(&backend, &uri, text).await;

    let locs = references_at(&backend, &uri, 5, 21, true).await;
    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();

    assert!(
        lines.contains(&5),
        "Should include NotificationRepository::find declaration on L5; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&11),
        "Should include $notifications->find() on L11; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&15),
        "Should NOT include unresolved $notificationRepository->find() on L15 — receivers are matched by resolved type, never by variable name; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&2),
        "Should NOT include base ServiceEntityRepository::find declaration on L2; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&8),
        "Should NOT include sibling UserRepository::find declaration on L8; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&12),
        "Should NOT include base-typed $base->find() on L12; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&13),
        "Should NOT include sibling $users->find() on L13; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&16),
        "Should NOT include unresolved $unknown->find() on L16; got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_concrete_method_references_include_interface_typed_calls() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                           // L0
        "namespace App;\n",                                                  // L1
        "class Notification {}\n",                                           // L2
        "interface NotificationGateway {\n",                                 // L3
        "    public function insert(Notification $notification): void;\n",   // L4
        "}\n",                                                               // L5
        "interface UserGateway {\n",                                         // L6
        "    public function insert(Notification $notification): void;\n",   // L7
        "}\n",                                                               // L8
        "class NotificationRepository implements NotificationGateway {\n",   // L9
        "    public function insert(Notification $notification): void {}\n", // L10
        "}\n",                                                               // L11
        "class AddNotification {\n",                                         // L12
        "    public function __construct(private readonly NotificationGateway $notificationGateway) {}\n", // L13
        "    public function execute(Notification $notification): void {\n", // L14
        "        $this->notificationGateway->insert($notification);\n",      // L15
        "    }\n",                                                           // L16
        "}\n",                                                               // L17
        "class AddUser {\n",                                                 // L18
        "    public function __construct(private readonly UserGateway $userGateway) {}\n", // L19
        "    public function execute(Notification $notification): void {\n", // L20
        "        $this->userGateway->insert($notification);\n",              // L21
        "    }\n",                                                           // L22
        "}\n",                                                               // L23
    );

    open_php(&backend, &uri, text).await;

    let locs = references_at(&backend, &uri, 10, 21, true).await;
    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();

    assert!(
        lines.contains(&4),
        "Should include NotificationGateway::insert declaration on L4; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&10),
        "Should include NotificationRepository::insert declaration on L10; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&15),
        "Should include interface-typed $notificationGateway->insert() on L15; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&7),
        "Should NOT include unrelated UserGateway::insert declaration on L7; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&21),
        "Should NOT include unrelated $userGateway->insert() on L21; got lines: {:?}",
        lines
    );
}

#[tokio::test]
async fn test_this_method_references_excludes_unrelated() {
    // $this->method() inside one class should not match $this->method()
    // inside an unrelated class with the same method name.
    // Note: $this references are currently file-local, but the member
    // reference search is cross-file.  This test checks the member
    // name filtering when triggered from a $this-> call site.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // L0
        "class Dog {\n",                              // L1
        "    public function speak(): void {}\n",     // L2
        "    public function demo(): void {\n",       // L3
        "        $this->speak();\n",                  // L4
        "    }\n",                                    // L5
        "}\n",                                        // L6
        "class Cat {\n",                              // L7
        "    public function speak(): void {}\n",     // L8
        "    public function demo(): void {\n",       // L9
        "        $this->speak();\n",                  // L10
        "    }\n",                                    // L11
        "}\n",                                        // L12
        "function outside(Dog $d, Cat $c): void {\n", // L13
        "    $d->speak();\n",                         // L14
        "    $c->speak();\n",                         // L15
        "}\n",                                        // L16
    );

    open_php(&backend, &uri, text).await;

    // Click on speak() at L14 ($d->speak(), $d: Dog).
    let locs = references_at(&backend, &uri, 14, 10, true).await;

    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&14),
        "Should find $d->speak() on L14; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&2),
        "Should include Dog::speak() declaration on L2; got lines: {:?}",
        lines
    );
    assert!(
        lines.contains(&4),
        "Should include $this->speak() inside Dog on L4; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&8),
        "Should NOT include Cat::speak() declaration on L8; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&10),
        "Should NOT include $this->speak() inside Cat on L10; got lines: {:?}",
        lines
    );
    assert!(
        !lines.contains(&15),
        "Should NOT include $c->speak() on L15 (unrelated class); got lines: {:?}",
        lines
    );
}

// ─── PHPDoc @property and @method References ────────────────────────────────

#[tokio::test]
async fn test_phpdoc_property_references_from_usage() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // L0
        "/**\n",                                // L1
        " * @property string $email\n",         // L2
        " */\n",                                // L3
        "class User {\n",                       // L4
        "    public function demo(): void {\n", // L5
        "        echo $this->email;\n",         // L6
        "    }\n",                              // L7
        "}\n",                                  // L8
        "$u = new User();\n",                   // L9
        "echo $u->email;\n",                    // L10
    );

    open_php(&backend, &uri, text).await;

    // Click on "email" at line 10 ($u->email).
    let locs = references_at(&backend, &uri, 10, 13, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references to email (declaration + 2 usages), got {}",
        locs.len()
    );

    // Should include the @property declaration (line 2).
    let has_declaration = locs.iter().any(|l| l.range.start.line == 2);
    assert!(
        has_declaration,
        "Should include the @property declaration on line 2"
    );

    // Should include the $this->email usage (line 6).
    let has_this_usage = locs.iter().any(|l| l.range.start.line == 6);
    assert!(
        has_this_usage,
        "Should include the $this->email usage on line 6"
    );

    // Should include the $u->email usage (line 10).
    let has_external_usage = locs.iter().any(|l| l.range.start.line == 10);
    assert!(
        has_external_usage,
        "Should include the $u->email usage on line 10"
    );
}

#[tokio::test]
async fn test_phpdoc_property_references_from_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // L0
        "/**\n",                                // L1
        " * @property string $email\n",         // L2
        " */\n",                                // L3
        "class User {\n",                       // L4
        "    public function demo(): void {\n", // L5
        "        echo $this->email;\n",         // L6
        "    }\n",                              // L7
        "}\n",                                  // L8
        "$u = new User();\n",                   // L9
        "echo $u->email;\n",                    // L10
    );

    open_php(&backend, &uri, text).await;

    // Click on "email" in the @property tag (line 2).
    // Line: " * @property string $email"
    // The MemberDeclaration span covers "email" (without $) starting at char 22.
    let locs = references_at(&backend, &uri, 2, 22, true).await;
    assert!(
        locs.len() >= 3,
        "Expected at least 3 references from @property declaration, got {}",
        locs.len()
    );
}

#[tokio::test]
async fn test_phpdoc_method_references_from_usage() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                        // L0
        "/**\n",                          // L1
        " * @method string getEmail()\n", // L2
        " */\n",                          // L3
        "class User {\n",                 // L4
        "}\n",                            // L5
        "$u = new User();\n",             // L6
        "echo $u->getEmail();\n",         // L7
    );

    open_php(&backend, &uri, text).await;

    // Click on "getEmail" at line 7 ($u->getEmail()).
    let locs = references_at(&backend, &uri, 7, 10, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to getEmail (declaration + usage), got {}",
        locs.len()
    );

    // Should include the @method declaration (line 2).
    let has_declaration = locs.iter().any(|l| l.range.start.line == 2);
    assert!(
        has_declaration,
        "Should include the @method declaration on line 2"
    );
}

#[tokio::test]
async fn test_phpdoc_property_references_exclude_unrelated_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // L0
        "/**\n",                        // L1
        " * @property string $email\n", // L2
        " */\n",                        // L3
        "class User {}\n",              // L4
        "/**\n",                        // L5
        " * @property int $email\n",    // L6
        " */\n",                        // L7
        "class Order {}\n",             // L8
        "$u = new User();\n",           // L9
        "echo $u->email;\n",            // L10
        "$o = new Order();\n",          // L11
        "echo $o->email;\n",            // L12
    );

    open_php(&backend, &uri, text).await;

    // Click on "email" at line 10 ($u->email).
    let locs = references_at(&backend, &uri, 10, 13, true).await;

    // Should include User's @property and $u->email, but NOT Order's @property or $o->email.
    let has_user_declaration = locs.iter().any(|l| l.range.start.line == 2);
    let has_user_usage = locs.iter().any(|l| l.range.start.line == 10);
    let has_order_declaration = locs.iter().any(|l| l.range.start.line == 6);
    let has_order_usage = locs.iter().any(|l| l.range.start.line == 12);

    assert!(
        has_user_declaration,
        "Should include User's @property declaration"
    );
    assert!(has_user_usage, "Should include $u->email usage");
    assert!(
        !has_order_declaration,
        "Should NOT include Order's @property declaration"
    );
    assert!(!has_order_usage, "Should NOT include $o->email usage");
}

#[tokio::test]
async fn test_phpdoc_property_multiple_properties() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                      // L0
        "/**\n",                        // L1
        " * @property int $id\n",       // L2
        " * @property string $email\n", // L3
        " * @property string $name\n",  // L4
        " */\n",                        // L5
        "class User {}\n",              // L6
        "$u = new User();\n",           // L7
        "echo $u->email;\n",            // L8
    );

    open_php(&backend, &uri, text).await;

    // Click on "email" at line 8 ($u->email).
    let locs = references_at(&backend, &uri, 8, 13, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to email, got {}",
        locs.len()
    );

    // Should include only the @property string $email declaration (line 3), not id or name.
    let has_email_decl = locs.iter().any(|l| l.range.start.line == 3);
    let has_id_decl = locs.iter().any(|l| l.range.start.line == 2);
    let has_name_decl = locs.iter().any(|l| l.range.start.line == 4);
    assert!(
        has_email_decl,
        "Should include @property string $email declaration"
    );
    assert!(
        !has_id_decl,
        "Should NOT include @property int $id declaration"
    );
    assert!(
        !has_name_decl,
        "Should NOT include @property string $name declaration"
    );
}

#[tokio::test]
async fn test_phpdoc_property_read_write_variants() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                          // L0
        "/**\n",                            // L1
        " * @property-read string $name\n", // L2
        " * @property-write int $age\n",    // L3
        " */\n",                            // L4
        "class User {}\n",                  // L5
        "$u = new User();\n",               // L6
        "echo $u->name;\n",                 // L7
    );

    open_php(&backend, &uri, text).await;

    // Click on "name" at line 7 ($u->name).
    let locs = references_at(&backend, &uri, 7, 13, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to name (property-read declaration + usage), got {}",
        locs.len()
    );

    // Should include the @property-read declaration (line 2).
    let has_read_decl = locs.iter().any(|l| l.range.start.line == 2);
    assert!(
        has_read_decl,
        "Should include the @property-read declaration"
    );
}

#[tokio::test]
async fn test_phpdoc_method_references_from_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                        // L0
        "/**\n",                          // L1
        " * @method string getEmail()\n", // L2
        " */\n",                          // L3
        "class User {}\n",                // L4
        "$u = new User();\n",             // L5
        "echo $u->getEmail();\n",         // L6
    );

    open_php(&backend, &uri, text).await;

    // Click on "getEmail" in the @method tag (line 2).
    // Line: " * @method string getEmail()"
    // The MemberDeclaration span covers "getEmail" starting at char 19.
    let locs = references_at(&backend, &uri, 2, 19, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references from @method declaration, got {}",
        locs.len()
    );

    let has_usage = locs.iter().any(|l| l.range.start.line == 6);
    assert!(has_usage, "Should include $u->getEmail() usage on line 6");
}

#[tokio::test]
async fn test_phpdoc_method_no_return_type_references() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                 // L0
        "/**\n",                   // L1
        " * @method getEmail()\n", // L2
        " */\n",                   // L3
        "class User {}\n",         // L4
        "$u = new User();\n",      // L5
        "echo $u->getEmail();\n",  // L6
    );

    open_php(&backend, &uri, text).await;

    // Click on "getEmail" at line 6 ($u->getEmail()).
    let locs = references_at(&backend, &uri, 6, 10, true).await;
    assert!(
        locs.len() >= 2,
        "Expected at least 2 references to getEmail (declaration + usage), got {}",
        locs.len()
    );

    // Should include the @method declaration (line 2).
    let has_declaration = locs.iter().any(|l| l.range.start.line == 2);
    assert!(
        has_declaration,
        "Should include the @method declaration on line 2"
    );
}

// ─── Constructor References ──────────────────────────────────────────

#[tokio::test]
async fn test_constructor_references_finds_instantiations() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ctor.php").unwrap();
    let text = concat!(
        "<?php\n",                                // L0
        "class Service {\n",                      // L1
        "    public function __construct() {}\n", // L2
        "}\n",                                    // L3
        "$a = new Service();\n",                  // L4
        "$b = new Service();\n",                  // L5
    );

    open_php(&backend, &uri, text).await;

    // Click on "__construct" at line 2.
    let locs = references_at(&backend, &uri, 2, 25, true).await;

    // Both `new Service()` sites should be found.
    let has_l4 = locs.iter().any(|l| l.range.start.line == 4);
    let has_l5 = locs.iter().any(|l| l.range.start.line == 5);
    assert!(
        has_l4 && has_l5,
        "Expected both `new Service()` instantiations (L4 + L5), got {:?}",
        locs
    );
}

#[tokio::test]
async fn test_constructor_references_includes_inheriting_subclass() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ctor_inherit.php").unwrap();
    let text = concat!(
        "<?php\n",                                // L0
        "class Base {\n",                         // L1
        "    public function __construct() {}\n", // L2
        "}\n",                                    // L3
        "class Child extends Base {}\n",          // L4
        "$a = new Base();\n",                     // L5
        "$b = new Child();\n",                    // L6
    );

    open_php(&backend, &uri, text).await;

    // Click on "__construct" at line 2.
    let locs = references_at(&backend, &uri, 2, 25, true).await;

    // `new Child()` inherits Base's constructor, so it counts.
    let has_base = locs.iter().any(|l| l.range.start.line == 5);
    let has_child = locs.iter().any(|l| l.range.start.line == 6);
    assert!(
        has_base && has_child,
        "Expected `new Base()` (L5) and inherited `new Child()` (L6), got {:?}",
        locs
    );
}

#[tokio::test]
async fn test_constructor_references_excludes_overriding_subclass() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ctor_override.php").unwrap();
    let text = concat!(
        "<?php\n",                                // L0
        "class Base {\n",                         // L1
        "    public function __construct() {}\n", // L2
        "}\n",                                    // L3
        "class Child extends Base {\n",           // L4
        "    public function __construct() {}\n", // L5
        "}\n",                                    // L6
        "$a = new Base();\n",                     // L7
        "$b = new Child();\n",                    // L8
    );

    open_php(&backend, &uri, text).await;

    // Click on Base's "__construct" at line 2.
    let locs = references_at(&backend, &uri, 2, 25, true).await;

    // `new Child()` invokes Child's OWN constructor, so it must be excluded.
    let has_base = locs.iter().any(|l| l.range.start.line == 7);
    let has_child = locs.iter().any(|l| l.range.start.line == 8);
    assert!(has_base, "Expected `new Base()` (L7), got {:?}", locs);
    assert!(
        !has_child,
        "`new Child()` (L8) overrides the constructor and must be excluded, got {:?}",
        locs
    );
}

#[tokio::test]
async fn test_constructor_references_finds_attribute_usage() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ctor_attr.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // L0
        "#[\\Attribute]\n",                                 // L1
        "class MyAttr {\n",                                 // L2
        "    public function __construct(int $x = 0) {}\n", // L3
        "}\n",                                              // L4
        "#[MyAttr(1)]\n",                                   // L5
        "class Target {}\n",                                // L6
    );

    open_php(&backend, &uri, text).await;

    // Click on MyAttr's "__construct" at line 3.
    let locs = references_at(&backend, &uri, 3, 25, true).await;

    // The `#[MyAttr(1)]` attribute usage on line 5 invokes the constructor.
    let has_attr_usage = locs.iter().any(|l| l.range.start.line == 5);
    assert!(
        has_attr_usage,
        "Expected the `#[MyAttr(1)]` attribute usage (L5) to be a constructor reference, got {:?}",
        locs
    );
}
// ─── Property-receiver resolution (psysh corpus shapes) ─────────────────────
//
// Each receiver property is named `$ctx` (not `$holder`/`$context`) so the
// deleted name-vs-class-name fallback could never have matched; a found
// reference proves the receiver's type genuinely resolved.

#[tokio::test]
async fn test_member_references_through_property_receivers() {
    let backend = create_test_backend();
    let uri_ctx = Url::parse("file:///Context.php").unwrap();
    let uri_use = Url::parse("file:///users.php").unwrap();

    let text_ctx = concat!(
        "<?php\n",                                 // L0
        "namespace Psy;\n",                        // L1
        "class Context {\n",                       // L2
        "    public function getAll(): array {\n", // L3
        "        return [];\n",                    // L4
        "    }\n",                                 // L5
        "}\n",                                     // L6
    );
    let text_use = concat!(
        "<?php\n",                                           // L0
        "namespace Psy\\Sub;\n",                             // L1
        "use Psy\\Context;\n",                               // L2
        "class NativeTyped {\n",                             // L3
        "    private Context $ctx;\n",                       // L4
        "    public function go(): array {\n",               // L5
        "        return $this->ctx->getAll();\n",            // L6
        "    }\n",                                           // L7
        "}\n",                                               // L8
        "class DocblockTyped {\n",                           // L9
        "    /** @var Context */\n",                         // L10
        "    protected $ctx;\n",                             // L11
        "    public function go(): array {\n",               // L12
        "        return $this->ctx->getAll();\n",            // L13
        "    }\n",                                           // L14
        "}\n",                                               // L15
        "class CtorAssigned {\n",                            // L16
        "    private $ctx;\n",                               // L17
        "    public function __construct(Context $ctx) {\n", // L18
        "        $this->ctx = $ctx;\n",                      // L19
        "    }\n",                                           // L20
        "    public function go(): array {\n",               // L21
        "        return $this->ctx->getAll();\n",            // L22
        "    }\n",                                           // L23
        "}\n",                                               // L24
        "class SetterAssigned {\n",                          // L25
        "    protected $ctx;\n",                             // L26
        "    public function setContext(Context $ctx) {\n",  // L27
        "        $this->ctx = $ctx;\n",                      // L28
        "    }\n",                                           // L29
        "    public function go(): array {\n",               // L30
        "        return $this->ctx->getAll();\n",            // L31
        "    }\n",                                           // L32
        "}\n",                                               // L33
    );

    open_php(&backend, &uri_ctx, text_ctx).await;
    open_php(&backend, &uri_use, text_use).await;

    // Find references to getAll() from its declaration.
    let locs = references_at(&backend, &uri_ctx, 3, 21, false).await;
    let call_lines: Vec<u32> = locs
        .iter()
        .filter(|l| l.uri == uri_use)
        .map(|l| l.range.start.line)
        .collect();
    for expected in [6u32, 13, 22, 31] {
        assert!(
            call_lines.contains(&expected),
            "expected getAll() reference at users.php line {expected}, got {call_lines:?}"
        );
    }
}

/// A member reached through a value read out of the Reflection API is a
/// reference like any other, once the reflected read types.
///
/// The receiver is `$value`, so the deleted name-vs-class-name fallback
/// could not have matched `Shell` on spelling; the hit proves the type
/// travelled from `Configuration::$shell` through `getProperty('shell')`
/// and `getValue()`.
#[tokio::test]
async fn test_const_reference_through_a_reflected_property_read() {
    let backend = create_test_backend_with_full_stubs();
    let uri_shell = Url::parse("file:///Shell.php").unwrap();
    let uri_config = Url::parse("file:///Configuration.php").unwrap();
    let uri_use = Url::parse("file:///probe.php").unwrap();

    let text_shell = concat!(
        "<?php\n",                     // L0
        "namespace Psy;\n",            // L1
        "class Shell {\n",             // L2
        "    const VERSION = 'v1';\n", // L3
        "}\n",                         // L4
    );
    let text_config = concat!(
        "<?php\n",                             // L0
        "namespace Psy;\n",                    // L1
        "class Configuration {\n",             // L2
        "    private ?Shell $shell = null;\n", // L3
        "}\n",                                 // L4
    );
    let text_use = concat!(
        "<?php\n",                                         // L0
        "namespace Psy;\n",                                // L1
        "function probe(Configuration $config): void {\n", // L2
        "    $refl = new \\ReflectionObject($config);\n",  // L3
        "    $reflected = $refl->getProperty('shell');\n", // L4
        "    $value = $reflected->getValue($config);\n",   // L5
        "    echo $value::VERSION;\n",                     // L6
        "}\n",                                             // L7
    );

    open_php(&backend, &uri_shell, text_shell).await;
    open_php(&backend, &uri_config, text_config).await;
    open_php(&backend, &uri_use, text_use).await;

    let locs = references_at(&backend, &uri_shell, 3, 11, false).await;
    let lines: Vec<u32> = locs
        .iter()
        .filter(|l| l.uri == uri_use)
        .map(|l| l.range.start.line)
        .collect();
    assert!(
        lines.contains(&6),
        "expected the VERSION read on probe.php line 6 to be a reference, got {lines:?}"
    );
}
#[tokio::test]
async fn function_references_match_a_call_spelled_in_another_case() {
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

    let locs = references_at(&backend, &uri_a, 1, 10, false).await;
    let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    assert_eq!(
        lines,
        vec![3, 4],
        "PHP resolves function names case-insensitively, so HELPER() calls helper()"
    );
}

#[tokio::test]
async fn function_references_from_a_use_function_import_reach_its_call_sites() {
    let backend = create_test_backend();
    let uri_a = Url::parse("file:///helpers.php").unwrap();
    let uri_b = Url::parse("file:///main.php").unwrap();

    let text_a = concat!(
        "<?php\n",                     // L0
        "namespace Support;\n",        // L1
        "function shout(): void {}\n", // L2
    );
    let text_b = concat!(
        "<?php\n",                        // L0
        "namespace App;\n",               // L1
        "use function Support\\shout;\n", // L2
        "function demo(): void {\n",      // L3
        "    shout();\n",                 // L4
        "}\n",                            // L5
    );

    open_php(&backend, &uri_a, text_a).await;
    open_php(&backend, &uri_b, text_b).await;

    // Started from the import, whose span text is the qualified name.
    let locs = references_at(&backend, &uri_b, 2, 22, false).await;
    assert!(
        locs.iter()
            .any(|l| l.uri == uri_b && l.range.start.line == 4),
        "expected the shout() call site, got {locs:?}"
    );
}

/// A package class that implements the interface is absent from the reverse
/// inheritance index until something parses its file, so a search scoped to
/// that index alone drops every access on it.  The scope has to settle a
/// receiver by walking up from the receiver's own class, so the search
/// answers the same whether or not the session happened to load the package
/// before it ran.
#[tokio::test]
async fn member_references_reach_an_implementor_nothing_has_parsed() {
    let installed_json = r#"{"packages": [{
        "name": "acme/services",
        "version": "1.0.0",
        "install-path": "../acme/services",
        "autoload": {"psr-4": {"Acme\\": ""}}
    }]}"#;

    let (backend, dir) = crate::common::create_psr4_workspace(
        r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#,
        &[
            ("vendor/composer/installed.json", installed_json),
            (
                "vendor/acme/services/ServiceInterface.php",
                r#"<?php

namespace Acme;

interface ServiceInterface
{
    public function handle(): void;
}
"#,
            ),
            (
                "vendor/acme/services/PackageService.php",
                r#"<?php

namespace Acme;

class PackageService implements ServiceInterface
{
    public function handle(): void {}
}
"#,
            ),
            (
                "src/AppService.php",
                r#"<?php

namespace App;

use Acme\ServiceInterface;

class AppService implements ServiceInterface
{
    public function handle(): void {}
}
"#,
            ),
            (
                "src/Consumer.php",
                r#"<?php

namespace App;

use Acme\PackageService;

class Consumer
{
    public function run(PackageService $service): void
    {
        $service->handle();
    }
}
"#,
            ),
        ],
    );

    // The vendor scan files the package's classes under their paths without
    // parsing them, the way a dependency is known before anything needs it.
    backend
        .initialized(tower_lsp::lsp_types::InitializedParams {})
        .await;

    let declaration_path = dir.path().join("src/AppService.php");
    let declaration_uri = Url::from_file_path(&declaration_path).unwrap().to_string();
    let declaration_content = std::fs::read_to_string(&declaration_path).unwrap();
    open_file(&backend, &declaration_uri, &declaration_content);

    let consumer_uri = Url::from_file_path(dir.path().join("src/Consumer.php")).unwrap();

    // `handle` in `public function handle(): void {}`.
    let results = backend
        .find_references(
            &declaration_uri,
            &declaration_content,
            Position::new(8, 20),
            true,
        )
        .expect("should find method references");

    assert!(
        results
            .iter()
            .any(|loc| loc.uri == consumer_uri && loc.range.start.line == 10),
        "expected the call on the package implementor, got {results:?}"
    );
}
