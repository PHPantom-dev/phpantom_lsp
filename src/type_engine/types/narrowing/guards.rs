//! Scalar and structural type guards (`is_string`, `is_array`, …),
//! class-string and member-existence guards, `in_array` element
//! narrowing, and guard-clause (early-return) narrowing.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use crate::atom::{atom, bytes_to_str};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{AssertionKind, ClassInfo, ResolvedType};

use mago_span::{HasSpan, Span};
use mago_syntax::cst::*;

use super::super::conditional::extract_class_string_from_expr;
use crate::type_engine::resolver::{FunctionLoaderFn, ScopeVarResolverFn, VarResolutionCtx};

use super::*;

/// Detect a class-string narrowing guard on `var_name`:
///
///   - `is_a($var, ClassName::class, true)` — the `allow_string` third
///     argument lets `$var` be a class-string as well as an object, so
///     a string-typed `$var` narrows to `class-string<ClassName>`
///     rather than an instance of `ClassName`.
///   - `class_exists($var)`, `interface_exists($var)`, `enum_exists($var)`,
///     `trait_exists($var)` — confirms `$var` names *some* declared
///     class-like, narrowing a string to the generic `class-string`
///     (the target class is not known statically).
///
/// Returns `Some((target, negated))` where `target` is `Some(name)` for
/// `is_a()` with a resolvable second argument, or `None` for the generic
/// `*_exists()` forms.  `negated` is `true` when the guard is wrapped in
/// `!`.
pub(in crate::type_engine) fn try_extract_class_string_guard(
    expr: &Expression<'_>,
    var_name: &str,
) -> Option<(Option<String>, bool)> {
    match expr {
        Expression::Parenthesized(inner) => {
            try_extract_class_string_guard(inner.expression, var_name)
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            try_extract_class_string_guard(prefix.operand, var_name)
                .map(|(target, negated)| (target, !negated))
        }
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => {
                    bytes_to_str(ident.value()).trim_start_matches('\\')
                }
                _ => return None,
            };
            let args: Vec<_> = func_call.argument_list.arguments.iter().collect();
            match func_name {
                "is_a" => {
                    if args.len() < 3 {
                        return None;
                    }
                    if expr_to_subject_key(argument_value(args[0])).as_deref() != Some(var_name) {
                        return None;
                    }
                    if !argument_value(args[2]).is_true() {
                        return None;
                    }
                    let target = extract_class_string_from_expr(argument_value(args[1]));
                    Some((target, false))
                }
                "class_exists" | "interface_exists" | "enum_exists" | "trait_exists" => {
                    if args.is_empty() {
                        return None;
                    }
                    if expr_to_subject_key(argument_value(args[0])).as_deref() != Some(var_name) {
                        return None;
                    }
                    Some((None, false))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Detect a member-existence guard on `var_name`:
///
///   - `property_exists($var, 'name')` — proves `$var` has a property
///     called `name` in the branch where the guard is true.  PHPStan
///     models this as an `object&hasProperty(name)` intersection.
///   - `method_exists($var, 'name')` — same for a method called `name`.
///   - `isset($var->name)` — proves `$var` has a property called `name`
///     (and that it is non-null) in the branch where the guard is true.
///     PHPStan treats this as an existence proof for the guarded access.
///
/// Only literal member names are recognised — a dynamic name proves the
/// existence of *some* member but not which one, so nothing can be added
/// to the type.
///
/// Returns `Some((member_name, is_method, negated))`; `negated` is `true`
/// when the guard is wrapped in `!`.
pub(in crate::type_engine) fn try_extract_member_exists_guard(
    expr: &Expression<'_>,
    var_name: &str,
) -> Option<(String, bool, bool)> {
    match expr {
        Expression::Parenthesized(inner) => {
            try_extract_member_exists_guard(inner.expression, var_name)
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            try_extract_member_exists_guard(prefix.operand, var_name)
                .map(|(name, is_method, negated)| (name, is_method, !negated))
        }
        // `isset($var->name)` proves the property exists on `$var`.  An
        // `isset()` may carry several arguments; the first whose subject
        // is `var_name` and whose member name is a literal identifier
        // proves that member.  Only direct property access on `var_name`
        // counts (a chained `$var->a->b` proves nothing about `$var`).
        Expression::Construct(Construct::Isset(isset)) => {
            for value in isset.values.iter() {
                let (object, property) = match value {
                    Expression::Access(Access::Property(pa)) => (pa.object, &pa.property),
                    Expression::Access(Access::NullSafeProperty(pa)) => (pa.object, &pa.property),
                    _ => continue,
                };
                if expr_to_subject_key(object).as_deref() != Some(var_name) {
                    continue;
                }
                if let ClassLikeMemberSelector::Identifier(ident) = property {
                    return Some((bytes_to_str(ident.value).to_string(), false, false));
                }
            }
            None
        }
        Expression::Call(Call::Function(func_call)) => {
            let func_name = match func_call.function {
                Expression::Identifier(ident) => {
                    bytes_to_str(ident.value()).trim_start_matches('\\')
                }
                _ => return None,
            };
            let is_method = match func_name {
                "property_exists" => false,
                "method_exists" => true,
                _ => return None,
            };
            let args: Vec<_> = func_call.argument_list.arguments.iter().collect();
            if args.len() < 2 {
                return None;
            }
            if expr_to_subject_key(argument_value(args[0])).as_deref() != Some(var_name) {
                return None;
            }
            let member = string_literal_value(argument_value(args[1]))?;
            Some((member, is_method, false))
        }
        _ => None,
    }
}

/// Check whether a statement unconditionally exits the current scope.
///
/// A statement unconditionally exits if every code path through it
/// ends with `return`, `throw`, `continue`, or `break`.  This is used
/// to detect guard clause patterns like:
///
/// ```text
/// if (!$var instanceof Foo) {
///     return;
/// }
/// // $var is Foo here
/// ```
///
/// A call to a function or method declared `never` also exits, which
/// takes type information rather than the AST alone; [`ExitCtx`] carries
/// what that lookup needs.
pub(in crate::type_engine) fn statement_unconditionally_exits(
    stmt: &Statement<'_>,
    ctx: &ExitCtx<'_>,
) -> bool {
    match stmt {
        Statement::Return(_) => true,
        Statement::Continue(_) => true,
        Statement::Break(_) => true,
        // `throw new …;` is parsed as an expression statement
        // containing a Throw expression.
        Statement::Expression(es) => {
            matches!(
                es.expression,
                Expression::Throw(_)
                    | Expression::Construct(mago_syntax::cst::Construct::Exit(_))
                    | Expression::Construct(mago_syntax::cst::Construct::Die(_))
            ) || expression_is_never_call(es.expression, ctx)
        }
        // A block exits if any statement in it exits — everything after
        // the first exiting statement is unreachable, so a trailing
        // assignment does not make the block fall through.
        Statement::Block(block) => block
            .statements
            .iter()
            .any(|s| statement_unconditionally_exits(s, ctx)),
        // An if/else exits if ALL branches exist and ALL exit.
        Statement::If(if_stmt) => if_body_unconditionally_exits(&if_stmt.body, ctx),
        _ => false,
    }
}

/// Check whether an `if` body (including all branches) unconditionally
/// exits.  This requires:
///   - The then-body exits, AND
///   - All elseif bodies exit, AND
///   - An else clause exists and exits.
fn if_body_unconditionally_exits(body: &IfBody<'_>, ctx: &ExitCtx<'_>) -> bool {
    match body {
        IfBody::Statement(stmt_body) => {
            if !statement_unconditionally_exits(stmt_body.statement, ctx) {
                return false;
            }
            if !stmt_body
                .else_if_clauses
                .iter()
                .all(|ei| statement_unconditionally_exits(ei.statement, ctx))
            {
                return false;
            }
            stmt_body
                .else_clause
                .as_ref()
                .is_some_and(|ec| statement_unconditionally_exits(ec.statement, ctx))
        }
        IfBody::ColonDelimited(colon_body) => {
            if !colon_body
                .statements
                .iter()
                .any(|s| statement_unconditionally_exits(s, ctx))
            {
                return false;
            }
            if !colon_body.else_if_clauses.iter().all(|ei| {
                ei.statements
                    .iter()
                    .any(|s| statement_unconditionally_exits(s, ctx))
            }) {
                return false;
            }
            colon_body.else_clause.as_ref().is_some_and(|ec| {
                ec.statements
                    .iter()
                    .any(|s| statement_unconditionally_exits(s, ctx))
            })
        }
    }
}

/// Check whether an `if` body's then-branch unconditionally exits.
/// Used for guard clause detection where we only need the then-body
/// to exit (no else clause required).
fn then_body_unconditionally_exits(body: &IfBody<'_>, ctx: &ExitCtx<'_>) -> bool {
    match body {
        IfBody::Statement(stmt_body) => statement_unconditionally_exits(stmt_body.statement, ctx),
        IfBody::ColonDelimited(colon_body) => colon_body
            .statements
            .iter()
            .any(|s| statement_unconditionally_exits(s, ctx)),
    }
}

/// The type information [`statement_unconditionally_exits`] needs to
/// recognise a call to a `never`-returning function or method.
///
/// Every consumer that asks "does this branch terminate?" builds one
/// from its own resolution context, so a guard clause ending in
/// `abort()` terminates the branch identically for local variables
/// (forward walker) and for properties (property narrowing).
pub(in crate::type_engine) struct ExitCtx<'a> {
    /// Enclosing class, used for `$this->…`, `self::`, `static::` and
    /// `parent::` receivers and for namespace-relative name resolution.
    pub current_class: &'a ClassInfo,
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    pub function_loader: FunctionLoaderFn<'a>,
    pub resolved_class_cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
    /// Resolves a `$`-prefixed local variable to its types, when the
    /// caller has a scope to consult.  Without it, only `$this` can be
    /// typed as a method-call receiver.
    pub var_types: ScopeVarResolverFn<'a>,
    /// Resolves a receiver that is not a plain variable, so that
    /// `app()->abort()` and `$this->aborter->fail()` terminate a branch
    /// the same way `$app->abort()` does.  See [`ReceiverResolverFn`].
    pub receiver_resolver: ReceiverResolverFn<'a>,
}

impl<'a> ExitCtx<'a> {
    /// Build an exit context from a variable-resolution context.
    ///
    /// The receiver resolver is supplied separately because the closure
    /// it wraps has to outlive the context, which a constructor cannot
    /// arrange for its own caller.
    pub(in crate::type_engine) fn from_var_ctx(
        ctx: &'a VarResolutionCtx<'a>,
        receiver_resolver: ReceiverResolverFn<'a>,
    ) -> Self {
        Self {
            current_class: ctx.current_class,
            class_loader: ctx.class_loader,
            function_loader: ctx.loaders.function_loader,
            resolved_class_cache: ctx.resolved_class_cache,
            var_types: ctx.scope_var_resolver,
            receiver_resolver,
        }
    }
}

/// Resolves a method-call receiver that is not a plain variable (a call
/// result, a `new` expression, a property chain) to the fully-qualified
/// names of the classes it can hold.
///
/// The consumer supplies this rather than the guard code resolving the
/// expression itself: each consumer already owns the scope the receiver
/// has to be read against (the forward walker's in-progress
/// `ScopeState`, or a `VarResolutionCtx`), and both feed the same shared
/// expression pipeline.
pub(in crate::type_engine) type ReceiverResolverFn<'a> =
    Option<&'a dyn for<'e> Fn(&'e Expression<'e>) -> Vec<String>>;

/// Keep the class-backed entries of a resolution result, as FQNs.
///
/// Both consumers build their [`ReceiverResolverFn`] on this, so a
/// receiver whose type is a union of a class and a scalar contributes
/// only the class.
pub(in crate::type_engine) fn class_names_of(types: &[ResolvedType]) -> Vec<String> {
    types
        .iter()
        .filter_map(|rt| rt.class_info.as_ref().map(|ci| ci.fqn().to_string()))
        .collect()
}

thread_local! {
    /// Receiver expressions whose type is currently being resolved on
    /// this thread, keyed by source span.
    ///
    /// Resolving a receiver runs the shared expression pipeline, which
    /// can walk back into the statement that asked whether the branch
    /// exits: `$this->aborter->fail()` asks for the type of
    /// `$this->aborter`, and property narrowing re-reads the guard
    /// clauses in front of the cursor to answer.  Keying by span breaks
    /// exactly that cycle and nothing else, since a span identifies one
    /// expression in one file.
    static RESOLVING_RECEIVERS: RefCell<HashSet<Span>> = RefCell::new(HashSet::new());
}

/// RAII guard for [`RESOLVING_RECEIVERS`].
struct ReceiverGuard {
    span: Span,
}

impl Drop for ReceiverGuard {
    fn drop(&mut self) {
        RESOLVING_RECEIVERS.with(|set| {
            set.borrow_mut().remove(&self.span);
        });
    }
}

/// Try to claim `span` for receiver resolution.  Returns `None` when the
/// same receiver is already being resolved further up the stack.
fn try_acquire_receiver_guard(span: Span) -> Option<ReceiverGuard> {
    let inserted = RESOLVING_RECEIVERS.with(|set| set.borrow_mut().insert(span));
    // Build the guard lazily: `then_some` would construct one even when
    // the insert failed, and dropping it would release the entry the
    // frame above owns, letting the cycle right back through.
    inserted.then(|| ReceiverGuard { span })
}

/// Check whether an expression is a call to a `never`-returning
/// function or method, which terminates the enclosing code path.
fn expression_is_never_call(expr: &Expression<'_>, ctx: &ExitCtx<'_>) -> bool {
    let Expression::Call(call) = expr else {
        return false;
    };
    match call {
        Call::Function(fc) => {
            let Expression::Identifier(ident) = &fc.function else {
                return false;
            };
            let Some(function_loader) = ctx.function_loader else {
                return false;
            };
            function_loader(bytes_to_str(ident.value()), fc.function.span().start.offset)
                .is_some_and(|fi| type_is_never(fi.return_type.as_ref()))
        }
        Call::Method(mc) => {
            let Some(method_name) = member_selector_name(&mc.method) else {
                return false;
            };
            let receivers = receiver_class_names(mc.object, ctx);
            !receivers.is_empty()
                && receivers
                    .iter()
                    .all(|class_name| class_has_never_method(class_name, method_name, ctx))
        }
        Call::StaticMethod(sc) => {
            let Some(method_name) = member_selector_name(&sc.method) else {
                return false;
            };
            let class_name = match &sc.class {
                // A source-level reference: PHP resolves an unqualified
                // name against the current namespace before the global
                // scope, so a same-namespace class must win over a
                // global class of the same short name.
                Expression::Identifier(ident) => crate::util::resolve_source_class_name(
                    bytes_to_str(ident.value()),
                    ctx.current_class.file_namespace.as_deref(),
                    ctx.class_loader,
                ),
                // `never` is the bottom type, so a child override can
                // only narrow to `never` again — reading `static::`
                // off the current class is sound.
                Expression::Self_(_) | Expression::Static(_) => ctx.current_class.fqn().to_string(),
                Expression::Parent(_) => match ctx.current_class.parent_class.as_ref() {
                    Some(parent) => parent.to_string(),
                    None => return false,
                },
                _ => return false,
            };
            class_has_never_method(&class_name, method_name, ctx)
        }
        // `$x?->fail()` is skipped entirely when `$x` is null, so it
        // does not unconditionally exit.
        Call::NullSafeMethod(_) => false,
    }
}

fn member_selector_name<'s>(selector: &ClassLikeMemberSelector<'s>) -> Option<&'s str> {
    match selector {
        ClassLikeMemberSelector::Identifier(ident) => Some(bytes_to_str(ident.value)),
        _ => None,
    }
}

