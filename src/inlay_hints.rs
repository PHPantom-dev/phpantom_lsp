//! Inlay hints (`textDocument/inlayHint`).
//!
//! Displays inline annotations in the editor for:
//! - **Parameter name hints** at call sites (e.g. `/*needle:*/ $x`).
//! - **By-reference indicators** for arguments passed by reference (`&`).
//! - **Closure parameter type hints** for untyped closure/arrow function
//!   parameters when the type can be inferred from the callable context.
//! - **Closure return type hints** for closures/arrow functions without an
//!   explicit return type when the callable context specifies one.
//!
//! The handler walks precomputed [`CallSite`] entries from the
//! [`SymbolMap`] within the requested viewport range, resolves each
//! callable to obtain parameter metadata, and emits [`InlayHint`]
//! entries for arguments that would benefit from a label.

use tower_lsp::jsonrpc;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::{CallSite, UntypedClosureSite};
use crate::types::FileContext;
use crate::util::{offset_to_position, position_to_offset};

impl Backend {
    /// Entry point for the `textDocument/inlayHint` request.
    ///
    /// Called by the native [`LanguageServer::inlay_hint`] trait method
    /// (available since `tower-lsp` 0.19).
    pub async fn inlay_hint_request(
        &self,
        params: InlayHintParams,
    ) -> jsonrpc::Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri.to_string();
        let range = params.range;
        let result = self.with_file_content("textDocument/inlayHint", &uri, None, |content, _| {
            self.handle_inlay_hints(&uri, content, range)
        });

