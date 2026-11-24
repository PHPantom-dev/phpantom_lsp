//! How a component tag's name and a view's name map onto each other.
//!
//! Laravel's `ComponentTagCompiler` addresses an anonymous component by
//! its view: `<x-brand.boxes>` renders `components.brand.boxes`, a
//! namespaced `<x-webshop::brand.boxes>` renders
//! `webshop::components.brand.boxes`, and a directory registered with
//! `Blade::anonymousComponentNamespace()` or `anonymousComponentPath()`
//! puts its templates behind a prefix of their own. An index component
//! (`card.index`, or `card.card`) also answers to its directory alone.
//!
//! This module is that mapping in both directions, for the readers that
//! need to know which template a tag renders (completion, go-to-definition,
//! the outline) and which tags a template answers to (call-site inference
//! into the template). The tags themselves are read from the source by
//! [`super::component_tags`].

use crate::Backend;

/// One anonymous-component registration in effect: the tag prefix it is
/// addressed under, and where its templates live.
///
/// `Blade::anonymousComponentNamespace('components', 'webshop')` is the
/// prefix `webshop` over the view directory `components`.  An empty prefix
/// is the prefix-less `Blade::anonymousComponentPath()` registration, whose
/// templates every un-namespaced tag can address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnonymousNamespace {
    pub(crate) prefix: String,
    /// What a template's view name starts with: a view directory in dot
    /// notation (`components`, joined to the rest with a `.`), or a whole
    /// view namespace ending in `::` for a directory outside every view
    /// root, which Laravel registers as a view namespace of its own.
    pub(crate) directory: String,
    /// Whether the registration is an `anonymousComponentPath()` one, which
    /// `ComponentTagCompiler::guessAnonymousComponentUsingPaths` also
    /// reaches with a tag that names no namespace at all, whatever prefix
    /// the path was given.
    pub(crate) path: bool,
}

impl AnonymousNamespace {
    /// A registration over a view directory in dot notation.
    pub(crate) fn new(prefix: &str, directory: &str, path: bool) -> Self {
        Self {
            prefix: prefix.to_string(),
            directory: directory.to_string(),
            path,
        }
    }
}

/// The view namespace an `anonymousComponentPath()` directory outside every
/// view root is read under.
///
/// Laravel registers every such path as a view namespace named after a hash
/// of its prefix, which no template ever writes; this stands in for it.  It
/// is not a name a view can be addressed by, so the view-name readers leave
/// it out ([`is_anonymous_path_view_namespace`]).
pub(crate) fn anonymous_path_view_namespace(prefix: &str) -> String {
    format!("{ANONYMOUS_PATH_VIEW_NAMESPACE}{prefix}")
}

/// Whether a view name is one read under [`anonymous_path_view_namespace`].
pub(crate) fn is_anonymous_path_view_name(view_name: &str) -> bool {
    view_name.starts_with(ANONYMOUS_PATH_VIEW_NAMESPACE)
}

const ANONYMOUS_PATH_VIEW_NAMESPACE: &str = "__anonymous_component_path_";

impl Backend {
    /// The anonymous-component registrations in effect — what
    /// `ComponentTagCompiler::guessAnonymousComponentUsingNamespaces` and
    /// its path-keyed twin read before falling back to `components.`.
    ///
    /// A path registration names a directory on disk rather than a view
    /// prefix, so it is rewritten as the view directory that directory sits
    /// in.  One outside every configured view root (a package's own
    /// templates, as Flux registers them) is read under a view namespace of
    /// its own, the way Laravel reads it ([`Self::anonymous_component_view_roots`]).
    pub(crate) fn anonymous_component_namespaces(&self) -> Vec<AnonymousNamespace> {
        self.anonymous_component_registrations().0
    }

    /// The directories of the path registrations outside every view root,
    /// as `(view namespace, directory)`, for the view index to scan.
    pub(crate) fn anonymous_component_view_roots(&self) -> Vec<(String, std::path::PathBuf)> {
        self.anonymous_component_registrations().1
    }

