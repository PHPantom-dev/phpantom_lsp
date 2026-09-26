use super::{
    ArrayWriteKey, merge_array_plus, merge_keyed_type, merge_nested_array_write, merge_push_type,
    normalize_array_key_type,
};
use crate::atom::atom;
use crate::php_type::{PhpType, ShapeEntry};

#[test]
fn collection_key_normalization_preserves_non_numeric_string_domains() {
    assert_eq!(
        normalize_array_key_type(&PhpType::parse("class-string<Foo>")),
        Some(PhpType::parse("class-string<Foo>"))
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::parse("interface-string<Foo>")),
        Some(PhpType::parse("interface-string<Foo>"))
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::parse("class-string")),
        Some(PhpType::parse("class-string"))
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::string())
            .unwrap()
            .to_string(),
        "string"
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::named(atom("number"))),
        Some(PhpType::int())
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::named(atom("Number"))),
        None
    );
    // `\x38` decodes to `"8"`, a canonical decimal integer key, once
    // escapes are decoded.
    assert_eq!(
        normalize_array_key_type(&PhpType::literal_string_raw("\"\\x38\""))
            .unwrap()
            .to_string(),
        "int"
    );
}

/// An element write refines what the variable already tracks. It never
/// leaves a tracked shape untouched, and never rebuilds a keyed array as a
/// shape that claims the written key is the only one there.
#[test]
fn element_writes_refine_the_type_they_are_written_into() {
    let write = |base: &str, keys: Vec<ArrayWriteKey>, value: &str| {
        merge_nested_array_write(&PhpType::parse(base), &keys, &PhpType::parse(value), false)
            .to_string()
    };
    let shape = |key: &str| ArrayWriteKey::Shape(key.to_string());

    // An append lands on the next free integer key, keeping the keys the
    // shape already tracks.
    assert_eq!(
        write("array{name: string}", vec![ArrayWriteKey::Append], "User"),
        "array{name: string, User}"
    );
    assert_eq!(
        write("array{5: string}", vec![ArrayWriteKey::Append], "User"),
        "array{5: string, 6: User}"
    );
    // The same append one level down refines that entry rather than
    // leaving the value it was initialised with. The entry is a literal's
    // positional shape, and a straight-line append keeps its arity,
    // extending it by the one entry PHP appends.
    assert_eq!(
        write(
            "array{rows: array{'first'}}",
            vec![shape("rows"), ArrayWriteKey::Append],
            "string",
        ),
        "array{rows: array{'first', string}}"
    );
    // A dynamic key may land on any entry, so the shape widens instead of
    // standing still.
    assert_eq!(
        write(
            "array{name: string}",
            vec![ArrayWriteKey::Keyed {
                key_type: PhpType::string(),
                slot: None,
            }],
            "int",
        ),
        "non-empty-array<string, string|int>"
    );
    // A written-out integer key the shape already spells out updates that
    // entry in place, the same as a positional slot would.
    assert_eq!(
        write(
            "array{1: string, 2: int}",
            vec![ArrayWriteKey::Keyed {
                key_type: PhpType::int(),
                slot: Some(1),
            }],
            "string",
        ),
        "array{1: string, 2: int}"
    );
    // A keyed array keeps its key and value types through a literal-key
    // write and through an append.
    assert_eq!(
        write("array<string, int>", vec![shape("name")], "int"),
        "non-empty-array<string, int>"
    );
    assert_eq!(
        write("array<string, int>", vec![ArrayWriteKey::Append], "int"),
        "non-empty-array<string|int, int>"
    );
    // An auto-vivified level starts from what the base says sits there.
    // The written key is non-empty afterwards, but it joins the entries
    // the write did not touch, so the outer value domain stays the wider
    // `list<string>`.
    assert_eq!(
        write(
            "array<string, list<string>>",
            vec![shape("words"), ArrayWriteKey::Append],
            "string",
        ),
        "non-empty-array<string, list<string>>"
    );
}

/// A written-out integer the shape does not have yet adds that slot, and a
/// write below a dynamic key replaces the element it started from rather
/// than joining it, so the added slot can be read back as present.
#[test]
fn integer_key_writes_add_the_slot_to_the_shape_they_reach() {
    let write = |base: &str, keys: Vec<ArrayWriteKey>, value: &str| {
        merge_nested_array_write(&PhpType::parse(base), &keys, &PhpType::parse(value), false)
            .to_string()
    };
    let slot = |index: usize| ArrayWriteKey::Keyed {
        key_type: PhpType::literal_int(index.to_string()),
        slot: Some(index),
    };

    assert_eq!(
        write("array{string, bool}", vec![slot(2)], "int"),
        "array{string, bool, int}"
    );
    assert_eq!(
        write("array{string, bool}", vec![slot(5)], "int"),
        "array{string, bool, 5: int}"
    );
    // An empty shape has no arity to keep.
    assert_eq!(
        write("array{}", vec![slot(0)], "int"),
        "non-empty-array<int, int>"
    );
    assert_eq!(
        write(
            "array<string, array{string, bool, string}>",
            vec![
                ArrayWriteKey::Keyed {
                    key_type: PhpType::string(),
                    slot: None,
                },
                slot(3),
            ],
            "array{'I'}",
        ),
        "non-empty-array<string, array{string, bool, string, array{'I'}}>"
    );
}

