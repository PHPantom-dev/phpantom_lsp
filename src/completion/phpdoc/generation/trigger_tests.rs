use super::*;

#[test]
fn detects_trigger_at_line_start() {
    let content = "<?php\n/**";
    let pos = Position {
        line: 1,
        character: 3,
    };
    let result = detect_docblock_trigger(content, pos);
    assert!(result.is_some(), "Should detect /** trigger");
    let (range, indent) = result.unwrap();
    assert_eq!(indent, "");
    assert_eq!(range.start.character, 0);
    assert_eq!(range.end.character, 3);
}

#[test]
fn detects_trigger_with_indentation() {
    let content = "<?php\nclass Foo {\n    /**";
    let pos = Position {
        line: 2,
        character: 7,
    };
    let result = detect_docblock_trigger(content, pos);
    assert!(result.is_some(), "Should detect indented /** trigger");
    let (_, indent) = result.unwrap();
    assert_eq!(indent, "    ");
}

#[test]
fn rejects_trigger_inside_existing_docblock() {
    let content = "<?php\n/**\n * @param\n */\nfunction test() {}";
    let pos = Position {
        line: 1,
        character: 3,
    };
    let result = detect_docblock_trigger(content, pos);
    assert!(
        result.is_none(),
        "Should not trigger inside existing docblock"
    );
}

#[test]
fn rejects_trigger_with_closing_on_same_line() {
    let content = "<?php\n/** @var int */";
    let pos = Position {
        line: 1,
        character: 3,
    };
    let result = detect_docblock_trigger(content, pos);
    assert!(
        result.is_none(),
        "Should not trigger when */ is on the same line"
    );
}

#[test]
fn rejects_trigger_with_code_before() {
    let content = "<?php\n$x = /**";
    let pos = Position {
        line: 1,
        character: 8,
    };
    let result = detect_docblock_trigger(content, pos);
    assert!(
        result.is_none(),
        "Should not trigger when code precedes /**"
    );
}

#[test]
fn no_panic_on_multibyte_characters() {
    // "ń" is 2 bytes in UTF-8 but 1 UTF-16 code unit.
    // The cursor is after the closing paren, UTF-16 column 32.
    // Using that as a byte offset would land inside "ń" and panic.
    let content = "<?php\n                $table->string(ń);";
    let pos = Position {
        line: 1,
        character: 32,
    };
    // Must not panic — should simply return None.
    let result = detect_docblock_trigger(content, pos);
    assert!(result.is_none());
}
