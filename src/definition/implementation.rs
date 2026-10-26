//! Go-to-implementation support (`textDocument/implementation`).
//!
//! When the cursor is on an interface name, abstract class name, or any
//! non-final class name, this module finds all concrete implementations
//! (and, for concrete class targets, all subclasses including abstract
//! ones) and returns their locations.  The same applies to method calls
//! where the owning type is an interface or abstract class.
//!
//! When the cursor is on a method *definition* inside a concrete class, the
//! reverse direction is also supported: the handler finds the interface or
//! abstract class that declares the method and jumps to it.
//!
//! Final classes cannot be extended, so go-to-implementation is a no-op
//! for them.
//!
//! # Resolution strategy
//!
//! 1. **Determine the target symbol** — consult the precomputed `SymbolMap`
//!    for the word under the cursor.
//! 2. **Identify the target type** — resolve the symbol to a `ClassInfo` and
//!    check whether it is a non-final class or interface.
//! 3. **Scan for implementors** — walk all classes known to the server
//!    (`uri_classes_index`, `fqn_uri_index`, PSR-4 directories) and collect
//!    those whose `interfaces` list or `parent_class` matches the target type.
//! 4. **Return locations** — for class-level requests, return the class
//!    declaration position; for method-level requests, return the method
//!    position in each implementing class.
//! 5. **Reverse jump** — for `MemberDeclaration` symbols on a concrete class,
//!    walk the class's interfaces and parent abstract classes to find the
//!    prototype method declaration and return its location.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp_server::gen_lsp_types::*;

use super::member::MemberKind;
use super::{class_keyword_location, point_location};
use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader, find_class_at_offset};
use crate::config::IndexingStrategy;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind};
use crate::text_position::position_to_offset;
use crate::types::{ClassInfo, ClassLikeKind, FileContext, MAX_INHERITANCE_DEPTH, ResolvedType};
use crate::util::short_name;

/// The target and listing rules of one [`Backend::find_implementors`] call.
struct ImplementorQuery<'a> {
    target_short: &'a str,
    target_fqn: &'a str,
    class_loader: &'a ResolvedClassLoader<'a>,
    include_abstract: bool,
    direct_only: bool,
}

/// The implementors found so far, tracked by FQN to avoid short-name
/// collisions across namespaces.
#[derive(Default)]
struct FoundImplementors {
    result: Vec<Arc<ClassInfo>>,
    seen_fqns: HashSet<String>,
}

