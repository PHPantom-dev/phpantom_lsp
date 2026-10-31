<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_posts;
use function PHPStan\Testing\assertType;

assertType('array<int, WP_Post>', get_posts()); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('array<int, WP_Post>', get_posts(['key' => 'value'])); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('array<int, WP_Post>', get_posts(['fields' => ''])); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('array<int, int>', get_posts(['fields' => 'ids'])); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('array<int, int>', get_posts(['fields' => 'id=>parent'])); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('array<int, WP_Post>', get_posts(['fields' => 'Hello'])); // SKIP: the last of two `@phpstan-return` tags is not the one read

// Nonconstant array
assertType('array<int, int|WP_Post>', get_posts(Faker::array())); // SKIP: the last of two `@phpstan-return` tags is not the one read

// Unions
$union = Faker::union(['key' => 'value'], ['some' => 'thing']);
assertType('array<int, WP_Post>', get_posts($union)); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(['key' => 'value'], ['fields' => 'ids']);
assertType('array<int, int|WP_Post>', get_posts($union)); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(['key' => 'value'], ['fields' => '']);
assertType('array<int, WP_Post>', get_posts($union)); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(['key' => 'value'], ['fields' => 'id=>parent']);
assertType('array<int, int|WP_Post>', get_posts($union)); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(['fields' => ''], ['fields' => 'ids']);
assertType('array<int, int|WP_Post>', get_posts($union)); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(['fields' => ''], ['fields' => 'id=>parent']);
assertType('array<int, int|WP_Post>', get_posts($union)); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(['fields' => 'ids'], ['fields' => 'id=>parent']);
assertType('array<int, int>', get_posts($union)); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(Faker::array(), ['fields' => '']);
assertType('array<int, int|WP_Post>', get_posts($union)); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(Faker::array(), ['fields' => 'ids']);
assertType('array<int, int|WP_Post>', get_posts($union)); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(Faker::array(), ['fields' => 'id=>parent']);
assertType('array<int, int|WP_Post>', get_posts($union)); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(Faker::string(), '');
assertType('array<int, int|WP_Post>', get_posts(['fields' => $union])); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(Faker::string(), 'ids');
assertType('array<int, int|WP_Post>', get_posts(['fields' => $union])); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(Faker::string(), 'id=>parent');
assertType('array<int, int|WP_Post>', get_posts(['fields' => $union])); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(Faker::string(), 'fields');
assertType('array<int, int|WP_Post>', get_posts([$union => ''])); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(Faker::string(), 'fields');
assertType('array<int, int|WP_Post>', get_posts([$union => 'ids'])); // SKIP: the last of two `@phpstan-return` tags is not the one read

$union = Faker::union(Faker::string(), 'fields');
assertType('array<int, int|WP_Post>', get_posts([$union => 'id=>parent'])); // SKIP: the last of two `@phpstan-return` tags is not the one read
