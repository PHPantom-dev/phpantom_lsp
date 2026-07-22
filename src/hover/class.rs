//! Class/interface/trait/enum hover.
//!
//! Renders the declaration signature (with `extends`/`implements`),
//! docblock description, `@see` links, template parameters, and a body
//! preview: enum cases for enums and public member signatures for
//! traits.

use tower_lsp::lsp_types::Hover;

use crate::Backend;
use crate::docblock::extract_template_params_full;
use crate::types::*;
use crate::util::short_name;

use super::formatting::*;

impl Backend {
    /// Produce hover information for a class reference.
    pub(super) fn hover_class_reference(
        &self,
        name: &str,
        uri: &str,
        content: &str,
        class_loader: &dyn Fn(&str) -> Option<std::sync::Arc<ClassInfo>>,
        cursor_offset: u32,
    ) -> Option<Hover> {
        let class_info = class_loader(name);

        if let Some(cls) = class_info {
            Some(self.hover_for_class_info(&cls, uri, content))
        } else {
            // Check whether this is a template parameter in scope.
            if let Some(tpl) = self.find_template_def_for_hover(uri, name, cursor_offset) {
                return Some(tpl);
            }
            None
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

        // For enums, show cases inside the code block.
        // For traits, show public method signatures inside the code block.
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
    let cases: Vec<&ConstantInfo> = cls.constants.iter().filter(|c| c.is_enum_case).collect();

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

    // Public constants.
    for constant in &cls.constants {
        if constant.visibility != Visibility::Public {
            continue;
        }
        let type_hint = constant
            .type_hint
            .as_ref()
            .map(|t| format!(": {}", t))
            .unwrap_or_default();
        let value_suffix = constant
            .value
            .as_ref()
            .map(|v| format!(" = {}", v))
            .unwrap_or_default();
        member_lines.push(format!(
            "    const {}{}{};",
            constant.name, type_hint, value_suffix
        ));
    }

    // Public properties.
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

    // Public methods.
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
