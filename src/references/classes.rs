//! Class and constructor reference finders.
//!
//! [`Backend::find_class_references`] matches `ClassReference` /
//! `ClassDeclaration` / `self`/`static`/`parent` spans whose resolved
//! FQN equals the target.  [`Backend::find_constructor_references`]
//! resolves `new ClassName(...)` (and `parent::__construct()`) call
//! sites through the constructor's owning hierarchy.

use super::*;

use std::cell::OnceCell;

use tower_lsp::lsp_types::Location;

use crate::atom::Atom;
use crate::symbol_map::{ClassRefContext, SelfStaticParentKind, SymbolKind};
use crate::types::ClassInfo;
use crate::util::build_fqn;

/// What one file needs to turn a class-reference span into a
/// fully-qualified name.
///
/// The `use` map is loaded on the first span that needs it: most spans
/// are answered by the name resolver alone, and most files carry no span
/// the search is interested in at all.
struct SpanFqnResolver<'a> {
    backend: &'a Backend,
    file_uri: &'a str,
    namespace: &'a Option<String>,
    resolved_names: Option<&'a crate::names::OwnedResolvedNames>,
    use_map: std::cell::OnceCell<std::collections::HashMap<String, String>>,
}

impl SpanFqnResolver<'_> {
    /// The fully-qualified name the span at `span_start` refers to.
    ///
    /// A name written fully qualified already is one; otherwise the name
    /// resolver's answer for that offset wins, and failing that (a
    /// docblock-sourced reference, which the resolver does not track) the
    /// file's `use` map and namespace decide.
    fn fqn(&self, name: &str, is_fqn: bool, span_start: u32) -> String {
        if is_fqn {
            return name.to_string();
        }
        if let Some(fqn) = self.resolved_names.and_then(|rn| rn.get(span_start)) {
            return fqn.to_string();
        }
        let use_map = self.use_map.get_or_init(|| {
            self.backend
                .file_imports
                .read()
                .get(self.file_uri)
                .cloned()
                .unwrap_or_default()
        });
        Backend::resolve_to_fqn(name, use_map, self.namespace)
    }
}

