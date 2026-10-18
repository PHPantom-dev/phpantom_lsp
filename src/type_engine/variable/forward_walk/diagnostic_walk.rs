//! Building the diagnostic scope snapshots for a whole file: every
//! function, method, property hook and top-level body is seeded and walked
//! with [`walk_body_for_diagnostics`].

use super::*;
use std::sync::Arc;

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;
use crate::type_engine::resolver::Loaders;
use crate::types::ClassInfo;
use crate::types::{BlockClassLoaders, BlockImports};

#[cfg(test)]
thread_local! {
    /// How many function bodies a scope walk has seeded and walked, so a
    /// test can assert a targeted walk skipped the ones nothing asked
    /// about.
    static TEST_BODY_WALKS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_test_body_walks() {
    TEST_BODY_WALKS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn test_body_walks() -> usize {
    TEST_BODY_WALKS.with(Cell::get)
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_diagnostic_scopes(
    content: &str,
    local_classes: &[Arc<ClassInfo>],
    class_loaders: &BlockClassLoaders<'_>,
    source_class_loaders: &BlockClassLoaders<'_>,
    block_imports: &BlockImports<'_>,
    backend: Option<&crate::Backend>,
    loaders: Loaders<'_>,
    resolved_class_cache: Option<&crate::virtual_members::ResolvedClassCache>,
) {
    if !is_diagnostic_scope_active() {
        return;
    }

    // Skip if the scope cache is already populated by an earlier
    // whole-file walk (prevents double walk when both the analyze loop
    // and collect_slow_diagnostics call this function).  Snapshots left
    // by a targeted walk cover only part of the file, so they do not
    // stand in for the whole-file walk a caller asked for here.
    if scopes_populated() && scope_coverage_is_whole() {
        return;
    }
    set_whole_scope_coverage();

    walk_file_for_scopes(
        content,
        local_classes,
        class_loaders,
        source_class_loaders,
        block_imports,
        backend,
        loaders,
        resolved_class_cache,
        &[],
    );
}

/// Build diagnostic scope snapshots for just the bodies that enclose
/// `offsets`.
///
/// A member-reference search asks about one or two accesses in a
/// candidate file; walking the file's other few dozen bodies to answer
/// costs a pass of the type engine each and answers nothing.  This walks
/// only the function, method, property hook, or top-level region holding
/// each offset, into the same [`DIAGNOSTIC_SCOPE`] cache the whole-file
/// walk populates, so the two consumers cannot disagree about a type.
///
/// Offsets a previous walk already covered are not walked again, so a
/// second access in a body the first one walked is free.
///
/// The caller must have activated the cache via
/// [`with_diagnostic_scope_cache`] before calling this function.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_diagnostic_scopes_for_offsets(
    content: &str,
    local_classes: &[Arc<ClassInfo>],
    class_loaders: &BlockClassLoaders<'_>,
    source_class_loaders: &BlockClassLoaders<'_>,
    block_imports: &BlockImports<'_>,
    backend: Option<&crate::Backend>,
    loaders: Loaders<'_>,
    resolved_class_cache: Option<&crate::virtual_members::ResolvedClassCache>,
    offsets: &[u32],
) {
    if !is_diagnostic_scope_active() || offsets.is_empty() {
        return;
    }

    // A whole-file walk already answers for every offset.
    if scopes_populated() && scope_coverage_is_whole() {
        return;
    }
    begin_targeted_coverage();

    let mut targets: Vec<u32> = offsets
        .iter()
        .copied()
        .filter(|offset| !scope_snapshots_cover(*offset))
        .collect();
    if targets.is_empty() {
        return;
    }
    targets.sort_unstable();
    targets.dedup();

    walk_file_for_scopes(
        content,
        local_classes,
        class_loaders,
        source_class_loaders,
        block_imports,
        backend,
        loaders,
        resolved_class_cache,
        &targets,
    );
}

/// Whether the active scope cache holds any snapshots.
fn scopes_populated() -> bool {
    DIAGNOSTIC_SCOPE.with(|cell| cell.borrow().as_ref().is_some_and(|m| !m.is_empty()))
}

/// Parse `content` and walk it for scope snapshots, restricted to the
/// bodies enclosing `targets` when that slice is non-empty.
#[allow(clippy::too_many_arguments)]
fn walk_file_for_scopes(
    content: &str,
    local_classes: &[Arc<ClassInfo>],
    class_loaders: &BlockClassLoaders<'_>,
    source_class_loaders: &BlockClassLoaders<'_>,
    block_imports: &BlockImports<'_>,
    backend: Option<&crate::Backend>,
    loaders: Loaders<'_>,
    resolved_class_cache: Option<&crate::virtual_members::ResolvedClassCache>,
    targets: &[u32],
) {
    // Mark that we are building the scope cache so that nested
    // resolution calls (e.g. resolve_variable_types) do not read
    // from the partially-populated cache.
    BUILDING_SCOPES.with(|cell: &Cell<bool>| cell.set(true));
    let _building_guard = BuildingScopesGuard;

    let default_class = crate::class_lookup::placeholder_in_namespace(None);
    let diag_ctx = DiagnosticWalkCtx {
        content,
        local_classes,
        class_loader: *class_loaders.at(0),
        source_class_loader: *source_class_loaders.at(0),
        source_imports: block_imports.at(0),
        class_loaders,
        source_class_loaders,
        block_imports,
        backend,
        loaders,
        resolved_class_cache,
        targets,
        globals: None,
    };

    with_parsed_program(content, "build_diagnostic_scopes", |program, _content| {
        // Walk all top-level statements, analyzing function/method
        // bodies AND top-level code (assignments, expressions, if,
        // foreach, etc.) that lives outside any function body.
        walk_top_level_statements(program.statements.iter(), &default_class, &diag_ctx);
    });
}

/// Walk a sequence of top-level (or namespace-level) statements,
/// maintaining a shared `ScopeState` for code that lives outside any
/// function or class body.  Function/class/trait/interface/enum bodies
/// are analyzed in isolation (as before), but top-level assignments,
/// expressions, if/foreach/while/for/try/switch, and other statements
/// are walked through the forward walker so that scope snapshots are
/// recorded.  This ensures variable accesses resolve from the scope
/// cache.
pub(crate) fn walk_top_level_statements<'a, 'b: 'a>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    default_class: &ClassInfo,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    let statements: Vec<&'b Statement<'b>> = statements.collect();
    let ctx = diag_ctx.forward_walk_ctx(default_class);

    // A `global` statement reads the variables the file's top-level code
    // leaves behind, the same scope the request path builds for it.
    let globals = (diag_ctx.globals.is_none() && diag_ctx.content.contains("global "))
        .then(|| top_level_scope_for_globals(statements.iter().copied(), &ctx).locals);
    let globals_ctx;
    let diag_ctx = match &globals {
        Some(locals) => {
            globals_ctx = DiagnosticWalkCtx {
                globals: Some(locals),
                ..*diag_ctx
            };
            &globals_ctx
        }
        None => diag_ctx,
    };
    let ctx = diag_ctx.forward_walk_ctx(default_class);

    let mut top_level_scope = ScopeState::new();

    // Seed superglobals for top-level code.
    seed_superglobals(&mut top_level_scope);

    let last_target = diag_ctx.last_target();

    for stmt in statements {
        match stmt {
            Statement::Namespace(ns) => {
                // Recurse into namespace body with a fresh scope.  The
                // stand-in class carries the block's namespace so that
                // source-level class references inside a plain function
                // resolve against it, the way they do inside a method.
                let ns_class = crate::class_lookup::placeholder_in_namespace(
                    ns.name.as_ref().map(|ident| bytes_to_str(ident.value())),
                );
                // The block's own imports apply inside it, not the file's.
                let block_ctx = DiagnosticWalkCtx {
                    class_loader: *diag_ctx.class_loaders.at(ns.span().start.offset),
                    source_class_loader: *diag_ctx.source_class_loaders.at(ns.span().start.offset),
                    source_imports: diag_ctx.block_imports.at(ns.span().start.offset),
                    ..*diag_ctx
                };
                walk_top_level_statements(ns.statements().iter(), &ns_class, &block_ctx);
            }
            _ if let Some(body) = crate::parser::class_like_body(stmt) => {
                if !diag_ctx.wants(stmt.span()) {
                    continue;
                }
                let enclosing = find_enclosing_class_for_offset(
                    diag_ctx.local_classes,
                    body.left_brace.start.offset,
                )
                .unwrap_or(default_class);
                for member in body.members {
                    walk_class_member_body(member, enclosing, diag_ctx);
                }
            }
            Statement::Function(func) => {
                if !diag_ctx.wants(stmt.span()) {
                    continue;
                }
                diag_ctx.mark_walked(stmt.span());
                analyze_function_body(
                    func.parameter_list.parameters.iter(),
                    func.body.statements.iter(),
                    func.span().start.offset,
                    default_class,
                    BodyOwner::FUNCTION,
                    diag_ctx,
                );
            }
            // Functions nested inside if blocks (common pattern:
            // `if (!function_exists('name')) { function name() {} }`)
            // must be analyzed the same way as top-level functions.
            Statement::If(_) => {
                if stmt.span().start.offset <= last_target {
                    diag_ctx.mark_walked(stmt.span());
                    record_scope_snapshot(stmt.span().start.offset, &top_level_scope);
                    let pre_stmt_scope = top_level_scope.clone();
                    process_statement(stmt, &mut top_level_scope, &ctx);
                    walk_closures_in_statement(stmt, &pre_stmt_scope, &top_level_scope, &ctx);
                    record_scope_snapshot(stmt.span().end.offset, &top_level_scope);
                }
                if diag_ctx.wants(stmt.span()) {
                    walk_functions_in_if(stmt, default_class, diag_ctx);
                }
            }
            // Top-level code: walk it with the shared scope so that
            // variable assignments accumulate and subsequent accesses
            // can be served from the scope cache instead of remaining
            // unresolved.  A targeted walk still has to carry the scope
            // through every statement before the last offset it was
            // asked about, since any of them can be what gives a
            // variable its type; past that point nothing it learns can
            // change an answer.
            _ => {
                if stmt.span().start.offset > last_target {
                    continue;
                }
                diag_ctx.mark_walked(stmt.span());
                record_scope_snapshot(stmt.span().start.offset, &top_level_scope);
                let pre_stmt_scope = top_level_scope.clone();
                process_statement(stmt, &mut top_level_scope, &ctx);
                walk_closures_in_statement(stmt, &pre_stmt_scope, &top_level_scope, &ctx);
                record_scope_snapshot(stmt.span().end.offset, &top_level_scope);
            }
        }
    }
}

/// Analyze each function declared inside an `if` statement, in any branch
/// and at any depth of nested blocks, the same way as a top-level
/// function.  Handles the common PHP pattern:
/// `if (!function_exists('name')) { function name(...) { ... } }`
pub(crate) fn walk_functions_in_if<'b>(
    if_stmt: &'b Statement<'b>,
    default_class: &ClassInfo,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    crate::parser::for_each_nested_function(if_stmt, &mut |func| {
        if !diag_ctx.wants(func.span()) {
            return;
        }
        diag_ctx.mark_walked(func.span());
        analyze_function_body(
            func.parameter_list.parameters.iter(),
            func.body.statements.iter(),
            func.span().start.offset,
            default_class,
            BodyOwner::FUNCTION,
            diag_ctx,
        );
    });
}

