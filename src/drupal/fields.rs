//! The fields of Drupal's content entities, read as properties.
//!
//! Every content entity reads its fields through `ContentEntityBase::__get()`,
//! which also hands back any other value stored on the entity, so any
//! property name reads on the entity class, and writes every value through
//! `ContentEntityBase::__set()`.  The interfaces a receiver is usually typed
//! with (`NodeInterface`, `ContentEntityInterface`) declare neither, so
//! [`FieldableEntityProvider`] gives them the same untyped ones the class
//! has.
//!
//! The fields an entity type is known to have are typed properties on top of
//! that: a field is its field type's item list, `FieldItemList<StringItem>`.
//! [`receiver_entity_types`] maps a receiver to the entity types it may
//! hold, and the receiver has the fields all of them have.  The fields are
//! read into a [`FieldIndex`] on first use (see [`super::base_fields`] and,
//! for configurable fields, [`super::field_storage`]),
//! which the provider reads from the resolved-class cache: providers never
//! see the `Backend`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::{Mutex, RwLock};

use crate::atom::{Atom, atom};
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::php_type::PhpType;
use crate::symbol_map::FieldItemSite;
use crate::types::{
    ClassInfo, ClassLikeKind, MAX_INHERITANCE_DEPTH, MethodInfo, ParameterInfo, PropertyInfo,
    PropertySource, SharedVec,
};
use crate::virtual_members::{ResolvedClassCache, VirtualMemberProvider, VirtualMembers};

/// The interface every entity with fields implements.
pub(crate) const FIELDABLE_ENTITY: &str = "Drupal\\Core\\Entity\\FieldableEntityInterface";

/// What the resolved-class cache carries for Drupal, since virtual member
/// providers receive the cache but never the `Backend`.
pub(crate) struct DrupalCacheState {
    /// Whether the workspace holds a Drupal project, shared with
    /// [`Backend::is_drupal_project`](crate::Backend::is_drupal_project).
    /// Off in a cache made without a `Backend`, so code that resolves
    /// without a project pays nothing for Drupal.
    enabled: Arc<AtomicBool>,
    /// The fields of the project's entity types, shared with the `Backend`
    /// that reads them.
    fields: Arc<FieldSlot>,
}

impl Default for DrupalCacheState {
    fn default() -> Self {
        Self::new(Arc::new(AtomicBool::new(false)), Arc::default())
    }
}

