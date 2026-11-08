//! Yii params (`yiisoft/config`): the `$params` array every configuration
//! file is handed, and the keys it reads from it.
//!
//! ```php
//! // config/common/di/application.php
//! 'name' => $params['application']['name'],
//! ```
//!
//! `yiisoft/config` merges `$params` from the `params` groups: the base
//! `params` group, the `params-web` and `params-console` groups the runners
//! nest under it, and the environment's override, each made of the files
//! the installed packages contribute followed by the project's own.  The
//! merge is recursive: a key whose value is an array in both files merges
//! the two, and any other value replaces the one before it.
//!
//! The merge plan is read statically rather than run.  The project's groups
//! come from the `config-plugin` of its `composer.json`, or the file its
//! `config-plugin-file` names (`config/configuration.php` in `yiisoft/app`),
//! and a package's from the `config-plugin` of its `composer.json`, which
//! `installed.json` repeats.  Every file of a group other than a params one
//! is handed `$params`, and the reads of it there are
//! [`YiiParam`](crate::symbol_map::StringKeyKind::YiiParam) spans keyed by
//! the path of keys they read (see [`param_key`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::php_type::{PhpType, ShapeEntry};
use crate::text_position::LineIndex;

/// Separates the keys of a path in a
/// [`YiiParam`](crate::symbol_map::StringKeyKind::YiiParam) key.  A params
/// key can hold any other character: `yiisoft/yii-debug` names its
/// collectors `collectors.web`.
pub(crate) const KEY_SEPARATOR: char = '\u{1f}';

/// The key of a read of the path `keys` from `$params`.
pub(crate) fn param_key<S: AsRef<str>>(keys: &[S]) -> String {
    let mut key = String::new();
    for (i, segment) in keys.iter().enumerate() {
        if i > 0 {
            key.push(KEY_SEPARATOR);
        }
        key.push_str(segment.as_ref());
    }
    key
}

/// The keys of the path a [`param_key`] spells.
pub(crate) fn param_key_segments(key: &str) -> std::str::Split<'_, char> {
    key.split(KEY_SEPARATOR)
}

/// A params key as the code reads it: `['application']['name']`.
pub(crate) fn param_key_display(key: &str) -> String {
    param_key_segments(key)
        .map(|segment| format!("['{segment}']"))
        .collect()
}

/// Whether `group` is one `$params` is merged from.  The runners name their
/// params groups `params`, `params-web`, and `params-console`.
fn is_params_group(group: &str) -> bool {
    group == "params" || group.starts_with("params-")
}

/// How much a params file's keys weigh against another's: the higher one
/// wins.  Files of one rank are alternatives, as the web and console groups
/// and the environments are, or packages, which may not declare a key twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    /// A package's base `params` group.
    Package,
    /// The project's base `params` group.
    Project,
    /// A package's nested params group (`params-web`).
    NestedPackage,
    /// The project's nested params group.
    NestedProject,
    /// An environment's params.
    Environment,
}

/// The project's merge plan, as far as `$params` goes: the params files in
/// merge order, and the files that are handed `$params`.
#[derive(Debug, Default)]
pub(crate) struct ConfigPlan {
    params_files: Vec<(Rank, PathBuf)>,
    receives_params: HashSet<PathBuf>,
    /// The files of the project's events groups, which list the event
    /// listeners (see [`super::events`]).
    events_files: HashSet<PathBuf>,
    /// The names of the project's groups, its environments' included.
    groups: Vec<String>,
}

