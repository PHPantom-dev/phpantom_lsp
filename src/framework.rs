//! Symfony and Doctrine configuration reference indexing.
//!
//! PHPantom's normal [`SymbolMap`](crate::symbol_map::SymbolMap) is built from
//! PHP ASTs, but framework configuration also encodes symbols in YAML/XML and
//! PHP string literals. A parallel lightweight index lets those references
//! participate in go-to-definition, find-references, rename, code lenses, and
//! namespace/folder refactors.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use tower_lsp::lsp_types::{
    DocumentHighlight, DocumentHighlightKind, Location, Position, Range, TextEdit, Url,
};

use crate::Backend;
use crate::references::sort_locations_for_references;
use crate::text_position::{LineIndex, offset_to_position, position_to_offset};
use crate::util::strip_fqn_prefix;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SymfonySymbolKind {
    Service,
    Parameter,
}

impl SymfonySymbolKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Parameter => "parameter",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrameworkReferenceKind {
    /// A fully-qualified class/interface/trait/enum reference.
    Class { fqn: String },
    /// A member reference encoded in a framework string, e.g.
    /// `App\Controller\HomeController::index`.
    Method {
        class_fqn: String,
        member_name: String,
    },
    /// A namespace-prefix key, e.g. `App\:` in `services.yaml`.
    Namespace { prefix: String },
    /// A path-like scalar used by Symfony resource/exclude imports.
    Path { value: String },
    /// A named Symfony resource such as a service ID or parameter name.
    SymfonySymbol {
        kind: SymfonySymbolKind,
        name: String,
        declaration: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrameworkReference {
    pub(crate) uri: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) kind: FrameworkReferenceKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctrineRepositoryMapping {
    pub(crate) uri: String,
    pub(crate) entity_fqn: String,
    pub(crate) entity_start: u32,
    pub(crate) entity_end: u32,
    pub(crate) repository_fqn: String,
    pub(crate) repository_start: u32,
    pub(crate) repository_end: u32,
}

pub(crate) type FrameworkReferenceIndex =
    Arc<RwLock<HashMap<String, Arc<Vec<FrameworkReference>>>>>;

pub(crate) type DoctrineRepositoryIndex =
    Arc<RwLock<HashMap<String, Arc<Vec<DoctrineRepositoryMapping>>>>>;

#[derive(Debug, Clone)]
struct IndexedFrameworkLocation {
    uri: Arc<str>,
    range: Range,
}

impl IndexedFrameworkLocation {
    fn to_lsp(&self) -> Option<Location> {
        Some(Location {
            uri: Url::parse(&self.uri).ok()?,
            range: self.range,
        })
    }
}

#[derive(Debug, Clone)]
struct IndexedFrameworkMemberLocation {
    class_fqn: String,
    location: IndexedFrameworkLocation,
}

#[derive(Debug, Default)]
struct FrameworkLookupUriKeys {
    classes: HashSet<String>,
    methods: HashSet<String>,
}

/// Inverted locations for class and method references in framework resources.
///
/// The primary framework index stays keyed by URI for cursor-local features.
/// This derived index makes cross-file lookups proportional to the matching
/// references instead of to every YAML/XML reference in the workspace. The
/// reverse URI map keeps watched-file updates proportional to one resource.
#[derive(Debug, Default)]
pub(crate) struct FrameworkReferenceLookupIndexInner {
    classes: HashMap<String, Vec<IndexedFrameworkLocation>>,
    methods: HashMap<String, Vec<IndexedFrameworkMemberLocation>>,
    uri_keys: HashMap<Arc<str>, FrameworkLookupUriKeys>,
}

pub(crate) type FrameworkReferenceLookupIndex = Arc<RwLock<FrameworkReferenceLookupIndexInner>>;

pub(crate) fn new_framework_reference_index() -> FrameworkReferenceIndex {
    Arc::new(RwLock::new(HashMap::new()))
}

pub(crate) fn new_doctrine_repository_index() -> DoctrineRepositoryIndex {
    Arc::new(RwLock::new(HashMap::new()))
}

pub(crate) fn new_framework_reference_lookup_index() -> FrameworkReferenceLookupIndex {
    Arc::new(RwLock::new(FrameworkReferenceLookupIndexInner::default()))
}

pub(crate) fn is_framework_resource_uri(uri: &str) -> bool {
    let path = uri
        .strip_prefix("file://")
        .unwrap_or(uri)
        .split('?')
        .next()
        .unwrap_or(uri);
    let path_lower = path.to_ascii_lowercase();
    path_lower.ends_with(".yaml") || path_lower.ends_with(".yml") || path_lower.ends_with(".xml")
}

fn is_framework_resource_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
        Some(ext) if matches!(ext.as_str(), "yaml" | "yml" | "xml")
    )
}

fn is_php_uri(uri: &str) -> bool {
    let path = uri
        .strip_prefix("file://")
        .unwrap_or(uri)
        .split('?')
        .next()
        .unwrap_or(uri);
    path.get(path.len().saturating_sub(4)..)
        .is_some_and(|extension| extension.eq_ignore_ascii_case(".php"))
}

pub(crate) fn is_framework_php_config_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("php"))
        && path
            .components()
            .any(|component| matches!(component, Component::Normal(name) if name == "config"))
}

pub(crate) fn is_framework_php_config_uri(uri: &str) -> bool {
    if !is_php_uri(uri) {
        return false;
    }
    uri.split('?')
        .next()
        .unwrap_or(uri)
        .split('/')
        .any(|component| component == "config")
}

pub(crate) fn should_index_framework_php_content(uri: &str, content: &str) -> bool {
    is_php_uri(uri)
        && (is_framework_php_config_uri(uri)
            || content.contains("Autowire")
            || content.contains("ContainerInterface")
            || content.contains("ContainerBagInterface")
            || content.contains("ServiceLocator")
            || content.contains("getParameter(")
            || content.contains("hasParameter(")
            || content.contains("service(")
            || content.contains("param(")
            || content.contains("$container->get("))
}

fn is_skipped_resource_path(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => {
            let name = name.to_string_lossy();
            matches!(
                name.as_ref(),
                "vendor" | "node_modules" | ".git" | "var" | "cache"
            )
        }
        _ => false,
    })
}

impl Backend {
    fn replace_framework_lookup_uri(
        lookup: &mut FrameworkReferenceLookupIndexInner,
        uri: &str,
        content: &str,
        references: &[FrameworkReference],
    ) {
        Self::remove_framework_lookup_uri(lookup, uri);

        let uri: Arc<str> = Arc::from(uri);
        let mut keys = FrameworkLookupUriKeys::default();
        for reference in references {
            let location = IndexedFrameworkLocation {
                uri: Arc::clone(&uri),
                range: Range::new(
                    offset_to_position(content, reference.start as usize),
                    offset_to_position(content, reference.end as usize),
                ),
            };
            match &reference.kind {
                FrameworkReferenceKind::Class { fqn } => {
                    let key = framework_fqn_lookup_key(fqn);
                    lookup
                        .classes
                        .entry(key.clone())
                        .or_default()
                        .push(location);
                    keys.classes.insert(key);
                }
                FrameworkReferenceKind::Method {
                    class_fqn,
                    member_name,
                } => {
                    lookup.methods.entry(member_name.clone()).or_default().push(
                        IndexedFrameworkMemberLocation {
                            class_fqn: normalize_framework_fqn(class_fqn),
                            location,
                        },
                    );
                    keys.methods.insert(member_name.clone());
                }
                FrameworkReferenceKind::Namespace { .. }
                | FrameworkReferenceKind::Path { .. }
                | FrameworkReferenceKind::SymfonySymbol { .. } => {}
            }
        }

        if !keys.classes.is_empty() || !keys.methods.is_empty() {
            lookup.uri_keys.insert(uri, keys);
        }
    }

    fn remove_framework_lookup_uri(lookup: &mut FrameworkReferenceLookupIndexInner, uri: &str) {
        let Some(keys) = lookup.uri_keys.remove(uri) else {
            return;
        };

        for key in keys.classes {
            let remove_key = lookup.classes.get_mut(&key).is_some_and(|locations| {
                locations.retain(|location| location.uri.as_ref() != uri);
                locations.is_empty()
            });
            if remove_key {
                lookup.classes.remove(&key);
            }
        }
        for key in keys.methods {
            let remove_key = lookup.methods.get_mut(&key).is_some_and(|locations| {
                locations.retain(|entry| entry.location.uri.as_ref() != uri);
                locations.is_empty()
            });
            if remove_key {
                lookup.methods.remove(&key);
            }
        }
    }

    /// Scan framework configuration under the workspace root.
    pub(crate) fn index_framework_workspace(&self) -> usize {
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return 0;
        };

        let mut indexed = HashMap::new();
        let mut doctrine_repositories = HashMap::new();
        let mut lookup = FrameworkReferenceLookupIndexInner::default();
        for entry in ignore::WalkBuilder::new(&root)
            .hidden(false)
            .build()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            if (!is_framework_resource_path(path) && !is_framework_php_config_path(path))
                || is_skipped_resource_path(path)
            {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            let uri = crate::util::path_to_uri(path);
            let mappings = if is_framework_resource_uri(&uri) {
                scan_doctrine_repository_mappings(&uri, &content)
            } else {
                Vec::new()
            };
            if !mappings.is_empty() {
                doctrine_repositories.insert(uri.clone(), Arc::new(mappings));
            }
            if let Some(refs) = self.scan_framework_uri_references(&uri, &content)
                && !refs.is_empty()
            {
                Self::replace_framework_lookup_uri(&mut lookup, &uri, &content, &refs);
                indexed.insert(uri, Arc::new(refs));
            }
        }

