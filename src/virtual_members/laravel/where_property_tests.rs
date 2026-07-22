use crate::atom::atom;
use std::collections::HashSet;

use crate::php_type::PhpType;
use crate::types::{ClassInfo, MethodInfo, PropertyInfo, Visibility};
use crate::virtual_members::laravel::where_property::{
    build_where_property_methods_for_class, lowercase_method_names,
};

fn make_class(name: &str) -> ClassInfo {
    ClassInfo {
        name: crate::atom::atom(name),
        ..Default::default()
    }
}

fn make_model(name: &str) -> ClassInfo {
    let mut c = make_class(name);
    c.parent_class = Some(atom("Illuminate\\Database\\Eloquent\\Model"));
    c
}

#[test]
fn synthesizes_where_methods_from_cast_properties() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().casts_definitions = vec![
        ("brand_id".to_string(), "int".to_string()),
        ("is_active".to_string(), "boolean".to_string()),
    ];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        names.contains(&"whereBrandId"),
        "Expected whereBrandId, got: {names:?}"
    );
    assert!(
        names.contains(&"whereIsActive"),
        "Expected whereIsActive, got: {names:?}"
    );
}

#[test]
fn synthesizes_where_methods_from_column_names() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().column_names = vec!["lang_code".to_string(), "subcategory_id".to_string()];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        names.contains(&"whereLangCode"),
        "Expected whereLangCode, got: {names:?}"
    );
    assert!(
        names.contains(&"whereSubcategoryId"),
        "Expected whereSubcategoryId, got: {names:?}"
    );
}

#[test]
fn where_methods_return_builder_with_model_generic() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().casts_definitions = vec![("name".to_string(), "string".to_string())];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let method = methods.iter().find(|m| m.name == "whereName").unwrap();
    let ret = method.return_type.as_ref().unwrap().to_string();
    assert_eq!(
        ret,
        "Illuminate\\Database\\Eloquent\\Builder<App\\Models\\User>"
    );
}

#[test]
fn where_methods_return_builder_with_fqn_when_name_is_short() {
    let mut channel = make_model("Channel");
    channel.file_namespace = Some(atom("App\\Models"));
    channel.laravel_mut().casts_definitions = vec![("name".to_string(), "string".to_string())];

    let methods = build_where_property_methods_for_class(&channel, &HashSet::new());

    let method = methods.iter().find(|m| m.name == "whereName").unwrap();
    let ret = method.return_type.as_ref().unwrap().to_string();
    assert_eq!(
        ret,
        "Illuminate\\Database\\Eloquent\\Builder<App\\Models\\Channel>"
    );
}

#[test]
fn where_methods_have_value_parameter() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().casts_definitions = vec![("email".to_string(), "string".to_string())];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let method = methods.iter().find(|m| m.name == "whereEmail").unwrap();
    assert_eq!(method.parameters.len(), 1);
    assert_eq!(method.parameters[0].name, "$value");
    assert_eq!(
        method.parameters[0].type_hint.as_ref().unwrap().to_string(),
        "mixed"
    );
}

#[test]
fn where_methods_are_instance_methods() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().casts_definitions = vec![("name".to_string(), "string".to_string())];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    for m in &methods {
        assert!(!m.is_static, "where methods should be instance methods");
    }
}

#[test]
fn skips_existing_methods_with_same_name() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().casts_definitions = vec![
        ("name".to_string(), "string".to_string()),
        ("email".to_string(), "string".to_string()),
    ];

    let existing = vec![MethodInfo::virtual_method("whereName", Some("self"))];
    let existing_names = lowercase_method_names(&existing);

    let methods = build_where_property_methods_for_class(&user, &existing_names);

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        !names.contains(&"whereName"),
        "Should skip whereName because it already exists"
    );
    assert!(
        names.contains(&"whereEmail"),
        "Should still include whereEmail"
    );
}

#[test]
fn returns_empty_when_no_columns() {
    let user = make_model("App\\Models\\User");
    let methods = build_where_property_methods_for_class(&user, &HashSet::new());
    assert!(methods.is_empty());
}

#[test]
fn where_methods_are_virtual() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().casts_definitions = vec![("status".to_string(), "string".to_string())];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    for m in &methods {
        assert!(m.is_virtual, "where methods should be virtual");
    }
}

#[test]
fn no_duplicate_methods_for_overlapping_property_sources() {
    let mut user = make_model("App\\Models\\User");
    // Same column in both casts and column_names — only one where method.
    user.laravel_mut().casts_definitions = vec![("email".to_string(), "string".to_string())];
    user.laravel_mut().column_names = vec!["email".to_string()];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let count = methods.iter().filter(|m| m.name == "whereEmail").count();
    assert_eq!(count, 1, "Should have exactly one whereEmail method");
}

#[test]
fn synthesizes_from_dates_definitions() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().dates_definitions = vec!["deleted_at".to_string()];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        names.contains(&"whereDeletedAt"),
        "Expected whereDeletedAt from $dates, got: {names:?}"
    );
}

#[test]
fn synthesizes_from_attributes_definitions() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().attributes_definitions =
        vec![("role".to_string(), PhpType::Named("string".to_string()))];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        names.contains(&"whereRole"),
        "Expected whereRole from $attributes, got: {names:?}"
    );
}

