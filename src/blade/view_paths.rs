//! Where a project's Blade templates live: the view roots `config/view.php`
//! configures, or Laravel's conventional `resources/views` when it names
//! none.

use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::Url;

use crate::text_scan::unquote_php_string;

/// Discover Laravel Blade view directories from `config/view.php`.
///
/// Parses the `'paths'` array in the config file to extract directory
/// paths.  Falls back to `resources/views` if the config file is
/// missing or unparseable.  Returns only directories that exist.
pub fn discover_view_paths(workspace_root: &Path) -> Vec<PathBuf> {
    let config = std::fs::read_to_string(workspace_root.join("config/view.php")).ok();
    view_paths_from_config(config.as_deref(), workspace_root)
}

/// [`discover_view_paths`] for a `config/view.php` whose text the caller
/// already has (`None` when there is no such file).
fn view_paths_from_config(config: Option<&str>, workspace_root: &Path) -> Vec<PathBuf> {
    let paths = config
        .map(|content| parse_view_config_paths(content, workspace_root))
        .unwrap_or_default();

    if paths.is_empty() {
        // Fallback: use the conventional Laravel view directory.
        let default = workspace_root.join("resources/views");
        if default.is_dir() {
            return vec![default];
        }
        return Vec::new();
    }

    paths
}

/// A Blade view root directory.
#[derive(Debug)]
pub(crate) struct ViewRoot {
    /// The directory as configured.
    pub path: PathBuf,
    /// The same directory with symlinks resolved, for a template path that
    /// reaches it under another spelling.  `None` when it cannot be
    /// resolved.
    pub canonical: Option<PathBuf>,
}

/// Whether `uri`'s file already lies under one of `roots`.
///
/// Used to tell an edit inside an already-known root (which needs no
/// recompute) from a file that just appeared under a directory the cached
/// roots don't know about yet: a `resources/views` created after the roots
/// were first (and, until now, permanently) computed, or a custom root
/// from `config/view.php` that didn't exist at the time. A `uri` that
/// cannot be parsed to a file path is treated as covered, so it cannot
/// force a spurious recompute.
pub(crate) fn view_root_covers(roots: &[ViewRoot], uri: &str) -> bool {
    let Some(path) = Url::parse(uri).ok().and_then(|u| u.to_file_path().ok()) else {
        return true;
    };
    roots.iter().any(|root| {
        path.starts_with(&root.path)
            || root
                .canonical
                .as_deref()
                .is_some_and(|c| path.starts_with(c))
    })
}

impl crate::Backend {
    /// The configured Blade view root directories.
    ///
    /// Reads the `paths` array from `config/view.php` (falling back to
    /// the conventional `resources/views`) so that projects with custom
    /// view directories resolve `view()` names correctly. Only existing
    /// directories are returned. Cached until `config/view.php` changes or a
    /// file appears under a directory the cached roots don't already
    /// cover, and read through the editor's buffer when the file is open.
    pub(crate) fn laravel_view_roots(&self) -> std::sync::Arc<Vec<ViewRoot>> {
        // The workspace root is not known before `initialize` sets it. Don't
        // cache this answer: caching it would freeze the roots at "none"
        // for the rest of the session once the real root becomes known.
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return std::sync::Arc::new(Vec::new());
        };
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.view_roots,
            |cache| cache.view_roots.clone(),
            |cache, roots| cache.view_roots = Some(roots),
            || {
                let config_uri = crate::util::path_to_uri(&root.join("config/view.php"));
                let config = self.get_file_content_arc(&config_uri);
                let roots = view_paths_from_config(config.as_deref().map(String::as_str), &root)
                    .into_iter()
                    .map(|path| ViewRoot {
                        canonical: path.canonicalize().ok(),
                        path,
                    })
                    .collect();
                std::sync::Arc::new(roots)
            },
        )
    }
}

/// Parse `config/view.php` to extract the `'paths'` array entries.
///
/// Looks for string literals inside `'paths' => [...]` and resolves
/// `base_path('...')` calls relative to the workspace root.
fn parse_view_config_paths(content: &str, workspace_root: &Path) -> Vec<PathBuf> {
    // Find the 'paths' => [...] section.
    let paths_idx = match content.find("'paths'") {
        Some(i) => i,
        None => return Vec::new(),
    };
    let after = &content[paths_idx..];

    // Find the opening bracket.
    let bracket_start = match after.find('[') {
        Some(i) => i,
        None => return Vec::new(),
    };
    let bracket_end = match after[bracket_start..].find(']') {
        Some(i) => bracket_start + i,
        None => return Vec::new(),
    };
    let array_content = &after[bracket_start + 1..bracket_end];

    let mut result = Vec::new();

    // Match `base_path('...')`, `resource_path('...')`, `realpath(...)`
    // wrappers, and bare string literals.
    for segment in array_content.split(',') {
        let trimmed = segment.trim();
        if let Some(path) = extract_view_path_arg(trimmed) {
            let resolved = workspace_root.join(path);
            if resolved.is_dir() {
                result.push(resolved);
            }
        } else if let Some(path) = unquote_php_string(trimmed) {
            // Absolute or relative path literal.
            let resolved = if Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else {
                workspace_root.join(path)
            };
            if resolved.is_dir() {
                result.push(resolved);
            }
        }
    }

    result
}

