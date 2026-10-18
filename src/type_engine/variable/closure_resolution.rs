//! Closure and arrow-function helpers for the forward walker.
//!
//! This module provides:
//!
//! - **`@param-closure-this` resolution:** detects when the cursor is
//!   inside a closure whose enclosing call site declares a
//!   `@param-closure-this` tag and overrides `$this` accordingly.
//! - **Callable parameter inference helpers:** shared receiver
//!   resolution and generic-argument recovery used by
//!   `forward_walk/callable_inference.rs` to infer untyped
//!   closure/arrow-function parameter types from the enclosing callable
//!   signature (e.g. `$users->map(fn($u) => …)` infers `$u` from the
//!   `map` method's parameter type).

use std::cell::RefCell;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use crate::atom::{atom, bytes_to_str};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN;

thread_local! {
    /// Positions currently being resolved by
    /// [`find_closure_this_override`], as `(source address, cursor
    /// offset)` pairs.
    ///
    /// The override check re-parses the program and resolves the
    /// receiver of the enclosing call expression.  If the receiver
    /// is `$this`, that triggers `resolve_target_classes_expr` →
    /// `SubjectExpr::This` → `find_closure_this_override` again.
    ///
    /// Re-entry on a *different* position is legitimate work that must
    /// proceed: for a closure nested inside another closure, resolving
    /// the inner call's `$this` receiver re-enters at a position inside
    /// the outer closure body, which is exactly where the outer
    /// `@param-closure-this` applies.  Only re-entry on the *same*
    /// position is a cycle; returning `None` there breaks it and leaves
    /// the receiver on the normal `current_class` fallback.
    static CLOSURE_THIS_IN_PROGRESS: RefCell<Vec<(usize, u32)>> =
        const { RefCell::new(Vec::new()) };
}

/// RAII entry in [`CLOSURE_THIS_IN_PROGRESS`].
struct ClosureThisGuard {
    key: (usize, u32),
}

impl ClosureThisGuard {
    /// Register the position `ctx` resolves, or return `None` when that
    /// same position is already on the stack (a cycle).
    ///
    /// The source address is part of the key so that the same offset in
    /// a different file is not mistaken for re-entry.  Both `&str`s are
    /// alive for the whole nested call tree, so their addresses cannot
    /// alias.
    fn enter(ctx: &ResolutionCtx<'_>) -> Option<Self> {
        let key = (ctx.content.as_ptr() as usize, ctx.cursor_offset);
        CLOSURE_THIS_IN_PROGRESS.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.contains(&key) {
                return None;
            }
            stack.push(key);
            Some(Self { key })
        })
    }
}

impl Drop for ClosureThisGuard {
    fn drop(&mut self) {
        CLOSURE_THIS_IN_PROGRESS.with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(pos) = stack.iter().rposition(|k| *k == self.key) {
                stack.remove(pos);
            }
        });
    }
}

use crate::parser::with_parsed_program;
use crate::types::{ClassInfo, FunctionInfo, MethodInfo, ResolvedType};

// ─── @param-closure-this resolution ─────────────────────────────────────────

/// Check whether the cursor is inside a closure that is passed as an
/// argument to a function/method whose parameter carries a
/// `@param-closure-this` annotation.  If so, resolve the declared type
/// and return it as a `ClassInfo`.
///
/// This is the static-analysis equivalent of `Closure::bindTo()`:
/// frameworks like Laravel rebind closures so that `$this` inside the
/// closure body refers to a different object.  The
/// `@param-closure-this` PHPDoc tag declares what `$this` should
/// resolve to.
pub(crate) fn find_closure_this_override(ctx: &ResolutionCtx<'_>) -> Option<Arc<ClassInfo>> {
    find_closure_this_types(ctx)?
        .into_iter()
        .find_map(|rt| rt.class_info)
}

