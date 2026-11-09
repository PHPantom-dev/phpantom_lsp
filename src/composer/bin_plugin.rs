//! Tools installed with [`bamarni/composer-bin-plugin`](https://github.com/bamarni/composer-bin-plugin).
//!
//! The plugin installs each development tool into a Composer project of
//! its own, a *bin namespace* under `vendor-bin/<name>/`, so tools whose
//! constraints clash stay apart.  The installing project's code uses those
//! tools all the same (a test suite extends PHPUnit's `TestCase` from
//! `vendor-bin/phpunit/vendor/`), so a bin namespace's vendor directory is
//! one more vendor directory of the project that requires the plugin, not
//! a nested project of its own.

use std::path::{Path, PathBuf};

use mago_composer::{ComposerPackage, ComposerPackageExtra};

use super::{get_vendor_dir, has_dependency, read_composer_package};

/// The plugin's package name.
const BIN_PLUGIN: &str = "bamarni/composer-bin-plugin";

/// The bin directory the plugin uses when `extra.bamarni-bin` names none.
const DEFAULT_TARGET_DIRECTORY: &str = "vendor-bin";

/// One bin namespace: a directory directly under the bin directory that
/// holds a `composer.json`.
pub(crate) struct BinNamespace {
    /// The directory holding the namespace's `composer.json`.
    pub(crate) root: PathBuf,
    pub(crate) package: Option<ComposerPackage>,
    /// The namespace's vendor directory as its `composer.json` spells it.
    pub(crate) vendor_dir: String,
}

impl BinNamespace {
    /// The namespace's vendor directory as an absolute path.
    pub(crate) fn vendor_path(&self) -> PathBuf {
        self.root.join(&self.vendor_dir)
    }
}

/// The bin directory of the project at `project_root`, or `None` when the
/// project does not require the plugin.
pub(crate) fn bin_target_directory(
    project_root: &Path,
    package: &ComposerPackage,
) -> Option<PathBuf> {
    if !has_dependency(package, BIN_PLUGIN) {
        return None;
    }
    let configured = match &package.extra {
        Some(ComposerPackageExtra::Object(extra)) => extra
            .get("bamarni-bin")
            .and_then(|config| config.get("target-directory"))
            .and_then(serde_json::Value::as_str)
            .map(|dir| dir.trim_end_matches('/'))
            .filter(|dir| !dir.is_empty()),
        _ => None,
    };
    Some(project_root.join(configured.unwrap_or(DEFAULT_TARGET_DIRECTORY)))
}

/// The bin namespaces of the project at `project_root`, in the order the
/// plugin visits them (alphabetical), which is the order their classes
/// win in when two namespaces install the same one.
pub(crate) fn bin_namespaces(project_root: &Path, package: &ComposerPackage) -> Vec<BinNamespace> {
    let Some(target) = bin_target_directory(project_root, package) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&target) else {
        return Vec::new();
    };
    // The plugin globs `<target>/*`, which leaves hidden directories out.
    let mut roots: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .filter(|dir| dir.join("composer.json").is_file())
        .collect();
    roots.sort();
    roots
        .into_iter()
        .map(|root| {
            let package = read_composer_package(&root);
            let vendor_dir = package
                .as_ref()
                .map(get_vendor_dir)
                .unwrap_or_else(|| "vendor".to_string());
            BinNamespace {
                root,
                package,
                vendor_dir,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn pkg(json: &str) -> ComposerPackage {
        json.parse::<ComposerPackage>().unwrap()
    }

    #[test]
    fn a_project_without_the_plugin_has_no_bin_directory() {
        let root = Path::new("/project");
        assert!(bin_target_directory(root, &pkg("{}")).is_none());
    }

    #[test]
    fn the_bin_directory_defaults_to_vendor_bin() {
        let root = Path::new("/project");
        let package = pkg(r#"{ "require-dev": { "bamarni/composer-bin-plugin": "^1.8" } }"#);
        assert_eq!(
            bin_target_directory(root, &package),
            Some(root.join("vendor-bin"))
        );
    }

    #[test]
    fn the_bin_directory_follows_the_plugin_config() {
        let root = Path::new("/project");
        let package = pkg(r#"{
            "require": { "bamarni/composer-bin-plugin": "^1.8" },
            "extra": { "bamarni-bin": { "target-directory": "tools/" } }
        }"#);
        assert_eq!(
            bin_target_directory(root, &package),
            Some(root.join("tools"))
        );
    }

    #[test]
    fn namespaces_are_the_directories_holding_a_composer_json_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("vendor-bin");
        write(&bin.join("rector/composer.json"), "{}");
        write(
            &bin.join("phpunit/composer.json"),
            r#"{ "config": { "vendor-dir": "lib" } }"#,
        );
        write(&bin.join("notes/README.md"), "");
        write(&bin.join(".hidden/composer.json"), "{}");
        let package = pkg(r#"{ "require-dev": { "bamarni/composer-bin-plugin": "*" } }"#);

        let namespaces = bin_namespaces(dir.path(), &package);

        let roots: Vec<&Path> = namespaces.iter().map(|ns| ns.root.as_path()).collect();
        assert_eq!(roots, [bin.join("phpunit"), bin.join("rector")]);
        assert_eq!(namespaces[0].vendor_path(), bin.join("phpunit/lib"));
        assert_eq!(namespaces[1].vendor_path(), bin.join("rector/vendor"));
    }
}
