//! What a service ID or a container parameter resolves to: the class a
//! service is built from, and the type of a parameter's value.
//!
//! Each declaration records what it gives beside its span, as a
//! [`ContainerValueSite`] (see [`super::resources`] for YAML and XML, and
//! symbol-map extraction for the PHP configurators).  The answers here
//! follow those declarations through aliases and `%name%` placeholders, so
//! `$container->get('app.mailer')` and `$this->getParameter('app.sender')`
//! are typed by what the project's own configuration declares.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::{Mutex, RwLock};

use crate::Backend;
use crate::atom::{Atom, AtomMap};
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::PhpType;
use crate::symbol_map::{ContainerValue, ServiceDecoration, StringKeyKind, SymbolKind, SymbolMap};
use crate::types::ClassInfo;

/// What each service ID and container parameter is declared with, and the
/// services declared to decorate each ID, gathered in one pass over the
/// declarations.
///
/// A `get('x')` is resolved in every file that makes one, and finding
/// `x`'s declarations by walking every declaring file for each of them
/// costs as much as the walk, times the number of lookups.
struct ContainerIndex {
    /// The [`ContainerIndexState::generation`] it was built at.
    generation: u64,
    /// Whether it was built for a Symfony project, which reads the
    /// configurator calls and the installed bundles too.
    is_symfony: bool,
    services: AtomMap<Vec<Option<ContainerValue>>>,
    parameters: AtomMap<Vec<Option<ContainerValue>>>,
    decorators: AtomMap<Vec<Atom>>,
}

impl ContainerIndex {
    /// What each declaration of the `kind` name `key` gives it as, `None`
    /// for one that gives nothing.
    fn values(&self, kind: StringKeyKind, key: &str) -> &[Option<ContainerValue>] {
        let map = match kind {
            StringKeyKind::ServiceId => &self.services,
            _ => &self.parameters,
        };
        // A name nothing interned was never declared.
        Atom::from_existing(key)
            .and_then(|key| map.get(&key))
            .map_or(&[], Vec::as_slice)
    }
}

/// The [`ContainerIndex`] once built, and what keeps it current.
#[derive(Default)]
pub(crate) struct ContainerIndexState {
    built: RwLock<Option<Arc<ContainerIndex>>>,
    /// Moves whenever a declaration the index reads may have changed.
    generation: AtomicU64,
    /// What each file gives the index, so a re-parse that leaves a file's
    /// declarations as they were keeps the index.
    contributions: Mutex<HashMap<String, Contribution>>,
}

/// The service IDs and parameters one file declares, with what it gives
/// each, and the decorations it declares.
#[derive(Default, PartialEq)]
struct Contribution {
    declarations: Vec<(StringKeyKind, String, Vec<ContainerValue>)>,
    /// The calls whose receiver the type engine has to confirm before they
    /// declare anything, which depends on the classes of the whole
    /// workspace and not just this file.
    typed: Vec<(String, Vec<ContainerValue>)>,
    decorations: Vec<ServiceDecoration>,
}

impl Contribution {
    fn of(map: &SymbolMap) -> Self {
        let values_at = |start: u32| -> Vec<ContainerValue> {
            let at = map
                .container_values
                .partition_point(|site| site.start < start);
            map.container_values[at..]
                .iter()
                .take_while(|site| site.start == start)
                .map(|site| site.value.clone())
                .collect()
        };
        let indexed = |kind: StringKeyKind| {
            matches!(
                kind,
                StringKeyKind::ServiceId | StringKeyKind::ContainerParameter
            )
        };
        let declarations = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind,
                    key,
                    is_write: true,
                    ..
                } if indexed(*kind) => Some((*kind, key.clone(), values_at(span.start))),
                _ => None,
            })
            .collect();
        let typed = map
            .typed_receiver_sites
            .iter()
            .filter(|site| site.is_write && site.receiver.names_any(indexed))
            .map(|site| (site.key.clone(), values_at(site.start)))
            .collect();
        Self {
            declarations,
            typed,
            decorations: map.service_decorations.clone(),
        }
    }

    fn is_empty(&self) -> bool {
        self.declarations.is_empty() && self.typed.is_empty() && self.decorations.is_empty()
    }
}

