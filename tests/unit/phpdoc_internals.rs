//! Tests for PHPDoc internal helpers.
//!
//! These tests exercise `phpantom_lsp::completion::phpdoc` — docblock
//! context detection, tag prefix extraction, and PHPDoc completion
//! generation.

use std::sync::Arc;

use phpantom_lsp::atom::atom;
use phpantom_lsp::completion::phpdoc::*;
use phpantom_lsp::php_type::PhpType;
use phpantom_lsp::types::ClassInfo;
use tower_lsp::lsp_types::*;

// ── is_inside_non_doc_comment ───────────────────────────────────────

#[test]
fn inside_line_comment() {
    let content = "<?php\n// this is a comment\n";
    let pos = Position {
        line: 1,
        character: 10,
    };
    assert!(is_inside_non_doc_comment(content, pos));
}

#[test]
fn inside_line_comment_at_start() {
    let content = "<?php\n// comment\n";
    let pos = Position {
        line: 1,
        character: 2,
    };
    assert!(is_inside_non_doc_comment(content, pos));
}

#[test]
fn not_inside_line_comment_next_line() {
    let content = "<?php\n// comment\n$x = 1;\n";
    let pos = Position {
        line: 2,
        character: 3,
    };
    assert!(!is_inside_non_doc_comment(content, pos));
}

#[test]
fn inside_block_comment() {
    let content = "<?php\n/* block comment */\n";
    let pos = Position {
        line: 1,
        character: 8,
    };
    assert!(is_inside_non_doc_comment(content, pos));
}

#[test]
fn inside_multiline_block_comment() {
    let content = "<?php\n/* block\n   comment\n*/\n";
    let pos = Position {
        line: 2,
        character: 5,
    };
    assert!(is_inside_non_doc_comment(content, pos));
}

#[test]
fn not_inside_block_comment_after_close() {
    let content = "<?php\n/* block comment */\n$x = 1;\n";
    let pos = Position {
        line: 2,
        character: 3,
    };
    assert!(!is_inside_non_doc_comment(content, pos));
}

#[test]
fn not_inside_docblock_for_non_doc_check() {
    // `/** … */` docblocks should NOT be flagged as non-doc comments
    let content = "<?php\n/**\n * @param string $x\n */\n";
    let pos = Position {
        line: 2,
        character: 5,
    };
    assert!(!is_inside_non_doc_comment(content, pos));
}

#[test]
fn not_inside_code_for_non_doc_check() {
    let content = "<?php\n$x = 1;\n";
    let pos = Position {
        line: 1,
        character: 3,
    };
    assert!(!is_inside_non_doc_comment(content, pos));
}

#[test]
fn line_comment_inside_string_ignored() {
    // `//` inside a string literal is not a comment
    let content = "<?php\n$x = '// not a comment';\n";
    let pos = Position {
        line: 1,
        character: 12,
    };
    assert!(!is_inside_non_doc_comment(content, pos));
}

#[test]
fn block_comment_inside_double_string_ignored() {
    // `/* */` inside a double-quoted string is not a comment
    let content = "<?php\n$x = \"/* not a comment */\";\n";
    let pos = Position {
        line: 1,
        character: 12,
    };
    assert!(!is_inside_non_doc_comment(content, pos));
}

#[test]
fn line_comment_after_code_on_same_line() {
    let content = "<?php\n$x = 1; // trailing comment\n";
    let pos = Position {
        line: 1,
        character: 18,
    };
    assert!(is_inside_non_doc_comment(content, pos));
}

#[test]
fn before_line_comment_on_same_line() {
    let content = "<?php\n$x = 1; // trailing comment\n";
    let pos = Position {
        line: 1,
        character: 3,
    };
    assert!(!is_inside_non_doc_comment(content, pos));
}

#[test]
fn triple_star_is_block_comment_not_docblock() {
    // `/***` — PHP's tokeniser treats this as T_COMMENT (regular comment),
    // NOT T_DOC_COMMENT.  Only `/**` followed by a non-`*` char is a docblock.
    let content = "<?php\n/*** not a docblock */\n";
    let pos = Position {
        line: 1,
        character: 10,
    };
    assert!(is_inside_non_doc_comment(content, pos));
}

#[test]
fn heredoc_does_not_leak_into_comment_detection() {
    // Ensure `//` inside a heredoc body is not treated as a comment
    let content = "<?php\n$x = <<<EOT\n// not a comment\nEOT;\n$y = 1;\n";
    let pos = Position {
        line: 2,
        character: 5,
    };
    assert!(!is_inside_non_doc_comment(content, pos));
}

#[test]
fn after_heredoc_code_is_normal() {
    let content = "<?php\n$x = <<<EOT\nsome text\nEOT;\n// real comment\n";
    let pos = Position {
        line: 4,
        character: 5,
    };
    assert!(is_inside_non_doc_comment(content, pos));
}

#[test]
fn escaped_quote_in_string_does_not_leak() {
    // Escaped quote inside a string should not close it prematurely
    let content = "<?php\n$x = 'it\\'s fine'; // comment\n";
    let pos = Position {
        line: 1,
        character: 25,
    };
    assert!(is_inside_non_doc_comment(content, pos));
}

#[test]
fn unclosed_line_comment_at_eof() {
    let content = "<?php\n// comment without newline";
    let pos = Position {
        line: 1,
        character: 15,
    };
    assert!(is_inside_non_doc_comment(content, pos));
}

