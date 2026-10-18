//! Laravel path helpers: `base_path('routes/web.php')` and friends.
//!
//! Each helper anchors its argument to a fixed directory under the project
//! root, so the file an argument names is knowable without booting the
//! application.  This module turns a helper call into the path it resolves
//! to; go-to-definition and document links navigate there, and completion
//! lists the entries of the directory the argument is being typed into.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use mago_syntax::cst::*;
use tower_lsp::lsp_types::{Location, Position, Url};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::document_links::normalize_path;
use crate::util::strip_fqn_prefix;

use super::expr_walker::{walk_all_php_expressions, walk_program_expressions};
use super::helpers::extract_string_literal;

/// Every path helper, with the project-root-relative directory it anchors its
/// argument to.  `lang_path` carries the modern directory here; the one case
/// where that is wrong is handled in [`path_helper_base`].
const PATH_HELPERS: [(&str, &str); 8] = [
    ("base_path", ""),
    ("app_path", "app"),
    ("config_path", "config"),
    ("database_path", "database"),
    ("lang_path", "lang"),
    ("public_path", "public"),
    ("resource_path", "resources"),
    ("storage_path", "storage"),
];

/// Whether `name` is one of the path helpers.
///
/// Kept apart from [`path_helper_base`] so a caller on a hot path — completion
/// runs on every keystroke inside a string — can rule the name out before
/// reaching for the workspace root behind its lock.
pub(crate) fn is_path_helper(name: &str) -> bool {
    PATH_HELPERS
        .iter()
        .any(|(helper, _)| name.eq_ignore_ascii_case(helper))
}

/// The directory `helper` anchors its argument to, or `None` when the name is
/// not one of the path helpers.
///
/// `lang_path()` is the only helper whose directory is not fixed by
/// convention alone: Laravel binds it to `resources/lang` when that directory
/// exists and to `lang` in the project root otherwise, which is what keeps
/// applications upgraded from Laravel 8 working.  The same rule applies here.
pub(crate) fn path_helper_base(root: &Path, helper: &str) -> Option<PathBuf> {
    if helper.eq_ignore_ascii_case("lang_path") {
        let legacy = root.join("resources").join("lang");
        return Some(if legacy.is_dir() {
            legacy
        } else {
            root.join("lang")
        });
    }

    PATH_HELPERS
        .iter()
        .find(|(name, _)| helper.eq_ignore_ascii_case(name))
        .map(|(_, dir)| {
            if dir.is_empty() {
                root.to_path_buf()
            } else {
                root.join(dir)
            }
        })
}

/// The absolute path `helper('argument')` names, whether or not anything is
/// there.  A leading separator on the argument is dropped, since PHP
/// concatenates it onto the base directory rather than treating it as a root.
fn path_helper_target(root: &Path, helper: &str, argument: &str) -> Option<PathBuf> {
    let base = path_helper_base(root, helper)?;
    let relative = argument.trim_start_matches(['/', '\\']);
    Some(normalize_path(&base.join(relative)))
}

