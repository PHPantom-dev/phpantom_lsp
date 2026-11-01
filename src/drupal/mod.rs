//! Drupal's own declarations, read the way Drupal reads them.
//!
//! Drupal builds a Symfony service container from YAML in Symfony's
//! service format, but finds the files itself: every module, theme, and
//! install profile under the web root declares its services in
//! `<name>.services.yml`, core in `core/core.services.yml`, and the site in
//! `sites/*/services.yml`.  Those are read by the same scan as Symfony's
//! configuration (see [`crate::symfony::resources`]), so service IDs and
//! container parameters work alike in both.  Nothing else Symfony declares
//! (routes, translations, events) is read here: Drupal declares those its
//! own way.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use parking_lot::RwLock;

use crate::Backend;

/// The web roots of the workspace's Drupal projects.
#[derive(Default)]
pub(crate) struct DrupalState {
    web_roots: RwLock<Vec<PathBuf>>,
}

/// Whether the file name at the end of `path` is one Drupal loads services
/// from: `services.yml`, or one ending in `.services.yml`.
fn is_services_file_name(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "services.yml" || name.ends_with(".services.yml")
}

impl Backend {
    /// Whether the workspace holds a Drupal project: one whose web root
    /// was detected; see [`Backend::mark_drupal_project`].
    pub(crate) fn is_drupal_project(&self) -> bool {
        self.workspace.is_drupal.load(Ordering::Acquire)
    }

    /// Whether the project builds a Symfony service container, as Symfony
    /// and Drupal both do: service IDs and container parameters are read in
    /// either.
    pub(crate) fn reads_service_container(&self) -> bool {
        self.is_symfony_project() || self.is_drupal_project()
    }

    /// Forget the web roots, before the workspace's Composer projects are
    /// read again.
    pub(crate) fn clear_drupal_web_roots(&self) {
        self.drupal.web_roots.write().clear();
    }

    /// Remember the web root of a Drupal project.
    pub(crate) fn note_drupal_web_root(&self, web_root: &Path) {
        let mut roots = self.drupal.web_roots.write();
        if !roots.iter().any(|root| root == web_root) {
            roots.push(web_root.to_path_buf());
        }
    }

    /// Whether `uri` is a file a Drupal project loads services from: a
    /// services file inside one of its web roots.
    ///
    /// Other YAML files in a Drupal project have a `services:` key with
    /// another meaning, or none at all.
    pub(crate) fn is_drupal_services_uri(&self, uri: &str) -> bool {
        if !self.is_drupal_project() || !is_services_file_name(uri) {
            return false;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return false;
        };
        self.drupal
            .web_roots
            .read()
            .iter()
            .any(|root| path.starts_with(root))
    }
}

#[cfg(test)]
mod tests;
