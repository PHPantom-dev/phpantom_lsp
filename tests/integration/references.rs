use crate::common::create_test_backend;
use tower_lsp::lsp_types::Position;

use std::sync::Arc;

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

// ─── Member access tests ────────────────────────────────────────────────────

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

#[test]
fn workspace_index_refreshes_after_new_file_is_added() {
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

    let initial_results = backend
        .find_references(&item_uri, &item_content, Position::new(3, 6), true)
        .expect("should find initial class references");

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
    let refreshed_results = backend
        .find_references(&item_uri, &item_content, Position::new(3, 6), true)
        .expect("should find refreshed class references");

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
