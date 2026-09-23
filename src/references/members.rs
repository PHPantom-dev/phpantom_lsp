//! Member (method / property / constant) reference finding.
//!
//! Member references are filtered by the class hierarchy of the target
//! member so that an access on an unrelated class that merely shares a
//! member name is excluded.  Working out that hierarchy lives in
//! [`member_scope`](super::member_scope); this module runs the search,
//! including Laravel macros, which are invoked both statically and on
//! instances.

use super::*;

use std::cell::OnceCell;

use tower_lsp::lsp_types::{Location, Range};

use crate::atom::{Atom, AtomMap};
use crate::class_lookup::find_class_at_offset;
use crate::references::member_scope::MemberScope;
use crate::references::push_location;
use crate::symbol_map::SymbolMap;
use crate::symbol_map::{SelfStaticParentKind, SubjectText, SymbolKind};
use crate::text_position::{LineIndex, offset_to_position};

/// Whether a receiver of this text resolves without the file's content.
///
/// [`resolve_subject_type`](crate::type_engine::subject_resolution::resolve_subject_type)
/// reads the content only for the receivers it hands to the forward walker
/// and the chain resolver.  The class keywords never get there, and neither
/// does the subject of a static access: both resolve from the enclosing
/// class and the import table alone, which the candidate filter already
/// holds.
fn receiver_settles_without_content(subject_text: &str, is_static: bool) -> bool {
    matches!(subject_text, "$this" | "self" | "static" | "parent")
        || (is_static && !subject_text.starts_with('$'))
}

/// The text a member access's receiver is written with, recovered without
/// reading the file, for the receivers whose class the file's own text
/// settles.
///
/// The candidate filter runs before any file is opened, so it cannot slice a
/// `Range` subject out of the content the way the scan does.  What it can do
/// is recognise the *span* that range covers: a receiver written as `$this`,
/// `self`, `static`, `parent`, or a class name emits a span of its own at
/// exactly that range, and that span carries the text.  Each arm
/// reconstructs the source the span was built from and checks it against the
/// range's width, so a reconstruction that comes out a different length
/// counts as unsettled rather than as something else.
///
/// Handing the text back rather than a resolved class is what keeps the
/// filter honest: it goes to the same
/// [`resolve_subject_to_fqns`](Backend::resolve_subject_to_fqns) the scan
/// calls, so the two cannot answer differently.
///
/// `None` for every other receiver — a variable, a chain, a `new` expression
/// — since settling those is the type engine's job and needs the file.
fn settled_receiver_text(symbol_map: &SymbolMap, span_index: usize) -> Option<String> {
    let SymbolKind::MemberAccess {
        subject_text,
        is_static,
        ..
    } = &symbol_map.spans[span_index].kind
    else {
        return None;
    };

    let (start, end) = match subject_text {
        // A subject the extraction synthesised carries its own text, so
        // there is nothing to recover.  It is passed on as written rather
        // than trimmed, since that is what the scan would hand over.
        SubjectText::Owned(text) => {
            return receiver_settles_without_content(text, *is_static).then(|| text.to_string());
        }
        SubjectText::Range { start, end } => (*start, *end),
    };
    let width = end.checked_sub(start)?;

    let text = match &symbol_map.span_covering_exactly(start, end)?.kind {
        SymbolKind::SelfStaticParent(kind) => match kind {
            SelfStaticParentKind::This => "$this".to_string(),
            SelfStaticParentKind::Self_ => "self".to_string(),
            SelfStaticParentKind::Static => "static".to_string(),
            SelfStaticParentKind::Parent => "parent".to_string(),
        },
        SymbolKind::ClassReference { name, is_fqn, .. } => {
            if *is_fqn {
                format!("\\{name}")
            } else {
                name.to_string()
            }
        }
        _ => return None,
    };

    (text.len() as u32 == width && receiver_settles_without_content(&text, *is_static))
        .then_some(text)
}

