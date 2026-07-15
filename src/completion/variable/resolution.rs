/// Variable type resolution — routing layer and shared helpers.
///
/// All variable type resolution is performed by the forward walker in
/// [`super::forward_walk`].  This module provides the public entry
/// points ([`resolve_variable_types`]) that callers across the crate
/// use, a diagnostic-scope-cache fast path, and shared helper functions
/// (template substitution, array shape merging, pass-by-reference
/// seeding, abstract-method parameter resolution) that the forward
/// walker delegates to.
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{atom, bytes_to_str, last_segment};
use crate::docblock;
use crate::parser::{extract_hint_type, with_parsed_program};
use crate::php_type::{PhpType, ShapeEntry, is_keyword_type};
use crate::types::{ClassInfo, ParameterInfo, ResolvedType};

use crate::completion::resolver::{Loaders, VarResolutionCtx};

/// Build a [`VarClassStringResolver`] closure from a [`VarResolutionCtx`].
///
/// The returned closure resolves a variable name (e.g. `"$requestType"`)
/// to the class names it holds as class-string values by delegating to
/// [`resolve_class_string_targets`](super::class_string_resolution::resolve_class_string_targets).
pub(in crate::completion) fn build_var_resolver_from_ctx<'a>(
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
        )
        .iter()
        .map(|c| c.name.to_string())
        .collect()
    }
}

/// Check whether a type hint should be enriched with generic args for
/// Eloquent scope method Builder parameters.
///
/// When `type_str` resolves to `Builder` (the Eloquent Builder, without
/// generic parameters) and the enclosing method is a scope on a class
/// that extends Eloquent Model, returns a `PhpType::Generic` wrapping
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

    // Only applies inside scope methods: either the scopeX naming
    // convention or the #[Scope] attribute.
    let is_convention_scope = method_name.starts_with("scope") && method_name.len() > 5;
    if !is_convention_scope && !has_scope_attr {
        return None;
    }

    // Only applies when the enclosing class extends Eloquent Model.
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
    let type_name = match type_hint {
        PhpType::Named(n) => n.as_str(),
        _ => return None,
    };
    let is_eloquent_builder = type_name == ELOQUENT_BUILDER_FQN || type_name == "Builder";
    if !is_eloquent_builder {
        return None;
    }

    // Build the enriched type with the enclosing model as the generic arg.
    Some(PhpType::Generic(
        type_name.to_string(),
        vec![PhpType::Named(current_class.name.to_string())],
    ))
}

/// Resolve the type of `$variable` at `cursor_offset`.
///
/// Checks the diagnostic scope cache first (O(log N) lookup from the
/// forward walker's pre-computed snapshots).  On cache miss, parses the
/// file and delegates to the forward walker via
/// [`resolve_variable_in_statements`].
pub(crate) fn resolve_variable_types(
    var_name: &str,
    current_class: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    content: &str,
    cursor_offset: u32,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
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
            return types;
        }
        // Variable not in the forward-walked scope — fall through to
        // the full resolution path.
    }

    with_parsed_program(content, "resolve_variable_types", |program, _content| {
        let active_cache = crate::virtual_members::active_resolved_class_cache();
        let ctx = VarResolutionCtx {
            var_name,
            current_class,
            all_classes,
            content,
            cursor_offset,
            class_loader,
            loaders,
            resolved_class_cache: active_cache,
            enclosing_return_type: None,
            top_level_scope: None,
            branch_aware: false,
            match_arm_narrowing: HashMap::new(),

            scope_var_resolver: None,
        };

        resolve_variable_in_statements(program.statements.iter(), &ctx)
    })
}

