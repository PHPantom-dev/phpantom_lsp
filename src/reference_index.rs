//! Cross-file reference candidate index.
//!
//! The precise find-references logic still lives in `references`: it resolves
//! aliases, class hierarchies, `self/static/parent`, and Laravel declarations.
//! This index is intentionally a coarse candidate index keyed by symbol name so
//! those scanners can skip files that cannot contain a match once the workspace
//! has been fully parsed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use parking_lot::RwLock;

use crate::Backend;
use crate::atom::{AtomMap, AtomSet, atom};
use crate::class_lookup::find_class_at_offset;
use crate::symbol_map::{LaravelStringKind, SelfStaticParentKind, SymbolKind, SymbolMap};
use crate::util::{build_fqn, short_name, strip_fqn_prefix};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ReferenceIndexKey {
    Class(String),
    Function(String),
    Constant(String),
    Member {
        name: String,
        is_static: bool,
    },
    LaravelString {
        kind: LaravelStringKind,
        key: String,
    },
}

/// Build the one index identity shared by a Laravel string-key usage and its
/// config-backed aliases.
///
/// Resource spans deliberately retain their short source text (for example,
/// `redis`), while the reference index stores the canonical config address
/// (`cache.stores.redis`). This keeps one entry per semantic identity without
/// changing the exact source range consumers return.
pub(crate) fn laravel_string_reference_key(
    kind: LaravelStringKind,
    key: &str,
) -> ReferenceIndexKey {
    match kind {
        LaravelStringKind::ConfigResource(resource)
            if !crate::symbol_map::laravel_resources::is_implicit_resource_name(resource, key) =>
        {
            ReferenceIndexKey::LaravelString {
                kind: LaravelStringKind::Config,
                key: crate::symbol_map::laravel_resources::config_key(resource, key),
            }
        }
        _ => ReferenceIndexKey::LaravelString {
            kind,
            key: key.to_string(),
        },
    }
}

impl ReferenceIndexKey {
    /// Memory-audit tooling: heap bytes held by the key's name string.
    #[cfg(feature = "mem-audit")]
    pub(crate) fn audit_heap(&self) -> usize {
        match self {
            Self::Class(s) | Self::Function(s) | Self::Constant(s) => s.capacity(),
            Self::Member { name, .. } => name.capacity(),
            Self::LaravelString { key, .. } => key.capacity(),
        }
    }
}

/// The candidate index plus a reverse map of which keys each URI has
/// entries under.
///
/// Per key, only the distinct contributing URIs are kept (as the shared
/// per-file `Arc<str>`) mapped to the number of spans in that URI that
/// reference the key's name — the two facts consumers actually read
/// (`reference_candidate_uris_for_keys` needs the URI set,
/// `inlay_hints::ref_count` needs the count). Declarations and the
/// alias keys a reference is merely searchable under contribute a URI
/// but no count, so a class is never credited with the references to a
/// namesake in another namespace. The per-span `start`/`end` offsets and
/// the raw span multiplicity are never read anywhere, so they are not
/// stored.
///
/// The reverse map keeps per-file eviction proportional to that file's
/// own contributions.  Without it, evicting a URI means scanning every
/// entry of every key in the index — an O(whole-index) pass under the
/// write lock that serialises the parallel indexing workers (each file
/// they publish evicts itself first) and grows with workspace size on
/// every keystroke.
#[derive(Default)]
pub(crate) struct ReferenceIndexInner {
    by_key: HashMap<ReferenceIndexKey, HashMap<Arc<str>, u32>>,
    uri_keys: HashMap<Arc<str>, Vec<ReferenceIndexKey>>,
}

impl ReferenceIndexInner {
    pub(crate) fn get(&self, key: &ReferenceIndexKey) -> Option<&HashMap<Arc<str>, u32>> {
        self.by_key.get(key)
    }

