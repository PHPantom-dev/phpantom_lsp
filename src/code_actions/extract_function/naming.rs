use super::*;

// ─── Name generation ────────────────────────────────────────────────────────

/// Generate a unique function/method name that doesn't conflict with
/// existing members or functions.
/// Context passed to [`generate_function_name`] to produce meaningful names.
pub(crate) struct NamingContext<'a> {
    /// The enclosing function/method name (e.g. `"run"`, `"process"`).
    pub(crate) enclosing_name: &'a str,
    /// The return strategy chosen for the extraction.
    pub(crate) return_strategy: &'a ReturnStrategy,
    /// The selected body text (trimmed source of the extracted statements).
    pub(crate) body_text: &'a str,
    /// Names of return-value variables (written inside, read after).
    pub(crate) return_var_names: &'a [String],
    /// The trailing return type hint (e.g. `Collection`, `User`).
    pub(crate) trailing_return_type: &'a PhpType,
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
pub(crate) fn generate_function_name(
    content: &str,
    enclosing_ctx: &EnclosingContext,
    naming: &NamingContext,
) -> String {
    let base = derive_base_name(naming);

    // Deduplicate against the right scope.
    deduplicate_name(&base, content, enclosing_ctx)
}

/// Pick a base name from the naming context (before deduplication).
pub(crate) fn derive_base_name(ctx: &NamingContext) -> String {
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
pub(crate) fn detect_factory_pattern(body: &str) -> Option<String> {
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
pub(crate) fn extract_new_class_name(text: &str) -> Option<String> {
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
pub(crate) fn is_output_line(line: &str) -> bool {
    OUTPUT_PREFIXES.iter().any(|p| line.starts_with(p))
}

/// Check whether every statement in the body is a pure output statement
/// (`echo`, `print`, `printf`, `var_dump`, `print_r`, `var_export`).
pub(crate) fn is_pure_output(body: &str) -> bool {
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
pub(crate) fn ends_with_output(body: &str) -> bool {
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
pub(crate) fn detect_single_call(body: &str) -> Option<String> {
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
pub(crate) fn deduplicate_name(base: &str, content: &str, ctx: &EnclosingContext) -> String {
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
pub(crate) fn trim_selection(content: &str, start: usize, end: usize) -> Option<(usize, usize)> {
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
pub(crate) fn detect_line_indent(content: &str, offset: usize) -> String {
    let before = &content[..offset];
    let line_start = before.rfind('\n').map_or(0, |p| p + 1);
    let line = &content[line_start..offset];
    line.chars().take_while(|c| c.is_whitespace()).collect()
}

/// Detect whether the file uses tabs or spaces (and how many spaces).
pub(crate) fn detect_indent_unit(content: &str) -> &str {
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
pub(crate) fn find_line_end(content: &str, offset: usize) -> usize {
    match content[offset..].find('\n') {
        Some(pos) => offset + pos + 1,
        None => content.len(),
    }
}

/// Find the start of the line containing `offset`.
pub(crate) fn find_line_start(content: &str, offset: usize) -> usize {
    content[..offset].rfind('\n').map_or(0, |p| p + 1)
}

/// Extract the indentation (leading whitespace) of the line at `offset`.
pub(crate) fn indent_at(content: &str, offset: usize) -> String {
    let line_start = find_line_start(content, offset);
    let rest = &content[line_start..];
    rest.chars().take_while(|c| c.is_whitespace()).collect()
}
