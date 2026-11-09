//! Static scan of the event listeners and model observers a Laravel
//! application registers in its service providers.
//!
//! Laravel wires a listener to an event, and an observer to a model, in
//! three ways a provider can spell:
//!
//! * `Event::listen(OrderShipped::class, SendShipmentNotification::class)`,
//!   with the listener also written as `[Listener::class, 'method']` or
//!   `'Listener@method'`, and the event as a list of several.
//! * An `EventServiceProvider`'s `$listen` array (event → listeners) and
//!   `$observers` array (model → observers).
//! * `Post::observe(PostObserver::class)`, or a list of observers.
//!
//! Listeners Laravel discovers by their `handle()` type hint, and observers a
//! model names in `#[ObservedBy]`, are read off the classes themselves rather
//! than out of a provider, so they are not part of this index.

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;

use super::file_contributions::{Contribution, FileContributions};
use crate::atom::{bytes_to_str, literal_bytes_to_str};
use crate::names::OwnedResolvedNames;

/// FQN of the facade `Event::listen()` goes through.
const EVENT_FACADE_FQN: &str = "Illuminate\\Support\\Facades\\Event";

/// One event → listener registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListenerRegistration {
    pub event_fqn: String,
    pub listener_fqn: String,
    /// The method the registration names, when it names one.
    pub method: Option<String>,
}

/// One model → observer registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObserverRegistration {
    pub model_fqn: String,
    pub observer_fqn: String,
}

/// Everything one file contributes to the event index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EventScan {
    pub listeners: Vec<ListenerRegistration>,
    pub observers: Vec<ObserverRegistration>,
}

impl Contribution for EventScan {
    fn is_empty(&self) -> bool {
        self.listeners.is_empty() && self.observers.is_empty()
    }
}

/// Whether `content` spells a token any registration needs, so only such a
/// file is parsed.  `listen` covers `Event::listen()` and `$listen`,
/// `observe` covers `::observe()` and `$observers`.
pub(crate) fn may_register_events(content: &str) -> bool {
    let bytes = content.as_bytes();
    memchr::memmem::find(bytes, b"listen").is_some()
        || memchr::memmem::find(bytes, b"observe").is_some()
}

