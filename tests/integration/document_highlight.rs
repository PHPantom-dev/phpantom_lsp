use crate::common::create_test_backend;
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

/// Helper: open a file, trigger document highlight at a position, and return results.
fn highlight_at(
    backend: &Backend,
    uri: &str,
    php: &str,
    line: u32,
    character: u32,
) -> Vec<DocumentHighlight> {
    backend.update_ast(uri, php);
    backend
        .handle_document_highlight(uri, php, Position { line, character })
        .unwrap_or_default()
}

/// Shorthand to check that a highlight has the expected line, start col, end col, and kind.
fn assert_highlight(
    h: &DocumentHighlight,
    line: u32,
    start_char: u32,
    end_char: u32,
    kind: DocumentHighlightKind,
) {
    assert_eq!(
        h.range.start.line, line,
        "expected line {}, got {}",
        line, h.range.start.line
    );
    assert_eq!(
        h.range.start.character, start_char,
        "expected start char {}, got {}",
        start_char, h.range.start.character
    );
    assert_eq!(
        h.range.end.character, end_char,
        "expected end char {}, got {}",
        end_char, h.range.end.character
    );
    assert_eq!(
        h.kind,
        Some(kind),
        "expected kind {:?}, got {:?}",
        kind,
        h.kind
    );
}

// ─── Variable highlighting ──────────────────────────────────────────────────

#[test]
fn highlight_variable_in_same_scope() {
    let backend = create_test_backend();
    let php = r#"<?php
function demo() {
    $user = new User();
    echo $user->name;
    return $user;
}
"#;

    // Cursor on `$user` at line 2 (the assignment)
    let highlights = highlight_at(&backend, "file:///test.php", php, 2, 5);

    assert_eq!(highlights.len(), 3);
    // Line 2: $user = ... (write)
    assert_highlight(&highlights[0], 2, 4, 9, DocumentHighlightKind::WRITE);
    // Line 3: echo $user->name (read)
    assert_highlight(&highlights[1], 3, 9, 14, DocumentHighlightKind::READ);
    // Line 4: return $user (read)
    assert_highlight(&highlights[2], 4, 11, 16, DocumentHighlightKind::READ);
}

#[test]
fn highlight_variable_scoped_to_function() {
    let backend = create_test_backend();
    let php = r#"<?php
function foo() {
    $x = 1;
    return $x;
}
function bar() {
    $x = 2;
    return $x;
}
"#;

    // Cursor on `$x` in foo() — should only highlight within foo
    let highlights = highlight_at(&backend, "file:///test.php", php, 2, 5);
    assert_eq!(highlights.len(), 2);
    assert_eq!(highlights[0].range.start.line, 2);
    assert_eq!(highlights[1].range.start.line, 3);
}

#[test]
fn highlight_variable_parameter_is_write() {
    let backend = create_test_backend();
    let php = r#"<?php
function greet(string $name) {
    echo $name;
}
"#;

    let highlights = highlight_at(&backend, "file:///test.php", php, 2, 10);

    // Should have at least the parameter def (write) and usage (read)
    assert!(highlights.len() >= 2);

    let has_write = highlights
        .iter()
        .any(|h| h.kind == Some(DocumentHighlightKind::WRITE));
    let has_read = highlights
        .iter()
        .any(|h| h.kind == Some(DocumentHighlightKind::READ));
    assert!(
        has_write,
        "expected a WRITE highlight for the parameter definition"
    );
    assert!(has_read, "expected a READ highlight for the variable usage");
}

// ─── Class name highlighting ────────────────────────────────────────────────

#[test]
fn highlight_class_references() {
    let backend = create_test_backend();
    let php = r#"<?php
class Foo {
    public function bar(): Foo {
        return new Foo();
    }
}
"#;

    // Cursor on `Foo` in the return type (line 2)
    let highlights = highlight_at(&backend, "file:///test.php", php, 2, 28);

    // Should match: class declaration, return type, new expression
    assert!(
        highlights.len() >= 2,
        "expected at least 2 highlights for class Foo, got {}",
        highlights.len()
    );

    // All should be READ kind (class declarations are still READ for highlighting purposes)
    for h in &highlights {
        assert_eq!(h.kind, Some(DocumentHighlightKind::READ));
    }
}

