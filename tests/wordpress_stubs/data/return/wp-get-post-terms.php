<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_post_terms;
use function PHPStan\Testing\assertType;

$postID = 123;
$taxonomy = 'post_tag';

// Default argument values (fields => all)
assertType('array<int, WP_Term>|WP_Error', wp_get_post_terms($postID, $taxonomy));
assertType('array<int, WP_Term>|WP_Error', wp_get_post_terms($postID, $taxonomy, []));

// Empty $post_id or $taxonomy
assertType('array{}', wp_get_post_terms(0, $taxonomy));
assertType('array{}', wp_get_post_terms($postID, '')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('array{}', wp_get_post_terms($postID, [])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged

// Requesting names or slugs
assertType('list<string>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'names'])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('list<string>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'slugs'])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('array<int, string>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'id=>name']));
assertType('array<int, string>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'id=>slug']));

// Requesting IDs
assertType('list<int>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'ids'])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('list<int>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'tt_ids'])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged

// Requesting parent IDs
assertType('array<int, int>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'id=>parent']));

// Requesting objects
assertType('array<int, WP_Term>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'all']));
assertType('array<int, WP_Term>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'all_with_object_id']));
assertType('array<int, WP_Term>|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'foo']));

// Requesting a count
assertType('numeric-string|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => 'count'])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged

// Unknown fields value
assertType('array<int, int|string|WP_Term>|numeric-string|WP_Error', wp_get_post_terms($postID, $taxonomy, ['fields' => Faker::string()])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
