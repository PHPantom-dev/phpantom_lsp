use super::*;

const NAMED: &str = "Method Foo::run() should return array but returns int.";
const ANONYMOUS: &str = "Anonymous function should return string but returns int.";

/// The source of the declaration the diagnostic is about, from its start
/// to the end of the file.
fn found<'a>(content: &'a str, line: usize, anchor: Anchor, message: &str) -> Option<&'a str> {
    let target = target_function(content, line, anchor, message)?;
    Some(&content[target.start as usize..])
}

// ── Diagnostics on a `return` ───────────────────────────────────

const CLOSURE_IN_METHOD: &str = "<?php
class Foo {
    public function run(): array {
        $f = function (): string {
            return 42;
        };
        return [];
    }
}
";

#[test]
fn a_return_in_a_closure_belongs_to_the_closure() {
    let found = found(CLOSURE_IN_METHOD, 4, Anchor::Return, ANONYMOUS).unwrap();
    assert!(found.starts_with("function (): string"), "{found}");
}

#[test]
fn a_return_in_a_closure_is_not_the_method_one() {
    assert!(found(CLOSURE_IN_METHOD, 4, Anchor::Return, NAMED).is_none());
    let found = found(CLOSURE_IN_METHOD, 6, Anchor::Return, NAMED).unwrap();
    assert!(found.starts_with("public function run()"), "{found}");
}

#[test]
fn the_message_picks_between_returns_sharing_a_line() {
    let content = "<?php
class Foo {
    public function run(): array {
        return array_map(function ($x): string { return $x; }, [1]);
    }
}
";
    let closure = found(content, 3, Anchor::Return, ANONYMOUS).unwrap();
    assert!(closure.starts_with("function ($x)"), "{closure}");
    let method = found(content, 3, Anchor::Return, NAMED).unwrap();
    assert!(method.starts_with("public function run()"), "{method}");
}

#[test]
fn an_arrow_function_returns_on_the_line_of_its_expression() {
    let content = "<?php
function run(): void {
    $f = fn (): string =>
        42;
}
";
    assert!(found(content, 2, Anchor::Return, ANONYMOUS).is_none());
    let target = target_function(content, 3, Anchor::Return, ANONYMOUS).unwrap();
    assert!(content[target.start as usize..].starts_with("fn (): string"));
    assert!(target.statement.is_none());
    assert_eq!(
        target.native_return_type(content),
        Some(PhpType::parse("string"))
    );
}

#[test]
fn a_return_in_a_property_hook_belongs_to_no_function() {
    let content = "<?php
class Foo {
    public function make(): object {
        return new class {
            public int $x {
                get { return 1; }
            }
        };
    }
}
";
    let message = "Get hook for property Foo::$x should return int but returns string.";
    assert!(found(content, 5, Anchor::Return, message).is_none());
    let method = found(content, 3, Anchor::Return, NAMED).unwrap();
    assert!(method.starts_with("public function make()"), "{method}");
}

#[test]
fn a_top_level_return_belongs_to_no_function() {
    assert!(found("<?php\nreturn 1;\n", 1, Anchor::Return, NAMED).is_none());
}

#[test]
fn no_function_past_the_end_of_the_file() {
    assert!(found("<?php\n", 5, Anchor::Return, NAMED).is_none());
}

// ── Diagnostics on a declaration ────────────────────────────────

#[test]
fn any_line_of_a_declaration_names_it() {
    let content = "<?php
class Foo {
    #[Pure]
    public function run(
        int $x,
    ): ?int {
        return $x;
    }
}
";
    for line in 2..=5 {
        let found = found(content, line, Anchor::Declaration, NAMED).unwrap();
        assert!(found.starts_with("#[Pure]"), "line {line}: {found}");
    }
    assert!(found(content, 6, Anchor::Declaration, NAMED).is_none());
}

#[test]
fn a_closure_declared_on_the_line_of_another_statement() {
    let content = "<?php
function run(): int {
    $f = function (): ?string {
        return 'x';
    };
    return 1;
}
";
    let message =
        "Anonymous function never returns null so it can be removed from the return type.";
    let found = found(content, 2, Anchor::Declaration, message).unwrap();
    assert!(found.starts_with("function (): ?string"), "{found}");
}

// ── Reading and editing the declaration ─────────────────────────

#[test]
fn a_return_type_goes_after_the_parameter_list() {
    let content = "<?php
class Foo {
    public function returnsObject() // this must have a type
    {
        return new \\stdClass();
    }
}
";
    let target = target_function(content, 2, Anchor::Declaration, NAMED).unwrap();
    assert!(!target.has_return_type());
    let edit = target.add_return_type_edit(content, "\\stdClass");
    assert!(
        crate::text_position::apply_text_edits(content, &[edit])
            .contains("returnsObject(): \\stdClass // this must have a type")
    );
}

#[test]
fn a_closure_return_type_goes_after_its_use_clause() {
    let content = "<?php
function run(): void {
    $f = function () use ($x) {
        return $x;
    };
}
";
    let target = target_function(content, 3, Anchor::Return, ANONYMOUS).unwrap();
    let edit = target.add_return_type_edit(content, "int");
    assert!(
        crate::text_position::apply_text_edits(content, &[edit])
            .contains("function () use ($x): int {")
    );
}

#[test]
fn replaces_the_whole_native_return_type() {
    let content = "<?php
function run():Countable & Traversable {
    return new ArrayIterator();
}
";
    let target = target_function(content, 2, Anchor::Return, NAMED).unwrap();
    let edit = target
        .replace_return_type_edit(content, "ArrayIterator")
        .unwrap();
    assert_eq!(
        crate::text_position::apply_text_edits(content, &[edit]),
        "<?php
function run(): ArrayIterator {
    return new ArrayIterator();
}
"
    );
    assert!(
        target
            .replace_return_type_edit(content, "Countable & Traversable")
            .is_none()
    );
}

#[test]
fn reads_the_return_statement_it_was_found_by() {
    let content = "<?php
function run(): void {
    if (rand(0, 1)) return null;
}
";
    let message =
        "Function run() with return type void returns null but should not return anything.";
    let target = target_function(content, 2, Anchor::Return, message).unwrap();
    let statement = target.statement.unwrap();
    assert!(content[statement.start as usize..].starts_with("return null;"));
    assert_eq!(
        &content[..statement.end.unwrap() as usize],
        "<?php
function run(): void {
    if (rand(0, 1)) return null;"
    );
    assert!(statement.returns_null);
}
