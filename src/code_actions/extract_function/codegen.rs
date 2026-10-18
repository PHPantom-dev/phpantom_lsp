use super::*;
use crate::php_type::TypeKind;

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
    use_map: &std::collections::HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    let mut tags: Vec<String> = Vec::new();
    let shorten = |ty: &PhpType| {
        ty.resolve_names(&|n| crate::names::shorten_from_fqn(n, use_map, file_namespace))
    };

    // Collect @param tags that need enrichment.
    for (name, type_hint, raw) in params {
        let has_native_hint = type_hint.to_native_hint().is_some_and(|s| !s.is_empty());
        if !has_native_hint && raw.is_empty() {
            continue;
        }
        // Prefer the raw resolved type when it carries concrete generics.
        if raw.has_type_structure() {
            tags.push(format!("@param {} {}", shorten(raw), name));
            continue;
        }
        let type_for_enrichment = if has_native_hint { type_hint } else { raw };
        if let Some(enriched) = enrichment_plain(
            Some(type_for_enrichment),
            class_loader,
            use_map,
            file_namespace,
        ) {
            tags.push(format!("@param {} {}", enriched, name));
        }
    }

    // Collect @return tag if the return type needs enrichment.
    if !return_type_hint.is_empty() || !raw_return_type.is_empty() {
        if raw_return_type.has_type_structure() {
            tags.push(format!("@return {}", shorten(raw_return_type)));
        } else {
            let hint = if return_type_hint.is_empty() {
                raw_return_type
            } else {
                return_type_hint
            };
            if let Some(enriched) =
                enrichment_plain(Some(hint), class_loader, use_map, file_namespace)
            {
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

    out.push_str(&info.member_indent);
    if info.target == ExtractionTarget::Method {
        out.push_str("private ");
        if info.is_static {
            out.push_str("static ");
        }
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

    // Rewrite guard returns in the body if needed.
    let body_text = match &info.return_strategy {
        ReturnStrategy::VoidGuards => {
            // Bare `return;` → `return false;` (false = early exit).
            rewrite_guard_returns(&info.body, |value| value.is_none(), "false")
        }
        ReturnStrategy::UniformGuards(value) => {
            let lower = value.to_lowercase();
            if lower == "false" || lower == "true" {
                // Already boolean — the body's returns are correct as-is.
                info.body.clone()
            } else {
                // Non-boolean uniform value (e.g. `null`, `0`, `'error'`):
                // rewrite `return <value>;` → `return false;`.
                let value = value.trim();
                rewrite_guard_returns(
                    &info.body,
                    |returned| {
                        returned.is_some_and(|returned| returned.eq_ignore_ascii_case(value))
                    },
                    "false",
                )
            }
        }
        ReturnStrategy::NullGuardWithValue(void_guards) if *void_guards => {
            // Bare `return;` → `return null;` so the extracted
            // function returns null on guard-fire.
            rewrite_guard_returns(&info.body, |value| value.is_none(), "null")
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
            // uses bool: guards become `return false;` (exit, already
            // rewritten above by `rewrite_guard_returns`), and
            // fall-through is `return true;` (continue).  Here we just
            // add the fall-through sentinel.
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

/// Rewrite each `return` the selected statements in `body` own, and that
/// `is_guard` picks by the value it returns (`None` for a bare `return;`),
/// to `return <value>;`.
///
/// A return inside a nested function or closure belongs to it and is left
/// alone, and so is anything that only reads like a return: a string, a
/// comment, or an identifier such as `$returnValue`.
pub(crate) fn rewrite_guard_returns(
    body: &str,
    is_guard: impl Fn(Option<&str>) -> bool,
    value: &str,
) -> String {
    // The body is whole statements, so it parses on its own behind an
    // opening tag.
    const OPEN_TAG: &str = "<?php\n";
    let source = format!("{OPEN_TAG}{body}");
    let mut sites =
        crate::parser::with_parsed_program(&source, "extract_function", |program, content| {
            collect_own_returns(content, program.statements.iter())
                .into_iter()
                .filter(|own| is_guard(own.value))
                .map(|own| own.after_keyword)
                .collect::<Vec<_>>()
        });
    sites.sort_by_key(|site| site.start);

    let mut out = String::with_capacity(body.len() + sites.len() * (value.len() + 1));
    let mut copied = 0;
    for site in sites {
        out.push_str(&body[copied..site.start - OPEN_TAG.len()]);
        out.push(' ');
        out.push_str(value);
        copied = site.end - OPEN_TAG.len();
    }
    out.push_str(&body[copied..]);
    out
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
                && !matches!(cleaned.kind(), TypeKind::Nullable(_))
            {
                return PhpType::nullable(cleaned).to_string();
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
                        && !matches!(cleaned.kind(), TypeKind::Nullable(_))
                    {
                        return PhpType::nullable(cleaned).to_string();
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