impl DrupalCacheState {
    pub(crate) fn new(enabled: Arc<AtomicBool>, fields: Arc<FieldSlot>) -> Self {
        Self { enabled, fields }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
}

/// Where the [`FieldIndex`] lives once read, shared between the `Backend`
/// that reads it and the resolved-class cache the provider reads it from.
///
/// It is published only once complete, and read by one thread at a time:
/// a half-read index would look like entity types without fields.
#[derive(Default)]
pub(crate) struct FieldSlot {
    index: RwLock<Option<Arc<FieldIndex>>>,
    /// Serializes reads, so a thread that arrives during one waits for it.
    build: Mutex<()>,
    /// Moves with every [`invalidate`](Self::invalidate), so a read that
    /// spans one is read again.
    generation: AtomicU64,
    /// Whether an index was handed out since the last invalidation, so a
    /// resolved class may hold fields read from it.
    handed_out: AtomicBool,
    /// The state of the `Backend` that reads the index, for the providers,
    /// which never see the `Backend`.  Weak, since the state holds the
    /// cache that holds this slot.
    backend: std::sync::OnceLock<std::sync::Weak<crate::BackendState>>,
    /// The classes, lowercased, the read in progress has looked at so far:
    /// a publish that changes none of them leaves the read alone, as
    /// nearly every publish during indexing does.
    read_classes: Mutex<std::collections::HashSet<String>>,
}

thread_local! {
    /// Whether this thread is reading a field index, during which the
    /// class loads it makes must not start another.
    static READING_FIELDS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Marks this thread as reading a field index for as long as it lives,
/// panics included.
struct ReadingFields;

impl ReadingFields {
    fn enter() -> Self {
        READING_FIELDS.set(true);
        Self
    }
}

impl Drop for ReadingFields {
    fn drop(&mut self) {
        READING_FIELDS.set(false);
    }
}

impl FieldSlot {
    /// The index, or `None` when nothing has read it since the last change.
    pub(crate) fn peek(&self) -> Option<Arc<FieldIndex>> {
        let slot = self.index.read();
        let index = slot.clone()?;
        // Marked under the lock, so an invalidation either sees the mark
        // or finds the index gone.
        self.handed_out.store(true, Ordering::Release);
        Some(index)
    }

    /// The index, read by `read` unless it is there already.  `None` on
    /// the thread that is reading it.  A read an invalidation overtakes is
    /// read again, so what is handed out is always what was published.
    pub(crate) fn get_or_read(
        &self,
        mut read: impl FnMut() -> FieldIndex,
    ) -> Option<Arc<FieldIndex>> {
        if let Some(index) = self.peek() {
            return Some(index);
        }
        if READING_FIELDS.get() {
            return None;
        }
        let _build = self.build.lock();
        loop {
            if let Some(index) = self.peek() {
                return Some(index);
            }
            let generation = self.generation.load(Ordering::Acquire);
            self.read_classes.lock().clear();
            let index = {
                let _reading = ReadingFields::enter();
                Arc::new(read())
            };
            let mut slot = self.index.write();
            if self.generation.load(Ordering::Acquire) == generation {
                self.handed_out.store(true, Ordering::Release);
                *slot = Some(Arc::clone(&index));
                self.read_classes.lock().clear();
                return Some(index);
            }
        }
    }

    /// The index, read by the `Backend` [`bind`](Self::bind) names unless
    /// it is there already.  `None` before the workspace is a Drupal
    /// project, and on the thread that is reading it.
    pub(crate) fn get(&self) -> Option<Arc<FieldIndex>> {
        if let Some(index) = self.peek() {
            return Some(index);
        }
        let state = self.backend.get()?.upgrade()?;
        crate::Backend {
            state,
            request_progress: None,
        }
        .drupal_field_index()
    }

    /// How many times the index was invalidated.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// The index as published, without handing it out.
    pub(crate) fn published(&self) -> Option<Arc<FieldIndex>> {
        self.index.read().clone()
    }

    /// Note that the read in progress looked at the class `name`.  Noted
    /// before the class is loaded, so a declaration published after
    /// [`read_looked_at`](Self::read_looked_at) said no is one the read
    /// finds.
    pub(crate) fn note_read_class(&self, name: &str) {
        self.read_classes
            .lock()
            .insert(name.trim_start_matches('\\').to_ascii_lowercase());
    }

    /// Whether the read in progress, if any, has looked at one of
    /// `classes`, so what it read of them may have changed.
    pub(crate) fn read_looked_at(&self, classes: &[&str]) -> bool {
        let read = self.read_classes.lock();
        !read.is_empty()
            && classes
                .iter()
                .any(|class| read.contains(&class.trim_start_matches('\\').to_ascii_lowercase()))
    }

    /// The classes the read in progress has looked at.  They stay noted
    /// until its index is published, which then answers for them.
    pub(crate) fn read_classes(&self) -> std::collections::HashSet<String> {
        self.read_classes.lock().clone()
    }

    /// Name the state of the `Backend` that reads the index.
    pub(crate) fn bind(&self, backend: &Arc<crate::BackendState>) {
        let _ = self.backend.set(Arc::downgrade(backend));
    }

    /// Drop the index, so the next reader reads it again.  Whether an index
    /// was handed out since the last invalidation, so the classes that may
    /// have fields from it have to go.
    pub(crate) fn invalidate(&self) -> bool {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let mut slot = self.index.write();
        *slot = None;
        self.handed_out.swap(false, Ordering::AcqRel)
    }
}

/// One field of an entity type.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Field {
    pub name: String,
    /// The item list the field holds, with its item class as type argument
    /// and, for a reference field, its target entity class after it.
    pub ty: PhpType,
    /// The IDs of the field types it is created with, `string` or
    /// `entity_reference`: one, unless its declarations disagree.
    pub field_types: Vec<Atom>,
    /// Where the field is declared.  None for the fields Drupal's entity
    /// field manager adds itself.
    pub declarations: Vec<FieldDeclaration>,
}

/// Where a field is declared.  Where it is written is looked up when
/// asked, since an edit above it moves it without changing the fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FieldDeclaration {
    /// The statement at `statement` among those the base field method
    /// `method` of `owner` in the file at `uri` creates fields with.
    BaseField {
        uri: Arc<str>,
        owner: Atom,
        method: Atom,
        statement: usize,
    },
    /// The field storage file at `uri`.
    Storage { uri: Arc<str> },
}

