//! **Extract Function / Method** code action (`refactor.extract`).
//!
//! When the user selects one or more complete statements inside a
//! function or method body, this action extracts them into a new
//! function (or method, if `$this`/`self::`/`static::` is used).
//!
//! The implementation uses the `ScopeCollector` infrastructure to
//! classify variables as parameters, return values, or locals relative
//! to the selected range.  Type annotations are inferred via the hover
//! variable-type resolution pipeline.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use std::collections::HashMap;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::code_actions::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::completion::phpdoc::generation::enrichment_plain;
use crate::completion::resolver::Loaders;
use crate::php_type::PhpType;
use crate::scope_collector::ScopeMap;
use crate::types::ClassInfo;
use crate::util::{find_class_at_offset, offset_to_position, position_to_byte_offset};

// ─── Statement boundary validation ─────────────────────────────────────────

/// Check whether the selected byte range `[start, end)` covers one or
/// more complete statements.
///
/// We parse the file and walk the AST to verify that every statement
/// whose span overlaps the selection is *fully* contained within it.
/// If any statement is only partially selected, the selection is
/// invalid for extraction.
fn selection_covers_complete_statements(content: &str, start: usize, end: usize) -> bool {
    crate::parser::with_parsed_program(content, "extract_function", |program, _| {
        // Find the enclosing function/method body statements.
        let body_stmts = find_enclosing_body_statements(&program.statements, start as u32);
        if body_stmts.is_empty() {
            return false;
        }

        let mut found_any = false;
        for stmt in &body_stmts {
            let span = stmt.span();
            let stmt_start = span.start.offset as usize;
            let stmt_end = span.end.offset as usize;

            // Statement fully outside the selection — fine, skip it.
            if stmt_end <= start || stmt_start >= end {
                continue;
            }

            // Statement overlaps the selection — it must be fully contained.
            if stmt_start < start || stmt_end > end {
                return false;
            }

            found_any = true;
        }

        found_any
    })
}

/// Collect references to top-level statements in the enclosing
/// function/method body that contains `offset`.
///
/// Returns byte ranges `(start, end)` for each direct child statement.
fn find_enclosing_body_statements<'a>(
    statements: &'a Sequence<'a, Statement<'a>>,
    offset: u32,
) -> Vec<&'a Statement<'a>> {
    for stmt in statements.iter() {
        match stmt {
            Statement::Function(func) => {
                let body_start = func.body.left_brace.start.offset;
                let body_end = func.body.right_brace.end.offset;
                if offset >= body_start && offset <= body_end {
                    return func.body.statements.iter().collect();
                }
            }
            Statement::Class(class) => {
                if let Some(block) = crate::util::find_enclosing_method_block_in_members(
                    class.members.iter(),
                    offset,
                ) {
                    return block.statements.iter().collect();
                }
            }
            Statement::Trait(tr) => {
                if let Some(block) =
                    crate::util::find_enclosing_method_block_in_members(tr.members.iter(), offset)
                {
                    return block.statements.iter().collect();
                }
            }
            Statement::Enum(en) => {
                if let Some(block) =
                    crate::util::find_enclosing_method_block_in_members(en.members.iter(), offset)
                {
                    return block.statements.iter().collect();
                }
            }
            Statement::Namespace(ns) => {
                let result = find_enclosing_body_statements(ns.statements(), offset);
                if !result.is_empty() {
                    return result;
                }
            }
            _ => {}
        }
    }
    Vec::new()
}

// ─── Context detection ──────────────────────────────────────────────────────

/// Whether the extracted code should become a method or a standalone function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtractionTarget {
    /// Extract as a private method on the enclosing class.
    Method,
    /// Extract as a standalone function after the enclosing function.
    Function,
}

/// Information about the enclosing function/method for insertion purposes.
#[derive(Debug, Clone)]
struct EnclosingContext {
    /// Whether to extract as a method or function.
    target: ExtractionTarget,
    /// Byte offset of the closing `}` of the enclosing class (for method
    /// insertion) or the enclosing function (for function insertion).
    insert_offset: usize,
    /// The body's opening `{` offset — used to determine indentation.
    body_start: usize,
    /// Whether the enclosing method is static.
    is_static: bool,
    /// The name of the enclosing function/method (e.g. `"run"`, `"process"`).
    /// Used by name generation to produce contextual names like `runGuard`.
    enclosing_name: String,
    /// Method names that already exist in the enclosing class (for
    /// deduplication when extracting a method).  Empty when extracting
    /// a standalone function.
    sibling_method_names: Vec<String>,
}

/// Determine the extraction target and insertion point by walking the AST.
fn find_enclosing_context(content: &str, offset: u32, uses_this: bool) -> Option<EnclosingContext> {
    crate::parser::with_parsed_program(content, "extract_function", |program, content| {
        let ctx = find_cursor_context(&program.statements, offset);

        match ctx {
            CursorContext::InClassLike {
                member,
                all_members,
                ..
            } => {
                if let MemberContext::Method(method, true) = member {
                    let is_static = method.modifiers.iter().any(|m| m.is_static());
                    let enclosing_name = bytes_to_str(method.name.value).to_string();

                    // Collect sibling method names for scoped deduplication.
                    let sibling_method_names: Vec<String> = all_members
                        .iter()
                        .filter_map(|m| {
                            if let ClassLikeMember::Method(m) = m {
                                Some(bytes_to_str(m.name.value).to_string())
                            } else {
                                None
                            }
                        })
                        .collect();

                    // For method extraction, insert before the closing `}` of
                    // the class.  Find the class closing brace by walking up
                    // from the method.
                    let class_end = find_class_end_offset(&program.statements, offset);

                    if let MethodBody::Concrete(block) = &method.body {
                        let body_start = block.left_brace.start.offset as usize;

                        if uses_this && is_static {
                            // $this in a static method — can't extract as method.
                            // Fall back to extracting as a function.
                            let func_end = block.right_brace.end.offset as usize;
                            return Some(EnclosingContext {
                                target: ExtractionTarget::Function,
                                insert_offset: find_after_class_end(&program.statements, offset)
                                    .unwrap_or(func_end),
                                body_start,
                                is_static,
                                enclosing_name,
                                sibling_method_names: Vec::new(),
                            });
                        }

                        return Some(EnclosingContext {
                            target: ExtractionTarget::Method,
                            insert_offset: class_end
                                .unwrap_or(block.right_brace.end.offset as usize),
                            body_start,
                            is_static,
                            enclosing_name,
                            sibling_method_names,
                        });
                    }
                }
                None
            }
            CursorContext::InFunction(func, true) => {
                let body_start = func.body.left_brace.start.offset as usize;
                let func_end = func.body.right_brace.end.offset as usize;
                let enclosing_name = bytes_to_str(func.name.value).to_string();

                // For function extraction, insert after the enclosing function.
                // Find the end of the line containing the closing `}`.
                let insert_offset = find_line_end(content, func_end);

                Some(EnclosingContext {
                    target: ExtractionTarget::Function,
                    insert_offset,
                    body_start,
                    is_static: false,
                    enclosing_name,
                    sibling_method_names: Vec::new(),
                })
            }
            _ => None,
        }
    })
}

/// Find the byte offset of the closing `}` of the class containing `offset`.
fn find_class_end_offset(statements: &Sequence<'_, Statement<'_>>, offset: u32) -> Option<usize> {
    for stmt in statements.iter() {
        match stmt {
            Statement::Class(class) => {
                let span = class.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(class.right_brace.start.offset as usize);
                }
            }
            Statement::Trait(tr) => {
                let span = tr.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(tr.right_brace.start.offset as usize);
                }
            }
            Statement::Enum(en) => {
                let span = en.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(en.right_brace.start.offset as usize);
                }
            }
            Statement::Namespace(ns) => {
                if let Some(offset) = find_class_end_offset(ns.statements(), offset) {
                    return Some(offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the byte offset after the closing `}` of the class containing `offset`.
fn find_after_class_end(statements: &Sequence<'_, Statement<'_>>, offset: u32) -> Option<usize> {
    for stmt in statements.iter() {
        match stmt {
            Statement::Class(class) => {
                let span = class.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(span.end.offset as usize);
                }
            }
            Statement::Trait(tr) => {
                let span = tr.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(span.end.offset as usize);
                }
            }
            Statement::Enum(en) => {
                let span = en.span();
                if offset >= span.start.offset && offset <= span.end.offset {
                    return Some(span.end.offset as usize);
                }
            }
            Statement::Namespace(ns) => {
                if let Some(end) = find_after_class_end(ns.statements(), offset) {
                    return Some(end);
                }
            }
            _ => {}
        }
    }
    None
}

// ─── Scope map building ─────────────────────────────────────────────────────

/// Build a `ScopeMap` for the enclosing function/method at `offset`.
fn build_scope_map(content: &str, offset: u32) -> ScopeMap {
    crate::parser::with_parsed_program(content, "extract_function", |program, content| {
        crate::scope_collector::build_scope_map_for_offset(
            program.statements.as_slice(),
            offset,
            content.len() as u32,
        )
    })
}

// ─── Type resolution ────────────────────────────────────────────────────────

/// Resolve the type of a variable at a given offset using the hover
/// pipeline.
fn resolve_var_type(
    backend: &Backend,
    var_name: &str,
    content: &str,
    cursor_offset: u32,
    uri: &str,
) -> Option<PhpType> {
    let ctx = backend.file_context(uri);
    let class_loader = backend.class_loader(&ctx);
    let function_loader = backend.function_loader(&ctx);
    let constant_loader = backend.constant_loader();
    let loaders = Loaders {
        function_loader: Some(
            &function_loader as &dyn Fn(&str) -> Option<crate::types::FunctionInfo>,
        ),
        constant_loader: Some(&constant_loader),
    };

    let current_class = find_class_at_offset(&ctx.classes, cursor_offset);

    crate::completion::variable::resolution::resolve_variable_php_type(
        var_name,
        content,
        cursor_offset,
        current_class,
        &ctx.classes,
        &class_loader,
        loaders,
    )
}

// ─── Name generation ────────────────────────────────────────────────────────

/// Generate a unique function/method name that doesn't conflict with
/// existing members or functions.
/// Context passed to [`generate_function_name`] to produce meaningful names.
struct NamingContext<'a> {
    /// The enclosing function/method name (e.g. `"run"`, `"process"`).
    enclosing_name: &'a str,
    /// The return strategy chosen for the extraction.
    return_strategy: &'a ReturnStrategy,
    /// The selected body text (trimmed source of the extracted statements).
    body_text: &'a str,
    /// Names of return-value variables (written inside, read after).
    return_var_names: &'a [String],
    /// The trailing return type hint (e.g. `Collection`, `User`).
    trailing_return_type: &'a PhpType,
}

