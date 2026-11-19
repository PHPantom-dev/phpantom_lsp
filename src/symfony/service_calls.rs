//! The methods a service definition's `calls:` names.
//!
//! ```yaml
//! services:
//!     App\Mailer:
//!         calls:
//!             - [setLogger, ['@logger']]
//!             - setTransport: ['@mailer.transport']
//!             - { method: setFormatter, arguments: ['@formatter'] }
//! ```
//!
//! The container calls each of them on the service after building it, so
//! each name is a method of the service's class: its `class:`, or its key
//! when that is a class name.  XML spells the same thing
//! `<call method="setLogger">` inside a `<service>`.  Symfony's services
//! files and Drupal's `*.services.yml` share the format.
//!
//! The names become member accesses on the class in the file's resource
//! symbol map, which go-to-definition and Find References already read,
//! and the position of one is where completion offers the class's methods.

use crate::atom::atom;
use crate::symbol_map::{DocblockMemberRef, SubjectText, SymbolKind, SymbolSpan};

/// Where a method name is, or would be written, under a service's `calls:`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CallSite {
    /// The class the method belongs to.
    pub(crate) class: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// The method-call spans of one services file.
pub(crate) fn service_call_spans(uri: &str, content: &str) -> Vec<SymbolSpan> {
    call_sites(uri, content)
        .into_iter()
        .filter(|site| is_method_name(&content[site.start..site.end]))
        .map(|site| SymbolSpan {
            start: site.start as u32,
            end: site.end as u32,
            kind: SymbolKind::MemberAccess {
                member_name: atom(&content[site.start..site.end]),
                subject_text: SubjectText::owned(site.class),
                is_static: false,
                is_method_call: true,
                docblock_ref: DocblockMemberRef::No,
                is_array_callable: false,
                is_nullsafe: false,
            },
        })
        .collect()
}

/// The call site the cursor is typing a method name at, if any.
pub(crate) fn call_site_at(uri: &str, content: &str, cursor: usize) -> Option<CallSite> {
    call_sites(uri, content)
        .into_iter()
        .find(|site| (site.start..=site.end).contains(&cursor))
}

fn call_sites(uri: &str, content: &str) -> Vec<CallSite> {
    match super::config_format(uri) {
        Some(super::ConfigFormat::Yaml) if content.contains("calls") => yaml_call_sites(content),
        Some(super::ConfigFormat::Xml) if content.contains("<call") => xml_call_sites(content),
        _ => Vec::new(),
    }
}

/// Whether `name` is a PHP identifier.
fn is_method_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first == b'_' || first.is_ascii_alphabetic() || !first.is_ascii())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric() || !byte.is_ascii())
}

/// A class name as a services file spells it, without its quotes, YAML's
/// doubled backslashes, or a leading backslash.
fn class_name(raw: &str) -> Option<String> {
    let (value, _) = crate::framework::strip_yaml_quotes(raw.trim());
    let name = crate::framework::normalize_framework_fqn(value);
    (name.contains('\\') && name.split('\\').all(is_method_name)).then_some(name)
}

// ─── YAML ───────────────────────────────────────────────────────────────────

/// One entry of a `calls:` list.
struct Item {
    dash_indent: usize,
    /// The column its content starts at, where a block mapping's further
    /// keys line up.
    content_indent: usize,
}

/// A service definition being read.
struct Service {
    indent: usize,
    key_class: Option<String>,
    class: Option<String>,
    property_indent: Option<usize>,
    /// The indentation of `calls:`, while its entries are being read.
    calls_indent: Option<usize>,
    item: Option<Item>,
    names: Vec<(usize, usize)>,
}

impl Service {
    fn new(indent: usize, key_class: Option<String>) -> Self {
        Self {
            indent,
            key_class,
            class: None,
            property_indent: None,
            calls_indent: None,
            item: None,
            names: Vec::new(),
        }
    }

