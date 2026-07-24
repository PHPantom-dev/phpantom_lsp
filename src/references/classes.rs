//! Class and constructor reference finders.
//!
//! [`Backend::find_class_references`] matches `ClassReference` /
//! `ClassDeclaration` / `self`/`static`/`parent` spans whose resolved
//! FQN equals the target.  [`Backend::find_constructor_references`]
//! resolves `new ClassName(...)` (and `parent::__construct()`) call
//! sites through the constructor's owning hierarchy.

use super::*;

use tower_lsp::lsp_types::{Location, Range};

use crate::references::push_unique_location;
use crate::symbol_map::{ClassRefContext, SelfStaticParentKind, SymbolKind};
use crate::text_position::offset_to_position;
use crate::types::ClassInfo;
use crate::util::build_fqn;

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
            let file_use_map = std::cell::OnceCell::new();

            // First pass: resolved-name check to avoid unnecessary content work.
            // Aliased imports (`use Foo as Bar; new Bar`) must still reach the
            // full matching loop, because the textual span name is the alias.
            let has_potential_match = symbol_map.spans.iter().any(|span| match &span.kind {
                SymbolKind::ClassReference { name, .. } => {
                    if crate::util::short_name(name) == target_short {
                        true
                    } else {
                        let resolved = if let Some(fqn) =
                            resolved_names.as_ref().and_then(|rn| rn.get(span.start))
                        {
                            fqn.to_string()
                        } else {
                            let use_map = file_use_map.get_or_init(|| {
                                self.file_imports
                                    .read()
                                    .get(file_uri)
                                    .cloned()
                                    .unwrap_or_default()
                            });
                            Self::resolve_to_fqn(name, use_map, &file_namespace)
                        };
                        class_names_match(strip_fqn_prefix(&resolved), target, target_short)
                    }
                }
                SymbolKind::ClassDeclaration { name } => {
                    include_declaration && name == target_short
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

            // Lazily load file content only if we find a true FQN match.
            let mut file_content: Option<Arc<String>> = None;

            for span in &symbol_map.spans {
                let matched = match &span.kind {
                    SymbolKind::ClassReference { name, is_fqn, .. } => {
                        let resolved = if *is_fqn {
                            name.clone()
                        } else if let Some(fqn) =
                            resolved_names.as_ref().and_then(|rn| rn.get(span.start))
                        {
                            fqn.to_string()
                        } else {
                            let use_map = file_use_map.get_or_init(|| {
                                self.file_imports
                                    .read()
                                    .get(file_uri)
                                    .cloned()
                                    .unwrap_or_default()
                            });
                            Self::resolve_to_fqn(name, use_map, &file_namespace)
                        };
                        class_names_match(strip_fqn_prefix(&resolved), target, target_short)
                    }
                    SymbolKind::ClassDeclaration { name } if include_declaration => {
                        if name != target_short {
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

                if matched {
                    if file_content.is_none() {
                        file_content = self.reference_file_content_arc(file_uri);
                    }
                    if let Some(ref content) = file_content {
                        let start = offset_to_position(content, span.start as usize);
                        let end = offset_to_position(content, span.end as usize);
                        locations.push(Location {
                            uri: parsed_uri.clone(),
                            range: Range { start, end },
                        });
                    }
                }
            }
        }

        // Sort: by URI, then by position.
        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        locations.dedup();
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
            let file_use_map = std::cell::OnceCell::new();
            let file_ctx = std::cell::OnceCell::new();

            let Some(parsed_uri) = Url::parse(file_uri).ok() else {
                continue;
            };

            let mut file_content: Option<Arc<String>> = None;

            for span in &symbol_map.spans {
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
                        let resolved = if *is_fqn {
                            name
                        } else if let Some(fqn) =
                            resolved_names.as_ref().and_then(|rn| rn.get(span.start))
                        {
                            fqn
                        } else {
                            let use_map = file_use_map.get_or_init(|| {
                                self.file_imports
                                    .read()
                                    .get(file_uri)
                                    .cloned()
                                    .unwrap_or_default()
                            });
                            &Self::resolve_to_fqn(name, use_map, &file_namespace)
                        };
                        scoped.contains(&normalize_fqn(strip_fqn_prefix(resolved)))
                    }
                    // Explicit constructor delegation written as
                    // `parent::__construct()`, `self::__construct()`, or
                    // `Foo::__construct()` lands here.  Resolve the subject
                    // class and keep the call when it falls within the
                    // constructor's owning hierarchy.
                    SymbolKind::MemberAccess {
                        subject_text,
                        member_name,
                        is_static,
                        ..
                    } if is_constructor_name(member_name) => {
                        if file_content.is_none() {
                            file_content = self.reference_file_content_arc(file_uri);
                        }
                        match &file_content {
                            Some(content) => {
                                let ctx = file_ctx.get_or_init(|| self.file_context(file_uri));
                                self.resolve_subject_to_fqns(
                                    subject_text,
                                    *is_static,
                                    ctx,
                                    span.start,
                                    content,
                                )
                                .iter()
                                .any(|fqn| scoped.contains(&normalize_fqn(strip_fqn_prefix(fqn))))
                            }
                            None => false,
                        }
                    }
                    _ => false,
                };

                if matched {
                    if file_content.is_none() {
                        file_content = self.reference_file_content_arc(file_uri);
                    }
                    if let Some(content) = &file_content {
                        let start = offset_to_position(content, span.start as usize);
                        let end = offset_to_position(content, span.end as usize);
                        push_unique_location(&mut locations, &parsed_uri, start, end);
                    }
                }
            }

            // Optionally include the constructor declaration site(s).
            if include_declaration && let Some(classes) = self.get_classes_for_uri(file_uri) {
                for class in &classes {
                    let class_fqn = normalize_fqn(&class.fqn()).to_string();
                    if !scoped.contains(&class_fqn) {
                        continue;
                    }

                    for method in class.methods.iter() {
                        if is_constructor_name(&method.name) && method.name_offset != 0 {
                            if file_content.is_none() {
                                file_content = self.reference_file_content_arc(file_uri);
                            }
                            let Some(content) = &file_content else {
                                break;
                            };
                            let offset = method.name_offset as usize;
                            let start = offset_to_position(content, offset);
                            let end = offset_to_position(content, offset + method.name.len());
                            push_unique_location(&mut locations, &parsed_uri, start, end);
                        }
                    }
                }
            }
        }

        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });
        locations.dedup();
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
    fn collect_constructor_hierarchy(&self, owner_fqns: &[String]) -> HashSet<String> {
        let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };
        let declares_ctor = |fqn: &str| -> bool {
            class_loader(fqn)
                .map(|c| c.methods.iter().any(|m| is_constructor_name(&m.name)))
                .unwrap_or(false)
        };

        let owners: Vec<String> = owner_fqns.iter().map(|f| normalize_fqn(f)).collect();
        let mut result: HashSet<String> = owners.iter().cloned().collect();

        // Walk down from each owner, including inheriting descendants and
        // pruning at overrides.
        let gti = self.gti_index.read();
        let mut queue: std::collections::VecDeque<String> = owners.iter().cloned().collect();
        let mut seen: HashSet<String> = owners.iter().cloned().collect();
        while let Some(fqn) = queue.pop_front() {
            if let Some(descendants) = gti.get(&fqn) {
                for desc in descendants {
                    let normalized = normalize_fqn(desc).to_string();
                    if !seen.insert(normalized.clone()) {
                        continue;
                    }
                    // A descendant that declares its own constructor uses a
                    // different constructor — exclude it and stop walking
                    // past it.
                    if declares_ctor(&normalized) {
                        continue;
                    }
                    result.insert(normalized.clone());
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