#[test]
fn unclosed_block_comment_at_eof() {
    let content = "<?php\n/* unclosed block comment";
    let pos = Position {
        line: 1,
        character: 15,
    };
    assert!(is_inside_non_doc_comment(content, pos));
}

// ── detect_docblock_typing_position ─────────────────────────────

#[test]
fn typing_pos_param_empty_type() {
    let content = "<?php\n/**\n * @param \n */\nfunction foo(string $x): void {}\n";
    let pos = Position {
        line: 2,
        character: 11,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "param".to_string(),
        })
    );
}

#[test]
fn typing_pos_param_partial_type() {
    let content = "<?php\n/**\n * @param Str\n */\nfunction foo(string $x): void {}\n";
    let pos = Position {
        line: 2,
        character: 14,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Str".to_string(),
            tag: "param".to_string(),
        })
    );
}

#[test]
fn typing_pos_param_variable_empty() {
    let content = "<?php\n/**\n * @param string \n */\nfunction foo(string $x): void {}\n";
    let pos = Position {
        line: 2,
        character: 18,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Variable {
            partial: String::new()
        })
    );
}

#[test]
fn typing_pos_param_variable_partial() {
    let content = "<?php\n/**\n * @param string $x\n */\nfunction foo(string $x): void {}\n";
    let pos = Position {
        line: 2,
        character: 20,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Variable {
            partial: "$x".to_string()
        })
    );
}

#[test]
fn typing_pos_param_description_after_variable() {
    let content =
        "<?php\n/**\n * @param string $x some description\n */\nfunction foo(string $x): void {}\n";
    let pos = Position {
        line: 2,
        character: 30,
    };
    assert_eq!(detect_docblock_typing_position(content, pos), None);
}

#[test]
fn typing_pos_return_empty_type() {
    let content = "<?php\n/**\n * @return \n */\nfunction foo(): string {}\n";
    let pos = Position {
        line: 2,
        character: 12,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "return".to_string(),
        })
    );
}

#[test]
fn typing_pos_return_partial_type() {
    let content = "<?php\n/**\n * @return Coll\n */\nfunction foo(): Collection {}\n";
    let pos = Position {
        line: 2,
        character: 15,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Coll".to_string(),
            tag: "return".to_string(),
        })
    );
}

#[test]
fn typing_pos_return_description() {
    let content = "<?php\n/**\n * @return string the name\n */\nfunction foo(): string {}\n";
    let pos = Position {
        line: 2,
        character: 25,
    };
    assert_eq!(detect_docblock_typing_position(content, pos), None);
}

#[test]
fn typing_pos_throws_empty_type() {
    let content = "<?php\n/**\n * @throws \n */\nfunction foo(): void {}\n";
    let pos = Position {
        line: 2,
        character: 12,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "throws".to_string(),
        })
    );
}

#[test]
fn typing_pos_throws_partial_type() {
    let content = "<?php\n/**\n * @throws Invalid\n */\nfunction foo(): void {}\n";
    let pos = Position {
        line: 2,
        character: 19,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Invalid".to_string(),
            tag: "throws".to_string(),
        })
    );
}

#[test]
fn typing_pos_var_empty_type() {
    let content = "<?php\n/**\n * @var \n */\n";
    let pos = Position {
        line: 2,
        character: 8,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "var".to_string(),
        })
    );
}

#[test]
fn typing_pos_var_partial_type() {
    let content = "<?php\n/**\n * @var Dat\n */\n";
    let pos = Position {
        line: 2,
        character: 11,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Dat".to_string(),
            tag: "var".to_string(),
        })
    );
}

#[test]
fn typing_pos_mixin_empty_type() {
    let content = "<?php\n/**\n * @mixin \n */\nclass Foo {}\n";
    let pos = Position {
        line: 2,
        character: 11,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "mixin".to_string(),
        })
    );
}

#[test]
fn typing_pos_extends_partial_type() {
    let content = "<?php\n/**\n * @extends Base\n */\nclass Foo extends Bar {}\n";
    let pos = Position {
        line: 2,
        character: 16,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Base".to_string(),
            tag: "extends".to_string(),
        })
    );
}

#[test]
fn typing_pos_implements_empty_type() {
    let content = "<?php\n/**\n * @implements \n */\nclass Foo implements Bar {}\n";
    let pos = Position {
        line: 2,
        character: 17,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "implements".to_string(),
        })
    );
}

#[test]
fn typing_pos_property_empty_type() {
    let content = "<?php\n/**\n * @property \n */\nclass Foo {}\n";
    let pos = Position {
        line: 2,
        character: 15,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "property".to_string(),
        })
    );
}

#[test]
fn typing_pos_property_variable() {
    let content = "<?php\n/**\n * @property string $\n */\nclass Foo {}\n";
    let pos = Position {
        line: 2,
        character: 23,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Variable {
            partial: "$".to_string()
        })
    );
}

#[test]
fn typing_pos_property_read_type() {
    let content = "<?php\n/**\n * @property-read \n */\nclass Foo {}\n";
    let pos = Position {
        line: 2,
        character: 20,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "property-read".to_string(),
        })
    );
}

#[test]
fn typing_pos_phpstan_param_type() {
    let content = "<?php\n/**\n * @phpstan-param \n */\nfunction foo(array $x): void {}\n";
    let pos = Position {
        line: 2,
        character: 20,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "phpstan-param".to_string(),
        })
    );
}