        Ok(result.flatten())
    }

    /// Handle a `textDocument/inlayHint` request.
    ///
    /// Returns inlay hints for call-site parameter names and by-reference
    /// indicators within the given range.
    pub fn handle_inlay_hints(
        &self,
        uri: &str,
        content: &str,
        range: Range,
    ) -> Option<Vec<InlayHint>> {
        let symbol_map = self.symbol_maps.read().get(uri).cloned()?;
        let ctx = self.file_context(uri);

        // If this is a Blade file, the `range` is in Blade coordinates.
        // We must translate it to virtual PHP coordinates before comparing
        // against offsets in the symbol map.
        let virtual_range = if self.is_blade_file(uri) {
            Range {
                start: self.translate_blade_to_php(uri, range.start),
                end: self.translate_blade_to_php(uri, range.end),
            }
        } else {
            range
        };

        let range_start = position_to_offset(content, virtual_range.start);
        let range_end = position_to_offset(content, virtual_range.end);

        let mut hints = Vec::new();

        for call_site in &symbol_map.call_sites {
            // Skip call sites entirely outside the requested range.
            if call_site.args_end < range_start || call_site.args_start > range_end {
                continue;
            }

            // Skip calls with no arguments.
            if call_site.arg_count == 0 {
                continue;
            }

            self.emit_parameter_hints(call_site, content, range, &ctx, &mut hints);
        }

        // ── Closure / arrow function hints ──────────────────────────
        if !symbol_map.untyped_closure_sites.is_empty() {
            self.emit_closure_hints(
                content,
                &symbol_map.untyped_closure_sites,
                &symbol_map.call_sites,
                (range_start, range_end),
                &ctx,
                &mut hints,
            );
        }

        // Translate hints back to Blade if needed.
        if self.is_blade_file(uri) {
            for hint in &mut hints {
                hint.position = self.translate_php_to_blade(uri, hint.position);
            }
        }

        Some(hints)
    }

    /// Emit parameter-name and by-reference hints for a single call site.
    fn emit_parameter_hints(
        &self,
        call_site: &CallSite,
        content: &str,
        range: Range,
        ctx: &FileContext,
        hints: &mut Vec<InlayHint>,
    ) {
        // Build a synthetic position from the call site's start so that
        // resolve_callable_target has a cursor context.
        let position = offset_to_position(content, call_site.args_start as usize);

        let resolved = match self.resolve_callable_target(
            &call_site.call_expression,
            content,
            position,
            ctx,
        ) {
            Some(r) => r,
            None => return,
        };

        let params = &resolved.parameters;
        if params.is_empty() {
            return;
        }

        let range_start = position_to_offset(content, range.start);
        let range_end = position_to_offset(content, range.end);

        // Build a set of parameter names consumed by named arguments so
        // positional arguments can be mapped to the remaining parameters.
        let named_consumed: std::collections::HashSet<&str> = call_site
            .named_arg_names
            .iter()
            .map(|n| n.as_str())
            .collect();

        // Parameters not consumed by named args, in declaration order.
        // Each positional argument is assigned to the next entry in this
        // list.  For variadic parameters the last entry is reused.
        let remaining_params: Vec<usize> = params
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let name = p.name.strip_prefix('$').unwrap_or(&p.name);
                !named_consumed.contains(name)
            })
            .map(|(i, _)| i)
            .collect();

        let mut positional_counter: usize = 0;

        for (arg_idx, &arg_offset) in call_site.arg_offsets.iter().enumerate() {
            // Skip arguments outside the viewport range.
            if arg_offset < range_start || arg_offset > range_end {
                continue;
            }

            // Skip named arguments — the parameter name is already visible.
            if call_site.named_arg_indices.contains(&(arg_idx as u32)) {
                continue;
            }

            // Skip spread arguments — a single `...$args` may expand into
            // multiple parameters, so any single parameter name would be
            // misleading.  Still advance the positional counter because
            // the spread occupies at least one parameter slot.
            if call_site.spread_arg_indices.contains(&(arg_idx as u32)) {
                positional_counter += 1;
                continue;
            }

            // Determine which parameter this positional argument corresponds
            // to. Named arguments consume specific parameters out of order,
            // so positional arguments fill the remaining slots sequentially.
            let param_idx = if positional_counter < remaining_params.len() {
                remaining_params[positional_counter]
            } else if params.last().is_some_and(|p| p.is_variadic) {
                params.len() - 1
            } else {
                // More positional arguments than remaining parameters and
                // the last param is not variadic. Skip (likely a bug in
                // user code; we don't hint).
                positional_counter += 1;
                continue;
            };

            positional_counter += 1;

            let param = &params[param_idx];

            // Build the hint label parts.
            let mut label_parts: Vec<String> = Vec::new();

            // By-reference indicator.
            if param.is_reference {
                label_parts.push("&".to_string());
            }

            // Parameter name hint.
            // Strip the `$` prefix for a cleaner display.
            let param_display_name = param.name.strip_prefix('$').unwrap_or(&param.name);

            // Skip the hint when the argument is a simple variable whose
            // name matches the parameter name (the hint would be redundant).
            // For example: `foo($needle)` when the param is `$needle`.
            if !param.is_reference && should_suppress_hint(param_display_name, content, arg_offset)
            {
                continue;
            }

            // For single-argument calls where the function name already
            // makes the parameter obvious, skip the hint.
            if !param.is_reference
                && call_site.arg_count == 1
                && is_obvious_single_param(&call_site.call_expression, param_display_name)
            {
                continue;
            }

            label_parts.push(format!("{}:", param_display_name));

            let label_text = label_parts.join("");
            if label_text.is_empty() {
                continue;
            }

            let hint_position = offset_to_position(content, arg_offset as usize);

            hints.push(InlayHint {
                position: hint_position,
                label: InlayHintLabel::String(label_text),
                kind: Some(InlayHintKind::PARAMETER),
                text_edits: None,
                tooltip: param
                    .type_hint
                    .as_ref()
                    .map(|t| InlayHintTooltip::String(format!("{} {}", t, param.name))),
                padding_left: None,
                padding_right: Some(true),
                data: None,
            });
        }
    }

    /// Emit a return-type inlay hint for a function / method / closure /
    /// arrow function that lacks an explicit return type declaration.
    /// Emit parameter-type and return-type inlay hints for closures and
    /// arrow functions whose types can be inferred from the callable context.
    fn emit_closure_hints(
        &self,
        content: &str,
        sites: &[UntypedClosureSite],
        call_sites: &[CallSite],
        range: (u32, u32),
        ctx: &FileContext,
        hints: &mut Vec<InlayHint>,
    ) {
        let (range_start, range_end) = range;
        for site in sites {
            // Quick range check: use close_paren_offset if available,
            // otherwise the first untyped param offset.
            let representative_offset = site
                .close_paren_offset
                .or_else(|| site.untyped_params.first().map(|&(_, off)| off));
            if let Some(off) = representative_offset {
                if off < range_start || off > range_end {
                    continue;
                }
            } else {
                continue;
            }

            // Find the matching CallSite so we can extract the full
            // argument text for template substitution.  We match by
            // call expression string and verify that any of the
            // closure site's offsets fall within the call site's
            // argument range.  We check ALL untyped-param offsets
            // and the close-paren offset since the representative
            // offset alone may not be inside the parent call's range
            // for all AST shapes.
            let call_args_text: Option<&str> = {
                let closure_offsets: Vec<u32> = site
                    .untyped_params
                    .iter()
                    .map(|&(_, off)| off)
                    .chain(site.close_paren_offset)
                    .collect();
                call_sites
                    .iter()
                    .find(|cs| {
                        cs.call_expression == site.parent_call_expression
                            && closure_offsets
                                .iter()
                                .any(|&off| off >= cs.args_start && off <= cs.args_end)
                    })
                    .and_then(|cs| content.get(cs.args_start as usize..cs.args_end as usize))
            };

            // Resolve the callable to get the parameter's type signature.
            // Pass the call-site argument text so that function/method-level
            // @template parameters are inferred from the sibling arguments
            // and substituted into parameter type hints (e.g. turning
            // `callable(T): T` into `callable(int): int`).
            let position = offset_to_position(content, representative_offset.unwrap_or(0) as usize);
            let resolved = match self.resolve_callable_target_with_args(
                &site.parent_call_expression,
                content,
                position,
                ctx,
                call_args_text,
            ) {
                Some(r) => r,
                None => continue,
            };

            let param_info = match resolved.parameters.get(site.arg_index_in_parent) {
                Some(p) => p,
                None => continue,
            };
            let callable_type = match param_info.type_hint.as_ref() {
                Some(t) => t,
                None => continue,
            };

            // ── Parameter type hints ────────────────────────────────
            if let Some(callable_params) = callable_type.callable_param_types() {
                for &(param_idx, param_offset) in &site.untyped_params {
                    if let Some(cp) = callable_params.get(param_idx) {
                        let shortened = cp.type_hint.shorten();
                        let type_str = shortened.to_string();
                        if type_str.is_empty() || shortened.is_mixed() {
                            continue;
                        }

                        let hint_position = offset_to_position(content, param_offset as usize);

                        hints.push(InlayHint {
                            position: hint_position,
                            label: InlayHintLabel::String(format!("{} ", type_str)),
                            kind: Some(InlayHintKind::TYPE),
                            text_edits: None,
                            tooltip: None,
                            padding_left: None,
                            padding_right: Some(false),
                            data: None,
                        });
                    }
                }
            }

            // ── Return type hint ────────────────────────────────────
            if let Some(close_paren) = site.close_paren_offset
                && let Some(ret_type) = callable_type.callable_return_type()
            {
                let shortened = ret_type.shorten();
                let type_str = shortened.to_string();
                if !type_str.is_empty() && !shortened.is_mixed() {
                    let hint_position = offset_to_position(content, close_paren as usize);

                    hints.push(InlayHint {
                        position: hint_position,
                        label: InlayHintLabel::String(format!(": {}", type_str)),
                        kind: Some(InlayHintKind::TYPE),
                        text_edits: None,
                        tooltip: None,
                        padding_left: None,
                        padding_right: None,
                        data: None,
                    });
                }
            }
        }
    }
}

