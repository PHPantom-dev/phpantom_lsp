//! Embedded PHP stub support (powered by JetBrains phpstorm-stubs).
//!
//! This module provides access to PHP standard library stubs (interfaces,
//! classes, and functions) that are embedded directly into the binary at
//! compile time.  The stubs come from the
//! [phpstorm-stubs](https://github.com/JetBrains/phpstorm-stubs) package.
//!
//! ## How it works
//!
//! 1. A `build.rs` script parses `PhpStormStubsMap.php` (the index file
//!    shipped with phpstorm-stubs) and generates `stub_map_generated.rs`
//!    containing:
//!    - `STUB_FILES`: an array of every PHP stub file, embedded via
//!      `include_str!`.
//!    - `STUB_CLASS_MAP`: a `(class_name, file_index)` array mapping
//!      class/interface/trait names to indices into `STUB_FILES`.
//!    - `STUB_FUNCTION_MAP`: the same for standalone functions.
//!    - `STUB_FILE_EXTENSIONS`: the extension directory of each file.
//!
//! 2. At `Backend` construction time, [`build_stub_class_index`] and
//!    [`build_stub_function_index`] convert the static arrays into
//!    `HashMap`s for O(1) lookup.
//!
//! 3. `find_or_load_class` (in `resolution/classes.rs`) consults the class
//!    index for built-in names ahead of the classmap, and for everything
//!    else as a final fallback after the `uri_classes_index` and PSR-4
//!    resolution.  The stub PHP source is parsed lazily on first access and
//!    cached in the `uri_classes_index` under a [`STUB_URI_PREFIX`] URI so
//!    subsequent lookups are free.
//!
//! ## Updating stubs
//!
//! Delete the `stubs/` directory and rebuild. The `build.rs` script will
//! automatically fetch the latest release from GitHub, re-read the map
//! file and re-embed everything.

use crate::ci_map::CiMap;
use std::collections::{HashMap, HashSet};

// Pull in the generated static arrays.
include!(concat!(env!("OUT_DIR"), "/stub_map_generated.rs"));

/// The phpstorm-stubs version that was embedded at build time.
///
/// Set by `build.rs` via `cargo:rustc-env`.  Contains the pinned commit
/// (e.g. `"master@2049ca3cfa"`), `"external"` when the stubs came from
/// `PHPANTOM_STUBS_DIR`, or `"none"` when the `offline-stubs` feature
/// built without them.
pub const STUBS_VERSION: &str = env!("PHPANTOM_STUBS_VERSION");

/// Warning shown when the binary embeds no stubs, so a broken package
/// says why everything built in is reported as unknown.
pub(crate) const MISSING_STUBS_WARNING: &str = "PHPantom was built without the PHP standard library stubs, so every built-in function, class, and constant is reported as unknown. Rebuild it with network access, or with PHPANTOM_STUBS_DIR pointing at phpstorm-stubs.";

/// Whether the binary was built with the phpstorm-stubs embedded.
pub(crate) fn has_embedded_stubs() -> bool {
    !STUB_FILES.is_empty()
}

/// URI prefix an embedded class or constant stub is cached under, as in
/// `phpantom-stub://PDO` or `phpantom-stub://const/PHP_EOL`.
pub(crate) const STUB_URI_PREFIX: &str = "phpantom-stub://";

/// URI prefix the functions and classes of an embedded function stub file
/// are cached under, as in `phpantom-stub-fn://array_map`.
pub(crate) const STUB_FUNCTION_URI_PREFIX: &str = "phpantom-stub-fn://";

/// Whether `uri` names a stub embedded in the binary rather than a file.
///
/// These are the declarations [`crate::stub_patches`] corrects.
pub(crate) fn is_embedded_stub_uri(uri: &str) -> bool {
    uri.starts_with(STUB_URI_PREFIX) || uri.starts_with(STUB_FUNCTION_URI_PREFIX)
}

/// Whether `uri` names a stub, from whichever source, rather than project
/// or library code.
///
/// A stub describes declarations nobody runs, so it is never analysed,
/// searched for references, or edited.
pub(crate) fn is_stub_uri(uri: &str) -> bool {
    is_embedded_stub_uri(uri)
}

