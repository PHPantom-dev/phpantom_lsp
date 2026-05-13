//! Laravel helper utilities.
//!
//! This module contains:
//!
//! - **Case conversion:** `camel_to_snake`, `snake_to_camel`, `snake_to_pascal`.
//! - **Model ancestry:** `extends_eloquent_model` and the generic
//!   `walks_parent_chain` helper.
//! - **Accessor mapping:** `legacy_accessor_method_name`,
//!   `accessor_method_candidates` for go-to-definition on virtual properties.
//! - **PHP AST walker:** `walk_all_php_expressions` traverses every
//!   expression in a PHP source string, and `extract_string_literal`
//!   pulls the raw value and byte span from a string literal node.

use std::ops::ControlFlow;
use std::sync::Arc;

use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH};

use super::ELOQUENT_MODEL_FQN;

/// Walk the parent chain of `class` checking whether any ancestor
/// (including the class itself) satisfies `predicate`.
///
/// This is the shared implementation behind [`extends_eloquent_model`]
/// and `extends_eloquent_factory`.  The predicate receives a class name
/// (without a leading backslash normalisation — callers handle that
/// themselves) and returns `true` when the target base class is found.
pub(in crate::virtual_members::laravel) fn walks_parent_chain(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    predicate: fn(&str) -> bool,
) -> bool {
    if predicate(&class.name) {
        return true;
    }

    // Walk the parent chain without cloning ClassInfo.  We only need
    // each parent's `name` and `parent_class` fields, so keep a
    // cheap Arc handle instead of cloning the entire struct (which
    // copies hundreds of methods/properties/constants).
    let mut current_parent = class.parent_class;
    let mut depth = 0u32;
    while let Some(ref parent_name) = current_parent {
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            break;
        }
        if predicate(parent_name) {
            return true;
        }
        match class_loader(parent_name) {
            Some(parent) => {
                current_parent = parent.parent_class;
            }
            None => break,
        }
    }

    false
}

/// Determine whether `class_name` is the Eloquent Model base class.
///
/// Checks against the FQN with and without a leading backslash.
pub(in crate::virtual_members::laravel) fn is_eloquent_model(class_name: &str) -> bool {
    class_name == ELOQUENT_MODEL_FQN
}

/// Walk the parent chain of `class` looking for
/// `Illuminate\Database\Eloquent\Model`.
///
/// Returns `true` if the class itself is `Model` or any ancestor is.
pub fn extends_eloquent_model(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    walks_parent_chain(class, class_loader, is_eloquent_model)
}

/// Determine whether `class_name` is the Eloquent Builder base class.
pub(in crate::virtual_members::laravel) fn is_eloquent_builder(class_name: &str) -> bool {
    class_name == super::ELOQUENT_BUILDER_FQN
}

/// Walk the parent chain of `class` looking for
/// `Illuminate\Database\Eloquent\Builder`.
pub fn extends_eloquent_builder(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    walks_parent_chain(class, class_loader, is_eloquent_builder)
}

