//! The base fields of Drupal's content entity types, read the way Drupal's
//! entity field manager builds them.
//!
//! An entity class's `baseFieldDefinitions()` creates each field with
//! `BaseFieldDefinition::create('type')` under its name, and takes the
//! fields of the methods it calls on `parent::`, `static::`, or `self::`:
//! `ContentEntityBase`'s key fields, and the owner, published, and revision
//! log fields core's traits give.  A name written as
//! `$entity_type->getKey('owner')` is the field the entity type names for
//! that key, and a key the entity type lacks drops the field, as `hasKey()`
//! does in core.  The field manager then adds three boolean fields of its
//! own (`EntityFieldManager::buildBaseFieldDefinitions()`).
//!
//! The entity types and base field methods each file declares are copied
//! out of its symbol map as it is published, since core's traits and base
//! classes let go of their parse once indexed, with the configurable field
//! each field storage file declares (see [`super::field_storage`]), and
//! read into a [`FieldIndex`] on first use.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Location, Range};

use super::fields::{EntityTypeFields, FIELDABLE_ENTITY, Field, FieldDeclaration, FieldIndex};
use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::PhpType;
use crate::symbol_map::{
    BaseFieldMethodSite, BaseFieldName, DefaultTarget, EntityTypeSite, FieldItemSite,
    FieldStorageSite, FieldTarget, MethodScope, SymbolMap,
};
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH};

/// The item list class of a field type that names none.
const FIELD_ITEM_LIST: &str = "Drupal\\Core\\Field\\FieldItemList";
/// What a field is when its field type is not known.
const FIELD_ITEM_LIST_INTERFACE: &str = "Drupal\\Core\\Field\\FieldItemListInterface";
/// The field type of the fields the entity field manager adds.
const BOOLEAN: &str = "boolean";

/// What one file gives the field index, copied out of its symbol map.
#[derive(Debug, PartialEq)]
pub(crate) struct FieldSources {
    uri: Arc<str>,
    /// The entity types the file declares, by ID.
    entity_types: Vec<(String, EntityTypeSite)>,
    methods: Vec<BaseFieldMethodSite>,
    /// What the file's field item classes declare about their items.
    field_items: Vec<FieldItemSite>,
    /// The configurable field the file declares, when it is a field
    /// storage file.
    storage: Option<FieldStorageSite>,
}

impl FieldSources {
    /// What `map`, the symbol map of the file at `uri`, gives, if anything:
    /// its entity types and base field methods when `declarations` counts
    /// them, and its configurable field when `storage` counts it.
    fn of(
        uri: &str,
        map: &SymbolMap,
        declarations: impl FnOnce() -> bool,
        storage: impl FnOnce() -> bool,
    ) -> Option<Self> {
        let fields = map.drupal_fields.as_deref();
        let declares = (!map.entity_types.is_empty()
            || fields.is_some_and(|fields| {
                !fields.base_field_methods.is_empty() || !fields.field_items.is_empty()
            }))
            && declarations();
        let storage = fields
            .and_then(|fields| fields.storage.as_ref())
            .filter(|_| storage())
            .cloned();
        if !declares && storage.is_none() {
            return None;
        }
        Some(Self {
            uri: Arc::from(uri),
            entity_types: if declares {
                super::entity_type_declarations(map)
                    .map(|(id, site)| (id.to_string(), site.clone()))
                    .collect()
            } else {
                Vec::new()
            },
            methods: match fields {
                Some(fields) if declares => fields.base_field_methods.clone(),
                _ => Vec::new(),
            },
            field_items: match fields {
                Some(fields) if declares => fields.field_items.clone(),
                _ => Vec::new(),
            },
            storage,
        })
    }

    /// Whether the two give the same fields, wherever they are written.
    fn declares_as(&self, other: &Self) -> bool {
        self.entity_types.len() == other.entity_types.len()
            && self
                .entity_types
                .iter()
                .zip(&other.entity_types)
                .all(|((a_id, a), (b_id, b))| {
                    a_id == b_id
                        && EntityTypeSite {
                            start: b.start,
                            ..a.clone()
                        } == *b
                })
            && self.methods.len() == other.methods.len()
            && self
                .methods
                .iter()
                .zip(&other.methods)
                .all(|(a, b)| a.declares_as(b))
            && self.field_items == other.field_items
            && match (&self.storage, &other.storage) {
                (Some(a), Some(b)) => a.declares_as(b),
                (a, b) => a.is_none() && b.is_none(),
            }
    }
}

/// The source files of the field index, by URI.
pub(crate) type FieldSourceMap = BTreeMap<String, Arc<FieldSources>>;

