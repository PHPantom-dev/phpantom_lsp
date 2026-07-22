use super::*;

// ── PHPDOC_TYPE_KEYWORDS ────────────────────────────────────────

#[test]
fn phpdoc_type_keywords_has_no_duplicates() {
    let mut seen = std::collections::HashSet::new();
    for entry in PHPDOC_TYPE_KEYWORDS {
        assert!(
            seen.insert(entry),
            "PHPDOC_TYPE_KEYWORDS contains duplicate entry {:?}",
            entry
        );
    }
}
