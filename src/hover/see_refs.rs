//! `@see` reference resolution for hover.
//!
//! Resolves each raw `@see` string (a class name, `Class::method()`,
//! `Class::$prop`, or `Class::CONSTANT`) to a `file://` URI with a line
//! fragment so the hover popup can render it as a clickable link.  URLs
//! and unresolvable symbols are left unlinked.

use tower_lsp::lsp_types::Url;

use crate::Backend;

use super::formatting::ResolvedSeeRef;

impl Backend {
    /// Resolve `@see` references to file locations where possible.
    ///
    /// For each raw `@see` string, attempts to resolve symbol references
    /// (class names, `Class::member()`, `Class::$prop`) to a `file://`
    /// URI with a line fragment so that the hover popup renders them as
    /// clickable links.  URLs and unresolvable symbols get `None`.
    pub(crate) fn resolve_see_refs(
        &self,
        see_refs: &[String],
        uri: &str,
        content: &str,
    ) -> Vec<ResolvedSeeRef> {
        see_refs
            .iter()
            .map(|raw| {
                // Extract the first token (the symbol or URL).
                let target = raw
                    .split_once(|c: char| c.is_whitespace())
                    .map(|(t, _)| t.trim())
                    .unwrap_or(raw.as_str());

                // URLs don't need resolution.
                if target.starts_with("http://") || target.starts_with("https://") {
                    return ResolvedSeeRef {
                        raw: raw.clone(),
                        location_uri: None,
                    };
                }

                // Try to resolve as a class or class::member reference.
                let location_uri = self.resolve_see_target(target, uri, content);

                ResolvedSeeRef {
                    raw: raw.clone(),
                    location_uri,
                }
            })
            .collect()
    }

    /// Resolve a single `@see` target to a `file://` URI with line fragment.
    ///
    /// Handles:
    /// - `ClassName` → class keyword offset
    /// - `ClassName::method()` → method name offset
    /// - `ClassName::$property` → property name offset
    /// - `ClassName::CONSTANT` → constant name offset
    fn resolve_see_target(&self, target: &str, uri: &str, content: &str) -> Option<String> {
        // Check for Class::member syntax.
        if let Some(sep) = target.find("::") {
            let class_name = &target[..sep];
            let mut member_part = target[sep + 2..].to_string();
            // Strip trailing "()" from method references.
            if member_part.ends_with("()") {
                member_part.truncate(member_part.len() - 2);
            }
            // Strip leading "$" from property references.
            let member_name = member_part.strip_prefix('$').unwrap_or(&member_part);

            let cls = self.find_or_load_class(class_name)?;
            let (class_uri, class_content) =
                self.find_class_file_content(&cls.name, uri, content)?;

            // Find the member's name_offset.
            let offset = cls
                .get_method_ci(member_name)
                .map(|m| m.name_offset)
                .or_else(|| {
                    cls.properties
                        .iter()
                        .find(|p| p.name == member_name)
                        .map(|p| p.name_offset)
                })
                .or_else(|| {
                    cls.constants
                        .iter()
                        .find(|c| c.name == member_name)
                        .map(|c| c.name_offset)
                })
                .filter(|&off| off > 0)?;

            let pos = crate::text_position::offset_to_position(&class_content, offset as usize);
            let parsed_uri = Url::parse(&class_uri).ok()?;
            Some(format!("{}#L{}", parsed_uri, pos.line + 1))
        } else {
            // Plain class name.
            let cls = self.find_or_load_class(target)?;
            let (class_uri, class_content) =
                self.find_class_file_content(&cls.name, uri, content)?;

            if cls.keyword_offset == 0 {
                return None;
            }
            let pos = crate::text_position::offset_to_position(
                &class_content,
                cls.keyword_offset as usize,
            );
            let parsed_uri = Url::parse(&class_uri).ok()?;
            Some(format!("{}#L{}", parsed_uri, pos.line + 1))
        }
    }
}