    fn anonymous_component_registrations(
        &self,
    ) -> (Vec<AnonymousNamespace>, Vec<(String, std::path::PathBuf)>) {
        let (namespaces, mut paths) = {
            let resources = self.laravel.provider_resources.read();
            (
                resources.anonymous_component_namespaces.clone(),
                resources.anonymous_component_paths.clone(),
            )
        };
        // Livewire 4 registers every component location as a prefix-less
        // anonymous component path and every component namespace as one
        // under its name, skipping a directory that does not exist.
        let livewire = self.livewire_views();
        paths.extend(
            livewire
                .locations
                .iter()
                .map(|location| (String::new(), location.clone()))
                .chain(livewire.namespaces.iter().cloned())
                .filter(|(_, path)| path.is_dir()),
        );
        let mut registrations: Vec<AnonymousNamespace> = namespaces
            .iter()
            .map(|(prefix, directory)| AnonymousNamespace::new(prefix, directory, false))
            .collect();
        let mut roots = Vec::new();
        if paths.is_empty() {
            return (registrations, roots);
        }
        let view_roots = self.laravel_view_roots();
        for (prefix, path) in paths {
            let path = dunce::canonicalize(&path).unwrap_or(path);
            let directory = view_roots.iter().find_map(|root| {
                let root = root.canonical.as_ref().unwrap_or(&root.path);
                let rel = path.strip_prefix(root).ok()?;
                Some(rel.to_string_lossy().replace(['/', '\\'], "."))
            });
            let directory = match directory {
                Some(directory) => directory,
                None => {
                    let namespace = anonymous_path_view_namespace(&prefix);
                    let directory = format!("{namespace}::");
                    roots.push((namespace, path));
                    directory
                }
            };
            registrations.push(AnonymousNamespace::new(&prefix, &directory, true));
        }
        (registrations, roots)
    }

    /// The view an `<x-…>` tag with no class behind it renders: an
    /// anonymous component is a template, so its name is the closest
    /// thing it has to a class name.
    ///
    /// The first candidate the project ships wins, in the order
    /// [`view_names_for_component_tag`] tries them.
    ///
    /// `anonymous` is passed in rather than read here because resolving
    /// the registrations touches the filesystem, and a caller asking
    /// about every tag in a file only has to do that once.
    pub(crate) fn anonymous_component_view(
        &self,
        tag: &str,
        anonymous: &[AnonymousNamespace],
    ) -> Option<String> {
        let discovery = self.blade_discovery();
        view_names_for_component_tag(tag, anonymous)
            .into_iter()
            .find(|name| discovery.views.contains_key(name))
    }
}

/// The view names a new anonymous component written as `<x-{tag}>` could
/// be created as, best first: the directory registered for the tag's own
/// prefix, then the `components.` convention, then a prefix-less
/// registration.  Only the plain forms, not the index-component ones,
/// since a new component has no reason to be an index, and only under the
/// application's own view roots: a directory outside them is a package's.
pub(crate) fn view_names_to_create_for_component_tag(
    tag: &str,
    anonymous: &[AnonymousNamespace],
) -> Vec<String> {
    let mut names = Vec::new();
    let mut push = |name: String| {
        if !names.contains(&name) {
            names.push(name);
        }
    };
    let in_view_root = anonymous
        .iter()
        .filter(|registration| !registration.directory.ends_with("::"));
    for registration in in_view_root.clone().filter(|r| !r.prefix.is_empty()) {
        if let Some(rest) = tag
            .strip_prefix(registration.prefix.as_str())
            .and_then(|rest| rest.strip_prefix("::"))
        {
            push(guess_view_name(rest, &registration.directory));
        }
    }
    push(guess_view_name(tag, "components"));
    for registration in in_view_root.filter(|r| r.prefix.is_empty()) {
        if !tag.contains("::") {
            push(guess_view_name(tag, &registration.directory));
        }
    }
    names
}

