//! Where a namespace's files go: the PSR-4 directory move a namespace
//! rename carries along, and the conflicts that rule a move out.

use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::composer;

impl Backend {
    /// Why a namespace cannot be renamed onto `new_prefix`, or `None`
    /// when every name it carries lands somewhere free.
    ///
    /// Renaming `App\Internal` to an `App\Support` that already exists
    /// is a merge, and the merge is only well-defined while the two
    /// namespaces share no name.  Where they do, there is no answer the
    /// rename can pick: moving the rest and leaving the clash behind
    /// still rewrites every `App\Internal\Helper` reference to
    /// `App\Support\Helper`, which is a different class.
    pub(super) fn namespace_merge_conflict(
        &self,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Option<String> {
        if old_prefix.eq_ignore_ascii_case(new_prefix) {
            return None;
        }

        let mut clashes: Vec<String> = {
            let index = self.symbols.fqn_uri_index.read();
            index
                .iter()
                .filter_map(|(fqn, uri)| {
                    // The tail of a name the rename carries over, e.g.
                    // `Nested\Deep` of `App\Internal\Nested\Deep`.
                    let tail = fqn
                        .get(..old_prefix.len())
                        .filter(|head| head.eq_ignore_ascii_case(old_prefix))
                        .and_then(|_| fqn.get(old_prefix.len()..))
                        .and_then(|rest| rest.strip_prefix('\\'))?;
                    let (declared, at_uri) =
                        index.get_key_value(&format!("{}\\{}", new_prefix, tail))?;
                    // A name the move itself produces is not a clash.
                    (at_uri != uri).then(|| declared.to_string())
                })
                .collect()
        };
        clashes.sort();
        clashes.dedup();

        if clashes.is_empty() {
            return None;
        }

        let listed: Vec<&str> = clashes.iter().take(5).map(String::as_str).collect();
        let extra = clashes.len().saturating_sub(listed.len());
        let suffix = if extra > 0 {
            format!(" (and {} more)", extra)
        } else {
            String::new()
        };

        Some(format!(
            "Cannot rename `{}` to `{}`: {} already declares {}{}. \
             Rename or move the clashing {} first, then retry.",
            old_prefix,
            new_prefix,
            new_prefix,
            listed.join(", "),
            suffix,
            if clashes.len() == 1 {
                "class"
            } else {
                "classes"
            },
        ))
    }

    /// Determine PSR-4 file/directory rename operations for a namespace
    /// rename.
    ///
    /// Returns pairs of `(old_uri, new_uri)`, or `None` if no PSR-4
    /// mapping applies.  Where the destination directory does not exist
    /// the whole directory is moved in one operation; where it does, the
    /// move is a merge and each file is moved individually so the
    /// contents already there survive.
    pub(super) fn build_namespace_psr4_rename_ops(
        &self,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Option<Vec<(Url, Url)>> {
        let psr4 = self.workspace.psr4_mappings.read();
        let workspace_root = self.workspace.workspace_root.read().clone()?;

        // The destination directory has to come from whichever mapping
        // covers the *new* namespace, which need not be the one covering
        // the old one: a namespace can move between mappings, or out of
        // the autoload map entirely.  Deriving it from the old mapping
        // instead builds a path around a prefix the new name never had.
        // No mapping covers the destination means no file can be placed
        // there, so nothing moves and the declarations are rewritten in
        // place.
        let new_dir = composer::psr4_directory_for_namespace(&psr4, &workspace_root, new_prefix)?;

        let mut ops: Vec<(Url, Url)> = Vec::new();

        for (_, old_dir) in namespace_source_dirs(&psr4, &workspace_root, old_prefix, &new_dir) {
            if new_dir.exists() {
                collect_merge_move_ops(&old_dir, &old_dir, &new_dir, &mut ops);
            } else {
                let old_url = Url::from_file_path(&old_dir).ok()?;
                let new_url = Url::from_file_path(&new_dir).ok()?;
                ops.push((old_url, new_url));
            }
        }

        if ops.is_empty() { None } else { Some(ops) }
    }

    /// Why a namespace cannot be moved out of its PSR-4 roots, or `None`
    /// when it has just the one to move.
    ///
    /// Composer accepts an array of directories per prefix, and every one
    /// of them holds part of the same namespace.  There is no single
    /// directory to move, and each root's files would land on the same
    /// destination, so the plan would carry classes the caller never
    /// named (and collide with itself doing so).  Moving only the root
    /// the caller meant needs a rewriter that can split a namespace by
    /// the directory its classes are declared in, which is not what this
    /// one does, so the move is refused instead.
    pub(super) fn namespace_psr4_root_conflict(
        &self,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Option<String> {
        let psr4 = self.workspace.psr4_mappings.read();
        let workspace_root = self.workspace.workspace_root.read().clone()?;
        let new_dir = composer::psr4_directory_for_namespace(&psr4, &workspace_root, new_prefix)?;

        let roots = namespace_source_dirs(&psr4, &workspace_root, old_prefix, &new_dir);
        if roots.len() < 2 {
            return None;
        }

        let listed: Vec<String> = roots
            .iter()
            .map(|(mapping, _)| format!("`{}`", mapping.base_path))
            .collect();

        Some(format!(
            "Cannot move `{}` to `{}`: PSR-4 spreads `{}` over more than one root ({}), \
             so there is no single directory to move and the classes under every root \
             would move together. Give `{}` a single root in `composer.json`, or move \
             the classes one at a time.",
            old_prefix,
            new_prefix,
            old_prefix,
            listed.join(", "),
            old_prefix,
        ))
    }
}

/// The directories holding the files a namespace move takes along, each
/// paired with the PSR-4 mapping that places it there.
///
/// A mapping Composer lists but the project never created holds nothing
/// to move, and a directory that already is the destination is not a
/// move at all, so both are left out. Nested prefixes can also name one
/// directory twice (`Tests\` at `tests/` and `Tests\Unit\` at
/// `tests/Unit/` both place `Tests\Unit` in `tests/Unit`), so each
/// directory is kept once. What remains is what the move has to carry,
/// which is also what tells the caller whether the namespace sits in
/// more than one root.
fn namespace_source_dirs<'a>(
    psr4: &'a [composer::Psr4Mapping],
    workspace_root: &'a Path,
    old_prefix: &'a str,
    new_dir: &Path,
) -> Vec<(&'a composer::Psr4Mapping, PathBuf)> {
    let mut dirs: Vec<(&composer::Psr4Mapping, PathBuf)> = Vec::new();
    for (mapping, old_dir) in
        composer::psr4_directories_for_namespace(psr4, workspace_root, old_prefix)
    {
        if old_dir == new_dir || !old_dir.is_dir() {
            continue;
        }
        if dirs.iter().any(|(_, seen)| seen == &old_dir) {
            continue;
        }
        dirs.push((mapping, old_dir));
    }
    dirs
}

/// Collect one move operation per file under `dir`, rebasing each onto
/// `new_root` at the same relative path.
///
/// This is the merge case: `new_root` already exists, so moving the
/// source directory on top of it would clobber or fail depending on the
/// editor.  A destination that is already occupied is left out entirely
/// — the file stays where it is rather than overwriting what is there.
fn collect_merge_move_ops(dir: &Path, old_root: &Path, new_root: &Path, ops: &mut Vec<(Url, Url)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // `file_type` describes the entry itself rather than what a link
        // points at, so a link is moved as the link it is.  Following one
        // back up the tree would re-enter it until the kernel's symlink
        // limit stops the walk, emitting a move for the same file under
        // every path it went round.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_merge_move_ops(&path, old_root, new_root, ops);
            continue;
        }

        let Ok(relative) = path.strip_prefix(old_root) else {
            continue;
        };
        let destination = new_root.join(relative);
        if destination.exists() {
            continue;
        }

        if let (Ok(old_url), Ok(new_url)) = (
            Url::from_file_path(&path),
            Url::from_file_path(&destination),
        ) {
            ops.push((old_url, new_url));
        }
    }
}
