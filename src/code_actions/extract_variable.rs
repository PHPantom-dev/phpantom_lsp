//! **Extract Variable** code action (`refactor.extract`).
//!
//! When the user selects a non-empty expression, this action introduces a
//! new local variable assigned to the selected expression on the line
//! immediately before the enclosing statement, and replaces the selection
//! with the new variable reference.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::code_actions::naming::{snake_to_camel, to_camel_case};
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::parser::with_parsed_program;
use crate::scope_collector::ScopeMap;
use crate::util::{find_identical_occurrences, offset_to_position, position_to_byte_offset};

// ─── Name generation ────────────────────────────────────────────────────────

/// Strip a single layer of balanced outer parentheses from an expression.
///
/// `"($a + $b)"` → `"$a + $b"`, but `"foo($x)"` is left unchanged
/// because the parens are part of the call syntax, not a redundant wrapper.
fn strip_outer_parens(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'(' || bytes[bytes.len() - 1] != b')' {
        return s;
    }
    // Walk the interior and verify the opening '(' at position 0 is
    // the one that matches the closing ')' at the end.  If the depth
    // drops to zero before we reach the last character, the outer
    // parens are not a matched wrapper (e.g. `(a) + (b)`).
    let mut depth: u32 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 && i < bytes.len() - 1 {
                    // Closed before the final character — not an outer wrapper.
                    return s;
                }
            }
            _ => {}
        }
    }
    // The parens wrap the entire expression — strip them.
    s[1..s.len() - 1].trim()
}

/// Returns `true` when the selected text parses as a valid, self-contained
/// PHP expression.  We wrap it in `<?php $__x = <selection>;` and check
/// that the parser produces no errors.  This rejects fragments like
/// `save` (bare method name), `$this` when it's part of `$this->foo()`,
/// partial tokens, and other nonsensical selections.
fn is_valid_expression(selected_text: &str) -> bool {
    let trimmed = selected_text.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Quick rejects for obvious non-expressions:
    // - Bare identifiers that aren't `$var`, `self`, `static`, `parent`,
    //   `true`, `false`, `null`, or a numeric/string literal.
    //   e.g. `save`, `getName` — these are method/function names, not
    //   standalone expressions.
    if !trimmed.starts_with('$')
        && !trimmed.starts_with('\'')
        && !trimmed.starts_with('"')
        && !trimmed.starts_with('[')
        && !trimmed.starts_with('(')
        && !trimmed.starts_with("new ")
        && !trimmed.starts_with("clone ")
        && !trimmed.starts_with("fn(")
        && !trimmed.starts_with("fn (")
        && !trimmed.starts_with("function")
        && !trimmed.starts_with("match")
        && !trimmed.starts_with("yield")
        && !trimmed.starts_with("throw")
        && !trimmed.starts_with('!')
        && !trimmed.starts_with('-')
        && !trimmed.starts_with('~')
        && !trimmed.starts_with('\\')
        && !trimmed.starts_with("self::")
        && !trimmed.starts_with("static::")
        && !trimmed.starts_with("parent::")
    {
        // Could be a numeric literal (0, 1.5, 0x1F, etc.), a constant
        // (true/false/null/CONST), or a function/static-method call.
        // Allow those through if they look like a call or known keyword.
        let first_char = trimmed.as_bytes()[0];
        let is_numeric = first_char.is_ascii_digit();
        let is_keyword = matches!(
            trimmed,
            "true" | "false" | "null" | "self" | "static" | "parent"
        );
        // Allow `ClassName::method(...)`, `func(...)`, `CONST_NAME`.
        let has_call_parens = trimmed.contains('(');
        let has_double_colon = trimmed.contains("::");
        let is_all_upper_const = trimmed.chars().all(|c| c.is_ascii_uppercase() || c == '_');

        if !is_numeric
            && !is_keyword
            && !has_call_parens
            && !has_double_colon
            && !is_all_upper_const
        {
            return false;
        }
    }

    // Reject selections that contain a semicolon in a non-trailing
    // position — this indicates multiple statements (e.g.
    // `$this->foo();\n$this->bar()`).  A trailing semicolon is fine
    // because `$expr;` is just an expression with a statement terminator
    // that we strip before wrapping.
    let body = trimmed.strip_suffix(';').unwrap_or(trimmed);
    if contains_unquoted_semicolon(body) {
        return false;
    }

    // Parse `<?php $__x = <body>;` — if the parser produces errors,
    // the selection is not a valid expression.
    let wrapper = format!("<?php $__x = {};", body);
    with_parsed_program(&wrapper, "extract_variable_check", |program, _| {
        program.errors.is_empty()
    })
}

