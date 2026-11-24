//! Whether a `<x-…>`, `<flux:…>` or `<livewire:…>` tag names a component the project
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

/// The directories a Livewire tag can name a single-file component in
/// when the installed Livewire's own locations are not known (Volt's
/// default mounts and Livewire 4's default locations and namespaces), as
/// view-name prefixes.
const FALLBACK_VIEW_DIRECTORIES: [&str; 4] = ["livewire.", "components.", "pages.", "layouts."];

/// The markers Livewire 4 accepts in front of a view-based component's
/// name: none, or the zap emoji bare or with either variation selector.
const LIVEWIRE_MARKERS: [&str; 4] = ["", "\u{26A1}", "\u{26A1}\u{FE0E}", "\u{26A1}\u{FE0F}"];

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
    aliases: Vec<String>,
    open_alias_prefixes: Vec<String>,
    /// `None` when Livewire tags cannot be judged at all.
    livewire_aliases: Option<Vec<String>>,
    /// The namespaces a Blade tag's class is guessed in, as `(tag prefix,
    /// namespace)`, the application's own having no prefix.
    class_namespaces: Vec<(Option<String>, String)>,
    livewire_namespace: String,
    /// Where a Livewire tag can name a view-based component.
    livewire_view_prefixes: Vec<LivewireViewPrefix>,
    /// Livewire 4's config names its component locations in a way this
    /// cannot read, so any tag that names no namespace may be an anonymous
    /// component in one of them.
    open_anonymous_paths: bool,
}

