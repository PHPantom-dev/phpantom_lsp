//! Whether a statement list leaves the block it sits in: `return`,
//! `throw`, `exit`, a `continue` or `break`, or a call to a function or
//! method declared to return `never`.  Guard clauses narrow the code after
//! them only when their body leaves.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use mago_span::{HasSpan, Span};
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::type_engine::resolver::{FunctionLoaderFn, ScopeVarResolverFn};
use crate::types::{ClassInfo, ResolvedType};

/// Whether every path through `stmts` leaves the block they sit in (see
/// [`statements_leave_block`]), counting a call to a `never`-returning
/// function or method as leaving it.
pub(in crate::type_engine) fn statements_unconditionally_exit<'s>(
    stmts: impl Iterator<Item = &'s Statement<'s>>,
    ctx: &ExitCtx<'_>,
) -> bool {
    statements_leave_block(stmts, &|expr| expression_is_never_call(expr, ctx))
}

/// Whether every path through `stmt` leaves the statement list it sits in.
///
/// `return`, `throw`, `exit`, `die`, `continue` and `break` leave; so does
/// a block whose statements do (see [`statements_leave_block`]) and an
/// `if` whose every branch does, which needs an `else`.  `goto` does not
/// count: its label may sit further down the same list, in which case the
/// list keeps running.
///
/// `never_call` recognises an expression statement that calls a
/// `never`-returning function.  That takes types, so a caller with none
/// to hand passes `&|_| false`.
pub(crate) fn statement_leaves_block(
    stmt: &Statement<'_>,
    never_call: &dyn Fn(&Expression<'_>) -> bool,
) -> bool {
    match stmt {
        Statement::Return(_) | Statement::Continue(_) | Statement::Break(_) => true,
        // `throw`, `exit` and `die` are expressions in PHP, so they
        // reach here wrapped in an expression statement.
        Statement::Expression(es) => {
            matches!(
                es.expression,
                Expression::Throw(_)
                    | Expression::Construct(Construct::Exit(_))
                    | Expression::Construct(Construct::Die(_))
            ) || never_call(es.expression)
        }
        Statement::Block(block) => statements_leave_block(block.statements.iter(), never_call),
        // An if/else exits if ALL branches exist and ALL exit.
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => {
                statement_leaves_block(body.statement, never_call)
                    && body
                        .else_if_clauses
                        .iter()
                        .all(|ei| statement_leaves_block(ei.statement, never_call))
                    && body
                        .else_clause
                        .as_ref()
                        .is_some_and(|ec| statement_leaves_block(ec.statement, never_call))
            }
            IfBody::ColonDelimited(body) => {
                statements_leave_block(body.statements.iter(), never_call)
                    && body
                        .else_if_clauses
                        .iter()
                        .all(|ei| statements_leave_block(ei.statements.iter(), never_call))
                    && body
                        .else_clause
                        .as_ref()
                        .is_some_and(|ec| statements_leave_block(ec.statements.iter(), never_call))
            }
        },
        _ => false,
    }
}

/// Whether control is gone by the end of a statement list.
///
/// Everything after the first statement that leaves is unreachable, so a
/// trailing assignment does not make the list fall through.  A `goto`
/// label is the exception: it is an entry point, so control can be back
/// in the list after the exit and run out of its end.  That holds for a
/// label nested inside a later statement too (see
/// [`contains_entry_label`]), in which case whether the list ends
/// reachable is that statement's own answer.
pub(crate) fn statements_leave_block<'s>(
    stmts: impl Iterator<Item = &'s Statement<'s>>,
    never_call: &dyn Fn(&Expression<'_>) -> bool,
) -> bool {
    let mut reachable = true;
    for stmt in stmts {
        if matches!(stmt, Statement::Label(_)) {
            reachable = true;
        } else if reachable || contains_entry_label(stmt) {
            reachable = !statement_leaves_block(stmt, never_call);
        }
    }
    !reachable
}

/// Whether a `goto` can land somewhere inside this statement.
///
/// PHP lets a jump enter a block, an `if` branch, a `try` or `catch` body
/// and a `declare` body, but not a loop, a `switch`, a `finally` or another
/// function, so only the former are searched.
pub(crate) fn contains_entry_label(stmt: &Statement<'_>) -> bool {
    let any = |stmts: &[Statement<'_>]| stmts.iter().any(contains_entry_label);
    match stmt {
        Statement::Label(_) => true,
        Statement::Block(block) => any(block.statements.as_slice()),
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => {
                contains_entry_label(body.statement)
                    || body
                        .else_if_clauses
                        .iter()
                        .any(|ei| contains_entry_label(ei.statement))
                    || body
                        .else_clause
                        .as_ref()
                        .is_some_and(|ec| contains_entry_label(ec.statement))
            }
            IfBody::ColonDelimited(body) => {
                any(body.statements.as_slice())
                    || body
                        .else_if_clauses
                        .iter()
                        .any(|ei| any(ei.statements.as_slice()))
                    || body
                        .else_clause
                        .as_ref()
                        .is_some_and(|ec| any(ec.statements.as_slice()))
            }
        },
        Statement::Try(try_stmt) => {
            any(try_stmt.block.statements.as_slice())
                || try_stmt
                    .catch_clauses
                    .iter()
                    .any(|catch| any(catch.block.statements.as_slice()))
        }
        Statement::Declare(declare) => match &declare.body {
            DeclareBody::Statement(body) => contains_entry_label(body),
            DeclareBody::ColonDelimited(body) => any(body.statements.as_slice()),
        },
        _ => false,
    }
}

