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
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::ast::argument::Argument;
use mago_syntax::ast::sequence::TokenSeparatedSequence;
use mago_syntax::ast::*;

use crate::atom::{Atom, AtomMap, atom, bytes_to_str};
use crate::completion::resolver::{Loaders, VarResolutionCtx};
use crate::completion::types::narrowing;
use crate::parser::{extract_hint_type, with_parsed_program};
use crate::php_type::{PhpType, ShapeEntry};
use crate::types::{AccessKind, ClassInfo, ResolvedType};

// ─── Hover scope cache (Phase 3) ────────────────────────────────────────────
//
// When multiple hover requests arrive for the same file content (e.g. a
// test file with 80+ `assertType()` calls), each request would otherwise
// trigger a full forward walk of the method body from statement 1 to the
// cursor position.  That produces O(n²) total work.
//
// The hover scope cache amortises this to O(n) per method body: the first
// hover that hits a given method walks the **full** body once (cursor at
// u32::MAX) and stores the resulting `ScopeSnapshotMap`.  Subsequent
// hovers on the same file content look up the pre-computed snapshots in
// O(log N) time via a `BTreeMap::range` search — no re-walk at all.
//
// Cache invalidation: the key is a 64-bit FNV-1a hash of the full content
// string.  This is robust against memory reuse (two different test contents
// that happen to land at the same address would produce different hashes),
// while remaining cheap to compute (single pass, no allocation).
//
// The hover cache must not interfere with the diagnostic scope cache:
// - It is only consulted / populated when `is_diagnostic_scope_active()`
//   returns `false`.
// - `build_diagnostic_scopes` never touches `HOVER_SCOPE_CACHE`.

struct HoverScopeCache {
    /// FNV-1a hash of the content string used to build this cache.
    /// When the content changes (different hash), the cache is reset.
    content_hash: u64,
    /// method_span_start → full-body scope snapshot map.
    methods: HashMap<u32, ScopeSnapshotMap>,
}

/// Compute a fast 64-bit FNV-1a hash of a byte slice.
fn fnv1a_hash(data: &[u8]) -> u64 {
    const OFFSET: u64 = 14695981039346656037;
    const PRIME: u64 = 1099511628211;
    let mut hash = OFFSET;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

thread_local! {
    static HOVER_SCOPE_CACHE: RefCell<Option<HoverScopeCache>> =
        const { RefCell::new(None) };
}

/// Ensure the hover scope cache is active and valid for `content`.
///
/// If the cache already exists for the same content hash, this is a no-op
/// (the existing snapshots remain valid).  If the hash changed (content was
/// replaced), the cache is reset to empty so stale snapshots from the
/// previous content are discarded.
///
/// This must not be called while a diagnostic scope pass is in progress.
pub(crate) fn activate_hover_scope_cache(content: &str) {
    let content_hash = fnv1a_hash(content.as_bytes());
    HOVER_SCOPE_CACHE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_ref() {
            Some(cache) if cache.content_hash == content_hash => {
                // Cache is already valid for this content — nothing to do.
            }
            _ => {
                *borrow = Some(HoverScopeCache {
                    content_hash,
                    methods: HashMap::new(),
                });
            }
        }
    });
}

/// Returns `true` when the hover scope cache is active.
fn is_hover_scope_cache_active() -> bool {
    HOVER_SCOPE_CACHE.with(|cell| cell.borrow().is_some())
}

/// Returns `true` when the hover scope cache already has a snapshot map
/// for the given method body.
fn hover_scope_has_method(method_span_start: u32) -> bool {
    HOVER_SCOPE_CACHE.with(|cell| {
        let borrow = cell.borrow();
        borrow
            .as_ref()
            .is_some_and(|c| c.methods.contains_key(&method_span_start))
    })
}

/// Store a complete scope snapshot map for a method body in the hover
/// scope cache.
fn populate_hover_scope_cache_for_method(method_span_start: u32, snapshots: ScopeSnapshotMap) {
    HOVER_SCOPE_CACHE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if let Some(ref mut cache) = *borrow {
            cache.methods.insert(method_span_start, snapshots);
        }
    });
}

/// Extract and return the current contents of the diagnostic scope cache,
/// replacing it with an empty map.
///
/// Used by `build_method_snapshots_via_diag_cache` to harvest the
/// snapshots that were recorded by a temporary diagnostic-scope walk.
fn take_diagnostic_scope_map() -> ScopeSnapshotMap {
    DIAGNOSTIC_SCOPE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(map) => std::mem::take(map),
            None => BTreeMap::new(),
        }
    })
}

// ─── Diagnostic scope cache (Phase 2) ───────────────────────────────────────
//
// During a diagnostic pass, `build_diagnostic_scopes` walks every
// function/method body in the file once and records a scope snapshot at
// each statement boundary.  The snapshots are stored in a thread-local
// `BTreeMap<u32, HashMap<String, Vec<ResolvedType>>>` keyed by byte
// offset.  When `resolve_variable_types` is called for a diagnostic
// member-access span, `lookup_diagnostic_scope` finds the nearest
// snapshot at-or-before the requested offset and returns the variable's
// types in O(log N) time — no backward scanning, no recursion.

/// Scope snapshot map: byte offset → variable name → resolved types.
type ScopeSnapshotMap = BTreeMap<u32, AtomMap<Vec<ResolvedType>>>;

thread_local! {
    /// When `Some`, `lookup_diagnostic_scope` will consult this map.
    /// Activated by [`with_diagnostic_scope_cache`], cleared on guard
    /// drop.
    static DIAGNOSTIC_SCOPE: RefCell<Option<ScopeSnapshotMap>> =
        const { RefCell::new(None) };

    /// Set to `true` while `build_diagnostic_scopes` is populating the
    /// scope cache.  Code that would normally read from the cache should
    /// skip the lookup when this flag is set, because the cache is
    /// incomplete and may contain stale data from earlier offsets.
    static BUILDING_SCOPES: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard that clears the diagnostic scope cache on drop.
pub(crate) struct DiagnosticScopeGuard {
    owns: bool,
}

impl Drop for DiagnosticScopeGuard {
    fn drop(&mut self) {
        if self.owns {
            DIAGNOSTIC_SCOPE.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }
}

/// RAII guard that resets [`BUILDING_SCOPES`] to `false` on drop.
pub(crate) struct BuildingScopesGuard;

impl Drop for BuildingScopesGuard {
    fn drop(&mut self) {
        BUILDING_SCOPES.with(|cell: &Cell<bool>| cell.set(false));
    }
}

/// Returns `true` while `build_diagnostic_scopes` is populating the
/// scope cache.
pub(crate) fn is_building_scopes() -> bool {
    BUILDING_SCOPES.with(|cell: &Cell<bool>| cell.get())
}

/// Activate the thread-local diagnostic scope cache.
///
/// Returns a guard that clears the cache on drop.  If the cache is
/// already active (nested call), the guard is a no-op.
pub(crate) fn with_diagnostic_scope_cache() -> DiagnosticScopeGuard {
    let already_active = DIAGNOSTIC_SCOPE.with(|cell| cell.borrow().is_some());
    if already_active {
        return DiagnosticScopeGuard { owns: false };
    }
    DIAGNOSTIC_SCOPE.with(|cell| {
        *cell.borrow_mut() = Some(BTreeMap::new());
    });
    DiagnosticScopeGuard { owns: true }
}

/// Look up a variable's types from the diagnostic scope cache.
///
/// Finds the scope snapshot at the largest offset that is ≤ `offset`,
/// then returns the variable's types from that snapshot.  Returns
/// `None` when the cache is not active or no snapshot covers the
/// requested offset.
pub(crate) fn lookup_diagnostic_scope(var_name: &str, offset: u32) -> Option<Vec<ResolvedType>> {
    DIAGNOSTIC_SCOPE.with(|cell| {
        let borrow = cell.borrow();
        let map = borrow.as_ref()?;
        // Find the snapshot at-or-before `offset`.
        let (_snap_offset, snap) = map.range(..=offset).next_back()?;
        // If the variable is in the snapshot, return its types.
        // If the snapshot exists but the variable is absent, the
        // forward walker has already walked this scope region and
        // determined the variable has no known type here.  Return
        // empty rather than `None` so the caller treats the variable
        // as unresolved at this position.
        let result = snap.get(&atom(var_name)).cloned().unwrap_or_default();
        Some(result)
    })
}

/// Check whether the diagnostic scope cache is currently active.
pub(crate) fn is_diagnostic_scope_active() -> bool {
    DIAGNOSTIC_SCOPE.with(|cell| cell.borrow().is_some())
}

/// Insert a scope snapshot into the diagnostic scope cache at the given
/// byte offset.
fn record_scope_snapshot(offset: u32, scope: &ScopeState) {
    DIAGNOSTIC_SCOPE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if let Some(ref mut map) = *borrow {
            map.insert(offset, scope.locals.clone());
        }
    });
}

/// Walk a sequence of statements for diagnostic scope building.
///
/// Unlike [`walk_body_forward`] (which stops at the cursor), this walks
/// the **entire** body and records a scope snapshot at every statement
/// boundary.  The snapshots are stored in the thread-local
/// [`DIAGNOSTIC_SCOPE`] cache.
///
/// For each statement, this also discovers closure and arrow function
/// expressions and walks their bodies with properly seeded scopes so
/// that variables inside closures are fully resolved.
fn walk_body_for_diagnostics<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    for stmt in statements {
        // Record the scope at this statement's start offset so that
        // any member-access span inside this statement can look up
        // variable types that were established by prior statements.
        record_scope_snapshot(stmt.span().start.offset, scope);

        process_statement(stmt, scope, ctx);

        // Walk closure and arrow function bodies found in this
        // statement.  Each closure gets a fresh scope seeded with
        // `use()` variables from the outer scope and its own parameter
        // types (with callable inference from the enclosing call's
        // signature).  Arrow functions inherit the outer scope with
        // their parameter types added on top.  The body is fully
        // walked so that scope snapshots are recorded for every
        // statement inside the closure/arrow function.
        walk_closures_in_statement(stmt, scope, ctx);

        // Also record at the statement's end offset, which covers
        // member accesses that appear after the last statement in
        // a block (e.g. the closing `}` region).
        record_scope_snapshot(stmt.span().end.offset, scope);
    }
}

/// Scan a statement for closure/arrow function expressions and walk
/// their bodies with properly seeded scopes.
///
/// For each closure found, a fresh scope is created and seeded with
/// `use()` variables from the outer scope plus the closure's own
/// parameters.  For each arrow function, the outer scope is cloned
/// and the arrow's parameters are added on top.  The body is then
/// fully walked so that scope snapshots are recorded for every
/// statement inside the closure/arrow function.
/// Scan a statement for closure/arrow function expressions and record
/// scope "shadow" snapshots inside their bodies.
///
/// When a closure/arrow function parameter shadows an outer variable
/// (e.g. `fn(Request $request)` where the outer scope has a different
/// `$request`), the scope cache would return the outer type for lookups
/// inside the closure body.  We fix this by recording a scope snapshot
/// at the closure body's start offset that removes shadowed variables.
/// The scope cache lookup then returns `None` for those variables,
/// calling `resolve_variable_types` which would re-enter the forward
/// walker.
///
/// This approach avoids walking the entire closure body (which would
/// override `resolve_variable_types` for ALL variables, including those
/// from foreach bindings over generic collections where the outer
/// resolver produces better types).
/// Scan a **single** statement's direct expressions for closure/arrow
/// function literals and walk their bodies with properly seeded scopes.
///
/// This function intentionally does **not** recurse into nested block
/// bodies (if/while/foreach/try/switch).  Those bodies are walked by
/// [`walk_body_forward`], which calls this function for each statement
/// it processes — at that point the scope already reflects narrowing,
/// foreach bindings, and other context from the enclosing block.
///
/// Only the expressions that are directly part of this statement (the
/// condition expression, the iteration expression, echo values, etc.)
/// are scanned for closures.  Closures inside nested block bodies will
/// be picked up when `walk_body_forward` processes the inner statements.
fn walk_closures_in_statement<'b>(
    stmt: &'b Statement<'b>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match stmt {
        Statement::Expression(expr_stmt) => {
            walk_closures_in_expr(expr_stmt.expression, outer_scope, ctx, None);
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                walk_closures_in_expr(val, outer_scope, ctx, None);
            }
        }
        Statement::Echo(echo) => {
            for val in echo.values.iter() {
                walk_closures_in_expr(val, outer_scope, ctx, None);
            }
        }
        // For compound statements (if, while, foreach, etc.) only scan
        // the condition/iteration expression — not the block bodies.
        // Block bodies are walked by walk_body_forward which calls us
        // per-statement with the correct inner scope.
        Statement::If(if_stmt) => {
            walk_closures_in_expr(if_stmt.condition, outer_scope, ctx, None);
        }
        Statement::While(while_stmt) => {
            walk_closures_in_expr(while_stmt.condition, outer_scope, ctx, None);
        }
        Statement::DoWhile(dw) => {
            walk_closures_in_expr(dw.condition, outer_scope, ctx, None);
        }
        Statement::Foreach(foreach) => {
            walk_closures_in_expr(foreach.expression, outer_scope, ctx, None);
        }
        Statement::For(for_stmt) => {
            for init in for_stmt.initializations.iter() {
                walk_closures_in_expr(init, outer_scope, ctx, None);
            }
            for cond in for_stmt.conditions.iter() {
                walk_closures_in_expr(cond, outer_scope, ctx, None);
            }
            for update in for_stmt.increments.iter() {
                walk_closures_in_expr(update, outer_scope, ctx, None);
            }
        }
        Statement::Switch(switch) => {
            walk_closures_in_expr(switch.expression, outer_scope, ctx, None);
        }
        _ => {}
    }
}

/// Recursively scan an expression tree for closures/arrow functions
/// and walk their bodies with properly seeded scopes.
///
/// When a closure/arrow function is found:
/// 1. Build a scope for the closure body (fresh for closures, cloned
///    from outer for arrow functions).
/// 2. Seed the scope with parameter types (using callable inference
///    from the enclosing call's signature when parameters are untyped).
/// 3. Walk the body using [`walk_body_for_diagnostics`] so that scope
///    snapshots are recorded at every statement boundary.
///
/// The `inferred_params` argument carries callable parameter types
/// inferred from the enclosing call's signature.  When a closure is
/// found as a direct argument to a function/method call, the caller
/// passes the inferred types so untyped parameters get the correct
/// types.
fn walk_closures_in_expr<'b>(
    expr: &'b Expression<'b>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inferred_params: Option<&[PhpType]>,
) {
    match expr {
        Expression::Closure(closure) => {
            // Build a fresh scope for the closure.
            let mut closure_scope = ScopeState::new();

            // PHP closures implicitly capture `$this` from the
            // enclosing class method.  Seed it from the outer scope
            // so that `$this->prop` inside the closure resolves
            // without calling `resolve_variable_types`.
            let this_types = outer_scope.get("$this");
            if !this_types.is_empty() {
                closure_scope.set("$this", this_types.to_vec());
            }

            // Seed with `use(...)` variables from the outer scope.
            if let Some(ref use_clause) = closure.use_clause {
                for use_var in use_clause.variables.iter() {
                    let var_name = bytes_to_str(use_var.variable.name).to_string();
                    let from_outer = outer_scope.get(&var_name);
                    if !from_outer.is_empty() {
                        closure_scope.set(&var_name, from_outer.to_vec());
                    }
                }
            }

            // Seed with parameter types, using callable inference when
            // available.  Filter out any inferred params whose base
            // type is unresolvable (e.g. PHPStan pseudo-types like
            // `collection-of<T>`) so they don't poison the scope —
            // the param simply won't be seeded, which is better than
            // skipping the entire closure body.
            let inferred = inferred_params.unwrap_or(&[]);
            let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
            seed_closure_params(
                &mut closure_scope,
                &closure.parameter_list,
                closure.span().start.offset,
                &filtered_inferred,
                ctx,
            );

            // Record the scope at the body start.
            let body_span = closure.body.span();
            record_scope_snapshot(body_span.start.offset, &closure_scope);

            // Walk the closure body.
            walk_body_for_diagnostics(closure.body.statements.iter(), &mut closure_scope, ctx);

            // Record at body end (closure scope).
            record_scope_snapshot(body_span.end.offset, &closure_scope);

            // Restore the outer scope immediately after the closure body
            // so that code following the closure in the same expression
            // (e.g. `->where('product_id', $product->id)` after a
            // `whereHas(function (Builder $q) { ... })`) sees the outer
            // scope's variables, not the closure's.
            record_scope_snapshot(body_span.end.offset + 1, outer_scope);
        }
        Expression::ArrowFunction(arrow) => {
            // Arrow functions inherit the enclosing scope.
            let mut arrow_scope = outer_scope.clone();

            // Seed with parameter types, using callable inference when
            // available.
            let inferred = inferred_params.unwrap_or(&[]);
            let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
            seed_closure_params(
                &mut arrow_scope,
                &arrow.parameter_list,
                arrow.span().start.offset,
                &filtered_inferred,
                ctx,
            );

            // Record the scope at the body expression.
            let body_span = arrow.expression.span();
            record_scope_snapshot(body_span.start.offset, &arrow_scope);
            record_scope_snapshot(body_span.end.offset, &arrow_scope);

            // Restore the outer scope after the arrow body (same
            // reasoning as for closures above).
            record_scope_snapshot(body_span.end.offset + 1, outer_scope);

            // Recurse into the body expression for nested closures.
            walk_closures_in_expr(arrow.expression, &arrow_scope, ctx, None);
        }
        // For call expressions, try to infer callable parameter types
        // from the function/method signature before recursing into
        // the arguments.
        Expression::Call(call) => {
            walk_closures_in_call(call, outer_scope, ctx);
        }
        // Recurse into sub-expressions that may contain closures.
        Expression::Parenthesized(inner) => {
            walk_closures_in_expr(inner.expression, outer_scope, ctx, None);
        }
        Expression::Assignment(assignment) => {
            walk_closures_in_expr(assignment.rhs, outer_scope, ctx, None);
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => {
                walk_closures_in_expr(pa.object, outer_scope, ctx, None);
            }
            Access::NullSafeProperty(pa) => {
                walk_closures_in_expr(pa.object, outer_scope, ctx, None);
            }
            _ => {}
        },
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                let elem_expr = match elem {
                    ArrayElement::KeyValue(kv) => {
                        walk_closures_in_expr(kv.key, outer_scope, ctx, None);
                        kv.value
                    }
                    ArrayElement::Value(val) => val.value,
                    ArrayElement::Variadic(v) => v.value,
                    ArrayElement::Missing(_) => continue,
                };
                walk_closures_in_expr(elem_expr, outer_scope, ctx, None);
            }
        }
        Expression::LegacyArray(arr) => {
            for elem in arr.elements.iter() {
                let elem_expr = match elem {
                    ArrayElement::KeyValue(kv) => {
                        walk_closures_in_expr(kv.key, outer_scope, ctx, None);
                        kv.value
                    }
                    ArrayElement::Value(val) => val.value,
                    ArrayElement::Variadic(v) => v.value,
                    ArrayElement::Missing(_) => continue,
                };
                walk_closures_in_expr(elem_expr, outer_scope, ctx, None);
            }
        }
        Expression::Binary(bin) => {
            walk_closures_in_expr(bin.lhs, outer_scope, ctx, None);
            walk_closures_in_expr(bin.rhs, outer_scope, ctx, None);
        }
        Expression::UnaryPrefix(prefix) => {
            walk_closures_in_expr(prefix.operand, outer_scope, ctx, None);
        }
        Expression::Conditional(cond) => {
            walk_closures_in_expr(cond.condition, outer_scope, ctx, None);
            if let Some(then_expr) = cond.then {
                walk_closures_in_expr(then_expr, outer_scope, ctx, None);
            }
            walk_closures_in_expr(cond.r#else, outer_scope, ctx, None);
        }
        Expression::Match(m) => {
            walk_closures_in_expr(m.expression, outer_scope, ctx, None);
            for arm in m.arms.iter() {
                walk_closures_in_expr(arm.expression(), outer_scope, ctx, None);
            }
        }
        Expression::Instantiation(inst) => {
            if let Some(ref args) = inst.argument_list {
                walk_closures_in_call_args(&args.arguments, outer_scope, ctx, |_| vec![]);
            }
        }
        Expression::Yield(y) => match y {
            Yield::Value(yv) => {
                if let Some(val) = &yv.value {
                    walk_closures_in_expr(val, outer_scope, ctx, None);
                }
            }
            Yield::Pair(yp) => {
                walk_closures_in_expr(yp.key, outer_scope, ctx, None);
                walk_closures_in_expr(yp.value, outer_scope, ctx, None);
            }
            Yield::From(yf) => {
                walk_closures_in_expr(yf.iterator, outer_scope, ctx, None);
            }
        },
        Expression::Throw(t) => {
            walk_closures_in_expr(t.exception, outer_scope, ctx, None);
        }
        Expression::Clone(c) => {
            walk_closures_in_expr(c.object, outer_scope, ctx, None);
        }
        Expression::Pipe(p) => {
            walk_closures_in_expr(p.input, outer_scope, ctx, None);
            walk_closures_in_expr(p.callable, outer_scope, ctx, None);
        }
        _ => {}
    }
}

/// Handle a call expression: infer callable parameter types from the
/// function/method signature and pass them when walking closure arguments.
fn walk_closures_in_call<'b>(
    call: &'b Call<'b>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match call {
        Call::Function(fc) => {
            // Recurse into the function expression (for closures in
            // chained calls like `$fn()($anotherClosure)`).
            walk_closures_in_expr(fc.function, outer_scope, ctx, None);

            let func_name = match fc.function {
                Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                _ => None,
            };
            walk_closures_in_call_args(&fc.argument_list.arguments, outer_scope, ctx, |arg_idx| {
                if let Some(ref name) = func_name {
                    infer_callable_params_from_function_fw(
                        name,
                        arg_idx,
                        &fc.argument_list.arguments,
                        outer_scope,
                        ctx,
                    )
                } else {
                    vec![]
                }
            });
        }
        Call::Method(mc) => {
            walk_closures_in_expr(mc.object, outer_scope, ctx, None);

            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            let obj_span = mc.object.span();
            let first_arg = extract_first_arg_string_fw(&mc.argument_list.arguments, ctx.content);
            walk_closures_in_call_args(&mc.argument_list.arguments, outer_scope, ctx, |arg_idx| {
                if let Some(ref name) = method_name {
                    infer_callable_params_from_receiver_fw(
                        obj_span.start.offset,
                        obj_span.end.offset,
                        name,
                        arg_idx,
                        first_arg.as_deref(),
                        outer_scope,
                        ctx,
                    )
                } else {
                    vec![]
                }
            });
        }
        Call::NullSafeMethod(mc) => {
            walk_closures_in_expr(mc.object, outer_scope, ctx, None);

            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            let obj_span = mc.object.span();
            let first_arg = extract_first_arg_string_fw(&mc.argument_list.arguments, ctx.content);
            walk_closures_in_call_args(&mc.argument_list.arguments, outer_scope, ctx, |arg_idx| {
                if let Some(ref name) = method_name {
                    infer_callable_params_from_receiver_fw(
                        obj_span.start.offset,
                        obj_span.end.offset,
                        name,
                        arg_idx,
                        first_arg.as_deref(),
                        outer_scope,
                        ctx,
                    )
                } else {
                    vec![]
                }
            });
        }
        Call::StaticMethod(sc) => {
            walk_closures_in_expr(sc.class, outer_scope, ctx, None);

            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &sc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            let first_arg = extract_first_arg_string_fw(&sc.argument_list.arguments, ctx.content);
            walk_closures_in_call_args(&sc.argument_list.arguments, outer_scope, ctx, |arg_idx| {
                if let Some(ref name) = method_name {
                    infer_callable_params_from_static_receiver_fw(
                        sc.class,
                        name,
                        arg_idx,
                        first_arg.as_deref(),
                        outer_scope,
                        ctx,
                    )
                } else {
                    vec![]
                }
            });
        }
    }
}

/// Walk the arguments of a call expression, invoking `infer_fn` for
/// each argument index to get inferred callable parameter types.
/// When an argument is a closure/arrow function, the inferred types
/// are passed through so untyped parameters get the correct types.
fn walk_closures_in_call_args<'b, F>(
    arguments: &'b TokenSeparatedSequence<'b, Argument<'b>>,
    outer_scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    infer_fn: F,
) where
    F: Fn(usize) -> Vec<PhpType>,
{
    for (arg_idx, arg) in arguments.iter().enumerate() {
        let arg_expr = match arg {
            Argument::Positional(a) => a.value,
            Argument::Named(a) => a.value,
        };
        match arg_expr {
            Expression::Closure(_) | Expression::ArrowFunction(_) => {
                let inferred = infer_fn(arg_idx);
                walk_closures_in_expr(
                    arg_expr,
                    outer_scope,
                    ctx,
                    if inferred.is_empty() {
                        None
                    } else {
                        Some(&inferred)
                    },
                );
            }
            _ => {
                walk_closures_in_expr(arg_expr, outer_scope, ctx, None);
            }
        }
    }
}

/// Seed a closure/arrow function scope with parameter types, using
/// inferred callable types as fallback for untyped parameters.
///
/// This mirrors [`seed_params`] but additionally accepts `inferred_types`
/// from the enclosing call's callable signature.  When a parameter has
/// no explicit type hint, the corresponding inferred type (matched by
/// positional index) is used instead.
/// Check whether a `/** … */` docblock is directly attached to the
/// code at `fn_offset` — i.e. only whitespace separates the closing
/// `*/` from `fn_offset`.  This prevents `@param` annotations from
/// sibling closures/arrow functions from leaking across statement
/// boundaries.
fn is_docblock_adjacent(content: &str, fn_offset: usize) -> bool {
    let before = match content.get(..fn_offset) {
        Some(s) => s,
        None => return false,
    };
    // Walk backward over whitespace, then over optional keywords
    // (`static`, visibility modifiers) that may sit between the
    // docblock and `fn`.
    let trimmed = before.trim_end();
    if trimmed.ends_with("*/") {
        return true;
    }
    // Allow `static` keyword between docblock and `fn(…)`:
    //   /** @param T $x */ static fn(T $x) => …
    // Also allow the `function` keyword for regular closures.
    let trimmed = trimmed
        .trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_')
        .trim_end();
    trimmed.ends_with("*/")
}

fn seed_closure_params(
    scope: &mut ScopeState,
    parameter_list: &FunctionLikeParameterList<'_>,
    fn_span_start: u32,
    inferred_types: &[PhpType],
    ctx: &ForwardWalkCtx<'_>,
) {
    for (idx, param) in parameter_list.parameters.iter().enumerate() {
        let pname = bytes_to_str(param.variable.name).to_string();
        let is_variadic = param.ellipsis.is_some();

        let native_type = param.hint.as_ref().map(|h| extract_hint_type(h));

        // Check the `@param` docblock annotation.
        //
        // Only trust the result when the docblock is directly attached
        // to this closure/arrow function (no intervening code).  Without
        // this guard, sibling arrow functions that share a parameter
        // name (e.g. two `array_map(fn($row) => …)` calls) would leak
        // `@param` annotations from one closure to the other, because
        // arrow functions don't introduce `{`/`}` scope boundaries and
        // `find_iterable_raw_type_in_source` scans backward freely.
        let raw_docblock_type = crate::docblock::find_iterable_raw_type_in_source(
            ctx.content,
            fn_span_start as usize,
            &pname,
        )
        .filter(|_| is_docblock_adjacent(ctx.content, fn_span_start as usize))
        .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));

        let effective_type = crate::docblock::resolve_effective_type_typed(
            native_type.as_ref(),
            raw_docblock_type.as_ref(),
        );

        // Substitute method-level template params with their bounds.
        let effective_type = effective_type.map(|ty| {
            let ty = super::resolution::substitute_template_param_bounds(
                ty,
                ctx.content,
                fn_span_start as usize,
            );
            super::resolution::substitute_class_string_template_bounds(
                ty,
                ctx.content,
                fn_span_start as usize,
            )
        });

        let inferred_for_idx = inferred_types.get(idx);

        // When the explicit hint is a bare class name and the inferred
        // type is the same class WITH generic args, prefer the inferred
        // type (preserves template substitution).
        let use_inferred_over_explicit = if let Some(ref eff) = effective_type
            && let Some(inferred) = inferred_for_idx
        {
            super::closure_resolution::inferred_type_is_more_specific_pub(eff, inferred)
        } else {
            false
        };

        let mut param_results = if use_inferred_over_explicit {
            let pi = inferred_for_idx.unwrap();
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                pi,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if !resolved.is_empty() {
                ResolvedType::from_classes_with_hint(resolved, pi.clone())
            } else if pi.is_informative() {
                // The inferred type is more specific (e.g.
                // `array<int, array<string, string>>` vs bare `array`)
                // but doesn't resolve to a class.  Preserve the type
                // string so the parameter is still seeded in scope.
                vec![ResolvedType::from_type_string(pi.clone())]
            } else {
                vec![]
            }
        } else if let Some(ref eff) = effective_type {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                eff,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if !resolved.is_empty() {
                // Check if inferred is a subtype and more specific.
                if let Some(inferred) = inferred_for_idx {
                    let inferred_resolved =
                        crate::completion::type_resolution::type_hint_to_classes_typed(
                            inferred,
                            &ctx.current_class.name,
                            ctx.all_classes,
                            ctx.class_loader,
                        );
                    if !inferred_resolved.is_empty()
                        && inferred_resolved.iter().all(|inferred_cls| {
                            resolved.iter().any(|explicit_cls| {
                                crate::util::is_subtype_of_names(
                                    &inferred_cls.fqn(),
                                    &explicit_cls.fqn(),
                                    ctx.class_loader,
                                )
                            })
                        })
                    {
                        ResolvedType::from_classes_with_hint(inferred_resolved, inferred.clone())
                    } else {
                        ResolvedType::from_classes_with_hint(resolved, eff.clone())
                    }
                } else {
                    ResolvedType::from_classes_with_hint(resolved, eff.clone())
                }
            } else {
                // The explicit hint didn't resolve to a class.
                // Try docblock for a richer type.
                if let Some(ref parsed_dt) = raw_docblock_type {
                    let doc_resolved =
                        crate::completion::type_resolution::type_hint_to_classes_typed(
                            parsed_dt,
                            &ctx.current_class.name,
                            ctx.all_classes,
                            ctx.class_loader,
                        );
                    if !doc_resolved.is_empty() {
                        ResolvedType::from_classes_with_hint(doc_resolved, parsed_dt.clone())
                    } else {
                        let best_type = raw_docblock_type
                            .clone()
                            .or_else(|| effective_type.clone())
                            .unwrap_or_else(PhpType::untyped);
                        vec![ResolvedType::from_type_string(best_type)]
                    }
                } else {
                    let best_type = effective_type.clone().unwrap_or_else(PhpType::untyped);
                    vec![ResolvedType::from_type_string(best_type)]
                }
            }
        } else if let Some(inferred) = inferred_for_idx {
            // No explicit type — use the inferred type from the
            // callable signature.
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                inferred,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if !resolved.is_empty() {
                ResolvedType::from_classes_with_hint(resolved, inferred.clone())
            } else if inferred.is_informative() {
                vec![ResolvedType::from_type_string(inferred.clone())]
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Variadic parameter wrapping.
        if is_variadic && !param_results.is_empty() {
            for rt in &mut param_results {
                rt.type_string = PhpType::list(rt.type_string.clone());
                rt.class_info = None;
            }
        }

        if !param_results.is_empty() {
            scope.seed(&pname, param_results);
        }
    }
}

// ─── Callable parameter inference for the forward walker ────────────────────
//
// These functions mirror the inference logic in `closure_resolution.rs`
// but operate with a `ForwardWalkCtx` + `ScopeState` instead of a
// `VarResolutionCtx`.  They build a temporary `VarResolutionCtx` with
// a scope-based variable resolver injected so that variable lookups
// during receiver resolution read from the forward walker's scope.

/// Infer callable parameter types for a closure passed at position
/// `arg_idx` to a standalone function call.
fn infer_callable_params_from_function_fw(
    func_name: &str,
    arg_idx: usize,
    arguments: &TokenSeparatedSequence<'_, Argument<'_>>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    let scope_locals = &scope.locals;
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_locals
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope("$__infer", ctx.cursor_offset, &scope_resolver);
    let rctx = var_ctx.as_resolution_ctx();
    let func_info = if let Some(fl) = rctx.function_loader {
        fl(func_name)
    } else {
        None
    };
    if let Some(fi) = func_info {
        let mut params = extract_callable_params_at_fw(&fi.parameters, arg_idx);

        if !params.is_empty() && !fi.template_params.is_empty() && !fi.template_bindings.is_empty()
        {
            let arg_texts = extract_argument_texts_fw(arguments, ctx.content);
            let subs = super::rhs_resolution::build_function_template_subs(&fi, &arg_texts, &rctx);
            if !subs.is_empty() {
                params = params.into_iter().map(|p| p.substitute(&subs)).collect();
            }
        }

        params
    } else {
        vec![]
    }
}

/// Infer callable parameter types for a closure passed at position
/// `arg_idx` to an instance method call.
fn infer_callable_params_from_receiver_fw(
    obj_start: u32,
    obj_end: u32,
    method_name: &str,
    arg_idx: usize,
    first_arg_text: Option<&str>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    let start = obj_start as usize;
    let end = obj_end as usize;
    if end > ctx.content.len() {
        return vec![];
    }
    let obj_text = ctx.content[start..end].trim();
    let scope_locals = &scope.locals;
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_locals
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope("$__infer", obj_start, &scope_resolver);
    let rctx = var_ctx.as_resolution_ctx();
    // Keep the raw ResolvedTypes so we can extract generic args from
    // the receiver's type_string (e.g. `Builder<Product>` carries the
    // concrete `Product` arg that must substitute `TModel`).
    let resolved_types =
        crate::completion::resolver::resolve_target_classes(obj_text, AccessKind::Arrow, &rctx);
    let receiver_classes = ResolvedType::into_arced_classes(resolved_types.clone());

    // For relation-query methods (whereHas, etc.), override the closure
    // parameter type with Builder<RelatedModel>.
    if let Some(override_params) = super::closure_resolution::try_relation_query_override_pub(
        &receiver_classes,
        method_name,
        first_arg_text,
        ctx.class_loader,
    ) {
        return override_params;
    }

    let params = find_callable_params_on_classes_fw(&receiver_classes, method_name, arg_idx, ctx);

    // Build a template substitution map from the receiver's generic
    // args.  When the receiver resolves to e.g. `Builder<Product>`,
    // the type_string is `Generic("Builder", [Named("Product")])`.
    // We extract those args, pair them with the class's @template
    // params (e.g. `TModel`), and substitute so that callable params
    // like `Closure(Builder<TModel>)` become `Closure(Builder<Product>)`.
    let template_subs = build_receiver_template_subs(&resolved_types, &receiver_classes, ctx);

    // Apply template substitution, then replace `$this`/`static`
    // tokens with the receiver's full type.
    let params = if !template_subs.is_empty() {
        params
            .into_iter()
            .map(|p| p.substitute(&template_subs))
            .collect()
    } else {
        params
    };

    if let Some(receiver) = receiver_classes.first() {
        let receiver_type =
            super::closure_resolution::build_receiver_self_type_pub(receiver, ctx.class_loader);
        params
            .into_iter()
            .map(|p| p.replace_self_with_type(&receiver_type))
            .collect()
    } else {
        params
    }
}

/// Filter inferred callable param types, replacing any param whose type
/// has an unresolvable base (e.g. PHPStan pseudo-types like
/// `collection-of<T>`) with `PhpType::mixed()`.  `mixed` is not
/// considered informative by `seed_closure_params`, so the param simply
/// won't be seeded — much better than skipping the entire closure body.
fn filter_resolvable_inferred_params(
    inferred: &[PhpType],
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    inferred
        .iter()
        .map(|ty| {
            if has_unresolvable_base(ty, ctx) {
                PhpType::mixed()
            } else {
                ty.clone()
            }
        })
        .collect()
}

/// Check whether a type has a base name that looks class-like but
/// doesn't resolve to any known class in the project or stubs.
fn has_unresolvable_base(ty: &PhpType, ctx: &ForwardWalkCtx<'_>) -> bool {
    match ty {
        PhpType::Named(name) => is_unresolvable_class_name(name, ctx),
        PhpType::Generic(base, args) => {
            is_unresolvable_class_name(base, ctx)
                || args.iter().any(|a| has_unresolvable_base(a, ctx))
        }
        PhpType::Union(parts) | PhpType::Intersection(parts) => {
            parts.iter().any(|p| has_unresolvable_base(p, ctx))
        }
        PhpType::Nullable(inner) => has_unresolvable_base(inner, ctx),
        PhpType::Callable {
            params,
            return_type,
            ..
        } => {
            if let Some(ret) = return_type
                && has_unresolvable_base(ret, ctx)
            {
                return true;
            }
            params
                .iter()
                .any(|p| has_unresolvable_base(&p.type_hint, ctx))
        }
        _ => false,
    }
}

