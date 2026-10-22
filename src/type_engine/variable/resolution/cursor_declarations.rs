//! Answers for a cursor that sits on a declaration rather than a use: a
//! parameter, a `catch` binding, or the right-hand side of an assignment
//! to the same variable.

use super::*;

use std::sync::Arc;

use crate::atom::bytes_to_str;
use crate::class_lookup::ResolvedClassLoader;
use crate::docblock;
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;
use crate::types::ClassInfo;

/// Check if the cursor is on a parameter definition and return it.
///
/// Walks namespaces, classes, and functions to find a parameter list
/// that contains `cursor_offset` with a parameter matching `var_name`.
pub(super) fn find_param_at_cursor(
    stmts: &[&Statement<'_>],
    var_name: &str,
    cursor_offset: u32,
    content: &str,
) -> Option<ParamSite> {
    use mago_span::HasSpan;

    for stmt in stmts {
        match stmt {
            Statement::Namespace(ns) => {
                let inner: Vec<&Statement> = ns.statements().iter().collect();
                if let Some(t) = find_param_at_cursor(&inner, var_name, cursor_offset, content) {
                    return Some(t);
                }
            }
            _ if let Some(body) = crate::parser::class_like_body(stmt) => {
                for member in body.members {
                    if let ClassLikeMember::Method(method) = member
                        && let Some(t) = check_param_list(
                            &method.parameter_list,
                            var_name,
                            cursor_offset,
                            content,
                            method.span().start.offset as usize,
                        )
                    {
                        return Some(t);
                    }
                }
            }
            Statement::Function(func) => {
                if let Some(t) = check_param_list(
                    &func.parameter_list,
                    var_name,
                    cursor_offset,
                    content,
                    func.span().start.offset as usize,
                ) {
                    return Some(t);
                }
            }
            _ => {}
        }
    }
    None
}

/// Check if a parameter list contains the cursor and has a matching
/// parameter with a type hint.
fn check_param_list(
    param_list: &FunctionLikeParameterList<'_>,
    var_name: &str,
    cursor_offset: u32,
    content: &str,
    method_start_offset: usize,
) -> Option<ParamSite> {
    use mago_span::HasSpan;

    let span = param_list.span();
    if cursor_offset < span.start.offset || cursor_offset > span.end.offset {
        return None;
    }

    let param = param_list
        .parameters
        .iter()
        .find(|param| bytes_to_str(param.variable.name) == var_name)?;
    let native = param.hint.as_ref().map(|h| extract_hint_type(h));
    let docblock = declared_param_docblock_type(content, method_start_offset, var_name);
    (native.is_some() || docblock.is_some()).then_some(ParamSite {
        native,
        docblock,
        decl_start: method_start_offset,
    })
}

/// A parameter in its declaration: the native hint and the `@param` type,
/// both as written, and where the declaration starts.
pub(super) struct ParamSite {
    native: Option<PhpType>,
    docblock: Option<PhpType>,
    decl_start: usize,
}

impl ParamSite {
    /// The type the parameter holds, the docblock's where it may stand in
    /// for the native hint (see [`param_docblock_narrows_hint`]).
    pub(super) fn settled_type(
        self,
        current_class: &ClassInfo,
        content: &str,
        class_loader: &ResolvedClassLoader<'_>,
        source_class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<PhpType> {
        let effective =
            docblock::resolve_effective_type_typed(self.native.as_ref(), self.docblock.as_ref());
        match (effective, self.native) {
            (Some(settled), Some(written))
                if settled != written
                    && !param_docblock_narrows_hint(
                        &crate::names::resolve_php_type_names(
                            &settled,
                            source_class_loader,
                            class_loader,
                        ),
                        &written,
                        self.decl_start,
                        current_class,
                        content,
                        class_loader,
                        source_class_loader,
                    ) =>
            {
                Some(written)
            }
            (effective, _) => effective,
        }
    }
}

/// Check if the cursor is on a catch variable binding and return its type.
pub(super) fn find_catch_var_type_at_cursor(
    stmts: &[&Statement<'_>],
    var_name: &str,
    cursor_offset: u32,
) -> Option<PhpType> {
    use mago_span::HasSpan;

    for stmt in stmts {
        let stmt_span = stmt.span();
        if cursor_offset < stmt_span.start.offset || cursor_offset > stmt_span.end.offset {
            continue;
        }
        match stmt {
            Statement::Try(try_stmt) => {
                for catch in try_stmt.catch_clauses.iter() {
                    if let Some(ref var) = catch.variable
                        && bytes_to_str(var.name) == var_name
                    {
                        let var_start = var.span.start.offset;
                        let var_end = var.span.end.offset;
                        if cursor_offset >= var_start && cursor_offset <= var_end {
                            return Some(extract_hint_type(&catch.hint));
                        }
                    }
                }
                // Recurse into try/catch/finally bodies.
                let try_stmts: Vec<&Statement> = try_stmt.block.statements.iter().collect();
                if let Some(t) = find_catch_var_type_at_cursor(&try_stmts, var_name, cursor_offset)
                {
                    return Some(t);
                }
                for catch in try_stmt.catch_clauses.iter() {
                    let catch_stmts: Vec<&Statement> = catch.block.statements.iter().collect();
                    if let Some(t) =
                        find_catch_var_type_at_cursor(&catch_stmts, var_name, cursor_offset)
                    {
                        return Some(t);
                    }
                }
                if let Some(ref finally) = try_stmt.finally_clause {
                    let fin_stmts: Vec<&Statement> = finally.block.statements.iter().collect();
                    if let Some(t) =
                        find_catch_var_type_at_cursor(&fin_stmts, var_name, cursor_offset)
                    {
                        return Some(t);
                    }
                }
            }
            Statement::Namespace(ns) => {
                let inner: Vec<&Statement> = ns.statements().iter().collect();
                if let Some(t) = find_catch_var_type_at_cursor(&inner, var_name, cursor_offset) {
                    return Some(t);
                }
            }
            Statement::Class(class) => {
                for member in class.members.iter() {
                    if let ClassLikeMember::Method(method) = member
                        && let MethodBody::Concrete(body) = &method.body
                    {
                        let body_stmts: Vec<&Statement> = body.statements.iter().collect();
                        if let Some(t) =
                            find_catch_var_type_at_cursor(&body_stmts, var_name, cursor_offset)
                        {
                            return Some(t);
                        }
                    }
                }
            }
            Statement::Function(func) => {
                let body_stmts: Vec<&Statement> = func.body.statements.iter().collect();
                if let Some(t) = find_catch_var_type_at_cursor(&body_stmts, var_name, cursor_offset)
                {
                    return Some(t);
                }
            }
            _ => {}
        }
    }
    None
}
