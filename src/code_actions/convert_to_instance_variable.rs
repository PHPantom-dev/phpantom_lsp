//! **Convert to Instance Variable** code action (`refactor.extract`).
//!
//! When the cursor is on a local variable assignment like
//! `$result = expr;` inside a method body, this action:
//!
//! 1. Creates a new `private` property on the enclosing class
//! 2. Replaces `$result` with `$this->result` (or `self::$result` for static methods)
//! 3. Replaces all other occurrences of `$result` within the same method scope
//!
//! ### Checks
//!
//! - If a property with the same name already exists (including promoted
//!   constructor parameters), the action is **not** offered.
//! - The `$this` variable is never offered for conversion.
//! - Only works inside a method body of a class-like declaration.

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::MethodBody;
use mago_syntax::cst::class_like::property::Property;
use mago_syntax::cst::sequence::Sequence;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::code_actions::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::code_actions::{CodeActionData, detect_indent_from_members, make_code_action_data};
use crate::parser::with_parsed_program;
use crate::scope_collector::collect_function_scope;
use crate::text_position::{offset_to_position, position_to_byte_offset};

// ─── AST helpers ────────────────────────────────────────────────────────────

/// Information gathered in Phase 1 about the assignment at the cursor.
struct ConvertInfo {
    /// The variable name including `$` prefix (e.g. `"$result"`).
    var_name: String,
    /// Whether the enclosing method is static.
    is_static: bool,
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

/// Try to collect convert-to-instance-variable info from the parsed AST.
///
/// Returns `None` if the cursor is not on a suitable assignment in a method body.
fn collect_info(content: &str, cursor_offset: u32) -> Option<ConvertInfo> {
    with_parsed_program(
        content,
        "convert_to_instance_variable",
        |program, _content| {
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

            Some(ConvertInfo {
                var_name,
                is_static,
            })
        },
    )
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
    /// Collect "Convert to Instance Variable" code actions (Phase 1).
    ///
    /// This is a lightweight check that verifies the cursor is on a local
    /// variable assignment inside a method body, and that no property with
    /// the same name already exists.
    pub(crate) fn collect_convert_to_instance_variable_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let cursor_offset = position_to_byte_offset(content, params.range.start) as u32;

        let info = match collect_info(content, cursor_offset) {
            Some(i) => i,
            None => return,
        };

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

        let result = with_parsed_program(
            content,
            "convert_to_instance_variable",
            |program, _content| {
                let ctx = find_cursor_context(&program.statements, cursor_offset);

                let (method, all_members) = match ctx {
                    CursorContext::InClassLike {
                        member: MemberContext::Method(method, true),
                        all_members,
                        ..
                    } => (method, all_members),
                    _ => return None,
                };

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

                let bare_name = var_name.strip_prefix('$').unwrap_or(&var_name).to_string();

                if property_exists(all_members, &bare_name) {
                    return None;
                }

                let is_static = method.modifiers.iter().any(|m| m.is_static());
                let indent = detect_indent_from_members(all_members, content);
                let insert_offset = find_property_insertion_point(all_members, content);

                // Build the property declaration text.
                let has_properties = all_members
                    .iter()
                    .any(|m| matches!(m, ClassLikeMember::Property(_)));

                let property_text = if is_static {
                    if has_properties {
                        format!("{}private static ${};\n", indent, bare_name)
                    } else {
                        format!("{}private static ${};\n\n", indent, bare_name)
                    }
                } else if has_properties {
                    format!("{}private ${};\n", indent, bare_name)
                } else {
                    format!("{}private ${};\n\n", indent, bare_name)
                };

                // Collect all occurrences of the variable in the method scope.
                let body_start = block.left_brace.start.offset;
                let body_end = block.right_brace.end.offset;
                let scope_map = collect_function_scope(
                    &method.parameter_list,
                    block.statements.as_slice(),
                    body_start,
                    body_end,
                );

                // Find an occurrence offset for scope lookup — use the first
                // occurrence in the method body.
                let occurrences = scope_map.all_occurrences(&var_name, body_start);

                // Build replacement text.
                let replacement = if is_static {
                    format!("self::${}", bare_name)
                } else {
                    format!("$this->{}", bare_name)
                };

                Some((
                    insert_offset,
                    property_text,
                    occurrences,
                    var_name,
                    replacement,
                ))
            },
        )?;

        let (insert_offset, property_text, occurrences, var_name, replacement) = result;

        let doc_uri: Url = data.uri.parse().ok()?;
        let mut edits: Vec<TextEdit> = Vec::new();

        // 1. Property insertion edit.
        let insert_pos = offset_to_position(content, insert_offset);
        edits.push(TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: property_text,
        });

        // 2. Replace each occurrence of $varname with the instance/static access.
        for (offset, _kind) in &occurrences {
            if let Some(edit) = crate::code_actions::occurrence_replacement_edit(
                content,
                *offset as usize,
                &var_name,
                &replacement,
            ) {
                edits.push(edit);
            }
        }

        crate::code_actions::sort_edits_by_position(&mut edits);

        Some(crate::code_actions::single_file_edit(doc_uri, edits))
    }
}
