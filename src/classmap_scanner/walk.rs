//! `.gitignore`-aware walks of the workspace.
//!
//! [`walk_roots`] is the parallel multi-root walk class discovery lists its
//! files with.  The serial walks serve the passes that read project files
//! by path rather than through the symbol index: the full workspace index,
//! namespace rename, go-to-implementation, the `analyze` file list, and the
//! Laravel enumerations that find `config/` and `lang/` files on disk.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{FollowedLinks, IndexFilters, LinkClaims, workspace_walk_builder};

/// Recursively collect all `.php` files under a workspace root,
/// respecting `.gitignore` rules (including nested and global
/// gitignore files).
///
/// Like every walk here, this goes through the `ignore` crate's
/// [`ignore::WalkBuilder`], so generated/cached directories listed in
/// `.gitignore` (e.g. `storage/framework/views/`, `var/cache/`,
/// `node_modules/`) are automatically skipped.
///
/// All known vendor directories are always skipped regardless of
/// `.gitignore` content, since some projects commit their vendor
/// directory.  `vendor_dir_paths` contains absolute paths of all known
/// vendor directories (one per subproject in monorepo mode).
///
/// Hidden files and directories are skipped by default (handled by the
/// `ignore` crate).
pub(crate) fn collect_php_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
    filters: &Arc<IndexFilters>,
    followed: Option<&FollowedLinks>,
) -> Vec<PathBuf> {
    let mut result = Vec::new();
    visit_workspace_files_gitignore(root, vendor_dir_paths, filters, followed, |path| {
        if filters.is_php_file(path) {
            result.push(path.to_path_buf());
        }
    });
    result
}

/// Collect the PHP and schema-free YAML/XML inputs used by the full workspace
/// index in one `.gitignore`-aware walk, with every file that can be a Twig
/// template.
pub(crate) fn collect_workspace_index_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
    filters: &Arc<IndexFilters>,
    followed: Option<&FollowedLinks>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut php_files = Vec::new();
    let mut resource_files = Vec::new();
    visit_workspace_files_gitignore(root, vendor_dir_paths, filters, followed, |path| {
        if filters.is_php_file(path) {
            php_files.push(path.to_path_buf());
        } else if crate::resource_navigation::is_resource_path(path)
            || crate::twig::roots::is_candidate_path(path)
        {
            resource_files.push(path.to_path_buf());
        }
    });
    (php_files, resource_files)
}

/// Pass every file under `root` that the workspace walk reaches to
/// `visit`: `.gitignore`-aware, skipping hidden entries, `[indexing]
/// exclude` and the `vendor_dir_paths` trees, and following links through
/// `followed`.
pub(crate) fn visit_workspace_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
    filters: &Arc<IndexFilters>,
    followed: Option<&FollowedLinks>,
    mut visit: impl FnMut(&Path),
) {
    let walker = workspace_walk_builder(
        root,
        Arc::new(vendor_dir_paths.to_vec()),
        Arc::clone(filters),
        false,
        LinkClaims::new([root.to_path_buf()], followed),
    )
    .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() {
            visit(path);
        }
    }
}

/// What a [`walk_roots`] call leaves out.
pub(super) struct WalkOptions<'a> {
    /// Directories that must never be entered: vendor trees scanned
    /// through `installed.json` instead, and monorepo subproject roots
    /// another pipeline already covered.  Shared behind an `Arc` because
    /// `ignore` requires the `filter_entry` closure to own its captures.
    skip_dirs: std::sync::Arc<Vec<PathBuf>>,
    /// Absolute file paths to leave out of the result, typically the ones
    /// Composer's generated classmap already covers.
    skip_paths: &'a HashSet<PathBuf>,
    /// Compiled `[indexing]` exclude globs and extra PHP extensions.
    filters: std::sync::Arc<IndexFilters>,
    /// Where to report the directory symlinks this walk descends
    /// through, so the client can be asked to watch the trees behind
    /// them. `None` for a walk whose caller has no watchers to register
    /// (the `analyze`, `fix`, and `format` pipelines).
    followed: Option<&'a FollowedLinks>,
}

impl<'a> WalkOptions<'a> {
    pub(super) fn new(
        skip_dirs: Vec<PathBuf>,
        skip_paths: &'a HashSet<PathBuf>,
        filters: std::sync::Arc<IndexFilters>,
        followed: Option<&'a FollowedLinks>,
    ) -> Self {
        Self {
            skip_dirs: std::sync::Arc::new(skip_dirs),
            skip_paths,
            filters,
            followed,
        }
    }
}

