//! Symfony Messenger: the buses the configuration declares, and the
//! handlers `#[AsMessageHandler]` registers for each message class.
//!
//! A bus is declared by its key under `framework.messenger.buses` (a
//! `<framework:bus name="…">` in XML) and read by the `default_bus` there.
//! Like the other Symfony names, each one is a
//! [`StringKey`](crate::symbol_map::SymbolKind::StringKey) span in its
//! file's resource symbol map.
//!
//! A handler is recorded in the symbol map of the file that declares it
//! (see [`MessengerHandlerSite`]), and those files are among the ones
//! [`super::SymfonyState`] tracks, so a message's handlers are read from
//! there rather than from a second index.

use super::NameSlot;
use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::Fqn;
use crate::symbol_map::{MessengerHandlerSite, StringKeyKind};

/// The bus names one YAML or XML configuration file declares and reads,
/// including the ones still being typed.
pub(crate) fn bus_slots(uri: &str, content: &str) -> Vec<NameSlot> {
    let mut slots = Vec::new();
    match super::config_format(uri) {
        Some(super::ConfigFormat::Yaml) => scan_yaml(content, &mut slots),
        Some(super::ConfigFormat::Xml) => scan_xml(content, &mut slots),
        None => {}
    }
    slots
}

/// Where a YAML line sits in the Messenger configuration.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    /// Directly under `messenger:`.
    Messenger,
    /// Directly under its `buses:`.
    Buses,
}

fn scan_yaml(content: &str, slots: &mut Vec<NameSlot>) {
    // The mapping keys the line being read is nested under, by indentation.
    let mut path: Vec<(usize, &str)> = Vec::new();
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = super::resources::yaml_content_before_comment(line);
        let trimmed = semantic.trim();
        if trimmed.is_empty() || trimmed.starts_with('-') {
            continue;
        }
        let indent = crate::framework::leading_spaces(semantic);
        let Some((raw_key, key_start)) = super::resources::yaml_mapping_key(semantic, line_start)
        else {
            continue;
        };
        let (key, (quote, _)) = crate::framework::strip_yaml_quotes(raw_key);
        while path.last().is_some_and(|(at, _)| *at >= indent) {
            path.pop();
        }
        let section = match path.as_slice() {
            [.., (_, "messenger")] => Some(Section::Messenger),
            [.., (_, "messenger"), (_, "buses")] => Some(Section::Buses),
            _ => None,
        };
        match section {
            Some(Section::Buses) => slots.push(NameSlot {
                kind: StringKeyKind::MessengerBus,
                start: key_start + quote,
                end: key_start + quote + key.len(),
                is_write: true,
            }),
            Some(Section::Messenger) if key == "default_bus" => {
                let colon = semantic[indent..].find(':').map_or(0, |at| indent + at + 1);
                let after = &semantic[colon..];
                let lead = after.len() - after.trim_start().len();
                let (value, (quote, _)) = crate::framework::strip_yaml_quotes(after.trim());
                let start = line_start + colon + lead + quote;
                slots.push(NameSlot {
                    kind: StringKeyKind::MessengerBus,
                    start,
                    end: start + value.len(),
                    is_write: false,
                });
            }
            _ => {}
        }
        path.push((indent, key));
    }
}

fn scan_xml(content: &str, slots: &mut Vec<NameSlot>) {
    if !content.contains("messenger") {
        return;
    }
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    while let Some(relative) = memchr::memchr(b'<', &bytes[cursor..]) {
        let tag_start = cursor + relative;
        let Some(tag_len) = memchr::memchr(b'>', &bytes[tag_start..]) else {
            break;
        };
        let tag_end = tag_start + tag_len + 1;
        cursor = tag_end;
        let tag = &content[tag_start..tag_end];
        let name_end = tag[1..]
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .map_or(tag.len(), |at| at + 1);
        let element = &tag[1..name_end];
        let element = element.rsplit(':').next().unwrap_or(element);
        let (attribute, is_write) = match element {
            "bus" => ("name", true),
            "messenger" => ("default-bus", false),
            _ => continue,
        };
        if let Some((_, start, end)) =
            crate::framework::xml_attr_value(tag, tag_start, &[attribute])
        {
            slots.push(NameSlot {
                kind: StringKeyKind::MessengerBus,
                start,
                end,
                is_write,
            });
        }
    }
}

impl Backend {
    /// Visit every Messenger handler the project registers.
    fn visit_messenger_handlers(&self, mut visit: impl FnMut(&MessengerHandlerSite)) {
        let mut uris: Vec<String> = self.symfony.declaring_uris.read().iter().cloned().collect();
        uris.sort_unstable();
        for uri in uris {
            if let Some(map) = self.symbol_map_for(&uri) {
                map.messenger_handlers.iter().for_each(&mut visit);
            }
        }
    }

    /// The handlers that receive a `message`: the ones registered for its
    /// class, and for any class it extends or interface it implements, as
    /// Messenger's handler lookup matches them.
    pub(crate) fn symfony_message_handlers(
        &self,
        message: &crate::types::ClassInfo,
    ) -> Vec<MessengerHandlerSite> {
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        let mut handlers = Vec::new();
        self.visit_messenger_handlers(|site| {
            if crate::class_lookup::is_subtype_of(message, &site.message, &class_loader) {
                handlers.push(site.clone());
            }
        });
        handlers.sort_by_key(|a| (a.handler, a.method));
        handlers.dedup_by(|a, b| a.handler == b.handler && a.method == b.method);
        handlers
    }

    /// The messages the handler class `handler` is registered for, each
    /// with the method that handles it, as its own file records them.
    pub(crate) fn symfony_handled_messages(&self, uri: &str, handler: Atom) -> Vec<(Atom, Atom)> {
        self.symbol_map_for(uri)
            .map(|map| {
                map.messenger_handlers
                    .iter()
                    .filter(|site| site.handler == handler)
                    .map(|site| (site.message, site.method))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(uri: &str, content: &str) -> Vec<(String, bool)> {
        bus_slots(uri, content)
            .into_iter()
            .map(|slot| (content[slot.start..slot.end].to_string(), slot.is_write))
            .collect()
    }

    #[test]
    fn yaml_declares_buses_and_reads_the_default_one() {
        let yaml = "framework:
    messenger:
        default_bus: command.bus
        buses:
            command.bus:
                middleware: [validation]
            'query.bus': ~
        transports:
            async: '%env(MESSENGER_TRANSPORT_DSN)%'
when@test:
    framework:
        messenger:
            default_bus: ''
            buses:
                test.bus: ~
";
        assert_eq!(
            named("file:///p/config/packages/messenger.yaml", yaml),
            vec![
                ("command.bus".to_string(), false),
                ("command.bus".to_string(), true),
                ("query.bus".to_string(), true),
                (String::new(), false),
                ("test.bus".to_string(), true),
            ]
        );
    }

    #[test]
    fn xml_declares_buses_and_reads_the_default_one() {
        let xml = r#"<framework:config>
    <framework:messenger default-bus="command.bus">
        <framework:bus name="command.bus"/>
        <framework:bus name="query.bus">
            <framework:middleware id="validation"/>
        </framework:bus>
    </framework:messenger>
</framework:config>
"#;
        assert_eq!(
            named("file:///p/config/packages/messenger.xml", xml),
            vec![
                ("command.bus".to_string(), false),
                ("command.bus".to_string(), true),
                ("query.bus".to_string(), true),
            ]
        );
    }
}