impl ConfigPlan {
    /// Read the project's merge plan from `root`'s `composer.json`.
    pub(crate) fn load(root: &Path) -> Self {
        let mut plan = Self::default();
        let Some(extra) = std::fs::read_to_string(root.join("composer.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|composer| composer.get("extra").cloned())
        else {
            return plan;
        };
        let extra = match extra.get("config-plugin-file").and_then(|f| f.as_str()) {
            Some(file) => match read_php_config(&root.join(file)) {
                Some(extra) => extra,
                None => return plan,
            },
            None => extra,
        };
        let source = extra
            .get("config-plugin-options")
            .and_then(|options| options.get("source-directory"))
            .and_then(|dir| dir.as_str())
            .unwrap_or("");
        let dir = crate::document_links::normalize_path(&root.join(source));
        let mut receives = Vec::new();
        let mut events = Vec::new();
        if let Some(groups) = extra.get("config-plugin").and_then(|g| g.as_object()) {
            for (group, entries) in groups {
                plan.groups.push(group.clone());
                match group.as_str() {
                    "params" => plan.add_params(Rank::Project, entries, &dir),
                    group if is_params_group(group) => {
                        plan.add_params(Rank::NestedProject, entries, &dir);
                    }
                    group => {
                        if super::events::is_events_group(group) {
                            super::definitions::group_files(entries, &dir, &mut events);
                        }
                        super::definitions::group_files(entries, &dir, &mut receives);
                    }
                }
            }
        }
        if let Some(environments) = extra
            .get("config-plugin-environments")
            .and_then(|e| e.as_object())
        {
            for groups in environments.values().filter_map(|g| g.as_object()) {
                for (group, entries) in groups {
                    plan.groups.push(group.clone());
                    if is_params_group(group) {
                        plan.add_params(Rank::Environment, entries, &dir);
                    } else {
                        if super::events::is_events_group(group) {
                            super::definitions::group_files(entries, &dir, &mut events);
                        }
                        super::definitions::group_files(entries, &dir, &mut receives);
                    }
                }
            }
        }
        plan.params_files.sort_by_key(|(rank, _)| *rank);
        plan.receives_params = receives.iter().map(|path| canonical(path)).collect();
        plan.events_files = events.iter().map(|path| canonical(path)).collect();
        plan
    }

    fn add_params(&mut self, rank: Rank, entries: &serde_json::Value, dir: &Path) {
        let mut files = Vec::new();
        super::definitions::group_files(entries, dir, &mut files);
        self.params_files
            .extend(files.into_iter().map(|file| (rank, file)));
    }

    /// The names of the project's groups, its environments' included.
    pub(crate) fn groups(&self) -> &[String] {
        &self.groups
    }

    /// The project's configuration files other than its params files: the
    /// files `yiisoft/config` hands the `$params`.
    pub(crate) fn config_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.receives_params.iter()
    }

    /// Whether `yiisoft/config` hands the file at `path` the `$params`.
    pub(crate) fn receives_params(&self, path: &Path) -> bool {
        contains_path(&self.receives_params, path)
    }

    /// The files of the project's events groups.
    pub(crate) fn events_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.events_files.iter()
    }

    /// Whether the file at `path` is one of the project's events groups'.
    pub(crate) fn is_events_file(&self, path: &Path) -> bool {
        contains_path(&self.events_files, path)
    }
}

/// `path` as the plan keeps it: canonical when the file is there.
fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| crate::document_links::normalize_path(path))
}

/// Whether `files`, kept canonical, hold `path`.  A path reached another
/// way (a short name on Windows) only matches once it is canonical too.
fn contains_path(files: &HashSet<PathBuf>, path: &Path) -> bool {
    !files.is_empty()
        && (files.contains(&crate::document_links::normalize_path(path))
            || dunce::canonicalize(path).is_ok_and(|path| files.contains(&path)))
}

/// The params files `package` contributes, ranked: each file its
/// `config-plugin` lists under a params group, under its `source-directory`.
fn package_params_files(package: &serde_json::Value, root: &Path) -> Vec<(Rank, PathBuf)> {
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
    let mut ranked = Vec::new();
    for (group, entries) in groups {
        let rank = match group.as_str() {
            "params" => Rank::Package,
            group if is_params_group(group) => Rank::NestedPackage,
            _ => continue,
        };
        let mut files = Vec::new();
        super::definitions::group_files(entries, &dir, &mut files);
        ranked.extend(files.into_iter().map(|file| (rank, file)));
    }
    ranked
}

