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

use crate::types::FileContext;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::references::MemberRefKind;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind, SymbolMap, SymbolSpan, VarDefKind};
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
        let Some(span) = self.lookup_symbol_at_position(uri, content, position) else {
            return self.framework_highlights(uri, content, position);
        };

        let symbol_map = self.symbol_map_for(uri)?;
        let symbol_map = &*symbol_map;

        let highlights = match &span.kind {
            SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => {
                // Check if this is actually a property declaration — if
                // so, highlight member accesses instead of local vars.
                // Constructor-promoted properties are tagged
                // VarDefKind::Parameter (the token is also a real
                // parameter) but still declare a class property.
                let is_property = matches!(
                    symbol_map.var_def_kind_at(name, span.start),
                    Some(VarDefKind::Property)
                ) || self.is_promoted_property_param(uri, span.start);
                if is_property {
                    self.highlight_member_name(uri, symbol_map, content, name, MemberRefKind::Value)
                } else {
                    self.highlight_variable(symbol_map, content, name, span.start)
                }
            }
            kind @ (SymbolKind::ClassReference { .. } | SymbolKind::ClassDeclaration { .. }) => {
                let ctx = self.file_context(uri);
                let fqn = class_span_fqn(kind, span.start, &ctx)?;
                self.highlight_class(symbol_map, content, &fqn, &ctx)
            }
            kind @ SymbolKind::MemberAccess { member_name, .. } => self.highlight_member_name(
                uri,
                symbol_map,
                content,
                member_name,
                MemberRefKind::of_access(kind).unwrap_or(MemberRefKind::Either),
            ),
            SymbolKind::MemberDeclaration {
                name, is_method, ..
            } => self.highlight_member_name(
                uri,
                symbol_map,
                content,
                name,
                MemberRefKind::of_method(*is_method),
            ),
            SymbolKind::FunctionCall { name, .. } => {
                self.highlight_function(symbol_map, content, name)
            }
            SymbolKind::ConstantReference { name, .. } => {
                self.highlight_constant(symbol_map, content, name)
            }
            SymbolKind::SelfStaticParent(ssp_kind) => {
                self.highlight_keyword(symbol_map, content, *ssp_kind, span.start, uri)
            }
            SymbolKind::NamespaceDeclaration { .. }
            | SymbolKind::StringKey { .. }
            | SymbolKind::LaravelMacroString { .. }
            | SymbolKind::CommandOwnParam { .. }
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
        let mut seen_offsets = std::collections::HashSet::new();

        let mut highlights = collect_highlights(symbol_map, content, |span| {
            let name = match &span.kind {
                SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => name,
                _ => return None,
            };
            if name != var_name || symbol_map.find_variable_scope(name, span.start) != scope_start {
                return None;
            }
            seen_offsets.insert(span.start);
            Some(if symbol_map.var_def_kind_at(name, span.start).is_some() {
                DocumentHighlightKind::Write
            } else {
                DocumentHighlightKind::Read
            })
        });

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
                    kind: Some(DocumentHighlightKind::Write),
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
        ctx: &FileContext,
    ) -> Vec<DocumentHighlight> {
        collect_highlights(symbol_map, content, |span| {
            (class_span_fqn(&span.kind, span.start, ctx)? == target_fqn)
                .then_some(DocumentHighlightKind::Read)
        })
    }

    /// Highlight all member accesses and declarations with the same name
    /// that can be a member of `target_kind`, so a property does not light
    /// up the method of its name.
    ///
    /// This is a name-only match (no subject type resolution) which is
    /// acceptable for v1. It may produce false positives across unrelated
    /// classes in the same file, but that is a rare scenario.
    fn highlight_member_name(
        &self,
        uri: &str,
        symbol_map: &SymbolMap,
        content: &str,
        target_name: &str,
        target_kind: MemberRefKind,
    ) -> Vec<DocumentHighlight> {
        // A constructor-promoted property is declared by a parameter, so its
        // Variable span is tagged as a parameter rather than a property; the
        // class's property offsets are what pick it out.
        let property_offsets: Vec<u32> = if target_kind.admits(MemberRefKind::Value) {
            self.shared_classes_for_uri(uri)
                .iter()
                .flat_map(|classes| classes.iter())
                .flat_map(|class| class.properties.iter())
                .filter(|prop| prop.name_offset != 0 && prop.name == target_name)
                .map(|prop| prop.name_offset)
                .collect()
        } else {
            Vec::new()
        };
        collect_highlights(symbol_map, content, |span| match &span.kind {
            kind @ SymbolKind::MemberAccess { member_name, .. }
                if member_name == target_name && target_kind.admits_access(kind) =>
            {
                Some(DocumentHighlightKind::Read)
            }
            SymbolKind::MemberDeclaration {
                name, is_method, ..
            } if name == target_name
                && target_kind.admits(MemberRefKind::of_method(*is_method)) =>
            {
                Some(DocumentHighlightKind::Write)
            }
            // Also match property declarations that appear as Variable spans.
            SymbolKind::Variable { name }
                if name == target_name
                    && target_kind.admits(MemberRefKind::Value)
                    && (property_offsets.contains(&span.start)
                        || symbol_map
                            .var_def_kind_at(name, span.start)
                            .is_some_and(|k| *k == VarDefKind::Property)) =>
            {
                Some(DocumentHighlightKind::Write)
            }
            _ => None,
        })
    }

    /// Highlight all occurrences of a standalone function name.
    fn highlight_function(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        target_name: &str,
    ) -> Vec<DocumentHighlight> {
        collect_highlights(symbol_map, content, |span| {
            matches!(&span.kind, SymbolKind::FunctionCall { name, .. } if name == target_name)
                .then_some(DocumentHighlightKind::Read)
        })
    }

    /// Highlight all occurrences of a constant name.
    fn highlight_constant(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        target_name: &str,
    ) -> Vec<DocumentHighlight> {
        collect_highlights(symbol_map, content, |span| {
            matches!(&span.kind, SymbolKind::ConstantReference { name, .. } if name == target_name)
                .then_some(DocumentHighlightKind::Read)
        })
    }

    /// Highlight all occurrences of `self`, `static`, `parent`, or `$this`
    /// within the same class body.
    fn highlight_keyword(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        target_kind: SelfStaticParentKind,
        cursor_offset: u32,
        uri: &str,
    ) -> Vec<DocumentHighlight> {
        let (class_start, class_end) = self
            .symbols
            .uri_classes_index
            .read()
            .get(uri)
            .and_then(|classes| crate::class_lookup::find_class_at_offset(classes, cursor_offset))
            .map_or((0, u32::MAX), |class| {
                (class.start_offset, class.end_offset)
            });

        collect_highlights(symbol_map, content, |span| {
            ((class_start..=class_end).contains(&span.start)
                && matches!(span.kind, SymbolKind::SelfStaticParent(kind) if kind == target_kind))
            .then_some(DocumentHighlightKind::Read)
        })
    }
}

/// Highlight every span of `symbol_map` that `kind_of` gives a highlight
/// kind, in position order.
fn collect_highlights(
    symbol_map: &SymbolMap,
    content: &str,
    mut kind_of: impl FnMut(&SymbolSpan) -> Option<DocumentHighlightKind>,
) -> Vec<DocumentHighlight> {
    symbol_map
        .spans
        .iter()
        .filter_map(|span| {
            Some(DocumentHighlight {
                range: byte_range_to_lsp_range(content, span.start as usize, span.end as usize),
                kind: Some(kind_of(span)?),
            })
        })
        .collect()
}

/// The FQN of the class a class reference or declaration span names, with
/// a reference resolved by the rule for the place it is written.
fn class_span_fqn(kind: &SymbolKind, offset: u32, ctx: &FileContext) -> Option<String> {
    match kind {
        SymbolKind::ClassReference { name, is_fqn, .. } => Some(if *is_fqn {
            name.to_string()
        } else {
            ctx.resolve_name_at(name, offset)
        }),
        SymbolKind::ClassDeclaration { name } => {
            Some(build_fqn(name, ctx.namespace_at(offset).as_deref()))
        }
        _ => None,
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
