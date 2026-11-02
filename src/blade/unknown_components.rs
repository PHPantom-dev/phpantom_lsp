//! Whether a `<x-…>` or `<livewire:…>` tag names a component the project
//! has.
//!
//! A tag resolves through the same index completion and go-to-definition
//! read ([`super::discovery`]), and through the aliases providers register
//! by name. Only a name this can prove absent counts as unknown: a tag
//! under a prefix nothing here can enumerate, or in a project that
//! registers components in a way the provider scan cannot read, is left
//! alone, since the valid set there is unknown rather than empty.

use std::sync::Arc;

use super::component_names::{AnonymousNamespace, component_tag_names};
use super::component_tags::TagKind;
use super::discovery::{BladeDiscovery, DEFAULT_COMPONENT_NAMESPACE_TAIL};
use crate::Backend;

/// Blade tags the framework handles itself rather than through a
/// component a project ships.
const BUILTIN_BLADE_TAGS: [&str; 2] = ["slot", "dynamic-component"];

/// The tag prefix `ComponentTagCompiler::componentClass()` lets through
/// unresolved, for the Markdown mail components the mailer registers at
/// render time.
const MAIL_PREFIX: &str = "mail";

/// Livewire tags its own tag compiler handles: the asset tags and the
/// dynamic component, whose real name is an attribute.
const BUILTIN_LIVEWIRE_TAGS: [&str; 4] = ["styles", "scripts", "is", "dynamic-component"];

/// The directories a Livewire tag can name a single-file (Volt or Livewire
/// 4) component in, as view-name prefixes.
const LIVEWIRE_VIEW_DIRECTORIES: [&str; 4] = ["livewire", "components", "pages", "layouts"];

/// The marker Livewire 4 puts in front of a single-file component's name.
const LIVEWIRE_SFC_MARKER: &str = "\u{26A1}";

/// The class a dotted component path names below `namespace`, each segment
/// studly-cased the way Laravel and Livewire derive a class from a tag:
/// `forms.date-picker` under `App\View\Components` is
/// `App\View\Components\Forms\DatePicker`.
pub(crate) fn class_for_tag_path(namespace: &str, path: &str) -> String {
    std::iter::once(namespace.to_string())
        .chain(path.split('.').map(studly))
        .collect::<Vec<_>>()
        .join("\\")
}