/// Check whether `text` contains a semicolon outside of string literals.
///
/// Uses a simple quote-parity heuristic that handles the common cases
/// (`'...'` and `"..."`) but not heredoc/nowdoc.
fn contains_unquoted_semicolon(text: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let mut prev_backslash = false;

    for ch in text.chars() {
        if prev_backslash {
            prev_backslash = false;
            continue;
        }
        if ch == '\\' {
            prev_backslash = true;
            continue;
        }
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ';' if !in_single && !in_double => return true,
            _ => {}
        }
    }
    false
}

/// Returns `true` when the selection `[start, end)` covers the entire
/// RHS of a simple assignment like `$var = <selection>;`.  Extracting
/// it would just produce a pointless intermediary:
/// `$variable = expr; $var = $variable;`
fn is_entire_assignment_rhs(content: &str, start: usize, end: usize) -> bool {
    // Find the start of the line containing `start`.
    let before = &content[..start];
    let line_start = match before.rfind('\n') {
        Some(pos) => pos + 1,
        None => 0,
    };

    // Find the end of the line containing `end`.
    let line_end = content[end..]
        .find('\n')
        .map_or(content.len(), |pos| end + pos);

    let line = &content[line_start..line_end];
    let line_trimmed = line.trim();
    let selected = content[start..end].trim();

    // Check if the line matches `$var = <selected>;` (with optional
    // type hint / visibility modifiers stripped — keep it simple and
    // just look for `= <selected>;` at the end).
    if let Some(eq_pos) = line_trimmed.find('=') {
        // Make sure it's `=` not `==`, `===`, `!=`, `<=`, `>=`, `=>`.
        let before_eq = if eq_pos > 0 {
            line_trimmed.as_bytes()[eq_pos - 1]
        } else {
            b' '
        };
        let after_eq = if eq_pos + 1 < line_trimmed.len() {
            line_trimmed.as_bytes()[eq_pos + 1]
        } else {
            b' '
        };
        if before_eq != b'!'
            && before_eq != b'<'
            && before_eq != b'>'
            && after_eq != b'='
            && after_eq != b'>'
        {
            let rhs_part = line_trimmed[eq_pos + 1..].trim();
            // rhs_part should be `<selected>;`
            if rhs_part == format!("{};", selected) {
                return true;
            }
        }
    }
    false
}

/// Returns `true` when the selection `[start, end)` covers the entire
/// expression part of a standalone expression statement (i.e. the line
/// is just `<indent><expression>;`).  Extracting such a selection into
/// a variable would produce a useless `$var;` statement.
fn is_entire_expression_statement(content: &str, start: usize, end: usize) -> bool {
    let selected = content[start..end].trim();
    if selected.is_empty() {
        return false;
    }

    // Strip a trailing semicolon if present — `var_dump($value);` and
    // `var_dump($value)` should both be recognised as standalone
    // expression statements.
    let expr = selected.strip_suffix(';').unwrap_or(selected).trim();
    if expr.is_empty() {
        return false;
    }

    // Find the source line that contains the expression.  Use `end - 1`
    // so a selection ending right after `;\n` still lands on the correct
    // line.
    let expr_end = end.saturating_sub(1).max(start);
    let line_start = content[..expr_end].rfind('\n').map_or(0, |pos| pos + 1);
    let line_end = content[end..]
        .find('\n')
        .map_or(content.len(), |pos| end + pos);

    let line_trimmed = content[line_start..line_end].trim();

    // The line (after trimming whitespace) is exactly `expr;` — the
    // selection covers the whole expression statement.
    let with_semi = format!("{};", expr);
    line_trimmed == with_semi || line_trimmed == expr
}