/// What a string value gives a service or a parameter as: the parameter a
/// whole `%name%` placeholder copies, a class when the string is spelled as
/// a namespaced one, the type of the environment variable a whole
/// `%env(…)%` reads, and a `string` otherwise.
pub(crate) fn string_value(text: &str) -> ContainerValue {
    if let Some(ty) = env_placeholder_type(text) {
        return ContainerValue::Type(ty);
    }
    if placeholder_name(text).is_some() {
        return ContainerValue::Class(crate::atom::atom(text));
    }
    let class = text.strip_prefix('\\').unwrap_or(text);
    if class.contains('\\') && is_class_name(class) {
        return ContainerValue::Class(crate::atom::atom(class));
    }
    ContainerValue::Type(PhpType::string())
}

/// The value of a YAML parameter written inline after its key, as Symfony's
/// YAML parser reads it.  `None` for a value whose type the text does not
/// settle: a tag such as `!php/const`, or an anchor or alias.
pub(crate) fn yaml_scalar_value(raw: &str) -> Option<ContainerValue> {
    let raw = raw.trim();
    if raw.starts_with(['[', '{']) {
        return Some(ContainerValue::Type(PhpType::array()));
    }
    if let Some(inner) = raw
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .or_else(|| {
            raw.strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        })
    {
        return Some(string_value(inner));
    }
    if raw.starts_with(['!', '&', '*']) {
        return None;
    }
    // A block scalar (`|` or `>`) is a string, whatever its lines say.
    if raw.starts_with(['|', '>']) {
        return Some(ContainerValue::Type(PhpType::string()));
    }
    Some(match scalar_type(raw, true) {
        Some(ty) => ContainerValue::Type(ty),
        None => string_value(raw),
    })
}

/// The value of an XML `<parameter>`, from its text and its `type`
/// attribute.  Symfony reads the text the way `XmlUtils::phpize()` does
/// unless the type says it is a string.
pub(crate) fn xml_parameter_value(
    text: &str,
    type_attribute: Option<&str>,
) -> Option<ContainerValue> {
    match type_attribute {
        Some("collection") => Some(ContainerValue::Type(PhpType::array())),
        Some("string" | "binary") => Some(string_value(text)),
        Some(_) => None,
        None => {
            let text = text.trim();
            Some(match scalar_type(text, false) {
                Some(ty) => ContainerValue::Type(ty),
                None => string_value(text),
            })
        }
    }
}

/// The type of an unquoted scalar that is not a string: a null, a boolean,
/// an integer, or a float.  YAML also reads `~` as null and the `.inf` and
/// `.nan` spellings as floats.
fn scalar_type(text: &str, yaml: bool) -> Option<PhpType> {
    let lower = text.to_ascii_lowercase();
    match lower.as_str() {
        "null" => return Some(PhpType::null()),
        "~" if yaml => return Some(PhpType::null()),
        "true" | "false" => return Some(PhpType::bool()),
        ".inf" | "-.inf" | "+.inf" | ".nan" if yaml => return Some(PhpType::float()),
        _ => {}
    }
    let digits = lower.trim_start_matches(['-', '+']);
    if digits.is_empty() {
        return None;
    }
    if digits
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'_')
        || digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0o"))
            .or_else(|| digits.strip_prefix("0b"))
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Some(PhpType::int());
    }
    let is_float = digits.bytes().any(|byte| matches!(byte, b'.' | b'e'))
        && digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'-' | b'+' | b'_'))
        && digits.parse::<f64>().is_ok();
    is_float.then(PhpType::float)
}

