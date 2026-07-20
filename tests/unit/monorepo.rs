//! Integration tests for monorepo / multi-composer-root support.
//!
//! These tests verify that when the workspace root has no `composer.json`
//! but contains subdirectories with their own `composer.json` files,
//! PHPantom correctly discovers subprojects, merges classmaps, indexes
//! autoload files, and picks up loose PHP files outside subproject trees.

use std::collections::HashSet;
use std::path::PathBuf;

use phpantom_lsp::classmap_scanner::scan_workspace_fallback_full;
use phpantom_lsp::composer::{
    discover_subproject_roots, parse_autoload_classmap, parse_autoload_files, parse_composer_json,
};

// ═══════════════════════════════════════════════════════════════════════════
// Subproject discovery
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn discover_finds_single_subproject() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("project-a");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .unwrap();

    let roots = discover_subproject_roots(dir.path());
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].0, sub);
    assert_eq!(roots[0].1, "vendor");
}

#[test]
fn discover_finds_multiple_subprojects_at_different_depths() {
    let dir = tempfile::tempdir().unwrap();

    let sub_a = dir.path().join("project-a");
    std::fs::create_dir_all(&sub_a).unwrap();
    std::fs::write(sub_a.join("composer.json"), "{}").unwrap();

    let sub_b = dir.path().join("packages").join("project-b");
    std::fs::create_dir_all(&sub_b).unwrap();
    std::fs::write(sub_b.join("composer.json"), "{}").unwrap();

    let sub_c = dir.path().join("deep").join("nested").join("project-c");
    std::fs::create_dir_all(&sub_c).unwrap();
    std::fs::write(sub_c.join("composer.json"), "{}").unwrap();

    let roots = discover_subproject_roots(dir.path());
    let paths: HashSet<PathBuf> = roots.iter().map(|(p, _)| p.clone()).collect();
    assert_eq!(paths.len(), 3);
    assert!(paths.contains(&sub_a));
    assert!(paths.contains(&sub_b));
    assert!(paths.contains(&sub_c));
}

#[test]
fn discover_skips_nested_composer_json_inside_subproject() {
    let dir = tempfile::tempdir().unwrap();

    // project-a has its own composer.json
    let sub_a = dir.path().join("project-a");
    std::fs::create_dir_all(&sub_a).unwrap();
    std::fs::write(sub_a.join("composer.json"), "{}").unwrap();

    // project-a/vendor/some-pkg also has composer.json — should be skipped
    // because it is inside a gitignored vendor dir.
    // Since we can't easily set up a .gitignore-aware walk in tests
    // (no git repo), test the filter-out-nested logic instead:
    // even if both are found, the deeper one should be filtered out
    // because it is inside an already-accepted subproject root.
    let nested = sub_a.join("subdir");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("composer.json"), "{}").unwrap();

    let roots = discover_subproject_roots(dir.path());
    // Should only find project-a, not project-a/subdir
    assert_eq!(
        roots.len(),
        1,
        "nested composer.json should be filtered out: {:?}",
        roots
    );
    assert_eq!(roots[0].0, sub_a);
}

#[test]
fn discover_skips_workspace_root_composer_json() {
    let dir = tempfile::tempdir().unwrap();
    // Root itself has composer.json — should be skipped
    std::fs::write(dir.path().join("composer.json"), "{}").unwrap();

    let sub = dir.path().join("subproject");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("composer.json"), "{}").unwrap();

    let roots = discover_subproject_roots(dir.path());
    let paths: Vec<PathBuf> = roots.iter().map(|(p, _)| p.clone()).collect();
    assert!(
        !paths.contains(&dir.path().to_path_buf()),
        "workspace root should not be in subproject list"
    );
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].0, sub);
}

#[test]
fn discover_reads_custom_vendor_dir() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("project");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join("composer.json"),
        r#"{"config":{"vendor-dir":"deps"}}"#,
    )
    .unwrap();

    let roots = discover_subproject_roots(dir.path());
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].1, "deps");
}

