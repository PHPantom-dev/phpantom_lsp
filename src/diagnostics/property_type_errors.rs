//! Property type assignment mismatch diagnostics.
//!
//! Walk assignment expressions in the file and flag every assignment
//! to a typed property where the assigned value's resolved type is
//! incompatible with the declared property type.
//!
//! Handles both instance properties (`$this->prop = expr`) and static
//! properties (`ClassName::$prop = expr`).
//!
//! Uses the same conservative approach as argument type checking:
//! when in doubt (unresolved types, `mixed`, complex generics),
//! the diagnostic is suppressed to avoid false positives.

use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::access::Access;
use mago_syntax::cst::class_like::member::ClassLikeMemberSelector;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::statement::Statement;
use mago_syntax::cst::variable::Variable;

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

/// Diagnostic code used for property type mismatch diagnostics.
pub(crate) const TYPE_MISMATCH_PROPERTY_CODE: &str = "type_mismatch_property";

// ── Collected assignment site info ──────────────────────────────────────────

/// A single property assignment's resolved type plus the byte range
/// of the RHS expression in source.
struct ResolvedPropertyAssignment {
    /// The resolved type of the RHS expression.
    rhs_type: PhpType,
    /// The declared type of the property.
    declared_type: PhpType,
    /// Byte offset of the RHS expression start (inclusive).
    start: usize,
    /// Byte offset of the RHS expression end (exclusive).
    end: usize,
    /// The property name (for diagnostic messages).
    property_name: String,
}

// ── Context struct ──────────────────────────────────────────────────────────

/// Bundles the read-only context and output accumulator threaded through
/// every walker function, eliminating `clippy::too_many_arguments`.
struct PropertyCheckCtx<'a> {
    content: &'a str,
    file_ctx: &'a crate::types::FileContext,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: &'a dyn Fn(&str) -> Option<crate::types::FunctionInfo>,
    constant_loader: &'a dyn Fn(&str) -> Option<Option<String>>,
    backend: &'a Backend,
    out: &'a mut Vec<ResolvedPropertyAssignment>,
}

// ── AST walkers ─────────────────────────────────────────────────────────────

/// Walk a top-level statement collecting property assignments.
fn collect_from_statement(stmt: &Statement<'_>, ctx: &mut PropertyCheckCtx<'_>) {
    match stmt {
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                collect_from_statement(inner, ctx);
            }
        }
        Statement::Class(class) => {
            for member in class.members.iter() {
                collect_from_class_member(member, ctx);
            }
        }
        Statement::Interface(iface) => {
            for member in iface.members.iter() {
                collect_from_class_member(member, ctx);
            }
        }
        Statement::Trait(trait_def) => {
            for member in trait_def.members.iter() {
                collect_from_class_member(member, ctx);
            }
        }
        Statement::Enum(enum_def) => {
            for member in enum_def.members.iter() {
                collect_from_class_member(member, ctx);
            }
        }
        Statement::Function(func) => {
            for s in func.body.statements.iter() {
                collect_from_statement(s, ctx);
            }
        }
        Statement::Expression(expr_stmt) => {
            check_expression_for_property_assignment(expr_stmt.expression, ctx);
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                check_expression_for_property_assignment(val, ctx);
            }
        }
        Statement::If(if_stmt) => {
            collect_from_if_body(&if_stmt.body, ctx);
        }
        Statement::While(w) => {
            for s in w.body.statements() {
                collect_from_statement(s, ctx);
            }
        }
        Statement::DoWhile(dw) => {
            collect_from_statement(dw.statement, ctx);
        }
        Statement::For(f) => {
            for s in f.body.statements() {
                collect_from_statement(s, ctx);
            }
        }
        Statement::Foreach(fe) => {
            for s in fe.body.statements() {
                collect_from_statement(s, ctx);
            }
        }
        Statement::Switch(sw) => {
            collect_from_switch_body(&sw.body, ctx);
        }
        Statement::Try(t) => {
            for s in t.block.statements.iter() {
                collect_from_statement(s, ctx);
            }
            for catch in t.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_from_statement(s, ctx);
                }
            }
            if let Some(ref finally) = t.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_from_statement(s, ctx);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_from_statement(s, ctx);
            }
        }
        Statement::Declare(declare) => {
            use mago_syntax::cst::declare::DeclareBody;
            match &declare.body {
                DeclareBody::Statement(inner) => {
                    collect_from_statement(inner, ctx);
                }
                DeclareBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_from_statement(s, ctx);
                    }
                }
            }
        }
        _ => {}
    }
}

