<?php declare(strict_types = 1);

// Regression: a generic class declared inside a conditional `if`/`else` block
// must still be indexed with its parent and `@template-extends` generics, so
// the inheritance chain resolves the class-level template to the concrete type.
//
// This is the Doctrine `ServiceEntityRepository` shape: doctrine-bundle defines
// `ServiceEntityRepository` differently for ORM2 vs ORM3 inside an
// `if (! property_exists(EntityRepository::class, '_entityName')) { ... } else { ... }`
// guard. Before the fix, a class declared inside such a block was discovered by
// name only — its parent and `@extends` generics were dropped — so the chain
// `ConcreteRepo -> ServiceEntityRepository<T> -> EntityRepository<T>` collapsed
// and `$repo->get()` resolved to `object`/`mixed` instead of the entity.

namespace ConditionalClassGenericExtends;

use function PHPStan\Testing\assertType;

class Entity {}

/**
 * @template T of object
 */
class EntityRepository
{
	/** @return T */
	public function get(): object {}
}

// The same FQN is declared in both branches of a runtime guard; the first
// (source-order) declaration wins. Both branches bind
// `@template-extends EntityRepository<T>`, mirroring Doctrine's ORM3/ORM2 split.
if (\PHP_VERSION_ID >= 80000) {
	/**
	 * @template T of object
	 * @template-extends EntityRepository<T>
	 */
	class ServiceEntityRepository extends EntityRepository {}
} else {
	/**
	 * @template T of object
	 * @template-extends EntityRepository<T>
	 */
	class ServiceEntityRepository extends EntityRepository {}
}

/** @extends ServiceEntityRepository<Entity> */
class EntityRepo extends ServiceEntityRepository {}

function t(EntityRepo $repo): void
{
	// Chain resolves through a class declared inside the `if` branch:
	// EntityRepo -> ServiceEntityRepository<Entity> -> EntityRepository<Entity>.
	assertType('Entity', $repo->get());
}
