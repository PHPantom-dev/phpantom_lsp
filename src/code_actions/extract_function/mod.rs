//! **Extract Function / Method** code action (`refactor.extract`).
//!
//! When the user selects one or more complete statements inside a
//! function or method body, this action extracts them into a new
//! function (or method, if `$this`/`self::`/`static::` is used).
//!
//! The implementation uses the `ScopeCollector` infrastructure to
//! classify variables as parameters, return values, or locals relative
//! to the selected range.  Type annotations are inferred via the hover
//! variable-type resolution pipeline.

// Re-exported (`pub(crate) use`) so the section submodules below can pull the
// whole shared import surface in with a single `use super::*;`.
pub(crate) use mago_span::HasSpan;
pub(crate) use mago_syntax::cst::*;
pub(crate) use std::collections::HashMap;
pub(crate) use std::sync::Arc;
pub(crate) use tower_lsp::lsp_types::*;

pub(crate) use crate::Backend;
pub(crate) use crate::atom::bytes_to_str;
pub(crate) use crate::class_lookup::find_class_at_offset;
pub(crate) use crate::code_actions::cursor_context::{
    CursorContext, MemberContext, find_cursor_context,
};
pub(crate) use crate::code_actions::naming::capitalise;
pub(crate) use crate::code_actions::{CodeActionData, make_code_action_data};
pub(crate) use crate::completion::phpdoc::generation::enrichment_plain;
pub(crate) use crate::completion::resolver::Loaders;
pub(crate) use crate::php_type::PhpType;
pub(crate) use crate::scope_collector::ScopeMap;
use crate::text_position::{offset_to_position, position_to_byte_offset};
pub(crate) use crate::types::ClassInfo;

mod codegen;
mod context;
mod naming;
mod returns;
mod scope;

pub(crate) use codegen::*;
pub(crate) use context::*;
pub(crate) use naming::*;
pub(crate) use returns::*;
pub(crate) use scope::*;

// ─── Main code action collector ─────────────────────────────────────────────

impl Backend {
    /// Collect "Extract Function" / "Extract Method" code actions.
    ///
    /// This action is offered when the user has a non-empty selection
    /// that covers one or more complete statements inside a function or
    /// method body.
    ///
    /// Phase 1 performs lightweight validation only.  The expensive
    /// work (scope classification, type resolution, PHPDoc generation,
    /// edit building) is deferred to [`resolve_extract_function`]
    /// (Phase 2).
    pub(crate) fn collect_extract_function_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        // Only activate when the selection is non-empty.
        if params.range.start == params.range.end {
            return;
        }

        let start_offset = position_to_byte_offset(content, params.range.start);
        let end_offset = position_to_byte_offset(content, params.range.end);

        // Trim the selection to exclude leading/trailing whitespace.
        let (start, end) = match trim_selection(content, start_offset, end_offset) {
            Some(range) => range,
            None => return,
        };

        // Validate that the selection covers complete statements.
        if !selection_covers_complete_statements(content, start, end) {
            return;
        }

        // ── Determine method vs function for the title ──────────────
        // We only need to know whether `$this`/`self::`/`static::` is
        // referenced to pick "Extract method" vs "Extract function".
        // A simple text scan is sufficient for the title — the full
        // scope analysis happens in Phase 2.
        let selected_text = &content[start..end];
        let looks_like_method = selected_text.contains("$this")
            || selected_text.contains("self::")
            || selected_text.contains("static::")
            || selected_text.contains("parent::");

        let title = if looks_like_method {
            "Extract method".to_string()
        } else {
            "Extract function".to_string()
        };

