//! Symfony validation mappings in YAML and XML: the properties a class's
//! constraints are declared on, and the constraints by their short names.
//!
//! A mapped property is a reference to the class's property, so it becomes
//! a member access on the class in the file's resource symbol map, which
//! every member feature already reads.  A constraint written by its short
//! name (`NotBlank`) is one of the Validator's own, so it becomes a class
//! reference to `Symfony\Component\Validator\Constraints\NotBlank`.  One
//! written by its class name is already the class reference every resource
//! file records.
//!
//! A constraint's options are the named arguments its constructor is
//! called with, so the option keys the mapping gives a constraint (the keys
//! of its YAML mapping, the `<option name="…">` of its XML element) are
//! recorded with the constraint's class, for completion and for the check
//! that each one names an option the constraint takes (see
//! [`Backend::constraint_options`]).
//!
//! The scan reads the mapping as text, by indentation in YAML and by tag in
//! XML.  The constraints an option of another constraint holds (`All`'s,
//! `Sequentially`'s) are left alone in YAML, where telling them from an
//! option's other values would take reading the constraint's options.

use std::sync::Arc;

use crate::Backend;
use crate::atom::atom;
use crate::class_lookup::Fqn;
use crate::symbol_map::{ClassRefContext, DocblockMemberRef, SubjectText, SymbolKind, SymbolSpan};
use crate::types::ClassInfo;

const CONSTRAINTS_NS: &str = "Symfony\\Component\\Validator\\Constraints\\";

/// Whether `uri` is a validation mapping: a `validation.*` file or one in a
/// `validator` or `validation` directory, under a `config` directory (the
/// application's `config/validator/`, or a bundle's
/// `Resources/config/validation.yaml`).
pub(crate) fn is_validation_mapping_uri(uri: &str) -> bool {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    let mut segments = path.split('/').peekable();
    let mut in_config = false;
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            return in_config && segment.starts_with("validation.");
        }
        if in_config && (segment == "validator" || segment == "validation") {
            return true;
        }
        in_config |= segment == "config";
    }
    false
}

/// The property and constraint spans of one validation mapping.
pub(crate) fn validation_spans(uri: &str, content: &str) -> Vec<SymbolSpan> {
    scan(uri, content).spans
}

/// The option keys each constraint of one validation mapping is given.
pub(crate) fn constraint_option_keys(uri: &str, content: &str) -> Vec<ConstraintOptionKeys> {
    scan(uri, content).options
}

/// What a scan of a validation mapping finds.
#[derive(Default)]
struct Scan {
    spans: Vec<SymbolSpan>,
    options: Vec<ConstraintOptionKeys>,
}

fn scan(uri: &str, content: &str) -> Scan {
    let path = uri
        .split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .to_ascii_lowercase();
    let path = path.strip_suffix(".dist").unwrap_or(&path);
    let mut found = Scan::default();
    if path.ends_with(".yaml") || path.ends_with(".yml") {
        scan_yaml(content, &mut found);
    } else if path.ends_with(".xml") && content.contains("<constraint-mapping") {
        scan_xml(content, &mut found);
    }
    found
}

/// The options a mapping gives one constraint.
pub(crate) struct ConstraintOptionKeys {
    /// The constraint's class.
    pub constraint: String,
    /// Each option key's byte range, quotes excluded.  An XML option's may
    /// be empty.
    pub keys: Vec<(usize, usize)>,
    /// Where in the mapping a key of this constraint is written.
    slot: OptionSlot,
}

impl ConstraintOptionKeys {
    /// Whether the keys are all the constraint's, rather than the text of
    /// a flow mapping not yet closed or a list read for keys.
    pub(crate) fn is_judged(&self) -> bool {
        match self.slot {
            OptionSlot::Flow { close, .. } => close.is_some(),
            OptionSlot::Block { is_list, .. } => !is_list,
            OptionSlot::Xml => true,
        }
    }
}

