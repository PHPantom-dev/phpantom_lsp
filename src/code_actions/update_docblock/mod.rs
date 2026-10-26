//! Update Docblock to Match Signature code action.
//!
//! When a function or method signature changes (parameters added, removed,
//! reordered, or type hints updated), the docblock often falls out of sync.
//! This code action patches the `@param` and `@return` tags to match the
//! current signature while preserving descriptions and other tags.
//!
//! **Trigger:** Cursor is inside the docblock of a function/method whose
//! `@param` tags don't match the signature's parameters (by name, count,
//! or order), whose `@return` tag contradicts the return type hint, or
//! whose `@return` tag can be enriched with body-based type inference.
//!
//! **Code action kind:** `quickfix`.
//!
//! The action runs in three steps: [`find`] the function whose docblock
//! holds the cursor, decide what [`changes`], and [`rewrite`] the
//! docblock.

mod changes;
mod find;
mod rewrite;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::php_type::PhpType;
use crate::text_position::byte_range_to_lsp_range;

use changes::{analyse_body, check_needs_update};
use find::find_function_with_docblock_from_context;
use rewrite::build_updated_docblock;

use super::cursor_context::find_cursor_context;

// ── Data types ──────────────────────────────────────────────────────────────

/// A parameter extracted from the function/method signature.
#[derive(Debug, Clone)]
struct SigParam {
    /// Parameter name including `$` prefix.
    name: String,
    /// Native type hint as a structured type, if present.
    type_hint: Option<PhpType>,
    /// Whether the parameter is variadic (`...$args`).
    is_variadic: bool,
}

/// A `@param` tag parsed from an existing docblock.
#[derive(Debug, Clone)]
struct DocParam {
    /// The original type string from the tag, preserved for docblock output.
    type_str_raw: String,
    /// The parsed type, constructed once from `type_str_raw`.
    type_parsed: PhpType,
    /// Parameter name including `$` prefix (and optional `...` prefix for variadic).
    name: String,
    /// Description text after the `$name`.
    description: String,
}

/// A `@return` tag parsed from an existing docblock.
#[derive(Debug, Clone)]
struct DocReturn {
    /// The parsed type, constructed once from the raw tag string.
    type_parsed: PhpType,
    /// Description text after the type.
    description: String,
}

/// Information about the function/method under the cursor, including its
/// docblock position and parsed tags.
struct FunctionWithDocblock {
    /// Byte range of the docblock comment (from `/**` to `*/` inclusive).
    docblock_start: usize,
    docblock_end: usize,
    /// The raw docblock text.
    docblock_text: String,
    /// Parameters from the signature.
    sig_params: Vec<SigParam>,
    /// Return type from the signature (if any).
    sig_return: Option<PhpType>,
    /// `@param` tags from the docblock.
    doc_params: Vec<DocParam>,
    /// `@return` tag from the docblock (if any).
    doc_return: Option<DocReturn>,
    /// `@throws` exception type names from the docblock.
    doc_throws: Vec<String>,
    /// Indentation of the docblock lines (whitespace before ` * `).
    indent: String,
    /// LSP position of the docblock start (for throws analysis).
    docblock_position: Position,
    /// Byte offset of the start of the declaration the docblock documents.
    decl_offset: u32,
}

impl DocParam {
    /// The parameter name without the variadic `...` prefix.
    fn bare_name(&self) -> &str {
        self.name.strip_prefix("...").unwrap_or(&self.name)
    }
}

impl FunctionWithDocblock {
    /// The `@param` tag documenting the signature parameter `name`.
    fn doc_param(&self, name: &str) -> Option<&DocParam> {
        self.doc_params.iter().find(|dp| dp.bare_name() == name)
    }
}

/// Body-derived facts that both the staleness check and the rewrite read.
/// Each costs a parse of the file, so they are computed once per request.
struct BodyFacts {
    /// Body-inferred `@return` type that should replace the current tag.
    enriched_return: Option<PhpType>,
    /// Uncaught exception FQNs that have no `@throws` tag yet.
    missing_throws: Vec<String>,
}

impl Backend {
    /// Collect "Update docblock" code actions for the function/method
    /// under the cursor.
    pub(crate) fn collect_update_docblock_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionResponse>,
    ) {
        let doc_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let cursor_offset = crate::text_position::position_to_offset(content, params.range.start);

        // Resolve the function/method (and its docblock) under the cursor.
        // The returned info is owned, so the borrowed AST does not escape.
        let info =
            crate::parser::with_parsed_program(content, "update_docblock", |program, content| {
                let ctx = find_cursor_context(&program.statements, cursor_offset);
                let trivia = program.trivia.as_slice();
                find_function_with_docblock_from_context(
                    &ctx,
                    &program.statements,
                    trivia,
                    content,
                    cursor_offset,
                )
            });
        let info = match info {
            Some(info) => info,
            None => return,
        };

        let ctx = self.file_context_at(uri, cursor_offset);
        let class_loader = self.class_loader(&ctx);
        let resolved_class_loader = self.resolved_class_loader(&ctx);
        let function_loader = self.function_loader(&ctx);

        let facts = analyse_body(
            &info,
            content,
            &ctx.classes,
            &class_loader,
            &resolved_class_loader,
            Some(self),
            Some(&function_loader),
            &ctx.use_map,
            &ctx.namespace,
        );

        if !check_needs_update(&info, &facts, &class_loader, &ctx.use_map, &ctx.namespace) {
            return;
        }

        let new_docblock = build_updated_docblock(
            &info,
            &facts,
            &class_loader,
            &resolved_class_loader,
            &ctx.use_map,
            &ctx.namespace,
        );
        if new_docblock == info.docblock_text {
            return;
        }

        out.push(CodeActionResponse::CodeAction(CodeAction {
            title: "Update docblock to match signature".to_string(),
            kind: Some(CodeActionKind::QuickFix),
            diagnostics: None,
            edit: Some(crate::code_actions::single_edit(
                doc_uri,
                byte_range_to_lsp_range(content, info.docblock_start, info.docblock_end),
                new_docblock,
            )),
            command: None,
            is_preferred: Some(true),
            disabled: None,
            data: None,
            tags: None,
        }));
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
