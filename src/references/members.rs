//! Member (method / property / constant) reference finding.
//!
//! Member references are filtered by the class hierarchy of the target
//! member so that an access on an unrelated class that merely shares a
//! member name is excluded.  Working out that hierarchy lives in
//! [`member_scope`](super::member_scope); this module runs the search,
//! including Laravel macros, which are invoked both statically and on
//! instances.  Resolving each access's receiver, and remembering the
//! answer, lives in [`receivers`](super::receivers).

use super::*;

use tower_lsp::lsp_types::{Location, Range};

use crate::atom::{Atom, AtomMap};
use crate::references::member_scope::MemberScope;
use crate::references::push_location;
use crate::references::receivers::ReceiverWalk;
use crate::symbol_map::SymbolKind;
use crate::symbol_map::SymbolMap;
use crate::text_position::offset_to_position;

#[derive(Clone)]
pub(crate) struct MemberDeclarationReferenceQuery {
    pub(crate) uri: Arc<str>,
    pub(crate) offset: u32,
    pub(crate) member: Atom,
    pub(crate) is_static: bool,
}

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
        let candidate_keys = [
            ReferenceIndexKey::Member {
                name: name.to_string(),
                is_static: false,
            },
            ReferenceIndexKey::Member {
                name: name.to_string(),
                is_static: true,
            },
        ];
        let mut locations = self.scan_reference_candidates(
            &candidate_keys,
            "Scanning for macro references",
            |file, symbol_map, locations| {
                if symbol_map.member_access_indices(name).is_empty() {
                    return;
                }

                // The rest of Find References reads a template as the
                // virtual PHP its symbol map describes, and
                // `try_translate_location` maps the results back; reading the
                // template's own bytes here would slice every span against
                // text the map knows nothing about.
                let Some(content) = file.content() else {
                    return;
                };
                let Some(source) = symbol_map.source(content) else {
                    return;
                };
                let file_ctx = self.file_context(file.uri());

                // Isolated for the same reason as the guard in
                // `resolve_member_receivers`: an outer request-level chain
                // cache can stay active across the whole scan, and `content`
                // is a fresh `Arc<String>` per file that is freed once this
                // file is done, so its address can be reused by a later file.
                // A fresh map per file keeps this file's entries from leaking
                // into (or answering for) the next one.
                let _chain_guard = crate::type_engine::resolver::with_isolated_chain_cache();

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

                    let matches_macro = if subject_text.as_str(source).contains('(') {
                        // Chained call receivers like
                        // `$query->pluck(...)->macroName()` are expensive to
                        // resolve precisely here and are the main real-world
                        // macro-registration rename case.
                        true
                    } else {
                        let subject_fqns = self.resolve_subject_to_fqns(
                            subject_text.as_str(source),
                            *is_static,
                            &file_ctx,
                            span.start,
                            content,
                        );
                        !subject_fqns.is_empty()
                            && subject_fqns.iter().any(|fqn| hierarchy.contains(self, fqn))
                    };
                    if !matches_macro {
                        continue;
                    }

                    if let Some(location) = file.location(span.start, span.end) {
                        locations.push(location);
                    }
                }
            },
        );

        if include_declaration {
            let macro_scope = MemberScope::exact(targets.iter().cloned().collect());
            self.append_laravel_macro_registration_locations(
                &mut locations,
                name,
                Some(&macro_scope),
            );
            sort_locations_for_references(&mut locations);
        }

        locations
    }

    pub(super) fn append_laravel_macro_registration_locations(
        &self,
        locations: &mut Vec<Location>,
        name: &str,
        targets: Option<&MemberScope>,
    ) {
        let Some(targets) = targets else {
            return;
        };
        let index = self.laravel_macros.read();
        for target in targets.indexed() {
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
            push_location(locations, &parsed_uri, start, end);
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
        push_location(locations, &parsed_uri, start, end);
    }

    /// Find the references to a member declaration, scoped to the class
    /// hierarchy that declares it.
    ///
    /// This is the same search Find References runs on the declaration, so
    /// the number matches what the user sees when they follow the hint.
    pub(crate) fn member_declaration_references(
        &self,
        uri: &str,
        offset: u32,
        member_name: &str,
        is_static: bool,
    ) -> Vec<Location> {
        self.member_declaration_references_batch(&[MemberDeclarationReferenceQuery {
            uri: Arc::from(uri),
            offset,
            member: crate::atom::atom(member_name),
            is_static,
        }])
        .pop()
        .unwrap_or_default()
    }

    /// Find exact references for several member declarations in one semantic
    /// pass over the union of their candidate files.
    ///
    /// A viewport commonly queues many declarations at once. Resolving each
    /// declaration separately reopens the same files and repeats receiver
    /// inference for every same-named method in unrelated class hierarchies.
    /// This batch resolves each matching access once, then attributes it only
    /// to queries whose hierarchy contains the receiver class.
    pub(crate) fn member_declaration_references_batch(
        &self,
        queries: &[MemberDeclarationReferenceQuery],
    ) -> Vec<Vec<Location>> {
        self.member_declaration_references_batch_in(queries, None)
    }

    /// The same search, optionally narrowed to `restrict_to`.
    ///
    /// An edit moves the accesses in the files it reparsed and leaves every
    /// other file's alone, so a declaration whose locations are still cached
    /// only has to be searched again in those files.  Passing `None` searches
    /// every candidate file, which is what a first computation needs.
    pub(crate) fn member_declaration_references_batch_in(
        &self,
        queries: &[MemberDeclarationReferenceQuery],
        restrict_to: Option<&HashSet<Arc<str>>>,
    ) -> Vec<Vec<Location>> {
        struct PreparedQuery {
            member: Atom,
            is_static: bool,
            hierarchy: Option<MemberScope>,
        }

        if queries.is_empty() {
            return Vec::new();
        }

        let mode = ReferenceSearchMode::References;
        let prepared: Vec<_> = queries
            .iter()
            .map(|query| PreparedQuery {
                member: query.member,
                is_static: query.is_static,
                hierarchy: self
                    .resolve_member_declaration_scopes(
                        &query.uri,
                        query.offset,
                        &query.member,
                        query.is_static,
                        mode,
                    )
                    .map(|(hierarchy, _)| hierarchy),
            })
            .collect();

        let mut by_member: AtomMap<Vec<usize>> = AtomMap::default();
        let mut candidate_keys = HashSet::new();
        for (query_index, query) in prepared.iter().enumerate() {
            by_member.entry(query.member).or_default().push(query_index);
            candidate_keys.extend(member_candidate_keys(
                &query.member,
                query.is_static,
                query.hierarchy.as_ref(),
            ));
        }

        let candidate_keys: Vec<_> = candidate_keys.into_iter().collect();
        let mut snapshot = self.user_file_symbol_maps_for_reference_keys(&candidate_keys);
        if let Some(files) = restrict_to {
            snapshot.retain(|(uri, _)| files.contains(uri.as_str()));
        }

        // Only a query that filters by hierarchy can rule a file out; the
        // name-only fallback accepts any receiver, so one such query in the
        // batch keeps every candidate.
        let filtered: Option<Vec<(Atom, &MemberScope)>> = prepared
            .iter()
            .map(|query| query.hierarchy.as_ref().map(|h| (query.member, h)))
            .collect();
        if let Some(filtered) = filtered {
            snapshot.retain(|(uri, symbol_map)| {
                !self.member_accesses_ruled_out(uri, symbol_map, &filtered)
            });
        }

        let scan_file =
            |file: &CandidateFile<'_>, symbol_map: &Arc<SymbolMap>| -> Vec<(usize, Location)> {
                let file_uri = file.uri();
                let mut span_indices = Vec::new();
                for member in by_member.keys() {
                    span_indices.extend_from_slice(symbol_map.member_access_indices(member));
                }
                if span_indices.is_empty() {
                    return Vec::new();
                }
                span_indices.sort_unstable();
                span_indices.dedup();

                let Some(parsed_uri) = file.url() else {
                    return Vec::new();
                };

                // A warm entry that answers for every name being searched records
                // both the receiver and the range of each access it resolved, so
                // the file's text is not needed at all.  An access it holds
                // nothing for resolved to nothing, which only a query filtering by
                // hierarchy can decide without a range — hence the `all`.
                let hierarchy_only = prepared.iter().all(|query| query.hierarchy.is_some());
                let warm = hierarchy_only
                    .then(|| self.resolved_member_file(file_uri, symbol_map))
                    .flatten()
                    .filter(|file| file.covers(by_member.keys().copied()));
                if let Some(warm) = warm {
                    let mut matches = Vec::new();
                    for span_index in span_indices {
                        let SymbolKind::MemberAccess { member_name, .. } =
                            &symbol_map.spans[span_index].kind
                        else {
                            continue;
                        };
                        let Some(query_indices) = by_member.get(member_name) else {
                            continue;
                        };
                        let Some((range, subject_fqns)) = warm.resolved_access(span_index) else {
                            continue;
                        };
                        for &query_index in query_indices {
                            let Some(hierarchy) = prepared[query_index].hierarchy.as_ref() else {
                                continue;
                            };
                            if subject_fqns.iter().any(|fqn| hierarchy.contains(self, fqn)) {
                                matches.push((
                                    query_index,
                                    Location {
                                        uri: parsed_uri.clone(),
                                        range,
                                    },
                                ));
                            }
                        }
                    }
                    return matches;
                }

                let Some(content) = file.content() else {
                    return Vec::new();
                };
                let Some(source) = symbol_map.source(content) else {
                    return Vec::new();
                };
                let position = |offset: u32| file.position(offset).unwrap_or_default();
                let needs_receiver = prepared.iter().any(|query| query.hierarchy.is_some());
                let resolved_file = needs_receiver.then(|| {
                    // Only the accesses this search asks about are worth
                    // resolving: a receiver walk costs a pass of the type engine
                    // over the whole file, and a file that holds one `->save()`
                    // among two hundred other member accesses would otherwise pay
                    // for all of them.  What an earlier search resolved is carried
                    // over rather than walked again.
                    let searched: Vec<Atom> = by_member.keys().copied().collect();
                    self.resolve_member_receivers(
                        file_uri,
                        symbol_map,
                        content,
                        source,
                        &position,
                        ReceiverWalk::Names(&searched),
                    )
                });

                let mut matches = Vec::new();
                for span_index in span_indices {
                    let span = &symbol_map.spans[span_index];
                    let SymbolKind::MemberAccess {
                        member_name,
                        is_static,
                        ..
                    } = &span.kind
                    else {
                        continue;
                    };
                    let Some(query_indices) = by_member.get(member_name) else {
                        continue;
                    };

                    let resolved = resolved_file
                        .as_ref()
                        .and_then(|resolved| resolved.resolved_access(span_index));
                    let (subject_fqns, range) = match resolved {
                        Some((range, targets)) => (targets, range),
                        None => (
                            &[][..],
                            Range::new(position(span.start), position(span.end)),
                        ),
                    };

                    for &query_index in query_indices {
                        let query = &prepared[query_index];
                        if let Some(hierarchy) = &query.hierarchy {
                            if !subject_fqns.iter().any(|fqn| hierarchy.contains(self, fqn)) {
                                continue;
                            }
                        } else if query.is_static != *is_static {
                            continue;
                        }

                        matches.push((
                            query_index,
                            Location {
                                uri: parsed_uri.clone(),
                                range,
                            },
                        ));
                    }
                }
                matches
            };

        let mut locations = vec![Vec::new(); queries.len()];
        for (query_index, location) in
            self.scan_candidate_snapshot(snapshot, "Scanning for member references", scan_file)
        {
            locations[query_index].push(location);
        }

        for query_locations in &mut locations {
            sort_locations_for_references(query_locations);
        }

        locations
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
        hierarchy: Option<&MemberScope>,
        declaration_scope: Option<&MemberScope>,
    ) -> Vec<Location> {
        let candidate_keys = member_candidate_keys(target_member, target_is_static, hierarchy);
        let member = crate::atom::atom(target_member);
        let target_name = target_member.strip_prefix('$').unwrap_or(target_member);
        self.scan_reference_candidates(
            &candidate_keys,
            "Scanning for member references",
            |file, symbol_map, locations| {
                let file_uri = file.uri();
                // First pass: name-only check to avoid unnecessary work.
                // When a hierarchy is present (e.g. Laravel), we allow static
                // mismatch.
                let names_the_member =
                    symbol_map
                        .member_access_indices(target_member)
                        .iter()
                        .any(|&idx| match &symbol_map.spans[idx].kind {
                            SymbolKind::MemberAccess { is_static, .. } => {
                                hierarchy.is_some() || *is_static == target_is_static
                            }
                            _ => false,
                        });
                // A file whose accesses to the name all settle on a class
                // outside the hierarchy is answered here rather than read.
                let has_member_access_match = names_the_member
                    && hierarchy.is_none_or(|hier| {
                        !self.member_accesses_ruled_out(file_uri, symbol_map, &[(member, hier)])
                    });

                if has_member_access_match {
                    self.push_member_access_matches(
                        file,
                        symbol_map,
                        member,
                        target_is_static,
                        hierarchy,
                        locations,
                    );
                }

                if !include_declaration {
                    return;
                }

                // Lazily resolved file context — only computed when we need
                // to find a declaration's enclosing class.
                let file_ctx_cell: std::cell::OnceCell<crate::types::FileContext> =
                    std::cell::OnceCell::new();

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
                                let enclosing = super::member_scope::enclosing_class_for_member(
                                    &ctx.classes,
                                    span.start,
                                );
                                if let Some(enclosing) = enclosing
                                    && !hier.contains(self, &enclosing.fqn())
                                {
                                    continue;
                                }
                            }

                            let Some(location) = file.location(span.start, span.end) else {
                                break;
                            };
                            locations.push(location);
                        }
                        _ => {}
                    }
                }

                // Property declarations use Variable spans (not
                // MemberDeclaration) because GTD relies on the Variable
                // kind to jump to the type hint.  Scan the uri_classes_index
                // to pick up property declaration sites.
                if let Some(classes) = self.shared_classes_for_uri(file_uri) {
                    for class in &classes {
                        if let Some(hier) = declaration_scope.or(hierarchy)
                            && !hier.contains(self, &class.fqn())
                        {
                            continue;
                        }

                        for prop in &class.properties {
                            let prop_name = prop.name.strip_prefix('$').unwrap_or(&prop.name);
                            if prop_name == target_name
                                && prop.is_static == target_is_static
                                && prop.name_offset != 0
                            {
                                // `name_offset` points at the `$` sigil while
                                // `prop.name` excludes it, so the range must
                                // span the `$` plus the name (`$name`, not
                                // `$nam`).
                                let offset = prop.name_offset;
                                let Some(location) =
                                    file.location(offset, offset + 1 + prop.name.len() as u32)
                                else {
                                    break;
                                };
                                locations.push(location);
                            }
                        }
                    }
                }
            },
        )
    }

    /// The accesses to `member` in one file that Find References reports.
    ///
    /// With a hierarchy, each receiver is resolved through
    /// [`resolve_member_receivers`](Self::resolve_member_receivers), the
    /// same pass the reference-count lens runs: an earlier search's entry
    /// answers without the file being opened, and this search's walk is
    /// recorded for the next one.  An access whose receiver resolves to
    /// nothing is skipped, since accepting every unresolved
    /// `$x->method()` makes common names such as `find` unusably noisy in
    /// large projects.
    ///
    /// Without one, every access of the right static-ness matches.
    fn push_member_access_matches(
        &self,
        file: &CandidateFile<'_>,
        symbol_map: &Arc<SymbolMap>,
        member: Atom,
        target_is_static: bool,
        hierarchy: Option<&MemberScope>,
        locations: &mut Vec<Location>,
    ) {
        let access_indices = symbol_map.member_access_indices(&member);

        let Some(hierarchy) = hierarchy else {
            for &span_index in access_indices {
                let span = &symbol_map.spans[span_index];
                let SymbolKind::MemberAccess { is_static, .. } = &span.kind else {
                    continue;
                };
                if *is_static != target_is_static {
                    continue;
                }
                let Some(location) = file.location(span.start, span.end) else {
                    return;
                };
                locations.push(location);
            }
            return;
        };

        // A static-ness mismatch is allowed here: for Laravel custom
        // builders `Model::active()` is static while `UserBuilder->active()`
        // is not, and the hierarchy is what shows they are related.
        let Some(resolved) = self.member_receivers_for(file, symbol_map, &[member]) else {
            return;
        };
        for &span_index in access_indices {
            let Some((range, targets)) = resolved.resolved_access(span_index) else {
                continue;
            };
            if targets.iter().any(|fqn| hierarchy.contains(self, fqn))
                && let Some(uri) = file.url()
            {
                locations.push(Location { uri, range });
            }
        }
    }
}
