//! The value a key path reaches inside an array or an object.
//!
//! Helpers like Yii's `ArrayHelper::getValueByPath($user, 'profile.city')`
//! and Laravel's `data_get()` and `pluck()` walk a value one key at a time:
//! a key of an array, or a property of an object.  This module takes one
//! such step on a type ([`segment_type`]), a whole path of them
//! ([`path_type`]), and lists the keys a step can take from a type
//! ([`segment_names`]), so every helper that reads a path types it, and
//! completes it, the same way.  Each helper splits its own path into
//! segments, since they disagree on delimiters, escapes, and wildcards.

use std::sync::Arc;

use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::{ClassInfo, Visibility};

/// What a path may step through besides an array's keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyPathRules {
    /// Whether a segment reads a property of an object.
    pub(crate) properties: bool,
    /// Whether a segment written `name()` calls a method of an object, as
    /// Yii's `ArrayHelper` reads it.
    pub(crate) method_calls: bool,
}

/// The value a path reached, and whether the path may have named nothing
/// on the way there (an optional key, a key of an array with no known
/// shape, a `null`), where the helper gives its default instead.
#[derive(Clone, Debug)]
pub(crate) struct KeyStep {
    pub(crate) ty: PhpType,
    pub(crate) may_miss: bool,
}

/// Take the step `segment` names from a value of type `value`.
///
/// `None` when the type says nothing about what the segment reaches: a key
/// a closed shape does not list, a property no class of the value has, or
/// a value that is neither an array nor an object.
pub(crate) fn segment_type(
    value: &PhpType,
    segment: &str,
    rules: KeyPathRules,
    ctx: &ResolutionCtx<'_>,
) -> Option<KeyStep> {
    match value.kind() {
        TypeKind::Nullable(inner) => {
            let step = segment_type(inner, segment, rules, ctx)?;
            return Some(KeyStep {
                ty: step.ty,
                may_miss: true,
            });
        }
        TypeKind::Union(members) => {
            let mut types = Vec::with_capacity(members.len());
            let mut may_miss = false;
            for member in members {
                if member.is_null() {
                    may_miss = true;
                    continue;
                }
                let step = segment_type(member, segment, rules, ctx)?;
                may_miss |= step.may_miss;
                types.push(step.ty);
            }
            return Some(KeyStep {
                ty: if types.is_empty() {
                    PhpType::null()
                } else {
                    PhpType::union(types)
                },
                may_miss,
            });
        }
        _ => {}
    }
    if value.is_null() {
        return Some(KeyStep {
            ty: PhpType::null(),
            may_miss: true,
        });
    }
    if let Some(entry) = value.shape_entry(segment) {
        return Some(KeyStep {
            ty: entry.value_type.clone(),
            may_miss: entry.optional,
        });
    }
    match value.kind() {
        TypeKind::ArrayShape(_) | TypeKind::ObjectShape(_) => return None,
        _ if value.is_array_like() => {
            return Some(KeyStep {
                ty: value.iterable_element_type()?,
                may_miss: true,
            });
        }
        _ => {}
    }
    if !rules.properties {
        return None;
    }
    let method = segment.strip_suffix("()").filter(|_| rules.method_calls);
    let types: Vec<PhpType> = value_classes(value, ctx)
        .iter()
        .filter_map(|class| match method {
            Some(method) => {
                crate::inheritance::resolve_method_return_type(class, method, ctx.file.class_loader)
                    .map(|ty| ty.replace_self_bound(&class.fqn(), None))
            }
            None => crate::inheritance::resolve_property_type_hint(
                class,
                segment,
                ctx.file.class_loader,
            ),
        })
        .collect();
    (!types.is_empty()).then(|| KeyStep {
        ty: PhpType::union(types),
        may_miss: false,
    })
}

/// Walk `segments` from a value of type `value`, one [`segment_type`] at a
/// time.  The path may miss when any step may.
pub(crate) fn path_type<'s>(
    value: &PhpType,
    segments: impl IntoIterator<Item = &'s str>,
    rules: KeyPathRules,
    ctx: &ResolutionCtx<'_>,
) -> Option<KeyStep> {
    let mut reached = KeyStep {
        ty: value.clone(),
        may_miss: false,
    };
    for segment in segments {
        let step = segment_type(&reached.ty, segment, rules, ctx)?;
        reached = KeyStep {
            ty: step.ty,
            may_miss: reached.may_miss || step.may_miss,
        };
    }
    Some(reached)
}

