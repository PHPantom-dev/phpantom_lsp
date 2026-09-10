/// Variable type resolution — routing layer and shared helpers.
///
/// All variable type resolution is performed by the forward walker in
/// [`super::forward_walk`].  This module provides the public entry
/// points ([`resolve_variable_types`]) that callers across the crate
/// use, a diagnostic-scope-cache fast path, and shared helper functions
/// (template substitution, array shape merging, pass-by-reference
/// seeding, abstract-method parameter resolution) that the forward
/// walker delegates to.
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{Atom, atom, bytes_to_str, last_segment, literal_bytes_to_str};
use crate::docblock;
use crate::parser::{extract_hint_type, with_parsed_program};
use crate::php_type::{
    LiteralValue, PhpType, ShapeEntry, TypeKind, is_decimal_int_array_key, is_keyword_type,
    runtime_shape_keys,
};
use crate::types::{ClassInfo, ParameterInfo, ResolvedType};

use crate::Backend;
use crate::type_engine::call_resolution::OutParamCallee;
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};

// ─── Re-entry guards ───────────────────────────────────────────────────────
//
// Variable resolution can re-enter itself while building the top-level
// scope for `global` keyword resolution.  The cycle is:
//
//   resolve_variable_types → resolve_variable_in_statements
//   → walk_top_level_for_globals → RHS resolution → resolve_variable_types
//
// Two guards break this cycle at different levels:
//
// Guard 1 (`BUILDING_TOP_LEVEL_SCOPE`): prevents re-entrant top-level
//   scope construction for the same file.
// Guard 2 (`RESOLVING_VARS`): prevents re-entrant resolution of the
//   exact same variable query.

thread_local! {
    /// Source content addresses currently building a top-level scope
    /// via [`walk_top_level_for_globals`](super::forward_walk::walk_top_level_for_globals).
    /// Keyed by `content.as_ptr() as usize`: re-entry within one call
    /// tree always borrows the same slice, so pointer identity marks
    /// exactly the cycle we want to break.  Two independent copies of
    /// the same text are separate queries and must not block one
    /// another, which pointer identity also gets right.
    static BUILDING_TOP_LEVEL_SCOPE: RefCell<HashSet<usize>> =
        RefCell::new(HashSet::new());

    /// Variable resolution queries currently in progress on this
    /// thread.  Keyed by [`VarQueryKey`] so that only the exact same
    /// query is suppressed.
    static RESOLVING_VARS: RefCell<HashSet<VarQueryKey>> =
        RefCell::new(HashSet::new());

    /// When `Some`, memoises what [`resolve_variable_types`] computed
    /// the hard way, keyed by [`VarQueryKey`].  Activated with the rest
    /// of the request-scoped type-engine memos, so it lives exactly as
    /// long as one request / one file's diagnostic pass.
    ///
    /// Without it, a body is walked from its first statement once per
    /// *ask*, not once per question: deciding whether a guard branch
    /// exits resolves the receiver of every call it holds, and each of
    /// those receivers sends the walker over the whole body again.  On a
    /// long method whose branches each hold a chained call — the shape
    /// `phpstan-src`'s `AnalyseCommand::execute` has — the same handful
    /// of `(variable, offset)` questions get asked hundreds of times.
    static VAR_TYPE_MEMO: RefCell<Option<HashMap<VarQueryKey, Vec<ResolvedType>>>> =
        const { RefCell::new(None) };

    #[cfg(test)]
    static TEST_SCOPE_CACHE_HITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_test_scope_cache_hits() {
    TEST_SCOPE_CACHE_HITS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn test_scope_cache_hits() -> usize {
    TEST_SCOPE_CACHE_HITS.with(std::cell::Cell::get)
}

/// What identifies one "what is the type of `$var` here?" question: the
/// source it is asked of, the hash of the un-prefixed variable name, the
/// offset, and the class the question is asked in.
///
/// The source is `(pointer, length)` rather than the pointer alone: the
/// memo outlives any single query, so a freed buffer whose address is
/// handed straight back to a different one would otherwise read as the
/// same source.
type VarQueryKey = ((usize, usize), u64, u32, Atom);

/// Build the key identifying one variable query.
///
/// Callers spell the same variable both with and without the `$`
/// prefix, so the name is normalised before hashing.  It is hashed
/// rather than interned: this runs on every variable resolution, and
/// `atom` would take the global interner lock and allocate for the
/// unprefixed spelling.
fn var_query_key(
    content: &str,
    var_name: &str,
    cursor_offset: u32,
    class_name: Atom,
) -> VarQueryKey {
    let mut hasher = DefaultHasher::new();
    var_name
        .strip_prefix('$')
        .unwrap_or(var_name)
        .hash(&mut hasher);
    (
        (content.as_ptr() as usize, content.len()),
        hasher.finish(),
        cursor_offset,
        class_name,
    )
}

/// RAII guard that clears [`VAR_TYPE_MEMO`] when the pass that installed
/// it ends.  Nested activation is a no-op, so an inner pass cannot
/// discard the entries an outer one is still relying on.
pub(crate) struct VarTypeMemoGuard {
    owns: bool,
}

impl Drop for VarTypeMemoGuard {
    fn drop(&mut self) {
        if self.owns {
            VAR_TYPE_MEMO.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }
}

/// Activate the variable-type memo for the current thread.
pub(crate) fn with_var_type_memo() -> VarTypeMemoGuard {
    let already_active = VAR_TYPE_MEMO.with(|cell| cell.borrow().is_some());
    if already_active {
        return VarTypeMemoGuard { owns: false };
    }
    VAR_TYPE_MEMO.with(|cell| {
        *cell.borrow_mut() = Some(HashMap::new());
    });
    VarTypeMemoGuard { owns: true }
}

/// RAII guard for [`BUILDING_TOP_LEVEL_SCOPE`].
struct TopLevelScopeGuard {
    key: usize,
}

impl Drop for TopLevelScopeGuard {
    fn drop(&mut self) {
        BUILDING_TOP_LEVEL_SCOPE.with(|set| {
            set.borrow_mut().remove(&self.key);
        });
    }
}

/// Try to acquire the top-level scope build guard for `content`.
/// Returns `Some(guard)` on success, `None` when the same file is
/// already mid-construction (re-entry detected).
fn try_acquire_top_level_guard(content: &str) -> Option<TopLevelScopeGuard> {
    let key = content.as_ptr() as usize;
    let inserted = BUILDING_TOP_LEVEL_SCOPE.with(|set| set.borrow_mut().insert(key));
    if inserted {
        Some(TopLevelScopeGuard { key })
    } else {
        None
    }
}

/// RAII guard for [`RESOLVING_VARS`].
struct ResolvingVarGuard {
    key: VarQueryKey,
}

impl Drop for ResolvingVarGuard {
    fn drop(&mut self) {
        RESOLVING_VARS.with(|set| {
            set.borrow_mut().remove(&self.key);
        });
    }
}

/// Try to acquire the variable resolution guard for `key`.
/// Returns `Some(guard)` on success, `None` on re-entry.
///
/// Only the queries on the current stack (a handful) share the set, so
/// the hashed name in the key cannot realistically collide.
fn try_acquire_var_guard(key: VarQueryKey) -> Option<ResolvingVarGuard> {
    let inserted = RESOLVING_VARS.with(|set| set.borrow_mut().insert(key));
    if inserted {
        Some(ResolvingVarGuard { key })
    } else {
        None
    }
}

/// Build a [`VarClassStringResolver`] closure from a [`VarResolutionCtx`].
///
/// The returned closure resolves a variable name (e.g. `"$requestType"`)
/// to the fully-qualified names of the classes it holds as class-string
/// values by delegating to
/// [`resolve_class_string_targets`](super::class_string_resolution::resolve_class_string_targets).
/// The names are qualified because the caller reads them in whatever file
/// the call site lives in, which may have no import for a class the
/// class-string travelled in from.
pub(in crate::type_engine) fn build_var_resolver_from_ctx<'a>(
    ctx: &'a VarResolutionCtx<'a>,
) -> impl Fn(&str) -> Vec<String> + 'a {
    move |var_name: &str| -> Vec<String> {
        super::class_string_resolution::resolve_class_string_targets(
            var_name,
            ctx.current_class,
            ctx.all_classes,
            ctx.content,
            ctx.cursor_offset,
            ctx.class_loader,
            ctx.backend,
        )
        .iter()
        .map(|c| c.fqn().to_string())
        .collect()
    }
}

/// Check whether a type hint should be enriched with generic args for
/// Eloquent scope method Builder parameters.
///
/// When `type_str` resolves to `Builder` (the Eloquent Builder, without
/// generic parameters) and the enclosing method is a scope on a class
/// that extends Eloquent Model, returns a `TypeKind::Generic` wrapping
/// the builder name and the enclosing model.  Otherwise returns `None`,
/// meaning the caller should use the original type.
///
/// A method is considered a scope when it uses the `scopeX` naming
/// convention (name starts with `scope`, len > 5) **or** when
/// `has_scope_attr` is `true` (the method has `#[Scope]`).
pub(super) fn enrich_builder_type_in_scope(
    type_hint: &PhpType,
    method_name: &str,
    has_scope_attr: bool,
    current_class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    use crate::virtual_members::laravel::{ELOQUENT_BUILDER_FQN, extends_eloquent_model};

    let is_convention_scope = method_name.starts_with("scope") && method_name.len() > 5;
    if !is_convention_scope && !has_scope_attr {
        return None;
    }

    if !extends_eloquent_model(current_class, class_loader) {
        return None;
    }

    // Check if the type is the Eloquent Builder (without generic args).
    // Accept both the FQN and the short name `Builder` (common in use
    // imports).  If the type already has generic args (e.g.
    // `Builder<User>`), do not enrich — the user-supplied generics
    // should be used as-is.
    if type_hint.has_type_structure() {
        return None;
    }
    let type_name = match type_hint.kind() {
        TypeKind::Named(n) => n.as_str(),
        _ => return None,
    };
    let is_eloquent_builder = type_name == ELOQUENT_BUILDER_FQN || type_name == "Builder";
    if !is_eloquent_builder {
        return None;
    }

    Some(PhpType::generic(
        type_name,
        vec![PhpType::named(atom(current_class.name.as_ref()))],
    ))
}

/// Resolve the type of `$variable` at `cursor_offset`.
///
/// Checks the diagnostic scope cache first (O(log N) lookup from the
/// forward walker's pre-computed snapshots).  On cache miss, parses the
/// file and delegates to the forward walker via
/// [`resolve_variable_in_statements`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_variable_types(
    var_name: &str,
    current_class: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    content: &str,
    cursor_offset: u32,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&Backend>,
    loaders: Loaders<'_>,
) -> Vec<ResolvedType> {
    // ── Diagnostic scope cache fast path ─────────────────────────
    // When the diagnostic scope cache is active (populated by
    // `build_diagnostic_scopes` during a diagnostic pass), look up
    // the variable's pre-computed type from the forward-walked scope
    // snapshots.  This is O(log N) with zero recursion.
    if super::forward_walk::is_diagnostic_scope_active()
        && !super::forward_walk::is_building_scopes()
    {
        // The forward walker stores types under the `$`-prefixed name.
        let prefixed = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        if let Some(types) = super::forward_walk::lookup_diagnostic_scope(&prefixed, cursor_offset)
        {
            #[cfg(test)]
            TEST_SCOPE_CACHE_HITS.with(|count| count.set(count.get() + 1));
            return types;
        }
        // Variable not in the forward-walked scope — fall through to
        // the full resolution path.
    }

    let key = var_query_key(content, var_name, cursor_offset, current_class.name);

    // ── Memo ────────────────────────────────────────────────────
    // Everything below walks the enclosing body from its first
    // statement, which reaches the same answer every time within one
    // pass — except inside a body-return inference, where the walk
    // seeds the body's parameters with what the *call site* passed.
    // The same variable at the same offset legitimately answers
    // differently for each caller then, and what decided it is not in
    // the key, so that walk neither reads the memo nor writes to it.
    let memoisable = !crate::type_engine::call_resolution::body_inference_in_progress();
    if memoisable
        && let Some(hit) = VAR_TYPE_MEMO.with(|cell| {
            cell.borrow()
                .as_ref()
                .and_then(|memo| memo.get(&key).cloned())
        })
    {
        return hit;
    }

    // ── Re-entry guard (Guard 2) ────────────────────────────────
    // Break cycles where the same variable query re-enters through
    // call-argument resolution or template substitution paths that
    // bypass scope_var_resolver.  A re-entrant query cannot
    // contribute information while its outer invocation is still
    // incomplete.
    //
    // A suppressed query is not memoised: the empty answer describes
    // the stack it was asked on, not the question.
    let _var_guard = match try_acquire_var_guard(key) {
        Some(guard) => guard,
        None => return vec![],
    };

    let resolved = with_parsed_program(content, "resolve_variable_types", |program, _content| {
        let active_cache = crate::virtual_members::active_resolved_class_cache();
        let ctx = VarResolutionCtx {
            var_name,
            current_class,
            all_classes,
            content,
            cursor_offset,
            class_loader,
            backend,
            loaders,
            resolved_class_cache: active_cache,
            enclosing_return_type: None,
            top_level_scope: None,
            branch_aware: false,
            match_arm_narrowing: HashMap::new(),

            scope_var_resolver: None,
            scope_proofs: None,
        };

        resolve_variable_in_statements(program.statements.iter(), &ctx)
    });

    if memoisable {
        VAR_TYPE_MEMO.with(|cell| {
            if let Some(memo) = cell.borrow_mut().as_mut() {
                memo.insert(key, resolved.clone());
            }
        });
    }

    resolved
}

