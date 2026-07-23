use super::*;

// ── Message matching ───────────────────────────────────────────

#[test]
fn matches_return_void_message() {
    let msg = "Method Foo::bar() with return type void returns int but should not return anything.";
    assert!(msg.ends_with(RETURN_VOID_MSG_SUFFIX));
}

// ── extract_actual_type ─────────────────────────────────────────

#[test]
fn extracts_actual_type_int() {
    let msg = "Method Foo::bar() with return type void returns int but should not return anything.";
    assert_eq!(extract_actual_type(msg), Some(PhpType::parse("int")));
}

#[test]
fn extracts_actual_type_string() {
    let msg = "Function foo() with return type void returns string but should not return anything.";
    assert_eq!(extract_actual_type(msg), Some(PhpType::parse("string")));
}

#[test]
fn extracts_actual_type_union() {
    let msg =
        "Method X::y() with return type void returns int|string but should not return anything.";
    assert_eq!(extract_actual_type(msg), Some(PhpType::parse("int|string")));
}

#[test]
fn extracts_actual_type_null() {
    let msg = "Method X::y() with return type void returns null but should not return anything.";
    assert_eq!(extract_actual_type(msg), Some(PhpType::parse("null")));
}

#[test]
fn extract_actual_type_returns_none_for_unrelated_message() {
    let msg = "Some other message.";
    assert_eq!(extract_actual_type(msg), None);
}

// ── extract_return_type_actual (return.type) ────────────────────

#[test]
fn extracts_return_type_actual_int() {
    let msg = "Method Foo::bar() should return string but returns int.";
    assert_eq!(extract_return_type_actual(msg), Some(PhpType::parse("int")));
}

#[test]
fn extracts_return_type_actual_union() {
    let msg = "Function foo() should return int but returns int|string.";
    assert_eq!(
        extract_return_type_actual(msg),
        Some(PhpType::parse("int|string"))
    );
}

#[test]
fn extracts_return_type_actual_class() {
    let msg = "Method X::y() should return self but returns App\\Models\\User.";
    assert_eq!(
        extract_return_type_actual(msg),
        Some(PhpType::parse("App\\Models\\User"))
    );
}

#[test]
fn extract_return_type_actual_returns_none_for_unrelated() {
    let msg = "Some other message.";
    assert_eq!(extract_return_type_actual(msg), None);
}

#[test]
fn matches_return_empty_message() {
    let msg = "Method App\\Foo::bar() should return int but empty return statement found.";
    assert!(msg.contains(RETURN_EMPTY_MSG_FRAGMENT));
}

#[test]
fn rejects_unrelated_message() {
    let msg = "Call to function assert() with true will always evaluate to true.";
    assert!(!msg.ends_with(RETURN_VOID_MSG_SUFFIX));
    assert!(!msg.contains(RETURN_EMPTY_MSG_FRAGMENT));
}