#[test]
fn typing_pos_phpstan_return_type() {
    let content = "<?php\n/**\n * @phpstan-return Coll\n */\nfunction foo(): Collection {}\n";
    let pos = Position {
        line: 2,
        character: 23,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Coll".to_string(),
            tag: "phpstan-return".to_string(),
        })
    );
}

// ── Union / intersection / nullable types ───────────────────────

#[test]
fn typing_pos_union_type_second_part() {
    let content = "<?php\n/**\n * @param string|Fo\n */\nfunction foo($x): void {}\n";
    let pos = Position {
        line: 2,
        character: 21,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Fo".to_string(),
            tag: "param".to_string(),
        })
    );
}

#[test]
fn typing_pos_union_type_pipe_only() {
    let content = "<?php\n/**\n * @param string|\n */\nfunction foo($x): void {}\n";
    let pos = Position {
        line: 2,
        character: 19,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "param".to_string(),
        })
    );
}

#[test]
fn typing_pos_intersection_type() {
    let content = "<?php\n/**\n * @param Foo&Ba\n */\nfunction foo($x): void {}\n";
    let pos = Position {
        line: 2,
        character: 17,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Ba".to_string(),
            tag: "param".to_string(),
        })
    );
}

#[test]
fn typing_pos_nullable_type() {
    let content = "<?php\n/**\n * @param ?Fo\n */\nfunction foo($x): void {}\n";
    let pos = Position {
        line: 2,
        character: 14,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Fo".to_string(),
            tag: "param".to_string(),
        })
    );
}

// ── Generic types ───────────────────────────────────────────────

#[test]
fn typing_pos_generic_type_inside_angle_brackets() {
    // Cursor inside `Collection<Us` — still inside the type token
    let content = "<?php\n/**\n * @return Collection<Us\n */\nfunction foo() {}\n";
    let pos = Position {
        line: 2,
        character: 25,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Us".to_string(),
            tag: "return".to_string(),
        })
    );
}

#[test]
fn typing_pos_generic_type_after_comma() {
    // Cursor inside `array<string, ` — still in type token
    let content = "<?php\n/**\n * @return array<string, \n */\nfunction foo() {}\n";
    let pos = Position {
        line: 2,
        character: 26,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "return".to_string(),
        })
    );
}

#[test]
fn typing_pos_generic_type_after_comma_partial() {
    let content = "<?php\n/**\n * @return array<string, Us\n */\nfunction foo() {}\n";
    let pos = Position {
        line: 2,
        character: 28,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "Us".to_string(),
            tag: "return".to_string(),
        })
    );
}

#[test]
fn typing_pos_closed_generic_then_variable() {
    // `@param Collection<User> $` — generic is closed, now at variable pos
    let content =
        "<?php\n/**\n * @param Collection<User> $\n */\nfunction foo(Collection $c): void {}\n";
    let pos = Position {
        line: 2,
        character: 30,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Variable {
            partial: "$".to_string()
        })
    );
}

// ── Array shape types ───────────────────────────────────────────

#[test]
fn typing_pos_array_shape_inside_braces() {
    // Cursor inside `array{name: str` — still in type token
    let content = "<?php\n/**\n * @return array{name: str\n */\nfunction foo() {}\n";
    let pos = Position {
        line: 2,
        character: 28,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "str".to_string(),
            tag: "return".to_string(),
        })
    );
}

// ── Namespaced types ────────────────────────────────────────────

#[test]
fn typing_pos_namespaced_partial() {
    let content = "<?php\n/**\n * @param App\\Models\\Us\n */\nfunction foo($x): void {}\n";
    let pos = Position {
        line: 2,
        character: 24,
    };
    assert_eq!(
        detect_docblock_typing_position(content, pos),
        Some(DocblockTypingContext::Type {
            partial: "App\\Models\\Us".to_string(),
            tag: "param".to_string(),
        })
    );
}

// ── Unrecognised / non-type tags ────────────────────────────────

#[test]
fn typing_pos_deprecated_returns_none() {
    let content = "<?php\n/**\n * @deprecated \n */\n";
    let pos = Position {
        line: 2,
        character: 17,
    };
    assert_eq!(detect_docblock_typing_position(content, pos), None);
}

#[test]
fn typing_pos_todo_returns_none() {
    let content = "<?php\n/**\n * @todo \n */\n";
    let pos = Position {
        line: 2,
        character: 10,
    };
    assert_eq!(detect_docblock_typing_position(content, pos), None);
}

#[test]
fn typing_pos_description_line_returns_none() {
    // Plain description line (no tag)
    let content = "<?php\n/**\n * This class handles Foo\n */\n";
    let pos = Position {
        line: 2,
        character: 24,
    };
    assert_eq!(detect_docblock_typing_position(content, pos), None);
}

#[test]
fn typing_pos_outside_docblock_returns_none() {
    let content = "<?php\n// @param string\n";
    let pos = Position {
        line: 1,
        character: 15,
    };
    assert_eq!(detect_docblock_typing_position(content, pos), None);
}

// ── Still typing tag name ───────────────────────────────────────

#[test]
fn typing_pos_still_typing_tag_name_returns_none() {
    // `@par` — still typing the tag, no space yet → None (handled by extract_phpdoc_prefix)
    let content = "<?php\n/**\n * @par\n */\n";
    let pos = Position {
        line: 2,
        character: 8,
    };
    assert_eq!(detect_docblock_typing_position(content, pos), None);
}