/// A class name is "unresolvable" if it:
/// 1. Contains a hyphen (e.g. `collection-of`, `non-empty-list`) — these
///    are PHPStan pseudo-types that aren't real PHP classes.
/// 2. Is not a scalar/builtin/special type.
/// 3. Doesn't resolve to a class in the project or stubs.
///
/// We only flag hyphenated names because they are guaranteed to not be
/// valid PHP class names.  Non-hyphenated names that fail resolution
/// might just be missing from the index (vendor code, etc.) and
/// shouldn't trigger the guard.
fn is_unresolvable_class_name(name: &str, _ctx: &ForwardWalkCtx<'_>) -> bool {
    // Hyphenated names are never valid PHP class names.  PHPStan uses
    // them for pseudo-types like `collection-of`, `non-empty-list`,
    // `non-empty-array`, `non-empty-string`, `class-string`, etc.
    // `class-string` is handled elsewhere, but the rest are not
    // resolvable as classes.
    if name.contains('-') {
        // Allow well-known pseudo-types that we DO handle elsewhere.
        let lower = name.to_ascii_lowercase();
        if lower == "class-string"
            || lower == "array-key"
            || lower == "non-empty-string"
            || lower == "non-empty-array"
            || lower == "non-empty-list"
            || lower == "non-falsy-string"
            || lower == "numeric-string"
            || lower == "literal-string"
            || lower == "callable-string"
        {
            return false;
        }
        return true;
    }
    false
}

/// Build a template substitution map from the receiver's resolved types.
///
/// When the receiver resolves to a generic type like `Builder<Product>`,
/// this extracts the generic args from the `type_string` and pairs them
/// with the class's `@template` parameters to produce a substitution map
/// (e.g. `{TModel => Product}`).  This enables callable parameter types
/// that reference template params to be fully substituted.
///
/// When the `type_string` is self-like (`static`, `self`, `$this`) —
/// which happens when a method returns `static` on a generic class —
/// the function reconstructs the generic args from the class_info's
/// method return types via `build_receiver_self_type_pub`.  This
/// preserves generic context through method chains like
/// `Model::where(…)->orderBy(…)->each(fn)` where intermediate steps
/// return `static`.
fn build_receiver_template_subs(
    resolved_types: &[ResolvedType],
    receiver_classes: &[Arc<ClassInfo>],
    ctx: &ForwardWalkCtx<'_>,
) -> HashMap<String, PhpType> {
    // Use the first resolved type that has generic args and a matching
    // class with template params.
    for rt in resolved_types {
        let generic_args = match &rt.type_string {
            PhpType::Generic(_, args) if !args.is_empty() => args,
            _ => continue,
        };
        // Find the matching class info (by FQN or short name).
        let base_name = rt.type_string.base_name().unwrap_or_default();
        let class = receiver_classes.iter().find(|c| {
            c.fqn() == base_name
                || c.name == base_name
                || crate::util::short_name(&c.fqn()) == crate::util::short_name(base_name)
        });
        if let Some(cls) = class
            && !cls.template_params.is_empty()
        {
            return crate::inheritance::build_generic_subs(cls, generic_args);
        }
    }

    // Fallback: when the type_string is self-like (e.g. `Named("static")`)
    // but the class has template params, reconstruct the generic args from
    // the class_info's method return types.  This handles method chains
    // where `static` returns lose generic context in the type_string.
    for rt in resolved_types {
        if !rt.type_string.is_self_like() {
            continue;
        }
        let cls = match &rt.class_info {
            Some(c) if !c.template_params.is_empty() => c,
            _ => continue,
        };
        let reconstructed =
            super::closure_resolution::build_receiver_self_type_pub(cls, ctx.class_loader);
        if let PhpType::Generic(_, ref args) = reconstructed
            && !args.is_empty()
        {
            return crate::inheritance::build_generic_subs(cls, args);
        }
    }

    HashMap::new()
}

/// Infer callable parameter types for a closure passed at position
/// `arg_idx` to a static method call.
fn infer_callable_params_from_static_receiver_fw(
    class_expr: &Expression<'_>,
    method_name: &str,
    arg_idx: usize,
    first_arg_text: Option<&str>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    let _ = scope; // scope not needed for static receiver resolution

    let class_name = match class_expr {
        Expression::Self_(_) => Some(ctx.current_class.name.to_string()),
        Expression::Static(_) => Some(ctx.current_class.name.to_string()),
        Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
        Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string()),
        _ => None,
    };
    let owner = class_name.and_then(|name| {
        ctx.all_classes
            .iter()
            .find(|c| c.name == name)
            .map(|c| ClassInfo::clone(c))
            .or_else(|| (ctx.class_loader)(&name).map(Arc::unwrap_or_clone))
    });
    if let Some(ref cls) = owner {
        // For relation-query methods, override with Builder<RelatedModel>.
        if let Some(override_params) = super::closure_resolution::try_relation_query_override_pub(
            &[Arc::new(cls.clone())],
            method_name,
            first_arg_text,
            ctx.class_loader,
        ) {
            return override_params;
        }

        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let params = find_callable_params_on_method_fw(&resolved, method_name, arg_idx);

        // Build a template substitution map from the owner class.
        // When the owner is a generic class (e.g. `Builder<Customer>`
        // via `@extends`), reconstruct its full generic type and pair
        // the args with the class's @template params so that callable
        // params like `Closure(Collection<int, TModel>)` become
        // `Closure(Collection<int, Customer>)`.
        let receiver_type =
            super::closure_resolution::build_receiver_self_type_pub(cls, ctx.class_loader);
        let template_subs = if let PhpType::Generic(_, ref args) = receiver_type
            && !args.is_empty()
            && !cls.template_params.is_empty()
        {
            crate::inheritance::build_generic_subs(cls, args)
        } else {
            HashMap::new()
        };

        let params = if !template_subs.is_empty() {
            params
                .into_iter()
                .map(|p| p.substitute(&template_subs))
                .collect()
        } else {
            params
        };

        params
            .into_iter()
            .map(|p| p.replace_self_with_type(&receiver_type))
            .collect()
    } else {
        vec![]
    }
}

/// Search for method `method_name` on each of `classes` and extract
/// callable parameter types at `arg_idx`.
///
/// Tries the input class first — when the receiver came from a generic
/// instantiation (e.g. `Stream<int, Product>`), its `class_info` already
/// has template substitutions applied by `type_hint_to_classes_typed` →
/// `resolve_class_fully_with_generics`.  Extracting callable params from
/// that class preserves the concrete types (e.g. `callable(Product)`
/// instead of `callable(TVal)`).
///
/// Falls back to `resolve_class_fully_maybe_cached` only when the method
/// isn't found on the input class — this handles methods that come
/// exclusively from virtual member providers or late-merged traits.
fn find_callable_params_on_classes_fw(
    classes: &[Arc<ClassInfo>],
    method_name: &str,
    arg_idx: usize,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    for cls in classes {
        // First: try the class as-is.  When it came from a generic
        // instantiation, template params are already substituted in
        // all method signatures.  Re-resolving via
        // `resolve_class_fully_maybe_cached` would load the base
        // class definition (keyed by FQN with empty generic args),
        // discarding those substitutions.
        let result = find_callable_params_on_method_fw(cls, method_name, arg_idx);
        if !result.is_empty() {
            return result;
        }

        // Fallback: the method wasn't found on the input class.
        // This can happen when the method comes from a virtual member
        // provider, a late-merged trait, or a mixin that wasn't
        // included in the original resolution.  Re-resolve fully
        // and try again.
        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let result = find_callable_params_on_method_fw(&resolved, method_name, arg_idx);
        if !result.is_empty() {
            return result;
        }
    }
    vec![]
}

/// Look up method `method_name` on `class` and extract callable
/// parameter types from the parameter at position `arg_idx`.
fn find_callable_params_on_method_fw(
    class: &ClassInfo,
    method_name: &str,
    arg_idx: usize,
) -> Vec<PhpType> {
    let method = class.get_method(method_name);
    if let Some(m) = method {
        extract_callable_params_at_fw(&m.parameters, arg_idx)
    } else {
        vec![]
    }
}

/// Given a list of parameters, look at `arg_idx` and extract callable
/// parameter types if the type hint is `callable(...)` or `Closure(...)`.
fn extract_callable_params_at_fw(
    params: &[crate::types::ParameterInfo],
    arg_idx: usize,
) -> Vec<PhpType> {
    let param = params.get(arg_idx);
    if let Some(p) = param
        && let Some(ref hint) = p.type_hint
        && let Some(callable_params) = hint.callable_param_types()
    {
        return callable_params
            .iter()
            .map(|cp| cp.type_hint.clone())
            .collect();
    }
    vec![]
}

/// Extract the text of each argument from a call's argument list.
fn extract_argument_texts_fw(
    arguments: &TokenSeparatedSequence<'_, Argument<'_>>,
    content: &str,
) -> Vec<String> {
    arguments
        .iter()
        .map(|arg| {
            let span = match arg {
                Argument::Positional(pos) => pos.value.span(),
                Argument::Named(named) => named.value.span(),
            };
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            if end <= content.len() {
                content[start..end].to_string()
            } else {
                String::new()
            }
        })
        .collect()
}

/// Extract the text of the first positional argument, stripping quotes.
fn extract_first_arg_string_fw(
    arguments: &TokenSeparatedSequence<'_, Argument<'_>>,
    content: &str,
) -> Option<String> {
    let first = arguments.iter().next()?;
    let expr = match first {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    let span = expr.span();
    let start = span.start.offset as usize;
    let end = span.end.offset as usize;
    let raw = content.get(start..end)?.trim();

    if raw.len() >= 2
        && ((raw.starts_with('\'') && raw.ends_with('\''))
            || (raw.starts_with('"') && raw.ends_with('"')))
    {
        Some(raw[1..raw.len() - 1].to_string())
    } else {
        None
    }
}

/// Build diagnostic scope snapshots for every function/method body in
/// the file.
///
/// Parses the file, iterates all top-level and class-level
/// function/method bodies, runs the forward walker on each, and stores
/// scope snapshots in the thread-local [`DIAGNOSTIC_SCOPE`] cache.
///
/// The caller must have activated the cache via
/// [`with_diagnostic_scope_cache`] before calling this function.
pub(crate) fn build_diagnostic_scopes(
    content: &str,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    loaders: Loaders<'_>,
    resolved_class_cache: Option<&crate::virtual_members::ResolvedClassCache>,
) {
    if !is_diagnostic_scope_active() {
        return;
    }

    // Skip if the scope cache is already populated (prevents double
    // walk when both the analyze loop and collect_slow_diagnostics
    // call this function).
    let already_populated =
        DIAGNOSTIC_SCOPE.with(|cell| cell.borrow().as_ref().is_some_and(|m| !m.is_empty()));
    if already_populated {
        return;
    }

    // Mark that we are building the scope cache so that nested
    // resolution calls (e.g. resolve_variable_types) do not read
    // from the partially-populated cache.
    BUILDING_SCOPES.with(|cell: &Cell<bool>| cell.set(true));
    let _building_guard = BuildingScopesGuard;

    let default_class = ClassInfo::default();
    let diag_ctx = DiagnosticWalkCtx {
        content,
        local_classes,
        class_loader,
        loaders,
        resolved_class_cache,
    };

    with_parsed_program(content, "build_diagnostic_scopes", |program, _content| {
        // Walk all top-level statements, analyzing function/method
        // bodies AND top-level code (assignments, expressions, if,
        // foreach, etc.) that lives outside any function body.
        walk_top_level_statements(program.statements.iter(), &default_class, &diag_ctx);
    });
}

/// Recursively walk the AST to find function and method bodies, running
/// the forward walker on each.
/// Seed `$this` in the scope when inside a non-static class method.
///
/// This creates a `ResolvedType` from the enclosing `ClassInfo` and
/// stores it under `"$this"`.  The scope-based variable resolver then
/// returns this entry for any `$this` lookup, eliminating the need to
/// remain unresolved.
fn seed_this(scope: &mut ScopeState, current_class: &ClassInfo) {
    if current_class.name.is_empty() {
        return;
    }
    scope.set(
        "$this",
        vec![ResolvedType::from_class(current_class.clone())],
    );
}

/// Walk a sequence of top-level (or namespace-level) statements,
/// maintaining a shared `ScopeState` for code that lives outside any
/// function or class body.  Function/class/trait/interface/enum bodies
/// are analyzed in isolation (as before), but top-level assignments,
/// expressions, if/foreach/while/for/try/switch, and other statements
/// are walked through the forward walker so that scope snapshots are
/// recorded.  This ensures variable accesses resolve from the scope
/// cache.
fn walk_top_level_statements<'a, 'b: 'a>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    default_class: &ClassInfo,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    let ctx = ForwardWalkCtx {
        current_class: default_class,
        all_classes: diag_ctx.local_classes,
        content: diag_ctx.content,
        cursor_offset: u32::MAX,
        class_loader: diag_ctx.class_loader,
        loaders: diag_ctx.loaders,
        resolved_class_cache: diag_ctx.resolved_class_cache,
        enclosing_return_type: None,
        top_level_scope: None,
    };

    let mut top_level_scope = ScopeState::new();

    // Seed superglobals for top-level code.
    seed_superglobals(&mut top_level_scope);

    for stmt in statements {
        match stmt {
            Statement::Namespace(ns) => {
                // Recurse into namespace body with a fresh scope.
                walk_top_level_statements(ns.statements().iter(), default_class, diag_ctx);
            }
            Statement::Class(class) => {
                let enclosing = find_enclosing_class_for_offset(
                    diag_ctx.local_classes,
                    class.left_brace.start.offset,
                )
                .unwrap_or(default_class);
                for member in class.members.iter() {
                    walk_class_member_body(member, enclosing, diag_ctx);
                }
            }
            Statement::Interface(iface) => {
                let enclosing = find_enclosing_class_for_offset(
                    diag_ctx.local_classes,
                    iface.left_brace.start.offset,
                )
                .unwrap_or(default_class);
                for member in iface.members.iter() {
                    walk_class_member_body(member, enclosing, diag_ctx);
                }
            }
            Statement::Trait(trait_def) => {
                let enclosing = find_enclosing_class_for_offset(
                    diag_ctx.local_classes,
                    trait_def.left_brace.start.offset,
                )
                .unwrap_or(default_class);
                for member in trait_def.members.iter() {
                    walk_class_member_body(member, enclosing, diag_ctx);
                }
            }
            Statement::Enum(enum_def) => {
                let enclosing = find_enclosing_class_for_offset(
                    diag_ctx.local_classes,
                    enum_def.left_brace.start.offset,
                )
                .unwrap_or(default_class);
                for member in enum_def.members.iter() {
                    walk_class_member_body(member, enclosing, diag_ctx);
                }
            }
            Statement::Function(func) => {
                analyze_function_body(
                    func.parameter_list.parameters.iter(),
                    func.body.statements.iter(),
                    func.span().start.offset,
                    default_class,
                    None,
                    true, // standalone functions have no `$this`
                    diag_ctx,
                );
            }
            // Functions nested inside if blocks (common pattern:
            // `if (!function_exists('name')) { function name() {} }`)
            // must be analyzed the same way as top-level functions.
            Statement::If(if_stmt) => {
                record_scope_snapshot(stmt.span().start.offset, &top_level_scope);
                process_statement(stmt, &mut top_level_scope, &ctx);
                walk_closures_in_statement(stmt, &top_level_scope, &ctx);
                record_scope_snapshot(stmt.span().end.offset, &top_level_scope);
                walk_functions_in_if_body(&if_stmt.body, default_class, diag_ctx);
            }
            // Top-level code: walk it with the shared scope so that
            // variable assignments accumulate and subsequent accesses
            // can be served from the scope cache instead of remaining
            // unresolved.
            _ => {
                record_scope_snapshot(stmt.span().start.offset, &top_level_scope);
                process_statement(stmt, &mut top_level_scope, &ctx);
                walk_closures_in_statement(stmt, &top_level_scope, &ctx);
                record_scope_snapshot(stmt.span().end.offset, &top_level_scope);
            }
        }
    }
}

/// Recurse into an if-statement body looking for function declarations
/// and analyze each one.  Handles the common PHP pattern:
/// `if (!function_exists('name')) { function name(...) { ... } }`
fn walk_functions_in_if_body<'b>(
    body: &'b mago_syntax::ast::control_flow::r#if::IfBody<'b>,
    default_class: &ClassInfo,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    use mago_syntax::ast::control_flow::r#if::IfBody;

    let statements: &[Statement<'b>] = match body {
        IfBody::Statement(stmt_body) => {
            // Single statement body — check if it's a block.
            if let Statement::Block(block) = stmt_body.statement {
                block.statements.as_slice()
            } else if let Statement::Function(func) = stmt_body.statement {
                analyze_function_body(
                    func.parameter_list.parameters.iter(),
                    func.body.statements.iter(),
                    func.span().start.offset,
                    default_class,
                    None,
                    true,
                    diag_ctx,
                );
                return;
            } else {
                return;
            }
        }
        IfBody::ColonDelimited(colon_body) => colon_body.statements.as_slice(),
    };

    for inner_stmt in statements.iter() {
        if let Statement::Function(func) = inner_stmt {
            analyze_function_body(
                func.parameter_list.parameters.iter(),
                func.body.statements.iter(),
                func.span().start.offset,
                default_class,
                None,
                true,
                diag_ctx,
            );
        }
    }
}

/// Walk a class member to find method bodies and run the forward walker.
fn walk_class_member_body<'b>(
    member: &'b mago_syntax::ast::class_like::member::ClassLikeMember<'b>,
    enclosing_class: &ClassInfo,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    use mago_syntax::ast::class_like::member::ClassLikeMember;
    use mago_syntax::ast::class_like::method::MethodBody;

    if let ClassLikeMember::Method(method) = member
        && let MethodBody::Concrete(block) = &method.body
    {
        let method_name = bytes_to_str(method.name.value).to_string();
        let is_static = method.modifiers.contains_static();
        analyze_function_body(
            method.parameter_list.parameters.iter(),
            block.statements.iter(),
            method.span().start.offset,
            enclosing_class,
            Some(&method_name),
            is_static,
            diag_ctx,
        );
    }
}

/// Bundles the immutable context needed by [`analyze_function_body`] and
/// the AST walkers so we don't pass 5+ individual arguments everywhere.
struct DiagnosticWalkCtx<'a> {
    content: &'a str,
    local_classes: &'a [Arc<ClassInfo>],
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    loaders: Loaders<'a>,
    resolved_class_cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
}

/// Run the forward walker on a single function/method body and record
/// scope snapshots for diagnostics.
///
/// `is_static` indicates whether this is a static method.  When `false`
/// and `current_class` has a non-empty name, `$this` is seeded in the
/// scope so that expressions like `$this->prop` and `foreach ($this->items as $item)`
/// can resolve without remaining unresolved.
fn analyze_function_body<'b>(
    parameters: impl Iterator<Item = &'b FunctionLikeParameter<'b>>,
    body_statements: impl Iterator<Item = &'b Statement<'b>>,
    fn_span_start: u32,
    current_class: &ClassInfo,
    method_name: Option<&str>,
    is_static: bool,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    let ctx = ForwardWalkCtx {
        current_class,
        all_classes: diag_ctx.local_classes,
        content: diag_ctx.content,
        cursor_offset: u32::MAX,
        class_loader: diag_ctx.class_loader,
        loaders: diag_ctx.loaders,
        resolved_class_cache: diag_ctx.resolved_class_cache,
        enclosing_return_type: None,
        top_level_scope: None,
    };

    let mut scope = ScopeState::new();

    // Seed `$this` for non-static class methods so that expressions
    // referencing `$this` (e.g. `$this->prop`, `foreach ($this->items …)`)
    // resolve from the scope instead of falling through to the backward
    // scanner.
    if !is_static {
        seed_this(&mut scope, current_class);
    }

    // Seed scope with parameter types.
    // Detect whether this method has a #[Scope] attribute by scanning
    // the source text around the method span for `#[Scope]`.
    let has_scope_attr = method_name
        .map(|_| detect_scope_attribute_from_source(diag_ctx.content, fn_span_start as usize))
        .unwrap_or(false);
    seed_params(
        &mut scope,
        parameters,
        fn_span_start,
        method_name,
        has_scope_attr,
        &ctx,
    );

    // Seed superglobals so that accesses like `$_SERVER['key']` don't
    // remain unresolved.
    seed_superglobals(&mut scope);

    // Record the scope right at the function body start so that
    // member accesses on parameters before any assignment are covered.
    record_scope_snapshot(fn_span_start, &scope);

    // Walk the entire body, recording snapshots at each statement.
    walk_body_for_diagnostics(body_statements, &mut scope, &ctx);
}

/// Find the innermost class whose body span contains `offset`.
///
/// This is the diagnostic-module equivalent of
/// [`find_innermost_enclosing_class`](crate::diagnostics::helpers::find_innermost_enclosing_class).
fn find_enclosing_class_for_offset(
    local_classes: &[Arc<ClassInfo>],
    offset: u32,
) -> Option<&ClassInfo> {
    local_classes
        .iter()
        .filter(|c| offset >= c.start_offset && offset <= c.end_offset)
        .min_by_key(|c| c.end_offset.saturating_sub(c.start_offset))
        .map(|c| c.as_ref())
}

// ─── Core data structures ───────────────────────────────────────────────────

/// The type-state of all variables at a single program point.
///
/// This is the equivalent of PHPStan's `expressionTypes` map and Mago's
/// `BlockContext.locals`.  It is created once at the start of a function
/// body analysis, seeded with parameter types, and passed as `&mut` through
/// the forward walk.
#[derive(Clone, Debug)]
pub(crate) struct ScopeState {
    /// Variable name (with `$` prefix, e.g. `"$foo"`) → resolved types.
    ///
    /// This is the single source of truth for all variable types at the
    /// current program point.  Every variable that has been assigned,
    /// declared as a parameter, or bound by a foreach/catch before the
    /// current statement has an entry here.
    pub locals: AtomMap<Vec<ResolvedType>>,
}

impl ScopeState {
    /// Create an empty scope.
    pub fn new() -> Self {
        Self {
            locals: AtomMap::default(),
        }
    }

    /// Look up a variable's types.  Returns an empty slice when the
    /// variable has not been assigned.
    pub fn get(&self, var_name: &str) -> &[ResolvedType] {
        self.locals
            .get(&atom(var_name))
            .map_or(&[], |v| v.as_slice())
    }

    /// Check whether a variable exists in scope (even if its type list is empty).
    pub fn contains(&self, var_name: &str) -> bool {
        self.locals.contains_key(&atom(var_name))
    }

    /// Insert or overwrite a variable's types.
    pub fn set(&mut self, var_name: &str, types: Vec<ResolvedType>) {
        if types.is_empty() {
            return;
        }
        self.locals.insert(atom(var_name), types);
    }

    /// Record that a variable exists in scope with an empty type list.
    /// This prevents the variable from appearing unseen by the forward
    /// walker.
    pub fn set_empty(&mut self, var_name: &str) {
        self.locals.entry(atom(var_name)).or_default();
    }

    /// Insert a variable's types from parameter seeding.
    pub fn seed(&mut self, var_name: &str, types: Vec<ResolvedType>) {
        if types.is_empty() {
            return;
        }
        self.locals.insert(atom(var_name), types);
    }

    /// Remove a variable (e.g. after `unset($x)`).
    pub fn remove(&mut self, var_name: &str) {
        self.locals.remove(&atom(var_name));
    }

    /// Merge another scope into `self`.
    ///
    /// For each variable:
    /// - Present in both: union the type sets (variable was assigned
    ///   in both branches).
    /// - Present in only one: keep it with the existing types (variable
    ///   was assigned in only one branch — it *might* have those types).
    ///
    /// After merging, subsumed entries are removed.  When one entry's
    /// type is a subset of another (e.g. `string|null` ⊆
    /// `int|string|null`, or `Foo` ⊆ `mixed`), the subset entry is
    /// dropped because the superset already covers it.  Without this,
    /// narrowed types from non-exiting if-branches leak into the
    /// post-merge scope and pollute subsequent narrowing operations.
    pub fn merge_branch(&mut self, other: &ScopeState) {
        for (name, other_types) in &other.locals {
            let entry = self.locals.entry(*name).or_default();

            // Merge other_types into entry.  When an incoming entry
            // shares a class name with an existing entry but has a
            // broader type_string (e.g. `?A` vs `A`), widen the
            // existing entry's type_string instead of discarding
            // the incoming one.  This prevents post-loop merges from
            // losing nullable information.
            for rt in other_types.iter() {
                let mut merged_into_existing = false;
                if let Some(ref rt_cls) = rt.class_info {
                    for existing in entry.iter_mut() {
                        if let Some(ref ex_cls) = existing.class_info
                            && ex_cls.name == rt_cls.name
                        {
                            // Same class.  If the incoming type is
                            // broader, adopt it.
                            if existing.type_string != rt.type_string
                                && existing.type_string.is_subset_of(&rt.type_string)
                            {
                                existing.type_string = rt.type_string.clone();
                            }
                            merged_into_existing = true;
                            break;
                        }
                    }
                }
                if !merged_into_existing {
                    ResolvedType::push_unique(entry, rt.clone());
                }
            }

            // Remove entries whose type is subsumed by a broader entry.
            // E.g. `string|null` ⊆ `int|string|null` → drop the former.
            if entry.len() > 1 {
                let types: Vec<crate::php_type::PhpType> =
                    entry.iter().map(|rt| rt.type_string.clone()).collect();
                let mut keep = vec![true; types.len()];
                for i in 0..types.len() {
                    if !keep[i] {
                        continue;
                    }
                    for j in 0..types.len() {
                        if i == j || !keep[j] {
                            continue;
                        }
                        // If j is a strict subset of i, drop j.
                        if types[j] != types[i] && types[j].is_subset_of(&types[i]) {
                            keep[j] = false;
                        }
                    }
                }
                let mut idx = 0;
                entry.retain(|_| {
                    let k = keep[idx];
                    idx += 1;
                    k
                });
            }
        }
    }
}

/// Simplify unions in a scope by collapsing child/parent class pairs.
///
/// When merging branches produces a union like `Child | Parent` where
/// `Child extends Parent`, the union is redundant — every value of
/// type `Child` is also a `Parent`.  This collapses such unions to
/// the broadest (parent) type.
///
/// Only operates on variables that have exactly two `ResolvedType`
/// entries with named class types.  More complex unions (3+ members,
/// scalars, generics) are left unchanged.
fn simplify_class_hierarchy_unions(
    scope: &mut ScopeState,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) {
    let keys: Vec<Atom> = scope.locals.keys().copied().collect();
    for key in keys {
        let types = match scope.locals.get(&key) {
            Some(t) if t.len() == 2 => t,
            _ => continue,
        };

        // Extract class names from the two ResolvedType entries.
        let name_a = match types[0].type_string.class_name() {
            Some(n) => n,
            None => continue,
        };
        let name_b = match types[1].type_string.class_name() {
            Some(n) => n,
            None => continue,
        };

        // Check if one is a subclass of the other.
        if is_subclass_of(name_a, name_b, class_loader) {
            // A extends B → keep B (the parent).
            scope.locals.get_mut(&key).unwrap().remove(0);
        } else if is_subclass_of(name_b, name_a, class_loader) {
            // B extends A → keep A (the parent).
            scope.locals.get_mut(&key).unwrap().remove(1);
        }
    }
}

/// Check whether `child` is a subclass (direct or transitive) of
/// `parent` by walking the inheritance chain via the class loader.
///
/// Returns `false` if either class cannot be loaded or if there is
/// no inheritance relationship.  Limits the chain walk to 20 steps
/// to avoid infinite loops on cyclic hierarchies.
fn is_subclass_of(
    child: &str,
    parent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    if child.eq_ignore_ascii_case(parent) {
        return false; // same class, not a subclass
    }
    let mut current = child.to_string();
    for _ in 0..20 {
        let cls = match class_loader(&current) {
            Some(c) => c,
            None => return false,
        };
        // Check implemented interfaces at every level.
        for iface in &cls.interfaces {
            if iface.as_str().eq_ignore_ascii_case(parent) {
                return true;
            }
        }
        if let Some(ref p) = cls.parent_class {
            if p.as_str().eq_ignore_ascii_case(parent) {
                return true;
            }
            current = p.to_string();
        } else {
            return false;
        }
    }
    false
}

/// Context for the forward walk.
///
/// Bundles the immutable context that every statement/expression handler
/// needs — the class loader, function loader, current class info, source
/// text, etc.  The mutable `ScopeState` is passed separately as `&mut`.
pub(crate) struct ForwardWalkCtx<'a> {
    /// The class containing the method being analyzed (or a dummy for
    /// top-level functions).
    pub current_class: &'a ClassInfo,
    /// All classes known in the current file.
    pub all_classes: &'a [Arc<ClassInfo>],
    /// Full source text of the current file.
    pub content: &'a str,
    /// Byte offset of the cursor.  The walk stops when a statement's
    /// start offset reaches or exceeds this value.
    pub cursor_offset: u32,
    /// Cross-file class resolution callback.
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// Cross-file loader callbacks (function loader, constant loader).
    pub loaders: Loaders<'a>,
    /// Shared cache of fully-resolved classes.
    pub resolved_class_cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
    /// The `@return` type of the enclosing function/method, if known.
    /// Used for generator yield inference.
    pub enclosing_return_type: Option<PhpType>,
    /// Pre-computed top-level scope for resolving `global` variable imports.
    /// When a function body contains `global $x;`, the walker looks up
    /// `$x` in this map to seed the local scope with the top-level type.
    pub top_level_scope: Option<AtomMap<Vec<ResolvedType>>>,
}

impl<'a> ForwardWalkCtx<'a> {
    /// Return a copy of this context with a different `cursor_offset`.
    ///
    /// Used by the two-pass loop strategy: pass 1 runs with
    /// `cursor_offset = u32::MAX` so the entire loop body is walked
    /// and all assignments are discovered, even those after the real
    /// cursor position.
    fn with_cursor_offset(&self, cursor_offset: u32) -> ForwardWalkCtx<'a> {
        ForwardWalkCtx {
            current_class: self.current_class,
            all_classes: self.all_classes,
            content: self.content,
            cursor_offset,
            class_loader: self.class_loader,
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: self.enclosing_return_type.clone(),
            top_level_scope: self.top_level_scope.clone(),
        }
    }

    /// Build a [`VarResolutionCtx`] with a scope-based variable
    /// resolver.  Used by [`resolve_rhs_with_scope`] so that
    /// `resolve_rhs_expression` and its sub-functions read variable
    /// types from the forward walker's in-progress `ScopeState`
    /// instead of re-entering `resolve_variable_types`.
    fn var_ctx_for_with_scope<'b>(
        &'b self,
        var_name: &'b str,
        cursor_offset: u32,
        scope_resolver: &'b dyn Fn(&str) -> Vec<ResolvedType>,
    ) -> VarResolutionCtx<'b>
    where
        'a: 'b,
    {
        VarResolutionCtx {
            var_name,
            current_class: self.current_class,
            all_classes: self.all_classes,
            content: self.content,
            cursor_offset,
            class_loader: self.class_loader,
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: self.enclosing_return_type.clone(),
            top_level_scope: self.top_level_scope.clone(),
            branch_aware: false,
            match_arm_narrowing: HashMap::new(),
            scope_var_resolver: Some(scope_resolver),
        }
    }
}

// ─── Forward walk entry point ───────────────────────────────────────────────

thread_local! {
    /// Tracks the current loop nesting depth (foreach, while, for,
    /// do-while).  Used to reduce the number of loop iterations for
    /// deeply nested loops, preventing the exponential blowup that
    /// occurs when loop iteration interacts with if-branch merging.
    static LOOP_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Maximum loop nesting depth before loop bodies are skipped entirely.
/// PHP code rarely nests loops beyond 6 levels; this is a hard safety net.
const MAX_LOOP_DEPTH: u32 = 6;

/// Increment the loop depth counter and return the new depth.
fn enter_loop() -> u32 {
    LOOP_DEPTH.with(|c| {
        let v = c.get() + 1;
        c.set(v);
        v
    })
}

/// Decrement the loop depth counter.
fn leave_loop(depth: u32) {
    LOOP_DEPTH.with(|c| c.set(depth - 1));
}

/// Clamp `max_iterations` based on the current loop nesting depth.
///
/// At depth 1 (outermost loop), the full assignment-depth-bounded
/// iteration count is used.  At depth 2, cap at 2 iterations.
/// At depth 3+, use a single pass only.  This prevents exponential
/// blowup from the interaction of loop iteration with if-branch
/// merging in deeply nested loops.
fn clamp_iterations_for_depth(max_iterations: u32, loop_depth: u32) -> u32 {
    match loop_depth {
        0 | 1 => max_iterations,
        2 => max_iterations.min(2),
        _ => 1,
    }
}

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
            walk_closures_in_statement(stmt, scope, ctx);
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

    // Walk the body forward.
    walk_body_forward(stmts_vec.iter().copied(), &mut scope, ctx);

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

    // Walk the body forward.
    walk_body_forward(func.body.statements.iter(), &mut scope, ctx);

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

    // Walk the top-level statements forward.
    walk_body_forward(statements, &mut scope, ctx);

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

// ─── Parameter seeding ──────────────────────────────────────────────────────

/// Seed the scope with types from function/method parameters.
///
/// For each parameter, resolves its type from:
/// 1. The native type hint
/// 2. The `@param` docblock annotation (which may be more specific)
/// 3. The merged class info (from parent/interface inheritance)
/// 4. Eloquent scope Builder enrichment
fn seed_params<'b>(
    scope: &mut ScopeState,
    parameters: impl Iterator<Item = &'b FunctionLikeParameter<'b>>,
    method_span_start: u32,
    method_name: Option<&str>,
    has_scope_attr: bool,
    ctx: &ForwardWalkCtx<'_>,
) {
    for param in parameters {
        let pname = bytes_to_str(param.variable.name).to_string();
        let is_variadic = param.ellipsis.is_some();
        let native_type = param.hint.as_ref().map(|h| extract_hint_type(h));

        // For promoted constructor properties, check for an inline
        // `/** @var Type */` docblock on the parameter itself.  The
        // property parser already uses this for the property's type_hint,
        // but the forward walker resolves parameter variables via
        // `resolve_param_type` which only checks `@param` tags on the
        // method docblock.  When an inline `@var` is present, resolve it
        // directly and seed the scope, bypassing `resolve_param_type`
        // (which would otherwise fall back to the merged class's native
        // parameter type, losing the docblock refinement).
        if param.is_promoted_property() {
            let param_offset = param.span().start.offset as usize;
            if let Some((var_type, _name)) =
                crate::docblock::find_inline_var_docblock(ctx.content, param_offset)
            {
                let var_type = crate::util::resolve_php_type_names(&var_type, ctx.class_loader);
                let effective = crate::docblock::resolve_effective_type_typed(
                    native_type.as_ref(),
                    Some(&var_type),
                )
                .unwrap_or(var_type);

                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    &effective,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );

                let results = if !resolved.is_empty() {
                    ResolvedType::from_classes_with_hint(resolved, effective)
                } else {
                    vec![ResolvedType::from_type_string(effective)]
                };

                scope.seed(&pname, results);
                continue;
            }
        }

        let param_results = resolve_param_type(
            &pname,
            native_type.as_ref(),
            is_variadic,
            method_span_start,
            method_name,
            has_scope_attr,
            ctx,
        );

        if !param_results.is_empty() {
            scope.seed(&pname, param_results);
        } else {
            // Seed untyped parameters with empty types so they exist
            // in scope.  This allows instanceof narrowing to find them
            // (apply_condition_narrowing iterates scope.locals.keys()).
            scope.set_empty(&pname);
        }
    }
}

