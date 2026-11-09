<?php

declare(strict_types=1);

namespace Doctrine\ORM\Query\Expr;

use Stringable;

use function count;
use function implode;

/**
 * Expression class for building DQL Order By parts.
 *
 * @link    www.doctrine-project.org
 */
class OrderBy implements Stringable
{
    protected string $preSeparator  = '';
    protected string $separator     = ', ';
    protected string $postSeparator = '';

    /** @var string[] */
    protected array $allowedClasses = [];

    /** @phpstan-var list<string> */
    protected array $parts = [];

    public function __construct(
        string|null $sort = null,
        string|null $order = null,
    ) {}

    public function add(string $sort, string|null $order = null): void
    {}

    /** @phpstan-return 0|positive-int */
    public function count(): int
    {}

    /** @phpstan-return list<string> */
    public function getParts(): array
    {}

    public function __toString(): string
    {}
}