/// Check whether the argument at `arg_offset` is a simple variable whose
/// name (without `$`) matches the parameter name, making a hint redundant.
///
/// Also suppresses hints when the argument is a property access or method
/// call whose trailing identifier matches the parameter name:
/// `foo($this->needle)` for param `$needle`.
fn should_suppress_hint(param_name: &str, content: &str, arg_offset: u32) -> bool {
    let rest = &content[arg_offset as usize..];

    // Case 1: Simple variable `$paramName`.
    if let Some(var_rest) = rest.strip_prefix('$') {
        let var_name: String = var_rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if eq_ignore_case_snake(&var_name, param_name) {
            return true;
        }
    }

    // Case 2: The argument text ends with `->paramName` or `?->paramName`.
    // Find the end of this argument (next comma or closing paren at depth 0).
    let arg_text = extract_argument_text(rest);
    if let Some(trailing) = extract_trailing_identifier(arg_text)
        && eq_ignore_case_snake(trailing, param_name)
    {
        return true;
    }

    // Case 3: Boolean/null literals matching the parameter name pattern.
    // `foo(true)` for param `$enabled`, `foo(null)` for param `$default`.
    let trimmed = arg_text.trim();
    if matches!(
        trimmed,
        "true" | "false" | "null" | "TRUE" | "FALSE" | "NULL"
    ) {
        return false;
    }

    // Case 4: String literal whose content matches param name.
    // `foo('needle')` for param `$needle`.
    if (trimmed.starts_with('\'') || trimmed.starts_with('"')) && trimmed.len() >= 2 {
        let quote = trimmed.as_bytes()[0];
        if trimmed.as_bytes().last() == Some(&quote) {
            let inner = &trimmed[1..trimmed.len() - 1];
            if eq_ignore_case_snake(inner, param_name) {
                return true;
            }
        }
    }

    false
}

