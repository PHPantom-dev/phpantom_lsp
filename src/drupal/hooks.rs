//! Drupal's hooks: the extensions that implement them, and what a PHP file
//! of a Drupal project can hold of them.
//!
//! A hook is documented by a `hook_<name>()` function in an `*.api.php`
//! file, which Drupal never loads, and implemented by a method a `#[Hook]`
//! attribute marks in an extension's `src/Hook`, or by a function named
//! `<extension>_<name>` in an extension's procedural files.  The
//! extensions come from their `*.info.yml`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Backend;

/// One module, install profile, or theme, read from its `*.info.yml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Extension {
    /// The directory holding the `.info.yml`.
    pub dir: PathBuf,
    /// The machine name, from the file name.
    pub name: String,
    /// Themes implement hooks the theme manager invokes; modules and
    /// install profiles the ones the module handler does.
    pub is_theme: bool,
}

/// What a PHP file of a Drupal project holds of hooks, which only its path
/// says: the extraction is told before it reads the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum DrupalFile {
    /// Anything else: its hook classes, if any, are read by namespace.
    #[default]
    Other,
    /// An `*.api.php` file, whose `hook_*()` functions document hooks.
    Api,
    /// A file of a test module, which no real site enables: it neither
    /// documents nor implements a hook.
    Test,
    /// An extension's procedural file, whose global functions named
    /// `<extension>_<hook>` implement hooks.
    Procedural(Procedural),
}

/// The extension a procedural file belongs to, and the names its
/// functions can start with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Procedural {
    /// The file's own extension, tried first.
    pub extension: String,
    /// Every extension of the file's group (modules and profiles, or
    /// themes), longest name first, tried next.
    pub group: Arc<[String]>,
    /// The file is a `.post_update.php`, whose functions implement
    /// `hook_post_update_NAME` and nothing Drupal collects.
    pub post_update: bool,
    /// The extension is a theme.
    pub is_theme: bool,
}

/// Hooks Drupal's hook collector leaves out of procedural code: they are
/// called at install time by name, and only link to their documentation.
pub(crate) const NOT_COLLECTED: [&str; 7] = [
    "install",
    "install_tasks",
    "install_tasks_alter",
    "schema",
    "uninstall",
    "update_last_removed",
    "update_dependencies",
];

/// The extensions of the workspace, by directory, and the names of each
/// group, longest first.
#[derive(Default)]
pub(crate) struct ExtensionIndex {
    by_dir: HashMap<PathBuf, (String, bool)>,
    modules: Arc<[String]>,
    themes: Arc<[String]>,
}

impl ExtensionIndex {
    fn add(&mut self, extensions: Vec<Extension>) {
        for extension in extensions {
            self.by_dir
                .insert(extension.dir, (extension.name, extension.is_theme));
        }
        let group = |is_theme: bool| -> Arc<[String]> {
            let mut names: Vec<String> = self
                .by_dir
                .values()
                .filter(|(_, theme)| *theme == is_theme)
                .map(|(name, _)| name.clone())
                .collect();
            names.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
            names.dedup();
            names.into()
        };
        self.modules = group(false);
        self.themes = group(true);
    }

    /// Whether the extension `name` is a theme, when the walk found it.
    fn is_theme(&self, name: &str) -> Option<bool> {
        self.by_dir
            .values()
            .find(|(known, _)| known == name)
            .map(|(_, is_theme)| *is_theme)
    }

    /// The extension a file belongs to: the one of its nearest ancestor
    /// directory holding an `.info.yml`, with that directory.
    fn owner<'a>(&'a self, path: &'a Path) -> Option<(&'a Path, &'a str, bool)> {
        path.ancestors().skip(1).find_map(|dir| {
            self.by_dir
                .get(dir)
                .map(|(name, is_theme)| (dir, name.as_str(), *is_theme))
        })
    }
}

