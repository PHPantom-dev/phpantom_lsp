//! The repository Doctrine's `getRepository()` hands back for an entity
//! class: the one the entity's mapping names, which the method's declared
//! `EntityRepository<T>` / `ObjectRepository<T>` cannot know.

use crate::atom::{Atom, atom};
use crate::class_lookup::Fqn;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::ClassInfo;

/// The object managers and registries whose `getRepository()` returns the
/// repository the entity's mapping names.
const REPOSITORY_OWNERS: &[&str] = &[
    "Doctrine\\Persistence\\ObjectManager",
    "Doctrine\\Persistence\\ManagerRegistry",
    "Doctrine\\Common\\Persistence\\ObjectManager",
    "Doctrine\\Common\\Persistence\\ManagerRegistry",
];

/// What `getRepository()` on `owner` returns for the entity class its
/// first argument names, given the call's argument texts: the repository
/// the entity's mapping names, with the entity bound to its template when
/// it has one.  A `class-string` naming several entities gives each
/// repository the entities that share it.
///
/// `None` for any other call, and when an entity names no repository of
/// its own, so the call keeps the default repository the method declares.
pub(crate) fn repository_at_call(
    owner: &ClassInfo,
    method_name: &str,
    args: &[&str],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    if !method_name.eq_ignore_ascii_case("getRepository") {
        return None;
    }
    if !REPOSITORY_OWNERS
        .iter()
        .any(|base| crate::class_lookup::is_subtype_of(owner, base, ctx.file.class_loader))
    {
        return None;
    }
    let argument = entity_argument(args)?;
    let entities =
        crate::type_engine::variable::rhs_resolution::class_string_inner_binding(argument, ctx)?;

    let mut repositories: Vec<(Atom, Vec<PhpType>)> = Vec::new();
    for entity in entities.union_members() {
        let TypeKind::Named(name) = entity.kind() else {
            return None;
        };
        let entity = (ctx.file.class_loader)(Fqn::new(name))?;
        let repository = repository_for(&entity, ctx)?;
        let entity = PhpType::named(entity.fqn());
        match repositories
            .iter_mut()
            .find(|(known, _)| *known == repository)
        {
            Some((_, entities)) => entities.push(entity),
            None => repositories.push((repository, vec![entity])),
        }
    }

    let mut types = Vec::with_capacity(repositories.len());
    for (repository, entities) in repositories {
        let class = (ctx.file.class_loader)(Fqn::new(&repository))?;
        types.push(if class.template_params.is_empty() {
            PhpType::named(class.fqn())
        } else {
            PhpType::generic_atom(class.fqn(), vec![PhpType::union(entities)])
        });
    }
    Some(PhpType::union(types))
}

/// The text of the argument naming the entity class: the first, positional
/// or named.  `ObjectManager` calls the parameter `$className` and
/// `ManagerRegistry` `$persistentObject`.
fn entity_argument<'a>(args: &[&'a str]) -> Option<&'a str> {
    crate::call_args::first_param_text_arg(args, "className")
        .or_else(|| crate::call_args::first_param_text_arg(args, "persistentObject"))
}

/// The repository `entity` names in its `#[ORM\Entity]` attribute, else the
/// one a YAML or XML mapping pairs it with when exactly one does.
fn repository_for(entity: &ClassInfo, ctx: &ResolutionCtx<'_>) -> Option<Atom> {
    if let Some(repository) = entity.doctrine_repository {
        return Some(repository);
    }
    let backend = ctx.file.backend?;
    let [repository] = backend
        .framework_doctrine_repository_fqns_for_entity(&entity.fqn())
        .try_into()
        .ok()?;
    Some(atom(&repository))
}
