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

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::ast::class_like::member::ClassLikeMember;
use mago_syntax::ast::class_like::method::MethodBody;
use mago_syntax::ast::class_like::property::Property;
use mago_syntax::ast::sequence::Sequence;
use mago_syntax::ast::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::code_actions::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::code_actions::{CodeActionData, detect_indent_from_members, make_code_action_data};
use crate::parser::with_parsed_program;
use crate::scope_collector::collect_function_scope;
use crate::util::{offset_to_position, position_to_byte_offset};

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

            // Find the assignment at the cursor.
            let assignment_info =
                find_assignment_in_block(block.statements.as_slice(), cursor_offset)?;
            let var_name = assignment_info.0;
            // Skip $this.
            if var_name == "$this" {
                return None;
            }

            let bare_name = var_name.strip_prefix('$').unwrap_or(&var_name);

            // Check if property already exists.
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
            // Check the if body.
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

                // Find the assignment at cursor.
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
            let start = *offset as usize;
            let end = start + var_name.len();

            if end > content.len() {
                continue;
            }

            // Verify the text at this offset matches.
            if content[start..end] != *var_name {
                continue;
            }

            let start_pos = offset_to_position(content, start);
            let end_pos = offset_to_position(content, end);
            edits.push(TextEdit {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: replacement.clone(),
            });
        }

        // Sort edits by position.
        edits.sort_by(|a, b| {
            a.range
                .start
                .line
                .cmp(&b.range.start.line)
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        let mut changes = HashMap::new();
        changes.insert(doc_uri, edits);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: given PHP source with a cursor marker `/*|*/`, run the
    /// convert-to-instance-variable action and return the resulting edits.
    fn run_convert(php: &str) -> Option<Vec<TextEdit>> {
        let marker = "/*|*/";
        let marker_pos = php.find(marker)?;
        let content = php.replace(marker, "");

        let uri = "file:///test.php";
        let cursor_offset = marker_pos;
        let position = offset_to_position(&content, cursor_offset);
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::parse(uri).unwrap(),
            },
            range: Range {
                start: position,
                end: position,
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
        };

        let backend = Backend::new_test();
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.clone()));

        let mut actions = Vec::new();
        backend.collect_convert_to_instance_variable_actions(uri, &content, &params, &mut actions);

        if actions.is_empty() {
            return None;
        }

        let action = match &actions[0] {
            CodeActionOrCommand::CodeAction(a) => a.clone(),
            _ => return None,
        };

        assert!(action.edit.is_none(), "Phase 1 should not compute edits");
        assert!(action.data.is_some(), "Phase 1 should attach resolve data");

        // Phase 2: resolve.
        let (resolved, _) = backend.resolve_code_action(action);
        let edit = resolved.edit.as_ref()?;
        let changes = edit.changes.as_ref()?;
        let parsed_uri = Url::parse(uri).unwrap();
        let edits = changes.get(&parsed_uri)?;
        Some(edits.clone())
    }

    /// Apply TextEdits to content (edits applied from bottom to top).
    fn apply_edits(content: &str, edits: &[TextEdit]) -> String {
        let mut result = content.to_string();
        let mut sorted: Vec<&TextEdit> = edits.iter().collect();
        sorted.sort_by(|a, b| {
            b.range
                .start
                .line
                .cmp(&a.range.start.line)
                .then(b.range.start.character.cmp(&a.range.start.character))
        });
        for edit in sorted {
            let start = position_to_byte_offset(&result, edit.range.start);
            let end = position_to_byte_offset(&result, edit.range.end);
            result.replace_range(start..end, &edit.new_text);
        }
        result
    }

    // ── Basic conversion ────────────────────────────────────────────────

    #[test]
    fn converts_simple_variable() {
        let php =
            "<?php\nclass Foo {\n    public function bar() {\n        /*|*/$result = 42;\n    }\n}";
        let content = php.replace("/*|*/", "");
        let edits = run_convert(php).expect("action should be offered");
        let result = apply_edits(&content, &edits);
        assert!(
            result.contains("private $result;"),
            "should declare property: {}",
            result
        );
        assert!(
            result.contains("$this->result = 42;"),
            "should replace assignment: {}",
            result
        );
    }

    #[test]
    fn replaces_all_occurrences_in_method() {
        let php = "<?php\nclass Foo {\n    public function bar() {\n        /*|*/$x = 1;\n        echo $x;\n        return $x;\n    }\n}";
        let content = php.replace("/*|*/", "");
        let edits = run_convert(php).expect("action should be offered");
        let result = apply_edits(&content, &edits);
        assert!(
            result.contains("private $x;"),
            "should declare property: {}",
            result
        );
        // All three occurrences should be replaced.
        let count = result.matches("$this->x").count();
        assert_eq!(
            count, 3,
            "should replace all 3 occurrences, got: {}",
            result
        );
        // No bare $x should remain in the method body.
        // The property declaration `private $x;` still contains `$x`, so
        // check that the method body lines don't have bare `$x`.
        assert!(
            !result.contains("echo $x;") && !result.contains("return $x;"),
            "no bare $x should remain in method body: {}",
            result
        );
    }

    #[test]
    fn rejects_when_property_exists() {
        let php = "<?php\nclass Foo {\n    private $result;\n    public function bar() {\n        /*|*/$result = 42;\n    }\n}";
        assert!(
            run_convert(php).is_none(),
            "should not offer action when property exists"
        );
    }

    #[test]
    fn rejects_when_promoted_property_exists() {
        let php = "<?php\nclass Foo {\n    public function __construct(private $result) {}\n    public function bar() {\n        /*|*/$result = 42;\n    }\n}";
        assert!(
            run_convert(php).is_none(),
            "should not offer action when promoted property exists"
        );
    }

    #[test]
    fn converts_in_static_method() {
        let php = "<?php\nclass Foo {\n    public static function bar() {\n        /*|*/$result = 42;\n    }\n}";
        let content = php.replace("/*|*/", "");
        let edits = run_convert(php).expect("action should be offered for static method");
        let result = apply_edits(&content, &edits);
        assert!(
            result.contains("private static $result;"),
            "should declare static property: {}",
            result
        );
        assert!(
            result.contains("self::$result = 42;"),
            "should use self:: access: {}",
            result
        );
    }

    #[test]
    fn rejects_outside_method_body() {
        let php = "<?php\n/*|*/$result = 42;\n";
        assert!(
            run_convert(php).is_none(),
            "should not offer action outside a method"
        );
    }

    #[test]
    fn rejects_this_variable() {
        // $this can never be converted — it's special.
        let php = "<?php\nclass Foo {\n    public function bar() {\n        /*|*/$this = new self();\n    }\n}";
        assert!(
            run_convert(php).is_none(),
            "should not offer action for $this"
        );
    }
}
