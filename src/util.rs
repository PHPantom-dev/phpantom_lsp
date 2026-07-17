/// Utility functions for the PHPantom server.
///
/// This module contains helper methods for position/offset conversion,
/// class lookup by offset, logging, panic catching, and shared
/// text-processing helpers used by multiple modules.
///
/// Cross-file class/function resolution and name-resolution logic live
/// in the dedicated [`crate::resolution`] module.
///
/// Subject-extraction helpers (walking backwards through characters to
/// find variables, call expressions, balanced parentheses, `new`
/// expressions, etc.) live in [`crate::subject_extraction`].
use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe, UnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::php_type::PhpType;

/// Resolve an unqualified or partially-qualified PHP class/function name
/// to a fully-qualified name using the file's `use` map and namespace.
///
/// Rules:
///   - Leading `\` — strip it and return (already fully-qualified).
///   - Unqualified (no `\`):
///     1. Check the `use_map` for a direct mapping.
///     2. Prefix with the current namespace.
///     3. Fall back to the bare name (global namespace).
///   - Qualified (contains `\`, no leading `\`):
///     1. Check if the first segment is in the `use_map`; if so, expand it.
///     2. Prefix with the current namespace.
///     3. Fall back to the bare name.
pub(crate) fn resolve_to_fqn(
    name: &str,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
) -> String {
    // Already fully-qualified with leading `\` — strip and return.
    if let Some(stripped) = name.strip_prefix('\\') {
        return stripped.to_string();
    }

    // Unqualified name (no backslash) — try use_map, then namespace, then bare.
    if !name.contains('\\') {
        if let Some(fqn) = use_map.get(name) {
            return fqn.clone();
        }
        if let Some(ns) = namespace {
            return format!("{}\\{}", ns, name);
        }
        return name.to_string();
    }

    // Qualified name (contains `\` but no leading `\`).
    let first_segment = name.split('\\').next().unwrap_or(name);
    if let Some(fqn_prefix) = use_map.get(first_segment) {
        let rest = &name[first_segment.len()..];
        return format!("{}{}", fqn_prefix, rest);
    }
    if let Some(ns) = namespace {
        return format!("{}\\{}", ns, name);
    }
    name.to_string()
}

/// Resolve a class name to its FQN via the class loader.
///
/// Returns the FQN from the loaded `ClassInfo` when the loader can find
/// the class, or falls back to the original `name` unchanged.
///
/// **Caveat:** when the loader cannot resolve `name`, the original string
/// is returned as-is.  If `name` is a short (unqualified) class name,
/// the returned value is *not* a FQN — it is the same short name.
/// Callers that need a guaranteed FQN should use [`resolve_to_fqn`]
/// with the file's use-map and namespace instead, falling back to this
/// function only for names that are already expected to be resolvable
/// by the class loader (e.g. names extracted from `::class` expressions
/// or already-resolved type hints).
pub(crate) fn resolve_name_via_loader(
    name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> String {
    class_loader(name)
        .map(|cls| cls.fqn().to_string())
        .unwrap_or_else(|| name.to_string())
}

/// Resolve a source-level (user-typed) class reference to its FQN,
/// preferring a class in the current namespace over a global class of the
/// same short name.
///
/// PHP resolves an unqualified class reference (as written in `new Foo()`
/// or a type hint) against the current namespace before falling back to
/// the global scope, so `new Iterator()` inside `namespace App` means
/// `App\Iterator` when that class exists, not the global SPL `\Iterator`.
///
/// The plain [`resolve_name_via_loader`] delegates to the class loader,
/// which is deliberately global-first for already-canonicalised names
/// (parent/interface FQNs, where a backslash-free name signals an
/// intentional global reference).  This helper corrects that for raw
/// source references: when the loader lands on a global class, it re-checks
/// the namespace-qualified form and prefers a genuine same-namespace class.
///
/// An explicit `use` import still wins: it resolves to a namespaced or
/// aliased class whose FQN differs from the bare name, which this helper
/// leaves untouched.
pub(crate) fn resolve_source_class_name(
    name: &str,
    namespace: Option<&str>,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> String {
    let resolved = class_loader(name);
    // Only an unqualified name inside a namespace can be shadowed by a
    // global stub of the same short name.
    if !name.contains('\\')
        && let Some(ns) = namespace
    {
        // The loader landed on a global class (or found nothing); an
        // explicit import would have resolved to a namespaced/aliased
        // class whose FQN carries a namespace.
        let landed_on_global = resolved.as_ref().is_none_or(|c| c.file_namespace.is_none());
        if landed_on_global {
            let ns_qualified = format!("{ns}\\{name}");
            if let Some(local) = class_loader(&ns_qualified)
                && local.fqn().eq_ignore_ascii_case(&ns_qualified)
            {
                return local.fqn().to_string();
            }
        }
    }
    resolved
        .map(|cls| cls.fqn().to_string())
        .unwrap_or_else(|| name.to_string())
}

/// Resolve all class names inside a [`PhpType`] to their fully-qualified
/// forms using the class loader.  Scalar/keyword types are left untouched.
///
/// This should be called on any `PhpType` that originates from raw source
/// text (docblock annotations, AST identifiers) before it is stored in a
/// [`ResolvedType`](crate::types::ResolvedType).
pub(crate) fn resolve_php_type_names(
    ty: &crate::php_type::PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> crate::php_type::PhpType {
    ty.resolve_names(&|name| resolve_name_via_loader(name, class_loader))
}

/// Check whether two LSP ranges overlap (share at least one character
/// position).
///
/// Two ranges do **not** overlap when one ends exactly where the other
/// starts (i.e. touching ranges are non-overlapping).  This matches
/// the LSP convention where a range's `end` position is exclusive.
pub(crate) fn ranges_overlap(a: &Range, b: &Range) -> bool {
    !(a.end.line < b.start.line
        || (a.end.line == b.start.line && a.end.character <= b.start.character)
        || b.end.line < a.start.line
        || (b.end.line == a.start.line && b.end.character <= a.start.character))
}

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
/// When both forms end up as keys in `symbol_maps`, the same file is
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
    Url::from_file_path(effective)
        .map(|u| u.to_string())
        .unwrap_or_else(|()| format!("file://{}", effective.display()))
}

/// Recursively collect all `.php` files under a directory, respecting
/// `.gitignore` rules and skipping hidden directories (`.git`,
/// `.idea`, etc.).
///
/// Uses the `ignore` crate's `WalkBuilder` for gitignore-aware
/// traversal.  This is consistent with the other workspace walkers
/// (`scan_workspace_fallback_full`, `collect_php_files_gitignore`).
///
/// Used by Go-to-implementation (Phase 5) which walks PSR-4 source
/// directories.
///
/// `vendor_dir_paths` contains absolute paths of all known vendor
/// directories (one per subproject in monorepo mode).  Any directory
/// whose absolute path matches one of these is skipped regardless of
/// `.gitignore` content.
///
/// Silently skips directories and files that cannot be read (e.g.
/// permission errors, broken symlinks).
pub(crate) fn collect_php_files(dir: &Path, vendor_dir_paths: &[PathBuf]) -> Vec<PathBuf> {
    use ignore::WalkBuilder;

    let mut result = Vec::new();
    let vendor_paths: Vec<PathBuf> = vendor_dir_paths.to_vec();

    let walker = WalkBuilder::new(dir)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(true)
        .parents(true)
        .ignore(true)
        .filter_entry(move |entry| {
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                let path = entry.path();
                if vendor_paths.iter().any(|vp| vp == path) {
                    return false;
                }
            }
            true
        })
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "php") {
            result.push(path.to_path_buf());
        }
    }

    result
}

/// Recursively collect all `.php` files under a workspace root,
/// respecting `.gitignore` rules (including nested and global
/// gitignore files).
///
/// Used by Find References which walks the entire workspace root.
/// Unlike [`collect_php_files`], this uses the `ignore` crate's
/// [`WalkBuilder`] so that generated/cached directories listed in
/// `.gitignore` (e.g. `storage/framework/views/`, `var/cache/`,
/// `node_modules/`) are automatically skipped.
///
/// All known vendor directories are always skipped regardless of
/// `.gitignore` content, since some projects commit their vendor
/// directory.  `vendor_dir_paths` contains absolute paths of all
/// known vendor directories (one per subproject in monorepo mode).
///
/// Hidden files and directories are skipped by default (handled by
/// the `ignore` crate).
pub(crate) fn collect_php_files_gitignore(
    root: &Path,
    vendor_dir_paths: &[PathBuf],
) -> Vec<PathBuf> {
    use ignore::WalkBuilder;

    let mut result = Vec::new();
    let vendor_paths_owned: Vec<PathBuf> = vendor_dir_paths.to_vec();

    let walker = WalkBuilder::new(root)
        // Respect .gitignore, .git/info/exclude, global gitignore
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        // Skip hidden files/dirs (.git, .idea, etc.)
        .hidden(true)
        // Read parent .gitignore files
        .parents(true)
        // Also respect .ignore files (ripgrep convention)
        .ignore(true)
        // Always skip vendor directories, even if not gitignored
        .filter_entry(move |entry| {
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                let path = entry.path();
                if vendor_paths_owned.iter().any(|vp| vp == path) {
                    return false;
                }
            }
            true
        })
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "php") {
            result.push(path.to_path_buf());
        }
    }

    result
}

/// Convert a byte offset in `content` to an LSP `Position` (line, character).
///
/// This is the inverse of [`position_to_byte_offset`].  Characters are
/// counted as UTF-16 code units per the LSP specification.
/// If `offset` is past the end of `content`, the position at the end of
/// the file is returned.
pub(crate) fn offset_to_position(content: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in content.char_indices() {
        if i == offset {
            return Position {
                line,
                character: col,
            };
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    // offset == content.len() (end of file)
    Position {
        line,
        character: col,
    }
}

/// Precomputed line-start byte offsets for fast repeated offset→[`Position`]
/// lookups within a single piece of content.
///
/// [`offset_to_position`] rescans `content` from the start on every call, so
/// it is O(offset). Converting many offsets in a loop (e.g. one per semantic
/// token, of which a large file has thousands) is therefore O(n²) in the file
/// size. `LineIndex` builds the line table once and answers each query with a
/// binary search for the line plus a short within-line UTF-16 scan, turning the
/// loop into O(n log n).
pub(crate) struct LineIndex<'a> {
    content: &'a str,
    /// Byte offset of the first character of each line. Always starts with `0`.
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    /// Build the line table for `content` in a single pass.
    pub(crate) fn new(content: &'a str) -> Self {
        let mut line_starts = Vec::with_capacity(content.len() / 24 + 1);
        line_starts.push(0usize);
        for (i, b) in content.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            content,
            line_starts,
        }
    }

    /// The content this index was built from, for callers that also need to
    /// scan the raw source (e.g. to locate a declaration's end).
    pub(crate) fn content(&self) -> &'a str {
        self.content
    }

    /// Convert a byte `offset` to an LSP [`Position`] (0-based line, UTF-16
    /// column). Offsets past the end of the content clamp to the content
    /// length, matching [`offset_to_position`].
    pub(crate) fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.content.len());
        // Greatest line start that is <= offset. `line_starts[0] == 0`, so the
        // `Err(0)` case (offset before the first line start) cannot happen.
        let line = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx - 1,
        };
        let line_start = self.line_starts[line];
        let character = self.content[line_start..offset]
            .chars()
            .map(|c| c.len_utf16() as u32)
            .sum();
        Position {
            line: line as u32,
            character,
        }
    }
}

