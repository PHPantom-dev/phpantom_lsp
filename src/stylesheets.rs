//! The CSS class names the project's own stylesheets declare.
//!
//! A class name written in PHP (`Html::div()->class('card-body')` in Yii)
//! is declared by every class selector that names it, `.card-body { … }`,
//! in a `.css`, `.scss`, or `.less` file of the workspace.  The files are
//! found with the same `.gitignore`-aware walk the PHP index uses, so
//! installed packages and published or built copies of the stylesheets
//! are left out, and read once, on first use.  A watched stylesheet that
//! changes drops the whole index, which is read again on the next use.
//!
//! Only the selectors are read: what a block declares, and anything an
//! at-rule's prelude says, are skipped, so `url(logo.png)` and `@media
//! (min-width: 1.5em)` name no class.  Nested rules are read like any
//! other, so `&.active` in a block declares `active`, but a class SCSS
//! builds out of its parent's name (`&-title`) or an interpolation has no
//! name to read.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use tower_lsp_server::gen_lsp_types::Location;

use crate::Backend;

/// The extensions of the stylesheets that are read.
const STYLESHEET_EXTENSIONS: [&str; 3] = ["css", "scss", "less"];

/// The index, read on first use and dropped when a stylesheet changes.
#[derive(Default)]
pub(crate) struct StylesheetState {
    index: RwLock<Option<Arc<StylesheetIndex>>>,
}

/// Every class selector of the workspace's stylesheets.
#[derive(Default)]
pub(crate) struct StylesheetIndex {
    /// The stylesheets, in the order the walk reached them.
    files: Vec<PathBuf>,
    /// Each class name, with the file (an index into [`Self::files`]) and
    /// the byte offset of every selector that declares it.
    classes: HashMap<String, Vec<(u32, u32)>>,
}

impl StylesheetIndex {
    /// Read every stylesheet under `root` the workspace walk reaches.
    fn load(
        root: &Path,
        vendor_dir_paths: &[PathBuf],
        filters: &Arc<crate::classmap_scanner::IndexFilters>,
    ) -> Self {
        let mut index = Self::default();
        let mut paths = Vec::new();
        crate::classmap_scanner::visit_workspace_files_gitignore(
            root,
            vendor_dir_paths,
            filters,
            None,
            |path| {
                if is_stylesheet_path(path)
                    && !path
                        .components()
                        .any(|component| component.as_os_str() == "node_modules")
                {
                    paths.push(path.to_path_buf());
                }
            },
        );
        for path in paths {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            index.add(path, &content);
        }
        index
    }

    /// Record the class selectors of one stylesheet.
    fn add(&mut self, path: PathBuf, content: &str) {
        let line_comments = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| !ext.eq_ignore_ascii_case("css"));
        let file = self.files.len() as u32;
        self.files.push(path);
        for (name, offset) in class_selectors(content, line_comments) {
            self.classes
                .entry(name)
                .or_default()
                .push((file, offset as u32));
        }
    }

    /// Every class name declared, sorted.
    pub(crate) fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.classes.keys().cloned().collect();
        names.sort_unstable();
        names
    }

    /// The stylesheets that declare `name`, each once, in walk order.
    pub(crate) fn files_declaring(&self, name: &str) -> Vec<&Path> {
        let mut files: Vec<&Path> = Vec::new();
        for (file, _) in self
            .classes
            .get(name)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let path = self.files[*file as usize].as_path();
            if !files.contains(&path) {
                files.push(path);
            }
        }
        files
    }
}

/// Whether `path` is a stylesheet whose selectors are read.
pub(crate) fn is_stylesheet_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            STYLESHEET_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

impl Backend {
    /// The class selectors of the workspace's stylesheets.
    pub(crate) fn stylesheet_index(&self) -> Arc<StylesheetIndex> {
        if let Some(index) = self.stylesheets.index.read().as_ref() {
            return Arc::clone(index);
        }
        let root = self.workspace.workspace_root.read().clone();
        let index = Arc::new(match root {
            Some(root) => {
                let vendor_dir_paths = self.workspace.vendor_dir_paths.lock().clone();
                StylesheetIndex::load(&root, &vendor_dir_paths, &self.index_filters())
            }
            None => StylesheetIndex::default(),
        });
        *self.stylesheets.index.write() = Some(Arc::clone(&index));
        index
    }

