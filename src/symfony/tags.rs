//! The service tags that name an event or a Messenger bus in Symfony's YAML
//! and XML configuration: a `kernel.event_listener` tag subscribes its
//! service to the event its `event` names, and a
//! `messenger.message_handler` tag registers its service on the bus its
//! `bus` names.
//!
//! The YAML is read as text, line by line.  A tag is a flow mapping
//! (`- { name: kernel.event_listener, event: app.order.placed }`), a block
//! mapping under a list item (`- name: kernel.event_listener` with its
//! attributes on the lines after it), or either shape keyed by the tag's
//! name (`- kernel.event_listener: { event: app.order.placed }`).

use super::NameSlot;
use crate::symbol_map::StringKeyKind;

const EVENT_LISTENER_TAG: &str = "kernel.event_listener";
const MESSAGE_HANDLER_TAG: &str = "messenger.message_handler";

/// The attribute of a tag named `tag` that names an event or a bus, with
/// the kind of name it is and whether the tag declares it.
fn named_attribute(tag: &str) -> Option<(&'static str, StringKeyKind, bool)> {
    match tag {
        EVENT_LISTENER_TAG => Some(("event", StringKeyKind::SymfonyEvent, true)),
        MESSAGE_HANDLER_TAG => Some(("bus", StringKeyKind::MessengerBus, false)),
        _ => None,
    }
}

/// The event and bus names the service tags of one YAML or XML
/// configuration file hold, including the ones still being typed.
pub(crate) fn tag_slots(uri: &str, content: &str) -> Vec<NameSlot> {
    let mut slots = Vec::new();
    match super::config_format(uri) {
        Some(super::ConfigFormat::Yaml) => scan_yaml(content, &mut slots),
        Some(super::ConfigFormat::Xml) => scan_xml(content, &mut slots),
        None => {}
    }
    slots
}

// ─── YAML ───────────────────────────────────────────────────────────────────

/// One `key: value` of a tag, with where its value starts and ends.
struct Pair<'c> {
    key: &'c str,
    value: &'c str,
    start: usize,
    end: usize,
}

/// A tag being read: its name, when written as the key of the mapping
/// rather than as its `name` attribute, and its attributes.
#[derive(Default)]
struct Tag<'c> {
    name: Option<&'c str>,
    pairs: Vec<Pair<'c>>,
}

impl<'c> Tag<'c> {
    fn finish(self, slots: &mut Vec<NameSlot>) {
        let name = self.name.or_else(|| {
            self.pairs
                .iter()
                .find(|pair| pair.key == "name")
                .map(|pair| pair.value)
        });
        let Some((attribute, kind, is_write)) = name.and_then(named_attribute) else {
            return;
        };
        for pair in self.pairs.iter().filter(|pair| pair.key == attribute) {
            slots.push(NameSlot {
                kind,
                start: pair.start,
                end: pair.end,
                is_write,
            });
        }
    }
}

/// A block-mapping tag under a list item, still open.
struct BlockTag<'c> {
    tag: Tag<'c>,
    /// The indentation of the item's `-`.
    dash_indent: usize,
    /// The indentation of the tag's attributes, once known.
    indent: Option<usize>,
}

/// The `key: value` a stretch of YAML holds, `text` starting at the
/// absolute offset `offset`.  The value is unquoted and may be empty.
fn pair(text: &str, offset: usize) -> Option<Pair<'_>> {
    let colon = text.find(':')?;
    let key = text[..colon].trim();
    let (key, _) = crate::framework::strip_yaml_quotes(key);
    if key.is_empty() {
        return None;
    }
    let after = &text[colon + 1..];
    let lead = after.len() - after.trim_start().len();
    let raw = after.trim();
    let (value, (quote, _)) = crate::framework::strip_yaml_quotes(raw);
    let start = offset + colon + 1 + lead + quote;
    Some(Pair {
        key,
        value,
        start,
        end: start + value.len(),
    })
}

fn scan_yaml(content: &str, slots: &mut Vec<NameSlot>) {
    let mut open: Option<BlockTag<'_>> = None;
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = super::resources::yaml_content_before_comment(line);
        let trimmed = semantic.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = crate::framework::leading_spaces(semantic);

        // A line at or left of an open item's `-` ends it, and so does one
        // left of its attributes.
        if let Some(block) = &open
            && (indent <= block.dash_indent || block.indent.is_some_and(|at| indent < at))
            && let Some(block) = open.take()
        {
            block.tag.finish(slots);
        }

        if let Some(brace) = semantic.find('{') {
            scan_flow_mappings(semantic, line_start, brace, slots);
            continue;
        }

        if let Some(item) = trimmed.strip_prefix("- ") {
            let item_start = semantic.len() - trimmed.len() + 2;
            let item_indent = item_start + (item.len() - item.trim_start().len());
            let mut block = BlockTag {
                tag: Tag::default(),
                dash_indent: indent,
                indent: None,
            };
            if let Some(found) = pair(&semantic[item_indent..], line_start + item_indent) {
                if found.value.is_empty() && !found.key.contains(' ') {
                    // `- kernel.event_listener:` with its attributes nested
                    // under it.
                    block.tag.name = Some(found.key);
                } else {
                    block.indent = Some(item_indent);
                    block.tag.pairs.push(found);
                }
                open = Some(block);
            }
            continue;
        }

        if let Some(block) = &mut open {
            let at = *block.indent.get_or_insert(indent);
            if indent == at
                && let Some(found) = pair(semantic, line_start)
            {
                block.tag.pairs.push(found);
            }
        }
    }
    if let Some(block) = open {
        block.tag.finish(slots);
    }
}

