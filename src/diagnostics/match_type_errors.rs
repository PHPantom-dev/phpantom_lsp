//! Match-arm type diagnostic.
//!
//! `match` compares with `===`, so a scalar literal arm whose type differs
//! from the subject's type can never be selected.  This flags those arms.
//!
//! The check only runs when the subject resolves to a closed set of
//! scalars.  Anything else (an object, `mixed`, a template parameter, a
//! union with a non-scalar member) leaves the arm alone, because a value
//! outside that set could still compare equal.

use mago_span::HasSpan;
use mago_syntax::cst::control_flow::r#match::{Match, MatchArm};
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::unary::UnaryPrefixOperator;
use mago_syntax::walker::Walker;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::{LendsLoaders, VarResolutionCtx};
use crate::type_engine::variable::foreach_resolution::resolve_expression_type;
use crate::types::ClassInfo;

use super::helpers::{find_innermost_enclosing_class, make_diagnostic};

struct LiteralCondition {
    scalar_type: &'static str,
    start: usize,
    end: usize,
}

/// A `match` expression with at least one scalar-literal arm condition.
///
/// Holds the subject expression by reference so the resolve pass can type
/// it without a second parse or an offset-keyed search of the AST.
struct MatchExprData<'arena> {
    subject: &'arena Expression<'arena>,
    conditions: Vec<LiteralCondition>,
}

struct MatchArmIssue {
    start: usize,
    end: usize,
    literal_type: &'static str,
    subject_type: String,
}

impl Backend {
    pub fn collect_match_type_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let file_ctx = self.file_context(uri);
        let _parse_guard = with_parse_cache(content);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader_cl = self.function_loader(&file_ctx);
        let constant_loader_cl = self.constant_loader(&file_ctx);
        let default_class = ClassInfo::default();

        let issues: Vec<MatchArmIssue> =
            with_parsed_program(content, "match_type_diagnostics", |program, _| {
                let mut matches: Vec<MatchExprData<'_>> = Vec::new();
                let walker = MatchCollector;
                for stmt in program.statements.iter() {
                    walker.walk_statement(stmt, &mut matches);
                }

                let mut issues = Vec::new();
                for match_data in &matches {
                    let subject_offset = match_data.subject.span().start.offset;
                    let enclosing =
                        find_innermost_enclosing_class(&file_ctx.classes, subject_offset);
                    let current_class = enclosing.unwrap_or(&default_class);

                    let owned_loaders =
                        self.diagnostic_loaders_over(&function_loader_cl, &constant_loader_cl);
                    let loaders = owned_loaders.loaders();

                    let var_ctx = VarResolutionCtx {
                        backend: Some(self),
                        loaders,
                        resolved_class_cache: Some(&self.resolved_class_cache),
                        ..VarResolutionCtx::new(
                            "",
                            current_class,
                            &file_ctx.classes,
                            content,
                            subject_offset,
                            &class_loader,
                        )
                    };

                    let subject_type = match resolve_expression_type(match_data.subject, &var_ctx) {
                        Some(ty) => ty,
                        None => continue,
                    };

                    let Some(subject_scalars) = subject_scalar_types(&subject_type) else {
                        continue;
                    };
                    if subject_scalars.is_empty() {
                        continue;
                    }

                    let subject_display = subject_type.to_string();

                    for cond in &match_data.conditions {
                        if !subject_scalars.contains(&cond.scalar_type) {
                            issues.push(MatchArmIssue {
                                start: cond.start,
                                end: cond.end,
                                literal_type: cond.scalar_type,
                                subject_type: subject_display.clone(),
                            });
                        }
                    }
                }
                issues
            });

        for issue in &issues {
            let range = match self.offset_range_to_lsp_range(uri, content, issue.start, issue.end) {
                Some(r) => r,
                None => continue,
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::WARNING,
                "unreachable_match_arm",
                format!(
                    "Match arm of type '{}' will never match subject of type '{}' (match uses ===)",
                    issue.literal_type, issue.subject_type
                ),
            ));
        }
    }
}

struct MatchCollector;

