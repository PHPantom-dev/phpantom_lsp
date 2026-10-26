<?php declare(strict_types = 1);

namespace NamedManagerRepositoryInference;

use Doctrine\ORM\EntityRepository;
use Doctrine\Persistence\ManagerRegistry;
use function PHPStan\Testing\assertType;

class Example
{

	public function explicitManagerName(ManagerRegistry $registry): void
	{
		assertType(DefaultSharedRepository::class . '<' . SharedEntity::class . '>', $registry->getRepository(SharedEntity::class, 'default')); // SKIP: Doctrine getRepository() is typed as the generic repository
		assertType(TenantSharedRepository::class . '<' . SharedEntity::class . '>', $registry->getRepository(SharedEntity::class, 'tenant')); // SKIP: Doctrine getRepository() is typed as the generic repository
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
