//! Yii DI definitions (`yiisoft/definitions`): the classes they bind, and
//! the arguments their method keys pass by name.
//!
//! ```php
//! MailerInterface::class => [
//!     'class' => Mailer::class,
//!     '__construct()' => ['transport' => Reference::to(TransportInterface::class)],
//!     'setLogger()' => [Reference::to(LoggerInterface::class)],
//! ],
//! ```
//!
//! Symbol-map extraction (`symbol_map/extraction/yii_definitions.rs`)
//! recognises a definition by its shape wherever it is written.  Its
//! `'name()'` and `'$name'` keys are member accesses on the class it builds,
//! which the shared member machinery navigates, renames, and checks.  What
//! is answered here is the rest: the parameter an argument key names, and
//! where the classes a definition names are bound.
//!
//! The project's own definitions are found in the symbol maps of its files.
//! An installed package contributes definitions through `yiisoft/config`,
//! by the `di` groups the `config-plugin` of its `composer.json` lists,
//! which `installed.json` repeats; those files are read from disk once.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::class_lookup::Fqn;
use crate::symbol_map::YiiDefinitionSite;
use crate::text_position::LineIndex;
use crate::types::{ClassInfo, MethodInfo};

/// The member a definition key names: the method of a `'name()'` key, as
/// `(name, true)`, or the property of a `'$name'` key, as `(name, false)`.
///
/// What follows a method key's `()` is not read, so one method can be
/// called twice under two keys (`'addItem()-first'`, `'addItem()-second'`).
pub(crate) fn definition_key(key: &str) -> Option<(&str, bool)> {
    let (name, is_method) = match key.split_once("()") {
        Some((_, rest)) if rest.contains("()") => return None,
        Some((name, _)) => (name, true),
        None => (key.strip_prefix('$')?, false),
    };
    let mut chars = name.chars();
    let starts = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || !c.is_ascii());
    (starts && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii()))
        .then_some((name, is_method))
}

/// The key of a [`YiiDefinitionArgument`](crate::symbol_map::StringKeyKind::YiiDefinitionArgument)
/// span: the argument `name` of the `class`'s `method`.  PHP compares
/// method names without regard to case, so the method is lowercased.
pub(crate) fn definition_argument_key(class: &str, method: &str, name: &str) -> String {
    format!(
        "{}::{}#{name}",
        class.trim_start_matches('\\'),
        method.to_ascii_lowercase()
    )
}

/// The class, the method, and the argument name of an argument's key.
pub(crate) fn split_definition_argument_key(key: &str) -> (&str, &str, &str) {
    let (callable, name) = key.rsplit_once('#').unwrap_or(("", key));
    let (class, method) = callable.rsplit_once("::").unwrap_or((callable, ""));
    (class, method, name)
}

/// The `di` groups of the `config-plugin` a package's `composer.json`
/// declares, whose files hold definitions: the common one, the web and
/// console ones, and the `widgets` group of `yiisoft/widget`.
fn is_definition_group(group: &str) -> bool {
    matches!(group, "di" | "di-web" | "di-console" | "widgets") || group.starts_with("widgets-")
}

/// The definition files `package` contributes: each file a definition group
/// of its `config-plugin` lists, under its `source-directory`.  A group that
/// names another (`'$di'`) adds no file of its own.
fn package_definition_files(package: &serde_json::Value, root: &Path) -> Vec<PathBuf> {
    let Some(extra) = package.get("extra") else {
        return Vec::new();
    };
    let Some(groups) = extra.get("config-plugin").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    let source = extra
        .get("config-plugin-options")
        .and_then(|options| options.get("source-directory"))
        .and_then(|dir| dir.as_str())
        .unwrap_or("");
    let dir = root.join(source);
    let mut files = Vec::new();
    for (group, entries) in groups {
        if is_definition_group(group) {
            group_files(entries, &dir, &mut files);
        }
    }
    files
}

