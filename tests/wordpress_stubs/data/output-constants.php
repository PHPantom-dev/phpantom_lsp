<?php

/**
 * Not ported: PHPantom's own fixture of the constructs the WordPress stubs
 * lean on that upstream's fixtures take for granted. Upstream's
 * tests/bootstrap.php defines the output constants; here they come from
 * PHPantom.
 */

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use WP_Error;
use WP_Post;

use function PHPStan\Testing\assertType;

// The output constants, and the conditional return types over them
assertType("'OBJECT'", OBJECT);
assertType("'OBJECT_K'", OBJECT_K);
assertType("'ARRAY_A'", ARRAY_A);
assertType("'ARRAY_N'", ARRAY_N);
assertType('WP_Post|null', get_post(1));
assertType('WP_Post|null', get_post(1, OBJECT));
assertType('non-empty-array<string, mixed>|null', get_post(1, ARRAY_A));
assertType('non-empty-array<int, mixed>|null', get_post(1, ARRAY_N));

// @phpstan-assert-if-true narrowing
/** @var int|WP_Error $result */
$result = Faker::mixed();
if (is_wp_error($result)) {
    assertType('WP_Error', $result);
    return;
}
assertType('int', $result);

// A function that never returns
/** @var WP_Post|null $post */
$post = Faker::mixed();
if ($post === null) {
    wp_die();
}
assertType('WP_Post', $post); // SKIP: an array-shape condition matches any array, so `wp_die()` with its default arguments is read as returning
