//! Yii view names: the templates a renderer's `render()` family and a
//! view's own `render()` load.
//!
//! A [`YiiView`](StringKeyKind::YiiView) key is the path the view names, as
//! `yiisoft/view` builds it: the renderer's view path and the name
//! (`@views/blog/index`), the template's own directory and a name rendered
//! from inside it, or the path an argument spells outright
//! (`/app/src/Web/HomePage/template`).  The alias is left in, since the
//! aliases are configuration that can change without the file that names
//! the view changing, and replaced only when the key is resolved.  The
//! `.php` the renderer adds to a name without an extension is left off.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Location, Position};

use crate::Backend;
use crate::symbol_map::{ReceiverClass, StringKeyKind, SymbolKind, SymbolMap};

/// The extensions a view without one is looked for with, in order:
/// `yiisoft/view`'s default, and the one `yiisoft/view-twig` adds.
const FALLBACK_EXTENSIONS: [&str; 2] = ["php", "twig"];

/// Whether `method` renders the view its first argument names, on a view
/// renderer or on a view.
pub(crate) fn is_view_render_method(method: &str) -> bool {
    [
        "render",
        "renderPartial",
        "renderAsString",
        "renderPartialAsString",
    ]
    .iter()
    .any(|name| method.eq_ignore_ascii_case(name))
}

/// The directory a renderer's `withController()` adds to its view path
/// for the controller class `fqn`, as `ViewRenderer` derives it: the
/// namespace after a `Controller\` or `Controllers\` segment and the class
/// name, without its `Controller` suffix and in kebab case
/// (`App\Controller\FooBar\BazController` is `foo-bar/baz`).
pub(crate) fn controller_name(fqn: &str) -> Option<String> {
    let fqn = fqn.trim_start_matches('\\');
    let lower = fqn.to_ascii_lowercase();
    let stem_len = lower.strip_suffix("controller")?.len();
    let after_segment = ["controller\\", "controllers\\"]
        .iter()
        .find_map(|segment| {
            let start = lower.find(segment)? + segment.len();
            (start < stem_len).then_some(start)
        });
    let start = after_segment.unwrap_or_else(|| lower[..stem_len].rfind('\\').map_or(0, |i| i + 1));
    let name = &fqn[start..stem_len];
    (!name.is_empty()).then(|| pascal_case_to_id(&name.replace('\\', "/")))
}

/// Yii's `Inflector::pascalCaseToId()`: a `-` before each capital that
/// follows a lowercase letter, underscores as `-`, all lowercased.
fn pascal_case_to_id(name: &str) -> String {
    let mut id = String::with_capacity(name.len() + 4);
    let mut previous: Option<char> = None;
    for c in name.chars() {
        if c.is_uppercase() && previous.is_some_and(|p| p.is_alphabetic() && !p.is_uppercase()) {
            id.push('-');
        }
        id.push(if c == '_' { '-' } else { c });
        previous = Some(c);
    }
    id.trim_matches('-').to_lowercase()
}

/// Collapse the `.` and `..` segments of a view path.  An alias is the
/// root of the path it starts, so `..` never climbs out of it.
pub(crate) fn normalize_view_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.last().is_some_and(|last| !last.starts_with('@')) {
                    segments.pop();
                }
            }
            other => segments.push(other),
        }
    }
    let joined = segments.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

/// The key a view `name` passed to a render call names, as
/// `ViewTrait::resolveViewFilePath()` builds the path: an absolute path is
/// itself, one starting with `./` or `../` is relative to `current_dir`,
/// the directory of the template being rendered, and any other name is
/// under `base`, the renderer's view path.  `//name`, under the view's base
/// path, is not followed.
pub(crate) fn view_key(
    name: &str,
    base: Option<&str>,
    current_dir: Option<&str>,
) -> Option<String> {
    if name.is_empty() || name.starts_with("//") {
        return None;
    }
    let path = if name.starts_with('/') {
        name.to_string()
    } else if name.starts_with("./") || name.starts_with("../") {
        format!("{}/{name}", current_dir?)
    } else {
        format!("{}/{name}", base?)
    };
    Some(normalize_view_path(&path))
}

/// The completion candidates for a view whose key so far is `typed`,
/// relative to the directory `base` names: the views and the directories
/// in the directory `typed` has reached, spelled as `typed` spells it.
/// `exclude` is the file the completion is in, which is no view of its
/// own.
pub(crate) fn view_candidates(
    aliases: &super::aliases::Aliases,
    base: &str,
    typed: &str,
    exclude: Option<&Path>,
) -> Vec<String> {
    let typed_dir = typed.rfind('/').map_or("", |slash| &typed[..=slash]);
    let joined = if typed_dir.starts_with('/') {
        format!("{base}{typed_dir}")
    } else {
        format!("{base}/{typed_dir}")
    };
    let Some(dir) = aliases.resolve(&normalize_view_path(&joined)) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name.starts_with('.') || exclude.is_some_and(|exclude| exclude == path) {
            continue;
        }
        if path.is_dir() {
            if file_name != "vendor" && file_name != "node_modules" {
                names.push(format!("{typed_dir}{file_name}/"));
            }
        } else if let Some(stem) = file_name.strip_suffix(".php")
            && !stem.is_empty()
        {
            names.push(format!("{typed_dir}{stem}"));
        }
    }
    names.sort_unstable();
    names
}

