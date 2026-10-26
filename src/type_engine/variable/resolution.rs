//! Variable type resolution: the routing layer and its shared helpers.
//!
//! All variable type resolution is performed by the forward walker in
//! [`super::forward_walk`].  This module provides the entry points
//! ([`resolve_variable_types`]) that callers across the crate use, and
//! routes each query to the function, method, property hook, or top-level
//! code the cursor is in.  The helpers the forward walker delegates to
//! live in its submodules: the re-entry guards, answers for a cursor that
//! sits on a declaration, `@template` bounds and `@param` docblocks, and
//! what a call does with its arguments.

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{atom, bytes_to_str};
use crate::class_lookup::ResolvedClassLoader;
use crate::parser::with_parsed_program;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};

use crate::type_engine::resolver::{FileCtx, Loaders, VarResolutionCtx};

mod arguments;
mod cursor_declarations;
mod reentry;
mod template_bounds;

pub(crate) use arguments::*;
use cursor_declarations::*;
use reentry::*;
#[cfg(test)]
pub(crate) use reentry::{reset_test_scope_cache_hits, test_scope_cache_hits};
pub(crate) use template_bounds::*;

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
            ctx.cursor_offset,
            ctx,
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
    class_loader: &ResolvedClassLoader<'_>,
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
        vec![PhpType::named(current_class.fqn())],
    ))
}

/// A question for the forward walker: the type of `var_name` at
/// `cursor_offset` in `content`, read inside `current_class` (outside a
/// class, inside a placeholder for the namespace there), with the loaders
/// that resolve the names the walk meets.
///
/// The walk builds its own [`VarResolutionCtx`] from these, so none of a
/// caller's forward-walk state (scope resolvers, branch narrowing, …)
/// reaches it.  [`VarResolutionCtx::var_query`] and
/// [`ResolutionCtx::var_query`](crate::type_engine::resolver::ResolutionCtx::var_query)
/// project one out of the contexts that already hold the inputs.
#[derive(Clone, Copy)]
pub(crate) struct VarQuery<'a> {
    /// The file the variable is read in.
    pub file: FileCtx<'a>,
    pub var_name: &'a str,
    pub cursor_offset: u32,
    pub current_class: Option<&'a ClassInfo>,
    pub loaders: Loaders<'a>,
}

impl<'a> VarQuery<'a> {
    /// A query for `var_name` at `cursor_offset` of `file`, read inside
    /// `current_class`, with no loaders beyond the file's.
    pub(crate) fn new(
        var_name: &'a str,
        file: FileCtx<'a>,
        current_class: Option<&'a ClassInfo>,
        cursor_offset: u32,
    ) -> Self {
        Self {
            file,
            var_name,
            cursor_offset,
            current_class,
            loaders: Loaders::default(),
        }
    }

    /// The forward walker's context for this query, read inside
    /// `current_class`.
    fn var_ctx<'q>(&'q self, current_class: &'q ClassInfo) -> VarResolutionCtx<'q> {
        VarResolutionCtx {
            loaders: self.loaders,
            resolved_class_cache: crate::virtual_members::active_resolved_class_cache(),
            ..VarResolutionCtx::new(self.var_name, self.file, current_class, self.cursor_offset)
        }
    }
}

/// The class `query` is read inside: its own, or a placeholder for the
/// namespace at the cursor, built in `slot`.
fn query_class<'q>(query: &VarQuery<'q>, slot: &'q mut Option<ClassInfo>) -> &'q ClassInfo {
    match query.current_class {
        Some(cls) => cls,
        None => slot.insert(crate::class_lookup::class_context_placeholder(
            query.file.content,
            query.cursor_offset,
        )),
    }
}

