<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_post_stati;
use function PHPStan\Testing\assertType;

// Default
assertType('array<string, string>', get_post_stati());
assertType('array<string, string>', get_post_stati([]));
assertType('array<string, string>', get_post_stati([], 'names'));

// Objects
assertType('array<string, stdClass>', get_post_stati([], 'objects'));

// Unexpected
assertType('array<string, stdClass>', get_post_stati([], 'Hello'));

// Unknown
assertType('array<string, string>|array<string, stdClass>', get_post_stati([], Faker::string())); // PHPantom keeps the array types apart where PHPStan merges them

// Unions
assertType('array<string, stdClass|string>', get_post_stati([], Faker::union('names', 'objects'))); // SKIP: a variadic template parameter binds only the first argument
assertType('array<string, string>|array<string, stdClass>', get_post_stati([], Faker::union(Faker::string(), 'names'))); // PHPantom keeps the array types apart where PHPStan merges them
assertType('array<string, string>|array<string, stdClass>', get_post_stati([], Faker::union(Faker::string(), 'objects'))); // PHPantom keeps the array types apart where PHPStan merges them