/// The entity keys and revision metadata keys of one entity type, with the
/// defaults Drupal's entity type classes give them.
struct EntityKeys<'a> {
    keys: HashMap<&'a str, &'a str>,
    revision_metadata: HashMap<&'a str, &'a str>,
    /// The entity type's `bundle_entity_type`.
    bundle_entity_type: Option<&'a str>,
}

impl<'a> EntityKeys<'a> {
    fn of(site: &'a EntityTypeSite) -> Self {
        let mut keys: HashMap<&str, &str> = site
            .keys
            .iter()
            .map(|(key, field)| (key.as_str(), field.as_str()))
            .collect();
        // `EntityType::__construct()`.
        for (key, field) in [
            ("default_langcode", "default_langcode"),
            (
                "revision_translation_affected",
                "revision_translation_affected",
            ),
        ] {
            keys.entry(key).or_insert(field);
        }
        let mut revision_metadata: HashMap<&str, &str> = site
            .revision_metadata_keys
            .iter()
            .map(|(key, field)| (key.as_str(), field.as_str()))
            .collect();
        // `ContentEntityType::__construct()`.
        if site.content {
            revision_metadata
                .entry("revision_default")
                .or_insert("revision_default");
        }
        Self {
            keys,
            revision_metadata,
            bundle_entity_type: site.bundle_entity_type.as_deref(),
        }
    }

    /// The field the entity type names for `key`, if it has the key: as
    /// `hasKey()` reads it, an empty name is no key.
    fn key(&self, key: &str) -> Option<&'a str> {
        self.keys
            .get(key)
            .copied()
            .filter(|field| !field.is_empty())
    }

    fn revision_metadata_key(&self, key: &str) -> Option<&'a str> {
        self.revision_metadata
            .get(key)
            .copied()
            .filter(|field| !field.is_empty())
    }

    fn name_of(&self, name: &'a BaseFieldName) -> Option<&'a str> {
        match name {
            BaseFieldName::Literal(name) => Some(name),
            BaseFieldName::Key(key) => self.key(key),
            BaseFieldName::RevisionMetadataKey(key) => self.revision_metadata_key(key),
        }
    }
}

/// One statement that creates a field, or one the field manager adds.
struct Creation<'a> {
    field_type: &'a str,
    target_type: Option<&'a str>,
    declaration: Option<FieldDeclaration>,
}

/// A field name with every statement that creates it: two when the method
/// creates it in two branches.
type Created<'a> = Vec<(String, Vec<Creation<'a>>)>;

/// Reads base field methods, following the calls between them.
struct BaseFieldReader<'a> {
    /// Each base field method by its lowercased declaring class or trait
    /// and lowercased name, with the file that declares it.
    methods: HashMap<(String, String), (&'a Arc<str>, &'a BaseFieldMethodSite)>,
    class_loader: &'a ResolvedClassLoader<'a>,
}

impl<'a> BaseFieldReader<'a> {
    /// The fields `method`, looked up from `start` up, creates for the
    /// entity type whose class is `entity_class`.
    fn fields(
        &self,
        entity_class: Atom,
        start: Atom,
        method: &str,
        keys: &EntityKeys<'a>,
        path: &mut Vec<(Atom, String)>,
    ) -> Created<'a> {
        let Some((owner, using, uri, site)) = self.find(start, method) else {
            return Vec::new();
        };
        let step = (owner, method.to_ascii_lowercase());
        if path.contains(&step) || path.len() >= MAX_INHERITANCE_DEPTH as usize {
            return Vec::new();
        }
        path.push(step);
        let mut fields: Created<'a> = Vec::new();
        // `$fields + static::helper()` keeps the fields already there.
        for include in &site.includes {
            // A trait's `self::` and `parent::` are those of the class that
            // uses it.
            let from = match include.scope {
                MethodScope::Static => Some(entity_class),
                MethodScope::Self_ => Some(using),
                MethodScope::Parent => self.load(using).and_then(|class| class.parent_class),
            };
            let Some(from) = from else {
                continue;
            };
            for (name, creations) in self.fields(entity_class, from, &include.method, keys, path) {
                if !fields.iter().any(|(known, _)| *known == name) {
                    fields.push((name, creations));
                }
            }
        }
        fields.retain(|(name, _)| {
            !site
                .removed
                .iter()
                .any(|removed| keys.name_of(removed) == Some(name.as_str()))
        });
        // The method's own statements replace what it took from others.
        let mut own: Created<'a> = Vec::new();
        for (statement, field) in site.fields.iter().enumerate() {
            let Some(name) = keys.name_of(&field.name) else {
                continue;
            };
            if field
                .bundle_condition
                .is_some_and(|bundled| bundled != keys.bundle_entity_type.is_some())
            {
                continue;
            }
            let creation = Creation {
                field_type: &field.field_type,
                target_type: match &field.target {
                    Some(FieldTarget::EntityType(target)) => Some(target),
                    Some(FieldTarget::BundleEntityType) => keys.bundle_entity_type,
                    None => None,
                },
                declaration: Some(FieldDeclaration::BaseField {
                    uri: Arc::clone(uri),
                    owner: site.owner,
                    method: site.method,
                    statement,
                }),
            };
            match own.iter_mut().find(|(known, _)| known == name) {
                Some((_, creations)) => creations.push(creation),
                None => own.push((name.to_string(), vec![creation])),
            }
        }
        for (name, creations) in own {
            set_field(&mut fields, name, creations);
        }
        path.pop();
        fields
    }

    /// The class or trait that declares the base field method `method` for
    /// `start`, and the class that has it.
    fn find(
        &self,
        start: Atom,
        method: &str,
    ) -> Option<(Atom, Atom, &'a Arc<str>, &'a BaseFieldMethodSite)> {
        let method = method.to_ascii_lowercase();
        find_static_method(start, self.class_loader, |class| {
            self.methods
                .get(&(class.to_ascii_lowercase(), method.clone()))
                .copied()
        })
        .map(|(owner, using, (uri, site))| (owner, using, uri, site))
    }

    fn load(&self, class: Atom) -> Option<Arc<ClassInfo>> {
        (self.class_loader)(Fqn::new(&class))
    }
}

