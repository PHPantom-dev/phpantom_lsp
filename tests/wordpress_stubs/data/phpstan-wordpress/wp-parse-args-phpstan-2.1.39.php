<?php

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use function PHPStan\Testing\assertType;

/**
 * The extension resolves get_object_vars() with a fully qualified name, so this
 * namespaced function must not shadow the built-in one in the results below.
 */
function get_object_vars(): int
{
    return 1;
}

/**
 * Defaults that may or may not be empty can only guarantee the keys of the arguments.
 *
 * @param array{page?: int} $args
 */
function wpParseArgsWithMaybeEmptyDefaults(array $args, bool $condition): void
{
    $defaults = $condition ? [] : ['page' => 1];

    assertType('array{page?: int}', wp_parse_args($args, $defaults));
}

/**
 * Objects are converted to an array of their properties before the defaults are merged in.
 */
function wpParseArgsWithObject(\WP_Post $post): void
{
    assertType("non-empty-array<mixed>&hasOffset('extra')", wp_parse_args($post, ['extra' => 1])); // SKIP: `wp_parse_args()` of an object does not read the properties WordPress sees
}
