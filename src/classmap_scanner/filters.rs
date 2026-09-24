//! Compiled `[indexing]` file filters: exclude globs and extra PHP
//! extensions.
//!
//! `.phpantom.toml` lets a project exclude paths from workspace
//! discovery (`exclude`, gitignore syntax relative to the workspace
//! root) and treat additional file extensions as PHP source
//! (`extensions`, e.g. Drupal's `module`/`inc`). The raw config
//! strings are compiled once into an [`IndexFilters`] that every
//! directory walker and the file watcher consult, so glob compilation
//! never happens on a per-file path.
//!
//! Exclusion uses gitignore semantics (via [`ignore::gitignore`])
//! rather than plain globs: a bare name matches at any depth, a
//! pattern containing `/` anchors to the workspace root, a trailing
//! `/` restricts to directories, and a leading `!` re-includes.
//! `Override` from the same crate is deliberately *not* used — its
//! whitelist-first semantics would invert the meaning of an exclude
//! list containing a `!` pattern.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// The `ignore` walk every workspace scan starts from: the repository's
/// gitignore rules (its own, the global one, `.git/info/exclude`, parent
/// directories, and ripgrep-style `.ignore` files) plus the two
/// exclusions that hold regardless of what git ignores. `skip_dirs`
/// (vendor trees scanned through `installed.json`, monorepo subproject
/// roots another pipeline covers) are never entered, and `[indexing]
/// exclude` matches are pruned through `filters`.
///
/// Dotfiles are skipped unless `include_dotfiles` is set, in which case
/// `.git` itself is still pruned. A directory symlink is descended into
/// the first time the walk reaches its target and skipped every later
/// time, so no directory is walked twice under two spellings; see
/// [`LinkClaims`]. Callers add further roots, set the thread count, or
/// keep the walk serial as their scan needs.
pub fn workspace_walk_builder(
    root: &Path,
    skip_dirs: Arc<Vec<PathBuf>>,
    filters: Arc<IndexFilters>,
    include_dotfiles: bool,
    claims: LinkClaims,
) -> WalkBuilder {
    // Pruning a skipped tree by path only stops the walk reaching it
    // *directly*.  A link pointing into one arrives under a different
    // path, so the prune above never fires and the tree is walked after
    // all, under a spelling nested inside whatever held the link.
    claims.cover(skip_dirs.iter().cloned());
    let skip_dirs = respell_under_root(root, skip_dirs);

    let mut builder = WalkBuilder::new(root);
    builder
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(!include_dotfiles)
        .parents(true)
        .ignore(true)
        .follow_links(true)
        .filter_entry(move |entry| {
            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            if is_dir {
                if include_dotfiles && entry.file_name() == ".git" {
                    return false;
                }
                if skip_dirs.iter().any(|dir| dir == entry.path()) {
                    return false;
                }
                // Depth 0 is a root the caller asked for by name, even
                // when it is itself a symlink, so it is never a second
                // spelling of anything.
                if entry.depth() > 0 && entry.path_is_symlink() && !claims.claim(entry.path()) {
                    return false;
                }
            }
            !filters.is_excluded_entry(entry.path(), is_dir)
        });
    builder
}

/// `skip_dirs`, plus each one spelled under `root` wherever it names the
/// same directory by another path.
///
/// Entries are pruned by comparing paths, and a walk's paths are spelled
/// the way its root is.  A skipped tree registered through an alias of the
/// root (a symlinked checkout, macOS's `/var` for `/private/var`) would
/// otherwise never compare equal and be walked after all.  Resolving the
/// root and each skipped tree once per walk keeps the per-directory check
/// a plain comparison.
fn respell_under_root(root: &Path, skip_dirs: Arc<Vec<PathBuf>>) -> Arc<Vec<PathBuf>> {
    let Ok(real_root) = root.canonicalize() else {
        return skip_dirs;
    };
    let respelled: Vec<PathBuf> = skip_dirs
        .iter()
        .filter_map(|dir| {
            let real_dir = dir.canonicalize().ok()?;
            let spelled = root.join(real_dir.strip_prefix(&real_root).ok()?);
            (!skip_dirs.contains(&spelled)).then_some(spelled)
        })
        .collect();
    if respelled.is_empty() {
        return skip_dirs;
    }
    Arc::new(skip_dirs.iter().cloned().chain(respelled).collect())
}