/// Resolve the type of a variable at `cursor_offset` as a [`PhpType`].
///
/// This is the **single entry point** for all consumers that need to
/// answer "what is the type of `$var` at this offset?"  It wraps the
/// forward walker ([`resolve_variable_types`]) and converts the result
/// to a `PhpType`, incorporating:
///
/// - The forward walker's branch-aware narrowing.
/// - Inline `/** @var Type $var */` docblocks, as a fallback for the
///   positions the walker cannot reach (a Blade template, a variable
///   with no assignment in the file at all).  An annotation the walker
///   *did* see has already been applied to the assignment it documents,
///   and the walker's answer accounts for everything that happened to
///   the variable since — a reassignment, a `!== null` guard — so it
///   wins.
///
/// Consumers: hover, go-to-type-definition, find-references (variable
/// subject resolution), deprecated diagnostics, code actions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_variable_php_type(
    var_name: &str,
    content: &str,
    cursor_offset: u32,
    current_class: Option<&ClassInfo>,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&Backend>,
    loaders: Loaders<'_>,
) -> Option<PhpType> {
    // Ensure the variable name is $-prefixed for docblock lookups.
    let prefixed = if var_name.starts_with('$') {
        var_name.to_owned()
    } else {
        format!("${}", var_name)
    };

    // 1. Inline @var override (skip for self-assignment RHS).
    let var_override: Option<PhpType> = if let Some(var_type) =
        docblock::find_var_raw_type_in_source(content, cursor_offset as usize, &prefixed)
        && !is_cursor_in_self_assignment_rhs(content, cursor_offset as usize, &prefixed)
    {
        Some(crate::util::resolve_php_type_names(&var_type, class_loader))
    } else {
        None
    };

    // 2. Forward walker resolution.
    let dummy_class;
    let effective_class = match current_class {
        Some(cc) => cc,
        None => {
            dummy_class = crate::class_lookup::class_context_placeholder(content, cursor_offset);
            &dummy_class
        }
    };

    let resolved = resolve_variable_types(
        &prefixed,
        effective_class,
        all_classes,
        content,
        cursor_offset,
        class_loader,
        backend,
        loaders,
    );

    if !resolved.is_empty() {
        return Some(ResolvedType::types_joined(&resolved));
    }

    // 3. Parameter definition site fallback.
    //    When the cursor is on a parameter declaration (inside the
    //    parameter list, not the body), the forward walker won't find
    //    it because it only processes body statements.  Parse the file
    //    and check if the cursor is on a parameter with a type hint.
    let param_type = with_parsed_program(content, "resolve_var_param_site", |program, _| {
        let stmts: Vec<&Statement> = program.statements.iter().collect();
        find_param_type_at_cursor(&stmts, &prefixed, cursor_offset, content)
            .or_else(|| find_catch_var_type_at_cursor(&stmts, &prefixed, cursor_offset))
    });
    if param_type.is_some() {
        return param_type;
    }

    // Fall back to the @var override.
    var_override
}

/// Check whether `cursor_offset` falls inside the RHS of an assignment
/// like `$var = $var->…` on the same line.  Used to avoid applying an
/// inline `@var` cast to the RHS reference.
fn is_cursor_in_self_assignment_rhs(content: &str, cursor_offset: usize, var_name: &str) -> bool {
    let before = match content.get(..cursor_offset) {
        Some(b) => b,
        None => return false,
    };
    let line_start = before.rfind('\n').map_or(0, |pos| pos + 1);

    let after = match content.get(cursor_offset..) {
        Some(a) => a,
        None => return false,
    };
    let line_end = after
        .find('\n')
        .map_or(content.len(), |pos| cursor_offset + pos);

    let line = match content.get(line_start..line_end) {
        Some(l) => l,
        None => return false,
    };

    let needle = format!("{} = ", var_name);
    if let Some(assign_pos) = line.find(&needle) {
        let rhs_start_in_line = assign_pos + needle.len();
        let cursor_in_line = cursor_offset - line_start;
        let rhs = &line[rhs_start_in_line..];
        if cursor_in_line >= rhs_start_in_line && rhs.contains(var_name) {
            return true;
        }
    }
    false
}

