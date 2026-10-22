//! Goto-definition for standalone constants and functions.
//!
//! Both resolve a list of candidate names against the global symbol maps,
//! lazily parsing the defining file when the byte-level startup scan only
//! recorded its path, and jump to the declaration's stored name offset.

use tower_lsp::lsp_types::Location;

use super::point_location;
use crate::Backend;
use crate::resolution::DefineSearch;

/// The value stored under the first candidate name `lookup` finds.
fn first_candidate<'m, T>(
    candidates: &[String],
    lookup: impl Fn(&str) -> Option<&'m T>,
) -> Option<&'m T> {
    candidates.iter().find_map(|c| lookup(c.as_str()))
}

impl Backend {
    /// Resolve a standalone constant to its `define('NAME', …)` call or
    /// `const` declaration, found the way the constant's value is.
    ///
    /// Built-in constants from `stub_constant_index` are not navigable
    /// (they have no real file).
    pub(super) fn resolve_constant_definition(&self, candidates: &[String]) -> Option<Location> {
        let candidates: Vec<&str> = candidates.iter().map(String::as_str).collect();
        let (file_uri, name_offset) =
            self.find_or_load_define(&candidates, DefineSearch::Exhaustive, |info| {
                (info.file_uri.clone(), info.name_offset)
            })?;
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
        if file_uri.starts_with(crate::stubs::STUB_FUNCTION_URI_PREFIX) {
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
        Some(point_location(crate::util::parse_url(file_uri)?, position))
    }
}
