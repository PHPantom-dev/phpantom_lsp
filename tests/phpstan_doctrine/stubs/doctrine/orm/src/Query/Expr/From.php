<?php

declare(strict_types=1);

namespace Doctrine\ORM\Query\Expr;

use Stringable;

/**
 * Expression class for DQL from.
 *
 * @link    www.doctrine-project.org
 */
class From implements Stringable
{
    /**
     * @param class-string $from  The class name.
     * @param string       $alias The alias of the class.
     */
    public function __construct(
        protected string $from,
        protected string $alias,
        protected string|null $indexBy = null,
    ) {}

    /** @return class-string */
    public function getFrom(): string
    {}

    public function getAlias(): string
    {}

    public function getIndexBy(): string|null
    {}

    public function __toString(): string
    {}
}
