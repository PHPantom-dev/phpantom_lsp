//! Which classes a member reference search is about.
//!
//! A member search is scoped to the class hierarchy that declares the
//! member, so an access on an unrelated class that merely shares the name
//! is excluded.  This module works that hierarchy out: from a receiver or
//! a declaration, through ancestors and descendants, Laravel macros, and
//! the Model/Builder bridging that Eloquent's magic requires.

use super::*;

use std::collections::HashMap;

use crate::class_lookup::find_class_at_offset;
use crate::types::ClassInfo;

/// The class a member declared at `offset` belongs to.
///
/// The offset of a declaration in a class body is inside the class; the
/// offset of one a class docblock documents (`@method`, `@property`) is
/// before the class's opening brace, so the nearest class whose body
/// starts past the offset is the one whose docblock holds it.
pub(super) fn enclosing_class_for_member(
    classes: &[Arc<ClassInfo>],
    offset: u32,
) -> Option<&ClassInfo> {
    find_class_at_offset(classes, offset).or_else(|| {
        classes
            .iter()
            .map(|c| c.as_ref())
            .filter(|c| c.keyword_offset > 0 && offset < c.start_offset)
            .min_by_key(|c| c.start_offset)
    })
}

/// The classes a member search counts as carrying the member it searches for.
///
/// A scope built from the classes that *declare* the member also has to cover
/// everything that inherits the declaration, and the reverse-inheritance
/// index answers that in one pass.  What the index cannot answer is a class
/// nothing has parsed yet: a package class is parsed the first time something
/// needs it, so an implementor sitting in a dependency has no edge in the
/// index until then, and every access on it would be judged to be on an
/// unrelated class.  A receiver the index did not account for is therefore
/// settled by walking up from the receiver's own class instead, which loads
/// what that walk needs.  Without it the same search answers differently
/// depending on what the session parsed before it.
#[derive(Clone)]
pub(crate) struct MemberScope(Arc<MemberScopeInner>);

struct MemberScopeInner {
    /// The declaring classes plus every descendant the reverse-inheritance
    /// index knew about when the scope was built.
    indexed: HashSet<String>,
    /// The declaring classes, empty for a scope that is exactly `indexed`.
    roots: HashSet<String>,
    /// What the upward walk has already settled, so a receiver that recurs
    /// across the thousands of files a search scans is walked once.
    walked: parking_lot::RwLock<HashMap<String, bool>>,
}

impl MemberScope {
    /// A scope that is exactly `fqns`, with no walk behind it.
    pub(crate) fn exact(fqns: HashSet<String>) -> Self {
        Self(Arc::new(MemberScopeInner {
            indexed: fqns,
            roots: HashSet::new(),
            walked: parking_lot::RwLock::new(HashMap::new()),
        }))
    }

    /// The classes that inherit the member from `roots`, with the walk that
    /// reaches the ones the index has no edge for.
    fn descendants_of(roots: HashSet<String>, indexed: HashSet<String>) -> Self {
        Self(Arc::new(MemberScopeInner {
            indexed,
            roots,
            walked: parking_lot::RwLock::new(HashMap::new()),
        }))
    }

    /// The classes the scope holds without walking: what a caller that needs
    /// to enumerate the scope rather than test one class against it gets.
    pub(super) fn indexed(&self) -> &HashSet<String> {
        &self.0.indexed
    }

    /// Whether a receiver resolved to `fqn` carries the searched member.
    pub(crate) fn contains(&self, backend: &Backend, fqn: &str) -> bool {
        let fqn = strip_fqn_prefix(fqn);
        if self.0.indexed.contains(fqn) {
            return true;
        }
        if self.0.roots.is_empty() {
            return false;
        }
        if let Some(&walked) = self.0.walked.read().get(fqn) {
            return walked;
        }
        let inherits = backend.inherits_from_any(fqn, &self.0.roots);
        self.0.walked.write().insert(fqn.to_string(), inherits);
        inherits
    }
}

