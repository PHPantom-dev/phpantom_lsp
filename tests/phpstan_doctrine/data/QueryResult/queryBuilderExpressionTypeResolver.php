<?php declare(strict_types = 1);  // lint >= 8.1

namespace QueryResult\CreateQuery;

use Doctrine\Common\Collections\Criteria;
use Doctrine\ORM\AbstractQuery;
use Doctrine\ORM\EntityManagerInterface;
use Doctrine\ORM\EntityRepository;
use Doctrine\ORM\Query\Expr\Andx;
use Doctrine\ORM\Query\Expr\From;
use Doctrine\ORM\QueryBuilder;
use QueryResult\Entities\Many;
use function PHPStan\Testing\assertType;

class QueryBuilderExpressionTypeResolverTest
{

	/**
	 * @var MyRepository
	 */
	private $myRepository;

	public function testQueryTypeIsInferredOnAcrossMethods(EntityManagerInterface $em): void
	{
		$query = $this->getQueryBuilder($em)->getQuery();
		$branchingQuery = $this->getBranchingQueryBuilder($em)->getQuery();

		assertType('Doctrine\ORM\Query<null, QueryResult\Entities\Many>', $query); // SKIP: Doctrine DQL and query builder result types are not read
		assertType('Doctrine\ORM\Query<null, QueryResult\Entities\Many>#1|Doctrine\ORM\Query<null, QueryResult\Entities\Many>#2', $branchingQuery); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testQueryTypeIsInferredOnAcrossMethodsEvenWhenVariableAssignmentIsUsed(EntityManagerInterface $em): void
	{
		$queryBuilder = $this->getQueryBuilder($em);

		assertType('Doctrine\ORM\Query<null, QueryResult\Entities\Many>', $queryBuilder->getQuery()); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testQueryBuilderPassedElsewhereNotTracked(EntityManagerInterface $em): void
	{
		$queryBuilder = $this->getQueryBuilder($em);
		$queryBuilder->indexBy('m', 'm.stringColumn');

		$this->adjustQueryBuilderToIndexByInt($queryBuilder);

		assertType('Doctrine\ORM\Query<string, QueryResult\Entities\Many>', $queryBuilder->getQuery()); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testDiveIntoCustomEntityRepository(EntityManagerInterface $em): void
	{
		$queryBuilder = $this->myRepository->getCustomQueryBuilder($em);

		assertType('Doctrine\ORM\Query<null, QueryResult\Entities\Many>', $queryBuilder->getQuery()); // SKIP: Doctrine DQL and query builder result types are not read
	}


	public function testStaticCallWorksToo(EntityManagerInterface $em): void
	{
		$queryBuilder = self::getStaticQueryBuilder($em);

		assertType('Doctrine\ORM\Query<null, QueryResult\Entities\Many>', $queryBuilder->getQuery()); // SKIP: Doctrine DQL and query builder result types are not read
	}

	public function testFirstClassCallableDoesNotFail(EntityManagerInterface $em): void
	{
		$this->getQueryBuilder(...);
	}

	public function testNullableQueryBuilderIsNotInferred(EntityManagerInterface $em): void
	{
		assertType('Doctrine\\ORM\\QueryBuilder|null', $this->getNullableQueryBuilder($em));
	}

	private function adjustQueryBuilderToIndexByInt(QueryBuilder $qb): void
	{
		$qb->indexBy('m', 'm.intColumn');
	}

	private function getQueryBuilderRecursively(EntityManagerInterface $em): QueryBuilder
	{
		return $this->getQueryBuilderRecursively($em);
	}

	private function getQueryBuilderRecursivelyInLoop(EntityManagerInterface $em): QueryBuilder
	{
		return $this->getQueryBuilderRecursivelyInLoop2($em);
	}

	private function getQueryBuilderRecursivelyInLoop2(EntityManagerInterface $em): QueryBuilder
	{
		return $this->getQueryBuilderRecursivelyInLoop($em);
	}

	private function getQueryBuilder(EntityManagerInterface $em): QueryBuilder
	{
		return $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm');
	}

	private static function getStaticQueryBuilder(EntityManagerInterface $em): QueryBuilder
	{
		return $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm');
	}

	private function getNullableQueryBuilder(EntityManagerInterface $em): ?QueryBuilder
	{
		return $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm');
	}

	private function getBranchingQueryBuilder(EntityManagerInterface $em): QueryBuilder
	{
		$queryBuilder = $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm');

		if (random_int(0, 1) === 1) {
			$queryBuilder->andWhere('m.intColumn = 1');
		}

		return $queryBuilder;
	}
}

class MyRepository extends EntityRepository {

	private function getCustomQueryBuilder(EntityManagerInterface $em): QueryBuilder
	{
		return $em->createQueryBuilder()
			->select('m')
			->from(Many::class, 'm');
	}
}
