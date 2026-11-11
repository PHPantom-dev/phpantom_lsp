//! Find References on an event name, in either of its spellings.
//!
//! A class constant a subscriber keys its map by is the definition of the
//! event name it holds (see [`crate::symfony::event_constants`]), so the
//! references of the event string, of the constant's name, and of the
//! string the constant is initialized with are one list: the constant's
//! declaration and uses, the subscriptions, and the dispatcher calls.  It
//! is put together when asked for; the reference index holds only spans.

use super::*;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::virtual_members::laravel;

impl Backend {
    /// Add the rest of the event `sym` belongs to to its `locations`: the
    /// constant that holds an event string, or the event strings a
    /// constant holds.  Anything else is left as it is.
    pub(super) fn link_event_references(
        &self,
        sym: &SymbolSpan,
        uri: &str,
        content: &str,
        position: Position,
        include_declaration: bool,
        locations: &mut Vec<Location>,
    ) {
        if !self.string_key_kind_enabled(StringKeyKind::SymfonyEvent) {
            return;
        }
        let name = match &sym.kind {
            SymbolKind::StringKey {
                kind: StringKeyKind::SymfonyEvent,
                key,
                ..
            } => key.clone(),
            SymbolKind::MemberDeclaration {
                name,
                is_static: true,
                is_method: false,
            } if self.names_event_constant(name) => {
                let Some(declaration) = location_of(uri, content, sym.start, sym.end) else {
                    return;
                };
                let Some(constant) = self.symfony_event_constant_at(&declaration, name) else {
                    return;
                };
                constant.event
            }
            SymbolKind::MemberAccess {
                member_name,
                is_static: true,
                is_method_call: false,
                ..
            } if self.names_event_constant(member_name) => {
                let constant = self
                    .resolve_definition(uri, content, position)
                    .iter()
                    .find_map(|declaration| {
                        self.symfony_event_constant_at(declaration, member_name)
                    });
                let Some(constant) = constant else {
                    return;
                };
                constant.event
            }
            _ => return,
        };
        self.add_event_references(&name, include_declaration, locations);
    }

    /// The references of the event a class constant's initializer names,
    /// when the cursor is on the string a subscriber's constant holds.
    pub(super) fn event_constant_initializer_references(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        if !self.string_key_kind_enabled(StringKeyKind::SymfonyEvent) {
            return None;
        }
        let offset = position_to_offset(content, position);
        let (name, start, end) = constant_initialized_at(content, offset)?;
        if !self.names_event_constant(&name) {
            return None;
        }
        let declaration = location_of(uri, content, start, end)?;
        let constant = self.symfony_event_constant_at(&declaration, &name)?;
        let mut locations = self.constant_references_from(&constant.use_site, include_declaration);
        self.add_event_references(&constant.event, include_declaration, &mut locations);
        Some(locations)
    }

    /// Add every reference of the event `name` to `locations`, each once:
    /// the event strings, the subscriptions, and each constant that holds
    /// it with its uses.
    ///
    /// A subscription declares the event whatever its spelling, so with
    /// declarations left out the subscriptions keyed by a constant go too;
    /// with them in, such a subscription is listed as the use of the
    /// constant its key holds.
    fn add_event_references(
        &self,
        name: &str,
        include_declaration: bool,
        locations: &mut Vec<Location>,
    ) {
        let kind = StringKeyKind::SymfonyEvent;
        let reference_key = crate::reference_index::string_key_reference_key(kind, name);
        let snapshot = self.user_file_symbol_maps_for_reference_keys(&[reference_key]);
        let strings = laravel::find_laravel_string_key_references(
            self,
            &kind,
            name,
            "",
            &snapshot,
            include_declaration,
        );
        let found = self.symfony_event_constants_named(name);
        let mut seen: HashSet<Location> = locations.iter().cloned().collect();
        let uses = found
            .constants
            .iter()
            .flat_map(|constant| {
                self.constant_references_from(&constant.use_site, include_declaration)
            })
            .collect::<Vec<_>>();
        for location in strings.into_iter().chain(uses) {
            if seen.insert(location.clone()) {
                locations.push(location);
            }
        }
        // A constant's own references search the project's files, which a
        // constant a dependency declares is not among.
        if include_declaration {
            for constant in &found.constants {
                let declaration = &constant.declaration;
                if !locations.iter().any(|location| {
                    location.uri == declaration.uri
                        && location.range.start == declaration.range.start
                }) {
                    locations.push(name_location(declaration, constant.name.as_str()));
                }
            }
        }
        if found.keys.is_empty() {
            return;
        }
        let keys: HashSet<&Location> = found.keys.iter().collect();
        let dropped: Vec<bool> = locations
            .iter()
            .map(|location| {
                if include_declaration {
                    keys.contains(location)
                        && locations
                            .iter()
                            .any(|inner| inner != location && within(inner, location))
                } else {
                    found.keys.iter().any(|key| within(location, key))
                }
            })
            .collect();
        let mut index = 0;
        locations.retain(|_| {
            let keep = !dropped[index];
            index += 1;
            keep
        });
    }

