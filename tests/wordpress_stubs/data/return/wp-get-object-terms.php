<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_object_terms;
use function PHPStan\Testing\assertType;

$objectIDs = 123;
$taxonomies = 'category';

// Default argument values (fields => all)
assertType('array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies));
assertType('array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, []));

// Empty $object_ids or $taxonomies
assertType('array{}', wp_get_object_terms(0, $taxonomies, ['fields' => 'count']));
assertType('array{}', wp_get_object_terms([], $taxonomies));
assertType('array{}', wp_get_object_terms($objectIDs, ''));
assertType('array{}', wp_get_object_terms($objectIDs, []));

// Requesting names or slugs
assertType('list<string>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'names']));
assertType('list<string>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'slugs']));
assertType('array<int, string>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'id=>name']));
assertType('array<int, string>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'id=>slug']));

// Requesting IDs
assertType('list<int>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'ids']));
assertType('list<int>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'tt_ids']));

// Requesting parent IDs
assertType('array<int, int>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'id=>parent']));

// Requesting objects
assertType('array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'all']));
assertType('array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'all_with_object_id']));
assertType('array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'foo']));

// Requesting a count
assertType('numeric-string|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => 'count']));

// Unknown fields value
assertType('list<string>|array<int, string>|array<int, int>|list<int>|numeric-string|array<int, WP_Term>|WP_Error', wp_get_object_terms($objectIDs, $taxonomies, ['fields' => Faker::string()])); // PHPantom keeps the array types apart where PHPStan merges them
