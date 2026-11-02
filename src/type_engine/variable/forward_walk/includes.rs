//! The variables an `include`/`require` leaves in the including scope.
//!
//! The included file runs in the scope of the statement that includes it,
//! so what its top level assigns is there once the include has run.  When
//! the path resolves (see
//! [`Backend::resolve_include_path`](crate::Backend::resolve_include_path)),
//! the included file's top level is walked and the variables it assigns
//! replace the including scope's.
//!
//! The walk starts with the variables of the including scope the included
//! file reads, and is memoized on their types, so a file included from
//! many places that hand it the same types is walked once.  A file that
//! includes another, or reaches variables by a name no static reading can
//! see (`extract()`, `$$name`), starts with every variable the including
//! scope holds.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Backend;
use crate::atom::{Atom, atom};
use crate::php_type::PhpType;
use crate::scope_collector::{AccessKind, ScopeBody, collect_scope};
use crate::type_engine::{BoundedInference, BoundedMemo, MemoGuard};
use crate::types::ResolvedType;

use super::*;

/// The variables an included file's top level leaves, with their types.
/// `None` when the file could not be read.
pub(crate) type IncludedLocals = Option<Arc<Vec<(Atom, Vec<ResolvedType>)>>>;

/// The variables an included file's walk starts with, by name and type.
type Seed = Box<[(Atom, Box<[PhpType]>)]>;

type IncludeMemo = BoundedMemo<(PathBuf, Seed), IncludedLocals>;

/// The variables an included file reads from the scope it runs in, in
/// name order.  `None` when it can reach variables no static reading
/// names.
type ReadNames = Option<Arc<[Atom]>>;

type ReadsMemo = HashMap<PathBuf, ReadNames>;

thread_local! {
    /// When `Some`, the variables each included file leaves given the
    /// variables it starts with, for the current request.  A file
    /// included from every function of another would otherwise be walked
    /// once per include.
    static INCLUDE_MEMO: RefCell<Option<IncludeMemo>> = const { RefCell::new(None) };

    /// When `Some`, the [`ReadNames`] of each included file, for the
    /// current request.
    static READS_MEMO: RefCell<Option<ReadsMemo>> = const { RefCell::new(None) };

    /// The files whose top level is being walked.  A file that includes
    /// itself, directly or through another, adds nothing on re-entry.
    static INCLUDE_VISITED: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
}

/// The guard [`with_include_memo`] hands back.
pub(crate) type IncludeMemoGuard = (MemoGuard<IncludeMemo>, MemoGuard<ReadsMemo>);

/// Activate the include memos for the current thread.
pub(crate) fn with_include_memo() -> IncludeMemoGuard {
    (
        crate::type_engine::activate_memo(&INCLUDE_MEMO),
        crate::type_engine::activate_memo(&READS_MEMO),
    )
}

/// How deep a chain of includes of includes is followed.
///
/// Cycles are broken by [`INCLUDE_VISITED`]; this bounds the cost of an
/// acyclic chain, each level of which is a walk of a whole file.
const MAX_INCLUDE_DEPTH: u8 = 8;

/// The path expression of an `include`, `include_once`, `require` or
/// `require_once`.
pub(crate) fn include_path_expr<'b>(construct: &'b Construct<'b>) -> Option<&'b Expression<'b>> {
    match construct {
        Construct::Include(c) => Some(c.value),
        Construct::IncludeOnce(c) => Some(c.value),
        Construct::Require(c) => Some(c.value),
        Construct::RequireOnce(c) => Some(c.value),
        _ => None,
    }
}

