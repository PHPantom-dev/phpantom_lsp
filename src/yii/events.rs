//! Yii event listeners (`yiisoft/yii-event`): the listeners the events
//! configuration attaches to each event class.
//!
//! ```php
//! // config/common/events.php
//! return [
//!     UserRegistered::class => [
//!         [WelcomeMailer::class, 'send'],
//!         static fn (UserRegistered $event) => ...,
//!     ],
//! ];
//! ```
//!
//! `yiisoft/yii-event` reads the listeners from the `events`, `events-web`,
//! and `events-console` groups, each made of the files the installed
//! packages contribute followed by the project's own.  The project's files
//! come from its merge plan (see [`super::params::ConfigPlan`]), and a
//! package's from the `config-plugin` of its `composer.json`, which
//! `installed.json` repeats.
//!
//! The method string of an `[Handler::class, 'method']` listener is an
//! array callable like any other, which the shared member machinery
//! navigates, completes, and renames.  In an events file it is known to be
//! called, so the unknown-member check judges it there too (see
//! [`Backend::yii_listener_method_offsets`]).
//!
//! A listener is called through `yiisoft/injector`, which hands the event
//! to the first parameter typed with a class the event is an instance of,
//! and asks the container for any other class-typed one.  A required
//! parameter typed with no class at all is filled by neither, and calling
//! the listener throws.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{Diagnostic, DiagnosticSeverity, Location, Range};

use crate::Backend;
use crate::atom::{Atom, bytes_to_str};
use crate::php_type::{PhpType, TypeKind};
use crate::text_position::LineIndex;
use crate::types::ParameterInfo;
use crate::virtual_members::laravel::helpers::resolved_string_literal;

/// Diagnostic code for a listener the event dispatcher cannot call.
pub(crate) const INVALID_YII_EVENT_LISTENER_CODE: &str = "invalid_yii_event_listener";

/// Whether `group` is one `yiisoft/yii-event` reads listeners from.  The
/// runners name theirs `events-web` and `events-console`, nested under the
/// base `events` group.
pub(crate) fn is_events_group(group: &str) -> bool {
    group == "events" || group.starts_with("events-")
}

/// A required parameter of a listener that the injector cannot fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnfillableParameter {
    /// The parameter's name, with its `$`.
    pub(crate) name: String,
    /// Where the parameter's variable is written, for a closure's.
    pub(crate) start: u32,
    pub(crate) end: u32,
}

/// What a listener is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListenerKind {
    /// `[Handler::class, 'method']`, with the range of the method's name
    /// inside its quotes.
    Method {
        class: Atom,
        method: String,
        start: u32,
        end: u32,
    },
    /// A closure or an arrow function, with the parameters the injector
    /// cannot fill.
    Closure {
        unfillable: Vec<UnfillableParameter>,
    },
    /// Anything else: a class the container builds as an invokable, or a
    /// callable built in code.
    Other,
}

/// One listener the events configuration attaches to an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listener {
    /// The event class, fully qualified, without a leading `\`.
    pub(crate) event: Atom,
    /// Where the listener is written.
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) kind: ListenerKind,
}

/// Whether a value of a parameter typed `hint` can be filled with an
/// object, the event or one the container builds.  `object` and any class
/// can, and so can a type this does not take apart.
fn takes_object(hint: &PhpType) -> bool {
    match hint.kind() {
        TypeKind::Named(name) => {
            let name = name.as_str();
            name.eq_ignore_ascii_case("object")
                || !crate::php_type::is_builtin_non_class_type(&name.to_ascii_lowercase())
        }
        TypeKind::Nullable(inner) => takes_object(inner),
        TypeKind::Union(members) => members.iter().any(takes_object),
        TypeKind::Array(_)
        | TypeKind::ArrayShape(_)
        | TypeKind::Callable(_)
        | TypeKind::Literal(_)
        | TypeKind::IntRange(..) => false,
        _ => true,
    }
}

/// Whether the injector cannot fill `parameter` when it calls a listener
/// with the event: the parameter is required, and typed with no class and
/// without `null`, which the injector passes to a nullable one.
pub(crate) fn is_unfillable(parameter: &ParameterInfo) -> bool {
    if !parameter.is_required || parameter.is_variadic {
        return false;
    }
    match &parameter.native_type_hint {
        None => true,
        Some(hint) => !hint.accepts_null() && !takes_object(hint),
    }
}

