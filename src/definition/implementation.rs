/// Go-to-implementation support (`textDocument/implementation`).
///
/// When the cursor is on an interface name, abstract class name, or any
/// non-final class name, this module finds all concrete implementations
/// (and, for concrete class targets, all subclasses including abstract
/// ones) and returns their locations.  The same applies to method calls
/// where the owning type is an interface or abstract class.
///
/// When the cursor is on a method *definition* inside a concrete class, the
/// reverse direction is also supported: the handler finds the interface or
/// abstract class that declares the method and jumps to it.
///
/// Final classes cannot be extended, so go-to-implementation is a no-op
/// for them.
///
/// # Resolution strategy
///
/// 1. **Determine the target symbol** — consult the precomputed `SymbolMap`
///    for the word under the cursor.
/// 2. **Identify the target type** — resolve the symbol to a `ClassInfo` and
///    check whether it is a non-final class or interface.
/// 3. **Scan for implementors** — walk all classes known to the server
///    (`uri_classes_index`, `fqn_uri_index`, PSR-4 directories) and collect
///    those whose `interfaces` list or `parent_class` matches the target type.
/// 4. **Return locations** — for class-level requests, return the class
///    declaration position; for method-level requests, return the method
///    position in each implementing class.
/// 5. **Reverse jump** — for `MemberDeclaration` symbols on a concrete class,
///    walk the class's interfaces and parent abstract classes to find the
///    prototype method declaration and return its location.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use super::member::MemberKind;
use super::point_location;
use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::find_class_at_offset;
use crate::config::IndexingStrategy;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind};
use crate::text_position::{LineIndex, line_start_byte_offset, position_to_offset};
use crate::type_engine::resolver::CtxLoaders;
use crate::types::{ClassInfo, ClassLikeKind, FileContext, MAX_INHERITANCE_DEPTH, ResolvedType};
use crate::util::{collect_php_files, short_name};

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
                    let class_loader = self.class_loader(&ctx);
                    let current_class = find_class_at_offset(&ctx.classes, sym.start);
                    let target = match ssp_kind {
                        SelfStaticParentKind::Parent => current_class
                            .and_then(|cc| cc.parent_class.as_deref())
                            .and_then(|p| class_loader(p).map(Arc::unwrap_or_clone)),
                        _ => current_class.cloned(),
                    };
                    if let Some(ref t) = target {
                        return self
                            .resolve_class_implementation(uri, content, &t.name, &ctx, sym.start);
                    }
                    return None;
                }
                // Member declaration — reverse jump: from a concrete method
                // definition to the interface/abstract method it implements.
                SymbolKind::MemberDeclaration { name, .. } => {
                    let ctx = self.file_context_at(uri, sym.start);
                    let class_loader = self.class_loader(&ctx);
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

        let fqn = ctx.resolve_name_at(name, name_offset);
        let target = class_loader(&fqn)
            .or_else(|| class_loader(name))
            .map(Arc::unwrap_or_clone)?;

        // Final classes cannot be extended, so there are no implementations.
        if target.is_final {
            return None;
        }

        let target_short = target.name;
        // Compute target FQN from the class's own namespace (most
        // reliable), then fall back to fqn_uri_index, then to the FQN we
        // resolved from the use-map, and finally to the short name.
        let target_fqn = {
            let from_class = crate::util::build_fqn(&target.name, target.file_namespace.as_deref());
            if from_class.contains('\\') {
                from_class
            } else {
                self.class_fqn_for_short(&target_short).unwrap_or_else(|| {
                    if fqn.contains('\\') {
                        fqn.clone()
                    } else {
                        target_short.to_string()
                    }
                })
            }
        };

        let descendants =
            self.implementation_descendants(&target, &target_fqn, &class_loader, false);
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
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
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
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
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
                    && let Some(member_pos) = Self::find_member_position_in_class(
                        &class_content,
                        member_name,
                        member_kind,
                        &iface,
                    )
                    && let Ok(parsed_uri) = Url::parse(&class_uri)
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
                && let Some(member_pos) = Self::find_member_position_in_class(
                    &class_content,
                    member_name,
                    MemberKind::Method,
                    &parent_cls,
                )
                && let Ok(parsed_uri) = Url::parse(&class_uri)
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
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<Vec<Location>> {
        let target_fqn = self.implementor_target_fqn(interface_class);
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
                let member_pos = Self::find_member_position_in_class(
                    &class_content,
                    member_name,
                    member_kind,
                    provider,
                )?;
                Some(point_location(Url::parse(&class_uri).ok()?, member_pos))
            })
            .collect();
        sort_and_dedup_locations(&mut locations);
        locations
    }

    /// The FQN to search implementors of `cls` by: the namespace the class
    /// declares itself when it has one, falling back to whatever the class
    /// index knows about its short name.
    fn implementor_target_fqn(&self, cls: &ClassInfo) -> String {
        let from_class = crate::util::build_fqn(&cls.name, cls.file_namespace.as_deref());
        if from_class.contains('\\') {
            return from_class;
        }
        self.class_fqn_for_short(&cls.name)
            .unwrap_or_else(|| cls.name.to_string())
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
        let current_class = find_class_at_offset(&ctx.classes, cursor_offset);

        let class_loader = self.class_loader(ctx);
        let function_loader = self.function_loader(ctx);
        let laravel_macro_this_resolver = self.laravel_macro_this_resolver(&class_loader);

        // Resolve the subject to candidate classes.
        let rctx = self.resolution_ctx_at(
            current_class,
            &ctx.classes,
            content,
            cursor_offset,
            CtxLoaders::new(
                &class_loader,
                &function_loader,
                &laravel_macro_this_resolver,
            ),
        );

        let candidates = ResolvedType::into_arced_classes(
            crate::type_engine::resolver::resolve_target_classes(&subject, access_kind, &rctx),
        );

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
                &class_loader,
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

            let target_fqn = self.implementor_target_fqn(candidate);
            let descendants =
                self.implementation_descendants(candidate, &target_fqn, &class_loader, false);
            let providers = member_implementation_providers(
                &target_fqn,
                member_name,
                member_kind,
                &descendants,
                &class_loader,
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
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        include_abstract: bool,
        direct_only: bool,
        project_only: bool,
    ) -> Vec<Arc<ClassInfo>> {
        let mut result: Vec<Arc<ClassInfo>> = Vec::new();
        // Track by FQN to avoid short-name collisions across namespaces.
        let mut seen_fqns: HashSet<String> = HashSet::new();
        if self.config().indexing.strategy() == IndexingStrategy::Full
            && !self.workspace_indexed.load(Ordering::Acquire)
        {
            self.ensure_workspace_indexed_for_request();
        }
        let workspace_index_ready = self.workspace_indexed.load(Ordering::Acquire);

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

        // Whether an FQN's indexed source is project code (outside
        // `/vendor/` and not an embedded stub).  A class with no indexed
        // URI is treated as project-local (mirrors the downstream filter in
        // `project_auth_implementors`).
        //
        // The filter applies to the Phase 1 reverse-index candidates
        // whenever those candidates are returned without the follow-up
        // vendor/classmap/stub scans: a `project_only` caller, or the
        // workspace-index-ready fast path below.  The reverse index is
        // populated by *every* parse, so a vendor or stub class loaded
        // earlier in the session (via hover, completion, or resolution)
        // would otherwise leak into results while an identical class that
        // was never loaded would not, making results depend on session
        // history.  The full workspace index parses only project files, so
        // user-only is the deterministic set the fast path should return.
        let exclude_non_project = project_only || (workspace_index_ready && !scan_vendor_target);
        let is_project_fqn = |fqn: &str| -> bool {
            if !exclude_non_project {
                return true;
            }
            match self.symbols.fqn_uri_index.read().get(fqn) {
                Some(uri) => {
                    !uri.contains("/vendor/")
                        && !uri.starts_with("phpantom-stub://")
                        && !uri.starts_with("phpantom-stub-fn://")
                }
                None => true,
            }
        };

        // ── Phase 1: GTI index lookup ───────────────────────────────────
        // Use the reverse inheritance index for O(1) lookup of classes
        // that directly extend/implement/use the target.  Then
        // recursively collect transitive children.
        let gti_candidates: Vec<String> = {
            let gti = self.symbols.gti_index.read();
            if direct_only {
                gti.get(target_fqn).cloned().unwrap_or_default()
            } else {
                // Transitive: BFS collecting all descendants.
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
        };

        for child_fqn in &gti_candidates {
            if seen_fqns.contains(child_fqn) {
                continue;
            }
            if !is_project_fqn(child_fqn) {
                continue;
            }
            if let Some(cls) = class_loader(child_fqn) {
                if !direct_only {
                    if cls.kind == ClassLikeKind::Interface {
                        continue;
                    }
                    if cls.is_abstract && !include_abstract {
                        continue;
                    }
                }
                seen_fqns.insert(child_fqn.clone());
                result.push(cls);
            }
        }

        if workspace_index_ready && !scan_vendor_target {
            return result;
        }

        // The remaining phases scan known-size candidate sets; report
        // per-file progress into the request's scan window (the totals
        // accumulate as each phase adds its candidate list).
        let progress = self.request_progress.as_deref();
        if let Some(p) = progress {
            p.set_scope(80, 100, "Scanning for implementations");
        }

        // ── Phase 2: collect the sources nothing has parsed yet ─────────
        // Class index files (one file may define several classes, so they
        // are de-duplicated by path), files under the user's PSR-4 roots
        // that the class index does not know about yet, and the embedded
        // stubs.  Vendor PSR-4 roots are not walked: vendor classes are
        // assumed complete in the class index.  Already-parsed files are
        // left out, because Phase 1 covered their classes.
        let loaded_uris: HashSet<String> = self.parsed_uris.read().iter().cloned().collect();
        let index_paths: HashSet<PathBuf> = self
            .symbols
            .fqn_uri_index
            .read()
            .values()
            .filter(|uri| !(project_only && uri.contains("/vendor/")))
            .filter_map(|uri| Url::parse(uri).ok().and_then(|u| u.to_file_path().ok()))
            .collect();
        let mut pending_files: Vec<PathBuf> = index_paths
            .iter()
            .filter(|path| !loaded_uris.contains(&crate::util::path_to_uri(path)))
            .cloned()
            .collect();

        let workspace_root = self.workspace.workspace_root.read().clone();
        if let Some(workspace_root) = workspace_root {
            // The vendor dir paths are needed by collect_php_files even
            // though we only walk user PSR-4 roots.  A fallback mapping
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
                for php_file in collect_php_files(dir, &vendor_dir_paths, &filters) {
                    if index_paths.contains(&php_file)
                        || loaded_uris.contains(&crate::util::path_to_uri(&php_file))
                        || !walked.insert(php_file.clone())
                    {
                        continue;
                    }
                    pending_files.push(php_file);
                }
            }
        }

        // `stub_index` maps every stub class to the whole stub file that
        // declares it, so group the names by file to search each file once.
        // Stubs are vendor/built-in definitions, so they are skipped
        // entirely when only project implementors are wanted.
        let mut pending_stubs: Vec<(&'static str, Vec<String>)> = Vec::new();
        if !project_only {
            let stub_idx = self.stub_index.read();
            let mut by_source: HashMap<*const u8, usize> = HashMap::new();
            for (stub_name, &stub_source) in stub_idx.iter() {
                let slot = *by_source.entry(stub_source.as_ptr()).or_insert_with(|| {
                    pending_stubs.push((stub_source, Vec::new()));
                    pending_stubs.len() - 1
                });
                pending_stubs[slot].1.push(stub_name.to_string());
            }
        }

        // ── Phase 3: parse the sources that name a known subtype ───────
        // A direct subtype's source names its parent, so a cheap byte
        // search for the target's short name narrows the candidates before
        // anything is parsed.  A transitive one need not name the target
        // (`class B extends A` where `A implements Target`), so the search
        // repeats with the short names of the subtypes each round found,
        // until a round finds none.  Every class in a parsed file is
        // classified against the full ancestor chain, so a file is parsed
        // at most once.  The Phase 1 descendants seed the first round too,
        // since an unparsed file can extend one of them without naming the
        // target.  The other way round, an already-parsed class whose
        // parent was unparsed is reached through the reverse index once
        // that parent turns up.
        let mut descendants: HashSet<String> = gti_candidates.iter().cloned().collect();
        let mut searched: HashSet<String> = HashSet::new();
        let mut needles: Vec<String> = Vec::new();
        for name in std::iter::once(target_short)
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
                candidates.extend(stub_names.iter().filter_map(|name| class_loader(name)));
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
                    target_short,
                    target_fqn,
                    class_loader,
                    direct_only,
                ) {
                    continue;
                }
                let cls_fqn = cls.fqn().to_string();
                if !descendants.insert(cls_fqn.clone()) {
                    continue;
                }
                if Self::is_listed_kind(&cls, include_abstract, direct_only)
                    && is_project_fqn(&cls_fqn)
                    && seen_fqns.insert(cls_fqn.clone())
                {
                    result.push(Arc::clone(&cls));
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
                        .filter_map(|child| class_loader(child)),
                );
            }
        }

        result
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
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
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
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        depth: u32,
    ) -> bool {
        if depth >= MAX_INHERITANCE_DEPTH {
            return false;
        }

        let Some(iface_cls) = class_loader(iface_name) else {
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

    /// Find a member position scoped to a specific class body.
    ///
    /// When multiple classes in the same file define a method with the same
    /// name, [`find_member_position`](Self::find_member_position) would
    /// always return the first match.  This variant restricts the search
    /// to lines that fall within the class's `start_offset..end_offset`
    /// byte range so that each implementing class resolves to its own
    /// definition.
    fn find_member_position_in_class(
        content: &str,
        member_name: &str,
        kind: MemberKind,
        cls: &ClassInfo,
    ) -> Option<Position> {
        // Fast path: use stored AST offset when available.
        let name_offset = cls.member_name_offset(member_name, kind.as_str());
        if name_offset.is_some() {
            return Self::find_member_position(content, member_name, kind, name_offset);
        }

        // Restrict the search to lines within the class's byte range, so
        // that a member name shared with another class in the same file
        // resolves to this class's own definition.
        let index = LineIndex::new(content);
        let start_line = index.line_of(cls.start_offset as usize);
        let end_line = index.line_of(cls.end_offset as usize);
        let start_byte = line_start_byte_offset(content, start_line);
        let end_byte = line_start_byte_offset(content, end_line + 1);
        let class_body = &content[start_byte..end_byte];

        Self::find_member_position(class_body, member_name, kind, None).map(|pos| Position {
            line: pos.line + start_line as u32,
            character: pos.character,
        })
    }

    /// Get the FQN for a class given its short name, by looking it up in
    /// the `fqn_uri_index`.
    fn class_fqn_for_short(&self, target_short: &str) -> Option<String> {
        let idx = self.symbols.fqn_uri_index.read();
        // Look for an entry whose short name matches.
        for fqn in idx.keys() {
            let short = short_name(fqn);
            if short.eq_ignore_ascii_case(target_short) {
                return Some(fqn.to_owned());
            }
        }
        None
    }

    /// Find the location of a class declaration for an implementor.
    fn locate_class_declaration(
        &self,
        cls: &ClassInfo,
        current_uri: &str,
        current_content: &str,
    ) -> Option<Location> {
        let cls_fqn = crate::util::build_fqn(&cls.name, cls.file_namespace.as_deref());
        let (class_uri, class_content) =
            self.find_class_file_content(&cls_fqn, current_uri, current_content)?;

        if cls.keyword_offset == 0 {
            return None;
        }
        let position =
            crate::text_position::offset_to_position(&class_content, cls.keyword_offset as usize);
        let parsed_uri = Url::parse(&class_uri).ok()?;

        Some(point_location(parsed_uri, position))
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
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
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
mod tests {
    use std::fs;

    use tower_lsp::lsp_types::{Position, Url};

    use super::*;
    use crate::config::{Config, IndexingStrategy};

    #[test]
    fn full_indexed_implementation_uses_gti_without_vendor_fallback() {
        let dir = tempfile::tempdir().expect("temp dir");
        let src = dir.path().join("src");
        let vendor = dir.path().join("vendor");
        fs::create_dir_all(src.join("Contracts")).expect("src contracts dir");
        fs::create_dir_all(src.join("Impl")).expect("src impl dir");
        fs::create_dir_all(vendor.join("Pkg")).expect("vendor pkg dir");

        let interface_php = concat!(
            "<?php\n",
            "namespace App\\Contracts;\n",
            "interface Service {}\n",
        );
        let user_impl_php = concat!(
            "<?php\n",
            "namespace App\\Impl;\n",
            "use App\\Contracts\\Service;\n",
            "class UserService implements Service {}\n",
        );
        let vendor_impl_php = concat!(
            "<?php\n",
            "namespace Vendor\\Pkg;\n",
            "use App\\Contracts\\Service;\n",
            "class VendorService implements Service {}\n",
        );

        let interface_path = src.join("Contracts/Service.php");
        let user_impl_path = src.join("Impl/UserService.php");
        let vendor_impl_path = vendor.join("Pkg/VendorService.php");
        fs::write(&interface_path, interface_php).expect("interface file");
        fs::write(&user_impl_path, user_impl_php).expect("user impl file");
        fs::write(&vendor_impl_path, vendor_impl_php).expect("vendor impl file");

        let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
        backend.add_vendor_dir(&vendor);
        let mut config = Config::default();
        config.indexing.strategy = Some(IndexingStrategy::Full);
        backend.set_config(config);

        backend.symbols.fqn_uri_index.write().insert(
            "Vendor\\Pkg\\VendorService".to_string(),
            Url::from_file_path(&vendor_impl_path)
                .expect("vendor uri")
                .to_string(),
        );

        let interface_uri = Url::from_file_path(&interface_path).expect("interface uri");
        backend.update_ast(interface_uri.as_str(), interface_php);

        let locations = backend
            .resolve_implementation(
                interface_uri.as_str(),
                interface_php,
                Position {
                    line: 2,
                    character: 12,
                },
            )
            .expect("user implementation should be found");

        assert_eq!(
            locations.len(),
            1,
            "full index should use GTI and avoid vendor fallback results: {locations:?}",
        );
        assert_eq!(
            locations[0].uri,
            Url::from_file_path(&user_impl_path).expect("user impl uri")
        );
    }

    #[test]
    fn non_full_indexed_implementation_uses_ready_gti_without_fallback() {
        let dir = tempfile::tempdir().expect("temp dir");
        let src = dir.path().join("src");
        let vendor = dir.path().join("vendor");
        fs::create_dir_all(src.join("Contracts")).expect("src contracts dir");
        fs::create_dir_all(src.join("Impl")).expect("src impl dir");
        fs::create_dir_all(vendor.join("Pkg")).expect("vendor pkg dir");

        let interface_php = concat!(
            "<?php\n",
            "namespace App\\Contracts;\n",
            "interface Service {}\n",
        );
        let user_impl_php = concat!(
            "<?php\n",
            "namespace App\\Impl;\n",
            "use App\\Contracts\\Service;\n",
            "class UserService implements Service {}\n",
        );
        let vendor_impl_php = concat!(
            "<?php\n",
            "namespace Vendor\\Pkg;\n",
            "use App\\Contracts\\Service;\n",
            "class VendorService implements Service {}\n",
        );

        let interface_path = src.join("Contracts/Service.php");
        let user_impl_path = src.join("Impl/UserService.php");
        let vendor_impl_path = vendor.join("Pkg/VendorService.php");
        fs::write(&interface_path, interface_php).expect("interface file");
        fs::write(&user_impl_path, user_impl_php).expect("user impl file");
        fs::write(&vendor_impl_path, vendor_impl_php).expect("vendor impl file");

        let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
        backend.add_vendor_dir(&vendor);
        let mut config = Config::default();
        config.indexing.strategy = Some(IndexingStrategy::Composer);
        backend.set_config(config);
        backend
            .workspace_indexed
            .store(true, std::sync::atomic::Ordering::Release);

        backend.symbols.fqn_uri_index.write().insert(
            "Vendor\\Pkg\\VendorService".to_string(),
            Url::from_file_path(&vendor_impl_path)
                .expect("vendor uri")
                .to_string(),
        );

        let interface_uri = Url::from_file_path(&interface_path).expect("interface uri");
        let user_impl_uri = Url::from_file_path(&user_impl_path).expect("user impl uri");
        backend.update_ast(interface_uri.as_str(), interface_php);
        backend.update_ast(user_impl_uri.as_str(), user_impl_php);

        let locations = backend
            .resolve_implementation(
                interface_uri.as_str(),
                interface_php,
                Position {
                    line: 2,
                    character: 12,
                },
            )
            .expect("user implementation should be found from the ready GTI index");

        assert_eq!(
            locations.len(),
            1,
            "ready non-full indexing should return GTI results without falling back to vendor scans"
        );
        assert_eq!(locations[0].uri, user_impl_uri);
    }

    #[test]
    fn same_short_name_interface_and_implementation_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("Contracts")).expect("contracts dir");
        fs::create_dir_all(src.join("Foo")).expect("foo dir");

        let interface_php = concat!(
            "<?php\n",
            "namespace App\\Contracts;\n",
            "interface HttpClient {}\n",
        );
        let impl_php = concat!(
            "<?php\n",
            "namespace App\\Foo;\n",
            "use App\\Contracts\\HttpClient as HttpClientInterface;\n",
            "class HttpClient implements HttpClientInterface {}\n",
        );

        let interface_path = src.join("Contracts/HttpClient.php");
        let impl_path = src.join("Foo/HttpClient.php");
        fs::write(&interface_path, interface_php).expect("interface file");
        fs::write(&impl_path, impl_php).expect("impl file");

        let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
        let mut config = Config::default();
        config.indexing.strategy = Some(IndexingStrategy::Full);
        backend.set_config(config);

        let interface_uri = Url::from_file_path(&interface_path).expect("interface uri");
        let impl_uri = Url::from_file_path(&impl_path).expect("impl uri");
        backend.update_ast(interface_uri.as_str(), interface_php);
        backend.update_ast(impl_uri.as_str(), impl_php);

        let locations = backend
            .resolve_implementation(
                interface_uri.as_str(),
                interface_php,
                Position {
                    line: 2,
                    character: 12,
                },
            )
            .expect("implementation with same short name should be found");

        assert_eq!(
            locations.len(),
            1,
            "should find exactly one implementation: {locations:?}",
        );
        assert_eq!(locations[0].uri, impl_uri);
    }

    // ─── Method implementations across vendor and inheritance ───────────

    const VENDOR_IFACE: &str = "vendor/symfony/http-kernel/HttpKernelInterface.php";
    const VENDOR_IMPL: &str = "vendor/symfony/http-kernel/HttpKernel.php";
    const VENDOR_SUB_IFACE: &str = "vendor/symfony/http-kernel/KernelInterface.php";
    const VENDOR_ABSTRACT: &str = "vendor/symfony/http-kernel/Kernel.php";
    const PROJECT_KERNEL: &str = "src/Kernel.php";

    /// Build a backend over a temp workspace holding `files` (relative
    /// path, content), with `vendor/` registered as a vendor directory and
    /// every `(fqn, relative path)` in `indexed` present in the FQN → URI
    /// index the way the Composer classmap scan leaves it.
    fn scenario_workspace(
        files: &[(&str, &str)],
        indexed: &[(&str, &str)],
    ) -> (Backend, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(
            dir.path().join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("composer.json");
        for (rel, content) in files {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().expect("file parent")).expect("file dirs");
            fs::write(&path, content).expect("php file");
        }

        let (mappings, _vendor_dir) = crate::composer::parse_composer_json(dir.path());
        let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);
        backend.add_vendor_dir(&dir.path().join("vendor"));
        let mut config = Config::default();
        config.indexing.strategy = Some(IndexingStrategy::Full);
        backend.set_config(config);

        {
            let mut idx = backend.symbols.fqn_uri_index.write();
            for (fqn, rel) in indexed {
                idx.insert(
                    (*fqn).to_string(),
                    Url::from_file_path(dir.path().join(rel))
                        .expect("indexed uri")
                        .to_string(),
                );
            }
        }

        (backend, dir)
    }

    /// Parse `rel` into the backend the way opening it in the editor does,
    /// returning its URI and content.
    fn open_file(backend: &Backend, dir: &std::path::Path, rel: &str) -> (Url, String) {
        let path = dir.join(rel);
        let content = fs::read_to_string(&path).expect("read php file");
        let uri = Url::from_file_path(&path).expect("file uri");
        backend.update_ast(uri.as_str(), &content);
        (uri, content)
    }

    /// The workspace-relative files the returned locations point at.
    fn located_files(locations: &[Location], dir: &std::path::Path) -> Vec<String> {
        locations
            .iter()
            .map(|loc| {
                let path = loc.uri.to_file_path().expect("location path");
                path.strip_prefix(dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    /// A Symfony-like `vendor/` tree: an interface, a concrete direct
    /// implementor, a sub-interface, an abstract class whose `handle()` is
    /// concrete, and a project subclass that only inherits `handle()`.
    fn symfony_like_workspace() -> (Backend, tempfile::TempDir) {
        scenario_workspace(
            &[
                (
                    VENDOR_IFACE,
                    concat!(
                        "<?php\n",
                        "namespace Symfony\\Component\\HttpKernel;\n",
                        "interface HttpKernelInterface\n",
                        "{\n",
                        "    public function handle(string $request): string;\n",
                        "}\n",
                    ),
                ),
                (
                    VENDOR_IMPL,
                    concat!(
                        "<?php\n",
                        "namespace Symfony\\Component\\HttpKernel;\n",
                        "class HttpKernel implements HttpKernelInterface\n",
                        "{\n",
                        "    public function handle(string $request): string\n",
                        "    {\n",
                        "        return 'response';\n",
                        "    }\n",
                        "}\n",
                    ),
                ),
                (
                    VENDOR_SUB_IFACE,
                    concat!(
                        "<?php\n",
                        "namespace Symfony\\Component\\HttpKernel;\n",
                        "interface KernelInterface extends HttpKernelInterface\n",
                        "{\n",
                        "}\n",
                    ),
                ),
                (
                    VENDOR_ABSTRACT,
                    concat!(
                        "<?php\n",
                        "namespace Symfony\\Component\\HttpKernel;\n",
                        "abstract class Kernel implements KernelInterface\n",
                        "{\n",
                        "    public function handle(string $request): string\n",
                        "    {\n",
                        "        return 'kernel';\n",
                        "    }\n",
                        "}\n",
                    ),
                ),
                (
                    PROJECT_KERNEL,
                    concat!(
                        "<?php\n",
                        "namespace App;\n",
                        "use Symfony\\Component\\HttpKernel\\Kernel as BaseKernel;\n",
                        "class Kernel extends BaseKernel\n",
                        "{\n",
                        "}\n",
                    ),
                ),
            ],
            &[
                (
                    "Symfony\\Component\\HttpKernel\\HttpKernelInterface",
                    VENDOR_IFACE,
                ),
                ("Symfony\\Component\\HttpKernel\\HttpKernel", VENDOR_IMPL),
                (
                    "Symfony\\Component\\HttpKernel\\KernelInterface",
                    VENDOR_SUB_IFACE,
                ),
                ("Symfony\\Component\\HttpKernel\\Kernel", VENDOR_ABSTRACT),
                ("App\\Kernel", PROJECT_KERNEL),
            ],
        )
    }

    /// A method declared on a vendor interface resolves to the concrete
    /// implementations that vendor package ships.  The workspace index
    /// covers project files only, so the vendor implementors have to come
    /// from the class-index scans.
    #[test]
    fn vendor_interface_method_finds_vendor_implementations() {
        let (backend, dir) = symfony_like_workspace();
        let (iface_uri, iface_text) = open_file(&backend, dir.path(), VENDOR_IFACE);

        // Cursor on `handle` in `public function handle(...)` (line 4).
        let locations = backend
            .resolve_implementation(
                iface_uri.as_str(),
                &iface_text,
                Position {
                    line: 4,
                    character: 22,
                },
            )
            .expect("vendor implementations of HttpKernelInterface::handle should be found");

        let files = located_files(&locations, dir.path());
        assert!(
            files.contains(&VENDOR_IMPL.to_string()),
            "the concrete direct implementor HttpKernel::handle should be returned: {files:?}"
        );
        assert!(
            !files.contains(&VENDOR_IFACE.to_string()),
            "the queried interface itself is not an implementation: {files:?}"
        );
    }

    /// The class-level request on a vendor interface returns the vendor
    /// classes that implement it, not an empty list.
    #[test]
    fn vendor_interface_class_finds_vendor_implementors() {
        let (backend, dir) = symfony_like_workspace();
        let (iface_uri, iface_text) = open_file(&backend, dir.path(), VENDOR_IFACE);

        // Cursor on `HttpKernelInterface` in the interface declaration.
        let locations = backend
            .resolve_implementation(
                iface_uri.as_str(),
                &iface_text,
                Position {
                    line: 2,
                    character: 12,
                },
            )
            .expect("vendor implementors of HttpKernelInterface should be found");

        let files = located_files(&locations, dir.path());
        assert!(
            files.contains(&VENDOR_IMPL.to_string()),
            "HttpKernel implements the interface directly: {files:?}"
        );
        assert!(
            files.contains(&PROJECT_KERNEL.to_string()),
            "App\\Kernel implements it through the vendor base kernel: {files:?}"
        );
    }

    /// The vendor scan parses only files that name the target or a subtype
    /// found so far.  A class whose file never names the target is still
    /// found through the intermediate class it extends, including an
    /// already-open one whose parent nothing had parsed yet.
    #[test]
    fn vendor_scan_follows_subtypes_without_parsing_unrelated_files() {
        const TARGET: &str = "vendor/acme/queue/src/ShouldQueue.php";
        const BASE: &str = "vendor/acme/queue/src/QueuedJob.php";
        const LEAF: &str = "vendor/acme/queue/src/RetryingJob.php";
        const UNRELATED: &str = "vendor/acme/queue/src/Clock.php";
        const PROJECT_JOB: &str = "src/SendMail.php";
        let (backend, dir) = scenario_workspace(
            &[
                (
                    TARGET,
                    "<?php\nnamespace Acme\\Queue;\ninterface ShouldQueue\n{\n}\n",
                ),
                (
                    BASE,
                    "<?php\nnamespace Acme\\Queue;\nabstract class QueuedJob implements ShouldQueue\n{\n}\n",
                ),
                (
                    LEAF,
                    "<?php\nnamespace Acme\\Queue;\nclass RetryingJob extends QueuedJob\n{\n}\n",
                ),
                (
                    UNRELATED,
                    "<?php\nnamespace Acme\\Queue;\nclass Clock\n{\n}\n",
                ),
                (
                    PROJECT_JOB,
                    "<?php\nnamespace App;\nuse Acme\\Queue\\RetryingJob;\nclass SendMail extends RetryingJob\n{\n}\n",
                ),
            ],
            &[
                ("Acme\\Queue\\ShouldQueue", TARGET),
                ("Acme\\Queue\\QueuedJob", BASE),
                ("Acme\\Queue\\RetryingJob", LEAF),
                ("Acme\\Queue\\Clock", UNRELATED),
                ("App\\SendMail", PROJECT_JOB),
            ],
        );
        let (target_uri, target_text) = open_file(&backend, dir.path(), TARGET);
        open_file(&backend, dir.path(), PROJECT_JOB);

        // Cursor on `ShouldQueue` in the interface declaration.
        let locations = backend
            .resolve_implementation(
                target_uri.as_str(),
                &target_text,
                Position {
                    line: 2,
                    character: 12,
                },
            )
            .expect("implementors of ShouldQueue should be found");

        let mut files = located_files(&locations, dir.path());
        files.sort();
        assert_eq!(files, vec![PROJECT_JOB.to_string(), LEAF.to_string()]);

        let unrelated_uri = crate::util::path_to_uri(&dir.path().join(UNRELATED));
        assert!(
            !backend.parsed_uris.read().contains(&unrelated_uri),
            "a vendor file that names no subtype should not be parsed"
        );
    }

    /// An abstract class whose method body is concrete is a valid method
    /// implementation: `Symfony\Component\HttpKernel\Kernel` is abstract but
    /// owns a concrete `handle()`.
    #[test]
    fn abstract_class_with_concrete_method_is_an_implementation() {
        let (backend, dir) = symfony_like_workspace();
        let (iface_uri, iface_text) = open_file(&backend, dir.path(), VENDOR_IFACE);

        let locations = backend
            .resolve_implementation(
                iface_uri.as_str(),
                &iface_text,
                Position {
                    line: 4,
                    character: 22,
                },
            )
            .expect("implementations of HttpKernelInterface::handle should be found");

        let files = located_files(&locations, dir.path());
        assert!(
            files.contains(&VENDOR_ABSTRACT.to_string()),
            "the abstract Kernel owns the concrete handle() body: {files:?}"
        );
    }

    /// A concrete subclass that inherits the method without overriding it
    /// resolves to the nearest ancestor that actually declares the method.
    #[test]
    fn inherited_method_resolves_to_declaring_ancestor() {
        let (backend, dir) = scenario_workspace(
            &[
                (
                    "src/Handler.php",
                    concat!(
                        "<?php\n",
                        "namespace App;\n",
                        "interface Handler\n",
                        "{\n",
                        "    public function handle(): void;\n",
                        "}\n",
                    ),
                ),
                // Declares handle() but is unrelated to Handler, so it is
                // never itself an implementor of the interface.
                (
                    "src/BaseHandler.php",
                    concat!(
                        "<?php\n",
                        "namespace App;\n",
                        "abstract class BaseHandler\n",
                        "{\n",
                        "    public function handle(): void\n",
                        "    {\n",
                        "    }\n",
                        "}\n",
                    ),
                ),
                (
                    "src/AppHandler.php",
                    concat!(
                        "<?php\n",
                        "namespace App;\n",
                        "class AppHandler extends BaseHandler implements Handler\n",
                        "{\n",
                        "}\n",
                    ),
                ),
            ],
            &[],
        );
        let (iface_uri, iface_text) = open_file(&backend, dir.path(), "src/Handler.php");

        let locations = backend
            .resolve_implementation(
                iface_uri.as_str(),
                &iface_text,
                Position {
                    line: 4,
                    character: 22,
                },
            )
            .expect("the inherited implementation should resolve to its declaring class");

        let files = located_files(&locations, dir.path());
        assert_eq!(
            files,
            vec!["src/BaseHandler.php".to_string()],
            "only the class that declares handle() has a body to jump to: {files:?}"
        );
        assert_eq!(
            locations[0].range.start.line, 4,
            "should point at BaseHandler::handle: {locations:?}"
        );
    }

    /// An abstract re-declaration is another declaration, not an
    /// implementation, so only the class with the body is returned.
    #[test]
    fn abstract_method_redeclaration_is_not_an_implementation() {
        let (backend, dir) = scenario_workspace(
            &[
                (
                    "src/Handler.php",
                    concat!(
                        "<?php\n",
                        "namespace App;\n",
                        "interface Handler\n",
                        "{\n",
                        "    public function handle(): void;\n",
                        "}\n",
                    ),
                ),
                (
                    "src/AbstractHandler.php",
                    concat!(
                        "<?php\n",
                        "namespace App;\n",
                        "abstract class AbstractHandler implements Handler\n",
                        "{\n",
                        "    abstract public function handle(): void;\n",
                        "}\n",
                    ),
                ),
                (
                    "src/RealHandler.php",
                    concat!(
                        "<?php\n",
                        "namespace App;\n",
                        "class RealHandler extends AbstractHandler\n",
                        "{\n",
                        "    public function handle(): void\n",
                        "    {\n",
                        "    }\n",
                        "}\n",
                    ),
                ),
            ],
            &[],
        );
        let (iface_uri, iface_text) = open_file(&backend, dir.path(), "src/Handler.php");

        let locations = backend
            .resolve_implementation(
                iface_uri.as_str(),
                &iface_text,
                Position {
                    line: 4,
                    character: 22,
                },
            )
            .expect("the concrete implementation should be found");

        let files = located_files(&locations, dir.path());
        assert_eq!(
            files,
            vec!["src/RealHandler.php".to_string()],
            "the abstract re-declaration is not an implementation: {files:?}"
        );
    }

    /// A vendor class implementing a *project* interface stays excluded:
    /// the project-only restriction still applies when the queried symbol
    /// belongs to the project.
    #[test]
    fn project_interface_method_still_excludes_vendor_implementors() {
        let (backend, dir) = scenario_workspace(
            &[
                (
                    "src/Contracts/Cacheable.php",
                    concat!(
                        "<?php\n",
                        "namespace App\\Contracts;\n",
                        "interface Cacheable\n",
                        "{\n",
                        "    public function cacheKey(): string;\n",
                        "}\n",
                    ),
                ),
                (
                    "src/Cache/FileCache.php",
                    concat!(
                        "<?php\n",
                        "namespace App\\Cache;\n",
                        "use App\\Contracts\\Cacheable;\n",
                        "class FileCache implements Cacheable\n",
                        "{\n",
                        "    public function cacheKey(): string\n",
                        "    {\n",
                        "        return 'file';\n",
                        "    }\n",
                        "}\n",
                    ),
                ),
                (
                    "vendor/acme/cache/src/AcmeCache.php",
                    concat!(
                        "<?php\n",
                        "namespace Acme\\Cache;\n",
                        "use App\\Contracts\\Cacheable;\n",
                        "class AcmeCache implements Cacheable\n",
                        "{\n",
                        "    public function cacheKey(): string\n",
                        "    {\n",
                        "        return 'acme';\n",
                        "    }\n",
                        "}\n",
                    ),
                ),
            ],
            &[
                ("App\\Contracts\\Cacheable", "src/Contracts/Cacheable.php"),
                ("App\\Cache\\FileCache", "src/Cache/FileCache.php"),
                (
                    "Acme\\Cache\\AcmeCache",
                    "vendor/acme/cache/src/AcmeCache.php",
                ),
            ],
        );

        // The vendor implementor was parsed earlier in the session, which
        // puts it into the reverse-inheritance index.
        open_file(&backend, dir.path(), "vendor/acme/cache/src/AcmeCache.php");
        let (iface_uri, iface_text) =
            open_file(&backend, dir.path(), "src/Contracts/Cacheable.php");

        let locations = backend
            .resolve_implementation(
                iface_uri.as_str(),
                &iface_text,
                Position {
                    line: 4,
                    character: 22,
                },
            )
            .expect("the project implementation should be found");

        let files = located_files(&locations, dir.path());
        assert_eq!(
            files,
            vec!["src/Cache/FileCache.php".to_string()],
            "a vendor implementor of a project interface must stay excluded: {files:?}"
        );
    }
}