#[test]
fn highlight_class_declaration_highlights_all_references() {
    let backend = create_test_backend();
    let php = r#"<?php
class MyService {
    public static function create(): MyService {
        return new MyService();
    }
}
"#;

    // Cursor on the class declaration name
    let highlights = highlight_at(&backend, "file:///test.php", php, 1, 7);
    assert!(
        highlights.len() >= 2,
        "expected class declaration to highlight references too, got {}",
        highlights.len()
    );
}

// ─── Member highlighting ────────────────────────────────────────────────────

#[test]
fn highlight_method_accesses() {
    let backend = create_test_backend();
    let php = r#"<?php
class Calculator {
    public function add(int $a): int { return $a; }
    public function demo() {
        $this->add(1);
        $this->add(2);
    }
}
"#;

    // Cursor on `add` in `$this->add(1)` (line 4)
    let highlights = highlight_at(&backend, "file:///test.php", php, 4, 16);

    // Declaration + two usages
    assert!(
        highlights.len() >= 2,
        "expected at least 2 highlights for method 'add', got {}",
        highlights.len()
    );
}

#[test]
fn highlight_member_declaration_matches_accesses() {
    let backend = create_test_backend();
    let php = r#"<?php
class Dog {
    public string $name;
    public function greet() {
        echo $this->name;
    }
}
"#;

    // Cursor on the property access `name` in $this->name
    let highlights = highlight_at(&backend, "file:///test.php", php, 4, 21);

    assert!(
        !highlights.is_empty(),
        "expected at least 1 highlight for property 'name', got {}",
        highlights.len()
    );
}

// ─── Function highlighting ──────────────────────────────────────────────────

#[test]
fn highlight_function_calls() {
    let backend = create_test_backend();
    let php = r#"<?php
function helper() {}
helper();
helper();
"#;

    // Cursor on first call to `helper()` at line 2
    let highlights = highlight_at(&backend, "file:///test.php", php, 2, 1);

    assert!(
        highlights.len() >= 2,
        "expected at least 2 highlights for function 'helper', got {}",
        highlights.len()
    );
    for h in &highlights {
        assert_eq!(h.kind, Some(DocumentHighlightKind::READ));
    }
}

// ─── Constant highlighting ──────────────────────────────────────────────────

#[test]
fn highlight_constant_references() {
    let backend = create_test_backend();
    let php = r#"<?php
class Config {
    const MAX = 100;
    public function check(int $v): bool {
        return $v < self::MAX;
    }
}
"#;

    // Cursor on `MAX` in `self::MAX` (line 4)
    let highlights = highlight_at(&backend, "file:///test.php", php, 4, 26);

    assert!(
        !highlights.is_empty(),
        "expected at least 1 highlight for constant 'MAX', got {}",
        highlights.len()
    );
}

// ─── $this highlighting ─────────────────────────────────────────────────────

#[test]
fn highlight_this_keyword() {
    let backend = create_test_backend();
    let php = r#"<?php
class Example {
    private int $x;
    public function setX(int $v): void {
        $this->x = $v;
    }
    public function getX(): int {
        return $this->x;
    }
}
"#;

    // Cursor on `$this` at line 4
    let highlights = highlight_at(&backend, "file:///test.php", php, 4, 9);

    assert!(
        highlights.len() >= 2,
        "expected at least 2 highlights for $this, got {}",
        highlights.len()
    );
    for h in &highlights {
        assert_eq!(h.kind, Some(DocumentHighlightKind::READ));
    }
}

// ─── self/static/parent highlighting ────────────────────────────────────────