        let count = indexed.len();
        *self.framework_references.write() = indexed;
        *self.framework_doctrine_repositories.write() = doctrine_repositories;
        *self.framework_reference_lookup.write() = lookup;
        count
    }

    pub(crate) fn index_framework_uri_content(&self, uri: &str, content: &str) {
        let refs = self.scan_framework_uri_references(uri, content);
        if refs.is_none() && !self.framework_references.read().contains_key(uri) {
            return;
        }
        let mappings = if is_framework_resource_uri(uri) {
            scan_doctrine_repository_mappings(uri, content)
        } else {
            Vec::new()
        };
        let mut index = self.framework_references.write();
        let mut lookup = self.framework_reference_lookup.write();
        match refs {
            Some(refs) if !refs.is_empty() => {
                Self::replace_framework_lookup_uri(&mut lookup, uri, content, &refs);
                index.insert(uri.to_string(), Arc::new(refs));
            }
            Some(_) | None => {
                index.remove(uri);
                Self::remove_framework_lookup_uri(&mut lookup, uri);
            }
        }
        let mut doctrine_repositories = self.framework_doctrine_repositories.write();
        if mappings.is_empty() {
            doctrine_repositories.remove(uri);
        } else {
            doctrine_repositories.insert(uri.to_string(), Arc::new(mappings));
        }
    }

    pub(crate) fn reindex_framework_uri_from_disk(&self, uri: &str) {
        if !is_framework_resource_uri(uri)
            && !is_framework_php_config_uri(uri)
            && !self.framework_references.read().contains_key(uri)
        {
            return;
        }
        let content = self.get_file_content(uri).or_else(|| {
            Url::parse(uri)
                .ok()
                .and_then(|u| u.to_file_path().ok())
                .and_then(|p| std::fs::read_to_string(p).ok())
        });
        match content {
            Some(content) => self.index_framework_uri_content(uri, &content),
            None => self.remove_framework_uri(uri),
        }
    }

    pub(crate) fn remove_framework_uri(&self, uri: &str) {
        self.framework_references.write().remove(uri);
        self.framework_doctrine_repositories.write().remove(uri);
        Self::remove_framework_lookup_uri(&mut self.framework_reference_lookup.write(), uri);
    }

    pub(crate) fn apply_framework_file_change(
        &self,
        uri: &str,
        path: &Path,
        change_type: tower_lsp::lsp_types::FileChangeType,
    ) -> bool {
        let is_php = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("php"));
        if (!is_framework_resource_path(path) && !is_php) || is_skipped_resource_path(path) {
            return false;
        }

        match change_type {
            tower_lsp::lsp_types::FileChangeType::DELETED => {
                self.remove_framework_uri(uri);
                true
            }
            tower_lsp::lsp_types::FileChangeType::CREATED
            | tower_lsp::lsp_types::FileChangeType::CHANGED => {
                let Ok(content) = std::fs::read_to_string(path) else {
                    self.remove_framework_uri(uri);
                    return true;
                };
                self.index_framework_uri_content(uri, &content);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn framework_reference_at_position(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<FrameworkReference> {
        let offset = position_to_offset(content, position);
        let refs = self
            .framework_references
            .read()
            .get(uri)
            .cloned()
            .or_else(|| {
                self.scan_framework_uri_references(uri, content)
                    .map(Arc::new)
            })?;

        refs.iter()
            .find(|reference| {
                offset >= reference.start
                    && (offset < reference.end
                        || (offset == reference.end && offset > reference.start))
            })
            .cloned()
            .or_else(|| {
                offset.checked_sub(1).and_then(|prev| {
                    refs.iter()
                        .find(|reference| prev >= reference.start && prev < reference.end)
                        .cloned()
                })
            })
    }

    pub(crate) fn framework_class_reference_locations(&self, target_fqn: &str) -> Vec<Location> {
        let lookup = self.framework_reference_lookup.read();
        let mut locations = lookup
            .classes
            .get(&framework_fqn_lookup_key(target_fqn))
            .into_iter()
            .flatten()
            .filter_map(IndexedFrameworkLocation::to_lsp)
            .collect();

        sort_locations(&mut locations);
        locations
    }

    /// Whether any framework resource names a method called `member`, so a
    /// caller can skip building the class scope a lookup would filter by.
    pub(crate) fn has_framework_member_references(&self, member: &str) -> bool {
        self.framework_reference_lookup
            .read()
            .methods
            .contains_key(member)
    }

    pub(crate) fn framework_member_reference_locations(
        &self,
        target_member: &str,
        hierarchy: Option<&crate::references::MemberScope>,
    ) -> Vec<Location> {
        let lookup = self.framework_reference_lookup.read();
        let mut locations = lookup
            .methods
            .get(target_member)
            .into_iter()
            .flatten()
            .filter(|entry| {
                hierarchy.is_none_or(|hierarchy| hierarchy.contains(self, &entry.class_fqn))
            })
            .filter_map(|entry| entry.location.to_lsp())
            .collect();
        sort_locations(&mut locations);
        locations
    }

    pub(crate) fn framework_symfony_symbol_names(
        &self,
        target_kind: SymfonySymbolKind,
    ) -> Vec<String> {
        let mut names = Vec::new();
        for refs in self.framework_references.read().values() {
            for reference in refs.iter() {
                let FrameworkReferenceKind::SymfonySymbol {
                    kind,
                    name,
                    declaration: true,
                } = &reference.kind
                else {
                    continue;
                };
                if *kind == target_kind {
                    push_unique_string(&mut names, name.clone());
                }
            }
        }
        names.sort_unstable();
        names
    }

    pub(crate) fn framework_symfony_symbol_locations(
        &self,
        target_kind: SymfonySymbolKind,
        target_name: &str,
        include_declarations: bool,
        include_references: bool,
    ) -> Vec<Location> {
        let mut locations = Vec::new();
        for (uri, refs) in self.framework_references.read().iter() {
            let Ok(parsed_uri) = Url::parse(uri) else {
                continue;
            };
            let Some(content) = self.get_file_content_arc(uri) else {
                continue;
            };
            let mut line_index = None;
            for reference in refs.iter() {
                let FrameworkReferenceKind::SymfonySymbol {
                    kind,
                    name,
                    declaration,
                } = &reference.kind
                else {
                    continue;
                };
                if *kind != target_kind
                    || name != target_name
                    || (*declaration && !include_declarations)
                    || (!*declaration && !include_references)
                {
                    continue;
                }
                let index = line_index.get_or_insert_with(|| LineIndex::new(&content));
                locations.push(Location {
                    uri: parsed_uri.clone(),
                    range: Range {
                        start: index.position(reference.start as usize),
                        end: index.position(reference.end as usize),
                    },
                });
            }
        }
        sort_locations_for_references(&mut locations);
        locations
    }

    pub(crate) fn framework_doctrine_repository_fqns_for_entity(
        &self,
        entity_fqn: &str,
    ) -> Vec<String> {
        let target = normalize_framework_fqn(entity_fqn);
        let mut out = Vec::new();
        for mappings in self.framework_doctrine_repositories.read().values() {
            for mapping in mappings.iter() {
                if normalize_framework_fqn(&mapping.entity_fqn).eq_ignore_ascii_case(&target) {
                    push_unique_string(&mut out, normalize_framework_fqn(&mapping.repository_fqn));
                }
            }
        }
        out
    }

    pub(crate) fn framework_doctrine_entity_fqns_for_repository(
        &self,
        repository_fqn: &str,
    ) -> Vec<String> {
        let target = normalize_framework_fqn(repository_fqn);
        let mut out = Vec::new();
        for mappings in self.framework_doctrine_repositories.read().values() {
            for mapping in mappings.iter() {
                if normalize_framework_fqn(&mapping.repository_fqn).eq_ignore_ascii_case(&target) {
                    push_unique_string(&mut out, normalize_framework_fqn(&mapping.entity_fqn));
                }
            }
        }
        out
    }

    pub(crate) fn framework_highlights(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Vec<DocumentHighlight>> {
        let reference = self.framework_reference_at_position(uri, content, position)?;
        let refs = self
            .framework_references
            .read()
            .get(uri)
            .cloned()
            .or_else(|| {
                self.scan_framework_uri_references(uri, content)
                    .map(Arc::new)
            })?;

        let mut highlights = Vec::new();
        for candidate in refs.iter() {
            let matched =
                match (&reference.kind, &candidate.kind) {
                    (
                        FrameworkReferenceKind::Class { fqn: lhs },
                        FrameworkReferenceKind::Class { fqn: rhs },
                    ) => normalize_framework_fqn(lhs)
                        .eq_ignore_ascii_case(&normalize_framework_fqn(rhs)),
                    (
                        FrameworkReferenceKind::Method {
                            class_fqn: lhs_class,
                            member_name: lhs_name,
                        },
                        FrameworkReferenceKind::Method {
                            class_fqn: rhs_class,
                            member_name: rhs_name,
                        },
                    ) => {
                        lhs_name == rhs_name
                            && normalize_framework_fqn(lhs_class)
                                .eq_ignore_ascii_case(&normalize_framework_fqn(rhs_class))
                    }
                    (
                        FrameworkReferenceKind::Namespace { prefix: lhs },
                        FrameworkReferenceKind::Namespace { prefix: rhs },
                    ) => normalize_framework_fqn(lhs)
                        .eq_ignore_ascii_case(&normalize_framework_fqn(rhs)),
                    (
                        FrameworkReferenceKind::Path { value: lhs },
                        FrameworkReferenceKind::Path { value: rhs },
                    ) => lhs == rhs,
                    (
                        FrameworkReferenceKind::SymfonySymbol {
                            kind: lhs_kind,
                            name: lhs_name,
                            ..
                        },
                        FrameworkReferenceKind::SymfonySymbol {
                            kind: rhs_kind,
                            name: rhs_name,
                            ..
                        },
                    ) => lhs_kind == rhs_kind && lhs_name == rhs_name,
                    _ => false,
                };
            if matched {
                highlights.push(DocumentHighlight {
                    range: Range {
                        start: offset_to_position(content, candidate.start as usize),
                        end: offset_to_position(content, candidate.end as usize),
                    },
                    kind: Some(DocumentHighlightKind::READ),
                });
            }
        }

        if highlights.is_empty() {
            None
        } else {
            highlights.sort_by(|a, b| {
                a.range
                    .start
                    .line
                    .cmp(&b.range.start.line)
                    .then(a.range.start.character.cmp(&b.range.start.character))
            });
            Some(highlights)
        }
    }

    /// Edits that move every framework resource name under `old_prefix` to
    /// `new_prefix`: the namespace itself, the namespace-prefix service keys
    /// under it, and every class it contains.
    pub(crate) fn collect_framework_namespace_edits(
        &self,
        old_prefix: &str,
        new_prefix: &str,
        changes: &mut HashMap<Url, Vec<TextEdit>>,
    ) {
        self.collect_framework_name_edits(old_prefix, new_prefix, true, changes);
    }

    /// Edits that rename class `old_fqn` to `new_fqn` wherever a framework
    /// resource names it, `Class::method` controller strings included.
    pub(crate) fn collect_framework_class_edits(
        &self,
        old_fqn: &str,
        new_fqn: &str,
        changes: &mut HashMap<Url, Vec<TextEdit>>,
    ) {
        self.collect_framework_name_edits(old_fqn, new_fqn, false, changes);
    }

    /// Rewrite the framework resource names equal to `old_name`, or nested
    /// under it when `nested` is set, to the matching name under
    /// `new_name`.
    ///
    /// Each occurrence is written back in the spelling its document uses
    /// (a leading backslash, the doubled backslashes of a quoted YAML
    /// string), which is why these edits are built here rather than by the
    /// PHP rename.  Each occurrence is also checked against the document's
    /// current text on its own: one the index no longer matches is left
    /// alone instead of cancelling the edits around it.  An occurrence the
    /// PHP rename already edited (a `Foo::class` in a PHP configurator) is
    /// left to that edit.
    fn collect_framework_name_edits(
        &self,
        old_name: &str,
        new_name: &str,
        nested: bool,
        changes: &mut HashMap<Url, Vec<TextEdit>>,
    ) {
        let old_name = normalize_framework_fqn(old_name);
        let new_name = normalize_framework_fqn(new_name);
        if old_name.is_empty() || new_name.is_empty() {
            return;
        }
        let nested_prefix = format!("{}\\", old_name.to_ascii_lowercase());

        for (uri, refs) in self.framework_references.read().iter() {
            let Ok(parsed_uri) = Url::parse(uri) else {
                continue;
            };
            let Some(content) = self.get_file_content_arc(uri) else {
                continue;
            };
            let mut line_index = None;
            let php_edits: Vec<Range> = changes
                .get(&parsed_uri)
                .map(|edits| edits.iter().map(|edit| edit.range).collect())
                .unwrap_or_default();
            for reference in refs.iter() {
                let name = match &reference.kind {
                    FrameworkReferenceKind::Class { fqn } => fqn,
                    FrameworkReferenceKind::Namespace { prefix } if nested => prefix,
                    _ => continue,
                };
                let normalized = normalize_framework_fqn(name);
                let exact = normalized.eq_ignore_ascii_case(&old_name);
                if !exact
                    && !(nested && normalized.to_ascii_lowercase().starts_with(&nested_prefix))
                {
                    continue;
                }
                let Some(source) = content.get(reference.start as usize..reference.end as usize)
                else {
                    continue;
                };
                if !normalize_framework_fqn(source).eq_ignore_ascii_case(&normalized) {
                    continue;
                }

                let replacement = if exact {
                    new_name.clone()
                } else {
                    format!("{}{}", new_name, &normalized[old_name.len()..])
                };
                let index = line_index.get_or_insert_with(|| LineIndex::new(&content));
                let range = Range {
                    start: index.position(reference.start as usize),
                    end: index.position(reference.end as usize),
                };
                if php_edits
                    .iter()
                    .any(|edit| edit.start < range.end && range.start < edit.end)
                {
                    continue;
                }
                changes
                    .entry(parsed_uri.clone())
                    .or_default()
                    .push(TextEdit {
                        range,
                        new_text: rewrite_framework_fqn_literal(source, &replacement),
                    });
            }
        }
    }

    pub(crate) fn collect_framework_path_edits_for_directory_renames(
        &self,
        directory_renames: &[(Url, Url)],
        changes: &mut HashMap<Url, Vec<TextEdit>>,
    ) {
        if directory_renames.is_empty() {
            return;
        }

        let workspace_root = self.workspace.workspace_root.read().clone();
        let renames: Vec<(PathBuf, PathBuf)> = directory_renames
            .iter()
            .filter_map(|(old_uri, new_uri)| {
                let old_path = old_uri.to_file_path().ok()?;
                let new_path = new_uri.to_file_path().ok()?;
                Some((normalize_path(old_path), normalize_path(new_path)))
            })
            .collect();

        if renames.is_empty() {
            return;
        }

        for (uri, refs) in self.framework_references.read().iter() {
            let Ok(parsed_uri) = Url::parse(uri) else {
                continue;
            };
            let Ok(file_path) = parsed_uri.to_file_path() else {
                continue;
            };
            let Some(file_dir) = file_path.parent() else {
                continue;
            };
            let Some(content) = self.get_file_content_arc(uri) else {
                continue;
            };

            for reference in refs.iter() {
                let FrameworkReferenceKind::Path { value } = &reference.kind else {
                    continue;
                };
                let Some(rewritten) = rewrite_framework_path_for_directory_renames(
                    value,
                    file_dir,
                    workspace_root.as_deref(),
                    &renames,
                ) else {
                    continue;
                };
                if rewritten == *value {
                    continue;
                }

                changes
                    .entry(parsed_uri.clone())
                    .or_default()
                    .push(TextEdit {
                        range: Range {
                            start: offset_to_position(&content, reference.start as usize),
                            end: offset_to_position(&content, reference.end as usize),
                        },
                        new_text: rewritten,
                    });
            }
        }
    }

    fn scan_framework_uri_references(
        &self,
        uri: &str,
        content: &str,
    ) -> Option<Vec<FrameworkReference>> {
        if is_framework_resource_uri(uri) {
            return Some(scan_framework_references(uri, content));
        }
        if should_index_framework_php_content(uri, content) {
            return Some(self.scan_symfony_php_references(uri, content));
        }
        None
    }

    fn scan_symfony_php_references(&self, uri: &str, content: &str) -> Vec<FrameworkReference> {
        let use_map = self.parse_use_statements(content);
        let namespace = self.parse_namespace(content);
        let mut refs = Vec::new();
        let include_config_resources =
            is_framework_php_config_uri(uri) && is_symfony_php_config_content(content);
        let literals = scan_php_string_literals_and_class_constants(
            uri,
            content,
            &use_map,
            &namespace,
            include_config_resources,
            &mut refs,
        );

        for (idx, literal) in literals.iter().enumerate() {
            if include_config_resources {
                scan_php_config_literal(uri, literal, &mut refs);
            }
            scan_php_symfony_literal(
                uri,
                content,
                &literals,
                idx,
                include_config_resources,
                &mut refs,
            );

            let value = literal.value.trim();
            if include_config_resources && valid_framework_segment(value) {
                let class_fqn =
                    php_callable_class_before(content, literal.quote_start, &use_map, &namespace)
                        .or_else(|| php_callable_string_class_before(content, &literals, idx));
                if let Some(class_fqn) = class_fqn {
                    refs.push(FrameworkReference {
                        uri: uri.to_string(),
                        start: literal.start as u32,
                        end: literal.end as u32,
                        kind: FrameworkReferenceKind::Method {
                            class_fqn,
                            member_name: value.to_string(),
                        },
                    });
                }
            }

            if include_config_resources
                && looks_like_path_value(value)
                && php_literal_has_path_context(content, &literals, idx)
            {
                refs.push(FrameworkReference {
                    uri: uri.to_string(),
                    start: literal.start as u32,
                    end: literal.end as u32,
                    kind: FrameworkReferenceKind::Path {
                        value: value.to_string(),
                    },
                });
            }
        }

        if include_config_resources {
            let class_service_declarations: Vec<(u32, u32, String)> = refs
                .iter()
                .filter_map(|reference| {
                    let FrameworkReferenceKind::Class { fqn } = &reference.kind else {
                        return None;
                    };
                    let call = php_call_context(content, reference.start as usize)?;
                    (call.name == "set" && call.argument_index == 0)
                        .then(|| (reference.start, reference.end, normalize_framework_fqn(fqn)))
                })
                .collect();
            for (start, end, name) in class_service_declarations {
                refs.push(FrameworkReference {
                    uri: uri.to_string(),
                    start,
                    end,
                    kind: FrameworkReferenceKind::SymfonySymbol {
                        kind: SymfonySymbolKind::Service,
                        name,
                        declaration: true,
                    },
                });
            }
        }

        refs.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
        refs.dedup();
        refs
    }
}

#[derive(Debug, Clone, Copy)]
struct PhpStringLiteral<'a> {
    value: &'a str,
    quote_start: usize,
    quote_end: usize,
    start: usize,
    end: usize,
}

fn is_symfony_php_config_content(content: &str) -> bool {
    let has_configurator = content.contains("Configurator");
    if !has_configurator && !content.contains("Symfony\\Config\\") {
        return false;
    }

    content.contains(r"Symfony\Component\DependencyInjection\Loader\Configurator")
        || content.contains(r"Symfony\Component\Routing\Loader\Configurator")
        || content.contains("Symfony\\Config\\")
        || (has_configurator
            && (content.contains("ContainerConfigurator")
                || content.contains("RoutingConfigurator"))
            && [
                "->services(",
                "->set(",
                "->load(",
                "->controller(",
                "->import(",
                "::config(",
            ]
            .iter()
            .any(|needle| content.contains(needle)))
}

fn scan_php_string_literals_and_class_constants<'a>(
    uri: &str,
    content: &'a str,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
    capture_class_references: bool,
    refs: &mut Vec<FrameworkReference>,
) -> Vec<PhpStringLiteral<'a>> {
    let bytes = content.as_bytes();
    let mut literals = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'#' && bytes.get(i + 1) != Some(&b'[') {
            i += 1;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }

        if matches!(bytes[i], b'\'' | b'"') {
            let quote = bytes[i];
            let quote_start = i;
            let start = i + 1;
            i = start;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    literals.push(PhpStringLiteral {
                        value: &content[start..i],
                        quote_start,
                        quote_end: i,
                        start,
                        end: i,
                    });
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        if is_php_name_start(bytes[i]) && (i == 0 || !is_php_name_char(bytes[i.saturating_sub(1)]))
        {
            let start = i;
            i += 1;
            while i < bytes.len() && is_php_name_char(bytes[i]) {
                i += 1;
            }
            let end = i;
            let mut cursor = end;
            skip_ascii_whitespace(bytes, &mut cursor);
            if bytes.get(cursor..cursor + 2) != Some(b"::") {
                continue;
            }
            cursor += 2;
            skip_ascii_whitespace(bytes, &mut cursor);
            if !content
                .get(cursor..cursor + 5)
                .is_some_and(|keyword| keyword.eq_ignore_ascii_case("class"))
                || bytes
                    .get(cursor + 5)
                    .is_some_and(|byte| is_php_identifier_char(*byte))
            {
                continue;
            }

            let raw_name = &content[start..end];
            if matches!(
                raw_name.to_ascii_lowercase().as_str(),
                "self" | "static" | "parent"
            ) {
                continue;
            }
            let fqn =
                normalize_framework_fqn(&crate::util::resolve_to_fqn(raw_name, use_map, namespace));
            if capture_class_references && valid_framework_name(&fqn) {
                refs.push(FrameworkReference {
                    uri: uri.to_string(),
                    start: start as u32,
                    end: end as u32,
                    kind: FrameworkReferenceKind::Class { fqn },
                });
            }
            continue;
        }

        i += 1;
    }

    literals
}