/// What `$this` is bound to inside the closure at the cursor, when a call
/// rebinds it: a `@param-closure-this` parameter the closure is passed to,
/// or `Closure::call()` invoked on the closure itself.
///
/// Unlike [`find_closure_this_override`] this keeps the whole type, so an
/// object narrowed to an intersection before `->call($obj)` stays one.
pub(crate) fn find_closure_this_types(ctx: &ResolutionCtx<'_>) -> Option<Vec<ResolvedType>> {
    let _guard = ClosureThisGuard::enter(ctx)?;

    with_parsed_program(ctx.content, "find_closure_this_override", |program, _| {
        for stmt in program.statements.iter() {
            if let Some(result) = walk_stmt_for_closure_this(stmt, ctx) {
                return Some(result);
            }
        }
        None
    })
}

/// Recursively walk a statement looking for a closure argument that
/// contains the cursor and whose receiving parameter has
/// `closure_this_type`.
fn walk_stmt_for_closure_this(
    stmt: &Statement<'_>,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let sp = stmt.span();
    if ctx.cursor_offset < sp.start.offset || ctx.cursor_offset > sp.end.offset {
        return None;
    }

    match stmt {
        Statement::Class(class) => {
            let start = class.left_brace.start.offset;
            let end = class.right_brace.end.offset;
            if ctx.cursor_offset < start || ctx.cursor_offset > end {
                return None;
            }
            for member in class.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(body) = &method.body
                {
                    let bsp = body.span();
                    if ctx.cursor_offset >= bsp.start.offset && ctx.cursor_offset <= bsp.end.offset
                    {
                        for inner in body.statements.iter() {
                            if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                                return Some(r);
                            }
                        }
                    }
                }
            }
            None
        }
        Statement::Expression(expr_stmt) => walk_expr_for_closure_this(expr_stmt.expression, ctx),
        Statement::Return(ret) => ret
            .value
            .as_ref()
            .and_then(|v| walk_expr_for_closure_this(v, ctx)),
        Statement::Function(func) => {
            let bsp = func.body.span();
            if ctx.cursor_offset >= bsp.start.offset && ctx.cursor_offset <= bsp.end.offset {
                for inner in func.body.statements.iter() {
                    if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                        return Some(r);
                    }
                }
            }
            None
        }
        _ => crate::parser::find_map_nested_statement(stmt, |inner| {
            walk_stmt_for_closure_this(inner, ctx)
        }),
    }
}