/// Whether a file at `relative` to its extension's directory is one the
/// hook collector reads functions from.
fn is_procedural_file(relative: &Path, is_theme: bool) -> bool {
    let skipped: &[&str] = if is_theme {
        &["tests", "js", "css", "templates"]
    } else {
        &["tests", "js", "css"]
    };
    let in_skipped = relative.parent().is_some_and(|dir| {
        dir.components()
            .any(|part| skipped.iter().any(|skip| part.as_os_str() == *skip))
    });
    let Some(name) = relative.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let suffixes: &[&str] = if is_theme {
        &[".inc", ".theme"]
    } else {
        &[".module", ".inc", ".install", ".profile"]
    };
    let eligible = suffixes.iter().any(|suffix| name.ends_with(suffix))
        || (is_theme && name == "theme-settings.php");
    eligible && !in_skipped
}

/// Whether the file name at the end of `path` is an extension's
/// `*.info.yml`.
pub(crate) fn is_info_file_name(name: &str) -> bool {
    name.ends_with(".info.yml")
}

/// The extension an `*.info.yml` at `path` declares, from its `type:`
/// line; `None` for anything but a module, profile, or theme.
pub(crate) fn read_extension(path: &Path) -> Option<Extension> {
    let name = path
        .file_name()?
        .to_str()?
        .strip_suffix(".info.yml")?
        .to_string();
    let content = std::fs::read_to_string(path).ok()?;
    let kind = content.lines().find_map(|line| {
        let value = line.strip_prefix("type:")?.trim();
        let value = crate::framework::strip_yaml_quotes(value).0;
        Some(value.to_string())
    })?;
    let is_theme = match kind.as_str() {
        "module" | "profile" => false,
        "theme" => true,
        _ => return None,
    };
    Some(Extension {
        dir: path.parent()?.to_path_buf(),
        name,
        is_theme,
    })
}

/// The extension a hook class of namespace `Drupal\<extension>\Hook\…`
/// belongs to, which is where Drupal discovers hook classes.
pub(crate) fn hook_class_extension(fqn: &str) -> Option<&str> {
    let mut segments = fqn.trim_start_matches('\\').split('\\');
    (segments.next() == Some("Drupal")).then_some(())?;
    let extension = segments.next()?;
    (segments.next() == Some("Hook") && segments.next().is_some()).then_some(extension)
}

impl Backend {
    /// Remember the extensions the Drupal walk found.
    pub(crate) fn note_drupal_extensions(&self, extensions: Vec<Extension>) {
        self.drupal.extensions.write().add(extensions);
    }

    /// What the PHP file at `uri` holds of hooks.
    pub(crate) fn drupal_file(&self, uri: &str) -> DrupalFile {
        if !self.is_drupal_project() {
            return DrupalFile::Other;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return DrupalFile::Other;
        };
        if self.is_drupal_test_path(&path) {
            return DrupalFile::Test;
        }
        let is_api = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".api.php"));
        if is_api {
            return DrupalFile::Api;
        }
        self.procedural_file(&path)
            .map_or(DrupalFile::Other, DrupalFile::Procedural)
    }

    /// The extension and group of a procedural file, by the extension that
    /// owns it, or before the walk has read any `.info.yml` by the file's
    /// own name (`acme.module`).
    fn procedural_file(&self, path: &Path) -> Option<Procedural> {
        let name = path.file_name()?.to_str()?;
        let extensions = self.drupal.extensions.read();
        let (extension, is_theme, relative) = match extensions.owner(path) {
            Some((dir, extension, is_theme)) => (
                extension.to_string(),
                is_theme,
                path.strip_prefix(dir).ok()?,
            ),
            None => {
                let (stem, suffix) = name.rsplit_once('.')?;
                let is_theme = match suffix {
                    "module" | "install" | "profile" => false,
                    "theme" => true,
                    _ => return None,
                };
                (stem.to_string(), is_theme, Path::new(name))
            }
        };
        let post_update = !is_theme && name.ends_with(".post_update.php");
        if !post_update && !is_procedural_file(relative, is_theme) {
            return None;
        }
        let group = if is_theme {
            Arc::clone(&extensions.themes)
        } else {
            Arc::clone(&extensions.modules)
        };
        Some(Procedural {
            extension,
            group,
            post_update,
            is_theme,
        })
    }
}