        // Phase 1: emit a lightweight code action with no edit.
        // The full workspace edit is computed lazily in
        // `resolve_extract_function` (Phase 2) when the user picks
        // this action.
        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::REFACTOR_EXTRACT),
            diagnostics: None,
            edit: None,
            command: None,
            is_preferred: Some(false),
            disabled: None,
            data: Some(make_code_action_data(
                "refactor.extractFunction",
                uri,
                &params.range,
                serde_json::json!({}),
            )),
        }));
    }

    /// Resolve types for a list of variable names at a given offset.
    ///
    /// Returns `(dollar_name, cleaned_type, raw_hint)` triples.
    /// `cleaned_type` has generics stripped for use in native PHP
    /// signatures.  `raw_hint` preserves the full resolved type
    /// (e.g. `Collection<User>`) for PHPDoc generation.
    fn resolve_param_types(
        &self,
        uri: &str,
        content: &str,
        offset: u32,
        var_names: &[String],
    ) -> Vec<(String, PhpType, PhpType)> {
        var_names
            .iter()
            .map(|name| {
                let dollar_name = if name.starts_with('$') {
                    name.clone()
                } else {
                    format!("${}", name)
                };
                let resolved_type = resolve_var_type(self, &dollar_name, content, offset, uri);
                let raw_type = resolved_type.clone().unwrap_or_else(PhpType::untyped);
                // Clean up the type for use in a signature — stays as PhpType.
                let cleaned = resolved_type
                    .as_ref()
                    .and_then(clean_type_for_signature_typed)
                    .unwrap_or_else(PhpType::untyped);
                (dollar_name, cleaned, raw_type)
            })
            .collect()
    }

    /// Resolve a deferred "Extract Function/Method" code action.
    ///
    /// This is **Phase 2** of the two-phase code-action model.  Phase 1
    /// (`collect_extract_function_actions`) already validated the
    /// selection and emitted a lightweight `CodeAction` with a title
    /// but no edit.  Here we re-run the full extraction logic from the
    /// selection range stored in `data` and produce the workspace edit.
    pub(crate) fn resolve_extract_function(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;
        let range = &data.range;

        // ── Re-validate the selection (content may have changed) ────
        let start_offset = position_to_byte_offset(content, range.start);
        let end_offset = position_to_byte_offset(content, range.end);

        let (start, end) = trim_selection(content, start_offset, end_offset)?;

        if !selection_covers_complete_statements(content, start, end) {
            return None;
        }

        // ── Scope map & classification ──────────────────────────────
        let scope_map = build_scope_map(content, start as u32);
        let classification = scope_map.classify_range(start as u32, end as u32);

        let return_value_count = classification.return_values.len();
        let return_strategy = analyse_returns(content, start, end, return_value_count);

        if return_strategy == ReturnStrategy::Unsafe {
            return None;
        }

        let uses_this = if scope_map.has_this_or_self {
            classification.uses_this
        } else {
            false
        };

        if scope_map.uses_reference_params() && !classification.reference_writes.is_empty() {
            return None;
        }

        if classification.return_values.len() > 4 {
            return None;
        }

        // ── Enclosing context ───────────────────────────────────────
        let enclosing = find_enclosing_context(content, start as u32, uses_this)?;

        // ── Naming ──────────────────────────────────────────────────
        let body_line_start_for_naming = find_line_start(content, start);
        let body_text_for_naming = &content[body_line_start_for_naming..end];
        let pre_trailing_return_type = if matches!(return_strategy, ReturnStrategy::TrailingReturn)
        {
            resolve_enclosing_return_type(content, start as u32)
        } else {
            PhpType::untyped()
        };
        let naming_ctx = NamingContext {
            enclosing_name: &enclosing.enclosing_name,
            return_strategy: &return_strategy,
            body_text: body_text_for_naming,
            return_var_names: &classification.return_values,
            trailing_return_type: &pre_trailing_return_type,
        };
        let fn_name = generate_function_name(content, &enclosing, &naming_ctx);

        // ── Type resolution ─────────────────────────────────────────
        let typed_params =
            self.resolve_param_types(uri, content, start as u32, &classification.parameters);
        let enclosing_param_order = resolve_enclosing_param_order(content, start as u32);
        let typed_params = sort_params_by_enclosing_order(typed_params, &enclosing_param_order);
        let typed_returns =
            self.resolve_param_types(uri, content, start as u32, &classification.return_values);

        // ── Indentation ─────────────────────────────────────────────
        let call_indent = indent_at(content, start);
        let (member_indent, body_indent) = match enclosing.target {
            ExtractionTarget::Method => {
                let member = detect_line_indent(content, enclosing.body_start);
                let unit = detect_indent_unit(content);
                let body = format!("{}{}", member, unit);
                (member, body)
            }
            ExtractionTarget::Function => {
                let member = String::new();
                let unit = detect_indent_unit(content);
                (member, unit.to_string())
            }
        };

        // ── Body text ───────────────────────────────────────────────
        let body_line_start = find_line_start(content, start);
        let body_text = content[body_line_start..end].to_string();

        // ── Return type resolution ──────────────────────────────────
        let trailing_return_type = if matches!(
            return_strategy,
            ReturnStrategy::TrailingReturn
                | ReturnStrategy::SentinelNull
                | ReturnStrategy::NullGuardWithValue(_)
        ) {
            resolve_enclosing_return_type(content, start as u32)
        } else {
            PhpType::untyped()
        };

        let enclosing_docblock_return: Option<PhpType> = if matches!(
            return_strategy,
            ReturnStrategy::TrailingReturn | ReturnStrategy::SentinelNull
        ) {
            crate::docblock::find_enclosing_return_type(content, start)
        } else {
            None
        };

        // ── PHPDoc generation ───────────────────────────────────────
        let return_type_for_docblock = build_return_type_hint_for_docblock(
            &return_strategy,
            &trailing_return_type,
            &typed_returns,
        );
        let raw_return_type_for_docblock = build_raw_return_type_for_docblock(
            &return_strategy,
            &trailing_return_type,
            enclosing_docblock_return.as_ref(),
            &typed_returns,
        );
        let ctx = self.file_context(uri);
        let class_loader = self.class_loader(&ctx);
        let docblock = build_docblock_for_extraction(
            &typed_params,
            &return_type_for_docblock,
            &raw_return_type_for_docblock,
            &member_indent,
            &class_loader,
        );

        // ── Build ExtractionInfo ────────────────────────────────────
        let params_for_info: Vec<(String, PhpType)> = typed_params
            .iter()
            .map(|(name, cleaned, _)| (name.clone(), cleaned.clone()))
            .collect();
        let returns_for_info: Vec<(String, PhpType)> = typed_returns
            .iter()
            .map(|(name, cleaned, _)| (name.clone(), cleaned.clone()))
            .collect();

        let info = ExtractionInfo {
            name: fn_name,
            params: params_for_info,
            returns: returns_for_info,
            body: body_text,
            target: enclosing.target,
            is_static: enclosing.is_static,
            member_indent,
            body_indent,
            return_strategy,
            trailing_return_type,
            docblock,
        };

        // ── Build edits ─────────────────────────────────────────────
        let definition = build_extracted_definition(&info);
        let call_site = build_call_site(&info, &call_indent);

        let doc_uri: Url = uri.parse().ok()?;

        let replace_start = find_line_start(content, start);
        let replace_end = find_line_end(content, end.saturating_sub(1).max(start));

        let replace_start_pos = offset_to_position(content, replace_start);
        let replace_end_pos = offset_to_position(content, replace_end);

        let insert_pos = offset_to_position(content, enclosing.insert_offset);

        let edits = vec![
            TextEdit {
                range: Range {
                    start: replace_start_pos,
                    end: replace_end_pos,
                },
                new_text: call_site,
            },
            TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: definition,
            },
        ];

        let mut changes = HashMap::new();
        changes.insert(doc_uri, edits);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