/// Generate a variable name (without `$` prefix) from the selected
/// expression text.
///
/// Heuristics:
/// - Method call: `$user->getName()` → `name`
/// - Property access: `$user->email` → `email`
/// - Static call: `Carbon::now()` → `now`
/// - Function call: `array_filter($items, ...)` → `arrayFilter`
/// - Fallback: `variable`
fn generate_variable_name(expression: &str) -> String {
    let expr = expression.trim();

    // Try method call: `...->name(...)` or `...?->name(...)`
    if let Some(name) = extract_method_call_name(expr) {
        return name;
    }

    // Try property access: `...->name` or `...?->name`
    if let Some(name) = extract_property_name(expr) {
        return name;
    }

    // Try static call: `Class::method(...)`
    if let Some(name) = extract_static_call_name(expr) {
        return name;
    }

    // Try function call: `func_name(...)`
    if let Some(name) = extract_function_call_name(expr) {
        return name;
    }

    "variable".to_string()
}

/// Extract name from a method call like `$user->getName()`.
fn extract_method_call_name(expr: &str) -> Option<String> {
    // Find the last `->` or `?->` that is followed by an identifier and `(`
    // We need to be careful with nested calls, so find the rightmost
    // arrow operator at the top nesting level.
    let name_part = find_last_member_access(expr)?;

    // name_part should look like `getName()` or `getName($x)`
    // Strip trailing parens+args
    let ident = name_part.split('(').next()?;
    let ident = ident.trim();

    if ident.is_empty() || !name_part.contains('(') {
        return None;
    }

    // Strip common prefixes like get/is/has for cleaner names
    let stripped = strip_accessor_prefix(ident);
    Some(to_camel_case(stripped))
}

/// Extract name from a property access like `$user->email`.
fn extract_property_name(expr: &str) -> Option<String> {
    let name_part = find_last_member_access(expr)?;

    // Must NOT contain `(` (that would be a method call)
    if name_part.contains('(') {
        return None;
    }

    let ident = name_part.trim();
    if ident.is_empty() {
        return None;
    }

    Some(to_camel_case(ident))
}

/// Extract name from a static call like `Carbon::now()`.
fn extract_static_call_name(expr: &str) -> Option<String> {
    // Find `::` not inside strings/parens
    let double_colon = find_top_level_double_colon(expr)?;
    let after = &expr[double_colon + 2..];

    let ident = after.split('(').next()?.trim();
    if ident.is_empty() {
        return None;
    }
    if !after.contains('(') {
        // Static property or constant access — still a valid extraction
        let stripped = ident.strip_prefix('$').unwrap_or(ident);
        return Some(to_camel_case(stripped));
    }

    Some(to_camel_case(ident))
}

/// Extract name from a function call like `array_filter(...)`.
fn extract_function_call_name(expr: &str) -> Option<String> {
    // Must start with an identifier (possibly namespaced) followed by `(`
    let paren_pos = expr.find('(')?;
    let before = expr[..paren_pos].trim();

    // Get the last segment if namespaced: `Foo\Bar\baz` → `baz`
    let ident = before.rsplit('\\').next().unwrap_or(before);

    if ident.is_empty() || !ident.chars().next()?.is_alphabetic() {
        return None;
    }

    // Verify all chars are valid identifier chars
    if !ident.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }

    Some(snake_to_camel(ident))
}