/// Walk a set of directory roots across all CPU cores and return the
/// matching files of each.
///
/// All the roots go into a *single* `ignore` walk, which matters twice
/// over.  `ignore` then reads the global gitignore and compiles each
/// shared ancestor `.gitignore` once for the whole set instead of once
/// per root: a vendor tree has thousands of autoload roots sharing the
/// same ancestors, and that setup cost used to be most of the scan's CPU
/// time.  And its worker pool splits the work by *directory* over a
/// work-stealing deque, so one enormous package (`aws/aws-sdk-php` has
/// 2900 files under a single PSR-4 root) no longer runs on one core while
/// the rest of the scan waits for it.
///
/// The result has one entry per input root, in root order, so callers
/// keep control over the concatenation order their duplicate-symbol
/// tie-break depends on.  Roots that are not directories yield an empty
/// entry, and a file below two overlapping roots is reported for both.
///
/// Each root's files are sorted by path.  A parallel walk reaches files
/// in whatever order the workers happen to get to them, and a first-wins
/// classmap must not depend on that; sorting also makes the result
/// reproducible across runs, which the readdir order it replaces was not.
pub(super) fn walk_roots(roots: &[PathBuf], opts: &WalkOptions) -> Vec<Vec<PathBuf>> {
    use ignore::WalkState;

    // Two PSR-4 prefixes can map to the same directory, so the walk is
    // over the distinct roots; the attribution below still hands the
    // files to every root that named them.
    let mut roots_by_path: HashMap<&Path, Vec<usize>> = HashMap::new();
    let mut distinct_roots: Vec<&Path> = Vec::new();
    for (index, dir) in roots.iter().enumerate() {
        if !dir.is_dir() {
            continue;
        }
        match roots_by_path.entry(dir.as_path()) {
            std::collections::hash_map::Entry::Occupied(mut e) => e.get_mut().push(index),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(vec![index]);
                distinct_roots.push(dir);
            }
        }
    }

    let mut out: Vec<Vec<PathBuf>> = vec![Vec::new(); roots.len()];
    let Some((first_root, other_roots)) = distinct_roots.split_first() else {
        return out;
    };

    // Every root is claimed up front, so a link inside one root pointing
    // into another is skipped in favour of that root's own walk.
    let mut builder = workspace_walk_builder(
        first_root,
        std::sync::Arc::clone(&opts.skip_dirs),
        std::sync::Arc::clone(&opts.filters),
        false,
        LinkClaims::new(
            distinct_roots.iter().map(|r| r.to_path_buf()),
            opts.followed,
        ),
    );
    for dir in other_roots {
        builder.add(dir);
    }
    builder.threads(crate::parallel::available_cores());

    let (tx, rx) = std::sync::mpsc::channel::<(usize, PathBuf)>();
    let skip_paths = opts.skip_paths;
    let filters = &opts.filters;
    let roots_by_path = &roots_by_path;
    builder.build_parallel().run(|| {
        let tx = tx.clone();
        Box::new(move |entry| {
            let Ok(entry) = entry else {
                return WalkState::Continue;
            };
            let path = entry.path();
            let file_type = entry.file_type();
            if file_type.is_some_and(|ft| ft.is_dir())
                || !filters.is_php_file(path)
                || skip_paths.contains(path)
                // `ignore` reports the target's type for a symlink it
                // followed, and the symlink's own for one it did not
                // (a claimed target, a broken link), so confirm the
                // target is a regular file before indexing it.  The
                // tests above keep this stat off the common path.
                || !(file_type.is_some_and(|ft| ft.is_file()) || path.is_file())
            {
                return WalkState::Continue;
            }
            // The root this entry came from is its `depth`-th ancestor.
            // Overlapping roots (Laravel maps both `src/Illuminate` and
            // `src/Illuminate/Collections`) are walked once each, so
            // attributing by depth gives every root exactly the files its
            // own walk produced.
            if let Some(root) = path.ancestors().nth(entry.depth())
                && let Some(indices) = roots_by_path.get(root)
            {
                for &index in indices {
                    let _ = tx.send((index, path.to_path_buf()));
                }
            }
            WalkState::Continue
        })
    });
    drop(tx);

    for (index, path) in rx {
        out[index].push(path);
    }
    for files in &mut out {
        files.sort_unstable();
    }
    out
}

#[cfg(test)]
#[path = "walk_tests.rs"]
mod tests;
