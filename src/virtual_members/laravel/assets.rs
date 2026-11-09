//! The files Laravel's asset helpers and Inertia's renders name: a path
//! under `public/` (`asset('css/app.css')`), a key of Laravel Mix's
//! `public/mix-manifest.json` (`mix('js/app.js')`), a Vite entry point
//! relative to the project root (`@vite('resources/js/app.js')`), and an
//! Inertia page under the configured page directories
//! (`Inertia::render('Users/Index')`).
//!
//! Every answer is read from the filesystem when it is asked for rather than
//! enumerated once and cached: none of these files is PHP, so no watched-file
//! notification says when a build adds one or a page is created, and a stat
//! per name is cheaper than keeping a listing of `public/` current.  Only the
//! Inertia page configuration is cached, since it is read from a config file
//! whose edits do invalidate it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{Location, Position};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::document_links::normalize_path;
use crate::symbol_map::StringKeyKind;
use crate::util::strip_fqn_prefix;

/// The page extensions `inertia-laravel` looks a page up with when
/// `config/inertia.php` names none.
const DEFAULT_PAGE_EXTENSIONS: [&str; 6] = ["js", "jsx", "svelte", "ts", "tsx", "vue"];

/// The page directories Inertia's starter kits use: the documented
/// `resources/js/Pages`, and the lowercase spelling the newer kits ship.
const DEFAULT_PAGE_DIRECTORIES: [&str; 2] = ["resources/js/Pages", "resources/js/pages"];

/// The configuration file names Vite looks for in the project root.
const VITE_CONFIG_FILES: [&str; 6] = [
    "vite.config.js",
    "vite.config.ts",
    "vite.config.mjs",
    "vite.config.mts",
    "vite.config.cjs",
    "vite.config.cts",
];

/// How many page files one walk of the page directories reads, so a page
/// directory pointed at something huge by mistake cannot stall completion.
const MAX_PAGE_FILES: usize = 10_000;

/// Where Inertia looks for the pages a render names, as `config/inertia.php`
/// configures it.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct InertiaPages {
    /// The page directories, configured or conventional.
    pub roots: Vec<PathBuf>,
    /// The extensions a page file may have, without the leading dot.
    pub extensions: Vec<String>,
}

impl InertiaPages {
    /// The page directories that exist.
    fn existing_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.roots.iter().filter(|root| root.is_dir())
    }

    /// The first file the page `name` resolves to.
    fn page_file(&self, name: &str) -> Option<PathBuf> {
        let name = page_name(name)?;
        self.existing_roots().find_map(|root| {
            self.extensions.iter().find_map(|extension| {
                let candidate = root.join(format!("{name}.{extension}"));
                candidate.is_file().then_some(candidate)
            })
        })
    }

    /// Whether `name` is a page, or `None` when no page directory exists to
    /// tell.
    fn has_page(&self, name: &str) -> Option<bool> {
        self.existing_roots().next()?;
        let name = page_name(name)?;
        Some(self.page_file(name).is_some())
    }

    /// Every page name the page directories hold, sorted.
    pub(crate) fn page_names(&self) -> Vec<String> {
        let mut pages = Vec::new();
        let mut budget = MAX_PAGE_FILES;
        for root in self.existing_roots() {
            collect_pages(root, "", &self.extensions, &mut budget, &mut pages);
        }
        let mut names: Vec<String> = pages.into_iter().map(|(name, _)| name).collect();
        names.sort();
        names.dedup();
        names
    }

    /// The file a new page named `name` belongs in: under the first page
    /// directory that exists (else the first configured), with the
    /// extension most of the existing pages have, or `.vue` in a project
    /// that has none yet.
    pub(crate) fn new_page_file(&self, name: &str) -> Option<PathBuf> {
        let name = page_name(name)?;
        let root = self.existing_roots().next().or(self.roots.first())?;
        let mut pages = Vec::new();
        let mut budget = MAX_PAGE_FILES;
        for root in self.existing_roots() {
            collect_pages(root, "", &self.extensions, &mut budget, &mut pages);
        }
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for (_, extension) in &pages {
            match counts.iter_mut().find(|(known, _)| known == extension) {
                Some((_, count)) => *count += 1,
                None => counts.push((extension, 1)),
            }
        }
        // The first-seen extension wins a tie, so the choice is stable.
        let extension = counts
            .iter()
            .fold(
                None::<(&str, usize)>,
                |best, &(extension, count)| match best {
                    Some((_, best_count)) if best_count >= count => best,
                    _ => Some((extension, count)),
                },
            )
            .map_or("vue", |(extension, _)| extension);
        Some(root.join(format!("{name}.{extension}")))
    }
}

