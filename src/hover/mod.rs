//! Hover support (`textDocument/hover`).
//!
//! This module resolves the symbol under the cursor and returns a
//! human-readable description including type information, method
//! signatures, and docblock descriptions.
//!
//! The implementation reuses the same symbol-map lookup that powers
//! go-to-definition, and the same type-resolution pipeline that
//! powers completion.
//!
//! [`Backend::handle_hover`] dispatches by symbol kind into the sibling
//! submodules: [`member`] (methods / properties / constants),
//! [`variable`], [`class`] (also `self` / `static` / `parent` / `$this`),
//! [`function`], [`constants`], and [`laravel`]; [`see_refs`] and
//! [`templates`] serve several of them.  The [`formatting`] submodule
//! holds the shared Markdown builders.

mod class;
mod constants;
mod formatting;
mod function;
mod laravel;
mod laravel_trans;
mod member;
mod see_refs;
mod templates;
mod validation_strings;
mod variable;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::find_class_at_offset;
use crate::references::MemberRefKind;
use crate::resolution::FileLoaders;
use crate::symbol_map::{SymbolKind, SymbolSpan};
use crate::types::*;

use formatting::*;
use laravel::hover_command_own_param;

// Re-export `pub(crate)` items so external callers keep using `crate::hover::`.
pub(crate) use formatting::{
    extract_description_from_info, extract_var_description_from_info, hover_for_function,
    shorten_php_type,
};
pub(crate) use member::{MemberKindForOrigin, find_declaring_class};

/// A symbol-map hit's position, with the file's context and loaders read
/// once for whichever hover path the symbol's kind takes.
struct HoverSite<'a> {
    uri: &'a str,
    content: &'a str,
    offset: u32,
    ctx: &'a FileContext,
    loaders: &'a FileLoaders<'a>,
}

impl HoverSite<'_> {
    /// The class the hovered code is written in, if any.
    fn current_class(&self) -> Option<&ClassInfo> {
        self.loaders.enclosing_class(self.offset)
    }
}

impl Backend {
    /// Return a Markdown provenance line for a class FQN, or `None` for
    /// project-local classes.
    pub(crate) fn provenance_line_for_class(&self, fqn: &str) -> Option<String> {
        let class_uri = self.class_declaration_uri(fqn)?;
        let (origin, pkg_name) = self.package_info_for_uri(&class_uri);
        format_provenance_line(origin, pkg_name.as_deref())
    }

    /// Return a Markdown provenance line for a function by name.
    ///
    /// By the time hover asks for provenance the function has already
    /// been resolved, so `global_functions` holds its defining URI
    /// (including `phpantom-stub-fn://` for built-ins). Falls back to
    /// the autoload index for functions discovered by the byte-level
    /// scan but not yet parsed.
    pub(crate) fn provenance_line_for_function(&self, func_name: &str) -> Option<String> {
        let uri = self
            .symbols
            .global_functions
            .read()
            .get(func_name)
            .map(|(uri, _)| uri.clone());
        if let Some(uri) = uri {
            let (origin, pkg_name) = self.package_info_for_uri(&uri);
            return format_provenance_line(origin, pkg_name.as_deref());
        }
        let path = self
            .symbols
            .autoload_function_index
            .read()
            .get(func_name)
            .cloned()?;
        let (origin, pkg_name) = self.package_info_for_path(&path);
        format_provenance_line(origin, pkg_name.as_deref())
    }