/// The parameter a whole `%name%` placeholder names.
fn placeholder_name(text: &str) -> Option<&str> {
    let name = text.strip_prefix('%')?.strip_suffix('%')?;
    (!name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')))
    .then_some(name)
}

/// The type a whole `%env(processor:NAME)%` placeholder reads the variable
/// as, by its outermost processor; a variable read without one is a string.
fn env_placeholder_type(text: &str) -> Option<PhpType> {
    let inner = text.strip_prefix("%env(")?.strip_suffix(")%")?;
    let Some((processor, _)) = inner.split_once(':') else {
        return Some(PhpType::string());
    };
    let provided = match processor {
        "base64" | "file" | "resolve" | "string" | "trim" | "urlencode" => "string",
        "bool" | "not" | "defined" => "bool",
        "int" => "int",
        "float" => "float",
        "csv" | "json" | "url" | "query_string" | "shuffle" => "array",
        "enum" => "\\BackedEnum",
        _ => "bool|int|float|string|array",
    };
    Some(PhpType::parse(provided))
}

/// Whether `name` is spelled as a class name.
pub(crate) fn is_class_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && !name.ends_with('\\')
        && !name.contains("\\\\")
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'\\' || byte >= 0x80
        })
}

/// The parameters the kernel sets on every application, and their types.
fn kernel_parameter_type(name: &str) -> Option<PhpType> {
    Some(match name {
        "kernel.project_dir"
        | "kernel.environment"
        | "kernel.runtime_environment"
        | "kernel.build_dir"
        | "kernel.cache_dir"
        | "kernel.logs_dir"
        | "kernel.charset"
        | "kernel.container_class"
        | "kernel.secret"
        | "kernel.share_dir"
        | "kernel.runtime_mode" => PhpType::string(),
        "kernel.debug" | "kernel.runtime_mode.web" | "kernel.runtime_mode.cli" => PhpType::bool(),
        "kernel.bundles" | "kernel.bundles_metadata" | "kernel.enabled_locales" => PhpType::array(),
        _ => return None,
    })
}

impl Backend {
    /// Keep the container index in step with what the file at `uri`
    /// declares: `map` when it counts as one of the project's declaring
    /// files, `None` when it does not or no longer exists.
    pub(crate) fn note_container_declarations(&self, uri: &str, map: Option<&SymbolMap>) {
        let contribution = map.map(Contribution::of).filter(|c| !c.is_empty());
        let state = &self.symfony.container_index;
        let mut contributions = state.contributions.lock();
        let changed = match contribution {
            Some(contribution) => {
                if contributions.get(uri) == Some(&contribution) {
                    false
                } else {
                    contributions.insert(uri.to_string(), contribution);
                    true
                }
            }
            None => contributions.remove(uri).is_some(),
        };
        if changed {
            state.generation.fetch_add(1, Ordering::AcqRel);
            drop(contributions);
            #[cfg(test)]
            if let Some(hook) = AFTER_GENERATION_MOVES.take() {
                hook(self);
            }
            // A receiver typed by a service, as `\Drupal::service('…')->`
            // is, may have been confirmed against declarations not read yet.
            self.retire_typed_receiver_spans();
        }
    }

    /// Rebuild the container index on its next read when anything it holds
    /// may have changed without a declaring file changing: the installed
    /// bundles, or (`typed_only`) the classes that confirm a configurator
    /// call's receiver.
    pub(crate) fn invalidate_container_index(&self, typed_only: bool) {
        let state = &self.symfony.container_index;
        if typed_only
            && !state
                .contributions
                .lock()
                .values()
                .any(|contribution| !contribution.typed.is_empty())
        {
            return;
        }
        state.generation.fetch_add(1, Ordering::AcqRel);
        self.retire_typed_receiver_spans();
    }

