//! Yii view completion: the views, and the directories holding more, under
//! the path the view being typed is relative to.
//!
//! The call is judged the way the symbol map judges it (see
//! `symbol_map/extraction/yii.rs`): a name passed to a render call whose
//! receiver the type engine confirms is a view renderer or a view, relative
//! to the view path the class gives its renderer or, in a template, to the
//! template's own directory, and a path built from `__DIR__` relative to
//! the file's directory whatever the receiver.  The buffer is parsed for
//! the view path, since it is read from the class around the call.

use std::path::Path;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{
    CompletionItem, CompletionItemTextEdit, CompletionResponse, Position, Range, TextEdit,
};

use crate::Backend;
use crate::completion::eloquent_string::detect_string_call_context;
use crate::completion::laravel_string_keys::{string_key_item_kind, string_key_response};
use crate::completion::source::code_context::CodeContext;
use crate::symbol_map::{ReceiverClass, StringKeyKind};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::{AccessKind, FileContext};
use crate::yii::views::{is_layout_method, is_view_render_method, view_candidates};

impl Backend {
    /// Complete the view a render call's or a `withLayout()`'s string is
    /// being typed as.
    pub(crate) fn try_yii_view_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        let literal_start = quote + 1;
        let typed = content.get(literal_start..cursor)?;
        let paren = code.enclosing_paren()?;
        if paren.commas != 0 || paren.callee_operator.is_none_or(|op| op.is_static) {
            return None;
        }
        let method = content.get(paren.callee_name_start?..paren.code_before)?;
        let is_layout = is_layout_method(method);
        if !is_layout && !is_view_render_method(method) {
            return None;
        }
        // Only the renderer resolves an alias, and only in a layout.
        if !is_layout && typed.starts_with('@') {
            return None;
        }
        let file = crate::util::uri_to_path(uri)?;
        let dir = file.parent()?;
        let between = content.get(paren.offset + 1..quote)?.trim();

        let base = if between.is_empty() {
            if !self.is_yii_view_receiver(content, cursor, code, ctx) {
                return None;
            }
            if is_layout && typed.starts_with('@') {
                String::new()
            } else {
                view_base_at(content, dir, quote as u32, ctx, is_layout)?
            }
        } else if is_dir_concatenation(between) && typed.starts_with('/') {
            dir.to_string_lossy().into_owned()
        } else {
            return None;
        };

        let names = view_candidates(&self.yii_aliases(), &base, typed, Some(&file));
        string_key_response(
            names,
            string_key_item_kind(&StringKeyKind::YiiView),
            content,
            literal_start,
            position,
        )
    }

    /// Complete a key of the parameters a render call passes its view, from
    /// the `@var` block the template opens with.
    ///
    /// The view is read from the call's first argument the way
    /// [`Self::try_yii_view_completion`] reads one being typed: a name the
    /// receiver's type confirms, or a path built from `__DIR__`.
    pub(crate) fn try_yii_view_parameter_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        if !matches!(code.last_code_byte(), Some(b'[' | b',')) {
            return None;
        }
        let (bracket, _) = code.nested_pair(b'[', b'(')?;
        let paren = code.enclosing_paren()?;
        if paren.commas != 1 || paren.callee_operator.is_none_or(|op| op.is_static) {
            return None;
        }
        let method = content.get(paren.callee_name_start?..paren.code_before)?;
        if !is_view_render_method(method) {
            return None;
        }
        let file = crate::util::uri_to_path(uri)?;
        let dir = file.parent()?;
        let dir_text = dir.to_string_lossy();
        let first = content.get(paren.offset + 1..bracket)?.trim();
        let first = first.strip_suffix(',')?.trim_end();
        let view = if let Some(path) = first
            .strip_prefix("__DIR__")
            .and_then(|rest| rest.trim_start().strip_prefix('.'))
        {
            let path = plain_string_literal(path.trim_start())?;
            if !path.starts_with('/') {
                return None;
            }
            crate::yii::views::normalize_view_path(&format!("{dir_text}{path}"))
        } else {
            let name = plain_string_literal(first)?;
            if !self.is_yii_view_receiver(content, cursor, code, ctx) {
                return None;
            }
            let name_quote =
                paren.offset + 1 + content.get(paren.offset + 1..)?.find(['\'', '"'])?;
            let base = view_base_at(content, dir, name_quote as u32, ctx, false)?;
            let current = (base == dir_text).then_some(dir_text.as_ref());
            crate::yii::views::view_key(name, Some(&base), current)?
        };

        let prefix = content.get(quote + 1..cursor)?.as_bytes();
        let edit_range = Range {
            start: offset_to_position(content, quote + 1),
            end: position,
        };
        let items: Vec<CompletionItem> = self
            .yii_view_parameter_names(&view)
            .into_iter()
            .filter(|(name, _)| {
                name.as_bytes()
                    .get(..prefix.len())
                    .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
            })
            .enumerate()
            .map(|(index, (name, ty))| CompletionItem {
                label: name.clone(),
                kind: Some(string_key_item_kind(&StringKeyKind::YiiViewParameter)),
                detail: Some(ty),
                sort_text: Some(format!("{index:05}")),
                filter_text: Some(name.clone()),
                text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range: edit_range,
                    new_text: name,
                })),
                ..Default::default()
            })
            .collect();
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }

    /// Whether the receiver of the call the cursor's string is the first
    /// argument of is a view renderer or a view.
    fn is_yii_view_receiver(
        &self,
        content: &str,
        cursor: usize,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> bool {
        let Some(call) = detect_string_call_context(content, cursor, code) else {
            return false;
        };
        let Some(subject) = call.subject.as_deref() else {
            return false;
        };
        let loaders = self.file_loaders(ctx);
        let class_loader = loaders.resolved_class_loader();
        let rctx = loaders.resolution_ctx_at(content, cursor as u32);
        crate::type_engine::resolver::resolve_target_classes(subject, AccessKind::Arrow, &rctx)
            .into_iter()
            .filter_map(|resolved| resolved.class_info)
            .any(|class| {
                ReceiverClass::YiiViewRenderer
                    .targets()
                    .iter()
                    .any(|(fqn, _)| crate::class_lookup::is_subtype_of(&class, fqn, class_loader))
            })
    }
}

