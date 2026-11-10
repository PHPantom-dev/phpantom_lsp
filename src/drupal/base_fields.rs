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
//! classes let go of their parse once indexed, and read into a
//! [`FieldIndex`] on first use.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Location, Range};

use super::fields::{EntityTypeFields, FIELDABLE_ENTITY, Field, FieldDeclaration, FieldIndex};
use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::PhpType;
use crate::symbol_map::{
    BaseFieldMethodSite, BaseFieldName, EntityTypeSite, MethodScope, SymbolMap,
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
}

impl FieldSources {
    /// What `map`, the symbol map of the file at `uri`, gives, if anything.
    fn of(uri: &str, map: &SymbolMap) -> Option<Self> {
        if map.entity_types.is_empty() && map.drupal_base_field_methods.is_empty() {
            return None;
        }
        Some(Self {
            uri: Arc::from(uri),
            entity_types: super::entity_type_declarations(map)
                .map(|(id, site)| (id.to_string(), site.clone()))
                .collect(),
            methods: map.drupal_base_field_methods.clone(),
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
    }
}

/// The source files of the field index, by URI.
pub(crate) type FieldSourceMap = BTreeMap<String, Arc<FieldSources>>;

/// The entity keys and revision metadata keys of one entity type, with the
/// defaults Drupal's entity type classes give them.
struct EntityKeys<'a> {
    keys: HashMap<&'a str, &'a str>,
    revision_metadata: HashMap<&'a str, &'a str>,
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
        let Some((owner, uri, site)) = self.find(start, method) else {
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
            let from = match include.scope {
                MethodScope::Static => Some(entity_class),
                MethodScope::Self_ => Some(owner),
                MethodScope::Parent => self.load(owner).and_then(|class| class.parent_class),
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
        // The method's own statements replace what it took from others.
        let mut own: Created<'a> = Vec::new();
        for (statement, field) in site.fields.iter().enumerate() {
            let Some(name) = keys.name_of(&field.name) else {
                continue;
            };
            let creation = Creation {
                field_type: &field.field_type,
                target_type: field.target_type.as_deref(),
                declaration: Some(FieldDeclaration {
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
    /// `start`, as PHP finds it: the class's own, then its traits', then
    /// its parent's.
    fn find(
        &self,
        start: Atom,
        method: &str,
    ) -> Option<(Atom, &'a Arc<str>, &'a BaseFieldMethodSite)> {
        let method = method.to_ascii_lowercase();
        let mut current = Some(start);
        let mut depth = 0;
        while let Some(class) = current
            && depth < MAX_INHERITANCE_DEPTH
        {
            depth += 1;
            if let Some(found) = self.declared_in(class, &method, 0) {
                return Some(found);
            }
            current = self.load(class).and_then(|info| info.parent_class);
        }
        None
    }

    /// The base field method `method` of `class` or of a trait it uses.
    fn declared_in(
        &self,
        class: Atom,
        method: &str,
        depth: u32,
    ) -> Option<(Atom, &'a Arc<str>, &'a BaseFieldMethodSite)> {
        if let Some((uri, site)) = self
            .methods
            .get(&(class.to_ascii_lowercase(), method.to_string()))
        {
            return Some((class, uri, site));
        }
        if depth >= MAX_INHERITANCE_DEPTH {
            return None;
        }
        self.load(class)?
            .used_traits
            .iter()
            .find_map(|used| self.declared_in(*used, method, depth + 1))
    }

    fn load(&self, class: Atom) -> Option<Arc<ClassInfo>> {
        (self.class_loader)(Fqn::new(&class))
    }
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
        let sources = map
            .filter(|_| self.is_drupal_project())
            .and_then(|map| FieldSources::of(uri, map))
            .filter(|_| {
                // A test module's entity types are only seen by test code,
                // while a class's fields are the same whoever asks, and a
                // dependency's are never part of the project's own.
                !crate::util::uri_to_path(uri).is_some_and(|path| self.is_drupal_test_path(&path))
                    && !self
                        .workspace
                        .vendor_uri_prefixes
                        .lock()
                        .iter()
                        .any(|prefix| uri.starts_with(prefix.as_str()))
            });
        let old = self.drupal.field_sources.read().get(uri).cloned();
        if old.as_deref() == sources.as_ref() {
            return;
        }
        let same_fields = match (old.as_deref(), sources.as_ref()) {
            (Some(old), Some(new)) => old.declares_as(new),
            _ => false,
        };
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
    }

    /// Drop the field index and the resolved classes that have fields: the
    /// fields of an entity class show on interfaces that do not depend on
    /// it, so every class under `FieldableEntityInterface` goes.
    pub(crate) fn invalidate_drupal_fields(&self) {
        self.drupal.fields.invalidate();
        {
            let mut cache = self.resolved_class_cache.write();
            crate::virtual_members::evict_fqn(&mut cache, FIELDABLE_ENTITY);
        }
        // A cached receiver resolution records no dependency on the files
        // the fields are read from.
        self.clear_resolved_member_files();
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
        let loader = self.resolved_class_loader_with(&[]);
        let mut methods = HashMap::new();
        let mut declared: Vec<(&str, &EntityTypeSite)> = Vec::new();
        for source in &sources {
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
        for &(id, site) in declared.iter().filter(|(_, site)| site.content) {
            let keys = EntityKeys::of(site);
            let mut created = reader.fields(
                site.class,
                site.class,
                "baseFieldDefinitions",
                &keys,
                &mut Vec::new(),
            );
            add_manager_fields(&mut created, site, &keys);
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
                                field_type(classes, creation.target_type, &declared)
                            })
                            .collect(),
                    );
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
        index
    }

    /// Where the field `name` of `receiver` is declared: the first of the
    /// declarations the entity types it may hold give, when the property
    /// `name` of `receiver` is that field.
    pub(crate) fn drupal_field_location(
        &self,
        receiver: &ClassInfo,
        name: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Option<Location> {
        if !self.is_drupal_project() {
            return None;
        }
        let resolved = crate::virtual_members::resolve_class_fully_cached(
            receiver,
            class_loader,
            &self.resolved_class_cache,
        );
        if !resolved
            .properties
            .iter()
            .any(|property| property.name == name && property.writes_through_magic_set())
        {
            return None;
        }
        let index = self.drupal_field_index()?;
        let field = index
            .fields_of(receiver, class_loader)
            .into_iter()
            .find(|field| field.name == name)?;
        let declaration = field.declarations.first()?;
        let (start, end) = {
            let sources = self.drupal.field_sources.read();
            let site = sources
                .get(&*declaration.uri)?
                .methods
                .iter()
                .find(|site| {
                    site.owner == declaration.owner && site.method == declaration.method
                })?;
            let statement = site.fields.get(declaration.statement)?;
            (statement.start, statement.end)
        };
        let uri = &*declaration.uri;
        let content = self.get_file_content_arc(uri)?;
        let lines = crate::text_position::LineIndex::new(&content);
        Some(Location {
            uri: crate::util::parse_url(uri)?,
            range: Range {
                start: lines.position(start as usize),
                end: lines.position(end as usize),
            },
        })
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
/// holds, tagged with the class of the entity type `target_type` names.
fn field_type(
    classes: &[(Atom, Option<Atom>)],
    target_type: Option<&str>,
    entity_types: &[(&str, &EntityTypeSite)],
) -> PhpType {
    if classes.is_empty() {
        return PhpType::named(crate::atom::atom(FIELD_ITEM_LIST_INTERFACE));
    }
    let mut targets: Vec<PhpType> = Vec::new();
    if let Some(target_type) = target_type {
        for (id, site) in entity_types {
            let target = PhpType::named(site.class);
            if *id == target_type && !targets.contains(&target) {
                targets.push(target);
            }
        }
    }
    PhpType::union(
        classes
            .iter()
            .map(|(item, list)| {
                let mut args = vec![PhpType::named(*item)];
                if !targets.is_empty() {
                    args.push(PhpType::union(targets.clone()));
                }
                PhpType::generic_atom(
                    list.unwrap_or_else(|| crate::atom::atom(FIELD_ITEM_LIST)),
                    args,
                )
            })
            .collect(),
    )
}

#[cfg(test)]
#[path = "base_fields_tests.rs"]
mod tests;
