<?php declare(strict_types = 1);

namespace Bug219;

use Doctrine\ORM\EntityManagerInterface;
use Doctrine\ORM\EntityRepository;

use function PHPStan\Testing\assertType;

class Test
{

	/** @var EntityManagerInterface */
	private $entityManager;

	public function __construct(EntityManagerInterface $entityManager)
	{
		$this->entityManager = $entityManager;
	}

	public function getEntityManager(): EntityManagerInterface
	{
		return $this->entityManager;
	}

	public function update(): void
	{
		$this->getEntityManager()->getRepository(\stdClass::class)->createQueryBuilder('t')->update();

		// PHPStan spells the type `$this(Bug219\Test)`; the class is the same.
		assertType('Bug219\Test', $this);
	}

}