/// Extract the workspace-relative directory from a `config/view.php`
/// path expression: `base_path('resources/views')`,
/// `resource_path('views')`, or either wrapped in `realpath(...)`.
///
/// `resource_path('X')` resolves to `resources/X` (and bare
/// `resource_path()` to `resources`), matching Laravel's helper.
fn extract_view_path_arg(s: &str) -> Option<String> {
    // Strip an optional `realpath(` wrapper.
    let inner = if let Some(rest) = s.strip_prefix("realpath(") {
        rest.strip_suffix(')')?.trim()
    } else {
        s
    };

    if let Some(rest) = inner.strip_prefix("base_path(") {
        let arg = rest.strip_suffix(')')?.trim();
        return unquote_php_string(arg).map(|p| p.to_string());
    }

    if let Some(rest) = inner.strip_prefix("resource_path(") {
        let arg = rest.strip_suffix(')')?.trim();
        if arg.is_empty() {
            return Some("resources".to_string());
        }
        return unquote_php_string(arg).map(|p| format!("resources/{p}"));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_path_arg_variants() {
        assert_eq!(
            extract_view_path_arg("base_path('resources/views')").as_deref(),
            Some("resources/views")
        );
        assert_eq!(
            extract_view_path_arg("realpath(base_path('resources/backoffice/views'))").as_deref(),
            Some("resources/backoffice/views")
        );
        // resource_path('X') resolves relative to the resources dir.
        assert_eq!(
            extract_view_path_arg("resource_path('views')").as_deref(),
            Some("resources/views")
        );
        assert_eq!(
            extract_view_path_arg("resource_path('theme/views')").as_deref(),
            Some("resources/theme/views")
        );
        assert_eq!(
            extract_view_path_arg("resource_path()").as_deref(),
            Some("resources")
        );
        assert_eq!(extract_view_path_arg("some_other_call('x')"), None);
    }

    #[test]
    fn discover_view_paths_reads_custom_config() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("resources/backoffice/views")).unwrap();
        std::fs::create_dir_all(root.join("resources/views")).unwrap();
        std::fs::write(
            root.join("config/view.php"),
            "<?php\nreturn [\n 'paths' => [\n  realpath(base_path('resources/backoffice/views')),\n  resource_path('views'),\n ],\n];\n",
        )
        .unwrap();

        let paths = discover_view_paths(root);
        assert!(paths.contains(&root.join("resources/backoffice/views")));
        assert!(paths.contains(&root.join("resources/views")));
    }

    #[test]
    fn discover_view_paths_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("resources/views")).unwrap();
        // No config/view.php present.
        let paths = discover_view_paths(root);
        assert_eq!(paths, vec![root.join("resources/views")]);
    }

    #[test]
    fn view_roots_recompute_when_a_new_root_directory_appears() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let backend = crate::Backend::new_test();
        *backend.workspace_root().write() = Some(root.to_path_buf());

        // No `resources/views` yet, so no roots are known.
        assert!(backend.laravel_view_roots().is_empty());

        std::fs::create_dir_all(root.join("resources/views")).unwrap();
        let blade_path = root.join("resources/views/welcome.blade.php");
        let content = "<p>hi</p>\n";
        std::fs::write(&blade_path, content).unwrap();
        let uri = crate::util::path_to_uri(&blade_path);
        backend.update_ast(&uri, content);

        let roots = backend.laravel_view_roots();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, root.join("resources/views"));
    }

    #[test]
    fn view_roots_are_not_cached_before_workspace_root_is_known() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("resources/views")).unwrap();

        let backend = crate::Backend::new_test();
        // The workspace root is not known yet: no directory can be found.
        assert!(backend.laravel_view_roots().is_empty());

        *backend.workspace_root().write() = Some(root.to_path_buf());
        let roots = backend.laravel_view_roots();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, root.join("resources/views"));
    }
}
