<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_post_tags;
use function PHPStan\Testing\assertType;

$postID = 123;

// Default argument values (fields => all)
assertType('array<int, WP_Term>|WP_Error', wp_get_post_tags($postID)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', wp_get_post_tags($postID, [])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Empty $post_id
assertType('array{}', wp_get_post_tags(0));

// Requesting names or slugs
assertType('list<string>|WP_Error', wp_get_post_tags($postID, ['fields' => 'names'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<string>|WP_Error', wp_get_post_tags($postID, ['fields' => 'slugs'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, string>|WP_Error', wp_get_post_tags($postID, ['fields' => 'id=>name'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, string>|WP_Error', wp_get_post_tags($postID, ['fields' => 'id=>slug'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting IDs
assertType('list<int>|WP_Error', wp_get_post_tags($postID, ['fields' => 'ids'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<int>|WP_Error', wp_get_post_tags($postID, ['fields' => 'tt_ids'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting parent IDs
assertType('array<int, int>|WP_Error', wp_get_post_tags($postID, ['fields' => 'id=>parent'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting objects
assertType('array<int, WP_Term>|WP_Error', wp_get_post_tags($postID, ['fields' => 'all'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', wp_get_post_tags($postID, ['fields' => 'all_with_object_id'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', wp_get_post_tags($postID, ['fields' => 'foo'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting a count
assertType('numeric-string|WP_Error', wp_get_post_tags($postID, ['fields' => 'count'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Unknown fields value
assertType('array<int, int|string|WP_Term>|numeric-string|WP_Error', wp_get_post_tags($postID, ['fields' => Faker::string()])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
