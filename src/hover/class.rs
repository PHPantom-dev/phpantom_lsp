//! Class/interface/trait/enum hover.
//!
//! Renders the declaration signature (with `extends`/`implements`),
//! docblock description, `@see` links, template parameters, and a body
//! preview: enum cases for enums and public member signatures for
//! traits.

use std::sync::Arc;

use tower_lsp::lsp_types::Hover;

use crate::Backend;
use crate::docblock::extract_template_params_full;
use crate::symbol_map::{ClassRefContext, SelfStaticParentKind};
use crate::types::*;
use crate::util::short_name;

use super::HoverSite;
use super::formatting::*;

impl Backend {
    /// Hover for a class name the symbol map found; on `new ClassName`,
    /// the constructor's.
    pub(super) fn hover_class_symbol(
        &self,
        site: &HoverSite,
        name: &str,
        is_fqn: bool,
        context: ClassRefContext,
    ) -> Option<Hover> {
        let HoverSite {
            uri,
            content,
            offset: cursor_offset,
            loaders,
            ..
        } = *site;
        let class_loader = loaders.class_loader_at(cursor_offset);
        let resolved_class_loader = loaders.resolved_class_loader();
        // A fully qualified reference (`\stdClass`, stored without
        // its backslash) must not go through the file's imports
        // and namespace again.
        let class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>> = if is_fqn {
            &resolved_class_loader
        } else {
            &class_loader
        };
        // On `new ClassName`, show the __construct method hover
        // instead of the class hover.
        if context == crate::symbol_map::ClassRefContext::New
            && let Some(cls) = class_loader(name)
        {
            let merged = crate::virtual_members::resolve_class_fully_cached(
                &cls,
                &resolved_class_loader,
                &self.resolved_class_cache,
            );
            if let Some(constructor) = merged.get_method_ci("__construct") {
                return Some(self.hover_for_method(
                    constructor,
                    &merged,
                    &resolved_class_loader,
                    uri,
                    content,
                ));
            }
        }

        self.hover_class_reference(name, uri, content, class_loader, cursor_offset)
    }

    /// Hover for `self`, `static`, `parent` or `$this`: the class the
    /// keyword stands for at the cursor.
    pub(super) fn hover_self_static_parent(
        &self,
        site: &HoverSite,
        ssp_kind: SelfStaticParentKind,
    ) -> Option<Hover> {
        let HoverSite {
            uri,
            content,
            offset: cursor_offset,
            loaders,
            ..
        } = *site;
        let current_class = site.current_class();
        let resolved_class_loader = loaders.resolved_class_loader();
        let is_this = ssp_kind == SelfStaticParentKind::This;

        let resolved = match ssp_kind {
            SelfStaticParentKind::Self_ | SelfStaticParentKind::Static => current_class.cloned(),
            SelfStaticParentKind::This => self
                .resolve_closure_this_override(uri, content, cursor_offset)
                .map(Arc::unwrap_or_clone)
                .or_else(|| current_class.cloned()),
            SelfStaticParentKind::Parent => current_class
                .and_then(|cc| cc.parent_class.as_ref())
                .and_then(|parent_name| {
                    resolved_class_loader(parent_name).map(Arc::unwrap_or_clone)
                }),
        };
        if let Some(cls) = resolved {
            let mut lines = Vec::new();

            if let Some(desc) = extract_docblock_description(cls.class_docblock.as_deref()) {
                lines.push(desc);
            }

            if let Some(ref msg) = cls.deprecation_message {
                lines.push(format_deprecation_line(msg));
            }

            let ns_line = namespace_line(cls.file_namespace.as_deref());
            if is_this {
                lines.push(format!(
                    "```php\n<?php\n{}$this = {}\n```",
                    ns_line, cls.name
                ));
            } else {
                let keyword_str = match ssp_kind {
                    SelfStaticParentKind::Self_ => "self",
                    SelfStaticParentKind::Static => "static",
                    SelfStaticParentKind::Parent => "parent",
                    SelfStaticParentKind::This => unreachable!(),
                };
                lines.push(format!(
                    "```php\n<?php\n{}{} = {}\n```",
                    ns_line, keyword_str, cls.name
                ));
            }

            Some(make_hover(lines.join("\n\n")))
        } else {
            None
        }
    }