#[test]
fn typing_pos_no_panic_on_multibyte_characters() {
    // "ń" is 2 bytes in UTF-8 but 1 UTF-16 code unit.
    // Using the UTF-16 column as a byte offset would land inside the
    // multibyte character and panic.
    let content = "<?php\n/**\n * @param ń\n */\nfunction foo() {}\n";
    let pos = Position {
        line: 2,
        character: 12,
    };
    // Must not panic. "ń" is not a valid PHP identifier character, so
    // the trailing identifier extraction returns an empty partial.
    let result = detect_docblock_typing_position(content, pos);
    assert_eq!(
        result,
        Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: "param".to_string(),
        })
    );
}

#[test]
fn typing_pos_tag_no_space_returns_none() {
    // Cursor right after `@param` with no space — tag name, not type
    let content = "<?php\n/**\n * @param\n */\n";
    let pos = Position {
        line: 2,
        character: 10,
    };
    assert_eq!(detect_docblock_typing_position(content, pos), None);
}

// ── is_inside_docblock ──────────────────────────────────────────

#[test]
fn inside_open_docblock() {
    let content = "<?php\n/**\n * @\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert!(is_inside_docblock(content, pos));
}

#[test]
fn inside_closed_docblock() {
    let content = "<?php\n/**\n * @param string $x\n */\nfunction foo() {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert!(is_inside_docblock(content, pos));
}

#[test]
fn outside_docblock_after_close() {
    let content = "<?php\n/**\n * @param string $x\n */\nfunction foo() {}\n";
    let pos = Position {
        line: 4,
        character: 5,
    };
    assert!(!is_inside_docblock(content, pos));
}

#[test]
fn outside_docblock_before_open() {
    let content = "<?php\n\n/**\n * @param string $x\n */\n";
    let pos = Position {
        line: 1,
        character: 0,
    };
    assert!(!is_inside_docblock(content, pos));
}

#[test]
fn not_inside_regular_comment() {
    let content = "<?php\n/* regular comment @param */\n";
    let pos = Position {
        line: 1,
        character: 22,
    };
    assert!(!is_inside_docblock(content, pos));
}

#[test]
fn inside_multiline_docblock() {
    let content = "<?php\n/**\n * Some description.\n *\n * @\n */\n";
    let pos = Position {
        line: 4,
        character: 4,
    };
    assert!(is_inside_docblock(content, pos));
}

// ── extract_phpdoc_prefix ───────────────────────────────────────

#[test]
fn prefix_bare_at() {
    let content = "<?php\n/**\n * @\n */\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert_eq!(extract_phpdoc_prefix(content, pos), Some("@".to_string()));
}

#[test]
fn prefix_partial_tag() {
    let content = "<?php\n/**\n * @par\n */\n";
    let pos = Position {
        line: 2,
        character: 7,
    };
    assert_eq!(
        extract_phpdoc_prefix(content, pos),
        Some("@par".to_string())
    );
}

#[test]
fn prefix_phpstan_tag() {
    let content = "<?php\n/**\n * @phpstan-a\n */\n";
    let pos = Position {
        line: 2,
        character: 14,
    };
    assert_eq!(
        extract_phpdoc_prefix(content, pos),
        Some("@phpstan-a".to_string())
    );
}

#[test]
fn prefix_full_tag() {
    let content = "<?php\n/**\n * @return\n */\n";
    let pos = Position {
        line: 2,
        character: 10,
    };
    assert_eq!(
        extract_phpdoc_prefix(content, pos),
        Some("@return".to_string())
    );
}

#[test]
fn no_prefix_outside_docblock() {
    let content = "<?php\n$email = 'user@example.com';\n";
    let pos = Position {
        line: 1,
        character: 25,
    };
    assert_eq!(extract_phpdoc_prefix(content, pos), None);
}

#[test]
fn no_prefix_no_at_sign() {
    let content = "<?php\n/**\n * Just a description\n */\n";
    let pos = Position {
        line: 2,
        character: 20,
    };
    assert_eq!(extract_phpdoc_prefix(content, pos), None);
}

#[test]
fn no_prefix_in_email_inside_docblock() {
    let content = "<?php\n/**\n * Contact user@example.com\n */\n";
    let pos = Position {
        line: 2,
        character: 25,
    };
    assert_eq!(extract_phpdoc_prefix(content, pos), None);
}

// ── detect_context ──────────────────────────────────────────────

#[test]
fn context_function() {
    let content = "<?php\n/**\n * @\n */\nfunction hello(): void {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert_eq!(
        detect_context(content, pos),
        DocblockContext::FunctionOrMethod
    );
}

#[test]
fn context_method() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {}\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    assert_eq!(
        detect_context(content, pos),
        DocblockContext::FunctionOrMethod
    );
}

#[test]
fn context_static_method() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public static function bar(): void {}\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    assert_eq!(
        detect_context(content, pos),
        DocblockContext::FunctionOrMethod
    );
}

#[test]
fn context_class() {
    let content = "<?php\n/**\n * @\n */\nclass MyClass {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::ClassLike);
}

#[test]
fn context_abstract_class() {
    let content = "<?php\n/**\n * @\n */\nabstract class MyClass {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::ClassLike);
}

#[test]
fn context_final_class() {
    let content = "<?php\n/**\n * @\n */\nfinal class MyClass {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::ClassLike);
}

#[test]
fn context_interface() {
    let content = "<?php\n/**\n * @\n */\ninterface MyInterface {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::ClassLike);
}