#[test]
fn highlight_self_keyword() {
    let backend = create_test_backend();
    let php = r#"<?php
class Counter {
    private static int $count = 0;
    public static function increment(): void {
        self::$count++;
    }
    public static function get(): int {
        return self::$count;
    }
}
"#;

    // Cursor on `self` at line 4
    let highlights = highlight_at(&backend, "file:///test.php", php, 4, 9);

    assert!(
        highlights.len() >= 2,
        "expected at least 2 highlights for self keyword, got {}",
        highlights.len()
    );
}

// ─── Edge cases ─────────────────────────────────────────────────────────────

#[test]
fn highlight_returns_none_on_whitespace() {
    let backend = create_test_backend();
    let php = r#"<?php
function foo() {}
"#;

    // Cursor on whitespace
    let result = {
        backend.update_ast("file:///test.php", php);
        backend.handle_document_highlight(
            "file:///test.php",
            php,
            Position {
                line: 0,
                character: 0,
            },
        )
    };

    assert!(
        result.is_none(),
        "expected None when cursor is on non-navigable token"
    );
}

#[test]
fn highlight_foreach_variable() {
    let backend = create_test_backend();
    let php = r#"<?php
function process(array $items) {
    foreach ($items as $item) {
        echo $item;
    }
}
"#;

    // Cursor on `$item` in foreach binding (line 2)
    let highlights = highlight_at(&backend, "file:///test.php", php, 2, 24);

    assert!(
        highlights.len() >= 2,
        "expected at least 2 highlights for $item, got {}",
        highlights.len()
    );

    let has_write = highlights
        .iter()
        .any(|h| h.kind == Some(DocumentHighlightKind::WRITE));
    assert!(has_write, "foreach binding should be a WRITE highlight");
}

#[test]
fn highlight_variable_assignment_is_write() {
    let backend = create_test_backend();
    let php = r#"<?php
function test() {
    $val = 1;
    $val = 2;
    echo $val;
}
"#;

    let highlights = highlight_at(&backend, "file:///test.php", php, 2, 5);

    assert_eq!(highlights.len(), 3);
    // First two are assignments (WRITE), last is usage (READ)
    assert_highlight(&highlights[0], 2, 4, 8, DocumentHighlightKind::WRITE);
    assert_highlight(&highlights[1], 3, 4, 8, DocumentHighlightKind::WRITE);
    assert_highlight(&highlights[2], 4, 9, 13, DocumentHighlightKind::READ);
}

#[test]
fn highlight_static_method() {
    let backend = create_test_backend();
    let php = r#"<?php
class Factory {
    public static function make(): self { return new self(); }
    public function demo() {
        self::make();
        static::make();
    }
}
"#;

    // Cursor on `make` at line 4
    let highlights = highlight_at(&backend, "file:///test.php", php, 4, 15);

    // Declaration + two calls
    assert!(
        highlights.len() >= 2,
        "expected at least 2 highlights for static method 'make', got {}",
        highlights.len()
    );
}

// ─── Semi-reserved keywords used as member names ────────────────────────────

#[test]
fn highlight_member_named_like_a_keyword() {
    let backend = create_test_backend();
    let uri = "file:///highlight_keyword_member.php";
    let php = concat!(
        "<?php\n",
        "class Node {\n",
        "    public string $class = '';\n",
        "}\n",
        "function test(Node $n): void {\n",
        "    $n->class = 'a';\n",
        "    echo $n->class;\n",
        "}\n",
    );

    let highlights = highlight_at(&backend, uri, php, 6, 14);
    let mut spans: Vec<(u32, u32, u32)> = highlights
        .iter()
        .map(|h| {
            (
                h.range.start.line,
                h.range.start.character,
                h.range.end.character,
            )
        })
        .collect();
    spans.sort_unstable();
    assert!(
        spans.contains(&(5, 8, 13)) && spans.contains(&(6, 13, 18)),
        "both `->class` accesses should highlight, got: {spans:?}"
    );
}
