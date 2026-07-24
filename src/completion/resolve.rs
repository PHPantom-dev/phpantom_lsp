//! Completion item resolution (`completionItem/resolve`).
//!
//! When the user highlights a completion item in the popup, the editor
//! sends a `completionItem/resolve` request to lazily fill in the
//! `documentation` field.  This avoids computing rich markdown for every
//! item up front.
//!
//! The identity of each item is encoded in [`CompletionItemData`] and
//! serialized into the `data` field of the `CompletionItem` during
//! initial completion (see [`builder`]).  The resolve handler
//! deserializes it, loads the relevant symbol, and delegates to the
//! existing hover methods to build the documentation.
//!
//! Supported kinds:
//!
//! - `"method"`, `"property"`, `"constant"` — class members, resolved
//!   via `class_name` (and `extra_class_names` for union types).
//! - `"function"` — standalone functions, looked up via
//!   `member_name` (which stores the FQN).
//! - `"class"` — class/interface/trait/enum references, looked up
//!   via `member_name` (which stores the FQN).
//! - `"global_constant"` — global `define()` / stub constants,
//!   looked up via `member_name`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::hover::{MemberKindForOrigin, find_declaring_class, hover_for_function};
use crate::types::*;

/// Identity data stored in `CompletionItem::data` for later resolution.
///
/// Serialized as JSON when the item is created and deserialized in the
/// resolve handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CompletionItemData {
    /// The owning class name (short name as stored in `ClassInfo::name`).
    ///
    /// For non-member kinds (`"function"`, `"class"`, `"global_constant"`)
    /// this is an empty string.
    #[serde(rename = "c")]
    pub class_name: String,
    /// The member name (method name, property name, or constant name).
    ///
    /// For non-member kinds this stores the lookup key:
    /// - `"function"`: the FQN or short name of the function.
    /// - `"class"`: the FQN of the class.
    /// - `"global_constant"`: the constant name.
    #[serde(rename = "m")]
    pub member_name: String,
    /// The item kind: `"method"`, `"property"`, `"constant"`,
    /// `"function"`, `"class"`, or `"global_constant"`.
    #[serde(rename = "k")]
    pub kind: String,
    /// The file URI where the completion was triggered.
    ///
    /// Used to build a `FileContext` for the class loader so that
    /// same-file classes are found during resolution.
    #[serde(rename = "u")]
    pub uri: String,
    /// Additional class names for union-type members.
    ///
    /// When a member appears on multiple union branches (e.g.
    /// `Lamp|Faucet` both have `turnOff()`), the primary class is in
    /// `class_name` and the remaining classes are stored here.  The
    /// resolve handler iterates all of them to build a combined hover
    /// that mirrors inline hover behaviour.
    #[serde(rename = "e", default, skip_serializing_if = "Vec::is_empty")]
    pub extra_class_names: Vec<String>,
}

/// Extract the markdown string from a `Hover` value.
///
/// The hover methods always produce `HoverContents::Markup`, so this
/// just unwraps the inner `MarkupContent::value`.
fn extract_hover_markdown(hover: Hover) -> Option<String> {
    match hover.contents {
        HoverContents::Markup(mc) => Some(mc.value),
        _ => None,
    }
}

/// Set the `documentation` field on a `CompletionItem` from a markdown
/// string, if non-empty.
fn set_documentation(item: &mut CompletionItem, markdown: String) {
    item.documentation = Some(Documentation::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value: markdown,
    }));
}

impl Backend {
    /// Handle `completionItem/resolve` by populating the `documentation`
    /// field with rich markdown.
    ///
    /// Reuses the existing hover methods ([`hover_for_method`],
    /// [`hover_for_property`], [`hover_for_constant`],
    /// [`hover_for_function`], [`hover_for_class_info`]) so that the
    /// documentation shown on selection is identical to what the user
    /// sees when hovering over a symbol.
    ///
    /// For union-type members (where `extra_class_names` is non-empty),
    /// builds a hover for each branch and joins them with a horizontal
    /// rule, deduplicating by declaring class.  This mirrors the inline
    /// hover behaviour for union member access.
    pub(crate) fn handle_completion_resolve(&self, mut item: CompletionItem) -> CompletionItem {
        let Some(ref data_value) = item.data else {
            return item;
        };

        let Ok(data) = serde_json::from_value::<CompletionItemData>(data_value.clone()) else {
            return item;
        };

        let ctx = self.file_context(&data.uri);
        let content = self.get_file_content(&data.uri).unwrap_or_default();

        match data.kind.as_str() {
            // ── Class member kinds ──────────────────────────────────
            "method" | "property" | "constant" => {
                self.resolve_member(&mut item, &data, &ctx, &content);
            }

            // ── Standalone function ─────────────────────────────────
            "function" => {
                self.resolve_function(&mut item, &data, &ctx, &content);
            }

            // ── Class / interface / trait / enum reference ───────────
            "class" => {
                self.resolve_class(&mut item, &data, &ctx, &content);
            }

            // ── Global constant (define / stub) ─────────────────────
            "global_constant" => {
                self.resolve_global_constant(&mut item, &data);
            }

            _ => {}
        }

        item
    }

