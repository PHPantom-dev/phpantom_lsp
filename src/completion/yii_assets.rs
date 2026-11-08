//! Yii asset bundle completion: the files, and the directories holding
//! more, under the directory an asset bundle's `$css` and `$js` entries are
//! relative to.
//!
//! The bundle is judged the way the symbol map judges it (see
//! `crate::yii::assets`), from a parse of the buffer, since the directory
//! is read from the class around the entry.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{CompletionResponse, Position};

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::text_position::position_to_offset;
use crate::types::FileContext;
use crate::yii::assets::{bundle_dir, bundle_entries, is_asset_bundle};

impl Backend {
    /// Complete the file an asset bundle's `$css` or `$js` entry is being
    /// typed as.
    pub(crate) fn try_yii_asset_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        // Every bundle names the class it extends, so a file that does not
        // is not parsed on each keystroke.
        memchr::memmem::find(content.as_bytes(), b"AssetBundle")?;
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        let literal_start = quote + 1;
        let typed = content.get(literal_start..cursor)?;
        if typed.starts_with('/') || typed.contains("://") {
            return None;
        }
        let file = crate::util::uri_to_path(uri)?;
        let file_dir = file.parent()?;
        let dir =
            crate::parser::with_parsed_program(content, "yii_asset_completion", |program, _| {
                let mut found = None;
                let finder = BundleFinder {
                    quote: quote as u32,
                };
                for statement in program.statements.iter() {
                    mago_syntax::walker::Walker::walk_statement(&finder, statement, &mut found);
                }
                let class = found?;
                let is_bundle = is_asset_bundle(class, |parent| {
                    ctx.resolve_name_at(
                        crate::atom::bytes_to_str(parent.value()),
                        parent.span().start.offset,
                    )
                });
                let names_entry =
                    bundle_entries(class).any(|entry| entry.span().start.offset as usize == quote);
                if !is_bundle || !names_entry {
                    return None;
                }
                bundle_dir(class, file_dir)
            })?;
        let base = self.yii_asset_dir(&dir)?;
        crate::completion::laravel_paths::directory_entry_completion(
            &base,
            typed,
            literal_start,
            content,
            position,
        )
    }
}

/// Finds the innermost class around the string whose quote is at `quote`.
struct BundleFinder {
    quote: u32,
}

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Option<&'ast Class<'arena>>>
    for BundleFinder
{
    fn walk_in_class(&self, node: &'ast Class<'arena>, found: &mut Option<&'ast Class<'arena>>) {
        let span = node.span();
        if span.start.offset <= self.quote && self.quote < span.end.offset {
            *found = Some(node);
        }
    }
}
