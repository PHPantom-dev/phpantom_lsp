<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_taxonomies_for_attachments;
use function PHPStan\Testing\assertType;

// Default
assertType('array<int, string>', get_taxonomies_for_attachments());
assertType('array<int, string>', get_taxonomies_for_attachments('names'));

// Objects
assertType('array<string, WP_Taxonomy>', get_taxonomies_for_attachments('objects'));

// Unexpected
assertType('array<string, WP_Taxonomy>', get_taxonomies_for_attachments('Hello'));

// Unknown
assertType('array<int, string>|array<string, WP_Taxonomy>', get_taxonomies_for_attachments(Faker::string())); // PHPantom keeps the array types apart where PHPStan merges them

// Unions
assertType('array<int|string, string|WP_Taxonomy>', get_taxonomies_for_attachments(Faker::union('names', 'objects'))); // SKIP: a variadic template parameter binds only the first argument
assertType('array<int, string>|array<string, WP_Taxonomy>', get_taxonomies_for_attachments(Faker::union(Faker::string(), 'names'))); // PHPantom keeps the array types apart where PHPStan merges them
assertType('array<int, string>|array<string, WP_Taxonomy>', get_taxonomies_for_attachments(Faker::union(Faker::string(), 'objects'))); // PHPantom keeps the array types apart where PHPStan merges them