    /// Memory-audit tooling: expose the internal maps for deep-size
    /// accounting.
    #[allow(clippy::type_complexity)]
    #[cfg(feature = "mem-audit")]
    pub(crate) fn audit_maps(
        &self,
    ) -> (
        &HashMap<ReferenceIndexKey, HashMap<Arc<str>, u32>>,
        &HashMap<Arc<str>, Vec<ReferenceIndexKey>>,
    ) {
        (&self.by_key, &self.uri_keys)
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

pub(crate) type ReferenceIndex = Arc<RwLock<ReferenceIndexInner>>;

pub(crate) fn new_reference_index() -> ReferenceIndex {
    Arc::new(RwLock::new(ReferenceIndexInner::default()))
}

impl Backend {
    pub(crate) fn evict_reference_index_uri(&self, uri: &str) {
        let track_members = !self.member_ref_counts.is_empty();
        let mut index = self.reference_index.write();
        let dropped = if track_members {
            member_contributions(&index, uri)
        } else {
            AtomMap::default()
        };
        evict_reference_index_uri_locked(&mut index, uri);
        drop(index);
        for name in dropped.into_keys() {
            self.member_ref_counts.invalidate_member(name);
        }
        self.forget_class_shape(uri);
    }

    pub(crate) fn reference_candidate_uris_for_keys(
        &self,
        keys: &[ReferenceIndexKey],
    ) -> Option<HashSet<Arc<str>>> {
        if self.skip_reference_index || !self.workspace_indexed.load(Ordering::Acquire) {
            return None;
        }

        let index = self.reference_index.read();
        let mut uris = HashSet::new();
        for key in keys {
            if let Some(entries) = index.get(key) {
                uris.extend(entries.keys().cloned());
            }
        }
        Some(uris)
    }

    /// Narrow `uris` to the files that reference one of `keys`.
    ///
    /// A file the index does not track at all is kept: the index skips
    /// vendor files and stubs, and only records a file once it has been
    /// parsed, so an absent entry means "references unknown" rather than
    /// "references nothing".  Returns `false` without touching `uris` when
    /// the index cannot answer yet (the workspace is still being indexed).
    pub(crate) fn retain_reference_candidates(
        &self,
        keys: &[ReferenceIndexKey],
        uris: &mut Vec<String>,
    ) -> bool {
        if !self.workspace_indexed.load(Ordering::Acquire) {
            return false;
        }

        let index = self.reference_index.read();
        let mut candidates = HashSet::new();
        for key in keys {
            if let Some(entries) = index.get(key) {
                candidates.extend(entries.keys().map(Arc::as_ref));
            }
        }
        uris.retain(|uri| {
            candidates.contains(uri.as_str()) || !index.uri_keys.contains_key(uri.as_str())
        });
        true
    }

    pub(crate) fn reindex_references_for_symbol_maps_batch(
        &self,
        items: Vec<(String, Arc<SymbolMap>)>,
    ) {
        if items.is_empty() || self.skip_reference_index {
            return;
        }

        // Nothing is cached until a declaration hint has been asked for,
        // and until then the invalidation bookkeeping below is pure cost:
        // the whole workspace index publishes through here.
        let track_members = !self.member_ref_counts.is_empty();

        let mut rebuilt = Vec::with_capacity(items.len());
        for (uri, symbol_map) in items {
            if self.is_reference_indexable_uri(&uri) {
                // A class that gains a parent, an interface, or a trait
                // moves which accesses belong to which declaration, and
                // that is not visible in the per-file key deltas below.
                if track_members && self.class_shape_changed(&uri) {
                    self.member_ref_counts.invalidate_all();
                }
                rebuilt.push((
                    uri.clone(),
                    self.reference_entries_for_symbol_map(&uri, &symbol_map),
                ));
            } else {
                rebuilt.push((uri, Vec::new()));
            }
        }

        let mut keep = vec![true; rebuilt.len()];
        let mut seen_uris = HashSet::new();
        for (idx, (uri, _)) in rebuilt.iter().enumerate().rev() {
            if !seen_uris.insert(uri.clone()) {
                keep[idx] = false;
            }
        }
        rebuilt = rebuilt
            .into_iter()
            .enumerate()
            .filter_map(|(idx, item)| keep[idx].then_some(item))
            .collect();

        // Which member names each file contributed a reference to, so the
        // counts cached for those members can be marked stale.  Only the
        // members whose contribution actually changed are invalidated:
        // editing a method body without touching an access leaves every
        // count valid, and recomputing one is expensive.
        let mut stale = AtomSet::default();

        let mut index = self.reference_index.write();
        let mut previous = Vec::with_capacity(if track_members { rebuilt.len() } else { 0 });
        for (uri, _) in &rebuilt {
            if track_members {
                previous.push(member_contributions(&index, uri));
            }
            evict_reference_index_uri_locked(&mut index, uri);
        }
        for (idx, (uri, entries)) in rebuilt.into_iter().enumerate() {
            if track_members {
                let old = &previous[idx];
                let new = member_contributions_of_entries(&entries);
                stale.extend(
                    old.iter()
                        .filter(|(name, count)| new.get(*name) != Some(count))
                        .map(|(name, _)| *name),
                );
                stale.extend(
                    new.iter()
                        .filter(|(name, count)| old.get(*name) != Some(count))
                        .map(|(name, _)| *name),
                );
            }

            if entries.is_empty() {
                continue;
            }
            let uri: Arc<str> = Arc::from(uri.as_str());
            let mut keys: HashSet<ReferenceIndexKey> = HashSet::new();
            for (key, counts_as_reference) in entries {
                let count = index
                    .by_key
                    .entry(key.clone())
                    .or_default()
                    .entry(Arc::clone(&uri))
                    .or_insert(0);
                if counts_as_reference {
                    *count += 1;
                }
                keys.insert(key);
            }
            index.uri_keys.insert(uri, keys.into_iter().collect());
        }
        drop(index);

        for name in stale {
            self.member_ref_counts.invalidate_member(name);
        }
    }

    /// Build the `(key, counts_as_reference)` pairs a file contributes to
    /// the index. The caller interns the URI once per file and shares it
    /// across every key, rather than allocating a fresh `String` per span.
    ///
    /// A span joins the index under every name it could be searched by,
    /// but only one of those names is the symbol it actually names: a
    /// reference to `App\Widget` is a candidate under `Widget` too, and
    /// counting it there would credit a global `\Widget` with it.  So the
    /// count rides on the resolved name alone, and a declaration counts
    /// under no name at all.
    fn reference_entries_for_symbol_map(
        &self,
        uri: &str,
        symbol_map: &SymbolMap,
    ) -> Vec<(ReferenceIndexKey, bool)> {
        if !self.is_reference_indexable_uri(uri) {
            return Vec::new();
        }

        let mut entries: Vec<(ReferenceIndexKey, bool)> = Vec::new();
        for span in &symbol_map.spans {
            let is_declaration = matches!(
                &span.kind,
                SymbolKind::ClassDeclaration { .. }
                    | SymbolKind::FunctionCall {
                        is_definition: true,
                        ..
                    }
                    | SymbolKind::MemberDeclaration { .. }
            );

            for (key, is_resolved_name) in self.reference_keys_for_span(uri, span) {
                entries.push((key, is_resolved_name && !is_declaration));
            }
        }

        // A render site behind a typed receiver is only settled lazily, and
        // whoever wants the answer has to find the file before it can be
        // asked for. The candidate joins the index unconfirmed: this index
        // narrows a workspace scan to the files that *might* hold a key,
        // and every consumer of it re-checks what it found.
        for site in &symbol_map.view_receiver_sites {
            entries.push((
                ReferenceIndexKey::LaravelString {
                    kind: crate::symbol_map::LaravelStringKind::View,
                    key: site.key.clone(),
                },
                false,
            ));
        }

        if let Some(classes) = self.symbols.uri_classes_index.read().get(uri).cloned() {
            for class in classes {
                for prop in &class.properties {
                    if prop.name_offset == 0 {
                        continue;
                    }
                    entries.push((
                        ReferenceIndexKey::Member {
                            name: prop
                                .name
                                .strip_prefix('$')
                                .unwrap_or(&prop.name)
                                .to_string(),
                            is_static: prop.is_static,
                        },
                        true,
                    ));
                }
            }
        }

        entries
    }

    /// The keys a span joins the index under, each paired with whether it
    /// is the name the span actually resolves to (see
    /// [`reference_entries_for_symbol_map`]).
    fn reference_keys_for_span(
        &self,
        uri: &str,
        span: &crate::symbol_map::SymbolSpan,
    ) -> Vec<(ReferenceIndexKey, bool)> {
        match &span.kind {
            SymbolKind::ClassReference { name, is_fqn, .. } => {
                let resolved = if *is_fqn {
                    normalize_symbol_name(name)
                } else if let Some(fqn) = self.resolved_name_at(uri, span.start) {
                    fqn
                } else {
                    let (use_map, namespace) = self.use_map_and_namespace_at(uri, span.start);
                    normalize_symbol_name(Self::resolve_to_fqn(name, &use_map, &namespace))
                };
                class_keys(&resolved, name)
            }
            SymbolKind::ClassDeclaration { name } => {
                let namespace = self.namespace_at_offset(uri, span.start);
                let fqn = build_fqn(name, namespace.as_deref());
                class_keys(&fqn, name)
            }
            SymbolKind::SelfStaticParent(kind) if *kind != SelfStaticParentKind::This => {
                let (classes, namespace) = self.classes_and_namespace_at(uri, span.start);
                let Some(current_class) = find_class_at_offset(&classes, span.start) else {
                    return Vec::new();
                };
                let fqn = match kind {
                    SelfStaticParentKind::Parent => {
                        current_class.parent_class.map(normalize_symbol_name)
                    }
                    _ => Some(build_fqn(&current_class.name, namespace.as_deref())),
                };
                fqn.map(|name| class_keys(&name, short_name(&name)))
                    .unwrap_or_default()
            }
            SymbolKind::FunctionCall { name, .. } => {
                let resolved = if let Some(fqn) = self.resolved_name_at(uri, span.start) {
                    fqn
                } else {
                    let (use_map, namespace) = self.use_map_and_namespace_at(uri, span.start);
                    normalize_symbol_name(Self::resolve_to_fqn(name, &use_map, &namespace))
                };
                function_keys(&resolved, name)
            }
            SymbolKind::ConstantReference { name, .. } => {
                let resolved = self.constant_fqn_at(uri, span.start, name);
                constant_keys(&resolved, name)
            }
            SymbolKind::MemberAccess {
                member_name,
                is_static,
                ..
            }
            | SymbolKind::MemberDeclaration {
                name: member_name,
                is_static,
            } => {
                vec![(
                    ReferenceIndexKey::Member {
                        name: member_name.to_string(),
                        is_static: *is_static,
                    },
                    true,
                )]
            }
            SymbolKind::LaravelStringKey { kind, key, .. } => {
                vec![(laravel_string_reference_key(*kind, key), true)]
            }
            _ => Vec::new(),
        }
    }

    fn is_reference_indexable_uri(&self, uri: &str) -> bool {
        if uri.starts_with("phpantom-stub://") || uri.starts_with("phpantom-stub-fn://") {
            return false;
        }
        !self
            .workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
    }

    fn resolved_name_at(&self, uri: &str, offset: u32) -> Option<String> {
        self.resolved_names
            .read()
            .get(uri)
            .and_then(|rn| rn.get(offset).map(normalize_symbol_name))
    }

    /// The key a function named at `offset` in `uri` is indexed under.
    ///
    /// Resolution lives here rather than at the asking end so a caller
    /// counting references cannot key the function differently from the
    /// way its call sites were keyed on the way in.
    pub(crate) fn function_reference_key(
        &self,
        uri: &str,
        offset: u32,
        name: &str,
    ) -> ReferenceIndexKey {
        ReferenceIndexKey::Function(self.function_fqn_at(uri, offset, name))
    }

    /// The fully-qualified name of the function named at `offset` in
    /// `uri`, with any leading `\` stripped.
    ///
    /// mago-names answers first because it resolves the way PHP does,
    /// including across a file's separate namespace blocks; the use-map
    /// and namespace are the fallback for a file it has no entry for.
    pub(crate) fn function_fqn_at(&self, uri: &str, offset: u32, name: &str) -> String {
        let resolved = if let Some(fqn) = self.resolved_name_at(uri, offset) {
            fqn
        } else {
            let (use_map, namespace) = self.use_map_and_namespace_at(uri, offset);
            Self::resolve_to_fqn(name, &use_map, &namespace)
        };
        normalize_symbol_name(resolved)
    }

    /// The fully-qualified name of the constant named at `offset` in
    /// `uri`, with any leading `\` stripped.
    ///
    /// Unlike a function, a constant falls back to the name as written
    /// rather than to a use-map guess.  The use-map is untyped -- it
    /// holds class, function, and const imports together -- and PHP
    /// resolves an unqualified constant by looking in the current
    /// namespace and then the global one, not through a class import.
    /// Guessing through it would key a constant under a name that names
    /// something else.
    pub(crate) fn constant_fqn_at(&self, uri: &str, offset: u32, name: &str) -> String {
        self.resolved_name_at(uri, offset)
            .unwrap_or_else(|| normalize_symbol_name(name))
    }
}

/// How many references to each member name a file currently contributes.
fn member_contributions(index: &ReferenceIndexInner, uri: &str) -> AtomMap<u32> {
    let mut counts = AtomMap::default();
    let Some(keys) = index.uri_keys.get(uri) else {
        return counts;
    };
    for key in keys {
        let ReferenceIndexKey::Member { name, .. } = key else {
            continue;
        };
        let count = index
            .by_key
            .get(key)
            .and_then(|entries| entries.get(uri))
            .copied()
            .unwrap_or(0);
        *counts.entry(atom(name)).or_insert(0) += count;
    }
    counts
}

/// The same tally, taken from the entries a file is about to be indexed
/// under rather than from the index itself.
fn member_contributions_of_entries(entries: &[(ReferenceIndexKey, bool)]) -> AtomMap<u32> {
    let mut counts = AtomMap::default();
    for (key, counts_as_reference) in entries {
        let ReferenceIndexKey::Member { name, .. } = key else {
            continue;
        };
        let entry = counts.entry(atom(name)).or_insert(0);
        if *counts_as_reference {
            *entry += 1;
        }
    }
    counts
}

fn evict_reference_index_uri_locked(index: &mut ReferenceIndexInner, uri: &str) {
    let Some(keys) = index.uri_keys.remove(uri) else {
        return;
    };
    for key in keys {
        if let Some(entries) = index.by_key.get_mut(&key) {
            entries.remove(uri);
            if entries.is_empty() {
                index.by_key.remove(&key);
            }
        }
    }
}

fn normalize_symbol_name(name: impl AsRef<str>) -> String {
    strip_fqn_prefix(name.as_ref()).to_string()
}

fn class_keys(resolved: &str, source_name: &str) -> Vec<(ReferenceIndexKey, bool)> {
    symbol_name_keys(resolved, source_name)
        .into_iter()
        .map(|(name, is_resolved)| (ReferenceIndexKey::Class(name), is_resolved))
        .collect()
}

fn function_keys(resolved: &str, source_name: &str) -> Vec<(ReferenceIndexKey, bool)> {
    symbol_name_keys(resolved, source_name)
        .into_iter()
        .map(|(name, is_resolved)| (ReferenceIndexKey::Function(name), is_resolved))
        .collect()
}

fn constant_keys(resolved: &str, source_name: &str) -> Vec<(ReferenceIndexKey, bool)> {
    symbol_name_keys(resolved, source_name)
        .into_iter()
        .map(|(name, is_resolved)| (ReferenceIndexKey::Constant(name), is_resolved))
        .collect()
}

/// Every name a symbol reference can be searched by, each flagged with
/// whether it is the name the reference resolves to.
fn symbol_name_keys(resolved: &str, source_name: &str) -> Vec<(String, bool)> {
    let resolved = normalize_symbol_name(resolved);
    let mut keys = vec![
        normalize_symbol_name(source_name),
        short_name(&resolved).to_string(),
    ];
    keys.sort();
    keys.dedup();
    keys.retain(|name| *name != resolved);
    let mut keys: Vec<(String, bool)> = keys.into_iter().map(|name| (name, false)).collect();
    keys.push((resolved, true));
    keys
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::Backend;
    use crate::symbol_map::{LaravelConfigResource, SymbolMap, SymbolSpan};

    #[test]
    fn candidate_lookup_is_disabled_until_workspace_is_indexed() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Foo.php";
        backend.update_ast(
            uri,
            "<?php\nnamespace App;\nclass Foo {}\n$foo = new Foo();\n",
        );

        let candidates = backend
            .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Class("App\\Foo".to_string())]);