#[derive(Clone, Copy)]
struct PhpCallContext<'a> {
    name: &'a str,
    argument_index: usize,
    args_start: usize,
}

fn scan_php_symfony_literal(
    uri: &str,
    content: &str,
    literals: &[PhpStringLiteral<'_>],
    literal_idx: usize,
    in_configurator: bool,
    refs: &mut Vec<FrameworkReference>,
) {
    let literal = &literals[literal_idx];
    scan_parameter_placeholders(uri, literal.value, literal.start, refs);

    let leading = literal.value.len() - literal.value.trim_start().len();
    let trailing = literal.value.len() - literal.value.trim_end().len();
    let raw = literal.value.trim();
    if raw.is_empty() {
        return;
    }

    if in_configurator {
        let service_prefix = raw
            .bytes()
            .take_while(|byte| matches!(byte, b'@' | b'?' | b'!'))
            .count();
        if service_prefix > 0 {
            let name = php_semantic_string(&raw[service_prefix..]);
            if valid_symfony_symbol_name(&name) {
                push_symfony_symbol(
                    refs,
                    uri,
                    SymfonySymbolKind::Service,
                    name,
                    literal.start + leading + service_prefix,
                    literal.end - trailing,
                    false,
                );
            }
        }
    }

    let Some(call) = php_call_context(content, literal.quote_start) else {
        return;
    };
    let call_name = call.name.to_ascii_lowercase();
    let named_argument = php_named_argument_before(content, call.args_start, literal.quote_start);
    let semantic_value = php_semantic_string(raw);
    if !valid_symfony_symbol_name(&semantic_value) {
        return;
    }

    let service_reference = (call_name == "alias" && call.argument_index == 1)
        || (matches!(call_name.as_str(), "service" | "decorate" | "target")
            && call.argument_index == 0)
        || (matches!(call_name.as_str(), "get" | "has")
            && call.argument_index == 0
            && looks_like_container_call(content, call))
        || (call_name == "autowire"
            && named_argument.is_some_and(|name| name.eq_ignore_ascii_case("service")));
    let parameter_reference = (matches!(
        call_name.as_str(),
        "param" | "getparameter" | "hasparameter"
    ) && call.argument_index == 0)
        || (call_name == "autowire"
            && named_argument.is_some_and(|name| name.eq_ignore_ascii_case("param")));
    let (kind, declaration) = if in_configurator
        && call.argument_index == 0
        && call_name == "set"
        && looks_like_parameter_set(content, call)
    {
        (SymfonySymbolKind::Parameter, true)
    } else if in_configurator
        && call.argument_index == 0
        && matches!(call_name.as_str(), "set" | "alias")
    {
        (SymfonySymbolKind::Service, true)
    } else if in_configurator && call_name == "setparameter" && call.argument_index == 0 {
        (SymfonySymbolKind::Parameter, true)
    } else if service_reference {
        (SymfonySymbolKind::Service, false)
    } else if parameter_reference {
        (SymfonySymbolKind::Parameter, false)
    } else {
        return;
    };

    push_symfony_symbol(
        refs,
        uri,
        kind,
        semantic_value,
        literal.start + leading,
        literal.end - trailing,
        declaration,
    );
}

fn php_call_context(content: &str, offset: usize) -> Option<PhpCallContext<'_>> {
    let prefix = content.get(..offset)?;
    let search_start = offset.saturating_sub(2048);
    let open = prefix.as_bytes()[search_start..]
        .iter()
        .rposition(|byte| *byte == b'(')?
        + search_start;
    let bytes = content.as_bytes();
    let mut name_end = open;
    skip_ascii_whitespace_backwards(bytes, &mut name_end);
    let mut name_start = name_end;
    while name_start > 0 && is_php_identifier_char(bytes[name_start - 1]) {
        name_start -= 1;
    }
    if name_start == name_end {
        return None;
    }

    let mut argument_index = 0usize;
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;
    let mut brace_depth = 0u32;
    let mut quote = None;
    let mut escaped = false;
    for byte in bytes[open + 1..offset].iter().copied() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if matches!(byte, b'\'' | b'"') {
            if quote == Some(byte) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(byte);
            }
            continue;
        }
        if quote.is_some() {
            continue;
        }
        match byte {
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                argument_index += 1;
            }
            _ => {}
        }
    }

    Some(PhpCallContext {
        name: &content[name_start..name_end],
        argument_index,
        args_start: open + 1,
    })
}

