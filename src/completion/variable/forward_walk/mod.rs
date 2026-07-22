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
/// The old backward scanner (now removed) resolved one variable at a
/// time by walking backward from the cursor, recursively calling itself
/// for each RHS variable reference.  That caused O(depth × file_size)
/// work per variable lookup.
///
/// This forward walker replaces that recursion with a single forward pass:
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
/// # Phases
///
/// - **Phase 1** (completion): wired into the completion path.  The
///   forward walker is called per-request with `cursor_offset` set to
///   the cursor position.  Only the target variable's type is read.
/// - **Phase 2** (diagnostics): [`build_diagnostic_scopes`] walks every
///   function/method body in the file once (`cursor_offset = u32::MAX`)
///   and records scope snapshots at each statement boundary in a
///   thread-local [`DIAGNOSTIC_SCOPE`] cache.  When
///   `resolve_variable_types` is called for a diagnostic span, it
///   checks the cache first via [`lookup_diagnostic_scope`] and returns
///   the pre-computed types in O(log N) time, eliminating the
///   O(N x depth x file_size) cost of per-span backward scanning.
use std::cell::{Cell, RefCell};

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::types::ResolvedType;

mod assignment;
mod callable_inference;
mod closures;
mod cond_narrowing;
mod control_flow;
mod diagnostic_cache;
mod diagnostic_walk;
mod loop_control;
mod scope_state;
mod snapshot_narrowing;

pub(crate) use assignment::*;
pub(crate) use callable_inference::*;
pub(crate) use closures::*;
pub(crate) use cond_narrowing::*;
pub(crate) use control_flow::*;
pub(crate) use diagnostic_cache::*;
pub(crate) use diagnostic_walk::*;
pub(crate) use loop_control::*;
pub(crate) use scope_state::*;
pub(crate) use snapshot_narrowing::*;

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
        // that closure's scope instead.
        let stmt_span = stmt.span();
        if ctx.cursor_offset >= stmt_span.start.offset
            && ctx.cursor_offset <= stmt_span.end.offset
            && try_enter_closure(stmt, scope, ctx)
        {
            return;
        }

        // On the completion path, when the cursor is inside a ternary
        // instanceof branch or match(true) arm, apply narrowing to the
        // scope so the variable lookup sees the narrowed type.
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

        process_statement(stmt, scope, ctx);

        if cursor_inside_stmt && !record_snapshots {
            let expr_opt = match stmt {
                Statement::Expression(es) => Some(es.expression),
                Statement::Return(ret) => ret.value,
                _ => None,
            };
            if let Some(expr) = expr_opt {
                apply_cursor_ternary_narrowing(expr, scope, ctx);
            }

            // Also apply narrowing inside if/while/for conditions.
            // E.g. `if ($e instanceof Foo && $e->errorInfo)` — the
            // cursor on `$e->errorInfo` needs instanceof narrowing.
            match stmt {
                Statement::If(if_stmt) => {
                    let cond_span = if_stmt.condition.span();
                    if ctx.cursor_offset >= cond_span.start.offset
                        && ctx.cursor_offset <= cond_span.end.offset
                    {
                        apply_cursor_ternary_narrowing(if_stmt.condition, scope, ctx);
                    }
                }
                Statement::While(while_stmt) => {
                    let cond_span = while_stmt.condition.span();
                    if ctx.cursor_offset >= cond_span.start.offset
                        && ctx.cursor_offset <= cond_span.end.offset
                    {
                        apply_cursor_ternary_narrowing(while_stmt.condition, scope, ctx);
                    }
                }
                _ => {}
            }
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
            walk_closures_in_statement(stmt, closure_scope, ctx);
            record_scope_snapshot(stmt_span.end.offset, scope);
        }
    }
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
    // Collect iterators up front so they can be reused across the cache
    // populate path and the standard walk path without ownership issues.
    let params_vec: Vec<&'b FunctionLikeParameter<'b>> = parameters.collect();
    let stmts_vec: Vec<&'b Statement<'b>> = body_statements.collect();

    // ── Hover scope cache ────────────────────────────────────────────────
    // The hover scope cache records snapshots at each statement's START
    // offset (before the statement is processed).  This works well for
    // member-access resolution within a statement (which needs the scope
    // from before the statement), but returns the wrong type for variable
    // hover on the LHS of an assignment: hovering `$x` in `$x = new Foo()`
    // should show the post-assignment type (`Foo`), not the pre-assignment
    // type.  Detecting all edge cases (nudged offsets, nested blocks,
    // closures) is fragile, so variable resolution always uses the
    // standard walk which processes statements up to the cursor and
    // returns the correct post-assignment scope.
    //
    // The cache IS still populated here (if not yet present) so that
    // other consumers (diagnostics member-access lookups via
    // `lookup_diagnostic_scope`) benefit from it.
    if !is_diagnostic_scope_active()
        && is_hover_scope_cache_active()
        && !hover_scope_has_method(method_span_start)
    {
        // Activate a temporary diagnostic scope so that walk_body_forward
        // records snapshots at every statement boundary.
        let _diag_guard = with_diagnostic_scope_cache();
        // This is a dedicated scope-building walk (it harvests the temp
        // cache below), so its snapshots must be recorded even when we
        // were reached from a nested resolution that suspended recording.
        let _resume_guard = resume_snapshot_recording();

        // Build a full-walk context (cursor at u32::MAX = walk entire body).
        let full_ctx = ForwardWalkCtx {
            cursor_offset: u32::MAX,
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            class_loader: ctx.class_loader,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope: ctx.top_level_scope.clone(),
        };

        let mut scope = ScopeState::new();
        if !is_static {
            seed_this(&mut scope, ctx.current_class);
        }
        let method_name = method_ctx.map(|(n, _)| n);
        let has_scope_attr = method_ctx.is_some_and(|(_, s)| s);

        seed_params(
            &mut scope,
            params_vec.iter().copied(),
            method_span_start,
            method_name,
            has_scope_attr,
            &full_ctx,
        );

        // Record the scope at the method body start.
        record_scope_snapshot(method_span_start, &scope);

        // Walk the full body to populate DIAGNOSTIC_SCOPE with snapshots.
        walk_body_for_diagnostics(stmts_vec.iter().copied(), &mut scope, &full_ctx);

        // Harvest the snapshots from the temporary diagnostic scope.
        let snapshots = take_diagnostic_scope_map();

        // The _diag_guard drop will clear DIAGNOSTIC_SCOPE; store
        // snapshots in the hover cache before that happens (we already
        // took ownership of the map above).
        populate_hover_scope_cache_for_method(method_span_start, snapshots);
        // Do NOT look up the variable from the freshly-populated cache.
        // The standard walk below will produce the correct result.
    }

    // ── Standard walk (diagnostics path or hover cache not active) ───────
    let mut scope = ScopeState::new();

    // Seed `$this` for non-static class methods.
    if !is_static {
        seed_this(&mut scope, ctx.current_class);
    }

    // Seed scope with parameter types.
    let method_name = method_ctx.map(|(n, _)| n);
    let has_scope_attr = method_ctx.is_some_and(|(_, s)| s);
    seed_params(
        &mut scope,
        params_vec.iter().copied(),
        method_span_start,
        method_name,
        has_scope_attr,
        ctx,
    );

    // Walk the body forward.  Suspend snapshot recording: this is a
    // transient lookup of `var_name`'s type, not the authoritative scope
    // build, so it must not write into an active diagnostic scope cache.
    {
        let _suspend = suspend_snapshot_recording();
        walk_body_forward(stmts_vec.iter().copied(), &mut scope, ctx);
    }

    // Read the target variable from the scope.
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

