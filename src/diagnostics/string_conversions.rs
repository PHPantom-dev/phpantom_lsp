//! Diagnostics for an object used as a string when its class cannot be
//! converted to one.
//!
//! `echo`, `print`, concatenation (`.` and `.=`), interpolation and the
//! `(string)` cast all convert their operand to a string.  For an object
//! that means calling `__toString()`, so an object whose class has none
//! throws
//! `Error: Object of class Foo could not be converted to string`:
//!
//! ```php
//! final class Invoice {}
//!
//! echo $invoice;               // error
//! $label = 'No. ' . $invoice;  // error
//! $text = "Invoice $invoice";  // error
//! ```
//!
//! An operand is reported only when every value it can hold, other than
//! `null`, is an object of a class without `__toString()`.  A union with a
//! scalar, `mixed`, or a class we cannot load leaves it alone.  The
//! severity follows the unknown-member rule: an error when every class is
//! final or an enum, since nothing can add the method, and a warning when
//! a subclass or an implementation could still add it.
//!
//! A `string` parameter is not checked here; the argument type check
//! already covers it.

use mago_span::HasSpan;
use mago_syntax::cst::assignment::{Assignment, AssignmentOperator};
use mago_syntax::cst::binary::{Binary, BinaryOperator};
use mago_syntax::cst::construct::PrintConstruct;
use mago_syntax::cst::echo::{Echo, EchoTag};
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::string::StringPart;
use mago_syntax::cst::unary::{UnaryPrefix, UnaryPrefixOperator};
use mago_syntax::walker::Walker;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::variable::iterable_elements::resolve_expression_type;
use crate::types::ClassLikeKind;
use crate::virtual_members::{ResolvedClassCache, resolve_class_fully_cached};

use super::helpers::{FileDiagnosticContext, make_diagnostic};

/// Diagnostic code for an object converted to a string without
/// `__toString()`.
pub(crate) const INVALID_STRING_CONVERSION_CODE: &str = "invalid_string_conversion";

/// An operand that cannot be converted to a string.
struct Finding {
    start: usize,
    end: usize,
    /// The classes the operand can hold, none of which has `__toString()`.
    classes: Vec<String>,
    /// Whether every class is final, so no subclass can add the method.
    certain: bool,
}

impl Backend {
    /// Collect string conversion diagnostics for a single file.
    pub fn collect_string_conversion_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_string_conversion_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// [`Self::collect_string_conversion_diagnostics`] over an
    /// already-gathered [`FileDiagnosticContext`].
    pub(crate) fn collect_string_conversion_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let _parse_guard = with_parse_cache(content);
        let loaders = self.file_loaders(&ctx.file);

        let findings: Vec<Finding> =
            with_parsed_program(content, "string_conversion_diagnostics", |program, _| {
                let mut operands = Vec::new();
                for stmt in program.statements.iter() {
                    OperandCollector.walk_statement(stmt, &mut operands);
                }

                let mut findings = Vec::new();
                for operand in operands {
                    let span = operand.span();
                    if !super::incremental::reports_site(span.start.offset) {
                        continue;
                    }
                    let var_ctx = loaders.var_ctx_at(content, span.start.offset);
                    let Some(ty) = resolve_expression_type(operand, &var_ctx) else {
                        continue;
                    };
                    if let Some((classes, certain)) = unconvertible_classes(
                        &ty,
                        loaders.resolved_class_loader(),
                        &self.resolved_class_cache,
                    ) {
                        findings.push(Finding {
                            start: span.start.offset as usize,
                            end: span.end.offset as usize,
                            classes,
                            certain,
                        });
                    }
                }
                findings
            });

        for finding in findings {
            let Some(range) =
                self.offset_range_to_lsp_range(uri, content, finding.start, finding.end)
            else {
                continue;
            };
            let severity = if finding.certain {
                DiagnosticSeverity::Error
            } else {
                DiagnosticSeverity::Warning
            };
            out.push(make_diagnostic(
                range,
                severity,
                INVALID_STRING_CONVERSION_CODE,
                format!(
                    "Object of class {} cannot be converted to string (no __toString() method)",
                    finding.classes.join("|")
                ),
            ));
        }
    }
}

/// Gathers every expression PHP converts to a string.
struct OperandCollector;

impl<'ast, 'arena> Walker<'ast, 'arena, Vec<&'ast Expression<'arena>>> for OperandCollector {
    fn walk_in_echo(&self, echo: &'ast Echo<'arena>, operands: &mut Vec<&'ast Expression<'arena>>) {
        operands.extend(echo.values.iter().copied().filter(|e| may_be_object(e)));
    }

    fn walk_in_echo_tag(
        &self,
        echo: &'ast EchoTag<'arena>,
        operands: &mut Vec<&'ast Expression<'arena>>,
    ) {
        operands.extend(echo.values.iter().copied().filter(|e| may_be_object(e)));
    }