/// Resolve the class names a method-call receiver can hold.
///
/// `$this` comes from the enclosing class; any other variable is read
/// from the caller's scope when one was supplied.  Anything else (a
/// property chain, a call result, a `new` expression) goes through the
/// caller's [`ReceiverResolverFn`], guarded against the cycle that
/// resolving an expression from a control-flow predicate can form.
fn receiver_class_names(object: &Expression<'_>, ctx: &ExitCtx<'_>) -> Vec<String> {
    if let Expression::Variable(Variable::Direct(dv)) = object {
        let var_name = bytes_to_str(dv.name);
        if var_name == "$this" {
            return vec![ctx.current_class.fqn().to_string()];
        }
        let Some(var_types) = ctx.var_types else {
            return Vec::new();
        };
        return class_names_of(&var_types(var_name));
    }
    let Some(receiver_resolver) = ctx.receiver_resolver else {
        return Vec::new();
    };
    let Some(_guard) = try_acquire_receiver_guard(object.span()) else {
        return Vec::new();
    };
    receiver_resolver(object)
}

fn class_has_never_method(class_name: &str, method_name: &str, ctx: &ExitCtx<'_>) -> bool {
    // A method declared on the class itself is the common case; only
    // fall back to an inheritance merge when the class does not declare
    // the method, so trait and parent declarations are still found.
    if ctx.current_class.fqn().eq_ignore_ascii_case(class_name)
        && let Some(is_never) = declared_method_returns_never(ctx.current_class, method_name)
    {
        return is_never;
    }
    let Some(class_info) = (ctx.class_loader)(class_name) else {
        return false;
    };
    if let Some(is_never) = declared_method_returns_never(&class_info, method_name) {
        return is_never;
    }
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        &class_info,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    declared_method_returns_never(&merged, method_name).unwrap_or(false)
}

