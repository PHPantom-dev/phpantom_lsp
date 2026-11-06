//! The fields a Doctrine XML or YAML mapping file maps.
//!
//! The attribute mapping is read with the class (see
//! [`DoctrineMapping`](crate::types::DoctrineMapping)); a mapping file maps
//! a class from the outside, so its fields are kept here, per document,
//! for the class to be looked up by name.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::framework::{
    leading_spaces, line_offsets, normalize_framework_fqn, strip_yaml_quotes, xml_attr_value,
};

/// A class a mapping file maps, and the fields it maps on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MappedClass {
    /// The class's fully-qualified name, without a leading `\`.
    pub(crate) fqn: String,
    /// Whether the class is mapped as an entity, which makes the file its
    /// whole mapping, rather than as a mapped superclass or an embeddable.
    pub(crate) is_entity: bool,
    /// The fields, identifiers, and associations the file maps.
    pub(crate) fields: Vec<String>,
}

/// The classes each mapping file maps, keyed by the file's URI.
pub(crate) type MappingFileIndex = Arc<RwLock<HashMap<String, Arc<Vec<MappedClass>>>>>;

pub(crate) fn new_mapping_file_index() -> MappingFileIndex {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Record the classes `content` maps under `uri`, or forget the document
/// when it maps none.  Returns whether that changed what the index holds.
pub(crate) fn index_mapping_file(index: &MappingFileIndex, uri: &str, content: &str) -> bool {
    let classes = scan_mapping_file(uri, content);
    let mut index = index.write();
    if classes.is_empty() {
        return index.remove(uri).is_some();
    }
    if index.get(uri).is_some_and(|known| **known == classes) {
        return false;
    }
    index.insert(uri.to_string(), Arc::new(classes));
    true
}

/// The fields every mapping file maps on `fqn`, and whether one of them
/// maps it as an entity.  `None` when no file maps it.
pub(crate) fn mapped_fields(index: &MappingFileIndex, fqn: &str) -> Option<(bool, Vec<String>)> {
    let fqn = fqn.strip_prefix('\\').unwrap_or(fqn);
    let mut found: Option<(bool, Vec<String>)> = None;
    for classes in index.read().values() {
        for class in classes.iter().filter(|c| c.fqn.eq_ignore_ascii_case(fqn)) {
            let (is_entity, fields) = found.get_or_insert_with(|| (false, Vec::new()));
            *is_entity |= class.is_entity;
            fields.extend(class.fields.iter().cloned());
        }
    }
    found
}

fn scan_mapping_file(uri: &str, content: &str) -> Vec<MappedClass> {
    let path = uri.to_ascii_lowercase();
    if path.ends_with(".orm.xml") || path.ends_with(".dcm.xml") {
        scan_xml(content)
    } else if [".orm.yml", ".orm.yaml", ".dcm.yml", ".dcm.yaml"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
    {
        scan_yaml(content)
    } else {
        Vec::new()
    }
}

/// The class elements of an XML mapping, with the field kinds they hold.
const XML_CLASS_TAGS: &[(&str, bool)] = &[
    ("entity", true),
    ("mapped-superclass", false),
    ("embeddable", false),
];

/// The elements that map a field, and the attribute naming it.
const XML_FIELD_TAGS: &[(&str, &str)] = &[
    ("field", "name"),
    ("id", "name"),
    ("many-to-one", "field"),
    ("one-to-one", "field"),
    ("one-to-many", "field"),
    ("many-to-many", "field"),
];

fn scan_xml(content: &str) -> Vec<MappedClass> {
    let mut classes = Vec::new();
    let mut current: Option<(usize, MappedClass)> = None;
    let mut search = 0;
    while let Some(rel) = content[search..].find('<') {
        let start = search + rel;
        let Some(rel_end) = content[start..].find('>') else {
            break;
        };
        let end = start + rel_end + 1;
        search = end;
        let tag = &content[start + 1..end - 1];
        let (closing, tag) = match tag.strip_prefix('/') {
            Some(rest) => (true, rest),
            None => (false, tag),
        };
        let name_len = tag
            .find(|c: char| c.is_ascii_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let name = &tag[..name_len];
        let self_closing = tag.ends_with('/');

        if let Some(&(_, is_entity)) = XML_CLASS_TAGS.iter().find(|(t, _)| *t == name) {
            if closing {
                if let Some((_, class)) = current.take() {
                    classes.push(class);
                }
                continue;
            }
            if let Some((_, class)) = current.take() {
                classes.push(class);
            }
            let Some((fqn, _, _)) = xml_attr_value(tag, 0, &["name"]) else {
                continue;
            };
            let class = MappedClass {
                fqn: normalize_framework_fqn(&fqn),
                is_entity,
                fields: Vec::new(),
            };
            if self_closing {
                classes.push(class);
            } else {
                current = Some((start, class));
            }
            continue;
        }
        if closing {
            continue;
        }
        if let Some((_, class)) = current.as_mut()
            && let Some(&(_, attr)) = XML_FIELD_TAGS.iter().find(|(t, _)| *t == name)
            && let Some((field, _, _)) = xml_attr_value(tag, 0, &[attr])
        {
            class.fields.push(field);
        }
    }
    if let Some((_, class)) = current {
        classes.push(class);
    }
    classes
}

/// The keys of a YAML class mapping whose children are fields.
const YAML_FIELD_SECTIONS: &[&str] = &[
    "id",
    "fields",
    "manyToOne",
    "oneToOne",
    "oneToMany",
    "manyToMany",
];

/// A YAML mapping (Doctrine ORM 2) maps each class under a top-level key
/// naming it, with a `type` of `entity`, `mappedSuperclass`, or
/// `embeddable`, and its fields as the keys of the sections above.
fn scan_yaml(content: &str) -> Vec<MappedClass> {
    let lines: Vec<(usize, &str)> = line_offsets(content)
        .into_iter()
        .filter(|(_, line)| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .collect();
    let mut classes = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (_, line) = lines[i];
        i += 1;
        if leading_spaces(line) != 0 {
            continue;
        }
        let Some(key) = yaml_key(line) else {
            continue;
        };
        let mut kind = None;
        let mut fields = Vec::new();
        let mut section_indent = None;
        let mut in_field_section = false;
        let mut field_indent = None;
        while i < lines.len() && leading_spaces(lines[i].1) > 0 {
            let line = lines[i].1;
            i += 1;
            let indent = leading_spaces(line);
            let section = *section_indent.get_or_insert(indent);
            if indent == section {
                in_field_section = false;
                field_indent = None;
                let Some(child) = yaml_key(line) else {
                    continue;
                };
                if child == "type" {
                    kind = yaml_value(line);
                } else if YAML_FIELD_SECTIONS.contains(&child) {
                    in_field_section = true;
                }
            } else if in_field_section
                && indent == *field_indent.get_or_insert(indent)
                && let Some(field) = yaml_key(line)
            {
                fields.push(field.to_string());
            }
        }
        let is_entity = match kind {
            Some("entity") => true,
            Some("mappedSuperclass" | "embeddable") => false,
            _ => continue,
        };
        classes.push(MappedClass {
            fqn: normalize_framework_fqn(key),
            is_entity,
            fields,
        });
    }
    classes
}

/// The key a YAML mapping line opens, unquoted.
fn yaml_key(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.starts_with('-') {
        return None;
    }
    let colon = trimmed.find(':')?;
    let (key, _) = strip_yaml_quotes(trimmed[..colon].trim());
    (!key.is_empty()).then_some(key)
}

/// The scalar value a YAML mapping line holds, unquoted.
fn yaml_value(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let colon = trimmed.find(':')?;
    let raw = trimmed[colon + 1..].split('#').next()?.trim();
    let (value, _) = strip_yaml_quotes(raw);
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_mapping_lists_fields_identifiers_and_associations() {
        let xml = r#"<?xml version="1.0"?>
<doctrine-mapping>
  <entity name="App\Entity\User" repository-class="App\Repository\UserRepository">
    <id name="id" type="integer"><generator strategy="AUTO"/></id>
    <field name="email" type="string"/>
    <many-to-one field="team" target-entity="App\Entity\Team"/>
    <order-by-field name="ignored"/>
    <embedded name="address" class="App\Entity\Address"/>
  </entity>
  <mapped-superclass name="App\Entity\Base">
    <field name="createdAt" type="datetime"/>
  </mapped-superclass>
</doctrine-mapping>"#;
        let classes = scan_mapping_file("file:///p/config/doctrine/User.orm.xml", xml);
        assert_eq!(
            classes,
            vec![
                MappedClass {
                    fqn: "App\\Entity\\User".to_string(),
                    is_entity: true,
                    fields: vec!["id".into(), "email".into(), "team".into()],
                },
                MappedClass {
                    fqn: "App\\Entity\\Base".to_string(),
                    is_entity: false,
                    fields: vec!["createdAt".into()],
                },
            ]
        );
    }

    #[test]
    fn yaml_mapping_lists_the_keys_of_its_field_sections() {
        let yaml = "\
App\\Entity\\User:
    type: entity
    repositoryClass: App\\Repository\\UserRepository
    id:
        id:
            type: integer
    fields:
        email:
            type: string
    manyToOne:
        team:
            targetEntity: Team
    lifecycleCallbacks:
        prePersist: [touch]
";
        let classes = scan_mapping_file("file:///p/config/doctrine/User.orm.yml", yaml);
        assert_eq!(
            classes,
            vec![MappedClass {
                fqn: "App\\Entity\\User".to_string(),
                is_entity: true,
                fields: vec!["id".into(), "email".into(), "team".into()],
            }]
        );
    }

    #[test]
    fn other_documents_map_nothing() {
        let yaml = "App\\Service\\Mailer:\n    type: entity\n";
        assert!(scan_mapping_file("file:///p/config/services.yaml", yaml).is_empty());
    }
}
