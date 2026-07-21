//! **Extract Constant** code action (`refactor.extract`).
//!
//! When the user selects a literal expression (string, integer, float,
//! or boolean) inside a class body, this action introduces a new class
//! constant with a generated name, assigns the literal value, and
//! replaces the selection (and optionally all identical occurrences in
//! the class) with `self::CONSTANT_NAME`.
//!
//! The action uses the two-phase `codeAction/resolve` model: Phase 1
//! emits a lightweight stub with no edit, Phase 2 computes the full
//! workspace edit when the user picks the action.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::code_actions::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::php_type::PhpType;
use crate::types::PhpVersion;
use crate::util::infer_type_from_literal;
use crate::util::{find_identical_occurrences, offset_to_position, position_to_byte_offset};

// ─── Literal detection ──────────────────────────────────────────────────────

/// Returns `true` when the trimmed selection text looks like a PHP
/// literal that can be extracted into a class constant.
fn is_extractable_literal(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }

    // String literals
    if (t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"')) {
        return t.len() >= 2;
    }

    // Boolean / null literals
    let lower = t.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "false" | "null") {
        return true;
    }

    // Numeric literals (integer or float)
    if is_numeric_literal(t) {
        return true;
    }

    // Concatenated string expression: `'a' . 'b'`
    if is_concat_expression(t) {
        return true;
    }

    // Negative numeric literal: `-42`, `-3.14`
    if t.starts_with('-') && is_numeric_literal(t[1..].trim_start()) {
        return true;
    }

    false
}

/// Returns `true` when the text is a numeric literal (integer or float,
/// including hex `0x`, octal `0o`, binary `0b`, and underscored forms).
fn is_numeric_literal(t: &str) -> bool {
    if t.is_empty() {
        return false;
    }

    let bytes = t.as_bytes();

    // Hex: 0x1F, 0X1f
    if bytes.len() >= 3
        && bytes[0] == b'0'
        && (bytes[1] == b'x' || bytes[1] == b'X')
        && bytes[2..]
            .iter()
            .all(|b| b.is_ascii_hexdigit() || *b == b'_')
    {
        return true;
    }

    // Binary: 0b101
    if bytes.len() >= 3
        && bytes[0] == b'0'
        && (bytes[1] == b'b' || bytes[1] == b'B')
        && bytes[2..]
            .iter()
            .all(|b| *b == b'0' || *b == b'1' || *b == b'_')
    {
        return true;
    }

    // Octal: 0o77
    if bytes.len() >= 3
        && bytes[0] == b'0'
        && (bytes[1] == b'o' || bytes[1] == b'O')
        && bytes[2..]
            .iter()
            .all(|b| (b'0'..=b'7').contains(b) || *b == b'_')
    {
        return true;
    }

    // Decimal integer or float
    let mut saw_dot = false;
    let mut saw_e = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'0'..=b'9' | b'_' => {}
            b'.' if !saw_dot && !saw_e => saw_dot = true,
            b'e' | b'E' if !saw_e && i > 0 => {
                saw_e = true;
                // Allow optional +/- after exponent
                if i + 1 < bytes.len() && (bytes[i + 1] == b'+' || bytes[i + 1] == b'-') {
                    // Skip the sign — it will be consumed next iteration.
                    // We need a slightly different approach: just validate
                    // the whole thing.
                    return validate_float_suffix(&bytes[i + 1..]);
                }
            }
            _ => return false,
        }
    }
    // Must have at least one digit
    bytes.iter().any(|b| b.is_ascii_digit())
}

/// Validate the remainder of a float literal after the 'e'/'E'.
fn validate_float_suffix(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let start = if bytes[0] == b'+' || bytes[0] == b'-' {
        1
    } else {
        0
    };
    if start >= bytes.len() {
        return false;
    }
    bytes[start..]
        .iter()
        .all(|b| b.is_ascii_digit() || *b == b'_')
        && bytes[start..].iter().any(|b| b.is_ascii_digit())
}

/// Returns `true` when the text is a concatenated string expression
/// like `'prefix_' . 'suffix'`. Each segment must be a string literal
/// or a numeric literal separated by `.` operators.
fn is_concat_expression(text: &str) -> bool {
    if !text.contains('.') {
        return false;
    }

    // Split on ` . ` (the PHP concatenation operator with typical spacing).
    // We also handle `.` without spaces.
    let parts = split_concat_parts(text);
    if parts.len() < 2 {
        return false;
    }

    parts.iter().all(|p| {
        let t = p.trim();
        (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2)
            || (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
            || is_numeric_literal(t)
    })
}

/// Split a string on the PHP `.` concatenation operator, respecting
/// string literal boundaries.
fn split_concat_parts(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'.' if !in_single && !in_double => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&text[start..]);
    parts
}

