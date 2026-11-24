//! The PHP callables and named arguments configured on a service.
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

use super::service_arguments::{Bindings, Target, argument_key, argument_spans};
use crate::atom::{Atom, atom};
use crate::symbol_map::{DocblockMemberRef, SubjectText, SymbolKind, SymbolSpan};
use std::ops::Range;
use std::sync::Arc;

/// The location and receiver of a configured method.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CallSite {
    /// The class name or the factory service reference (`@id`).
    pub(crate) class: String,
    /// Whether this factory calls the method statically.
    pub(crate) is_static: bool,
    /// Byte offset of the first character in the method name.
    pub(crate) start: usize,
    /// Byte offset after the last character in the method name.
    pub(crate) end: usize,
}

/// Method and named-argument spans for a services file.
pub(crate) fn service_call_spans(uri: &str, content: &str) -> Vec<SymbolSpan> {
    let mut spans = Vec::new();
    let calls = sites(uri, content, &mut spans);
    spans.extend(
        calls
            .into_iter()
            .filter(|site| is_method_name(&content[site.start..site.end]))
            .map(|site| SymbolSpan {
                start: site.start as u32,
                end: site.end as u32,
                kind: SymbolKind::MemberAccess {
                    member_name: atom(&content[site.start..site.end]),
                    subject_text: SubjectText::owned(site.class),
                    is_static: site.is_static,
                    is_method_call: true,
                    docblock_ref: DocblockMemberRef::No,
                    is_array_callable: false,
                    is_nullsafe: false,
                },
            }),
    );
    spans
}

/// The call site the cursor is typing a method name at, if any.
pub(crate) fn call_site_at(uri: &str, content: &str, cursor: usize) -> Option<CallSite> {
    sites(uri, content, &mut Vec::new())
        .into_iter()
        .find(|site| (site.start..=site.end).contains(&cursor))
}

fn sites(uri: &str, content: &str, arguments: &mut Vec<SymbolSpan>) -> Vec<CallSite> {
    match super::config_format(uri) {
        Some(super::ConfigFormat::Yaml)
            if ["calls", "factory", "tags", "arguments", "bind"]
                .iter()
                .any(|key| content.contains(key)) =>
        {
            yaml_call_sites(content, arguments)
        }
        Some(super::ConfigFormat::Xml) if content.contains("<call") || content.contains("<tag") => {
            xml_call_sites(content)
        }
        _ => Vec::new(),
    }
}