impl Backend {
    /// Find all references to a class/interface/trait/enum across all files.
    ///
    /// Matches `ClassReference` spans whose resolved FQN equals `target_fqn`,
    /// and optionally `ClassDeclaration` spans at the declaration site.
    pub(super) fn find_class_references(
        &self,
        target_fqn: &str,
        include_declaration: bool,
    ) -> Vec<Location> {
        let mut locations = Vec::new();

        // Normalise: strip leading backslash if present.
        let target = strip_fqn_prefix(target_fqn);
        let target_short = crate::util::short_name(target);

        let candidate_keys = class_candidate_keys(target, target_short);
        let snapshot = self.user_file_symbol_maps_for_reference_keys(&candidate_keys);
        self.begin_request_scan_window(snapshot.len(), "Scanning for class references");

        for (file_uri, symbol_map) in &snapshot {
            self.request_scan_file_done();
            // Prefer mago-names resolved_names for FQN resolution (byte-offset
            // based, applies PHP's full name resolution rules).  Falls back to
            // the legacy use_map lazily for identifiers not tracked by
            // mago-names (e.g. docblock-sourced references).
            let resolved_names = self.resolved_names.read().get(file_uri).cloned();
            let file_namespace = self.first_file_namespace(file_uri);
            let fqn_resolver = SpanFqnResolver {
                backend: self,
                file_uri,
                namespace: &file_namespace,
                resolved_names: resolved_names.as_deref(),
                use_map: std::cell::OnceCell::new(),
            };

            // First pass: resolved-name check to avoid unnecessary content work.
            // Aliased imports (`use Foo as Bar; new Bar`) must still reach the
            // full matching loop, because the textual span name is the alias.
            let has_potential_match = symbol_map.spans.iter().any(|span| match &span.kind {
                SymbolKind::ClassReference { name, .. } => {
                    if crate::util::short_name(name).eq_ignore_ascii_case(target_short) {
                        true
                    } else {
                        let resolved = fqn_resolver.fqn(name, false, span.start);
                        class_names_match(strip_fqn_prefix(&resolved), target, target_short)
                    }
                }
                SymbolKind::ClassDeclaration { name } => {
                    include_declaration && name.eq_ignore_ascii_case(target_short)
                }
                SymbolKind::SelfStaticParent(ssp_kind) => *ssp_kind != SelfStaticParentKind::This,
                _ => false,
            });

            if !has_potential_match {
                continue;
            }

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };

            // Content is loaded only once a true FQN match needs a position.
            let file = CandidateFile::new(self, file_uri);

            for span in &symbol_map.spans {
                let matched = match &span.kind {
                    SymbolKind::ClassReference { name, is_fqn, .. } => {
                        let resolved = fqn_resolver.fqn(name, *is_fqn, span.start);
                        class_names_match(strip_fqn_prefix(&resolved), target, target_short)
                    }
                    SymbolKind::ClassDeclaration { name } if include_declaration => {
                        if !name.eq_ignore_ascii_case(target_short) {
                            false
                        } else {
                            let fqn = build_fqn(name, file_namespace.as_deref());
                            class_names_match(&fqn, target, target_short)
                        }
                    }
                    SymbolKind::SelfStaticParent(ssp_kind)
                        if *ssp_kind != SelfStaticParentKind::This =>
                    {
                        if let Some(fqn) = self.resolve_keyword_to_fqn(
                            ssp_kind,
                            file_uri,
                            &file_namespace,
                            span.start,
                        ) {
                            class_names_match(&fqn, target, target_short)
                        } else {
                            false
                        }
                    }
                    _ => false,
                };

                if matched && let Some(range) = file.range(span.start, span.end) {
                    locations.push(Location {
                        uri: parsed_uri.clone(),
                        range,
                    });
                }
            }
        }