/// The class or trait that declares a static method for `start`, as PHP
/// finds it: the class's own, then its traits', then its parent's, with
/// the class among them that has the method, which a trait's `self::` and
/// `parent::` are bound to.  `declared` gives what a class or trait
/// declares itself.
pub(super) fn find_static_method<T>(
    start: Atom,
    class_loader: &ResolvedClassLoader<'_>,
    declared: impl Fn(Atom) -> Option<T>,
) -> Option<(Atom, Atom, T)> {
    let declared_in = |class: Atom| -> Option<(Atom, T)> {
        let mut pending = vec![(class, 0)];
        while let Some((class, depth)) = pending.pop() {
            if let Some(found) = declared(class) {
                return Some((class, found));
            }
            if depth < MAX_INHERITANCE_DEPTH
                && let Some(info) = class_loader(Fqn::new(&class))
            {
                pending.extend(info.used_traits.iter().rev().map(|used| (*used, depth + 1)));
            }
        }
        None
    };
    let mut current = Some(start);
    let mut depth = 0;
    while let Some(class) = current
        && depth < MAX_INHERITANCE_DEPTH
    {
        depth += 1;
        if let Some((owner, found)) = declared_in(class) {
            return Some((owner, class, found));
        }
        current = class_loader(Fqn::new(&class)).and_then(|info| info.parent_class);
    }
    None
}

/// Make `creations` the field `name` of `fields`, in place of any there.
fn set_field<'a>(fields: &mut Created<'a>, name: String, creations: Vec<Creation<'a>>) {
    match fields.iter_mut().find(|(known, _)| *known == name) {
        Some((_, known)) => *known = creations,
        None => fields.push((name, creations)),
    }
}

impl Backend {
    /// Keep the field index's sources in step with the symbol map of the
    /// file at `uri` (`None` once it is gone).  A change to what the fields
    /// are drops the index and evicts the classes that have them; one to
    /// where they are written, as any edit above them makes, keeps both.
    pub(crate) fn note_drupal_field_sources(&self, uri: &str, map: Option<&SymbolMap>) {
        let sources = map.filter(|_| self.is_drupal_project()).and_then(|map| {
            FieldSources::of(
                uri,
                map,
                || {
                    // A test module's entity types are only seen by test
                    // code, while a class's fields are the same whoever
                    // asks, and a dependency's are never part of the
                    // project's own.
                    !crate::util::uri_to_path(uri)
                        .is_some_and(|path| self.is_drupal_test_path(&path))
                        && !self
                            .workspace
                            .vendor_uri_prefixes
                            .lock()
                            .iter()
                            .any(|prefix| uri.starts_with(prefix.as_str()))
                },
                || self.is_drupal_field_storage_source(uri),
            )
        });
        let old = self.drupal.field_sources.read().get(uri).cloned();
        if old.as_deref() == sources.as_ref() {
            return;
        }
        let same_fields = match (old.as_deref(), sources.as_ref()) {
            (Some(old), Some(new)) => old.declares_as(new),
            _ => false,
        };
        // The item classes whose declarations changed.
        let mut item_classes: Vec<Atom> = Vec::new();
        let old_items = old.as_deref().map_or(&[][..], |old| &old.field_items);
        let new_items = sources.as_ref().map_or(&[][..], |new| &new.field_items);
        if old_items != new_items {
            for site in old_items.iter().chain(new_items) {
                if !item_classes.contains(&site.owner) {
                    item_classes.push(site.owner);
                }
            }
        }
        {
            let mut stored = self.drupal.field_sources.write();
            match sources {
                Some(sources) => {
                    stored.insert(uri.to_string(), Arc::new(sources));
                }
                None => {
                    stored.remove(uri);
                }
            }
        }
        if !same_fields {
            self.invalidate_drupal_fields();
        }
        // They go with the index, or one resolved before it does would keep
        // the properties it read from the old one.
        if !item_classes.is_empty() {
            let mut pending = self.drupal.changed_item_classes.lock();
            for class in item_classes {
                if !pending.contains(&class) {
                    pending.push(class);
                }
            }
        }
    }

