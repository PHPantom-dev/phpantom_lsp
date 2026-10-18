//! Rewriting Eloquent's base collection to the collection a model actually
//! builds.
//!
//! Called from the call and return-type resolvers under `type_engine/`
//! after a class has been resolved and its generics substituted.  Keeping
//! the framework logic here rather than inline in the generic resolver
//! avoids coupling the type engine to Laravel conventions.

use std::sync::Arc;

use crate::atom::atom;
use crate::inheritance::ancestors;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ELOQUENT_COLLECTION_FQN};

/// Swap a resolved Eloquent Collection to a model's custom collection.
///
/// When the resolved class is `Illuminate\Database\Eloquent\Collection`
/// and one of the generic type arguments is a model with a
/// `custom_collection` declared (via `#[CollectedBy]` or
/// `@use HasCollection<X>`), returns the custom collection class
/// instead.  This handles the common chain pattern:
///
/// ```php
/// Model::where(...)->get()  // returns Collection<int, TModel>
/// ```
///
/// where `TModel` has been substituted to the concrete model and the
/// model declares a custom collection like `ProductCollection`.
///
/// Only called for a *returned* type hint (a method's return type, never
/// a parameter/property/`@var` hint): a value the caller merely declares
/// as the base collection may be one it built itself, so treating it as
/// the custom subclass there would be an over-claim.  A collection is
/// only known to be the custom one when Eloquent actually produced it,
/// which is what a return position proves.
///
/// Returns `cls` unchanged when it is not the Eloquent Collection, has no
/// generic args, or the model does not declare a custom collection.
pub(crate) fn try_swap_custom_collection(
    cls: Arc<ClassInfo>,
    base_fqn: &str,
    generic_args: &[PhpType],
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Arc<ClassInfo> {
    if base_fqn != crate::types::ELOQUENT_COLLECTION_FQN || generic_args.is_empty() {
        return cls;
    }

    // The last generic arg is typically the model type.
    let model_name = match generic_args.last().unwrap().base_name() {
        Some(name) => name.to_string(),
        None => return cls,
    };
    let model_class = crate::class_lookup::find_class_by_name(all_classes, &model_name)
        .map(Arc::clone)
        .or_else(|| class_loader(&model_name));

    if let Some(ref mc) = model_class
        && let Some(coll_type) = mc.laravel().and_then(|l| l.custom_collection.as_ref())
    {
        let coll_name = coll_type.to_string();
        crate::class_lookup::find_class_by_name(all_classes, &coll_name)
            .map(Arc::clone)
            .or_else(|| class_loader(&coll_name))
            .unwrap_or(cls)
    } else {
        cls
    }
}

/// Rewrite every `Illuminate\Database\Eloquent\Collection<…, TModel>`
/// node in a type to the collection class the model actually builds.
///
/// Laravel's own `Builder::get()`, `Relation::get()` and friends are
/// annotated `@return Collection<int, TModel>`.  A model that declares a
/// custom collection (via `#[CollectedBy]`, `@use HasCollection<X>`, or a
/// `newCollection()` override) hands back that subclass at runtime, so the
/// declared base type is wrong for every such model.  Substituting
/// `TModel` alone leaves `Collection<int, Audience>` where the code
/// (correctly) declares `AudienceCollection`.
///
/// The model is therefore read off the *last* generic argument, so a builder that
/// returns some other model's collection resolves to that model's
/// collection class rather than the receiver's.
///
/// The generic arity of the result follows the target collection class:
/// a collection with two template parameters keeps `<key, model>`, one
/// keeps `<model>`, and a non-generic subclass (the common
/// `@extends Collection<int, Model>` shape) becomes a bare class name.
///
/// Only a collection Laravel *produces* is rewritten: the type itself when
/// it is a return, and the parameters of a callback it hands one to.  A
/// callback's own return is something the caller has to produce, so the
/// declared base type stands there, as it does for a parameter (see
/// [`replace_eloquent_collections_in_param_type`]).
///
/// Returns `None` when nothing was rewritten, so callers on the hot path
/// keep their existing type without allocating a copy.
pub(crate) fn replace_eloquent_collections_in_type(
    ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    if !mentions_eloquent_collection(ty) {
        return None;
    }
    rewrite_eloquent_collections(ty, true, class_loader)
}

/// [`replace_eloquent_collections_in_type`] for a parameter type.
///
/// A parameter is a demand on the caller, and narrowing it to the custom
/// collection would reject a plain `Collection<int, Customer>` the code
/// built itself.  What the callee hands back through it is produced, so
/// only a callback's parameters are rewritten:
/// `chunk(100, fn (CustomerCollection $c) => …)`.
pub(crate) fn replace_eloquent_collections_in_param_type(
    ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    if !mentions_eloquent_collection(ty) {
        return None;
    }
    rewrite_eloquent_collections(ty, false, class_loader)
}

/// Cheap pre-check for [`replace_eloquent_collections_in_type`].
///
/// Walking the tree twice is still cheaper than cloning it: the vast
/// majority of return types never name the Eloquent collection, and this
/// pass allocates nothing.
pub(crate) fn mentions_eloquent_collection(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Generic(g) => {
            is_eloquent_collection_name(&g.name) || g.args.iter().any(mentions_eloquent_collection)
        }
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            members.iter().any(mentions_eloquent_collection)
        }
        TypeKind::Nullable(inner) | TypeKind::Array(inner) => mentions_eloquent_collection(inner),
        TypeKind::Callable(callable) => {
            callable
                .params
                .iter()
                .any(|p| mentions_eloquent_collection(&p.type_hint))
                || callable
                    .return_type
                    .as_ref()
                    .is_some_and(mentions_eloquent_collection)
        }
        _ => false,
    }
}

