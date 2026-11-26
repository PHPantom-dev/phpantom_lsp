//! The fields a Doctrine XML or YAML mapping file maps.
//!
//! The attribute mapping is read with the class (see
//! [`DoctrineMapping`](crate::types::DoctrineMapping)); a mapping file maps
//! a class from the outside, so its fields are kept here, per document,
//! for the class to be looked up by name.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::framework::{
    leading_spaces, line_offsets, normalize_framework_fqn, strip_yaml_quotes, xml_attr_value,
};
use crate::symbol_map::SymbolSpan;

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
    /// The associations among `fields`, each with the entity it targets,
    /// fully qualified, when the file names one.
    pub(crate) associations: Vec<(String, Option<String>)>,
}

/// The classes each mapping file maps, keyed by the file's URI.
pub(crate) type MappingFileIndex = Arc<RwLock<HashMap<String, Arc<Vec<MappedClass>>>>>;

pub(crate) fn new_mapping_file_index() -> MappingFileIndex {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Record the classes `content` maps under `uri`, or forget the document
/// when it maps none.  Returns whether that changed what the index holds.
/// When `spans` is given, the spans of the PHP properties an XML mapping
/// names are added to it.
pub(crate) fn index_mapping_file(
    index: &MappingFileIndex,
    uri: &str,
    content: &str,
    spans: Option<&mut Vec<SymbolSpan>>,
) -> bool {
    let classes = scan_mapping_file(uri, content, spans);
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

/// Whether a mapping file maps `field` of `fqn` as an association, and the
/// entity it targets when the file names one.  `None` when no file maps
/// the field as an association.
pub(crate) fn mapped_association(
    index: &MappingFileIndex,
    fqn: &str,
    field: &str,
) -> Option<Option<String>> {
    let fqn = fqn.strip_prefix('\\').unwrap_or(fqn);
    let mut found = None;
    for classes in index.read().values() {
        for class in classes.iter().filter(|c| c.fqn.eq_ignore_ascii_case(fqn)) {
            for (name, target) in &class.associations {
                if name == field {
                    if target.is_some() {
                        return Some(target.clone());
                    }
                    found = Some(None);
                }
            }
        }
    }
    found
}

const XML_SUFFIXES: &[&str] = &[".orm.xml", ".dcm.xml"];
const YAML_SUFFIXES: &[&str] = &[".orm.yml", ".orm.yaml", ".dcm.yml", ".dcm.yaml"];

fn has_suffix(path: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|suffix| {
        path.len() >= suffix.len()
            && path.as_bytes()[path.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
    })
}

/// Whether the file at `path` is named like a Doctrine XML or YAML mapping.
pub(crate) fn is_mapping_file_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| has_suffix(name, XML_SUFFIXES) || has_suffix(name, YAML_SUFFIXES))
}

fn scan_mapping_file(
    uri: &str,
    content: &str,
    spans: Option<&mut Vec<SymbolSpan>>,
) -> Vec<MappedClass> {
    if has_suffix(uri, XML_SUFFIXES) {
        scan_xml(content, spans)
    } else if has_suffix(uri, YAML_SUFFIXES) {
        scan_yaml(content)
    } else {
        Vec::new()
    }
}

impl crate::Backend {
    /// Read the workspace's Doctrine mapping files, ahead of the
    /// background workspace index.
    ///
    /// That index reads them with the other resource files, after every
    /// PHP file is parsed, and until then an entity mapped only by a file
    /// has no fields: its repository no finders, a DQL query on it no
    /// result type.  Run during startup, before the open files are first
    /// diagnosed, so nothing is ever resolved without them.
    pub(crate) fn index_doctrine_mapping_files(&self) {
        if !self.is_doctrine_project() {
            return;
        }
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return;
        };
        let vendor_dir_paths = self.workspace.vendor_dir_paths.lock().clone();
        let mut paths = Vec::new();
        crate::classmap_scanner::visit_workspace_files_gitignore(
            &root,
            &vendor_dir_paths,
            &self.index_filters(),
            Some(self.followed_links()),
            |path| {
                if is_mapping_file_path(path) {
                    paths.push(path.to_path_buf());
                }
            },
        );
        for path in paths {
            let uri = crate::util::path_to_uri(&path);
            let content = match self.open_buffer_at_path(&path) {
                Some(content) => content,
                None => match crate::util::read_file_to_string(&path) {
                    Ok(content) => Arc::new(content),
                    Err(_) => continue,
                },
            };
            self.index_framework_uri_content(&uri, &content);
        }
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

fn scan_xml(content: &str, mut spans: Option<&mut Vec<SymbolSpan>>) -> Vec<MappedClass> {
    let mut classes = Vec::new();
    let mut current: Option<(usize, MappedClass)> = None;
    let mut search = 0;
    while let Some(rel) = content[search..].find('<') {
        let start = search + rel;
        if content[start..].starts_with("<!--") {
            search = content[start + 4..]
                .find("-->")
                .map_or(content.len(), |at| start + 4 + at + 3);
            continue;
        }
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
                associations: Vec::new(),
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
            && let Some((field, field_start, _)) = xml_attr_value(tag, start + 1, &[attr])
        {
            if let Some(out) = spans.as_deref_mut() {
                out.push(crate::resource_navigation::property_span(
                    &class.fqn,
                    &field,
                    field_start,
                ));
            }
            if attr == "field" {
                let target = xml_attr_value(tag, 0, &["target-entity"])
                    .map(|(target, _, _)| qualify_target(&class.fqn, &target));
                // The other side of a bidirectional association names its
                // property on the target entity, so a rename must reach it.
                if let (Some(out), Some(target)) = (spans.as_deref_mut(), target.as_deref()) {
                    for side in ["mapped-by", "inversed-by"] {
                        if let Some((property, property_start, _)) =
                            xml_attr_value(tag, start + 1, &[side])
                        {
                            out.push(crate::resource_navigation::property_span(
                                target,
                                &property,
                                property_start,
                            ));
                        }
                    }
                }
                class.associations.push((field.clone(), target));
            }
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
        let fqn = normalize_framework_fqn(key);
        let mut kind = None;
        let mut fields = Vec::new();
        let mut associations: Vec<(String, Option<String>)> = Vec::new();
        let mut section_indent = None;
        let mut in_field_section = false;
        let mut in_associations = false;
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
                    in_associations = !matches!(child, "id" | "fields");
                }
            } else if !in_field_section {
                continue;
            } else if indent == *field_indent.get_or_insert(indent) {
                if let Some(field) = yaml_key(line) {
                    fields.push(field.to_string());
                    if in_associations {
                        associations.push((field.to_string(), None));
                    }
                }
            } else if in_associations
                && indent > field_indent.unwrap_or(indent)
                && yaml_key(line) == Some("targetEntity")
                && let Some(target) = yaml_value(line)
                && let Some((_, known)) = associations.last_mut()
            {
                *known = Some(qualify_target(&fqn, target));
            }
        }
        let is_entity = match kind {
            Some("entity") => true,
            Some("mappedSuperclass" | "embeddable") => false,
            _ => continue,
        };
        classes.push(MappedClass {
            fqn,
            is_entity,
            fields,
            associations,
        });
    }
    classes
}