#[test]
fn context_trait() {
    let content = "<?php\n/**\n * @\n */\ntrait MyTrait {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::ClassLike);
}

#[test]
fn context_enum() {
    let content = "<?php\n/**\n * @\n */\nenum Status {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::ClassLike);
}

#[test]
fn context_property() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::Property);
}

#[test]
fn context_typed_property() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    protected ?int $count = 0;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::Property);
}

#[test]
fn context_readonly_property() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public readonly string $name;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::Property);
}

#[test]
fn context_static_property() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    private static array $cache = [];\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::Property);
}

#[test]
fn context_constant() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    const MAX_SIZE = 100;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::Constant);
}

#[test]
fn context_visibility_constant() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public const VERSION = '1.0';\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::Constant);
}

#[test]
fn context_unknown_file_level() {
    let content = "<?php\n/**\n * @\n */\n\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    assert_eq!(detect_context(content, pos), DocblockContext::Unknown);
}

// ── extract_symbol_info ─────────────────────────────────────────

#[test]
fn symbol_info_function_params() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function greet(string $name, int $age): string {\n",
        "    return '';\n",
        "}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let info = extract_symbol_info(content, pos);

    assert_eq!(info.params.len(), 2);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("string")), "$name".to_string())
    );
    assert_eq!(
        info.params[1],
        (Some(PhpType::parse("int")), "$age".to_string())
    );
    assert_eq!(info.return_type, Some(PhpType::parse("string")));
}

#[test]
fn symbol_info_method_no_type_hints() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar($x, $y) {}\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    let info = extract_symbol_info(content, pos);

    assert_eq!(info.params.len(), 2);
    assert_eq!(info.params[0], (None, "$x".to_string()));
    assert_eq!(info.params[1], (None, "$y".to_string()));
    assert_eq!(info.return_type, None);
}

#[test]
fn symbol_info_nullable_return() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function find(int $id): ?User {\n",
        "    return null;\n",
        "}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let info = extract_symbol_info(content, pos);
    assert_eq!(info.return_type, Some(PhpType::parse("?User")));
}

#[test]
fn symbol_info_property_type() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    let info = extract_symbol_info(content, pos);
    assert_eq!(info.type_hint, Some(PhpType::parse("string")));
}

#[test]
fn symbol_info_nullable_property() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    protected ?int $count = 0;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    let info = extract_symbol_info(content, pos);
    assert_eq!(info.type_hint, Some(PhpType::parse("?int")));
}

#[test]
fn symbol_info_readonly_property() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public readonly string $name;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    let info = extract_symbol_info(content, pos);
    assert_eq!(info.type_hint, Some(PhpType::parse("string")));
}

#[test]
fn symbol_info_variadic_param() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function merge(array ...$arrays): array {}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let info = extract_symbol_info(content, pos);
    assert_eq!(info.params.len(), 1);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("array")), "$arrays".to_string())
    );
}

#[test]
fn symbol_info_reference_param() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function swap(int &$a, int &$b): void {}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let info = extract_symbol_info(content, pos);
    assert_eq!(info.params.len(), 2);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("int")), "$a".to_string())
    );
    assert_eq!(
        info.params[1],
        (Some(PhpType::parse("int")), "$b".to_string())
    );
}

#[test]
fn symbol_info_no_params() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function now(): DateTimeImmutable {}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let info = extract_symbol_info(content, pos);
    assert!(info.params.is_empty());
    assert_eq!(info.return_type, Some(PhpType::parse("DateTimeImmutable")));
}

// ── find_existing_param_tags ─────────────────────────────────────

#[test]
fn finds_existing_param_tags() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $name\n",
        " * @param int $age\n",
        " * @\n",
        " */\n",
        "function greet(string $name, int $age, bool $formal): string {}\n",
    );
    let pos = Position {
        line: 4,
        character: 4,
    };
    let existing = find_existing_param_tags(content, pos);
    assert_eq!(existing, vec!["$name", "$age"]);
}

#[test]
fn no_existing_param_tags() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function greet(string $name): string {}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let existing = find_existing_param_tags(content, pos);
    assert!(existing.is_empty());
}

// ── build_phpdoc_completions ────────────────────────────────────

#[test]
fn completions_bare_at_function() {
    // Function with a param, non-void return, and a throw so that smart
    // tags are emitted for @param, @return, and @throws.
    let content = "<?php\n/**\n * @\n */\nfunction foo(string $x): int {\n    throw new RuntimeException('boom');\n}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // Should suggest function tags (some may have pre-filled info)
    assert!(
        labels
            .iter()
            .any(|l| l.starts_with("@param") || l == &"@param Type $name"),
        "Should suggest @param. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("@return")),
        "Should suggest @return. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("@throws")),
        "Should suggest @throws. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l == &"@deprecated"),
        "Should suggest @deprecated"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("@phpstan-assert")),
        "Should suggest @phpstan-assert"
    );

    // Should NOT suggest class-only tags
    assert!(
        !labels.iter().any(|l| l.starts_with("@property")),
        "Should NOT suggest @property in function context"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("@method")),
        "Should NOT suggest @method in function context"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("@mixin")),
        "Should NOT suggest @mixin in function context"
    );
}