impl Backend {
    /// Resolve the class hierarchy for a `MemberAccess` subject.
    ///
    /// Returns `(hierarchy, declaration_scope)`, both `None` when the
    /// subject cannot be resolved to at least one class.  `hierarchy` scopes
    /// member *access* sites (the seed FQNs' full ancestor/descendant/
    /// Laravel-builder closure); `declaration_scope` additionally narrows
    /// *declaration* sites to the classes that actually declare
    /// `member_name`.  The two coincide except for Laravel macros, where the
    /// macro's registered target is narrower than the full class hierarchy.
    pub(super) fn resolve_member_access_scopes(
        &self,
        uri: &str,
        subject_text: &str,
        is_static: bool,
        span_start: u32,
        member_name: &str,
        mode: ReferenceSearchMode,
    ) -> (Option<MemberScope>, Option<MemberScope>) {
        let ctx = self.file_context(uri);
        let Some(content) = self.reference_file_content(uri) else {
            return (None, None);
        };
        let fqns =
            self.resolve_subject_to_fqns(subject_text, is_static, &ctx, span_start, &content);
        if fqns.is_empty() {
            return (None, None);
        }
        if let Some(macro_targets) = self.collect_macro_declaring_targets(&fqns, member_name) {
            return (
                Some(self.collect_hierarchy_for_fqns(&macro_targets)),
                Some(self.descendant_scope(
                    macro_targets.iter().map(|fqn| normalize_fqn(fqn)).collect(),
                )),
            );
        }
        let member_scope = self
            .collect_member_receiver_scope(
                &fqns,
                member_name,
                is_static,
                mode.include_declaring_interfaces(),
            )
            .unwrap_or_else(|| self.collect_hierarchy_for_fqns(&fqns));
        (Some(member_scope.clone()), Some(member_scope))
    }

    /// Resolve the class hierarchies for a `MemberDeclaration` at a given
    /// offset.
    ///
    /// Finds the enclosing class and builds the scopes from it.  Returns
    /// `(hierarchy, declaration_scope)` as
    /// [`resolve_member_access_scopes`](Self::resolve_member_access_scopes)
    /// does: the hierarchy scopes access sites, and is the class's whole
    /// hierarchy when nothing in it is found to declare the member; the
    /// declaration scope is only the classes that do declare it, so it is
    /// `None` in that case.  `None` altogether when no class encloses the
    /// offset.
    pub(super) fn resolve_member_declaration_scopes(
        &self,
        uri: &str,
        offset: u32,
        member_name: &str,
        is_static: bool,
        mode: ReferenceSearchMode,
    ) -> Option<(MemberScope, Option<MemberScope>)> {
        let classes: Vec<Arc<ClassInfo>> = self
            .symbols
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let fqn = enclosing_class_for_member(&classes, offset)?
            .fqn()
            .to_string();
        let declaration_scope = self.collect_member_receiver_scope(
            std::slice::from_ref(&fqn),
            member_name,
            is_static,
            mode.include_declaring_interfaces(),
        );
        let hierarchy = declaration_scope
            .clone()
            .unwrap_or_else(|| self.collect_hierarchy_for_fqns(&[fqn]));
        Some((hierarchy, declaration_scope))
    }

    /// Resolve a member-access subject to the FQN(s) of its type(s), using
    /// the shared subject-resolution utility.  Falls back to a Laravel
    /// static-builder-entrypoint heuristic (e.g. `Model::where(...)`) when
    /// the general resolver returns nothing.
    pub(super) fn resolve_subject_to_fqns(
        &self,
        subject_text: &str,
        is_static: bool,
        ctx: &crate::types::FileContext,
        access_offset: u32,
        content: &str,
    ) -> Vec<String> {
        let resolved = match self.resolve_subject_type_at(
            subject_text,
            is_static,
            ctx,
            access_offset,
            content,
        ) {
            Some(php_type) => {
                self.class_names_to_fqns(php_type.top_level_class_names(), ctx, access_offset)
            }
            None => self.resolve_static_laravel_builder_subject_to_fqns(
                subject_text,
                ctx.use_map_at(access_offset),
                ctx.namespace_at(access_offset),
                &self.class_loader_with(
                    &ctx.classes,
                    ctx.use_map_at(access_offset),
                    ctx.namespace_at(access_offset),
                ),
            ),
        };

        // Doctrine types `getRepository()` as its generic repository, so when
        // that (or nothing) is all the type engine knows, the entity class the
        // call names picks the concrete repository instead.
        if resolved
            .iter()
            .all(|fqn| is_generic_doctrine_repository(fqn))
            && (subject_text.contains("getRepository") || subject_text.starts_with('$'))
        {
            let doctrine_repository_fqns = self.resolve_doctrine_repository_subject_to_fqns(
                subject_text,
                ctx,
                access_offset,
                content,
            );
            if !doctrine_repository_fqns.is_empty() {
                return doctrine_repository_fqns;
            }
        }
        resolved
    }

