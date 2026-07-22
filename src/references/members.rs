//! Member (method / property / constant) reference finding, plus the
//! class-hierarchy resolution helpers that scope member searches.
//!
//! Member references are filtered by the class hierarchy of the target
//! member so that an access on an unrelated class that merely shares a
//! member name is excluded.  This module also handles Laravel macros
//! (invoked both statically and on instances) and the Model/Builder
//! bridging that Eloquent's magic requires.

use super::*;

use std::collections::HashMap;

use tower_lsp::lsp_types::{Location, Range};

use crate::symbol_map::SymbolKind;
use crate::types::ClassInfo;
use crate::util::{find_class_at_offset, offset_to_position, push_unique_location};

impl Backend {
    pub(super) fn find_laravel_macro_references(
        &self,
        uri: &str,
        span_start: u32,
        name: &str,
        include_declaration: bool,
    ) -> Vec<Location> {
        let targets = {
            let index = self.laravel_macros.read();
            index.targets_at(uri, span_start, name)
        };
        if targets.is_empty() {
            return Vec::new();
        }

        let hierarchy = self.collect_hierarchy_for_fqns(&targets);

        // Macros are invoked both statically (`Widget::shine()`) and on
        // instances (`$widget->shine()`), so prune candidate files with
        // both member-key variants.
        let snapshot = self.user_file_symbol_maps_for_reference_keys(&[
            ReferenceIndexKey::Member {
                name: name.to_string(),
                is_static: false,
            },
            ReferenceIndexKey::Member {
                name: name.to_string(),
                is_static: true,
            },
        ]);
        self.begin_request_scan_window(snapshot.len(), "Scanning for macro references");
        let mut locations = Vec::new();
        for (file_uri, symbol_map) in &snapshot {
            self.request_scan_file_done();
            if symbol_map.member_access_indices(name).is_empty() {
                continue;
            }

            let Ok(parsed_uri) = Url::parse(file_uri) else {
                continue;
            };
            let Some(content) = self.get_file_content_arc(file_uri) else {
                continue;
            };
            let file_ctx = self.file_context(file_uri);

            for &span_idx in symbol_map.member_access_indices(name) {
                let span = &symbol_map.spans[span_idx];
                let SymbolKind::MemberAccess {
                    member_name,
                    subject_text,
                    is_static,
                    ..
                } = &span.kind
                else {
                    continue;
                };
                if member_name != name {
                    continue;
                }

                let matches_macro = if subject_text.contains('(') {
                    // Chained call receivers like `$query->pluck(...)->macroName()`
                    // are expensive to resolve precisely here and are the main
                    // real-world macro-registration rename case.
                    true
                } else {
                    let subject_fqns = self.resolve_subject_to_fqns(
                        subject_text,
                        *is_static,
                        &file_ctx,
                        span.start,
                        &content,
                    );
                    !subject_fqns.is_empty()
                        && subject_fqns.iter().any(|fqn| hierarchy.contains(fqn))
                };
                if !matches_macro {
                    continue;
                }

                let start = offset_to_position(&content, span.start as usize);
                let end = offset_to_position(&content, span.end as usize);
                push_unique_location(&mut locations, &parsed_uri, start, end);
            }
        }

        if include_declaration {
            let macro_scope: HashSet<String> = targets.iter().cloned().collect();
            self.append_laravel_macro_registration_locations(
                &mut locations,
                name,
                Some(&macro_scope),
            );
        }

        locations
    }

    pub(super) fn append_laravel_macro_registration_locations(
        &self,
        locations: &mut Vec<Location>,
        name: &str,
        targets: Option<&HashSet<String>>,
    ) {
        let Some(targets) = targets else {
            return;
        };
        let index = self.laravel_macros.read();
        for target in targets {
            if !index.has_macro(target, name) {
                continue;
            }
            let Some((uri, offset)) = index.definition(target, name) else {
                continue;
            };
            let Some(content) = self.get_file_content_arc(uri) else {
                continue;
            };
            let Ok(parsed_uri) = Url::parse(uri) else {
                continue;
            };
            let start = offset_to_position(&content, offset as usize + 1);
            let end = offset_to_position(&content, offset as usize + 1 + name.len());
            push_unique_location(locations, &parsed_uri, start, end);
        }
    }