/// How much of a file a receiver resolution walks.
///
/// The walk is what costs — resolving a receiver means forward-walking the
/// body it sits in from the first statement — so a search walks as little of
/// the file as it can get away with, while a warm-up with no particular
/// access in mind walks all of it once.
#[derive(Clone, Copy)]
enum ReceiverWalk<'a> {
    /// Walk the bodies holding the accesses to these member names, then
    /// widen the entry to whatever else those bodies turned out to answer
    /// for.
    Names(&'a [Atom]),
    /// Walk every body, so the entry answers for every access in the file
    /// and no later search has to open it.
    WholeFile,
}

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
            // The rest of Find References reads a template as the virtual
            // PHP its symbol map describes, and `try_translate_location`
            // maps the results back; reading the template's own bytes here
            // would slice every span against text the map knows nothing
            // about.
            let file = CandidateFile::new(self, file_uri);
            let Some(content) = file.content() else {
                continue;
            };
            let Some(source) = symbol_map.source(content) else {
                continue;
            };
            let file_ctx = self.file_context(file_uri);

            // Isolated for the same reason as the guard in
            // `resolve_member_receivers`: an outer request-level chain cache
            // can stay active across this whole loop, and `content` is a
            // fresh `Arc<String>` per file that is freed at the end of this
            // iteration, so its address can be reused by a later file. A
            // fresh map per file keeps this file's entries from leaking into
            // (or answering for) the next one.
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
                    // Chained call receivers like `$query->pluck(...)->macroName()`
                    // are expensive to resolve precisely here and are the main
                    // real-world macro-registration rename case.
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

                if let Some(range) = file.range(span.start, span.end) {
                    locations.push(Location {
                        uri: parsed_uri.clone(),
                        range,
                    });
                }
            }
        }

        if include_declaration {
            let macro_scope = MemberScope::exact(targets.iter().cloned().collect());
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

    /// Whether what is already in memory answers "no reference here" for
    /// `uri`, so the file never has to be read.
    ///
    /// Candidate files are selected by member name alone, so a common name
    /// such as `save` or `handle` selects every file that accesses *any*
    /// class's member of that name — thousands of them on a large
    /// application, almost all on classes unrelated to the one being
    /// searched.  Telling them apart normally means reading each file and
    /// running the type engine over it to find out what its receivers are.
    ///
    /// For the receivers a file settles by itself — `$this`, `self`,
    /// `static`, `parent`, and a static access on a class name written at
    /// the access site — the answer instead comes out of the symbol map and
    /// the file's imports, both already in memory.  A file whose searched
    /// accesses are all of that kind, and none of which lands in a hierarchy
    /// being searched, cannot hold a reference and is dropped before
    /// anything opens it.
    ///
    /// For the receivers it does not settle, the answer comes from the
    /// resolved-member layer when an earlier search already walked this file
    /// for the same member name: that entry records what each access resolved
    /// to, which is exactly what the scan would recompute.  Every walk widens
    /// its entry to every name the bodies it entered answer for, so a file
    /// walked for one class rules itself out for the next without being
    /// opened.
    ///
    /// Conservative wherever it cannot be sure: one receiver that is neither
    /// settled by the text nor recorded keeps the whole file.  Every query has
    /// to carry a hierarchy, since the name-only fallback the search drops to
    /// without one accepts any receiver and so rules nothing out.
    pub(super) fn member_accesses_ruled_out(
        &self,
        uri: &str,
        symbol_map: &Arc<SymbolMap>,
        queries: &[(Atom, &MemberScope)],
    ) -> bool {
        let file_ctx = OnceCell::new();
        let resolved = self.resolved_member_file(uri, symbol_map);
        for (member, hierarchy) in queries {
            // A recorded entry answers for every access of a name it covers,
            // settled or not, so it replaces the test below rather than
            // supplementing it.  An access it holds nothing for resolved to
            // nothing and cannot be a reference.
            if let Some(recorded) = resolved
                .as_ref()
                .filter(|file| file.covers(std::iter::once(*member)))
            {
                for &span_index in symbol_map.member_access_indices(member) {
                    if let Some((_, targets)) = recorded.resolved_access(span_index)
                        && targets.iter().any(|fqn| hierarchy.contains(self, fqn))
                    {
                        return false;
                    }
                }
                continue;
            }

            for &span_index in symbol_map.member_access_indices(member) {
                let Some(subject_text) = settled_receiver_text(symbol_map, span_index) else {
                    return false;
                };
                let span = &symbol_map.spans[span_index];
                let SymbolKind::MemberAccess { is_static, .. } = &span.kind else {
                    continue;
                };
                // The content argument is unused for these subjects: every
                // one of them resolves from the enclosing class or the
                // import table, which is precisely what makes them settled.
                let subject_fqns = self.resolve_subject_to_fqns(
                    &subject_text,
                    *is_static,
                    file_ctx.get_or_init(|| self.file_context(uri)),
                    span.start,
                    "",
                );
                if subject_fqns.iter().any(|fqn| hierarchy.contains(self, fqn)) {
                    return false;
                }
            }
        }
        true
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
                hierarchy: self.resolve_member_declaration_hierarchy(
                    &query.uri,
                    query.offset,
                    &query.member,
                    query.is_static,
                    mode,
                ),
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

        self.begin_request_scan_window(snapshot.len(), "Scanning for member references");

        let scan_file = |file_uri: &str,
                         symbol_map: &Arc<crate::symbol_map::SymbolMap>|
         -> Vec<(usize, Location)> {
            let mut span_indices = Vec::new();
            for member in by_member.keys() {
                span_indices.extend_from_slice(symbol_map.member_access_indices(member));
            }
            if span_indices.is_empty() {
                return Vec::new();
            }
            span_indices.sort_unstable();
            span_indices.dedup();

            let Ok(parsed_uri) = Url::parse(file_uri) else {
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

            let Some(content) = self.reference_file_content_arc(file_uri) else {
                return Vec::new();
            };
            let Some(source) = symbol_map.source(&content) else {
                return Vec::new();
            };
            // One line table for the whole file: converting each access's
            // offsets by rescanning from the start of the file makes a file
            // holding many accesses to the searched names quadratic in its own
            // size, and a candidate file is scanned for both ends of every one.
            let lines = OnceCell::new();
            let position = |offset: u32| {
                lines
                    .get_or_init(|| LineIndex::new(&content))
                    .position(offset as usize)
            };
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
                    &content,
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
                    .and_then(|file| file.resolved_access(span_index));
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
        if snapshot.len() <= 2 {
            for (file_uri, symbol_map) in &snapshot {
                self.request_scan_file_done();
                for (query_index, location) in scan_file(file_uri, symbol_map) {
                    locations[query_index].push(location);
                }
            }
        } else {
            let worker_results =
                crate::parallel::map_indexed("member-references", snapshot.len(), |_, index| {
                    let (file_uri, symbol_map) = &snapshot[index];
                    self.request_scan_file_done();
                    let matches = scan_file(file_uri, symbol_map);
                    (!matches.is_empty()).then_some(matches)
                });
            for (_, matches) in worker_results {
                for (query_index, location) in matches {
                    locations[query_index].push(location);
                }
            }
        }

        for query_locations in &mut locations {
            sort_locations_for_references(query_locations);
        }

        locations
    }

    /// Resolve the receiver class of a file's member accesses and record
    /// them, so a later search reads the answer instead of walking the file.
    ///
    /// Whatever an earlier walk left behind is carried over, so a file
    /// already walked for one member name pays only for the names it has
    /// not seen yet, and a file whose entry already answers is not opened
    /// at all.
    ///
    /// The caller owns the file's text and its line table: a search has
    /// both in hand for the accesses it reports, and building a second line
    /// table for the same file is what this avoids.
    fn resolve_member_receivers(
        &self,
        uri: &str,
        symbol_map: &Arc<SymbolMap>,
        content: &Arc<String>,
        source: crate::symbol_map::MappedSource<'_>,
        position: &dyn Fn(u32) -> Position,
        walk: ReceiverWalk<'_>,
    ) -> Arc<crate::reference_index::ResolvedMemberFile> {
        // A whole-file walk is asked about every name the file accesses.  The
        // caller of the other arm already owns its list, so only this one has
        // a list to build.
        let whole_file_names: Vec<Atom> = match walk {
            ReceiverWalk::Names(_) => Vec::new(),
            ReceiverWalk::WholeFile => symbol_map.member_access_indices.keys().copied().collect(),
        };
        let searched: &[Atom] = match walk {
            ReceiverWalk::Names(names) => names,
            ReceiverWalk::WholeFile => &whole_file_names,
        };

        let previous = self.resolved_member_file(uri, symbol_map);
        if let Some(covering) = previous
            .as_ref()
            .filter(|file| file.covers(searched.iter().copied()))
        {
            return Arc::clone(covering);
        }

        let carried: Vec<(usize, Range, Vec<Atom>)> = previous
            .as_ref()
            .map(|file| file.resolutions().collect())
            .unwrap_or_default();
        // Carrying an earlier walk's resolutions over carries what that walk
        // consulted: the merged entry is only as valid as the older half of
        // it.
        let carried_deps: Vec<Atom> = previous
            .as_ref()
            .map(|file| file.deps().to_vec())
            .unwrap_or_default();
        let mut access_indices: Vec<usize> = Vec::new();
        for member in searched {
            let already_resolved = previous
                .as_ref()
                .is_some_and(|file| file.covers(std::iter::once(*member)));
            if !already_resolved {
                access_indices.extend_from_slice(symbol_map.member_access_indices(member));
            }
        }
        access_indices.sort_unstable();
        access_indices.dedup();

        let covered: Vec<Atom> = previous
            .as_ref()
            .map(|file| file.covered().to_vec())
            .unwrap_or_default()
            .into_iter()
            .chain(searched.iter().copied())
            .collect();

        if access_indices.is_empty() {
            return self.cache_resolved_member_file(
                uri,
                Arc::clone(symbol_map),
                covered,
                carried,
                carried_deps,
            );
        }

        // Everything below resolves receivers, and every class and function
        // it consults is what the entry stays valid against.  The recording
        // is confined to this file's walk: both callers run one file per
        // worker thread.
        let recording = crate::resolution_deps::record_consulted_names();

        // A scan worker is a thread of its own, so the request's
        // resolved-class cache is not active on it and every model this
        // file's receivers reach would be merged and synthesized from
        // scratch, once per candidate file.
        let _resolved_classes_guard =
            crate::virtual_members::with_active_resolved_class_cache(&self.resolved_class_cache);
        let _parse_cache_guard = crate::parser::with_parse_cache_arc(Arc::clone(content));
        let file_ctx = self.file_context(uri);

        // Build variable scopes once, then resolve every access while those
        // snapshots are hot.  A file whose accesses all have a `$this` or
        // static receiver never needs them, which is most of the
        // scope-building cost of a workspace scan.
        let _scope_guard =
            crate::type_engine::variable::forward_walk::with_diagnostic_scope_cache();
        let needs_variable_scopes = access_indices.iter().any(|&span_index| {
            let SymbolKind::MemberAccess { subject_text, .. } = &symbol_map.spans[span_index].kind
            else {
                return false;
            };
            let subject = subject_text.as_str(source).trim_start();
            subject.starts_with('$') && !subject.starts_with("$this")
        });
        if needs_variable_scopes {
            let class_loader = self.class_loader(&file_ctx);
            let function_loader = self.function_loader(&file_ctx);
            let constant_loader = self.constant_loader(&file_ctx);
            let config_resolver = |key: &str| self.resolve_config_type(key);
            let trans_resolver = |key: &str| self.resolve_trans_type(key);
            let loaders = crate::type_engine::resolver::Loaders {
                function_loader: Some(&function_loader),
                constant_loader: Some(&constant_loader),
                config_resolver: Some(&config_resolver),
                trans_resolver: Some(&trans_resolver),
            };
            match walk {
                // Only the bodies holding the accesses being searched for:
                // a candidate file holds an access or two out of dozens of
                // methods, and walking the rest answers nothing.
                ReceiverWalk::Names(_) => {
                    let scope_offsets: Vec<u32> = access_indices
                        .iter()
                        .map(|&span_index| symbol_map.spans[span_index].start)
                        .collect();
                    crate::type_engine::variable::forward_walk::build_diagnostic_scopes_for_offsets(
                        content,
                        &file_ctx.classes,
                        &class_loader,
                        Some(self),
                        loaders,
                        Some(&self.resolved_class_cache),
                        &scope_offsets,
                    );
                }
                // Every body, since a warm-up has no particular access in
                // mind and its entry has to answer for all of them.
                ReceiverWalk::WholeFile => {
                    crate::type_engine::variable::forward_walk::build_diagnostic_scopes(
                        content,
                        &file_ctx.classes,
                        &class_loader,
                        Some(self),
                        loaders,
                        Some(&self.resolved_class_cache),
                    );
                }
            }
        }
        // Whether the snapshots now in the cache are this file's own
        // targeted walk.  A targeted walk always leaves the coverage
        // region-based, so coverage still reading as whole-file means an
        // outer walk owns the cache and its regions say nothing about which
        // of this file's offsets were walked.
        let walked_regions_are_ours = needs_variable_scopes
            && !crate::type_engine::variable::forward_walk::scope_coverage_is_whole();

        // The walk above is what costs: a body it entered answers for every
        // access inside it, not just the one that pulled it in.  Resolving
        // those too turns this file's entry from an answer to *this* search
        // into an answer the next search for an unrelated member name can be
        // ruled out by without the file being opened at all, and costs only
        // the resolutions themselves — a fraction of the walk that already
        // ran.  A whole-file walk already asked about every name, so this
        // finds nothing left to add.
        //
        // A name is only added when *every* one of its accesses is
        // answerable here, since a name in `covered` promises the entry
        // holds each of them: a name half-resolved would read back as "no
        // receiver" for the rest and silently drop their references.
        let mut widened: Vec<Atom> = Vec::new();
        for (member, indices) in &symbol_map.member_access_indices {
            if covered.contains(member) {
                continue;
            }
            let answerable = indices.iter().all(|&span_index| {
                settled_receiver_text(symbol_map, span_index).is_some()
                    || (walked_regions_are_ours
                        && crate::type_engine::variable::forward_walk::scope_snapshots_cover(
                            symbol_map.spans[span_index].start,
                        ))
            });
            if answerable {
                widened.push(*member);
                access_indices.extend_from_slice(indices);
            }
        }
        let covered: Vec<Atom> = covered.into_iter().chain(widened).collect();
        access_indices.sort_unstable();
        access_indices.dedup();

        // Isolated rather than shared: a request-wide chain cache (find
        // references, the lens batch) keeps one map active across every
        // candidate file, one after another.  Each file's content is a
        // fresh `Arc<String>` freed once its resolution finishes, and the
        // allocator can hand the next file's content the same address —
        // `chain_cache_key` discriminates files by that address, so a
        // shared map would then serve this file's queries the previous
        // file's cached answer. An isolated map is only ever populated and
        // read within this one file's resolution below.
        let _chain_guard = crate::type_engine::resolver::with_isolated_chain_cache();
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();
        let resolved = carried
            .into_iter()
            .chain(access_indices.into_iter().filter_map(|span_index| {
                let span = &symbol_map.spans[span_index];
                let SymbolKind::MemberAccess {
                    subject_text,
                    is_static,
                    ..
                } = &span.kind
                else {
                    return None;
                };
                let targets: Vec<Atom> = self
                    .resolve_subject_to_fqns(
                        subject_text.as_str(source),
                        *is_static,
                        &file_ctx,
                        span.start,
                        content,
                    )
                    .into_iter()
                    .map(|target| crate::atom::atom(&target))
                    .collect();
                if targets.is_empty() {
                    return None;
                }
                let range = Range::new(position(span.start), position(span.end));
                Some((span_index, range, targets))
            }))
            .collect();
        let mut deps = recording.consulted();
        deps.extend(carried_deps);
        self.cache_resolved_member_file(uri, Arc::clone(symbol_map), covered, resolved, deps)
    }

    /// Resolve every member access in `uri` ahead of any search.
    ///
    /// A search that finds an entry covering the name it is after never
    /// opens the file: it reads the receiver each access resolved to
    /// straight out of the entry.  Doing that work once per file in the
    /// background is what keeps the *first* search of a session from paying
    /// for a type-engine walk of every file that happens to mention the
    /// name.
    ///
    /// Returns whether the file was walked, so a caller driving a whole
    /// workspace can report how much of it it actually had to do.
    pub(crate) fn warm_member_receivers(&self, uri: &str) -> bool {
        if self.skip_reference_index {
            return false;
        }
        let Some(symbol_map) = self.symbol_maps.read().get(uri).cloned() else {
            return false;
        };
        if symbol_map.member_access_indices.is_empty() {
            return false;
        }
        if self
            .resolved_member_file(uri, &symbol_map)
            .is_some_and(|file| file.covers(symbol_map.member_access_indices.keys().copied()))
        {
            return false;
        }
        let Some(content) = self.reference_file_content_arc(uri) else {
            return false;
        };
        let Some(source) = symbol_map.source(&content) else {
            return false;
        };
        let lines = OnceCell::new();
        let position = |offset: u32| {
            lines
                .get_or_init(|| LineIndex::new(&content))
                .position(offset as usize)
        };
        self.resolve_member_receivers(
            uri,
            &symbol_map,
            &content,
            source,
            &position,
            ReceiverWalk::WholeFile,
        );
        true
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
        let mut locations = Vec::new();

        let candidate_keys = member_candidate_keys(target_member, target_is_static, hierarchy);
        let snapshot = self.user_file_symbol_maps_for_reference_keys(&candidate_keys);
        self.begin_request_scan_window(snapshot.len(), "Scanning for member references");

        let member = crate::atom::atom(target_member);
        for (file_uri, symbol_map) in &snapshot {
            self.request_scan_file_done();
            // First pass: name-only check to avoid unnecessary work.
            // When a hierarchy is present (e.g. Laravel), we allow static mismatch.
            let names_the_member = symbol_map.member_access_indices(target_member).iter().any(
                |&idx| match &symbol_map.spans[idx].kind {
                    SymbolKind::MemberAccess { is_static, .. } => {
                        hierarchy.is_some() || *is_static == target_is_static
                    }
                    _ => false,
                },
            );
            // A file whose accesses to the name all settle on a class
            // outside the hierarchy is answered here rather than read.
            let has_member_access_match = names_the_member
                && hierarchy.is_none_or(|hier| {
                    !self.member_accesses_ruled_out(file_uri, symbol_map, &[(member, hier)])
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
                && let Some(classes) = self.shared_classes_for_uri(file_uri)
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

            let file = CandidateFile::new(self, file_uri);

            // Lazily resolved file context — only computed when we need
            // to find a declaration's enclosing class.
            let file_ctx_cell: std::cell::OnceCell<crate::types::FileContext> =
                std::cell::OnceCell::new();

            if has_member_access_match {
                self.push_member_access_matches(
                    &file,
                    symbol_map,
                    &parsed_uri,
                    member,
                    target_is_static,
                    hierarchy,
                    &mut locations,
                );
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
                                if let Some(enclosing) = enclosing
                                    && !hier.contains(self, &enclosing.fqn())
                                {
                                    continue;
                                }
                            }

                            let Some(range) = file.range(span.start, span.end) else {
                                break;
                            };
                            locations.push(Location {
                                uri: parsed_uri.clone(),
                                range,
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
            if include_declaration && let Some(classes) = self.shared_classes_for_uri(file_uri) {
                for class in &classes {
                    if let Some(hier) = declaration_scope.or(hierarchy)
                        && !hier.contains(self, &class.fqn())
                    {
                        continue;
                    }

                    for prop in &class.properties {
                        let prop_name = prop.name.strip_prefix('$').unwrap_or(&prop.name);
                        let target_name = target_member.strip_prefix('$').unwrap_or(target_member);
                        if prop_name == target_name
                            && prop.is_static == target_is_static
                            && prop.name_offset != 0
                        {
                            // `name_offset` points at the `$` sigil while
                            // `prop.name` excludes it, so the range must span
                            // the `$` plus the name (`$name`, not `$nam`).
                            let offset = prop.name_offset;
                            let Some(range) =
                                file.range(offset, offset + 1 + prop.name.len() as u32)
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

    /// What the accesses to `names` in one file resolve to, for a search
    /// that has not necessarily opened the file.
    ///
    /// An entry an earlier search or the background warm-up left behind
    /// answers without the file's text; otherwise the file is read and
    /// walked through [`resolve_member_receivers`](Self::resolve_member_receivers),
    /// which records the answer for the next search.  `None` only when the
    /// file cannot be read.
    pub(super) fn member_receivers_for(
        &self,
        file: &CandidateFile<'_>,
        symbol_map: &Arc<SymbolMap>,
        names: &[Atom],
    ) -> Option<Arc<crate::reference_index::ResolvedMemberFile>> {
        if let Some(warm) = self
            .resolved_member_file(file.uri, symbol_map)
            .filter(|entry| entry.covers(names.iter().copied()))
        {
            return Some(warm);
        }
        let content = file.content()?;
        let source = symbol_map.source(content)?;
        let position = |offset: u32| file.position(offset).unwrap_or_default();
        Some(self.resolve_member_receivers(
            file.uri,
            symbol_map,
            content,
            source,
            &position,
            ReceiverWalk::Names(names),
        ))
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
    #[allow(clippy::too_many_arguments)]
    fn push_member_access_matches(
        &self,
        file: &CandidateFile<'_>,
        symbol_map: &Arc<SymbolMap>,
        parsed_uri: &Url,
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
                let Some(range) = file.range(span.start, span.end) else {
                    return;
                };
                locations.push(Location {
                    uri: parsed_uri.clone(),
                    range,
                });
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
            if targets.iter().any(|fqn| hierarchy.contains(self, fqn)) {
                locations.push(Location {
                    uri: parsed_uri.clone(),
                    range,
                });
            }
        }
    }
}
