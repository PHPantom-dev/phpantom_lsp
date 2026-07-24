//! Strategies for completion inside a `/** … */` docblock: `@tag` name
//! completion, and type/variable completion after a recognised tag
//! (`@param `, `@return `, `@throws `, `@var `).

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionResponse, CompletionTextEdit,
    Position, Range, TextEdit,
};

use crate::Backend;
use crate::completion::class_completion::{ClassCompletionParams, ClassNameContext};
use crate::docblock::type_strings::PHPDOC_TYPE_KEYWORDS;
use crate::php_type::PhpType;
use crate::types::FileContext;

impl Backend {
    // ─── Strategy: PHPDoc tag completion ─────────────────────────────────

    /// Build completions for `@tag` names inside a `/** … */` docblock.
    ///
    /// Called when [`crate::completion::phpdoc::extract_phpdoc_prefix`]
    /// detects that the cursor follows an `@` sign inside a docblock.
    /// Always returns a response (possibly with an empty item list) so
    /// that partial tags like `@potato` never fall through to
    /// class/constant/function completion.
    pub(super) fn complete_phpdoc_tag(
        &self,
        content: &str,
        prefix: &str,
        position: Position,
        ctx: &FileContext,
    ) -> CompletionResponse {
        let context = crate::completion::phpdoc::detect_context(content, position);
        let class_loader = self.class_loader(ctx);
        let function_loader = self.function_loader(ctx);

        // For inline variable assignments, try to infer the type from
        // the assignment RHS so that @var can be pre-filled.
        let inferred_var_type =
            if matches!(context, crate::completion::phpdoc::DocblockContext::Inline) {
                let sym = crate::completion::phpdoc::extract_symbol_info(content, position);
                crate::completion::phpdoc::generation::infer_inline_variable_type(
                    &sym,
                    content,
                    position,
                    &ctx.classes,
                    &class_loader,
                    Some(
                        &function_loader
                            as &dyn Fn(&str, u32) -> Option<crate::types::FunctionInfo>,
                    ),
                )
            } else {
                None
            };

        let smart = crate::completion::phpdoc::SmartContext {
            inferred_inline_var_type: inferred_var_type,
            class_loader: Some(&class_loader),
            function_loader: Some(&function_loader),
        };
        let items = crate::completion::phpdoc::build_phpdoc_completions(
            content,
            prefix,
            context,
            position,
            &ctx.use_map,
            &ctx.namespace,
            &smart,
        );
        CompletionResponse::Array(items)
    }

    // ─── Strategy: docblock type / variable completion ───────────────────

    /// Build completions at a type or variable position inside a docblock.
    ///
    /// When the cursor is inside a `/** … */` docblock at a recognised tag
    /// position (e.g. after `@param `, `@return `, `@throws `, `@var `),
    /// offer class-name or `$variable` completions as appropriate.  At all
    /// other docblock positions (descriptions, unknown tags) return `None`
    /// so that random words don't trigger class/variable suggestions.
    pub(super) fn complete_docblock_type_or_variable(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        uri: &str,
    ) -> Option<CompletionResponse> {
        use crate::completion::phpdoc::{
            DocblockTypingContext, detect_docblock_typing_position, extract_symbol_info,
        };

        match detect_docblock_typing_position(content, position) {
            Some(DocblockTypingContext::Type { partial, tag }) => {
                // For @throws, use Throwable-filtered completion with
                // the same ordering as `throw new` so that exception
                // classes appear at the top.
                if tag == "throws" {
                    let (class_items, class_incomplete) =
                        self.build_class_name_completions(ClassCompletionParams {
                            file_use_map: &ctx.use_map,
                            file_namespace: &ctx.namespace,
                            prefix: &partial,
                            content,
                            context: ClassNameContext::Catch,
                            position,
                            affinity_table_override: None,
                            uri,
                        });
                    return if class_items.is_empty() {
                        None
                    } else {
                        Some(CompletionResponse::List(CompletionList {
                            is_incomplete: class_incomplete,
                            items: class_items,
                        }))
                    };
                }

                // Offer scalar / built-in types first, then class
                // / interface / enum names from the project.
                let partial_lower = partial.to_lowercase();
                let mut items: Vec<CompletionItem> = PHPDOC_TYPE_KEYWORDS
                    .iter()
                    .filter(|t| t.to_lowercase().starts_with(&partial_lower))
                    .enumerate()
                    .map(|(idx, t)| CompletionItem {
                        label: t.to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        detail: Some("PHP built-in type".to_string()),
                        insert_text: Some(t.to_string()),
                        filter_text: Some(t.to_string()),
                        sort_text: Some(format!("0_scalar_{:03}", idx)),
                        ..CompletionItem::default()
                    })
                    .collect();

                let (class_items, class_incomplete) =
                    self.build_class_name_completions(ClassCompletionParams {
                        file_use_map: &ctx.use_map,
                        file_namespace: &ctx.namespace,
                        prefix: &partial,
                        content,
                        context: ClassNameContext::TypeHint,
                        position,
                        affinity_table_override: None,
                        uri,
                    });
                items.extend(class_items);

                if items.is_empty() {
                    None
                } else {
                    Some(CompletionResponse::List(CompletionList {
                        is_incomplete: class_incomplete,
                        items,
                    }))
                }
            }
            Some(DocblockTypingContext::Variable { partial }) => {
                // Offer $parameter names from the function declaration.
                let sym = extract_symbol_info(content, position);
                let partial_lower = partial.to_lowercase();

                // Compute an explicit replacement range covering the typed
                // `$…` prefix.  Using `text_edit` with a range prevents
                // the double-dollar problem in editors (Helix, Neovim) that
                // don't treat `$` as a word character — the same fix that
                // was applied to regular variable completion.
                let prefix_char_len = partial.chars().count() as u32;
                let replace_range = Range {
                    start: Position {
                        line: position.line,
                        character: position.character.saturating_sub(prefix_char_len),
                    },
                    end: position,
                };

                let items: Vec<CompletionItem> = sym
                    .params
                    .iter()
                    .filter(|(_, name)| {
                        partial_lower.is_empty() || name.to_lowercase().starts_with(&partial_lower)
                    })
                    .map(|(type_hint, name)| {
                        let detail = type_hint
                            .as_ref()
                            .map(|t| t.to_string())
                            .unwrap_or_else(|| PhpType::mixed().to_string());
                        CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::VARIABLE),
                            detail: Some(detail),
                            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                                range: replace_range,
                                new_text: name.clone(),
                            })),
                            filter_text: Some(name.clone()),
                            sort_text: Some(format!("0_{}", name.to_lowercase())),
                            ..CompletionItem::default()
                        }
                    })
                    .collect();
                if items.is_empty() {
                    None
                } else {
                    Some(CompletionResponse::Array(items))
                }
            }
            None => {
                // Description text or unrecognised position — no
                // completions.
                None
            }
        }
    }
}