/// Extensions whose phpstorm-stubs directory describes code shipped with
/// php-src (the engine, its SAPIs, and every extension bundled with it,
/// including ones since moved to PECL).
///
/// Their parameter names are fixed by the PHP version, which the stubs
/// track.  Every other directory stubs a separately released extension
/// whose parameter names change from one of its own releases to the next
/// (the `RedisSentinel` constructor took `$options_or_host` in redis 5 and
/// `$options` in 6), so the stub cannot be trusted to name them right for
/// the version a project runs.
const BUNDLED_EXTENSIONS: &[&str] = &[
    "apache",
    "bcmath",
    "bz2",
    "calendar",
    "com_dotnet",
    "core",
    "ctype",
    "curl",
    "date",
    "dba",
    "dom",
    "enchant",
    "exif",
    "ffi",
    "fileinfo",
    "filter",
    "fpm",
    "ftp",
    "gd",
    "gettext",
    "gmp",
    "hash",
    "iconv",
    "imap",
    "intl",
    "json",
    "ldap",
    "libxml",
    "litespeed",
    "mbstring",
    "mysqli",
    "oci8",
    "odbc",
    "openssl",
    "pcntl",
    "pcre",
    "pdo",
    "pgsql",
    "phar",
    "phpdbg",
    "posix",
    "pspell",
    "random",
    "readline",
    "reflection",
    "session",
    "shmop",
    "simplexml",
    "snmp",
    "soap",
    "sockets",
    "sodium",
    "spl",
    "sqlite3",
    "standard",
    "sysvmsg",
    "sysvsem",
    "sysvshm",
    "tidy",
    "tokenizer",
    "uri",
    "xml",
    "xmlreader",
    "xmlwriter",
    "xsl",
    "zend opcache",
    "zip",
    "zlib",
];

/// Whether `content`, parsed under `uri`, is the phpstorm-stubs file of an
/// extension php-src does not ship (see [`BUNDLED_EXTENSIONS`]).
///
/// An embedded file is recognised by its address, since every caller hands
/// the embedded `&'static str` on unchanged; a checkout under a stub path
/// by the directory its file sits in.
pub(crate) fn is_unbundled_extension_stub(uri: &str, content: &str) -> bool {
    let extension = STUB_FILES
        .iter()
        .position(|file| std::ptr::eq(file.as_ptr(), content.as_ptr()))
        .map(|i| STUB_FILE_EXTENSIONS[i])
        .or_else(|| {
            if is_embedded_stub_uri(uri) {
                return None;
            }
            let mut segments = uri.rsplit('/');
            segments.next();
            segments.next()
        });
    extension.is_some_and(|ext| !is_bundled_extension(ext))
}

fn is_bundled_extension(extension: &str) -> bool {
    let extension = extension.replace("%20", " ");
    BUNDLED_EXTENSIONS
        .iter()
        .any(|bundled| bundled.eq_ignore_ascii_case(&extension))
}

/// Whether `content` is the embedded phpstorm-stubs file of an extension
/// php-src ships, recognised by its address like in
/// [`is_unbundled_extension_stub`].
///
/// Only there does an `@since` tag name the PHP version a declaration was
/// added in.  An unbundled extension's stub tags the extension's own
/// releases, and any other stub (a library's, WordPress') its own.
pub(crate) fn is_bundled_extension_stub_file(content: &str) -> bool {
    STUB_FILES
        .iter()
        .position(|file| std::ptr::eq(file.as_ptr(), content.as_ptr()))
        .is_some_and(|i| is_bundled_extension(STUB_FILE_EXTENSIONS[i]))
}

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
/// (`function`, `class`, `interface`, `trait`, `enum`, `const`, or
/// `define(`) that follows it before the next docblock opens.  Returns
/// `None` when nothing in the file is removed, which is the case for most
/// stub files.
pub fn removed_stub_names(
    source: &str,
    php_version: crate::types::PhpVersion,
) -> Option<RemovedStubNames<'_>> {
    let mut names = RemovedStubNames::default();
    for_each_tagged_declaration(source, "@removed", |docblock, declared| {
        let Declared::Symbol(kind, name) = declared else {
            return;
        };
        if !docblock_removed_at(docblock, php_version) {
            return;
        }
        match kind {
            StubSymbolKind::Function => names.functions.insert(name),
            StubSymbolKind::Class => names.classes.insert(name),
            StubSymbolKind::Constant => names.constants.insert(name),
        };
    });
    (!names.is_empty()).then_some(names)
}

/// Short names of the declarations in one stub file whose docblock says
/// they were added `@since` a PHP version newer than a given one, each
/// with that version.
#[derive(Debug, Default)]
pub struct NewerStubNames<'a> {
    pub functions: HashMap<&'a str, crate::types::PhpVersion>,
    pub classes: HashMap<&'a str, crate::types::PhpVersion>,
    pub constants: HashMap<&'a str, crate::types::PhpVersion>,
    /// The names of the newer methods and class constants, whatever class
    /// declares them.
    pub members: HashSet<&'a str>,
}

