//! The keys a configuration object's schema declares: the `mapping` of its
//! schema entry, merged with the mappings of the named types it is built
//! on (`config_object` gives every object `_core` and `langcode`), one
//! level at a time, so `notifications.` offers what `notifications` maps.
//!
//! A key's span is keyed by the configuration name as well as the key
//! (see [`config_key`]): `enabled` only means something under its object.
//! The schema files are read when a key is first asked for, and kept with
//! the configuration index (see [`super::config::ConfigIndex`]), which a
//! changed schema file drops.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::php_type::PhpType;

/// The key of a [`crate::symbol_map::StringKeyKind::DrupalConfigKey`]
/// span: the key `path` of the configuration object `name`.
pub(crate) fn config_key(name: &str, path: &str) -> String {
    format!("{name}#{path}")
}

/// The configuration name and the key of a configuration key's span key.
pub(crate) fn split_config_key(key: &str) -> (&str, &str) {
    key.split_once('#').unwrap_or(("", key))
}

/// One `key: value` line of a schema file, with the lines nested under it.
#[derive(Debug, Clone)]
pub(crate) struct YamlNode {
    pub key: String,
    /// Byte offset of the key, quotes excluded.
    pub start: usize,
    pub value: String,
    pub children: Vec<YamlNode>,
}

impl YamlNode {
    fn child(&self, key: &str) -> Option<&YamlNode> {
        self.children.iter().find(|child| child.key == key)
    }

    /// The scalar a setting gives, without quotes.
    fn setting(&self, key: &str) -> Option<&str> {
        self.child(key)
            .map(|child| child.value.as_str())
            .filter(|value| !value.is_empty())
    }
}

/// The `key: value` lines of a YAML file as a tree, by indentation.  List
/// items, comments, and the text of block scalars are left out; the lines
/// nested under a list item are read as nested under the key before it,
/// which only matters to settings a schema reader never asks about.
pub(crate) fn yaml_tree(content: &str) -> Vec<YamlNode> {
    let mut lines = Vec::new();
    for (line_start, line) in crate::framework::line_offsets(content) {
        let semantic = crate::symfony::resources::yaml_content_before_comment(line);
        let indent = crate::framework::leading_spaces(semantic);
        let text = &semantic[indent..];
        if text.trim().is_empty() || text.starts_with('-') {
            continue;
        }
        let Some(colon) = super::config::yaml_key_end(text) else {
            continue;
        };
        let (key, (quote, _)) = crate::framework::strip_yaml_quotes(text[..colon].trim_end());
        let value = crate::framework::strip_yaml_quotes(text[colon + 1..].trim()).0;
        lines.push((indent, key, line_start + indent + quote, value));
    }
    let mut at = 0;
    nodes_at(&lines, &mut at, 0)
}

type Line<'a> = (usize, &'a str, usize, &'a str);

