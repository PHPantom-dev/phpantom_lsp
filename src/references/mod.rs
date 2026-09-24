//! Find References (`textDocument/references`).
//!
//! When the user invokes "Find All References" on a symbol, the LSP
//! collects every occurrence of that symbol across the project.
//!
//! **Same-file references** are answered from the precomputed
//! [`SymbolMap`] — we iterate all spans and collect those that match
//! the symbol under the cursor.
//!
//! **Cross-file references** iterate every `SymbolMap` stored in
//! `self.symbol_maps` (one per opened / parsed file).  For files that
//! are in the workspace but have not been opened yet, we lazily parse
//! them on demand (via the fqn_uri_index, PSR-4, and workspace scan).
//!
//! **Variable references** (including `$this`) are strictly scoped to
//! the enclosing function / method / closure body within the current
//! file.
//!
//! **Member references** (methods, properties, constants) are filtered
//! by the class hierarchy of the target member.  When the user triggers
//! "Find References" on `MyClass::save()`, only accesses where the
//! subject resolves to a class in the same inheritance tree are returned.
//! Accesses on unrelated classes that happen to have a member with the
//! same name are excluded.
//!
//! The per-symbol-kind finders live in sibling submodules
//! ([`dispatch`], [`variables`], [`classes`], [`members`],
//! [`functions`]); this module retains the shared symbol-map snapshot
//! helpers, the workspace-indexing pipeline, and the free helpers those
//! finders share.

mod classes;
mod covers;
mod dispatch;
mod functions;
mod member_scope;
mod members;
mod receivers;
mod variables;

pub(crate) use members::MemberDeclarationReferenceQuery;
use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::Backend;
use crate::reference_index::ReferenceIndexKey;
use crate::symbol_map::SymbolMap;
use crate::util::strip_fqn_prefix;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReferenceSearchMode {
    References,
    Rename,
}

impl ReferenceSearchMode {
    fn include_declaring_interfaces(self) -> bool {
        matches!(self, ReferenceSearchMode::References)
    }
}

impl Backend {
    /// Snapshot all symbol maps for user (non-vendor, non-stub) files.
    ///
    /// Ensures the workspace is indexed first, then returns a cloned
    /// snapshot of every symbol map whose URI does not fall under the
    /// vendor directory or the internal stub scheme.  All four cross-file
    /// reference scanners use this to restrict results to user code.
    pub(crate) fn user_file_symbol_maps(&self) -> Vec<(String, Arc<SymbolMap>)> {
        self.ensure_workspace_index_ready_for_request();
        self.user_file_symbol_maps_matching(None)
    }

    /// Like [`user_file_symbol_maps`], but never blocks on (or
    /// triggers) workspace indexing — it snapshots whatever is already
    /// parsed. For callers that can run inside the workspace index
    /// itself (the Laravel config-tree build is reached from
    /// `find_or_load_class` and from the blade injected-vars refresh),
    /// where ensuring the index would re-enter its own lock.
    pub(crate) fn user_file_symbol_maps_nonblocking(&self) -> Vec<(String, Arc<SymbolMap>)> {
        self.user_file_symbol_maps_matching(None)
    }

    pub(crate) fn user_file_symbol_maps_for_reference_keys(
        &self,
        keys: &[ReferenceIndexKey],
    ) -> Vec<(String, Arc<SymbolMap>)> {
        self.ensure_workspace_index_ready_for_request();
        let candidate_uris = self.reference_candidate_uris_for_keys(keys);
        self.user_file_symbol_maps_matching(candidate_uris.as_ref())
    }

    /// Like [`user_file_symbol_maps_for_reference_keys`], but never
    /// blocks on (or triggers) workspace indexing — it snapshots
    /// whatever is already parsed.  For callers on hot paths
    /// (`update_ast`) where waiting on the index lock would stall
    /// typing or deadlock a parse worker.  Before the reference index
    /// is built the candidate filter is unavailable, so this falls
    /// back to every parsed user file.
    pub(crate) fn user_file_symbol_maps_for_reference_keys_nonblocking(
        &self,
        keys: &[ReferenceIndexKey],
    ) -> Vec<(String, Arc<SymbolMap>)> {
        let candidate_uris = self.reference_candidate_uris_for_keys(keys);
        self.user_file_symbol_maps_matching(candidate_uris.as_ref())
    }