/// Return the byte offset of the start of the line at `line_idx`
/// (0-based) within `content`.
///
/// Unlike the common `content.lines().take(n).map(|l| l.len() + 1).sum()`
/// idiom, this counts the real line terminator. `str::lines()` strips the
/// `\r` of a `\r\n` (CRLF) pair, so the sum-of-`len + 1` approach
/// undercounts every preceding line by one byte on CRLF files and drifts
/// the computed offset. Counting newline bytes directly stays correct for
/// both LF and CRLF (the `\n` is the final byte of a CRLF pair either way).
///
/// If `line_idx` exceeds the number of lines, `content.len()` is returned.
pub(crate) fn line_start_byte_offset(content: &str, line_idx: usize) -> usize {
    if line_idx == 0 {
        return 0;
    }
    let mut seen = 0usize;
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line_idx {
                return i + 1;
            }
        }
    }
    content.len()
}

/// Convert an LSP `Position` (line, character) to a byte offset in
/// `content`.
///
/// Characters are counted as UTF-16 code units per the LSP specification.
/// If the position is past the end of the file, the content length is
/// returned.
pub(crate) fn position_to_byte_offset(content: &str, position: Position) -> usize {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in content.char_indices() {
        if line == position.line && col == position.character {
            return i;
        }
        if ch == '\n' {
            if line == position.line {
                // Position is past the end of this line — clamp to newline.
                return i;
            }
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    // Position at end of content.
    content.len()
}

/// Convert a UTF-16 column offset to a byte offset within a single line.
///
/// LSP positions use UTF-16 code units for the character offset.  When a
/// line contains multi-byte characters (e.g. `ń` is 2 bytes in UTF-8 but
/// 1 UTF-16 code unit), the two offsets diverge.  This helper walks the
/// line counting UTF-16 code units and returns the corresponding byte
/// position.
///
/// Returns `line.len()` if `utf16_col` is past the end of the line.
pub(crate) fn utf16_col_to_byte_offset(line: &str, utf16_col: u32) -> usize {
    let mut col = 0u32;
    for (i, ch) in line.char_indices() {
        if col == utf16_col {
            return i;
        }
        col += ch.len_utf16() as u32;
    }
    line.len()
}

/// Convert a byte offset within a single line to a UTF-16 column offset.
///
/// This is the inverse of [`utf16_col_to_byte_offset`].  It counts
/// UTF-16 code units for all characters before `byte_offset` and returns
/// the result.
///
/// Returns the total UTF-16 length of the line if `byte_offset` is past
/// the end.
pub(crate) fn byte_offset_to_utf16_col(line: &str, byte_offset: usize) -> u32 {
    let mut col = 0u32;
    for (i, ch) in line.char_indices() {
        if i >= byte_offset {
            return col;
        }
        col += ch.len_utf16() as u32;
    }
    col
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

/// Remove surrounding single or double quotes from a PHP string literal.
///
/// `"'hello'"` → `Some("hello")`, `"\"world\""` → `Some("world")`,
/// `"bare"` → `None`.
pub(crate) fn unquote_php_string(raw: &str) -> Option<&str> {
    raw.strip_prefix('\'')
        .and_then(|r| r.strip_suffix('\''))
        .or_else(|| raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')))
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

/// Check whether a type string has unclosed delimiters (`<>`, `()`, `{}`).
///
/// Returns `true` when at least one delimiter pair is left open,
/// indicating that the string is a fragment of a longer type.
pub(crate) fn has_unclosed_delimiters(s: &str) -> bool {
    let mut angle = 0i32;
    let mut paren = 0i32;
    let mut brace = 0i32;
    for b in s.bytes() {
        match b {
            b'<' => angle += 1,
            b'>' => angle -= 1,
            b'(' => paren += 1,
            b')' => paren -= 1,
            b'{' => brace += 1,
            b'}' => brace -= 1,
            _ => {}
        }
    }
    angle > 0 || paren > 0 || brace > 0
}

/// Convert a byte offset range to an LSP `Range`.
///
/// Returns a `Range` with both endpoints converted from byte offsets
/// to `Position` (line/character).
pub(crate) fn byte_range_to_lsp_range(content: &str, start: usize, end: usize) -> Range {
    let start_pos = offset_to_position(content, start);
    let end_pos = offset_to_position(content, end);
    Range {
        start: start_pos,
        end: end_pos,
    }
}

/// Strip trailing PHP visibility/modifier keywords from a string.
///
/// Given a string like `"  /** ... */\n    public static"`, returns
/// `"  /** ... */"` (after stripping `static` and `public`).
///
/// Recognised modifiers: `public`, `protected`, `private`, `static`,
/// `abstract`, `final`, `readonly`.
pub(crate) fn strip_trailing_modifiers(s: &str) -> &str {
    const MODIFIERS: &[&str] = &[
        "public",
        "protected",
        "private",
        "static",
        "abstract",
        "final",
        "readonly",
    ];

    let mut result = s;
    loop {
        let trimmed = result.trim_end();
        let mut found = false;
        for &kw in MODIFIERS {
            if let Some(prefix) = trimmed.strip_suffix(kw) {
                // Make sure the keyword isn't part of a larger identifier.
                if prefix.is_empty()
                    || prefix
                        .as_bytes()
                        .last()
                        .is_some_and(|&b| !b.is_ascii_alphanumeric() && b != b'_')
                {
                    result = prefix;
                    found = true;
                    break;
                }
            }
        }
        if !found {
            break;
        }
    }
    result.trim_end()
}

/// Find the first `;` in `s` that is not nested inside `()`, `[]`,
/// `{}`, or string literals.
///
/// Returns the byte offset of the semicolon, or `None` if no
/// top-level semicolon exists.  Used by multiple completion modules
/// to delimit the right-hand side of assignment statements.
pub(crate) fn find_semicolon_balanced(s: &str) -> Option<usize> {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut prev_char = '\0';

    for (i, ch) in s.char_indices() {
        if in_single_quote {
            if ch == '\'' && prev_char != '\\' {
                in_single_quote = false;
            }
            prev_char = ch;
            continue;
        }
        if in_double_quote {
            if ch == '"' && prev_char != '\\' {
                in_double_quote = false;
            }
            prev_char = ch;
            continue;
        }
        match ch {
            '\'' => in_single_quote = true,
            '"' => in_double_quote = true,
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            ';' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                return Some(i);
            }
            _ => {}
        }
        prev_char = ch;
    }
    None
}

/// Find the position of the closing delimiter that matches the opening
/// delimiter at `open_pos`, scanning forward.
///
/// `open` and `close` are the opening and closing byte values (e.g.
/// `b'{'` / `b'}'` or `b'('` / `b')'`).  The scan is aware of string
/// literals (`'…'` and `"…"` with backslash escaping) and both styles
/// of PHP comment (`// …` and `/* … */`), so delimiters inside strings
/// or comments are not counted.
pub(crate) fn find_matching_forward(
    text: &str,
    open_pos: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    if open_pos >= len || bytes[open_pos] != open {
        return None;
    }
    let mut depth = 1u32;
    let mut pos = open_pos + 1;
    let mut in_single = false;
    let mut in_double = false;
    while pos < len && depth > 0 {
        let b = bytes[pos];
        if in_single {
            if b == b'\\' {
                pos += 1;
            } else if b == b'\'' {
                in_single = false;
            }
        } else if in_double {
            if b == b'\\' {
                pos += 1;
            } else if b == b'"' {
                in_double = false;
            }
        } else {
            match b {
                b'\'' => in_single = true,
                b'"' => in_double = true,
                b if b == open => depth += 1,
                b if b == close => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(pos);
                    }
                }
                b'/' if pos + 1 < len => {
                    if bytes[pos + 1] == b'/' {
                        // Line comment — skip to end of line
                        while pos < len && bytes[pos] != b'\n' {
                            pos += 1;
                        }
                        continue;
                    }
                    if bytes[pos + 1] == b'*' {
                        // Block comment — skip to `*/`
                        pos += 2;
                        while pos + 1 < len {
                            if bytes[pos] == b'*' && bytes[pos + 1] == b'/' {
                                pos += 1;
                                break;
                            }
                            pos += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        pos += 1;
    }
    None
}

/// Find the position of the opening delimiter that matches the closing
/// delimiter at `close_pos`, scanning backward.
///
/// `open` and `close` are the opening and closing byte values (e.g.
/// `b'{'` / `b'}'` or `b'('` / `b')'`).  The scan skips over string
/// literals (`'…'` and `"…"`) by counting preceding backslashes to
/// distinguish escaped from unescaped quotes.
pub(crate) fn find_matching_backward(
    text: &str,
    close_pos: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let bytes = text.as_bytes();
    if close_pos >= bytes.len() || bytes[close_pos] != close {
        return None;
    }

    let mut depth = 1i32;
    let mut pos = close_pos;

    while pos > 0 {
        pos -= 1;
        match bytes[pos] {
            b if b == close => depth += 1,
            b if b == open => {
                depth -= 1;
                if depth == 0 {
                    return Some(pos);
                }
            }
            // Skip string literals by walking backward to the opening quote.
            b'\'' | b'"' => {
                let quote = bytes[pos];
                if pos > 0 {
                    pos -= 1;
                    while pos > 0 {
                        if bytes[pos] == quote {
                            // Check for escape — count preceding backslashes
                            let mut bs = 0;
                            let mut check = pos;
                            while check > 0 && bytes[check - 1] == b'\\' {
                                bs += 1;
                                check -= 1;
                            }
                            if bs % 2 == 0 {
                                break; // unescaped quote — string start
                            }
                        }
                        pos -= 1;
                    }
                }
            }
            _ => {}
        }
    }

    None
}

use crate::Backend;
use crate::types::{ClassInfo, FileContext};

/// Convert an LSP Position (line, character) to a byte offset in content.
///
/// Thin wrapper around [`position_to_byte_offset`] that returns `u32`
/// (matching the offset type used by `ClassInfo::start_offset` /
/// `end_offset` and `ResolutionCtx::cursor_offset`).
pub(crate) fn position_to_offset(content: &str, position: Position) -> u32 {
    position_to_byte_offset(content, position) as u32
}

/// Convert an LSP `Position` (line/character) to a character offset into
/// a pre-built char array.
///
/// Returns `None` when the position is beyond the end of `chars`.
/// Handles UTF-16 column widths, end-of-line clamping, and trailing
/// content without a newline.
pub fn position_to_char_offset(chars: &[char], position: Position) -> Option<usize> {
    let target_line = position.line as usize;
    let target_col = position.character as usize;
    let mut line = 0usize;
    let mut col = 0usize;

    for (i, &ch) in chars.iter().enumerate() {
        if line == target_line && col == target_col {
            return Some(i);
        }
        if ch == '\n' {
            // If we're at the target line and the target column is at or
            // past the end of the line, clamp to end-of-line.
            if line == target_line {
                return Some(i);
            }
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16();
        }
    }

    // Cursor at very end of content
    if line == target_line && col == target_col {
        return Some(chars.len());
    }
    // Target column past end of last line (no trailing newline)
    if line == target_line {
        return Some(chars.len());
    }

    None
}

/// Find which class the cursor (byte offset) is inside.
///
/// When multiple classes contain the offset (e.g. an anonymous class
/// nested inside a named class's method), the smallest (most specific)
/// class is returned.  This ensures that `$this` inside an anonymous
/// class body resolves to the anonymous class, not the outer class.
///
/// The span runs from the declaration start (`decl_start_offset`, which
/// includes any leading attribute lists) to the closing brace, so a
/// `self::` reference inside a class-level attribute — which sits before
/// the body braces — still resolves to the class it decorates.
pub(crate) fn find_class_at_offset(classes: &[Arc<ClassInfo>], offset: u32) -> Option<&ClassInfo> {
    classes
        .iter()
        .map(|c| c.as_ref())
        .map(|c| {
            let start = if c.decl_start_offset != 0 {
                c.decl_start_offset
            } else {
                c.start_offset
            };
            (c, start)
        })
        .filter(|(c, start)| offset >= *start && offset <= c.end_offset)
        .min_by_key(|(c, start)| c.end_offset.saturating_sub(*start))
        .map(|(c, _)| c)
}

/// Find a class in a slice by name, preferring namespace-aware matching
/// when the name is fully qualified.
///
/// When `name` contains backslashes (e.g. `Illuminate\Database\Eloquent\Builder`),
/// the lookup checks each candidate's `file_namespace` field so that the
/// correct class is returned even when multiple classes share the same short
/// name but live in different namespace blocks within the same file (e.g.
/// `Demo\Builder` vs `Illuminate\Database\Eloquent\Builder`).
///
/// When `name` is a bare short name (no backslashes), the first class with
/// a matching `name` field is returned (preserving existing behavior).
pub(crate) fn find_class_by_name<'a>(
    all_classes: &'a [Arc<ClassInfo>],
    name: &str,
) -> Option<&'a Arc<ClassInfo>> {
    let short = short_name(name);

    if name.contains('\\') {
        let expected_ns = name.rsplit_once('\\').map(|(ns, _)| ns);
        all_classes
            .iter()
            .find(|c| c.name == short && c.file_namespace.as_deref() == expected_ns)
    } else {
        all_classes.iter().find(|c| c.name == short)
    }
}

/// Check whether `class` is a subtype of the class identified by
/// `ancestor_name`.  Returns `true` when:
///
/// - `class.name` equals `ancestor_name` (same class), or
/// - walking the `parent_class` chain reaches `ancestor_name`, or
/// - `ancestor_name` appears in the `interfaces` list of `class` or any
///   of its ancestors.
///
/// Both short names and fully-qualified names are compared so that
/// cross-file relationships (where `parent_class` stores FQNs) work.
pub(crate) fn is_subtype_of(
    class: &crate::types::ClassInfo,
    ancestor_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> bool {
    // Every name handled by this function — the ancestor plus each
    // `interfaces`/`parent_class` entry pulled from a loaded class — is a
    // canonical FQN, not a user-typed reference.  A backslash-free name
    // (e.g. `Iterator`, `Traversable`, `Exception`) is therefore a
    // root-namespace class and must resolve in the global namespace.
    //
    // The raw `class_loader` applies the consuming file's use-map to
    // unqualified names.  That is correct for user-typed references but
    // wrong here: when a calling file has `use App\Iterator;`, loading the
    // global `\Iterator` interface node inside an SPL class's hierarchy
    // would return `App\Iterator` and break the walk (so subtype checks
    // against `Iterator`/`Traversable` spuriously fail).  Route
    // backslash-free names through the `__fqn__\` bypass, which skips the
    // use-map and falls back to a global short-name lookup.  The
    // `.or_else` preserves the raw loader for genuine short names that
    // have no global class (they resolve via namespace context as before).
    let load_fqn = |name: &str| -> Option<Arc<crate::types::ClassInfo>> {
        if name.contains('\\') {
            class_loader(name)
        } else {
            class_loader(&format!("__fqn__\\{name}")).or_else(|| class_loader(name))
        }
    };

    // Resolve the ancestor to its FQN so that all comparisons below are
    // FQN-vs-FQN.  When `ancestor_name` is already a FQN (contains `\`)
    // we use it directly.  When it is a short name we try to load it
    // through `load_fqn` and use the loaded class's FQN.  For
    // root-namespace classes (e.g. `RuntimeException`) the FQN equals the
    // short name, so the fallback to `ancestor_name` is correct.
    let ancestor_fqn: String = if ancestor_name.contains('\\') {
        ancestor_name.to_string()
    } else if let Some(loaded) = load_fqn(ancestor_name) {
        loaded.fqn().to_string()
    } else {
        // Cannot resolve — keep the original name.  For root-namespace
        // classes this is already the FQN.
        ancestor_name.to_string()
    };
    let ancestor = ancestor_fqn.as_str();

    // Same class?  Always compare by FQN.
    if class.fqn() == ancestor {
        return true;
    }

    // Check interfaces on the class itself (stored as FQNs after
    // resolve_parent_class_names), walking the full interface
    // inheritance tree so that transitive relationships are found
    // (e.g. Response implements ResponseInterface extends MessageInterface).
    let mut iface_queue: Vec<String> = class.interfaces.iter().map(|a| a.to_string()).collect();
    let mut visited_ifaces: std::collections::HashSet<String> =
        iface_queue.iter().cloned().collect();
    while let Some(iface_name) = iface_queue.pop() {
        if iface_name == ancestor {
            return true;
        }
        // Load the interface and check its parents (interface extends).
        if let Some(iface_info) = load_fqn(&iface_name) {
            // Interface parents are stored in both `parent_class`
            // (first parent for single-extends compat) and
            // `interfaces` (all parents for multi-extends).
            for parent_iface in &iface_info.interfaces {
                if visited_ifaces.insert(parent_iface.to_string()) {
                    iface_queue.push(parent_iface.to_string());
                }
            }
            if let Some(ref pc) = iface_info.parent_class
                && visited_ifaces.insert(pc.to_string())
            {
                iface_queue.push(pc.to_string());
            }
        }
    }

    // Walk the parent class chain (parent_class is also a resolved FQN).
    let mut current_parent = class.parent_class.map(|a| a.to_string());
    let mut visited_parents: std::collections::HashSet<String> = std::collections::HashSet::new();
    visited_parents.insert(class.fqn().to_string());
    let mut depth = 0u32;
    while let Some(ref name) = current_parent {
        depth += 1;
        if depth > 20 {
            break;
        }
        if name == ancestor {
            return true;
        }
        // Load the parent to check its interfaces (transitively)
        // and continue the class chain.  `load_fqn` resolves
        // root-namespace names globally, so the use-map can no longer
        // shadow a global parent (e.g. a same-file `use App\Exception;`
        // does not hijack a stub class's global `\Exception` parent).
        if let Some(parent_info) = load_fqn(name) {
            let mut p_iface_queue: Vec<String> = parent_info
                .interfaces
                .iter()
                .map(|a| a.to_string())
                .collect();
            let mut p_visited: std::collections::HashSet<String> =
                p_iface_queue.iter().cloned().collect();
            while let Some(iface_name) = p_iface_queue.pop() {
                if iface_name == ancestor {
                    return true;
                }
                if let Some(iface_info) = load_fqn(&iface_name) {
                    for pi in &iface_info.interfaces {
                        if p_visited.insert(pi.to_string()) {
                            p_iface_queue.push(pi.to_string());
                        }
                    }
                    if let Some(ref pc) = iface_info.parent_class
                        && p_visited.insert(pc.to_string())
                    {
                        p_iface_queue.push(pc.to_string());
                    }
                }
            }
            // Cycle detection: stop if we have already visited this
            // parent's FQN (a malformed or self-referential hierarchy).
            if !visited_parents.insert(parent_info.fqn().to_string()) {
                break;
            }
            current_parent = parent_info.parent_class.map(|a| a.to_string());
        } else {
            break;
        }
    }

    false
}

/// Check whether `subtype` is a subtype of `supertype`, combining
/// structural subtyping ([`PhpType::is_subtype_of`]) with nominal
/// class-hierarchy walking ([`is_subtype_of`]).
///
/// This is the single entry point for all subtype checks that need
/// both layers:
///
/// - Scalars, unions, intersections, generics, callables, literals,
///   and other structural relationships are handled by
///   `PhpType::is_subtype_of`.
/// - Nominal class relationships (`Cat <: Animal`) are resolved by
///   loading the class via `class_loader` and walking its parent
///   chain and interface list.
///
/// Returns `true` when the structural check succeeds, or when both
/// types are named (class/interface) types and the class hierarchy
/// confirms the relationship.
/// Convenience wrapper around [`is_subtype_of_typed`] that accepts bare
/// class names instead of pre-constructed [`PhpType`] values.
///
/// This avoids the boilerplate of wrapping each name in
/// `PhpType::Named(name.to_string())` at call sites that already have
/// `&str` class names.
pub(crate) fn is_subtype_of_names(
    subtype_name: &str,
    supertype_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> bool {
    use crate::php_type::PhpType;
    is_subtype_of_typed(
        &PhpType::Named(subtype_name.to_string()),
        &PhpType::Named(supertype_name.to_string()),
        class_loader,
    )
}

/// Like [`is_subtype_of_typed`] but accepts a `&str` for the supertype,
/// avoiding `PhpType::Named` wrapping at call sites that already have a
/// `&PhpType` subtype and a bare class name as supertype.
pub(crate) fn is_subtype_of_named(
    subtype: &crate::php_type::PhpType,
    supertype_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> bool {
    use crate::php_type::PhpType;
    is_subtype_of_typed(
        subtype,
        &PhpType::Named(supertype_name.to_string()),
        class_loader,
    )
}

pub(crate) fn is_subtype_of_typed(
    subtype: &crate::php_type::PhpType,
    supertype: &crate::php_type::PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
) -> bool {
    use crate::php_type::PhpType;

    // Fast path: structural subtyping covers scalars, unions,
    // intersections, generics, callables, literals, etc.
    if subtype.is_subtype_of(supertype) {
        return true;
    }

    // ── Union subtype: every member must be a subtype ───────────
    if let PhpType::Union(members) = subtype {
        return members
            .iter()
            .all(|m| is_subtype_of_typed(m, supertype, class_loader));
    }

    // ── Union supertype: at least one member must accept subtype ─
    if let PhpType::Union(members) = supertype {
        return members
            .iter()
            .any(|m| is_subtype_of_typed(subtype, m, class_loader));
    }

    // ── Nullable normalisation ──────────────────────────────────
    if let PhpType::Nullable(inner) = subtype {
        let as_union = PhpType::Union(vec![inner.as_ref().clone(), PhpType::null()]);
        return is_subtype_of_typed(&as_union, supertype, class_loader);
    }
    if let PhpType::Nullable(inner) = supertype {
        let as_union = PhpType::Union(vec![inner.as_ref().clone(), PhpType::null()]);
        return is_subtype_of_typed(subtype, &as_union, class_loader);
    }

    // ── Intersection supertype: all members required ────────────
    // Checked before the subtype-intersection branch so that the
    // both-intersections case (e.g. `A&B&C` <: `A&B`) decomposes the
    // supertype first: every supertype member must be satisfied by
    // *some* subtype member.  Handling the subtype first would instead
    // demand that a single member satisfy the whole supertype, wrongly
    // rejecting a narrower intersection that carries extra constraints.
    if let PhpType::Intersection(members) = supertype {
        return members
            .iter()
            .all(|m| is_subtype_of_typed(subtype, m, class_loader));
    }

    // ── Intersection subtype: at least one member suffices ──────
    if let PhpType::Intersection(members) = subtype {
        return members
            .iter()
            .any(|m| is_subtype_of_typed(m, supertype, class_loader));
    }

    // ── Generic covariance with class-loader awareness ──────────
    // The structural `is_subtype_of` compares generic type params
    // by structural equality, which fails when one side uses a
    // namespace-qualified name and the other uses a short name
    // (e.g. `list<Pen>` vs `list<Demo\Pen>`).  Re-check with the
    // class loader so nominal hierarchy applies to inner params.
    if let (PhpType::Generic(name_sub, args_sub), PhpType::Generic(name_sup, args_sup)) =
        (subtype, supertype)
    {
        let base_sub = name_sub.to_ascii_lowercase();
        let base_sup = name_sup.to_ascii_lowercase();
        let bases_compatible = base_sub == base_sup
            || (crate::php_type::is_array_like_name(name_sub)
                && crate::php_type::is_array_like_name(name_sup));
        if bases_compatible && args_sub.len() == args_sup.len() {
            let is_array_like = crate::php_type::is_array_like_name(name_sub)
                || crate::php_type::is_array_like_name(name_sup);
            let all_params_ok = args_sub.iter().zip(args_sup.iter()).all(|(s, t)| {
                if is_array_like {
                    // Arrays are covariant in PHP (read-only semantics)
                    is_subtype_of_typed(s, t, class_loader)
                } else {
                    // Non-array generics are invariant by default
                    // (both directions must hold, or they must be equal)
                    s == t
                        || (is_subtype_of_typed(s, t, class_loader)
                            && is_subtype_of_typed(t, s, class_loader))
                }
            });
            if all_params_ok {
                return true;
            }
        }
    }

    // ── Array slice covariance ──────────────────────────────────
    // The structural `is_subtype_of` compares `X[]` vs `Y[]` by
    // structural equality on the inner type, which misses nominal
    // subclass relationships (e.g. `Cat[]` <: `Animal[]` where
    // `Cat extends Animal`).  Re-check with the class loader so
    // the hierarchy walk applies to inner types.
    if let (PhpType::Array(inner_sub), PhpType::Array(inner_sup)) = (subtype, supertype)
        && is_subtype_of_typed(inner_sub, inner_sup, class_loader)
    {
        return true;
    }

    // ── Callable specification <: Closure / object ──────────────
    // A `Closure(int): string` is a Closure instance, which is an
    // object.  The structural check only handles `callable` as
    // the named supertype; extend to `Closure` and `object`.
    if matches!(subtype, PhpType::Callable { .. })
        && let Some(sup) = supertype.base_name()
        && (sup.eq_ignore_ascii_case("Closure") || sup.eq_ignore_ascii_case("object"))
    {
        return true;
    }

    // ── class-string covariance through nominal hierarchy ────────
    // The structural `is_subtype_of` handles `class-string<Cat> <:
    // class-string<Animal>` only when `Cat` and `Animal` are
    // structurally equal.  Extend to nominal hierarchy so that
    // `class-string<Cat>` is accepted where `class-string<Animal>`
    // is expected when `Cat extends Animal`.  A bare `class-string`
    // is treated as `class-string<object>`, so it satisfies
    // `class-string<object>` (and its `mixed` equivalent) — any class
    // name is a class-string of some object.
    if let (PhpType::ClassString(sub_inner), PhpType::ClassString(sup_inner)) = (subtype, supertype)
    {
        let object_bound = PhpType::Named("object".to_string());
        let sub = sub_inner.as_deref().unwrap_or(&object_bound);
        let sup = sup_inner.as_deref().unwrap_or(&object_bound);
        return is_subtype_of_typed(sub, sup, class_loader);
    }

    // ── String literal <: class-string<Bound> ────────────────────
    // A string literal that names an existing class satisfying the
    // bound is a valid `class-string<Bound>`, e.g. passing
    // `'RuntimeException'` where `class-string<Throwable>` is
    // expected.  Stay silent (return true) whenever the literal's
    // content cannot be resolved to a class — it may simply live in
    // a file we haven't indexed — and only reject when the resolved
    // class provably fails to satisfy the bound.
    if let PhpType::Literal(crate::php_type::LiteralValue::String(_)) = subtype
        && matches!(
            supertype,
            PhpType::ClassString(_) | PhpType::InterfaceString(_)
        )
    {
        let PhpType::Literal(lit) = subtype else {
            unreachable!()
        };
        let Some(class_name) = lit.string_content() else {
            return true;
        };
        let Some(cls) = class_loader(class_name) else {
            return true;
        };
        return match supertype {
            PhpType::ClassString(Some(bound)) | PhpType::InterfaceString(Some(bound)) => {
                match bound.base_name() {
                    Some(bound_name) => is_subtype_of(&cls, bound_name, class_loader),
                    None => true,
                }
            }
            _ => true,
        };
    }

    // ── Nominal class hierarchy check ───────────────────────────
    // Both sides must resolve to a class name for the hierarchy walk.
    let sub_name = subtype.base_name();
    let sup_name = supertype.base_name();

    if let (Some(sub), Some(sup)) = (sub_name, sup_name) {
        // Try to load the subtype class and walk its hierarchy.
        if let Some(cls) = class_loader(sub) {
            return is_subtype_of(&cls, sup, class_loader);
        }
    }

    false
}

/// Collapse multi-line method chains around the cursor into a single line.
///
/// When the cursor line (after trimming leading whitespace) begins with
/// `->` or `?->`, this function walks backwards through preceding lines
/// that are also continuations, plus the base expression line, and joins
/// them into one flattened string.  The returned column is the cursor's
/// position within that flattened string.
///
/// If the cursor line is not a continuation, the original line and column
/// are returned unchanged.
///
/// # Returns
///
/// `(collapsed_line, adjusted_column)` — the flattened text and the
/// cursor's character offset within it.
pub(crate) fn collapse_continuation_lines(
    lines: &[&str],
    cursor_line: usize,
    cursor_col: usize,
) -> (String, usize) {
    let line = lines[cursor_line];
    let trimmed = line.trim_start();

    // Only collapse when the cursor line is a continuation (starts with
    // `->` or `?->` after optional whitespace).
    if !trimmed.starts_with("->") && !trimmed.starts_with("?->") {
        return (line.to_string(), cursor_col);
    }

    let cursor_leading_ws = line.len() - trimmed.len();

    // Walk backwards to find the first non-continuation line (the base).
    //
    // A continuation line is one that starts with `->` or `?->`.  However,
    // multi-line closure/function arguments can break the chain:
    //
    //   Brand::whereNested(function (Builder $q): void {
    //   })
    //   ->   // ← cursor
    //
    // Here line `})` is NOT a continuation but is part of the call
    // expression on the base line.  We detect this by tracking
    // brace/paren balance: when the accumulated lines (from the current
    // candidate upwards to the cursor) have unmatched closing delimiters,
    // we keep walking backwards until the delimiters balance out.
    let mut start = cursor_line;
    while start > 0 {
        let prev_trimmed = lines[start - 1].trim_start();

        // Skip blank (whitespace-only) lines — they don't terminate a
        // chain.  Without this, a blank line between chain segments
        // causes the backward walk to stop prematurely.
        if prev_trimmed.is_empty() {
            start -= 1;
            continue;
        }

        if prev_trimmed.starts_with("->") || prev_trimmed.starts_with("?->") {
            start -= 1;
        } else {
            // Check whether the accumulated text from this candidate
            // line through the line just before the cursor has
            // unbalanced closing delimiters.  If so, this line is in
            // the middle of a multi-line argument list and we must
            // keep walking backwards.
            start -= 1;

            // Count paren/brace balance from `start` up to (but not
            // including) the cursor line.
            let mut paren_depth: i32 = 0;
            let mut brace_depth: i32 = 0;
            for line in lines.iter().take(cursor_line).skip(start) {
                for ch in line.chars() {
                    match ch {
                        '(' => paren_depth += 1,
                        ')' => paren_depth -= 1,
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }
            }

            // If balanced (or net-open), this is a proper base line.
            if paren_depth >= 0 && brace_depth >= 0 {
                break;
            }

            // Unbalanced — keep walking backwards until we close the
            // gap.  Each step re-checks the running balance.
            while start > 0 && (paren_depth < 0 || brace_depth < 0) {
                start -= 1;
                for ch in lines[start].chars() {
                    match ch {
                        '(' => paren_depth += 1,
                        ')' => paren_depth -= 1,
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }
            }

            // After re-balancing we may have landed on a continuation
            // line (e.g. `->where(...\n...\n)->`) — keep walking if so.
            if start > 0 {
                let landed = lines[start].trim_start();
                if landed.starts_with("->") || landed.starts_with("?->") {
                    continue;
                }
            }
            break;
        }
    }

    // Build the collapsed string from the base line through the cursor line,
    // skipping blank lines so they don't leave gaps in the collapsed result.
    let mut prefix = String::new();
    for (i, line) in lines.iter().enumerate().take(cursor_line).skip(start) {
        let piece = if i == start {
            line.trim_end()
        } else {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            t
        };
        prefix.push_str(piece);
    }

    // The cursor position in the collapsed string is the length of the
    // prefix (everything before the cursor line) plus the cursor's offset
    // within the trimmed cursor line.
    let new_col = prefix.chars().count() + (cursor_col.saturating_sub(cursor_leading_ws));

    prefix.push_str(trimmed);

    (prefix, new_col)
}

/// Scan forward through `lines` starting at `start_line`, tracking brace
/// depth while respecting string literals (`'…'`, `"…"`) and comments
/// (`// …`, `/* … */`).
///
/// Calls `pred(depth)` after every `}` decrement.  Returns the line
/// index of the first `}` where `pred` returns `true`.
///
/// # Examples
///
/// Find the closing `}` that matches the `{` on `brace_line` (depth
/// starts at 0, first `{` pushes to 1, match when depth returns to 0):
///
/// ```ignore
/// find_brace_match_line(&lines, brace_line, |d| d == 0);
/// ```
///
/// Find the enclosing block's `}` from inside a body (depth starts at
/// 0, first unmatched `}` brings depth to −1):
///
/// ```ignore
/// find_brace_match_line(&lines, start_line, |d| d < 0);
/// ```
pub(crate) fn find_brace_match_line(
    lines: &[&str],
    start_line: usize,
    pred: impl Fn(i32) -> bool,
) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_block_comment = false;

    for (line_idx, line) in lines.iter().enumerate().skip(start_line) {
        let bytes = line.as_bytes();
        let len = bytes.len();
        let mut in_line_comment = false;
        let mut i = 0;

        while i < len {
            let b = bytes[i];

            if in_single_quote {
                if b == b'\\' && i + 1 < len {
                    i += 2; // skip escaped character
                    continue;
                }
                if b == b'\'' {
                    in_single_quote = false;
                }
                i += 1;
                continue;
            }

            if in_double_quote {
                if b == b'\\' && i + 1 < len {
                    i += 2; // skip escaped character
                    continue;
                }
                if b == b'"' {
                    in_double_quote = false;
                }
                i += 1;
                continue;
            }

            if in_block_comment {
                if b == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }

            if in_line_comment {
                i += 1;
                continue;
            }

            // Normal code
            if b == b'/' && i + 1 < len {
                if bytes[i + 1] == b'/' {
                    in_line_comment = true;
                    i += 2;
                    continue;
                }
                if bytes[i + 1] == b'*' {
                    in_block_comment = true;
                    i += 2;
                    continue;
                }
            }

            match b {
                b'\'' => in_single_quote = true,
                b'"' => in_double_quote = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if pred(depth) {
                        return Some(line_idx);
                    }
                }
                _ => {}
            }

            i += 1;
        }
    }

    None
}

impl Backend {
    pub(crate) fn laravel_macro_token_cached(&self, uri: &str, content: &str) -> bool {
        let has_token = memchr::memmem::find(content.as_bytes(), b"macro(").is_some();
        self.laravel_macro_token_cache
            .write()
            .insert(uri.to_string(), has_token);
        has_token
    }

    /// Look up a class by its (possibly namespace-qualified) name in the
    /// in-memory `uri_classes_index`, without triggering any disk I/O.
    ///
    /// The `class_name` can be:
    ///   - A simple name like `"Customer"`
    ///   - A namespace-qualified name like `"Klarna\\Customer"`
    ///   - A fully-qualified name like `"\\Klarna\\Customer"` (leading `\` is stripped)
    ///
    /// When a namespace prefix is present, the file's namespace (from
    /// `namespace_map`) must match for the class to be returned.  This
    /// prevents `"Demo\\PDO"` from matching the global `PDO` stub.
    ///
    /// Returns a shared `Arc<ClassInfo>` if found, or `None`.
    pub(crate) fn find_class_in_uri_classes_index(
        &self,
        class_name: &str,
    ) -> Option<Arc<ClassInfo>> {
        // ── Fast path: O(1) lookup via fqn_index ──
        // For namespace-qualified names the FQN is the normalized name
        // itself.  For bare names (no backslash) the FQN equals the
        // short name, which is also stored in the index.
        if let Some(cls) = self.fqn_class_index.read().get(class_name) {
            return Some(Arc::clone(cls));
        }

        // The fqn_class_index is always populated before (or at the
        // same time as) uri_classes_index, so if the O(1) lookup above
        // missed, a linear scan would not find it either.
        None
    }

    /// Get the content of a file by URI, trying open files first then disk.
    ///
    /// This replaces the repeated pattern of locking `open_files`, looking
    /// up the URI, and falling back to reading from disk via
    /// `Url::to_file_path` + `std::fs::read_to_string`.  Three call sites
    /// in the definition modules used this exact sequence.
    pub(crate) fn get_file_content(&self, uri: &str) -> Option<String> {
        if let Some(content) = self.open_files.read().get(uri) {
            return Some(String::clone(content));
        }

        // Embedded class stubs live under synthetic `phpantom-stub://`
        // URIs and have no on-disk file.  Retrieve the raw source from
        // the stub_index keyed by the class short name (the URI path).
        if let Some(class_name) = uri.strip_prefix("phpantom-stub://") {
            let stub_idx = self.stub_index.read();
            return stub_idx.get(class_name).map(|s| s.to_string());
        }

        // Embedded function stubs use `phpantom-stub-fn://` URIs.
        // The path component is the function name used as key in
        // stub_function_index.
        if let Some(func_name) = uri.strip_prefix("phpantom-stub-fn://") {
            let stub_fn_idx = self.stub_function_index.read();
            return stub_fn_idx.get(func_name).map(|s| s.to_string());
        }

        let path = Url::parse(uri).ok()?.to_file_path().ok()?;
        std::fs::read_to_string(path).ok()
    }

    /// Retrieve file content as a cheap `Arc<String>` reference when the
    /// file is in `open_files`.  Falls back to reading from disk (which
    /// wraps the result in a new `Arc`).
    ///
    /// Prefer this over [`get_file_content`] in hot paths where the
    /// content will be shared across tasks or stored for the duration
    /// of a request, since it avoids deep-cloning the file string.
    pub(crate) fn get_file_content_arc(&self, uri: &str) -> Option<Arc<String>> {
        if let Some(content) = self.open_files.read().get(uri) {
            return Some(Arc::clone(content));
        }

        // Embedded class stubs live under synthetic `phpantom-stub://`
        // URIs and have no on-disk file.
        if let Some(class_name) = uri.strip_prefix("phpantom-stub://") {
            let stub_idx = self.stub_index.read();
            return stub_idx.get(class_name).map(|s| Arc::new(s.to_string()));
        }

        // Embedded function stubs use `phpantom-stub-fn://` URIs.
        if let Some(func_name) = uri.strip_prefix("phpantom-stub-fn://") {
            let stub_fn_idx = self.stub_function_index.read();
            return stub_fn_idx.get(func_name).map(|s| Arc::new(s.to_string()));
        }

        let path = Url::parse(uri).ok()?.to_file_path().ok()?;
        std::fs::read_to_string(path).ok().map(Arc::new)
    }

    /// Public helper for tests: get the uri_classes_index entry for a given URI.
    pub fn get_classes_for_uri(&self, uri: &str) -> Option<Vec<ClassInfo>> {
        self.uri_classes_index
            .read()
            .get(uri)
            .map(|classes| classes.iter().map(|c| ClassInfo::clone(c)).collect())
    }

    /// Gather the per-file context (classes, use-map, namespace) in one call.
    ///
    /// This replaces the repeated lock-and-unwrap boilerplate that was
    /// duplicated across the completion handler, definition resolver,
    /// member definition, implementation resolver, and variable definition
    /// modules.  Each of those sites used to have three nearly-identical
    /// blocks acquiring `uri_classes_index`, `use_map`, and `namespace_map` locks
    /// and extracting the entry for a given URI.
    pub(crate) fn file_context(&self, uri: &str) -> FileContext {
        let classes = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        // The legacy use_map (short name → FQN from `use` statements)
        // remains the canonical import table.  `resolved_names` is a
        // supplementary data source for consumers that can query by
        // byte offset — it must NOT replace the use_map because
        // `to_use_map()` only contains names that are actually
        // *referenced* in the code, not all *declared* imports.
        // The unused-imports diagnostic relies on seeing declared-but-
        // unreferenced imports.
        let use_map = self
            .file_imports
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        let namespace = self
            .file_namespaces
            .read()
            .get(uri)
            .and_then(|spans| spans.first())
            .and_then(|s| s.namespace.clone());

        let resolved_names = self.resolved_names.read().get(uri).cloned();

        FileContext {
            classes,
            use_map,
            namespace,
            resolved_names,
        }
    }

    /// Like [`file_context`](Self::file_context) but resolves the namespace
    /// for the namespace block that contains `byte_offset`.
    ///
    /// In single-namespace files this returns the same result as
    /// `file_context`.  In multi-namespace files it picks the correct
    /// namespace block for the cursor position.
    pub(crate) fn file_context_at(&self, uri: &str, byte_offset: u32) -> FileContext {
        let classes = self
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let use_map = self
            .file_imports
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let namespace = self.namespace_at_offset(uri, byte_offset);
        let resolved_names = self.resolved_names.read().get(uri).cloned();

        FileContext {
            classes,
            use_map,
            namespace,
            resolved_names,
        }
    }

    /// Return the namespace that contains the given byte offset in a file.
    ///
    /// For single-namespace files (the common case) this returns the file's
    /// only namespace.  For multi-namespace files it finds the namespace
    /// block whose byte range contains `byte_offset`.  Returns `None` when
    /// the offset is in the global namespace or the file has no namespace.
    pub(crate) fn namespace_at_offset(&self, uri: &str, byte_offset: u32) -> Option<String> {
        let nmap = self.file_namespaces.read();
        let spans = nmap.get(uri)?;
        // Try to find the namespace block containing the offset.
        for span in spans {
            if byte_offset >= span.start && byte_offset <= span.end {
                return span.namespace.clone();
            }
        }
        // Fallback: if the offset is past all namespace blocks (e.g.
        // code after the last closing brace), return the last namespace.
        spans.last().and_then(|s| s.namespace.clone())
    }

    /// Return the first namespace declared in a file.
    ///
    /// For single-namespace files this is the file's namespace.  For
    /// multi-namespace files this returns the first block's namespace,
    /// which may not be correct for all positions in the file.  Prefer
    /// [`namespace_at_offset`](Self::namespace_at_offset) when a cursor
    /// position is available.
    pub(crate) fn first_file_namespace(&self, uri: &str) -> Option<String> {
        self.file_namespaces
            .read()
            .get(uri)
            .and_then(|spans| spans.first())
            .and_then(|s| s.namespace.clone())
    }

    /// Return the import table (short name → FQN) for a file.
    ///
    /// Returns the legacy `use_map` which contains all *declared*
    /// imports from `use` statements, regardless of whether they are
    /// actually referenced in the code.  This is the correct source
    /// for consumers that need the full import table (unused-import
    /// detection, import-class code actions, name resolution helpers).
    ///
    /// For consumers that can resolve names by byte offset, prefer
    /// querying `resolved_names` directly via [`file_context`] instead.
    pub(crate) fn file_use_map(&self, uri: &str) -> std::collections::HashMap<String, String> {
        self.file_imports
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default()
    }

    /// Remove a file's entries from `uri_classes_index`, `use_map`, and `namespace_map`.
    ///
    /// This is the mirror of [`file_context`](Self::file_context): where that
    /// method *reads* the three maps, this method *clears* them for a given URI.
    /// Called from `did_close` to clean up state when a file is closed.
    pub(crate) fn clear_file_maps(&self, uri: &str) {
        // Drop per-file maps that are only needed while the file is
        // open.  uri_classes_index is redundant with fqn_class_index once indexing is
        // complete — GTD falls back to fqn_uri_index + parse_and_cache_file
        // when the uri_classes_index entry is missing.
        self.uri_classes_index.write().remove(uri);
        self.symbol_maps.write().remove(uri);
        self.file_imports.write().remove(uri);
        self.resolved_names.write().remove(uri);
        self.file_namespaces.write().remove(uri);
        // NOTE: We intentionally keep fqn_uri_index and fqn_class_index intact.
        // fqn_uri_index maps FQN → URI so GTD can locate the file, and
        // fqn_class_index keeps the full ClassInfo for cross-file resolution.
        // The file will be re-parsed from disk on next access via
        // parse_and_cache_file when needed (issue #99).
    }

    pub(crate) async fn log(&self, typ: MessageType, message: String) {
        if let Some(client) = &self.client {
            client.log_message(typ, message).await;
        }
    }

    // ── Work-done progress helpers ──────────────────────────────────

    /// Create a server-initiated work-done progress token and send the
    /// `window/workDoneProgress/create` request to the client.
    ///
    /// Returns `Some(token)` on success, `None` when there is no client
    /// or the client rejects the request.  The caller should pass the
    /// returned token to [`progress_begin`], [`progress_report`], and
    /// [`progress_end`].
    pub(crate) async fn progress_create(&self, token_name: &str) -> Option<NumberOrString> {
        use tower_lsp::lsp_types::request::WorkDoneProgressCreate;

        // Per the LSP spec, servers must only use
        // window/workDoneProgress/create when the client signals
        // support via the window.workDoneProgress capability.
        if !self
            .supports_work_done_progress
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }

        let client = self.client.as_ref()?;
        let token = NumberOrString::String(token_name.to_string());
        let params = WorkDoneProgressCreateParams {
            token: token.clone(),
        };
        // Use a timeout to avoid deadlocking the service loop.
        // progress_create is a server-to-client request; if the
        // client is busy (e.g. flooding didClose/hover), awaiting
        // indefinitely could block the handler and starve other
        // requests.  Progress reporting is best-effort, so timing
        // out is harmless.
        match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.send_request::<WorkDoneProgressCreate>(params),
        )
        .await
        {
            Ok(Ok(())) => Some(token),
            _ => None,
        }
    }

    /// Send a `WorkDoneProgressBegin` notification for the given token.
    ///
    /// `title` is the short label shown by the editor (e.g. "Indexing").
    /// `message` is an optional detail line (e.g. "Scanning subprojects").
    pub(crate) async fn progress_begin(
        &self,
        token: &NumberOrString,
        title: &str,
        message: Option<String>,
    ) {
        use tower_lsp::lsp_types::notification::Progress;

        let Some(client) = &self.client else { return };
        client
            .send_notification::<Progress>(ProgressParams {
                token: token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                    WorkDoneProgressBegin {
                        title: title.to_string(),
                        cancellable: Some(false),
                        message,
                        percentage: Some(0),
                    },
                )),
            })
            .await;
    }

    /// Send a `WorkDoneProgressReport` notification with a percentage
    /// and optional message.
    ///
    /// `percentage` should be in the range 0..=100.  `message` replaces
    /// the previous detail line when `Some`.
    pub(crate) async fn progress_report(
        &self,
        token: &NumberOrString,
        percentage: u32,
        message: Option<String>,
    ) {
        use tower_lsp::lsp_types::notification::Progress;

        let Some(client) = &self.client else { return };
        client
            .send_notification::<Progress>(ProgressParams {
                token: token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                    WorkDoneProgressReport {
                        cancellable: Some(false),
                        message,
                        percentage: Some(percentage),
                    },
                )),
            })
            .await;
    }

    /// Send a `WorkDoneProgressEnd` notification.
    ///
    /// After this call the editor removes the progress indicator.
    /// `message` is an optional final status line (e.g. "Indexed 5,678
    /// classes").
    pub(crate) async fn progress_end(&self, token: &NumberOrString, message: Option<String>) {
        use tower_lsp::lsp_types::notification::Progress;

        let Some(client) = &self.client else { return };
        client
            .send_notification::<Progress>(ProgressParams {
                token: token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
                    message,
                })),
            })
            .await;
    }
}