enum OptionSlot {
    /// The YAML flow mapping that opens at `open` and closes at `close`,
    /// when it does.
    Flow { open: usize, close: Option<usize> },
    /// The YAML block mapping under a constraint list item: the lines from
    /// `start` to `end`, more indented than the item's dash at
    /// `item_indent`, with the keys at `key_indent` once a line sets it.
    /// A block that turns out to be a list is no mapping.
    Block {
        start: usize,
        end: usize,
        item_indent: usize,
        key_indent: Option<usize>,
        is_list: bool,
    },
    /// The `name` attributes of an XML constraint's `<option>` elements,
    /// which are its keys.
    Xml,
}

/// The option key of a constraint the cursor is writing: the constraint's
/// class, where the key starts, and the other keys the constraint is given.
pub(crate) fn option_key_at(
    uri: &str,
    content: &str,
    cursor: usize,
) -> Option<(String, usize, Vec<String>)> {
    let found = scan(uri, content);
    found.options.into_iter().find_map(|options| {
        let start = match options.slot {
            OptionSlot::Flow { open, close } => {
                flow_key_start(content, open, close.unwrap_or(content.len()), cursor)?
            }
            OptionSlot::Block {
                start,
                end,
                item_indent,
                key_indent,
                is_list,
            } => {
                if is_list || cursor < start || cursor > end {
                    return None;
                }
                let line_start = content[..cursor].rfind('\n').map_or(0, |at| at + 1);
                let before = &content[line_start..cursor];
                let indent = crate::framework::leading_spaces(before);
                let written = &before[indent..];
                let is_key = indent > item_indent
                    && key_indent.is_none_or(|keys| keys == indent)
                    && written
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
                if !is_key {
                    return None;
                }
                cursor - written.len()
            }
            OptionSlot::Xml => {
                options
                    .keys
                    .iter()
                    .find(|&&(start, end)| start <= cursor && cursor <= end)?
                    .0
            }
        };
        let given = options
            .keys
            .iter()
            .filter(|&&(key_start, _)| key_start != start)
            .map(|&(key_start, key_end)| content[key_start..key_end].to_string())
            .collect();
        Some((options.constraint, start, given))
    })
}

fn property_span(class: &str, property: &str, start: usize) -> SymbolSpan {
    SymbolSpan {
        start: start as u32,
        end: (start + property.len()) as u32,
        kind: SymbolKind::MemberAccess {
            subject_text: SubjectText::owned(class.to_string()),
            member_name: atom(property),
            is_static: false,
            is_method_call: false,
            docblock_ref: DocblockMemberRef::No,
            is_array_callable: false,
            is_nullsafe: false,
        },
    }
}

/// The class reference a constraint written by its short name makes.
fn constraint_span(name: &str, start: usize) -> Option<SymbolSpan> {
    let is_short_name = name
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_uppercase())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    is_short_name.then(|| SymbolSpan {
        start: start as u32,
        end: (start + name.len()) as u32,
        kind: SymbolKind::ClassReference {
            name: atom(&format!("{CONSTRAINTS_NS}{name}")),
            is_fqn: true,
            context: ClassRefContext::Other,
        },
    })
}

/// The class of a constraint named `name` in a mapping: a class name, or
/// the short name of one of the Validator's own.  A name through an
/// aliased namespace (`custom:Name`) is left alone.
fn constraint_class(name: &str) -> Option<String> {
    if name.contains('\\') {
        return Some(crate::framework::normalize_framework_fqn(name));
    }
    (is_identifier(name) && name.bytes().next().is_some_and(|b| b.is_ascii_uppercase()))
        .then(|| format!("{CONSTRAINTS_NS}{name}"))
}

