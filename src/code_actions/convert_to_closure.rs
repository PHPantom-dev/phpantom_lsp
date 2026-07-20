//! **Convert to closure** code action (`refactor.rewrite`).
//!
//! Converts an arrow function to an anonymous function (closure):
//! `fn($x) => $x * 2` -> `function($x) { return $x * 2; }`.
//!
//! Variables from the outer scope that are used in the expression are
//! captured via a `use()` clause, since closures (unlike arrow functions)
//! do not auto-capture.
//!
//! The action is always offered when the cursor is on an arrow function.

use std::collections::{HashMap, HashSet};

use mago_span::HasSpan;
use mago_syntax::cst::access::Access;
use mago_syntax::cst::call::Call;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::util::{offset_to_position, position_to_byte_offset};

impl Backend {
    /// Collect "Convert to closure" code actions for arrow functions at the
    /// cursor position.
    pub(crate) fn collect_convert_to_closure_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let doc_uri: Url = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let cursor_offset = position_to_byte_offset(content, params.range.start) as u32;

        let best = crate::parser::with_parsed_program(
            content,
            "convert_to_closure",
            |program, content| {
                let mut best: Option<(u32, u32, String)> = None;
                for stmt in program.statements.iter() {
                    find_arrow_in_statement(stmt, cursor_offset, content, &mut best);
                }
                best
            },
        );

        let (arrow_start, arrow_end, replacement) = match best {
            Some(b) => b,
            None => return,
        };

        let start_pos = offset_to_position(content, arrow_start as usize);
        let end_pos = offset_to_position(content, arrow_end as usize);

        let mut changes = HashMap::new();
        changes.insert(
            doc_uri,
            vec![TextEdit {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: replacement,
            }],
        );

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Convert to closure".to_string(),
            kind: Some(CodeActionKind::new("refactor.rewrite")),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: Some(false),
            disabled: None,
            data: None,
        }));
    }
}

// ─── AST walking ────────────────────────────────────────────────────────────

/// Search for a convertible arrow function at the cursor position within a statement.
fn find_arrow_in_statement(
    stmt: &Statement<'_>,
    cursor: u32,
    content: &str,
    best: &mut Option<(u32, u32, String)>,
) {
    let span = stmt.span();
    if cursor < span.start.offset || cursor > span.end.offset {
        return;
    }

    match stmt {
        Statement::Expression(expr_stmt) => {
            find_arrow_in_expression(expr_stmt.expression, cursor, content, best);
        }
        Statement::Return(ret) => {
            if let Some(expr) = &ret.value {
                find_arrow_in_expression(expr, cursor, content, best);
            }
        }
        Statement::Echo(echo) => {
            for expr in echo.values.iter() {
                find_arrow_in_expression(expr, cursor, content, best);
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                find_arrow_in_statement(s, cursor, content, best);
            }
        }
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                find_arrow_in_statement(s, cursor, content, best);
            }
        }
        Statement::Class(class) => {
            for member in class.members.iter() {
                find_arrow_in_member(member, cursor, content, best);
            }
        }
        Statement::Trait(tr) => {
            for member in tr.members.iter() {
                find_arrow_in_member(member, cursor, content, best);
            }
        }
        Statement::Enum(en) => {
            for member in en.members.iter() {
                find_arrow_in_member(member, cursor, content, best);
            }
        }
        Statement::Interface(iface) => {
            for member in iface.members.iter() {
                find_arrow_in_member(member, cursor, content, best);
            }
        }
        Statement::Function(func) => {
            for s in func.body.statements.iter() {
                find_arrow_in_statement(s, cursor, content, best);
            }
        }
        Statement::If(if_stmt) => {
            find_arrow_in_expression(if_stmt.condition, cursor, content, best);
            for s in if_stmt.body.statements().iter() {
                find_arrow_in_statement(s, cursor, content, best);
            }
        }
        Statement::While(w) => {
            find_arrow_in_expression(w.condition, cursor, content, best);
            for s in w.body.statements().iter() {
                find_arrow_in_statement(s, cursor, content, best);
            }
        }
        Statement::DoWhile(dw) => {
            find_arrow_in_expression(dw.condition, cursor, content, best);
            find_arrow_in_statement(dw.statement, cursor, content, best);
        }
        Statement::For(f) => {
            for expr in f.initializations.iter() {
                find_arrow_in_expression(expr, cursor, content, best);
            }
            for expr in f.conditions.iter() {
                find_arrow_in_expression(expr, cursor, content, best);
            }
            for expr in f.increments.iter() {
                find_arrow_in_expression(expr, cursor, content, best);
            }
            for s in f.body.statements().iter() {
                find_arrow_in_statement(s, cursor, content, best);
            }
        }
        Statement::Foreach(fe) => {
            find_arrow_in_expression(fe.expression, cursor, content, best);
            for s in fe.body.statements().iter() {
                find_arrow_in_statement(s, cursor, content, best);
            }
        }
        Statement::Switch(sw) => {
            find_arrow_in_expression(sw.expression, cursor, content, best);
            for case in sw.body.cases().iter() {
                let stmts = match case {
                    SwitchCase::Expression(e) => &e.statements,
                    SwitchCase::Default(d) => &d.statements,
                };
                for s in stmts.iter() {
                    find_arrow_in_statement(s, cursor, content, best);
                }
            }
        }
        Statement::Try(tr) => {
            for s in tr.block.statements.iter() {
                find_arrow_in_statement(s, cursor, content, best);
            }
            for catch in tr.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    find_arrow_in_statement(s, cursor, content, best);
                }
            }
            if let Some(finally) = &tr.finally_clause {
                for s in finally.block.statements.iter() {
                    find_arrow_in_statement(s, cursor, content, best);
                }
            }
        }
        _ => {}
    }
}

