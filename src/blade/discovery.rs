//! Discovery of the Blade templates and component classes a project ships.
//!
//! Laravel addresses a template by a dotted view name (`users.index`) and a
//! component by a tag name (`<x-forms.input>`), neither of which is written
//! anywhere in the project: both are string transforms of a file path. This
//! module performs those transforms once, over the configured view roots and
//! the registered component namespaces, so every consumer reads one index
//! instead of re-deriving names from disk per request.
//!
//! Nothing here resolves a class. A component entry records the FQN a tag
//! name *would* name; the caller loads and validates it through the usual
//! `find_or_load_class` pipeline, exactly as before.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::component_tags::kebab_case;
use super::preprocessor::{ComponentBinding, ComponentParameter, ComponentTarget};
use crate::Backend;

/// The namespace tail Laravel looks for class-based components under when no
/// provider registers a namespace of its own.
const DEFAULT_COMPONENT_NAMESPACE_TAIL: &str = "View\\Components";

/// Which framework's naming rules a class is indexed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComponentKind {
    Blade,
    Livewire,
}

/// The templates and component classes a project makes addressable by name.
#[derive(Debug, Default)]
pub(crate) struct BladeDiscovery {
    /// View dot-name (`users.index`, `pkg::message`) → template file.
    ///
    /// One path per name: Laravel's `FileViewFinder` renders the first hit
    /// across the configured roots, so a name shadowed by an earlier root
    /// never reaches the file behind it.
    pub(crate) views: HashMap<String, PathBuf>,
    /// Component tag name (`alert`, `forms.input`, `pkg::calendar`) → the
    /// FQN of the class backing it.
    pub(crate) components: HashMap<String, String>,
    /// Livewire component name (`counter`, `admin.users`) → class FQN.
    pub(crate) livewire: HashMap<String, String>,
}

impl Backend {
    /// The project's Blade discovery index, built on first use and reused
    /// until an edit invalidates it.
    pub(crate) fn blade_discovery(&self) -> Arc<BladeDiscovery> {
        self.cached_laravel_enumeration(
            &self.laravel_string_key_build_locks.blade_discovery,
            |cache| cache.blade_discovery.clone(),
            |cache, discovery| cache.blade_discovery = Some(discovery),
            || Arc::new(self.build_blade_discovery()),
        )
    }

    /// Walk the view roots and the component namespaces and record every
    /// name they make addressable.
    fn build_blade_discovery(&self) -> BladeDiscovery {
        let mut discovery = BladeDiscovery {
            views: self.scan_view_names(),
            ..BladeDiscovery::default()
        };
        let namespaces = self.component_namespaces();
        let livewire_namespace = self
            .livewire_class_namespace()
            .trim_matches('\\')
            .to_string();
        let classes = self.classes_under(&namespaces, &livewire_namespace);

        for (prefix, namespace, fqn) in classes {
            let kind = if prefix.is_none() && namespace == livewire_namespace {
                ComponentKind::Livewire
            } else {
                ComponentKind::Blade
            };
            let target = match kind {
                ComponentKind::Livewire => &mut discovery.livewire,
                ComponentKind::Blade => &mut discovery.components,
            };
            let Some(tail) = strip_namespace(&fqn, &namespace) else {
                continue;
            };
            for name in component_names_for_tail(tail, kind) {
                let key = match &prefix {
                    Some(prefix) => format!("{prefix}::{name}"),
                    None => name,
                };
                target.entry(key).or_insert_with(|| fqn.clone());
            }
        }

        discovery
    }

