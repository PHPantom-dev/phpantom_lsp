//! Return type mismatch diagnostics.
//!
//! Walk methods and functions in the file and flag every `return`
//! statement where the returned expression's resolved type is
//! incompatible with the declared return type.
//!
//! Uses the same conservative approach as argument type checking:
//! when in doubt (unresolved types, `mixed`, complex generics),
//! the diagnostic is suppressed to avoid false positives.

use std::sync::Arc;

use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::statement::Statement;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;
use crate::php_type::{PhpType, TypeKind};
use crate::return_collection::collect_returns;
use crate::type_engine::resolver::{CtxLoaders, LendsLoaders, Loaders, VarResolutionCtx};
use crate::type_engine::variable::foreach_resolution::resolve_expression_type;
use crate::types::ClassInfo;

use super::helpers::{collect_type_check, find_innermost_enclosing_class};
use super::type_errors::is_type_compatible;

/// Diagnostic code used for return type mismatch diagnostics.
pub(crate) const TYPE_MISMATCH_RETURN_CODE: &str = "type_mismatch_return";

// ── Collected return site info ──────────────────────────────────────────────

/// A single return statement's resolved type plus the byte range of
/// the return expression in source.
struct ResolvedReturn {
    /// The resolved type of the return expression, or `None` for bare `return;`.
    ty: Option<PhpType>,
    /// Byte offset of the return expression (or `return` keyword for bare returns) start (inclusive).
    start: usize,
    /// Byte offset of the return expression (or `return` keyword for bare returns) end (exclusive).
    end: usize,
    /// The declared return type of the enclosing function/method.
    declared_type: PhpType,
}

// ── AST walkers ─────────────────────────────────────────────────────────────

/// Check whether a statement list contains any `yield` expression
/// (indicating a generator function whose return type semantics differ).
fn body_contains_yield(stmts: &mago_syntax::cst::sequence::Sequence<'_, Statement<'_>>) -> bool {
    for stmt in stmts.iter() {
        if stmt_contains_yield(stmt) {
            return true;
        }
    }
    false
}

fn stmt_contains_yield(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Expression(expr_stmt) => expr_contains_yield(expr_stmt.expression),
        Statement::Return(ret) => ret.value.is_some_and(|v| expr_contains_yield(v)),
        Statement::Echo(echo) => echo.values.iter().any(|e| expr_contains_yield(e)),
        Statement::If(if_stmt) => {
            expr_contains_yield(if_stmt.condition) || if_body_contains_yield(&if_stmt.body)
        }
        Statement::While(w) => {
            expr_contains_yield(w.condition)
                || w.body.statements().iter().any(|s| stmt_contains_yield(s))
        }
        Statement::DoWhile(dw) => {
            stmt_contains_yield(dw.statement) || expr_contains_yield(dw.condition)
        }
        Statement::For(f) => {
            f.initializations.iter().any(|e| expr_contains_yield(e))
                || f.conditions.iter().any(|e| expr_contains_yield(e))
                || f.increments.iter().any(|e| expr_contains_yield(e))
                || f.body.statements().iter().any(|s| stmt_contains_yield(s))
        }
        Statement::Foreach(fe) => {
            expr_contains_yield(fe.expression)
                || fe.body.statements().iter().any(|s| stmt_contains_yield(s))
        }
        Statement::Switch(sw) => {
            expr_contains_yield(sw.expression) || switch_body_contains_yield(&sw.body)
        }
        Statement::Try(t) => {
            t.block.statements.iter().any(|s| stmt_contains_yield(s))
                || t.catch_clauses
                    .iter()
                    .any(|c| c.block.statements.iter().any(|s| stmt_contains_yield(s)))
                || t.finally_clause
                    .as_ref()
                    .is_some_and(|f| f.block.statements.iter().any(|s| stmt_contains_yield(s)))
        }
        Statement::Block(b) => b.statements.iter().any(|s| stmt_contains_yield(s)),
        // Don't recurse into nested functions/closures — their yields
        // don't make the *enclosing* function a generator.
        _ => false,
    }
}

