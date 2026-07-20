use super::*;
use crate::atom::atom;
use crate::php_type::PhpType;
use crate::test_fixtures::{make_class, make_method, no_loader};
use crate::virtual_members::laravel::CONFIGURED_DATE_CLASS_FQN;
use std::sync::Arc;

// ── cast_type_to_php_type: built-in types ───────────────────────────

#[test]
fn cast_datetime_maps_to_carbon() {
    assert_eq!(
        cast_type_to_php_type("datetime", &no_loader).to_string(),
        CONFIGURED_DATE_CLASS_FQN
    );
}

#[test]
fn cast_date_maps_to_carbon() {
    assert_eq!(
        cast_type_to_php_type("date", &no_loader).to_string(),
        CONFIGURED_DATE_CLASS_FQN
    );
}

#[test]
fn cast_timestamp_maps_to_int() {
    assert_eq!(
        cast_type_to_php_type("timestamp", &no_loader).to_string(),
        "int"
    );
}

#[test]
fn cast_immutable_datetime_maps_to_carbon_immutable() {
    assert_eq!(
        cast_type_to_php_type("immutable_datetime", &no_loader).to_string(),
        "Carbon\\CarbonImmutable"
    );
}

#[test]
fn cast_immutable_date_maps_to_carbon_immutable() {
    assert_eq!(
        cast_type_to_php_type("immutable_date", &no_loader).to_string(),
        "Carbon\\CarbonImmutable"
    );
}

#[test]
fn cast_boolean_maps_to_bool() {
    assert_eq!(
        cast_type_to_php_type("boolean", &no_loader).to_string(),
        "bool"
    );
}

#[test]
fn cast_bool_maps_to_bool() {
    assert_eq!(
        cast_type_to_php_type("bool", &no_loader).to_string(),
        "bool"
    );
}

#[test]
fn cast_integer_maps_to_int() {
    assert_eq!(
        cast_type_to_php_type("integer", &no_loader).to_string(),
        "int"
    );
}

#[test]
fn cast_int_maps_to_int() {
    assert_eq!(cast_type_to_php_type("int", &no_loader).to_string(), "int");
}

#[test]
fn cast_float_maps_to_float() {
    assert_eq!(
        cast_type_to_php_type("float", &no_loader).to_string(),
        "float"
    );
}

#[test]
fn cast_double_maps_to_float() {
    assert_eq!(
        cast_type_to_php_type("double", &no_loader).to_string(),
        "float"
    );
}

#[test]
fn cast_real_maps_to_float() {
    assert_eq!(
        cast_type_to_php_type("real", &no_loader).to_string(),
        "float"
    );
}

#[test]
fn cast_string_maps_to_string() {
    assert_eq!(
        cast_type_to_php_type("string", &no_loader).to_string(),
        "string"
    );
}

#[test]
fn cast_array_maps_to_array() {
    assert_eq!(
        cast_type_to_php_type("array", &no_loader).to_string(),
        "array"
    );
}

#[test]
fn cast_json_maps_to_array() {
    assert_eq!(
        cast_type_to_php_type("json", &no_loader).to_string(),
        "array"
    );
}

#[test]
fn cast_object_maps_to_object() {
    assert_eq!(
        cast_type_to_php_type("object", &no_loader).to_string(),
        "object"
    );
}

#[test]
fn cast_collection_maps_to_illuminate_collection() {
    assert_eq!(
        cast_type_to_php_type("collection", &no_loader).to_string(),
        "Illuminate\\Support\\Collection"
    );
}

#[test]
fn cast_encrypted_maps_to_string() {
    assert_eq!(
        cast_type_to_php_type("encrypted", &no_loader).to_string(),
        "string"
    );
}

#[test]
fn cast_encrypted_array_maps_to_array() {
    assert_eq!(
        cast_type_to_php_type("encrypted:array", &no_loader).to_string(),
        "array"
    );
}

#[test]
fn cast_encrypted_collection_maps_to_collection() {
    assert_eq!(
        cast_type_to_php_type("encrypted:collection", &no_loader).to_string(),
        "Illuminate\\Support\\Collection"
    );
}