impl<'ast, 'arena> Walker<'ast, 'arena, Vec<MatchExprData<'arena>>> for MatchCollector {
    fn walk_in_match(
        &self,
        match_expr: &'ast Match<'arena>,
        data: &mut Vec<MatchExprData<'arena>>,
    ) {
        // `match (true)` is the condition-dispatch idiom; its arms are
        // boolean expressions rather than values compared to a subject.
        if match_expr.expression.is_true() {
            return;
        }

        let mut conditions = Vec::new();
        for arm in match_expr.arms.iter() {
            let arm_conditions = match arm {
                MatchArm::Expression(expr_arm) => &expr_arm.conditions,
                MatchArm::Default(_) => continue,
            };
            for condition in arm_conditions.iter() {
                if let Some(lc) = literal_scalar_type(condition) {
                    conditions.push(lc);
                }
            }
        }

        if !conditions.is_empty() {
            data.push(MatchExprData {
                subject: match_expr.expression,
                conditions,
            });
        }
    }
}

/// The scalar label for a type node, or `None` when the node is not a
/// scalar we can compare arm literals against.
///
/// A literal type (`'exception'`, `42`) deliberately answers `None`. It
/// claims the subject holds exactly one value, which is only sound if
/// every alternative was accounted for, and a literal is just as often
/// what is left after resolution lost an alternative it could not type.
/// Widening it to its scalar kind would take the claim without the
/// evidence.
fn scalar_type_label(ty: &PhpType) -> Option<&'static str> {
    match ty.kind() {
        TypeKind::Named(name) => {
            let name: &str = name;
            match name {
                "int" | "integer" => Some("int"),
                "string" => Some("string"),
                "float" | "double" => Some("float"),
                "bool" | "boolean" | "true" | "false" => Some("bool"),
                "null" => Some("null"),
                _ => None,
            }
        }
        TypeKind::IntRange(_, _) => Some("int"),
        _ => None,
    }
}

/// Every scalar the subject can hold, or `None` when the subject is not a
/// closed set of scalars.
///
/// `None` and an empty set both mean "do not check": a single non-scalar
/// member (an object, `mixed`, a template parameter) makes the whole
/// subject unusable for this diagnostic, because a value outside the
/// scalars we recognise could still compare equal to an arm.
fn subject_scalar_types(ty: &PhpType) -> Option<Vec<&'static str>> {
    let mut labels = Vec::new();
    collect_scalar_labels(ty, &mut labels).then_some(labels)
}

fn collect_scalar_labels(ty: &PhpType, out: &mut Vec<&'static str>) -> bool {
    match ty.kind() {
        TypeKind::Union(members) => members.iter().all(|m| collect_scalar_labels(m, out)),
        TypeKind::Nullable(inner) => {
            push_label(out, "null");
            collect_scalar_labels(inner, out)
        }
        _ => match scalar_type_label(ty) {
            Some(label) => {
                push_label(out, label);
                true
            }
            None => false,
        },
    }
}

fn push_label(out: &mut Vec<&'static str>, label: &'static str) {
    if !out.contains(&label) {
        out.push(label);
    }
}

fn literal_scalar_type(expr: &Expression<'_>) -> Option<LiteralCondition> {
    match expr {
        Expression::Literal(lit) => {
            let (ty, start, end) = match lit {
                Literal::Integer(i) => ("int", i.span.start.offset, i.span.end.offset),
                Literal::String(s) => ("string", s.span.start.offset, s.span.end.offset),
                Literal::Float(f) => ("float", f.span.start.offset, f.span.end.offset),
                Literal::True(k) | Literal::False(k) => {
                    ("bool", k.span.start.offset, k.span.end.offset)
                }
                Literal::Null(k) => ("null", k.span.start.offset, k.span.end.offset),
            };
            Some(LiteralCondition {
                scalar_type: ty,
                start: start as usize,
                end: end as usize,
            })
        }
        // Only sign prefixes keep the operand's type. `!`, `~`, and the
        // casts change it, so they are left to the general resolver.
        Expression::UnaryPrefix(prefix)
            if matches!(
                prefix.operator,
                UnaryPrefixOperator::Plus(_) | UnaryPrefixOperator::Negation(_)
            ) =>
        {
            match prefix.operand {
                Expression::Literal(Literal::Integer(i)) => Some(LiteralCondition {
                    scalar_type: "int",
                    start: prefix.operator.span().start.offset as usize,
                    end: i.span.end.offset as usize,
                }),
                Expression::Literal(Literal::Float(f)) => Some(LiteralCondition {
                    scalar_type: "float",
                    start: prefix.operator.span().start.offset as usize,
                    end: f.span.end.offset as usize,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}
