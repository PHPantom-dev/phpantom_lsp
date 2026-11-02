<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_bookmark;
use function get_bookmark_field;
use function sanitize_bookmark_field;
use function PHPStan\Testing\assertType;

$stdClassOrInt = Faker::union(Faker::stdClass(), Faker::int());

/*
 * get_bookmark()
 */

assertType('stdClass|null', get_bookmark($stdClassOrInt));
assertType('stdClass|null', get_bookmark($stdClassOrInt, 'OBJECT'));
assertType('array<string, mixed>|null', get_bookmark($stdClassOrInt, 'ARRAY_A'));
assertType('array<int, mixed>|null', get_bookmark($stdClassOrInt, 'ARRAY_N'));

/*
 * get_bookmark_field()
 */

assertType('array<int, int<1, max>>|int|string|WP_Error', get_bookmark_field('link_id', Faker::int()));
assertType('array<int, int<1, max>>|int|string|WP_Error', get_bookmark_field('foo', Faker::int()));
assertType('array<int, int<1, max>>|int|string|WP_Error', get_bookmark_field(Faker::string(), Faker::int()));

/*
 * sanitize_bookmark_field()
 */

assertType('array<int, int>|int|string', sanitize_bookmark_field('link_id', Faker::mixed(), Faker::int(), 'raw'));
assertType('array<int, int>|int|string', sanitize_bookmark_field('foo', Faker::mixed(), Faker::int(), 'raw'));
assertType('array<int, int>|int|string', sanitize_bookmark_field(Faker::string(), Faker::mixed(), Faker::int(), 'raw'));
