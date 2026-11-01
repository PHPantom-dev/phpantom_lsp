<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_category;
use function PHPStan\Testing\assertType;

assertType('WP_Term', get_category(Faker::object())); // SKIP: an intersection of unions is not reduced to the members they share
assertType('WP_Term', get_category(Faker::object(), 'OBJECT')); // SKIP: an intersection of unions is not reduced to the members they share
assertType('array<string, mixed>', get_category(Faker::object(), 'ARRAY_A')); // SKIP: an intersection of unions is not reduced to the members they share
assertType('array<int, mixed>', get_category(Faker::object(), 'ARRAY_N')); // SKIP: an intersection of unions is not reduced to the members they share

$intOrObject = Faker::union(Faker::int(), Faker::object());
assertType('WP_Error|WP_Term|null', get_category($intOrObject)); // SKIP: an intersection of unions is not reduced to the members they share
assertType('WP_Error|WP_Term|null', get_category($intOrObject, 'OBJECT')); // SKIP: an intersection of unions is not reduced to the members they share
assertType('array<string, mixed>|WP_Error|null', get_category($intOrObject, 'ARRAY_A')); // SKIP: an intersection of unions is not reduced to the members they share
assertType('array<int, mixed>|WP_Error|null', get_category($intOrObject, 'ARRAY_N')); // SKIP: an intersection of unions is not reduced to the members they share