    fn finish(self, sites: &mut Vec<CallSite>) {
        let Some(class) = self.class.or(self.key_class) else {
            return;
        };
        for (start, end) in self.names {
            sites.push(CallSite {
                class: class.clone(),
                start,
                end,
            });
        }
    }

    /// Read one line nested under the service's key.
    fn read(&mut self, text: &str, line_start: usize, indent: usize) {
        let property_indent = *self.property_indent.get_or_insert(indent);
        let trimmed = text.trim();
        // YAML lets a sequence's entries sit at its key's own indentation.
        let compact_entry =
            indent == property_indent && self.calls_indent.is_some() && trimmed.starts_with('-');
        if indent <= property_indent && !compact_entry {
            self.calls_indent = None;
            self.item = None;
            if indent < property_indent {
                return;
            }
            let Some((key, value, _)) = mapping_entry(trimmed) else {
                return;
            };
            match key {
                "class" => self.class = class_name(value),
                "calls" if value.is_empty() => self.calls_indent = Some(indent),
                _ => {}
            }
            return;
        }
        if self.calls_indent.is_none() {
            return;
        }

        let rest = &text[indent..];
        if let Some(after_dash) = rest.strip_prefix('-')
            && self
                .item
                .as_ref()
                .is_none_or(|item| indent <= item.dash_indent)
        {
            let gap = after_dash.len() - after_dash.trim_start_matches(' ').len();
            let content_indent = indent + 1 + gap;
            self.item = Some(Item {
                dash_indent: indent,
                content_indent,
            });
            let content = after_dash.trim_start_matches(' ');
            if let Some(site) = item_name(content, line_start + content_indent) {
                self.names.push(site);
            }
            return;
        }
        // A further key of an entry written as a block mapping.
        if self
            .item
            .as_ref()
            .is_some_and(|item| indent == item.content_indent)
            && let Some(("method", value, value_offset)) = mapping_entry(rest)
        {
            self.names
                .push(name_site(value, line_start + indent + value_offset));
        }
    }
}

/// The key of a `key: value` entry, its value, and the value's offset in
/// `text`.
fn mapping_entry(text: &str) -> Option<(&str, &str, usize)> {
    let colon = text
        .match_indices(':')
        .map(|(at, _)| at)
        .find(|&at| text[at + 1..].is_empty() || text[at + 1..].starts_with([' ', '\t']))?;
    let (key, _) = crate::framework::strip_yaml_quotes(text[..colon].trim());
    let after = &text[colon + 1..];
    let value = after.trim();
    let value_offset = colon + 1 + (after.len() - after.trim_start().len());
    Some((key, value, value_offset))
}

/// Where the method name is in the content of one `calls:` entry, which
/// starts at `start`: `[name, …]`, `name: …`, `method: name`, or
/// `{ method: name, … }`.  A bare word is a name still being typed.
fn item_name(content: &str, start: usize) -> Option<(usize, usize)> {
    if let Some(list) = content.strip_prefix('[') {
        let first = list.split([',', ']']).next().unwrap_or(list);
        let lead = first.len() - first.trim_start().len();
        return Some(name_site(first.trim(), start + 1 + lead));
    }
    if let Some(map) = content.strip_prefix('{') {
        let mut offset = 1;
        for pair in map.split([',', '}']) {
            if let Some(("method", value, value_offset)) = mapping_entry(pair.trim_start()) {
                let lead = pair.len() - pair.trim_start().len();
                return Some(name_site(value, start + offset + lead + value_offset));
            }
            offset += pair.len() + 1;
        }
        return None;
    }
    match mapping_entry(content) {
        Some(("method", value, value_offset)) => Some(name_site(value, start + value_offset)),
        Some(("arguments" | "returns_clone", _, _)) => None,
        Some((_, _, _)) => {
            let key = content[..content.find(':')?].trim_end();
            Some(name_site(key, start))
        }
        None => Some(name_site(content.trim_end(), start)),
    }
}

