//! Yii path aliases (`yiisoft/aliases`): the `@root`, `@views`, … names a
//! path in a string can start with.
//!
//! An application declares them in its params, under
//! `params['yiisoft/aliases']['aliases']`, usually by requiring a file of
//! their own (`config/common/aliases.php` in `yiisoft/app`).  They are read
//! statically from the params files under `config/`: each one that names
//! `yiisoft/aliases` is parsed, and the array it gives the key, written in
//! place or returned by the file it requires, is the aliases.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mago_syntax::cst::*;

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
        }
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
                                    array => self.read_alias_array(array, dir),
                                }
                            }
                        },
                    );
                }
                required
            });
        if let Some(required) = required
            && let Ok(content) = std::fs::read_to_string(&required)
            && let Some(dir) = required.parent()
        {
            crate::parser::with_parsed_program(&content, "yii_aliases", |program, _| {
                for returned in crate::virtual_members::laravel::array_file::returned_exprs(program)
                {
                    self.read_alias_array(returned, dir);
                }
            });
        }
    }

    /// Record the `'@name' => path` entries of an array written in a file
    /// in `dir`.
    fn read_alias_array(&mut self, array: &Expression<'_>, dir: &Path) {
        let elements = match array {
            Expression::Array(array) => &array.elements,
            Expression::LegacyArray(array) => &array.elements,
            _ => return,
        };
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
            // Yii adds the `@` a name is declared without, and drops the
            // separator a path ends in.
            let name = if name.starts_with('@') {
                name.to_string()
            } else {
                format!("@{name}")
            };
            self.entries
                .insert(name, path.trim_end_matches(['/', '\\']).to_string());
        }
    }
}

/// The path an alias is declared with: a string, which may start with
/// another alias, or a path built from `__DIR__` and `dirname()`.
fn alias_path(value: &Expression<'_>, dir: &Path) -> Option<String> {
    if let Expression::Literal(Literal::String(literal)) = value
        && let Some(text) = literal.value.and_then(crate::atom::literal_bytes_to_str)
        && (text.starts_with('@') || text.starts_with('/'))
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