/// Extract every literal listener and observer registration from a file's
/// source.
pub(crate) fn scan_event_registrations(content: &str) -> EventScan {
    if !may_register_events(content) {
        return EventScan::default();
    }

    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, content.as_bytes());
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);

    let mut scan = EventScan::default();
    // An explicit worklist rather than recursion: a method chain nests one
    // node per link, deep enough in real files to overflow the stack.
    let mut pending: Vec<Node<'_, '_>> = vec![Node::Program(program)];
    while let Some(node) = pending.pop() {
        match node {
            Node::StaticMethodCall(call) => {
                if let ClassLikeMemberSelector::Identifier(method) = &call.method {
                    collect_static_call(
                        bytes_to_str(method.value),
                        call.class,
                        &call.argument_list,
                        &owned,
                        &mut scan,
                    );
                }
            }
            Node::PlainProperty(plain) => {
                for item in plain.items.iter() {
                    let PropertyItem::Concrete(concrete) = item else {
                        continue;
                    };
                    match concrete.variable.name {
                        b"$listen" => collect_listen_map(concrete.value, &owned, &mut scan),
                        b"$observers" => collect_observer_map(concrete.value, &owned, &mut scan),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        // Pushed in reverse so the walk, and the registrations, follow the
        // source order.
        let start = pending.len();
        node.visit_children(|child| pending.push(child));
        pending[start..].reverse();
    }
    scan
}

/// Read an `Event::listen(…)` or `Model::observe(…)` call into the scan.
fn collect_static_call(
    method: &str,
    class: &Expression<'_>,
    arguments: &ArgumentList<'_>,
    resolved: &OwnedResolvedNames,
    scan: &mut EventScan,
) {
    let Expression::Identifier(ident) = class else {
        return;
    };
    let mut args = arguments.arguments.iter();
    let Some(first) = args.next() else {
        return;
    };

    if method.eq_ignore_ascii_case("observe") {
        let Some(model_fqn) = resolved.class_fqn(ident) else {
            return;
        };
        for observer_fqn in class_list(first.value(), resolved) {
            scan.observers.push(ObserverRegistration {
                model_fqn: model_fqn.clone(),
                observer_fqn,
            });
        }
        return;
    }

    if !method.eq_ignore_ascii_case("listen")
        || !resolved
            .class_fqn(ident)
            .is_some_and(|fqn| fqn.eq_ignore_ascii_case(EVENT_FACADE_FQN))
    {
        return;
    }
    let Some(second) = args.next() else {
        return;
    };
    let Some((listener_fqn, method)) = listener_spec(second.value(), resolved) else {
        return;
    };
    for event_fqn in class_list(first.value(), resolved) {
        scan.listeners.push(ListenerRegistration {
            event_fqn,
            listener_fqn: listener_fqn.clone(),
            method: method.clone(),
        });
    }
}

/// Read a `[Event::class => [Listener::class, …], …]` literal.
fn collect_listen_map(expr: &Expression<'_>, resolved: &OwnedResolvedNames, scan: &mut EventScan) {
    for (event_fqn, value) in class_keyed_entries(expr, resolved) {
        let listeners: Vec<(String, Option<String>)> =
            match crate::parser::array_literal_elements(value) {
                Some(elements) => elements
                    .iter()
                    .filter_map(|element| match element {
                        ArrayElement::Value(value) => listener_spec(value.value, resolved),
                        _ => None,
                    })
                    .collect(),
                None => listener_spec(value, resolved).into_iter().collect(),
            };
        for (listener_fqn, method) in listeners {
            scan.listeners.push(ListenerRegistration {
                event_fqn: event_fqn.clone(),
                listener_fqn,
                method,
            });
        }
    }
}

/// Read a `[Model::class => [Observer::class, …], …]` literal.
fn collect_observer_map(
    expr: &Expression<'_>,
    resolved: &OwnedResolvedNames,
    scan: &mut EventScan,
) {
    for (model_fqn, value) in class_keyed_entries(expr, resolved) {
        for observer_fqn in class_list(value, resolved) {
            scan.observers.push(ObserverRegistration {
                model_fqn: model_fqn.clone(),
                observer_fqn,
            });
        }
    }
}

/// The entries of an array literal whose key names a class.
fn class_keyed_entries<'a, 'ast>(
    expr: &'a Expression<'ast>,
    resolved: &OwnedResolvedNames,
) -> Vec<(String, &'a Expression<'ast>)> {
    let Some(elements) = crate::parser::array_literal_elements(expr) else {
        return Vec::new();
    };
    elements
        .iter()
        .filter_map(|element| match element {
            ArrayElement::KeyValue(kv) => Some((class_name(kv.key, resolved)?, kv.value)),
            _ => None,
        })
        .collect()
}

/// The classes a `Foo::class` or a `[Foo::class, Bar::class]` list names.
fn class_list(expr: &Expression<'_>, resolved: &OwnedResolvedNames) -> Vec<String> {
    match crate::parser::array_literal_elements(expr) {
        Some(elements) => elements
            .iter()
            .filter_map(|element| match element {
                ArrayElement::Value(value) => class_name(value.value, resolved),
                _ => None,
            })
            .collect(),
        None => class_name(expr, resolved).into_iter().collect(),
    }
}

/// The listener class, and the method when one is named, of a listener
/// written as `Listener::class`, `[Listener::class, 'method']`, or
/// `'Listener@method'`.
fn listener_spec(
    expr: &Expression<'_>,
    resolved: &OwnedResolvedNames,
) -> Option<(String, Option<String>)> {
    if let Some(elements) = crate::parser::array_literal_elements(expr) {
        let mut values = elements.iter().filter_map(|element| match element {
            ArrayElement::Value(value) => Some(value.value),
            _ => None,
        });
        let class = class_name(values.next()?, resolved)?;
        let method = values.next().and_then(string_literal).map(str::to_string);
        return Some((class, method));
    }
    if let Some(ident) = crate::parser::class_fetch_identifier(expr) {
        return Some((resolved.class_fqn(ident)?, None));
    }
    let (class, method) = match string_literal(expr)?.split_once('@') {
        Some((class, method)) => (class, Some(method.to_string())),
        None => (string_literal(expr)?, None),
    };
    Some((literal_class_name(class)?, method))
}

/// The class a `Foo::class` fetch or a fully-qualified string literal names.
/// `self`/`static`/`parent` name no class that can be loaded from here.
fn class_name(expr: &Expression<'_>, resolved: &OwnedResolvedNames) -> Option<String> {
    if let Some(ident) = crate::parser::class_fetch_identifier(expr) {
        return resolved.class_fqn(ident);
    }
    literal_class_name(string_literal(expr)?)
}

fn literal_class_name(value: &str) -> Option<String> {
    let fqn = value.replace("\\\\", "\\");
    let fqn = fqn.trim_start_matches('\\');
    (!fqn.is_empty() && fqn.contains('\\')).then(|| fqn.to_string())
}

fn string_literal<'a>(expr: &Expression<'a>) -> Option<&'a str> {
    let Expression::Literal(Literal::String(s)) = expr else {
        return None;
    };
    literal_bytes_to_str(s.value?)
}

// ─── Index ───────────────────────────────────────────────────────────────────

/// Project-wide index of the listener and observer registrations in the
/// service providers.  `files` is the source of truth; the flattened lists
/// are rebuilt from it.
#[derive(Default)]
pub(crate) struct LaravelEventIndex {
    pub(crate) files: FileContributions<EventScan>,
    listeners: Vec<ListenerRegistration>,
    observers: Vec<ObserverRegistration>,
}

impl LaravelEventIndex {
    /// Rebuild the flattened registrations from the per-file scans, in a
    /// stable order and without duplicates.
    pub(crate) fn rebuild(&mut self) {
        let mut listeners: Vec<ListenerRegistration> = Vec::new();
        let mut observers: Vec<ObserverRegistration> = Vec::new();
        for scan in self.files.values() {
            listeners.extend(scan.listeners.iter().cloned());
            observers.extend(scan.observers.iter().cloned());
        }
        listeners.sort_by(|a, b| {
            (&a.event_fqn, &a.listener_fqn, &a.method).cmp(&(
                &b.event_fqn,
                &b.listener_fqn,
                &b.method,
            ))
        });
        listeners.dedup();
        observers
            .sort_by(|a, b| (&a.model_fqn, &a.observer_fqn).cmp(&(&b.model_fqn, &b.observer_fqn)));
        observers.dedup();
        self.listeners = listeners;
        self.observers = observers;
    }

    /// Every listener registration.
    pub(crate) fn listeners(&self) -> &[ListenerRegistration] {
        &self.listeners
    }

    /// Every observer registration.
    pub(crate) fn observers(&self) -> &[ObserverRegistration] {
        &self.observers
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