/// The fields of one entity type declaration.
#[derive(Debug)]
pub(crate) struct EntityTypeFields {
    pub id: String,
    pub class: Atom,
    pub fields: Vec<Field>,
}

/// The fields of every content entity type the project declares, and the
/// properties of every field item class.
#[derive(Debug, Default)]
pub(crate) struct FieldIndex {
    pub entity_types: Vec<EntityTypeFields>,
    /// What each field item class declares about its items, by its
    /// lowercased class.
    pub field_items: HashMap<String, FieldItemSite>,
    /// The classes, interfaces, and traits the index was read through,
    /// lowercased: the entity classes with all they extend, implement, and
    /// use, and the classes their base field methods were looked up in.
    pub classes: std::collections::HashSet<String>,
}

impl FieldIndex {
    /// The fields `receiver` has: those every entity type it may hold has,
    /// each typed by all of theirs.
    pub(crate) fn fields_of(
        &self,
        receiver: &ClassInfo,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Vec<Field> {
        let ids = receiver_entity_types(
            receiver,
            self.entity_types
                .iter()
                .map(|entity_type| (entity_type.id.as_str(), entity_type.class)),
            class_loader,
        );
        let mut per_type = ids.iter().map(|id| {
            // An ID declared twice has the fields of either declaration.
            let mut fields: Vec<Field> = Vec::new();
            for entity_type in self.entity_types.iter().filter(|e| e.id == *id) {
                for field in &entity_type.fields {
                    match fields.iter_mut().find(|known| known.name == field.name) {
                        Some(known) => merge_field(known, field),
                        None => fields.push(field.clone()),
                    }
                }
            }
            fields
        });
        let Some(mut shared) = per_type.next() else {
            return Vec::new();
        };
        for fields in per_type {
            let by_name: HashMap<&str, &Field> = fields
                .iter()
                .map(|field| (field.name.as_str(), field))
                .collect();
            shared.retain_mut(|field| match by_name.get(field.name.as_str()) {
                Some(other) => {
                    merge_field(field, other);
                    true
                }
                None => false,
            });
        }
        shared
    }
}

/// Make `field` the same field as it is and as `other` is.
fn merge_field(field: &mut Field, other: &Field) {
    if field.ty != other.ty {
        field.ty = PhpType::union(vec![field.ty.clone(), other.ty.clone()]);
    }
    for field_type in &other.field_types {
        if !field.field_types.contains(field_type) {
            field.field_types.push(*field_type);
        }
    }
    for declaration in &other.declarations {
        if !field.declarations.contains(declaration) {
            field.declarations.push(declaration.clone());
        }
    }
}

/// Whether `class` is `FieldableEntityInterface` or extends or implements
/// it.
pub(crate) fn is_fieldable(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    // Read first, as a class that extends and implements nothing is most
    // classes, and is no subtype of anything but itself.
    (extends_or_implements(class) || class.fqn() == FIELDABLE_ENTITY)
        && is_subtype_of(class, FIELDABLE_ENTITY, class_loader)
}

/// Whether `class` names a parent or an interface, without which it is a
/// subtype of nothing but itself.
pub(super) fn extends_or_implements(class: &ClassInfo) -> bool {
    class.parent_class.is_some() || !class.interfaces.is_empty()
}

/// The members a fieldable entity interface reads and writes its fields
/// with.
pub(crate) struct FieldableEntityProvider;

impl VirtualMemberProvider for FieldableEntityProvider {
    fn applies_to(&self, class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
        matches!(class.kind, ClassLikeKind::Interface | ClassLikeKind::Class)
            && is_fieldable(class, class_loader)
    }

