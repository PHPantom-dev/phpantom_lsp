/// Forward-walking scope model for variable type resolution.
///
/// This module implements a single top-to-bottom pass through a function
/// or method body, maintaining a mutable type map (`ScopeState`) that
/// records each variable's type as assignments are encountered.  When the
/// walk reaches the cursor position it stops and the caller reads the
/// target variable's type from the map — an O(1) `HashMap` lookup with
/// zero recursion.
///
/// # Architecture
///
/// A backward scanner that resolves one variable at a time from the
/// cursor, recursively resolving each RHS variable reference, costs
/// O(depth × file_size) per lookup.  The forward walker replaces that
/// recursion with a single forward pass:
///
/// 1. Seed `ScopeState` with parameter types.
/// 2. Walk statements top-to-bottom.  At each assignment `$a = expr`,
///    evaluate `expr` by reading other variables from the scope (O(1)
///    map lookups) and store the result under `$a`.
/// 3. At the cursor, read the target variable from the scope.
///
/// There is no recursion on variable resolution, no depth limit, and
/// every variable resolved during the walk is available to subsequent
/// statements for free.
///
/// # Consumers
///
/// - **Per-request lookups** (completion, hover, go-to-definition,
///   signature help): the walker is called with `cursor_offset` set to
///   the request position and only the target variable's type is read.
/// - **Diagnostics**: [`build_diagnostic_scopes`] walks every
///   function/method body in the file once (`cursor_offset = u32::MAX`)
///   and records scope snapshots at each statement boundary in a
///   thread-local [`DIAGNOSTIC_SCOPE`] cache.  When
///   `resolve_variable_types` is called for a diagnostic span, it
///   checks the cache first via [`lookup_diagnostic_scope`] and returns
///   the pre-computed types in O(log N) time instead of re-walking the
///   body for every span.
/// - **Member-reference searches**, which ask about an access or two in
///   a candidate file rather than all of them, populate the same cache
///   through [`build_diagnostic_scopes_for_offsets`], which walks only
///   the bodies holding the offsets it is given.
use std::cell::{Cell, RefCell};

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::types::ResolvedType;

mod array_assignment;
mod assignment;
mod assignment_deps;
mod by_ref;
mod callable_inference;
mod closures;
mod cond_narrowing;
mod condition;
mod control_flow;
mod diagnostic_cache;
mod diagnostic_walk;
mod expr;
mod foreach;
mod if_else;
mod loop_control;
mod loops;
mod param_seeding;
mod reachability;
mod readonly_properties;
mod receiver_mutation;
mod scope_state;
mod static_locals;
mod throw_points;
mod var_docblocks;
mod walk_ctx;
mod while_for;

pub(crate) use array_assignment::*;
pub(crate) use assignment::*;
pub(crate) use assignment_deps::*;
pub(crate) use by_ref::*;
pub(crate) use callable_inference::*;
pub(crate) use closures::*;
pub(crate) use cond_narrowing::*;
pub(crate) use condition::*;
pub(crate) use control_flow::*;
pub(crate) use diagnostic_cache::*;
pub(crate) use diagnostic_walk::*;
pub(crate) use expr::*;
pub(crate) use foreach::*;
pub(crate) use if_else::*;
pub(crate) use loop_control::*;
pub(crate) use loops::*;
pub(crate) use param_seeding::*;
pub(crate) use reachability::*;
pub(crate) use receiver_mutation::*;
pub(crate) use scope_state::*;
pub(crate) use throw_points::*;
pub(crate) use var_docblocks::*;
pub(crate) use walk_ctx::*;
pub(crate) use while_for::*;