// ─── Self-keyword helpers ───────────────────────────────────────────────────

/// Returns `true` if `s` is one of the PHP keywords that refer to the
/// *current* class (not the parent): `self`, `static`, or `$this`.
///
/// Callers that also need to match `parent` should add a separate
/// `eq_ignore_ascii_case("parent")` check, because `parent` resolves
/// to the *parent* class rather than the current one.
///
/// The comparison is case-insensitive for `self` and `static`.
/// `$this` is matched literally (it is always lowercase in PHP).
pub(crate) fn is_self_or_static(s: &str) -> bool {
    s.eq_ignore_ascii_case("self") || s.eq_ignore_ascii_case("static") || s == "$this"
}

/// Returns `true` if `s` is one of the PHP class-keyword references:
/// `self`, `static`, `$this`, or `parent`.
///
/// Use this when you need a single guard that covers *all* class
/// keywords, including `parent`.  For the subset that resolves to the
/// *current* class only, use [`is_self_or_static`].
pub(crate) fn is_class_keyword(s: &str) -> bool {
    is_self_or_static(s) || s.eq_ignore_ascii_case("parent")
}

/// Resolve `self`, `static`, `$this`, or `parent` to a class name.
///
/// Returns `Some(class_name)` when the keyword can be resolved, or
/// `None` when:
/// - `keyword` is not a recognised class keyword, or
/// - there is no `current_class`, or
/// - `parent` is used but the class has no parent.
///
/// This centralises the keyword → class-name mapping that was
/// previously duplicated across 10+ call sites.
pub(crate) fn resolve_class_keyword(
    keyword: &str,
    current_class: Option<&ClassInfo>,
) -> Option<String> {
    if is_self_or_static(keyword) {
        current_class.map(|cc| cc.name.to_string())
    } else if keyword.eq_ignore_ascii_case("parent") {
        current_class.and_then(|cc| cc.parent_class.map(|a| a.to_string()))
    } else {
        None
    }
}

