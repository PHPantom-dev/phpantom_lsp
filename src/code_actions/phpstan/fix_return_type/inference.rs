//! Return-type inference from function bodies.
//!
//! Scans the `return` statements of a function to infer a return type:
//! literals are inferred syntactically (cheap), `$variable` returns go
//! through the full variable-resolution pipeline, and everything else
//! falls back to `mixed`.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::Position;

use crate::Backend;
use crate::completion::resolver::Loaders;
use crate::completion::variable::resolution::resolve_variable_types;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FunctionLoader, ResolvedType};
use crate::util::{find_brace_match_line, find_semicolon_balanced, line_start_byte_offset};

use super::edits::find_open_brace_from_declaration;

// ── Return type inference result ────────────────────────────────────────────

/// The result of inferring a return type from a function body.
///
/// Separates the native PHP type hint (for the `: type` declaration)
/// from the effective PHPStan type (for a `@return` docblock tag).
/// When the two are identical, no docblock is needed.
pub(crate) struct InferredReturnType {
    /// Valid native PHP type hint (e.g. `array`, `int`, `Foo`).
    pub(crate) native: PhpType,
    /// Full effective type including generics/shapes (e.g. `list<string>`).
    /// `None` when the native type already captures the full type.
    pub(crate) effective: Option<PhpType>,
}

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Infer the return type of the function at `func_line` by scanning
    /// all return statements in the body.
    ///
    /// For simple literals (`return 1;`, `return 'hello';`, `return new Foo()`)
    /// the type is inferred syntactically. For `$variable` returns, the
    /// full variable-resolution pipeline is used. All other expressions
    /// (method calls, function calls, complex expressions) produce `mixed`.
    ///
    /// Returns an [`InferredReturnType`] that separates the native PHP
    /// type hint from the richer effective type.  When they differ (e.g.
    /// `list<string>` vs `array`), the caller should add a `@return` tag.
    ///
    /// When `self_as_marker` is `true`, `return $this;` yields the self-like
    /// marker `$this` so the type engine can map it to the receiver class
    /// rather than the (possibly trait) class that declares the method.
    pub(crate) fn infer_return_type_for_function(
        &self,
        uri: &str,
        content: &str,
        func_line: usize,
        self_as_marker: bool,
    ) -> Option<InferredReturnType> {
        // Set up the resolution infrastructure from Backend state.
        let local_classes: Vec<Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);
        let class_loader = self.class_loader_with(&local_classes, &file_use_map, &file_namespace);
        let function_loader = self.function_loader_with(None, &file_use_map, &file_namespace);

        infer_return_type(
            content,
            func_line,
            &local_classes,
            &class_loader,
            Some(&function_loader),
            self_as_marker,
        )
    }
}

// ── Shared return-type inference ────────────────────────────────────────────

