//! The `config-plugin` of a `composer.json` in a Yii project: the config
//! groups a package or the project declares and the files each one lists.
//!
//! ```json
//! "extra": {
//!     "config-plugin": {
//!         "params": "config/params.php",
//!         "di-web": ["config/di-web.php", "$di"]
//!     }
//! }
//! ```
//!
//! A group is a key of `config-plugin`, or of one environment under
//! `config-plugin-environments`.  Its entries are a file or a list of them,
//! relative to the `source-directory` of `config-plugin-options`: a path, a
//! pattern (`di/*.php`), a path `?` marks optional, or `$group`, which adds
//! the entries of another group.
//!
//! The document is read by a scanner that keeps each string's place, and
//! that reads a document still being typed as far as it goes, so the
//! cursor's string is known before the document is valid JSON again.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{
    CompletionItemKind, CompletionResponse, Diagnostic, DiagnosticSeverity, Location, Position,
    Range,
};

use crate::Backend;
use crate::text_position::{LineIndex, position_to_offset};

/// Whether `uri` names a `composer.json`.
pub(crate) fn is_composer_json_uri(uri: &str) -> bool {
    uri.split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .ends_with("/composer.json")
}

/// One step of the way from the document's root to a value.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Key(String),
    Index,
}

/// A string of the document: where its text is, inside the quotes, what it
/// says, and where it sits.
#[derive(Debug)]
struct JsonString {
    /// The offset of its first byte after the opening quote.
    start: usize,
    /// The offset of its closing quote, or of the end of the line when it
    /// has none yet.
    end: usize,
    value: String,
    is_key: bool,
    /// The keys and list items leading to it: to the object it is a key
    /// of, or to the value it is.
    path: Vec<Segment>,
}

enum Frame {
    Object {
        key: Option<String>,
        expecting_key: bool,
    },
    Array,
}

/// The way to the value the innermost frame is at.
fn frames_path(frames: &[Frame]) -> Vec<Segment> {
    frames
        .iter()
        .map(|frame| match frame {
            Frame::Object { key, .. } => Segment::Key(key.clone().unwrap_or_default()),
            Frame::Array => Segment::Index,
        })
        .collect()
}

/// Every string of `content`, in order.  A string with no closing quote ends
/// at the end of its line.
fn scan_strings(content: &str) -> Vec<JsonString> {
    let bytes = content.as_bytes();
    let mut frames: Vec<Frame> = Vec::new();
    let mut strings = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => frames.push(Frame::Object {
                key: None,
                expecting_key: true,
            }),
            b'[' => frames.push(Frame::Array),
            b'}' | b']' => {
                frames.pop();
            }
            b':' => {
                if let Some(Frame::Object { expecting_key, .. }) = frames.last_mut() {
                    *expecting_key = false;
                }
            }
            b',' => {
                if let Some(Frame::Object { key, expecting_key }) = frames.last_mut() {
                    *key = None;
                    *expecting_key = true;
                }
            }
            b'"' => {
                let start = i + 1;
                let mut j = start;
                let mut closed = false;
                while j < bytes.len() {
                    match bytes[j] {
                        b'\\' => j += 2,
                        b'"' => {
                            closed = true;
                            break;
                        }
                        b'\n' | b'\r' => break,
                        _ => j += 1,
                    }
                }
                let end = j.min(bytes.len());
                let raw = &content[start..end];
                let value = if closed {
                    serde_json::from_str::<String>(&content[i..=end])
                        .unwrap_or_else(|_| raw.to_string())
                } else {
                    raw.to_string()
                };
                let is_key = matches!(
                    frames.last(),
                    Some(Frame::Object {
                        expecting_key: true,
                        ..
                    })
                );
                let path = if is_key {
                    frames_path(&frames[..frames.len() - 1])
                } else {
                    frames_path(&frames)
                };
                if is_key && let Some(Frame::Object { key, .. }) = frames.last_mut() {
                    *key = Some(value.clone());
                }
                strings.push(JsonString {
                    start,
                    end,
                    value,
                    is_key,
                    path,
                });
                i = if closed { end + 1 } else { end };
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    strings
}

/// Which groups a group or an entry is among: the base ones, or one
/// environment's.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Scope {
    Base,
    Environment(String),
}

