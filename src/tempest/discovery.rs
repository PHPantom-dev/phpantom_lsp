//! Lazy discovery of `.view.php` files under Composer's autoload roots.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};

use crate::{Backend, composer};

#[derive(Default)]
pub(crate) struct TempestState {
    catalog: Mutex<Option<Arc<Catalog>>>,
    pub(crate) has_views: std::sync::atomic::AtomicBool,
    pub(super) refresh: Mutex<super::inference::Memo>,
    pub(crate) scopes: RwLock<HashMap<String, Arc<super::inference::Scope>>>,
    /// Files the last refresh read view data from: render callers and
    /// custom View declarations.
    pub(super) sources: RwLock<HashSet<String>>,
}

#[derive(Default)]
pub(super) struct Catalog {
    pub(super) roots: Vec<PathBuf>,
    pub(super) files: Vec<PathBuf>,
    pub(super) components: BTreeMap<String, PathBuf>,
}

/// Match indexed paths while retaining the URI of an unsaved buffer.
pub(super) fn canonical_uri(uri: &str) -> String {
    crate::util::uri_to_path(uri)
        .and_then(|path| dunce::canonicalize(path).ok())
        .map_or_else(|| uri.to_owned(), |path| crate::util::path_to_uri(&path))
}

impl Backend {
    /// Forget discovered paths after a file is created/deleted or Composer
    /// changes. Contents are read through the open-buffer accessor on demand.
    pub(crate) fn invalidate_tempest_views(&self) {
        *self.tempest.catalog.lock() = None;
    }

    pub(super) fn tempest_catalog(&self) -> Arc<Catalog> {
        let mut cached = self.tempest.catalog.lock();
        if let Some(catalog) = cached.as_ref() {
            return Arc::clone(catalog);
        }
        let mut catalog = Catalog::default();
        if let Some(root) = self.workspace.workspace_root.read().clone() {
            let (mappings, vendor) = composer::parse_composer_json(&root);
            let mut roots: Vec<_> = mappings
                .iter()
                .filter(|mapping| !mapping.is_dev)
                .map(|mapping| root.join(&mapping.base_path))
                .collect();
            if roots.is_empty() {
                roots.push(root.clone());
            }
            roots.sort();
            roots.dedup();
            let vendor_root = root.join(&vendor);
            catalog.scan(&roots, Some(&vendor_root));
            if let Some(installed) = composer::read_installed_packages(&root, &vendor) {
                for package in &installed.packages {
                    let Some(package_root) = installed.package_root(package) else {
                        continue;
                    };
                    let (mappings, _) = composer::parse_composer_json(&package_root);
                    let roots: Vec<_> = mappings
                        .iter()
                        .filter(|mapping| !mapping.is_dev)
                        .map(|mapping| package_root.join(&mapping.base_path))
                        .collect();
                    // Project components win over vendor components.
                    catalog.scan(&roots, None);
                }
            }
        }
        catalog.files.sort();
        catalog.files.dedup();
        let catalog = Arc::new(catalog);
        *cached = Some(Arc::clone(&catalog));
        catalog
    }

    /// Resolve paths as Tempest does: as written from the project root,
    /// relative to the caller, then relative to each discovery root.
    pub(crate) fn tempest_view_file(&self, uri: &str, name: &str) -> Option<PathBuf> {
        let caller = crate::util::uri_to_path(uri)?;
        let root = self.workspace.workspace_root.read().clone();
        let path = Path::new(name);
        let direct = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.as_deref().unwrap_or(caller.parent()?).join(path)
        };
        let caller_relative = caller.parent()?.join(path);
        for path in [direct, caller_relative] {
            if self.tempest_path_exists(&path) {
                return Some(dunce::canonicalize(&path).unwrap_or(path));
            }
        }
        self.tempest_catalog().roots.iter().find_map(|root| {
            let path = root.join(name);
            self.tempest_path_exists(&path)
                .then(|| dunce::canonicalize(&path).unwrap_or(path))
        })
    }

    fn tempest_path_exists(&self, path: &Path) -> bool {
        self.open_files
            .read()
            .contains_key(&crate::util::path_to_uri(path))
            || path.is_file()
    }

    pub(super) fn tempest_components(&self) -> BTreeMap<String, PathBuf> {
        let mut components = self.tempest_catalog().components.clone();
        for uri in self
            .open_files
            .read()
            .keys()
            .filter(|uri| super::is_view(uri))
        {
            if let Some(path) = crate::util::uri_to_path(uri)
                && let Some(name) = component_name(&path)
            {
                components.insert(name.to_string(), path);
            }
        }
        components
    }
}

impl Catalog {
    fn scan(&mut self, roots: &[PathBuf], excluded: Option<&Path>) {
        for root in roots {
            let root = dunce::canonicalize(root).unwrap_or_else(|_| root.clone());
            if self.roots.contains(&root) {
                continue;
            }
            self.roots.push(root.clone());
            let excluded = excluded.map(Path::to_path_buf);
            let walker = ignore::WalkBuilder::new(&root)
                .git_ignore(excluded.is_some())
                .git_global(excluded.is_some())
                .git_exclude(excluded.is_some())
                .filter_entry(move |entry| {
                    excluded
                        .as_ref()
                        .is_none_or(|path| !entry.path().starts_with(path))
                })
                .build();
            for entry in walker.flatten() {
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    continue;
                }
                let path = entry.path();
                if !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(super::is_view)
                {
                    continue;
                }
                let path = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                if let Some(name) = component_name(&path) {
                    self.components
                        .entry(name.to_string())
                        .or_insert_with(|| path.clone());
                }
                self.files.push(path);
            }
        }
    }
}

fn component_name(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?.strip_suffix(".view.php")?;
    name.starts_with("x-").then_some(name)
}
