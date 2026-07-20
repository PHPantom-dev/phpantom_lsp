//! Return type mismatch diagnostics.
//!
//! Walk methods and functions in the file and flag every `return`
//! statement where the returned expression's resolved type is
//! incompatible with the declared return type.
//!
//! Uses the same conservative approach as argument type checking:
//! when in doubt (unresolved types, `mixed`, complex generics),
//! the diagnostic is suppressed to avoid false positives.

use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::statement::Statement;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::completion::resolver::{Loaders, VarResolutionCtx};
use crate::completion::variable::foreach_resolution::resolve_expression_type;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::php_type::PhpType;
use crate::types::ClassInfo;

use super::helpers::{find_innermost_enclosing_class, make_diagnostic};
use super::type_errors::{has_strict_types, is_type_compatible};

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

/// Collect return expressions from a function/method body.
///
/// Recurses into control flow blocks (if/else, for, while, try/catch,
/// etc.) but does NOT recurse into closures, arrow functions, or nested
/// function declarations — those have their own return types.
fn collect_returns_from_body<'a>(
    stmts: &mago_syntax::cst::sequence::Sequence<'a, Statement<'a>>,
    returns: &mut Vec<(Option<&'a Expression<'a>>, usize, usize)>,
) {
    for stmt in stmts.iter() {
        collect_returns_from_stmt(stmt, returns);
    }
}

fn collect_returns_from_stmt<'a>(
    stmt: &Statement<'a>,
    returns: &mut Vec<(Option<&'a Expression<'a>>, usize, usize)>,
) {
    match stmt {
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                let span = val.span();
                returns.push((
                    Some(val),
                    span.start.offset as usize,
                    span.end.offset as usize,
                ));
            } else {
                // Bare `return;` — use the `return` keyword span.
                let kw_span = ret.r#return.span;
                returns.push((
                    None,
                    kw_span.start.offset as usize,
                    kw_span.end.offset as usize,
                ));
            }
        }
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                collect_returns_from_stmt(inner, returns);
            }
        }
        Statement::If(if_stmt) => {
            collect_returns_from_if_body(&if_stmt.body, returns);
        }
        Statement::While(w) => {
            for s in w.body.statements() {
                collect_returns_from_stmt(s, returns);
            }
        }
        Statement::DoWhile(dw) => {
            collect_returns_from_stmt(dw.statement, returns);
        }
        Statement::For(f) => {
            for s in f.body.statements() {
                collect_returns_from_stmt(s, returns);
            }
        }
        Statement::Foreach(fe) => {
            for s in fe.body.statements() {
                collect_returns_from_stmt(s, returns);
            }
        }
        Statement::Switch(sw) => {
            collect_returns_from_switch_body(&sw.body, returns);
        }
        Statement::Try(t) => {
            for s in t.block.statements.iter() {
                collect_returns_from_stmt(s, returns);
            }
            for catch in t.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_returns_from_stmt(s, returns);
                }
            }
            if let Some(ref finally) = t.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_returns_from_stmt(s, returns);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_returns_from_stmt(s, returns);
            }
        }
        Statement::Declare(declare) => {
            use mago_syntax::cst::declare::DeclareBody;
            match &declare.body {
                DeclareBody::Statement(inner) => {
                    collect_returns_from_stmt(inner, returns);
                }
                DeclareBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_returns_from_stmt(s, returns);
                    }
                }
            }
        }
        // Do NOT recurse into closures, arrow functions, or nested
        // functions — they have their own return types.
        Statement::Function(_) => {}
        Statement::Class(_)
        | Statement::Interface(_)
        | Statement::Trait(_)
        | Statement::Enum(_) => {}
        _ => {}
    }
}