/// Walk a sequence of statements top-to-bottom, updating `scope` at
/// each step.  Stops when a statement's start offset reaches or exceeds
/// `ctx.cursor_offset`.
///
/// After this function returns, `scope.get("$varName")` contains the
/// types of `$varName` at the cursor position.
pub(crate) fn walk_body_forward<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // When the diagnostic scope cache is active, record snapshots at
    // every statement boundary — even inside branches (if/else, try,
    // foreach, loops).  Without this, member accesses inside branch
    // bodies would only see the scope from before the branch started,
    // missing assignments made inside the branch and causing false-
    // positive diagnostics.
    let record_snapshots = is_diagnostic_scope_active();

    for stmt in statements {
        // Stop when we have passed the cursor.  We use `>` rather than
        // `>=` so that a statement whose start offset exactly equals the
        // cursor is still processed.  This matters when hovering on the
        // LHS variable of an assignment: the cursor sits at the first
        // token of the statement, and the user expects to see the *result*
        // type of the assignment, not the type from before it.
        if stmt.span().start.offset > ctx.cursor_offset {
            break;
        }

        // Check whether the cursor is inside a closure/arrow function
        // within this statement.  If so, we need to resolve within
        // that closure's scope instead.  The closure's body is walked
        // with a cursor scope of its own; the statement's is not the
        // place to record anything the search walks on the way there.
        let stmt_span = stmt.span();
        if ctx.cursor_offset >= stmt_span.start.offset
            && ctx.cursor_offset <= stmt_span.end.offset
            && try_enter_closure(stmt, scope, &ctx.with_cursor_scope(None))
        {
            return;
        }

        let cursor_inside_stmt = ctx.cursor_offset >= stmt_span.start.offset
            && ctx.cursor_offset <= stmt_span.end.offset;

        // Snapshot the pre-statement scope for the closure walk below.
        // References inside this statement's own expression (including
        // closure/arrow bodies) evaluate before the statement's
        // assignment takes effect, so they must see the pre-assignment
        // types rather than the reassigned result.
        let pre_stmt_scope = if record_snapshots {
            Some(scope.clone())
        } else {
            None
        };

        if record_snapshots {
            record_scope_snapshot(stmt_span.start.offset, scope);
        }

        if cursor_inside_stmt {
            // The cursor can sit deep inside this statement's expression,
            // where the scope is not the one around the statement: in the
            // right operand of `&&`, in a ternary arm, after a write the
            // expression made earlier.  The expression walk records the
            // scope it reached the cursor with, and that is the answer.
            let at_cursor = CursorScope::default();
            process_statement(stmt, scope, &ctx.with_cursor_scope(Some(&at_cursor)));
            if let Some(at_cursor) = at_cursor.take() {
                *scope = at_cursor;
            }
        } else {
            process_statement(stmt, scope, ctx);
        }

        // When the diagnostic scope cache is active, walk closure and
        // arrow function bodies found in this statement.  This is the
        // same call that `walk_body_for_diagnostics` makes for
        // top-level statements, but here it also covers closures
        // inside branch bodies (if/else, foreach, try, etc.) where
        // the scope reflects narrowing and bindings from the enclosing
        // block.
        if record_snapshots {
            let closure_scope = pre_stmt_scope.as_ref().unwrap_or(scope);
            walk_closures_in_statement(stmt, closure_scope, scope, ctx);
            record_scope_snapshot(stmt_span.end.offset, scope);
        }
    }
}

thread_local! {
    /// When `Some`, the scope each walk to one position reached it with,
    /// keyed by [`WalkKey`].  Activated with the rest of the request-scoped
    /// type-engine memos.
    ///
    /// A walk answers every name asked about at its position, so the
    /// variables and member paths one request resolves at the cursor
    /// (`$this->a`, `$this->a->b()`, `$x`) share one walk of the body
    /// instead of walking it again per name.
    static WALK_MEMO: RefCell<Option<std::collections::HashMap<WalkKey, ScopeState>>> =
        const { RefCell::new(None) };
}

