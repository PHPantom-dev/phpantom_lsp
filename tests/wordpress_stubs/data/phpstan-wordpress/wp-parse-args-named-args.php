<?php

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use function PHPStan\Testing\assertType;

/**
 * Named arguments are matched up with the parameters, whatever their order.
 *
 * @param array{page?: int} $args
 */
function wpParseArgsWithNamedArguments(array $args): void
{
    assertType("array{mode: 'custom'}", wp_parse_args(defaults: ['mode' => 'default'], args: ['mode' => 'custom']));
    assertType("array{mode: 'custom'}", wp_parse_args(args: ['mode' => 'custom'], defaults: ['mode' => 'default']));
    assertType('array{page: int}', wp_parse_args(defaults: ['page' => 1], args: $args));
    assertType('array{page?: int}', wp_parse_args(args: $args));
}