/// The nodes of the lines from `at` on, at indentation `indent` or deeper
/// for the first one, until a line less indented than it.
fn nodes_at(lines: &[Line<'_>], at: &mut usize, indent: usize) -> Vec<YamlNode> {
    let mut nodes = Vec::new();
    let Some(level) = lines
        .get(*at)
        .map(|line| line.0)
        .filter(|level| *level >= indent)
    else {
        return nodes;
    };
    while let Some(&(line_indent, key, start, value)) = lines.get(*at) {
        if line_indent < level {
            break;
        }
        if line_indent > level {
            // Nested under a list item or a block scalar.
            *at += 1;
            continue;
        }
        *at += 1;
        let children = match lines.get(*at) {
            Some(next) if next.0 > level => nodes_at(lines, at, next.0),
            _ => Vec::new(),
        };
        nodes.push(YamlNode {
            key: key.to_string(),
            start,
            value: value.to_string(),
            children,
        });
    }
    nodes
}

/// A key a configuration object's schema declares.
#[derive(Debug, Clone)]
pub(crate) struct SchemaKey {
    pub name: String,
    pub uri: String,
    /// The byte range of the key in its schema file.
    pub start: u32,
    pub end: u32,
    pub ty: Option<String>,
    pub label: Option<String>,
    node: YamlNode,
}

/// Whether a schema type is one a reader can follow to another entry: not
/// a mapping or a sequence spelled out in place, and not a placeholder
/// (`[%parent.type]`) only known per value.
fn is_named_type(ty: &str) -> bool {
    !matches!(ty, "mapping" | "sequence") && !ty.contains(['[', '%'])
}

impl Backend {
    /// The parsed schema file at `uri`, read once per configuration index.
    fn schema_tree(
        &self,
        index: &super::config::ConfigIndex,
        uri: &str,
    ) -> Option<Arc<Vec<YamlNode>>> {
        if let Some(tree) = index.trees.lock().get(uri) {
            return Some(Arc::clone(tree));
        }
        let content = self.get_file_content_arc(uri)?;
        let tree = Arc::new(yaml_tree(&content));
        index
            .trees
            .lock()
            .insert(uri.to_string(), Arc::clone(&tree));
        Some(tree)
    }

    /// The top-level node of the schema entry `key`.
    fn schema_node(
        &self,
        index: &super::config::ConfigIndex,
        key: &str,
    ) -> Option<(String, YamlNode)> {
        let entry = index.schema_named(key)?;
        let tree = self.schema_tree(index, &entry.uri)?;
        let node = tree
            .iter()
            .find(|node| node.start == entry.start as usize)?
            .clone();
        Some((entry.uri.clone(), node))
    }

    /// The keys `node`, written in the file at `uri`, maps: its own, then
    /// the ones of the named types it is built on, which it can override.
    fn keys_of(
        &self,
        index: &super::config::ConfigIndex,
        uri: &str,
        node: &YamlNode,
        seen: &mut Vec<String>,
    ) -> Vec<SchemaKey> {
        let mut keys: Vec<SchemaKey> = node
            .child("mapping")
            .map(|mapping| {
                mapping
                    .children
                    .iter()
                    .map(|child| SchemaKey {
                        name: child.key.clone(),
                        uri: uri.to_string(),
                        start: child.start as u32,
                        end: (child.start + child.key.len()) as u32,
                        ty: child.setting("type").map(str::to_string),
                        label: child.setting("label").map(str::to_string),
                        node: child.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(ty) = node.setting("type").filter(|ty| is_named_type(ty))
            && !seen.iter().any(|known| known == ty)
        {
            seen.push(ty.to_string());
            if let Some((base_uri, base)) = self.schema_node(index, ty) {
                for key in self.keys_of(index, &base_uri, &base, seen) {
                    if !keys.iter().any(|own| own.name == key.name) {
                        keys.push(key);
                    }
                }
            }
        }
        keys
    }

    /// The keys the schema of the configuration object `name` declares
    /// under the key `parent` (`""` for its top level, `notifications` for
    /// what `notifications.` offers), sorted by name.
    pub(crate) fn drupal_config_keys(&self, name: &str, parent: &str) -> Vec<SchemaKey> {
        let index = self.drupal_config_index();
        let Some((key, _)) = index.schema_entry(name) else {
            return Vec::new();
        };
        let Some((uri, node)) = self.schema_node(&index, &key) else {
            return Vec::new();
        };
        let mut seen = vec![key.clone()];
        let mut level = self.keys_of(&index, &uri, &node, &mut seen);
        for part in parent.split('.').filter(|part| !part.is_empty()) {
            let Some(found) = level.into_iter().find(|key| key.name == part) else {
                return Vec::new();
            };
            let mut seen = Vec::new();
            level = self.keys_of(&index, &found.uri, &found.node, &mut seen);
        }
        level.sort_by(|a, b| a.name.cmp(&b.name));
        level
    }

    /// The schema key a configuration key's span key names.
    fn drupal_config_key(&self, key: &str) -> Option<SchemaKey> {
        let (name, path) = split_config_key(key);
        let (parent, last) = path.rsplit_once('.').unwrap_or(("", path));
        self.drupal_config_keys(name, parent)
            .into_iter()
            .find(|key| key.name == last)
    }

    /// Where the schema declares the configuration key `key`.
    pub(crate) fn drupal_config_key_locations(&self, key: &str) -> Vec<Location> {
        let Some(found) = self.drupal_config_key(key) else {
            return Vec::new();
        };
        let (Some(uri), Some(content)) = (
            crate::util::parse_url(&found.uri),
            self.get_file_content_arc(&found.uri),
        ) else {
            return Vec::new();
        };
        let lines = crate::text_position::LineIndex::new(&content);
        vec![Location {
            uri,
            range: Range {
                start: lines.position(found.start as usize),
                end: lines.position(found.end as usize),
            },
        }]
    }

    /// The hover text of the configuration key `key`: its label, its type,
    /// and the configuration object it belongs to.
    pub(crate) fn drupal_config_key_detail(&self, key: &str) -> String {
        let (name, _) = split_config_key(key);
        let Some(found) = self.drupal_config_key(key) else {
            return format!("Not declared in the schema of `{name}`");
        };
        let mut parts = Vec::new();
        if let Some(label) = &found.label {
            parts.push(format!("**{label}**"));
        }
        if let Some(ty) = &found.ty {
            parts.push(format!("Type `{ty}`"));
        }
        parts.push(format!("Configuration `{name}`"));
        parts.join("\n\n")
    }

    /// The configuration names the type `ty` of a config object carries,
    /// when it may be written to: `set()` and `clear()` throw on an
    /// `ImmutableConfig`, so a type that can only be one carries none.
    pub(crate) fn drupal_config_names_of(
        &self,
        ty: &PhpType,
        editable: bool,
        class_loader: &crate::class_lookup::ResolvedClassLoader<'_>,
    ) -> Vec<String> {
        if editable && crate::class_lookup::is_subtype_of_named(ty, IMMUTABLE_CONFIG, class_loader)
        {
            return Vec::new();
        }
        super::types::config_names_of(ty)
    }
}

const IMMUTABLE_CONFIG: &str = "Drupal\\Core\\Config\\ImmutableConfig";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schema_reads_as_a_tree() {
        let yaml = "acme.settings:\n  type: config_object\n  label: 'Acme'\n  mapping:\n    notifications:\n      type: mapping\n      mapping:\n        email:\n          type: email\n    allowed:\n      type: sequence\n      sequence:\n        type: string\n      constraints:\n        Choice:\n          - a\n          - b\n    text:\n      type: text\n      label: >\n        Some: text\nother:\n  type: mapping\n";
        let tree = yaml_tree(yaml);
        assert_eq!(
            tree.iter()
                .map(|node| node.key.as_str())
                .collect::<Vec<_>>(),
            ["acme.settings", "other"]
        );
        let mapping = tree[0].child("mapping").unwrap();
        assert_eq!(
            mapping
                .children
                .iter()
                .map(|node| node.key.as_str())
                .collect::<Vec<_>>(),
            ["notifications", "allowed", "text"]
        );
        let email = &mapping.children[0].child("mapping").unwrap().children[0];
        assert_eq!(&yaml[email.start..email.start + 5], "email");
        assert_eq!(email.setting("type"), Some("email"));
        assert_eq!(tree[0].setting("label"), Some("Acme"));
    }

    #[test]
    fn a_key_names_its_object() {
        let key = config_key("acme.settings", "notifications.email");
        assert_eq!(
            split_config_key(&key),
            ("acme.settings", "notifications.email")
        );
    }
}
