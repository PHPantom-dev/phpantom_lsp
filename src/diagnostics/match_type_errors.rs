//! Match diagnostics.
//!
//! `match` compares with `===`, so a scalar literal arm whose type differs
//! from the subject's type can never be selected.  The first check flags
//! those arms.
//!
//! That check only runs when the subject resolves to a closed set of
//! scalars.  Anything else (an object, `mixed`, a template parameter, a
//! union with a non-scalar member) leaves the arm alone, because a value
//! outside that set could still compare equal.
//!
//! The second check reports a `match` without a `default` arm whose arms
//! leave a value of the subject unhandled, which throws
//! `UnhandledMatchError` at runtime.  It runs only when the subject is a
//! closed set of values (enum cases, `bool`, `null`, int and string
//! literals) and every arm condition is one exact value; a condition that
//! could be several values, or one we cannot name, leaves the `match`
//! alone.  A `match` inside a `try` that catches `UnhandledMatchError`, or
//! in a function documented to throw it, expects the error and is not
//! reported.

use mago_span::HasSpan;
use mago_syntax::cst::class_like::method::Method;
use mago_syntax::cst::control_flow::r#match::{Match, MatchArm};
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::function_like::arrow_function::ArrowFunction;
use mago_syntax::cst::function_like::closure::Closure;
use mago_syntax::cst::function_like::function::Function;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::r#try::Try;
use mago_syntax::cst::unary::UnaryPrefixOperator;
use mago_syntax::walker::Walker;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader, SourceClassLoader};
use crate::docblock::extract_throws_tags;
use crate::parser::{extract_hint_type, with_parse_cache, with_parsed_program};
use crate::php_type::{LiteralValue, PhpType, TypeKind};
use crate::resolution::FileLoaders;
use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::variable::forward_walk::docblock_above;
use crate::type_engine::variable::iterable_elements::resolve_expression_type;
use crate::types::ClassLikeKind;

use super::helpers::{FileDiagnosticContext, make_diagnostic};

/// Diagnostic code for a `match` that leaves a value of its subject
/// unhandled.
pub(crate) const UNHANDLED_MATCH_CODE: &str = "unhandled_match";

struct LiteralCondition {
    scalar_type: &'static str,
    start: usize,
    end: usize,
}

/// A `match` expression one of the checks has something to say about:
/// one with a scalar-literal arm condition, or one without `default`.
///
/// Holds the expression by reference so the resolve pass can type its
/// subject and conditions without a second parse or an offset-keyed
/// search of the AST.
struct MatchExprData<'ast, 'arena> {
    match_expr: &'ast Match<'arena>,
    conditions: Vec<LiteralCondition>,
    has_default: bool,
}

/// What the walk gathers: the `match` expressions to check, and the `try`
/// statements and function-like bodies that decide whether an unhandled
/// value is expected.
struct MatchSites<'ast, 'arena> {
    matches: Vec<MatchExprData<'ast, 'arena>>,
    tries: Vec<&'ast Try<'arena>>,
    /// The span of every function, method, closure and arrow function.
    functions: Vec<(u32, u32)>,
}

enum MatchIssue {
    UnreachableArm {
        start: usize,
        end: usize,
        literal_type: &'static str,
        subject_type: String,
    },
    Unhandled {
        start: usize,
        end: usize,
        remaining: Vec<MatchValue>,
    },
}

impl Backend {
    pub fn collect_match_type_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_match_type_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// [`Self::collect_match_type_diagnostics`] over an already-gathered
    /// [`FileDiagnosticContext`].
    pub(crate) fn collect_match_type_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let _parse_guard = with_parse_cache(content);
        let loaders = self.file_loaders(&ctx.file);

