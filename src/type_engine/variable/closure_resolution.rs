/// Closure and arrow-function helpers for the forward walker.
///
/// This module provides:
///
/// - **`@param-closure-this` resolution:** detects when the cursor is
///   inside a closure whose enclosing call site declares a
///   `@param-closure-this` tag and overrides `$this` accordingly.
/// - **Callable parameter inference helpers:** shared receiver
///   resolution and generic-argument recovery used by
///   `forward_walk/callable_inference.rs` to infer untyped
///   closure/arrow-function parameter types from the enclosing callable
///   signature (e.g. `$users->map(fn($u) => …)` infers `$u` from the
///   `map` method's parameter type).
use std::cell::RefCell;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use crate::atom::{atom, bytes_to_str};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::virtual_members::laravel::{
    ELOQUENT_BUILDER_FQN, RELATION_QUERY_METHODS, extends_eloquent_model, resolve_relation_chain,
};

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
use crate::types::{AccessKind, ClassInfo, FunctionInfo, MethodInfo, ResolvedType};

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
pub(crate) fn find_closure_this_override(ctx: &ResolutionCtx<'_>) -> Option<ClassInfo> {
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
fn walk_stmt_for_closure_this(stmt: &Statement<'_>, ctx: &ResolutionCtx<'_>) -> Option<ClassInfo> {
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
        Statement::Block(block) => {
            for inner in block.statements.iter() {
                if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                    return Some(r);
                }
            }
            None
        }
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => walk_stmt_for_closure_this(body.statement, ctx),
            IfBody::ColonDelimited(body) => {
                for inner in body.statements.iter() {
                    if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                        return Some(r);
                    }
                }
                None
            }
        },
        Statement::Foreach(foreach) => match &foreach.body {
            ForeachBody::Statement(inner) => walk_stmt_for_closure_this(inner, ctx),
            ForeachBody::ColonDelimited(body) => {
                for inner in body.statements.iter() {
                    if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                        return Some(r);
                    }
                }
                None
            }
        },
        Statement::While(while_stmt) => match &while_stmt.body {
            WhileBody::Statement(inner) => walk_stmt_for_closure_this(inner, ctx),
            WhileBody::ColonDelimited(body) => {
                for inner in body.statements.iter() {
                    if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                        return Some(r);
                    }
                }
                None
            }
        },
        Statement::For(for_stmt) => match &for_stmt.body {
            ForBody::Statement(inner) => walk_stmt_for_closure_this(inner, ctx),
            ForBody::ColonDelimited(body) => {
                for inner in body.statements.iter() {
                    if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                        return Some(r);
                    }
                }
                None
            }
        },
        Statement::DoWhile(dw) => walk_stmt_for_closure_this(dw.statement, ctx),
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                    return Some(r);
                }
            }
            None
        }
        Statement::Try(try_stmt) => {
            for inner in try_stmt.block.statements.iter() {
                if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                    return Some(r);
                }
            }
            for catch in try_stmt.catch_clauses.iter() {
                for inner in catch.block.statements.iter() {
                    if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                        return Some(r);
                    }
                }
            }
            if let Some(finally) = &try_stmt.finally_clause {
                for inner in finally.block.statements.iter() {
                    if let Some(r) = walk_stmt_for_closure_this(inner, ctx) {
                        return Some(r);
                    }
                }
            }
            None
        }
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
        _ => None,
    }
}

