//! Go-to-definition on a Blade component tag.
//!
//! `<x-alert>` and `<livewire:counter>` are HTML, and the virtual PHP the
//! rest of go-to-definition works from emits a tag's call where the tag
//! *closes* rather than where its name is written, so the tag name has no
//! position in it to resolve from. This reads the raw Blade buffer
//! instead, the same way component completion does.

use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::Backend;
use crate::blade::component_tags::{TagCursor, TagKind, tag_context_at};
use crate::text_position::position_to_byte_offset;

impl Backend {
    /// The declaration the component tag at `position` names: the class
    /// backing it, or, for an anonymous component, the template Laravel
    /// renders in its place.
    pub(crate) fn blade_component_tag_definition(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<Location> {
        let content = self.get_file_content(uri)?;
        let offset = position_to_byte_offset(&content, position);
        let ctx = tag_context_at(&content, offset)?;
        if ctx.cursor != TagCursor::Name || ctx.name.is_empty() {
            return None;
        }
        match ctx.kind {
            TagKind::Livewire => self
                .livewire_component_fqn(&ctx.name)
                .and_then(|fqn| self.class_declaration_location(&fqn)),
            TagKind::Blade => self
                .blade_component_fqn(&ctx.name)
                .and_then(|fqn| self.class_declaration_location(&fqn))
                .or_else(|| self.anonymous_component_template(&ctx.name)),
        }
    }

    /// The template a tag no class answers for renders, at its first
    /// character: an anonymous component has no declaration to point at,
    /// so the file itself is the definition.
    fn anonymous_component_template(&self, tag: &str) -> Option<Location> {
        let view = self.anonymous_component_view(tag, &self.anonymous_component_namespaces())?;
        let discovery = self.blade_discovery();
        let path = discovery.views.get(&view)?;
        Some(Location {
            uri: Url::from_file_path(path).ok()?,
            range: Range::default(),
        })
    }
}