// ─── Name generation ────────────────────────────────────────────────────────

/// Generate a SCREAMING_SNAKE_CASE constant name from a literal value.
fn generate_constant_name(value: &str) -> String {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();

    // Boolean
    if lower == "true" {
        return "IS_ENABLED".to_string();
    }
    if lower == "false" {
        return "IS_DISABLED".to_string();
    }
    if lower == "null" {
        return "DEFAULT_VALUE".to_string();
    }

    // String literal — strip quotes and convert to SCREAMING_SNAKE_CASE
    if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"'))
    {
        let inner = &trimmed[1..trimmed.len() - 1];
        let name = string_to_screaming_snake(inner);
        if !name.is_empty() && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            return name;
        }
        return "CONSTANT".to_string();
    }

    // Negative numeric
    if let Some(stripped) = trimmed.strip_prefix('-') {
        let abs = stripped.trim_start();
        if abs.contains('.') || abs.contains('e') || abs.contains('E') {
            return "VALUE".to_string();
        }
        return format!("VALUE_{}", abs.replace('_', ""));
    }

    // Numeric literal
    if is_numeric_literal(trimmed) {
        // Float
        if trimmed.contains('.') || trimmed.contains('e') || trimmed.contains('E') {
            return "VALUE".to_string();
        }
        // Integer — use VALUE_NNN
        return format!("VALUE_{}", trimmed.replace('_', ""));
    }

    // Concatenated string expression — try to use the first segment
    if is_concat_expression(trimmed) {
        return "CONSTANT".to_string();
    }

    "CONSTANT".to_string()
}

/// Determine the PHP type name for a literal value.
///
/// Returns `Some("string")`, `Some("int")`, `Some("float")`, or
/// `Some("bool")` for the corresponding literal kinds.  Returns `None`
/// for values that don't have a clean single type (e.g. concat
/// expressions, `null`).
fn literal_type_name(value: &str) -> Option<PhpType> {
    let t = value.trim();
    if t.is_empty() {
        return None;
    }

    // null — PHP does not allow `null` as a typed constant type
    if t.eq_ignore_ascii_case("null") {
        return None;
    }

    // Negative numeric — strip the `-` prefix and delegate for the
    // absolute part so `infer_type_from_literal` handles the rest.
    if let Some(stripped) = t.strip_prefix('-') {
        let abs = stripped.trim_start();
        if let Some(ty) = infer_type_from_literal(abs) {
            if ty.is_int() {
                return Some(PhpType::int());
            }
            if ty.is_float() {
                return Some(PhpType::float());
            }
        }
        return None;
    }

    // Delegate to the shared literal type inference utility.
    // This must run BEFORE the concat check because `3.14` contains
    // `.` which `is_concat_expression` would misinterpret as the PHP
    // concatenation operator.
    if let Some(ty) = infer_type_from_literal(t) {
        if ty.is_int() {
            return Some(PhpType::int());
        }
        if ty.is_float() {
            return Some(PhpType::float());
        }
        if ty.is_bool() {
            return Some(PhpType::bool());
        }
        if ty.is_string_type() {
            return Some(PhpType::string());
        }
    }

    // Concat expression — result is string but syntax is complex.
    // Checked after the shared util so that floats like `3.14` are
    // not misclassified as concatenation (`3 . 14`).
    if is_concat_expression(t) {
        return Some(PhpType::string());
    }

    None
}

/// Convert a string to SCREAMING_SNAKE_CASE.
///
/// Non-alphanumeric characters become underscores. Consecutive
/// underscores are collapsed.
fn string_to_screaming_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_uppercase());
        } else if (ch == '_' || ch == '-' || ch == ' ' || ch == '/' || ch == '.')
            && !result.ends_with('_')
        {
            result.push('_');
        }
        // Skip other characters (e.g. special symbols).
    }
    // Trim trailing underscore
    while result.ends_with('_') {
        result.pop();
    }
    // Trim leading underscore
    while result.starts_with('_') {
        result.remove(0);
    }
    result
}

/// Ensure the generated name doesn't collide with existing constants.
/// If it does, append a numeric suffix.
fn deduplicate_constant_name(name: &str, existing: &[String]) -> String {
    if !existing.iter().any(|e| e == name) {
        return name.to_string();
    }
    for i in 1u32.. {
        let candidate = format!("{}_{}", name, i);
        if !existing.iter().any(|e| e == &candidate) {
            return candidate;
        }
    }
    unreachable!()
}

// ─── AST helpers ────────────────────────────────────────────────────────────

