//! Goto-definition for standalone constants and functions.
//!
//! Both resolve a list of candidate names against the global symbol maps,
//! lazily parsing the defining file when the byte-level startup scan only
//! recorded its path, and jump to the declaration's stored name offset.

use tower_lsp_server::gen_lsp_types::Location;

use super::point_location;
use crate::Backend;
use crate::resolution::DefineSearch;

impl Backend {
    /// Resolve a standalone constant to its `define('NAME', …)` call or
    /// `const` declaration, found the way the constant's value is.
    ///
    /// A built-in constant from the embedded stubs has no file of its own
    /// to jump to.
    pub(super) fn resolve_constant_definition(&self, candidates: &[String]) -> Option<Location> {
        let candidates: Vec<&str> = candidates.iter().map(String::as_str).collect();
        let found = self.find_or_load_define(&candidates, DefineSearch::Exhaustive, |info| {
            (info.file_uri.clone(), info.name_offset)
        });
        let (file_uri, name_offset) =
            found.filter(|(file_uri, _)| !crate::stubs::is_embedded_stub_uri(file_uri))?;
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
    /// declaration line.  An embedded stub function (with a
    /// `phpantom-stub-fn://` URI) has no file of its own to jump to.
    pub(crate) fn resolve_function_definition(&self, candidates: &[String]) -> Option<Location> {
        let find_function = || {
            let fmap = self.symbols.global_functions.read();
            candidates.iter().find_map(|c| {
                fmap.get(c.as_str())
                    .map(|(uri, info)| (c.as_str(), uri.clone(), info.name_offset))
            })
        };

        // ── Step 1: Check global_functions (user code + cached stubs) ──
        let (_, file_uri, name_offset) = match find_function() {
            Some(found) => found,
            None => {
                // ── Step 2: Try embedded PHP stubs as fallback ──
                let str_candidates: Vec<&str> = candidates.iter().map(|s| s.as_str()).collect();
                self.find_or_load_function(&str_candidates)?;
                // After find_or_load_function, the function is cached in
                // global_functions, a stub under its `phpantom-stub-fn://` URI.
                find_function()?
            }
        };

        if crate::stubs::is_embedded_stub_uri(&file_uri) {
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