/// Walk an expression looking for a call whose closure argument
/// contains the cursor and whose parameter has `closure_this_type`.
fn walk_expr_for_closure_this(
    expr: &Expression<'_>,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let sp = expr.span();
    if ctx.cursor_offset < sp.start.offset || ctx.cursor_offset > sp.end.offset {
        return None;
    }

    match expr {
        Expression::Call(call) => walk_call_for_closure_this(call, ctx),
        Expression::Parenthesized(p) => walk_expr_for_closure_this(p.expression, ctx),
        Expression::Assignment(a) => walk_expr_for_closure_this(a.lhs, ctx)
            .or_else(|| walk_expr_for_closure_this(a.rhs, ctx)),
        Expression::Binary(bin) => walk_expr_for_closure_this(bin.lhs, ctx)
            .or_else(|| walk_expr_for_closure_this(bin.rhs, ctx)),
        Expression::Conditional(cond) => walk_expr_for_closure_this(cond.condition, ctx)
            .or_else(|| cond.then.and_then(|e| walk_expr_for_closure_this(e, ctx)))
            .or_else(|| walk_expr_for_closure_this(cond.r#else, ctx)),
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                let found = match elem {
                    ArrayElement::KeyValue(kv) => walk_expr_for_closure_this(kv.key, ctx)
                        .or_else(|| walk_expr_for_closure_this(kv.value, ctx)),
                    ArrayElement::Value(v) => walk_expr_for_closure_this(v.value, ctx),
                    ArrayElement::Variadic(v) => walk_expr_for_closure_this(v.value, ctx),
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
                    ArrayElement::KeyValue(kv) => walk_expr_for_closure_this(kv.key, ctx)
                        .or_else(|| walk_expr_for_closure_this(kv.value, ctx)),
                    ArrayElement::Value(v) => walk_expr_for_closure_this(v.value, ctx),
                    ArrayElement::Variadic(v) => walk_expr_for_closure_this(v.value, ctx),
                    _ => None,
                };
                if found.is_some() {
                    return found;
                }
            }
            None
        }
        Expression::Match(m) => {
            if let Some(r) = walk_expr_for_closure_this(m.expression, ctx) {
                return Some(r);
            }
            for arm in m.arms.iter() {
                if let Some(r) = walk_expr_for_closure_this(arm.expression(), ctx) {
                    return Some(r);
                }
            }
            None
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => walk_expr_for_closure_this(pa.object, ctx),
            Access::NullSafeProperty(pa) => walk_expr_for_closure_this(pa.object, ctx),
            Access::StaticProperty(pa) => walk_expr_for_closure_this(pa.class, ctx),
            Access::ClassConstant(pa) => walk_expr_for_closure_this(pa.class, ctx),
        },
        Expression::Instantiation(inst) => {
            if let Some(ref args) = inst.argument_list {
                walk_args_for_closure_this(&args.arguments, ctx, &|_| None)
            } else {
                None
            }
        }
        Expression::UnaryPrefix(u) => walk_expr_for_closure_this(u.operand, ctx),
        Expression::UnaryPostfix(u) => walk_expr_for_closure_this(u.operand, ctx),
        Expression::Yield(y) => match y {
            Yield::Value(yv) => yv
                .value
                .as_ref()
                .and_then(|v| walk_expr_for_closure_this(v, ctx)),
            Yield::Pair(yp) => walk_expr_for_closure_this(yp.key, ctx)
                .or_else(|| walk_expr_for_closure_this(yp.value, ctx)),
            Yield::From(yf) => walk_expr_for_closure_this(yf.iterator, ctx),
        },
        Expression::Throw(t) => walk_expr_for_closure_this(t.exception, ctx),
        Expression::Clone(c) => walk_expr_for_closure_this(c.object, ctx),
        Expression::Pipe(p) => walk_expr_for_closure_this(p.input, ctx)
            .or_else(|| walk_expr_for_closure_this(p.callable, ctx)),
        // A closure/arrow-function met here is not itself a call
        // argument (assigned to a variable, stored in an array, etc.),
        // so it does not rebind `$this`.  Still descend into its body
        // looking for a nested call whose closure argument contains the
        // cursor; that nested call may sit under a `@param-closure-this`
        // parameter.
        Expression::Closure(_) | Expression::ArrowFunction(_) => {
            walk_closure_body_for_closure_this(expr, ctx)
        }
        _ => None,
    }
}

/// Walk a call expression, checking each closure/arrow-function argument
/// to see if the cursor is inside it and the target parameter has
/// `closure_this_type`.
fn walk_call_for_closure_this(
    call: &Call<'_>,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let (receiver, argument_list) = match call {
        Call::Function(fc) => (None, &fc.argument_list),
        Call::Method(mc) => {
            if let Some(r) = closure_call_this(mc, ctx) {
                return Some(r);
            }
            (Some(mc.object), &mc.argument_list)
        }
        Call::NullSafeMethod(mc) => (Some(mc.object), &mc.argument_list),
        Call::StaticMethod(sc) => (Some(sc.class), &sc.argument_list),
    };
    if let Some(r) = receiver.and_then(|receiver| walk_expr_for_closure_this(receiver, ctx)) {
        return Some(r);
    }
    walk_args_for_closure_this(&argument_list.arguments, ctx, &|arg_idx| {
        closure_this_for_argument(call, arg_idx, ctx)
    })
    // Recurse into arguments that are not closures (e.g. nested calls).
    .or_else(|| {
        argument_list
            .arguments
            .iter()
            .map(|arg| arg.value())
            .filter(|arg_expr| !is_closure_like(arg_expr))
            .find_map(|arg_expr| walk_expr_for_closure_this(arg_expr, ctx))
    })
}

