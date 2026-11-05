//! The hooks fired outside the project's own files: by the WordPress core,
//! plugins and themes Composer installs, or, when none of them is core, by
//! the core a list in the binary names.
//!
//! A dependency's files are not in the workspace index, so the places they
//! fire hooks are read from disk once, the way the installed Symfony bundles
//! are (see `symfony::bundles`), and kept until the dependencies change. A
//! package counts by the type Composer gives it (`wordpress-core`,
//! `wordpress-plugin`, …), wherever its installer put it: Bedrock installs
//! core under its web root, not under `vendor/`.
//!
//! A project that has only the stubs has no core to read: the stubs'
//! functions have no bodies. `core_hooks.txt` lists the hooks a release of
//! core fires, read by this same code, so those names still complete.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Backend;
use crate::symbol_map::{StringKeyKind, SymbolKind};

/// The Composer package types WordPress's installers place, whose files
/// fire hooks.
const PACKAGE_TYPES: &[&str] = &[
    "wordpress-core",
    "wordpress-plugin",
    "wordpress-muplugin",
    "wordpress-theme",
    "wordpress-dropin",
];

/// Text a file has to contain to fire a hook, so the files that fire none
/// are not parsed.
const FIRING_CALLS: [&[u8]; 2] = [b"do_action", b"apply_filters"];

/// The hooks a release of core fires, one name per line, as
/// [`hook_firings`] reads them. Regenerate with
/// `WORDPRESS_CORE=<install> cargo test regenerate_core_hooks -- --ignored`.
const CORE_HOOKS: &str = include_str!("core_hooks.txt");

/// The hooks a release of core fires, for a project that has no core of
/// its own to read them from.
pub(crate) fn listed_core_hooks() -> impl Iterator<Item = &'static str> {
    CORE_HOOKS.lines().filter(|line| !line.is_empty())
}

/// One place a dependency fires a hook: its byte range and its name.
pub(crate) struct Firing {
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) key: String,
}

/// The hooks the installed WordPress packages fire, file by file.
#[derive(Default)]
pub(crate) struct DependencyHooks {
    /// Each package's root, as a URI prefix ending in `/`.
    pub(crate) roots: Vec<String>,
    /// The files that fire a hook, sorted by URI.
    pub(crate) files: Vec<(String, Vec<Firing>)>,
    /// Whether one of the packages is core.
    pub(crate) has_core: bool,
}

impl DependencyHooks {
    /// Whether the file at `uri` belongs to one of the packages.
    pub(crate) fn holds(&self, uri: &str) -> bool {
        self.roots.iter().any(|root| uri.starts_with(root.as_str()))
    }
}

/// Every PHP file under `dir`, leaving out tests and JavaScript packages.
fn php_files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some("tests" | "Tests" | "node_modules")
                ) {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|ext| ext == "php") {
                out.push(path);
            }
        }
    }
}

/// The places the PHP source `content` fires a hook, in source order.
pub(crate) fn hook_firings(content: &str) -> Vec<Firing> {
    let bytes = content.as_bytes();
    if !FIRING_CALLS
        .iter()
        .any(|needle| memchr::memmem::find(bytes, needle).is_some())
    {
        return Vec::new();
    }
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(b"hooks.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, bytes);
    let resolver = mago_names::resolver::NameResolver::new(&arena);
    let names = crate::names::OwnedResolvedNames::from_resolved(&resolver.resolve(program));
    let map = crate::symbol_map::extraction::extract_symbol_map_for_file(
        program,
        content,
        &names,
        crate::symbol_map::extraction::Frameworks {
            symfony: false,
            service_container: false,
            drupal: false,
            twig: false,
        },
        None,
        crate::drupal::hooks::DrupalFile::Other,
    );
    map.spans
        .into_iter()
        .filter_map(|span| match span.kind {
            SymbolKind::StringKey {
                kind: StringKeyKind::WordPressHook,
                key,
                is_write: true,
                ..
            } => Some(Firing {
                start: span.start,
                end: span.end,
                key,
            }),
            _ => None,
        })
        .collect()
}

/// Read the hooks the WordPress packages installed in the vendor directory
/// `vendor` fire into `found`.
fn read_vendor_dir(vendor: &Path, found: &mut DependencyHooks, paths: &mut Vec<PathBuf>) {
    let (Some(parent), Some(name)) = (vendor.parent(), vendor.file_name().and_then(|n| n.to_str()))
    else {
        return;
    };
    let Some(installed) = crate::composer::read_installed_packages(parent, name) else {
        return;
    };
    for package in &installed.packages {
        let Some(kind) = package.get("type").and_then(|t| t.as_str()) else {
            continue;
        };
        if !PACKAGE_TYPES.contains(&kind) {
            continue;
        }
        let Some(root) = installed.package_root(package) else {
            continue;
        };
        found.has_core |= kind == "wordpress-core";
        let mut prefix = crate::util::path_to_uri(&root);
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        found.roots.push(prefix);
        php_files_under(&root, paths);
    }
}

impl Backend {
    /// The hooks the installed WordPress packages fire.
    pub(crate) fn wordpress_dependency_hooks(&self) -> Arc<DependencyHooks> {
        let mut known = self.wordpress_hooks.dependencies.lock();
        if let Some(known) = known.as_ref() {
            return Arc::clone(known);
        }
        let vendors = self.workspace.vendor_dir_paths.lock().clone();
        let mut found = DependencyHooks::default();
        let mut paths = Vec::new();
        for vendor in &vendors {
            read_vendor_dir(vendor, &mut found, &mut paths);
        }
        found.roots.sort_unstable();
        found.roots.dedup();
        paths.sort_unstable();
        paths.dedup();
        found.files = paths
            .iter()
            .filter_map(|path| {
                let content = crate::util::read_php_source(path)?;
                let firings = hook_firings(&content);
                (!firings.is_empty()).then(|| (crate::util::path_to_uri(path), firings))
            })
            .collect();
        found.files.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        let found = Arc::new(found);
        *known = Some(Arc::clone(&found));
        found
    }

    /// Forget what the installed WordPress packages fire, once they
    /// changed.
    pub(crate) fn forget_wordpress_dependency_hooks(&self) {
        *self.wordpress_hooks.dependencies.lock() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listed_core_hooks_are_sorted_names() {
        let hooks: Vec<&str> = listed_core_hooks().collect();
        assert!(hooks.contains(&"init"));
        assert!(hooks.contains(&"save_post"));
        assert!(hooks.contains(&"save_post_{$post->post_type}"));
        assert!(hooks.windows(2).all(|pair| pair[0] < pair[1]));
    }

    /// Write `core_hooks.txt` from the WordPress install `WORDPRESS_CORE`
    /// names: every hook its PHP files fire, sorted.
    #[test]
    #[ignore = "regenerates core_hooks.txt from a WordPress install"]
    fn regenerate_core_hooks() {
        let core = PathBuf::from(
            std::env::var("WORDPRESS_CORE").expect("WORDPRESS_CORE names a WordPress install"),
        );
        let mut paths = Vec::new();
        php_files_under(&core, &mut paths);
        let mut names: Vec<String> = paths
            .iter()
            .filter(|path| !path.starts_with(core.join("wp-content")))
            .filter_map(|path| crate::util::read_php_source(path))
            .flat_map(|content| hook_firings(&content))
            .map(|firing| firing.key)
            .collect();
        names.sort_unstable();
        names.dedup();
        let mut out = names.join("\n");
        out.push('\n');
        let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/wordpress/core_hooks.txt");
        std::fs::write(target, out).unwrap();
    }
}