/// The listeners the array an events file returns attaches, with the
/// classes resolved through `names`.
fn listeners_of(
    returned: &Expression<'_>,
    content: &str,
    names: &crate::names::OwnedResolvedNames,
) -> Vec<Listener> {
    let mut listeners = Vec::new();
    let Some(events) = array_elements(returned) else {
        return listeners;
    };
    for element in events.iter() {
        let ArrayElement::KeyValue(entry) = element else {
            continue;
        };
        let Some(event) = class_name(entry.key, names) else {
            continue;
        };
        let Some(entries) = array_elements(entry.value) else {
            continue;
        };
        for listener in entries.iter() {
            let ArrayElement::Value(listener) = listener else {
                continue;
            };
            let span = listener.value.span();
            listeners.push(Listener {
                event,
                start: span.start.offset,
                end: span.end.offset,
                kind: listener_kind(listener.value, content, names),
            });
        }
    }
    listeners
}

/// The elements of an array literal, or `None` for any other value.
fn array_elements<'a>(
    value: &'a Expression<'a>,
) -> Option<&'a TokenSeparatedSequence<'a, ArrayElement<'a>>> {
    match value {
        Expression::Parenthesized(p) => array_elements(p.expression),
        Expression::Array(array) => Some(&array.elements),
        Expression::LegacyArray(array) => Some(&array.elements),
        _ => None,
    }
}

/// The class a `Foo::class` expression names, fully qualified.
fn class_name(expr: &Expression<'_>, names: &crate::names::OwnedResolvedNames) -> Option<Atom> {
    let Expression::Identifier(identifier) = crate::parser::class_fetch_target(expr)? else {
        return None;
    };
    let written = bytes_to_str(identifier.value());
    let fqn = names
        .get(identifier.span().start.offset)
        .unwrap_or(written)
        .trim_start_matches('\\');
    (!fqn.is_empty()).then(|| crate::atom::atom(fqn))
}

fn listener_kind(
    listener: &Expression<'_>,
    content: &str,
    names: &crate::names::OwnedResolvedNames,
) -> ListenerKind {
    let parameters = match listener {
        Expression::Closure(closure) => &closure.parameter_list,
        Expression::ArrowFunction(arrow) => &arrow.parameter_list,
        listener => return method_listener(listener, content, names),
    };
    // Without a PHP version nothing is filtered out, so the parameters
    // line up with the ones written.
    let infos = crate::parser::extract_parameters(parameters, Some(content), None, None);
    let unfillable = infos
        .iter()
        .zip(parameters.parameters.iter())
        .filter(|(info, _)| is_unfillable(info))
        .map(|(info, written)| UnfillableParameter {
            name: info.name.to_string(),
            start: written.variable.span.start.offset,
            end: written.variable.span.end.offset,
        })
        .collect();
    ListenerKind::Closure { unfillable }
}

/// The `[Handler::class, 'method']` listener `listener` is, or
/// [`ListenerKind::Other`].
fn method_listener(
    listener: &Expression<'_>,
    content: &str,
    names: &crate::names::OwnedResolvedNames,
) -> ListenerKind {
    let Some(elements) = array_elements(listener) else {
        return ListenerKind::Other;
    };
    let mut values = elements.iter();
    let (Some(ArrayElement::Value(class)), Some(ArrayElement::Value(method)), None) =
        (values.next(), values.next(), values.next())
    else {
        return ListenerKind::Other;
    };
    let (Some(class), Some((method, start, end))) = (
        class_name(class.value, names),
        resolved_string_literal(method.value, content),
    ) else {
        return ListenerKind::Other;
    };
    ListenerKind::Method {
        class,
        method: method.to_string(),
        start: start as u32,
        end: end as u32,
    }
}

/// The listeners the events file `content` attaches, read from its own
/// text.  `file_id` names it to the parser.
pub(crate) fn read_listeners(content: &str, file_id: &str) -> Vec<Listener> {
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(file_id.as_bytes());
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let resolver = mago_names::resolver::NameResolver::new(&arena);
    let names = crate::names::OwnedResolvedNames::from_resolved(&resolver.resolve(program));
    crate::virtual_members::laravel::array_file::returned_exprs(program)
        .first()
        .map(|returned| listeners_of(returned, content, &names))
        .unwrap_or_default()
}

/// The listeners one package's events file attaches, with the file's URI
/// and content for turning them into locations.
pub(crate) struct PackageListenerFile {
    uri: String,
    content: String,
    listeners: Vec<Listener>,
}

/// The listeners of the package files at `paths`.
fn read_package_listener_files(paths: &[PathBuf]) -> Vec<PackageListenerFile> {
    let mut files = Vec::new();
    for path in paths {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        if !content.contains("::class") {
            continue;
        }
        let uri = crate::util::path_to_uri(path);
        let listeners = read_listeners(&content, &uri);
        if !listeners.is_empty() {
            files.push(PackageListenerFile {
                uri,
                content,
                listeners,
            });
        }
    }
    files
}