/// A directory a Livewire tag can name a view-based component in, as the
/// view names its templates are indexed under.
pub(crate) struct LivewireViewPrefix {
    /// The component namespace the directory is registered under, `None`
    /// for a component location.
    namespace: Option<String>,
    /// What the templates' view names start with: `livewire.` for a
    /// location inside a view root, `pages::` for a component namespace,
    /// nothing for a location that is a view root of its own.
    prefix: String,
    /// The directory itself, when `prefix` is empty: a view root of its own
    /// shares its names with every other root, so a name only counts when
    /// the template is in this directory.
    root: Option<std::path::PathBuf>,
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
        let anonymous = self.anonymous_component_namespaces();
        let livewire_views = self.livewire_views();
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
                .chain(
                    livewire_views
                        .namespaces
                        .iter()
                        .map(|(namespace, _)| namespace.clone()),
                )
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
            && (!discovery.livewire.is_empty()
                || !livewire_aliases.is_empty()
                || livewire_views.is_active())
            && self.find_or_load_class("Livewire\\Component").is_some()
            && !livewire_views.open;
        ComponentIndex {
            discovery,
            anonymous,
            known_prefixes,
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
            livewire_view_prefixes: if livewire_judgeable {
                self.livewire_view_prefixes(&livewire_views)
            } else {
                Vec::new()
            },
            open_anonymous_paths: livewire_views.open,
        }
    }

    /// The view-name prefixes Livewire's finder reaches a view-based
    /// component under.  A component location is named relative to the
    /// first view root that holds it, which is the name the template index
    /// records it under: Livewire 4 makes the location a view root of its
    /// own, but only after the configured ones.
    fn livewire_view_prefixes(
        &self,
        livewire: &super::livewire_views::LivewireViews,
    ) -> Vec<LivewireViewPrefix> {
        if !livewire.is_active() {
            return FALLBACK_VIEW_DIRECTORIES
                .iter()
                .map(|prefix| LivewireViewPrefix {
                    namespace: None,
                    prefix: (*prefix).to_string(),
                    root: None,
                })
                .collect();
        }
        let roots = self.laravel_view_roots();
        let mut prefixes: Vec<LivewireViewPrefix> = livewire
            .locations
            .iter()
            .filter_map(|location| {
                let canonical = dunce::canonicalize(location).unwrap_or_else(|_| location.clone());
                let rel = roots.iter().find_map(|root| {
                    location
                        .strip_prefix(&root.path)
                        .ok()
                        .or_else(|| canonical.strip_prefix(root.canonical.as_ref()?).ok())
                })?;
                let rel = rel.to_string_lossy().replace(['/', '\\'], ".");
                Some(if rel.is_empty() {
                    LivewireViewPrefix {
                        namespace: None,
                        prefix: rel,
                        root: Some(location.clone()),
                    }
                } else {
                    LivewireViewPrefix {
                        namespace: None,
                        prefix: format!("{rel}."),
                        root: None,
                    }
                })
            })
            .collect();
        prefixes.extend(
            livewire
                .namespaces
                .iter()
                .map(|(namespace, _)| LivewireViewPrefix {
                    namespace: Some(namespace.clone()),
                    prefix: format!("{namespace}::"),
                    root: None,
                }),
        );
        prefixes
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
        match kind.blade_name(name) {
            Some(name) => self.is_unknown_blade(&name) && !self.blade_class_exists(backend, &name),
            None => self.is_unknown_livewire(name) && !self.livewire_class_exists(backend, name),
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
        if self.open_anonymous_paths && !name.contains("::") {
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
                prefix != MAIL_PREFIX && self.known_prefixes.iter().any(|known| known == prefix)
            }
            None => !self.open_alias_prefixes.iter().any(|prefix| {
                prefix.is_empty()
                    || name
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|rest| rest.starts_with('-'))
            }),
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
        if BUILTIN_LIVEWIRE_TAGS.contains(&name) {
            return false;
        }
        // A namespace only Livewire's own config declares is one whose
        // components are all view-based; any other may hold classes this
        // cannot enumerate.
        if let Some((namespace, _)) = name.split_once("::")
            && !self
                .livewire_view_prefixes
                .iter()
                .any(|registered| registered.namespace.as_deref() == Some(namespace))
        {
            return false;
        }
        !(self.discovery.livewire_component(name).is_some()
            || aliases.iter().any(|alias| alias == name)
            || self.livewire_view_resolves(name))
    }

    /// Whether a view-based component answers to the Livewire tag `name`:
    /// a template in one of the directories Livewire's finder looks in,
    /// with or without Livewire 4's marker, either on its own, as the index
    /// of a directory of the same name, or as a multi-file component's
    /// directory.
    fn livewire_view_resolves(&self, name: &str) -> bool {
        let (namespace, path) = match name.split_once("::") {
            Some((namespace, path)) => (Some(namespace), path),
            None => (None, name),
        };
        let (lead, last) = match path.rsplit_once('.') {
            Some((dirs, last)) => (format!("{dirs}."), last),
            None => (String::new(), path),
        };
        self.livewire_view_prefixes
            .iter()
            .filter(|registered| registered.namespace.as_deref() == namespace)
            .any(|LivewireViewPrefix { prefix, root, .. }| {
                LIVEWIRE_MARKERS.iter().any(|marker| {
                    [
                        // A single file, on its own, as an index, or named
                        // after its directory.
                        format!("{prefix}{lead}{marker}{last}"),
                        format!("{prefix}{lead}{last}.{marker}index"),
                        format!("{prefix}{lead}{last}.{marker}{last}"),
                        // A multi-file component's directory, holding a
                        // template named after it without the marker.
                        format!("{prefix}{lead}{marker}{last}.{last}"),
                        format!("{prefix}{lead}{last}.{marker}index.index"),
                        format!("{prefix}{lead}{last}.{marker}{last}.{last}"),
                    ]
                    .iter()
                    .any(|view| self.livewire_view_exists(view, root.as_deref()))
                })
            })
    }

    /// Whether the template index holds `view`, in `root` when one is
    /// given.  An earlier view root can hold a template of the same name,
    /// which the index then records instead, so that case falls back to
    /// looking in `root` itself.
    fn livewire_view_exists(&self, view: &str, root: Option<&std::path::Path>) -> bool {
        let Some(path) = self.discovery.views.get(view) else {
            return false;
        };
        let Some(root) = root else {
            return true;
        };
        path.starts_with(root)
            || [".blade.php", ".php"].iter().any(|extension| {
                root.join(format!("{}{extension}", view.replace('.', "/")))
                    .is_file()
            })
    }

    /// Every name a tag of `kind` can be written with, sorted, for
    /// suggesting what a misspelt one meant.
    pub(crate) fn known_names(&self, kind: TagKind) -> Vec<String> {
        let mut names: Vec<String> = match kind {
            TagKind::Blade | TagKind::Flux => {
                let views: Vec<String> = self.discovery.views.keys().cloned().collect();
                self.discovery
                    .components
                    .keys()
                    .cloned()
                    .chain(component_tag_names(&views, &self.anonymous))
                    .chain(self.aliases.iter().cloned())
                    .filter_map(|name| Some(kind.tag_name_for(&name)?.to_string()))
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
