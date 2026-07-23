//! Byte-offset guard collection for the undefined-variable diagnostic.
//!
//! These helpers scan a function/method body (or its raw source text)
//! for byte offsets that the diagnostic must treat specially: reads
//! guarded by `isset()`/`empty()`, reads under the `@` error
//! suppression operator, and `/** @var Type $var */` inline docblock
//! annotations, which act as a write at the annotation's offset rather
//! than a guard.

use std::collections::HashSet;

use mago_span::HasSpan;
use mago_syntax::cst::*;

// ─── @var annotation collection ─────────────────────────────────────────────

/// Scan the source text for `/** @var Type $varName */` inline
/// docblocks and return each declared variable name paired with the byte
/// offset of its `$` sigil.
///
/// The offset lets callers treat the annotation as a write at that
/// position so it (a) only defines the variable within the scope it
/// appears in, and (b) follows the same "prior write in source order"
/// rule as ordinary assignments.
pub(super) fn collect_var_annotations(content: &str) -> Vec<(String, u32)> {
    let mut vars = Vec::new();
    // Look for patterns like: @var SomeType $varName
    // The regex-like scan: find `@var ` followed by a type, then `$name`.
    let mut line_start = 0usize;
    for line in content.lines() {
        // `lines()` strips the line terminator; track the running byte
        // offset so we can report absolute positions.
        let this_line_start = line_start;
        line_start += line.len() + 1; // +1 for the stripped '\n'

        if !line.contains("@var") {
            continue;
        }
        // Find `@var` and extract the variable name after the type.
        if let Some(var_pos) = line.find("@var") {
            let after_var_off = var_pos + 4;
            let after_var = &line[after_var_off..];
            let ws = after_var.len() - after_var.trim_start().len();
            let after_var = after_var.trim_start();
            // Skip the type (everything before the $).
            if let Some(dollar_pos) = after_var.find('$') {
                let var_part = &after_var[dollar_pos..];
                // Extract the variable name: $[a-zA-Z_][a-zA-Z0-9_]*
                let name_end = var_part
                    .char_indices()
                    .skip(1) // skip the $
                    .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                    .map(|(i, _)| i)
                    .unwrap_or(var_part.len());
                let var_name = &var_part[..name_end];
                // Trim trailing `*/` if present.
                let var_name = var_name.trim_end_matches("*/").trim();
                if var_name.len() > 1 {
                    let dollar_offset = this_line_start + after_var_off + ws + dollar_pos;
                    vars.push((var_name.to_string(), dollar_offset as u32));
                }
            }
        }
    }
    vars
}

// ─── Error suppression (@) offset collection ────────────────────────────────