#[test]
fn completions_bare_at_class() {
    let content = "<?php\n/**\n * @\n */\nclass Foo {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::ClassLike,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );
    let filter_texts: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(
        filter_texts.contains(&"@property"),
        "Should suggest @property"
    );
    assert!(filter_texts.contains(&"@method"), "Should suggest @method");
    assert!(filter_texts.contains(&"@mixin"), "Should suggest @mixin");
    assert!(
        filter_texts.contains(&"@template"),
        "Should suggest @template"
    );
    assert!(
        filter_texts.contains(&"@deprecated"),
        "Should suggest @deprecated"
    );

    // Should NOT suggest function-only tags
    assert!(
        !filter_texts.contains(&"@param"),
        "Should NOT suggest @param in class context"
    );
    assert!(
        !filter_texts.contains(&"@return"),
        "Should NOT suggest @return in class context"
    );
    assert!(
        !filter_texts.contains(&"@throws"),
        "Should NOT suggest @throws in class context"
    );
}

#[test]
fn completions_bare_at_property() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::Property,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );
    let filter_texts: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(filter_texts.contains(&"@var"), "Should suggest @var");
    assert!(
        filter_texts.contains(&"@deprecated"),
        "Should suggest @deprecated"
    );

    assert!(
        !filter_texts.contains(&"@param"),
        "Should NOT suggest @param in property context"
    );
    assert!(
        !filter_texts.contains(&"@return"),
        "Should NOT suggest @return in property context"
    );
    assert!(
        !filter_texts.contains(&"@method"),
        "Should NOT suggest @method in property context"
    );
}

#[test]
fn completions_bare_at_constant() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    const X = 1;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::Constant,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );
    let filter_texts: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(filter_texts.contains(&"@var"), "Should suggest @var");
    assert!(
        filter_texts.contains(&"@deprecated"),
        "Should suggest @deprecated"
    );

    assert!(
        !filter_texts.contains(&"@param"),
        "Should NOT suggest @param in constant context"
    );
}

#[test]
fn completions_unknown_includes_all() {
    let content = "<?php\n/**\n * @\n */\n\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::Unknown,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );
    let filter_texts: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    // Unknown context: class tags and general tags should appear.
    // @param, @return, @throws are filtered because no function body
    // can be detected (no params, no return, no throws).
    assert!(
        filter_texts.contains(&"@property"),
        "Should suggest @property"
    );
    assert!(filter_texts.contains(&"@method"), "Should suggest @method");
    assert!(filter_texts.contains(&"@var"), "Should suggest @var");
    assert!(
        filter_texts.contains(&"@deprecated"),
        "Should suggest @deprecated"
    );
    assert!(
        filter_texts.contains(&"@inheritdoc"),
        "Should suggest @inheritdoc"
    );
}

#[test]
fn completions_filtered_by_prefix() {
    // Function needs a param so that @param is emitted as a smart item.
    let content = "<?php\n/**\n * @par\n */\nfunction foo(string $x): void {}\n";
    let pos = Position {
        line: 2,
        character: 7,
    };
    let items = build_phpdoc_completions(
        content,
        "@par",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );
    let filter_texts: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(filter_texts.contains(&"@param"), "Should suggest @param");
    assert!(
        !filter_texts.contains(&"@return"),
        "Should NOT suggest @return for prefix @par"
    );
}

#[test]
fn completions_phpstan_prefix() {
    let content = "<?php\n/**\n * @phpstan-a\n */\nfunction foo(): void {}\n";
    let pos = Position {
        line: 2,
        character: 14,
    };
    let items = build_phpdoc_completions(
        content,
        "@phpstan-a",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );
    let filter_texts: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(
        filter_texts.contains(&"@phpstan-assert"),
        "Should suggest @phpstan-assert"
    );
    assert!(
        filter_texts.contains(&"@phpstan-assert-if-true"),
        "Should suggest @phpstan-assert-if-true"
    );
    assert!(
        filter_texts.contains(&"@phpstan-assert-if-false"),
        "Should suggest @phpstan-assert-if-false"
    );
    assert!(
        !filter_texts.contains(&"@phpstan-self-out"),
        "Should NOT suggest @phpstan-self-out for prefix @phpstan-a"
    );
}

#[test]
fn completions_case_insensitive() {
    // Function needs a param so that @param is emitted.
    let content = "<?php\n/**\n * @PAR\n */\nfunction foo(int $n): void {}\n";
    let pos = Position {
        line: 2,
        character: 7,
    };
    let items = build_phpdoc_completions(
        content,
        "@PAR",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );
    let filter_texts: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(
        filter_texts.contains(&"@param"),
        "Should match case-insensitively"
    );
}

#[test]
fn completions_have_keyword_kind() {
    let content = "<?php\n/**\n * @\n */\nfunction foo(): void {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );
    for item in &items {
        assert_eq!(
            item.kind,
            Some(CompletionItemKind::KEYWORD),
            "PHPDoc tags should use KEYWORD kind"
        );
    }
}

#[test]
fn completions_no_duplicates() {
    let content = "<?php\n/**\n * @\n */\n\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::Unknown,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );
    let filter_texts: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();
    let unique: std::collections::HashSet<&&str> = filter_texts.iter().collect();
    assert_eq!(
        filter_texts.len(),
        unique.len(),
        "Should not have duplicate tags. Got: {:?}",
        filter_texts
    );
}

// ── Smart pre-fill tests ────────────────────────────────────────

