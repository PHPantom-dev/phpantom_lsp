use mago_allocator::LocalArena;
use mago_database::file::FileId;
use tower_lsp::lsp_types::{Location, Position, Url};

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
        match self.cached_trans_key_shapes().get(key) {
            Some(false) => Some(PhpType::string()),
            Some(true) => Some(trans_group_type()),
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
/// Falls back to the top of the file when the exact key cannot be located.
pub(crate) fn resolve_trans_definitions(backend: &Backend, key: &str) -> Vec<Location> {
    let mut results = Vec::new();

    if let Some((namespace, rest)) = key.split_once("::") {
        let file_stem = rest.split('.').next().unwrap_or(rest);
        for res in &backend.laravel_provider_resources.read().trans_dirs {
            if res.namespace != namespace {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&res.path) else {
                continue;
            };
            for entry in entries.flatten() {
                let locale_dir = entry.path();
                if !locale_dir.is_dir() {
                    continue;
                }
                let candidate = locale_dir.join(format!("{file_stem}.php"));
                if !candidate.is_file() {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&candidate) else {
                    continue;
                };
                let Ok(uri) = Url::from_file_path(&candidate) else {
                    continue;
                };
                let prefix = format!("{namespace}::{file_stem}");
                let declarations = collect_trans_declarations(&content, &prefix);
                if let Some(decl) = declarations.into_iter().find(|d| d.key == key) {
                    let pos = crate::text_position::offset_to_position(&content, decl.start);
                    results.push(crate::definition::point_location(uri, pos));
                    continue;
                }
                results.push(crate::definition::point_location(uri, Position::new(0, 0)));
            }
        }
        return results;
    }

    let snapshot = backend.user_file_symbol_maps();

    let file_stem = key.split('.').next().unwrap_or(key);
    let target_suffix = format!("/{file_stem}.php");

    for (file_uri, _) in &snapshot {
        if !(file_uri.contains("/lang/") || file_uri.contains("/resources/lang/")) {
            continue;
        }

        if file_uri.ends_with(&target_suffix) {
            let Ok(uri) = Url::parse(file_uri) else {
                continue;
            };
            let Some(content) = backend.get_file_content(file_uri) else {
                continue;
            };

            let declarations = collect_trans_declarations(&content, file_stem);
            if let Some(decl) = declarations.into_iter().find(|d| d.key == key) {
                let pos = crate::text_position::offset_to_position(&content, decl.start);
                results.push(crate::definition::point_location(uri, pos));
                continue;
            }

            results.push(crate::definition::point_location(uri, Position::new(0, 0)));
        }
    }

    if let Some(root) = backend.workspace.workspace_root.read().clone() {
        for_each_json_lang_file(&root, |path, map| {
            if map.contains_key(key)
                && let Ok(uri) = Url::from_file_path(path)
            {
                results.push(crate::definition::point_location(uri, Position::new(0, 0)));
            }
        });
    }

    results
}

/// The line a translation key resolves to inside the file that declares it.
///
/// The file is the one [`resolve_trans_definitions`] settled on, so hover
/// quotes the string from the same locale it names, and a group (which has
/// no single line) resolves to `None`.
pub(crate) fn trans_line(backend: &Backend, key: &str, file_uri: &Url) -> Option<String> {
    let path = file_uri.path();
    if path.ends_with(".json") {
        let content = std::fs::read_to_string(file_uri.to_file_path().ok()?).ok()?;
        let map =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&content).ok()?;
        return map.get(key)?.as_str().map(str::to_string);
    }
    let content = backend.get_file_content(file_uri.as_str())?;
    collect_trans_declarations(&content, &trans_file_prefix(key))
        .into_iter()
        .find(|decl| decl.key == key)?
        .value
}

/// The prefix [`collect_trans_declarations`] flattens a file's keys under,
/// derived from the key being looked up: the first dotted segment, or
/// `namespace::file` for a package translation.
fn trans_file_prefix(key: &str) -> String {
    match key.split_once("::") {
        Some((namespace, rest)) => {
            format!("{namespace}::{}", rest.split('.').next().unwrap_or(rest))
        }
        None => key.split('.').next().unwrap_or(key).to_string(),
    }
}

// ─── Declaration extractor (mirrors config_keys logic) ───────────────────────

#[derive(Debug)]
pub(crate) struct TransKeyMatch {
    pub key: String,
    pub start: usize,
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
        super::array_file::for_each_entry(expr, content, &mut |path, start, _end, value| {
            out.push(TransKeyMatch {
                key: super::array_file::dotted_key(file_stem, path),
                start,
                // A group is recognized exactly when there is more beneath
                // it to flatten.
                is_group: super::array_file::is_array_expr(value),
                value: super::helpers::extract_string_literal(value, content)
                    .map(|(text, _, _)| text.to_string()),
            });
        });
    }
    out
}

/// Call `visit` with each `lang/*.json` and `resources/lang/*.json` file
/// under `root` and its top-level map.
///
/// Laravel's JSON translations are flat `{ "Some phrase": "Translated" }`
/// objects whose keys are used directly in `__('Some phrase')`.  They are
/// not PHP, so they never appear in the symbol maps and are read from disk.
pub(crate) fn for_each_json_lang_file(
    root: &std::path::Path,
    mut visit: impl FnMut(&std::path::Path, &serde_json::Map<String, serde_json::Value>),
) {
    for sub in ["lang", "resources/lang"] {
        let Ok(entries) = std::fs::read_dir(root.join(sub)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(content) = std::fs::read_to_string(&path)
                && let Ok(map) =
                    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&content)
            {
                visit(&path, &map);
            }
        }
    }
}

#[cfg(test)]
#[path = "trans_keys_tests.rs"]
mod tests;
