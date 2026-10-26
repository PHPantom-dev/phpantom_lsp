<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\EntityManager;

class TestAndxWithConsequentMethodCall
{

	/** @var EntityManager */
	private $entityManager;

	public function __construct(EntityManager $entityManager)
	{
		$this->entityManager = $entityManager;
	}

	public function emptyConstructorConsequentCall(): void
	{
		$expr = (new \Doctrine\ORM\Query\Expr\Andx())->add('1 = 1');
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->andWhere($expr);
		$queryBuilder->getQuery();
	}

	public function usedConstructor(): void
	{
		$expr = new \Doctrine\ORM\Query\Expr\Andx('1 = 1');
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->andWhere($expr);
		$queryBuilder->getQuery();
	}

	public function createdFromQueryBuilder(): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->andWhere($queryBuilder->expr()->andX('1 = 1'));
		$queryBuilder->getQuery();
	}

}
