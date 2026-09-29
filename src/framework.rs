//! Symfony and Doctrine resource-file reference indexing.
//!
//! PHPantom's normal [`SymbolMap`](crate::symbol_map::SymbolMap) is built from
//! PHP ASTs, so YAML/XML framework resources need a parallel lightweight index
//! if they are going to participate in go-to-definition, find-references,
//! rename, and namespace/folder refactors.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use tower_lsp::lsp_types::{
    DocumentHighlight, DocumentHighlightKind, Location, Position, Range, TextEdit, Url,
};

use crate::Backend;
use crate::text_position::{LineIndex, offset_to_position, position_to_offset};
use crate::util::strip_fqn_prefix;

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

/// Whether `path` sits in a dependency, VCS, or generated directory of the
/// workspace. Only the part below `root` is inspected: a workspace that itself
/// lives under `/var/www` or a temp directory is not a cache directory.
fn is_skipped_resource_path(root: Option<&Path>, path: &Path) -> bool {
    let relative = root
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path);
    relative.components().any(|component| match component {
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
        let line_index = LineIndex::new(content);
        let mut keys = FrameworkLookupUriKeys::default();
        for reference in references {
            let location = IndexedFrameworkLocation {
                uri: Arc::clone(&uri),
                range: Range::new(
                    line_index.position(reference.start as usize),
                    line_index.position(reference.end as usize),
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
                FrameworkReferenceKind::Namespace { .. } | FrameworkReferenceKind::Path { .. } => {}
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

    /// Scan all YAML/XML framework resources under the workspace root.
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
            if !is_framework_resource_path(path) || is_skipped_resource_path(Some(&root), path) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            let uri = crate::util::path_to_uri(path);
            let mappings = scan_doctrine_repository_mappings(&uri, &content);
            if !mappings.is_empty() {
                doctrine_repositories.insert(uri.clone(), Arc::new(mappings));
            }
            let refs = scan_framework_references(&uri, &content);
            if !refs.is_empty() {
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
        if !is_framework_resource_uri(uri) {
            return;
        }
        let refs = scan_framework_references(uri, content);
        let mappings = scan_doctrine_repository_mappings(uri, content);
        let mut index = self.framework_references.write();
        let mut lookup = self.framework_reference_lookup.write();
        if refs.is_empty() {
            index.remove(uri);
            Self::remove_framework_lookup_uri(&mut lookup, uri);
        } else {
            Self::replace_framework_lookup_uri(&mut lookup, uri, content, &refs);
            index.insert(uri.to_string(), Arc::new(refs));
        }
        let mut doctrine_repositories = self.framework_doctrine_repositories.write();
        if mappings.is_empty() {
            doctrine_repositories.remove(uri);
        } else {
            doctrine_repositories.insert(uri.to_string(), Arc::new(mappings));
        }
    }

    pub(crate) fn reindex_framework_uri_from_disk(&self, uri: &str) {
        if !is_framework_resource_uri(uri) {
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
        let root = self.workspace.workspace_root.read().clone();
        if !is_framework_resource_path(path) || is_skipped_resource_path(root.as_deref(), path) {
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
        if !is_framework_resource_uri(uri) {
            return None;
        }

        let offset = position_to_offset(content, position);
        let refs = self
            .framework_references
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_else(|| Arc::new(scan_framework_references(uri, content)));

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
            .unwrap_or_else(|| Arc::new(scan_framework_references(uri, content)));

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
    /// alone instead of cancelling the edits around it.
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
                changes
                    .entry(parsed_uri.clone())
                    .or_default()
                    .push(TextEdit {
                        range: Range {
                            start: index.position(reference.start as usize),
                            end: index.position(reference.end as usize),
                        },
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
}

fn scan_framework_references(uri: &str, content: &str) -> Vec<FrameworkReference> {
    let mut refs = Vec::new();
    scan_class_like_tokens(uri, content, &mut refs);
    scan_path_scalars(uri, content, &mut refs);
    refs.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
    refs.dedup();
    refs
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
    let normalized_source = source.trim_end_matches('\\');
    let mut offset = absolute_start;
    // An escaped `App\\Service` splits into an empty piece between the two
    // backslashes; only the non-empty pieces are namespace segments.
    let mut idx = 0;
    for segment in normalized_source.split('\\') {
        if segment.is_empty() {
            offset += 1;
            continue;
        }
        let end = offset + segment.len() as u32;
        if cursor >= offset && cursor <= end {
            return Some((idx, offset, end));
        }
        offset = end + 1;
        idx += 1;
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
    fn workspace_inside_a_var_directory_is_indexed_but_its_own_cache_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("var/www/app");
        let yaml = "home:\n  path: /\n  controller: App\\Controller\\HomeController::index\n";
        for relative in ["config/routes.yaml", "var/cache/dev/routes.yaml"] {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, yaml).unwrap();
        }

        let backend = Backend::new_test();
        *backend.workspace.workspace_root.write() = Some(root.clone());

        assert_eq!(backend.index_framework_workspace(), 1);
        let indexed: Vec<String> = backend
            .framework_references
            .read()
            .keys()
            .cloned()
            .collect();
        assert!(indexed[0].ends_with("/config/routes.yaml"), "{indexed:?}");
    }

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
}