/// `Some(true)`/`Some(false)` when `class_info` declares `method_name`,
/// `None` when it does not declare it at all.
fn declared_method_returns_never(class_info: &ClassInfo, method_name: &str) -> Option<bool> {
    class_info
        .methods
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(method_name))
        .map(|m| type_is_never(m.return_type.as_ref()))
}

fn type_is_never(return_type: Option<&PhpType>) -> bool {
    return_type.is_some_and(|t| t.is_never())
}

/// Apply guard clause narrowing after an `if` statement whose
/// then-body unconditionally exits (return/throw/continue/break)
/// and which has no else/elseif clauses.
///
/// When a guard clause like:
/// ```text
/// if (!$var instanceof Foo) { return; }
/// ```
/// appears before the cursor, the code after it can only be reached
/// when the condition was *false* — so we apply the inverse narrowing.
///
/// This handles:
///   - `instanceof` / `is_a()` / `get_class()` / `::class` checks
///   - `@phpstan-assert-if-true` / `@phpstan-assert-if-false` guards
pub(in crate::type_engine) fn apply_guard_clause_narrowing(
    if_stmt: &If<'_>,
    ctx: &VarResolutionCtx<'_>,
    results: &mut Vec<ClassInfo>,
) {
    let receiver_resolver = |expr: &Expression<'_>| {
        class_names_of(
            &crate::type_engine::variable::rhs_resolution::resolve_rhs_expression(expr, ctx),
        )
    };
    if !then_body_unconditionally_exits(
        &if_stmt.body,
        &ExitCtx::from_var_ctx(ctx, Some(&receiver_resolver)),
    ) {
        return;
    }
    if if_stmt.body.has_else_clause() || if_stmt.body.has_else_if_clauses() {
        return;
    }

    // ── Compound OR guard clause ────────────────────────────────────
    // `if ($x instanceof A || $x instanceof B) { return; }`
    // After the if, $x is neither A nor B → exclude both.
    if let Some(classes) = try_extract_compound_or_instanceof(if_stmt.condition, ctx.var_name)
        && !classes.is_empty()
    {
        for cls_type in &classes {
            apply_instanceof_exclusion(cls_type, ctx, results);
        }
        return;
    }

    // ── Compound negated AND guard clause ───────────────────────────
    // `if (!$x instanceof A && !$x instanceof B) { return; }`
    // The then-body exits when $x is neither A nor B.  After the if,
    // the condition was false, so $x IS instanceof A or B → include both.
    if let Some(classes) =
        try_extract_compound_negated_and_instanceof(if_stmt.condition, ctx.var_name)
        && !classes.is_empty()
    {
        let union = resolve_class_names_to_union(&classes, ctx);
        if !union.is_empty() {
            results.clear();
            *results = union;
        }
        return;
    }

    // ── Heterogeneous OR guard clause ───────────────────────────────
    // `if (!$a instanceof A || !$a->b instanceof B) { return; }`
    // De Morgan: after the guard every disjunct's negation holds, so
    // each disjunct narrows its own subject.  Apply the guard-inverse
    // for whichever disjunct is an instanceof on the current subject
    // (`ctx.var_name`).  This complements the same-subject compound OR
    // handler above, which returns early when it matches.
    {
        let operands = collect_or_operands(if_stmt.condition);
        if operands.len() > 1 {
            let mut narrowed = false;
            for operand in &operands {
                if let Some(mut extraction) =
                    try_extract_instanceof_with_negation(operand, ctx.var_name)
                {
                    resolve_extraction_to_fqn(&mut extraction, ctx.class_loader);
                    // Positive disjunct → excluded after the guard;
                    // negated disjunct → included after the guard.
                    if extraction.negated {
                        apply_instanceof_inclusion(
                            &extraction.class_type,
                            extraction.exact,
                            ctx,
                            results,
                        );
                    } else {
                        apply_instanceof_exclusion(&extraction.class_type, ctx, results);
                    }
                    narrowed = true;
                }
            }
            if narrowed {
                return;
            }
        }
    }

    // ── instanceof / is_a / get_class / ::class narrowing ──
    // The then-body exits, so subsequent code is the "else" — apply
    // the inverse of the condition.
    if let Some(mut extraction) =
        try_extract_instanceof_with_negation(if_stmt.condition, ctx.var_name)
    {
        resolve_extraction_to_fqn(&mut extraction, ctx.class_loader);
        // Positive instanceof + exit → exclude after (var is NOT that class)
        // Negated instanceof + exit → include after (var IS that class)
        if extraction.negated {
            apply_instanceof_inclusion(&extraction.class_type, extraction.exact, ctx, results);
        } else {
            apply_instanceof_exclusion(&extraction.class_type, ctx, results);
        }
    }

    // ── @phpstan-assert-if-true / @phpstan-assert-if-false ──
    // When a function or static method with assert-if-true/false is the
    // condition and the then-body exits, the code after runs when the
    // callee returned the opposite boolean — apply the inverse narrowing.
    let (func_call_expr, condition_negated) = unwrap_condition_negation(if_stmt.condition);

    if let Expression::Call(call) = func_call_expr
        && let Some(info) = extract_call_assertions(call, ctx)
    {
        // The then-body exits, so we're in the "else" conceptually.
        // inverted=true, same logic as apply_phpstan_assert_condition_narrowing.
        let function_returned_true = condition_negated;

        for assertion in &info.assertions {
            let applies_positively = match assertion.kind {
                AssertionKind::IfTrue => function_returned_true,
                AssertionKind::IfFalse => !function_returned_true,
                AssertionKind::Always => continue,
            };
            // The equality form (`!=Type`) promises a comparison, and a
            // comparison that fails rules nothing out, so it only speaks for
            // the branch it names.  Inverting it the way the subtype form
            // inverts would put Laravel's `filled()`/`blank()` promises in
            // the wrong branch.
            if !applies_positively && assertion.is_equality {
                continue;
            }

            if let Some(arg_var) = find_assertion_arg_variable(
                info.argument_list,
                &assertion.param_name,
                &info.parameters,
            ) && arg_var == ctx.var_name
            {
                let should_exclude = assertion.negated ^ !applies_positively;
                if should_exclude {
                    apply_instanceof_exclusion(&assertion.asserted_type, ctx, results);
                } else {
                    apply_instanceof_inclusion(&assertion.asserted_type, false, ctx, results);
                }
            }
        }
    }
}