    /// Forget the stylesheets, so the next lookup reads them again.
    pub(crate) fn forget_stylesheets(&self) {
        *self.stylesheets.index.write() = None;
    }

    /// Every class selector that declares the class `name`.
    pub(crate) fn css_class_definitions(&self, name: &str) -> Vec<Location> {
        let index = self.stylesheet_index();
        let Some(declarations) = index.classes.get(name) else {
            return Vec::new();
        };
        let mut locations = Vec::new();
        let mut read: Option<(u32, String)> = None;
        for &(file, offset) in declarations {
            if read.as_ref().is_none_or(|(current, _)| *current != file) {
                let Ok(content) = std::fs::read_to_string(&index.files[file as usize]) else {
                    continue;
                };
                read = Some((file, content));
            }
            let (Some((_, content)), Some(uri)) = (
                read.as_ref(),
                crate::util::path_to_url(&index.files[file as usize]),
            ) else {
                continue;
            };
            let start = offset as usize;
            let end = start + content.get(start..).map_or(0, css_identifier_len);
            locations.push(Location {
                uri,
                range: tower_lsp_server::gen_lsp_types::Range {
                    start: crate::text_position::offset_to_position(content, start),
                    end: crate::text_position::offset_to_position(content, end),
                },
            });
        }
        locations
    }

    /// What hover says about the class `name`: the stylesheets that declare
    /// it.
    pub(crate) fn css_class_detail(&self, name: &str) -> String {
        let index = self.stylesheet_index();
        let files: Vec<String> = index
            .files_declaring(name)
            .into_iter()
            .filter_map(crate::util::path_to_url)
            .filter_map(|uri| self.workspace_relative_path(uri.as_str()))
            .map(|path| format!("`{path}`"))
            .collect();
        if files.is_empty() {
            return "Not declared in the project's stylesheets".to_string();
        }
        format!("Declared in {}", files.join(", "))
    }
}

/// The class names the selectors of a stylesheet declare, each with the
/// byte offset of the name (after its `.`).  `line_comments` reads `//` as
/// starting a comment, as SCSS and Less do.
pub(crate) fn class_selectors(content: &str, line_comments: bool) -> Vec<(String, usize)> {
    let bytes = content.as_bytes();
    let mut found = Vec::new();
    // The classes of the statement being read, kept once a `{` shows it is
    // a rule's selector, and dropped at a `;` or `}` that shows it is a
    // declaration.
    let mut pending: Vec<(String, usize)> = Vec::new();
    // Whether the statement so far is an at-rule's prelude.
    let mut at_rule = false;
    let mut statement_started = false;
    let mut brackets = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        match byte {
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = memchr::memmem::find(&bytes[i + 2..], b"*/")
                    .map_or(bytes.len(), |end| i + end + 4);
                continue;
            }
            b'/' if line_comments
                && bytes.get(i + 1) == Some(&b'/')
                && (i == 0
                    || bytes[i - 1].is_ascii_whitespace()
                    || b";{}".contains(&bytes[i - 1])) =>
            {
                i = memchr::memchr(b'\n', &bytes[i..]).map_or(bytes.len(), |end| i + end);
                continue;
            }
            b'"' | b'\'' => {
                statement_started = true;
                i = string_end(bytes, i);
                continue;
            }
            b'{' => {
                if !at_rule {
                    found.append(&mut pending);
                }
                pending.clear();
                (at_rule, statement_started, brackets) = (false, false, 0);
            }
            b';' | b'}' => {
                pending.clear();
                (at_rule, statement_started, brackets) = (false, false, 0);
            }
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'@' if !statement_started => {
                at_rule = true;
                statement_started = true;
            }
            b'.' if brackets == 0 && !at_rule => {
                statement_started = true;
                let start = i + 1;
                let len = css_identifier_len(&content[start..]);
                if len > 0 {
                    let after = bytes.get(start + len).copied();
                    // `.btn-#{$name}` is built by interpolation, and
                    // `.btn-@{name}` by a Less variable.
                    let interpolated = matches!(after, Some(b'#' | b'@'))
                        && bytes.get(start + len + 1) == Some(&b'{');
                    if !interpolated {
                        pending.push((unescape_identifier(&content[start..start + len]), start));
                    }
                    i = start + len;
                    continue;
                }
            }
            _ if !byte.is_ascii_whitespace() => statement_started = true,
            _ => {}
        }
        i += 1;
    }
    found
}