/// The link targets a single walk has already committed to descending
/// into, so no directory is walked twice under two spellings.
///
/// One instance covers one walk. A walk that reports into a
/// [`FollowedLinks`] registry also tells it which links it went through,
/// which is what lets the watcher registration ask the client to watch
/// the trees behind them.
///
/// `ignore` refuses a symlink pointing at one of its own ancestors,
/// which bounds a true cycle, and nothing else. Three shapes slip past
/// it, all of them real: two links to the same tree, a link pointing
/// back inside the workspace the walk is already covering, and a chain
/// of directories holding two links apiece, which reaches the leaf 2^n
/// ways without ever forming a cycle. Each one walks the same files
/// again under a different path, so a class resolves to an arbitrary
/// spelling of its file, find-references reports every hit as many times
/// as the walk found it, and the fan-out case buys that with exponential
/// work.
///
/// Claiming a target's real path the first time a link reaches it makes
/// every directory visited once however many ways the links spell it.
/// The walk's own roots are claimed up front, so a link pointing back
/// inside one of them loses to the spelling the walk already had. Two
/// links to the *same* tree outside the roots are equally arbitrary and
/// a parallel walk picks whichever gets there first; that one is not
/// reproducible across runs, where indexing both was reproducibly wrong.
///
/// A claim covers the whole walk rather than the root that made it, even
/// though [`walk_roots`](super::discovery) otherwise keeps each root's
/// files separate so it can namespace-filter them against that root's own
/// PSR-4 prefix. Two roots whose links converge on one tree therefore see
/// it once between them, which costs nothing real: a PHP file declares a
/// single namespace, so at most one root's prefix could ever have
/// accepted it, and the classmap the rest feeds is a flat first-wins map
/// where the second copy is discarded anyway.
///
/// Only directories are tracked. A symlinked file costs a bounded amount
/// of duplicate work, and paying a `canonicalize` per file to find that
/// out would cost more than it saves.
#[derive(Clone)]
pub struct LinkClaims {
    claimed: Arc<parking_lot::Mutex<Vec<PathBuf>>>,
    followed: Option<FollowedLinks>,
}

impl LinkClaims {
    /// Start a walk with `roots` already covered, reporting the links it
    /// descends through into `followed` when one is given.
    ///
    /// A root that cannot be canonicalized is left out rather than
    /// failing the walk: the walk still yields it, and the only loss is
    /// that a link pointing back into it is followed instead of skipped.
    pub fn new(roots: impl IntoIterator<Item = PathBuf>, followed: Option<&FollowedLinks>) -> Self {
        let claimed = roots
            .into_iter()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .collect();
        Self {
            claimed: Arc::new(parking_lot::Mutex::new(claimed)),
            followed: followed.cloned(),
        }
    }

    /// Treat `paths` as trees the walk already accounts for, so a link
    /// pointing into one of them is skipped in favour of the spelling that
    /// tree is covered under.
    ///
    /// This is what a walk's `skip_dirs` need: a vendor tree scanned
    /// through `installed.json`, or a monorepo subproject another pipeline
    /// walks, is already indexed under its own path.
    /// `orchestra/testbench-core` ships `laravel/vendor -> <project>/vendor`
    /// and is installed in a great many Laravel projects, so a walk of the
    /// vendor packages meets exactly this link and would otherwise index
    /// every package a second time beneath it.
    pub(crate) fn cover(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let covered = paths
            .into_iter()
            .filter_map(|path| std::fs::canonicalize(path).ok());
        self.claimed.lock().extend(covered);
    }