/// Whether a documented hook name has placeholders, written in uppercase
/// (`form_FORM_ID_alter`): it stands for every hook that fills them in.
pub(crate) fn is_hook_pattern(name: &str) -> bool {
    name.bytes().any(|byte| byte.is_ascii_uppercase())
}

/// One piece of a hook name pattern.
enum Piece<'a> {
    Text(&'a str),
    /// An uppercase run, such as `FORM_ID` or `ENTITY_TYPE`.
    Placeholder(&'a str),
}

/// A pattern split into text and placeholders: an uppercase run starts a
/// placeholder at the start of the pattern or after an underscore, and goes
/// on through underscores followed by more uppercase.
fn pieces(pattern: &str) -> Vec<Piece<'_>> {
    let bytes = pattern.as_bytes();
    let mut pieces = Vec::new();
    let (mut text_start, mut i) = (0, 0);
    while i < bytes.len() {
        let starts = bytes[i].is_ascii_uppercase() && (i == 0 || bytes[i - 1] == b'_');
        if !starts {
            i += 1;
            continue;
        }
        let mut end = i;
        loop {
            while end < bytes.len()
                && (bytes[end].is_ascii_uppercase() || bytes[end].is_ascii_digit())
            {
                end += 1;
            }
            if end + 1 < bytes.len() && bytes[end] == b'_' && bytes[end + 1].is_ascii_uppercase() {
                end += 1;
            } else {
                break;
            }
        }
        if text_start < i {
            pieces.push(Piece::Text(&pattern[text_start..i]));
        }
        pieces.push(Piece::Placeholder(&pattern[i..end]));
        text_start = end;
        i = end;
    }
    if text_start < bytes.len() {
        pieces.push(Piece::Text(&pattern[text_start..]));
    }
    pieces
}

/// Whether `hook` is one the documented `pattern` stands for: `ENTITY_TYPE`
/// is an entity type ID `is_entity_type` accepts, `N` digits, and any other
/// placeholder a run of lowercase letters, digits, and underscores.
pub(crate) fn matches_hook_pattern(
    pattern: &str,
    hook: &str,
    is_entity_type: &dyn Fn(&str) -> bool,
) -> bool {
    fn rest(pieces: &[Piece<'_>], hook: &str, is_entity_type: &dyn Fn(&str) -> bool) -> bool {
        let Some((first, others)) = pieces.split_first() else {
            return hook.is_empty();
        };
        match first {
            Piece::Text(text) => hook
                .strip_prefix(text)
                .is_some_and(|after| rest(others, after, is_entity_type)),
            Piece::Placeholder(name) => (1..=hook.len())
                .filter(|length| hook.is_char_boundary(*length))
                .any(|length| {
                    let (value, after) = hook.split_at(length);
                    let shaped = match *name {
                        "N" => value.bytes().all(|byte| byte.is_ascii_digit()),
                        _ => value.bytes().all(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                        }),
                    };
                    // The rest of the name is cheaper to check than whether
                    // a run is a declared entity type.
                    shaped
                        && rest(others, after, is_entity_type)
                        && (*name != "ENTITY_TYPE" || is_entity_type(value))
                }),
        }
    }
    rest(&pieces(pattern), hook, is_entity_type)
}

/// How much of `pattern` is written out: its length without its
/// placeholders.  Of the patterns a hook matches, the ones that write out
/// the most are the ones that document it (`theme_suggestions_HOOK_alter`
/// over `theme_suggestions_HOOK` for `theme_suggestions_node_alter`).
fn written_out(pattern: &str) -> usize {
    pieces(pattern)
        .iter()
        .map(|piece| match piece {
            Piece::Text(text) => text.len(),
            Piece::Placeholder(_) => 0,
        })
        .sum()
}

/// What the documentation of the workspace's hooks says, read once and
/// kept until a file that declares hooks or entity types changes.
#[derive(Default)]
pub(crate) struct HookIndex {
    /// Every documented hook, patterns included, sorted.
    documented: Vec<String>,
    /// The documented hooks with placeholders, sorted.
    patterns: Vec<String>,
    /// Every declared entity type ID.
    entity_types: std::collections::HashSet<String>,
}