#[test]
fn synthesizes_from_timestamp_columns() {
    let mut user = make_model("App\\Models\\User");
    // timestamps default to enabled when not set
    user.laravel_mut().timestamps = None;

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        names.contains(&"whereCreatedAt"),
        "Expected whereCreatedAt from timestamps, got: {names:?}"
    );
    assert!(
        names.contains(&"whereUpdatedAt"),
        "Expected whereUpdatedAt from timestamps, got: {names:?}"
    );
}

#[test]
fn no_timestamps_when_disabled() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().timestamps = Some(false);

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        !names.contains(&"whereCreatedAt"),
        "Should not have whereCreatedAt when timestamps disabled"
    );
    assert!(
        !names.contains(&"whereUpdatedAt"),
        "Should not have whereUpdatedAt when timestamps disabled"
    );
}

#[test]
fn custom_timestamp_column_names() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().timestamps = Some(true);
    user.laravel_mut().created_at_name = Some(Some("added_on".to_string()));
    user.laravel_mut().updated_at_name = Some(None); // disabled

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        names.contains(&"whereAddedOn"),
        "Expected whereAddedOn from custom CREATED_AT, got: {names:?}"
    );
    assert!(
        !names.contains(&"whereCreatedAt"),
        "Should not have default whereCreatedAt when overridden"
    );
    assert!(
        !names.contains(&"whereUpdatedAt"),
        "Should not have whereUpdatedAt when UPDATED_AT is null"
    );
}

#[test]
fn synthesizes_from_declared_properties() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().timestamps = Some(false);
    user.properties.push(PropertyInfo::virtual_property(
        "display_name",
        Some("string"),
    ));

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        names.contains(&"whereDisplayName"),
        "Expected whereDisplayName from @property, got: {names:?}"
    );
}

#[test]
fn where_methods_have_description() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().casts_definitions = vec![("brand_id".to_string(), "int".to_string())];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let method = methods.iter().find(|m| m.name == "whereBrandId").unwrap();
    let desc = method.description.as_ref().unwrap();
    assert!(
        desc.contains("brand_id"),
        "Description should mention the column name, got: {desc}"
    );
}

#[test]
fn where_methods_are_public() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().casts_definitions = vec![("name".to_string(), "string".to_string())];

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    for m in &methods {
        assert_eq!(
            m.visibility,
            Visibility::Public,
            "where methods should be public"
        );
    }
}

#[test]
fn case_insensitive_existing_method_skip() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().casts_definitions = vec![("email".to_string(), "string".to_string())];

    // Simulate an existing method with different casing.
    let existing = vec![MethodInfo::virtual_method("whereEmail", Some("self"))];
    let existing_names = lowercase_method_names(&existing);

    let methods = build_where_property_methods_for_class(&user, &existing_names);

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        !names.contains(&"whereEmail"),
        "Should skip whereEmail due to case-insensitive match"
    );
}

#[test]
fn lowercase_method_names_helper() {
    let methods = vec![
        MethodInfo::virtual_method("whereName", Some("self")),
        MethodInfo::virtual_method("OrderBy", Some("self")),
    ];

    let names = lowercase_method_names(&methods);

    assert!(names.contains("wherename"));
    assert!(names.contains("orderby"));
    assert_eq!(names.len(), 2);
}

#[test]
fn synthesizes_from_docblock_property_tags() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().timestamps = Some(false);
    // Simulate a model with @property tags in the class docblock but
    // nothing in the properties vec (as happens before full resolution).
    user.class_docblock =
        Some("/**\n * @property int $brand_id\n * @property string $email\n */".to_string());

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        names.contains(&"whereBrandId"),
        "Expected whereBrandId from @property docblock tag, got: {names:?}"
    );
    assert!(
        names.contains(&"whereEmail"),
        "Expected whereEmail from @property docblock tag, got: {names:?}"
    );
}

#[test]
fn docblock_property_deduplicates_with_existing_properties() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().timestamps = Some(false);
    // Same property in both properties vec and docblock — only one where method.
    user.properties
        .push(PropertyInfo::virtual_property("brand_id", Some("int")));
    user.class_docblock = Some("/** @property int $brand_id */".to_string());

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let count = methods.iter().filter(|m| m.name == "whereBrandId").count();
    assert_eq!(count, 1, "Should have exactly one whereBrandId method");
}

#[test]
fn all_sources_combined() {
    let mut user = make_model("App\\Models\\User");
    user.laravel_mut().casts_definitions = vec![("is_admin".to_string(), "boolean".to_string())];
    user.laravel_mut().dates_definitions = vec!["verified_at".to_string()];
    user.laravel_mut().attributes_definitions =
        vec![("role".to_string(), PhpType::Named("string".to_string()))];
    user.laravel_mut().column_names = vec!["nickname".to_string()];
    user.laravel_mut().timestamps = Some(true);
    user.properties
        .push(PropertyInfo::virtual_property("avatar_url", Some("string")));

    let methods = build_where_property_methods_for_class(&user, &HashSet::new());

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"whereIsAdmin"), "from $casts: {names:?}");
    assert!(names.contains(&"whereVerifiedAt"), "from $dates: {names:?}");
    assert!(names.contains(&"whereRole"), "from $attributes: {names:?}");
    assert!(
        names.contains(&"whereNickname"),
        "from column_names: {names:?}"
    );
    assert!(
        names.contains(&"whereCreatedAt"),
        "from timestamps: {names:?}"
    );
    assert!(
        names.contains(&"whereUpdatedAt"),
        "from timestamps: {names:?}"
    );
    assert!(
        names.contains(&"whereAvatarUrl"),
        "from @property: {names:?}"
    );
}