// ── in_array strict-mode narrowing ───────────────────────────────

/// Extract the haystack expression from an
/// `in_array($needle, $haystack, true)` call where the needle
/// matches `var_name`.
///
/// Returns `Some(haystack_expr)` when:
///   - The function name is `in_array`
///   - The first argument is a simple `$variable` matching `var_name`
///   - There are at least 3 arguments and the third is the literal `true`
///
/// The caller is responsible for resolving the haystack expression's
/// iterable element type.
pub(in crate::type_engine) fn try_extract_in_array<'b>(
    expr: &'b Expression<'b>,
    var_name: &str,
) -> Option<&'b Expression<'b>> {
    let expr = match expr {
        Expression::Parenthesized(inner) => inner.expression,
        other => other,
    };
    let func_call = match expr {
        Expression::Call(Call::Function(fc)) => fc,
        _ => return None,
    };
    let name = match func_call.function {
        Expression::Identifier(ident) => bytes_to_str(ident.value()),
        _ => return None,
    };
    if !crate::util::strip_fqn_prefix(name).eq_ignore_ascii_case("in_array") {
        return None;
    }
    let args: Vec<_> = func_call.argument_list.arguments.iter().collect();
    if args.len() < 3 {
        return None;
    }

    let third_expr = match &args[2] {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    if !third_expr.is_true() {
        return None;
    }

    let first_expr = match &args[0] {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    // The needle is a subject like any other, so a property path
    // (`$row->email`) or a call (`$user->getEmail()`) names one just as a
    // bare variable does.
    if expr_to_subject_key(first_expr).as_deref() != Some(var_name) {
        return None;
    }

    let second_expr = match &args[1] {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    Some(second_expr)
}

/// The category of a PHP type-checking function like `is_array`, `is_string`, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeGuardKind {
    Array,
    String,
    Int,
    Float,
    Bool,
    Object,
    Numeric,
    Callable,
    Null,
    Scalar,
    Resource,
    /// `is_iterable` / `@phpstan-assert iterable`: an array or anything
    /// `foreach` can walk, i.e. a `Traversable`.
    Iterable,
}

/// Return the canonical `PhpType` that a type-guard narrows `mixed` to.
///
/// When a variable has type `mixed` and a type-guard like `is_object()`
/// succeeds, the variable should narrow to `object` (not stay `mixed`
/// and not become empty).  This function maps each guard kind to the
/// PHP type it asserts.
pub(crate) fn guard_kind_to_narrowed_type(kind: TypeGuardKind) -> PhpType {
    match kind {
        TypeGuardKind::Array => PhpType::array(),
        TypeGuardKind::String => PhpType::string(),
        TypeGuardKind::Int => PhpType::int(),
        TypeGuardKind::Float => PhpType::float(),
        TypeGuardKind::Bool => PhpType::bool(),
        TypeGuardKind::Object => PhpType::object(),
        TypeGuardKind::Numeric => PhpType::numeric(),
        TypeGuardKind::Callable => PhpType::callable(),
        TypeGuardKind::Null => PhpType::null(),
        TypeGuardKind::Scalar => PhpType::union(vec![
            PhpType::int(),
            PhpType::float(),
            PhpType::string(),
            PhpType::bool(),
        ]),
        TypeGuardKind::Resource => PhpType::named(atom("resource")),
        // `iterable` is PHP's own name for exactly this union, so keeping
        // the keyword says more than spelling it out would.
        TypeGuardKind::Iterable => PhpType::named(atom("iterable")),
    }
}

/// The interface every non-array iterable implements.
const TRAVERSABLE_FQN: &str = "Traversable";

/// The domain a `is_*()` builtin tests its argument against.
///
/// Returns `None` for any other function name.
pub(crate) fn type_guard_kind_from_name(name: &str) -> Option<TypeGuardKind> {
    Some(match name.trim_start_matches('\\') {
        "is_array" => TypeGuardKind::Array,
        "is_string" => TypeGuardKind::String,
        "is_int" | "is_integer" | "is_long" => TypeGuardKind::Int,
        "is_float" | "is_double" | "is_real" => TypeGuardKind::Float,
        "is_bool" => TypeGuardKind::Bool,
        "is_object" => TypeGuardKind::Object,
        "is_numeric" => TypeGuardKind::Numeric,
        "is_callable" => TypeGuardKind::Callable,
        "is_null" => TypeGuardKind::Null,
        "is_scalar" => TypeGuardKind::Scalar,
        "is_resource" => TypeGuardKind::Resource,
        "is_iterable" => TypeGuardKind::Iterable,
        _ => return None,
    })
}

/// Narrow `ty` to the values the `is_*()` builtin named by `name`
/// accepts.
///
/// Returns `None` when `name` is not a type guard, when every value of
/// `ty` already passes it, and when none can.
pub(crate) fn narrow_type_by_guard_name(
    name: &str,
    ty: &PhpType,
    class_loader: GuardClassLoader<'_>,
) -> Option<PhpType> {
    let kind = type_guard_kind_from_name(name)?;
    let narrowed = filter_type_by_guard(ty, kind, true, class_loader)?;
    (!narrowed.is_empty_sentinel()).then_some(narrowed)
}

/// Split `ty` by the type-check function `name` (`is_int`, `is_string`, …)
/// into the values it accepts and whether it can reject any.
///
/// A type check decides by type alone, so unlike an arbitrary callback
/// both halves are known: the accepted part is `None` when no value of
/// `ty` passes, and the flag is `false` when every value does.  Returns
/// `None` for a name that is not a type check.
pub(crate) fn split_type_by_guard_name(
    name: &str,
    ty: &PhpType,
    class_loader: GuardClassLoader<'_>,
) -> Option<(Option<PhpType>, bool)> {
    let kind = type_guard_kind_from_name(name)?;
    let accepted = match filter_type_by_guard(ty, kind, true, class_loader) {
        None => Some(ty.clone()),
        Some(filtered) if filtered.is_empty_sentinel() => None,
        Some(filtered) => Some(filtered),
    };
    let may_reject = guard_outcome_possible(ty, kind, false, class_loader);
    Some((accepted, may_reject))
}

/// Narrow `ty` to the values of `var_name` that can make `condition`
/// truthy.
///
/// `&&` chains narrow through each operand in turn, `||` chains join what
/// each branch admits, and a negated guard narrows by exclusion. Returns
/// `None` when the condition says nothing about `var_name` (so the caller
/// keeps the type it has) and when it admits no value at all, since an
/// empty type is never a useful answer for a caller that only knows the
/// condition held.
pub(crate) fn narrow_type_by_condition(
    condition: &Expression<'_>,
    var_name: &str,
    ty: &PhpType,
    class_loader: GuardClassLoader<'_>,
) -> Option<PhpType> {
    let narrowed = narrow_by_condition_inner(condition, var_name, ty, class_loader)?;
    (!narrowed.is_empty_sentinel()).then_some(narrowed)
}

fn narrow_by_condition_inner(
    condition: &Expression<'_>,
    var_name: &str,
    ty: &PhpType,
    class_loader: GuardClassLoader<'_>,
) -> Option<PhpType> {
    match condition {
        Expression::Parenthesized(inner) => {
            return narrow_by_condition_inner(inner.expression, var_name, ty, class_loader);
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
            ) =>
        {
            // Both operands hold, so the right-hand one narrows whatever
            // the left-hand one left behind.
            let lhs = narrow_by_condition_inner(bin.lhs, var_name, ty, class_loader);
            let base = lhs.as_ref().unwrap_or(ty);
            return narrow_by_condition_inner(bin.rhs, var_name, base, class_loader).or(lhs);
        }
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
            ) =>
        {
            // Either operand may be what let the value through, so the
            // answer is what they admit between them. An operand that says
            // nothing about `var_name` admits everything, which makes the
            // whole condition uninformative.
            let lhs = narrow_by_condition_inner(bin.lhs, var_name, ty, class_loader)?;
            let rhs = narrow_by_condition_inner(bin.rhs, var_name, ty, class_loader)?;
            let members: Vec<PhpType> = [lhs, rhs]
                .into_iter()
                .filter(|m| !m.is_empty_sentinel())
                .collect();
            return match members.len() {
                0 => Some(PhpType::empty_sentinel()),
                1 => members.into_iter().next(),
                _ => Some(PhpType::join_runtime_value_types(members)),
            };
        }
        _ => {}
    }

    // `$v instanceof Foo` is the type test a filter callback is most often
    // written with, and `$v !== null` the one `is_null()` is spelled as
    // when nobody reaches for a function call.
    if let Some(extraction) = super::try_extract_instanceof_with_negation(condition, var_name) {
        return narrow_type_by_instanceof(ty, &extraction, class_loader);
    }
    if let Some(expects_null) = try_extract_null_comparison(condition, var_name) {
        return filter_type_by_guard(ty, TypeGuardKind::Null, expects_null, class_loader);
    }

    let (kind, negated) = try_extract_type_guard(condition, var_name)?;
    filter_type_by_guard(ty, kind, !negated, class_loader)
}

/// What a `null` comparison concludes about the value that passes it.
struct NullComparison {
    /// Whether passing the comparison proves the value *is* null.
    expects_null: bool,
    /// Whether the operator carrying that conclusion was the strict one.
    strict: bool,
}

/// Whether `expr` compares `var_name` against `null`, and whether passing
/// it proves the value *is* null.
///
/// Only the strict operators prove a value is null: `$v == null` is also
/// true for `''`, `0` and `[]`, so reporting `null` for it would claim
/// more than the comparison shows — and neither does the negated spelling
/// of the same thing, `!($v != null)`. Proving a value is *not* null needs
/// no such care, since anything that survives either `!==` or `!=` is
/// non-null.
fn try_extract_null_comparison(expr: &Expression<'_>, var_name: &str) -> Option<bool> {
    let comparison = extract_null_comparison(expr, var_name)?;
    (!comparison.expects_null || comparison.strict).then_some(comparison.expects_null)
}

/// The raw conclusion of a `null` comparison on `var_name`, before the
/// loose operators are ruled out.
///
/// Strictness travels through the negations so that the caller can judge
/// `!($v != null)` by the operator that actually appears in it.
fn extract_null_comparison(expr: &Expression<'_>, var_name: &str) -> Option<NullComparison> {
    match expr {
        Expression::Parenthesized(inner) => extract_null_comparison(inner.expression, var_name),
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            extract_null_comparison(prefix.operand, var_name).map(|c| NullComparison {
                expects_null: !c.expects_null,
                strict: c.strict,
            })
        }
        Expression::Binary(bin) => {
            let (expects_null, strict) = match bin.operator {
                BinaryOperator::Identical(_) => (true, true),
                BinaryOperator::Equal(_) => (true, false),
                BinaryOperator::NotIdentical(_) => (false, true),
                BinaryOperator::NotEqual(_) => (false, false),
                _ => return None,
            };
            let (subject, literal) = if is_null_literal(bin.rhs) {
                (bin.lhs, bin.rhs)
            } else {
                (bin.rhs, bin.lhs)
            };
            if !is_null_literal(literal) || expr_to_subject_key(subject)? != var_name {
                return None;
            }
            Some(NullComparison {
                expects_null,
                strict,
            })
        }
        _ => None,
    }
}

