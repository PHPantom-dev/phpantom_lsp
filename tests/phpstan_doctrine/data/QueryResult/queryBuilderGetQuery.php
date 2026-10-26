<?php declare(strict_types = 1);

namespace QueryResult\CreateQuery;

use Doctrine\Common\Collections\Criteria;
use Doctrine\ORM\AbstractQuery;
use Doctrine\ORM\EntityManagerInterface;
use Doctrine\ORM\Query\Expr\Andx;
use Doctrine\ORM\Query\Expr\From;
use Doctrine\ORM\QueryBuilder;
use QueryResult\Entities\Many;
use Type\Doctrine\data\QueryResult\Entities\Truck;
use Type\Doctrine\data\QueryResult\Entities\VehicleInterface;
use function PHPStan\Testing\assertType;

class QueryBuilderGetQuery
{
	private function getQueryBuilderMany(EntityManagerInterface $em): QueryBuilder
	{
		return $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm');
	}

	public function addAndWhereAndGetQuery(EntityManagerInterface $em): void
	{
		$qb = $this->getQueryBuilderMany($em)->andWhere('m.intColumn = 1');
		assertType('list<QueryResult\Entities\Many>', $qb->getQuery()->getResult()); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function getQueryDirectly(EntityManagerInterface $em): void
	{
		assertType('list<QueryResult\Entities\Many>', $this->getQueryBuilderMany($em)->getQuery()->getResult()); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testQueryTypeParametersAreInfered(EntityManagerInterface $em): void
	{
		$query = $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm')
			->getQuery();

		assertType('Doctrine\ORM\Query<null, QueryResult\Entities\Many>', $query); // SKIP: Doctrine DQL and query builder result types are not read

		$query = $em->createQueryBuilder()
			->select(['m.intColumn', 'm.stringNullColumn'])
			->from(Many::class, 'm')
			->getQuery();

		assertType('Doctrine\ORM\Query<null, array{intColumn: int, stringNullColumn: string|null}>', $query); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testIndexByInfering(EntityManagerInterface $em): void
	{
		$query = $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm', 'm.intColumn')
			->getQuery();

		assertType('Doctrine\ORM\Query<int, QueryResult\Entities\Many>', $query); // SKIP: Doctrine DQL and query builder result types are not read

		$query = $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm', 'm.stringColumn')
			->getQuery();

		assertType('Doctrine\ORM\Query<string, QueryResult\Entities\Many>', $query); // SKIP: Doctrine DQL and query builder result types are not read

		$query = $em->createQueryBuilder()
			->select(['m.intColumn', 'm.stringNullColumn'])
			->from(Many::class, 'm')
			->indexBy('m', 'm.stringColumn')
			->getQuery();

		assertType('Doctrine\ORM\Query<string, array{intColumn: int, stringNullColumn: string|null}>', $query); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testIndexByResultInfering(EntityManagerInterface $em): void
	{
		$result = $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm', 'm.intColumn')
			->getQuery()
			->getResult();

		assertType('array<int, QueryResult\Entities\Many>', $result); // SKIP: Doctrine DQL and query builder result types are not read

		$result = $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm', 'm.stringColumn')
			->getQuery()
			->getResult();

		assertType('array<string, QueryResult\Entities\Many>', $result); // SKIP: Doctrine DQL and query builder result types are not read

		$result = $em->createQueryBuilder()
			->select(['m.intColumn', 'm.stringNullColumn'])
			->from(Many::class, 'm')
			->indexBy('m', 'm.stringColumn')
			->getQuery()
			->getResult();

		assertType('array<string, array{intColumn: int, stringNullColumn: string|null}>', $result); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testConditionalAddSelect(EntityManagerInterface $em, bool $bool): void
	{
		$qb = $em->createQueryBuilder();
		if ($bool) {
			$qb->select('m.intColumn');
		} else {
			$qb->select('m.intColumn', 'm.stringNullColumn');
		}
		$query = $qb->from(Many::class, 'm')->getQuery();

		assertType('Doctrine\ORM\Query<null, array{intColumn: int}>|Doctrine\ORM\Query<null, array{intColumn: int, stringNullColumn: string|null}>', $query); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testQueryResultTypeIsMixedWhenDQLIsNotKnown(QueryBuilder $builder): void
	{
		$query = $builder->getQuery();

		assertType('Doctrine\ORM\Query<null, mixed>', $query); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testQueryResultTypeIsMixedWhenDQLIsInvalid(EntityManagerInterface $em): void
	{
		$query = $em->createQueryBuilder()
			->select('invalid')
			->from(Many::class, 'm')
			->getQuery();

		assertType('Doctrine\ORM\Query<mixed, mixed>', $query); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testQueryResultTypeIsMixedWhenDQLIsUsingAnInterfaceTypeDefinition(EntityManagerInterface $em): void
	{
		$vehicle = $this->createVehicule();

		assertType(VehicleInterface::class, $vehicle);

		$query = $em->createQueryBuilder()
			->select('v')
			->from(get_class($vehicle), 'v')
			->getQuery();

		assertType('Doctrine\ORM\Query<null, mixed>', $query); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testQueryResultTypeIsVoidWithDeleteOrUpdate(EntityManagerInterface $em): void
	{
		$query = $em->getRepository(Many::class)
				 ->createQueryBuilder('m')
				 ->where('m.id IN (:ids)')
				 ->setParameter('ids', $ids)
				 ->delete()
				 ->getQuery();

		assertType('Doctrine\ORM\Query<void, void>', $query); // SKIP: Doctrine DQL and query builder result types are not read

		$query = $em->getRepository(Many::class)
				 ->createQueryBuilder('m')
				 ->where('m.id IN (:ids)')
				 ->setParameter('ids', $ids)
				 ->update()
				 ->set('m.intColumn', '42')
				 ->getQuery();

		assertType('Doctrine\ORM\Query<void, void>', $query); // SKIP: Doctrine DQL and query builder result types are not read
	}


	public function testDynamicMethodCall(
		EntityManagerInterface $em,
		Andx $and,
		Criteria $criteria,
		string $string
	): void
	{
		$result = $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm')
			->andWhere($and)
			->setParameter($string, $string)
			->setParameters([$string])
			->orWhere($string)
			->addOrderBy($string)
			->addGroupBy($string)
			->addCriteria($criteria)
			->getQuery()
			->getResult();

		assertType('list<QueryResult\Entities\Many>', $result); // SKIP: Doctrine DQL and query builder result types are not read

		$result = $em->createQueryBuilder()
			->select(['m.stringNullColumn'])
			->add('from', new From(Many::class, 'm', null), true)
			->where($string)
			->orderBy($string)
			->groupBy($string)
			->getQuery()
			->getResult();

		assertType('list<array{stringNullColumn: string|null}>', $result); // SKIP: Doctrine DQL and query builder result types are not read

		$result = $em->createQueryBuilder()
			->select(['m.intColumn', 'm.stringNullColumn'])
			->from($string, 'm')
			->getQuery()
			->getResult();

		assertType('mixed', $result);

		$result = $em->createQueryBuilder()
			->select(['m.intColumn', 'm.stringNullColumn'])
			->from(Many::class, 'm')
			->indexBy($string, $string)
			->getQuery()
			->getResult();

		assertType('mixed', $result);

		$result = $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm', $string)
			->getQuery()
			->getResult();

		assertType('mixed', $result);

		$result = $em->createQueryBuilder()
			->select([$string, 'm.stringNullColumn'])
			->from(Many::class, 'm')
			->getQuery()
			->getResult();

		assertType('mixed', $result);

		$result = $em->createQueryBuilder()
			->select(['m.stringNullColumn'])
			->from(Many::class, $string)
			->getQuery()
			->getResult();

		assertType('mixed', $result);

		$result = $em->createQueryBuilder()
			->addSelect($string)
			->from(Many::class, 'm')
			->getQuery()
			->getResult();

		assertType('mixed', $result);

		$result = $em->createQueryBuilder()
			->addSelect('m')
			->from(Many::class, 'm')
			->join($string, $string)
			->getQuery()
			->getResult();

		assertType('mixed', $result);
	}

	private function createVehicule(): VehicleInterface
	{
		return new Truck();
	}

	/**
	 * @param class-string<Many> $many
	 */
	public function testRegularClassString(EntityManagerInterface $em, string $many)
	{
		$result = $em->createQueryBuilder()
			->select("m")
			->from($many, 'm')
			->getQuery()
			->getResult();

		assertType('list<QueryResult\Entities\Many>', $result); // SKIP: Doctrine DQL and query builder result types are not read
	}
	/**
	 * @param class-string<T> $many
	 * @template T of Many
	 */
	public function testTemplatedClassString(EntityManagerInterface $em, string $many)
	{
		$result = $em->createQueryBuilder()
			->select("m")
			->from($many, 'm')
			->getQuery()
			->getResult();

		assertType('list<QueryResult\Entities\Many>', $result); // SKIP: Doctrine DQL and query builder result types are not read
	}


	/**
	 * @param class-string<self> $classString
	 */
	public function testNonEntityClassString(EntityManagerInterface $em, string $classString)
	{
		$result = $em->createQueryBuilder()
			->select("m")
			->from($classString, 'm')
			->getQuery()
			->getResult();

		assertType('mixed', $result);
	}

	public function testEventAlias(EntityManagerInterface $em): void
	{
		$query = $em->createQueryBuilder()
			->select('event')
			->from(Many::class, 'event')
			->getQuery();

		assertType('Doctrine\ORM\Query<null, QueryResult\Entities\Many>', $query); // SKIP: Doctrine DQL and query builder result types are not read
	}

}