/// Generate a contextual name for the extracted function/method.
///
/// The naming follows these heuristics (first match wins):
///
/// 1. **Guard strategies** (`VoidGuards`, `UniformGuards`,
///    `NullGuardWithValue`): `{enclosing}Guard` — the user extracted
///    validation / precondition logic.
/// 2. **`SentinelNull`**: `try{Enclosing}` — a "try" pattern where
///    `null` signals failure.
/// 3. **`TrailingReturn` with `new ClassName`** in the body:
///    `create{ClassName}` — a factory pattern.
/// 4. **`TrailingReturn`** (other): `get{Enclosing}Result`.
/// 5. **Body is pure output** (every statement is `echo`/`print`/
///    `printf`/`var_dump`): `render{Enclosing}`.
/// 6. **Single return variable**: `compute{VarName}` — the user
///    extracted a calculation into its own function.
/// 7. **Body ends with output** (setup assignments followed by
///    `echo`/`print`): `render{Enclosing}`.
/// 8. **Single delegating call** (`$this->foo(…)`, `doWork(…)`):
///    the name of the called method/function.
/// 9. **Fallback**: `"extracted"`.
///
/// After choosing a base name, the function deduplicates against
/// existing names in the appropriate scope (class members for methods,
/// file-level `function` declarations for standalone functions).
fn generate_function_name(
    content: &str,
    enclosing_ctx: &EnclosingContext,
    naming: &NamingContext,
) -> String {
    let base = derive_base_name(naming);

    // Deduplicate against the right scope.
    deduplicate_name(&base, content, enclosing_ctx)
}

/// Pick a base name from the naming context (before deduplication).
fn derive_base_name(ctx: &NamingContext) -> String {
    let enc = ctx.enclosing_name;

    // 1. Guard strategies → {enclosing}Guard
    match ctx.return_strategy {
        ReturnStrategy::VoidGuards
        | ReturnStrategy::UniformGuards(_)
        | ReturnStrategy::NullGuardWithValue(_) => {
            if !enc.is_empty() {
                return format!("{}Guard", enc);
            }
            return "guard".to_string();
        }

        // 2. SentinelNull → try{Enclosing}
        ReturnStrategy::SentinelNull => {
            if !enc.is_empty() {
                return format!("try{}", capitalise(enc));
            }
            return "tryExtract".to_string();
        }

        // 3–4. TrailingReturn
        ReturnStrategy::TrailingReturn => {
            // 3. Factory: body contains `new ClassName` → create{ClassName}
            if let Some(class_name) = detect_factory_pattern(ctx.body_text) {
                return format!("create{}", class_name);
            }
            // 4. Generic trailing return
            if !enc.is_empty() {
                // If there's a return type, use it for a more descriptive name
                if !ctx.trailing_return_type.is_empty() {
                    // Only use the return type if it's a class name (starts uppercase)
                    if let Some(name) = ctx.trailing_return_type.base_name() {
                        return format!("get{}", name);
                    }
                }
                return format!("get{}Result", capitalise(enc));
            }
        }

        ReturnStrategy::None | ReturnStrategy::Unsafe => {}
    }

    // 5. Pure output → render{Enclosing}
    if is_pure_output(ctx.body_text) && !enc.is_empty() {
        return format!("render{}", capitalise(enc));
    }

    // 6. Single return variable → compute{VarName}
    if ctx.return_var_names.len() == 1 {
        let var = ctx.return_var_names[0].trim_start_matches('$');
        if !var.is_empty() {
            return format!("compute{}", capitalise(var));
        }
    }

    // 7. Ends with output (setup + echo/print) → render{Enclosing}
    if ends_with_output(ctx.body_text) && !enc.is_empty() {
        return format!("render{}", capitalise(enc));
    }

    // 8. Single method/function call → {calledName}
    if let Some(name) = detect_single_call(ctx.body_text)
        && !name.is_empty()
    {
        return name;
    }

    // 9. Fallback
    "extracted".to_string()
}

/// Capitalise the first character of a string (ASCII).
fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            format!("{}{}", upper, chars.as_str())
        }
        None => String::new(),
    }
}

/// Detect if the body text is a factory pattern: the extracted code
/// constructs an object and returns it.
///
/// Returns a name suitable for `create{Name}`.
///
/// When the body assigns `$var = new X(…)` and later returns `$var`,
/// the variable name is used (e.g. `$users` → `"Users"`).  This
/// produces `createUsers` rather than `createCollection`, which
/// matches how developers think about the domain object.
///
/// When the body does `return new ClassName(…)` directly, the class
/// name is used instead (there is no variable to take a hint from).
fn detect_factory_pattern(body: &str) -> Option<String> {
    let mut returned_class: Option<String> = None;
    let mut returned_var: Option<String> = None;
    let mut assigned_var: Option<String> = None;
    let mut assigned_class: Option<String> = None;

    for line in body.lines() {
        let trimmed = line.trim();
        // Check for `return new ClassName(…)` — direct return.
        if let Some(after_return) = trimmed.strip_prefix("return ")
            && let Some(name) = extract_new_class_name(after_return.trim_start())
        {
            returned_class = Some(name);
        }
        // Check for `return $var;` — returning a variable.
        if let Some(after_return) = trimmed.strip_prefix("return ") {
            let var = after_return.trim().trim_end_matches(';').trim();
            if var.starts_with('$') && var[1..].chars().all(|c| c.is_alphanumeric() || c == '_') {
                returned_var = Some(var.to_string());
            }
        }
        // Check for `$var = new ClassName(…)` (direct assignment).
        if let Some(eq_pos) = trimmed.find('=') {
            // Make sure it's `=` not `==` / `===` / `!=` etc.
            let before_eq = &trimmed[..eq_pos];
            let after_eq = &trimmed[eq_pos + 1..];
            let var_name = before_eq.trim();
            if var_name.starts_with('$')
                && !after_eq.starts_with('=')
                && !before_eq.ends_with('!')
                && !before_eq.ends_with('<')
                && !before_eq.ends_with('>')
                && let Some(class_name) = extract_new_class_name(after_eq.trim_start())
            {
                assigned_var = Some(var_name.to_string());
                assigned_class = Some(class_name);
            }
        }
    }

    // Best case: `$var = new X(…); ... return $var;` — use the
    // variable name because it carries domain meaning (e.g. `$users`
    // → `createUsers`).  Fall back to the class name when the variable
    // is too short to be meaningful (`$u`, `$x`, etc.).
    if let Some(ref ret_var) = returned_var
        && let Some(ref asgn_var) = assigned_var
        && ret_var == asgn_var
    {
        let var_clean = ret_var.trim_start_matches('$');
        if var_clean.len() > 2 {
            return Some(capitalise(var_clean));
        }
        // Short variable — prefer the class name.
        if let Some(ref name) = assigned_class {
            let short = name.rsplit('\\').next().unwrap_or(name);
            return Some(short.to_string());
        }
    }

    // `return new ClassName(…)` — use the class name.
    if let Some(name) = returned_class {
        let short = name.rsplit('\\').next().unwrap_or(&name);
        return Some(short.to_string());
    }

    // `$var = new ClassName(…)` without an explicit return — use the
    // variable name if long enough, otherwise the class name.
    if let Some(ref var) = assigned_var {
        let var_clean = var.trim_start_matches('$');
        if var_clean.len() > 2 {
            return Some(capitalise(var_clean));
        }
    }
    if let Some(name) = assigned_class {
        let short = name.rsplit('\\').next().unwrap_or(&name);
        return Some(short.to_string());
    }

    None
}

/// Extract a class name from text starting with `new ClassName`.
///
/// Returns `None` if the text doesn't start with `new ` followed by
/// an uppercase identifier.
fn extract_new_class_name(text: &str) -> Option<String> {
    let rest = text.strip_prefix("new ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '\\')
        .collect();
    if !name.is_empty() && name.starts_with(|c: char| c.is_ascii_uppercase()) {
        Some(name)
    } else {
        None
    }
}

/// Output-statement prefixes shared by the pure/trailing output checks.
const OUTPUT_PREFIXES: &[&str] = &[
    "echo ",
    "echo(",
    "echo \"",
    "echo '",
    "print ",
    "print(",
    "printf(",
    "var_dump(",
    "print_r(",
    "var_export(",
];

/// Returns `true` when `line` (trimmed, without trailing `;`) looks
/// like an output statement.
fn is_output_line(line: &str) -> bool {
    OUTPUT_PREFIXES.iter().any(|p| line.starts_with(p))
}

/// Check whether every statement in the body is a pure output statement
/// (`echo`, `print`, `printf`, `var_dump`, `print_r`, `var_export`).
fn is_pure_output(body: &str) -> bool {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return false;
    }

    for line in trimmed.lines() {
        let line = line.trim().trim_end_matches(';').trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        if !is_output_line(line) {
            return false;
        }
    }

    true
}