/// The array a PHP configuration file returns, as JSON: string and list
/// entries are kept, and every other value is `null`.  A key that is not a
/// literal (`Environment::DEV`) is kept by its source text.
fn read_php_config(path: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(path).ok()?;
    crate::parser::with_parsed_program(&content, "yii_config_plugin_file", |program, content| {
        crate::virtual_members::laravel::array_file::returned_exprs(program)
            .first()
            .map(|returned| php_value_to_json(returned, content))
    })
}

fn php_value_to_json(expr: &Expression<'_>, content: &str) -> serde_json::Value {
    use mago_span::HasSpan;

    let elements = match expr {
        Expression::Parenthesized(p) => return php_value_to_json(p.expression, content),
        Expression::Literal(Literal::String(literal)) => {
            return literal
                .value
                .and_then(crate::atom::literal_bytes_to_str)
                .map_or(serde_json::Value::Null, |text| {
                    serde_json::Value::String(text.to_string())
                });
        }
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return serde_json::Value::Null,
    };
    let keyed = elements
        .iter()
        .any(|element| matches!(element, ArrayElement::KeyValue(_)));
    if !keyed {
        return serde_json::Value::Array(
            elements
                .iter()
                .filter_map(|element| match element {
                    ArrayElement::Value(value) => Some(php_value_to_json(value.value, content)),
                    _ => None,
                })
                .collect(),
        );
    }
    let mut object = serde_json::Map::new();
    for element in elements.iter() {
        let ArrayElement::KeyValue(entry) = element else {
            continue;
        };
        let key = match entry.key {
            Expression::Literal(Literal::String(key)) => key
                .value
                .and_then(crate::atom::literal_bytes_to_str)
                .map(str::to_string),
            key => {
                let span = key.span();
                content
                    .get(span.start.offset as usize..span.end.offset as usize)
                    .map(str::to_string)
            }
        };
        if let Some(key) = key {
            object.insert(key, php_value_to_json(entry.value, content));
        }
    }
    serde_json::Value::Object(object)
}

/// Where a params key is declared: the file, and the range of the key's
/// string there, inside the quotes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamDeclaration {
    pub(crate) file: PathBuf,
    pub(crate) range: Range,
}

/// One key of the merged params, or the params themselves.
#[derive(Debug, Default)]
pub(crate) struct ParamNode {
    /// The keys of the arrays declared here, in declaration order.
    children: Vec<(String, ParamNode)>,
    /// The entries that declare the key, the ones of the rank that wins.
    declarations: Vec<ParamDeclaration>,
    rank: Option<Rank>,
    /// Whether a file of a rank below the environments' declares the key,
    /// so that it is there whichever environment the application runs in.
    in_every_environment: bool,
    /// The types of the values declared here other than arrays with keys.
    values: Vec<PhpType>,
    /// The values declared here that are string literals.
    strings: Vec<String>,
    /// Whether a value declared here holds keys that cannot be read: an
    /// array with a spread or a computed key, or a value that is no array
    /// literal.
    open: bool,
}