/// What `$this` is bound to inside a closure passed as argument `arg_idx`
/// of `call`, when the parameter receiving it carries
/// `@param-closure-this`.
pub(crate) fn closure_this_for_argument(
    call: &Call<'_>,
    arg_idx: usize,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    fn method_name<'a>(selector: &ClassLikeMemberSelector<'a>) -> Option<&'a str> {
        match selector {
            ClassLikeMemberSelector::Identifier(ident) => Some(bytes_to_str(ident.value)),
            _ => None,
        }
    }
    match call {
        Call::Function(fc) => {
            let Expression::Identifier(ident) = fc.function else {
                return None;
            };
            let fi = ctx
                .function_loader
                .and_then(|fl| fl(bytes_to_str(ident.value()), fc.function.span().start.offset))?;
            closure_this_from_function_params(&fi, arg_idx, ctx)
        }
        Call::Method(MethodCall { object, method, .. })
        | Call::NullSafeMethod(NullSafeMethodCall { object, method, .. }) => {
            closure_this_from_receiver(object, method_name(method)?, arg_idx, ctx)
        }
        Call::StaticMethod(sc) => {
            closure_this_from_static_receiver(sc.class, method_name(&sc.method)?, arg_idx, ctx)
        }
    }
}

/// `(function () { … })->call($obj)` runs the closure with `$this` bound
/// to `$obj`.
fn closure_call_this(mc: &MethodCall<'_>, ctx: &ResolutionCtx<'_>) -> Option<Vec<ResolvedType>> {
    let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
        return None;
    };
    if !bytes_to_str(ident.value).eq_ignore_ascii_case("call") {
        return None;
    }
    let closure = crate::parser::unwrap_parens(mc.object);
    if !cursor_inside_closure_body(closure, ctx) {
        return None;
    }
    if let Some(nested) = walk_closure_body_for_closure_this(closure, ctx) {
        return Some(nested);
    }
    let new_this = mc.argument_list.arguments.first()?.value();
    let types = resolve_receiver_types(new_this, ctx);
    (!types.is_empty()).then_some(types)
}

/// Whether the cursor is inside the body of `expr`, a closure or arrow
/// function literal.
fn cursor_inside_closure_body(expr: &Expression<'_>, ctx: &ResolutionCtx<'_>) -> bool {
    match expr {
        Expression::Closure(closure) => {
            let body_start = closure.body.left_brace.start.offset;
            let body_end = closure.body.right_brace.end.offset;
            ctx.cursor_offset >= body_start && ctx.cursor_offset <= body_end
        }
        Expression::ArrowFunction(arrow) => {
            let arrow_body_span = arrow.expression.span();
            ctx.cursor_offset >= arrow.arrow.start.offset
                && ctx.cursor_offset <= arrow_body_span.end.offset
        }
        _ => false,
    }
}

fn is_closure_like(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Closure(_) | Expression::ArrowFunction(_))
}

/// Walk call arguments.  For each closure/arrow-function argument whose
/// body contains the cursor, call `lookup_fn(arg_idx)` to check whether
/// the target parameter has `closure_this_type`.
///
/// A call nested inside that body is checked first: the innermost
/// `@param-closure-this` is the one that binds `$this` at the cursor.
fn walk_args_for_closure_this<F>(
    arguments: &TokenSeparatedSequence<'_, Argument<'_>>,
    ctx: &ResolutionCtx<'_>,
    lookup_fn: &F,
) -> Option<Vec<ResolvedType>>
where
    F: Fn(usize) -> Option<Vec<ResolvedType>>,
{
    for (arg_idx, arg) in arguments.iter().enumerate() {
        let arg_expr = arg.value();
        let arg_span = arg_expr.span();
        if ctx.cursor_offset < arg_span.start.offset || ctx.cursor_offset > arg_span.end.offset {
            continue;
        }

        if cursor_inside_closure_body(arg_expr, ctx) {
            if let Some(nested) = walk_closure_body_for_closure_this(arg_expr, ctx) {
                return Some(nested);
            }
            return lookup_fn(arg_idx);
        }
    }
    None
}

