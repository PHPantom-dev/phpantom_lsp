//! Expanding the model operators (`builder-of<Model>`,
//! `collection-of<Model>`, `factory-of<Model>`, `relation-of<Model, 'path'>`)
//! to the classes they name.

use std::sync::Arc;

use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ELOQUENT_COLLECTION_FQN};

use super::collections::{collection_type_for, custom_collection_for_model};
use super::{
    ELOQUENT_BUILDER_FQN, ELOQUENT_MODEL_FQN, ELOQUENT_RELATION_FQN, custom_builder_fqn,
    extends_eloquent_model, factory, resolve_relation_chain, resolve_relation_type,
};

/// Replace every model operator in `ty` — `builder-of<Model>`,
/// `collection-of<Model>`, `factory-of<Model>`, `relation-of<Model, 'path'>`
/// — with the class it names, wherever in the type it appears.
///
/// Each operator is resolved as far as the project allows: a model's own
/// builder, collection, or factory when it declares one, the model a
/// relation path ends on, and the framework's base class otherwise (see
/// [`model_type_fallback`]).  A union of models becomes a union of their
/// classes, resolved model by model, so one member falling back never costs
/// the others their concrete class.
///
/// Types holding no operator are returned as they were, which
/// [`has_model_type_operator`] makes cheap enough to ask on every type.
pub(crate) fn expand_model_type(
    ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> PhpType {
    if !has_model_type_operator(ty) {
        return ty.clone();
    }
    if let TypeKind::Generic(g) = ty.kind()
        && matches!(
            g.name.as_str(),
            "builder-of" | "collection-of" | "factory-of" | "relation-of"
        )
    {
        let (model, key) = if g.name == "collection-of" && g.args.len() == 2 {
            (&g.args[1], Some(&g.args[0]))
        } else if let Some(model) = g.args.first() {
            (model, None)
        } else {
            return model_type_fallback(&g.name, None, None);
        };
        if g.name == "relation-of" && g.args.len() != 2 {
            return model_type_fallback(&g.name, None, None);
        }
        let models: &[PhpType] = match model.kind() {
            TypeKind::Union(members) => members,
            _ => std::slice::from_ref(model),
        };
        let mut resolved = Vec::with_capacity(models.len());
        for model in models {
            let model_name = model.base_name().or_else(|| match model.kind() {
                TypeKind::Intersection(members) => members.iter().find_map(|member| {
                    let name = member.base_name()?;
                    let cls = class_loader(name)?;
                    extends_eloquent_model(&cls, class_loader).then_some(name)
                }),
                _ => None,
            });
            let model_class = model_name.and_then(class_loader).filter(|class| {
                crate::virtual_members::laravel::extends_eloquent_model(class, class_loader)
            });
            let Some(model_class) = model_class else {
                resolved.push(model_type_fallback(&g.name, None, key));
                continue;
            };
            let concrete = match g.name.as_str() {
                "builder-of" => {
                    let relations: &[PhpType] = match g.args.get(1).map(PhpType::kind) {
                        Some(TypeKind::Union(names)) => names,
                        _ => g.args.get(1).map(std::slice::from_ref).unwrap_or(&[]),
                    };
                    let targets: Vec<_> = relations
                        .iter()
                        .filter_map(|relation| {
                            let path = relation.as_literal()?.string_content()?;
                            let fqn = resolve_relation_chain(
                                &model_class,
                                &path,
                                class_loader,
                                crate::virtual_members::active_resolved_class_cache(),
                            )?;
                            class_loader(&fqn)
                        })
                        .collect();
                    let targets: Vec<_> = if targets.is_empty() {
                        vec![Arc::clone(&model_class)]
                    } else {
                        targets
                    };
                    PhpType::union(
                        targets
                            .iter()
                            .map(|target| {
                                let builder = custom_builder_fqn(target, class_loader)
                                    .filter(|name| class_loader(name).is_some())
                                    .unwrap_or_else(|| ELOQUENT_BUILDER_FQN.to_string());
                                let target_model = if target.fqn() == model_class.fqn() {
                                    model.clone()
                                } else {
                                    PhpType::named(atom(&target.fqn()))
                                };
                                PhpType::generic(builder, vec![target_model])
                            })
                            .collect(),
                    )
                }
                "collection-of" => {
                    let args = [
                        key.cloned().unwrap_or_else(|| {
                            PhpType::union(vec![PhpType::int(), PhpType::string()])
                        }),
                        model.clone(),
                    ];
                    if let Some(collection) =
                        custom_collection_for_model(&model_class.fqn(), class_loader)
                            .filter(|name| class_loader(name).is_some())
                    {
                        collection_type_for(&collection, &args, class_loader)
                    } else {
                        PhpType::generic(ELOQUENT_COLLECTION_FQN, args.to_vec())
                    }
                }
                "factory-of" => factory::factory_for_model(&model_class, class_loader),
                "relation-of" => {
                    let relations: &[PhpType] = match g.args[1].kind() {
                        TypeKind::Union(names) => names,
                        _ => std::slice::from_ref(&g.args[1]),
                    };
                    let resolved: Vec<PhpType> = relations
                        .iter()
                        .filter_map(|relation| {
                            let path = relation.as_literal()?.string_content()?;
                            resolve_relation_type(
                                &model_class,
                                &path,
                                class_loader,
                                crate::virtual_members::active_resolved_class_cache(),
                            )
                        })
                        .collect();
                    if resolved.is_empty() {
                        model_type_fallback(&g.name, Some(model), key)
                    } else {
                        PhpType::union(resolved)
                    }
                }
                _ => unreachable!(),
            };
            resolved.push(concrete);
        }
        return PhpType::union(resolved);
    }
    match ty.kind() {
        TypeKind::Generic(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Nullable(_)
        | TypeKind::Array(_)
        | TypeKind::ArrayShape(_)
        | TypeKind::ObjectShape(_)
        | TypeKind::Callable(_)
        | TypeKind::Conditional(_)
        | TypeKind::ClassString(_)
        | TypeKind::InterfaceString(_)
        | TypeKind::KeyOf(_)
        | TypeKind::ValueOf(_)
        | TypeKind::IndexAccess(..) => {
            ty.map_children(&|child| expand_model_type(child, class_loader))
        }
        _ => ty.clone(),
    }
}

/// The framework's own class for a model operator whose subject could not
/// be pinned down: the model does not resolve, does not extend Eloquent's
/// `Model`, or the relation path names nothing.
///
/// Answering with the base class rather than leaving the pseudo-type
/// standing is what keeps such a value usable: `builder-of<Whatever>` that
/// resolves to nothing types the subject as nothing at all, where
/// `Builder<Model>` still carries every method the framework declares.  It
/// is also the same answer Laravel's own annotations give where they cannot
/// be more specific.
///
/// `model` is the model the operator named, when that much is known — a
/// relation path that fails to resolve still leaves the declaring model
/// settled — and `key` the key type a `collection-of` was written with.
fn model_type_fallback(kind: &str, model: Option<&PhpType>, key: Option<&PhpType>) -> PhpType {
    let model = model
        .cloned()
        .unwrap_or_else(|| PhpType::named(atom(ELOQUENT_MODEL_FQN)));
    match kind {
        "builder-of" => PhpType::generic(ELOQUENT_BUILDER_FQN, vec![model]),
        "collection-of" => PhpType::generic(
            ELOQUENT_COLLECTION_FQN,
            vec![
                key.cloned()
                    .unwrap_or_else(|| PhpType::union(vec![PhpType::int(), PhpType::string()])),
                model,
            ],
        ),
        "factory-of" => PhpType::generic(factory::FACTORY_FQN, vec![model]),
        // `Relation<TRelatedModel, TDeclaringModel, TResult>`: the related
        // model is what a failed path lost, so only the declaring one is
        // filled in.
        "relation-of" => PhpType::generic(
            ELOQUENT_RELATION_FQN,
            vec![
                PhpType::named(atom(ELOQUENT_MODEL_FQN)),
                model,
                PhpType::mixed(),
            ],
        ),
        _ => unreachable!("every model operator has a base class"),
    }
}

/// Whether a model operator appears anywhere in `ty`.
///
/// Allocation-free guard for [`expand_model_type`], which clones as it
/// rewrites: almost no type names one of these operators, and walking the
/// tree twice is far cheaper than copying it once.
pub(crate) fn has_model_type_operator(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Generic(g) => {
            matches!(
                g.name.as_str(),
                "builder-of" | "collection-of" | "factory-of" | "relation-of"
            ) || g.args.iter().any(has_model_type_operator)
        }
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            members.iter().any(has_model_type_operator)
        }
        TypeKind::Nullable(inner)
        | TypeKind::Array(inner)
        | TypeKind::ClassString(Some(inner))
        | TypeKind::InterfaceString(Some(inner))
        | TypeKind::KeyOf(inner)
        | TypeKind::ValueOf(inner) => has_model_type_operator(inner),
        TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => entries
            .iter()
            .any(|entry| has_model_type_operator(&entry.value_type)),
        TypeKind::Callable(c) => {
            c.params
                .iter()
                .any(|p| has_model_type_operator(&p.type_hint))
                || c.return_type.as_ref().is_some_and(has_model_type_operator)
        }
        TypeKind::Conditional(c) => {
            has_model_type_operator(&c.condition)
                || has_model_type_operator(&c.then_type)
                || has_model_type_operator(&c.else_type)
        }
        TypeKind::IndexAccess(base, key) => {
            has_model_type_operator(base) || has_model_type_operator(key)
        }
        _ => false,
    }
}