/// Infer the return type of a function by scanning all `return`
/// statements in the body.
///
/// For simple literals (`return 1;`, `return 'hello';`, `return new Foo()`)
/// the type is inferred syntactically.  For `$variable` returns, the
/// full variable-resolution pipeline is used.  All other expressions
/// (method calls, function calls, complex expressions) produce `mixed`.
///
/// Returns an [`InferredReturnType`] that separates the native PHP
/// type hint from the richer effective type.  When they differ (e.g.
/// `list<string>` vs `array`), the caller should add a `@return` tag.
///
/// When `self_as_marker` is `true`, a `return $this;` statement yields
/// the self-like marker `$this` instead of resolving to the concrete
/// enclosing class.  The type engine needs this so that a fluent method
/// inherited from a trait maps to the class the method is *called on*,
/// not the trait that lexically declares it.  Code actions that write a
/// concrete return type or docblock pass `false`.
///
/// This is the shared core used by:
/// - `Backend::infer_return_type_for_function` (PHPStan code actions)
/// - `enrichment_return_type` (Generate / Update PHPDoc)
pub(crate) fn infer_return_type(
    content: &str,
    func_line: usize,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
    self_as_marker: bool,
) -> Option<InferredReturnType> {
    let lines: Vec<&str> = content.lines().collect();
    if func_line >= lines.len() {
        return None;
    }

    // Find the function body boundaries.
    let brace_line = find_open_brace_from_declaration(&lines, func_line)?;

    // Find the closing `}` that matches the `{` on `brace_line`.
    let body_end = find_brace_match_line(&lines, brace_line, |d| d == 0)?;

    // Find the enclosing class at the function line offset.
    let func_offset = content
        .lines()
        .take(func_line)
        .map(|l| l.len() + 1)
        .sum::<usize>() as u32;
    let enclosing_class = local_classes
        .iter()
        .find(|c| {
            !c.name.starts_with("__anonymous@")
                && func_offset >= c.start_offset
                && func_offset <= c.end_offset
        })
        .map(|c| ClassInfo::clone(c))
        .unwrap_or_default();

    // Scan return statements and resolve their types.
    let mut return_types: Vec<PhpType> = Vec::new();
    let mut has_bare_return = false;
    let mut has_return_with_value = false;

    // Single-line function body: `function foo() { return new Bar(); }`
    // The normal loop (skip(brace_line+1)..take(body_end)) would iterate
    // zero times because brace_line == body_end.  Extract the content
    // between the first `{` and last `}` and scan it directly.
    if brace_line == body_end
        && let line = lines[brace_line]
        && let Some(open) = line.find('{')
        && let Some(close) = line.rfind('}')
        && close > open + 1
    {
        let inner = line[open + 1..close].trim();
        if inner == "return;" || inner.is_empty() {
            has_bare_return = !inner.is_empty();
        } else if let Some(rest) = inner.strip_prefix("return ") {
            let expr = rest.strip_suffix(';').unwrap_or(rest).trim();
            has_return_with_value = true;
            if self_as_marker && expr == "$this" {
                return_types.push(PhpType::this());
            } else if let Some(t) = infer_type_from_literal(expr) {
                let resolved = t.resolve_names(&|name: &str| {
                    if let Some(cls) = class_loader(name) {
                        cls.fqn().to_string()
                    } else {
                        name.to_string()
                    }
                });
                return_types.push(resolved);
            } else {
                return_types.push(PhpType::mixed());
            }
        }
    }

    // Track nested closure/anonymous-function depth so we skip their
    // return statements (they belong to a different scope) while still
    // capturing returns inside control structures (if/for/while/switch/
    // try) which are in the same scope.
    //
    // `closure_depth` only increments when a line contains a `function`
    // or `fn` keyword before an opening `{`.  Plain `{` from control
    // structures does not change it.
    let mut closure_depth: i32 = 0;

    for (line_idx, line) in lines.iter().enumerate().take(body_end).skip(brace_line + 1) {
        let trimmed = line.trim();

        // Detect nested closure / anonymous function openings.
        // A line like `$f = function () {` or `fn() => {` introduces
        // a new function scope whose returns we must ignore.
        let has_fn_keyword = trimmed.contains("function ")
            || trimmed.contains("function(")
            || trimmed.starts_with("fn ")
            || trimmed.starts_with("fn(")
            || trimmed.contains(" fn(")
            || trimmed.contains(" fn ");

        for ch in line.chars() {
            match ch {
                '{' => {
                    if has_fn_keyword && closure_depth == 0 {
                        // First `{` on a line with a function keyword
                        // opens a nested scope.
                        closure_depth += 1;
                    } else if closure_depth > 0 {
                        // Already inside a nested closure — track depth
                        // so we know when we leave it.
                        closure_depth += 1;
                    }
                    // Plain `{` at closure_depth == 0 from control
                    // structures: do nothing.
                }
                '}' if closure_depth > 0 => {
                    closure_depth -= 1;
                }
                _ => {}
            }
        }

        // Skip return statements inside nested closures / anonymous fns.
        if closure_depth > 0 {
            continue;
        }

        // Skip comments.
        if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
            continue;
        }

        if trimmed == "return;" {
            has_bare_return = true;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("return ") {
            let rest = rest.trim();
            if rest == ";" {
                has_bare_return = true;
                continue;
            }
            has_return_with_value = true;

            // A `return` expression may span several physical lines (e.g. a
            // multi-line array literal).  Extract the full statement text
            // from the `return` keyword up to the balanced `;`, instead of
            // assuming the whole expression fits on this line — otherwise a
            // formatting-only change (breaking `['a']` across lines) would
            // leave us with an unbalanced fragment like `[` that infers as
            // `mixed` rather than `list<string>`.
            let line_start = line_start_byte_offset(content, line_idx);
            let expr_offset_in_line = line.find("return ").unwrap_or(0) + "return ".len();
            let expr_byte_offset = line_start + expr_offset_in_line;
            let expr = match find_semicolon_balanced(&content[expr_byte_offset..]) {
                Some(semi) => content[expr_byte_offset..expr_byte_offset + semi].trim(),
                None => rest.strip_suffix(';').unwrap_or(rest).trim(),
            };

            // `return $this;` is a fluent self-return.  Yield the self-like
            // marker so the caller maps it to the actual receiver class
            // rather than the class that lexically declares the method
            // (which for a trait method is the trait, not the using class).
            if self_as_marker && expr == "$this" {
                return_types.push(PhpType::this());
                continue;
            }

            // Try syntax-level inference first (cheap).
            if let Some(t) = infer_type_from_literal(expr) {
                // Resolve short class names to FQN via the class loader
                // so that `new Foo(…)` produces a fully-qualified type.
                let resolved = t.resolve_names(&|name: &str| {
                    if let Some(cls) = class_loader(name) {
                        cls.fqn().to_string()
                    } else {
                        name.to_string()
                    }
                });
                return_types.push(resolved);
                continue;
            }

            // Fall back to the variable/expression resolver.
            let expr_offset = expr_byte_offset as u32;

            // Try variable resolution for `$var` expressions.
            if expr.starts_with('$') && !expr.contains(' ') {
                let results = resolve_variable_types(
                    expr,
                    &enclosing_class,
                    local_classes,
                    content,
                    expr_offset,
                    class_loader,
                    Loaders::with_function(function_loader),
                );
                let joined = ResolvedType::types_joined(&results);
                if !joined.is_mixed() {
                    return_types.push(joined);
                    continue;
                }
            }

            // For other expressions, fall back to `mixed`.
            return_types.push(PhpType::mixed());
        }
    }

    if !has_return_with_value && !has_bare_return {
        return Some(InferredReturnType {
            native: PhpType::void(),
            effective: None,
        });
    }

    if return_types.is_empty() && has_bare_return {
        return Some(InferredReturnType {
            native: PhpType::void(),
            effective: None,
        });
    }

    // Deduplicate types structurally (no string round-trip).
    let mut deduped: Vec<PhpType> = Vec::with_capacity(return_types.len());
    for ty in &return_types {
        if !deduped.iter().any(|existing| existing.equivalent(ty)) {
            deduped.push(ty.clone());
        }
    }

    if has_bare_return {
        let has_null = deduped.iter().any(|t| t.is_null());
        if !has_null {
            deduped.push(PhpType::null());
        }
    }

    let effective = if deduped.len() == 1 {
        deduped.into_iter().next().unwrap()
    } else if deduped.len() <= 3 {
        PhpType::Union(deduped)
    } else {
        return None;
    };

    // Convert effective type → native PHP type hint.
    let native = effective
        .to_native_hint_typed()
        .unwrap_or_else(PhpType::mixed);

    let needs_docblock = !native.equivalent(&effective);
    Some(InferredReturnType {
        native,
        effective: if needs_docblock {
            Some(effective)
        } else {
            None
        },
    })
}

