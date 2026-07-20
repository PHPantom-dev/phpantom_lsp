//! Array callable method completion.
//!
//! Detects when the cursor is inside the method-name string of a PHP
//! array callable — `[Class::class, '|']` or `[$object, '|']` — and
//! offers method name completions from the resolved class.
//!
//! This fires before the `InStringLiteral` suppression in the completion
//! pipeline, alongside array shape and Eloquent string completions.

use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::types::{AccessKind, ClassInfo, FileContext};
use crate::util::position_to_offset;

// ─── Context ────────────────────────────────────────────────────────────────

/// Context extracted when the cursor is inside the method-name string
/// of an array callable.
#[derive(Debug)]
pub(crate) struct ArrayCallableContext {
    /// The subject from the first array element.
    /// For `Class::class` this is the class name (e.g. `"SortableController"`).
    /// For `$var` this is the variable (e.g. `"$this"`).
    pub subject: String,
    /// Whether the first element is a `::class` constant (static access).
    pub is_static: bool,
    /// The partial method name the user has typed so far.
    pub prefix: String,
}

// ─── Detection ──────────────────────────────────────────────────────────────

/// Detect whether the cursor is inside the method-name string of an
/// array callable pattern.
///
/// Recognises patterns like:
///   - `[Controller::class, '|']`         — class constant, empty prefix
///   - `[Controller::class, 'ind|']`      — class constant, partial prefix
///   - `[$this, '|']`                     — variable, empty prefix
///   - `[$obj, 'han|']`                   — variable, partial prefix
///
/// Also works without a closing quote (user is still typing):
///   - `[Controller::class, '|`
///
/// Returns `None` if the cursor is not in such a context.
pub(crate) fn detect_array_callable_context(
    content: &str,
    position: Position,
) -> Option<ArrayCallableContext> {
    let cursor_offset = position_to_offset(content, position) as usize;
    let bytes = content.as_bytes();

    if cursor_offset == 0 || cursor_offset > bytes.len() {
        return None;
    }

    // ── Step 1: Find the opening quote before the cursor ────────────
    let mut quote_pos = None;
    let mut i = cursor_offset;
    while i > 0 {
        i -= 1;
        let ch = bytes[i];
        if ch == b'\'' || ch == b'"' {
            // Make sure this isn't an escaped quote.
            let mut backslashes = 0;
            let mut j = i;
            while j > 0 && bytes[j - 1] == b'\\' {
                backslashes += 1;
                j -= 1;
            }
            if backslashes % 2 == 0 {
                quote_pos = Some(i);
                break;
            }
        }
        // Stop at newlines — strings don't span lines.
        if ch == b'\n' {
            return None;
        }
    }

    let quote_pos = quote_pos?;
    let string_content_start = quote_pos + 1;

    // The partial text typed so far (the method name prefix).
    let prefix = content[string_content_start..cursor_offset].to_string();

    // Verify the prefix looks like a valid identifier prefix (or is empty).
    if !prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }

    // ── Step 2: Before the quote, expect comma ──────────────────────
    let before_quote = content[..quote_pos].trim_end();
    if !before_quote.ends_with(',') {
        return None;
    }
    let before_comma = before_quote[..before_quote.len() - 1].trim_end();

    // ── Step 3: Identify the first element ──────────────────────────
    // Either `Something::class` or `$variable`.
    let (subject, is_static, before_first_elem) =
        if let Some(stripped) = strip_class_const_suffix(before_comma) {
            // Found `::class` — extract the class name before it.
            let trimmed = stripped.trim_end();
            let class_name = extract_class_name_backwards(trimmed)?;
            let before_class = trimmed[..trimmed.len() - class_name.len()].trim_end();
            (class_name, true, before_class)
        } else {
            // Try matching a variable like `$this` or `$obj`.
            let var_name = extract_variable_backwards(before_comma)?;
            let before_var = before_comma[..before_comma.len() - var_name.len()].trim_end();
            (var_name, false, before_var)
        };

    // ── Step 4: Before the first element, expect `[` ────────────────
    if !before_first_elem.ends_with('[') {
        return None;
    }

    Some(ArrayCallableContext {
        subject,
        is_static,
        prefix,
    })
}

/// Strip a trailing `::class` (case-insensitive) from the text,
/// returning the portion before `::class`, or `None` if not present.
fn strip_class_const_suffix(text: &str) -> Option<&str> {
    let lower = text.to_ascii_lowercase();
    if lower.ends_with("::class") {
        Some(&text[..text.len() - 7]) // len("::class") == 7
    } else {
        None
    }
}

