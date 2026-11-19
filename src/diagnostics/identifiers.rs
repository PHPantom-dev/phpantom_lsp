//! Renamed diagnostic identifiers.
//!
//! An identifier is a public interface: users write it into
//! `[[diagnostics.ignore]]`, `@phpantom-ignore` comments, and
//! `analyze --code`.  When one is renamed, its old spelling stays here so
//! every place a user may have written it keeps matching.

/// Old identifier → the identifier that replaced it.  Only spellings a
/// release shipped belong here.
const RENAMED: &[(&str, &str)] = &[
    ("invalid_member_access", "inaccessible_member"),
    ("invalid_laravel_trans", "invalid_laravel_translation"),
    (
        "invalid_command_parameter",
        "invalid_laravel_command_parameter",
    ),
    ("missing_view_variable", "missing_blade_variable"),
    ("unused_view_variable", "unused_blade_variable"),
    (
        "type_mismatch_view_variable",
        "type_mismatch_blade_variable",
    ),
    (
        "blade_signature_widens_layout",
        "incompatible_blade_signature",
    ),
];

/// The identifier `code` was renamed to, when it is an old spelling.
pub(crate) fn renamed_to(code: &str) -> Option<&'static str> {
    RENAMED
        .iter()
        .find(|(old, _)| *old == code)
        .map(|(_, new)| *new)
}

/// The current spelling of `code`: the identifier that replaced it, or
/// `code` itself when it was never renamed.
pub(crate) fn current_identifier(code: &str) -> &str {
    renamed_to(code).unwrap_or(code)
}

/// Print a warning to stderr that `code`, written in `place`, is an old
/// spelling, and return its current one.
pub(crate) fn current_identifier_or_warn(code: &str, place: &str) -> String {
    match renamed_to(code) {
        Some(new) => {
            eprintln!("warning: {place} `{code}` has been renamed to `{new}`");
            new.to_string()
        }
        None => code.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_an_old_identifier_to_its_replacement() {
        assert_eq!(
            current_identifier("invalid_member_access"),
            "inaccessible_member"
        );
    }

    #[test]
    fn leaves_a_current_identifier_alone() {
        assert_eq!(current_identifier("unknown_class"), "unknown_class");
        assert_eq!(renamed_to("inaccessible_member"), None);
    }

    #[test]
    fn no_replacement_is_itself_renamed() {
        for (old, new) in RENAMED {
            assert!(renamed_to(new).is_none(), "{old} → {new} is renamed again");
        }
    }
}
