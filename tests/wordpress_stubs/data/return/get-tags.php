<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_tags;
use function PHPStan\Testing\assertType;

// Default argument values (fields => all)
assertType('array<int, WP_Term>|WP_Error', get_tags()); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', get_tags([])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting a count
assertType('numeric-string|WP_Error', get_tags(['fields' => 'count'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('numeric-string|WP_Error', get_tags(['foo' => 'bar','fields' => 'count'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting names or slugs
assertType('list<string>|WP_Error', get_tags(['fields' => 'names'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<string>|WP_Error', get_tags(['fields' => 'slugs'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, string>|WP_Error', get_tags(['fields' => 'id=>name'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, string>|WP_Error', get_tags(['fields' => 'id=>slug'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting IDs
assertType('list<int>|WP_Error', get_tags(['fields' => 'ids'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<int>|WP_Error', get_tags(['fields' => 'tt_ids'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting parent IDs
assertType('array<int, int>|WP_Error', get_tags(['fields' => 'id=>parent'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting objects
assertType('array<int, WP_Term>|WP_Error', get_tags(['fields' => 'all'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', get_tags(['fields' => 'all_with_object_id'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', get_tags(['fields' => 'foo'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Unknown fields value
assertType('array<int, int|string|WP_Term>|numeric-string|WP_Error', get_tags(['fields' => Faker::string()])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
