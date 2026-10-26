<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\Common\Collections\Criteria;
use Doctrine\ORM\EntityManagerInterface;
use Doctrine\ORM\Query\Expr\Andx;
use Doctrine\ORM\Query\Expr\From;

class DynamicCalls
{
	public function testDynamicMethodCall(
		EntityManagerInterface $em,
		Andx $and,
		Criteria $criteria,
		string $string
	): void
	{
		$em->createQueryBuilder()
			->select('m')
			->from(MyEntity::class, 'm')
			->andWhere($and)
			->setParameter($string, $string)
			->setParameters([$string])
			->orWhere($string)
			->addOrderBy($string)
			->addGroupBy($string)
			->addCriteria($criteria)
			->getQuery();

		$em->createQueryBuilder()
			->select('m')
			->add('from', new From(MyEntity::class, 'm', null), true)
			->where($string)
			->orderBy($string)
			->groupBy($string)
			->getQuery();

		// all below are disallowed dynamic
		$em->createQueryBuilder()
			->select('m')
			->from($string, 'm')
			->getQuery();

		$em->createQueryBuilder()
			->select('m')
			->from(MyEntity::class, 'm')
			->indexBy($string, $string)
			->getQuery();

		$em->createQueryBuilder()
			->select('m')
			->from(MyEntity::class, 'm', $string)
			->getQuery();

		$em->createQueryBuilder()
			->select([$string])
			->from(MyEntity::class, 'm')
			->getQuery();

		$em->createQueryBuilder()
			->select(['m'])
			->from(MyEntity::class, $string)
			->getQuery();

		$em->createQueryBuilder()
			->addSelect($string)
			->from(MyEntity::class, 'm')
			->getQuery();

		$em->createQueryBuilder()
			->addSelect('m')
			->from(MyEntity::class, 'm')
			->join($string, $string)
			->getQuery();
	}

}