    /// Evict `classes` and every class that extends or implements one, with
    /// the generic specialisations that name them: a class resolved from
    /// its parent's members records no dependency on an ancestor further
    /// up, so an ancestor's eviction does not reach a class whose parent
    /// is not cached.  The names of every class the eviction reached, as
    /// dependencies are recorded.
    fn evict_with_descendants(&self, classes: Vec<String>) -> Vec<Atom> {
        let descendants = self.descendants_closure(classes.iter().cloned());
        let mut changed: Vec<Atom> = Vec::with_capacity(descendants.len());
        let mut cache = self.resolved_class_cache.write();
        // Most descendants are neither cached nor named by a cached class.
        for fqn in &descendants {
            changed.push(crate::resolution_deps::dep_key(fqn));
            if classes.contains(fqn) || cache.holds_or_depends_on(fqn) {
                for evicted in crate::virtual_members::evict_fqn(&mut cache, fqn) {
                    changed.push(crate::resolution_deps::dep_key(&evicted));
                }
            }
        }
        changed
    }

    /// Note that what the fields are read from changed.  They go when the
    /// publish that brought the change ends (see
    /// [`drupal_field_flush`](Self::drupal_field_flush)), so the many
    /// changes one batch of indexing brings drop them once.
    pub(crate) fn invalidate_drupal_fields(&self) {
        self.drupal
            .fields_changed
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// A guard that drops the fields as it goes when a change was noted
    /// meanwhile.  Taken before the locks a publish holds, so it runs after
    /// they are released.
    pub(crate) fn drupal_field_flush(&self) -> DrupalFieldFlush<'_> {
        DrupalFieldFlush(self)
    }

    /// Drop the field index and the resolved classes that have fields: the
    /// fields of an entity class show on interfaces that do not depend on
    /// it, so every class under `FieldableEntityInterface` goes.
    fn flush_drupal_field_changes(&self) {
        if !self
            .drupal
            .fields_changed
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        // A class takes fields only from an index handed out to it, so
        // while none was, as through most of indexing, there is nothing to
        // walk for.
        let mut changed = if self.drupal.fields.invalidate() {
            self.evict_with_descendants(vec![FIELDABLE_ENTITY.to_string()])
        } else {
            let mut cache = self.resolved_class_cache.write();
            crate::virtual_members::evict_fqn(&mut cache, FIELDABLE_ENTITY)
                .iter()
                .map(|fqn| crate::resolution_deps::dep_key(fqn))
                .chain(std::iter::once(crate::resolution_deps::dep_key(
                    FIELDABLE_ENTITY,
                )))
                .collect()
        };
        // The item classes whose properties changed, and the classes that
        // extend them.
        let item_classes: Vec<String> =
            std::mem::take(&mut *self.drupal.changed_item_classes.lock())
                .iter()
                .map(Atom::to_string)
                .collect();
        if !item_classes.is_empty() {
            changed.extend(self.evict_with_descendants(item_classes));
        }
        // A cached receiver resolution depends on the classes that have the
        // fields, while member completion records nothing to tell.
        changed.sort_unstable();
        changed.dedup();
        self.retain_resolved_member_files(&changed);
        self.member_completion_cache.lock().clear();
    }

    /// The fields of the project's content entity types, read on first use.
    /// `None` outside a Drupal project, and on the thread reading them.
    pub(crate) fn drupal_field_index(&self) -> Option<Arc<FieldIndex>> {
        if !self.is_drupal_project() {
            return None;
        }
        self.drupal
            .fields
            .get_or_read(|| self.read_drupal_field_index())
    }

