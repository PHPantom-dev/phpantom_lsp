<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\EntityManager;

class MagicRepositoryCalls
{

	/** @var EntityManager */
	private $entityManager;

	public function __construct(EntityManager $entityManager)
	{
		$this->entityManager = $entityManager;
	}

	public function doFindBy(): void
	{
		$entityRepository = $this->entityManager->getRepository(MyEntity::class);
		$entityRepository->findBy(['id' => 1]);
		$entityRepository->findById(1);
		$entityRepository->findByTitle('test');
		$entityRepository->findByTransient('test');
		$entityRepository->findByNonexistent('test');
		$entityRepository->findByCustomMethod();
	}

	public function doFindOneBy(): void
	{
		$entityRepository = $this->entityManager->getRepository(MyEntity::class);
		$entityRepository->findOneBy(['id' => 1]);
		$entityRepository->findOneById(1);
		$entityRepository->findOneByTitle('test');
		$entityRepository->findOneByTransient('test');
		$entityRepository->findOneByNonexistent('test');
	}

	public function doCountBy(): void
	{
		$entityRepository = $this->entityManager->getRepository(MyEntity::class);
		$entityRepository->countBy(['id' => 1]);
		$entityRepository->countById(1);
		$entityRepository->countByTitle('test');
		$entityRepository->countByTransient('test');
		$entityRepository->countByNonexistent('test');
	}

	public function doFindByWithRepository(): void
	{
		$entityRepository = $this->entityManager->getRepository(MySecondEntity::class);
		$entityRepository->findBy(['id' => 1]);
		$entityRepository->findById(1);
		$entityRepository->findByTitle('test');
		$entityRepository->findByTransient('test');
		$entityRepository->findByNonexistent('test');
		$entityRepository->findByCustomMethod();
	}

	public function doFindByWithOptionals(): void
	{
		$entityRepository = $this->entityManager->getRepository(MyEntity::class);
		$entityRepository->findByTitle('test', ['id' => 'DESC']);
		$entityRepository->findByTitle('test', null);
		$entityRepository->findByTitle('test', [1 => 'DESC']);
		$entityRepository->findByTitle('test', ['id' => 'DESC'], 1);
		$entityRepository->findByTitle('test', ['id' => 'DESC'], null);
		$entityRepository->findByTitle('test', ['id' => 'DESC'], '1');
		$entityRepository->findByTitle('test', ['id' => 'DESC'], 1, 1);
		$entityRepository->findByTitle('test', ['id' => 'DESC'], 1, null);
		$entityRepository->findByTitle('test', ['id' => 'DESC'], 1, '1');
	}

	public function doFindOneByWithOptionals(): void
	{
		$entityRepository = $this->entityManager->getRepository(MyEntity::class);
		$entityRepository->findOneByTitle('test', ['id' => 'DESC']);
		$entityRepository->findOneByTitle('test', null);
		$entityRepository->findOneByTitle('test', [1 => 'DESC']);
	}

	public function doCountByWithOptionals(): void
	{
		$entityRepository = $this->entityManager->getRepository(MyEntity::class);
		$entityRepository->countByTitle('test', ['id' => 'DESC']);
	}

	/**
	 * @param class-string<MySecondEntity|MyThirdEntity> $entityClass
	 */
	public function doFindByWithRepositoryOfMultiplesClasses(string $entityClass): void
	{
		$entityRepository = $this->entityManager->getRepository($entityClass);
		$entityRepository->findBy(['id' => 1]);
		$entityRepository->findById(1);
		$entityRepository->findByTitle('test');
		$entityRepository->findByTransient('test');
		$entityRepository->findByNonexistent('test');
		$entityRepository->findByCustomMethod();
	}
}
