//! Drupal's configuration names: the configuration objects a site's
//! extensions ship default configuration for, and the ones their
//! configuration schema describes.
//!
//! An extension's default configuration is one file per configuration
//! object under `config/install` or `config/optional`, named after the
//! object.  The files are never read: their names are the names.  Core
//! alone ships hundreds of them.
//!
//! The configuration schema, `config/schema/*.schema.yml`, describes each
//! configuration object under its name, with a `type` of `config_object` or
//! of a type built on it, next to reusable data types and the configuration
//! entities' wildcard entries (`node.type.*`).  It is read when a name is
//! first asked for, and kept until a schema file or the list of default
//! configuration files changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;

/// The schema type every configuration object is built on.
const CONFIG_OBJECT: &str = "config_object";

/// Whether `path` is a default configuration file of an extension under
/// `web_root`: a `*.yml` right inside a `config/install` or
/// `config/optional` folder.
pub(crate) fn is_default_config_file(path: &Path, web_root: &Path) -> bool {
    has_default_config_shape(path) && is_loaded(path, web_root)
}

/// Whether `path` is a `*.yml` right inside a `config/install` or
/// `config/optional` folder.
fn has_default_config_shape(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "yml")
        && path
            .parent()
            .and_then(|folder| {
                let name = folder.file_name()?;
                (name == "install" || name == "optional").then_some(folder)
            })
            .and_then(Path::parent)
            .is_some_and(|config| config.file_name().is_some_and(|name| name == "config"))
}

/// Whether `path` is a configuration schema file of an extension, or of
/// core, under `web_root`: a `*.schema.yml` right inside a `config/schema`
/// folder.
pub(crate) fn is_schema_file(path: &Path, web_root: &Path) -> bool {
    has_schema_shape(path) && is_loaded(path, web_root)
}

/// Whether `path` is a `*.schema.yml` right inside a `config/schema`
/// folder.
fn has_schema_shape(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".schema.yml"))
        && path
            .parent()
            .filter(|folder| folder.file_name().is_some_and(|name| name == "schema"))
            .and_then(Path::parent)
            .is_some_and(|config| config.file_name().is_some_and(|name| name == "config"))
}

/// Whether `class`, or a class it extends, uses core's
/// `ConfigFormBaseTrait`, whose `config()` reads a configuration object as
/// a config form's does.  A trait is no type, so the subtype checks that
/// find the `config()` of controllers and forms do not find it.
pub(crate) fn uses_config_form_trait(
    class: &crate::types::ClassInfo,
    class_loader: &crate::class_lookup::ResolvedClassLoader<'_>,
) -> bool {
    const CONFIG_FORM_TRAIT: &str = "Drupal\\Core\\Form\\ConfigFormBaseTrait";
    let uses = |class: &crate::types::ClassInfo| {
        class.used_traits.iter().any(|used| {
            used.trim_start_matches('\\')
                .eq_ignore_ascii_case(CONFIG_FORM_TRAIT)
        })
    };
    uses(class)
        || crate::inheritance::ancestors(class, class_loader).any(|(_, parent)| uses(&parent))
}

/// Whether Drupal reads `path`: it is not a test module's, and not in a
/// folder Drupal never loads.
fn is_loaded(path: &Path, web_root: &Path) -> bool {
    !super::is_test_fixture(path, web_root) && !super::is_unloaded(path, web_root)
}

/// The configuration name a default configuration file ships.
fn default_config_name(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()?.strip_suffix(".yml")
}

/// The names to look a configuration name's schema up under, in the order
/// Drupal's `TypedConfigManager::getFallbackName()` tries them: the name
/// itself, then the name with its last part that is not a wildcard
/// replaced by `*`, each followed by the same with its trailing wildcards
/// collapsed into one (`rest.resource.entity.node`, `rest.resource.entity.*`,
/// `rest.resource.*.*`, `rest.resource.*`, …).
pub(crate) fn schema_lookup_names(name: &str) -> impl Iterator<Item = String> {
    static LAST_PART: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"([^.:]+)([.:*]*)$").expect("valid"));
    static TRAILING: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\.([:.*]*)$").expect("valid"));
    let mut names = vec![name.to_string()];
    let mut current = name.to_string();
    loop {
        let replaced = LAST_PART.replace(&current, "*${2}").into_owned();
        if replaced == current {
            break;
        }
        let one_star = TRAILING.replace(&replaced, ".*").into_owned();
        names.push(replaced.clone());
        if one_star != replaced {
            names.push(one_star);
        }
        current = replaced;
    }
    names.into_iter()
}