/// Extract a class name (possibly namespaced) scanning backwards from
/// the end of `text`.
fn extract_class_name_backwards(text: &str) -> Option<String> {
    let trimmed = text.trim_end();
    let bytes = trimmed.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let mut end = bytes.len();
    while end > 0
        && (bytes[end - 1].is_ascii_alphanumeric()
            || bytes[end - 1] == b'_'
            || bytes[end - 1] == b'\\')
    {
        end -= 1;
    }

    let name = &trimmed[end..];
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Extract a variable name scanning backwards from the end of `text`.
fn extract_variable_backwards(text: &str) -> Option<String> {
    let trimmed = text.trim_end();
    let bytes = trimmed.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let mut end = bytes.len();
    // Collect identifier chars.
    while end > 0 && (bytes[end - 1].is_ascii_alphanumeric() || bytes[end - 1] == b'_') {
        end -= 1;
    }
    // Must have `$` prefix.
    if end == 0 || bytes[end - 1] != b'$' {
        return None;
    }
    end -= 1; // include $

    let var = &trimmed[end..];
    if var.len() < 2 {
        return None;
    }
    Some(var.to_string())
}

// ─── Completion ─────────────────────────────────────────────────────────────

impl Backend {
    /// Try array callable method completion.
    ///
    /// When the cursor is inside the method-name string of
    /// `[Class::class, '|']` or `[$obj, '|']`, resolve the class and
    /// offer its methods as completion items.
    ///
    /// Reuses the standard member-completion builder so that items have
    /// the same rich formatting (label details, return type, deprecation
    /// tags, `data` for lazy documentation resolve) as regular `->` / `::`
    /// member completions.  The only post-processing is stripping snippet
    /// parentheses from `insert_text` since we are inserting into a string
    /// literal, not code.
    pub(crate) fn try_array_callable_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let ac_ctx = detect_array_callable_context(content, position)?;

        let class_loader = self.class_loader(ctx);

        // Resolve the subject to a ClassInfo.
        let class_info = if ac_ctx.is_static {
            // `Class::class` — resolve the class name to FQN and load.
            let fqn = resolve_class_name_to_fqn(self, &ac_ctx.subject, ctx)?;
            class_loader(&fqn)?
        } else {
            // Variable — resolve its type.
            let cursor_offset = position_to_offset(content, position);
            let current_class = crate::util::find_class_at_offset(&ctx.classes, cursor_offset);

            if ac_ctx.subject == "$this" {
                // `$this` — use the current class.
                current_class.map(|cc| Arc::new(cc.clone()))?
            } else {
                // Other variable — try variable resolution.
                let default_class = ClassInfo::default();
                let current = current_class.unwrap_or(&default_class);
                let results = crate::completion::variable::resolution::resolve_variable_types(
                    &ac_ctx.subject,
                    current,
                    &ctx.classes,
                    content,
                    cursor_offset,
                    &class_loader,
                    crate::completion::resolver::Loaders::default(),
                );
                let mut resolved_class = None;
                for rt in &results {
                    if let Some(base) = rt.type_string.base_name()
                        && let Some(cls) = class_loader(base)
                    {
                        resolved_class = Some(cls);
                        break;
                    }
                }
                resolved_class?
            }
        };

        // Build completion items using the standard builder, which gives
        // us full label details, return types, deprecation tags, and
        // `data` for lazy documentation resolve — identical to regular
        // member completion.
        //
        // `AccessKind::Arrow` is the right access kind for array
        // callables: PHP invokes them on an instance regardless of the
        // subject syntax. Once instance-access completion surfaces static
        // methods too, they will flow through here automatically. Until
        // then this offers the instance methods, which covers the common
        // case (controller actions, `$this`/`$obj` handlers).
        let candidates = vec![class_info];
        let cursor_offset = position_to_offset(content, position);
        let current_class = crate::util::find_class_at_offset(&ctx.classes, cursor_offset);
        let mut items = super::builder::build_union_completion_items(
            &candidates,
            AccessKind::Arrow,
            current_class,
            &class_loader,
            &self.resolved_class_cache,
            uri,
        );

        // Post-process: we are inserting into a string literal, so
        // replace snippet insert text (e.g. `sort(${1:\$request})$0`)
        // with the plain method name, and clear snippet-related fields.
        for item in &mut items {
            if item.kind == Some(CompletionItemKind::METHOD) {
                if let Some(ref filter) = item.filter_text {
                    item.insert_text = Some(filter.clone());
                }
                item.insert_text_format = None;
            }
        }

        // Filter out non-method items (properties, constants) since
        // array callables only reference methods.
        items.retain(|item| item.kind == Some(CompletionItemKind::METHOD));

        // Filter by the prefix already typed.
        if !ac_ctx.prefix.is_empty() {
            let prefix_lower = ac_ctx.prefix.to_lowercase();
            items.retain(|item| {
                item.filter_text
                    .as_ref()
                    .or(Some(&item.label))
                    .is_some_and(|t| t.to_lowercase().starts_with(&prefix_lower))
            });
        }

        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        }
    }
}

