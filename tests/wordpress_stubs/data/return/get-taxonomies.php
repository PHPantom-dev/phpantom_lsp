<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_taxonomies;
use function PHPStan\Testing\assertType;

// Default output
assertType('array<string, string>', get_taxonomies([]));
assertType('array<string, string>', get_taxonomies([], 'names'));

// Objects output
assertType('array<string, WP_Taxonomy>', get_taxonomies([], 'objects'));

// Unexpected output
assertType('array<string, WP_Taxonomy>', get_taxonomies([], 'Hello'));

// Unknown string output
assertType('array<string, string>|array<string, WP_Taxonomy>', get_taxonomies([], Faker::string())); // PHPantom keeps the array types apart where PHPStan merges them
