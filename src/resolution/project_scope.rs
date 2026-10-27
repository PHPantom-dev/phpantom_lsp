//! Which Composer project a class lookup answers for.
//!
//! A workspace can hold several Composer projects (a monorepo's
//! subprojects, or projects nested below a root `composer.json`), each
//! with its own `vendor/`.  They all feed one shared class index, but at
//! runtime a file only sees the dependencies of the project that holds
//! it: a class that only `platform/vendor/` installs does not exist for
//! `mobile/`, and two projects installing different versions of one
//! package each load their own copy.
//!
//! The work done for one file (a request, the file's diagnostic pass)
//! runs inside a [`ProjectScopeGuard`] for the project holding that file,
//! and the class loader hides every declaration that lives in another
//! project: in its vendor directory, or anywhere else in its tree.  Only a
//! package the project installed from there, such as a path repository,
//! stays visible.  Files outside every project are visible to all of
//! them.  Work that is not about one file (indexing, reference searches)
//! runs unscoped and sees the shared index as it is.
//!
//! A workspace with fewer than two Composer projects that installed their
//! dependencies has nothing to keep apart, so it has no scopes at all and
//! lookups take exactly the path they always did.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::Arc;

use crate::Backend;
use crate::util::dir_uri_prefix;
use crate::workspace_env::ComposerProjectPaths;

/// The view of the class index a file of one Composer project has.
#[derive(Debug)]
pub(crate) struct ProjectScope {
    /// Distinguishes this project's answers in the per-thread class memo.
    /// Never 0, which stands for "unscoped".
    pub(crate) id: u32,
    /// Marks this project's entries in the resolved-class cache, whose
    /// classes depend on which copy of each ancestor the lookups found.
    /// Starts with a NUL byte like the cache's other key markers.
    pub(crate) cache_tag: String,
    /// URI prefixes of the project's own vendor directory.
    own_vendor: Vec<String>,
    /// URI prefixes of the packages the project installed from outside
    /// its vendor directory.
    linked_packages: Vec<String>,
    /// URI prefixes of every other project's vendor directory.
    foreign_vendor: Vec<String>,
    /// URI prefixes of the project's root directory.
    own_roots: Vec<String>,
    /// URI prefixes of every other project's root directory.
    foreign_roots: Vec<String>,
}

impl ProjectScope {
    /// Whether a declaration in the file `uri` exists for this project.
    pub(crate) fn sees(&self, uri: &str) -> bool {
        if self.owns(uri) || matches_any(&self.linked_packages, uri) {
            return true;
        }
        if matches_any(&self.foreign_vendor, uri) {
            return false;
        }
        // A file belongs to the innermost project holding it, so one below
        // this project's root can still be a nested project's, and one
        // below an enclosing project's root but outside this one is the
        // enclosing project's.
        let own_depth = longest_match(&self.own_roots, uri);
        longest_match(&self.foreign_roots, uri) <= own_depth
    }

    /// The declarations among `uris` this project sees, the ones in its
    /// own vendor directory first.
    pub(crate) fn visible_alternates(&self, mut uris: Vec<String>) -> Vec<String> {
        uris.retain(|uri| self.sees(uri));
        uris.sort_by_key(|uri| !self.owns(uri));
        uris
    }

    /// Whether `uri` lies in this project's own vendor directory.
    pub(crate) fn owns(&self, uri: &str) -> bool {
        matches_any(&self.own_vendor, uri)
    }
}

fn matches_any(prefixes: &[String], uri: &str) -> bool {
    prefixes
        .iter()
        .any(|prefix| uri.starts_with(prefix.as_str()))
}

/// The length of the longest of `prefixes` that `uri` starts with, 0 when
/// none does.
fn longest_match(prefixes: &[String], uri: &str) -> usize {
    prefixes
        .iter()
        .filter(|prefix| uri.starts_with(prefix.as_str()))
        .map(String::len)
        .max()
        .unwrap_or(0)
}

/// The scopes of a workspace's Composer projects.
#[derive(Debug, Default)]
pub(crate) struct ProjectScopes {
    /// Each project's root URI prefixes with its scope, the most deeply
    /// nested root first so a file is claimed by the innermost project.
    projects: Vec<(Vec<String>, Arc<ProjectScope>)>,
}

