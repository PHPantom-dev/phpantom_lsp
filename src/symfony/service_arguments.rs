//! Named arguments bind to the callable that constructs the service. A
//! file's default bindings can bind parameters on several callables.

use std::sync::Arc;

use crate::Backend;
use crate::atom::{Atom, atom};
use crate::symbol_map::{SymbolKind, SymbolSpan};
use crate::type_engine::call_resolution::NamedArgumentDeclaration;

/// A configured callable or a namespace of services loaded from a directory.
#[derive(Clone, Debug)]
pub(crate) enum Target {
    Method {
        owner: Atom,
        method: Atom,
    },
    Resource {
        namespace: Atom,
        directory: Atom,
        excludes: Vec<Atom>,
    },
}

pub(super) struct Argument {
    start: usize,
    end: usize,
    native_type: Option<Atom>,
}

/// Record the name in either a bare binding or a type-qualified binding.
pub(super) fn argument_key(key: &str, start: usize, out: &mut Vec<Argument>) {
    let Some(name) = key
        .split_whitespace()
        .next_back()
        .and_then(|key| key.strip_prefix('$'))
    else {
        return;
    };
    if super::service_calls::is_method_name(name) {
        let offset = super::resources::offset_within(key, name) - 1;
        let ty = key[..offset].trim();
        out.push(Argument {
            start: start + offset,
            end: start + offset + 1 + name.len(),
            native_type: (!ty.is_empty()).then(|| atom(ty.trim_start_matches('\\'))),
        });
    }
}

/// All keys in one binding map share its list of candidate callables.
pub(crate) type Targets = Arc<[Target]>;

/// Attach the service callables to the YAML keys that bind their parameters.
pub(super) fn argument_spans(
    content: &str,
    names: Vec<Argument>,
    targets: Targets,
    spans: &mut Vec<SymbolSpan>,
) {
    for Argument {
        start,
        end,
        native_type,
    } in names
    {
        spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::ServiceArgument {
                name: atom(&content[start + 1..end]),
                native_type,
                targets: Arc::clone(&targets),
            },
        });
    }
}

#[derive(Default)]
pub(super) struct Bindings {
    /// Track the callables in this section when the file contains default bindings.
    pub(super) collect_targets: bool,
    /// Constructors and factories declared under the current `services:` key.
    pub(super) targets: Vec<Target>,
    /// Keys of `_defaults.bind` awaiting the complete list of services.
    pub(super) defaults: Vec<Argument>,
}

impl Bindings {
    /// End a services section before moving into another environment.
    pub(super) fn finish(&mut self, content: &str, spans: &mut Vec<SymbolSpan>) {
        if !self.defaults.is_empty() {
            argument_spans(
                content,
                std::mem::take(&mut self.defaults),
                std::mem::take(&mut self.targets).into(),
                spans,
            );
        }
        self.targets.clear();
    }
}

impl Backend {
    /// Resolve a configuration binding through each service callable in its scope.
    pub(crate) fn service_argument_declarations(
        &self,
        uri: &str,
        name: &Atom,
        targets: &[Target],
        native_type: Option<Atom>,
    ) -> Vec<NamedArgumentDeclaration> {
        let mut declarations = Vec::new();
        for target in targets {
            let mut visit = |class: &Atom, method: &str| {
                if let Some(mut decl) =
                    self.declared_parameter(uri, class, method, name, native_type)
                    && decl.name_token.is_some()
                    && !declarations.iter().any(|d| decl.is_same_declaration(d))
                {
                    decl.promotes_property =
                        method == "__construct" && self.constructor_promotes(&decl, name);
                    declarations.push(decl);
                }
            };
            match target {
                Target::Method { owner, method } => {
                    if let Some(id) = owner.strip_prefix('@') {
                        for class in self.symfony_service_classes(id) {
                            visit(&class, method);
                        }
                    } else {
                        visit(owner, method);
                    }
                }
                Target::Resource {
                    namespace,
                    directory,
                    excludes,
                } => {
                    for class in self.service_resource_classes(uri, namespace, directory, excludes)
                    {
                        visit(&class, "__construct");
                    }
                }
            }
        }
        declarations
    }

    /// A namespace resource loads each class from its matching relative PHP path.
    /// Snapshot the class names before resolving them, which may update the index.
    fn service_resource_classes(
        &self,
        uri: &str,
        namespace: &str,
        directory: &str,
        excludes: &[Atom],
    ) -> Vec<Atom> {
        let Some(config) = crate::util::uri_to_path(uri) else {
            return Vec::new();
        };
        let base = config.parent().unwrap_or(&config);
        let matcher = |pattern: &str| {
            let path = crate::document_links::normalize_path(&base.join(pattern));
            globset::Glob::new(&path.to_string_lossy().replace('\\', "/"))
                .ok()
                .map(|glob| glob.compile_matcher())
        };
        // A resource is a glob (`../src/*`, `../src/{Controller,Service}/`).
        // Class names follow the path below the part before the first
        // wildcard, and only files the glob reaches are loaded.
        let (root, include) = match directory.find(['*', '?', '{', '[']) {
            Some(wildcard) => {
                let prefix = &directory[..directory[..wildcard].rfind('/').map_or(0, |at| at + 1)];
                let Some(include) = matcher(directory) else {
                    return Vec::new();
                };
                (base.join(prefix), Some(include))
            }
            None => (base.join(directory), None),
        };
        let root = crate::document_links::normalize_path(&root);
        let exclude: Vec<_> = excludes
            .iter()
            .filter_map(|pattern| matcher(pattern))
            .collect();
        self.symbols
            .fqn_uri_index
            .read()
            .iter()
            .filter_map(|(name, uri)| {
                let suffix = name.strip_prefix(namespace)?.strip_prefix('\\')?;
                let file = crate::util::uri_to_path(uri)?;
                if file != root.join(suffix.replace('\\', "/")).with_extension("php")
                    || include
                        .as_ref()
                        .is_some_and(|include| !file.ancestors().any(|path| include.is_match(path)))
                    || file
                        .ancestors()
                        .any(|path| exclude.iter().any(|matcher| matcher.is_match(path)))
                {
                    return None;
                }
                Some(atom(name))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_relative_service_resource_requires_a_file_uri() {
        let backend = crate::test_fixtures::make_backend();
        assert!(
            backend
                .service_resource_classes("untitled:services.yaml", "App", "../src", &[])
                .is_empty()
        );
    }
}