/// Infer a `@return` type string for a function whose signature is
/// at `position` in `content`.
///
/// Returns `Some("list<string>")` when the body analysis produces a
/// type richer than the native hint, or `None` when inference fails
/// or the native type already captures the full information.
///
/// This is the entry point for docblock generation (`enrichment_plain`
/// replacement for `@return`) — it finds the function line from the
/// position and delegates to [`infer_return_type`].
pub(crate) fn enrichment_return_type(
    content: &str,
    position: Position,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
) -> Option<PhpType> {
    // The position is on or near the docblock / function signature.
    // Search forward from that line to find the `function` keyword.
    let lines: Vec<&str> = content.lines().collect();
    let start = position.line as usize;
    let end = (start + 10).min(lines.len());
    let func_line =
        (start..end).find(|&i| lines[i].contains("function ") || lines[i].contains("function("))?;

    let inferred = infer_return_type(
        content,
        func_line,
        local_classes,
        class_loader,
        function_loader,
        // Docblock generation wants a concrete written type, not a `$this`
        // marker, so resolve `return $this` to the enclosing class.
        false,
    )?;

    // Return the effective type if it's richer than the native hint,
    // otherwise return the native type (which may still be useful for
    // callers that want any inferred type, e.g. `void`).
    Some(inferred.effective.unwrap_or(inferred.native))
}

// ── Literal inference ───────────────────────────────────────────────────────