fn if_body_contains_yield(body: &mago_syntax::cst::control_flow::r#if::IfBody<'_>) -> bool {
    use mago_syntax::cst::control_flow::r#if::IfBody;
    match body {
        IfBody::Statement(inner) => {
            stmt_contains_yield(inner.statement)
                || inner
                    .else_if_clauses
                    .iter()
                    .any(|c| stmt_contains_yield(c.statement))
                || inner
                    .else_clause
                    .as_ref()
                    .is_some_and(|c| stmt_contains_yield(c.statement))
        }
        IfBody::ColonDelimited(body) => {
            body.statements.iter().any(|s| stmt_contains_yield(s))
                || body
                    .else_if_clauses
                    .iter()
                    .any(|c| c.statements.iter().any(|s| stmt_contains_yield(s)))
                || body
                    .else_clause
                    .as_ref()
                    .is_some_and(|c| c.statements.iter().any(|s| stmt_contains_yield(s)))
        }
    }
}

fn switch_body_contains_yield(
    body: &mago_syntax::cst::control_flow::switch::SwitchBody<'_>,
) -> bool {
    use mago_syntax::cst::control_flow::switch::SwitchBody;
    match body {
        SwitchBody::BraceDelimited(b) => b
            .cases
            .iter()
            .any(|c| c.statements().iter().any(|s| stmt_contains_yield(s))),
        SwitchBody::ColonDelimited(b) => b
            .cases
            .iter()
            .any(|c| c.statements().iter().any(|s| stmt_contains_yield(s))),
    }
}

fn expr_contains_yield(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Yield(_))
}

// ── Main diagnostic collection ──────────────────────────────────────────────

impl Backend {
    /// Collect return type mismatch diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_return_type_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        collect_type_check(
            self,
            uri,
            content,
            TYPE_MISMATCH_RETURN_CODE,
            out,
            // Walk the AST, find return statements in method/function
            // bodies, resolve their types, and pair them with the
            // declared return type.
            |ctx| {
                with_parsed_program(content, "return_type_diagnostics", |program, _content| {
                    let mut resolved_returns: Vec<ResolvedReturn> = Vec::new();
                    for stmt in program.statements.iter() {
                        process_top_level_statement(
                            stmt,
                            uri,
                            content,
                            ctx.file_ctx,
                            ctx.class_loader,
                            ctx.function_loader,
                            ctx.constant_loader,
                            self,
                            &mut resolved_returns,
                        );
                    }
                    resolved_returns
                })
            },
            |ret, ctx| {
                let message = match &ret.ty {
                    // Bare `return;` in a void function — OK.
                    None if ret.declared_type.is_void() => return None,
                    // Bare `return;` in a non-void function — error.
                    None => format!(
                        "Function with return type {} must not return without a value",
                        ret.declared_type,
                    ),
                    // `return $expr;` in a void function — error.
                    Some(_) if ret.declared_type.is_void() => {
                        "Void function must not return a value".to_string()
                    }
                    // `return $expr;` with a compatible type — OK.
                    Some(ty)
                        if is_type_compatible(
                            ty,
                            &ret.declared_type,
                            &ctx.class_loader,
                            ctx.strict_types,
                        ) =>
                    {
                        return None;
                    }
                    // `return $expr;` with an incompatible type — error.
                    Some(ty) => format!(
                        "Return type {} is incompatible with declared return type {}",
                        ty, ret.declared_type,
                    ),
                };
                Some(((ret.start, ret.end), message))
            },
        );
    }
}

