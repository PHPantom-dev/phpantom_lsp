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
//! first asked for.  Each schema file is kept as read until its text
//! changes, and only a changed file is read again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use tower_lsp_server::gen_lsp_types::{FileChangeType, Location, Range};

use super::config_keys::{YamlNode, yaml_tree};
use crate::Backend;
use crate::symbol_map::SymbolMap;

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
pub(super) fn has_default_config_shape(path: &Path) -> bool {
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
    pub uri: Arc<str>,
    /// The byte range of the entry's key.
    pub start: u32,
    pub end: u32,
    /// The `type` it gives.
    pub ty: Option<String>,
    pub label: Option<String>,
}

/// One configuration schema file as last read: its top-level entries, the
/// hash of the text they were read from, and the file as a tree once a
/// configuration key is asked for.
#[derive(Debug)]
pub(crate) struct SchemaFile {
    uri: Arc<str>,
    hash: u64,
    entries: Vec<(Arc<str>, Arc<SchemaEntry>)>,
    tree: OnceLock<Arc<Vec<YamlNode>>>,
}

impl SchemaFile {
    fn read(uri: Arc<str>, content: &str) -> Self {
        let entries = schema_entries(&uri, content)
            .into_iter()
            .map(|(key, entry)| (Arc::from(key), Arc::new(entry)))
            .collect();
        Self {
            uri,
            hash: SymbolMap::hash_source(content),
            entries,
            tree: OnceLock::new(),
        }
    }

    /// The file as a tree, read with `read` the first time.  A tree is
    /// kept only when read from the text the entries were: its nodes are
    /// found by where their entries start.
    fn tree(&self, read: impl FnOnce(&str) -> Option<Arc<String>>) -> Option<Arc<Vec<YamlNode>>> {
        if let Some(tree) = self.tree.get() {
            return Some(Arc::clone(tree));
        }
        let content = read(&self.uri)?;
        let tree = Arc::new(yaml_tree(&content));
        if SymbolMap::hash_source(&content) == self.hash {
            let _ = self.tree.set(Arc::clone(&tree));
        }
        Some(tree)
    }
}

/// The configuration names of a site, and where each is declared.
#[derive(Debug, Default)]
pub(crate) struct ConfigIndex {
    /// Every configuration name, sorted.
    names: Vec<String>,
    /// The default configuration files of each name.
    defaults: HashMap<String, Vec<PathBuf>>,
    /// Each top-level schema entry, by its key, the first one read kept.
    schema: HashMap<Arc<str>, Arc<SchemaEntry>>,
    /// The schema files it was read from, by URI.
    files: HashMap<Arc<str>, Arc<SchemaFile>>,
    /// The test modules' files that were open when it was read, which
    /// count only while they are.
    open_test_files: Vec<(PathBuf, bool)>,
}

