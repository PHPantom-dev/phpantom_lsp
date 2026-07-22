//! Global constant lookup for hover.
//!
//! Resolves a global constant's value through the parsed defines, the
//! autoload constant index, known autoload files, and finally the
//! embedded PHP stubs, lazily parsing files as needed.

use crate::Backend;

impl Backend {
    /// Look up a global constant by name, returning its value if found.
    ///
    /// Searches in order:
    /// 1. `global_defines` — constants already parsed from user files.
    /// 2. `autoload_constant_index` — lazily parses the defining file.
    /// 3. `autoload_file_paths` — last-resort lazy parse of known
    ///    autoload files for constants the byte-level scanner missed.
    /// 4. `stub_constant_index` — built-in PHP constants from stubs.
    ///    Lazily parses the stub file via `update_ast` (which populates
    ///    `global_defines`), then re-checks.
    ///
    /// Returns `Some(Some(val))` when the constant exists with a known
    /// value, `Some(None)` when it exists but the value is unknown, and
    /// `None` when the constant was not found at all.
    pub(crate) fn lookup_global_constant(&self, name: &str) -> Option<Option<String>> {
        // Phase 1: already-parsed constants.
        let lookup = self
            .global_defines
            .read()
            .get(name)
            .map(|info| info.value.clone());
        if lookup.is_some() {
            return lookup;
        }

        // Phase 2: autoload constant index — lazily parse the file.
        let path = self.autoload_constant_index.read().get(name).cloned();
        if let Some(path) = path
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            let file_uri = crate::util::path_to_uri(&path);
            self.update_ast(&file_uri, &content);
            let lookup = self
                .global_defines
                .read()
                .get(name)
                .map(|info| info.value.clone());
            if lookup.is_some() {
                return lookup;
            }
        }

        // Phase 3: lazily parse known autoload files for constants
        // the byte-level scanner missed (e.g. inside
        // `if (!defined(...))` guards).
        {
            let paths = self.autoload_file_paths.read().clone();
            for path in &paths {
                let uri = crate::util::path_to_uri(path);
                if self.parsed_uris.read().contains(&uri) {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(path) {
                    self.update_ast(&uri, &content);
                    let lookup = self
                        .global_defines
                        .read()
                        .get(name)
                        .map(|info| info.value.clone());
                    if lookup.is_some() {
                        return lookup;
                    }
                }
            }
        }

        // Phase 4: built-in PHP constants from embedded stubs.
        // Parse the stub via update_ast (which populates global_defines),
        // then re-check.  This is the same lazy-parse pattern as Phases
        // 2 and 3 — no special raw-source scanning needed.
        let stub_const_idx = self.stub_constant_index.read();
        if let Some(&stub_source) = stub_const_idx.get(name) {
            let stub_uri = format!("phpantom-stub://const/{}", name);
            self.update_ast(&stub_uri, stub_source);
            let lookup = self
                .global_defines
                .read()
                .get(name)
                .map(|info| info.value.clone());
            if lookup.is_some() {
                return lookup;
            }
            // Stub was parsed but constant not found in global_defines —
            // it exists in the index, so report it with unknown value.
            return Some(None);
        }

        None
    }
}

/// Extract the value of a constant from PHP source text.
///
/// Scans for patterns like:
/// - `define('NAME', value)` or `define("NAME", value)`
/// - `const NAME = value;`
///
/// Returns `Some(value_string)` when found, `None` when the constant
/// definition could not be located or the value could not be extracted.
///
/// **Note:** Production code should use `update_ast` to parse constants
/// through the AST pipeline (which populates `global_defines`).  This
/// function exists only for unit tests.
#[cfg(test)]
pub(super) fn extract_constant_value_from_source(name: &str, source: &str) -> Option<String> {
    // Try `define('NAME', value)` pattern.
    for quote in &["'", "\""] {
        let needle = format!("define({quote}{name}{quote}");
        if let Some(pos) = source.find(&needle) {
            // Extract only the second argument.  Stop at the first
            // unquoted comma (third argument) or closing paren,
            // whichever comes first.
            let after = &source[pos + needle.len()..];
            if let Some(comma) = after.find(',') {
                let value_start = &after[comma + 1..];
                let trimmed = value_start.trim_start();
                // Find where the second argument ends: either an
                // unquoted comma (start of optional third arg) or
                // the closing paren.
                let end =
                    find_unquoted_comma(trimmed).or_else(|| find_balanced_close_paren(trimmed));
                if let Some(end) = end {
                    let val = trimmed[..end].trim();
                    if !val.is_empty() {
                        // Empty string literals are placeholders for
                        // runtime-defined values — show the type instead.
                        if val == "''" || val == "\"\"" {
                            return Some("string".to_string());
                        }
                        return Some(val.to_string());
                    }
                }
            }
        }
    }

    // Try `const NAME = value;` pattern.
    let const_needle = format!("const {name}");
    for (i, _) in source.match_indices(&const_needle) {
        let after = &source[i + const_needle.len()..];
        let trimmed = after.trim_start();
        if let Some(rest) = trimmed.strip_prefix('=') {
            let value_part = rest.trim_start();
            if let Some(semi) = value_part.find(';') {
                let val = value_part[..semi].trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }

    None
}

/// Find the position of the first unquoted comma in `s`.
///
/// Skips over single- and double-quoted string literals so that
/// commas inside string values are not mistaken for argument
/// separators.
#[cfg(test)]
fn find_unquoted_comma(s: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    let mut prev = b'\0';

    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'\'' if !in_double && prev != b'\\' => in_single = !in_single,
            b'"' if !in_single && prev != b'\\' => in_double = !in_double,
            b',' if !in_single && !in_double => return Some(i),
            _ => {}
        }
        prev = b;
    }
    None
}

/// Find the position of the closing `)` that matches an implicit
/// opening paren, handling one level of nesting and string literals.
#[cfg(test)]
fn find_balanced_close_paren(s: &str) -> Option<usize> {
    let mut depth = 0u32;
    let mut in_single = false;
    let mut in_double = false;
    let mut prev = b'\0';

    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'\'' if !in_double && prev != b'\\' => in_single = !in_single,
            b'"' if !in_single && prev != b'\\' => in_double = !in_double,
            b'(' if !in_single && !in_double => depth += 1,
            b')' if !in_single && !in_double => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
        prev = b;
    }
    None
}