/// Find the last `->` or `?->` member access at the top nesting level
/// and return the part after it.
fn find_last_member_access(expr: &str) -> Option<String> {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut last_arrow_end = None;
    let bytes = expr.as_bytes();

    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];

        // Skip escaped characters inside strings
        if (in_single_quote || in_double_quote) && ch == b'\\' {
            i += 2;
            continue;
        }

        if ch == b'\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
        } else if ch == b'"' && !in_single_quote {
            in_double_quote = !in_double_quote;
        }

        if in_single_quote || in_double_quote {
            i += 1;
            continue;
        }

        match ch {
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'[' => depth_bracket += 1,
            b']' => depth_bracket -= 1,
            b'-' if depth_paren == 0
                && depth_bracket == 0
                && i + 1 < bytes.len()
                && bytes[i + 1] == b'>' =>
            {
                last_arrow_end = Some(i + 2);
                i += 2;
                continue;
            }
            b'?' if depth_paren == 0
                && depth_bracket == 0
                && i + 2 < bytes.len()
                && bytes[i + 1] == b'-'
                && bytes[i + 2] == b'>' =>
            {
                last_arrow_end = Some(i + 3);
                i += 3;
                continue;
            }
            _ => {}
        }

        i += 1;
    }

    let arrow_end = last_arrow_end?;
    let after = &expr[arrow_end..];
    if after.is_empty() {
        return None;
    }
    Some(after.to_string())
}

/// Find `::` at the top level (outside parens/brackets/strings).
fn find_top_level_double_colon(expr: &str) -> Option<usize> {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let bytes = expr.as_bytes();

    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];

        if (in_single_quote || in_double_quote) && ch == b'\\' {
            i += 2;
            continue;
        }

        if ch == b'\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
        } else if ch == b'"' && !in_single_quote {
            in_double_quote = !in_double_quote;
        }

        if in_single_quote || in_double_quote {
            i += 1;
            continue;
        }

        match ch {
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'[' => depth_bracket += 1,
            b']' => depth_bracket -= 1,
            b':' if depth_paren == 0
                && depth_bracket == 0
                && i + 1 < bytes.len()
                && bytes[i + 1] == b':' =>
            {
                return Some(i);
            }
            _ => {}
        }

        i += 1;
    }

    None
}

/// Strip common accessor prefixes (`get`, `is`, `has`) from a method name
/// for cleaner variable names: `getName` → `Name`, then camelCase → `name`.
fn strip_accessor_prefix(name: &str) -> &str {
    for prefix in &["get", "is", "has"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            // Only strip if the next char is uppercase (to avoid stripping
            // from names like "island" or "hasty").
            if rest.starts_with(|c: char| c.is_uppercase()) {
                return rest;
            }
        }
    }
    name
}

/// Deduplicate a variable name against existing variables in scope.
///
/// If `$name` already exists, tries `$name1`, `$name2`, etc.
/// `existing_vars` should contain names WITH `$` prefix.
fn deduplicate_name(name: &str, existing_vars: &[String]) -> String {
    let candidate = format!("${}", name);
    if !existing_vars.contains(&candidate) {
        return name.to_string();
    }

    for i in 1..100 {
        let numbered = format!("${}{}", name, i);
        if !existing_vars.contains(&numbered) {
            return format!("{}{}", name, i);
        }
    }

    // Extremely unlikely fallback
    name.to_string()
}

// ─── Insertion point ────────────────────────────────────────────────────────

/// Find the start-of-line offset and indentation for the statement that
/// contains the selection.
///
/// Returns `(line_start_offset, indentation_string)`.
fn find_enclosing_statement_line(content: &str, selection_start: usize) -> (usize, String) {
    // Walk backwards from the selection start to find the beginning of the line.
    // The "enclosing statement" heuristic: find the start of the line
    // containing the selection. This works well for typical single-statement
    // lines in PHP.
    let before = &content[..selection_start];

    let line_start = match before.rfind('\n') {
        Some(pos) => pos + 1,
        None => 0,
    };

    // Extract indentation (leading whitespace on this line).
    let line_content = &content[line_start..];
    let indent_len = line_content
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    let indentation = line_content[..indent_len].to_string();

    (line_start, indentation)
}

