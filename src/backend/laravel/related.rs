//! The classes and files a Laravel class is conventionally paired with:
//! a model's factory, policy, observers, migrations, API resources and
//! custom collection, the way back from a factory, policy or observer to
//! its model, and the links between an event and its listeners.
//!
//! Each link follows the rule Laravel itself resolves it by (the
//! `HasFactory` lookup, `Gate::getPolicyFor()`, `#[ObservedBy]` and
//! `::observe()`, the resource guesser, and event discovery's reading of a
//! listener's `handle()` type hint), so a link is only offered where the
//! framework would make the same connection.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::Location;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::Fqn;
use crate::types::{ClassInfo, ClassLikeKind, Visibility};
use crate::virtual_members::laravel;

/// One class or file a class links to.
pub(crate) struct RelatedLink {
    /// What the target is to the class, for one target and for several
    /// (`Observer`, `observers`).
    pub kind: (&'static str, &'static str),
    /// The target's display name.
    pub label: String,
    pub location: Location,
    /// The method of the class the link belongs to, or `None` for the
    /// class itself.
    pub anchor: Option<Atom>,
}

/// A listener class Laravel's event discovery would register, with the
/// method and the events that method's first parameter names.
pub(crate) struct DiscoveredListener {
    listener: String,
    method: Atom,
    events: Vec<String>,
}