/// The short name of the class `fqn`: the part every way of writing it in
/// a file spells out, alias imports included.
fn short_name(fqn: &str) -> &str {
    fqn.rsplit('\\').next().unwrap_or(fqn)
}

impl Backend {
    /// The listeners the installed packages attach, read on first use.
    fn yii_package_listeners(&self) -> Arc<Vec<PackageListenerFile>> {
        let mut known = self.yii.package_listeners.lock();
        if let Some(known) = known.as_ref() {
            return Arc::clone(known);
        }
        let vendors = self.workspace.vendor_dir_paths.lock().clone();
        let mut paths = Vec::new();
        for vendor in &vendors {
            let Some(installed) = crate::composer::read_installed_packages(vendor, "") else {
                continue;
            };
            for package in &installed.packages {
                if let Some(root) = installed.package_root(package) {
                    paths.extend(super::definitions::package_group_files(
                        package,
                        &root,
                        is_events_group,
                    ));
                }
            }
        }
        paths.sort_unstable();
        paths.dedup();
        let found = Arc::new(read_package_listener_files(&paths));
        *known = Some(Arc::clone(&found));
        found
    }

    /// Forget the listeners the installed packages attach, once they
    /// changed.
    pub(crate) fn forget_yii_package_listeners(&self) {
        *self.yii.package_listeners.lock() = None;
    }

    /// Whether the file at `path` is one of the project's events files.
    pub(crate) fn is_yii_events_file(&self, path: &Path) -> bool {
        self.is_yii_project() && self.yii_config_plan().is_events_file(path)
    }

    /// The listeners the file at `uri` attaches, or none when it is not one
    /// of the project's events files.
    pub(crate) fn yii_file_listeners(&self, uri: &str, content: &str) -> Vec<Listener> {
        if !self.is_yii_project() {
            return Vec::new();
        }
        match crate::util::uri_to_path(uri) {
            Some(path) if self.is_yii_events_file(&path) => read_listeners(content, uri),
            _ => Vec::new(),
        }
    }

    /// Where the method names of the `[Handler::class, 'method']`
    /// listeners of the file at `uri` start, in order.  The unknown-member
    /// check judges these array callables, which elsewhere may be data.
    pub(crate) fn yii_listener_method_offsets(&self, uri: &str, content: &str) -> Vec<u32> {
        let mut offsets: Vec<u32> = self
            .yii_file_listeners(uri, content)
            .into_iter()
            .filter_map(|listener| match listener.kind {
                ListenerKind::Method { start, .. } => Some(start),
                _ => None,
            })
            .collect();
        offsets.sort_unstable();
        offsets
    }

    /// Where the events configuration attaches listeners to the event class
    /// `fqn`: the project's own files, then the installed packages'.
    pub(crate) fn yii_event_listener_locations(&self, fqn: &str) -> Vec<Location> {
        let fqn = fqn.trim_start_matches('\\');
        let short = short_name(fqn);
        let attaches = |listener: &Listener| listener.event.as_str().eq_ignore_ascii_case(fqn);
        let mut locations = Vec::new();
        let plan = self.yii_config_plan();
        let mut files: Vec<&PathBuf> = plan.events_files().collect();
        files.sort_unstable();
        for path in files {
            let uri = crate::util::path_to_uri(path);
            let content = match self.get_file_content_arc(&uri) {
                Some(content) => content,
                None => match std::fs::read_to_string(path) {
                    Ok(content) => Arc::new(content),
                    Err(_) => continue,
                },
            };
            // Every way of naming the class spells its short name.
            if memchr::memmem::find(content.as_bytes(), short.as_bytes()).is_none() {
                continue;
            }
            let lines = LineIndex::new(&content);
            locations.extend(
                read_listeners(&content, &uri)
                    .iter()
                    .filter(|listener| attaches(listener))
                    .filter_map(|listener| {
                        super::definitions::location(&uri, &lines, listener.start, listener.end)
                    }),
            );
        }
        for file in self.yii_package_listeners().iter() {
            if !file.listeners.iter().any(attaches) {
                continue;
            }
            let lines = LineIndex::new(&file.content);
            locations.extend(
                file.listeners
                    .iter()
                    .filter(|listener| attaches(listener))
                    .filter_map(|listener| {
                        super::definitions::location(
                            &file.uri,
                            &lines,
                            listener.start,
                            listener.end,
                        )
                    }),
            );
        }
        locations
    }

