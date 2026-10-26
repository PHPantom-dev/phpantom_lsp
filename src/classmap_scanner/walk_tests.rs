use super::*;

// ── Interior symlink walking (issue #383) ─────────────────────────
//
// A symlinked directory *inside* a walk root is descended into, which is
// how a `kdhelp -> ../kdhelp` style link to a framework tree kept outside
// the repository gets indexed with the rest of the project.  Every path
// the walk yields keeps the symlink spelling — the contract the index and
// the URIs returned to the editor depend on — and each target is entered
// once however many links reach it.

#[test]
fn walk_roots_skips_a_link_pointing_at_a_skipped_tree() {
    // `orchestra/testbench-core` ships `laravel/vendor -> <project>/vendor`,
    // a link back at the vendor tree the walk is already covering through
    // `installed.json`.  Descending it would index every vendor package a
    // second time under a path inside testbench, and a third time under
    // that copy's own copy of the link.  A skipped tree has to be claimed
    // up front the same way a root is.
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("vendor");
    let pkg = vendor.join("acme/pkg");
    std::fs::create_dir_all(pkg.join("laravel")).unwrap();
    std::fs::create_dir_all(vendor.join("other")).unwrap();
    std::fs::write(vendor.join("other/Other.php"), "<?php\nclass Other {}").unwrap();
    std::fs::write(pkg.join("Pkg.php"), "<?php\nclass Pkg {}").unwrap();

    let link = pkg.join("laravel/vendor");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&vendor, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&vendor, &link).unwrap();

    let empty = HashSet::new();
    let opts = WalkOptions::new(vec![vendor.clone()], &empty, IndexFilters::empty(), None);
    let files: Vec<PathBuf> = walk_roots(std::slice::from_ref(&pkg), &opts)
        .into_iter()
        .flatten()
        .collect();

    assert!(
        files.iter().any(|p| p.ends_with("Pkg.php")),
        "the package's own files must still be found: {files:?}"
    );
    assert!(
        !files.iter().any(|p| p.starts_with(&link)),
        "a link back at the skipped vendor tree must not be walked: {files:?}"
    );
}

#[test]
fn walk_roots_follows_interior_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    let real = dir.path().join("real");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("Hidden.php"), "<?php\nclass Hidden {}").unwrap();

    let link = root.join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real, &link).unwrap();

    let empty = HashSet::new();
    let opts = WalkOptions::new(Vec::new(), &empty, IndexFilters::empty(), None);
    let files: Vec<PathBuf> = walk_roots(&[root], &opts).into_iter().flatten().collect();
    let linked = files
        .iter()
        .find(|p| p.ends_with("Hidden.php"))
        .unwrap_or_else(|| panic!("linked file must be indexed: {files:?}"));
    assert!(
        linked.starts_with(&link),
        "paths must keep the symlink spelling: {linked:?} vs {link:?}"
    );
}

#[test]
fn walk_roots_attributes_a_followed_link_to_the_root_that_reached_it() {
    // `walk_roots` puts every root in one `ignore` walk and attributes
    // each file to a root by its depth, so a root's own files are the
    // ones its own descent produced.  Following a symlink must not
    // disturb that: the walk goes deeper under the link spelling, which
    // is still below the root that owns it.  A second, unrelated root
    // alongside it is what a directory named directly (rather than
    // reached through a link) looks like to this walk, and the two must
    // not bleed into each other.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    let real = dir.path().join("real");
    let other = dir.path().join("other");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&real).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(real.join("Linked.php"), "<?php\nclass Linked {}").unwrap();
    std::fs::write(other.join("Named.php"), "<?php\nclass Named {}").unwrap();

    let link = root.join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real, &link).unwrap();

    let empty = HashSet::new();
    let opts = WalkOptions::new(Vec::new(), &empty, IndexFilters::empty(), None);
    let per_root = walk_roots(&[root.clone(), other.clone()], &opts);

    assert_eq!(
        per_root[0],
        vec![link.join("Linked.php")],
        "the workspace root owns the file its own followed link reached"
    );
    assert_eq!(
        per_root[1],
        vec![other.join("Named.php")],
        "a root named outright keeps its own files and none of the link's"
    );
}

#[test]
fn walk_roots_follows_nested_symlinks() {
    // The kdhelp/soa scenario: a symlink inside a symlinked target,
    // pointing at a second external tree, is followed transitively and
    // keeps the full symlink prefix in its yielded paths.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    let ext1 = dir.path().join("ext1");
    let ext2 = dir.path().join("ext2");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&ext1).unwrap();
    std::fs::create_dir_all(&ext2).unwrap();
    std::fs::write(ext2.join("Deep.php"), "<?php\nclass Deep {}").unwrap();

    let kdhelp = root.join("kdhelp");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&ext1, &kdhelp).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&ext1, &kdhelp).unwrap();

    // The second link lives *inside* the first link's target, so it is
    // only reachable when the walk follows into ext1.
    let soa = ext1.join("soa");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&ext2, &soa).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&ext2, &soa).unwrap();

    let empty = HashSet::new();
    let opts = WalkOptions::new(Vec::new(), &empty, IndexFilters::empty(), None);
    let files: Vec<PathBuf> = walk_roots(std::slice::from_ref(&root), &opts)
        .into_iter()
        .flatten()
        .collect();
    let linked = files
        .iter()
        .find(|p| p.ends_with("Deep.php"))
        .unwrap_or_else(|| panic!("nested linked file must be indexed: {files:?}"));
    // Both link spellings survive transitively: the yielded path is
    // ws/kdhelp/soa/Deep.php, never the real ext1/… / ext2/… targets.
    let expected_prefix = root.join("kdhelp").join("soa");
    assert!(
        linked.starts_with(&expected_prefix),
        "nested links must keep the full symlink prefix: {linked:?} vs {expected_prefix:?}"
    );
}

