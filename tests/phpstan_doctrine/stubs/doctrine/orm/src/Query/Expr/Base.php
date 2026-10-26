<?php

declare(strict_types=1);

namespace Doctrine\ORM\Query\Expr;

use InvalidArgumentException;
use Stringable;

use function array_key_exists;
use function count;
use function get_debug_type;
use function implode;
use function in_array;
use function is_array;
use function is_object;
use function is_string;
use function sprintf;

/**
 * Abstract base Expr class for building DQL parts.
 *
 * @link    www.doctrine-project.org
 */
abstract class Base implements Stringable
{
    protected string $preSeparator  = '(';
    protected string $separator     = ', ';
    protected string $postSeparator = ')';

    /** @var list<class-string<Stringable>> */
    protected array $allowedClasses = [];

    /** @var list<string|Stringable> */
    protected array $parts = [];

    public function __construct(mixed $args = [])
    {}

    /**
     * @param string[]|object[]|string|object $args
     * @phpstan-param list<string|object>|string|object $args
     *
     * @return $this
     */
    public function addMultiple(array|string|object $args = []): static
    {}

    /**
     * @param string|Stringable|null $arg
     *
     * @return $this
     *
     * @throws InvalidArgumentException
     */
    public function add(mixed $arg): static
    {}

    /** @phpstan-return 0|positive-int */
    public function count(): int
    {}

    public function __toString(): string
    {}
}