impl Backend {
    /// Every class or file `class` is conventionally paired with, in a
    /// stable order.  Empty outside a Laravel project.
    pub(crate) fn laravel_related_links(&self, class: &ClassInfo) -> Vec<RelatedLink> {
        if class.keyword_offset == 0 || !self.resolved_class_cache.read().laravel().is_enabled() {
            return Vec::new();
        }
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        let mut links = Vec::new();
        if class.kind == ClassLikeKind::Class {
            if laravel::extends_eloquent_model(class, &class_loader) {
                self.push_model_links(class, &mut links);
            } else if laravel::extends_eloquent_factory(class, &class_loader) {
                self.push_factory_model_link(class, &mut links);
            }
            let short = crate::util::short_name(&class.name);
            if short.len() > "Policy".len() && short.ends_with("Policy") {
                self.push_policy_model_links(class, &mut links);
            }
            if short.len() > "Observer".len() && short.ends_with("Observer") {
                self.push_observer_model_links(class, &mut links);
            }
        }
        // Discovery has to see every listener, so like the implementation
        // lenses these wait for the workspace index.
        if self
            .workspace_index
            .indexed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.push_event_links(class, &mut links);
        }
        links
    }

    fn push_model_links(&self, model: &ClassInfo, links: &mut Vec<RelatedLink>) {
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        let model_fqn = model.fqn();
        let push_class = |links: &mut Vec<RelatedLink>, kind, fqn: &str| {
            if let Some(link) = self.class_link(kind, fqn, None) {
                links.push(link);
            }
        };

        let factory = laravel::factory_for_model(model, &class_loader);
        if let Some(factory) = factory.base_name()
            && factory != laravel::FACTORY_FQN
        {
            push_class(links, ("Factory", "factories"), factory);
        }

        if let Some(policy) = laravel::gates::policy_class_for_model(self, &model_fqn) {
            push_class(links, ("Policy", "policies"), &policy.fqn());
        }

        for observer in self.model_observers(model) {
            push_class(links, ("Observer", "observers"), &observer);
        }

        if let Some((connection, table)) =
            laravel::model_connection_and_table(model, Some(&self.resolved_class_cache))
        {
            let index = self.laravel.schema_index.read();
            for (path, content, offset) in index.table_migrations(&connection, &table) {
                let Some(uri) = crate::util::parse_url(&crate::util::path_to_uri(path)) else {
                    continue;
                };
                let label = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or(&table)
                    .to_string();
                links.push(RelatedLink {
                    kind: ("Migration", "migrations"),
                    label,
                    location: crate::definition::point_location(
                        uri,
                        crate::text_position::offset_to_position(content, offset as usize),
                    ),
                    anchor: None,
                });
            }
        }

        let related = model.laravel().and_then(|l| l.related_classes.as_deref());
        let guessed = guessed_resource_names(&model_fqn);
        let resource = related
            .and_then(|r| r.resource.clone())
            .filter(|fqn| self.find_or_load_class(fqn).is_some())
            .or_else(|| {
                guessed
                    .iter()
                    .find(|fqn| self.find_or_load_class(fqn).is_some())
                    .cloned()
            });
        if let Some(resource) = &resource {
            push_class(links, ("Resource", "resources"), resource);
        }
        let collection = related
            .and_then(|r| r.resource_collection.clone())
            .filter(|fqn| self.find_or_load_class(fqn).is_some())
            .or_else(|| {
                guessed
                    .iter()
                    .map(|fqn| format!("{fqn}Collection"))
                    .find(|fqn| self.find_or_load_class(fqn).is_some())
            });
        if let Some(collection) = &collection {
            push_class(
                links,
                ("Resource collection", "resource collections"),
                collection,
            );
        }

        let declared_collection = |candidate: &ClassInfo| {
            candidate
                .laravel()
                .and_then(|l| l.custom_collection.as_ref())
                .and_then(|ty| ty.base_name())
                .map(str::to_string)
        };
        let custom_collection = declared_collection(model).or_else(|| {
            crate::inheritance::ancestors(model, &class_loader)
                .find_map(|(_, parent)| declared_collection(&parent))
        });
        // `Model` itself declares Eloquent's own collection, which is no
        // customisation.
        if let Some(collection) = custom_collection
            && collection.trim_start_matches('\\') != crate::types::ELOQUENT_COLLECTION_FQN
        {
            push_class(links, ("Collection", "collections"), &collection);
        }
    }

    /// The observers registered for `model`: by its own or an ancestor's
    /// `#[ObservedBy]`, which Laravel reads up the class chain, and by a
    /// service provider.
    fn model_observers(&self, model: &ClassInfo) -> Vec<String> {
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        let model_fqn = model.fqn();
        let mut observers: Vec<String> = Vec::new();
        let mut push = |fqn: &str| {
            if !observers
                .iter()
                .any(|known| known.eq_ignore_ascii_case(fqn))
            {
                observers.push(fqn.to_string());
            }
        };
        let declared = |class: &ClassInfo| {
            class
                .laravel()
                .and_then(|l| l.related_classes.as_deref())
                .map(|related| related.observers.clone())
                .unwrap_or_default()
        };
        declared(model).iter().for_each(|fqn| push(fqn));
        for (_, parent) in crate::inheritance::ancestors(model, &class_loader) {
            declared(&parent).iter().for_each(|fqn| push(fqn));
        }
        for registration in self.laravel.events.read().observers() {
            if registration.model_fqn.eq_ignore_ascii_case(&model_fqn) {
                push(&registration.observer_fqn);
            }
        }
        observers
    }

    fn push_factory_model_link(&self, factory: &ClassInfo, links: &mut Vec<RelatedLink>) {
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        if let Some(model) = laravel::factory_model_type(factory, &class_loader)
            && let Some(model) = model.base_name()
            && let Some(link) = self.class_link(("Model", "models"), model, None)
        {
            links.push(link);
        }
    }

    /// The models a policy governs: the ones a registration pairs it with,
    /// and the project models named like it that Laravel would resolve to
    /// it by `#[UsePolicy]` or by its naming guess.
    fn push_policy_model_links(&self, policy: &ClassInfo, links: &mut Vec<RelatedLink>) {
        let policy_fqn = policy.fqn();
        let mut models = self.laravel.gates.read().models_for_policy(&policy_fqn);
        let model_short = crate::util::short_name(&policy.name)
            .strip_suffix("Policy")
            .unwrap_or_default();
        for candidate in self.project_model_fqns_named(model_short) {
            if !models.contains(&candidate)
                && laravel::gates::policy_class_for_model(self, &candidate)
                    .is_some_and(|resolved| resolved.fqn() == policy_fqn)
            {
                models.push(candidate);
            }
        }
        for model in models {
            if let Some(link) = self.class_link(("Model", "models"), &model, None) {
                links.push(link);
            }
        }
    }

    /// The models an observer is registered for: by a service provider,
    /// and by the `#[ObservedBy]` of a project model named like it.
    fn push_observer_model_links(&self, observer: &ClassInfo, links: &mut Vec<RelatedLink>) {
        let observer_fqn = observer.fqn();
        let mut models: Vec<String> = self
            .laravel
            .events
            .read()
            .observers()
            .iter()
            .filter(|registration| {
                registration
                    .observer_fqn
                    .eq_ignore_ascii_case(&observer_fqn)
            })
            .map(|registration| registration.model_fqn.clone())
            .collect();
        let model_short = crate::util::short_name(&observer.name)
            .strip_suffix("Observer")
            .unwrap_or_default();
        for candidate in self.project_model_fqns_named(model_short) {
            if models.contains(&candidate) {
                continue;
            }
            let Some(model) = self.find_or_load_class(&candidate) else {
                continue;
            };
            if self
                .model_observers(&model)
                .iter()
                .any(|fqn| fqn.eq_ignore_ascii_case(&observer_fqn))
            {
                models.push(candidate);
            }
        }
        for model in models {
            if let Some(link) = self.class_link(("Model", "models"), &model, None) {
                links.push(link);
            }
        }
    }

    /// The project classes whose short name is `short`, which are the
    /// models a `{Model}Policy` or `{Model}Observer` is named after.
    fn project_model_fqns_named(&self, short: &str) -> Vec<String> {
        if short.is_empty() {
            return Vec::new();
        }
        let mut fqns: Vec<String> = self
            .symbols
            .fqn_uri_index
            .read()
            .iter()
            .filter(|(fqn, uri)| {
                !uri.contains("/vendor/")
                    && crate::util::short_name(fqn).eq_ignore_ascii_case(short)
            })
            .map(|(fqn, _)| fqn.to_string())
            .collect();
        fqns.sort();
        fqns
    }

    /// The listeners of an event, on the class, and the events a listener
    /// handles, on the method that handles each.
    fn push_event_links(&self, class: &ClassInfo, links: &mut Vec<RelatedLink>) {
        let class_fqn = class.fqn();
        let registrations = self.laravel.events.read().listeners().to_vec();
        let discovered = self.cached_discovered_listeners();

        // This class as a listener.
        let mut handled: Vec<(String, Option<Atom>)> = Vec::new();
        for registration in &registrations {
            if registration.listener_fqn.eq_ignore_ascii_case(&class_fqn) {
                let method = registration
                    .method
                    .as_deref()
                    .and_then(|name| own_method(class, name));
                handled.push((registration.event_fqn.clone(), method));
            }
        }
        for listener in discovered.iter() {
            if listener.listener.eq_ignore_ascii_case(&class_fqn) {
                for event in &listener.events {
                    handled.push((event.clone(), Some(listener.method)));
                }
            }
        }
        handled.sort();
        handled.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0) && a.1 == b.1);
        // A registration that names no method and the discovered `handle()`
        // are the same link, which belongs on the method.
        let anchored: Vec<String> = handled
            .iter()
            .filter(|(_, method)| method.is_some())
            .map(|(event, _)| event.to_ascii_lowercase())
            .collect();
        handled.retain(|(event, method)| {
            method.is_some() || !anchored.contains(&event.to_ascii_lowercase())
        });
        for (event, method) in handled {
            if let Some(link) = self.class_link(("Event", "events"), &event, method) {
                links.push(link);
            }
        }

        // This class as an event.  Laravel adds the listeners registered for
        // an interface the event implements, but not those of a parent class.
        let names = self.event_listener_keys(class);
        let listens_to = |event: &str| names.iter().any(|name| name.eq_ignore_ascii_case(event));
        let mut listeners: Vec<(String, Option<String>)> = registrations
            .iter()
            .filter(|registration| listens_to(&registration.event_fqn))
            .map(|registration| {
                (
                    registration.listener_fqn.clone(),
                    registration.method.clone(),
                )
            })
            .collect();
        for listener in discovered.iter() {
            if listener.events.iter().any(|event| listens_to(event)) {
                listeners.push((listener.listener.clone(), Some(listener.method.to_string())));
            }
        }
        listeners.sort();
        listeners.dedup();
        for (listener, method) in listeners {
            // A registration that names no method calls `handle()`, or
            // `__invoke()` on a class without one.
            let location = match method.as_deref() {
                Some(method) => self.own_method_location(&listener, method),
                None => self
                    .own_method_location(&listener, "handle")
                    .or_else(|| self.own_method_location(&listener, "__invoke")),
            }
            .or_else(|| self.class_declaration_location(&listener));
            let Some(location) = location else {
                continue;
            };
            links.push(RelatedLink {
                kind: ("Listener", "listeners"),
                label: crate::util::short_name(&listener).to_string(),
                location,
                anchor: None,
            });
        }
    }

    /// The names a listener of `event` is registered under: the event's own
    /// class and every interface it implements, which is the set Laravel's
    /// dispatcher looks listeners up by.
    fn event_listener_keys(&self, event: &ClassInfo) -> Vec<String> {
        let mut keys = vec![event.fqn().to_string()];
        let mut pending: Vec<Atom> = event.interfaces.to_vec();
        let mut parent = event.parent_class;
        let mut depth = 0;
        while let Some(name) = parent.take()
            && depth < crate::types::MAX_INHERITANCE_DEPTH
        {
            depth += 1;
            if let Some(class) = self.find_or_load_class(&name) {
                pending.extend(class.interfaces.iter().copied());
                parent = class.parent_class;
            }
        }
        while let Some(interface) = pending.pop() {
            let name = interface.trim_start_matches('\\');
            if keys.iter().any(|known| known.eq_ignore_ascii_case(name)) {
                continue;
            }
            keys.push(name.to_string());
            if let Some(class) = self.find_or_load_class(name) {
                pending.extend(class.interfaces.iter().copied());
            }
        }
        keys
    }

    /// Where the class `fqn` declares the method `method` itself, `None`
    /// when it only inherits it.
    fn own_method_location(&self, fqn: &str, method: &str) -> Option<Location> {
        let class = self.find_or_load_class(fqn)?;
        let offset = class
            .methods
            .iter()
            .find(|m| !m.is_virtual && m.name_offset != 0 && m.name.eq_ignore_ascii_case(method))?
            .name_offset;
        let uri = self.class_declaration_uri(fqn)?;
        let content = self.get_file_content(&uri)?;
        Some(crate::definition::point_location(
            crate::util::parse_url(&uri)?,
            crate::text_position::offset_to_position(&content, offset as usize),
        ))
    }

    /// The listeners Laravel's event discovery registers, built on first
    /// use and kept until a listener file changes.
    fn cached_discovered_listeners(&self) -> Arc<[DiscoveredListener]> {
        self.cached_laravel_enumeration(
            &self.laravel.string_key_build_locks.discovered_listeners,
            |cache| cache.discovered_listeners.clone(),
            |cache, listeners| cache.discovered_listeners = Some(listeners),
            || self.discover_listeners().into(),
        )
    }

    /// The listeners Laravel's event discovery registers: every public
    /// `handle*()` or `__invoke()` method of a project class under a
    /// `Listeners` namespace, for the classes its first parameter names.
    fn discover_listeners(&self) -> Vec<DiscoveredListener> {
        let mut fqns: Vec<String> = self
            .symbols
            .fqn_uri_index
            .read()
            .iter()
            .filter(|(fqn, uri)| !uri.contains("/vendor/") && fqn.contains("\\Listeners\\"))
            .map(|(fqn, _)| fqn.to_string())
            .collect();
        fqns.sort();
        let mut listeners = Vec::new();
        for fqn in fqns {
            let Some(class) = self.find_or_load_class(&fqn) else {
                continue;
            };
            if class.kind != ClassLikeKind::Class || class.is_abstract {
                continue;
            }
            for method in class.methods.iter() {
                if method.is_virtual
                    || method.visibility != Visibility::Public
                    || !(method.name.starts_with("handle") || method.name == "__invoke")
                {
                    continue;
                }
                let Some(hint) = method.parameters.first().and_then(|p| p.type_hint.as_ref())
                else {
                    continue;
                };
                let events = hint.top_level_class_names();
                if !events.is_empty() {
                    listeners.push(DiscoveredListener {
                        listener: fqn.clone(),
                        method: method.name,
                        events,
                    });
                }
            }
        }
        listeners
    }

    /// A link to the declaration of the class `fqn`, `None` when it cannot
    /// be located.
    fn class_link(
        &self,
        kind: (&'static str, &'static str),
        fqn: &str,
        anchor: Option<Atom>,
    ) -> Option<RelatedLink> {
        let fqn = fqn.trim_start_matches('\\');
        Some(RelatedLink {
            kind,
            label: crate::util::short_name(fqn).to_string(),
            location: self.class_declaration_location(fqn)?,
            anchor,
        })
    }
}