fn collect_from_class_member(
    member: &mago_syntax::cst::class_like::member::ClassLikeMember<'_>,
    ctx: &mut PropertyCheckCtx<'_>,
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

    if let ClassLikeMember::Method(method) = member
        && let MethodBody::Concrete(block) = &method.body
    {
        for s in block.statements.iter() {
            collect_from_statement(s, ctx);
        }
    }
}

fn collect_from_if_body(
    body: &mago_syntax::cst::control_flow::r#if::IfBody<'_>,
    ctx: &mut PropertyCheckCtx<'_>,
) {
    use mago_syntax::cst::control_flow::r#if::IfBody;
    match body {
        IfBody::Statement(inner) => {
            collect_from_statement(inner.statement, ctx);
            for c in inner.else_if_clauses.iter() {
                collect_from_statement(c.statement, ctx);
            }
            if let Some(ref c) = inner.else_clause {
                collect_from_statement(c.statement, ctx);
            }
        }
        IfBody::ColonDelimited(body) => {
            for s in body.statements.iter() {
                collect_from_statement(s, ctx);
            }
            for c in body.else_if_clauses.iter() {
                for s in c.statements.iter() {
                    collect_from_statement(s, ctx);
                }
            }
            if let Some(ref c) = body.else_clause {
                for s in c.statements.iter() {
                    collect_from_statement(s, ctx);
                }
            }
        }
    }
}

fn collect_from_switch_body(
    body: &mago_syntax::cst::control_flow::switch::SwitchBody<'_>,
    ctx: &mut PropertyCheckCtx<'_>,
) {
    use mago_syntax::cst::control_flow::switch::SwitchBody;
    match body {
        SwitchBody::BraceDelimited(b) => {
            for case in b.cases.iter() {
                for s in case.statements().iter() {
                    collect_from_statement(s, ctx);
                }
            }
        }
        SwitchBody::ColonDelimited(b) => {
            for case in b.cases.iter() {
                for s in case.statements().iter() {
                    collect_from_statement(s, ctx);
                }
            }
        }
    }
}

