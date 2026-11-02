<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_tags;
use function PHPStan\Testing\assertType;

// Default argument values (fields => all)
assertType('array<int, WP_Term>|WP_Error', get_tags());
assertType('array<int, WP_Term>|WP_Error', get_tags([]));

// Requesting a count
assertType('numeric-string|WP_Error', get_tags(['fields' => 'count']));
assertType('numeric-string|WP_Error', get_tags(['foo' => 'bar','fields' => 'count']));

// Requesting names or slugs
assertType('list<string>|WP_Error', get_tags(['fields' => 'names']));
assertType('list<string>|WP_Error', get_tags(['fields' => 'slugs']));
assertType('array<int, string>|WP_Error', get_tags(['fields' => 'id=>name']));
assertType('array<int, string>|WP_Error', get_tags(['fields' => 'id=>slug']));

// Requesting IDs
assertType('list<int>|WP_Error', get_tags(['fields' => 'ids']));
assertType('list<int>|WP_Error', get_tags(['fields' => 'tt_ids']));

// Requesting parent IDs
assertType('array<int, int>|WP_Error', get_tags(['fields' => 'id=>parent']));

// Requesting objects
assertType('array<int, WP_Term>|WP_Error', get_tags(['fields' => 'all']));
assertType('array<int, WP_Term>|WP_Error', get_tags(['fields' => 'all_with_object_id']));
assertType('array<int, WP_Term>|WP_Error', get_tags(['fields' => 'foo']));

// Unknown fields value
assertType('numeric-string|list<string>|array<int, string>|array<int, int>|list<int>|array<int, WP_Term>|WP_Error', get_tags(['fields' => Faker::string()])); // PHPantom keeps the array types apart where PHPStan merges them