/// Check whether the body *ends* with one or more output statements
/// but also contains non-output setup lines (assignments, calls, etc.).
///
/// This catches the common "compute then display" pattern:
/// ```php
/// $first = $users->first();
/// echo $first->name;
/// ```
fn ends_with_output(body: &str) -> bool {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lines: Vec<&str> = trimmed
        .lines()
        .map(|l| l.trim().trim_end_matches(';').trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with('#'))
        .collect();

    if lines.len() < 2 {
        return false;
    }

    // The last line must be output.
    if !is_output_line(lines[lines.len() - 1]) {
        return false;
    }

    // At least one earlier line must NOT be output (otherwise
    // `is_pure_output` already matched).
    lines[..lines.len() - 1].iter().any(|l| !is_output_line(l))
}

/// Detect when the body is a single method call or function call
/// statement (no assignment, no return).  Returns a name derived from
/// the called method/function.
///
/// Examples:
/// - `$this->execute($fn)` → `"execute"`
/// - `self::validate($x)`  → `"validate"`
/// - `doSomething($x)`     → `"doSomething"`
fn detect_single_call(body: &str) -> Option<String> {
    let trimmed = body.trim();

    // Must be a single non-comment line.
    let lines: Vec<&str> = trimmed
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with('#'))
        .collect();
    if lines.len() != 1 {
        return None;
    }

    let line = lines[0].strip_suffix(';').unwrap_or(lines[0]).trim();

    // Must not be an assignment.
    if line.contains('=') {
        // Allow `==`, `!=`, `===`, `!==`, `>=`, `<=` inside expressions,
        // but reject bare `$var = ...` assignments.
        if let Some(eq_pos) = line.find('=') {
            let before = &line[..eq_pos];
            let after = &line[eq_pos + 1..];
            if before.trim().starts_with('$')
                && !after.starts_with('=')
                && !before.ends_with('!')
                && !before.ends_with('<')
                && !before.ends_with('>')
            {
                return None;
            }
        }
    }

    // Extract the method/function name from the call.
    // `$this->foo(...)` or `$var->foo(...)`
    if let Some(arrow_pos) = line.rfind("->") {
        let after = &line[arrow_pos + 2..];
        let name: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() && after[name.len()..].starts_with('(') {
            return Some(name);
        }
    }
    // `self::foo(...)` or `static::foo(...)` or `ClassName::foo(...)`
    if let Some(colon_pos) = line.rfind("::") {
        let after = &line[colon_pos + 2..];
        let name: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() && after[name.len()..].starts_with('(') {
            return Some(name);
        }
    }
    // `functionName(...)` — bare function call
    let name: String = line
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '\\')
        .collect();
    if !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c == '\\')
        && line[name.len()..].starts_with('(')
    {
        // Use the short name (after last backslash).
        let short = name.rsplit('\\').next().unwrap_or(&name);
        return Some(short.to_string());
    }

    None
}

/// Deduplicate a base name against existing names in the appropriate scope.
///
/// For methods, checks against sibling method names in the class.
/// For functions, checks against `function <name>` patterns in the file.
fn deduplicate_name(base: &str, content: &str, ctx: &EnclosingContext) -> String {
    let mut name = base.to_string();
    let mut counter = 1u32;

    match ctx.target {
        ExtractionTarget::Method => {
            // Check against sibling method names in the class.
            loop {
                if !ctx.sibling_method_names.contains(&name) {
                    break;
                }
                counter += 1;
                name = format!("{}{}", base, counter);
            }
        }
        ExtractionTarget::Function => {
            // Check against function declarations in the file.
            loop {
                let pattern_fn = format!("function {}", name);
                if !content.contains(&pattern_fn) {
                    break;
                }
                counter += 1;
                name = format!("{}{}", base, counter);
            }
        }
    }

    name
}

// ─── Selection trimming ────────────────────────────────────────────────────

/// Trim the selection to exclude leading/trailing whitespace and ensure
/// it starts/ends on statement boundaries.
///
/// Returns `(trimmed_start, trimmed_end)` or `None` if the trimmed
/// selection is empty.
fn trim_selection(content: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    if start >= end || end > content.len() {
        return None;
    }

    let selected = &content[start..end];
    let trimmed = selected.trim();
    if trimmed.is_empty() {
        return None;
    }

    let trim_start = start + (selected.len() - selected.trim_start().len());
    let trim_end = end - (selected.len() - selected.trim_end().len());

    if trim_start >= trim_end {
        return None;
    }

    Some((trim_start, trim_end))
}

// ─── Indentation helpers ────────────────────────────────────────────────────

/// Detect the indentation of the line containing the given offset.
///
/// Returns only the leading whitespace of that line, without adding
/// an extra indent level.
fn detect_line_indent(content: &str, offset: usize) -> String {
    let before = &content[..offset];
    let line_start = before.rfind('\n').map_or(0, |p| p + 1);
    let line = &content[line_start..offset];
    line.chars().take_while(|c| c.is_whitespace()).collect()
}

/// Detect whether the file uses tabs or spaces (and how many spaces).
fn detect_indent_unit(content: &str) -> &str {
    for line in content.lines() {
        if line.starts_with('\t') {
            return "\t";
        }
        let spaces: usize = line.chars().take_while(|c| *c == ' ').count();
        if spaces >= 2 {
            if spaces.is_multiple_of(4) {
                return "    ";
            }
            return "  ";
        }
    }
    "    "
}

/// Find the end of the line containing `offset` (after the `\n`).
fn find_line_end(content: &str, offset: usize) -> usize {
    match content[offset..].find('\n') {
        Some(pos) => offset + pos + 1,
        None => content.len(),
    }
}

/// Find the start of the line containing `offset`.
fn find_line_start(content: &str, offset: usize) -> usize {
    content[..offset].rfind('\n').map_or(0, |p| p + 1)
}

/// Extract the indentation (leading whitespace) of the line at `offset`.
fn indent_at(content: &str, offset: usize) -> String {
    let line_start = find_line_start(content, offset);
    let rest = &content[line_start..];
    rest.chars().take_while(|c| c.is_whitespace()).collect()
}

// ─── Code generation ────────────────────────────────────────────────────────

/// Information gathered for code generation.
struct ExtractionInfo {
    /// The name of the new function/method.
    name: String,
    /// Parameters: `(var_name_with_dollar, cleaned_type_hint)`.
    params: Vec<(String, PhpType)>,
    /// Return values: `(var_name_with_dollar, cleaned_type_hint)`.
    returns: Vec<(String, PhpType)>,
    /// The selected statements as source text.
    body: String,
    /// Whether to extract as method or function.
    target: ExtractionTarget,
    /// Whether the enclosing method is static.
    is_static: bool,
    /// Indentation of the member level (for methods) or top level (for functions).
    member_indent: String,
    /// Indentation of the body inside the new function/method.
    body_indent: String,
    /// How return statements in the selection are handled.
    return_strategy: ReturnStrategy,
    /// Return type hint for the trailing return (resolved from the
    /// enclosing function's return type or the return expression).
    trailing_return_type: PhpType,
    /// Pre-computed PHPDoc block (including `/**` … `*/\n`) to prepend
    /// before the function definition, or empty if no enrichment needed.
    docblock: String,
}

/// Build a PHPDoc block for the extracted function when types need enrichment.
///
/// Each parameter is a triple `(var_name, cleaned_type, raw_type)` where
/// `cleaned_type` is the native PHP hint (generics stripped) and
/// `raw_type` is the full resolved type as a [`PhpType`] (e.g.
/// `Collection<User>`).
///
/// When `raw_type` already contains concrete generic arguments,
/// it is used verbatim as the docblock type.  Otherwise we fall back to
/// `enrichment_plain` which reconstructs template parameters from the
/// class definition (yielding placeholder names like `T`).
///
/// A `@return` tag follows the same logic: if `raw_return_type` carries
/// concrete generics, use it; otherwise try enrichment.
///
/// Returns an empty string when no enrichment is needed.
fn build_docblock_for_extraction(
    params: &[(String, PhpType, PhpType)],
    return_type_hint: &PhpType,
    raw_return_type: &PhpType,
    member_indent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let mut tags: Vec<String> = Vec::new();

    // Collect @param tags that need enrichment.
    for (name, type_hint, raw) in params {
        let has_native_hint = type_hint.to_native_hint().is_some_and(|s| !s.is_empty());
        if !has_native_hint && raw.is_empty() {
            continue;
        }
        // Prefer the raw resolved type when it carries concrete generics.
        if raw.has_type_structure() {
            tags.push(format!("@param {} {}", raw, name));
            continue;
        }
        let type_for_enrichment = if has_native_hint { type_hint } else { raw };
        if let Some(enriched) = enrichment_plain(Some(type_for_enrichment), class_loader) {
            tags.push(format!("@param {} {}", enriched, name));
        }
    }

    // Collect @return tag if the return type needs enrichment.
    if !return_type_hint.is_empty() || !raw_return_type.is_empty() {
        if raw_return_type.has_type_structure() {
            tags.push(format!("@return {}", raw_return_type));
        } else {
            let hint = if return_type_hint.is_empty() {
                raw_return_type
            } else {
                return_type_hint
            };
            if let Some(enriched) = enrichment_plain(Some(hint), class_loader) {
                tags.push(format!("@return {}", enriched));
            }
        }
    }

    if tags.is_empty() {
        return String::new();
    }

    // Align @param tag types for readability.
    // Find the max type width among @param tags.
    let param_tags: Vec<(&str, &str)> = tags
        .iter()
        .filter_map(|t| {
            let rest = t.strip_prefix("@param ")?;
            // Split on `$` — PHP param names always start with `$`,
            // and the type string may contain spaces (e.g. `(Closure(): mixed)`).
            let dollar_pos = rest.find('$')?;
            let type_str = rest[..dollar_pos].trim_end();
            let name_str = &rest[dollar_pos..];
            Some((type_str, name_str))
        })
        .collect();

    let max_type_len = param_tags.iter().map(|(t, _)| t.len()).max().unwrap_or(0);

    let mut out = String::new();
    out.push_str(member_indent);
    out.push_str("/**\n");

    for tag in &tags {
        out.push_str(member_indent);
        out.push_str(" * ");
        if let Some(rest) = tag.strip_prefix("@param ") {
            if let Some(dollar_pos) = rest.find('$') {
                let type_str = rest[..dollar_pos].trim_end();
                let name_str = &rest[dollar_pos..];
                out.push_str("@param ");
                out.push_str(type_str);
                // Pad to align parameter names.
                for _ in 0..(max_type_len.saturating_sub(type_str.len())) {
                    out.push(' ');
                }
                out.push(' ');
                out.push_str(name_str);
            } else {
                out.push_str(tag);
            }
        } else {
            out.push_str(tag);
        }
        out.push('\n');
    }

    out.push_str(member_indent);
    out.push_str(" */\n");

    out
}

