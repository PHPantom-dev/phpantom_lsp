<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_post_tags;
use function PHPStan\Testing\assertType;

$postID = 123;

// Default argument values (fields => all)
assertType('array<int, WP_Term>|WP_Error', wp_get_post_tags($postID));
assertType('array<int, WP_Term>|WP_Error', wp_get_post_tags($postID, []));

// Empty $post_id
assertType('array{}', wp_get_post_tags(0));

// Requesting names or slugs
assertType('list<string>|WP_Error', wp_get_post_tags($postID, ['fields' => 'names']));
assertType('list<string>|WP_Error', wp_get_post_tags($postID, ['fields' => 'slugs']));
assertType('array<int, string>|WP_Error', wp_get_post_tags($postID, ['fields' => 'id=>name']));
assertType('array<int, string>|WP_Error', wp_get_post_tags($postID, ['fields' => 'id=>slug']));

// Requesting IDs
assertType('list<int>|WP_Error', wp_get_post_tags($postID, ['fields' => 'ids']));
assertType('list<int>|WP_Error', wp_get_post_tags($postID, ['fields' => 'tt_ids']));

// Requesting parent IDs
assertType('array<int, int>|WP_Error', wp_get_post_tags($postID, ['fields' => 'id=>parent']));

// Requesting objects
assertType('array<int, WP_Term>|WP_Error', wp_get_post_tags($postID, ['fields' => 'all']));
assertType('array<int, WP_Term>|WP_Error', wp_get_post_tags($postID, ['fields' => 'all_with_object_id']));
assertType('array<int, WP_Term>|WP_Error', wp_get_post_tags($postID, ['fields' => 'foo']));

// Requesting a count
assertType('numeric-string|WP_Error', wp_get_post_tags($postID, ['fields' => 'count']));

// Unknown fields value
assertType('list<string>|array<int, string>|array<int, int>|list<int>|numeric-string|array<int, WP_Term>|WP_Error', wp_get_post_tags($postID, ['fields' => Faker::string()])); // PHPantom keeps the array types apart where PHPStan merges them