impl ConfigIndex {
    /// The index of the default configuration files `defaults` and the
    /// schema files `schemas`, each with its URI and content.
    #[cfg(test)]
    pub(crate) fn build<'a>(
        defaults: &[PathBuf],
        schemas: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        let files = schemas
            .into_iter()
            .map(|(uri, content)| Arc::new(SchemaFile::read(Arc::from(uri), content)))
            .collect();
        Self::combine(defaults, files)
    }

    /// The index of the default configuration files `defaults` and the
    /// schema files `files`, in that order.
    fn combine(defaults: &[PathBuf], files: Vec<Arc<SchemaFile>>) -> Self {
        let mut index = Self::default();
        for file in &files {
            for (key, entry) in &file.entries {
                if !index.schema.contains_key(key) {
                    index.schema.insert(Arc::clone(key), Arc::clone(entry));
                }
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
            .map(|(key, _)| key.to_string())
            .chain(index.defaults.keys().cloned())
            .collect();
        names.sort_unstable();
        names.dedup();
        index.names = names;
        index.files = files
            .into_iter()
            .map(|file| (Arc::clone(&file.uri), file))
            .collect();
        index
    }

    /// The schema file at `uri` as a tree, read with `read` the first time
    /// it is asked for.
    pub(crate) fn tree(
        &self,
        uri: &str,
        read: impl FnOnce(&str) -> Option<Arc<String>>,
    ) -> Option<Arc<Vec<YamlNode>>> {
        self.files.get(uri)?.tree(read)
    }

    /// Every configuration name, sorted.
    pub(crate) fn names(&self) -> &[String] {
        &self.names
    }

    /// The schema entry under the key `key` itself.
    pub(crate) fn schema_named(&self, key: &str) -> Option<&SchemaEntry> {
        self.schema.get(key).map(Arc::as_ref)
    }

    /// The default configuration files that ship `name`.
    pub(crate) fn default_files(&self, name: &str) -> &[PathBuf] {
        self.defaults.get(name).map_or(&[], Vec::as_slice)
    }

    /// The schema entry that describes `name`, and the key it is under:
    /// its own, or the first wildcard entry that matches it.
    pub(crate) fn schema_entry(&self, name: &str) -> Option<(String, &SchemaEntry)> {
        schema_lookup_names(name).find_map(|key| {
            let entry = self.schema.get(key.as_str())?;
            Some((key, entry.as_ref()))
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
fn schema_entries(uri: &Arc<str>, content: &str) -> Vec<(String, SchemaEntry)> {
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
                    uri: Arc::clone(uri),
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
pub(super) fn yaml_key_end(text: &str) -> Option<usize> {
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

/// What the configuration index is read from: the default configuration
/// files and schema files the walk found, each schema file as last read,
/// and the index built from them.  A change to one schema file reads that
/// file again and no other.
#[derive(Default)]
pub(crate) struct ConfigSources {
    inner: parking_lot::RwLock<Sources>,
}

#[derive(Default)]
struct Sources {
    defaults: Vec<PathBuf>,
    /// Each schema file with the URI it is read by.
    schemas: Vec<(PathBuf, Arc<str>)>,
    /// The schema files read, by URI, less the ones changed since.
    files: HashMap<Arc<str>, Arc<SchemaFile>>,
    index: Option<Arc<ConfigIndex>>,
    /// Moves on every change to the files or their lists, so that a build
    /// that overlapped one stores no index.
    generation: u64,
    /// The generation each schema file last changed in, so that a build
    /// that overlapped a change keeps the other files it read.
    changed_in: HashMap<Arc<str>, u64>,
    /// The generation the files were last all forgotten in.
    cleared_in: u64,
}

impl Sources {
    fn changed(&mut self) {
        self.index = None;
        self.generation += 1;
    }

    /// Forget the schema file at `uri` as read.
    fn evict(&mut self, uri: &str) {
        self.changed();
        self.files.remove(uri);
        match self.changed_in.get_mut(uri) {
            Some(generation) => *generation = self.generation,
            None => {
                self.changed_in.insert(Arc::from(uri), self.generation);
            }
        }
    }

    fn is_listed(&self, uri: &str) -> bool {
        self.schemas.iter().any(|(_, listed)| &**listed == uri)
    }
}

/// A default configuration file or schema file created, changed, or
/// deleted on disk.
pub(crate) struct ConfigFileEvent {
    pub path: PathBuf,
    pub is_schema: bool,
    pub kind: FileChangeType,
    /// Whether it is open, when its buffer is read rather than the disk.
    pub is_open: bool,
}

impl ConfigSources {
    /// Add the default configuration files and schema files the walk found.
    pub(crate) fn add_files(&self, defaults: Vec<PathBuf>, schemas: Vec<PathBuf>) {
        let schemas: Vec<(PathBuf, Arc<str>)> = schemas
            .into_iter()
            .map(|path| {
                let uri = Arc::from(crate::util::path_to_uri(&path));
                (path, uri)
            })
            .collect();
        let mut sources = self.inner.write();
        sources.defaults.extend(defaults);
        sources.schemas.extend(schemas);
        sources.changed();
    }

    /// Forget every file.
    pub(crate) fn clear(&self) {
        let mut sources = self.inner.write();
        sources.defaults.clear();
        sources.schemas.clear();
        sources.files.clear();
        sources.changed_in.clear();
        sources.changed();
        sources.cleared_in = sources.generation;
    }

    /// The configuration index, built on first use from the files found and
    /// the open test modules' files `open_test_files` gives.  Only the
    /// schema files not read yet, or changed since, are read with `read`.
    pub(crate) fn index(
        &self,
        read: impl Fn(&str) -> Option<Arc<String>>,
        open_test_files: impl FnOnce() -> Vec<(PathBuf, bool)>,
    ) -> Arc<ConfigIndex> {
        // Read before the open files are, so that one opening meanwhile
        // keeps this build from being stored.
        let generation = {
            let sources = self.inner.read();
            if let Some(index) = &sources.index {
                return Arc::clone(index);
            }
            sources.generation
        };
        // A test module's files count while they are open, as its other
        // declaration files do.
        let open_test_files = open_test_files();
        let (defaults, schemas) = {
            let sources = self.inner.read();
            if let Some(index) = &sources.index {
                return Arc::clone(index);
            }
            let mut defaults = sources.defaults.clone();
            let mut schemas: Vec<(Arc<str>, Option<Arc<SchemaFile>>)> = sources
                .schemas
                .iter()
                .map(|(_, uri)| (Arc::clone(uri), sources.files.get(uri).cloned()))
                .collect();
            for (path, is_schema) in &open_test_files {
                if !*is_schema {
                    if !defaults.contains(path) {
                        defaults.push(path.clone());
                    }
                } else if !sources.schemas.iter().any(|(listed, _)| listed == path) {
                    let uri: Arc<str> = Arc::from(crate::util::path_to_uri(path));
                    let read = sources.files.get(&uri).cloned();
                    schemas.push((uri, read));
                }
            }
            (defaults, schemas)
        };
        let mut read_now = Vec::new();
        let files = schemas
            .into_iter()
            .filter_map(|(uri, known)| match known {
                Some(file) => Some(file),
                None => {
                    let content = read(&uri)?;
                    let file = Arc::new(SchemaFile::read(uri, &content));
                    read_now.push(Arc::clone(&file));
                    Some(file)
                }
            })
            .collect();
        let mut index = ConfigIndex::combine(&defaults, files);
        index.open_test_files = open_test_files;
        let index = Arc::new(index);
        let mut sources = self.inner.write();
        if sources.generation == generation {
            if let Some(stored) = &sources.index {
                return Arc::clone(stored);
            }
            sources.files = index.files.clone();
            sources.index = Some(Arc::clone(&index));
        } else if sources.cleared_in <= generation {
            // Keep the files read that did not change meanwhile.
            for file in read_now {
                if sources
                    .changed_in
                    .get(&file.uri)
                    .is_none_or(|changed| *changed <= generation)
                {
                    sources.files.entry(Arc::clone(&file.uri)).or_insert(file);
                }
            }
        }
        index
    }

    /// Follow a document event for the default configuration or schema
    /// file at `uri`, spelled as the walk spells its path, which now holds
    /// `content`, or is gone (`None`): a schema file is read again when its
    /// text changed, and the index is read again when a test module's file
    /// opened or closed, which `open_test_files` tells.
    pub(crate) fn record_document(
        &self,
        uri: &str,
        content: Option<&str>,
        open_test_files: impl FnOnce() -> Vec<(PathBuf, bool)>,
    ) {
        if uri.contains("/config/schema/") {
            let stale = {
                let sources = self.inner.read();
                match sources.files.get(uri) {
                    Some(file) => {
                        content.is_none_or(|content| SymbolMap::hash_source(content) != file.hash)
                    }
                    // A file not read yet matters when a build may be
                    // reading it: a listed one, or a test module's, which
                    // is read while it is open.
                    None => uri.contains("/tests/") || sources.is_listed(uri),
                }
            };
            if stale {
                self.inner.write().evict(uri);
                return;
            }
        }
        // Only a test module's file opening or closing changes which are
        // open.
        if !uri.contains("/tests/") {
            return;
        }
        let Some(index) = self.inner.read().index.clone() else {
            // A build under way may have seen the files open before.
            self.inner.write().changed();
            return;
        };
        if index.open_test_files != open_test_files() {
            self.inner.write().changed();
        }
    }

    /// Follow default configuration files and schema files created,
    /// changed, or deleted on disk.  A default configuration file gives its
    /// name by its path alone, so only its creation or deletion counts.  A
    /// schema file keeps its place in the list unless its last event in the
    /// batch deletes it, which decides the entry kept for a key two files
    /// declare, and an open one follows its buffer, not the disk.
    pub(crate) fn record_watched_changes(
        &self,
        changes: impl IntoIterator<Item = ConfigFileEvent>,
    ) {
        let mut defaults: Vec<ConfigFileEvent> = Vec::new();
        let mut schemas: Vec<ConfigFileEvent> = Vec::new();
        for change in changes {
            if !change.is_schema {
                if change.kind != FileChangeType::Changed {
                    defaults.push(change);
                }
            } else if let Some(last) = schemas.iter_mut().find(|last| last.path == change.path) {
                *last = change;
            } else {
                schemas.push(change);
            }
        }
        if defaults.is_empty() && schemas.is_empty() {
            return;
        }
        let mut sources = self.inner.write();
        for change in defaults {
            sources.defaults.retain(|known| *known != change.path);
            if change.kind != FileChangeType::Deleted {
                sources.defaults.push(change.path);
            }
            sources.changed();
        }
        for change in schemas {
            let uri = crate::util::path_to_uri(&change.path);
            let listed = sources.is_listed(&uri);
            if change.kind == FileChangeType::Deleted {
                sources.schemas.retain(|(known, _)| *known != change.path);
            } else if !listed {
                sources.schemas.push((change.path, Arc::from(uri.as_str())));
            } else if change.kind == FileChangeType::Changed && change.is_open {
                continue;
            }
            sources.evict(&uri);
        }
    }
}

impl Backend {
    /// Remember the default configuration files and schema files the
    /// Drupal walk found.
    pub(crate) fn note_drupal_config_files(&self, defaults: Vec<PathBuf>, schemas: Vec<PathBuf>) {
        self.drupal.config.add_files(defaults, schemas);
    }

    /// The configuration names of the site, and where each is declared,
    /// read on first use.
    pub(crate) fn drupal_config_index(&self) -> Arc<ConfigIndex> {
        self.drupal.config.index(
            |uri| self.get_file_content_arc(uri),
            || self.open_drupal_test_config_files(),
        )
    }

    /// The open default configuration and schema files of test modules,
    /// each with whether it is a schema file, sorted.
    fn open_drupal_test_config_files(&self) -> Vec<(PathBuf, bool)> {
        let roots = self.drupal.web_roots.read().clone();
        let mut files: Vec<(PathBuf, bool)> = self
            .open_files
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
            .collect();
        files.sort_unstable();
        files
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
    /// or deleted on disk.
    pub(crate) fn note_drupal_config_changes(
        &self,
        changes: &[tower_lsp_server::gen_lsp_types::FileEvent],
    ) {
        let roots = self.drupal.web_roots.read().clone();
        self.drupal
            .config
            .record_watched_changes(changes.iter().filter_map(|change| {
                let path_str = change.uri.path();
                if !path_str.ends_with(".yml") || !path_str.contains("/config/") {
                    return None;
                }
                let path = crate::util::url_to_path(&change.uri)?;
                let root = roots.iter().find(|root| path.starts_with(root))?;
                let is_schema = if is_default_config_file(&path, root) {
                    false
                } else if is_schema_file(&path, root) {
                    true
                } else {
                    return None;
                };
                Some(ConfigFileEvent {
                    path,
                    is_schema,
                    kind: change.kind,
                    is_open: self.open_files.read().contains_key(change.uri.as_str()),
                })
            }));
    }

    /// Follow a document change that can change the configuration: a
    /// schema file is read from its buffer while it is open, and a test
    /// module's files count while they are.  A default configuration file
    /// gives its name by its path alone, so editing one changes nothing.
    /// `content` is `None` once the file is gone.
    pub(crate) fn note_drupal_config_document(&self, uri: &str, content: Option<&str>) {
        if !self.is_drupal_project()
            || !uri.ends_with(".yml")
            || !["/config/install/", "/config/optional/", "/config/schema/"]
                .iter()
                .any(|folder| uri.contains(folder))
        {
            return;
        }
        let spelled;
        let uri = if uri.contains("/config/schema/")
            && let Some(walked) = self.spell_as_walked(uri)
        {
            spelled = walked;
            spelled.as_str()
        } else {
            uri
        };
        self.drupal
            .config
            .record_document(uri, content, || self.open_drupal_test_config_files());
    }

    /// `uri` spelled the way the walk spells its file's path: through the
    /// directory links it followed, as watched-file events are spelled.
    fn spell_as_walked(&self, uri: &str) -> Option<String> {
        let path = crate::util::uri_to_path(uri)?;
        let path = self
            .followed_links()
            .to_link_spelling(&path)
            .unwrap_or(path);
        Some(crate::util::path_to_uri(&path))
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
        let entries = schema_entries(&Arc::from("u"), SCHEMA);
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

    /// Schema files by URI, as a reader that counts how often each is read.
    #[derive(Default)]
    struct Disk {
        texts: parking_lot::Mutex<HashMap<String, Arc<String>>>,
        reads: parking_lot::Mutex<HashMap<String, usize>>,
    }

    impl Disk {
        fn write(&self, uri: &str, text: &str) {
            self.texts
                .lock()
                .insert(uri.to_string(), Arc::new(text.to_string()));
        }

        fn read(&self, uri: &str) -> Option<Arc<String>> {
            *self.reads.lock().entry(uri.to_string()).or_default() += 1;
            self.texts.lock().get(uri).cloned()
        }

        fn reads(&self, uri: &str) -> usize {
            self.reads.lock().get(uri).copied().unwrap_or_default()
        }
    }

    /// The schema file of the extension `name`, and its URI.
    fn schema_file(name: &str) -> (PathBuf, String) {
        let path = std::env::temp_dir()
            .join("phpantom-config-sources")
            .join(name)
            .join("config/schema")
            .join(format!("{name}.schema.yml"));
        let uri = crate::util::path_to_uri(&path);
        (path, uri)
    }

    /// A watched-file event for the schema file of the extension `name`.
    fn on_disk(name: &str, kind: FileChangeType, is_open: bool) -> ConfigFileEvent {
        ConfigFileEvent {
            path: schema_file(name).0,
            is_schema: true,
            kind,
            is_open,
        }
    }

    const ACME: &str = "acme.settings:\n  type: config_object\n  label: 'Acme settings'\n  mapping:\n    notify:\n      type: string\nacme.shared:\n  type: config_object\n  label: 'From acme'\n";
    const EXAMPLE: &str = "example.settings:\n  type: config_object\n  mapping:\n    enabled:\n      type: boolean\nacme.shared:\n  type: config_object\n  label: 'From example'\n";

    /// Two schema files, `acme` before `example`, read once.
    fn two_files() -> (ConfigSources, Disk, String, String) {
        let (acme, acme_uri) = schema_file("acme");
        let (example, example_uri) = schema_file("example");
        let disk = Disk::default();
        disk.write(&acme_uri, ACME);
        disk.write(&example_uri, EXAMPLE);
        let sources = ConfigSources::default();
        sources.add_files(Vec::new(), vec![acme, example]);
        (sources, disk, acme_uri, example_uri)
    }

    #[test]
    fn an_edit_reads_only_the_edited_schema_file_again() {
        let (sources, disk, acme, example) = two_files();
        let read = |uri: &str| disk.read(uri);
        let index = sources.index(read, Vec::new);
        let tree = index.tree(&example, read).expect("the tree");
        assert_eq!((disk.reads(&acme), disk.reads(&example)), (1, 2));

        let edited = "acme.settings:\n  type: config_object\n  label: 'Acme edited'\nacme.added:\n  type: config_object\nacme.shared:\n  type: config_object\n  label: 'Still acme'\n";
        disk.write(&acme, edited);
        sources.record_document(&acme, Some(edited), Vec::new);
        let after = sources.index(read, Vec::new);
        assert!(!Arc::ptr_eq(&index, &after));
        assert_eq!((disk.reads(&acme), disk.reads(&example)), (2, 2));
        assert!(Arc::ptr_eq(
            &tree,
            &after.tree(&example, read).expect("the tree")
        ));
        assert_eq!(disk.reads(&example), 2);

        assert_eq!(
            after.names(),
            [
                "acme.added",
                "acme.settings",
                "acme.shared",
                "example.settings"
            ]
        );
        let label = |key: &str| {
            after
                .schema_named(key)
                .and_then(|entry| entry.label.clone())
        };
        assert_eq!(label("acme.settings").as_deref(), Some("Acme edited"));
        assert_eq!(label("acme.shared").as_deref(), Some("Still acme"));
        let fresh = ConfigIndex::build(&[], [(acme.as_str(), edited), (example.as_str(), EXAMPLE)]);
        assert_eq!(after.names(), fresh.names());
        assert_eq!(after.schema, fresh.schema);
    }

    #[test]
    fn an_event_with_the_text_read_keeps_the_index() {
        let (sources, disk, acme, _) = two_files();
        let read = |uri: &str| disk.read(uri);
        let index = sources.index(read, Vec::new);
        sources.record_document(&acme, Some(ACME), Vec::new);
        assert!(Arc::ptr_eq(&index, &sources.index(read, Vec::new)));
        assert_eq!(disk.reads(&acme), 1);
    }

    /// An event is judged by the text it carries, which the buffer read
    /// may not hold yet.
    #[test]
    fn an_event_is_judged_by_its_own_text() {
        let (sources, disk, acme, _) = two_files();
        let read = |uri: &str| disk.read(uri);
        let index = sources.index(read, Vec::new);
        disk.write(&acme, "acme.other:\n  type: config_object\n");
        sources.record_document(&acme, Some(ACME), Vec::new);
        assert!(Arc::ptr_eq(&index, &sources.index(read, Vec::new)));

        disk.write(&acme, ACME);
        sources.record_document(
            &acme,
            Some("acme.other:\n  type: config_object\n"),
            Vec::new,
        );
        assert!(!Arc::ptr_eq(&index, &sources.index(read, Vec::new)));
        assert_eq!(disk.reads(&acme), 2);
    }

    /// A key two files declare keeps the entry of the first file listed
    /// after either changes on disk.
    #[test]
    fn a_schema_file_changed_on_disk_keeps_its_place() {
        let (sources, disk, acme, example) = two_files();
        let read = |uri: &str| disk.read(uri);
        sources.index(read, Vec::new);
        sources.record_watched_changes([on_disk("acme", FileChangeType::Changed, false)]);
        let index = sources.index(read, Vec::new);
        assert_eq!((disk.reads(&acme), disk.reads(&example)), (2, 1));
        let shared = index.schema_named("acme.shared").expect("the entry");
        assert_eq!(&*shared.uri, acme);
        assert_eq!(shared.label.as_deref(), Some("From acme"));
    }

    /// An editor that saves by deleting a file and writing it again, and a
    /// checkout, report both in one batch.
    #[test]
    fn a_schema_file_deleted_and_created_at_once_keeps_its_place() {
        let (sources, disk, acme, _) = two_files();
        let read = |uri: &str| disk.read(uri);
        sources.index(read, Vec::new);
        sources.record_watched_changes([
            on_disk("acme", FileChangeType::Deleted, false),
            on_disk("acme", FileChangeType::Created, false),
        ]);
        let index = sources.index(read, Vec::new);
        assert_eq!(disk.reads(&acme), 2);
        let shared = index.schema_named("acme.shared").expect("the entry");
        assert_eq!(&*shared.uri, acme);

        sources.record_watched_changes([
            on_disk("acme", FileChangeType::Created, false),
            on_disk("acme", FileChangeType::Deleted, false),
        ]);
        let index = sources.index(read, Vec::new);
        assert!(index.schema_named("acme.settings").is_none());
    }

    /// An open schema file is read from its buffer, which a change on disk
    /// leaves as it is.
    #[test]
    fn a_change_on_disk_to_an_open_schema_file_keeps_the_index() {
        let (sources, disk, acme, _) = two_files();
        let read = |uri: &str| disk.read(uri);
        let index = sources.index(read, Vec::new);
        sources.record_watched_changes([on_disk("acme", FileChangeType::Changed, true)]);
        assert!(Arc::ptr_eq(&index, &sources.index(read, Vec::new)));
        assert_eq!(disk.reads(&acme), 1);
    }

    /// A schema file of an extension Drupal does not load is no source.
    #[test]
    fn an_edit_to_a_schema_file_not_listed_keeps_the_index() {
        let (sources, disk, _, _) = two_files();
        let read = |uri: &str| disk.read(uri);
        let index = sources.index(read, Vec::new);
        let (_, other) = schema_file("other");
        sources.record_document(&other, Some("other.settings: {}\n"), Vec::new);
        assert!(Arc::ptr_eq(&index, &sources.index(read, Vec::new)));
    }

    /// A build that overlaps an edit returns what it read, and keeps only
    /// the files the edit did not change.
    #[test]
    fn a_build_overlapping_an_edit_is_not_kept() {
        let (sources, disk, acme, example) = two_files();
        let edited = "acme.settings:\n  type: config_object\n  label: 'Acme edited'\n";
        let edit_once = std::sync::Once::new();
        let read = |uri: &str| {
            if uri == example {
                edit_once.call_once(|| {
                    disk.write(&acme, edited);
                    sources.record_document(&acme, Some(edited), Vec::new);
                });
            }
            disk.read(uri)
        };
        let overlapped = sources.index(read, Vec::new);
        let label = |index: &ConfigIndex| {
            index
                .schema_named("acme.settings")
                .and_then(|entry| entry.label.clone())
        };
        assert_eq!(label(&overlapped).as_deref(), Some("Acme settings"));

        let index = sources.index(read, Vec::new);
        assert!(!Arc::ptr_eq(&overlapped, &index));
        assert_eq!(label(&index).as_deref(), Some("Acme edited"));
        assert_eq!((disk.reads(&acme), disk.reads(&example)), (2, 1));
        assert!(Arc::ptr_eq(&index, &sources.index(read, Vec::new)));
    }

    /// A test module's file that opens while a build asks which are open
    /// counts from the next build on.
    #[test]
    fn a_build_overlapping_a_test_file_opening_is_not_kept() {
        let (sources, disk, _, _) = two_files();
        let read = |uri: &str| disk.read(uri);
        let path = std::env::temp_dir().join(
            "phpantom-config-sources/acme/tests/modules/acme_test/config/install/acme_test.settings.yml",
        );
        let uri = crate::util::path_to_uri(&path);
        let opened = parking_lot::Mutex::new(Vec::new());
        let open_test_files = || {
            let before = opened.lock().clone();
            if before.is_empty() {
                opened.lock().push((path.clone(), false));
                sources.record_document(&uri, Some("a: b\n"), || opened.lock().clone());
            }
            before
        };
        let overlapped = sources.index(read, open_test_files);
        assert!(
            !overlapped
                .names()
                .contains(&"acme_test.settings".to_string())
        );
        let index = sources.index(read, open_test_files);
        assert!(index.names().contains(&"acme_test.settings".to_string()));
    }

    /// A test module's open schema file, edited while a build reads it,
    /// is read again by the next build.
    #[test]
    fn a_test_schema_file_edited_while_read_is_read_again() {
        let (sources, disk, _, _) = two_files();
        let path = std::env::temp_dir().join(
            "phpantom-config-sources/acme/tests/modules/acme_test/config/schema/acme_test.schema.yml",
        );
        let uri = crate::util::path_to_uri(&path);
        disk.write(&uri, "acme_test.first:\n  type: config_object\n");
        let edited = "acme_test.edited:\n  type: config_object\n";
        let edit_once = std::sync::Once::new();
        let read = |read_uri: &str| {
            let text = disk.read(read_uri);
            if read_uri == uri {
                edit_once.call_once(|| {
                    disk.write(&uri, edited);
                    sources.record_document(&uri, Some(edited), Vec::new);
                });
            }
            text
        };
        let open_test_files = || vec![(path.clone(), true)];
        let overlapped = sources.index(read, open_test_files);
        assert!(overlapped.names().contains(&"acme_test.first".to_string()));
        let index = sources.index(read, open_test_files);
        assert!(index.names().contains(&"acme_test.edited".to_string()));
        assert!(!index.names().contains(&"acme_test.first".to_string()));
    }

    /// A tree is found by where the entries start, so one read from newer
    /// text than they were is used once and not kept.
    #[test]
    fn a_tree_of_newer_text_than_the_entries_is_not_kept() {
        let (sources, disk, acme, _) = two_files();
        let read = |uri: &str| disk.read(uri);
        let index = sources.index(read, Vec::new);
        disk.write(&acme, "acme.other:\n  type: config_object\n");
        let newer = index.tree(&acme, read).expect("the tree");
        assert_eq!(newer[0].key, "acme.other");

        disk.write(&acme, ACME);
        let tree = index.tree(&acme, read).expect("the tree");
        assert_eq!(tree[0].key, "acme.settings");
        assert!(Arc::ptr_eq(
            &tree,
            &index.tree(&acme, read).expect("the tree")
        ));
        assert_eq!(disk.reads(&acme), 3);
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
