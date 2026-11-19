use std::sync::Arc;

use super::*;
use crate::test_fixtures::{make_class, make_method};

// ── is_legacy_accessor ──────────────────────────────────────────

#[test]
fn legacy_accessor_detected() {
    let method = make_method("getFullNameAttribute", Some("string"));
    assert!(is_legacy_accessor(&method));
}

#[test]
fn legacy_accessor_single_word() {
    let method = make_method("getNameAttribute", Some("string"));
    assert!(is_legacy_accessor(&method));
}

#[test]
fn legacy_accessor_get_attribute_itself_not_accessor() {
    // getAttribute() is a real Eloquent method, not an accessor.
    let method = make_method("getAttribute", Some("mixed"));
    assert!(!is_legacy_accessor(&method));
}

#[test]
fn legacy_accessor_wrong_prefix() {
    let method = make_method("setFullNameAttribute", None);
    assert!(!is_legacy_accessor(&method));
}

#[test]
fn legacy_accessor_no_attribute_suffix() {
    let method = make_method("getFullName", Some("string"));
    assert!(!is_legacy_accessor(&method));
}

#[test]
fn legacy_accessor_lowercase_after_get() {
    // getfooAttribute — first char after "get" must be uppercase.
    let method = make_method("getfooAttribute", Some("string"));
    assert!(!is_legacy_accessor(&method));
}

// ── legacy_accessor_property_name ───────────────────────────────

#[test]
fn legacy_property_name_simple() {
    assert_eq!(legacy_accessor_property_name("getNameAttribute"), "name");
}

#[test]
fn legacy_property_name_multi_word() {
    assert_eq!(
        legacy_accessor_property_name("getFullNameAttribute"),
        "full_name"
    );
}

// ── is_modern_accessor ──────────────────────────────────────────

#[test]
fn modern_accessor_fqn() {
    let method = make_method(
        "fullName",
        Some("Illuminate\\Database\\Eloquent\\Casts\\Attribute"),
    );
    assert!(is_modern_accessor(&method));
}

#[test]
fn modern_accessor_fqn_canonical() {
    let method = make_method(
        "fullName",
        Some("Illuminate\\Database\\Eloquent\\Casts\\Attribute"),
    );
    assert!(is_modern_accessor(&method));
}

#[test]
fn global_attribute_class_is_not_an_eloquent_accessor() {
    let method = make_method("fullName", Some("Attribute"));
    assert!(!is_modern_accessor(&method));
}

#[test]
fn accessor_names_without_import_context_require_the_framework_class() {
    assert!(written_attribute_class(
        "\\Illuminate\\Database\\Eloquent\\Casts\\Attribute",
        None
    ));
    assert!(!written_attribute_class("Attribute", None));
    assert!(!written_attribute_class("Other\\Attribute", None));
}

#[test]
fn modern_setters_are_read_from_the_returned_attribute() {
    use mago_syntax::cst::{ClassLikeMember, Statement};
    for (declaration, expected) in [
        (
            "protected function value(): Value { return new Value(null, fn ($value) => $value); }",
            true,
        ),
        (
            "protected function value(): Value { if ($condition) { return Value::make(set: fn ($value) => $value); } return Value::get(fn () => ''); }",
            true,
        ),
        ("abstract protected function value(): Value;", false),
        (
            "protected function value(): Value { return new Value; }",
            false,
        ),
        (
            "protected function value(): Value { return $factory; }",
            false,
        ),
        (
            "protected function value(): Value { return new $factory(set: fn ($value) => $value); }",
            false,
        ),
        (
            "protected function value(): Value { return Value::{$factory}(set: fn ($value) => $value); }",
            false,
        ),
        (
            "protected function value(): Value { return Other::make(set: fn ($value) => $value); }",
            false,
        ),
        (
            "protected function value(): Value { return Value::make(...$callbacks); }",
            false,
        ),
        (
            "protected function value(): Value { $callback = function () { return Value::set(fn ($value) => $value); }; return Value::get($callback); }",
            false,
        ),
        ("protected function value(): Value { return; }", false),
    ] {
        let source = format!("<?php abstract class Example {{ {declaration} }}");
        crate::parser::with_parsed_program(&source, "accessor_setter", |program, content| {
            let class = program
                .statements
                .iter()
                .find_map(|statement| match statement {
                    Statement::Class(class) => Some(class),
                    _ => None,
                })
                .unwrap();
            let ClassLikeMember::Method(method) = &class.members[0] else {
                panic!("expected method")
            };
            let context = crate::parser::DocblockCtx {
                trivias: program.trivia.as_slice(),
                content,
                php_version: None,
                use_map: [("Value".to_string(), ATTRIBUTE_CAST_FQN.to_string())]
                    .into_iter()
                    .collect(),
                namespace: Some("App".to_string()),
            };
            assert_eq!(
                has_attribute_setter(
                    Some(&PhpType::named("Value".into())),
                    method,
                    Some(&context)
                ),
                expected,
                "{declaration}"
            );
        });
    }
}