/// Resolve the type of a variable at `cursor_offset` as a [`PhpType`].
///
/// This is the **single entry point** for all consumers that need to
/// answer "what is the type of `$var` at this offset?"  It wraps the
/// forward walker ([`resolve_variable_types`]) and converts the result
/// to a `PhpType`, incorporating:
///
/// - Inline `/** @var Type $var */` docblock overrides (unless the
///   cursor is inside the RHS of a self-referential assignment).
/// - The forward walker's branch-aware narrowing.
/// - Proper preference logic: the forward walker result wins when it
///   applies narrowing (e.g. array shape key null-stripping through
///   guard clauses); the `@var` override wins otherwise.
///
/// Consumers: hover, go-to-type-definition, find-references (variable
/// subject resolution), deprecated diagnostics, code actions.
pub(crate) fn resolve_variable_php_type(
    var_name: &str,
    content: &str,
    cursor_offset: u32,
    current_class: Option<&ClassInfo>,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
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
            dummy_class = ClassInfo::default();
            &dummy_class
        }
    };

    // Activate the hover scope cache when NOT inside a self-assignment RHS.
    if !is_cursor_in_self_assignment_rhs(content, cursor_offset as usize, &prefixed) {
        super::forward_walk::activate_hover_scope_cache(content);
    }

    let resolved = resolve_variable_types(
        &prefixed,
        effective_class,
        all_classes,
        content,
        cursor_offset,
        class_loader,
        loaders,
    );

    if !resolved.is_empty() {
        let joined = ResolvedType::types_joined(&resolved);

        // When the forward walk produced a result and we have a @var
        // override, prefer the forward walk when it narrowed the type
        // (shape entries with condition-based null stripping).
        if let Some(ref vo) = var_override {
            if !vo.equivalent(&joined) && vo.shape_entries().is_some() {
                return Some(joined);
            }
            return Some(vo.clone());
        }

        return Some(joined);
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
pub(in crate::completion) fn resolve_variable_in_statements<'b>(
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
        let tl_fw_ctx = super::forward_walk::ForwardWalkCtx {
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: u32::MAX,
            class_loader: ctx.class_loader,
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
        ctx.top_level_scope.clone()
    };

    // Shadow ctx with one that carries the top-level scope, reusing
    // the existing `with_cursor_offset` helper to copy all fields.
    let ctx_with_tls;
    let ctx: &VarResolutionCtx<'_> = if top_level_scope.is_some() && ctx.top_level_scope.is_none() {
        ctx_with_tls = VarResolutionCtx {
            var_name: ctx.var_name,
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope,
            branch_aware: ctx.branch_aware,
            match_arm_narrowing: ctx.match_arm_narrowing.clone(),
            scope_var_resolver: ctx.scope_var_resolver,
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

/// Locate the enclosing method for the cursor position and delegate to
/// the forward walker.  For abstract methods (no body), returns the
/// parameter type hint directly.
fn resolve_variable_in_members<'b>(
    members: impl Iterator<Item = &'b ClassLikeMember<'b>>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    for member in members {
        if let ClassLikeMember::Method(method) = member {
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
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: None,
            top_level_scope: ctx.top_level_scope.clone(),
        };

        return super::forward_walk::resolve_param_type(
            pname,
            native_type.as_ref(),
            is_variadic,
            method.span().start.offset,
            Some(&method_name_str),
            has_scope_attr,
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
    match ty {
        PhpType::Named(name) => {
            // Well-known scalars/pseudo-types are never template params.
            !is_keyword_type(name)
        }
        PhpType::Union(members) | PhpType::Intersection(members) => {
            members.iter().any(type_may_contain_template_param)
        }
        PhpType::Nullable(inner) => type_may_contain_template_param(inner),
        PhpType::Generic(base, args) => {
            !crate::php_type::is_keyword_type(base)
                || args.iter().any(type_may_contain_template_param)
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
    let inner_name = match &ty {
        PhpType::ClassString(Some(inner)) => match inner.as_ref() {
            PhpType::Named(name) => Some(name.clone()),
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
            return PhpType::ClassString(Some(Box::new(bound_type)));
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
                Some(PhpType::Named(fqn))
            }
            Expression::Self_(_) => Some(PhpType::Named(ctx.current_class.name.to_string())),
            Expression::Static(_) => Some(PhpType::Named(ctx.current_class.name.to_string())),
            _ => None,
        },
        // Function / method calls → look up the return type.
        Expression::Call(call) => match call {
            Call::Function(func_call) => {
                let func_name = match func_call.function {
                    Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                    _ => None,
                };
                func_name.and_then(|name| {
                    ctx.function_loader()
                        .and_then(|fl| fl(&name))
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

/// Merge a chain of string keys into a (possibly nested) array shape.
///
/// For keys `["a", "b"]` and value type `string`, produces:
///   `array{a: array{b: string}}`
///
/// When the base already contains entries, they are preserved and the
/// nested key path is merged in.  For example, merging `["a", "c"]`
/// with value `int` into `array{a: array{b: string}}` produces:
///   `array{a: array{b: string, c: int}}`
pub(super) fn merge_nested_shape_keys(
    base: &PhpType,
    keys: &[String],
    value_type: &PhpType,
) -> PhpType {
    debug_assert!(!keys.is_empty());
    if keys.len() == 1 {
        return merge_shape_key(base, &keys[0], value_type);
    }

    // For nested keys, we need to:
    // 1. Look up the existing inner type for the first key
    // 2. Recursively merge the remaining keys into that inner type
    // 3. Merge the result back at the first key level
    let first_key = &keys[0];
    let inner_base = base
        .shape_value_type(first_key)
        .cloned()
        .unwrap_or_else(PhpType::array);
    let inner_merged = merge_nested_shape_keys(&inner_base, &keys[1..], value_type);
    merge_shape_key(base, first_key, &inner_merged)
}

/// A single key segment in a (possibly nested) array write like
/// `$var['a'][$i]['b'] = …`.
pub(super) enum ArrayWriteKey {
    /// A string-literal key tracked as a shape entry, e.g. `['name']`.
    Shape(String),
    /// A dynamic (variable / expression / numeric) key tracked as a
    /// generic `array<K, V>` level. Carries the inferred key type.
    Keyed(PhpType),
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
/// A dynamic write onto an existing non-empty shape leaves the shape
/// unchanged (the literal keys already tracked are worth more than a
/// widened `array<K, V>`), matching the single-segment behaviour.
pub(super) fn merge_nested_array_write(
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
                let inner_base = base
                    .shape_value_type(key)
                    .cloned()
                    .unwrap_or_else(PhpType::array);
                let inner_merged = merge_nested_array_write(&inner_base, &keys[1..], value_type);
                merge_shape_key(base, key, &inner_merged)
            }
        }
        ArrayWriteKey::Keyed(key_type) => {
            // Preserve an existing non-empty shape rather than widening
            // it to a generic array and losing its literal keys.
            if base.shape_entries().is_some_and(|e| !e.is_empty()) {
                return base.clone();
            }
            let inner_merged = if keys.len() == 1 {
                value_type.clone()
            } else {
                let inner_base = base.iterable_element_type().unwrap_or_else(PhpType::array);
                merge_nested_array_write(&inner_base, &keys[1..], value_type)
            };
            merge_keyed_type(base, key_type, &inner_merged)
        }
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
        let key = s
            .value
            .map(|v| bytes_to_str(v).to_string())
            .unwrap_or_else(|| {
                crate::util::unquote_php_string(bytes_to_str(s.raw))
                    .unwrap_or(bytes_to_str(s.raw))
                    .to_string()
            });
        // Skip numeric-only keys — they are positional, not shape entries.
        if key.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some(key)
    } else {
        None
    }
}

/// Merge a `(key, value_type)` pair into an existing `PhpType` to
/// produce an `ArrayShape`.
///
/// If `base` is already an `ArrayShape`, the key is added or updated.
/// Otherwise a new shape is created with just the given key.
///
/// Returns `PhpType::ArrayShape(entries)` with the merged entries.
fn merge_shape_key(base: &PhpType, key: &str, value_type: &PhpType) -> PhpType {
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
        value_type: value_type.clone(),
        optional: false,
    });

    PhpType::ArrayShape(entries)
}

/// Merge a push element type into an existing `PhpType` to produce
/// a `Generic("list", …)` type.
///
/// If `base` already has a generic value type (e.g. `list<User>`),
/// the new type is unioned with it (e.g. `list<User|Admin>`).
/// Otherwise, produces `list<value_type>`.
///
/// Returns `PhpType::list(elem_type)` or
/// `PhpType::Named("array")` when no element types are available.
pub(super) fn merge_push_type(base: &PhpType, value_type: &PhpType) -> PhpType {
    let mut elem_types: Vec<PhpType> = Vec::new();

    // Extract existing element types from the base.
    if let Some(existing_elem) = base.extract_element_type() {
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

    let elem_type = if elem_types.len() == 1 {
        elem_types.into_iter().next().unwrap()
    } else {
        PhpType::Union(elem_types)
    };

    PhpType::list(elem_type)
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
/// available, or `PhpType::Named("array")` when no element types
/// are available.
pub(super) fn merge_keyed_type(
    base: &PhpType,
    key_type: &PhpType,
    value_type: &PhpType,
) -> PhpType {
    // Collect existing key types from the base.
    let mut key_types: Vec<PhpType> = Vec::new();
    if let Some(existing_key) = base.extract_key_type(false)
        && !existing_key.is_empty()
    {
        key_types.push(existing_key.clone());
    }
    // Add new key type members.
    for member in key_type.union_members() {
        if !member.is_empty() && !key_types.iter().any(|e| e.equivalent(member)) {
            key_types.push(member.clone());
        }
    }

    // Collect existing value types from the base.
    let mut elem_types: Vec<PhpType> = Vec::new();
    if let Some(existing_elem) = base.extract_element_type() {
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

    let val_type = if elem_types.len() == 1 {
        elem_types.into_iter().next().unwrap()
    } else {
        PhpType::Union(elem_types)
    };

    if key_types.is_empty() {
        // No key type information — use a single-param generic.
        PhpType::generic_array_val(val_type)
    } else {
        let k_type = if key_types.len() == 1 {
            key_types.into_iter().next().unwrap()
        } else {
            PhpType::Union(key_types)
        };
        PhpType::generic_array(k_type, val_type)
    }
}

/// Infer the key type of an array-access index expression.
///
/// Returns `"string"` for expressions that are known to produce
/// strings (string literals, method calls returning `string`, string
/// variables), `"int"` for integer expressions, and `"int|string"`
/// when the type cannot be determined.
pub(super) fn infer_array_key_type(index: &Expression<'_>, ctx: &VarResolutionCtx<'_>) -> PhpType {
    // Fast path: literal values.
    match index {
        Expression::Literal(Literal::Integer(_)) => return PhpType::int(),
        Expression::Literal(Literal::String(_)) => return PhpType::string(),
        _ => {}
    }

    // Resolve the expression type through the standard pipeline.
    let resolved = super::rhs_resolution::resolve_rhs_expression(index, ctx);
    if !resolved.is_empty() {
        let joined = ResolvedType::types_joined(&resolved);
        // Normalise the resolved type to a valid array key type.
        // PHP array keys are always int or string; bool and null are
        // coerced to int, float is truncated to int.
        if is_int_like_key_typed(&joined) {
            return PhpType::int();
        }
        if is_string_like_key(&joined) {
            return PhpType::string();
        }
        if joined.is_mixed() || is_array_key_type(&joined) {
            return PhpType::Union(vec![PhpType::int(), PhpType::string()]);
        }
        // For anything else (e.g. a class-string<T>, or a union),
        // return as-is if it is composed entirely of int/string
        // subtypes; otherwise fall back.
        return joined;
    }

    PhpType::Union(vec![PhpType::int(), PhpType::string()])
}

/// Returns `true` when the [`PhpType`] represents a PHP type that
/// is always coerced to `int` when used as an array key.
fn is_int_like_key_typed(ty: &PhpType) -> bool {
    ty.is_int_coercible_key()
}

/// Returns `true` when the [`PhpType`] represents a string-like
/// array key type.
fn is_string_like_key(ty: &PhpType) -> bool {
    ty.is_string_subtype()
}

/// Returns `true` when the [`PhpType`] is `array-key` or the
/// equivalent `int|string` union.
fn is_array_key_type(ty: &PhpType) -> bool {
    if ty.is_array_key() {
        return true;
    }
    match ty {
        PhpType::Union(members) if members.len() == 2 => {
            let has_int = members.iter().any(|m| m.is_int());
            let has_string = members.iter().any(|m| m.is_string_type());
            has_int && has_string
        }
        _ => false,
    }
}

// ── Array function type preservation helpers ─────────────────────────

/// Extract the first positional argument expression from an
/// argument list.
pub(in crate::completion) fn first_arg_expr<'b>(
    args: &'b ArgumentList<'b>,
) -> Option<&'b Expression<'b>> {
    args.arguments.first().map(|arg| match arg {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    })
}

/// Extract the nth positional argument expression (0-based).
pub(in crate::completion) fn nth_arg_expr<'b>(
    args: &'b ArgumentList<'b>,
    n: usize,
) -> Option<&'b Expression<'b>> {
    args.arguments.iter().nth(n).map(|arg| match arg {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    })
}

/// Resolve the raw iterable type of an argument expression.
///
/// Handles `$variable` (via docblock scanning) and delegates to
/// `resolve_expression_type` for method calls, property access,
/// etc.
pub(in crate::completion) fn resolve_arg_raw_type<'b>(
    arg_expr: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    // Direct variable — scan for @var / @param annotation.
    if let Expression::Variable(Variable::Direct(dv)) = arg_expr {
        let var_text = bytes_to_str(dv.name).to_string();
        let offset = arg_expr.span().start.offset as usize;
        let from_docblock =
            docblock::find_iterable_raw_type_in_source(ctx.content, offset, &var_text)
                .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));
        if let Some(raw) = from_docblock {
            return Some(raw);
        }

        // No docblock — resolve the variable's type to extract the
        // raw iterable type.  This handles cases like
        // `$users = $this->getUsers(); array_pop($users)` where
        // `$users` has no `@var` annotation but was assigned from a
        // method returning `list<User>`.
        //
        // When a scope_var_resolver is available (forward walker is
        // active), read from the in-progress ScopeState.  Falling
        // through to resolve_variable_types would re-enter the
        // forward walker, causing infinite recursion on patterns
        // like `$a['k'] = f($a['k'])`.
        let resolved = if let Some(resolver) = ctx.scope_var_resolver {
            let prefixed = if var_text.starts_with('$') {
                var_text.clone()
            } else {
                format!("${}", var_text)
            };
            resolver(&prefixed)
        } else {
            resolve_variable_types(
                &var_text,
                ctx.current_class,
                ctx.all_classes,
                ctx.content,
                offset as u32,
                ctx.class_loader,
                Loaders::with_function(ctx.function_loader()),
            )
        };
        if !resolved.is_empty() {
            let joined = crate::types::ResolvedType::types_joined(&resolved);
            if joined.extract_value_type(true).is_some() {
                return Some(joined);
            }
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
/// Currently handles standalone function calls (via `function_loader`).
/// Handles standalone function calls, instance method calls, static
/// method calls, and constructor calls.
pub(super) fn try_apply_pass_by_reference_type(
    expr: &Expression<'_>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ResolvedType>,
    conditional: bool,
) {
    let (argument_list, parameters) = match expr {
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
                _ => return,
            };
            let fl = match ctx.function_loader() {
                Some(fl) => fl,
                None => return,
            };
            let func_info = match fl(&func_name) {
                Some(fi) => fi,
                None => return,
            };
            // Borrow the argument list and clone the parameters so we
            // can iterate them together.
            (&func_call.argument_list, func_info.parameters)
        }
        Expression::Call(Call::Method(method_call)) => {
            match try_resolve_method_params(method_call.object, &method_call.method, ctx) {
                Some((params,)) => (&method_call.argument_list, params),
                None => return,
            }
        }
        Expression::Call(Call::NullSafeMethod(method_call)) => {
            match try_resolve_method_params(method_call.object, &method_call.method, ctx) {
                Some((params,)) => (&method_call.argument_list, params),
                None => return,
            }
        }
        Expression::Call(Call::StaticMethod(static_call)) => {
            match try_resolve_static_method_params(static_call, ctx) {
                Some((params, arg_list)) => (arg_list, params),
                None => return,
            }
        }
        Expression::Instantiation(inst) => match try_resolve_constructor_params(inst, ctx) {
            Some((params, arg_list)) => (arg_list, params),
            None => return,
        },
        _ => return,
    };

    // Bind arguments to parameters following PHP's rules so that named
    // arguments consult the parameter they actually target, not the one at
    // their ordinal position in the call.
    let bound = crate::call_args::bind_args_to_params(&parameters, argument_list);

    for (param, arg_expr) in parameters.iter().zip(bound.iter()) {
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
            && let Some(type_hint) = &param.type_hint
        {
            let resolved = crate::completion::type_resolution::type_hint_to_classes_typed(
                type_hint,
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
                    ResolvedType::from_classes_with_hint(resolved, type_hint.clone()),
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
) -> Option<(Vec<ParameterInfo>,)> {
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
    Some((method_info.parameters.clone(),))
}

/// Resolve parameters for a static method call.
fn try_resolve_static_method_params<'a>(
    static_call: &'a StaticMethodCall<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<(Vec<ParameterInfo>, &'a ArgumentList<'a>)> {
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
    Some((method_info.parameters.clone(), &static_call.argument_list))
}

/// Resolve parameters for a constructor call (`new Cls(...)`).
fn try_resolve_constructor_params<'a>(
    inst: &'a Instantiation<'a>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<(Vec<ParameterInfo>, &'a ArgumentList<'a>)> {
    let class_name = match inst.class {
        Expression::Identifier(ident) => bytes_to_str(ident.value()).to_string(),
        Expression::Self_(_) | Expression::Static(_) => ctx.current_class.name.to_string(),
        Expression::Parent(_) => ctx.current_class.parent_class.map(|a| a.to_string())?,
        _ => return None,
    };

    let args = inst.argument_list.as_ref()?;
    let cls = (ctx.class_loader)(&class_name)?;
    let ctor = cls.get_method("__construct")?;
    Some((ctor.parameters.clone(), args))
}

#[cfg(test)]
#[path = "resolution_tests.rs"]
mod tests;