    /// Whether the walk should descend through the directory symlink at
    /// `link`, claiming its target when it should.
    ///
    /// A broken link, or one whose target is already covered by a root
    /// or by an earlier link, answers `false`.
    pub(crate) fn claim(&self, link: &Path) -> bool {
        let Ok(target) = std::fs::canonicalize(link) else {
            return false;
        };
        let mut claimed = self.claimed.lock();
        if claimed.iter().any(|seen| target.starts_with(seen)) {
            return false;
        }
        claimed.push(target.clone());
        drop(claimed);
        if let Some(followed) = &self.followed {
            followed.record(link.to_path_buf(), target);
        }
        true
    }
}

/// Every directory symlink the workspace walks have indexed through,
/// paired with the real directory it resolves to.
///
/// Owned by the `Backend` and filled in by the walks themselves, because
/// a link is only interesting once something was indexed behind it. Two
/// consumers need it, and both would otherwise be guessing:
///
/// - **Watcher registration.** A client watches its workspace folders,
///   and a tree behind a link is not in one of them. Each link here
///   becomes a [`RelativePattern`](tower_lsp::lsp_types::RelativePattern)
///   watcher based at the link, which is what asks the client for those
///   events.
/// - **Event paths.** A client that resolves the base it was handed
///   reports the real path, which matches nothing in an index that holds
///   the symlink spelling. [`Self::to_link_spelling`] maps it back.
///
/// Links accumulate: a rediscovery walk re-records the ones still there
/// rather than starting over, so a link deleted from disk leaves a
/// watcher for a path that no longer exists until the session ends. The
/// client is watching a path that produces no events, which costs one
/// dead registration and nothing else.
#[derive(Clone, Default)]
pub struct FollowedLinks {
    /// Link path as the walk spelled it → the canonical directory it
    /// resolves to. Keyed by link so re-walking is idempotent, and small
    /// enough (one entry per symlink a project deliberately checked in)
    /// that the reverse lookup is a scan.
    links: Arc<parking_lot::RwLock<std::collections::BTreeMap<PathBuf, PathBuf>>>,
}

impl FollowedLinks {
    fn record(&self, link: PathBuf, target: PathBuf) {
        self.links.write().insert(link, target);
    }

    /// Whether any walk has indexed through a symlink yet.
    pub fn is_empty(&self) -> bool {
        self.links.read().is_empty()
    }

    /// The link spellings, sorted, for the watcher registration to base
    /// its patterns on.
    pub fn link_paths(&self) -> Vec<PathBuf> {
        self.links.read().keys().cloned().collect()
    }

    /// Rewrite a real path that falls inside a followed link's target
    /// into the spelling the index holds, or `None` when it is already a
    /// path the walk could have produced.
    ///
    /// The longest matching target wins, so a link nested inside another
    /// link's tree maps to the nearer of the two.
    pub fn to_link_spelling(&self, path: &Path) -> Option<PathBuf> {
        let links = self.links.read();
        let (link, rest) = links
            .iter()
            .filter(|(link, _)| !path.starts_with(link))
            .filter_map(|(link, target)| path.strip_prefix(target).ok().map(|rest| (link, rest)))
            .min_by_key(|(_, rest)| rest.components().count())?;
        Some(link.join(rest))
    }
}

/// Compiled exclude matcher and extra-extension set for file discovery.
pub struct IndexFilters {
    /// Compiled `[indexing] exclude` globs, `None` when no valid
    /// pattern is configured so the hot path is a single branch.
    excludes: Option<Gitignore>,
    /// The raw patterns `excludes` was compiled from, kept only so
    /// [`may_admit_more_than`](Self::may_admit_more_than) can tell a
    /// widened exclude list from a narrowed one; `Gitignore` does not
    /// hand its patterns back.
    exclude_patterns: Vec<String>,
    /// Lowercase extra extensions (without the dot) treated as PHP.
    extensions: Vec<String>,
}