    fn user_file_symbol_maps_matching(
        &self,
        candidate_uris: Option<&HashSet<Arc<str>>>,
    ) -> Vec<(String, Arc<SymbolMap>)> {
        let vendor_prefixes = self.workspace.vendor_uri_prefixes.lock().clone();

        let maps = self.symbol_maps.read();
        maps.iter()
            .filter(|(uri, _)| {
                candidate_uris.is_none_or(|uris| uris.contains(uri.as_str()))
                    && !uri.starts_with("phpantom-stub://")
                    && !uri.starts_with("phpantom-stub-fn://")
                    && !vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str()))
            })
            .map(|(uri, map)| (uri.clone(), Arc::clone(map)))
            .collect()
    }

    pub(super) fn reference_file_content(&self, uri: &str) -> Option<String> {
        self.reference_file_content_arc(uri)
            .map(|content| String::clone(&content))
    }

    /// Read content in the coordinate space used by the file's symbol map.
    /// Blade maps describe generated PHP; locations are translated for the client later.
    pub(crate) fn reference_file_content_arc(&self, uri: &str) -> Option<Arc<String>> {
        if self.is_blade_file(uri)
            && let Some(content) = self.blade_virtual_php_arc(uri)
        {
            return Some(content);
        }
        self.get_file_content_arc(uri)
    }

    /// Enter the per-file scan window (80..100) of the current
    /// request's progress bar and register `total` files to scan.
    /// No-op when no progress sink is attached.
    pub(crate) fn begin_request_scan_window(&self, total: usize, label: &str) {
        if let Some(state) = self.request_progress.as_deref() {
            state.set_scope(80, 100, label);
            state.add_total(total as u64);
        }
    }

    /// Record one scanned file in the current request's progress bar.
    pub(crate) fn request_scan_file_done(&self) {
        if let Some(state) = self.request_progress.as_deref() {
            state.add_done(1);
        }
    }

    /// Run one Find References search over the files the reference index
    /// names as candidates for `candidate_keys`, and return what it found
    /// in reporting order.
    ///
    /// This owns what every per-kind search shares: the candidate snapshot,
    /// the progress window, one [`CandidateFile`] per file, and the final
    /// sort.  `scan` reports the matches in one file.  Beyond two files the
    /// scan runs on a worker pool, the way the reference-count lens batch
    /// does, since resolving a member access's receiver or a `new`
    /// expression's class walks the type engine over the file.
    pub(super) fn scan_reference_candidates(
        &self,
        candidate_keys: &[ReferenceIndexKey],
        progress_label: &str,
        scan: impl Fn(&CandidateFile<'_>, &Arc<SymbolMap>, &mut Vec<Location>) + Sync,
    ) -> Vec<Location> {
        let snapshot = self.user_file_symbol_maps_for_reference_keys(candidate_keys);
        let mut locations =
            self.scan_candidate_snapshot(snapshot, progress_label, |file, symbol_map| {
                let mut found = Vec::new();
                scan(file, symbol_map, &mut found);
                found
            });
        sort_locations_for_references(&mut locations);
        locations
    }

    /// Run `scan` over every file of a candidate snapshot and concatenate
    /// what it reports.
    ///
    /// The loop under every search: one progress window, one
    /// [`CandidateFile`] per file, serial for two files or fewer and a
    /// worker pool beyond that.  A search that needs more than a flat
    /// location list (the lens batch attributes each hit to one of several
    /// queries) builds its own snapshot and folds the results itself.
    pub(super) fn scan_candidate_snapshot<T: Send>(
        &self,
        snapshot: Vec<(String, Arc<SymbolMap>)>,
        progress_label: &str,
        scan: impl Fn(&CandidateFile<'_>, &Arc<SymbolMap>) -> Vec<T> + Sync,
    ) -> Vec<T> {
        self.begin_request_scan_window(snapshot.len(), progress_label);

        let scan_file = |(uri, symbol_map): &(String, Arc<SymbolMap>)| {
            self.request_scan_file_done();
            let file = CandidateFile::new(self, uri);
            scan(&file, symbol_map)
        };
        let mut results = Vec::new();
        if snapshot.len() <= 2 {
            for entry in &snapshot {
                results.extend(scan_file(entry));
            }
        } else {
            let found =
                crate::parallel::map_indexed("reference-scan", snapshot.len(), |_, index| {
                    let found = scan_file(&snapshot[index]);
                    (!found.is_empty()).then_some(found)
                });
            for (_, found) in found {
                results.extend(found);
            }
        }
        results
    }
}

/// What one file needs to turn a class, function or constant reference
/// span into a fully-qualified name.
///
/// The `use` map is loaded on the first span that needs it: most spans
/// are answered by the name resolver alone, and most files carry no span
/// the search is interested in at all.
pub(super) struct SpanFqnResolver<'a> {
    backend: &'a Backend,
    file_uri: &'a str,
    /// The file's first namespace, which a name the resolver does not
    /// track is resolved against.
    pub(super) namespace: Option<String>,
    resolved_names: Option<Arc<crate::names::OwnedResolvedNames>>,
    use_map: std::cell::OnceCell<std::collections::HashMap<String, String>>,
}

