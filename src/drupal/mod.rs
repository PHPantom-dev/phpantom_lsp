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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use parking_lot::RwLock;

pub(crate) mod computed_hooks;
pub(crate) mod config;
pub(crate) mod config_keys;
pub(crate) mod elements;
pub(crate) mod hooks;
pub(crate) mod libraries;
pub(crate) mod links;
pub(crate) mod permissions;
pub(crate) mod plugins;
pub(crate) mod routing;
pub(crate) mod service_providers;
pub(crate) mod services;
pub(crate) mod template_variables;
pub(crate) mod theme_hooks;
pub(crate) mod types;
pub(crate) mod upcasting;

use crate::Backend;
use crate::atom::Atom;
use crate::symbol_map::{ReceiverClass, StringKeyKind, SymbolKind};

/// The web roots of the workspace's Drupal projects, and the declaration
/// files found under them.
#[derive(Default)]
pub(crate) struct DrupalState {
    web_roots: RwLock<Vec<PathBuf>>,
    /// Core's and contributed extensions' declaration files sit in folders
    /// the workspace's `.gitignore` leaves out, so the walk that finds the
    /// rest of the project's YAML never reaches them.
    declaration_files: RwLock<Vec<PathBuf>>,
    /// The project's routes by the methods they dispatch to, once read.
    route_index: RwLock<Option<std::sync::Arc<upcasting::RouteIndex>>>,
    /// The PHP files of core and contributed extensions that document or
    /// implement hooks, which the workspace walk never reaches either.
    hook_php_files: RwLock<Vec<PathBuf>>,
    /// The modules, install profiles, and themes of every web root.
    extensions: RwLock<hooks::ExtensionIndex>,
    /// What the hook documentation says, until a declaring file changes.
    hook_index: RwLock<Option<std::sync::Arc<hooks::HookIndex>>>,
    /// The paths of the Twig templates of every web root, outside test
    /// modules, which the workspace walk does not reach in core and
    /// contributed extensions, by the hook name their file name spells
    /// (see [`theme_hooks::template_key`]).
    templates: RwLock<std::collections::HashMap<String, Vec<PathBuf>>>,
    /// The theme hooks the site declares, until a file they come from
    /// changes.
    theme_hook_index: RwLock<Option<std::sync::Arc<theme_hooks::ThemeHookIndex>>>,
    /// The extensions that build libraries in code, until a file that
    /// implements the hook changes.
    library_builders: RwLock<Option<std::sync::Arc<libraries::LibraryBuilders>>>,
    /// The theme hook suggestions (`<hook>__<detail>`) render arrays name,
    /// sorted; see [`template_variables`].
    theme_suggestions: RwLock<Option<std::sync::Arc<std::collections::BTreeSet<String>>>>,
    /// The services each service provider's `alter()` gives another class,
    /// keyed by the provider's URI; see [`service_providers`].
    service_replacements: RwLock<BTreeMap<String, Vec<(Atom, Atom)>>>,
    /// The plugin types and the plugins of each, until a file that declares
    /// a plugin manager or a plugin changes; see [`plugins`].
    plugin_index: RwLock<Option<std::sync::Arc<plugins::PluginIndex>>>,
    /// Counts the changes to files the plugin index reads, so an index read
    /// while one landed is not kept.
    plugin_generation: std::sync::atomic::AtomicU64,
    /// The closed test modules' files that declare plugins, by URI: what
    /// they declare is kept apart from the index's plugins (see
    /// [`plugins::PluginIndex::declares`]).
    plugin_test_sources: RwLock<std::collections::HashSet<String>>,
    /// The default configuration files and configuration schema files of
    /// every extension, and the configuration names read from them.
    config: config::ConfigSources,
    /// The PHP files that may define services and the services files of
    /// test modules, by URI; see [`services`].
    service_definition_files: RwLock<std::collections::HashSet<String>>,
    /// What those files define, once read.
    php_service_ids: RwLock<Option<std::sync::Arc<services::PhpServiceIds>>>,
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
    /// `<extension>.permissions.yml`.
    Permissions,
    /// `<extension>.libraries.yml`.
    Libraries,
    /// An extension's `<extension>.info.yml`.
    Info,
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
    } else if name.ends_with(".permissions.yml") {
        Some(DeclarationFile::Permissions)
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
    } else if libraries::libraries_file_extension(name).is_some() {
        Some(DeclarationFile::Libraries)
    } else if hooks::is_info_file_name(name) {
        Some(DeclarationFile::Info)
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
/// reads a configuration object by its name: `\Drupal::config()`.  Only
/// the global `Drupal` class counts.
pub(crate) fn is_config_lookup(class: &str, method: &str) -> bool {
    class.strip_prefix('\\').unwrap_or(class) == "Drupal" && method.eq_ignore_ascii_case("config")
}

/// Whether `class`, a resolved class name, is `Drupal\Core\Form\ConfigTarget`,
/// whose constructor takes a configuration name first.
pub(crate) fn is_config_target_class(class: &str) -> bool {
    class
        .strip_prefix('\\')
        .unwrap_or(class)
        .eq_ignore_ascii_case("Drupal\\Core\\Form\\ConfigTarget")
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

/// The calls on a typed receiver that take a route's parameters after the
/// route name they take first, with the receiver that confirms them.  Each
/// one's route name is read through the receiver
/// `symbol_map::extraction::symfony_receiver_method` gives it.
const ROUTE_PARAMETER_CALLS: &[(&str, ReceiverClass, &str)] = &[
    (
        "setRedirect",
        ReceiverClass::FormRedirectParameters,
        "route_parameters",
    ),
    (
        "redirect",
        ReceiverClass::RouteRedirectParameters,
        "route_parameters",
    ),
    (
        "generateFromRoute",
        ReceiverClass::UrlGeneratorParameters,
        "parameters",
    ),
    (
        "generate",
        ReceiverClass::UrlGeneratorParameters,
        "parameters",
    ),
    (
        "checkNamedRoute",
        ReceiverClass::AccessCheckParameters,
        "parameters",
    ),
];

/// The receiver that confirms the route parameters a call of `method`
/// passes second, and the name of their argument, when the call takes
/// them.
pub(crate) fn route_parameter_call(method: &str) -> Option<(ReceiverClass, &'static str)> {
    ROUTE_PARAMETER_CALLS
        .iter()
        .find(|(name, _, _)| method.eq_ignore_ascii_case(name))
        .map(|(_, receiver, parameter)| (*receiver, *parameter))
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

/// Whether `path` is a file Drupal reads declarations from: a services,
/// routing, links, permissions, or libraries file, or an extension's
/// `*.info.yml`.
pub(crate) fn is_declaration_file(path: &Path, web_root: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| declaration_file_kind(name).is_some())
        && !is_test_fixture(path, web_root)
        && !is_unloaded(path, web_root)
}

/// Whether `path` is a Twig template a theme hook may be drawn with: an
/// `*.html.twig` outside test modules.
pub(crate) fn is_template_file(path: &Path, web_root: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(theme_hooks::TEMPLATE_SUFFIX))
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

/// Whether `path` is a PHP file that can document or implement hooks: an
/// `*.api.php`, an extension's procedural file, or a class under an
/// extension's `src/Hook`.
pub(crate) fn is_hook_php_file(path: &Path, web_root: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let by_name = [
        ".api.php", ".module", ".install", ".inc", ".profile", ".theme",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
        || name == "theme-settings.php";
    let in_hook_dir = name.ends_with(".php")
        && path
            .components()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair[0].as_os_str() == "src" && pair[1].as_os_str() == "Hook");
    (by_name || in_hook_dir) && !is_test_fixture(path, web_root) && !is_unloaded(path, web_root)
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
        self.drupal.hook_php_files.write().clear();
        self.drupal.templates.write().clear();
        *self.drupal.theme_hook_index.write() = None;
        *self.drupal.library_builders.write() = None;
        *self.drupal.theme_suggestions.write() = None;
        *self.drupal.plugin_index.write() = None;
        self.drupal.plugin_test_sources.write().clear();
        *self.drupal.extensions.write() = hooks::ExtensionIndex::default();
        self.drupal.service_replacements.write().clear();
        self.drupal.service_definition_files.write().clear();
        *self.drupal.php_service_ids.write() = None;
        self.drupal.config.clear();
    }

    /// Remember the declaration files the Drupal walk found: the YAML for
    /// the workspace index, the extensions their `*.info.yml` declare, and
    /// the PHP files that document or implement hooks.
    pub(crate) fn note_drupal_declaration_files(&self, files: Vec<PathBuf>) {
        let mut yaml = Vec::new();
        let mut php = Vec::new();
        let mut extensions = Vec::new();
        for file in files {
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if hooks::is_info_file_name(name) {
                if let Some(extension) = hooks::read_extension(&file) {
                    // A theme's info file attaches libraries.
                    if extension.is_theme {
                        yaml.push(file);
                    }
                    extensions.push(extension);
                }
            } else if name.ends_with(".yml") {
                yaml.push(file);
            } else {
                php.push(file);
            }
        }
        self.drupal.declaration_files.write().extend(yaml);
        self.drupal.hook_php_files.write().extend(php);
        self.note_drupal_extensions(extensions);
    }

    /// The PHP files of core and contributed extensions that document or
    /// implement hooks, for the workspace index to parse.
    pub(crate) fn drupal_hook_php_files(&self) -> Vec<PathBuf> {
        self.drupal.hook_php_files.read().clone()
    }

    /// Whether `path` lies under a `tests` folder of one of the web roots.
    pub(crate) fn is_drupal_test_path(&self, path: &Path) -> bool {
        self.drupal
            .web_roots
            .read()
            .iter()
            .any(|root| path.starts_with(root) && is_test_fixture(path, root))
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

    /// The paths of the routes the project names `name`, sorted.
    pub(crate) fn drupal_route_paths(&self, name: &str) -> Vec<String> {
        let mut paths = Vec::new();
        self.visit_declaring_symbol_maps(|_, map| {
            paths.extend(
                map.drupal_routes
                    .iter()
                    .filter(|route| route.name == name)
                    .map(|route| route.path.clone()),
            );
        });
        paths.sort_unstable();
        paths.dedup();
        paths
    }

    /// Whether `uri` is code a Drupal site never runs against its own
    /// declarations: a file under a `tests` folder of one of its web roots,
    /// whose route names are a test module's or invented for a mock, or an
    /// extension's `*.api.php`, which documents hooks with example code.
    pub(crate) fn is_drupal_example_or_test_code(&self, uri: &str) -> bool {
        if !self.is_drupal_project() {
            return false;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return false;
        };
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".api.php"))
            || self.is_drupal_test_path(&path)
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
