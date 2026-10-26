//! Indexing the classes inside PHAR archives an autoload file loads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::Backend;
use crate::classmap_scanner::{self, WorkspaceScanResult};
use crate::phar;

/// Files claimed per block of work when scanning a phar archive.
const PHAR_SCAN_BLOCK_FILES: usize = 32;

/// A class found inside a phar: its FQN and the path of the entry
/// declaring it, inside the archive.
type PharClass = (String, PathBuf);

impl Backend {
    /// Parse a `.phar` archive and register its PHP classes in the
    /// fqn_uri_index for lazy loading.
    ///
    /// The phar's raw bytes are read from disk, parsed by
    /// [`phar::PharArchive`], and stored in
    /// [`phar_archives`](crate::Backend::phar_archives).  Each `.php`
    /// file inside the archive is scanned with the lightweight
    /// [`find_classes`](classmap_scanner::find_classes) byte scanner,
    /// and discovered classes are registered in `fqn_uri_index` under a
    /// URI like `phar:///path/to/phpstan.phar/src/Type/Type.php`, the
    /// same URI a parse of the entry caches its classes under.
    /// [`parse_and_cache_uri`](crate::Backend::parse_and_cache_uri)
    /// reads such a URI back out of the archive.
    pub(super) fn scan_phar_archive(&self, phar_path: &Path) {
        // Avoid scanning the same phar twice.
        if self.phar_archives.read().contains_key(phar_path) {
            return;
        }

        // Map the archive rather than copying it onto the heap: the scan
        // below only touches the pages of the `.php` entries it reads, and
        // the map is dropped when this function returns.
        let data = match classmap_scanner::read_for_scan(phar_path) {
            Ok(d) => d,
            Err(e) => {
                self.log_detached(
                    tower_lsp_server::gen_lsp_types::MessageType::Warning,
                    format!("Failed to read phar archive {}: {e}", phar_path.display()),
                );
                return;
            }
        };
        let archive = match phar::PharArchive::parse(phar_path, &data) {
            Some(a) => a,
            None => {
                self.log_detached(
                    tower_lsp_server::gen_lsp_types::MessageType::Warning,
                    format!("Failed to parse phar archive {}", phar_path.display()),
                );
                return;
            }
        };

        // Collect PHP file paths first so we can iterate while
        // holding the archive reference.
        let php_files: Vec<String> = archive
            .file_paths()
            .filter(|p| p.ends_with(".php"))
            .map(String::from)
            .collect();

        // A tool phar is large (phpstan's is tens of megabytes of PHP), so
        // the byte scan is spread over the cores.  The classes come back in
        // manifest order, so the first entry to declare a name wins it, the
        // same as in a sequential scan.
        let scanned: Vec<Vec<PharClass>> = crate::parallel::map_in_blocks(
            "phar-scan",
            &php_files,
            PHAR_SCAN_BLOCK_FILES,
            |internal_path| {
                let content = data.get(archive.file_range(internal_path)?)?;
                let classes: Vec<PharClass> = classmap_scanner::find_classes(content)
                    .into_iter()
                    .map(|fqn| (fqn, PathBuf::from(internal_path)))
                    .collect();
                (!classes.is_empty()).then_some(classes)
            },
        );
        let mut classmap: HashMap<String, PathBuf> = HashMap::new();
        for (fqn, entry) in scanned.into_iter().flatten() {
            classmap.entry(fqn).or_insert(entry);
        }
        let class_count = classmap.len();

        // The archive must be in place before its classes are published:
        // a lookup that finds a class in the index but cannot read its
        // source caches the class as not found.
        self.phar_archives
            .write()
            .insert(phar_path.to_owned(), archive);

        self.symbols
            .merge_phar_scan(WorkspaceScanResult::from_classmap(classmap), phar_path);

        // Clear the negative class cache so that classes previously
        // looked up (and cached as "not found") before the phar was
        // scanned can now be resolved.
        if class_count > 0 {
            self.clear_not_found_caches();
        }

        tracing::info!(
            "scanned phar {}: {} PHP files, {} classes",
            phar_path.display(),
            php_files.len(),
            class_count,
        );
    }
}
