//! The project's PSR-4 mappings, and resolving a class or a namespace to
//! the file or directory a mapping places it in, and a file back to the
//! class it should declare.

use std::path::{Path, PathBuf};

use super::ComposerPackage;

/// A single PSR-4 namespace-to-directory mapping.
///
/// PSR-4 mappings are sourced exclusively from the project's own
/// `composer.json` (`autoload` and `autoload-dev` sections).  Vendor
/// packages are not included — the Composer classmap is the sole source
/// of truth for vendor code.
#[derive(Debug, Clone)]
pub struct Psr4Mapping {
    /// The namespace prefix, always ending with `\` (e.g. `"Klarna\"`).
    pub prefix: String,
    /// The base directory path relative to the workspace root (e.g. `"src/Klarna/"`).
    pub base_path: String,
}

/// Extract all PSR-4 autoload mappings from both `autoload` and
/// `autoload-dev` sections of a parsed [`ComposerPackage`].
///
/// Returns the mappings sorted by prefix length descending so that
/// longest-prefix-first matching works correctly.
pub fn extract_psr4_mappings_from_package(package: &ComposerPackage) -> Vec<Psr4Mapping> {
    let mut mappings = Vec::new();

    if let Some(autoload) = &package.autoload {
        for (prefix, paths) in &autoload.psr_4 {
            collect_psr4_entries(prefix, paths, &mut mappings);
        }
    }

    if let Some(autoload_dev) = &package.autoload_dev {
        for (prefix, paths) in &autoload_dev.psr_4 {
            collect_psr4_entries(prefix, paths, &mut mappings);
        }
    }

    // Sort by prefix length descending so longest-prefix-first matching works
    mappings.sort_by_key(|b| std::cmp::Reverse(b.prefix.len()));

    mappings
}

/// Directories that `build_self_scan_composer` needs from a parsed
/// `composer.json`.  Returned by [`extract_scan_dirs`].
pub struct ScanDirs {
    /// `(normalised_prefix, directory_path_string)` pairs from both
    /// `autoload.psr-4` and `autoload-dev.psr-4`.
    pub psr4: Vec<(String, String)>,
    /// Directory/file paths from both `autoload.classmap` and
    /// `autoload-dev.classmap`.
    pub classmap: Vec<String>,
}

/// Extract the PSR-4 and classmap directory lists from a
/// [`ComposerPackage`] for workspace scanning.
///
/// This keeps mago types private to this module — callers receive
/// plain strings and don't need to depend on `mago_composer`.
pub fn extract_scan_dirs(package: &ComposerPackage) -> ScanDirs {
    let mut psr4 = Vec::new();
    let mut classmap = Vec::new();

    if let Some(autoload) = &package.autoload {
        for (prefix, paths) in &autoload.psr_4 {
            let normalised = normalise_prefix(prefix);
            paths.for_each_path(|dir| {
                psr4.push((normalised.clone(), dir.to_owned()));
            });
        }
        classmap.extend(autoload.classmap.iter().cloned());
    }

    if let Some(autoload_dev) = &package.autoload_dev {
        for (prefix, paths) in &autoload_dev.psr_4 {
            let normalised = normalise_prefix(prefix);
            paths.for_each_path(|dir| {
                psr4.push((normalised.clone(), dir.to_owned()));
            });
        }
        classmap.extend(autoload_dev.classmap.iter().cloned());
    }

    ScanDirs { psr4, classmap }
}

// ── PSR-4 path abstraction ─────────────────────────────────────────
//
// `mago-composer` emits two structurally identical but nominally
// distinct enums for `autoload.psr-4` and `autoload-dev.psr-4`.
// The trait + macro below erase the difference so a single generic
// function handles both.

/// Uniform access to the path strings inside a PSR-4 value, regardless
/// of whether it came from `autoload` or `autoload-dev`.
trait Psr4Paths {
    /// Call `f` for each directory path in this value.
    fn for_each_path(&self, f: impl FnMut(&str));
}

macro_rules! impl_psr4_paths {
    ($ty:ty) => {
        impl Psr4Paths for $ty {
            fn for_each_path(&self, mut f: impl FnMut(&str)) {
                match self {
                    Self::String(s) => f(s),
                    Self::Array(arr) => arr.iter().for_each(|s| f(s)),
                }
            }
        }
    };
}

impl_psr4_paths!(mago_composer::AutoloadPsr4value);
impl_psr4_paths!(mago_composer::ComposerPackageAutoloadDevPsr4value);

