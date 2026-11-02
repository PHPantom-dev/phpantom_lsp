<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use WP_Term_Query;

use function PHPStan\Testing\assertType;

$termQuery = new WP_Term_Query();

// Default argument values (fields => all)
assertType('array<int, WP_Term>', $termQuery->query([]));

// Requesting a count. WP_Term_Query::get_terms() returns int 0 rather than a
// numeric string when the queried parent term has no descendants.
assertType('0|numeric-string', $termQuery->query(['fields' => 'count']));
assertType('0|numeric-string', $termQuery->query(['taxonomy' => 'category', 'fields' => 'count']));

// Requesting names or slugs
assertType('list<string>', $termQuery->query(['fields' => 'names']));
assertType('list<string>', $termQuery->query(['fields' => 'slugs']));
assertType('array<int, string>', $termQuery->query(['fields' => 'id=>name']));
assertType('array<int, string>', $termQuery->query(['fields' => 'id=>slug']));

// Requesting IDs
assertType('list<int>', $termQuery->query(['fields' => 'ids']));
assertType('list<int>', $termQuery->query(['fields' => 'tt_ids']));

// Requesting parent IDs
assertType('array<int, int>', $termQuery->query(['fields' => 'id=>parent']));

// Requesting objects
assertType('array<int, WP_Term>', $termQuery->query(['fields' => 'all']));
assertType('array<int, WP_Term>', $termQuery->query(['fields' => 'all_with_object_id']));
assertType('array<int, WP_Term>', $termQuery->query(['fields' => 'foo']));

// Unknown fields value
assertType('0|numeric-string|list<string>|array<int, string>|array<int, int>|list<int>|array<int, WP_Term>', $termQuery->query(['fields' => Faker::string()])); // PHPantom keeps the array types apart where PHPStan merges them

// A URL query string is parsed at runtime, so the fields value is not visible
// to PHPStan and every result shape stays possible.
assertType('0|array<int, int|string|WP_Term>|numeric-string', $termQuery->query('fields=count'));
assertType('0|array<int, int|string|WP_Term>|numeric-string', $termQuery->query(Faker::string()));

// WP_Term_Query::get_terms() reads the query vars off the instance instead of
// taking them as an argument, so there is nothing to narrow the return type on.
// It still carries the int 0 that the count branch can return.
assertType('0|array<int, int|string|WP_Term>|numeric-string', $termQuery->get_terms());