/// Resolve a short/relative class name to FQN using use statements
/// and the namespace from the file context.
pub(super) fn resolve_class_name_to_fqn(
    backend: &Backend,
    name: &str,
    ctx: &FileContext,
) -> Option<String> {
    let clean = name.trim_start_matches('\\');
    // Check use map.
    if let Some(fqn) = ctx.use_map.get(clean) {
        return Some(fqn.clone());
    }
    // If it looks like a FQN already.
    if clean.contains('\\') {
        return Some(clean.to_string());
    }
    // Try prepending the file namespace.
    if let Some(ref ns) = ctx.namespace {
        let fqn = format!("{}\\{}", ns, clean);
        if backend.find_or_load_class(&fqn).is_some() {
            return Some(fqn);
        }
    }
    // Try bare name.
    if backend.find_or_load_class(clean).is_some() {
        return Some(clean.to_string());
    }
    None
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Position;

    #[test]
    fn detects_class_const_callable() {
        let content = "<?php\n[SortableController::class, 'sort'];\n";
        // Cursor inside 'sort' — after the 's'.
        let line = 1;
        let col = content.lines().nth(1).unwrap().find("sort']").unwrap() as u32 + 1;
        let ctx = detect_array_callable_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect array callable context");
        assert_eq!(ctx.subject, "SortableController");
        assert!(ctx.is_static);
        assert_eq!(ctx.prefix, "s");
    }

    #[test]
    fn detects_variable_callable() {
        let content = "<?php\n[$this, 'handle'];\n";
        let line = 1;
        let col = content.lines().nth(1).unwrap().find("handle']").unwrap() as u32;
        let ctx = detect_array_callable_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect array callable context");
        assert_eq!(ctx.subject, "$this");
        assert!(!ctx.is_static);
        assert_eq!(ctx.prefix, "");
    }

    #[test]
    fn detects_empty_string() {
        let content = "<?php\n[Foo::class, ''];\n";
        let line = 1;
        let col = content.lines().nth(1).unwrap().find("']").unwrap() as u32;
        let ctx = detect_array_callable_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect array callable context");
        assert_eq!(ctx.subject, "Foo");
        assert!(ctx.is_static);
        assert_eq!(ctx.prefix, "");
    }

    #[test]
    fn rejects_plain_string_array() {
        let content = "<?php\n['foo', 'bar'];\n";
        let line = 1;
        let col = content.lines().nth(1).unwrap().find("bar']").unwrap() as u32;
        let ctx = detect_array_callable_context(content, Position::new(line, col));
        assert!(ctx.is_none(), "plain string array should not match");
    }

    #[test]
    fn detects_namespaced_class() {
        let content = "<?php\n[App\\Http\\Controllers\\IndexController::class, 'index'];\n";
        let line = 1;
        let col = content.lines().nth(1).unwrap().find("index']").unwrap() as u32;
        let ctx = detect_array_callable_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect namespaced class");
        assert_eq!(ctx.subject, "App\\Http\\Controllers\\IndexController");
        assert!(ctx.is_static);
        assert_eq!(ctx.prefix, "");
    }

    #[test]
    fn detects_with_whitespace() {
        let content = "<?php\n[  Foo::class ,  'bar'  ];\n";
        let line = 1;
        let col = content.lines().nth(1).unwrap().find("bar'").unwrap() as u32;
        let ctx = detect_array_callable_context(content, Position::new(line, col));
        let ctx = ctx.expect("should handle whitespace");
        assert_eq!(ctx.subject, "Foo");
        assert!(ctx.is_static);
        assert_eq!(ctx.prefix, "");
    }

    #[test]
    fn rejects_non_identifier_content() {
        let content = "<?php\n[$this, 'not a method'];\n";
        let line = 1;
        // Place cursor after the space so the prefix includes it: "not "
        let col = content.lines().nth(1).unwrap().find("not a").unwrap() as u32 + 4;
        let ctx = detect_array_callable_context(content, Position::new(line, col));
        assert!(ctx.is_none(), "non-identifier string should not match");
    }

    #[test]
    fn detects_unclosed_string() {
        // User is still typing — no closing quote yet.
        let content = "<?php\n[Foo::class, 'ge";
        let line = 1;
        let col = content.lines().nth(1).unwrap().len() as u32;
        let ctx = detect_array_callable_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect unclosed string");
        assert_eq!(ctx.subject, "Foo");
        assert!(ctx.is_static);
        assert_eq!(ctx.prefix, "ge");
    }
}