/// A page name with any surrounding separators dropped, or `None` when
/// nothing is left.
fn page_name(name: &str) -> Option<&str> {
    let name = name.trim_matches('/');
    (!name.is_empty()).then_some(name)
}

/// Collect every page under `directory` as its name and extension.
fn collect_pages(
    directory: &Path,
    prefix: &str,
    extensions: &[String],
    budget: &mut usize,
    pages: &mut Vec<(String, String)>,
) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_pages(
                &entry.path(),
                &format!("{prefix}{name}/"),
                extensions,
                budget,
                pages,
            );
            continue;
        }
        *budget -= 1;
        if let Some((stem, extension)) = name.rsplit_once('.')
            && !stem.is_empty()
            && extensions.iter().any(|known| known == extension)
        {
            pages.push((format!("{prefix}{stem}"), extension.to_string()));
        }
    }
}

/// Read the page directories and extensions from `config/inertia.php`'s
/// contents, falling back to Inertia's conventions for whichever it does not
/// configure.
///
/// The keys are read the way the Laravel extension reads them:
/// `pages.paths` (inertia-laravel 2), then `page_paths`, then the
/// `testing.page_paths` older releases nest them under, and likewise for the
/// extensions.
pub(crate) fn inertia_pages_from_config(config: Option<&str>, root: &Path) -> InertiaPages {
    let (mut roots, mut extensions) = config
        .map(|content| read_inertia_config(content, root))
        .unwrap_or_default();
    if roots.is_empty() {
        roots = DEFAULT_PAGE_DIRECTORIES
            .iter()
            .map(|directory| root.join(directory))
            .collect();
    }
    if extensions.is_empty() {
        extensions = DEFAULT_PAGE_EXTENSIONS
            .iter()
            .map(|extension| (*extension).to_string())
            .collect();
    }
    InertiaPages { roots, extensions }
}

fn read_inertia_config(content: &str, root: &Path) -> (Vec<PathBuf>, Vec<String>) {
    crate::parser::with_parsed_program(content, "read_inertia_config", |program, content| {
        // Indexed by precedence: the first slot a key fills wins.
        let mut paths: [Option<Vec<PathBuf>>; 3] = Default::default();
        let mut extensions: [Option<Vec<String>>; 3] = Default::default();
        for expr in super::array_file::returned_exprs(program) {
            super::array_file::for_each_entry(expr, content, &mut |path, _, _, value| {
                let keys: Vec<&str> = path.iter().map(String::as_str).collect();
                let rank = match keys.as_slice() {
                    ["pages", "paths" | "extensions"] => 0,
                    ["page_paths" | "page_extensions"] => 1,
                    ["testing", "page_paths" | "page_extensions"] => 2,
                    _ => return,
                };
                let Some(elements) = crate::parser::array_literal_elements(value) else {
                    return;
                };
                let values = elements.iter().filter_map(|element| match element {
                    ArrayElement::Value(element) => Some(element.value),
                    _ => None,
                });
                if keys.last().is_some_and(|key| key.ends_with("extensions")) {
                    extensions[rank] = Some(
                        values
                            .filter_map(|value| {
                                let (text, _, _) =
                                    super::helpers::resolved_string_literal(value, content)?;
                                let text = text.trim_start_matches('.');
                                (!text.is_empty()).then(|| text.to_string())
                            })
                            .collect(),
                    );
                } else {
                    paths[rank] = Some(
                        values
                            .filter_map(|value| config_path(value, content, root))
                            .collect(),
                    );
                }
            });
        }
        fn first<T>(slots: [Option<Vec<T>>; 3]) -> Vec<T> {
            slots
                .into_iter()
                .flatten()
                .find(|values| !values.is_empty())
                .unwrap_or_default()
        }
        (first(paths), first(extensions))
    })
}