    fn walk_in_print_construct(
        &self,
        print: &'ast PrintConstruct<'arena>,
        operands: &mut Vec<&'ast Expression<'arena>>,
    ) {
        push_operand(operands, print.value);
    }

    fn walk_in_binary(
        &self,
        binary: &'ast Binary<'arena>,
        operands: &mut Vec<&'ast Expression<'arena>>,
    ) {
        if matches!(binary.operator, BinaryOperator::StringConcat(_)) {
            push_operand(operands, binary.lhs);
            push_operand(operands, binary.rhs);
        }
    }

    fn walk_in_assignment(
        &self,
        assignment: &'ast Assignment<'arena>,
        operands: &mut Vec<&'ast Expression<'arena>>,
    ) {
        if matches!(assignment.operator, AssignmentOperator::Concat(_)) {
            push_operand(operands, assignment.lhs);
            push_operand(operands, assignment.rhs);
        }
    }

    fn walk_in_unary_prefix(
        &self,
        unary: &'ast UnaryPrefix<'arena>,
        operands: &mut Vec<&'ast Expression<'arena>>,
    ) {
        if matches!(
            unary.operator,
            UnaryPrefixOperator::StringCast(..) | UnaryPrefixOperator::BinaryCast(..)
        ) {
            push_operand(operands, unary.operand);
        }
    }

    fn walk_in_string_part(
        &self,
        part: &'ast StringPart<'arena>,
        operands: &mut Vec<&'ast Expression<'arena>>,
    ) {
        match part {
            StringPart::Expression(expression) => push_operand(operands, expression),
            StringPart::BracedExpression(braced) => push_operand(operands, braced.expression),
            StringPart::Literal(_) => {}
        }
    }
}

fn push_operand<'ast, 'arena>(
    operands: &mut Vec<&'ast Expression<'arena>>,
    expression: &'ast Expression<'arena>,
) {
    if may_be_object(expression) {
        operands.push(expression);
    }
}

/// Whether `expression` can evaluate to an object, judged from its shape
/// alone.  Concatenation operands are mostly literals and other
/// concatenations, and skipping those here saves resolving them.
fn may_be_object(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::Literal(_) | Expression::CompositeString(_) | Expression::MagicConstant(_) => {
            false
        }
        // Every binary operator but `??` yields a scalar.
        Expression::Binary(binary) => matches!(binary.operator, BinaryOperator::NullCoalesce(_)),
        // So does every prefix operator but `@` and `&`.
        Expression::UnaryPrefix(unary) => matches!(
            unary.operator,
            UnaryPrefixOperator::ErrorControl(_) | UnaryPrefixOperator::Reference(_)
        ),
        Expression::Parenthesized(inner) => may_be_object(inner.expression),
        _ => true,
    }
}

/// The classes of `ty` that cannot be converted to a string, and whether
/// every one of them is final, or `None` when some value `ty` can hold
/// converts fine (or might).
fn unconvertible_classes(
    ty: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
    cache: &ResolvedClassCache,
) -> Option<(Vec<String>, bool)> {
    let mut classes = Vec::new();
    let mut certain = true;
    for member in ty.union_members() {
        if member.is_null() {
            continue;
        }
        if let TypeKind::Intersection(parts) = member.kind() {
            // A value of several types at once converts if any of them
            // declares `__toString()`, and a class implementing them all
            // could always add it.
            let mut names = Vec::with_capacity(parts.len());
            for part in parts.iter() {
                let (name, _) = class_without_to_string(part, class_loader, cache)?;
                names.push(name);
            }
            classes.push(names.join("&"));
            certain = false;
            continue;
        }
        let (name, is_final) = class_without_to_string(member, class_loader, cache)?;
        certain &= is_final;
        if !classes.contains(&name) {
            classes.push(name);
        }
    }
    (!classes.is_empty()).then_some((classes, certain))
}

/// The name of the class `ty` names and whether it is final, when that
/// class has no `__toString()`.  `None` when `ty` is not a class we can
/// load, or its class converts to a string.
fn class_without_to_string(
    ty: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
    cache: &ResolvedClassCache,
) -> Option<(String, bool)> {
    let class = class_loader(Fqn::new(ty.base_name()?))?;
    if class.kind == ClassLikeKind::Trait || is_subtype_of(&class, "Stringable", class_loader) {
        return None;
    }
    // A trait or a parent can supply the method without the class
    // declaring it, which the merged class shows.
    let resolved = resolve_class_fully_cached(&class, class_loader, cache);
    if resolved.get_method("__toString").is_some() {
        return None;
    }
    let is_final = class.is_final || class.kind == ClassLikeKind::Enum;
    let fqn = class.fqn();
    Some((fqn.trim_start_matches('\\').to_string(), is_final))
}