/// A union of shapes is one of them at runtime, so a write lands in each
/// alternative rather than folding them all into one generic pair.
#[test]
fn a_write_into_a_union_of_shapes_updates_each_shape() {
    let slot = ArrayWriteKey::Keyed {
        key_type: PhpType::literal_int("3"),
        slot: Some(3),
    };
    assert_eq!(
        merge_nested_array_write(
            &PhpType::parse("list{'a', bool, 'c'}|array{string, bool, string}"),
            &[slot],
            &PhpType::parse("list<string>"),
            false,
        )
        .to_string(),
        "list{'a', bool, 'c', list<string>}|array{string, bool, string, list<string>}"
    );
}

/// A straight-line `[]` append keeps the shape's arity and the exact value
/// it wrote, the same as an array literal's own entries — only a write
/// inside a loop body (`in_loop: true`) widens straight to the list it is
/// building, since the fixed-point walk cannot know how many times the
/// statement actually runs.
#[test]
fn straight_line_append_keeps_the_shape_and_its_literals() {
    assert_eq!(
        merge_nested_array_write(
            &PhpType::array_shape(Vec::new()),
            &[ArrayWriteKey::Append],
            &PhpType::literal_string_raw("'one'"),
            false,
        ),
        PhpType::array_shape(vec![ShapeEntry {
            key: None,
            value_type: PhpType::literal_string_raw("'one'"),
            optional: false,
        }])
    );

    let literal_entry = |value_type: PhpType| ShapeEntry {
        key: None,
        value_type,
        optional: false,
    };
    let three_literals = PhpType::array_shape(vec![
        literal_entry(PhpType::literal_int("1")),
        literal_entry(PhpType::literal_int("2")),
        literal_entry(PhpType::literal_int("3")),
    ]);
    assert_eq!(
        merge_nested_array_write(
            &three_literals,
            &[ArrayWriteKey::Append],
            &PhpType::null(),
            false,
        ),
        PhpType::array_shape(vec![
            literal_entry(PhpType::literal_int("1")),
            literal_entry(PhpType::literal_int("2")),
            literal_entry(PhpType::literal_int("3")),
            literal_entry(PhpType::null()),
        ])
    );

    // The same append, marked as sitting inside a loop body, cannot know
    // how many times it actually runs, so it widens straight to the list
    // instead of growing the shape by one entry per re-walk.
    assert_eq!(
        merge_nested_array_write(
            &three_literals,
            &[ArrayWriteKey::Append],
            &PhpType::null(),
            true
        )
        .to_string(),
        "non-empty-list<1|2|3|null>"
    );
}

/// `+` unions two shapes key by key, and positional entries have keys —
/// the index they sit at.
#[test]
fn array_plus_unions_positional_entries_by_index() {
    let plus = |lhs: &str, rhs: &str| {
        merge_array_plus(&PhpType::parse(lhs), &PhpType::parse(rhs)).to_string()
    };

    assert_eq!(
        plus("list{int, string}", "list{float}"),
        "list{int, string}"
    );
    assert_eq!(
        plus("list{int}", "list{float, string}"),
        "list{int, string}"
    );
    assert_eq!(
        plus("array{int, string}", "array{slot: Pen}"),
        "array{int, string, slot: Pen}"
    );
    assert_eq!(
        plus("array{name: string}", "array{Pen}"),
        "array{name: string, 0: Pen}"
    );
}

#[test]
fn mutable_collection_merges_normalize_complete_existing_domains() {
    assert_eq!(
        merge_push_type(
            &PhpType::parse("list<'existing'>"),
            &PhpType::literal_string_raw("'new'"),
        ),
        PhpType::parse("list<string>")
    );
    assert_eq!(
        merge_keyed_type(
            &PhpType::parse("array<'existing-key', 'existing-value'>"),
            &PhpType::literal_string_raw("'new-key'"),
            &PhpType::literal_string_raw("'new-value'"),
        ),
        PhpType::parse("array<string, string>")
    );
    assert_eq!(
        merge_keyed_type(
            &PhpType::parse("list<'existing-value'>"),
            &PhpType::literal_string_raw("'named-key'"),
            &PhpType::literal_string_raw("'new-value'"),
        ),
        PhpType::parse("array<int|string, string>")
    );
}