/// Resolve the target variable from a standalone function body using
/// the forward walker.
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

pub(crate) fn resolve_in_function_body<'b>(
    var_name: &str,
    func: &'b Function<'b>,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let mut scope = ScopeState::new();

    // Seed scope with parameter types.
    seed_params(
        &mut scope,
        func.parameter_list.parameters.iter(),
        func.span().start.offset,
        None,
        false, // standalone functions are never scope methods
        ctx,
    );

    // Walk the body forward.  Suspend snapshot recording (see
    // `resolve_in_method_body`): this transient lookup must not pollute
    // an active diagnostic scope cache.
    {
        let _suspend = suspend_snapshot_recording();
        walk_body_forward(func.body.statements.iter(), &mut scope, ctx);
    }

    // Read the target variable.
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
    let mut scope = ScopeState::new();

    // Seed superglobals so that `$_GET`, `$_POST`, etc. resolve.
    seed_superglobals(&mut scope);

    // Walk the top-level statements forward.  Suspend snapshot recording
    // (see `resolve_in_method_body`): this transient lookup must not
    // pollute an active diagnostic scope cache.  Its statements can even
    // belong to another file (return-type inference of a called function),
    // whose offsets would otherwise collide with the outer file's.
    {
        let _suspend = suspend_snapshot_recording();
        walk_body_forward(statements, &mut scope, ctx);
    }

    // Return `Some` when the variable exists in scope (even with
    // empty types), `None` when it was never seen.
    if scope.contains(var_name) {
        Some(scope.get(var_name).to_vec())
    } else {
        None
    }
}

/// Walk top-level statements to build a scope of variable types for
/// `global` keyword resolution.  This is a lightweight walk that only
/// processes expression-level assignments (and skips class/function/
/// interface/enum/trait bodies, which have isolated scopes).
pub(crate) fn walk_top_level_for_globals<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    seed_superglobals(scope);
    // Suspend snapshot recording (see `resolve_in_method_body`): this
    // transient `global`-resolution walk must not pollute an active
    // diagnostic scope cache.
    let _suspend = suspend_snapshot_recording();
    walk_body_forward(statements, scope, ctx);
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

    // Scan the source text for `yield $varName` within the enclosing
    // function body.  We search a window around the cursor.
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

    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
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