impl IndexFilters {
    /// Compile the raw `[indexing]` filter strings.
    ///
    /// Invalid glob patterns are skipped with a warning rather than
    /// failing the whole config load, mirroring how
    /// `[[diagnostics.ignore]]` rules are compiled. `root` anchors
    /// patterns containing `/`; without a workspace root the exclude
    /// list is ignored (extensions still apply).
    pub fn compile(root: Option<&Path>, exclude: &[String], extensions: &[String]) -> Self {
        let excludes = root.filter(|_| !exclude.is_empty()).and_then(|root| {
            let mut builder = GitignoreBuilder::new(root);
            for pattern in exclude {
                if let Err(e) = builder.add_line(None, pattern) {
                    eprintln!(
                        "warning: skipping invalid [indexing] exclude pattern `{pattern}`: {e}"
                    );
                }
            }
            match builder.build() {
                Ok(gi) if gi.num_ignores() + gi.num_whitelists() > 0 => Some(gi),
                Ok(_) => None,
                Err(e) => {
                    eprintln!("warning: failed to compile [indexing] exclude patterns: {e}");
                    None
                }
            }
        });

        let extensions: Vec<String> = extensions
            .iter()
            .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
            .filter(|ext| !ext.is_empty() && ext != "php")
            .collect();

        Self {
            excludes,
            exclude_patterns: exclude.to_vec(),
            extensions,
        }
    }

    /// A shared no-op filter for callers outside the indexing pipeline
    /// (thin public wrappers, tests).
    pub fn empty() -> Arc<IndexFilters> {
        static EMPTY: OnceLock<Arc<IndexFilters>> = OnceLock::new();
        Arc::clone(EMPTY.get_or_init(|| {
            Arc::new(IndexFilters {
                excludes: None,
                exclude_patterns: Vec::new(),
                extensions: Vec::new(),
            })
        }))
    }

    /// Whether a walked entry is excluded by `[indexing] exclude`.
    ///
    /// Matches the entry's own path only. Directory walkers prune
    /// excluded directories, so files below them are never asked;
    /// for arbitrary paths (file-watch events) use
    /// [`is_excluded_path`](Self::is_excluded_path) instead.
    pub fn is_excluded_entry(&self, path: &Path, is_dir: bool) -> bool {
        self.excludes
            .as_ref()
            .is_some_and(|gi| gi.matched(path, is_dir).is_ignore())
    }

    /// Whether a path is excluded, considering its ancestors.
    ///
    /// A file inside an excluded directory is itself excluded, the way
    /// git never descends into an ignored directory.
    pub fn is_excluded_path(&self, path: &Path, is_dir: bool) -> bool {
        self.excludes
            .as_ref()
            .is_some_and(|gi| gi.matched_path_or_any_parents(path, is_dir).is_ignore())
    }