/// The tags written as flow mappings on one line, the first `{` at
/// `brace`.  A mapping keyed by a tag's name (`kernel.event_listener: {…}`)
/// takes that name.
fn scan_flow_mappings(line: &str, line_start: usize, brace: usize, slots: &mut Vec<NameSlot>) {
    let mut cursor = brace;
    while let Some(open) = line[cursor..].find('{').map(|at| cursor + at) {
        let close = line[open..].find('}').map_or(line.len(), |at| open + at);
        let mut tag = Tag::default();
        let before = line[..open].trim_end();
        if let Some(key) = before.strip_suffix(':') {
            let key = key.trim_start_matches(|c: char| c == '-' || c == '[' || c.is_whitespace());
            let key = key.rsplit([' ', '[', ',']).next().unwrap_or(key);
            tag.name = Some(crate::framework::strip_yaml_quotes(key).0);
        }
        let mut item_start = open + 1;
        for item in line[open + 1..close].split(',') {
            if let Some(found) = pair(item, line_start + item_start) {
                tag.pairs.push(found);
            }
            item_start += item.len() + 1;
        }
        tag.finish(slots);
        cursor = close.min(line.len().saturating_sub(1)) + 1;
        if cursor >= line.len() {
            break;
        }
    }
}

// ─── XML ────────────────────────────────────────────────────────────────────

fn scan_xml(content: &str, slots: &mut Vec<NameSlot>) {
    let mut cursor = 0usize;
    while let Some(tag_start) = content[cursor..].find("<tag").map(|at| cursor + at) {
        let Some(tag_end) = content[tag_start..].find('>').map(|at| tag_start + at + 1) else {
            break;
        };
        cursor = tag_end;
        let tag = &content[tag_start..tag_end];
        let Some((name, _, _)) = crate::framework::xml_attr_value(tag, tag_start, &["name"]) else {
            continue;
        };
        let Some((attribute, kind, is_write)) = named_attribute(&name) else {
            continue;
        };
        if let Some((_, start, end)) =
            crate::framework::xml_attr_value(tag, tag_start, &[attribute])
        {
            slots.push(NameSlot {
                kind,
                start,
                end,
                is_write,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(uri: &str, content: &str) -> Vec<(StringKeyKind, String, bool)> {
        tag_slots(uri, content)
            .into_iter()
            .map(|slot| {
                (
                    slot.kind,
                    content[slot.start..slot.end].to_string(),
                    slot.is_write,
                )
            })
            .collect()
    }

    use StringKeyKind::{MessengerBus as Bus, SymfonyEvent as Event};

    #[test]
    fn yaml_tags_subscribe_listeners_and_register_handlers() {
        let yaml = "services:
    App\\Listener\\A:
        tags:
            - { name: kernel.event_listener, event: app.order.placed, method: onPlaced }
            - { name: kernel.event_listener, event: '' }
            - { name: other.tag, event: not.an.event }
    App\\Listener\\B:
        tags:
            - name: kernel.event_listener
              event: 'app.order.shipped' # shipped
              method: onShipped
            - event: app.order.cancelled
              name: kernel.event_listener
    App\\Listener\\C:
        tags:
            - kernel.event_listener: { event: app.order.refunded }
            - kernel.event_listener:
                  event: app.order.returned
            - messenger.message_handler: { bus: command.bus }
    App\\Handler\\D:
        tags: [{ name: messenger.message_handler, bus: query.bus }]
";
        assert_eq!(
            named("file:///p/config/services.yaml", yaml),
            vec![
                (Event, "app.order.placed".to_string(), true),
                (Event, String::new(), true),
                (Event, "app.order.shipped".to_string(), true),
                (Event, "app.order.cancelled".to_string(), true),
                (Event, "app.order.refunded".to_string(), true),
                (Event, "app.order.returned".to_string(), true),
                (Bus, "command.bus".to_string(), false),
                (Bus, "query.bus".to_string(), false),
            ]
        );
    }

    #[test]
    fn xml_tags_subscribe_listeners_and_register_handlers() {
        let xml = r#"<services>
  <service id="App\Listener\A">
    <tag name="kernel.event_listener" event="app.order.placed" method="onPlaced"/>
    <tag name="other.tag" event="not.an.event"/>
  </service>
  <service id="App\Handler\D">
    <tag name="messenger.message_handler" bus="query.bus"/>
  </service>
</services>
"#;
        assert_eq!(
            named("file:///p/config/services.xml", xml),
            vec![
                (Event, "app.order.placed".to_string(), true),
                (Bus, "query.bus".to_string(), false),
            ]
        );
    }
}