fn php_named_argument_before(content: &str, args_start: usize, quote_start: usize) -> Option<&str> {
    let before = content.get(args_start..quote_start)?;
    let segment = before
        .rsplit_once(',')
        .map_or(before, |(_, tail)| tail)
        .trim();
    let colon = segment.rfind(':')?;
    let name = segment[..colon].trim();
    (!name.is_empty() && name.bytes().all(is_php_identifier_char)).then_some(name)
}

fn looks_like_container_call(content: &str, call: PhpCallContext<'_>) -> bool {
    let name_offset = call.name.as_ptr() as usize - content.as_ptr() as usize;
    let before = content[..name_offset].trim_end();
    let receiver_end = before.strip_suffix("->").map(str::trim_end);
    let Some(receiver_end) = receiver_end else {
        return false;
    };
    let receiver_start = receiver_end
        .rfind(|character: char| {
            !(character == '$' || character == '_' || character.is_ascii_alphanumeric())
        })
        .map_or(0, |index| index + 1);
    let receiver = &receiver_end[receiver_start..];
    matches!(
        receiver,
        "$container" | "$serviceLocator" | "$locator" | "container"
    ) || (!receiver.is_empty()
        && [
            format!("ContainerInterface {receiver}"),
            format!("ServiceLocator {receiver}"),
            format!("ContainerBagInterface {receiver}"),
        ]
        .iter()
        .any(|typed| content.contains(typed)))
}

