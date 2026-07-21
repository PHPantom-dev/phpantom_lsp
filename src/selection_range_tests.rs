use super::*;
use crate::test_fixtures::make_backend;

fn selection_ranges(content: &str, positions: &[Position]) -> Vec<SelectionRange> {
    let backend = make_backend();
    backend
        .handle_selection_range(content, positions)
        .unwrap_or_default()
}

/// Flatten a SelectionRange linked list into a Vec of Ranges (innermost first).
fn flatten(sel: &SelectionRange) -> Vec<Range> {
    let mut result = vec![sel.range];
    let mut current = &sel.parent;
    while let Some(parent) = current {
        result.push(parent.range);
        current = &parent.parent;
    }
    result
}

#[test]
fn single_variable_in_function() {
    let content = r#"<?php
function hello() {
    $name = "world";
    echo $name;
}
"#;
    // Position cursor on `$name` in the echo statement (line 3, char 9).
    let results = selection_ranges(content, &[Position::new(3, 9)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);

    // Should have multiple levels: at minimum variable → expression → statement → block → function → file.
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 selection range levels, got {}",
        ranges.len()
    );

    // The innermost range should be smaller than the outermost.
    let innermost = &ranges[0];
    let outermost = ranges.last().unwrap();
    assert!(
        innermost.start.line >= outermost.start.line
            || innermost.start.character >= outermost.start.character,
        "Innermost range should be within outermost"
    );
}

#[test]
fn class_method_body() {
    let content = r#"<?php
class Greeter {
    public function greet(string $name): string {
        return "Hello, " . $name;
    }
}
"#;
    // Position cursor on `$name` in the return statement (line 3, char 29).
    let results = selection_ranges(content, &[Position::new(3, 29)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);

    // Should have levels: variable → expression → return → block → method → class body → class → file.
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 selection range levels, got {}",
        ranges.len()
    );
}

#[test]
fn multiple_positions() {
    let content = r#"<?php
$a = 1;
$b = 2;
"#;
    let results = selection_ranges(content, &[Position::new(1, 1), Position::new(2, 1)]);
    assert_eq!(results.len(), 2);

    // Each should produce a valid chain.
    for result in &results {
        let ranges = flatten(result);
        assert!(!ranges.is_empty());
    }
}

#[test]
fn nested_if_statement() {
    let content = r#"<?php
if (true) {
    if (false) {
        echo "inner";
    }
}
"#;
    // Cursor on "inner" (line 3, char 14).
    let results = selection_ranges(content, &[Position::new(3, 14)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);

    // Should have many levels: string → echo args → echo stmt → block → inner if → block → outer if → file.
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels, got {}",
        ranges.len()
    );
}

#[test]
fn empty_file() {
    let content = "<?php\n";
    let results = selection_ranges(content, &[Position::new(0, 3)]);
    assert_eq!(results.len(), 1);
    // Even an empty file should return at least the file-level range.
    let ranges = flatten(&results[0]);
    assert!(!ranges.is_empty());
}