/// Build the definition text of the extracted function or method.
fn build_extracted_definition(info: &ExtractionInfo) -> String {
    let mut out = String::new();

    // Blank line before the new definition.
    out.push('\n');

    // Prepend PHPDoc block if types need enrichment.
    if !info.docblock.is_empty() {
        out.push_str(&info.docblock);
    }

    let param_list = build_param_list(&info.params);
    let return_type = build_return_type(info);

    match info.target {
        ExtractionTarget::Method => {
            out.push_str(&info.member_indent);
            out.push_str("private ");
            if info.is_static {
                out.push_str("static ");
            }
            out.push_str("function ");
            out.push_str(&info.name);
            out.push('(');
            out.push_str(&param_list);
            out.push(')');
            if !return_type.is_empty() {
                out.push_str(": ");
                out.push_str(&return_type);
            }
            out.push('\n');
            out.push_str(&info.member_indent);
            out.push_str("{\n");
        }
        ExtractionTarget::Function => {
            out.push_str(&info.member_indent);
            out.push_str("function ");
            out.push_str(&info.name);
            out.push('(');
            out.push_str(&param_list);
            out.push(')');
            if !return_type.is_empty() {
                out.push_str(": ");
                out.push_str(&return_type);
            }
            out.push('\n');
            out.push_str(&info.member_indent);
            out.push_str("{\n");
        }
    }

    // Rewrite guard returns in the body if needed.
    let body_text = match &info.return_strategy {
        ReturnStrategy::VoidGuards => {
            // Bare `return;` → `return false;` (false = early exit).
            rewrite_guard_returns(&info.body, None)
        }
        ReturnStrategy::UniformGuards(value) => {
            let lower = value.to_lowercase();
            if lower == "false" || lower == "true" {
                // Already boolean — the body's returns are correct as-is.
                info.body.clone()
            } else {
                // Non-boolean uniform value (e.g. `null`, `0`, `'error'`):
                // rewrite `return <value>;` → `return false;`.
                rewrite_guard_returns(&info.body, Some(value))
            }
        }
        ReturnStrategy::NullGuardWithValue(void_guards) if *void_guards => {
            // Bare `return;` → `return null;` so the extracted
            // function returns null on guard-fire.
            rewrite_void_returns_to_null(&info.body)
        }
        _ => info.body.clone(),
    };

    // Re-indent the body to match the new function's body indentation.
    let body_lines = body_text.lines().collect::<Vec<_>>();
    let min_indent = body_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    for line in &body_lines {
        if line.trim().is_empty() {
            out.push('\n');
        } else {
            out.push_str(&info.body_indent);
            if line.len() > min_indent {
                out.push_str(&line[min_indent..]);
            }
            out.push('\n');
        }
    }

    // Add return/sentinel after the body based on the strategy.
    match &info.return_strategy {
        ReturnStrategy::TrailingReturn => {
            // Body already ends with `return` — nothing to add.
        }
        ReturnStrategy::VoidGuards => {
            // All guards are bare `return;`.  Add `return true;` as the
            // fall-through (meaning "no early exit, keep going").
            out.push_str(&info.body_indent);
            out.push_str("return true;\n");
        }
        ReturnStrategy::UniformGuards(value) => {
            // All guards return the same value.  The extracted function
            // uses bool: guards become `return false;` (exit), and
            // fall-through is `return true;` (continue).
            // But the body already has the original returns — we need
            // to add the sentinel.  The body's returns stay as-is and
            // get rewritten below by `rewrite_guard_returns_to_bool`.
            // Here we just add the fall-through sentinel.
            let lower = value.to_lowercase();
            let sentinel = if lower == "false" {
                "true"
            } else if lower == "true" {
                "false"
            } else {
                // Non-boolean uniform value: use `true` = continue.
                "true"
            };
            out.push_str(&info.body_indent);
            out.push_str("return ");
            out.push_str(sentinel);
            out.push_str(";\n");
        }
        ReturnStrategy::SentinelNull => {
            // Different non-null values — null = "no early exit".
            out.push_str(&info.body_indent);
            out.push_str("return null;\n");
        }
        ReturnStrategy::NullGuardWithValue(_) => {
            // Guards return null (or were rewritten from bare return;),
            // and we also compute a value.  The fall-through returns
            // the computed variable.
            if info.returns.len() == 1 {
                out.push_str(&info.body_indent);
                out.push_str("return ");
                out.push_str(&info.returns[0].0);
                out.push_str(";\n");
            }
        }
        ReturnStrategy::None | ReturnStrategy::Unsafe => {
            // Normal extraction: add return for captured variables.
            if info.returns.len() == 1 {
                out.push_str(&info.body_indent);
                out.push_str("return ");
                out.push_str(&info.returns[0].0);
                out.push_str(";\n");
            } else if info.returns.len() > 1 {
                out.push_str(&info.body_indent);
                out.push_str("return [");
                let names: Vec<&str> = info.returns.iter().map(|(n, _)| n.as_str()).collect();
                out.push_str(&names.join(", "));
                out.push_str("];\n");
            }
        }
    }

    out.push_str(&info.member_indent);
    out.push_str("}\n");

    out
}

/// Rewrite guard-clause return statements in the body text.
///
/// For `VoidGuards` (`uniform_value` is `None`): bare `return;` becomes
/// `return false;`.
///
/// For `UniformGuards` with a non-boolean value (`uniform_value` is
/// `Some`): `return <value>;` becomes `return false;`.
///
/// This operates on source text rather than AST to keep things simple.
/// It matches `return` followed by optional whitespace and either `;`
/// (void) or the uniform value and `;`.
///
/// See also [`rewrite_void_returns_to_null`] for the
/// `NullGuardWithValue(true)` case.
fn rewrite_guard_returns(body: &str, uniform_value: Option<&str>) -> String {
    match uniform_value {
        None => {
            // VoidGuards: rewrite bare `return;` to `return false;`.
            // We need to be careful not to match `return $x;` etc.
            // Strategy: find `return` followed by optional whitespace
            // then `;`, with no expression in between.
            let mut result = String::with_capacity(body.len());
            let mut remaining = body;
            while let Some(pos) = remaining.find("return") {
                // Check that this is a keyword boundary (not part of
                // `$returnValue` etc.).
                let before_ok = pos == 0
                    || !remaining.as_bytes()[pos - 1].is_ascii_alphanumeric()
                        && remaining.as_bytes()[pos - 1] != b'_'
                        && remaining.as_bytes()[pos - 1] != b'$';
                if !before_ok {
                    result.push_str(&remaining[..pos + 6]);
                    remaining = &remaining[pos + 6..];
                    continue;
                }
                let after = &remaining[pos + 6..];
                let trimmed = after.trim_start();
                if trimmed.starts_with(';') {
                    // Bare `return;` → `return false;`
                    result.push_str(&remaining[..pos]);
                    result.push_str("return false");
                    // Skip past `return` + whitespace, keep the `;`.
                    let ws_len = after.len() - trimmed.len();
                    remaining = &remaining[pos + 6 + ws_len..];
                } else {
                    result.push_str(&remaining[..pos + 6]);
                    remaining = &remaining[pos + 6..];
                }
            }
            result.push_str(remaining);
            result
        }
        Some(value) => {
            // UniformGuards with non-boolean value: rewrite
            // `return <value>;` to `return false;`.
            let mut result = String::with_capacity(body.len());
            let mut remaining = body;
            while let Some(pos) = remaining.find("return") {
                let before_ok = pos == 0
                    || !remaining.as_bytes()[pos - 1].is_ascii_alphanumeric()
                        && remaining.as_bytes()[pos - 1] != b'_'
                        && remaining.as_bytes()[pos - 1] != b'$';
                if !before_ok {
                    result.push_str(&remaining[..pos + 6]);
                    remaining = &remaining[pos + 6..];
                    continue;
                }
                let after = &remaining[pos + 6..];
                let trimmed = after.trim_start();
                // Check if the return expression matches the uniform
                // value (case-insensitive for keywords like `null`).
                let value_trimmed = value.trim();
                if trimmed.len() >= value_trimmed.len() {
                    let candidate = &trimmed[..value_trimmed.len()];
                    let after_value = trimmed[value_trimmed.len()..].trim_start();
                    if candidate.eq_ignore_ascii_case(value_trimmed) && after_value.starts_with(';')
                    {
                        // `return <value>;` → `return false;`
                        result.push_str(&remaining[..pos]);
                        result.push_str("return false");
                        // Skip past `return <ws> <value> <ws>`, keep `;`.
                        let consumed = (trimmed.as_ptr() as usize - after.as_ptr() as usize)
                            + value_trimmed.len()
                            + (after_value.as_ptr() as usize
                                - trimmed[value_trimmed.len()..].as_ptr() as usize);
                        remaining = &remaining[pos + 6 + consumed..];
                        continue;
                    }
                }
                result.push_str(&remaining[..pos + 6]);
                remaining = &remaining[pos + 6..];
            }
            result.push_str(remaining);
            result
        }
    }
}