#[test]
fn discover_returns_empty_when_no_subprojects() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.php"), "<?php\necho 'hello';").unwrap();

    let roots = discover_subproject_roots(dir.path());
    assert!(roots.is_empty());
}

#[test]
fn discover_skips_hidden_directories() {
    let dir = tempfile::tempdir().unwrap();

    let hidden = dir.path().join(".hidden-project");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("composer.json"), "{}").unwrap();

    let roots = discover_subproject_roots(dir.path());
    assert!(
        roots.is_empty(),
        "hidden directories should be skipped: {:?}",
        roots
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Classmap merging from multiple subprojects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn classmaps_merge_from_two_subprojects() {
    let dir = tempfile::tempdir().unwrap();

    // Subproject A with a classmap
    let sub_a = dir.path().join("project-a");
    let vendor_a = sub_a.join("vendor").join("composer");
    std::fs::create_dir_all(&vendor_a).unwrap();
    std::fs::write(
        sub_a.join("composer.json"),
        r#"{"autoload":{"psr-4":{"A\\":"src/"}}}"#,
    )
    .unwrap();

    let src_a = sub_a.join("src");
    std::fs::create_dir_all(&src_a).unwrap();
    std::fs::write(src_a.join("Foo.php"), "<?php\nnamespace A;\nclass Foo {}").unwrap();

    // Classmap for project A
    std::fs::write(
        vendor_a.join("autoload_classmap.php"),
        "<?php\nreturn array(\n    'A\\\\Foo' => $baseDir . '/src/Foo.php',\n);",
    )
    .unwrap();

    // Subproject B with a classmap
    let sub_b = dir.path().join("project-b");
    let vendor_b = sub_b.join("vendor").join("composer");
    std::fs::create_dir_all(&vendor_b).unwrap();
    std::fs::write(
        sub_b.join("composer.json"),
        r#"{"autoload":{"psr-4":{"B\\":"src/"}}}"#,
    )
    .unwrap();

    let src_b = sub_b.join("src");
    std::fs::create_dir_all(&src_b).unwrap();
    std::fs::write(src_b.join("Bar.php"), "<?php\nnamespace B;\nclass Bar {}").unwrap();

    std::fs::write(
        vendor_b.join("autoload_classmap.php"),
        "<?php\nreturn array(\n    'B\\\\Bar' => $baseDir . '/src/Bar.php',\n);",
    )
    .unwrap();

    // Parse classmaps from both subprojects
    let cm_a = parse_autoload_classmap(&sub_a, "vendor");
    let cm_b = parse_autoload_classmap(&sub_b, "vendor");

    // Merge (simulating what init_monorepo does)
    let mut merged = cm_a;
    for (fqcn, path) in cm_b {
        merged.entry(fqcn).or_insert(path);
    }

    assert!(
        merged.contains_key("A\\Foo"),
        "should have class from subproject A: {:?}",
        merged.keys().collect::<Vec<_>>()
    );
    assert!(
        merged.contains_key("B\\Bar"),
        "should have class from subproject B: {:?}",
        merged.keys().collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// PSR-4 resolution across subprojects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn psr4_mappings_resolve_across_subprojects() {
    let dir = tempfile::tempdir().unwrap();

    let sub_a = dir.path().join("project-a");
    let src_a = sub_a.join("src");
    std::fs::create_dir_all(&src_a).unwrap();
    std::fs::write(
        sub_a.join("composer.json"),
        r#"{"autoload":{"psr-4":{"Alpha\\":"src/"}}}"#,
    )
    .unwrap();
    std::fs::write(
        src_a.join("Widget.php"),
        "<?php\nnamespace Alpha;\nclass Widget {}",
    )
    .unwrap();

    let sub_b = dir.path().join("project-b");
    let src_b = sub_b.join("lib");
    std::fs::create_dir_all(&src_b).unwrap();
    std::fs::write(
        sub_b.join("composer.json"),
        r#"{"autoload":{"psr-4":{"Beta\\":"lib/"}}}"#,
    )
    .unwrap();
    std::fs::write(
        src_b.join("Gadget.php"),
        "<?php\nnamespace Beta;\nclass Gadget {}",
    )
    .unwrap();

    // Parse PSR-4 from both subprojects
    let (mappings_a, _) = parse_composer_json(&sub_a);
    let (mappings_b, _) = parse_composer_json(&sub_b);

    // Convert to absolute paths (simulating what init_monorepo does)
    let mut all_mappings = Vec::new();
    for m in &mappings_a {
        let abs_base = sub_a.join(&m.base_path).to_string_lossy().to_string();
        all_mappings.push(phpantom_lsp::composer::Psr4Mapping {
            prefix: m.prefix.clone(),
            base_path: phpantom_lsp::composer::normalise_path(&abs_base),
        });
    }
    for m in &mappings_b {
        let abs_base = sub_b.join(&m.base_path).to_string_lossy().to_string();
        all_mappings.push(phpantom_lsp::composer::Psr4Mapping {
            prefix: m.prefix.clone(),
            base_path: phpantom_lsp::composer::normalise_path(&abs_base),
        });
    }

    // Sort by prefix length descending
    all_mappings.sort_by_key(|x| std::cmp::Reverse(x.prefix.len()));

    // Resolve using a dummy workspace root — since base_paths are absolute,
    // we use an empty path as the root.
    let empty_root = std::path::Path::new("");
    let result_a =
        phpantom_lsp::composer::resolve_class_path(&all_mappings, empty_root, "Alpha\\Widget");
    assert!(
        result_a.is_some(),
        "should resolve Alpha\\Widget from subproject A"
    );

    let result_b =
        phpantom_lsp::composer::resolve_class_path(&all_mappings, empty_root, "Beta\\Gadget");
    assert!(
        result_b.is_some(),
        "should resolve Beta\\Gadget from subproject B"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Autoload file indexing from subprojects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn autoload_files_indexed_from_subproject() {
    let dir = tempfile::tempdir().unwrap();

    let sub = dir.path().join("project");
    let vendor = sub.join("vendor");
    let composer_dir = vendor.join("composer");
    std::fs::create_dir_all(&composer_dir).unwrap();
    std::fs::write(sub.join("composer.json"), "{}").unwrap();

    // Create a helpers file
    let helpers = vendor.join("some-pkg").join("src");
    std::fs::create_dir_all(&helpers).unwrap();
    std::fs::write(
        helpers.join("helpers.php"),
        "<?php\nfunction my_helper(): string { return ''; }",
    )
    .unwrap();

    // Create autoload_files.php pointing to it
    let helpers_path = helpers.join("helpers.php");
    let helpers_rel = helpers_path
        .strip_prefix(&sub)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    std::fs::write(
        composer_dir.join("autoload_files.php"),
        format!("<?php\nreturn array(\n    'abc123' => $baseDir . '/{helpers_rel}',\n);",),
    )
    .unwrap();

    let files = parse_autoload_files(&sub, "vendor");
    assert!(
        !files.is_empty(),
        "should find autoload files in subproject"
    );
    assert!(
        files.iter().any(|p| p.ends_with("helpers.php")),
        "should include helpers.php: {:?}",
        files
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Loose file discovery (full-scan with skip set)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn loose_files_discovered_outside_subprojects() {
    let dir = tempfile::tempdir().unwrap();

    // Subproject directory (should be skipped)
    let sub = dir.path().join("project-a");
    let sub_src = sub.join("src");
    std::fs::create_dir_all(&sub_src).unwrap();
    std::fs::write(
        sub_src.join("Internal.php"),
        "<?php\nnamespace A;\nclass Internal {}",
    )
    .unwrap();

    // Loose file outside any subproject
    std::fs::write(
        dir.path().join("bootstrap.php"),
        "<?php\nfunction bootstrap(): void {}\ndefine('APP_ROOT', __DIR__);\nclass Config {}",
    )
    .unwrap();

    // Another loose file in a subdirectory
    let scripts = dir.path().join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(
        scripts.join("migrate.php"),
        "<?php\nfunction run_migrations(): void {}",
    )
    .unwrap();

    let mut skip_dirs = HashSet::new();
    skip_dirs.insert(sub.clone());

    let result = scan_workspace_fallback_full(dir.path(), &skip_dirs, None);

    // Should find loose files
    assert!(
        result.classmap.contains_key("Config"),
        "should find loose class: {:?}",
        result.classmap.keys().collect::<Vec<_>>()
    );
    assert!(
        result.function_index.contains_key("bootstrap"),
        "should find loose function: {:?}",
        result.function_index.keys().collect::<Vec<_>>()
    );
    assert!(
        result.constant_index.contains_key("APP_ROOT"),
        "should find loose constant: {:?}",
        result.constant_index.keys().collect::<Vec<_>>()
    );
    assert!(
        result.function_index.contains_key("run_migrations"),
        "should find function in loose subdirectory: {:?}",
        result.function_index.keys().collect::<Vec<_>>()
    );

    // Should NOT find files inside skipped subproject directories
    assert!(
        !result.classmap.contains_key("A\\Internal"),
        "should skip classes inside subproject directories"
    );
}

#[test]
fn no_double_scanning_of_subproject_files() {
    let dir = tempfile::tempdir().unwrap();

    // A subproject with some PHP files
    let sub = dir.path().join("my-project");
    let sub_src = sub.join("src");
    std::fs::create_dir_all(&sub_src).unwrap();
    std::fs::write(
        sub_src.join("Service.php"),
        "<?php\nnamespace MyProject;\nclass Service {}",
    )
    .unwrap();

    // Skip set includes the subproject root
    let mut skip_dirs = HashSet::new();
    skip_dirs.insert(sub.clone());

    let result = scan_workspace_fallback_full(dir.path(), &skip_dirs, None);

    // The subproject files should NOT be in the scan result
    // (they would be handled by the Composer pipeline instead)
    assert!(
        !result.classmap.contains_key("MyProject\\Service"),
        "subproject files should not be scanned by full-scan walker"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Single-project compatibility
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn single_project_still_works_with_root_composer_json() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();

    std::fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .unwrap();
    std::fs::write(
        src.join("Model.php"),
        "<?php\nnamespace App;\nclass Model {}",
    )
    .unwrap();

    // When root composer.json exists, discover_subproject_roots should
    // still find any nested subprojects, but the caller (server.rs)
    // would take the single-project path instead.  Here we just verify
    // the parse_composer_json function still works correctly.
    let (mappings, vendor_dir) = parse_composer_json(dir.path());
    assert!(!mappings.is_empty(), "should find PSR-4 mappings");
    assert_eq!(vendor_dir, "vendor");
    assert!(mappings.iter().any(|m| m.prefix == "App\\"));
}

#[test]
fn full_scan_with_empty_skip_set_finds_everything() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.php"),
        "<?php\nfunction app_func(): void {}\nclass AppClass {}",
    )
    .unwrap();

    let sub = dir.path().join("lib");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("util.php"), "<?php\nfunction lib_func(): void {}").unwrap();

    let skip = HashSet::new();
    let result = scan_workspace_fallback_full(dir.path(), &skip, None);

    assert!(result.classmap.contains_key("AppClass"));
    assert!(result.function_index.contains_key("app_func"));
    assert!(result.function_index.contains_key("lib_func"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Subproject without vendor (composer install not run)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn subproject_without_vendor_produces_no_errors() {
    let dir = tempfile::tempdir().unwrap();

    let sub = dir.path().join("project");
    let src = sub.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        sub.join("composer.json"),
        r#"{"autoload":{"psr-4":{"My\\":"src/"}}}"#,
    )
    .unwrap();
    std::fs::write(
        src.join("Thing.php"),
        "<?php\nnamespace My;\nclass Thing {}",
    )
    .unwrap();
    // No vendor/ directory — composer install was not run

    // Discovery should find the subproject
    let roots = discover_subproject_roots(dir.path());
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].0, sub);

    // Classmap and autoload files should be empty (no vendor dir)
    let cm = parse_autoload_classmap(&sub, "vendor");
    assert!(cm.is_empty(), "no classmap without vendor dir");

    let files = parse_autoload_files(&sub, "vendor");
    assert!(files.is_empty(), "no autoload files without vendor dir");

    // PSR-4 mappings should still be parsed from composer.json
    let (mappings, _) = parse_composer_json(&sub);
    assert!(
        !mappings.is_empty(),
        "PSR-4 mappings should exist even without vendor"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Conflicting class names across subprojects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn first_subproject_wins_for_duplicate_fqns() {
    let dir = tempfile::tempdir().unwrap();

    // Two subprojects both define the same FQN via classmap
    let sub_a = dir.path().join("alpha");
    let vendor_a = sub_a.join("vendor").join("composer");
    std::fs::create_dir_all(&vendor_a).unwrap();
    std::fs::write(sub_a.join("composer.json"), "{}").unwrap();

    let src_a = sub_a.join("src");
    std::fs::create_dir_all(&src_a).unwrap();
    std::fs::write(
        src_a.join("Logger.php"),
        "<?php\nnamespace Shared;\nclass Logger { /* alpha */ }",
    )
    .unwrap();
    std::fs::write(
        vendor_a.join("autoload_classmap.php"),
        "<?php\nreturn array(\n    'Shared\\\\Logger' => $baseDir . '/src/Logger.php',\n);",
    )
    .unwrap();

    let sub_b = dir.path().join("beta");
    let vendor_b = sub_b.join("vendor").join("composer");
    std::fs::create_dir_all(&vendor_b).unwrap();
    std::fs::write(sub_b.join("composer.json"), "{}").unwrap();

    let src_b = sub_b.join("src");
    std::fs::create_dir_all(&src_b).unwrap();
    std::fs::write(
        src_b.join("Logger.php"),
        "<?php\nnamespace Shared;\nclass Logger { /* beta */ }",
    )
    .unwrap();
    std::fs::write(
        vendor_b.join("autoload_classmap.php"),
        "<?php\nreturn array(\n    'Shared\\\\Logger' => $baseDir . '/src/Logger.php',\n);",
    )
    .unwrap();

    let cm_a = parse_autoload_classmap(&sub_a, "vendor");
    let cm_b = parse_autoload_classmap(&sub_b, "vendor");

    // Merge with first-wins semantics
    let mut merged = cm_a;
    for (fqcn, path) in cm_b {
        merged.entry(fqcn).or_insert(path);
    }

    assert!(merged.contains_key("Shared\\Logger"));
    // The path should come from sub_a (first wins)
    let resolved_path = &merged["Shared\\Logger"];
    assert!(
        resolved_path.starts_with(&sub_a),
        "first subproject should win for duplicate FQN; got {:?}",
        resolved_path
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Full-scan respects gitignore (hidden dirs)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn full_scan_skips_hidden_directories() {
    let dir = tempfile::tempdir().unwrap();

    let hidden = dir.path().join(".cache");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(
        hidden.join("cached.php"),
        "<?php\nfunction cached_func(): void {}",
    )
    .unwrap();

    std::fs::write(
        dir.path().join("app.php"),
        "<?php\nfunction app_func(): void {}",
    )
    .unwrap();

    let skip = HashSet::new();
    let result = scan_workspace_fallback_full(dir.path(), &skip, None);

    assert!(result.function_index.contains_key("app_func"));
    assert!(
        !result.function_index.contains_key("cached_func"),
        "hidden directory functions should be excluded"
    );
}