/// Check if the cursor is on a parameter definition and return its type.
///
/// Walks namespaces, classes, and functions to find a parameter list
/// that contains `cursor_offset` with a parameter matching `var_name`.
fn find_param_type_at_cursor(
    stmts: &[&Statement<'_>],
    var_name: &str,
    cursor_offset: u32,
    content: &str,
) -> Option<PhpType> {
    use mago_span::HasSpan;

    for stmt in stmts {
        match stmt {
            Statement::Namespace(ns) => {
                let inner: Vec<&Statement> = ns.statements().iter().collect();
                if let Some(t) = find_param_type_at_cursor(&inner, var_name, cursor_offset, content)
                {
                    return Some(t);
                }
            }
            Statement::Class(class) => {
                for member in class.members.iter() {
                    if let ClassLikeMember::Method(method) = member
                        && let Some(t) = check_param_list(
                            &method.parameter_list,
                            var_name,
                            cursor_offset,
                            content,
                            method.span().start.offset as usize,
                        )
                    {
                        return Some(t);
                    }
                }
            }
            Statement::Trait(trait_def) => {
                for member in trait_def.members.iter() {
                    if let ClassLikeMember::Method(method) = member
                        && let Some(t) = check_param_list(
                            &method.parameter_list,
                            var_name,
                            cursor_offset,
                            content,
                            method.span().start.offset as usize,
                        )
                    {
                        return Some(t);
                    }
                }
            }
            Statement::Enum(enum_def) => {
                for member in enum_def.members.iter() {
                    if let ClassLikeMember::Method(method) = member
                        && let Some(t) = check_param_list(
                            &method.parameter_list,
                            var_name,
                            cursor_offset,
                            content,
                            method.span().start.offset as usize,
                        )
                    {
                        return Some(t);
                    }
                }
            }
            Statement::Interface(iface) => {
                for member in iface.members.iter() {
                    if let ClassLikeMember::Method(method) = member
                        && let Some(t) = check_param_list(
                            &method.parameter_list,
                            var_name,
                            cursor_offset,
                            content,
                            method.span().start.offset as usize,
                        )
                    {
                        return Some(t);
                    }
                }
            }
            Statement::Function(func) => {
                if let Some(t) = check_param_list(
                    &func.parameter_list,
                    var_name,
                    cursor_offset,
                    content,
                    func.span().start.offset as usize,
                ) {
                    return Some(t);
                }
            }
            _ => {}
        }
    }
    None
}

/// Check if a parameter list contains the cursor and has a matching
/// parameter with a type hint.
fn check_param_list(
    param_list: &FunctionLikeParameterList<'_>,
    var_name: &str,
    cursor_offset: u32,
    content: &str,
    method_start_offset: usize,
) -> Option<PhpType> {
    use mago_span::HasSpan;

    let span = param_list.span();
    if cursor_offset < span.start.offset || cursor_offset > span.end.offset {
        return None;
    }

    for param in param_list.parameters.iter() {
        let pname = bytes_to_str(param.variable.name);
        if pname != var_name {
            continue;
        }

        let native_type = param.hint.as_ref().map(|h| extract_hint_type(h));

        // Try @param docblock type.
        let docblock_type =
            docblock::find_iterable_raw_type_in_source(content, method_start_offset, var_name)
                .or_else(|| {
                    // Try extracting from docblock text directly.
                    find_method_docblock_text(content, method_start_offset)
                        .and_then(|doc| docblock::extract_param_raw_type(&doc, pname))
                });

        let effective =
            docblock::resolve_effective_type_typed(native_type.as_ref(), docblock_type.as_ref());

        if effective.is_some() {
            return effective;
        }
        return native_type;
    }
    None
}

/// Extract the raw docblock text preceding a method/function.
fn find_method_docblock_text(content: &str, method_start: usize) -> Option<String> {
    let before = content.get(..method_start)?;
    let trimmed = before.trim_end();
    if !trimmed.ends_with("*/") {
        return None;
    }
    let doc_end = trimmed.len();
    let doc_start = trimmed.rfind("/**")?;
    Some(trimmed[doc_start..doc_end].to_string())
}

/// Check if the cursor is on a catch variable binding and return its type.
fn find_catch_var_type_at_cursor(
    stmts: &[&Statement<'_>],
    var_name: &str,
    cursor_offset: u32,
) -> Option<PhpType> {
    use mago_span::HasSpan;

    for stmt in stmts {
        let stmt_span = stmt.span();
        if cursor_offset < stmt_span.start.offset || cursor_offset > stmt_span.end.offset {
            continue;
        }
        match stmt {
            Statement::Try(try_stmt) => {
                for catch in try_stmt.catch_clauses.iter() {
                    if let Some(ref var) = catch.variable
                        && bytes_to_str(var.name) == var_name
                    {
                        let var_start = var.span.start.offset;
                        let var_end = var.span.end.offset;
                        if cursor_offset >= var_start && cursor_offset <= var_end {
                            return Some(extract_hint_type(&catch.hint));
                        }
                    }
                }
                // Recurse into try/catch/finally bodies.
                let try_stmts: Vec<&Statement> = try_stmt.block.statements.iter().collect();
                if let Some(t) = find_catch_var_type_at_cursor(&try_stmts, var_name, cursor_offset)
                {
                    return Some(t);
                }
                for catch in try_stmt.catch_clauses.iter() {
                    let catch_stmts: Vec<&Statement> = catch.block.statements.iter().collect();
                    if let Some(t) =
                        find_catch_var_type_at_cursor(&catch_stmts, var_name, cursor_offset)
                    {
                        return Some(t);
                    }
                }
                if let Some(ref finally) = try_stmt.finally_clause {
                    let fin_stmts: Vec<&Statement> = finally.block.statements.iter().collect();
                    if let Some(t) =
                        find_catch_var_type_at_cursor(&fin_stmts, var_name, cursor_offset)
                    {
                        return Some(t);
                    }
                }
            }
            Statement::Namespace(ns) => {
                let inner: Vec<&Statement> = ns.statements().iter().collect();
                if let Some(t) = find_catch_var_type_at_cursor(&inner, var_name, cursor_offset) {
                    return Some(t);
                }
            }
            Statement::Class(class) => {
                for member in class.members.iter() {
                    if let ClassLikeMember::Method(method) = member
                        && let MethodBody::Concrete(body) = &method.body
                    {
                        let body_stmts: Vec<&Statement> = body.statements.iter().collect();
                        if let Some(t) =
                            find_catch_var_type_at_cursor(&body_stmts, var_name, cursor_offset)
                        {
                            return Some(t);
                        }
                    }
                }
            }
            Statement::Function(func) => {
                let body_stmts: Vec<&Statement> = func.body.statements.iter().collect();
                if let Some(t) = find_catch_var_type_at_cursor(&body_stmts, var_name, cursor_offset)
                {
                    return Some(t);
                }
            }
            _ => {}
        }
    }
    None
}

/// Walk a sequence of top-level statements to find the class or
/// function body that contains the cursor, then resolve the target
/// variable's type within that scope.
pub(in crate::type_engine) fn resolve_variable_in_statements<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // Collect so we can iterate twice: once to check class bodies,
    // once (if needed) to walk top-level statements.
    let stmts: Vec<&Statement> = statements.collect();

    // Pre-compute top-level variable scope so that `global $x` inside
    // function bodies can look up `$x`'s type from the file's top level.
    // Only do the expensive full-file walk when the file actually uses the
    // `global` keyword.  When the cursor turns out to be at the top level
    // (not inside any class or function), this scope is also reused for
    // the variable lookup, avoiding a redundant second forward walk.
    let file_has_global_keyword = ctx.content.contains("global ");
    let top_level_scope = if ctx.top_level_scope.is_none() && file_has_global_keyword {
        // Guard 1: prevent re-entrant top-level scope construction for
        // the same file.  RHS resolution during the walk can trigger
        // another resolve_variable_types call, which would start a
        // second walk_top_level_for_globals on the same file content.
        if let Some(_tl_guard) = try_acquire_top_level_guard(ctx.content) {
            let tl_fw_ctx = super::forward_walk::ForwardWalkCtx {
                current_class: ctx.current_class,
                all_classes: ctx.all_classes,
                content: ctx.content,
                cursor_offset: u32::MAX,
                class_loader: ctx.class_loader,
                backend: ctx.backend,
                loaders: ctx.loaders,
                resolved_class_cache: ctx.resolved_class_cache,
                enclosing_return_type: None,
                top_level_scope: None,
            };
            let mut tl_scope = super::forward_walk::ScopeState::new();
            super::forward_walk::walk_top_level_for_globals(
                stmts.iter().copied(),
                &mut tl_scope,
                &tl_fw_ctx,
            );
            if tl_scope.locals.is_empty() {
                None
            } else {
                Some(tl_scope.locals)
            }
        } else {
            None
        }
    } else {
        ctx.top_level_scope.clone()
    };

    // Shadow ctx with one that carries the pre-computed top-level
    // scope, copying all other fields unchanged.
    let ctx_with_tls;
    let ctx: &VarResolutionCtx<'_> = if top_level_scope.is_some() && ctx.top_level_scope.is_none() {
        ctx_with_tls = VarResolutionCtx {
            var_name: ctx.var_name,
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope,
            branch_aware: ctx.branch_aware,
            match_arm_narrowing: ctx.match_arm_narrowing.clone(),
            scope_var_resolver: ctx.scope_var_resolver,
            scope_proofs: ctx.scope_proofs,
        };
        &ctx_with_tls
    } else {
        ctx
    };

    for &stmt in &stmts {
        match stmt {
            Statement::Class(class) => {
                let start = class.left_brace.start.offset;
                let end = class.right_brace.end.offset;
                if ctx.cursor_offset < start || ctx.cursor_offset > end {
                    continue;
                }
                // The cursor is inside this class body.  PHP method
                // scopes are isolated — they cannot access variables
                // from enclosing or top-level code.  Return whatever
                // the member scan found (even if empty, e.g. after
                // `unset($var)`), and never fall through to the
                // top-level walk.
                return resolve_variable_in_members(class.members.iter(), ctx);
            }
            Statement::Interface(iface) => {
                let start = iface.left_brace.start.offset;
                let end = iface.right_brace.end.offset;
                if ctx.cursor_offset < start || ctx.cursor_offset > end {
                    continue;
                }
                return resolve_variable_in_members(iface.members.iter(), ctx);
            }
            Statement::Enum(enum_def) => {
                let start = enum_def.left_brace.start.offset;
                let end = enum_def.right_brace.end.offset;
                if ctx.cursor_offset < start || ctx.cursor_offset > end {
                    continue;
                }
                return resolve_variable_in_members(enum_def.members.iter(), ctx);
            }
            Statement::Trait(trait_def) => {
                let start = trait_def.left_brace.start.offset;
                let end = trait_def.right_brace.end.offset;
                if ctx.cursor_offset < start || ctx.cursor_offset > end {
                    continue;
                }
                return resolve_variable_in_members(trait_def.members.iter(), ctx);
            }
            Statement::Namespace(ns) => {
                // Only recurse into namespace blocks that contain the
                // cursor.  Without this check, variables with the same
                // name in earlier namespace blocks (e.g. `$b` in two
                // different blocks) would be returned from the wrong
                // block, causing cross-namespace variable shadowing.
                let ns_span = ns.span();
                if ctx.cursor_offset < ns_span.start.offset
                    || ctx.cursor_offset > ns_span.end.offset
                {
                    continue;
                }
                let results = resolve_variable_in_statements(ns.statements().iter(), ctx);
                if !results.is_empty() {
                    return results;
                }
            }
            // ── Top-level function declarations ──
            // If the cursor is inside a `function foo(Type $p) { … }`
            // at the top level, resolve the variable from its params
            // and walk its body.
            Statement::Function(func) => {
                if let Some(results) = try_resolve_in_function(func, ctx) {
                    return results;
                }
            }
            // ── Functions inside if-guards / blocks ──
            // The common PHP pattern `if (! function_exists('foo'))
            // { function foo(Type $p) { … } }` nests the function
            // declaration inside an if body.  Recurse into blocks
            // and if-bodies so the function's parameters and body
            // assignments are still resolved.
            Statement::If(_) | Statement::Block(_) => {
                if let Some(results) = try_resolve_in_nested_function(stmt, ctx) {
                    return results;
                }
            }
            _ => {}
        }

        // ── Anonymous classes inside expressions ──
        // Anonymous classes (`new class { … }`) appear as expressions
        // inside statements (e.g. `return new class extends Foo { … };`
        // or `$x = new class { … };`).  If the cursor falls inside one,
        // resolve variables from its member methods just like we do for
        // named classes above.
        let stmt_span = stmt.span();
        if ctx.cursor_offset >= stmt_span.start.offset
            && ctx.cursor_offset <= stmt_span.end.offset
            && let Some(anon) = find_anonymous_class_containing_cursor(stmt, ctx.cursor_offset)
        {
            return resolve_variable_in_members(anon.members.iter(), ctx);
        }
    }

    // The cursor is not inside any class/interface/enum body — it must
    // be in top-level code.  Look up the variable from the pre-computed
    // top-level scope (built above with cursor_offset=u32::MAX).
    if let Some(ref tls) = ctx.top_level_scope {
        let prefixed = if ctx.var_name.starts_with('$') {
            ctx.var_name.to_string()
        } else {
            format!("${}", ctx.var_name)
        };
        if let Some(types) = tls.get(&atom(&prefixed))
            && !types.is_empty()
        {
            return types.clone();
        }
    } else {
        // Fallback: top_level_scope was not pre-computed (should not
        // happen in normal flow, but be defensive).  Run a
        // position-aware forward walk.
        let fw_ctx = super::forward_walk::ForwardWalkCtx {
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: None,
            top_level_scope: None,
        };
        if let Some(fw_results) =
            super::forward_walk::resolve_in_top_level(ctx.var_name, stmts.iter().copied(), &fw_ctx)
        {
            return fw_results;
        }
    }

    vec![]
}

