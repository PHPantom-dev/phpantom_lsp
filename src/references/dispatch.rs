//! Find-references entry points and symbol-kind dispatch.
//!
//! [`Backend::find_references`] and its rename-specific sibling look up
//! the symbol under the cursor and route it to the appropriate
//! per-symbol-kind finder (variables, classes, members, functions).

use super::*;

use tower_lsp::lsp_types::{Location, Position};

use crate::symbol_map::{SelfStaticParentKind, SymbolKind};
use crate::util::{build_fqn, offset_to_position, push_unique_location};
use crate::virtual_members::laravel;

impl Backend {
    /// Entry point for `textDocument/references`.
    ///
    /// Returns all locations where the symbol under the cursor is
    /// referenced.  When `include_declaration` is true the declaration
    /// site itself is included in the results.
    pub fn find_references(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
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
            let locations = self.dispatch_symbol_references(
                &sym.kind,
                uri,
                content,
                sym.start,
                include_declaration,
                mode,
            );
            tracing::info!(
                "Find References: total time for {:?}: {:?}",
                sym.kind,
                start_total.elapsed()
            );
            if !locations.is_empty() {
                return Some(locations);
            }
        }

        // Fallback for declaration sites in config/*.php
        let start_laravel = std::time::Instant::now();
        if let Some(locations) =
            laravel::find_config_references(self, uri, content, position, include_declaration)
        {
            tracing::info!(
                "Find References: found Laravel config references in {:?}",
                start_laravel.elapsed()
            );
            tracing::info!(
                "Find References: total time (fallback path): {:?}",
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
                if let Some(crate::symbol_map::VarDefKind::Property) =
                    self.lookup_var_def_kind_at(uri, name, span_start)
                {
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
                    let hierarchy = self.resolve_member_declaration_hierarchy(
                        uri, span_start, name, is_static, mode,
                    );
                    let declaration_scope = self
                        .resolve_member_declaration_scope(uri, span_start, name, is_static, mode);
                    return self.find_member_references(
                        name,
                        is_static,
                        include_declaration,
                        hierarchy.as_ref(),
                        declaration_scope.as_ref(),
                    );
                }
                self.find_variable_references(uri, content, name, span_start, include_declaration)
            }
            SymbolKind::ClassReference { name, is_fqn, .. } => {
                let ctx = self.file_context(uri);
                let fqn = if *is_fqn {
                    name.clone()
                } else {
                    ctx.resolve_name_at(name, span_start)
                };
                self.find_class_references(&fqn, include_declaration)
            }
            SymbolKind::ClassDeclaration { name } => {
                let ctx = self.file_context(uri);
                let fqn = build_fqn(name, ctx.namespace.as_deref());
                self.find_class_references(&fqn, include_declaration)
            }
            SymbolKind::MemberAccess {
                subject_text,
                member_name,
                is_static,
                is_method_call,
                ..
            } => {
                // Resolve the subject to determine the class hierarchy
                // so we only return references on related classes.
                let (hierarchy, declaration_scope) = self.resolve_member_access_scopes(
                    uri,
                    subject_text,
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
                        .map(|content| {
                            self.resolve_subject_to_fqns(
                                subject_text,
                                *is_static,
                                &self.file_context(uri),
                                span_start,
                                &content,
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
                        push_unique_location(&mut locations, &def.uri, start, end);
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
                self.find_function_references(&fqn, name, include_declaration)
            }
            SymbolKind::ConstantReference { name } => {
                self.find_constant_references(name, include_declaration)
            }
            SymbolKind::MemberDeclaration { name, is_static } => {
                // A constructor declaration's "references" are the
                // `new ClassName(...)` instantiation sites (and `#[...]`
                // attribute usages), not `->__construct()` member accesses
                // (which don't exist in normal PHP code).
                if is_constructor_name(name) {
                    let ctx = self.file_context(uri);
                    let seeds: Vec<String> =
                        crate::util::find_class_at_offset(&ctx.classes, span_start)
                            .map(|cc| vec![build_fqn(&cc.name, ctx.namespace.as_deref())])
                            .unwrap_or_default();
                    return self.find_constructor_references(&seeds, include_declaration);
                }

                // Resolve the enclosing class to scope the search.
                let hierarchy = self
                    .resolve_member_declaration_hierarchy(uri, span_start, name, *is_static, mode);
                let declaration_scope =
                    self.resolve_member_declaration_scope(uri, span_start, name, *is_static, mode);
                self.find_member_references(
                    name,
                    *is_static,
                    include_declaration,
                    hierarchy.as_ref(),
                    declaration_scope.as_ref(),
                )
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
                let current_class = crate::util::find_class_at_offset(&ctx.classes, span_start);
                let fqn = match ssp_kind {
                    SelfStaticParentKind::Parent => {
                        current_class.and_then(|cc| cc.parent_class.map(|a| a.to_string()))
                    }
                    _ => current_class.map(|cc| build_fqn(&cc.name, ctx.namespace.as_deref())),
                };
                if let Some(fqn) = fqn {
                    self.find_class_references(&fqn, include_declaration)
                } else {
                    Vec::new()
                }
            }

            SymbolKind::NamespaceDeclaration { .. } => Vec::new(),

            SymbolKind::LaravelStringKey { kind, key } => {
                let snapshot = if include_declaration
                    && matches!(kind, crate::symbol_map::LaravelStringKind::Config)
                {
                    self.user_file_symbol_maps()
                } else {
                    self.user_file_symbol_maps_for_reference_keys(&[
                        ReferenceIndexKey::LaravelString {
                            kind: kind.clone(),
                            key: key.to_string(),
                        },
                    ])
                };
                laravel::find_laravel_string_key_references(
                    self,
                    kind,
                    key,
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
