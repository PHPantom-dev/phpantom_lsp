//! Laravel self-scan: the index-building passes that read the project's
//! own source (service providers, macro registrations, morph maps, gates,
//! console commands, published resources, migrations) and the incremental
//! refreshes that re-run one of them when a file the pass depends on
//! changes.
//!
//! The indexes these passes fill are consumed by
//! [`crate::virtual_members::laravel`]; this module is only the scanner.
//! Each index has its own file; what they share is the discovery of the
//! registered service providers, which lives here.

mod commands;
mod date_class;
mod gates;
mod macros;
mod morph_map;
mod provider_resources;
mod schema;
mod storage;

use crate::Backend;
use crate::virtual_members::laravel::file_contributions::{Contribution, FileContributions};

impl Backend {
    /// Collect the FQNs of every Laravel service provider that could register a
    /// macro: those installed vendor packages auto-discover (via
    /// `extra.laravel.providers` in each vendor's `installed.json`) plus those
    /// the app lists in `bootstrap/providers.php` / `config/app.php`.
    fn laravel_provider_fqns(&self) -> Vec<String> {
        self.laravel_providers_with_origin()
            .into_iter()
            .map(|(fqn, _)| fqn)
            .collect()
    }

    /// The same providers, each tagged with how it was registered so a
    /// container key two of them bind can be settled the way the container
    /// settles it.
    ///
    /// A provider reached both ways keeps the origin it was first found under:
    /// the container registers it once, at the first point it is named.
    fn laravel_providers_with_origin(
        &self,
    ) -> Vec<(String, crate::virtual_members::laravel::ProviderOrigin)> {
        use crate::virtual_members::laravel::ProviderOrigin;

        let mut providers: Vec<(String, ProviderOrigin)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut push =
            |providers: &mut Vec<(String, ProviderOrigin)>, fqn: String, origin: ProviderOrigin| {
                if seen.insert(fqn.clone()) {
                    providers.push((fqn, origin));
                }
            };

        for vendor_dir in self.workspace.vendor_dir_paths.lock().iter() {
            let installed = vendor_dir.join("composer").join("installed.json");
            if let Ok(content) = std::fs::read_to_string(&installed) {
                for fqn in crate::virtual_members::laravel::parse_installed_providers(&content) {
                    push(&mut providers, fqn, ProviderOrigin::Package);
                }
            }
        }

        if let Some(root) = self.workspace.workspace_root.read().clone() {
            for rel in ["bootstrap/providers.php", "config/app.php"] {
                let path = root.join(rel);
                let uri = crate::util::path_to_uri(&path);
                let content = self
                    .get_file_content(&uri)
                    .or_else(|| std::fs::read_to_string(&path).ok());
                if let Some(content) = content {
                    for fqn in crate::virtual_members::laravel::parse_provider_class_list(&content)
                    {
                        // The configured list is registered `Illuminate\*`
                        // first, then the auto-discovered packages, then the
                        // rest, which is the order
                        // `registerConfiguredProviders()` partitions it into.
                        let origin = if fqn.starts_with("Illuminate\\") {
                            ProviderOrigin::Framework
                        } else {
                            ProviderOrigin::Application
                        };
                        push(&mut providers, fqn, origin);
                    }
                }
            }
        }

        providers
    }

    /// Scan every registered service provider's file into `files`, one
    /// contribution per file, and report how many files were read.
    ///
    /// A file two providers share is read once, and a scan that registers
    /// nothing leaves no entry.  The caller rebuilds the registry's derived
    /// lookups afterwards, so a bulk build rebuilds once.
    fn scan_providers_into<C: Contribution>(
        &self,
        files: &mut FileContributions<C>,
        mut scan: impl FnMut(&str) -> C,
    ) -> usize {
        let mut scanned = 0usize;
        for fqn in self.laravel_provider_fqns() {
            let Some(uri) = self.resolve_class_uri(&fqn) else {
                continue;
            };
            if files.has_uri(&uri) {
                continue;
            }
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            scanned += 1;
            files.set_file(uri, scan(&content));
        }
        scanned
    }

    /// Resolve a class FQN to the URI of the file that declares it, loading the
    /// class if it is not yet in the FQN → URI index.  Used to locate provider
    /// source files for the macro scan.
    pub(crate) fn resolve_class_uri(&self, fqn: &str) -> Option<String> {
        if let Some(uri) = self.symbols.fqn_uri_index.read().get(fqn).cloned() {
            return Some(uri);
        }
        // Not indexed yet: loading the class populates its FQN → URI entry.
        self.find_or_load_class(fqn);
        self.symbols.fqn_uri_index.read().get(fqn).cloned()
    }

    /// Whether `uri` is one of the app's provider registration files
    /// (`bootstrap/providers.php` / `config/app.php`), whose macro-relevant
    /// references are the provider class list rather than method-body class
    /// references.
    fn is_laravel_provider_list_uri(&self, uri: &str) -> bool {
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return false;
        };
        ["bootstrap/providers.php", "config/app.php"]
            .iter()
            .any(|rel| crate::util::path_to_uri(&root.join(rel)) == uri)
    }

    /// Drop what a file deleted from disk contributed to the Laravel
    /// registries.
    ///
    /// Each registry keys a file's registrations by its URI and replaces
    /// them whenever the file is parsed, which a deleted file never is
    /// again.  Re-running the passes against empty content hands them what
    /// the file now contributes and lets each keep its own downstream
    /// invalidation (evicting the classes a macro attached to, dropping the
    /// storage disk type, marking the pivot index dirty).
    pub(crate) fn forget_laravel_file_contributions(&self, uri: &str) {
        self.refresh_laravel_macros(uri, "");
        self.refresh_laravel_storage_drivers(uri, "");
        self.refresh_laravel_pivots(uri, "");
        self.refresh_laravel_command_index(uri);
        self.refresh_laravel_morph_map(uri, "");
        self.refresh_laravel_gates(uri, "");
        self.forget_laravel_provider_resources(uri);
    }

    fn is_in_vendor_dir(&self, path: &std::path::Path) -> bool {
        self.workspace
            .vendor_dir_paths
            .lock()
            .iter()
            .any(|vendor_dir| path.starts_with(vendor_dir))
    }
}
