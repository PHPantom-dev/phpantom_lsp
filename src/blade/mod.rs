pub mod directives;
pub mod preprocessor;
pub mod source_map;

use std::path::{Path, PathBuf};

/// Number of lines the Blade preprocessor injects as a prologue
/// (<?php header, $errors declaration, $__env declaration, wrapper function, etc.).
pub const PROLOGUE_LINES: u32 = 6;

/// Check whether a URI refers to a Blade template file.
pub fn is_blade_file(uri: &str) -> bool {
    uri.ends_with(".blade.php")
}

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

    // Match `base_path('...')` or `realpath(base_path('...'))`.
    for segment in array_content.split(',') {
        let trimmed = segment.trim();
        if let Some(path) = extract_base_path_arg(trimmed) {
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

/// Extract the string argument from `base_path('...')` or
/// `realpath(base_path('...'))`.
fn extract_base_path_arg(s: &str) -> Option<&str> {
    // Strip optional `realpath(` wrapper.
    let inner = if let Some(rest) = s.strip_prefix("realpath(") {
        rest.strip_suffix(')')?.trim()
    } else {
        s
    };

    let rest = inner.strip_prefix("base_path(")?.strip_suffix(')')?.trim();
    extract_string_literal(rest)
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

use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

impl crate::Backend {
    /// If the cursor is on a `{{`, `}}`, `{!!`, or `!!}` Blade echo delimiter,
    /// return a hover describing the implicit `e()` call (for escaped echo)
    /// or raw output (for unescaped echo).
    pub(crate) fn blade_echo_delimiter_hover(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<Hover> {
        let content = self.get_file_content(uri)?;
        let line = content.lines().nth(position.line as usize)?;
        let col = position.character as usize;

        // Check if cursor is on `{{` (escaped echo open)
        if col < line.len()
            && line.get(col..col + 2) == Some("{{")
            && line.get(col..col + 3) != Some("{!!")
        {
            return Some(self.blade_e_hover(position, 2));
        }
        // Also match if cursor is on the second `{` of `{{`
        if col > 0
            && line.get(col - 1..col + 1) == Some("{{")
            && (col < 2 || line.get(col - 1..col + 2) != Some("{!!"))
        {
            return Some(self.blade_e_hover(
                Position {
                    line: position.line,
                    character: (col - 1) as u32,
                },
                2,
            ));
        }
        // `}}` closing delimiter
        if col < line.len()
            && line.get(col..col + 2) == Some("}}")
            && (col == 0 || line.as_bytes().get(col - 1) != Some(&b'!'))
        {
            return Some(self.blade_e_hover(position, 2));
        }
        if col > 0
            && line.get(col - 1..col + 1) == Some("}}")
            && (col < 2 || line.as_bytes().get(col - 2) != Some(&b'!'))
        {
            return Some(self.blade_e_hover(
                Position {
                    line: position.line,
                    character: (col - 1) as u32,
                },
                2,
            ));
        }

        None
    }

    /// Build hover content for `{{ }}` (escaped echo via `e()`).
    fn blade_e_hover(&self, start: Position, len: u32) -> Hover {
        // Try to resolve the actual `e()` function from the project/stubs.
        let empty_use_map = std::collections::HashMap::new();
        let loader = self.function_loader_with(&empty_use_map, &None);
        let content = if let Some(func) = loader("e") {
            crate::hover::hover_for_function(&func, None, None, false).contents
        } else {
            HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "Blade escaped echo. Output is passed through `e()` (`htmlspecialchars`).\n\n\
                    ```php\n<?php\nfunction e(mixed $value, bool $doubleEncode = true): string;\n```"
                    .to_string(),
            })
        };
        Hover {
            contents: content,
            range: Some(Range {
                start,
                end: Position {
                    line: start.line,
                    character: start.character + len,
                },
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_blade_file_by_extension() {
        assert!(is_blade_file("file:///app/views/welcome.blade.php"));
        assert!(!is_blade_file("file:///app/controllers/Home.php"));
    }

    #[test]
    fn test_is_blade_file_by_language_id() {
        let backend = crate::Backend::test_defaults();
        // Not blade by extension
        let uri = "file:///app/views/welcome.php";
        assert!(!backend.is_blade_file(uri));

        // Register via language_id
        backend.blade_uris.write().insert(uri.to_string());
        assert!(backend.is_blade_file(uri));
    }
}