/// Walk a class member to find method bodies and run the forward walker.
pub(crate) fn walk_class_member_body<'b>(
    member: &'b mago_syntax::cst::class_like::member::ClassLikeMember<'b>,
    enclosing_class: &ClassInfo,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

    if !diag_ctx.wants(member.span()) {
        return;
    }

    match member {
        ClassLikeMember::Method(method) => {
            // A constructor-promoted property carries its hooks in the
            // parameter list, so they need walking whether or not the
            // constructor itself has a body.
            for param in method.parameter_list.parameters.iter() {
                if let Some(hooks) = &param.hooks {
                    walk_property_hook_bodies(
                        param.hint.as_ref(),
                        hooks,
                        enclosing_class,
                        diag_ctx,
                    );
                }
            }

            let MethodBody::Concrete(block) = &method.body else {
                return;
            };
            diag_ctx.mark_walked(method.span());
            analyze_function_body(
                method.parameter_list.parameters.iter(),
                block.statements.iter(),
                method.span().start.offset,
                enclosing_class,
                BodyOwner::method(method),
                diag_ctx,
            );
        }
        ClassLikeMember::Property(Property::Hooked(hooked)) => {
            walk_property_hook_bodies(
                hooked.hint.as_ref(),
                &hooked.hook_list,
                enclosing_class,
                diag_ctx,
            );
        }
        _ => {}
    }
}

