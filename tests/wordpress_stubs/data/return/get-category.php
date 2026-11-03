<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_category;
use function PHPStan\Testing\assertType;

assertType('WP_Term', get_category(Faker::object()));
assertType('WP_Term', get_category(Faker::object(), 'OBJECT'));
assertType('array<string, mixed>', get_category(Faker::object(), 'ARRAY_A'));
assertType('array<int, mixed>', get_category(Faker::object(), 'ARRAY_N'));

$intOrObject = Faker::union(Faker::int(), Faker::object());
assertType('WP_Error|WP_Term|null', get_category($intOrObject));
assertType('WP_Error|WP_Term|null', get_category($intOrObject, 'OBJECT'));
assertType('array<string, mixed>|WP_Error|null', get_category($intOrObject, 'ARRAY_A'));
assertType('array<int, mixed>|WP_Error|null', get_category($intOrObject, 'ARRAY_N'));
