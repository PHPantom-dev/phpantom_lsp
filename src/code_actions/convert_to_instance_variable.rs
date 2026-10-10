//! **Convert to Instance Variable** code action (`refactor.extract`).
//!
//! When the cursor is on a local variable assignment like
//! `$result = expr;` inside a method body, this action:
//!
//! 1. Creates a new `private` property on the enclosing class
//! 2. Replaces `$result` with `$this->result` (or `self::$result` for static methods)
//! 3. Replaces all other occurrences of `$result` within the same method scope,
//!    including inside arrow functions and closures that capture it (a
//!    closure's `use ($result)` capture is dropped, since the property
//!    reaches the closure through `$this`)
//!
//! ### Checks
//!
//! - If a property with the same name already exists (including promoted
//!   constructor parameters), the action is **not** offered.
//! - The `$this` variable is never offered for conversion.
//! - Only works inside a method body of a class-like declaration.
//! - The action is **not** offered when the variable sits somewhere a
//!   property access cannot (a parameter, `catch`, `static`, or `global`
//!   declaration), is reached by name (`compact()`, `extract()`, `$$name`),
//!   or is captured by a `static` closure that has no `$this`.

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::MethodBody;
use mago_syntax::cst::class_like::property::Property;
use mago_syntax::cst::sequence::Sequence;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::find_class_at_offset;
use crate::code_actions::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::code_actions::{CodeActionData, detect_indent_from_members, make_code_action_data};
use crate::parser::with_parsed_program;
use crate::scope_collector::{
    AccessRole, ClosureUseList, Frame, FrameKind, ScopeMap, collect_function_scope,
};
use crate::text_position::{offset_to_position, position_to_byte_offset};
use crate::types::{ClassInfo, Visibility};
use crate::virtual_members::resolve_class_fully_cached;

// ─── AST helpers ────────────────────────────────────────────────────────────

/// Everything the conversion of the assignment at the cursor needs.
struct Conversion {
    /// The variable name including `$` prefix (e.g. `"$result"`).
    var_name: String,
    /// Whether the enclosing method is static.
    is_static: bool,
    /// Byte offset where the property declaration is inserted.
    insert_offset: usize,
    /// The property declaration, including indentation and newlines.
    property_text: String,
    /// The changes inside the method body.
    rewrites: Vec<Rewrite>,
}

/// One change inside the method body.
enum Rewrite {
    /// Replace the `$var` at this offset with the property access.
    Replace(u32),
    /// Delete the bytes in `[start, end)`.
    Delete(u32, u32),
}

