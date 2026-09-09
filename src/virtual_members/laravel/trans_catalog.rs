//! Shared, lazy translation declarations for editor features.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use tower_lsp::lsp_types::{Location, Range, Url};

use crate::Backend;
use crate::text_position::LineIndex;

use super::provider_resources::ProviderResource;
use super::trans_json::collect_json_trans_declarations;
use super::trans_keys::collect_trans_declarations;

/// A language file shared by all the keys it declares.
pub(crate) struct TranslationFile {
    /// The URI shared by this file's declarations.
    pub uri: Url,
    /// The locale derived from the containing directory or JSON filename.
    pub locale: String,
    /// PHP group name, with its package namespace; JSON files have no group.
    pub group: Option<String>,
}

/// One locale's declaration of a translation key.
pub(crate) struct TranslationEntry {
    /// Index of the declaring file in [`TranslationCatalog::files`].
    pub file: usize,
    /// The key's source range in UTF-16 coordinates.
    pub range: Range,
    /// The literal translation, when its value is statically known.
    pub value: Option<String>,
    /// Whether the key describes an array of translations.
    pub is_group: bool,
}

/// Translation keys, values, and locations parsed once per resource update.
/// Files and key strings are shared across locales instead of duplicating
/// paths for every entry. The ordered maps also keep completion stable.
#[derive(Default)]
pub(crate) struct TranslationCatalog {
    /// The definitions of each key, ordered by locale and file URI.
    pub entries: BTreeMap<String, Vec<TranslationEntry>>,
    /// All readable PHP and JSON language files, including empty groups.
    pub files: Vec<TranslationFile>,
    /// Locales declared by directories or JSON filenames.
    pub locales: BTreeSet<String>,
    roots: Vec<String>,
}

impl TranslationCatalog {
    /// Whether an edit falls below one of the catalog's translation roots.
    pub(crate) fn contains_uri(&self, uri: &str) -> bool {
        self.roots.iter().any(|root| uri.starts_with(root))
    }

    fn insert_file(&mut self, backend: &Backend, path: &Path, locale: &str, namespace: &str) {
        let Ok(uri) = Url::from_file_path(path) else {
            return;
        };
        let Some(content) = backend.get_file_content(uri.as_str()) else {
            return;
        };
        let group = if path.extension().is_some_and(|ext| ext == "php") {
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                return;
            };
            Some(if namespace.is_empty() {
                stem.to_string()
            } else {
                format!("{namespace}::{stem}")
            })
        } else {
            None
        };
        let declarations = match &group {
            Some(group) => collect_trans_declarations(&content, group),
            None => collect_json_trans_declarations(&content),
        };
        let file = self.files.len();
        self.files.push(TranslationFile {
            uri,
            locale: locale.to_string(),
            group,
        });
        let lines = LineIndex::new(&content);
        for declaration in declarations {
            let entry = TranslationEntry {
                file,
                range: Range::new(
                    lines.position(declaration.start),
                    lines.position(declaration.end),
                ),
                value: declaration.value,
                is_group: declaration.is_group,
            };
            let entries = self.entries.entry(declaration.key).or_default();
            // PHP arrays and JSON objects both keep the last duplicate key.
            if let Some(previous) = entries.iter_mut().find(|entry| entry.file == file) {
                *previous = entry;
            } else {
                entries.push(entry);
            }
        }
    }
}

impl Backend {
    /// Read the shared translation catalog, building it only on a cache miss.
    pub(crate) fn cached_translations(&self) -> Arc<TranslationCatalog> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.translations,
            |cache| cache.translations.clone(),
            |cache, translations| cache.translations = Some(translations),
            || Arc::new(self.build_translation_catalog()),
        )
    }

    fn build_translation_catalog(&self) -> TranslationCatalog {
        let mut roots = self.laravel_provider_resources.read().trans_dirs.clone();
        if let Some(root) = self.workspace.workspace_root.read().as_ref() {
            roots.extend(
                ["lang", "resources/lang"].map(|directory| ProviderResource {
                    path: root.join(directory),
                    namespace: String::new(),
                }),
            );
        }
        roots.sort_by(|a, b| (&a.path, &a.namespace).cmp(&(&b.path, &b.namespace)));
        roots.dedup();
        let mut catalog = TranslationCatalog::default();
        for root in roots {
            if let Ok(uri) = Url::from_directory_path(&root.path) {
                catalog.roots.push(uri.to_string());
            }
            let Ok(entries) = std::fs::read_dir(&root.path) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let Some(locale) = path.file_name().and_then(|name| name.to_str()) else {
                        continue;
                    };
                    if locale == "vendor" {
                        continue;
                    }
                    catalog.locales.insert(locale.to_string());
                    let Ok(files) = std::fs::read_dir(&path) else {
                        continue;
                    };
                    for file in files.flatten() {
                        let path = file.path();
                        if path.extension().is_some_and(|ext| ext == "php") {
                            catalog.insert_file(self, &path, locale, &root.namespace);
                        }
                    }
                } else if root.namespace.is_empty()
                    && path.extension().is_some_and(|ext| ext == "json")
                    && let Some(locale) = path.file_stem().and_then(|name| name.to_str())
                {
                    catalog.locales.insert(locale.to_string());
                    catalog.insert_file(self, &path, locale, "");
                }
            }
        }
        for entries in catalog.entries.values_mut() {
            entries.sort_by(|a, b| {
                let a = &catalog.files[a.file];
                let b = &catalog.files[b.file];
                (&a.locale, &a.uri).cmp(&(&b.locale, &b.uri))
            });
        }
        catalog
    }

    /// Resolve actual declarations, or the file of a whole PHP group.
    pub(crate) fn translation_definitions(&self, key: &str) -> Vec<Location> {
        let catalog = self.cached_translations();
        if let Some(entries) = catalog.entries.get(key) {
            entries
                .iter()
                .map(|entry| Location::new(catalog.files[entry.file].uri.clone(), entry.range))
                .collect()
        } else {
            catalog
                .files
                .iter()
                .filter(|file| file.group.as_deref() == Some(key))
                .map(|file| Location::new(file.uri.clone(), Range::default()))
                .collect()
        }
    }
}

#[cfg(test)]
#[path = "trans_catalog_tests.rs"]
mod tests;
