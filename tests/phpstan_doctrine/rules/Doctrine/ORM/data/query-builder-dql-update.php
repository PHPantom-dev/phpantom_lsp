<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\EntityManager;

class TestUpdateQueryBuilder
{

	/** @var EntityManager */
	private $entityManager;

	public function __construct(EntityManager $entityManager)
	{
		$this->entityManager = $entityManager;
	}

	public function nonDynamicSet(int $int, string $title): void
	{
		$this->entityManager->createQueryBuilder()
			->update(MyEntity::class, 'e')
			->set('e.title', $title)
			->andWhere('e.id = :int')
			->setParameter('int', $int)
			->getQuery()
			->execute();
	}

	public function dynamicSet(int $int, string $field, string $title): void
	{
		$this->entityManager->createQueryBuilder()
			->update(MyEntity::class, 'e')
			->set('e.' . $field, $title)
			->andWhere('e.id = :int')
			->setParameter('int', $int)
			->getQuery()
			->execute();
	}

}