impl HookIndex {
    /// The documented patterns `hook` matches that write out the most.
    fn best_patterns(&self, hook: &str) -> Vec<&str> {
        let is_entity_type = |id: &str| self.entity_types.contains(id);
        let matching: Vec<&str> = self
            .patterns
            .iter()
            .filter(|pattern| matches_hook_pattern(pattern, hook, &is_entity_type))
            .map(String::as_str)
            .collect();
        let best = matching.iter().map(|pattern| written_out(pattern)).max();
        matching
            .into_iter()
            .filter(|pattern| Some(written_out(pattern)) == best)
            .collect()
    }

    /// Whether `hook` is documented, by name or by a pattern.
    fn documents(&self, hook: &str) -> bool {
        self.documented
            .binary_search_by(|known| known.as_str().cmp(hook))
            .is_ok()
            || !self.best_patterns(hook).is_empty()
    }
}

/// Where a hook is named, and as what.
#[derive(Debug, Clone)]
struct HookOccurrence {
    uri: String,
    start: u32,
    end: u32,
    role: Occurrence,
}

#[derive(Debug, Clone, PartialEq)]
enum Occurrence {
    Documentation,
    /// An invocation of the module handler's (or the theme manager's)
    /// hooks, limited to one extension when it names one.
    Invocation {
        themes: bool,
        extension: Option<crate::atom::Atom>,
    },
    Implementation {
        extension: crate::atom::Atom,
        is_theme: bool,
        collected: bool,
    },
}

impl Occurrence {
    /// Whether an invocation reaches an implementation: of its group, of
    /// the extension it names if any, and collected by Drupal.
    fn reaches(invocation: &Self, implementation: &Self) -> bool {
        match (invocation, implementation) {
            (
                Self::Invocation { themes, extension },
                Self::Implementation {
                    extension: implemented_for,
                    is_theme,
                    collected,
                },
            ) => {
                *collected
                    && themes == is_theme
                    && extension.is_none_or(|extension| extension == *implemented_for)
            }
            _ => false,
        }
    }
}