/// The bare tag names (without the `x-` prefix) a Blade file's own view
/// names make it addressable by: `components.brand.boxes` becomes
/// `brand.boxes` (so `<x-brand.boxes>` matches it), and a namespaced name
/// drops the `components.` segment after the namespace the same way
/// Laravel's `ComponentTagCompiler::guessViewName` inserts it —
/// `webshop::components.brand.boxes` is what `<x-webshop::brand.boxes>`
/// compiles to.
///
/// `anonymous` adds the directories a project registered a tag prefix for,
/// under which a view is addressed without the `components.` convention at
/// all: with `('webshop', 'components')` registered,
/// `components.pages.boxes` is also what `<x-webshop::pages.boxes>` names.
///
/// A view name that no rule makes a tag of contributes nothing.
pub(crate) fn component_tag_names(
    view_names: &[String],
    anonymous: &[AnonymousNamespace],
) -> Vec<String> {
    // Every view name in the project reaches this, so the seen set is
    // kept beside the ordered list rather than scanning it per tag.
    let mut tags = TagList::default();
    for name in view_names {
        if let Some(tag) = component_tag_for_view_name(name) {
            push_tag(tag, &mut tags);
        }
        for registration in anonymous {
            let Some(rest) = strip_view_directory(name, &registration.directory) else {
                continue;
            };
            // A path registered under a prefix also answers to the bare
            // name, but only after every other registration had its turn,
            // so the prefixed name is the one that reliably reaches it.
            push_tag(
                if registration.prefix.is_empty() {
                    rest.to_string()
                } else {
                    format!("{}::{rest}", registration.prefix)
                },
                &mut tags,
            );
        }
    }
    tags.tags
}

/// The tags collected so far, in the order they were found, with the set
/// that answers "already collected?" without walking them.
#[derive(Default)]
struct TagList {
    tags: Vec<String>,
    seen: std::collections::HashSet<String>,
}

impl TagList {
    fn push(&mut self, tag: String) {
        if self.seen.insert(tag.clone()) {
            self.tags.push(tag);
        }
    }
}

/// Add a tag and, for the view of an index component, the shorter tag it
/// also answers to.
///
/// Laravel falls back to `{view}.index` and to `{view}.{last segment}` when
/// a component's own view name does not exist, so `components.card.index`
/// and `components.card.card` are both what `<x-card>` reaches.
fn push_tag(tag: String, tags: &mut TagList) {
    if let Some(shorter) = index_component_tag(&tag) {
        tags.push(shorter);
    }
    tags.push(tag);
}

/// The tag an index component's view name is *also* addressable by:
/// `card.index` and `card.card` both answer to `<x-card>`.
fn index_component_tag(tag: &str) -> Option<String> {
    let (head, last) = tag.rsplit_once('.')?;
    if head.is_empty() || head.ends_with("::") {
        return None;
    }
    let previous = head.rsplit_once('.').map_or(head, |(_, seg)| seg);
    let previous = previous.rsplit_once("::").map_or(previous, |(_, seg)| seg);
    (last == "index" || last == previous).then(|| head.to_string())
}

/// The component name a view under a registered directory is addressed by,
/// or `None` for a view that does not sit under it.
fn strip_view_directory<'a>(view_name: &'a str, directory: &str) -> Option<&'a str> {
    if directory.is_empty() {
        return Some(view_name);
    }
    let rest = view_name.strip_prefix(directory)?;
    if directory.ends_with("::") {
        return Some(rest);
    }
    rest.strip_prefix('.')
}

/// The tag name a view makes a component addressable by, or `None` for a
/// view outside the `components.` namespace, which no `<x-…>` tag names.
///
/// A namespaced view keeps its namespace (`nightshade::calendar`), and
/// drops a `components.` segment a package puts its component views under,
/// since the class behind them sits directly in the registered namespace.
pub(crate) fn component_tag_for_view_name(view_name: &str) -> Option<String> {
    if is_anonymous_path_view_name(view_name) {
        return None;
    }
    match view_name.split_once("::") {
        Some((namespace, rest)) => {
            let bare = rest.strip_prefix("components.").unwrap_or(rest);
            Some(format!("{namespace}::{bare}"))
        }
        None => view_name.strip_prefix("components.").map(str::to_string),
    }
}

/// The inverse of [`component_tag_names`]: the view names a tag written as
/// `<x-{tag}>` can resolve to, in the order Laravel's
/// `ComponentTagCompiler::componentClass` tries them — the `components.`
/// convention first (with the `components.` prefix going after the
/// namespace when the tag has one), then each registered anonymous
/// directory whose prefix the tag is written under (and every path
/// registration, for a tag that names no namespace).
///
/// Each of those is tried as itself, then as its `.index` and repeated-last
/// segment forms, which is how an index component is addressed by its
/// directory alone.
pub(crate) fn view_names_for_component_tag(
    tag: &str,
    anonymous: &[AnonymousNamespace],
) -> Vec<String> {
    let mut names = Vec::new();
    push_view_name(guess_view_name(tag, "components"), tag, &mut names);
    for registration in anonymous {
        let Some(rest) = strip_tag_prefix(tag, registration) else {
            continue;
        };
        push_view_name(
            guess_view_name(rest, &registration.directory),
            rest,
            &mut names,
        );
    }
    names
}