/// Sources that no parse has reached yet: files on disk, and embedded
/// stub files with the class names each declares.
struct PendingSources {
    files: Vec<PathBuf>,
    stubs: Vec<(&'static str, Vec<String>)>,
}

impl Backend {
    /// Entry point for `textDocument/implementation`.
    ///
    /// Returns a list of locations where the symbol under the cursor is
    /// concretely implemented.  Returns `None` if the cursor is not on a
    /// resolvable interface/abstract symbol.
    pub(crate) fn resolve_implementation(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Vec<Location>> {
        // ── 1. Extract the word under the cursor ────────────────────────
        // Primary path: consult the precomputed symbol map.
        let symbol = self.lookup_symbol_at_position(uri, content, position);

        if let Some(ref sym) = symbol {
            match &sym.kind {
                // Member access — delegate directly to member implementation
                // resolution using the structured symbol information.
                SymbolKind::MemberAccess { member_name, .. } => {
                    let ctx = self.file_context_at(uri, sym.start);
                    return self.resolve_member_implementations(
                        uri,
                        content,
                        position,
                        member_name.as_str(),
                        &ctx,
                    );
                }
                // Class reference or declaration — resolve as a class/interface name.
                SymbolKind::ClassReference { name, .. } | SymbolKind::ClassDeclaration { name } => {
                    let ctx = self.file_context_at(uri, sym.start);
                    return self.resolve_class_implementation(uri, content, name, &ctx, sym.start);
                }
                // self/static/parent — resolve the keyword to the current
                // class and check whether it is an interface/abstract.
                SymbolKind::SelfStaticParent(ssp_kind) => {
                    let ctx = self.file_context_at(uri, sym.start);
                    let class_loader = self.resolved_class_loader(&ctx);
                    let current_class = find_class_at_offset(&ctx.classes, sym.start);
                    let target = match ssp_kind {
                        SelfStaticParentKind::Parent => current_class
                            .and_then(|cc| cc.parent_class.as_deref())
                            .and_then(|p| class_loader(Fqn::new(p)).map(Arc::unwrap_or_clone)),
                        _ => current_class.cloned(),
                    };
                    return target
                        .and_then(|t| self.class_implementation(uri, content, t, &class_loader));
                }
                // Member declaration — reverse jump: from a concrete method
                // definition to the interface/abstract method it implements.
                SymbolKind::MemberDeclaration { name, .. } => {
                    let ctx = self.file_context_at(uri, sym.start);
                    let class_loader = self.resolved_class_loader(&ctx);
                    let current_class = find_class_at_offset(&ctx.classes, sym.start);
                    if let Some(cls) = current_class {
                        return self.resolve_reverse_implementation(
                            uri,
                            content,
                            cls,
                            name,
                            &class_loader,
                        );
                    }
                    return None;
                }
                // Other symbol kinds (variables, function calls, etc.)
                // are not meaningful for go-to-implementation.
                _ => return None,
            }
        }

        // No symbol map span covers the cursor — nothing to resolve.
        None
    }

    /// Resolve go-to-implementation for a class/interface name.
    ///
    /// Resolves `name` to a fully-qualified class, checks that it is an
    /// interface or abstract class (or a non-final concrete class that
    /// may have subclasses), finds all implementors/subclasses, and
    /// returns their declaration locations.
    fn resolve_class_implementation(
        &self,
        uri: &str,
        content: &str,
        name: &str,
        ctx: &FileContext,
        name_offset: u32,
    ) -> Option<Vec<Location>> {
        let class_loader = self.class_loader(ctx);
        let resolved_class_loader = self.resolved_class_loader(ctx);

        let fqn = ctx.resolve_name_at(name, name_offset);
        let target = resolved_class_loader(Fqn::new(&fqn))
            .or_else(|| class_loader(name))
            .map(Arc::unwrap_or_clone)?;
        self.class_implementation(uri, content, target, &resolved_class_loader)
    }

    /// Go-to-implementation for the class `target`.
    ///
    /// Checks that `target` can have implementations, finds every
    /// implementor or subclass, and returns their declaration locations.
    fn class_implementation(
        &self,
        uri: &str,
        content: &str,
        target: ClassInfo,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<Vec<Location>> {
        // Final classes cannot be extended, so there are no implementations.
        if target.is_final {
            return None;
        }

        let target_fqn = target.fqn().to_string();

        let descendants =
            self.implementation_descendants(&target, &target_fqn, class_loader, false);
        let locations = self.class_implementation_locations(uri, content, &target, &descendants);
        (!locations.is_empty()).then_some(locations)
    }

    /// Every class that extends, implements, or uses `target`, directly or
    /// transitively, abstract ones included.  Empty for a final target.
    ///
    /// Shared by go-to-implementation and the implementation lens so the
    /// two agree on what counts as an implementation.
    pub(crate) fn implementation_descendants(
        &self,
        target: &ClassInfo,
        target_fqn: &str,
        class_loader: &ResolvedClassLoader<'_>,
        project_only: bool,
    ) -> Vec<Arc<ClassInfo>> {
        if target.is_final {
            return Vec::new();
        }
        self.find_implementors(
            &target.name,
            target_fqn,
            class_loader,
            true,
            false,
            project_only,
        )
    }

    /// The declaration locations of the `descendants` of `target`.
    ///
    /// Abstract descendants are only listed for a concrete target: there
    /// the user is exploring the class hierarchy, while for an interface or
    /// an abstract class they want the instantiable implementations.
    pub(crate) fn class_implementation_locations(
        &self,
        uri: &str,
        content: &str,
        target: &ClassInfo,
        descendants: &[Arc<ClassInfo>],
    ) -> Vec<Location> {
        let include_abstract = target.kind != ClassLikeKind::Interface && !target.is_abstract;
        let mut locations: Vec<Location> = descendants
            .iter()
            .filter(|descendant| include_abstract || !descendant.is_abstract)
            .filter_map(|descendant| self.locate_class_declaration(descendant, uri, content))
            .collect();
        sort_and_dedup_locations(&mut locations);
        locations
    }

    /// Reverse jump: from a method definition in a concrete class to the
    /// interface or abstract class that declares the prototype.
    ///
    /// When the cursor is on a method name at its definition site (e.g.
    /// `public function handle()` in a class that implements `Handler`),
    /// this finds the interface/abstract method declaration and returns
    /// its location.
    pub(super) fn resolve_reverse_implementation(
        &self,
        uri: &str,
        content: &str,
        current_class: &ClassInfo,
        member_name: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<Vec<Location>> {
        // For interfaces, abstract classes, and traits, the forward
        // direction applies: find concrete implementors that define the
        // method.
        if matches!(
            current_class.kind,
            ClassLikeKind::Interface | ClassLikeKind::Trait
        ) || current_class.is_abstract
        {
            return self.resolve_interface_member_implementations(
                uri,
                content,
                current_class,
                member_name,
                class_loader,
            );
        }

        let mut locations = Vec::new();

        // Check implemented interfaces for a method with the same name.
        let all_ifaces = crate::inheritance::ancestry::supertypes(current_class, class_loader)
            .into_iter()
            .filter(|(_, supertype)| supertype.kind == ClassLikeKind::Interface);
        for (iface_name, iface) in all_ifaces {
            let has_member = iface.has_method(member_name)
                || iface.properties.iter().any(|p| p.name == member_name);
            if has_member {
                let member_kind = if iface.has_method(member_name) {
                    MemberKind::Method
                } else {
                    MemberKind::Property
                };
                if let Some((class_uri, class_content)) =
                    self.find_class_file_content(&iface_name, uri, content)
                    && let Some(member_pos) =
                        Self::find_member_position(&class_content, member_name, member_kind, &iface)
                    && let Some(parsed_uri) = crate::util::parse_url(&class_uri)
                {
                    let loc = point_location(parsed_uri, member_pos);
                    if !locations.contains(&loc) {
                        locations.push(loc);
                    }
                }
            }
        }

        // Check parent abstract classes for an abstract method with the
        // same name.
        for (parent_name, parent_cls) in crate::inheritance::ancestors(current_class, class_loader)
        {
            // Only consider abstract methods on abstract parents.
            if !(parent_cls.is_abstract || parent_cls.kind == ClassLikeKind::Interface) {
                continue;
            }
            if parent_cls.has_method(member_name)
                && let Some((class_uri, class_content)) =
                    self.find_class_file_content(&parent_name, uri, content)
                && let Some(member_pos) = Self::find_member_position(
                    &class_content,
                    member_name,
                    MemberKind::Method,
                    &parent_cls,
                )
                && let Some(parsed_uri) = crate::util::parse_url(&class_uri)
            {
                let loc = point_location(parsed_uri, member_pos);
                if !locations.contains(&loc) {
                    locations.push(loc);
                }
            }
        }

        if locations.is_empty() {
            None
        } else {
            Some(locations)
        }
    }

    /// Resolve implementations of a method on an interface/abstract class
    /// when invoked from the interface declaration itself (reverse jump
    /// from an interface method to concrete implementations).
    fn resolve_interface_member_implementations(
        &self,
        uri: &str,
        content: &str,
        interface_class: &ClassInfo,
        member_name: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<Vec<Location>> {
        let target_fqn = interface_class.fqn().to_string();
        let descendants =
            self.implementation_descendants(interface_class, &target_fqn, class_loader, false);

        let member_kind = if interface_class
            .methods
            .iter()
            .any(|m| m.name == member_name)
        {
            MemberKind::Method
        } else if interface_class
            .properties
            .iter()
            .any(|p| p.name == member_name)
        {
            MemberKind::Property
        } else {
            MemberKind::Constant
        };

        let providers = member_implementation_providers(
            &target_fqn,
            member_name,
            member_kind,
            &descendants,
            class_loader,
        );
        let locations = self.member_implementation_locations(
            uri,
            content,
            member_name,
            member_kind,
            &providers,
        );
        (!locations.is_empty()).then_some(locations)
    }

    /// The locations of `member_name` in each of the `providers` that
    /// [`member_implementation_providers`] found.
    pub(crate) fn member_implementation_locations(
        &self,
        uri: &str,
        content: &str,
        member_name: &str,
        member_kind: MemberKind,
        providers: &[Arc<ClassInfo>],
    ) -> Vec<Location> {
        let mut locations: Vec<Location> = providers
            .iter()
            .filter_map(|provider| {
                let (class_uri, class_content) =
                    self.find_class_file_content(&provider.fqn(), uri, content)?;
                let member_pos =
                    Self::find_member_position(&class_content, member_name, member_kind, provider)?;
                Some(point_location(
                    crate::util::parse_url(&class_uri)?,
                    member_pos,
                ))
            })
            .collect();
        sort_and_dedup_locations(&mut locations);
        locations
    }

    /// Resolve implementations of a method call on an interface/abstract class.
    fn resolve_member_implementations(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        member_name: &str,
        ctx: &FileContext,
    ) -> Option<Vec<Location>> {
        // Extract the subject (left side of -> or ::).
        let (subject, access_kind) = self.lookup_member_access_context(uri, content, position)?;

        let cursor_offset = position_to_offset(content, position);
        let loaders = self.file_loaders(ctx);
        let resolved_class_loader = loaders.resolved_class_loader();

        // Resolve the subject to candidate classes.
        let candidates =
            ResolvedType::into_arced_classes(crate::type_engine::resolver::resolve_target_classes(
                &subject,
                access_kind,
                &loaders.resolution_ctx_at(content, cursor_offset),
            ));

        if candidates.is_empty() {
            return None;
        }

        // Check if ANY candidate is an interface or abstract class with this
        // method.  If so, find all implementors that have the method.
        let mut all_locations = Vec::new();

        for candidate in &candidates {
            if candidate.kind != ClassLikeKind::Interface && !candidate.is_abstract {
                continue;
            }

            // Verify the method exists on this interface/abstract class
            // (directly or inherited).
            let merged = crate::virtual_members::resolve_class_fully_cached(
                candidate,
                &resolved_class_loader,
                &self.resolved_class_cache,
            );
            let has_method = merged.has_method(member_name);
            let has_property = merged.properties.iter().any(|p| p.name == member_name);

            if !has_method && !has_property {
                continue;
            }

            let member_kind = if has_method {
                MemberKind::Method
            } else {
                MemberKind::Property
            };

            let target_fqn = candidate.fqn().to_string();
            let descendants = self.implementation_descendants(
                candidate,
                &target_fqn,
                &resolved_class_loader,
                false,
            );
            let providers = member_implementation_providers(
                &target_fqn,
                member_name,
                member_kind,
                &descendants,
                &resolved_class_loader,
            );
            all_locations.extend(self.member_implementation_locations(
                uri,
                content,
                member_name,
                member_kind,
                &providers,
            ));
        }

        sort_and_dedup_locations(&mut all_locations);
        if all_locations.is_empty() {
            return None;
        }

        Some(all_locations)
    }

    /// Find all classes that implement or extend the target.
    ///
    /// Scans:
    /// 1. Already-parsed classes, through the reverse inheritance index
    /// 2. Collects the unparsed sources: class index files, user PSR-4
    ///    files the class index does not cover yet (vendor PSR-4 roots are
    ///    assumed complete in the class index), and embedded PHP stubs
    /// 3. Parses only the sources that name the target or a subtype found
    ///    so far, repeating until a round finds no new subtype
    ///
    /// When `include_abstract` is `false` (the default for interface and
    /// abstract-class targets), abstract subclasses are excluded from the
    /// results so that only concrete implementations are returned.  When
    /// `true` (used for concrete-class targets), abstract subclasses are
    /// included because the user is exploring the full class hierarchy.
    ///
    /// When `direct_only` is `true`, only classes/interfaces/enums whose
    /// `extends`, `implements`, or `use` clause **directly** names the
    /// target are returned.  Transitive relationships (e.g. a class that
    /// extends another class that implements the target interface) are
    /// excluded.  This mode is used by the type hierarchy protocol where
    /// the client walks the tree one level at a time.
    ///
    /// When `project_only` is `true`, vendor classes are skipped before
    /// they are loaded: every phase drops any candidate whose source lives
    /// under a `/vendor/` directory, and the embedded stubs are not
    /// searched at all.  This keeps callers that need only project implementors
    /// (e.g. the Laravel auth-model floor) from paying the cost of loading
    /// and parsing every class in a large vendor tree.
    ///
    /// Independently of `project_only`, once the workspace index is ready
    /// the function returns only the Phase 1 reverse-index results, and
    /// those are restricted to project classes (excluding `/vendor/` and
    /// embedded stubs).  The reverse index is populated by every parse, so
    /// a vendor or stub class that happened to be loaded earlier in the
    /// session would otherwise appear non-deterministically.  The full
    /// workspace index only parses project files, so user-only results are
    /// both deterministic and consistent with the indexed set.
    ///
    /// That restriction is lifted when the *target itself* lives under
    /// `/vendor/`: an interface shipped by a Composer package is normally
    /// implemented inside that same package, so keeping only project
    /// classes would answer a request about `HttpKernelInterface` with the
    /// implementations Symfony ships filtered out — usually an empty
    /// result.  Such a target therefore keeps the class-index scans below,
    /// which is the only way to reach classes the workspace index never
    /// parses.  Embedded stubs stay on the fast path: PHP's own interfaces
    /// (`Countable`, `Iterator`, …) are implemented across the whole
    /// dependency tree, so scanning it for them would cost far more than
    /// the answer is worth.
    pub(crate) fn find_implementors(
        &self,
        target_short: &str,
        target_fqn: &str,
        class_loader: &ResolvedClassLoader<'_>,
        include_abstract: bool,
        direct_only: bool,
        project_only: bool,
    ) -> Vec<Arc<ClassInfo>> {
        let query = ImplementorQuery {
            target_short,
            target_fqn,
            class_loader,
            include_abstract,
            direct_only,
        };
        if self.config().indexing.strategy() == IndexingStrategy::Full
            && !self.workspace_index.indexed.load(Ordering::Acquire)
        {
            self.ensure_workspace_indexed_for_request();
        }
        let workspace_index_ready = self.workspace_index.indexed.load(Ordering::Acquire);

        // A target that ships in a Composer package is answered from the
        // class index rather than from the project-only workspace index,
        // so that the package's own implementations are reachable.
        let scan_vendor_target = !project_only
            && self
                .symbols
                .fqn_uri_index
                .read()
                .get(target_fqn)
                .is_some_and(|uri| uri.contains("/vendor/"));

        // The project filter applies to the Phase 1 reverse-index
        // candidates whenever those candidates are returned without the
        // follow-up vendor/classmap/stub scans: a `project_only` caller,
        // or the workspace-index-ready fast path below.  The reverse index
        // is populated by *every* parse, so a vendor or stub class loaded
        // earlier in the session (via hover, completion, or resolution)
        // would otherwise leak into results while an identical class that
        // was never loaded would not, making results depend on session
        // history.  The full workspace index parses only project files, so
        // user-only is the deterministic set the fast path should return.
        let exclude_non_project = project_only || (workspace_index_ready && !scan_vendor_target);
        let mut found = FoundImplementors::default();

        // ── Phase 1: GTI index lookup ───────────────────────────────────
        let gti_candidates = self.gti_descendants(target_fqn, direct_only);
        for child_fqn in &gti_candidates {
            if found.seen_fqns.contains(child_fqn)
                || !self.is_project_fqn(child_fqn, exclude_non_project)
            {
                continue;
            }
            if let Some(cls) = class_loader(Fqn::new(child_fqn)) {
                if !direct_only {
                    if cls.kind == ClassLikeKind::Interface {
                        continue;
                    }
                    if cls.is_abstract && !include_abstract {
                        continue;
                    }
                }
                found.seen_fqns.insert(child_fqn.clone());
                found.result.push(cls);
            }
        }

        if workspace_index_ready && !scan_vendor_target {
            return found.result;
        }

        // The remaining phases scan known-size candidate sets; report
        // per-file progress into the request's scan window (the totals
        // accumulate as each phase adds its candidate list).
        if let Some(p) = self.request_progress.as_deref() {
            p.set_scope(80, 100, "Scanning for implementations");
        }

        // ── Phase 2: collect the sources nothing has parsed yet ─────────
        let pending = self.unparsed_implementor_sources(project_only);

        // ── Phase 3: parse the sources that name a known subtype ───────
        self.scan_sources_for_subtypes(
            &query,
            pending,
            &gti_candidates,
            exclude_non_project,
            &mut found,
        );

        found.result
    }

    /// The FQNs the reverse inheritance index lists below `target_fqn`:
    /// only the direct children in `direct_only` mode, otherwise every
    /// transitive descendant.
    fn gti_descendants(&self, target_fqn: &str, direct_only: bool) -> Vec<String> {
        let gti = self.symbols.gti_index.read();
        if direct_only {
            return gti.get(target_fqn).cloned().unwrap_or_default();
        }
        let mut all_children: Vec<String> = Vec::new();
        let mut queue: Vec<String> = vec![target_fqn.to_string()];
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(target_fqn.to_string());
        while let Some(parent) = queue.pop() {
            if let Some(children) = gti.get(&parent) {
                for child in children {
                    if visited.insert(child.clone()) {
                        all_children.push(child.clone());
                        queue.push(child.clone());
                    }
                }
            }
        }
        all_children
    }

    /// Whether an FQN's indexed source is project code (outside
    /// `/vendor/` and not an embedded stub), or `true` for everything when
    /// `exclude_non_project` is off.  A class with no indexed URI is
    /// treated as project-local (mirrors the downstream filter in
    /// `project_auth_implementors`).
    fn is_project_fqn(&self, fqn: &str, exclude_non_project: bool) -> bool {
        if !exclude_non_project {
            return true;
        }
        match self.symbols.fqn_uri_index.read().get(fqn) {
            Some(uri) => !uri.contains("/vendor/") && !crate::stubs::is_stub_uri(uri),
            None => true,
        }
    }

    /// Collect the sources a subtype could still hide in: class index
    /// files (one file may define several classes, so they are
    /// de-duplicated by path), files under the user's PSR-4 roots that the
    /// class index does not know about yet, and the embedded stubs.
    /// Vendor PSR-4 roots are not walked: vendor classes are assumed
    /// complete in the class index.  Already-parsed files are left out,
    /// because the reverse index covers their classes.
    fn unparsed_implementor_sources(&self, project_only: bool) -> PendingSources {
        let loaded_uris: HashSet<String> = self.parsed_uris.read().iter().cloned().collect();
        let index_paths: HashSet<PathBuf> = self
            .symbols
            .fqn_uri_index
            .read()
            .values()
            .filter(|uri| !(project_only && uri.contains("/vendor/")))
            .filter_map(|uri| crate::util::uri_to_path(uri))
            .collect();
        let mut files: Vec<PathBuf> = index_paths
            .iter()
            .filter(|path| !loaded_uris.contains(&crate::util::path_to_uri(path)))
            .cloned()
            .collect();

        let workspace_root = self.workspace.workspace_root.read().clone();
        if let Some(workspace_root) = workspace_root {
            // The vendor dir paths are needed by the walk even though we
            // only walk user PSR-4 roots.  A fallback mapping
            // like `"" => "."` resolves to the workspace root, so the
            // walk must still skip vendor directories (and hidden
            // directories like .git).
            let vendor_dir_paths = self.workspace.vendor_dir_paths.lock().clone();
            let psr4_dirs: Vec<PathBuf> = {
                let mappings = self.workspace.psr4_mappings.read();
                mappings
                    .iter()
                    .map(|m| workspace_root.join(&m.base_path))
                    .filter(|p| p.is_dir())
                    .collect()
            };
            let filters = self.index_filters();
            let mut walked: HashSet<PathBuf> = HashSet::new();
            for dir in &psr4_dirs {
                for php_file in crate::classmap_scanner::collect_php_files_gitignore(
                    dir,
                    &vendor_dir_paths,
                    &filters,
                    None,
                ) {
                    if index_paths.contains(&php_file)
                        || loaded_uris.contains(&crate::util::path_to_uri(&php_file))
                        || !walked.insert(php_file.clone())
                    {
                        continue;
                    }
                    files.push(php_file);
                }
            }
        }

        // `stub_index` maps every stub class to the whole stub file that
        // declares it, so group the names by file to search each file once.
        // Stubs are vendor/built-in definitions, so they are skipped
        // entirely when only project implementors are wanted.
        let mut stubs: Vec<(&'static str, Vec<String>)> = Vec::new();
        if !project_only {
            let stub_idx = self.stub_index.read();
            let mut by_source: HashMap<*const u8, usize> = HashMap::new();
            for (stub_name, &stub_source) in stub_idx.iter() {
                let slot = *by_source.entry(stub_source.as_ptr()).or_insert_with(|| {
                    stubs.push((stub_source, Vec::new()));
                    stubs.len() - 1
                });
                stubs[slot].1.push(stub_name.to_string());
            }
        }

        PendingSources { files, stubs }
    }

    /// Parse the pending sources that name a known subtype and add the
    /// subtypes they declare to `found`.
    ///
    /// A direct subtype's source names its parent, so a cheap byte search
    /// for the target's short name narrows the candidates before anything
    /// is parsed.  A transitive one need not name the target (`class B
    /// extends A` where `A implements Target`), so the search repeats with
    /// the short names of the subtypes each round found, until a round
    /// finds none.  Every class in a parsed file is classified against the
    /// full ancestor chain, so a file is parsed at most once.  The
    /// reverse-index descendants (`gti_candidates`) seed the first round
    /// too, since an unparsed file can extend one of them without naming
    /// the target.  The other way round, an already-parsed class whose
    /// parent was unparsed is reached through the reverse index once that
    /// parent turns up.
    fn scan_sources_for_subtypes(
        &self,
        query: &ImplementorQuery<'_>,
        pending: PendingSources,
        gti_candidates: &[String],
        exclude_non_project: bool,
        found: &mut FoundImplementors,
    ) {
        let PendingSources {
            files: mut pending_files,
            stubs: mut pending_stubs,
        } = pending;
        let class_loader = query.class_loader;
        let direct_only = query.direct_only;
        let progress = self.request_progress.as_deref();
        let mut descendants: HashSet<String> = gti_candidates.iter().cloned().collect();
        let mut searched: HashSet<String> = HashSet::new();
        let mut needles: Vec<String> = Vec::new();
        for name in std::iter::once(query.target_short)
            .chain(gti_candidates.iter().map(|fqn| short_name(fqn)))
            .take(if direct_only { 1 } else { usize::MAX })
        {
            if searched.insert(name.to_string()) {
                needles.push(name.to_string());
            }
        }

        while !needles.is_empty() {
            let finders: Vec<memchr::memmem::Finder<'_>> = needles
                .iter()
                .map(|n| memchr::memmem::Finder::new(n.as_bytes()))
                .collect();
            let mentions_needle = |bytes: &[u8]| finders.iter().any(|f| f.find(bytes).is_some());
            let mut candidates: Vec<Arc<ClassInfo>> = Vec::new();

            if let Some(p) = progress {
                p.add_total((pending_stubs.len() + pending_files.len()) as u64);
            }
            pending_stubs.retain(|(stub_source, stub_names)| {
                if let Some(p) = progress {
                    p.add_done(1);
                }
                if !mentions_needle(stub_source.as_bytes()) {
                    return true;
                }
                candidates.extend(
                    stub_names
                        .iter()
                        .filter_map(|name| class_loader(Fqn::new(name))),
                );
                false
            });
            pending_files.retain(|path| {
                if let Some(p) = progress {
                    p.add_done(1);
                }
                // A class lookup during an earlier round may have parsed
                // this file since the list was built.  Use those classes
                // rather than parsing it again.
                let uri = crate::util::path_to_uri(path);
                let classes = match self.shared_classes_for_uri(&uri) {
                    Some(classes) => classes,
                    None => {
                        let Ok(raw) = crate::classmap_scanner::read_for_scan(path) else {
                            return false;
                        };
                        if !mentions_needle(&raw) {
                            return true;
                        }
                        drop(raw);
                        match self.parse_and_cache_file(path) {
                            Some(classes) => classes,
                            None => return false,
                        }
                    }
                };
                candidates.extend(classes);
                false
            });
            drop(finders);

            needles = Vec::new();
            while let Some(cls) = candidates.pop() {
                if !self.class_descends_from(
                    &cls,
                    query.target_short,
                    query.target_fqn,
                    class_loader,
                    direct_only,
                ) {
                    continue;
                }
                let cls_fqn = cls.fqn().to_string();
                if !descendants.insert(cls_fqn.clone()) {
                    continue;
                }
                if Self::is_listed_kind(&cls, query.include_abstract, direct_only)
                    && self.is_project_fqn(&cls_fqn, exclude_non_project)
                    && found.seen_fqns.insert(cls_fqn.clone())
                {
                    found.result.push(Arc::clone(&cls));
                }
                if direct_only {
                    continue;
                }
                let short = short_name(&cls_fqn);
                if searched.insert(short.to_string()) {
                    needles.push(short.to_string());
                }
                // Subtypes of this class that are already parsed are in
                // the reverse index, but Phase 1 could not reach them
                // while this class was unparsed.
                let children = self.symbols.gti_index.read().get(&cls_fqn).cloned();
                candidates.extend(
                    children
                        .iter()
                        .flatten()
                        .filter(|child| !descendants.contains(*child))
                        .filter_map(|child| class_loader(Fqn::new(child))),
                );
            }
        }
    }

    /// Whether a subtype of `cls`'s kind belongs in the results.  Type
    /// hierarchy (`direct_only`) lists every kind of subtype, while
    /// go-to-implementation never lists interfaces and lists abstract
    /// classes only when `include_abstract` is set.
    fn is_listed_kind(cls: &ClassInfo, include_abstract: bool, direct_only: bool) -> bool {
        direct_only
            || (cls.kind != ClassLikeKind::Interface && (include_abstract || !cls.is_abstract))
    }

    /// Check whether `cls` implements the target interface or extends the
    /// target class (directly or transitively through its parent chain),
    /// whatever kind of class-like `cls` is.  In `direct_only` mode only
    /// its own `extends`/`implements` clauses count.
    ///
    /// Comparisons use fully-qualified names to avoid false positives when
    /// two interfaces in different namespaces share the same short name.
    fn class_descends_from(
        &self,
        cls: &ClassInfo,
        target_short: &str,
        target_fqn: &str,
        class_loader: &ResolvedClassLoader<'_>,
        direct_only: bool,
    ) -> bool {
        // Skip the target class itself — compare by FQN so that
        // classes in different namespaces that share the same short
        // name are not incorrectly excluded.
        if cls.fqn() == target_fqn {
            return false;
        }

        // Whether the target has a known FQN (contains a namespace
        // separator).  When it does, short-name comparison is skipped
        // to avoid false positives between identically-named classes in
        // different namespaces (e.g. App\Logger vs Vendor\Logger).
        let has_fqn = target_fqn.contains('\\');

        // Direct `implements` match (interfaces are FQN after resolution).
        for iface in &cls.interfaces {
            if *iface == target_fqn || (!has_fqn && short_name(iface) == target_short) {
                return true;
            }
        }

        // Direct `extends` match (for abstract class implementations).
        if let Some(parent) = cls.parent_class
            && (&*parent == target_fqn || (!has_fqn && short_name(&parent) == target_short))
        {
            return true;
        }

        // In direct_only mode we only care about the immediate
        // extends/implements/use clauses checked above.
        if direct_only {
            return false;
        }

        // ── Transitive check: walk the interface-extends chains ─────────
        // If ClassC implements InterfaceB, and InterfaceB extends
        // InterfaceA, a go-to-implementation on InterfaceA should find
        // ClassC.  Load each directly-implemented interface and
        // recursively check whether it extends the target.
        for iface in &cls.interfaces {
            if Self::interface_extends_target(
                iface,
                target_short,
                target_fqn,
                has_fqn,
                class_loader,
                0,
            ) {
                return true;
            }
        }

        // ── Transitive check: walk the parent class chain ───────────────
        // A class might extend another class that implements the target
        // interface.  Walk up to a bounded depth to find it.
        for (_, parent_cls) in crate::inheritance::ancestors(cls, class_loader) {
            // Check if the parent implements the target interface.
            for iface in &parent_cls.interfaces {
                if *iface == target_fqn || (!has_fqn && short_name(iface) == target_short) {
                    return true;
                }
                // Also walk the interface's own extends chain.
                if Self::interface_extends_target(
                    iface,
                    target_short,
                    target_fqn,
                    has_fqn,
                    class_loader,
                    0,
                ) {
                    return true;
                }
            }

            // Check if the parent IS the target (for abstract class chains).
            let parent_fqn =
                crate::util::build_fqn(&parent_cls.name, parent_cls.file_namespace.as_deref());
            if parent_fqn == target_fqn {
                return true;
            }
        }

        false
    }

    /// Check whether `iface_name` transitively extends the target interface.
    ///
    /// Loads the interface via `class_loader`, then checks its
    /// `parent_class` (single-extends) and `interfaces` (multi-extends)
    /// lists recursively up to [`MAX_INHERITANCE_DEPTH`].
    fn interface_extends_target(
        iface_name: &str,
        target_short: &str,
        target_fqn: &str,
        has_fqn: bool,
        class_loader: &ResolvedClassLoader<'_>,
        depth: u32,
    ) -> bool {
        if depth >= MAX_INHERITANCE_DEPTH {
            return false;
        }

        let Some(iface_cls) = class_loader(Fqn::new(iface_name)) else {
            return false;
        };

        // `parent_class` stores the first extended interface (kept for
        // backward compatibility); `interfaces` covers the rest, for
        // interfaces that extend more than one parent.
        iface_cls
            .parent_class
            .iter()
            .copied()
            .chain(iface_cls.interfaces.iter().copied())
            .any(|parent_iface| {
                let parent_short = short_name(&parent_iface);
                parent_iface == target_fqn
                    || (!has_fqn && parent_short == target_short)
                    || Self::interface_extends_target(
                        &parent_iface,
                        target_short,
                        target_fqn,
                        has_fqn,
                        class_loader,
                        depth + 1,
                    )
            })
    }

    /// Find the location of a class declaration for an implementor.
    fn locate_class_declaration(
        &self,
        cls: &ClassInfo,
        current_uri: &str,
        current_content: &str,
    ) -> Option<Location> {
        let (class_uri, class_content) =
            self.find_class_file_content(&cls.fqn(), current_uri, current_content)?;
        class_keyword_location(cls, &class_uri, &class_content)
    }
}

/// The classes whose declarations implement `member_name` for the
/// `descendants` of the class `target_fqn`, one entry per declaration.
///
/// A descendant that declares the member itself provides it; one that
/// inherits it unchanged is provided for by the trait or ancestor it
/// inherits it from, in PHP's member precedence order.  A method that is
/// only ever re-declared `abstract` is another declaration rather than an
/// implementation, and a member a descendant inherits from the target
/// itself is the declaration the search started from, so neither counts.
/// Neither does an interface's, since an interface cannot implement.
///
/// Reads class metadata only, so the implementation lens can tell whether
/// a member has any implementation without opening their files.
pub(crate) fn member_implementation_providers(
    target_fqn: &str,
    member_name: &str,
    member_kind: MemberKind,
    descendants: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    let declares = |cls: &ClassInfo| {
        cls.kind != ClassLikeKind::Interface
            && match member_kind {
                MemberKind::Method => cls
                    .get_method_ci(member_name)
                    .is_some_and(|m| !m.is_abstract && !m.is_virtual),
                MemberKind::Property => cls.properties.iter().any(|p| p.name == member_name),
                MemberKind::Constant => cls.constants.iter().any(|c| c.name == member_name),
            }
    };

    let mut seen: HashSet<Atom> = HashSet::new();
    let mut providers = Vec::new();
    for descendant in descendants {
        let provider = if declares(descendant) {
            Arc::clone(descendant)
        } else {
            match crate::inheritance::find_declaring_ancestor(descendant, class_loader, &declares) {
                Some((_, ancestor)) => ancestor,
                None => continue,
            }
        };
        let provider_fqn = provider.fqn();
        if provider_fqn != target_fqn && seen.insert(provider_fqn) {
            providers.push(provider);
        }
    }
    providers
}

/// Order `locations` by file and position and drop duplicates, so a
/// declaration several descendants inherit is listed once and the result
/// does not depend on the order the index was populated in.
fn sort_and_dedup_locations(locations: &mut Vec<Location>) {
    locations.sort_by(|left, right| {
        left.uri
            .as_str()
            .cmp(right.uri.as_str())
            .then(left.range.start.line.cmp(&right.range.start.line))
            .then(left.range.start.character.cmp(&right.range.start.character))
    });
    locations.dedup();
}

#[cfg(test)]
#[path = "implementation_tests.rs"]
mod tests;
