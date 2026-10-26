<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\EntityManager;

class TestQueryBuilderRepositoryBranches
{

	/** @var EntityManager */
	private $entityManager;

	public function __construct(EntityManager $entityManager)
	{
		$this->entityManager = $entityManager;
	}

	public function f(bool $bool): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('p.id = 1');

		if ($bool) {
			doFoo();
		} else {
			doBar();
		}

		$queryBuilder->getQuery();
	}

	public function fo(bool $bool): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('p.id = 1');

		if ($bool) {
			doFoo();
		}

		$queryBuilder->getQuery();
	}

	public function foo(bool $bool): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('p.id = 1');

		if ($bool) {
			$queryBuilder->join('e.parent', 'p');
		}

		$queryBuilder->getQuery();
	}

	public function fooo(bool $bool): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('p.id = 1');

		if ($bool) {
			$queryBuilder->join('e.parent', 'p');
		} else {
			$queryBuilder->join('e.parent', 'p');
		}

		$queryBuilder->getQuery();
	}

	public function foooo(bool $bool): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->join('e.parent', 'p')
			->andWhere('p.id = 1');

		if ($bool) {
			$queryBuilder->andWhere('t.id = 1');
		}

		$queryBuilder->getQuery();
	}

	public function fooooo(bool $bool): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->join('e.parent', 'p')
			->andWhere('p.id = 1');

		if ($bool) {
			$queryBuilder->andWhere('t.id = 1');
		} else {
			$queryBuilder->andWhere('e.foo = 1');
		}

		$queryBuilder->getQuery();
	}

}
