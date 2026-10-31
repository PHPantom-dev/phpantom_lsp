<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_categories;
use function PHPStan\Testing\assertType;

// Default argument values (fields => all)
assertType('array<int, WP_Term>', get_categories());
assertType('array<int, WP_Term>', get_categories([])); // SKIP: an array-shape condition matches any array

// Requesting a count
assertType('list<numeric-string>', get_categories(['fields' => 'count']));
assertType('list<numeric-string>', get_categories(['fields' => 'count', 'foo' => 'bar']));

// Requesting names or slugs
assertType('list<string>', get_categories(['fields' => 'names'])); // SKIP: an array-shape condition matches any array
assertType('list<string>', get_categories(['fields' => 'slugs'])); // SKIP: an array-shape condition matches any array
assertType('array<int, string>', get_categories(['fields' => 'id=>name'])); // SKIP: an array-shape condition matches any array
assertType('array<int, string>', get_categories(['fields' => 'id=>slug'])); // SKIP: an array-shape condition matches any array

// Requesting IDs
assertType('list<int>', get_categories(['fields' => 'ids'])); // SKIP: an array-shape condition matches any array
assertType('list<int>', get_categories(['fields' => 'tt_ids'])); // SKIP: an array-shape condition matches any array

// Requesting parent IDs
assertType('array<int, int>', get_categories(['fields' => 'id=>parent'])); // SKIP: an array-shape condition matches any array

// Requesting objects
assertType('array<int, WP_Term>', get_categories(['fields' => 'all'])); // SKIP: an array-shape condition matches any array
assertType('array<int, WP_Term>', get_categories(['fields' => 'all_with_object_id'])); // SKIP: an array-shape condition matches any array

// Unknown fields value
assertType('array<int, WP_Term>', get_categories(['fields' => 'foo'])); // SKIP: an array-shape condition matches any array
assertType('array<int, int|string|WP_Term>', get_categories(['fields' => Faker::string()])); // SKIP: an array-shape condition matches any array