/// Check an expression for property assignments (`$this->prop = expr`).
fn check_expression_for_property_assignment(expr: &Expression<'_>, ctx: &mut PropertyCheckCtx<'_>) {
    // Only handle plain `=` assignments, not compound (`+=`, `.=`, etc.).
    let assign = match expr {
        Expression::Assignment(a) if a.operator.is_assign() => a,
        _ => return,
    };

    // Check if the LHS is a property access.
    let (prop_name, prop_class) = match assign.lhs {
        // Instance property: `$this->propName = expr`
        Expression::Access(Access::Property(pa)) => {
            let is_this = matches!(
                pa.object,
                Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this"
            );
            if !is_this {
                return;
            }
            let name = match &pa.property {
                ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value).to_string(),
                _ => return, // Dynamic property name -- skip
            };

            let offset = pa.object.span().start.offset;
            let enclosing = find_innermost_enclosing_class(&ctx.file_ctx.classes, offset);
            match enclosing {
                Some(cls) => (name, cls),
                None => return,
            }
        }
        // Static property: `self::$propName = expr` or `static::$propName = expr`
        Expression::Access(Access::StaticProperty(spa)) => {
            let class_name = match spa.class {
                Expression::Identifier(ident) => {
                    let raw = bytes_to_str(ident.value());
                    let lower = raw.to_ascii_lowercase();
                    if lower != "self" && lower != "static" {
                        return; // Only handle self/static for now
                    }
                    raw.to_string()
                }
                _ => return,
            };
            let _ = class_name; // We use offset-based class lookup

            let prop_name = match &spa.property {
                Variable::Direct(dv) => {
                    let raw = bytes_to_str(dv.name).to_string();
                    raw.strip_prefix('$').unwrap_or(&raw).to_string()
                }
                _ => return, // Dynamic variable name -- skip
            };

            let offset = spa.class.span().start.offset;
            let enclosing = find_innermost_enclosing_class(&ctx.file_ctx.classes, offset);
            match enclosing {
                Some(cls) => (prop_name, cls),
                None => return,
            }
        }
        _ => return,
    };

    // Look up the property's declared type.
    let declared_type = prop_class
        .properties
        .iter()
        .find(|p| p.name == prop_name)
        .and_then(|p| p.type_hint.clone());

    let declared_type = match declared_type {
        Some(t) if !t.is_untyped() && !t.is_mixed() => t,
        _ => return,
    };

    // Resolve `self`/`static`/`parent`/`$this` in the declared property
    // type, then expand any remaining short class names to their
    // fully-qualified form.
    let declared_type = declared_type
        .resolve_self_refs(
            prop_class.fqn().as_str(),
            prop_class.parent_class.as_deref(),
        )
        .resolve_names(&|name: &str| {
            if name.contains("__anonymous@") {
                return name.to_string();
            }
            if let Some(cls) = (ctx.class_loader)(name) {
                cls.fqn().to_string()
            } else {
                name.to_string()
            }
        });

    // Resolve the RHS expression type.
    let rhs_span = assign.rhs.span();
    let rhs_start = rhs_span.start.offset as usize;
    let rhs_end = rhs_span.end.offset as usize;

    let loaders = Loaders {
        function_loader: Some(ctx.function_loader),
        constant_loader: Some(ctx.constant_loader),
    };

    let var_ctx = VarResolutionCtx {
        var_name: "",
        top_level_scope: None,
        current_class: prop_class,
        all_classes: &ctx.file_ctx.classes,
        content: ctx.content,
        cursor_offset: rhs_start as u32,
        class_loader: ctx.class_loader,
        loaders,
        resolved_class_cache: Some(&ctx.backend.resolved_class_cache),
        enclosing_return_type: None,
        branch_aware: true,
        match_arm_narrowing: HashMap::new(),
        scope_var_resolver: None,
    };

    let rhs_type = resolve_expression_type(assign.rhs, &var_ctx).unwrap_or_else(PhpType::untyped);

    // Skip unresolved types.
    if rhs_type.is_untyped()
        || rhs_type.is_empty()
        || matches!(&rhs_type, PhpType::Raw(s) if s.is_empty())
    {
        return;
    }

    // Resolve short class names to FQN.
    let rhs_type = rhs_type.resolve_names(&|name: &str| {
        if name.contains("__anonymous@") {
            return name.to_string();
        }
        if let Some(cls) = (ctx.class_loader)(name) {
            cls.fqn().to_string()
        } else {
            name.to_string()
        }
    });

    ctx.out.push(ResolvedPropertyAssignment {
        rhs_type,
        declared_type,
        start: rhs_start,
        end: rhs_end,
        property_name: prop_name,
    });
}

// ── Main diagnostic collection ──────────────────────────────────────────────

impl Backend {
    /// Collect property type assignment mismatch diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_property_type_diagnostics(
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

        let results: Vec<ResolvedPropertyAssignment> =
            with_parsed_program(content, "property_type_diagnostics", |program, _content| {
                let mut resolved: Vec<ResolvedPropertyAssignment> = Vec::new();

                let mut ctx = PropertyCheckCtx {
                    content,
                    file_ctx: &file_ctx,
                    class_loader: &class_loader,
                    function_loader: &function_loader_cl,
                    constant_loader: &constant_loader_cl,
                    backend: self,
                    out: &mut resolved,
                };

                for stmt in program.statements.iter() {
                    collect_from_statement(stmt, &mut ctx);
                }

                resolved
            });

        // Emit diagnostics for incompatible property assignments.
        let strict_types = with_parsed_program(content, "property_strict", |program, _| {
            has_strict_types(program)
        });

        for assignment in &results {
            if is_type_compatible(
                &assignment.rhs_type,
                &assignment.declared_type,
                &class_loader,
                strict_types,
            ) {
                continue;
            }

            let range = match self.offset_range_to_lsp_range(
                uri,
                content,
                assignment.start,
                assignment.end,
            ) {
                Some(r) => r,
                None => continue,
            };

            let message = format!(
                "Property ${} expects {}, got {}",
                assignment.property_name, assignment.declared_type, assignment.rhs_type,
            );

            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::ERROR,
                TYPE_MISMATCH_PROPERTY_CODE,
                message,
            ));
        }
    }
}
