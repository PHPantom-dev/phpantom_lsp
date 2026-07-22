use super::*;
use crate::test_fixtures::{make_class, make_method};
use crate::types::{PivotRelation, PropertySource};
use std::collections::HashMap;

/// A model class with `file_namespace` unset, so its `fqn()` is the name given.
fn model(fqn: &str) -> ClassInfo {
    let mut c = make_class(fqn);
    c.parent_class = Some(crate::atom::atom("Illuminate\\Database\\Eloquent\\Model"));
    c
}

fn push_method(class: &mut ClassInfo, name: &str, return_type: &str) {
    class
        .methods
        .push(Arc::new(make_method(name, Some(return_type))));
}

fn loader_from(classes: Vec<Arc<ClassInfo>>) -> impl Fn(&str) -> Option<Arc<ClassInfo>> {
    let map: HashMap<String, Arc<ClassInfo>> = classes
        .into_iter()
        .map(|c| (c.fqn().to_string(), c))
        .collect();
    move |name: &str| map.get(name.trim_start_matches('\\')).cloned()
}

fn permission() -> Arc<ClassInfo> {
    Arc::new(model("App\\Models\\Permission"))
}
fn permission_role() -> Arc<ClassInfo> {
    Arc::new(model("App\\Models\\PermissionRole"))
}

#[test]
fn build_maps_related_to_third_generic_pivot() {
    let mut user = model("App\\Models\\User");
    push_method(
        &mut user,
        "permissions",
        "BelongsToMany<\\App\\Models\\Permission, $this, \\App\\Models\\PermissionRole>",
    );

    let classes = vec![("file:///User.php".to_string(), Arc::new(user))];
    let loader = loader_from(vec![permission(), permission_role()]);
    let index = build_pivot_index(&classes, &loader);

    assert_eq!(
        index.get("App\\Models\\Permission"),
        Some(&PhpType::Named("App\\Models\\PermissionRole".to_string()))
    );
}

#[test]
fn build_falls_back_to_base_pivot_without_third_generic() {
    let mut user = model("App\\Models\\User");
    push_method(
        &mut user,
        "roles",
        "BelongsToMany<\\App\\Models\\Role, $this>",
    );

    let classes = vec![("file:///User.php".to_string(), Arc::new(user))];
    let loader = loader_from(vec![Arc::new(model("App\\Models\\Role"))]);
    let index = build_pivot_index(&classes, &loader);

    assert_eq!(
        index.get("App\\Models\\Role"),
        Some(&PhpType::Named(ELOQUENT_PIVOT_FQN.to_string()))
    );
}

#[test]
fn build_uses_parsed_using_when_no_third_generic() {
    let mut user = model("App\\Models\\User");
    push_method(
        &mut user,
        "roles",
        "BelongsToMany<\\App\\Models\\Role, $this>",
    );
    // The parser recovered `->using(RoleUser::class)` (FQN-resolved).
    user.laravel_mut().belongs_to_many_pivots = vec![PivotRelation {
        method: "roles".to_string(),
        using: Some("App\\Models\\RoleUser".to_string()),
        columns: Vec::new(),
    }];

    let classes = vec![("file:///User.php".to_string(), Arc::new(user))];
    let loader = loader_from(vec![Arc::new(model("App\\Models\\Role"))]);
    let index = build_pivot_index(&classes, &loader);

    assert_eq!(
        index.get("App\\Models\\Role"),
        Some(&PhpType::Named("App\\Models\\RoleUser".to_string()))
    );
}

#[test]
fn build_ambiguous_targets_fall_back_to_base() {
    let mut user = model("App\\Models\\User");
    push_method(
        &mut user,
        "permissions",
        "BelongsToMany<\\App\\Models\\Permission, $this, \\App\\Models\\PermissionRole>",
    );
    let mut team = model("App\\Models\\Team");
    push_method(
        &mut team,
        "permissions",
        "BelongsToMany<\\App\\Models\\Permission, $this>",
    );

    let classes = vec![
        ("file:///User.php".to_string(), Arc::new(user)),
        ("file:///Team.php".to_string(), Arc::new(team)),
    ];
    let loader = loader_from(vec![permission(), permission_role()]);
    let index = build_pivot_index(&classes, &loader);

    assert_eq!(
        index.get("App\\Models\\Permission"),
        Some(&PhpType::Named(ELOQUENT_PIVOT_FQN.to_string())),
        "conflicting pivots collapse to the base Pivot"
    );
}

#[test]
fn build_ignores_non_pivot_relationships() {
    let mut user = model("App\\Models\\User");
    push_method(&mut user, "posts", "HasMany<\\App\\Models\\Post, $this>");

    let classes = vec![("file:///User.php".to_string(), Arc::new(user))];
    let loader = loader_from(vec![Arc::new(model("App\\Models\\Post"))]);
    let index = build_pivot_index(&classes, &loader);

    assert!(index.is_empty(), "HasMany does not expose a pivot");
}

#[test]
fn inject_adds_typed_pivot_for_target() {
    let mut user = model("App\\Models\\User");
    push_method(
        &mut user,
        "permissions",
        "BelongsToMany<\\App\\Models\\Permission, $this, \\App\\Models\\PermissionRole>",
    );
    let classes = vec![("file:///User.php".to_string(), Arc::new(user))];
    let loader = loader_from(vec![permission(), permission_role()]);
    let index = build_pivot_index(&classes, &loader);

    let injected = inject_pivot(&index, permission());
    let pivot = injected
        .properties
        .iter()
        .find(|p| p.name == "pivot")
        .expect("Permission is a many-to-many target");
    assert_eq!(
        pivot.type_hint_str().as_deref(),
        Some("App\\Models\\PermissionRole")
    );
    assert!(matches!(pivot.source, Some(PropertySource::Pivot)));
}

#[test]
fn inject_skips_non_target() {
    let index = build_pivot_index(&[], &loader_from(Vec::new()));
    let user = Arc::new(model("App\\Models\\User"));
    let out = inject_pivot(&index, Arc::clone(&user));
    assert!(!out.properties.iter().any(|p| p.name == "pivot"));
}

#[test]
fn inject_respects_declared_pivot() {
    let mut user = model("App\\Models\\User");
    push_method(
        &mut user,
        "permissions",
        "BelongsToMany<\\App\\Models\\Permission, $this, \\App\\Models\\PermissionRole>",
    );
    let classes = vec![("file:///User.php".to_string(), Arc::new(user))];
    let loader = loader_from(vec![permission(), permission_role()]);
    let index = build_pivot_index(&classes, &loader);

    // Permission already declares its own `pivot` property.
    let mut permission_with_pivot = model("App\\Models\\Permission");
    permission_with_pivot
        .properties
        .push(PropertyInfo::virtual_property_typed(
            "pivot",
            Some(&PhpType::Named("App\\Models\\Custom".to_string())),
        ));
    let out = inject_pivot(&index, Arc::new(permission_with_pivot));
    let pivots: Vec<_> = out
        .properties
        .iter()
        .filter(|p| p.name == "pivot")
        .collect();
    assert_eq!(pivots.len(), 1, "declared pivot is not duplicated");
    assert_eq!(
        pivots[0].type_hint_str().as_deref(),
        Some("App\\Models\\Custom")
    );
}
