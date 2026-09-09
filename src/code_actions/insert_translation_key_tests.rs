use super::*;
use crate::text_position::position_to_offset;

fn apply(content: &str, path: &str) -> String {
    let mut edits = insertion_edits(content, path).unwrap_or_else(|| panic!("no edits: {content}"));
    edits.sort_by_key(|edit| edit.range.start);
    let mut result = content.to_string();
    for edit in edits.into_iter().rev() {
        let start = position_to_offset(content, edit.range.start) as usize;
        let end = position_to_offset(content, edit.range.end) as usize;
        result.replace_range(start..end, &edit.new_text);
    }
    crate::parser::with_parsed_program(&result, "verify_translation_edit", |program, _| {
        assert!(program.errors.is_empty(), "{result}: {:?}", program.errors);
    });
    assert!(
        insertion_edits(&result, path).is_none(),
        "must not insert a duplicate"
    );
    result
}

#[test]
fn translation_insertion_preserves_siblings_comments_and_layout() {
    for (source, expected) in [
        ("<?php return [];", "<?php return ['new' => ''];"),
        (
            "<?php return ['old' => 'yes'];",
            "<?php return ['old' => 'yes', 'new' => ''];",
        ),
        (
            "<?php return ['old' => 'yes',];",
            "<?php return ['old' => 'yes', 'new' => '',];",
        ),
        (
            "<?php return [\n    'old' => 'yes' // keep me\n];",
            "<?php return [\n    'old' => 'yes', // keep me\n    'new' => '',\n];",
        ),
        (
            "<?php return [\r\n\t'old' => 'yes',\r\n];",
            "<?php return [\r\n\t'old' => 'yes',\r\n\t'new' => '',\r\n];",
        ),
        ("<?php return [\n];", "<?php return [\n    'new' => '',\n];"),
        ("<?php return array();", "<?php return array('new' => '');"),
        ("<?php return ([]);", "<?php return (['new' => '']);"),
    ] {
        assert_eq!(apply(source, "new"), expected);
    }
    let multiline = apply("<?php return [\n 'old' => 'yes'];", "new");
    assert!(multiline.contains("'old' => 'yes',\n 'new' => '',\n]"));
}

#[test]
fn translation_insertion_adds_nested_keys_without_overwriting_existing_values() {
    let result = apply(
        "<?php return ['checkout' => ['existing' => 'yes']];",
        "checkout.address.label",
    );
    assert_eq!(
        result,
        "<?php return ['checkout' => ['existing' => 'yes', 'address' => ['label' => '']]];"
    );
    assert_eq!(
        apply("<?php return [];", "it's.back\\slash"),
        "<?php return ['it\\'s' => ['back\\\\slash' => '']];"
    );
    for source in [
        "<?php return ['checkout' => 'scalar'];",
        "<?php return ['checkout' => []];",
        "<?php return ['checkout' => 'first', 'checkout' => 'last'];",
        "<?php return array_merge([], []);",
        "<?php return $values;",
        "<?php $x = [];",
        "<?php return [;",
        "<?php return ['value'];",
        "<?php return [...$values];",
        "<?php return [$dynamic => 'value'];",
        "<?php return [1 => 'value'];",
    ] {
        assert!(insertion_edits(source, "checkout").is_none(), "{source}");
    }
    assert!(insertion_edits("<?php return ['checkout' => 'scalar'];", "checkout.title").is_none());
}