fn looks_like_parameter_set(content: &str, call: PhpCallContext<'_>) -> bool {
    let name_offset = call.name.as_ptr() as usize - content.as_ptr() as usize;
    let start = name_offset.saturating_sub(160);
    let prefix = &content[start..name_offset];
    prefix.contains("->parameters()->")
        || prefix.trim_end().ends_with("$parameters->")
        || prefix.trim_end().ends_with("$params->")
}

fn php_semantic_string(raw: &str) -> String {
    if raw.contains('\\') {
        raw.replace("\\\\", "\\")
    } else {
        raw.to_string()
    }
}

fn scan_php_config_literal(
    uri: &str,
    literal: &PhpStringLiteral<'_>,
    refs: &mut Vec<FrameworkReference>,
) {
    let leading_whitespace = literal.value.len() - literal.value.trim_start().len();
    let trimmed = literal.value.trim();
    if trimmed.is_empty() {
        return;
    }

    let service_prefix = trimmed
        .bytes()
        .take_while(|byte| matches!(byte, b'@' | b'?'))
        .count();
    let source = &trimmed[service_prefix..];
    if source.is_empty() {
        return;
    }
    let start = literal.start + leading_whitespace + service_prefix;

    if let Some(separator) = source.find("::") {
        let class_source = &source[..separator];
        let method_name = &source[separator + 2..];
        let class_fqn = normalize_framework_fqn(class_source);
        if valid_framework_name(&class_fqn) && valid_framework_segment(method_name) {
            refs.push(FrameworkReference {
                uri: uri.to_string(),
                start: start as u32,
                end: (start + class_source.len()) as u32,
                kind: FrameworkReferenceKind::Class {
                    fqn: class_fqn.clone(),
                },
            });
            refs.push(FrameworkReference {
                uri: uri.to_string(),
                start: (start + separator + 2) as u32,
                end: (start + source.len()) as u32,
                kind: FrameworkReferenceKind::Method {
                    class_fqn,
                    member_name: method_name.to_string(),
                },
            });
        }
        return;
    }

    let normalized = normalize_framework_fqn(source);
    if !source.contains('\\') || !valid_framework_name(&normalized) {
        return;
    }

    let kind = if source.ends_with('\\') {
        FrameworkReferenceKind::Namespace { prefix: normalized }
    } else {
        FrameworkReferenceKind::Class { fqn: normalized }
    };
    refs.push(FrameworkReference {
        uri: uri.to_string(),
        start: start as u32,
        end: (start + source.len()) as u32,
        kind,
    });
}

fn php_callable_class_before(
    content: &str,
    quote_start: usize,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
) -> Option<String> {
    let bytes = content.as_bytes();
    let mut cursor = quote_start;
    skip_ascii_whitespace_backwards(bytes, &mut cursor);
    if cursor == 0 || bytes[cursor - 1] != b',' {
        return None;
    }
    cursor -= 1;
    skip_ascii_whitespace_backwards(bytes, &mut cursor);
    let keyword_start = cursor.checked_sub(5)?;
    if !content[keyword_start..cursor].eq_ignore_ascii_case("class") {
        return None;
    }
    cursor = keyword_start;
    skip_ascii_whitespace_backwards(bytes, &mut cursor);
    if cursor < 2 || &bytes[cursor - 2..cursor] != b"::" {
        return None;
    }
    cursor -= 2;
    skip_ascii_whitespace_backwards(bytes, &mut cursor);
    let end = cursor;
    while cursor > 0 && is_php_name_char(bytes[cursor - 1]) {
        cursor -= 1;
    }
    if cursor == end {
        return None;
    }
    let raw_name = &content[cursor..end];
    let fqn = normalize_framework_fqn(&crate::util::resolve_to_fqn(raw_name, use_map, namespace));
    valid_framework_name(&fqn).then_some(fqn)
}

fn php_callable_string_class_before(
    content: &str,
    literals: &[PhpStringLiteral<'_>],
    current_idx: usize,
) -> Option<String> {
    let previous = literals.get(current_idx.checked_sub(1)?)?;
    let current = literals.get(current_idx)?;
    if content[previous.quote_end + 1..current.quote_start].trim() != "," {
        return None;
    }
    if !content[..previous.quote_start].trim_end().ends_with('[') {
        return None;
    }
    let class_fqn = normalize_framework_fqn(previous.value.trim());
    valid_framework_name(&class_fqn).then_some(class_fqn)
}

fn php_literal_has_path_context(
    content: &str,
    literals: &[PhpStringLiteral<'_>],
    current_idx: usize,
) -> bool {
    let current = &literals[current_idx];
    let prefix = &content[..current.quote_start];
    if let Some(open_paren) = prefix.rfind('(') {
        let mut name_end = open_paren;
        skip_ascii_whitespace_backwards(content.as_bytes(), &mut name_end);
        let mut name_start = name_end;
        while name_start > 0 && is_php_identifier_char(content.as_bytes()[name_start - 1]) {
            name_start -= 1;
        }
        let call_name = &content[name_start..name_end];
        let argument_index = content[open_paren + 1..current.quote_start]
            .bytes()
            .filter(|byte| *byte == b',')
            .count();
        if (call_name == "import" && argument_index == 0)
            || (call_name == "load" && argument_index == 1)
        {
            return true;
        }
    }

    for previous in literals[..current_idx].iter().rev() {
        if current.quote_start.saturating_sub(previous.quote_end) > 512 {
            break;
        }
        if !matches!(
            previous.value.trim(),
            "resource" | "exclude" | "path" | "paths" | "dir" | "directory"
        ) {
            continue;
        }
        let between = content[previous.quote_end + 1..current.quote_start].trim();
        let Some(after_arrow) = between.strip_prefix("=>") else {
            continue;
        };
        let after_arrow = after_arrow.trim();
        if after_arrow.is_empty() {
            return true;
        }
        if after_arrow.starts_with('[')
            && after_arrow.bytes().filter(|byte| *byte == b'[').count()
                > after_arrow.bytes().filter(|byte| *byte == b']').count()
        {
            return true;
        }
    }

    false
}

fn is_php_name_start(byte: u8) -> bool {
    byte == b'\\' || byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_php_name_char(byte: u8) -> bool {
    byte == b'\\' || is_php_identifier_char(byte)
}

fn is_php_identifier_char(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn skip_ascii_whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes.get(*cursor).is_some_and(u8::is_ascii_whitespace) {
        *cursor += 1;
    }
}

fn skip_ascii_whitespace_backwards(bytes: &[u8], cursor: &mut usize) {
    while *cursor > 0 && bytes[*cursor - 1].is_ascii_whitespace() {
        *cursor -= 1;
    }
}

fn scan_framework_references(uri: &str, content: &str) -> Vec<FrameworkReference> {
    let mut refs = Vec::new();
    scan_class_like_tokens(uri, content, &mut refs);
    scan_path_scalars(uri, content, &mut refs);
    if uri
        .split('?')
        .next()
        .is_some_and(|path| path.ends_with(".yaml") || path.ends_with(".yml"))
    {
        scan_symfony_yaml_container_symbols(uri, content, &mut refs);
    } else if uri
        .split('?')
        .next()
        .is_some_and(|path| path.ends_with(".xml"))
    {
        scan_symfony_xml_container_symbols(uri, content, &mut refs);
    }
    refs.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
    refs.dedup();
    refs
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum YamlContainerSectionKind {
    Services,
    Parameters,
}

struct YamlContainerSection {
    kind: YamlContainerSectionKind,
    indent: usize,
    child_indent: Option<usize>,
}

fn scan_symfony_yaml_container_symbols(
    uri: &str,
    content: &str,
    refs: &mut Vec<FrameworkReference>,
) {
    let mut section: Option<YamlContainerSection> = None;
    let has_container_section = content.lines().any(|line| {
        matches!(
            line.trim(),
            "services:"
                | "\"services\":"
                | "'services':"
                | "parameters:"
                | "\"parameters\":"
                | "'parameters':"
        )
    });
    if !has_container_section {
        return;
    }

    for (line_start, line) in line_offsets(content) {
        let semantic = yaml_content_before_comment(line);
        let trimmed = semantic.trim();
        if trimmed.is_empty() || trimmed.starts_with('-') {
            continue;
        }
        scan_parameter_placeholders(uri, semantic, line_start, refs);

        let indent = leading_spaces(semantic);
        let section_kind = match trimmed {
            "services:" | "\"services\":" | "'services':" => {
                Some(YamlContainerSectionKind::Services)
            }
            "parameters:" | "\"parameters\":" | "'parameters':" => {
                Some(YamlContainerSectionKind::Parameters)
            }
            _ => None,
        };
        if let Some(kind) = section_kind {
            section = Some(YamlContainerSection {
                kind,
                indent,
                child_indent: None,
            });
            continue;
        }

        if section
            .as_ref()
            .is_some_and(|current| indent <= current.indent)
        {
            section = None;
        }

        let Some(current) = section.as_mut() else {
            continue;
        };
        if current.child_indent.is_none() {
            current.child_indent = Some(indent);
        }

        if current.child_indent == Some(indent)
            && let Some((raw_key, key_start, key_end, value_start)) =
                yaml_mapping_entry(semantic, line_start)
        {
            let (key, quote_adjust) = strip_yaml_quotes(raw_key);
            let key_start = key_start + quote_adjust.0;
            let key_end = key_end.saturating_sub(quote_adjust.1);
            let is_declaration = match current.kind {
                YamlContainerSectionKind::Services => !key.starts_with('_') && !key.ends_with('\\'),
                YamlContainerSectionKind::Parameters => !key.starts_with('_'),
            };
            if is_declaration && valid_symfony_symbol_name(key) {
                refs.push(FrameworkReference {
                    uri: uri.to_string(),
                    start: key_start as u32,
                    end: key_end as u32,
                    kind: FrameworkReferenceKind::SymfonySymbol {
                        kind: match current.kind {
                            YamlContainerSectionKind::Services => SymfonySymbolKind::Service,
                            YamlContainerSectionKind::Parameters => SymfonySymbolKind::Parameter,
                        },
                        name: key.to_string(),
                        declaration: true,
                    },
                });
            }

            if matches!(current.kind, YamlContainerSectionKind::Services) {
                scan_service_references_in_text(uri, semantic, line_start, value_start, refs);
            }
        }

        if matches!(current.kind, YamlContainerSectionKind::Services) {
            scan_service_references_in_text(uri, semantic, line_start, indent, refs);
        }
    }
}

fn yaml_mapping_entry(line: &str, line_start: usize) -> Option<(&str, usize, usize, usize)> {
    let indent = leading_spaces(line);
    let trimmed = &line[indent..];
    let colon = trimmed.find(':')?;
    let raw_key = trimmed[..colon].trim();
    if raw_key.is_empty() {
        return None;
    }
    let raw_offset = trimmed[..colon].find(raw_key)?;
    let key_start = line_start + indent + raw_offset;
    let key_end = key_start + raw_key.len();
    Some((raw_key, key_start, key_end, indent + colon + 1))
}

fn yaml_content_before_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quote = None;
    let mut escaped = false;
    for (idx, byte) in bytes.iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if matches!(byte, b'\'' | b'"') {
            if quote == Some(byte) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(byte);
            }
            continue;
        }
        if byte == b'#' && quote.is_none() {
            return &line[..idx];
        }
    }
    line
}

fn scan_service_references_in_text(
    uri: &str,
    text: &str,
    absolute_start: usize,
    from: usize,
    refs: &mut Vec<FrameworkReference>,
) {
    let bytes = text.as_bytes();
    let mut cursor = from.min(bytes.len());
    while cursor < bytes.len() {
        if bytes[cursor] != b'@' {
            cursor += 1;
            continue;
        }
        let mut start = cursor + 1;
        while bytes
            .get(start)
            .is_some_and(|byte| matches!(*byte, b'?' | b'!'))
        {
            start += 1;
        }
        let mut end = start;
        while bytes
            .get(end)
            .is_some_and(|byte| is_symfony_symbol_char(*byte))
        {
            end += 1;
        }
        let name = &text[start..end];
        if valid_symfony_symbol_name(name) {
            refs.push(FrameworkReference {
                uri: uri.to_string(),
                start: (absolute_start + start) as u32,
                end: (absolute_start + end) as u32,
                kind: FrameworkReferenceKind::SymfonySymbol {
                    kind: SymfonySymbolKind::Service,
                    name: name.to_string(),
                    declaration: false,
                },
            });
        }
        cursor = end.max(cursor + 1);
    }
}

fn scan_parameter_placeholders(
    uri: &str,
    text: &str,
    absolute_start: usize,
    refs: &mut Vec<FrameworkReference>,
) {
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let Some(open_rel) = text[cursor..].find('%') else {
            break;
        };
        let open = cursor + open_rel;
        let Some(close_rel) = text[open + 1..].find('%') else {
            break;
        };
        let close = open + 1 + close_rel;
        let name = &text[open + 1..close];
        if valid_symfony_symbol_name(name)
            && !name.starts_with("env(")
            && !name.starts_with("resolve:")
        {
            refs.push(FrameworkReference {
                uri: uri.to_string(),
                start: (absolute_start + open + 1) as u32,
                end: (absolute_start + close) as u32,
                kind: FrameworkReferenceKind::SymfonySymbol {
                    kind: SymfonySymbolKind::Parameter,
                    name: name.to_string(),
                    declaration: false,
                },
            });
        }
        cursor = close + 1;
    }
}