#[test]
fn smart_param_completions_per_parameter() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function greet(string $name, int $age): string {}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let param_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@param"))
        .collect();

    // Should have one item per parameter
    assert_eq!(
        param_items.len(),
        2,
        "Should have one @param per parameter. Got: {:?}",
        param_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    assert_eq!(param_items[0].label, "@param string $name");
    assert_eq!(
        param_items[0].insert_text.as_deref(),
        Some("param string $name")
    );
    assert_eq!(param_items[1].label, "@param int $age");
    assert_eq!(
        param_items[1].insert_text.as_deref(),
        Some("param int $age")
    );
}

#[test]
fn smart_param_skips_already_documented() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $name\n",
        " * @\n",
        " */\n",
        "function greet(string $name, int $age): string {}\n",
    );
    let pos = Position {
        line: 3,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let param_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@param"))
        .collect();

    // $name is already documented, only $age should appear
    assert_eq!(
        param_items.len(),
        1,
        "Should only suggest undocumented params. Got: {:?}",
        param_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(param_items[0].label, "@param int $age");
}

#[test]
fn smart_return_prefilled() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function getName(): string {}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(
        return_item.is_some(),
        "Should have @return item. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let r = return_item.unwrap();
    assert_eq!(r.label, "@return string");
    assert_eq!(r.insert_text.as_deref(), Some("return string"));
}

#[test]
fn smart_return_void_uses_no_return_item() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function doStuff(): void {}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    // Explicit `: void` type hint → @return is not needed
    assert!(
        return_item.is_none(),
        "Should NOT suggest @return when `: void` type hint is present. Got: {:?}",
        return_item.map(|i| &i.label)
    );
}

#[test]
fn smart_return_skipped_when_already_documented() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @return string\n",
        " * @\n",
        " */\n",
        "function getName(): string {}\n",
    );
    let pos = Position {
        line: 3,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let return_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@return"))
        .collect();

    assert!(
        return_items.is_empty(),
        "Should NOT suggest @return when already documented. Got: {:?}",
        return_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[test]
fn smart_var_prefilled_for_property() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::Property,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_item.is_some(), "Should have @var item");
    let v = var_item.unwrap();
    assert_eq!(v.label, "@var string");
    assert_eq!(v.insert_text.as_deref(), Some("var string"));
}

#[test]
fn smart_var_nullable_property() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    protected ?int $count = 0;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::Property,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_item.is_some(), "Should have @var item");
    assert_eq!(var_item.unwrap().label, "@var ?int");
}

#[test]
fn display_labels_for_generic_tags() {
    let content = "<?php\n/**\n * @\n */\nclass Foo {}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::ClassLike,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let method_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@method"));
    assert!(method_item.is_some(), "Should have @method item");
    assert_eq!(
        method_item.unwrap().label,
        "@method ReturnType name()",
        "@method should show usage pattern as label"
    );

    let template_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@template"));
    assert!(template_item.is_some(), "Should have @template item");
    assert_eq!(
        template_item.unwrap().label,
        "@template T",
        "@template should show usage pattern as label"
    );
}

#[test]
fn display_labels_for_general_tags() {
    // Function with a throw so that @throws appears as a smart item.
    let content =
        "<?php\n/**\n * @\n */\nfunction foo(): void {\n    throw new RuntimeException('x');\n}\n";
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let throws_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@throws"));
    assert!(throws_item.is_some(), "Should have @throws item");
    // Smart item shows the concrete type, not the generic label
    assert_eq!(throws_item.unwrap().label, "@throws RuntimeException");

    // Tags with no special format should use tag as label
    let deprecated_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@deprecated"));
    assert!(deprecated_item.is_some(), "Should have @deprecated item");
    assert_eq!(deprecated_item.unwrap().label, "@deprecated");
}

#[test]
fn smart_param_untyped_params() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function process($data, $options) {}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let param_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@param"))
        .collect();

    assert_eq!(param_items.len(), 2);
    assert_eq!(param_items[0].label, "@param $data");
    assert_eq!(param_items[0].insert_text.as_deref(), Some("param $data"));
    assert_eq!(param_items[1].label, "@param $options");
}

#[test]
fn smart_return_nullable() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function find(): ?User {}\n",
    );
    let pos = Position {
        line: 2,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let return_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@return"));
    assert!(return_item.is_some());
    assert_eq!(return_item.unwrap().label, "@return ?User");
}

#[test]
fn all_params_documented_falls_back_to_generic() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $name\n",
        " * @\n",
        " */\n",
        "function greet(string $name): string {}\n",
    );
    let pos = Position {
        line: 3,
        character: 4,
    };
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::FunctionOrMethod,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let param_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@param"))
        .collect();

    // All params documented → @param is filtered out entirely
    assert!(
        param_items.is_empty(),
        "Should NOT suggest @param when all params are documented. Got: {:?}",
        param_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[test]
fn smart_var_prefilled_for_property_with_prefix() {
    // When typing `@v` (not just bare `@`), the smart pre-filled @var
    // item should still appear — not the generic `@var Type $var` snippet.
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @v\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 9,
    };
    let items = build_phpdoc_completions(
        content,
        "@v",
        DocblockContext::Property,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let var_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@var"))
        .collect();
    assert!(
        !var_items.is_empty(),
        "Should have @var item with @v prefix"
    );
    // There should be exactly one @var item — the smart pre-filled one.
    assert_eq!(
        var_items.len(),
        1,
        "Should have exactly one @var item, got: {:?}",
        var_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let v = var_items[0];
    assert_eq!(
        v.label, "@var string",
        "Should be smart pre-filled, not generic snippet"
    );
    assert_eq!(v.insert_text.as_deref(), Some("var string"));
}

#[test]
fn smart_var_property_with_templates_has_snippet_tab_stops() {
    // When a property's type is a class with @template params, the smart
    // @var completion should use snippet format with tab stops on the
    // template parameters: `var Collection<${1:TKey}, ${2:TModel}>`
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public Collection $items;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };

    let collection_class = Arc::new(ClassInfo {
        name: atom("Collection"),
        template_params: vec![atom("TKey"), atom("TModel")],
        ..ClassInfo::default()
    });
    let collection_ref = collection_class.clone();
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Collection" {
            Some(collection_ref.clone())
        } else {
            None
        }
    };
    let smart = SmartContext {
        inferred_inline_var_type: None,
        class_loader: Some(&class_loader),
        function_loader: None,
        import_target: None,
    };

    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::Property,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &smart,
    );

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_item.is_some(), "Should have @var item");
    let v = var_item.unwrap();
    // Label uses plain text (no tab stops).
    assert_eq!(v.label, "@var Collection<TKey, TModel>");
    // Insert text uses snippet format with tab stops.
    assert_eq!(
        v.insert_text.as_deref(),
        Some("var Collection<${1:TKey}, ${2:TModel}>")
    );
    assert_eq!(
        v.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Should use snippet format when templates are present"
    );
}