/// Resolve the path helper argument under the cursor to the file it names.
///
/// Only files are navigated to.  A helper names a directory just as readily,
/// but a `Location` is opened as a document, and an editor asked to open a
/// folder as one reports an error instead of navigating; directories are
/// offered by completion, where they are the useful half of a path segment.
pub(crate) fn resolve_path_helper_definition(
    backend: &Backend,
    content: &str,
    position: Position,
) -> Option<Location> {
    let root = path_helper_root(backend, content)?;
    let cursor_offset = crate::text_position::position_to_offset(content, position) as usize;

    let mut found: Option<(&'static str, String)> = None;
    walk_all_php_expressions(content, &mut |expr| {
        if let Some((helper, argument, start, end)) = path_helper_call(expr, content)
            && cursor_offset >= start
            && cursor_offset <= end
        {
            found = Some((helper, argument.to_string()));
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });

    let (helper, argument) = found?;
    let target = path_helper_target(&root, helper, &argument)?;
    if !target.is_file() {
        return None;
    }

    Some(crate::definition::point_location(
        Url::from_file_path(&target).ok()?,
        Position::new(0, 0),
    ))
}

/// Every path helper argument in the file that names an existing file, as the
/// byte range of the string's contents paired with the file it resolves to.
///
/// Directories are left out for the same reason go-to-definition leaves them
/// out — see [`resolve_path_helper_definition`].
///
/// Takes the caller's already-parsed `Program` rather than parsing again: the
/// document-link handler walks the same file for its include paths, and a
/// second parse of every open file on every edit is the whole cost of the
/// feature paid twice.
pub(crate) fn collect_path_helper_links(
    backend: &Backend,
    content: &str,
    program: &Program<'_>,
) -> Vec<(usize, usize, PathBuf)> {
    let Some(root) = path_helper_root(backend, content) else {
        return Vec::new();
    };

    let mut links = Vec::new();
    walk_program_expressions(program, &mut |expr| {
        if let Some((helper, argument, start, end)) = path_helper_call(expr, content)
            && let Some(target) = path_helper_target(&root, helper, argument)
            && target.is_file()
        {
            links.push((start, end, target));
        }
        ControlFlow::Continue(())
    });
    links
}

/// The project root to resolve path helpers against, or `None` when this file
/// cannot contain one.
///
/// The byte scan comes first so that files with no helper call — the vast
/// majority — never reach the parse.  Outside a Laravel project the names
/// mean whatever the project made them mean, so nothing is resolved there.
fn path_helper_root(backend: &Backend, content: &str) -> Option<PathBuf> {
    if !content.contains("_path(") {
        return None;
    }
    if !backend.resolved_class_cache.read().is_laravel() {
        return None;
    }
    backend.workspace.workspace_root.read().clone()
}

/// The helper name and first string argument of a path helper call, with the
/// byte range of the argument's contents.
fn path_helper_call<'c>(
    expr: &Expression<'_>,
    content: &'c str,
) -> Option<(&'static str, &'c str, usize, usize)> {
    let Expression::Call(Call::Function(call)) = expr else {
        return None;
    };
    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    let name = strip_fqn_prefix(bytes_to_str(ident.value()));
    let helper = PATH_HELPERS
        .iter()
        .map(|(helper, _)| *helper)
        .find(|helper| name.eq_ignore_ascii_case(helper))?;

    let first_arg = call.argument_list.arguments.iter().next()?.value();
    let (argument, start, end) = extract_string_literal(first_arg, content)?;
    Some((helper, argument, start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every path helper call the source holds, as (helper, argument).
    fn helper_calls(content: &str) -> Vec<(&'static str, String)> {
        let mut calls = Vec::new();
        walk_all_php_expressions(content, &mut |expr| {
            if let Some((helper, argument, _, _)) = path_helper_call(expr, content) {
                calls.push((helper, argument.to_string()));
            }
            ControlFlow::Continue(())
        });
        calls
    }

    #[test]
    fn recognises_a_helper_call() {
        assert_eq!(
            helper_calls("<?php\nbase_path('routes/web.php');\n"),
            vec![("base_path", "routes/web.php".to_string())]
        );
        assert_eq!(
            helper_calls("<?php\n\\resource_path('views');\n"),
            vec![("resource_path", "views".to_string())],
            "a root-namespaced call is the same function"
        );
    }

    #[test]
    fn ignores_calls_that_only_look_like_one() {
        assert!(
            helper_calls("<?php\n$helper('routes/web.php');\n").is_empty(),
            "a dynamic callee names no helper we can read"
        );
        assert!(
            helper_calls("<?php\nsprintf('routes/web.php');\n").is_empty(),
            "an unrelated function is left alone"
        );
        assert!(
            helper_calls("<?php\nbase_path($file);\n").is_empty(),
            "a computed argument names no path we can resolve"
        );
        assert!(
            helper_calls("<?php\nbase_path();\n").is_empty(),
            "with no argument the helper names its own directory, not a file"
        );
    }

    #[test]
    fn maps_helpers_to_conventional_directories() {
        let root = Path::new("/project");
        assert_eq!(path_helper_base(root, "base_path"), Some(root.into()));
        assert_eq!(
            path_helper_base(root, "app_path"),
            Some(root.join("app")),
            "app_path anchors to app/"
        );
        assert_eq!(
            path_helper_base(root, "resource_path"),
            Some(root.join("resources"))
        );
        assert_eq!(
            path_helper_base(root, "storage_path"),
            Some(root.join("storage"))
        );
    }

    #[test]
    fn helper_names_are_case_insensitive() {
        let root = Path::new("/project");
        assert_eq!(
            path_helper_base(root, "Config_Path"),
            Some(root.join("config"))
        );
    }

    #[test]
    fn ignores_unrelated_function_names() {
        let root = Path::new("/project");
        assert_eq!(path_helper_base(root, "view_path"), None);
        assert_eq!(path_helper_base(root, "path"), None);
    }

    #[test]
    fn lang_path_falls_back_to_the_project_root() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            path_helper_base(dir.path(), "lang_path"),
            Some(dir.path().join("lang")),
            "a project with no resources/lang uses the root-level lang/"
        );

        std::fs::create_dir_all(dir.path().join("resources/lang")).unwrap();
        assert_eq!(
            path_helper_base(dir.path(), "lang_path"),
            Some(dir.path().join("resources").join("lang")),
            "an upgraded project keeps its resources/lang"
        );
    }

    #[test]
    fn target_joins_the_argument_onto_the_base() {
        let root = Path::new("/project");
        assert_eq!(
            path_helper_target(root, "base_path", "routes/web.php"),
            Some(root.join("routes/web.php"))
        );
        assert_eq!(
            path_helper_target(root, "resource_path", "/views/welcome.blade.php"),
            Some(root.join("resources/views/welcome.blade.php")),
            "a leading separator is concatenation in PHP, not a new root"
        );
        assert_eq!(
            path_helper_target(root, "app_path", "../config/app.php"),
            Some(root.join("config/app.php")),
            "parent segments are resolved rather than left in the URL"
        );
    }
}