fn scan_symfony_xml_container_symbols(
    uri: &str,
    content: &str,
    refs: &mut Vec<FrameworkReference>,
) {
    if !content.contains("<service")
        && !content.contains("<parameter")
        && !content.contains("<argument")
    {
        return;
    }
    scan_parameter_placeholders(uri, content, 0, refs);

    let lower = content.to_ascii_lowercase();
    let mut search = 0usize;
    while let Some(rel_start) = lower[search..].find('<') {
        let tag_start = search + rel_start;
        let Some(rel_end) = content[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + rel_end + 1;
        let tag = &content[tag_start..tag_end];
        let tag_lower = tag.to_ascii_lowercase();

        if tag_lower.starts_with("<service") {
            if let Some((name, start, end)) = xml_attr_value(tag, tag_start, &["id"])
                && valid_symfony_symbol_name(&name)
            {
                push_symfony_symbol(
                    refs,
                    uri,
                    SymfonySymbolKind::Service,
                    name,
                    start,
                    end,
                    true,
                );
            }
            for attr in [
                "alias",
                "decorates",
                "parent",
                "factory-service",
                "configurator-service",
            ] {
                if let Some((name, start, end)) = xml_attr_value(tag, tag_start, &[attr])
                    && valid_symfony_symbol_name(&name)
                {
                    push_symfony_symbol(
                        refs,
                        uri,
                        SymfonySymbolKind::Service,
                        name,
                        start,
                        end,
                        false,
                    );
                }
            }
        } else if tag_lower.starts_with("<argument") {
            let service_argument = xml_attr_value(tag, tag_start, &["type"])
                .is_some_and(|(value, _, _)| value.eq_ignore_ascii_case("service"));
            if service_argument
                && let Some((name, start, end)) = xml_attr_value(tag, tag_start, &["id", "service"])
                && valid_symfony_symbol_name(&name)
            {
                push_symfony_symbol(
                    refs,
                    uri,
                    SymfonySymbolKind::Service,
                    name,
                    start,
                    end,
                    false,
                );
            }
        } else if tag_lower.starts_with("<parameter")
            && let Some((name, start, end)) = xml_attr_value(tag, tag_start, &["key", "name", "id"])
            && valid_symfony_symbol_name(&name)
        {
            push_symfony_symbol(
                refs,
                uri,
                SymfonySymbolKind::Parameter,
                name,
                start,
                end,
                true,
            );
        }

        search = tag_end;
    }
}

fn push_symfony_symbol(
    refs: &mut Vec<FrameworkReference>,
    uri: &str,
    kind: SymfonySymbolKind,
    name: String,
    start: usize,
    end: usize,
    declaration: bool,
) {
    refs.push(FrameworkReference {
        uri: uri.to_string(),
        start: start as u32,
        end: end as u32,
        kind: FrameworkReferenceKind::SymfonySymbol {
            kind,
            name,
            declaration,
        },
    });
}

fn valid_symfony_symbol_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| is_symfony_symbol_char(byte) || byte == b'\\')
}

fn is_symfony_symbol_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b':' | b'/' | b'\\')
}

fn scan_doctrine_repository_mappings(uri: &str, content: &str) -> Vec<DoctrineRepositoryMapping> {
    let mut mappings = Vec::new();
    scan_doctrine_yaml_repository_mappings(uri, content, &mut mappings);
    scan_doctrine_xml_repository_mappings(uri, content, &mut mappings);
    mappings
}

fn scan_doctrine_yaml_repository_mappings(
    uri: &str,
    content: &str,
    mappings: &mut Vec<DoctrineRepositoryMapping>,
) {
    let lines = line_offsets(content);
    for (idx, (line_start, line)) in lines.iter().enumerate() {
        let Some((entity_fqn, entity_start, entity_end, entity_indent)) =
            yaml_doctrine_entity_key(line, *line_start)
        else {
            continue;
        };

        for (child_start, child_line) in lines.iter().skip(idx + 1) {
            let trimmed = child_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let child_indent = leading_spaces(child_line);
            if child_indent <= entity_indent {
                break;
            }

            if let Some((repository_fqn, repository_start, repository_end)) =
                yaml_repository_class_value(child_line, *child_start)
            {
                mappings.push(DoctrineRepositoryMapping {
                    uri: uri.to_string(),
                    entity_fqn: entity_fqn.clone(),
                    entity_start: entity_start as u32,
                    entity_end: entity_end as u32,
                    repository_fqn,
                    repository_start: repository_start as u32,
                    repository_end: repository_end as u32,
                });
                break;
            }
        }
    }
}

