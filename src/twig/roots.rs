//! Where a project's Twig templates live, read from the templates
//! themselves.
//!
//! A loader's directories usually come from runtime settings (the theme, the
//! locale), so they cannot be read from the PHP that builds it.  The names a
//! template references say where it lives instead: a name resolves against
//! one of the loader's roots, so the closest directory above the template
//! that holds the named file is a root.  `templates/` is a root as well when
//! the project has one, which is where Symfony keeps its templates.
//!
//! A template is a `.twig` file, or a file of another extension a template
//! can have (`.html`, `.htm`) under a root, once some template has been seen
//! naming a file of that extension.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// The extensions of the files that can be templates: a `.twig` file always
/// is one, and the others are when they lie under a root.
const CANDIDATE_EXTENSIONS: [&str; 3] = ["twig", "html", "htm"];

/// The extension of the last segment of `name`, the text after its last dot.
fn extension_of(name: &str) -> Option<&str> {
    let file = name.rsplit('/').next()?;
    let (stem, extension) = file.rsplit_once('.')?;
    (!stem.is_empty() && !extension.is_empty()).then_some(extension)
}

/// Whether `extension` is one of [`CANDIDATE_EXTENSIONS`].
fn is_candidate_extension(extension: &str) -> bool {
    CANDIDATE_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

/// Whether the file at `uri` (or a path spelled the same way) can be a
/// template: a `.twig`, `.html`, or `.htm` file.
pub(crate) fn is_candidate_uri(uri: &str) -> bool {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    extension_of(path).is_some_and(is_candidate_extension)
}

/// Whether the file at `path` can be a template.
pub(crate) fn is_candidate_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(is_candidate_extension)
}

/// `path` with its components joined by `/`, when they are all plain names.
pub(crate) fn slash_path(path: &Path) -> Option<String> {
    let mut joined = String::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return None;
        };
        if !joined.is_empty() {
            joined.push('/');
        }
        joined.push_str(part.to_str()?);
    }
    (!joined.is_empty()).then_some(joined)
}

/// `name` as a path relative to a root, when it is one: no Twig namespace,
/// and no way out of the root it is resolved against.
pub(crate) fn plain_relative(name: &str) -> Option<&Path> {
    let relative = Path::new(name);
    (!name.starts_with('@')
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_))))
    .then_some(relative)
}

/// The roots a project's templates are loaded from, and the extensions its
/// templates use.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TwigRoots {
    /// Sorted.  One root may lie inside another.
    roots: Vec<PathBuf>,
    /// The project's `templates/` directory, when it has one, whose
    /// `bundles/` subdirectory overrides the templates of a bundle.
    templates_dir: Option<PathBuf>,
    /// Lowercase and sorted.  Always holds `twig`.
    extensions: Vec<String>,
}

impl Default for TwigRoots {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            templates_dir: None,
            extensions: vec!["twig".to_string()],
        }
    }
}

impl TwigRoots {
    /// Discover the roots of the templates in `files`, each with the plain
    /// names (see [`plain_relative`]) it references, inside the workspace
    /// at `workspace_root`.
    ///
    /// A name resolves against the closest directory above the template that
    /// holds it: a template in a subdirectory of a root that names another
    /// file of that subdirectory names it by its path from the root.
    pub(crate) fn discover(workspace_root: &Path, files: &HashMap<PathBuf, Vec<String>>) -> Self {
        let templates_dir = Some(workspace_root.join("templates")).filter(|dir| dir.is_dir());
        let mut roots: Vec<PathBuf> = templates_dir.iter().cloned().collect();
        let mut extensions = vec!["twig".to_string()];
        for (path, references) in files {
            for name in references {
                let Some(relative) = plain_relative(name) else {
                    continue;
                };
                let mut dir = path.parent();
                while let Some(candidate) = dir.filter(|dir| dir.starts_with(workspace_root)) {
                    if files.contains_key(&candidate.join(relative)) {
                        roots.push(candidate.to_path_buf());
                        if let Some(extension) = extension_of(name)
                            && !extensions
                                .iter()
                                .any(|known| known.eq_ignore_ascii_case(extension))
                        {
                            extensions.push(extension.to_ascii_lowercase());
                        }
                        break;
                    }
                    dir = candidate.parent();
                }
            }
        }
        roots.sort_unstable();
        roots.dedup();
        extensions.sort_unstable();
        Self {
            roots,
            templates_dir,
            extensions,
        }
    }

