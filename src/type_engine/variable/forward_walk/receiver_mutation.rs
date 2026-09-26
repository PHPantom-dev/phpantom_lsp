//! Receiver mutation: a call that changes the state of the object it is
//! called on invalidates what the walker knows about that object.

use super::*;

use std::sync::Arc;

use mago_span::HasSpan;

use crate::atom::bytes_to_str;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::types::narrowing;

use super::scope_state::MemberInvalidation;

/// Drop what a state-changing call could have altered.
///
/// A check is only worth remembering while the thing it was made about
/// still holds. `if ($stmt->fetch('id') !== false)` proves something about
/// `$stmt`'s current row; `$stmt->execute()` moves to another one, so the
/// proof describes a state the program has left.
///
/// Three things change an object behind a variable that still holds it:
/// a call on it, a call it is passed to, and a write to one of its
/// properties.  Which calls count, and how much of what is known they
/// take with them, is [`CallEffect`]'s decision, and getting it wrong the
/// other way is what made guard-then-read fail: a second getter on the
/// same object (`$r->getFileName() !== false` proved, then
/// `$r->getDocComment()` read) is not an event that unproves the first.
pub(crate) fn process_receiver_mutation<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let mut invalidations: Vec<Invalidation> = Vec::new();
    collect_call_invalidations(expr, scope, ctx, &mut invalidations);

    // A write to `$x->prop` (or through it, `$x->prop['k'] = …`) is what
    // the object's own methods read, so their recorded results go stale.
    // The other properties keep what they were shown to hold.
    if let Some(object) = written_object(expr)
        && scope_reads_through(scope, &object)
    {
        invalidations.push(Invalidation {
            subject: object,
            made: None,
            members: false,
            method: None,
        });
    }
    for invalidation in invalidations {
        if !invalidation.members {
            scope.invalidate_receiver_state(
                &invalidation.subject,
                invalidation.made.as_deref(),
                &MemberInvalidation::Calls,
            );
            continue;
        }
        let kept = untouched_property_keys(
            &invalidation.subject,
            invalidation.method.as_deref(),
            scope,
            ctx,
        );
        // A property path goes back to what its declaration promises
        // rather than out of the scope: with no entry at all, a read of
        // `$this->prop` would find the assignment the call may have
        // overwritten by scanning back through the method.
        let reset: Vec<String> = scope
            .locals
            .keys()
            .filter(|key| {
                let key: &str = key;
                key != invalidation.subject
                    && !kept.iter().any(|k| k == key)
                    && !narrowing::is_call_key(key)
                    && key.ends_with(|c: char| c.is_alphanumeric() || c == '_')
                    && narrowing::key_reads_variable(key, &invalidation.subject)
            })
            .map(|key| key.to_string())
            .collect();
        scope.invalidate_receiver_state(
            &invalidation.subject,
            invalidation.made.as_deref(),
            &MemberInvalidation::Members { kept },
        );
        for key in reset {
            let declared = super::cond_narrowing::declared_key_type(&key, scope, ctx);
            if !declared.is_empty() {
                scope.set(&key, declared);
            }
        }
    }
}

/// Forget what a condition proved about the result of a call declared
/// impure.
///
/// `if ($this->impure() === 1)` proves something about one evaluation;
/// the next `$this->impure()` is another one, so the result is not a
/// subject the scope can hold on to.
pub(crate) fn forget_impure_call_results(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match condition {
        Expression::Parenthesized(inner) => {
            forget_impure_call_results(inner.expression, scope, ctx)
        }
        Expression::UnaryPrefix(unary) => forget_impure_call_results(unary.operand, scope, ctx),
        Expression::Binary(bin) => {
            forget_impure_call_results(bin.lhs, scope, ctx);
            forget_impure_call_results(bin.rhs, scope, ctx);
        }
        Expression::Call(call) => {
            let Some(key) = narrowing::expr_to_subject_key(condition) else {
                return;
            };
            if !scope.contains(&key) {
                return;
            }
            let effect = match call {
                Call::Method(MethodCall { object, method, .. })
                | Call::NullSafeMethod(NullSafeMethodCall { object, method, .. }) => {
                    let ClassLikeMemberSelector::Identifier(ident) = method else {
                        return;
                    };
                    method_call_effect(object, bytes_to_str(ident.value), scope, ctx)
                }
                Call::StaticMethod(sc) => {
                    let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                        return;
                    };
                    static_call_effect(sc.class, bytes_to_str(ident.value), ctx).0
                }
                Call::Function(fc) => {
                    let Expression::Identifier(ident) = fc.function else {
                        return;
                    };
                    function_call_effect(fc.function, bytes_to_str(ident.value()), ctx)
                }
            };
            if effect.forgets_own_result() {
                scope.remove(&key);
            }
        }
        _ => {}
    }
}