/// Clean a resolved type string for use in a function signature.
///
/// Removes generic parameters (PHP doesn't support them in signatures),
/// and simplifies union types that are too complex for type hints.
/// Compute the raw (un-cleaned) return type hint string for PHPDoc
/// enrichment purposes.  Unlike `build_return_type` (which strips
/// generics for native hints), this preserves the full type so that
/// `enrichment_plain` can detect whether a docblock `@return` tag is
/// warranted.
fn build_return_type_hint_for_docblock(
    strategy: &ReturnStrategy,
    trailing_return_type: &PhpType,
    returns: &[(String, PhpType, PhpType)],
) -> PhpType {
    match strategy {
        ReturnStrategy::TrailingReturn => trailing_return_type.clone(),
        ReturnStrategy::VoidGuards | ReturnStrategy::UniformGuards(_) => PhpType::bool(),
        ReturnStrategy::SentinelNull => {
            if !trailing_return_type.is_empty() {
                trailing_return_type.clone()
            } else {
                PhpType::untyped()
            }
        }
        ReturnStrategy::NullGuardWithValue(_) => {
            if returns.len() == 1 {
                if let Some(hint) = returns[0].1.to_native_hint_typed() {
                    return hint;
                }
                PhpType::untyped()
            } else {
                PhpType::untyped()
            }
        }
        ReturnStrategy::None | ReturnStrategy::Unsafe => {
            if returns.is_empty() {
                PhpType::void()
            } else if returns.len() == 1 {
                if let Some(hint) = returns[0].1.to_native_hint_typed() {
                    return hint;
                }
                PhpType::untyped()
            } else {
                PhpType::array()
            }
        }
    }
}