    /// Whether no root is known, so that no name can be told unknown.
    pub(crate) fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Whether `name` can be a template name: it ends in the extension of
    /// one of the project's templates, and holds nothing a name never does.
    /// Twig takes any name its loader finds, but a string that ends in
    /// anything else is far more often something else.
    pub(crate) fn is_template_name(&self, name: &str) -> bool {
        super::templates::looks_like_template_name(name)
            && extension_of(name).is_some_and(|extension| self.uses_extension(extension))
    }

    fn uses_extension(&self, extension: &str) -> bool {
        self.extensions
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
    }

    /// Whether the file at `path` is a template under one of the roots.
    pub(crate) fn holds(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| self.uses_extension(extension))
            && self.roots.iter().any(|root| path.starts_with(root))
    }

    /// The names the template at `path` is loaded by: its path under each
    /// root that holds it, and a bundle's template overridden under
    /// `templates/bundles/AcmeBundle/` by the bundle's Twig namespace as
    /// well (`@Acme/…`).  Sorted.
    pub(crate) fn template_names(&self, path: &Path) -> Vec<String> {
        let mut names = Vec::new();
        for root in &self.roots {
            let Some(name) = path
                .strip_prefix(root)
                .ok()
                .and_then(slash_path)
                .filter(|name| self.is_template_name(name))
            else {
                continue;
            };
            if self.templates_dir.as_ref() == Some(root)
                && let Some((bundle, rest)) = name
                    .strip_prefix("bundles/")
                    .and_then(|rest| rest.split_once('/'))
                && let Some(namespace) = bundle.strip_suffix("Bundle").filter(|ns| !ns.is_empty())
            {
                names.push(format!("@{namespace}/{rest}"));
            }
            names.push(name);
        }
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Where a template named `name` is created: under `templates/`, or the
    /// only root when there is just one, or else under `default_dir`, for a
    /// name with no Twig namespace and no way out of that directory.
    pub(crate) fn template_file(&self, name: &str, default_dir: Option<&Path>) -> Option<PathBuf> {
        let relative = plain_relative(name).filter(|_| self.is_template_name(name))?;
        let dir = self
            .templates_dir
            .as_deref()
            .or(match self.roots.as_slice() {
                [only] => Some(only.as_path()),
                _ => None,
            })
            .or(default_dir)?;
        Some(dir.join(relative))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(root: &Path, entries: &[(&str, &[&str])]) -> HashMap<PathBuf, Vec<String>> {
        entries
            .iter()
            .map(|(path, references)| {
                (
                    root.join(path),
                    references.iter().map(|name| name.to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn roots_are_where_the_referenced_names_resolve() {
        let root = Path::new("/p");
        let found = TwigRoots::discover(
            root,
            &files(
                root,
                &[
                    ("theme/default/index.html", &["menu.html"]),
                    ("theme/default/menu.html", &[]),
                    ("theme/default/list.html", &["index.html"]),
                    ("theme/default/admin/index.html", &[]),
                    ("theme/default/admin/list.html", &["admin/index.html"]),
                    ("theme/other/index.html", &["menu.html"]),
                    ("theme/other/menu.html", &[]),
                    // Nothing here references anything, so it is no root of
                    // its own.
                    ("theme/default/da_DK/price.html", &[]),
                    // A name that resolves nowhere, and a namespaced one.
                    ("theme/lone/index.html", &["missing.html", "@admin/x.html"]),
                    ("docs/page.html", &[]),
                ],
            ),
        );
        assert_eq!(
            found.roots,
            vec![
                PathBuf::from("/p/theme/default"),
                PathBuf::from("/p/theme/other")
            ]
        );
        assert_eq!(
            found.extensions,
            vec!["html".to_string(), "twig".to_string()]
        );

        assert_eq!(
            found.template_names(Path::new("/p/theme/default/admin/list.html")),
            vec!["admin/list.html".to_string()]
        );
        assert_eq!(
            found.template_names(Path::new("/p/theme/default/da_DK/price.html")),
            vec!["da_DK/price.html".to_string()]
        );
        assert!(found.holds(Path::new("/p/theme/other/new.html")));
        assert!(!found.holds(Path::new("/p/docs/page.html")));
        assert!(!found.holds(Path::new("/p/theme/default/readme.md")));
        assert!(found.is_template_name("index.html"));
        assert!(!found.is_template_name("index.htm"));
        assert!(!found.is_template_name("style.css"));
    }

    #[test]
    fn without_html_templates_only_twig_names_count() {
        let root = Path::new("/p");
        let found = TwigRoots::discover(
            root,
            &files(
                root,
                &[
                    ("views/page.html.twig", &["base.html.twig"]),
                    ("views/base.html.twig", &[]),
                ],
            ),
        );
        assert_eq!(found.roots, vec![PathBuf::from("/p/views")]);
        assert!(found.is_template_name("page.html.twig"));
        assert!(!found.is_template_name("index.html"));
        assert!(!found.holds(Path::new("/p/views/static.html")));
    }

    #[test]
    fn bundle_overrides_are_named_by_the_bundle_namespace() {
        let roots = TwigRoots {
            roots: vec![PathBuf::from("/p/templates")],
            templates_dir: Some(PathBuf::from("/p/templates")),
            extensions: vec!["twig".to_string()],
        };
        assert_eq!(
            roots.template_names(Path::new("/p/templates/demo/index.html.twig")),
            vec!["demo/index.html.twig".to_string()]
        );
        assert_eq!(
            roots.template_names(Path::new(
                "/p/templates/bundles/AcmeBundle/widget.html.twig"
            )),
            vec![
                "@Acme/widget.html.twig".to_string(),
                "bundles/AcmeBundle/widget.html.twig".to_string(),
            ]
        );
        assert!(
            roots
                .template_names(Path::new("/p/src/view.html.twig"))
                .is_empty()
        );
        assert!(
            roots
                .template_names(Path::new("/p/templates/readme.md"))
                .is_empty()
        );
    }

    #[test]
    fn only_a_plain_name_is_created() {
        let roots = TwigRoots {
            roots: vec![PathBuf::from("/p/templates")],
            templates_dir: Some(PathBuf::from("/p/templates")),
            extensions: vec!["twig".to_string()],
        };
        assert_eq!(
            roots.template_file("demo/new.html.twig", None),
            Some(PathBuf::from("/p/templates/demo/new.html.twig"))
        );
        assert_eq!(roots.template_file("@Acme/new.html.twig", None), None);
        assert_eq!(roots.template_file("../escape.html.twig", None), None);
        assert_eq!(roots.template_file("/abs.html.twig", None), None);
        assert_eq!(roots.template_file("demo/new.txt", None), None);

        // With several roots and no `templates/`, nothing says which one.
        let themes = TwigRoots {
            roots: vec![PathBuf::from("/p/a"), PathBuf::from("/p/b")],
            templates_dir: None,
            extensions: vec!["html".to_string(), "twig".to_string()],
        };
        assert_eq!(themes.template_file("new.html", None), None);
        assert_eq!(
            themes.template_file("new.html", Some(Path::new("/p/templates"))),
            Some(PathBuf::from("/p/templates/new.html"))
        );
    }
}
