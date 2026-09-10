//! Cross-file reference candidate index.
//!
//! The precise find-references logic still lives in `references`: it resolves
//! aliases, class hierarchies, `self/static/parent`, and Laravel declarations.
//! This index is intentionally a coarse candidate index keyed by symbol name so
//! those scanners can skip files that cannot contain a match once the workspace
//! has been fully parsed.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use parking_lot::RwLock;

use crate::Backend;
use crate::atom::{Atom, AtomMap, AtomSet, atom};
use crate::backend::file_access::namespace_in_spans;
use crate::class_lookup::find_class_at_offset;
use crate::symbol_map::{LaravelStringKind, SelfStaticParentKind, SymbolKind, SymbolMap};
use crate::types::NamespaceSpan;
use crate::util::{build_fqn, short_name, strip_fqn_prefix};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ReferenceIndexKey {
    /// A class/interface/trait/enum name, ASCII-lowercased.  PHP resolves
    /// class names case-insensitively, so `new WIDGET()` and `new Widget()`
    /// name the same class and have to land on the same key; build one with
    /// [`class`](ReferenceIndexKey::class) or
    /// [`class_owned`](ReferenceIndexKey::class_owned) rather than by hand.
    Class(String),
    /// A function name, ASCII-lowercased.  PHP resolves function names
    /// case-insensitively, so `HELPER()` and `helper()` are the same
    /// function and have to land on the same key; build one with
    /// [`function`](ReferenceIndexKey::function) or
    /// [`function_owned`](ReferenceIndexKey::function_owned) rather than
    /// by hand.
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

impl ReferenceIndexKey {
    /// The key a class is indexed under, case-folded to match PHP's
    /// case-insensitive class resolution.
    pub(crate) fn class(name: &str) -> Self {
        Self::Class(crate::ci_map::fold(name).into_owned())
    }

    /// [`class`](Self::class) for a name that is already owned, folding it
    /// in place rather than allocating a second copy.  Every file the
    /// workspace index publishes goes through here.
    pub(crate) fn class_owned(mut name: String) -> Self {
        name.make_ascii_lowercase();
        Self::Class(name)
    }

    /// The key a function is indexed under, case-folded to match PHP's
    /// case-insensitive function resolution.
    pub(crate) fn function(name: &str) -> Self {
        Self::Function(crate::ci_map::fold(name).into_owned())
    }

    /// [`function`](Self::function) for a name that is already owned,
    /// folding it in place rather than allocating a second copy.  Every
    /// file the workspace index publishes goes through here.
    pub(crate) fn function_owned(mut name: String) -> Self {
        name.make_ascii_lowercase();
        Self::Function(name)
    }

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
    resolved_members: HashMap<Arc<str>, Arc<ResolvedMemberFile>>,
}

#[derive(Clone, Copy)]
struct ResolvedMemberAccess {
    span_index: u32,
    target_start: u32,
    target_len: u32,
}

/// Receiver classes resolved for every member access in one immutable symbol
/// map.  Targets are packed into one allocation; the access table points into
/// it and stays sorted by symbol-span index for binary lookup.
pub(crate) struct ResolvedMemberFile {
    symbol_map: Arc<SymbolMap>,
    accesses: Vec<ResolvedMemberAccess>,
    targets: Vec<Atom>,
}

impl ResolvedMemberFile {
    pub(crate) fn new(symbol_map: Arc<SymbolMap>, mut resolved: Vec<(usize, Vec<Atom>)>) -> Self {
        resolved.sort_unstable_by_key(|(span_index, _)| *span_index);
        let target_count = resolved.iter().map(|(_, targets)| targets.len()).sum();
        let mut accesses = Vec::with_capacity(resolved.len());
        let mut packed_targets = Vec::with_capacity(target_count);
        for (span_index, mut targets) in resolved {
            if targets.is_empty() {
                continue;
            }
            targets.sort_unstable();
            targets.dedup();
            let target_start = packed_targets.len();
            packed_targets.extend(targets);
            accesses.push(ResolvedMemberAccess {
                span_index: span_index as u32,
                target_start: target_start as u32,
                target_len: (packed_targets.len() - target_start) as u32,
            });
        }
        Self {
            symbol_map,
            accesses,
            targets: packed_targets,
        }
    }

    pub(crate) fn targets_for_span(&self, span_index: usize) -> &[Atom] {
        let Ok(span_index) = u32::try_from(span_index) else {
            return &[];
        };
        let Ok(index) = self
            .accesses
            .binary_search_by_key(&span_index, |access| access.span_index)
        else {
            return &[];
        };
        let access = self.accesses[index];
        let start = access.target_start as usize;
        &self.targets[start..start + access.target_len as usize]
    }

    fn matches_symbol_map(&self, symbol_map: &Arc<SymbolMap>) -> bool {
        Arc::ptr_eq(&self.symbol_map, symbol_map)
    }

    #[cfg(feature = "mem-audit")]
    pub(crate) fn audit_heap(&self) -> (usize, usize, usize) {
        (
            self.accesses.len(),
            self.accesses.capacity() * std::mem::size_of::<ResolvedMemberAccess>()
                + self.targets.capacity() * std::mem::size_of::<Atom>(),
            usize::from(self.accesses.capacity() > 0) + usize::from(self.targets.capacity() > 0),
        )
    }
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
        &HashMap<Arc<str>, Arc<ResolvedMemberFile>>,
    ) {
        (&self.by_key, &self.uri_keys, &self.resolved_members)
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
    pub(crate) fn resolved_member_file(
        &self,
        uri: &str,
        symbol_map: &Arc<SymbolMap>,
    ) -> Option<Arc<ResolvedMemberFile>> {
        self.reference_index
            .read()
            .resolved_members
            .get(uri)
            .filter(|file| file.matches_symbol_map(symbol_map))
            .cloned()
    }

    pub(crate) fn cache_resolved_member_file(
        &self,
        uri: &str,
        symbol_map: Arc<SymbolMap>,
        resolved: Vec<(usize, Vec<Atom>)>,
    ) -> Arc<ResolvedMemberFile> {
        let built = Arc::new(ResolvedMemberFile::new(Arc::clone(&symbol_map), resolved));

        // A didChange parse may have replaced the symbol map while the
        // semantic walk was running. The result remains usable by its caller,
        // which owns the same snapshot, but must not become the current cache.
        if !self
            .symbol_maps
            .read()
            .get(uri)
            .is_some_and(|current| Arc::ptr_eq(current, &symbol_map))
        {
            return built;
        }

        let mut index = self.reference_index.write();
        if let Some(existing) = index
            .resolved_members
            .get(uri)
            .filter(|file| file.matches_symbol_map(&symbol_map))
        {
            return Arc::clone(existing);
        }
        let interned_uri = index
            .uri_keys
            .get_key_value(uri)
            .map(|(uri, _)| Arc::clone(uri))
            .unwrap_or_else(|| Arc::from(uri));
        index
            .resolved_members
            .insert(interned_uri, Arc::clone(&built));
        built
    }

    pub(crate) fn clear_resolved_member_files(&self) {
        self.reference_index.write().resolved_members.clear();
    }

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
        if track_members {
            self.member_ref_counts.invalidate_locations_all();
        }
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

    /// Number of indexed reference occurrences for `key`.
    ///
    /// `None` means the workspace index cannot answer yet.  A returned zero
    /// is conclusive: semantic filtering can remove name matches, but it
    /// cannot create a reference that the symbol map did not index.
    pub(crate) fn indexed_reference_count(&self, key: &ReferenceIndexKey) -> Option<usize> {
        if self.skip_reference_index || !self.workspace_indexed.load(Ordering::Acquire) {
            return None;
        }

        let index = self.reference_index.read();
        Some(occurrences_for_key(&index, key))
    }

    /// Number of indexed occurrences of a member name, counting the instance
    /// and static access forms together.
    ///
    /// Both forms have to be summed for a zero to be conclusive.  A static
    /// access can reach an instance declaration (`parent::handle()`), and
    /// Laravel's model forwarding turns `User::active()` into a call to an
    /// instance method on a custom builder; the exact search searches both
    /// keys for exactly that reason (see `member_candidate_keys`).
    pub(crate) fn indexed_member_reference_count(&self, member: &str) -> Option<usize> {
        if self.skip_reference_index || !self.workspace_indexed.load(Ordering::Acquire) {
            return None;
        }

        let mut key = ReferenceIndexKey::Member {
            name: member.to_string(),
            is_static: false,
        };
        let index = self.reference_index.read();
        let mut total = occurrences_for_key(&index, &key);
        if let ReferenceIndexKey::Member { is_static, .. } = &mut key {
            *is_static = true;
        }
        total += occurrences_for_key(&index, &key);
        Some(total)
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

        if track_members && !rebuilt.is_empty() {
            self.member_ref_counts.invalidate_locations_all();
        }

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
        // Resolving a namespace-qualified name back to the namespace it
        // was written in needs the file's namespace blocks; cloning them
        // once beats re-locking `file_namespaces` per span.
        let namespaces = OnceCell::new();
        for span in &symbol_map.spans {
            let is_declaration = matches!(
                &span.kind,
                SymbolKind::ClassDeclaration { .. }
                    | SymbolKind::FunctionCall {
                        is_definition: true,
                        ..
                    }
                    | SymbolKind::ConstantReference {
                        is_definition: true,
                        ..
                    }
                    | SymbolKind::MemberDeclaration { .. }
            );

            for (key, counts_as_reference) in self.reference_keys_for_span(uri, span, &namespaces) {
                entries.push((key, counts_as_reference && !is_declaration));
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

    /// The keys a span joins the index under, each paired with whether the
    /// span counts as a reference to that key (see
    /// [`reference_entries_for_symbol_map`]).
    fn reference_keys_for_span(
        &self,
        uri: &str,
        span: &crate::symbol_map::SymbolSpan,
        namespaces: &OnceCell<Vec<NamespaceSpan>>,
    ) -> Vec<(ReferenceIndexKey, bool)> {
        match &span.kind {
            SymbolKind::ClassReference { name, is_fqn, .. } => {
                // A resource document only ever records fully-qualified
                // names, so the short-name candidate key `class_keys` adds
                // for a source spelling that could differ from the resolved
                // FQN can never apply.  Skipping it keeps a large generated
                // XML from adding one index entry per distinct class it
                // names on top of the one that answers lookups.
                if *is_fqn && crate::resource_navigation::is_resource_document(uri) {
                    return vec![(
                        ReferenceIndexKey::class_owned(normalize_symbol_name(name)),
                        true,
                    )];
                }
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
                let global_fallback =
                    self.names_global_fallback(uri, span.start, name, &resolved, namespaces);
                function_keys(&resolved, name, global_fallback)
            }
            SymbolKind::ConstantReference { name, .. } => {
                let resolved = self.constant_fqn_at(uri, span.start, name);
                let global_fallback =
                    self.names_global_fallback(uri, span.start, name, &resolved, namespaces);
                constant_keys(&resolved, name, global_fallback)
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
                vec![(
                    ReferenceIndexKey::LaravelString {
                        kind: kind.clone(),
                        key: key.to_string(),
                    },
                    true,
                )]
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

    /// Whether an unqualified function or constant name at `offset` also
    /// names the *global* symbol of that name.
    ///
    /// PHP resolves an unqualified function or constant by looking in the
    /// current namespace first and falling back to the global namespace
    /// when nothing is declared there, and mago-names always reports the
    /// namespace-qualified guess.  So `helper()` written inside
    /// `namespace App;` resolves to `App\helper` but calls the global
    /// `helper()` whenever `App\helper` does not exist -- which is the
    /// usual arrangement, since a global helper file is exactly what
    /// namespaced code calls into.  Both spellings therefore have to be
    /// credited with the reference; whether the namespaced one exists is
    /// not knowable here (the file that would declare it may not be
    /// parsed yet).
    ///
    /// A name reached through an import (`use function Foo\bar;`) or an
    /// alias is *not* a fallback: it resolves to the imported symbol
    /// outright.  Those are excluded by requiring that the resolved name
    /// is the source name qualified with the namespace it was written in.
    fn names_global_fallback(
        &self,
        uri: &str,
        offset: u32,
        source_name: &str,
        resolved: &str,
        namespaces: &OnceCell<Vec<NamespaceSpan>>,
    ) -> bool {
        if source_name.contains('\\') || !resolved.contains('\\') {
            return false;
        }
        let spans = namespaces.get_or_init(|| self.namespace_spans_for_uri(uri));
        let Some(namespace) = namespace_in_spans(spans, offset) else {
            return false;
        };
        build_fqn(source_name, Some(namespace)) == resolved
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
        ReferenceIndexKey::function(&self.function_fqn_at(uri, offset, name))
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
    index.resolved_members.remove(uri);
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

fn occurrences_for_key(index: &ReferenceIndexInner, key: &ReferenceIndexKey) -> usize {
    index
        .get(key)
        .map(|entries| entries.values().map(|&count| count as usize).sum())
        .unwrap_or(0)
}

fn normalize_symbol_name(name: impl AsRef<str>) -> String {
    strip_fqn_prefix(name.as_ref()).to_string()
}

fn class_keys(resolved: &str, source_name: &str) -> Vec<(ReferenceIndexKey, bool)> {
    // A class has no global fallback: an unqualified `Widget` inside
    // `namespace App;` names `App\Widget` and nothing else.
    symbol_name_keys(resolved, source_name, false)
        .into_iter()
        .map(|(name, counts)| (ReferenceIndexKey::class_owned(name), counts))
        .collect()
}

fn function_keys(
    resolved: &str,
    source_name: &str,
    global_fallback: bool,
) -> Vec<(ReferenceIndexKey, bool)> {
    symbol_name_keys(resolved, source_name, global_fallback)
        .into_iter()
        .map(|(name, counts)| (ReferenceIndexKey::function_owned(name), counts))
        .collect()
}

fn constant_keys(
    resolved: &str,
    source_name: &str,
    global_fallback: bool,
) -> Vec<(ReferenceIndexKey, bool)> {
    symbol_name_keys(resolved, source_name, global_fallback)
        .into_iter()
        .map(|(name, counts)| (ReferenceIndexKey::Constant(name), counts))
        .collect()
}

/// Every name a symbol reference can be searched by, each flagged with
/// whether the reference counts towards that name's reference count.
///
/// The resolved name always counts.  The alias keys normally do not (a
/// reference to `App\Widget` is searchable under `Widget`, but counting it
/// there would credit a global `\Widget` with it) -- except for the short
/// name of a symbol PHP would fall back to globally, which the reference
/// genuinely names as well; see
/// [`names_global_fallback`](Backend::names_global_fallback).
fn symbol_name_keys(
    resolved: &str,
    source_name: &str,
    global_fallback: bool,
) -> Vec<(String, bool)> {
    let resolved = normalize_symbol_name(resolved);
    let mut keys = vec![
        normalize_symbol_name(source_name),
        short_name(&resolved).to_string(),
    ];
    keys.sort();
    keys.dedup();
    keys.retain(|name| *name != resolved);
    let mut keys: Vec<(String, bool)> = keys
        .into_iter()
        .map(|name| {
            let counts = global_fallback && name == short_name(&resolved);
            (name, counts)
        })
        .collect();
    keys.push((resolved, true));
    keys
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::Backend;
    use crate::symbol_map::{SymbolMap, SymbolSpan};

    #[test]
    fn candidate_lookup_is_disabled_until_workspace_is_indexed() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Foo.php";
        backend.update_ast(
            uri,
            "<?php\nnamespace App;\nclass Foo {}\n$foo = new Foo();\n",
        );

        let candidates =
            backend.reference_candidate_uris_for_keys(&[ReferenceIndexKey::class("App\\Foo")]);

        assert!(candidates.is_none());
    }

    #[test]
    fn candidate_lookup_is_disabled_when_reference_indexing_is_skipped() {
        let mut backend = Backend::new_test();
        backend.skip_reference_index = true;
        backend.workspace_indexed.store(true, Ordering::Release);

        let candidates =
            backend.reference_candidate_uris_for_keys(&[ReferenceIndexKey::class("App\\Foo")]);

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

        assert_candidate_contains(&backend, ReferenceIndexKey::class("App\\Foo"), uri);
        assert_candidate_contains(&backend, ReferenceIndexKey::function("App\\helper"), uri);
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
    fn reference_index_evicts_candidates_when_file_maps_clear() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Foo.php";
        backend.update_ast(uri, "<?php\nnamespace App;\nclass Foo {}\nnew Foo();\n");
        backend.workspace_indexed.store(true, Ordering::Release);

        assert_candidate_contains(&backend, ReferenceIndexKey::class("App\\Foo"), uri);

        backend.clear_file_maps(uri);

        let candidates =
            backend.reference_candidate_uris_for_keys(&[ReferenceIndexKey::class("App\\Foo")]);
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
        assert_candidate_contains(&backend, ReferenceIndexKey::class("Old"), &uri);

        backend.reindex_references_for_symbol_maps_batch(vec![
            (uri.clone(), class_declaration_symbol_map("First")),
            (uri.clone(), class_declaration_symbol_map("Second")),
        ]);

        assert_candidate_not_contains(&backend, ReferenceIndexKey::class("Old"), &uri);
        assert_candidate_not_contains(&backend, ReferenceIndexKey::class("First"), &uri);
        assert_candidate_contains(&backend, ReferenceIndexKey::class("Second"), &uri);
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

        assert_candidate_contains(&backend, ReferenceIndexKey::class("User"), &user_uri);
        assert_candidate_not_contains(&backend, ReferenceIndexKey::class("Package"), &vendor_uri);
        assert_candidate_not_contains(
            &backend,
            ReferenceIndexKey::class("StubClass"),
            "phpantom-stub://core.php",
        );
        assert_candidate_not_contains(
            &backend,
            ReferenceIndexKey::class("StubFunctionClass"),
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
            .reference_candidate_uris_for_keys(&[ReferenceIndexKey::class("self")])
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
        let key = ReferenceIndexKey::class("Foo");
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

    #[test]
    fn a_constants_own_declaration_is_not_one_of_its_references() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/constants.php";
        backend.update_ast(uri, "<?php\nconst FOO = 1;\necho FOO;\n");
        backend.workspace_indexed.store(true, Ordering::Release);

        assert_eq!(
            reference_count(&backend, ReferenceIndexKey::Constant("FOO".to_string())),
            1,
            "only the `echo FOO` use is a reference; the `const FOO` declaration is not"
        );
    }

    #[test]
    fn a_define_declaration_is_not_one_of_its_references() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/constants.php";
        backend.update_ast(uri, "<?php\ndefine('FOO', 1);\necho FOO;\n");
        backend.workspace_indexed.store(true, Ordering::Release);

        assert_eq!(
            reference_count(&backend, ReferenceIndexKey::Constant("FOO".to_string())),
            1
        );
    }

    #[test]
    fn an_unqualified_call_credits_the_global_function_it_can_fall_back_to() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Service.php";
        backend.update_ast(
            uri,
            "<?php\nnamespace App;\nfunction run(): void {\n    helper();\n    helper();\n}\n",
        );
        backend.workspace_indexed.store(true, Ordering::Release);

        // PHP calls `App\helper` when it exists and the global `helper`
        // otherwise, so both spellings are credited.
        assert_eq!(
            reference_count(&backend, ReferenceIndexKey::function("App\\helper")),
            2
        );
        assert_eq!(
            reference_count(&backend, ReferenceIndexKey::function("helper")),
            2
        );
    }

    #[test]
    fn an_imported_call_credits_only_the_function_it_imports() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Service.php";
        backend.update_ast(
            uri,
            "<?php\nnamespace App;\nuse function Support\\helper;\nfunction run(): void {\n    helper();\n}\n",
        );
        backend.workspace_indexed.store(true, Ordering::Release);

        // The `use function` import names it too, so both spans count.
        assert_eq!(
            reference_count(&backend, ReferenceIndexKey::function("Support\\helper")),
            2
        );
        assert_eq!(
            reference_count(&backend, ReferenceIndexKey::function("helper")),
            0,
            "an import resolves outright, so a global `helper` is not a fallback target"
        );
    }

    #[test]
    fn function_keys_ignore_the_case_the_call_is_spelled_with() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Service.php";
        backend.update_ast(uri, "<?php\nfunction helper(): void {}\nHELPER();\n");
        backend.workspace_indexed.store(true, Ordering::Release);

        assert_eq!(
            reference_count(&backend, ReferenceIndexKey::function("helper")),
            1
        );
    }

    #[test]
    fn class_keys_ignore_the_case_the_reference_is_spelled_with() {
        let backend = Backend::new_test();
        let uri = "file:///project/src/Widget.php";
        backend.update_ast(
            uri,
            "<?php\nnamespace App;\nclass Widget {}\n$a = new WIDGET();\n$b = new widget();\n",
        );
        backend.workspace_indexed.store(true, Ordering::Release);

        assert_eq!(
            reference_count(&backend, ReferenceIndexKey::class("App\\Widget")),
            2
        );
        assert_candidate_contains(&backend, ReferenceIndexKey::class("App\\WIDGET"), uri);
    }

    /// How many references the whole index credits to `key`.
    fn reference_count(backend: &Backend, key: ReferenceIndexKey) -> u32 {
        backend
            .reference_index
            .read()
            .get(&key)
            .map(|entries| entries.values().copied().sum())
            .unwrap_or(0)
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
}
