// ── find_import_candidates smoke test ───────────────────────────────

#[test]
fn find_candidates_from_fqn_uri_index() {
    let backend = crate::Backend::new_test();
    // Populate class index with a known class.
    {
        let mut cmap = backend.symbols.fqn_uri_index.write();
        cmap.insert(
            "App\\Models\\User".to_string(),
            "file:///fake/path/User.php".to_string(),
        );
        cmap.insert(
            "App\\Http\\Request".to_string(),
            "file:///fake/path/Request.php".to_string(),
        );
    }

    let table = std::collections::HashMap::new();
    let candidates = backend.find_import_candidates("User", &table);
    assert!(candidates.contains(&"App\\Models\\User".to_string()));
    assert!(!candidates.contains(&"App\\Http\\Request".to_string()));
}

#[test]
fn find_candidates_case_insensitive() {
    let backend = crate::Backend::new_test();
    {
        let mut cmap = backend.symbols.fqn_uri_index.write();
        cmap.insert(
            "Vendor\\Obscure\\ZYGOMORPHIC".to_string(),
            "file:///fake/path.php".to_string(),
        );
    }

    let table = std::collections::HashMap::new();
    let candidates = backend.find_import_candidates("Zygomorphic", &table);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0], "Vendor\\Obscure\\ZYGOMORPHIC");
}

#[test]
fn find_candidates_deduplicates() {
    let backend = crate::Backend::new_test();
    // Add the same FQN to fqn_uri_index — should only appear once.
    {
        let mut idx = backend.symbols.fqn_uri_index.write();
        idx.insert("App\\Foo".to_string(), "file:///foo.php".to_string());
    }

    let table = std::collections::HashMap::new();
    let candidates = backend.find_import_candidates("Foo", &table);
    let count = candidates.iter().filter(|c| *c == "App\\Foo").count();
    assert_eq!(count, 1, "should not have duplicates");
}