/// Normalise a PSR-4 namespace prefix: ensure it ends with `\` unless
/// it is the empty root-namespace prefix.
pub(super) fn normalise_prefix(prefix: &str) -> String {
    if prefix.ends_with('\\') {
        prefix.to_string()
    } else if prefix.is_empty() {
        // Empty prefix means "fallback" / root namespace
        String::new()
    } else {
        format!("{}\\", prefix)
    }
}

/// Collect PSR-4 entries from any value that implements [`Psr4Paths`].
fn collect_psr4_entries<P: Psr4Paths>(prefix: &str, paths: &P, mappings: &mut Vec<Psr4Mapping>) {
    let normalised_prefix = normalise_prefix(prefix);
    paths.for_each_path(|path| {
        mappings.push(Psr4Mapping {
            prefix: normalised_prefix.clone(),
            base_path: normalise_path(path),
        });
    });
}

/// Normalise a directory path: ensure it uses forward slashes and ends with `/`.
pub fn normalise_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    if p.ends_with('/') || p.is_empty() {
        p
    } else {
        format!("{}/", p)
    }
}

/// The file each PSR-4 mapping that covers `class_name` would place it
/// at, in mapping order (longest prefix first).
///
/// An empty prefix is the root fallback mapping and covers every class.
fn psr4_candidate_paths<'a>(
    mappings: &'a [Psr4Mapping],
    workspace_root: &'a Path,
    class_name: &'a str,
) -> impl Iterator<Item = PathBuf> + 'a {
    let class_name = class_name.strip_prefix('\\').unwrap_or(class_name);
    mappings.iter().filter_map(move |mapping| {
        let relative = if mapping.prefix.is_empty() {
            class_name
        } else {
            class_name.strip_prefix(&mapping.prefix)?
        };
        Some(
            workspace_root
                .join(&mapping.base_path)
                .join(format!("{}.php", relative.replace('\\', "/"))),
        )
    })
}

/// Where PSR-4 places `class_name`, whether or not the file exists yet:
/// the path a class moved or created under that name has to take.
pub(crate) fn psr4_path_for_class(
    mappings: &[Psr4Mapping],
    workspace_root: &Path,
    class_name: &str,
) -> Option<PathBuf> {
    psr4_candidate_paths(mappings, workspace_root, class_name).next()
}

/// Resolve a fully-qualified PHP class name to a file path using PSR-4 mappings.
///
/// The `class_name` should be the namespace-qualified name (e.g.
/// `"Klarna\\Customer"` or `"Klarna\\Rest\\Order"`). A leading `\` is
/// stripped if present (PHP fully-qualified syntax).
///
/// Returns the first path that exists on disk, or `None` if no mapping
/// matches or the resolved file doesn't exist.
pub fn resolve_class_path(
    mappings: &[Psr4Mapping],
    workspace_root: &Path,
    class_name: &str,
) -> Option<PathBuf> {
    psr4_class_files(mappings, workspace_root, class_name).next()
}

/// Every existing file PSR-4 places `class_name` at, in mapping order:
/// [`resolve_class_path`]'s answer first, then the ones a longer prefix
/// shadowed.
pub(crate) fn psr4_class_files<'a>(
    mappings: &'a [Psr4Mapping],
    workspace_root: &'a Path,
    class_name: &'a str,
) -> impl Iterator<Item = PathBuf> + 'a {
    // Skip built-in type keywords that are never real classes
    (!crate::php_type::is_keyword_type(class_name))
        .then(|| psr4_candidate_paths(mappings, workspace_root, class_name))
        .into_iter()
        .flatten()
        .filter(|path| path.is_file())
}

/// The part of `namespace` that lies below a PSR-4 mapping's prefix, or
/// `None` when the mapping does not cover the namespace at all.
///
/// `""` means the namespace *is* the mapping's root. Matching is
/// ASCII-case-insensitive, which is how PHP compares namespace segments,
/// and byte lengths are preserved so the remainder can be sliced off the
/// original spelling.
fn namespace_below_prefix<'a>(namespace: &'a str, mapping_prefix: &str) -> Option<&'a str> {
    let prefix = mapping_prefix.trim_end_matches('\\');
    if prefix.is_empty() {
        // A root fallback mapping (`"": "src/"`) covers every namespace.
        return Some(namespace);
    }
    if !namespace.get(..prefix.len())?.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let rest = &namespace[prefix.len()..];
    if rest.is_empty() {
        return Some(rest);
    }
    rest.strip_prefix('\\')
}

