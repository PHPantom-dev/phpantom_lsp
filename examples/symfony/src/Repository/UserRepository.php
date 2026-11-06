<?php

namespace App\Repository;

use App\Entity\User;
use Doctrine\Bundle\DoctrineBundle\Repository\ServiceEntityRepository;

/**
 * @extends ServiceEntityRepository<User>
 */
final class UserRepository extends ServiceEntityRepository
{
    /**
     * @return list<User>
     */
    public function withEmail(string $email): array
    {
        return $this->findBy(['email' => $email]);
    }

    /**
     * @return list<User>
     */
    public function named(string $name): array
    {
        // Try: complete after `u.`, or go-to-definition on displayName or on
        // the entity class. The query builder's `u` stands for the
        // repository's own entity, and a field the mapping does not map is
        // reported.
        $byBuilder = $this->createQueryBuilder('u')
            ->where('u.displayName = :name')
            ->orderBy('u.email', 'ASC')
            ->setParameter('name', $name)
            ->getQuery()
            ->getResult();

        $byQuery = $this->getEntityManager()
            ->createQuery('SELECT u FROM App\Entity\User u WHERE u.displayName = :name')
            ->setParameter('name', $name)
            ->getResult();

        return [...$byBuilder, ...$byQuery];
    }
}