    /// Drop the index when an edit to `uri` could have changed the names
    /// it holds.
    ///
    /// A content edit to a template the index already knows changes
    /// nothing about it, and rebuilding per keystroke would put a walk of
    /// every view root back on the edit path. A template the index has
    /// never seen, a component class, or a change to where the roots and
    /// namespaces are does change it.
    pub(crate) fn refresh_blade_discovery(&self, uri: &str) {
        // A component class is addressable the moment its file exists, and
        // both conventions put those files under a `Components/` or
        // `Livewire/` directory whatever root namespace maps them.
        let structural = uri.contains("/config/view.php")
            || uri.contains("/config/livewire.php")
            || uri.contains("/Components/")
            || uri.contains("/Livewire/");
        if !structural {
            if !uri.ends_with(".blade.php") && !uri.contains("/views/") {
                return;
            }
            let cache = self.laravel_string_key_cache.read();
            // Nothing built yet means nothing to drop.
            let Some(discovery) = cache.blade_discovery.as_ref() else {
                return;
            };
            let names = self.view_names_for_blade_uri(uri);
            // A template outside every view root contributes no name at
            // all, so the index is as correct without it as with it.
            let known =
                names.is_empty() || names.iter().all(|name| discovery.views.contains_key(name));
            drop(cache);
            if known {
                return;
            }
        }
        let mut cache = self.laravel_string_key_cache.write();
        cache.blade_discovery = None;
        // The block index is keyed by the very names that just changed.
        cache.blade_blocks = None;
    }

    /// The template file a view name renders, as recorded by the index.
    pub(crate) fn blade_view_path(&self, name: &str) -> Option<PathBuf> {
        self.blade_discovery().views.get(name).cloned()
    }

