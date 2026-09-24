use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::cst::{Expression, Literal};
use tower_lsp::lsp_types::Location;

use crate::Backend;
use crate::php_type::PhpType;

impl Backend {
    /// The type a translation key resolves to, so that
    /// `__('messages.welcome')` / `trans(...)` / `Lang::get(...)` calls
    /// don't carry the full `string|array|null` union of the framework's
    /// declared return type into an argument a literal key settles.
    ///
    /// A leaf entry is the line itself; a group hands back the nested array
    /// of lines beneath it.  A key the indexed translations do not cover
    /// falls back to [`unresolved_trans_type`].
    pub(crate) fn resolve_trans_type(&self, key: &str) -> Option<PhpType> {
        match self.cached_translations().entries.get(key) {
            Some(entries) if entries.iter().any(|entry| entry.is_group) => Some(trans_group_type()),
            Some(_) => Some(PhpType::string()),
            None => Some(unresolved_trans_type()),
        }
    }
}

/// The type a translation group resolves to: the lines nested beneath it,
/// keyed by their own names.  The values are a mix of lines and further
/// groups, which is as far as a key alone settles the shape.
fn trans_group_type() -> PhpType {
    PhpType::generic_array(PhpType::string(), PhpType::mixed())
}

/// The type a translation call hands back when its key cannot be read: one
/// built at runtime, or one naming lines that are not in the workspace.
///
/// `null` is not among the branches.  `__()` and `trans()` return it only
/// for the keyless form, and every call that names a key at all gets a
/// string back even when the translation is missing (Laravel echoes the key
/// itself).  Which of the two remaining branches applies depends on a key
/// PHPantom cannot see, so the union is benevolent: a call site that passes
/// it on is accepted rather than reported against every branch.
pub(crate) fn unresolved_trans_type() -> PhpType {
    PhpType::benevolent(PhpType::union(vec![PhpType::string(), trans_group_type()]))
}

/// Resolve `__('file.key')` / `trans('file.key')` / `Lang::get('file.key')` to the
/// matching keys inside all matching `lang/{locale}/file.php` translation files,
/// or inside `lang/{locale}.json` JSON translation files.
///
/// For PHP files the key format is `file_stem.nested.key` (first segment = file,
/// rest = array path).  For JSON files the key is looked up directly as a
/// top-level object key (Laravel's JSON translations are flat).
///
/// Whole PHP groups resolve to the start of their file.
pub(crate) fn resolve_trans_definitions(backend: &Backend, key: &str) -> Vec<Location> {
    backend.translation_definitions(key)
}

/// The application's translation directories, relative to the project root.
/// `lang_path()` is one or the other depending on whether `resources/lang`
/// exists; both are read so a project part-way through the move resolves.
const APP_LANG_DIRS: [&str; 2] = ["lang", "resources/lang"];

/// The group an application translation file holds, or `None` when `uri` is
/// not one.
///
/// `FileLoader` reads `<lang>/<locale>/<group>.php`, and a group may name a
/// subdirectory: `lang/en/admin/users.php` is the `admin/users` group.
/// `<lang>/vendor/` holds published package overrides, which are only read
/// for their namespace, and a `lang/` directory anywhere else in the project
/// (a package's own) is not the application's.
pub(crate) fn app_lang_group<'a>(root_uri: &str, uri: &'a str) -> Option<&'a str> {
    let rel = uri
        .strip_prefix(root_uri.trim_end_matches('/'))?
        .strip_prefix('/')?;
    let rest = APP_LANG_DIRS
        .iter()
        .find_map(|dir| rel.strip_prefix(dir)?.strip_prefix('/'))?;
    let (locale, file) = rest.split_once('/')?;
    if locale == "vendor" {
        return None;
    }
    file.strip_suffix(".php").filter(|group| !group.is_empty())
}

/// The directories an application publishes a package's translations into,
/// `<lang>/vendor/<namespace>`, which `FileLoader::loadNamespaceOverrides()`
/// lays over the package's own lines.
pub(crate) fn published_trans_dirs(
    root: &std::path::Path,
    namespace: &str,
) -> impl Iterator<Item = std::path::PathBuf> {
    APP_LANG_DIRS
        .iter()
        .map(move |dir| root.join(dir).join("vendor").join(namespace))
}

// ─── Declaration extractor (mirrors config_keys logic) ───────────────────────

#[derive(Debug)]
pub(crate) struct TransKeyMatch {
    pub key: String,
    pub start: usize,
    /// Byte offset immediately after the key's source text, before its quote.
    pub end: usize,
    /// Whether the key's value is itself a nested array (a translation
    /// group) rather than a scalar string entry.
    pub is_group: bool,
    /// The line itself, for a scalar entry written as a string literal.
    pub value: Option<String>,
}

pub(crate) fn collect_trans_declarations(content: &str, file_stem: &str) -> Vec<TransKeyMatch> {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let mut out = Vec::new();
    for expr in super::array_file::returned_exprs(program) {
        super::array_file::for_each_entry(expr, content, &mut |path, start, end, value| {
            out.push(TransKeyMatch {
                key: super::array_file::dotted_key(file_stem, path),
                start,
                end,
                // A group is recognized exactly when there is more beneath
                // it to flatten.
                is_group: super::array_file::is_array_expr(value),
                value: match value {
                    Expression::Literal(Literal::String(string)) => string
                        .value
                        .and_then(crate::atom::literal_bytes_to_str)
                        .map(str::to_string),
                    _ => None,
                },
            });
        });
    }
    out
}

#[cfg(test)]
#[path = "trans_keys_tests.rs"]
mod tests;