/// The variables the file an include's `path` names leaves behind when it
/// runs in `scope`, when the path resolves.
pub(crate) fn include_locals(
    path: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> IncludedLocals {
    let backend = ctx.file.backend?;
    let file = crate::util::uri_to_path(ctx.file.uri?)?;
    let namespace = ctx
        .current_class
        .file_namespace
        .as_ref()
        .map(|ns| ns.as_str());
    let included = backend.resolve_include_path(path, &file, namespace)?;
    let seed: Vec<(Atom, Vec<ResolvedType>)> = match read_names(backend, &included) {
        Some(names) => names
            .iter()
            .filter_map(|name| Some((*name, scope.locals.get(name)?.clone())))
            .filter(|(_, types)| !types.is_empty())
            .collect(),
        None => {
            let mut all: Vec<_> = scope
                .locals
                .variables()
                .filter(|(name, types)| {
                    !types.is_empty()
                        && !crate::scope_collector::AUTO_GLOBALS.contains(&name.as_str())
                })
                .map(|(name, types)| (*name, types.clone()))
                .collect();
            all.sort_unstable_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
            all
        }
    };
    included_locals(backend, &included, &seed)
}

/// Write `locals`, what an include left, into `scope`.
pub(crate) fn apply_include_locals(locals: &[(Atom, Vec<ResolvedType>)], scope: &mut ScopeState) {
    for (name, types) in locals {
        if types.is_empty() {
            scope.set_unknown(name);
        } else {
            scope.set(name, types.clone());
        }
        scope.invalidate_dependent_keys(name);
        scope.invalidate_proofs(name);
    }
}

/// The variables the top level of the file at `path` leaves when it
/// starts with `seed`, memoized and bounded.
fn included_locals(
    backend: &Backend,
    path: &Path,
    seed: &[(Atom, Vec<ResolvedType>)],
) -> IncludedLocals {
    let seed_key: Seed = seed
        .iter()
        .map(|(name, types)| (*name, types.iter().map(|t| t.type_string.clone()).collect()))
        .collect();
    crate::type_engine::memoized_bounded_inference(
        &INCLUDE_MEMO,
        &INCLUDE_VISITED,
        BoundedInference::Include,
        MAX_INCLUDE_DEPTH,
        (path.to_path_buf(), seed_key),
        path.to_path_buf(),
        || walk_included_file(backend, path, seed),
    )
}

/// The [`ReadNames`] of the file at `path`, memoized.
fn read_names(backend: &Backend, path: &Path) -> ReadNames {
    if let Some(found) = READS_MEMO.with(|c| c.borrow().as_ref()?.get(path).cloned()) {
        return found;
    }
    let names = scan_read_names(backend, path);
    READS_MEMO.with(|c| {
        if let Some(memo) = c.borrow_mut().as_mut() {
            memo.insert(path.to_path_buf(), names.clone());
        }
    });
    names
}

/// Every variable the file at `path` reads anywhere, which is more than
/// its top level reads from the including scope, but no less.
fn scan_read_names(backend: &Backend, path: &Path) -> ReadNames {
    let content = backend.get_file_content_arc(&crate::util::path_to_uri(path))?;
    crate::parser::with_parsed_program(&content, "include_reads", |program, content| {
        let statements = program.statements.as_slice();
        // What a nested include reads comes from this scope too.
        if !crate::resolution::include_sites(ScopeBody::Statements(statements)).is_empty() {
            return None;
        }
        let scope = collect_scope(statements, 0, content.len() as u32);
        let mut names = Vec::new();
        for dynamic in &scope.dynamic_scope_accesses {
            names.push(atom(dynamic.name.as_deref()?));
        }
        names.extend(
            scope
                .accesses
                .iter()
                .filter(|access| access.kind != AccessKind::Write)
                .filter(|access| {
                    !crate::scope_collector::AUTO_GLOBALS.contains(&access.name.as_str())
                })
                .map(|access| atom(&access.name)),
        );
        names.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
        names.dedup();
        Some(names.into())
    })
}

/// Walk the top level of the file at `path`, starting with `seed`, and
/// read off the variables it assigns, superglobals aside.
fn walk_included_file(
    backend: &Backend,
    path: &Path,
    seed: &[(Atom, Vec<ResolvedType>)],
) -> IncludedLocals {
    let uri = crate::util::path_to_uri(path);
    // An edit to the included file changes what the including files read.
    crate::type_engine::body_reads::record(&uri, 0);
    let content = backend.get_file_content_arc(&uri)?;
    let file_ctx = backend.file_context(&uri);
    let loaders = backend.file_loaders(&file_ctx);

    // The scope cache and the chain cache both key on offsets alone, and
    // the offsets below belong to another file.  Neither may answer, nor
    // record, anything while this walk runs.
    let _isolated = (
        suspend_diagnostic_scope(),
        crate::type_engine::resolver::with_isolated_chain_cache(),
    );
    let _parse_guard = crate::parser::with_parse_cache_arc(Arc::clone(&content));

    let locals =
        crate::parser::with_parsed_program(&content, "include_locals", |program, content| {
            let flow = WalkFlow::default();
            let ctx = ForwardWalkCtx {
                loaders: loaders.loaders_at(0),
                resolved_class_cache: Some(&backend.resolved_class_cache),
                ..ForwardWalkCtx::new(
                    loaders.file_ctx_at(content, 0),
                    loaders.class_at(0),
                    u32::MAX,
                    &flow,
                )
            };
            let mut start = ScopeState::new();
            seed_superglobals(&mut start);
            for (name, types) in seed {
                start.seed(name, types.clone());
            }
            let mut scope = start.clone();
            let statements: Vec<&Statement<'_>> = program.statements.iter().collect();
            walk_top_level(&statements, &mut scope, &ctx);
            scope
                .locals
                .variables_changed_from(&start.locals)
                .into_iter()
                .filter(|(name, _)| !crate::scope_collector::AUTO_GLOBALS.contains(&name.as_str()))
                .map(|(name, types)| (*name, types.clone()))
                .collect::<Vec<_>>()
        });
    Some(Arc::new(locals))
}