/// Walk the `get`/`set` bodies of a hooked property.
///
/// Each hook gets its own seeded scope, exactly as a method body does.
/// Without one the snapshot in force inside the hook is whatever the
/// previously walked body left behind, so `$this` resolves to that body's
/// class and every `$this->member` in the hook is flagged as unknown.
fn walk_property_hook_bodies(
    property_hint: Option<&Hint<'_>>,
    hook_list: &PropertyHookList<'_>,
    enclosing_class: &ClassInfo,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    let ctx = diag_ctx.forward_walk_ctx(enclosing_class);

    for hook in hook_list.hooks.iter() {
        let PropertyHookBody::Concrete(body) = &hook.body else {
            continue;
        };
        if !diag_ctx.wants(hook.span()) {
            continue;
        }

        let mut scope = seed_property_hook_scope(property_hint, hook, &ctx);
        diag_ctx.mark_walked(hook.span());
        record_scope_snapshot(hook.span().start.offset, &scope);

        match body {
            PropertyHookConcreteBody::Block(block) => {
                walk_body_for_diagnostics(block.statements.iter(), &mut scope, &ctx);
            }
            // A one-line hook holds a single expression with nothing to
            // assign into the scope, so the snapshot above already
            // describes every point in it — except inside a closure
            // embedded in that expression, whose parameters need their
            // own snapshot.
            PropertyHookConcreteBody::Expression(expr_body) => {
                walk_closures_in_expr(expr_body.expression, &scope, &ctx, None);
            }
        }
    }
}