/// Resolve a single parameter's type through the full resolution
/// pipeline: native hint → Eloquent Builder enrichment → docblock
/// `@param` → template substitution → merged class fallback →
/// type-string-only fallback.
///
/// Used by [`seed_params`] (forward walker) and
/// [`super::resolution::resolve_abstract_method_param`] (abstract
/// methods with no body).
pub(super) fn resolve_param_type(
    pname: &str,
    native_type: Option<&PhpType>,
    is_variadic: bool,
    method_span_start: u32,
    method_name: Option<&str>,
    has_scope_attr: bool,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    // Eloquent scope Builder enrichment: when the enclosing class
    // extends Eloquent Model and this is a scope method (convention
    // or #[Scope] attribute), enrich bare `Builder` to
    // `Builder<EnclosingModel>`.
    let enriched_type = native_type.and_then(|nt| {
        if let Some(mname) = method_name {
            super::resolution::enrich_builder_type_in_scope(
                nt,
                mname,
                has_scope_attr,
                ctx.current_class,
                ctx.class_loader,
            )
        } else {
            None
        }
    });

    let type_for_resolution: Option<&PhpType> = enriched_type.as_ref().or(native_type);

    // Check the `@param` docblock annotation.
    let raw_docblock_type = crate::docblock::find_iterable_raw_type_in_source(
        ctx.content,
        method_span_start as usize,
        pname,
    )
    .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));

    // Pick the effective type: docblock overrides native when it is
    // a compatible refinement.  Use the enriched type (e.g.
    // `Builder<User>`) rather than the bare native type so that
    // the generic args survive into the resolved ClassInfo.
    let native_for_effective = enriched_type.as_ref().or(native_type).cloned();
    let doc_parsed = raw_docblock_type.clone();
    let effective_type = crate::docblock::resolve_effective_type_typed(
        native_for_effective.as_ref(),
        doc_parsed.as_ref(),
    );

    // Substitute method-level template params with their bounds.
    let effective_type = effective_type.map(|ty| {
        let ty = super::resolution::substitute_template_param_bounds(
            ty,
            ctx.content,
            method_span_start as usize,
        );
        // Also substitute inside class-string<T> so that
        // `class-string<T>` with `@template T of Foo` becomes
        // `class-string<Foo>`.
        super::resolution::substitute_class_string_template_bounds(
            ty,
            ctx.content,
            method_span_start as usize,
        )
    });

    let mut resolved_from_effective = effective_type
        .as_ref()
        .map(|ty| {
            crate::completion::type_resolution::type_hint_to_classes_typed(
                ty,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )
        })
        .unwrap_or_default();

    // When the effective type is `class-string<Foo>`, the base
    // type `class-string` doesn't resolve to a class.  Unwrap the
    // inner type and resolve it so that `$class::KEY` finds
    // static members on `Foo`.
    if resolved_from_effective.is_empty()
        && let Some(ref eff) = effective_type
        && let Some(inner) = eff.unwrap_class_string_inner()
    {
        let inner_resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
            inner,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        );
        if !inner_resolved.is_empty() {
            resolved_from_effective = inner_resolved;
        }
    }

    let mut param_results = if !resolved_from_effective.is_empty() {
        ResolvedType::from_classes_with_hint(
            resolved_from_effective,
            effective_type.unwrap_or_else(|| {
                type_for_resolution
                    .cloned()
                    .unwrap_or_else(PhpType::untyped)
            }),
        )
    } else if let Some(ref eff) = effective_type
        && raw_docblock_type.as_ref().is_some_and(|rdt| *rdt != *eff)
    {
        // The effective type differs from the raw docblock type, meaning
        // template substitution produced a concrete type (e.g. `K` →
        // `array-key`).  Use the substituted type so that downstream
        // narrowing (type guards, instanceof) operates on the concrete
        // type rather than the bare template parameter name.
        vec![ResolvedType::from_type_string(eff.clone())]
    } else if let Some(ref rdt) = raw_docblock_type {
        let parsed_docblock = rdt.clone();
        let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
            &parsed_docblock,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        );
        if !resolved.is_empty() {
            ResolvedType::from_classes_with_hint(resolved, parsed_docblock)
        } else {
            // Try the merged class for a richer type.
            try_resolve_from_merged_class(pname, method_name, ctx).unwrap_or_else(|| {
                build_type_string_only_result(
                    raw_docblock_type.as_ref(),
                    type_for_resolution,
                    ctx.content,
                    method_span_start as usize,
                )
            })
        }
    } else {
        // Try the merged class.
        try_resolve_from_merged_class(pname, method_name, ctx).unwrap_or_else(|| {
            build_type_string_only_result(
                raw_docblock_type.as_ref(),
                type_for_resolution,
                ctx.content,
                method_span_start as usize,
            )
        })
    };

    // Variadic parameter wrapping.
    if is_variadic && !param_results.is_empty() {
        for rt in &mut param_results {
            rt.type_string = PhpType::list(rt.type_string.clone());
            rt.class_info = None;
        }
    }

    param_results
}

/// Try to resolve a parameter type from the fully-merged class info
/// (with interface members merged and `@implements` generics applied).
///
/// When a class declares `@implements CastsAttributes<Decimal, Decimal>`
/// and the interface method `set()` has a generic parameter `TSet $value`,
/// the merged class will have `set($value: Decimal)`.  This function
/// looks up the merged method and returns the substituted parameter type.
fn try_resolve_from_merged_class(
    pname: &str,
    method_name: Option<&str>,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let method_name = method_name?;

    // Only attempt this for real classes (not the default/dummy class
    // used for top-level functions).
    if ctx.current_class.name.is_empty() {
        return None;
    }

    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        ctx.current_class,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );

    let merged_method = merged.get_method(method_name)?;

    // Find the matching parameter by name.
    // ParameterInfo.name includes the `$` prefix.
    let merged_param = merged_method.parameters.iter().find(|p| p.name == pname)?;
    let hint = merged_param.type_hint.as_ref()?;

    let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
        hint,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );

    if !resolved.is_empty() {
        Some(ResolvedType::from_classes_with_hint(resolved, hint.clone()))
    } else {
        // The merged type doesn't resolve to a class (e.g. `list<Pen>`,
        // `array<string, int>`).  Return a type-string-only result so
        // the merged hint (which may be richer than the native type
        // from the child's signature, e.g. `list<Pen>` vs bare `array`)
        // is preserved in the scope.  This allows array-access
        // resolution to extract the element type from `list<Pen>`.
        Some(vec![ResolvedType::from_type_string(hint.clone())])
    }
}

/// Build a type-string-only `ResolvedType` result for a parameter whose
/// type does not resolve to any class.
fn build_type_string_only_result(
    raw_docblock_type: Option<&PhpType>,
    type_for_resolution: Option<&PhpType>,
    content: &str,
    method_span_start: usize,
) -> Vec<ResolvedType> {
    let best_type = if let Some(rdt) = raw_docblock_type {
        Some(rdt.clone())
    } else {
        type_for_resolution.cloned()
    };
    if let Some(mut parsed) = best_type {
        parsed = super::resolution::substitute_class_string_template_bounds(
            parsed,
            content,
            method_span_start,
        );
        vec![ResolvedType::from_type_string(parsed)]
    } else {
        vec![]
    }
}

// ─── Statement processing ───────────────────────────────────────────────────

/// Process a single statement, updating `scope` with any variable
/// assignments, narrowing, or control-flow effects.
fn process_statement<'b>(
    stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match stmt {
        Statement::Expression(expr_stmt) => {
            process_expression_statement(expr_stmt, scope, ctx);
        }
        Statement::Foreach(foreach) => {
            process_foreach(foreach, scope, ctx);
        }
        Statement::If(if_stmt) => {
            process_if(if_stmt, stmt, scope, ctx);
        }
        Statement::While(while_stmt) => {
            process_while(while_stmt, scope, ctx);
        }
        Statement::For(for_stmt) => {
            process_for(for_stmt, scope, ctx);
        }
        Statement::DoWhile(dw) => {
            process_do_while(dw, scope, ctx);
        }
        Statement::Try(try_stmt) => {
            process_try(try_stmt, scope, ctx);
        }
        Statement::Switch(switch) => {
            process_switch(switch, scope, ctx);
        }
        Statement::Block(block) => {
            walk_body_forward(block.statements.iter(), scope, ctx);
        }
        Statement::Unset(unset_stmt) => {
            for val in unset_stmt.values.iter() {
                if let Expression::Variable(Variable::Direct(dv)) = val {
                    scope.remove(bytes_to_str(dv.name));
                }
            }
        }
        Statement::Namespace(ns) => {
            walk_body_forward(ns.statements().iter(), scope, ctx);
        }
        Statement::Global(global) => {
            for var in global.variables.iter() {
                if let Variable::Direct(dv) = var {
                    let var_name = bytes_to_str(dv.name).to_string();
                    if let Some(top_scope) = &ctx.top_level_scope {
                        if let Some(types) = top_scope.get(&atom(&var_name)) {
                            scope.set(&var_name, types.clone());
                        } else {
                            scope.set_empty(&var_name);
                        }
                    } else {
                        scope.set_empty(&var_name);
                    }
                }
            }
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                // Record `&&` chain snapshots so that member accesses
                // after an instanceof/null guard see the narrowed type.
                // E.g. `return $x instanceof Foo && $x->bar()`
                record_and_chain_snapshots(val, scope, ctx);

                // Record narrowed snapshots inside match(true) arms
                // and ternary instanceof branches.
                if is_diagnostic_scope_active() {
                    record_match_ternary_snapshots(val, scope, ctx);
                }
            }
        }
        _ => {}
    }
}

// ─── Expression statement handling ──────────────────────────────────────────

/// Process an expression statement: handle assignments, assert narrowing,
/// pass-by-reference type inference, etc.
fn process_expression_statement<'b>(
    expr_stmt: &'b ExpressionStatement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let expr = expr_stmt.expression;

    // Try inline `/** @var Type $x */` override first.
    match try_process_inline_var_override(expr, stmt_offset(expr), scope, ctx) {
        VarOverrideResult::NamedVar => {
            // Re-record the scope snapshot at this expression's offset
            // so that variable lookups within the same statement (e.g.
            // `$app` in `$client = $app->make(...)` where a preceding
            // `@var` block declared `$app`) see the updated types.
            // The snapshot recorded by `walk_body_for_diagnostics` at
            // the statement start was taken *before* the `@var`
            // override was applied.
            record_scope_snapshot(stmt_offset(expr), scope);
            return;
        }
        VarOverrideResult::NoVar => {
            // A `@var Type` (no variable name) was applied to the
            // assignment LHS.  The override already set the LHS type,
            // so skip further assignment processing to avoid the RHS
            // overwriting the docblock type.
            return;
        }
        VarOverrideResult::None => {}
    }

    // Record intermediate scope snapshots within `&&` chains so that
    // member accesses after an instanceof/null guard see the narrowed
    // type.  E.g. `$x instanceof Foo && $x->bar()` as an expression
    // statement.
    record_and_chain_snapshots(expr, scope, ctx);

    // Record narrowed snapshots inside match(true) arms and ternary
    // instanceof branches within this expression.
    if is_diagnostic_scope_active() {
        record_match_ternary_snapshots(expr, scope, ctx);
    }

    // Process assignments.
    process_assignment_expr(expr, scope, ctx);

    // Process pass-by-reference parameter type inference.
    process_pass_by_ref(expr, scope, ctx);

    // Process assert narrowing.
    process_assert_narrowing(expr, scope, ctx);

    // Process increment/decrement: $a++, ++$a, $a--, --$a.
    process_increment_decrement(expr, scope, ctx);
}

/// Process increment/decrement expressions (`$a++`, `++$a`, `$a--`, `--$a`).
///
/// For numeric types (int, float), the type is preserved.
/// For numeric strings, the result becomes `int|float`.
/// For general strings, PHP increments alphabetically (stays string).
fn process_increment_decrement<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    _ctx: &ForwardWalkCtx<'_>,
) {
    use mago_syntax::ast::unary::{UnaryPostfixOperator, UnaryPrefixOperator};

    let var_expr = match expr {
        Expression::UnaryPostfix(postfix) => match &postfix.operator {
            UnaryPostfixOperator::PostIncrement(_) | UnaryPostfixOperator::PostDecrement(_) => {
                postfix.operand
            }
        },
        Expression::UnaryPrefix(prefix) => match &prefix.operator {
            UnaryPrefixOperator::PreIncrement(_) | UnaryPrefixOperator::PreDecrement(_) => {
                prefix.operand
            }
            _ => return,
        },
        _ => return,
    };

    let var_name = match var_expr {
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        _ => return,
    };

    let existing = scope.get(&var_name).to_vec();
    if existing.is_empty() {
        return;
    }

    // Check if the type is numeric or a numeric-string (including
    // literal string values like '123').  If so, increment produces
    // int|float because PHP converts numeric strings to numbers.
    let current_type = ResolvedType::types_joined(&existing);
    let is_numeric_like = {
        let lower = current_type.to_string().to_ascii_lowercase();
        lower == "numeric" || lower == "numeric-string"
    } || current_type.is_subtype_of(&PhpType::Named("numeric-string".into()));
    if is_numeric_like {
        scope.set(
            &var_name,
            vec![ResolvedType::from_type_string(PhpType::Union(vec![
                PhpType::int(),
                PhpType::float(),
            ]))],
        );
    } else if current_type.is_string_literal() {
        // Non-numeric string literal: PHP increments alphabetically
        // (e.g. "a" → "b"), so the result is still a string but no
        // longer the same literal value.  Widen to `string`.
        scope.set(
            &var_name,
            vec![ResolvedType::from_type_string(PhpType::string())],
        );
    }
    // For int, float, plain string: the type stays the same
    // (PHP preserves the type for numeric increment/decrement).
}

/// Get the byte offset of an expression (used for cursor comparisons).
fn stmt_offset(expr: &Expression<'_>) -> u32 {
    expr.span().start.offset
}

// ─── `&&` chain narrowing for diagnostic scope snapshots ────────────────────

/// Collect operands of a `&&` chain into a left-to-right list.
///
/// `a && b && c` is parsed as `(a && b) && c`.  This function flattens
/// it into `[a, b, c]`.  Non-`&&` expressions return a single-element
/// list.
fn collect_and_chain_operands<'b>(expr: &'b Expression<'b>) -> Vec<&'b Expression<'b>> {
    let mut operands = Vec::new();
    collect_and_chain_operands_inner(expr, &mut operands);
    operands
}

fn collect_and_chain_operands_inner<'b>(
    expr: &'b Expression<'b>,
    out: &mut Vec<&'b Expression<'b>>,
) {
    if let Expression::Binary(bin) = expr
        && matches!(
            bin.operator,
            BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
        )
    {
        collect_and_chain_operands_inner(bin.lhs, out);
        collect_and_chain_operands_inner(bin.rhs, out);
        return;
    }
    // Also unwrap parenthesised `&&` chains.
    if let Expression::Parenthesized(inner) = expr {
        let inner_ops = collect_and_chain_operands(inner.expression);
        if inner_ops.len() > 1 {
            out.extend(inner_ops);
            return;
        }
    }
    out.push(expr);
}

fn collect_or_chain_operands<'b>(expr: &'b Expression<'b>) -> Vec<&'b Expression<'b>> {
    let mut operands = Vec::new();
    collect_or_chain_operands_inner(expr, &mut operands);
    operands
}

fn collect_or_chain_operands_inner<'b>(
    expr: &'b Expression<'b>,
    out: &mut Vec<&'b Expression<'b>>,
) {
    if let Expression::Binary(bin) = expr
        && matches!(
            bin.operator,
            BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
        )
    {
        collect_or_chain_operands_inner(bin.lhs, out);
        collect_or_chain_operands_inner(bin.rhs, out);
        return;
    }
    // Also unwrap parenthesised `||` chains.
    if let Expression::Parenthesized(inner) = expr {
        let inner_ops = collect_or_chain_operands(inner.expression);
        if inner_ops.len() > 1 {
            out.extend(inner_ops);
            return;
        }
    }
    out.push(expr);
}

/// Walk an expression tree looking for `match(true)` arms and ternary
/// `instanceof` patterns.  When found, clone the scope, apply per-arm
/// or per-branch narrowing, and record scope snapshots so that member
/// accesses inside the narrowed context see the correct variable types.
///
/// Unlike [`record_scope_snapshot_recursive`], this function does NOT
/// record snapshots at every sub-expression offset.  It only writes
/// snapshots at offsets inside match arms and ternary branches where
/// narrowing applies.  This avoids polluting the scope cache with
/// redundant entries that could conflict with `&&`-chain snapshots.
fn record_match_ternary_snapshots<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match expr {
        Expression::Match(match_expr) if match_expr.expression.is_true() => {
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(expr_arm) => {
                        let mut arm_scope = scope.clone();
                        for condition in expr_arm.conditions.iter() {
                            apply_condition_narrowing(condition, &mut arm_scope, ctx);
                        }
                        record_scope_snapshot(expr_arm.expression.span().start.offset, &arm_scope);
                        record_scope_snapshot_recursive(expr_arm.expression, &arm_scope);
                        // Recurse into the arm body for nested patterns.
                        record_match_ternary_snapshots(expr_arm.expression, &arm_scope, ctx);
                    }
                    MatchArm::Default(def_arm) => {
                        record_scope_snapshot(def_arm.expression.span().start.offset, scope);
                        record_scope_snapshot_recursive(def_arm.expression, scope);
                        record_match_ternary_snapshots(def_arm.expression, scope, ctx);
                    }
                }
            }
        }
        Expression::Conditional(conditional) => {
            // Only apply narrowing when the condition contains an
            // instanceof check (simple or compound OR).  General
            // truthiness/null narrowing is too broad and can produce
            // incorrect scope snapshots for arbitrary ternaries.
            let has_instanceof = {
                let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
                var_names.iter().any(|vn| {
                    narrowing::try_extract_instanceof(conditional.condition, vn).is_some()
                        || narrowing::try_extract_compound_or_instanceof(conditional.condition, vn)
                            .is_some()
                })
            };
            if has_instanceof {
                let mut then_scope = scope.clone();
                apply_condition_narrowing(conditional.condition, &mut then_scope, ctx);
                if let Some(then_expr) = conditional.then {
                    record_scope_snapshot(then_expr.span().start.offset, &then_scope);
                    record_scope_snapshot_recursive(then_expr, &then_scope);
                    record_match_ternary_snapshots(then_expr, &then_scope, ctx);
                }
                let mut else_scope = scope.clone();
                apply_condition_narrowing_inverse(conditional.condition, &mut else_scope, ctx);
                record_scope_snapshot(conditional.r#else.span().start.offset, &else_scope);
                record_scope_snapshot_recursive(conditional.r#else, &else_scope);
                record_match_ternary_snapshots(conditional.r#else, &else_scope, ctx);
            } else {
                // No instanceof — just recurse for nested patterns.
                if let Some(then_expr) = conditional.then {
                    record_match_ternary_snapshots(then_expr, scope, ctx);
                }
                record_match_ternary_snapshots(conditional.r#else, scope, ctx);
            }
        }
        Expression::Assignment(assignment) => {
            record_match_ternary_snapshots(assignment.rhs, scope, ctx);
        }
        Expression::Parenthesized(inner) => {
            record_match_ternary_snapshots(inner.expression, scope, ctx);
        }
        Expression::Call(call) => {
            let args = match call {
                Call::Function(fc) => {
                    record_match_ternary_snapshots(fc.function, scope, ctx);
                    &fc.argument_list
                }
                Call::Method(mc) => {
                    record_match_ternary_snapshots(mc.object, scope, ctx);
                    &mc.argument_list
                }
                Call::NullSafeMethod(mc) => {
                    record_match_ternary_snapshots(mc.object, scope, ctx);
                    &mc.argument_list
                }
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for arg in args.arguments.iter() {
                let arg_expr = match arg {
                    Argument::Positional(a) => a.value,
                    Argument::Named(a) => a.value,
                };
                record_match_ternary_snapshots(arg_expr, scope, ctx);
            }
        }
        Expression::Binary(bin) => {
            record_match_ternary_snapshots(bin.lhs, scope, ctx);
            record_match_ternary_snapshots(bin.rhs, scope, ctx);
        }
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                let elem_expr = match elem {
                    ArrayElement::KeyValue(kv) => {
                        record_match_ternary_snapshots(kv.key, scope, ctx);
                        kv.value
                    }
                    ArrayElement::Value(val) => val.value,
                    ArrayElement::Variadic(v) => v.value,
                    ArrayElement::Missing(_) => continue,
                };
                record_match_ternary_snapshots(elem_expr, scope, ctx);
            }
        }
        // Match expressions where the subject is NOT `true` — just
        // recurse into arm expressions.
        Expression::Match(match_expr) => {
            for arm in match_expr.arms.iter() {
                let arm_expr = match arm {
                    MatchArm::Expression(e) => e.expression,
                    MatchArm::Default(d) => d.expression,
                };
                record_match_ternary_snapshots(arm_expr, scope, ctx);
            }
        }
        _ => {}
    }
}

// ─── Completion-path ternary/match(true) narrowing ──────────────────────────

/// Walk an expression tree looking for a `match(true)` arm or ternary
/// `instanceof` branch that contains the cursor.  When found, apply
/// the appropriate narrowing to `scope` so that variable lookups see
/// the narrowed type.
///
/// This is the completion-path counterpart of
/// [`record_match_ternary_snapshots`], which records scope snapshots
/// for the diagnostic path.  Here we modify the live scope in-place
/// because the completion path only needs one variable's type at one
/// cursor position.
fn apply_cursor_ternary_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let cursor = ctx.cursor_offset;
    let span = expr.span();
    if cursor < span.start.offset || cursor > span.end.offset {
        return;
    }

    match expr {
        Expression::Match(match_expr) if match_expr.expression.is_true() => {
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(expr_arm) => {
                        let arm_span = expr_arm.expression.span();
                        if cursor >= arm_span.start.offset && cursor <= arm_span.end.offset {
                            for condition in expr_arm.conditions.iter() {
                                apply_condition_narrowing(condition, scope, ctx);
                            }
                            // Recurse into the arm body for nested patterns.
                            apply_cursor_ternary_narrowing(expr_arm.expression, scope, ctx);
                            return;
                        }
                    }
                    MatchArm::Default(def_arm) => {
                        let arm_span = def_arm.expression.span();
                        if cursor >= arm_span.start.offset && cursor <= arm_span.end.offset {
                            apply_cursor_ternary_narrowing(def_arm.expression, scope, ctx);
                            return;
                        }
                    }
                }
            }
        }
        Expression::Conditional(conditional) => {
            // Check if the condition contains an instanceof check for
            // any variable currently in scope.
            let has_instanceof = {
                let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
                var_names.iter().any(|vn| {
                    narrowing::try_extract_instanceof(conditional.condition, vn).is_some()
                        || narrowing::try_extract_instanceof_with_negation(
                            conditional.condition,
                            vn,
                        )
                        .is_some()
                        || narrowing::try_extract_compound_or_instanceof(conditional.condition, vn)
                            .is_some()
                })
            };
            if has_instanceof {
                if let Some(then_expr) = conditional.then {
                    let then_span = then_expr.span();
                    if cursor >= then_span.start.offset && cursor <= then_span.end.offset {
                        apply_condition_narrowing(conditional.condition, scope, ctx);
                        apply_cursor_ternary_narrowing(then_expr, scope, ctx);
                        return;
                    }
                }
                let else_span = conditional.r#else.span();
                if cursor >= else_span.start.offset && cursor <= else_span.end.offset {
                    apply_condition_narrowing_inverse(conditional.condition, scope, ctx);
                    apply_cursor_ternary_narrowing(conditional.r#else, scope, ctx);
                }
            } else {
                // No instanceof — just recurse for nested patterns.
                if let Some(then_expr) = conditional.then {
                    apply_cursor_ternary_narrowing(then_expr, scope, ctx);
                }
                apply_cursor_ternary_narrowing(conditional.r#else, scope, ctx);
            }
        }
        Expression::Assignment(assignment) => {
            apply_cursor_ternary_narrowing(assignment.rhs, scope, ctx);
        }
        Expression::Parenthesized(inner) => {
            apply_cursor_ternary_narrowing(inner.expression, scope, ctx);
        }
        Expression::Call(call) => {
            let args = match call {
                Call::Function(fc) => {
                    apply_cursor_ternary_narrowing(fc.function, scope, ctx);
                    &fc.argument_list
                }
                Call::Method(mc) => {
                    apply_cursor_ternary_narrowing(mc.object, scope, ctx);
                    &mc.argument_list
                }
                Call::NullSafeMethod(mc) => {
                    apply_cursor_ternary_narrowing(mc.object, scope, ctx);
                    &mc.argument_list
                }
                Call::StaticMethod(_) => return,
            };
            for arg in args.arguments.iter() {
                let arg_expr = match arg {
                    Argument::Positional(a) => a.value,
                    Argument::Named(a) => a.value,
                };
                apply_cursor_ternary_narrowing(arg_expr, scope, ctx);
            }
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
            ) =>
        {
            // `&&` chain: apply narrowing from LHS operands when the
            // cursor is in the RHS.  E.g. `$x instanceof Foo && $x->bar()`
            // narrows `$x` to `Foo` for the `$x->bar()` operand.
            let operands = collect_and_chain_operands(expr);
            if operands.len() >= 2 {
                let mut narrowed = false;
                for (i, operand) in operands.iter().enumerate() {
                    let op_span = operand.span();
                    if cursor >= op_span.start.offset && cursor <= op_span.end.offset {
                        // Cursor is inside this operand — apply
                        // narrowing from all preceding operands.
                        // (Already applied cumulatively in the loop.)
                        narrowed = true;
                        apply_cursor_ternary_narrowing(operand, scope, ctx);
                        break;
                    }
                    // Apply this operand's narrowing for subsequent operands.
                    if i < operands.len() - 1 {
                        apply_condition_narrowing(operand, scope, ctx);
                    }
                }
                if !narrowed {
                    // Cursor not inside any operand — just recurse.
                    apply_cursor_ternary_narrowing(bin.lhs, scope, ctx);
                    apply_cursor_ternary_narrowing(bin.rhs, scope, ctx);
                }
            } else {
                apply_cursor_ternary_narrowing(bin.lhs, scope, ctx);
                apply_cursor_ternary_narrowing(bin.rhs, scope, ctx);
            }
        }
        Expression::Binary(bin) => {
            apply_cursor_ternary_narrowing(bin.lhs, scope, ctx);
            apply_cursor_ternary_narrowing(bin.rhs, scope, ctx);
        }
        // Non-`true` match expressions — recurse into arms.
        Expression::Match(match_expr) => {
            for arm in match_expr.arms.iter() {
                let arm_expr = match arm {
                    MatchArm::Expression(e) => e.expression,
                    MatchArm::Default(d) => d.expression,
                };
                apply_cursor_ternary_narrowing(arm_expr, scope, ctx);
            }
        }
        _ => {}
    }
}

/// Record intermediate scope snapshots within `&&` chains.
///
/// When the diagnostic scope cache is active and an expression contains
/// a `&&` chain, this function:
///
/// 1. Collects the operands left-to-right.
/// 2. For each operand after the first, applies instanceof and null
///    narrowing from all previous operands to a temporary scope.
/// 3. Records a scope snapshot at the operand's byte offset so that
///    diagnostic member-access lookups within the operand see the
///    narrowed types.
///
/// This fixes patterns like:
/// - `return $x instanceof Foo && $x->bar()` — `$x` narrowed to `Foo`
///   for the `$x->bar()` span.
/// - `$x !== null && $x->method()` — `$x` narrowed to non-null for
///   the `$x->method()` span.
///
/// The narrowing is applied only to snapshots — it does NOT mutate the
/// caller's scope, so subsequent statements see the original types.
fn record_and_chain_snapshots<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if !is_diagnostic_scope_active() {
        return;
    }

    let operands = collect_and_chain_operands(expr);
    if operands.len() < 2 {
        // Not a `&&` chain — nothing to do.  The scope snapshot at
        // the statement boundary already covers single expressions.
        // Match(true) and ternary narrowing are handled separately
        // by `record_match_ternary_snapshots`.
        return;
    }

    // Apply narrowing cumulatively: each operand sees the narrowing
    // from all previous operands.
    let mut narrowed_scope = scope.clone();
    for (i, operand) in operands.iter().enumerate() {
        if i == 0 {
            // First operand: apply its narrowing for subsequent operands.
            apply_condition_narrowing(operand, &mut narrowed_scope, ctx);
            continue;
        }

        // Record a snapshot at this operand's start offset so that
        // member accesses within it see the narrowed types.
        record_scope_snapshot(operand.span().start.offset, &narrowed_scope);

        // Also recurse into sub-expressions of this operand that might
        // contain member accesses at deeper byte offsets.  For example,
        // `is_array($x->errorInfo)` — the access `$x->errorInfo` is
        // inside a function call argument.
        record_scope_snapshot_recursive(operand, &narrowed_scope);

        // Apply this operand's narrowing for the next operand.
        apply_condition_narrowing(operand, &mut narrowed_scope, ctx);
    }
}

