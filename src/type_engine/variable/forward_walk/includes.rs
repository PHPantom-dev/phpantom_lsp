//! The variables an `include`/`require` leaves in the including scope.
//!
//! The included file runs in the scope of the statement that includes it,
//! so what its top level assigns is there once the include has run.  When
//! the path resolves (see
//! [`Backend::resolve_include_path`](crate::Backend::resolve_include_path)),
//! the included file's top level is walked on a scope of its own and the
//! variables it ends up holding replace the including scope's.
//!
//! The included file is walked without the including scope's variables,
//! so one walk answers for every place that includes it.  A variable it
//! only reads, or assigns from one it only reads, is typed as far as that
//! walk can see.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Backend;
use crate::atom::Atom;
use crate::type_engine::{BoundedInference, BoundedMemo, MemoGuard};
use crate::types::ResolvedType;

use super::*;

/// The variables an included file's top level leaves, with their types.
/// `None` when the file could not be read.
pub(crate) type IncludedLocals = Option<Arc<Vec<(Atom, Vec<ResolvedType>)>>>;

type IncludeMemo = BoundedMemo<PathBuf, IncludedLocals>;

thread_local! {
    /// When `Some`, the variables each included file leaves, for the
    /// current request.  A file included from every function of another
    /// would otherwise be walked once per include.
    static INCLUDE_MEMO: RefCell<Option<IncludeMemo>> = const { RefCell::new(None) };

    /// The files whose top level is being walked.  A file that includes
    /// itself, directly or through another, adds nothing on re-entry.
    static INCLUDE_VISITED: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
}

/// The guard [`with_include_memo`] hands back.
pub(crate) type IncludeMemoGuard = MemoGuard<IncludeMemo>;

/// Activate the include memo for the current thread.
pub(crate) fn with_include_memo() -> IncludeMemoGuard {
    crate::type_engine::activate_memo(&INCLUDE_MEMO)
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

/// The variables the file an include's `path` names leaves behind, when
/// the path resolves.
pub(crate) fn include_locals(path: &Expression<'_>, ctx: &ForwardWalkCtx<'_>) -> IncludedLocals {
    let backend = ctx.file.backend?;
    let file = crate::util::uri_to_path(ctx.file.uri?)?;
    let namespace = ctx
        .current_class
        .file_namespace
        .as_ref()
        .map(|ns| ns.as_str());
    let included = backend.resolve_include_path(path, &file, namespace)?;
    included_locals(backend, &included)
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

/// The variables the top level of the file at `path` leaves, memoized and
/// bounded.
fn included_locals(backend: &Backend, path: &Path) -> IncludedLocals {
    crate::type_engine::memoized_bounded_inference(
        &INCLUDE_MEMO,
        &INCLUDE_VISITED,
        BoundedInference::Include,
        MAX_INCLUDE_DEPTH,
        path.to_path_buf(),
        path.to_path_buf(),
        || walk_included_file(backend, path),
    )
}

/// Walk the top level of the file at `path` and read off the variables it
/// leaves, superglobals aside.
fn walk_included_file(backend: &Backend, path: &Path) -> IncludedLocals {
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
            let scope = top_level_scope_for_globals(program.statements.iter(), &ctx);
            scope
                .locals
                .variables()
                .filter(|(name, _)| !crate::scope_collector::AUTO_GLOBALS.contains(&name.as_str()))
                .map(|(name, types)| (*name, types.clone()))
                .collect::<Vec<_>>()
        });
    Some(Arc::new(locals))
}
