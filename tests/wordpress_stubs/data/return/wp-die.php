<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_die;
use function PHPStan\Testing\assertType;

// default (['exit' => true])
assertType('never', wp_die()); // SKIP: an array-shape condition matches any array
assertType('never', wp_die('')); // SKIP: an array-shape condition matches any array
assertType('never', wp_die('', '')); // SKIP: an array-shape condition matches any array

// explicit exit
assertType('never', wp_die('', '', ['exit' => true])); // SKIP: an array-shape condition matches any array
assertType('null', wp_die('', '', ['exit' => false])); // SKIP: `void` from a conditional return type is not read as `null`

// unknown
assertType('null', wp_die('', '', ['exit' => Faker::bool()])); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', wp_die('', '', Faker::array())); // SKIP: an array-shape condition matches any array

// non-array $args parameter ($args not string type per @phpstan-param)
assertType('never', wp_die('', '', Faker::int()));
