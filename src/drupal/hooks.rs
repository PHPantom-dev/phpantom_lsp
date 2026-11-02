//! Drupal's hooks: the extensions that implement them, and what a PHP file
//! of a Drupal project can hold of them.
//!
//! A hook is documented by a `hook_<name>()` function in an `*.api.php`
//! file, which Drupal never loads, and implemented by a method a `#[Hook]`
//! attribute marks in an extension's `src/Hook`, or by a function named
//! `<extension>_<name>` in an extension's procedural files.  The
//! extensions come from their `*.info.yml`.

use std::path::{Path, PathBuf};

use crate::Backend;

/// One module, install profile, or theme, read from its `*.info.yml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Extension {
    /// The directory holding the `.info.yml`.
    pub dir: PathBuf,
    /// The machine name, from the file name.
    pub name: String,
    /// Themes implement hooks the theme manager invokes; modules and
    /// install profiles the ones the module handler does.
    pub is_theme: bool,
}

/// What a PHP file of a Drupal project holds of hooks, which only its path
/// says: the extraction is told before it reads the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum DrupalFile {
    /// Anything else: its hook classes, if any, are read by namespace.
    #[default]
    Other,
    /// An `*.api.php` file, whose `hook_*()` functions document hooks.
    Api,
    /// A file of a test module, which no real site enables: it neither
    /// documents nor implements a hook.
    Test,
}

/// Whether the file name at the end of `path` is an extension's
/// `*.info.yml`.
pub(crate) fn is_info_file_name(name: &str) -> bool {
    name.ends_with(".info.yml")
}

/// The extension an `*.info.yml` at `path` declares, from its `type:`
/// line; `None` for anything but a module, profile, or theme.
pub(crate) fn read_extension(path: &Path) -> Option<Extension> {
    let name = path
        .file_name()?
        .to_str()?
        .strip_suffix(".info.yml")?
        .to_string();
    let content = std::fs::read_to_string(path).ok()?;
    let kind = content.lines().find_map(|line| {
        let value = line.strip_prefix("type:")?.trim();
        let value = crate::framework::strip_yaml_quotes(value).0;
        Some(value.to_string())
    })?;
    let is_theme = match kind.as_str() {
        "module" | "profile" => false,
        "theme" => true,
        _ => return None,
    };
    Some(Extension {
        dir: path.parent()?.to_path_buf(),
        name,
        is_theme,
    })
}

/// The extension a hook class of namespace `Drupal\<extension>\Hook\…`
/// belongs to, which is where Drupal discovers hook classes.
pub(crate) fn hook_class_extension(fqn: &str) -> Option<&str> {
    let mut segments = fqn.trim_start_matches('\\').split('\\');
    (segments.next() == Some("Drupal")).then_some(())?;
    let extension = segments.next()?;
    (segments.next() == Some("Hook") && segments.next().is_some()).then_some(extension)
}

impl Backend {
    /// Remember the extensions the Drupal walk found.
    pub(crate) fn note_drupal_extensions(&self, extensions: Vec<Extension>) {
        self.drupal.extensions.write().extend(extensions);
    }

    /// What the PHP file at `uri` holds of hooks.
    pub(crate) fn drupal_file(&self, uri: &str) -> DrupalFile {
        if !self.is_drupal_project() {
            return DrupalFile::Other;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return DrupalFile::Other;
        };
        if self.is_drupal_test_path(&path) {
            return DrupalFile::Test;
        }
        let is_api = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".api.php"));
        if is_api {
            DrupalFile::Api
        } else {
            DrupalFile::Other
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_classes_belong_to_the_extension_of_their_namespace() {
        assert_eq!(
            hook_class_extension("Drupal\\acme\\Hook\\AcmeHooks"),
            Some("acme")
        );
        assert_eq!(
            hook_class_extension("\\Drupal\\acme\\Hook\\Form\\FormHooks"),
            Some("acme")
        );
        assert_eq!(hook_class_extension("Drupal\\acme\\Hooks"), None);
        assert_eq!(hook_class_extension("Drupal\\acme\\Form\\Hook"), None);
    }

    #[test]
    fn info_files_give_the_extension_and_its_kind() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, content: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, content).unwrap();
            path
        };
        let module = read_extension(&write("acme.info.yml", "name: Acme\ntype: module\n")).unwrap();
        assert_eq!((module.name.as_str(), module.is_theme), ("acme", false));
        let theme = read_extension(&write("olivero.info.yml", "type: 'theme'\n")).unwrap();
        assert!(theme.is_theme);
        assert!(read_extension(&write("engine.info.yml", "type: theme_engine\n")).is_none());
    }
}