impl NewerStubNames<'_> {
    /// The version the symbol `name` (qualified or not) of the given kind
    /// was added in, when it is among the newer declarations.
    pub fn get(&self, kind: StubSymbolKind, name: &str) -> Option<crate::types::PhpVersion> {
        let short = name.rsplit('\\').next().unwrap_or(name);
        match kind {
            StubSymbolKind::Function => self.functions.get(short),
            StubSymbolKind::Class => self.classes.get(short),
            StubSymbolKind::Constant => self.constants.get(short),
        }
        .copied()
    }
}

/// The built-ins of the stub indices that a project's PHP version does not
/// have yet, keyed the way each index is, with the version that added
/// each.  Built from [`newer_stub_names`] by `Backend::set_php_version`.
#[derive(Debug, Default)]
pub struct NewerStubSymbols {
    pub functions: CiMap<crate::types::PhpVersion>,
    pub classes: CiMap<crate::types::PhpVersion>,
    pub constants: HashMap<&'static str, crate::types::PhpVersion>,
    /// The names of the newer methods and class constants, lowercased: a
    /// member access that names none of them cannot use one.
    pub members: HashSet<String>,
}

impl NewerStubSymbols {
    /// The version that added the built-in `name` of the given kind, when
    /// the target does not have it yet.
    pub fn get(&self, kind: StubSymbolKind, name: &str) -> Option<crate::types::PhpVersion> {
        let name = name.strip_prefix('\\').unwrap_or(name);
        match kind {
            StubSymbolKind::Function => self.functions.get(name),
            StubSymbolKind::Class => self.classes.get(name),
            StubSymbolKind::Constant => self.constants.get(name),
        }
        .copied()
    }

    /// Whether the target PHP version has every built-in.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
            && self.classes.is_empty()
            && self.constants.is_empty()
            && self.members.is_empty()
    }

    /// Whether some built-in class has a method or constant named `name`
    /// that the target PHP version does not have yet.
    pub fn has_member_named(&self, name: &str) -> bool {
        !self.members.is_empty() && self.members.contains(&name.to_ascii_lowercase())
    }
}

/// Collect every declaration in a stub file whose `@since` tag names a
/// PHP version newer than `php_version`, in one pass over the file.
///
/// Attributed like [`removed_stub_names`].  Only meaningful for the stub
/// of a bundled extension (see [`is_bundled_extension_stub_file`]).
/// Returns `None` when nothing in the file is newer.
pub fn newer_stub_names(
    source: &str,
    php_version: crate::types::PhpVersion,
) -> Option<NewerStubNames<'_>> {
    let mut names = NewerStubNames::default();
    for_each_tagged_declaration(source, "@since", |docblock, declared| {
        let Some(since) = docblock_since(docblock).filter(|&since| since > php_version) else {
            return;
        };
        let (kind, name) = match declared {
            Declared::Symbol(kind, name) => (kind, name),
            Declared::Member(name) => {
                names.members.insert(name);
                return;
            }
        };
        let map = match kind {
            StubSymbolKind::Function => &mut names.functions,
            StubSymbolKind::Class => &mut names.classes,
            StubSymbolKind::Constant => &mut names.constants,
        };
        map.entry(name).or_insert(since);
    });
    let empty = names.functions.is_empty()
        && names.classes.is_empty()
        && names.constants.is_empty()
        && names.members.is_empty();
    (!empty).then_some(names)
}

/// Call `f` with the docblock and the declaration of every docblock in
/// `source` that holds `tag`.
///
/// The docblock is attributed to the first declaration that follows it
/// before the next docblock opens.
fn for_each_tagged_declaration<'a>(
    source: &'a str,
    tag: &str,
    mut f: impl FnMut(&'a str, Declared<'a>),
) {
    let mut search_from = 0;
    while let Some(rel) = source[search_from..].find(tag) {
        let tag_pos = search_from + rel;
        let Some(doc_end) = source[tag_pos..].find("*/").map(|p| tag_pos + p + 2) else {
            break;
        };
        search_from = doc_end;
        let Some(doc_start) = source[..tag_pos].rfind("/**") else {
            continue;
        };
        let after = &source[doc_end..];
        let after = &after[..after.find("/**").unwrap_or(after.len())];
        if let Some(declared) = first_declaration(after) {
            f(&source[doc_start..doc_end], declared);
        }
    }
}