    /// Resolve a class member (method, property, or constant).
    fn resolve_member(
        &self,
        item: &mut CompletionItem,
        data: &CompletionItemData,
        ctx: &FileContext,
        content: &str,
    ) {
        let class_loader = self.class_loader(ctx);

        // Collect all class names (primary + extras for union members).
        let mut all_class_names = vec![data.class_name.clone()];
        all_class_names.extend(data.extra_class_names.iter().cloned());

        let member_kind = match data.kind.as_str() {
            "method" => MemberKindForOrigin::Method,
            "property" => MemberKindForOrigin::Property,
            "constant" => MemberKindForOrigin::Constant,
            _ => return,
        };

        // Build hover markdown for each class, deduplicating by
        // declaring class so that a member inherited from a common
        // ancestor is shown only once.
        let mut hover_markdowns: Vec<String> = Vec::new();
        let mut seen_declaring: Vec<String> = Vec::new();

        for class_name in &all_class_names {
            let class_info: Option<Arc<ClassInfo>> = class_loader(class_name)
                .or_else(|| ctx.classes.iter().find(|c| c.name == *class_name).cloned());

            let Some(class_info) = class_info else {
                continue;
            };

            let merged = crate::virtual_members::resolve_class_fully_cached(
                &class_info,
                &class_loader,
                &self.resolved_class_cache,
            );

            let hover = match data.kind.as_str() {
                "method" => {
                    let method = merged.get_method(&data.member_name);
                    method.map(|m| {
                        let declaring = find_declaring_class(
                            &merged,
                            &data.member_name,
                            &member_kind,
                            &class_loader,
                        );
                        (
                            declaring.name.to_string(),
                            self.hover_for_method(m, &declaring, &class_loader, &data.uri, content),
                        )
                    })
                }
                "property" => {
                    let property = merged
                        .properties
                        .iter()
                        .find(|p| p.name == data.member_name);
                    property.map(|p| {
                        let declaring = find_declaring_class(
                            &merged,
                            &data.member_name,
                            &member_kind,
                            &class_loader,
                        );
                        (
                            declaring.name.to_string(),
                            self.hover_for_property(p, &declaring, &class_loader),
                        )
                    })
                }
                "constant" => {
                    let constant = merged.constants.iter().find(|c| c.name == data.member_name);
                    constant.map(|c| {
                        let declaring = find_declaring_class(
                            &merged,
                            &data.member_name,
                            &member_kind,
                            &class_loader,
                        );
                        (
                            declaring.name.to_string(),
                            self.hover_for_constant(c, &declaring, &class_loader),
                        )
                    })
                }
                _ => None,
            };

            if let Some((declaring_name, h)) = hover {
                if seen_declaring.contains(&declaring_name) {
                    continue;
                }
                seen_declaring.push(declaring_name);
                if let Some(md) = extract_hover_markdown(h) {
                    hover_markdowns.push(md);
                }
            }
        }

        if !hover_markdowns.is_empty() {
            set_documentation(item, hover_markdowns.join("\n\n---\n\n"));
        }
    }

    /// Resolve a standalone function.
    fn resolve_function(
        &self,
        item: &mut CompletionItem,
        data: &CompletionItemData,
        ctx: &FileContext,
        content: &str,
    ) {
        let function_loader = self.function_loader(ctx);
        if let Some(func) = function_loader(&data.member_name, 0) {
            let resolved_see = self.resolve_see_refs(&func.see_refs, &data.uri, content);
            let provenance = self.provenance_line_for_function(&data.member_name);
            let hover = hover_for_function(&func, Some(&resolved_see), provenance, false);
            if let Some(md) = extract_hover_markdown(hover) {
                set_documentation(item, md);
            }
        }
    }

    /// Resolve a class / interface / trait / enum reference.
    fn resolve_class(
        &self,
        item: &mut CompletionItem,
        data: &CompletionItemData,
        ctx: &FileContext,
        content: &str,
    ) {
        let class_loader = self.class_loader(ctx);
        if let Some(cls) = class_loader(&data.member_name) {
            let hover = self.hover_for_class_info(&cls, &data.uri, content);
            if let Some(md) = extract_hover_markdown(hover) {
                set_documentation(item, md);
            }
        }
    }

    /// Resolve a global constant (`define()` or stub constant).
    fn resolve_global_constant(&self, item: &mut CompletionItem, data: &CompletionItemData) {
        let name = &data.member_name;

        let lookup = self.lookup_global_constant(name);

        let md = match &lookup {
            Some(Some(val)) => format!("```php\n<?php\nconst {} = {};\n```", name, val),
            Some(None) => format!("```php\n<?php\nconst {};\n```", name),
            None => return,
        };

        // Fill in the inline detail (value hint next to the label).
        if let Some(Some(val)) = &lookup {
            item.detail = Some(val.clone());
        }

        set_documentation(item, md);
    }
}