/// Recursively record scope snapshots at every sub-expression offset
/// within an expression.  This ensures that member accesses nested
/// inside function calls, array accesses, ternaries, etc. within a
/// `&&` chain operand see the narrowed scope.
fn record_scope_snapshot_recursive(expr: &Expression<'_>, scope: &ScopeState) {
    match expr {
        Expression::Call(call) => {
            let args = match call {
                Call::Function(fc) => {
                    // Record at the function call's argument list.
                    for arg in fc.argument_list.arguments.iter() {
                        let arg_expr = match arg {
                            Argument::Positional(a) => a.value,
                            Argument::Named(a) => a.value,
                        };
                        record_scope_snapshot(arg_expr.span().start.offset, scope);
                        record_scope_snapshot_recursive(arg_expr, scope);
                    }
                    return;
                }
                Call::Method(mc) => {
                    record_scope_snapshot(mc.object.span().start.offset, scope);
                    record_scope_snapshot_recursive(mc.object, scope);
                    &mc.argument_list
                }
                Call::NullSafeMethod(mc) => {
                    record_scope_snapshot(mc.object.span().start.offset, scope);
                    record_scope_snapshot_recursive(mc.object, scope);
                    &mc.argument_list
                }
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for arg in args.arguments.iter() {
                let arg_expr = match arg {
                    Argument::Positional(a) => a.value,
                    Argument::Named(a) => a.value,
                };
                record_scope_snapshot(arg_expr.span().start.offset, scope);
                record_scope_snapshot_recursive(arg_expr, scope);
            }
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => {
                record_scope_snapshot(pa.object.span().start.offset, scope);
                record_scope_snapshot_recursive(pa.object, scope);
            }
            Access::NullSafeProperty(pa) => {
                record_scope_snapshot(pa.object.span().start.offset, scope);
                record_scope_snapshot_recursive(pa.object, scope);
            }
            Access::StaticProperty(sp) => {
                record_scope_snapshot(sp.span().start.offset, scope);
            }
            Access::ClassConstant(cc) => {
                record_scope_snapshot(cc.span().start.offset, scope);
            }
        },
        Expression::Parenthesized(inner) => {
            record_scope_snapshot(inner.expression.span().start.offset, scope);
            record_scope_snapshot_recursive(inner.expression, scope);
        }
        Expression::Binary(bin) => {
            record_scope_snapshot(bin.lhs.span().start.offset, scope);
            record_scope_snapshot_recursive(bin.lhs, scope);
            record_scope_snapshot(bin.rhs.span().start.offset, scope);
            record_scope_snapshot_recursive(bin.rhs, scope);
        }
        Expression::UnaryPrefix(prefix) => {
            record_scope_snapshot(prefix.operand.span().start.offset, scope);
            record_scope_snapshot_recursive(prefix.operand, scope);
        }
        Expression::Conditional(conditional) => {
            if let Some(then_expr) = conditional.then {
                record_scope_snapshot(then_expr.span().start.offset, scope);
                record_scope_snapshot_recursive(then_expr, scope);
            }
            record_scope_snapshot(conditional.r#else.span().start.offset, scope);
            record_scope_snapshot_recursive(conditional.r#else, scope);
        }
        Expression::ArrayAccess(aa) => {
            record_scope_snapshot(aa.array.span().start.offset, scope);
            record_scope_snapshot_recursive(aa.array, scope);
        }
        _ => {}
    }
}

/// Try to process an inline `/** @var Type $x */` docblock override.
///
/// Returns `true` if an override was found and applied.
/// Result of [`try_process_inline_var_override`].
enum VarOverrideResult {
    /// No `@var` docblock found.
    None,
    /// A `@var Type $varName` block (with explicit variable name) was
    /// applied.  The caller should re-record the scope snapshot so that
    /// lookups within the same statement see the updated types.
    NamedVar,
    /// A `@var Type` block (without variable name) was applied to the
    /// assignment LHS.  The caller must NOT re-record the snapshot
    /// because the LHS variable should not be visible in the RHS.
    NoVar,
}

fn try_process_inline_var_override<'b>(
    expr: &'b Expression<'b>,
    expr_offset: u32,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> VarOverrideResult {
    // Parse the inline @var docblock at this expression's position.
    let offset = expr_offset as usize;
    if offset == 0 {
        return VarOverrideResult::None;
    }

    // Look for `/** @var Type $varName */` before this expression.
    let before = &ctx.content[..offset.min(ctx.content.len())];
    let trimmed = before.trim_end();

    // Quick check: does it end with `*/`?
    if !trimmed.ends_with("*/") {
        return VarOverrideResult::None;
    }

    // Find the docblock start.
    let doc_end = trimmed.len();
    let doc_start = if let Some(pos) = trimmed.rfind("/**") {
        pos
    } else {
        return VarOverrideResult::None;
    };

    let doc_text = &trimmed[doc_start..doc_end];

    // Try multi-@var first: a single docblock may declare several
    // variables (e.g. `/** @var App $app  @var array{…} $params */`).
    let multi = parse_all_inline_var_docblocks(doc_text, ctx);
    if !multi.is_empty() {
        // When the cursor is inside the RHS of an assignment, skip
        // overriding the LHS variable so that hover/completion on the
        // RHS sees the pre-override type.  E.g.:
        //   /** @var array<string, mixed> $response */
        //   $response = $response->json();
        // Hovering on the RHS `$response` should show `ApiResponse`,
        // not `array<string, mixed>`.
        let skip_var: Option<String> = if let Expression::Assignment(assignment) = expr {
            let rhs_span = assignment.rhs.span();
            let cursor_in_rhs = ctx.cursor_offset >= rhs_span.start.offset
                && ctx.cursor_offset <= rhs_span.end.offset;
            if cursor_in_rhs {
                if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                    Some(bytes_to_str(dv.name).to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        for (var_name, php_type) in &multi {
            if skip_var.as_deref() == Some(var_name.as_str()) {
                continue;
            }
            let resolved = resolve_type_to_resolved_types(php_type, ctx);
            scope.set(var_name, resolved);
        }
        // After processing the immediate docblock, scan backwards for
        // additional standalone docblocks that precede it.  This handles
        // patterns like:
        //   /** @var App $app  @var array{…} $params */
        //   /** @var Client $client */
        //   $client = $app->make(Client::class);
        // where the first block is separated from the expression by
        // another docblock.
        apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);

        // When the @var variable names all differ from the assignment
        // LHS, return None so the caller continues processing the
        // assignment.  E.g.:
        //   /** @var Foo[] $items */
        //   $item = array_shift($items);
        // The @var sets `$items` in scope (done above), and the caller
        // must also process `$item = array_shift($items)`.
        //
        // When any @var name matches the LHS, return NamedVar so the
        // caller skips the assignment (the @var type is authoritative).
        if let Expression::Assignment(assignment) = expr
            && let Expression::Variable(Variable::Direct(dv)) = assignment.lhs
        {
            let lhs_name = bytes_to_str(dv.name).to_string();
            if !multi.iter().any(|(n, _)| *n == lhs_name) {
                return VarOverrideResult::None;
            }
        }
        return VarOverrideResult::NamedVar;
    }

    // Also check for `/** @var Type */` without variable name — this
    // applies to the immediately following expression if it's a simple
    // variable or assignment.
    if let Some(php_type) = parse_inline_var_docblock_no_var(doc_text, ctx) {
        let resolved = resolve_type_to_resolved_types(&php_type, ctx);
        if let Expression::Assignment(assignment) = expr {
            if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                // When the cursor is inside the RHS, skip the override
                // so that the variable retains its pre-assignment type.
                // E.g. `/** @var array<string, mixed> */ $data = $data->toArray()`
                // — the cursor on `$data->` in the RHS should see Data, not array.
                let rhs_span = assignment.rhs.span();
                let cursor_in_rhs = ctx.cursor_offset >= rhs_span.start.offset
                    && ctx.cursor_offset <= rhs_span.end.offset;
                if cursor_in_rhs {
                    return VarOverrideResult::None;
                }

                // Scalar-blocking: when the RHS resolves to a concrete
                // scalar type (string, int, bool, etc.), reject a class
                // `@var` override.  E.g. `/** @var Session */ $s =
                // $this->getName()` where `getName()` returns `string`
                // should NOT override `$s` to `Session`.
                let native_type = resolve_rhs_native_type(assignment.rhs, scope, ctx);
                if let Some(ref native) = native_type
                    && !crate::docblock::should_override_type_typed(&php_type, native)
                {
                    // The override was rejected (scalar blocking).
                    return VarOverrideResult::None;
                }

                let var_name = bytes_to_str(dv.name).to_string();
                scope.set(&var_name, resolved);
                // Scan for preceding docblocks.
                apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);
                return VarOverrideResult::NoVar;
            }
        } else if let Expression::Variable(Variable::Direct(dv)) = expr {
            let var_name = bytes_to_str(dv.name).to_string();
            scope.set(&var_name, resolved);
            apply_preceding_var_docblocks(&trimmed[..doc_start], scope, ctx);
            return VarOverrideResult::NoVar;
        }
    }

    VarOverrideResult::None
}

/// Extract the native type of an RHS expression using the current scope.
///
/// Used by [`try_process_inline_var_override`] to determine whether a
/// `@var` override should be blocked by a scalar native type.
///
/// This delegates to [`super::resolution::extract_native_type_from_rhs`]
/// via a `VarResolutionCtx` that has scope-based variable resolution.
/// That function already handles method calls, function calls, static
/// calls, casts, literals, and other patterns — including extracting
/// scalar return types from method signatures.
fn resolve_rhs_native_type(
    rhs: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = move |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope("$__rhs_check", 0, &scope_resolver);
    super::resolution::extract_native_type_from_rhs(rhs, &var_ctx)
}

/// Scan backwards through `before` (content before a docblock we already
/// processed) for additional standalone `/** @var Type $var */` blocks.
/// Each discovered block's `@var` tags are applied to `scope`.  Stops as
/// soon as the text no longer ends with `*/` (after trimming).
fn apply_preceding_var_docblocks(before: &str, scope: &mut ScopeState, ctx: &ForwardWalkCtx<'_>) {
    let mut remaining = before.trim_end();
    // Keep scanning as long as the preceding text ends with a docblock.
    while remaining.ends_with("*/") {
        let doc_end = remaining.len();
        let doc_start = match remaining.rfind("/**") {
            Some(pos) => pos,
            None => break,
        };
        let doc_text = &remaining[doc_start..doc_end];
        let vars = parse_all_inline_var_docblocks(doc_text, ctx);
        if vars.is_empty() {
            // Not a @var docblock — stop scanning.
            break;
        }
        for (var_name, php_type) in &vars {
            let resolved = resolve_type_to_resolved_types(php_type, ctx);
            scope.set(var_name, resolved);
        }
        remaining = remaining[..doc_start].trim_end();
    }
}

/// Parse `/** @var Type $varName */` and return (var_name, PhpType).
/// Resolve a [`PhpType`] to a complete `Vec<ResolvedType>` with
/// `class_info` populated when possible.  Falls back to a
/// type-string-only entry for scalars and unresolvable types.
fn resolve_type_to_resolved_types(
    php_type: &PhpType,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
        php_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    if !classes.is_empty() {
        ResolvedType::from_classes_with_hint(classes, php_type.clone())
    } else {
        vec![ResolvedType::from_type_string(php_type.clone())]
    }
}

/// Parse ALL `@var Type $varName` pairs from a docblock.  Returns an
/// empty vec when none are found.  Handles multi-line docblocks like:
/// ```text
/// /**
///  * @var App                      $app
///  * @var array{indexName: string} $params
///  */
/// ```
fn parse_all_inline_var_docblocks(
    doc_text: &str,
    _ctx: &ForwardWalkCtx<'_>,
) -> Vec<(String, PhpType)> {
    let inner = match doc_text
        .strip_prefix("/**")
        .and_then(|s| s.strip_suffix("*/"))
    {
        Some(s) => s,
        None => return vec![],
    };

    let mut results = Vec::new();

    // Split on `@var` and process each occurrence.
    let mut search_from = 0;
    while let Some(pos) = inner[search_from..].find("@var") {
        let abs_pos = search_from + pos;
        let after = inner[abs_pos + 4..].trim_start();

        // Find the `$` that starts the variable name.  The type string
        // may contain spaces (e.g. `array<string, int>`).
        if let Some(dollar_pos) = after.find('$') {
            if dollar_pos > 0
                && let type_str = after[..dollar_pos].trim()
                && !type_str.is_empty()
                && let rest = &after[dollar_pos..]
                && let Some(var_name) = rest.split_whitespace().next()
                && !var_name.is_empty()
            {
                let php_type = PhpType::parse(type_str);
                results.push((var_name.to_string(), php_type));
            }
            search_from = abs_pos + 4 + dollar_pos + 1;
        } else {
            // No `$` after this @var — skip it.
            search_from = abs_pos + 4;
        }
    }

    results
}

/// Parse ALL `@var Type $varName` annotations from a docblock.
/// Supports both single-line (`/** @var Type $var */`) and multi-line
/// docblocks with multiple `@var` tags.
fn parse_all_var_docblock_annotations(doc_text: &str) -> Vec<(String, PhpType)> {
    let mut results = Vec::new();
    // Strip `/**` and `*/`
    let inner = match doc_text
        .strip_prefix("/**")
        .and_then(|s| s.strip_suffix("*/"))
    {
        Some(s) => s,
        None => return results,
    };
    // Scan each line for `@var`
    for line in inner.lines() {
        let trimmed = line.trim().trim_start_matches('*').trim();
        if let Some(rest) = trimmed.strip_prefix("@var") {
            let rest = rest.trim_start();
            // Find the `$` that starts the variable name.
            if let Some(dollar_pos) = rest.find('$') {
                if dollar_pos == 0 {
                    // `@var $var Type` format — skip.
                    continue;
                }
                let type_str = rest[..dollar_pos].trim();
                let var_part = &rest[dollar_pos..];
                let var_name = var_part.split_whitespace().next().unwrap_or("");
                if !type_str.is_empty() && !var_name.is_empty() {
                    let php_type = PhpType::parse(type_str);
                    results.push((var_name.to_string(), php_type));
                }
            }
        }
    }
    results
}

/// Parse `/** @var Type */` (without variable name) and return the PhpType.
fn parse_inline_var_docblock_no_var(doc_text: &str, _ctx: &ForwardWalkCtx<'_>) -> Option<PhpType> {
    let inner = doc_text.strip_prefix("/**")?.strip_suffix("*/")?.trim();

    let inner = inner
        .strip_prefix("@var")
        .or_else(|| inner.strip_prefix("* @var"))?;
    let inner = inner.trim();

    // For multi-line docblocks, only take the type from the first line.
    // Additional lines may contain other tags like @psalm-suppress that
    // would corrupt the type string.
    let inner = inner.lines().next().unwrap_or(inner).trim();
    // Strip trailing `*` that may remain from `* @var Type  *` formatting.
    let inner = inner.trim_end_matches('*').trim();

    // If there's a `$` it has a variable name — not the no-var form.
    if inner.contains('$') {
        return None;
    }

    if inner.is_empty() {
        return None;
    }

    Some(PhpType::parse(inner))
}

/// Process assignment expressions, updating the scope.
fn process_assignment_expr<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if let Expression::Assignment(assignment) = expr {
        if !assignment.operator.is_assign() {
            // Compound assignment: $x op= expr.
            // The type depends on the operator.
            process_compound_assignment(assignment, scope, ctx);
            return;
        }

        // Chain assignments: `$a = $b = expr` — the RHS is itself an
        // assignment expression.  Process it first so that the inner
        // variable (`$b`) gets its type before we resolve the outer one.
        if matches!(assignment.rhs, Expression::Assignment(_)) {
            process_assignment_expr(assignment.rhs, scope, ctx);
        }

        // Array destructuring: `[$a, $b] = …` / `list($a, $b) = …`
        if matches!(assignment.lhs, Expression::Array(_) | Expression::List(_)) {
            process_destructuring_assignment(assignment, scope, ctx);
            return;
        }

        // Array key assignment: `$var['key'] = expr;`
        if let Expression::ArrayAccess(array_access) = assignment.lhs {
            process_array_key_assignment(array_access, assignment, scope, ctx);
            return;
        }

        // Array push: `$var[] = expr;`
        if let Expression::ArrayAppend(array_append) = assignment.lhs {
            if let Expression::Variable(Variable::Direct(dv)) = array_append.array {
                let var_name = bytes_to_str(dv.name).to_string();
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                if !rhs_types.is_empty() {
                    let value_type = ResolvedType::types_joined(&rhs_types);
                    let base_type = scope
                        .get(&var_name)
                        .last()
                        .map(|rt| rt.type_string.clone())
                        .unwrap_or_else(PhpType::array);
                    if !base_type.is_array_shape() {
                        let merged = super::resolution::merge_push_type(&base_type, &value_type);
                        scope.set(&var_name, vec![ResolvedType::from_type_string(merged)]);
                    }
                }
            }
            return;
        }

        // Simple variable assignment: `$var = expr;`
        let lhs_name = match assignment.lhs {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => return,
        };

        // When the cursor is inside the RHS of this assignment, skip
        // storing the new type so that variable lookups within the RHS
        // see the pre-assignment type.  E.g. in `$request = new Bar(
        // name: $request->)`, the cursor on `$request->` should see
        // the old `Foo` type, not the new `Bar` type.
        let rhs_span = assignment.rhs.span();
        let cursor_in_rhs =
            ctx.cursor_offset >= rhs_span.start.offset && ctx.cursor_offset <= rhs_span.end.offset;
        if cursor_in_rhs {
            return;
        }

        let mut rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        // When the RHS is a numeric string literal (e.g. "123", '4.5'),
        // refine the type from `string` to `numeric-string` so that
        // downstream increment/decrement inference can detect it.
        if let Expression::Literal(Literal::String(lit_str)) = assignment.rhs {
            let raw = bytes_to_str(lit_str.raw).to_string();
            let unquoted = raw
                .strip_prefix('\'')
                .or_else(|| raw.strip_prefix('"'))
                .and_then(|s| s.strip_suffix('\'').or_else(|| s.strip_suffix('"')))
                .unwrap_or(&raw);
            if unquoted.parse::<i64>().is_ok() || unquoted.parse::<f64>().is_ok() {
                for rt in &mut rhs_types {
                    if rt.type_string.is_subtype_of(&PhpType::string()) {
                        rt.type_string = PhpType::Named("numeric-string".into());
                    }
                }
            }
        }
        if !rhs_types.is_empty() {
            scope.set(&lhs_name, rhs_types);
        } else if !scope.contains(&lhs_name) {
            scope.set_empty(&lhs_name);
        }
    }
}

/// Process compound assignment operators (`+=`, `-=`, `/=`, `*=`, etc.).
///
/// The result type depends on the operator kind:
/// - `.=` → string
/// - `%=` → int
/// - `<<=`, `>>=`, `&=`, `|=`, `^=` → int
/// - `+=`, `-=`, `*=`, `/=`, `**=` → int|float
/// - `??=` → union of LHS non-null type and RHS type
fn process_compound_assignment<'b>(
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    use mago_syntax::ast::assignment::AssignmentOperator;

    let var_name = match assignment.lhs {
        Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
        _ => return,
    };
    // `??=` is handled separately: its result is the union of the LHS
    // (with `null` stripped) and the RHS.  We combine the resolved types
    // directly so the `class_info` already attached to each operand is
    // preserved — collapsing to a freshly built union *type string* would
    // discard it and force a re-resolution that fails for some subjects.
    if matches!(assignment.operator, AssignmentOperator::Coalesce(_)) {
        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        let mut combined: Vec<ResolvedType> = Vec::new();
        for lt in scope.get(&var_name) {
            // Drop a bare `null` member — `??=` only keeps the LHS when it
            // is non-null.
            if lt.type_string.is_null() {
                continue;
            }
            let mut kept = lt.clone();
            if let Some(non_null) = kept.type_string.non_null_type() {
                kept.type_string = non_null;
            }
            combined.push(kept);
        }
        combined.extend(rhs_types);
        // Deduplicate by type string so an identical LHS/RHS type (e.g.
        // `Foo|null ??= new Foo()`) does not produce a redundant union.
        let mut seen: Vec<PhpType> = Vec::new();
        combined.retain(|rt| {
            if seen.contains(&rt.type_string) {
                false
            } else {
                seen.push(rt.type_string.clone());
                true
            }
        });
        if !combined.is_empty() {
            scope.set(&var_name, combined);
        } else if !scope.contains(&var_name) {
            scope.set_empty(&var_name);
        }
        return;
    }

    let result_type = match &assignment.operator {
        AssignmentOperator::Concat(_) => PhpType::string(),
        AssignmentOperator::Modulo(_) => PhpType::int(),
        AssignmentOperator::LeftShift(_)
        | AssignmentOperator::RightShift(_)
        | AssignmentOperator::BitwiseAnd(_)
        | AssignmentOperator::BitwiseOr(_)
        | AssignmentOperator::BitwiseXor(_) => PhpType::int(),
        AssignmentOperator::Addition(_)
        | AssignmentOperator::Subtraction(_)
        | AssignmentOperator::Multiplication(_)
        | AssignmentOperator::Division(_)
        | AssignmentOperator::Exponentiation(_) => {
            let lhs_types = scope.get(&var_name).to_vec();
            let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
            let is_division = matches!(assignment.operator, AssignmentOperator::Division(_));
            infer_arithmetic_result_type(&lhs_types, &rhs_types, is_division)
        }
        AssignmentOperator::Coalesce(_) | AssignmentOperator::Assign(_) => return, // handled above / elsewhere
    };

    scope.set(&var_name, vec![ResolvedType::from_type_string(result_type)]);
}

/// Unwrap parenthesized expressions to their inner expression.
fn unwrap_parens<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::Parenthesized(p) => unwrap_parens(p.expression),
        other => other,
    }
}

/// Classify a resolved operand as `int`, `float`, or unknown for
/// arithmetic type promotion.
///
/// Returns `Some(true)` for float, `Some(false)` for int/bool,
/// `None` when the type is mixed or otherwise ambiguous.
/// Handles unions and nullable types by classifying each member.
fn classify_numeric_operand(types: &[ResolvedType]) -> Option<bool> {
    if types.is_empty() {
        return None;
    }
    let mut saw_float = false;
    let mut saw_int = false;
    for rt in types {
        classify_php_type(&rt.type_string, &mut saw_float, &mut saw_int)?;
    }
    if saw_float && saw_int {
        // Both int-like and float-like members present (e.g. int|float
        // union) — the runtime result could be either, so return None
        // to fall back to the conservative int|float.
        None
    } else if saw_float {
        Some(true)
    } else if saw_int {
        Some(false)
    } else {
        None
    }
}

/// Recursively classify a `PhpType` as int-like or float-like.
///
/// Returns `None` (and short-circuits) if any member is ambiguous
/// (mixed, string, object, etc.).  Updates `saw_float` and `saw_int`
/// flags for known numeric members.  `null` members are ignored
/// since they coerce to 0 in arithmetic context.
fn classify_php_type(ty: &PhpType, saw_float: &mut bool, saw_int: &mut bool) -> Option<()> {
    match ty {
        PhpType::Named(n) => {
            let lower = n.to_ascii_lowercase();
            if lower == "float" || lower == "double" || lower == "real" {
                *saw_float = true;
            } else if lower == "int"
                || lower == "integer"
                || lower == "bool"
                || lower == "boolean"
                || lower == "true"
                || lower == "false"
            {
                *saw_int = true;
            } else if lower == "numeric" || lower == "number" {
                *saw_int = true;
                *saw_float = true;
            } else if lower == "null" {
                // null coerces to 0 (int) in arithmetic; ignore it
                // so that `int|null` classifies as int-like.
            } else {
                return None; // mixed, string, object, etc.
            }
            Some(())
        }
        PhpType::Union(members) => {
            for member in members {
                classify_php_type(member, saw_float, saw_int)?;
            }
            Some(())
        }
        PhpType::Nullable(inner) => {
            // ?T is T|null — classify the inner type, ignore null.
            classify_php_type(inner, saw_float, saw_int)
        }
        _ => None,
    }
}

/// Infer the result type of an arithmetic operation based on operand
/// types, following PHP's numeric type promotion rules.
///
/// - `int op int` → `int` (for `+`, `-`, `*`, `**`)
/// - `int op float` or `float op int` → `float`
/// - `float op float` → `float`
/// - `int / int` → `int|float` (division can produce either)
/// - Anything else → `int|float`
fn infer_arithmetic_result_type(
    lhs_types: &[ResolvedType],
    rhs_types: &[ResolvedType],
    is_division: bool,
) -> PhpType {
    let lhs = classify_numeric_operand(lhs_types);
    let rhs = classify_numeric_operand(rhs_types);
    match (lhs, rhs) {
        // Both are known int (not float): int op int.
        (Some(false), Some(false)) => {
            if is_division {
                // int / int can return float (e.g. 7/2 = 3.5).
                PhpType::Union(vec![PhpType::int(), PhpType::float()])
            } else {
                PhpType::int()
            }
        }
        // At least one float, the other is known: result is float.
        (Some(true), Some(_)) | (Some(_), Some(true)) => PhpType::float(),
        // One or both operands are unknown: fall back to int|float.
        _ => PhpType::Union(vec![PhpType::int(), PhpType::float()]),
    }
}

/// Resolve the type of an RHS expression using the current scope.
///
/// This is the key integration point: instead of calling
/// `resolve_variable_types` (which would recurse), we build a
/// `VarResolutionCtx` that already has the answer for any variable
/// references in the RHS — the forward walker has already resolved
/// them.
///
/// We delegate to `resolve_rhs_expression` with a `VarResolutionCtx`
/// whose `scope_var_resolver` reads directly from the forward walker's
/// in-progress `ScopeState`.  For bare variable references in the RHS,
/// we intercept them and return the scope-based result directly.
fn resolve_rhs_with_scope<'b>(
    rhs: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    // Chain assignment: `$a = $b = expr` — the value of an assignment
    // expression is the value of its RHS.  Recurse into the inner RHS
    // so that `$a` resolves to the same type as `$b`.
    if let Expression::Assignment(assignment) = rhs
        && assignment.operator.is_assign()
    {
        return resolve_rhs_with_scope(assignment.rhs, scope, ctx);
    }

    // Compound assignment as RHS: `$a = ($x /= 2)` — the value of the
    // compound assignment is the result after the operation.  Infer the
    // type from the operator kind.
    if let Expression::Assignment(assignment) = rhs
        && !assignment.operator.is_assign()
    {
        use mago_syntax::ast::assignment::AssignmentOperator;
        let result_type = match &assignment.operator {
            AssignmentOperator::Concat(_) => Some(PhpType::string()),
            AssignmentOperator::Modulo(_) => Some(PhpType::int()),
            AssignmentOperator::LeftShift(_)
            | AssignmentOperator::RightShift(_)
            | AssignmentOperator::BitwiseAnd(_)
            | AssignmentOperator::BitwiseOr(_)
            | AssignmentOperator::BitwiseXor(_) => Some(PhpType::int()),
            AssignmentOperator::Addition(_)
            | AssignmentOperator::Subtraction(_)
            | AssignmentOperator::Multiplication(_)
            | AssignmentOperator::Division(_)
            | AssignmentOperator::Exponentiation(_) => {
                let lhs_types = if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
                    scope.get(bytes_to_str(dv.name)).to_vec()
                } else {
                    vec![]
                };
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                let is_division = matches!(assignment.operator, AssignmentOperator::Division(_));
                Some(infer_arithmetic_result_type(
                    &lhs_types,
                    &rhs_types,
                    is_division,
                ))
            }
            AssignmentOperator::Coalesce(_) => {
                let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
                let rhs_type = if rhs_types.is_empty() {
                    PhpType::mixed()
                } else {
                    ResolvedType::types_joined(&rhs_types)
                };
                Some(rhs_type)
            }
            AssignmentOperator::Assign(_) => None,
        };
        if let Some(ty) = result_type {
            return vec![ResolvedType::from_type_string(ty)];
        }
    }

    // For bare variable references, read directly from scope.
    // This is the O(1) path that replaces the recursive backward scan.
    if let Expression::Variable(Variable::Direct(dv)) = rhs {
        let var_name = bytes_to_str(dv.name).to_string();
        let from_scope = scope.get(&var_name);
        if !from_scope.is_empty() {
            return from_scope.to_vec();
        }
        // Variable not in scope — fall through to rhs_resolution which
        // handles some special patterns.
    }

    // ── Foo::class → class-string<Foo> ──────────────────────────
    // `Foo::class` is parsed as `Access::ClassConstant` with the
    // identifier `class`.  resolve_rhs_expression doesn't return a
    // useful type for this (it looks for a constant named "class"
    // on the class and finds nothing).  Handle it here so that
    // subsequent `new $var` can resolve the class-string.
    if let Expression::Access(Access::ClassConstant(cca)) = rhs
        && let ClassLikeConstantSelector::Identifier(ident) = &cca.constant
        && ident.value == b"class"
    {
        let class_name = match cca.class {
            Expression::Identifier(id) => Some(bytes_to_str(id.value()).to_string()),
            Expression::Self_(_) | Expression::Static(_) => {
                if !ctx.current_class.name.is_empty() {
                    Some(ctx.current_class.name.to_string())
                } else {
                    None
                }
            }
            Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string()),
            _ => None,
        };
        if let Some(name) = class_name {
            let resolved_name = name.strip_prefix('\\').unwrap_or(&name);
            // Resolve the class so we can store a proper ResolvedType
            // with class_info.  This allows `new $var` to work.
            let class_string_type =
                PhpType::ClassString(Some(Box::new(PhpType::Named(resolved_name.to_string()))));
            let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
                &PhpType::Named(resolved_name.to_string()),
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            if !classes.is_empty() {
                return ResolvedType::from_classes_with_hint(classes, class_string_type);
            }
            // Even if we can't resolve the class, return a type-string-only result
            // so the variable is non-empty in scope.
            return vec![ResolvedType::from_type_string(class_string_type)];
        }
    }

    // ── Fast paths for expressions whose type is known structurally ──
    // These avoid the full resolve_rhs_expression round-trip for
    // common patterns where the result type depends only on the
    // expression kind, not on the operand types.

    // Type casts: (int)$x → int, (string)$x → string, etc.
    if let Expression::UnaryPrefix(prefix) = rhs {
        use mago_syntax::ast::unary::UnaryPrefixOperator;
        let cast_type = match &prefix.operator {
            UnaryPrefixOperator::IntCast(..) | UnaryPrefixOperator::IntegerCast(..) => {
                Some(PhpType::int())
            }
            UnaryPrefixOperator::StringCast(..) | UnaryPrefixOperator::BinaryCast(..) => {
                Some(PhpType::string())
            }
            UnaryPrefixOperator::FloatCast(..)
            | UnaryPrefixOperator::DoubleCast(..)
            | UnaryPrefixOperator::RealCast(..) => Some(PhpType::float()),
            UnaryPrefixOperator::BoolCast(..) | UnaryPrefixOperator::BooleanCast(..) => {
                Some(PhpType::bool())
            }
            UnaryPrefixOperator::ArrayCast(..) => Some(PhpType::array()),
            UnaryPrefixOperator::ObjectCast(..) => {
                // Resolve the operand type to produce an object shape:
                // - scalar → object{scalar: <type>}
                // - array shape → object{key: type, ...}
                // - otherwise → stdClass
                let operand_types = resolve_rhs_with_scope(prefix.operand, scope, ctx);
                let inner = operand_types.first().map(|rt| &rt.type_string).cloned();
                let obj_type = match inner {
                    Some(PhpType::ArrayShape(entries)) => {
                        // Widen literal types to their base types:
                        // PHP (object) cast doesn't preserve literal precision.
                        let widened = entries
                            .into_iter()
                            .map(|mut e| {
                                e.value_type = widen_literal(&e.value_type);
                                e
                            })
                            .collect();
                        PhpType::ObjectShape(widened)
                    }
                    Some(ref ty) if matches!(ty, PhpType::Named(s) if matches!(s.to_ascii_lowercase().as_str(), "int" | "integer" | "string" | "float" | "double" | "real" | "bool" | "boolean")) => {
                        PhpType::ObjectShape(vec![ShapeEntry {
                            key: Some("scalar".to_string()),
                            value_type: ty.clone(),
                            optional: false,
                        }])
                    }
                    _ => PhpType::Named("stdClass".into()),
                };
                Some(obj_type)
            }
            UnaryPrefixOperator::UnsetCast(..) => Some(PhpType::Named("null".into())),
            UnaryPrefixOperator::Negation(_) | UnaryPrefixOperator::Plus(_) => {
                // Unary +/- preserves int or float; conservatively
                // return int|float.
                Some(PhpType::Union(vec![PhpType::int(), PhpType::float()]))
            }
            UnaryPrefixOperator::BitwiseNot(_) => None, // handled below
            UnaryPrefixOperator::Not(_) => Some(PhpType::bool()),
            _ => None,
        };
        if let Some(ty) = cast_type {
            return vec![ResolvedType::from_type_string(ty)];
        }
    }

    // Bitwise NOT (~): returns string when operand is string, int otherwise.
    if let Expression::UnaryPrefix(prefix) = rhs {
        use mago_syntax::ast::unary::UnaryPrefixOperator;
        if matches!(prefix.operator, UnaryPrefixOperator::BitwiseNot(_)) {
            let operand_types = resolve_rhs_with_scope(prefix.operand, scope, ctx);
            let is_string = !operand_types.is_empty()
                && operand_types
                    .iter()
                    .all(|rt| rt.type_string.is_subtype_of(&PhpType::string()));
            return vec![ResolvedType::from_type_string(if is_string {
                PhpType::string()
            } else {
                PhpType::int()
            })];
        }
    }

    // For all other expressions, delegate to the existing RHS resolver
    // with a scope-based variable resolver injected.  When
    // `resolve_rhs_expression` (or its sub-functions like
    // `resolve_rhs_method_call_inner`, `resolve_rhs_property_access`)
    // need to resolve a variable's type, they call `resolve_var_types`
    // which checks `scope_var_resolver` first.  This reads directly
    // from the forward walker's in-progress `ScopeState`, bypassing
    // `resolve_variable_types` entirely.
    let rhs_offset = rhs.span().start.offset;
    let dummy_var = "$__rhs";
    let scope_locals = &scope.locals;
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_locals
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope(dummy_var, rhs_offset, &scope_resolver);

    let result = super::rhs_resolution::resolve_rhs_expression(rhs, &var_ctx);
    if !result.is_empty() {
        return result;
    }

    // ── Structural fallbacks ────────────────────────────────────
    // When resolve_rhs_expression returns empty, infer the type
    // purely from the expression structure.  These only fire as a
    // last resort so they never override a more precise result.

    // Unwrap parenthesized expressions for structural inference.
    let rhs = unwrap_parens(rhs);

    // String literals (including interpolated/composite strings).
    if matches!(
        rhs,
        Expression::Literal(Literal::String(_)) | Expression::CompositeString(_)
    ) {
        return vec![ResolvedType::from_type_string(PhpType::string())];
    }

    // Integer literals.
    if matches!(rhs, Expression::Literal(Literal::Integer(_))) {
        return vec![ResolvedType::from_type_string(PhpType::int())];
    }

    // Float literals.
    if matches!(rhs, Expression::Literal(Literal::Float(_))) {
        return vec![ResolvedType::from_type_string(PhpType::float())];
    }

    // Boolean and null literals.
    if matches!(
        rhs,
        Expression::Literal(Literal::True(_) | Literal::False(_))
    ) {
        return vec![ResolvedType::from_type_string(PhpType::bool())];
    }
    if matches!(rhs, Expression::Literal(Literal::Null(_))) {
        return vec![ResolvedType::from_type_string(PhpType::Named(
            "null".into(),
        ))];
    }

    // Binary operators — the result type depends on the operator kind.
    if let Expression::Binary(binary) = rhs {
        use mago_syntax::ast::binary::BinaryOperator;

        // Spaceship (<=>): always int (-1, 0, or 1).
        if matches!(binary.operator, BinaryOperator::Spaceship(_)) {
            return vec![ResolvedType::from_type_string(PhpType::int())];
        }

        // instanceof, comparison, logical: always bool.
        if binary.operator.is_instanceof()
            || binary.operator.is_comparison()
            || binary.operator.is_logical()
        {
            return vec![ResolvedType::from_type_string(PhpType::bool())];
        }

        // Concatenation (.): always string.
        if matches!(binary.operator, BinaryOperator::StringConcat(_)) {
            return vec![ResolvedType::from_type_string(PhpType::string())];
        }

        // Modulo (%): always int.
        if matches!(binary.operator, BinaryOperator::Modulo(_)) {
            return vec![ResolvedType::from_type_string(PhpType::int())];
        }

        // Addition (+): PHP overloads this for array union vs numeric
        // addition.  If either operand resolves to an array type, the
        // result is array; otherwise apply numeric type promotion.
        if matches!(binary.operator, BinaryOperator::Addition(_)) {
            let lhs_types = resolve_rhs_with_scope(binary.lhs, scope, ctx);
            let rhs_types = resolve_rhs_with_scope(binary.rhs, scope, ctx);
            let either_is_array = lhs_types
                .iter()
                .chain(rhs_types.iter())
                .any(|rt| rt.type_string.is_array_like());
            if either_is_array {
                return vec![ResolvedType::from_type_string(PhpType::Named(
                    "array".to_string(),
                ))];
            }
            return vec![ResolvedType::from_type_string(
                infer_arithmetic_result_type(&lhs_types, &rhs_types, false),
            )];
        }

        // Arithmetic: -, *, /, **.
        if matches!(
            binary.operator,
            BinaryOperator::Subtraction(_)
                | BinaryOperator::Multiplication(_)
                | BinaryOperator::Division(_)
                | BinaryOperator::Exponentiation(_)
        ) {
            let lhs_types = resolve_rhs_with_scope(binary.lhs, scope, ctx);
            let rhs_types = resolve_rhs_with_scope(binary.rhs, scope, ctx);
            let is_division = matches!(binary.operator, BinaryOperator::Division(_));
            return vec![ResolvedType::from_type_string(
                infer_arithmetic_result_type(&lhs_types, &rhs_types, is_division),
            )];
        }

        // Bitwise operators (&, |, ^, <<, >>).
        // When both operands are strings, PHP applies bitwise ops
        // character-by-character and returns a string.  Otherwise int.
        if matches!(
            binary.operator,
            BinaryOperator::BitwiseAnd(_)
                | BinaryOperator::BitwiseOr(_)
                | BinaryOperator::BitwiseXor(_)
                | BinaryOperator::LeftShift(_)
                | BinaryOperator::RightShift(_)
        ) {
            // Check if both operands are string-typed for &, |, ^.
            if matches!(
                binary.operator,
                BinaryOperator::BitwiseAnd(_)
                    | BinaryOperator::BitwiseOr(_)
                    | BinaryOperator::BitwiseXor(_)
            ) {
                let lhs_types = resolve_rhs_with_scope(binary.lhs, scope, ctx);
                let rhs_types = resolve_rhs_with_scope(binary.rhs, scope, ctx);
                let both_strings = !lhs_types.is_empty()
                    && !rhs_types.is_empty()
                    && lhs_types
                        .iter()
                        .all(|rt| rt.type_string.is_subtype_of(&PhpType::string()))
                    && rhs_types
                        .iter()
                        .all(|rt| rt.type_string.is_subtype_of(&PhpType::string()));
                if both_strings {
                    return vec![ResolvedType::from_type_string(PhpType::string())];
                }
            }
            return vec![ResolvedType::from_type_string(PhpType::int())];
        }
    }

    // ── Subject pipeline fallback ───────────────────────────────
    // When resolve_rhs_expression and the structural fallbacks both
    // return empty, try the full subject resolution pipeline
    // (resolve_target_classes).  This handles method calls and
    // static calls that resolve_rhs_expression cannot resolve
    // because the receiver or intermediate types are only reachable
    // through the subject pipeline's broader strategies (e.g.
    // docblock @return types, merged inheritance, virtual members).
    //
    // Property access (Expression::Access) is intentionally excluded
    // because resolve_target_classes resolves the *subject* (what
    // you'd complete after `->`) rather than the property's value
    // type.  For Eloquent relations like `$this->model->orderProducts`,
    // the subject pipeline returns the element type instead of the
    // collection, which breaks foreach value binding.  Property
    // access RHS resolution is handled by resolve_rhs_expression's
    // own property resolution path.
    if matches!(rhs, Expression::Call(_) | Expression::Instantiation(_)) {
        let rhs_span = rhs.span();
        let rhs_start = rhs_span.start.offset as usize;
        let rhs_end = rhs_span.end.offset as usize;
        if let Some(rhs_text) = ctx.content.get(rhs_start..rhs_end) {
            let rhs_text = rhs_text.trim();
            if !rhs_text.is_empty() {
                let subject_result = resolve_rhs_via_subject(rhs_text, scope, ctx);
                if !subject_result.is_empty() {
                    return subject_result;
                }
            }
        }
    }

    result
}

/// Resolve an RHS expression through the full subject pipeline.
///
/// This is a last-resort fallback for expressions that
/// `resolve_rhs_expression` can't handle.  It extracts the
/// expression text and passes it to `resolve_target_classes`, which
/// goes through SubjectExpr parsing, property/method chain
/// resolution, and the full type resolution infrastructure.
///
/// Only called for method calls, property access, static calls, and
/// instantiation — expression kinds that typically produce
/// object-typed results resolvable through the subject pipeline.
fn resolve_rhs_via_subject(
    rhs_text: &str,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = move |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope("$__rhs_subject", 0, &scope_resolver);
    let rctx = var_ctx.as_resolution_ctx();

    // Determine the access kind from the expression text.
    let access_kind = if rhs_text.contains("::") {
        crate::types::AccessKind::DoubleColon
    } else {
        crate::types::AccessKind::Arrow
    };

    crate::completion::resolver::resolve_target_classes(rhs_text, access_kind, &rctx)
}

/// Process array destructuring assignments.
///
/// Resolves the RHS type once, then walks the LHS pattern to assign
/// types to each destructured variable.  Handles nested patterns like
/// `[$a, [$b, $c]] = $nested` by recursing into inner array/list
/// expressions.
fn process_destructuring_assignment<'b>(
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };

    // Build a temporary VarResolutionCtx just to resolve the RHS type.
    // The var_name doesn't matter here since we're resolving the RHS
    // expression, not looking up a specific variable.
    let dummy_name = String::from("$__destructuring_rhs");
    let var_ctx = VarResolutionCtx {
        var_name: &dummy_name,
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: assignment.span().start.offset,
        class_loader: ctx.class_loader,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: ctx.enclosing_return_type.clone(),
        top_level_scope: ctx.top_level_scope.clone(),
        branch_aware: false,
        match_arm_narrowing: HashMap::new(),
        scope_var_resolver: Some(&scope_resolver),
    };

    // Try inline @var docblock first, then fall back to RHS expression.
    let stmt_offset = assignment.span().start.offset as usize;
    let raw_type: Option<PhpType> =
        crate::docblock::find_inline_var_docblock(ctx.content, stmt_offset)
            .map(|(vt, _)| crate::util::resolve_php_type_names(&vt, ctx.class_loader))
            .or_else(|| {
                super::foreach_resolution::resolve_expression_type(assignment.rhs, &var_ctx)
            });

    // Expand type aliases before shape/generic extraction.
    let raw_type = raw_type.map(|rt| {
        crate::completion::type_resolution::resolve_type_alias_typed(
            &rt,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        )
        .unwrap_or(rt)
    });

    if let Some(ref rhs_type) = raw_type {
        bind_destructured_pattern(assignment.lhs, rhs_type, scope, ctx);
    }
}

/// Recursively bind types from a destructuring LHS pattern against a
/// resolved RHS type.  For each variable in the pattern, extracts the
/// corresponding type from the RHS type (via shape key or positional
/// index) and sets it in scope.  For nested array/list sub-patterns,
/// recurses with the extracted element type.
fn bind_destructured_pattern<'b>(
    lhs: &'b Expression<'b>,
    rhs_type: &PhpType,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let elements: Vec<&ArrayElement<'b>> = match lhs {
        Expression::Array(arr) => arr.elements.iter().collect(),
        Expression::List(list) => list.elements.iter().collect(),
        _ => return,
    };

    let mut positional_index: usize = 0;
    for elem in elements {
        let (value_expr, shape_key) = match elem {
            ArrayElement::KeyValue(kv) => {
                let key = extract_foreach_destr_key(kv.key);
                (kv.value, key)
            }
            ArrayElement::Value(val) => {
                let key = Some(positional_index.to_string());
                positional_index += 1;
                (val.value, key)
            }
            _ => continue,
        };

        // Determine the type for this element position.
        let elem_type: Option<PhpType> = shape_key
            .as_ref()
            .and_then(|k| rhs_type.shape_value_type(k).cloned())
            .or_else(|| rhs_type.extract_value_type(false).cloned());

        match value_expr {
            // Direct variable: bind the type.
            Expression::Variable(Variable::Direct(dv)) => {
                if let Some(ref vt) = elem_type {
                    let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                        vt,
                        &ctx.current_class.name,
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                    let resolved_types = if !resolved.is_empty() {
                        ResolvedType::from_classes_with_hint(resolved, vt.clone())
                    } else {
                        vec![ResolvedType::from_type_string(vt.clone())]
                    };
                    scope.set(bytes_to_str(dv.name), resolved_types);
                }
            }
            // Nested pattern: recurse with the extracted element type.
            Expression::Array(_) | Expression::List(_) => {
                if let Some(ref vt) = elem_type {
                    bind_destructured_pattern(value_expr, vt, scope, ctx);
                }
            }
            _ => {}
        }
    }
}