/// The span of the name in `raw`, which starts at `start`, without its
/// quotes (the closing one may not be typed yet).
fn name_site(raw: &str, start: usize) -> (usize, usize) {
    let quote = usize::from(raw.starts_with(['\'', '"']));
    let name = &raw[quote..];
    let name = name.strip_suffix(['\'', '"']).unwrap_or(name);
    (start + quote, start + quote + name.len())
}

fn yaml_call_sites(content: &str) -> Vec<CallSite> {
    let mut sites = Vec::new();
    // The indentation of `services:` and of its entries.
    let mut section: Option<(usize, Option<usize>)> = None;
    // The indentation of `_instanceof:` and of its entries, which are
    // keyed by the class or interface they configure.
    let mut instanceof: Option<(usize, Option<usize>)> = None;
    let mut service: Option<Service> = None;

    for (line_start, line) in crate::framework::line_offsets(content) {
        let text = super::resources::yaml_content_before_comment(line);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = crate::framework::leading_spaces(text);

        if service.as_ref().is_some_and(|open| indent <= open.indent)
            && let Some(done) = service.take()
        {
            done.finish(&mut sites);
        }
        if let Some(open) = service.as_mut() {
            open.read(text, line_start, indent);
            continue;
        }

        if trimmed == "services:" {
            section = Some((indent, None));
            instanceof = None;
            continue;
        }
        let Some((section_indent, entries)) = section.as_mut() else {
            continue;
        };
        if indent <= *section_indent {
            section = None;
            instanceof = None;
            continue;
        }
        let entry_indent = *entries.get_or_insert(indent);
        let Some((key, value, _)) = mapping_entry(trimmed) else {
            continue;
        };
        if indent == entry_indent {
            instanceof = (key == "_instanceof").then_some((indent, None));
            if value.is_empty() && !key.starts_with('_') {
                service = Some(Service::new(indent, class_name(key)));
            }
        } else if let Some((instanceof_indent, entries)) = instanceof.as_mut()
            && indent > *instanceof_indent
            && indent == *entries.get_or_insert(indent)
            && value.is_empty()
        {
            service = Some(Service::new(indent, class_name(key)));
        }
    }
    if let Some(done) = service {
        done.finish(&mut sites);
    }
    sites
}

// ─── XML ────────────────────────────────────────────────────────────────────