/// One top-level entry of a schema file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaEntry {
    pub uri: String,
    /// The byte range of the entry's key.
    pub start: u32,
    pub end: u32,
    /// The `type` it gives.
    pub ty: Option<String>,
    pub label: Option<String>,
}

/// The configuration names of a site, and where each is declared.
#[derive(Debug, Default)]
pub(crate) struct ConfigIndex {
    /// Every configuration name, sorted.
    names: Vec<String>,
    /// The default configuration files of each name.
    defaults: HashMap<String, Vec<PathBuf>>,
    /// Each top-level schema entry, by its key, the first one read kept.
    schema: HashMap<String, SchemaEntry>,
}

impl ConfigIndex {
    /// The index of the default configuration files `defaults` and the
    /// schema files `schemas`, each with its URI and content.
    pub(crate) fn build<'a>(
        defaults: &[PathBuf],
        schemas: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        let mut index = Self::default();
        for (uri, content) in schemas {
            for entry in schema_entries(uri, content) {
                index.schema.entry(entry.0).or_insert(entry.1);
            }
        }
        for path in defaults {
            if let Some(name) = default_config_name(path) {
                index
                    .defaults
                    .entry(name.to_string())
                    .or_default()
                    .push(path.clone());
            }
        }
        let mut names: Vec<String> = index
            .schema
            .iter()
            .filter(|(key, entry)| {
                is_config_name(key) && index.derives_from_config_object(entry.ty.as_deref())
            })
            .map(|(key, _)| key.clone())
            .chain(index.defaults.keys().cloned())
            .collect();
        names.sort_unstable();
        names.dedup();
        index.names = names;
        index
    }

    /// Every configuration name, sorted.
    pub(crate) fn names(&self) -> &[String] {
        &self.names
    }

    /// The default configuration files that ship `name`.
    pub(crate) fn default_files(&self, name: &str) -> &[PathBuf] {
        self.defaults.get(name).map_or(&[], Vec::as_slice)
    }

    /// The schema entry that describes `name`, and the key it is under:
    /// its own, or the first wildcard entry that matches it.
    pub(crate) fn schema_entry(&self, name: &str) -> Option<(String, &SchemaEntry)> {
        schema_lookup_names(name).find_map(|key| {
            let entry = self.schema.get(&key)?;
            Some((key, entry))
        })
    }

    /// Whether the schema type `ty` is `config_object` or built on it,
    /// following named types through the schema.
    fn derives_from_config_object(&self, ty: Option<&str>) -> bool {
        let mut seen: Vec<&str> = Vec::new();
        let mut current = ty;
        while let Some(ty) = current {
            if ty == CONFIG_OBJECT {
                return true;
            }
            // A placeholder (`[%parent.type]`) is only known per value.
            if ty.contains(['[', '%']) || seen.contains(&ty) {
                return false;
            }
            seen.push(ty);
            current = self.schema.get(ty).and_then(|entry| entry.ty.as_deref());
        }
        false
    }
}

/// Whether a schema key can be a configuration name: Drupal requires a
/// namespace (a `.`), and a `*` stands for a configuration entity's
/// names.
fn is_config_name(key: &str) -> bool {
    key.contains('.') && !key.contains(['*', ':', '?', '<', '>', '\'', '"', '/', '\\'])
}