    /// Every view name in the project, sorted.
    pub(crate) fn blade_view_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.blade_discovery().views.keys().cloned().collect();
        names.sort();
        names
    }

    /// The class an `<x-…>` tag name names, if the project ships one.
    pub(crate) fn blade_component_fqn(&self, tag: &str) -> Option<String> {
        self.blade_discovery().components.get(tag).cloned()
    }

    /// The class a `<livewire:…>` component name names, if the project
    /// ships one.
    pub(crate) fn livewire_component_fqn(&self, name: &str) -> Option<String> {
        self.blade_discovery().livewire.get(name).cloned()
    }

    /// The discovery index, but only when one is already built.
    ///
    /// For the callers that run on the edit path: building the index walks
    /// every view root and the whole class index, which a keystroke (or a
    /// parallel index worker, which invalidates the index as it goes) must
    /// not pay for.
    fn built_blade_discovery(&self) -> Option<Arc<BladeDiscovery>> {
        self.laravel_string_key_cache.read().blade_discovery.clone()
    }

    /// Resolve every component tag `content` references to the class
    /// behind it, as `(tag as written, target)` pairs sorted by tag.
    ///
    /// Part of a template's cached scope rather than something the
    /// preprocessor looks up, for the same reason its injected variables
    /// are: the serial refresh passes own project-wide enumeration and
    /// `update_ast` only reads what they cached.
    pub(crate) fn resolve_component_tags(&self, content: &str) -> Vec<(String, ComponentTarget)> {
        let tags = super::component_tags::referenced_tags(content);
        if tags.is_empty() {
            // Nothing to look up, and asking for the index would build it
            // for a template that has no use for it.
            return Vec::new();
        }
        let discovery = self.blade_discovery();
        let mut resolved: Vec<(String, ComponentTarget)> = tags
            .into_iter()
            .filter_map(|tag| {
                let target = self.component_tag_target(&tag, &discovery)?;
                Some((tag, target))
            })
            .collect();
        resolved.sort_by(|a, b| a.0.cmp(&b.0));
        resolved
    }

    /// The class a tag written as `x-alert` or `livewire:counter` names,
    /// and the call its attributes fill.
    fn component_tag_target(
        &self,
        tag: &str,
        discovery: &BladeDiscovery,
    ) -> Option<ComponentTarget> {
        if let Some(name) = tag.strip_prefix("livewire:") {
            let fqn = discovery.livewire.get(name)?.clone();
            // Livewire builds the component through the container and
            // hands the tag's attributes to `mount()`.
            let binding = self
                .component_signature(&fqn, "mount")
                .map_or(ComponentBinding::Declare, ComponentBinding::Mount);
            return Some(ComponentTarget { fqn, binding });
        }
        let name = tag.strip_prefix("x-")?;
        if let Some(fqn) = discovery.components.get(name) {
            let binding = self
                .component_signature(fqn, "__construct")
                .map_or(ComponentBinding::Declare, ComponentBinding::Construct);
            return Some(ComponentTarget {
                fqn: fqn.clone(),
                binding,
            });
        }
        // No class backs the tag, but a template addressed by it does:
        // Laravel renders that through `AnonymousComponent`, whose own
        // constructor takes the view name and the data rather than the
        // attributes, so the tag declares the variable without a call.
        let anonymous = self.anonymous_component_namespaces();
        super::component_names::view_names_for_component_tag(name, &anonymous)
            .iter()
            .any(|view| discovery.views.contains_key(view))
            .then(|| ComponentTarget {
                fqn: super::ANONYMOUS_COMPONENT.to_string(),
                binding: ComponentBinding::Declare,
            })
    }

    /// The parameters of `class`'s `method`, as the attributes of a tag
    /// that renders it have to fill them.
    ///
    /// `None` when the class cannot be read at all, which leaves the tag
    /// declaring `$component` without claiming anything about its
    /// arguments.  A class that simply declares no such method has an
    /// empty parameter list: Laravel still constructs it, and a tag
    /// passing attributes to it is passing them to the attribute bag.
    fn component_signature(&self, fqn: &str, method: &str) -> Option<Vec<ComponentParameter>> {
        let class = self.find_or_load_class(fqn.trim_matches('\\'))?;
        let loader = |name: &str| self.find_or_load_class(name);
        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            &class,
            &loader,
            Some(&self.resolved_class_cache),
        );
        let Some(signature) = resolved
            .methods
            .iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(method))
        else {
            return Some(Vec::new());
        };

        let mut parameters = Vec::with_capacity(signature.parameters.len());
        for param in signature.parameters.iter() {
            // A variadic parameter collects what is left over
            // positionally, so no attribute names it.
            if param.is_variadic {
                continue;
            }
            let fallback = match param
                .is_required
                .then(|| container_argument(param.type_hint.as_ref()))
                .flatten()
            {
                Some(ContainerArgument::Null) => Some("null".to_string()),
                Some(ContainerArgument::Resolve(class)) => {
                    // Without the helper to name it with, there is no way
                    // to say the container fills this one — and reporting
                    // it missing would be wrong — so the tag makes no call
                    // at all.
                    if !self.has_container_resolve_helper() {
                        return None;
                    }
                    Some(format!("resolve(\\{class}::class)"))
                }
                None => None,
            };
            parameters.push(ComponentParameter {
                name: param.name.trim_start_matches('$').to_string(),
                fallback,
            });
        }
        Some(parameters)
    }

    /// Whether the project has Laravel's `resolve()` helper, which is how
    /// the virtual PHP spells "the container builds this one".
    fn has_container_resolve_helper(&self) -> bool {
        let empty_use_map = std::collections::HashMap::new();
        self.function_loader_with(None, &empty_use_map, &None)("resolve", 0).is_some()
    }

    /// A [`super::preprocessor::ComponentResolver`] over the index the
    /// project already has: whatever a refresh pass cached for this
    /// template, and the live discovery index when one is built.
    ///
    /// The live index leads, so a tag typed since the last refresh pass
    /// resolves on the next keystroke rather than at the next save.
    pub(crate) fn blade_component_resolver<'a>(
        &self,
        cached: &'a [(String, ComponentTarget)],
    ) -> BladeComponentResolver<'a, '_> {
        BladeComponentResolver {
            backend: self,
            discovery: self.built_blade_discovery(),
            cached,
        }
    }

    /// Every configured view root and provider-registered view directory,
    /// scanned into dot-notation names.
    fn scan_view_names(&self) -> HashMap<String, PathBuf> {
        let mut views = HashMap::new();
        for root in self.laravel_view_roots() {
            merge_view_root(&root, "", &mut views);
        }
        for res in &self.laravel_provider_resources.read().view_dirs {
            merge_view_root(&res.path, &res.namespace, &mut views);
        }
        views
    }

    /// The namespaces class-based components are looked up in, as
    /// `(tag prefix, namespace)`: the ones providers registered through
    /// `Blade::componentNamespace()`, then the application's own
    /// `App\View\Components` convention, which has no prefix.
    fn component_namespaces(&self) -> Vec<(Option<String>, String)> {
        let mut namespaces: Vec<(Option<String>, String)> = self
            .laravel_provider_resources
            .read()
            .class_component_namespaces
            .iter()
            .map(|(prefix, namespace)| {
                (
                    Some(prefix.clone()),
                    namespace.trim_matches('\\').to_string(),
                )
            })
            .collect();
        namespaces.push((
            None,
            format!(
                "{}{DEFAULT_COMPONENT_NAMESPACE_TAIL}",
                self.application_namespace()
            ),
        ));
        namespaces
    }

    /// The class FQNs living under each requested namespace, as
    /// `(tag prefix, namespace, FQN)`.
    ///
    /// Two sources, because neither covers the other's blind spot: the
    /// directory a PSR-4 mapping puts the namespace in finds a class the
    /// classmap has no entry for (a project that never ran
    /// `composer dump-autoload`), and the class index finds one whose
    /// namespace no PSR-4 mapping of the project's own `composer.json`
    /// covers (a vendor package that registered a component namespace).
    fn classes_under(
        &self,
        namespaces: &[(Option<String>, String)],
        livewire_namespace: &str,
    ) -> Vec<(Option<String>, String, String)> {
        let mut targets: Vec<(Option<String>, String)> = namespaces.to_vec();
        targets.push((None, livewire_namespace.to_string()));

        let mut classes = Vec::new();
        for (prefix, namespace) in &targets {
            for fqn in self.classes_in_namespace_dir(namespace) {
                classes.push((prefix.clone(), namespace.clone(), fqn));
            }
        }

        // One pass over the class index for all namespaces at once: it holds
        // every class in the project and re-walking it per namespace would
        // multiply the cost of a cache build for no gain.
        let index = self.fqn_uri_index().read();
        for (fqn, _) in index.iter() {
            for (prefix, namespace) in &targets {
                if strip_namespace(fqn, namespace).is_some() {
                    classes.push((prefix.clone(), namespace.clone(), fqn.to_string()));
                }
            }
        }

        classes
    }

    /// The classes in the directory a PSR-4 mapping puts `namespace` in.
    ///
    /// Empty when no mapping covers it, which is the normal case for a
    /// vendor package's namespace: the class index picks those up instead.
    fn classes_in_namespace_dir(&self, namespace: &str) -> Vec<String> {
        let Some(root) = self.workspace_root().read().clone() else {
            return Vec::new();
        };
        let Some(dir) = self.namespace_directory(&root, namespace) else {
            return Vec::new();
        };
        let mut classes = Vec::new();
        collect_class_files(&dir, &dir, namespace, &mut classes);
        classes
    }

    /// Resolve `namespace` to the directory the project's own PSR-4
    /// mappings put it in, taking the longest matching prefix so a nested
    /// mapping wins over a root one.
    fn namespace_directory(&self, root: &Path, namespace: &str) -> Option<PathBuf> {
        let namespace = namespace.trim_matches('\\');
        let mappings = self.psr4_mappings().read();
        let mut best: Option<(usize, PathBuf)> = None;
        for mapping in mappings.iter() {
            let prefix = mapping.prefix.trim_matches('\\');
            let rest = if prefix.is_empty() {
                Some(namespace)
            } else if namespace.eq_ignore_ascii_case(prefix) {
                Some("")
            } else {
                namespace
                    .get(..prefix.len())
                    .filter(|head| head.eq_ignore_ascii_case(prefix))
                    .and_then(|_| namespace[prefix.len()..].strip_prefix('\\'))
            };
            let Some(rest) = rest else {
                continue;
            };
            if best.as_ref().is_some_and(|(len, _)| *len >= prefix.len()) {
                continue;
            }
            let mut dir = root.join(mapping.base_path.trim_start_matches("./"));
            for segment in rest.split('\\').filter(|s| !s.is_empty()) {
                dir.push(segment);
            }
            best = Some((prefix.len(), dir));
        }
        best.map(|(_, dir)| dir).filter(|dir| dir.is_dir())
    }
}

