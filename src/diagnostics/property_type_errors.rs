//! Property type assignment mismatch diagnostics.
//!
//! Walk assignment expressions in the file and flag every assignment
//! to a typed property where the assigned value's resolved type is
//! incompatible with the declared property type.
//!
//! Handles both instance properties (`$obj->prop = expr`) and static
//! properties (`ClassName::$prop = expr`), through any receiver the type
//! engine can resolve.
//!
//! Uses the same conservative approach as argument type checking:
//! when in doubt (unresolved types, `mixed`, complex generics),
//! the diagnostic is suppressed to avoid false positives.

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::access::Access;
use mago_syntax::cst::class_like::member::ClassLikeMemberSelector;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::statement::Statement;
use mago_syntax::cst::variable::Variable;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::parser::{for_each_nested_statement_list, with_parsed_program};
use crate::php_type::{PhpType, TypeKind};
use crate::symbol_map::{SymbolKind, SymbolMap};
use crate::type_engine::resolver::{ResolutionCtx, SubjectOutcome, resolve_subject_outcome};
use crate::type_engine::variable::iterable_elements::resolve_expression_type;
use crate::types::{AccessKind, ClassInfo};
use crate::virtual_members::resolve_class_fully_cached;

use super::helpers::{FileDiagnosticContext, TypeCheckCtx, collect_type_check};
use super::type_errors::is_type_compatible;

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
    symbol_map: &'a SymbolMap,
    type_ctx: &'a TypeCheckCtx<'a>,
    backend: &'a Backend,
    out: &'a mut Vec<ResolvedPropertyAssignment>,
}

// ── AST walkers ─────────────────────────────────────────────────────────────

