//! Parser for phpstorm-stubs' `PhpStormStubsMap.php`.
//!
//! Shared between `build.rs` (which pulls this file in with
//! `#[path = "src/stub_map.rs"] mod stub_map;`) and the server, so it must
//! depend on nothing but `std`.

use std::collections::BTreeMap;

/// Parse one of the `const CLASSES = array(...)`, `const FUNCTIONS = array(...)`,
/// or `const CONSTANTS = array(...)` sections from the PhpStormStubsMap.php file.
///
/// Returns a `BTreeMap<String, String>` of `symbol_name → relative_file_path`.
pub fn parse_section(content: &str, section_name: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();

    // Find the start: `const SECTION = array (`
    let marker = format!("const {} = array (", section_name);
    let start = match content.find(&marker) {
        Some(pos) => pos + marker.len(),
        None => return map,
    };

    // Walk line by line until we hit `);`
    for line in content[start..].lines() {
        let trimmed = line.trim();
        if trimmed == ");" {
            break;
        }

        // Lines look like:  'ClassName' => 'relative/path.php',
        if let Some(entry) = parse_map_entry(trimmed) {
            map.insert(entry.0, entry.1);
        }
    }

    map
}

/// Parse a single `'key' => 'value',` line.
pub fn parse_map_entry(line: &str) -> Option<(String, String)> {
    // Strip leading whitespace and trailing comma.
    let trimmed = line.trim().trim_end_matches(',');

    // Split on ` => `.
    let (lhs, rhs) = trimmed.split_once(" => ")?;

    // Strip surrounding single quotes.
    let key = lhs.trim().strip_prefix('\'')?.strip_suffix('\'')?;
    let value = rhs.trim().strip_prefix('\'')?.strip_suffix('\'')?;

    // Unescape PHP single-quoted string escapes:
    //   `\\` → `\`   and   `\'` → `'`
    // This is needed because the PhpStormStubsMap.php file uses PHP
    // single-quoted strings where namespace separators are written as
    // `\\` (e.g. `'Couchbase\\GetUserOptions'` → `Couchbase\GetUserOptions`).
    let key = php_unescape_single_quoted(key);
    let value = php_unescape_single_quoted(value);

    Some((key, value))
}

/// Unescape a PHP single-quoted string value.
///
/// PHP single-quoted strings only recognise two escape sequences:
///   - `\\` → `\`
///   - `\'` → `'`
pub fn php_unescape_single_quoted(s: &str) -> String {
    s.replace("\\\\", "\x00")
        .replace("\\'", "'")
        .replace('\x00', "\\")
}