/// Walk class-like members to find arrow functions inside method bodies.
fn find_arrow_in_member(
    member: &ClassLikeMember<'_>,
    cursor: u32,
    content: &str,
    best: &mut Option<(u32, u32, String)>,
) {
    match member {
        ClassLikeMember::Method(method) => {
            if let MethodBody::Concrete(body) = &method.body {
                for s in body.statements.iter() {
                    find_arrow_in_statement(s, cursor, content, best);
                }
            }
        }
        ClassLikeMember::Property(prop) => match prop {
            Property::Plain(plain) => {
                for item in plain.items.iter() {
                    if let class_like::property::PropertyItem::Concrete(concrete) = item {
                        find_arrow_in_expression(concrete.value, cursor, content, best);
                    }
                }
            }
            Property::Hooked(hooked) => {
                if let class_like::property::PropertyItem::Concrete(concrete) = &hooked.item {
                    find_arrow_in_expression(concrete.value, cursor, content, best);
                }
            }
        },
        ClassLikeMember::Constant(constant) => {
            for item in constant.items.iter() {
                find_arrow_in_expression(item.value, cursor, content, best);
            }
        }
        _ => {}
    }
}

/// Walk an expression, checking if it's an arrow function or recursing.
fn find_arrow_in_expression(
    expr: &Expression<'_>,
    cursor: u32,
    content: &str,
    best: &mut Option<(u32, u32, String)>,
) {
    let span = expr.span();
    if cursor < span.start.offset || cursor > span.end.offset {
        return;
    }

    // Check if this expression IS an arrow function we can convert.
    if let Expression::ArrowFunction(af) = expr
        && let Some(replacement) = try_convert_arrow(af, content)
    {
        let start = span.start.offset;
        let end = span.end.offset;
        if best.is_none() || (end - start) < (best.as_ref().unwrap().1 - best.as_ref().unwrap().0) {
            *best = Some((start, end, replacement));
        }
    }

    // Recurse into sub-expressions.
    match expr {
        Expression::Parenthesized(p) => {
            find_arrow_in_expression(p.expression, cursor, content, best)
        }
        Expression::UnaryPrefix(u) => find_arrow_in_expression(u.operand, cursor, content, best),
        Expression::UnaryPostfix(u) => find_arrow_in_expression(u.operand, cursor, content, best),
        Expression::Binary(b) => {
            find_arrow_in_expression(b.lhs, cursor, content, best);
            find_arrow_in_expression(b.rhs, cursor, content, best);
        }
        Expression::Assignment(a) => {
            find_arrow_in_expression(a.lhs, cursor, content, best);
            find_arrow_in_expression(a.rhs, cursor, content, best);
        }
        Expression::Conditional(c) => {
            find_arrow_in_expression(c.condition, cursor, content, best);
            if let Some(then) = c.then {
                find_arrow_in_expression(then, cursor, content, best);
            }
            find_arrow_in_expression(c.r#else, cursor, content, best);
        }
        Expression::Call(call) => match call {
            Call::Function(fc) => {
                find_arrow_in_expression(fc.function, cursor, content, best);
                for arg in fc.argument_list.arguments.iter() {
                    find_arrow_in_expression(arg.value(), cursor, content, best);
                }
            }
            Call::Method(mc) => {
                find_arrow_in_expression(mc.object, cursor, content, best);
                for arg in mc.argument_list.arguments.iter() {
                    find_arrow_in_expression(arg.value(), cursor, content, best);
                }
            }
            Call::NullSafeMethod(mc) => {
                find_arrow_in_expression(mc.object, cursor, content, best);
                for arg in mc.argument_list.arguments.iter() {
                    find_arrow_in_expression(arg.value(), cursor, content, best);
                }
            }
            Call::StaticMethod(sc) => {
                find_arrow_in_expression(sc.class, cursor, content, best);
                for arg in sc.argument_list.arguments.iter() {
                    find_arrow_in_expression(arg.value(), cursor, content, best);
                }
            }
        },
        Expression::Access(access) => match access {
            Access::Property(pa) => find_arrow_in_expression(pa.object, cursor, content, best),
            Access::NullSafeProperty(pa) => {
                find_arrow_in_expression(pa.object, cursor, content, best)
            }
            Access::StaticProperty(pa) => find_arrow_in_expression(pa.class, cursor, content, best),
            Access::ClassConstant(pa) => find_arrow_in_expression(pa.class, cursor, content, best),
        },
        Expression::Closure(closure) => {
            for s in closure.body.statements.iter() {
                find_arrow_in_statement(s, cursor, content, best);
            }
        }
        Expression::ArrowFunction(af) => {
            find_arrow_in_expression(af.expression, cursor, content, best);
        }
        Expression::Array(arr) => {
            for element in arr.elements.iter() {
                if let array::ArrayElement::KeyValue(kv) = element {
                    find_arrow_in_expression(kv.key, cursor, content, best);
                    find_arrow_in_expression(kv.value, cursor, content, best);
                } else if let array::ArrayElement::Value(val) = element {
                    find_arrow_in_expression(val.value, cursor, content, best);
                }
            }
        }
        Expression::LegacyArray(arr) => {
            for element in arr.elements.iter() {
                if let array::ArrayElement::KeyValue(kv) = element {
                    find_arrow_in_expression(kv.key, cursor, content, best);
                    find_arrow_in_expression(kv.value, cursor, content, best);
                } else if let array::ArrayElement::Value(val) = element {
                    find_arrow_in_expression(val.value, cursor, content, best);
                }
            }
        }
        Expression::ArrayAccess(aa) => {
            find_arrow_in_expression(aa.array, cursor, content, best);
            find_arrow_in_expression(aa.index, cursor, content, best);
        }
        Expression::Instantiation(inst) => {
            find_arrow_in_expression(inst.class, cursor, content, best);
            if let Some(arg_list) = &inst.argument_list {
                for a in arg_list.arguments.iter() {
                    find_arrow_in_expression(a.value(), cursor, content, best);
                }
            }
        }
        Expression::CompositeString(composite) => {
            for part in composite.parts().iter() {
                match part {
                    StringPart::Expression(inner) => {
                        find_arrow_in_expression(inner, cursor, content, best);
                    }
                    StringPart::BracedExpression(braced) => {
                        find_arrow_in_expression(braced.expression, cursor, content, best);
                    }
                    StringPart::Literal(_) => {}
                }
            }
        }
        Expression::Match(match_expr) => {
            find_arrow_in_expression(match_expr.expression, cursor, content, best);
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(expr_arm) => {
                        for cond in expr_arm.conditions.iter() {
                            find_arrow_in_expression(cond, cursor, content, best);
                        }
                        find_arrow_in_expression(expr_arm.expression, cursor, content, best);
                    }
                    MatchArm::Default(default_arm) => {
                        find_arrow_in_expression(default_arm.expression, cursor, content, best);
                    }
                }
            }
        }
        Expression::Construct(construct) => match construct {
            Construct::Isset(isset) => {
                for val in isset.values.iter() {
                    find_arrow_in_expression(val, cursor, content, best);
                }
            }
            Construct::Empty(empty) => {
                find_arrow_in_expression(empty.value, cursor, content, best);
            }
            Construct::Eval(eval) => {
                find_arrow_in_expression(eval.value, cursor, content, best);
            }
            Construct::Include(inc) => {
                find_arrow_in_expression(inc.value, cursor, content, best);
            }
            Construct::IncludeOnce(inc) => {
                find_arrow_in_expression(inc.value, cursor, content, best);
            }
            Construct::Require(req) => {
                find_arrow_in_expression(req.value, cursor, content, best);
            }
            Construct::RequireOnce(req) => {
                find_arrow_in_expression(req.value, cursor, content, best);
            }
            Construct::Print(print) => {
                find_arrow_in_expression(print.value, cursor, content, best);
            }
            Construct::Exit(exit) => {
                if let Some(args) = &exit.arguments {
                    for a in args.arguments.iter() {
                        find_arrow_in_expression(a.value(), cursor, content, best);
                    }
                }
            }
            Construct::Die(die) => {
                if let Some(args) = &die.arguments {
                    for a in args.arguments.iter() {
                        find_arrow_in_expression(a.value(), cursor, content, best);
                    }
                }
            }
        },
        Expression::Throw(throw) => {
            find_arrow_in_expression(throw.exception, cursor, content, best);
        }
        Expression::Clone(clone) => {
            find_arrow_in_expression(clone.object, cursor, content, best);
        }
        Expression::Yield(yield_expr) => match yield_expr {
            Yield::Value(yv) => {
                if let Some(val) = yv.value {
                    find_arrow_in_expression(val, cursor, content, best);
                }
            }
            Yield::Pair(yp) => {
                find_arrow_in_expression(yp.key, cursor, content, best);
                find_arrow_in_expression(yp.value, cursor, content, best);
            }
            Yield::From(yf) => {
                find_arrow_in_expression(yf.iterator, cursor, content, best);
            }
        },
        Expression::List(list) => {
            for element in list.elements.iter() {
                if let array::ArrayElement::KeyValue(kv) = element {
                    find_arrow_in_expression(kv.key, cursor, content, best);
                    find_arrow_in_expression(kv.value, cursor, content, best);
                } else if let array::ArrayElement::Value(val) = element {
                    find_arrow_in_expression(val.value, cursor, content, best);
                }
            }
        }
        Expression::ArrayAppend(append) => {
            find_arrow_in_expression(append.array, cursor, content, best);
        }
        Expression::PartialApplication(partial) => match partial {
            PartialApplication::Function(func_pa) => {
                find_arrow_in_expression(func_pa.function, cursor, content, best);
            }
            PartialApplication::Method(method_pa) => {
                find_arrow_in_expression(method_pa.object, cursor, content, best);
            }
            PartialApplication::StaticMethod(static_pa) => {
                find_arrow_in_expression(static_pa.class, cursor, content, best);
            }
        },
        Expression::AnonymousClass(anon) => {
            if let Some(args) = &anon.argument_list {
                for a in args.arguments.iter() {
                    if let Some(value) = a.value() {
                        find_arrow_in_expression(value, cursor, content, best);
                    }
                }
            }
        }
        _ => {}
    }
}

