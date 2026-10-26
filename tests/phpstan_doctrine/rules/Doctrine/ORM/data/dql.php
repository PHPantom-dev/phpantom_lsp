<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\EntityManager;
use Doctrine\ORM\EntityRepository;

/**
 * @template T
 * @extends EntityRepository<T>
 */
class TestRepository extends EntityRepository
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
		return $this->entityManager->createQuery(
			'SELECT e FROM ' . MyEntity::class . ' e'
		)->getResult();
	}

	public function parseError(): void
	{
		$this->entityManager->createQuery(
			'SELECT e FROM ' . MyEntity::class
		)->getResult();
	}

	public function unknownField(): void
	{
		$this->entityManager->createQuery(
			'SELECT e FROM ' . MyEntity::class . ' e WHERE e.transient = :test'
		)->getResult();
	}

	public function unknownEntity(): void
	{
		$this->entityManager->createQuery(
			'SELECT e FROM Foo e'
		)->getResult();
	}

	public function heredoc(): void
	{
		$this->entityManager->createQuery(<<<DQL
			SELECT e FROM Foo
DQL
		)->getResult();
	}

	public function nowdoc(): void
	{
		$this->entityManager->createQuery(<<<'DQL'
			SELECT e FROM Foo
DQL
		)->getResult();
	}

    public function findByCustomMethod(): array
    {
        return [];
    }

}