// ─── Scope map building ─────────────────────────────────────────────────────

/// Build a `ScopeMap` for the file by walking the AST.
///
/// This finds the enclosing function/method scope or falls back to
/// top-level scope.
fn build_scope_map(content: &str, offset: u32) -> ScopeMap {
    with_parsed_program(content, "extract_variable", |program, content| {
        crate::scope_collector::build_scope_map_for_offset(
            program.statements.as_slice(),
            offset,
            content.len() as u32,
        )
    })
}

// ─── Code action ────────────────────────────────────────────────────────────

impl Backend {
    /// Collect "Extract Variable" code actions.
    ///
    /// This action is offered when the user has a non-empty selection.
    /// It extracts the selected expression into a new local variable.
    ///
    /// Phase 1 performs lightweight validation only.  The expensive
    /// work (scope map, name generation, occurrence counting, edit
    /// building) is deferred to [`resolve_extract_variable`] (Phase 2).
    pub(crate) fn collect_extract_variable_actions(
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

        let in_executable_body = crate::parser::with_parsed_program(
            content,
            "extract_variable_context",
            |program, _| {
                matches!(
                    find_cursor_context(&program.statements, start_offset as u32),
                    CursorContext::InFunction(_, true)
                        | CursorContext::InClassLike {
                            member: MemberContext::Method(_, true),
                            ..
                        }
                )
            },
        );
        if !in_executable_body {
            return;
        }

        let selected_text = &content[start_offset..end_offset];

        // Skip if the selection is purely whitespace.
        if selected_text.trim().is_empty() {
            return;
        }

        // Skip if the selected text is not a valid self-contained expression.
        // This rejects nonsensical selections like `save` (bare method name),
        // `$this` when it's the object in `$this->foo()`, or any partial
        // token / syntax fragment that would produce broken code.
        if !is_valid_expression(selected_text) {
            return;
        }

        // Skip if the selection is just a plain variable (`$id`, `$this`,
        // `$total`, etc.).  Extracting a variable into another variable
        // is always pointless.
        let trimmed_check = selected_text.trim();
        if trimmed_check.starts_with('$')
            && trimmed_check[1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return;
        }

        // Skip if the selection is a fragment of a member access chain.
        // Selecting `getLabel()` from `$order->getLabel()` would produce
        // broken code — it's not a standalone expression even though the
        // parser accepts `getLabel()` as a global function call.
        {
            let before = &content[..start_offset];
            let before_trimmed = before.trim_end();
            if before_trimmed.ends_with("->")
                || before_trimmed.ends_with("?->")
                || before_trimmed.ends_with("::")
            {
                return;
            }
        }

        // Skip if the selection covers the entire expression of a standalone
        // expression statement.  Extracting `$this->save($id)` into
        // `$save = $this->save($id); $save;` is nonsensical — the call
        // doesn't produce a value worth capturing.
        if is_entire_expression_statement(content, start_offset, end_offset) {
            return;
        }

        // Skip if the selection covers the entire RHS of an existing
        // assignment.  Extracting `$total * 0.21` from `$tax = $total * 0.21;`
        // just produces a pointless `$variable = $total * 0.21; $tax = $variable;`.
        if is_entire_assignment_rhs(content, start_offset, end_offset) {
            return;
        }

        // Phase 1: emit lightweight code action(s) with no edit.
        // Scope map building, name generation, and edit construction
        // are deferred to Phase 2.

        // Cheap text search: does the trimmed expression appear more
        // than once in the file?  This avoids building a scope map
        // just to decide whether to show the "all occurrences" variant.
        let trimmed = selected_text.trim();
        let has_other_occurrences = {
            let first = content.find(trimmed);
            match first {
                Some(pos) => content[pos + trimmed.len()..].contains(trimmed),
                None => false,
            }
        };

        let title = if has_other_occurrences {
            "Extract variable (this occurrence)"
        } else {
            "Extract variable"
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
                "refactor.extractVariable",
                uri,
                &params.range,
                serde_json::json!({ "all_occurrences": false }),
            )),
        }));

        if has_other_occurrences {
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Extract variable (all occurrences)".to_string(),
                kind: Some(CodeActionKind::REFACTOR_EXTRACT),
                diagnostics: None,
                edit: None,
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: Some(make_code_action_data(
                    "refactor.extractVariableAll",
                    uri,
                    &params.range,
                    serde_json::json!({ "all_occurrences": true }),
                )),
            }));
        }
    }

    /// Resolve a deferred "Extract Variable" code action by computing
    /// the full workspace edit.
    ///
    /// Called from `resolve_code_action` when `action_kind` is
    /// `"refactor.extractVariable"` or `"refactor.extractVariableAll"`.
    pub(crate) fn resolve_extract_variable(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let all_occurrences = data
            .extra
            .get("all_occurrences")
            .and_then(|v| v.as_bool())
            .unwrap_or(data.action_kind == "refactor.extractVariableAll");

        let start_offset = position_to_byte_offset(content, data.range.start);
        let end_offset = position_to_byte_offset(content, data.range.end);

        if start_offset >= end_offset || end_offset > content.len() {
            return None;
        }

        let selected_text = &content[start_offset..end_offset];
        let trimmed = selected_text.trim();

        if trimmed.is_empty() || !is_valid_expression(trimmed) {
            return None;
        }

        // Generate variable name and deduplicate.
        let base_name = generate_variable_name(selected_text);
        let scope_map = build_scope_map(content, start_offset as u32);
        let existing_vars = scope_map.variables_in_scope(start_offset as u32);
        let var_name = deduplicate_name(&base_name, &existing_vars);

        let rhs = strip_outer_parens(trimmed);
        let replacement_text = format!("${}", var_name);

        let doc_uri: Url = match data.uri.parse() {
            Ok(u) => u,
            Err(_) => return None,
        };

        if all_occurrences {
            // ── All occurrences mode ────────────────────────────────
            let (scope_start, scope_end) = scope_map
                .enclosing_frame(start_offset as u32)
                .map(|f| (f.start as usize, f.end as usize))
                .unwrap_or((0, content.len()));

            let trim_start_delta = selected_text.len() - selected_text.trim_start().len();
            let trim_end_delta = selected_text.len() - selected_text.trim_end().len();
            let trimmed_start = start_offset + trim_start_delta;
            let trimmed_end = end_offset - trim_end_delta;

            let other_occurrences = find_identical_occurrences(
                content,
                trimmed,
                trimmed_start,
                trimmed_end,
                scope_start,
                scope_end,
            );

            let mut all_offsets: Vec<(usize, usize)> = vec![(start_offset, end_offset)];
            all_offsets.extend(&other_occurrences);
            all_offsets.sort_by_key(|&(s, _)| s);

            // Insert before the first occurrence's enclosing statement.
            let (first_start, _) = all_offsets[0];
            let (first_line_start, first_indent) =
                find_enclosing_statement_line(content, first_start);
            let insert_text = format!("{}${} = {};\n", first_indent, var_name, rhs);
            let insert_pos = offset_to_position(content, first_line_start);

            let mut edits = vec![TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: insert_text,
            }];

            for &(occ_start, occ_end) in &all_offsets {
                let start_pos = offset_to_position(content, occ_start);
                let end_pos = offset_to_position(content, occ_end);
                edits.push(TextEdit {
                    range: Range {
                        start: start_pos,
                        end: end_pos,
                    },
                    new_text: replacement_text.clone(),
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
            let (line_start, indentation) = find_enclosing_statement_line(content, start_offset);
            let insert_text = format!("{}${} = {};\n", indentation, var_name, rhs);
            let insert_pos = offset_to_position(content, line_start);

            let edit_insert = TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: insert_text,
            };

            let edit_replace = TextEdit {
                range: data.range,
                new_text: replacement_text,
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
#[path = "extract_variable_tests.rs"]
mod tests;