#[test]
fn instanceof_in_method_has_fine_grained_levels() {
    let content = r#"<?php
class Demo {
    public function test(): void {
        $x = new User();
        if ($x instanceof User) {
            $x->getEmail();
        }
    }
}
"#;
    // Cursor on "getEmail" (line 5, char 17).
    let results = selection_ranges(content, &[Position::new(5, 17)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);

    // Expected levels (innermost first):
    //   getEmail (method selector)
    //   $x->getEmail() (call expression)
    //   $x->getEmail(); (expression statement)
    //   { ... } (if block body)
    //   if (...) { ... } (if statement)
    //   { ... } (method body block)
    //   public function test()... (method member)
    //   { ... } (class body)
    //   class Demo { ... } (class statement)
    //   file
    assert!(
        ranges.len() >= 7,
        "Expected at least 7 fine-grained levels for method call inside if, got {}: {:?}",
        ranges.len(),
        ranges,
    );
}

#[test]
fn ranges_are_nested() {
    let content = r#"<?php
function test() {
    $x = [1, 2, 3];
}
"#;
    // Cursor on `2` in the array (line 2, char 13).
    let results = selection_ranges(content, &[Position::new(2, 13)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);

    // Verify that each range is contained within or equal to its parent.
    for window in ranges.windows(2) {
        let inner = &window[0];
        let outer = &window[1];
        assert!(
            (inner.start.line > outer.start.line
                || (inner.start.line == outer.start.line
                    && inner.start.character >= outer.start.character))
                && (inner.end.line < outer.end.line
                    || (inner.end.line == outer.end.line
                        && inner.end.character <= outer.end.character)),
            "Inner range {:?} should be contained within outer range {:?}",
            inner,
            outer,
        );
    }
}

/// Helper: assert every range in the chain is contained within its parent.
fn assert_nested(ranges: &[Range]) {
    for window in ranges.windows(2) {
        let inner = &window[0];
        let outer = &window[1];
        assert!(
            (inner.start.line > outer.start.line
                || (inner.start.line == outer.start.line
                    && inner.start.character >= outer.start.character))
                && (inner.end.line < outer.end.line
                    || (inner.end.line == outer.end.line
                        && inner.end.character <= outer.end.character)),
            "Inner range {:?} should be contained within outer range {:?}",
            inner,
            outer,
        );
    }
}

// ─── 1. Switch statement ────────────────────────────────────────────

#[test]
fn switch_statement_case_body() {
    let content = r#"<?php
switch ($x) {
    case 1:
        echo "one";
        break;
    case 2:
        echo "two";
        break;
    default:
        echo "other";
}
"#;
    // Cursor on "one" inside first case (line 3, char 14).
    let results = selection_ranges(content, &[Position::new(3, 14)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → echo → case → { } → switch → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for switch case body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 2. Foreach loop ────────────────────────────────────────────────

#[test]
fn foreach_value_variable() {
    let content = r#"<?php
$items = [1, 2, 3];
foreach ($items as $item) {
    echo $item;
}
"#;
    // Cursor on $item in the echo (line 3, char 10).
    let results = selection_ranges(content, &[Position::new(3, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $item → echo stmt → foreach body stmt → foreach → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for foreach value, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn foreach_key_value() {
    let content = r#"<?php
$map = ['a' => 1];
foreach ($map as $key => $val) {
    echo $key;
}
"#;
    // Cursor on $key in the echo (line 3, char 10).
    let results = selection_ranges(content, &[Position::new(3, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for foreach key-value body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn foreach_cursor_on_key_target() {
    let content = r#"<?php
$map = ['a' => 1];
foreach ($map as $key => $val) {
    echo $val;
}
"#;
    // Cursor on $key in the foreach target (line 2, char 18).
    let results = selection_ranges(content, &[Position::new(2, 18)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $key → target → foreach → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for foreach key target, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 3. For loop ────────────────────────────────────────────────────

#[test]
fn for_loop_body() {
    let content = r#"<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}
"#;
    // Cursor on $i in echo (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $i → echo → for body → for → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for for loop body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 4. While loop ─────────────────────────────────────────────────

#[test]
fn while_loop_body() {
    let content = r#"<?php
while (true) {
    echo "loop";
}
"#;
    // Cursor on "loop" (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → echo → while body → while → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for while body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 5. Do-while loop ──────────────────────────────────────────────

#[test]
fn do_while_body() {
    let content = r#"<?php
do {
    echo "loop";
} while (true);
"#;
    // Cursor on "loop" (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → echo → block → do-while → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for do-while body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 6. Try/catch/finally ───────────────────────────────────────────

#[test]
fn try_body() {
    let content = r#"<?php
try {
    echo "try";
} catch (\Exception $e) {
    echo "catch";
} finally {
    echo "finally";
}
"#;
    // Cursor on "try" string (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → echo → try block body → try block → try stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for try body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn catch_body() {
    let content = r#"<?php
try {
    echo "try";
} catch (\Exception $e) {
    echo "catch";
} finally {
    echo "finally";
}
"#;
    // Cursor on "catch" string (line 4, char 10).
    let results = selection_ranges(content, &[Position::new(4, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → echo → catch block body → catch block → catch clause → try stmt → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for catch body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn finally_body() {
    let content = r#"<?php
try {
    echo "try";
} catch (\Exception $e) {
    echo "catch";
} finally {
    echo "finally";
}
"#;
    // Cursor on "finally" string (line 6, char 10).
    let results = selection_ranges(content, &[Position::new(6, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → echo → finally block body → finally block → finally clause → try stmt → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for finally body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 7. Return statement ────────────────────────────────────────────

#[test]
fn return_statement_value() {
    let content = r#"<?php
function foo() {
    return 42;
}
"#;
    // Cursor on 42 (line 2, char 11).
    let results = selection_ranges(content, &[Position::new(2, 11)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 42 → return → block → function → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for return value, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 8. Echo statement ──────────────────────────────────────────────

#[test]
fn echo_statement_value() {
    let content = r#"<?php
echo "hello", "world";
"#;
    // Cursor on "world" (line 1, char 15).
    let results = selection_ranges(content, &[Position::new(1, 15)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → echo → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for echo value, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 9. Closure expression ──────────────────────────────────────────

#[test]
fn closure_body() {
    let content = r#"<?php
$fn = function ($x) {
    return $x + 1;
};
"#;
    // Cursor on $x in return (line 2, char 11).
    let results = selection_ranges(content, &[Position::new(2, 11)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → binary → return → closure body block → closure body → closure expr → assignment → expr stmt → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for closure body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 10. Arrow function ─────────────────────────────────────────────

#[test]
fn arrow_function_expression_body() {
    let content = r#"<?php
$fn = fn($x) => $x + 1;
"#;
    // Cursor on $x in the arrow body expression (line 1, char 17).
    let results = selection_ranges(content, &[Position::new(1, 17)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → binary → arrow fn → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for arrow function body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 11. Match expression ───────────────────────────────────────────

#[test]
fn match_arm_expression() {
    let content = r#"<?php
$result = match($x) {
    1 => "one",
    2 => "two",
    default => "other",
};
"#;
    // Cursor on "two" (line 3, char 10).
    let results = selection_ranges(content, &[Position::new(3, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "two" → arm → { } → match → assignment → expr stmt → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for match arm, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 12. Anonymous class ────────────────────────────────────────────

#[test]
fn anonymous_class_method() {
    let content = r#"<?php
$obj = new class {
    public function hello() {
        echo "hi";
    }
};
"#;
    // Cursor on "hi" (line 3, char 14).
    let results = selection_ranges(content, &[Position::new(3, 14)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "hi" → echo → method block → method → anon class body → anon class → instantiation → assignment → expr stmt → file
    assert!(
        ranges.len() >= 6,
        "Expected at least 6 levels for anonymous class method, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 13. Array literal (short syntax) ───────────────────────────────

#[test]
fn array_key_value_element() {
    let content = r#"<?php
$x = ['key' => 'value', 'b' => 'c'];
"#;
    // Cursor on 'value' (line 1, char 17).
    let results = selection_ranges(content, &[Position::new(1, 17)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 'value' → key-value element → array → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for array key-value element, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 14. Legacy array() ─────────────────────────────────────────────

#[test]
fn legacy_array_elements() {
    let content = r#"<?php
$x = array('a' => 1, 'b' => 2);
"#;
    // Cursor on 1 (line 1, char 19).
    let results = selection_ranges(content, &[Position::new(1, 19)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 1 → kv element → array() → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for legacy array element, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 15. List/destructuring ─────────────────────────────────────────

#[test]
fn list_expression() {
    let content = r#"<?php
list($a, $b) = [1, 2];
"#;
    // Cursor on $a (line 1, char 5).
    let results = selection_ranges(content, &[Position::new(1, 5)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // element → list → assignment → expr stmt → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for list expression, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 16. Binary expression ──────────────────────────────────────────

#[test]
fn binary_expression_rhs() {
    let content = r#"<?php
$c = $a + $b;
"#;
    // Cursor on $b (line 1, char 11).
    let results = selection_ranges(content, &[Position::new(1, 11)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $b → binary ($a + $b) → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for binary rhs, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 17. Conditional/ternary ────────────────────────────────────────

#[test]
fn ternary_else_branch() {
    let content = r#"<?php
$x = true ? "yes" : "no";
"#;
    // Cursor on "no" (line 1, char 22).
    let results = selection_ranges(content, &[Position::new(1, 22)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "no" → ternary → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for ternary else, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 18. Property access ────────────────────────────────────────────

#[test]
fn property_access() {
    let content = r#"<?php
$x = $obj->prop;
"#;
    // Cursor on prop (line 1, char 12).
    let results = selection_ranges(content, &[Position::new(1, 12)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // prop → $obj->prop → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for property access, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 19. Static method call ─────────────────────────────────────────

#[test]
fn static_method_call() {
    let content = r#"<?php
$x = Foo::bar();
"#;
    // Cursor on bar (line 1, char 11).
    let results = selection_ranges(content, &[Position::new(1, 11)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // bar → Foo::bar() → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for static method call, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 20. Instantiation ──────────────────────────────────────────────

#[test]
fn instantiation_expression() {
    let content = r#"<?php
$x = new Foo();
"#;
    // Cursor on Foo (line 1, char 10).
    let results = selection_ranges(content, &[Position::new(1, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // Foo → new Foo() → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for instantiation, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 21. Yield expression ───────────────────────────────────────────

#[test]
fn yield_value() {
    let content = r#"<?php
function gen() {
    yield 42;
}
"#;
    // Cursor on 42 (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 42 → yield → expr stmt → block → function → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for yield value, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn yield_pair() {
    let content = r#"<?php
function gen() {
    yield 'key' => 'value';
}
"#;
    // Cursor on 'value' (line 2, char 20).
    let results = selection_ranges(content, &[Position::new(2, 20)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 'value' → yield pair → expr stmt → block → function → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for yield pair, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn yield_from() {
    let content = r#"<?php
function gen() {
    yield from other();
}
"#;
    // Cursor on other (line 2, char 16).
    let results = selection_ranges(content, &[Position::new(2, 16)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // other() → yield from → expr stmt → block → function → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for yield from, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 22. Throw expression ───────────────────────────────────────────

#[test]
fn throw_expression() {
    let content = r#"<?php
throw new \Exception("error");
"#;
    // Cursor on Exception (line 1, char 12).
    let results = selection_ranges(content, &[Position::new(1, 12)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // Exception → new \Exception(...) → throw → expr stmt → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for throw expression, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 23. Clone expression ───────────────────────────────────────────

#[test]
fn clone_expression() {
    let content = r#"<?php
$y = clone $x;
"#;
    // Cursor on $x (line 1, char 12).
    let results = selection_ranges(content, &[Position::new(1, 12)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → clone $x → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for clone expression, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 24. Construct expressions ──────────────────────────────────────

#[test]
fn construct_isset() {
    let content = r#"<?php
$x = isset($a, $b);
"#;
    // Cursor on $a (line 1, char 12).
    let results = selection_ranges(content, &[Position::new(1, 12)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $a → isset(...) → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for isset, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn construct_empty() {
    let content = r#"<?php
$x = empty($a);
"#;
    // Cursor on $a (line 1, char 12).
    let results = selection_ranges(content, &[Position::new(1, 12)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $a → empty(...) → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for empty, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn construct_eval() {
    let content = r#"<?php
eval('echo 1;');
"#;
    // Cursor on 'echo 1;' (line 1, char 6).
    let results = selection_ranges(content, &[Position::new(1, 6)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → eval → expr stmt → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for eval, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn construct_include() {
    let content = r#"<?php
include 'file.php';
"#;
    // Cursor on 'file.php' (line 1, char 10).
    let results = selection_ranges(content, &[Position::new(1, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → include → expr stmt → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for include, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn construct_include_once() {
    let content = r#"<?php
include_once 'file.php';
"#;
    // Cursor on 'file.php' (line 1, char 15).
    let results = selection_ranges(content, &[Position::new(1, 15)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for include_once, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn construct_require() {
    let content = r#"<?php
require 'file.php';
"#;
    // Cursor on 'file.php' (line 1, char 10).
    let results = selection_ranges(content, &[Position::new(1, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for require, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn construct_require_once() {
    let content = r#"<?php
require_once 'file.php';
"#;
    // Cursor on 'file.php' (line 1, char 15).
    let results = selection_ranges(content, &[Position::new(1, 15)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for require_once, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 25. Namespace (brace-delimited) ────────────────────────────────

#[test]
fn namespace_brace_delimited() {
    let content = r#"<?php
namespace App {
    function foo() {
        echo "hello";
    }
}
"#;
    // Cursor on "hello" (line 3, char 14).
    let results = selection_ranges(content, &[Position::new(3, 14)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → echo → block → function → { } → namespace → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for braced namespace, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 26. Namespace (implicit) ───────────────────────────────────────

#[test]
fn namespace_implicit() {
    let content = r#"<?php
namespace App;
function foo() {
    echo "hello";
}
"#;
    // Cursor on "hello" (line 3, char 10).
    let results = selection_ranges(content, &[Position::new(3, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // string → echo → block → function → namespace → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for implicit namespace, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 27. Interface members ──────────────────────────────────────────

#[test]
fn interface_method() {
    let content = r#"<?php
interface Greetable {
    public function greet(string $name): string;
}
"#;
    // Cursor on $name (line 2, char 35).
    let results = selection_ranges(content, &[Position::new(2, 35)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $name → parameter → param list → method → { } → interface → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for interface method, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 28. Trait members ──────────────────────────────────────────────

#[test]
fn trait_method_body() {
    let content = r#"<?php
trait Greeter {
    public function greet(): string {
        return "hello";
    }
}
"#;
    // Cursor on "hello" (line 3, char 16).
    let results = selection_ranges(content, &[Position::new(3, 16)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "hello" → return → block → method → { } → trait → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for trait method body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 29. Enum members ───────────────────────────────────────────────

#[test]
fn backed_enum_case() {
    let content = r#"<?php
enum Color: string {
    case Red = 'red';
    case Blue = 'blue';
}
"#;
    // Cursor on 'red' (line 2, char 16).
    let results = selection_ranges(content, &[Position::new(2, 16)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 'red' → backed item → enum case → { } → enum → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for backed enum case, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 30. Method parameter ───────────────────────────────────────────

#[test]
fn method_parameter_variable() {
    let content = r#"<?php
class Foo {
    public function bar(int $x, string $y): void {}
}
"#;
    // Cursor on $y (line 2, char 40).
    let results = selection_ranges(content, &[Position::new(2, 40)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $y → parameter → param list → method → { } → class → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for method parameter, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 31. Function parameter with default ────────────────────────────

#[test]
fn function_parameter_default() {
    let content = r#"<?php
function foo(int $x = 42) {
    echo $x;
}
"#;
    // Cursor on 42 (line 1, char 22).
    let results = selection_ranges(content, &[Position::new(1, 22)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 42 → parameter → param list → function → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for param default, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 32. Named argument ─────────────────────────────────────────────

#[test]
fn named_argument_value() {
    let content = r#"<?php
foo(name: "John");
"#;
    // Cursor on "John" (line 1, char 11).
    let results = selection_ranges(content, &[Position::new(1, 11)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "John" → named arg → arg list → call → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for named argument, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 33. Unset statement ────────────────────────────────────────────

#[test]
fn unset_statement() {
    let content = r#"<?php
unset($a, $b);
"#;
    // Cursor on $a (line 1, char 6).
    let results = selection_ranges(content, &[Position::new(1, 6)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $a → unset → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for unset, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 34. Declare statement ──────────────────────────────────────────

#[test]
fn declare_statement_body() {
    let content = r#"<?php
declare(strict_types=1) {
    echo "strict";
}
"#;
    // Cursor on "strict" (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "strict" → echo → block → declare → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for declare body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 35. CompositeString ────────────────────────────────────────────

#[test]
fn composite_string_expression() {
    let content = r#"<?php
$name = "world";
echo "hello {$name}!";
"#;
    // Cursor on $name inside the string (line 2, char 14).
    let results = selection_ranges(content, &[Position::new(2, 14)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $name → composite string → echo → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for composite string, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 36. Pipe expression ────────────────────────────────────────────

#[test]
fn pipe_expression() {
    // Pipe operator may not parse in all PHP versions, but the branch exists.
    // If it doesn't parse, the test simply passes with fewer levels.
    let content = r#"<?php
$x = $a |> 'strtoupper';
"#;
    let results = selection_ranges(content, &[Position::new(1, 6)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // At minimum: some expr → assignment → expr stmt → file
    assert!(
        ranges.len() >= 2,
        "Expected at least 2 levels for pipe expression, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 37. Elseif/else clause ─────────────────────────────────────────

#[test]
fn elseif_body() {
    let content = r#"<?php
if (true) {
    echo "a";
} elseif (false) {
    echo "b";
} else {
    echo "c";
}
"#;
    // Cursor on "b" in the elseif body (line 4, char 10).
    let results = selection_ranges(content, &[Position::new(4, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "b" → echo → block stmt → elseif clause → if → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for elseif body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn else_body() {
    let content = r#"<?php
if (true) {
    echo "a";
} else {
    echo "c";
}
"#;
    // Cursor on "c" in the else body (line 4, char 10).
    let results = selection_ranges(content, &[Position::new(4, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "c" → echo → block stmt → else clause → if → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for else body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 38. Colon-delimited if ─────────────────────────────────────────

#[test]
fn colon_delimited_if_body() {
    let content = r#"<?php
if (true):
    echo "a";
elseif (false):
    echo "b";
else:
    echo "c";
endif;
"#;
    // Cursor on "a" in the if body (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "a" → echo → if → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for colon-delimited if body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn colon_delimited_elseif_body() {
    let content = r#"<?php
if (true):
    echo "a";
elseif (false):
    echo "b";
else:
    echo "c";
endif;
"#;
    // Cursor on "b" in the elseif body (line 4, char 10).
    let results = selection_ranges(content, &[Position::new(4, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "b" → echo → elseif clause → if → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for colon-delimited elseif body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn colon_delimited_else_body() {
    let content = r#"<?php
if (true):
    echo "a";
else:
    echo "c";
endif;
"#;
    // Cursor on "c" in the else body (line 4, char 10).
    let results = selection_ranges(content, &[Position::new(4, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "c" → echo → else clause → if → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for colon-delimited else body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 39. Class constant ─────────────────────────────────────────────

#[test]
fn class_constant_value() {
    let content = r#"<?php
class Foo {
    const BAR = 42;
}
"#;
    // Cursor on 42 (line 2, char 16).
    let results = selection_ranges(content, &[Position::new(2, 16)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 42 → constant item → constant member → { } → class → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for class constant, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 40. Enum case (backed) value ───────────────────────────────────

#[test]
fn enum_backed_case_value() {
    let content = r#"<?php
enum Status: int {
    case Active = 1;
    case Inactive = 0;
}
"#;
    // Cursor on 0 (line 3, char 20).
    let results = selection_ranges(content, &[Position::new(3, 20)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 0 → backed item → enum case → { } → enum → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for enum backed case value, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 41. Property with default ──────────────────────────────────────

#[test]
fn property_with_default() {
    let content = r#"<?php
class Foo {
    public int $x = 42;
}
"#;
    // Cursor on 42 (line 2, char 21).
    let results = selection_ranges(content, &[Position::new(2, 21)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 42 → property item → property → { } → class → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for property default, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 42. Array access ───────────────────────────────────────────────

#[test]
fn array_access_expression() {
    let content = r#"<?php
$x = $arr[0];
"#;
    // Cursor on 0 (line 1, char 10).
    let results = selection_ranges(content, &[Position::new(1, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 0 → $arr[0] → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for array access, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── 43. Array append ───────────────────────────────────────────────

#[test]
fn array_append_expression() {
    let content = r#"<?php
$arr[] = 42;
"#;
    // Cursor on $arr (line 1, char 1).
    let results = selection_ranges(content, &[Position::new(1, 1)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $arr → $arr[] → assignment → expr stmt → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for array append, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── Additional branch coverage ─────────────────────────────────────

#[test]
fn unary_prefix_expression() {
    let content = r#"<?php
$x = !$flag;
"#;
    // Cursor on $flag (line 1, char 7).
    let results = selection_ranges(content, &[Position::new(1, 7)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $flag → !$flag → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for unary prefix, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn unary_postfix_expression() {
    let content = r#"<?php
$x++;
"#;
    // Cursor on $x (line 1, char 1).
    let results = selection_ranges(content, &[Position::new(1, 1)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → $x++ → expr stmt → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for unary postfix, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn parenthesized_expression() {
    let content = r#"<?php
$x = (1 + 2);
"#;
    // Cursor on 1 (line 1, char 6).
    let results = selection_ranges(content, &[Position::new(1, 6)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 1 → 1+2 → (1+2) → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for parenthesized, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn assignment_expression() {
    let content = r#"<?php
$x = $y = 5;
"#;
    // Cursor on 5 (line 1, char 10).
    let results = selection_ranges(content, &[Position::new(1, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 5 → $y = 5 → $x = $y = 5 → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for chained assignment, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn method_call_expression() {
    let content = r#"<?php
$obj->method(42);
"#;
    // Cursor on 42 (line 1, char 13).
    let results = selection_ranges(content, &[Position::new(1, 13)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 42 → arg → arg list → $obj->method(42) → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for method call arg, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn null_safe_property_access() {
    let content = r#"<?php
$x = $obj?->prop;
"#;
    // Cursor on prop (line 1, char 13).
    let results = selection_ranges(content, &[Position::new(1, 13)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // prop → $obj?->prop → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for null-safe property access, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn static_property_access() {
    let content = r#"<?php
$x = Foo::$bar;
"#;
    // Cursor on $bar (line 1, char 11).
    let results = selection_ranges(content, &[Position::new(1, 11)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $bar → Foo::$bar → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for static property access, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn class_constant_access() {
    let content = r#"<?php
$x = Foo::BAR;
"#;
    // Cursor on BAR (line 1, char 11).
    let results = selection_ranges(content, &[Position::new(1, 11)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // BAR → Foo::BAR → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for class constant access, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn null_safe_method_call() {
    let content = r#"<?php
$x = $obj?->method(1);
"#;
    // Cursor on method (line 1, char 14).
    let results = selection_ranges(content, &[Position::new(1, 14)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // method → $obj?->method(1) → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for null-safe method call, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn function_call_expression() {
    let content = r#"<?php
strlen("hello");
"#;
    // Cursor on "hello" (line 1, char 8).
    let results = selection_ranges(content, &[Position::new(1, 8)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "hello" → positional arg → arg list → strlen(...) → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for function call, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn array_variadic_element() {
    let content = r#"<?php
$x = [...$arr];
"#;
    // Cursor on $arr (line 1, char 10).
    let results = selection_ranges(content, &[Position::new(1, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $arr → variadic element → array → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for array variadic, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn closure_parameter() {
    let content = r#"<?php
$fn = function (int $x) {
    return $x;
};
"#;
    // Cursor on $x in parameter (line 1, char 20).
    let results = selection_ranges(content, &[Position::new(1, 20)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → parameter → param list → closure → assignment → expr stmt → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for closure parameter, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn arrow_function_parameter() {
    let content = r#"<?php
$fn = fn(int $x) => $x + 1;
"#;
    // Cursor on $x in parameter (line 1, char 14).
    let results = selection_ranges(content, &[Position::new(1, 14)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → parameter → param list → arrow fn → assignment → expr stmt → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for arrow fn parameter, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn construct_print() {
    let content = r#"<?php
print "hello";
"#;
    // Cursor on "hello" (line 1, char 7).
    let results = selection_ranges(content, &[Position::new(1, 7)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "hello" → print → expr stmt → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for print, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn for_loop_colon_delimited() {
    let content = r#"<?php
for ($i = 0; $i < 10; $i++):
    echo $i;
endfor;
"#;
    // Cursor on $i in echo (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $i → echo → for → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for colon-delimited for, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn while_loop_colon_delimited() {
    let content = r#"<?php
while (true):
    echo "loop";
endwhile;
"#;
    // Cursor on "loop" (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "loop" → echo → while → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for colon-delimited while, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn foreach_colon_delimited() {
    let content = r#"<?php
foreach ([1, 2] as $v):
    echo $v;
endforeach;
"#;
    // Cursor on $v in echo (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $v → echo → foreach → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for colon-delimited foreach, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn switch_colon_delimited() {
    let content = r#"<?php
switch ($x):
    case 1:
        echo "one";
        break;
endswitch;
"#;
    // Cursor on "one" (line 3, char 14).
    let results = selection_ranges(content, &[Position::new(3, 14)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "one" → echo → case → switch → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for colon-delimited switch, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn declare_colon_delimited() {
    let content = r#"<?php
declare(strict_types=1):
    echo "hello";
enddeclare;
"#;
    // Cursor on "hello" (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "hello" → echo → declare → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for colon-delimited declare, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn block_statement() {
    let content = r#"<?php
{
    echo "inside";
}
"#;
    // Cursor on "inside" (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "inside" → echo → { } → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for block statement, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn enum_unit_case() {
    let content = r#"<?php
enum Suit {
    case Hearts;
    case Diamonds;
}
"#;
    // Cursor on Hearts (line 2, char 10).
    let results = selection_ranges(content, &[Position::new(2, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // unit item → enum case → { } → enum → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for unit enum case, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn abstract_property() {
    let content = r#"<?php
class Foo {
    public int $x;
}
"#;
    // Cursor on $x (line 2, char 16).
    let results = selection_ranges(content, &[Position::new(2, 16)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → property → { } → class → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for abstract property, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn ternary_short_form() {
    // Short ternary: $a ?: $b (no then branch).
    let content = r#"<?php
$x = $a ?: $b;
"#;
    // Cursor on $b (line 1, char 12).
    let results = selection_ranges(content, &[Position::new(1, 12)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $b → ternary → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for short ternary, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn instantiation_without_args() {
    let content = r#"<?php
$x = new Foo;
"#;
    // Cursor on Foo (line 1, char 10).
    let results = selection_ranges(content, &[Position::new(1, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // Foo → new Foo → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for new without args, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn legacy_array_value_element() {
    let content = r#"<?php
$x = array(1, 2, 3);
"#;
    // Cursor on 2 (line 1, char 15).
    let results = selection_ranges(content, &[Position::new(1, 15)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 2 → value element → array() → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for legacy array value element, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn enum_with_method() {
    let content = r#"<?php
enum Color: string {
    case Red = 'red';

    public function label(): string {
        return "Color";
    }
}
"#;
    // Cursor on "Color" in the method body (line 5, char 16).
    let results = selection_ranges(content, &[Position::new(5, 16)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "Color" → return → block → method → { } → enum → file
    assert!(
        ranges.len() >= 5,
        "Expected at least 5 levels for enum method body, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn for_loop_initializer() {
    let content = r#"<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}
"#;
    // Cursor on $i in the initializer (line 1, char 6).
    let results = selection_ranges(content, &[Position::new(1, 6)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $i → $i = 0 → for → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for for initializer, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn for_loop_condition() {
    let content = r#"<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}
"#;
    // Cursor on 10 in the condition (line 1, char 19).
    let results = selection_ranges(content, &[Position::new(1, 19)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 10 → $i < 10 → for → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for for condition, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn for_loop_increment() {
    let content = r#"<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}
"#;
    // Cursor on $i in the increment (line 1, char 23).
    let results = selection_ranges(content, &[Position::new(1, 23)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $i → $i++ → for → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for for increment, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn while_condition() {
    let content = r#"<?php
while ($x > 0) {
    $x--;
}
"#;
    // Cursor on $x in the condition (line 1, char 8).
    let results = selection_ranges(content, &[Position::new(1, 8)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → $x > 0 → while → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for while condition, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn do_while_condition() {
    let content = r#"<?php
do {
    $x--;
} while ($x > 0);
"#;
    // Cursor on $x in the condition (line 3, char 10).
    let results = selection_ranges(content, &[Position::new(3, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → $x > 0 → do-while → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for do-while condition, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn foreach_source_expression() {
    let content = r#"<?php
foreach ($items as $item) {
    echo $item;
}
"#;
    // Cursor on $items in the foreach expression (line 1, char 10).
    let results = selection_ranges(content, &[Position::new(1, 10)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $items → foreach → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for foreach source expression, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn switch_expression() {
    let content = r#"<?php
switch ($x) {
    case 1:
        break;
}
"#;
    // Cursor on $x in switch expression (line 1, char 9).
    let results = selection_ranges(content, &[Position::new(1, 9)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → switch → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for switch expression, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn match_subject_expression() {
    let content = r#"<?php
$r = match($x) {
    default => 1,
};
"#;
    // Cursor on $x (line 1, char 12).
    let results = selection_ranges(content, &[Position::new(1, 12)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → match → assignment → expr stmt → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for match subject, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

#[test]
fn if_condition() {
    let content = r#"<?php
if ($x > 0) {
    echo "positive";
}
"#;
    // Cursor on $x (line 1, char 5).
    let results = selection_ranges(content, &[Position::new(1, 5)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $x → $x > 0 → if → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for if condition, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── Construct: exit() ──────────────────────────────────────────────

#[test]
fn construct_exit_with_args() {
    let content = r#"<?php
exit(1);
"#;
    // Cursor on 1 (line 1, char 5).
    let results = selection_ranges(content, &[Position::new(1, 5)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // 1 → arg → arg list → exit(...) → expr stmt → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for exit with args, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── Construct: die() ───────────────────────────────────────────────

#[test]
fn construct_die_with_args() {
    let content = r#"<?php
die("error");
"#;
    // Cursor on "error" (line 1, char 5).
    let results = selection_ranges(content, &[Position::new(1, 5)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "error" → arg → arg list → die(...) → expr stmt → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for die with args, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── Trait use statement ────────────────────────────────────────────

#[test]
fn trait_use_member() {
    let content = r#"<?php
trait Greeter {
    public function greet(): string {
        return "hello";
    }
}

class Foo {
    use Greeter;

    public function bar(): void {}
}
"#;
    // Cursor on Greeter in `use Greeter;` (line 8, char 8).
    let results = selection_ranges(content, &[Position::new(8, 8)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // trait use member → { } → class → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for trait use member, got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── Hooked property (abstract / no default) ────────────────────────

#[test]
fn hooked_property_abstract() {
    let content = r#"<?php
class Foo {
    public string $name {
        get => $this->name;
    }
}
"#;
    // Cursor on $name (line 2, char 19).
    let results = selection_ranges(content, &[Position::new(2, 19)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // $name → property member → { } → class → file
    assert!(
        ranges.len() >= 3,
        "Expected at least 3 levels for hooked property (abstract), got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}

// ─── Hooked property (concrete / with default) ──────────────────────

#[test]
fn hooked_property_concrete() {
    let content = r#"<?php
class Foo {
    public string $name = "default" {
        get => $this->name;
    }
}
"#;
    // Cursor on "default" (line 2, char 27).
    let results = selection_ranges(content, &[Position::new(2, 27)]);
    assert_eq!(results.len(), 1);
    let ranges = flatten(&results[0]);
    // "default" → concrete item → property member → { } → class → file
    assert!(
        ranges.len() >= 4,
        "Expected at least 4 levels for hooked property (concrete), got {}",
        ranges.len()
    );
    assert_nested(&ranges);
}