/// What a string of the document is to the `config-plugin`.
#[derive(Debug, PartialEq, Eq)]
enum Role {
    /// The name of a group.
    Group(Scope),
    /// An entry of the group named.
    Entry(Scope, String),
    SourceDirectory,
}

fn key(segment: &Segment) -> Option<&str> {
    match segment {
        Segment::Key(key) => Some(key),
        Segment::Index => None,
    }
}

fn role(string: &JsonString) -> Option<Role> {
    let [first, rest @ ..] = string.path.as_slice() else {
        return None;
    };
    if key(first)? != "extra" {
        return None;
    }
    let keys: Vec<Option<&str>> = rest.iter().map(key).collect();
    if string.is_key {
        return match keys.as_slice() {
            [Some("config-plugin")] => Some(Role::Group(Scope::Base)),
            [Some("config-plugin-environments"), Some(environment)] => {
                Some(Role::Group(Scope::Environment(environment.to_string())))
            }
            _ => None,
        };
    }
    if let [Some("config-plugin-options"), Some("source-directory")] = keys.as_slice() {
        return Some(Role::SourceDirectory);
    }
    // A group's entries are a string or a list of them.
    let group_path = match keys.as_slice() {
        [head @ .., None] => head,
        keys => keys,
    };
    match group_path {
        [Some("config-plugin"), Some(group)] => Some(Role::Entry(Scope::Base, group.to_string())),
        [
            Some("config-plugin-environments"),
            Some(environment),
            Some(group),
        ] => Some(Role::Entry(
            Scope::Environment(environment.to_string()),
            group.to_string(),
        )),
        _ => None,
    }
}

/// A string of the document that means something to the `config-plugin`.
struct Site {
    string: JsonString,
    role: Role,
}

/// The `config-plugin` of one `composer.json`.
struct ConfigPlugin {
    sites: Vec<Site>,
    /// The directory the entries are relative to.
    dir: PathBuf,
    /// The directory of the `composer.json`.
    package_dir: PathBuf,
}

impl ConfigPlugin {
    fn read(path: &Path, content: &str) -> Self {
        let sites: Vec<Site> = scan_strings(content)
            .into_iter()
            .filter_map(|string| {
                let role = role(&string)?;
                Some(Site { string, role })
            })
            .collect();
        let package_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let source = sites
            .iter()
            .find(|site| site.role == Role::SourceDirectory)
            .map_or("", |site| site.string.value.as_str());
        Self {
            dir: crate::document_links::normalize_path(&package_dir.join(source)),
            package_dir,
            sites,
        }
    }

    /// The site whose string the cursor at `offset` is in.
    fn site_at(&self, offset: usize) -> Option<&Site> {
        self.sites
            .iter()
            .find(|site| site.string.start <= offset && offset <= site.string.end)
    }

    /// The groups declared among `scope`'s.
    fn groups(&self, scope: &Scope) -> impl Iterator<Item = &Site> {
        self.sites
            .iter()
            .filter(move |site| matches!(&site.role, Role::Group(s) if s == scope))
    }
}

/// What an entry names, once its `?` is set aside.
enum EntryTarget<'a> {
    Group(&'a str),
    Path { path: &'a str, optional: bool },
}

fn entry_target(entry: &str) -> EntryTarget<'_> {
    let optional = entry.starts_with('?');
    let entry = entry.trim_start_matches('?');
    match entry.strip_prefix('$') {
        Some(group) => EntryTarget::Group(group),
        None => EntryTarget::Path {
            path: entry,
            optional,
        },
    }
}

/// The names of the groups the installed packages declare.
fn package_group_names(packages: &crate::composer::InstalledPackages) -> Vec<String> {
    packages
        .packages
        .iter()
        .filter_map(|package| package.get("extra")?.get("config-plugin")?.as_object())
        .flat_map(|groups| groups.keys().cloned())
        .collect()
}

impl Backend {
    /// Whether `uri` is a `composer.json` whose `config-plugin` is read.
    pub(crate) fn is_yii_config_plugin_uri(&self, uri: &str) -> bool {
        self.is_yii_project() && is_composer_json_uri(uri)
    }

