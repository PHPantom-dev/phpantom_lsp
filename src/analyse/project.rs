//! Opening a project for a command-line run.
//!
//! Every subcommand that works on a whole project (`analyze`, `fix`,
//! `move`, `format`) starts the way the LSP server's `initialized`
//! handler does: read the project's `composer.json`, settle on a PHP
//! version, and set up a headless `Backend`. This is that sequence,
//! written once, so a new subcommand cannot drift from the others in
//! which version it analyses against or what it indexes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

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
    let php_version = project_php_version(&cfg, composer_package.as_ref());

    *backend.workspace_root().write() = Some(root.to_path_buf());
    backend.set_config(cfg);
    backend.set_php_version(php_version);
    backend
        .init_single_project(root, php_version, composer_package, None)
        .await;
}

/// Point `backend` at the project under `root` without indexing it.
///
/// `format` asks the backend only where the project keeps its code and
/// which PHP version to format for; it never looks a class up. Building
/// the class index would be the dominant cost of the run and would answer
/// neither question, so this stops after the project's shape: config, PHP
/// version, PSR-4 source directories, and the vendor directory to skip.
pub(crate) fn open_headless_project_unindexed(backend: &Backend, root: &Path, cfg: config::Config) {
    let composer_package = composer::read_composer_package(root);
    let php_version = project_php_version(&cfg, composer_package.as_ref());

    *backend.workspace_root().write() = Some(root.to_path_buf());
    backend.set_config(cfg);
    backend.set_php_version(php_version);
    backend.init_autoload_paths(root, composer_package.as_ref());
}

/// The PHP version a command-line run works against: the one the config
/// names, then the `php` constraint in `composer.json`, then the default.
fn project_php_version(
    cfg: &config::Config,
    composer_package: Option<&composer::ComposerPackage>,
) -> PhpVersion {
    cfg.php
        .version
        .as_deref()
        .and_then(PhpVersion::from_composer_constraint)
        .or_else(|| composer_package.and_then(composer::detect_php_version_from_package))
        .unwrap_or_default()
}

/// Parse every file in `files` on parallel workers, populating the
/// backend's per-file indexes, and hand back each file's URI and content
/// at the same index so the caller's next phase can reuse them without
/// re-reading.  A file that cannot be read is `None`.
///
/// The workers get [`crate::PARSE_WORKER_STACK_SIZE`]: the parser is
/// recursive and overflows the 2 MB default a spawned thread otherwise
/// has.  With `trace`, each file is named on stderr as it is parsed.
pub(crate) fn parse_user_files(
    backend: &Backend,
    root: &Path,
    files: &[PathBuf],
    trace: bool,
) -> Vec<Option<(String, String)>> {
    let file_count = files.len();
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let next_idx = AtomicUsize::new(0);

    std::thread::scope(|s| {
        let handles: Vec<_> = (0..n_threads)
            .map(|worker| {
                let next_idx = &next_idx;
                std::thread::Builder::new()
                    .name("index-worker".into())
                    .stack_size(crate::PARSE_WORKER_STACK_SIZE)
                    .spawn_scoped(s, move || {
                        let mut entries: Vec<(usize, String, String)> = Vec::new();
                        loop {
                            let i = next_idx.fetch_add(1, Ordering::Relaxed);
                            if i >= file_count {
                                break;
                            }
                            let file_path = &files[i];
                            if trace {
                                let display =
                                    file_path.strip_prefix(root).unwrap_or(file_path).display();
                                eprintln!("[w{worker:02}] parse {display}");
                            }
                            let Ok(content) = std::fs::read_to_string(file_path) else {
                                continue;
                            };
                            let uri = crate::util::path_to_uri(file_path);
                            backend.update_ast(&uri, &content);
                            entries.push((i, uri, content));
                        }
                        entries
                    })
                    .expect("failed to spawn index-worker thread")
            })
            .collect();

        let mut indexed: Vec<Option<(String, String)>> = (0..file_count).map(|_| None).collect();
        for handle in handles {
            for (i, uri, content) in handle.join().unwrap_or_default() {
                indexed[i] = Some((uri, content));
            }
        }
        indexed
    })
}

/// Discover what a Laravel project registers through its service
/// providers, the way the LSP's `initialized` handler does once the
/// workspace is indexed.
///
/// Must run after [`parse_user_files`]: discovery reads the project's own
/// providers.  Without it the `now()`/`today()` helpers and the Date
/// facade resolve to nothing, `config()`/`view()`/`trans()`/`route()`
/// string keys are all unknown, and morph aliases, gate abilities, Artisan
/// command names, and vendor-registered macros read as invalid, so a run
/// would report (or fix against) false positives.  A project that is not
/// Laravel has nothing to discover.
pub(crate) fn discover_laravel_resources(backend: &Backend) {
    if !backend.resolved_class_cache.read().is_laravel() {
        return;
    }
    backend.build_laravel_date_class();
    backend.build_provider_resources();
    backend.build_laravel_morph_map_index();
    backend.build_laravel_gate_index();
    // `update_ast` only refreshes these from the files it parses, which
    // here is the project's own source, so without a scan of the whole
    // FQN index the vendor entries are missing.
    backend.build_laravel_command_index();
    backend.build_laravel_macro_index();
}