/// The offset just past the string whose opening quote is at `start`.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' => return i,
            byte if byte == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// The length in bytes of the CSS identifier `text` starts with, escapes
/// included, or `0` when it does not start with one.
fn css_identifier_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut i = 0;
    // An identifier may start with one `-`, or two, but not with a digit.
    while i < 2 && bytes.get(i) == Some(&b'-') {
        i += 1;
    }
    match bytes.get(i) {
        Some(b'\\') => {}
        Some(&byte) if byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80 => {}
        _ if i == 2 => {}
        _ => return 0,
    }
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += escape_len(&bytes[i..]),
            byte if byte.is_ascii_alphanumeric()
                || byte == b'_'
                || byte == b'-'
                || byte >= 0x80 =>
            {
                i += 1;
            }
            _ => break,
        }
    }
    i
}

/// The length of the escape at the start of `bytes`: a backslash and the
/// character it escapes, or up to six hex digits and one whitespace after
/// them.
fn escape_len(bytes: &[u8]) -> usize {
    let hex = bytes
        .iter()
        .skip(1)
        .take(6)
        .take_while(|byte| byte.is_ascii_hexdigit())
        .count();
    if hex == 0 {
        // A backslash before a line break escapes nothing.
        return match bytes.get(1) {
            None | Some(b'\n' | b'\r') => 1,
            Some(&byte) => 1 + utf8_width(byte),
        };
    }
    let end = 1 + hex;
    end + usize::from(bytes.get(end).is_some_and(u8::is_ascii_whitespace))
}

/// The width of the UTF-8 character whose first byte is `byte`.
fn utf8_width(byte: u8) -> usize {
    match byte {
        0xF0.. => 4,
        0xE0.. => 3,
        0xC0.. => 2,
        _ => 1,
    }
}

/// The name an identifier spells, with its escapes resolved:
/// `md\:flex` is `md:flex`.
fn unescape_identifier(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut name = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            let width = utf8_width(bytes[i]);
            name.push_str(&text[i..i + width]);
            i += width;
            continue;
        }
        let len = escape_len(&bytes[i..]);
        let escaped = text[i + 1..i + len].trim_end();
        match u32::from_str_radix(escaped, 16) {
            Ok(code) if !escaped.is_empty() && escaped.bytes().all(|b| b.is_ascii_hexdigit()) => {
                name.push(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER));
            }
            _ => name.push_str(escaped),
        }
        i += len;
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(css: &str, line_comments: bool) -> Vec<String> {
        class_selectors(css, line_comments)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    #[test]
    fn reads_the_classes_of_selectors() {
        let css = ".header, nav.menu > .item:not(.active) { color: red; }\n.a.b{}";
        assert_eq!(
            names(css, false),
            ["header", "menu", "item", "active", "a", "b"]
        );
        let (name, offset) = &class_selectors(css, false)[0];
        assert_eq!(&css[*offset..*offset + name.len()], "header");
    }

    #[test]
    fn skips_declarations_comments_strings_and_at_rule_preludes() {
        let css = "/* .old { } */\n.logo { background: url(img/logo.png); margin: .5em; }\n\
                   @media (min-width: 1.5em) { .wide { width: 100%; } }\n\
                   @keyframes spin { 12.5% { opacity: 0; } }\n\
                   a[href$=\".pdf\"] { content: \".x\"; }\n@import 'theme.css';";
        assert_eq!(names(css, false), ["logo", "wide"]);
    }

    #[test]
    fn reads_nested_rules_and_escapes() {
        let css = ".card {\n  // .commented { }\n  &.active { color: red; }\n  &-title { }\n  .btn-#{$x} { }\n}\n.md\\:flex { }\n.\\31 0 { }";
        assert_eq!(names(css, true), ["card", "active", "md:flex", "10"]);
    }

    #[test]
    fn a_double_slash_is_no_comment_in_plain_css() {
        assert_eq!(names("a { b: url(//x.test/y.png); }\n.c { }", false), ["c"]);
    }
}