// ─── Shared helpers for code actions and diagnostics ────────────────────────

/// Check if a line contains the `function` keyword as a standalone word
/// (not part of a larger identifier like `$functionality`).
pub(crate) fn contains_function_keyword(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(pos) = trimmed.find("function") else {
        return false;
    };
    let before_ok = pos == 0 || trimmed.as_bytes()[pos - 1].is_ascii_whitespace();
    let after_pos = pos + "function".len();
    let after_ok = after_pos >= trimmed.len()
        || !trimmed.as_bytes()[after_pos].is_ascii_alphanumeric()
            && trimmed.as_bytes()[after_pos] != b'_';
    before_ok && after_ok
}

/// Check if a `#[...]` line contains a specific PHP attribute name.
///
/// Matches patterns like `#[Override]`, `#[\Override]`,
/// `#[Override, SomethingElse]`, `#[SomethingElse, \Override]`, etc.
/// The attribute name is matched as a standalone token: preceded by
/// `[`, `\`, `,`, or whitespace and followed by `]`, `,`, `(`, or
/// whitespace.
pub(crate) fn contains_php_attribute(line: &str, attr_name: &[u8]) -> bool {
    let bytes = line.as_bytes();
    let target_len = attr_name.len();

    let mut i = 0;
    while i + target_len <= bytes.len() {
        if &bytes[i..i + target_len] == attr_name {
            let ok_before = if i == 0 {
                false
            } else {
                let prev = bytes[i - 1];
                prev == b'[' || prev == b'\\' || prev == b',' || prev == b' ' || prev == b'\t'
            };
            let ok_after = if i + target_len >= bytes.len() {
                true
            } else {
                let next = bytes[i + target_len];
                next == b']' || next == b',' || next == b'(' || next == b' ' || next == b'\t'
            };
            if ok_before && ok_after {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Find all occurrences of `needle` in `content` within the byte range
/// `[scope_start, scope_end)` that are textually identical to the selected
/// expression, excluding the original selection `[sel_start, sel_end)`.
///
/// Returns `(start, end)` byte offset pairs. Word boundaries are checked
/// so that substrings of longer identifiers are not matched.
pub(crate) fn find_identical_occurrences(
    content: &str,
    needle: &str,
    sel_start: usize,
    sel_end: usize,
    scope_start: usize,
    scope_end: usize,
) -> Vec<(usize, usize)> {
    if needle.is_empty() || scope_start >= scope_end || scope_end > content.len() {
        return Vec::new();
    }
    let haystack = &content[scope_start..scope_end];
    let mut results = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = haystack[search_from..].find(needle) {
        let abs_start = scope_start + search_from + pos;
        let abs_end = abs_start + needle.len();
        // Skip the original selection.
        if abs_start != sel_start || abs_end != sel_end {
            // Check word boundaries to avoid matching substrings.
            let before_ok = abs_start == 0
                || !content.as_bytes()[abs_start - 1].is_ascii_alphanumeric()
                    && content.as_bytes()[abs_start - 1] != b'_'
                    && content.as_bytes()[abs_start - 1] != b'$';
            let after_ok = abs_end >= content.len()
                || !content.as_bytes()[abs_end].is_ascii_alphanumeric()
                    && content.as_bytes()[abs_end] != b'_';
            if before_ok && after_ok {
                results.push((abs_start, abs_end));
            }
        }
        search_from = search_from + pos + 1;
    }
    results
}

/// Infer a [`PhpType`] from a literal expression string.
///
/// Recognises integer, float, boolean, null, and string literals as
/// well as empty arrays (`[]`).  Returns `None` for anything that
/// is not a simple literal — callers should fall back to the full
/// type resolver for those cases.
///
/// This is the shared core used by:
/// - `code_actions::phpstan::fix_return_type::infer_type_from_literal`
///   (extended wrapper that also handles `new` expressions and array
///   literal contents)
/// - `code_actions::extract_constant::literal_type_name`
/// - `parser::classes` (Team 3, future)
pub(crate) fn infer_type_from_literal(expr: &str) -> Option<PhpType> {
    // Integer literal (decimal, hex, octal, binary — all parse as i64
    // after stripping underscores for PHP 7.4+ numeric separators).
    let clean = expr.replace('_', "");
    if clean.parse::<i64>().is_ok() {
        return Some(PhpType::int());
    }
    // Hex / octal / binary that i64 doesn't cover directly.
    if (clean.starts_with("0x") || clean.starts_with("0X"))
        && i64::from_str_radix(&clean[2..], 16).is_ok()
    {
        return Some(PhpType::int());
    }
    if (clean.starts_with("0b") || clean.starts_with("0B"))
        && i64::from_str_radix(&clean[2..], 2).is_ok()
    {
        return Some(PhpType::int());
    }
    // Octal
    if clean.starts_with('0')
        && clean.len() > 1
        && clean[1..].chars().all(|c| c.is_ascii_digit())
        && i64::from_str_radix(&clean[1..], 8).is_ok()
    {
        return Some(PhpType::int());
    }

    // Float literal (must contain `.`, `e`, or `E` to distinguish from int).
    if (clean.contains('.') || clean.contains('e') || clean.contains('E'))
        && clean.parse::<f64>().is_ok()
    {
        return Some(PhpType::float());
    }

    // Negative numeric literals.
    if let Some(stripped) = expr.strip_prefix('-') {
        let abs = stripped.trim_start();
        if let Some(inner) = infer_type_from_literal(abs)
            && (inner.is_int() || inner.is_float())
        {
            return Some(inner);
        }
    }

    // Boolean literals.
    if expr.eq_ignore_ascii_case("true") || expr.eq_ignore_ascii_case("false") {
        return Some(PhpType::bool());
    }

    // Null.
    if expr.eq_ignore_ascii_case("null") {
        return Some(PhpType::null());
    }

    // String literals (single- or double-quoted).
    if (expr.starts_with('\'') && expr.ends_with('\''))
        || (expr.starts_with('"') && expr.ends_with('"'))
    {
        return Some(PhpType::string());
    }

    // Empty array literal.
    if expr == "[]" {
        return Some(PhpType::array());
    }

    // Not a simple literal.
    None
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
    members: impl Iterator<Item = &'a mago_syntax::ast::class_like::member::ClassLikeMember<'a>>,
    offset: u32,
) -> Option<&'a mago_syntax::ast::block::Block<'a>> {
    use mago_syntax::ast::class_like::member::ClassLikeMember;
    use mago_syntax::ast::class_like::method::MethodBody;

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

/// Push a location only if it is not already present (deduplication).
pub fn push_unique_location(
    locations: &mut Vec<Location>,
    uri: &Url,
    start: Position,
    end: Position,
) {
    let already_present = locations.iter().any(|l| {
        l.uri == *uri
            && l.range.start.line == start.line
            && l.range.start.character == start.character
    });
    if !already_present {
        locations.push(Location {
            uri: uri.clone(),
            range: Range { start, end },
        });
    }
}

/// Result of running an external command via [`run_command_with_timeout`].
#[derive(Debug)]
pub struct CommandOutput {
    /// Exit code (or -1 if the process was killed / no code available).
    pub code: i32,
    /// Captured stdout content.
    pub stdout: String,
    /// Captured stderr content.
    pub stderr: String,
}

/// Spawn a command, feed it optional stdin, wait for it with a timeout,
/// and return its exit code plus captured stdout/stderr.
///
/// stdout and stderr are drained on dedicated reader threads that run
/// **while** the child is alive. This is essential: a child that writes
/// more than the OS pipe buffer (~64 KB — easily exceeded by the JSON
/// output of PHPStan/PHPCS/Mago on a real project) blocks on the write
/// until the pipe is drained. If we only read after the process exits,
/// the child can never exit and the call spins until it times out,
/// returning an error instead of the diagnostics. Reading concurrently
/// keeps the pipe from filling.
///
/// When `stdin_content` is `Some`, it is written to the child's stdin
/// and the pipe is then closed (EOF). The write happens after the reader
/// threads are started so a large stdin payload cannot deadlock against
/// the child's output. When `stdin_content` is `None`, stdin is set to
/// null so the child never inherits the server's stdin.
///
/// `tool_name` is used only for error messages. On timeout or
/// cancellation the child is killed and an `Err` is returned.
pub fn run_command_with_timeout(
    command: &mut std::process::Command,
    timeout: std::time::Duration,
    cancelled: &std::sync::atomic::AtomicBool,
    tool_name: &str,
    stdin_content: Option<&str>,
) -> Result<CommandOutput, String> {
    use std::io::{Read, Write};
    use std::process::Stdio;
    use std::sync::atomic::Ordering;

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_content.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", tool_name, e))?;

    // Drain stdout/stderr concurrently so the child can never block
    // writing to a full pipe while we wait for it to exit.
    let stdout_reader = child.stdout.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            buf
        })
    });
    let stderr_reader = child.stderr.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            buf
        })
    });

    // Feed stdin (if any) and close it so the child sees EOF. A broken
    // pipe here means the child exited early; the status/output below is
    // what we care about, so the write error is intentionally ignored.
    if let Some((content, mut stdin)) = stdin_content.zip(child.stdin.take()) {
        let _ = stdin.write_all(content.as_bytes());
    }

    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{} timed out after {}ms",
                        tool_name,
                        timeout.as_millis()
                    ));
                }
                if cancelled.load(Ordering::Acquire) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{} cancelled (server shutting down)", tool_name));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("Error waiting for {}: {}", tool_name, e));
            }
        }
    };

    // The child has exited, so its pipe write ends are closed and the
    // reader threads will reach EOF; join them to collect the output.
    let stdout = stdout_reader
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr = stderr_reader
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    Ok(CommandOutput {
        code: status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
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

    #[test]
    fn line_index_matches_offset_to_position() {
        // Include multi-byte (é = 2 bytes / 1 UTF-16 unit) and an emoji
        // (🎉 = 4 bytes / 2 UTF-16 units) to exercise the column math.
        let content = "<?php\n$x = 'é';\n// 🎉 comment\nfinal;\n";
        let index = LineIndex::new(content);
        // Every char boundary must agree with the scanning implementation.
        for (offset, _) in content
            .char_indices()
            .chain(std::iter::once((content.len(), ' ')))
        {
            assert_eq!(
                index.position(offset),
                offset_to_position(content, offset),
                "mismatch at offset {offset}"
            );
        }
    }

    #[test]
    fn line_index_clamps_past_end() {
        let content = "ab\ncd";
        let index = LineIndex::new(content);
        assert_eq!(
            index.position(999),
            offset_to_position(content, content.len())
        );
    }

    #[test]
    fn line_start_byte_offset_lf() {
        let content = "aaa\nbb\nc\n";
        assert_eq!(line_start_byte_offset(content, 0), 0);
        assert_eq!(line_start_byte_offset(content, 1), 4); // after "aaa\n"
        assert_eq!(line_start_byte_offset(content, 2), 7); // after "bb\n"
        assert_eq!(line_start_byte_offset(content, 3), 9); // after "c\n" (EOF)
    }

    #[test]
    fn line_start_byte_offset_crlf() {
        let content = "aaa\r\nbb\r\nc\r\n";
        assert_eq!(line_start_byte_offset(content, 0), 0);
        assert_eq!(line_start_byte_offset(content, 1), 5); // after "aaa\r\n"
        assert_eq!(line_start_byte_offset(content, 2), 9); // after "bb\r\n"
        assert_eq!(line_start_byte_offset(content, 3), 12); // after "c\r\n" (EOF)
    }

    #[test]
    fn line_start_byte_offset_past_end_returns_len() {
        let content = "one\ntwo";
        assert_eq!(line_start_byte_offset(content, 5), content.len());
    }

    #[test]
    fn is_self_or_static_matches_three() {
        assert!(is_self_or_static("self"));
        assert!(is_self_or_static("static"));
        assert!(is_self_or_static("$this"));
    }

    #[test]
    fn is_self_or_static_excludes_parent() {
        assert!(!is_self_or_static("parent"));
        assert!(!is_self_or_static("Parent"));
        assert!(!is_self_or_static("PARENT"));
    }

    #[test]
    fn is_self_or_static_case_insensitive() {
        assert!(is_self_or_static("Self"));
        assert!(is_self_or_static("SELF"));
        assert!(is_self_or_static("Static"));
        assert!(is_self_or_static("STATIC"));
    }

    #[test]
    fn is_self_or_static_rejects_others() {
        assert!(!is_self_or_static(""));
        assert!(!is_self_or_static("this"));
        assert!(!is_self_or_static("Foo"));
    }

    /// Helper to build a minimal `ClassInfo` for hierarchy tests.
    fn make_class(
        name: &str,
        namespace: Option<&str>,
        parent: Option<&str>,
        interfaces: &[&str],
    ) -> Arc<crate::types::ClassInfo> {
        Arc::new(crate::types::ClassInfo {
            name: crate::atom::atom(name),
            file_namespace: namespace.map(crate::atom::atom),
            parent_class: parent.map(crate::atom::atom),
            interfaces: interfaces.iter().map(|s| crate::atom::atom(s)).collect(),
            ..Default::default()
        })
    }

    /// Build a class loader from a slice of `Arc<ClassInfo>`.
    fn loader_from(
        classes: &[Arc<crate::types::ClassInfo>],
    ) -> impl Fn(&str) -> Option<Arc<crate::types::ClassInfo>> + '_ {
        move |name: &str| classes.iter().find(|c| c.fqn() == name).cloned()
    }

    // ── is_subtype_of: FQN self-check ──────────────────────────

    #[test]
    fn subtype_of_self_by_fqn() {
        let cls = make_class("User", Some("App\\Models"), None, &[]);
        let classes = [cls.clone()];
        let loader = loader_from(&classes);
        assert!(is_subtype_of(&cls, "App\\Models\\User", &loader));
    }

    #[test]
    fn subtype_of_self_root_namespace() {
        // Root-namespace class: FQN == short name.
        let cls = make_class("RuntimeException", None, None, &[]);
        let classes = [cls.clone()];
        let loader = loader_from(&classes);
        assert!(is_subtype_of(&cls, "RuntimeException", &loader));
    }

    #[test]
    fn subtype_of_self_short_name_resolves_via_loader() {
        // Passing a short name that the loader can resolve to a FQN.
        let cls = make_class("User", Some("App\\Models"), None, &[]);
        let classes = [cls.clone()];
        let loader = loader_from(&classes);
        // The loader finds "User" → no, it only matches on fqn().
        // So passing just "User" when the class is App\Models\User
        // should NOT match (different FQN).
        assert!(!is_subtype_of(&cls, "User", &loader));
    }

    // ── is_subtype_of: interface matching by FQN ────────────────

    #[test]
    fn subtype_of_interface_fqn_match() {
        let cls = make_class(
            "UserRepo",
            Some("App\\Repos"),
            None,
            &["App\\Contracts\\Repository"],
        );
        let iface = make_class("Repository", Some("App\\Contracts"), None, &[]);
        let classes = [cls.clone(), iface];
        let loader = loader_from(&classes);
        assert!(is_subtype_of(&cls, "App\\Contracts\\Repository", &loader));
    }

    #[test]
    fn subtype_of_interface_short_name_does_not_match_different_namespace() {
        // Two unrelated classes that share the short name "Carbon".
        // `is_subtype_of` must NOT treat them as the same type.
        let vendor_carbon = make_class("Carbon", Some("Vendor\\DateTime"), None, &[]);
        let cls = make_class("MyDate", Some("App"), None, &["Vendor\\DateTime\\Carbon"]);
        let app_carbon = make_class("Carbon", Some("App\\DateTime"), None, &[]);
        let classes = [cls.clone(), vendor_carbon, app_carbon.clone()];
        let loader = loader_from(&classes);

        // The class implements Vendor\DateTime\Carbon, NOT App\DateTime\Carbon.
        assert!(is_subtype_of(&cls, "Vendor\\DateTime\\Carbon", &loader));
        assert!(!is_subtype_of(&cls, "App\\DateTime\\Carbon", &loader));
    }

    // ── is_subtype_of: parent chain by FQN ──────────────────────

    #[test]
    fn subtype_of_parent_fqn() {
        let parent = make_class("BaseModel", Some("App\\Models"), None, &[]);
        let child = make_class(
            "User",
            Some("App\\Models"),
            Some("App\\Models\\BaseModel"),
            &[],
        );
        let classes = [parent, child.clone()];
        let loader = loader_from(&classes);
        assert!(is_subtype_of(&child, "App\\Models\\BaseModel", &loader));
    }

    #[test]
    fn subtype_of_grandparent_fqn() {
        let grandparent = make_class("Model", Some("Illuminate"), None, &[]);
        let parent = make_class("BaseModel", Some("App"), Some("Illuminate\\Model"), &[]);
        let child = make_class("User", Some("App"), Some("App\\BaseModel"), &[]);
        let classes = [grandparent, parent, child.clone()];
        let loader = loader_from(&classes);
        assert!(is_subtype_of(&child, "Illuminate\\Model", &loader));
    }

    #[test]
    fn subtype_of_parent_interface_fqn() {
        // Parent implements an interface; child should also be a subtype.
        let iface = make_class("Countable", None, None, &[]);
        let parent = make_class("Collection", Some("App"), None, &["Countable"]);
        let child = make_class("UserCollection", Some("App"), Some("App\\Collection"), &[]);
        let classes = [iface, parent, child.clone()];
        let loader = loader_from(&classes);
        assert!(is_subtype_of(&child, "Countable", &loader));
    }

    #[test]
    fn subtype_of_unrelated_class_returns_false() {
        let cls = make_class("User", Some("App"), None, &[]);
        let other = make_class("Order", Some("App"), None, &[]);
        let classes = [cls.clone(), other];
        let loader = loader_from(&classes);
        assert!(!is_subtype_of(&cls, "App\\Order", &loader));
    }

    // ── is_subtype_of: short ancestor resolves through loader ───

    #[test]
    fn subtype_of_short_ancestor_resolved_by_loader() {
        // The ancestor name "RuntimeException" is a root-namespace class.
        // The loader can resolve it, and the comparison should work.
        let exc = make_class("RuntimeException", None, None, &["Exception"]);
        let cls = make_class("AppException", Some("App"), Some("RuntimeException"), &[]);
        let classes = [exc, cls.clone()];
        let loader = loader_from(&classes);
        assert!(is_subtype_of(&cls, "RuntimeException", &loader));
    }

    // ── is_subtype_of: global name shadowed by consuming file's use-map ─

    #[test]
    fn subtype_of_global_interface_not_broken_by_shadowing_use_import() {
        // A project class shares the short name of a global interface
        // (`Iterator`).  The consuming file imports the project class
        // (`use App\Input\Iterator;`), so its use-map maps the unqualified
        // name `Iterator` to `App\Input\Iterator`.  Subtype checks against
        // the global `\Iterator` / `\Traversable` — reached while walking a
        // stub class's hierarchy — must still succeed.
        let traversable = make_class("Traversable", None, None, &[]);
        let iterator = make_class("Iterator", None, None, &["Traversable"]);
        let recursive_iterator = make_class("RecursiveIterator", None, None, &["Iterator"]);
        let rec_dir_iterator = make_class(
            "RecursiveDirectoryIterator",
            None,
            None,
            &["RecursiveIterator"],
        );
        // The shadowing project class (imported into the consuming file).
        let app_iterator = make_class("Iterator", Some("App\\Input"), None, &[]);

        let classes = [
            traversable,
            iterator,
            recursive_iterator,
            rec_dir_iterator.clone(),
            app_iterator.clone(),
        ];

        // A loader that mirrors `class_loader_with`: an unqualified name in
        // the use-map resolves to the imported class first; the `__fqn__\`
        // bypass skips the use-map and resolves the global short name.
        let loader = move |name: &str| -> Option<Arc<crate::types::ClassInfo>> {
            let stripped = name.strip_prefix('\\').unwrap_or(name);
            if !stripped.contains('\\') && stripped == "Iterator" {
                // use-map shadow: unqualified `Iterator` → App\Input\Iterator
                return Some(app_iterator.clone());
            }
            if let Some(cls) = classes.iter().find(|c| c.fqn() == stripped).cloned() {
                return Some(cls);
            }
            if stripped.contains('\\') {
                let short = crate::util::short_name(stripped);
                return classes.iter().find(|c| c.fqn() == short).cloned();
            }
            None
        };

        // Without the fix, the intermediate global `Iterator` node resolves
        // to `App\Input\Iterator`, so the walk never reaches `Traversable`
        // and the ancestor `Iterator` itself mis-resolves.
        assert!(is_subtype_of(&rec_dir_iterator, "Iterator", &loader));
        assert!(is_subtype_of(&rec_dir_iterator, "Traversable", &loader));
        // A genuinely unrelated global class is still rejected.
        assert!(!is_subtype_of(&rec_dir_iterator, "Countable", &loader));
    }

    // ── run_command_with_timeout ────────────────────────────────────

    /// A child that writes far more than the OS pipe buffer (~64 KB)
    /// must not deadlock: the reader threads keep the pipe drained while
    /// the child runs, so it can exit and we collect the full output.
    #[cfg(unix)]
    #[test]
    fn run_command_drains_large_stdout_without_deadlock() {
        use std::process::Command;
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;

        // 200_000 NUL bytes (valid UTF-8), well above the pipe buffer.
        let mut cmd = Command::new("head");
        cmd.arg("-c").arg("200000").arg("/dev/zero");
        let cancelled = AtomicBool::new(false);
        let out =
            run_command_with_timeout(&mut cmd, Duration::from_secs(10), &cancelled, "test", None)
                .expect("command should complete");
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout.len(), 200000);
    }

    /// Feeding large stdin while the child writes large stdout (here
    /// `cat`, which echoes stdin) exercises both pipes at once. Under the
    /// old read-after-exit logic this deadlocked.
    #[cfg(unix)]
    #[test]
    fn run_command_echoes_large_stdin() {
        use std::process::Command;
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;

        let payload = "x".repeat(200000);
        let mut cmd = Command::new("cat");
        let cancelled = AtomicBool::new(false);
        let out = run_command_with_timeout(
            &mut cmd,
            Duration::from_secs(10),
            &cancelled,
            "test",
            Some(&payload),
        )
        .expect("command should complete");
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, payload);
    }

    /// A long-running child is killed when the timeout elapses, returning
    /// an error rather than hanging.
    #[cfg(unix)]
    #[test]
    fn run_command_times_out() {
        use std::process::Command;
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;

        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        let cancelled = AtomicBool::new(false);
        let result = run_command_with_timeout(
            &mut cmd,
            Duration::from_millis(100),
            &cancelled,
            "test",
            None,
        );
        let err = result.expect_err("should time out");
        assert!(err.contains("timed out"), "unexpected error: {err}");
    }

    /// A spawn failure surfaces as an error rather than panicking.
    #[test]
    fn run_command_reports_spawn_failure() {
        use std::process::Command;
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;

        let mut cmd = Command::new("phpantom-no-such-binary-xyz");
        let cancelled = AtomicBool::new(false);
        let result =
            run_command_with_timeout(&mut cmd, Duration::from_secs(1), &cancelled, "test", None);
        let err = result.expect_err("spawn should fail");
        assert!(
            err.contains("Failed to spawn test"),
            "unexpected error: {err}"
        );
    }
}
