//! The per-file half of a project-wide Laravel registry.
//!
//! The macro, command, morph-map, gate, and storage-driver indexes are all
//! built the same way: every contributing file is scanned on its own, the
//! scan is kept under the file's URI so an edit replaces just that file's
//! registrations, and the merged lookups the features read are rebuilt
//! from the whole set afterwards.  This type holds the per-file scans;
//! each registry keeps its own derived lookups and its own `rebuild`.

use std::collections::HashMap;

/// What one file contributes to a registry, replaced whole on every
/// rescan of that file.
pub(crate) trait Contribution {
    /// Whether the contribution registers nothing, so the file can be
    /// dropped from the registry instead of stored empty.
    fn is_empty(&self) -> bool;
}

impl<T> Contribution for Vec<T> {
    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }
}

/// A registry built from per-file contributions: the contributions
/// themselves and the derived lookups `rebuild` recomputes from them.
pub(crate) trait Registry {
    type Contribution: Contribution;
    fn files(&self) -> &FileContributions<Self::Contribution>;
    fn files_mut(&mut self) -> &mut FileContributions<Self::Contribution>;
    fn rebuild(&mut self);
}

macro_rules! impl_registry {
    ($index:ty, $contribution:ty) => {
        impl Registry for $index {
            type Contribution = $contribution;

            fn files(&self) -> &FileContributions<Self::Contribution> {
                &self.files
            }

            fn files_mut(&mut self) -> &mut FileContributions<Self::Contribution> {
                &mut self.files
            }

            fn rebuild(&mut self) {
                <$index>::rebuild(self)
            }
        }
    };
}

impl_registry!(super::LaravelGateIndex, super::gates::GateScan);
impl_registry!(super::LaravelMorphMapIndex, super::MorphMapScan);
impl_registry!(
    super::LaravelCommandIndex,
    Vec<super::commands::CommandEntry>
);
impl_registry!(
    super::LaravelStorageDriverIndex,
    Vec<super::storage::StorageDriverRegistration>
);

/// Replace what `uri` contributes to `registry` after an edit.
///
/// `may_contribute` is the caller's cheap pre-filter on the new content
/// (a byte search for the registration token, a file-name rule).  A file
/// that neither contributed before nor passes it is left alone without
/// running `scan`; one that contributed before is always rescanned, so a
/// registration the edit removed goes with it.  Returns whether the
/// registry was touched, for the caller's own downstream invalidation.
pub(crate) fn refresh_file<R: Registry>(
    registry: &parking_lot::RwLock<R>,
    uri: &str,
    may_contribute: bool,
    scan: impl FnOnce() -> R::Contribution,
) -> bool {
    let was_contributor = registry.read().files().has_uri(uri);
    if !was_contributor && !may_contribute {
        return false;
    }
    let contribution = scan();
    if !was_contributor && contribution.is_empty() {
        return false;
    }
    let mut registry = registry.write();
    registry.files_mut().set_file(uri.to_string(), contribution);
    registry.rebuild();
    true
}

/// One contribution per contributing file, keyed by the file's URI.
pub(crate) struct FileContributions<C> {
    by_uri: HashMap<String, C>,
}

impl<C> Default for FileContributions<C> {
    fn default() -> Self {
        Self {
            by_uri: HashMap::new(),
        }
    }
}

impl<C: Contribution> FileContributions<C> {
    /// Replace the contribution of `uri`.  An empty contribution removes
    /// the file.  The registry's derived lookups are rebuilt separately,
    /// deferred so a bulk build rebuilds once rather than per file.
    pub(crate) fn set_file(&mut self, uri: String, contribution: C) {
        if contribution.is_empty() {
            self.by_uri.remove(&uri);
        } else {
            self.by_uri.insert(uri, contribution);
        }
    }

    /// Whether `uri` currently contributes anything.
    pub(crate) fn has_uri(&self, uri: &str) -> bool {
        self.by_uri.contains_key(uri)
    }

    /// Every contribution with the URI of the file it came from, in no
    /// particular order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &C)> {
        self.by_uri.iter()
    }

    /// Every contribution, in no particular order.
    pub(crate) fn values(&self) -> impl Iterator<Item = &C> {
        self.by_uri.values()
    }

    /// Every contribution in URI order, for a rebuild whose first-wins
    /// merge must not flip on hash-iteration order alone.
    pub(crate) fn iter_sorted(&self) -> Vec<(&String, &C)> {
        let mut entries: Vec<(&String, &C)> = self.by_uri.iter().collect();
        entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
        entries
    }
}