fn xml_call_sites(content: &str) -> Vec<CallSite> {
    let bytes = content.as_bytes();
    let mut sites = Vec::new();
    // The class of each `<service>` or `<instanceof>` the scan is inside.
    let mut services: Vec<Option<String>> = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = memchr::memchr(b'<', &bytes[cursor..]) {
        let tag_start = cursor + relative;
        if content[tag_start..].starts_with("<!--") {
            cursor = content[tag_start..]
                .find("-->")
                .map_or(bytes.len(), |end| tag_start + end + 3);
            continue;
        }
        let tag_end =
            memchr::memchr(b'>', &bytes[tag_start..]).map_or(bytes.len(), |n| tag_start + n + 1);
        let tag = &content[tag_start..tag_end];
        cursor = tag_end;

        let (closing, body) = match tag[1..].strip_prefix('/') {
            Some(body) => (true, body),
            None => (false, &tag[1..]),
        };
        let name = &body[..body
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .unwrap_or(body.len())];
        let attribute = |names: &[&str]| crate::framework::xml_attr_value(tag, tag_start, names);
        match name {
            "service" | "instanceof" if closing => {
                services.pop();
            }
            "service" | "instanceof" if !tag.ends_with("/>") => {
                let class = attribute(&["class"])
                    .or_else(|| attribute(&["id"]))
                    .and_then(|(value, _, _)| class_name(&value));
                services.push(class);
            }
            "call" if !closing => {
                if let Some(Some(class)) = services.last()
                    && let Some((_, start, end)) = attribute(&["method"])
                {
                    sites.push(CallSite {
                        class: class.clone(),
                        start,
                        end,
                    });
                }
            }
            _ => {}
        }
    }
    sites
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(uri: &str, content: &str) -> Vec<(String, String)> {
        call_sites(uri, content)
            .into_iter()
            .map(|site| (site.class, content[site.start..site.end].to_string()))
            .collect()
    }

    fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
        expected
            .iter()
            .map(|(class, name)| (class.to_string(), name.to_string()))
            .collect()
    }

    #[test]
    fn yaml_calls_in_every_spelling() {
        let yaml = concat!(
            "services:\n",
            "    _defaults:\n",
            "        autowire: true\n",
            "    App\\Mailer:\n",
            "        calls:\n",
            "            - [setLogger, ['@logger']]\n",
            "            - [ 'setQuoted' ]\n",
            "            - setTransport: ['@mailer.transport']\n",
            "            - withClock: !returns_clone ['@clock']\n",
            "            - { method: setFormatter, arguments: ['@formatter'] }\n",
            "            - method: setRetries\n",
            "              arguments:\n",
            "                  - 3\n",
            "            - arguments: [1]\n",
            "              method: setLimit\n",
            "        tags: [mailer]\n",
            "    mailer.alias:\n",
            "        calls:\n",
            "        - [setOther]\n",
            "        class: \"App\\\\Other\"\n",
            "    no.class:\n",
            "        calls:\n",
            "            - [ignored]\n",
        );
        assert_eq!(
            named("file:///p/config/services.yaml", yaml),
            pairs(&[
                ("App\\Mailer", "setLogger"),
                ("App\\Mailer", "setQuoted"),
                ("App\\Mailer", "setTransport"),
                ("App\\Mailer", "withClock"),
                ("App\\Mailer", "setFormatter"),
                ("App\\Mailer", "setRetries"),
                ("App\\Mailer", "setLimit"),
                ("App\\Other", "setOther"),
            ])
        );
    }

    #[test]
    fn yaml_instanceof_entries_are_keyed_by_their_class() {
        let yaml = concat!(
            "services:\n",
            "  _instanceof:\n",
            "    App\\LoggerAware:\n",
            "      calls:\n",
            "        - [setLogger, ['@logger']]\n",
            "  App\\Mailer: ~\n",
        );
        assert_eq!(
            named("file:///p/config/services.yaml", yaml),
            pairs(&[("App\\LoggerAware", "setLogger")])
        );
    }

    #[test]
    fn yaml_calls_outside_services_are_not_read() {
        let yaml = "framework:\n    App\\Mailer:\n        calls:\n            - [setLogger]\n";
        assert!(named("file:///p/config/packages/x.yaml", yaml).is_empty());
    }

    #[test]
    fn a_name_being_typed_is_a_site() {
        let yaml = "services:\n  App\\Mailer:\n    calls:\n      - [set\n      - \n";
        let sites = call_sites("file:///p/config/services.yaml", yaml);
        assert_eq!(sites.len(), 2);
        assert_eq!(&yaml[sites[0].start..sites[0].end], "set");
        assert_eq!(sites[1].start, sites[1].end);
        assert!(service_call_spans("file:///p/config/services.yaml", yaml).len() == 1);
    }

    #[test]
    fn xml_calls_name_the_service_class() {
        let xml = concat!(
            "<services>\n",
            "  <service id=\"mailer\" class=\"App\\Mailer\">\n",
            "    <argument type=\"service\"><service class=\"App\\Inner\"/></argument>\n",
            "    <call method=\"setLogger\"><argument type=\"service\" id=\"logger\"/></call>\n",
            "  </service>\n",
            "  <service id=\"App\\Other\">\n",
            "    <call method=\"setClock\"/>\n",
            "  </service>\n",
            "</services>\n",
        );
        assert_eq!(
            named("file:///p/config/services.xml", xml),
            pairs(&[("App\\Mailer", "setLogger"), ("App\\Other", "setClock")])
        );
    }
}