/// What kind of name a segment is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SegmentKind {
    Key,
    Property,
    Method,
}

/// A segment a step from some value can take.
#[derive(Clone, Debug)]
pub(crate) struct SegmentName {
    /// The segment as written in a path: a key or a property name.
    pub(crate) name: String,
    pub(crate) kind: SegmentKind,
    /// The type of what the segment reaches, when known.
    pub(crate) ty: Option<PhpType>,
}

/// The segments a step from a value of type `value` can take: the keys
/// its shapes list and, by `rules`, the public properties of its classes.
/// Each name appears once, at its first sighting.
///
/// A method a path can call is left out: it is called for its result, and
/// a path is no place to offer `save()` or `delete()` alongside the reads.
pub(crate) fn segment_names(
    value: &PhpType,
    rules: KeyPathRules,
    ctx: &ResolutionCtx<'_>,
) -> Vec<SegmentName> {
    let mut names: Vec<SegmentName> = Vec::new();
    let mut push = |name: String, kind: SegmentKind, ty: Option<PhpType>| {
        if !names.iter().any(|known| known.name == name) {
            names.push(SegmentName { name, kind, ty });
        }
    };
    for member in value_members(value) {
        if let Some(entries) = member.known_shape_entries() {
            for entry in entries {
                if let Some(key) = &entry.key {
                    push(
                        key.clone(),
                        SegmentKind::Key,
                        Some(entry.value_type.clone()),
                    );
                }
            }
            continue;
        }
        if !rules.properties || member.is_array_like() {
            continue;
        }
        for class in value_classes(member, ctx) {
            let class = crate::virtual_members::resolve_class_fully_maybe_cached(
                &class,
                ctx.file.class_loader,
                ctx.resolved_class_cache,
            );
            for property in class.properties.iter() {
                if property.visibility == Visibility::Public {
                    push(
                        property.name.trim_start_matches('$').to_string(),
                        SegmentKind::Property,
                        property.type_hint.clone(),
                    );
                }
            }
        }
    }
    names
}

/// The classes of a value of type `value` that have the property or the
/// method the segment `name` reads, with which of the two it is.  A key of
/// an array is declared by no class, so it has none.
pub(crate) fn segment_owners(
    value: &PhpType,
    name: &str,
    rules: KeyPathRules,
    ctx: &ResolutionCtx<'_>,
) -> Vec<(Arc<ClassInfo>, SegmentKind)> {
    if !rules.properties {
        return Vec::new();
    }
    let method = name.strip_suffix("()").filter(|_| rules.method_calls);
    let mut owners = Vec::new();
    for member in value_members(value) {
        if member.known_shape_entries().is_some() || member.is_array_like() {
            continue;
        }
        for class in value_classes(member, ctx) {
            let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
                &class,
                ctx.file.class_loader,
                ctx.resolved_class_cache,
            );
            let found = match method {
                Some(method) => resolved
                    .get_method_ci(method)
                    .is_some()
                    .then_some(SegmentKind::Method),
                None => resolved
                    .properties
                    .iter()
                    .any(|property| property.name == name)
                    .then_some(SegmentKind::Property),
            };
            if let Some(kind) = found
                && !owners
                    .iter()
                    .any(|(known, _): &(Arc<ClassInfo>, _)| known.fqn() == class.fqn())
            {
                owners.push((class, kind));
            }
        }
    }
    owners
}

/// The members of a union or a nullable type, and the type itself
/// otherwise, `null` left out.
fn value_members(value: &PhpType) -> Vec<&PhpType> {
    match value.kind() {
        TypeKind::Nullable(inner) => value_members(inner),
        TypeKind::Union(members) => members
            .iter()
            .flat_map(value_members)
            .filter(|member| !member.is_null())
            .collect(),
        _ if value.is_null() => Vec::new(),
        _ => vec![value],
    }
}

/// The classes a value of type `value` is an instance of.
fn value_classes(value: &PhpType, ctx: &ResolutionCtx<'_>) -> Vec<Arc<ClassInfo>> {
    crate::type_engine::type_resolution::type_hint_to_classes_typed(
        value,
        "",
        ctx.file.all_classes,
        ctx.file.class_loader,
    )
}
