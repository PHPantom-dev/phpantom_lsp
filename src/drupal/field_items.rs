//! The properties of Drupal's field items, and of the item lists that hold
//! them.
//!
//! A field item class declares its properties in `propertyDefinitions()`,
//! taking its parent's with `parent::propertyDefinitions()`, and reads and
//! writes them through the magic `__get()` and `__set()` of
//! `FieldItemBase`.  [`FieldItemProvider`] gives the item class those
//! properties.  An item list reads a property on its first item
//! (`FieldItemList::__get()`), so a field typed
//! `FieldItemList<StringItem>` has `StringItem`'s properties, and a
//! reference field's target class, its list's second type argument, types
//! the `entity` property and `referencedEntities()`
//! ([`apply_to_field_type`]).  Both read the type arguments, so they apply
//! where a type hint names the list or item, not to the bare class.

use std::sync::Arc;

use crate::atom::{Atom, atom};
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{
    ClassInfo, ClassLikeKind, MAX_INHERITANCE_DEPTH, PropertyInfo, PropertySource, Visibility,
};
use crate::virtual_members::{ResolvedClassCache, VirtualMemberProvider, VirtualMembers};

use super::base_fields::find_static_method;
use super::fields::{FieldIndex, field_index};

/// The interface every field item implements.
const FIELD_ITEM: &str = "Drupal\\Core\\Field\\FieldItemInterface";
/// The interface every field item list implements.
const FIELD_ITEM_LIST: &str = "Drupal\\Core\\Field\\FieldItemListInterface";
const ENTITY: &str = "Drupal\\Core\\Entity\\EntityInterface";
const LANGUAGE: &str = "Drupal\\Core\\Language\\LanguageInterface";

/// The properties a field item class's `propertyDefinitions()` declares.
pub(crate) struct FieldItemProvider;

impl VirtualMemberProvider for FieldItemProvider {
    fn applies_to(&self, class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
        class.kind == ClassLikeKind::Class
            && super::fields::extends_or_implements(class)
            && is_subtype_of(class, FIELD_ITEM, class_loader)
    }

    fn provide(
        &self,
        class: &ClassInfo,
        class_loader: &ResolvedClassLoader<'_>,
        cache: Option<&ResolvedClassCache>,
    ) -> VirtualMembers {
        let properties = cache
            .and_then(field_index)
            .map(|index| {
                item_properties(&index, class.fqn(), class_loader)
                    .into_iter()
                    .map(|(name, data_type)| item_property(&name, data_type.as_deref()))
                    .collect()
            })
            .unwrap_or_default();
        VirtualMembers {
            methods: Vec::new(),
            properties,
            constants: Vec::new(),
        }
    }
}

