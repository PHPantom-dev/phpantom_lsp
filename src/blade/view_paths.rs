//! Where a project's Blade templates live: the view roots `config/view.php`
//! configures, or Laravel's conventional `resources/views` when it names
//! none.

use std::path::{Path, PathBuf};

/// Discover Laravel Blade view directories from `config/view.php`.
///
/// Parses the `'paths'` array in the config file to extract directory
/// paths.  Falls back to `resources/views` if the config file is
/// missing or unparseable.  Returns only directories that exist.
pub fn discover_view_paths(workspace_root: &Path) -> Vec<PathBuf> {
    let config_path = workspace_root.join("config/view.php");
    let paths = if config_path.is_file() {
        parse_view_config_paths(&config_path, workspace_root)
    } else {
        Vec::new()
    };

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

impl crate::Backend {
    /// The configured Blade view root directories.
    ///
    /// Reads the `paths` array from `config/view.php` (falling back to
    /// the conventional `resources/views`) so that projects with custom
    /// view directories resolve `view()` names correctly. Only existing
    /// directories are returned. Read from disk, so unsaved edits to
    /// `config/view.php` are not reflected until saved.
    pub(crate) fn laravel_view_roots(&self) -> Vec<PathBuf> {
        match self.workspace.workspace_root.read().clone() {
            Some(root) => discover_view_paths(&root),
            None => Vec::new(),
        }
    }
}

/// Parse `config/view.php` to extract the `'paths'` array entries.
///
/// Looks for string literals inside `'paths' => [...]` and resolves
/// `base_path('...')` calls relative to the workspace root.
fn parse_view_config_paths(config_path: &Path, workspace_root: &Path) -> Vec<PathBuf> {
    let content = match std::fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

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
        } else if let Some(path) = extract_string_literal(trimmed) {
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
        return extract_string_literal(arg).map(|p| p.to_string());
    }

    if let Some(rest) = inner.strip_prefix("resource_path(") {
        let arg = rest.strip_suffix(')')?.trim();
        if arg.is_empty() {
            return Some("resources".to_string());
        }
        return extract_string_literal(arg).map(|p| format!("resources/{p}"));
    }

    None
}

/// Extract content from a single- or double-quoted PHP string literal.
fn extract_string_literal(s: &str) -> Option<&str> {
    let s = s.trim();
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        Some(&s[1..s.len() - 1])
    } else {
        None
    }
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
}
