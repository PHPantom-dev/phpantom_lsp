//! Find-references entry points and symbol-kind dispatch.
//!
//! [`Backend::find_references`] and its rename-specific sibling look up
//! the symbol under the cursor and route it to the appropriate
//! per-symbol-kind finder (variables, classes, members, functions).

use super::*;

use tower_lsp::lsp_types::{Location, Position};

use crate::references::push_location;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind};
use crate::text_position::offset_to_position;
use crate::util::build_fqn;
use crate::virtual_members::laravel;

impl Backend {
    /// Entry point for `textDocument/references`.
    ///
    /// Returns all locations where the symbol under the cursor is
    /// referenced.  When `include_declaration` is true the declaration
    /// site itself is included in the results.
    ///
    /// Waits for the initial workspace index but does not refresh it.  A
    /// user command that should also discover files created without a
    /// watcher event runs [`Self::ensure_workspace_indexed_for_request`]
    /// itself, *before* it reads `content`: a refresh that parses new files
    /// re-infers Blade templates, which can rewrite their virtual PHP, so a
    /// template's `content` and `position` read ahead of it would no longer
    /// be the text its symbol map describes.
    pub fn find_references(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        self.ensure_workspace_index_ready_for_request();
        self.find_references_inner(
            uri,
            content,
            position,
            include_declaration,
            ReferenceSearchMode::References,
        )
    }

    /// Like [`find_references`], but kept separate for rename-specific call
    /// sites that need the same precise member filtering.
    pub(crate) fn find_references_for_rename(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        self.ensure_workspace_index_ready_for_request();
        self.find_references_inner(
            uri,
            content,
            position,
            include_declaration,
            ReferenceSearchMode::Rename,
        )
    }

    fn find_references_inner(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        include_declaration: bool,
        mode: ReferenceSearchMode,
    ) -> Option<Vec<Location>> {
        let start_total = std::time::Instant::now();
        tracing::info!(
            "Find References: starting at {} line {} char {}",
            uri,
            position.line,
            position.character
        );

        // Consult the precomputed symbol map for the current file
        // (retries one byte earlier for end-of-token edge cases).
        let symbol = self.lookup_symbol_at_position(uri, content, position);

        // When the cursor is on a symbol span, dispatch by kind.
        if let Some(ref sym) = symbol {
            tracing::info!(
                "Find References: found symbol kind {:?} at offset {}",
                sym.kind,
                sym.start
            );
            let mut locations = self.dispatch_symbol_references(
                &sym.kind,
                uri,
                content,
                sym.start,
                include_declaration,
                mode,
            );
            // A YAML/XML occurrence of a class or namespace is a reference
            // the user can be shown, but not one a PHP-shaped edit can be
            // written over: its text may be the escaped `App\\Handler` form
            // the document's own quoting requires.  Rename rewrites those
            // through the framework resource index, which keeps each
            // occurrence's spelling, so they are left out here.  A member
            // occurrence (`Controller::action`) covers the bare member name,
            // which the PHP edit replaces as written.
            if mode == ReferenceSearchMode::Rename
                && !matches!(
                    sym.kind,
                    SymbolKind::MemberAccess { .. } | SymbolKind::MemberDeclaration { .. }
                )
            {
                locations.retain(|location| {
                    !crate::resource_navigation::is_resource_document(location.uri.as_str())
                });
            }
            sort_locations_for_references(&mut locations);
            tracing::info!(
                "Find References: total time for {:?}: {:?}",
                sym.kind,
                start_total.elapsed()
            );
            if !locations.is_empty() {
                return Some(locations);
            }
        }

        // Fallback for declaration sites in config/*.php and routes/*.php
        let start_laravel = std::time::Instant::now();
        if self.resolved_class_cache.read().is_laravel()
            && let Some(mut locations) =
                laravel::find_config_references(self, uri, content, position, include_declaration)
                    .or_else(|| {
                        laravel::find_route_registration_references(
                            self,
                            uri,
                            content,
                            position,
                            include_declaration,
                        )
                    })
        {
            sort_locations_for_references(&mut locations);
            tracing::info!(
                "Find References: found Laravel declaration-site references in {:?}",
                start_laravel.elapsed()
            );
            tracing::info!(
                "Find References: total time (fallback path): {:?}",
                start_total.elapsed()
            );
            return Some(locations);
        }

        if let Some(locations) =
            self.find_framework_references_at(uri, content, position, include_declaration, mode)
            && !locations.is_empty()
        {
            tracing::info!("Find References: found Symfony/Doctrine resource references");
            tracing::info!(
                "Find References: total time (framework path): {:?}",
                start_total.elapsed()
            );
            return Some(locations);
        }

        tracing::info!(
            "Find References: no references found in {:?}",
            start_total.elapsed()
        );
        None
    }