/// Rewrite bare `return;` to `return null;` in the body text.
///
/// Used by `NullGuardWithValue(true)` — void guard clauses that are
/// extracted alongside a computed value.  The extracted function must
/// return `null` (not void) to signal "guard fired" to the caller.
fn rewrite_void_returns_to_null(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    let mut remaining = body;
    while let Some(pos) = remaining.find("return") {
        let before_ok = pos == 0
            || !remaining.as_bytes()[pos - 1].is_ascii_alphanumeric()
                && remaining.as_bytes()[pos - 1] != b'_'
                && remaining.as_bytes()[pos - 1] != b'$';
        if !before_ok {
            result.push_str(&remaining[..pos + 6]);
            remaining = &remaining[pos + 6..];
            continue;
        }
        let after = &remaining[pos + 6..];
        let trimmed = after.trim_start();
        if trimmed.starts_with(';') {
            // Bare `return;` → `return null;`
            result.push_str(&remaining[..pos]);
            result.push_str("return null");
            let ws_len = after.len() - trimmed.len();
            remaining = &remaining[pos + 6 + ws_len..];
        } else {
            result.push_str(&remaining[..pos + 6]);
            remaining = &remaining[pos + 6..];
        }
    }
    result.push_str(remaining);
    result
}

/// Build the parameter list string for the function signature.
fn build_param_list(params: &[(String, PhpType)]) -> String {
    params
        .iter()
        .map(|(name, type_hint)| {
            let hint_str = type_hint.to_native_hint().unwrap_or_default();
            if hint_str.is_empty() {
                name.clone()
            } else {
                format!("{} {}", hint_str, name)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build the return type annotation string.
fn build_return_type(info: &ExtractionInfo) -> String {
    match &info.return_strategy {
        ReturnStrategy::TrailingReturn => {
            // Use the enclosing function's return type — already a PhpType,
            // no need to re-parse.
            if let Some(cleaned) = clean_type_for_signature_typed(&info.trailing_return_type) {
                return cleaned.to_string();
            }
            String::new()
        }
        ReturnStrategy::VoidGuards | ReturnStrategy::UniformGuards(_) => {
            // Guard strategies use bool: true = continue, false = exit.
            "bool".to_string()
        }
        ReturnStrategy::SentinelNull => {
            // Sentinel-null: the return type is nullable.  Try to
            // derive it from the trailing_return_type if available,
            // otherwise leave untyped.
            if let Some(cleaned) = clean_type_for_signature_typed(&info.trailing_return_type)
                && !cleaned.is_null()
                && !cleaned.is_mixed()
                && !matches!(cleaned, PhpType::Nullable(_))
            {
                return PhpType::Nullable(Box::new(cleaned)).to_string();
            }
            // Can't determine a useful nullable type.
            String::new()
        }
        ReturnStrategy::NullGuardWithValue(_) => {
            // The return type is the computed value's type made nullable.
            if info.returns.len() == 1 {
                let type_hint = &info.returns[0].1;
                if let Some(cleaned) = clean_type_for_signature_typed(type_hint) {
                    if !cleaned.is_null()
                        && !cleaned.is_mixed()
                        && !matches!(cleaned, PhpType::Nullable(_))
                    {
                        return PhpType::Nullable(Box::new(cleaned)).to_string();
                    }
                    // Already nullable or mixed — use as-is.
                    return cleaned.to_string();
                }
            }
            String::new()
        }
        ReturnStrategy::None | ReturnStrategy::Unsafe => {
            // Normal extraction — derive from return variables.
            if info.returns.is_empty() {
                return "void".to_string();
            }
            if info.returns.len() == 1 {
                let type_hint = &info.returns[0].1;
                let hint_str = type_hint.to_native_hint().unwrap_or_default();
                if hint_str.is_empty() {
                    return String::new();
                }
                return hint_str;
            }
            // Multiple return values → return as array.
            "array".to_string()
        }
    }
}

/// Build the call-site text that replaces the selected statements.
fn build_call_site(info: &ExtractionInfo, call_indent: &str) -> String {
    let mut out = String::new();

    let args: Vec<&str> = info.params.iter().map(|(n, _)| n.as_str()).collect();
    let arg_list = args.join(", ");

    // Build the function/method call expression.
    let call_expr = match info.target {
        ExtractionTarget::Method => {
            if info.is_static {
                format!("self::{}({})", info.name, arg_list)
            } else {
                format!("$this->{}({})", info.name, arg_list)
            }
        }
        ExtractionTarget::Function => {
            format!("{}({})", info.name, arg_list)
        }
    };

    match &info.return_strategy {
        ReturnStrategy::TrailingReturn => {
            // The body ends with `return expr;` — the call site passes
            // the return value through.
            out.push_str(call_indent);
            out.push_str("return ");
            out.push_str(&call_expr);
            out.push_str(";\n");
        }
        ReturnStrategy::VoidGuards => {
            // Extracted function returns bool (true = continue).
            // Call site: `if (!extracted(…)) return;`
            out.push_str(call_indent);
            out.push_str("if (!");
            out.push_str(&call_expr);
            out.push_str(") return;\n");
        }
        ReturnStrategy::UniformGuards(value) => {
            // Extracted function returns bool (true = continue).
            // Call site: `if (!extracted(…)) return <value>;`
            out.push_str(call_indent);
            out.push_str("if (!");
            out.push_str(&call_expr);
            out.push_str(") return ");
            out.push_str(value);
            out.push_str(";\n");
        }
        ReturnStrategy::SentinelNull => {
            // Extracted function returns null on fall-through, or the
            // actual value on early exit.
            // Call site:
            //   $result = extracted(…);
            //   if ($result !== null) return $result;
            out.push_str(call_indent);
            out.push_str("$result = ");
            out.push_str(&call_expr);
            out.push_str(";\n");
            out.push_str(call_indent);
            out.push_str("if ($result !== null) return $result;\n");
        }
        ReturnStrategy::NullGuardWithValue(void_guards) => {
            // Guards return null (or were void), the function also
            // computes a value.
            // Call site:
            //   $var = extracted(…);
            //   if ($var === null) return null;  // or `return;`
            if info.returns.len() == 1 {
                out.push_str(call_indent);
                out.push_str(&info.returns[0].0);
                out.push_str(" = ");
                out.push_str(&call_expr);
                out.push_str(";\n");
                out.push_str(call_indent);
                out.push_str("if (");
                out.push_str(&info.returns[0].0);
                if *void_guards {
                    out.push_str(" === null) return;\n");
                } else {
                    out.push_str(" === null) return null;\n");
                }
            }
        }
        ReturnStrategy::None | ReturnStrategy::Unsafe => {
            // Normal extraction.
            if info.returns.is_empty() {
                // No return values — just call the function.
                out.push_str(call_indent);
                out.push_str(&call_expr);
                out.push_str(";\n");
            } else if info.returns.len() == 1 {
                // Single return value — assign it.
                out.push_str(call_indent);
                out.push_str(&info.returns[0].0);
                out.push_str(" = ");
                out.push_str(&call_expr);
                out.push_str(";\n");
            } else {
                // Multiple return values — destructure from array.
                let vars: Vec<&str> = info.returns.iter().map(|(n, _)| n.as_str()).collect();
                out.push_str(call_indent);
                out.push('[');
                out.push_str(&vars.join(", "));
                out.push_str("] = ");
                out.push_str(&call_expr);
                out.push_str(";\n");
            }
        }
    }

    out
}

// ─── Return statement analysis ──────────────────────────────────────────────

/// Analyse `return` statements within the selected range and determine
/// the extraction strategy.
///
/// The returned `ReturnStrategy` tells the code generator how to handle
/// early returns in the extracted code:
/// - `None` — no returns in the selection.
/// - `TrailingReturn` — last statement is `return`, call site uses
///   `return extracted(…)`.
/// - `VoidGuards` / `UniformGuards` / `SentinelNull` — guard-clause
///   patterns that can be safely extracted with special call sites.
/// - `Unsafe` — cannot safely extract.
///
/// `return_value_count` is the number of variables modified inside the
/// selection that are read after it (the scope classifier's
/// `return_values.len()`).  Most guard strategies are rejected when
/// this is non-zero, except `NullGuardWithValue` which handles exactly
/// one return value with all-null guards.
fn analyse_returns(
    content: &str,
    start: usize,
    end: usize,
    return_value_count: usize,
) -> ReturnStrategy {
    crate::parser::with_parsed_program(content, "extract_function", |program, content| {
        let body_stmts = find_enclosing_body_statements(&program.statements, start as u32);

        // Collect the statements that fall inside the selection.
        let selected: Vec<&Statement<'_>> = body_stmts
            .iter()
            .filter(|stmt| {
                let span = stmt.span();
                let s = span.start.offset as usize;
                let e = span.end.offset as usize;
                s >= start && e <= end
            })
            .copied()
            .collect();

        if selected.is_empty() {
            return Some(ReturnStrategy::None);
        }

        // Check whether the last selected statement is a `return`.
        let has_trailing_return = matches!(selected.last(), Some(Statement::Return(_)));

        // Check whether any statement in the selection contains a return
        // (at any nesting level).
        let any_return = selected.iter().any(|s| selection_stmt_contains_return(s));

        if !any_return {
            return Some(ReturnStrategy::None);
        }

        // When the selection ends with `return`, the call site is
        // `return extracted(…)`, so every return path inside the
        // extracted function propagates correctly.
        if has_trailing_return {
            return Some(ReturnStrategy::TrailingReturn);
        }

        // The selection contains returns but does NOT end with one.
        // Try to find a guard-clause strategy.
        Some(classify_guard_returns(
            content,
            &selected,
            return_value_count,
        ))
    })
    .unwrap_or(ReturnStrategy::None)
}

/// Check whether a statement is or contains a `return` at any depth.
fn selection_stmt_contains_return(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Return(_) => true,
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => {
                selection_stmt_contains_return(body.statement)
                    || body
                        .else_if_clauses
                        .iter()
                        .any(|c| selection_stmt_contains_return(c.statement))
                    || body
                        .else_clause
                        .as_ref()
                        .is_some_and(|c| selection_stmt_contains_return(c.statement))
            }
            IfBody::ColonDelimited(body) => {
                body.statements
                    .iter()
                    .any(|s| selection_stmt_contains_return(s))
                    || body.else_if_clauses.iter().any(|c| {
                        c.statements
                            .iter()
                            .any(|s| selection_stmt_contains_return(s))
                    })
                    || body.else_clause.as_ref().is_some_and(|c| {
                        c.statements
                            .iter()
                            .any(|s| selection_stmt_contains_return(s))
                    })
            }
        },
        Statement::Foreach(f) => match &f.body {
            ForeachBody::Statement(s) => selection_stmt_contains_return(s),
            ForeachBody::ColonDelimited(b) => b
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s)),
        },
        Statement::While(w) => match &w.body {
            WhileBody::Statement(s) => selection_stmt_contains_return(s),
            WhileBody::ColonDelimited(b) => b
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s)),
        },
        Statement::DoWhile(dw) => selection_stmt_contains_return(dw.statement),
        Statement::For(f) => match &f.body {
            ForBody::Statement(s) => selection_stmt_contains_return(s),
            ForBody::ColonDelimited(b) => b
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s)),
        },
        Statement::Switch(sw) => sw.body.cases().iter().any(|c| match c {
            SwitchCase::Expression(e) => e
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s)),
            SwitchCase::Default(d) => d
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s)),
        }),
        Statement::Try(t) => {
            t.block
                .statements
                .iter()
                .any(|s| selection_stmt_contains_return(s))
                || t.catch_clauses.iter().any(|c| {
                    c.block
                        .statements
                        .iter()
                        .any(|s| selection_stmt_contains_return(s))
                })
                || t.finally_clause.as_ref().is_some_and(|f| {
                    f.block
                        .statements
                        .iter()
                        .any(|s| selection_stmt_contains_return(s))
                })
        }
        Statement::Block(b) => b
            .statements
            .iter()
            .any(|s| selection_stmt_contains_return(s)),
        _ => false,
    }
}