/// Walk a top-level statement collecting property assignments.
fn collect_from_statement(stmt: &Statement<'_>, ctx: &mut PropertyCheckCtx<'_>) {
    match stmt {
        _ if let Some(body) = crate::parser::class_like_body(stmt) => {
            for member in body.members {
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
        _ => for_each_nested_statement_list(stmt, |nested| {
            for inner in nested {
                collect_from_statement(inner, ctx);
            }
        }),
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

/// Check an expression for a typed property assignment
/// (`$obj->prop = expr`, `Cls::$prop = expr`).
fn check_expression_for_property_assignment(expr: &Expression<'_>, ctx: &mut PropertyCheckCtx<'_>) {
    // Only handle plain `=` assignments, not compound (`+=`, `.=`, etc.).
    let assign = match expr {
        Expression::Assignment(a) if a.operator.is_assign() => a,
        _ => return,
    };
    if !super::incremental::reports_site(assign.rhs.span().start.offset) {
        return;
    }

    // The offset of the property name, where the symbol map records the
    // member access.  Dynamic names (`$obj->$name`, `Cls::$$name`) are
    // skipped.
    let member_offset = match assign.lhs {
        Expression::Access(Access::Property(pa)) => match &pa.property {
            ClassLikeMemberSelector::Identifier(ident) => ident.span.start.offset,
            _ => return,
        },
        Expression::Access(Access::StaticProperty(spa)) => match &spa.property {
            Variable::Direct(dv) => dv.span.start.offset,
            _ => return,
        },
        _ => return,
    };
    let Some(source) = ctx.symbol_map.source(ctx.content) else {
        return;
    };
    let Some(SymbolKind::MemberAccess {
        subject_text,
        member_name,
        is_static,
        is_method_call: false,
        ..
    }) = ctx
        .symbol_map
        .lookup(member_offset)
        .filter(|span| span.start == member_offset)
        .map(|span| &span.kind)
    else {
        return;
    };
    let is_static = *is_static;
    let prop_name = member_name.as_str();

    // Resolve the receiver the way member-access checks resolve theirs,
    // so a write through any object, class name, or class-string is
    // checked, not just `$this` and `self`.
    let loaders = ctx.type_ctx.loaders;
    let rctx = ResolutionCtx {
        is_in_static_method: ctx.symbol_map.is_in_static_method(member_offset),
        ..loaders.resolution_ctx_at(ctx.content, member_offset)
    };
    let SubjectOutcome::Resolved(receivers) = resolve_subject_outcome(
        subject_text.as_str(source),
        AccessKind::of_member_access(is_static),
        &rctx,
    ) else {
        return;
    };

    let resolved_class_loader = loaders.resolved_class_loader();

    // Every possible receiver has to declare a typed property of that name;
    // otherwise the write may land on one this check knows nothing about.
    let mut declared_types: Vec<PhpType> = Vec::with_capacity(receivers.len());
    for receiver in &receivers {
        let Some(declared) = declared_property_type(
            receiver,
            prop_name,
            is_static,
            resolved_class_loader,
            ctx.backend,
        ) else {
            return;
        };
        if !declared_types.contains(&declared) {
            declared_types.push(declared);
        }
    }
    let declared_type = if declared_types.len() == 1 {
        declared_types.pop().unwrap_or_else(PhpType::untyped)
    } else {
        PhpType::union(declared_types)
    };

    let site = assign.rhs.span().start.offset;
    let declared_type = declared_type.resolve_names(&|name: &str| {
        if name.contains("__anonymous@") {
            return name.to_string();
        }
        if let Some(cls) = resolved_class_loader(Fqn::new(name)) {
            cls.fqn().to_string()
        } else {
            name.to_string()
        }
    });

    // Resolve the RHS expression type.
    let rhs_span = assign.rhs.span();
    let rhs_start = rhs_span.start.offset as usize;
    let rhs_end = rhs_span.end.offset as usize;

    let var_ctx = loaders.var_ctx_at(ctx.content, site);

    let rhs_type = resolve_expression_type(assign.rhs, &var_ctx).unwrap_or_else(PhpType::untyped);

    // Skip unresolved types.
    if rhs_type.is_untyped()
        || rhs_type.is_empty()
        || matches!(&rhs_type.kind(), TypeKind::Raw(s) if s.is_empty())
    {
        return;
    }

    // Resolve short class names to FQN.
    let rhs_type = rhs_type.resolve_names(&|name: &str| {
        if name.contains("__anonymous@") {
            return name.to_string();
        }
        if let Some(cls) = resolved_class_loader(Fqn::new(name)) {
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
        property_name: prop_name.to_string(),
    });
}

/// The declared type of `receiver`'s property `name`, with `self` and
/// `static` read as the receiver.
///
/// `None` when the receiver has no such property, when its staticness
/// does not match the access, when the property is virtual (`@property`,
/// framework attributes: magic `__set` decides what those accept), or
/// when it carries no type worth checking.
fn declared_property_type(
    receiver: &Arc<ClassInfo>,
    name: &str,
    is_static: bool,
    class_loader: &ResolvedClassLoader<'_>,
    backend: &Backend,
) -> Option<PhpType> {
    let find = |class: &ClassInfo| {
        class
            .properties
            .iter()
            .find(|p| p.name == name)
            .map(|p| (p.is_virtual, p.is_static, p.type_hint.clone()))
    };
    // The receiver itself first: it carries any generic substitutions
    // the subject applied, which the cached merge of its class does not.
    let found = match find(receiver) {
        Some(found) => found,
        None if receiver.name == "__object_shape" => return None,
        None => find(&resolve_class_fully_cached(
            receiver,
            class_loader,
            &backend.resolved_class_cache,
        ))?,
    };
    let (is_virtual, prop_is_static, type_hint) = found;
    if is_virtual || prop_is_static != is_static {
        return None;
    }
    let ty = type_hint.filter(|t| !t.is_untyped() && !t.is_mixed())?;
    Some(ty.resolve_self_refs(receiver.fqn().as_str(), receiver.parent_class.as_deref()))
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
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_property_type_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// [`Self::collect_property_type_diagnostics`] over an already-gathered
    /// [`FileDiagnosticContext`].
    pub(crate) fn collect_property_type_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let symbol_map = &ctx.symbol_map;
        out.extend(collect_type_check(
            self,
            &ctx.file,
            uri,
            content,
            TYPE_MISMATCH_PROPERTY_CODE,
            |ctx| {
                with_parsed_program(content, "property_type_diagnostics", |program, _content| {
                    let mut resolved: Vec<ResolvedPropertyAssignment> = Vec::new();

                    let mut walk_ctx = PropertyCheckCtx {
                        content,
                        symbol_map,
                        type_ctx: ctx,
                        backend: self,
                        out: &mut resolved,
                    };

                    for stmt in program.statements.iter() {
                        collect_from_statement(stmt, &mut walk_ctx);
                    }

                    resolved
                })
            },
            |assignment, ctx| {
                if is_type_compatible(
                    &assignment.rhs_type,
                    &assignment.declared_type,
                    ctx.loaders.resolved_class_loader(),
                    ctx.strict_types,
                ) {
                    return None;
                }
                Some((
                    (assignment.start, assignment.end),
                    format!(
                        "Property ${} expects {}, got {}",
                        assignment.property_name, assignment.declared_type, assignment.rhs_type,
                    ),
                ))
            },
        ));
    }
}