    pub(super) fn append_unique_laravel_macro_registration_location(
        &self,
        locations: &mut Vec<Location>,
        name: &str,
    ) {
        let index = self.laravel_macros.read();
        let Some((uri, offset)) = index.unique_definition_for_name(name) else {
            return;
        };
        let Some(content) = self.get_file_content_arc(uri) else {
            return;
        };
        let Ok(parsed_uri) = Url::parse(uri) else {
            return;
        };
        let start = offset_to_position(&content, offset as usize + 1);
        let end = offset_to_position(&content, offset as usize + 1 + name.len());
        push_unique_location(locations, &parsed_uri, start, end);
    }

    /// Find all references to a member (method, property, or constant)
    /// across all files.
    ///
    /// When `hierarchy` is `Some`, only references where the subject
    /// resolves to a class in the given set of FQNs are returned.  When
    /// the subject cannot be resolved (e.g. a complex expression or an
    /// untyped variable), the reference is skipped; accepting every
    /// unresolved `$x->method()` makes common names such as `find` unusably
    /// noisy in large projects.
    ///
    /// When `hierarchy` is `None`, all references with a matching member
    /// name and static-ness are returned (the v1 behaviour, kept as a
    /// fallback when the target class cannot be determined).
    pub(super) fn find_member_references(
        &self,
        target_member: &str,
        target_is_static: bool,
        include_declaration: bool,
        hierarchy: Option<&HashSet<String>>,
        declaration_scope: Option<&HashSet<String>>,
    ) -> Vec<Location> {
        let mut locations = Vec::new();

        let candidate_keys = member_candidate_keys(target_member, target_is_static, hierarchy);
        let snapshot = self.user_file_symbol_maps_for_reference_keys(&candidate_keys);
        self.begin_request_scan_window(snapshot.len(), "Scanning for member references");

        for (file_uri, symbol_map) in &snapshot {
            self.request_scan_file_done();
            // First pass: name-only check to avoid unnecessary work.
            // When a hierarchy is present (e.g. Laravel), we allow static mismatch.
            let has_member_access_match = symbol_map
                .member_access_indices(target_member)
                .iter()
                .any(|&idx| match &symbol_map.spans[idx].kind {
                    SymbolKind::MemberAccess { is_static, .. } => {
                        hierarchy.is_some() || *is_static == target_is_static
                    }
                    _ => false,
                });
            let has_declaration_match = include_declaration
                && symbol_map.spans.iter().any(|span| match &span.kind {
                    SymbolKind::MemberDeclaration { name, is_static } if name == target_member => {
                        hierarchy.is_some() || *is_static == target_is_static
                    }
                    _ => false,
                });
            let has_potential_match = has_member_access_match || has_declaration_match;

            // Special check for property declarations in ClassInfo (represented as Variable spans)
            let mut check_ast_map = false;
            if !has_potential_match
                && include_declaration
                && let Some(classes) = self.get_classes_for_uri(file_uri)
            {
                for class in &classes {
                    for prop in &class.properties {
                        let prop_name = prop.name.strip_prefix('$').unwrap_or(&prop.name);
                        let target_name = target_member.strip_prefix('$').unwrap_or(target_member);
                        if prop_name == target_name && prop.is_static == target_is_static {
                            check_ast_map = true;
                            break;
                        }
                    }
                    if check_ast_map {
                        break;
                    }
                }
            }

            if !has_potential_match && !check_ast_map {
                continue;
            }

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let mut file_content: Option<Arc<String>> = None;

            // Lazily resolved file context — only computed when we need
            // to check a candidate's subject against the hierarchy.
            let file_ctx_cell: std::cell::OnceCell<crate::types::FileContext> =
                std::cell::OnceCell::new();

            for &span_idx in symbol_map.member_access_indices(target_member) {
                let span = &symbol_map.spans[span_idx];
                match &span.kind {
                    SymbolKind::MemberAccess {
                        subject_text,
                        member_name,
                        is_static,
                        ..
                    } if member_name == target_member => {
                        // For Laravel custom builders, we allow static-ness mismatch
                        // (Model::active() is static, UserBuilder->active() is instance).
                        if *is_static != target_is_static {
                            // Only allow mismatch if we have a hierarchy to verify
                            // that they are indeed related (one is Model, one is Builder).
                            if hierarchy.is_none() {
                                continue;
                            }
                        }

                        // Check if the subject belongs to the target hierarchy.
                        if let Some(hier) = hierarchy {
                            if file_content.is_none() {
                                file_content = self.reference_file_content_arc(file_uri);
                            }
                            let Some(ref content) = file_content else {
                                break;
                            };

                            let ctx = file_ctx_cell.get_or_init(|| self.file_context(file_uri));
                            let subject_fqns = self.resolve_subject_to_fqns(
                                subject_text,
                                *is_static,
                                ctx,
                                span.start,
                                content,
                            );
                            if subject_fqns.is_empty() {
                                if !unresolved_member_subject_matches_scope(subject_text, hier) {
                                    continue;
                                }
                            } else if !subject_fqns.iter().any(|fqn| hier.contains(fqn)) {
                                // Subject resolved but none of the resolved
                                // classes are in the target hierarchy — skip.
                                continue;
                            }
                        }

                        if file_content.is_none() {
                            file_content = self.reference_file_content_arc(file_uri);
                        }
                        let Some(ref content) = file_content else {
                            break;
                        };

                        let start = offset_to_position(content, span.start as usize);
                        let end = offset_to_position(content, span.end as usize);
                        locations.push(Location {
                            uri: parsed_uri.clone(),
                            range: Range { start, end },
                        });
                    }
                    _ => {}
                }
            }

            if include_declaration {
                for span in &symbol_map.spans {
                    match &span.kind {
                        SymbolKind::MemberDeclaration { name, is_static }
                            if name == target_member =>
                        {
                            if *is_static != target_is_static && hierarchy.is_none() {
                                continue;
                            }

                            let declaration_filter = if *is_static == target_is_static {
                                declaration_scope.or(hierarchy)
                            } else {
                                hierarchy
                            };
                            if let Some(hier) = declaration_filter {
                                let ctx = file_ctx_cell.get_or_init(|| self.file_context(file_uri));
                                let enclosing = find_class_at_offset(&ctx.classes, span.start)
                                    .or_else(|| {
                                        ctx.classes
                                            .iter()
                                            .map(|c| c.as_ref())
                                            .filter(|c| {
                                                c.keyword_offset > 0 && span.start < c.start_offset
                                            })
                                            .min_by_key(|c| c.start_offset)
                                    });
                                if let Some(enclosing) = enclosing {
                                    let fqn = enclosing.fqn().to_string();
                                    if !hier.contains(&fqn) {
                                        continue;
                                    }
                                }
                            }

                            if file_content.is_none() {
                                file_content = self.reference_file_content_arc(file_uri);
                            }
                            let Some(ref content) = file_content else {
                                break;
                            };

                            let start = offset_to_position(content, span.start as usize);
                            let end = offset_to_position(content, span.end as usize);
                            locations.push(Location {
                                uri: parsed_uri.clone(),
                                range: Range { start, end },
                            });
                        }
                        _ => {}
                    }
                }
            }

            // Property declarations use Variable spans (not
            // MemberDeclaration) because GTD relies on the Variable
            // kind to jump to the type hint.  Scan the uri_classes_index to
            // pick up property declaration sites.
            if include_declaration && let Some(classes) = self.get_classes_for_uri(file_uri) {
                for class in &classes {
                    // Filter by hierarchy when available.
                    if let Some(hier) = declaration_scope.or(hierarchy) {
                        let class_fqn = class.fqn().to_string();
                        if !hier.contains(&class_fqn) {
                            continue;
                        }
                    }

                    for prop in &class.properties {
                        let prop_name = prop.name.strip_prefix('$').unwrap_or(&prop.name);
                        let target_name = target_member.strip_prefix('$').unwrap_or(target_member);
                        if prop_name == target_name
                            && prop.is_static == target_is_static
                            && prop.name_offset != 0
                        {
                            if file_content.is_none() {
                                file_content = self.reference_file_content_arc(file_uri);
                            }
                            let Some(ref content) = file_content else {
                                break;
                            };

                            // `name_offset` points at the `$` sigil while
                            // `prop.name` excludes it, so the range must span
                            // the `$` plus the name (`$name`, not `$nam`).
                            let offset = prop.name_offset;
                            let start = offset_to_position(content, offset as usize);
                            let end =
                                offset_to_position(content, offset as usize + 1 + prop.name.len());
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

        locations
    }

    // ─── Class hierarchy resolution for member references ───────────────────

    /// Resolve the class hierarchy for a `MemberAccess` subject.
    ///
    /// Returns `Some(set_of_fqns)` when the subject can be resolved to at
    /// least one class, or `None` when resolution fails entirely.
    pub(super) fn resolve_member_access_scopes(
        &self,
        uri: &str,
        subject_text: &str,
        is_static: bool,
        span_start: u32,
        member_name: &str,
        mode: ReferenceSearchMode,
    ) -> (Option<HashSet<String>>, Option<HashSet<String>>) {
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
                Some(self.collect_macro_declaring_scope(&macro_targets)),
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

    /// Resolve the class hierarchy for a `MemberDeclaration` at a given offset.
    ///
    /// Finds the enclosing class and builds the hierarchy set from it.
    pub(super) fn resolve_member_declaration_hierarchy(
        &self,
        uri: &str,
        offset: u32,
        member_name: &str,
        is_static: bool,
        mode: ReferenceSearchMode,
    ) -> Option<HashSet<String>> {
        let classes: Vec<Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let current_class = find_class_at_offset(&classes, offset).or_else(|| {
            // Fallback: offset may be in a class docblock (before the opening
            // brace).  Find the nearest class whose body starts past the
            // offset, meaning its docblock region likely contains the offset.
            classes
                .iter()
                .map(|c| c.as_ref())
                .filter(|c| c.keyword_offset > 0 && offset < c.start_offset)
                .min_by_key(|c| c.start_offset)
        })?;
        let fqn = current_class.fqn().to_string();
        Some(
            self.collect_member_receiver_scope(
                std::slice::from_ref(&fqn),
                member_name,
                is_static,
                mode.include_declaring_interfaces(),
            )
            .unwrap_or_else(|| self.collect_hierarchy_for_fqns(&[fqn])),
        )
    }

    pub(super) fn resolve_member_declaration_scope(
        &self,
        uri: &str,
        offset: u32,
        member_name: &str,
        is_static: bool,
        mode: ReferenceSearchMode,
    ) -> Option<HashSet<String>> {
        let classes: Vec<Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let current_class = find_class_at_offset(&classes, offset).or_else(|| {
            classes
                .iter()
                .map(|c| c.as_ref())
                .filter(|c| c.keyword_offset > 0 && offset < c.start_offset)
                .min_by_key(|c| c.start_offset)
        })?;
        self.collect_member_receiver_scope(
            &[current_class.fqn().to_string()],
            member_name,
            is_static,
            mode.include_declaring_interfaces(),
        )
    }

    /// Resolve a member access subject to zero or more class FQNs.
    ///
    /// This is a lightweight resolution path used during reference scanning.
    /// It handles the common cases (`self`, `static`, `$this`, `parent`,
    /// Resolve a member-access subject to the FQN(s) of its type(s)
    /// using the shared subject resolution utility.
    pub(super) fn resolve_subject_to_fqns(
        &self,
        subject_text: &str,
        is_static: bool,
        ctx: &crate::types::FileContext,
        access_offset: u32,
        content: &str,
    ) -> Vec<String> {
        let class_loader = self.class_loader(ctx);
        let function_loader = self.function_loader(ctx);
        let use_map = &ctx.use_map;
        let namespace = &ctx.namespace;
        let resolution_ctx = crate::subject_resolution::SubjectResolutionCtx {
            local_classes: &ctx.classes,
            use_map,
            namespace,
            content,
            class_loader: &class_loader,
            function_loader: &function_loader,
        };

        match crate::subject_resolution::resolve_subject_type(
            subject_text,
            is_static,
            access_offset,
            &resolution_ctx,
        ) {
            Some(php_type) => php_type
                .top_level_class_names()
                .into_iter()
                .map(|n| {
                    let normalized = normalize_fqn(&n);
                    // top_level_class_names() may return short names
                    // (e.g. "BlogAuthor" instead of
                    // "App\Models\BlogAuthor").  Resolve them through
                    // the file's use-map and namespace so they match
                    // the FQNs used in the hierarchy set.
                    if normalized.contains('\\') {
                        normalized.to_string()
                    } else {
                        normalize_fqn(&Self::resolve_to_fqn(&normalized, use_map, namespace))
                            .to_string()
                    }
                })
                .collect(),
            None => self.resolve_static_laravel_builder_subject_to_fqns(
                subject_text,
                use_map,
                namespace,
                &class_loader,
            ),
        }
    }

    fn resolve_static_laravel_builder_subject_to_fqns(
        &self,
        subject_text: &str,
        use_map: &HashMap<String, String>,
        namespace: &Option<String>,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Vec<String> {
        let expr = crate::subject_expr::SubjectExpr::parse(subject_text);
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
    fn collect_hierarchy_for_fqns(&self, seed_fqns: &[String]) -> HashSet<String> {
        let mut hierarchy = HashSet::new();
        let class_loader = |name: &str| -> Option<Arc<ClassInfo>> { self.find_or_load_class(name) };

        // Insert the seeds.
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
        let mut model_seeds = Vec::new();
        {
            let class_index = self.fqn_class_index.read();
            for (class_fqn, class_info) in class_index.iter() {
                if let Some(laravel) = class_info.laravel() {
                    if let Some(normalized) = laravel
                        .custom_builder
                        .as_ref()
                        .and_then(|b| b.base_name())
                        .map(normalize_fqn)
                    {
                        if builder_roots.contains(normalized.as_str()) {
                            model_seeds.push(class_fqn.to_owned());
                        }
                    } else if builder_roots
                        .contains(crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN)
                    {
                        // All models use the base Eloquent Builder by default.
                        model_seeds.push(class_fqn.to_owned());
                    }
                }
            }
        }
        for model_fqn in &model_seeds {
            if hierarchy.insert(normalize_fqn(model_fqn).to_string()) {
                self.collect_ancestors(model_fqn, &class_loader, &mut hierarchy);
            }
        }

        // Walk down: collect descendants from the original target classes,
        // not every ancestor. This keeps a concrete class rename from
        // fanning out through an implemented interface into sibling classes.
        let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        for fqn in seed_fqns {
            queue.push_back(normalize_fqn(fqn).to_string());
        }
        for ext_fqn in &extensions {
            queue.push_back(ext_fqn.clone());
        }
        for model_fqn in &model_seeds {
            queue.push_back(normalize_fqn(model_fqn).to_string());
        }

        let gti = self.gti_index.read();
        while let Some(fqn) = queue.pop_front() {
            if let Some(descendants) = gti.get(&fqn) {
                for desc in descendants {
                    let normalized = normalize_fqn(desc).to_string();
                    if hierarchy.insert(normalized.clone()) {
                        queue.push_back(normalized);
                    }
                }
            }
        }

        hierarchy
    }

    fn collect_member_receiver_scope(
        &self,
        seed_fqns: &[String],
        member_name: &str,
        is_static: bool,
        include_declaring_interfaces: bool,
    ) -> Option<HashSet<String>> {
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
        Some(self.collect_descendants_for_roots(roots))
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

        let mut model_roots = Vec::new();
        {
            let class_index = self.fqn_class_index.read();
            for (class_fqn, class_info) in class_index.iter() {
                if let Some(laravel) = class_info.laravel() {
                    if let Some(builder_fqn) = laravel
                        .custom_builder
                        .as_ref()
                        .and_then(|b| b.base_name())
                        .map(normalize_fqn)
                    {
                        if candidate_roots.contains(&builder_fqn) {
                            model_roots.push(normalize_fqn(class_fqn).to_string());
                            builder_roots.insert(builder_fqn);
                        }
                    } else if candidate_roots
                        .contains(crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN)
                    {
                        model_roots.push(normalize_fqn(class_fqn).to_string());
                        builder_roots.insert(
                            crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN.to_string(),
                        );
                    }
                }
            }
        }

        roots.extend(model_roots);
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

    fn collect_descendants_for_roots(&self, roots: HashSet<String>) -> HashSet<String> {
        let mut scope = roots.clone();
        let mut queue: std::collections::VecDeque<String> = roots.into_iter().collect();
        let gti = self.gti_index.read();
        while let Some(fqn) = queue.pop_front() {
            if let Some(descendants) = gti.get(&fqn) {
                for desc in descendants {
                    let normalized = normalize_fqn(desc).to_string();
                    if scope.insert(normalized.clone()) {
                        queue.push_back(normalized);
                    }
                }
            }
        }
        scope
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

    fn collect_macro_declaring_scope(&self, macro_targets: &[String]) -> HashSet<String> {
        let mut scope: HashSet<String> = macro_targets
            .iter()
            .map(|fqn| normalize_fqn(fqn).to_string())
            .collect();
        let mut queue: std::collections::VecDeque<String> = scope.iter().cloned().collect();
        let gti = self.gti_index.read();
        while let Some(fqn) = queue.pop_front() {
            if let Some(descendants) = gti.get(&fqn) {
                for desc in descendants {
                    let normalized = normalize_fqn(desc).to_string();
                    if scope.insert(normalized.clone()) {
                        queue.push_back(normalized);
                    }
                }
            }
        }
        scope
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

        // Parent class chain.
        if let Some(ref parent) = cls.parent_class {
            let parent_fqn = normalize_fqn(parent);
            if hierarchy.insert(parent_fqn.clone()) {
                self.collect_ancestors(&parent_fqn, class_loader, hierarchy);
            }
        }

        // Interfaces.
        for iface in &cls.interfaces {
            let iface_fqn = normalize_fqn(iface);
            if hierarchy.insert(iface_fqn.clone()) {
                self.collect_ancestors(&iface_fqn, class_loader, hierarchy);
            }
        }

        // Used traits.
        for trait_name in &cls.used_traits {
            let trait_fqn = normalize_fqn(trait_name);
            if hierarchy.insert(trait_fqn.clone()) {
                self.collect_ancestors(&trait_fqn, class_loader, hierarchy);
            }
        }

        // Mixins.
        for mixin in &cls.mixins {
            let mixin_fqn = normalize_fqn(mixin);
            if hierarchy.insert(mixin_fqn.clone()) {
                self.collect_ancestors(&mixin_fqn, class_loader, hierarchy);
            }
        }
    }
}