/// What Laravel's container passes for a required parameter no attribute
/// filled.
enum ContainerArgument {
    /// A nullable parameter the container leaves unset.
    Null,
    /// A class the container builds, by name.
    Resolve(String),
}

/// What the container would pass for a required parameter a component tag
/// left out, which is what Laravel does as soon as a tag's attributes do
/// not cover every parameter (`Component::resolveComponent` hands the
/// whole thing to the container rather than calling `new` itself).
///
/// `None` for a parameter the container cannot build either: that one is
/// genuinely missing, and reporting it is right.
fn container_argument(type_hint: Option<&crate::php_type::PhpType>) -> Option<ContainerArgument> {
    let type_hint = type_hint?;
    if type_hint.accepts_null() {
        return Some(ContainerArgument::Null);
    }
    let class = type_hint.class_name()?;
    Some(ContainerArgument::Resolve(
        class.trim_matches('\\').to_string(),
    ))
}

/// Resolves a component tag against the project's Blade indexes without
/// building any of them. See [`Backend::blade_component_resolver`].
pub(crate) struct BladeComponentResolver<'a, 'b> {
    backend: &'b Backend,
    discovery: Option<Arc<BladeDiscovery>>,
    cached: &'a [(String, ComponentTarget)],
}

impl BladeComponentResolver<'_, '_> {
    fn resolve(&self, tag: &str) -> Option<ComponentTarget> {
        if let Some(discovery) = &self.discovery
            && let Some(target) = self.backend.component_tag_target(tag, discovery)
        {
            return Some(target);
        }
        self.cached
            .iter()
            .find(|(cached_tag, _)| cached_tag == tag)
            .map(|(_, target)| target.clone())
    }
}

