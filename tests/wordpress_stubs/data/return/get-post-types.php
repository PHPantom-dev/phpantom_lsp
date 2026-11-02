<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_post_types;
use function PHPStan\Testing\assertType;

// Default output
assertType('array<string, string>', get_post_types([]));
assertType('array<string, string>', get_post_types([], 'names'));

// Objects output
assertType('array<string, WP_Post_Type>', get_post_types([], 'objects'));

// Unknown string
assertType('array<string, string>|array<string, WP_Post_Type>', get_post_types([], Faker::string())); // PHPantom keeps the array types apart where PHPStan merges them

// Unexpected output
assertType('array<string, WP_Post_Type>', get_post_types([], 'Hello'));