impl Backend {
    /// The path a view key names with its alias replaced, before the
    /// renderer's fallback extension is added, or `None` when its alias is
    /// one the project does not declare.
    fn yii_view_path(&self, key: &str) -> Option<PathBuf> {
        self.yii_aliases()
            .resolve(key)
            .filter(|path| path.starts_with('/'))
            .map(|path| PathBuf::from(normalize_view_path(&path)))
    }

    /// The template file a view key names, when there is one: the path
    /// itself when it has an extension and exists, else the path with each
    /// fallback extension.
    pub(crate) fn yii_view_file(&self, key: &str) -> Option<PathBuf> {
        view_file(&self.yii_view_path(key)?)
    }

    /// Whether the template a view key names exists, or `None` when the key
    /// cannot be resolved.
    pub(crate) fn yii_view_exists(&self, key: &str) -> Option<bool> {
        let path = self.yii_view_path(key)?;
        Some(view_file(&path).is_some())
    }

    /// The file a view key names, or the `.php` file the renderer would
    /// look for when no file exists, so two spellings of one view compare
    /// equal whether it exists or not.
    fn yii_view_identity(&self, key: &str) -> Option<PathBuf> {
        let path = self.yii_view_path(key)?;
        Some(view_file(&path).unwrap_or_else(|| with_extension(&path, "php")))
    }

    /// The view a key names, as a diagnostic or hover spells it: relative
    /// to the workspace when it is under it.
    pub(crate) fn yii_view_display(&self, key: &str) -> String {
        let Some(path) = self.yii_view_path(key) else {
            return key.to_string();
        };
        let root = self.workspace.workspace_root.read().clone();
        root.and_then(|root| {
            path.strip_prefix(&root)
                .ok()
                .map(|relative| relative.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }

    /// Go-to-definition on a view key: the template it names.
    pub(crate) fn resolve_yii_view_definitions(&self, key: &str) -> Vec<Location> {
        self.yii_view_file(key)
            .and_then(|file| crate::util::path_to_url(&file))
            .map(|uri| crate::definition::point_location(uri, Position::new(0, 0)))
            .into_iter()
            .collect()
    }

    /// Find references on a view key: every render of the template it
    /// names, however each one spells it, and the template itself.
    pub(crate) fn find_yii_view_references(
        &self,
        key: &str,
        include_declaration: bool,
    ) -> Vec<Location> {
        let Some(target) = self.yii_view_identity(key) else {
            return Vec::new();
        };
        let snapshot = self.user_file_symbol_maps();
        let mut locations = self.yii_view_usages(&target, &snapshot);
        if include_declaration {
            locations.extend(self.resolve_yii_view_definitions(key));
        }
        locations
    }

    /// The renders of the template at `path`, for the lens on the template
    /// itself.  Reads what is indexed so far rather than waiting for the
    /// rest.
    pub(crate) fn yii_view_renders_of(&self, path: &Path) -> Vec<Location> {
        let snapshot = self.user_file_symbol_maps_nonblocking();
        self.yii_view_usages(path, &snapshot)
    }

    /// Every view key in `snapshot` that names the file `target`.
    fn yii_view_usages(
        &self,
        target: &Path,
        snapshot: &[(String, Arc<SymbolMap>)],
    ) -> Vec<Location> {
        let mut identities: HashMap<String, Option<PathBuf>> = HashMap::new();
        let mut names_target = |key: &str| {
            identities
                .entry(key.to_string())
                .or_insert_with(|| self.yii_view_identity(key))
                .as_deref()
                == Some(target)
        };
        let mut locations = Vec::new();
        for (uri, map) in snapshot {
            let mut starts: Vec<(u32, u32)> = map
                .spans
                .iter()
                .filter_map(|span| match &span.kind {
                    SymbolKind::StringKey {
                        kind: StringKeyKind::YiiView,
                        key,
                        ..
                    } if names_target(key) => Some((span.start, span.end)),
                    _ => None,
                })
                .collect();
            // A render whose receiver only a type settles is confirmed only
            // when its key names the template, so the type engine runs for
            // the handful of files that could contribute one.
            let has_candidate = map.typed_receiver_sites.iter().any(|site| {
                site.receiver == ReceiverClass::YiiViewRenderer && names_target(&site.key)
            });
            if has_candidate {
                for span in self.typed_receiver_spans_for(uri, map).iter() {
                    if let SymbolKind::StringKey {
                        kind: StringKeyKind::YiiView,
                        key,
                        ..
                    } = &span.kind
                        && names_target(key)
                    {
                        starts.push((span.start, span.end));
                    }
                }
            }
            if starts.is_empty() {
                continue;
            }
            let Some(parsed_uri) = crate::util::parse_url(uri) else {
                continue;
            };
            let Some(content) = self.reference_file_content_arc(uri) else {
                continue;
            };
            let lines = crate::text_position::LineIndex::new(&content);
            for (start, end) in starts {
                crate::references::push_location(
                    &mut locations,
                    &parsed_uri,
                    lines.position(start as usize),
                    lines.position(end as usize),
                );
            }
        }
        locations
    }
}

/// The file a view path names: itself when it has an extension and
/// exists, else the path with the first fallback extension that exists.
fn view_file(path: &Path) -> Option<PathBuf> {
    if path.extension().is_some() && path.is_file() {
        return Some(path.to_path_buf());
    }
    FALLBACK_EXTENSIONS
        .iter()
        .map(|extension| with_extension(path, extension))
        .find(|candidate| candidate.is_file())
}

/// `path` with `.extension` appended, keeping any dot its name already has.
fn with_extension(path: &Path, extension: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}
