//! "Replace deprecated call" code action.
//!
//! When the cursor is on a deprecated function call or method call that
//! has a `deprecated_replacement` template (from `#[Deprecated(replacement: "...")]`),
//! this module offers a code action to replace the call with the suggested
//! replacement.
//!
//! Template variables supported (matching phpstorm-stubs conventions):
//!
//! - `%parametersList%` — all arguments from the call site, comma-separated.
//! - `%parameter0%`, `%parameter1%`, … — individual arguments by index.
//! - `%class%` — the object/class expression for method call replacements.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::diagnostics::offset_range_to_lsp_range;
use crate::symbol_map::SymbolKind;
use crate::types::ClassInfo;
use crate::virtual_members::resolve_class_fully_cached;

/// File-level context needed for subject resolution.
///
/// Bundles the use-map, namespace, and local classes so that
/// [`resolve_subject_to_class`] stays under the argument limit.
struct FileCtx<'a> {
    use_map: &'a HashMap<String, String>,
    namespace: &'a Option<String>,
    local_classes: &'a [Arc<ClassInfo>],
}

impl Backend {
    /// Collect "Replace deprecated call" code actions for the cursor position.
    ///
    /// When the cursor overlaps a deprecated symbol that carries a
    /// `deprecated_replacement` template, this produces a code action
    /// that rewrites the call expression to the suggested replacement.
    pub(crate) fn collect_replace_deprecated_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let request_start =
            crate::text_position::position_to_byte_offset(content, params.range.start);
        let request_end = crate::text_position::position_to_byte_offset(content, params.range.end);

        let symbol_map = {
            let maps = self.symbol_maps.read();
            match maps.get(uri) {
                Some(sm) => sm.clone(),
                None => return,
            }
        };

