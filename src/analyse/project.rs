//! Opening a project for a command-line run.
//!
//! Every subcommand that works on a whole project (`analyze`, `fix`,
//! `move`) starts the way the LSP server's `initialized` handler does:
//! read the project's `composer.json`, settle on a PHP version, and run
//! the indexing pipeline on a headless `Backend`. This is that sequence,
//! written once, so a new subcommand cannot drift from the others in
//! which version it analyses against or what it indexes.

use std::path::Path;

use crate::types::PhpVersion;
use crate::{Backend, composer, config};

/// Load the project's `.phpantom.toml` (merged over `global_config`), or
/// fall back to the defaults with a note on stderr when it does not parse.
///
/// A broken config should not stop a command-line run, but it should not
/// go unmentioned either: the run would otherwise silently ignore every
/// setting the project relies on.
pub(crate) fn load_config_or_default(root: &Path, global_config: Option<&Path>) -> config::Config {
    match config::load_config_from(root, global_config) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Warning: failed to load .phpantom.toml: {e}");
            config::Config::default()
        }
    }
}

/// Point `backend` at the project under `root`, configured by `cfg`, and
/// run the same indexing pipeline the LSP server runs on `initialized`.
///
/// The PHP version comes from the config when it names one, then from the
/// `php` constraint in `composer.json`, then the default. A project with
/// no `composer.json` still opens: classes are found by scanning the tree.
pub(crate) async fn open_headless_project(backend: &Backend, root: &Path, cfg: config::Config) {
    let composer_package = composer::read_composer_package(root);
    let php_version = cfg
        .php
        .version
        .as_deref()
        .and_then(PhpVersion::from_composer_constraint)
        .or_else(|| {
            composer_package
                .as_ref()
                .and_then(composer::detect_php_version_from_package)
        })
        .unwrap_or_default();

    *backend.workspace_root().write() = Some(root.to_path_buf());
    backend.set_config(cfg);
    backend.set_php_version(php_version);
    backend
        .init_single_project(root, php_version, composer_package, None)
        .await;
}