/// Drop the recorded results of the stat-cached filesystem checks, which
/// `clearstatcache()` and `unlink()` make stale.
///
/// The list is the one PHP documents for `clearstatcache()`.
fn forget_stat_cache(scope: &mut ScopeState) {
    const STAT_CACHED: &[&str] = &[
        "stat",
        "lstat",
        "file_exists",
        "is_writable",
        "is_writeable",
        "is_readable",
        "is_executable",
        "is_file",
        "is_dir",
        "is_link",
        "filectime",
        "fileatime",
        "filemtime",
        "fileinode",
        "filegroup",
        "fileowner",
        "filesize",
        "filetype",
        "fileperms",
    ];
    let is_stat_call = |key: &str| {
        let name = key.trim_start_matches('\\');
        STAT_CACHED.iter().any(|f| {
            name.len() > f.len()
                && name.as_bytes()[f.len()] == b'('
                && name[..f.len()].eq_ignore_ascii_case(f)
        })
    };
    scope.locals.retain(|key, _| !is_stat_call(key));
}

/// One subject a statement's calls may have changed.
struct Invalidation {
    /// Scope key of the object.
    subject: String,
    /// Key of the call doing the invalidating, whose own proof survives.
    made: Option<String>,
    /// Whether property paths through it go too, not just call results.
    members: bool,
    /// The method called on the subject, when it is the receiver.  A
    /// method its class inherits cannot write the class's private
    /// properties.
    method: Option<String>,
}

/// What a call does to the objects it touches.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CallEffect {
    /// Pure, or a call that computes a value and is read as doing only
    /// that.
    None,
    /// The callee could not be found.  What it recorded about the
    /// receiver's calls goes, since keeping a stale check costs
    /// correctness, but nothing else is assumed.
    Unknown,
    /// The call changes state: it returns nothing, returns `$this`, or is
    /// declared impure.
    Changes {
        /// Declared `@impure`, so even its own result is not a stable fact.
        impure: bool,
        /// Returns `$this` without being declared impure: a fluent setter
        /// changes the receiver, not what it was handed.
        fluent: bool,
    },
}

impl CallEffect {
    /// The effect of a call that may reach any of several callees.
    fn join(self, other: CallEffect) -> CallEffect {
        match (self, other) {
            (
                CallEffect::Changes { impure, fluent },
                CallEffect::Changes {
                    impure: other_impure,
                    fluent: other_fluent,
                },
            ) => CallEffect::Changes {
                impure: impure || other_impure,
                fluent: fluent && other_fluent,
            },
            (CallEffect::Changes { .. }, _) => self,
            (_, CallEffect::Changes { .. }) => other,
            (CallEffect::Unknown, _) | (_, CallEffect::Unknown) => CallEffect::Unknown,
            _ => CallEffect::None,
        }
    }

    /// Whether the call's own result is not worth remembering.
    fn forgets_own_result(self) -> bool {
        matches!(self, CallEffect::Changes { impure: true, .. })
    }

    /// Whether the objects passed to the call may have changed.
    fn changes_arguments(self) -> bool {
        matches!(self, CallEffect::Changes { fluent: false, .. })
    }
}