fn is_identifier(name: &str) -> bool {
    name.bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// The options a constraint takes: its constructor's parameters, which the
/// loader passes the options as named arguments, and its public
/// properties, which it falls back to setting from an options array when
/// the constructor takes no such parameter.
pub(crate) struct ConstraintOptions {
    /// The constructor's parameters in order, then the other properties.
    names: Vec<String>,
    /// Whether the constructor collects any named argument, being variadic.
    takes_any: bool,
}

impl ConstraintOptions {
    /// Whether the constraint takes an option `name`.  Every constraint
    /// takes `value`, which sets its default option.
    pub(crate) fn accepts(&self, name: &str) -> bool {
        self.takes_any || name == "value" || self.names.iter().any(|known| known == name)
    }

    /// The options to offer, leaving out the `options` array a constructor
    /// takes the deprecated options array in.
    pub(crate) fn offered(&self) -> impl Iterator<Item = &str> {
        self.names
            .iter()
            .map(String::as_str)
            .filter(|name| *name != "options")
    }
}

impl Backend {
    /// The options the constraint class `fqn` takes, or `None` when the
    /// class or a class it inherits from does not load, which would leave
    /// options out.
    pub(crate) fn constraint_options(&self, fqn: &str) -> Option<ConstraintOptions> {
        let loader = self.resolved_class_loader_with(&[]);
        let class = loader(Fqn::new(fqn))?;
        if !ancestors_load(&class, &loader) {
            return None;
        }
        let class = crate::virtual_members::resolve_class_fully_cached(
            &class,
            &loader,
            &self.resolved_class_cache,
        );
        let mut options = ConstraintOptions {
            names: Vec::new(),
            takes_any: false,
        };
        if let Some(constructor) = class.get_method("__construct") {
            for parameter in constructor.parameters.iter() {
                options.takes_any |= parameter.is_variadic;
                let name = parameter.name.strip_prefix('$').unwrap_or(&parameter.name);
                options.names.push(name.to_string());
            }
        }
        for property in &class.properties {
            if property.visibility == crate::types::Visibility::Public
                && !property.is_static
                && !property.is_virtual
                && !options
                    .names
                    .iter()
                    .any(|name| name == property.name.as_str())
            {
                options.names.push(property.name.to_string());
            }
        }
        Some(options)
    }
}

/// Whether every class `class` extends and every trait it or they use
/// loads, so that its members are all known.
pub(crate) fn ancestors_load(
    class: &ClassInfo,
    loader: &impl Fn(Fqn<'_>) -> Option<Arc<ClassInfo>>,
) -> bool {
    let mut pending: Vec<_> = class
        .parent_class
        .iter()
        .chain(&class.used_traits)
        .copied()
        .collect();
    let mut seen = Vec::new();
    while let Some(name) = pending.pop() {
        if seen.contains(&name) {
            continue;
        }
        seen.push(name);
        let Some(ancestor) = loader(Fqn::new(&name)) else {
            return false;
        };
        pending.extend(
            ancestor
                .parent_class
                .iter()
                .chain(&ancestor.used_traits)
                .copied(),
        );
    }
    true
}

// ─── YAML ───────────────────────────────────────────────────────────────────

/// Where in a class's mapping a line is.
enum YamlPlace {
    /// Directly in the class's mapping.
    Class,
    /// In `properties:` or `getters:`, whose own keys are members.
    Members {
        indent: usize,
        is_property: bool,
        member_indent: Option<usize>,
    },
    /// In one member's constraint list, or in the class's `constraints:`.
    Constraints {
        indent: usize,
        item_indent: Option<usize>,
    },
    /// In the block mapping of the constraint whose item's dash is at
    /// `indent`, the `options`-th of the scan's.
    Options { indent: usize, options: usize },
}

fn scan_yaml(content: &str, found: &mut Scan) {
    let mut class: Option<String> = None;
    // The sections a line is nested in, innermost last.
    let mut places: Vec<YamlPlace> = Vec::new();
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = crate::symfony::resources::yaml_content_before_comment(line);
        let trimmed = semantic.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = crate::framework::leading_spaces(semantic);

        while places.len() > 1
            && match places.last() {
                Some(YamlPlace::Members { indent: open, .. })
                | Some(YamlPlace::Constraints { indent: open, .. })
                | Some(YamlPlace::Options { indent: open, .. }) => indent <= *open,
                _ => false,
            }
        {
            if let Some(YamlPlace::Options { options, .. }) = places.pop()
                && let OptionSlot::Block { end, .. } = &mut found.options[options].slot
            {
                *end = line_start;
            }
        }
        if indent == 0 {
            places.clear();
            class = crate::symfony::resources::yaml_mapping_key(semantic, line_start).and_then(
                |(raw_key, _)| {
                    let (key, _) = crate::framework::strip_yaml_quotes(raw_key);
                    let class = crate::framework::normalize_framework_fqn(key);
                    class.contains('\\').then_some(class)
                },
            );
            if class.is_some() {
                places.push(YamlPlace::Class);
            }
            continue;
        }
        let Some(class) = &class else {
            continue;
        };

        match places.last_mut() {
            Some(YamlPlace::Class) => {
                let Some((raw_key, _)) =
                    crate::symfony::resources::yaml_mapping_key(semantic, line_start)
                else {
                    continue;
                };
                match crate::framework::strip_yaml_quotes(raw_key).0 {
                    section @ ("properties" | "getters") => places.push(YamlPlace::Members {
                        indent,
                        is_property: section == "properties",
                        member_indent: None,
                    }),
                    "constraints" => places.push(YamlPlace::Constraints {
                        indent,
                        item_indent: None,
                    }),
                    _ => {}
                }
            }
            Some(YamlPlace::Members {
                is_property,
                member_indent,
                ..
            }) => {
                if *member_indent.get_or_insert(indent) != indent {
                    continue;
                }
                let Some((raw_key, key_start)) =
                    crate::symfony::resources::yaml_mapping_key(semantic, line_start)
                else {
                    continue;
                };
                let (member, (quote, _)) = crate::framework::strip_yaml_quotes(raw_key);
                // A getter is mapped by its property-like name, which the
                // `get`, `is`, or `has` method it reads is named after
                // rather than spelled as.
                if *is_property && is_identifier(member) {
                    found
                        .spans
                        .push(property_span(class, member, key_start + quote));
                }
                places.push(YamlPlace::Constraints {
                    indent,
                    item_indent: None,
                });
            }
            Some(YamlPlace::Constraints { item_indent, .. }) => {
                if !trimmed.starts_with('-') || *item_indent.get_or_insert(indent) != indent {
                    continue;
                }
                let item = trimmed[1..].trim_start();
                let item_offset = indent + 1 + (trimmed[1..].len() - item.len());
                let (name, value) = match item.split_once(':') {
                    Some((name, value)) => (name.trim_end(), Some(value)),
                    None => (item, None),
                };
                let (name, (quote, _)) = crate::framework::strip_yaml_quotes(name);
                found
                    .spans
                    .extend(constraint_span(name, line_start + item_offset + quote));
                let (Some(constraint), Some(value)) = (constraint_class(name), value) else {
                    continue;
                };
                let options = value.trim_start();
                if options.is_empty() {
                    places.push(YamlPlace::Options {
                        indent,
                        options: found.options.len(),
                    });
                    found.options.push(ConstraintOptionKeys {
                        constraint,
                        keys: Vec::new(),
                        slot: OptionSlot::Block {
                            start: line_start + line.len(),
                            end: content.len(),
                            item_indent: indent,
                            key_indent: None,
                            is_list: false,
                        },
                    });
                } else if options.starts_with('{') {
                    let open = line_start + item_offset + (item.len() - options.len());
                    let (keys, close) = flow_mapping_keys(content, open);
                    found.options.push(ConstraintOptionKeys {
                        constraint,
                        keys,
                        slot: OptionSlot::Flow { open, close },
                    });
                }
            }
            Some(YamlPlace::Options { options, .. }) => {
                let options = &mut found.options[*options];
                let OptionSlot::Block {
                    key_indent,
                    is_list,
                    ..
                } = &mut options.slot
                else {
                    continue;
                };
                if key_indent.is_none() {
                    *is_list = trimmed.starts_with('-');
                }
                if *key_indent.get_or_insert(indent) != indent || trimmed.starts_with('-') {
                    continue;
                }
                if let Some((raw_key, key_start)) =
                    crate::symfony::resources::yaml_mapping_key(semantic, line_start)
                {
                    let (key, (quote, _)) = crate::framework::strip_yaml_quotes(raw_key);
                    options
                        .keys
                        .push((key_start + quote, key_start + quote + key.len()));
                }
            }
            None => {}
        }
    }
}

/// Where the quoted scalar opening at `at` ends, past its closing quote.
fn skip_quoted(bytes: &[u8], at: usize) -> usize {
    let quote = bytes[at];
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if quote == b'"' => i += 1,
            // A single-quoted scalar escapes its quote by doubling it,
            // which reads as a close and a reopen.
            byte if byte == quote => return i + 1,
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

/// Walk the YAML flow mapping opening at `open`, calling `entry` with the
/// range of each of its top-level entries' keys, and `at` with the walk's
/// state at each offset: whether a key is being written there, and where
/// the entry started.  Returns where the mapping closes, if it does.
fn walk_flow_mapping(
    content: &str,
    open: usize,
    mut entry: impl FnMut(usize, usize),
    mut at: impl FnMut(usize, bool, usize),
) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut entry_start = open + 1;
    let mut in_value = false;
    let mut i = open;
    while i < bytes.len() {
        at(i, depth == 1 && !in_value, entry_start);
        match bytes[i] {
            b'\'' | b'"' => {
                i = skip_quoted(bytes, i);
                continue;
            }
            b'#' if i > 0 && bytes[i - 1].is_ascii_whitespace() => {
                i = memchr::memchr(b'\n', &bytes[i..]).map_or(bytes.len(), |end| i + end);
                continue;
            }
            b'{' | b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if !in_value {
                        entry(entry_start, i);
                    }
                    return Some(i);
                }
            }
            b',' if depth == 1 => {
                if !in_value {
                    entry(entry_start, i);
                }
                entry_start = i + 1;
                in_value = false;
            }
            b':' if depth == 1 && !in_value => {
                entry(entry_start, i);
                in_value = true;
            }
            _ => {}
        }
        i += 1;
    }
    at(bytes.len(), depth == 1 && !in_value, entry_start);
    None
}

