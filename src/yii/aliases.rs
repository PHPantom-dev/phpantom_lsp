//! Yii path aliases (`yiisoft/aliases`): the `@root`, `@views`, … names a
//! path in a string can start with.
//!
//! An application declares them in its params, under
//! `params['yiisoft/aliases']['aliases']`, usually by requiring a file of
//! their own (`config/common/aliases.php` in `yiisoft/app`).  They are read
//! statically from the params files under `config/`: each one that names
//! `yiisoft/aliases` is parsed, and the array it gives the key, written in
//! place or returned by the file it requires, is the aliases.  Where each
//! one is declared is kept beside the path it names, for navigation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{FoldingRange, Location, Range};

use crate::Backend;

/// How deep under `config/` a params file is looked for.  `yiisoft/app`
/// keeps them two levels down (`config/common/params.php`,
/// `config/environments/dev/params.php`).
const MAX_CONFIG_DEPTH: usize = 4;

/// The params key the aliases are declared under.
const ALIASES_PARAM: &str = "yiisoft/aliases";

/// The aliases one project declares, each by its name (`@views`, or a
/// nested `@views/admin`) to the path it was declared with: an absolute
/// path, or one starting with another alias.
#[derive(Debug, Default)]
pub(crate) struct Aliases {
    entries: HashMap<String, String>,
    declarations: HashMap<String, AliasDeclaration>,
}

/// Where an alias is declared: the file, and the range of its name's string
/// there, inside the quotes.
#[derive(Debug, Clone)]
pub(crate) struct AliasDeclaration {
    pub(crate) file: PathBuf,
    pub(crate) range: Range,
}

impl Aliases {
    /// Read the aliases the params files under `root`'s `config/` declare.
    pub(crate) fn load(root: &Path) -> Self {
        let mut files = Vec::new();
        collect_php_files(&root.join("config"), 0, &mut files);
        // The walk order is the filesystem's; a stable one keeps the answer
        // the same between runs when two files declare one alias.
        files.sort_unstable();
        let mut aliases = Self::default();
        for file in files {
            let Ok(content) = std::fs::read_to_string(&file) else {
                continue;
            };
            if !content.contains(ALIASES_PARAM) {
                continue;
            }
            aliases.read_params_file(&file, &content);
        }
        aliases
    }

    /// An alias table holding `entries`, for tests.
    #[cfg(test)]
    pub(crate) fn from_entries(entries: &[(&str, &str)]) -> Self {
        Self {
            entries: entries
                .iter()
                .map(|(name, path)| (name.to_string(), path.to_string()))
                .collect(),
            declarations: HashMap::new(),
        }
    }

