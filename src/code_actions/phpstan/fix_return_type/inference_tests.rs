use super::*;

// ── infer_type_from_literal ─────────────────────────────────────

#[test]
fn literal_int() {
    assert_eq!(
        infer_type_from_literal("42").map(|t| t.to_string()),
        Some("int".to_string())
    );
    assert_eq!(
        infer_type_from_literal("-1").map(|t| t.to_string()),
        Some("int".to_string())
    );
}

#[test]
fn literal_float() {
    assert_eq!(
        infer_type_from_literal("1.5").map(|t| t.to_string()),
        Some("float".to_string())
    );
}

#[test]
fn literal_bool() {
    assert_eq!(
        infer_type_from_literal("true").map(|t| t.to_string()),
        Some("bool".to_string())
    );
    assert_eq!(
        infer_type_from_literal("false").map(|t| t.to_string()),
        Some("bool".to_string())
    );
}

#[test]
fn literal_string() {
    assert_eq!(
        infer_type_from_literal("'hello'").map(|t| t.to_string()),
        Some("string".to_string())
    );
    assert_eq!(
        infer_type_from_literal("\"world\"").map(|t| t.to_string()),
        Some("string".to_string())
    );
}

#[test]
fn literal_array_empty() {
    assert_eq!(
        infer_type_from_literal("[]").map(|t| t.to_string()),
        Some("array".to_string())
    );
}

#[test]
fn literal_array_of_strings() {
    assert_eq!(
        infer_type_from_literal("['string']").map(|t| t.to_string()),
        Some("list<string>".to_string())
    );
    assert_eq!(
        infer_type_from_literal("['a', 'b', 'c']").map(|t| t.to_string()),
        Some("list<string>".to_string())
    );
}

#[test]
fn literal_array_of_ints() {
    assert_eq!(
        infer_type_from_literal("[1, 2, 3]").map(|t| t.to_string()),
        Some("list<int>".to_string())
    );
}

#[test]
fn literal_array_mixed_scalars() {
    assert_eq!(
        infer_type_from_literal("['a', 1]").map(|t| t.to_string()),
        Some("list<string|int>".to_string())
    );
}

#[test]
fn literal_array_with_string_keys() {
    assert_eq!(
        infer_type_from_literal("['key' => 'value']").map(|t| t.to_string()),
        Some("array<string, string>".to_string())
    );
    assert_eq!(
        infer_type_from_literal("['name' => 'Alice', 'age' => 42]").map(|t| t.to_string()),
        Some("array<string, string|int>".to_string())
    );
}

#[test]
fn literal_array_nested() {
    assert_eq!(
        infer_type_from_literal("[['a'], ['b']]").map(|t| t.to_string()),
        Some("list<list<string>>".to_string())
    );
}

#[test]
fn literal_array_with_variable_falls_back() {
    assert_eq!(
        infer_type_from_literal("[$var, 'a']").map(|t| t.to_string()),
        Some("array".to_string())
    );
}

#[test]
fn literal_array_legacy_syntax() {
    assert_eq!(
        infer_type_from_literal("array('a', 'b')").map(|t| t.to_string()),
        Some("list<string>".to_string())
    );
}

#[test]
fn literal_array_new_objects() {
    assert_eq!(
        infer_type_from_literal("[new Foo(), new Foo()]").map(|t| t.to_string()),
        Some("list<Foo>".to_string())
    );
}

#[test]
fn literal_array_trailing_comma() {
    assert_eq!(
        infer_type_from_literal("['a', 'b',]").map(|t| t.to_string()),
        Some("list<string>".to_string())
    );
}

#[test]
fn literal_new_class() {
    assert_eq!(
        infer_type_from_literal("new Foo()").map(|t| t.to_string()),
        Some("Foo".to_string())
    );
}

#[test]
fn literal_null() {
    assert_eq!(
        infer_type_from_literal("null").map(|t| t.to_string()),
        Some("null".to_string())
    );
}

#[test]
fn non_literal_returns_none() {
    assert_eq!(infer_type_from_literal("$var"), None);
    assert_eq!(infer_type_from_literal("$this->bar()"), None);
    assert_eq!(infer_type_from_literal("foo()"), None);
    assert_eq!(infer_type_from_literal("Str::toUpper($x)"), None);
}