/// The top-level entries of a schema file, each with its key.
fn schema_entries(uri: &str, content: &str) -> Vec<(String, SchemaEntry)> {
    let mut entries: Vec<(String, SchemaEntry)> = Vec::new();
    // The indentation of the current entry's settings, once seen.
    let mut child_indent: Option<usize> = None;
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = crate::symfony::resources::yaml_content_before_comment(line);
        let indent = crate::framework::leading_spaces(semantic);
        let text = &semantic[indent..];
        if text.trim().is_empty() || text.starts_with('-') {
            continue;
        }
        let Some(colon) = yaml_key_end(text) else {
            continue;
        };
        let (key, (quote, _)) = crate::framework::strip_yaml_quotes(text[..colon].trim_end());
        if indent == 0 {
            child_indent = None;
            let start = (line_start + quote) as u32;
            entries.push((
                key.to_string(),
                SchemaEntry {
                    uri: uri.to_string(),
                    start,
                    end: start + key.len() as u32,
                    ty: None,
                    label: None,
                },
            ));
            continue;
        }
        let Some((_, entry)) = entries.last_mut() else {
            continue;
        };
        if *child_indent.get_or_insert(indent) != indent {
            continue;
        }
        let value = crate::framework::strip_yaml_quotes(text[colon + 1..].trim())
            .0
            .to_string();
        match key {
            "type" if !value.is_empty() => entry.ty = Some(value),
            "label" if !value.is_empty() => entry.label = Some(value),
            _ => {}
        }
    }
    entries
}

/// Where the key of a YAML `key: value` line ends: at the first `:`
/// followed by a space or the end of the line, outside quotes.  A key such
/// as `block.settings.system_menu_block:*` holds a `:` of its own.
fn yaml_key_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut quote = None;
    for (at, byte) in bytes.iter().enumerate() {
        match (quote, byte) {
            (None, b'\'' | b'"') if at == 0 => quote = Some(*byte),
            (Some(open), _) if *byte == open => quote = None,
            (None, b':') if bytes.get(at + 1).is_none_or(|next| *next == b' ') => {
                return Some(at);
            }
            _ => {}
        }
    }
    None
}

impl Backend {
    /// Remember the default configuration files and schema files the
    /// Drupal walk found.
    pub(crate) fn note_drupal_config_files(&self, defaults: Vec<PathBuf>, schemas: Vec<PathBuf>) {
        self.drupal.default_config_files.write().extend(defaults);
        self.drupal.config_schema_files.write().extend(schemas);
        *self.drupal.config_index.write() = None;
    }

    /// The configuration names of the site, and where each is declared,
    /// read on first use.
    pub(crate) fn drupal_config_index(&self) -> Arc<ConfigIndex> {
        if let Some(index) = self.drupal.config_index.read().as_ref() {
            return Arc::clone(index);
        }
        let mut defaults = self.drupal.default_config_files.read().clone();
        let mut schema_files = self.drupal.config_schema_files.read().clone();
        // A test module's files count while they are open, as its other
        // declaration files do.
        for (path, is_schema) in self.open_drupal_test_config_files() {
            let files = if is_schema {
                &mut schema_files
            } else {
                &mut defaults
            };
            if !files.contains(&path) {
                files.push(path);
            }
        }
        let schemas: Vec<(String, Arc<String>)> = schema_files
            .iter()
            .filter_map(|path| {
                let uri = crate::util::path_to_uri(path);
                let content = self.get_file_content_arc(&uri)?;
                Some((uri, content))
            })
            .collect();
        let index = Arc::new(ConfigIndex::build(
            &defaults,
            schemas
                .iter()
                .map(|(uri, content)| (uri.as_str(), content.as_str())),
        ));
        *self.drupal.config_index.write() = Some(Arc::clone(&index));
        index
    }

    /// The open default configuration and schema files of test modules,
    /// each with whether it is a schema file.
    fn open_drupal_test_config_files(&self) -> Vec<(PathBuf, bool)> {
        let roots = self.drupal.web_roots.read().clone();
        self.open_files
            .read()
            .keys()
            .filter(|uri| uri.ends_with(".yml") && uri.contains("/config/"))
            .filter_map(|uri| crate::util::uri_to_path(uri))
            .filter_map(|path| {
                let root = roots.iter().find(|root| path.starts_with(root))?;
                let in_test_module =
                    super::is_test_fixture(&path, root) && !super::is_unloaded(&path, root);
                let is_schema = has_schema_shape(&path);
                (in_test_module && (is_schema || has_default_config_shape(&path)))
                    .then_some((path, is_schema))
            })
            .collect()
    }

    /// Every configuration name of the site, sorted.
    pub(crate) fn drupal_config_names(&self) -> Vec<String> {
        self.drupal_config_index().names().to_vec()
    }