/// Bundles the immutable context needed by [`analyze_function_body`] and
/// the AST walkers so we don't pass 5+ individual arguments everywhere.
#[derive(Clone, Copy)]
pub(crate) struct DiagnosticWalkCtx<'a> {
    content: &'a str,
    local_classes: &'a [Arc<ClassInfo>],
    /// The loader for the `namespace` block being walked.
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// The block's loader for names as written in source.
    source_class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// The block's `use` imports.
    source_imports: &'a std::collections::HashMap<String, String>,
    /// Every block's loader, to switch to at a block boundary.
    class_loaders: &'a BlockClassLoaders<'a>,
    source_class_loaders: &'a BlockClassLoaders<'a>,
    block_imports: &'a BlockImports<'a>,
    backend: Option<&'a crate::Backend>,
    loaders: Loaders<'a>,
    resolved_class_cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
    /// Sorted byte offsets the caller needs scopes for, or empty to walk
    /// the whole file.
    targets: &'a [u32],
    /// The file's top-level variables, which a `global` statement in a
    /// body imports.  `None` until the top-level walk has built them, and
    /// in a file that never says `global`.
    globals: Option<&'a Locals>,
}

impl<'a> DiagnosticWalkCtx<'a> {
    /// Whether a body spanning `span` holds one of the offsets this walk
    /// was asked about.  Always true for a whole-file walk.
    fn wants(&self, span: mago_span::Span) -> bool {
        if self.targets.is_empty() {
            return true;
        }
        let start = self.targets.partition_point(|&o| o < span.start.offset);
        let end = self.targets.partition_point(|&o| o <= span.end.offset);
        start < end
    }

    /// The last offset this walk was asked about, or `u32::MAX` for a
    /// whole-file walk.  Statements that begin past it cannot affect any
    /// answer, so a targeted walk stops carrying the file-level scope
    /// through them.
    fn last_target(&self) -> u32 {
        self.targets.last().copied().unwrap_or(u32::MAX)
    }

    /// Record that this walk produced snapshots for every offset in
    /// `span`, so lookups inside it may be answered from them.
    fn mark_walked(&self, span: mago_span::Span) {
        if !self.targets.is_empty() {
            record_covered_region(span.start.offset, span.end.offset);
        }
    }

    /// Build the forward-walk context for a body belonging to
    /// `current_class`.  The diagnostic pass walks the whole file, so the
    /// cursor is past the end of every body it visits.
    fn forward_walk_ctx<'c>(&'c self, current_class: &'c ClassInfo) -> ForwardWalkCtx<'c> {
        ForwardWalkCtx {
            current_class,
            all_classes: self.local_classes,
            content: self.content,
            cursor_offset: u32::MAX,
            class_loader: self.class_loader,
            source_class_loader: self.source_class_loader,
            source_imports: Some(self.source_imports),
            backend: self.backend,
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: None,
            top_level_scope: self.globals.cloned(),
            in_loop: false,
            template_markers: None,
            cursor_scope: None,
        }
    }
}

