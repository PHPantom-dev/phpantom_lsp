//! PHPDoc block generation on `/**`.
//!
//! Two entry points:
//!
//! 1. **Completion** (`try_generate_docblock`) — fires when `/` is a
//!    trigger character and the cursor is right after `/**`.  Returns a
//!    snippet-format `CompletionItem` with tab stops.  Works in editors
//!    that do *not* auto-close `/**`.
//!
//! 2. **On-type formatting** (`try_generate_docblock_on_enter`) — fires
//!    on Enter (`\n`) via `textDocument/onTypeFormatting`.  Detects a
//!    freshly auto-generated empty `/** … */` block (the kind VS Code
//!    and Zed produce when you type `/**`), replaces it with a filled
//!    docblock, and positions the cursor on the summary line.  Works in
//!    editors that *do* auto-close `/**`.
//!
//! Both paths share the same declaration analysis (`parse_decl`) and
//! snippet/text building helpers (`build`) defined in the sibling
//! modules below. `trigger` detects the `/**` trigger position and the
//! declaration text that follows it.
//!
//! **Design choices:**
//!
//! - A docblock is always generated (at minimum a summary skeleton).
//! - `@param` / `@return` tags are only emitted when the native type
//!   hint cannot fully express the type: missing type, bare `array`,
//!   `Closure` / `callable`, union containing any of those, or a
//!   class that has `@template` parameters.
//! - `Closure` and `callable` get a callable-signature placeholder
//!   wrapped in parentheses: `(Closure(): mixed)`, `(callable(): mixed)`.
//! - Union types containing `array`, `Closure`, or `callable` echo
//!   the raw type string so the user can refine the relevant part.
//! - `@throws` tags are always added for uncaught exception types.
//! - No special treatment for overrides — the same rules apply.
//! - Class-like declarations get `@extends` / `@implements` tags when
//!   the parent or interface has `@template` parameters.
//! - Properties and constants always get `@var Type`.
//! - Tags are ordered `@param`, `@throws`, `@return` with a blank
//!   `*` separator line between different groups (not within a group,
//!   and not before the first group).  No summary line is emitted
//!   when tags are present.
//! - When there are no tags, a summary-only skeleton is generated.
//! - Parameter names within the `@param` block are space-aligned.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use super::context::DocblockContext;
use crate::completion::source::throws_analysis::{self, ThrowsContext};
use crate::completion::use_edit::{analyze_use_block, build_use_edit};
use crate::types::{ClassInfo, FunctionLoader};

mod build;
mod parse_decl;
mod trigger;

pub(crate) use build::{
    enrichment_plain, enrichment_plain_typed, enrichment_snippet, infer_inline_variable_type,
};

/// Detect whether the cursor is immediately after a `/**` trigger and,
/// if so, generate a full docblock completion item.
///
/// Returns `None` when the cursor is not at a `/**` trigger position or
/// when the declaration below cannot be identified.
pub fn try_generate_docblock(
    content: &str,
    position: Position,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
) -> Option<CompletionResponse> {
    let (trigger_range, indent) = trigger::detect_docblock_trigger(content, position)?;

    // Find the declaration below and classify it.
    let remaining = trigger::get_text_after_trigger(content, position);
    let context = parse_decl::classify_declaration(&remaining);

    // Inside a function body (Inline / Unknown) we don't generate a
    // full docblock — the `@` tag completion is more appropriate there
    // because the user might want @var, @throws, @todo, etc.
    if matches!(context, DocblockContext::Inline | DocblockContext::Unknown) {
        return None;
    }

    let mut sym = parse_decl::parse_declaration_info(&remaining);

    // For untyped properties, try to fill in the type from the parsed
    // class data (e.g. constructor-inferred `$this->prop = new Foo()`).
    if matches!(context, DocblockContext::Property) && sym.type_hint.is_none() {
        parse_decl::enrich_property_type_from_class(&mut sym, content, position, local_classes);
    }

    let snippet = build::build_docblock_snippet(
        &context,
        &sym,
        &indent,
        content,
        position,
        use_map,
        file_namespace,
        local_classes,
        class_loader,
        function_loader,
    );

    if snippet.is_empty() {
        return None;
    }

    // Collect additional text edits (e.g. use imports for @throws).
    let additional_edits = build_throws_import_edits(
        content,
        position,
        use_map,
        file_namespace,
        &context,
        class_loader,
        function_loader,
    );

    let item = CompletionItem {
        label: "/** PHPDoc Block */".to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some("Generate PHPDoc block".to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
            range: trigger_range,
            new_text: snippet,
        })),
        filter_text: Some("/**".to_string()),
        sort_text: Some("0".to_string()),
        additional_text_edits: if additional_edits.is_empty() {
            None
        } else {
            Some(additional_edits)
        },
        // Pre-select so the user can just press Enter.
        preselect: Some(true),
        ..CompletionItem::default()
    };

    Some(CompletionResponse::Array(vec![item]))
}