// ─── Conversion logic ───────────────────────────────────────────────────────

/// Try to convert an arrow function to a closure. Returns the replacement text.
fn try_convert_arrow(
    af: &function_like::arrow_function::ArrowFunction<'_>,
    content: &str,
) -> Option<String> {
    // Collect parameter names so we can exclude them from the `use` clause.
    let param_names: HashSet<String> = af
        .parameter_list
        .parameters
        .iter()
        .map(|p| source_text(content, p.variable.span()).to_string())
        .collect();

    // Collect variables used in the expression body that come from
    // the outer scope (need to be captured via `use()`).
    let mut captured: Vec<String> = Vec::new();
    collect_variables_in_expression(af.expression, content, &param_names, &mut captured);
    // Deduplicate while preserving order.
    let mut seen = HashSet::new();
    captured.retain(|v| seen.insert(v.clone()));

    let mut result = String::new();

    // Static keyword
    if af.r#static.is_some() {
        result.push_str("static ");
    }

    // function keyword + parameters
    result.push_str("function");
    let params_text = source_text(content, af.parameter_list.span());
    result.push_str(params_text);

    // use() clause for captured variables
    if !captured.is_empty() {
        result.push_str(" use (");
        result.push_str(&captured.join(", "));
        result.push(')');
    }

    // Return type hint (if any)
    if let Some(return_type) = &af.return_type_hint {
        let hint_text = source_text(content, return_type.span());
        result.push_str(hint_text);
    }

    // Body: { return expr; }
    result.push_str(" { return ");
    let expr_text = source_text(content, af.expression.span());
    result.push_str(expr_text);
    result.push_str("; }");

    Some(result)
}