impl ProjectScopes {
    pub(crate) fn build(projects: &[ComposerProjectPaths]) -> Self {
        let installed: Vec<(usize, &ComposerProjectPaths)> = projects
            .iter()
            .enumerate()
            .filter(|(_, project)| !project.vendor_paths.is_empty())
            .collect();
        if installed.len() < 2 {
            return Self::default();
        }
        let prefixes = |paths: &[PathBuf]| -> Vec<String> {
            paths.iter().map(|p| dir_uri_prefix(p)).collect()
        };
        let roots: Vec<Vec<String>> = installed
            .iter()
            .map(|(_, project)| prefixes(&crate::indexing::path_aliases(&project.root)))
            .collect();
        let mut scopes: Vec<(Vec<String>, Arc<ProjectScope>)> = installed
            .iter()
            .enumerate()
            .map(|(position, &(index, project))| {
                let own_vendor = prefixes(&project.vendor_paths);
                let own_roots = roots[position].clone();
                let others = || {
                    installed
                        .iter()
                        .zip(&roots)
                        .filter(move |((other, _), _)| *other != index)
                };
                let foreign_vendor = others()
                    .flat_map(|((_, other), _)| prefixes(&other.vendor_paths))
                    .filter(|prefix| !own_vendor.contains(prefix))
                    .collect();
                let foreign_roots = others()
                    .flat_map(|(_, other_roots)| other_roots.iter().cloned())
                    .filter(|prefix| !own_roots.contains(prefix))
                    .collect();
                let id = index as u32 + 1;
                let scope = ProjectScope {
                    id,
                    cache_tag: format!("\0project:{id}"),
                    own_vendor,
                    linked_packages: prefixes(&project.linked_packages),
                    foreign_vendor,
                    own_roots: own_roots.clone(),
                    foreign_roots,
                };
                (own_roots, Arc::new(scope))
            })
            .collect();
        scopes.sort_by_key(|(roots, _)| {
            std::cmp::Reverse(roots.iter().map(String::len).max().unwrap_or(0))
        });
        Self { projects: scopes }
    }

    /// The scope of the project holding the file `uri`, or `None` for a
    /// file outside every project.
    pub(crate) fn for_uri(&self, uri: &str) -> Option<Arc<ProjectScope>> {
        self.projects
            .iter()
            .find(|(roots, _)| roots.iter().any(|root| uri.starts_with(root.as_str())))
            .map(|(_, scope)| Arc::clone(scope))
    }
}

thread_local! {
    /// The scope the current file's work runs in.
    static ACTIVE: RefCell<Option<Arc<ProjectScope>>> = const { RefCell::new(None) };
    /// `ACTIVE`'s id, read on every class lookup without touching the
    /// `RefCell` or the `Arc`'s count.
    static ACTIVE_ID: Cell<u32> = const { Cell::new(0) };
}

/// Restores the scope that was active before it on drop, so scopes nest.
///
/// Held only within one synchronous call tree: never across an `.await`.
pub(crate) struct ProjectScopeGuard {
    previous: Option<Arc<ProjectScope>>,
}

impl Drop for ProjectScopeGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        ACTIVE_ID.with(|id| id.set(previous.as_ref().map_or(0, |scope| scope.id)));
        ACTIVE.with(|active| *active.borrow_mut() = previous);
    }
}

/// Make `scope` the active one until the guard drops.
pub(crate) fn enter(scope: Option<Arc<ProjectScope>>) -> ProjectScopeGuard {
    ACTIVE_ID.with(|id| id.set(scope.as_ref().map_or(0, |scope| scope.id)));
    let previous = ACTIVE.with(|active| std::mem::replace(&mut *active.borrow_mut(), scope));
    ProjectScopeGuard { previous }
}

/// The id of the active scope, 0 when none is.
#[inline]
pub(crate) fn active_id() -> u32 {
    ACTIVE_ID.with(Cell::get)
}

/// The active scope, if any.
pub(crate) fn active() -> Option<Arc<ProjectScope>> {
    if active_id() == 0 {
        return None;
    }
    ACTIVE.with(|active| active.borrow().clone())
}

impl Backend {
    /// Run the rest of the caller's work on `uri` in the scope of the
    /// Composer project holding it.
    pub(crate) fn enter_project_scope(&self, uri: &str) -> ProjectScopeGuard {
        enter(self.project_scope_for(uri))
    }

    /// The scope of the Composer project holding `uri`, or `None` when the
    /// workspace has nothing to keep apart or the file is in no project.
    pub(crate) fn project_scope_for(&self, uri: &str) -> Option<Arc<ProjectScope>> {
        self.workspace.project_scopes().for_uri(uri)
    }

    /// The file declaring the class `fqn` as the file whose work is running
    /// sees it: the one the index holds, unless that is another Composer
    /// project's dependency, in which case the project's own copy if it
    /// installed one.
    pub(crate) fn class_declaration_uri(&self, fqn: &str) -> Option<String> {
        let indexed = self.symbols.fqn_uri_index.read().get(fqn).cloned()?;
        let Some(scope) = active() else {
            return Some(indexed);
        };
        if scope.sees(&indexed) {
            return Some(indexed);
        }
        scope
            .visible_alternates(self.symbols.alternate_class_uris(fqn))
            .into_iter()
            .next()
    }
}