/// Convert a camelCase or PascalCase string to snake_case.
///
/// Inserts an underscore before each uppercase letter that follows a
/// lowercase letter or digit, and before an uppercase letter that is
/// followed by a lowercase letter when preceded by another uppercase
/// letter (to handle acronyms like `URL` → `u_r_l`).
///
/// `FullName` → `full_name`
/// `firstName` → `first_name`
/// `isAdmin` → `is_admin`
pub(crate) fn camel_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                // Insert underscore when: lowercase/digit → uppercase,
                // or uppercase → uppercase followed by lowercase (acronym boundary).
                if prev.is_lowercase() || prev.is_ascii_digit() {
                    result.push('_');
                } else if prev.is_uppercase() {
                    // Check next char for acronym boundary: "URL" + "Name" → "u_r_l_name"
                    if let Some(&next) = chars.get(i + 1)
                        && next.is_lowercase()
                    {
                        result.push('_');
                    }
                }
            }
            for lc in c.to_lowercase() {
                result.push(lc);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert a snake_case string to camelCase.
///
/// `full_name` → `fullName`
/// `avatar_url` → `avatarUrl`
/// `name` → `name`
pub(crate) fn snake_to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            for uc in c.to_uppercase() {
                result.push(uc);
            }
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert a snake_case string to PascalCase.
///
/// `full_name` → `FullName`
/// `avatar_url` → `AvatarUrl`
/// `name` → `Name`
pub(in crate::virtual_members::laravel) fn snake_to_pascal(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            for uc in c.to_uppercase() {
                result.push(uc);
            }
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Build the legacy accessor method name from a virtual property name.
///
/// `display_name` → `getDisplayNameAttribute`
/// `name` → `getNameAttribute`
pub(crate) fn legacy_accessor_method_name(property_name: &str) -> String {
    let pascal = snake_to_pascal(property_name);
    format!("get{pascal}Attribute")
}

/// Return candidate accessor method names for a virtual property name.
///
/// Go-to-definition uses this to map a snake_case virtual property back
/// to the method that produces it.  Returns both the legacy
/// (`getDisplayNameAttribute`) and modern (`displayName`) forms so the
/// caller can try each one.
pub(crate) fn accessor_method_candidates(property_name: &str) -> Vec<String> {
    vec![
        legacy_accessor_method_name(property_name),
        snake_to_camel(property_name),
    ]
}

/// Extract the `'as' => 'prefix.'` name prefix from a `Route::group([…], fn(){})` argument list.
///
/// The array may be in any position; all non-array arguments are skipped.
pub(crate) fn extract_as_prefix_from_args<'a>(
    args: impl Iterator<Item = &'a Expression<'a>>,
    content: &str,
) -> String {
    for arg in args {
        let elements: Vec<&ArrayElement<'_>> = match arg {
            Expression::Array(arr) => arr.elements.iter().collect(),
            Expression::LegacyArray(arr) => arr.elements.iter().collect(),
            _ => continue,
        };
        for element in elements {
            let ArrayElement::KeyValue(kv) = element else {
                continue;
            };
            let Some((key, _, _)) = extract_string_literal(kv.key, content) else {
                continue;
            };
            if key == "as"
                && let Some((val, _, _)) = extract_string_literal(kv.value, content)
            {
                return val.to_string();
            }
        }
    }
    String::new()
}