    /// The names of the declared aliases, sorted.
    pub(crate) fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.entries.keys().cloned().collect();
        names.sort_unstable();
        names
    }

    /// The path the alias `name` was declared with, as written.
    pub(crate) fn declared_path(&self, name: &str) -> Option<&str> {
        self.entries.get(name).map(String::as_str)
    }

    /// Every declared alias, by name, with where it is declared.
    pub(crate) fn declarations(&self) -> impl Iterator<Item = (&str, &AliasDeclaration)> {
        self.declarations
            .iter()
            .map(|(name, declaration)| (name.as_str(), declaration))
    }

    /// Where the alias `name` is declared.
    pub(crate) fn declaration(&self, name: &str) -> Option<&AliasDeclaration> {
        self.declarations.get(name)
    }

    /// The path `path` names once its alias is replaced, as Yii's
    /// `Aliases::get()` replaces it: by the longest declared alias it
    /// starts with, then by whatever alias that one's path starts with.
    /// A path with no alias is itself; one whose alias the project does
    /// not declare names nothing that can be told.
    pub(crate) fn resolve(&self, path: &str) -> Option<String> {
        let mut path = path.to_string();
        // Each alias is replaced at most once, so a cycle ends.
        let mut replaced: Vec<&str> = Vec::new();
        while path.starts_with('@') {
            let (alias, value) = self.longest_alias(&path)?;
            if replaced.contains(&alias) {
                return None;
            }
            replaced.push(alias);
            path = format!("{value}{}", &path[alias.len()..]);
        }
        Some(path)
    }

    /// The longest declared alias `path` starts with, as a whole segment.
    fn longest_alias(&self, path: &str) -> Option<(&str, &str)> {
        self.entries
            .iter()
            .filter(|(alias, _)| {
                path.strip_prefix(alias.as_str())
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
            })
            .max_by_key(|(alias, _)| alias.len())
            .map(|(alias, value)| (alias.as_str(), value.as_str()))
    }

    /// Read the aliases one params file gives `yiisoft/aliases`.
    fn read_params_file(&mut self, file: &Path, content: &str) {
        let Some(dir) = file.parent() else {
            return;
        };
        let required =
            crate::parser::with_parsed_program(content, "yii_alias_params", |program, content| {
                let mut required = None;
                for returned in crate::virtual_members::laravel::array_file::returned_exprs(program)
                {
                    crate::virtual_members::laravel::array_file::for_each_entry(
                        returned,
                        content,
                        &mut |path, _, _, value| {
                            if path.len() == 2 && path[0] == ALIASES_PARAM && path[1] == "aliases" {
                                match value {
                                    Expression::Construct(construct) => {
                                        required = required_path(construct, dir);
                                    }
                                    array => self.read_alias_array(array, file, content),
                                }
                            }
                        },
                    );
                }
                required
            });
        if let Some(required) = required
            && let Ok(content) = std::fs::read_to_string(&required)
        {
            crate::parser::with_parsed_program(&content, "yii_aliases", |program, content| {
                for returned in crate::virtual_members::laravel::array_file::returned_exprs(program)
                {
                    self.read_alias_array(returned, &required, content);
                }
            });
        }
    }

    /// Record the `'@name' => path` entries of an array written in `file`,
    /// whose text is `content`.
    fn read_alias_array(&mut self, array: &Expression<'_>, file: &Path, content: &str) {
        let Some(dir) = file.parent() else {
            return;
        };
        let elements = match array {
            Expression::Array(array) => &array.elements,
            Expression::LegacyArray(array) => &array.elements,
            _ => return,
        };
        let mut lines = None;
        for element in elements.iter() {
            let ArrayElement::KeyValue(entry) = element else {
                continue;
            };
            let Expression::Literal(Literal::String(key)) = entry.key else {
                continue;
            };
            let Some(name) = key.value.and_then(crate::atom::literal_bytes_to_str) else {
                continue;
            };
            let Some(path) = alias_path(entry.value, dir) else {
                continue;
            };
            let lines = lines.get_or_insert_with(|| crate::text_position::LineIndex::new(content));
            let range = Range::new(
                lines.position(key.span.start.offset as usize + 1),
                lines.position((key.span.end.offset as usize).saturating_sub(1)),
            );
            // Yii adds the `@` a name is declared without, and drops the
            // separator a path ends in.
            let name = if name.starts_with('@') {
                name.to_string()
            } else {
                format!("@{name}")
            };
            self.declarations.insert(
                name.clone(),
                AliasDeclaration {
                    file: file.to_path_buf(),
                    range,
                },
            );
            self.entries
                .insert(name, path.trim_end_matches(['/', '\\']).to_string());
        }
    }
}

impl Backend {
    /// Go-to-definition on an alias: the string that declares it.
    pub(crate) fn yii_alias_definitions(&self, name: &str) -> Vec<Location> {
        self.yii_aliases()
            .declaration(name)
            .and_then(|declaration| {
                Some(Location {
                    uri: crate::util::path_to_url(&declaration.file)?,
                    range: declaration.range,
                })
            })
            .into_iter()
            .collect()
    }

    /// The hover detail of a declared alias: the path it is declared with,
    /// and, when that starts with another alias, the path it resolves to.
    /// `None` when the project does not declare it.
    pub(crate) fn yii_alias_detail(&self, name: &str) -> Option<String> {
        let aliases = self.yii_aliases();
        let declared = aliases.declared_path(name)?;
        let root = self.workspace.workspace_root.read().clone();
        let mut detail = format!("`{}`", shown_path(declared, root.as_deref()));
        if declared.starts_with('@')
            && let Some(resolved) = aliases.resolve(declared)
        {
            detail.push_str(&format!(
                ", which is `{}`",
                shown_path(&resolved, root.as_deref())
            ));
        }
        Some(detail)
    }
}

impl Backend {
    /// The strings in `uri` that start with an alias, each as the byte
    /// offsets of its text's start, of the alias's end, and of the text's
    /// end.  A string with an escape or an interpolation, whose text is not
    /// the path it names, is left out.
    fn yii_alias_strings(&self, uri: &str, content: &str) -> Vec<(usize, usize, usize)> {
        if !self.is_yii_project() {
            return Vec::new();
        }
        let Some(map) = self.symbol_map_for(uri) else {
            return Vec::new();
        };
        if !map.matches_source(content) {
            return Vec::new();
        }
        map.spans
            .iter()
            .filter_map(|span| {
                let crate::symbol_map::SymbolKind::StringKey {
                    kind: crate::symbol_map::StringKeyKind::YiiAlias,
                    ..
                } = &span.kind
                else {
                    return None;
                };
                let (start, alias_end) = (span.start as usize, span.end as usize);
                let end = string_end(content, start, alias_end)?;
                Some((start, alias_end, end))
            })
            .collect()
    }