/// Run the forward walker on a single function/method body and record
/// scope snapshots for diagnostics.
pub(crate) fn analyze_function_body<'b>(
    parameters: impl Iterator<Item = &'b FunctionLikeParameter<'b>>,
    body_statements: impl Iterator<Item = &'b Statement<'b>>,
    fn_span_start: u32,
    current_class: &ClassInfo,
    owner: BodyOwner<'_>,
    diag_ctx: &DiagnosticWalkCtx<'_>,
) {
    let ctx = diag_ctx.forward_walk_ctx(current_class);
    seed_and_walk_function_body(parameters, body_statements, fn_span_start, owner, &ctx);
}

/// Seed a fresh scope for a function/method body and walk it for
/// diagnostic scope snapshots.
///
/// Shared by [`analyze_function_body`] (top-level functions and class
/// methods) and [`walk_anonymous_class_member_bodies`] (methods declared
/// inside an anonymous class expression), which differ only in the
/// `current_class` carried by `ctx`.
pub(crate) fn seed_and_walk_function_body<'b>(
    parameters: impl Iterator<Item = &'b FunctionLikeParameter<'b>>,
    body_statements: impl Iterator<Item = &'b Statement<'b>>,
    fn_span_start: u32,
    owner: BodyOwner<'_>,
    ctx: &ForwardWalkCtx<'_>,
) {
    #[cfg(test)]
    TEST_BODY_WALKS.with(|count| count.set(count.get() + 1));

    let ctx = &ctx.for_declaration(fn_span_start);
    let body: Vec<&Statement<'_>> = body_statements.collect();
    let mut scope = seed_body_scope(parameters, &body, fn_span_start, owner, ctx);

    // Record the scope right at the function body start so that
    // member accesses on parameters before any assignment are covered.
    record_scope_snapshot(fn_span_start, &scope);

    // Walk the entire body, recording snapshots at each statement.
    walk_body_for_diagnostics(body.iter().copied(), &mut scope, ctx);
}

/// Walk the method bodies of an anonymous class expression, seeding
/// `$this` to the anonymous class itself.
///
/// Without this, the forward walker records `$this` snapshots for the
/// lexically enclosing method (whose `$this` is the outer class) and
/// those snapshots leak into the anonymous class's method bodies, since
/// they sit at higher offsets with no intervening re-seed.  Member
/// accesses like `$this->prop` inside the anonymous class would then
/// resolve against the outer class and be flagged as unknown.
pub(crate) fn walk_anonymous_class_member_bodies<'b>(
    anon: &'b AnonymousClass<'b>,
    ctx: &ForwardWalkCtx<'_>,
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

    // The parser extracts anonymous classes as `ClassInfo` with the
    // synthetic name `__anonymous@<left_brace_offset>`.  Look it up so
    // the walk sees the anonymous class's real members instead of the
    // enclosing class.
    let anon_name = format!("__anonymous@{}", anon.left_brace.start.offset);
    let Some(anon_class) = ctx.all_classes.iter().find(|c| *c.name == anon_name) else {
        return;
    };

    let anon_ctx = ForwardWalkCtx {
        current_class: anon_class.as_ref(),
        ..ctx.fresh()
    };

    for member in anon.members.iter() {
        if let ClassLikeMember::Method(method) = member
            && let MethodBody::Concrete(block) = &method.body
        {
            seed_and_walk_function_body(
                method.parameter_list.parameters.iter(),
                block.statements.iter(),
                method.span().start.offset,
                BodyOwner::method(method),
                &anon_ctx,
            );
        }
    }
}

/// Find the innermost class whose body span contains `offset`.
///
/// Unlike [`find_class_at_offset`](crate::class_lookup::find_class_at_offset),
/// a class's attributes and header are not part of it.
pub(crate) fn find_enclosing_class_for_offset(
    local_classes: &[Arc<ClassInfo>],
    offset: u32,
) -> Option<&ClassInfo> {
    local_classes
        .iter()
        .filter(|c| offset >= c.start_offset && offset <= c.end_offset)
        .min_by_key(|c| c.end_offset.saturating_sub(c.start_offset))
        .map(|c| c.as_ref())
}
