//! Where the application's kernel loads its configuration from.
//!
//! `MicroKernelTrait` loads the bundles, packages, services, and routes from
//! the directory `getConfigDir()` returns, `config/` under the project
//! directory unless a kernel of the project overrides it.  A kernel that
//! returns a literal path under its project directory moves the whole of
//! `config/` there, and a file under it is read as the same file under
//! `config/` would be (see [`Backend::symfony_config_uri`]).  A kernel that
//! returns anything else keeps its configuration somewhere this cannot
//! tell, so the names that configuration declares are unknown, and no
//! name of a kind it declares is reported missing (see
//! [`Backend::symfony_config_unread`]).
//!
//! The kernels are the project's subclasses of Symfony's `Kernel`, which
//! the workspace index has parsed by the time it reads the resource files,
//! so they are found then, and again whenever one of their files changes.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::Backend;
use crate::atom::literal_bytes_to_str;
use crate::types::ClassInfo;

/// The class every application kernel extends.
const KERNEL: &str = "Symfony\\Component\\HttpKernel\\Kernel";

/// Where the project's kernels keep their configuration.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct KernelConfig {
    /// The configuration directories that are not `config/`, each with the
    /// `config/` of its project, as URI prefixes ending in `/`.
    moved: Vec<(String, String)>,
    /// Whether a kernel keeps its configuration where this cannot tell.
    unread: bool,
    /// The files the answer was read from, which a change to has to be
    /// read again.
    sources: Vec<String>,
}

/// What a kernel's `getConfigDir()` returns.
#[derive(Debug, PartialEq, Eq)]
enum ConfigDir {
    /// A path, absolute and without `.` or `..` segments.
    Path(PathBuf),
    /// Anything else.
    Unknown,
}

/// The URI prefix of the directory `dir`, ending in `/`.
fn dir_uri_prefix(dir: &Path) -> String {
    let mut uri = crate::util::path_to_uri(dir);
    if !uri.ends_with('/') {
        uri.push('/');
    }
    uri
}

/// `path` without its `.` and `..` segments, which a path a kernel builds
/// by concatenation (`__DIR__.'/../etc'`) has.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

/// The project directory of the kernel declared in `kernel_file`: the
/// nearest directory above it holding a `composer.json`, which is where
/// Symfony's `Kernel::getProjectDir()` looks.
fn project_dir(kernel_file: &Path) -> Option<PathBuf> {
    kernel_file
        .ancestors()
        .skip(1)
        .find(|dir| dir.join("composer.json").is_file())
        .map(Path::to_path_buf)
}

/// Finds the `getConfigDir()` declared at a given offset, and reads what it
/// returns.
struct ConfigDirMethod<'a> {
    name_offset: u32,
    /// The kernel's project directory, or `None` when the kernel overrides
    /// `getProjectDir()` or has no `composer.json` above it.
    project_dir: Option<&'a Path>,
    /// The directory of the file the method is declared in, for `__DIR__`.
    file_dir: &'a Path,
}

impl<'ast, 'arena> Walker<'ast, 'arena, Option<ConfigDir>> for ConfigDirMethod<'_> {
    fn walk_in_method(&self, node: &'ast Method<'arena>, found: &mut Option<ConfigDir>) {
        if node.name.span.start.offset != self.name_offset {
            return;
        }
        let MethodBody::Concrete(body) = &node.body else {
            return;
        };
        // Only a body that is one `return` says what it returns.
        let returned = match body.statements.as_slice() {
            [
                Statement::Return(Return {
                    value: Some(value), ..
                }),
            ] => self.path(value),
            _ => None,
        };
        *found = Some(returned.map_or(ConfigDir::Unknown, |path| {
            ConfigDir::Path(normalize(Path::new(&path)))
        }));
    }
}

