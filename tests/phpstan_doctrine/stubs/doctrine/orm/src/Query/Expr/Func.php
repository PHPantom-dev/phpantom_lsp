<?php

declare(strict_types=1);

namespace Doctrine\ORM\Query\Expr;

use Stringable;

use function implode;

/**
 * Expression class for generating DQL functions.
 *
 * @link    www.doctrine-project.org
 */
class Func implements Stringable
{
    /** @var mixed[] */
    protected array $arguments;

    /**
     * Creates a function, with the given argument.
     *
     * @phpstan-param list<mixed>|mixed $arguments
     */
    public function __construct(
        protected string $name,
        mixed $arguments,
    ) {}

    public function getName(): string
    {}

    /** @phpstan-return list<mixed> */
    public function getArguments(): array
    {}

    public function __toString(): string
    {}
}
