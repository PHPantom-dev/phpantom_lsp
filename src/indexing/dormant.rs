//! Files of an installed package that the workspace index reads and lets
//! go of again.
//!
//! A package Composer installed outside the vendor directory (Drupal core
//! and contributed extensions under the web root, say) is project code to
//! every feature: its references are found, its hooks reached, its classes
//! resolved.  Keeping its whole parse resident is what made a Drupal site
//! cost over a gigabyte, so the workspace index parses such a file for its
//! declarations and reference-index entries and then drops the rest, which
//! leaves it costing little more than a vendor file.  A search that selects it
//! parses it again ([`Backend::wake_dormant_files`]), and so does opening
//! it in the editor.

use crate::Backend;

impl Backend {
    /// Whether `uri` lies in a package Composer installed outside every
    /// vendor directory.
    pub(crate) fn is_installed_package_file(&self, uri: &str) -> bool {
        self.workspace
            .installed_package_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
    }

    /// Drop what the index keeps resident for each of `uris` that may go
    /// without it: a file of an installed package that is not open and
    /// declares nothing a framework index reads its symbol map for.
    ///
    /// Its declarations stay in the class, function, and constant indexes,
    /// and its entries in the reference index, so lookups and searches
    /// still find it.  The eager pass leaves its classes unresolved until
    /// something asks for them (see `eager_populate_resolved_classes`).
    pub(crate) fn put_to_sleep<'a>(&self, uris: impl IntoIterator<Item = &'a str>) {
        let sleepers: Vec<&str> = uris
            .into_iter()
            .filter(|uri| {
                self.is_installed_package_file(uri)
                    && !self.open_files.read().contains_key(*uri)
                    && !self.is_declaring_uri(uri)
                    && !self.holds_wordpress_hooks(uri)
            })
            .collect();
        if sleepers.is_empty() {
            return;
        }
        {
            let mut files = self.files.write();
            for uri in &sleepers {
                files.remove(*uri);
            }
        }
        let mut dormant = self.dormant_files.write();
        dormant.extend(sleepers.into_iter().map(str::to_string));
    }

    /// Parse each dormant file of `uris` again, so that it has a symbol
    /// map and the rest of its parse for whoever reads it next.
    ///
    /// A woken file stays resident until the index reads it from disk
    /// again.
    pub(crate) fn wake_dormant_files<'a>(&self, uris: impl IntoIterator<Item = &'a str>) {
        let asleep: Vec<(String, std::path::PathBuf)> = {
            let dormant = self.dormant_files.read();
            if dormant.is_empty() {
                return;
            }
            uris.into_iter()
                .filter(|uri| dormant.contains(*uri))
                .filter_map(|uri| Some((uri.to_string(), crate::util::uri_to_path(uri)?)))
                .collect()
        };
        if asleep.is_empty() {
            return;
        }
        // Publishing a file's state takes it out of `dormant_files`.
        self.parse_paths_parallel_awake(&asleep);
    }
}

#[cfg(test)]
#[path = "dormant_tests.rs"]
mod tests;