/// What identifies one walk to a position: the source (`(pointer,
/// length)`, see `variable::resolution`'s `VarQueryKey`), the span of the
/// walked body, the cursor, the class the body belongs to, the
/// body-inference context it is walked under (a body read for its return
/// type seeds its parameters from the call site), and whether `global`
/// statements could read the file's top-level scope (a walk started while
/// that scope is still being built goes without it).
type WalkKey = (
    (usize, usize),
    (u32, u32),
    u32,
    crate::atom::Atom,
    u64,
    bool,
);

/// The guard [`with_walk_memo`] hands back.
pub(crate) type WalkMemoGuard =
    crate::type_engine::MemoGuard<std::collections::HashMap<WalkKey, ScopeState>>;

/// Activate [`WALK_MEMO`] for the current thread.  Nested activation is a
/// no-op.
pub(crate) fn with_walk_memo() -> WalkMemoGuard {
    crate::type_engine::activate_memo(&WALK_MEMO)
}

/// The scope a walk of the body spanning `body` reaches the cursor with:
/// `walk`'s answer, unless this request already walked there.
fn walked_scope(
    ctx: &ForwardWalkCtx<'_>,
    body: (u32, u32),
    walk: impl FnOnce() -> ScopeState,
) -> ScopeState {
    let key: WalkKey = (
        (ctx.content.as_ptr() as usize, ctx.content.len()),
        body,
        ctx.cursor_offset,
        ctx.current_class.name,
        crate::type_engine::call_resolution::body_inference_context(),
        ctx.top_level_scope.is_some(),
    );
    if let Some(hit) = WALK_MEMO.with(|cell| {
        cell.borrow()
            .as_ref()
            .and_then(|memo| memo.get(&key).cloned())
    }) {
        return hit;
    }
    let scope = walk();
    WALK_MEMO.with(|cell| {
        if let Some(memo) = cell.borrow_mut().as_mut() {
            memo.insert(key, scope.clone());
        }
    });
    scope
}

/// Where the last of `statements` ends, or `0` when there are none.
fn statements_end(statements: &[&Statement<'_>]) -> u32 {
    statements.last().map_or(0, |s| s.span().end.offset)
}

/// Resolve the target variable from a method body using the forward
/// walker.
///
/// This is the main entry point called from `resolve_variable_in_members`.
/// It seeds the scope with parameter types and walks the method body
/// forward to the cursor.
pub(crate) fn resolve_in_method_body<'b>(
    var_name: &str,
    parameters: impl Iterator<Item = &'b FunctionLikeParameter<'b>>,
    body_statements: impl Iterator<Item = &'b Statement<'b>>,
    method_span_start: u32,
    method_ctx: Option<(&str, bool)>,
    is_static: bool,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let ctx = &ctx.for_declaration(method_span_start);
    let body: Vec<&Statement<'_>> = body_statements.collect();
    let scope = walked_scope(ctx, (method_span_start, statements_end(&body)), || {
        let mut scope = ScopeState::new();

        let method_name = method_ctx.map(|(n, _)| n);
        if !is_static {
            seed_this(&mut scope, ctx);
            readonly_properties::seed_constructor_readonly_properties(&mut scope, method_name, ctx);
        }

        let has_scope_attr = method_ctx.is_some_and(|(_, s)| s);
        seed_params(
            &mut scope,
            parameters,
            method_span_start,
            method_name,
            has_scope_attr,
            ctx,
        );

        // Suspend snapshot recording: this is a transient lookup, not the
        // authoritative scope build, so it must not write into an active
        // diagnostic scope cache.  This body's `return`s likewise belong
        // to it, not to any closure being walked for its by-reference
        // captures further out.
        let _suspend = suspend_snapshot_recording();
        let _barrier = suspend_return_edges();
        static_locals::seed_static_locals(&mut scope, &body, ctx);
        walk_body_forward(body.iter().copied(), &mut scope, ctx);
        scope
    });

    // Return `Some(types)` when the variable exists in scope (even if
    // the type list is empty — that means "unknown/narrowed-away"),
    // and `None` when the variable was never seen by the forward walker.
    if scope.contains(var_name) {
        let types = scope.get(var_name).to_vec();
        // When the variable is in scope but has no resolved types and
        // the enclosing function returns a Generator, try reverse
        // inference from yield statements.
        if types.is_empty()
            && let Some(inferred) = try_generator_yield_inference(var_name, ctx)
        {
            return Some(inferred);
        }
        Some(types)
    } else {
        // Variable was never assigned.  Try generator yield reverse
        // inference: if the variable appears as `yield $var` and the
        // enclosing function returns Generator<TKey, TValue>, infer
        // the variable's type as TValue.
        if let Some(inferred) = try_generator_yield_inference(var_name, ctx) {
            return Some(inferred);
        }
        None
    }
}