    /// Handle a `textDocument/hover` request.
    ///
    /// Returns `Some(Hover)` when the symbol under the cursor can be
    /// resolved to a meaningful description, or `None` when resolution
    /// fails or the cursor is not on a navigable symbol.
    pub fn handle_hover(&self, uri: &str, content: &str, position: Position) -> Option<Hover> {
        // Inside a request this re-enters the scope `with_file_content`
        // opened; a caller holding its own content (the type-assertion
        // runner, in-process tests) gets the same caches a request does.
        let _scope = self.enter_request_scope_borrowed(uri, content);
        let offset = crate::text_position::position_to_offset(content, position);

        if let Some(symbol) = self.lookup_symbol_map(uri, offset)
            && let Some(Some(mut hover)) =
                crate::util::catch_panic_unwind_safe("hover", uri, Some(position), || {
                    self.hover_from_symbol(&symbol, uri, content, offset)
                })
        {
            hover.range = Some(symbol_span_to_range(content, &symbol));
            return Some(hover);
        }

        // Retry one byte earlier for end-of-token edge cases.
        if offset > 0
            && let Some(symbol) = self.lookup_symbol_map(uri, offset - 1)
            && let Some(Some(mut hover)) =
                crate::util::catch_panic_unwind_safe("hover", uri, Some(position), || {
                    self.hover_from_symbol(&symbol, uri, content, offset - 1)
                })
        {
            hover.range = Some(symbol_span_to_range(content, &symbol));
            return Some(hover);
        }

        if let Some(hover) = self.validation_string_hover(uri, content, position) {
            return Some(hover);
        }

        // ── model-property<Model> string hover ─────────────────
        // When the cursor is inside a string argument whose parameter
        // is typed as model-property<Model>, show the property's type
        // and source info (same as hovering over $model->property).
        if let Some(hover) = self.try_model_property_hover(uri, content, position) {
            return Some(hover);
        }

        // ── dropped trailing statement ──────────────────────────
        // An incomplete construct at the end of a file (`$obj->` or
        // `Foo::` with nothing typed after it yet) is a parse error mago's
        // recovery drops entirely rather than keeping as an error node, so
        // the symbol map has no span there at all — not just a gap between
        // spans, but nothing past the last one it could build. Recover the
        // bare `$variable` or class name touching the cursor from the raw
        // text and feed it into the same variable/class hover paths a
        // symbol-map hit would have used.
        if let Some(hover) = self.hover_dropped_tail_identifier(uri, content, offset) {
            return Some(hover);
        }

        None
    }

    /// Fallback for [`handle_hover`] when the symbol map has nothing at or
    /// after `offset`: read the `$variable` or bare identifier touching the
    /// cursor directly from `content` and hover it through the normal
    /// variable/class paths.
    ///
    /// Only consulted when nothing in the map extends this far, so it never
    /// second-guesses successfully parsed code, and a cursor inside a
    /// string literal or comment is left alone.
    fn hover_dropped_tail_identifier(
        &self,
        uri: &str,
        content: &str,
        offset: u32,
    ) -> Option<Hover> {
        let last_span_end = {
            let map = self.symbol_map_for(uri)?;
            if !map.spans.iter().all(|s| s.end <= offset) {
                return None;
            }
            map.spans.iter().map(|s| s.end).max().unwrap_or(0)
        };

        let (is_variable, name, token_start) = identifier_token_at(content, offset as usize)?;
        // Text past the last span is not necessarily code: a string literal
        // or comment there is left alone, as one before it is.
        if inside_string_or_comment(
            content.as_bytes(),
            last_span_end as usize,
            token_start as usize,
        ) {
            return None;
        }

        let ctx = self.file_context_at(uri, offset);
        let current_class = find_class_at_offset(&ctx.classes, offset);

        let mut hover = if is_variable {
            self.hover_variable(&name, uri, content, offset, current_class, &ctx)?
        } else {
            let class_loader = self.class_loader(&ctx);
            self.hover_class_reference(&name, uri, content, &class_loader, offset)?
        };
        let token_end = token_start + name.len() as u32 + u32::from(is_variable);
        hover.range = Some(Range {
            start: crate::text_position::offset_to_position(content, token_start as usize),
            end: crate::text_position::offset_to_position(content, token_end as usize),
        });
        Some(hover)
    }

