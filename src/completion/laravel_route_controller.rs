//! Laravel route controller method completion.
//!
//! Detects when the cursor is inside the action string argument of a route
//! definition call within a `->controller(X::class)->group(fn(){…})` context
//! and offers controller method name completions.
//!
//! For example:
//!
//! ```php
//! Route::controller(WorkItemController::class)->group(function () {
//!     Route::patch('cancel', '|');  // <-- offers WorkItemController methods
//! });
//! ```
//!
//! This is the completion-time counterpart of the extraction-time
//! `MemberAccess` span emission in `symbol_map/extraction.rs`
//! (`try_emit_laravel_route_controller_spans`).  The extraction layer
//! gives go-to-definition, references, rename, hover, and diagnostics
//! for free; this module adds autocompletion.

use std::ops::ControlFlow;

use tower_lsp::lsp_types::*;

use mago_syntax::ast::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::types::{AccessKind, FileContext};
use crate::util::position_to_offset;

// ─── Context ────────────────────────────────────────────────────────────────

/// Context extracted when the cursor is inside the action string of a
/// route definition call within a controller group.
#[derive(Debug)]
pub(crate) struct LaravelRouteControllerContext {
    /// The controller class name as written in source
    /// (e.g. `"WorkItemResourceController"`).
    pub controller_class: String,
    /// The partial method name the user has typed so far.
    pub prefix: String,
}

// ─── Route HTTP methods ─────────────────────────────────────────────────────

const ROUTE_HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "options", "any"];

// ─── Detection (text scanning + AST) ────────────────────────────────────────

/// Detect whether the cursor is inside the action string of a route call
/// within a `->controller(X::class)->group(fn(){…})` context.
///
/// Uses fast backward text scanning to confirm the cursor is in the 2nd
/// argument of a `Route::{http_method}(…)` call, then parses the file to
/// find the enclosing `->controller()` in the group chain.
pub(crate) fn detect_laravel_route_controller_context(
    content: &str,
    position: Position,
) -> Option<LaravelRouteControllerContext> {
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
        if ch == b'\n' {
            return None;
        }
    }
    let quote_pos = quote_pos?;
    let prefix = content[quote_pos + 1..cursor_offset].to_string();

    // Validate prefix is identifier-like (or empty).
    if !prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }

    // ── Step 2: Before the quote, expect comma (2nd argument) ───────
    let before_quote = content[..quote_pos].trim_end();
    if !before_quote.ends_with(',') {
        return None;
    }
    let before_comma = before_quote[..before_quote.len() - 1].trim_end();

    // Before the comma should be a string (the route URI — 1st arg).
    if !(before_comma.ends_with('\'') || before_comma.ends_with('"')) {
        return None;
    }

    // Walk back past the first string to find Route::{method}(
    let closing_quote_byte = before_comma.as_bytes()[before_comma.len() - 1];
    let inner = &before_comma[..before_comma.len() - 1];
    let inner_bytes = inner.as_bytes();
    let mut open_pos = None;
    let mut k = inner.len();
    while k > 0 {
        k -= 1;
        if inner_bytes[k] == closing_quote_byte {
            let mut bs = 0;
            let mut j = k;
            while j > 0 && inner_bytes[j - 1] == b'\\' {
                bs += 1;
                j -= 1;
            }
            if bs % 2 == 0 {
                open_pos = Some(k);
                break;
            }
        }
    }
    let open_pos = open_pos?;
    let before_first_string = inner[..open_pos].trim_end();

    // Expect `(`
    if !before_first_string.ends_with('(') {
        return None;
    }
    let before_paren = before_first_string[..before_first_string.len() - 1].trim_end();

    // ── Step 3: Extract Route::{method} ─────────────────────────────
    let bp_bytes = before_paren.as_bytes();
    let method_end = bp_bytes.len();
    let mut method_start = method_end;
    while method_start > 0
        && (bp_bytes[method_start - 1].is_ascii_alphanumeric()
            || bp_bytes[method_start - 1] == b'_')
    {
        method_start -= 1;
    }
    let method_name = &before_paren[method_start..method_end];
    if !ROUTE_HTTP_METHODS
        .iter()
        .any(|m| m.eq_ignore_ascii_case(method_name))
    {
        return None;
    }

    // Before method name, expect `::`
    let before_method = before_paren[..method_start].trim_end();
    if !before_method.ends_with("::") {
        return None;
    }
    let before_colons = before_method[..before_method.len() - 2].trim_end();

    // Extract class name — must end with "Route".
    let bc_bytes = before_colons.as_bytes();
    let class_end = bc_bytes.len();
    let mut class_start = class_end;
    while class_start > 0
        && (bc_bytes[class_start - 1].is_ascii_alphanumeric()
            || bc_bytes[class_start - 1] == b'_'
            || bc_bytes[class_start - 1] == b'\\')
    {
        class_start -= 1;
    }
    let class_name = &before_colons[class_start..class_end];
    let short_name = class_name.rsplit('\\').next().unwrap_or(class_name);
    if !short_name.eq_ignore_ascii_case("Route") {
        return None;
    }

    // ── Step 4: Find enclosing controller via AST ───────────────────
    let controller = find_enclosing_controller_class(content, cursor_offset as u32)?;

    Some(LaravelRouteControllerContext {
        controller_class: controller,
        prefix,
    })
}