/// Detect whether a method has a `#[Scope]` attribute by scanning the
/// source text around the method span.  The attribute list precedes or
/// is part of the method node, so we search a window around the offset.
fn detect_scope_attribute_from_source(content: &str, method_offset: usize) -> bool {
    // Search backwards from the method offset for `#[Scope]` or
    // `#[\...\Scope]` in the preceding ~500 characters.
    let mut search_start = method_offset.saturating_sub(500);
    while search_start < content.len() && !content.is_char_boundary(search_start) {
        search_start += 1;
    }
    let mut search_end = content.len().min(method_offset + 200);
    while search_end > search_start && !content.is_char_boundary(search_end) {
        search_end -= 1;
    }
    let region = &content[search_start..search_end];
    // Find occurrences of `#[` and check if any contain `Scope`.
    let mut pos = 0;
    while let Some(bracket_pos) = region[pos..].find("#[") {
        let abs = pos + bracket_pos;
        if let Some(end) = region[abs..].find(']') {
            let attr_text = &region[abs..abs + end + 1];
            if attr_text.contains("Scope") {
                return true;
            }
            pos = abs + end + 1;
        } else {
            break;
        }
    }
    false
}

/// Resolve the target variable from a standalone function body using
/// the forward walker.
pub(crate) fn resolve_in_function_body<'b>(
    var_name: &str,
    func: &'b Function<'b>,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let ctx = &ctx.for_declaration(func.span().start.offset);
    let func_span = func.span();
    let scope = walked_scope(ctx, (func_span.start.offset, func_span.end.offset), || {
        let mut scope = ScopeState::new();

        seed_params(
            &mut scope,
            func.parameter_list.parameters.iter(),
            func.span().start.offset,
            None,
            false, // standalone functions are never scope methods
            ctx,
        );

        // Suspend snapshot recording (see `resolve_in_method_body`): this
        // transient lookup must not pollute an active diagnostic scope
        // cache.
        let _suspend = suspend_snapshot_recording();
        let _barrier = suspend_return_edges();
        let body: Vec<&Statement<'_>> = func.body.statements.iter().collect();
        static_locals::seed_static_locals(&mut scope, &body, ctx);
        walk_body_forward(body.iter().copied(), &mut scope, ctx);
        scope
    });

    // Return `Some` when the variable exists in scope (even with
    // empty types), `None` when it was never seen.
    if scope.contains(var_name) {
        let types = scope.get(var_name).to_vec();
        if types.is_empty()
            && let Some(inferred) = try_generator_yield_inference(var_name, ctx)
        {
            return Some(inferred);
        }
        Some(types)
    } else {
        if let Some(inferred) = try_generator_yield_inference(var_name, ctx) {
            return Some(inferred);
        }
        None
    }
}