// ─── Return strategy ────────────────────────────────────────────────────────

/// How to handle return statements in the extracted code.
///
/// When the selection contains `return` statements that are NOT the last
/// statement, naive extraction would break control flow.  This enum
/// describes the strategy for preserving the caller's early-exit
/// semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReturnStrategy {
    /// No return statements in the selection.
    None,
    /// The last selected statement is a `return` — the call site becomes
    /// `return extracted(…)` and every return path propagates correctly.
    TrailingReturn,
    /// All returns are bare `return;` (void guards).  The extracted
    /// function returns `bool` (true = continue, false = exit early)
    /// and the call site is `if (!extracted(…)) return;`.
    VoidGuards,
    /// All returns return the same non-null literal value.  The
    /// extracted function returns `bool` and the call site is
    /// `if (!extracted(…)) return <value>;`.
    ///
    /// The string is the source text of the common return value.
    UniformGuards(String),
    /// Returns have different non-null values — use `null` as a
    /// sentinel for "no early exit."  The extracted function returns
    /// `?<type>` and the call site is:
    /// ```php
    /// $result = extracted(…);
    /// if ($result !== null) return $result;
    /// ```
    SentinelNull,
    /// All guard returns are `null` (or bare `return;`) and the
    /// selection also computes exactly one return value.  The extracted
    /// function returns the computed value on success or `null` when a
    /// guard fires.  The call site assigns the result and checks for
    /// null:
    /// ```php
    /// $var = extracted(…);
    /// if ($var === null) return null;  // or `return;` for void guards
    /// ```
    ///
    /// The `bool` flag is `true` when the original guards were bare
    /// `return;` (void).  In that case the body's `return;` statements
    /// are rewritten to `return null;`, and the call site uses bare
    /// `return;` instead of `return null;`.
    NullGuardWithValue(bool),
    /// Cannot safely extract (e.g. returns null, or modified variables
    /// are used after the selection).
    Unsafe,
}

/// Collect the source text of every `return` expression in the selected
/// statements.
///
/// Bare `return;` is represented as `None`.  `return expr;` yields
/// `Some("expr")` with the expression's source text.
fn collect_return_expressions<'a>(
    content: &'a str,
    stmts: &[&Statement<'_>],
) -> Vec<Option<&'a str>> {
    let mut out = Vec::new();
    for stmt in stmts {
        collect_returns_from_stmt(content, stmt, &mut out);
    }
    out
}

/// Recursively collect return expressions from a single statement.
fn collect_returns_from_stmt<'a>(
    content: &'a str,
    stmt: &Statement<'_>,
    out: &mut Vec<Option<&'a str>>,
) {
    match stmt {
        Statement::Return(ret) => {
            let expr_text = ret.value.as_ref().map(|expr| {
                let s = expr.span().start.offset as usize;
                let e = expr.span().end.offset as usize;
                content[s..e].trim()
            });
            out.push(expr_text);
        }
        Statement::If(if_stmt) => match &if_stmt.body {
            IfBody::Statement(body) => {
                collect_returns_from_stmt(content, body.statement, out);
                for c in &body.else_if_clauses {
                    collect_returns_from_stmt(content, c.statement, out);
                }
                if let Some(c) = &body.else_clause {
                    collect_returns_from_stmt(content, c.statement, out);
                }
            }
            IfBody::ColonDelimited(body) => {
                for s in &body.statements {
                    collect_returns_from_stmt(content, s, out);
                }
                for c in &body.else_if_clauses {
                    for s in &c.statements {
                        collect_returns_from_stmt(content, s, out);
                    }
                }
                if let Some(c) = &body.else_clause {
                    for s in &c.statements {
                        collect_returns_from_stmt(content, s, out);
                    }
                }
            }
        },
        Statement::Foreach(f) => match &f.body {
            ForeachBody::Statement(s) => collect_returns_from_stmt(content, s, out),
            ForeachBody::ColonDelimited(b) => {
                for s in &b.statements {
                    collect_returns_from_stmt(content, s, out);
                }
            }
        },
        Statement::While(w) => match &w.body {
            WhileBody::Statement(s) => collect_returns_from_stmt(content, s, out),
            WhileBody::ColonDelimited(b) => {
                for s in &b.statements {
                    collect_returns_from_stmt(content, s, out);
                }
            }
        },
        Statement::DoWhile(dw) => collect_returns_from_stmt(content, dw.statement, out),
        Statement::For(f) => match &f.body {
            ForBody::Statement(s) => collect_returns_from_stmt(content, s, out),
            ForBody::ColonDelimited(b) => {
                for s in &b.statements {
                    collect_returns_from_stmt(content, s, out);
                }
            }
        },
        Statement::Switch(sw) => {
            for c in sw.body.cases().iter() {
                let stmts = match c {
                    SwitchCase::Expression(e) => &e.statements,
                    SwitchCase::Default(d) => &d.statements,
                };
                for s in stmts.iter() {
                    collect_returns_from_stmt(content, s, out);
                }
            }
        }
        Statement::Try(t) => {
            for s in &t.block.statements {
                collect_returns_from_stmt(content, s, out);
            }
            for c in &t.catch_clauses {
                for s in &c.block.statements {
                    collect_returns_from_stmt(content, s, out);
                }
            }
            if let Some(f) = &t.finally_clause {
                for s in &f.block.statements {
                    collect_returns_from_stmt(content, s, out);
                }
            }
        }
        Statement::Block(b) => {
            for s in &b.statements {
                collect_returns_from_stmt(content, s, out);
            }
        }
        _ => {}
    }
}

/// Whether a guard's return value is safe to reproduce verbatim at the
/// call site.
///
/// `UniformGuards` re-emits the return expression at the call site
/// (`if (!extracted(…)) return <value>;`).  That is only correct when the
/// value is a side-effect-free literal or constant: it must not reference
/// a variable (which could be local to the selection and out of scope at
/// the call site) and must not be a call (which would run twice).  String
/// literals are allowed even though they may contain `(`.
fn is_reproducible_guard_value(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return false;
    }
    // Any variable reference is risky — it may be selection-local.
    if v.contains('$') {
        return false;
    }
    // Quoted string literals are fine (their contents are inert).
    let single = v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'');
    let double = v.len() >= 2 && v.starts_with('"') && v.ends_with('"');
    if single || double {
        return true;
    }
    // Numbers and constants (e.g. `42`, `Status::Bad`, `self::FOO`) are
    // fine; a `(` indicates a call, which may have side effects.
    !v.contains('(')
}

