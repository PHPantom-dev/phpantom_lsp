//! Drupal's configurable fields, each declared by a field storage file,
//! `field.storage.<entity type>.<field>.yml`.
//!
//! Extensions ship them as default configuration (`config/install` and
//! `config/optional`), core's profiles included, and a site keeps the ones
//! it has in its exported configuration (`config/sync`, config split
//! folders), anywhere in the project.  Recipes and tests ship them too, for
//! sites that apply a recipe or run a test, so those are no source.  A field
//! is offered on every bundle of its entity type: `field.field.*` files,
//! which attach it to bundles, are not read.
//!
//! The workspace walk already reads every YAML file the project commits,
//! and the Drupal walk hands it core's and contributed extensions' field
//! storage files, so each one's symbol map carries what it declares (see
//! [`read_field_storage`]) into the field index (see [`super::base_fields`]).

use std::path::Path;

use super::config_keys::yaml_tree;
use crate::Backend;
use crate::symbol_map::FieldStorageSite;

/// Whether the file name `name` is a field storage file's:
/// `field.storage.<entity type>.<field>.yml`.
pub(crate) fn is_field_storage_file_name(name: &str) -> bool {
    name.strip_prefix("field.storage.")
        .and_then(|rest| rest.strip_suffix(".yml"))
        .and_then(|rest| rest.split_once('.'))
        .is_some_and(|(entity_type, field)| {
            !entity_type.is_empty() && !field.is_empty() && !field.contains('.')
        })
}

/// Whether the file at the end of `uri` is a field storage file.
pub(crate) fn is_field_storage_uri(uri: &str) -> bool {
    is_field_storage_file_name(uri.rsplit('/').next().unwrap_or(uri))
}

/// Whether `path` is a field storage file.
pub(crate) fn is_field_storage_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_field_storage_file_name)
}

/// The field a field storage file's `content` declares: its `entity_type`,
/// `field_name`, `type`, and `settings.target_type`.
pub(crate) fn read_field_storage(content: &str) -> Option<FieldStorageSite> {
    let tree = yaml_tree(content);
    let top = |key: &str| tree.iter().find(|node| node.key == key);
    let value = |key: &str| {
        top(key)
            .map(|node| node.value.as_str())
            .filter(|value| !value.is_empty())
    };
    let name = top("field_name").filter(|node| !node.value.is_empty())?;
    // The value starts after the key's colon, past any quote.
    let line = &content[name.start..];
    let line = &line[..line.find('\n').unwrap_or(line.len())];
    let start = name.start
        + line
            .find(':')
            .and_then(|colon| line[colon..].find(name.value.as_str()).map(|at| colon + at))
            .unwrap_or(0);
    Some(FieldStorageSite {
        entity_type: value("entity_type")?.to_string(),
        field_name: name.value.clone(),
        field_type: value("type")?.to_string(),
        target_type: top("settings")
            .and_then(|settings| {
                settings
                    .children
                    .iter()
                    .find(|node| node.key == "target_type")
            })
            .map(|node| node.value.clone())
            .filter(|value| !value.is_empty()),
        start: start as u32,
        end: (start + name.value.len()) as u32,
    })
}

/// Whether a field storage file at `relative`, relative to the web root
/// when `in_web_root` and to the workspace otherwise, declares a field of
/// the site: not a recipe's, not a test's unless `is_open`, and inside the
/// folders Composer installs only as an extension's default configuration.
fn counts(relative: &Path, in_web_root: bool, is_open: bool) -> bool {
    let mut parts = relative.components().map(|part| part.as_os_str());
    if parts.clone().any(|part| part == "recipes") {
        return false;
    }
    if !is_open && parts.any(|part| part == "tests") {
        return false;
    }
    if !in_web_root {
        return true;
    }
    if super::UNLOADED_DIRS
        .iter()
        .any(|dir| relative.starts_with(dir))
    {
        return false;
    }
    !super::READ_ONLY_DIRS
        .iter()
        .any(|dir| relative.starts_with(dir))
        || super::config::has_default_config_shape(relative)
}