/// Process array key assignment: `$var['key'] = expr;`
fn process_array_key_assignment<'b>(
    _array_access: &'b ArrayAccess<'b>,
    assignment: &'b Assignment<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Delegate to the existing check_expression_for_assignment
    // infrastructure for array key assignments.  This handles
    // both string-keyed shape building and generic element tracking.
    //
    // We iterate over all variables currently in scope and check
    // whether the assignment targets any of them.
    // For simplicity in Phase 1, use the existing path.
    // Extract the base variable name from the array access.
    if let Some((base_name, key_chain)) =
        super::resolution::extract_nested_array_access_chain(_array_access)
    {
        // Resolve the RHS value type.
        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        let value_php_type = if !rhs_types.is_empty() {
            ResolvedType::types_joined(&rhs_types)
        } else {
            PhpType::mixed()
        };
        let base_type = scope
            .get(&base_name)
            .last()
            .map(|rt| rt.type_string.clone())
            .unwrap_or_else(PhpType::array);

        // If the base variable is an object (e.g. SplObjectStorage, ArrayAccess),
        // array-access syntax invokes offsetSet, not actual array mutation.
        // Preserve the original object type instead of overwriting it with an array shape.
        if base_type.is_object_like() && !base_type.is_array_like() {
            return;
        }

        // Extract all keys in the chain.
        let all_string_keys: Option<Vec<String>> = key_chain
            .iter()
            .map(|idx| super::resolution::extract_array_key_for_shape(idx))
            .collect();

        if let Some(keys) = all_string_keys {
            let merged =
                super::resolution::merge_nested_shape_keys(&base_type, &keys, &value_php_type);
            scope.set(&base_name, vec![ResolvedType::from_type_string(merged)]);
        } else if key_chain.len() == 1 && !base_type.is_array_shape() {
            let rhs_offset = assignment.span().start.offset;
            let scope_locals = &scope.locals;
            let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
                scope_locals
                    .get(&atom(var_name))
                    .cloned()
                    .unwrap_or_default()
            };
            let rhs_ctx = ctx.var_ctx_for_with_scope("$__idx", rhs_offset, &scope_resolver);
            let key_php_type = super::resolution::infer_array_key_type(key_chain[0], &rhs_ctx);
            let merged =
                super::resolution::merge_keyed_type(&base_type, &key_php_type, &value_php_type);
            scope.set(&base_name, vec![ResolvedType::from_type_string(merged)]);
        }
    }
}

/// Process pass-by-reference parameter type inference.
fn process_pass_by_ref<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // When a function call passes a variable to a parameter declared
    // as `Type &$param`, the variable acquires that type after the call.
    //
    // We need to check both variables already in scope AND variables
    // that appear as arguments but don't exist in scope yet (e.g.
    // `$matches` in `preg_match($pattern, $subject, $matches)`).
    //
    // Phase 1: use the existing `try_apply_pass_by_reference_type`
    // infrastructure for variables already in scope (works for class
    // types like `Type &$param`).
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };

    // Collect all variable names that appear as arguments in this
    // expression, including ones not yet in scope.
    let mut all_var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    for arg_var in extract_call_arg_variables(expr) {
        if !all_var_names.contains(&arg_var) {
            all_var_names.push(arg_var);
        }
    }

    for var_name in all_var_names {
        let var_ctx = VarResolutionCtx {
            var_name: &var_name,
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope: ctx.top_level_scope.clone(),
            branch_aware: false,
            match_arm_narrowing: HashMap::new(),
            scope_var_resolver: Some(&scope_resolver),
        };
        let before = scope.get(&var_name).to_vec();
        let mut results = before.clone();
        super::resolution::try_apply_pass_by_reference_type(expr, &var_ctx, &mut results, false);
        if results.len() != before.len() {
            scope.set(&var_name, results);
        }
    }

    // Phase 2: for variables NOT yet in scope that are passed to
    // pass-by-reference parameters with primitive type hints (e.g.
    // `array &$matches` in `preg_match`), store the type hint
    // directly.  `try_apply_pass_by_reference_type` only produces
    // results for class-based type hints; primitive types like
    // `array`, `int`, `string` return empty from
    // `type_hint_to_classes_typed` and are missed.
    seed_pass_by_ref_primitives(expr, scope, ctx);
}

/// Seed PHP superglobals (`$_SERVER`, `$_GET`, `$_POST`, etc.) into the
/// scope as `array` so that accesses on them resolve correctly.
/// PHP makes these available in every scope without
/// an explicit `global` declaration.
fn seed_superglobals(scope: &mut ScopeState) {
    let array_type = vec![ResolvedType::from_type_string(PhpType::Named(
        "array".to_string(),
    ))];
    for name in [
        "$_SERVER",
        "$_GET",
        "$_POST",
        "$_COOKIE",
        "$_REQUEST",
        "$_FILES",
        "$_ENV",
        "$_SESSION",
        "$GLOBALS",
    ] {
        scope.set(name, array_type.clone());
    }
}

/// Recursively walk an expression tree to find function call
/// sub-expressions and seed pass-by-reference primitive types for each.
/// This handles patterns like `if (preg_match($pattern, $subject, $matches))`
/// and `if (preg_match(..., $matches) === 1)` where the call is nested
/// inside a comparison or logical expression rather than appearing as a
/// standalone expression statement.
///
/// Only uses [`seed_pass_by_ref_primitives`] (not the full
/// [`process_pass_by_ref`]) to avoid triggering recursive variable
/// resolution through `try_apply_pass_by_reference_type`, which would
/// inflate the fallthrough counter for every variable already in scope.
fn seed_pass_by_ref_in_condition<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match expr {
        // Direct call expressions — seed primitive pass-by-ref types.
        Expression::Call(_) => {
            seed_pass_by_ref_primitives(expr, scope, ctx);
        }
        // Binary operators (e.g. `preg_match(...) === 1`, `a && b`)
        // — recurse into both sides.
        Expression::Binary(bin) => {
            seed_pass_by_ref_in_condition(bin.lhs, scope, ctx);
            seed_pass_by_ref_in_condition(bin.rhs, scope, ctx);
        }
        // Unary prefix (e.g. `!preg_match(...)`) — recurse into operand.
        Expression::UnaryPrefix(unary) => {
            seed_pass_by_ref_in_condition(unary.operand, scope, ctx);
        }
        // Unary postfix — recurse into operand.
        Expression::UnaryPostfix(unary) => {
            seed_pass_by_ref_in_condition(unary.operand, scope, ctx);
        }
        // Parenthesized — recurse into inner expression.
        Expression::Parenthesized(paren) => {
            seed_pass_by_ref_in_condition(paren.expression, scope, ctx);
        }
        // Assignment in condition (e.g. `if ($x = preg_match(..., $m))`)
        // — recurse into the RHS.
        Expression::Assignment(assignment) => {
            seed_pass_by_ref_in_condition(assignment.rhs, scope, ctx);
        }
        _ => {}
    }
}

/// For each variable argument in a call expression that is passed to a
/// pass-by-reference parameter with a primitive type hint (e.g.
/// `array &$matches`), seed the variable in scope if it isn't already
/// there.  This complements [`process_pass_by_ref`] which handles
/// class-typed parameters via `try_apply_pass_by_reference_type`.
fn seed_pass_by_ref_primitives<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Resolve the called function/method's parameters.
    let (arg_list, parameters) = match expr {
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let fl = match ctx.loaders.function_loader {
                Some(fl) => fl,
                None => return,
            };
            let func_info = match fl(&func_name) {
                Some(fi) => fi,
                None => return,
            };
            (&func_call.argument_list, func_info.parameters)
        }
        Expression::Call(Call::Method(mc)) => {
            let method_name = match &mc.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let receiver_class = match mc.object {
                Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
                    Some(ctx.current_class.name.to_string())
                }
                Expression::Variable(Variable::Direct(dv)) => {
                    let types = scope.get(bytes_to_str(dv.name));
                    types.iter().find_map(|rt| {
                        let name = rt.type_string.base_name()?;
                        if crate::php_type::is_primitive_scalar_name(name) {
                            None
                        } else {
                            Some(name.to_string())
                        }
                    })
                }
                _ => return,
            };
            let class_name = match receiver_class {
                Some(n) => n,
                None => return,
            };
            let cls = match (ctx.class_loader)(&class_name) {
                Some(c) => c,
                None => return,
            };
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method = match merged.get_method(&method_name) {
                Some(m) => m,
                None => return,
            };
            (&mc.argument_list, method.parameters.clone())
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            let method_name = match &mc.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let receiver_class = match mc.object {
                Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
                    Some(ctx.current_class.name.to_string())
                }
                Expression::Variable(Variable::Direct(dv)) => {
                    let types = scope.get(bytes_to_str(dv.name));
                    types.iter().find_map(|rt| {
                        let name = rt.type_string.base_name()?;
                        if crate::php_type::is_primitive_scalar_name(name) {
                            None
                        } else {
                            Some(name.to_string())
                        }
                    })
                }
                _ => return,
            };
            let class_name = match receiver_class {
                Some(n) => n,
                None => return,
            };
            let cls = match (ctx.class_loader)(&class_name) {
                Some(c) => c,
                None => return,
            };
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method = match merged.get_method(&method_name) {
                Some(m) => m,
                None => return,
            };
            (&mc.argument_list, method.parameters.clone())
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            let method_name = match &sc.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let class_name = match sc.class {
                Expression::Self_(_) | Expression::Static(_) => ctx.current_class.name.to_string(),
                Expression::Parent(_) => match ctx.current_class.parent_class {
                    Some(p) => p.to_string(),
                    None => return,
                },
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let cls = match (ctx.class_loader)(&class_name) {
                Some(c) => c,
                None => return,
            };
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method = match merged.get_method(&method_name) {
                Some(m) => m,
                None => return,
            };
            (&sc.argument_list, method.parameters.clone())
        }
        _ => return,
    };

    // Bind arguments to parameters following PHP's rules so a named argument
    // seeds the parameter it actually targets, not the one at its ordinal
    // position in the call.
    let bound = crate::call_args::bind_args_to_params(&parameters, arg_list);

    for (param, arg_expr) in parameters.iter().zip(bound.iter()) {
        let arg_expr = match arg_expr {
            Some(expr) => *expr,
            None => continue,
        };

        // Only handle direct variable arguments.
        let var_name = match arg_expr {
            Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
            _ => continue,
        };

        // Skip if already in scope (Phase 1 handled it).
        if !scope.get(&var_name).is_empty() {
            continue;
        }

        // Check if the corresponding parameter is pass-by-reference.
        if param.is_reference {
            if let Some(type_hint) = &param.type_hint {
                scope.set(
                    &var_name,
                    vec![ResolvedType::from_type_string(type_hint.clone())],
                );
            } else {
                // Untyped pass-by-reference parameters (e.g. `&$matches`
                // in `preg_match`, `&$result` in `parse_str`) are most
                // commonly arrays.  Seed as `array` so that subsequent
                // array accesses like `$matches[1]` don't fall through
                // to the backward scanner.
                scope.set(
                    &var_name,
                    vec![ResolvedType::from_type_string(PhpType::Named(
                        "array".to_string(),
                    ))],
                );
            }
        }
    }
}

/// Extract all `$variable` names that appear as direct arguments in a
/// call expression.  Used by [`process_pass_by_ref`] to discover
/// variables that may be introduced by pass-by-reference parameters
/// (e.g. `$matches` in `preg_match($pattern, $subject, $matches)`).
fn extract_call_arg_variables<'b>(expr: &'b Expression<'b>) -> Vec<String> {
    let arg_list = match expr {
        Expression::Call(Call::Function(fc)) => &fc.argument_list,
        Expression::Call(Call::Method(mc)) => &mc.argument_list,
        Expression::Call(Call::NullSafeMethod(mc)) => &mc.argument_list,
        Expression::Call(Call::StaticMethod(sc)) => &sc.argument_list,
        Expression::Instantiation(inst) => match &inst.argument_list {
            Some(al) => al,
            None => return vec![],
        },
        _ => return vec![],
    };
    let mut vars = Vec::new();
    for arg in arg_list.arguments.iter() {
        let arg_expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
            vars.push(bytes_to_str(dv.name).to_string());
        }
    }
    vars
}

/// Process assert narrowing (assert($x instanceof Foo), @phpstan-assert, etc.)
fn process_assert_narrowing<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // ── Handle assert($x instanceof Foo) for variables NOT yet in scope ──
    // When a foreach binds a variable but the iterable element type is
    // unknown, the variable won't be in the scope map.  A subsequent
    // `assert($x instanceof Foo)` should add it with the asserted type.
    if let Expression::Call(Call::Function(fc)) = expr
        && matches!(fc.function, Expression::Identifier(ident) if ident.value() == b"assert")
        && let Some(arg) = fc.argument_list.arguments.first()
    {
        let arg_expr = match arg {
            Argument::Positional(pos) => pos.value,
            Argument::Named(named) => named.value,
        };
        if let Expression::Binary(bin) = arg_expr
            && bin.operator.is_instanceof()
            && let Expression::Variable(Variable::Direct(dv)) = bin.lhs
        {
            let var_name = bytes_to_str(dv.name).to_string();
            if scope.get(&var_name).is_empty() {
                // Variable not in scope — seed it with the asserted type.
                let class_name = match bin.rhs {
                    Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                    Expression::Self_(_) => Some(ctx.current_class.name.to_string()),
                    Expression::Static(_) => Some(ctx.current_class.name.to_string()),
                    Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string()),
                    _ => None,
                };
                if let Some(name) = class_name {
                    let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                        &PhpType::Named(name.clone()),
                        &ctx.current_class.name,
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                    if !resolved.is_empty() {
                        scope.set(
                            &var_name,
                            ResolvedType::from_classes_with_hint(resolved, PhpType::Named(name)),
                        );
                    } else {
                        scope.set(
                            &var_name,
                            vec![ResolvedType::from_type_string(PhpType::Named(name))],
                        );
                    }
                }
            }
        }
    }

    // Apply assert narrowing to each variable in scope.
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
    for var_name in var_names {
        let var_ctx = VarResolutionCtx {
            var_name: &var_name,
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope: ctx.top_level_scope.clone(),
            branch_aware: false,
            match_arm_narrowing: HashMap::new(),
            scope_var_resolver: Some(&scope_resolver),
        };
        let before = scope.get(&var_name).to_vec();
        let mut results = before.clone();

        // assert($x instanceof Foo)
        ResolvedType::apply_narrowing(&mut results, |classes| {
            narrowing::try_apply_assert_instanceof_narrowing(expr, &var_ctx, classes);
        });

        // @phpstan-assert / @psalm-assert
        ResolvedType::apply_narrowing(&mut results, |classes| {
            narrowing::try_apply_custom_assert_narrowing(expr, &var_ctx, classes);
        });

        if resolved_types_differ(&results, &before) {
            if results.is_empty() {
                // Narrowing removed all types (e.g. assert($x instanceof
                // UnresolvableClass)).  Explicitly clear the variable so
                // that diagnostics see "unknown type" and suppress false
                // positives.  `scope.set()` is a no-op for empty vecs.
                scope.locals.insert(var_name, vec![]);
            } else {
                scope.set(&var_name, results);
            }
        }
    }
}

/// Compare two `ResolvedType` slices by their observable identity
/// (type string + class FQN).  `ResolvedType` intentionally does not
/// implement `PartialEq` because `ClassInfo` is a large struct where
/// field-by-field equality is too expensive and semantically wrong.
/// This lightweight comparison detects when narrowing changed the
/// resolved type (e.g. replaced `BaseCatalogFeature` with `self`).
fn resolved_types_differ(a: &[ResolvedType], b: &[ResolvedType]) -> bool {
    if a.len() != b.len() {
        return true;
    }
    for (ra, rb) in a.iter().zip(b.iter()) {
        if ra.type_string != rb.type_string {
            return true;
        }
        match (&ra.class_info, &rb.class_info) {
            (Some(ca), Some(cb)) => {
                if ca.fqn() != cb.fqn() {
                    return true;
                }
            }
            (None, None) => {}
            _ => return true,
        }
    }
    false
}

// ─── Control flow handling ──────────────────────────────────────────────────

/// Process an `if` statement with branch merging.
fn process_if<'b>(
    if_stmt: &'b If<'b>,
    enclosing_stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Record `&&` chain snapshots for the condition expression so that
    // member accesses after an instanceof/null guard within the condition
    // see the narrowed type.  E.g. `if ($x !== null && $x->method())`
    // — the `$x->method()` span needs `$x` narrowed to non-null.
    record_and_chain_snapshots(if_stmt.condition, scope, ctx);

    // Check if the cursor is inside the condition expression.
    // If so, apply inline && narrowing.
    let cond_span = if_stmt.condition.span();
    if ctx.cursor_offset >= cond_span.start.offset && ctx.cursor_offset <= cond_span.end.offset {
        // Cursor is in the condition — scope is already correct.
        return;
    }

    // Assignment in condition: `if ($x = expr())`
    process_condition_assignment(if_stmt.condition, scope, ctx);

    // Pass-by-reference in condition: `if (preg_match(..., $matches))`
    seed_pass_by_ref_in_condition(if_stmt.condition, scope, ctx);

    // Record a snapshot after condition processing so that variables
    // seeded by pass-by-reference (e.g. `$matches` from `preg_match`)
    // are visible in the then-body and elseif/else bodies.  Without
    // this, the pre-statement snapshot (recorded by the outer
    // `walk_body_forward` before `process_if` runs) would be the
    // nearest floor entry, and it predates the seeding.
    if is_diagnostic_scope_active() {
        let body_start = match &if_stmt.body {
            IfBody::Statement(body) => body.statement.span().start.offset,
            IfBody::ColonDelimited(body) => body.colon.start.offset,
        };
        record_scope_snapshot(body_start, scope);
    }

    match &if_stmt.body {
        IfBody::Statement(body) => {
            process_if_statement_body(if_stmt, body, enclosing_stmt, scope, ctx);
        }
        IfBody::ColonDelimited(body) => {
            process_if_colon_body(if_stmt, body, enclosing_stmt, scope, ctx);
        }
    }
}

/// Process if with statement body (brace-style).
fn process_if_statement_body<'b>(
    if_stmt: &'b If<'b>,
    body: &'b IfStatementBody<'b>,
    enclosing_stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let then_span = body.statement.span();
    let cursor_in_then =
        ctx.cursor_offset >= then_span.start.offset && ctx.cursor_offset <= then_span.end.offset;

    // Check if cursor is in any elseif body.
    let cursor_in_elseif = body.else_if_clauses.iter().any(|ei| {
        let sp = ei.statement.span();
        ctx.cursor_offset >= sp.start.offset && ctx.cursor_offset <= sp.end.offset
    });

    // Check if cursor is in else body.
    let cursor_in_else = body.else_clause.as_ref().is_some_and(|ec| {
        let sp = ec.statement.span();
        ctx.cursor_offset >= sp.start.offset && ctx.cursor_offset <= sp.end.offset
    });

    if cursor_in_then {
        // Cursor is inside the then-branch.  Apply instanceof narrowing
        // and walk only this branch.
        apply_condition_narrowing(if_stmt.condition, scope, ctx);
        walk_body_forward(std::iter::once(body.statement), scope, ctx);
        return;
    }

    if cursor_in_elseif {
        // Find which elseif contains the cursor.
        for ei in body.else_if_clauses.iter() {
            let sp = ei.statement.span();
            if ctx.cursor_offset >= sp.start.offset && ctx.cursor_offset <= sp.end.offset {
                // Apply negated narrowing from the if condition, then
                // positive narrowing from this elseif condition.
                apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
                // Also apply inverse narrowing for preceding elseifs.
                for prev_ei in body.else_if_clauses.iter() {
                    if std::ptr::eq(prev_ei, ei) {
                        break;
                    }
                    apply_condition_narrowing_inverse(prev_ei.condition, scope, ctx);
                }
                apply_condition_narrowing(ei.condition, scope, ctx);
                process_condition_assignment(ei.condition, scope, ctx);
                seed_pass_by_ref_in_condition(ei.condition, scope, ctx);
                walk_body_forward(std::iter::once(ei.statement), scope, ctx);
                return;
            }
        }
        return;
    }

    if cursor_in_else && let Some(ref else_clause) = body.else_clause {
        // Apply inverse narrowing from all conditions.
        apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
        for ei in body.else_if_clauses.iter() {
            apply_condition_narrowing_inverse(ei.condition, scope, ctx);
        }
        walk_body_forward(std::iter::once(else_clause.statement), scope, ctx);
        return;
    }

    // Cursor is AFTER the if/else block.  We need to merge all branches.
    let pre_if_scope = scope.clone();

    // Walk each branch independently and merge results.
    let mut then_scope = scope.clone();
    apply_condition_narrowing(if_stmt.condition, &mut then_scope, ctx);
    walk_body_forward(std::iter::once(body.statement), &mut then_scope, ctx);
    let then_exits = statement_unconditionally_exits(body.statement);

    let mut elseif_scopes: Vec<(ScopeState, bool)> = Vec::new();
    for ei in body.else_if_clauses.iter() {
        let mut ei_scope = pre_if_scope.clone();
        apply_condition_narrowing_inverse(if_stmt.condition, &mut ei_scope, ctx);
        for (prev_idx, prev_ei) in body.else_if_clauses.iter().enumerate() {
            if std::ptr::eq(prev_ei, ei) {
                break;
            }
            apply_condition_narrowing_inverse(prev_ei.condition, &mut ei_scope, ctx);
            let _ = prev_idx;
        }
        apply_condition_narrowing(ei.condition, &mut ei_scope, ctx);
        process_condition_assignment(ei.condition, &mut ei_scope, ctx);
        seed_pass_by_ref_in_condition(ei.condition, &mut ei_scope, ctx);
        walk_body_forward(std::iter::once(ei.statement), &mut ei_scope, ctx);
        let exits = statement_unconditionally_exits(ei.statement);
        elseif_scopes.push((ei_scope, exits));
    }

    let (else_scope, else_exits) = if let Some(ref else_clause) = body.else_clause {
        let mut else_scope = pre_if_scope.clone();
        apply_condition_narrowing_inverse(if_stmt.condition, &mut else_scope, ctx);
        for ei in body.else_if_clauses.iter() {
            apply_condition_narrowing_inverse(ei.condition, &mut else_scope, ctx);
        }
        walk_body_forward(std::iter::once(else_clause.statement), &mut else_scope, ctx);
        let exits = statement_unconditionally_exits(else_clause.statement);
        (Some(else_scope), exits)
    } else {
        (None, false)
    };

    // Merge: collect all surviving (non-exiting) branch scopes.
    // Branches that exit via break/continue are loop-local exits —
    // their variable assignments flow to the post-loop scope, so
    // they must be included in the merge alongside truly surviving
    // branches.
    //
    // When there is no else clause, the pre-if scope represents the
    // implicit "condition was false" path.  We apply inverse condition
    // narrowing to it so that information from the condition (e.g.
    // `$a["test"] === null` → `$a["test"]` is NOT null in the else
    // path) is reflected in the merge.
    let mut implicit_else_scope;
    let mut surviving_scopes: Vec<&ScopeState> = Vec::new();

    let then_exits_via_loop = exits_via_loop_control(body.statement);
    if !then_exits || then_exits_via_loop {
        surviving_scopes.push(&then_scope);
    }
    for (idx, (ei_scope, ei_exits)) in elseif_scopes.iter().enumerate() {
        if !ei_exits
            || body
                .else_if_clauses
                .iter()
                .nth(idx)
                .is_some_and(|ei| exits_via_loop_control(ei.statement))
        {
            surviving_scopes.push(ei_scope);
        }
    }
    if let Some(ref es) = else_scope {
        if !else_exits
            || body
                .else_clause
                .as_ref()
                .is_some_and(|ec| exits_via_loop_control(ec.statement))
        {
            surviving_scopes.push(es);
        }
    } else {
        // No else clause — the pre-if scope is an implicit surviving path.
        // When the then-body does NOT exit, apply inverse condition
        // narrowing so that information from the condition (e.g.
        // `$a["test"] === null` → `$a["test"]` is NOT null in the
        // implicit else path) is reflected in the merge.
        //
        // When the then-body DOES exit (guard clause), skip inverse
        // narrowing here — the dedicated guard clause section below
        // handles it.  Applying it in both places would double-narrow.
        implicit_else_scope = pre_if_scope.clone();
        if !then_exits {
            apply_condition_narrowing_inverse(if_stmt.condition, &mut implicit_else_scope, ctx);
        }
        surviving_scopes.push(&implicit_else_scope);
    }

    if surviving_scopes.is_empty() {
        // All branches exit — theoretically unreachable code after.
        // Keep the pre-if scope.
        *scope = pre_if_scope;
    } else if surviving_scopes.len() == 1 {
        *scope = surviving_scopes[0].clone();
    } else {
        // Merge all surviving scopes.
        let mut merged = surviving_scopes[0].clone();
        for s in &surviving_scopes[1..] {
            merged.merge_branch(s);
        }
        // Simplify unions where a child class is merged with its
        // parent — e.g. `ClassResolvesBackChild | ClassResolvesBack`
        // collapses to `ClassResolvesBack`.
        simplify_class_hierarchy_unions(&mut merged, ctx.class_loader);
        *scope = merged;
    }

    // Remove synthetic property access keys that were seeded by
    // condition narrowing inside branches.  These represent narrowed
    // types that only hold within specific branches, not after the
    // if/elseif/else block.  This must run BEFORE guard clause
    // narrowing so that guard-clause-narrowed property keys (e.g.
    // `$this->model` narrowed to `Order` after
    // `if (!$this->model instanceof Order) { return; }`) survive
    // into the post-if scope.
    strip_synthetic_property_keys(scope);

    // Guard clause narrowing: when the if body unconditionally exits
    // and there are no elseif/else branches, apply inverse narrowing.
    // This applies to ALL exit types (return, throw, break, continue)
    // because the code after the if in the current scope does not
    // execute in that path.  Break/continue branch scopes are already
    // included in `surviving_scopes` above so their variable
    // assignments are preserved in the merge.
    if enclosing_stmt.span().end.offset < ctx.cursor_offset
        && then_exits
        && body.else_if_clauses.is_empty()
        && body.else_clause.is_none()
    {
        apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
        apply_guard_clause_null_narrowing(if_stmt, scope, ctx);
    }
}

/// Process if with colon-delimited body.
fn process_if_colon_body<'b>(
    if_stmt: &'b If<'b>,
    body: &'b IfColonDelimitedBody<'b>,
    _enclosing_stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Simplified handling for colon-delimited if.
    // Check if cursor is inside the then-body.
    let then_end = if !body.else_if_clauses.is_empty() {
        body.else_if_clauses
            .first()
            .unwrap()
            .elseif
            .span()
            .start
            .offset
    } else if let Some(ref ec) = body.else_clause {
        ec.r#else.span().start.offset
    } else {
        body.endif.span().start.offset
    };

    let then_start = body.colon.start.offset;
    let cursor_in_then = ctx.cursor_offset >= then_start && ctx.cursor_offset < then_end;

    if cursor_in_then {
        apply_condition_narrowing(if_stmt.condition, scope, ctx);
        walk_body_forward(body.statements.iter(), scope, ctx);
        return;
    }

    // Check elseif clauses.
    for ei in body.else_if_clauses.iter() {
        let ei_start = ei.colon.start.offset;
        let ei_end = ei
            .statements
            .last()
            .map(|s| s.span().end.offset)
            .unwrap_or(ei_start);
        if ctx.cursor_offset >= ei_start && ctx.cursor_offset <= ei_end {
            apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
            apply_condition_narrowing(ei.condition, scope, ctx);
            process_condition_assignment(ei.condition, scope, ctx);
            seed_pass_by_ref_in_condition(ei.condition, scope, ctx);
            walk_body_forward(ei.statements.iter(), scope, ctx);
            return;
        }
    }

    // Check else clause.
    if let Some(ref else_clause) = body.else_clause {
        let ec_start = else_clause.colon.start.offset;
        let ec_end = else_clause
            .statements
            .last()
            .map(|s| s.span().end.offset)
            .unwrap_or(ec_start);
        if ctx.cursor_offset >= ec_start && ctx.cursor_offset <= ec_end {
            apply_condition_narrowing_inverse(if_stmt.condition, scope, ctx);
            for ei in body.else_if_clauses.iter() {
                apply_condition_narrowing_inverse(ei.condition, scope, ctx);
            }
            walk_body_forward(else_clause.statements.iter(), scope, ctx);
            return;
        }
    }

    // Cursor is after the if — merge branches.
    let pre_if_scope = scope.clone();
    let mut then_scope = scope.clone();
    apply_condition_narrowing(if_stmt.condition, &mut then_scope, ctx);
    walk_body_forward(body.statements.iter(), &mut then_scope, ctx);

    let mut all_scopes = vec![then_scope];
    for ei in body.else_if_clauses.iter() {
        let mut ei_scope = pre_if_scope.clone();
        apply_condition_narrowing(ei.condition, &mut ei_scope, ctx);
        process_condition_assignment(ei.condition, &mut ei_scope, ctx);
        seed_pass_by_ref_in_condition(ei.condition, &mut ei_scope, ctx);
        walk_body_forward(ei.statements.iter(), &mut ei_scope, ctx);
        all_scopes.push(ei_scope);
    }
    if let Some(ref else_clause) = body.else_clause {
        let mut else_scope = pre_if_scope.clone();
        apply_condition_narrowing_inverse(if_stmt.condition, &mut else_scope, ctx);
        walk_body_forward(else_clause.statements.iter(), &mut else_scope, ctx);
        all_scopes.push(else_scope);
    } else {
        all_scopes.push(pre_if_scope);
    }

    // Merge all surviving scopes.
    if let Some(first) = all_scopes.first() {
        let mut merged = first.clone();
        for s in &all_scopes[1..] {
            merged.merge_branch(s);
        }
        *scope = merged;
    }
}

/// Compute the assignment dependency depth for a loop body.
///
/// Does a cheap AST walk (no type resolution) to find which variables
/// are assigned and which other variables appear on the RHS.  Then
/// follows the dependency chain to compute the longest path.
///
/// For example, in:
///   $a = $input;
///   $b = transform($a);
///   $c = $b + 1;
///
/// The dependency map is {$a → {$input}, $b → {$a}, $c → {$b}} and
/// the longest chain is 3 ($input → $a → $b → $c).
///
/// This determines how many loop iterations are needed for types to
/// propagate through the entire chain.  Typically 1-3 for real PHP.
fn assignment_map_depth(statements: &[&Statement<'_>]) -> u32 {
    // Build dependency map: assigned_var → set of RHS variables
    let mut deps: HashMap<String, HashSet<String>> = HashMap::new();

    for stmt in statements {
        collect_assignment_deps(stmt, &mut deps);
    }

    if deps.is_empty() {
        return 1;
    }

    // Compute longest dependency chain via DFS with cycle detection.
    let mut cache: HashMap<String, u32> = HashMap::new();
    let mut max_depth: u32 = 1;
    let keys: Vec<String> = deps.keys().cloned().collect();
    for key in &keys {
        let d = chain_depth(key, &deps, &mut cache, &mut HashSet::new());
        max_depth = max_depth.max(d);
    }

    // The chain depth tells us how many levels of variable-to-variable
    // propagation exist.  But even a single assignment needs 2 iterations:
    // one to discover the assignment, one to re-walk with the discovered
    // type visible from the start.  So: iterations = depth + 1.
    // Clamp to a reasonable maximum to avoid pathological cases.
    (max_depth + 1).min(3)
}

/// Recursively compute the dependency chain depth for a variable.
fn chain_depth(
    var: &str,
    deps: &HashMap<String, HashSet<String>>,
    cache: &mut HashMap<String, u32>,
    visiting: &mut HashSet<String>,
) -> u32 {
    if let Some(&cached) = cache.get(var) {
        return cached;
    }
    if !visiting.insert(var.to_string()) {
        // Cycle detected — break it.
        return 1;
    }
    let depth = if let Some(rhs_vars) = deps.get(var) {
        let mut max_child: u32 = 0;
        for dep in rhs_vars {
            max_child = max_child.max(chain_depth(dep, deps, cache, visiting));
        }
        max_child + 1
    } else {
        1
    };
    visiting.remove(var);
    cache.insert(var.to_string(), depth);
    depth
}

/// Collect assignment dependencies from a statement (cheap AST walk).
fn collect_assignment_deps(stmt: &Statement<'_>, deps: &mut HashMap<String, HashSet<String>>) {
    match stmt {
        Statement::Expression(expr_stmt) => {
            collect_expr_assignment_deps(expr_stmt.expression, deps);
        }
        Statement::If(if_stmt) => {
            // Walk all branches via the IfBody enum.
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_assignment_deps(body.statement, deps);
                    for ei in body.else_if_clauses.iter() {
                        collect_assignment_deps(ei.statement, deps);
                    }
                    if let Some(ref else_clause) = body.else_clause {
                        collect_assignment_deps(else_clause.statement, deps);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_assignment_deps(s, deps);
                    }
                    for ei in body.else_if_clauses.iter() {
                        for s in ei.statements.iter() {
                            collect_assignment_deps(s, deps);
                        }
                    }
                    if let Some(ref else_clause) = body.else_clause {
                        for s in else_clause.statements.iter() {
                            collect_assignment_deps(s, deps);
                        }
                    }
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_assignment_deps(s, deps);
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_assignment_deps(s, deps);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_assignment_deps(s, deps);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_assignment_deps(s, deps);
                }
            }
        }
        Statement::Switch(switch) => {
            for case in switch.body.cases().iter() {
                for s in case.statements().iter() {
                    collect_assignment_deps(s, deps);
                }
            }
        }
        // Nested loops: walk their bodies too.
        Statement::Foreach(f) => match &f.body {
            ForeachBody::Statement(s) => {
                collect_assignment_deps(s, deps);
            }
            ForeachBody::ColonDelimited(body) => {
                for s in body.statements.iter() {
                    collect_assignment_deps(s, deps);
                }
            }
        },
        Statement::While(w) => match &w.body {
            WhileBody::Statement(s) => {
                collect_assignment_deps(s, deps);
            }
            WhileBody::ColonDelimited(body) => {
                for s in body.statements.iter() {
                    collect_assignment_deps(s, deps);
                }
            }
        },
        Statement::For(f) => match &f.body {
            ForBody::Statement(s) => {
                collect_assignment_deps(s, deps);
            }
            ForBody::ColonDelimited(body) => {
                for s in body.statements.iter() {
                    collect_assignment_deps(s, deps);
                }
            }
        },
        Statement::DoWhile(dw) => {
            collect_assignment_deps(dw.statement, deps);
        }
        _ => {}
    }
}

/// Extract assignment dependencies from an expression.
fn collect_expr_assignment_deps(
    expr: &Expression<'_>,
    deps: &mut HashMap<String, HashSet<String>>,
) {
    use mago_syntax::ast::variable::Variable;

    if let Expression::Assignment(assign) = expr
        && let Expression::Variable(Variable::Direct(dv)) = assign.lhs
    {
        let lhs_name = bytes_to_str(dv.name).to_string();
        let mut rhs_vars = HashSet::new();
        collect_rhs_variables(assign.rhs, &mut rhs_vars);
        deps.entry(lhs_name).or_default().extend(rhs_vars);
    }
}