/// Walk `expr` for calls, collecting what each one invalidates.
fn collect_call_invalidations<'b>(
    expr: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    out: &mut Vec<Invalidation>,
) {
    match expr {
        Expression::Parenthesized(inner) => {
            collect_call_invalidations(inner.expression, scope, ctx, out)
        }
        Expression::Assignment(assignment) => {
            collect_call_invalidations(assignment.rhs, scope, ctx, out)
        }
        Expression::Binary(bin) => {
            collect_call_invalidations(bin.lhs, scope, ctx, out);
            collect_call_invalidations(bin.rhs, scope, ctx, out);
        }
        Expression::UnaryPrefix(unary) => {
            collect_call_invalidations(unary.operand, scope, ctx, out)
        }
        Expression::Instantiation(inst) => {
            let Some(args) = inst.argument_list.as_ref() else {
                return;
            };
            for arg in args.arguments.iter() {
                collect_call_invalidations(arg.value(), scope, ctx, out);
            }
            if !any_argument_has_state(args, scope) {
                return;
            }
            // A constructor that returns nothing is still not read as
            // changing its arguments: building an object from them is what
            // it is for.  Only a declared `@impure` says otherwise.
            let impure = static_receiver_class_names(inst.class, ctx)
                .iter()
                .filter_map(|name| (ctx.class_loader)(name))
                .any(|cls| declared_purity(&cls, "__construct", ctx) == Some(false));
            if impure {
                push_argument_invalidations(args, scope, out);
            }
        }
        Expression::Call(call) => {
            let (object, args) = match call {
                Call::Method(mc) => (Some(mc.object), &mc.argument_list),
                Call::NullSafeMethod(mc) => (Some(mc.object), &mc.argument_list),
                Call::Function(fc) => (None, &fc.argument_list),
                Call::StaticMethod(sc) => (None, &sc.argument_list),
            };
            // A chained call's receiver is itself a call, and an argument
            // may hold one too, so both are searched.
            if let Some(object) = object {
                collect_call_invalidations(object, scope, ctx, out);
            }

            // `(function () { … })()` runs its body right here, so a
            // state-changing call inside it reaches the same `$this` and
            // captured variables the outer scope already tracks under
            // those names, exactly as if the body were inlined at the
            // call site.
            if let Call::Function(fc) = call
                && let Expression::Closure(closure) = crate::parser::unwrap_parens(fc.function)
            {
                collect_closure_invalidations(closure, scope, ctx, out);
            }

            let mut next_positional = 0usize;
            for arg in args.arguments.iter() {
                let (arg_expr, selector) = arg_expr_and_selector(arg, &mut next_positional);
                // A callable argument the callee is known to invoke before
                // returning (`call_user_func($cb)`, `array_map($cb, …)`)
                // runs the same way; one it merely stores away for later
                // is not provably run at all, so it is left alone here.
                if let Expression::Closure(closure) = arg_expr
                    && call_invokes_arg_immediately(call, &selector, scope, ctx)
                {
                    collect_closure_invalidations(closure, scope, ctx, out);
                } else {
                    collect_call_invalidations(arg_expr, scope, ctx, out);
                }
            }

            match call {
                Call::Method(MethodCall { object, method, .. })
                | Call::NullSafeMethod(NullSafeMethodCall { object, method, .. }) => {
                    let ClassLikeMemberSelector::Identifier(ident) = method else {
                        return;
                    };
                    let receiver = narrowing::expr_to_subject_key(object);
                    let receiver_has_state = receiver
                        .as_deref()
                        .is_some_and(|r| scope_reads_through(scope, r));
                    // Checked before the class lookup below, which is the
                    // expensive half: the great majority of calls reach
                    // this and stop.
                    if !receiver_has_state && !any_argument_has_state(args, scope) {
                        return;
                    }
                    let effect = method_call_effect(object, bytes_to_str(ident.value), scope, ctx);
                    if let Some(receiver) = receiver.filter(|_| receiver_has_state) {
                        push_receiver_invalidation(
                            receiver,
                            bytes_to_str(ident.value),
                            expr,
                            effect,
                            out,
                        );
                    }
                    if effect.changes_arguments() {
                        push_argument_invalidations(args, scope, out);
                    }
                }
                Call::StaticMethod(sc) => {
                    let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                        return;
                    };
                    let method_name = bytes_to_str(ident.value);
                    // `parent::__construct()`, `self::reset()`: a call on
                    // the class the walk is inside reaches `$this`.
                    let on_this = matches!(
                        sc.class,
                        Expression::Parent(_) | Expression::Self_(_) | Expression::Static(_)
                    ) && scope_reads_through(scope, "$this");
                    if !on_this && !any_argument_has_state(args, scope) {
                        return;
                    }
                    let (effect, is_static) = static_call_effect(sc.class, method_name, ctx);
                    if on_this && !is_static {
                        let effect = if method_name.eq_ignore_ascii_case("__construct") {
                            CallEffect::Changes {
                                impure: false,
                                fluent: false,
                            }
                        } else {
                            effect
                        };
                        push_receiver_invalidation(
                            "$this".to_string(),
                            method_name,
                            expr,
                            effect,
                            out,
                        );
                    }
                    if effect.changes_arguments() {
                        push_argument_invalidations(args, scope, out);
                    }
                }
                Call::Function(fc) => {
                    let Expression::Identifier(ident) = fc.function else {
                        return;
                    };
                    let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
                    if name.eq_ignore_ascii_case("clearstatcache")
                        || name.eq_ignore_ascii_case("unlink")
                    {
                        forget_stat_cache(scope);
                    }
                    if !any_argument_has_state(args, scope) {
                        return;
                    }
                    let effect =
                        function_call_effect(fc.function, bytes_to_str(ident.value()), ctx);
                    if effect.changes_arguments() {
                        push_argument_invalidations(args, scope, out);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Walk a closure body that is provably invoked before the enclosing call
/// returns for the calls inside it, feeding into the same collector as if
/// the body were inlined at the call site.
///
/// `$this` is captured implicitly (unless the closure is `static`) and a
/// `use (…)` variable by name, so both are exactly the scope keys the
/// outer scope already tracks under; no capture translation is needed.
/// But a bare variable inside the closure body that names neither is not
/// a capture at all, only a same-spelled local of its own (PHP closures
/// see no outer variable without `use`) — `function () { $s = new
/// self(); $s->stop(); }` calls `stop()` on a fresh object, not whatever
/// the caller's `$s` was, so an invalidation on anything but a captured
/// name is dropped rather than applied to the outer scope's variable of
/// the same spelling.
fn collect_closure_invalidations<'b>(
    closure: &'b Closure<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    out: &mut Vec<Invalidation>,
) {
    let mut captured: Vec<String> = closure
        .use_clause
        .as_ref()
        .map(|use_clause| {
            use_clause
                .variables
                .iter()
                .map(|use_var| bytes_to_str(use_var.variable.name).to_string())
                .collect()
        })
        .unwrap_or_default();
    if closure.r#static.is_none() {
        captured.push("$this".to_string());
    }
    if captured.is_empty() {
        return;
    }

    let mut closure_out = Vec::new();
    collect_call_invalidations_in_stmts(
        closure.body.statements.as_slice(),
        scope,
        ctx,
        &mut closure_out,
    );
    out.extend(closure_out.into_iter().filter(|invalidation| {
        captured.iter().any(|name| {
            invalidation.subject == *name
                || narrowing::key_reads_variable(&invalidation.subject, name)
        })
    }));
}

fn collect_call_invalidations_in_stmts<'b>(
    stmts: &'b [Statement<'b>],
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    out: &mut Vec<Invalidation>,
) {
    for stmt in stmts {
        collect_call_invalidations_in_stmt(stmt, scope, ctx, out);
    }
}

/// [`collect_call_invalidations`] for a statement, recursing into whatever
/// nested statements and conditions it carries.  Mirrors the coverage of
/// [`super::assignment_deps::collect_assignment_deps`], plus `return` and
/// `echo`, which commonly carry the call a closure exists to make.
fn collect_call_invalidations_in_stmt<'b>(
    stmt: &'b Statement<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    out: &mut Vec<Invalidation>,
) {
    match stmt {
        Statement::Expression(expr_stmt) => {
            collect_call_invalidations(expr_stmt.expression, scope, ctx, out);
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                collect_call_invalidations(val, scope, ctx, out);
            }
        }
        Statement::Echo(echo) => {
            for value in echo.values.iter() {
                collect_call_invalidations(value, scope, ctx, out);
            }
        }
        Statement::Block(block) => {
            collect_call_invalidations_in_stmts(block.statements.as_slice(), scope, ctx, out);
        }
        Statement::If(if_stmt) => {
            collect_call_invalidations(if_stmt.condition, scope, ctx, out);
            collect_call_invalidations_in_stmts(if_stmt.body.statements(), scope, ctx, out);
            for (condition, stmts) in if_stmt.body.else_if_clauses() {
                collect_call_invalidations(condition, scope, ctx, out);
                collect_call_invalidations_in_stmts(stmts, scope, ctx, out);
            }
            if let Some(stmts) = if_stmt.body.else_statements() {
                collect_call_invalidations_in_stmts(stmts, scope, ctx, out);
            }
        }
        Statement::Try(try_stmt) => {
            collect_call_invalidations_in_stmts(
                try_stmt.block.statements.as_slice(),
                scope,
                ctx,
                out,
            );
            for catch in try_stmt.catch_clauses.iter() {
                collect_call_invalidations_in_stmts(
                    catch.block.statements.as_slice(),
                    scope,
                    ctx,
                    out,
                );
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                collect_call_invalidations_in_stmts(
                    finally.block.statements.as_slice(),
                    scope,
                    ctx,
                    out,
                );
            }
        }
        Statement::Switch(switch) => {
            collect_call_invalidations(switch.expression, scope, ctx, out);
            for case in switch.body.cases().iter() {
                if let Some(condition) = case.expression() {
                    collect_call_invalidations(condition, scope, ctx, out);
                }
                collect_call_invalidations_in_stmts(case.statements(), scope, ctx, out);
            }
        }
        Statement::Foreach(f) => {
            collect_call_invalidations(f.expression, scope, ctx, out);
            collect_call_invalidations_in_stmts(f.body.statements(), scope, ctx, out);
        }
        Statement::While(w) => {
            collect_call_invalidations(w.condition, scope, ctx, out);
            collect_call_invalidations_in_stmts(w.body.statements(), scope, ctx, out);
        }
        Statement::For(f) => {
            for condition in f.conditions.iter() {
                collect_call_invalidations(condition, scope, ctx, out);
            }
            collect_call_invalidations_in_stmts(f.body.statements(), scope, ctx, out);
        }
        Statement::DoWhile(dw) => {
            collect_call_invalidations_in_stmt(dw.statement, scope, ctx, out);
            collect_call_invalidations(dw.condition, scope, ctx, out);
        }
        _ => {}
    }
}