/// Laravel's `Str::studly()`: each `-`/`_`-separated word capitalised and
/// joined.
fn studly(segment: &str) -> String {
    segment
        .split(['-', '_'])
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

/// Everything a tag is judged against, read once per template.
pub(crate) struct ComponentIndex {
    discovery: Arc<BladeDiscovery>,
    anonymous: Vec<AnonymousNamespace>,
    /// Tag prefixes whose components can be enumerated: class component
    /// namespaces, anonymous component namespaces, and view namespaces.
    known_prefixes: Vec<String>,
    /// Prefixes of anonymous component directories outside every view
    /// root, whose templates the index holds no name for.
    unmapped_prefixes: Vec<String>,
    aliases: Vec<String>,
    open_alias_prefixes: Vec<String>,
    /// `None` when Livewire tags cannot be judged at all.
    livewire_aliases: Option<Vec<String>>,
    /// The namespaces a Blade tag's class is guessed in, as `(tag prefix,
    /// namespace)`, the application's own having no prefix.
    class_namespaces: Vec<(Option<String>, String)>,
    livewire_namespace: String,
}

impl Backend {
    /// The index component tags are judged against.  Builds the discovery
    /// index when it is not built yet.
    ///
    /// Whether Livewire tags can be judged at all takes a class lookup and
    /// a read of Livewire's config, so it is only worked out when
    /// `with_livewire` says a template has a Livewire tag to judge.
    pub(crate) fn component_index(&self, with_livewire: bool) -> ComponentIndex {
        let discovery = self.blade_discovery();
        let (anonymous, unmapped) = self.anonymous_component_registrations();
        let (
            known_prefixes,
            aliases,
            open_alias_prefixes,
            livewire_aliases,
            open_livewire,
            mut class_namespaces,
        ) = {
            let resources = self.laravel.provider_resources.read();
            let known_prefixes: Vec<String> = resources
                .class_component_namespaces
                .iter()
                .map(|(prefix, _)| prefix.clone())
                .chain(
                    resources
                        .anonymous_component_namespaces
                        .iter()
                        .map(|(prefix, _)| prefix.clone()),
                )
                .chain(
                    resources
                        .anonymous_component_paths
                        .iter()
                        .map(|(prefix, _)| prefix.clone()),
                )
                .chain(resources.view_dirs.iter().map(|dir| dir.namespace.clone()))
                .filter(|prefix| !prefix.is_empty())
                .collect();
            (
                known_prefixes,
                resources.component_aliases.clone(),
                resources.open_component_alias_prefixes.clone(),
                resources.livewire_aliases.clone(),
                resources.open_livewire_aliases,
                resources
                    .class_component_namespaces
                    .iter()
                    .map(|(prefix, namespace)| {
                        (
                            Some(prefix.clone()),
                            namespace.trim_matches('\\').to_string(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        };
        class_namespaces.push((
            None,
            format!(
                "{}{DEFAULT_COMPONENT_NAMESPACE_TAIL}",
                self.application_namespace()
            ),
        ));
        let livewire_judgeable = with_livewire
            && !open_livewire
            // A project that has no component Livewire can find may keep
            // them where this cannot look (Livewire 2's `App\Http\Livewire`
            // without a config file), so it tells nothing about any name.
            && (!discovery.livewire.is_empty() || !livewire_aliases.is_empty())
            && self.find_or_load_class("Livewire\\Component").is_some()
            && !self.livewire_config_has_component_locations();
        ComponentIndex {
            discovery,
            anonymous,
            known_prefixes,
            unmapped_prefixes: unmapped.into_iter().map(|(prefix, _)| prefix).collect(),
            aliases,
            open_alias_prefixes,
            livewire_aliases: livewire_judgeable.then_some(livewire_aliases),
            class_namespaces,
            livewire_namespace: if livewire_judgeable {
                self.livewire_class_namespace()
                    .trim_matches('\\')
                    .to_string()
            } else {
                String::new()
            },
        }
    }

    /// Whether `config/livewire.php` lists the directories Livewire 4
    /// looks for single-file components in, which this does not read.
    fn livewire_config_has_component_locations(&self) -> bool {
        let Some(root) = self.workspace_root().read().clone() else {
            return false;
        };
        let path = root.join("config").join("livewire.php");
        let content = crate::util::path_to_url(&path)
            .and_then(|uri| self.get_file_content(uri.as_str()))
            .or_else(|| std::fs::read_to_string(&path).ok());
        content.is_some_and(|content| content.contains("component_locations"))
    }
}

impl ComponentIndex {
    /// The anonymous component registrations in effect.
    pub(crate) fn anonymous(&self) -> &[AnonymousNamespace] {
        &self.anonymous
    }

    /// Whether a tag of `kind` named `name` provably names nothing.
    ///
    /// The discovery index answers for almost every tag; one it does not
    /// know is looked up the way the framework itself does it, by the
    /// class the name studly-cases to, which reaches a class the index
    /// never listed (one a vendor package autoloads).
    pub(crate) fn is_unknown(&self, backend: &Backend, kind: TagKind, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        match kind {
            TagKind::Blade => {
                self.is_unknown_blade(name) && !self.blade_class_exists(backend, name)
            }
            TagKind::Livewire => {
                self.is_unknown_livewire(name) && !self.livewire_class_exists(backend, name)
            }
        }
    }

    /// `ComponentTagCompiler::guessClassName()` and its repeated-segment
    /// fallback, under the namespace the tag's prefix is registered for.
    fn blade_class_exists(&self, backend: &Backend, name: &str) -> bool {
        let (prefix, path) = match name.split_once("::") {
            Some((prefix, path)) => (Some(prefix), path),
            None => (None, name),
        };
        let Some((_, namespace)) = self
            .class_namespaces
            .iter()
            .find(|(registered, _)| registered.as_deref() == prefix)
        else {
            return false;
        };
        let class = class_for_tag_path(namespace, path);
        let last = class.rsplit('\\').next().unwrap_or_default();
        let index = format!("{class}\\{last}");
        backend.find_or_load_class(&class).is_some() || backend.find_or_load_class(&index).is_some()
    }

    /// Livewire's `ComponentRegistry` guess and its `Index` fallback.
    fn livewire_class_exists(&self, backend: &Backend, name: &str) -> bool {
        if self.livewire_namespace.is_empty() {
            return false;
        }
        let class = class_for_tag_path(&self.livewire_namespace, name);
        backend.find_or_load_class(&class).is_some()
            || backend
                .find_or_load_class(&format!("{class}\\Index"))
                .is_some()
    }

    fn is_unknown_blade(&self, name: &str) -> bool {
        if BUILTIN_BLADE_TAGS.contains(&name) || name.starts_with("slot:") {
            return false;
        }
        // A project whose views the scan found none of may keep them
        // anywhere, so no name is provably missing.
        if self.discovery.views.is_empty() && self.discovery.components.is_empty() {
            return false;
        }
        if self.blade_resolves(name) || self.aliases.iter().any(|alias| alias == name) {
            return false;
        }
        match name.split_once("::") {
            Some((prefix, _)) => {
                prefix != MAIL_PREFIX
                    && self.known_prefixes.iter().any(|known| known == prefix)
                    && !self.unmapped_prefixes.iter().any(|open| open == prefix)
            }
            None => {
                !self.unmapped_prefixes.iter().any(String::is_empty)
                    && !self.open_alias_prefixes.iter().any(|prefix| {
                        prefix.is_empty()
                            || name
                                .strip_prefix(prefix.as_str())
                                .is_some_and(|rest| rest.starts_with('-'))
                    })
            }
        }
    }

    /// Whether a class or a template answers to the Blade tag `name`.
    fn blade_resolves(&self, name: &str) -> bool {
        self.discovery.blade_component(name).is_some()
            || super::component_names::view_names_for_component_tag(name, &self.anonymous)
                .iter()
                .any(|view| self.discovery.views.contains_key(view))
    }

    fn is_unknown_livewire(&self, name: &str) -> bool {
        let Some(aliases) = &self.livewire_aliases else {
            return false;
        };
        if BUILTIN_LIVEWIRE_TAGS.contains(&name) || name.contains("::") {
            return false;
        }
        !(self.discovery.livewire_component(name).is_some()
            || aliases.iter().any(|alias| alias == name)
            || self.livewire_view_resolves(name))
    }

    /// Whether a single-file component answers to the Livewire tag
    /// `name`: a template in one of the directories Volt and Livewire 4
    /// look in, with or without Livewire 4's marker, either on its own or
    /// as the index of a directory of the same name.
    fn livewire_view_resolves(&self, name: &str) -> bool {
        let (dirs, last) = match name.rsplit_once('.') {
            Some((dirs, last)) => (Some(dirs), last),
            None => (None, name),
        };
        let marked = format!("{LIVEWIRE_SFC_MARKER}{last}");
        LIVEWIRE_VIEW_DIRECTORIES.iter().any(|directory| {
            [last, marked.as_str()].iter().any(|leaf| {
                let base = match dirs {
                    Some(dirs) => format!("{directory}.{dirs}.{leaf}"),
                    None => format!("{directory}.{leaf}"),
                };
                [
                    base.clone(),
                    format!("{base}.index"),
                    format!("{base}.{last}"),
                ]
                .iter()
                .any(|view| self.discovery.views.contains_key(view))
            })
        })
    }

    /// Every name a tag of `kind` can be written with, sorted, for
    /// suggesting what a misspelt one meant.
    pub(crate) fn known_names(&self, kind: TagKind) -> Vec<String> {
        let mut names: Vec<String> = match kind {
            TagKind::Blade => {
                let views: Vec<String> = self.discovery.views.keys().cloned().collect();
                self.discovery
                    .components
                    .keys()
                    .cloned()
                    .chain(component_tag_names(&views, &self.anonymous))
                    .chain(self.aliases.iter().cloned())
                    .collect()
            }
            TagKind::Livewire => self
                .discovery
                .livewire
                .keys()
                .cloned()
                .chain(self.livewire_aliases.iter().flatten().cloned())
                .collect(),
        };
        names.sort();
        names.dedup();
        names
    }
}
