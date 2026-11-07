//! Lazy discovery of `.view.php` files under Composer's autoload roots.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::{Mutex, RwLock};

use crate::{Backend, composer};

#[derive(Default)]
pub(crate) struct TempestState {
    catalog: Mutex<Option<Arc<Catalog>>>,
    configured: Mutex<Option<ConfiguredCatalog>>,
    revision: AtomicU64,
    pub(crate) has_views: std::sync::atomic::AtomicBool,
    pub(super) refresh: Mutex<()>,
    pub(crate) scopes: RwLock<HashMap<String, Arc<super::inference::Scope>>>,
}

#[derive(Clone)]
struct ConfiguredCatalog {
    revision: u64,
    locations: Vec<(super::configuration::Location, Arc<Catalog>)>,
    autoload: Arc<Catalog>,
    configuration: Arc<super::configuration::Configuration>,
    catalog: Arc<Catalog>,
}

#[derive(Clone, Default)]
pub(super) struct Catalog {
    pub(super) roots: Vec<PathBuf>,
    pub(super) files: Vec<PathBuf>,
    pub(super) components: BTreeMap<String, PathBuf>,
    /// Configuration files found while scanning these roots.
    pub(super) config_files: Vec<PathBuf>,
    vendor_components: BTreeSet<String>,
    locations: Vec<super::configuration::Location>,
    skipped: Vec<PathBuf>,
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
        *self.tempest.configured.lock() = None;
        self.tempest.revision.fetch_add(1, Ordering::Relaxed);
    }

    /// PHP edits may change constants or helpers a configuration reads.
    /// Only its resolved values are refreshed; filesystem discovery stays
    /// cached until those values, Composer, or the set of files changes.
    pub(crate) fn note_tempest_php_change(&self, uri: &str) {
        if !super::is_view(uri) && self.tempest.revision.load(Ordering::Relaxed) != 0 {
            self.tempest.revision.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn tempest_catalog(&self) -> Arc<Catalog> {
        let _ = self
            .tempest
            .revision
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed);
        let base = self.tempest_autoload_catalog();
        let revision = self.tempest.revision.load(Ordering::Relaxed);
        let previous = self.tempest.configured.lock().clone();
        if let Some(previous) = &previous
            && previous.revision == revision
            && Arc::ptr_eq(&previous.autoload, &base)
        {
            return Arc::clone(&previous.catalog);
        }
        let mut sources = super::configuration::sources(self, &base);
        let mut locations = Vec::new();
        let (configuration, mut catalog) = loop {
            let configuration = super::configuration::collect(self, &sources);
            let mut catalog = (*base).clone();
            for location in &configuration.locations {
                let cached = locations
                    .iter()
                    .chain(
                        previous
                            .iter()
                            .filter(|old| Arc::ptr_eq(&old.autoload, &base))
                            .flat_map(|old| old.locations.iter()),
                    )
                    .find(|(old, _)| old == location)
                    .map(|(_, catalog)| Arc::clone(catalog));
                let discovered = cached.unwrap_or_else(|| {
                    let mut discovered = Catalog::default();
                    discovered.scan(
                        std::slice::from_ref(&location.path),
                        None,
                        &location.ignore,
                        location.vendor,
                    );
                    Arc::new(discovered)
                });
                catalog.extend(&discovered);
                if !locations.iter().any(|(old, _)| old == location) {
                    locations.push((location.clone(), discovered));
                }
            }
            // A discovered configuration can register another discovery root.
            // The canonical source set makes cycles terminate without a depth limit.
            let next = super::configuration::sources(self, &catalog);
            let before = sources.len();
            sources.extend(next);
            if sources.len() == before {
                break (configuration, catalog);
            }
        };
        catalog.files.retain(|path| {
            !configuration
                .skipped
                .iter()
                .any(|skip| path.starts_with(skip))
        });
        catalog.components.retain(|_, path| {
            !configuration
                .skipped
                .iter()
                .any(|skip| path.starts_with(skip))
        });
        for (name, component) in &configuration.components {
            if self.tempest_path_exists(&component.path) {
                catalog.add_component(name.clone(), component.path.clone(), component.vendor);
                catalog.files.push(component.path.clone());
            }
        }
        catalog.files.sort();
        catalog.files.dedup();
        catalog.skipped = configuration.skipped.clone();
        let catalog = previous
            .filter(|old| Arc::ptr_eq(&old.autoload, &base) && *old.configuration == configuration)
            .map_or_else(|| Arc::new(catalog), |old| old.catalog);
        *self.tempest.configured.lock() = Some(ConfiguredCatalog {
            revision,
            locations,
            autoload: base,
            configuration: Arc::new(configuration),
            catalog: Arc::clone(&catalog),
        });
        catalog
    }

    fn tempest_autoload_catalog(&self) -> Arc<Catalog> {
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
            catalog.scan(&roots, Some(&vendor_root), &[], false);
            for entry in [root.join("tempest"), root.join("public/index.php")] {
                if entry.is_file() {
                    catalog.config_files.push(entry);
                }
            }
            if let Some(installed) = composer::read_installed_packages(&root, &vendor) {
                for tier in 0..3 {
                    for package in &installed.packages {
                        if package_tier(package) != Some(tier) {
                            continue;
                        }
                        let Some(package_root) = installed.package_root(package) else {
                            continue;
                        };
                        let Some(namespaces) = package["autoload"]["psr-4"].as_object() else {
                            continue;
                        };
                        let ignored: Vec<_> = package["extra"]["tempest"]["ignore"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|_| tier == 2)
                            .filter_map(|value| value.as_str())
                            .map(|path| {
                                crate::util::canonical_path_for_comparison(&package_root.join(path))
                            })
                            .collect();
                        for (namespace, paths) in namespaces {
                            let paths = paths
                                .as_array()
                                .map(Vec::as_slice)
                                .unwrap_or_else(|| std::slice::from_ref(paths));
                            for path in paths.iter().filter_map(|path| path.as_str()) {
                                let root = crate::util::canonical_path_for_comparison(
                                    &package_root.join(path),
                                );
                                let vendor = namespace.starts_with("Tempest")
                                    || root.components().any(|part| part.as_os_str() == "vendor");
                                catalog.scan(&[root], None, &ignored, vendor);
                            }
                        }
                    }
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
        let catalog = self.tempest_catalog();
        let mut components = catalog.components.clone();
        let opened: BTreeMap<_, _> = self
            .open_files
            .read()
            .keys()
            .filter(|uri| super::is_view(uri))
            .filter_map(|uri| crate::util::uri_to_path(uri))
            .map(|path| (crate::util::canonical_path_for_comparison(&path), path))
            .collect();
        for path in components.values_mut() {
            if let Some(open) = opened.get(path) {
                *path = open.clone();
            }
        }
        for (path, original) in opened {
            if let Some(vendor) = catalog.discovery_kind(&path)
                && let Some(name) = component_name(&path)
                && (!components.contains_key(name)
                    || (catalog.vendor_components.contains(name) && !vendor))
            {
                components.insert(name.to_string(), original);
            }
        }
        components
    }
}

impl Catalog {
    /// Whether a file belongs to an active discovery root, and its precedence.
    pub(super) fn discovery_kind(&self, path: &Path) -> Option<bool> {
        self.locations
            .iter()
            .filter(|location| {
                path.starts_with(&location.path)
                    && !location
                        .ignore
                        .iter()
                        .chain(self.skipped.iter())
                        .any(|ignore| path.starts_with(ignore))
            })
            .map(|location| location.vendor)
            .min()
    }

    fn extend(&mut self, other: &Self) {
        for root in &other.roots {
            if !self.roots.contains(root) {
                self.roots.push(root.clone());
            }
        }
        self.locations.extend_from_slice(&other.locations);
        self.files.extend_from_slice(&other.files);
        self.config_files.extend_from_slice(&other.config_files);
        for (name, path) in &other.components {
            self.add_component(
                name.clone(),
                path.clone(),
                other.vendor_components.contains(name),
            );
        }
    }

    fn add_component(&mut self, name: String, path: PathBuf, vendor: bool) {
        if !self.components.contains_key(&name)
            || (self.vendor_components.contains(&name) && !vendor)
        {
            if vendor {
                self.vendor_components.insert(name.clone());
            } else {
                self.vendor_components.remove(&name);
            }
            self.components.insert(name, path);
        }
    }

    fn scan(
        &mut self,
        roots: &[PathBuf],
        excluded: Option<&Path>,
        ignored: &[PathBuf],
        vendor: bool,
    ) {
        for root in roots {
            let root = dunce::canonicalize(root).unwrap_or_else(|_| root.clone());
            if self.roots.contains(&root) {
                continue;
            }
            self.roots.push(root.clone());
            let excluded = excluded.map(crate::util::canonical_path_for_comparison);
            self.locations.push(super::configuration::Location {
                path: root.clone(),
                vendor,
                ignore: ignored.iter().cloned().chain(excluded.clone()).collect(),
            });
            let ignored = ignored.to_vec();
            let walker = ignore::WalkBuilder::new(&root)
                .git_ignore(excluded.is_some())
                .git_global(excluded.is_some())
                .git_exclude(excluded.is_some())
                .filter_entry(move |entry| {
                    excluded
                        .as_ref()
                        .is_none_or(|path| !entry.path().starts_with(path))
                        && !ignored.iter().any(|path| entry.path().starts_with(path))
                })
                .build();
            for entry in walker.flatten() {
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    continue;
                }
                let path = entry.path();
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".config.php"))
                {
                    self.config_files.push(path.to_path_buf());
                }
                if !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(super::is_view)
                {
                    continue;
                }
                let path = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                if let Some(name) = component_name(&path) {
                    self.add_component(name.to_string(), path.clone(), vendor);
                }
                self.files.push(path);
            }
        }
    }
}

pub(super) fn component_name(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?.strip_suffix(".view.php")?;
    name.starts_with("x-").then_some(name)
}

/// Tempest scans its own packages, dependants, then explicit opt-ins.
fn package_tier(package: &serde_json::Value) -> Option<u8> {
    if package["name"]
        .as_str()
        .is_some_and(|name| name.starts_with("tempest/"))
    {
        Some(0)
    } else if package["require"]
        .as_object()
        .is_some_and(|require| require.keys().any(|name| name.starts_with("tempest/")))
    {
        Some(1)
    } else if package["extra"]["tempest"]["can-discover"] == true {
        Some(2)
    } else {
        None
    }
}