    /// The references of a class constant, searched from a use of it at
    /// `offset` in the file `uri`, a file whose symbol map is read even
    /// when the constant's own file is not.
    fn constant_references_from(
        &self,
        (uri, offset): &(String, u32),
        include_declaration: bool,
    ) -> Vec<Location> {
        let Some(content) = self.get_file_content_arc(uri) else {
            return Vec::new();
        };
        let position = offset_to_position(&content, *offset as usize);
        let Some(symbol) = self.lookup_symbol_at_position(uri, &content, position) else {
            return Vec::new();
        };
        self.dispatch_symbol_references(
            &symbol,
            uri,
            &content,
            include_declaration,
            ReferenceSearchMode::References,
        )
    }
}

/// The location of the byte range `start..end` of `content`, the text of
/// the file `uri`.
fn location_of(uri: &str, content: &str, start: u32, end: u32) -> Option<Location> {
    Some(Location {
        uri: crate::util::parse_url(uri)?,
        range: Range {
            start: offset_to_position(content, start as usize),
            end: offset_to_position(content, end as usize),
        },
    })
}

/// `declaration`, covering the constant's name `name` when it is the point
/// a definition gives.
fn name_location(declaration: &Location, name: &str) -> Location {
    let mut location = declaration.clone();
    if location.range.end == location.range.start {
        location.range.end.character += name.encode_utf16().count() as u32;
    }
    location
}

/// Whether `inner` lies within `outer`, in the same file.
fn within(inner: &Location, outer: &Location) -> bool {
    inner.uri == outer.uri
        && outer.range.start <= inner.range.start
        && inner.range.end <= outer.range.end
}

/// The class constant whose initializer is the string literal `offset` is
/// in: its name and where the name starts and ends.
fn constant_initialized_at(content: &str, offset: u32) -> Option<(String, u32, u32)> {
    crate::parser::with_parsed_program(content, "event_constant_initializer", |program, _| {
        let mut found = None;
        find_constant_initialized_at(Node::Program(program), offset, &mut found);
        found
    })
}

fn find_constant_initialized_at(
    node: Node<'_, '_>,
    offset: u32,
    found: &mut Option<(String, u32, u32)>,
) {
    node.visit_children(|child| {
        if found.is_some() {
            return;
        }
        if let Node::ClassLikeConstantItem(item) = child {
            if let Expression::Literal(Literal::String(literal)) = item.value {
                let span = literal.span();
                if span.start.offset <= offset && offset < span.end.offset {
                    let name = &item.name;
                    *found = Some((
                        String::from_utf8_lossy(name.value).into_owned(),
                        name.span.start.offset,
                        name.span.end.offset,
                    ));
                }
            }
            return;
        }
        let span = child.span();
        if span.start.offset <= offset && offset <= span.end.offset {
            find_constant_initialized_at(child, offset, found);
        }
    });
}
