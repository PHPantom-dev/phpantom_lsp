<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_is_numeric_array;
use function PHPStan\Testing\assertType;

// No array
assertType('false', wp_is_numeric_array(Faker::string()));
assertType('false', wp_is_numeric_array(Faker::int()));
assertType('false', wp_is_numeric_array(Faker::bool()));
assertType('false', wp_is_numeric_array(null));

// Arrays
assertType('true', wp_is_numeric_array([]));
assertType('true', wp_is_numeric_array(Faker::list()));
assertType('true', wp_is_numeric_array(Faker::intArray()));
assertType('false', wp_is_numeric_array(Faker::strArray()));
assertType('bool', wp_is_numeric_array(Faker::array()));
assertType('bool', wp_is_numeric_array(Faker::union(Faker::strArray(), Faker::intArray())));

// Constant arrays
assertType('false', wp_is_numeric_array(['key1' => 'value1', 'key2' => 'value2']));
assertType('true', wp_is_numeric_array(['value0', 'value1']));
assertType('bool', wp_is_numeric_array(['value0', 'key1' => 'value1']));

// Maybe array
assertType('bool', wp_is_numeric_array(Faker::mixed()));