impl ParamNode {
    /// The key `name` directly under this one.
    pub(crate) fn child(&self, name: &str) -> Option<&ParamNode> {
        self.children
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, child)| child)
    }

    /// The names of the keys directly under this one, in declaration order.
    pub(crate) fn child_names(&self) -> impl Iterator<Item = &str> {
        self.children.iter().map(|(key, _)| key.as_str())
    }

    /// The entries that declare this key.
    pub(crate) fn declarations(&self) -> &[ParamDeclaration] {
        &self.declarations
    }

    /// The string the key holds, when every value declared for it is that
    /// one string literal.
    pub(crate) fn string_value(&self) -> Option<&str> {
        let (first, rest) = self.strings.split_first()?;
        (self.values.len() == self.strings.len()
            && self.children.is_empty()
            && rest.iter().all(|other| other == first))
        .then_some(first.as_str())
    }

    /// Whether every key under this one is known, so that a read of any
    /// other can be told to name nothing.
    fn keys_known(&self) -> bool {
        !self.open && self.values.is_empty() && !self.children.is_empty()
    }

    /// The type of the value under this key.
    pub(crate) fn php_type(&self) -> PhpType {
        let mut members: Vec<PhpType> = Vec::new();
        for value in &self.values {
            if !members.contains(value) {
                members.push(value.clone());
            }
        }
        if !self.children.is_empty() {
            members.push(PhpType::array_shape(
                self.children
                    .iter()
                    .map(|(key, child)| ShapeEntry {
                        key: Some(key.clone()),
                        value_type: child.php_type(),
                        optional: !child.in_every_environment,
                    })
                    .collect(),
            ));
        }
        match members.len() {
            0 => PhpType::mixed(),
            1 => members.pop().unwrap_or_else(PhpType::mixed),
            _ => PhpType::union(members),
        }
    }

    fn child_mut(&mut self, name: &str) -> &mut ParamNode {
        let at = match self.children.iter().position(|(key, _)| key == name) {
            Some(at) => at,
            None => {
                self.children.push((name.to_string(), ParamNode::default()));
                self.children.len() - 1
            }
        };
        &mut self.children[at].1
    }

    /// Note that a file of `rank` declares this key at `declaration`, with
    /// a value that is an array with keys when `merges`.  A higher rank
    /// replaces what the lower ones declared, except that an array merges
    /// with the keys already there, and every file that adds to them
    /// declares the key.
    fn declare(&mut self, rank: Rank, declaration: ParamDeclaration, merges: bool) {
        if self.rank.is_none_or(|known| rank > known) {
            let merged = merges && self.values.is_empty() && !self.children.is_empty();
            if !merges {
                self.children.clear();
            }
            if !merged {
                self.declarations.clear();
            }
            self.values.clear();
            self.strings.clear();
            self.open = false;
            self.rank = Some(rank);
        }
        if rank < Rank::Environment {
            self.in_every_environment = true;
        }
        if !self.declarations.contains(&declaration) {
            self.declarations.push(declaration);
        }
    }
}

/// The merged params of one project.
#[derive(Debug, Default)]
pub(crate) struct Params {
    root: ParamNode,
    /// The type `$params` holds, built on first use.
    php_type: std::sync::OnceLock<Option<PhpType>>,
}

impl Params {
    /// Read and merge the params files of `files`, in order.
    fn load(files: &[(Rank, PathBuf)]) -> Self {
        let mut params = Self::default();
        for (rank, file) in files {
            // A listed file that is missing is one marked optional.
            let mut reading = vec![file.clone()];
            read_params_file(&mut params.root, file, *rank, &mut reading);
        }
        params
    }

    /// The params the project's own params files declare, for tests.
    #[cfg(test)]
    pub(crate) fn from_plan(plan: &ConfigPlan) -> Self {
        Self::load(&plan.params_files)
    }

    /// The key a [`param_key`] spells, or `None` when no file declares it.
    pub(crate) fn get(&self, key: &str) -> Option<&ParamNode> {
        let mut node = &self.root;
        for segment in param_key_segments(key) {
            node = node.child(segment)?;
        }
        Some(node)
    }

    /// The params themselves, under no key.
    pub(crate) fn root(&self) -> &ParamNode {
        &self.root
    }

    /// The type `$params` holds, or `None` when no params were read.
    pub(crate) fn php_type(&self) -> Option<PhpType> {
        self.php_type
            .get_or_init(|| (!self.root.children.is_empty()).then(|| self.root.php_type()))
            .clone()
    }

    /// Whether the key a [`param_key`] spells is declared, or `None` when
    /// that cannot be told: the keys beside it are not all known.  A key
    /// under one no file declares is not judged, since the read of that one
    /// is already wrong.
    pub(crate) fn declares(&self, key: &str) -> Option<bool> {
        let (parent, name) = match key.rsplit_once(KEY_SEPARATOR) {
            Some((parent, name)) => (self.get(parent)?, name),
            None => (&self.root, key),
        };
        parent.keys_known().then(|| parent.child(name).is_some())
    }
}

