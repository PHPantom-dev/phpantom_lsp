//! The array literal a Laravel `config/*.php` or `lang/*/*.php` file
//! returns.
//!
//! Both kinds of file are a PHP script whose only job is to `return` a
//! nested array of string-keyed entries, either directly or through a
//! variable (`$config = [...]; return $config;`).  The config-key and
//! translation-key extractors and the config-value reader all flatten
//! that array the same way, so the walk lives here and each of them only
//! decides what to record per entry.

use mago_syntax::cst::*;

/// The expressions a file's `return` hands back, in source order: the
/// returned expression itself, or, for `return $config;`, every value the
/// top level assigns to that variable.
pub(crate) fn returned_exprs<'a>(program: &'a Program<'a>) -> Vec<&'a Expression<'a>> {
    let mut returned_var_name: Option<&[u8]> = None;
    for stmt in program.statements.iter() {
        if let Statement::Return(ret) = stmt {
            match ret.value {
                Some(Expression::Variable(Variable::Direct(dv))) => {
                    returned_var_name = Some(dv.name);
                }
                Some(val) => return vec![val],
                None => return Vec::new(),
            }
            break;
        }
    }
    let Some(var_name) = returned_var_name else {
        return Vec::new();
    };
    program
        .statements
        .iter()
        .filter_map(|stmt| {
            let Statement::Expression(expr_stmt) = stmt else {
                return None;
            };
            let Expression::Assignment(assign) = expr_stmt.expression else {
                return None;
            };
            let Expression::Variable(Variable::Direct(dv)) = assign.lhs else {
                return None;
            };
            (dv.name == var_name).then_some(assign.rhs)
        })
        .collect()
}

/// Whether `expr` is an array as far as [`for_each_entry`] is concerned:
/// an array literal, one in parentheses, or an `array_merge()` of such.
pub(crate) fn is_array_expr(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Array(_) | Expression::LegacyArray(_) => true,
        Expression::Parenthesized(p) => is_array_expr(p.expression),
        Expression::Call(Call::Function(fc)) => is_array_merge(fc),
        _ => false,
    }
}

fn is_array_merge(call: &FunctionCall<'_>) -> bool {
    matches!(call.function, Expression::Identifier(ident) if ident.value().eq_ignore_ascii_case(b"array_merge"))
}

/// What [`for_each_entry`] hands its visitor: the entry's key path from
/// the top of the array down to and including its own key, the key
/// literal's byte span in the content, and the entry's value expression.
pub(crate) type EntryVisitor<'a, 'v> = dyn FnMut(&[String], usize, usize, &'a Expression<'a>) + 'v;

/// Call `visit` for every string-keyed entry under `expr`, descending into
/// nested arrays, parenthesised arrays, and the arguments of
/// `array_merge()`.
///
/// The name a key is spelled with is taken from `content`, so a key the
/// parser could not read as a string literal is skipped along with
/// everything beneath it.
pub(crate) fn for_each_entry<'a>(
    expr: &'a Expression<'a>,
    content: &str,
    visit: &mut EntryVisitor<'a, '_>,
) {
    let mut path = Vec::new();
    walk_expr(expr, content, &mut path, visit);
}

fn walk_expr<'a>(
    expr: &'a Expression<'a>,
    content: &str,
    path: &mut Vec<String>,
    visit: &mut EntryVisitor<'a, '_>,
) {
    match expr {
        Expression::Array(arr) => walk_elements(arr.elements.iter(), content, path, visit),
        Expression::LegacyArray(arr) => walk_elements(arr.elements.iter(), content, path, visit),
        Expression::Parenthesized(p) => walk_expr(p.expression, content, path, visit),
        Expression::Call(Call::Function(fc)) if is_array_merge(fc) => {
            for arg in fc.argument_list.arguments.iter() {
                let arg_expr = match arg {
                    Argument::Positional(pos) => pos.value,
                    Argument::Named(named) => named.value,
                };
                walk_expr(arg_expr, content, path, visit);
            }
        }
        _ => {}
    }
}

fn walk_elements<'a>(
    elements: impl Iterator<Item = &'a ArrayElement<'a>>,
    content: &str,
    path: &mut Vec<String>,
    visit: &mut EntryVisitor<'a, '_>,
) {
    for element in elements {
        let ArrayElement::KeyValue(kv) = element else {
            continue;
        };
        let Some((key_text, key_start, key_end)) =
            super::helpers::extract_string_literal(kv.key, content)
        else {
            continue;
        };
        path.push(key_text.to_string());
        visit(path, key_start, key_end, kv.value);
        walk_expr(kv.value, content, path, visit);
        path.pop();
    }
}

/// The dotted key an entry path spells under `prefix`
/// (`app` + `[mail, from]` → `app.mail.from`).
pub(crate) fn dotted_key(prefix: &str, path: &[String]) -> String {
    format!("{prefix}.{}", path.join("."))
}