impl Backend {
    /// Every place the hook `key` is named, with what it is there.
    fn hook_occurrences(&self, key: &str) -> Vec<HookOccurrence> {
        use crate::symbol_map::{HookRole, ReceiverClass, StringKeyKind, SymbolKind};
        let reference_key =
            crate::reference_index::string_key_reference_key(StringKeyKind::DrupalHook, key);
        let snapshot = self.user_file_symbol_maps_for_reference_keys(&[reference_key]);
        let mut found = Vec::new();
        for (uri, map) in &snapshot {
            let typed = if map
                .typed_receiver_sites
                .iter()
                .any(|site| site.may_name(StringKeyKind::DrupalHook, key))
            {
                self.typed_receiver_spans_for(uri, map)
            } else {
                std::sync::Arc::default()
            };
            for span in map.spans.iter().chain(typed.iter()) {
                let SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalHook,
                    key: named,
                    is_write,
                    ..
                } = &span.kind
                else {
                    continue;
                };
                if named != key {
                    continue;
                }
                let site = map
                    .drupal_hooks
                    .binary_search_by_key(&span.start, |site| site.start)
                    .ok()
                    .map(|at| &map.drupal_hooks[at].role);
                let role = match site {
                    None if *is_write => Occurrence::Documentation,
                    None => continue,
                    Some(HookRole::Implementation {
                        extension,
                        is_theme,
                        collected,
                    }) => Occurrence::Implementation {
                        extension: *extension,
                        is_theme: is_theme
                            .or_else(|| self.drupal.extensions.read().is_theme(extension))
                            .unwrap_or(false),
                        collected: *collected,
                    },
                    Some(HookRole::Invocation { extension }) => {
                        // A method both have is asked about again as the
                        // theme manager's.
                        let themes = map
                            .typed_receiver_sites
                            .iter()
                            .find(|site| site.start == span.start)
                            .is_some_and(|site| match site.receiver {
                                ReceiverClass::ThemeHooks => true,
                                ReceiverClass::Hooks => {
                                    let mut theme = site.clone();
                                    theme.receiver = ReceiverClass::ThemeHooks;
                                    self.confirms_typed_receiver_site(uri, &theme)
                                }
                                _ => false,
                            });
                        Occurrence::Invocation {
                            themes,
                            extension: *extension,
                        }
                    }
                };
                found.push(HookOccurrence {
                    uri: uri.clone(),
                    start: span.start,
                    end: span.end,
                    role,
                });
            }
        }
        found.sort_by(|a, b| (&a.uri, a.start).cmp(&(&b.uri, b.start)));
        found
    }

    /// Whether the type engine confirms `site` of the file at `uri`, read
    /// in isolation from whatever pass is running, as the declarations of
    /// another file are.
    fn confirms_typed_receiver_site(
        &self,
        uri: &str,
        site: &crate::symbol_map::TypedReceiverSite,
    ) -> bool {
        let _isolated = (
            crate::type_engine::variable::forward_walk::suspend_diagnostic_scope(),
            crate::type_engine::resolver::with_isolated_chain_cache(),
        );
        self.get_file_content_arc(uri).is_some_and(|content| {
            !self
                .confirm_typed_receiver_sites(uri, &content, std::slice::from_ref(site))
                .is_empty()
        })
    }

    /// Where go-to-definition on the hook `key`, named at `start` in `uri`,
    /// leads: from an invocation, the implementations it reaches; from an
    /// implementation, the invocations that reach it; from the
    /// documentation, the implementations, those of every hook a documented
    /// pattern stands for included; then the documentation, the patterns
    /// the hook matches when none documents it by name.
    pub(crate) fn drupal_hook_definitions(
        &self,
        uri: &str,
        start: u32,
        key: &str,
    ) -> Vec<tower_lsp_server::gen_lsp_types::Location> {
        let occurrences = self.hook_occurrences(key);
        let Some(here) = occurrences
            .iter()
            .find(|occurrence| occurrence.uri == uri && occurrence.start == start)
            .map(|occurrence| occurrence.role.clone())
        else {
            return Vec::new();
        };
        if here == Occurrence::Documentation {
            let implemented: Vec<HookOccurrence> = if is_hook_pattern(key) {
                self.hooks_matching(key)
                    .iter()
                    .flat_map(|hook| self.hook_occurrences(hook))
                    .collect()
            } else {
                occurrences
            };
            let targets: Vec<&HookOccurrence> = implemented
                .iter()
                .filter(|other| matches!(other.role, Occurrence::Implementation { .. }))
                .collect();
            return self.hook_locations(&targets);
        }
        let docs = self.hook_documentation_occurrences(key, &occurrences);
        // Then the calls in core that fire it under a computed name.
        let computed: Vec<HookOccurrence> = match &here {
            Occurrence::Implementation {
                is_theme,
                collected: true,
                ..
            } => self
                .computed_hook_call_sites(key, *is_theme)
                .into_iter()
                .map(|(uri, start, end)| HookOccurrence {
                    uri,
                    start,
                    end,
                    role: Occurrence::Invocation {
                        themes: *is_theme,
                        extension: None,
                    },
                })
                .collect(),
            _ => Vec::new(),
        };
        let targets: Vec<&HookOccurrence> = occurrences
            .iter()
            .filter(|other| match (&here, &other.role) {
                (Occurrence::Invocation { .. }, Occurrence::Implementation { .. }) => {
                    Occurrence::reaches(&here, &other.role)
                }
                (Occurrence::Implementation { .. }, Occurrence::Invocation { .. }) => {
                    Occurrence::reaches(&other.role, &here)
                }
                _ => false,
            })
            .chain(computed.iter())
            .chain(docs.iter())
            .collect();
        self.hook_locations(&targets)
    }

    /// The documentation of the hook `key`: its own, or that of every
    /// documented pattern it matches.
    pub(crate) fn drupal_hook_documentation(
        &self,
        key: &str,
    ) -> Vec<tower_lsp_server::gen_lsp_types::Location> {
        let occurrences = self.hook_occurrences(key);
        let docs = self.hook_documentation_occurrences(key, &occurrences);
        self.hook_locations(&docs.iter().collect::<Vec<_>>())
    }

    /// Every place the hook `key` is named, and for a documented pattern
    /// every place a hook it stands for is, with the documentation of the
    /// patterns a hook matches when `include_declaration` asks for it.
    pub(crate) fn drupal_hook_references(
        &self,
        key: &str,
        include_declaration: bool,
    ) -> Vec<tower_lsp_server::gen_lsp_types::Location> {
        let mut found = self.hook_occurrences(key);
        if is_hook_pattern(key) {
            for hook in self.hooks_matching(key) {
                found.extend(self.hook_occurrences(&hook));
            }
        } else if include_declaration && !found.iter().any(|o| o.role == Occurrence::Documentation)
        {
            found.extend(self.hook_documentation_occurrences(key, &[]));
        }
        if !include_declaration {
            found.retain(|occurrence| occurrence.role != Occurrence::Documentation);
        }
        found.sort_by(|a, b| (&a.uri, a.start).cmp(&(&b.uri, b.start)));
        found.dedup_by(|a, b| a.uri == b.uri && a.start == b.start);
        self.hook_locations(&found.iter().collect::<Vec<_>>())
    }

    /// The documentation among `occurrences` of `key`, else that of every
    /// documented pattern `key` matches.
    fn hook_documentation_occurrences(
        &self,
        key: &str,
        occurrences: &[HookOccurrence],
    ) -> Vec<HookOccurrence> {
        let own: Vec<HookOccurrence> = occurrences
            .iter()
            .filter(|occurrence| occurrence.role == Occurrence::Documentation)
            .cloned()
            .collect();
        if !own.is_empty() || is_hook_pattern(key) {
            return own;
        }
        self.hook_index()
            .best_patterns(key)
            .into_iter()
            .flat_map(|pattern| self.hook_occurrences(pattern))
            .filter(|occurrence| occurrence.role == Occurrence::Documentation)
            .collect()
    }

    /// Every hook an `*.api.php` file documents, patterns included, sorted.
    pub(crate) fn drupal_documented_hooks(&self) -> Vec<String> {
        self.hook_index().documented.clone()
    }

    /// What the documentation of the workspace's hooks says, read once
    /// until a declaring file changes (see
    /// [`Backend::forget_drupal_hook_index`]).
    fn hook_index(&self) -> std::sync::Arc<HookIndex> {
        use crate::symbol_map::{StringKeyKind, SymbolKind};
        if let Some(index) = self.drupal.hook_index.read().as_ref() {
            return std::sync::Arc::clone(index);
        }
        let mut index = HookIndex::default();
        // The documentation is a plain span; an invocation, the other kind
        // of declaration, is a call the type engine confirms, and is left
        // out.
        self.visit_declaring_symbol_maps(|_, map| {
            for span in &map.spans {
                if let SymbolKind::StringKey {
                    kind,
                    key,
                    is_write: true,
                    ..
                } = &span.kind
                {
                    match kind {
                        StringKeyKind::DrupalHook => index.documented.push(key.clone()),
                        StringKeyKind::DrupalEntityType => {
                            index.entity_types.insert(key.clone());
                        }
                        _ => {}
                    }
                }
            }
        });
        index.documented.sort_unstable();
        index.documented.dedup();
        index.patterns = index
            .documented
            .iter()
            .filter(|hook| is_hook_pattern(hook))
            .cloned()
            .collect();
        let index = std::sync::Arc::new(index);
        *self.drupal.hook_index.write() = Some(std::sync::Arc::clone(&index));
        index
    }

    /// Forget what [`Backend::hook_index`] read, once a file that declares
    /// hooks or entity types may have changed.
    pub(crate) fn forget_drupal_hook_index(&self) {
        if self.drupal.hook_index.read().is_some() {
            *self.drupal.hook_index.write() = None;
        }
    }

    /// The span the cursor means at `span`: the function itself, when
    /// `span` is the hook part of a procedural function's name and nothing
    /// but the function backs that hook, which makes it a helper named
    /// after its extension rather than a hook implementation.
    pub(crate) fn drupal_hook_or_function<'m>(
        &self,
        map: &'m crate::symbol_map::SymbolMap,
        span: &'m crate::symbol_map::SymbolSpan,
    ) -> &'m crate::symbol_map::SymbolSpan {
        use crate::symbol_map::{HookRole, StringKeyKind, SymbolKind};
        let SymbolKind::StringKey {
            kind: StringKeyKind::DrupalHook,
            key,
            is_write: false,
            ..
        } = &span.kind
        else {
            return span;
        };
        let procedural = map
            .drupal_hooks
            .binary_search_by_key(&span.start, |site| site.start)
            .ok()
            .is_some_and(|at| {
                matches!(
                    map.drupal_hooks[at].role,
                    HookRole::Implementation {
                        is_theme: Some(_),
                        ..
                    }
                )
            });
        if !procedural || self.drupal_hook_is_backed(key) {
            return span;
        }
        // The function's own span starts before the hook part and ends with
        // it.
        let at = map.spans.partition_point(|other| other.start < span.start);
        map.spans[..at]
            .iter()
            .rev()
            .find(|other| {
                other.end == span.end && matches!(other.kind, SymbolKind::FunctionCall { .. })
            })
            .unwrap_or(span)
    }

    /// Whether the hook `key` is backed by anything but its implementations:
    /// documented, by name or by a pattern, or invoked.  A procedural
    /// function named after its extension whose hook nothing backs is
    /// read as the function it is.
    pub(crate) fn drupal_hook_is_backed(&self, key: &str) -> bool {
        self.hook_index().documents(key)
            || self
                .hook_occurrences(key)
                .iter()
                .any(|occurrence| matches!(occurrence.role, Occurrence::Invocation { .. }))
    }

    /// The documentation function of the hook the function or method
    /// declared at `decl_start` of `content` implements: a procedural
    /// function's (`name` `None`, read from the content), or a hook class
    /// method's (`name`, on the class `class_fqn`).  Read from the symbol
    /// map of the file that declares it, which has to be `content`.
    pub(crate) fn drupal_hook_documentation_function(
        &self,
        content: &str,
        decl_start: u32,
        method: Option<&str>,
        class_fqn: &str,
    ) -> Option<crate::types::FunctionInfo> {
        use crate::symbol_map::{StringKeyKind, SymbolKind};
        if !self.is_drupal_project() {
            return None;
        }
        let rest = content.get(decl_start as usize..)?;
        let keyword = rest.find("function")?;
        let after = &rest[keyword + "function".len()..];
        let name_from = after.len() - after.trim_start().len();
        let name_start = decl_start as usize + keyword + "function".len() + name_from;
        let name_len = content[name_start..]
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        let name = &content[name_start..name_start + name_len];
        let uri = match method {
            None => self
                .symbols
                .global_functions
                .read()
                .get(name)
                .map(|(uri, _)| uri.clone())?,
            Some(_) => self.class_declaration_uri(class_fqn.trim_start_matches('\\'))?,
        };
        let map = self
            .symbol_map_for(&uri)
            .filter(|map| map.matches_source(content))?;
        // A procedural function names its hook after its extension; a
        // method's `#[Hook]` stands among the attributes before its name.
        let (from, to) = match method {
            None => (name_start as u32, (name_start + name_len) as u32),
            Some(_) => (decl_start, name_start as u32),
        };
        let at = map.spans.partition_point(|span| span.start < from);
        let hook = map.spans[at..]
            .iter()
            .take_while(|span| span.start < to)
            .find_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalHook,
                    key,
                    is_write: false,
                    ..
                } => Some(key.clone()),
                _ => None,
            })?;
        let index = self.hook_index();
        let documented = if index
            .documented
            .binary_search_by(|known| known.as_str().cmp(&hook))
            .is_ok()
        {
            hook
        } else {
            index.best_patterns(&hook).first()?.to_string()
        };
        self.find_or_load_function(&[format!("hook_{documented}").as_str()])
    }

    /// Every hook the workspace names, implemented or documented, that the
    /// documented `pattern` stands for, and that no pattern writing out
    /// more documents.
    fn hooks_matching(&self, pattern: &str) -> Vec<String> {
        use crate::symbol_map::{StringKeyKind, SymbolKind};
        let index = self.hook_index();
        let is_entity_type = |id: &str| index.entity_types.contains(id);
        let mut hooks: Vec<String> = Vec::new();
        for (_, map) in self.user_file_symbol_maps() {
            for span in &map.spans {
                if let SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalHook,
                    key,
                    ..
                } = &span.kind
                    && !is_hook_pattern(key)
                    && !hooks.contains(key)
                    && matches_hook_pattern(pattern, key, &is_entity_type)
                    && index.best_patterns(key).contains(&pattern)
                {
                    hooks.push(key.clone());
                }
            }
        }
        hooks
    }

    fn hook_locations(
        &self,
        occurrences: &[&HookOccurrence],
    ) -> Vec<tower_lsp_server::gen_lsp_types::Location> {
        occurrences
            .iter()
            .filter_map(|occurrence| {
                let parsed = crate::util::parse_url(&occurrence.uri)?;
                let content = self.get_file_content_arc(&occurrence.uri)?;
                let lines = crate::text_position::LineIndex::new(&content);
                Some(tower_lsp_server::gen_lsp_types::Location {
                    uri: parsed,
                    range: tower_lsp_server::gen_lsp_types::Range {
                        start: lines.position(occurrence.start as usize),
                        end: lines.position(occurrence.end as usize),
                    },
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_fill_their_placeholders() {
        let entity = |id: &str| matches!(id, "node" | "taxonomy_term");
        let index = HookIndex {
            documented: Vec::new(),
            patterns: vec![
                "theme_suggestions_HOOK".to_string(),
                "theme_suggestions_HOOK_alter".to_string(),
            ],
            entity_types: Default::default(),
        };
        assert_eq!(
            index.best_patterns("theme_suggestions_node_alter"),
            ["theme_suggestions_HOOK_alter"]
        );
        assert_eq!(
            index.best_patterns("theme_suggestions_node"),
            ["theme_suggestions_HOOK"]
        );
        assert!(matches_hook_pattern(
            "form_FORM_ID_alter",
            "form_user_login_form_alter",
            &entity
        ));
        assert!(matches_hook_pattern(
            "ENTITY_TYPE_presave",
            "taxonomy_term_presave",
            &entity
        ));
        assert!(!matches_hook_pattern(
            "ENTITY_TYPE_presave",
            "acme_presave",
            &entity
        ));
        assert!(matches_hook_pattern("update_N", "update_10001", &entity));
        assert!(!matches_hook_pattern("update_N", "update_last", &entity));
        assert!(matches_hook_pattern(
            "entity_query_tag__ENTITY_TYPE__TAG_alter",
            "entity_query_tag__node__acme_alter",
            &entity
        ));
        assert!(!matches_hook_pattern(
            "form_FORM_ID_alter",
            "form_alter",
            &entity
        ));
        assert!(is_hook_pattern("preprocess_HOOK"));
        assert!(!is_hook_pattern("preprocess_page"));
    }

    #[test]
    fn hook_classes_belong_to_the_extension_of_their_namespace() {
        assert_eq!(
            hook_class_extension("Drupal\\acme\\Hook\\AcmeHooks"),
            Some("acme")
        );
        assert_eq!(
            hook_class_extension("\\Drupal\\acme\\Hook\\Form\\FormHooks"),
            Some("acme")
        );
        assert_eq!(hook_class_extension("Drupal\\acme\\Hooks"), None);
        assert_eq!(hook_class_extension("Drupal\\acme\\Form\\Hook"), None);
    }

    #[test]
    fn info_files_give_the_extension_and_its_kind() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, content: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, content).unwrap();
            path
        };
        let module = read_extension(&write("acme.info.yml", "name: Acme\ntype: module\n")).unwrap();
        assert_eq!((module.name.as_str(), module.is_theme), ("acme", false));
        let theme = read_extension(&write("olivero.info.yml", "type: 'theme'\n")).unwrap();
        assert!(theme.is_theme);
        assert!(read_extension(&write("engine.info.yml", "type: theme_engine\n")).is_none());
    }
}