    fn read_drupal_field_index(&self) -> FieldIndex {
        // Copied out, since loading a class may publish a file.
        let sources: Vec<Arc<FieldSources>> =
            self.drupal.field_sources.read().values().cloned().collect();
        let resolved = self.resolved_class_loader_with(&[]);
        // Every class the read looks at, so that a change to what one of
        // them extends, implements, or uses reads the fields again.
        let loader = |name: Fqn<'_>| {
            self.drupal.fields.note_read_class(name.as_str());
            resolved(name)
        };
        let mut methods = HashMap::new();
        let mut declared: Vec<(&str, &EntityTypeSite)> = Vec::new();
        let mut storages: Vec<(&Arc<str>, &FieldStorageSite)> = Vec::new();
        for source in &sources {
            if let Some(storage) = &source.storage {
                storages.push((&source.uri, storage));
            }
            for site in &source.methods {
                methods
                    .entry((
                        site.owner.to_ascii_lowercase(),
                        site.method.to_ascii_lowercase(),
                    ))
                    .or_insert((&source.uri, site));
            }
            declared.extend(
                source
                    .entity_types
                    .iter()
                    .map(|(id, site)| (id.as_str(), site)),
            );
        }
        let reader = BaseFieldReader {
            methods,
            class_loader: &loader,
        };
        let mut field_types: HashMap<&str, Vec<(Atom, Option<Atom>)>> = HashMap::new();
        let mut index = FieldIndex::default();
        for source in &sources {
            for site in &source.field_items {
                index
                    .field_items
                    .entry(site.owner.to_ascii_lowercase())
                    .or_insert_with(|| site.clone());
            }
        }
        for &(id, site) in declared.iter().filter(|(_, site)| site.content) {
            // Which receivers hold the entity type depends on all of it.
            load_class_graph(site.class, &loader);
            let keys = EntityKeys::of(site);
            let mut created = reader.fields(
                site.class,
                site.class,
                "baseFieldDefinitions",
                &keys,
                &mut Vec::new(),
            );
            add_manager_fields(&mut created, site, &keys);
            // A field declared by several files is one field.
            for &(uri, storage) in storages
                .iter()
                .filter(|(_, storage)| storage.entity_type == id)
            {
                let creation = Creation {
                    field_type: &storage.field_type,
                    target_type: storage.target_type.as_deref(),
                    declaration: Some(FieldDeclaration::Storage {
                        uri: Arc::clone(uri),
                    }),
                };
                match created
                    .iter_mut()
                    .find(|(known, _)| *known == storage.field_name)
                {
                    Some((_, creations)) => creations.push(creation),
                    None => created.push((storage.field_name.clone(), vec![creation])),
                }
            }
            let fields = created
                .into_iter()
                .map(|(name, creations)| {
                    let ty = PhpType::union(
                        creations
                            .iter()
                            .map(|creation| {
                                let classes =
                                    field_types.entry(creation.field_type).or_insert_with(|| {
                                        self.drupal_field_type_classes(creation.field_type)
                                    });
                                field_type(classes, creation.target_type, &declared, |item| {
                                    default_target(&index.field_items, item, &loader)
                                })
                            })
                            .collect(),
                    );
                    let mut field_types = Vec::new();
                    for creation in &creations {
                        let field_type = crate::atom::atom(creation.field_type);
                        if !field_types.contains(&field_type) {
                            field_types.push(field_type);
                        }
                    }
                    let mut declarations = Vec::new();
                    for creation in creations {
                        if let Some(declaration) = creation.declaration
                            && !declarations.contains(&declaration)
                        {
                            declarations.push(declaration);
                        }
                    }
                    Field {
                        name,
                        ty,
                        field_types,
                        declarations,
                    }
                })
                .collect();
            index.entity_types.push(EntityTypeFields {
                id: id.to_string(),
                class: site.class,
                fields,
            });
        }
        index.classes = self.drupal.fields.read_classes();
        index
    }

    /// Note that the classes named `changed` were declared, dropped, or
    /// given another parent, other interfaces, or other traits: the fields
    /// go when the index was read through one of them, or is being read.
    pub(crate) fn note_drupal_class_graph_changes(&self, changed: &[&str]) {
        if changed.is_empty() || !self.is_drupal_project() {
            return;
        }
        let stale = match self.drupal.fields.published() {
            Some(index) => changed.iter().any(|class| {
                index
                    .classes
                    .contains(&class.trim_start_matches('\\').to_ascii_lowercase())
            }),
            // A read in progress restarts only for a class it has looked at,
            // or every publish of indexing would restart it.
            None => self.drupal.fields.read_looked_at(changed),
        };
        if stale {
            self.invalidate_drupal_fields();
        }
    }

    /// The IDs of the entity types a receiver of type `ty` may hold, each
    /// once: the ones a field name asked of it belongs to.
    pub(crate) fn drupal_receiver_entity_types(
        &self,
        ty: &PhpType,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Vec<String> {
        let Some(index) = self.drupal_field_index() else {
            return Vec::new();
        };
        let mut ids: Vec<String> = Vec::new();
        for class in ty.top_level_class_names() {
            let Some(class) = class_loader(Fqn::new(&class)) else {
                continue;
            };
            let held = super::fields::receiver_entity_types(
                &class,
                index
                    .entity_types
                    .iter()
                    .map(|entity_type| (entity_type.id.as_str(), entity_type.class)),
                class_loader,
            );
            for id in held {
                if !ids.iter().any(|known| known == id) {
                    ids.push(id.to_string());
                }
            }
        }
        ids
    }

    /// The names of the fields a receiver of type `ty` has.
    pub(crate) fn drupal_receiver_field_names(
        &self,
        ty: &PhpType,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Vec<String> {
        let Some(index) = self.drupal_field_index() else {
            return Vec::new();
        };
        let mut names: Vec<String> = Vec::new();
        for class in ty.top_level_class_names() {
            let Some(class) = class_loader(Fqn::new(&class)) else {
                continue;
            };
            for field in index.fields_of(&class, class_loader) {
                if !names.contains(&field.name) {
                    names.push(field.name);
                }
            }
        }
        names
    }

    /// The field a field name's span `key` names, as the field index has it
    /// now, with the entity type's ID.
    pub(crate) fn drupal_field_named(&self, key: &str) -> Option<Field> {
        let name = super::fields::split_field_key(key).1;
        let index = self.drupal_field_index()?;
        let mut shared: Option<Field> = None;
        // The field every entity type of the key has, as the property is.
        for entity_type in super::fields::field_key_entity_types(key) {
            let mut found: Option<Field> = None;
            // An ID declared twice has the fields of either declaration.
            for declared in index.entity_types.iter().filter(|e| e.id == entity_type) {
                let Some(field) = declared.fields.iter().find(|field| field.name == name) else {
                    continue;
                };
                match &mut found {
                    Some(known) => super::fields::merge_field(known, field),
                    None => found = Some(field.clone()),
                }
            }
            let found = found?;
            match &mut shared {
                Some(known) => super::fields::merge_field(known, &found),
                None => shared = Some(found),
            }
        }
        shared
    }

    /// Whether the field name spans `a` and `b` name the same field: the
    /// same span, or a field both name, of an entity type both may hold.
    pub(crate) fn drupal_field_keys_match(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        if super::fields::split_field_key(a).1 != super::fields::split_field_key(b).1 {
            return false;
        }
        super::fields::field_key_entity_types(a)
            .any(|entity_type| super::fields::field_key_entity_types(b).any(|e| e == entity_type))
            && self.drupal_field_named(a).is_some()
            && self.drupal_field_named(b).is_some()
    }

    /// Where the field a field name's span `key` names is declared.
    pub(crate) fn drupal_field_name_locations(&self, key: &str) -> Vec<Location> {
        self.drupal_field_named(key)
            .map(|field| self.field_locations(&field))
            .unwrap_or_default()
    }

    /// The hover text of the field a field name's span `key` names: its
    /// field type, its type, its entity type, and the files that declare
    /// it.
    pub(crate) fn drupal_field_name_detail(&self, key: &str) -> String {
        let entity_types: Vec<String> = super::fields::field_key_entity_types(key)
            .map(|entity_type| format!("`{entity_type}`"))
            .collect();
        let Some(field) = self.drupal_field_named(key) else {
            return if entity_types.len() == 1 {
                format!("Not a known field of {}", entity_types[0])
            } else {
                format!("Not a field all of {} have", entity_types.join(", "))
            };
        };
        let mut parts = Vec::new();
        if !field.field_types.is_empty() {
            let field_types: Vec<String> = field
                .field_types
                .iter()
                .map(|field_type| format!("`{field_type}`"))
                .collect();
            parts.push(format!("Field type {}", field_types.join(", ")));
        }
        parts.push(format!("Type `{}`", field.ty.shorten()));
        parts.push(if entity_types.len() == 1 {
            format!("Entity type {}", entity_types[0])
        } else {
            format!("Entity types {}", entity_types.join(", "))
        });
        let mut files: Vec<String> = Vec::new();
        for location in self.field_locations(&field) {
            let path = self
                .workspace_relative_path(location.uri.as_str())
                .unwrap_or_else(|| location.uri.path().to_string());
            if !files.contains(&path) {
                files.push(path);
            }
        }
        if !files.is_empty() {
            let files: Vec<String> = files.iter().map(|file| format!("`{file}`")).collect();
            parts.push(format!("Declared in {}", files.join(", ")));
        }
        parts.join("\n\n")
    }

    /// Where the field `name` of `receiver` is declared, every declaration
    /// of the entity types it may hold, when the property `name` of
    /// `receiver` is that field.
    pub(crate) fn drupal_field_locations(
        &self,
        receiver: &ClassInfo,
        name: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Vec<Location> {
        if !self.is_drupal_project() {
            return Vec::new();
        }
        let resolved = crate::virtual_members::resolve_class_fully_cached(
            receiver,
            class_loader,
            &self.resolved_class_cache,
        );
        if !resolved.properties.iter().any(|property| {
            property.name == name
                && property.source == Some(crate::types::PropertySource::DrupalField)
        }) {
            return Vec::new();
        }
        let Some(index) = self.drupal_field_index() else {
            return Vec::new();
        };
        let Some(field) = index
            .fields_of(receiver, class_loader)
            .into_iter()
            .find(|field| field.name == name)
        else {
            return Vec::new();
        };
        self.field_locations(&field)
    }

    /// Whether the property access `span` of the file at `uri` reads a
    /// field of an entity or a property of a field item, which the
    /// entity's `get()` calls, field storage files, and property
    /// definitions also name.
    pub(crate) fn is_drupal_field_access(
        &self,
        uri: &str,
        content: &str,
        span: &crate::symbol_map::SymbolSpan,
    ) -> bool {
        let crate::symbol_map::SymbolKind::MemberAccess {
            subject_text,
            member_name,
            is_static: false,
            is_method_call: false,
            ..
        } = &span.kind
        else {
            return false;
        };
        if !self.is_drupal_project() {
            return false;
        }
        let Some(source) = self.symbol_map_source(uri, content) else {
            return false;
        };
        let subject = subject_text.as_str(source);
        let ctx = self.file_context_at(uri, span.start);
        let loaders = self.file_loaders(&ctx);
        let class_loader = loaders.resolved_class_loader();
        crate::type_engine::resolver::resolve_target_classes(
            subject,
            crate::AccessKind::Arrow,
            &loaders.resolution_ctx_at(content, span.start),
        )
        .into_iter()
        .filter_map(|resolved| resolved.class_info)
        .any(|class| {
            let is_field = |class: &ClassInfo| {
                class.properties.iter().any(|property| {
                    property.name == member_name.as_str() && property.writes_through_magic_set()
                })
            };
            // A field's item list comes resolved for its type arguments.
            is_field(&class)
                || is_field(&crate::virtual_members::resolve_class_fully_cached(
                    &class,
                    class_loader,
                    &self.resolved_class_cache,
                ))
        })
    }

    /// Where each declaration of `field` is written now.
    fn field_locations(&self, field: &Field) -> Vec<Location> {
        let mut locations: Vec<Location> = Vec::new();
        for declaration in &field.declarations {
            if let Some(location) = self.field_declaration_location(declaration)
                && !locations.contains(&location)
            {
                locations.push(location);
            }
        }
        locations
    }

    /// Where `declaration` is written now.
    fn field_declaration_location(&self, declaration: &FieldDeclaration) -> Option<Location> {
        let (uri, start, end) = {
            let sources = self.drupal.field_sources.read();
            match declaration {
                FieldDeclaration::BaseField {
                    uri,
                    owner,
                    method,
                    statement,
                } => {
                    let statement = sources
                        .get(&**uri)?
                        .methods
                        .iter()
                        .find(|site| site.owner == *owner && site.method == *method)?
                        .fields
                        .get(*statement)?;
                    (Arc::clone(uri), statement.start, statement.end)
                }
                FieldDeclaration::Storage { uri } => {
                    let storage = sources.get(&**uri)?.storage.as_ref()?;
                    (Arc::clone(uri), storage.start, storage.end)
                }
            }
        };
        let content = self.get_file_content_arc(&uri)?;
        let lines = crate::text_position::LineIndex::new(&content);
        Some(Location {
            uri: crate::util::parse_url(&uri)?,
            range: Range {
                start: lines.position(start as usize),
                end: lines.position(end as usize),
            },
        })
    }
}

/// Load `class` and everything it extends, implements, and uses.
fn load_class_graph(class: Atom, class_loader: &ResolvedClassLoader<'_>) {
    let mut pending = vec![class];
    let mut seen: Vec<Atom> = Vec::new();
    while let Some(name) = pending.pop() {
        if seen.contains(&name) {
            continue;
        }
        seen.push(name);
        if let Some(info) = class_loader(Fqn::new(&name)) {
            pending.extend(info.parent_class);
            pending.extend(info.interfaces.iter().copied());
            pending.extend(info.used_traits.iter().copied());
        }
    }
}

/// See [`Backend::drupal_field_flush`].
pub(crate) struct DrupalFieldFlush<'a>(&'a Backend);

impl Drop for DrupalFieldFlush<'_> {
    fn drop(&mut self) {
        self.0.flush_drupal_field_changes();
    }
}

