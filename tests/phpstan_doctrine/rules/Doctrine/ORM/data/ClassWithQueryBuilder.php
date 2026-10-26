<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\EntityManager;
use Doctrine\ORM\QueryBuilder;

class ClassWithQueryBuilder
{

	/** @var EntityManager */
	private $entityManager;

	public function __construct(EntityManager $entityManager)
	{
		$this->entityManager = $entityManager;
	}

	public function getQueryBuilder(): QueryBuilder
	{
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e');

		return $queryBuilder;
	}

}