/// Whether `expr` is the `null` keyword.
fn is_null_literal(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Parenthesized(inner) => is_null_literal(inner.expression),
        Expression::Literal(Literal::Null(_)) => true,
        Expression::ConstantAccess(access) => bytes_to_str(access.name.value())
            .trim_start_matches('\\')
            .eq_ignore_ascii_case("null"),
        _ => false,
    }
}

/// Try to extract a type-guard function call on a variable.
///
/// Matches `is_array($var)`, `is_string($var)`, etc. (with optional
/// parenthesisation and negation).
///
/// Returns `Some((kind, negated))` when the expression is a recognised
/// type-guard call on `var_name`.
pub(crate) fn try_extract_type_guard(
    expr: &Expression<'_>,
    var_name: &str,
) -> Option<(TypeGuardKind, bool)> {
    match expr {
        Expression::Parenthesized(inner) => try_extract_type_guard(inner.expression, var_name),
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            try_extract_type_guard(prefix.operand, var_name).map(|(kind, neg)| (kind, !neg))
        }
        Expression::Call(Call::Function(fc)) => {
            let func_name = match &fc.function {
                Expression::Identifier(ident) => {
                    bytes_to_str(ident.value()).trim_start_matches('\\')
                }
                _ => return None,
            };
            let kind = type_guard_kind_from_name(func_name)?;
            let args = &fc.argument_list.arguments;
            if args.len() != 1 {
                return None;
            }
            let arg_expr = match args.first() {
                Some(Argument::Positional(pos)) => pos.value,
                Some(Argument::Named(named)) => named.value,
                _ => return None,
            };
            let arg_name = expr_to_subject_key(arg_expr)?;
            if arg_name != var_name {
                return None;
            }
            Some((kind, false))
        }
        _ => None,
    }
}

