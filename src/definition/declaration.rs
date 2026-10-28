//! Go-to-declaration support (`textDocument/declaration`).
//!
//! Where go-to-definition lands on the concrete member a call reaches,
//! go-to-declaration lands on the contract that member fulfils: the
//! interface member, or the method of an abstract ancestor, it
//! implements.  A member with no such prototype, and every symbol that is
//! not a member, answers the same as go-to-definition.

use std::borrow::Cow;

use tower_lsp_server::gen_lsp_types::{Location, Position};

use crate::Backend;
use crate::class_lookup::find_class_at_offset;
use crate::symbol_map::SymbolKind;
use crate::text_position::position_to_offset;

impl Backend {
    /// Entry point for `textDocument/declaration`.
    pub(crate) fn resolve_declaration(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Vec<Location> {
        let prototypes = match self
            .lookup_symbol_at_position(uri, content, position)
            .map(|sym| (sym.kind, sym.start))
        {
            Some((SymbolKind::MemberDeclaration { name, .. }, start)) => {
                self.prototypes_of_member_at(uri, content, start, &name)
            }
            Some((SymbolKind::MemberAccess { member_name, .. }, _)) => {
                let definitions = self.resolve_definition(uri, content, position);
                let prototypes =
                    self.prototypes_of_definitions(uri, content, &definitions, &member_name);
                if prototypes.is_empty() {
                    return definitions;
                }
                prototypes
            }
            _ => Vec::new(),
        };
        if prototypes.is_empty() {
            return self.resolve_definition(uri, content, position);
        }
        prototypes
    }

    /// The prototypes of the member each of `definitions` lands on.
    fn prototypes_of_definitions(
        &self,
        uri: &str,
        content: &str,
        definitions: &[Location],
        member_name: &str,
    ) -> Vec<Location> {
        let mut prototypes = Vec::new();
        for definition in definitions {
            let def_uri = definition.uri.to_string();
            let def_content = if def_uri == uri {
                Cow::Borrowed(content)
            } else {
                let Some(def_content) = self.get_file_content(&def_uri) else {
                    continue;
                };
                Cow::Owned(def_content)
            };
            let offset = position_to_offset(&def_content, definition.range.start);
            for location in
                self.prototypes_of_member_at(&def_uri, &def_content, offset, member_name)
            {
                if !prototypes.contains(&location) {
                    prototypes.push(location);
                }
            }
        }
        prototypes
    }

    /// The prototypes of the member `member_name` declared in the class
    /// that encloses `offset` in `uri`.
    fn prototypes_of_member_at(
        &self,
        uri: &str,
        content: &str,
        offset: u32,
        member_name: &str,
    ) -> Vec<Location> {
        let ctx = self.file_context_at(uri, offset);
        let Some(class) = find_class_at_offset(&ctx.classes, offset) else {
            return Vec::new();
        };
        let class_loader = self.resolved_class_loader(&ctx);
        self.member_prototype_locations(uri, content, class, member_name, &class_loader)
    }
}