impl Backend {
    /// Whether the field storage file at `uri` declares a field of the
    /// site: one in the workspace or a web root, outside its dependencies,
    /// that [`counts`].
    pub(crate) fn is_drupal_field_storage_source(&self, uri: &str) -> bool {
        if self
            .workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
        {
            return false;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return false;
        };
        let is_open = || self.open_files.read().contains_key(uri);
        if let Some(root) = self
            .drupal
            .web_roots
            .read()
            .iter()
            .find(|root| path.starts_with(root))
        {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            return counts(relative, true, is_open());
        }
        let workspace = self.workspace.workspace_root.read().clone();
        workspace.is_some_and(|workspace| {
            path.strip_prefix(&workspace)
                .is_ok_and(|relative| counts(relative, false, is_open()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_storage_files_are_found_by_name() {
        assert!(is_field_storage_file_name(
            "field.storage.node.field_tags.yml"
        ));
        for name in [
            "field.field.node.article.field_tags.yml",
            "field.storage.node.yml",
            "field.storage.node.field_tags.extra.yml",
            "field.storage..field_tags.yml",
            "field.storage.node.field_tags.yaml",
        ] {
            assert!(!is_field_storage_file_name(name), "{name}");
        }
        assert!(is_field_storage_uri(
            "file:///site/config/sync/field.storage.node.field_tags.yml"
        ));
    }

    #[test]
    fn a_storage_gives_its_entity_type_name_type_and_target() {
        let content = "uuid: 0d4c5b0e\nlangcode: en\ndependencies:\n  module:\n    - node\n    - taxonomy\nid: node.field_tags\nfield_name: field_tags\nentity_type: node\ntype: entity_reference\nsettings:\n  target_type: taxonomy_term\nmodule: core\n";
        let site = read_field_storage(content).expect("a field");
        assert_eq!(site.entity_type, "node");
        assert_eq!(site.field_name, "field_tags");
        assert_eq!(site.field_type, "entity_reference");
        assert_eq!(site.target_type.as_deref(), Some("taxonomy_term"));
        assert_eq!(
            &content[site.start as usize..site.end as usize],
            "field_tags"
        );
        assert!(content[..site.start as usize].ends_with("field_name: "));

        let quoted = "field_name: 'field_body'\nentity_type: node\ntype: text_with_summary\nsettings: {  }\n";
        let site = read_field_storage(quoted).expect("a field");
        assert_eq!(site.target_type, None);
        assert_eq!(
            &quoted[site.start as usize..site.end as usize],
            "field_body"
        );
        assert!(read_field_storage("field_name: field_tags\ntype: string\n").is_none());
    }

    /// An export committed from Windows has CRLF line endings.
    #[test]
    fn a_storage_with_crlf_line_endings_reads_the_same() {
        let content = "id: node.field_tags\r\nfield_name: field_tags\r\nentity_type: node\r\ntype: entity_reference\r\nsettings:\r\n  target_type: taxonomy_term\r\n";
        let site = read_field_storage(content).expect("a field");
        assert_eq!(site.field_type, "entity_reference");
        assert_eq!(site.target_type.as_deref(), Some("taxonomy_term"));
        assert_eq!(
            &content[site.start as usize..site.end as usize],
            "field_tags"
        );
    }

    #[test]
    fn storages_count_outside_recipes_tests_and_installed_code() {
        let web = |path: &str, open: bool| counts(Path::new(path), true, open);
        let project = |path: &str, open: bool| counts(Path::new(path), false, open);
        assert!(project(
            "config/sync/field.storage.node.field_tags.yml",
            false
        ));
        assert!(project(
            "config/split/dev/field.storage.node.field_tags.yml",
            false
        ));
        assert!(web(
            "modules/custom/acme/config/install/field.storage.acme_note.field_tags.yml",
            false
        ));
        assert!(web(
            "core/profiles/standard/config/install/field.storage.user.user_picture.yml",
            false
        ));
        assert!(web(
            "modules/contrib/example/config/optional/field.storage.node.field_example.yml",
            false
        ));
        assert!(!web(
            "core/recipes/article_tags/config/field.storage.node.field_tags.yml",
            false
        ));
        assert!(!project(
            "recipes/example/config/field.storage.node.field_tags.yml",
            false
        ));
        assert!(!web(
            "core/modules/node/tests/modules/node_test/config/install/field.storage.node.field_test.yml",
            false
        ));
        assert!(web(
            "core/modules/node/tests/modules/node_test/config/install/field.storage.node.field_test.yml",
            true
        ));
        assert!(!project(
            "tests/fixtures/field.storage.node.field_test.yml",
            false
        ));
        assert!(!web("core/assets/field.storage.node.field_tags.yml", false));
        assert!(!web(
            "modules/contrib/example/field.storage.node.field_example.yml",
            false
        ));
    }
}