/// The directory a config value names: a string literal, a path helper
/// called with one (`resource_path('js/Pages')`), or either wrapped in
/// `realpath()`.
fn config_path(expr: &Expression<'_>, content: &str, root: &Path) -> Option<PathBuf> {
    if let Some((text, _, _)) = super::helpers::resolved_string_literal(expr, content) {
        let path = Path::new(text);
        return Some(if path.is_absolute() {
            path.to_path_buf()
        } else {
            normalize_path(&root.join(path))
        });
    }
    let Expression::Call(Call::Function(call)) = expr else {
        return None;
    };
    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    let name = strip_fqn_prefix(bytes_to_str(ident.value()));
    let argument = call.argument_list.arguments.iter().next();
    if name.eq_ignore_ascii_case("realpath") {
        return config_path(argument?.value(), content, root);
    }
    let base = super::path_helper_base(root, name)?;
    let Some(argument) = argument else {
        return Some(base);
    };
    let (text, _, _) = super::helpers::resolved_string_literal(argument.value(), content)?;
    Some(normalize_path(
        &base.join(text.trim_start_matches(['/', '\\'])),
    ))
}

/// The path under `public/` an `asset()` argument names, without the query
/// string or fragment a URL may carry, or `None` when the argument is a URL
/// of its own or names the root.
fn public_relative_path(key: &str) -> Option<&str> {
    if key.contains("://") || key.starts_with("//") {
        return None;
    }
    let path = key.split(['?', '#']).next()?.trim_start_matches('/');
    (!path.is_empty()).then_some(path)
}

/// The key `mix()` looks an argument up under: Laravel adds the leading
/// slash the manifest's keys carry.
fn mix_manifest_key(key: &str) -> std::borrow::Cow<'_, str> {
    if key.starts_with('/') {
        std::borrow::Cow::Borrowed(key)
    } else {
        std::borrow::Cow::Owned(format!("/{key}"))
    }
}

/// The entries of `public/mix-manifest.json`, keyed as Mix writes them.
pub(crate) struct MixManifest {
    path: PathBuf,
    entries: serde_json::Map<String, serde_json::Value>,
}

impl MixManifest {
    /// Read the project's manifest, if it has one that parses.
    fn read(root: &Path) -> Option<Self> {
        let path = root.join("public").join("mix-manifest.json");
        let content = std::fs::read_to_string(&path).ok()?;
        let entries = serde_json::from_str(&content).ok()?;
        Some(Self { path, entries })
    }

    /// The compiled file `key` maps to, relative to `public/`.
    fn compiled_path(&self, key: &str) -> Option<&str> {
        let value = self.entries.get(mix_manifest_key(key).as_ref())?.as_str()?;
        public_relative_path(value)
    }

    fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(mix_manifest_key(key).as_ref())
    }
}

/// What the filesystem says about the asset keys of one diagnostic pass,
/// read once for every span in the file.
#[derive(Default)]
pub(crate) struct AssetWorld {
    root: Option<PathBuf>,
    /// `public/` exists.
    has_public: bool,
    /// The Mix manifest, unless Mix's dev server is running (`public/hot`),
    /// in which case `mix()` never reads it.
    mix: Option<MixManifest>,
    /// The project builds with Vite.
    has_vite: bool,
    inertia: Option<Arc<InertiaPages>>,
}

impl AssetWorld {
    /// Whether the asset `key` of `kind` exists, or `None` when that cannot
    /// be told: the directory it would be in is missing, or it names a
    /// file only the running application puts there.
    pub(crate) fn judge(&self, kind: StringKeyKind, key: &str) -> Option<bool> {
        let root = self.root.as_deref()?;
        match kind {
            StringKeyKind::Asset => {
                if !self.has_public {
                    return None;
                }
                let path = public_relative_path(key)?;
                let public = root.join("public");
                // `public/storage` is the link `storage:link` makes to the
                // files the application stores at runtime, and any other
                // link under `public/` shares a directory the same way.
                let first = path.split('/').next().unwrap_or(path);
                if first == "storage" || public.join(first).is_symlink() {
                    return None;
                }
                Some(normalize_path(&public.join(path)).exists())
            }
            StringKeyKind::MixAsset => Some(self.mix.as_ref()?.contains(key)),
            StringKeyKind::ViteAsset => {
                if !self.has_vite || key.contains("://") {
                    return None;
                }
                let path = key.trim_start_matches('/');
                (!path.is_empty()).then(|| normalize_path(&root.join(path)).is_file())
            }
            StringKeyKind::InertiaPage => self.inertia.as_ref()?.has_page(key),
            _ => None,
        }
    }
}

