//! Looking up a global constant, the constant counterpart of
//! [`Backend::find_or_load_class`] and [`Backend::find_or_load_function`].
//!
//! Resolves a constant through the parsed defines, the autoload constant
//! index, known autoload files, and finally the embedded PHP stubs,
//! lazily parsing files as needed.

use crate::Backend;
use crate::php_type::PhpType;
use crate::types::DefineInfo;

/// How far [`Backend::find_or_load_define`] looks for a declaration.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefineSearch {
    /// The parsed defines, and the one file the autoload constant index
    /// names for each candidate.
    Indexed,
    /// Those, then every known autoload file not parsed yet, for a
    /// `define()` the byte-level scan skipped inside an
    /// `if (!defined(…))` guard.
    Exhaustive,
}

impl Backend {
    /// Find the `define()` or top-level `const` declaring one of
    /// `candidates` in project or vendor code, and return what `read`
    /// takes from it.
    ///
    /// Searches in order, lazily parsing files as needed:
    /// 1. `global_defines` — constants already parsed.
    /// 2. `autoload_constant_index` — the file the byte-level scan found
    ///    the constant in.
    /// 3. With [`DefineSearch::Exhaustive`], `autoload_file_paths` — every
    ///    known autoload file not parsed yet.
    ///
    /// The candidates are names in the order PHP would try them (see
    /// [`Self::lookup_global_constant_candidates`]), and each phase is
    /// tried for every candidate before the next, more expensive one runs.
    /// The embedded stubs are not searched: what they declare has no file
    /// a declaration can be read back out of.
    pub(crate) fn find_or_load_define<T>(
        &self,
        candidates: &[&str],
        search: DefineSearch,
        read: impl Fn(&DefineInfo) -> T,
    ) -> Option<T> {
        let find_parsed = || {
            let dmap = self.symbols.global_defines.read();
            candidates
                .iter()
                .find_map(|name| dmap.get(*name).map(&read))
        };

        // Phase 1: already-parsed constants.
        if let Some(found) = find_parsed() {
            return Some(found);
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
                && let Some(content) = crate::util::read_php_source(&path)
            {
                self.update_ast(&crate::util::path_to_uri(&path), &content);
                if let Some(found) = self.symbols.global_defines.read().get(*name).map(&read) {
                    return Some(found);
                }
            }
        }

        if search == DefineSearch::Indexed {
            return None;
        }

        // Phase 3: lazily parse known autoload files for constants the
        // byte-level scanner missed (e.g. inside `if (!defined(...))`
        // guards).  Each file is parsed at most once: later lookups are
        // answered by Phase 1.
        let paths = self.symbols.autoload_file_paths.read().clone();
        for path in &paths {
            let uri = crate::util::path_to_uri(path);
            if self.parsed_uris.read().contains(&uri) {
                continue;
            }
            if let Some(content) = crate::util::read_php_source(path) {
                self.update_ast(&uri, &content);
                if let Some(found) = find_parsed() {
                    return Some(found);
                }
            }
        }
        None
    }

    /// Look up a global constant by name, returning its value if found.
    ///
    /// Searches project and vendor code the way
    /// [`Self::find_or_load_define`] does, exhaustively, then
    /// `stub_constant_index` (built-in PHP constants from stubs), lazily
    /// parsing the stub file via `update_ast` (which populates
    /// `global_defines`) and re-checking.
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

        if let Some(value) =
            self.find_or_load_define(candidates, DefineSearch::Exhaustive, |info| {
                info.value.clone()
            })
        {
            return Some(value);
        }

        // Built-in PHP constants from embedded stubs.  Parse the stub via
        // update_ast (which populates global_defines), then re-check.  This
        // is the same lazy-parse pattern as the project files above — no
        // special raw-source scanning needed.
        for name in candidates {
            let stub_source = self.stub_constant_index.read().get(*name).copied();
            if let Some(stub_source) = stub_source {
                let stub_uri = format!("{}const/{}", crate::stubs::STUB_URI_PREFIX, name);
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
        self.find_or_load_define(&[name], DefineSearch::Indexed, |info| info.value.clone())?
    }
}

/// The type of a PHP version constant whose value depends on the patch
/// release (see `Backend::php_version_constant`), or `None` for any other
/// name.
pub(crate) fn unversioned_php_version_constant_type(name: &str) -> Option<PhpType> {
    match name {
        "PHP_VERSION_ID" | "PHP_RELEASE_VERSION" => Some(PhpType::int()),
        "PHP_VERSION" | "PHP_EXTRA_VERSION" => Some(PhpType::string()),
        _ => None,
    }
}
