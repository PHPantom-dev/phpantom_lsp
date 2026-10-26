<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\EntityManager;

class TestQueryBuilderRepository
{

	/** @var EntityManager */
	private $entityManager;

	public function __construct(EntityManager $entityManager)
	{
		$this->entityManager = $entityManager;
	}

	/**
	 * @return MyEntity[]
	 */
	public function getEntities(): array
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->getQuery();
	}

	public function parseError(): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('e.id = 1)')
			->getQuery();
	}

	public function parseErrorNonFluent(int $id): void
	{
		$qb = $this->entityManager->createQueryBuilder();
		$qb = $qb->select('e');
		$qb = $qb->from(MyEntity::class, 'e');
		$qb->andWhere('e.id = :id)')
			->setParameter('id', $id)
			->getQuery();
	}

	public function parseErrorStateful(int $id): void
	{
		$qb = $this->entityManager->createQueryBuilder();
		$qb->select('e');
		$qb->from(MyEntity::class, 'e');
		$qb->andWhere('e.id = :id)');
		$qb->setParameters(['id' => $id]);
		$qb->getQuery();
	}



	public function unknownField(): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->where('e.transient = :test')
			->getQuery();
	}

	public function unknownEntity(): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from('Foo', 'e')
			->getQuery();
	}

	public function selectArray(): void
	{
		$this->entityManager->createQueryBuilder()
			->select([
				'e.id',
				'e.title',
			])->from(MyEntity::class, 'e')
			->getQuery();
	}

	public function analyseQueryBuilderOtherMethodBeginning(): void
	{
		$this->createQb()->getQuery();
	}

	private function createQb(): \Doctrine\ORM\QueryBuilder
	{
		return $this->entityManager->createQueryBuilder()->select('e')->from(MyEntity::class, 'e');
	}

	public function analyseQueryBuilderDynamicArgs(string $entity): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from($entity, 'e')
			->getQuery();
	}

	public function limitOffset(int $offset, int $limit): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('e.transient = 1')
			->setFirstResult($offset)
			->setMaxResults($limit)
			->getQuery();
	}

	public function limitOffsetCorrect(int $offset, int $limit): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('e.id = 1')
			->setFirstResult($offset)
			->setMaxResults($limit)
			->getQuery();
	}

	public function addNewExprSyntaxError(): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('e.id = 1')
			->add('orderBy', new \Doctrine\ORM\Query\Expr\OrderBy('e.name)', 'ASC'))
			->getQuery();
	}

	public function addNewExprSemanticError(): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('e.id = 1')
			->add('orderBy', new \Doctrine\ORM\Query\Expr\OrderBy('e.name', 'ASC'))
			->getQuery();
	}

	public function addNewExprCorrect(): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('e.id = 1')
			->add('orderBy', new \Doctrine\ORM\Query\Expr\OrderBy('e.title', 'ASC'))
			->getQuery();
	}

	public function addNewExprFirstAssignedToVariable(): void
	{
		$orderBy = new \Doctrine\ORM\Query\Expr\OrderBy('e.name', 'ASC');
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->andWhere('e.id = 1')
			->add('orderBy', $orderBy)
			->getQuery();
	}

	public function addNewExprBase(): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->add('where', new \Doctrine\ORM\Query\Expr\Andx([
				'e.transient = 1',
				'e.name = \'foo\'',
			]))
			->getQuery();
	}

	public function addNewExprBaseCorrect(): void
	{
		$this->entityManager->createQueryBuilder()
			->select('e')
			->from(MyEntity::class, 'e')
			->add('where', new \Doctrine\ORM\Query\Expr\Andx([
				'e.id = 1',
			]))
			->getQuery();
	}

	public function qbExpr(): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->add('where', $queryBuilder->expr()->orX(
				$queryBuilder->expr()->eq('e.id', '1'),
				$queryBuilder->expr()->like('e.nickname', '\'nick\'')
			))
			->getQuery();
	}

	public function qbExprIsNull(): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->add('where', $queryBuilder->expr()->orX(
				$queryBuilder->expr()->eq('e.id', '1'),
				$queryBuilder->expr()->isNull('e.nickname')
			))
			->getQuery();
	}

	public function qbExprIsNullSyntaxError(): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->add('where', $queryBuilder->expr()->orX(
				$queryBuilder->expr()->eq('e.id', '1'),
				$queryBuilder->expr()->isNull('e.nickname)')
			))
			->getQuery();
	}

	public function weirdTypeSpecifyingExtensionProblem(): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->where('e.transient = :test');

		$queryBuilder->getQuery();
	}

	public function weirdTypeSpecifyingExtensionProblemCorrect(): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->where('e.title = :test');

		$queryBuilder->getQuery();
	}

	public function queryBuilderFromSomewhereElse(): void
	{
		$class = new ClassWithQueryBuilder($this->entityManager);
		$queryBuilder = $class->getQueryBuilder()->andWhere('e.nonexistent = :test');
		$queryBuilder->getQuery();
	}

	public function anotherWeirdTypeSpecifyingExtensionProblem(): void
	{
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->setParameter('test', '123');

		$queryBuilder->getQuery();
	}

	public function qbCustomExprMethod(): void
	{
		$expr = new CustomExpr();
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->andWhere($expr->correct());
		$queryBuilder->getQuery();
	}

	public function qbCustomExprMethodSyntaxError(): void
	{
		$expr = new CustomExpr();
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->andWhere($expr->syntaxError());
		$queryBuilder->getQuery();
	}

	public function qbExprMethod(): void
	{
		$expr = (new \Doctrine\ORM\Query\Expr\Andx())->add('1 = 1');
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->andWhere($expr);
		$queryBuilder->getQuery();
	}

	public function bug602(array $objectConditions, bool $rand): void
	{
		$orParts = ['e.title LIKE :termLike'];
		if ($rand) {
			$orParts[] = 'p.version = :term';
		}
		$queryBuilder = $this->entityManager->createQueryBuilder();
		$queryBuilder->select('e')
			->from(MyEntity::class, 'e')
			->andWhere($queryBuilder->expr()->orX(...$orParts))
				->setParameter('termLike', 'someTerm');
	}

}

class CustomExpr extends \Doctrine\ORM\Query\Expr
{

	public function __construct()
	{
		// necessary so that NewExprDynamicReturnTypeExtension works
	}

	public function syntaxError(): string
	{
		return 'foo';
	}

	public function correct(): string
	{
		return 'e.id = 1';
	}

	public function newParent(): void
	{
		$test = new parent();
	}

}

class CreateBaseQuery
{

	/** @var EntityManager */
	private $entityManager;

	public function __construct(EntityManager $entityManager)
	{
		$this->entityManager = $entityManager;
	}

	public function findPendingPackingNotesToResolve(): array
	{
		return $this->createBaseQuery()
			->select('mye')
			->getQuery()
			->getResult();
	}

	private function createBaseQuery(): \Doctrine\ORM\QueryBuilder
	{
		return $this->entityManager->createQueryBuilder()
			->from(MyEntity::class, 'mye');
	}

}