        let issues: Vec<MatchIssue> =
            with_parsed_program(content, "match_type_diagnostics", |program, _| {
                let mut sites = MatchSites {
                    matches: Vec::new(),
                    tries: Vec::new(),
                    functions: Vec::new(),
                };
                let walker = MatchCollector;
                for stmt in program.statements.iter() {
                    walker.walk_statement(stmt, &mut sites);
                }

                let mut issues = Vec::new();
                for match_data in &sites.matches {
                    let match_expr = match_data.match_expr;
                    let subject_offset = match_expr.expression.span().start.offset;
                    if !super::incremental::reports_site(subject_offset) {
                        continue;
                    }
                    let var_ctx = loaders.var_ctx_at(content, subject_offset);

                    let subject_type =
                        match resolve_expression_type(match_expr.expression, &var_ctx) {
                            Some(ty) => ty,
                            None => continue,
                        };

                    if !match_data.conditions.is_empty()
                        && let Some(subject_scalars) = subject_scalar_types(&subject_type)
                        && !subject_scalars.is_empty()
                    {
                        let subject_display = subject_type.to_string();
                        for cond in &match_data.conditions {
                            if !subject_scalars.contains(&cond.scalar_type) {
                                issues.push(MatchIssue::UnreachableArm {
                                    start: cond.start,
                                    end: cond.end,
                                    literal_type: cond.scalar_type,
                                    subject_type: subject_display.clone(),
                                });
                            }
                        }
                    }

                    if !match_data.has_default
                        && let Some(remaining) = unhandled_values(
                            &subject_type,
                            match_expr,
                            &var_ctx,
                            loaders.resolved_class_loader(),
                        )
                        && !expects_unhandled_match_error(
                            &sites,
                            match_expr.r#match.span.start.offset,
                            content,
                            &loaders,
                        )
                    {
                        issues.push(MatchIssue::Unhandled {
                            start: match_expr.r#match.span.start.offset as usize,
                            end: match_expr.right_parenthesis.end.offset as usize,
                            remaining,
                        });
                    }
                }
                issues
            });

        for issue in issues {
            let (start, end, severity, code, message) = match issue {
                MatchIssue::UnreachableArm {
                    start,
                    end,
                    literal_type,
                    subject_type,
                } => (
                    start,
                    end,
                    DiagnosticSeverity::Warning,
                    "unreachable_match_arm",
                    format!(
                        "Match arm of type '{literal_type}' will never match subject of type '{subject_type}' (match uses ===)"
                    ),
                ),
                MatchIssue::Unhandled {
                    start,
                    end,
                    remaining,
                } => {
                    let noun = if remaining.len() == 1 {
                        "value"
                    } else {
                        "values"
                    };
                    let values: Vec<&str> = remaining.iter().map(|v| v.label.as_str()).collect();
                    (
                        start,
                        end,
                        DiagnosticSeverity::Error,
                        UNHANDLED_MATCH_CODE,
                        format!(
                            "Match expression does not handle remaining {noun}: {}",
                            values.join("|")
                        ),
                    )
                }
            };
            let Some(range) = self.offset_range_to_lsp_range(uri, content, start, end) else {
                continue;
            };
            out.push(make_diagnostic(range, severity, code, message));
        }
    }
}

struct MatchCollector;

impl<'ast, 'arena> Walker<'ast, 'arena, MatchSites<'ast, 'arena>> for MatchCollector {
    fn walk_in_try(&self, try_statement: &'ast Try<'arena>, sites: &mut MatchSites<'ast, 'arena>) {
        if !try_statement.catch_clauses.is_empty() {
            sites.tries.push(try_statement);
        }
    }

    fn walk_in_function(&self, node: &'ast Function<'arena>, sites: &mut MatchSites<'ast, 'arena>) {
        sites.functions.push(span_offsets(node));
    }

    fn walk_in_method(&self, node: &'ast Method<'arena>, sites: &mut MatchSites<'ast, 'arena>) {
        sites.functions.push(span_offsets(node));
    }

    fn walk_in_closure(&self, node: &'ast Closure<'arena>, sites: &mut MatchSites<'ast, 'arena>) {
        sites.functions.push(span_offsets(node));
    }

    fn walk_in_arrow_function(
        &self,
        node: &'ast ArrowFunction<'arena>,
        sites: &mut MatchSites<'ast, 'arena>,
    ) {
        sites.functions.push(span_offsets(node));
    }