/// Whether the project at `root` builds with Vite.
fn has_vite_config(root: &Path) -> bool {
    VITE_CONFIG_FILES
        .iter()
        .any(|name| root.join(name).is_file())
}

impl Backend {
    /// Read what the asset keys of `kinds` are judged against.
    pub(crate) fn asset_world(&self, kinds: impl Fn(StringKeyKind) -> bool) -> AssetWorld {
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return AssetWorld::default();
        };
        let public = root.join("public");
        AssetWorld {
            has_public: kinds(StringKeyKind::Asset) && public.is_dir(),
            mix: (kinds(StringKeyKind::MixAsset) && !public.join("hot").exists())
                .then(|| MixManifest::read(&root))
                .flatten(),
            has_vite: kinds(StringKeyKind::ViteAsset) && has_vite_config(&root),
            inertia: kinds(StringKeyKind::InertiaPage).then(|| self.inertia_pages()),
            root: Some(root),
        }
    }

    /// Where Inertia looks for pages, cached until `config/inertia.php`
    /// changes.
    pub(crate) fn inertia_pages(&self) -> Arc<InertiaPages> {
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return Arc::default();
        };
        self.cached_laravel_enumeration(
            &self.laravel.string_key_build_locks.inertia_pages,
            |cache| cache.inertia_pages.clone(),
            |cache, pages| cache.inertia_pages = Some(pages),
            || {
                let config_uri = crate::util::path_to_uri(&root.join("config/inertia.php"));
                let config = self.get_file_content_arc(&config_uri);
                Arc::new(inertia_pages_from_config(
                    config.as_deref().map(String::as_str),
                    &root,
                ))
            },
        )
    }

    /// The directory an asset path of `kind` is typed relative to, for the
    /// kinds completed a directory at a time.
    pub(crate) fn asset_completion_base(&self, kind: StringKeyKind) -> Option<PathBuf> {
        let root = self.workspace.workspace_root.read().clone()?;
        match kind {
            StringKeyKind::Asset => Some(root.join("public")),
            StringKeyKind::ViteAsset => Some(root),
            _ => None,
        }
    }

    /// The keys of the Mix manifest, spelled the way `typed_prefix` starts
    /// them: with the leading slash the manifest writes, or without it, the
    /// way `mix()` is usually called.
    pub(crate) fn mix_manifest_keys(&self, typed_prefix: &str) -> Vec<String> {
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return Vec::new();
        };
        let Some(manifest) = MixManifest::read(&root) else {
            return Vec::new();
        };
        let keep_slash = typed_prefix.starts_with('/');
        let mut keys: Vec<String> = manifest
            .entries
            .keys()
            .map(|key| {
                if keep_slash {
                    key.clone()
                } else {
                    key.trim_start_matches('/').to_string()
                }
            })
            .collect();
        keys.sort();
        keys
    }

    /// The file an asset key of `kind` names, when it exists.
    pub(crate) fn asset_file(&self, kind: StringKeyKind, key: &str) -> Option<PathBuf> {
        let root = self.workspace.workspace_root.read().clone()?;
        let file = match kind {
            StringKeyKind::Asset => {
                normalize_path(&root.join("public").join(public_relative_path(key)?))
            }
            StringKeyKind::MixAsset => {
                let manifest = MixManifest::read(&root)?;
                let compiled = manifest
                    .compiled_path(key)
                    .map(|path| normalize_path(&root.join("public").join(path)));
                match compiled {
                    Some(file) if file.is_file() => file,
                    // An entry whose build output is missing still has its
                    // line in the manifest.
                    _ => manifest.contains(key).then_some(manifest.path)?,
                }
            }
            StringKeyKind::ViteAsset => {
                if key.contains("://") {
                    return None;
                }
                normalize_path(&root.join(key.trim_start_matches('/')))
            }
            StringKeyKind::InertiaPage => self.inertia_pages().page_file(key)?,
            _ => return None,
        };
        file.is_file().then_some(file)
    }
}

/// Resolve an asset key or an Inertia page name to the file it names.
pub(crate) fn resolve_asset_definitions(
    backend: &Backend,
    kind: StringKeyKind,
    key: &str,
) -> Vec<Location> {
    backend
        .asset_file(kind, key)
        .and_then(|file| crate::util::path_to_url(&file))
        .map(|uri| vec![crate::definition::point_location(uri, Position::new(0, 0))])
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "assets_tests.rs"]
mod tests;