fn is_eloquent_collection_name(name: &str) -> bool {
    name.trim_start_matches('\\') == ELOQUENT_COLLECTION_FQN
}

/// Rewrite the Eloquent collections in `ty`, swapping only those in a
/// produced position.  `produced` flips at each callable's parameter list,
/// since a callback's parameters are produced by whoever calls it.
fn rewrite_eloquent_collections(
    ty: &PhpType,
    produced: bool,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    match ty.kind() {
        TypeKind::Generic(g) if is_eloquent_collection_name(&g.name) => {
            if !produced {
                return None;
            }
            let model = g.args.last()?.base_name()?;
            let collection = custom_collection_for_model(model, class_loader)?;
            Some(collection_type_for(&collection, &g.args, class_loader))
        }
        TypeKind::Generic(g) => {
            let args = rewrite_members(&g.args, produced, class_loader)?;
            Some(PhpType::generic_atom(g.name, args))
        }
        TypeKind::Union(members) => Some(PhpType::union(rewrite_members(
            members,
            produced,
            class_loader,
        )?)),
        TypeKind::Intersection(members) => Some(PhpType::intersection(rewrite_members(
            members,
            produced,
            class_loader,
        )?)),
        TypeKind::Nullable(inner) => Some(PhpType::nullable(rewrite_eloquent_collections(
            inner,
            produced,
            class_loader,
        )?)),
        TypeKind::Array(inner) => Some(PhpType::array_of(rewrite_eloquent_collections(
            inner,
            produced,
            class_loader,
        )?)),
        TypeKind::Callable(callable) => {
            let mut rewritten = (**callable).clone();
            let mut changed = false;
            for param in &mut rewritten.params {
                if let Some(new) =
                    rewrite_eloquent_collections(&param.type_hint, !produced, class_loader)
                {
                    param.type_hint = new;
                    changed = true;
                }
            }
            if let Some(ret) = &rewritten.return_type
                && let Some(new) = rewrite_eloquent_collections(ret, produced, class_loader)
            {
                rewritten.return_type = Some(new);
                changed = true;
            }
            changed.then(|| TypeKind::Callable(Box::new(rewritten)).into())
        }
        _ => None,
    }
}

/// Rewrite a list of type members, returning `None` when none changed.
fn rewrite_members(
    members: &[PhpType],
    produced: bool,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Vec<PhpType>> {
    let mut changed = false;
    let rewritten: Vec<PhpType> = members
        .iter()
        .map(
            |m| match rewrite_eloquent_collections(m, produced, class_loader) {
                Some(new) => {
                    changed = true;
                    new
                }
                None => m.clone(),
            },
        )
        .collect();
    changed.then_some(rewritten)
}

/// Look up the custom collection declared by a model or, failing that,
/// inherited from one of its parents.
///
/// `#[CollectedBy]`, `$collectionClass` and `newCollection()` overrides
/// all live on the class that declares them but apply to every subclass,
/// so a shared base model can set the collection for a whole hierarchy.
pub(super) fn custom_collection_for_model(
    model: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    let declared = |candidate: &ClassInfo| {
        candidate
            .laravel()
            .and_then(|l| l.custom_collection.as_ref())
            .and_then(|collection| collection.base_name())
            .map(str::to_owned)
    };
    let model_class = class_loader(model)?;
    declared(&model_class)
        .or_else(|| ancestors(&model_class, class_loader).find_map(|(_, parent)| declared(&parent)))
}

/// Build the replacement type for a custom collection class, matching its
/// generic arity to the class's own template parameters.
pub(super) fn collection_type_for(
    collection: &str,
    base_args: &[PhpType],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> PhpType {
    let arity = class_loader(collection).map_or(0, |c| c.template_params.len());
    match arity {
        0 => PhpType::named(atom(collection)),
        1 => PhpType::generic(
            collection,
            vec![base_args.last().cloned().unwrap_or_else(PhpType::mixed)],
        ),
        _ if arity >= base_args.len() => PhpType::generic(collection, base_args.to_vec()),
        // More arguments than the target accepts: keep the trailing ones,
        // which are the value types (`<key, model>` → `<model>`).
        n => PhpType::generic(collection, base_args[base_args.len() - n..].to_vec()),
    }
}