    fn walk_in_match(&self, match_expr: &'ast Match<'arena>, sites: &mut MatchSites<'ast, 'arena>) {
        // `match (true)` is the condition-dispatch idiom; its arms are
        // boolean expressions rather than values compared to a subject.
        if match_expr.expression.is_true() {
            return;
        }

        let mut conditions = Vec::new();
        let mut has_default = false;
        for arm in match_expr.arms.iter() {
            let arm_conditions = match arm {
                MatchArm::Expression(expr_arm) => &expr_arm.conditions,
                MatchArm::Default(_) => {
                    has_default = true;
                    continue;
                }
            };
            for condition in arm_conditions.iter() {
                if let Some(lc) = literal_scalar_type(condition) {
                    conditions.push(lc);
                }
            }
        }

        if !conditions.is_empty() || !has_default {
            sites.matches.push(MatchExprData {
                match_expr,
                conditions,
                has_default,
            });
        }
    }
}

fn span_offsets(node: &impl HasSpan) -> (u32, u32) {
    let span = node.span();
    (span.start.offset, span.end.offset)
}

/// Whether the code around the `match` at `offset` expects it to throw
/// `UnhandledMatchError`: a `try` around it in the same function body
/// catches the error, or the enclosing function documents `@throws` for it.
///
/// A closure or arrow function does not run where it is written, so a `try`
/// outside one does not cover a `match` inside it.
fn expects_unhandled_match_error(
    sites: &MatchSites<'_, '_>,
    offset: u32,
    content: &str,
    loaders: &FileLoaders<'_>,
) -> bool {
    let enclosing = sites
        .functions
        .iter()
        .filter(|(start, end)| *start <= offset && offset < *end)
        .max_by_key(|(start, _)| *start);
    let body_start = enclosing.map_or(0, |(start, _)| *start);

    let caught = sites.tries.iter().any(|try_statement| {
        let block = try_statement.block.span();
        block.start.offset > body_start
            && block.start.offset <= offset
            && offset < block.end.offset
            && try_statement.catch_clauses.iter().any(|catch| {
                let hint_offset = catch.hint.span().start.offset;
                names_unhandled_match_error(
                    &extract_hint_type(&catch.hint),
                    loaders.class_loader_at(hint_offset),
                )
            })
    });
    if caught {
        return true;
    }

    let Some((start, _)) = enclosing else {
        return false;
    };
    let Some(docblock) = docblock_above(content, *start as usize) else {
        return false;
    };
    let class_loader = loaders.class_loader_at(*start);
    extract_throws_tags(docblock)
        .iter()
        .any(|thrown| names_unhandled_match_error(thrown, class_loader))
}

