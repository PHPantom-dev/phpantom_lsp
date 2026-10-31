<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_object_terms;
use function PHPStan\Testing\assertType;

$objectIDs = 123;
$taxonomies = 'category';

// Default argument values (fields => all)
assertType('array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, [])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Empty $object_ids or $taxonomies
assertType('array{}', wp_get_object_terms(0, $taxonomies, ['fields' => 'count'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array{}', wp_get_object_terms([], $taxonomies)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array{}', wp_get_object_terms($objectIDs, '')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array{}', wp_get_object_terms($objectIDs, [])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting names or slugs
assertType('list<string>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'names'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<string>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'slugs'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, string>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'id=>name'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, string>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'id=>slug'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting IDs
assertType('list<int>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'ids'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<int>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'tt_ids'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting parent IDs
assertType('array<int, int>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'id=>parent'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting objects
assertType('array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'all'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'all_with_object_id'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'foo'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting a count
assertType('numeric-string|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'count'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Unknown fields value
assertType('array<int, int|string|WP_Term>|numeric-string|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => Faker::string()])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