        assert!(candidates.is_none());
    }

    #[test]
    fn candidate_lookup_is_disabled_when_reference_indexing_is_skipped() {
        let mut backend = Backend::new_test();
        backend.skip_reference_index = true;
        backend.workspace_indexed.store(true, Ordering::Release);

        let candidates = backend
            .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Class("App\\Foo".to_string())]);

        assert!(candidates.is_none());
    }

    #[test]
    fn reference_index_populates_candidates_from_parsed_file() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Foo.php";
        backend.update_ast(
            uri,
            "<?php\nnamespace App;\nclass Foo { public string $name; public function save(): void {} }\nfunction helper(): void {}\n$foo = new Foo();\n$foo->save();\nconfig('app.name');\n",
        );
        backend.workspace_indexed.store(true, Ordering::Release);

        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Class("App\\Foo".to_string()),
            uri,
        );
        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Function("App\\helper".to_string()),
            uri,
        );
        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Member {
                name: "save".to_string(),
                is_static: false,
            },
            uri,
        );
        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Member {
                name: "name".to_string(),
                is_static: false,
            },
            uri,
        );
        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::LaravelString {
                kind: LaravelStringKind::Config,
                key: "app.name".to_string(),
            },
            uri,
        );
    }

    #[test]
    fn config_resources_and_generic_config_keys_share_one_canonical_candidate_key() {
        let backend = Backend::new_test();
        backend.workspace_indexed.store(true, Ordering::Release);
        let resource_uri = "file:///project/src/CacheConsumer.php";
        let config_uri = "file:///project/src/ConfigConsumer.php";
        let queue_uri = "file:///project/src/QueueConsumer.php";

        backend.reindex_references_for_symbol_maps_batch(vec![
            (
                resource_uri.to_string(),
                laravel_string_map(
                    LaravelStringKind::ConfigResource(LaravelConfigResource::CacheStore),
                    "redis",
                ),
            ),
            (
                config_uri.to_string(),
                laravel_string_map(LaravelStringKind::Config, "cache.stores.redis"),
            ),
            (
                queue_uri.to_string(),
                laravel_string_map(
                    LaravelStringKind::ConfigResource(LaravelConfigResource::QueueConnection),
                    "redis",
                ),
            ),
        ]);

        let resource_key = laravel_string_reference_key(
            LaravelStringKind::ConfigResource(LaravelConfigResource::CacheStore),
            "redis",
        );
        let config_key =
            laravel_string_reference_key(LaravelStringKind::Config, "cache.stores.redis");
        assert_eq!(resource_key, config_key);
        assert_candidate_contains(&backend, config_key.clone(), resource_uri);
        assert_candidate_contains(&backend, config_key, config_uri);
        let queue_key = laravel_string_reference_key(
            LaravelStringKind::ConfigResource(LaravelConfigResource::QueueConnection),
            "redis",
        );
        assert_candidate_contains(&backend, queue_key.clone(), queue_uri);
        assert_candidate_not_contains(&backend, queue_key, resource_uri);

        let index = backend.reference_index.read();
        assert_eq!(index.by_key.len(), 2);
        assert_eq!(index.uri_keys.get(resource_uri).map(Vec::len), Some(1));
        assert_eq!(index.uri_keys.get(config_uri).map(Vec::len), Some(1));
        assert_eq!(index.uri_keys.get(queue_uri).map(Vec::len), Some(1));
    }

    #[test]
    fn non_config_laravel_string_keys_keep_their_original_identity() {
        assert_eq!(
            laravel_string_reference_key(LaravelStringKind::View, "dashboard"),
            ReferenceIndexKey::LaravelString {
                kind: LaravelStringKind::View,
                key: "dashboard".to_string(),
            }
        );
        assert_eq!(
            laravel_string_reference_key(
                LaravelStringKind::ConfigResource(LaravelConfigResource::CacheStore),
                "null",
            ),
            ReferenceIndexKey::LaravelString {
                kind: LaravelStringKind::ConfigResource(LaravelConfigResource::CacheStore),
                key: "null".to_string(),
            }
        );
        assert_eq!(
            laravel_string_reference_key(
                LaravelStringKind::ConfigResource(LaravelConfigResource::DatabaseConnection),
                "mysql::read",
            ),
            ReferenceIndexKey::LaravelString {
                kind: LaravelStringKind::Config,
                key: "database.connections.mysql".to_string(),
            }
        );
    }

    #[test]
    fn reference_index_evicts_candidates_when_file_maps_clear() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Foo.php";
        backend.update_ast(uri, "<?php\nnamespace App;\nclass Foo {}\nnew Foo();\n");
        backend.workspace_indexed.store(true, Ordering::Release);

        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Class("App\\Foo".to_string()),
            uri,
        );

        backend.clear_file_maps(uri);

        let candidates = backend
            .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Class("App\\Foo".to_string())]);
        assert!(candidates.unwrap().is_empty());
    }

    #[test]
    fn batch_reindex_replaces_existing_uri_and_keeps_last_duplicate() {
        let backend = Backend::new_test();
        backend.workspace_indexed.store(true, Ordering::Release);
        let uri = "file:///project/src/Foo.php".to_string();

        backend.reindex_references_for_symbol_maps_batch(vec![(
            uri.clone(),
            class_declaration_symbol_map("Old"),
        )]);
        assert_candidate_contains(&backend, ReferenceIndexKey::Class("Old".to_string()), &uri);

        backend.reindex_references_for_symbol_maps_batch(vec![
            (uri.clone(), class_declaration_symbol_map("First")),
            (uri.clone(), class_declaration_symbol_map("Second")),
        ]);

        assert_candidate_not_contains(&backend, ReferenceIndexKey::Class("Old".to_string()), &uri);
        assert_candidate_not_contains(
            &backend,
            ReferenceIndexKey::Class("First".to_string()),
            &uri,
        );
        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Class("Second".to_string()),
            &uri,
        );
    }

    #[test]
    fn batch_reindex_empty_input_is_noop() {
        let backend = Backend::new_test();
        backend.reindex_references_for_symbol_maps_batch(Vec::new());
        assert!(backend.reference_index.read().is_empty());
    }

    #[test]
    fn headless_backend_skips_reference_indexing() {
        let backend = Backend::new_headless();
        let uri = "file:///project/src/Foo.php".to_string();

        backend.reindex_references_for_symbol_maps_batch(vec![(
            uri,
            class_declaration_symbol_map("App\\Foo"),
        )]);

        assert!(backend.reference_index.read().is_empty());
    }

    #[test]
    fn batch_reindex_skips_vendor_and_stub_uris() {
        let dir = tempfile::tempdir().expect("temp dir");
        let vendor = dir.path().join("vendor");
        std::fs::create_dir_all(&vendor).expect("vendor dir");

        let backend = Backend::new_test();
        backend.add_vendor_dir(&vendor);
        backend.workspace_indexed.store(true, Ordering::Release);

        let user_uri = "file:///project/src/User.php".to_string();
        let vendor_uri = crate::util::path_to_uri(&vendor.join("Package.php"));
        backend.reindex_references_for_symbol_maps_batch(vec![
            (user_uri.clone(), class_declaration_symbol_map("User")),
            (vendor_uri.clone(), class_declaration_symbol_map("Package")),
            (
                "phpantom-stub://core.php".to_string(),
                class_declaration_symbol_map("StubClass"),
            ),
            (
                "phpantom-stub-fn://core.php".to_string(),
                class_declaration_symbol_map("StubFunctionClass"),
            ),
        ]);

        assert_candidate_contains(
            &backend,
            ReferenceIndexKey::Class("User".to_string()),
            &user_uri,
        );
        assert_candidate_not_contains(
            &backend,
            ReferenceIndexKey::Class("Package".to_string()),
            &vendor_uri,
        );
        assert_candidate_not_contains(
            &backend,
            ReferenceIndexKey::Class("StubClass".to_string()),
            "phpantom-stub://core.php",
        );
        assert_candidate_not_contains(
            &backend,
            ReferenceIndexKey::Class("StubFunctionClass".to_string()),
            "phpantom-stub-fn://core.php",
        );
    }

    #[test]
    fn self_static_parent_without_enclosing_class_is_not_indexed() {
        let backend = Backend::new_test();
        backend.workspace_indexed.store(true, Ordering::Release);
        let uri = "file:///project/src/Loose.php".to_string();

        backend.reindex_references_for_symbol_maps_batch(vec![(
            uri,
            Arc::new(SymbolMap {
                spans: vec![SymbolSpan {
                    start: 6,
                    end: 10,
                    kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::Self_),
                }],
                ..SymbolMap::default()
            }),
        )]);

        let candidates = backend
            .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Class("self".to_string())])
            .expect("workspace should be marked indexed");
        assert!(candidates.is_empty());
    }

    #[test]
    fn zero_offset_property_declaration_is_not_indexed() {
        let backend = Backend::new_test();
        backend.workspace_indexed.store(true, Ordering::Release);
        let uri = "file:///project/src/Foo.php".to_string();

        let mut class = crate::types::ClassInfo {
            name: crate::atom::atom("Foo"),
            ..crate::types::ClassInfo::default()
        };
        class.properties = crate::types::SharedVec::from_vec(vec![Arc::new(
            crate::types::PropertyInfo::virtual_property("name", None),
        )]);
        backend
            .symbols
            .uri_classes_index
            .write()
            .insert(uri.clone(), vec![Arc::new(class)]);

        backend
            .reindex_references_for_symbol_maps_batch(vec![(uri, Arc::new(SymbolMap::default()))]);

        let candidates = backend
            .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Member {
                name: "name".to_string(),
                is_static: false,
            }])
            .expect("workspace should be marked indexed");
        assert!(candidates.is_empty());
    }

    #[test]
    fn direct_entry_build_skips_non_indexable_uri() {
        let backend = Backend::new_test();
        let entries = backend.reference_entries_for_symbol_map(
            "phpantom-stub://core.php",
            &class_declaration_symbol_map("StubClass"),
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn evicting_unknown_uri_is_noop() {
        let mut index = ReferenceIndexInner::default();
        let key = ReferenceIndexKey::Class("Foo".to_string());
        let uri: Arc<str> = Arc::from("file:///project/src/Foo.php");
        index
            .by_key
            .entry(key.clone())
            .or_default()
            .insert(Arc::clone(&uri), 0);
        index.uri_keys.insert(uri, vec![key.clone()]);

        evict_reference_index_uri_locked(&mut index, "file:///project/src/Other.php");
        assert!(index.by_key.contains_key(&key));
    }

    fn assert_candidate_contains(backend: &Backend, key: ReferenceIndexKey, uri: &str) {
        let candidates = backend
            .reference_candidate_uris_for_keys(&[key])
            .expect("workspace should be marked indexed");
        assert!(
            candidates.contains(uri),
            "expected candidates to contain {uri}, got {candidates:?}"
        );
    }

    fn assert_candidate_not_contains(backend: &Backend, key: ReferenceIndexKey, uri: &str) {
        let candidates = backend
            .reference_candidate_uris_for_keys(&[key])
            .expect("workspace should be marked indexed");
        assert!(
            !candidates.contains(uri),
            "expected candidates not to contain {uri}, got {candidates:?}"
        );
    }

    fn class_declaration_symbol_map(name: &str) -> Arc<SymbolMap> {
        Arc::new(SymbolMap {
            spans: vec![SymbolSpan {
                start: 0,
                end: name.len() as u32,
                kind: SymbolKind::ClassDeclaration {
                    name: crate::atom::atom(name),
                },
            }],
            ..SymbolMap::default()
        })
    }

    fn laravel_string_map(kind: LaravelStringKind, key: &str) -> Arc<SymbolMap> {
        Arc::new(SymbolMap {
            spans: vec![SymbolSpan {
                start: 0,
                end: key.len() as u32,
                kind: SymbolKind::LaravelStringKey {
                    kind,
                    key: key.to_string(),
                    is_write: false,
                    is_optional: false,
                },
            }],
            ..SymbolMap::default()
        })
    }
}
