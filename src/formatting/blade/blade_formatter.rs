//! Detecting shufo/blade-formatter and the files it leaves alone.
//!
//! A project formats Blade with blade-formatter when it both depends on
//! the npm package and carries one of the config files the tool reads,
//! and PHPantom runs the project's own `node_modules/.bin/blade-formatter`.
//! Neither signal is enough alone: a project can list the package and
//! hand the CLI its settings with `--config`, which a run on stdin would
//! not see, and a config file can outlive the dependency. Running it with
//! settings the project never wrote would reflow every template.
//!
//! Given stdin, blade-formatter finds its config from its working
//! directory, which is the workspace root when PHPantom runs it, but does
//! not consult `.bladeignore`; the CLI and the VS Code extension both
//! skip the files it lists, so PHPantom does too.

use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// The config files blade-formatter looks for, in its own order.
const CONFIG_FILES: [&str; 2] = [".bladeformatterrc.json", ".bladeformatterrc"];

/// The npm package, as `package.json` lists it.
const PACKAGE: &str = "blade-formatter";

/// The binary npm links into `node_modules/.bin`.
const BINARY: &str = if cfg!(windows) {
    "blade-formatter.cmd"
} else {
    "blade-formatter"
};

/// Whether the project at `workspace_root` formats Blade with
/// blade-formatter: its `package.json` lists the package and the root
/// holds a blade-formatter config file.
pub(in crate::formatting) fn uses_blade_formatter(workspace_root: &Path) -> bool {
    CONFIG_FILES
        .iter()
        .any(|name| workspace_root.join(name).is_file())
        && package_json_lists_it(workspace_root)
}

fn package_json_lists_it(workspace_root: &Path) -> bool {
    let Ok(source) = std::fs::read_to_string(workspace_root.join("package.json")) else {
        return false;
    };
    let Ok(package) = serde_json::from_str::<serde_json::Value>(&source) else {
        return false;
    };
    ["devDependencies", "dependencies"].iter().any(|section| {
        package
            .get(section)
            .and_then(|deps| deps.get(PACKAGE))
            .is_some()
    })
}

/// The blade-formatter binary the project installed.
pub(super) fn binary(workspace_root: &Path) -> Option<PathBuf> {
    let candidate = workspace_root.join("node_modules/.bin").join(BINARY);
    candidate.is_file().then_some(candidate)
}

/// The workspace `.bladeignore`, when there is one with any patterns.
pub(super) fn load_ignore(workspace_root: &Path) -> Option<Gitignore> {
    let path = workspace_root.join(".bladeignore");
    if !path.is_file() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(workspace_root);
    if let Some(e) = builder.add(&path) {
        tracing::warn!("blade-formatter: skipping part of {}: {e}", path.display());
    }
    builder
        .build()
        .ok()
        .filter(|ignore| ignore.num_ignores() + ignore.num_whitelists() > 0)
}

/// Whether `.bladeignore` excludes `file_path`.
pub(super) fn is_ignored(ignore: &Gitignore, file_path: &Path) -> bool {
    file_path.starts_with(ignore.path())
        && ignore
            .matched_path_or_any_parents(file_path, false)
            .is_ignore()
}