/// Every directory PSR-4 places `namespace` in, paired with the mapping
/// that puts it there.
///
/// Composer accepts an array of directories per prefix, so one namespace
/// can be spread over several roots and each of them holds part of it.
/// `mappings` is assumed to be sorted longest-prefix-first, so the most
/// specific mapping comes first and a root fallback comes last.
pub(crate) fn psr4_directories_for_namespace<'a>(
    mappings: &'a [Psr4Mapping],
    workspace_root: &'a Path,
    namespace: &'a str,
) -> impl Iterator<Item = (&'a Psr4Mapping, PathBuf)> + 'a {
    mappings.iter().filter_map(move |mapping| {
        let relative = namespace_below_prefix(namespace, &mapping.prefix)?;
        let base = workspace_root.join(&mapping.base_path);
        Some((
            mapping,
            if relative.is_empty() {
                base
            } else {
                base.join(relative.replace('\\', std::path::MAIN_SEPARATOR_STR))
            },
        ))
    })
}

/// The directory PSR-4 places `namespace` in, or `None` when no mapping
/// covers it.
///
/// The counterpart of [`resolve_namespace_from_path`], and the namespace
/// equivalent of [`resolve_class_path`]: it answers where a namespace's
/// files have to live for the autoloader to find them. Where several
/// mappings cover the namespace the most specific one wins; use
/// [`psr4_directories_for_namespace`] to see all of them.
pub(crate) fn psr4_directory_for_namespace(
    mappings: &[Psr4Mapping],
    workspace_root: &Path,
    namespace: &str,
) -> Option<PathBuf> {
    psr4_directories_for_namespace(mappings, workspace_root, namespace)
        .next()
        .map(|(_, directory)| directory)
}

/// Reverse of [`resolve_class_path`]: given a file path, compute the
/// expected PSR-4 namespace and class name.
///
/// Returns `Some((namespace, class_name))` where `namespace` is the
/// expected namespace (e.g. `"App\\Models"`) and `class_name` is the
/// expected short class name (the filename stem, e.g. `"User"`).
///
/// Returns `None` if the file doesn't end in `.php` or doesn't fall
/// under any PSR-4 mapping.
pub fn resolve_namespace_from_path(
    mappings: &[Psr4Mapping],
    workspace_root: &Path,
    file_path: &Path,
) -> Option<(Option<String>, String)> {
    if file_path.extension().and_then(|e| e.to_str()) != Some("php") {
        return None;
    }

    let file_unix = file_path.to_str()?.replace('\\', "/");

    let mut best: Option<(&Psr4Mapping, String)> = None;

    for mapping in mappings {
        let abs_base = workspace_root.join(&mapping.base_path);
        let abs_base_str = abs_base.to_str()?.replace('\\', "/");
        let abs_base_slash = if abs_base_str.ends_with('/') {
            abs_base_str
        } else {
            format!("{}/", abs_base_str)
        };

        if let Some(remainder) = file_unix.strip_prefix(&abs_base_slash)
            && best
                .as_ref()
                .is_none_or(|(b, _)| mapping.base_path.len() > b.base_path.len())
        {
            best = Some((mapping, remainder.to_string()));
        }
    }

    let (mapping, remainder) = best?;
    let without_ext = remainder.strip_suffix(".php")?;
    let class_name = without_ext
        .rsplit('/')
        .next()
        .unwrap_or(without_ext)
        .to_string();
    let ns_from_path = without_ext.replace('/', "\\");
    let prefix = mapping.prefix.trim_end_matches('\\');

    let namespace = if ns_from_path.contains('\\') {
        let dir_part = &ns_from_path[..ns_from_path.rfind('\\').unwrap()];
        if prefix.is_empty() {
            Some(dir_part.to_string())
        } else {
            Some(format!("{}\\{}", prefix, dir_part))
        }
    } else if prefix.is_empty() {
        None
    } else {
        Some(prefix.to_string())
    };

    Some((namespace, class_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_namespace_from_path ─────────────────────────────────

    #[test]
    fn reverse_psr4_basic() {
        let mappings = vec![Psr4Mapping {
            prefix: "App\\".to_string(),
            base_path: "app/".to_string(),
        }];
        let root = Path::new("/project");
        let result =
            resolve_namespace_from_path(&mappings, root, Path::new("/project/app/Models/User.php"));
        assert_eq!(
            result,
            Some((Some("App\\Models".to_string()), "User".to_string()))
        );
    }

    #[test]
    fn reverse_psr4_root_of_mapping() {
        let mappings = vec![Psr4Mapping {
            prefix: "App\\".to_string(),
            base_path: "app/".to_string(),
        }];
        let root = Path::new("/project");
        let result =
            resolve_namespace_from_path(&mappings, root, Path::new("/project/app/Kernel.php"));
        assert_eq!(
            result,
            Some((Some("App".to_string()), "Kernel".to_string()))
        );
    }

    #[test]
    fn reverse_psr4_deeply_nested() {
        let mappings = vec![Psr4Mapping {
            prefix: "App\\".to_string(),
            base_path: "app/".to_string(),
        }];
        let root = Path::new("/project");
        let result = resolve_namespace_from_path(
            &mappings,
            root,
            Path::new("/project/app/Http/Controllers/Api/V2/UserController.php"),
        );
        assert_eq!(
            result,
            Some((
                Some("App\\Http\\Controllers\\Api\\V2".to_string()),
                "UserController".to_string()
            ))
        );
    }

    #[test]
    fn a_prefix_with_several_roots_yields_every_directory() {
        // Composer allows an array of directories per prefix, and each of
        // them holds part of the same namespace.
        let mappings = vec![
            Psr4Mapping {
                prefix: "Tests\\".to_string(),
                base_path: "tests/".to_string(),
            },
            Psr4Mapping {
                prefix: "Tests\\".to_string(),
                base_path: "shared/tests/".to_string(),
            },
        ];
        let root = Path::new("/project");
        let directories: Vec<PathBuf> =
            psr4_directories_for_namespace(&mappings, root, "Tests\\Unit")
                .map(|(_, directory)| directory)
                .collect();
        assert_eq!(
            directories,
            vec![
                PathBuf::from("/project/tests/Unit"),
                PathBuf::from("/project/shared/tests/Unit"),
            ]
        );
        assert_eq!(
            psr4_directory_for_namespace(&mappings, root, "Tests\\Unit"),
            Some(PathBuf::from("/project/tests/Unit"))
        );
    }

    #[test]
    fn reverse_psr4_longest_base_path_wins() {
        let mappings = vec![
            Psr4Mapping {
                prefix: "App\\".to_string(),
                base_path: "app/".to_string(),
            },
            Psr4Mapping {
                prefix: "App\\Models\\".to_string(),
                base_path: "app/Models/".to_string(),
            },
        ];
        let root = Path::new("/project");
        let result =
            resolve_namespace_from_path(&mappings, root, Path::new("/project/app/Models/User.php"));
        assert_eq!(
            result,
            Some((Some("App\\Models".to_string()), "User".to_string()))
        );
    }

    #[test]
    fn reverse_psr4_empty_prefix() {
        let mappings = vec![Psr4Mapping {
            prefix: String::new(),
            base_path: "src/".to_string(),
        }];
        let root = Path::new("/project");
        let result =
            resolve_namespace_from_path(&mappings, root, Path::new("/project/src/Foo.php"));
        assert_eq!(result, Some((None, "Foo".to_string())));
    }

    #[test]
    fn reverse_psr4_empty_prefix_nested() {
        let mappings = vec![Psr4Mapping {
            prefix: String::new(),
            base_path: "src/".to_string(),
        }];
        let root = Path::new("/project");
        let result =
            resolve_namespace_from_path(&mappings, root, Path::new("/project/src/Bar/Baz.php"));
        assert_eq!(result, Some((Some("Bar".to_string()), "Baz".to_string())));
    }

    #[test]
    fn reverse_psr4_no_match() {
        let mappings = vec![Psr4Mapping {
            prefix: "App\\".to_string(),
            base_path: "app/".to_string(),
        }];
        let root = Path::new("/project");
        let result = resolve_namespace_from_path(
            &mappings,
            root,
            Path::new("/project/vendor/some/File.php"),
        );
        assert_eq!(result, None);
    }

    #[test]
    fn reverse_psr4_non_php() {
        let mappings = vec![Psr4Mapping {
            prefix: "App\\".to_string(),
            base_path: "app/".to_string(),
        }];
        let root = Path::new("/project");
        let result =
            resolve_namespace_from_path(&mappings, root, Path::new("/project/app/Foo.txt"));
        assert_eq!(result, None);
    }

    #[test]
    fn reverse_psr4_absolute_base_path() {
        let mappings = vec![Psr4Mapping {
            prefix: "Acme\\Billing\\".to_string(),
            base_path: "/project/modules/billing/src/".to_string(),
        }];
        let root = Path::new("/project");
        let result = resolve_namespace_from_path(
            &mappings,
            root,
            Path::new("/project/modules/billing/src/Models/Invoice.php"),
        );
        assert_eq!(
            result,
            Some((
                Some("Acme\\Billing\\Models".to_string()),
                "Invoice".to_string()
            ))
        );
    }
}
