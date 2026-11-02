//! Drupal's own declarations, read the way Drupal reads them.
//!
//! Drupal builds a Symfony service container from YAML in Symfony's
//! service format, but finds the files itself: every module, theme, and
//! install profile under the web root declares its services in
//! `<name>.services.yml`, core in `core/core.services.yml`, and the site in
//! `sites/*/services.yml`.  Those are read by the same scan as Symfony's
//! configuration (see [`crate::symfony::resources`]), so service IDs and
//! container parameters work alike in both.  Nothing else Symfony declares
//! (translations, events) is read here, and routes are read from Drupal's
//! own routing files: Drupal declares those its own way.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use parking_lot::RwLock;

pub(crate) mod links;
pub(crate) mod routing;
pub(crate) mod types;

use crate::Backend;
use crate::atom::Atom;
use crate::symbol_map::{StringKeyKind, SymbolKind};

/// The web roots of the workspace's Drupal projects, and the declaration
/// files found under them.
#[derive(Default)]
pub(crate) struct DrupalState {
    web_roots: RwLock<Vec<PathBuf>>,
    /// Core's and contributed extensions' declaration files sit in folders
    /// the workspace's `.gitignore` leaves out, so the walk that finds the
    /// rest of the project's YAML never reaches them.
    declaration_files: RwLock<Vec<PathBuf>>,
}

/// A kind of file Drupal reads declarations from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclarationFile {
    /// `services.yml`, or a file ending in `.services.yml` other than
    /// `default.services.yml`, the template a site's `services.yml` is
    /// copied from.
    Services,
    /// `<extension>.routing.yml`.
    Routing,
    /// `<extension>.links.{menu,task,action,contextual}.yml`.
    Links,
}

/// The kind of declaration file the file name at the end of `path` is, if
/// any.
fn declaration_file_kind(path: &str) -> Option<DeclarationFile> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == "services.yml" || (name.ends_with(".services.yml") && name != "default.services.yml")
    {
        Some(DeclarationFile::Services)
    } else if name.ends_with(".routing.yml") {
        Some(DeclarationFile::Routing)
    } else if [
        ".links.menu.yml",
        ".links.task.yml",
        ".links.action.yml",
        ".links.contextual.yml",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
    {
        Some(DeclarationFile::Links)
    } else {
        None
    }
}

/// The folders of a web root whose YAML Drupal never loads as it stands:
/// core's scaffold sources, which Composer copies into `sites/`, and the
/// sites functional tests generate.
const UNLOADED_DIRS: [&str; 2] = ["core/assets", "sites/simpletest"];

/// Whether `path` lies in one of the [`UNLOADED_DIRS`] of `web_root`.
fn is_unloaded(path: &Path, web_root: &Path) -> bool {
    UNLOADED_DIRS
        .iter()
        .any(|dir| path.starts_with(web_root.join(dir)))
}

/// The folders of a web root that Composer installs and overwrites: core,
/// and the contributed extensions and libraries.
const READ_ONLY_DIRS: [&str; 5] = [
    "core",
    "modules/contrib",
    "themes/contrib",
    "profiles/contrib",
    "libraries",
];

/// Whether a static call of `method` on `class`, a resolved class name,
/// looks a service up by its ID: `\Drupal::service()` or
/// `\Drupal::hasService()`.  Only the global `Drupal` class counts.
pub(crate) fn is_service_lookup(class: &str, method: &str) -> bool {
    class.strip_prefix('\\').unwrap_or(class) == "Drupal"
        && (method.eq_ignore_ascii_case("service") || method.eq_ignore_ascii_case("hasService"))
}

/// Whether a static call of `method` on `class`, a resolved class name,
/// starts an entity query on an entity type ID: `\Drupal::entityQuery()` or
/// `\Drupal::entityQueryAggregate()`.  Only the global `Drupal` class
/// counts.
pub(crate) fn is_entity_query(class: &str, method: &str) -> bool {
    class.strip_prefix('\\').unwrap_or(class) == "Drupal"
        && (method.eq_ignore_ascii_case("entityQuery")
            || method.eq_ignore_ascii_case("entityQueryAggregate"))
}