#[test]
fn smart_var_property_with_templates_and_prefix_has_snippet_tab_stops() {
    // Same as above but with `@v` prefix — should still produce snippet.
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @v\n",
        "     */\n",
        "    public Collection $items;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 9,
    };

    let collection_class = Arc::new(ClassInfo {
        name: atom("Collection"),
        template_params: vec![atom("TKey"), atom("TModel")],
        ..ClassInfo::default()
    });
    let collection_ref = collection_class.clone();
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Collection" {
            Some(collection_ref.clone())
        } else {
            None
        }
    };
    let smart = SmartContext {
        inferred_inline_var_type: None,
        class_loader: Some(&class_loader),
        function_loader: None,
        import_target: None,
    };

    let items = build_phpdoc_completions(
        content,
        "@v",
        DocblockContext::Property,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &smart,
    );

    let var_items: Vec<_> = items
        .iter()
        .filter(|i| i.filter_text.as_deref() == Some("@var"))
        .collect();
    assert_eq!(
        var_items.len(),
        1,
        "Should have exactly one @var item, got: {:?}",
        var_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    let v = var_items[0];
    assert_eq!(v.label, "@var Collection<TKey, TModel>");
    assert_eq!(
        v.insert_text.as_deref(),
        Some("var Collection<${1:TKey}, ${2:TModel}>")
    );
    assert_eq!(v.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

#[test]
fn smart_var_property_scalar_no_snippet_format() {
    // For scalar types (no templates), the insert text should be plain
    // and insert_text_format should be None (not snippet).
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public string $name;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };

    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::Property,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &SmartContext::EMPTY,
    );

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_item.is_some(), "Should have @var item");
    let v = var_item.unwrap();
    assert_eq!(v.label, "@var string");
    assert_eq!(v.insert_text.as_deref(), Some("var string"));
    assert_eq!(
        v.insert_text_format, None,
        "Scalar type should not use snippet format"
    );
}

#[test]
fn smart_var_property_class_without_templates_no_snippet_format() {
    // A class type without @template params should produce plain text,
    // not snippet format.
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public DateTime $created;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };

    let datetime_class = Arc::new(ClassInfo {
        name: atom("DateTime"),
        template_params: vec![],
        ..ClassInfo::default()
    });
    let datetime_ref = datetime_class.clone();
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "DateTime" {
            Some(datetime_ref.clone())
        } else {
            None
        }
    };
    let smart = SmartContext {
        inferred_inline_var_type: None,
        class_loader: Some(&class_loader),
        function_loader: None,
        import_target: None,
    };

    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::Property,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &smart,
    );

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_item.is_some(), "Should have @var item");
    let v = var_item.unwrap();
    assert_eq!(v.label, "@var DateTime");
    assert_eq!(v.insert_text.as_deref(), Some("var DateTime"));
    assert_eq!(
        v.insert_text_format, None,
        "Class without templates should not use snippet format"
    );
}

#[test]
fn smart_var_constant_with_templates_has_snippet_tab_stops() {
    // Constants also get the smart @var with template snippet tab stops.
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public Collection $items;\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 8,
    };

    let collection_class = Arc::new(ClassInfo {
        name: atom("Collection"),
        template_params: vec![atom("TKey"), atom("TModel")],
        ..ClassInfo::default()
    });
    let collection_ref = collection_class.clone();
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Collection" {
            Some(collection_ref.clone())
        } else {
            None
        }
    };
    let smart = SmartContext {
        inferred_inline_var_type: None,
        class_loader: Some(&class_loader),
        function_loader: None,
        import_target: None,
    };

    // Use Constant context — should behave the same as Property.
    let items = build_phpdoc_completions(
        content,
        "@",
        DocblockContext::Constant,
        pos,
        &std::collections::HashMap::new(),
        &None,
        &smart,
    );

    let var_item = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("@var"));
    assert!(var_item.is_some(), "Should have @var item for constant");
    let v = var_item.unwrap();
    assert_eq!(v.label, "@var Collection<TKey, TModel>");
    assert_eq!(
        v.insert_text.as_deref(),
        Some("var Collection<${1:TKey}, ${2:TModel}>")
    );
    assert_eq!(v.insert_text_format, Some(InsertTextFormat::SNIPPET));
}