/// Create a directory symlink, spelled the way each platform needs.
#[cfg(any(unix, windows))]
fn link_dir(target: &std::path::Path, link: &std::path::Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

#[test]
fn walk_roots_walks_a_link_target_once_however_many_links_reach_it() {
    // `ignore` only refuses a link pointing at one of its own ancestors,
    // so two links to the same tree are not a cycle to it and it walks
    // that tree twice.  Both copies land in the index, and every class in
    // them resolves to whichever spelling happened to win.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    let ext = dir.path().join("ext");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&ext).unwrap();
    std::fs::write(ext.join("Dup.php"), "<?php\nclass Dup {}").unwrap();

    link_dir(&ext, &root.join("a"));
    link_dir(&ext, &root.join("b"));

    let empty = HashSet::new();
    let opts = WalkOptions::new(Vec::new(), &empty, IndexFilters::empty(), None);
    let files: Vec<PathBuf> = walk_roots(&[root], &opts).into_iter().flatten().collect();
    assert_eq!(
        files.len(),
        1,
        "the linked tree must be walked once, not once per link: {files:?}"
    );
}

#[test]
fn walk_roots_keeps_the_real_spelling_of_a_link_back_into_a_root() {
    // A link pointing back inside the workspace is not a cycle either,
    // and the directory it names is one the walk covers anyway.  The
    // roots are claimed before the walk starts, so the spelling the walk
    // already had wins and the link is not descended into.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("Inside.php"), "<?php\nclass Inside {}").unwrap();

    link_dir(&src, &root.join("link"));

    let empty = HashSet::new();
    let opts = WalkOptions::new(Vec::new(), &empty, IndexFilters::empty(), None);
    let files: Vec<PathBuf> = walk_roots(std::slice::from_ref(&root), &opts)
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(
        files,
        vec![src.join("Inside.php")],
        "a link back into the workspace must lose to the real path: {files:?}"
    );
}

#[test]
fn walk_roots_does_not_fan_out_through_a_diamond_of_links() {
    // Five directories holding two links apiece, each pair pointing at
    // the next directory.  No link points at an ancestor, so nothing here
    // is a cycle and `ignore` walks every one of the 2^5 routes to the
    // leaf.  Claiming each target the first time a link reaches it turns
    // the fan-out back into a single descent.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    std::fs::create_dir_all(&root).unwrap();
    let levels: Vec<PathBuf> = (0..6).map(|i| dir.path().join(format!("d{i}"))).collect();
    for level in &levels {
        std::fs::create_dir_all(level).unwrap();
    }
    std::fs::write(levels[5].join("Leaf.php"), "<?php\nclass Leaf {}").unwrap();

    for i in 0..5 {
        link_dir(&levels[i + 1], &levels[i].join("x"));
        link_dir(&levels[i + 1], &levels[i].join("y"));
    }
    link_dir(&levels[0], &root.join("entry"));

    let empty = HashSet::new();
    let opts = WalkOptions::new(Vec::new(), &empty, IndexFilters::empty(), None);
    let files: Vec<PathBuf> = walk_roots(&[root], &opts).into_iter().flatten().collect();
    assert_eq!(
        files.len(),
        1,
        "a diamond of links must not multiply the leaf: {files:?}"
    );
}

#[test]
fn walk_roots_follows_symlink_cycle_safely() {
    // A symlink pointing back at the workspace itself must terminate:
    // `ignore`'s parallel walker detects the cycle via dev+inode handles
    // and skips the re-entered directory.  Without the cycle guard this
    // test would walk forever.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("App.php"), "<?php\nclass App {}").unwrap();

    let link = root.join("loop");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&root, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&root, &link).unwrap();

    let empty = HashSet::new();
    let opts = WalkOptions::new(Vec::new(), &empty, IndexFilters::empty(), None);
    let files: Vec<PathBuf> = walk_roots(&[root], &opts).into_iter().flatten().collect();
    assert!(
        files.iter().any(|p| p.ends_with("App.php")),
        "workspace files must still be found next to a cycle: {files:?}"
    );
}

#[test]
fn walk_roots_prunes_a_link_into_a_skipped_tree_by_its_target() {
    // `skip_dirs` prunes by literal path, which only stops the walk
    // reaching a tree directly; a link into it arrives under the link's
    // spelling and slips past.  Claiming the skipped trees up front closes
    // that: a tree another pipeline already covers (a vendor directory
    // scanned through `installed.json`, a monorepo subproject) must not be
    // indexed a second time just because something links to it.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    let real_vendor = dir.path().join("real-vendor");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&real_vendor).unwrap();
    std::fs::write(real_vendor.join("Pkg.php"), "<?php\nclass Pkg {}").unwrap();
    std::fs::write(root.join("Own.php"), "<?php\nclass Own {}").unwrap();

    let link = root.join("vendor-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_vendor, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&real_vendor, &link).unwrap();

    let skip_dirs = vec![real_vendor];
    let empty = HashSet::new();
    let opts = WalkOptions::new(skip_dirs, &empty, IndexFilters::empty(), None);
    let files: Vec<PathBuf> = walk_roots(&[root], &opts).into_iter().flatten().collect();
    assert!(
        files.iter().any(|p| p.ends_with("Own.php")),
        "the walk's own files must still be found: {files:?}"
    );
    assert!(
        !files.iter().any(|p| p.ends_with("Pkg.php")),
        "a link into a skipped tree must not walk it: {files:?}"
    );
}