/// Add the boolean fields `EntityFieldManager::buildBaseFieldDefinitions()`
/// adds to those of a translatable or revisionable entity type, under the
/// same conditions.
fn add_manager_fields<'a>(created: &mut Created<'a>, site: &EntityTypeSite, keys: &EntityKeys<'_>) {
    let boolean = || {
        vec![Creation {
            field_type: BOOLEAN,
            target_type: None,
            declaration: None,
        }]
    };
    let revisionable = keys.key("revision").is_some();
    if site.translatable
        && let Some(name) = keys.key("default_langcode")
        && !created.iter().any(|(known, _)| known == name)
    {
        created.push((name.to_string(), boolean()));
    }
    if revisionable && let Some(name) = keys.revision_metadata_key("revision_default") {
        set_field(created, name.to_string(), boolean());
    }
    if revisionable
        && site.translatable
        && let Some(name) = keys.key("revision_translation_affected")
    {
        set_field(created, name.to_string(), boolean());
    }
}

/// The item list a field of a field type with the item and list `classes`
/// holds, tagged with the class of the entity type `target_type` names, or
/// when it names none, the one the item class's
/// `defaultStorageSettings()` gives (`default_target`).
fn field_type(
    classes: &[(Atom, Option<Atom>)],
    target_type: Option<&str>,
    entity_types: &[(&str, &EntityTypeSite)],
    default_target: impl Fn(Atom) -> Option<String>,
) -> PhpType {
    if classes.is_empty() {
        return PhpType::named(crate::atom::atom(FIELD_ITEM_LIST_INTERFACE));
    }
    let targets = |target_type: &str| {
        let mut targets: Vec<PhpType> = Vec::new();
        for (id, site) in entity_types {
            let target = PhpType::named(site.class);
            if *id == target_type && !targets.contains(&target) {
                targets.push(target);
            }
        }
        targets
    };
    PhpType::union(
        classes
            .iter()
            .map(|(item, list)| {
                let mut args = vec![PhpType::named(*item)];
                let targets = match target_type {
                    Some(target_type) => targets(target_type),
                    None => default_target(*item)
                        .map(|target_type| targets(&target_type))
                        .unwrap_or_default(),
                };
                if !targets.is_empty() {
                    args.push(PhpType::union(targets));
                }
                PhpType::generic_atom(
                    list.unwrap_or_else(|| crate::atom::atom(FIELD_ITEM_LIST)),
                    args,
                )
            })
            .collect(),
    )
}

