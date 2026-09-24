//! Resolving the receivers of a file's member accesses, and remembering
//! the answers.
//!
//! A member search has to know which class each `$x->name()` in a
//! candidate file is called on before it can tell a reference from an
//! unrelated access that shares the name.  Working that out walks the type
//! engine over the file, so the answers are recorded per file in the
//! reference index and re-read by every later search (and by the
//! reference-count lens batch); the receivers a file settles by itself
//! (`$this`, `self`, `static`, `parent`, a static access on a written
//! class name) are answered from the symbol map without opening it.

use super::*;

use std::cell::OnceCell;

use tower_lsp::lsp_types::Range;

use crate::atom::Atom;
use crate::references::member_scope::MemberScope;
use crate::symbol_map::SymbolMap;
use crate::symbol_map::{SelfStaticParentKind, SubjectText, SymbolKind};
use crate::text_position::LineIndex;

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
pub(super) fn settled_receiver_text(symbol_map: &SymbolMap, span_index: usize) -> Option<String> {
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
pub(super) enum ReceiverWalk<'a> {
    /// Walk the bodies holding the accesses to these member names, then
    /// widen the entry to whatever else those bodies turned out to answer
    /// for.
    Names(&'a [Atom]),
    /// Walk every body, so the entry answers for every access in the file
    /// and no later search has to open it.
    WholeFile,
}

impl Backend {
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
    pub(super) fn resolve_member_receivers(
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
}
