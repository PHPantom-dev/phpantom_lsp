<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_terms;
use function PHPStan\Testing\assertType;

// Default argument values (fields => all)
assertType('array<int, WP_Term>|WP_Error', get_terms()); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', get_terms([])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting a count. WP_Term_Query::get_terms() returns int 0 rather than a
// numeric string when the queried parent term has no descendants.
assertType('0|numeric-string|WP_Error', get_terms(['fields' => 'count'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('0|numeric-string|WP_Error', get_terms(['foo' => 'bar','fields' => 'count'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting names or slugs
assertType('list<string>|WP_Error', get_terms(['fields' => 'names'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<string>|WP_Error', get_terms(['fields' => 'slugs'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, string>|WP_Error', get_terms(['fields' => 'id=>name'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, string>|WP_Error', get_terms(['fields' => 'id=>slug'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting IDs
assertType('list<int>|WP_Error', get_terms(['fields' => 'ids'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<int>|WP_Error', get_terms(['fields' => 'tt_ids'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting parent IDs
assertType('array<int, int>|WP_Error', get_terms(['fields' => 'id=>parent'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting objects
assertType('array<int, WP_Term>|WP_Error', get_terms(['fields' => 'all'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', get_terms(['fields' => 'all_with_object_id'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', get_terms(['fields' => 'foo'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Unknown fields value
assertType('0|array<int, int|string|WP_Term>|numeric-string|WP_Error', get_terms(['fields' => Faker::string()])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
