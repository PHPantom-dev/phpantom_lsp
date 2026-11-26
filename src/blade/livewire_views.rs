//! Where Livewire 4 keeps view-based components.
//!
//! Livewire 4's `LivewireServiceProvider::bootConfig()` adds each directory
//! in its `component_locations` config as a view location and a prefix-less
//! anonymous component path, and each `component_namespaces` entry as a view
//! namespace and an anonymous component namespace of the same name.  Its
//! finder looks for single-file and multi-file components in the same
//! directories.  The settings come from the project's `config/livewire.php`,
//! with each top-level key the project leaves out taken from the installed
//! Livewire's own config (`mergeConfigFrom()`), so a project that never
//! published the file gets Livewire's defaults.  Livewire 3 has neither key,
//! which leaves the set empty.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::cst::*;

use crate::Backend;
use crate::virtual_members::laravel::array_file::returned_array_entry;
use crate::virtual_members::laravel::{PathContext, resolve_path_arg};

const LOCATIONS_KEY: &str = "component_locations";
const NAMESPACES_KEY: &str = "component_namespaces";

/// Livewire 4's view-based component directories, whether or not each one
/// exists yet.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct LivewireViews {
    /// `component_locations`, in order: view roots after the ones
    /// `config/view.php` configures, and prefix-less anonymous component
    /// paths.
    pub locations: Vec<PathBuf>,
    /// `component_namespaces`, as `(namespace, directory)`: view namespaces
    /// and anonymous component namespaces.
    pub namespaces: Vec<(String, PathBuf)>,
    /// A setting is written in a way this cannot read, so any view or
    /// component name may live in a directory it names.
    pub open: bool,
}

impl LivewireViews {
    /// Whether the installed Livewire registers view-based component
    /// directories at all.
    pub(crate) fn is_active(&self) -> bool {
        self.open || !self.locations.is_empty() || !self.namespaces.is_empty()
    }
}

/// What one config file says about one setting.
enum Setting<T> {
    /// The file does not set it.
    Absent,
    Read(Vec<T>),
    /// The file sets it in a way this cannot read.
    Unknown,
}

/// The two settings as one `config/livewire.php` writes them.
struct ConfigFile {
    locations: Setting<PathBuf>,
    namespaces: Setting<(String, PathBuf)>,
}