/// Record what `effect` invalidates on the receiver of the call `call`.
fn push_receiver_invalidation(
    receiver: String,
    method: &str,
    call: &Expression<'_>,
    effect: CallEffect,
    out: &mut Vec<Invalidation>,
) {
    let members = match effect {
        CallEffect::None => return,
        CallEffect::Unknown => false,
        CallEffect::Changes { .. } => true,
    };
    let made = if effect.forgets_own_result() {
        None
    } else {
        narrowing::expr_to_subject_key(call)
    };
    push_unique(
        out,
        Invalidation {
            subject: receiver,
            made,
            members,
            method: Some(method.to_string()),
        },
    );
}

/// Record that every argument with something recorded through it may have
/// changed.  A scalar passed by value cannot change, and one passed by
/// reference is rewritten by the by-reference pass, so only what is read
/// through an argument (its properties and calls) is at stake.
fn push_argument_invalidations(
    args: &ArgumentList<'_>,
    scope: &ScopeState,
    out: &mut Vec<Invalidation>,
) {
    for arg in args.arguments.iter() {
        let Some(key) = narrowing::expr_to_subject_key(arg.value()) else {
            continue;
        };
        if !scope_reads_through(scope, &key) {
            continue;
        }
        push_unique(
            out,
            Invalidation {
                subject: key,
                made: None,
                members: true,
                method: None,
            },
        );
    }
}

