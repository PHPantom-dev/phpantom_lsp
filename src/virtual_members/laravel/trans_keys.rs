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
    let root = backend.workspace.workspace_root.read().clone();

    if let Some((namespace, rest)) = key.split_once("::") {
        let file_stem = rest.split('.').next().unwrap_or(rest);
        let prefix = format!("{namespace}::{file_stem}");
        let resources = backend.laravel_provider_resources.read();
        if !resources
            .trans_dirs
            .iter()
            .any(|res| res.namespace == namespace)
        {
            return results;
        }
        // A published override replaces the package's line, so it comes
        // first and is the one hover quotes.  It only has to declare the
        // keys it changes, so a file that lacks the key is no definition.
        if let Some(root) = &root {
            for dir in published_trans_dirs(root, namespace) {
                push_group_definitions(&dir, file_stem, &prefix, key, false, &mut results);
            }
        }
        for res in &resources.trans_dirs {
            if res.namespace == namespace {
                push_group_definitions(&res.path, file_stem, &prefix, key, true, &mut results);
            }
        }
        return results;
    }

    let snapshot = backend.user_file_symbol_maps();

    let file_stem = key.split('.').next().unwrap_or(key);
    let root_uri = root.as_deref().map(crate::util::path_to_uri);

    for (file_uri, _) in &snapshot {
        if root_uri
            .as_deref()
            .and_then(|root_uri| app_lang_group(root_uri, file_uri))
            != Some(file_stem)
        {
            continue;
        }
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

    if let Some(root) = root {
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

/// Push a location for `key` in each `<locale>/<file_stem>.php` under `dir`,
/// a directory of locale subdirectories.  A file that exists but does not
/// declare the key is reached at its top when `fallback_to_top` is set.
fn push_group_definitions(
    dir: &std::path::Path,
    file_stem: &str,
    prefix: &str,
    key: &str,
    fallback_to_top: bool,
    results: &mut Vec<Location>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
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
        let declarations = collect_trans_declarations(&content, prefix);
        if let Some(decl) = declarations.into_iter().find(|d| d.key == key) {
            let pos = crate::text_position::offset_to_position(&content, decl.start);
            results.push(crate::definition::point_location(uri, pos));
        } else if fallback_to_top {
            results.push(crate::definition::point_location(uri, Position::new(0, 0)));
        }
    }
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
    for sub in APP_LANG_DIRS {
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