/// Laravel's `ComponentTagCompiler::guessViewName`: the directory becomes
/// the view's prefix, and goes after the namespace when the component name
/// carries one.  A directory that is a whole view namespace (`ns::`) is
/// the view's prefix as it stands.
fn guess_view_name(component: &str, directory: &str) -> String {
    if directory.is_empty() {
        return component.to_string();
    }
    if directory.ends_with("::") {
        return format!("{directory}{component}");
    }
    match component.split_once("::") {
        Some((namespace, rest)) => format!("{namespace}::{directory}.{rest}"),
        None => format!("{directory}.{component}"),
    }
}

/// Add a candidate view name and the two an index component also answers
/// to, skipping the ones already recorded.
fn push_view_name(view_name: String, component: &str, names: &mut Vec<String>) {
    let last = component.rsplit(['.', ':']).next().unwrap_or(component);
    let mut candidates = vec![format!("{view_name}.index")];
    if !last.is_empty() {
        candidates.push(format!("{view_name}.{last}"));
    }
    candidates.insert(0, view_name);
    for candidate in candidates {
        if !names.contains(&candidate) {
            names.push(candidate);
        }
    }
}

/// The component name a tag addresses under a registration, or `None` for
/// a tag the registration does not reach.
///
/// A tag written under a namespace reaches only the registrations made for
/// that prefix.  One that names no namespace reaches a prefix-less
/// registration, and every path registration whatever its prefix, which
/// `guessAnonymousComponentUsingPaths` only skips for a tag written under
/// another namespace.
fn strip_tag_prefix<'a>(tag: &'a str, registration: &AnonymousNamespace) -> Option<&'a str> {
    if !tag.contains("::") {
        return (registration.prefix.is_empty() || registration.path).then_some(tag);
    }
    if registration.prefix.is_empty() {
        return None;
    }
    tag.strip_prefix(registration.prefix.as_str())?
        .strip_prefix("::")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registrations a project with no `anonymousComponent…` call has.
    const NONE: &[AnonymousNamespace] = &[];

    fn registered(prefix: &str, directory: &str) -> Vec<AnonymousNamespace> {
        vec![AnonymousNamespace::new(prefix, directory, false)]
    }

    fn registered_path(prefix: &str, directory: &str) -> Vec<AnonymousNamespace> {
        vec![AnonymousNamespace::new(prefix, directory, true)]
    }

    #[test]
    fn component_tag_names_strips_the_components_prefix() {
        assert_eq!(
            component_tag_names(&["components.brand.boxes".to_string()], NONE),
            vec!["brand.boxes"]
        );
    }

    #[test]
    fn component_tag_names_strips_components_after_a_namespace() {
        assert_eq!(
            component_tag_names(&["webshop::components.brand.boxes".to_string()], NONE),
            vec!["webshop::brand.boxes"]
        );
        assert_eq!(
            component_tag_names(&["mail::message".to_string()], NONE),
            vec!["mail::message"]
        );
    }

    #[test]
    fn component_tag_names_skips_a_bare_non_component_view() {
        assert!(component_tag_names(&["emails.welcome".to_string()], NONE).is_empty());
    }

    /// `Blade::anonymousComponentNamespace('components', 'webshop')` makes
    /// the same template addressable under the registered prefix as well as
    /// by the un-registered `components.` convention.
    #[test]
    fn a_registered_prefix_adds_a_tag_for_the_directory_it_names() {
        assert_eq!(
            component_tag_names(
                &["components.pages.boxes".to_string()],
                &registered("webshop", "components"),
            ),
            vec!["pages.boxes", "webshop::pages.boxes"]
        );
    }

    /// A registration whose directory the view does not sit under names
    /// nothing about it.
    #[test]
    fn a_registration_for_another_directory_adds_no_tag() {
        assert_eq!(
            component_tag_names(
                &["components.pages.boxes".to_string()],
                &registered("webshop", "theme.components"),
            ),
            vec!["pages.boxes"]
        );
    }

    /// A prefix-less `anonymousComponentPath()` registration puts its whole
    /// directory behind bare tag names.
    #[test]
    fn a_prefix_less_registration_addresses_its_directory_bare() {
        assert_eq!(
            component_tag_names(&["ui.alert".to_string()], &registered_path("", "ui")),
            vec!["alert"]
        );
    }

    /// Laravel falls back to `{view}.index` and to the repeated-directory
    /// form, so both are what the directory's own tag reaches.
    #[test]
    fn an_index_component_answers_to_its_directory_alone() {
        assert_eq!(
            component_tag_names(&["components.card.index".to_string()], NONE),
            vec!["card", "card.index"]
        );
        assert_eq!(
            component_tag_names(&["components.card.card".to_string()], NONE),
            vec!["card", "card.card"]
        );
        assert_eq!(
            component_tag_names(&["components.index".to_string()], NONE),
            vec!["index"],
            "a component named `index` is not the index of anything"
        );
    }

    #[test]
    fn view_names_for_component_tag_round_trips() {
        assert_eq!(
            view_names_for_component_tag("brand.boxes", NONE),
            vec![
                "components.brand.boxes",
                "components.brand.boxes.index",
                "components.brand.boxes.boxes"
            ]
        );
        assert_eq!(
            view_names_for_component_tag("webshop::brand.boxes", NONE),
            vec![
                "webshop::components.brand.boxes",
                "webshop::components.brand.boxes.index",
                "webshop::components.brand.boxes.boxes"
            ]
        );
    }

    /// A tag written under a registered prefix names the view in the
    /// registered directory on top of the un-registered fallback, which is
    /// the one Laravel tries first.
    #[test]
    fn a_registered_prefix_adds_the_view_it_names() {
        assert_eq!(
            view_names_for_component_tag(
                "webshop::pages.boxes",
                &registered("webshop", "components")
            ),
            vec![
                "webshop::components.pages.boxes",
                "webshop::components.pages.boxes.index",
                "webshop::components.pages.boxes.boxes",
                "components.pages.boxes",
                "components.pages.boxes.index",
                "components.pages.boxes.boxes",
            ]
        );
    }

    /// A namespaced tag belongs to its own namespace, so a prefix-less path
    /// registration does not claim it.
    #[test]
    fn a_prefix_less_registration_ignores_a_namespaced_tag() {
        assert_eq!(
            view_names_for_component_tag("mail::message", &registered_path("", "ui")),
            vec![
                "mail::components.message",
                "mail::components.message.index",
                "mail::components.message.message"
            ]
        );
    }

    /// `guessAnonymousComponentUsingPaths` lets a tag that names no
    /// namespace through to every path, so a path registered under a prefix
    /// still answers to the bare name.  A namespace registration does not.
    #[test]
    fn a_prefixed_path_also_answers_a_bare_tag() {
        assert!(
            view_names_for_component_tag("button", &registered_path("flux", "ui"))
                .contains(&"ui.button".to_string())
        );
        assert!(
            !view_names_for_component_tag("button", &registered("flux", "ui"))
                .contains(&"ui.button".to_string())
        );
        assert!(
            !view_names_for_component_tag("other::button", &registered_path("flux", "ui"))
                .contains(&"ui.button".to_string())
        );
    }

    /// A path outside every view root is read under a view namespace of its
    /// own, which addresses its templates directly and names no tag itself.
    #[test]
    fn a_path_outside_the_view_roots_is_read_under_its_own_namespace() {
        let namespace = anonymous_path_view_namespace("flux");
        let directory = format!("{namespace}::");
        let registration = registered_path("flux", &directory);
        assert_eq!(
            view_names_for_component_tag("flux::button", &registration)[3..],
            [
                format!("{namespace}::button"),
                format!("{namespace}::button.index"),
                format!("{namespace}::button.button"),
            ]
        );
        assert_eq!(
            component_tag_names(&[format!("{namespace}::button.index")], &registration),
            vec!["flux::button", "flux::button.index"]
        );
        assert_eq!(
            view_names_to_create_for_component_tag("flux::button", &registration),
            vec!["flux::components.button"],
            "a package's own directory is no place to create a component"
        );
    }
}