    /// The document links of the strings in `uri` that start with an alias
    /// and go on to name a file or directory in the project: each covers
    /// the part after the alias, which the alias itself leaves to
    /// go-to-definition, and leads to what the whole string names.
    pub(crate) fn yii_alias_links(&self, uri: &str, content: &str) -> Vec<(usize, usize, PathBuf)> {
        let strings = self.yii_alias_strings(uri, content);
        if strings.is_empty() {
            return Vec::new();
        }
        let Some(root) = self.workspace.workspace_root.read().clone() else {
            return Vec::new();
        };
        let aliases = self.yii_aliases();
        let mut links = Vec::new();
        for (start, alias_end, end) in strings {
            if end == alias_end {
                continue;
            }
            let Some(resolved) = aliases.resolve(&content[start..end]) else {
                continue;
            };
            let path = crate::document_links::normalize_path(Path::new(&resolved));
            if path.starts_with(&root) && path.exists() {
                links.push((alias_end, end, path));
            }
        }
        links
    }

    /// A folding range over the text of each string in `uri` that starts
    /// with an alias the project declares, collapsing to the path the
    /// string resolves to.  The range lies within one line, so it is only
    /// of use to a client that folds part of a line and shows the
    /// collapsed text.
    pub(crate) fn yii_alias_folds(&self, uri: &str, content: &str) -> Vec<FoldingRange> {
        let strings = self.yii_alias_strings(uri, content);
        if strings.is_empty() {
            return Vec::new();
        }
        let aliases = self.yii_aliases();
        let root = self.workspace.workspace_root.read().clone();
        let lines = crate::text_position::LineIndex::new(content);
        strings
            .into_iter()
            .filter_map(|(start, _, end)| {
                let resolved = aliases.resolve(&content[start..end])?;
                let (start, end) = (lines.position(start), lines.position(end));
                Some(FoldingRange {
                    start_line: start.line,
                    start_character: Some(start.character),
                    end_line: end.line,
                    end_character: Some(end.character),
                    kind: None,
                    collapsed_text: Some(shown_path(&resolved, root.as_deref())),
                })
            })
            .collect()
    }
}

/// `path` as it is shown to the user: relative to the workspace `root`
/// when it lies inside it, as it is otherwise.
fn shown_path(path: &str, root: Option<&Path>) -> String {
    root.and_then(|root| {
        Path::new(path)
            .strip_prefix(root)
            .ok()
            .filter(|relative| !relative.as_os_str().is_empty())
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
    })
    .unwrap_or_else(|| path.to_string())
}

/// Where the string whose text starts at `start` ends, read on from
/// `from`: its closing quote.  `None` for a string with an escape or, in
/// double quotes, an interpolation, whose text is not the path it names.
fn string_end(content: &str, start: usize, from: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let quote = *bytes.get(start.checked_sub(1)?)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let len = bytes.get(from..)?.iter().position(|&b| b == quote)?;
    let text = &bytes[from..from + len];
    if text.contains(&b'\\') || (quote == b'"' && text.contains(&b'$')) {
        return None;
    }
    Some(from + len)
}

/// The alias the text of a string starts with: an `@`, and the name up to
/// the first `/`.  A name is letters, digits, `_`, `-`, and `.`, which
/// leaves out an `@` that starts something other than a path.
pub(crate) fn alias_prefix(text: &str) -> Option<&str> {
    let name = text.strip_prefix('@')?;
    let len = name.find('/').unwrap_or(name.len());
    let valid = len > 0
        && name.as_bytes()[..len]
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    valid.then(|| &text[..len + 1])
}

/// The path an alias is declared with: a string, which may start with
/// another alias, or a path built from `__DIR__` and `dirname()`.
fn alias_path(value: &Expression<'_>, dir: &Path) -> Option<String> {
    if let Expression::Literal(Literal::String(literal)) = value
        && let Some(text) = literal.value.and_then(crate::atom::literal_bytes_to_str)
        && (text.starts_with('@') || super::views::is_absolute_view_path(text))
    {
        return Some(text.to_string());
    }
    let path = crate::document_links::try_evaluate_path_expr(value, dir)?;
    Some(
        crate::document_links::normalize_path(Path::new(&path))
            .to_string_lossy()
            .into_owned(),
    )
}

/// The file a `require` of the aliases evaluates to.
fn required_path(construct: &construct::Construct<'_>, dir: &Path) -> Option<PathBuf> {
    let value = match construct {
        construct::Construct::Require(c) => c.value,
        construct::Construct::RequireOnce(c) => c.value,
        construct::Construct::Include(c) => c.value,
        construct::Construct::IncludeOnce(c) => c.value,
        _ => return None,
    };
    let path = crate::document_links::try_evaluate_path_expr(value, dir)?;
    Some(crate::document_links::normalize_path(Path::new(&path)))
}

fn collect_php_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_CONFIG_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_php_files(&path, depth + 1, out);
        } else if path.extension().is_some_and(|ext| ext == "php") {
            out.push(path);
        }
    }
}