/// The argument a static call of `method` on `class`, a resolved class
/// name, takes a route name in: the first of `Url::fromRoute()`, the second
/// of `Link::createFromRoute()`.
pub(crate) fn static_route_argument(class: &str, method: &str) -> Option<usize> {
    let class = class.strip_prefix('\\').unwrap_or(class);
    if class.eq_ignore_ascii_case("Drupal\\Core\\Url") && method.eq_ignore_ascii_case("fromRoute") {
        Some(0)
    } else if class.eq_ignore_ascii_case("Drupal\\Core\\Link")
        && method.eq_ignore_ascii_case("createFromRoute")
    {
        Some(1)
    } else {
        None
    }
}

/// Whether `class`, a resolved class name, is `Drupal\Core\Url`, whose
/// constructor takes a route name first.
pub(crate) fn is_url_class(class: &str) -> bool {
    class
        .strip_prefix('\\')
        .unwrap_or(class)
        .eq_ignore_ascii_case("Drupal\\Core\\Url")
}

/// Whether the route name `name` has the shape `pattern`, in which each `*`
/// stands for any run of characters.
pub(crate) fn matches_route_pattern(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// Whether `path` is a file Drupal reads declarations from.
pub(crate) fn is_declaration_file(path: &Path, web_root: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| declaration_file_kind(name).is_some())
        && !is_test_fixture(path, web_root)
        && !is_unloaded(path, web_root)
}

/// Whether `name` has the shape of the route Views builds for a display of
/// a view: `view.<view_id>.<display_id>`, neither ID holding a dot.
pub(crate) fn is_views_route_name(name: &str) -> bool {
    let mut parts = name.split('.');
    parts.next() == Some("view")
        && matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(view), Some(display), None) if !view.is_empty() && !display.is_empty()
        )
}

/// Whether `path` lies under a `tests` folder of the web root: it belongs to
/// a test module, which no real site enables.  Roughly half of core's
/// declaration files are.
fn is_test_fixture(path: &Path, web_root: &Path) -> bool {
    path.strip_prefix(web_root)
        .unwrap_or(path)
        .components()
        .any(|part| part.as_os_str() == "tests")
}

impl Backend {
    /// Whether the workspace holds a Drupal project: one whose web root
    /// was detected; see [`Backend::mark_drupal_project`].
    pub(crate) fn is_drupal_project(&self) -> bool {
        self.workspace.is_drupal.load(Ordering::Acquire)
    }

    /// Whether the project builds a Symfony service container, as Symfony
    /// and Drupal both do: service IDs and container parameters are read in
    /// either.
    pub(crate) fn reads_service_container(&self) -> bool {
        self.is_symfony_project() || self.is_drupal_project()
    }

    /// Forget the web roots and their declaration files, before the
    /// workspace's Composer projects are read again.
    pub(crate) fn clear_drupal_web_roots(&self) {
        self.drupal.web_roots.write().clear();
        self.drupal.declaration_files.write().clear();
    }

    /// Remember the declaration files the Drupal walk found.
    pub(crate) fn note_drupal_declaration_files(&self, files: Vec<PathBuf>) {
        self.drupal.declaration_files.write().extend(files);
    }

    /// The declaration files the Drupal walk found, for the workspace index
    /// to read with the rest of the project's YAML.
    pub(crate) fn drupal_declaration_files(&self) -> Vec<PathBuf> {
        self.drupal.declaration_files.read().clone()
    }

    /// Remember the web root of a Drupal project.
    pub(crate) fn note_drupal_web_root(&self, web_root: &Path) {
        let mut roots = self.drupal.web_roots.write();
        if !roots.iter().any(|root| root == web_root) {
            roots.push(web_root.to_path_buf());
        }
    }

    /// Whether `uri` is code Composer installs and overwrites: a vendor
    /// package, or Drupal core or a contributed extension in the web root.
    /// Rename and move never edit it.
    pub(crate) fn is_installed_package_uri(&self, uri: &str, vendor_prefixes: &[String]) -> bool {
        vendor_prefixes
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
            || self.is_drupal_read_only_uri(uri)
    }

    /// Whether `uri` is a file of Drupal core or of a contributed extension,
    /// which Composer overwrites on its next update: read for what it
    /// declares, but never edited.
    pub(crate) fn is_drupal_read_only_uri(&self, uri: &str) -> bool {
        if !self.is_drupal_project() {
            return false;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return false;
        };
        self.drupal.web_roots.read().iter().any(|root| {
            READ_ONLY_DIRS
                .iter()
                .any(|dir| path.starts_with(root.join(dir)))
        })
    }