    /// The occurrences a rename of the Symfony or Doctrine name under the
    /// cursor has to edit.
    pub(crate) fn find_framework_references_for_rename(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Vec<Location>> {
        self.find_framework_references_at(uri, content, position, true, ReferenceSearchMode::Rename)
    }

    fn find_framework_references_at(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        include_declaration: bool,
        mode: ReferenceSearchMode,
    ) -> Option<Vec<Location>> {
        let reference = self.framework_reference_at_position(uri, content, position)?;
        let locations = match reference.kind {
            FrameworkReferenceKind::Class { fqn } => {
                self.find_class_references_in_mode(&fqn, include_declaration, mode)
            }
            FrameworkReferenceKind::Method {
                class_fqn,
                member_name,
            } => {
                let hierarchy = self
                    .collect_member_receiver_scope(
                        std::slice::from_ref(&class_fqn),
                        &member_name,
                        false,
                        mode.include_declaring_interfaces(),
                    )
                    .unwrap_or_else(|| self.collect_hierarchy_for_fqns(&[class_fqn]));
                self.find_member_references(
                    &member_name,
                    false,
                    include_declaration,
                    Some(&hierarchy),
                    Some(&hierarchy),
                )
            }
            FrameworkReferenceKind::SymfonySymbol { kind, name, .. } => {
                self.framework_symfony_symbol_locations(kind, &name, include_declaration, true)
            }
            FrameworkReferenceKind::RouteParameter {
                route_name, name, ..
            } => self.framework_route_parameter_locations(
                &route_name,
                &name,
                include_declaration,
                true,
            ),
            FrameworkReferenceKind::Translation { domain, name, .. } => {
                self.framework_translation_locations(&domain, &name, include_declaration, true)
            }
            FrameworkReferenceKind::Namespace { .. } | FrameworkReferenceKind::Path { .. } => {
                Vec::new()
            }
        };
        Some(locations)
    }

    /// Dispatch a symbol-map hit to the appropriate reference finder.
    fn dispatch_symbol_references(
        &self,
        kind: &SymbolKind,
        uri: &str,
        content: &str,
        span_start: u32,
        include_declaration: bool,
        mode: ReferenceSearchMode,
    ) -> Vec<Location> {
        match kind {
            SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => {
                // Property declarations use Variable spans (so GTD can
                // jump to the type hint), but Find References should
                // search for member accesses, not local variable uses.
                // Constructor-promoted properties are also Variable spans
                // (tagged VarDefKind::Parameter, since the token is also a
                // real parameter) but declare a property just the same.
                let is_property = matches!(
                    self.lookup_var_def_kind_at(uri, name, span_start),
                    Some(crate::symbol_map::VarDefKind::Property)
                ) || self.is_promoted_property_param(uri, span_start);
                if is_property {
                    // Properties are never static in the Variable span
                    // context ($this->prop).  Static properties use
                    // MemberAccess spans at their usage sites with
                    // is_static=true, but the declaration-site Variable
                    // span doesn't encode static-ness.  Check the
                    // uri_classes_index to determine the correct flag.
                    let is_static = self
                        .get_classes_for_uri(uri)
                        .iter()
                        .flat_map(|classes| classes.iter())
                        .flat_map(|c| c.properties.iter())
                        .any(|p| {
                            let p_name = p.name.strip_prefix('$').unwrap_or(&p.name);
                            p_name == name && p.is_static
                        });

                    // Resolve the enclosing class to scope the search.
                    let (hierarchy, declaration_scope) = self
                        .resolve_member_declaration_scopes(uri, span_start, name, is_static, mode)
                        .unzip();
                    return self.find_member_references(
                        name,
                        is_static,
                        include_declaration,
                        hierarchy.as_ref(),
                        declaration_scope.flatten().as_ref(),
                    );
                }
                self.find_variable_references(uri, content, name, span_start, include_declaration)
            }
            SymbolKind::ClassReference { name, is_fqn, .. } => {
                let ctx = self.file_context(uri);
                let fqn = if *is_fqn {
                    name.to_string()
                } else {
                    ctx.resolve_name_at(name, span_start)
                };
                self.find_class_references_in_mode(&fqn, include_declaration, mode)
            }
            SymbolKind::ClassDeclaration { name } => {
                let ctx = self.file_context(uri);
                let fqn = build_fqn(name, ctx.namespace_at(span_start).as_deref());
                self.find_class_references_in_mode(&fqn, include_declaration, mode)
            }
            SymbolKind::MemberAccess {
                subject_text,
                member_name,
                is_static,
                is_method_call,
                ..
            } => {
                let Some(source) = self.symbol_map_source(uri, content) else {
                    return Vec::new();
                };

                // Resolve the subject to determine the class hierarchy
                // so we only return references on related classes.
                let (hierarchy, declaration_scope) = self.resolve_member_access_scopes(
                    uri,
                    subject_text.as_str(source),
                    *is_static,
                    span_start,
                    member_name,
                    mode,
                );

                // Constructors are not invoked through member accesses
                // (`$obj->__construct()`); they are invoked through
                // `new ClassName(...)`.  An explicit `parent::__construct()`
                // call still lands here, so route to the constructor finder
                // seeded with the subject's resolved class(es).
                if is_constructor_name(member_name) {
                    let seeds = self
                        .reference_file_content(uri)
                        .map(|file_content| {
                            self.resolve_subject_to_fqns(
                                subject_text.as_str(source),
                                *is_static,
                                &self.file_context(uri),
                                span_start,
                                &file_content,
                            )
                        })
                        .unwrap_or_default();
                    return self.find_constructor_references(&seeds, include_declaration);
                }

                let mut locations = self.find_member_references(
                    member_name,
                    *is_static,
                    include_declaration,
                    hierarchy.as_ref(),
                    declaration_scope.as_ref(),
                );

                if *is_method_call && include_declaration {
                    let before_len = locations.len();
                    let call_position = offset_to_position(content, span_start as usize);
                    for def in self.resolve_definition(uri, content, call_position) {
                        let mut start = def.range.start;
                        let mut end = def.range.end;
                        if start == end {
                            let def_uri = def.uri.to_string();
                            if let Some(def_content) = self.get_file_content(&def_uri)
                                && let Some(def_span) =
                                    self.lookup_symbol_at_position(&def_uri, &def_content, start)
                            {
                                start = offset_to_position(&def_content, def_span.start as usize);
                                end = offset_to_position(&def_content, def_span.end as usize);
                            }
                        }
                        push_location(&mut locations, &def.uri, start, end);
                    }
                    self.append_laravel_macro_registration_locations(
                        &mut locations,
                        member_name,
                        declaration_scope.as_ref().or(hierarchy.as_ref()),
                    );
                    if locations.len() == before_len {
                        self.append_unique_laravel_macro_registration_location(
                            &mut locations,
                            member_name,
                        );
                    }
                }

                locations
            }
            SymbolKind::FunctionCall { name, .. } => {
                let ctx = self.file_context(uri);
                let fqn = ctx.resolve_name_at(name, span_start);
                // The span text is qualified at a `use function Foo\bar;`
                // import and at an FQN call site, and the short-name
                // fallback compares against a short name.
                self.find_function_references(
                    &fqn,
                    crate::util::short_name(name),
                    include_declaration,
                )
            }
            SymbolKind::ConstantReference { name, .. } => {
                let fqn = self.constant_fqn_at(uri, span_start, name);
                self.find_constant_references(
                    &fqn,
                    crate::util::short_name(name),
                    include_declaration,
                )
            }
            SymbolKind::MemberDeclaration { name, is_static } => {
                // A constructor declaration's "references" are the
                // `new ClassName(...)` instantiation sites (and `#[...]`
                // attribute usages), not `->__construct()` member accesses
                // (which don't exist in normal PHP code).
                if is_constructor_name(name) {
                    let ctx = self.file_context(uri);
                    let seeds: Vec<String> =
                        crate::class_lookup::find_class_at_offset(&ctx.classes, span_start)
                            .map(|cc| vec![cc.fqn().to_string()])
                            .unwrap_or_default();
                    return self.find_constructor_references(&seeds, include_declaration);
                }

                // Resolve the enclosing class to scope the search.
                let (hierarchy, declaration_scope) = self
                    .resolve_member_declaration_scopes(uri, span_start, name, *is_static, mode)
                    .unzip();
                let mut locations = self.find_member_references(
                    name,
                    *is_static,
                    include_declaration,
                    hierarchy.as_ref(),
                    declaration_scope.flatten().as_ref(),
                );
                if let Some(magic) = self.eloquent_magic_member_at(uri, span_start, name) {
                    locations.extend(
                        self.eloquent_magic_references_batch(&[&magic], None)
                            .into_iter()
                            .flatten(),
                    );
                }
                locations
            }
            SymbolKind::SelfStaticParent(ssp_kind) => {
                // `$this` is a file-local variable, not a cross-file class search.
                if *ssp_kind == SelfStaticParentKind::This {
                    return self.find_this_references(
                        uri,
                        content,
                        span_start,
                        include_declaration,
                    );
                }

                // For real self/static/parent keywords, resolve to the class FQN.
                let ctx = self.file_context(uri);
                let current_class =
                    crate::class_lookup::find_class_at_offset(&ctx.classes, span_start);
                let keyword = match ssp_kind {
                    SelfStaticParentKind::Parent => "parent",
                    _ => "self",
                };
                if let Some(fqn) =
                    crate::class_lookup::resolve_class_keyword(keyword, current_class)
                {
                    self.find_class_references_in_mode(&fqn, include_declaration, mode)
                } else {
                    Vec::new()
                }
            }

            SymbolKind::NamespaceDeclaration { .. } => Vec::new(),

            SymbolKind::LaravelStringKey { kind, key, .. } => {
                if !self.resolved_class_cache.read().is_laravel() {
                    return Vec::new();
                }
                // Config resources and their generic config spelling share a
                // canonical index identity. Declaration lookup is independent
                // of this usage snapshot, so every request stays narrow.
                let reference_key =
                    crate::reference_index::laravel_string_reference_key(*kind, key);
                let snapshot = self.user_file_symbol_maps_for_reference_keys(&[reference_key]);
                laravel::find_laravel_string_key_references(
                    self,
                    kind,
                    key,
                    uri,
                    &snapshot,
                    include_declaration,
                )
            }

            SymbolKind::LaravelMacroString { name } => {
                self.find_laravel_macro_references(uri, span_start, name, include_declaration)
            }

            SymbolKind::CommandOwnParam { .. }
            | SymbolKind::Keyword
            | SymbolKind::CastType
            | SymbolKind::Comment => Vec::new(),
        }
    }
}
