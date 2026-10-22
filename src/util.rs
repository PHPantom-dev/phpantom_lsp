//! Core cross-cutting utility functions used throughout the PHPantom
//! server: panic containment for LSP request handlers, path/URI
//! conversion, and a few other helpers with no single natural module
//! home.
//!
//! Related but more cohesive clusters live in dedicated modules:
//! reading a class name against a file's imports and namespace in
//! [`crate::names`], byte-offset/LSP-position conversion in
//! [`crate::text_position`], PHP source text scanning in
//! [`crate::text_scan`], class lookup and subtype checks in
//! [`crate::class_lookup`], external process spawning in
//! [`crate::process`], and `Backend` file/context accessors in
//! [`crate::backend::file_access`].
//!
//! Cross-file class/function resolution lives in the dedicated
//! [`crate::resolution`] module.
//!
//! Subject-extraction helpers (walking backwards through characters to
//! find variables, call expressions, balanced parentheses, `new`
//! expressions, etc.) live in [`crate::type_engine::subject_extraction`].
use std::panic::{self, AssertUnwindSafe, UnwindSafe};
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::*;

/// Run `f` inside [`panic::catch_unwind`], logging and swallowing any
/// panic.
///
/// Returns `Some(value)` on success and `None` on panic.  The error
/// message includes `label` (the operation name, e.g. `"hover"` or
/// `"goto_definition"`), `uri`, and the optional cursor `position`.
///
/// This centralises the boilerplate that every LSP handler uses to
/// guard against stack overflows and unexpected panics in the
/// resolution pipeline.
///
/// # Examples
///
/// ```ignore
/// let result = catch_panic("hover", uri, Some(position), || {
///     self.handle_hover(uri, content, position)
/// });
/// ```
pub(crate) fn catch_panic<T>(
    label: &str,
    uri: &str,
    position: Option<Position>,
    f: impl FnOnce() -> T + UnwindSafe,
) -> Option<T> {
    match panic::catch_unwind(f) {
        Ok(value) => Some(value),
        Err(_) => {
            if let Some(pos) = position {
                tracing::error!(
                    "PHPantom: panic during {} at {}:{}:{}",
                    label,
                    uri,
                    pos.line,
                    pos.character
                );
            } else {
                tracing::error!("PHPantom: panic during {} at {}", label, uri);
            }
            None
        }
    }
}

/// Convenience wrapper around [`catch_panic`] for closures that
/// capture `&self` or other non-[`UnwindSafe`] references.
///
/// Wraps `f` in [`AssertUnwindSafe`] before forwarding to
/// [`catch_panic`].  This is safe in our context because a panic
/// during LSP handling never leaves shared state in an inconsistent
/// state (the worst case is a stale cache entry).
pub(crate) fn catch_panic_unwind_safe<T>(
    label: &str,
    uri: &str,
    position: Option<Position>,
    f: impl FnOnce() -> T,
) -> Option<T> {
    catch_panic(label, uri, position, AssertUnwindSafe(f))
}

/// Convert a filesystem path to a properly percent-encoded `file://` URI string.
///
/// This **must** be used instead of `format!("file://{}", path.display())`
/// everywhere in the codebase.  The `format!` approach produces raw,
/// un-encoded paths (e.g. `file:///home/user/My Project/Foo.php`) while
/// LSP clients send URIs through the `Url` type which percent-encodes
/// special characters (e.g. `file:///home/user/My%20Project/Foo.php`).
/// When both forms end up as keys in `Backend::files`, the same file is
/// indexed twice and every Find References result is duplicated.
///
/// Falls back to the raw `format!` form only when `Url::from_file_path`
/// fails (non-absolute paths on some platforms), which should never
/// happen in practice.
pub(crate) fn path_to_uri(path: &Path) -> String {
    // Canonicalize relative paths to absolute so that
    // `Url::from_file_path` never fails due to a missing leading `/`.
    let abs_path;
    let effective = if path.is_relative() {
        abs_path = std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf());
        abs_path.as_path()
    } else {
        path
    };
    path_to_url(effective)
        .map(|u| u.to_string())
        .unwrap_or_else(|| format!("file://{}", effective.display()))
}