/// Walk the body of a closure/arrow-function argument, looking for a
/// nested call whose own closure argument contains the cursor.
///
/// Only closures passed *as call arguments* rebind `$this`; a closure
/// merely defined inside the body inherits `$this` from its enclosing
/// scope, which is why the walk descends through calls rather than
/// through every closure it meets.
fn walk_closure_body_for_closure_this(
    arg_expr: &Expression<'_>,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    match arg_expr {
        Expression::Closure(closure) => {
            for stmt in closure.body.statements.iter() {
                if let Some(r) = walk_stmt_for_closure_this(stmt, ctx) {
                    return Some(r);
                }
            }
            None
        }
        Expression::ArrowFunction(arrow) => walk_expr_for_closure_this(arrow.expression, ctx),
        _ => None,
    }
}

/// Look up `closure_this_type` on a standalone function's parameter at
/// `arg_idx`.
fn closure_this_from_function_params(
    fi: &FunctionInfo,
    arg_idx: usize,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let param = fi.parameters.get(arg_idx)?;
    let php_type = param.closure_this_type.as_ref()?;
    resolve_closure_this_type(php_type, None, ctx)
}

// ─── Shared receiver resolution ─────────────────────────────────────────
//
// These helpers resolve "the receiver/owner class of a call's method"
// from the AST, independent of what the caller extracts from the target
// method once found.  The `@param-closure-this` resolution above and the
// forward walker's passes over a call's arguments (callable-parameter
// inference, relation callbacks, by-reference parameters and captures)
// all need this same receiver resolution; they differ only in what they
// pull out of the matched method.

/// Resolve the receiver of an instance/nullsafe method call, `object`.
///
/// Resolves at the object's start so that variable resolution looks up the
/// scope snapshot *before* the call (where the receiver variable is
/// actually in scope), not at a position inside a later closure argument
/// where the variable doesn't exist yet.
pub(in crate::type_engine) fn resolve_receiver_types(
    object: &Expression<'_>,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let at = object.span().start.offset;
    let placeholder;
    let current_class = match ctx.current_class {
        Some(class) => class,
        None => {
            placeholder = crate::class_lookup::class_context_placeholder(ctx.content, at);
            &placeholder
        }
    };
    let var_ctx = crate::type_engine::resolver::VarResolutionCtx {
        source_imports: ctx.source_imports,
        backend: ctx.backend,
        loaders: crate::type_engine::resolver::Loaders::with_function(ctx.function_loader),
        resolved_class_cache: ctx.resolved_class_cache,
        scope_var_resolver: ctx.scope_var_resolver,
        ..crate::type_engine::resolver::VarResolutionCtx::new(
            "",
            current_class,
            ctx.all_classes,
            ctx.content,
            at,
            ctx.class_loader,
            ctx.source_class_loader,
        )
    };
    super::rhs_resolution::resolve_rhs_expression(object, &var_ctx)
}