/// Collect all variable references (`$name`) in an expression,
/// excluding parameter names and `$this`.
fn collect_variables_in_expression(
    expr: &Expression<'_>,
    content: &str,
    param_names: &HashSet<String>,
    out: &mut Vec<String>,
) {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            let name = source_text(content, dv.span());
            if name != "$this" && !param_names.contains(name) {
                out.push(name.to_string());
            }
        }
        Expression::Variable(Variable::Indirect(_)) => {}
        Expression::Parenthesized(p) => {
            collect_variables_in_expression(p.expression, content, param_names, out);
        }
        Expression::UnaryPrefix(u) => {
            collect_variables_in_expression(u.operand, content, param_names, out);
        }
        Expression::UnaryPostfix(u) => {
            collect_variables_in_expression(u.operand, content, param_names, out);
        }
        Expression::Binary(b) => {
            collect_variables_in_expression(b.lhs, content, param_names, out);
            collect_variables_in_expression(b.rhs, content, param_names, out);
        }
        Expression::Assignment(a) => {
            collect_variables_in_expression(a.lhs, content, param_names, out);
            collect_variables_in_expression(a.rhs, content, param_names, out);
        }
        Expression::Conditional(c) => {
            collect_variables_in_expression(c.condition, content, param_names, out);
            if let Some(then) = c.then {
                collect_variables_in_expression(then, content, param_names, out);
            }
            collect_variables_in_expression(c.r#else, content, param_names, out);
        }
        Expression::Call(call) => match call {
            Call::Function(fc) => {
                collect_variables_in_expression(fc.function, content, param_names, out);
                for arg in fc.argument_list.arguments.iter() {
                    collect_variables_in_expression(arg.value(), content, param_names, out);
                }
            }
            Call::Method(mc) => {
                collect_variables_in_expression(mc.object, content, param_names, out);
                for arg in mc.argument_list.arguments.iter() {
                    collect_variables_in_expression(arg.value(), content, param_names, out);
                }
            }
            Call::NullSafeMethod(mc) => {
                collect_variables_in_expression(mc.object, content, param_names, out);
                for arg in mc.argument_list.arguments.iter() {
                    collect_variables_in_expression(arg.value(), content, param_names, out);
                }
            }
            Call::StaticMethod(sc) => {
                collect_variables_in_expression(sc.class, content, param_names, out);
                for arg in sc.argument_list.arguments.iter() {
                    collect_variables_in_expression(arg.value(), content, param_names, out);
                }
            }
        },
        Expression::Access(access) => match access {
            Access::Property(pa) => {
                collect_variables_in_expression(pa.object, content, param_names, out);
            }
            Access::NullSafeProperty(pa) => {
                collect_variables_in_expression(pa.object, content, param_names, out);
            }
            Access::StaticProperty(pa) => {
                collect_variables_in_expression(pa.class, content, param_names, out);
            }
            Access::ClassConstant(pa) => {
                collect_variables_in_expression(pa.class, content, param_names, out);
            }
        },
        Expression::Array(arr) => {
            for element in arr.elements.iter() {
                if let array::ArrayElement::KeyValue(kv) = element {
                    collect_variables_in_expression(kv.key, content, param_names, out);
                    collect_variables_in_expression(kv.value, content, param_names, out);
                } else if let array::ArrayElement::Value(val) = element {
                    collect_variables_in_expression(val.value, content, param_names, out);
                }
            }
        }
        Expression::LegacyArray(arr) => {
            for element in arr.elements.iter() {
                if let array::ArrayElement::KeyValue(kv) = element {
                    collect_variables_in_expression(kv.key, content, param_names, out);
                    collect_variables_in_expression(kv.value, content, param_names, out);
                } else if let array::ArrayElement::Value(val) = element {
                    collect_variables_in_expression(val.value, content, param_names, out);
                }
            }
        }
        Expression::ArrayAccess(aa) => {
            collect_variables_in_expression(aa.array, content, param_names, out);
            collect_variables_in_expression(aa.index, content, param_names, out);
        }
        Expression::Instantiation(inst) => {
            collect_variables_in_expression(inst.class, content, param_names, out);
            if let Some(arg_list) = &inst.argument_list {
                for a in arg_list.arguments.iter() {
                    collect_variables_in_expression(a.value(), content, param_names, out);
                }
            }
        }
        Expression::CompositeString(composite) => {
            for part in composite.parts().iter() {
                match part {
                    StringPart::Expression(inner) => {
                        collect_variables_in_expression(inner, content, param_names, out);
                    }
                    StringPart::BracedExpression(braced) => {
                        collect_variables_in_expression(
                            braced.expression,
                            content,
                            param_names,
                            out,
                        );
                    }
                    StringPart::Literal(_) => {}
                }
            }
        }
        Expression::Match(match_expr) => {
            collect_variables_in_expression(match_expr.expression, content, param_names, out);
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(expr_arm) => {
                        for cond in expr_arm.conditions.iter() {
                            collect_variables_in_expression(cond, content, param_names, out);
                        }
                        collect_variables_in_expression(
                            expr_arm.expression,
                            content,
                            param_names,
                            out,
                        );
                    }
                    MatchArm::Default(default_arm) => {
                        collect_variables_in_expression(
                            default_arm.expression,
                            content,
                            param_names,
                            out,
                        );
                    }
                }
            }
        }
        Expression::Construct(construct) => match construct {
            Construct::Isset(isset) => {
                for val in isset.values.iter() {
                    collect_variables_in_expression(val, content, param_names, out);
                }
            }
            Construct::Empty(empty) => {
                collect_variables_in_expression(empty.value, content, param_names, out);
            }
            Construct::Eval(eval) => {
                collect_variables_in_expression(eval.value, content, param_names, out);
            }
            Construct::Include(inc) => {
                collect_variables_in_expression(inc.value, content, param_names, out);
            }
            Construct::IncludeOnce(inc) => {
                collect_variables_in_expression(inc.value, content, param_names, out);
            }
            Construct::Require(req) => {
                collect_variables_in_expression(req.value, content, param_names, out);
            }
            Construct::RequireOnce(req) => {
                collect_variables_in_expression(req.value, content, param_names, out);
            }
            Construct::Print(print) => {
                collect_variables_in_expression(print.value, content, param_names, out);
            }
            Construct::Exit(exit) => {
                if let Some(args) = &exit.arguments {
                    for a in args.arguments.iter() {
                        collect_variables_in_expression(a.value(), content, param_names, out);
                    }
                }
            }
            Construct::Die(die) => {
                if let Some(args) = &die.arguments {
                    for a in args.arguments.iter() {
                        collect_variables_in_expression(a.value(), content, param_names, out);
                    }
                }
            }
        },
        Expression::Throw(throw) => {
            collect_variables_in_expression(throw.exception, content, param_names, out);
        }
        Expression::Clone(clone) => {
            collect_variables_in_expression(clone.object, content, param_names, out);
        }
        Expression::Yield(yield_expr) => match yield_expr {
            Yield::Value(yv) => {
                if let Some(val) = yv.value {
                    collect_variables_in_expression(val, content, param_names, out);
                }
            }
            Yield::Pair(yp) => {
                collect_variables_in_expression(yp.key, content, param_names, out);
                collect_variables_in_expression(yp.value, content, param_names, out);
            }
            Yield::From(yf) => {
                collect_variables_in_expression(yf.iterator, content, param_names, out);
            }
        },
        Expression::List(list) => {
            for element in list.elements.iter() {
                if let array::ArrayElement::KeyValue(kv) = element {
                    collect_variables_in_expression(kv.key, content, param_names, out);
                    collect_variables_in_expression(kv.value, content, param_names, out);
                } else if let array::ArrayElement::Value(val) = element {
                    collect_variables_in_expression(val.value, content, param_names, out);
                }
            }
        }
        Expression::ArrayAppend(append) => {
            collect_variables_in_expression(append.array, content, param_names, out);
        }
        Expression::PartialApplication(partial) => match partial {
            PartialApplication::Function(func_pa) => {
                collect_variables_in_expression(func_pa.function, content, param_names, out);
            }
            PartialApplication::Method(method_pa) => {
                collect_variables_in_expression(method_pa.object, content, param_names, out);
            }
            PartialApplication::StaticMethod(static_pa) => {
                collect_variables_in_expression(static_pa.class, content, param_names, out);
            }
        },
        Expression::AnonymousClass(anon) => {
            if let Some(args) = &anon.argument_list {
                for a in args.arguments.iter() {
                    if let Some(value) = a.value() {
                        collect_variables_in_expression(value, content, param_names, out);
                    }
                }
            }
        }
        Expression::Closure(closure) => {
            // Don't collect variables inside nested closures — they
            // have their own scope and capture semantics.
            let _ = closure;
        }
        Expression::ArrowFunction(inner_af) => {
            // Nested arrow functions capture from our scope, so we
            // need to collect their variables too, but exclude the
            // inner arrow's parameters.
            let mut inner_params = param_names.clone();
            for p in inner_af.parameter_list.parameters.iter() {
                inner_params.insert(source_text(content, p.variable.span()).to_string());
            }
            collect_variables_in_expression(inner_af.expression, content, &inner_params, out);
        }
        _ => {}
    }
}

