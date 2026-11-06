//! Doctrine ORM: the fields an entity maps, and what its repository makes
//! of them.
//!
//! `EntityRepository::__call()` answers `findBy<Field>()`,
//! `findOneBy<Field>()`, and `countBy<Field>()` for each field and
//! association the entity maps, and throws for any other name.  Its
//! `findBy()`, `findOneBy()`, and `count()` take the same names as the keys
//! of their criteria, and `findBy()` and `findOneBy()` as the keys of their
//! `$orderBy`.
//!
//! A repository's entity is the `T` its `EntityRepository<T>` ancestor is
//! bound to, through the `@extends` chain of a repository of its own
//! (`UserRepository extends ServiceEntityRepository<User>`) or by the type
//! a value carries (`EntityRepository<User>`, which `getRepository()`
//! returns for an entity without a repository of its own).  The entity's
//! fields come from its mapping: the attributes the parser records on the
//! class, its parents, and its traits, or an XML or YAML mapping file.
//! Where neither maps the entity, its fields are not known, and nothing
//! here claims anything about it.

pub(crate) mod dql;
pub(crate) mod dql_syntax;
pub(crate) mod mapping_files;
pub(crate) mod query;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::atom::{Atom, atom};
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, DoctrineMapping, MethodInfo, ParameterInfo};
use crate::virtual_members::ResolvedClassCache;

pub(crate) use mapping_files::{MappingFileIndex, new_mapping_file_index};

/// Doctrine ORM's repository base class, the one declaring the magic
/// finders and the `T` they find.
pub(crate) const ENTITY_REPOSITORY: &str = "Doctrine\\ORM\\EntityRepository";

/// The classes whose `__call()` is Doctrine's magic finder: the
/// repository's own, and DoctrineBundle's proxy, which forwards every call
/// to the repository it wraps.
const MAGIC_FINDER_HANDLERS: &[&str] = &[
    ENTITY_REPOSITORY,
    "Doctrine\\Bundle\\DoctrineBundle\\Repository\\ServiceEntityRepositoryProxy",
];

/// The repository methods that take field names: the method, and the
/// positional indexes of the arguments keyed by them.
const FIELD_KEYED_METHODS: &[(&str, &[usize])] =
    &[("findBy", &[0, 1]), ("findOneBy", &[0, 1]), ("count", &[0])];

/// The Doctrine facts the resolved-class cache hands to the resolver, which
/// sees neither the `Backend` nor its indexes.  Both are shared by handle
/// with the `Backend`, so the resolver always reads what it holds now.
#[derive(Clone)]
pub(crate) struct DoctrineCacheState {
    /// Whether the project uses Doctrine; see
    /// [`Backend::is_doctrine`](crate::Backend::is_doctrine).
    enabled: Arc<AtomicBool>,
    /// The fields the project's XML and YAML mapping files map.
    mapping_files: MappingFileIndex,
}

impl Default for DoctrineCacheState {
    fn default() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(true)),
            mapping_files: new_mapping_file_index(),
        }
    }
}