/// Look up a class by its FQN: local classes first, then the cross-file
/// loader.
pub(in crate::type_engine) fn find_owner_by_name(
    fqn: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Arc<ClassInfo>> {
    all_classes
        .iter()
        .find(|c| c.fqn().eq_ignore_ascii_case(fqn))
        .map(Arc::clone)
        .or_else(|| class_loader(fqn))
}

/// Every class the class expression of a static call or `new` names.
///
/// A name, `self`, `static` or `parent` names one class.  Any other
/// expression (`$class::make()`, `new $class`, `$this->model::query()`) is
/// evaluated, and names the classes its value names as a class name (a
/// `class-string<T>`, or a string spelling a class), or else the class of
/// the object it holds, which PHP dispatches a static call on just the
/// same.
pub(in crate::type_engine) fn resolve_static_receiver_classes(
    class_expr: &Expression<'_>,
    ctx: &ResolutionCtx<'_>,
) -> Vec<Arc<ClassInfo>> {
    if let Some(name) = ctx.written_names().static_receiver_class_name(class_expr) {
        return find_owner_by_name(&name, ctx.all_classes, ctx.class_loader)
            .into_iter()
            .collect();
    }
    let held = resolve_receiver_types(class_expr, ctx);
    let owning_class = ctx.current_class.map(ClassInfo::fqn).unwrap_or_default();
    let mut named: Vec<Arc<ClassInfo>> = Vec::new();
    for rt in &held {
        ClassInfo::extend_unique_arc(
            &mut named,
            super::class_string_resolution::classes_named_by_type(
                &rt.type_string,
                &owning_class,
                ctx.all_classes,
                ctx.class_loader,
            ),
        );
    }
    if named.is_empty() {
        ResolvedType::into_arced_classes(held)
    } else {
        named
    }
}

/// Look up `closure_this_type` on an instance method's parameter at
/// `arg_idx`, resolving the receiver `object`.
fn closure_this_from_receiver(
    object: &Expression<'_>,
    method_name: &str,
    arg_idx: usize,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let receiver_classes = ResolvedType::into_arced_classes(resolve_receiver_types(object, ctx));
    for cls in &receiver_classes {
        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        if let Some(method) = resolved.get_method(method_name)
            && let Some(result) =
                closure_this_from_method_params(method, arg_idx, Some(&resolved), ctx)
        {
            return Some(result);
        }
    }
    None
}

/// Look up `closure_this_type` on a static method's parameter at
/// `arg_idx`.
fn closure_this_from_static_receiver(
    class_expr: &Expression<'_>,
    method_name: &str,
    arg_idx: usize,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    resolve_static_receiver_classes(class_expr, ctx)
        .iter()
        .find_map(|owner| {
            if method_name.eq_ignore_ascii_case("macro")
                && arg_idx == 1
                && let Some(resolve_macro_this) = ctx.laravel_macro_this_resolver
                && let Some(bound) = resolve_macro_this(&owner.fqn())
            {
                return Some(vec![ResolvedType::from_arc(
                    crate::virtual_members::resolve_class_fully_maybe_cached(
                        &bound,
                        ctx.class_loader,
                        ctx.resolved_class_cache,
                    ),
                )]);
            }
            let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
                owner,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            let method = resolved.get_method(method_name)?;
            closure_this_from_method_params(method, arg_idx, Some(&resolved), ctx)
        })
}

/// Extract `closure_this_type` from a method's parameter at `arg_idx`
/// and resolve it to the classes it names.
fn closure_this_from_method_params(
    method: &MethodInfo,
    arg_idx: usize,
    owner: Option<&Arc<ClassInfo>>,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let param = method.parameters.get(arg_idx)?;
    let php_type = param.closure_this_type.as_ref()?;
    resolve_closure_this_type(php_type, owner, ctx)
}

/// Resolve a `@param-closure-this` type to the classes it names: one,
/// or every member of an intersection (`TestCase&CreatesProducts`).
///
/// Handles `$this`, `static`, and `self` by mapping them to the
/// declaring class (owner), and resolves fully-qualified class names
/// through the class loader.
fn resolve_closure_this_type(
    php_type: &PhpType,
    owner: Option<&Arc<ClassInfo>>,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let resolved: Vec<ResolvedType> = match php_type.kind() {
        TypeKind::Intersection(members) => members
            .iter()
            .filter_map(|member| resolve_closure_this_class(member, owner, ctx))
            .map(ResolvedType::from_arc)
            .collect(),
        _ => vec![ResolvedType::from_arc(resolve_closure_this_class(
            php_type, owner, ctx,
        )?)],
    };
    (!resolved.is_empty()).then_some(resolved)
}

fn resolve_closure_this_class(
    php_type: &PhpType,
    owner: Option<&Arc<ClassInfo>>,
    ctx: &ResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    if php_type.is_self_like() {
        return owner
            .cloned()
            .or_else(|| ctx.current_class.cloned().map(Arc::new));
    }

    // Extract the base class name without stringifying.
    let type_str = php_type.base_name()?;

    // Try local classes first, then the cross-file loader.
    if let Some(cls) = ctx
        .all_classes
        .iter()
        .find(|c| c.fqn().eq_ignore_ascii_case(type_str))
    {
        return Some(Arc::clone(cls));
    }

    let resolved = (ctx.class_loader)(type_str)?;
    Some(crate::virtual_members::resolve_class_fully_maybe_cached(
        &resolved,
        ctx.class_loader,
        ctx.resolved_class_cache,
    ))
}

/// Check whether the inferred callable-signature type is a more specific
/// version of the explicit type hint.
///
/// Returns `true` when generic or element information refines the hint:
///
/// * The explicit hint is a bare class name (e.g. `Collection`) and the
///   inferred type is the same class with generic arguments (e.g.
///   `Collection<int, Customer>`).  Names are compared as FQNs, so an
///   unrelated class that only shares the short name does not match.
/// * The explicit hint is a bare `array`/`iterable` keyword, which says
///   nothing about the elements, and the inferred type is an array-like
///   type that does describe them (e.g. `array{Foo, string}`,
///   `list<User>`, `User[]`).  A closure passed to `array_map()` over
///   `list<array{Foo, string}>` may declare its parameter as plain
///   `array`; the call site still knows the shape.
/// * The inferred generic class is a subclass of the explicit bare class,
///   such as a custom Eloquent builder passed to a `Builder` parameter.
pub(in crate::type_engine) fn inferred_type_is_more_specific(
    explicit_hint: &PhpType,
    inferred: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    if let TypeKind::Union(parts) = inferred.kind() {
        return parts
            .iter()
            .all(|part| inferred_type_is_more_specific(explicit_hint, part, class_loader));
    }
    if let TypeKind::Union(parts) = explicit_hint.kind() {
        return parts
            .iter()
            .any(|part| inferred_type_is_more_specific(part, inferred, class_loader));
    }
    // The explicit hint must be a bare name (no generic args).
    let explicit_base = match explicit_hint.kind() {
        TypeKind::Named(name) => name.as_str(),
        _ => return false,
    };

    if explicit_base.eq_ignore_ascii_case("array") || explicit_base.eq_ignore_ascii_case("iterable")
    {
        let allow_iterable = explicit_base.eq_ignore_ascii_case("iterable");
        return describes_elements(inferred, allow_iterable);
    }

    // The inferred type must be a generic type (carries generic args).
    let inferred_base = match inferred.kind() {
        TypeKind::Generic(g) => g.name.as_str(),
        _ => return false,
    };

    let explicit_base = explicit_base.trim_start_matches('\\');
    explicit_base.eq_ignore_ascii_case(inferred_base.trim_start_matches('\\'))
        || class_loader(explicit_base).is_some_and(|explicit| {
            crate::class_lookup::is_subtype_of_typed(
                inferred,
                &PhpType::named(explicit.fqn()),
                class_loader,
            )
        })
}

/// Whether an array type carries element information worth keeping over a
/// bare `array`/`iterable` hint.  `array<mixed>` and a bare `array` do
/// not; `array{Foo}`, `list<User>` and `User[]` do.
///
/// The replacement must not give up what the declared hint guarantees, so
/// `iterable<T>` only qualifies when the hint was `iterable` itself
/// (`allow_iterable`), and a nullable type never does.
fn describes_elements(ty: &PhpType, allow_iterable: bool) -> bool {
    match ty.kind() {
        TypeKind::ArrayShape(entries) => !entries.is_empty(),
        TypeKind::Array(inner) => inner.is_informative(),
        TypeKind::Generic(g) => {
            crate::php_type::is_array_like_name(&g.name)
                && (allow_iterable || !g.name.eq_ignore_ascii_case("iterable"))
                && g.args.iter().any(|arg| arg.is_informative())
        }
        _ => false,
    }
}

// ── Callable parameter inference helpers ────────────────────────────

/// Build a `PhpType` representing the receiver class for `$this`/`static`
/// replacement in callable parameter inference.
///
/// For most classes this returns `PhpType::named(fqn)`.  For classes
/// whose template parameters have been concretely substituted (detected
/// by scanning method return types for generic signatures), the full
/// generic type is reconstructed.  For example, an Eloquent
/// `Builder<Product>` receiver produces
/// `TypeKind::Generic("Illuminate\\Database\\Eloquent\\Builder", [Named("App\\Product")])`
/// instead of a bare `PhpType::named("Illuminate\\Database\\Eloquent\\Builder")`.
///
/// This preserves generic args through callable param inference so that
/// `callable($this)` on `Builder<Product>` infers `Builder<Product>`,
/// not bare `Builder`.
pub(in crate::type_engine) fn build_receiver_self_type(
    receiver: &ClassInfo,
    _class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> PhpType {
    let fqn = receiver.fqn();

    // Only attempt reconstruction when the class declares template
    // params — otherwise there are no generic args to recover.
    if receiver.template_params.is_empty() {
        return PhpType::named(atom(fqn.as_ref()));
    }

    // For Eloquent Builder, extract the model name from method return
    // types where generic substitution has already been applied.
    if (receiver.name == "Builder" || fqn == ELOQUENT_BUILDER_FQN)
        && let Some(model_type) = extract_model_from_builder(receiver)
    {
        return PhpType::generic(ELOQUENT_BUILDER_FQN, vec![model_type]);
    }

    // General case: try to recover concrete generic args from method
    // return types that reference the class itself with generic params.
    // For example, if a `Collection<int, User>` has a method returning
    // `Collection<int, User>`, we can extract `[int, User]` as the
    // concrete args.
    if let Some(args) = extract_generic_args_from_methods(receiver, &fqn) {
        return PhpType::generic_atom(fqn, args);
    }

    // Fallback: if we have a parent class with @extends generics and
    // only one template param, try to extract from the parent chain.
    // This covers cases like Relation<TRelatedModel> subclasses.
    if !receiver.extends_generics.is_empty() && receiver.template_params.len() == 1 {
        for (_, args) in &receiver.extends_generics {
            if let Some(first_arg) = args.first() {
                // Skip raw template param names that weren't substituted.
                let is_unsubstituted = if let TypeKind::Named(name) = first_arg.kind() {
                    receiver.template_params.iter().any(|p| p.as_str() == name)
                } else {
                    false
                };
                if !is_unsubstituted {
                    return PhpType::generic_atom(fqn, vec![first_arg.clone()]);
                }
            }
        }
    }

    PhpType::named(atom(fqn.as_ref()))
}

/// Try to extract concrete generic args from a class's own methods.
///
/// Scans method return types for `ClassName<Arg1, Arg2, ...>` patterns
/// where the base name matches the class, and the args are concrete
/// (not raw template param names).
fn extract_generic_args_from_methods(class: &ClassInfo, class_fqn: &str) -> Option<Vec<PhpType>> {
    let class_short = crate::util::short_name(class_fqn);
    for method in &class.methods {
        if let Some(TypeKind::Generic(g)) = method.return_type.as_deref() {
            let (base, args) = (&g.name, &g.args);
            let base_short = crate::util::short_name(base);
            if (base == class_fqn || base_short.eq_ignore_ascii_case(class_short))
                && !args.is_empty()
                && args.iter().all(|a| {
                    if let TypeKind::Named(n) = a.kind() {
                        !class.template_params.iter().any(|p| p.as_str() == n)
                    } else {
                        true
                    }
                })
            {
                return Some(args.clone());
            }
        }
    }
    None
}

/// Extract the model type from a resolved `Builder<Model>` class by
/// scanning its method return types for `Builder<X>` and returning `X`.
pub(in crate::type_engine) fn extract_model_from_builder(builder: &ClassInfo) -> Option<PhpType> {
    for method in &builder.methods {
        if let Some(ref ret) = method.return_type
            && let TypeKind::Generic(g) = ret.kind()
            && !g.args.is_empty()
            && (g.name == ELOQUENT_BUILDER_FQN || g.name == "Builder")
        {
            // Skip unsubstituted template params like "TModel".
            if !g.args[0].is_empty() && !g.args[0].is_named("TModel") {
                return Some(g.args[0].clone());
            }
        }
    }
    None
}