/// Collect all variable references from an expression (cheap, no type resolution).
fn collect_rhs_variables(expr: &Expression<'_>, vars: &mut HashSet<String>) {
    use mago_syntax::ast::variable::Variable;

    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            vars.insert(bytes_to_str(dv.name).to_string());
        }
        Expression::Binary(binary) => {
            collect_rhs_variables(binary.lhs, vars);
            collect_rhs_variables(binary.rhs, vars);
        }
        Expression::UnaryPrefix(unary) => {
            collect_rhs_variables(unary.operand, vars);
        }
        Expression::UnaryPostfix(unary) => {
            collect_rhs_variables(unary.operand, vars);
        }
        Expression::Parenthesized(p) => {
            collect_rhs_variables(p.expression, vars);
        }
        Expression::Call(call) => {
            // Collect variables from call arguments.
            match call {
                Call::Function(fc) => {
                    collect_rhs_variables(fc.function, vars);
                    collect_arglist_variables(&fc.argument_list, vars);
                }
                Call::Method(mc) => {
                    collect_rhs_variables(mc.object, vars);
                    collect_arglist_variables(&mc.argument_list, vars);
                }
                Call::NullSafeMethod(mc) => {
                    collect_rhs_variables(mc.object, vars);
                    collect_arglist_variables(&mc.argument_list, vars);
                }
                Call::StaticMethod(sc) => {
                    collect_rhs_variables(sc.class, vars);
                    collect_arglist_variables(&sc.argument_list, vars);
                }
            }
        }
        Expression::Access(access) => match access {
            mago_syntax::ast::access::Access::Property(pa) => {
                collect_rhs_variables(pa.object, vars);
            }
            mago_syntax::ast::access::Access::NullSafeProperty(pa) => {
                collect_rhs_variables(pa.object, vars);
            }
            mago_syntax::ast::access::Access::StaticProperty(sp) => {
                collect_rhs_variables(sp.class, vars);
            }
            mago_syntax::ast::access::Access::ClassConstant(cc) => {
                collect_rhs_variables(cc.class, vars);
            }
        },
        Expression::ArrayAccess(aa) => {
            collect_rhs_variables(aa.array, vars);
        }
        Expression::Conditional(cond) => {
            collect_rhs_variables(cond.condition, vars);
            if let Some(then_expr) = cond.then {
                collect_rhs_variables(then_expr, vars);
            }
            collect_rhs_variables(cond.r#else, vars);
        }

        Expression::Instantiation(inst) => {
            collect_rhs_variables(inst.class, vars);
            if let Some(ref args) = inst.argument_list {
                collect_arglist_variables(args, vars);
            }
        }
        Expression::Assignment(assign) => {
            // Nested assignments like `$a = $b = expr`.
            collect_rhs_variables(assign.rhs, vars);
        }
        _ => {}
    }
}

/// Collect variable references from an argument list.
fn collect_arglist_variables(
    args: &mago_syntax::ast::argument::ArgumentList<'_>,
    vars: &mut HashSet<String>,
) {
    for arg in args.arguments.iter() {
        let expr = match arg {
            Argument::Positional(a) => a.value,
            Argument::Named(a) => a.value,
        };
        collect_rhs_variables(expr, vars);
    }
}

/// Check whether the post-walk scope has any NEW or CHANGED variable
/// types compared to the pre-loop scope.  This is the Mago-style
/// fixed-point check that runs BEFORE a re-walk: if nothing changed,
/// there's no point walking the body again.
///
/// Unlike `scopes_equal`, this is asymmetric: new variables in
/// `after` that weren't in `before` count as changes, but variables
/// in `before` that aren't in `after` do not (they were just not
/// assigned in the loop body).
fn scope_has_changes(before: &ScopeState, after: &ScopeState) -> bool {
    for (name, after_types) in &after.locals {
        match before.locals.get(name) {
            None => {
                // New variable assigned in the loop body.
                if !after_types.is_empty() {
                    return true;
                }
            }
            Some(before_types) => {
                if after_types.len() != before_types.len() {
                    return true;
                }
                for (at, bt) in after_types.iter().zip(before_types.iter()) {
                    if at.type_string != bt.type_string {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Process a `foreach` statement.
fn process_foreach<'b>(foreach: &'b Foreach<'b>, scope: &mut ScopeState, ctx: &ForwardWalkCtx<'_>) {
    let loop_depth = enter_loop();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        leave_loop(loop_depth);
        return;
    }

    // Apply any standalone `/** @var Type $var */` docblocks that precede
    // the foreach keyword.  These are not separate AST statements (the
    // parser attaches them as comments to the foreach), so they won't be
    // processed by `process_expression_statement`.  Without this, variables
    // typed only via docblock (common in Blade templates) won't be in scope
    // when the iterable expression is resolved.
    //
    // We extract all variables referenced in the foreach expression and
    // check for @var annotations for each one.
    let foreach_offset = foreach.foreach.span().start.offset as usize;
    if let Expression::Variable(Variable::Direct(dv)) = foreach.expression {
        let var_name = format!("${}", bytes_to_str(dv.name));
        if scope.get(bytes_to_str(dv.name)).is_empty()
            && let Some(var_type) =
                crate::docblock::find_var_raw_type_in_source(ctx.content, foreach_offset, &var_name)
        {
            let resolved = resolve_type_to_resolved_types(
                &crate::util::resolve_php_type_names(&var_type, ctx.class_loader),
                ctx,
            );
            scope.set(bytes_to_str(dv.name), resolved);
        }
    } else {
        // For complex expressions like `$users->active()->byName()`,
        // extract the base variable and resolve its type from @var.
        let expr_start = foreach.expression.span().start.offset as usize;
        let expr_end = foreach.expression.span().end.offset as usize;
        if let Some(expr_text) = ctx.content.get(expr_start..expr_end) {
            // Extract the base variable (e.g. "$users" from "$users->active()->byName()")
            if let Some(base_end) = expr_text.find("->").or_else(|| expr_text.find("::")) {
                let base_var = expr_text[..base_end].trim();
                if let Some(scope_key) = base_var.strip_prefix('$')
                    && scope.get(scope_key).is_empty()
                    && let Some(var_type) = crate::docblock::find_var_raw_type_in_source(
                        ctx.content,
                        foreach_offset,
                        base_var,
                    )
                {
                    let resolved = resolve_type_to_resolved_types(
                        &crate::util::resolve_php_type_names(&var_type, ctx.class_loader),
                        ctx,
                    );
                    scope.set(scope_key, resolved);
                }
            }
        }
    }

    // Resolve the iterable expression's type.
    let iter_type = resolve_foreach_iterable_type(foreach, scope, ctx);

    let pre_loop_scope = scope.clone();

    // When the cursor is inside the loop body (completion path), discovery
    // passes must walk the ENTIRE body; the final pass uses the real
    // cursor_offset so it stops at the cursor as usual.
    let body_span = match &foreach.body {
        ForeachBody::Statement(inner) => inner.span(),
        ForeachBody::ColonDelimited(body) => body.span(),
    };
    let cursor_in_body =
        ctx.cursor_offset >= body_span.start.offset && ctx.cursor_offset <= body_span.end.offset;
    let discovery_ctx = if cursor_in_body && !is_diagnostic_scope_active() {
        ctx.with_cursor_offset(u32::MAX)
    } else {
        ctx.with_cursor_offset(ctx.cursor_offset)
    };

    // Bind the value variable (and optionally the key variable).
    match &foreach.target {
        ForeachTarget::Value(val) => {
            bind_foreach_value(val.value, &iter_type, scope, ctx);
        }
        ForeachTarget::KeyValue(kv) => {
            bind_foreach_key(kv.key, &iter_type, scope, ctx);
            bind_foreach_value(kv.value, &iter_type, scope, ctx);
        }
    }

    // Docblock fallback: when `bind_foreach_value`/`bind_foreach_key`
    // could not determine the element type from the iterable (e.g. the
    // iterable is `mixed` or a bare `array`), check for inline
    // `/** @var Type $var */` docblock(s) preceding the foreach keyword
    // and use them to seed the key and/or value variables.  @var
    // annotations are explicit developer overrides that take priority
    // over types inferred from the iterable.
    let value_var_name = match &foreach.target {
        ForeachTarget::Value(val) => extract_foreach_var_name(val.value),
        ForeachTarget::KeyValue(kv) => extract_foreach_var_name(kv.value),
    };
    let key_var_name = match &foreach.target {
        ForeachTarget::Value(_) => None,
        ForeachTarget::KeyValue(kv) => extract_foreach_var_name(kv.key),
    };

    // Collect resolved docblock overrides for key/value variables.
    let mut value_docblock_override: Option<Vec<ResolvedType>> = None;
    let mut key_docblock_override: Option<Vec<ResolvedType>> = None;
    let foreach_offset = foreach.foreach.span().start.offset as usize;
    let before = &ctx.content[..foreach_offset.min(ctx.content.len())];
    let trimmed = before.trim_end();
    if trimmed.ends_with("*/")
        && let Some(doc_start) = trimmed.rfind("/**")
    {
        let doc_text = &trimmed[doc_start..trimmed.len()];
        let var_annotations = parse_all_var_docblock_annotations(doc_text);
        for (doc_var, php_type) in &var_annotations {
            if let Some(ref vn) = value_var_name
                && doc_var == vn
            {
                value_docblock_override = Some(resolve_type_to_resolved_types(php_type, ctx));
            }
            if let Some(ref kn) = key_var_name
                && doc_var == kn
            {
                key_docblock_override = Some(resolve_type_to_resolved_types(php_type, ctx));
            }
        }
    }

    // Apply docblock overrides (overwrites bind_foreach_key/value results).
    if let Some(ref resolved) = value_docblock_override
        && let Some(ref vn) = value_var_name
    {
        scope.set(vn, resolved.clone());
    }
    if let Some(ref resolved) = key_docblock_override
        && let Some(ref kn) = key_var_name
    {
        scope.set(kn, resolved.clone());
    }
    // When the iterable is a bare `array` (no generic parameters)
    // and no @var docblock provided a concrete type, the element
    // type is `mixed`.  Seed it so that assignments from the loop
    // variable propagate `mixed` correctly through the body.
    if let Some(ref vn) = value_var_name
        && value_docblock_override.is_none()
        && scope.get(vn).is_empty()
        && iter_type.as_ref().is_some_and(|it| it.is_bare_array())
    {
        scope.set(vn, vec![ResolvedType::from_type_string(PhpType::mixed())]);
    }

    // ── Assignment-depth-bounded loop iteration ─────────────────
    //
    // Walk the body once (always needed).  Then check whether any
    // variable types changed compared to the pre-loop scope.  Only
    // re-walk if there are actual changes AND the assignment depth
    // requires further propagation.  This matches Mago's approach:
    // the fixed-point check happens BEFORE the expensive re-walk,
    // not after.
    let body_stmts: Vec<&Statement<'b>> = match &foreach.body {
        ForeachBody::Statement(inner) => vec![*inner],
        ForeachBody::ColonDelimited(body) => body.statements.iter().collect(),
    };
    let assignment_depth =
        clamp_iterations_for_depth(assignment_map_depth(&body_stmts), loop_depth);

    // ── Initial walk (always performed) ─────────────────────────
    let initial_ctx = if assignment_depth > 1 {
        &discovery_ctx
    } else {
        ctx
    };
    match &foreach.body {
        ForeachBody::Statement(inner) => {
            walk_body_forward(std::iter::once(*inner), scope, initial_ctx);
        }
        ForeachBody::ColonDelimited(body) => {
            walk_body_forward(body.statements.iter(), scope, initial_ctx);
        }
    }

    // ── Re-walk iterations (only if types changed) ──────────────
    for iteration in 0..assignment_depth.saturating_sub(1) {
        // Check for changes BEFORE re-walking: compare post-walk
        // scope against the pre-loop scope.  If no variable has a
        // type that differs from what was known before the loop,
        // there's nothing new to propagate — skip the re-walk.
        if !scope_has_changes(&pre_loop_scope, scope) {
            break;
        }

        // Merge discovered types back into the pre-loop scope and
        // re-bind foreach variables for the next iteration.
        let mut next_scope = pre_loop_scope.clone();
        next_scope.merge_branch(scope);
        match &foreach.target {
            ForeachTarget::Value(val) => {
                bind_foreach_value(val.value, &iter_type, &mut next_scope, ctx);
            }
            ForeachTarget::KeyValue(kv) => {
                bind_foreach_key(kv.key, &iter_type, &mut next_scope, ctx);
                bind_foreach_value(kv.value, &iter_type, &mut next_scope, ctx);
            }
        }
        // Re-apply docblock overrides after re-binding.
        if let Some(ref resolved) = value_docblock_override
            && let Some(ref vn) = value_var_name
        {
            next_scope.set(vn, resolved.clone());
        }
        if let Some(ref resolved) = key_docblock_override
            && let Some(ref kn) = key_var_name
        {
            next_scope.set(kn, resolved.clone());
        }
        *scope = next_scope;

        // Use the real context on the final iteration so diagnostic
        // snapshots and cursor handling are correct.
        let is_final = iteration + 1 >= assignment_depth.saturating_sub(1);
        let walk_ctx = if is_final { ctx } else { &discovery_ctx };

        match &foreach.body {
            ForeachBody::Statement(inner) => {
                walk_body_forward(std::iter::once(*inner), scope, walk_ctx);
            }
            ForeachBody::ColonDelimited(body) => {
                walk_body_forward(body.statements.iter(), scope, walk_ctx);
            }
        }
    }

    // The iterable might be empty, so the loop body might not execute
    // at all.  Merge with the pre-loop scope.
    let post_loop = scope.clone();
    *scope = pre_loop_scope;
    scope.merge_branch(&post_loop);

    // When the iterable is a non-empty literal array (e.g. `["a", "b",
    // "c"]`), the loop body is guaranteed to execute at least once.
    // The pre-loop sentinel value (e.g. `null` from `$tag = null`) must
    // not survive as a possible post-loop type for the foreach target
    // variable — override it with the post-loop value from the body walk.
    if is_non_empty_array_literal(foreach.expression) {
        let target_var = match &foreach.target {
            ForeachTarget::Value(val) => extract_foreach_var_name(val.value),
            ForeachTarget::KeyValue(kv) => extract_foreach_var_name(kv.value),
        };
        if let Some(ref vn) = target_var
            && let Some(post_val) = post_loop.locals.get(&ustr::ustr(vn.as_str()))
            && !post_val.is_empty()
        {
            scope.set(vn, post_val.clone());
        }
    }

    leave_loop(loop_depth);
}

/// Resolve the iterable expression's type for a foreach.
fn resolve_foreach_iterable_type<'b>(
    foreach: &'b Foreach<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    // Try direct scope lookup for bare variable iterators.
    if let Expression::Variable(Variable::Direct(dv)) = foreach.expression {
        let var_name = bytes_to_str(dv.name).to_string();
        let from_scope = scope.get(&var_name);
        if !from_scope.is_empty() {
            return Some(ResolvedType::types_joined(from_scope));
        }
    }

    // Fall back to resolve_rhs_expression for complex expressions.
    let resolved = resolve_rhs_with_scope(foreach.expression, scope, ctx);
    if !resolved.is_empty() {
        let joined = ResolvedType::types_joined(&resolved);
        // Expand type aliases (e.g. `@phpstan-type UserList array<int, User>`)
        // so that `extract_value_type` can see the underlying generic type.
        let expanded = crate::completion::type_resolution::resolve_type_alias_typed(
            &joined,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        )
        .unwrap_or(joined);
        return Some(expanded);
    }

    // Fallback: for simple `$variable` iterators, check for an inline
    // `/** @var Type $var */` or `@param` annotation near the foreach.
    // This mirrors the backward scanner's `find_iterable_raw_type_in_source`
    // fallback and handles cases where the variable's type comes from a
    // docblock rather than an assignment.
    if let Expression::Variable(Variable::Direct(dv)) = foreach.expression {
        let var_name = bytes_to_str(dv.name).to_string();
        let foreach_offset = foreach.foreach.span().start.offset as usize;
        if let Some(docblock_type) = crate::docblock::find_iterable_raw_type_in_source(
            ctx.content,
            foreach_offset,
            &var_name,
        )
        .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader))
        {
            // Expand type aliases on the docblock result too.
            let expanded = crate::completion::type_resolution::resolve_type_alias_typed(
                &docblock_type,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )
            .unwrap_or(docblock_type);
            return Some(expanded);
        }
    }

    // Final fallback: resolve the foreach expression as a "subject"
    // through the full resolver pipeline (SubjectExpr::parse →
    // property/method chain resolution).  This mirrors the backward
    // scanner's `resolve_foreach_expression_to_classes` and handles
    // cases like `$this->getItems()` or `self::fetchAll()` where
    // the expression type wasn't captured by scope lookup or
    // resolve_rhs_expression above.
    if let Some(iter_type) = resolve_foreach_expr_via_subject(foreach.expression, scope, ctx) {
        return Some(iter_type);
    }

    None
}

/// Resolve a foreach expression to a `PhpType` by treating it as a
/// subject string and going through the full resolver pipeline.
///
/// This is the forward walker's equivalent of the backward scanner's
/// `resolve_foreach_expression_to_classes`.  It extracts the expression
/// text, calls `resolve_target_classes` to get `ClassInfo` objects, and
/// constructs a `PhpType::Named` from the first resolved class.
fn resolve_foreach_expr_via_subject<'b>(
    expression: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    let expr_span = expression.span();
    let expr_start = expr_span.start.offset as usize;
    let expr_end = expr_span.end.offset as usize;
    let expr_text = ctx.content.get(expr_start..expr_end)?.trim();
    if expr_text.is_empty() {
        return None;
    }

    // Build a ResolutionCtx from the forward walker's context.
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = move |var_name: &str| -> Vec<ResolvedType> {
        scope_snapshot
            .get(&atom(var_name))
            .cloned()
            .unwrap_or_default()
    };
    let var_ctx = ctx.var_ctx_for_with_scope("$__foreach", expr_span.start.offset, &scope_resolver);
    let rctx = var_ctx.as_resolution_ctx();

    let resolved = crate::completion::resolver::resolve_target_classes(
        expr_text,
        crate::types::AccessKind::Arrow,
        &rctx,
    );

    if resolved.is_empty() {
        return None;
    }

    // Construct a PhpType from the resolved classes.  If any resolved
    // type has a structured type_string (e.g. `list<User>`,
    // `Collection<int, Product>`), prefer that — it carries generic
    // parameters that `extract_value_type` can use.
    for rt in &resolved {
        if rt.type_string.has_type_structure() {
            let expanded = crate::completion::type_resolution::resolve_type_alias_typed(
                &rt.type_string,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            )
            .unwrap_or_else(|| rt.type_string.clone());
            return Some(expanded);
        }
    }

    // Fall back to the class name — `bind_foreach_value` Strategy 2
    // will resolve it through inheritance to find element types.
    // Use `fqn()` (not `name`) so that the returned `PhpType::Named`
    // carries the fully-qualified class name.  `ClassInfo.name` is
    // always the short name (e.g. `OrderProductCollection`), while
    // `fqn()` combines namespace + name into the FQN that the class
    // loader needs to find and merge the class.
    let first = resolved.first()?;
    let name = first
        .class_info
        .as_ref()
        .map(|c| c.fqn().to_string())
        .or_else(|| first.type_string.base_name().map(|s| s.to_string()))?;

    Some(PhpType::Named(name))
}

/// Bind a foreach value variable from the iterable's element type.
///
/// Resolution strategy:
/// 1. Try `PhpType::extract_value_type` — works for types that already
///    carry generic parameters (e.g. `list<User>`, `array<int, Order>`,
///    `Collection<int, Product>`).
/// 2. Class-based fallback — when the type is a bare class name (e.g.
///    `OrderProductCollection`), resolve it to `ClassInfo`, merge
///    inheritance, and extract the element type from `@extends` /
///    `@implements` generics.  This mirrors what
///    `try_resolve_foreach_value_type` does in the backward scanner.
fn bind_foreach_value<'b>(
    value_expr: &'b Expression<'b>,
    iter_type: &Option<PhpType>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Unwrap `&$value` (by-reference foreach) to get the inner variable.
    let value_expr = if let Expression::UnaryPrefix(up) = value_expr
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        up.operand
    } else {
        value_expr
    };
    if let Expression::Variable(Variable::Direct(dv)) = value_expr {
        let var_name = bytes_to_str(dv.name).to_string();
        if let Some(it) = iter_type {
            // Strategy 1: extract from the type's own generic parameters.
            let value_php_type = it.extract_value_type(false);
            if let Some(vt) = value_php_type {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    vt,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );
                if !resolved.is_empty() {
                    scope.set(
                        &var_name,
                        ResolvedType::from_classes_with_hint(resolved, vt.clone()),
                    );
                } else {
                    scope.set(&var_name, vec![ResolvedType::from_type_string(vt.clone())]);
                }
                return;
            }

            // Strategy 2: class-based fallback for bare collection names.
            let element_via_class = resolve_iterable_element_via_class(it, ctx);
            if let Some(element_type) = element_via_class
                && !is_unsubstituted_template_param(&element_type)
            {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    &element_type,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );
                if !resolved.is_empty() {
                    scope.set(
                        &var_name,
                        ResolvedType::from_classes_with_hint(resolved, element_type),
                    );
                } else {
                    scope.set(
                        &var_name,
                        vec![ResolvedType::from_type_string(element_type)],
                    );
                }
            }

            // Strategy 3: union type fallback — try each member individually.
            // When the iterable is a union like `ProductCollection|Product`,
            // neither `extract_value_type` nor `resolve_iterable_element_via_class`
            // works on the union as a whole.  Walk each member and use the
            // first one that yields an element type.
            if let PhpType::Union(members) = it {
                for member in members {
                    // Try extract_value_type on each member (handles generic collections).
                    if let Some(vt) = member.extract_value_type(false) {
                        let resolved =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                vt,
                                &ctx.current_class.name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if !resolved.is_empty() {
                            scope.set(
                                &var_name,
                                ResolvedType::from_classes_with_hint(resolved, vt.clone()),
                            );
                        } else {
                            scope.set(&var_name, vec![ResolvedType::from_type_string(vt.clone())]);
                        }
                        return;
                    }
                    // Try class-based element extraction on each member.
                    if let Some(element_type) = resolve_iterable_element_via_class(member, ctx)
                        && !is_unsubstituted_template_param(&element_type)
                    {
                        let resolved =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                &element_type,
                                &ctx.current_class.name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if !resolved.is_empty() {
                            scope.set(
                                &var_name,
                                ResolvedType::from_classes_with_hint(resolved, element_type),
                            );
                        } else {
                            scope.set(
                                &var_name,
                                vec![ResolvedType::from_type_string(element_type)],
                            );
                        }
                        return;
                    }
                }
            }
        }
        // Couldn't determine the element type.  Store empty in the
        // scope so that `lookup_diagnostic_scope` returns
        // `Some(vec![])` instead of `None`, preventing a pointless
        // fallthrough to the backward scanner.
        if !scope.contains(&var_name) {
            scope.set_empty(&var_name);
        }
    } else if let Expression::Array(_) | Expression::List(_) = value_expr {
        // Array/list destructuring in foreach: `foreach ($items as [$a, $b])`
        // Extract the element type from the iterable, then resolve each
        // destructured variable's type from that element type using shape
        // keys or positional indices.
        let element_type: Option<PhpType> = iter_type.as_ref().and_then(|it| {
            // Try direct value type extraction first.
            if let Some(vt) = it.extract_value_type(false) {
                return Some(vt.clone());
            }
            // Try class-based iterable element extraction.
            if let Some(et) = resolve_iterable_element_via_class(it, ctx)
                && !is_unsubstituted_template_param(&et)
            {
                return Some(et);
            }
            // Try union members individually.
            if let PhpType::Union(members) = it {
                for member in members {
                    if let Some(vt) = member.extract_value_type(false) {
                        return Some(vt.clone());
                    }
                    if let Some(et) = resolve_iterable_element_via_class(member, ctx)
                        && !is_unsubstituted_template_param(&et)
                    {
                        return Some(et);
                    }
                }
            }
            None
        });

        if let Some(ref elem_type) = element_type {
            let elements_iter: Vec<&ArrayElement<'_>> = match value_expr {
                Expression::Array(arr) => arr.elements.iter().collect(),
                Expression::List(list) => list.elements.iter().collect(),
                _ => vec![],
            };

            let mut positional_index: usize = 0;
            for elem in elements_iter {
                let (var_name, shape_key) = match elem {
                    ArrayElement::KeyValue(kv) => {
                        if let Expression::Variable(Variable::Direct(dv)) = kv.value {
                            (
                                bytes_to_str(dv.name).to_string(),
                                extract_foreach_destr_key(kv.key),
                            )
                        } else {
                            continue;
                        }
                    }
                    ArrayElement::Value(val) => {
                        let key = Some(positional_index.to_string());
                        positional_index += 1;
                        if let Expression::Variable(Variable::Direct(dv)) = val.value {
                            (bytes_to_str(dv.name).to_string(), key)
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };

                // Try shape key lookup first, then fall back to generic element type.
                let resolved_type = shape_key
                    .as_ref()
                    .and_then(|k| elem_type.shape_value_type(k).cloned())
                    .or_else(|| elem_type.extract_value_type(true).cloned());

                if let Some(ref vt) = resolved_type {
                    let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                        vt,
                        &ctx.current_class.name,
                        ctx.all_classes,
                        ctx.class_loader,
                    );
                    if !resolved.is_empty() {
                        scope.set(
                            &var_name,
                            ResolvedType::from_classes_with_hint(resolved, vt.clone()),
                        );
                    } else {
                        scope.set(&var_name, vec![ResolvedType::from_type_string(vt.clone())]);
                    }
                }
            }
        }
    }
}

/// Returns `true` when `expr` is a non-empty array literal such as
/// `["a", "b", "c"]` or `array(1, 2, 3)`.
///
/// Used by `process_foreach` to detect iterables that are guaranteed to
/// have at least one element, so that the pre-loop type of the target
/// variable does not survive into the post-loop scope.
fn is_non_empty_array_literal(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Array(arr) => !arr.elements.is_empty(),
        Expression::LegacyArray(arr) => !arr.elements.is_empty(),
        _ => false,
    }
}

/// Extract the variable name from a foreach value expression, unwrapping
/// a leading `&` (by-reference) if present.
fn extract_foreach_var_name(expr: &Expression<'_>) -> Option<String> {
    let inner = if let Expression::UnaryPrefix(up) = expr
        && matches!(up.operator, UnaryPrefixOperator::Reference(_))
    {
        up.operand
    } else {
        expr
    };
    if let Expression::Variable(Variable::Direct(dv)) = inner {
        Some(bytes_to_str(dv.name).to_string())
    } else {
        None
    }
}

/// Extract a string key from a foreach destructuring key expression.
///
/// Handles string literals (`'user'`, `"user"`) and integer literals.
fn extract_foreach_destr_key(key_expr: &Expression<'_>) -> Option<String> {
    match key_expr {
        Expression::Literal(Literal::String(lit_str)) => lit_str
            .value
            .map(|v| bytes_to_str(v).to_string())
            .or_else(|| {
                let raw = bytes_to_str(lit_str.raw).to_string();
                Some(raw.trim_matches('\'').trim_matches('"').to_string())
            }),
        Expression::Literal(Literal::Integer(lit_int)) => {
            Some(bytes_to_str(lit_int.raw).to_string())
        }
        _ => None,
    }
}

/// Check whether a `PhpType` looks like an unsubstituted template
/// parameter (e.g. `TValue`, `TKey`, `TModel`).  These are bare named
/// types whose name starts with `T` followed by an uppercase letter
/// and are not known PHP built-in types.
fn is_unsubstituted_template_param(ty: &PhpType) -> bool {
    let name = match ty {
        PhpType::Named(n) => n.as_str(),
        _ => return false,
    };
    let bytes = name.as_bytes();
    bytes.len() >= 2 && bytes[0] == b'T' && bytes[1].is_ascii_uppercase()
}

/// Resolve the element type of an iterable via class inheritance.
///
/// When the iterable type is a bare class name (e.g. `OrderProductCollection`),
/// this resolves it to `ClassInfo`, merges the full inheritance chain, and
/// extracts the element type from `@extends` / `@implements` generics using
/// [`extract_iterable_element_type_from_class`].
fn resolve_iterable_element_via_class(
    iter_type: &PhpType,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    // Only attempt for Named types (bare class names without generics).
    // Generic types like `Collection<int, User>` are handled by
    // extract_value_type above.
    let class_name = match iter_type {
        PhpType::Named(name) => name.as_str(),
        _ => return None,
    };

    // Resolve the class name to ClassInfo.
    let classes = crate::completion::type_resolution::type_hint_to_classes_typed(
        iter_type,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );

    if classes.is_empty() {
        // Try direct class loader as fallback (handles FQN names).
        let cls = (ctx.class_loader)(class_name)?;
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        return super::foreach_resolution::extract_iterable_element_type_from_class(
            &merged,
            ctx.class_loader,
        );
    }

    for cls in &classes {
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        let element_type = super::foreach_resolution::extract_iterable_element_type_from_class(
            &merged,
            ctx.class_loader,
        );
        if let Some(ref et) = element_type {
            // When the extracted type is an unsubstituted template parameter
            // (e.g. `TModel`), resolve it through the class's template bounds
            // (e.g. `@template TModel of BlogAuthor` → `BlogAuthor`).
            if let Some(name) = et.base_name()
                && merged
                    .template_params
                    .iter()
                    .any(|p| p.as_ref() as &str == name)
                && let Some(bound) = merged.template_param_bounds.get(&crate::atom::atom(name))
            {
                return Some(bound.clone());
            }
            return element_type;
        }
    }

    None
}

/// Bind a foreach key variable.
fn bind_foreach_key<'b>(
    key_expr: &'b Expression<'b>,
    iter_type: &Option<PhpType>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if let Expression::Variable(Variable::Direct(dv)) = key_expr {
        let var_name = bytes_to_str(dv.name).to_string();
        if let Some(it) = iter_type {
            let key_php_type = it.extract_key_type(false);
            if let Some(kt) = key_php_type {
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    kt,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );
                if !resolved.is_empty() {
                    scope.set(
                        &var_name,
                        ResolvedType::from_classes_with_hint(resolved, kt.clone()),
                    );
                } else {
                    scope.set(&var_name, vec![ResolvedType::from_type_string(kt.clone())]);
                }
                return;
            }
        }
        // Default: key is int|string.
        scope.set(
            &var_name,
            vec![ResolvedType::from_type_string(PhpType::Union(vec![
                PhpType::int(),
                PhpType::string(),
            ]))],
        );
    }
}

/// Process a `while` loop.
///
/// Uses the same two-pass strategy as `process_foreach` and
/// `process_for`: the first pass discovers all variable assignments
/// inside the loop body, the results are merged back into the
/// pre-loop scope, and the final pass re-walks with full visibility
/// of loop-carried assignments.
fn process_while<'b>(while_stmt: &'b While<'b>, scope: &mut ScopeState, ctx: &ForwardWalkCtx<'_>) {
    let loop_depth = enter_loop();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        leave_loop(loop_depth);
        return;
    }

    // Record `&&` chain snapshots for the while condition.
    record_and_chain_snapshots(while_stmt.condition, scope, ctx);

    let pre_loop_scope = scope.clone();

    // The while body executes when the condition is truthy, so apply
    // condition narrowing (instanceof, phpstan-assert-if-true, etc.).
    // This must happen AFTER saving pre_loop_scope so the narrowing
    // only affects the loop body, not the post-loop scope.
    apply_condition_narrowing(while_stmt.condition, scope, ctx);

    // When the cursor is inside the loop body (completion path), discovery
    // passes must walk the ENTIRE body; the final pass uses the real
    // cursor_offset so it stops at the cursor as usual.
    let body_span = match &while_stmt.body {
        WhileBody::Statement(inner) => inner.span(),
        WhileBody::ColonDelimited(body) => body.span(),
    };
    let cursor_in_body =
        ctx.cursor_offset >= body_span.start.offset && ctx.cursor_offset <= body_span.end.offset;
    let discovery_ctx = if cursor_in_body && !is_diagnostic_scope_active() {
        ctx.with_cursor_offset(u32::MAX)
    } else {
        ctx.with_cursor_offset(ctx.cursor_offset)
    };

    // Assignment in condition: `while ($x = expr())`
    process_condition_assignment(while_stmt.condition, scope, ctx);

    // Pass-by-reference in condition: `while (preg_match(..., $matches))`
    seed_pass_by_ref_in_condition(while_stmt.condition, scope, ctx);

    // Record a snapshot after condition processing (same reasoning as
    // the corresponding snapshot in `process_if`).
    if is_diagnostic_scope_active() {
        let body_start = match &while_stmt.body {
            WhileBody::Statement(inner) => inner.span().start.offset,
            WhileBody::ColonDelimited(body) => body.colon.start.offset,
        };
        record_scope_snapshot(body_start, scope);
    }

    // ── Assignment-depth-bounded loop iteration ─────────────────
    let body_stmts: Vec<&Statement<'b>> = match &while_stmt.body {
        WhileBody::Statement(inner) => vec![*inner],
        WhileBody::ColonDelimited(body) => body.statements.iter().collect(),
    };
    let assignment_depth =
        clamp_iterations_for_depth(assignment_map_depth(&body_stmts), loop_depth);

    // ── Initial walk (always performed) ─────────────────────────
    let initial_ctx = if assignment_depth > 1 {
        &discovery_ctx
    } else {
        ctx
    };
    match &while_stmt.body {
        WhileBody::Statement(inner) => {
            walk_body_forward(std::iter::once(*inner), scope, initial_ctx);
        }
        WhileBody::ColonDelimited(body) => {
            walk_body_forward(body.statements.iter(), scope, initial_ctx);
        }
    }

    // ── Re-walk iterations (only if types changed) ──────────────
    for iteration in 0..assignment_depth.saturating_sub(1) {
        if !scope_has_changes(&pre_loop_scope, scope) {
            break;
        }

        let mut next_scope = pre_loop_scope.clone();
        next_scope.merge_branch(scope);
        apply_condition_narrowing(while_stmt.condition, &mut next_scope, ctx);
        process_condition_assignment(while_stmt.condition, &mut next_scope, ctx);
        seed_pass_by_ref_in_condition(while_stmt.condition, &mut next_scope, ctx);
        *scope = next_scope;

        let is_final = iteration + 1 >= assignment_depth.saturating_sub(1);
        let walk_ctx = if is_final { ctx } else { &discovery_ctx };

        match &while_stmt.body {
            WhileBody::Statement(inner) => {
                walk_body_forward(std::iter::once(*inner), scope, walk_ctx);
            }
            WhileBody::ColonDelimited(body) => {
                walk_body_forward(body.statements.iter(), scope, walk_ctx);
            }
        }
    }

    // When the cursor is inside the loop body (completion path), keep
    // the scope with condition narrowing applied.  The post-loop
    // merge would erase the narrowing (since the loop might not execute),
    // but the cursor IS inside the body, so the condition is true.
    if cursor_in_body && !is_diagnostic_scope_active() {
        return;
    }

    // The loop body might not execute at all (condition false on
    // first check), so merge with the pre-loop scope.
    let post_loop = scope.clone();
    *scope = pre_loop_scope;
    scope.merge_branch(&post_loop);

    // After the loop, the condition evaluated to false (that's why the
    // loop exited).  Apply the inverse of the condition to narrow types.
    // For example: `while ($a) { $a = $a->parent; }` => after loop, $a is null.
    apply_condition_narrowing_inverse(while_stmt.condition, scope, ctx);

    // Remove synthetic property access keys that were seeded by
    // condition narrowing.  These represent narrowed types that only
    // hold inside the loop body (where the condition is true).
    // After the loop, the condition may be false, so the narrowing
    // no longer applies.
    strip_synthetic_property_keys(scope);

    leave_loop(loop_depth);
}

/// Process a `for` loop.
///
/// Uses the same assignment-depth-bounded iteration as `process_foreach`:
/// a cheap AST walk determines the dependency chain depth, then the body
/// is re-walked up to that many times with fixed-point early exit.
fn process_for<'b>(for_stmt: &'b For<'b>, scope: &mut ScopeState, ctx: &ForwardWalkCtx<'_>) {
    let loop_depth = enter_loop();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        leave_loop(loop_depth);
        return;
    }

    // Process initializer expressions (e.g. `$i = 0`).
    for init_expr in for_stmt.initializations.iter() {
        process_assignment_expr(init_expr, scope, ctx);
    }

    // Process condition assignments (e.g. `for (; $x = nextItem(); )`)
    // and pass-by-ref in conditions (e.g. `for (; preg_match(..., $m); )`).
    for cond_expr in for_stmt.conditions.iter() {
        process_condition_assignment(cond_expr, scope, ctx);
        seed_pass_by_ref_in_condition(cond_expr, scope, ctx);
    }

    let pre_loop_scope = scope.clone();

    // When the cursor is inside the loop body (completion path), discovery
    // passes must walk the ENTIRE body; the final pass uses the real
    // cursor_offset so it stops at the cursor as usual.
    let body_span = match &for_stmt.body {
        ForBody::Statement(inner) => inner.span(),
        ForBody::ColonDelimited(body) => body.span(),
    };
    let cursor_in_body =
        ctx.cursor_offset >= body_span.start.offset && ctx.cursor_offset <= body_span.end.offset;
    let discovery_ctx = if cursor_in_body && !is_diagnostic_scope_active() {
        ctx.with_cursor_offset(u32::MAX)
    } else {
        ctx.with_cursor_offset(ctx.cursor_offset)
    };

    // ── Assignment-depth-bounded loop iteration ─────────────────
    let body_stmts: Vec<&Statement<'b>> = match &for_stmt.body {
        ForBody::Statement(inner) => vec![*inner],
        ForBody::ColonDelimited(body) => body.statements.iter().collect(),
    };
    let assignment_depth =
        clamp_iterations_for_depth(assignment_map_depth(&body_stmts), loop_depth);

    // ── Initial walk (always performed) ─────────────────────────
    let initial_ctx = if assignment_depth > 1 {
        &discovery_ctx
    } else {
        ctx
    };
    match &for_stmt.body {
        ForBody::Statement(inner) => {
            walk_body_forward(std::iter::once(*inner), scope, initial_ctx);
        }
        ForBody::ColonDelimited(body) => {
            walk_body_forward(body.statements.iter(), scope, initial_ctx);
        }
    }

    // ── Re-walk iterations (only if types changed) ──────────────
    for iteration in 0..assignment_depth.saturating_sub(1) {
        if !scope_has_changes(&pre_loop_scope, scope) {
            break;
        }

        let mut next_scope = pre_loop_scope.clone();
        next_scope.merge_branch(scope);
        for init_expr in for_stmt.initializations.iter() {
            process_assignment_expr(init_expr, &mut next_scope, ctx);
        }
        *scope = next_scope;

        let is_final = iteration + 1 >= assignment_depth.saturating_sub(1);
        let walk_ctx = if is_final { ctx } else { &discovery_ctx };

        match &for_stmt.body {
            ForBody::Statement(inner) => {
                walk_body_forward(std::iter::once(*inner), scope, walk_ctx);
            }
            ForBody::ColonDelimited(body) => {
                walk_body_forward(body.statements.iter(), scope, walk_ctx);
            }
        }
    }

    // The loop body might not execute at all (condition false on
    // first check), so merge with the pre-loop scope.
    let post_loop = scope.clone();
    *scope = pre_loop_scope;
    scope.merge_branch(&post_loop);

    leave_loop(loop_depth);
}