/// Handle `textDocument/onTypeFormatting` after Enter inside a freshly
/// auto-generated `/** */` or `/**\n * \n */` block.
///
/// Most editors (VS Code, Zed, Neovim with auto-pairs) expand `/**`
/// into a closed block before the LSP sees anything.  The user then
/// presses Enter, and `onTypeFormatting` fires with `ch = "\n"`.
///
/// This function detects that pattern, finds the declaration below the
/// docblock, and returns `TextEdit`s that replace the empty block with
/// a filled one.  Returns `None` when the cursor is not inside a fresh
/// empty docblock.
pub fn try_generate_docblock_on_enter(
    content: &str,
    position: Position,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
) -> Option<Vec<TextEdit>> {
    // Detect the empty docblock range and indentation.
    let (block_range, _block_indent, after_block) =
        trigger::detect_empty_docblock(content, position)?;

    // Use the declaration's indentation rather than the `/**` line's.
    // Some editors (e.g. Zed) place the auto-closed `/** */` at the
    // wrong indent level inside constructor parameter lists.  The
    // declaration line is always at the correct level.
    let indent = trigger::declaration_indent(&after_block);

    // Classify and parse the declaration after the block.
    let context = parse_decl::classify_declaration(&after_block);

    // Inside a function body (Inline / Unknown) we don't generate a
    // full docblock — the `@` tag completion is more appropriate there.
    if matches!(context, DocblockContext::Inline | DocblockContext::Unknown) {
        return None;
    }

    let mut sym = parse_decl::parse_declaration_info(&after_block);

    // For untyped properties, try to fill in the type from the parsed
    // class data (e.g. constructor-inferred `$this->prop = new Foo()`).
    if matches!(context, DocblockContext::Property) && sym.type_hint.is_none() {
        parse_decl::enrich_property_type_from_class(&mut sym, content, position, local_classes);
    }

    // Build the docblock as plain text (no snippet tab stops).
    let plain = build::build_docblock_plain(
        &context,
        &sym,
        &indent,
        content,
        position,
        use_map,
        file_namespace,
        local_classes,
        class_loader,
        function_loader,
    );

    if plain.is_empty() {
        return None;
    }

    let mut edits = vec![TextEdit {
        range: block_range,
        new_text: plain,
    }];

    // Auto-import edits for @throws.
    edits.extend(build_throws_import_edits(
        content,
        position,
        use_map,
        file_namespace,
        &context,
        class_loader,
        function_loader,
    ));

    Some(edits)
}

// ─── Import Edits ───────────────────────────────────────────────────────────

/// Build additional text edits for auto-importing exception types
/// referenced in `@throws` tags.
fn build_throws_import_edits(
    content: &str,
    position: Position,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    context: &DocblockContext,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
) -> Vec<TextEdit> {
    if !matches!(context, DocblockContext::FunctionOrMethod) {
        return Vec::new();
    }

    let throws_ctx = ThrowsContext {
        class_loader,
        function_loader,
        use_map,
        file_namespace,
    };
    let uncaught = throws_analysis::find_uncaught_throw_types_with_context(
        content,
        position,
        Some(&throws_ctx),
    );
    if uncaught.is_empty() {
        return Vec::new();
    }

    let use_block = analyze_use_block(content);
    let mut edits = Vec::new();

    for exc in &uncaught {
        // Exception types are already resolved to FQNs by
        // `find_uncaught_throw_types_with_context` — do not re-resolve.
        let fqn = exc.to_string();
        if !throws_analysis::has_use_import(content, &fqn)
            && let Some(edit) = build_use_edit(&fqn, &use_block, file_namespace)
        {
            edits.extend(edit);
        }
    }

    edits
}