/// Resolve the target variable from top-level code (outside any
/// function or class body) using the forward walker.
///
/// Seeds superglobals, then walks all top-level statements forward to
/// the cursor, skipping class/function/interface/enum/trait declarations
/// (which have their own isolated scopes).
pub(crate) fn resolve_in_top_level<'b>(
    var_name: &str,
    statements: impl Iterator<Item = &'b Statement<'b>>,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let statements: Vec<&Statement<'_>> = statements.collect();
    let span = (
        statements.first().map_or(0, |s| s.span().start.offset),
        statements_end(&statements),
    );
    let scope = walked_scope(ctx, span, || {
        let mut scope = ScopeState::new();

        seed_superglobals(&mut scope);

        // Suspend snapshot recording (see `resolve_in_method_body`): this
        // transient lookup must not pollute an active diagnostic scope
        // cache.  Its statements can even belong to another file
        // (return-type inference of a called function), whose offsets
        // would otherwise collide with the outer file's.
        let _suspend = suspend_snapshot_recording();
        let _barrier = suspend_return_edges();
        walk_body_forward(statements.iter().copied(), &mut scope, ctx);
        scope
    });

    // Return `Some` when the variable exists in scope (even with
    // empty types), `None` when it was never seen.
    if scope.contains(var_name) {
        Some(scope.get(var_name).to_vec())
    } else {
        None
    }
}

/// Walk top-level statements to build a scope of variable types for
/// `global` keyword resolution.  This runs the standard forward walk
/// over the top-level statements (skipping class/function/interface/
/// enum/trait bodies, which have isolated scopes), once per request.
pub(crate) fn top_level_scope_for_globals<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    ctx: &ForwardWalkCtx<'_>,
) -> ScopeState {
    let statements: Vec<&Statement<'_>> = statements.collect();
    let span = (
        statements.first().map_or(0, |s| s.span().start.offset),
        statements_end(&statements),
    );
    walked_scope(ctx, span, || {
        let mut scope = ScopeState::new();
        seed_superglobals(&mut scope);
        // Suspend snapshot recording (see `resolve_in_method_body`): this
        // transient `global`-resolution walk must not pollute an active
        // diagnostic scope cache.
        let _suspend = suspend_snapshot_recording();
        let _barrier = suspend_return_edges();
        walk_body_forward(statements.iter().copied(), &mut scope, ctx);
        scope
    })
}

// ─── Generator yield reverse inference ──────────────────────────────────────

/// When the enclosing function/method returns a `Generator<TKey, TValue>`,
/// scan the source text for `yield $varName` and infer the variable's type
/// as `TValue`.  This handles the pattern where a variable is yielded but
/// never explicitly assigned — its type comes from the Generator's return
/// type annotation.
fn try_generator_yield_inference(
    var_name: &str,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let return_type = ctx.enclosing_return_type.as_ref()?;
    let value_type = return_type.extract_value_type(false)?;

    let cursor = ctx.cursor_offset as usize;
    let content = ctx.content;

    // Find the enclosing function body boundaries by scanning backward
    // for the opening `{`.
    let search_before = content.get(..cursor).unwrap_or("");
    let mut brace_depth = 0i32;
    let mut body_start = None;
    for (i, ch) in search_before.char_indices().rev() {
        match ch {
            '}' => brace_depth += 1,
            '{' => {
                brace_depth -= 1;
                if brace_depth < 0 {
                    body_start = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }

    let start = body_start?;

    // Find the matching closing `}`.
    let after_open = content.get(start..).unwrap_or("");
    let mut depth = 0i32;
    let mut body_end = content.len();
    for (i, ch) in after_open.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    body_end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }

    let body = content.get(start..body_end).unwrap_or("");

    // Look for `yield $varName` or `=> $varName` in yield context.
    let yield_pattern = format!("yield {}", var_name);
    let has_yield = body.contains(&yield_pattern);

    let yield_pair_needle = format!("=> {}", var_name);
    let has_yield_pair = body.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.contains("yield ") && trimmed.contains(&yield_pair_needle)
    });

    if !has_yield && !has_yield_pair {
        return None;
    }

    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        value_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    if classes.is_empty() {
        return None;
    }
    Some(ResolvedType::from_classes(classes))
}