impl<'a> SpanFqnResolver<'a> {
    pub(super) fn new(backend: &'a Backend, file_uri: &'a str) -> Self {
        Self {
            backend,
            file_uri,
            namespace: backend.first_file_namespace(file_uri),
            resolved_names: backend.resolved_names.read().get(file_uri).cloned(),
            use_map: std::cell::OnceCell::new(),
        }
    }

    /// The fully-qualified name the span at `span_start` refers to.
    ///
    /// A name written fully qualified already is one; otherwise the name
    /// resolver's answer for that offset wins, and failing that (a
    /// docblock-sourced reference, which the resolver does not track) the
    /// file's `use` map and namespace decide.
    pub(super) fn fqn(&self, name: &str, is_fqn: bool, span_start: u32) -> String {
        if is_fqn {
            return name.to_string();
        }
        if let Some(fqn) = self
            .resolved_names
            .as_deref()
            .and_then(|rn| rn.get(span_start))
        {
            return fqn.to_string();
        }
        let use_map = self.use_map.get_or_init(|| {
            self.backend
                .file_imports
                .read()
                .get(self.file_uri)
                .cloned()
                .unwrap_or_default()
        });
        Backend::resolve_to_fqn(name, use_map, &self.namespace)
    }
}

/// Normalise a class FQN: strip leading `\` if present.
pub(super) fn normalize_fqn(fqn: &str) -> String {
    strip_fqn_prefix(fqn).to_string()
}

/// [`normalize_fqn`] plus ASCII case folding, for the sets that decide
/// whether two spellings name the same class.  PHP resolves class names
/// case-insensitively, so `App\WIDGET` and `App\Widget` have to compare
/// equal; only use this for membership tests, never for a name that is
/// shown to the user or written back into source.
pub(super) fn fold_class_fqn(fqn: &str) -> String {
    strip_fqn_prefix(fqn).to_ascii_lowercase()
}

pub(super) fn static_call_root(
    expr: &crate::type_engine::subject_expr::SubjectExpr,
) -> Option<(&str, &str)> {
    match expr {
        crate::type_engine::subject_expr::SubjectExpr::CallExpr { callee, .. } => {
            static_call_root(callee)
        }
        crate::type_engine::subject_expr::SubjectExpr::MethodCall { base, .. } => {
            static_call_root(base)
        }
        crate::type_engine::subject_expr::SubjectExpr::StaticMethodCall { class, method } => {
            Some((class.as_str(), method.as_str()))
        }
        _ => None,
    }
}

pub(super) fn is_laravel_builder_static_entrypoint(method_name: &str) -> bool {
    matches!(
        method_name.to_ascii_lowercase().as_str(),
        "query"
            | "newquery"
            | "where"
            | "wherein"
            | "wherenull"
            | "wherenotnull"
            | "orderby"
            | "select"
            | "with"
            | "without"
            | "latest"
            | "oldest"
    )
}

/// Whether a member name is the PHP constructor (`__construct`).
///
/// PHP method names are case-insensitive, so `__CONSTRUCT` matches too.
pub(super) fn is_constructor_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("__construct")
}

/// Put the locations a Find References answer carries into the order an
/// editor lists them: by file, then by position within it, with the
/// locations that start at the same place collapsed to the first one found.
///
/// This is the one place a search de-duplicates. Checking every hit
/// against every location found so far is quadratic in the hit count, and
/// a widely used name has thousands of them.
pub(crate) fn sort_locations_for_references(locations: &mut Vec<Location>) {
    locations.sort_by(|a, b| {
        a.uri
            .as_str()
            .cmp(b.uri.as_str())
            .then(a.range.start.line.cmp(&b.range.start.line))
            .then(a.range.start.character.cmp(&b.range.start.character))
    });
    locations.dedup_by(|later, earlier| {
        later.uri == earlier.uri && later.range.start == earlier.range.start
    });
}

/// Check whether a resolved class name matches the target FQN.
///
/// Two names match if their fully-qualified forms are equal, or if both
/// are unqualified and their short names match.  PHP resolves class names
/// case-insensitively, so `WIDGET` and `Widget` are the same class and all
/// the comparisons here fold case.
pub(super) fn class_names_match(resolved: &str, target: &str, target_short: &str) -> bool {
    if resolved.eq_ignore_ascii_case(target) {
        return true;
    }
    if !resolved.contains('\\') && !target.contains('\\') {
        return resolved.eq_ignore_ascii_case(target_short);
    }
    // When the resolved name is unqualified but the target is
    // namespace-qualified, the resolved name might be a short-name
    // reference to the target class (e.g. `Request` referencing
    // `Illuminate\Http\Request` via a `use` import that was not
    // tracked in the resolved-names map).  Accept the match only
    // when the short names agree.
    //
    // The reverse (resolved is qualified, target is unqualified) is
    // NOT accepted: `App\Helper` is a different class from a global
    // `Helper`, so matching by short name alone would produce false
    // positives.
    if !resolved.contains('\\') && target.contains('\\') {
        return resolved.eq_ignore_ascii_case(target_short);
    }
    false
}

