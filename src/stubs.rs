/// Embedded PHP stub support (powered by JetBrains phpstorm-stubs).
///
/// This module provides access to PHP standard library stubs (interfaces,
/// classes, and functions) that are embedded directly into the binary at
/// compile time.  The stubs come from the
/// [phpstorm-stubs](https://github.com/JetBrains/phpstorm-stubs) package.
///
/// ## How it works
///
/// 1. A `build.rs` script parses `PhpStormStubsMap.php` (the index file
///    shipped with phpstorm-stubs) and generates `stub_map_generated.rs`
///    containing:
///    - `STUB_FILES`: an array of every PHP stub file, embedded via
///      `include_str!`.
///    - `STUB_CLASS_MAP`: a `(class_name, file_index)` array mapping
///      class/interface/trait names to indices into `STUB_FILES`.
///    - `STUB_FUNCTION_MAP`: the same for standalone functions.
///
/// 2. At `Backend` construction time, [`build_stub_class_index`] and
///    [`build_stub_function_index`] convert the static arrays into
///    `HashMap`s for O(1) lookup.
///
/// 3. `find_or_load_class` (in `util.rs`) consults the class index as a
///    final fallback (Phase 3) after the `uri_classes_index` and PSR-4 resolution.
///    The stub PHP source is parsed lazily on first access and cached in
///    the `uri_classes_index` under a `phpantom-stub://` URI so subsequent lookups
///    are free.
///
/// ## Updating stubs
///
/// Delete the `stubs/` directory and rebuild. The `build.rs` script will
/// automatically fetch the latest release from GitHub, re-read the map
/// file and re-embed everything.
use std::collections::{HashMap, HashSet};

// Pull in the generated static arrays.
include!(concat!(env!("OUT_DIR"), "/stub_map_generated.rs"));

/// The phpstorm-stubs version that was embedded at build time.
///
/// Set by `build.rs` via `cargo:rustc-env`.  Contains the GitHub release
/// tag (e.g. `"v2025.3"`), `"unknown"` when stubs were present but the
/// version file was missing, or `"none"` when stubs could not be fetched.
pub const STUBS_VERSION: &str = env!("PHPANTOM_STUBS_VERSION");

/// Build a lookup table mapping class/interface/trait short names to their
/// embedded PHP source code.
///
/// Called once during `Backend` construction.  The returned map is stored
/// on the backend and consulted by `find_or_load_class` as a final
/// fallback after the `uri_classes_index` and PSR-4 resolution.
pub fn build_stub_class_index() -> HashMap<&'static str, &'static str> {
    STUB_CLASS_MAP
        .iter()
        .map(|&(name, idx)| (name, STUB_FILES[idx]))
        .collect()
}

/// Build a lookup table mapping function names to their embedded PHP
/// source code.
///
/// This covers both unqualified names (e.g. `"array_map"`) and
/// namespace-qualified names (e.g. `"Brotli\\compress"`).
///
/// Called once during `Backend` construction.  The returned map can be
/// consulted when resolving standalone function calls to provide return
/// type information from stubs.
pub fn build_stub_function_index() -> HashMap<&'static str, &'static str> {
    STUB_FUNCTION_MAP
        .iter()
        .map(|&(name, idx)| (name, STUB_FILES[idx]))
        .collect()
}

/// Short names of the declarations in one stub file whose docblock marks
/// them `@removed` at or before a given PHP version.
#[derive(Debug, Default)]
pub struct RemovedStubNames<'a> {
    pub functions: HashSet<&'a str>,
    pub classes: HashSet<&'a str>,
    pub constants: HashSet<&'a str>,
}

impl RemovedStubNames<'_> {
    /// Whether the symbol `name` (qualified or not) of the given kind is
    /// among the removed declarations.
    pub fn contains(&self, kind: StubSymbolKind, name: &str) -> bool {
        let short = name.rsplit('\\').next().unwrap_or(name);
        match kind {
            StubSymbolKind::Function => self.functions.contains(short),
            StubSymbolKind::Class => self.classes.contains(short),
            StubSymbolKind::Constant => self.constants.contains(short),
        }
    }

    fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.classes.is_empty() && self.constants.is_empty()
    }
}

