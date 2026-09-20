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
mod reachability;
mod scope_state;
mod snapshot_narrowing;
mod static_locals;

pub(crate) use assignment::*;
pub(crate) use callable_inference::*;
pub(crate) use closures::*;
pub(crate) use cond_narrowing::*;
pub(crate) use control_flow::*;
pub(crate) use diagnostic_cache::*;
pub(crate) use diagnostic_walk::*;
pub(crate) use loop_control::*;
pub(crate) use reachability::*;
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

        // On the per-request path, when the cursor is inside a ternary
        // instanceof branch or match(true) arm, apply narrowing to the
        // scope so the variable lookup sees the narrowed type.
        if cursor_inside_stmt && !record_snapshots {
            let expr_opt = match stmt {
                Statement::Expression(es) => Some(es.expression),
                Statement::Return(ret) => ret.value,
                _ => None,
            };
            if let Some(expr) = expr_opt {
                apply_cursor_ternary_narrowing(expr, scope, ctx);
            }

            // Also apply narrowing inside if/while conditions.
            // E.g. `if ($e instanceof Foo && $e->errorInfo)` — the
            // cursor on `$e->errorInfo` needs instanceof narrowing.
            match stmt {
                Statement::If(if_stmt) => {
                    // An `elseif`'s own condition narrows its later `&&`
                    // operands exactly as the leading `if`'s does, so both
                    // are offered the cursor pass.
                    for condition in
                        std::iter::once(if_stmt.condition).chain(elseif_conditions(&if_stmt.body))
                    {
                        let cond_span = condition.span();
                        if ctx.cursor_offset >= cond_span.start.offset
                            && ctx.cursor_offset <= cond_span.end.offset
                        {
                            apply_cursor_ternary_narrowing(condition, scope, ctx);
                            break;
                        }
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
                Statement::DoWhile(dw) => {
                    let cond_span = dw.condition.span();
                    if ctx.cursor_offset >= cond_span.start.offset
                        && ctx.cursor_offset <= cond_span.end.offset
                    {
                        apply_cursor_ternary_narrowing(dw.condition, scope, ctx);
                    }
                }
                Statement::Foreach(foreach) => {
                    let expr_span = foreach.expression.span();
                    if ctx.cursor_offset >= expr_span.start.offset
                        && ctx.cursor_offset <= expr_span.end.offset
                    {
                        apply_cursor_ternary_narrowing(foreach.expression, scope, ctx);
                    }
                }
                // A `for` header holds a comma-separated condition list, so
                // each entry needs its own containment check.
                Statement::For(for_stmt) => {
                    for condition in for_stmt.conditions.iter() {
                        let cond_span = condition.span();
                        if ctx.cursor_offset >= cond_span.start.offset
                            && ctx.cursor_offset <= cond_span.end.offset
                        {
                            apply_cursor_ternary_narrowing(condition, scope, ctx);
                            break;
                        }
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
    let mut scope = ScopeState::new();

    if !is_static {
        seed_this(&mut scope, ctx);
    }

    let method_name = method_ctx.map(|(n, _)| n);
    let has_scope_attr = method_ctx.is_some_and(|(_, s)| s);
    seed_params(
        &mut scope,
        parameters,
        method_span_start,
        method_name,
        has_scope_attr,
        ctx,
    );

    // Suspend snapshot recording: this is a transient lookup of
    // `var_name`'s type, not the authoritative scope build, so it must
    // not write into an active diagnostic scope cache.  This body's
    // `return`s likewise belong to it, not to any closure being walked
    // for its by-reference captures further out.
    {
        let _suspend = suspend_snapshot_recording();
        let _barrier = suspend_return_edges();
        let body: Vec<&Statement<'_>> = body_statements.collect();
        static_locals::seed_static_locals(&mut scope, &body, ctx);
        walk_body_forward(body.iter().copied(), &mut scope, ctx);
    }

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
    // transient lookup must not pollute an active diagnostic scope cache.
    {
        let _suspend = suspend_snapshot_recording();
        let _barrier = suspend_return_edges();
        let body: Vec<&Statement<'_>> = func.body.statements.iter().collect();
        static_locals::seed_static_locals(&mut scope, &body, ctx);
        walk_body_forward(body.iter().copied(), &mut scope, ctx);
    }

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

    seed_superglobals(&mut scope);

    // Suspend snapshot recording (see `resolve_in_method_body`): this
    // transient lookup must not pollute an active diagnostic scope
    // cache.  Its statements can even belong to another file (return-type
    // inference of a called function), whose offsets would otherwise
    // collide with the outer file's.
    {
        let _suspend = suspend_snapshot_recording();
        let _barrier = suspend_return_edges();
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
/// `global` keyword resolution.  This runs the standard forward walk
/// over the top-level statements (skipping class/function/interface/
/// enum/trait bodies, which have isolated scopes).
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
    let _barrier = suspend_return_edges();
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