    /// The type a member-access subject resolves to, through the shared
    /// subject-resolution utility.
    pub(super) fn resolve_subject_type_at(
        &self,
        subject_text: &str,
        is_static: bool,
        ctx: &crate::types::FileContext,
        access_offset: u32,
        content: &str,
    ) -> Option<crate::php_type::PhpType> {
        let use_map = ctx.use_map_at(access_offset);
        let namespace = ctx.namespace_at(access_offset);
        let class_loader = self.class_loader_with(&ctx.classes, use_map, namespace);
        let function_loader =
            self.function_loader_with(ctx.resolved_names.as_deref(), use_map, namespace);
        let resolution_ctx = crate::type_engine::subject_resolution::SubjectResolutionCtx {
            local_classes: &ctx.classes,
            use_map,
            namespace,
            content,
            class_loader: &class_loader,
            backend: Some(self),
            function_loader: &function_loader,
        };
        crate::type_engine::subject_resolution::resolve_subject_type(
            subject_text,
            is_static,
            access_offset,
            &resolution_ctx,
        )
    }

    /// Normalize class names a resolved type carries into FQNs.
    ///
    /// A type may carry short names (`BlogAuthor` instead of
    /// `App\Models\BlogAuthor`), which are resolved through the file's
    /// use-map and namespace so they match the FQNs in a hierarchy set.
    pub(super) fn class_names_to_fqns(
        &self,
        names: Vec<String>,
        ctx: &crate::types::FileContext,
        access_offset: u32,
    ) -> Vec<String> {
        names
            .into_iter()
            .map(|n| {
                let normalized = normalize_fqn(&n);
                if normalized.contains('\\') {
                    normalized
                } else {
                    normalize_fqn(&Self::resolve_to_fqn(
                        &normalized,
                        ctx.use_map_at(access_offset),
                        ctx.namespace_at(access_offset),
                    ))
                }
            })
            .collect()
    }

    fn resolve_doctrine_repository_subject_to_fqns(
        &self,
        subject_text: &str,
        ctx: &crate::types::FileContext,
        access_offset: u32,
        content: &str,
    ) -> Vec<String> {
        let use_map = ctx.use_map_at(access_offset);
        let namespace = ctx.namespace_at(access_offset);
        let class_loader = self.class_loader_with(&ctx.classes, use_map, namespace);
        let expr = crate::type_engine::subject_expr::SubjectExpr::parse(subject_text);
        let mut candidates = self.doctrine_repository_fqns_from_expr(
            &expr,
            use_map,
            namespace,
            &ctx.classes,
            access_offset,
            &class_loader,
        );

        if candidates.is_empty()
            && let crate::type_engine::subject_expr::SubjectExpr::Variable(var_name) = &expr
            && let Some(assigned_expr) =
                last_assignment_expression_before(content, access_offset, var_name)
        {
            let assigned = crate::type_engine::subject_expr::SubjectExpr::parse(assigned_expr);
            candidates = self.doctrine_repository_fqns_from_expr(
                &assigned,
                use_map,
                namespace,
                &ctx.classes,
                access_offset,
                &class_loader,
            );
        }

        candidates
    }