/// Recursively walk a statement's expression tree looking for an
/// `AnonymousClass` whose body (between `{` and `}`) contains the
/// given cursor offset.  Returns a reference to the first matching
/// anonymous class node, or `None`.
fn find_anonymous_class_containing_cursor<'a>(
    stmt: &'a Statement<'a>,
    cursor_offset: u32,
) -> Option<&'a AnonymousClass<'a>> {
    /// Walk an expression tree for an anonymous class containing the cursor.
    fn walk_expr<'a>(expr: &'a Expression<'a>, cursor: u32) -> Option<&'a AnonymousClass<'a>> {
        let sp = expr.span();
        if cursor < sp.start.offset || cursor > sp.end.offset {
            return None;
        }
        match expr {
            Expression::AnonymousClass(anon) => {
                if cursor >= anon.left_brace.start.offset && cursor <= anon.right_brace.end.offset {
                    return Some(anon);
                }
                None
            }
            Expression::Parenthesized(p) => walk_expr(p.expression, cursor),
            Expression::Assignment(a) => {
                walk_expr(a.lhs, cursor).or_else(|| walk_expr(a.rhs, cursor))
            }
            Expression::Binary(b) => walk_expr(b.lhs, cursor).or_else(|| walk_expr(b.rhs, cursor)),
            Expression::Conditional(c) => walk_expr(c.condition, cursor)
                .or_else(|| c.then.and_then(|e| walk_expr(e, cursor)))
                .or_else(|| walk_expr(c.r#else, cursor)),
            Expression::Call(call) => match call {
                Call::Function(fc) => walk_args(&fc.argument_list.arguments, cursor),
                Call::Method(mc) => walk_expr(mc.object, cursor)
                    .or_else(|| walk_args(&mc.argument_list.arguments, cursor)),
                Call::NullSafeMethod(mc) => walk_expr(mc.object, cursor)
                    .or_else(|| walk_args(&mc.argument_list.arguments, cursor)),
                Call::StaticMethod(sc) => walk_expr(sc.class, cursor)
                    .or_else(|| walk_args(&sc.argument_list.arguments, cursor)),
            },
            Expression::Array(arr) => {
                for elem in arr.elements.iter() {
                    let found = match elem {
                        ArrayElement::KeyValue(kv) => {
                            walk_expr(kv.key, cursor).or_else(|| walk_expr(kv.value, cursor))
                        }
                        ArrayElement::Value(v) => walk_expr(v.value, cursor),
                        ArrayElement::Variadic(v) => walk_expr(v.value, cursor),
                        _ => None,
                    };
                    if found.is_some() {
                        return found;
                    }
                }
                None
            }
            Expression::LegacyArray(arr) => {
                for elem in arr.elements.iter() {
                    let found = match elem {
                        ArrayElement::KeyValue(kv) => {
                            walk_expr(kv.key, cursor).or_else(|| walk_expr(kv.value, cursor))
                        }
                        ArrayElement::Value(v) => walk_expr(v.value, cursor),
                        ArrayElement::Variadic(v) => walk_expr(v.value, cursor),
                        _ => None,
                    };
                    if found.is_some() {
                        return found;
                    }
                }
                None
            }
            Expression::Closure(closure) => {
                // The anonymous class could be inside a closure body.
                for inner in closure.body.statements.iter() {
                    if let Some(anon) = find_anonymous_class_containing_cursor(inner, cursor) {
                        return Some(anon);
                    }
                }
                None
            }
            Expression::ArrowFunction(arrow) => walk_expr(arrow.expression, cursor),
            Expression::Instantiation(inst) => {
                if let Some(ref args) = inst.argument_list {
                    walk_args(&args.arguments, cursor)
                } else {
                    None
                }
            }
            Expression::UnaryPrefix(u) => walk_expr(u.operand, cursor),
            Expression::UnaryPostfix(u) => walk_expr(u.operand, cursor),
            Expression::Throw(t) => walk_expr(t.exception, cursor),
            Expression::Clone(c) => walk_expr(c.object, cursor),
            Expression::Match(m) => {
                if let Some(found) = walk_expr(m.expression, cursor) {
                    return Some(found);
                }
                for arm in m.arms.iter() {
                    if let Some(found) = walk_expr(arm.expression(), cursor) {
                        return Some(found);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Walk a list of call arguments.
    fn walk_args<'a>(
        arguments: &'a mago_syntax::cst::sequence::TokenSeparatedSequence<'a, Argument<'a>>,
        cursor: u32,
    ) -> Option<&'a AnonymousClass<'a>> {
        for arg in arguments.iter() {
            let arg_expr = match arg {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            };
            if let Some(found) = walk_expr(arg_expr, cursor) {
                return Some(found);
            }
        }
        None
    }

    match stmt {
        Statement::Expression(expr_stmt) => walk_expr(expr_stmt.expression, cursor_offset),
        Statement::Return(ret) => ret.value.as_ref().and_then(|v| walk_expr(v, cursor_offset)),
        Statement::Block(block) => {
            for inner in block.statements.iter() {
                if let Some(anon) = find_anonymous_class_containing_cursor(inner, cursor_offset) {
                    return Some(anon);
                }
            }
            None
        }
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => {
                find_anonymous_class_containing_cursor(body.statement, cursor_offset)
            }
            IfBody::ColonDelimited(body) => {
                for inner in body.statements.iter() {
                    if let Some(anon) = find_anonymous_class_containing_cursor(inner, cursor_offset)
                    {
                        return Some(anon);
                    }
                }
                None
            }
        },
        Statement::Foreach(foreach) => match &foreach.body {
            ForeachBody::Statement(inner) => {
                find_anonymous_class_containing_cursor(inner, cursor_offset)
            }
            ForeachBody::ColonDelimited(body) => {
                for inner in body.statements.iter() {
                    if let Some(anon) = find_anonymous_class_containing_cursor(inner, cursor_offset)
                    {
                        return Some(anon);
                    }
                }
                None
            }
        },
        Statement::While(while_stmt) => match &while_stmt.body {
            WhileBody::Statement(inner) => {
                find_anonymous_class_containing_cursor(inner, cursor_offset)
            }
            WhileBody::ColonDelimited(body) => {
                for inner in body.statements.iter() {
                    if let Some(anon) = find_anonymous_class_containing_cursor(inner, cursor_offset)
                    {
                        return Some(anon);
                    }
                }
                None
            }
        },
        Statement::For(for_stmt) => match &for_stmt.body {
            ForBody::Statement(inner) => {
                find_anonymous_class_containing_cursor(inner, cursor_offset)
            }
            ForBody::ColonDelimited(body) => {
                for inner in body.statements.iter() {
                    if let Some(anon) = find_anonymous_class_containing_cursor(inner, cursor_offset)
                    {
                        return Some(anon);
                    }
                }
                None
            }
        },
        Statement::DoWhile(dw) => {
            find_anonymous_class_containing_cursor(dw.statement, cursor_offset)
        }
        Statement::Try(try_stmt) => {
            for inner in try_stmt.block.statements.iter() {
                if let Some(anon) = find_anonymous_class_containing_cursor(inner, cursor_offset) {
                    return Some(anon);
                }
            }
            for catch in try_stmt.catch_clauses.iter() {
                for inner in catch.block.statements.iter() {
                    if let Some(anon) = find_anonymous_class_containing_cursor(inner, cursor_offset)
                    {
                        return Some(anon);
                    }
                }
            }
            if let Some(finally) = &try_stmt.finally_clause {
                for inner in finally.block.statements.iter() {
                    if let Some(anon) = find_anonymous_class_containing_cursor(inner, cursor_offset)
                    {
                        return Some(anon);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Try to resolve the target variable inside a `Function` declaration.
///
/// Returns `Some(results)` when the cursor falls inside the function body
/// (the function introduces an isolated scope, so we always return even
/// when the result vec is empty).  Returns `None` when the cursor is
/// outside this function.
fn try_resolve_in_function(
    func: &Function<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let body_start = func.body.left_brace.start.offset;
    let body_end = func.body.right_brace.end.offset;
    if ctx.cursor_offset < body_start || ctx.cursor_offset > body_end {
        return None;
    }
    // Extract the enclosing function's @return type for generator
    // yield inference inside the body.  Use body_start + 1 (just
    // past the opening `{`) so the backward brace scan in
    // find_enclosing_return_type immediately finds the function's
    // own `{` and does NOT get confused by intermediate `{`/`}`
    // from nested control-flow.
    let enclosing_ret =
        crate::docblock::find_enclosing_return_type(ctx.content, (body_start + 1) as usize);

    // ── Forward walker (sole resolver) ──
    let fw_ctx = super::forward_walk::ForwardWalkCtx {
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: ctx.cursor_offset,
        class_loader: ctx.class_loader,
        backend: ctx.backend,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: enclosing_ret,
        top_level_scope: ctx.top_level_scope.clone(),
    };
    Some(
        super::forward_walk::resolve_in_function_body(ctx.var_name, func, &fw_ctx)
            .unwrap_or_default(),
    )
}

/// Recursively search a statement for a nested `Function` declaration
/// whose body contains the cursor.
///
/// This handles the common PHP pattern where functions are wrapped in
/// `if (! function_exists('name')) { function name(…) { … } }` guards.
/// The function may be nested inside `Block`, `If`, or other compound
/// statements.
fn try_resolve_in_nested_function(
    stmt: &Statement<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    // Quick span check — skip if cursor is outside this statement entirely.
    let span = stmt.span();
    if ctx.cursor_offset < span.start.offset || ctx.cursor_offset > span.end.offset {
        return None;
    }
    match stmt {
        Statement::Function(func) => try_resolve_in_function(func, ctx),
        Statement::Block(block) => {
            for inner in block.statements.iter() {
                if let Some(results) = try_resolve_in_nested_function(inner, ctx) {
                    return Some(results);
                }
            }
            None
        }
        Statement::If(if_stmt) => {
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    if let Some(results) = try_resolve_in_nested_function(body.statement, ctx) {
                        return Some(results);
                    }
                    for else_if in body.else_if_clauses.iter() {
                        if let Some(results) =
                            try_resolve_in_nested_function(else_if.statement, ctx)
                        {
                            return Some(results);
                        }
                    }
                    if let Some(else_clause) = &body.else_clause
                        && let Some(results) =
                            try_resolve_in_nested_function(else_clause.statement, ctx)
                    {
                        return Some(results);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for inner in body.statements.iter() {
                        if let Some(results) = try_resolve_in_nested_function(inner, ctx) {
                            return Some(results);
                        }
                    }
                    for else_if in body.else_if_clauses.iter() {
                        for inner in else_if.statements.iter() {
                            if let Some(results) = try_resolve_in_nested_function(inner, ctx) {
                                return Some(results);
                            }
                        }
                    }
                    if let Some(else_clause) = &body.else_clause {
                        for inner in else_clause.statements.iter() {
                            if let Some(results) = try_resolve_in_nested_function(inner, ctx) {
                                return Some(results);
                            }
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Locate the enclosing method or property hook for the cursor position
/// and delegate to the forward walker.  For abstract methods (no body),
/// returns the parameter type hint directly.
fn resolve_variable_in_members<'b>(
    members: impl Iterator<Item = &'b ClassLikeMember<'b>>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    for member in members {
        if let ClassLikeMember::Property(Property::Hooked(hooked)) = member {
            let hooks_span = hooked.hook_list.span();
            if ctx.cursor_offset < hooks_span.start.offset
                || ctx.cursor_offset > hooks_span.end.offset
            {
                continue;
            }
            return resolve_variable_in_property_hooks(
                hooked.hint.as_ref(),
                &hooked.hook_list,
                ctx,
            );
        }

        if let ClassLikeMember::Method(method) = member {
            // A constructor-promoted property's hooks sit in the parameter
            // list, outside the method body checked below.
            for param in method.parameter_list.parameters.iter() {
                let Some(hooks) = &param.hooks else {
                    continue;
                };
                let hooks_span = hooks.span();
                if ctx.cursor_offset < hooks_span.start.offset
                    || ctx.cursor_offset > hooks_span.end.offset
                {
                    continue;
                }
                return resolve_variable_in_property_hooks(param.hint.as_ref(), hooks, ctx);
            }

            // ── Concrete method: delegate entirely to the forward walker ──
            if let MethodBody::Concrete(block) = &method.body {
                let blk_start = block.left_brace.start.offset;
                let blk_end = block.right_brace.end.offset;
                if ctx.cursor_offset >= blk_start && ctx.cursor_offset <= blk_end {
                    let has_scope_attr = method.attribute_lists.iter().any(|al| {
                        al.attributes
                            .iter()
                            .any(|a| last_segment(a.name.value()) == b"Scope")
                    });

                    // Extract the enclosing method's @return type for
                    // generator yield inference inside the body.
                    // Use blk_start + 1 (just past the opening `{`)
                    // so the brace scan in find_enclosing_return_type
                    // immediately finds the method's own `{` and does
                    // NOT get confused by intermediate `{`/`}` from
                    // nested control-flow.
                    let enclosing_ret = crate::docblock::find_enclosing_return_type(
                        ctx.content,
                        (blk_start + 1) as usize,
                    );

                    let fw_ctx = super::forward_walk::ForwardWalkCtx {
                        current_class: ctx.current_class,
                        all_classes: ctx.all_classes,
                        content: ctx.content,
                        cursor_offset: ctx.cursor_offset,
                        class_loader: ctx.class_loader,
                        backend: ctx.backend,
                        loaders: ctx.loaders,
                        resolved_class_cache: ctx.resolved_class_cache,
                        enclosing_return_type: enclosing_ret,
                        top_level_scope: ctx.top_level_scope.clone(),
                    };
                    let method_name_str = bytes_to_str(method.name.value).to_string();
                    let is_static = method.modifiers.contains_static();
                    return super::forward_walk::resolve_in_method_body(
                        ctx.var_name,
                        method.parameter_list.parameters.iter(),
                        block.statements.iter(),
                        method.span().start.offset,
                        Some((&method_name_str, has_scope_attr)),
                        is_static,
                        &fw_ctx,
                    )
                    .unwrap_or_default();
                }
                // Cursor is not inside this method's body — skip to
                // the next member.
                continue;
            }

            // ── Abstract method (no body) ──
            // Return the parameter type hint when the cursor falls
            // within the method's overall span (signature region).
            let method_start = method.span().start.offset;
            let method_end = method.span().end.offset;
            if ctx.cursor_offset < method_start || ctx.cursor_offset > method_end {
                continue;
            }

            return resolve_abstract_method_param(method, ctx);
        }
    }
    vec![]
}

/// Resolve a variable read from inside a `get`/`set` hook body.
///
/// A hook body is a function body: `$this` is the enclosing instance, a
/// `set` hook takes the assigned value as `$value` (declared or implicit),
/// and a block-bodied hook can assign locals of its own.  The forward
/// walker answers all three, the same way it does for a method body.
fn resolve_variable_in_property_hooks(
    property_hint: Option<&Hint<'_>>,
    hook_list: &PropertyHookList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    for hook in hook_list.hooks.iter() {
        let PropertyHookBody::Concrete(body) = &hook.body else {
            continue;
        };

        let body_span = body.span();
        if ctx.cursor_offset < body_span.start.offset || ctx.cursor_offset > body_span.end.offset {
            continue;
        }

        let fw_ctx = super::forward_walk::ForwardWalkCtx {
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: None,
            top_level_scope: ctx.top_level_scope.clone(),
        };

        let mut scope = super::forward_walk::seed_property_hook_scope(property_hint, hook, &fw_ctx);
        if let PropertyHookConcreteBody::Block(block) = body {
            // Suspend snapshot recording for the same reason
            // `resolve_in_method_body` does: this is a transient lookup,
            // not the authoritative scope build.
            let _suspend = super::forward_walk::suspend_snapshot_recording();
            let _barrier = super::forward_walk::suspend_return_edges();
            super::forward_walk::walk_body_forward(block.statements.iter(), &mut scope, &fw_ctx);
        }

        return scope.get(ctx.var_name).to_vec();
    }

    vec![]
}

/// Resolve a parameter's type for an abstract method (no concrete body).
///
/// Delegates to [`super::forward_walk::resolve_param_type`] which
/// contains the shared parameter resolution pipeline (native hint →
/// Builder enrichment → docblock → template substitution → merged
/// class → type-string fallback).
fn resolve_abstract_method_param(
    method: &Method<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let has_scope_attr = method.attribute_lists.iter().any(|al| {
        al.attributes
            .iter()
            .any(|a| last_segment(a.name.value()) == b"Scope")
    });

    let method_name_str = bytes_to_str(method.name.value).to_string();

    for param in method.parameter_list.parameters.iter() {
        let pname = bytes_to_str(param.variable.name);
        if pname != ctx.var_name {
            continue;
        }

        let is_variadic = param.ellipsis.is_some();
        let native_type = param.hint.as_ref().map(|h| extract_hint_type(h));

        let fw_ctx = super::forward_walk::ForwardWalkCtx {
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: None,
            top_level_scope: ctx.top_level_scope.clone(),
        };

        let trait_prototype =
            super::forward_walk::trait_prototype_method(Some(&method_name_str), &fw_ctx);

        return super::forward_walk::resolve_param_type(
            pname,
            native_type.as_ref(),
            is_variadic,
            &super::forward_walk::EnclosingMethod {
                span_start: method.span().start.offset,
                name: Some(&method_name_str),
                has_scope_attr,
                trait_prototype: trait_prototype.as_ref(),
            },
            &fw_ctx,
        );
    }

    vec![]
}

/// Substitute method/function-level template parameter names with their
/// upper bounds from `@template T of Bound` annotations.
///
/// This handles the general case where a parameter type IS a template
/// parameter (e.g. `@param T $query` where `@template T of Builder`).
/// Without this substitution, `T` remains an unresolvable named type
/// and member access on `$query` fails with "subject type 'T' could not
/// be resolved".
///
/// Works on any `PhpType` structure — bare names, unions, intersections,
/// nullable wrappers, generics, etc. — via `PhpType::substitute`.
pub(super) fn substitute_template_param_bounds(
    ty: PhpType,
    content: &str,
    method_start_offset: usize,
) -> PhpType {
    // Quick check: only act when the type contains at least one bare
    // identifier that could be a template parameter.  This avoids the
    // docblock parse for the common case where the type is a concrete
    // class name or scalar.
    if !type_may_contain_template_param(&ty) {
        return ty;
    }

    let before = &content[..method_start_offset];
    let docblock = extract_preceding_docblock(before);

    let Some(docblock) = docblock else {
        return ty;
    };

    let bounds = docblock::extract_template_params_with_bounds(docblock);
    if bounds.is_empty() {
        return ty;
    }

    let mut subs = std::collections::HashMap::new();
    for (name, bound) in bounds {
        if let Some(bound_type) = bound {
            subs.insert(name, bound_type);
        }
    }

    if subs.is_empty() {
        return ty;
    }

    ty.substitute(&subs)
}

/// Check whether a `PhpType` tree may contain a bare template parameter
/// name — i.e. a `Named` variant whose value is not a well-known scalar
/// or pseudo-type.  This is a cheap pre-filter so that we only parse the
/// docblock when there is a realistic chance of finding a substitution.
fn type_may_contain_template_param(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(name) => {
            // Well-known scalars/pseudo-types are never template params.
            !is_keyword_type(name)
        }
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            members.iter().any(type_may_contain_template_param)
        }
        TypeKind::Nullable(inner) => type_may_contain_template_param(inner),
        TypeKind::Generic(g) => {
            !crate::php_type::is_keyword_type(&g.name)
                || g.args.iter().any(type_may_contain_template_param)
        }
        _ => false,
    }
}

/// Substitute method-level template parameters inside `class-string<T>`
/// types with their upper bounds from `@template T of Bound` annotations.
///
/// This enables `$class::` static member access resolution when the
/// parameter is typed as `class-string<T>` and `T` is bounded by a
/// concrete class.  Without this substitution, `T` remains an
/// unresolvable named type and `$class::` yields no completions.
pub(super) fn substitute_class_string_template_bounds(
    ty: PhpType,
    content: &str,
    method_start_offset: usize,
) -> PhpType {
    // Only act on class-string<T> where the inner type is a simple name
    // (i.e. a potential template parameter).
    let inner_name = match &ty.kind() {
        TypeKind::ClassString(Some(inner)) => match inner.kind() {
            TypeKind::Named(name) => Some(*name),
            _ => None,
        },
        _ => None,
    };

    let Some(tpl_name) = inner_name else {
        return ty;
    };

    // Extract the method's docblock to find template parameter bounds.
    // The docblock sits immediately before the method declaration, so
    // we search backward from the method's start offset.
    let before = &content[..method_start_offset];
    let docblock = extract_preceding_docblock(before);

    let Some(docblock) = docblock else {
        return ty;
    };

    let bounds = docblock::extract_template_params_with_bounds(docblock);
    for (name, bound) in bounds {
        if name == tpl_name
            && let Some(bound_type) = bound
        {
            return PhpType::class_string(Some(bound_type));
        }
    }

    ty
}

/// Extract the docblock comment immediately preceding a given offset.
///
/// Scans backward from `before` (the source text up to the method start)
/// to find the closest `/** ... */` block.  Returns `None` when no
/// docblock is found or when there is non-whitespace between the
/// docblock and the method declaration.
fn extract_preceding_docblock(before: &str) -> Option<&str> {
    let trimmed = before.trim_end();
    if !trimmed.ends_with("*/") {
        return None;
    }
    let close_pos = trimmed.len();
    let open_pos = trimmed.rfind("/**")?;
    Some(&trimmed[open_pos..close_pos])
}

/// Extract the "native" return-type string from the RHS of an assignment
/// expression, without resolving it to `ClassInfo`.
///
/// This is used by [`try_inline_var_override`] to feed
/// [`docblock::resolve_effective_type`] with the same kind of parsed
/// `PhpType` that `@return` override checking uses.
///
/// Returns `None` when the native type cannot be determined (the
/// caller should treat this as "unknown", which lets the docblock type
/// win unconditionally).
pub(crate) fn extract_native_type_from_rhs<'b>(
    rhs: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    match rhs {
        // `new ClassName(…)` → the class name.
        Expression::Instantiation(inst) => match inst.class {
            Expression::Identifier(ident) => {
                let name = bytes_to_str(ident.value()).to_string();
                let fqn = crate::util::resolve_source_class_name(
                    &name,
                    ctx.current_class.file_namespace.as_deref(),
                    ctx.class_loader,
                );
                Some(PhpType::named(atom(&fqn)))
            }
            Expression::Self_(_) => Some(PhpType::named(atom(ctx.current_class.name.as_ref()))),
            Expression::Static(_) => Some(PhpType::named(atom(ctx.current_class.name.as_ref()))),
            _ => None,
        },
        // Function / method calls → look up the return type.
        Expression::Call(call) => match call {
            Call::Function(func_call) => {
                let func_name = match func_call.function {
                    Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                    _ => None,
                };
                let func_name_offset = func_call.function.span().start.offset;
                func_name.and_then(|name| {
                    ctx.function_loader()
                        .and_then(|fl| fl(&name, func_name_offset))
                        .and_then(|fi| fi.return_type.clone())
                })
            }
            Call::Method(method_call) => {
                if let Expression::Variable(Variable::Direct(dv)) = method_call.object
                    && dv.name == b"$this"
                    && let ClassLikeMemberSelector::Identifier(ident) = &method_call.method
                {
                    let method_name = bytes_to_str(ident.value).to_string();
                    ctx.all_classes
                        .iter()
                        .find(|c| c.name == ctx.current_class.name)
                        .and_then(|cls| {
                            cls.get_method(&method_name)
                                .and_then(|m| m.return_type.clone())
                        })
                } else {
                    None
                }
            }
            Call::StaticMethod(static_call) => {
                let class_name = match static_call.class {
                    Expression::Self_(_) | Expression::Static(_) => {
                        Some(ctx.current_class.name.to_string())
                    }
                    Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                    _ => None,
                };
                if let Some(cls_name) = class_name
                    && let ClassLikeMemberSelector::Identifier(ident) = &static_call.method
                {
                    let method_name = bytes_to_str(ident.value).to_string();
                    let owner = ctx
                        .all_classes
                        .iter()
                        .find(|c| c.name == cls_name)
                        .map(|c| ClassInfo::clone(c))
                        .or_else(|| (ctx.class_loader)(&cls_name).map(Arc::unwrap_or_clone));
                    owner.and_then(|o| {
                        o.get_method(&method_name)
                            .and_then(|m| m.return_type.clone())
                    })
                } else {
                    None
                }
            }
            _ => None,
        },
        // First-class callable syntax, closure literals, and arrow
        // functions always produce a Closure. When we can infer the
        // body return type, preserve it in a typed `Closure(): T`.
        Expression::PartialApplication(_)
        | Expression::Closure(_)
        | Expression::ArrowFunction(_) => {
            Some(super::rhs_resolution::infer_closure_literal_type(rhs, ctx))
        }
        _ => None,
    }
}
// ── Shape mutation helpers ───────────────────────────────────────────

/// Walk a (possibly nested) `ArrayAccess` chain and return the base
/// variable name and the ordered list of index expressions from
/// outermost to innermost.
///
/// For `$var['a']['b']['c']` returns `Some(("$var", [expr_a, expr_b, expr_c]))`.
/// Returns `None` when the base expression is not a simple direct variable.
pub(super) fn extract_nested_array_access_chain<'a, 'b>(
    outermost: &'a ArrayAccess<'b>,
) -> Option<(String, Vec<&'a Expression<'b>>)> {
    let mut keys: Vec<&'a Expression<'b>> = Vec::new();
    keys.push(outermost.index);

    let mut current: &'a Expression<'b> = outermost.array;
    loop {
        match current {
            Expression::ArrayAccess(inner) => {
                keys.push(inner.index);
                current = inner.array;
            }
            Expression::Variable(Variable::Direct(dv)) => {
                // We collected keys innermost-first; reverse so the
                // outermost key (closest to the variable) comes first.
                keys.reverse();
                return Some((bytes_to_str(dv.name).to_string(), keys));
            }
            _ => return None,
        }
    }
}

/// A single key segment in a (possibly nested) array write like
/// `$var['a'][$i]['b'] = …`.
pub(super) enum ArrayWriteKey {
    /// A string-literal key tracked as a shape entry, e.g. `['name']`.
    Shape(String),
    /// A dynamic (variable / expression / numeric) key tracked as a
    /// generic `array<K, V>` level. Carries the inferred key type.
    ///
    /// `slot` holds the index a written-out integer named, which lets a
    /// write update that positional slot of a tuple-style shape instead
    /// of collapsing the shape into the generic pair.
    Keyed {
        key_type: PhpType,
        slot: Option<usize>,
    },
    /// A trailing `[]` append, as in `$var['a'][] = …`. Only ever the
    /// last segment of a chain.
    Append,
}

/// Merge a nested array write with a mix of literal-string and dynamic
/// key segments into the base type.
///
/// Literal segments build/extend array shapes (like
/// [`merge_nested_shape_keys`]); dynamic segments build/extend generic
/// `array<K, V>` levels (like [`merge_keyed_type`]). For example,
/// merging `['data', $count, 'earnings']` with value `Decimal` into a
/// bare `array` produces:
///   `array{data: array<int, array{earnings: Decimal}>}`
///
/// A dynamic write onto an existing shape may land on any of its keys, so
/// the shape widens to the `array<K, V>` its entries and the written pair
/// describe together.
///
/// A trailing [`ArrayWriteKey::Append`] appends to the innermost level,
/// so `$rows[$id][] = $name` starting from `array{}` produces
/// `array<int, list<string>>`. Appending to a shape that tracks literal
/// keys adds the entry PHP's next free integer key would take.
pub(super) fn merge_nested_array_write(
    base: &PhpType,
    keys: &[ArrayWriteKey],
    value_type: &PhpType,
) -> PhpType {
    // Every level a write descends through holds at least the entry the
    // write put there, so the result is non-empty even when the tracked
    // key/value pair says nothing about which keys those are. That is what
    // lets a later `foreach` over the array know its body runs, and so
    // keeps a `null` sentinel assigned ahead of that loop from surviving
    // it. A write on only some paths gives the promise back at the branch
    // join, where `array{} | non-empty-array<K, V>` widens to
    // `array<K, V>`.
    merge_nested_array_write_inner(base, keys, value_type).non_empty_array_form()
}

fn merge_nested_array_write_inner(
    base: &PhpType,
    keys: &[ArrayWriteKey],
    value_type: &PhpType,
) -> PhpType {
    debug_assert!(!keys.is_empty());
    match &keys[0] {
        ArrayWriteKey::Shape(key) => {
            if keys.len() == 1 {
                merge_shape_key(base, key, value_type)
            } else {
                let inner_base = shape_slot_base(base, key);
                let inner_merged = merge_nested_array_write(&inner_base, &keys[1..], value_type);
                merge_shape_key(base, key, &inner_merged)
            }
        }
        ArrayWriteKey::Keyed { key_type, slot } => {
            // A written-out index into a shape that already has that
            // positional slot updates it in place, keeping the tuple's
            // arity and the slots the write did not name. Folding it into
            // the generic pair instead would union every slot's type
            // together, so reading any one of them back gives the union.
            if let Some(slot) = slot
                && let Some(entries) = base.shape_entries()
                && let Some(index) = positional_entry_index(entries, *slot)
            {
                let inner_merged = if keys.len() == 1 {
                    value_type.clone()
                } else {
                    merge_nested_array_write(&entries[index].value_type, &keys[1..], value_type)
                };
                let mut updated: Vec<ShapeEntry> = entries.to_vec();
                updated[index].value_type = inner_merged.widen_scalar_literals();
                updated[index].optional = false;
                return PhpType::array_shape(updated);
            }
            let inner_merged = if keys.len() == 1 {
                value_type.clone()
            } else {
                let inner_base = keyed_slot_base(base);
                merge_nested_array_write(&inner_base, &keys[1..], value_type)
            };
            merge_keyed_type(base, key_type, &inner_merged)
        }
        ArrayWriteKey::Append => {
            debug_assert_eq!(keys.len(), 1, "`[]` is only valid as the last segment");
            // A shape that tracks literal keys keeps them, and the append
            // lands on the next free integer key beside them. A positional
            // shape (`[$a, $b]`) has no such keys and takes the general
            // mutation treatment instead: the arity a literal spelled out
            // stops describing an array that is still being appended to,
            // all the more so from inside a loop, where the number of
            // appends is not the number of times the walker sees the
            // statement.
            if let TypeKind::ArrayShape(entries) = base.kind()
                && entries.iter().any(|entry| entry.key.is_some())
            {
                return append_to_shape(base, entries, value_type);
            }
            merge_push_type(base, value_type)
        }
    }
}

/// Apply an `unset($var[key1][key2]…)` element removal to a (possibly
/// nested) array type.
///
/// `keys` holds the array-access keys from outermost to innermost, each
/// `Some(literal)` for a string-literal key or `None` for a dynamic one —
/// mirroring [`ArrayWriteKey::Shape`]/[`ArrayWriteKey::Keyed`] but without
/// carrying a value type, since removal needs no new one. The innermost
/// level applies [`PhpType::after_element_unset`]; outer levels reuse the
/// same auto-vivifying descent as [`merge_nested_array_write`] to find the
/// slot the removal lands in, then write the updated slot back.
pub(super) fn apply_nested_array_unset(base: &PhpType, keys: &[Option<String>]) -> PhpType {
    debug_assert!(!keys.is_empty());
    let key = keys[0].as_deref();
    if keys.len() == 1 {
        return base.after_element_unset(key);
    }
    let inner_base = match key {
        Some(k) => shape_slot_base(base, k),
        None => keyed_slot_base(base),
    };
    let inner_updated = apply_nested_array_unset(&inner_base, &keys[1..]);
    match key {
        Some(k) => merge_shape_key(base, k, &inner_updated),
        None => {
            let key_type = base
                .iterable_key_type()
                .unwrap_or_else(|| PhpType::union(vec![PhpType::int(), PhpType::string()]));
            merge_keyed_type(base, &key_type, &inner_updated)
        }
    }
}

/// Extend a tracked shape with the entry a `[]` append writes.
///
/// PHP hands an append the next free integer key, so the shape keeps every
/// key it already tracks and gains one more. When that index is not
/// knowable — an optional integer-keyed entry may or may not be there, and
/// shifts every index after it — the shape widens to `array<K, V>` instead.
fn append_to_shape(base: &PhpType, entries: &[ShapeEntry], value_type: &PhpType) -> PhpType {
    let Some(index) = next_append_index(entries) else {
        return merge_keyed_type(base, &PhpType::int(), value_type);
    };
    // A positional entry is read back by counting the positional entries
    // before it, so it only spells the same key the append writes while no
    // explicit integer key has moved the index along.
    let positional_count = entries.iter().filter(|entry| entry.key.is_none()).count() as i64;
    let mut merged = entries.to_vec();
    merged.push(ShapeEntry {
        key: (index != positional_count).then(|| index.to_string()),
        value_type: value_type.widen_scalar_literals(),
        optional: false,
    });
    let shape = PhpType::array_shape(merged);
    if base.is_list_shape() && index == positional_count {
        PhpType::as_list_shape(shape)
    } else {
        shape
    }
}

/// The integer key a `[]` append writes to a shape holding `entries`.
///
/// Positional entries take the next free index in order, an explicit
/// integer key raises the cursor past itself, and string keys leave it
/// alone. Returns `None` when an optional integer-keyed entry leaves the
/// next index unknowable.
fn next_append_index(entries: &[ShapeEntry]) -> Option<i64> {
    let mut next: i64 = 0;
    for entry in entries {
        let index = match entry.key.as_deref() {
            None => Some(next),
            Some(key) => key.parse::<i64>().ok(),
        };
        let Some(index) = index else { continue };
        if entry.optional {
            return None;
        }
        next = next.max(index.checked_add(1)?);
    }
    Some(next)
}

/// The type an inner write should build on for the shape entry `key`.
///
/// A missing entry auto-vivifies: PHP creates an empty array there, so an
/// empty shape (rather than an unconstrained `array`) is the honest
/// starting point — it lets the nested merge below build a precise type
/// instead of unioning against `mixed`.
fn shape_slot_base(base: &PhpType, key: &str) -> PhpType {
    if let Some(value) = base.shape_value_type(key) {
        return value.clone();
    }
    // A base that tracks one value type for every key already describes
    // what sits under this one, tracked or not.
    if matches!(base.kind(), TypeKind::ArrayShape(_)) {
        return PhpType::array_shape(Vec::new());
    }
    keyed_slot_base(base)
}

/// The type an inner write should build on below a dynamic key segment.
///
/// Like [`shape_slot_base`], an unknown element type auto-vivifies to an
/// empty shape rather than an unconstrained `array`.
fn keyed_slot_base(base: &PhpType) -> PhpType {
    match base.iterable_element_type() {
        Some(elem) if !elem.is_empty() && !elem.is_mixed() => elem,
        _ => PhpType::array_shape(Vec::new()),
    }
}

/// Extract a string key from an array access index expression.
///
/// Returns `Some(key)` for string-literal keys like `'name'` or `"age"`.
/// Returns `None` for numeric keys, variable indices, and other
/// non-string-literal expressions — these are not tracked as shape
/// entries.
pub(super) fn extract_array_key_for_shape(index: &Expression<'_>) -> Option<String> {
    if let Expression::Literal(Literal::String(s)) = index {
        let key = match s.value {
            Some(bytes) => literal_bytes_to_str(bytes)?.to_string(),
            None => crate::text_scan::unquote_php_string(bytes_to_str(s.raw))
                .unwrap_or(bytes_to_str(s.raw))
                .to_string(),
        };
        // PHP casts canonical decimal-integer strings (including negatives)
        // to int keys. Keep non-canonical numeric-looking strings such as
        // `"08"`, `"+8"`, and `"1.5"` as exact shape keys.
        if is_decimal_int_array_key(&key) {
            return None;
        }
        Some(key)
    } else {
        None
    }
}

/// The literal integer an index expression spells out, if it is one.
///
/// A write through such an index can update the matching slot of a
/// tuple-style shape it already has (`$tuple[1] = …`). It deliberately
/// does not *create* a numeric-keyed shape: `$data[0] = 'x'` on a plain
/// array leaves the tracked `array<int, string>` pair alone, because a
/// written-out index is usually one of many an unrolled or generated
/// write sequence touches, not a promise about the array's arity.
pub(super) fn extract_array_write_index(index: &Expression<'_>) -> Option<usize> {
    if let Expression::Literal(Literal::Integer(int_lit)) = index {
        return int_lit.value.and_then(|v| usize::try_from(v).ok());
    }
    None
}

/// Merge a `(key, value_type)` pair into an existing `PhpType` to
/// produce an `ArrayShape`.
///
/// If `base` is already an `ArrayShape`, the key is added or updated.
/// Otherwise a new shape is created with just the given key.
///
/// Returns `PhpType::array_shape(entries)` with the merged entries.
fn merge_shape_key(base: &PhpType, key: &str, value_type: &PhpType) -> PhpType {
    // A base that tracks key and value types instead of individual keys
    // (`array<string, int>`, `list<User>`, `User[]`) still holds whatever
    // it held before the write. Rebuilding it as a one-entry shape would
    // claim the written key is the only one there, so the write folds into
    // the tracked pair instead.
    if base.is_array_like()
        && !matches!(base.kind(), TypeKind::ArrayShape(_))
        && base.iterable_key_type().is_some()
    {
        let key_type = if is_decimal_int_array_key(key) {
            PhpType::int()
        } else {
            PhpType::string()
        };
        return merge_keyed_type(base, &key_type, value_type);
    }

    let mut entries: Vec<ShapeEntry> = Vec::new();

    // Copy existing shape entries from the base type, skipping the
    // key we are about to upsert.
    if let Some(shape_entries) = base.shape_entries() {
        for entry in shape_entries {
            if entry.key.as_deref() != Some(key) {
                entries.push(entry.clone());
            }
        }
    }

    // Add/upsert the new key.
    entries.push(ShapeEntry {
        key: Some(key.to_string()),
        value_type: value_type.widen_scalar_literals(),
        optional: false,
    });

    PhpType::array_shape(entries)
}

/// The position in `entries` of the `index`th unkeyed entry.
fn positional_entry_index(entries: &[ShapeEntry], index: usize) -> Option<usize> {
    let mut positional = 0usize;
    for (slot, entry) in entries.iter().enumerate() {
        if entry.key.is_none() {
            if positional == index {
                return Some(slot);
            }
            positional += 1;
        }
    }
    None
}

/// Merge a push element type into an existing `PhpType` to produce
/// a `Generic("list", …)` type.
///
/// If `base` already has a generic value type (e.g. `list<User>`),
/// the new type is unioned with it (e.g. `list<User|Admin>`).
/// Otherwise, produces `list<value_type>`.
///
/// Returns `PhpType::list(elem_type)` or
/// `PhpType::named("array")` when no element types are available.
pub(super) fn merge_push_type(base: &PhpType, value_type: &PhpType) -> PhpType {
    // A base that already holds string keys stays a keyed array: an append
    // adds an integer key beside them, it does not make the value a list.
    if base.is_array_like()
        && base
            .iterable_key_type()
            .is_some_and(|key| !key.is_subtype_of(&PhpType::int()))
    {
        return merge_keyed_type(base, &PhpType::int(), value_type);
    }

    let mut elem_types: Vec<PhpType> = Vec::new();
    let value_type = value_type.widen_scalar_literals();

    // Extract existing element types from the base.
    let existing_elem = base.iterable_element_type();
    if let Some(existing_elem) = &existing_elem {
        for member in existing_elem.union_members() {
            if !member.is_empty() {
                elem_types.push(member.clone());
            }
        }
    }

    // Add new value type members (union-aware).
    for member in value_type.union_members() {
        if !member.is_empty() && !elem_types.iter().any(|e| e.equivalent(member)) {
            elem_types.push(member.clone());
        }
    }

    if elem_types.is_empty() {
        return PhpType::array();
    }

    let elem_type = join_element_types(elem_types, &value_type, existing_elem.as_ref());

    PhpType::list(elem_type)
}

/// Join the member types collected for one of a container's positions
/// (element or key), keeping the benevolence marker the collection dropped.
///
/// Splitting a union into its members loses the marker sitting above them,
/// and a type that was lenient on its own has to stay lenient once it is
/// inside `list<…>` / `array<…, …>`: the position's comparison is the same
/// comparison a direct return makes, so a union nobody wrote down would
/// otherwise be enforced against every declared type the moment it is
/// collected into a container. That applies just as much to the key an
/// `Arg[]` hands out as to the value beside it. The marker only survives
/// while every contributing source carried it — a member the code did
/// spell out makes the whole position worth enforcing again.
fn join_element_types(
    members: Vec<PhpType>,
    incoming: &PhpType,
    existing: Option<&PhpType>,
) -> PhpType {
    let joined = PhpType::join_runtime_value_types(members);
    let existing_is_lenient = existing.is_none_or(|ty| ty.is_empty() || ty.is_benevolent());
    if incoming.is_benevolent() && existing_is_lenient {
        return PhpType::benevolent(joined);
    }
    joined
}

/// Merge a keyed element type into an existing `PhpType` to produce
/// a `Generic("array", …)` type.
///
/// Similar to [`merge_push_type`] but preserves the key type from the
/// index expression instead of assuming sequential integer keys.
///
/// When the base already has a generic value type (e.g.
/// `array<string, User>`), the new value type is unioned with it and
/// key types are unioned as well.
///
/// Returns `PhpType::generic_array(key, val)`,
/// `PhpType::generic_array_val(val)` when no key types are
/// available, or `PhpType::named("array")` when no element types
/// are available.
pub(super) fn merge_keyed_type(
    base: &PhpType,
    key_type: &PhpType,
    value_type: &PhpType,
) -> PhpType {
    // Normalizing rebuilds a key through `kind()`, which sees straight
    // through the benevolence marker, so the leniency decision below reads
    // the types as they arrived rather than as they normalize.
    let existing_key = base.iterable_key_type();
    let normalized_key = normalize_array_key_type(key_type)
        .unwrap_or_else(|| PhpType::union(vec![PhpType::int(), PhpType::string()]));
    let value_type = value_type.widen_scalar_literals();

    // Collect existing key types from the base.
    let mut key_types: Vec<PhpType> = Vec::new();
    if let Some(normalized_existing) = existing_key
        .as_ref()
        .and_then(normalize_array_key_type)
        .filter(|key| !key.is_empty())
    {
        for member in normalized_existing.union_members() {
            if !key_types.iter().any(|e| e.equivalent(member)) {
                key_types.push(member.clone());
            }
        }
    }
    // Add new key type members.
    for member in normalized_key.union_members() {
        if !member.is_empty() && !key_types.iter().any(|e| e.equivalent(member)) {
            key_types.push(member.clone());
        }
    }

    // Collect existing value types from the base.
    let mut elem_types: Vec<PhpType> = Vec::new();
    let existing_elem = base.iterable_element_type();
    if let Some(existing_elem) = &existing_elem {
        for member in existing_elem.union_members() {
            if !member.is_empty() {
                elem_types.push(member.clone());
            }
        }
    }
    // Add new value type members.
    for member in value_type.union_members() {
        if !member.is_empty() && !elem_types.iter().any(|e| e.equivalent(member)) {
            elem_types.push(member.clone());
        }
    }

    if elem_types.is_empty() {
        return PhpType::array();
    }

    let val_type = join_element_types(elem_types, &value_type, existing_elem.as_ref());

    if key_types.is_empty() {
        // No key type information — use a single-param generic.
        PhpType::generic_array_val(val_type)
    } else {
        let k_type = join_element_types(key_types, key_type, existing_key.as_ref());
        PhpType::generic_array(k_type, val_type)
    }
}

/// Merge the operands of an array `+` / `+=`.
///
/// PHP's array union keeps every key already present on the left and adds
/// only the keys the right side contributes. Two tracked shapes therefore
/// merge into a single shape; anything looser keeps whatever key/value
/// information both sides carry instead of collapsing to a bare `array`.
pub(super) fn merge_array_plus(lhs: &PhpType, rhs: &PhpType) -> PhpType {
    if let (TypeKind::ArrayShape(lhs_entries), TypeKind::ArrayShape(rhs_entries)) =
        (lhs.kind(), rhs.kind())
        && let Some(lhs_keys) = runtime_shape_keys(lhs_entries)
        && let Some(rhs_keys) = runtime_shape_keys(rhs_entries)
    {
        // Two positional shapes union index by index, so their entries stay
        // positional. Once either side spells a key out, the entries behind
        // it no longer sit at their own index.
        let all_positional = lhs_entries
            .iter()
            .chain(rhs_entries)
            .all(|entry| entry.key.is_none());
        let mut entries: Vec<ShapeEntry> = Vec::with_capacity(lhs_entries.len());
        for (entry, key) in lhs_entries.iter().zip(&lhs_keys) {
            let rhs_match = rhs_keys
                .iter()
                .position(|other| other == key)
                .map(|index| &rhs_entries[index])
                .filter(|_| entry.optional);
            match rhs_match {
                // An optional left key may be absent at runtime, in which
                // case the right side's value for it wins.
                Some(other) => entries.push(ShapeEntry {
                    key: entry.key.clone(),
                    value_type: PhpType::union(vec![
                        entry.value_type.clone(),
                        other.value_type.clone(),
                    ]),
                    optional: other.optional,
                }),
                None => entries.push(entry.clone()),
            }
        }
        for (entry, key) in rhs_entries.iter().zip(&rhs_keys) {
            if lhs_keys.contains(key) {
                continue;
            }
            entries.push(ShapeEntry {
                key: entry
                    .key
                    .clone()
                    .or_else(|| (!all_positional).then(|| key.clone())),
                ..entry.clone()
            });
        }
        let merged = PhpType::array_shape(entries);
        return if all_positional && lhs.is_list_shape() && rhs.is_list_shape() {
            PhpType::as_list_shape(merged)
        } else {
            merged
        };
    }

    let Some(rhs_value) = rhs.iterable_element_type().filter(|v| !v.is_empty()) else {
        return PhpType::array();
    };
    let rhs_key = rhs
        .iterable_key_type()
        .unwrap_or_else(|| PhpType::union(vec![PhpType::int(), PhpType::string()]));
    merge_keyed_type(lhs, &rhs_key, &rhs_value)
}

/// Infer the source type of an array-access index expression from what the
/// shared RHS resolver made of it.
///
/// [`merge_keyed_type`] performs collection-boundary normalization exactly
/// once. Returning the exact source type here preserves distinctions such as a
/// known non-numeric string versus a broad `string`.
///
/// The caller resolves the index rather than this function, so that an index
/// PHP only builds by computing it (`$m[$line + 1]`) goes through the same
/// path an assignment's RHS does. Falling back to `int|string` for anything
/// the narrower expression resolver cannot answer is what widened an
/// all-integer key domain to the full `array-key`.
pub(super) fn infer_array_key_type(index: &Expression<'_>, resolved: &[ResolvedType]) -> PhpType {
    // Fast path: literal values.
    if let Expression::Literal(Literal::Integer(_)) = index {
        return PhpType::int();
    }

    if !resolved.is_empty() {
        let joined = ResolvedType::types_joined(resolved);
        if !joined.is_mixed() {
            return joined;
        }
    }

    PhpType::union(vec![PhpType::int(), PhpType::string()])
}

/// Normalize every possible runtime array-key branch to `int` or `string`.
///
/// PHP truncates float keys and coerces bool keys to int, while null becomes
/// the empty string key. Literal and refined scalar types must not escape
/// into `array<K, V>` payloads.
pub(super) fn normalize_array_key_type(ty: &PhpType) -> Option<PhpType> {
    fn is_non_numeric_string_domain(ty: &PhpType) -> bool {
        match ty.kind() {
            TypeKind::ClassString(_) | TypeKind::InterfaceString(_) => true,
            TypeKind::Named(name) => matches!(
                name.to_ascii_lowercase().as_str(),
                "class-string"
                    | "interface-string"
                    | "trait-string"
                    | "enum-string"
                    | "callable-string"
            ),
            TypeKind::Generic(generic) => matches!(
                generic.name.to_ascii_lowercase().as_str(),
                "class-string" | "interface-string"
            ),
            _ => false,
        }
    }

    fn collect(ty: &PhpType, normalized: &mut Vec<PhpType>) -> bool {
        match ty.kind() {
            TypeKind::Union(members) => members.iter().all(|member| collect(member, normalized)),
            TypeKind::Nullable(inner) => {
                if !collect(inner, normalized) {
                    return false;
                }
                // PHP converts the nullable branch to the empty string key.
                push_unique(normalized, PhpType::string());
                true
            }
            _ if ty.is_null() => {
                push_unique(normalized, PhpType::string());
                true
            }
            TypeKind::Literal(value) if matches!(&**value, LiteralValue::String(_)) => {
                let content = value.string_content().unwrap_or_default();
                push_unique(
                    normalized,
                    if is_decimal_int_array_key(&content) {
                        PhpType::int()
                    } else {
                        PhpType::string()
                    },
                );
                true
            }
            _ if ty.is_array_key() => {
                push_unique(normalized, PhpType::int());
                push_unique(normalized, PhpType::string());
                true
            }
            _ if ty.is_int_coercible_key() => {
                push_unique(normalized, PhpType::int());
                true
            }
            _ if ty.is_string_subtype() || is_non_numeric_string_domain(ty) => {
                // Only a *literal* decimal-integer string is known to become
                // an int key (handled above).  A broad string keeps `string`,
                // because widening it to `int|string` would mismatch every
                // `array<string, T>` the value is declared against.
                push_unique(normalized, PhpType::string());
                true
            }
            _ => false,
        }
    }

    fn push_unique(types: &mut Vec<PhpType>, member: PhpType) {
        if !types.iter().any(|existing| existing == &member) {
            types.push(member);
        }
    }

    let mut normalized = Vec::new();
    if !collect(ty, &mut normalized) || normalized.is_empty() {
        return None;
    }
    match normalized.len() {
        1 => normalized.into_iter().next(),
        _ => Some(PhpType::union(normalized)),
    }
}

// ── Array function type preservation helpers ─────────────────────────

/// Extract the nth positional argument expression (0-based).
pub(in crate::type_engine) fn nth_arg_expr<'b>(
    args: &'b ArgumentList<'b>,
    n: usize,
) -> Option<&'b Expression<'b>> {
    args.arguments.iter().nth(n).map(|arg| match arg {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    })
}

/// The types the forward walker knows for `$var_name` at `offset`,
/// through whichever of its two scope channels is live.
///
/// Argument-type resolution reaches for this before its backward
/// `@var`/`@param` text scan, because the two answer different
/// questions: the scan describes the variable at its *annotation*, while
/// the walker's scope describes it at the point of *use*, carrying
/// whatever the guards in between proved.  `array_slice($cached, …)`
/// inside `if ($cached !== null)` has to slice the non-null half, not the
/// `null|list<…>` written above the assignment.
///
/// The two channels are the same information reaching different
/// consumers: `scope_var_resolver` is the in-progress `ScopeState` the
/// walker threads down its own call tree, and the diagnostic scope cache
/// is the per-statement recording of it a diagnostic pass leaves behind
/// for the consumers the walker does not drive directly (the return-type
/// check among them).  Both are O(1)/O(log N) lookups.
pub(in crate::type_engine) fn walker_scope_types(
    var_name: &str,
    offset: u32,
    scope_var_resolver: crate::type_engine::resolver::ScopeVarResolverFn<'_>,
) -> Vec<ResolvedType> {
    let prefixed = if var_name.starts_with('$') {
        var_name.to_string()
    } else {
        format!("${}", var_name)
    };
    if let Some(resolver) = scope_var_resolver {
        let from_scope = resolver(&prefixed);
        if !from_scope.is_empty() {
            return from_scope;
        }
    }
    if super::forward_walk::is_diagnostic_scope_active()
        && !super::forward_walk::is_building_scopes()
        && let Some(types) = super::forward_walk::lookup_diagnostic_scope(&prefixed, offset)
    {
        return types;
    }
    Vec::new()
}

/// Resolve the raw iterable type of an argument expression.
///
/// Handles `$variable` (via the walker's scope, then docblock scanning)
/// and delegates to `resolve_expression_type` for method calls, property
/// access, etc.
pub(in crate::type_engine) fn resolve_arg_raw_type<'b>(
    arg_expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
        let var_text = bytes_to_str(dv.name).to_string();
        let offset = arg_expr.span().start.offset as usize;
        let from_docblock =
            docblock::find_iterable_raw_type_in_source(ctx.content, offset, &var_text)
                .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));

        // The scope the forward walker recorded for this position, from
        // whichever of its two channels is live.  Both are map reads,
        // cheap enough to consult alongside the annotation.
        let from_walker = walker_scope_types(&var_text, offset as u32, ctx.scope_var_resolver);

        // Neither channel had it, so the only thing left below is a full
        // re-resolution of the variable, and an annotation that already
        // answered is taken as-is.
        if from_walker.is_empty()
            && ctx.scope_var_resolver.is_none()
            && let Some(annotated) = from_docblock
        {
            return Some(annotated);
        }

        // Resolve the variable's type to extract the raw iterable type.
        // This handles cases like `$users = $this->getUsers();
        // array_pop($users)` where `$users` has no `@var` annotation but was
        // assigned from a method returning `list<User>`.
        //
        // Only reached when the walker's scope came up empty, and only
        // without a scope_var_resolver: falling through to
        // resolve_variable_types while the forward walker is active would
        // re-enter it, causing infinite recursion on patterns like
        // `$a['k'] = f($a['k'])`.
        let resolved = if !from_walker.is_empty() || ctx.scope_var_resolver.is_some() {
            from_walker
        } else {
            resolve_variable_types(
                &var_text,
                ctx.current_class,
                ctx.all_classes,
                ctx.content,
                offset as u32,
                ctx.class_loader,
                ctx.backend,
                ctx.loaders,
            )
        };
        let from_scope = (!resolved.is_empty())
            .then(|| crate::types::ResolvedType::types_joined(&resolved))
            // `skip_scalar` stays off: a `list<int>` carries an element type
            // every bit as usable as a `list<User>`, and asking whether the
            // element is non-scalar drops every scalar-element array back to
            // the annotation the walker has since narrowed.
            .filter(|joined| joined.extract_value_type(false).is_some());

        match (from_docblock, from_scope) {
            // The annotation is where the walker's own type came from, so
            // when the two disagree the walker has narrowed it since
            // (`assert($a !== [])` proving `non-empty-array`, or an
            // `is_array()` guard ruling out the `string` arm) and its answer
            // is the one that describes the value at this call.  Anything
            // wider than the annotation is the walker having lost the
            // generics on the way through, and the annotation still stands.
            (Some(annotated), Some(scoped)) if scoped.is_subtype_of(&annotated) => {
                return Some(scoped);
            }
            (Some(annotated), _) => return Some(annotated),
            (None, Some(scoped)) => return Some(scoped),
            (None, None) => {}
        }
    }
    // Fall back to the unified pipeline (method calls, etc.)
    super::foreach_resolution::resolve_expression_type(arg_expr, ctx)
}

/// Check whether a call expression passes the target variable to a
/// pass-by-reference parameter with a type hint, and if so, push the
/// resolved type into `results`.
///
/// For example, given `function foo(Baz &$bar): void {}` and the call
/// `foo($bar)`, this function detects that `$bar` is passed to a `&`
/// parameter typed as `Baz` and resolves `$bar` to `Baz`.
///
/// Handles standalone function calls, instance method calls, static
/// method calls, and constructor calls.
pub(super) fn try_apply_pass_by_reference_type(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ResolvedType>,
    conditional: bool,
) {
    let (argument_list, parameters, callee) = match expr {
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let func_name_offset = func_call.function.span().start.offset;
            let fl = match ctx.function_loader() {
                Some(fl) => fl,
                None => return,
            };
            let func_info = match fl(&func_name, func_name_offset) {
                Some(fi) => fi,
                None => return,
            };
            // Borrow the argument list and clone the parameters so we
            // can iterate them together.
            let parameters = func_info.parameters.clone();
            (
                &func_call.argument_list,
                parameters,
                OutParamCallee::Function(Box::new(func_info)),
            )
        }
        Expression::Call(Call::Method(method_call)) => {
            match try_resolve_method_params(method_call.object, &method_call.method, ctx) {
                Some((params, callee)) => (&method_call.argument_list, params, callee),
                None => return,
            }
        }
        Expression::Call(Call::NullSafeMethod(method_call)) => {
            match try_resolve_method_params(method_call.object, &method_call.method, ctx) {
                Some((params, callee)) => (&method_call.argument_list, params, callee),
                None => return,
            }
        }
        Expression::Call(Call::StaticMethod(static_call)) => {
            match try_resolve_static_method_params(static_call, ctx) {
                Some((params, arg_list, callee)) => (arg_list, params, callee),
                None => return,
            }
        }
        Expression::Instantiation(inst) => match try_resolve_constructor_params(inst, ctx) {
            Some((params, arg_list, callee)) => (arg_list, params, callee),
            None => return,
        },
        _ => return,
    };

    // Bind arguments to parameters following PHP's rules so that named
    // arguments consult the parameter they actually target, not the one at
    // their ordinal position in the call.
    let bound = crate::call_args::bind_args_to_params(&parameters, argument_list);

    for (param_index, (param, arg_expr)) in parameters.iter().zip(bound.iter()).enumerate() {
        let arg_expr = match arg_expr {
            Some(expr) => *expr,
            None => continue,
        };

        // Check if this argument is our target variable.
        let is_our_var = match arg_expr {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name) == ctx.var_name,
            _ => false,
        };
        if !is_our_var {
            continue;
        }

        // Check if the corresponding parameter is pass-by-reference
        // with a type hint.
        if param.is_reference
            && let Some(out_hint) = crate::type_engine::call_resolution::effective_out_type(
                param,
                param_index,
                &callee,
                ctx.backend,
            )
        {
            let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &out_hint,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if !resolved.is_empty() {
                if !conditional {
                    results.clear();
                }
                ResolvedType::extend_unique(
                    results,
                    ResolvedType::from_classes_with_hint(resolved, out_hint),
                );
            }
        }
    }
}

/// Resolve parameters for an instance method call.
///
/// Currently only handles `$this->method()` where the current class
/// is known.  General variable receiver resolution is deferred to the
/// forward-walking scope model to avoid re-entrant variable resolution.
fn try_resolve_method_params(
    object: &Expression<'_>,
    method: &ClassLikeMemberSelector<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<(crate::types::SharedVec<ParameterInfo>, OutParamCallee)> {
    let method_name = match method {
        ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
        _ => return None,
    };

    // Only handle `$this->method()` — we know the current class.
    match object {
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {}
        _ => return None,
    }

    let method_info = ctx.current_class.get_method(method_name)?;
    Some((
        method_info.parameters.clone(),
        OutParamCallee::Method(Arc::new(ctx.current_class.clone()), atom(method_name)),
    ))
}

/// Resolve parameters for a static method call.
fn try_resolve_static_method_params<'a>(
    static_call: &'a StaticMethodCall<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<(
    crate::types::SharedVec<ParameterInfo>,
    &'a ArgumentList<'a>,
    OutParamCallee,
)> {
    let method_name = match &static_call.method {
        ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
        _ => return None,
    };

    let class_name = match static_call.class {
        Expression::Self_(_) | Expression::Static(_) => ctx.current_class.name.to_string(),
        Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string())?,
        Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
        _ => return None,
    };

    let cls = (ctx.class_loader)(&class_name)?;
    let method_info = cls.get_method(method_name)?;
    Some((
        method_info.parameters.clone(),
        &static_call.argument_list,
        OutParamCallee::Method(cls, atom(method_name)),
    ))
}

/// Resolve parameters for a constructor call (`new Cls(...)`).
fn try_resolve_constructor_params<'a>(
    inst: &'a Instantiation<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<(
    crate::types::SharedVec<ParameterInfo>,
    &'a ArgumentList<'a>,
    OutParamCallee,
)> {
    let class_name = match inst.class {
        Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
        Expression::Self_(_) | Expression::Static(_) => ctx.current_class.name.to_string(),
        Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string())?,
        _ => return None,
    };

    let args = inst.argument_list.as_ref()?;
    let cls = (ctx.class_loader)(&class_name)?;
    let ctor = cls.get_method("__construct")?;
    Some((
        ctor.parameters.clone(),
        args,
        OutParamCallee::Method(cls, atom("__construct")),
    ))
}

#[cfg(test)]
#[path = "resolution_tests.rs"]
mod tests;
