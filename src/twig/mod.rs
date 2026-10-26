//! Twig templates, in any project that uses Twig: where they live, the
//! names they are loaded by, and the places that name one.
//!
//! The template names a project's PHP and templates read are
//! [`TwigTemplate`](crate::symbol_map::StringKeyKind::TwigTemplate) string
//! keys, declared by each template (see [`templates`]) and gathered with
//! Symfony's names (see [`crate::symfony`]).  The roots they are resolved
//! against are discovered from the names the templates reference (see
//! [`roots`]), so every template file in the workspace is remembered here
//! with those names, and the roots rediscovered when they change.

pub(crate) mod roots;
pub(crate) mod templates;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use parking_lot::RwLock;

use crate::Backend;
use roots::TwigRoots;

/// The project's template files and the roots discovered from them.
#[derive(Default)]
pub(crate) struct TwigState {
    /// Every file in the workspace that can be a template (see
    /// [`roots::is_candidate_path`]), with the plain names it references.
    files: RwLock<HashMap<PathBuf, Vec<String>>>,
    roots: RwLock<Arc<TwigRoots>>,
}

impl Backend {
    /// Whether the project uses Twig; see
    /// [`Backend::is_twig`](crate::Backend::is_twig).
    pub(crate) fn is_twig_project(&self) -> bool {
        self.is_twig.load(Ordering::Relaxed)
    }

    /// Record whether the project uses Twig, once the workspace's
    /// `composer.json` files have been read.
    pub(crate) fn set_is_twig(&self, is_twig: bool) {
        self.is_twig.store(is_twig, Ordering::Relaxed);
    }

    /// The roots the project's templates are loaded from.
    pub(crate) fn twig_roots(&self) -> Arc<TwigRoots> {
        Arc::clone(&self.twig.roots.read())
    }

    /// Whether the file at `uri` is a Twig template: a `.twig` file, or a
    /// file of another extension the project's templates use under one of
    /// their roots.
    pub(crate) fn is_twig_template_uri(&self, uri: &str) -> bool {
        if crate::symfony::routes::is_twig_uri(uri) {
            return true;
        }
        if !roots::is_candidate_uri(uri) || !self.is_twig_project() {
            return false;
        }
        let roots = self.twig_roots();
        !roots.is_empty() && crate::util::uri_to_path(uri).is_some_and(|path| roots.holds(&path))
    }

    /// Whether `uri` is a resource document (see
    /// [`crate::resource_navigation`]): one whose extension says so, or a
    /// template whose extension alone does not.
    pub(crate) fn is_resource_uri(&self, uri: &str) -> bool {
        crate::resource_navigation::is_resource_document(uri) || self.is_twig_template_uri(uri)
    }

    /// Where the Twig template named `name` is created, when it is a name
    /// of the project's own templates.  A Symfony project keeps them under
    /// `templates/` even before it has any.
    pub(crate) fn twig_template_file(&self, name: &str) -> Option<PathBuf> {
        let default_dir = self
            .is_symfony_project()
            .then(|| self.workspace.workspace_root.read().clone())
            .flatten()
            .map(|root| root.join("templates"));
        self.twig_roots()
            .template_file(name, default_dir.as_deref())
    }

    /// The path of `uri`, when it is a workspace file that can be a
    /// template and the project uses Twig.  Dependencies are not the
    /// project's templates.
    fn twig_candidate_path(&self, uri: &str) -> Option<PathBuf> {
        if !self.is_twig_project() || !roots::is_candidate_uri(uri) {
            return None;
        }
        let is_vendor = self
            .workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()));
        if is_vendor {
            return None;
        }
        crate::util::uri_to_path(uri)
    }

    /// Whether root discovery has seen the file at `path`, or never will:
    /// it cannot be a template, or the project does not use Twig.
    pub(crate) fn twig_has_seen(&self, path: &Path) -> bool {
        !self.is_twig_project()
            || !roots::is_candidate_path(path)
            || self.twig.files.read().contains_key(path)
    }

    /// Record what the file at `uri`, which may be a template, references.
    /// When that moves the roots, the other templates are re-indexed, since
    /// what they are named and which files are templates moved with them.
    pub(crate) fn note_twig_source(&self, uri: &str, content: &str) {
        let Some(path) = self.twig_candidate_path(uri) else {
            return;
        };
        let references = templates::template_references(content);
        {
            let files = self.twig.files.read();
            if files.get(&path) == Some(&references) {
                return;
            }
        }
        let moved = {
            let mut files = self.twig.files.write();
            files.insert(path.clone(), references);
            self.rediscover_twig_roots(&files)
        };
        if moved {
            self.refresh_twig_templates(|other| other == path);
        }
    }

    /// [`Self::note_twig_source`] for a batch of files read by the
    /// workspace walk, which are indexed after this returns.  The files
    /// indexed before (the open ones) are re-indexed when the roots moved.
    pub(crate) fn note_twig_sources<'a>(&self, sources: impl Iterator<Item = (&'a str, &'a str)>) {
        let mut batch: Vec<PathBuf> = Vec::new();
        let moved = {
            let mut files = self.twig.files.write();
            for (uri, content) in sources {
                if let Some(path) = self.twig_candidate_path(uri) {
                    files.insert(path.clone(), templates::template_references(content));
                    batch.push(path);
                }
            }
            !batch.is_empty() && self.rediscover_twig_roots(&files)
        };
        if moved {
            batch.sort_unstable();
            self.refresh_twig_templates(|path| {
                batch.binary_search_by(|p| p.as_path().cmp(path)).is_ok()
            });
        }
    }

    /// Forget a file that no longer exists.
    pub(crate) fn forget_twig_source(&self, uri: &str) {
        let Some(path) = crate::util::uri_to_path(uri) else {
            return;
        };
        if !self.twig.files.read().contains_key(&path) {
            return;
        }
        let moved = {
            let mut files = self.twig.files.write();
            files.remove(&path);
            self.rediscover_twig_roots(&files)
        };
        if moved {
            self.refresh_twig_templates(|_| false);
        }
    }

    /// Rediscover the roots from `files`, and say whether they moved.
    fn rediscover_twig_roots(&self, files: &HashMap<PathBuf, Vec<String>>) -> bool {
        let Some(workspace_root) = self.workspace.workspace_root.read().clone() else {
            return false;
        };
        let discovered = TwigRoots::discover(&workspace_root, files);
        let mut roots = self.twig.roots.write();
        if **roots == discovered {
            return false;
        }
        *roots = Arc::new(discovered);
        true
    }

    /// Re-index every template file but the ones `skip` names, after the
    /// roots moved: a template's names may have changed, and a file may
    /// have become a template or stopped being one.
    fn refresh_twig_templates(&self, skip: impl Fn(&Path) -> bool) {
        let paths: Vec<PathBuf> = self
            .twig
            .files
            .read()
            .keys()
            .filter(|path| !skip(path))
            .cloned()
            .collect();
        for path in paths {
            let uri = crate::util::path_to_uri(&path);
            let open = self.open_files.read().get(&uri).cloned();
            if self.is_resource_uri(&uri) {
                let content = match open {
                    Some(content) => content,
                    None => match std::fs::read_to_string(&path) {
                        Ok(content) => Arc::new(content),
                        Err(_) => continue,
                    },
                };
                self.publish_resource_symbol_index(&uri, &content);
            } else if self.symbol_map_for(&uri).is_some() {
                match open {
                    // An open file that stopped being a template is read
                    // the way any other open file is.
                    Some(content) => {
                        self.update_ast(&uri, &content);
                    }
                    None => self.clear_file_maps(&uri),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
