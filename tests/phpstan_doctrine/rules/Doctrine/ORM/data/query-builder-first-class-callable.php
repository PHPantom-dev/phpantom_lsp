<?php declare(strict_types = 1); // lint >= 8.1

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\EntityManager;

class TestFirstClassCallableExpr
{

	/** @var EntityManager */
	private $entityManager;

	public function __construct(EntityManager $entityManager)
	{
		$this->entityManager = $entityManager;
	}

	public function matchArmWithFirstClassCallable(): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$expr = $queryBuilder->expr();

		$comparator = match (random_int(0, 1)) {
			0 => $expr->in(...),
			1 => $expr->notIn(...),
		};

		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->andWhere($comparator('e.id', [1, 2, 3]));
		$queryBuilder->getQuery();
	}

	public function variableWithFirstClassCallable(): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$expr = $queryBuilder->expr();

		$fn = $expr->eq(...);

		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->andWhere($fn('e.id', '1'));
		$queryBuilder->getQuery();
	}

}