// ─── AST-based controller resolution ────────────────────────────────────────

/// Parse the file and walk the AST to find the `->controller(X::class)` that
/// encloses the given cursor offset.
///
/// When multiple nested controller groups contain the cursor, the tightest
/// (innermost) match wins.
fn find_enclosing_controller_class(content: &str, cursor_offset: u32) -> Option<String> {
    let mut best: Option<(String, u32)> = None; // (controller, range_size)

    crate::virtual_members::laravel::walk_all_php_expressions(content, &mut |expr| {
        if let Expression::Call(Call::Method(mc)) = expr
            && let ClassLikeMemberSelector::Identifier(ident) = &mc.method
            && ident.value.eq_ignore_ascii_case(b"group")
            && let Some(controller) = find_chain_controller(mc.object)
        {
            for arg in mc.argument_list.arguments.iter() {
                if let Some((start, end)) = closure_body_range(arg.value())
                    && cursor_offset >= start
                    && cursor_offset <= end
                {
                    let range_size = end - start;
                    if best.as_ref().is_none_or(|(_, sz)| range_size < *sz) {
                        best = Some((controller.clone(), range_size));
                    }
                }
            }
        }
        ControlFlow::Continue(())
    });

    best.map(|(name, _)| name)
}

/// Walk a method call chain looking for `->controller(X::class)`.
fn find_chain_controller(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Call(Call::Method(mc)) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &mc.method
                && ident.value.eq_ignore_ascii_case(b"controller")
            {
                return extract_class_arg(&mc.argument_list);
            }
            find_chain_controller(mc.object)
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &sc.method
                && ident.value.eq_ignore_ascii_case(b"controller")
            {
                return extract_class_arg(&sc.argument_list);
            }
            None
        }
        _ => None,
    }
}

