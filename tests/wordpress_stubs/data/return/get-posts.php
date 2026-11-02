<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_posts;
use function PHPStan\Testing\assertType;

assertType('array<int, WP_Post>', get_posts());
assertType('array<int, WP_Post>', get_posts(['key' => 'value']));
assertType('array<int, WP_Post>', get_posts(['fields' => '']));
assertType('array<int, int>', get_posts(['fields' => 'ids']));
assertType('array<int, int>', get_posts(['fields' => 'id=>parent']));
assertType('array<int, WP_Post>', get_posts(['fields' => 'Hello']));

// Nonconstant array
assertType('array<int, int>|array<int, WP_Post>', get_posts(Faker::array())); // PHPantom keeps the array types apart where PHPStan merges them

// Unions
$union = Faker::union(['key' => 'value'], ['some' => 'thing']);
assertType('array<int, WP_Post>', get_posts($union));

$union = Faker::union(['key' => 'value'], ['fields' => 'ids']);
assertType('array<int, int>|array<int, WP_Post>', get_posts($union)); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(['key' => 'value'], ['fields' => '']);
assertType('array<int, WP_Post>', get_posts($union));

$union = Faker::union(['key' => 'value'], ['fields' => 'id=>parent']);
assertType('array<int, int>|array<int, WP_Post>', get_posts($union)); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(['fields' => ''], ['fields' => 'ids']);
assertType('array<int, int>|array<int, WP_Post>', get_posts($union)); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(['fields' => ''], ['fields' => 'id=>parent']);
assertType('array<int, int>|array<int, WP_Post>', get_posts($union)); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(['fields' => 'ids'], ['fields' => 'id=>parent']);
assertType('array<int, int>', get_posts($union));

$union = Faker::union(Faker::array(), ['fields' => '']);
assertType('array<int, int>|array<int, WP_Post>', get_posts($union)); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(Faker::array(), ['fields' => 'ids']);
assertType('array<int, int>|array<int, WP_Post>', get_posts($union)); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(Faker::array(), ['fields' => 'id=>parent']);
assertType('array<int, int>|array<int, WP_Post>', get_posts($union)); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(Faker::string(), '');
assertType('array<int, int>|array<int, WP_Post>', get_posts(['fields' => $union])); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(Faker::string(), 'ids');
assertType('array<int, int>|array<int, WP_Post>', get_posts(['fields' => $union])); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(Faker::string(), 'id=>parent');
assertType('array<int, int>|array<int, WP_Post>', get_posts(['fields' => $union])); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(Faker::string(), 'fields');
assertType('array<int, int>|array<int, WP_Post>', get_posts([$union => ''])); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(Faker::string(), 'fields');
assertType('array<int, int>|array<int, WP_Post>', get_posts([$union => 'ids'])); // PHPantom keeps the array types apart where PHPStan merges them

$union = Faker::union(Faker::string(), 'fields');
assertType('array<int, int>|array<int, WP_Post>', get_posts([$union => 'id=>parent'])); // PHPantom keeps the array types apart where PHPStan merges them