/// The properties of the field item class `class`, each with the data type
/// its definition creates: those of the `propertyDefinitions()` PHP calls
/// for it, with those it takes from its parent's.
fn item_properties(
    index: &FieldIndex,
    class: Atom,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<(String, Option<String>)> {
    let mut start = Some(class);
    // The declaring classes, nearest first.
    let mut chain = Vec::new();
    while let Some(from) = start
        && chain.len() < MAX_INHERITANCE_DEPTH as usize
    {
        let Some((_, using, site)) = find_static_method(from, class_loader, |class| {
            index
                .field_items
                .get(&class.to_ascii_lowercase())
                .and_then(|site| site.property_definitions.as_ref())
        }) else {
            break;
        };
        chain.push(site);
        start = site
            .takes_parent
            .then(|| class_loader(Fqn::new(&using)).and_then(|class| class.parent_class))
            .flatten();
    }
    let mut properties: Vec<(String, Option<String>)> = Vec::new();
    for site in chain.into_iter().rev() {
        properties.retain(|(name, _)| !site.removed.contains(name));
        for property in &site.properties {
            match properties
                .iter_mut()
                .find(|(name, _)| *name == property.name)
            {
                Some(known) => known.1.clone_from(&property.data_type),
                None => properties.push((property.name.clone(), property.data_type.clone())),
            }
        }
    }
    properties
}

/// The property `name` of a field item, whose definition creates
/// `data_type`: a referenced entity or language, else the stored value,
/// which the database gives as it is.
fn item_property(name: &str, data_type: Option<&str>) -> PropertyInfo {
    let ty = match data_type {
        Some("entity") => entity_type(None),
        Some("language") => PhpType::nullable(PhpType::named(atom(LANGUAGE))),
        _ => PhpType::mixed(),
    };
    PropertyInfo {
        source: Some(PropertySource::DrupalItemProperty),
        ..PropertyInfo::virtual_property_owned(atom(name), Some(ty))
    }
}

/// What the `entity` property of a reference field's item is: its target
/// class when known, else any entity, or `null` when the field is empty.
fn entity_type(target: Option<&PhpType>) -> PhpType {
    match target {
        Some(target) => match target.kind() {
            TypeKind::Union(members) => {
                let mut members = members.to_vec();
                members.push(PhpType::null());
                PhpType::union(members)
            }
            _ => PhpType::nullable(target.clone()),
        },
        None => PhpType::nullable(PhpType::named(atom(ENTITY))),
    }
}

/// A type hint's arguments on a field item list or a field item.
#[derive(Debug)]
pub(crate) enum FieldTypeHint {
    /// `FieldItemList<Item>`, or `EntityReferenceFieldItemList<Item, Target>`
    /// for a reference field.
    List {
        item: PhpType,
        target: Option<PhpType>,
    },
    /// `EntityReferenceItem<Target>`: an item of a reference field, which an
    /// item list hands out.  `own` are the arguments, when the item class
    /// has templates of its own for them.
    Item { target: PhpType, own: Vec<PhpType> },
}

impl FieldTypeHint {
    /// The arguments the class's own templates take: the item, carrying the
    /// target, for a list, whose template is the item's; for an item, those
    /// it has templates for.
    pub(crate) fn specialisation(&self) -> Vec<PhpType> {
        match self {
            Self::List { item, target } => vec![list_item(item, target.as_ref())],
            Self::Item { own, .. } => own.clone(),
        }
    }
}

/// The items of a list of `item`, carrying the reference field's `target`
/// unless `item` names arguments of its own.
fn list_item(item: &PhpType, target: Option<&PhpType>) -> PhpType {
    match (item.base_name(), target, item.kind()) {
        (_, _, TypeKind::Generic(_)) => item.clone(),
        (Some(name), Some(target), _) => PhpType::generic_atom(atom(name), vec![target.clone()]),
        _ => item.clone(),
    }
}

/// What the type hint naming `class` with `generic_args` says about a field
/// item list or item, in a Drupal project.
pub(crate) fn field_type_hint(
    class: &ClassInfo,
    generic_args: &[PhpType],
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<FieldTypeHint> {
    if generic_args.is_empty()
        || !crate::virtual_members::active_resolved_class_cache()
            .is_some_and(|cache| cache.read().drupal().is_enabled())
    {
        return None;
    }
    if is_subtype_of(class, FIELD_ITEM_LIST, class_loader) {
        let item = generic_args[0].clone();
        item.base_name()?;
        return Some(FieldTypeHint::List {
            item,
            target: generic_args.get(1).cloned(),
        });
    }
    if class.kind == ClassLikeKind::Class && is_subtype_of(class, FIELD_ITEM, class_loader) {
        return Some(FieldTypeHint::Item {
            target: generic_args[0].clone(),
            own: if class.template_params.is_empty() {
                Vec::new()
            } else {
                generic_args.to_vec()
            },
        });
    }
    None
}

/// Give `result`, the class a type hint names, what `hint` says: a list the
/// properties of its item, and a reference field's list and item its
/// target.
pub(crate) fn apply_to_field_type(
    result: &mut ClassInfo,
    hint: &FieldTypeHint,
    class_loader: &ResolvedClassLoader<'_>,
) {
    match hint {
        FieldTypeHint::List { item, target } => {
            let Some(item_class) = item
                .base_name()
                .and_then(|name| class_loader(Fqn::new(name)))
            else {
                return;
            };
            let resolved = match target {
                Some(target) => crate::virtual_members::resolve_class_for_type_hint(
                    &item_class,
                    class_loader,
                    &[target.to_string()],
                    std::slice::from_ref(target),
                ),
                None => crate::virtual_members::resolve_class_fully_maybe_cached(
                    &item_class,
                    class_loader,
                    crate::virtual_members::active_resolved_class_cache(),
                ),
            };
            // A read from outside reaches `__get()` past a property the list
            // keeps to itself, such as its protected `$langcode`.
            let added: Vec<Arc<PropertyInfo>> = resolved
                .properties
                .iter()
                .filter(|property| {
                    property.source == Some(PropertySource::DrupalItemProperty)
                        && !result.properties.iter().any(|own| {
                            own.name == property.name && own.visibility == Visibility::Public
                        })
                })
                .cloned()
                .collect();
            if !added.is_empty() {
                let properties = result.properties.make_mut();
                properties.retain(|own| !added.iter().any(|item| item.name == own.name));
                properties.extend(added);
            }
            // A reference list's own interface names no item, which a
            // list class without templates of its own would otherwise
            // take its items' type from.
            let item = PhpType::nullable(list_item(item, target.as_ref()));
            set_return_type(result, "first", &item);
            set_return_type(result, "get", &item);
            if let Some(target) = target {
                set_return_type(
                    result,
                    "referencedEntities",
                    &PhpType::array_of(target.clone()),
                );
            }
        }
        FieldTypeHint::Item { target, .. } => {
            let untyped = entity_type(None);
            let is_entity = |property: &PropertyInfo| {
                property.source == Some(PropertySource::DrupalItemProperty)
                    && property.type_hint.as_ref() == Some(&untyped)
            };
            if !result.properties.iter().any(|property| is_entity(property)) {
                return;
            }
            let typed = entity_type(Some(target));
            for property in result.properties.make_mut().iter_mut() {
                if is_entity(property) {
                    let mut retyped = (**property).clone();
                    retyped.type_hint = Some(typed.clone());
                    *property = Arc::new(retyped);
                }
            }
        }
    }
}

/// Make `ty` the return type of `result`'s method `name`, if it has one.
fn set_return_type(result: &mut ClassInfo, name: &str, ty: &PhpType) {
    let Some(position) = result
        .methods
        .iter()
        .position(|method| method.name.eq_ignore_ascii_case(name))
    else {
        return;
    };
    if result.methods[position].return_type.as_ref() == Some(ty) {
        return;
    }
    let method = &mut result.methods.make_mut()[position];
    let mut typed = (**method).clone();
    typed.return_type = Some(ty.clone());
    *method = Arc::new(typed);
}