#[test]
fn cast_encrypted_object_maps_to_object() {
    assert_eq!(
        cast_type_to_php_type("encrypted:object", &no_loader).to_string(),
        "object"
    );
}

#[test]
fn cast_hashed_maps_to_string() {
    assert_eq!(
        cast_type_to_php_type("hashed", &no_loader).to_string(),
        "string"
    );
}

// ── cast_type_to_php_type: decimal variants ─────────────────────────

#[test]
fn cast_decimal_with_precision_maps_to_float() {
    assert_eq!(
        cast_type_to_php_type("decimal:2", &no_loader).to_string(),
        "float"
    );
}

#[test]
fn cast_decimal_bare_maps_to_float() {
    assert_eq!(
        cast_type_to_php_type("decimal", &no_loader).to_string(),
        "float"
    );
}

// ── cast_type_to_php_type: datetime/date format variants ────────────

#[test]
fn cast_datetime_with_format_maps_to_carbon() {
    assert_eq!(
        cast_type_to_php_type("datetime:Y-m-d", &no_loader).to_string(),
        CONFIGURED_DATE_CLASS_FQN
    );
}

#[test]
fn cast_date_with_format_maps_to_carbon() {
    assert_eq!(
        cast_type_to_php_type("date:Y-m-d", &no_loader).to_string(),
        CONFIGURED_DATE_CLASS_FQN
    );
}

#[test]
fn cast_immutable_datetime_with_format() {
    assert_eq!(
        cast_type_to_php_type("immutable_datetime:Y-m-d H:i:s", &no_loader).to_string(),
        "Carbon\\CarbonImmutable"
    );
}

#[test]
fn cast_immutable_date_with_format() {
    assert_eq!(
        cast_type_to_php_type("immutable_date:Y-m-d", &no_loader).to_string(),
        "Carbon\\CarbonImmutable"
    );
}

// ── cast_type_to_php_type: case insensitivity and unknown ───────────

#[test]
fn cast_case_insensitive() {
    assert_eq!(
        cast_type_to_php_type("Boolean", &no_loader).to_string(),
        "bool"
    );
    assert_eq!(
        cast_type_to_php_type("DATETIME", &no_loader).to_string(),
        CONFIGURED_DATE_CLASS_FQN
    );
    assert_eq!(
        cast_type_to_php_type("Integer", &no_loader).to_string(),
        "int"
    );
}

#[test]
fn cast_unknown_type_falls_back_to_mixed() {
    assert_eq!(
        cast_type_to_php_type("unknown_cast", &no_loader).to_string(),
        "mixed"
    );
}

// ── cast_type_to_php_type: custom cast classes ──────────────────────

#[test]
fn cast_custom_class_with_get_method() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\MoneyCast" {
            let mut cast_class = make_class("MoneyCast");
            cast_class
                .methods
                .push(Arc::new(make_method("get", Some("\\App\\Money"))));
            Some(Arc::new(cast_class))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\MoneyCast", &loader).to_string(),
        "\\App\\Money"
    );
}

#[test]
fn cast_custom_class_canonical_fqn() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\MoneyCast" {
            let mut cast_class = make_class("MoneyCast");
            cast_class
                .methods
                .push(Arc::new(make_method("get", Some("App\\Money"))));
            Some(Arc::new(cast_class))
        } else {
            None
        }
    };
    // Input is canonical (no leading `\`) — ingestion boundary in
    // ast_update.rs strips the prefix before values reach this function.
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\MoneyCast", &loader).to_string(),
        "App\\Money"
    );
}

#[test]
fn cast_custom_class_without_get_returns_mixed() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\WeirdCast" {
            Some(Arc::new(make_class("WeirdCast")))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\WeirdCast", &loader).to_string(),
        "mixed"
    );
}

// ── cast_type_to_php_type: enum casts ───────────────────────────────

#[test]
fn cast_enum_resolves_to_enum_class() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Enums\\Status" {
            let mut e = make_class("Status");
            e.kind = ClassLikeKind::Enum;
            Some(Arc::new(e))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Enums\\Status", &loader).to_string(),
        "App\\Enums\\Status"
    );
}