/// Process a `do-while` loop.
///
/// Uses the same assignment-depth-bounded iteration as `process_foreach`:
/// a cheap AST walk determines the dependency chain depth, then the body
/// is re-walked up to that many times with fixed-point early exit.
///
/// Unlike `for`/`while`, the body of a `do-while` always executes at
/// least once, so we do NOT merge with a pre-loop scope at the end.
fn process_do_while<'b>(dw: &'b DoWhile<'b>, scope: &mut ScopeState, ctx: &ForwardWalkCtx<'_>) {
    let loop_depth = enter_loop();

    // Hard limit: skip the body entirely at excessive nesting depth.
    if loop_depth > MAX_LOOP_DEPTH {
        leave_loop(loop_depth);
        return;
    }

    let pre_loop_scope = scope.clone();

    // ── Assignment-depth-bounded loop iteration ─────────────────
    let body_stmts: Vec<&Statement<'b>> = vec![dw.statement];
    let assignment_depth =
        clamp_iterations_for_depth(assignment_map_depth(&body_stmts), loop_depth);

    // ── Initial walk (always performed) ─────────────────────────
    walk_body_forward(std::iter::once(dw.statement), scope, ctx);

    // ── Re-walk iterations (only if types changed) ──────────────
    for _iteration in 0..assignment_depth.saturating_sub(1) {
        if !scope_has_changes(&pre_loop_scope, scope) {
            break;
        }

        let mut next_scope = pre_loop_scope.clone();
        next_scope.merge_branch(scope);
        process_condition_assignment(dw.condition, &mut next_scope, ctx);
        seed_pass_by_ref_in_condition(dw.condition, &mut next_scope, ctx);
        *scope = next_scope;

        walk_body_forward(std::iter::once(dw.statement), scope, ctx);
    }

    // After the do-while loop, the condition evaluated to false (that's
    // why the loop exited).  Apply the inverse of the condition to narrow
    // types.  For example: `do { $a = getA(); } while ($a !== null);`
    // => after loop, $a is null.
    apply_condition_narrowing_inverse(dw.condition, scope, ctx);

    leave_loop(loop_depth);
}

/// Process a `try-catch-finally` statement.
fn process_try<'b>(try_stmt: &'b Try<'b>, scope: &mut ScopeState, ctx: &ForwardWalkCtx<'_>) {
    let pre_try_scope = scope.clone();

    // Check if cursor is inside the try body.
    let try_body_span = try_stmt.block.span();
    let cursor_in_try = ctx.cursor_offset >= try_body_span.start.offset
        && ctx.cursor_offset <= try_body_span.end.offset;

    if cursor_in_try {
        // Walk only the try body.
        walk_body_forward(try_stmt.block.statements.iter(), scope, ctx);
        return;
    }

    // Check if cursor is inside a catch block.
    for catch in try_stmt.catch_clauses.iter() {
        let catch_span = catch.block.span();
        if ctx.cursor_offset >= catch_span.start.offset
            && ctx.cursor_offset <= catch_span.end.offset
        {
            // Bind the caught exception variable.
            if let Some(ref var) = catch.variable {
                let var_name = bytes_to_str(var.name).to_string();
                let parsed_hint = extract_hint_type(&catch.hint);
                let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                    &parsed_hint,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                );
                let exception_types = ResolvedType::from_classes_with_hint(resolved, parsed_hint);
                // Merge pre-try scope (since the exception could have
                // been thrown at any point in the try body) with the
                // catch variable.
                *scope = pre_try_scope.clone();
                if !exception_types.is_empty() {
                    scope.set(&var_name, exception_types);
                }
            } else {
                *scope = pre_try_scope.clone();
            }
            walk_body_forward(catch.block.statements.iter(), scope, ctx);
            return;
        }
    }

    // Check if cursor is inside the finally block.
    if let Some(ref finally) = try_stmt.finally_clause {
        let finally_span = finally.block.span();
        if ctx.cursor_offset >= finally_span.start.offset
            && ctx.cursor_offset <= finally_span.end.offset
        {
            // In finally, merge all possible paths.
            walk_body_forward(try_stmt.block.statements.iter(), scope, ctx);
            walk_body_forward(finally.block.statements.iter(), scope, ctx);
            return;
        }
    }

    // Cursor is after the try/catch/finally.  Walk the try body and
    // merge all catch scopes.
    walk_body_forward(try_stmt.block.statements.iter(), scope, ctx);
    let try_scope = scope.clone();

    let mut all_scopes = vec![try_scope];
    for catch in try_stmt.catch_clauses.iter() {
        let mut catch_scope = pre_try_scope.clone();
        if let Some(ref var) = catch.variable {
            let var_name = bytes_to_str(var.name).to_string();
            let parsed_hint = extract_hint_type(&catch.hint);
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                &parsed_hint,
                &ctx.current_class.name,
                ctx.all_classes,
                ctx.class_loader,
            );
            let exception_types = ResolvedType::from_classes_with_hint(resolved, parsed_hint);
            if !exception_types.is_empty() {
                catch_scope.set(&var_name, exception_types);
            }
        }
        walk_body_forward(catch.block.statements.iter(), &mut catch_scope, ctx);
        all_scopes.push(catch_scope);
    }

    // Merge all scopes.
    let mut merged = all_scopes[0].clone();
    for s in &all_scopes[1..] {
        merged.merge_branch(s);
    }
    *scope = merged;

    // Walk the finally block if present.
    if let Some(ref finally) = try_stmt.finally_clause {
        walk_body_forward(finally.block.statements.iter(), scope, ctx);
    }
}

/// Process a `switch` statement.
///
/// Each case arm is walked on a clone of the pre-switch scope so that
/// assignments in one arm don't leak into another.  After all arms are
/// walked, the resulting scopes are merged (union of types), matching
/// the runtime behaviour where only one arm executes.
///
/// Fall-through cases (cases with no statements) share their scope
/// with the next non-empty case, mirroring PHP semantics.
fn process_switch<'b>(switch: &'b Switch<'b>, scope: &mut ScopeState, ctx: &ForwardWalkCtx<'_>) {
    let pre_switch_scope = scope.clone();
    let cases: Vec<_> = switch.body.cases().iter().collect();

    if cases.is_empty() {
        return;
    }

    let mut branch_scopes: Vec<ScopeState> = Vec::new();
    let mut has_default = false;

    // Walk cases, accumulating fall-through groups.
    let mut accumulated_stmts: Vec<&Statement<'b>> = Vec::new();
    for case in &cases {
        if case.is_default() {
            has_default = true;
        }

        let stmts: Vec<_> = case.statements().iter().collect();
        if stmts.is_empty() {
            // Fall-through: no statements, will share scope with next case.
            continue;
        }

        accumulated_stmts.extend(stmts);

        let mut case_scope = pre_switch_scope.clone();
        walk_body_forward(accumulated_stmts.iter().copied(), &mut case_scope, ctx);
        branch_scopes.push(case_scope);
        accumulated_stmts.clear();
    }

    // Handle trailing fall-through cases (empty cases at the end).
    if !accumulated_stmts.is_empty() {
        let mut case_scope = pre_switch_scope.clone();
        walk_body_forward(accumulated_stmts.iter().copied(), &mut case_scope, ctx);
        branch_scopes.push(case_scope);
    }

    if branch_scopes.is_empty() {
        return;
    }

    // Merge all branch scopes.
    let mut merged = branch_scopes[0].clone();
    for s in &branch_scopes[1..] {
        merged.merge_branch(s);
    }

    // If there is no default case, the switch might not execute any
    // arm at all, so merge with the pre-switch scope.
    if !has_default {
        merged.merge_branch(&pre_switch_scope);
    }

    *scope = merged;
}

// ─── Narrowing helpers ──────────────────────────────────────────────────────

/// Apply condition-based narrowing (instanceof, null check, type guard)
/// to the scope.  This narrows types for the "truthy" branch.
fn apply_condition_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Seed property access keys from conditions into the scope so that
    // narrowing functions can find and narrow them.
    seed_property_keys_into_scope(condition, scope, ctx);

    // Decompose `&&` chains so that `$x instanceof Foo && $x instanceof Bar`
    // applies both narrowings as a union (intersection semantics: the
    // variable satisfies both checks, so members from both types are
    // available).
    let operands = collect_and_chain_operands(condition);

    // First pass: collect all instanceof extractions per variable across
    // all `&&` operands.  This prevents later operands from overwriting
    // earlier ones when both narrow the same variable.
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    let mut var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    // Include variables from instanceof conditions that may not be in
    // scope yet (e.g. undeclared variables used in instanceof checks).
    for name in collect_condition_var_names(condition) {
        if !var_names.contains(&name) {
            var_names.push(name);
        }
    }
    // Include property access keys from conditions (e.g. `$a->foo`
    // from `$a->foo instanceof Foo`) so instanceof narrowing applies.
    for key in collect_condition_property_keys(condition) {
        if !var_names.contains(&key) {
            var_names.push(key);
        }
    }

    // Track which variables have been narrowed by instanceof across
    // `&&` operands so we can merge them into a union.
    let mut instanceof_results: HashMap<String, Vec<ResolvedType>> = HashMap::new();

    for operand in &operands {
        for var_name in &var_names {
            // Compound OR instanceof: `$x instanceof A || $x instanceof B`
            if let Some(classes) = narrowing::try_extract_compound_or_instanceof(operand, var_name)
                && !classes.is_empty()
            {
                let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
                let union = narrowing::resolve_class_names_to_union(&classes, &var_ctx);
                if !union.is_empty() {
                    let entry = instanceof_results.entry(var_name.clone()).or_default();
                    ResolvedType::extend_unique(
                        entry,
                        union.into_iter().map(ResolvedType::from_class).collect(),
                    );
                }
                continue;
            }

            // Single instanceof (including negated, is_a, get_class).
            if let Some(extraction) =
                narrowing::try_extract_instanceof_with_negation(operand, var_name)
            {
                let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
                if extraction.negated {
                    // Negated instanceof: apply exclusion to the current
                    // scope immediately (each negation removes one type).
                    let mut results = scope.get(var_name).to_vec();
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_exclusion(
                            &extraction.class_type,
                            &var_ctx,
                            classes,
                        );
                    });
                    // Negated instanceof exclusion does NOT eliminate
                    // null — `!$x instanceof Foo` is true when $x is
                    // null, so null stays in the union.  No stripping.
                    if !results.is_empty() {
                        scope.set(var_name, results);
                    }
                } else {
                    // Positive instanceof: resolve and accumulate into
                    // the per-variable union.  For a single operand this
                    // produces `[Foo]`; for `&& instanceof Bar` it
                    // accumulates `[Foo, Bar]`.
                    let mut single = Vec::new();
                    ResolvedType::apply_narrowing(&mut single, |classes| {
                        narrowing::apply_instanceof_inclusion(
                            &extraction.class_type,
                            extraction.exact,
                            &var_ctx,
                            classes,
                        );
                    });
                    if !single.is_empty() {
                        let entry = instanceof_results.entry(var_name.clone()).or_default();
                        ResolvedType::extend_unique(entry, single);
                    } else {
                        // Target class is unresolvable — mark variable
                        // as empty so diagnostics suppress false positives.
                        instanceof_results.entry(var_name.clone()).or_default();
                    }
                }
            }
        }
    }

    // Apply the accumulated instanceof narrowing results to the scope.
    for (var_name, narrowed) in instanceof_results {
        if !narrowed.is_empty() {
            let existing = scope.get(&var_name);
            if existing.is_empty() {
                // Untyped variable — instanceof provides the type.
                scope.set(&var_name, narrowed);
            } else {
                // When the existing type is entirely `mixed` or
                // `object`, instanceof replaces it — there is no
                // useful information to preserve or intersect.
                let all_broad = existing.iter().all(|rt| {
                    rt.class_info.is_none()
                        && matches!(
                            rt.type_string.unwrap_nullable(),
                            PhpType::Named(n) if n.eq_ignore_ascii_case("mixed") || n.eq_ignore_ascii_case("object")
                        )
                });
                if all_broad {
                    scope.set(&var_name, narrowed);
                    continue;
                }

                // Typed variable — filter the existing union to only
                // types present in the narrowed set.  This correctly
                // handles both single instanceof (`Dog|Cat` → `Dog`)
                // and OR instanceof (`Dog|Cat|Other` → `Dog|Cat`).
                //
                // When the narrowed type is NOT in the existing union
                // (e.g. `MockInterface` narrowed to `MolliePayment`),
                // this is an intersection case — apply via
                // apply_instanceof_inclusion which has interface
                // intersection logic.
                let narrowed_fqns: Vec<String> = narrowed
                    .iter()
                    .filter_map(|rt| rt.class_info.as_ref().map(|c| c.fqn().to_string()))
                    .collect();

                // Try filtering: keep existing entries whose class is
                // in the narrowed set.  Strip null from the type_string
                // because a successful instanceof check guarantees the
                // value is non-null (e.g. `?Foo` → `Foo`).
                let filtered: Vec<ResolvedType> = existing
                    .iter()
                    .filter(|rt| {
                        rt.class_info
                            .as_ref()
                            .is_some_and(|c| narrowed_fqns.contains(&c.fqn().to_string()))
                    })
                    .map(|rt| {
                        if let Some(non_null) = rt.type_string.non_null_type() {
                            ResolvedType {
                                type_string: non_null,
                                class_info: rt.class_info.clone(),
                            }
                        } else {
                            rt.clone()
                        }
                    })
                    .collect();

                if !filtered.is_empty() {
                    // Filter matched — use the filtered results
                    // (preserves richer type info from original resolution).
                    // Also strip bare `null` entries: a successful
                    // instanceof check guarantees non-null, so `null`
                    // entries added by `from_classes_with_hint` must
                    // be removed.
                    let filtered: Vec<ResolvedType> = filtered
                        .into_iter()
                        .filter(|rt| !rt.type_string.is_null())
                        .collect();
                    if filtered.is_empty() {
                        scope.set(&var_name, narrowed);
                    } else {
                        scope.set(&var_name, filtered);
                    }
                } else {
                    // No overlap between existing and narrowed types.
                    // This is the intersection case (e.g. MockInterface
                    // narrowed to MolliePayment).  Use
                    // apply_instanceof_inclusion which produces the
                    // intersection when one side is an interface.
                    let mut results = existing.to_vec();
                    // Apply all narrowed classes as a single group by
                    // building a union type.
                    let union_type = if narrowed_fqns.len() == 1 {
                        PhpType::Named(narrowed_fqns[0].clone())
                    } else {
                        PhpType::Union(
                            narrowed_fqns
                                .iter()
                                .map(|n| PhpType::Named(n.clone()))
                                .collect(),
                        )
                    };
                    let var_ctx = build_var_ctx(&var_name, ctx, &scope_resolver);
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_inclusion(
                            &union_type,
                            false,
                            &var_ctx,
                            classes,
                        );
                    });
                    // Instanceof guarantees non-null — strip bare
                    // `null` entries that were preserved by
                    // `apply_narrowing`'s `None => true` rule.
                    results.retain(|rt| !rt.type_string.is_null());
                    if !results.is_empty() {
                        scope.set(&var_name, results);
                    } else {
                        // Fallback: use the narrowed types directly.
                        scope.set(&var_name, narrowed);
                    }
                }
            }
        } else {
            // Empty narrowed list means the target was unresolvable.
            scope.locals.insert(atom(&var_name), vec![]);
        }
    }

    // Type guard narrowing: `is_object($x)`, `is_array($x)`, etc.
    apply_type_guard_narrowing_truthy(condition, scope);

    // Null narrowing: `if ($x !== null)` — remove null from scope.
    apply_null_narrowing_truthy(condition, scope, ctx);

    // @phpstan-assert-if-true / -if-false narrowing.
    apply_phpstan_assert_condition_narrowing(condition, scope, ctx, false);

    // in_array($var, $haystack, true) narrowing.
    apply_in_array_narrowing(condition, scope, ctx, false);
}

/// Apply inverse narrowing for a single condition expression (not
/// decomposed).  Called by [`apply_condition_narrowing_inverse`] for
/// each operand in a `&&` chain, or for the whole condition when it
/// is not a chain.
fn apply_condition_narrowing_inverse_single<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Seed property access keys from conditions into the scope so that
    // narrowing functions can find and narrow them.
    seed_property_keys_into_scope(condition, scope, ctx);

    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    // Include variables from instanceof conditions that may not be in
    // scope yet (e.g. `if (!$foobar instanceof Foobar) { break; }`
    // where `$foobar` was never assigned).  After the guard clause,
    // `$foobar` must be `Foobar`.
    let mut var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    for name in collect_condition_var_names(condition) {
        if !var_names.contains(&name) {
            var_names.push(name);
        }
    }
    // Include property access keys from conditions (e.g. `$a->foo`
    // from `$a->foo instanceof Foo`) so instanceof narrowing applies.
    for key in collect_condition_property_keys(condition) {
        if !var_names.contains(&key) {
            var_names.push(key);
        }
    }
    for var_name in &var_names {
        if let Some(classes) = narrowing::try_extract_compound_or_instanceof(condition, var_name)
            && !classes.is_empty()
        {
            let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
            let mut results = scope.get(var_name).to_vec();
            for cls_type in &classes {
                ResolvedType::apply_narrowing(&mut results, |class_list| {
                    narrowing::apply_instanceof_exclusion(cls_type, &var_ctx, class_list);
                });
            }
            if !results.is_empty() {
                scope.set(var_name, results);
            }
            continue;
        }

        if let Some(extraction) =
            narrowing::try_extract_instanceof_with_negation(condition, var_name)
        {
            let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
            let mut results = scope.get(var_name).to_vec();
            if extraction.negated {
                // Inverse of negated instanceof → positive instanceof.
                // Instanceof guarantees non-null, so strip null entries.
                ResolvedType::apply_narrowing(&mut results, |classes| {
                    narrowing::apply_instanceof_inclusion(
                        &extraction.class_type,
                        extraction.exact,
                        &var_ctx,
                        classes,
                    );
                });
                results.retain(|rt| !rt.type_string.is_null());
            } else {
                // Inverse of positive instanceof → exclusion.
                // Exclusion does NOT strip null (`!instanceof` is
                // true for null values).
                ResolvedType::apply_narrowing(&mut results, |classes| {
                    narrowing::apply_instanceof_exclusion(
                        &extraction.class_type,
                        &var_ctx,
                        classes,
                    );
                });
            }
            if !results.is_empty() {
                scope.set(var_name, results);
            }
        }
    }
}

/// Apply inverse condition-based narrowing (for else branches and
/// guard clauses).
fn apply_condition_narrowing_inverse<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Decompose `||` chains: NOT (A || B) = !A && !B.
    // Each operand's inverse is applied sequentially (intersection
    // semantics: all must hold simultaneously).
    let or_operands = collect_or_chain_operands(condition);
    if or_operands.len() > 1 {
        for operand in &or_operands {
            apply_condition_narrowing_inverse_single(operand, scope, ctx);
        }
        // Type guard, null, phpstan-assert, and in_array narrowing
        // operate on the full condition expression.
        apply_type_guard_narrowing_inverse(condition, scope);
        apply_null_narrowing_inverse(condition, scope, ctx);
        apply_phpstan_assert_condition_narrowing(condition, scope, ctx, true);
        apply_in_array_narrowing(condition, scope, ctx, true);
        return;
    }

    // Decompose `&&` chains so that each operand is processed
    // individually.  For guard clauses like
    // `if (!$x instanceof A && !$x instanceof B) { return; }`,
    // the inverse (code after the guard) means `$x IS A || $x IS B`.
    //
    // De Morgan: NOT (!A && !B) = A || B.  Each operand's inverse
    // produces one branch of the union.  We clone the scope for each
    // operand, apply the inverse, then merge (union) all results back
    // into the main scope.
    let operands = collect_and_chain_operands(condition);
    if operands.len() > 1 {
        let base_scope = scope.clone();
        let mut branch_scopes: Vec<ScopeState> = Vec::new();
        for operand in &operands {
            let mut branch = base_scope.clone();
            apply_condition_narrowing_inverse_single(operand, &mut branch, ctx);
            branch_scopes.push(branch);
        }
        // Merge all branch scopes (union of all narrowed types).
        if let Some(first) = branch_scopes.first() {
            let mut merged = first.clone();
            for branch in &branch_scopes[1..] {
                merged.merge_branch(branch);
            }
            *scope = merged;
        }
        // Type guard, null, phpstan-assert, and in_array narrowing
        // operate on the full condition expression.
        apply_type_guard_narrowing_inverse(condition, scope);
        apply_null_narrowing_inverse(condition, scope, ctx);
        apply_phpstan_assert_condition_narrowing(condition, scope, ctx, true);
        apply_in_array_narrowing(condition, scope, ctx, true);
        return;
    }

    apply_condition_narrowing_inverse_single(condition, scope, ctx);

    // Inverse type guard narrowing: `if (is_object($x))` in else → exclude object.
    apply_type_guard_narrowing_inverse(condition, scope);

    // Inverse null narrowing: `if ($x === null)` after guard → remove null.
    apply_null_narrowing_inverse(condition, scope, ctx);

    // Inverse @phpstan-assert-if-true / -if-false narrowing.
    apply_phpstan_assert_condition_narrowing(condition, scope, ctx, true);

    // Inverse in_array narrowing: exclude the element type in the else branch.
    apply_in_array_narrowing(condition, scope, ctx, true);
}

/// Apply `in_array($var, $haystack, true)` narrowing.
///
/// When `inverted` is false (truthy branch / while body), the variable is
/// narrowed to the haystack's element type (inclusion).  When `inverted` is
/// true (else branch / guard clause inverse), the variable is narrowed by
/// excluding the element type.
fn apply_in_array_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inverted: bool,
) {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };

    // Unwrap parentheses and detect negation.
    let (inner, negated) = narrowing::unwrap_condition_negation(condition);

    // Check every variable in scope as the potential needle.
    let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
    for var_name in &var_names {
        if let Some(haystack_expr) = narrowing::try_extract_in_array(inner, var_name) {
            // Resolve the haystack's type from the scope to extract the
            // element type.  This replaces the backward scanner's
            // `resolve_arg_raw_type` with a scope-based lookup.
            let element_type = resolve_in_array_element_type_fw(haystack_expr, scope, ctx);
            let element_type = match element_type {
                Some(et) => et,
                None => continue,
            };

            // Determine whether to include or exclude:
            // - truthy + positive  → include (var IS in haystack)
            // - truthy + negated   → exclude (var is NOT in haystack)
            // - inverse + positive → exclude
            // - inverse + negated  → include
            let should_exclude = inverted ^ negated;

            let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
            let mut results = scope.get(var_name).to_vec();

            if should_exclude {
                // Skip exclusion when it would remove ALL type information.
                let would_remove_all = {
                    let mut test = results.clone();
                    ResolvedType::apply_narrowing(&mut test, |classes| {
                        narrowing::apply_instanceof_exclusion(&element_type, &var_ctx, classes);
                    });
                    test.is_empty()
                };
                if !would_remove_all {
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_exclusion(&element_type, &var_ctx, classes);
                    });
                }
            } else {
                ResolvedType::apply_narrowing(&mut results, |classes| {
                    narrowing::apply_instanceof_inclusion(&element_type, false, &var_ctx, classes);
                });
            }

            if !results.is_empty() {
                scope.set(var_name, results);
            }
        }
    }
}

/// Resolve the element type of a haystack expression for `in_array`
/// narrowing, using the forward walker's scope instead of the backward
/// scanner.
fn resolve_in_array_element_type_fw(
    haystack_expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<PhpType> {
    // If the haystack is a simple variable, look it up in the scope.
    if let Expression::Variable(Variable::Direct(dv)) = haystack_expr {
        let var_name = bytes_to_str(dv.name).to_string();
        let types = scope.get(&var_name);
        if !types.is_empty() {
            let joined = ResolvedType::types_joined(types);
            if let Some(elem) = joined.extract_element_type() {
                return Some(elem.clone());
            }
            // Try extracting value type for generic collections.
            if let Some(val) = joined.extract_value_type(true) {
                return Some(val.clone());
            }
        }
        // Fall back to docblock annotation.
        let offset = haystack_expr.span().start.offset as usize;
        let from_docblock =
            crate::docblock::find_iterable_raw_type_in_source(ctx.content, offset, &var_name)
                .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));
        if let Some(raw) = from_docblock
            && let Some(elem) = raw.extract_element_type()
        {
            return Some(elem.clone());
        }
        return None;
    }

    // For non-variable expressions (method calls, property access, etc.),
    // try resolving via the expression resolution pipeline.
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    let var_ctx = build_var_ctx("", ctx, &scope_resolver);
    let raw_type =
        crate::completion::variable::resolution::resolve_arg_raw_type(haystack_expr, &var_ctx);
    raw_type.and_then(|t| t.extract_element_type().cloned())
}

/// Apply null narrowing for the truthy branch.
/// Build a [`VarResolutionCtx`] from a variable name and forward-walk context.
///
/// Shared helper used by the narrowing functions in this module to avoid
/// repeating the struct construction at every call site.
/// Apply `@phpstan-assert-if-true` / `@phpstan-assert-if-false` narrowing
/// from a function or static/instance method call used as a condition.
///
/// When `inverted` is false we are in the truthy branch (then-body or
/// while-body).  When `inverted` is true we are in the else branch or
/// applying guard-clause inverse narrowing.
fn apply_phpstan_assert_condition_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inverted: bool,
) {
    use crate::types::AssertionKind;

    // Unwrap parentheses and detect negation (`!func($var)`).
    let (func_call_expr, condition_negated) = narrowing::unwrap_condition_negation(condition);

    let call = match func_call_expr {
        Expression::Call(c) => c,
        _ => return,
    };

    // Determine whether the function returned true in this branch.
    let function_returned_true = !(inverted ^ condition_negated);

    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };

    // Try to extract assertion info from function calls and static method calls.
    match call {
        Call::Function(func_call) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let func_info = match ctx.loaders.function_loader {
                Some(fl) => match fl(&func_name) {
                    Some(fi) => fi,
                    None => return,
                },
                None => return,
            };
            if func_info.type_assertions.is_empty() {
                return;
            }
            for assertion in &func_info.type_assertions {
                let applies_positively = match assertion.kind {
                    AssertionKind::IfTrue => function_returned_true,
                    AssertionKind::IfFalse => !function_returned_true,
                    AssertionKind::Always => continue,
                };
                if let Some(arg_var) = narrowing::find_assertion_arg_variable(
                    &func_call.argument_list,
                    &assertion.param_name,
                    &func_info.parameters,
                ) {
                    let should_exclude = assertion.negated ^ !applies_positively;
                    let var_ctx = build_var_ctx(&arg_var, ctx, &scope_resolver);
                    let mut results = scope.get(&arg_var).to_vec();
                    if should_exclude {
                        ResolvedType::apply_narrowing(&mut results, |classes| {
                            narrowing::apply_instanceof_exclusion(
                                &assertion.asserted_type,
                                &var_ctx,
                                classes,
                            );
                        });
                    } else {
                        ResolvedType::apply_narrowing(&mut results, |classes| {
                            narrowing::apply_instanceof_inclusion(
                                &assertion.asserted_type,
                                false,
                                &var_ctx,
                                classes,
                            );
                        });
                    }
                    if !results.is_empty() {
                        scope.set(&arg_var, results);
                    }
                }
            }
        }
        Call::StaticMethod(static_call) => {
            let class_name = match static_call.class {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                Expression::Self_(_) | Expression::Static(_) => ctx.current_class.name.to_string(),
                _ => return,
            };
            let method_name = match &static_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            let class_info = match (ctx.class_loader)(&class_name) {
                Some(ci) => ci,
                None => return,
            };
            let method = match class_info
                .methods
                .iter()
                .find(|m| m.name == method_name && m.is_static)
            {
                Some(m) => m.clone(),
                None => return,
            };
            if method.type_assertions.is_empty() {
                return;
            }
            for assertion in &method.type_assertions {
                let applies_positively = match assertion.kind {
                    AssertionKind::IfTrue => function_returned_true,
                    AssertionKind::IfFalse => !function_returned_true,
                    AssertionKind::Always => continue,
                };
                if let Some(arg_var) = narrowing::find_assertion_arg_variable(
                    &static_call.argument_list,
                    &assertion.param_name,
                    &method.parameters,
                ) {
                    let should_exclude = assertion.negated ^ !applies_positively;
                    // Resolve `self`/`static`/`$this` in the asserted type
                    // against the declaring class, not the enclosing class.
                    let resolved_assert_type = if assertion.asserted_type.contains_self_ref() {
                        assertion.asserted_type.replace_self(&class_info.fqn())
                    } else {
                        assertion.asserted_type.clone()
                    };
                    let var_ctx = build_var_ctx(&arg_var, ctx, &scope_resolver);
                    let mut results = scope.get(&arg_var).to_vec();
                    if should_exclude {
                        ResolvedType::apply_narrowing(&mut results, |classes| {
                            narrowing::apply_instanceof_exclusion(
                                &resolved_assert_type,
                                &var_ctx,
                                classes,
                            );
                        });
                    } else {
                        ResolvedType::apply_narrowing(&mut results, |classes| {
                            narrowing::apply_instanceof_inclusion(
                                &resolved_assert_type,
                                false,
                                &var_ctx,
                                classes,
                            );
                        });
                    }
                    if !results.is_empty() {
                        scope.set(&arg_var, results);
                    }
                }
            }
        }
        Call::Method(method_call) => {
            // Instance method: `$var->method()` with `@phpstan-assert-if-true Type $this`
            let receiver_var = match method_call.object {
                Expression::Variable(Variable::Direct(dv)) => bytes_to_str(dv.name).to_string(),
                _ => return,
            };
            let method_name = match &method_call.method {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return,
            };
            // Resolve the receiver's type to find the method's assertions.
            let receiver_types = scope.get(&receiver_var);
            if receiver_types.is_empty() {
                return;
            }
            // Collect assertions from all candidate classes.
            let mut to_apply: Vec<(crate::php_type::PhpType, bool, String)> = Vec::new();
            for rt in receiver_types {
                let class_info = match (ctx.class_loader)(&rt.type_string.to_string()) {
                    Some(ci) => ci,
                    None => {
                        continue;
                    }
                };
                let method = match class_info
                    .methods
                    .iter()
                    .find(|m| m.name == method_name && !m.is_static)
                {
                    Some(m) => m,
                    None => continue,
                };
                for assertion in &method.type_assertions {
                    let applies_positively = match assertion.kind {
                        AssertionKind::IfTrue => function_returned_true,
                        AssertionKind::IfFalse => !function_returned_true,
                        AssertionKind::Always => continue,
                    };
                    let should_exclude = assertion.negated ^ !applies_positively;
                    // Resolve `self`/`static`/`$this` in the asserted type
                    // against the *declaring* class (e.g. `Decimal`), not the
                    // enclosing class (e.g. `Monetary`).  Without this,
                    // `@phpstan-assert-if-false self<true> $this` on
                    // `Decimal::isZero()` would narrow $denominator to
                    // `Monetary` instead of `Decimal`.
                    let resolved_type = if assertion.asserted_type.contains_self_ref() {
                        assertion.asserted_type.replace_self(&class_info.fqn())
                    } else {
                        assertion.asserted_type.clone()
                    };
                    if assertion.param_name == "$this" {
                        // Narrows the receiver variable itself.
                        to_apply.push((resolved_type, should_exclude, receiver_var.clone()));
                    } else if let Some(arg_var) = narrowing::find_assertion_arg_variable(
                        &method_call.argument_list,
                        &assertion.param_name,
                        &method.parameters,
                    ) {
                        to_apply.push((resolved_type, should_exclude, arg_var));
                    }
                }
            }
            for (asserted_type, should_exclude, target_var) in to_apply {
                let var_ctx = build_var_ctx(&target_var, ctx, &scope_resolver);
                let mut results = scope.get(&target_var).to_vec();
                if should_exclude {
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_exclusion(&asserted_type, &var_ctx, classes);
                    });
                } else {
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_inclusion(
                            &asserted_type,
                            false,
                            &var_ctx,
                            classes,
                        );
                    });
                }
                if !results.is_empty() {
                    scope.set(&target_var, results);
                }
            }
        }
        _ => {}
    }
}

fn build_var_ctx<'a>(
    var_name: &'a str,
    ctx: &'a ForwardWalkCtx<'_>,
    scope_resolver: &'a dyn Fn(&str) -> Vec<ResolvedType>,
) -> VarResolutionCtx<'a> {
    VarResolutionCtx {
        var_name,
        current_class: ctx.current_class,
        all_classes: ctx.all_classes,
        content: ctx.content,
        cursor_offset: ctx.cursor_offset,
        class_loader: ctx.class_loader,
        loaders: ctx.loaders,
        resolved_class_cache: ctx.resolved_class_cache,
        enclosing_return_type: ctx.enclosing_return_type.clone(),
        top_level_scope: ctx.top_level_scope.clone(),
        branch_aware: false,
        match_arm_narrowing: HashMap::new(),
        scope_var_resolver: Some(scope_resolver),
    }
}

///
/// Handles `$x !== null`, `$x != null`, `isset($x)`, `!empty($x)`,
/// `!is_null($x)`, and truthiness checks.
/// Apply type-guard narrowing in the truthy branch.
///
/// When `is_object($var)` (or `is_array`, `is_string`, etc.) appears
/// in a condition, narrow the variable's type.  For `mixed` variables,
/// this replaces `mixed` with the guard's canonical type (e.g. `object`).
/// For union types, it filters to only the members that match the guard.
///
/// Handles compound `&&` conditions by decomposing them into individual
/// operands and applying each type guard found.  For example,
/// `is_object($data) && property_exists($data, 'error_link')` applies
/// the `is_object` guard to `$data`.
fn apply_type_guard_narrowing_truthy(condition: &Expression<'_>, scope: &mut ScopeState) {
    apply_type_guard_on_operands(condition, scope, true);
}

/// Apply type-guard narrowing in the inverse (else) branch.
///
/// When `is_object($var)` appears in a condition, the else branch
/// knows the variable is NOT an object — filter out object-like
/// members from the union type.
fn apply_type_guard_narrowing_inverse(condition: &Expression<'_>, scope: &mut ScopeState) {
    apply_type_guard_on_operands(condition, scope, false);
}

/// Shared implementation for truthy and inverse type-guard narrowing.
///
/// Decomposes `&&` chains into individual operands and applies each
/// type guard found.  When `truthy` is `true`, applies inclusion
/// narrowing (then-body); when `false`, applies exclusion (else-body).
fn apply_type_guard_on_operands(condition: &Expression<'_>, scope: &mut ScopeState, truthy: bool) {
    // Decompose `&&` chains so that `is_object($x) && is_string($y)`
    // applies both guards.
    let operands = collect_and_chain_operands(condition);
    let mut var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    // Include property access keys from conditions (e.g. `$a->foo`
    // from `is_string($a->foo)`) so they can be narrowed.
    for key in collect_condition_property_keys(condition) {
        if !var_names.contains(&key) {
            var_names.push(key);
        }
    }
    for operand in &operands {
        for var_name in &var_names {
            if let Some((kind, negated)) = narrowing::try_extract_type_guard(operand, var_name) {
                // When the guard is negated (e.g. `!is_object($x)`),
                // flip the inclusion/exclusion logic: the truthy branch
                // of a negated guard means the variable is NOT the
                // guarded type, and vice versa.
                let effective_truthy = if negated { !truthy } else { truthy };
                let mut results = scope.get(var_name).to_vec();
                if !results.is_empty() {
                    if effective_truthy {
                        narrowing::apply_type_guard_inclusion(kind, &mut results);
                    } else {
                        narrowing::apply_type_guard_exclusion(kind, &mut results);
                    }
                    if !results.is_empty() {
                        scope.set(var_name, results);
                    }
                }
            }
        }
    }
}