    fn doctrine_repository_fqns_from_expr(
        &self,
        expr: &crate::type_engine::subject_expr::SubjectExpr,
        use_map: &HashMap<String, String>,
        namespace: &Option<String>,
        local_classes: &[Arc<ClassInfo>],
        access_offset: u32,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Vec<String> {
        let crate::type_engine::subject_expr::SubjectExpr::CallExpr { callee, args_text } = expr
        else {
            return Vec::new();
        };
        let crate::type_engine::subject_expr::SubjectExpr::MethodCall { method, .. } =
            callee.as_ref()
        else {
            return Vec::new();
        };
        if !method.eq_ignore_ascii_case("getRepository") {
            return Vec::new();
        }

        let Some(entity_fqn) = doctrine_repository_entity_arg(
            args_text,
            use_map,
            namespace,
            local_classes,
            access_offset,
        ) else {
            return Vec::new();
        };

        self.doctrine_repository_fqns_for_entity(&entity_fqn, class_loader)
    }

    pub(crate) fn doctrine_repository_fqns_for_entity(
        &self,
        entity_fqn: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Vec<String> {
        let entity = normalize_fqn(entity_fqn);
        let entity_short = crate::util::short_name(&entity);
        let repository_short = doctrine_repository_short_name(entity_short);
        let mut candidate_fqns = self.framework_doctrine_repository_fqns_for_entity(&entity);
        let mapped = candidate_fqns.len();
        candidate_fqns.extend(doctrine_repository_convention_candidates(
            &entity,
            &repository_short,
        ));
        let conventional = candidate_fqns.len();
        candidate_fqns.extend(
            self.doctrine_repository_classes()
                .into_iter()
                .filter(|fqn| crate::util::short_name(fqn).eq_ignore_ascii_case(&repository_short)),
        );

        for fallback in [
            "Doctrine\\Bundle\\DoctrineBundle\\Repository\\ServiceEntityRepository",
            "Doctrine\\ORM\\EntityRepository",
            "Doctrine\\Persistence\\ObjectRepository",
            "ServiceEntityRepository",
            "EntityRepository",
            "ObjectRepository",
        ] {
            candidate_fqns.push(fallback.to_string());
        }

        let mut resolved = Vec::new();
        for (index, candidate) in candidate_fqns.into_iter().enumerate() {
            let normalized = normalize_fqn(&candidate);
            if resolved
                .iter()
                .any(|known: &String| known.eq_ignore_ascii_case(&normalized))
            {
                continue;
            }
            let Some(class_info) = class_loader(&normalized) else {
                continue;
            };
            // A class the mapping names is the repository whatever it extends;
            // one found by its name has to actually be a Doctrine repository,
            // or a project's own `UserRepository` would pass for one.
            if (mapped..conventional).contains(&index)
                && !self.is_doctrine_repository_class(&class_info.fqn())
            {
                continue;
            }
            resolved.push(normalize_fqn(&class_info.fqn()));
        }
        resolved
    }

    /// Whether `class_fqn` extends or implements one of Doctrine's
    /// repository types, directly or through its ancestors.
    pub(crate) fn is_doctrine_repository_class(&self, class_fqn: &str) -> bool {
        let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };
        let mut ancestors = HashSet::new();
        self.collect_ancestors(class_fqn, &class_loader, &mut ancestors);
        ancestors
            .iter()
            .any(|ancestor| is_generic_doctrine_repository(ancestor))
    }

    /// Every class the inheritance index records as a Doctrine repository.
    fn doctrine_repository_classes(&self) -> Vec<String> {
        let gti = self.symbols.gti_index.read();
        let mut pending: Vec<&str> = DOCTRINE_REPOSITORY_BASES.to_vec();
        let mut seen: HashSet<String> = HashSet::new();
        let mut out = Vec::new();
        while let Some(parent) = pending.pop() {
            let Some(children) = gti.get(parent) else {
                continue;
            };
            for child in children {
                if seen.insert(child.to_ascii_lowercase()) {
                    out.push(child.clone());
                    pending.push(child.as_str());
                }
            }
        }
        out
    }

    fn resolve_static_laravel_builder_subject_to_fqns(
        &self,
        subject_text: &str,
        use_map: &HashMap<String, String>,
        namespace: &Option<String>,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Vec<String> {
        let expr = crate::type_engine::subject_expr::SubjectExpr::parse(subject_text);
        let Some((class_name, method_name)) = static_call_root(&expr) else {
            return Vec::new();
        };
        if !is_laravel_builder_static_entrypoint(method_name) {
            return Vec::new();
        }

        let class_fqn = normalize_fqn(&Self::resolve_to_fqn(class_name, use_map, namespace));
        let Some(class_info) = class_loader(&class_fqn) else {
            return Vec::new();
        };
        let Some(laravel) = class_info.laravel() else {
            return Vec::new();
        };

        let mut fqns = vec![class_fqn];
        if let Some(builder_fqn) = laravel
            .custom_builder
            .as_ref()
            .and_then(|builder| builder.base_name())
            .map(normalize_fqn)
        {
            fqns.push(builder_fqn.to_string());
        }
        fqns.sort();
        fqns.dedup();
        fqns
    }

    /// Collect the full class hierarchy (ancestors and descendants) for
    /// a set of starting FQNs.
    ///
    /// The result includes:
    /// - The starting FQNs themselves
    /// - All ancestor FQNs (parent chain, interfaces, traits)
    /// - All descendant FQNs (classes that extend/implement any class in
    ///   the hierarchy)
    pub(super) fn collect_hierarchy_for_fqns(&self, seed_fqns: &[String]) -> MemberScope {
        let mut hierarchy = HashSet::new();
        let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };

        for fqn in seed_fqns {
            hierarchy.insert(normalize_fqn(fqn).to_string());
        }

        // Walk up: collect all ancestors for each seed.
        let seeds: Vec<String> = hierarchy.iter().cloned().collect();
        for fqn in seeds {
            self.collect_ancestors(&fqn, &class_loader, &mut hierarchy);
        }

        // Bridge Laravel Models and their Custom Builders.
        // If a class in the hierarchy is a Model with a custom builder,
        // add that builder to the hierarchy.
        let mut extensions = Vec::new();
        for fqn in &hierarchy {
            if let Some(cls) = class_loader(fqn)
                && let Some(builder_fqn) = cls
                    .laravel()
                    .and_then(|l| l.custom_builder.as_ref())
                    .and_then(|b| b.base_name())
            {
                extensions.push(normalize_fqn(builder_fqn).to_string());
            }
        }
        for ext_fqn in &extensions {
            if hierarchy.insert(ext_fqn.clone()) {
                self.collect_ancestors(ext_fqn, &class_loader, &mut hierarchy);
            }
        }

        // Bridge Laravel Builders back to their Models.
        // Only builder roots that are actually part of the original lookup
        // should contribute models. A custom builder's ancestors include the
        // base Eloquent builder, but that must not fan out into every model.
        let builder_roots: HashSet<String> = seed_fqns
            .iter()
            .map(|fqn| normalize_fqn(fqn).to_string())
            .chain(extensions.iter().cloned())
            .collect();
        let model_seeds: Vec<String> = self
            .models_built_by(&builder_roots)
            .into_iter()
            .map(|(model, _)| model)
            .collect();
        for model_fqn in &model_seeds {
            if hierarchy.insert(model_fqn.clone()) {
                self.collect_ancestors(model_fqn, &class_loader, &mut hierarchy);
            }
        }

        // Walk down: collect descendants from the original target classes,
        // not every ancestor. This keeps a concrete class rename from
        // fanning out through an implemented interface into sibling classes.
        let descendant_roots: HashSet<String> = seed_fqns
            .iter()
            .map(|fqn| normalize_fqn(fqn))
            .chain(extensions)
            .chain(model_seeds)
            .collect();
        hierarchy.extend(self.descendants_closure(descendant_roots.iter().cloned()));

        MemberScope::descendants_of(descendant_roots, hierarchy)
    }

    /// The scope of the classes that inherit the member from `roots`.
    pub(super) fn descendant_scope(&self, roots: HashSet<String>) -> MemberScope {
        let indexed = self.descendants_closure(roots.iter().cloned());
        MemberScope::descendants_of(roots, indexed)
    }

    /// Whether `fqn` inherits from any of `roots`, walking up from the class
    /// itself.
    ///
    /// The walk loads the ancestors it needs, so it answers for a class the
    /// reverse-inheritance index has no edge for — which is every class in a
    /// package nothing has parsed yet.
    fn inherits_from_any(&self, fqn: &str, roots: &HashSet<String>) -> bool {
        let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };
        let mut ancestors = HashSet::new();
        self.collect_ancestors(fqn, &class_loader, &mut ancestors);
        ancestors.iter().any(|ancestor| roots.contains(ancestor))
    }