#[test]
fn cast_enum_canonical_fqn() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Enums\\Status" {
            let mut e = make_class("Status");
            e.kind = ClassLikeKind::Enum;
            Some(Arc::new(e))
        } else {
            None
        }
    };
    // Input is canonical (no leading `\`) — ingestion boundary in
    // ast_update.rs strips the prefix before values reach this function.
    assert_eq!(
        cast_type_to_php_type("App\\Enums\\Status", &loader).to_string(),
        "App\\Enums\\Status"
    );
}

// ── cast_type_to_php_type: Castable implementations ─────────────────

#[test]
fn cast_castable_resolves_to_class_itself() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\Address" {
            let mut c = make_class("Address");
            c.interfaces = vec![atom(CASTABLE_FQN)];
            Some(Arc::new(c))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\Address", &loader).to_string(),
        "App\\Casts\\Address"
    );
}

#[test]
fn cast_castable_with_fqn_interface() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\Address" {
            let mut c = make_class("Address");
            c.interfaces = vec![atom(CASTABLE_FQN)];
            Some(Arc::new(c))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\Address", &loader).to_string(),
        "App\\Casts\\Address"
    );
}

#[test]
fn cast_castable_short_interface_name() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\Address" {
            let mut c = make_class("Address");
            c.interfaces = vec![atom("Castable")];
            Some(Arc::new(c))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\Address", &loader).to_string(),
        "App\\Casts\\Address"
    );
}

// ── cast_type_to_php_type: colon argument suffix ────────────────────

#[test]
fn cast_class_with_colon_argument_suffix() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\Address" {
            let mut c = make_class("Address");
            c.interfaces = vec![atom(CASTABLE_FQN)];
            Some(Arc::new(c))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\Address:nullable", &loader).to_string(),
        "App\\Casts\\Address"
    );
}

#[test]
fn cast_enum_with_colon_argument_suffix() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Enums\\Status" {
            let mut e = make_class("Status");
            e.kind = ClassLikeKind::Enum;
            Some(Arc::new(e))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Enums\\Status:force", &loader).to_string(),
        "App\\Enums\\Status"
    );
}

#[test]
fn cast_custom_class_with_colon_argument_and_get() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\MoneyCast" {
            let mut cast_class = make_class("MoneyCast");
            cast_class
                .methods
                .push(Arc::new(make_method("get", Some("\\App\\Money"))));
            Some(Arc::new(cast_class))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\MoneyCast:precision,2", &loader).to_string(),
        "\\App\\Money"
    );
}

// ── is_castable ─────────────────────────────────────────────────────

#[test]
fn is_castable_with_fqn() {
    let mut c = make_class("Address");
    c.interfaces = vec![atom(CASTABLE_FQN)];
    assert!(is_castable(&c));
}

#[test]
fn is_castable_with_fqn_interface() {
    // Interfaces are canonical (no leading backslash) after resolution.
    let mut c = make_class("Address");
    c.interfaces = vec![atom(CASTABLE_FQN)];
    assert!(is_castable(&c));
}

#[test]
fn is_castable_with_short_name() {
    let mut c = make_class("Address");
    c.interfaces = vec![atom("Castable")];
    assert!(is_castable(&c));
}

#[test]
fn is_not_castable() {
    let c = make_class("SomePlainClass");
    assert!(!is_castable(&c));
}

// ── extract_tget_from_implements_generics ────────────────────────────

#[test]
fn tget_from_casts_attributes_short_name() {
    let mut c = make_class("App\\Casts\\HtmlCast");
    c.implements_generics = vec![(
        atom("CastsAttributes"),
        vec![PhpType::parse("HtmlString"), PhpType::parse("HtmlString")],
    )];
    assert_eq!(
        extract_tget_from_implements_generics(&c),
        Some(PhpType::Named("HtmlString".to_string()))
    );
}