/// Classify the return strategy for a selection that contains return
/// statements but does NOT end with one.
///
/// This is called only when `has_unsafe_return` would have been `true`
/// under the old logic.  It inspects the actual return expressions to
/// decide whether a safe extraction pattern exists.
fn classify_guard_returns(
    content: &str,
    stmts: &[&Statement<'_>],
    return_value_count: usize,
) -> ReturnStrategy {
    let return_exprs = collect_return_expressions(content, stmts);
    if return_exprs.is_empty() {
        return ReturnStrategy::Unsafe;
    }

    // When the selection modifies variables that are used after it,
    // most guard strategies can't work — we'd need to return both
    // the sentinel and the modified variables.  The exception is
    // NullGuardWithValue: all guards return null (or bare return;),
    // exactly one return value, and the extracted function returns
    // the value or null.
    if return_value_count > 0 {
        if return_value_count != 1 {
            return ReturnStrategy::Unsafe;
        }
        // All bare `return;` → NullGuardWithValue(true) (void guards).
        if return_exprs.iter().all(|e| e.is_none()) {
            return ReturnStrategy::NullGuardWithValue(true);
        }
        // All `return null;` → NullGuardWithValue(false).
        if return_exprs.iter().any(|e| e.is_none()) {
            // Mix of bare and valued returns — can't handle.
            return ReturnStrategy::Unsafe;
        }
        let all_null = return_exprs
            .iter()
            .all(|e| e.unwrap().trim().eq_ignore_ascii_case("null"));
        if all_null {
            return ReturnStrategy::NullGuardWithValue(false);
        }
        return ReturnStrategy::Unsafe;
    }

    // Case 1: All returns are bare `return;` (void guards).
    if return_exprs.iter().all(|e| e.is_none()) {
        return ReturnStrategy::VoidGuards;
    }

    // If any return is bare but others aren't, we have a mix of void
    // and valued returns — can't handle this.
    if return_exprs.iter().any(|e| e.is_none()) {
        return ReturnStrategy::Unsafe;
    }

    // All returns have values.  Check if any returns null.
    let values: Vec<&str> = return_exprs.iter().map(|e| e.unwrap()).collect();
    let any_returns_null = values.iter().any(|v| {
        let lower = v.trim().to_lowercase();
        lower == "null"
    });

    // Case 2: All return the same value.
    let all_same = values.windows(2).all(|w| w[0].trim() == w[1].trim());
    if all_same {
        let value = values[0].trim().to_string();
        let lower = value.to_lowercase();
        // `true`/`false`/`null` are always safe to reproduce at the call
        // site: the extracted function returns bool and the call site
        // does `if (!extracted(…)) return <value>;`.
        if lower == "false" || lower == "true" || lower == "null" {
            return ReturnStrategy::UniformGuards(value);
        }
        // Other uniform values can only be reproduced at the call site
        // when they are side-effect-free literals or constants.  A value
        // that references a variable or a call (e.g. `redirect($url)`)
        // may depend on variables that are local to the selection and
        // therefore out of scope at the call site, or it may have side
        // effects that must not run twice.  Such a value is kept inside
        // the extracted function via the null sentinel below.
        if is_reproducible_guard_value(&value) {
            return ReturnStrategy::UniformGuards(value);
        }
        // Fall through to the null-sentinel strategy.
    }

    // Case 3: Different (or non-reproducible) values, none are null —
    // use null sentinel so the return expressions stay inside the
    // extracted function and propagate through `$result`.
    if !any_returns_null {
        return ReturnStrategy::SentinelNull;
    }

    // Different values including null — can't use null as sentinel
    // and can't use bool flag either.
    ReturnStrategy::Unsafe
}

/// Resolve the return type of the enclosing function/method at `offset`.
///
/// Extracts the native return type hint from the function signature.
/// Extract the parameter names of the enclosing function/method in
/// declaration order.  Used to sort extracted-function parameters so
/// they mirror the original signature.
fn resolve_enclosing_param_order(content: &str, offset: u32) -> Vec<String> {
    crate::parser::with_parsed_program(content, "extract_function", |program, _| {
        let ctx = find_cursor_context(&program.statements, offset);

        let param_list = match ctx {
            CursorContext::InClassLike { member, .. } => {
                if let MemberContext::Method(method, true) = member {
                    Some(&method.parameter_list)
                } else {
                    None
                }
            }
            CursorContext::InFunction(func, true) => Some(&func.parameter_list),
            _ => None,
        };

        match param_list {
            Some(pl) => pl
                .parameters
                .iter()
                .map(|p| bytes_to_str(p.variable.name).to_string())
                .collect(),
            None => Vec::new(),
        }
    })
}

/// Sort extracted-function parameters so that variables matching the
/// enclosing function's signature come first (in their original order),
/// followed by any other variables in classification order.
fn sort_params_by_enclosing_order(
    mut params: Vec<(String, PhpType, PhpType)>,
    enclosing_order: &[String],
) -> Vec<(String, PhpType, PhpType)> {
    if enclosing_order.is_empty() {
        return params;
    }
    params.sort_by(|a, b| {
        let idx_a = enclosing_order.iter().position(|n| *n == a.0);
        let idx_b = enclosing_order.iter().position(|n| *n == b.0);
        match (idx_a, idx_b) {
            // Both are signature params → preserve signature order.
            (Some(ia), Some(ib)) => ia.cmp(&ib),
            // Signature params come before non-signature variables.
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            // Neither is a signature param → preserve classification order.
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    params
}

fn resolve_enclosing_return_type(content: &str, offset: u32) -> PhpType {
    crate::parser::with_parsed_program(content, "extract_function", |program, _| {
        let ctx = find_cursor_context(&program.statements, offset);

        let ty = match ctx {
            CursorContext::InClassLike { member, .. } => {
                if let MemberContext::Method(method, true) = member {
                    method
                        .return_type_hint
                        .as_ref()
                        .map(|h| crate::parser::extract_hint_type(&h.hint))
                        .unwrap_or_else(PhpType::untyped)
                } else {
                    PhpType::untyped()
                }
            }
            CursorContext::InFunction(func, true) => func
                .return_type_hint
                .as_ref()
                .map(|h| crate::parser::extract_hint_type(&h.hint))
                .unwrap_or_else(PhpType::untyped),
            _ => PhpType::untyped(),
        };
        Some(ty)
    })
    .unwrap_or_else(PhpType::untyped)
}

// ─── Main code action collector ─────────────────────────────────────────────

impl Backend {
    /// Collect "Extract Function" / "Extract Method" code actions.
    ///
    /// This action is offered when the user has a non-empty selection
    /// that covers one or more complete statements inside a function or
    /// method body.
    ///
    /// Phase 1 performs lightweight validation only.  The expensive
    /// work (scope classification, type resolution, PHPDoc generation,
    /// edit building) is deferred to [`resolve_extract_function`]
    /// (Phase 2).
    pub(crate) fn collect_extract_function_actions(
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

        // Trim the selection to exclude leading/trailing whitespace.
        let (start, end) = match trim_selection(content, start_offset, end_offset) {
            Some(range) => range,
            None => return,
        };

        // Validate that the selection covers complete statements.
        if !selection_covers_complete_statements(content, start, end) {
            return;
        }

        // ── Determine method vs function for the title ──────────────
        // We only need to know whether `$this`/`self::`/`static::` is
        // referenced to pick "Extract method" vs "Extract function".
        // A simple text scan is sufficient for the title — the full
        // scope analysis happens in Phase 2.
        let selected_text = &content[start..end];
        let looks_like_method = selected_text.contains("$this")
            || selected_text.contains("self::")
            || selected_text.contains("static::")
            || selected_text.contains("parent::");

        let title = if looks_like_method {
            "Extract method".to_string()
        } else {
            "Extract function".to_string()
        };

        // Phase 1: emit a lightweight code action with no edit.
        // The full workspace edit is computed lazily in
        // `resolve_extract_function` (Phase 2) when the user picks
        // this action.
        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::REFACTOR_EXTRACT),
            diagnostics: None,
            edit: None,
            command: None,
            is_preferred: Some(false),
            disabled: None,
            data: Some(make_code_action_data(
                "refactor.extractFunction",
                uri,
                &params.range,
                serde_json::json!({}),
            )),
        }));
    }

    /// Resolve types for a list of variable names at a given offset.
    ///
    /// Returns `(dollar_name, cleaned_type, raw_hint)` triples.
    /// `cleaned_type` has generics stripped for use in native PHP
    /// signatures.  `raw_hint` preserves the full resolved type
    /// (e.g. `Collection<User>`) for PHPDoc generation.
    fn resolve_param_types(
        &self,
        uri: &str,
        content: &str,
        offset: u32,
        var_names: &[String],
    ) -> Vec<(String, PhpType, PhpType)> {
        var_names
            .iter()
            .map(|name| {
                let dollar_name = if name.starts_with('$') {
                    name.clone()
                } else {
                    format!("${}", name)
                };
                let resolved_type = resolve_var_type(self, &dollar_name, content, offset, uri);
                let raw_type = resolved_type.clone().unwrap_or_else(PhpType::untyped);
                // Clean up the type for use in a signature — stays as PhpType.
                let cleaned = resolved_type
                    .as_ref()
                    .and_then(clean_type_for_signature_typed)
                    .unwrap_or_else(PhpType::untyped);
                (dollar_name, cleaned, raw_type)
            })
            .collect()
    }

    /// Resolve a deferred "Extract Function/Method" code action.
    ///
    /// This is **Phase 2** of the two-phase code-action model.  Phase 1
    /// (`collect_extract_function_actions`) already validated the
    /// selection and emitted a lightweight `CodeAction` with a title
    /// but no edit.  Here we re-run the full extraction logic from the
    /// selection range stored in `data` and produce the workspace edit.
    pub(crate) fn resolve_extract_function(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;
        let range = &data.range;

        // ── Re-validate the selection (content may have changed) ────
        let start_offset = position_to_byte_offset(content, range.start);
        let end_offset = position_to_byte_offset(content, range.end);

        let (start, end) = trim_selection(content, start_offset, end_offset)?;

        if !selection_covers_complete_statements(content, start, end) {
            return None;
        }

        // ── Scope map & classification ──────────────────────────────
        let scope_map = build_scope_map(content, start as u32);
        let classification = scope_map.classify_range(start as u32, end as u32);

        let return_value_count = classification.return_values.len();
        let return_strategy = analyse_returns(content, start, end, return_value_count);

        if return_strategy == ReturnStrategy::Unsafe {
            return None;
        }

        let uses_this = if scope_map.has_this_or_self {
            classification.uses_this
        } else {
            false
        };

        if scope_map.uses_reference_params() && !classification.reference_writes.is_empty() {
            return None;
        }

        if classification.return_values.len() > 4 {
            return None;
        }

        // ── Enclosing context ───────────────────────────────────────
        let enclosing = find_enclosing_context(content, start as u32, uses_this)?;

        // ── Naming ──────────────────────────────────────────────────
        let body_line_start_for_naming = find_line_start(content, start);
        let body_text_for_naming = &content[body_line_start_for_naming..end];
        let pre_trailing_return_type = if matches!(return_strategy, ReturnStrategy::TrailingReturn)
        {
            resolve_enclosing_return_type(content, start as u32)
        } else {
            PhpType::untyped()
        };
        let naming_ctx = NamingContext {
            enclosing_name: &enclosing.enclosing_name,
            return_strategy: &return_strategy,
            body_text: body_text_for_naming,
            return_var_names: &classification.return_values,
            trailing_return_type: &pre_trailing_return_type,
        };
        let fn_name = generate_function_name(content, &enclosing, &naming_ctx);

        // ── Type resolution ─────────────────────────────────────────
        let typed_params =
            self.resolve_param_types(uri, content, start as u32, &classification.parameters);
        let enclosing_param_order = resolve_enclosing_param_order(content, start as u32);
        let typed_params = sort_params_by_enclosing_order(typed_params, &enclosing_param_order);
        let typed_returns =
            self.resolve_param_types(uri, content, start as u32, &classification.return_values);

        // ── Indentation ─────────────────────────────────────────────
        let call_indent = indent_at(content, start);
        let (member_indent, body_indent) = match enclosing.target {
            ExtractionTarget::Method => {
                let member = detect_line_indent(content, enclosing.body_start);
                let unit = detect_indent_unit(content);
                let body = format!("{}{}", member, unit);
                (member, body)
            }
            ExtractionTarget::Function => {
                let member = String::new();
                let unit = detect_indent_unit(content);
                (member, unit.to_string())
            }
        };

        // ── Body text ───────────────────────────────────────────────
        let body_line_start = find_line_start(content, start);
        let body_text = content[body_line_start..end].to_string();

        // ── Return type resolution ──────────────────────────────────
        let trailing_return_type = if matches!(
            return_strategy,
            ReturnStrategy::TrailingReturn
                | ReturnStrategy::SentinelNull
                | ReturnStrategy::NullGuardWithValue(_)
        ) {
            resolve_enclosing_return_type(content, start as u32)
        } else {
            PhpType::untyped()
        };

        let enclosing_docblock_return: Option<PhpType> = if matches!(
            return_strategy,
            ReturnStrategy::TrailingReturn | ReturnStrategy::SentinelNull
        ) {
            crate::docblock::find_enclosing_return_type(content, start)
        } else {
            None
        };

        // ── PHPDoc generation ───────────────────────────────────────
        let return_type_for_docblock = build_return_type_hint_for_docblock(
            &return_strategy,
            &trailing_return_type,
            &typed_returns,
        );
        let raw_return_type_for_docblock = build_raw_return_type_for_docblock(
            &return_strategy,
            &trailing_return_type,
            enclosing_docblock_return.as_ref(),
            &typed_returns,
        );
        let ctx = self.file_context(uri);
        let class_loader = self.class_loader(&ctx);
        let docblock = build_docblock_for_extraction(
            &typed_params,
            &return_type_for_docblock,
            &raw_return_type_for_docblock,
            &member_indent,
            &class_loader,
        );

        // ── Build ExtractionInfo ────────────────────────────────────
        let params_for_info: Vec<(String, PhpType)> = typed_params
            .iter()
            .map(|(name, cleaned, _)| (name.clone(), cleaned.clone()))
            .collect();
        let returns_for_info: Vec<(String, PhpType)> = typed_returns
            .iter()
            .map(|(name, cleaned, _)| (name.clone(), cleaned.clone()))
            .collect();

        let info = ExtractionInfo {
            name: fn_name,
            params: params_for_info,
            returns: returns_for_info,
            body: body_text,
            target: enclosing.target,
            is_static: enclosing.is_static,
            member_indent,
            body_indent,
            return_strategy,
            trailing_return_type,
            docblock,
        };

        // ── Build edits ─────────────────────────────────────────────
        let definition = build_extracted_definition(&info);
        let call_site = build_call_site(&info, &call_indent);

        let doc_uri: Url = uri.parse().ok()?;

        let replace_start = find_line_start(content, start);
        let replace_end = find_line_end(content, end.saturating_sub(1).max(start));

        let replace_start_pos = offset_to_position(content, replace_start);
        let replace_end_pos = offset_to_position(content, replace_end);

        let insert_pos = offset_to_position(content, enclosing.insert_offset);

        let edits = vec![
            TextEdit {
                range: Range {
                    start: replace_start_pos,
                    end: replace_end_pos,
                },
                new_text: call_site,
            },
            TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: definition,
            },
        ];

        let mut changes = HashMap::new();
        changes.insert(doc_uri, edits);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

/// Clean a resolved type string for use in a function signature.
///
/// Removes generic parameters (PHP doesn't support them in signatures),
/// and simplifies union types that are too complex for type hints.
/// Compute the raw (un-cleaned) return type hint string for PHPDoc
/// enrichment purposes.  Unlike `build_return_type` (which strips
/// generics for native hints), this preserves the full type so that
/// `enrichment_plain` can detect whether a docblock `@return` tag is
/// warranted.
fn build_return_type_hint_for_docblock(
    strategy: &ReturnStrategy,
    trailing_return_type: &PhpType,
    returns: &[(String, PhpType, PhpType)],
) -> PhpType {
    match strategy {
        ReturnStrategy::TrailingReturn => trailing_return_type.clone(),
        ReturnStrategy::VoidGuards | ReturnStrategy::UniformGuards(_) => PhpType::bool(),
        ReturnStrategy::SentinelNull => {
            if !trailing_return_type.is_empty() {
                trailing_return_type.clone()
            } else {
                PhpType::untyped()
            }
        }
        ReturnStrategy::NullGuardWithValue(_) => {
            if returns.len() == 1 {
                if let Some(hint) = returns[0].1.to_native_hint_typed() {
                    return hint;
                }
                PhpType::untyped()
            } else {
                PhpType::untyped()
            }
        }
        ReturnStrategy::None | ReturnStrategy::Unsafe => {
            if returns.is_empty() {
                PhpType::void()
            } else if returns.len() == 1 {
                if let Some(hint) = returns[0].1.to_native_hint_typed() {
                    return hint;
                }
                PhpType::untyped()
            } else {
                PhpType::array()
            }
        }
    }
}

/// Like `build_return_type_hint_for_docblock` but returns the raw
/// (un-cleaned) type that preserves concrete generic arguments.
fn build_raw_return_type_for_docblock(
    strategy: &ReturnStrategy,
    trailing_return_type: &PhpType,
    enclosing_docblock_return: Option<&PhpType>,
    returns: &[(String, PhpType, PhpType)],
) -> PhpType {
    match strategy {
        ReturnStrategy::TrailingReturn => {
            // Prefer the docblock @return type when it carries concrete
            // generics (e.g. `Collection<User>`) over the native hint
            // (e.g. `Collection`).
            if let Some(edr) = enclosing_docblock_return
                && edr.has_type_parameters()
            {
                return edr.clone();
            }
            trailing_return_type.clone()
        }
        ReturnStrategy::VoidGuards | ReturnStrategy::UniformGuards(_) => PhpType::bool(),
        ReturnStrategy::SentinelNull => {
            if let Some(edr) = enclosing_docblock_return
                && edr.has_type_parameters()
            {
                return edr.clone();
            }
            if !trailing_return_type.is_empty() {
                trailing_return_type.clone()
            } else {
                PhpType::untyped()
            }
        }
        ReturnStrategy::NullGuardWithValue(_) => {
            // Use raw type (index 2) which preserves generics.
            if returns.len() == 1 && !returns[0].2.is_empty() {
                returns[0].2.clone()
            } else {
                PhpType::untyped()
            }
        }
        ReturnStrategy::None | ReturnStrategy::Unsafe => {
            if returns.is_empty() {
                PhpType::void()
            } else if returns.len() == 1 {
                // Use raw type (index 2) which preserves generics.
                returns[0].2.clone()
            } else {
                PhpType::array()
            }
        }
    }
}

#[cfg(test)]
fn clean_type_for_signature(type_str: &str) -> String {
    if type_str.is_empty() {
        return String::new();
    }

    let parsed = PhpType::parse(type_str);
    parsed.to_native_hint().unwrap_or_default()
}

/// Like [`clean_type_for_signature`] but accepts an already-parsed
/// [`PhpType`] and returns a structured [`PhpType`] instead of a
/// `String`, avoiding a redundant `PhpType::parse` round-trip.
fn clean_type_for_signature_typed(ty: &PhpType) -> Option<PhpType> {
    ty.to_native_hint_typed()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "extract_function_tests.rs"]
mod tests;