impl ConfigDirMethod<'_> {
    /// The path `expression` builds: string literals, `__DIR__` and
    /// `dirname()` of it, and `$this->getProjectDir()`, concatenated.
    fn path(&self, expression: &Expression<'_>) -> Option<String> {
        match expression {
            Expression::Binary(binary) if binary.operator.is_concatenation() => {
                Some(self.path(binary.lhs)? + &self.path(binary.rhs)?)
            }
            Expression::Parenthesized(inner) => self.path(inner.expression),
            Expression::Literal(Literal::String(literal)) => {
                Some(literal_bytes_to_str(literal.value?)?.to_string())
            }
            Expression::Call(Call::Method(call))
                if matches!(call.object, Expression::Variable(Variable::Direct(this)) if this.name == b"$this")
                    && matches!(&call.method, ClassLikeMemberSelector::Identifier(name) if name.value.eq_ignore_ascii_case(b"getProjectDir"))
                    && call.argument_list.arguments.is_empty() =>
            {
                Some(self.project_dir?.to_string_lossy().into_owned())
            }
            Expression::MagicConstant(MagicConstant::Directory(_)) | Expression::Call(_) => {
                crate::document_links::try_evaluate_path_expr(expression, self.file_dir)
            }
            _ => None,
        }
    }
}