/// Read a PHP source file from disk, decoding it the way the workspace
/// scan does.
///
/// Invalid UTF-8 (a Latin-1 or Windows-1252 file) is replaced rather than
/// rejected, so a class the scan indexed from such a file can also be
/// loaded.  A read failure is logged the first time it happens for a
/// path and returns `None`.
pub(crate) fn read_php_source(path: &Path) -> Option<String> {
    static REPORTED: std::sync::LazyLock<parking_lot::Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::LazyLock::new(Default::default);

    match std::fs::read(path) {
        Ok(bytes) => Some(match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
        }),
        Err(err) => {
            if REPORTED.lock().insert(path.to_path_buf()) {
                tracing::warn!("could not read {}: {err}", path.display());
            }
            None
        }
    }
}

/// The `file://` URL of `path`, or `None` when the path is not absolute.
///
/// This, [`dir_path_to_url`], [`url_to_path`], [`uri_to_path`] and
/// [`parse_url`] are the only places that convert between file paths, URI
/// strings and URLs, so a change of URL type touches these functions
/// rather than every caller.
pub(crate) fn path_to_url(path: &Path) -> Option<Url> {
    Url::from_file_path(path).ok()
}

/// The `file://` URL of the directory `path`, with the trailing `/` that
/// makes relative URLs resolve inside it, or `None` when the path is not
/// absolute.
pub(crate) fn dir_path_to_url(path: &Path) -> Option<Url> {
    Url::from_directory_path(path).ok()
}

/// Parse a URI string into a URL, or `None` when it is malformed.
pub(crate) fn parse_url(uri: &str) -> Option<Url> {
    Url::parse(uri).ok()
}

/// The placeholder URL for a symbol whose own URI does not parse.
pub(crate) fn unknown_url() -> Url {
    Url::parse("file:///unknown").expect("constant URL parses")
}

/// The file a URL names, or `None` when it is not a `file://` URL.
pub(crate) fn url_to_path(url: &Url) -> Option<PathBuf> {
    url.to_file_path().ok()
}

/// The file a URI string names, or `None` when it does not parse or is
/// not a `file://` URI.
pub(crate) fn uri_to_path(uri: &str) -> Option<PathBuf> {
    url_to_path(&parse_url(uri)?)
}

/// The URI prefix every file under the directory `dir` starts with.
pub(crate) fn dir_uri_prefix(dir: &Path) -> String {
    format!("{}/", path_to_uri(dir))
}

/// The canonical spelling of `path`, for comparing against paths recorded
/// canonically.
///
/// A file that has just been deleted has no canonical form of its own, so
/// its directory is canonicalized instead: the file still has to match the
/// path it was recorded under.
pub(crate) fn canonical_path_for_comparison(path: &Path) -> std::path::PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    match (
        path.parent().and_then(|parent| parent.canonicalize().ok()),
        path.file_name(),
    ) {
        (Some(parent), Some(name)) => parent.join(name),
        _ => path.to_path_buf(),
    }
}

/// The URI of the file at `internal_path` inside the phar at `phar_path`,
/// e.g. `phar:///path/to/phpstan.phar/src/Type/Type.php`.
pub(crate) fn phar_entry_uri(phar_path: &Path, internal_path: &Path) -> String {
    format!("phar://{}/{}", phar_path.display(), internal_path.display())
}

/// Drop the entries `keep` marks `false`, in place.
///
/// `keep` is indexed the same as `items`, which is what a pairwise pass
/// over the whole list produces. `Vec::retain` cannot see the index it is
/// at, so a counter walks alongside it.
pub(crate) fn retain_by_mask<T>(items: &mut Vec<T>, keep: &[bool]) {
    debug_assert_eq!(items.len(), keep.len());
    let mut index = 0;
    items.retain(|_| {
        let retain = keep.get(index).copied().unwrap_or(true);
        index += 1;
        retain
    });
}