/// The type information [`statements_unconditionally_exit`] needs to
/// recognise a call to a `never`-returning function or method.
///
/// Every consumer that asks "does this branch terminate?" builds one
/// from its own resolution context, so a guard clause ending in
/// `abort()` terminates the branch wherever the question comes from.
pub(in crate::type_engine) struct ExitCtx<'a> {
    /// Enclosing class, used for `$this->…`, `self::`, `static::` and
    /// `parent::` receivers and for namespace-relative name resolution.
    pub current_class: &'a ClassInfo,
    /// The file's classes, so a name resolves to the one its own
    /// namespace block declares.
    pub all_classes: &'a [Arc<ClassInfo>],
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// See [`ResolutionCtx::source_class_loader`](crate::type_engine::resolver::ResolutionCtx::source_class_loader).
    pub source_class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// See [`ResolutionCtx::source_imports`](crate::type_engine::resolver::ResolutionCtx::source_imports).
    pub source_imports: crate::type_engine::resolver::SourceImports<'a>,
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
    /// Resolves the class expression of a static call that is not a class
    /// name (`$class::fail()`, `$this->model::fail()`) to the classes it
    /// names, so the call terminates a branch the same way
    /// `Aborter::fail()` does.
    pub static_receiver_resolver: ReceiverResolverFn<'a>,
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
                Expression::Identifier(ident) => crate::names::resolve_code_class_name(
                    bytes_to_str(ident.value()),
                    ctx.current_class.file_namespace.as_deref(),
                    ctx.source_imports,
                    ctx.all_classes,
                    ctx.source_class_loader,
                ),
                // `never` is the bottom type, so a child override can
                // only narrow to `never` again — reading `static::`
                // off the current class is sound.
                Expression::Self_(_) | Expression::Static(_) => ctx.current_class.fqn().to_string(),
                Expression::Parent(_) => match ctx.current_class.parent_class.as_ref() {
                    Some(parent) => parent.to_string(),
                    None => return false,
                },
                class_expr => {
                    let Some(static_receiver_resolver) = ctx.static_receiver_resolver else {
                        return false;
                    };
                    let Some(_guard) = try_acquire_receiver_guard(class_expr.span()) else {
                        return false;
                    };
                    let class_names = static_receiver_resolver(class_expr);
                    return !class_names.is_empty()
                        && class_names.iter().all(|class_name| {
                            class_has_never_method(class_name, method_name, ctx)
                        });
                }
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
/// A variable is read from the caller's scope when one was supplied.
/// `$this` is the enclosing class unless the scope holds the object a
/// closure was bound to instead (see [`this_is_rebound`]).  Anything else
/// (a property chain, a call result, a `new` expression) goes through the
/// caller's [`ReceiverResolverFn`], guarded against the cycle that
/// resolving an expression from a control-flow predicate can form.
fn receiver_class_names(object: &Expression<'_>, ctx: &ExitCtx<'_>) -> Vec<String> {
    if let Expression::Variable(Variable::Direct(dv)) = object {
        let held = ctx
            .var_types
            .map(|var_types| var_types(bytes_to_str(dv.name)))
            .unwrap_or_default();
        if dv.name == b"$this" && !this_is_rebound(&held, ctx.current_class) {
            return vec![ctx.current_class.fqn().to_string()];
        }
        return class_names_of(&held);
    }
    let Some(receiver_resolver) = ctx.receiver_resolver else {
        return Vec::new();
    };
    let Some(_guard) = try_acquire_receiver_guard(object.span()) else {
        return Vec::new();
    };
    receiver_resolver(object)
}

/// Whether the scope's `$this` (`held`) is an object a closure was bound to
/// (`Closure::call()`, `@param-closure-this`) rather than the enclosing
/// class.
///
/// Inside a class the scope's `$this` lists the class, followed by the
/// classes its traits require of it.  Those all describe the one object, so
/// the class alone is asked whether the method is `never`, rather than each
/// of them as if `$this` might be any one of them.
fn this_is_rebound(held: &[ResolvedType], current_class: &ClassInfo) -> bool {
    let enclosing = current_class.fqn();
    !held.is_empty()
        && !held.iter().any(|rt| {
            rt.class_info
                .as_ref()
                .is_some_and(|class| class.fqn() == enclosing)
        })
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