/// Extract the argument text up to the next top-level comma or closing
/// paren, respecting nesting of `()`, `[]`, and `{}`.
fn extract_argument_text(s: &str) -> &str {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut prev_was_escape = false;

    for (i, ch) in s.char_indices() {
        if prev_was_escape {
            prev_was_escape = false;
            continue;
        }
        if ch == '\\' && (in_single_quote || in_double_quote) {
            prev_was_escape = true;
            continue;
        }
        if in_single_quote {
            if ch == '\'' {
                in_single_quote = false;
            }
            continue;
        }
        if in_double_quote {
            if ch == '"' {
                in_double_quote = false;
            }
            continue;
        }
        match ch {
            '\'' => in_single_quote = true,
            '"' => in_double_quote = true,
            '(' => depth_paren += 1,
            ')' => {
                if depth_paren == 0 {
                    return &s[..i];
                }
                depth_paren -= 1;
            }
            '[' => depth_bracket += 1,
            ']' => depth_bracket = (depth_bracket - 1).max(0),
            '{' => depth_brace += 1,
            '}' => depth_brace = (depth_brace - 1).max(0),
            ',' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                return &s[..i];
            }
            _ => {}
        }
    }
    s
}

/// Extract the trailing identifier from a member-access expression.
/// For `$this->foo->bar`, returns `"bar"`.
/// For `SomeClass::method`, returns `"method"`.
fn extract_trailing_identifier(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    // Look for `->identifier` or `::identifier` at the end.
    let pos = trimmed.rfind("->").or_else(|| trimmed.rfind("::"))?;
    let after = &trimmed[pos + 2..];
    // The trailing part should be a simple identifier.
    if after.chars().all(|c| c.is_alphanumeric() || c == '_') && !after.is_empty() {
        Some(after)
    } else {
        None
    }
}

/// Compare two identifiers ignoring case and treating snake_case
/// as equivalent to camelCase.
///
/// For example, `eq_ignore_case_snake("myParam", "my_param")` returns true.
fn eq_ignore_case_snake(a: &str, b: &str) -> bool {
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    // Normalize both to lowercase without underscores and compare.
    let norm_a: String = a
        .chars()
        .filter(|c| *c != '_')
        .flat_map(|c| c.to_lowercase())
        .collect();
    let norm_b: String = b
        .chars()
        .filter(|c| *c != '_')
        .flat_map(|c| c.to_lowercase())
        .collect();
    norm_a == norm_b
}