fn push_unique(out: &mut Vec<Invalidation>, invalidation: Invalidation) {
    if let Some(existing) = out
        .iter_mut()
        .find(|i| i.subject == invalidation.subject && i.made == invalidation.made)
    {
        existing.members |= invalidation.members;
        // Passed as an argument as well as called on: the callee it was
        // passed to may write anything.
        if existing.method != invalidation.method {
            existing.method = None;
        }
    } else {
        out.push(invalidation);
    }
}

/// Whether any argument has something recorded through it.
fn any_argument_has_state(args: &ArgumentList<'_>, scope: &ScopeState) -> bool {
    args.arguments.iter().any(|arg| {
        narrowing::expr_to_subject_key(arg.value())
            .is_some_and(|key| scope_reads_through(scope, &key))
    })
}

/// Whether the scope holds any synthetic key read through `subject`.
fn scope_reads_through(scope: &ScopeState, subject: &str) -> bool {
    let reads = |key: &str| {
        key != subject && crate::type_engine::types::narrowing::key_reads_variable(key, subject)
    };
    scope.locals.keys().any(|k| reads(k))
        || scope
            .assertions
            .values()
            .any(|checks| checks.iter().any(|c| reads(&c.subject)))
}

/// The object whose property the assignment `expr` writes, as a scope key:
/// `$x` for `$x->prop = …` and for `$x->prop['k'][] = …`.
fn written_object(expr: &Expression<'_>) -> Option<String> {
    let Expression::Assignment(assignment) = crate::parser::unwrap_parens(expr) else {
        return None;
    };
    let mut target = assignment.lhs;
    loop {
        match target {
            Expression::ArrayAccess(access) => target = access.array,
            Expression::ArrayAppend(append) => target = append.array,
            Expression::Access(Access::Property(pa)) => {
                return narrowing::expr_to_subject_key(pa.object);
            }
            Expression::Access(Access::NullSafeProperty(pa)) => {
                return narrowing::expr_to_subject_key(pa.object);
            }
            _ => return None,
        }
    }
}

