<?php declare(strict_types = 1);

namespace QueryResult\MultipleEntityManagers;

use Doctrine\ORM\EntityManagerInterface;
use Doctrine\ORM\EntityRepository;
use QueryResult\MultipleEntityManagers\Main\User;
use QueryResult\MultipleEntityManagers\Tenant\App;
use function PHPStan\Testing\assertType;

class MultipleEntityManagers
{

	/**
	 * @param EntityRepository<User> $repository
	 */
	public function userRepository(EntityRepository $repository): void
	{
		$query = $repository->createQueryBuilder('u')->getQuery();

		assertType('Doctrine\ORM\Query<null, QueryResult\MultipleEntityManagers\Main\User>', $query);
		assertType('list<QueryResult\MultipleEntityManagers\Main\User>', $query->getResult());
	}

	/**
	 * @param EntityRepository<App> $repository
	 */
	public function tenantRepository(EntityRepository $repository): void
	{
		$query = $repository->createQueryBuilder('a')->getQuery();

		assertType('Doctrine\ORM\Query<null, QueryResult\MultipleEntityManagers\Tenant\App>', $query);
		assertType('list<QueryResult\MultipleEntityManagers\Tenant\App>', $query->getResult());
	}

	public function directTenantQuery(EntityManagerInterface $entityManager): void
	{
		$query = $entityManager->createQuery('SELECT a FROM QueryResult\MultipleEntityManagers\Tenant\App a');

		assertType('Doctrine\ORM\Query<null, QueryResult\MultipleEntityManagers\Tenant\App>', $query);
		assertType('list<QueryResult\MultipleEntityManagers\Tenant\App>', $query->getResult());
	}

	public function directDefaultQuery(EntityManagerInterface $entityManager): void
	{
		$query = $entityManager->createQuery('SELECT u FROM QueryResult\MultipleEntityManagers\Main\User u');

		assertType('Doctrine\ORM\Query<null, QueryResult\MultipleEntityManagers\Main\User>', $query);
		assertType('list<QueryResult\MultipleEntityManagers\Main\User>', $query->getResult());
	}

	public function directTenantDelete(EntityManagerInterface $entityManager): void
	{
		$query = $entityManager->createQuery('DELETE QueryResult\MultipleEntityManagers\Tenant\App a');

		assertType('Doctrine\ORM\Query<void, void>', $query);
	}

	public function directTenantDeleteWithFrom(EntityManagerInterface $entityManager): void
	{
		$query = $entityManager->createQuery('DELETE FROM QueryResult\MultipleEntityManagers\Tenant\App a');

		assertType('Doctrine\ORM\Query<void, void>', $query);
	}

	public function directTenantUpdate(EntityManagerInterface $entityManager): void
	{
		$query = $entityManager->createQuery('UPDATE QueryResult\MultipleEntityManagers\Tenant\App a SET a.id = :id');

		assertType('Doctrine\ORM\Query<void, void>', $query);
	}

}