/// The name `class` declares the method `name` under, matched the way PHP
/// matches method names.
fn own_method(class: &ClassInfo, name: &str) -> Option<Atom> {
    class
        .methods
        .iter()
        .find(|method| method.name.eq_ignore_ascii_case(name))
        .map(|method| method.name)
}

/// The resource classes Laravel's `TransformsToResource::guessResourceName()`
/// tries for a model, in its order: `App\Models\Post` gives
/// `App\Http\Resources\PostResource`, then `App\Http\Resources\Post`.
fn guessed_resource_names(model_fqn: &str) -> Vec<String> {
    let Some((root, after)) = model_fqn.split_once("\\Models\\") else {
        return Vec::new();
    };
    let short = crate::util::short_name(model_fqn);
    let relative = after
        .rsplit_once('\\')
        .map(|(namespace, _)| format!("{namespace}\\"))
        .unwrap_or_default();
    let base = format!("{root}\\Http\\Resources\\{relative}{short}");
    vec![format!("{base}Resource"), base]
}

#[cfg(test)]
mod tests {
    use super::guessed_resource_names;

    #[test]
    fn guesses_resources_the_way_laravel_does() {
        assert_eq!(
            guessed_resource_names("App\\Models\\Post"),
            vec![
                "App\\Http\\Resources\\PostResource".to_string(),
                "App\\Http\\Resources\\Post".to_string(),
            ]
        );
        assert_eq!(
            guessed_resource_names("App\\Models\\Blog\\Post"),
            vec![
                "App\\Http\\Resources\\Blog\\PostResource".to_string(),
                "App\\Http\\Resources\\Blog\\Post".to_string(),
            ]
        );
        assert!(guessed_resource_names("App\\Post").is_empty());
    }
}