/// Collect every declaration in a stub file that has been `@removed` at
/// or before `php_version`, in one pass over the file.
///
/// Each `@removed` docblock is attributed to the first declaration
/// (`function`, `class`, `interface`, `trait`, or `define(`) that follows
/// it before the next docblock opens.  Returns `None` when nothing in the
/// file is removed, which is the case for most stub files.
pub fn removed_stub_names(
    source: &str,
    php_version: crate::types::PhpVersion,
) -> Option<RemovedStubNames<'_>> {
    let mut names = RemovedStubNames::default();
    let mut search_from = 0;
    while let Some(rel) = source[search_from..].find("@removed") {
        let tag_pos = search_from + rel;
        let Some(doc_end) = source[tag_pos..].find("*/").map(|p| tag_pos + p + 2) else {
            break;
        };
        search_from = doc_end;
        let Some(doc_start) = source[..tag_pos].rfind("/**") else {
            continue;
        };
        if !docblock_removed_at(&source[doc_start..doc_end], php_version) {
            continue;
        }
        let after = &source[doc_end..];
        let after = &after[..after.find("/**").unwrap_or(after.len())];
        if let Some((kind, name)) = first_declaration(after) {
            match kind {
                StubSymbolKind::Function => names.functions.insert(name),
                StubSymbolKind::Class => names.classes.insert(name),
                StubSymbolKind::Constant => names.constants.insert(name),
            };
        }
    }
    (!names.is_empty()).then_some(names)
}

/// Whether a docblock carries an `@removed X.Y` tag with
/// `php_version >= X.Y`.
fn docblock_removed_at(docblock: &str, php_version: crate::types::PhpVersion) -> bool {
    docblock.lines().any(|line| {
        let trimmed = line.trim().trim_start_matches('*').trim();
        trimmed
            .strip_prefix("@removed")
            .map(str::trim_start)
            .filter(|rest| !rest.is_empty())
            .and_then(crate::types::PhpVersion::from_composer_constraint)
            .is_some_and(|ver| php_version >= ver)
    })
}

/// The kind of stub symbol a declaration introduces.
#[derive(Debug, Clone, Copy)]
pub enum StubSymbolKind {
    Function,
    Class,
    Constant,
}

/// Find the earliest declaration keyword in `text` and return the name
/// it declares.
fn first_declaration(text: &str) -> Option<(StubSymbolKind, &str)> {
    const KEYWORDS: [(&str, StubSymbolKind); 7] = [
        ("function ", StubSymbolKind::Function),
        ("class ", StubSymbolKind::Class),
        ("interface ", StubSymbolKind::Class),
        ("trait ", StubSymbolKind::Class),
        ("define('", StubSymbolKind::Constant),
        ("define(\"", StubSymbolKind::Constant),
        ("function&", StubSymbolKind::Function),
    ];
    let bytes = text.as_bytes();
    let (pos, keyword, kind) = KEYWORDS
        .iter()
        .flat_map(|&(keyword, kind)| {
            text.match_indices(keyword)
                .find(|&(pos, _)| {
                    pos == 0 || !(bytes[pos - 1].is_ascii_alphanumeric() || bytes[pos - 1] == b'_')
                })
                .map(|(pos, _)| (pos, keyword, kind))
        })
        .min_by_key(|&(pos, _, _)| pos)?;
    let rest = text[pos + keyword.len()..].trim_start_matches([' ', '\t', '&']);
    let len = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b >= 0x80)
        .count();
    (len > 0).then(|| (kind, &rest[..len]))
}