fn apply_null_narrowing_truthy<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Decompose `&&` chains so that `isset($a) && isset($b)` narrows
    // both variables, and `$x !== null && $y !== null` works too.
    let operands = collect_and_chain_operands(condition);
    if operands.len() > 1 {
        for operand in &operands {
            apply_null_narrowing_truthy(operand, scope, ctx);
        }
        return;
    }

    // Check for `$x !== null` or `$x != null` or `null !== $x` etc.
    if let Some(var_name) = extract_non_null_check_var(condition) {
        // For array access keys, narrow the shape on the base variable.
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
    // `isset($x)` — truthy branch means $x is not null: strip null.
    // Handles multiple args: `isset($a, $b)` strips null from both.
    for var_name in extract_isset_vars(condition) {
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
    // `!isset($x)` — truthy branch means $x is null: narrow to null.
    for var_name in extract_not_isset_vars(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // Check for `$x === null` or `$x == null` — narrow to null only.
    if let Some(var_name) = extract_null_equality_check_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // `!empty($x)` — truthy branch means $x is non-empty (truthy):
    // strip null (and false) from the type.
    if let Some(var_name) = extract_not_empty_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        strip_null_from_scope(&var_name, scope);
    }
}

/// Apply inverse null narrowing (for guard clause: `if ($x === null) { return; }`).
fn apply_null_narrowing_inverse<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // When the condition is `$x === null` (equality check for null),
    // the inverse (else/guard) means $x is NOT null.
    if let Some(var_name) = extract_null_equality_check_var(condition) {
        // For array access keys like `$a["test"]`, narrow the array
        // shape on the base variable directly rather than using a
        // synthetic scope entry.  This ensures the narrowed shape
        // survives scope merges.
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
    // When the condition is `$x !== null`, the inverse (else/guard)
    // means $x IS null — narrow to null only.
    if let Some(var_name) = extract_non_null_check_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // When the condition is `!$x` or `empty($x)`, the inverse means
    // $x is truthy — remove null.
    if let Some(var_name) = extract_falsy_check_var(condition) {
        strip_null_from_scope(&var_name, scope);
    }
    // When the condition is a bare `$x` (truthy check), the inverse means
    // $x is falsy.  For nullable types (`T|null`), narrow to null.
    // This handles `while ($a) { ... }` => after loop, $a is null.
    if let Some(var_name) = expr_to_var_name(condition) {
        narrow_to_null_in_scope(&var_name, scope);
    }
    // `isset($x)` — inverse (else) means $x was null: narrow to null.
    for var_name in extract_isset_vars(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // `!isset($x)` — inverse (guard after `!isset` return) means $x
    // is not null: strip null.
    for var_name in extract_not_isset_vars(condition) {
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
}

/// Extract variable name from `$x !== null` or `null !== $x` patterns.
fn extract_non_null_check_var(expr: &Expression<'_>) -> Option<String> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    match inner {
        Expression::Binary(bin) => {
            let is_not_identical = matches!(bin.operator, BinaryOperator::NotIdentical(_));
            let is_not_equal = matches!(bin.operator, BinaryOperator::NotEqual(_));
            let is_identical = matches!(bin.operator, BinaryOperator::Identical(_));
            let is_equal = matches!(bin.operator, BinaryOperator::Equal(_));

            // `$x !== null` or `null !== $x`
            if (is_not_identical || is_not_equal) && !negated
                || (is_identical || is_equal) && negated
            {
                if is_null_expr(bin.rhs) {
                    return expr_to_var_name(bin.lhs)
                        .or_else(|| narrowing::expr_to_subject_key(bin.lhs));
                }
                if is_null_expr(bin.lhs) {
                    return expr_to_var_name(bin.rhs)
                        .or_else(|| narrowing::expr_to_subject_key(bin.rhs));
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract all variable names from an `isset(…)` call (non-negated).
/// Handles simple variables (`$x`) and property/array access keys
/// (`$obj->prop`, `$arr["key"]`).  Returns an empty vec when the
/// expression is not an `isset()` call, or when it is negated.
fn extract_isset_vars(expr: &Expression<'_>) -> Vec<String> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    if negated {
        return vec![];
    }
    // `isset()` is a language construct, parsed as Expression::Construct(Construct::Isset).
    let Expression::Construct(Construct::Isset(isset)) = inner else {
        return vec![];
    };
    let mut vars = Vec::new();
    for value in isset.values.iter() {
        if let Some(name) =
            expr_to_var_name(value).or_else(|| narrowing::expr_to_subject_key(value))
        {
            vars.push(name);
        }
    }
    vars
}

/// Extract all variable names from a `!isset(…)` call (negated isset).
/// Returns an empty vec when the expression is not a negated `isset()`.
fn extract_not_isset_vars(expr: &Expression<'_>) -> Vec<String> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    if !negated {
        return vec![];
    }
    // `isset()` is a language construct, parsed as Expression::Construct(Construct::Isset).
    let Expression::Construct(Construct::Isset(isset)) = inner else {
        return vec![];
    };
    let mut vars = Vec::new();
    for value in isset.values.iter() {
        if let Some(name) =
            expr_to_var_name(value).or_else(|| narrowing::expr_to_subject_key(value))
        {
            vars.push(name);
        }
    }
    vars
}

/// Extract variable name from `$x === null` or `null === $x` patterns.
fn extract_null_equality_check_var(expr: &Expression<'_>) -> Option<String> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    match inner {
        Expression::Binary(bin) => {
            let is_identical = matches!(bin.operator, BinaryOperator::Identical(_));
            let is_equal = matches!(bin.operator, BinaryOperator::Equal(_));

            if (is_identical || is_equal) && !negated {
                if is_null_expr(bin.rhs) {
                    return expr_to_var_name(bin.lhs)
                        .or_else(|| narrowing::expr_to_subject_key(bin.lhs));
                }
                if is_null_expr(bin.lhs) {
                    return expr_to_var_name(bin.rhs)
                        .or_else(|| narrowing::expr_to_subject_key(bin.rhs));
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract variable name from `!empty($x)` (negated empty check).
fn extract_not_empty_var(expr: &Expression<'_>) -> Option<String> {
    if let Expression::UnaryPrefix(prefix) = expr
        && prefix.operator.is_not()
        && let Expression::Construct(Construct::Empty(empty)) = prefix.operand
    {
        return expr_to_var_name(empty.value);
    }
    None
}

/// Extract variable name from falsy checks: `!$x`, `empty($x)`.
fn extract_falsy_check_var(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            expr_to_var_name(prefix.operand)
        }
        // `empty($x)` — language construct, parsed as Expression::Construct(Construct::Empty).
        Expression::Construct(Construct::Empty(empty)) => expr_to_var_name(empty.value),
        _ => None,
    }
}

/// Check if an expression is `null`.
fn is_null_expr(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Literal(Literal::Null(_)) => true,
        Expression::ConstantAccess(ca) => {
            let name = ca.name.value();
            let clean = crate::util::strip_fqn_prefix(bytes_to_str(name));
            clean.eq_ignore_ascii_case("null")
        }
        _ => false,
    }
}

/// Extract a direct variable name from an expression.
fn expr_to_var_name(expr: &Expression<'_>) -> Option<String> {
    if let Expression::Variable(Variable::Direct(dv)) = expr {
        Some(bytes_to_str(dv.name).to_string())
    } else {
        None
    }
}

/// Strip `null` from a variable's type in the scope.
/// Narrow a variable in scope to `null` only.
///
/// Used when a condition like `$x === null` is true: the variable must
/// be null.  Replaces the variable's type with `null` if it currently
/// contains a nullable type, or sets it to `null` if the variable has
/// any type at all.
fn narrow_to_null_in_scope(var_name: &str, scope: &mut ScopeState) {
    let types = scope.get(var_name).to_vec();
    if types.is_empty() {
        return;
    }
    // Check whether any existing type contains null (Nullable, Union
    // with null member, or bare null).  `non_null_type()` returns
    // `Some` for `?T` and `T|null` unions; `is_null()` catches bare
    // `null`.
    let has_null = types
        .iter()
        .any(|rt| rt.type_string.non_null_type().is_some() || rt.type_string.is_null());
    if has_null {
        scope.set(
            var_name,
            vec![ResolvedType::from_type_string(PhpType::null())],
        );
    }
}

fn strip_null_from_scope(var_name: &str, scope: &mut ScopeState) {
    let types = scope.get(var_name).to_vec();
    if types.is_empty() {
        return;
    }

    let stripped: Vec<ResolvedType> = types
        .into_iter()
        .filter_map(|mut rt| match rt.type_string.non_null_type() {
            Some(non_null) => {
                rt.type_string = non_null;
                Some(rt)
            }
            None if rt.type_string == PhpType::null() => None,
            None => Some(rt),
        })
        .collect();

    if !stripped.is_empty() {
        scope.set(var_name, stripped);
    }
}

/// Strip both `null` and `false` from a variable's type in the scope.
///
/// Used after falsy guard clauses (`if (!$var) { throw; }`) where the
/// variable is known to be truthy (non-null and non-false) after the guard.
fn strip_falsy_from_scope(var_name: &str, scope: &mut ScopeState) {
    let types = scope.get(var_name).to_vec();
    if types.is_empty() {
        return;
    }

    let is_false = |t: &PhpType| matches!(t, PhpType::Named(n) if n == "false");

    let stripped: Vec<ResolvedType> = types
        .into_iter()
        .filter_map(|mut rt| {
            // Strip null
            let ty = match rt.type_string.non_null_type() {
                Some(non_null) => non_null,
                None if rt.type_string == PhpType::null() => return None,
                None => rt.type_string.clone(),
            };
            // Strip false
            if is_false(&ty) {
                return None;
            }
            let ty = match &ty {
                PhpType::Union(members) => {
                    let non_false: Vec<PhpType> =
                        members.iter().filter(|m| !is_false(m)).cloned().collect();
                    match non_false.len() {
                        0 => return None,
                        1 => non_false.into_iter().next().unwrap(),
                        _ => PhpType::Union(non_false),
                    }
                }
                _ => ty,
            };
            rt.type_string = ty;
            Some(rt)
        })
        .collect();

    if !stripped.is_empty() {
        scope.set(var_name, stripped);
    }
}

/// Split a single-level array access key like `$a["test"]` into base
/// variable and key name.  Returns `None` for non-array-access keys and
/// for multi-level access (`$a["x"]["y"]`), which this single-key
/// narrowing cannot represent and would otherwise mis-split.
fn split_array_access_key(key: &str) -> Option<(&str, &str)> {
    let bracket_pos = key.find("[\"")?;
    let base = &key[..bracket_pos];
    // The base must be a plain expression with no earlier array access.
    if base.contains('[') {
        return None;
    }
    let key_name = key[bracket_pos + 2..].strip_suffix("\"]")?;
    // A nested access leaves bracket characters inside the extracted key
    // (e.g. `x"]["y`); reject it rather than narrowing a bogus key.
    if key_name.contains('[') || key_name.contains(']') {
        return None;
    }
    Some((base, key_name))
}

/// Strip `null` from a specific array shape key on a variable.
///
/// Given variable `$a` typed as `array{test: ?int}` and key `"test"`,
/// rewrites the variable's type to `array{test: int}`.  This modifies
/// the base variable's type directly so the narrowed shape survives
/// scope merges (unlike synthetic scope entries which are stripped).
fn strip_null_from_array_shape_key(base_var: &str, key_name: &str, scope: &mut ScopeState) {
    let types = scope.get(base_var).to_vec();
    if types.is_empty() {
        return;
    }
    let narrowed: Vec<ResolvedType> = types
        .into_iter()
        .map(|mut rt| {
            rt.type_string = strip_null_from_shape_key(&rt.type_string, key_name);
            rt
        })
        .collect();
    scope.set(base_var, narrowed);
}

/// Recursively strip `null` from a specific key in an array shape type.
fn strip_null_from_shape_key(ty: &crate::php_type::PhpType, key: &str) -> crate::php_type::PhpType {
    use crate::php_type::{PhpType, ShapeEntry};
    match ty {
        PhpType::ArrayShape(entries) => {
            let new_entries: Vec<ShapeEntry> = entries
                .iter()
                .map(|e| {
                    if e.key.as_deref() == Some(key) {
                        let non_null = e
                            .value_type
                            .non_null_type()
                            .unwrap_or_else(|| e.value_type.clone());
                        ShapeEntry {
                            key: e.key.clone(),
                            value_type: non_null,
                            optional: false, // known to be present (was checked)
                        }
                    } else {
                        e.clone()
                    }
                })
                .collect();
            PhpType::ArrayShape(new_entries)
        }
        PhpType::Nullable(inner) => {
            // `?array{test: ?int}` → `?array{test: int}`
            PhpType::Nullable(Box::new(strip_null_from_shape_key(inner, key)))
        }
        PhpType::Union(members) => {
            let new_members: Vec<PhpType> = members
                .iter()
                .map(|m| strip_null_from_shape_key(m, key))
                .collect();
            PhpType::Union(new_members)
        }
        other => other.clone(),
    }
}

fn apply_guard_clause_null_narrowing<'b>(
    if_stmt: &'b If<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // When `if ($x === null) { return; }`, strip null from $x after.
    // When `if (!$x) { return; }`, strip null from $x after.
    if let Some(var_name) = extract_null_equality_check_var(if_stmt.condition) {
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
    if let Some(var_name) = extract_falsy_check_var(if_stmt.condition) {
        strip_falsy_from_scope(&var_name, scope);
    }
    // `if (!isset($x)) { return; }` — after the guard, $x is not null.
    for var_name in extract_not_isset_vars(if_stmt.condition) {
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        } else {
            seed_synthetic_key_if_needed(&var_name, scope, ctx);
            strip_null_from_scope(&var_name, scope);
        }
    }
    // `if ($x !== null)` with return doesn't narrow after — the
    // remaining code is the null path.  This is handled by the
    // inverse narrowing in the guard clause logic.
}

/// Process assignment in a condition: `if ($x = expr())`
fn process_condition_assignment<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Direct assignment: `if ($x = expr())`
    if let Expression::Assignment(assignment) = condition
        && assignment.operator.is_assign()
        && let Expression::Variable(Variable::Direct(dv)) = assignment.lhs
    {
        let var_name = bytes_to_str(dv.name).to_string();
        let rhs_types = resolve_rhs_with_scope(assignment.rhs, scope, ctx);
        if !rhs_types.is_empty() {
            scope.set(&var_name, rhs_types);
        }
        return;
    }
    // Parenthesized conditions: `if (($x = expr()))`
    if let Expression::Parenthesized(inner) = condition {
        process_condition_assignment(inner.expression, scope, ctx);
        return;
    }
    // Assignment inside a binary comparison:
    //   `if (($x = expr()) !== null)` or `if (null !== ($x = expr()))`
    if let Expression::Binary(bin) = condition {
        // Check LHS directly: `($x = expr()) !== null`
        if let Expression::Assignment(_) = bin.lhs {
            process_condition_assignment(bin.lhs, scope, ctx);
            return;
        }
        if let Expression::Parenthesized(p) = bin.lhs
            && let Expression::Assignment(_) = p.expression
        {
            process_condition_assignment(p.expression, scope, ctx);
            return;
        }
        // Check RHS: `null !== ($x = expr())`
        if let Expression::Assignment(_) = bin.rhs {
            process_condition_assignment(bin.rhs, scope, ctx);
            return;
        }
        if let Expression::Parenthesized(p) = bin.rhs
            && let Expression::Assignment(_) = p.expression
        {
            process_condition_assignment(p.expression, scope, ctx);
        }
    }
}

/// Extract variable names referenced in instanceof / is_a / get_class
/// conditions.  This catches variables that are not yet in scope but
/// are used in guard clauses like `if (!$x instanceof Foo) { return; }`.
fn collect_condition_var_names(expr: &Expression<'_>) -> Vec<String> {
    let mut names = Vec::new();
    collect_condition_var_names_inner(expr, &mut names);
    names
}

/// Remove synthetic property/array access keys from the scope.
/// Called after loop merges and other scope transitions where
/// condition-based narrowing no longer holds.
///
/// Synthetic keys contain `->` (property access) or `["` (array access).
fn strip_synthetic_property_keys(scope: &mut ScopeState) {
    scope
        .locals
        .retain(|key, _| !key.contains("->") && !key.contains("[\""));
}

/// Seed a synthetic scope entry for a compound key (property access
/// or array access) if it isn't already present.  Simple variable
/// names (no `->` or `["`) are skipped since they are already tracked.
fn seed_synthetic_key_if_needed(key: &str, scope: &mut ScopeState, ctx: &ForwardWalkCtx<'_>) {
    // Only seed compound keys (property access or array access).
    let is_property = key.contains("->");
    let is_array = key.contains("[\"");
    if !is_property && !is_array {
        return;
    }
    if scope.contains(key) {
        return;
    }

    if is_property {
        // Property access: delegate to existing seeding logic via a
        // one-key call (seed_property_keys_into_scope expects a
        // condition expression, but we already have the key).
        if let Some(arrow_pos) = key.rfind("->") {
            let obj_var = &key[..arrow_pos];
            let prop_name = &key[arrow_pos + 2..];
            let obj_types = scope.get(obj_var);
            if obj_types.is_empty() {
                return;
            }
            let mut prop_results: Vec<ResolvedType> = Vec::new();
            for rt in obj_types {
                if let Some(ref cls) = rt.class_info {
                    let type_hint = crate::inheritance::resolve_property_type_hint(
                        cls,
                        prop_name,
                        ctx.class_loader,
                    );
                    if let Some(hint) = type_hint {
                        let resolved_classes =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                &hint,
                                &ctx.current_class.name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if resolved_classes.is_empty() {
                            ResolvedType::extend_unique(
                                &mut prop_results,
                                vec![ResolvedType::from_type_string(hint)],
                            );
                        } else {
                            ResolvedType::extend_unique(
                                &mut prop_results,
                                ResolvedType::from_classes_with_hint(resolved_classes, hint),
                            );
                        }
                    }
                }
            }
            if !prop_results.is_empty() {
                scope.set(key, prop_results);
            }
        }
    } else if is_array {
        // Array access key: `$a["test"]`.
        // Extract the base variable and key name.
        if let Some(bracket_pos) = key.find("[\"") {
            let base_var = &key[..bracket_pos];
            let key_name = key[bracket_pos + 2..]
                .strip_suffix("\"]")
                .unwrap_or(&key[bracket_pos + 2..]);
            let base_types = scope.get(base_var);
            if base_types.is_empty() {
                return;
            }
            // Look up the array key's type from the array shape.
            let mut key_results: Vec<ResolvedType> = Vec::new();
            for rt in base_types {
                if let Some(element_type) = rt.type_string.extract_shape_key_type(key_name) {
                    let resolved_classes =
                        crate::completion::type_resolution::type_hint_to_classes_typed(
                            &element_type,
                            &ctx.current_class.name,
                            ctx.all_classes,
                            ctx.class_loader,
                        );
                    if resolved_classes.is_empty() {
                        ResolvedType::extend_unique(
                            &mut key_results,
                            vec![ResolvedType::from_type_string(element_type)],
                        );
                    } else {
                        ResolvedType::extend_unique(
                            &mut key_results,
                            ResolvedType::from_classes_with_hint(resolved_classes, element_type),
                        );
                    }
                }
            }
            if !key_results.is_empty() {
                scope.set(key, key_results);
            }
        }
    }
}

/// Collect property access keys (e.g. `$a->foo`) from conditions that
/// contain type guards or instanceof checks on property accesses.
/// These keys are injected into the scope so that narrowing applies.
fn collect_condition_property_keys(expr: &Expression<'_>) -> Vec<String> {
    let mut keys = Vec::new();
    collect_condition_property_keys_inner(expr, &mut keys);
    keys
}

fn collect_condition_property_keys_inner(expr: &Expression<'_>, keys: &mut Vec<String>) {
    match expr {
        // instanceof: `$a->foo instanceof Foo` or `$row["page"] instanceof Foo`
        Expression::Binary(bin) if bin.operator.is_instanceof() => {
            if let Some(key) = narrowing::expr_to_subject_key(bin.lhs)
                && (key.contains("->") || key.contains("[\""))
                && !keys.contains(&key)
            {
                keys.push(key);
            }
        }
        // Negation: `!is_string($a->foo)`, `!($a->foo instanceof Foo)`
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            collect_condition_property_keys_inner(prefix.operand, keys);
        }
        Expression::Parenthesized(p) => {
            collect_condition_property_keys_inner(p.expression, keys);
        }
        // Logical connectives
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_)
                    | BinaryOperator::LowAnd(_)
                    | BinaryOperator::Or(_)
                    | BinaryOperator::LowOr(_)
            ) =>
        {
            collect_condition_property_keys_inner(bin.lhs, keys);
            collect_condition_property_keys_inner(bin.rhs, keys);
        }
        // Type guard functions: `is_string($a->foo)`, `is_int($a->foo)`, etc.
        Expression::Call(Call::Function(func_call)) => {
            if let Expression::Identifier(ident) = func_call.function {
                let func_name = bytes_to_str(ident.value());
                let is_type_guard = matches!(
                    func_name,
                    "is_array"
                        | "is_string"
                        | "is_int"
                        | "is_integer"
                        | "is_long"
                        | "is_float"
                        | "is_double"
                        | "is_real"
                        | "is_bool"
                        | "is_object"
                        | "is_numeric"
                        | "is_callable"
                        | "is_null"
                        | "is_scalar"
                );
                if is_type_guard && let Some(first_arg) = func_call.argument_list.arguments.first()
                {
                    let arg_expr = match first_arg {
                        Argument::Positional(pos) => pos.value,
                        Argument::Named(named) => named.value,
                    };
                    if let Some(key) = narrowing::expr_to_subject_key(arg_expr)
                        && (key.contains("->") || key.contains("[\""))
                        && !keys.contains(&key)
                    {
                        keys.push(key);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Resolve the type of a property access key (e.g. `$a->foo`) from
/// the current scope and seed it into the scope as a synthetic entry.
/// This allows subsequent narrowing functions to find and narrow
/// property access expressions.
fn seed_property_keys_into_scope(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let keys = collect_condition_property_keys(condition);
    if keys.is_empty() {
        return;
    }
    for key in &keys {
        // Skip if already seeded (e.g. from a prior elseif condition).
        if scope.contains(key) {
            continue;
        }
        // Parse the key to extract object variable and property name.
        // Key format: `$var->prop` (possibly chained like `$a->b->c`).
        if let Some(arrow_pos) = key.rfind("->") {
            let obj_var = &key[..arrow_pos];
            let prop_name = &key[arrow_pos + 2..];

            // Resolve the object variable's type from scope.
            let obj_types = scope.get(obj_var);
            if obj_types.is_empty() {
                continue;
            }

            // Look up the property type on the resolved class(es).
            let mut prop_results: Vec<ResolvedType> = Vec::new();
            for rt in obj_types {
                if let Some(ref cls) = rt.class_info {
                    let type_hint = crate::inheritance::resolve_property_type_hint(
                        cls,
                        prop_name,
                        ctx.class_loader,
                    );
                    if let Some(hint) = type_hint {
                        let resolved_classes =
                            crate::completion::type_resolution::type_hint_to_classes_typed(
                                &hint,
                                &ctx.current_class.name,
                                ctx.all_classes,
                                ctx.class_loader,
                            );
                        if resolved_classes.is_empty() {
                            ResolvedType::extend_unique(
                                &mut prop_results,
                                vec![ResolvedType::from_type_string(hint)],
                            );
                        } else {
                            ResolvedType::extend_unique(
                                &mut prop_results,
                                ResolvedType::from_classes_with_hint(resolved_classes, hint),
                            );
                        }
                    }
                }
            }

            if !prop_results.is_empty() {
                scope.set(key, prop_results);
            }
        }
    }
}

fn collect_condition_var_names_inner(expr: &Expression<'_>, names: &mut Vec<String>) {
    match expr {
        Expression::Binary(bin) if bin.operator.is_instanceof() => {
            if let Expression::Variable(Variable::Direct(dv)) = bin.lhs {
                let name = bytes_to_str(dv.name).to_string();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            collect_condition_var_names_inner(prefix.operand, names);
        }
        Expression::Parenthesized(p) => {
            collect_condition_var_names_inner(p.expression, names);
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_)
                    | BinaryOperator::LowAnd(_)
                    | BinaryOperator::Or(_)
                    | BinaryOperator::LowOr(_)
            ) =>
        {
            collect_condition_var_names_inner(bin.lhs, names);
            collect_condition_var_names_inner(bin.rhs, names);
        }
        // is_a($var, ...) and get_class($var) === ...
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()),
                _ => return,
            };
            if (func_name == "is_a" || func_name == "get_class")
                && let Some(first_arg) = func_call.argument_list.arguments.first()
            {
                let arg_expr = match first_arg {
                    Argument::Positional(pos) => pos.value,
                    Argument::Named(named) => named.value,
                };
                if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
                    let name = bytes_to_str(dv.name).to_string();
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Check if a statement unconditionally exits (return/throw/continue/break).
fn statement_unconditionally_exits(stmt: &Statement<'_>) -> bool {
    narrowing::statement_unconditionally_exits(stmt)
}

/// Check whether a statement exits via `break` or `continue` (loop-local
/// exit) rather than `return` or `throw` (function exit).
///
/// When an if-branch exits via `break`/`continue`, the variable
/// assignments made in that branch still flow to the post-loop scope.
/// The if-merge should include these branch scopes in the surviving
/// set so that the merged post-if scope reflects the assignments.
fn exits_via_loop_control(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Break(_) | Statement::Continue(_) => true,
        Statement::Block(block) => block.statements.last().is_some_and(exits_via_loop_control),
        _ => false,
    }
}

// ─── Closure handling ───────────────────────────────────────────────────────

/// Try to enter a closure or arrow function if the cursor is inside one.
///
/// Returns `true` if the cursor was inside a closure and the scope was
/// updated accordingly.
fn try_enter_closure<'b>(
    stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    // Walk the statement's expression tree looking for closures/arrow
    // functions that contain the cursor.
    if let Statement::Expression(expr_stmt) = stmt {
        return try_enter_closure_expr(expr_stmt.expression, scope, ctx, None);
    }
    if let Statement::Return(ret) = stmt
        && let Some(val) = ret.value
    {
        return try_enter_closure_expr(val, scope, ctx, None);
    }
    // Closures/arrow functions can appear inside if/while/for/switch
    // conditions (e.g. `if (array_any($items, fn($x) => $x->...))`).
    // Recurse into these condition expressions so the forward walker
    // can enter the closure scope.
    if let Statement::If(if_stmt) = stmt {
        if try_enter_closure_expr(if_stmt.condition, scope, ctx, None) {
            return true;
        }
        // Also check elseif conditions and bodies for closures.
        match &if_stmt.body {
            IfBody::Statement(body) => {
                for ei in body.else_if_clauses.iter() {
                    if try_enter_closure_expr(ei.condition, scope, ctx, None) {
                        return true;
                    }
                }
            }
            IfBody::ColonDelimited(body) => {
                for ei in body.else_if_clauses.iter() {
                    if try_enter_closure_expr(ei.condition, scope, ctx, None) {
                        return true;
                    }
                }
            }
        }
    }
    if let Statement::While(while_stmt) = stmt
        && try_enter_closure_expr(while_stmt.condition, scope, ctx, None)
    {
        return true;
    }
    if let Statement::For(for_stmt) = stmt {
        for cond in for_stmt.conditions.iter() {
            if try_enter_closure_expr(cond, scope, ctx, None) {
                return true;
            }
        }
    }
    if let Statement::Switch(switch) = stmt {
        if try_enter_closure_expr(switch.expression, scope, ctx, None) {
            return true;
        }
        for case in switch.body.cases().iter() {
            if let Some(cond) = case.expression()
                && try_enter_closure_expr(cond, scope, ctx, None)
            {
                return true;
            }
        }
    }
    false
}

/// Recursively search an expression for a closure/arrow function
/// containing the cursor.
fn try_enter_closure_expr<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inferred_params: Option<&[PhpType]>,
) -> bool {
    match expr {
        Expression::Closure(closure) => {
            let body_span = closure.body.span();
            if ctx.cursor_offset >= body_span.start.offset
                && ctx.cursor_offset <= body_span.end.offset
            {
                // Create a fresh scope for the closure (closures have
                // isolated scope in PHP).
                let mut closure_scope = ScopeState::new();

                // PHP closures implicitly capture `$this` from the
                // enclosing class method.
                let this_types = scope.get("$this");
                if !this_types.is_empty() {
                    closure_scope.set("$this", this_types.to_vec());
                }

                // Seed with `use(...)` variables from the outer scope.
                if let Some(ref use_clause) = closure.use_clause {
                    for use_var in use_clause.variables.iter() {
                        let var_name = bytes_to_str(use_var.variable.name).to_string();
                        let from_outer = scope.get(&var_name);
                        if !from_outer.is_empty() {
                            closure_scope.set(&var_name, from_outer.to_vec());
                        }
                    }
                }

                // Seed with parameter types, using callable inference
                // when available (mirroring the diagnostic path's
                // seed_closure_params logic).
                let inferred = inferred_params.unwrap_or(&[]);
                let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
                seed_closure_params(
                    &mut closure_scope,
                    &closure.parameter_list,
                    closure.span().start.offset,
                    &filtered_inferred,
                    ctx,
                );

                // Walk the closure body.
                walk_body_forward(closure.body.statements.iter(), &mut closure_scope, ctx);

                // Replace the outer scope with the closure scope.
                *scope = closure_scope;
                return true;
            }
        }
        Expression::ArrowFunction(arrow) => {
            let body_span = arrow.expression.span();
            if ctx.cursor_offset >= body_span.start.offset
                && ctx.cursor_offset <= body_span.end.offset
            {
                // Arrow functions inherit the enclosing scope.
                // Seed with parameter types, using callable inference
                // when available.
                let inferred = inferred_params.unwrap_or(&[]);
                let filtered_inferred = filter_resolvable_inferred_params(inferred, ctx);
                seed_closure_params(
                    scope,
                    &arrow.parameter_list,
                    arrow.span().start.offset,
                    &filtered_inferred,
                    ctx,
                );
                // The body is a single expression.  Recurse into it
                // to find nested closures/arrow functions that may
                // contain the cursor (e.g. a closure passed as an
                // argument inside the arrow body).
                try_enter_closure_expr(arrow.expression, scope, ctx, None);
                return true;
            }
        }
        // Recurse into sub-expressions that might contain closures.
        Expression::Parenthesized(inner) => {
            return try_enter_closure_expr(inner.expression, scope, ctx, None);
        }
        Expression::Assignment(assignment) => {
            // Process the assignment first so the LHS var is in scope.
            process_assignment_expr(expr, scope, ctx);
            return try_enter_closure_expr(assignment.rhs, scope, ctx, None);
        }
        Expression::Call(call) => {
            // Check if any argument is a closure containing the cursor.
            // Infer callable parameter types from the function/method
            // signature so closure params get generic-substituted types
            // (mirroring the diagnostic path's walk_closures_in_call).
            let args = match call {
                Call::Function(fc) => &fc.argument_list,
                Call::Method(mc) => &mc.argument_list,
                Call::NullSafeMethod(mc) => &mc.argument_list,
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for (arg_idx, arg) in args.arguments.iter().enumerate() {
                let arg_expr = match arg {
                    Argument::Positional(a) => a.value,
                    Argument::Named(a) => a.value,
                };
                // Only infer callable params when the argument is a
                // closure or arrow function (or wraps one).
                let inferred = infer_callable_params_for_call(call, arg_idx, scope, ctx);
                let inferred_opt = if inferred.is_empty() {
                    None
                } else {
                    Some(inferred.as_slice())
                };
                if try_enter_closure_expr(arg_expr, scope, ctx, inferred_opt) {
                    return true;
                }
            }
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => {
                return try_enter_closure_expr(pa.object, scope, ctx, None);
            }
            Access::NullSafeProperty(pa) => {
                return try_enter_closure_expr(pa.object, scope, ctx, None);
            }
            _ => {}
        },
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                let elem_expr = match elem {
                    ArrayElement::KeyValue(kv) => kv.value,
                    ArrayElement::Value(val) => val.value,
                    ArrayElement::Variadic(v) => v.value,
                    ArrayElement::Missing(_) => continue,
                };
                if try_enter_closure_expr(elem_expr, scope, ctx, None) {
                    return true;
                }
            }
        }
        _ => {}
    }
    false
}

/// Infer callable parameter types for a specific argument index of a
/// call expression.  This reuses the same inference functions as the
/// diagnostic path (`infer_callable_params_from_function_fw`,
/// `infer_callable_params_from_receiver_fw`,
/// `infer_callable_params_from_static_receiver_fw`) so that closure
/// parameters on the completion/hover path receive the same
/// generic-substituted types.
fn infer_callable_params_for_call(
    call: &Call<'_>,
    arg_idx: usize,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    match call {
        Call::Function(fc) => {
            let func_name = match fc.function {
                Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                _ => None,
            };
            if let Some(ref name) = func_name {
                infer_callable_params_from_function_fw(
                    name,
                    arg_idx,
                    &fc.argument_list.arguments,
                    scope,
                    ctx,
                )
            } else {
                vec![]
            }
        }
        Call::Method(mc) => {
            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            if let Some(ref name) = method_name {
                let obj_span = mc.object.span();
                let first_arg =
                    extract_first_arg_string_fw(&mc.argument_list.arguments, ctx.content);
                infer_callable_params_from_receiver_fw(
                    obj_span.start.offset,
                    obj_span.end.offset,
                    name,
                    arg_idx,
                    first_arg.as_deref(),
                    scope,
                    ctx,
                )
            } else {
                vec![]
            }
        }
        Call::NullSafeMethod(mc) => {
            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            if let Some(ref name) = method_name {
                let obj_span = mc.object.span();
                let first_arg =
                    extract_first_arg_string_fw(&mc.argument_list.arguments, ctx.content);
                infer_callable_params_from_receiver_fw(
                    obj_span.start.offset,
                    obj_span.end.offset,
                    name,
                    arg_idx,
                    first_arg.as_deref(),
                    scope,
                    ctx,
                )
            } else {
                vec![]
            }
        }
        Call::StaticMethod(sc) => {
            let method_name = if let ClassLikeMemberSelector::Identifier(ident) = &sc.method {
                Some(bytes_to_str(ident.value).to_string())
            } else {
                None
            };
            if let Some(ref name) = method_name {
                let first_arg =
                    extract_first_arg_string_fw(&sc.argument_list.arguments, ctx.content);
                infer_callable_params_from_static_receiver_fw(
                    sc.class,
                    name,
                    arg_idx,
                    first_arg.as_deref(),
                    scope,
                    ctx,
                )
            } else {
                vec![]
            }
        }
    }
}

/// Widen a literal type to its base type (e.g. `1` → `int`, `'foo'` → `string`).
/// Non-literal types are returned unchanged.
fn widen_literal(ty: &PhpType) -> PhpType {
    match ty {
        PhpType::Literal(s) if s.parse::<i64>().is_ok() => PhpType::int(),
        PhpType::Literal(s)
            if (s.starts_with('\'') && s.ends_with('\''))
                || (s.starts_with('"') && s.ends_with('"')) =>
        {
            PhpType::string()
        }
        PhpType::Literal(s) if s.parse::<f64>().is_ok() => PhpType::float(),
        PhpType::Literal(s) if s == "true" || s == "false" => PhpType::bool(),
        _ => ty.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::split_array_access_key;

    #[test]
    fn splits_single_level_string_key() {
        assert_eq!(split_array_access_key("$a[\"test\"]"), Some(("$a", "test")));
    }

    #[test]
    fn rejects_non_array_access() {
        assert_eq!(split_array_access_key("$a"), None);
    }

    #[test]
    fn rejects_nested_array_access() {
        // `$a["x"]["y"]` must not be mis-split into base `$a` and key
        // `x"]["y`; single-key narrowing cannot represent it.
        assert_eq!(split_array_access_key("$a[\"x\"][\"y\"]"), None);
    }

    #[test]
    fn rejects_base_with_earlier_access() {
        assert_eq!(split_array_access_key("$a[0][\"y\"]"), None);
    }
}