    /// The container index, built if nothing current is kept.
    fn container_index(&self) -> Arc<ContainerIndex> {
        let state = &self.symfony.container_index;
        let generation = state.generation.load(Ordering::Acquire);
        let is_symfony = self.is_symfony_project();
        if let Some(index) = state.built.read().as_ref()
            && index.generation == generation
            && index.is_symfony == is_symfony
        {
            return Arc::clone(index);
        }
        // A lookup made while confirming a file's sites asks for the index
        // again, for every lookup the file makes, and so does each file the
        // build confirms in turn.  The files being confirmed read as
        // declaring nothing, and every other file's spans, once confirmed,
        // are cached and stay so while those files are being confirmed:
        // build it once for each set of files, and keep it until one of
        // them is confirmed (see `forget_builds_confirming`).
        let confirming = crate::blade::typed_receiver::confirming_files();
        if !confirming.is_empty()
            && let Some((index, partial)) = THREAD_BUILDS.with(|builds| {
                builds
                    .borrow()
                    .iter()
                    .find(|build| {
                        build.index.generation == generation
                            && build.index.is_symfony == is_symfony
                            && build.confirming == confirming
                    })
                    .map(|build| (Arc::clone(&build.index), build.partial))
            })
        {
            if partial {
                crate::blade::typed_receiver::note_partial_answer();
            }
            return index;
        }
        let partial_before = crate::blade::typed_receiver::partial_answers();
        let index = Arc::new(self.build_container_index(generation, is_symfony));
        // A configurator file being confirmed further up this thread reads
        // as declaring nothing, so an index built meanwhile is incomplete.
        let partial = crate::blade::typed_receiver::partial_answers() != partial_before;
        if !partial {
            let mut built = state.built.write();
            if built
                .as_ref()
                .is_none_or(|kept| kept.generation <= generation)
            {
                *built = Some(Arc::clone(&index));
            }
        }
        if !confirming.is_empty() {
            THREAD_BUILDS.with(|builds| {
                builds.borrow_mut().push(ThreadBuild {
                    confirming,
                    index: Arc::clone(&index),
                    partial,
                });
            });
        }
        index
    }

    fn build_container_index(&self, generation: u64, is_symfony: bool) -> ContainerIndex {
        #[cfg(test)]
        BUILDS.set(BUILDS.get() + 1);
        let mut index = ContainerIndex {
            generation,
            is_symfony,
            services: AtomMap::default(),
            parameters: AtomMap::default(),
            decorators: AtomMap::default(),
        };
        for kind in [StringKeyKind::ServiceId, StringKeyKind::ContainerParameter] {
            let values = match kind {
                StringKeyKind::ServiceId => &mut index.services,
                _ => &mut index.parameters,
            };
            self.visit_symfony_declarations(kind, |_, map, start, _, declared| {
                let values = values.entry(crate::atom::atom(declared)).or_default();
                let at = map
                    .container_values
                    .partition_point(|site| site.start < start);
                let before = values.len();
                values.extend(
                    map.container_values[at..]
                        .iter()
                        .take_while(|site| site.start == start)
                        .map(|site| Some(site.value.clone())),
                );
                if values.len() == before {
                    values.push(None);
                }
            });
        }
        let decorators = &mut index.decorators;
        let mut visit = |map: &SymbolMap| {
            for decoration in &map.service_decorations {
                push_unique(
                    decorators.entry(decoration.decorated).or_default(),
                    decoration.decorator,
                );
            }
        };
        self.visit_declaring_symbol_maps(|_, map| visit(map));
        if is_symfony {
            for (_, map) in &self.symfony_bundle_declarations().files {
                visit(map);
            }
        }
        index
    }

    /// The classes the service `id` is built from, following aliases, a
    /// class given as a parameter, and the parent a child definition takes
    /// its class from when it names none.  The classes of the services
    /// that decorate it join them, as do, in Drupal, the classes service
    /// providers replace it with: either may apply only in some
    /// environments, or with some modules enabled.  An ID spelled as a
    /// class is that class when no declaration says otherwise.  Empty when
    /// nothing the project declares says.
    pub(crate) fn symfony_service_classes(&self, id: &str) -> Vec<Atom> {
        let mut classes = Vec::new();
        self.collect_service_classes(id, &mut HashSet::new(), &mut classes);
        classes
    }

    fn collect_service_classes(&self, id: &str, seen: &mut HashSet<String>, out: &mut Vec<Atom>) {
        let index = self.container_index();
        self.collect_indexed_service_classes(&index, id, seen, out);
    }