/// Merge the array the params file at `file` returns into `node`, and
/// whether the file returned one.  `reading` holds the files being read,
/// which a file requiring one of them does not read again.
fn read_params_file(
    node: &mut ParamNode,
    file: &Path,
    rank: Rank,
    reading: &mut Vec<PathBuf>,
) -> bool {
    let Ok(content) = std::fs::read_to_string(file) else {
        return false;
    };
    crate::parser::with_parsed_program(&content, "yii_params", |program, content| {
        let mut use_map = HashMap::new();
        Backend::extract_use_statements_from_statements(
            program.statements.iter(),
            &mut use_map,
            None,
        );
        let source = ParamsSource {
            file,
            content,
            use_map,
            lines: LineIndex::new(content),
        };
        let returned = crate::virtual_members::laravel::array_file::returned_exprs(program);
        let Some(value) = returned.first() else {
            return false;
        };
        merge_value(node, value, &source, rank, reading);
        true
    })
}

/// A params file being read.
struct ParamsSource<'a> {
    file: &'a Path,
    content: &'a str,
    use_map: HashMap<String, String>,
    lines: LineIndex<'a>,
}

/// The elements of an array literal, or `None` for any other value.
fn array_elements<'a>(
    value: &'a Expression<'a>,
) -> Option<&'a TokenSeparatedSequence<'a, ArrayElement<'a>>> {
    match value {
        Expression::Parenthesized(p) => array_elements(p.expression),
        Expression::Array(array) => Some(&array.elements),
        Expression::LegacyArray(array) => Some(&array.elements),
        _ => None,
    }
}

/// Whether `value` merges with the array a key already holds rather than
/// replacing it: an array with keys, or a file required for one.
fn merges(value: &Expression<'_>) -> bool {
    match value {
        Expression::Construct(construct) => required_expr(construct).is_some(),
        value => array_elements(value).is_some_and(|elements| {
            elements
                .iter()
                .any(|element| matches!(element, ArrayElement::KeyValue(_)))
        }),
    }
}

/// Merge `value`, declared in `source`, into `node`.
fn merge_value(
    node: &mut ParamNode,
    value: &Expression<'_>,
    source: &ParamsSource<'_>,
    rank: Rank,
    reading: &mut Vec<PathBuf>,
) {
    if let Expression::Construct(construct) = value
        && let Some(required) = required_expr(construct)
    {
        let Some(path) = source
            .file
            .parent()
            .and_then(|dir| crate::document_links::try_evaluate_path_expr(required, dir))
            .map(|path| crate::document_links::normalize_path(Path::new(&path)))
        else {
            node.open = true;
            return;
        };
        if reading.contains(&path) {
            node.open = true;
            return;
        }
        reading.push(path.clone());
        if !read_params_file(node, &path, rank, reading) {
            node.open = true;
        }
        reading.pop();
        return;
    }
    if !merges(value) {
        node.values.push(value_type(value, source));
        if let Expression::Literal(Literal::String(literal)) = value
            && let Some(text) = literal.value.and_then(crate::atom::literal_bytes_to_str)
        {
            node.strings.push(text.to_string());
        }
        return;
    }
    let Some(elements) = array_elements(value) else {
        return;
    };
    for element in elements.iter() {
        let entry = match element {
            ArrayElement::KeyValue(entry) => entry,
            ArrayElement::Value(_) => continue,
            _ => {
                node.open = true;
                continue;
            }
        };
        let Expression::Literal(Literal::String(key)) = entry.key else {
            if !matches!(entry.key, Expression::Literal(Literal::Integer(_))) {
                node.open = true;
            }
            continue;
        };
        let Some(name) = key.value.and_then(crate::atom::literal_bytes_to_str) else {
            node.open = true;
            continue;
        };
        let declaration = ParamDeclaration {
            file: source.file.to_path_buf(),
            range: Range::new(
                source.lines.position(key.span.start.offset as usize + 1),
                source
                    .lines
                    .position((key.span.end.offset as usize).saturating_sub(1)),
            ),
        };
        let child = node.child_mut(name);
        child.declare(rank, declaration, merges(entry.value));
        merge_value(child, entry.value, source, rank, reading);
    }
}

/// The type of a value that is not an array with keys.  An empty array is
/// any array, since a later file may fill it.
fn value_type(value: &Expression<'_>, source: &ParamsSource<'_>) -> PhpType {
    if array_elements(value).is_some_and(|elements| elements.is_empty()) {
        return PhpType::array();
    }
    crate::virtual_members::laravel::config_values::node_from_expr(
        value,
        source.content,
        &source.use_map,
    )
    .to_php_type()
}