/// Like `build_return_type_hint_for_docblock` but returns the raw
/// (un-cleaned) type that preserves concrete generic arguments.
fn build_raw_return_type_for_docblock(
    strategy: &ReturnStrategy,
    trailing_return_type: &PhpType,
    enclosing_docblock_return: Option<&PhpType>,
    returns: &[(String, PhpType, PhpType)],
) -> PhpType {
    match strategy {
        ReturnStrategy::TrailingReturn => {
            // Prefer the docblock @return type when it carries concrete
            // generics (e.g. `Collection<User>`) over the native hint
            // (e.g. `Collection`).
            if let Some(edr) = enclosing_docblock_return
                && edr.has_type_parameters()
            {
                return edr.clone();
            }
            trailing_return_type.clone()
        }
        ReturnStrategy::VoidGuards | ReturnStrategy::UniformGuards(_) => PhpType::bool(),
        ReturnStrategy::SentinelNull => {
            if let Some(edr) = enclosing_docblock_return
                && edr.has_type_parameters()
            {
                return edr.clone();
            }
            if !trailing_return_type.is_empty() {
                trailing_return_type.clone()
            } else {
                PhpType::untyped()
            }
        }
        ReturnStrategy::NullGuardWithValue(_) => {
            // Use raw type (index 2) which preserves generics.
            if returns.len() == 1 && !returns[0].2.is_empty() {
                returns[0].2.clone()
            } else {
                PhpType::untyped()
            }
        }
        ReturnStrategy::None | ReturnStrategy::Unsafe => {
            if returns.is_empty() {
                PhpType::void()
            } else if returns.len() == 1 {
                // Use raw type (index 2) which preserves generics.
                returns[0].2.clone()
            } else {
                PhpType::array()
            }
        }
    }
}

/// Reduces a (possibly generic or docblock-style) type to a native PHP
/// type hint suitable for a function signature, e.g. `array<string>` →
/// `array`, `int[]` → `array`, `callable(int): string` → `callable`.
/// Returns `None` when the type has no valid native hint representation.
fn clean_type_for_signature_typed(ty: &PhpType) -> Option<PhpType> {
    ty.to_native_hint_typed()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "../extract_function_tests.rs"]
mod tests;
