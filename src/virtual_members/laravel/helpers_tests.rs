use super::*;
use crate::atom::atom;
use crate::class_lookup::Fqn;
use crate::test_fixtures::{make_class, no_loader};
use crate::types::ClassInfo;
use std::sync::Arc;

// ── is_eloquent_model ───────────────────────────────────────────────

#[test]
fn recognises_fqn() {
    assert!(is_eloquent_model("Illuminate\\Database\\Eloquent\\Model"));
}

#[test]
fn rejects_unrelated_class() {
    assert!(!is_eloquent_model("App\\Models\\User"));
}

// ── extends_eloquent_model ──────────────────────────────────────────

#[test]
fn direct_child_of_model() {
    let mut user = make_class("App\\Models\\User");
    user.parent_class = Some(atom("Illuminate\\Database\\Eloquent\\Model"));

    let model = make_class("Illuminate\\Database\\Eloquent\\Model");
    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        if name == "Illuminate\\Database\\Eloquent\\Model" {
            Some(Arc::new(model.clone()))
        } else {
            None
        }
    };

    assert!(extends_eloquent_model(&user, &loader));
}

#[test]
fn indirect_child_of_model() {
    let mut user = make_class("App\\Models\\Admin");
    user.parent_class = Some(atom("App\\Models\\User"));

    let mut base = make_class("App\\Models\\User");
    base.parent_class = Some(atom("Illuminate\\Database\\Eloquent\\Model"));

    let model = make_class("Illuminate\\Database\\Eloquent\\Model");

    let loader = |name: Fqn<'_>| -> Option<Arc<ClassInfo>> {
        let name = name.as_str();
        let name = name.strip_prefix('\\').unwrap_or(name);
        match name {
            "App\\Models\\User" => Some(Arc::new(base.clone())),
            "Illuminate\\Database\\Eloquent\\Model" => Some(Arc::new(model.clone())),
            _ => None,
        }
    };

    assert!(extends_eloquent_model(&user, &loader));
}

#[test]
fn not_a_model() {
    let service = make_class("App\\Services\\UserService");
    assert!(!extends_eloquent_model(&service, &no_loader));
}

// ── camel_to_snake ──────────────────────────────────────────────

#[test]
fn camel_to_snake_simple() {
    assert_eq!(camel_to_snake("FullName"), "full_name");
}

#[test]
fn camel_to_snake_single_word() {
    assert_eq!(camel_to_snake("Name"), "name");
}

#[test]
fn camel_to_snake_already_lower() {
    assert_eq!(camel_to_snake("name"), "name");
}

#[test]
fn camel_to_snake_camel_case() {
    assert_eq!(camel_to_snake("firstName"), "first_name");
}

#[test]
fn camel_to_snake_multiple_words() {
    assert_eq!(camel_to_snake("isAdminUser"), "is_admin_user");
}

#[test]
fn camel_to_snake_with_digit() {
    assert_eq!(camel_to_snake("item2Name"), "item2_name");
}

#[test]
fn camel_to_snake_acronym() {
    assert_eq!(camel_to_snake("URLName"), "url_name");
}

// ── legacy_accessor_method_name ─────────────────────────────────

#[test]
fn legacy_accessor_prop_name_simple() {
    assert_eq!(legacy_accessor_method_name("name"), "getNameAttribute");
}

#[test]
fn legacy_accessor_prop_name_multi_word() {
    assert_eq!(
        legacy_accessor_method_name("display_name"),
        "getDisplayNameAttribute"
    );
}

#[test]
fn legacy_accessor_prop_name_three_words() {
    assert_eq!(
        legacy_accessor_method_name("full_legal_name"),
        "getFullLegalNameAttribute"
    );
}