/// The keys of `subject`'s properties that the scope records and that the
/// call cannot have written.
///
/// A readonly property is set once.  A private one is only reachable from
/// its own class, so a method the class inherits (`parent::reset()`, or a
/// `void` helper a base class declares) leaves it alone; PHPStan keeps both.
fn untouched_property_keys(
    subject: &str,
    method: Option<&str>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<String> {
    let prefix = format!("{subject}->");
    let props: Vec<&str> = scope
        .locals
        .keys()
        .filter_map(|key| key.strip_prefix(prefix.as_str()))
        .filter(|prop| prop.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .collect();
    if props.is_empty() {
        return Vec::new();
    }
    let classes: Vec<Arc<crate::types::ClassInfo>> = subject_class_names(subject, scope, ctx)
        .iter()
        .filter_map(|name| (ctx.class_loader)(name))
        .collect();
    if classes.is_empty() {
        return Vec::new();
    }
    props
        .into_iter()
        .filter(|prop| {
            classes.iter().all(|cls| {
                let private_out_of_reach = method.is_some_and(|m| {
                    cls.properties.iter().any(|p| {
                        p.name.as_str() == *prop
                            && p.visibility == crate::types::Visibility::Private
                    }) && inherits_method(cls, m, ctx)
                });
                if private_out_of_reach {
                    return true;
                }
                {
                    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                        cls,
                        ctx.class_loader,
                        ctx.resolved_class_cache,
                    );
                    merged
                        .get_property(prop)
                        .is_some_and(|p| p.is_readonly || merged.is_readonly)
                }
            })
        })
        .map(|prop| format!("{prefix}{prop}"))
        .collect()
}

/// Whether `cls` gets `method` from an ancestor rather than declaring it:
/// no definition of its own or from a trait, but one up the parent chain.
///
/// A method only its docblock declares (`@method $this reset()`) belongs to
/// the class, which is why this asks the parents rather than looking for
/// the method among the class's parsed ones.
fn inherits_method(cls: &crate::types::ClassInfo, method: &str, ctx: &ForwardWalkCtx<'_>) -> bool {
    if declares_method(cls, method, ctx) {
        return false;
    }
    let Some(parent) = cls
        .parent_class
        .as_ref()
        .and_then(|p| (ctx.class_loader)(p))
    else {
        return false;
    };
    narrowing::find_method_in_chain_where(
        &parent,
        method,
        ctx.class_loader,
        &|_| true,
        &mut Vec::new(),
        0,
    )
    .is_some()
}

/// Whether `cls` declares `method` itself, directly or through a trait it
/// uses (a trait method runs as the class's own).
fn declares_method(cls: &crate::types::ClassInfo, method: &str, ctx: &ForwardWalkCtx<'_>) -> bool {
    fn walk(
        cls: &crate::types::ClassInfo,
        method: &str,
        ctx: &ForwardWalkCtx<'_>,
        visited: &mut Vec<crate::atom::Atom>,
    ) -> bool {
        if cls
            .methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(method))
        {
            return true;
        }
        cls.used_traits.iter().any(|name| {
            if visited.contains(name) {
                return false;
            }
            visited.push(*name);
            (ctx.class_loader)(name).is_some_and(|t| walk(&t, method, ctx, visited))
        })
    }
    walk(cls, method, ctx, &mut Vec::new())
}