/// The files the entries of one `config-plugin` group list, a file or a
/// list of them, relative to `dir`.  A pattern (`di/*.php`) lists the files
/// it matches, and an entry that names another group (`'$di'`) adds no file
/// of its own.
pub(crate) fn group_files(entries: &serde_json::Value, dir: &Path, out: &mut Vec<PathBuf>) {
    let entries: Vec<&str> = match entries {
        serde_json::Value::String(file) => vec![file.as_str()],
        serde_json::Value::Array(list) => list.iter().filter_map(|e| e.as_str()).collect(),
        _ => return,
    };
    for entry in entries {
        // `?` marks a file that may be missing, which is read when it is
        // there.
        let entry = entry.trim_start_matches('?');
        if entry.starts_with('$') {
            continue;
        }
        match entry.rsplit_once('/').unwrap_or(("", entry)) {
            (sub, pattern) if pattern.contains('*') => {
                let mut matched = Vec::new();
                glob_files(&dir.join(sub), pattern, &mut matched);
                // yiisoft/config reads a pattern's files in name order.
                matched.sort_unstable();
                out.extend(matched);
            }
            _ => out.push(dir.join(entry)),
        }
    }
}

/// The files in `dir` whose names match `pattern`, a name with `*`s in it.
fn glob_files(dir: &Path, pattern: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let parts: Vec<&str> = pattern.split('*').collect();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if matches_glob(name, &parts) && entry.path().is_file() {
            out.push(entry.path());
        }
    }
}

