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
}