impl Backend {
    /// `uri` as the Symfony support reads it: a file under a configuration
    /// directory a kernel moved is read as the same file under `config/`.
    pub(crate) fn symfony_config_uri<'a>(&self, uri: &'a str) -> Cow<'a, str> {
        let config = self.symfony.kernel_config.read();
        for (moved, config_dir) in &config.moved {
            if let Some(rest) = uri.strip_prefix(moved.as_str()) {
                return Cow::Owned(format!("{config_dir}{rest}"));
            }
        }
        Cow::Borrowed(uri)
    }

    /// Whether a kernel keeps its configuration where it cannot be read, so
    /// that the routes, services, parameters, and Twig setup it declares
    /// are unknown.
    pub(crate) fn symfony_config_unread(&self) -> bool {
        self.symfony.kernel_config.read().unread
    }

    /// The configuration directory of the project at `root`, or `None` when
    /// a kernel keeps it where it cannot be read.
    pub(crate) fn symfony_config_dir(&self, root: &Path) -> Option<PathBuf> {
        let config = self.symfony.kernel_config.read();
        if config.unread {
            return None;
        }
        let config_dir = root.join("config");
        let config_prefix = dir_uri_prefix(&config_dir);
        let moved = config
            .moved
            .iter()
            .find(|(_, config)| *config == config_prefix)
            .and_then(|(moved, _)| crate::util::uri_to_path(moved));
        Some(moved.unwrap_or(config_dir))
    }

    /// Whether the PHP file at `uri` is one the kernels' configuration
    /// directories were read from.
    pub(crate) fn is_symfony_kernel_source(&self, uri: &str) -> bool {
        self.symfony
            .kernel_config
            .read()
            .sources
            .iter()
            .any(|source| source == uri)
    }

    /// Read again where the project's kernels keep their configuration,
    /// and when that moved, read the resource files under the old and the
    /// new directories again.
    pub(crate) fn refresh_symfony_kernel_config(&self) {
        if !self.is_symfony_project() {
            return;
        }
        let found = self.find_kernel_config();
        let before = {
            let mut config = self.symfony.kernel_config.write();
            if *config == found {
                return;
            }
            std::mem::replace(&mut *config, found)
        };
        let after = self.symfony.kernel_config.read();
        let mut dirs: Vec<&str> = before
            .moved
            .iter()
            .chain(&after.moved)
            .flat_map(|(moved, config)| [moved.as_str(), config.as_str()])
            .collect();
        dirs.sort_unstable();
        dirs.dedup();
        let dirs: Vec<PathBuf> = dirs
            .into_iter()
            .filter_map(crate::util::uri_to_path)
            .collect();
        drop(after);
        self.forget_symfony_judged_translation_domains();
        for dir in dirs {
            self.reindex_indexed_resources_under(&dir);
        }
    }

    /// Publish again the resource files under `dir` that are indexed
    /// already: the rest are read by the walk that indexes them.
    fn reindex_indexed_resources_under(&self, dir: &Path) {
        let mut pending = vec![dir.to_path_buf()];
        while let Some(dir) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
                    pending.push(path);
                    continue;
                }
                let uri = crate::util::path_to_uri(&path);
                if !self.is_resource_uri(&uri) || self.symbol_map_for(&uri).is_none() {
                    continue;
                }
                if let Some(content) = self.get_file_content_arc(&uri) {
                    self.publish_resource_symbol_index(&uri, &content);
                }
            }
        }
    }

    /// Where the project's kernels keep their configuration, by the
    /// `getConfigDir()` each declares, itself or through a trait of the
    /// project.
    fn find_kernel_config(&self) -> KernelConfig {
        let mut config = KernelConfig::default();
        let vendor_prefixes = self.workspace.vendor_uri_prefixes.lock().clone();
        let is_project_uri = |uri: &str| {
            !vendor_prefixes
                .iter()
                .any(|prefix| uri.starts_with(prefix.as_str()))
                && !crate::stubs::is_stub_uri(uri)
                && uri.starts_with("file:")
        };
        let mut kernels = self.gti_descendants(KERNEL, false);
        kernels.sort_unstable();
        for fqn in kernels {
            let Some(kernel_uri) = self.class_declaration_uri(&fqn) else {
                continue;
            };
            if !is_project_uri(&kernel_uri) {
                continue;
            }
            let Some(kernel) = self.find_or_load_class(&fqn) else {
                continue;
            };
            let Some(kernel_file) = crate::util::uri_to_path(&kernel_uri) else {
                continue;
            };
            let overrides_project_dir = declares(&kernel, "getProjectDir");
            let project_dir = if overrides_project_dir {
                None
            } else {
                project_dir(&kernel_file)
            };

            // The kernel's own method comes before a trait's.
            let traits = kernel_traits(self, &kernel, &is_project_uri);
            // A change to any of these files can move the configuration.
            config.sources.push(kernel_uri.clone());
            config
                .sources
                .extend(traits.iter().map(|(_, uri)| uri.clone()));
            let declaring = std::iter::once((kernel, kernel_uri))
                .chain(traits)
                .find(|(class, _)| declares(class, "getConfigDir"));
            let Some((class, uri)) = declaring else {
                continue;
            };
            let found = self.read_config_dir(&class, &uri, project_dir.as_deref());
            match (found, &project_dir) {
                (Some(ConfigDir::Path(dir)), Some(root)) if dir.starts_with(root) => {
                    let config_dir = root.join("config");
                    if dir != config_dir {
                        let moved = (dir_uri_prefix(&dir), dir_uri_prefix(&config_dir));
                        if !config.moved.contains(&moved) {
                            config.moved.push(moved);
                        }
                    }
                }
                _ => config.unread = true,
            }
        }
        config.sources.sort_unstable();
        config.sources.dedup();
        config
    }

    /// What the `getConfigDir()` `class` declares in the file at `uri`
    /// returns.
    fn read_config_dir(
        &self,
        class: &ClassInfo,
        uri: &str,
        project_dir: Option<&Path>,
    ) -> Option<ConfigDir> {
        let method = class
            .methods
            .iter()
            .find(|method| method.name.eq_ignore_ascii_case("getConfigDir"))?;
        let content = self.get_file_content_arc(uri)?;
        let file = crate::util::uri_to_path(uri)?;
        let walker = ConfigDirMethod {
            name_offset: method.name_offset,
            project_dir,
            file_dir: file.parent()?,
        };
        crate::parser::with_parsed_program(&content, "read_config_dir", |program, _| {
            let mut found = None;
            walker.walk_program(program, &mut found);
            found
        })
    }
}

/// Whether `class` declares a method named `name` itself.
fn declares(class: &ClassInfo, name: &str) -> bool {
    class
        .methods
        .iter()
        .any(|method| method.name.eq_ignore_ascii_case(name))
}

/// The traits of the project `kernel` uses, with their files.
fn kernel_traits(
    backend: &Backend,
    kernel: &ClassInfo,
    is_project_uri: &impl Fn(&str) -> bool,
) -> Vec<(std::sync::Arc<ClassInfo>, String)> {
    kernel
        .used_traits
        .iter()
        .filter_map(|name| {
            let uri = backend.class_declaration_uri(name)?;
            if !is_project_uri(&uri) {
                return None;
            }
            Some((backend.find_or_load_class(name)?, uri))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_concatenated_path_loses_its_dot_segments() {
        assert_eq!(
            normalize(Path::new("/p/src/../etc/./symfony")),
            PathBuf::from("/p/etc/symfony")
        );
    }
}