pub(super) fn class_candidate_keys(target: &str, target_short: &str) -> Vec<ReferenceIndexKey> {
    symbol_candidate_names(target, target_short)
        .into_iter()
        .map(ReferenceIndexKey::class_owned)
        .collect()
}

pub(super) fn function_candidate_keys(target: &str, target_short: &str) -> Vec<ReferenceIndexKey> {
    symbol_candidate_names(target, target_short)
        .into_iter()
        .map(ReferenceIndexKey::function_owned)
        .collect()
}

pub(super) fn constant_candidate_keys(target: &str, target_short: &str) -> Vec<ReferenceIndexKey> {
    symbol_candidate_names(target, target_short)
        .into_iter()
        .map(ReferenceIndexKey::Constant)
        .collect()
}

fn symbol_candidate_names(target: &str, target_short: &str) -> Vec<String> {
    let mut keys = vec![
        strip_fqn_prefix(target).to_string(),
        strip_fqn_prefix(target_short).to_string(),
    ];
    keys.sort();
    keys.dedup();
    keys
}

pub(super) fn member_candidate_keys(
    target_member: &str,
    target_is_static: bool,
    hierarchy: Option<&member_scope::MemberScope>,
) -> Vec<ReferenceIndexKey> {
    let mut keys = vec![ReferenceIndexKey::Member {
        name: target_member.to_string(),
        is_static: target_is_static,
    }];
    if hierarchy.is_some() {
        keys.push(ReferenceIndexKey::Member {
            name: target_member.to_string(),
            is_static: !target_is_static,
        });
    }
    keys
}

/// A candidate file of a reference search, read only once a hit in it
/// needs the text.
///
/// Most candidate files turn out to hold nothing, so reading one up front
/// is wasted. Once one is read, every hit in it shares one line table:
/// converting each hit with [`offset_to_position`] rescans the file from
/// the top, which makes a busy file quadratic in its own size.
///
/// [`offset_to_position`]: crate::text_position::offset_to_position
pub(super) struct CandidateFile<'a> {
    backend: &'a Backend,
    uri: &'a str,
    url: std::cell::OnceCell<Option<Url>>,
    content: std::cell::OnceCell<Option<Arc<String>>>,
    line_starts: std::cell::OnceCell<Vec<usize>>,
}

impl<'a> CandidateFile<'a> {
    pub(super) fn new(backend: &'a Backend, uri: &'a str) -> Self {
        Self {
            backend,
            uri,
            url: std::cell::OnceCell::new(),
            content: std::cell::OnceCell::new(),
            line_starts: std::cell::OnceCell::new(),
        }
    }

    /// The URI the symbol map is keyed by.
    pub(super) fn uri(&self) -> &'a str {
        self.uri
    }

    /// The file's URI as a location carries it, or `None` when it does not
    /// parse.
    pub(super) fn url(&self) -> Option<Url> {
        self.url.get_or_init(|| Url::parse(self.uri).ok()).clone()
    }

    /// The location between two byte offsets in [`Self::content`], or
    /// `None` when the file cannot be read or its URI does not parse.
    pub(super) fn location(&self, start: u32, end: u32) -> Option<Location> {
        let range = self.range(start, end)?;
        Some(Location {
            uri: self.url()?,
            range,
        })
    }

    /// The text Find References reads the file as (the virtual PHP of a
    /// Blade template), or `None` when it cannot be read.
    pub(super) fn content(&self) -> Option<&Arc<String>> {
        self.content
            .get_or_init(|| self.backend.reference_file_content_arc(self.uri))
            .as_ref()
    }

    /// The position of a byte offset in [`Self::content`].
    pub(super) fn position(&self, offset: u32) -> Option<Position> {
        let content = self.content()?;
        let line_starts = self
            .line_starts
            .get_or_init(|| crate::text_position::line_starts(content));
        Some(crate::text_position::position_in(
            content,
            line_starts,
            offset as usize,
        ))
    }

    /// The range between two byte offsets in [`Self::content`].
    pub(super) fn range(&self, start: u32, end: u32) -> Option<Range> {
        Some(Range::new(self.position(start)?, self.position(end)?))
    }
}

/// Record a location. Duplicates are collapsed once, by
/// [`sort_locations_for_references`], when the search is done.
pub(crate) fn push_location(
    locations: &mut Vec<Location>,
    uri: &Url,
    start: Position,
    end: Position,
) {
    locations.push(Location {
        uri: uri.clone(),
        range: Range { start, end },
    });
}

#[cfg(test)]
mod tests;
