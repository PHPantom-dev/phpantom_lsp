<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_categories;
use function PHPStan\Testing\assertType;

// Default argument values (fields => all)
assertType('array<int, WP_Term>', get_categories());
assertType('array<int, WP_Term>', get_categories([]));

// Requesting a count
assertType('list<numeric-string>', get_categories(['fields' => 'count']));
assertType('list<numeric-string>', get_categories(['fields' => 'count', 'foo' => 'bar']));

// Requesting names or slugs
assertType('list<string>', get_categories(['fields' => 'names']));
assertType('list<string>', get_categories(['fields' => 'slugs']));
assertType('array<int, string>', get_categories(['fields' => 'id=>name']));
assertType('array<int, string>', get_categories(['fields' => 'id=>slug']));

// Requesting IDs
assertType('list<int>', get_categories(['fields' => 'ids']));
assertType('list<int>', get_categories(['fields' => 'tt_ids']));

// Requesting parent IDs
assertType('array<int, int>', get_categories(['fields' => 'id=>parent']));

// Requesting objects
assertType('array<int, WP_Term>', get_categories(['fields' => 'all']));
assertType('array<int, WP_Term>', get_categories(['fields' => 'all_with_object_id']));

// Unknown fields value
assertType('array<int, WP_Term>', get_categories(['fields' => 'foo']));
assertType('list<numeric-string>|list<string>|array<int, string>|array<int, int>|list<int>|array<int, WP_Term>', get_categories(['fields' => Faker::string()])); // PHPantom keeps the array types apart where PHPStan merges them
