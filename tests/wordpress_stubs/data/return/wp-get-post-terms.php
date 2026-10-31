<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_post_terms;
use function PHPStan\Testing\assertType;

$postID = 123;
$taxonomy = 'post_tag';

// Default argument values (fields => all)
assertType('array<int, WP_Term>|WP_Error', wp_get_post_terms($postID, $taxonomy)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', wp_get_post_terms($postID, $taxonomy, [])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Empty $post_id or $taxonomy
assertType('array{}', wp_get_post_terms(0, $taxonomy));
assertType('array{}', wp_get_post_terms($postID, '')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array{}', wp_get_post_terms($postID, [])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting names or slugs
assertType('list<string>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'names'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<string>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'slugs'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, string>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'id=>name'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, string>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'id=>slug'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting IDs
assertType('list<int>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'ids'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<int>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'tt_ids'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting parent IDs
assertType('array<int, int>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'id=>parent'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting objects
assertType('array<int, WP_Term>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'all'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'all_with_object_id'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, WP_Term>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'foo'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Requesting a count
assertType('numeric-string|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'count'])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// Unknown fields value
assertType('array<int, int|string|WP_Term>|numeric-string|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => Faker::string()])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