fn scan_doctrine_xml_repository_mappings(
    uri: &str,
    content: &str,
    mappings: &mut Vec<DoctrineRepositoryMapping>,
) {
    let mut search = 0usize;
    let lower = content.to_ascii_lowercase();
    while let Some(rel_start) = lower[search..].find("<entity") {
        let tag_start = search + rel_start;
        let Some(rel_end) = content[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + rel_end + 1;
        let tag = &content[tag_start..tag_end];

        let entity = xml_attr_value(tag, tag_start, &["name", "class"]);
        let repository = xml_attr_value(tag, tag_start, &["repository-class", "repositoryclass"]);
        if let (
            Some((entity_fqn, entity_start, entity_end)),
            Some((repo_fqn, repo_start, repo_end)),
        ) = (entity, repository)
            && valid_framework_name(&normalize_framework_fqn(&entity_fqn))
            && valid_framework_name(&normalize_framework_fqn(&repo_fqn))
        {
            mappings.push(DoctrineRepositoryMapping {
                uri: uri.to_string(),
                entity_fqn: normalize_framework_fqn(&entity_fqn),
                entity_start: entity_start as u32,
                entity_end: entity_end as u32,
                repository_fqn: normalize_framework_fqn(&repo_fqn),
                repository_start: repo_start as u32,
                repository_end: repo_end as u32,
            });
        }

        search = tag_end;
    }
}

fn yaml_doctrine_entity_key(
    line: &str,
    line_start: usize,
) -> Option<(String, usize, usize, usize)> {
    let indent = leading_spaces(line);
    let trimmed = line[indent..].trim_end();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
        return None;
    }

    let colon = trimmed.find(':')?;
    let raw_key = trimmed[..colon].trim();
    let (key, quote_adjust) = strip_yaml_quotes(raw_key);
    let normalized = normalize_framework_fqn(key);
    if !normalized.contains('\\') || !valid_framework_name(&normalized) {
        return None;
    }

    let raw_start = line[indent..].find(raw_key)? + indent;
    let start = line_start + raw_start + quote_adjust.0;
    let end = line_start + raw_start + raw_key.len().saturating_sub(quote_adjust.1);
    Some((normalized, start, end, indent))
}

fn yaml_repository_class_value(line: &str, line_start: usize) -> Option<(String, usize, usize)> {
    let colon = line.find(':')?;
    let raw_key = line[..colon].trim();
    let (key, _) = strip_yaml_quotes(raw_key);
    if !matches!(
        key,
        "repositoryClass" | "repository-class" | "repository_class"
    ) {
        return None;
    }

    let raw = line[colon + 1..].trim_start();
    let value_offset = line[colon + 1..].len() - raw.len();
    let (value, start, end) = scalar_value(raw, line_start + colon + 1 + value_offset)?;
    let normalized = normalize_framework_fqn(value);
    if normalized.contains('\\') && valid_framework_name(&normalized) {
        Some((normalized, start, end))
    } else {
        None
    }
}

fn xml_attr_value(tag: &str, tag_start: usize, names: &[&str]) -> Option<(String, usize, usize)> {
    let bytes = tag.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len()
            && (bytes[i] == b'-' || bytes[i] == b'_' || bytes[i].is_ascii_alphanumeric())
        {
            i += 1;
        }
        if i == name_start {
            i += 1;
            continue;
        }
        let attr_name = tag[name_start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let quote = *bytes.get(i)?;
        if quote != b'\'' && quote != b'"' {
            continue;
        }
        let value_start = i + 1;
        i = value_start;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if names
            .iter()
            .any(|name| attr_name == name.to_ascii_lowercase())
        {
            let value = tag[value_start..i].to_string();
            return Some((value, tag_start + value_start, tag_start + i));
        }
        i += 1;
    }
    None
}

fn strip_yaml_quotes(raw: &str) -> (&str, (usize, usize)) {
    let bytes = raw.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'))
    {
        (&raw[1..raw.len() - 1], (1, 1))
    } else {
        (raw, (0, 0))
    }
}

fn leading_spaces(line: &str) -> usize {
    line.bytes().take_while(|b| *b == b' ').count()
}

fn push_unique_string(out: &mut Vec<String>, value: String) {
    if !out.iter().any(|known| known.eq_ignore_ascii_case(&value)) {
        out.push(value);
    }
}

fn scan_class_like_tokens(uri: &str, content: &str, refs: &mut Vec<FrameworkReference>) {
    let bytes = content.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if !is_token_start(bytes[i]) || (i > 0 && is_token_char(bytes[i - 1])) {
            i += 1;
            continue;
        }

        let start = i;
        let mut end = i + 1;
        while end < bytes.len() && is_token_char(bytes[end]) {
            end += 1;
        }

        let token = &content[start..end];
        let normalized = normalize_framework_fqn(token);
        let token_has_namespace_separator = token.contains('\\');
        if token_has_namespace_separator && valid_framework_name(&normalized) {
            if token.ends_with('\\') || token.ends_with("\\\\") {
                let prefix = normalized.trim_end_matches('\\').to_string();
                if !prefix.is_empty() && valid_framework_name(&prefix) {
                    refs.push(FrameworkReference {
                        uri: uri.to_string(),
                        start: start as u32,
                        end: end as u32,
                        kind: FrameworkReferenceKind::Namespace { prefix },
                    });
                }
            } else {
                refs.push(FrameworkReference {
                    uri: uri.to_string(),
                    start: start as u32,
                    end: end as u32,
                    kind: FrameworkReferenceKind::Class {
                        fqn: normalized.clone(),
                    },
                });

                if bytes.get(end) == Some(&b':') && bytes.get(end + 1) == Some(&b':') {
                    let method_start = end + 2;
                    let method_end = scan_identifier(bytes, method_start);
                    if method_end > method_start {
                        refs.push(FrameworkReference {
                            uri: uri.to_string(),
                            start: method_start as u32,
                            end: method_end as u32,
                            kind: FrameworkReferenceKind::Method {
                                class_fqn: normalized,
                                member_name: content[method_start..method_end].to_string(),
                            },
                        });
                    }
                }
            }
        }

        i = end;
    }
}

fn scan_path_scalars(uri: &str, content: &str, refs: &mut Vec<FrameworkReference>) {
    for (line_start, line) in line_offsets(content) {
        let Some(colon) = line.find(':') else {
            continue;
        };
        let key = line[..colon].trim();
        if !matches!(
            key,
            "resource" | "exclude" | "path" | "paths" | "dir" | "directory"
        ) {
            continue;
        }
        let raw = line[colon + 1..].trim_start();
        let value_offset = line[colon + 1..].len() - raw.len();
        if let Some((value, start, end)) = scalar_value(raw, line_start + colon + 1 + value_offset)
            && looks_like_path_value(value)
        {
            refs.push(FrameworkReference {
                uri: uri.to_string(),
                start: start as u32,
                end: end as u32,
                kind: FrameworkReferenceKind::Path {
                    value: value.to_string(),
                },
            });
        }
    }

    for attr in ["resource", "exclude", "path", "dir", "directory"] {
        let mut search = 0usize;
        let pattern = format!("{attr}=");
        while let Some(pos) = content[search..].find(&pattern) {
            let attr_start = search + pos + pattern.len();
            if let Some((value, start, end)) = quoted_value_at(content, attr_start)
                && looks_like_path_value(value)
            {
                refs.push(FrameworkReference {
                    uri: uri.to_string(),
                    start: start as u32,
                    end: end as u32,
                    kind: FrameworkReferenceKind::Path {
                        value: value.to_string(),
                    },
                });
            }
            search = attr_start.saturating_add(1);
        }
    }
}

fn line_offsets(content: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for line in content.lines() {
        out.push((offset, line));
        offset += line.len() + 1;
    }
    out
}

fn scalar_value(raw: &str, absolute_start: usize) -> Option<(&str, usize, usize)> {
    if raw.is_empty() || raw.starts_with('#') {
        return None;
    }
    let bytes = raw.as_bytes();
    if matches!(bytes.first(), Some(b'"' | b'\'')) {
        let quote = bytes[0];
        let mut i = 1usize;
        while i < bytes.len() {
            if bytes[i] == quote {
                return Some((&raw[1..i], absolute_start + 1, absolute_start + i));
            }
            i += 1;
        }
        return None;
    }
    let end = raw.find('#').unwrap_or(raw.len());
    let value = raw[..end].trim_end();
    if value.is_empty() {
        None
    } else {
        Some((value, absolute_start, absolute_start + value.len()))
    }
}

fn quoted_value_at(content: &str, offset: usize) -> Option<(&str, usize, usize)> {
    let bytes = content.as_bytes();
    let quote = *bytes.get(offset)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let mut i = offset + 1;
    while i < bytes.len() {
        if bytes[i] == quote {
            return Some((&content[offset + 1..i], offset + 1, i));
        }
        i += 1;
    }
    None
}

fn looks_like_path_value(value: &str) -> bool {
    value.contains('/')
        && !value.contains("://")
        && (value.starts_with('.')
            || value.starts_with('/')
            || value.contains("src/")
            || value.contains("%kernel.project_dir%"))
}