    /// Produce hover information for a class reference.
    pub(super) fn hover_class_reference(
        &self,
        name: &str,
        uri: &str,
        content: &str,
        class_loader: &dyn Fn(&str) -> Option<std::sync::Arc<ClassInfo>>,
        cursor_offset: u32,
    ) -> Option<Hover> {
        // In a docblock a template parameter in scope shadows a class of
        // the same name, so `@param T $x` under `@template T` names the
        // template even when a class `T` exists.  In code it is the class.
        if crate::completion::source::comment_position::is_offset_inside_docblock(
            content,
            cursor_offset as usize,
        ) && let Some(tpl) = self.find_template_def_for_hover(uri, name, cursor_offset)
        {
            return Some(tpl);
        }

        if let Some(cls) = class_loader(name) {
            Some(self.hover_for_class_info(&cls, uri, content))
        } else {
            // Check whether this is a template parameter in scope.
            self.find_template_def_for_hover(uri, name, cursor_offset)
        }
    }

    /// Build hover content for a class/interface/trait/enum.
    pub(crate) fn hover_for_class_info(&self, cls: &ClassInfo, uri: &str, content: &str) -> Hover {
        let kind_str = match cls.kind {
            ClassLikeKind::Class => {
                if cls.is_abstract {
                    "abstract class"
                } else if cls.is_final {
                    "final class"
                } else {
                    "class"
                }
            }
            ClassLikeKind::Interface => "interface",
            ClassLikeKind::Trait => "trait",
            ClassLikeKind::Enum => "enum",
        };

        let mut extends_implements = String::new();

        // For interfaces, `parent_class` is the first element of
        // `interfaces` (both come from the same `extends` clause),
        // so skip it to avoid duplicating the name.
        if cls.kind != ClassLikeKind::Interface
            && let Some(ref parent) = cls.parent_class
        {
            extends_implements.push_str(&format!(" extends {}", short_name(parent)));
        }

        if !cls.interfaces.is_empty() {
            let keyword = if cls.kind == ClassLikeKind::Interface {
                "extends"
            } else {
                "implements"
            };
            let short_ifaces: Vec<&str> = cls.interfaces.iter().map(|i| short_name(i)).collect();
            extends_implements.push_str(&format!(" {} {}", keyword, short_ifaces.join(", ")));
        }

        let signature = format!("{} {}{}", kind_str, cls.name, extends_implements);
        let ns_line = namespace_line(cls.file_namespace.as_deref());

        let mut lines = Vec::new();

        if let Some(desc) = extract_docblock_description(cls.class_docblock.as_deref()) {
            lines.push(desc);
        }

        if let Some(ref msg) = cls.deprecation_message {
            lines.push(format_deprecation_line(msg));
        }

        for url in &cls.links {
            lines.push(format!("[{}]({})", url, url));
        }

        let resolved_see = self.resolve_see_refs(&cls.see_refs, uri, content);
        format_see_refs(&resolved_see, &cls.links, &mut lines);

        // Show template parameters with variance and bounds.
        if let Some(ref docblock) = cls.class_docblock {
            let tpl_entries: Vec<String> = extract_template_params_full(docblock)
                .into_iter()
                .map(|(name, bound, variance, default)| {
                    let bound_display = bound
                        .map(|b| format!(" of `{}`", b.shorten()))
                        .unwrap_or_default();
                    let default_display =
                        default.map(|d| format!(" = `{}`", d)).unwrap_or_default();
                    format!(
                        "**{}** `{}`{}{}",
                        variance.tag_name(),
                        name,
                        bound_display,
                        default_display
                    )
                })
                .collect();
            if !tpl_entries.is_empty() {
                lines.push(tpl_entries.join("  \n"));
            }
        }

        let body_lines = if cls.kind == ClassLikeKind::Enum {
            build_enum_case_body(cls)
        } else if cls.kind == ClassLikeKind::Trait {
            build_trait_summary_body(cls)
        } else {
            String::new()
        };

        if body_lines.is_empty() {
            lines.push(format!("```php\n<?php\n{}{}\n```", ns_line, signature));
        } else {
            lines.push(format!(
                "```php\n<?php\n{}{} {{\n{}}}\n```",
                ns_line, signature, body_lines
            ));
        }

        if let Some(prov) = self.provenance_line_for_class(&cls.fqn()) {
            lines.push(prov);
        }

        make_hover(lines.join("\n\n"))
    }
}