/// The path a `require` or `include` evaluates.
fn required_expr<'a>(construct: &'a Construct<'a>) -> Option<&'a Expression<'a>> {
    match construct {
        Construct::Require(c) => Some(c.value),
        Construct::RequireOnce(c) => Some(c.value),
        Construct::Include(c) => Some(c.value),
        Construct::IncludeOnce(c) => Some(c.value),
        _ => None,
    }
}

impl Backend {
    /// The project's merge plan, read on first use.
    pub(crate) fn yii_config_plan(&self) -> Arc<ConfigPlan> {
        if let Some(plan) = self.yii.config_plan.read().as_ref() {
            return Arc::clone(plan);
        }
        let root = self.workspace.workspace_root.read().clone();
        let plan = Arc::new(root.map(|root| ConfigPlan::load(&root)).unwrap_or_default());
        *self.yii.config_plan.write() = Some(Arc::clone(&plan));
        plan
    }

    /// Whether `yiisoft/config` hands the file at `path` the `$params`.
    pub(crate) fn yii_receives_params(&self, path: &Path) -> bool {
        self.is_yii_project() && self.yii_config_plan().receives_params(path)
    }

    /// The project's merged params, read on first use: the installed
    /// packages' params files, then the project's.
    pub(crate) fn yii_params(&self) -> Arc<Params> {
        let mut known = self.yii.params.lock();
        if let Some(known) = known.as_ref() {
            return Arc::clone(known);
        }
        let mut files = Vec::new();
        let vendors = self.workspace.vendor_dir_paths.lock().clone();
        for vendor in &vendors {
            let Some(installed) = crate::composer::read_installed_packages(vendor, "") else {
                continue;
            };
            for package in &installed.packages {
                if let Some(root) = installed.package_root(package) {
                    files.extend(package_params_files(package, &root));
                }
            }
        }
        files.extend(self.yii_config_plan().params_files.iter().cloned());
        // Stable, so that packages keep their order within a rank.
        files.sort_by_key(|(rank, _)| *rank);
        let params = Arc::new(Params::load(&files));
        *known = Some(Arc::clone(&params));
        params
    }

    /// The type of the `$params` the file at `uri` is handed, or `None`
    /// when it is handed none.
    pub(crate) fn yii_params_type(&self, uri: &str) -> Option<PhpType> {
        if !self.is_yii_project() {
            return None;
        }
        let path = crate::util::uri_to_path(uri)?;
        if !self.yii_receives_params(&path) {
            return None;
        }
        self.yii_params().php_type()
    }

    /// Go-to-definition on a params key: the entries that declare it, of
    /// the files whose value wins.
    pub(crate) fn yii_param_definitions(&self, key: &str) -> Vec<Location> {
        let params = self.yii_params();
        let Some(node) = params.get(key) else {
            return Vec::new();
        };
        node.declarations()
            .iter()
            .filter_map(|declaration| {
                Some(Location {
                    uri: crate::util::path_to_url(&declaration.file)?,
                    range: declaration.range,
                })
            })
            .collect()
    }

    /// The hover detail of a params key: the type of its value, and the
    /// files that declare it.  `None` when none does.
    pub(crate) fn yii_param_detail(&self, key: &str) -> Option<String> {
        let params = self.yii_params();
        let node = params.get(key)?;
        let root = self.workspace.workspace_root.read().clone();
        let shown = |path: &Path| {
            root.as_ref()
                .and_then(|root| path.strip_prefix(root).ok())
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        };
        let mut detail = format!("`{}`", node.php_type());
        for declaration in node.declarations() {
            detail.push_str(&format!("\n\nDeclared in `{}`", shown(&declaration.file)));
        }
        Some(detail)
    }

    /// Forget the params and the merge plan, once a configuration file or
    /// the installed packages changed.
    pub(crate) fn forget_yii_params(&self) {
        *self.yii.config_plan.write() = None;
        *self.yii.params.lock() = None;
    }
}