/// Extract the class name from the first `X::class` argument.
fn extract_class_arg(args: &ArgumentList<'_>) -> Option<String> {
    let first_arg = args.arguments.iter().next()?;
    if let Expression::Access(Access::ClassConstant(cca)) = first_arg.value() {
        let is_class = matches!(
            &cca.constant,
            ClassLikeConstantSelector::Identifier(ident)
                if bytes_to_str(ident.value).eq_ignore_ascii_case("class")
        );
        if is_class && let Expression::Identifier(id) = cca.class {
            let name = bytes_to_str(id.value()).to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Return the byte range of a closure body (between braces).
fn closure_body_range(expr: &Expression<'_>) -> Option<(u32, u32)> {
    if let Expression::Closure(c) = expr {
        Some((
            c.body.left_brace.start.offset,
            c.body.right_brace.end.offset,
        ))
    } else {
        None
    }
}

// ─── Completion ─────────────────────────────────────────────────────────────

impl Backend {
    /// Try Laravel route controller method completion.
    ///
    /// When the cursor is inside the action string of
    /// `Route::patch('path', '|')` within a controller group, resolve
    /// the controller class and offer its methods as completion items.
    pub(crate) fn try_laravel_route_controller_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let rc_ctx = detect_laravel_route_controller_context(content, position)?;

        let class_loader = self.class_loader(ctx);

        // Resolve the controller class name to FQN and load.
        let fqn =
            super::array_callable::resolve_class_name_to_fqn(self, &rc_ctx.controller_class, ctx)?;
        let class_info = class_loader(&fqn)?;

        // Build completion items using the standard builder.
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
        // replace snippet insert text with the plain method name.
        for item in &mut items {
            if item.kind == Some(CompletionItemKind::METHOD) {
                if let Some(ref filter) = item.filter_text {
                    item.insert_text = Some(filter.clone());
                }
                item.insert_text_format = None;
                item.commit_characters = None;
            }
        }

        // Only methods (no properties/constants).
        items.retain(|item| item.kind == Some(CompletionItemKind::METHOD));

        // Filter by prefix.
        if !rc_ctx.prefix.is_empty() {
            let prefix_lower = rc_ctx.prefix.to_lowercase();
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

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Position;

    #[test]
    fn detects_route_patch_second_arg() {
        let content = "<?php\nRoute::patch('cancel', 'cancel');\n";
        // Cursor inside 'cancel' (the 2nd arg) — but no controller group,
        // so detection should return None for the full context.
        let line = 1;
        let col = content.lines().nth(1).unwrap().rfind("cancel')").unwrap() as u32;
        let ctx = detect_laravel_route_controller_context(content, Position::new(line, col));
        assert!(
            ctx.is_none(),
            "No controller group — should not detect context"
        );
    }

    #[test]
    fn detects_controller_group_context() {
        let content = concat!(
            "<?php\n",
            "Route::controller(FooController::class)->group(function () {\n",
            "    Route::patch('cancel', 'can');\n",
            "});\n",
        );
        // Cursor inside 'can' on line 2.
        let line = 2;
        let line_text = content.lines().nth(2).unwrap();
        let col = line_text.find("can')").unwrap() as u32 + 3; // after "can"
        let ctx = detect_laravel_route_controller_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect controller group context");
        assert_eq!(ctx.controller_class, "FooController");
        assert_eq!(ctx.prefix, "can");
    }

    #[test]
    fn detects_empty_prefix() {
        let content = concat!(
            "<?php\n",
            "Route::controller(FooController::class)->group(function () {\n",
            "    Route::get('/', '');\n",
            "});\n",
        );
        let line = 2;
        let line_text = content.lines().nth(2).unwrap();
        let col = line_text.find("'');").unwrap() as u32 + 1; // inside empty ''
        let ctx = detect_laravel_route_controller_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect empty prefix");
        assert_eq!(ctx.controller_class, "FooController");
        assert_eq!(ctx.prefix, "");
    }

    #[test]
    fn nested_controller_shadows_outer() {
        let content = concat!(
            "<?php\n",
            "Route::controller(OuterController::class)->group(function () {\n",
            "    Route::controller(InnerController::class)->group(function () {\n",
            "        Route::get('/', 'idx');\n",
            "    });\n",
            "});\n",
        );
        let line = 3;
        let line_text = content.lines().nth(3).unwrap();
        let col = line_text.find("idx").unwrap() as u32 + 2;
        let ctx = detect_laravel_route_controller_context(content, Position::new(line, col));
        let ctx = ctx.expect("should detect inner controller");
        assert_eq!(ctx.controller_class, "InnerController");
    }

    #[test]
    fn rejects_first_arg() {
        let content = concat!(
            "<?php\n",
            "Route::controller(FooController::class)->group(function () {\n",
            "    Route::get('path', 'method');\n",
            "});\n",
        );
        // Cursor inside 'path' (the 1st arg) — should not match.
        let line = 2;
        let line_text = content.lines().nth(2).unwrap();
        let col = line_text.find("path").unwrap() as u32 + 2;
        let ctx = detect_laravel_route_controller_context(content, Position::new(line, col));
        assert!(ctx.is_none(), "First arg should not match");
    }
}