/// Build a lookup table mapping constant names to their embedded PHP
/// source code.
///
/// This covers both unqualified names (e.g. `"PHP_EOL"`) and
/// namespace-qualified names (e.g. `"CURL\\CURLOPT_URL"`).
///
/// Called once during `Backend` construction.  The returned map can be
/// consulted when resolving standalone constant references to provide
/// type and value information from stubs.
pub fn build_stub_constant_index() -> HashMap<&'static str, &'static str> {
    STUB_CONSTANT_MAP
        .iter()
        .map(|&(name, idx)| (name, STUB_FILES[idx]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PhpVersion;

    const PHP_7_1: PhpVersion = PhpVersion { major: 7, minor: 1 };
    const PHP_7_2: PhpVersion = PhpVersion { major: 7, minor: 2 };
    const PHP_7_4: PhpVersion = PhpVersion { major: 7, minor: 4 };
    const PHP_8_0: PhpVersion = PhpVersion { major: 8, minor: 0 };
    const PHP_8_4: PhpVersion = PhpVersion { major: 8, minor: 4 };

    fn removed_constants(source: &str, version: PhpVersion) -> Vec<&str> {
        let mut names: Vec<&str> = removed_stub_names(source, version)
            .map(|n| n.constants.into_iter().collect())
            .unwrap_or_default();
        names.sort_unstable();
        names
    }

    #[test]
    fn constant_removed_single_quote() {
        let source = r#"<?php
/**
 * @deprecated 7.1
 * @removed 7.2
 */
define('MCRYPT_ENCRYPT', 0);
"#;
        assert_eq!(removed_constants(source, PHP_7_2), ["MCRYPT_ENCRYPT"]);
        assert_eq!(removed_constants(source, PHP_8_0), ["MCRYPT_ENCRYPT"]);
        assert!(removed_stub_names(source, PHP_7_1).is_none());
    }

    #[test]
    fn constant_removed_double_quote() {
        let source = "<?php\n/**\n * @removed 8.0\n */\ndefine(\"OLD_CONST\", 1);\n";
        assert_eq!(removed_constants(source, PHP_8_0), ["OLD_CONST"]);
        assert!(removed_stub_names(source, PHP_7_4).is_none());
    }

    #[test]
    fn constant_not_removed() {
        let source =
            "<?php\n/**\n * @return int\n */\ndefine('PHP_INT_MAX', 9223372036854775807);\n";
        assert!(removed_stub_names(source, PHP_8_4).is_none());
    }

    #[test]
    fn constant_no_removed_tag_in_file() {
        let source = "<?php\ndefine('SOME_CONST', 42);\n";
        assert!(removed_stub_names(source, PHP_8_4).is_none());
    }

    #[test]
    fn undocumented_constant_does_not_inherit_previous_docblock() {
        let source = "<?php\n/**\n * @removed 8.0\n */\ndefine('ASSERT_QUIET_EVAL', 5);\ndefine('ASSERT_EXCEPTION', 5);\n";
        assert_eq!(removed_constants(source, PHP_8_0), ["ASSERT_QUIET_EVAL"]);
    }

    #[test]
    fn function_removed_basic() {
        let source = "<?php\n/**\n * @removed 7.2\n */\nfunction mcrypt_encrypt() {}\n";
        let names = removed_stub_names(source, PHP_7_2).unwrap();
        assert!(names.functions.contains("mcrypt_encrypt"));
        assert!(removed_stub_names(source, PHP_7_1).is_none());
    }

    #[test]
    fn function_removed_after_attribute() {
        let source =
            "<?php\n/**\n * @removed 8.0\n */\n#[Pure]\nfunction &each(array &$array) {}\n";
        let names = removed_stub_names(source, PHP_8_0).unwrap();
        assert!(names.functions.contains("each"));
    }

    #[test]
    fn class_removed_basic() {
        let source = "<?php\n/**\n * @removed 8.0\n */\nclass OldClass {}\n";
        let names = removed_stub_names(source, PHP_8_0).unwrap();
        assert!(names.classes.contains("OldClass"));
        assert!(removed_stub_names(source, PHP_7_4).is_none());
    }

    #[test]
    fn docblock_without_declaration_is_ignored() {
        let source = "<?php\n/**\n * @removed 7.0\n */\n\n/**\n * Kept.\n */\nfunction kept() {}\n";
        assert!(removed_stub_names(source, PHP_8_0).is_none());
    }
}
