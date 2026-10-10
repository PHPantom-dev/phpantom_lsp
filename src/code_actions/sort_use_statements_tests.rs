use super::*;

fn edits_of(content: &str) -> Vec<TextEdit> {
    compute_sort_use_edits(content)
}

fn lsp_pos_to_offset(content: &str, pos: Position) -> usize {
    let mut offset = 0;
    for (i, line) in content.lines().enumerate() {
        if i == pos.line as usize {
            return offset + pos.character as usize;
        }
        offset += line.len() + 1;
    }
    content.len()
}

fn apply(content: &str, edits: &[TextEdit]) -> String {
    let mut result = content.to_string();
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));

    for edit in sorted {
        let start = lsp_pos_to_offset(&result, edit.range.start);
        let end = lsp_pos_to_offset(&result, edit.range.end);
        result.replace_range(start..end, &edit.new_text);
    }
    result
}

#[test]
fn sorts_unsorted_class_imports() {
    let content = "<?php\nuse Zebra\\Foo;\nuse Aardvark\\Bar;\n\nclass Test {}\n";
    let edits = edits_of(content);
    assert!(!edits.is_empty(), "should offer edits for unsorted block");
    let result = apply(content, &edits);
    assert_eq!(
        result,
        "<?php\nuse Aardvark\\Bar;\nuse Zebra\\Foo;\n\nclass Test {}\n"
    );
}

#[test]
fn already_sorted_produces_no_edits() {
    let content = "<?php\nuse Aardvark\\Bar;\nuse Zebra\\Foo;\n\nclass Test {}\n";
    assert!(edits_of(content).is_empty());
}

#[test]
fn sorts_by_imported_name_not_alias() {
    let content = "<?php\nuse Zebra\\Foo as Aardvark;\nuse Yak\\Bar;\n\nclass Test {}\n";
    let edits = edits_of(content);
    let result = apply(content, &edits);
    // Zebra\Foo sorts under "zebra", after "yak", even though its alias
    // "Aardvark" would sort first.
    assert_eq!(
        result,
        "<?php\nuse Yak\\Bar;\nuse Zebra\\Foo as Aardvark;\n\nclass Test {}\n"
    );
}

#[test]
fn group_use_sorts_on_prefix() {
    let content = "<?php\nuse Zebra\\Foo;\nuse Aardvark\\{Bar, Baz};\n\nclass Test {}\n";
    let edits = edits_of(content);
    let result = apply(content, &edits);
    assert_eq!(
        result,
        "<?php\nuse Aardvark\\{Bar, Baz};\nuse Zebra\\Foo;\n\nclass Test {}\n"
    );
}

#[test]
fn does_not_interleave_import_kinds() {
    let content = "<?php\nuse function Zebra\\zeb;\nuse Zebra\\Foo;\nuse const Zebra\\ZEB;\nuse Aardvark\\Bar;\n\nclass Test {}\n";
    let edits = edits_of(content);
    let result = apply(content, &edits);
    assert_eq!(
        result,
        "<?php\nuse Aardvark\\Bar;\nuse Zebra\\Foo;\nuse const Zebra\\ZEB;\nuse function Zebra\\zeb;\n\nclass Test {}\n"
    );
}

#[test]
fn blank_line_separates_independent_groups() {
    let content = "<?php\nuse Zebra\\Foo;\nuse Aardvark\\Bar;\n\nuse Yak\\Second;\nuse Bison\\First;\n\nclass Test {}\n";
    let edits = edits_of(content);
    let result = apply(content, &edits);
    assert_eq!(
        result,
        "<?php\nuse Aardvark\\Bar;\nuse Zebra\\Foo;\n\nuse Bison\\First;\nuse Yak\\Second;\n\nclass Test {}\n"
    );
}

#[test]
fn leading_comment_moves_with_its_import() {
    let content = "<?php\nuse Zebra\\Foo;\n// Explains Bar\nuse Aardvark\\Bar;\n\nclass Test {}\n";
    let edits = edits_of(content);
    let result = apply(content, &edits);
    assert_eq!(
        result,
        "<?php\n// Explains Bar\nuse Aardvark\\Bar;\nuse Zebra\\Foo;\n\nclass Test {}\n"
    );
}

#[test]
fn trailing_comment_moves_with_its_import() {
    let content = "<?php\nuse Zebra\\Foo; // zeb\nuse Aardvark\\Bar; // aard\n\nclass Test {}\n";
    let edits = edits_of(content);
    let result = apply(content, &edits);
    assert_eq!(
        result,
        "<?php\nuse Aardvark\\Bar; // aard\nuse Zebra\\Foo; // zeb\n\nclass Test {}\n"
    );
}

#[test]
fn multiline_doc_comment_moves_with_its_import() {
    let content =
        "<?php\nuse Zebra\\Foo;\n/**\n * Explains Bar.\n */\nuse Aardvark\\Bar;\n\nclass Test {}\n";
    let edits = edits_of(content);
    let result = apply(content, &edits);
    assert_eq!(
        result,
        "<?php\n/**\n * Explains Bar.\n */\nuse Aardvark\\Bar;\nuse Zebra\\Foo;\n\nclass Test {}\n"
    );
}

#[test]
fn single_import_produces_no_edits() {
    let content = "<?php\nuse Foo\\Bar;\n\nclass Test {}\n";
    assert!(edits_of(content).is_empty());
}

#[test]
fn no_use_block_produces_no_edits() {
    let content = "<?php\nclass Test {}\n";
    assert!(edits_of(content).is_empty());
}

#[test]
fn multiline_group_use_sorts_as_one_entry() {
    let content =
        "<?php\nuse Zebra\\Foo;\nuse Aardvark\\{\n    Bar,\n    Baz,\n};\n\nclass Test {}\n";
    let edits = edits_of(content);
    let result = apply(content, &edits);
    assert_eq!(
        result,
        "<?php\nuse Aardvark\\{\n    Bar,\n    Baz,\n};\nuse Zebra\\Foo;\n\nclass Test {}\n"
    );
}

#[test]
fn keeps_crlf_line_endings() {
    let content = "<?php\r\nuse Zebra\\Foo;\r\nuse Aardvark\\Bar;\r\n\r\nclass Test {}\r\n";
    let result = crate::text_position::apply_text_edits(content, &edits_of(content));
    assert_eq!(
        result,
        "<?php\r\nuse Aardvark\\Bar;\r\nuse Zebra\\Foo;\r\n\r\nclass Test {}\r\n"
    );
}