    /// The names of the groups the installed packages declare, read on
    /// first use.
    fn yii_package_group_names(&self) -> Arc<Vec<String>> {
        let mut known = self.yii.package_groups.lock();
        if let Some(known) = known.as_ref() {
            return Arc::clone(known);
        }
        let vendors = self.workspace.vendor_dir_paths.lock().clone();
        let mut names: Vec<String> = vendors
            .iter()
            .filter_map(|vendor| crate::composer::read_installed_packages(vendor, ""))
            .flat_map(|installed| package_group_names(&installed))
            .collect();
        names.sort_unstable();
        names.dedup();
        let names = Arc::new(names);
        *known = Some(Arc::clone(&names));
        names
    }

    /// Forget the groups the installed packages declare, once they changed.
    pub(crate) fn forget_yii_package_groups(&self) {
        *self.yii.package_groups.lock() = None;
    }

    /// Every group name an entry of `plugin` can mean: its own, the
    /// project's, and the installed packages'.
    fn yii_known_group_names(&self, plugin: &ConfigPlugin) -> Vec<String> {
        let mut names: Vec<String> = plugin
            .sites
            .iter()
            .filter(|site| matches!(site.role, Role::Group(_)))
            .map(|site| site.string.value.clone())
            .collect();
        names.extend(self.yii_config_plan().groups().iter().cloned());
        names.extend(self.yii_package_group_names().iter().cloned());
        names.retain(|name| !name.is_empty());
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Complete a group name or an entry of a `config-plugin`.
    pub(crate) fn try_yii_config_plugin_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let path = crate::util::uri_to_path(uri)?;
        let plugin = ConfigPlugin::read(&path, content);
        let cursor = position_to_offset(content, position) as usize;
        let site = plugin.site_at(cursor)?;
        let typed = content.get(site.string.start..cursor)?;
        match &site.role {
            Role::Group(scope) => {
                // A group declared beside it already is no new name, but
                // the one under the cursor is still being written.
                let taken: Vec<&str> = plugin
                    .groups(scope)
                    .filter(|other| other.string.start != site.string.start)
                    .map(|other| other.string.value.as_str())
                    .collect();
                let mut names = self.yii_known_group_names(&plugin);
                names.retain(|name| !taken.contains(&name.as_str()));
                crate::completion::laravel_string_keys::string_key_response(
                    names,
                    CompletionItemKind::Module,
                    content,
                    site.string.start,
                    position,
                )
            }
            Role::Entry(scope, group) => {
                let optional = usize::from(typed.starts_with('?'));
                let start = site.string.start + optional;
                if typed[optional..].starts_with('$') {
                    // An environment's group adds the base group of its own
                    // name that way, but a base group cannot add itself.
                    let mut names: Vec<String> = self
                        .yii_known_group_names(&plugin)
                        .into_iter()
                        .filter(|name| *scope != Scope::Base || name != group)
                        .map(|name| format!("${name}"))
                        .collect();
                    names.sort_unstable();
                    crate::completion::laravel_string_keys::string_key_response(
                        names,
                        CompletionItemKind::Module,
                        content,
                        start,
                        position,
                    )
                } else {
                    crate::completion::laravel_paths::directory_entry_completion(
                        &plugin.dir,
                        &typed[optional..],
                        start,
                        content,
                        position,
                    )
                }
            }
            Role::SourceDirectory => None,
        }
    }