        let file_use_map: HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);
        let local_classes: Vec<Arc<ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        let class_loader = self.class_loader_with(&local_classes, &file_use_map, &file_namespace);
        let cache = &self.resolved_class_cache;

        let file_ctx = FileCtx {
            use_map: &file_use_map,
            namespace: &file_namespace,
            local_classes: &local_classes,
        };

        for span in &symbol_map.spans {
            // Only consider spans that overlap with the request range.
            if (span.end as usize) < request_start || (span.start as usize) > request_end {
                continue;
            }

            match &span.kind {
                SymbolKind::FunctionCall { name, .. } => {
                    let func_info =
                        match self.resolve_function_name(name, &file_use_map, &file_namespace) {
                            Some(f) => f,
                            None => continue,
                        };

                    let replacement_template = match &func_info.deprecated_replacement {
                        Some(r) => r.clone(),
                        None => continue,
                    };

                    // Find the full call expression range including arguments.
                    let call_site = symbol_map
                        .call_sites
                        .iter()
                        .find(|cs| cs.args_start > span.start && cs.args_start <= span.end + 2);

                    let (call_range, args_text) = match call_site {
                        Some(cs) => {
                            // The call range spans from the function name start
                            // to the closing paren of the argument list.
                            let range = offset_range_to_lsp_range(
                                content,
                                span.start as usize,
                                cs.args_end as usize,
                            );
                            let args = extract_arguments(content, cs.args_start, cs.args_end);
                            (range, args)
                        }
                        None => {
                            // No call site found — can't replace without
                            // knowing the argument list boundaries.
                            continue;
                        }
                    };

                    let Some(range) = call_range else {
                        continue;
                    };

                    let replacement = expand_template(&replacement_template, &args_text, None);

                    let title = format!("Replace with {}", summarize_replacement(&replacement));
                    emit_action(uri, title, range, &replacement, out);
                }

                SymbolKind::MemberAccess {
                    subject_text,
                    member_name,
                    is_static,
                    is_method_call,
                    ..
                } => {
                    if !is_method_call {
                        continue;
                    }

                    // Resolve the subject to find the method's replacement template.
                    let base_class = resolve_subject_to_class(
                        subject_text,
                        *is_static,
                        &file_ctx,
                        span.start,
                        content,
                        self,
                    );

                    let base_class = match base_class {
                        Some(c) => c,
                        None => continue,
                    };

                    let resolved = resolve_class_fully_cached(&base_class, &class_loader, cache);

                    let method = base_class
                        .get_method(member_name)
                        .or_else(|| resolved.get_method(member_name));

                    let replacement_template =
                        match method.and_then(|m| m.deprecated_replacement.as_ref()) {
                            Some(r) => r.clone(),
                            None => continue,
                        };

                    // Find the call site for this method call.
                    let call_site = symbol_map
                        .call_sites
                        .iter()
                        .find(|cs| cs.args_start > span.start && cs.args_start <= span.end + 2);

                    let (call_range, args_text) = match call_site {
                        Some(cs) => {
                            let args = extract_arguments(content, cs.args_start, cs.args_end);

                            // When the template contains %class%, the
                            // expanded replacement already includes the
                            // subject expression, so the edit range must
                            // cover the full `$subject->method(...)`, not
                            // just `method(...)`.
                            let range_start = if replacement_template.contains("%class%") {
                                find_subject_start(content, span.start as usize, *is_static)
                            } else {
                                span.start as usize
                            };

                            let range = offset_range_to_lsp_range(
                                content,
                                range_start,
                                cs.args_end as usize,
                            );
                            (range, args)
                        }
                        None => continue,
                    };

                    let Some(range) = call_range else {
                        continue;
                    };

                    let subject = Some(subject_text.trim().to_string());
                    let replacement =
                        expand_template(&replacement_template, &args_text, subject.as_deref());

                    let title = format!("Replace with {}", summarize_replacement(&replacement));
                    emit_action(uri, title, range, &replacement, out);
                }

                _ => {}
            }
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Emit a single code action that replaces `range` with `replacement`.
fn emit_action(
    uri: &str,
    title: String,
    range: Range,
    replacement: &str,
    out: &mut Vec<CodeActionOrCommand>,
) {
    let doc_uri: Url = match uri.parse() {
        Ok(u) => u,
        Err(_) => return,
    };

    let edit = TextEdit {
        range,
        new_text: replacement.to_string(),
    };

    let mut changes = HashMap::new();
    changes.insert(doc_uri, vec![edit]);

    out.push(CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(false),
        disabled: None,
        data: None,
    }));
}

/// Extract individual argument source texts from a call site.
///
/// `args_start` is the byte offset immediately after the opening `(`.
/// `args_end` is the byte offset of the closing `)`.
fn extract_arguments(content: &str, args_start: u32, args_end: u32) -> Vec<String> {
    let start = args_start as usize;
    // args_end points at the `)` character — use it directly as the
    // exclusive upper bound so we capture all argument text.
    let end = args_end as usize;

    if start >= content.len() || end <= start {
        return Vec::new();
    }

    let inner = &content[start..end.min(content.len())];
    if inner.trim().is_empty() {
        return Vec::new();
    }

    // Split on top-level commas (respecting parentheses, brackets, and strings).
    let mut args = Vec::new();
    let mut depth = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;
    let mut current_start = 0;

    for (i, ch) in inner.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && (in_single_quote || in_double_quote) {
            escape_next = true;
            continue;
        }
        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            continue;
        }
        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            continue;
        }
        if in_single_quote || in_double_quote {
            continue;
        }
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                args.push(inner[current_start..i].trim().to_string());
                current_start = i + 1;
            }
            _ => {}
        }
    }

    // Last argument (or only argument if no commas).
    let last = inner[current_start..].trim();
    if !last.is_empty() {
        args.push(last.to_string());
    }

    args
}