    /// The models whose query builder is one of `builders`, each with the
    /// builder it uses.
    ///
    /// A model with no custom builder uses the base Eloquent builder.
    fn models_built_by(&self, builders: &HashSet<String>) -> Vec<(String, String)> {
        let base_builder = crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN;
        let class_index = self.symbols.fqn_class_index.read();
        class_index
            .iter()
            .filter_map(|(class_fqn, class_info)| {
                let laravel = class_info.laravel()?;
                let builder = match laravel.custom_builder.as_ref() {
                    Some(builder) => normalize_fqn(builder.base_name()?),
                    None => base_builder.to_string(),
                };
                builders
                    .contains(&builder)
                    .then(|| (normalize_fqn(class_fqn), builder))
            })
            .collect()
    }

    /// `seeds` and every class that extends or implements one of them,
    /// transitively.
    fn descendants_closure(&self, seeds: impl IntoIterator<Item = String>) -> HashSet<String> {
        let mut closure: HashSet<String> = HashSet::new();
        let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        for seed in seeds {
            if closure.insert(seed.clone()) {
                queue.push_back(seed);
            }
        }
        let gti = self.symbols.gti_index.read();
        while let Some(fqn) = queue.pop_front() {
            if let Some(descendants) = gti.get(&fqn) {
                for desc in descendants {
                    let normalized = normalize_fqn(desc);
                    if closure.insert(normalized.clone()) {
                        queue.push_back(normalized);
                    }
                }
            }
        }
        closure
    }