impl DoctrineCacheState {
    pub(crate) fn new(enabled: Arc<AtomicBool>, mapping_files: MappingFileIndex) -> Self {
        Self {
            enabled,
            mapping_files,
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

/// The fields `entity` maps, or `None` when its mapping is not known in
/// full: it is mapped neither by an `#[ORM\Entity]` attribute nor by a
/// mapping file as an entity.
///
/// The fields are those of the entity, its parents, and the traits any of
/// them use: Doctrine's attribute driver reads every property the class
/// has, and a mapped superclass's mapping file maps the fields it passes
/// down.
pub(crate) fn entity_fields(
    entity: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
    state: &DoctrineCacheState,
) -> Option<Vec<Atom>> {
    if !is_entity(entity, state) {
        return None;
    }

    let mut fields: Vec<Atom> = Vec::new();
    for class in mapping_classes(entity, class_loader) {
        let attribute_fields = class.doctrine.iter().flat_map(|d| d.fields.iter().copied());
        let file_fields = mapping_files::mapped_fields(&state.mapping_files, &class.fqn())
            .map(|(_, mapped)| mapped)
            .unwrap_or_default()
            .into_iter()
            .map(|field| atom(&field));
        for field in attribute_fields.chain(file_fields) {
            if !fields.contains(&field) {
                fields.push(field);
            }
        }
    }
    Some(fields)
}

/// The classes whose mappings make up `entity`'s: the entity as loaded,
/// since `entity` may be a merged copy, its parents, and the traits any of
/// them use.
fn mapping_classes(
    entity: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<Arc<ClassInfo>> {
    let mut out: Vec<Arc<ClassInfo>> = Vec::new();
    let mut pending: Vec<Arc<ClassInfo>> = crate::inheritance::ancestors(entity, class_loader)
        .map(|(_, parent)| parent)
        .collect();
    pending.extend(class_loader(Fqn::new(&entity.fqn())));
    while let Some(class) = pending.pop() {
        if out.iter().any(|known| known.fqn() == class.fqn()) {
            continue;
        }
        pending.extend(
            class
                .used_traits
                .iter()
                .filter_map(|name| class_loader(Fqn::new(name))),
        );
        out.push(class);
    }
    out
}

/// Doctrine's collection interface, which a to-many association's property
/// holds.
const COLLECTION: &str = "Doctrine\\Common\\Collections\\Collection";

/// Whether the field `field` of `entity`, whose property is of type `ty`,
/// is an association, and the entity it targets when that is known: the
/// one its mapping names, else the one its property holds, alone or as the
/// elements of a collection.  `None` for a field that is no association.
pub(crate) fn association(
    entity: &ClassInfo,
    field: &str,
    ty: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
    state: &DoctrineCacheState,
) -> Option<Option<Atom>> {
    let mut mapped: Option<Option<Atom>> = None;
    for class in mapping_classes(entity, class_loader) {
        let in_attributes = class
            .doctrine
            .iter()
            .flat_map(|d| d.associations.iter())
            .find(|(name, _)| name == field)
            .map(|(_, target)| *target);
        let in_files = || {
            mapping_files::mapped_association(&state.mapping_files, &class.fqn(), field)
                .map(|target| target.map(|t| atom(&t)))
        };
        match in_attributes.or_else(in_files) {
            Some(Some(target)) => {
                let target = class_loader(Fqn::new(&target)).map_or(target, |c| c.fqn());
                return Some(Some(target));
            }
            Some(None) => mapped = Some(None),
            None => {}
        }
    }
    let typed = association_of_type(ty, class_loader, state);
    match mapped {
        Some(_) => Some(typed.flatten()),
        None => typed,
    }
}

/// Whether a property of type `ty` holds an association, and the entity it
/// targets when that is known: an entity, or a collection of them.
fn association_of_type(
    ty: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
    state: &DoctrineCacheState,
) -> Option<Option<Atom>> {
    let mut association = false;
    let mut target = None;
    let ty = ty.non_null_type().unwrap_or_else(|| ty.clone());
    let entity = |name: &str| {
        class_loader(Fqn::new(name.strip_prefix('\\').unwrap_or(name)))
            .filter(|class| is_entity(class, state))
    };
    for member in ty.union_members() {
        let (name, element) = match member.kind() {
            TypeKind::Named(name) => (*name, None),
            TypeKind::Generic(generic) => (generic.name, generic.args.last().cloned()),
            _ => continue,
        };
        if crate::php_type::is_keyword_type(name.as_str()) {
            continue;
        }
        let Some(class) = class_loader(Fqn::new(name.strip_prefix('\\').unwrap_or(&name))) else {
            continue;
        };
        if is_entity(&class, state) {
            association = true;
            target = Some(class.fqn());
        } else if crate::class_lookup::is_subtype_of(&class, COLLECTION, class_loader) {
            association = true;
            if let Some(TypeKind::Named(element)) = element.as_ref().map(PhpType::kind)
                && let Some(element) = entity(element)
            {
                target = Some(element.fqn());
            }
        }
    }
    association.then_some(target)
}

/// The type `entity` declares for the property of the field `field`.
pub(crate) fn field_type(
    entity: &ClassInfo,
    field: &str,
    class_loader: &ResolvedClassLoader<'_>,
    cache: Option<&ResolvedClassCache>,
) -> PhpType {
    crate::virtual_members::resolve_class_fully_maybe_cached(entity, class_loader, cache)
        .get_property(field)
        .and_then(|property| property.type_hint.clone())
        .unwrap_or_else(PhpType::mixed)
}

/// The entities the association `field` targets, of the entities
/// `receivers` stand for as [`managed_entity_classes`] reads them: what a
/// DQL variable joined through it stands for.
pub(crate) fn association_targets(
    receivers: Vec<Arc<ClassInfo>>,
    field: &str,
    class_loader: &ResolvedClassLoader<'_>,
    cache: Option<&ResolvedClassCache>,
) -> Vec<Arc<ClassInfo>> {
    let state = cache
        .map(|cache| cache.read().doctrine().clone())
        .unwrap_or_default();
    let mut out: Vec<Arc<ClassInfo>> = Vec::new();
    for entity in managed_entities(receivers, class_loader, cache, &state) {
        if !entity_fields(&entity, class_loader, &state)
            .is_some_and(|f| f.iter().any(|f| f == field))
        {
            continue;
        }
        let ty = field_type(&entity, field, class_loader, cache);
        if let Some(Some(target)) = association(&entity, field, &ty, class_loader, &state)
            && let Some(class) = class_loader(Fqn::new(&target))
            && !out.iter().any(|known| known.fqn() == class.fqn())
        {
            out.push(class);
        }
    }
    out
}

/// The type the `T` of `class`'s `EntityRepository` ancestor is bound to,
/// given the type arguments `class` itself carries.  `None` when `class`
/// is no `EntityRepository`, or leaves `T` unbound.
pub(crate) fn repository_entity_type(
    class: &ClassInfo,
    generic_args: &[PhpType],
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    let fqn = class.fqn();
    // Most classes are no repository at all; find that out before building
    // a substitution at every level.
    if fqn != ENTITY_REPOSITORY
        && !crate::inheritance::ancestors(class, class_loader)
            .any(|(_, parent)| parent.fqn() == ENTITY_REPOSITORY)
    {
        return None;
    }

    let mut subs = crate::inheritance::build_generic_subs(class, generic_args);
    let mut current: Option<Arc<ClassInfo>> = None;
    loop {
        let level: &ClassInfo = current.as_deref().unwrap_or(class);
        if level.fqn() == ENTITY_REPOSITORY {
            let bound = subs.get(level.template_params.first()?.as_str())?;
            return concrete_entities(bound).then(|| bound.clone());
        }
        let parent = class_loader(Fqn::new(level.parent_class.as_ref()?))?;
        let args: Vec<PhpType> = crate::inheritance::extends_type_args(level, &parent)
            .unwrap_or_default()
            .iter()
            .map(|arg| {
                let bound = arg.substitute(&subs);
                // A parameter of this level that nothing bound stays a bare
                // name, which is no entity.
                match bound.kind() {
                    TypeKind::Named(name)
                        if level.template_params.iter().any(|p| p == name.as_str()) =>
                    {
                        PhpType::mixed()
                    }
                    _ => bound,
                }
            })
            .collect();
        subs = crate::inheritance::build_generic_subs(&parent, &args);
        current = Some(parent);
    }
}

/// Whether `ty` names classes, and nothing else, as an entity type does.
fn concrete_entities(ty: &PhpType) -> bool {
    ty.union_members().iter().all(|member| {
        matches!(member.kind(), TypeKind::Named(name)
            if !crate::php_type::is_keyword_type(name.as_str()))
    })
}

/// The entity a repository manages and the fields every entity it may be
/// maps, when that is known in full.
fn managed_entity(
    class: &ClassInfo,
    generic_args: &[PhpType],
    class_loader: &ResolvedClassLoader<'_>,
    state: &DoctrineCacheState,
) -> Option<(PhpType, Vec<Atom>)> {
    let entity_type = repository_entity_type(class, generic_args, class_loader)?;
    let mut shared: Option<Vec<Atom>> = None;
    for member in entity_type.union_members() {
        let TypeKind::Named(name) = member.kind() else {
            return None;
        };
        let entity = class_loader(Fqn::new(name))?;
        let fields = entity_fields(&entity, class_loader, state)?;
        shared = Some(match shared {
            None => fields,
            Some(known) => known.into_iter().filter(|f| fields.contains(f)).collect(),
        });
    }
    Some((entity_type, shared?))
}

/// Give a resolved repository whose entity's mapping is known its magic
/// finders, and record the entity it manages.  `raw` is the class as
/// parsed and `generic_args` the type arguments the resolution binds.
///
/// A repository already given them, by the resolution of the class itself,
/// is left alone.
pub(crate) fn apply_to_repository(
    resolved: &mut ClassInfo,
    raw: &ClassInfo,
    generic_args: &[PhpType],
    class_loader: &ResolvedClassLoader<'_>,
    state: &DoctrineCacheState,
) {
    if !state.is_enabled() || raw.parent_class.is_none() && raw.fqn() != ENTITY_REPOSITORY {
        return;
    }
    if resolved
        .doctrine
        .as_ref()
        .is_some_and(|d| d.managed_entity.is_some())
    {
        return;
    }
    let Some((entity, fields)) = managed_entity(raw, generic_args, class_loader, state) else {
        return;
    };
    let methods = magic_finders(&entity, &fields);
    crate::virtual_members::merge_virtual_members(
        resolved,
        crate::virtual_members::VirtualMembers {
            methods,
            properties: Vec::new(),
            constants: Vec::new(),
        },
    );
    resolved
        .doctrine
        .get_or_insert_with(|| Box::new(DoctrineMapping::default()))
        .managed_entity = Some(entity);
    resolved.rebuild_method_index();
}

/// `findBy<Field>()`, `findOneBy<Field>()`, and `countBy<Field>()` for each
/// field, as `EntityRepository::__call()` answers them: the field's value,
/// then the arguments `findBy()`, `findOneBy()`, and `count()` take after
/// their criteria.
fn magic_finders(entity: &PhpType, fields: &[Atom]) -> Vec<Arc<MethodInfo>> {
    let order_by = PhpType::nullable(PhpType::generic(
        "array",
        vec![PhpType::string(), PhpType::string()],
    ));
    let nullable_int = PhpType::nullable(PhpType::int());
    let value = parameter("$value", PhpType::mixed(), None);
    let order_by = parameter("$orderBy", order_by, Some("null"));
    let limit = parameter("$limit", nullable_int.clone(), Some("null"));
    let offset = parameter("$offset", nullable_int, Some("null"));

    let find_by_returns = PhpType::list(entity.clone());
    let find_one_by_returns = PhpType::nullable(entity.clone());
    let count_by_returns = PhpType::parse("int<0, max>");

    let mut methods = Vec::with_capacity(fields.len() * 3);
    for field in fields {
        let suffix = ucfirst(field);
        let described = |what: &str| Some(format!("{what} by their mapped `{field}` field."));
        methods.push(Arc::new(MethodInfo {
            parameters: vec![
                value.clone(),
                order_by.clone(),
                limit.clone(),
                offset.clone(),
            ]
            .into(),
            description: described("Find the entities"),
            ..MethodInfo::virtual_method_typed(&format!("findBy{suffix}"), Some(&find_by_returns))
        }));
        methods.push(Arc::new(MethodInfo {
            parameters: vec![value.clone(), order_by.clone()].into(),
            description: described("Find one entity"),
            ..MethodInfo::virtual_method_typed(
                &format!("findOneBy{suffix}"),
                Some(&find_one_by_returns),
            )
        }));
        methods.push(Arc::new(MethodInfo {
            parameters: vec![value.clone()].into(),
            description: described("Count the entities"),
            ..MethodInfo::virtual_method_typed(&format!("countBy{suffix}"), Some(&count_by_returns))
        }));
    }
    methods
}

fn parameter(name: &str, type_hint: PhpType, default_value: Option<&str>) -> ParameterInfo {
    ParameterInfo {
        name: atom(name),
        is_required: default_value.is_none(),
        type_hint: Some(type_hint),
        native_type_hint: None,
        description: None,
        default_value: default_value.map(str::to_string),
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
        param_out_type: None,
        callable_invocation: None,
    }
}

/// `$by` as `EntityRepository::__call()` reads it back: the field whose
/// first letter it upper-cases.
fn ucfirst(field: &str) -> String {
    let mut chars = field.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Whether `class` is mapped as an entity, by an attribute or a mapping
/// file.
pub(crate) fn is_entity(class: &ClassInfo, state: &DoctrineCacheState) -> bool {
    class.doctrine.as_ref().is_some_and(|d| d.is_entity)
        || mapping_files::mapped_fields(&state.mapping_files, &class.fqn())
            .is_some_and(|(is_entity, _)| is_entity)
}

/// The entity classes a field name written against `receivers` names a
/// field of: the entities the repositories among them manage, for the
/// names a repository's criteria and order take, and the entities among
/// them themselves, for the fields a DQL variable names.  A receiver that
/// is neither gives nothing.
///
/// `cache` is the backend's resolved-class cache, which holds the mapping
/// files a receiver's resolution reads.
pub(crate) fn managed_entity_classes(
    receivers: Vec<Arc<ClassInfo>>,
    class_loader: &ResolvedClassLoader<'_>,
    cache: &ResolvedClassCache,
) -> Vec<Arc<ClassInfo>> {
    let state = cache.read().doctrine().clone();
    managed_entities(receivers, class_loader, Some(cache), &state)
}

fn managed_entities(
    receivers: Vec<Arc<ClassInfo>>,
    class_loader: &ResolvedClassLoader<'_>,
    cache: Option<&ResolvedClassCache>,
    state: &DoctrineCacheState,
) -> Vec<Arc<ClassInfo>> {
    let mut out: Vec<Arc<ClassInfo>> = Vec::new();
    for receiver in receivers {
        if is_entity(&receiver, state) {
            if !out.iter().any(|known| known.fqn() == receiver.fqn()) {
                out.push(receiver);
            }
            continue;
        }
        // A receiver typed by its own repository class may come back as
        // parsed; its entity is recorded on the resolved class.
        let receiver = if receiver.doctrine.is_none() && receiver.parent_class.is_some() {
            crate::virtual_members::resolve_class_fully_maybe_cached(&receiver, class_loader, cache)
        } else {
            receiver
        };
        let Some(entity) = receiver
            .doctrine
            .as_ref()
            .and_then(|d| d.managed_entity.as_ref())
        else {
            continue;
        };
        for member in entity.union_members() {
            if let TypeKind::Named(name) = member.kind()
                && let Some(class) = class_loader(Fqn::new(name))
                && !out.iter().any(|known| known.fqn() == class.fqn())
            {
                out.push(class);
            }
        }
    }
    out
}

/// The entity class names the repositories `receiver` types manage; the
/// [`managed_entity_classes`] of a receiver resolved to a type.
pub(crate) fn managed_entity_names(
    receiver: &PhpType,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
    cache: &ResolvedClassCache,
) -> Vec<String> {
    let receivers = crate::type_engine::type_ops::resolution::type_hint_to_classes_typed(
        receiver,
        "",
        local_classes,
        class_loader,
    );
    managed_entity_classes(receivers, class_loader, cache)
        .iter()
        .map(|class| class.fqn().to_string())
        .collect()
}

/// Whether `method` on a repository takes field names in its positional
/// argument `index`.
pub(crate) fn takes_field_names(method: &str, index: usize) -> bool {
    FIELD_KEYED_METHODS
        .iter()
        .any(|(name, indexes)| method.eq_ignore_ascii_case(name) && indexes.contains(&index))
}

/// The parameter names of the arguments [`takes_field_names`] lists, for a
/// call that names them.
pub(crate) fn field_keyed_parameter(method: &str, name: &str) -> bool {
    let index = match name {
        "criteria" => 0,
        "orderBy" => 1,
        _ => return false,
    };
    takes_field_names(method, index)
}

/// Whether `class`'s `__call()` is Doctrine's magic finder, which throws
/// for any name it does not answer.
pub(crate) fn has_magic_finder_call(
    class: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    let declares = |c: &ClassInfo| {
        c.methods
            .iter()
            .any(|m| !m.is_virtual && m.name.eq_ignore_ascii_case("__call"))
    };
    let Some(raw) = class_loader(Fqn::new(&class.fqn())) else {
        return false;
    };
    let declaring = if declares(&raw) {
        raw.fqn()
    } else {
        match crate::inheritance::find_declaring_ancestor(&raw, class_loader, &declares) {
            Some((_, ancestor)) => ancestor.fqn(),
            None => return false,
        }
    };
    MAGIC_FINDER_HANDLERS.contains(&declaring.as_str())
}

/// Whether Doctrine's magic finder rejects every name the merged
/// repository `class` does not declare.  It does once the entity's
/// mapping is known: the repository then carries each finder the handler
/// answers, and a name it lacks either misses the handler's prefixes or
/// names no field.  Without the mapping a call may still name a field, or
/// the repository may be one the entity names in a mapping not read here.
pub(crate) fn magic_finder_rejects(class: &ClassInfo) -> bool {
    class
        .doctrine
        .as_ref()
        .is_some_and(|d| d.managed_entity.is_some())
}