fn collect_returns_from_if_body<'a>(
    body: &mago_syntax::cst::control_flow::r#if::IfBody<'a>,
    returns: &mut Vec<(Option<&'a Expression<'a>>, usize, usize)>,
) {
    use mago_syntax::cst::control_flow::r#if::IfBody;
    match body {
        IfBody::Statement(inner) => {
            collect_returns_from_stmt(inner.statement, returns);
            for c in inner.else_if_clauses.iter() {
                collect_returns_from_stmt(c.statement, returns);
            }
            if let Some(ref c) = inner.else_clause {
                collect_returns_from_stmt(c.statement, returns);
            }
        }
        IfBody::ColonDelimited(body) => {
            for s in body.statements.iter() {
                collect_returns_from_stmt(s, returns);
            }
            for c in body.else_if_clauses.iter() {
                for s in c.statements.iter() {
                    collect_returns_from_stmt(s, returns);
                }
            }
            if let Some(ref c) = body.else_clause {
                for s in c.statements.iter() {
                    collect_returns_from_stmt(s, returns);
                }
            }
        }
    }
}

fn collect_returns_from_switch_body<'a>(
    body: &mago_syntax::cst::control_flow::switch::SwitchBody<'a>,
    returns: &mut Vec<(Option<&'a Expression<'a>>, usize, usize)>,
) {
    use mago_syntax::cst::control_flow::switch::SwitchBody;
    match body {
        SwitchBody::BraceDelimited(b) => {
            for case in b.cases.iter() {
                for s in case.statements().iter() {
                    collect_returns_from_stmt(s, returns);
                }
            }
        }
        SwitchBody::ColonDelimited(b) => {
            for case in b.cases.iter() {
                for s in case.statements().iter() {
                    collect_returns_from_stmt(s, returns);
                }
            }
        }
    }
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
        let file_ctx = self.file_context(uri);

        let _parse_guard = with_parse_cache(content);

        let class_loader = self.class_loader(&file_ctx);
        let function_loader_cl = self.function_loader(&file_ctx);
        let constant_loader_cl = self.constant_loader();
        let default_class = ClassInfo::default();

        // Walk the AST, find return statements in method/function
        // bodies, resolve their types, and pair them with the declared
        // return type.
        let results: Vec<ResolvedReturn> =
            with_parsed_program(content, "return_type_diagnostics", |program, _content| {
                let strict_types = has_strict_types(program);
                let _ = strict_types; // used below in VarResolutionCtx
                let mut resolved_returns: Vec<ResolvedReturn> = Vec::new();

                for stmt in program.statements.iter() {
                    process_top_level_statement(
                        stmt,
                        content,
                        &file_ctx,
                        &class_loader,
                        &function_loader_cl,
                        &constant_loader_cl,
                        &default_class,
                        self,
                        &mut resolved_returns,
                    );
                }

                resolved_returns
            });

        // Emit diagnostics for incompatible returns.
        let strict_types_for_check = with_parsed_program(content, "return_strict", |program, _| {
            has_strict_types(program)
        });

        for ret in &results {
            let range = match self.offset_range_to_lsp_range(uri, content, ret.start, ret.end) {
                Some(r) => r,
                None => continue,
            };

            let message = match &ret.ty {
                // Bare `return;` in a void function — OK.
                None if ret.declared_type.is_void() => continue,
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
                        &class_loader,
                        strict_types_for_check,
                    ) =>
                {
                    continue;
                }
                // `return $expr;` with an incompatible type — error.
                Some(ty) => format!(
                    "Return type {} is incompatible with declared return type {}",
                    ty, ret.declared_type,
                ),
            };

            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::ERROR,
                TYPE_MISMATCH_RETURN_CODE,
                message,
            ));
        }
    }
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

            let var_ctx = VarResolutionCtx {
                var_name: "",
                top_level_scope: None,
                current_class,
                all_classes,
                content,
                cursor_offset: start as u32,
                class_loader,
                loaders,
                resolved_class_cache: Some(&backend.resolved_class_cache),
                enclosing_return_type: None,
                branch_aware: true,
                match_arm_narrowing: HashMap::new(),
                scope_var_resolver: None,
            };

            let ty = resolve_expression_type(expr, &var_ctx).unwrap_or_else(PhpType::untyped);

            // Skip unresolved types.
            if ty.is_untyped() || ty.is_empty() || matches!(&ty, PhpType::Raw(s) if s.is_empty()) {
                return;
            }

            // Resolve short class names to FQN.
            let ty = ty.resolve_names(&|name: &str| {
                if name.contains("__anonymous@") {
                    return name.to_string();
                }
                if let Some(cls) = class_loader(name) {
                    cls.fqn().to_string()
                } else {
                    name.to_string()
                }
            });

            out.push(ResolvedReturn {
                ty: Some(ty),
                start,
                end,
                declared_type: declared_return.clone(),
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Walk a top-level statement looking for function/class declarations.
fn process_top_level_statement(
    stmt: &Statement<'_>,
    content: &str,
    file_ctx: &crate::types::FileContext,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: &dyn Fn(&str) -> Option<crate::types::FunctionInfo>,
    constant_loader: &dyn Fn(&str) -> Option<Option<String>>,
    default_class: &ClassInfo,
    backend: &Backend,
    out: &mut Vec<ResolvedReturn>,
) {
    match stmt {
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                process_top_level_statement(
                    inner,
                    content,
                    file_ctx,
                    class_loader,
                    function_loader,
                    constant_loader,
                    default_class,
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
                    default_class,
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
                    default_class,
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
                    default_class,
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
                    default_class,
                    backend,
                    out,
                );
            }
        }
        Statement::Function(func) => {
            let func_name = bytes_to_str(func.name.value);
            let func_offset = func.name.span.start.offset;

            // Extract the declared return type.  Prefer the AST's native
            // return type hint (always available for the current file),
            // then fall back to the global function index (which may
            // carry a richer docblock-enriched type).
            let declared_return = func
                .return_type_hint
                .as_ref()
                .map(|rth| crate::parser::extract_hint_type(&rth.hint))
                .or_else(|| {
                    let fqn = file_ctx.resolve_name_at(func_name, func_offset);
                    backend
                        .global_functions()
                        .read()
                        .get(&fqn)
                        .and_then(|(_, fi)| fi.return_type.clone())
                        .or_else(|| {
                            backend
                                .global_functions()
                                .read()
                                .get(func_name)
                                .and_then(|(_, fi)| fi.return_type.clone())
                        })
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
            let mut returns: Vec<(Option<&Expression<'_>>, usize, usize)> = Vec::new();
            collect_returns_from_body(&func.body.statements, &mut returns);

            if returns.is_empty() {
                return;
            }

            // Resolve types and check.
            let enclosing = find_innermost_enclosing_class(&file_ctx.classes, func_offset);
            let current_class = enclosing.unwrap_or(default_class);

            let loaders = Loaders {
                function_loader: Some(function_loader),
                constant_loader: Some(constant_loader),
            };

            for (maybe_expr, start, end) in returns {
                resolve_return_and_push(
                    maybe_expr,
                    start,
                    end,
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
                        content,
                        file_ctx,
                        class_loader,
                        function_loader,
                        constant_loader,
                        default_class,
                        backend,
                        out,
                    );
                }
                DeclareBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        process_top_level_statement(
                            s,
                            content,
                            file_ctx,
                            class_loader,
                            function_loader,
                            constant_loader,
                            default_class,
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
    function_loader: &dyn Fn(&str) -> Option<crate::types::FunctionInfo>,
    constant_loader: &dyn Fn(&str) -> Option<Option<String>>,
    _default_class: &ClassInfo,
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
    let mut returns: Vec<(Option<&Expression<'_>>, usize, usize)> = Vec::new();
    collect_returns_from_body(body, &mut returns);

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

    let loaders = Loaders {
        function_loader: Some(function_loader),
        constant_loader: Some(constant_loader),
    };

    for (maybe_expr, start, end) in returns {
        resolve_return_and_push(
            maybe_expr,
            start,
            end,
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