    /// Go-to-definition on an entry of a `config-plugin`: the group a
    /// `$group` names, among the cursor's own groups first and then the
    /// base ones, or the files a path or a pattern names.  A group never
    /// names itself, so an environment's `params` that adds `$params` adds
    /// the base group's.
    pub(crate) fn resolve_yii_config_plugin_definition(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Vec<Location>> {
        let path = crate::util::uri_to_path(uri)?;
        let plugin = ConfigPlugin::read(&path, content);
        let site = plugin.site_at(position_to_offset(content, position) as usize)?;
        let Role::Entry(scope, own) = &site.role else {
            return None;
        };
        match entry_target(&site.string.value) {
            EntryTarget::Group(group) => {
                let declared = plugin
                    .groups(scope)
                    .filter(|declared| declared.string.value != *own)
                    .chain(plugin.groups(&Scope::Base))
                    .find(|declared| declared.string.value == group)?;
                let lines = LineIndex::new(content);
                let target = crate::util::parse_url(uri)?;
                Some(vec![Location {
                    uri: target,
                    range: Range {
                        start: lines.position(declared.string.start),
                        end: lines.position(declared.string.end),
                    },
                }])
            }
            EntryTarget::Path { path, .. } => {
                let mut files = Vec::new();
                super::definitions::group_files(
                    &serde_json::Value::String(path.to_string()),
                    &plugin.dir,
                    &mut files,
                );
                let locations: Vec<Location> = files
                    .into_iter()
                    .filter(|file| file.is_file())
                    .filter_map(|file| {
                        Some(Location {
                            uri: crate::util::parse_url(&crate::util::path_to_uri(&file))?,
                            range: Range::default(),
                        })
                    })
                    .collect();
                (!locations.is_empty()).then_some(locations)
            }
        }
    }

    /// Report each path a `config-plugin` lists that names no file.  A path
    /// `?` marks optional may be missing, and a pattern may match nothing.
    pub(crate) fn collect_yii_config_plugin_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(path) = crate::util::uri_to_path(uri) else {
            return;
        };
        let plugin = ConfigPlugin::read(&path, content);
        let lines = LineIndex::new(content);
        for site in &plugin.sites {
            if !matches!(site.role, Role::Entry(..)) {
                continue;
            }
            let EntryTarget::Path {
                path: entry,
                optional: false,
            } = entry_target(&site.string.value)
            else {
                continue;
            };
            if entry.is_empty() || entry.contains('*') || plugin.dir.join(entry).is_file() {
                continue;
            }
            let shown = plugin
                .dir
                .join(entry)
                .strip_prefix(&plugin.package_dir)
                .map_or_else(
                    |_| entry.to_string(),
                    |relative| relative.to_string_lossy().replace('\\', "/"),
                );
            out.push(crate::diagnostics::helpers::make_diagnostic(
                Range {
                    start: lines.position(site.string.start),
                    end: lines.position(site.string.end),
                },
                DiagnosticSeverity::Warning,
                "invalid_yii_config_file",
                format!("Unknown config file '{shown}'"),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(content: &str) -> Vec<(String, Role)> {
        scan_strings(content)
            .into_iter()
            .filter_map(|string| {
                let role = role(&string)?;
                Some((string.value, role))
            })
            .collect()
    }

    #[test]
    fn groups_and_entries_are_told_apart() {
        let content = r#"{
    "name": "acme/app",
    "extra": {
        "config-plugin-options": { "source-directory": "config" },
        "config-plugin": {
            "params": "params.php",
            "di-web": ["$di", "?di-web.php"]
        },
        "config-plugin-environments": {
            "dev": { "params": ["dev/params.php"] }
        }
    }
}"#;
        assert_eq!(
            roles(content),
            vec![
                ("config".to_string(), Role::SourceDirectory),
                ("params".to_string(), Role::Group(Scope::Base)),
                (
                    "params.php".to_string(),
                    Role::Entry(Scope::Base, "params".to_string())
                ),
                ("di-web".to_string(), Role::Group(Scope::Base)),
                (
                    "$di".to_string(),
                    Role::Entry(Scope::Base, "di-web".to_string())
                ),
                (
                    "?di-web.php".to_string(),
                    Role::Entry(Scope::Base, "di-web".to_string())
                ),
                (
                    "params".to_string(),
                    Role::Group(Scope::Environment("dev".to_string()))
                ),
                (
                    "dev/params.php".to_string(),
                    Role::Entry(Scope::Environment("dev".to_string()), "params".to_string())
                ),
            ]
        );
    }

    #[test]
    fn a_string_still_being_typed_is_read() {
        let content = "{\"extra\": {\"config-plugin\": {\"di\": [\"$p\n";
        let strings = scan_strings(content);
        let last = strings.last().unwrap();
        assert_eq!(last.value, "$p");
        assert_eq!(last.end, content.len() - 1);
        assert_eq!(role(last), Some(Role::Entry(Scope::Base, "di".to_string())));
    }

    #[test]
    fn escapes_do_not_end_a_string() {
        let strings = scan_strings(r#"{"a\"b": "c\\"}"#);
        assert_eq!(strings[0].value, "a\"b");
        assert!(strings[0].is_key);
        assert_eq!(strings[1].value, "c\\");
        assert!(!strings[1].is_key);
    }

    #[test]
    fn only_composer_json_is_read() {
        assert!(is_composer_json_uri("file:///app/composer.json"));
        assert!(!is_composer_json_uri("file:///app/my-composer.json"));
        assert!(!is_composer_json_uri("file:///app/composer.lock"));
    }
}
