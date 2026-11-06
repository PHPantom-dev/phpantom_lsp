<?php

namespace App\Repository;

use App\Entity\User;
use Doctrine\Bundle\DoctrineBundle\Repository\ServiceEntityRepository;
use Doctrine\ORM\QueryBuilder;

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
        // reported. Delete the `u` after the entity class in the DQL string,
        // or add a `)` to the `where()`, and the syntax error is reported.
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

    /**
     * @return list<User>
     */
    public function inTeam(string $team): array
    {
        // Try: complete after `t.`, or go-to-definition on name. The `t` the
        // join declares stands for the Team entity the `team` association
        // targets. Change `t.name` to `x.name`, and the variable nothing
        // declares is reported.
        return $this->createQueryBuilder('u')
            ->join('u.team', 't')
            ->where('t.name = :team')
            ->setParameter('team', $team)
            ->getQuery()
            ->getResult();
    }

    /**
     * @return list<User>
     */
    public function search(?string $team): array
    {
        // Try: move the `andWhere()` below the `if`. The builder only joins
        // `t` when a team is given, so the query built without one uses a
        // variable it never declares, and that is reported.
        $builder = $this->createQueryBuilder('u');
        if ($team !== null) {
            $builder->join('u.team', 't')
                ->andWhere('t.name = :team')
                ->setParameter('team', $team);
        }

        return $builder->getQuery()->getResult();
    }

    /**
     * @return array<string, string>
     */
    public function displayNamesByEmail(): array
    {
        // Try: hover $rows. The query is typed by what its DQL selects, a
        // shape of the two fields, keyed by the email it is indexed by:
        // array<string, array{email: string, displayName: string}>.
        $rows = $this->createQueryBuilder('u', 'u.email')
            ->select('u.email', 'u.displayName')
            ->getQuery()
            ->getResult();

        return array_map(static fn (array $row): string => $row['displayName'], $rows);
    }

    /**
     * @return list<User>
     */
    public function searchByName(?string $name): array
    {
        // Try: hover $users. The builder byEmail() sets up carries its DQL
        // into $builder, and through the call the `if` makes on it, so the
        // result is still a list<User>. Change `u.displayName` to
        // `t.displayName`: byEmail() declares `u` but not `t`, and that is
        // reported.
        $builder = $this->byEmail();
        if ($name !== null) {
            $builder->andWhere('u.displayName = :name')->setParameter('name', $name);
        }
        $users = $builder->getQuery()->getResult();

        return $users;
    }

    /**
     * @return list<User>
     */
    public function inTeamByEmail(string $team): array
    {
        // Try: delete the `joinTeam()` call. The `t` it joins onto the
        // builder is then declared nowhere, and that is reported.
        $builder = $this->byEmail();
        $this->joinTeam($builder);

        return $builder->andWhere('t.name = :team')
            ->setParameter('team', $team)
            ->getQuery()
            ->getResult();
    }

    private function byEmail(): QueryBuilder
    {
        return $this->createQueryBuilder('u')->orderBy('u.email', 'ASC');
    }

    private function joinTeam(QueryBuilder $builder): void
    {
        $builder->join('u.team', 't');
    }
}