    pub(super) fn collect_member_receiver_scope(
        &self,
        seed_fqns: &[String],
        member_name: &str,
        is_static: bool,
        include_declaring_interfaces: bool,
    ) -> Option<MemberScope> {
        let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };
        let mut roots = HashSet::new();
        let mut seen = HashSet::new();

        for fqn in seed_fqns {
            let normalized = normalize_fqn(fqn).to_string();
            if self.defines_member(&normalized, member_name, is_static, &class_loader) {
                roots.insert(normalized.clone());
                if include_declaring_interfaces {
                    self.collect_declaring_member_interfaces(
                        &normalized,
                        member_name,
                        is_static,
                        &class_loader,
                        &mut roots,
                        &mut seen,
                    );
                }
            } else {
                self.collect_declaring_member_ancestors(
                    &normalized,
                    member_name,
                    is_static,
                    &class_loader,
                    &mut roots,
                    &mut seen,
                );
            }
        }

        if roots.is_empty() {
            return None;
        }

        self.extend_laravel_member_roots(&mut roots);
        Some(self.descendant_scope(roots))
    }

    fn collect_declaring_member_interfaces(
        &self,
        fqn: &str,
        member_name: &str,
        is_static: bool,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        roots: &mut HashSet<String>,
        seen: &mut HashSet<String>,
    ) {
        let normalized = normalize_fqn(fqn).to_string();
        if !seen.insert(normalized.clone()) {
            return;
        }
        let Some(cls) = class_loader(&normalized) else {
            return;
        };

        for iface in &cls.interfaces {
            let iface_fqn = normalize_fqn(iface).to_string();
            if self.defines_member(&iface_fqn, member_name, is_static, class_loader) {
                roots.insert(iface_fqn.clone());
            }
            self.collect_declaring_member_interfaces(
                &iface_fqn,
                member_name,
                is_static,
                class_loader,
                roots,
                seen,
            );
        }
    }

    fn extend_laravel_member_roots(&self, roots: &mut HashSet<String>) {
        if !self.resolved_class_cache.read().is_laravel() {
            return;
        }
        let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };
        let initial_roots: Vec<String> = roots.iter().cloned().collect();
        let mut candidate_roots: HashSet<String> = initial_roots.iter().cloned().collect();
        let mut builder_roots: HashSet<String> = HashSet::new();
        if candidate_roots.contains(crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN) {
            builder_roots.insert(crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN.to_string());
        }

        for fqn in &initial_roots {
            if let Some(cls) = class_loader(fqn)
                && let Some(builder_fqn) = cls
                    .laravel()
                    .and_then(|l| l.custom_builder.as_ref())
                    .and_then(|b| b.base_name())
                    .map(normalize_fqn)
            {
                let builder = builder_fqn.to_string();
                roots.insert(builder.clone());
                candidate_roots.insert(builder.clone());
                builder_roots.insert(builder);
            }
        }

        // Only a query builder has models built by it, and finding them
        // means a pass over every class the project knows, so the pass is
        // skipped unless one of the roots is the Eloquent builder or one of
        // its subclasses.
        let eloquent_builder: HashSet<String> =
            HashSet::from([crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN.to_string()]);
        let any_root_is_builder = candidate_roots.iter().any(|root| {
            eloquent_builder.contains(root) || self.inherits_from_any(root, &eloquent_builder)
        });
        if any_root_is_builder {
            for (model, builder) in self.models_built_by(&candidate_roots) {
                roots.insert(model);
                builder_roots.insert(builder);
            }
        }

        for builder in builder_roots {
            self.collect_ancestors(&builder, &class_loader, roots);
        }
    }

    fn collect_declaring_member_ancestors(
        &self,
        fqn: &str,
        member_name: &str,
        is_static: bool,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        roots: &mut HashSet<String>,
        seen: &mut HashSet<String>,
    ) {
        let normalized = normalize_fqn(fqn).to_string();
        if !seen.insert(normalized.clone()) {
            return;
        }
        let Some(cls) = class_loader(&normalized) else {
            return;
        };

        let ancestors = cls
            .parent_class
            .iter()
            .chain(cls.interfaces.iter())
            .chain(cls.used_traits.iter())
            .chain(cls.mixins.iter())
            .map(|name| normalize_fqn(name).to_string())
            .collect::<Vec<_>>();

        for ancestor in ancestors {
            if self.defines_member(&ancestor, member_name, is_static, class_loader) {
                roots.insert(ancestor);
            } else {
                self.collect_declaring_member_ancestors(
                    &ancestor,
                    member_name,
                    is_static,
                    class_loader,
                    roots,
                    seen,
                );
            }
        }
    }

    fn collect_macro_declaring_targets(
        &self,
        seed_fqns: &[String],
        member_name: &str,
    ) -> Option<Vec<String>> {
        let index = self.laravel_macros.read();
        let mut targets = Vec::new();
        for seed in seed_fqns {
            let mut ancestors = HashSet::new();
            let normalized = normalize_fqn(seed).to_string();
            ancestors.insert(normalized.clone());
            let class_loader =
                |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };
            self.collect_ancestors(&normalized, &class_loader, &mut ancestors);
            for candidate in ancestors {
                if index.has_macro(&candidate, member_name) && !targets.contains(&candidate) {
                    targets.push(candidate);
                }
            }
        }
        (!targets.is_empty()).then_some(targets)
    }

    fn defines_member(
        &self,
        fqn: &str,
        name: &str,
        is_static: bool,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool {
        let Some(cls) = class_loader(fqn) else {
            return false;
        };

        if cls
            .methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(name) && m.is_static == is_static)
        {
            return true;
        }

        let property_name = name.strip_prefix('$').unwrap_or(name);
        if cls.properties.iter().any(|p| {
            p.name.as_str().strip_prefix('$').unwrap_or(p.name.as_str()) == property_name
                && p.is_static == is_static
        }) {
            return true;
        }

        if let Some(laravel) = cls.laravel() {
            if let Some(builder_cls) = laravel
                .custom_builder
                .as_ref()
                .and_then(|b| b.base_name())
                .and_then(class_loader)
                && builder_cls
                    .methods
                    .iter()
                    .any(|m| m.name.eq_ignore_ascii_case(name) && (!is_static || !m.is_static))
            {
                return true;
            }
            if class_loader(crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN)
                .filter(|bc| {
                    bc.methods
                        .iter()
                        .any(|m| m.name.eq_ignore_ascii_case(name) && (!is_static || !m.is_static))
                })
                .is_some()
            {
                return true;
            }
        }

        false
    }

    /// Walk up the inheritance chain and collect all ancestor FQNs.
    fn collect_ancestors(
        &self,
        fqn: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        hierarchy: &mut HashSet<String>,
    ) {
        let cls = match class_loader(fqn) {
            Some(c) => c,
            None => return,
        };

        if let Some(ref parent) = cls.parent_class {
            let parent_fqn = normalize_fqn(parent);
            if hierarchy.insert(parent_fqn.clone()) {
                self.collect_ancestors(&parent_fqn, class_loader, hierarchy);
            }
        }

        for iface in &cls.interfaces {
            let iface_fqn = normalize_fqn(iface);
            if hierarchy.insert(iface_fqn.clone()) {
                self.collect_ancestors(&iface_fqn, class_loader, hierarchy);
            }
        }

        for trait_name in &cls.used_traits {
            let trait_fqn = normalize_fqn(trait_name);
            if hierarchy.insert(trait_fqn.clone()) {
                self.collect_ancestors(&trait_fqn, class_loader, hierarchy);
            }
        }

        for mixin in &cls.mixins {
            let mixin_fqn = normalize_fqn(mixin);
            if hierarchy.insert(mixin_fqn.clone()) {
                self.collect_ancestors(&mixin_fqn, class_loader, hierarchy);
            }
        }
    }
}