    /// Where the configuration name `name` is declared: its default
    /// configuration files, then the schema entry that describes it.
    pub(crate) fn drupal_config_locations(&self, name: &str) -> Vec<Location> {
        let index = self.drupal_config_index();
        let mut locations: Vec<Location> = index
            .default_files(name)
            .iter()
            .filter_map(|path| {
                Some(Location {
                    uri: crate::util::parse_url(&crate::util::path_to_uri(path))?,
                    range: Range::default(),
                })
            })
            .collect();
        if let Some((_, entry)) = index.schema_entry(name)
            && let (Some(uri), Some(content)) = (
                crate::util::parse_url(&entry.uri),
                self.get_file_content_arc(&entry.uri),
            )
        {
            let lines = crate::text_position::LineIndex::new(&content);
            locations.push(Location {
                uri,
                range: Range {
                    start: lines.position(entry.start as usize),
                    end: lines.position(entry.end as usize),
                },
            });
        }
        locations
    }

    /// The hover text of the configuration name `name`: its schema label,
    /// and the files that declare it.
    pub(crate) fn drupal_config_detail(&self, name: &str) -> String {
        let index = self.drupal_config_index();
        let mut parts = Vec::new();
        let schema = index.schema_entry(name);
        if let Some(label) = schema.as_ref().and_then(|(_, entry)| entry.label.as_ref()) {
            parts.push(format!("**{label}**"));
        }
        for path in index.default_files(name) {
            if let Some(relative) = self.workspace_relative_path(&crate::util::path_to_uri(path)) {
                parts.push(format!("Default configuration in `{relative}`"));
            }
        }
        if let Some((key, entry)) = &schema
            && let Some(relative) = self.workspace_relative_path(&entry.uri)
        {
            parts.push(format!("Schema `{key}` in `{relative}`"));
        }
        if parts.is_empty() {
            return "Not declared in the site's default configuration or schema".to_string();
        }
        parts.join("\n\n")
    }

    /// Keep the configuration files in step with files created, changed,
    /// or deleted on disk: a default configuration file created or deleted
    /// adds or removes its name, and any change to a schema file is read
    /// again.
    pub(crate) fn note_drupal_config_changes(
        &self,
        changes: &[tower_lsp_server::gen_lsp_types::FileEvent],
    ) {
        use tower_lsp_server::gen_lsp_types::FileChangeType;
        let roots = self.drupal.web_roots.read().clone();
        let mut changed = false;
        for change in changes {
            let path_str = change.uri.path();
            if !path_str.ends_with(".yml") || !path_str.contains("/config/") {
                continue;
            }
            let Some(path) = crate::util::url_to_path(&change.uri) else {
                continue;
            };
            let Some(root) = roots.iter().find(|root| path.starts_with(root)) else {
                continue;
            };
            let list = if is_default_config_file(&path, root) {
                if change.kind == FileChangeType::Changed {
                    continue;
                }
                &self.drupal.default_config_files
            } else if is_schema_file(&path, root) {
                &self.drupal.config_schema_files
            } else {
                continue;
            };
            let mut files = list.write();
            files.retain(|known| *known != path);
            if change.kind != FileChangeType::Deleted {
                files.push(path);
            }
            changed = true;
        }
        if changed {
            *self.drupal.config_index.write() = None;
        }
    }

