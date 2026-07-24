//! Document highlighting (`textDocument/documentHighlight`).
//!
//! When the cursor lands on a symbol, returns all other occurrences of
//! that symbol in the current file so the editor can highlight them.
//! This module reuses the precomputed [`SymbolMap`] — no additional
//! parsing or AST walking is needed.
//!
//! Highlight kind assignment:
//! - Variable on an assignment LHS, parameter, foreach binding, or
//!   catch binding → `DocumentHighlightKind::Write`
//! - Everything else → `DocumentHighlightKind::Read`
//!
//! Scope rules:
//! - **Variables** are scoped to their enclosing function/method/closure.
//! - **Class names, member names, function names, constants** are
//!   file-global — all occurrences in the file are highlighted.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind, SymbolMap, VarDefKind};
use crate::text_position::byte_range_to_lsp_range;
use crate::util::build_fqn;

impl Backend {
    /// Collect document highlights for the symbol under the cursor.
    ///
    /// Returns `None` when the cursor is not on a navigable symbol.
    pub fn handle_document_highlight(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Vec<DocumentHighlight>> {
        // Look up the symbol span at the cursor (retries one byte
        // earlier for end-of-token edge cases).
        let span = self.lookup_symbol_at_position(uri, content, position)?;

        let maps = self.symbol_maps.read();
        let symbol_map = maps.get(uri)?;

        let highlights = match &span.kind {
            SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => {
                // Check if this is actually a property declaration — if
                // so, highlight member accesses instead of local vars.
                if let Some(VarDefKind::Property) = symbol_map.var_def_kind_at(name, span.start) {
                    self.highlight_member_name(symbol_map, content, name)
                } else {
                    self.highlight_variable(symbol_map, content, name, span.start)
                }
            }
            SymbolKind::ClassReference { name, is_fqn, .. } => {
                let ctx = self.file_context(uri);
                let fqn = if *is_fqn {
                    name.clone()
                } else {
                    ctx.resolve_name_at(name, span.start)
                };
                self.highlight_class(symbol_map, content, &fqn, &ctx.use_map, &ctx.namespace)
            }
            SymbolKind::ClassDeclaration { name } => {
                let ctx = self.file_context(uri);
                let fqn = build_fqn(name, ctx.namespace.as_deref());
                self.highlight_class(symbol_map, content, &fqn, &ctx.use_map, &ctx.namespace)
            }
            SymbolKind::MemberAccess { member_name, .. } => {
                self.highlight_member_name(symbol_map, content, member_name)
            }
            SymbolKind::MemberDeclaration { name, .. } => {
                self.highlight_member_name(symbol_map, content, name)
            }
            SymbolKind::FunctionCall { name, .. } => {
                self.highlight_function(symbol_map, content, name)
            }
            SymbolKind::ConstantReference { name } => {
                self.highlight_constant(symbol_map, content, name)
            }
            SymbolKind::SelfStaticParent(ssp_kind) => {
                if *ssp_kind == SelfStaticParentKind::This {
                    self.highlight_this(symbol_map, content, span.start, uri)
                } else {
                    self.highlight_keyword(symbol_map, content, *ssp_kind, span.start, uri)
                }
            }
            SymbolKind::NamespaceDeclaration { .. }
            | SymbolKind::LaravelStringKey { .. }
            | SymbolKind::LaravelMacroString { .. }
            | SymbolKind::Keyword
            | SymbolKind::CastType
            | SymbolKind::Comment => Vec::new(),
        };

        if highlights.is_empty() {
            None
        } else {
            Some(highlights)
        }
    }

    /// Highlight all occurrences of a variable within the same scope.
    fn highlight_variable(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        var_name: &str,
        cursor_offset: u32,
    ) -> Vec<DocumentHighlight> {
        let scope_start = symbol_map.find_variable_scope(var_name, cursor_offset);
        let mut highlights = Vec::new();
        let mut seen_offsets = std::collections::HashSet::new();

        // Collect from symbol spans.
        for span in &symbol_map.spans {
            let name = match &span.kind {
                SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => name,
                _ => continue,
            };
            if name != var_name {
                continue;
            }
            let span_scope = symbol_map.find_variable_scope(name, span.start);
            if span_scope != scope_start {
                continue;
            }
            seen_offsets.insert(span.start);

            let kind = if symbol_map.var_def_kind_at(name, span.start).is_some() {
                DocumentHighlightKind::WRITE
            } else {
                DocumentHighlightKind::READ
            };

            highlights.push(DocumentHighlight {
                range: byte_range_to_lsp_range(content, span.start as usize, span.end as usize),
                kind: Some(kind),
            });
        }

        // Include var_def sites that may not have a matching Variable span
        // (e.g. parameters, foreach bindings).
        for def in &symbol_map.var_defs {
            if def.name == var_name
                && def.scope_start == scope_start
                && seen_offsets.insert(def.offset)
            {
                let end_offset = def.offset + 1 + def.name.len() as u32;
                highlights.push(DocumentHighlight {
                    range: byte_range_to_lsp_range(
                        content,
                        def.offset as usize,
                        end_offset as usize,
                    ),
                    kind: Some(DocumentHighlightKind::WRITE),
                });
            }
        }

        highlights.sort_by(cmp_highlight_range);
        highlights
    }

    /// Highlight all `$this` references within the same class body.
    fn highlight_this(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        cursor_offset: u32,
        uri: &str,
    ) -> Vec<DocumentHighlight> {
        let ctx_classes: Vec<std::sync::Arc<crate::types::ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let current_class = crate::class_lookup::find_class_at_offset(&ctx_classes, cursor_offset);
        let (class_start, class_end) = match current_class {
            Some(cc) => (cc.start_offset, cc.end_offset),
            None => (0, u32::MAX),
        };

        let mut highlights = Vec::new();

        for span in &symbol_map.spans {
            if span.start < class_start || span.start > class_end {
                continue;
            }
            if let SymbolKind::SelfStaticParent(SelfStaticParentKind::This) = &span.kind {
                highlights.push(DocumentHighlight {
                    range: byte_range_to_lsp_range(content, span.start as usize, span.end as usize),
                    kind: Some(DocumentHighlightKind::READ),
                });
            }
        }

        highlights.sort_by(cmp_highlight_range);
        highlights
    }

    /// Highlight all occurrences of a class/interface/trait/enum name
    /// (by FQN) in the file.
    fn highlight_class(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        target_fqn: &str,
        use_map: &HashMap<String, String>,
        namespace: &Option<String>,
    ) -> Vec<DocumentHighlight> {
        let mut highlights = Vec::new();

        for span in &symbol_map.spans {
            let fqn = match &span.kind {
                SymbolKind::ClassReference { name, is_fqn, .. } => {
                    if *is_fqn {
                        name.clone()
                    } else {
                        Self::resolve_to_fqn(name, use_map, namespace)
                    }
                }
                SymbolKind::ClassDeclaration { name } => build_fqn(name, namespace.as_deref()),
                _ => continue,
            };

            if fqn == target_fqn {
                highlights.push(DocumentHighlight {
                    range: byte_range_to_lsp_range(content, span.start as usize, span.end as usize),
                    kind: Some(DocumentHighlightKind::READ),
                });
            }
        }

        highlights.sort_by(cmp_highlight_range);
        highlights
    }

    /// Highlight all member accesses and declarations with the same name.
    ///
    /// This is a name-only match (no subject type resolution) which is
    /// acceptable for v1. It may produce false positives across unrelated
    /// classes in the same file, but that is a rare scenario.
    fn highlight_member_name(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        target_name: &str,
    ) -> Vec<DocumentHighlight> {
        let mut highlights = Vec::new();

        for span in &symbol_map.spans {
            match &span.kind {
                SymbolKind::MemberAccess { member_name, .. } if member_name == target_name => {
                    highlights.push(DocumentHighlight {
                        range: byte_range_to_lsp_range(
                            content,
                            span.start as usize,
                            span.end as usize,
                        ),
                        kind: Some(DocumentHighlightKind::READ),
                    });
                }
                SymbolKind::MemberDeclaration { name, .. } if name == target_name => {
                    highlights.push(DocumentHighlight {
                        range: byte_range_to_lsp_range(
                            content,
                            span.start as usize,
                            span.end as usize,
                        ),
                        kind: Some(DocumentHighlightKind::WRITE),
                    });
                }
                // Also match property declarations that appear as Variable spans.
                SymbolKind::Variable { name }
                    if name == target_name
                        && symbol_map
                            .var_def_kind_at(name, span.start)
                            .is_some_and(|k| *k == VarDefKind::Property) =>
                {
                    highlights.push(DocumentHighlight {
                        range: byte_range_to_lsp_range(
                            content,
                            span.start as usize,
                            span.end as usize,
                        ),
                        kind: Some(DocumentHighlightKind::WRITE),
                    });
                }
                _ => {}
            }
        }

        highlights.sort_by(cmp_highlight_range);
        highlights
    }

    /// Highlight all occurrences of a standalone function name.
    fn highlight_function(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        target_name: &str,
    ) -> Vec<DocumentHighlight> {
        let mut highlights = Vec::new();

        for span in &symbol_map.spans {
            if let SymbolKind::FunctionCall { name, .. } = &span.kind
                && name == target_name
            {
                highlights.push(DocumentHighlight {
                    range: byte_range_to_lsp_range(content, span.start as usize, span.end as usize),
                    kind: Some(DocumentHighlightKind::READ),
                });
            }
        }

        highlights.sort_by(cmp_highlight_range);
        highlights
    }

    /// Highlight all occurrences of a constant name.
    fn highlight_constant(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        target_name: &str,
    ) -> Vec<DocumentHighlight> {
        let mut highlights = Vec::new();

        for span in &symbol_map.spans {
            if let SymbolKind::ConstantReference { name } = &span.kind
                && name == target_name
            {
                highlights.push(DocumentHighlight {
                    range: byte_range_to_lsp_range(content, span.start as usize, span.end as usize),
                    kind: Some(DocumentHighlightKind::READ),
                });
            }
        }

        highlights.sort_by(cmp_highlight_range);
        highlights
    }

    /// Highlight all occurrences of `self`, `static`, or `parent` within
    /// the same class body.
    fn highlight_keyword(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        target_kind: SelfStaticParentKind,
        cursor_offset: u32,
        uri: &str,
    ) -> Vec<DocumentHighlight> {
        let ctx_classes: Vec<std::sync::Arc<crate::types::ClassInfo>> = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let current_class = crate::class_lookup::find_class_at_offset(&ctx_classes, cursor_offset);
        let (class_start, class_end) = match current_class {
            Some(cc) => (cc.start_offset, cc.end_offset),
            None => (0, u32::MAX),
        };

        let mut highlights = Vec::new();

        for span in &symbol_map.spans {
            if span.start < class_start || span.start > class_end {
                continue;
            }
            if let SymbolKind::SelfStaticParent(ssp_kind) = &span.kind
                && *ssp_kind == target_kind
            {
                highlights.push(DocumentHighlight {
                    range: byte_range_to_lsp_range(content, span.start as usize, span.end as usize),
                    kind: Some(DocumentHighlightKind::READ),
                });
            }
        }

        highlights.sort_by(cmp_highlight_range);
        highlights
    }
}

/// Compare two document highlights by position for stable ordering.
fn cmp_highlight_range(a: &DocumentHighlight, b: &DocumentHighlight) -> std::cmp::Ordering {
    a.range
        .start
        .line
        .cmp(&b.range.start.line)
        .then(a.range.start.character.cmp(&b.range.start.character))
}
