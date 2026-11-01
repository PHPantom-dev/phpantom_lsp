<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_category;
use function PHPStan\Testing\assertType;

assertType('WP_Term', get_category(Faker::object())); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('WP_Term', get_category(Faker::object(), 'OBJECT')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<string, mixed>', get_category(Faker::object(), 'ARRAY_A')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, mixed>', get_category(Faker::object(), 'ARRAY_N')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

$intOrObject = Faker::union(Faker::int(), Faker::object());
assertType('WP_Error|WP_Term|null', get_category($intOrObject)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('WP_Error|WP_Term|null', get_category($intOrObject, 'OBJECT')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<string, mixed>|WP_Error|null', get_category($intOrObject, 'ARRAY_A')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<int, mixed>|WP_Error|null', get_category($intOrObject, 'ARRAY_N')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