    /// Whether a file's extension marks it as PHP source: `.php` plus
    /// any configured `[indexing] extensions`.
    pub fn is_php_file(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| self.is_php_extension(ext))
    }

    /// Whether an extension string (without the dot) is treated as PHP.
    pub fn is_php_extension(&self, ext: &str) -> bool {
        ext.eq_ignore_ascii_case("php")
            || self
                .extensions
                .iter()
                .any(|extra| ext.eq_ignore_ascii_case(extra))
    }

    /// The configured extra extensions (lowercase, without the dot).
    pub fn extra_extensions(&self) -> &[String] {
        &self.extensions
    }

    /// Whether replacing `previous` with `self` can let a file into the
    /// index that `previous` kept out, so the caller knows a rescan of
    /// the workspace is needed rather than an eviction pass alone.
    ///
    /// Answered from the patterns rather than the filesystem, and
    /// deliberately errs towards `true`. Only two edits can re-admit a
    /// path: dropping a pattern, and adding a `!` re-include. A plain
    /// pattern that was not there before can only exclude more, whatever
    /// position it lands in, because gitignore's last-match-wins rule
    /// still only ever lets a non-negated pattern say "ignore".
    pub fn may_admit_more_than(&self, previous: &IndexFilters) -> bool {
        if self
            .extensions
            .iter()
            .any(|ext| !previous.extensions.contains(ext))
        {
            return true;
        }
        if previous
            .exclude_patterns
            .iter()
            .any(|pattern| !self.exclude_patterns.contains(pattern))
        {
            return true;
        }
        self.exclude_patterns
            .iter()
            .any(|pattern| pattern.starts_with('!') && !previous.exclude_patterns.contains(pattern))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn filters(exclude: &[&str], extensions: &[&str]) -> IndexFilters {
        IndexFilters::compile(
            Some(Path::new("/ws")),
            &exclude.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &extensions.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn bare_name_matches_at_any_depth() {
        let f = filters(&["fixtures"], &[]);
        assert!(f.is_excluded_entry(&PathBuf::from("/ws/a/b/fixtures"), true));
        assert!(f.is_excluded_entry(&PathBuf::from("/ws/fixtures"), true));
        assert!(!f.is_excluded_entry(&PathBuf::from("/ws/src"), true));
    }

    #[test]
    fn slash_pattern_anchors_to_root() {
        let f = filters(&["web/sites/default/files"], &[]);
        assert!(f.is_excluded_entry(&PathBuf::from("/ws/web/sites/default/files"), true));
        assert!(!f.is_excluded_entry(&PathBuf::from("/ws/other/web/sites/default/files"), true));
    }

    #[test]
    fn trailing_slash_restricts_to_directories() {
        let f = filters(&["tests/"], &[]);
        assert!(f.is_excluded_entry(&PathBuf::from("/ws/module/tests"), true));
        assert!(!f.is_excluded_entry(&PathBuf::from("/ws/module/tests"), false));
    }

    #[test]
    fn negation_re_includes() {
        // Gitignore idiom: `dir/*` + `!dir/keep.php`. (A bare `dir`
        // pattern would prune the directory before the re-include is
        // ever consulted, exactly like git.)
        let f = filters(&["generated/*", "!generated/keep.php"], &[]);
        assert!(f.is_excluded_entry(&PathBuf::from("/ws/generated/foo.php"), false));
        assert!(!f.is_excluded_entry(&PathBuf::from("/ws/generated/keep.php"), false));
        // The directory itself stays walkable so the re-include works.
        assert!(!f.is_excluded_entry(&PathBuf::from("/ws/generated"), true));
    }

    #[test]
    fn path_check_covers_ancestors() {
        let f = filters(&["generated"], &[]);
        // The entry check only matches the path itself…
        assert!(!f.is_excluded_entry(&PathBuf::from("/ws/generated/deep/file.php"), false));
        // …the path check also matches through excluded ancestors.
        assert!(f.is_excluded_path(&PathBuf::from("/ws/generated/deep/file.php"), false));
    }

    #[test]
    fn invalid_pattern_is_skipped_not_fatal() {
        let f = filters(&["[unclosed", "vendor-extra"], &[]);
        assert!(f.is_excluded_entry(&PathBuf::from("/ws/vendor-extra"), true));
    }

    #[test]
    fn no_root_disables_excludes_but_keeps_extensions() {
        let strings = vec!["tests".to_string()];
        let exts = vec!["module".to_string()];
        let f = IndexFilters::compile(None, &strings, &exts);
        assert!(!f.is_excluded_entry(&PathBuf::from("/ws/tests"), true));
        assert!(f.is_php_file(&PathBuf::from("/ws/foo.module")));
    }

    #[test]
    fn extensions_are_normalized() {
        let f = filters(&[], &[".Module", "php", "", "inc"]);
        assert_eq!(f.extra_extensions(), &["module", "inc"]);
        assert!(f.is_php_file(&PathBuf::from("/ws/foo.MODULE")));
        assert!(f.is_php_file(&PathBuf::from("/ws/foo.php")));
        assert!(!f.is_php_file(&PathBuf::from("/ws/foo.txt")));
    }

    /// A leading `/` anchors to the workspace root. Editor clients rely
    /// on this to translate a rootless glob from a dialect where that
    /// means "at the root" (VS Code's `files.exclude`) into gitignore,
    /// where a bare name would instead match at any depth.
    #[test]
    fn leading_slash_anchors_to_root() {
        let f = filters(&["/node_modules"], &[]);
        assert!(f.is_excluded_entry(&PathBuf::from("/ws/node_modules"), true));
        assert!(!f.is_excluded_entry(&PathBuf::from("/ws/pkg/node_modules"), true));
    }

    /// A directory whose name begins with `!` or `#` has to stay
    /// excludable, so both are escapable rather than being read as a
    /// negation and a comment.
    #[test]
    fn a_leading_bang_or_hash_can_be_escaped_to_a_literal() {
        let f = filters(&["\\!important", "\\#tmp"], &[]);
        assert!(f.is_excluded_entry(&PathBuf::from("/ws/!important"), true));
        assert!(f.is_excluded_entry(&PathBuf::from("/ws/#tmp"), true));
    }

    /// Excluding more is the common editor-settings edit, and it needs
    /// no disk access to honour: everything newly excluded is already in
    /// the index, so eviction alone finishes the job.
    #[test]
    fn adding_an_exclude_admits_nothing_new() {
        let before = filters(&["generated"], &[]);
        let after = filters(&["generated", "build"], &[]);
        assert!(!after.may_admit_more_than(&before));
    }

    /// The three edits that put files back in scope. Each one needs a
    /// walk to find what is now indexable, since nothing in memory
    /// records the files that were skipped.
    #[test]
    fn re_including_anything_needs_a_rescan() {
        let before = filters(&["generated", "build"], &[]);
        assert!(filters(&["generated"], &[]).may_admit_more_than(&before));
        assert!(
            filters(&["generated", "build", "!build/keep.php"], &[]).may_admit_more_than(&before)
        );
        assert!(filters(&["generated", "build"], &["module"]).may_admit_more_than(&before));
    }

    /// Clients re-push settings that repeat what is already in force, so
    /// an unchanged list must not read as a re-include.
    #[test]
    fn an_unchanged_filter_set_admits_nothing_new() {
        let before = filters(&["generated", "!generated/keep.php"], &["module"]);
        let after = filters(&["generated", "!generated/keep.php"], &["module"]);
        assert!(!after.may_admit_more_than(&before));
    }

    #[test]
    fn empty_filter_is_noop() {
        let f = IndexFilters::empty();
        assert!(!f.is_excluded_path(&PathBuf::from("/ws/anything"), true));
        assert!(f.is_php_file(&PathBuf::from("/ws/foo.php")));
        assert!(!f.is_php_file(&PathBuf::from("/ws/foo.module")));
    }

    /// A link inside another link's tree has to win for paths under it,
    /// or a file two links deep is respelled through the outer link and
    /// names a path that does not exist.
    #[test]
    fn to_link_spelling_prefers_the_nearest_link() {
        let dir = tempfile::tempdir().unwrap();
        let outer_target = dir.path().join("outer");
        let inner_target = dir.path().join("inner");
        std::fs::create_dir_all(outer_target.join("nested")).unwrap();
        std::fs::create_dir_all(&inner_target).unwrap();

        let links = FollowedLinks::default();
        links.record(PathBuf::from("/ws/outer"), outer_target.clone());
        links.record(
            PathBuf::from("/ws/outer/nested/inner"),
            inner_target.clone(),
        );

        assert_eq!(
            links.to_link_spelling(&outer_target.join("A.php")),
            Some(PathBuf::from("/ws/outer/A.php"))
        );
        assert_eq!(
            links.to_link_spelling(&inner_target.join("B.php")),
            Some(PathBuf::from("/ws/outer/nested/inner/B.php"))
        );
    }

    /// A path already spelled through a link is what the walk produced, so
    /// it must pass through untouched rather than being rewritten again.
    #[test]
    fn to_link_spelling_leaves_an_already_linked_path_alone() {
        let links = FollowedLinks::default();
        links.record(PathBuf::from("/ws/link"), PathBuf::from("/opt/real"));

        assert_eq!(
            links.to_link_spelling(&PathBuf::from("/ws/link/A.php")),
            None
        );
        assert_eq!(
            links.to_link_spelling(&PathBuf::from("/elsewhere/A.php")),
            None
        );
    }
}