/// Collect byte offsets of variable reads that are directly under the
/// `@` error suppression operator (e.g. `@$var`).
pub(super) fn collect_error_suppressed_offsets(statements: &[Statement<'_>]) -> HashSet<u32> {
    let mut offsets = HashSet::new();
    for stmt in statements {
        collect_suppressed_from_stmt(stmt, &mut offsets);
    }
    offsets
}

fn collect_suppressed_from_stmt(stmt: &Statement<'_>, offsets: &mut HashSet<u32>) {
    match stmt {
        Statement::Expression(es) => collect_suppressed_from_expr(es.expression, false, offsets),
        Statement::Return(ret) => {
            if let Some(v) = ret.value {
                collect_suppressed_from_expr(v, false, offsets);
            }
        }
        Statement::Echo(echo) => {
            for v in echo.values.iter() {
                collect_suppressed_from_expr(v, false, offsets);
            }
        }
        Statement::If(if_stmt) => {
            collect_suppressed_from_expr(if_stmt.condition, false, offsets);
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_suppressed_from_stmt(body.statement, offsets);
                    for clause in body.else_if_clauses.iter() {
                        collect_suppressed_from_expr(clause.condition, false, offsets);
                        collect_suppressed_from_stmt(clause.statement, offsets);
                    }
                    if let Some(ref el) = body.else_clause {
                        collect_suppressed_from_stmt(el.statement, offsets);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                    for clause in body.else_if_clauses.iter() {
                        collect_suppressed_from_expr(clause.condition, false, offsets);
                        for s in clause.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Foreach(foreach) => {
            collect_suppressed_from_expr(foreach.expression, false, offsets);
            match &foreach.body {
                ForeachBody::Statement(s) => collect_suppressed_from_stmt(s, offsets),
                ForeachBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::While(w) => {
            collect_suppressed_from_expr(w.condition, false, offsets);
            match &w.body {
                WhileBody::Statement(s) => collect_suppressed_from_stmt(s, offsets),
                WhileBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::DoWhile(dw) => {
            collect_suppressed_from_stmt(dw.statement, offsets);
            collect_suppressed_from_expr(dw.condition, false, offsets);
        }
        Statement::For(for_stmt) => {
            for e in for_stmt.initializations.iter() {
                collect_suppressed_from_expr(e, false, offsets);
            }
            for e in for_stmt.conditions.iter() {
                collect_suppressed_from_expr(e, false, offsets);
            }
            for e in for_stmt.increments.iter() {
                collect_suppressed_from_expr(e, false, offsets);
            }
            match &for_stmt.body {
                ForBody::Statement(s) => collect_suppressed_from_stmt(s, offsets),
                ForBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::Switch(sw) => {
            collect_suppressed_from_expr(sw.expression, false, offsets);
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(sc) => {
                        collect_suppressed_from_expr(sc.expression, false, offsets);
                        for s in sc.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                    SwitchCase::Default(dc) => {
                        for s in dc.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_suppressed_from_stmt(s, offsets);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_suppressed_from_stmt(s, offsets);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_suppressed_from_stmt(s, offsets);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_suppressed_from_stmt(s, offsets);
            }
        }
        _ => {}
    }
}

fn collect_suppressed_from_expr(
    expr: &Expression<'_>,
    under_error_control: bool,
    offsets: &mut HashSet<u32>,
) {
    match expr {
        Expression::UnaryPrefix(unary) if unary.operator.is_error_control() => {
            // The operand is under @.
            collect_suppressed_from_expr(unary.operand, true, offsets);
        }
        Expression::Variable(Variable::Direct(dv)) if under_error_control => {
            offsets.insert(dv.span().start.offset);
        }
        Expression::UnaryPrefix(unary) => {
            collect_suppressed_from_expr(unary.operand, under_error_control, offsets);
        }
        Expression::UnaryPostfix(unary) => {
            collect_suppressed_from_expr(unary.operand, under_error_control, offsets);
        }
        Expression::Assignment(a) => {
            collect_suppressed_from_expr(a.lhs, under_error_control, offsets);
            collect_suppressed_from_expr(a.rhs, under_error_control, offsets);
        }
        Expression::Binary(b) => {
            collect_suppressed_from_expr(b.lhs, under_error_control, offsets);
            collect_suppressed_from_expr(b.rhs, under_error_control, offsets);
        }
        Expression::Parenthesized(p) => {
            collect_suppressed_from_expr(p.expression, under_error_control, offsets);
        }
        Expression::Call(Call::Function(fc)) => {
            collect_suppressed_from_expr(fc.function, under_error_control, offsets);
            for arg in fc.argument_list.arguments.iter() {
                collect_suppressed_from_expr(arg.value(), under_error_control, offsets);
            }
        }
        Expression::Call(Call::Method(mc)) => {
            collect_suppressed_from_expr(mc.object, under_error_control, offsets);
            for arg in mc.argument_list.arguments.iter() {
                collect_suppressed_from_expr(arg.value(), under_error_control, offsets);
            }
        }
        Expression::Access(Access::Property(pa)) => {
            collect_suppressed_from_expr(pa.object, under_error_control, offsets);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_suppressed_from_expr(pa.object, under_error_control, offsets);
        }
        Expression::ArrayAccess(aa) => {
            collect_suppressed_from_expr(aa.array, under_error_control, offsets);
            collect_suppressed_from_expr(aa.index, under_error_control, offsets);
        }
        Expression::Conditional(c) => {
            collect_suppressed_from_expr(c.condition, under_error_control, offsets);
            if let Some(t) = c.then {
                collect_suppressed_from_expr(t, under_error_control, offsets);
            }
            collect_suppressed_from_expr(c.r#else, under_error_control, offsets);
        }
        // Don't recurse into closures/arrow functions.
        _ => {}
    }
}

// ─── isset() / empty() guarded offset collection ───────────────────────────

/// Collect byte offsets of variable reads that appear directly inside
/// `isset()` or `empty()` calls.  These variables are being guarded,
/// not used.
pub(super) fn collect_guarded_offsets(statements: &[Statement<'_>]) -> HashSet<u32> {
    let mut offsets = HashSet::new();
    for stmt in statements {
        collect_guarded_from_stmt(stmt, &mut offsets);
    }
    offsets
}

fn collect_guarded_from_stmt(stmt: &Statement<'_>, offsets: &mut HashSet<u32>) {
    match stmt {
        Statement::Expression(es) => collect_guarded_from_expr(es.expression, false, offsets),
        Statement::Return(ret) => {
            if let Some(v) = ret.value {
                collect_guarded_from_expr(v, false, offsets);
            }
        }
        Statement::Echo(echo) => {
            for v in echo.values.iter() {
                collect_guarded_from_expr(v, false, offsets);
            }
        }
        Statement::If(if_stmt) => {
            collect_guarded_from_expr(if_stmt.condition, false, offsets);
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_guarded_from_stmt(body.statement, offsets);
                    for clause in body.else_if_clauses.iter() {
                        collect_guarded_from_expr(clause.condition, false, offsets);
                        collect_guarded_from_stmt(clause.statement, offsets);
                    }
                    if let Some(ref el) = body.else_clause {
                        collect_guarded_from_stmt(el.statement, offsets);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                    for clause in body.else_if_clauses.iter() {
                        collect_guarded_from_expr(clause.condition, false, offsets);
                        for s in clause.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Foreach(foreach) => {
            collect_guarded_from_expr(foreach.expression, false, offsets);
            match &foreach.body {
                ForeachBody::Statement(s) => collect_guarded_from_stmt(s, offsets),
                ForeachBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::While(w) => {
            collect_guarded_from_expr(w.condition, false, offsets);
            match &w.body {
                WhileBody::Statement(s) => collect_guarded_from_stmt(s, offsets),
                WhileBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::DoWhile(dw) => {
            collect_guarded_from_stmt(dw.statement, offsets);
            collect_guarded_from_expr(dw.condition, false, offsets);
        }
        Statement::For(for_stmt) => {
            for e in for_stmt.initializations.iter() {
                collect_guarded_from_expr(e, false, offsets);
            }
            for e in for_stmt.conditions.iter() {
                collect_guarded_from_expr(e, false, offsets);
            }
            for e in for_stmt.increments.iter() {
                collect_guarded_from_expr(e, false, offsets);
            }
            match &for_stmt.body {
                ForBody::Statement(s) => collect_guarded_from_stmt(s, offsets),
                ForBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::Switch(sw) => {
            collect_guarded_from_expr(sw.expression, false, offsets);
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(sc) => {
                        collect_guarded_from_expr(sc.expression, false, offsets);
                        for s in sc.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                    SwitchCase::Default(dc) => {
                        for s in dc.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_guarded_from_stmt(s, offsets);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_guarded_from_stmt(s, offsets);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_guarded_from_stmt(s, offsets);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_guarded_from_stmt(s, offsets);
            }
        }
        _ => {}
    }
}

fn collect_guarded_from_expr(
    expr: &Expression<'_>,
    inside_guard: bool,
    offsets: &mut HashSet<u32>,
) {
    match expr {
        Expression::Construct(Construct::Isset(isset)) => {
            // All variables inside isset() are guarded.
            for val in isset.values.iter() {
                collect_guard_targets(val, offsets);
            }
        }
        Expression::Construct(Construct::Empty(empty)) => {
            // The variable inside empty() is guarded.
            collect_guard_targets(empty.value, offsets);
        }
        Expression::UnaryPrefix(unary) => {
            collect_guarded_from_expr(unary.operand, inside_guard, offsets);
        }
        Expression::UnaryPostfix(unary) => {
            collect_guarded_from_expr(unary.operand, inside_guard, offsets);
        }
        Expression::Assignment(a) => {
            collect_guarded_from_expr(a.lhs, inside_guard, offsets);
            collect_guarded_from_expr(a.rhs, inside_guard, offsets);
        }
        Expression::Binary(b) => {
            collect_guarded_from_expr(b.lhs, inside_guard, offsets);
            collect_guarded_from_expr(b.rhs, inside_guard, offsets);
        }
        Expression::Parenthesized(p) => {
            collect_guarded_from_expr(p.expression, inside_guard, offsets);
        }
        Expression::Conditional(c) => {
            collect_guarded_from_expr(c.condition, inside_guard, offsets);
            if let Some(t) = c.then {
                collect_guarded_from_expr(t, inside_guard, offsets);
            }
            collect_guarded_from_expr(c.r#else, inside_guard, offsets);
        }
        Expression::Call(Call::Function(fc)) => {
            collect_guarded_from_expr(fc.function, inside_guard, offsets);
            for arg in fc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Call(Call::Method(mc)) => {
            collect_guarded_from_expr(mc.object, inside_guard, offsets);
            for arg in mc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            collect_guarded_from_expr(mc.object, inside_guard, offsets);
            for arg in mc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            collect_guarded_from_expr(sc.class, inside_guard, offsets);
            for arg in sc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Access(Access::Property(pa)) => {
            collect_guarded_from_expr(pa.object, inside_guard, offsets);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_guarded_from_expr(pa.object, inside_guard, offsets);
        }
        Expression::ArrayAccess(aa) => {
            collect_guarded_from_expr(aa.array, inside_guard, offsets);
            collect_guarded_from_expr(aa.index, inside_guard, offsets);
        }
        Expression::Array(arr) => {
            for e in arr.elements.iter() {
                collect_guarded_from_array_elem(e, inside_guard, offsets);
            }
        }
        Expression::LegacyArray(arr) => {
            for e in arr.elements.iter() {
                collect_guarded_from_array_elem(e, inside_guard, offsets);
            }
        }
        Expression::Instantiation(inst) => {
            collect_guarded_from_expr(inst.class, inside_guard, offsets);
            if let Some(ref al) = inst.argument_list {
                for arg in al.arguments.iter() {
                    collect_guarded_from_expr(arg.value(), inside_guard, offsets);
                }
            }
        }
        // Don't recurse into closures/arrow functions.
        _ => {}
    }
}

fn collect_guarded_from_array_elem(
    elem: &ArrayElement<'_>,
    inside_guard: bool,
    offsets: &mut HashSet<u32>,
) {
    match elem {
        ArrayElement::KeyValue(kv) => {
            collect_guarded_from_expr(kv.key, inside_guard, offsets);
            collect_guarded_from_expr(kv.value, inside_guard, offsets);
        }
        ArrayElement::Value(v) => {
            collect_guarded_from_expr(v.value, inside_guard, offsets);
        }
        ArrayElement::Variadic(s) => {
            collect_guarded_from_expr(s.value, inside_guard, offsets);
        }
        ArrayElement::Missing(_) => {}
    }
}

/// Collect all variable offsets within an expression that is a target
/// of `isset()` or `empty()`.  This handles simple variables,
/// array access chains (`$arr['key']`), and property chains
/// (`$obj->prop`).
fn collect_guard_targets(expr: &Expression<'_>, offsets: &mut HashSet<u32>) {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            offsets.insert(dv.span().start.offset);
        }
        Expression::ArrayAccess(aa) => {
            collect_guard_targets(aa.array, offsets);
            // Don't mark the index expression as guarded.
        }
        Expression::Access(Access::Property(pa)) => {
            collect_guard_targets(pa.object, offsets);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_guard_targets(pa.object, offsets);
        }
        Expression::Access(Access::StaticProperty(spa)) => {
            collect_guard_targets(spa.class, offsets);
        }
        _ => {}
    }
}