    /// The magic methods go on interfaces only: an entity class has its own
    /// `__get()` and `__set()` from `ContentEntityBase`, and passes the
    /// interface's on to a class that lacks them.
    fn provide(
        &self,
        class: &ClassInfo,
        class_loader: &ResolvedClassLoader<'_>,
        cache: Option<&ResolvedClassCache>,
    ) -> VirtualMembers {
        let methods = if class.kind == ClassLikeKind::Interface
            && !class
                .methods
                .iter()
                .any(|method| method.name.eq_ignore_ascii_case("__get"))
        {
            vec![Arc::new(magic_get()), Arc::new(magic_set())]
        } else {
            Vec::new()
        };
        let properties = cache
            .and_then(field_index)
            .map(|index| {
                index
                    .fields_of(class, class_loader)
                    .into_iter()
                    .map(|field| field_property(&field))
                    .collect()
            })
            .unwrap_or_default();
        VirtualMembers {
            methods,
            properties,
            constants: Vec::new(),
        }
    }
}

/// The field index the `Backend` reads into `cache`.
pub(super) fn field_index(cache: &ResolvedClassCache) -> Option<Arc<FieldIndex>> {
    // Out of the cache's lock: reading the index resolves classes.
    let slot = Arc::clone(&cache.read().drupal().fields);
    slot.get()
}

/// The property `field` reads as.
fn field_property(field: &Field) -> PropertyInfo {
    PropertyInfo {
        source: Some(PropertySource::DrupalField),
        ..PropertyInfo::virtual_property_typed(&field.name, Some(&field.ty))
    }
}

/// `ContentEntityBase::&__get($name)`, which declares no return type.
fn magic_get() -> MethodInfo {
    MethodInfo {
        parameters: SharedVec::from(vec![untyped_parameter("$name")]),
        ..MethodInfo::virtual_method_typed("__get", None)
    }
}

/// `ContentEntityBase::__set($name, $value)`: a write on an interface
/// receiver is as opaque as on the class, so a later read is not narrowed
/// to the written value.
fn magic_set() -> MethodInfo {
    MethodInfo {
        parameters: SharedVec::from(vec![
            untyped_parameter("$name"),
            untyped_parameter("$value"),
        ]),
        ..MethodInfo::virtual_method_typed("__set", None)
    }
}

fn untyped_parameter(name: &str) -> ParameterInfo {
    ParameterInfo {
        name: atom(name),
        is_required: true,
        type_hint: None,
        native_type_hint: None,
        description: None,
        default_value: None,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
        param_out_type: None,
        callable_invocation: None,
        expected_values: None,
    }
}

/// The IDs of the entity types `receiver` may hold, among `entity_types`
/// (each ID with its entity class): those whose class is `receiver` or a
/// subtype of it, and the one whose class `receiver` extends, as a bundle
/// class extends its entity type's class.  None unless `receiver` is
/// fieldable: an `EntityInterface` holds every entity type, but has no
/// fields.
pub(crate) fn receiver_entity_types<'a>(
    receiver: &ClassInfo,
    entity_types: impl IntoIterator<Item = (&'a str, Atom)>,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<&'a str> {
    if !is_fieldable(receiver, class_loader) {
        return Vec::new();
    }
    let entity_types: Vec<(&str, Atom)> = entity_types.into_iter().collect();
    let receiver_fqn = receiver.fqn();
    let mut found: Vec<&str> = Vec::new();
    for &(id, class) in &entity_types {
        let holds = class_loader(Fqn::new(&class))
            .is_some_and(|class| is_subtype_of(&class, &receiver_fqn, class_loader));
        if holds && !found.contains(&id) {
            found.push(id);
        }
    }
    // The nearest class up the receiver's parents that is an entity class,
    // unless the receiver is one itself.
    if receiver.kind != ClassLikeKind::Interface
        && !entity_types.iter().any(|(_, class)| *class == receiver_fqn)
    {
        let mut parent = receiver.parent_class;
        let mut depth = 0;
        while let Some(name) = parent
            && depth < MAX_INHERITANCE_DEPTH
        {
            depth += 1;
            let declared: Vec<&str> = entity_types
                .iter()
                .filter(|(_, class)| *class == name)
                .map(|(id, _)| *id)
                .collect();
            if !declared.is_empty() {
                for id in declared {
                    if !found.contains(&id) {
                        found.push(id);
                    }
                }
                break;
            }
            parent = class_loader(Fqn::new(&name)).and_then(|class| class.parent_class);
        }
    }
    found
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;