/// Finish the type operators a declared return type reads through a
/// constant.
///
/// `@return value-of<ID_TABLE>` names the values a table holds, but the
/// docblock parser only ever saw the constant's name.  Reading it here means
/// the body is checked against those values, where an unevaluated operator
/// widens to whatever a value could be in general and proves nothing.
fn evaluate_declared_return(
    declared: PhpType,
    current_class: &ClassInfo,
    content: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: &dyn Fn(&str, u32) -> Option<crate::types::FunctionInfo>,
    backend: &Backend,
) -> PhpType {
    if !declared.contains_unevaluated_operator() {
        return declared;
    }
    let ctx = backend.resolution_ctx_at(
        Some(current_class),
        all_classes,
        content,
        0,
        CtxLoaders::without_macro_this(class_loader, function_loader),
    );
    crate::type_engine::call_resolution::evaluate_constant_operands(&declared, &ctx)
        .unwrap_or(declared)
}

#[allow(clippy::too_many_arguments)]
/// Resolve the type of a return expression and push a `ResolvedReturn`.
///
/// For bare `return;` statements (`maybe_expr` is `None`), pushes with
/// `ty: None` — the diagnostic emission handles these specially.
/// For `return $expr;`, resolves the expression type and pushes with
/// `ty: Some(resolved_type)`.
fn resolve_return_and_push(
    maybe_expr: Option<&Expression<'_>>,
    start: usize,
    end: usize,
    stmt_start: usize,
    declared_return: &PhpType,
    current_class: &ClassInfo,
    content: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    loaders: Loaders<'_>,
    backend: &Backend,
    out: &mut Vec<ResolvedReturn>,
) {
    match maybe_expr {
        None => {
            // Bare `return;` — push with ty: None for the emission logic.
            out.push(ResolvedReturn {
                ty: None,
                start,
                end,
                declared_type: declared_return.clone(),
            });
        }
        Some(expr) => {
            // `return $expr;` — skip void-declared functions here;
            // they'll be flagged regardless of the expression type.
            if declared_return.is_void() {
                out.push(ResolvedReturn {
                    ty: Some(PhpType::untyped()), // placeholder; message ignores it
                    start,
                    end,
                    declared_type: declared_return.clone(),
                });
                return;
            }

            let resolve_class_names = |ty: PhpType| {
                ty.resolve_names(&|name: &str| {
                    if name.contains("__anonymous@") {
                        return name.to_string();
                    }
                    if let Some(cls) = class_loader(name) {
                        cls.fqn().to_string()
                    } else {
                        name.to_string()
                    }
                })
            };

            // A standalone `/** @var Type */` docblock (no variable name)
            // immediately above the `return` keyword casts the returned
            // expression's type outright (PHPStan semantics), so it wins
            // over whatever the expression itself resolves to.
            if let Some(cast) =
                crate::type_engine::variable::forward_walk::find_preceding_nameless_var_cast(
                    content, stmt_start,
                )
                && !cast.is_untyped()
                && !cast.is_empty()
            {
                out.push(ResolvedReturn {
                    ty: Some(resolve_class_names(cast)),
                    start,
                    end,
                    declared_type: declared_return.clone(),
                });
                return;
            }

            let var_ctx = VarResolutionCtx {
                backend: Some(backend),
                loaders,
                resolved_class_cache: Some(&backend.resolved_class_cache),
                ..VarResolutionCtx::new(
                    "",
                    current_class,
                    all_classes,
                    content,
                    start as u32,
                    class_loader,
                )
            };

            let ty = resolve_expression_type(expr, &var_ctx).unwrap_or_else(PhpType::untyped);

            // Skip unresolved types.
            if ty.is_untyped()
                || ty.is_empty()
                || matches!(&ty.kind(), TypeKind::Raw(s) if s.is_empty())
            {
                return;
            }

            // Resolve short class names to FQN.
            let ty = resolve_class_names(ty);

            out.push(ResolvedReturn {
                ty: Some(ty),
                start,
                end,
                declared_type: declared_return.clone(),
            });
        }
    }
}

