<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\EntityManager;

class TestQueryBuilderBranchesPerformances
{

	/** @var EntityManager */
	private $entityManager;

	public function __construct(EntityManager $entityManager)
	{
		$this->entityManager = $entityManager;
	}

	public function foo(bool $bool): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('p.id = 1');

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 3');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 5');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 7');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 9');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 10');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 11');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 12');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 13');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 14');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		if ($bool) {
			$queryBuilder = $queryBuilder->andWhere('p.id = 15');
		} else {
			$queryBuilder = $queryBuilder->andWhere('p.id = 16');
		}

		$queryBuilder->getQuery();
	}

}