impl super::preprocessor::ComponentResolver for BladeComponentResolver<'_, '_> {
    fn x_component(&self, tag: &str) -> Option<ComponentTarget> {
        self.resolve(&format!("x-{tag}"))
    }

    fn livewire_component(&self, name: &str) -> Option<ComponentTarget> {
        self.resolve(&format!("livewire:{name}"))
    }
}

/// Scan one view root into dot-notation names and fold it into `views`,
/// leaving names an earlier root already claimed alone.
fn merge_view_root(root: &Path, namespace: &str, views: &mut HashMap<String, PathBuf>) {
    let mut local = HashMap::new();
    collect_view_files(root, root, namespace, &mut local);
    for (name, path) in local {
        views.entry(name).or_insert(path);
    }
}

/// Recursively record every template under `dir` by the name it renders as.
///
/// Within one root a `.blade.php` file wins over a `.php` file of the same
/// name, matching the extension order `FileViewFinder` tries.
fn collect_view_files(
    base: &Path,
    dir: &Path,
    namespace: &str,
    out: &mut HashMap<String, PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_view_files(base, &path, namespace, out);
            continue;
        }
        let Some(rel) = path.strip_prefix(base).ok().and_then(|rel| rel.to_str()) else {
            continue;
        };
        let is_blade = rel.ends_with(".blade.php");
        let Some(stem) = rel
            .strip_suffix(".blade.php")
            .or_else(|| rel.strip_suffix(".php"))
        else {
            continue;
        };
        let dotted = stem.replace([std::path::MAIN_SEPARATOR, '/'], ".");
        // An empty namespace means the app's own view roots, where names
        // are used bare (`admin.permissions.index`). Package views use the
        // `namespace::name` form.
        let name = if namespace.is_empty() {
            dotted
        } else {
            format!("{namespace}::{dotted}")
        };
        match out.entry(name) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(path);
            }
            std::collections::hash_map::Entry::Occupied(mut slot) => {
                if is_blade {
                    slot.insert(path);
                }
            }
        }
    }
}

/// Recursively record the FQN of every `.php` file under `dir`, treating
/// the path below `base` as the namespace tail below `namespace`.
fn collect_class_files(base: &Path, dir: &Path, namespace: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_class_files(base, &path, namespace, out);
            continue;
        }
        let Some(rel) = path.strip_prefix(base).ok().and_then(|rel| rel.to_str()) else {
            continue;
        };
        let Some(stem) = rel.strip_suffix(".php") else {
            continue;
        };
        let tail = stem.replace([std::path::MAIN_SEPARATOR, '/'], "\\");
        if tail.is_empty() {
            continue;
        }
        out.push(format!("{namespace}\\{tail}"));
    }
}