/// Whether `name` is a PHP identifier.
pub(super) fn is_method_name(name: &str) -> bool {
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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Property {
    Calls,
    Tags,
    Arguments,
    Excludes,
    #[default]
    Other,
}

/// A service definition being read.
struct Service {
    indent: usize,
    key_class: Option<String>,
    namespace: Option<Atom>,
    resource: Option<Atom>,
    excludes: Vec<Atom>,
    property: Property,
    defaults: bool,
    tags: Option<Range<usize>>,
    argument_indent: Option<usize>,
    arguments: Vec<super::service_arguments::Argument>,
    factory: Option<CallSite>,
    has_factory: bool,
    class: Option<String>,
    property_indent: Option<usize>,
    item: Option<Item>,
    names: Vec<(usize, usize)>,
}

impl Service {
    fn new(indent: usize, key: &str) -> Self {
        Self {
            indent,
            key_class: class_name(key),
            namespace: key
                .ends_with('\\')
                .then(|| atom(&crate::framework::normalize_framework_fqn(key))),
            resource: None,
            excludes: Vec::new(),
            property: Property::Other,
            defaults: key == "_defaults",
            tags: None,
            argument_indent: None,
            arguments: Vec::new(),
            factory: None,
            has_factory: false,
            class: None,
            property_indent: None,
            item: None,
            names: Vec::new(),
        }
    }

    fn finish(
        mut self,
        content: &str,
        sites: &mut Vec<CallSite>,
        bindings: &mut Bindings,
        spans: &mut Vec<SymbolSpan>,
    ) {
        if self.defaults {
            bindings.defaults.extend(self.arguments);
            return;
        }
        let class = self.class.or(self.key_class);
        let target = if let Some(factory) = self.factory {
            let target = Target::Method {
                owner: atom(&factory.class),
                method: atom(&content[factory.start..factory.end]),
            };
            sites.push(factory);
            Some(target)
        } else if self.has_factory || (self.arguments.is_empty() && !bindings.collect_targets) {
            None
        } else if let (Some(namespace), Some(directory)) = (self.namespace, self.resource) {
            Some(Target::Resource {
                namespace,
                directory,
                excludes: self.excludes,
            })
        } else {
            class.as_ref().map(|class| Target::Method {
                owner: atom(class),
                method: atom("__construct"),
            })
        };
        if let Some(target) = target {
            if !self.arguments.is_empty() {
                argument_spans(content, self.arguments, Arc::from([target.clone()]), spans);
            }
            if bindings.collect_targets {
                bindings.targets.push(target);
            }
        }
        let Some(class) = class else {
            return;
        };
        if let Some(tags) = self.tags {
            super::tags::listener_methods(&content[tags.clone()], tags.start, &mut self.names);
        }
        for (start, end) in self.names {
            sites.push(CallSite {
                class: class.clone(),
                is_static: false,
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
        let compact_entry = indent == property_indent
            && matches!(
                self.property,
                Property::Calls | Property::Tags | Property::Excludes
            )
            && trimmed.starts_with('-');
        if indent <= property_indent && !compact_entry {
            self.property = Property::Other;
            self.argument_indent = None;
            self.item = None;
            if indent < property_indent {
                return;
            }
            let Some((key, value, value_offset)) = mapping_entry(trimmed) else {
                return;
            };
            match key {
                "class" => self.class = class_name(value),
                "resource" if self.namespace.is_some() => {
                    self.resource = Some(atom(crate::framework::strip_yaml_quotes(value).0))
                }
                "exclude" if self.namespace.is_some() => {
                    if value.is_empty() {
                        self.property = Property::Excludes;
                    } else {
                        if let Some(list) =
                            value.strip_prefix('[').and_then(|v| v.strip_suffix(']'))
                        {
                            self.excludes
                                .extend(list.split(',').map(|v| {
                                    atom(crate::framework::strip_yaml_quotes(v.trim()).0)
                                }));
                        } else {
                            self.excludes
                                .push(atom(crate::framework::strip_yaml_quotes(value).0));
                        }
                    }
                }
                "tags" => {
                    self.property = Property::Tags;
                    self.tags = Some(line_start..line_start + text.len());
                }
                "factory" => {
                    self.has_factory = true;
                    self.factory = factory_site(value, line_start + indent + value_offset);
                }
                "arguments" | "bind" => {
                    if value.is_empty() {
                        self.property = Property::Arguments;
                    } else if value.starts_with('{') {
                        let (keys, _) = super::validation::flow_mapping_keys(value, 0);
                        for (start, end) in keys {
                            argument_key(
                                &value[start..end],
                                line_start + indent + value_offset + start,
                                &mut self.arguments,
                            );
                        }
                    }
                }
                "calls" if value.is_empty() => self.property = Property::Calls,
                _ => {}
            }
            return;
        }
        if self.property == Property::Excludes
            && let Some(value) = trimmed.strip_prefix("- ")
        {
            self.excludes
                .push(atom(crate::framework::strip_yaml_quotes(value.trim()).0));
        }
        if self.property == Property::Tags
            && let Some(range) = &mut self.tags
        {
            range.end = line_start + text.len();
        }
        if self.property == Property::Arguments
            && indent == *self.argument_indent.get_or_insert(indent)
            && let Some((key, _, _)) = mapping_entry(text)
        {
            argument_key(
                key,
                line_start + super::resources::offset_within(text, key),
                &mut self.arguments,
            );
        }
        if self.property != Property::Calls {
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

/// The receiver and method a service's factory calls. A receiver beginning
/// with @ is resolved through the live container index at request time.
fn factory_site(value: &str, start: usize) -> Option<CallSite> {
    let (value_text, _) = crate::framework::strip_yaml_quotes(value);
    let (owner, raw_method) = if let Some(list) = value_text
        .strip_prefix('[')
        .and_then(|list| list.strip_suffix(']'))
    {
        list.split_once(',')?
    } else {
        value_text.split_once("::")?
    };
    let (owner, _) = crate::framework::strip_yaml_quotes(owner.trim());
    let class = if let Some(id) = owner.strip_prefix('@') {
        if !super::resources::is_container_key(id) && class_name(id).is_none() {
            return None;
        }
        crate::framework::normalize_framework_fqn(owner)
    } else {
        class_name(owner)?
    };
    let raw = raw_method.trim();
    let (start, end) = name_site(raw, start + super::resources::offset_within(value, raw));
    Some(CallSite {
        is_static: !class.starts_with('@'),
        class,
        start,
        end,
    })
}

fn yaml_call_sites(content: &str, spans: &mut Vec<SymbolSpan>) -> Vec<CallSite> {
    let mut bindings = Bindings {
        collect_targets: content.contains("_defaults") && content.contains("bind"),
        ..Bindings::default()
    };
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
            done.finish(content, &mut sites, &mut bindings, spans);
        }
        if let Some(open) = service.as_mut() {
            open.read(text, line_start, indent);
            continue;
        }

        if trimmed == "services:" {
            bindings.finish(content, spans);
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
            if (value.is_empty() || value == "~") && (!key.starts_with('_') || key == "_defaults") {
                service = Some(Service::new(indent, key));
            }
        } else if let Some((instanceof_indent, entries)) = instanceof.as_mut()
            && indent > *instanceof_indent
            && indent == *entries.get_or_insert(indent)
            && value.is_empty()
        {
            service = Some(Service::new(indent, key));
        }
    }
    if let Some(done) = service {
        done.finish(content, &mut sites, &mut bindings, spans);
    }
    bindings.finish(content, spans);
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
            "call" | "tag"
                if !closing
                    && (name == "call"
                        || attribute(&["name"])
                            .is_some_and(|(tag, _, _)| tag == super::tags::EVENT_LISTENER_TAG)) =>
            {
                if let Some(Some(class)) = services.last()
                    && let Some((_, start, end)) = attribute(&["method"])
                {
                    sites.push(CallSite {
                        class: class.clone(),
                        is_static: false,
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
        sites(uri, content, &mut Vec::new())
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
    fn unsupported_factories_and_positional_keys_are_not_parameter_sites() {
        let yaml = r"services:
  unknown.factory:
    factory: ['@bad id', make]
    arguments: { 0: zero, $0bad: value }
  App\:
    resource: ../src/
    exclude: ['../src/One', '../src/Two']
  App\Elsewhere\:
    resource: ../elsewhere/
    exclude: '../elsewhere/Skip'
  _defaults:
    bind: { Type: value, $path: value }
";
        let spans = service_call_spans("file:///p/config/services.yaml", yaml);
        assert_eq!(spans.len(), 1);
        let span = &spans[0];
        assert_eq!(&yaml[span.start as usize..span.end as usize], "$path");
        assert!(
            matches!(&span.kind, SymbolKind::ServiceArgument { targets, .. }
            if matches!(targets.as_ref(), [Target::Resource { excludes: first, .. }, Target::Resource { excludes: second, .. }]
                if first.len() == 2 && second.len() == 1))
        );
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
        let sites = sites("file:///p/config/services.yaml", yaml, &mut Vec::new());
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
