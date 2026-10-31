<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_object_taxonomies;
use function PHPStan\Testing\assertType;

// Default output
assertType('array<int, string>', get_object_taxonomies('post'));
assertType('array<int, string>', get_object_taxonomies('post', 'names'));

// Objects output
assertType('array<string, WP_Taxonomy>', get_object_taxonomies('post', 'objects'));

// Unexpected output
assertType('array<string, WP_Taxonomy>', get_object_taxonomies('post', 'Hello'));

// Unknown string
assertType('array<int, string>|array<string, WP_Taxonomy>', get_object_taxonomies('post', Faker::string())); // PHPantom keeps the array types apart where PHPStan merges them

// Unions
assertType('array<int|string, string|WP_Taxonomy>', get_object_taxonomies('post', Faker::union('names', 'objects'))); // SKIP: a variadic template parameter binds only the first argument
assertType('array<int, string>|array<string, WP_Taxonomy>', get_object_taxonomies('post', Faker::union(Faker::string(), 'names'))); // PHPantom keeps the array types apart where PHPStan merges them
assertType('array<int, string>|array<string, WP_Taxonomy>', get_object_taxonomies('post', Faker::union(Faker::string(), 'objects'))); // PHPantom keeps the array types apart where PHPStan merges them