/// The entity type the `defaultStorageSettings()` PHP calls for the field
/// item class `item` gives as its `target_type`, following
/// `parent::defaultStorageSettings()`: the furthest ancestor's setting wins,
/// as `+` keeps its left operand's keys.
fn default_target(
    field_items: &HashMap<String, FieldItemSite>,
    item: Atom,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<String> {
    let mut start = Some(item);
    let mut depth = 0;
    // What the furthest class that adds its parent's settings writes after
    // them, for when no ancestor writes one.
    let mut otherwise: Option<Option<String>> = None;
    while let Some(from) = start
        && depth < MAX_INHERITANCE_DEPTH
    {
        depth += 1;
        let Some((_, using, declared)) = find_static_method(from, class_loader, |class| {
            field_items
                .get(&class.to_ascii_lowercase())
                .and_then(|site| site.default_target.as_ref())
        }) else {
            break;
        };
        match declared {
            DefaultTarget::Declared(target) => return target.clone(),
            DefaultTarget::FromParent { otherwise: then } => {
                if then.is_some() {
                    otherwise.clone_from(then);
                }
                start = class_loader(Fqn::new(&using)).and_then(|class| class.parent_class);
            }
        }
    }
    otherwise.flatten()
}

#[cfg(test)]
#[path = "base_fields_tests.rs"]
mod tests;