/// The entity a mapping of `owner` names as an association's target: a
/// name without a `\` is a class of the owner's namespace, as Doctrine's
/// `ClassMetadata::fullyQualifiedClassName()` reads it.
fn qualify_target(owner: &str, target: &str) -> String {
    let qualified = normalize_framework_fqn(target);
    match owner.rsplit_once('\\') {
        Some((namespace, _)) if !target.contains('\\') => format!("{namespace}\\{qualified}"),
        _ => qualified,
    }
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
    <one-to-many field="posts" target-entity="Post" mapped-by="author"/>
    <order-by-field name="ignored"/>
    <embedded name="address" class="App\Entity\Address"/>
  </entity>
  <mapped-superclass name="App\Entity\Base">
    <field name="createdAt" type="datetime"/>
  </mapped-superclass>
</doctrine-mapping>"#;
        let classes = scan_mapping_file("file:///p/config/doctrine/User.orm.xml", xml, None);
        assert_eq!(
            classes,
            vec![
                MappedClass {
                    fqn: "App\\Entity\\User".to_string(),
                    is_entity: true,
                    fields: vec!["id".into(), "email".into(), "team".into(), "posts".into()],
                    associations: vec![
                        ("team".into(), Some("App\\Entity\\Team".into())),
                        ("posts".into(), Some("App\\Entity\\Post".into())),
                    ],
                },
                MappedClass {
                    fqn: "App\\Entity\\Base".to_string(),
                    is_entity: false,
                    fields: vec!["createdAt".into()],
                    associations: Vec::new(),
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
        owner:
            inversedBy: teams
    lifecycleCallbacks:
        prePersist: [touch]
";
        let classes = scan_mapping_file("file:///p/config/doctrine/User.orm.yml", yaml, None);
        assert_eq!(
            classes,
            vec![MappedClass {
                fqn: "App\\Entity\\User".to_string(),
                is_entity: true,
                fields: vec!["id".into(), "email".into(), "team".into(), "owner".into()],
                associations: vec![
                    ("team".into(), Some("App\\Entity\\Team".into())),
                    ("owner".into(), None),
                ],
            }]
        );
    }

    #[test]
    fn xml_mapping_names_the_properties_of_both_association_sides() {
        let xml = r#"<doctrine-mapping>
  <entity name="App\Entity\User">
    <field name="email"/>
    <many-to-one field="team" target-entity="Team" inversed-by="members"/>
  </entity>
  <entity name="App\Entity\Team">
    <one-to-many field="members" target-entity="User" mapped-by="team"/>
  </entity>
</doctrine-mapping>"#;
        let mut spans = Vec::new();
        scan_mapping_file(
            "file:///p/config/doctrine/User.orm.xml",
            xml,
            Some(&mut spans),
        );
        let named: Vec<(String, String)> = spans
            .iter()
            .map(|span| {
                let crate::symbol_map::SymbolKind::MemberAccess {
                    subject_text,
                    member_name,
                    ..
                } = &span.kind
                else {
                    panic!("not a property: {span:?}");
                };
                let text = &xml[span.start as usize..span.end as usize];
                assert_eq!(text, member_name.as_str());
                (
                    subject_text.synthesised().unwrap().to_string(),
                    text.to_string(),
                )
            })
            .collect();
        assert_eq!(
            named,
            [
                ("App\\Entity\\User", "email"),
                ("App\\Entity\\User", "team"),
                ("App\\Entity\\Team", "members"),
                ("App\\Entity\\Team", "members"),
                ("App\\Entity\\User", "team"),
            ]
            .map(|(class, property)| (class.to_string(), property.to_string()))
        );
    }

    #[test]
    fn other_documents_map_nothing() {
        let yaml = "App\\Service\\Mailer:\n    type: entity\n";
        assert!(scan_mapping_file("file:///p/config/services.yaml", yaml, None).is_empty());
    }
}
