//! Goto-definition for standalone constants and functions.
//!
//! Both resolve a list of candidate names against the global symbol maps,
//! lazily parsing the defining file when the byte-level startup scan only
//! recorded its path, and jump to the declaration's stored name offset.

use tower_lsp::lsp_types::{Location, Url};

use super::point_location;
use crate::Backend;

/// The value stored under the first candidate name `lookup` finds.
fn first_candidate<'m, T>(
    candidates: &[String],
    lookup: impl Fn(&str) -> Option<&'m T>,
) -> Option<&'m T> {
    candidates.iter().find_map(|c| lookup(c.as_str()))
}

impl Backend {
    /// Resolve a standalone constant to its `define('NAME', …)` call site.
    ///
    /// Checks `global_defines` (user-defined constants discovered from parsed
    /// files) for a matching constant name, reads the source file, and returns
    /// a `Location` pointing at the `define(` call.  When not found, checks
    /// the `autoload_constant_index` (populated by the full-scan for
    /// non-Composer projects) and lazily parses the defining file via
    /// `update_ast`.  Built-in constants from `stub_constant_index` are not
    /// navigable (they have no real file).
    pub(super) fn resolve_constant_definition(&self, candidates: &[String]) -> Option<Location> {
        let find_define = || {
            let dmap = self.symbols.global_defines.read();
            first_candidate(candidates, |c| dmap.get(c))
                .map(|info| (info.file_uri.clone(), info.name_offset))
        };

        // ── Phase 1: Look up the constant in global_defines. ──
        let found = find_define();

        // ── Phase 1.5: Check autoload_constant_index (byte-level scan). ──
        // The lightweight `find_symbols` byte-level scan discovers
        // constant names at startup without a full AST parse, for both
        // non-Composer projects (workspace scan) and Composer projects
        // (autoload_files.php scan).  When a candidate matches, we
        // lazily call `update_ast` to get the complete `DefineInfo`
        // and re-check global_defines.
        let found = found.or_else(|| {
            let path = {
                let idx = self.symbols.autoload_constant_index.read();
                first_candidate(candidates, |c| idx.get(c))?.clone()
            };
            let content = std::fs::read_to_string(&path).ok()?;
            self.update_ast(&crate::util::path_to_uri(&path), &content);
            find_define()
        });

        // ── Phase 1.75: Last-resort lazy parse of known autoload files ──
        // The byte-level scanner misses constants inside conditional
        // blocks (e.g. `if (!defined(...))` guards).  As a safety net,
        // lazily parse each known autoload file via `update_ast` until
        // the constant is found.  Each file is parsed at most once:
        // subsequent lookups hit Phase 1 (`global_defines`).
        let found = found.or_else(|| {
            let paths = self.symbols.autoload_file_paths.read().clone();
            paths.iter().find_map(|path| {
                let uri = crate::util::path_to_uri(path);
                if self.parsed_uris.read().contains(&uri) {
                    return None;
                }
                let content = std::fs::read_to_string(path).ok()?;
                self.update_ast(&uri, &content);
                find_define()
            })
        });

        let (file_uri, name_offset) = found?;
        self.stored_offset_location(&file_uri, name_offset)
    }

    /// Try to resolve a standalone function name to its definition.
    ///
    /// Searches the `global_functions` map (populated from autoload files,
    /// opened/changed files, and cached stub functions) for any of the
    /// given candidate names.  If not found there, falls back to the
    /// embedded PHP stubs via `find_or_load_function` — which parses the
    /// stub lazily and caches it in `global_functions` for future lookups.
    ///
    /// When found, reads the source file and locates the `function name(`
    /// declaration line.  Stub functions (with `phpantom-stub-fn://` URIs)
    /// are not navigable so they are skipped for go-to-definition but
    /// still loaded into the cache for return-type resolution.
    pub(crate) fn resolve_function_definition(&self, candidates: &[String]) -> Option<Location> {
        let find_function = || {
            let fmap = self.symbols.global_functions.read();
            first_candidate(candidates, |c| fmap.get(c))
                .map(|(uri, info)| (uri.clone(), info.name_offset))
        };

        // ── Step 1: Check global_functions (user code + cached stubs) ──
        let (file_uri, name_offset) = match find_function() {
            Some(found) => found,
            None => {
                // ── Step 2: Try embedded PHP stubs as fallback ──
                let str_candidates: Vec<&str> = candidates.iter().map(|s| s.as_str()).collect();
                self.find_or_load_function(&str_candidates)?;
                // After find_or_load_function, the function is cached in
                // global_functions.  When it isn't, it came from a stub,
                // which has no location to jump to.
                find_function()?
            }
        };

        // Stub functions don't have real file locations — skip
        // go-to-definition for them (they're still useful for return-type
        // resolution via the function_loader).
        if file_uri.starts_with("phpantom-stub-fn://") {
            return None;
        }

        self.stored_offset_location(&file_uri, name_offset)
    }

    /// A point location at a declaration's stored name offset in
    /// `file_uri` (open files first, then disk).
    ///
    /// An offset of 0 means "not available" and yields `None`; the parser
    /// always sets it for symbols discovered via `update_ast`.
    fn stored_offset_location(&self, file_uri: &str, name_offset: u32) -> Option<Location> {
        let file_content = self.get_file_content(file_uri)?;
        if name_offset == 0 {
            return None;
        }
        let position =
            crate::text_position::offset_to_position(&file_content, name_offset as usize);
        Some(point_location(Url::parse(file_uri).ok()?, position))
    }
}