/// The PHP version a docblock's `@since` tag names, if it names one.
fn docblock_since(docblock: &str) -> Option<crate::types::PhpVersion> {
    docblock.lines().find_map(|line| {
        let trimmed = line
            .trim()
            .trim_start_matches("/**")
            .trim_start_matches('*');
        crate::docblock::parse_php_since_version(trimmed.trim().strip_prefix("@since")?)
    })
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

/// What a stub docblock documents.
enum Declared<'a> {
    /// A function, class-like, or global constant, by its short name.
    Symbol(StubSymbolKind, &'a str),
    /// A method, class constant, or enum case, by its name.  A property is
    /// left out.
    Member(&'a str),
}

/// Find the earliest declaration keyword in `text` and return what it
/// declares.
///
/// A declaration with a visibility modifier, which every member in
/// phpstorm-stubs carries, and an enum `case` are class members.
fn first_declaration(text: &str) -> Option<Declared<'_>> {
    const KEYWORDS: [(&str, Option<StubSymbolKind>); 14] = [
        ("function ", Some(StubSymbolKind::Function)),
        ("function&", Some(StubSymbolKind::Function)),
        ("class ", Some(StubSymbolKind::Class)),
        ("interface ", Some(StubSymbolKind::Class)),
        ("trait ", Some(StubSymbolKind::Class)),
        ("enum ", Some(StubSymbolKind::Class)),
        ("const ", Some(StubSymbolKind::Constant)),
        ("define('", Some(StubSymbolKind::Constant)),
        ("define(\"", Some(StubSymbolKind::Constant)),
        ("public ", None),
        ("protected ", None),
        ("private ", None),
        ("var ", None),
        ("case ", None),
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
    let Some(kind) = kind else {
        // The name is the last word before the parameter list, the
        // initializer, or the end of the declaration.
        let head = &text[pos..];
        let head = &head[..head.find(['(', '=', ';', '{']).unwrap_or(head.len())];
        let name = head.split_whitespace().next_back()?.trim_start_matches('&');
        let is_name = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80);
        return is_name.then_some(Declared::Member(name));
    };
    let rest = text[pos + keyword.len()..].trim_start_matches([' ', '\t', '&']);
    let len = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b >= 0x80)
        .count();
    (len > 0).then(|| Declared::Symbol(kind, &rest[..len]))
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
    fn newer_function_class_and_constant() {
        let source = "<?php\n/**\n * @since 8.0\n */\n#[Pure]\nfunction str_contains() {}\n\
                      /**\n * @since 8.4\n */\nenum RoundingMode {}\n\
                      /**\n * @since 8.1\n */\ndefine('MYSQLI_REFRESH_REPLICA', 64);\n\
                      /**\n * @since 5.3\n */\nfunction old() {}\n";
        let names = newer_stub_names(source, PHP_7_4).unwrap();
        assert_eq!(
            names.get(StubSymbolKind::Function, "str_contains"),
            Some(PHP_8_0)
        );
        assert_eq!(
            names.get(StubSymbolKind::Class, "RoundingMode"),
            Some(PhpVersion::new(8, 4))
        );
        assert_eq!(
            names.get(StubSymbolKind::Constant, "MYSQLI_REFRESH_REPLICA"),
            Some(PhpVersion::new(8, 1))
        );
        assert_eq!(names.get(StubSymbolKind::Function, "old"), None);
        assert!(newer_stub_names(source, PhpVersion::new(8, 4)).is_none());
    }

    #[test]
    fn newer_member_is_not_a_function() {
        let source = "<?php\nclass DateTime {\n    /**\n     * @since 8.0\n     */\n    \
                      public static function createFromInterface($object) {}\n\
                      /**\n * @since 8.3\n */\n    final public const int ATOM = 1;\n\
                      /** @since 8.4 */\n    case HalfEven;\n}\n\
                      function createFromInterface() {}\n";
        let names = newer_stub_names(source, PHP_7_4).unwrap();
        assert!(names.functions.is_empty());
        assert!(names.constants.is_empty());
        let mut members: Vec<_> = names.members.into_iter().collect();
        members.sort_unstable();
        assert_eq!(members, ["ATOM", "HalfEven", "createFromInterface"]);
    }

    #[test]
    fn extension_version_is_not_newer() {
        let source = "<?php\n/**\n * @since PECL rrd >= 0.9.0\n */\nfunction rrd_create() {}\n\
                      /**\n * @since 9.8\n */\nfunction newrelic_x() {}\n";
        let names = newer_stub_names(source, PHP_7_4).unwrap();
        assert_eq!(names.get(StubSymbolKind::Function, "rrd_create"), None);
        // A version above any PHP release still reads as one; only the
        // stub of a bundled extension is scanned for that reason.
        assert!(names.get(StubSymbolKind::Function, "newrelic_x").is_some());
    }

    #[test]
    fn docblock_without_declaration_is_ignored() {
        let source = "<?php\n/**\n * @removed 7.0\n */\n\n/**\n * Kept.\n */\nfunction kept() {}\n";
        assert!(removed_stub_names(source, PHP_8_0).is_none());
    }
}
