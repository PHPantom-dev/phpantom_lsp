<?php declare(strict_types = 1);

// Regression: a class-level generic return type whose method has a *nullable*
// native return hint (e.g. `object|null`) must still be resolved through the
// `@extends`/`@psalm-return` template binding, not collapse to the native hint.
//
// This is the Doctrine `ServiceEntityRepository<T>::find(): ?T` shape: before
// the fix, `should_override_type_typed` analysed `object|null` with its `null`
// member attached, judged it unrefinable (both `object` and `null` are "scalar
// names"), and discarded the generic docblock return — so `$repo->find()`
// resolved to `object|null` instead of `Entity|null`.

namespace InheritedGenericReturnNullable;

use function PHPStan\Testing\assertType;

class Entity {}

/**
 * @template T of object
 */
class EntityRepository
{
	/**
	 * @return object|null
	 * @psalm-return ?T
	 */
	public function find(mixed $id): object|null {}
}

/**
 * @template T of object
 * @template-extends EntityRepository<T>
 */
class ServiceEntityRepository extends EntityRepository {}

/** @extends ServiceEntityRepository<Entity> */
class EntityRepo extends ServiceEntityRepository {}

/** @extends EntityRepository<Entity> */
class DirectRepo extends EntityRepository {}

/** @template V */
class CollectionNonNull
{
	/** @return V */
	public function get(): object {}
}

/** @extends CollectionNonNull<Entity> */
class EntityCollection extends CollectionNonNull {}

function t(EntityRepo $multi, DirectRepo $single, EntityCollection $coll): void
{
	// Two-level @extends (Doctrine's exact shape): native object|null + @psalm-return ?T
	assertType('Entity|null', $multi->find(1));
	// Single-level @extends: native object|null + @psalm-return ?T
	assertType('Entity|null', $single->find(1));
	// Control: non-nullable native object + @return V already worked before the fix
	assertType('Entity', $coll->get());
}