/// Resolution `type_matches_guard` needs for the kinds whose answer is
/// nominal rather than structural.
///
/// Only `Iterable` uses it: whether a class is `foreach`-able is a
/// question about its interfaces, which the type alone cannot answer.
/// `None` means no loader was available, in which case a class type is
/// assumed iterable — a guard that has already passed at runtime is
/// better evidence than an unresolvable name.
pub(crate) type GuardClassLoader<'a> = Option<&'a dyn Fn(&str) -> Option<Arc<ClassInfo>>>;

/// Check whether a `PhpType` matches a given type-guard kind.
///
/// For `TypeGuardKind::Array`, returns `true` for array-like types
/// (`array`, `list<T>`, `T[]`, `array{…}`, `iterable`, etc.).
fn type_matches_guard(
    ty: &PhpType,
    kind: TypeGuardKind,
    class_loader: GuardClassLoader<'_>,
) -> bool {
    match kind {
        TypeGuardKind::Array => ty.is_array_like(),
        TypeGuardKind::String => ty.is_subtype_of(&PhpType::string()),
        TypeGuardKind::Int => ty.is_subtype_of(&PhpType::int()),
        // `is_float()` returns false for integers at runtime. The dedicated
        // helper includes exact float literals without applying PHP's
        // int-to-float parameter coercion.
        TypeGuardKind::Float => ty.is_float_subtype(),
        TypeGuardKind::Bool => ty.is_subtype_of(&PhpType::bool()),
        TypeGuardKind::Numeric => ty.is_subtype_of(&PhpType::numeric()),
        TypeGuardKind::Callable => ty.is_callable(),
        TypeGuardKind::Object => ty.is_object_like(),
        TypeGuardKind::Null => ty.is_null(),
        TypeGuardKind::Scalar => {
            ty.is_subtype_of(&PhpType::string())
                || ty.is_subtype_of(&PhpType::int())
                || ty.is_subtype_of(&PhpType::float())
                || ty.is_subtype_of(&PhpType::bool())
        }
        // `is_resource()` returns false for a closed resource, but a value
        // declared `closed-resource` is still in the resource domain, so the
        // subtype check covers both refinements.
        TypeGuardKind::Resource => ty.is_subtype_of(&PhpType::named(atom("resource"))),
        TypeGuardKind::Iterable => type_is_iterable(ty, class_loader),
    }
}