/// Whether `name` matches the pattern split at its `*`s into `parts`.
fn matches_glob(name: &str, parts: &[&str]) -> bool {
    let (Some(first), Some(last)) = (parts.first(), parts.last()) else {
        return false;
    };
    if parts.len() == 1 {
        return name == *first;
    }
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    for middle in &parts[1..parts.len() - 1] {
        let Some(at) = rest.find(middle) else {
            return false;
        };
        rest = &rest[at + middle.len()..];
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// The definition sites one package file holds, with the file's URI and
/// content for turning them into locations.
pub(crate) struct PackageDefinitionFile {
    uri: String,
    content: String,
    sites: Vec<YiiDefinitionSite>,
}

/// The definition sites of the files at `paths`.
fn read_package_definition_files(paths: &[PathBuf]) -> Vec<PackageDefinitionFile> {
    let mut files = Vec::new();
    for path in paths {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        if !content.contains("::class") {
            continue;
        }
        let uri = crate::util::path_to_uri(path);
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(uri.as_bytes());
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        let names = crate::names::OwnedResolvedNames::from_resolved(&resolver.resolve(program));
        let sites = crate::symbol_map::extraction::extract_symbol_map_for_file(
            program,
            &content,
            &names,
            crate::symbol_map::extraction::Frameworks {
                symfony: false,
                service_container: false,
                drupal: false,
                twig: false,
                doctrine: false,
                yii_params: false,
            },
            path.parent(),
            crate::drupal::hooks::DrupalFile::Other,
        )
        .yii_definitions;
        if !sites.is_empty() {
            files.push(PackageDefinitionFile {
                uri,
                content,
                sites,
            });
        }
    }
    files
}

/// The location of the bytes `[start, end)` of `content`, at `uri`.
fn location(uri: &str, lines: &LineIndex, start: u32, end: u32) -> Option<Location> {
    Some(Location {
        uri: crate::util::parse_url(uri)?,
        range: Range {
            start: lines.position(start as usize),
            end: lines.position(end as usize),
        },
    })
}

impl Backend {
    /// The definitions the installed packages contribute, read on first use.
    fn yii_package_definitions(&self) -> Arc<Vec<PackageDefinitionFile>> {
        let mut known = self.yii.package_definitions.lock();
        if let Some(known) = known.as_ref() {
            return Arc::clone(known);
        }
        let found = Arc::new(read_package_definition_files(
            &self.yii_package_definition_paths(),
        ));
        *known = Some(Arc::clone(&found));
        found
    }

    /// The definition files the installed packages contribute.
    pub(crate) fn yii_package_definition_paths(&self) -> Vec<PathBuf> {
        let vendors = self.workspace.vendor_dir_paths.lock().clone();
        let mut paths = Vec::new();
        for vendor in &vendors {
            let Some(installed) = crate::composer::read_installed_packages(vendor, "") else {
                continue;
            };
            for package in &installed.packages {
                if let Some(root) = installed.package_root(package) {
                    paths.extend(package_definition_files(package, &root));
                }
            }
        }
        paths.sort_unstable();
        paths.dedup();
        paths
    }

    /// Forget what the installed packages define, once they changed.
    pub(crate) fn forget_yii_package_definitions(&self) {
        *self.yii.package_definitions.lock() = None;
    }

    /// Where the DI definitions name the class `fqn`: the project's own,
    /// then the installed packages'.
    pub(crate) fn yii_definition_locations(&self, fqn: &str) -> Vec<Location> {
        let fqn = fqn.trim_start_matches('\\');
        let names = |site: &YiiDefinitionSite| site.class.as_str().eq_ignore_ascii_case(fqn);
        let mut locations = Vec::new();
        self.visit_declaring_symbol_maps(|uri, map| {
            if !map.yii_definitions.iter().any(names) {
                return;
            }
            let Some(content) = self.get_file_content_arc(uri) else {
                return;
            };
            let lines = LineIndex::new(&content);
            locations.extend(
                map.yii_definitions
                    .iter()
                    .filter(|site| names(site))
                    .filter_map(|site| location(uri, &lines, site.start, site.end)),
            );
        });
        for file in self.yii_package_definitions().iter() {
            if !file.sites.iter().any(names) {
                continue;
            }
            let lines = LineIndex::new(&file.content);
            locations.extend(
                file.sites
                    .iter()
                    .filter(|site| names(site))
                    .filter_map(|site| location(&file.uri, &lines, site.start, site.end)),
            );
        }
        locations
    }

    /// The class `fqn` with what it inherits, or `None` when it or a class
    /// it inherits from does not load, which would leave members out.
    pub(crate) fn yii_defined_class(&self, fqn: &str) -> Option<Arc<ClassInfo>> {
        let loader = self.resolved_class_loader_with(&[]);
        let class = loader(Fqn::new(fqn))?;
        if !crate::symfony::validation::ancestors_load(&class, &loader) {
            return None;
        }
        Some(crate::virtual_members::resolve_class_fully_cached(
            &class,
            &loader,
            &self.resolved_class_cache,
        ))
    }

    /// The parameters `method` of the class `fqn` takes by name, or `None`
    /// when that cannot be told: the class or the method is unknown (which
    /// the member check reports), or a variadic parameter collects any name.
    ///
    /// A class without a constructor of its own or inherited takes none.
    pub(crate) fn yii_definition_parameters(&self, fqn: &str, method: &str) -> Option<Vec<String>> {
        let class = self.yii_defined_class(fqn)?;
        let Some(method) = class.get_method_ci(method) else {
            return method.eq_ignore_ascii_case("__construct").then(Vec::new);
        };
        parameter_names(method)
    }

    /// What hover shows for an argument a definition passes by name: the
    /// parameter it fills, or that the method has none of that name.
    pub(crate) fn yii_definition_argument_detail(
        &self,
        class: &str,
        method: &str,
        name: &str,
    ) -> String {
        let Some(defined) = self.yii_defined_class(class) else {
            return format!("Passed to `{class}::{method}()`");
        };
        let Some(declared) = defined.get_method_ci(method) else {
            return format!("Passed to `{class}::{method}()`");
        };
        let callable = format!("{class}::{}()", declared.name);
        let parameter = declared
            .parameters
            .iter()
            .find(|p| p.name.strip_prefix('$').unwrap_or(&p.name) == name);
        match parameter {
            Some(parameter) => {
                let ty = parameter
                    .type_hint
                    .as_ref()
                    .map(|ty| format!("{ty} "))
                    .unwrap_or_default();
                format!("Parameter `{ty}${name}` of `{callable}`")
            }
            None => format!("`{callable}` has no parameter `${name}`"),
        }
    }

    /// Where the parameter an argument key names is declared.
    pub(crate) fn yii_definition_argument_locations(&self, key: &str) -> Vec<Location> {
        let (class, method, name) = split_definition_argument_key(key);
        let Some(declared) = self.class_member_declaration_location(class, method, false) else {
            return Vec::new();
        };
        let uri = declared.uri.as_str();
        let Some(content) = self.get_file_content_arc(uri) else {
            return Vec::new();
        };
        let lines = LineIndex::new(&content);
        let from = crate::text_position::position_to_byte_offset(&content, declared.range.start);
        parameter_offset(&content, from, name)
            .and_then(|start| location(uri, &lines, start as u32, (start + 1 + name.len()) as u32))
            .into_iter()
            .collect()
    }
}

/// The names `method` takes, without their `$`, or `None` when a variadic
/// parameter takes any.
fn parameter_names(method: &MethodInfo) -> Option<Vec<String>> {
    let mut names = Vec::with_capacity(method.parameters.len());
    for parameter in method.parameters.iter() {
        if parameter.is_variadic {
            return None;
        }
        names.push(
            parameter
                .name
                .strip_prefix('$')
                .unwrap_or(&parameter.name)
                .to_string(),
        );
    }
    Some(names)
}

/// Where the parameter `$name` of the method whose name starts at `from` is
/// written: the first `$name` after it that is not the start of a longer
/// name.
fn parameter_offset(content: &str, from: usize, name: &str) -> Option<usize> {
    let needle = format!("${name}");
    let bytes = content.as_bytes();
    let mut at = from;
    while let Some(found) = memchr::memmem::find(bytes.get(at..)?, needle.as_bytes()) {
        let start = at + found;
        let end = start + needle.len();
        let continues = bytes
            .get(end)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b >= 0x80);
        if !continues {
            return Some(start);
        }
        at = end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_name_methods_and_properties() {
        assert_eq!(definition_key("setLogger()"), Some(("setLogger", true)));
        assert_eq!(definition_key("$pageSize"), Some(("pageSize", false)));
        assert_eq!(definition_key("__construct()"), Some(("__construct", true)));
        assert_eq!(definition_key("class"), None);
        assert_eq!(definition_key("()"), None);
        assert_eq!(definition_key("$"), None);
        assert_eq!(definition_key("set logger()"), None);
        assert_eq!(definition_key("1st()"), None);
        assert_eq!(definition_key("addItem()-theme"), Some(("addItem", true)));
        assert_eq!(definition_key("a()b()"), None);
    }

    #[test]
    fn an_argument_key_names_its_class_and_method() {
        let key = definition_argument_key("\\App\\Mailer", "__Construct", "transport");
        assert_eq!(key, "App\\Mailer::__construct#transport");
        assert_eq!(
            split_definition_argument_key(&key),
            ("App\\Mailer", "__construct", "transport")
        );
    }

    #[test]
    fn globs_match_a_name_by_its_parts() {
        let parts = |pattern: &'static str| pattern.split('*').collect::<Vec<_>>();
        assert!(matches_glob("blog.php", &parts("*.php")));
        assert!(matches_glob("di-web.php", &parts("di-*.php")));
        assert!(!matches_glob("di.php", &parts("di-*.php")));
        assert!(!matches_glob("blog.yaml", &parts("*.php")));
        assert!(matches_glob("di.php", &parts("di.php")));
    }

    #[test]
    fn a_parameter_is_found_after_its_method() {
        let php = "function __construct(private array $postsById, array $posts) {}";
        let at = parameter_offset(php, 0, "posts").unwrap();
        assert_eq!(&php[at..at + 6], "$posts");
        assert_eq!(parameter_offset(php, 0, "missing"), None);
    }

    #[test]
    fn a_package_lists_its_definition_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config/di")).unwrap();
        std::fs::write(dir.path().join("config/di/a.php"), "<?php").unwrap();
        std::fs::write(dir.path().join("config/di/b.txt"), "").unwrap();
        let package = serde_json::json!({
            "extra": {
                "config-plugin-options": {"source-directory": "config"},
                "config-plugin": {
                    "di": "di.php",
                    "di-web": ["$di", "?web.php", "di/*.php"],
                    "params": "params.php",
                },
            },
        });
        let mut files = package_definition_files(&package, dir.path());
        files.sort();
        let config = dir.path().join("config");
        assert_eq!(
            files,
            vec![
                config.join("di").join("a.php"),
                config.join("di.php"),
                config.join("web.php"),
            ]
        );
    }
}