/// Collect all `->name('...')` values from the call chain that precedes `->group()`.
///
/// Handles both instance method chains (`->name('prefix.')`) and the static
/// entry point (`Route::name('prefix.')`).
pub(crate) fn chain_name_prefix<'a>(expr: &Expression<'a>, content: &str) -> String {
    match expr {
        Expression::Call(Call::Method(mc)) => {
            let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
                return chain_name_prefix(mc.object, content);
            };
            if ident.value.eq_ignore_ascii_case("name") {
                let arg_name = mc
                    .argument_list
                    .arguments
                    .iter()
                    .next()
                    .and_then(|a| extract_string_literal(a.value(), content))
                    .map(|(n, _, _)| n)
                    .unwrap_or("");
                let parent = chain_name_prefix(mc.object, content);
                format!("{parent}{arg_name}")
            } else {
                chain_name_prefix(mc.object, content)
            }
        }
        // Route::name('prefix.') — static entry point of the chain.
        Expression::Call(Call::StaticMethod(sc)) => {
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return String::new();
            };
            if ident.value.eq_ignore_ascii_case("name") {
                sc.argument_list
                    .arguments
                    .iter()
                    .next()
                    .and_then(|a| extract_string_literal(a.value(), content))
                    .map(|(n, _, _)| n.to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

// ─── Shared PHP AST walker ───────────────────────────────────────────────────

use bumpalo::Bump;
use mago_database::file::FileId;
use mago_syntax::ast::*;

/// Parse `content` as PHP and call `visitor` for every expression node
/// (pre-order, depth-first).  Used by navigation modules to find specific
/// function and static-method call patterns without duplicating the full
/// statement-walker boilerplate.
///
/// The visitor returns `ControlFlow::Continue(())` to keep walking or
/// `ControlFlow::Break(())` to stop early (e.g. after finding a match).
pub(crate) fn walk_all_php_expressions(
    content: &str,
    visitor: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) {
    let arena = Bump::new();
    let file_id = FileId::new("input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content);
    for stmt in program.statements.iter() {
        if walk_stmt_exprs(stmt, visitor).is_break() {
            return;
        }
    }
}

/// Extract the raw string value and inner byte offsets from a PHP string
/// literal expression.  Returns `(value, inner_start, inner_end)` where
/// `content[inner_start..inner_end]` is the string content without quotes.
pub(crate) fn extract_string_literal<'c>(
    expr: &Expression<'_>,
    content: &'c str,
) -> Option<(&'c str, usize, usize)> {
    let Expression::Literal(literal::Literal::String(s)) = expr else {
        return None;
    };
    let start = s.span.start.offset as usize + 1;
    let end = s.span.end.offset as usize - 1;
    if start >= end || end > content.len() {
        return None;
    }
    Some((&content[start..end], start, end))
}

/// Walk statements, returning `Break` as soon as the visitor signals early exit.
fn walk_stmt_exprs(
    stmt: &Statement<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    match stmt {
        Statement::Expression(e) => walk_expr_depth(e.expression, f)?,
        Statement::Return(r) => {
            if let Some(v) = r.value {
                walk_expr_depth(v, f)?;
            }
        }
        Statement::Echo(e) => {
            for v in e.values.iter() {
                walk_expr_depth(v, f)?;
            }
        }
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Block(b) => {
            for s in b.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::If(if_stmt) => {
            walk_expr_depth(if_stmt.condition, f)?;
            for s in if_stmt.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
            for stmts in if_stmt.body.else_if_statements() {
                for s in stmts {
                    walk_stmt_exprs(s, f)?;
                }
            }
            if let Some(else_stmts) = if_stmt.body.else_statements() {
                for s in else_stmts {
                    walk_stmt_exprs(s, f)?;
                }
            }
        }
        Statement::While(w) => {
            walk_expr_depth(w.condition, f)?;
            for s in w.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::DoWhile(dw) => {
            walk_expr_depth(dw.condition, f)?;
            walk_stmt_exprs(dw.statement, f)?;
        }
        Statement::For(fs) => {
            for init in fs.initializations.iter() {
                walk_expr_depth(init, f)?;
            }
            for cond in fs.conditions.iter() {
                walk_expr_depth(cond, f)?;
            }
            for update in fs.increments.iter() {
                walk_expr_depth(update, f)?;
            }
            for s in fs.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Foreach(fe) => {
            walk_expr_depth(fe.expression, f)?;
            for s in fe.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Try(t) => {
            for s in t.block.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
            for catch in t.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    walk_stmt_exprs(s, f)?;
                }
            }
            if let Some(ref fin) = t.finally_clause {
                for s in fin.block.statements.iter() {
                    walk_stmt_exprs(s, f)?;
                }
            }
        }
        Statement::Switch(sw) => {
            walk_expr_depth(sw.expression, f)?;
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(c) => {
                        walk_expr_depth(c.expression, f)?;
                        for s in c.statements.iter() {
                            walk_stmt_exprs(s, f)?;
                        }
                    }
                    SwitchCase::Default(c) => {
                        for s in c.statements.iter() {
                            walk_stmt_exprs(s, f)?;
                        }
                    }
                }
            }
        }
        Statement::Function(func) => {
            for s in func.body.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Class(class) => {
            for member in class.members.iter() {
                walk_class_member_exprs(member, f)?;
            }
        }
        Statement::Interface(iface) => {
            for member in iface.members.iter() {
                walk_class_member_exprs(member, f)?;
            }
        }
        Statement::Trait(t) => {
            for member in t.members.iter() {
                walk_class_member_exprs(member, f)?;
            }
        }
        Statement::Enum(e) => {
            for member in e.members.iter() {
                walk_class_member_exprs(member, f)?;
            }
        }

        Statement::Static(s) => {
            for item in s.items.iter() {
                if let Some(init) = item.value() {
                    walk_expr_depth(init, f)?;
                }
            }
        }
        Statement::Unset(u) => {
            for v in u.values.iter() {
                walk_expr_depth(v, f)?;
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

fn walk_class_member_exprs(
    member: &ClassLikeMember<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    match member {
        ClassLikeMember::Method(method) => {
            if let MethodBody::Concrete(body) = &method.body {
                for s in body.statements.iter() {
                    walk_stmt_exprs(s, f)?;
                }
            }
        }
        ClassLikeMember::Property(Property::Plain(prop)) => {
            for item in prop.items.iter() {
                if let PropertyItem::Concrete(concrete) = item {
                    walk_expr_depth(concrete.value, f)?;
                }
            }
        }
        ClassLikeMember::Constant(c) => {
            for item in c.items.iter() {
                walk_expr_depth(item.value, f)?;
            }
        }
        ClassLikeMember::EnumCase(ec) => {
            if let EnumCaseItem::Backed(backed) = &ec.item {
                walk_expr_depth(backed.value, f)?;
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

fn walk_expr_depth(
    expr: &Expression<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    f(expr)?;
    match expr {
        Expression::Call(call) => match call {
            Call::Function(fc) => {
                walk_expr_depth(fc.function, f)?;
                for arg in fc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
            Call::StaticMethod(sc) => {
                for arg in sc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
            Call::Method(mc) => {
                walk_expr_depth(mc.object, f)?;
                for arg in mc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
            Call::NullSafeMethod(mc) => {
                walk_expr_depth(mc.object, f)?;
                for arg in mc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
        },
        Expression::Binary(b) => {
            walk_expr_depth(b.lhs, f)?;
            walk_expr_depth(b.rhs, f)?;
        }
        Expression::UnaryPrefix(u) => walk_expr_depth(u.operand, f)?,
        Expression::UnaryPostfix(u) => walk_expr_depth(u.operand, f)?,
        Expression::Parenthesized(p) => walk_expr_depth(p.expression, f)?,
        Expression::Assignment(a) => {
            walk_expr_depth(a.lhs, f)?;
            walk_expr_depth(a.rhs, f)?;
        }
        Expression::Conditional(c) => {
            walk_expr_depth(c.condition, f)?;
            if let Some(then) = c.then {
                walk_expr_depth(then, f)?;
            }
            walk_expr_depth(c.r#else, f)?;
        }
        Expression::Array(arr) => {
            for el in arr.elements.iter() {
                walk_array_el_depth(el, f)?;
            }
        }
        Expression::LegacyArray(arr) => {
            for el in arr.elements.iter() {
                walk_array_el_depth(el, f)?;
            }
        }
        Expression::ArrayAccess(a) => {
            walk_expr_depth(a.array, f)?;
            walk_expr_depth(a.index, f)?;
        }
        Expression::Closure(c) => {
            for s in c.body.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Expression::ArrowFunction(af) => walk_expr_depth(af.expression, f)?,
        Expression::Match(m) => {
            walk_expr_depth(m.expression, f)?;
            for arm in m.arms.iter() {
                match arm {
                    MatchArm::Expression(ea) => {
                        for cond in ea.conditions.iter() {
                            walk_expr_depth(cond, f)?;
                        }
                        walk_expr_depth(ea.expression, f)?;
                    }
                    MatchArm::Default(da) => walk_expr_depth(da.expression, f)?,
                }
            }
        }
        Expression::Throw(t) => walk_expr_depth(t.exception, f)?,
        Expression::Yield(y) => match y {
            Yield::Value(yv) => {
                if let Some(val) = yv.value {
                    walk_expr_depth(val, f)?;
                }
            }
            Yield::Pair(yp) => {
                walk_expr_depth(yp.key, f)?;
                walk_expr_depth(yp.value, f)?;
            }
            Yield::From(yf) => walk_expr_depth(yf.iterator, f)?,
        },
        Expression::Clone(c) => walk_expr_depth(c.object, f)?,
        Expression::Instantiation(inst) => {
            if let Some(args) = &inst.argument_list {
                for a in args.arguments.iter() {
                    walk_expr_depth(a.value(), f)?;
                }
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

fn walk_array_el_depth(
    el: &ArrayElement<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    match el {
        ArrayElement::KeyValue(kv) => {
            walk_expr_depth(kv.key, f)?;
            walk_expr_depth(kv.value, f)?;
        }
        ArrayElement::Value(v) => walk_expr_depth(v.value, f)?,
        ArrayElement::Variadic(v) => walk_expr_depth(v.value, f)?,
        ArrayElement::Missing(_) => {}
    }
    ControlFlow::Continue(())
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod tests;