    /// Dispatch a symbol-map hit to the hover path for its kind.
    fn hover_from_symbol(
        &self,
        symbol: &SymbolSpan,
        uri: &str,
        content: &str,
        cursor_offset: u32,
    ) -> Option<Hover> {
        let ctx = self.file_context_at(uri, cursor_offset);
        let loaders = self.file_loaders(&ctx);
        let site = HoverSite {
            uri,
            content,
            offset: cursor_offset,
            ctx: &ctx,
            loaders: &loaders,
        };

        match &symbol.kind {
            SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => {
                self.hover_variable_symbol(&site, symbol, name)
            }
            SymbolKind::MemberAccess {
                subject_text,
                member_name,
                is_static,
                ..
            } => self.hover_member_access(
                &site,
                subject_text,
                member_name,
                *is_static,
                MemberRefKind::of_access(symbol)?,
            ),
            SymbolKind::ClassReference {
                name,
                is_fqn,
                context,
            } => self.hover_class_symbol(&site, name, *is_fqn, *context),
            SymbolKind::SelfStaticParent(ssp_kind) => {
                self.hover_self_static_parent(&site, *ssp_kind)
            }
            // The user is already at the declaration: hover would just
            // repeat the signature, docblock or value that is on screen.
            SymbolKind::ClassDeclaration { .. }
            | SymbolKind::MemberDeclaration { .. }
            | SymbolKind::NamespaceDeclaration { .. }
            | SymbolKind::FunctionCall {
                is_definition: true,
                ..
            }
            | SymbolKind::ConstantReference {
                is_definition: true,
                ..
            } => None,
            SymbolKind::FunctionCall {
                name,
                is_docblock_reference,
                ..
            } => self.hover_function_symbol(&site, symbol.start, name, *is_docblock_reference),
            SymbolKind::ConstantReference { name, .. } => {
                self.hover_constant_reference(&site, symbol.start, name)
            }
            SymbolKind::StringKey { kind, key, .. } => {
                if !self.string_key_kind_enabled(*kind) {
                    return None;
                }
                self.hover_laravel_string_key(kind, key, uri)
            }
            SymbolKind::CommandOwnParam { name, is_option } => {
                hover_command_own_param(content, cursor_offset as usize, name, *is_option)
            }
            SymbolKind::LaravelMacroString { .. }
            | SymbolKind::Keyword
            | SymbolKind::CastType
            | SymbolKind::Comment => None,
        }
    }
}

/// Whether `target` lies inside a string literal or comment, scanning
/// from `from`, a point known to be outside both.
fn inside_string_or_comment(bytes: &[u8], from: usize, target: usize) -> bool {
    let mut pos = from;
    while pos < target {
        let end = match bytes[pos] {
            b'\'' | b'"' => crate::text_scan::skip_string_forward(bytes, pos),
            _ => match crate::text_scan::skip_php_comment(bytes, pos) {
                Some(end) => end,
                None => {
                    pos += 1;
                    continue;
                }
            },
        };
        if end > target {
            return true;
        }
        pos = end;
    }
    false
}

/// Whether `c` can appear inside a PHP identifier.  PHP allows every byte
/// from `0x80` up, which is why this is not just `is_alphanumeric`.
fn is_php_identifier_char(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric() || (c as u32) >= 0x80
}

/// Find the `$variable` or bare identifier touching byte offset `offset`
/// in `content`, for cursor positions no AST node covers.
///
/// Returns `(is_variable, name, token_start)`: `name` excludes the `$`
/// sigil, and `token_start` is the byte offset of the `$` for a variable
/// or of the identifier's first byte otherwise. `None` when the offset
/// (or the byte just before it, for end-of-token cursors) does not sit on
/// an identifier character.
fn identifier_token_at(content: &str, offset: usize) -> Option<(bool, String, u32)> {
    let anchor = if content
        .get(offset..)
        .and_then(|s| s.chars().next())
        .is_some_and(is_php_identifier_char)
    {
        offset
    } else {
        let prev = content.get(..offset).and_then(|s| s.chars().next_back())?;
        if !is_php_identifier_char(prev) {
            return None;
        }
        offset - prev.len_utf8()
    };

    let mut start = anchor;
    while start > 0 {
        let Some(prev) = content[..start].chars().next_back() else {
            break;
        };
        if !is_php_identifier_char(prev) {
            break;
        }
        start -= prev.len_utf8();
    }

    let mut end = anchor;
    if let Some(c) = content[anchor..].chars().next()
        && is_php_identifier_char(c)
    {
        end += c.len_utf8();
    }
    while end < content.len() {
        let Some(c) = content[end..].chars().next() else {
            break;
        };
        if !is_php_identifier_char(c) {
            break;
        }
        end += c.len_utf8();
    }

    let is_variable = start > 0 && content.as_bytes()[start - 1] == b'$';
    let name = content[start..end].to_string();
    let token_start = if is_variable { start - 1 } else { start };
    Some((is_variable, name, token_start as u32))
}

#[cfg(test)]
mod tests;
