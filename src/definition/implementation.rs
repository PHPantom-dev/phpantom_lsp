use std::collections::HashSet;
use std::path::PathBuf;
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
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use super::member::MemberKind;
use super::point_location;
use crate::Backend;
use crate::completion::resolver::ResolutionCtx;
use crate::config::IndexingStrategy;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind};
use crate::types::{ClassInfo, ClassLikeKind, FileContext, MAX_INHERITANCE_DEPTH, ResolvedType};
use crate::util::{collect_php_files, find_class_at_offset, position_to_offset, short_name};

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
                    let ctx = self.file_context(uri);
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
                    let ctx = self.file_context(uri);
                    return self.resolve_class_implementation(uri, content, name, &ctx, sym.start);
                }
                // self/static/parent — resolve the keyword to the current
                // class and check whether it is an interface/abstract.
                SymbolKind::SelfStaticParent(ssp_kind) => {
                    let ctx = self.file_context(uri);
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
                    let ctx = self.file_context(uri);
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

        // Whether the target is a concrete (non-abstract, non-interface)
        // class.  When it is, we include abstract subclasses in the
        // results because the user is exploring the class hierarchy
        // rather than looking for instantiable implementations.
        let target_is_concrete = target.kind != ClassLikeKind::Interface && !target.is_abstract;

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

        let implementors = self.find_implementors(
            &target_short,
            &target_fqn,
            &class_loader,
            target_is_concrete,
            false,
            false,
        );

        if implementors.is_empty() {
            return None;
        }

        let mut locations = Vec::new();
        for imp in &implementors {
            if let Some(loc) = self.locate_class_declaration(imp, uri, content) {
                locations.push(loc);
            }
        }

        if locations.is_empty() {
            None
        } else {
            Some(locations)
        }
    }

    /// Reverse jump: from a method definition in a concrete class to the
    /// interface or abstract class that declares the prototype.
    ///
    /// When the cursor is on a method name at its definition site (e.g.
    /// `public function handle()` in a class that implements `Handler`),
    /// this finds the interface/abstract method declaration and returns
    /// its location.
    fn resolve_reverse_implementation(
        &self,
        uri: &str,
        content: &str,
        current_class: &ClassInfo,
        member_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<Vec<Location>> {
        // For interfaces and abstract classes, the forward direction
        // applies: find concrete implementors that define the method.
        if current_class.kind == ClassLikeKind::Interface || current_class.is_abstract {
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
        let all_ifaces = self.collect_all_interfaces(current_class, class_loader);
        for iface_name in &all_ifaces {
            if let Some(iface) = class_loader(iface_name) {
                let has_member = iface.has_method(member_name)
                    || iface.properties.iter().any(|p| p.name == member_name);
                if has_member {
                    let member_kind = if iface.has_method(member_name) {
                        MemberKind::Method
                    } else {
                        MemberKind::Property
                    };
                    if let Some((class_uri, class_content)) =
                        self.find_class_file_content(iface_name, uri, content)
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
        }

        // Check parent abstract classes for an abstract method with the
        // same name.
        let mut current = current_class.parent_class;
        let mut depth = 0u32;
        while let Some(parent_name) = current {
            if depth >= MAX_INHERITANCE_DEPTH {
                break;
            }
            depth += 1;

            if let Some(parent_cls) = class_loader(&parent_name) {
                // Only consider abstract methods on abstract parents.
                if parent_cls.is_abstract || parent_cls.kind == ClassLikeKind::Interface {
                    let has_method = parent_cls.has_method(member_name);
                    if has_method
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
                current = parent_cls.parent_class;
            } else {
                break;
            }
        }

        if locations.is_empty() {
            None
        } else {
            Some(locations)
        }
    }

    /// Collect all interface names from a class and its parent chain.
    ///
    /// Walks the class's `interfaces` list and its parent class chain,
    /// collecting all interface names (including those inherited from
    /// parents).  Also walks interface-extends chains transitively.
    fn collect_all_interfaces(
        &self,
        cls: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Vec<String> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();

        // Direct interfaces.
        for iface in &cls.interfaces {
            let s = iface.to_string();
            if seen.insert(s.clone()) {
                result.push(s.clone());
                // Also collect interfaces that this interface extends.
                self.collect_parent_interfaces(&s, class_loader, &mut result, &mut seen);
            }
        }

        // Interfaces from parent classes.
        let mut current = cls.parent_class;
        let mut depth = 0u32;
        while let Some(parent_name) = current {
            if depth >= MAX_INHERITANCE_DEPTH {
                break;
            }
            depth += 1;
            if let Some(parent_cls) = class_loader(&parent_name) {
                for iface in &parent_cls.interfaces {
                    let s = iface.to_string();
                    if seen.insert(s.clone()) {
                        result.push(s.clone());
                        self.collect_parent_interfaces(&s, class_loader, &mut result, &mut seen);
                    }
                }
                current = parent_cls.parent_class;
            } else {
                break;
            }
        }

        result
    }

    /// Recursively collect interfaces that an interface extends.
    fn collect_parent_interfaces(
        &self,
        iface_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        result: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        let Some(iface) = class_loader(iface_name) else {
            return;
        };
        // Check parent_class (first extended interface).
        if let Some(parent) = iface.parent_class
            && seen.insert(parent.to_string())
        {
            result.push(parent.to_string());
            self.collect_parent_interfaces(&parent, class_loader, result, seen);
        }
        // Check interfaces list (multi-extends).
        for parent_iface in &iface.interfaces {
            let s = parent_iface.to_string();
            if seen.insert(s.clone()) {
                result.push(s.clone());
                self.collect_parent_interfaces(parent_iface, class_loader, result, seen);
            }
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
        let target_short = &interface_class.name;
        let target_fqn = self
            .class_fqn_for_short(target_short)
            .unwrap_or(target_short.to_string());

        let implementors =
            self.find_implementors(target_short, &target_fqn, class_loader, false, false, false);

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

        let mut locations = Vec::new();
        for imp in &implementors {
            // Check that the implementor owns (not inherits) this member.
            let owns_member = match member_kind {
                MemberKind::Method => imp.has_method(member_name),
                MemberKind::Property => imp.properties.iter().any(|p| p.name == member_name),
                MemberKind::Constant => imp.constants.iter().any(|c| c.name == member_name),
            };
            if !owns_member {
                continue;
            }

            if let Some((class_uri, class_content)) =
                self.find_class_file_content(&imp.name, uri, content)
                && let Some(member_pos) = Self::find_member_position_in_class(
                    &class_content,
                    member_name,
                    member_kind,
                    imp,
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

        // Resolve the subject to candidate classes.
        let rctx = ResolutionCtx {
            current_class,
            all_classes: &ctx.classes,
            content,
            cursor_offset,
            class_loader: &class_loader,
            resolved_class_cache: Some(&self.resolved_class_cache),
            function_loader: Some(&function_loader),
            scope_var_resolver: None,
            is_in_static_method: false,
            preserve_static: false,
        };

        let candidates = ResolvedType::into_arced_classes(
            crate::completion::resolver::resolve_target_classes(&subject, access_kind, &rctx),
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

            let target_short = &candidate.name;
            let target_fqn = self
                .class_fqn_for_short(target_short)
                .unwrap_or(target_short.to_string());

            let implementors = self.find_implementors(
                target_short,
                &target_fqn,
                &class_loader,
                false,
                false,
                false,
            );

            for imp in &implementors {
                // Check that the implementor actually has this member.
                let imp_merged = crate::virtual_members::resolve_class_fully_cached(
                    imp,
                    &class_loader,
                    &self.resolved_class_cache,
                );
                let imp_has = match member_kind {
                    MemberKind::Method => imp_merged.has_method(member_name),
                    MemberKind::Property => {
                        imp_merged.properties.iter().any(|p| p.name == member_name)
                    }
                    MemberKind::Constant => {
                        imp_merged.constants.iter().any(|c| c.name == member_name)
                    }
                };

                if !imp_has {
                    continue;
                }

                // Find the member position in the implementor's file.
                // We want the member defined directly on this class (not
                // inherited), so check the un-merged class first.
                let owns_member = match member_kind {
                    MemberKind::Method => imp.has_method(member_name),
                    MemberKind::Property => imp.properties.iter().any(|p| p.name == member_name),
                    MemberKind::Constant => imp.constants.iter().any(|c| c.name == member_name),
                };

                if !owns_member {
                    // The member is inherited — the implementor doesn't
                    // override it, so there's no definition to jump to
                    // in this class.
                    continue;
                }

                if let Some((class_uri, class_content)) =
                    self.find_class_file_content(&imp.name, uri, content)
                    && let Some(member_pos) = Self::find_member_position_in_class(
                        &class_content,
                        member_name,
                        member_kind,
                        imp,
                    )
                    && let Ok(parsed_uri) = Url::parse(&class_uri)
                {
                    let loc = point_location(parsed_uri, member_pos);
                    if !all_locations.contains(&loc) {
                        all_locations.push(loc);
                    }
                }
            }
        }

        // If no interface/abstract candidate was found, try treating the
        // request as a regular "find all overrides" — useful for concrete
        // base-class methods too.
        if all_locations.is_empty() {
            return None;
        }

        Some(all_locations)
    }

    /// Find all classes that implement or extend the target.
    ///
    /// Scans:
    /// 1. All classes already in `uri_classes_index` (open files + autoload-discovered)
    /// 2. All classes loadable via `fqn_uri_index`
    /// 3. Class index files not yet loaded — string pre-filter then parse
    /// 4. Embedded PHP stubs — string pre-filter then lazy parse
    /// 5. User PSR-4 directories — walk for `.php` files not covered by
    ///    the class index, string pre-filter then parse.  Vendor PSR-4
    ///    roots are skipped because vendor classes are assumed complete
    ///    in the class index (Phase 3).
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
    /// they are loaded: the index scans (Phases 1-3) drop any candidate
    /// whose source lives under a `/vendor/` directory, and the embedded
    /// stub scan (Phase 4) is skipped entirely.  Only the project's own
    /// classes (already parsed, so their lookups hit the cache) are
    /// examined.  This keeps callers that need only project implementors
    /// (e.g. the Laravel auth-model floor) from paying the cost of loading
    /// and parsing every class in a large vendor tree.
    pub(crate) fn find_implementors(
        &self,
        target_short: &str,
        target_fqn: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        include_abstract: bool,
        direct_only: bool,
        project_only: bool,
    ) -> Vec<ClassInfo> {
        // Whether an FQN's indexed source lives outside `/vendor/`.  A class
        // with no indexed URI is treated as project-local (mirrors the
        // downstream filter in `project_auth_implementors`).
        let is_project_fqn = |fqn: &str| -> bool {
            if !project_only {
                return true;
            }
            match self.fqn_uri_index.read().get(fqn) {
                Some(uri) => !uri.contains("/vendor/"),
                None => true,
            }
        };
        let mut result: Vec<ClassInfo> = Vec::new();
        // Track by FQN to avoid short-name collisions across namespaces.
        let mut seen_fqns: HashSet<String> = HashSet::new();
        let workspace_index_ready = if self.config().indexing.strategy() == IndexingStrategy::Full {
            if !self.workspace_indexed.load(Ordering::Acquire) {
                self.ensure_workspace_indexed();
            }
            self.workspace_indexed.load(Ordering::Acquire)
        } else {
            self.workspace_indexed.load(Ordering::Acquire)
        };

        // ── Phase 1: GTI index lookup ───────────────────────────────────
        // Use the reverse inheritance index for O(1) lookup of classes
        // that directly extend/implement/use the target.  Then
        // recursively collect transitive children.
        let gti_candidates: Vec<String> = {
            let gti = self.gti_index.read();
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
                result.push(Arc::unwrap_or_clone(cls));
            }
        }

        if workspace_index_ready {
            return result;
        }

        // ── Phase 2: scan fqn_uri_index for classes not yet in uri_classes_index ────
        let index_entries: Vec<(String, String)> = {
            let idx = self.fqn_uri_index.read();
            idx.iter()
                .map(|(fqn, uri)| (fqn.to_owned(), uri.clone()))
                .collect()
        };

        for (fqn, uri) in &index_entries {
            if seen_fqns.contains(fqn) {
                continue;
            }
            if project_only && uri.contains("/vendor/") {
                continue;
            }
            if let Some(cls) = class_loader(fqn)
                && self.class_implements_or_extends(
                    &cls,
                    target_short,
                    target_fqn,
                    class_loader,
                    include_abstract,
                    direct_only,
                )
            {
                let cls_fqn = crate::util::build_fqn(&cls.name, cls.file_namespace.as_deref());
                if seen_fqns.insert(cls_fqn) {
                    result.push(Arc::unwrap_or_clone(cls));
                }
            }
        }

        // ── Phase 3: scan class index files with string pre-filter ────────
        // Collect unique file paths from the class index (one file may define
        // multiple classes, so we de-duplicate by path and scan each file
        // at most once).  Files already present in uri_classes_index were covered by
        // Phase 1 and can be skipped.
        let index_paths: HashSet<PathBuf> = self
            .fqn_uri_index
            .read()
            .values()
            .filter(|uri| !(project_only && uri.contains("/vendor/")))
            .filter_map(|uri| Url::parse(uri).ok().and_then(|u| u.to_file_path().ok()))
            .collect();

        let loaded_uris: HashSet<String> = self.parsed_uris.read().iter().cloned().collect();

        for path in &index_paths {
            let uri = crate::util::path_to_uri(path);
            if loaded_uris.contains(&uri) {
                continue;
            }

            // Cheap pre-filter: read the raw file and skip it if the
            // source doesn't mention the target name at all.
            let raw = match crate::classmap_scanner::read_for_scan(path) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if memchr::memmem::find(&raw, target_short.as_bytes()).is_none() {
                continue;
            }

            // Parse the file, cache it, and check every class it defines.
            if let Some(classes) = self.parse_and_cache_file(path) {
                for cls in &classes {
                    let cls_fqn = crate::util::build_fqn(&cls.name, cls.file_namespace.as_deref());
                    if seen_fqns.contains(&cls_fqn) {
                        continue;
                    }
                    if self.class_implements_or_extends(
                        cls,
                        target_short,
                        target_fqn,
                        class_loader,
                        include_abstract,
                        direct_only,
                    ) {
                        seen_fqns.insert(cls_fqn);
                        result.push(ClassInfo::clone(cls));
                    }
                }
            }
        }

        // ── Phase 4: scan embedded stubs with string pre-filter ─────────
        // Stubs are static strings baked into the binary.  A cheap text
        // search for the target name narrows candidates before we parse.
        // Parsing is lazy and cached in uri_classes_index, so subsequent lookups
        // hit Phase 1.  Stubs are vendor/built-in definitions, so they are
        // skipped entirely when only project implementors are wanted.
        if !project_only {
            let stub_idx = self.stub_index.read();
            for (stub_name, &stub_source) in stub_idx.iter() {
                if seen_fqns.contains(stub_name) {
                    continue;
                }
                // Cheap pre-filter: skip stubs whose source doesn't mention
                // the target name at all.
                if !stub_source.contains(target_short) {
                    continue;
                }
                if let Some(cls) = class_loader(stub_name)
                    && self.class_implements_or_extends(
                        &cls,
                        target_short,
                        target_fqn,
                        class_loader,
                        include_abstract,
                        direct_only,
                    )
                {
                    let cls_fqn = crate::util::build_fqn(&cls.name, cls.file_namespace.as_deref());
                    if seen_fqns.insert(cls_fqn) {
                        result.push(Arc::unwrap_or_clone(cls));
                    }
                }
            }
        }

        // ── Phase 5: scan user PSR-4 directories for files not in class index ──
        // The user may have created classes that are not yet in the
        // class index.  Walk user PSR-4 roots only — vendor classes are
        // assumed complete in the class index (Phase 3) and should not
        // require a filesystem walk.
        let workspace_root = self.workspace_root.read().clone();
        if let Some(workspace_root) = workspace_root {
            // The vendor dir paths are needed by collect_php_files even
            // though we only walk user PSR-4 roots.  A fallback mapping
            // like `"" => "."` resolves to the workspace root, so the
            // walk must still skip vendor directories (and hidden
            // directories like .git).
            let vendor_dir_paths = self.vendor_dir_paths.lock().clone();

            let psr4_dirs: Vec<PathBuf> = {
                let mappings = self.psr4_mappings.read();
                mappings
                    .iter()
                    .map(|m| workspace_root.join(&m.base_path))
                    .filter(|p| p.is_dir())
                    .collect()
            };

            let loaded_uris_p5: HashSet<String> = self.parsed_uris.read().iter().cloned().collect();

            for dir in &psr4_dirs {
                for php_file in collect_php_files(dir, &vendor_dir_paths) {
                    // Skip files already covered by the class index (Phase 3).
                    if index_paths.contains(&php_file) {
                        continue;
                    }

                    let uri = crate::util::path_to_uri(&php_file);
                    if loaded_uris_p5.contains(&uri) {
                        continue;
                    }

                    let raw = match std::fs::read_to_string(&php_file) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    if !raw.contains(target_short) {
                        continue;
                    }

                    if let Some(classes) = self.parse_and_cache_file(&php_file) {
                        for cls in &classes {
                            let cls_fqn =
                                crate::util::build_fqn(&cls.name, cls.file_namespace.as_deref());
                            if seen_fqns.contains(&cls_fqn) {
                                continue;
                            }
                            if self.class_implements_or_extends(
                                cls,
                                target_short,
                                target_fqn,
                                class_loader,
                                include_abstract,
                                direct_only,
                            ) {
                                seen_fqns.insert(cls_fqn);
                                result.push(ClassInfo::clone(cls));
                            }
                        }
                    }
                }
            }
        }

        result
    }

    /// Check whether `cls` implements the target interface or extends the
    /// target class (directly or transitively through its parent chain).
    ///
    /// Comparisons use fully-qualified names to avoid false positives when
    /// two interfaces in different namespaces share the same short name.
    ///
    /// When `include_abstract` is `false`, abstract classes and interfaces
    /// are skipped (only concrete implementations are returned).  When
    /// `true`, abstract subclasses are included in the results.
    fn class_implements_or_extends(
        &self,
        cls: &ClassInfo,
        target_short: &str,
        target_fqn: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        include_abstract: bool,
        direct_only: bool,
    ) -> bool {
        // Build the FQN of the candidate class for comparison.
        let cls_fqn = crate::util::build_fqn(&cls.name, cls.file_namespace.as_deref());

        // Skip the target class itself.
        if cls_fqn == target_fqn || cls.name == target_short {
            return false;
        }

        // In direct_only mode (type hierarchy), interfaces that extend
        // the target are valid subtypes, and traits that use the target
        // are too.  In normal mode (go-to-implementation), interfaces
        // are never implementations.
        if !direct_only {
            if cls.kind == ClassLikeKind::Interface {
                return false;
            }
            if cls.is_abstract && !include_abstract {
                return false;
            }
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
        let mut current = cls.parent_class;
        let mut depth = 0u32;

        while let Some(parent_name) = current {
            if depth >= MAX_INHERITANCE_DEPTH {
                break;
            }
            depth += 1;

            if let Some(parent_cls) = class_loader(&parent_name) {
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

                current = parent_cls.parent_class;
            } else {
                break;
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

        // Check `parent_class` (first extended interface stored here for
        // backward compatibility).
        if let Some(parent) = iface_cls.parent_class {
            let parent_short = short_name(&parent);
            if &*parent == target_fqn || (!has_fqn && parent_short == target_short) {
                return true;
            }
            if Self::interface_extends_target(
                &parent,
                target_short,
                target_fqn,
                has_fqn,
                class_loader,
                depth + 1,
            ) {
                return true;
            }
        }

        // Check all entries in `interfaces` (covers multi-extends for
        // interfaces that extend more than one parent).
        for parent_iface in &iface_cls.interfaces {
            let parent_short = short_name(parent_iface);
            if *parent_iface == target_fqn || (!has_fqn && parent_short == target_short) {
                return true;
            }
            if Self::interface_extends_target(
                parent_iface,
                target_short,
                target_fqn,
                has_fqn,
                class_loader,
                depth + 1,
            ) {
                return true;
            }
        }

        false
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

        // Convert byte offsets to line numbers.
        let start_line = content
            .get(..cls.start_offset as usize)
            .map(|s| s.matches('\n').count())
            .unwrap_or(0);
        let end_line = content
            .get(..cls.end_offset as usize)
            .map(|s| s.matches('\n').count())
            .unwrap_or(usize::MAX);

        // Build a sub-content containing only the class body lines and
        // delegate to the existing searcher, adjusting the result line.
        let class_lines: Vec<&str> = content
            .lines()
            .skip(start_line)
            .take(end_line - start_line + 1)
            .collect();
        let class_body = class_lines.join("\n");

        Self::find_member_position(&class_body, member_name, kind, None).map(|pos| Position {
            line: pos.line + start_line as u32,
            character: pos.character,
        })
    }

    /// Get the FQN for a class given its short name, by looking it up in
    /// the `fqn_uri_index`.
    fn class_fqn_for_short(&self, target_short: &str) -> Option<String> {
        let idx = self.fqn_uri_index.read();
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
        let (class_uri, class_content) =
            self.find_class_file_content(&cls.name, current_uri, current_content)?;

        if cls.keyword_offset == 0 {
            return None;
        }
        let position = crate::util::offset_to_position(&class_content, cls.keyword_offset as usize);
        let parsed_uri = Url::parse(&class_uri).ok()?;

        Some(point_location(parsed_uri, position))
    }
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

        backend.fqn_uri_index.write().insert(
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

        backend.fqn_uri_index.write().insert(
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
}