/// The contents of `text` when it is one string literal with nothing to
/// interpolate or unescape.
fn plain_string_literal(text: &str) -> Option<&str> {
    let quote = text.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let inner = text.strip_prefix(quote)?.strip_suffix(quote)?;
    (!inner.contains([quote, '\\', '$'])).then_some(inner)
}

/// Whether the code between a call's `(` and the string is `__DIR__ .`.
fn is_dir_concatenation(between: &str) -> bool {
    between
        .strip_suffix('.')
        .is_some_and(|dir| dir.trim_end() == "__DIR__")
}

/// The view path a name passed at `quote` is relative to: the one the
/// call's own receiver chain or the class around it gives its renderer, or
/// outside a class, the template's own directory, which a layout is never
/// relative to.
fn view_base_at(
    content: &str,
    dir: &Path,
    quote: u32,
    ctx: &FileContext,
    is_layout: bool,
) -> Option<String> {
    crate::parser::with_parsed_program(content, "yii_view_completion", |program, content| {
        let mut found = ViewBaseFinder::default();
        let finder = ViewBaseWalker { quote };
        for statement in program.statements.iter() {
            mago_syntax::walker::Walker::walk_statement(&finder, statement, &mut found);
        }
        let Some(class) = found.class else {
            return (!is_layout).then(|| dir.to_string_lossy().into_owned());
        };
        let class_fqn = {
            let short = crate::atom::bytes_to_str(class.name.value);
            match ctx.namespace_at(class.span().start.offset) {
                Some(namespace) => format!("{namespace}\\{short}"),
                None => short.to_string(),
            }
        };
        if let Some(object) = found.receiver
            && let Some(base) =
                crate::symbol_map::extraction::chain_view_base(object, dir, Some(&class_fqn))
        {
            return base;
        }
        crate::symbol_map::extraction::class_view_base(class, content, dir, Some(&class_fqn))
    })
}

/// What [`ViewBaseWalker`] found around the string.
#[derive(Default)]
struct ViewBaseFinder<'ast, 'arena> {
    /// The innermost class the string is in.
    class: Option<&'ast Class<'arena>>,
    /// The receiver of the call the string is the first argument of.
    receiver: Option<&'ast Expression<'arena>>,
}

/// Finds the class and the call around the string whose quote is at
/// `quote`.
struct ViewBaseWalker {
    quote: u32,
}

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, ViewBaseFinder<'ast, 'arena>>
    for ViewBaseWalker
{
    fn walk_in_class(&self, node: &'ast Class<'arena>, found: &mut ViewBaseFinder<'ast, 'arena>) {
        let span = node.span();
        if span.start.offset <= self.quote && self.quote < span.end.offset {
            found.class = Some(node);
        }
    }

    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        found: &mut ViewBaseFinder<'ast, 'arena>,
    ) {
        if node
            .argument_list
            .arguments
            .first()
            .is_some_and(|argument| argument.value().span().start.offset == self.quote)
        {
            found.receiver = Some(node.object);
        }
    }
}