        super::sort_locations_for_references(&mut locations);
        locations
    }

    /// Find all references to a constructor (`__construct`).
    ///
    /// Unlike ordinary methods, constructors are not invoked through
    /// member-access syntax (`$obj->__construct()`); the call sites are
    /// `new ClassName(...)` instantiation expressions plus explicit
    /// `parent::__construct()` / `self::__construct()` style calls.
    ///
    /// `owner_fqns` are the class(es) that declare the constructor under
    /// the cursor.  A `new SubClass()` expression only invokes this
    /// constructor when `SubClass` inherits it (i.e. does not declare its
    /// own), so the search scope is expanded to inheriting descendants and
    /// pruned at overriding ones (see
    /// [`Self::collect_constructor_hierarchy`]).
    pub(super) fn find_constructor_references(
        &self,
        owner_fqns: &[String],
        include_declaration: bool,
    ) -> Vec<Location> {
        if owner_fqns.is_empty() {
            return Vec::new();
        }

        // Expand the owners to the set of classes whose instantiation
        // invokes this same constructor (inheriting descendants), pruning
        // at descendants that override it.
        let scoped = self.collect_constructor_hierarchy(owner_fqns);
        if scoped.is_empty() {
            return Vec::new();
        }

        let mut locations = Vec::new();
        let mut candidate_keys = Vec::new();
        for fqn in &scoped {
            candidate_keys.extend(class_candidate_keys(fqn, crate::util::short_name(fqn)));
        }
        candidate_keys.extend([
            ReferenceIndexKey::Member {
                name: "__construct".to_string(),
                is_static: true,
            },
            ReferenceIndexKey::Member {
                name: "__construct".to_string(),
                is_static: false,
            },
        ]);
        let snapshot = self.user_file_symbol_maps_for_reference_keys(&candidate_keys);
        self.begin_request_scan_window(snapshot.len(), "Scanning for constructor references");

        for (file_uri, symbol_map) in &snapshot {
            self.request_scan_file_done();
            let resolved_names = self.resolved_names.read().get(file_uri).cloned();
            let file_namespace = self.first_file_namespace(file_uri);
            let fqn_resolver = SpanFqnResolver {
                backend: self,
                file_uri,
                namespace: &file_namespace,
                resolved_names: resolved_names.as_deref(),
                use_map: std::cell::OnceCell::new(),
            };
            let Some(parsed_uri) = Url::parse(file_uri).ok() else {
                continue;
            };

            let file = CandidateFile::new(self, file_uri);
            // `parent::__construct()` and its siblings are member accesses,
            // so their receivers go through the same pass (and the same
            // recorded answers) as every other member search.
            let receivers = OnceCell::new();
            let constructor_receivers = || {
                receivers
                    .get_or_init(|| {
                        let names: Vec<Atom> = symbol_map
                            .member_access_indices
                            .keys()
                            .filter(|name| is_constructor_name(name))
                            .copied()
                            .collect();
                        self.member_receivers_for(&file, symbol_map, &names)
                    })
                    .clone()
            };

            for (span_index, span) in symbol_map.spans.iter().enumerate() {
                let matched = match &span.kind {
                    // `new ClassName(...)` carries `ClassRefContext::New`;
                    // `#[ClassName(...)]` attribute usages carry
                    // `ClassRefContext::Attribute`.  Both invoke the
                    // constructor.
                    SymbolKind::ClassReference {
                        name,
                        is_fqn,
                        context: ClassRefContext::New | ClassRefContext::Attribute,
                    } => {
                        let resolved = fqn_resolver.fqn(name, *is_fqn, span.start);
                        scoped.contains(&fold_class_fqn(&resolved))
                    }
                    // `new self()` / `new static()` / `new parent()` carry
                    // `SelfStaticParent` spans rather than `ClassReference`,
                    // so they need the same enclosing-class resolution as
                    // `self::__construct()` below.  The same span kind is
                    // also emitted for `parent::__construct()`'s subject
                    // (handled by the `MemberAccess` arm below), so this
                    // only fires when the keyword is actually the operand
                    // of `new`.
                    SymbolKind::SelfStaticParent(ssp_kind)
                        if *ssp_kind != SelfStaticParentKind::This =>
                    {
                        match file.content() {
                            Some(content) if is_new_operand(content, span.start) => {
                                match self.resolve_keyword_to_fqn(
                                    ssp_kind,
                                    file_uri,
                                    &file_namespace,
                                    span.start,
                                ) {
                                    Some(fqn) => scoped.contains(&fold_class_fqn(&fqn)),
                                    None => false,
                                }
                            }
                            _ => false,
                        }
                    }
                    // Explicit constructor delegation written as
                    // `parent::__construct()`, `self::__construct()`, or
                    // `Foo::__construct()` lands here.  Resolve the subject
                    // class and keep the call when it falls within the
                    // constructor's owning hierarchy.
                    SymbolKind::MemberAccess { member_name, .. }
                        if is_constructor_name(member_name) =>
                    {
                        constructor_receivers()
                            .and_then(|file| {
                                file.resolved_access(span_index).map(|(_, targets)| {
                                    targets
                                        .iter()
                                        .any(|fqn| scoped.contains(&fold_class_fqn(fqn)))
                                })
                            })
                            .unwrap_or(false)
                    }
                    _ => false,
                };

                if matched && let Some(range) = file.range(span.start, span.end) {
                    locations.push(Location {
                        uri: parsed_uri.clone(),
                        range,
                    });
                }
            }

            // Optionally include the constructor declaration site(s).
            if include_declaration && let Some(classes) = self.get_classes_for_uri(file_uri) {
                for class in &classes {
                    if !scoped.contains(&fold_class_fqn(&class.fqn())) {
                        continue;
                    }

                    for method in class.methods.iter() {
                        if is_constructor_name(&method.name) && method.name_offset != 0 {
                            let offset = method.name_offset;
                            let Some(range) = file.range(offset, offset + method.name.len() as u32)
                            else {
                                break;
                            };
                            locations.push(Location {
                                uri: parsed_uri.clone(),
                                range,
                            });
                        }
                    }
                }
            }
        }

        super::sort_locations_for_references(&mut locations);
        locations
    }

    /// Expand the constructor owner class(es) into the full set of classes
    /// whose instantiation (`new X(...)`) invokes the same constructor.
    ///
    /// Starting from `owner_fqns` (the class(es) that declare the
    /// constructor under the cursor), walk down the inheritance tree and
    /// include every descendant that does *not* declare its own
    /// constructor (those inherit the owner's), pruning the walk at any
    /// descendant that overrides it.
    ///
    /// The returned FQNs are case-folded: they are only ever membership-
    /// tested against a call site's resolved class, and PHP lets that site
    /// spell the name in any casing.  The walk itself stays on the declared
    /// spelling, because the GTI index is keyed by the name each child
    /// writes in its `extends`/`implements` clause.
    fn collect_constructor_hierarchy(&self, owner_fqns: &[String]) -> HashSet<String> {
        let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };
        let declares_ctor = |fqn: &str| -> bool {
            class_loader(fqn)
                .map(|c| c.methods.iter().any(|m| is_constructor_name(&m.name)))
                .unwrap_or(false)
        };

        let owners: Vec<String> = owner_fqns.iter().map(|f| normalize_fqn(f)).collect();
        let mut result: HashSet<String> = owners.iter().map(|f| fold_class_fqn(f)).collect();

        // Walk down from each owner, including inheriting descendants and
        // pruning at overrides.
        let gti = self.symbols.gti_index.read();
        let mut queue: std::collections::VecDeque<String> = owners.iter().cloned().collect();
        let mut seen: HashSet<String> = owners.iter().map(|f| fold_class_fqn(f)).collect();
        while let Some(fqn) = queue.pop_front() {
            if let Some(descendants) = gti.get(&fqn) {
                for desc in descendants {
                    let normalized = normalize_fqn(desc).to_string();
                    if !seen.insert(fold_class_fqn(&normalized)) {
                        continue;
                    }
                    // A descendant that declares its own constructor uses a
                    // different constructor — exclude it and stop walking
                    // past it.
                    if declares_ctor(&normalized) {
                        continue;
                    }
                    result.insert(fold_class_fqn(&normalized));
                    queue.push_back(normalized);
                }
            }
        }

        result
    }

    fn resolve_keyword_to_fqn(
        &self,
        ssp_kind: &SelfStaticParentKind,
        uri: &str,
        namespace: &Option<String>,
        offset: u32,
    ) -> Option<String> {
        let classes: Vec<Arc<ClassInfo>> = self
            .symbols
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        let current_class = crate::class_lookup::find_class_at_offset(&classes, offset)?;

        match ssp_kind {
            SelfStaticParentKind::Parent => current_class.parent_class.map(|a| a.to_string()),
            _ => {
                // self / static → current class FQN
                Some(build_fqn(&current_class.name, namespace.as_deref()))
            }
        }
    }
}

/// Whether the `self`/`static`/`parent` keyword at `start` is the operand of
/// `new` (`new self()`) rather than the subject of a static access
/// (`self::__construct()`), which the same `SelfStaticParent` span kind is
/// also used for.
fn is_new_operand(content: &str, start: u32) -> bool {
    let bytes = content.as_bytes();
    let mut i = start as usize;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i < 3 {
        return false;
    }
    let word_start = i - 3;
    if word_start > 0 {
        let prev = bytes[word_start - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' {
            return false;
        }
    }
    bytes[word_start..i].eq_ignore_ascii_case(b"new")
}
