<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_post_types;
use function PHPStan\Testing\assertType;

// Default output
assertType('array<string, string>', get_post_types([])); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('array<string, string>', get_post_types([], 'names')); // SKIP: the last of two `@phpstan-return` tags is not the one read

// Objects output
assertType('array<string, WP_Post_Type>', get_post_types([], 'objects')); // SKIP: the last of two `@phpstan-return` tags is not the one read

// Unknown string
assertType('array<string, string|WP_Post_Type>', get_post_types([], Faker::string())); // SKIP: the last of two `@phpstan-return` tags is not the one read

// Unexpected output
assertType('array<string, WP_Post_Type>', get_post_types([], 'Hello')); // SKIP: the last of two `@phpstan-return` tags is not the one read