/// The return type the index holds for the function `uri` declares at
/// `func_offset`, with the `@return` docblock already merged over the native
/// hint.
///
/// Only this file's own declaration will do.  A same-named function in another
/// file has a docblock that says nothing about the body being checked, so the
/// index entry is accepted only when it was contributed by `uri`, and a name
/// another file won the race for is looked up among the runners-up instead.
fn indexed_return_type(
    backend: &Backend,
    file_ctx: &crate::types::FileContext,
    uri: &str,
    func_name: &str,
    func_offset: u32,
) -> Option<PhpType> {
    let fqn = file_ctx.resolve_name_at(func_name, func_offset);

    {
        let fmap = backend.global_functions().read();
        for name in [fqn.as_str(), func_name] {
            if let Some((decl_uri, fi)) = fmap.get(name)
                && decl_uri == uri
            {
                return fi.return_type.clone();
            }
        }
    }

    let dups = backend.symbols.duplicate_functions.read();
    for name in [fqn.as_str(), func_name] {
        if let Some(fi) = dups.get(name).and_then(|by_uri| by_uri.get(uri)) {
            return fi.return_type.clone();
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
/// Walk a top-level statement looking for function/class declarations.
fn process_top_level_statement(
    stmt: &Statement<'_>,
    uri: &str,
    content: &str,
    file_ctx: &crate::types::FileContext,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: &dyn Fn(&str, u32) -> Option<crate::types::FunctionInfo>,
    constant_loader: &dyn Fn(&str, u32) -> Option<Option<String>>,
    backend: &Backend,
    out: &mut Vec<ResolvedReturn>,
) {
    match stmt {
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                process_top_level_statement(
                    inner,
                    uri,
                    content,
                    file_ctx,
                    class_loader,
                    function_loader,
                    constant_loader,
                    backend,
                    out,
                );
            }
        }
        Statement::Class(class) => {
            for member in class.members.iter() {
                process_class_member(
                    member,
                    content,
                    file_ctx,
                    class_loader,
                    function_loader,
                    constant_loader,
                    backend,
                    out,
                );
            }
        }
        Statement::Interface(iface) => {
            for member in iface.members.iter() {
                process_class_member(
                    member,
                    content,
                    file_ctx,
                    class_loader,
                    function_loader,
                    constant_loader,
                    backend,
                    out,
                );
            }
        }
        Statement::Trait(trait_def) => {
            for member in trait_def.members.iter() {
                process_class_member(
                    member,
                    content,
                    file_ctx,
                    class_loader,
                    function_loader,
                    constant_loader,
                    backend,
                    out,
                );
            }
        }
        Statement::Enum(enum_def) => {
            for member in enum_def.members.iter() {
                process_class_member(
                    member,
                    content,
                    file_ctx,
                    class_loader,
                    function_loader,
                    constant_loader,
                    backend,
                    out,
                );
            }
        }
        Statement::Function(func) => {
            let func_name = bytes_to_str(func.name.value);
            let func_offset = func.name.span.start.offset;

            // Extract the declared return type.  Prefer the indexed
            // `FunctionInfo`, where the parser has already merged the
            // `@return` docblock over the native hint, so a body is checked
            // against `array<string, int>` rather than bare `array`.  Falling
            // back to the AST hint covers a function the index has not caught
            // up with yet.
            let declared_return =
                indexed_return_type(backend, file_ctx, uri, func_name, func_offset).or_else(|| {
                    func.return_type_hint
                        .as_ref()
                        .map(|rth| crate::parser::extract_hint_type(&rth.hint))
                });

            let declared_return = match declared_return {
                Some(t) if !t.is_untyped() && !t.is_mixed() => t,
                _ => return,
            };

            // Skip generators.
            if body_contains_yield(&func.body.statements) {
                return;
            }

            // Collect return statements (both bare and with values).
            let mut returns: Vec<(Option<&Expression<'_>>, usize, usize, usize)> = Vec::new();
            collect_returns(func.body.statements.iter(), &mut returns);

            if returns.is_empty() {
                return;
            }

            let placeholder;
            let current_class = match find_innermost_enclosing_class(&file_ctx.classes, func_offset)
            {
                Some(cc) => cc,
                None => {
                    placeholder =
                        crate::class_lookup::class_context_placeholder(content, func_offset);
                    &placeholder
                }
            };

            let declared_return = evaluate_declared_return(
                declared_return,
                current_class,
                content,
                &file_ctx.classes,
                class_loader,
                function_loader,
                backend,
            );

            let owned_loaders = backend.diagnostic_loaders_over(function_loader, constant_loader);
            let loaders = owned_loaders.loaders();

            for (maybe_expr, start, end, stmt_start) in returns {
                resolve_return_and_push(
                    maybe_expr,
                    start,
                    end,
                    stmt_start,
                    &declared_return,
                    current_class,
                    content,
                    &file_ctx.classes,
                    class_loader,
                    loaders,
                    backend,
                    out,
                );
            }
        }
        Statement::Declare(declare) => {
            use mago_syntax::cst::declare::DeclareBody;
            match &declare.body {
                DeclareBody::Statement(inner) => {
                    process_top_level_statement(
                        inner,
                        uri,
                        content,
                        file_ctx,
                        class_loader,
                        function_loader,
                        constant_loader,
                        backend,
                        out,
                    );
                }
                DeclareBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        process_top_level_statement(
                            s,
                            uri,
                            content,
                            file_ctx,
                            class_loader,
                            function_loader,
                            constant_loader,
                            backend,
                            out,
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
/// Process a class member (looking for methods with return types).
fn process_class_member(
    member: &mago_syntax::cst::class_like::member::ClassLikeMember<'_>,
    content: &str,
    file_ctx: &crate::types::FileContext,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: &dyn Fn(&str, u32) -> Option<crate::types::FunctionInfo>,
    constant_loader: &dyn Fn(&str, u32) -> Option<Option<String>>,
    backend: &Backend,
    out: &mut Vec<ResolvedReturn>,
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

    let method = match member {
        ClassLikeMember::Method(m) => m,
        _ => return,
    };

    let body = match &method.body {
        MethodBody::Concrete(block) => &block.statements,
        MethodBody::Abstract(_) => return,
    };

    let method_name = bytes_to_str(method.name.value);
    let method_offset = method.name.span.start.offset;

    // Find the enclosing class to look up the method's declared return type.
    let enclosing = find_innermost_enclosing_class(&file_ctx.classes, method_offset);
    let current_class = match enclosing {
        Some(cls) => cls,
        None => return,
    };

    // Look up the method's declared return type from the parsed MethodInfo.
    let declared_return = current_class
        .get_method(method_name)
        .and_then(|mi| mi.return_type.clone());

    let declared_return = match declared_return {
        Some(t) if !t.is_untyped() && !t.is_mixed() => t,
        _ => return,
    };

    // Skip generators.
    if body_contains_yield(body) {
        return;
    }

    // Collect return statements (both bare and with values).
    let mut returns: Vec<(Option<&Expression<'_>>, usize, usize, usize)> = Vec::new();
    collect_returns(body.iter(), &mut returns);

    if returns.is_empty() {
        return;
    }

    // Resolve the declared return type's `self`/`static`/`parent`/`$this`
    // to concrete class names for accurate comparison, then expand any
    // remaining short class names to their fully-qualified form.
    let declared_return = declared_return
        .resolve_self_refs(
            current_class.fqn().as_str(),
            current_class.parent_class.as_deref(),
        )
        .resolve_names(&|name: &str| {
            if name.contains("__anonymous@") {
                return name.to_string();
            }
            if let Some(cls) = class_loader(name) {
                cls.fqn().to_string()
            } else {
                name.to_string()
            }
        });

    let declared_return = evaluate_declared_return(
        declared_return,
        current_class,
        content,
        &file_ctx.classes,
        class_loader,
        function_loader,
        backend,
    );

    let owned_loaders = backend.diagnostic_loaders_over(function_loader, constant_loader);
    let loaders = owned_loaders.loaders();

    for (maybe_expr, start, end, stmt_start) in returns {
        resolve_return_and_push(
            maybe_expr,
            start,
            end,
            stmt_start,
            &declared_return,
            current_class,
            content,
            &file_ctx.classes,
            class_loader,
            loaders,
            backend,
            out,
        );
    }
}