/// Infer a PHP type from a literal return expression (cheap, no
/// resolution needed).
///
/// Delegates to the shared `crate::util::infer_type_from_literal()`
/// for basic scalar/null/string/empty-array literals, then handles
/// extended cases (array literals with elements, `new ClassName()`).
///
/// Returns `None` for anything that isn't a simple literal — the
/// caller should fall back to the full type resolver for those.
fn infer_type_from_literal(expr: &str) -> Option<PhpType> {
    // Try the shared utility for basic literals.
    if let Some(t) = crate::util::infer_type_from_literal(expr) {
        return Some(t);
    }

    // Array literal with elements.
    if expr.starts_with('[') && expr.ends_with(']') {
        return infer_array_literal_type(&expr[1..expr.len() - 1]);
    }
    if expr.starts_with("array(") && expr.ends_with(')') {
        return infer_array_literal_type(&expr[6..expr.len() - 1]);
    }

    // `new ClassName(...)` — extract the class name.
    if let Some(rest) = expr.strip_prefix("new ") {
        let class_name = rest
            .split(|c: char| c == '(' || c.is_whitespace())
            .next()
            .unwrap_or("")
            .trim();
        if !class_name.is_empty() {
            return Some(PhpType::Named(class_name.to_string()));
        }
    }

    // Not a literal — caller should use the full resolver.
    None
}

/// Infer a type from the comma-separated contents of an array literal.
///
/// Handles simple cases where all elements are the same scalar type
/// (e.g. `['a', 'b']` → `list<string>`, `[1, 2, 3]` → `list<int>`).
/// Key-value pairs with string keys produce `array<string, V>`.
/// Falls back to `array` when elements are mixed or too complex.
fn infer_array_literal_type(inner: &str) -> Option<PhpType> {
    let inner = inner.trim();
    if inner.is_empty() {
        return Some(PhpType::array());
    }

    // Split on commas at the top level (not inside nested brackets,
    // parens, or strings).
    let elements = split_array_elements(inner);
    if elements.is_empty() {
        return Some(PhpType::array());
    }

    let mut value_types: Vec<PhpType> = Vec::new();
    let mut has_string_keys = false;
    let mut has_int_keys = false;

    for elem in &elements {
        let elem = elem.trim();
        if elem.is_empty() {
            continue;
        }

        // Check for key => value syntax.
        if let Some(arrow_pos) = find_top_level_arrow(elem) {
            let key = elem[..arrow_pos].trim();
            let value = elem[arrow_pos + 2..].trim();

            if (key.starts_with('\'') && key.ends_with('\''))
                || (key.starts_with('"') && key.ends_with('"'))
            {
                has_string_keys = true;
            } else if key.parse::<i64>().is_ok() {
                has_int_keys = true;
            } else {
                // Complex key expression — bail.
                return Some(PhpType::array());
            }

            match infer_type_from_literal(value) {
                Some(t) => value_types.push(t),
                None => return Some(PhpType::array()),
            }
        } else {
            // Sequential element (no key).
            match infer_type_from_literal(elem) {
                Some(t) => value_types.push(t),
                None => return Some(PhpType::array()),
            }
        }
    }

    if value_types.is_empty() {
        return Some(PhpType::array());
    }

    // Deduplicate value types.
    let mut deduped: Vec<PhpType> = Vec::with_capacity(value_types.len());
    for ty in &value_types {
        if !deduped.iter().any(|existing| existing.equivalent(ty)) {
            deduped.push(ty.clone());
        }
    }

    let value_union_type = if deduped.len() > 3 {
        PhpType::mixed()
    } else if deduped.len() == 1 {
        deduped.into_iter().next().unwrap()
    } else {
        PhpType::Union(deduped)
    };

    if has_string_keys && !has_int_keys {
        Some(PhpType::generic_array(PhpType::string(), value_union_type))
    } else if has_string_keys {
        // Mixed key types — just use array with value type.
        Some(PhpType::generic_array_val(value_union_type))
    } else {
        Some(PhpType::list(value_union_type))
    }
}

/// Split array element text on top-level commas (not inside nested
/// brackets, parentheses, or string literals).
fn split_array_elements(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut start = 0;

    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            '[' | '(' if !in_single_quote && !in_double_quote => depth += 1,
            ']' | ')' if !in_single_quote && !in_double_quote => depth -= 1,
            ',' if depth == 0 && !in_single_quote && !in_double_quote => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            '\\' if in_single_quote || in_double_quote => {
                // Skip escaped character inside strings.
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < s.len() {
        parts.push(&s[start..]);
    }
    parts
}

/// Find the position of `=>` at the top level of an array element
/// (not inside nested brackets, parens, or strings).
fn find_top_level_arrow(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            '[' | '(' if !in_single_quote && !in_double_quote => depth += 1,
            ']' | ')' if !in_single_quote && !in_double_quote => depth -= 1,
            '=' if depth == 0
                && !in_single_quote
                && !in_double_quote
                && i + 1 < bytes.len()
                && bytes[i + 1] == b'>' =>
            {
                return Some(i);
            }
            '\\' if in_single_quote || in_double_quote => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "inference_tests.rs"]
mod tests;
