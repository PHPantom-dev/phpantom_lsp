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
    ///
    /// A fully-qualified reference (`\PATHINFO_ALL`, written that way inside a
    /// namespace to skip the fallback lookup) names the same constant as the
    /// bare form, so the leading separator is dropped before searching.
    pub(crate) fn lookup_global_constant(&self, name: &str) -> Option<Option<String>> {
        self.lookup_global_constant_candidates(&[name.strip_prefix('\\').unwrap_or(name)])
    }

    /// [`Self::lookup_global_constant`] over a list of candidate names, in
    /// the order PHP would try them.
    ///
    /// A constant is indexed under its fully-qualified name, and the name a
    /// reference writes rarely is one: an unqualified `FOO` inside
    /// `namespace App` means `App\FOO` if that exists and global `FOO`
    /// otherwise, and a qualified `Config\FOO` means whatever the file's
    /// `use` table makes of `Config`.  Callers build the candidates with
    /// [`Backend::resolve_constant_name_at`](crate::Backend), which is what
    /// knows the file.
    ///
    /// Each phase is tried for every candidate before the next, more
    /// expensive one runs, so a bare name that exists globally is never
    /// paid for with a full autoload parse looking for the namespaced
    /// spelling.
    pub(crate) fn lookup_global_constant_candidates(
        &self,
        candidates: &[&str],
    ) -> Option<Option<String>> {
        // The stubs record the version of the PHP build they were generated
        // on (`PHP_VERSION_ID` 50306); the one the project runs on is the
        // configured version, down to the minor.
        for name in candidates {
            if let Some(value) = self.php_version_constant(name) {
                return Some(value);
            }
        }

        // Phase 1: already-parsed constants.
        {
            let dmap = self.symbols.global_defines.read();
            for name in candidates {
                if let Some(info) = dmap.get(*name) {
                    return Some(info.value.clone());
                }
            }
        }

        // Phase 2: autoload constant index — lazily parse the file.
        for name in candidates {
            let path = self
                .symbols
                .autoload_constant_index
                .read()
                .get(*name)
                .cloned();
            if let Some(path) = path
                && let Ok(content) = std::fs::read_to_string(&path)
            {
                let file_uri = crate::util::path_to_uri(&path);
                self.update_ast(&file_uri, &content);
                let lookup = self
                    .symbols
                    .global_defines
                    .read()
                    .get(*name)
                    .map(|info| info.value.clone());
                if lookup.is_some() {
                    return lookup;
                }
            }
        }

        // Phase 3: lazily parse known autoload files for constants
        // the byte-level scanner missed (e.g. inside
        // `if (!defined(...))` guards).
        {
            let paths = self.symbols.autoload_file_paths.read().clone();
            for path in &paths {
                let uri = crate::util::path_to_uri(path);
                if self.parsed_uris.read().contains(&uri) {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(path) {
                    self.update_ast(&uri, &content);
                    let dmap = self.symbols.global_defines.read();
                    for name in candidates {
                        if let Some(info) = dmap.get(*name) {
                            return Some(info.value.clone());
                        }
                    }
                }
            }
        }

        // Phase 4: built-in PHP constants from embedded stubs.
        // Parse the stub via update_ast (which populates global_defines),
        // then re-check.  This is the same lazy-parse pattern as Phases
        // 2 and 3 — no special raw-source scanning needed.
        for name in candidates {
            let stub_source = self.stub_constant_index.read().get(*name).copied();
            if let Some(stub_source) = stub_source {
                let stub_uri = format!("phpantom-stub://const/{}", name);
                self.update_ast(&stub_uri, stub_source);
                let lookup = self
                    .symbols
                    .global_defines
                    .read()
                    .get(*name)
                    .map(|info| info.value.clone());
                if lookup.is_some() {
                    return lookup;
                }
                // Stub was parsed but constant not found in global_defines —
                // it exists in the index, so report it with unknown value.
                return Some(None);
            }
        }

        None
    }

    /// The value of a constant that describes the running PHP version, or
    /// `None` when `name` is not one.
    ///
    /// The major and minor versions are the configured ones.  The rest
    /// depend on the patch release, which nothing configures, so they exist
    /// with an unknown value (`Some(None)`).
    fn php_version_constant(&self, name: &str) -> Option<Option<String>> {
        let version = || self.php_version();
        match name {
            "PHP_MAJOR_VERSION" => Some(Some(version().major.to_string())),
            "PHP_MINOR_VERSION" => Some(Some(version().minor.to_string())),
            _ if unversioned_php_version_constant_type(name).is_some() => Some(None),
            _ => None,
        }
    }

    /// The initializer text of a global constant, looked up only where it
    /// costs a map probe or one targeted file parse to find.
    ///
    /// [`Self::lookup_global_constant`] ends by parsing *every* known
    /// autoload file, to catch a `define()` the byte-level scanner skipped
    /// inside an `if (!defined(…))` guard. That is worth paying when the user
    /// hovered one specific name, but not for a speculative lookup: the type
    /// engine asks about names taken out of a docblock type operator
    /// (`key-of<X>`, `X[K]`), where `X` is usually a template parameter and
    /// every miss would charge the whole autoload set.
    pub(crate) fn lookup_indexed_global_constant(&self, name: &str) -> Option<String> {
        if let Some(value) = self
            .symbols
            .global_defines
            .read()
            .get(name)
            .and_then(|info| info.value.clone())
        {
            return Some(value);
        }

        let path = self
            .symbols
            .autoload_constant_index
            .read()
            .get(name)
            .cloned()?;
        let content = std::fs::read_to_string(&path).ok()?;
        self.update_ast(&crate::util::path_to_uri(&path), &content);
        self.symbols
            .global_defines
            .read()
            .get(name)
            .and_then(|info| info.value.clone())
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

/// The type of a PHP version constant whose value depends on the patch
/// release (see `Backend::php_version_constant`), or `None` for any other
/// name.
pub(crate) fn unversioned_php_version_constant_type(
    name: &str,
) -> Option<crate::php_type::PhpType> {
    use crate::php_type::PhpType;
    match name {
        "PHP_VERSION_ID" | "PHP_RELEASE_VERSION" => Some(PhpType::int()),
        "PHP_VERSION" | "PHP_EXTRA_VERSION" => Some(PhpType::string()),
        _ => None,
    }
}
