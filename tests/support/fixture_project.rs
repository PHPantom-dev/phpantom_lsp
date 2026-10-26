//! The fixture projects the ported PHPStan extension suites run in.
//!
//! `tests/phpstan_symfony/` and `tests/phpstan_doctrine/` each hold a
//! `composer.json`, the slice of the framework their fixtures touch (copied
//! by `scripts/extract-framework-stubs.php`), and the configuration a booted
//! application would answer from. A fixture is analysed in a backend rooted
//! at its project, after the startup and workspace index the server runs,
//! so the project's classes and configuration files are indexed the way an
//! editor would index them.

use std::path::{Path, PathBuf};

use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::InitializedParams;

/// A backend with the embedded stubs, rooted at the project `root` and
/// with its workspace indexed.
pub fn project_backend(root: &Path) -> Backend {
    let backend = Backend::new_test_with_full_stubs();
    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(root);
    *backend.workspace_root().write() = Some(root.to_path_buf());
    *backend.psr4_mappings().write() = mappings;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .thread_stack_size(phpantom_lsp::PARSE_WORKER_STACK_SIZE)
        .build()
        .expect("build test runtime")
        .block_on(backend.initialized(InitializedParams {}));
    // A test backend has no client, so `initialized` leaves out the
    // background index that reads the configuration files.
    backend.ensure_workspace_indexed();
    backend
}

/// `path` made absolute against the test's working directory (the crate
/// root).
pub fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .expect("the test should have a current directory")
            .join(path)
    }
}