/// Walk an expression looking for a call whose closure argument
/// contains the cursor and whose parameter has `closure_this_type`.
fn walk_expr_for_closure_this(expr: &Expression<'_>, ctx: &ResolutionCtx<'_>) -> Option<ClassInfo> {
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
fn walk_call_for_closure_this(call: &Call<'_>, ctx: &ResolutionCtx<'_>) -> Option<ClassInfo> {
    match call {
        Call::Function(fc) => {
            let func_name = match fc.function {
                Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
                _ => None,
            };
            let func_name_offset = fc.function.span().start.offset;
            let result = walk_args_for_closure_this(&fc.argument_list.arguments, ctx, &|arg_idx| {
                let name = func_name.as_deref()?;
                let fi = ctx
                    .function_loader
                    .and_then(|fl| fl(name, func_name_offset))?;
                closure_this_from_function_params(&fi, arg_idx, ctx)
            });
            if result.is_some() {
                return result;
            }
            // Recurse into arguments that are not closures (e.g. nested calls).
            for arg in fc.argument_list.arguments.iter() {
                let arg_expr = arg.value();
                if !is_closure_like(arg_expr)
                    && let Some(r) = walk_expr_for_closure_this(arg_expr, ctx)
                {
                    return Some(r);
                }
            }
            None
        }
        Call::Method(mc) => {
            if let Some(r) = walk_expr_for_closure_this(mc.object, ctx) {
                return Some(r);
            }
            if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                let method_name = bytes_to_str(ident.value).to_string();
                let obj_span = mc.object.span();
                let result =
                    walk_args_for_closure_this(&mc.argument_list.arguments, ctx, &|arg_idx| {
                        closure_this_from_receiver(
                            obj_span.start.offset,
                            obj_span.end.offset,
                            &method_name,
                            arg_idx,
                            ctx,
                        )
                    });
                if result.is_some() {
                    return result;
                }
            }
            for arg in mc.argument_list.arguments.iter() {
                let arg_expr = arg.value();
                if !is_closure_like(arg_expr)
                    && let Some(r) = walk_expr_for_closure_this(arg_expr, ctx)
                {
                    return Some(r);
                }
            }
            None
        }
        Call::NullSafeMethod(mc) => {
            if let Some(r) = walk_expr_for_closure_this(mc.object, ctx) {
                return Some(r);
            }
            if let ClassLikeMemberSelector::Identifier(ident) = &mc.method {
                let method_name = bytes_to_str(ident.value).to_string();
                let obj_span = mc.object.span();
                let result =
                    walk_args_for_closure_this(&mc.argument_list.arguments, ctx, &|arg_idx| {
                        closure_this_from_receiver(
                            obj_span.start.offset,
                            obj_span.end.offset,
                            &method_name,
                            arg_idx,
                            ctx,
                        )
                    });
                if result.is_some() {
                    return result;
                }
            }
            for arg in mc.argument_list.arguments.iter() {
                let arg_expr = arg.value();
                if !is_closure_like(arg_expr)
                    && let Some(r) = walk_expr_for_closure_this(arg_expr, ctx)
                {
                    return Some(r);
                }
            }
            None
        }
        Call::StaticMethod(sc) => {
            if let Some(r) = walk_expr_for_closure_this(sc.class, ctx) {
                return Some(r);
            }
            if let ClassLikeMemberSelector::Identifier(ident) = &sc.method {
                let method_name = bytes_to_str(ident.value).to_string();
                let result =
                    walk_args_for_closure_this(&sc.argument_list.arguments, ctx, &|arg_idx| {
                        closure_this_from_static_receiver(sc.class, &method_name, arg_idx, ctx)
                    });
                if result.is_some() {
                    return result;
                }
            }
            for arg in sc.argument_list.arguments.iter() {
                let arg_expr = arg.value();
                if !is_closure_like(arg_expr)
                    && let Some(r) = walk_expr_for_closure_this(arg_expr, ctx)
                {
                    return Some(r);
                }
            }
            None
        }
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
) -> Option<ClassInfo>
where
    F: Fn(usize) -> Option<ClassInfo>,
{
    for (arg_idx, arg) in arguments.iter().enumerate() {
        let arg_expr = arg.value();
        let arg_span = arg_expr.span();
        if ctx.cursor_offset < arg_span.start.offset || ctx.cursor_offset > arg_span.end.offset {
            continue;
        }

        let cursor_inside_body = match arg_expr {
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
        };

        if cursor_inside_body {
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
) -> Option<ClassInfo> {
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
) -> Option<ClassInfo> {
    let param = fi.parameters.get(arg_idx)?;
    let php_type = param.closure_this_type.as_ref()?;
    resolve_closure_this_type(php_type, None, ctx)
}

// ─── Shared receiver resolution ─────────────────────────────────────────
//
// These helpers resolve "the receiver/owner class of a call's method"
// from the AST, independent of what the caller extracts from the target
// method once found.  Both the `@param-closure-this` resolution above
// and the callable-parameter inference in
// `forward_walk/callable_inference.rs` need this same receiver
// resolution; they differ only in what they pull out of the matched
// method (`closure_this_type` vs. `callable(...)` parameter types).

/// Resolve the receiver classes of an instance/nullsafe method call from
/// its object span.
///
/// Overrides `cursor_offset` to the object's start so that variable
/// resolution looks up the scope snapshot *before* the call (where the
/// receiver variable is actually in scope), not at a position inside a
/// later closure argument where the variable doesn't exist yet.
pub(in crate::type_engine) fn resolve_receiver_types(
    obj_start: u32,
    obj_end: u32,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let start = obj_start as usize;
    let end = obj_end as usize;
    if end > ctx.content.len() {
        return vec![];
    }
    let obj_text = ctx.content[start..end].trim();
    let obj_ctx = ResolutionCtx {
        cursor_offset: obj_start,
        ..*ctx
    };
    crate::type_engine::resolver::resolve_target_classes(obj_text, AccessKind::Arrow, &obj_ctx)
}

/// Resolve the class name referenced by a static-call receiver
/// expression (`self`, `static`, `parent`, or a class identifier).
pub(in crate::type_engine) fn static_receiver_class_name(
    class_expr: &Expression<'_>,
    current_class: Option<&ClassInfo>,
) -> Option<String> {
    match class_expr {
        Expression::Self_(_) | Expression::Static(_) => current_class.map(|cc| cc.name.to_string()),
        Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
        Expression::Parent(_) => {
            current_class.and_then(|cc| cc.parent_class.map(|a| a.to_string()))
        }
        _ => None,
    }
}

/// Look up a class by name: local classes first, then the cross-file
/// loader.
pub(in crate::type_engine) fn find_owner_by_name(
    name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<ClassInfo> {
    all_classes
        .iter()
        .find(|c| c.name == name)
        .map(|c| ClassInfo::clone(c))
        .or_else(|| class_loader(name).map(Arc::unwrap_or_clone))
}

/// Look up `closure_this_type` on an instance method's parameter at
/// `arg_idx`, resolving the receiver from the source span.
fn closure_this_from_receiver(
    obj_start: u32,
    obj_end: u32,
    method_name: &str,
    arg_idx: usize,
    ctx: &ResolutionCtx<'_>,
) -> Option<ClassInfo> {
    let receiver_classes =
        ResolvedType::into_arced_classes(resolve_receiver_types(obj_start, obj_end, ctx));
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
) -> Option<ClassInfo> {
    let class_name = static_receiver_class_name(class_expr, ctx.current_class)?;

    if method_name.eq_ignore_ascii_case("macro")
        && arg_idx == 1
        && let Some(resolve_macro_this) = ctx.laravel_macro_this_resolver
        && let Some(owner) = resolve_macro_this(&class_name)
    {
        return Some(Arc::unwrap_or_clone(
            crate::virtual_members::resolve_class_fully_maybe_cached(
                &owner,
                ctx.class_loader,
                ctx.resolved_class_cache,
            ),
        ));
    }

    let owner = find_owner_by_name(&class_name, ctx.all_classes, ctx.class_loader)?;

    let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
        &owner,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    let method = resolved.get_method(method_name)?;
    closure_this_from_method_params(method, arg_idx, Some(&resolved), ctx)
}

/// Extract `closure_this_type` from a method's parameter at `arg_idx`
/// and resolve it to a `ClassInfo`.
fn closure_this_from_method_params(
    method: &MethodInfo,
    arg_idx: usize,
    owner: Option<&ClassInfo>,
    ctx: &ResolutionCtx<'_>,
) -> Option<ClassInfo> {
    let param = method.parameters.get(arg_idx)?;
    let php_type = param.closure_this_type.as_ref()?;
    resolve_closure_this_type(php_type, owner, ctx)
}

/// Resolve a raw `@param-closure-this` type string to a `ClassInfo`.
///
/// Handles `$this`, `static`, and `self` by mapping them to the
/// declaring class (owner), and resolves fully-qualified class names
/// through the class loader.
fn resolve_closure_this_type(
    php_type: &PhpType,
    owner: Option<&ClassInfo>,
    ctx: &ResolutionCtx<'_>,
) -> Option<ClassInfo> {
    if php_type.is_self_like() {
        return owner.cloned().or_else(|| ctx.current_class.cloned());
    }

    // Extract the base class name without stringifying.
    let type_str = php_type.base_name()?;

    // Try local classes first, then the cross-file loader.
    if let Some(cls) = ctx.all_classes.iter().find(|c| c.name == type_str) {
        return Some(ClassInfo::clone(cls));
    }

    let resolved = (ctx.class_loader)(type_str)?;
    Some(Arc::unwrap_or_clone(
        crate::virtual_members::resolve_class_fully_maybe_cached(
            &resolved,
            ctx.class_loader,
            ctx.resolved_class_cache,
        ),
    ))
}

/// Check whether the inferred callable-signature type is a more specific
/// version of the explicit type hint.
///
/// Returns `true` in two situations:
///
/// * The explicit hint is a bare class name (e.g. `Collection`) and the
///   inferred type is the same class with generic arguments (e.g.
///   `Collection<int, Customer>`).  Namespace-qualified names are compared
///   by their last segment so that `Collection` matches
///   `Illuminate\Support\Collection<int, Customer>`.
/// * The explicit hint is a bare `array`/`iterable` keyword, which says
///   nothing about the elements, and the inferred type is an array-like
///   type that does describe them (e.g. `array{Foo, string}`,
///   `list<User>`, `User[]`).  A closure passed to `array_map()` over
///   `list<array{Foo, string}>` may declare its parameter as plain
///   `array`; the call site still knows the shape.
fn inferred_type_is_more_specific(explicit_hint: &PhpType, inferred: &PhpType) -> bool {
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

    // Compare by short name so that `Collection` matches
    // `Illuminate\Support\Collection<…>`.
    let explicit_short = crate::util::short_name(explicit_base);
    let inferred_short = crate::util::short_name(inferred_base);

    explicit_short.eq_ignore_ascii_case(inferred_short)
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

/// Check whether `method_name` is a relation-query method (e.g.
/// `whereHas`, `orWhereHas`, `whereDoesntHave`, etc.) and the receiver
/// is an Eloquent model or `Builder<Model>`.  If so, resolve the
/// relation chain from the first argument string and return
/// `Builder<FinalRelatedModel>` as the closure parameter type.
///
/// Returns `None` when the override does not apply (not a relation-query
/// method, receiver is not a model, relation chain cannot be resolved),
/// in which case the caller falls through to normal callable param
/// inference.
fn try_relation_query_override(
    receiver_classes: &[Arc<ClassInfo>],
    method_name: &str,
    first_arg_text: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Vec<PhpType>> {
    if !RELATION_QUERY_METHODS.contains(&method_name) {
        return None;
    }

    let relation_name = first_arg_text?;
    if relation_name.is_empty() {
        return None;
    }

    // Determine the base model from the receiver.  The receiver may be
    // the model itself (static call: `Brand::whereHas(...)`) or a
    // `Builder<Model>` instance.
    let model = find_model_from_receivers(receiver_classes, class_loader)?;

    // Walk the dot-separated relation chain to find the final related model.
    let related_fqn = resolve_relation_chain(&model, relation_name, class_loader, None)?;

    let builder_type = PhpType::generic(
        ELOQUENT_BUILDER_FQN,
        vec![PhpType::named(atom(&related_fqn))],
    );

    Some(vec![builder_type])
}

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
fn build_receiver_self_type(
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

/// Given a list of receiver classes, find the underlying Eloquent model:
/// the receiver itself when it is a model class, or the model extracted
/// from a `Builder<Model>` receiver.
fn find_model_from_receivers(
    receiver_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Arc<ClassInfo>> {
    for cls in receiver_classes {
        // Direct model class.
        if extends_eloquent_model(cls, class_loader) {
            return Some(Arc::clone(cls));
        }

        // Builder<Model> — extract the model name from the Builder's
        // method return types.  After generic substitution, methods like
        // `where()` return `Builder<Brand>`, so we can extract `Brand`
        // from any method's return type that is `Builder<X>`.
        let cls_fqn = cls.fqn();
        if (cls.name == "Builder" || cls_fqn == ELOQUENT_BUILDER_FQN)
            && let Some(model_type) = extract_model_from_builder(cls)
            && let Some(model_cls) = model_type.base_name().and_then(class_loader)
            && extends_eloquent_model(&model_cls, class_loader)
        {
            return Some(model_cls);
        }
    }
    None
}

/// Extract the model type from a resolved `Builder<Model>` class by
/// scanning its method return types for `Builder<X>` and returning `X`.
fn extract_model_from_builder(builder: &ClassInfo) -> Option<PhpType> {
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

// ─── Public wrappers for forward walker ─────────────────────────────────────
//
// These thin wrappers expose internal helpers to the forward walker
// (`forward_walk/callable_inference.rs` and `diagnostic_walk.rs`) so it
// can perform callable parameter inference without duplicating the logic.

pub(in crate::type_engine) fn try_relation_query_override_pub(
    receiver_classes: &[Arc<ClassInfo>],
    method_name: &str,
    first_arg_text: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Vec<PhpType>> {
    try_relation_query_override(receiver_classes, method_name, first_arg_text, class_loader)
}

pub(in crate::type_engine) fn build_receiver_self_type_pub(
    receiver: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> PhpType {
    build_receiver_self_type(receiver, class_loader)
}

pub(in crate::type_engine) fn inferred_type_is_more_specific_pub(
    explicit_hint: &PhpType,
    inferred: &PhpType,
) -> bool {
    inferred_type_is_more_specific(explicit_hint, inferred)
}
