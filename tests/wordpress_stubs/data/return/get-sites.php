<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_sites;
use function PHPStan\Testing\assertType;

// Default parameter
assertType('array<int, WP_Site>', get_sites()); // SKIP: an array-shape condition matches any array
assertType('array<int, WP_Site>', get_sites([])); // SKIP: an array-shape condition matches any array

// Non constant array parameter.
assertType('int|array<int, int>|array<int, WP_Site>', get_sites(Faker::array())); // PHPantom keeps the array types apart where PHPStan merges them

// Array parameter with explicit fields value and default count value.
assertType('array<int, int>', get_sites(['fields' => 'ids'])); // SKIP: an array-shape condition matches any array
assertType('array<int, WP_Site>', get_sites(['fields' => ''])); // SKIP: an array-shape condition matches any array
assertType('array<int, WP_Site>', get_sites(['fields' => 'nonEmptyString'])); // SKIP: an array-shape condition matches any array

// Array parameter with count set to true.
assertType('int', get_sites(['count' => true]));
assertType('int', get_sites(['fields' => '', 'count' => true]));
assertType('int', get_sites(['fields' => 'ids', 'count' => true]));
assertType('int', get_sites(['fields' => 'nonEmptyString', 'count' => true]));

// Array parameter with count set to false.
assertType('array<int, WP_Site>', get_sites(['count' => false])); // SKIP: an array-shape condition matches any array
assertType('array<int, WP_Site>', get_sites(['fields' => '', 'count' => false])); // SKIP: an array-shape condition matches any array
assertType('array<int, int>', get_sites(['fields' => 'ids', 'count' => false])); // SKIP: an array-shape condition matches any array
assertType('array<int, WP_Site>', get_sites(['fields' => 'nonEmptyString', 'count' => false])); // SKIP: an array-shape condition matches any array