fn doctrine_repository_entity_arg(
    args_text: &str,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
    local_classes: &[Arc<ClassInfo>],
    access_offset: u32,
) -> Option<String> {
    let first_arg = crate::type_engine::conditional_resolution::split_text_args(args_text)
        .into_iter()
        .next()?
        .trim();
    let class_expr = first_arg.strip_suffix("::class")?.trim();
    let class_expr = class_expr.trim_start_matches('\\');
    if class_expr.is_empty() {
        return None;
    }

    match class_expr {
        "self" | "static" => {
            let current = find_class_at_offset(local_classes, access_offset)?;
            Some(current.fqn().to_string())
        }
        "parent" => {
            let current = find_class_at_offset(local_classes, access_offset)?;
            current.parent_class.map(|parent| parent.to_string())
        }
        _ => Some(Backend::resolve_to_fqn(class_expr, use_map, namespace)),
    }
}

fn doctrine_repository_short_name(entity_short: &str) -> String {
    let stem = entity_short
        .strip_suffix("Entity")
        .or_else(|| entity_short.strip_suffix("Impl"))
        .unwrap_or(entity_short);
    format!("{stem}Repository")
}

pub(crate) fn doctrine_repository_matches_entity_convention(
    entity_fqn: &str,
    repository_fqn: &str,
) -> bool {
    let entity = normalize_fqn(entity_fqn);
    let repository = normalize_fqn(repository_fqn);
    let repository_short = doctrine_repository_short_name(crate::util::short_name(&entity));
    crate::util::short_name(&repository).eq_ignore_ascii_case(&repository_short)
        || doctrine_repository_convention_candidates(&entity, &repository_short)
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&repository))
}