    fn collect_indexed_service_classes(
        &self,
        index: &ContainerIndex,
        id: &str,
        seen: &mut HashSet<String>,
        out: &mut Vec<Atom>,
    ) {
        if !seen.insert(id.to_string()) {
            return;
        }
        let mut found = false;
        let mut parents = Vec::new();
        for value in index.values(StringKeyKind::ServiceId, id) {
            match value {
                Some(ContainerValue::Class(class)) => match placeholder_name(class) {
                    Some(parameter) => {
                        for value in parameter_values(index, parameter) {
                            if let Some(ContainerValue::Class(class)) = value
                                && placeholder_name(class).is_none()
                            {
                                found = true;
                                push_unique(out, *class);
                            }
                        }
                    }
                    None => {
                        found = true;
                        push_unique(out, *class);
                    }
                },
                Some(ContainerValue::Alias(target)) => {
                    found = true;
                    self.collect_indexed_service_classes(index, target, seen, out);
                }
                Some(ContainerValue::Parent(parent)) => parents.push(*parent),
                Some(ContainerValue::Type(_)) | None => {}
            }
        }
        if !found {
            let before = out.len();
            for parent in parents {
                self.collect_indexed_service_classes(index, &parent, seen, out);
            }
            found = out.len() > before;
        }
        if self.is_drupal_project() {
            for class in self.drupal_service_replacements(id) {
                found = true;
                push_unique(out, class);
            }
        }
        if !found && id.contains('\\') {
            let class = id.strip_prefix('\\').unwrap_or(id);
            if is_class_name(class) {
                push_unique(out, crate::atom::atom(class));
            }
        }
        // The services the project and its bundles declare to decorate it.
        let decorated = id.strip_prefix('\\').unwrap_or(id);
        if let Some(decorators) =
            Atom::from_existing(decorated).and_then(|id| index.decorators.get(&id))
        {
            for decorator in decorators {
                self.collect_indexed_service_classes(index, decorator, seen, out);
            }
        }
    }

    /// The type of the container parameter `name`'s value, when every
    /// declaration of it says, or when it is one the kernel sets.
    pub(crate) fn symfony_parameter_type(&self, name: &str) -> Option<PhpType> {
        let index = self.container_index();
        let values = parameter_values(&index, name);
        if values.is_empty() {
            return kernel_parameter_type(name);
        }
        let mut types = Vec::with_capacity(values.len());
        for value in values {
            types.push(match value.as_ref()? {
                ContainerValue::Type(ty) => ty.clone(),
                ContainerValue::Class(_) | ContainerValue::Alias(_) | ContainerValue::Parent(_) => {
                    PhpType::string()
                }
            });
        }
        Some(PhpType::union(types))
    }

    /// What `method_name(key)` on `owner` gives when it looks `key` up in
    /// the container: the classes a service ID is built from for `get()`
    /// on a container, and the type of a parameter's value for `get()` on
    /// a parameter bag or `getParameter()`.  `None` when the call is no
    /// container lookup or the project does not declare what `key` is, so
    /// the call keeps the type the method declares.
    pub(crate) fn container_lookup(
        &self,
        owner: &ClassInfo,
        method_name: &str,
        key: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<ContainerLookup> {
        if !method_name.eq_ignore_ascii_case("get")
            && !method_name.eq_ignore_ascii_case("getParameter")
        {
            return None;
        }
        if !self.reads_service_container() {
            return None;
        }
        let (receiver, _) = crate::symbol_map::extraction::symfony_receiver_method(method_name, 0)?;
        let (_, kind) = receiver
            .targets()
            .iter()
            .find(|(fqn, _)| crate::class_lookup::is_subtype_of(owner, fqn, class_loader))?;
        match kind {
            StringKeyKind::ServiceId => {
                let classes: Vec<Arc<ClassInfo>> = self
                    .symfony_service_classes(key)
                    .iter()
                    .filter_map(|class| class_loader(Fqn::new(class)))
                    .collect();
                (!classes.is_empty()).then_some(ContainerLookup::Classes(classes))
            }
            StringKeyKind::ContainerParameter => {
                self.symfony_parameter_type(key).map(ContainerLookup::Type)
            }
            _ => None,
        }
    }
}

/// What a container lookup gives (see [`Backend::container_lookup`]).
pub(crate) enum ContainerLookup {
    /// The classes of the service an ID names.
    Classes(Vec<Arc<ClassInfo>>),
    /// The type of a parameter's value.
    Type(PhpType),
}

/// The values the parameter `name` is declared with, following a value
/// that copies another parameter.
fn parameter_values<'a>(index: &'a ContainerIndex, name: &str) -> Vec<&'a Option<ContainerValue>> {
    let mut values = Vec::new();
    let mut seen = HashSet::new();
    let mut pending = vec![name];
    while let Some(name) = pending.pop() {
        if !seen.insert(name) {
            continue;
        }
        for value in index.values(StringKeyKind::ContainerParameter, name) {
            if let Some(ContainerValue::Class(class)) = value
                && let Some(copied) = placeholder_name(class)
            {
                pending.push(copied);
                continue;
            }
            values.push(value);
        }
    }
    values
}

