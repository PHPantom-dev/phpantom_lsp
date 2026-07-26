use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::control_flow::r#match::{Match, MatchArm};
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::walker::Walker;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::php_type::PhpType;
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::type_engine::variable::foreach_resolution::resolve_expression_type;
use crate::types::ClassInfo;

use super::helpers::{find_innermost_enclosing_class, make_diagnostic};

struct LiteralCondition {
    scalar_type: &'static str,
    start: usize,
    end: usize,
}

struct MatchExprData {
    subject_offset: u32,
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
        let constant_loader_cl = self.constant_loader();
        let default_class = ClassInfo::default();

        let matches: Vec<MatchExprData> =
            with_parsed_program(content, "match_type_diagnostics", |program, _| {
                let mut data = Vec::new();
                let walker = MatchCollector;
                for stmt in program.statements.iter() {
                    walker.walk_statement(stmt, &mut data);
                }
                data
            });

        if matches.is_empty() {
            return;
        }

        let mut issues: Vec<MatchArmIssue> = Vec::new();

        with_parsed_program(content, "match_type_resolve", |program, _| {
            for match_data in &matches {
                let enclosing =
                    find_innermost_enclosing_class(&file_ctx.classes, match_data.subject_offset);
                let current_class = enclosing.unwrap_or(&default_class);

                let config_resolver = |key: &str| self.resolve_config_type(key);
                let loaders = Loaders {
                    function_loader: Some(&function_loader_cl),
                    constant_loader: Some(&constant_loader_cl),
                    config_resolver: Some(&config_resolver),
                };

                let var_ctx = VarResolutionCtx {
                    var_name: "",
                    top_level_scope: None,
                    current_class,
                    all_classes: &file_ctx.classes,
                    content,
                    cursor_offset: match_data.subject_offset,
                    class_loader: &class_loader,
                    loaders,
                    resolved_class_cache: Some(&self.resolved_class_cache),
                    enclosing_return_type: None,
                    branch_aware: true,
                    match_arm_narrowing: HashMap::new(),
                    scope_var_resolver: None,
                };

                let subject_expr =
                    find_expression_at_offset(program.statements.iter(), match_data.subject_offset);
                let subject_expr = match subject_expr {
                    Some(e) => e,
                    None => continue,
                };

                let subject_type = match resolve_expression_type(subject_expr, &var_ctx) {
                    Some(ty) => ty,
                    None => continue,
                };

                let subject_scalars = subject_scalar_types(&subject_type);
                if subject_scalars.is_empty() {
                    continue;
                }

                let subject_display = subject_type.to_string();

                for cond in &match_data.conditions {
                    if !types_compatible_strict(cond.scalar_type, &subject_scalars) {
                        issues.push(MatchArmIssue {
                            start: cond.start,
                            end: cond.end,
                            literal_type: cond.scalar_type,
                            subject_type: subject_display.clone(),
                        });
                    }
                }
            }
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

impl<'a, 'b> Walker<'a, 'b, Vec<MatchExprData>> for MatchCollector {
    fn walk_in_match(&self, match_expr: &'a Match<'b>, data: &mut Vec<MatchExprData>) {
        if match_expr.expression.is_true() {
            return;
        }

        let subject_offset = match_expr.expression.span().start.offset;
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
                subject_offset,
                conditions,
            });
        }
    }
}

fn find_expression_at_offset<'a, 'b>(
    stmts: impl Iterator<Item = &'a mago_syntax::cst::statement::Statement<'b>>,
    offset: u32,
) -> Option<&'a Expression<'b>>
where
    'b: 'a,
{
    struct MatchFinder {
        target_offset: u32,
    }

    impl<'a, 'b> Walker<'a, 'b, Option<(*const Expression<'b>, std::marker::PhantomData<&'a ()>)>>
        for MatchFinder
    {
        fn walk_in_match(
            &self,
            match_expr: &'a Match<'b>,
            result: &mut Option<(*const Expression<'b>, std::marker::PhantomData<&'a ()>)>,
        ) {
            if match_expr.expression.span().start.offset == self.target_offset {
                *result = Some((
                    match_expr.expression as *const Expression<'b>,
                    std::marker::PhantomData,
                ));
            }
        }
    }

    let finder = MatchFinder {
        target_offset: offset,
    };
    let mut result = None;
    for stmt in stmts {
        finder.walk_statement(stmt, &mut result);
        if result.is_some() {
            break;
        }
    }
    result.map(|(ptr, _)| unsafe { &*ptr })
}

fn scalar_type_label(ty: &PhpType) -> Option<&'static str> {
    match ty {
        PhpType::Named(n) => {
            let s: &str = n;
            match s {
                "int" | "integer" => Some("int"),
                "string" => Some("string"),
                "float" | "double" => Some("float"),
                "bool" | "boolean" => Some("bool"),
                _ => None,
            }
        }
        _ => None,
    }
}

fn subject_scalar_types(ty: &PhpType) -> Vec<&'static str> {
    match ty {
        PhpType::Union(members) => members.iter().filter_map(scalar_type_label).collect(),
        PhpType::Nullable(inner) => {
            let mut types = subject_scalar_types(inner);
            if !types.contains(&"null") {
                types.push("null");
            }
            types
        }
        other => scalar_type_label(other).into_iter().collect(),
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
        Expression::UnaryPrefix(prefix) => match prefix.operand {
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
        },
        _ => None,
    }
}

fn types_compatible_strict(literal_type: &str, subject_types: &[&str]) -> bool {
    if subject_types.is_empty() {
        return true;
    }
    subject_types.contains(&literal_type)
}
