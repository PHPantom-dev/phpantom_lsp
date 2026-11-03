//! Typed Laravel connection completion, sharing the indexed receiver checks.

use super::*;
use crate::completion::eloquent_string::{extract_identifier_backwards, extract_receiver_subject};
use crate::symbol_map::ReceiverClass;
use crate::symbol_map::extraction::laravel::connection_receiver_method;
use crate::types::AccessKind;

impl Backend {
    /// Complete a connection selected by a typed call or property value.
    pub(super) fn try_laravel_connection_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let cursor = crate::text_position::position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        let kind = if let Some((callable, slot)) = scalar_argument_at(content, code) {
            let (method, _) = extract_identifier_backwards(callable)?;
            let (receiver, parameter) = connection_receiver_method(method)?;
            if !slot.is_first_or_named(parameter) {
                return None;
            }
            let operator = code.enclosing_paren()?.callee_operator?;
            if operator.is_static {
                return None;
            }
            let subject = extract_receiver_subject(content, &operator)?;
            self.connection_subject_kind(&subject, receiver, content, cursor, ctx)?
        } else {
            let lhs = code.code_before.strip_suffix('=')?.trim_end();
            let (property, before) = extract_identifier_backwards(lhs)?;
            if property != "connection" {
                return None;
            }
            if before.ends_with('$') {
                let name_offset = before.len() as u32 - 1;
                let loaders = self.file_loaders(ctx);
                let class = self.connection_property_class(content, code, ctx, name_offset)?;
                let class_loader = |name: crate::Fqn<'_>| {
                    if name.as_str().eq_ignore_ascii_case(&class.fqn()) {
                        Some(Arc::clone(&class))
                    } else {
                        loaders.resolved_class_loader()(name)
                    }
                };
                self.typed_receiver_key_kind(
                    &crate::php_type::PhpType::named(class.fqn()),
                    "",
                    0,
                    ReceiverClass::LaravelConnectionProperty,
                    &class_loader,
                )?
            } else {
                let chars: Vec<char> = lhs.chars().collect();
                let (subject, access) =
                    crate::type_engine::subject_extraction::detect_access_operator(
                        &chars,
                        chars.len(),
                    )?;
                if access != AccessKind::Arrow {
                    return None;
                }
                self.connection_subject_kind(
                    &subject,
                    ReceiverClass::LaravelConnectionProperty,
                    content,
                    cursor,
                    ctx,
                )?
            }
        };
        self.laravel_string_key_response(
            content,
            position,
            LaravelStringKeyContext {
                kind,
                prefix: &content[quote + 1..cursor],
                content_start_offset: quote + 1,
            },
        )
    }

    /// Recover the declaration when an unfinished string prevents the
    /// normal parse from recording the property being edited.
    fn connection_property_class(
        &self,
        content: &str,
        code: &CodeContext<'_>,
        ctx: &FileContext,
        offset: u32,
    ) -> Option<Arc<crate::types::ClassInfo>> {
        let declares = |class: &crate::types::ClassInfo| {
            class
                .properties
                .iter()
                .any(|property| property.name_offset == offset && !property.is_static)
        };
        if let Some(class) = crate::class_lookup::find_shared_class_at_offset(&ctx.classes, offset)
            && declares(class)
        {
            return Some(Arc::clone(class));
        }
        let (quote, _) = code.open_string?;
        let mut patched = content[..quote].to_string();
        patched.push_str("'';");
        for bracket in code.open_brackets.iter().rev() {
            patched.push(match bracket.byte {
                b'(' => ')',
                b'[' => ']',
                _ => '}',
            });
        }
        patched.push(';');
        self.parse_php_resolved(&patched)
            .into_iter()
            .find(declares)
            .map(Arc::new)
    }

    fn connection_subject_kind(
        &self,
        subject: &str,
        receiver: ReceiverClass,
        content: &str,
        cursor: usize,
        ctx: &FileContext,
    ) -> Option<StringKeyKind> {
        let loaders = self.file_loaders(ctx);
        let rctx = loaders.resolution_ctx_at(content, cursor as u32);
        let ty = Self::resolve_arg_text_to_type(subject, &rctx)?;
        self.typed_receiver_key_kind(&ty, "", 0, receiver, loaders.resolved_class_loader())
    }
}
