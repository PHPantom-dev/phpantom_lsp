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
use tower_lsp_server::gen_lsp_types::{CompletionResponse, Position};

use crate::Backend;
use crate::completion::eloquent_string::detect_string_call_context;
use crate::completion::laravel_string_keys::{string_key_item_kind, string_key_response};
use crate::completion::source::code_context::CodeContext;
use crate::symbol_map::{ReceiverClass, StringKeyKind};
use crate::text_position::position_to_offset;
use crate::types::{AccessKind, FileContext};
use crate::yii::views::{is_view_render_method, view_candidates};

impl Backend {
    /// Complete the view a render call's string is being typed as.
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
        if !is_view_render_method(method) {
            return None;
        }
        let file = crate::util::uri_to_path(uri)?;
        let dir = file.parent()?;
        let between = content.get(paren.offset + 1..quote)?.trim();

        let base = if between.is_empty() {
            if !self.is_yii_view_receiver(content, cursor, code, ctx) {
                return None;
            }
            view_base_at(content, dir, quote as u32, ctx)?
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

/// Whether the code between a call's `(` and the string is `__DIR__ .`.
fn is_dir_concatenation(between: &str) -> bool {
    between
        .strip_suffix('.')
        .is_some_and(|dir| dir.trim_end() == "__DIR__")
}

/// The view path a name passed at `quote` is relative to: the one the
/// call's own receiver chain or the class around it gives its renderer, or
/// outside a class, the template's own directory.
fn view_base_at(content: &str, dir: &Path, quote: u32, ctx: &FileContext) -> Option<String> {
    crate::parser::with_parsed_program(content, "yii_view_completion", |program, content| {
        let mut found = ViewBaseFinder::default();
        let finder = ViewBaseWalker { quote };
        for statement in program.statements.iter() {
            mago_syntax::walker::Walker::walk_statement(&finder, statement, &mut found);
        }
        let Some(class) = found.class else {
            return Some(dir.to_string_lossy().into_owned());
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
