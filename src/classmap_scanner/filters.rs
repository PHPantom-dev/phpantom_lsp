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

use std::path::Path;
use std::sync::{Arc, OnceLock};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

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
}
