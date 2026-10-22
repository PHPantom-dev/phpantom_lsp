//! `@see` reference resolution for hover.
//!
//! Resolves each raw `@see` string (a class name, `Class::method()`,
//! `Class::$prop`, or `Class::CONSTANT`) to a `file://` URI with a line
//! fragment so the hover popup can render it as a clickable link.  URLs
//! and unresolvable symbols are left unlinked.

use crate::Backend;
use crate::types::SeeRef;

use super::formatting::ResolvedSeeRef;

impl Backend {
    /// Resolve `@see` references to file locations where possible.
    ///
    /// For each `@see` entry, attempts to resolve symbol references
    /// (class names, `Class::member()`, `Class::$prop`) to a `file://`
    /// URI with a line fragment so that the hover popup renders them as
    /// clickable links.  URLs and unresolvable symbols get `None`.
    pub(crate) fn resolve_see_refs(
        &self,
        see_refs: &[SeeRef],
        uri: &str,
        content: &str,
    ) -> Vec<ResolvedSeeRef> {
        see_refs
            .iter()
            .map(|see| ResolvedSeeRef {
                raw: see.text.clone(),
                location_uri: self.resolve_see_target(see, uri, content),
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
    fn resolve_see_target(&self, see: &SeeRef, uri: &str, content: &str) -> Option<String> {
        let cls = self.find_or_load_class(see.class.as_deref()?)?;
        let (class_uri, class_content) = self.find_class_file_content(&cls.fqn(), uri, content)?;

        let offset = match see.target().split_once("::") {
            Some((_, member)) => {
                let member = member.strip_suffix("()").unwrap_or(member);
                let member_name = member.strip_prefix('$').unwrap_or(member);
                cls.get_method_ci(member_name)
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
                    })?
            }
            None => cls.keyword_offset,
        };
        if offset == 0 {
            return None;
        }

        let pos = crate::text_position::offset_to_position(&class_content, offset as usize);
        let parsed_uri = crate::util::parse_url(&class_uri)?;
        Some(format!("{}#L{}", parsed_uri, pos.line + 1))
    }
}