/// The classes the scope says `subject` may be an instance of.
fn subject_class_names(subject: &str, scope: &ScopeState, ctx: &ForwardWalkCtx<'_>) -> Vec<String> {
    if subject == "$this" {
        return vec![ctx.current_class.fqn().to_string()];
    }
    scope
        .get(subject)
        .iter()
        .filter_map(|rt| rt.type_string.base_name().map(str::to_owned))
        .collect()
}

/// What calling `object->method_name()` does.
///
/// Three signals, in order of authority, the same ones PHPStan reads
/// (`MethodReflection::hasSideEffects()`): a method that returns nothing
/// was called for its effect; `@pure` / `@impure` (and their `phpstan-` and
/// `psalm-` spellings, on the method or the one it overrides) say outright;
/// and a method that returns `$this` is a fluent setter.  Anything else
/// computes a value and is read as computing it, which is what keeps
/// guard-then-read on two getters of the same object working.
fn method_call_effect(
    object: &Expression<'_>,
    method_name: &str,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> CallEffect {
    let class_names: Vec<String> = match object {
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
            vec![ctx.current_class.fqn().to_string()]
        }
        _ => {
            let Some(key) = narrowing::expr_to_subject_key(object) else {
                return CallEffect::Unknown;
            };
            subject_class_names(&key, scope, ctx)
        }
    };
    if class_names.is_empty() {
        return CallEffect::Unknown;
    }
    class_names
        .iter()
        .map(|name| match (ctx.class_loader)(name) {
            Some(cls) => class_method_effect(&cls, method_name, ctx).0,
            None => CallEffect::Unknown,
        })
        .reduce(CallEffect::join)
        .unwrap_or(CallEffect::Unknown)
}

/// What calling `Class::method_name()` does, and whether the method is
/// static (a static method has no `$this` to change).
fn static_call_effect(
    class: &Expression<'_>,
    method_name: &str,
    ctx: &ForwardWalkCtx<'_>,
) -> (CallEffect, bool) {
    let classes: Vec<Arc<crate::types::ClassInfo>> = static_receiver_class_names(class, ctx)
        .iter()
        .filter_map(|name| (ctx.class_loader)(name))
        .collect();
    let Some(cls) = classes.first() else {
        return (CallEffect::Unknown, false);
    };
    class_method_effect(cls, method_name, ctx)
}