#[test]
fn tget_from_casts_attributes_fqn() {
    let mut c = make_class("App\\Casts\\HtmlCast");
    c.implements_generics = vec![(
        atom(CASTS_ATTRIBUTES_FQN),
        vec![
            PhpType::parse("\\Illuminate\\Support\\HtmlString"),
            PhpType::parse("string"),
        ],
    )];
    assert_eq!(
        extract_tget_from_implements_generics(&c),
        Some(PhpType::Named(
            "\\Illuminate\\Support\\HtmlString".to_string()
        ))
    );
}

#[test]
fn tget_from_casts_attributes_fqn_canonical() {
    // implements_generics names are canonical (no leading backslash)
    // after resolution.
    let mut c = make_class("App\\Casts\\HtmlCast");
    c.implements_generics = vec![(
        atom(CASTS_ATTRIBUTES_FQN),
        vec![PhpType::parse("HtmlString"), PhpType::parse("HtmlString")],
    )];
    assert_eq!(
        extract_tget_from_implements_generics(&c),
        Some(PhpType::Named("HtmlString".to_string()))
    );
}

#[test]
fn tget_returns_none_when_no_implements_generics() {
    let c = make_class("App\\Casts\\HtmlCast");
    assert_eq!(extract_tget_from_implements_generics(&c), None);
}

#[test]
fn tget_returns_none_for_unrelated_interface() {
    let mut c = make_class("App\\Casts\\HtmlCast");
    c.implements_generics = vec![(atom("SomeOtherInterface"), vec![PhpType::parse("Foo")])];
    assert_eq!(extract_tget_from_implements_generics(&c), None);
}

#[test]
fn tget_returns_none_for_empty_args() {
    let mut c = make_class("App\\Casts\\HtmlCast");
    c.implements_generics = vec![(atom("CastsAttributes"), vec![])];
    assert_eq!(extract_tget_from_implements_generics(&c), None);
}

#[test]
fn tget_skips_empty_string_arg() {
    let mut c = make_class("App\\Casts\\HtmlCast");
    c.implements_generics = vec![(
        atom("CastsAttributes"),
        vec![PhpType::parse(""), PhpType::parse("HtmlString")],
    )];
    assert_eq!(extract_tget_from_implements_generics(&c), None);
}

// ── cast_type_to_php_type: @implements fallback ─────────────────────

#[test]
fn cast_custom_class_falls_back_to_implements_generics() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\HtmlCast" {
            let mut cast_class = make_class("HtmlCast");
            // get() has no return type — mimics the real scenario.
            cast_class.methods.push(Arc::new(make_method("get", None)));
            cast_class.implements_generics = vec![(
                atom("CastsAttributes"),
                vec![PhpType::parse("HtmlString"), PhpType::parse("HtmlString")],
            )];
            Some(Arc::new(cast_class))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\HtmlCast", &loader).to_string(),
        "HtmlString"
    );
}

#[test]
fn cast_implements_generics_take_priority_over_get_return_type() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\HtmlCast" {
            let mut cast_class = make_class("HtmlCast");
            cast_class
                .methods
                .push(Arc::new(make_method("get", Some("?HtmlString"))));
            cast_class.implements_generics = vec![(
                atom("CastsAttributes"),
                vec![
                    PhpType::parse("DifferentType"),
                    PhpType::parse("DifferentType"),
                ],
            )];
            Some(Arc::new(cast_class))
        } else {
            None
        }
    };
    // @implements CastsAttributes<DifferentType, DifferentType> is the
    // canonical type declaration and should win over get()'s return type.
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\HtmlCast", &loader).to_string(),
        "DifferentType"
    );
}

#[test]
fn cast_get_return_type_used_when_no_implements_generics() {
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "App\\Casts\\HtmlCast" {
            let mut cast_class = make_class("HtmlCast");
            cast_class
                .methods
                .push(Arc::new(make_method("get", Some("?HtmlString"))));
            // No @implements generics — get() is the only signal.
            Some(Arc::new(cast_class))
        } else {
            None
        }
    };
    assert_eq!(
        cast_type_to_php_type("App\\Casts\\HtmlCast", &loader).to_string(),
        "?HtmlString"
    );
}