    /// Read a configuration file that was opened, edited, or closed again
    /// the next time a name is asked for: an open schema file is read from
    /// its buffer, and an open test module's files count.
    pub(crate) fn note_drupal_config_document(&self, uri: &str) {
        if !self.is_drupal_project()
            || !uri.ends_with(".yml")
            || !["/config/install/", "/config/optional/", "/config/schema/"]
                .iter()
                .any(|folder| uri.contains(folder))
        {
            return;
        }
        if self.drupal.config_index.read().is_some() {
            *self.drupal.config_index.write() = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = "# Comment: here\nacme.settings:\n  type: config_object\n  label: 'Acme settings'\n  mapping:\n    notify:\n      type: string\nacme_theme_settings:\n  type: config_object\nacme.theme.global:\n  type: acme_theme_settings\nacme.preset.*:\n  type: config_entity\nacme.data:\n  type: mapping\nacme.loop:\n  type: acme.loop_b\nacme.loop_b:\n  type: acme.loop\nacme.menu_block:*:\n  type: config_object\nacme.dynamic:\n  type: acme.[%parent.type]\n";

    fn index(defaults: &[&str]) -> ConfigIndex {
        let defaults: Vec<PathBuf> = defaults.iter().map(PathBuf::from).collect();
        ConfigIndex::build(&defaults, [("file:///acme.schema.yml", SCHEMA)])
    }

    #[test]
    fn names_are_config_objects_and_default_files() {
        let index = index(&[
            "/web/modules/acme/config/install/acme.other.yml",
            "/web/modules/acme/config/optional/acme.settings.yml",
        ]);
        // A data type, a configuration entity's wildcard, a plain mapping,
        // a cycle, a key holding a `:`, and a placeholder are not names.
        assert_eq!(
            index.names(),
            ["acme.other", "acme.settings", "acme.theme.global"]
        );
        assert_eq!(index.default_files("acme.settings").len(), 1);
    }

    #[test]
    fn a_key_holding_a_colon_is_read_whole() {
        let entries = schema_entries("u", SCHEMA);
        assert!(entries.iter().any(|(key, _)| key == "acme.menu_block:*"));
        let (_, settings) = entries
            .iter()
            .find(|(key, _)| key == "acme.settings")
            .unwrap();
        assert_eq!(
            &SCHEMA[settings.start as usize..settings.end as usize],
            "acme.settings"
        );
        assert_eq!(settings.label.as_deref(), Some("Acme settings"));
        assert_eq!(settings.ty.as_deref(), Some("config_object"));
    }

    /// Drupal collapses the trailing wildcards of a candidate that has no
    /// entry into one, for an entry that stands for any depth.
    #[test]
    fn trailing_wildcards_collapse_to_one() {
        let schema = "rest.resource.*:\n  type: config_entity\nrest.resource.entity.special:\n  type: config_entity\n";
        let index = ConfigIndex::build(&[], [("file:///rest.schema.yml", schema)]);
        assert_eq!(
            index
                .schema_entry("rest.resource.entity.node")
                .map(|(key, _)| key),
            Some("rest.resource.*".to_string())
        );
        assert_eq!(
            index
                .schema_entry("rest.resource.entity.special")
                .map(|(key, _)| key),
            Some("rest.resource.entity.special".to_string())
        );
    }

    #[test]
    fn a_schema_is_looked_up_exactly_then_by_wildcards() {
        assert_eq!(
            schema_lookup_names("field.storage.node.body").collect::<Vec<_>>(),
            [
                "field.storage.node.body",
                "field.storage.node.*",
                "field.storage.*.*",
                "field.storage.*",
                "field.*.*.*",
                "field.*",
                "*.*.*.*",
                "*.*",
            ]
        );
        let index = index(&[]);
        assert_eq!(
            index
                .schema_entry("acme.preset.default")
                .map(|(key, _)| key),
            Some("acme.preset.*".to_string())
        );
        assert_eq!(
            index.schema_entry("acme.settings").map(|(key, _)| key),
            Some("acme.settings".to_string())
        );
        assert!(index.schema_entry("other.thing").is_none());
    }

    #[test]
    fn default_and_schema_files_are_found_by_their_folders() {
        let root = Path::new("/site/web");
        let yes = |path: &str| is_default_config_file(Path::new(path), root);
        assert!(yes(
            "/site/web/modules/custom/acme/config/install/acme.settings.yml"
        ));
        assert!(yes(
            "/site/web/core/modules/node/config/optional/views.view.content.yml"
        ));
        assert!(!yes(
            "/site/web/modules/custom/acme/config/install/language/fr/acme.settings.yml"
        ));
        assert!(!yes(
            "/site/web/modules/custom/acme/config/schema/acme.schema.yml"
        ));
        assert!(!yes(
            "/site/web/core/modules/node/tests/modules/node_test/config/install/node_test.settings.yml"
        ));
        let schema = |path: &str| is_schema_file(Path::new(path), root);
        assert!(schema(
            "/site/web/core/config/schema/core.data_types.schema.yml"
        ));
        assert!(schema(
            "/site/web/modules/custom/acme/config/schema/acme.schema.yml"
        ));
        assert!(!schema(
            "/site/web/modules/custom/acme/config/install/acme.schema.yml"
        ));
    }
}