/// Expand a replacement template by substituting template variables.
///
/// - `%parametersList%` → all arguments joined with `, `.
/// - `%parameter0%`, `%parameter1%`, … → individual argument by index.
/// - `%class%` → the subject expression (for method calls).
fn expand_template(template: &str, args: &[String], subject: Option<&str>) -> String {
    let mut result = template.to_string();

    // Replace %parametersList% with all arguments.
    if result.contains("%parametersList%") {
        let all_args = args.join(", ");
        result = result.replace("%parametersList%", &all_args);
    }

    // Replace %class% with the subject expression.
    if result.contains("%class%") {
        let class_text = subject.unwrap_or("$this");
        result = result.replace("%class%", class_text);
    }

    // Replace %parameterN% with individual arguments.
    for (i, arg) in args.iter().enumerate() {
        let placeholder = format!("%parameter{}%", i);
        if result.contains(&placeholder) {
            result = result.replace(&placeholder, arg);
        }
    }

    // Clean up any remaining unreplaced parameter placeholders
    // (when the call has fewer arguments than the template expects).
    // Replace with empty string to avoid leaving broken template text.
    let mut cleaned = String::with_capacity(result.len());
    let mut chars = result.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            // Check if this starts a %parameterN% placeholder.
            let mut placeholder = String::from('%');
            let mut found_end = false;
            for next in chars.by_ref() {
                placeholder.push(next);
                if next == '%' {
                    found_end = true;
                    break;
                }
                // Bail if the placeholder gets too long (not a real placeholder).
                if placeholder.len() > 20 {
                    break;
                }
            }
            if found_end && placeholder.starts_with("%parameter") && placeholder.ends_with('%') {
                // This is an unresolved %parameterN% — drop it.
                continue;
            }
            cleaned.push_str(&placeholder);
        } else {
            cleaned.push(ch);
        }
    }

    cleaned
}

/// Produce a short summary of the replacement for the code action title.
///
/// Truncates long replacements to avoid unwieldy titles.
fn summarize_replacement(replacement: &str) -> String {
    if replacement.len() <= 60 {
        format!("`{}`", replacement)
    } else {
        format!("`{}…`", &replacement[..57])
    }
}

/// Find the byte offset where the full member-access expression starts,
/// scanning backward from the member name past the arrow (or `::`) and
/// the subject expression.
///
/// For `$src->legacySetTimezone(...)` where `member_start` points at the
/// `l`, this returns the offset of `$`.
fn find_subject_start(content: &str, member_start: usize, is_static: bool) -> usize {
    let bytes = content.as_bytes();

    // Step 1: skip backward past the arrow/operator.
    let mut pos = member_start;
    if pos == 0 {
        return 0;
    }

    // Skip any whitespace between operator and member name.
    while pos > 0 && bytes[pos - 1].is_ascii_whitespace() {
        pos -= 1;
    }

    // Skip the operator itself: `->`, `?->`, or `::`.
    if is_static {
        if pos >= 2 && &content[pos - 2..pos] == "::" {
            pos -= 2;
        }
    } else if pos >= 2 && &content[pos - 2..pos] == "->" {
        pos -= 2;
        // Also handle `?->` (nullsafe).
        if pos > 0 && bytes[pos - 1] == b'?' {
            pos -= 1;
        }
    }

    // Skip any whitespace between subject and operator.
    while pos > 0 && bytes[pos - 1].is_ascii_whitespace() {
        pos -= 1;
    }

    // Step 2: scan backward over the subject expression.
    // Handle closing `)` or `]` by balancing brackets.
    if pos > 0 && (bytes[pos - 1] == b')' || bytes[pos - 1] == b']') {
        let close = bytes[pos - 1];
        let open = if close == b')' { b'(' } else { b'[' };
        let mut depth = 1;
        pos -= 1;
        while pos > 0 && depth > 0 {
            pos -= 1;
            if bytes[pos] == close {
                depth += 1;
            } else if bytes[pos] == open {
                depth -= 1;
            }
        }
        // Continue scanning backward for the identifier/expression
        // before the parenthesized part (e.g. `func()` or `$arr[]`).
        while pos > 0
            && (bytes[pos - 1].is_ascii_alphanumeric()
                || bytes[pos - 1] == b'_'
                || bytes[pos - 1] == b'$'
                || bytes[pos - 1] == b'\\')
        {
            pos -= 1;
        }
        // Handle chained `->` / `?->` / `::` before the identifier.
        let mut chain_pos = pos;
        while chain_pos > 0 && bytes[chain_pos - 1].is_ascii_whitespace() {
            chain_pos -= 1;
        }
        if chain_pos >= 2
            && (&content[chain_pos - 2..chain_pos] == "->"
                || &content[chain_pos - 2..chain_pos] == "::")
        {
            // There's a deeper chain — recurse from the current pos.
            return find_subject_start(content, pos, false);
        }
        if chain_pos >= 3 && &content[chain_pos - 3..chain_pos] == "?->" {
            return find_subject_start(content, pos, false);
        }
    } else {
        // Simple identifier: `$var`, `self`, `static`, `ClassName`, etc.
        while pos > 0
            && (bytes[pos - 1].is_ascii_alphanumeric()
                || bytes[pos - 1] == b'_'
                || bytes[pos - 1] == b'$'
                || bytes[pos - 1] == b'\\')
        {
            pos -= 1;
        }
    }

    pos
}

