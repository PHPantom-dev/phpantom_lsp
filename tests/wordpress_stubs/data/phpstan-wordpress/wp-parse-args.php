<?php

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use function PHPStan\Testing\assertType;

/**
 * The extension resolves array_merge() with a fully qualified name, so this
 * namespaced function must not shadow the built-in one in the results below.
 */
function array_merge(): int
{
    return 1;
}

/**
 * The whole point of wp_parse_args() is that every default key is present in the
 * result, so optional keys of the given arguments become required ones.
 *
 * @param array{before?: string, after?: string, echo?: bool} $args
 */
function wpParseArgsWithShape(array $args): void
{
    assertType(
        'array{before: string, after: string, echo: bool}',
        wp_parse_args($args, ['before' => '', 'after' => '', 'echo' => true])
    );
}

/**
 * Arguments take precedence over the defaults they overwrite.
 */
function wpParseArgsWithConstantArrays(): void
{
    assertType("array{a: 1, b: 'y'}", wp_parse_args(['a' => 1], ['a' => 'x', 'b' => 'y']));
}

/**
 * Empty defaults are skipped by wp_parse_args(), which returns the arguments unchanged.
 *
 * @param array{page?: int} $args
 */
function wpParseArgsWithoutDefaults(array $args): void
{
    assertType('array{page?: int}', wp_parse_args($args));
    assertType('array{page?: int}', wp_parse_args($args, []));
}

/**
 * Defaults that are not an array are ignored, just like an empty array.
 *
 * @param array{page?: int} $args
 */
function wpParseArgsWithNonArrayDefaults(array $args, string $defaults): void
{
    assertType('array{page?: int}', wp_parse_args($args, $defaults));
}

/**
 * Inside a class the caller can see more properties than the global wp_parse_args()
 * can, so the object is not converted there and the declared return type is kept.
 */
class WpParseArgsInsideAClass
{
    /** @var int */
    public $number = 1;

    public function parse(): void
    {
        assertType('array', wp_parse_args($this, ['number' => 2]));
    }
}

/**
 * A query string cannot be described, so the declared return type is kept.
 */
function wpParseArgsWithString(string $query): void
{
    assertType('array', wp_parse_args($query, ['page' => 1]));
}