/// Check whether a property with the given bare name already exists on the class,
/// including promoted constructor parameters.
fn property_exists<'a>(all_members: &Sequence<'a, ClassLikeMember<'a>>, bare_name: &str) -> bool {
    for member in all_members.iter() {
        match member {
            ClassLikeMember::Property(property) => {
                if let Property::Plain(plain) = property {
                    for item in plain.items.iter() {
                        let var = item.variable();
                        let name = bytes_to_str(var.name);
                        let bare = name.strip_prefix('$').unwrap_or(name);
                        if bare == bare_name {
                            return true;
                        }
                    }
                }
                if let Property::Hooked(hooked) = property {
                    let var = hooked.item.variable();
                    let name = bytes_to_str(var.name);
                    let bare = name.strip_prefix('$').unwrap_or(name);
                    if bare == bare_name {
                        return true;
                    }
                }
            }
            ClassLikeMember::Method(method) if method.name.value == b"__construct" => {
                for param in method.parameter_list.parameters.iter() {
                    if param.is_promoted_property() {
                        let name = bytes_to_str(param.variable.name).to_string();
                        let bare = name.strip_prefix('$').unwrap_or(&name);
                        if bare == bare_name {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Find property insertion point offsets and the indent string.
///
/// Returns `(insert_byte_offset, property_text)` where `insert_byte_offset`
/// is the byte position in `content` where the new property line should be
/// inserted.
fn find_property_insertion_point<'a>(
    all_members: &Sequence<'a, ClassLikeMember<'a>>,
    content: &str,
) -> usize {
    let mut last_property_end: Option<u32> = None;
    let mut first_method_start: Option<u32> = None;

    for member in all_members.iter() {
        match member {
            ClassLikeMember::Property(_) => {
                last_property_end = Some(member.span().end.offset);
            }
            ClassLikeMember::Method(_) if first_method_start.is_none() => {
                first_method_start = Some(member.span().start.offset);
            }
            _ => {}
        }
    }

    if let Some(end) = last_property_end {
        // Insert after the last property — find the end of the line.
        let offset = end as usize;
        let next_newline = content[offset..].find('\n').map(|i| offset + i + 1);
        next_newline.unwrap_or(offset)
    } else if let Some(start) = first_method_start {
        // No properties exist — insert before the first method.
        // We want to insert at the beginning of the line containing
        // the first method.
        let offset = start as usize;
        content[..offset]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0)
    } else {
        // No members at all — shouldn't happen if we're in a method,
        // but fall back to end of content.
        content.len()
    }
}

/// Plan the conversion of the assignment at the cursor.
///
/// Returns `None` if the cursor is not on a suitable assignment in a
/// method body, or if some occurrence of the variable cannot become a
/// property access.
fn plan_conversion(content: &str, cursor_offset: u32) -> Option<Conversion> {
    with_parsed_program(
        content,
        "convert_to_instance_variable",
        |program, content| {
            let ctx = find_cursor_context(&program.statements, cursor_offset);

            let (method, all_members) = match ctx {
                CursorContext::InClassLike {
                    member: MemberContext::Method(method, true),
                    all_members,
                    ..
                } => (method, all_members),
                _ => return None,
            };

            // The method must have a concrete body.
            let block = match &method.body {
                MethodBody::Concrete(block) => block,
                _ => return None,
            };

            let assignment_info =
                find_assignment_in_block(block.statements.as_slice(), cursor_offset)?;
            let var_name = assignment_info.0;
            if var_name == "$this" {
                return None;
            }

            let bare_name = var_name.strip_prefix('$').unwrap_or(&var_name);

            if property_exists(all_members, bare_name) {
                return None;
            }

            let is_static = method.modifiers.iter().any(|m| m.is_static());

            let scope_map = collect_function_scope(
                &method.parameter_list,
                block.statements.as_slice(),
                block.left_brace.start.offset,
                block.right_brace.end.offset,
            );
            let frame = scope_map.enclosing_frame(block.left_brace.start.offset)?;
            // The body's occurrences would become the property while the
            // parameter still supplies the value read before the assignment.
            if frame.parameters.contains(&var_name) {
                return None;
            }
            let mut rewrites = Vec::new();
            plan_rewrites(
                &scope_map,
                frame,
                &var_name,
                is_static,
                false,
                content,
                &mut rewrites,
            )?;

            let indent = detect_indent_from_members(all_members, content);
            let insert_offset = find_property_insertion_point(all_members, content);
            let has_properties = all_members
                .iter()
                .any(|m| matches!(m, ClassLikeMember::Property(_)));
            let static_kw = if is_static { "static " } else { "" };
            let trailer = if has_properties { "\n" } else { "\n\n" };
            let property_text = format!("{indent}private {static_kw}${bare_name};{trailer}");

            Some(Conversion {
                var_name,
                is_static,
                insert_offset,
                property_text,
                rewrites,
            })
        },
    )
}

/// Collect the rewrites that turn every occurrence of `name` in `frame`
/// into a property access, descending into the arrow functions and
/// closures that capture it.
///
/// Returns `None` when an occurrence cannot become a property access.
/// `in_static_closure` is set inside a `static` closure or arrow function,
/// where `$this` does not exist.
fn plan_rewrites(
    map: &ScopeMap,
    frame: &Frame,
    name: &str,
    is_static: bool,
    in_static_closure: bool,
    content: &str,
    out: &mut Vec<Rewrite>,
) -> Option<()> {
    let reached_by_name = map
        .dynamic_scope_accesses
        .iter()
        .any(|d| map.frame_owns(frame, d.offset) && d.name.as_deref().is_none_or(|n| n == name));
    if reached_by_name {
        return None;
    }

    for access in map.accesses_in_frame(name, frame) {
        match access.role {
            AccessRole::Expression => {
                if in_static_closure && !is_static {
                    return None;
                }
                out.push(Rewrite::Replace(access.offset));
            }
            // A nested closure's or arrow function's own parameter, or
            // either side of a `use` capture, which the closure handles.
            AccessRole::Parameter | AccessRole::ClosureUse | AccessRole::ClosureCapture => {}
            AccessRole::CatchBinding
            | AccessRole::StaticDeclaration
            | AccessRole::GlobalDeclaration => return None,
        }
    }

    for child in map.child_frames(frame) {
        let captures = match child.kind {
            FrameKind::Closure => {
                let removal = map
                    .closure_use_lists
                    .iter()
                    .find(|list| list.frame_start == child.start)
                    .and_then(|list| {
                        let index = list.items.iter().position(|item| item.name == name)?;
                        Some(use_item_removal(list, index, content))
                    });
                if let Some((start, end)) = removal {
                    out.push(Rewrite::Delete(start, end));
                }
                removal.is_some()
            }
            FrameKind::ArrowFunction => !child.parameters.iter().any(|p| p == name),
            _ => false,
        };
        if captures {
            plan_rewrites(
                map,
                child,
                name,
                is_static,
                in_static_closure || child.is_static,
                content,
                out,
            )?;
        }
    }

    Some(())
}

/// The byte range to delete to drop item `index` from a closure's `use`
/// list, along with its separating comma.  Dropping the only item removes
/// the whole clause.
fn use_item_removal(list: &ClosureUseList, index: usize, content: &str) -> (u32, u32) {
    let items = &list.items;
    if items.len() == 1 {
        let before = &content[..list.clause_start as usize];
        (before.trim_end().len() as u32, list.clause_end)
    } else if index + 1 < items.len() {
        (items[index].start, items[index + 1].start)
    } else {
        (items[index - 1].end, items[index].end)
    }
}

/// Walk statements to find a simple `$var = expr;` assignment at cursor.
/// Returns the variable name (with `$` prefix) if found.
fn find_assignment_in_block(statements: &[Statement<'_>], cursor: u32) -> Option<(String,)> {
    for stmt in statements {
        if let Some(result) = find_assignment_in_stmt(stmt, cursor) {
            return Some(result);
        }
    }
    None
}

fn find_assignment_in_stmt(stmt: &Statement<'_>, cursor: u32) -> Option<(String,)> {
    let span = stmt.span();
    if cursor < span.start.offset || cursor > span.end.offset {
        return None;
    }

    match stmt {
        Statement::Expression(expr_stmt) => {
            if let Expression::Assignment(assignment) = expr_stmt.expression {
                if !assignment.operator.is_assign() {
                    return None;
                }
                let var = match assignment.lhs {
                    Expression::Variable(Variable::Direct(dv)) => dv,
                    _ => return None,
                };
                let var_name = bytes_to_str(var.name).to_string();
                if var_name == "$this" {
                    return None;
                }
                return Some((var_name,));
            }
            None
        }
        Statement::Block(block) => find_assignment_in_block(block.statements.as_slice(), cursor),
        Statement::If(if_stmt) => {
            if let Some(r) = find_assignment_in_if_body(if_stmt, cursor) {
                return Some(r);
            }
            None
        }
        Statement::While(w) => match &w.body {
            WhileBody::Statement(s) => find_assignment_in_stmt(s, cursor),
            WhileBody::ColonDelimited(body) => {
                find_assignment_in_block(body.statements.as_slice(), cursor)
            }
        },
        Statement::DoWhile(dw) => find_assignment_in_stmt(dw.statement, cursor),
        Statement::For(f) => match &f.body {
            ForBody::Statement(s) => find_assignment_in_stmt(s, cursor),
            ForBody::ColonDelimited(body) => {
                find_assignment_in_block(body.statements.as_slice(), cursor)
            }
        },
        Statement::Foreach(fe) => match &fe.body {
            ForeachBody::Statement(s) => find_assignment_in_stmt(s, cursor),
            ForeachBody::ColonDelimited(body) => {
                find_assignment_in_block(body.statements.as_slice(), cursor)
            }
        },
        Statement::Switch(sw) => {
            for case in sw.body.cases().iter() {
                let stmts = match case {
                    SwitchCase::Expression(c) => &c.statements,
                    SwitchCase::Default(c) => &c.statements,
                };
                if let Some(r) = find_assignment_in_block(stmts.as_slice(), cursor) {
                    return Some(r);
                }
            }
            None
        }
        Statement::Try(t) => {
            if let Some(r) = find_assignment_in_block(t.block.statements.as_slice(), cursor) {
                return Some(r);
            }
            for catch in t.catch_clauses.iter() {
                if let Some(r) = find_assignment_in_block(catch.block.statements.as_slice(), cursor)
                {
                    return Some(r);
                }
            }
            if let Some(ref finally) = t.finally_clause
                && let Some(r) =
                    find_assignment_in_block(finally.block.statements.as_slice(), cursor)
            {
                return Some(r);
            }
            None
        }
        _ => None,
    }
}

fn find_assignment_in_if_body(if_stmt: &If<'_>, cursor: u32) -> Option<(String,)> {
    match &if_stmt.body {
        IfBody::Statement(body) => {
            if let Some(r) = find_assignment_in_stmt(body.statement, cursor) {
                return Some(r);
            }
            for else_if in body.else_if_clauses.iter() {
                if let Some(r) = find_assignment_in_stmt(else_if.statement, cursor) {
                    return Some(r);
                }
            }
            if let Some(ref else_clause) = body.else_clause
                && let Some(r) = find_assignment_in_stmt(else_clause.statement, cursor)
            {
                return Some(r);
            }
        }
        IfBody::ColonDelimited(body) => {
            for s in body.statements.iter() {
                if let Some(r) = find_assignment_in_stmt(s, cursor) {
                    return Some(r);
                }
            }
            for else_if in body.else_if_clauses.iter() {
                for s in else_if.statements.iter() {
                    if let Some(r) = find_assignment_in_stmt(s, cursor) {
                        return Some(r);
                    }
                }
            }
            if let Some(ref else_clause) = body.else_clause {
                for s in else_clause.statements.iter() {
                    if let Some(r) = find_assignment_in_stmt(s, cursor) {
                        return Some(r);
                    }
                }
            }
        }
    }
    None
}

// ─── Backend impl ───────────────────────────────────────────────────────────

impl Backend {
    /// Whether the enclosing class already inherits a non-private property
    /// with this name from a parent or trait. Declaring it again would
    /// shadow it, and a narrower visibility is a fatal error.
    fn inherits_property(&self, uri: &str, cursor_offset: u32, var_name: &str) -> bool {
        let bare_name = var_name.strip_prefix('$').unwrap_or(var_name);
        let local_classes: Vec<Arc<ClassInfo>> = self
            .symbols
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let Some(enclosing) = find_class_at_offset(&local_classes, cursor_offset) else {
            return false;
        };
        let (use_map, namespace) = self.use_map_and_namespace_at(uri, cursor_offset);
        let class_loader = self.class_loader_with(&local_classes, &use_map, &namespace);
        let resolved =
            resolve_class_fully_cached(enclosing, &class_loader, &self.resolved_class_cache);
        resolved
            .get_property(bare_name)
            .is_some_and(|p| p.visibility != Visibility::Private)
    }

    /// Collect "Convert to Instance Variable" code actions (Phase 1).
    ///
    /// Verifies the cursor is on a local variable assignment inside a
    /// method body, that no property with the same name already exists,
    /// and that every occurrence of the variable can become one.
    pub(crate) fn collect_convert_to_instance_variable_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let cursor_offset = position_to_byte_offset(content, params.range.start) as u32;

        let Some(info) = plan_conversion(content, cursor_offset) else {
            return;
        };

        if self.inherits_property(uri, cursor_offset, &info.var_name) {
            return;
        }

        let title = if info.is_static {
            format!("Convert {} to static property", info.var_name)
        } else {
            format!("Convert {} to instance variable", info.var_name)
        };

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::new("refactor.extract")),
            diagnostics: None,
            edit: None,
            command: None,
            is_preferred: Some(false),
            disabled: None,
            data: Some(make_code_action_data(
                "refactor.extractInstanceVariable",
                uri,
                &params.range,
                serde_json::json!({}),
            )),
        }));
    }

    /// Resolve a deferred "Convert to Instance Variable" code action (Phase 2).
    ///
    /// Recomputes the full workspace edit: inserts a new property declaration
    /// and replaces all occurrences of the local variable with the instance
    /// (or static) property access.
    pub(crate) fn resolve_convert_to_instance_variable(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let cursor_offset = position_to_byte_offset(content, data.range.start) as u32;

        let conversion = plan_conversion(content, cursor_offset)?;
        if self.inherits_property(&data.uri, cursor_offset, &conversion.var_name) {
            return None;
        }

        let doc_uri: Url = data.uri.parse().ok()?;
        let mut edits: Vec<TextEdit> = Vec::new();

        let insert_pos = offset_to_position(content, conversion.insert_offset);
        edits.push(TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: conversion.property_text,
        });

        let var_name = &conversion.var_name;
        let bare_name = var_name.strip_prefix('$').unwrap_or(var_name);
        let replacement = if conversion.is_static {
            format!("self::${bare_name}")
        } else {
            format!("$this->{bare_name}")
        };
        for rewrite in &conversion.rewrites {
            match *rewrite {
                Rewrite::Replace(offset) => {
                    if let Some(edit) = crate::code_actions::occurrence_replacement_edit(
                        content,
                        offset as usize,
                        var_name,
                        &replacement,
                    ) {
                        edits.push(edit);
                    }
                }
                Rewrite::Delete(start, end) => edits.push(TextEdit {
                    range: Range {
                        start: offset_to_position(content, start as usize),
                        end: offset_to_position(content, end as usize),
                    },
                    new_text: String::new(),
                }),
            }
        }

        crate::code_actions::sort_edits_by_position(&mut edits);

        Some(crate::code_actions::single_file_edit(doc_uri, edits))
    }
}