#[test]
fn modern_accessor_with_generics() {
    let method = make_method(
        "fullName",
        Some("Illuminate\\Database\\Eloquent\\Casts\\Attribute<string, never>"),
    );
    assert!(is_modern_accessor(&method));
}

#[test]
fn modern_accessor_not_matching_return_type() {
    let method = make_method("fullName", Some("string"));
    assert!(!is_modern_accessor(&method));
}

#[test]
fn modern_accessor_no_return_type() {
    let method = make_method("fullName", None);
    assert!(!is_modern_accessor(&method));
}

// ── extract_modern_accessor_type ────────────────────────────────

#[test]
fn accessor_type_with_single_generic_arg() {
    let method = make_method(
        "firstName",
        Some("Illuminate\\Database\\Eloquent\\Casts\\Attribute<string>"),
    );
    assert_eq!(extract_modern_accessor_type(&method).to_string(), "string");
}

#[test]
fn accessor_type_with_two_generic_args() {
    let method = make_method(
        "firstName",
        Some("Illuminate\\Database\\Eloquent\\Casts\\Attribute<string, never>"),
    );
    assert_eq!(extract_modern_accessor_type(&method).to_string(), "string");
}

#[test]
fn accessor_type_canonical_fqn() {
    let method = make_method(
        "firstName",
        Some("Illuminate\\Database\\Eloquent\\Casts\\Attribute<int>"),
    );
    assert_eq!(extract_modern_accessor_type(&method).to_string(), "int");
}

#[test]
fn accessor_type_short_name_with_generic() {
    let method = make_method("firstName", Some("Attribute<bool>"));
    assert_eq!(extract_modern_accessor_type(&method).to_string(), "bool");
}

#[test]
fn accessor_type_no_generic_falls_back_to_mixed() {
    let method = make_method(
        "firstName",
        Some("Illuminate\\Database\\Eloquent\\Casts\\Attribute"),
    );
    assert_eq!(extract_modern_accessor_type(&method).to_string(), "mixed");
}

#[test]
fn accessor_type_no_return_type_falls_back_to_mixed() {
    let method = make_method("firstName", None);
    assert_eq!(extract_modern_accessor_type(&method).to_string(), "mixed");
}

#[test]
fn accessor_type_nullable_generic_arg() {
    let method = make_method("firstName", Some("Attribute<?string>"));
    assert_eq!(extract_modern_accessor_type(&method).to_string(), "?string");
}

#[test]
fn accessor_type_union_generic_arg() {
    let method = make_method("firstName", Some("Attribute<string|null>"));
    assert_eq!(
        extract_modern_accessor_type(&method).to_string(),
        "string|null"
    );
}

// ── is_accessor_method ──────────────────────────────────────────

#[test]
fn accessor_method_legacy() {
    let mut class = make_class("App\\Models\\User");
    class.methods.push(Arc::new(make_method(
        "getFullNameAttribute",
        Some("string"),
    )));
    assert!(is_accessor_or_mutator_method(
        &class,
        "getFullNameAttribute"
    ));
}

#[test]
fn accessor_method_modern() {
    let mut class = make_class("App\\Models\\User");
    class.methods.push(Arc::new(make_method(
        "fullName",
        Some("Illuminate\\Database\\Eloquent\\Casts\\Attribute"),
    )));
    assert!(is_accessor_or_mutator_method(&class, "fullName"));
}

#[test]
fn accessor_method_not_found() {
    let class = make_class("App\\Models\\User");
    assert!(!is_accessor_or_mutator_method(&class, "nonexistent"));
}

#[test]
fn accessor_method_non_accessor() {
    let mut class = make_class("App\\Models\\User");
    class
        .methods
        .push(Arc::new(make_method("posts", Some("HasMany<Post>"))));
    assert!(!is_accessor_or_mutator_method(&class, "posts"));
}