fn is_token_start(byte: u8) -> bool {
    byte == b'\\' || byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_token_char(byte: u8) -> bool {
    byte == b'\\' || byte == b'_' || byte.is_ascii_alphanumeric()
}

fn scan_identifier(bytes: &[u8], start: usize) -> usize {
    if !bytes
        .get(start)
        .is_some_and(|b| *b == b'_' || b.is_ascii_alphabetic())
    {
        return start;
    }
    let mut end = start + 1;
    while end < bytes.len() && (bytes[end] == b'_' || bytes[end].is_ascii_alphanumeric()) {
        end += 1;
    }
    end
}

pub(crate) fn normalize_framework_fqn(name: &str) -> String {
    let mut out = String::new();
    let mut prev_backslash = false;
    for ch in strip_fqn_prefix(name.trim()).chars() {
        if ch == '\\' {
            if !prev_backslash {
                out.push('\\');
            }
            prev_backslash = true;
        } else {
            out.push(ch);
            prev_backslash = false;
        }
    }
    out.trim_end_matches('\\').to_string()
}

fn framework_fqn_lookup_key(name: &str) -> String {
    let mut key = normalize_framework_fqn(name);
    key.make_ascii_lowercase();
    key
}

fn valid_framework_name(name: &str) -> bool {
    let name = name.trim_matches('\\');
    if name.is_empty() {
        return false;
    }
    name.split('\\').all(valid_framework_segment)
}

fn valid_framework_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub(crate) fn short_segment_range(source: &str, absolute_start: u32) -> (u32, u32) {
    let trimmed = source.trim_end_matches('\\');
    let short_start = trimmed.rfind('\\').map(|idx| idx + 1).unwrap_or(0);
    let start = absolute_start + short_start as u32;
    let end = absolute_start + trimmed.len() as u32;
    (start, end)
}

pub(crate) fn namespace_segment_range_at_offset(
    source: &str,
    absolute_start: u32,
    cursor: u32,
) -> Option<(usize, u32, u32)> {
    let bytes = source.as_bytes();
    let mut source_offset = 0usize;
    let mut segment_idx = 0usize;
    while source_offset < bytes.len() {
        while source_offset < bytes.len() && bytes[source_offset] == b'\\' {
            source_offset += 1;
        }
        if source_offset >= bytes.len() {
            break;
        }
        let segment_start = source_offset;
        while source_offset < bytes.len() && bytes[source_offset] != b'\\' {
            source_offset += 1;
        }
        let start = absolute_start + segment_start as u32;
        let end = absolute_start + source_offset as u32;
        if cursor >= start && cursor <= end {
            return Some((segment_idx, start, end));
        }
        segment_idx += 1;
    }
    None
}

fn rewrite_framework_fqn_literal(source: &str, replacement: &str) -> String {
    let mut out = replacement.to_string();
    if source.starts_with('\\') && !out.starts_with('\\') {
        out.insert(0, '\\');
    }
    if source.contains("\\\\") {
        out = out.replace('\\', "\\\\");
    }
    if source.ends_with('\\') || source.ends_with("\\\\") {
        out.push('\\');
        if source.ends_with("\\\\") {
            out.push('\\');
        }
    }
    out
}

fn rewrite_framework_path_for_directory_renames(
    value: &str,
    file_dir: &Path,
    workspace_root: Option<&Path>,
    renames: &[(PathBuf, PathBuf)],
) -> Option<String> {
    let resolved = resolve_framework_path_value(value, file_dir, workspace_root)?;
    for (old_dir, new_dir) in renames {
        if !resolved.starts_with(old_dir) {
            continue;
        }

        let suffix = resolved.strip_prefix(old_dir).ok()?;
        let target = normalize_path(new_dir.join(suffix));
        return format_rewritten_framework_path(value, file_dir, workspace_root, &target);
    }
    None
}

fn resolve_framework_path_value(
    value: &str,
    file_dir: &Path,
    workspace_root: Option<&Path>,
) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if let Some(root) = workspace_root
        && let Some(rest) = value.strip_prefix("%kernel.project_dir%")
    {
        let rest = rest.trim_start_matches(['/', '\\']);
        return Some(normalize_path(root.join(rest)));
    }

    let path = PathBuf::from(value);
    if path.is_absolute() {
        Some(normalize_path(path))
    } else {
        Some(normalize_path(file_dir.join(path)))
    }
}

fn format_rewritten_framework_path(
    original: &str,
    file_dir: &Path,
    workspace_root: Option<&Path>,
    target: &Path,
) -> Option<String> {
    let mut rewritten = if original.trim().starts_with("%kernel.project_dir%") {
        let root = workspace_root?;
        let relative = target.strip_prefix(root).ok()?;
        let relative = path_to_slash(relative);
        if relative.is_empty() {
            "%kernel.project_dir%".to_string()
        } else {
            format!("%kernel.project_dir%/{relative}")
        }
    } else if Path::new(original.trim()).is_absolute() {
        path_to_slash(target)
    } else {
        let relative = relative_path(file_dir, target)?;
        path_to_slash(&relative)
    };

    if (original.ends_with('/') || original.ends_with('\\')) && !rewritten.ends_with('/') {
        rewritten.push('/');
    }
    Some(rewritten)
}

fn relative_path(from_dir: &Path, target: &Path) -> Option<PathBuf> {
    let from_dir = normalize_path(from_dir.to_path_buf());
    let target = normalize_path(target.to_path_buf());
    let from_components: Vec<Component<'_>> = from_dir.components().collect();
    let target_components: Vec<Component<'_>> = target.components().collect();

    let mut common_len = 0usize;
    while common_len < from_components.len()
        && common_len < target_components.len()
        && from_components[common_len] == target_components[common_len]
    {
        common_len += 1;
    }

    if common_len == 0 && (from_dir.is_absolute() || target.is_absolute()) {
        return None;
    }

    let mut relative = PathBuf::new();
    for component in &from_components[common_len..] {
        if matches!(component, Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &target_components[common_len..] {
        relative.push(component.as_os_str());
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Some(relative)
}

fn path_to_slash(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn sort_locations(locations: &mut Vec<Location>) {
    locations.sort_by(|a, b| {
        a.uri
            .as_str()
            .cmp(b.uri.as_str())
            .then(a.range.start.line.cmp(&b.range.start.line))
            .then(a.range.start.character.cmp(&b.range.start.character))
    });
    locations.dedup();
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_segments_count_only_names_in_escaped_and_rooted_spellings() {
        // `App\\Domain\\` as a quoted YAML string: `Domain` is segment 1.
        let escaped = "App\\\\Domain\\\\";
        assert_eq!(
            namespace_segment_range_at_offset(escaped, 10, 16),
            Some((1, 15, 21))
        );
        // `\App\Domain`: the leading separator is not a segment.
        assert_eq!(
            namespace_segment_range_at_offset("\\App\\Domain", 0, 2),
            Some((0, 1, 4))
        );
    }

    #[test]
    fn doctrine_repository_index_updates_with_framework_resource() {
        let backend = Backend::new_test();
        let uri = "file:///project/config/doctrine/User.orm.yaml";
        backend.index_framework_uri_content(
            uri,
            "App\\Entity\\User:\n  repositoryClass: App\\Repository\\UserRepository\n",
        );
        assert_eq!(
            backend.framework_doctrine_repository_fqns_for_entity("App\\Entity\\User"),
            vec!["App\\Repository\\UserRepository"]
        );

        backend.index_framework_uri_content(
            uri,
            "App\\Entity\\User:\n  repositoryClass: App\\Storage\\UserStore\n",
        );
        assert_eq!(
            backend.framework_doctrine_repository_fqns_for_entity("App\\Entity\\User"),
            vec!["App\\Storage\\UserStore"]
        );
        assert!(
            backend
                .framework_doctrine_entity_fqns_for_repository("App\\Repository\\UserRepository")
                .is_empty()
        );

        backend.remove_framework_uri(uri);
        assert!(
            backend
                .framework_doctrine_repository_fqns_for_entity("App\\Entity\\User")
                .is_empty()
        );
    }

    #[test]
    fn framework_reference_lookup_updates_and_removes_one_resource() {
        let backend = Backend::new_test();
        let uri = "file:///project/config/routes.yaml";
        backend.index_framework_uri_content(
            uri,
            "home:\n  path: /\n  controller: App\\Controller\\HomeController::index\n",
        );

        assert_eq!(
            backend
                .framework_class_reference_locations("app\\controller\\homecontroller")
                .len(),
            1
        );
        assert_eq!(
            backend
                .framework_member_reference_locations("index", None)
                .len(),
            1
        );

        backend.index_framework_uri_content(
            uri,
            "admin:\n  path: /admin\n  controller: App\\Controller\\AdminController::dashboard\n",
        );
        assert!(
            backend
                .framework_member_reference_locations("index", None)
                .is_empty()
        );
        assert_eq!(
            backend
                .framework_member_reference_locations("dashboard", None)
                .len(),
            1
        );

        backend.remove_framework_uri(uri);
        assert!(
            backend
                .framework_class_reference_locations("App\\Controller\\AdminController")
                .is_empty()
        );
        assert!(
            backend
                .framework_member_reference_locations("dashboard", None)
                .is_empty()
        );
    }

    #[test]
    fn php_call_context_handles_multibyte_search_boundary() {
        let content = format!("─{} service('app.mailer')", "x".repeat(2037));
        let quote_start = content.find("'app.mailer").unwrap();
        let call = php_call_context(&content, quote_start).unwrap();

        assert_eq!(call.name, "service");
        assert_eq!(call.argument_index, 0);
    }

    #[test]
    fn php_symfony_scanner_ignores_whitespace_only_literal() {
        let content = "<?php service(' ');";
        let mut refs = Vec::new();
        let literals = scan_php_string_literals_and_class_constants(
            "file:///test.php",
            content,
            &HashMap::new(),
            &None,
            false,
            &mut refs,
        );

        scan_php_symfony_literal("file:///test.php", content, &literals, 0, false, &mut refs);

        assert!(refs.is_empty());
    }
}