/// Resolve a member access subject to a `ClassInfo`.
///
/// Handles `self`, `static`, `parent`, `$this`, `ClassName`, and
/// `$variable` subjects.  For variables, delegates to the variable
/// type resolution pipeline.
fn resolve_subject_to_class(
    subject_text: &str,
    is_static: bool,
    ctx: &FileCtx<'_>,
    access_offset: u32,
    content: &str,
    backend: &Backend,
) -> Option<ClassInfo> {
    let class_loader = backend.class_loader_with(ctx.local_classes, ctx.use_map, ctx.namespace);
    let function_loader = backend.function_loader_with(None, ctx.use_map, ctx.namespace);
    let res_ctx = crate::subject_resolution::SubjectResolutionCtx {
        local_classes: ctx.local_classes,
        use_map: ctx.use_map,
        namespace: ctx.namespace,
        content,
        class_loader: &class_loader,
        function_loader: &function_loader,
    };

    let php_type = crate::subject_resolution::resolve_subject_type(
        subject_text,
        is_static,
        access_offset,
        &res_ctx,
    )?;

    // Load the first class from the resolved type.
    php_type
        .top_level_class_names()
        .into_iter()
        .find_map(|name| backend.find_or_load_class(&name))
        .map(|arc| ClassInfo::clone(&arc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_template_parameters_list() {
        let args = vec!["$a".to_string(), "$b".to_string()];
        let result = expand_template("new_func(%parametersList%)", &args, None);
        assert_eq!(result, "new_func($a, $b)");
    }

    #[test]
    fn expand_template_individual_parameters() {
        let args = vec!["$dict".to_string(), "$word".to_string()];
        let result = expand_template("enchant_dict_add(%parameter0%, %parameter1%)", &args, None);
        assert_eq!(result, "enchant_dict_add($dict, $word)");
    }

    #[test]
    fn expand_template_class_replacement() {
        let args = vec![];
        let result = expand_template("%class%->clear()", &args, Some("$cache"));
        assert_eq!(result, "$cache->clear()");
    }

    #[test]
    fn expand_template_class_with_params() {
        let args = vec!["$tz".to_string()];
        let result = expand_template(
            "%class%->setTimeZone(%parametersList%)",
            &args,
            Some("$fmt"),
        );
        assert_eq!(result, "$fmt->setTimeZone($tz)");
    }

    #[test]
    fn expand_template_no_args() {
        let args: Vec<String> = vec![];
        let result = expand_template("exif_read_data(%parametersList%)", &args, None);
        assert_eq!(result, "exif_read_data()");
    }

    #[test]
    fn expand_template_missing_parameter_placeholder() {
        // When the call has fewer args than the template expects,
        // unreplaced %parameterN% placeholders are dropped.
        let args = vec!["$a".to_string()];
        let result = expand_template("func(%parameter0%, %parameter1%)", &args, None);
        assert_eq!(result, "func($a, )");
    }

    #[test]
    fn expand_template_literal_replacement() {
        // Some replacements are just textual descriptions, not templates.
        let args = vec![];
        let result = expand_template(
            "mb_convert_encoding($s, 'ISO-8859-1', 'UTF-8')",
            &args,
            None,
        );
        assert_eq!(result, "mb_convert_encoding($s, 'ISO-8859-1', 'UTF-8')");
    }

    #[test]
    fn extract_arguments_empty() {
        let content = "foo()";
        // ( at 3, ) at 4 → args_start=4, args_end=4
        let args = extract_arguments(content, 4, 4);
        assert!(args.is_empty());
    }

    #[test]
    fn extract_arguments_single() {
        let content = "foo($x)";
        // ( at 3, ) at 6 → args_start=4, args_end=6
        let args = extract_arguments(content, 4, 6);
        assert_eq!(args, vec!["$x"]);
    }

    #[test]
    fn extract_arguments_multiple() {
        let content = "foo($a, $b, $c)";
        // ( at 3, ) at 14 → args_start=4, args_end=14
        let args = extract_arguments(content, 4, 14);
        assert_eq!(args, vec!["$a", "$b", "$c"]);
    }

    #[test]
    fn extract_arguments_nested_parens() {
        let content = "foo(bar($x), $y)";
        // ( at 3, outer ) at 15 → args_start=4, args_end=15
        let args = extract_arguments(content, 4, 15);
        assert_eq!(args, vec!["bar($x)", "$y"]);
    }

    #[test]
    fn extract_arguments_string_with_comma() {
        let content = r#"foo("a,b", $y)"#;
        // ( at 3, ) at 13 → args_start=4, args_end=13
        let args = extract_arguments(content, 4, 13);
        assert_eq!(args, vec![r#""a,b""#, "$y"]);
    }

    #[test]
    fn summarize_short() {
        assert_eq!(summarize_replacement("foo()"), "`foo()`");
    }

    #[test]
    fn summarize_long() {
        let long = "a".repeat(80);
        let summary = summarize_replacement(&long);
        assert!(summary.ends_with("…`"));
        assert!(summary.len() < 70);
    }

    // ── find_subject_start ──────────────────────────────────────────

    #[test]
    fn find_subject_start_simple_variable() {
        let content = "$src->legacySetTimezone('UTC')";
        // member_start points at 'l' in legacySetTimezone (offset 6)
        let start = find_subject_start(content, 6, false);
        assert_eq!(start, 0);
        assert_eq!(&content[start..6], "$src->");
    }

    #[test]
    fn find_subject_start_static_call() {
        let content = "MyClass::oldMethod()";
        // member_start points at 'o' in oldMethod (offset 9)
        let start = find_subject_start(content, 9, true);
        assert_eq!(start, 0);
        assert_eq!(&content[start..9], "MyClass::");
    }

    #[test]
    fn find_subject_start_nullsafe() {
        let content = "$obj?->legacyMethod()";
        // member_start points at 'l' in legacyMethod (offset 7)
        let start = find_subject_start(content, 7, false);
        assert_eq!(start, 0);
        assert_eq!(&content[start..7], "$obj?->");
    }

    #[test]
    fn find_subject_start_this() {
        let content = "$this->legacyMethod()";
        let start = find_subject_start(content, 7, false);
        assert_eq!(start, 0);
        assert_eq!(&content[start..7], "$this->");
    }

    #[test]
    fn find_subject_start_with_prefix_code() {
        // Subject is not at the start of the content.
        let content = "if (true) { $src->legacyMethod() }";
        // $src starts at 12, -> at 16, legacyMethod at 18
        let start = find_subject_start(content, 18, false);
        assert_eq!(start, 12);
        assert_eq!(&content[start..18], "$src->");
    }

    #[test]
    fn find_subject_start_namespaced_static() {
        let content = "App\\Models\\User::oldMethod()";
        // :: at 15..17, oldMethod at 17
        let start = find_subject_start(content, 17, true);
        assert_eq!(start, 0);
        assert_eq!(&content[start..17], "App\\Models\\User::");
    }
}
