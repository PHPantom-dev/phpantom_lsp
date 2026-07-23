//! Whole-scope feature detection for the undefined/unused-variable
//! diagnostics.
//!
//! These helpers scan a function/method body for constructs that make
//! per-variable static analysis unsound (variable variables, `extract()`)
//! or that reference variables by string name rather than direct use
//! (`compact()`, `get_defined_vars()`). The caller uses the results to
//! bail out of the scope entirely or to treat the named variables as
//! always defined/used.

use std::collections::HashSet;

use mago_syntax::cst::*;

// ─── Dynamic variable / extract detection ───────────────────────────────────

/// Returns `true` if the statements contain variable variables (`$$x`)
/// anywhere in the function body.
pub(super) fn has_dynamic_variables(statements: &[Statement<'_>]) -> bool {
    for stmt in statements {
        if stmt_has_dynamic_var(stmt) {
            return true;
        }
    }
    false
}

fn stmt_has_dynamic_var(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Expression(es) => expr_has_dynamic_var(es.expression),
        Statement::Return(ret) => ret.value.is_some_and(|v| expr_has_dynamic_var(v)),
        Statement::Echo(echo) => echo.values.iter().any(|v| expr_has_dynamic_var(v)),
        Statement::If(if_stmt) => {
            if expr_has_dynamic_var(if_stmt.condition) {
                return true;
            }
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    if stmt_has_dynamic_var(body.statement) {
                        return true;
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_dynamic_var(clause.condition)
                            || stmt_has_dynamic_var(clause.statement)
                        {
                            return true;
                        }
                    }
                    if let Some(ref el) = body.else_clause
                        && stmt_has_dynamic_var(el.statement)
                    {
                        return true;
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        if stmt_has_dynamic_var(s) {
                            return true;
                        }
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_dynamic_var(clause.condition) {
                            return true;
                        }
                        for s in clause.statements.iter() {
                            if stmt_has_dynamic_var(s) {
                                return true;
                            }
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            if stmt_has_dynamic_var(s) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Statement::Foreach(foreach) => {
            expr_has_dynamic_var(foreach.expression)
                || match &foreach.body {
                    ForeachBody::Statement(s) => stmt_has_dynamic_var(s),
                    ForeachBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                }
        }
        Statement::While(w) => {
            expr_has_dynamic_var(w.condition)
                || match &w.body {
                    WhileBody::Statement(s) => stmt_has_dynamic_var(s),
                    WhileBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                }
        }
        Statement::DoWhile(dw) => {
            stmt_has_dynamic_var(dw.statement) || expr_has_dynamic_var(dw.condition)
        }
        Statement::For(for_stmt) => {
            for_stmt
                .initializations
                .iter()
                .any(|e| expr_has_dynamic_var(e))
                || for_stmt.conditions.iter().any(|e| expr_has_dynamic_var(e))
                || for_stmt.increments.iter().any(|e| expr_has_dynamic_var(e))
                || match &for_stmt.body {
                    ForBody::Statement(s) => stmt_has_dynamic_var(s),
                    ForBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                }
        }
        Statement::Switch(sw) => {
            expr_has_dynamic_var(sw.expression)
                || sw.body.cases().iter().any(|c| match c {
                    SwitchCase::Expression(sc) => {
                        expr_has_dynamic_var(sc.expression)
                            || sc.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                    SwitchCase::Default(dc) => {
                        dc.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                })
        }
        Statement::Try(try_stmt) => {
            try_stmt
                .block
                .statements
                .iter()
                .any(|s| stmt_has_dynamic_var(s))
                || try_stmt
                    .catch_clauses
                    .iter()
                    .any(|c| c.block.statements.iter().any(|s| stmt_has_dynamic_var(s)))
                || try_stmt
                    .finally_clause
                    .as_ref()
                    .is_some_and(|f| f.block.statements.iter().any(|s| stmt_has_dynamic_var(s)))
        }
        Statement::Block(block) => block.statements.iter().any(|s| stmt_has_dynamic_var(s)),
        Statement::Unset(_) => false,
        Statement::Global(_) => false,
        Statement::Static(_) => false,
        _ => false,
    }
}

fn expr_has_dynamic_var(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Variable(Variable::Indirect(_)) => true,
        Expression::Variable(Variable::Nested(_)) => true,
        Expression::Variable(_) => false,
        Expression::Assignment(a) => expr_has_dynamic_var(a.lhs) || expr_has_dynamic_var(a.rhs),
        Expression::Binary(b) => expr_has_dynamic_var(b.lhs) || expr_has_dynamic_var(b.rhs),
        Expression::UnaryPrefix(u) => expr_has_dynamic_var(u.operand),
        Expression::UnaryPostfix(u) => expr_has_dynamic_var(u.operand),
        Expression::Parenthesized(p) => expr_has_dynamic_var(p.expression),
        Expression::Call(call) => match call {
            Call::Function(fc) => {
                expr_has_dynamic_var(fc.function)
                    || fc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
            Call::Method(mc) => {
                expr_has_dynamic_var(mc.object)
                    || mc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
            Call::NullSafeMethod(mc) => {
                expr_has_dynamic_var(mc.object)
                    || mc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
            Call::StaticMethod(sc) => {
                expr_has_dynamic_var(sc.class)
                    || sc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
        },
        Expression::Access(access) => match access {
            Access::Property(pa) => expr_has_dynamic_var(pa.object),
            Access::NullSafeProperty(pa) => expr_has_dynamic_var(pa.object),
            Access::StaticProperty(spa) => expr_has_dynamic_var(spa.class),
            Access::ClassConstant(cca) => expr_has_dynamic_var(cca.class),
        },
        Expression::ArrayAccess(aa) => {
            expr_has_dynamic_var(aa.array) || expr_has_dynamic_var(aa.index)
        }
        Expression::Conditional(c) => {
            expr_has_dynamic_var(c.condition)
                || c.then.is_some_and(|t| expr_has_dynamic_var(t))
                || expr_has_dynamic_var(c.r#else)
        }
        Expression::Instantiation(inst) => {
            expr_has_dynamic_var(inst.class)
                || inst
                    .argument_list
                    .as_ref()
                    .is_some_and(|al| al.arguments.iter().any(|a| expr_has_dynamic_var(a.value())))
        }
        Expression::Array(arr) => arr.elements.iter().any(|e| array_elem_has_dynamic_var(e)),
        Expression::LegacyArray(arr) => arr.elements.iter().any(|e| array_elem_has_dynamic_var(e)),
        Expression::Throw(t) => expr_has_dynamic_var(t.exception),
        Expression::Clone(c) => expr_has_dynamic_var(c.object),
        Expression::Match(m) => {
            expr_has_dynamic_var(m.expression)
                || m.arms.iter().any(|arm| match arm {
                    MatchArm::Expression(ea) => {
                        ea.conditions.iter().any(|c| expr_has_dynamic_var(c))
                            || expr_has_dynamic_var(ea.expression)
                    }
                    MatchArm::Default(da) => expr_has_dynamic_var(da.expression),
                })
        }
        // Closures and arrow functions have their own scope — don't
        // recurse into them for dynamic variable detection.
        Expression::Closure(_) | Expression::ArrowFunction(_) => false,
        _ => false,
    }
}

fn array_elem_has_dynamic_var(elem: &ArrayElement<'_>) -> bool {
    match elem {
        ArrayElement::KeyValue(kv) => {
            expr_has_dynamic_var(kv.key) || expr_has_dynamic_var(kv.value)
        }
        ArrayElement::Value(v) => expr_has_dynamic_var(v.value),
        ArrayElement::Variadic(s) => expr_has_dynamic_var(s.value),
        ArrayElement::Missing(_) => false,
    }
}

/// Returns `true` if the statements contain a call to `extract()`.
pub(super) fn has_extract_call(statements: &[Statement<'_>]) -> bool {
    for stmt in statements {
        if stmt_has_extract(stmt) {
            return true;
        }
    }
    false
}

fn stmt_has_extract(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Expression(es) => expr_has_extract(es.expression),
        Statement::Return(ret) => ret.value.is_some_and(|v| expr_has_extract(v)),
        Statement::Echo(echo) => echo.values.iter().any(|v| expr_has_extract(v)),
        Statement::If(if_stmt) => {
            if expr_has_extract(if_stmt.condition) {
                return true;
            }
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    if stmt_has_extract(body.statement) {
                        return true;
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_extract(clause.condition) || stmt_has_extract(clause.statement)
                        {
                            return true;
                        }
                    }
                    if let Some(ref el) = body.else_clause
                        && stmt_has_extract(el.statement)
                    {
                        return true;
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        if stmt_has_extract(s) {
                            return true;
                        }
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_extract(clause.condition) {
                            return true;
                        }
                        for s in clause.statements.iter() {
                            if stmt_has_extract(s) {
                                return true;
                            }
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            if stmt_has_extract(s) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Statement::Foreach(foreach) => {
            expr_has_extract(foreach.expression)
                || match &foreach.body {
                    ForeachBody::Statement(s) => stmt_has_extract(s),
                    ForeachBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_extract(s))
                    }
                }
        }
        Statement::While(w) => {
            expr_has_extract(w.condition)
                || match &w.body {
                    WhileBody::Statement(s) => stmt_has_extract(s),
                    WhileBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_extract(s))
                    }
                }
        }
        Statement::DoWhile(dw) => stmt_has_extract(dw.statement) || expr_has_extract(dw.condition),
        Statement::For(for_stmt) => {
            for_stmt.initializations.iter().any(|e| expr_has_extract(e))
                || for_stmt.conditions.iter().any(|e| expr_has_extract(e))
                || for_stmt.increments.iter().any(|e| expr_has_extract(e))
                || match &for_stmt.body {
                    ForBody::Statement(s) => stmt_has_extract(s),
                    ForBody::ColonDelimited(b) => b.statements.iter().any(|s| stmt_has_extract(s)),
                }
        }
        Statement::Switch(sw) => {
            expr_has_extract(sw.expression)
                || sw.body.cases().iter().any(|c| match c {
                    SwitchCase::Expression(sc) => {
                        expr_has_extract(sc.expression)
                            || sc.statements.iter().any(|s| stmt_has_extract(s))
                    }
                    SwitchCase::Default(dc) => dc.statements.iter().any(|s| stmt_has_extract(s)),
                })
        }
        Statement::Try(try_stmt) => {
            try_stmt
                .block
                .statements
                .iter()
                .any(|s| stmt_has_extract(s))
                || try_stmt
                    .catch_clauses
                    .iter()
                    .any(|c| c.block.statements.iter().any(|s| stmt_has_extract(s)))
                || try_stmt
                    .finally_clause
                    .as_ref()
                    .is_some_and(|f| f.block.statements.iter().any(|s| stmt_has_extract(s)))
        }
        Statement::Block(block) => block.statements.iter().any(|s| stmt_has_extract(s)),
        _ => false,
    }
}

fn expr_has_extract(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"extract")
            {
                return true;
            }
            // Check arguments recursively.
            fc.argument_list
                .arguments
                .iter()
                .any(|a| expr_has_extract(a.value()))
        }
        Expression::Assignment(a) => expr_has_extract(a.lhs) || expr_has_extract(a.rhs),
        Expression::Binary(b) => expr_has_extract(b.lhs) || expr_has_extract(b.rhs),
        Expression::UnaryPrefix(u) => expr_has_extract(u.operand),
        Expression::UnaryPostfix(u) => expr_has_extract(u.operand),
        Expression::Parenthesized(p) => expr_has_extract(p.expression),
        Expression::Conditional(c) => {
            expr_has_extract(c.condition)
                || c.then.is_some_and(|t| expr_has_extract(t))
                || expr_has_extract(c.r#else)
        }
        Expression::Call(Call::Method(mc)) => {
            expr_has_extract(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_extract(a.value()))
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            expr_has_extract(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_extract(a.value()))
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            expr_has_extract(sc.class)
                || sc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_extract(a.value()))
        }
        // Don't recurse into closures/arrow functions.
        Expression::Closure(_) | Expression::ArrowFunction(_) => false,
        _ => false,
    }
}

// ─── compact() variable collection ──────────────────────────────────────────

/// Collect variable names referenced by `compact('var1', 'var2', …)`
/// calls.  These variables are used by string name and should be
/// considered defined.
pub(crate) fn collect_compact_vars(statements: &[Statement<'_>]) -> HashSet<String> {
    let mut vars = HashSet::new();
    for stmt in statements {
        collect_compact_from_stmt(stmt, &mut vars);
    }
    vars
}

fn collect_compact_from_stmt(stmt: &Statement<'_>, vars: &mut HashSet<String>) {
    match stmt {
        Statement::Expression(es) => collect_compact_from_expr(es.expression, vars),
        Statement::Return(ret) => {
            if let Some(v) = ret.value {
                collect_compact_from_expr(v, vars);
            }
        }
        Statement::Echo(echo) => {
            for v in echo.values.iter() {
                collect_compact_from_expr(v, vars);
            }
        }
        Statement::If(if_stmt) => {
            collect_compact_from_expr(if_stmt.condition, vars);
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_compact_from_stmt(body.statement, vars);
                    for clause in body.else_if_clauses.iter() {
                        collect_compact_from_expr(clause.condition, vars);
                        collect_compact_from_stmt(clause.statement, vars);
                    }
                    if let Some(ref el) = body.else_clause {
                        collect_compact_from_stmt(el.statement, vars);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                    for clause in body.else_if_clauses.iter() {
                        collect_compact_from_expr(clause.condition, vars);
                        for s in clause.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                }
            }
        }
        Statement::Foreach(foreach) => {
            collect_compact_from_expr(foreach.expression, vars);
            match &foreach.body {
                ForeachBody::Statement(s) => collect_compact_from_stmt(s, vars),
                ForeachBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                }
            }
        }
        Statement::While(w) => {
            collect_compact_from_expr(w.condition, vars);
            match &w.body {
                WhileBody::Statement(s) => collect_compact_from_stmt(s, vars),
                WhileBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                }
            }
        }
        Statement::DoWhile(dw) => {
            collect_compact_from_stmt(dw.statement, vars);
            collect_compact_from_expr(dw.condition, vars);
        }
        Statement::For(for_stmt) => {
            for e in for_stmt.initializations.iter() {
                collect_compact_from_expr(e, vars);
            }
            for e in for_stmt.conditions.iter() {
                collect_compact_from_expr(e, vars);
            }
            for e in for_stmt.increments.iter() {
                collect_compact_from_expr(e, vars);
            }
            match &for_stmt.body {
                ForBody::Statement(s) => collect_compact_from_stmt(s, vars),
                ForBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                }
            }
        }
        Statement::Switch(sw) => {
            collect_compact_from_expr(sw.expression, vars);
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(sc) => {
                        collect_compact_from_expr(sc.expression, vars);
                        for s in sc.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                    SwitchCase::Default(dc) => {
                        for s in dc.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_compact_from_stmt(s, vars);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_compact_from_stmt(s, vars);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_compact_from_stmt(s, vars);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_compact_from_stmt(s, vars);
            }
        }
        _ => {}
    }
}

fn collect_compact_from_expr(expr: &Expression<'_>, vars: &mut HashSet<String>) {
    match expr {
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"compact")
            {
                // Each argument is a variable name (string literal) or
                // an array of names (possibly nested), matching the
                // forms compact() accepts.
                for arg in fc.argument_list.arguments.iter() {
                    collect_compact_name_from_arg(arg.value(), vars);
                }
            }
            // Also recurse into arguments for nested compact() calls.
            for arg in fc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        Expression::Assignment(a) => {
            collect_compact_from_expr(a.lhs, vars);
            collect_compact_from_expr(a.rhs, vars);
        }
        Expression::Binary(b) => {
            collect_compact_from_expr(b.lhs, vars);
            collect_compact_from_expr(b.rhs, vars);
        }
        Expression::Parenthesized(p) => collect_compact_from_expr(p.expression, vars),
        Expression::Conditional(c) => {
            collect_compact_from_expr(c.condition, vars);
            if let Some(t) = c.then {
                collect_compact_from_expr(t, vars);
            }
            collect_compact_from_expr(c.r#else, vars);
        }
        Expression::Call(Call::Method(mc)) => {
            collect_compact_from_expr(mc.object, vars);
            for arg in mc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            collect_compact_from_expr(mc.object, vars);
            for arg in mc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            collect_compact_from_expr(sc.class, vars);
            for arg in sc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        // Don't recurse into closures/arrow functions.
        _ => {}
    }
}

/// Collect variable names from a single `compact()` argument. A string
/// literal names a variable directly; an array literal is descended
/// into recursively so `compact(['a', ['b']])` collects both names.
fn collect_compact_name_from_arg(expr: &Expression<'_>, vars: &mut HashSet<String>) {
    match expr {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the interpreted string content (without
            // quotes); fall back to `raw` and strip quotes manually
            // if `value` is `None`.
            let name: &str = if let Some(v) = s.value {
                crate::atom::bytes_to_str(v)
            } else {
                let raw = crate::atom::bytes_to_str(s.raw);
                raw.strip_prefix('\'')
                    .or_else(|| raw.strip_prefix('"'))
                    .and_then(|inner| inner.strip_suffix('\'').or_else(|| inner.strip_suffix('"')))
                    .unwrap_or(raw)
            };
            if !name.is_empty() {
                vars.insert(format!("${}", name));
            }
        }
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                collect_compact_name_from_elem(elem, vars);
            }
        }
        Expression::LegacyArray(arr) => {
            for elem in arr.elements.iter() {
                collect_compact_name_from_elem(elem, vars);
            }
        }
        _ => {}
    }
}

/// Collect variable names from one element of an array passed to
/// `compact()`. Keys are ignored; values are names or nested arrays.
fn collect_compact_name_from_elem(elem: &ArrayElement<'_>, vars: &mut HashSet<String>) {
    match elem {
        ArrayElement::KeyValue(kv) => collect_compact_name_from_arg(kv.value, vars),
        ArrayElement::Value(v) => collect_compact_name_from_arg(v.value, vars),
        ArrayElement::Variadic(s) => collect_compact_name_from_arg(s.value, vars),
        ArrayElement::Missing(_) => {}
    }
}

// ─── get_defined_vars() detection ───────────────────────────────────────────

/// Returns true if the statements contain a call to `get_defined_vars()`.
/// When present in a scope, all variables defined in that scope are
/// considered used (e.g. for debug dumps), so unused-variable diagnostics
/// should be suppressed for them.
pub(crate) fn has_get_defined_vars(statements: &[Statement<'_>]) -> bool {
    for stmt in statements {
        if stmt_has_get_defined_vars(stmt) {
            return true;
        }
    }
    false
}

fn stmt_has_get_defined_vars(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Expression(es) => expr_has_get_defined_vars(es.expression),
        Statement::Return(ret) => ret.value.is_some_and(|v| expr_has_get_defined_vars(v)),
        Statement::Echo(echo) => echo.values.iter().any(|v| expr_has_get_defined_vars(v)),
        Statement::If(if_stmt) => {
            if expr_has_get_defined_vars(if_stmt.condition) {
                return true;
            }
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    if stmt_has_get_defined_vars(body.statement) {
                        return true;
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_get_defined_vars(clause.condition)
                            || stmt_has_get_defined_vars(clause.statement)
                        {
                            return true;
                        }
                    }
                    if let Some(ref el) = body.else_clause
                        && stmt_has_get_defined_vars(el.statement)
                    {
                        return true;
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        if stmt_has_get_defined_vars(s) {
                            return true;
                        }
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_get_defined_vars(clause.condition) {
                            return true;
                        }
                        for s in clause.statements.iter() {
                            if stmt_has_get_defined_vars(s) {
                                return true;
                            }
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            if stmt_has_get_defined_vars(s) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Statement::Foreach(foreach) => {
            expr_has_get_defined_vars(foreach.expression)
                || match &foreach.body {
                    ForeachBody::Statement(s) => stmt_has_get_defined_vars(s),
                    ForeachBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                }
        }
        Statement::While(w) => {
            expr_has_get_defined_vars(w.condition)
                || match &w.body {
                    WhileBody::Statement(s) => stmt_has_get_defined_vars(s),
                    WhileBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                }
        }
        Statement::DoWhile(dw) => {
            stmt_has_get_defined_vars(dw.statement) || expr_has_get_defined_vars(dw.condition)
        }
        Statement::For(for_stmt) => {
            for_stmt
                .initializations
                .iter()
                .any(|e| expr_has_get_defined_vars(e))
                || for_stmt
                    .conditions
                    .iter()
                    .any(|e| expr_has_get_defined_vars(e))
                || for_stmt
                    .increments
                    .iter()
                    .any(|e| expr_has_get_defined_vars(e))
                || match &for_stmt.body {
                    ForBody::Statement(s) => stmt_has_get_defined_vars(s),
                    ForBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                }
        }
        Statement::Switch(sw) => {
            expr_has_get_defined_vars(sw.expression)
                || sw.body.cases().iter().any(|c| match c {
                    SwitchCase::Expression(sc) => {
                        expr_has_get_defined_vars(sc.expression)
                            || sc.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                    SwitchCase::Default(dc) => {
                        dc.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                })
        }
        Statement::Try(try_stmt) => {
            try_stmt
                .block
                .statements
                .iter()
                .any(|s| stmt_has_get_defined_vars(s))
                || try_stmt.catch_clauses.iter().any(|c| {
                    c.block
                        .statements
                        .iter()
                        .any(|s| stmt_has_get_defined_vars(s))
                })
                || try_stmt.finally_clause.as_ref().is_some_and(|f| {
                    f.block
                        .statements
                        .iter()
                        .any(|s| stmt_has_get_defined_vars(s))
                })
        }
        Statement::Block(block) => block
            .statements
            .iter()
            .any(|s| stmt_has_get_defined_vars(s)),
        _ => false,
    }
}

fn expr_has_get_defined_vars(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"get_defined_vars")
            {
                return true;
            }
            expr_has_get_defined_vars(fc.function)
                // Recurse into arguments for nested calls.
                || fc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Call(Call::Method(mc)) => {
            expr_has_get_defined_vars(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            expr_has_get_defined_vars(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            expr_has_get_defined_vars(sc.class)
                || sc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => expr_has_get_defined_vars(pa.object),
            Access::NullSafeProperty(pa) => expr_has_get_defined_vars(pa.object),
            Access::StaticProperty(spa) => expr_has_get_defined_vars(spa.class),
            Access::ClassConstant(cca) => expr_has_get_defined_vars(cca.class),
        },
        Expression::ArrayAccess(aa) => {
            expr_has_get_defined_vars(aa.array) || expr_has_get_defined_vars(aa.index)
        }
        Expression::ArrayAppend(append) => expr_has_get_defined_vars(append.array),
        Expression::Array(arr) => arr
            .elements
            .iter()
            .any(|e| array_elem_has_get_defined_vars(e)),
        Expression::LegacyArray(arr) => arr
            .elements
            .iter()
            .any(|e| array_elem_has_get_defined_vars(e)),
        Expression::List(list) => list
            .elements
            .iter()
            .any(|e| array_elem_has_get_defined_vars(e)),
        Expression::Assignment(a) => {
            expr_has_get_defined_vars(a.lhs) || expr_has_get_defined_vars(a.rhs)
        }
        Expression::Binary(b) => {
            expr_has_get_defined_vars(b.lhs) || expr_has_get_defined_vars(b.rhs)
        }
        Expression::UnaryPrefix(u) => expr_has_get_defined_vars(u.operand),
        Expression::UnaryPostfix(u) => expr_has_get_defined_vars(u.operand),
        Expression::Parenthesized(p) => expr_has_get_defined_vars(p.expression),
        Expression::Conditional(c) => {
            expr_has_get_defined_vars(c.condition)
                || c.then.is_some_and(|t| expr_has_get_defined_vars(t))
                || expr_has_get_defined_vars(c.r#else)
        }
        Expression::Instantiation(inst) => {
            expr_has_get_defined_vars(inst.class)
                || inst.argument_list.as_ref().is_some_and(|al| {
                    al.arguments
                        .iter()
                        .any(|a| expr_has_get_defined_vars(a.value()))
                })
        }
        Expression::Throw(t) => expr_has_get_defined_vars(t.exception),
        Expression::Clone(c) => expr_has_get_defined_vars(c.object),
        Expression::Yield(yield_expr) => match yield_expr {
            Yield::Value(yv) => yv.value.is_some_and(expr_has_get_defined_vars),
            Yield::Pair(yp) => {
                expr_has_get_defined_vars(yp.key) || expr_has_get_defined_vars(yp.value)
            }
            Yield::From(yf) => expr_has_get_defined_vars(yf.iterator),
        },
        Expression::Match(m) => {
            expr_has_get_defined_vars(m.expression)
                || m.arms.iter().any(|arm| match arm {
                    MatchArm::Expression(ea) => {
                        ea.conditions.iter().any(|c| expr_has_get_defined_vars(c))
                            || expr_has_get_defined_vars(ea.expression)
                    }
                    MatchArm::Default(da) => expr_has_get_defined_vars(da.expression),
                })
        }
        Expression::Construct(construct) => match construct {
            Construct::Isset(isset) => isset.values.iter().any(|v| expr_has_get_defined_vars(v)),
            Construct::Empty(empty) => expr_has_get_defined_vars(empty.value),
            Construct::Eval(eval) => expr_has_get_defined_vars(eval.value),
            Construct::Include(inc) => expr_has_get_defined_vars(inc.value),
            Construct::IncludeOnce(inc) => expr_has_get_defined_vars(inc.value),
            Construct::Require(req) => expr_has_get_defined_vars(req.value),
            Construct::RequireOnce(req) => expr_has_get_defined_vars(req.value),
            Construct::Print(print) => expr_has_get_defined_vars(print.value),
            Construct::Exit(exit) => exit.arguments.as_ref().is_some_and(|args| {
                args.arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
            }),
            Construct::Die(die) => die.arguments.as_ref().is_some_and(|args| {
                args.arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
            }),
        },
        Expression::CompositeString(composite) => composite.parts().iter().any(|part| match part {
            StringPart::Expression(inner_expr) => expr_has_get_defined_vars(inner_expr),
            StringPart::BracedExpression(braced) => expr_has_get_defined_vars(braced.expression),
            StringPart::Literal(_) => false,
        }),
        Expression::Pipe(pipe) => {
            expr_has_get_defined_vars(pipe.input) || expr_has_get_defined_vars(pipe.callable)
        }
        Expression::PartialApplication(partial) => match partial {
            PartialApplication::Function(func_pa) => expr_has_get_defined_vars(func_pa.function),
            PartialApplication::Method(method_pa) => expr_has_get_defined_vars(method_pa.object),
            PartialApplication::StaticMethod(static_pa) => {
                expr_has_get_defined_vars(static_pa.class)
            }
        },
        Expression::AnonymousClass(anon) => anon.argument_list.as_ref().is_some_and(|args| {
            args.arguments
                .iter()
                .any(|a| a.value().is_some_and(expr_has_get_defined_vars))
        }),
        // Don't recurse into closures/arrow functions.
        Expression::Closure(_) | Expression::ArrowFunction(_) => false,
        _ => false,
    }
}

fn array_elem_has_get_defined_vars(elem: &ArrayElement<'_>) -> bool {
    match elem {
        ArrayElement::KeyValue(kv) => {
            expr_has_get_defined_vars(kv.key) || expr_has_get_defined_vars(kv.value)
        }
        ArrayElement::Value(v) => expr_has_get_defined_vars(v.value),
        ArrayElement::Variadic(s) => expr_has_get_defined_vars(s.value),
        ArrayElement::Missing(_) => false,
    }
}
