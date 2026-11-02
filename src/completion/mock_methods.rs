//! Completion of the method a mock expectation names.
//!
//! Inside `$repo->method('|')`, `$mailer->shouldReceive('|')`,
//! `$mailer->allows(['|' => …])` or `Cache::shouldReceive('|')`, offer the
//! methods of the mocked class (or the facade's target).  Which calls
//! count, and which class a receiver mocks, come from
//! [`crate::mock_methods`], the same rules the symbol map's spans follow.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{CompletionResponse, Position};

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::completion::source::string_argument::{StringArgumentPosition, string_argument};
use crate::text_position::position_to_offset;
use crate::type_engine::resolver::resolve_target_classes_expr;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::{AccessKind, ClassInfo, FileContext, ResolvedType};

use super::eloquent_string::{extract_identifier_backwards, extract_receiver_subject};

impl Backend {
    /// Try mock expectation method-name completion.
    pub(crate) fn try_mock_method_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let (quote, _) = code.open_string?;
        let cursor_offset = position_to_offset(content, position) as usize;
        let call = code.open_brackets.iter().rev().find(|bracket| {
            bracket.byte == b'('
                && !extract_identifier_backwards(&content[..bracket.code_before])
                    .is_some_and(|(name, _)| name.eq_ignore_ascii_case("array"))
        })?;
        let operator = call.callee_operator.as_ref()?;
        let (method, _) = extract_identifier_backwards(&content[..call.code_before])?;
        let kind = crate::mock_methods::expectation_name_args(method, operator.is_static)?;

        let argument = string_argument(content, cursor_offset, code, call)?;
        if argument.name.is_some() || !kind.names_at(argument.index) {
            return None;
        }
        let names_a_method = match argument.position {
            StringArgumentPosition::Direct => true,
            StringArgumentPosition::ArrayKey | StringArgumentPosition::ArrayElement => {
                argument.index == 0 && kind.takes_map()
            }
            StringArgumentPosition::ArrayValue => false,
        };
        if !names_a_method {
            return None;
        }

        let subject = extract_receiver_subject(content, operator)?;
        let subject = SubjectExpr::parse(&subject);
        // `$mock->expects($this->once())->method('|')` configures the mock
        // the `expects()` was called on, as the symbol map reads it.
        let receiver = match &subject {
            SubjectExpr::CallExpr { callee, .. } if method.eq_ignore_ascii_case("method") => {
                match callee.as_ref() {
                    SubjectExpr::MethodCall { base, method }
                        if method.eq_ignore_ascii_case("expects") =>
                    {
                        base.as_ref()
                    }
                    _ => &subject,
                }
            }
            _ => &subject,
        };

        let cursor = cursor_offset as u32;
        let loaders = self.file_loaders(ctx);
        let class_loader = loaders.resolved_class_loader();
        let access_kind = if operator.is_static {
            AccessKind::DoubleColon
        } else {
            AccessKind::Arrow
        };
        let resolved = ResolvedType::into_arced_classes(resolve_target_classes_expr(
            receiver,
            access_kind,
            &loaders.resolution_ctx_at(content, cursor),
        ));
        let mocked =
            crate::mock_methods::mocked_classes(resolved, operator.is_static, class_loader);
        // A facade's static members are its target's instance methods, so
        // the target is what is offered: the facade's own `shouldReceive()`
        // and `swap()` are not among them.
        let classes: Vec<Arc<ClassInfo>> = if operator.is_static {
            mocked
                .iter()
                .filter_map(|facade| {
                    crate::virtual_members::laravel::facade_concrete_class(
                        facade,
                        class_loader,
                        Some(&self.resolved_class_cache),
                    )
                })
                .collect()
        } else {
            mocked
        };
        if classes.is_empty() {
            return None;
        }

        self.string_method_name_completions(
            uri,
            ctx,
            cursor,
            &classes,
            &content[quote + 1..cursor_offset],
        )
    }
}