    /// The entity class and storage handler of each declaration of the
    /// entity type `id`; a storage of `None` for a declaration that names
    /// none and has no default.
    pub(crate) fn drupal_entity_types(&self, id: &str) -> Vec<(Atom, Option<Atom>)> {
        let mut found: Vec<(Atom, Option<Atom>)> = Vec::new();
        self.visit_declaring_symbol_maps(|_, map| {
            for site in &map.entity_types {
                let at = map.spans.partition_point(|span| span.start < site.start);
                let declares_id = map.spans[at..]
                    .iter()
                    .take_while(|span| span.start == site.start)
                    .any(|span| {
                        matches!(
                            &span.kind,
                            SymbolKind::StringKey {
                                kind: StringKeyKind::DrupalEntityType,
                                key,
                                ..
                            } if key == id
                        )
                    });
                if declares_id
                    && !found
                        .iter()
                        .any(|(class, storage)| *class == site.class && *storage == site.storage)
                {
                    found.push((site.class, site.storage));
                }
            }
        });
        found
    }

    /// The shapes of the route names the project's route builders give the
    /// routes they build from parts, sorted.
    pub(crate) fn drupal_route_patterns(&self) -> Vec<String> {
        let mut patterns = Vec::new();
        self.visit_declaring_symbol_maps(|_, map| {
            patterns.extend(map.drupal_route_patterns.iter().cloned());
        });
        patterns.sort_unstable();
        patterns.dedup();
        patterns
    }

    /// The `Class::method` and `Class` names Drupal's attribute route
    /// discovery gives the project's controller routes, sorted.
    pub(crate) fn drupal_route_aliases(&self) -> Vec<String> {
        let mut aliases = Vec::new();
        self.visit_declaring_symbol_maps(|_, map| {
            aliases.extend(map.drupal_route_aliases.iter().cloned());
        });
        aliases.sort_unstable();
        aliases.dedup();
        aliases
    }

    /// Whether `uri` is test code of a Drupal project: a file under a
    /// `tests` folder of one of its web roots.
    pub(crate) fn is_drupal_test_code(&self, uri: &str) -> bool {
        if !self.is_drupal_project() {
            return false;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return false;
        };
        self.drupal
            .web_roots
            .read()
            .iter()
            .any(|root| path.starts_with(root) && is_test_fixture(&path, root))
    }

    /// Whether `uri` belongs to a test module of a Drupal project and is not
    /// open: what it declares is left out, since no real site enables it.
    pub(crate) fn is_closed_drupal_test_fixture(&self, uri: &str) -> bool {
        if !self.is_drupal_project() {
            return false;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return false;
        };
        self.drupal
            .web_roots
            .read()
            .iter()
            .any(|root| path.starts_with(root) && is_test_fixture(&path, root))
            && !self.open_files.read().contains_key(uri)
    }

    /// Whether `uri` is inside one of the web roots of the workspace's
    /// Drupal projects, where Drupal's own reading of a file wins over
    /// Symfony's.
    pub(crate) fn is_in_drupal_web_root(&self, uri: &str) -> bool {
        self.is_drupal_project()
            && crate::util::uri_to_path(uri).is_some_and(|path| {
                self.drupal
                    .web_roots
                    .read()
                    .iter()
                    .any(|root| path.starts_with(root))
            })
    }

    /// The kind of declaration file `uri` is in a Drupal project: one
    /// inside one of its web roots, outside test modules unless it is open.
    ///
    /// Other YAML files in a Drupal project have a `services:` key with
    /// another meaning, or none at all.
    pub(crate) fn drupal_declaration_file(&self, uri: &str) -> Option<DeclarationFile> {
        if !self.is_drupal_project() {
            return None;
        }
        let kind = declaration_file_kind(uri)?;
        let path = crate::util::uri_to_path(uri)?;
        let (in_fixture, unloaded) = self
            .drupal
            .web_roots
            .read()
            .iter()
            .find(|root| path.starts_with(root))
            .map(|root| (is_test_fixture(&path, root), is_unloaded(&path, root)))?;
        (!unloaded && (!in_fixture || self.open_files.read().contains_key(uri))).then_some(kind)
    }
}

#[cfg(test)]
mod tests;