/// The part of `fqn` below `namespace`, or `None` when it lives elsewhere.
fn strip_namespace<'a>(fqn: &'a str, namespace: &str) -> Option<&'a str> {
    let namespace = namespace.trim_matches('\\');
    if namespace.is_empty() {
        return Some(fqn.trim_start_matches('\\'));
    }
    let fqn = fqn.trim_start_matches('\\');
    fqn.get(..namespace.len())
        .filter(|head| head.eq_ignore_ascii_case(namespace))
        .and_then(|_| fqn[namespace.len()..].strip_prefix('\\'))
        .filter(|rest| !rest.is_empty())
}

/// The names a class tail is addressable by: the kebab-dotted form of its
/// path (`Forms\DatePicker` → `forms.date-picker`), plus the shorter name
/// an index component also answers to.
///
/// Both frameworks let a name that resolves to nothing fall through to a
/// class one directory deeper, but they spell that class differently:
/// Blade repeats the directory name (`<x-card>` reaches `Card\Card`, via
/// `ComponentTagCompiler::componentClass()`) and Livewire appends `Index`
/// (`<livewire:posts>` reaches `Posts\Index`, via
/// `ComponentRegistry::getNameAndClass()`).
fn component_names_for_tail(tail: &str, kind: ComponentKind) -> Vec<String> {
    let segments: Vec<&str> = tail.split('\\').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Vec::new();
    }
    let dotted = |segments: &[&str]| {
        segments
            .iter()
            .map(|segment| kebab_case(segment))
            .collect::<Vec<_>>()
            .join(".")
    };
    let mut names = vec![dotted(&segments)];
    if segments.len() >= 2 {
        let last = segments[segments.len() - 1];
        let is_index = match kind {
            ComponentKind::Blade => last.eq_ignore_ascii_case(segments[segments.len() - 2]),
            ComponentKind::Livewire => last.eq_ignore_ascii_case("index"),
        };
        if is_index {
            names.push(dotted(&segments[..segments.len() - 1]));
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn backend_at(root: &Path) -> Backend {
        let (mappings, _) = crate::composer::parse_composer_json(root);
        Backend::new_test_with_workspace(root.to_path_buf(), mappings)
    }

    #[test]
    fn a_nested_class_is_addressable_by_its_dotted_path() {
        assert_eq!(
            component_names_for_tail("Forms\\DatePicker", ComponentKind::Blade),
            vec!["forms.date-picker"]
        );
    }

    /// Each framework spells its index component differently, so neither
    /// rule may leak into the other's map.
    #[test]
    fn an_index_component_is_addressable_both_ways() {
        assert_eq!(
            component_names_for_tail("Card\\Card", ComponentKind::Blade),
            vec!["card.card", "card"]
        );
        assert_eq!(
            component_names_for_tail("Posts\\Index", ComponentKind::Livewire),
            vec!["posts.index", "posts"]
        );
        assert_eq!(
            component_names_for_tail("Posts\\Index", ComponentKind::Blade),
            vec!["posts.index"]
        );
        assert_eq!(
            component_names_for_tail("Card\\Card", ComponentKind::Livewire),
            vec!["card.card"]
        );
    }

    #[test]
    fn views_are_indexed_by_dot_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "composer.json",
            r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        );
        write(root, "resources/views/users/index.blade.php", "");
        write(root, "resources/views/components/alert.blade.php", "");

        let backend = backend_at(root);
        let discovery = backend.blade_discovery();
        assert_eq!(
            discovery.views.get("users.index"),
            Some(&root.join("resources/views/users/index.blade.php"))
        );
        assert!(discovery.views.contains_key("components.alert"));
    }

    /// A project that points `config/view.php` at its own directory has
    /// templates outside `resources/views`, and they are addressed by the
    /// same bare names.
    #[test]
    fn views_are_indexed_from_every_configured_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "composer.json", "{}");
        write(
            root,
            "config/view.php",
            "<?php\nreturn [\n 'paths' => [\n  base_path('resources/backoffice/views'),\n  resource_path('views'),\n ],\n];\n",
        );
        write(
            root,
            "resources/backoffice/views/admin/permissions/index.blade.php",
            "",
        );
        write(root, "resources/views/welcome.blade.php", "");

        let names = backend_at(root).blade_view_names();
        assert!(names.contains(&"admin.permissions.index".to_string()));
        assert!(names.contains(&"welcome".to_string()));
    }

    #[test]
    fn a_blade_template_wins_over_a_plain_php_view_of_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "composer.json", "{}");
        write(root, "resources/views/welcome.php", "");
        write(root, "resources/views/welcome.blade.php", "");

        let backend = backend_at(root);
        assert_eq!(
            backend.blade_view_path("welcome"),
            Some(root.join("resources/views/welcome.blade.php"))
        );
    }

    /// Rebuilding walks every view root, so a keystroke in a template the
    /// index already holds must not trigger one.
    #[test]
    fn editing_a_known_template_keeps_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "composer.json", "{}");
        write(root, "resources/views/welcome.blade.php", "");

        let backend = backend_at(root);
        let cached = |backend: &Backend| {
            backend
                .laravel_string_key_cache
                .read()
                .blade_discovery
                .is_some()
        };
        let uri_of = |relative: &str| {
            tower_lsp::lsp_types::Url::from_file_path(root.join(relative))
                .unwrap()
                .to_string()
        };

        backend.blade_discovery();
        backend.refresh_blade_discovery(&uri_of("resources/views/welcome.blade.php"));
        assert!(cached(&backend), "a known template changes no name");

        // A template the index has never seen adds one, so it must go.
        write(root, "resources/views/about.blade.php", "");
        backend.refresh_blade_discovery(&uri_of("resources/views/about.blade.php"));
        assert!(!cached(&backend));
        assert!(backend.blade_view_names().contains(&"about".to_string()));

        // So does a component class, wherever its namespace is rooted.
        backend.refresh_blade_discovery(&uri_of("app/View/Components/Alert.php"));
        assert!(!cached(&backend));
    }

    #[test]
    fn component_classes_are_indexed_by_tag_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "composer.json",
            r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        );
        write(root, "app/View/Components/Alert.php", "<?php\n");
        write(root, "app/View/Components/Forms/Input.php", "<?php\n");
        write(root, "app/View/Components/Card/Card.php", "<?php\n");

        let backend = backend_at(root);
        let discovery = backend.blade_discovery();
        assert_eq!(
            discovery.components.get("alert").map(String::as_str),
            Some("App\\View\\Components\\Alert")
        );
        assert_eq!(
            discovery.components.get("forms.input").map(String::as_str),
            Some("App\\View\\Components\\Forms\\Input")
        );
        // The index component answers to both spellings.
        assert_eq!(
            discovery.components.get("card").map(String::as_str),
            Some("App\\View\\Components\\Card\\Card")
        );
        assert!(discovery.components.contains_key("card.card"));
    }

    #[test]
    fn livewire_classes_are_indexed_by_component_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "composer.json",
            r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        );
        write(root, "app/Livewire/Counter.php", "<?php\n");
        write(root, "app/Livewire/Admin/Users.php", "<?php\n");
        write(root, "app/Livewire/Posts/Index.php", "<?php\n");

        let backend = backend_at(root);
        assert_eq!(
            backend.livewire_component_fqn("counter").as_deref(),
            Some("App\\Livewire\\Counter")
        );
        assert_eq!(
            backend.livewire_component_fqn("admin.users").as_deref(),
            Some("App\\Livewire\\Admin\\Users")
        );
        // An index component answers to the directory alone as well.
        assert_eq!(
            backend.livewire_component_fqn("posts").as_deref(),
            Some("App\\Livewire\\Posts\\Index")
        );
        // A Livewire class is not a Blade component tag.
        assert!(backend.blade_component_fqn("counter").is_none());
    }

    #[test]
    fn a_registered_namespace_is_indexed_under_its_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "composer.json",
            r#"{"autoload":{"psr-4":{"Nightshade\\":"packages/nightshade/src/"}}}"#,
        );
        write(
            root,
            "packages/nightshade/src/Views/Components/Calendar.php",
            "<?php\n",
        );

        let backend = backend_at(root);
        backend
            .laravel_provider_resources
            .write()
            .class_component_namespaces
            .push((
                "nightshade".to_string(),
                "Nightshade\\Views\\Components".to_string(),
            ));

        assert_eq!(
            backend
                .blade_component_fqn("nightshade::calendar")
                .as_deref(),
            Some("Nightshade\\Views\\Components\\Calendar")
        );
    }
}
