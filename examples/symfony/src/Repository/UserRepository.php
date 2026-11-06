<?php

namespace App\Repository;

use App\Entity\User;
use Doctrine\Bundle\DoctrineBundle\Repository\ServiceEntityRepository;
use Doctrine\ORM\Query\Expr;
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
     * @return list<array{name: string, members: int<0, max>, label: string, nameLength: float}>
     */
    public function teamSizes(): array
    {
        // Try: hover $rows. An aggregate or a function is typed where every
        // database driver agrees: COUNT() gives an int<0, max> and UPPER() a
        // string. What AVG() gives depends on the driver: SQLite, which the
        // URL in config/packages/doctrine.yaml names, gives a float. Change
        // the URL to start with mysql:// and it is a numeric-string.
        $rows = $this->getEntityManager()
            ->createQuery('SELECT t.name, COUNT(u.email) AS members, UPPER(t.name) AS label, AVG(LENGTH(u.displayName)) AS nameLength FROM App\Entity\User u JOIN u.team t GROUP BY t.name')
            ->getResult();

        return $rows;
    }

    /**
     * @return list<User>
     */
    public function searchByName(?string $name): array
    {
        // Try: hover $users. The builder byEmail() sets up carries its DQL
        // into $builder, and through the call the `if` makes on it, so the
        // result is still a list<User>. Go-to-definition on displayName:
        // the `u` byEmail() declared on the builder stands for User here.
        // Change `u.displayName` to `t.displayName`: byEmail() declares `u`
        // but not `t`, and that is reported.
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
        // builder is then declared nowhere, and that is reported. Or wrap
        // the call in an `if ($team !== '') { ... }`: the query built
        // without it uses `t` undeclared, and that is reported too.
        $builder = $this->byEmail();
        $this->joinTeam($builder);

        return $builder->andWhere('t.name = :team')
            ->setParameter('team', $team)
            ->getQuery()
            ->getResult();
    }

    /**
     * @return list<User>
     */
    public function teamMembers(string $team): array
    {
        // Try: change `t.name` to `x.name`. The builder comes from
        // byEmailInTeam(), which gets it from byEmail() and passes it on to
        // joinTeam(), so `u` and `t` are declared along that chain and `x`
        // is reported.
        return $this->byEmailInTeam()
            ->andWhere('t.name = :team')
            ->setParameter('team', $team)
            ->getQuery()
            ->getResult();
    }

    /**
     * @return list<User>
     */
    public function sortedByName(): array
    {
        // Try: go-to-definition on displayName. The DQL an expression object
        // is built from reads like any other fragment, so a field the mapping
        // does not map, or a stray `)`, is reported there too.
        return $this->createQueryBuilder('u')
            ->add('orderBy', new Expr\OrderBy('u.displayName', 'ASC'))
            ->getQuery()
            ->getResult();
    }

    private function byEmail(): QueryBuilder
    {
        return $this->createQueryBuilder('u')->orderBy('u.email', 'ASC');
    }

    private function byEmailInTeam(): QueryBuilder
    {
        $builder = $this->byEmail();
        $this->joinTeam($builder);

        return $builder;
    }

    private function joinTeam(QueryBuilder $builder): void
    {
        $builder->join('u.team', 't');
    }
}
