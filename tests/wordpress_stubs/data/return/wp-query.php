<?php

/**
 * Note:
 * Starting from PHPStan 1.10.49, void types, including void in unions, are
 * transformed into null.
 *
 * @link https://github.com/phpstan/phpstan-src/pull/2778
 */

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;

assertType('bool', Faker::wpQuery()->query_vars_changed);
assertType('bool|string', Faker::wpQuery()->query_vars_hash);
assertType('null', Faker::wpQuery()->init_query_flags());

// WP_Query::query(): default fields value.
assertType('array<int, WP_Post>', Faker::wpQuery()->query([]));
assertType('array<int, WP_Post>', Faker::wpQuery()->query(['fields' => '']));
assertType('array<int, WP_Post>', Faker::wpQuery()->query(['fields' => 'all']));

// WP_Query::query(): requesting IDs.
assertType('array<int, int>', Faker::wpQuery()->query(['fields' => 'ids']));
assertType('array<int, int>', Faker::wpQuery()->query(['post_type' => 'page', 'fields' => 'ids']));

// WP_Query::query(): requesting parent IDs keyed by post ID.
assertType('array<int, int>', Faker::wpQuery()->query(['fields' => 'id=>parent']));

// WP_Query::query(): unrecognized fields values fall back to post objects.
assertType('array<int, WP_Post>', Faker::wpQuery()->query(['fields' => 'foo']));

// WP_Query::query(): unknown fields value.
assertType('array<int, int>|array<int, WP_Post>', Faker::wpQuery()->query(['fields' => Faker::string()])); // PHPantom keeps the array types apart where PHPStan merges them

// WP_Query::query(): a URL query string is parsed at runtime, so the fields
// value is not visible to PHPStan and every result shape stays possible.
assertType('array<int, int>|array<int, WP_Post>', Faker::wpQuery()->query('fields=ids')); // PHPantom keeps the array types apart where PHPStan merges them
assertType('array<int, int>|array<int, WP_Post>', Faker::wpQuery()->query(Faker::string())); // PHPantom keeps the array types apart where PHPStan merges them

// WP_Query::get_posts() reads the query vars off the instance instead of taking
// them as an argument, so there is nothing to narrow the return type on.
assertType('array<WP_Post>|array<int>', Faker::wpQuery()->get_posts()); // PHPantom keeps the array types apart where PHPStan merges them
