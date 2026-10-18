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

use mago_span::HasSpan;
use mago_syntax::cst::access::Access;
use mago_syntax::cst::class_like::member::ClassLikeMemberSelector;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::statement::Statement;
use mago_syntax::cst::variable::Variable;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::find_class_at_offset;
use crate::parser::{for_each_nested_statement_list, with_parsed_program};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::variable::foreach_resolution::resolve_expression_type;

use super::helpers::{TypeCheckCtx, collect_type_check};
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
    file_ctx: &'a crate::types::FileContext,
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

/// Check an expression for property assignments (`$this->prop = expr`).
fn check_expression_for_property_assignment(expr: &Expression<'_>, ctx: &mut PropertyCheckCtx<'_>) {
    // Only handle plain `=` assignments, not compound (`+=`, `.=`, etc.).
    let assign = match expr {
        Expression::Assignment(a) if a.operator.is_assign() => a,
        _ => return,
    };
    if !super::incremental::reports_site(assign.rhs.span().start.offset) {
        return;
    }

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
            let enclosing = find_class_at_offset(&ctx.file_ctx.classes, offset);
            match enclosing {
                Some(cls) => (name, cls),
                None => return,
            }
        }
        // Static property: `self::$propName = expr` or `static::$propName = expr`
        Expression::Access(Access::StaticProperty(spa)) => {
            match spa.class {
                Expression::Identifier(ident) => {
                    let raw = bytes_to_str(ident.value());
                    let lower = raw.to_ascii_lowercase();
                    if lower != "self" && lower != "static" {
                        return; // Only handle self/static for now
                    }
                }
                _ => return,
            };

            let prop_name = match &spa.property {
                Variable::Direct(dv) => {
                    let raw = bytes_to_str(dv.name).to_string();
                    raw.strip_prefix('$').unwrap_or(&raw).to_string()
                }
                _ => return, // Dynamic variable name -- skip
            };

            let offset = spa.class.span().start.offset;
            let enclosing = find_class_at_offset(&ctx.file_ctx.classes, offset);
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

    let site = assign.rhs.span().start.offset;
    let class_loader = ctx.type_ctx.loaders.class_loader_at(site);
    let resolved_class_loader = ctx.type_ctx.loaders.resolved_class_loader();

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
            if let Some(cls) = resolved_class_loader(name) {
                cls.fqn().to_string()
            } else {
                name.to_string()
            }
        });

    // Resolve the RHS expression type.
    let rhs_span = assign.rhs.span();
    let rhs_start = rhs_span.start.offset as usize;
    let rhs_end = rhs_span.end.offset as usize;

    let var_ctx = VarResolutionCtx {
        source_imports: Some(ctx.type_ctx.file_ctx.use_map_at(site)),
        backend: Some(ctx.backend),
        loaders: ctx.type_ctx.loaders.loaders_at(site),
        resolved_class_cache: Some(&ctx.backend.resolved_class_cache),
        ..VarResolutionCtx::new(
            "",
            prop_class,
            &ctx.file_ctx.classes,
            ctx.content,
            rhs_start as u32,
            resolved_class_loader,
            class_loader,
        )
    };

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
        if let Some(cls) = resolved_class_loader(name) {
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
        collect_type_check(
            self,
            uri,
            content,
            TYPE_MISMATCH_PROPERTY_CODE,
            out,
            |ctx| {
                with_parsed_program(content, "property_type_diagnostics", |program, _content| {
                    let mut resolved: Vec<ResolvedPropertyAssignment> = Vec::new();

                    let mut walk_ctx = PropertyCheckCtx {
                        content,
                        file_ctx: ctx.file_ctx,
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
        );
    }
}