/// Whether `foreach` can walk a value of `ty`: an array (in any of its
/// spellings, `iterable` included) or an object implementing
/// `Traversable`.
///
/// A `Generator`, an `ArrayIterator`, and a collection declaring
/// `IteratorAggregate` all qualify through the interface walk, which
/// follows transitive extension, so `Traversable` need not be named
/// directly.
fn type_is_iterable(ty: &PhpType, class_loader: GuardClassLoader<'_>) -> bool {
    if ty.is_array_like() {
        return true;
    }
    let name = match ty.kind() {
        TypeKind::Named(n) => n.as_str(),
        TypeKind::Generic(g) => g.name.as_str(),
        TypeKind::Nullable(inner) => return type_is_iterable(inner, class_loader),
        // An object shape is a `stdClass`-alike, which is not traversable.
        _ => return false,
    };
    if !ty.is_object_like() {
        return false;
    }
    if name
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(TRAVERSABLE_FQN)
    {
        return true;
    }
    match class_loader {
        Some(loader) => loader(name)
            .is_some_and(|cls| crate::class_lookup::is_subtype_of(&cls, TRAVERSABLE_FQN, loader)),
        None => true,
    }
}

/// Narrow `results` to only the union members that match the given
/// type-guard kind.
///
/// For example, when `kind` is `Array` and the type string is
/// `null|list<Request>|Request`, the result is narrowed to
/// `list<Request>`.
pub(crate) fn apply_type_guard_inclusion(
    kind: TypeGuardKind,
    results: &mut Vec<ResolvedType>,
    class_loader: GuardClassLoader<'_>,
) {
    let had_types = !results.is_empty();
    for rt in results.iter_mut() {
        let filtered = filter_type_by_guard(&rt.type_string, kind, true, class_loader);
        if let Some(narrowed) = filtered {
            rt.replace_type(narrowed);
        }
    }
    // Remove entries that became empty (no union member matched).
    results.retain(|rt| !rt.type_string.is_empty_sentinel());

    // When the guard's assertion fully contradicts every statically known
    // candidate — e.g. `is_object($file)` where `$file` was inferred as
    // plain `string` because upstream inference (a foreach over a custom
    // iterator) missed a possible member — trust the runtime check over
    // the incomplete static type instead of silently discarding all type
    // information.  Only fires when *every* entry was eliminated; a
    // single stale/duplicate entry among several valid ones is dropped
    // as before.
    if had_types && results.is_empty() {
        results.push(ResolvedType::from_type_string(guard_kind_to_narrowed_type(
            kind,
        )));
    }
}

/// Narrow `results` to only the union members that do NOT match the
/// given type-guard kind (inverse / else-body narrowing).
pub(crate) fn apply_type_guard_exclusion(
    kind: TypeGuardKind,
    results: &mut Vec<ResolvedType>,
    class_loader: GuardClassLoader<'_>,
) {
    for rt in results.iter_mut() {
        let filtered = filter_type_by_guard(&rt.type_string, kind, false, class_loader);
        if let Some(narrowed) = filtered {
            rt.replace_type(narrowed);
        }
    }
    results.retain(|rt| !rt.type_string.is_empty_sentinel());
}

/// Report whether `ty` can produce the given outcome for a type guard:
/// with `expect_match = true`, whether the guard can pass; with `false`,
/// whether it can fail.
///
/// A `false` answer is a proof, not a guess: no value of `ty` reaches
/// that branch.  That is what lets a union of objects be discriminated by
/// a check on one of their properties.
pub(crate) fn guard_outcome_possible(
    ty: &PhpType,
    kind: TypeGuardKind,
    expect_match: bool,
    class_loader: GuardClassLoader<'_>,
) -> bool {
    match filter_type_by_guard(ty, kind, expect_match, class_loader) {
        Some(filtered) => !filtered.is_empty_sentinel(),
        None => true,
    }
}

/// Filter a `PhpType` to keep only members that match (or don't match)
/// the given type-guard kind.
///
/// When `keep_matching` is `true`, keeps only members where
/// `type_matches_guard` returns `true` (then-body semantics).
/// When `false`, keeps only members where it returns `false`
/// (else-body semantics).
///
/// Returns `None` when no filtering is needed (non-union type that
/// already satisfies the predicate).  Returns `Some(Named("__empty"))`
/// when all members are filtered out.
fn filter_type_by_guard(
    ty: &PhpType,
    kind: TypeGuardKind,
    keep_matching: bool,
    class_loader: GuardClassLoader<'_>,
) -> Option<PhpType> {
    // Expand compound pseudo-types into their constituent unions so
    // that type guards can filter individual members.  For example,
    // `array-key` → `int|string`, so `is_string()` on `array-key`
    // correctly narrows to `string`.
    if let Some(expanded) = expand_pseudo_type_for_guard(ty) {
        return filter_type_by_guard(&expanded, kind, keep_matching, class_loader);
    }
    // `is_array()` and `is_object()` each keep one half of an `iterable`,
    // so it has to be split before they can drop the other.
    if matches!(kind, TypeGuardKind::Array | TypeGuardKind::Object)
        && let Some(split) = ty.split_iterable()
    {
        return filter_type_by_guard(&split, kind, keep_matching, class_loader);
    }

    // `is_numeric()` also returns true for numeric strings, not just
    // `int`/`float`.  Narrow string-like members to `numeric-string`
    // instead of dropping them or widening to bare `int|float`, so the
    // narrowed type stays a subtype of the original `string`.
    if kind == TypeGuardKind::Numeric && keep_matching {
        let narrowed = narrow_to_numeric_inclusive(ty);
        return (narrowed != ty.clone()).then_some(narrowed);
    }

    match ty.kind() {
        TypeKind::Union(members) => {
            let filtered: Vec<PhpType> = members
                .iter()
                .filter(|m| type_matches_guard(m, kind, class_loader) == keep_matching)
                .cloned()
                .collect();
            if filtered.len() == members.len() {
                // Nothing was filtered out.
                None
            } else if filtered.is_empty() {
                Some(PhpType::empty_sentinel())
            } else if filtered.len() == 1 {
                Some(filtered.into_iter().next().unwrap())
            } else {
                Some(PhpType::union(filtered))
            }
        }
        TypeKind::Nullable(inner) => {
            // `?T` is `T|null`.  For `is_array`, null doesn't match,
            // so we keep only the inner type (if it matches) or only
            // null (if it doesn't).
            let inner_matches = type_matches_guard(inner, kind, class_loader);
            let null_matches = type_matches_guard(&PhpType::null(), kind, class_loader);
            match (
                inner_matches == keep_matching,
                null_matches == keep_matching,
            ) {
                (true, true) => None, // keep both → no change
                (true, false) => Some(inner.clone()),
                (false, true) => Some(PhpType::null()),
                (false, false) => Some(PhpType::empty_sentinel()),
            }
        }
        _ => {
            // `mixed` includes all types.  When narrowing in the
            // then-body (`keep_matching = true`), replace `mixed`
            // with the canonical type for the guard kind (e.g.
            // `is_object($mixed)` → `object`).  In the else-body
            // (`keep_matching = false`), `mixed` minus one kind is
            // still effectively `mixed`, so leave it unchanged.
            if ty.is_mixed() {
                return if keep_matching {
                    Some(guard_kind_to_narrowed_type(kind))
                } else {
                    None // mixed minus one kind ≈ mixed
                };
            }
            // Non-union type: if it matches the predicate, keep it.
            if type_matches_guard(ty, kind, class_loader) == keep_matching {
                None // no change needed
            } else {
                Some(PhpType::empty_sentinel())
            }
        }
    }
}