fn read_config(content: &str, file_dir: &Path, workspace_root: &Path) -> ConfigFile {
    let arena = LocalArena::new();
    let program = mago_syntax::parser::parse_file_content(
        &arena,
        FileId::new(b"livewire.php"),
        content.as_bytes(),
    );
    let paths = PathContext {
        file_dir,
        workspace_root,
        program,
    };
    let elements = |key: &str| match returned_array_entry(program, key) {
        Some(Expression::Array(array)) => Ok(Some(array.elements.iter())),
        Some(Expression::LegacyArray(array)) => Ok(Some(array.elements.iter())),
        Some(_) => Err(()),
        // A key the file does mention, but not as an entry of the array it
        // returns directly, is set somewhere this does not follow.
        None if content.contains(key) => Err(()),
        None => Ok(None),
    };

    let locations = match elements(LOCATIONS_KEY) {
        Ok(None) => Setting::Absent,
        Err(()) => Setting::Unknown,
        Ok(Some(elements)) => elements
            .map(|element| match element {
                ArrayElement::Value(value) => resolve_path_arg(value.value, content, &paths),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map_or(Setting::Unknown, Setting::Read),
    };
    let namespaces = match elements(NAMESPACES_KEY) {
        Ok(None) => Setting::Absent,
        Err(()) => Setting::Unknown,
        Ok(Some(elements)) => elements
            .map(|element| {
                let ArrayElement::KeyValue(entry) = element else {
                    return None;
                };
                let (namespace, _, _) =
                    crate::virtual_members::laravel::helpers::resolved_string_literal(
                        entry.key, content,
                    )?;
                let path = resolve_path_arg(entry.value, content, &paths)?;
                Some((namespace.to_string(), path))
            })
            .collect::<Option<Vec<_>>>()
            .map_or(Setting::Unknown, Setting::Read),
    };
    ConfigFile {
        locations,
        namespaces,
    }
}

/// The settings in effect: the project's own where it sets one, the
/// installed Livewire's otherwise.
fn merge_settings(project: Option<ConfigFile>, package: Option<ConfigFile>) -> LivewireViews {
    let (project_locations, project_namespaces) = match project {
        Some(file) => (file.locations, file.namespaces),
        None => (Setting::Absent, Setting::Absent),
    };
    let (package_locations, package_namespaces) = match package {
        Some(file) => (file.locations, file.namespaces),
        None => (Setting::Absent, Setting::Absent),
    };
    let mut views = LivewireViews::default();
    match pick(project_locations, package_locations) {
        Setting::Read(locations) => views.locations = locations,
        Setting::Unknown => views.open = true,
        Setting::Absent => {}
    }
    match pick(project_namespaces, package_namespaces) {
        Setting::Read(namespaces) => views.namespaces = namespaces,
        Setting::Unknown => views.open = true,
        Setting::Absent => {}
    }
    views
}

fn pick<T>(project: Setting<T>, package: Setting<T>) -> Setting<T> {
    match project {
        Setting::Absent => package,
        set => set,
    }
}

impl Backend {
    /// Livewire 4's view-based component directories.  Cached until
    /// `config/livewire.php` changes or the provider scan that locates
    /// Livewire's own config is published again.
    pub(crate) fn livewire_views(&self) -> Arc<LivewireViews> {
        let Some(root) = self.laravel_root() else {
            return Arc::new(LivewireViews::default());
        };
        self.cached_laravel_enumeration(
            &self.laravel.string_key_build_locks.livewire_views,
            |cache| cache.livewire_views.clone(),
            |cache, views| cache.livewire_views = Some(views),
            || Arc::new(self.read_livewire_views(&root)),
        )
    }

    fn read_livewire_views(&self, root: &Path) -> LivewireViews {
        // Only Livewire 4 reads these settings, and its own config is how
        // it is told apart from Livewire 3: the package config is the one
        // its service provider merges in.
        let package_config = self
            .laravel
            .provider_resources
            .read()
            .config_files
            .iter()
            .find(|file| file.namespace == "livewire")
            .map(|file| file.path.clone());
        let Some(package_config) = package_config else {
            return LivewireViews::default();
        };
        let Ok(package_content) = std::fs::read_to_string(&package_config) else {
            return LivewireViews::default();
        };
        if !package_content.contains(LOCATIONS_KEY) {
            return LivewireViews::default();
        }
        let package = read_config(
            &package_content,
            package_config.parent().unwrap_or(root),
            root,
        );

        let project_path = root.join("config").join("livewire.php");
        let project_content = crate::util::path_to_url(&project_path)
            .and_then(|uri| self.get_file_content(uri.as_str()))
            .or_else(|| std::fs::read_to_string(&project_path).ok());
        let project = project_content
            .map(|content| read_config(&content, project_path.parent().unwrap_or(root), root));
        merge_settings(project, Some(package))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(content: &str, root: &Path) -> ConfigFile {
        read_config(content, &root.join("config"), root)
    }

    #[test]
    fn reads_both_settings() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = read(
            "<?php\nreturn [\n    'component_locations' => [\n        resource_path('views/components'),\n        resource_path('views/livewire'),\n    ],\n    'component_namespaces' => [\n        'layouts' => resource_path('views/layouts'),\n        'pages' => base_path('pages'),\n    ],\n];\n",
            root,
        );
        let views = merge_settings(Some(file), None);
        assert_eq!(
            views.locations,
            vec![
                root.join("resources/views/components"),
                root.join("resources/views/livewire"),
            ]
        );
        assert_eq!(
            views.namespaces,
            vec![
                ("layouts".to_string(), root.join("resources/views/layouts")),
                ("pages".to_string(), root.join("pages")),
            ]
        );
        assert!(!views.open);
    }

    #[test]
    fn a_setting_the_project_leaves_out_comes_from_the_package() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let project = read(
            "<?php\nreturn [\n    'component_locations' => [app_path('Views')],\n];\n",
            root,
        );
        let package = read(
            "<?php\nreturn [\n    'component_locations' => [resource_path('views/livewire')],\n    'component_namespaces' => ['pages' => resource_path('views/pages')],\n];\n",
            root,
        );
        let views = merge_settings(Some(project), Some(package));
        assert_eq!(views.locations, vec![root.join("app/Views")]);
        assert_eq!(
            views.namespaces,
            vec![("pages".to_string(), root.join("resources/views/pages"))]
        );
    }

    #[test]
    fn an_unreadable_entry_leaves_the_set_open() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = read(
            "<?php\nreturn [\n    'component_locations' => [resource_path('views/livewire'), $somewhere],\n];\n",
            root,
        );
        let views = merge_settings(Some(file), None);
        assert!(views.open);
        assert!(views.locations.is_empty());

        let file = read(
            "<?php\nreturn [\n    'component_namespaces' => config('other.namespaces'),\n];\n",
            root,
        );
        assert!(merge_settings(Some(file), None).open);
    }
}