/// Extract a slice of the source text corresponding to a span.
fn source_text(content: &str, span: mago_span::Span) -> &str {
    &content[span.start.offset as usize..span.end.offset as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_conversion(php: &str) -> Option<String> {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());

        let mut best: Option<(u32, u32, String)> = None;
        // Place cursor inside the arrow function (after `fn`).
        let cursor = php.find("fn").unwrap_or(0) as u32 + 1;
        for stmt in program.statements.iter() {
            find_arrow_in_statement(stmt, cursor, php, &mut best);
        }
        best.map(|(_, _, replacement)| replacement)
    }

    #[test]
    fn simple_expression() {
        let php = r#"<?php $f = fn($x) => $x * 2;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($x) { return $x * 2; }");
    }

    #[test]
    fn with_type_hints() {
        let php = r#"<?php $f = fn(int $x): int => $x * 2;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function(int $x): int { return $x * 2; }");
    }

    #[test]
    fn static_arrow() {
        let php = r#"<?php $f = static fn($x) => $x + 1;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "static function($x) { return $x + 1; }");
    }

    #[test]
    fn captures_outer_variable() {
        let php = r#"<?php $f = fn($x) => $x + $y;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($x) use ($y) { return $x + $y; }");
    }

    #[test]
    fn captures_multiple_variables() {
        let php = r#"<?php $f = fn($x) => $x + $y + $z;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($x) use ($y, $z) { return $x + $y + $z; }");
    }

    #[test]
    fn no_capture_for_this() {
        let php = r#"<?php $f = fn() => $this->name;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function() { return $this->name; }");
    }

    #[test]
    fn no_capture_for_params() {
        let php = r#"<?php $f = fn($a, $b) => $a + $b;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($a, $b) { return $a + $b; }");
    }

    #[test]
    fn no_params_no_captures() {
        let php = r#"<?php $f = fn() => 42;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function() { return 42; }");
    }

    #[test]
    fn with_method_call() {
        let php = r#"<?php $f = fn($item) => $formatter->format($item);"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($item) use ($formatter) { return $formatter->format($item); }"
        );
    }

    #[test]
    fn deduplicates_captures() {
        let php = r#"<?php $f = fn($x) => $y + $y;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, "function($x) use ($y) { return $y + $y; }");
    }

    #[test]
    fn captures_variable_in_string_interpolation() {
        let php = r#"<?php $f = fn($x) => "value: $y";"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(result, r#"function($x) use ($y) { return "value: $y"; }"#);
    }

    #[test]
    fn captures_variable_in_braced_string_interpolation() {
        let php = r#"<?php $f = fn($x) => "value: {$y->prop}";"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            r#"function($x) use ($y) { return "value: {$y->prop}"; }"#
        );
    }

    #[test]
    fn captures_variable_in_match_expression() {
        let php = r#"<?php $f = fn($x) => match($x) { 1 => $y, default => $z };"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($x) use ($y, $z) { return match($x) { 1 => $y, default => $z }; }"
        );
    }

    #[test]
    fn captures_variable_in_isset() {
        let php = r#"<?php $f = fn($x) => isset($y) ? $y : $x;"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($x) use ($y) { return isset($y) ? $y : $x; }"
        );
    }

    #[test]
    fn captures_variable_in_throw() {
        let php = r#"<?php $f = fn($x) => throw new Exception($y);"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($x) use ($y) { return throw new Exception($y); }"
        );
    }

    #[test]
    fn captures_variable_used_as_dynamic_class_name() {
        let php = r#"<?php $f = fn($x) => new $className($x);"#;
        let result = find_conversion(php).unwrap();
        assert_eq!(
            result,
            "function($x) use ($className) { return new $className($x); }"
        );
    }
}