/// Expand compound pseudo-types into unions of their constituent scalar
/// types so that type guard filtering can operate on individual members.
///
/// - `array-key` → `int|string`
/// - `scalar` → `int|float|string|bool`
/// - `numeric` → `int|float|numeric-string`
/// - `number` → `int|float`
fn expand_pseudo_type_for_guard(ty: &PhpType) -> Option<PhpType> {
    let name = match ty.kind() {
        TypeKind::Named(n) => n.to_ascii_lowercase(),
        _ => return None,
    };
    match name.as_str() {
        "array-key" => Some(PhpType::union(vec![PhpType::int(), PhpType::string()])),
        "scalar" => Some(PhpType::union(vec![
            PhpType::int(),
            PhpType::float(),
            PhpType::string(),
            PhpType::bool(),
        ])),
        "numeric" => Some(PhpType::union(vec![
            PhpType::int(),
            PhpType::float(),
            PhpType::parse("numeric-string"),
        ])),
        "number" if ty.is_named("number") => {
            Some(PhpType::union(vec![PhpType::int(), PhpType::float()]))
        }
        _ => None,
    }
}

/// Narrow a type to what `is_numeric()` guarantees, keeping string-like
/// members within `numeric-string` rather than widening them to `int|float`
/// or dropping them.
fn narrow_to_numeric_inclusive(ty: &PhpType) -> PhpType {
    match ty.kind() {
        TypeKind::Union(members) => {
            let narrowed: Vec<PhpType> = members
                .iter()
                .filter_map(narrow_single_type_to_numeric)
                .collect();
            if narrowed.is_empty() {
                PhpType::empty_sentinel()
            } else {
                PhpType::union(narrowed)
            }
        }
        // `null` never satisfies `is_numeric()`; narrow the inner type only.
        TypeKind::Nullable(inner) => {
            narrow_single_type_to_numeric(inner).unwrap_or_else(PhpType::empty_sentinel)
        }
        _ => narrow_single_type_to_numeric(ty).unwrap_or_else(PhpType::empty_sentinel),
    }
}

/// Narrow a single (non-union) type to what `is_numeric()` guarantees.
/// Returns `None` when the type can never be numeric (e.g. an object).
fn narrow_single_type_to_numeric(ty: &PhpType) -> Option<PhpType> {
    if ty.is_mixed() {
        return Some(PhpType::union(vec![
            PhpType::int(),
            PhpType::float(),
            PhpType::parse("numeric-string"),
        ]));
    }
    if type_matches_guard(ty, TypeGuardKind::Numeric, None) {
        return Some(ty.clone());
    }
    // An exact non-numeric literal cannot become numeric merely because its
    // broad scalar type is string. Only an imprecise string-like type can be
    // refined to `numeric-string`.
    if matches!(ty.kind(), TypeKind::Literal(_)) {
        return None;
    }
    if ty.is_subtype_of(&PhpType::string()) {
        return Some(PhpType::parse("numeric-string"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_guard_accepts_float_literals_but_not_int_literals() {
        let float = PhpType::literal_float("1.5");
        let int = PhpType::literal_int("1");

        assert!(type_matches_guard(&float, TypeGuardKind::Float, None));
        assert!(type_matches_guard(
            &PhpType::parse("real"),
            TypeGuardKind::Float,
            None
        ));
        assert!(!type_matches_guard(&int, TypeGuardKind::Float, None));
        assert_eq!(
            filter_type_by_guard(&float, TypeGuardKind::Float, true, None),
            None
        );
        assert_eq!(
            filter_type_by_guard(&float, TypeGuardKind::Float, false, None),
            Some(PhpType::empty_sentinel())
        );
    }

    #[test]
    fn float_guard_filters_literal_unions_in_both_directions() {
        let float = PhpType::literal_float("1.5");
        let int = PhpType::literal_int("1");
        let union = PhpType::union(vec![float.clone(), int.clone()]);

        assert_eq!(
            filter_type_by_guard(&union, TypeGuardKind::Float, true, None),
            Some(float)
        );
        assert_eq!(
            filter_type_by_guard(&union, TypeGuardKind::Float, false, None),
            Some(int)
        );
    }

    #[test]
    fn numeric_guard_accepts_only_numeric_string_literals() {
        let numeric_string = PhpType::literal_string_raw("'1.5'");
        let text = PhpType::literal_string_raw("'draft'");
        // `\x31` decodes to `"1"`, a numeric string, once escapes are decoded.
        let escaped = PhpType::literal_string_raw("\"\\x31\"");

        assert!(type_matches_guard(
            &numeric_string,
            TypeGuardKind::Numeric,
            None
        ));
        assert!(!type_matches_guard(&text, TypeGuardKind::Numeric, None));
        assert!(type_matches_guard(&escaped, TypeGuardKind::Numeric, None));
        assert_eq!(
            filter_type_by_guard(&numeric_string, TypeGuardKind::Numeric, true, None),
            None
        );
        assert_eq!(
            filter_type_by_guard(&text, TypeGuardKind::Numeric, true, None),
            Some(PhpType::empty_sentinel())
        );
        assert_eq!(
            filter_type_by_guard(&numeric_string, TypeGuardKind::Numeric, false, None),
            Some(PhpType::empty_sentinel())
        );
        assert_eq!(
            filter_type_by_guard(&text, TypeGuardKind::Numeric, false, None),
            None
        );
        assert_eq!(
            filter_type_by_guard(&escaped, TypeGuardKind::Numeric, true, None),
            None
        );
    }
}