/// The keys of the YAML flow mapping opening at `open`, unquoted, and where
/// it closes, if it does.
fn flow_mapping_keys(content: &str, open: usize) -> (Vec<(usize, usize)>, Option<usize>) {
    let mut keys = Vec::new();
    let close = walk_flow_mapping(
        content,
        open,
        |start, end| {
            let raw = content[start..end].trim();
            if raw.is_empty() {
                return;
            }
            let (key, (quote, _)) = crate::framework::strip_yaml_quotes(raw);
            let key_start =
                start + (content[start..end].len() - content[start..end].trim_start().len());
            keys.push((key_start + quote, key_start + quote + key.len()));
        },
        |_, _, _| {},
    );
    (keys, close)
}

/// Where the key the cursor is writing in the YAML flow mapping opening at
/// `open` and closing at `close` starts, when the cursor is at a key.
fn flow_key_start(content: &str, open: usize, close: usize, cursor: usize) -> Option<usize> {
    if cursor <= open || cursor > close {
        return None;
    }
    let mut state = None;
    walk_flow_mapping(
        content,
        open,
        |_, _| {},
        |offset, at_key, entry_start| {
            if offset == cursor {
                state = Some((at_key, entry_start));
            }
        },
    );
    let (at_key, entry_start) = state?;
    let written = content[entry_start..cursor].trim_start();
    (at_key
        && written
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
    .then(|| cursor - written.len())
}

// ─── XML ────────────────────────────────────────────────────────────────────

fn scan_xml(content: &str, found: &mut Scan) {
    let bytes = content.as_bytes();
    let mut class: Option<String> = None;
    // The open `<constraint>` elements, innermost last, each with its
    // options' index in the scan when its class is known.
    let mut constraints: Vec<Option<usize>> = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = memchr::memchr(b'<', &bytes[cursor..]) {
        let tag_start = cursor + relative;
        if content[tag_start..].starts_with("<!--") {
            cursor = content[tag_start..]
                .find("-->")
                .map_or(bytes.len(), |end| tag_start + end + 3);
            continue;
        }
        // A tag still being written ends where the next one starts.
        let Some(tag_len) = memchr::memchr2(b'>', b'<', &bytes[tag_start + 1..]) else {
            break;
        };
        let tag_end = tag_start + 1 + tag_len;
        let tag_end = if bytes[tag_end] == b'>' {
            tag_end + 1
        } else {
            tag_end
        };
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
        match (name, closing) {
            ("class", false) => {
                class = attribute(&["name"])
                    .map(|(name, _, _)| crate::framework::normalize_framework_fqn(&name));
                constraints.clear();
            }
            ("class", true) => class = None,
            ("property", false) => {
                if let Some(class) = &class
                    && let Some((property, start, _)) = attribute(&["name"])
                    && is_identifier(&property)
                {
                    found.spans.push(property_span(class, &property, start));
                }
            }
            ("constraint", false) => {
                if class.is_none() {
                    continue;
                }
                let written = attribute(&["name"]);
                if let Some((constraint, start, _)) = &written {
                    found.spans.extend(constraint_span(constraint, *start));
                }
                if tag.ends_with("/>") {
                    continue;
                }
                let constraint = written.and_then(|(name, _, _)| constraint_class(&name));
                constraints.push(constraint.map(|constraint| {
                    found.options.push(ConstraintOptionKeys {
                        constraint,
                        keys: Vec::new(),
                        slot: OptionSlot::Xml,
                    });
                    found.options.len() - 1
                }));
            }
            ("constraint", true) => {
                constraints.pop();
            }
            ("option", false) => {
                if let Some(Some(options)) = constraints.last()
                    && let Some((_, start, end)) = attribute(&["name"])
                {
                    found.options[*options].keys.push((start, end));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(uri: &str, content: &str) -> Vec<(String, String)> {
        validation_spans(uri, content)
            .into_iter()
            .map(|span| {
                let text = content[span.start as usize..span.end as usize].to_string();
                let what = match span.kind {
                    SymbolKind::MemberAccess {
                        subject_text: SubjectText::Owned(class),
                        member_name,
                        ..
                    } => format!("{class}::${member_name}"),
                    SymbolKind::ClassReference { name, .. } => name.to_string(),
                    other => panic!("unexpected span {other:?}"),
                };
                (what, text)
            })
            .collect()
    }

    #[test]
    fn yaml_maps_properties_and_constraints() {
        let yaml = "App\\Entity\\User:\n    constraints:\n        - Callback: validate\n    properties:\n        email:\n            - NotBlank: ~\n            - Email:\n                message: x\n                groups:\n                    - Default\n        'displayName':\n            - App\\Validator\\Custom: ~\n            - Length: { min: 2 }\n    getters:\n        passwordSafe:\n            - IsTrue: ~\n";
        let found = found("file:///p/config/validator/User.yaml", yaml);
        let c = |name: &str| format!("{CONSTRAINTS_NS}{name}");
        assert_eq!(
            found
                .iter()
                .map(|(what, _)| what.clone())
                .collect::<Vec<_>>(),
            vec![
                c("Callback"),
                "App\\Entity\\User::$email".to_string(),
                c("NotBlank"),
                c("Email"),
                "App\\Entity\\User::$displayName".to_string(),
                c("Length"),
                c("IsTrue"),
            ]
        );
        for (what, text) in &found {
            assert!(
                what.ends_with(text.as_str()) || what.ends_with(&format!("${text}")),
                "{what} covers {text}"
            );
        }
    }

    #[test]
    fn xml_maps_properties_and_constraints() {
        let xml = r#"<?xml version="1.0"?>
<constraint-mapping>
    <class name="App\Entity\User">
        <constraint name="Callback">validate</constraint>
        <property name="email">
            <constraint name="NotBlank"/>
            <constraint name="App\Validator\Custom"/>
        </property>
    </class>
    <!-- <property name="commented"/> -->
</constraint-mapping>
"#;
        let found = found("file:///p/config/validator/User.xml", xml);
        assert_eq!(
            found,
            vec![
                (format!("{CONSTRAINTS_NS}Callback"), "Callback".to_string()),
                ("App\\Entity\\User::$email".to_string(), "email".to_string()),
                (format!("{CONSTRAINTS_NS}NotBlank"), "NotBlank".to_string()),
            ]
        );
    }

    /// Each judged constraint's class, short, with the keys it is given.
    fn keys(uri: &str, content: &str) -> Vec<(String, Vec<String>)> {
        constraint_option_keys(uri, content)
            .into_iter()
            .filter(ConstraintOptionKeys::is_judged)
            .map(|options| {
                let short = options.constraint.rsplit('\\').next().unwrap().to_string();
                let keys = options
                    .keys
                    .iter()
                    .map(|&(start, end)| content[start..end].to_string())
                    .collect();
                (short, keys)
            })
            .collect()
    }

    #[test]
    fn yaml_option_keys_are_read_from_flow_and_block_mappings() {
        let yaml = "App\\Entity\\User:\n    properties:\n        email:\n            - Length: { min: 2, 'max': 5, groups: [a, b], minMessage: \"x, y: z\" }  # min: 9\n            - Regex:\n                pattern: '/x/'\n                # match: false\n                groups:\n                - Default\n                normalizer: trim\n            - All:\n                - NotBlank: ~\n            - Choice: [a, b]\n            - Length: { min: 2\n        name:\n            - Length: { max: 1 }\n";
        assert_eq!(
            keys("file:///p/config/validator/User.yaml", yaml),
            vec![
                (
                    "Length".to_string(),
                    vec!["min", "max", "groups", "minMessage"]
                        .into_iter()
                        .map(String::from)
                        .collect()
                ),
                (
                    "Regex".to_string(),
                    vec!["pattern", "groups", "normalizer"]
                        .into_iter()
                        .map(String::from)
                        .collect()
                ),
                ("Length".to_string(), vec!["max".to_string()]),
            ]
        );
    }

    #[test]
    fn xml_options_belong_to_the_innermost_constraint() {
        let xml = r#"<constraint-mapping>
    <class name="App\Entity\User">
        <property name="tags">
            <constraint name="All">
                <option name="constraints">
                    <constraint name="Length">
                        <option name="min">2</option>
                    </constraint>
                    <constraint name="NotBlank"/>
                </option>
                <option name="groups"><value>Default</value></option>
            </constraint>
        </property>
    </class>
</constraint-mapping>
"#;
        assert_eq!(
            keys("file:///p/config/validator/User.xml", xml),
            vec![
                (
                    "All".to_string(),
                    vec!["constraints".to_string(), "groups".to_string()]
                ),
                ("Length".to_string(), vec!["min".to_string()]),
            ]
        );
    }

    #[test]
    fn the_key_being_written_is_found_with_the_others() {
        let uri = "file:///p/config/validator/User.yaml";
        let yaml = "App\\Entity\\User:\n    properties:\n        email:\n            - Length: { min: 2, ma }\n            - Length:\n                min: 2\n                ma\n";
        let at = |needle: &str, inside: usize| {
            option_key_at(uri, yaml, yaml.find(needle).unwrap() + inside)
                .map(|(class, start, given)| (class, &yaml[start..], given))
        };
        let length = format!("{CONSTRAINTS_NS}Length");
        assert_eq!(
            at("ma }", 2),
            Some((
                length.clone(),
                &yaml[yaml.find("ma }").unwrap()..],
                vec!["min".to_string()]
            ))
        );
        assert_eq!(
            at("{ min", 3).map(|(_, rest, given)| (&rest[..3], given)),
            Some(("min", vec!["ma".to_string()])),
            "inside a key already given"
        );
        assert_eq!(at("2, ma", 1), None, "a value");
        let written = yaml.rfind("ma\n").unwrap();
        assert_eq!(
            option_key_at(uri, yaml, written + 2),
            Some((length, written, vec!["min".to_string()]))
        );

        // An XML tag still being written ends where the next one starts.
        let xml = "<constraint-mapping>\n<class name=\"App\\User\">\n<constraint name=\"Length\">\n<option name=\"mi\"\n</constraint>\n<constraint name=\"NotBlank\"/>\n</class>\n</constraint-mapping>\n";
        let cursor = xml.find("\"mi\"").unwrap() + 3;
        assert_eq!(
            option_key_at("file:///p/config/validator/User.xml", xml, cursor)
                .map(|(class, start, _)| (class, start)),
            Some((format!("{CONSTRAINTS_NS}Length"), cursor - 2))
        );
    }

    #[test]
    fn mappings_are_recognised_by_their_location() {
        assert!(is_validation_mapping_uri(
            "file:///p/config/validator/User.yaml"
        ));
        assert!(is_validation_mapping_uri(
            "file:///p/src/Resources/config/validation.xml"
        ));
        assert!(is_validation_mapping_uri(
            "file:///p/src/Resources/config/validation/User.yaml"
        ));
        assert!(!is_validation_mapping_uri("file:///p/config/services.yaml"));
        assert!(!is_validation_mapping_uri("file:///p/validator/User.yaml"));
    }
}