/// Extract the short (unqualified) class name from a potentially
/// fully-qualified name.
///
/// For example, `"Illuminate\\Support\\Collection"` → `"Collection"`,
/// and `"Collection"` → `"Collection"`.
pub(crate) fn short_name(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

/// Strip the leading fully-qualified-name backslash from a PHP name.
///
/// `"\\Foo\\Bar"` -> `"Foo\\Bar"`, `"Foo"` -> `"Foo"`.
pub(crate) fn strip_fqn_prefix(name: &str) -> &str {
    name.strip_prefix('\\').unwrap_or(name)
}

/// Strip the surrounding quotes from a PHP string literal and unescape its
/// body, returning the runtime string value.
///
/// This matters when the literal names a class: `'Foo\\Bar'` is the class
/// `Foo\Bar` at runtime, so the doubled backslash in the source text must be
/// collapsed before the value is used as a type/class name. Backslash escapes
/// are resolved in a single left-to-right pass (sequential `replace` calls are
/// order-dependent and mis-handle adjacent escapes). In a single-quoted string
/// only `\\` and `\'` are special; in a double-quoted string only `\\` and
/// `\"` are recognised here (other backslash sequences are kept literal, which
/// is sufficient for class names). Returns `None` when `raw` is not a quoted
/// string literal.
pub(crate) fn unescape_php_string_literal(raw: &str) -> Option<String> {
    let (body, double_quoted) =
        if let Some(b) = raw.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
            (b, false)
        } else {
            let b = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"'))?;
            (b, true)
        };

    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('\'') if !double_quoted => out.push('\''),
                Some('"') if double_quoted => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Build a fully-qualified name from a short name and an optional namespace.
///
/// `("Foo", Some("App\\Models"))` → `"App\\Models\\Foo"`,
/// `("Foo", None)` → `"Foo"`.
pub(crate) fn build_fqn(short_name: &str, namespace: Option<&str>) -> String {
    match namespace {
        Some(ns) if !ns.is_empty() => format!("{}\\{}", ns, short_name),
        _ => short_name.to_string(),
    }
}

/// Find the concrete method body block that contains `offset` within
/// the given class-like members.  Returns `None` if no method body
/// spans the offset.
///
/// This is the shared kernel behind "find enclosing body" operations
/// used by extract-function, property-assignment narrowing, and
/// similar features that need to locate the method body surrounding
/// the cursor.
pub(crate) fn find_enclosing_method_block_in_members<'a>(
    members: impl Iterator<Item = &'a mago_syntax::cst::class_like::member::ClassLikeMember<'a>>,
    offset: u32,
) -> Option<&'a mago_syntax::cst::block::Block<'a>> {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

    for member in members {
        if let ClassLikeMember::Method(method) = member
            && let MethodBody::Concrete(block) = &method.body
        {
            let body_start = block.left_brace.start.offset;
            let body_end = block.right_brace.end.offset;
            if offset >= body_start && offset <= body_end {
                return Some(block);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_string_literal_collapses_namespace_separators() {
        // Single-quoted `'Foo\\Bar'` names the class `Foo\Bar` at runtime.
        assert_eq!(
            unescape_php_string_literal(r"'Foo\\Bar'").as_deref(),
            Some(r"Foo\Bar")
        );
        // Double-quoted spelling collapses the same way.
        assert_eq!(
            unescape_php_string_literal(r#""App\\Models\\User""#).as_deref(),
            Some(r"App\Models\User")
        );
    }

    #[test]
    fn unescape_string_literal_handles_quote_and_lone_backslash() {
        // `\'` produces a literal quote inside a single-quoted string.
        assert_eq!(
            unescape_php_string_literal(r"'a\'b'").as_deref(),
            Some("a'b")
        );
        // A lone backslash before a non-special char stays literal.
        assert_eq!(
            unescape_php_string_literal(r"'a\nb'").as_deref(),
            Some(r"a\nb")
        );
        // An empty string literal unescapes to an empty string.
        assert_eq!(unescape_php_string_literal("''").as_deref(), Some(""));
    }

    #[test]
    fn unescape_string_literal_rejects_unquoted_input() {
        assert_eq!(unescape_php_string_literal("bare"), None);
        assert_eq!(unescape_php_string_literal("'unterminated"), None);
    }
}