fn doctrine_repository_convention_candidates(
    entity_fqn: &str,
    repository_short: &str,
) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some((entity_ns, _)) = entity_fqn.rsplit_once('\\') {
        candidates.push(format!("{entity_ns}\\{repository_short}"));

        for marker in ["\\Entity\\", "\\Entities\\", "\\Model\\", "\\Models\\"] {
            if let Some((root, _tail)) = entity_fqn.rsplit_once(marker) {
                candidates.push(format!("{root}\\Repository\\{repository_short}"));
                candidates.push(format!("{root}\\Repositories\\{repository_short}"));
            }
        }

        for suffix in ["\\Entity", "\\Entities", "\\Model", "\\Models"] {
            if let Some(root) = entity_ns.strip_suffix(suffix) {
                candidates.push(format!("{root}\\Repository\\{repository_short}"));
                candidates.push(format!("{root}\\Repositories\\{repository_short}"));
            }
        }
    } else {
        candidates.push(repository_short.to_string());
    }

    candidates
}

/// Doctrine's own repository types, spelled the way a class may name them.
const DOCTRINE_REPOSITORY_BASES: [&str; 6] = [
    "Doctrine\\Bundle\\DoctrineBundle\\Repository\\ServiceEntityRepository",
    "Doctrine\\ORM\\EntityRepository",
    "Doctrine\\Persistence\\ObjectRepository",
    "ServiceEntityRepository",
    "EntityRepository",
    "ObjectRepository",
];

/// Whether `fqn` is one of Doctrine's generic repository types rather than a
/// project's concrete repository.
fn is_generic_doctrine_repository(fqn: &str) -> bool {
    let fqn = fqn.trim_start_matches('\\');
    DOCTRINE_REPOSITORY_BASES
        .iter()
        .any(|base| base.eq_ignore_ascii_case(fqn))
}

/// The expression last assigned to `var_name` before `access_offset`, within
/// the function the access sits in.
///
/// Only a plain `$var = …` counts: `$var == …`, `$var => …`, and a longer
/// name such as `$variable = …` are skipped.  Assignments in another
/// function body are out of reach of the access, so the search stops at the
/// nearest `function` keyword before it.
fn last_assignment_expression_before<'a>(
    content: &'a str,
    access_offset: u32,
    var_name: &str,
) -> Option<&'a str> {
    let prefix = content.get(..access_offset as usize)?;
    let scope = &prefix[enclosing_function_start(prefix)..];
    let mut search_end = scope.len();
    while let Some(at) = scope[..search_end].rfind(var_name) {
        search_end = at;
        let after = &scope[at + var_name.len()..];
        if after.starts_with(|c: char| c == '_' || c.is_ascii_alphanumeric()) {
            continue;
        }
        let Some(value) = after.trim_start().strip_prefix('=') else {
            continue;
        };
        if value.starts_with(['=', '>']) {
            continue;
        }
        let value = value.trim_start();
        let end = value
            .find(';')
            .or_else(|| value.find('\n'))
            .unwrap_or(value.len());
        let expr = value[..end].trim();
        return (!expr.is_empty()).then_some(expr);
    }
    None
}

/// Byte offset just past the last `function` keyword in `prefix`, or `0`.
fn enclosing_function_start(prefix: &str) -> usize {
    let bytes = prefix.as_bytes();
    let mut end = prefix.len();
    while let Some(at) = prefix[..end].rfind("function") {
        let before_ok = at == 0 || !is_identifier_byte(bytes[at - 1]);
        let after = at + "function".len();
        let after_ok = bytes
            .get(after)
            .is_none_or(|byte| !is_identifier_byte(*byte));
        if before_ok && after_ok {
            return after;
        }
        end = at;
    }
    0
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphanumeric()
}
