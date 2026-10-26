<?php

declare(strict_types=1);

namespace Doctrine\ORM\Internal;

use BadMethodCallException;

use function array_filter;
use function array_is_list;
use function array_keys;
use function array_values;
use function assert;
use function debug_backtrace;
use function implode;
use function is_string;
use function sprintf;

use const DEBUG_BACKTRACE_IGNORE_ARGS;

/**
 * Checks if a variadic parameter contains unexpected named arguments.
 *
 * @internal
 */
trait NoUnknownNamedArguments
{
    /**
     * @param TItem[] $parameter
     *
     * @template TItem
     * @phpstan-assert list<TItem> $parameter
     */
    private static function validateVariadicParameter(array $parameter): void
    {}
}