/// Maximum number of enum cases or trait methods to show before
/// truncating with a "and N more…" comment.
const MAX_BODY_ITEMS: usize = 30;

/// Build the body lines for an enum hover showing its cases.
///
/// Only enum cases are shown (not regular class constants).
/// Each case is rendered as `    case Name = 'value';` or `    case Name;`.
/// If there are more than [`MAX_BODY_ITEMS`] cases, the list is truncated
/// with a `// and N more…` comment.
fn build_enum_case_body(cls: &ClassInfo) -> String {
    let cases: Vec<&ConstantInfo> = cls
        .constants
        .iter()
        .filter(|c| c.is_enum_case)
        .map(|c| c.as_ref())
        .collect();

    if cases.is_empty() {
        return String::new();
    }

    let mut body = String::new();
    let shown = cases.len().min(MAX_BODY_ITEMS);

    for case in &cases[..shown] {
        if let Some(ref val) = case.enum_value {
            body.push_str(&format!("    case {} = {};\n", case.name, val));
        } else {
            body.push_str(&format!("    case {};\n", case.name));
        }
    }

    if cases.len() > MAX_BODY_ITEMS {
        body.push_str(&format!(
            "    // and {} more…\n",
            cases.len() - MAX_BODY_ITEMS
        ));
    }

    body
}

/// Build the body lines for a trait hover showing public member signatures.
///
/// Shows public methods (one-line signatures without bodies), public
/// properties, and public constants. Uses native types only and short
/// (unqualified) class names for a scannable summary.
///
/// If there are more than [`MAX_BODY_ITEMS`] members, the list is
/// truncated with a `// and N more…` comment.
fn build_trait_summary_body(cls: &ClassInfo) -> String {
    let mut member_lines: Vec<String> = Vec::new();

    for constant in &cls.constants {
        if constant.visibility != Visibility::Public {
            continue;
        }
        member_lines.push(format!("    {}", constant_declaration(constant)));
    }

    for prop in &cls.properties {
        if prop.visibility != Visibility::Public {
            continue;
        }
        let static_kw = if prop.is_static { "static " } else { "" };
        let native_type = prop
            .native_type_hint
            .as_ref()
            .map(|t| format!("{} ", t))
            .unwrap_or_default();
        member_lines.push(format!(
            "    public {}{}${};",
            static_kw, native_type, prop.name
        ));
    }

    for method in &cls.methods {
        if method.visibility != Visibility::Public {
            continue;
        }
        let static_kw = if method.is_static { "static " } else { "" };
        let native_params = format_native_params(&method.parameters);
        let native_ret = method
            .native_return_type
            .as_ref()
            .map(|r| format!(": {}", r))
            .unwrap_or_default();
        member_lines.push(format!(
            "    public {}function {}({}){};",
            static_kw, method.name, native_params, native_ret
        ));
    }

    if member_lines.is_empty() {
        return String::new();
    }

    let shown = member_lines.len().min(MAX_BODY_ITEMS);
    let mut body: String = member_lines[..shown].join("\n");
    body.push('\n');

    if member_lines.len() > MAX_BODY_ITEMS {
        body.push_str(&format!(
            "    // and {} more…\n",
            member_lines.len() - MAX_BODY_ITEMS
        ));
    }

    body
}