/// Check whether a single-parameter call has an obvious relationship
/// between the function/method name and the parameter, making the hint
/// redundant noise.
///
/// For example, `strlen($text)` — the function name already implies
/// the parameter is a string.
fn is_obvious_single_param(call_expression: &str, _param_name: &str) -> bool {
    // Extract the function/method name from the call expression.
    let func_name = if let Some(pos) = call_expression.rfind("->") {
        &call_expression[pos + 2..]
    } else if let Some(pos) = call_expression.rfind("::") {
        &call_expression[pos + 2..]
    } else if let Some(name) = call_expression.strip_prefix("new ") {
        // Constructor calls: `new Foo($bar)` — always show.
        let _ = name;
        return false;
    } else {
        call_expression
    };

    // Common single-param functions where the hint is noise.
    matches!(
        func_name.to_ascii_lowercase().as_str(),
        "count"
            | "strlen"
            | "isset"
            | "empty"
            | "unset"
            | "print"
            | "echo"
            | "var_dump"
            | "print_r"
            | "var_export"
            | "intval"
            | "floatval"
            | "strval"
            | "boolval"
            | "trim"
            | "ltrim"
            | "rtrim"
            | "strtolower"
            | "strtoupper"
            | "ucfirst"
            | "lcfirst"
            | "abs"
            | "ceil"
            | "floor"
            | "round"
            | "is_null"
            | "is_array"
            | "is_string"
            | "is_int"
            | "is_integer"
            | "is_float"
            | "is_double"
            | "is_bool"
            | "is_numeric"
            | "is_object"
            | "is_callable"
            | "json_encode"
            | "json_decode"
            | "serialize"
            | "unserialize"
            | "base64_encode"
            | "base64_decode"
            | "urlencode"
            | "urldecode"
            | "rawurlencode"
            | "rawurldecode"
            | "htmlspecialchars"
            | "htmlentities"
            | "md5"
            | "sha1"
            | "crc32"
            | "chr"
            | "ord"
            | "array_values"
            | "array_keys"
            | "array_unique"
            | "array_flip"
            | "array_reverse"
            | "array_pop"
            | "array_shift"
            | "sort"
            | "rsort"
            | "asort"
            | "arsort"
            | "ksort"
            | "krsort"
            | "shuffle"
            | "reset"
            | "end"
            | "current"
            | "next"
            | "prev"
            | "type"
            | "gettype"
            | "class_exists"
            | "interface_exists"
            | "trait_exists"
            | "function_exists"
            | "defined"
            | "compact"
            | "sizeof"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_suppress_simple_variable_match() {
        let content = "$needle, $haystack";
        assert!(should_suppress_hint("needle", content, 0));
    }

    #[test]
    fn test_should_not_suppress_different_variable() {
        let content = "$foo, $bar";
        assert!(!should_suppress_hint("needle", content, 0));
    }

    #[test]
    fn test_should_suppress_property_access_match() {
        let content = "$this->needle, $other";
        assert!(should_suppress_hint("needle", content, 0));
    }

    #[test]
    fn test_should_suppress_string_literal_match() {
        let content = "'needle', $other";
        assert!(should_suppress_hint("needle", content, 0));
    }

    #[test]
    fn test_should_not_suppress_boolean_literal() {
        let content = "true, $other";
        assert!(!should_suppress_hint("enabled", content, 0));
    }

    #[test]
    fn test_extract_argument_text_basic() {
        assert_eq!(extract_argument_text("$x, $y)"), "$x");
        assert_eq!(extract_argument_text("$x)"), "$x");
        assert_eq!(extract_argument_text("foo($a, $b), $c)"), "foo($a, $b)");
    }

    #[test]
    fn test_extract_trailing_identifier() {
        assert_eq!(extract_trailing_identifier("$this->foo"), Some("foo"));
        assert_eq!(extract_trailing_identifier("$obj->bar->baz"), Some("baz"));
        assert_eq!(
            extract_trailing_identifier("SomeClass::method"),
            Some("method")
        );
        assert_eq!(extract_trailing_identifier("$simple"), None);
    }

    #[test]
    fn test_eq_ignore_case_snake() {
        assert!(eq_ignore_case_snake("myParam", "myParam"));
        assert!(eq_ignore_case_snake("myParam", "myparam"));
        assert!(eq_ignore_case_snake("my_param", "myParam"));
        assert!(eq_ignore_case_snake("myParam", "my_param"));
        assert!(!eq_ignore_case_snake("foo", "bar"));
    }

    #[test]
    fn test_is_obvious_single_param() {
        assert!(is_obvious_single_param("strlen", "string"));
        assert!(is_obvious_single_param("count", "array"));
        assert!(is_obvious_single_param("json_encode", "value"));
        assert!(!is_obvious_single_param("customFunc", "value"));
        assert!(!is_obvious_single_param("new Foo", "bar"));
    }
}