fn push_unique(out: &mut Vec<Atom>, class: Atom) {
    if !out.contains(&class) {
        out.push(class);
    }
}

/// A container index this thread built while it was confirming
/// `confirming`.
struct ThreadBuild {
    confirming: Vec<String>,
    index: Arc<ContainerIndex>,
    /// Whether the files being confirmed left the index incomplete.
    partial: bool,
}

thread_local! {
    /// The container indexes this thread built for the sets of files it is
    /// still confirming (see `Backend::container_index`).
    static THREAD_BUILDS: std::cell::RefCell<Vec<ThreadBuild>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Forget the container indexes this thread built while confirming `uri`,
/// once it is confirmed: that set of files does not come back.
pub(crate) fn forget_builds_confirming(uri: &str) {
    THREAD_BUILDS.with(|builds| {
        let mut builds = builds.borrow_mut();
        if !builds.is_empty() {
            builds.retain(|build| !build.confirming.iter().any(|file| file == uri));
        }
    });
}

#[cfg(test)]
thread_local! {
    /// How many times this thread has built the container index.
    static BUILDS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// Run once, on this thread, right after a file's declarations move the
    /// container's generation on: a test's stand-in for a lookup on another
    /// thread landing then.
    static AFTER_GENERATION_MOVES: std::cell::Cell<Option<fn(&Backend)>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_of(value: Option<ContainerValue>) -> String {
        match value {
            Some(ContainerValue::Type(ty)) => ty.to_string(),
            Some(ContainerValue::Class(class)) => format!("class {class}"),
            Some(ContainerValue::Alias(id)) => format!("alias {id}"),
            Some(ContainerValue::Parent(id)) => format!("parent {id}"),
            None => "none".to_string(),
        }
    }

    #[test]
    fn yaml_scalars_are_read_as_symfony_reads_them() {
        let cases = [
            ("'PHPantom'", "string"),
            ("PHPantom", "string"),
            ("'123'", "string"),
            ("123", "int"),
            ("-1_000", "int"),
            ("0x1F", "int"),
            ("1.5", "float"),
            ("1e3", "float"),
            (".inf", "float"),
            ("true", "bool"),
            ("FALSE", "bool"),
            ("~", "null"),
            ("null", "null"),
            ("[a, b]", "array"),
            ("{ a: 1 }", "array"),
            ("'%env(int:PORT)%'", "int"),
            ("'%env(APP_SECRET)%'", "string"),
            ("'%env(json:CONFIG)%'", "array"),
            ("'%app.other%'", "class %app.other%"),
            ("App\\Service\\Mailer", "class App\\Service\\Mailer"),
            ("'\\App\\Service\\Mailer'", "class App\\Service\\Mailer"),
            ("!php/const PHP_INT_MAX", "none"),
            ("|", "string"),
        ];
        for (raw, expected) in cases {
            assert_eq!(type_of(yaml_scalar_value(raw)), expected, "{raw}");
        }
    }

    #[test]
    fn xml_parameters_are_phpized_unless_typed() {
        assert_eq!(type_of(xml_parameter_value("123", None)), "int");
        assert_eq!(
            type_of(xml_parameter_value("123", Some("string"))),
            "string"
        );
        assert_eq!(type_of(xml_parameter_value("true", None)), "bool");
        assert_eq!(type_of(xml_parameter_value("PHPantom", None)), "string");
        assert_eq!(
            type_of(xml_parameter_value("", Some("collection"))),
            "array"
        );
        assert_eq!(
            type_of(xml_parameter_value("X::Y", Some("constant"))),
            "none"
        );
    }

    const PSR: &str = "<?php\nnamespace Psr\\Container;\ninterface ContainerInterface {\n    public function get(string $id): mixed;\n}\n";
    const BUILDER: &str = "<?php\nnamespace Symfony\\Component\\DependencyInjection;\nclass ContainerBuilder implements \\Psr\\Container\\ContainerInterface {\n    public function get(string $id): mixed {}\n    public function register(string $id, ?string $class = null): Definition {}\n}\nclass Definition {}\n";

    fn open(backend: &Backend, uri: &str, source: &str) {
        backend
            .open_files
            .write()
            .insert(uri.to_string(), Arc::new(source.to_string()));
        backend.update_ast(uri, source);
    }

    /// A Symfony backend that knows the container's classes.
    fn symfony_backend() -> Backend {
        let backend = Backend::new_headless();
        backend.set_is_symfony(true);
        open(&backend, "file:///stubs/ContainerInterface.php", PSR);
        open(&backend, "file:///stubs/ContainerBuilder.php", BUILDER);
        backend
    }

    /// Configurator files that each register a service on the one the
    /// previous file registers, named so that each sorts before the file
    /// it depends on.
    fn chained_configurators(backend: &Backend, n: usize) {
        for k in 0..n {
            let receiver = if k == 0 {
                "$container".to_string()
            } else {
                format!("$container->get('app.s{}')", k - 1)
            };
            let source = format!(
                "<?php\nnamespace App;\nuse Symfony\\Component\\DependencyInjection\\ContainerBuilder;\nfunction configure{k}(ContainerBuilder $container): void {{\n    {receiver}->register('app.s{k}', ContainerBuilder::class);\n}}\n"
            );
            open(
                backend,
                &format!("file:///config/c{:02}.php", n - 1 - k),
                &source,
            );
        }
    }

    #[test]
    fn building_the_index_builds_it_once_for_each_configurator_being_confirmed() {
        let backend = symfony_backend();
        chained_configurators(&backend, 8);
        BUILDS.set(0);
        let index = backend.container_index();
        assert_eq!(index.services.len(), 8);
        assert!(BUILDS.get() <= 8, "{} builds", BUILDS.get());
    }

    #[test]
    fn confirming_a_configurator_builds_the_index_once_for_all_its_lookups() {
        let backend = symfony_backend();
        chained_configurators(&backend, 1);
        let uri = "file:///config/lookups.php";
        let source = "<?php\nnamespace App;\nuse Symfony\\Component\\DependencyInjection\\ContainerBuilder;\nfunction lookups(ContainerBuilder $container): void {\n    $container->get('app.s0')->register('app.a', ContainerBuilder::class);\n    $container->get('app.s0')->register('app.b', ContainerBuilder::class);\n    $container->get('app.s0')->register('app.c', ContainerBuilder::class);\n}\n";
        open(&backend, uri, source);
        let map = backend.symbol_map_for(uri).unwrap();
        BUILDS.set(0);
        let spans = backend.typed_receiver_spans_for(uri, &map);
        let declared = spans
            .iter()
            .filter(|span| matches!(span.kind, SymbolKind::StringKey { is_write: true, .. }))
            .count();
        assert_eq!(declared, 3);
        assert_eq!(BUILDS.get(), 1);
    }

    #[test]
    fn a_lookup_as_a_file_starts_declaring_reads_its_declarations() {
        let backend = Backend::new_headless();
        let uri = "file:///config/services.yaml";
        let map = Arc::new(SymbolMap {
            spans: vec![crate::symbol_map::SymbolSpan {
                start: 12,
                end: 22,
                kind: SymbolKind::StringKey {
                    kind: StringKeyKind::ServiceId,
                    key: "app.mailer".to_string(),
                    is_write: true,
                    is_optional: false,
                },
            }],
            ..Default::default()
        });
        backend.files.write().insert(
            uri.to_string(),
            Arc::new(crate::file_state::FileState {
                symbol_map: Some(Arc::clone(&map)),
                ..Default::default()
            }),
        );
        AFTER_GENERATION_MOVES.set(Some(|backend| {
            backend.container_index();
        }));
        backend.note_symfony_declarations(uri, &map);
        assert!(AFTER_GENERATION_MOVES.take().is_none());
        assert!(
            backend
                .container_index()
                .services
                .contains_key(&crate::atom::atom("app.mailer"))
        );
    }
}
