<?php declare(strict_types = 1);

namespace NamedManagerRepositoryInference;

use Doctrine\ORM\EntityRepository;
use Doctrine\Persistence\ManagerRegistry;
use function PHPStan\Testing\assertType;

class Example
{

	public function explicitManagerName(ManagerRegistry $registry): void
	{
		assertType(DefaultSharedRepository::class . '<' . SharedEntity::class . '>', $registry->getRepository(SharedEntity::class, 'default')); // SKIP: a repository an object manager sets at runtime
		assertType(TenantSharedRepository::class . '<' . SharedEntity::class . '>', $registry->getRepository(SharedEntity::class, 'tenant')); // SKIP: a repository an object manager sets at runtime
	}

}

class SharedEntity
{

}

/**
 * @template T of object
 * @extends EntityRepository<T>
 */
class DefaultSharedRepository extends EntityRepository
{

}

/**
 * @template T of object
 * @extends EntityRepository<T>
 */
class TenantSharedRepository extends EntityRepository
{

}