/// Information about the class-like declaration containing the cursor.
struct ClassBodyInfo {
    /// Byte offset of the opening `{` of the class body.
    body_start: usize,
    /// Byte offset of the closing `}` of the class body.
    body_end: usize,
    /// Names of existing constants in the class.
    existing_constants: Vec<String>,
    /// Byte offset immediately after the last existing constant
    /// declaration (including its trailing newline), or None if there
    /// are no constants.
    after_last_constant: Option<usize>,
    /// The visibility of the method or context where the literal appears.
    context_visibility: &'static str,
    /// Whether a blank line is needed after the new constant to separate
    /// it from the next non-constant member (method, property, etc.).
    /// `true` when the insertion point is immediately followed by a
    /// non-constant member with no intervening blank line.
    needs_trailing_blank_line: bool,
}

/// Walk the AST to find class body info at the given cursor offset.
fn find_class_body_info(content: &str, cursor: u32) -> Option<ClassBodyInfo> {
    crate::parser::with_parsed_program(content, "extract_constant", |program, content| {
        for stmt in program.statements.iter() {
            if let Some(info) = find_class_info_in_statement(stmt, content, cursor) {
                return Some(info);
            }
        }
        None
    })
}

/// Recursively search a statement for a class-like declaration
/// containing the cursor.
fn find_class_info_in_statement(
    stmt: &Statement<'_>,
    content: &str,
    cursor: u32,
) -> Option<ClassBodyInfo> {
    match stmt {
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                if let Some(info) = find_class_info_in_statement(s, content, cursor) {
                    return Some(info);
                }
            }
            None
        }
        Statement::Class(class) => {
            let span = class.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                Some(extract_class_body_info(
                    &class.members,
                    content,
                    class.left_brace.start.offset as usize,
                    class.right_brace.end.offset as usize,
                    cursor,
                ))
            } else {
                None
            }
        }
        Statement::Trait(tr) => {
            let span = tr.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                Some(extract_class_body_info(
                    &tr.members,
                    content,
                    tr.left_brace.start.offset as usize,
                    tr.right_brace.end.offset as usize,
                    cursor,
                ))
            } else {
                None
            }
        }
        Statement::Enum(en) => {
            let span = en.span();
            if cursor >= span.start.offset && cursor <= span.end.offset {
                Some(extract_class_body_info(
                    &en.members,
                    content,
                    en.left_brace.start.offset as usize,
                    en.right_brace.end.offset as usize,
                    cursor,
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract class body metadata from a sequence of class-like members.
fn extract_class_body_info(
    members: &Sequence<'_, ClassLikeMember<'_>>,
    content: &str,
    body_start: usize,
    body_end: usize,
    cursor: u32,
) -> ClassBodyInfo {
    let mut existing_constants = Vec::new();
    let mut after_last_constant: Option<usize> = None;
    let mut context_visibility = "private";
    // Byte offset of the start of the first non-constant member, if any.
    let mut first_non_const_start: Option<usize> = None;

    for member in members.iter() {
        match member {
            ClassLikeMember::Constant(constant) => {
                // Collect existing constant names.
                for item in constant.items.iter() {
                    existing_constants.push(bytes_to_str(item.name.value).to_string());
                }
                let end = constant.span().end.offset as usize;
                after_last_constant = Some(end);
            }
            other => {
                // Track the first non-constant member for blank-line logic.
                if first_non_const_start.is_none() {
                    first_non_const_start = Some(other.span().start.offset as usize);
                }

                match other {
                    ClassLikeMember::Method(method) => {
                        let method_span = method.span();
                        if cursor >= method_span.start.offset && cursor <= method_span.end.offset {
                            for m in method.modifiers.iter() {
                                match m {
                                    modifier::Modifier::Public(_) => context_visibility = "public",
                                    modifier::Modifier::Protected(_) => {
                                        context_visibility = "protected"
                                    }
                                    modifier::Modifier::Private(_) => {
                                        context_visibility = "private"
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    ClassLikeMember::Property(property) => {
                        let prop_span = property.span();
                        if cursor >= prop_span.start.offset && cursor <= prop_span.end.offset {
                            for m in property.modifiers().iter() {
                                match m {
                                    modifier::Modifier::Public(_) => context_visibility = "public",
                                    modifier::Modifier::Protected(_) => {
                                        context_visibility = "protected"
                                    }
                                    modifier::Modifier::Private(_) => {
                                        context_visibility = "private"
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Determine whether we need a trailing blank line after the new
    // constant.  This is true when:
    //  - There are no existing constants (we're inserting at the top
    //    of the class body, before non-constant members), OR
    //  - There are existing constants but the text between the last
    //    constant and the first non-constant member doesn't already
    //    contain a blank line.
    let needs_trailing_blank_line = if let Some(non_const_start) = first_non_const_start {
        let gap_from = after_last_constant.unwrap_or(body_start);
        let gap = &content[gap_from..non_const_start];
        // A "blank line" means two consecutive newlines (possibly with
        // whitespace between them).
        !has_blank_line(gap)
    } else {
        // No non-constant members at all — no trailing blank line needed.
        false
    };

    ClassBodyInfo {
        body_start,
        body_end,
        existing_constants,
        after_last_constant,
        context_visibility,
        needs_trailing_blank_line,
    }
}

/// Returns `true` when `text` contains a blank line (two newlines with
/// only whitespace between them).
fn has_blank_line(text: &str) -> bool {
    let mut saw_newline = false;
    for ch in text.chars() {
        if ch == '\n' {
            if saw_newline {
                return true;
            }
            saw_newline = true;
        } else if ch != ' ' && ch != '\t' && ch != '\r' {
            saw_newline = false;
        }
    }
    false
}

/// Determine the indentation used for members inside the class body.
/// Looks at the line after the opening brace to detect the indent.
fn detect_member_indent(content: &str, body_start: usize) -> String {
    // Find the first newline after the opening brace.
    if let Some(nl_pos) = content[body_start..].find('\n') {
        let line_start = body_start + nl_pos + 1;
        let rest = &content[line_start..];
        let indent_len = rest.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        if indent_len > 0 {
            return rest[..indent_len].to_string();
        }
    }
    // Fallback: 4 spaces.
    "    ".to_string()
}

// ─── Code action ────────────────────────────────────────────────────────────

impl Backend {
    /// Collect "Extract Constant" code actions.
    ///
    /// This action is offered when the user selects a literal expression
    /// inside a class, trait, or enum body.
    ///
    /// Phase 1 performs lightweight validation only.  The expensive
    /// work (AST walk, name generation, edit building) is deferred to
    /// [`resolve_extract_constant`] (Phase 2).
    pub(crate) fn collect_extract_constant_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        // Only activate when the selection is non-empty.
        if params.range.start == params.range.end {
            return;
        }

        let start_offset = position_to_byte_offset(content, params.range.start);
        let end_offset = position_to_byte_offset(content, params.range.end);

        if start_offset >= end_offset || end_offset > content.len() {
            return;
        }

        let selected_text = &content[start_offset..end_offset];
        let trimmed = selected_text.trim();

        // Skip if the selection is purely whitespace.
        if trimmed.is_empty() {
            return;
        }

        // Only extractable literals qualify.
        if !is_extractable_literal(trimmed) {
            return;
        }

        // Verify the cursor is inside a class-like body.  A method body
        // (interfaces can't have concrete methods, but the parser still
        // produces them, so we allow it) or a property default value is a
        // valid extraction site.  A constant value or a non-class context
        // is not.
        let cursor = start_offset as u32;
        let is_valid_site =
            crate::parser::with_parsed_program(content, "extract_constant", |program, _| {
                let ctx = find_cursor_context(&program.statements, cursor);
                matches!(
                    &ctx,
                    CursorContext::InClassLike {
                        member: MemberContext::Method(_, true),
                        ..
                    } | CursorContext::InClassLike {
                        member: MemberContext::Property(_),
                        ..
                    }
                )
            });
        if !is_valid_site {
            return;
        }

        // Cheap text search: does the literal appear more than once
        // in the file?
        let has_other_occurrences = {
            let first = content.find(trimmed);
            match first {
                Some(pos) => content[pos + trimmed.len()..].contains(trimmed),
                None => false,
            }
        };

        let title = if has_other_occurrences {
            "Extract constant (this occurrence)"
        } else {
            "Extract constant"
        };

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: title.to_string(),
            kind: Some(CodeActionKind::REFACTOR_EXTRACT),
            diagnostics: None,
            edit: None,
            command: None,
            is_preferred: Some(false),
            disabled: None,
            data: Some(make_code_action_data(
                "refactor.extractConstant",
                uri,
                &params.range,
                serde_json::json!({ "all_occurrences": false }),
            )),
        }));

        if has_other_occurrences {
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Extract constant (all occurrences)".to_string(),
                kind: Some(CodeActionKind::REFACTOR_EXTRACT),
                diagnostics: None,
                edit: None,
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: Some(make_code_action_data(
                    "refactor.extractConstantAll",
                    uri,
                    &params.range,
                    serde_json::json!({ "all_occurrences": true }),
                )),
            }));
        }
    }

    /// Resolve a deferred "Extract Constant" code action by computing
    /// the full workspace edit.
    ///
    /// Called from `resolve_code_action` when `action_kind` is
    /// `"refactor.extractConstant"` or `"refactor.extractConstantAll"`.
    pub(crate) fn resolve_extract_constant(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let all_occurrences = data
            .extra
            .get("all_occurrences")
            .and_then(|v| v.as_bool())
            .unwrap_or(data.action_kind == "refactor.extractConstantAll");

        let start_offset = position_to_byte_offset(content, data.range.start);
        let end_offset = position_to_byte_offset(content, data.range.end);

        if start_offset >= end_offset || end_offset > content.len() {
            return None;
        }

        let selected_text = &content[start_offset..end_offset];
        let trimmed = selected_text.trim();

        if trimmed.is_empty() || !is_extractable_literal(trimmed) {
            return None;
        }

        // Find class body information.
        let class_info = find_class_body_info(content, start_offset as u32)?;

        // Generate constant name and deduplicate.
        let base_name = generate_constant_name(trimmed);
        let const_name = deduplicate_constant_name(&base_name, &class_info.existing_constants);

        let visibility = class_info.context_visibility;
        let indent = detect_member_indent(content, class_info.body_start);
        let php_version = self.php_version();

        // Determine insertion point.
        let insert_offset = if let Some(after_const) = class_info.after_last_constant {
            // Insert after the last constant. Find the next newline.
            let rest = &content[after_const..];
            if let Some(nl) = rest.find('\n') {
                after_const + nl + 1
            } else {
                after_const
            }
        } else {
            // No existing constants — insert at the top of the class body.
            // Find the first newline after the opening brace.
            let rest = &content[class_info.body_start..];
            if let Some(nl) = rest.find('\n') {
                class_info.body_start + nl + 1
            } else {
                class_info.body_start + 1
            }
        };

        // Build the constant declaration with optional type annotation.
        let type_name = literal_type_name(trimmed);
        let trailing_blank = if class_info.needs_trailing_blank_line {
            "\n"
        } else {
            ""
        };

        let const_declaration = if let Some(ty) = type_name {
            if php_version >= (PhpVersion::new(8, 3)) {
                // PHP 8.3+: typed constant syntax.
                format!(
                    "{}{} const {} {} = {};\n{}",
                    indent, visibility, ty, const_name, trimmed, trailing_blank
                )
            } else {
                // PHP < 8.3: use a docblock annotation.
                format!(
                    "{}/** @var {} */\n{}{} const {} = {};\n{}",
                    indent, ty, indent, visibility, const_name, trimmed, trailing_blank
                )
            }
        } else {
            format!(
                "{}{} const {} = {};\n{}",
                indent, visibility, const_name, trimmed, trailing_blank
            )
        };
        let replacement = format!("self::{}", const_name);

        let doc_uri: Url = match data.uri.parse() {
            Ok(u) => u,
            Err(_) => return None,
        };

        let insert_pos = offset_to_position(content, insert_offset);

        if all_occurrences {
            // ── All occurrences mode ────────────────────────────────
            let trim_start_delta = selected_text.len() - selected_text.trim_start().len();
            let trim_end_delta = selected_text.len() - selected_text.trim_end().len();
            let trimmed_start = start_offset + trim_start_delta;
            let trimmed_end = end_offset - trim_end_delta;

            let other_occurrences = find_identical_occurrences(
                content,
                trimmed,
                trimmed_start,
                trimmed_end,
                class_info.body_start,
                class_info.body_end,
            );

            let mut all_offsets: Vec<(usize, usize)> = vec![(start_offset, end_offset)];
            all_offsets.extend(&other_occurrences);
            all_offsets.sort_by_key(|&(s, _)| s);

            let mut edits = vec![TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: const_declaration,
            }];

            for &(occ_start, occ_end) in &all_offsets {
                let start_pos = offset_to_position(content, occ_start);
                let end_pos = offset_to_position(content, occ_end);
                edits.push(TextEdit {
                    range: Range {
                        start: start_pos,
                        end: end_pos,
                    },
                    new_text: replacement.clone(),
                });
            }

            let mut changes = HashMap::new();
            changes.insert(doc_uri, edits);

            Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            })
        } else {
            // ── Single occurrence mode ──────────────────────────────
            let edit_insert = TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: const_declaration,
            };

            let edit_replace = TextEdit {
                range: data.range,
                new_text: replacement,
            };

            let mut changes = HashMap::new();
            changes.insert(doc_uri, vec![edit_insert, edit_replace]);

            Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            })
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "extract_constant_tests.rs"]
mod tests;