    /// Report the listeners of the file at `uri`, when it is one of the
    /// project's events files, that take a required parameter the injector
    /// cannot fill.  A method that does not exist or is not public is the
    /// unknown-member check's.
    pub(crate) fn collect_yii_event_listener_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let listeners = self.yii_file_listeners(uri, content);
        if listeners.is_empty() {
            return;
        }
        let lines = LineIndex::new(content);
        let range = |start: u32, end: u32| Range {
            start: lines.position(start as usize),
            end: lines.position(end as usize),
        };
        for listener in &listeners {
            match &listener.kind {
                ListenerKind::Closure { unfillable } => {
                    for parameter in unfillable {
                        if !crate::diagnostics::incremental::reports_site(parameter.start) {
                            continue;
                        }
                        out.push(crate::diagnostics::helpers::make_diagnostic(
                            range(parameter.start, parameter.end),
                            DiagnosticSeverity::Error,
                            INVALID_YII_EVENT_LISTENER_CODE,
                            unfillable_message(&parameter.name, None),
                        ));
                    }
                }
                ListenerKind::Method {
                    class,
                    method,
                    start,
                    end,
                } => {
                    if !crate::diagnostics::incremental::reports_site(*start) {
                        continue;
                    }
                    let Some(defined) = self.yii_defined_class(class.as_str()) else {
                        continue;
                    };
                    let Some(declared) = defined.get_method_ci(method) else {
                        continue;
                    };
                    let callable = format!("{}::{}()", class.as_str(), declared.name);
                    for parameter in declared.parameters.iter().filter(|p| is_unfillable(p)) {
                        out.push(crate::diagnostics::helpers::make_diagnostic(
                            range(*start, *end),
                            DiagnosticSeverity::Error,
                            INVALID_YII_EVENT_LISTENER_CODE,
                            unfillable_message(&parameter.name, Some(&callable)),
                        ));
                    }
                }
                ListenerKind::Other => {}
            }
        }
    }
}

/// The message for a listener parameter the injector cannot fill.
fn unfillable_message(parameter: &str, callable: Option<&str>) -> String {
    let parameter = match callable {
        Some(callable) => format!("Parameter {parameter} of {callable}"),
        None => format!("Parameter {parameter}"),
    };
    format!(
        "{parameter} cannot be filled when the event is dispatched: a listener's parameters take the event or a service by their class type"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listeners(content: &str) -> Vec<Listener> {
        read_listeners(content, "file:///config/events.php")
    }

    #[test]
    fn listeners_are_read_by_event() {
        let content = r#"<?php
use App\Event\UserRegistered;
use App\Mail\WelcomeMailer;

return [
    UserRegistered::class => [
        [WelcomeMailer::class, 'send'],
        static fn (UserRegistered $event) => null,
        'invokable-service',
    ],
];
"#;
        let found = listeners(content);
        assert_eq!(found.len(), 3);
        assert!(
            found
                .iter()
                .all(|l| l.event.as_str() == "App\\Event\\UserRegistered")
        );
        let ListenerKind::Method {
            class,
            method,
            start,
            end,
        } = &found[0].kind
        else {
            panic!("expected a method listener, got {:?}", found[0].kind);
        };
        assert_eq!(class.as_str(), "App\\Mail\\WelcomeMailer");
        assert_eq!(method, "send");
        assert_eq!(&content[*start as usize..*end as usize], "send");
        assert_eq!(
            found[1].kind,
            ListenerKind::Closure {
                unfillable: Vec::new()
            }
        );
        assert_eq!(found[2].kind, ListenerKind::Other);
    }

    #[test]
    fn parameters_without_a_class_are_unfillable() {
        let content = r#"<?php
return [
    Foo::class => [
        fn ($event) => null,
        fn (string $name, ?string $nullable, int $default = 1, object $any, Bar|int $either) => null,
        function (mixed $m, int ...$rest) {},
    ],
];
"#;
        let found = listeners(content);
        let unfillable: Vec<Vec<&str>> = found
            .iter()
            .map(|listener| match &listener.kind {
                ListenerKind::Closure { unfillable } => {
                    unfillable.iter().map(|p| p.name.as_str()).collect()
                }
                kind => panic!("expected a closure, got {kind:?}"),
            })
            .collect();
        assert_eq!(unfillable, vec![vec!["$event"], vec!["$name"], vec![]]);
        let ListenerKind::Closure { unfillable } = &found[0].kind else {
            unreachable!();
        };
        let at = &unfillable[0];
        assert_eq!(&content[at.start as usize..at.end as usize], "$event");
    }

    #[test]
    fn only_events_groups_are_read() {
        assert!(is_events_group("events"));
        assert!(is_events_group("events-web"));
        assert!(is_events_group("events-console"));
        assert!(!is_events_group("eventsweb"));
        assert!(!is_events_group("di"));
    }
}