/// Whether `ty`, written in source, names `UnhandledMatchError` or a class
/// it extends or implements.
fn names_unhandled_match_error(ty: &PhpType, class_loader: &SourceClassLoader<'_>) -> bool {
    ty.union_members().iter().any(|member| {
        member
            .base_name()
            .and_then(class_loader)
            .is_some_and(|class| {
                let fqn = class.fqn();
                let fqn = fqn.trim_start_matches('\\');
                ["UnhandledMatchError", "Error", "Throwable"]
                    .iter()
                    .any(|name| fqn.eq_ignore_ascii_case(name))
            })
    })
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

/// One value a `match` subject can hold, as the unhandled-match check
/// tracks it.
struct MatchValue {
    key: ValueKey,
    /// How the value is spelled in the diagnostic.
    label: String,
}

#[derive(PartialEq)]
enum ValueKey {
    /// An enum case, by the enum's lowercased FQN and the case name.
    Case(String, String),
    True,
    False,
    Null,
    Int(i64),
    Str(String),
}

/// The values of `subject_type` no arm of `match_expr` handles, or `None`
/// when every value is handled or the check cannot tell.
fn unhandled_values(
    subject_type: &PhpType,
    match_expr: &Match<'_>,
    var_ctx: &VarResolutionCtx<'_>,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Vec<MatchValue>> {
    let mut remaining = Vec::new();
    if !closed_values(subject_type, class_loader, &mut remaining) {
        return None;
    }
    let mut condition_values = Vec::with_capacity(1);
    for arm in match_expr.arms.iter() {
        let MatchArm::Expression(arm) = arm else {
            return None;
        };
        for condition in arm.conditions.iter() {
            if remaining.is_empty() {
                return None;
            }
            let condition_type = resolve_expression_type(condition, var_ctx)?;
            condition_values.clear();
            if !closed_values(&condition_type, class_loader, &mut condition_values) {
                return None;
            }
            // A condition that may be one of several values handles only
            // whichever it turns out to be, which is not known here.
            let [value] = condition_values.as_slice() else {
                return None;
            };
            remaining.retain(|v| v.key != value.key);
        }
    }
    (!remaining.is_empty()).then_some(remaining)
}

/// Push every value `ty` can hold onto `out`, answering `false` when it is
/// not a closed set this check can enumerate.
fn closed_values(
    ty: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
    out: &mut Vec<MatchValue>,
) -> bool {
    // Checked before `kind()`, which reads a case as its whole enum.
    if let Some((enum_name, case)) = ty.as_enum_case() {
        let enum_name = enum_name.trim_start_matches('\\');
        push_value(
            out,
            ValueKey::Case(enum_name.to_ascii_lowercase(), case.to_string()),
            || format!("{enum_name}::{case}"),
        );
        return true;
    }
    match ty.kind() {
        TypeKind::Union(members) => members
            .iter()
            .all(|member| closed_values(member, class_loader, out)),
        TypeKind::Nullable(inner) => {
            push_value(out, ValueKey::Null, || "null".to_string());
            closed_values(inner, class_loader, out)
        }
        TypeKind::Named(name) => {
            let name: &str = name;
            if name.eq_ignore_ascii_case("bool") || name.eq_ignore_ascii_case("boolean") {
                push_value(out, ValueKey::True, || "true".to_string());
                push_value(out, ValueKey::False, || "false".to_string());
            } else if name.eq_ignore_ascii_case("true") {
                push_value(out, ValueKey::True, || "true".to_string());
            } else if name.eq_ignore_ascii_case("false") {
                push_value(out, ValueKey::False, || "false".to_string());
            } else if name.eq_ignore_ascii_case("null") {
                push_value(out, ValueKey::Null, || "null".to_string());
            } else {
                return enum_cases(name, class_loader, out);
            }
            true
        }
        // An enum is final, so `static` and `$this` inside one are the
        // enum itself.
        TypeKind::StaticType(name) | TypeKind::ThisType(name) => {
            enum_cases(name, class_loader, out)
        }
        TypeKind::Literal(literal) => {
            let key = match &**literal {
                LiteralValue::Int(_) => literal.parse_i64().map(ValueKey::Int),
                LiteralValue::String(_) => literal
                    .string_content()
                    .map(|content| ValueKey::Str(content.into_owned())),
                LiteralValue::Float(_) => None,
            };
            let Some(key) = key else {
                return false;
            };
            push_value(out, key, || ty.to_string());
            true
        }
        _ => false,
    }
}

/// Push every case of the enum `name` onto `out`, answering `false` when
/// `name` is not an enum.
fn enum_cases(
    name: &str,
    class_loader: &ResolvedClassLoader<'_>,
    out: &mut Vec<MatchValue>,
) -> bool {
    let Some(class) = class_loader(Fqn::new(name)) else {
        return false;
    };
    if class.kind != ClassLikeKind::Enum {
        return false;
    }
    let fqn = class.fqn();
    let fqn = fqn.trim_start_matches('\\');
    let key = fqn.to_ascii_lowercase();
    for case in class.constants.iter().filter(|c| c.is_enum_case) {
        push_value(
            out,
            ValueKey::Case(key.clone(), case.name.to_string()),
            || format!("{fqn}::{}", case.name),
        );
    }
    true
}

fn push_value(out: &mut Vec<MatchValue>, key: ValueKey, label: impl FnOnce() -> String) {
    if !out.iter().any(|v| v.key == key) {
        out.push(MatchValue {
            key,
            label: label(),
        });
    }
}
