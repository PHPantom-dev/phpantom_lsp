use super::*;

// ─── Code generation ────────────────────────────────────────────────────────

/// Information gathered for code generation.
pub(crate) struct ExtractionInfo {
    /// The name of the new function/method.
    pub(crate) name: String,
    /// Parameters: `(var_name_with_dollar, cleaned_type_hint)`.
    pub(crate) params: Vec<(String, PhpType)>,
    /// Return values: `(var_name_with_dollar, cleaned_type_hint)`.
    pub(crate) returns: Vec<(String, PhpType)>,
    /// The selected statements as source text.
    pub(crate) body: String,
    /// Whether to extract as method or function.
    pub(crate) target: ExtractionTarget,
    /// Whether the enclosing method is static.
    pub(crate) is_static: bool,
    /// Indentation of the member level (for methods) or top level (for functions).
    pub(crate) member_indent: String,
    /// Indentation of the body inside the new function/method.
    pub(crate) body_indent: String,
    /// How return statements in the selection are handled.
    pub(crate) return_strategy: ReturnStrategy,
    /// Return type hint for the trailing return (resolved from the
    /// enclosing function's return type or the return expression).
    pub(crate) trailing_return_type: PhpType,
    /// Pre-computed PHPDoc block (including `/**` … `*/\n`) to prepend
    /// before the function definition, or empty if no enrichment needed.
    pub(crate) docblock: String,
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
pub(crate) fn build_docblock_for_extraction(
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
pub(crate) fn build_extracted_definition(info: &ExtractionInfo) -> String {
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
pub(crate) fn rewrite_guard_returns(body: &str, uniform_value: Option<&str>) -> String {
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
pub(crate) fn rewrite_void_returns_to_null(body: &str) -> String {
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
pub(crate) fn build_param_list(params: &[(String, PhpType)]) -> String {
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
pub(crate) fn build_return_type(info: &ExtractionInfo) -> String {
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
pub(crate) fn build_call_site(info: &ExtractionInfo, call_indent: &str) -> String {
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