/// [`method_call_effect`] for one class, plus whether the method is static.
fn class_method_effect(
    cls: &crate::types::ClassInfo,
    method_name: &str,
    ctx: &ForwardWalkCtx<'_>,
) -> (CallEffect, bool) {
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        cls,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    let Some(method) = merged.get_method_ci(method_name) else {
        return (CallEffect::Unknown, false);
    };
    let purity = if method.is_pure || method.is_impure {
        Some(method.is_pure)
    } else {
        declared_purity(cls, method_name, ctx)
    };
    let returns = method.return_type.as_ref();
    let effect = if !method_name.eq_ignore_ascii_case("__construct")
        && returns.is_some_and(|rt| rt.is_void() || rt.is_never())
    {
        CallEffect::Changes {
            impure: purity == Some(false),
            fluent: false,
        }
    } else {
        match purity {
            Some(true) => CallEffect::None,
            Some(false) => CallEffect::Changes {
                impure: true,
                fluent: false,
            },
            None if returns.is_some_and(returns_this) => CallEffect::Changes {
                impure: false,
                fluent: true,
            },
            None => CallEffect::None,
        }
    };
    (effect, method.is_static)
}

/// What calling the function `name` does: the same signals as a method,
/// minus the fluent one.  A function that cannot be found changes nothing
/// the walker would drop, since nothing it was handed is known to change.
fn function_call_effect(
    function: &Expression<'_>,
    name: &str,
    ctx: &ForwardWalkCtx<'_>,
) -> CallEffect {
    let Some(loader) = ctx.loaders.function_loader else {
        return CallEffect::None;
    };
    let Some(info) = loader(name, function.span().start.offset) else {
        return CallEffect::None;
    };
    if info
        .return_type
        .as_ref()
        .is_some_and(|rt| rt.is_void() || rt.is_never())
    {
        return CallEffect::Changes {
            impure: info.is_impure,
            fluent: false,
        };
    }
    if info.is_impure && !info.is_pure {
        return CallEffect::Changes {
            impure: true,
            fluent: false,
        };
    }
    CallEffect::None
}

/// Whether the definition of `method_name` closest to `cls` that says so
/// declares it pure (`Some(true)`) or impure (`Some(false)`).
///
/// An override without a tag keeps the promise of the method it overrides,
/// the way PHPStan reads it.
fn declared_purity(
    cls: &crate::types::ClassInfo,
    method_name: &str,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<bool> {
    narrowing::find_method_in_chain_where(
        cls,
        method_name,
        ctx.class_loader,
        &|m| m.is_pure || m.is_impure,
        &mut Vec::new(),
        0,
    )
    .map(|(method, _)| method.is_pure)
}

/// Whether a return type is `$this`, the fluent-setter signature.
fn returns_this(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::ThisType(_) => true,
        TypeKind::Named(name) => name.eq_ignore_ascii_case("$this"),
        _ => false,
    }
}

pub(crate) fn receiver_class_names(
    expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<String> {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            let var_name = bytes_to_str(dv.name);
            if var_name == "$this" && !ctx.current_class.name.is_empty() {
                return vec![
                    ctx.current_class.name.to_string(),
                    ctx.current_class.fqn().to_string(),
                ];
            }
            scope
                .get(var_name)
                .iter()
                .filter_map(|rt| rt.class_info.as_ref())
                .flat_map(|cls| [cls.name.to_string(), cls.fqn().to_string()])
                .collect()
        }
        Expression::Parenthesized(inner) => receiver_class_names(inner.expression, scope, ctx),
        _ => Vec::new(),
    }
}

pub(crate) fn static_receiver_class_names(
    expr: &Expression<'_>,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<String> {
    match expr {
        Expression::Self_(_) | Expression::Static(_) if !ctx.current_class.name.is_empty() => {
            vec![
                ctx.current_class.name.to_string(),
                ctx.current_class.fqn().to_string(),
            ]
        }
        Expression::Parent(_) => ctx
            .current_class
            .parent_class
            .map(|name| vec![name.to_string()])
            .unwrap_or_default(),
        Expression::Identifier(ident) => vec![bytes_to_str(ident.value()).to_string()],
        Expression::Parenthesized(inner) => static_receiver_class_names(inner.expression, ctx),
        _ => Vec::new(),
    }
}

pub(crate) fn class_name_matches_receiver(name: &[u8], receiver_names: &[String]) -> bool {
    let class_name = bytes_to_str(name);
    receiver_names.iter().any(|receiver| {
        receiver.eq_ignore_ascii_case(class_name)
            || crate::util::short_name(receiver).eq_ignore_ascii_case(class_name)
    })
}
