use super::*;

#[test]
fn a_renamed_scope_keeps_its_prefix_or_has_no_use_name() {
    assert_eq!(
        MagicMemberKind::Scope.use_name("scopeRecentlyActive"),
        Some("recentlyActive".to_string())
    );
    assert_eq!(MagicMemberKind::Scope.use_name("recentlyActive"), None);
    assert_eq!(MagicMemberKind::Scope.use_name("scope"), None);
}

#[test]
fn accessor_names_map_to_their_snake_case_property() {
    assert_eq!(
        MagicMemberKind::LegacyAccessor.use_name("getDisplayNameAttribute"),
        Some("display_name".to_string())
    );
    assert_eq!(
        MagicMemberKind::LegacyMutator.use_name("setLogoAttribute"),
        Some("logo".to_string())
    );
    assert_eq!(
        MagicMemberKind::LegacyMutator.use_name("getLogoAttribute"),
        None
    );
    assert_eq!(
        MagicMemberKind::ModernAccessor.use_name("avatarUrl"),
        Some("avatar_url".to_string())
    );
}

#[test]
fn declaring_names_invert_the_use_name() {
    let names: Vec<String> = declaring_method_names("display_name").collect();
    assert_eq!(
        names,
        [
            "scopeDisplay_name",
            "getDisplayNameAttribute",
            "setDisplayNameAttribute",
            "displayName"
        ]
    );
    let names: Vec<String> = declaring_method_names("active").collect();
    assert_eq!(
        names,
        ["scopeActive", "getActiveAttribute", "setActiveAttribute"]
    );
}
