<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_attachment_taxonomies;
use function PHPStan\Testing\assertType;

// Default
assertType('array<int, string>', get_attachment_taxonomies(Faker::int()));
assertType('array<int, string>', get_attachment_taxonomies(Faker::int(), 'names'));

// Objects
assertType('array<string, WP_Taxonomy>', get_attachment_taxonomies(Faker::int(), 'objects'));

// Unexpected
assertType('array<string, WP_Taxonomy>', get_attachment_taxonomies(Faker::int(), 'Hello'));

// Unknown
assertType('array<int, string>|array<string, WP_Taxonomy>', get_attachment_taxonomies(Faker::int(), Faker::string())); // PHPantom keeps the array types apart where PHPStan merges them

// Unions
assertType('array<int|string, string|WP_Taxonomy>', get_attachment_taxonomies(Faker::int(), Faker::union('names', 'objects'))); // SKIP: a variadic template parameter binds only the first argument
assertType('array<int, string>|array<string, WP_Taxonomy>', get_attachment_taxonomies(Faker::int(), Faker::union(Faker::string(), 'names'))); // PHPantom keeps the array types apart where PHPStan merges them
assertType('array<int, string>|array<string, WP_Taxonomy>', get_attachment_taxonomies(Faker::int(), Faker::union(Faker::string(), 'objects'))); // PHPantom keeps the array types apart where PHPStan merges them