/// Resolve the type of `$variable` at `cursor_offset`.
///
/// Checks the diagnostic scope cache first (O(log N) lookup from the
/// forward walker's pre-computed snapshots).  On cache miss, parses the
/// file and delegates to the forward walker via
/// [`resolve_variable_in_statements`].
pub(crate) fn resolve_variable_types(query: VarQuery<'_>) -> Vec<ResolvedType> {
    let VarQuery {
        var_name,
        cursor_offset,
        ..
    } = query;
    let content = query.file.content;

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

    let mut placeholder = None;
    let current_class = query_class(&query, &mut placeholder);
    let key = var_query_key(content, var_name, cursor_offset, current_class.name);

    // The walk below is shared by every name asked about at the same
    // position in one request (the forward walker's `WALK_MEMO`), so a
    // repeated question costs a lookup into the scope it reached.

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

    with_parsed_program(content, "resolve_variable_types", |program, _content| {
        resolve_variable_in_statements(program.statements.iter(), &query.var_ctx(current_class))
    })
}

/// The forward walk behind [`resolve_variable_types`], over a program the
/// caller has already parsed.
///
/// For a caller that needs the parse for something else first (finding
/// the offset to resolve at, say) and would otherwise parse the file
/// twice. It bypasses the per-pass memo and the diagnostic scope cache,
/// both of which key on offsets into the request's own file.
pub(crate) fn resolve_variable_types_in_program(
    program: &Program<'_>,
    query: VarQuery<'_>,
) -> Vec<ResolvedType> {
    let mut placeholder = None;
    let current_class = query_class(&query, &mut placeholder);
    resolve_variable_in_statements(program.statements.iter(), &query.var_ctx(current_class))
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
pub(crate) fn resolve_variable_php_type(query: VarQuery<'_>) -> Option<PhpType> {
    let VarQuery {
        var_name,
        cursor_offset,
        file,
        ..
    } = query;
    let FileCtx {
        content,
        class_loader,
        source_class_loader,
        ..
    } = file;
    let prefixed = if var_name.starts_with('$') {
        var_name.to_owned()
    } else {
        format!("${}", var_name)
    };

    let written_names = || file.written_names(query.current_class, cursor_offset);

    // 1. Forward walker resolution.
    let mut placeholder = None;
    let effective_class = query_class(&query, &mut placeholder);

    let resolved = resolve_variable_types(VarQuery {
        var_name: &prefixed,
        current_class: Some(effective_class),
        ..query
    });

    if !resolved.is_empty() {
        return Some(ResolvedType::types_joined(&resolved));
    }

    // 2. Parameter definition site fallback.
    //    When the cursor is on a parameter declaration (inside the
    //    parameter list, not the body), the forward walker won't find
    //    it because it only processes body statements.  Parse the file
    //    and check if the cursor is on a parameter with a type hint.
    let param_type = with_parsed_program(content, "resolve_var_param_site", |program, _| {
        let stmts: Vec<&Statement> = program.statements.iter().collect();
        find_param_at_cursor(&stmts, &prefixed, cursor_offset, content)
            .and_then(|site| {
                site.settled_type(effective_class, content, class_loader, source_class_loader)
            })
            .or_else(|| find_catch_var_type_at_cursor(&stmts, &prefixed, cursor_offset))
    });
    param_type.map(|written| written_names().resolve_type(&written))
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
    let file_has_global_keyword = ctx.file.content.contains("global ");
    let top_level_scope = if ctx.top_level_scope.is_none() && file_has_global_keyword {
        // Guard 1: prevent re-entrant top-level scope construction for
        // the same file.  RHS resolution during the walk can trigger
        // another resolve_variable_types call, which would start a
        // second top-level walk on the same file content.
        if let Some(_tl_guard) = try_acquire_top_level_guard(ctx.file.content) {
            let flow = super::forward_walk::WalkFlow::default();
            let tl_fw_ctx = super::forward_walk::ForwardWalkCtx {
                cursor_offset: u32::MAX,
                ..super::forward_walk::ForwardWalkCtx::fresh_from_var_ctx(ctx, &flow)
            };
            let tl_scope =
                super::forward_walk::top_level_scope_for_globals(stmts.iter().copied(), &tl_fw_ctx);
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
            top_level_scope,
            ..ctx.clone()
        };
        &ctx_with_tls
    } else {
        ctx
    };

    for &stmt in &stmts {
        match stmt {
            _ if let Some(body) = crate::parser::class_like_body(stmt) => {
                if !body.contains(ctx.cursor_offset) {
                    continue;
                }
                // The cursor is inside this class body.  PHP method
                // scopes are isolated — they cannot access variables
                // from enclosing or top-level code.  Return whatever
                // the member scan found (even if empty, e.g. after
                // `unset($var)`), and never fall through to the
                // top-level walk.
                return resolve_variable_in_members(body.members.iter(), ctx);
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
        if let Some(anon) = anonymous_class_body_at(stmt, ctx.cursor_offset) {
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
        let flow = super::forward_walk::WalkFlow::default();
        let fw_ctx = super::forward_walk::ForwardWalkCtx::fresh_from_var_ctx(ctx, &flow);
        if let Some(fw_results) =
            super::forward_walk::resolve_in_top_level(ctx.var_name, stmts.iter().copied(), &fw_ctx)
        {
            return fw_results;
        }
    }

    vec![]
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

    // ── Forward walker (sole resolver) ──
    let flow = super::forward_walk::WalkFlow::default();
    let fw_ctx = super::forward_walk::ForwardWalkCtx {
        top_level_scope: ctx.top_level_scope.clone(),
        ..super::forward_walk::ForwardWalkCtx::fresh_from_var_ctx(ctx, &flow)
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
        _ => crate::parser::find_map_nested_statement(stmt, |inner| {
            try_resolve_in_nested_function(inner, ctx)
        }),
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
                    let flow = super::forward_walk::WalkFlow::default();
                    let fw_ctx = super::forward_walk::ForwardWalkCtx {
                        top_level_scope: ctx.top_level_scope.clone(),
                        ..super::forward_walk::ForwardWalkCtx::fresh_from_var_ctx(ctx, &flow)
                    };
                    return super::forward_walk::resolve_in_method_body(
                        ctx.var_name,
                        method.parameter_list.parameters.iter(),
                        block.statements.iter(),
                        method.span().start.offset,
                        super::forward_walk::BodyOwner::method(method),
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

        let flow = super::forward_walk::WalkFlow::default();
        let fw_ctx = super::forward_walk::ForwardWalkCtx {
            top_level_scope: ctx.top_level_scope.clone(),
            ..super::forward_walk::ForwardWalkCtx::fresh_from_var_ctx(ctx, &flow)
        };

        let mut scope = super::forward_walk::seed_property_hook_scope(property_hint, hook, &fw_ctx);
        if let PropertyHookConcreteBody::Block(block) = body {
            // Suspend snapshot recording for the same reason
            // `resolve_in_method_body` does: this is a transient lookup,
            // not the authoritative scope build.
            let _suspend = super::forward_walk::suspend_snapshot_recording();
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
    let owner = super::forward_walk::BodyOwner::method(method);

    for param in method.parameter_list.parameters.iter() {
        let pname = bytes_to_str(param.variable.name);
        if pname != ctx.var_name {
            continue;
        }

        let flow = super::forward_walk::WalkFlow::default();
        let fw_ctx = super::forward_walk::ForwardWalkCtx {
            top_level_scope: ctx.top_level_scope.clone(),
            ..super::forward_walk::ForwardWalkCtx::fresh_from_var_ctx(ctx, &flow)
        };

        let trait_prototype =
            super::forward_walk::trait_prototype_method(owner.method_name, &fw_ctx);

        return super::forward_walk::resolve_param_type(
            pname,
            param,
            &super::forward_walk::EnclosingMethod {
                span_start: method.span().start.offset,
                name: owner.method_name,
                has_scope_attr: owner.has_scope_attr,
                trait_prototype: trait_prototype.as_ref(),
            },
            &fw_ctx,
        );
    }

    vec![]
}

#[cfg(test)]
#[path = "resolution_tests.rs"]
mod tests;

/// The innermost anonymous class in `stmt` whose body (between `{` and
/// `}`) holds `cursor_offset`.
fn anonymous_class_body_at<'a>(
    stmt: &'a Statement<'a>,
    cursor_offset: u32,
) -> Option<&'a AnonymousClass<'a>> {
    crate::parser::innermost_node_at(Node::Statement(stmt), cursor_offset, |node| match node {
        Node::AnonymousClass(anon)
            if cursor_offset >= anon.left_brace.start.offset
                && cursor_offset <= anon.right_brace.end.offset =>
        {
            Some(anon)
        }
        _ => None,
    })
}
