//! The class constants event subscribers key their maps by, as the
//! definition of the event name each holds.
//!
//! `KernelEvents::REQUEST` and `'kernel.request'` are one event in two
//! spellings once a subscriber keys its map by the constant, so the
//! constant's declaration is where the event name is defined.  Nothing
//! here is indexed: the constants are found from the subscriptions when
//! a definition or the references are asked for, and declared where PHP's
//! own go-to-definition of the key's constant lands, inherited ones
//! included.

use std::collections::HashMap;

use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::atom::Atom;
use crate::symbol_map::{EventConstantKey, StringKeyKind};

/// A class constant subscribers key their maps by.
pub(crate) struct EventConstant {
    pub name: Atom,
    /// Where it is declared.
    pub declaration: Location,
    /// The event name it holds.
    pub event: String,
    /// A use of it a subscription's key makes, in a file whose symbol map
    /// is always read: the file and an offset inside the constant's name.
    pub use_site: (String, u32),
}

/// The class constants that hold one event name, and the subscriptions
/// keyed by them.
#[derive(Default)]
pub(crate) struct EventConstants {
    pub constants: Vec<EventConstant>,
    /// The whole key of each subscription, `Events::NAME`.
    pub keys: Vec<Location>,
}

impl Backend {
    /// Whether a subscriber keys its map by a class constant named
    /// `constant`, of any class.
    pub(crate) fn names_event_constant(&self, constant: &str) -> bool {
        self.string_key_kind_enabled(StringKeyKind::SymfonyEvent)
            && self
                .event_constant_names()
                .contains(&crate::atom::atom(constant))
    }

    /// The class constants subscribers key their maps by whose value is
    /// the event name `name`, each once, and the subscriptions keyed by
    /// them.
    pub(crate) fn symfony_event_constants_named(&self, name: &str) -> EventConstants {
        let mut keys = Vec::new();
        let constants = self.event_constants(
            |_| true,
            |uri, key, value| {
                if value != name {
                    return false;
                }
                keys.push((uri.to_string(), key.start, key.end));
                true
            },
        );
        EventConstants {
            constants,
            keys: self.locations_of(keys),
        }
    }

    /// The class constant named `constant` declared at `declaration`, when
    /// a subscriber keys its map by it.  Any other constant is a plain one.
    pub(crate) fn symfony_event_constant_at(
        &self,
        declaration: &Location,
        constant: &str,
    ) -> Option<EventConstant> {
        self.event_constants(|key| key.constant == constant, |_, _, _| true)
            .into_iter()
            .find(|found| {
                found.declaration.uri == declaration.uri
                    && found.declaration.range.start == declaration.range.start
            })
    }

    /// The constants the keys `wanted` and `keep` let through name, each
    /// resolved once however many subscriptions name it.  `wanted` sees a
    /// key before its class is loaded, `keep` with the value it holds.
    fn event_constants(
        &self,
        wanted: impl Fn(&EventConstantKey) -> bool,
        mut keep: impl FnMut(&str, &EventConstantKey, &str) -> bool,
    ) -> Vec<EventConstant> {
        let mut named: HashMap<(Atom, Atom), (String, u32, String)> = HashMap::new();
        self.visit_event_constant_keys(wanted, |uri, key, value| {
            if keep(uri, key, value) {
                named
                    .entry((key.class, key.constant))
                    .or_insert_with(|| (uri.to_string(), key.end, value.to_string()));
            }
        });
        let mut named: Vec<_> = named
            .into_iter()
            .map(|((_, name), (uri, end, event))| (uri, end, event, name))
            .collect();
        named.sort_unstable_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        let mut constants: Vec<EventConstant> = Vec::new();
        for (uri, end, event, name) in named {
            let use_offset = end.saturating_sub(1);
            for declaration in self.event_constant_key_definition(&uri, use_offset) {
                if constants
                    .iter()
                    .all(|known| known.declaration != declaration)
                {
                    constants.push(EventConstant {
                        name,
                        declaration,
                        event: event.clone(),
                        use_site: (uri.clone(), use_offset),
                    });
                }
            }
        }
        constants
    }

    /// Where the constant a subscription's key names is declared: PHP's own
    /// definition of the constant's name, at `offset` in it.
    fn event_constant_key_definition(&self, uri: &str, offset: u32) -> Vec<Location> {
        let Some(content) = self.get_file_content_arc(uri) else {
            return Vec::new();
        };
        // Resolved for another file than the one a diagnostic pass may be
        // checking on this thread, so neither of its caches may answer.
        let _isolated = (
            crate::type_engine::variable::forward_walk::suspend_diagnostic_scope(),
            crate::type_engine::resolver::with_isolated_chain_cache(),
        );
        let position = crate::text_position::offset_to_position(&content, offset as usize);
        self.resolve_definition(uri, &content, position)
    }

    /// The locations of byte ranges in files, each file read once.
    fn locations_of(&self, mut ranges: Vec<(String, u32, u32)>) -> Vec<Location> {
        ranges.sort_unstable();
        let mut locations = Vec::with_capacity(ranges.len());
        for group in ranges.chunk_by(|a, b| a.0 == b.0) {
            let uri = &group[0].0;
            let (Some(parsed), Some(content)) =
                (crate::util::parse_url(uri), self.get_file_content_arc(uri))
            else {
                continue;
            };
            let lines = crate::text_position::LineIndex::new(&content);
            for (_, start, end) in group {
                locations.push(Location {
                    uri: parsed.clone(),
                    range: Range {
                        start: lines.position(*start as usize),
                        end: lines.position(*end as usize),
                    },
                });
            }
        }
        locations
    }
}
