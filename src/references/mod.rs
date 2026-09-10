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
mod members;
mod variables;

pub(crate) use members::MemberDeclarationReferenceQuery;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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
        if self.is_blade_file(uri)
            && let Some(content) = self.blade_virtual_content.read().get(uri)
        {
            return Some(content.clone());
        }
        self.get_file_content(uri)
    }

    pub(super) fn reference_file_content_arc(&self, uri: &str) -> Option<Arc<String>> {
        if self.is_blade_file(uri)
            && let Some(content) = self.blade_virtual_content.read().get(uri)
        {
            return Some(Arc::new(content.clone()));
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

fn sort_locations_for_references(locations: &mut Vec<Location>) {
    locations.sort_by(|a, b| {
        a.uri
            .as_str()
            .cmp(b.uri.as_str())
            .then(a.range.start.line.cmp(&b.range.start.line))
            .then(a.range.start.character.cmp(&b.range.start.character))
    });
    locations.dedup();
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
    hierarchy: Option<&HashSet<String>>,
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

/// Recursively collect all `.php` files under a workspace root,
/// respecting `.gitignore` rules (including nested and global
/// gitignore files).
///
/// Used by Find References which walks the entire workspace root.
/// Unlike `classmap_scanner`'s PSR-4 walkers, this uses the `ignore`
/// crate's [`ignore::WalkBuilder`] so that generated/cached directories
/// listed in `.gitignore` (e.g. `storage/framework/views/`,
/// `var/cache/`, `node_modules/`) are automatically skipped.
///
/// All known vendor directories are always skipped regardless of
/// `.gitignore` content, since some projects commit their vendor
/// directory.  `vendor_dir_paths` contains absolute paths of all
/// known vendor directories (one per subproject in monorepo mode).
///
/// Hidden files and directories are skipped by default (handled by
/// the `ignore` crate).
pub(crate) fn collect_php_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
    filters: &std::sync::Arc<crate::classmap_scanner::IndexFilters>,
) -> Vec<PathBuf> {
    let mut result = Vec::new();
    visit_workspace_files_gitignore(root, vendor_dir_paths, filters, |path| {
        if filters.is_php_file(path) {
            result.push(path.to_path_buf());
        }
    });
    result
}

/// Collect the PHP and schema-free YAML/XML inputs used by the full workspace
/// index in one `.gitignore`-aware walk.
pub(crate) fn collect_workspace_index_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
    filters: &std::sync::Arc<crate::classmap_scanner::IndexFilters>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut php_files = Vec::new();
    let mut resource_files = Vec::new();
    visit_workspace_files_gitignore(root, vendor_dir_paths, filters, |path| {
        if filters.is_php_file(path) {
            php_files.push(path.to_path_buf());
        } else if crate::resource_navigation::is_resource_path(path) {
            resource_files.push(path.to_path_buf());
        }
    });
    (php_files, resource_files)
}

fn visit_workspace_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
    filters: &std::sync::Arc<crate::classmap_scanner::IndexFilters>,
    mut visit: impl FnMut(&Path),
) {
    use ignore::WalkBuilder;

    let vendor_paths_owned: Vec<PathBuf> = vendor_dir_paths.to_vec();
    let filter_excludes = std::sync::Arc::clone(filters);

    let walker = WalkBuilder::new(root)
        // Respect .gitignore, .git/info/exclude, global gitignore
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        // Skip hidden files/dirs (.git, .idea, etc.)
        .hidden(true)
        // Read parent .gitignore files
        .parents(true)
        // Also respect .ignore files (ripgrep convention)
        .ignore(true)
        // Always skip vendor directories (even if not gitignored) and
        // `[indexing] exclude` matches
        .filter_entry(move |entry| {
            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            if is_dir && vendor_paths_owned.iter().any(|vp| vp == entry.path()) {
                return false;
            }
            !filter_excludes.is_excluded_entry(entry.path(), is_dir)
        })
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() {
            visit(path);
        }
    }
}

/// Push a location only if it is not already present (deduplication).
pub(crate) fn push_unique_location(
    locations: &mut Vec<Location>,
    uri: &Url,
    start: Position,
    end: Position,
) {
    let already_present = locations.iter().any(|l| {
        l.uri == *uri
            && l.range.start.line == start.line
            && l.range.start.character == start.character
    });
    if !already_present {
        locations.push(Location {
            uri: uri.clone(),
            range: Range { start, end },
        });
    }
}

#[cfg(test)]
mod tests;
