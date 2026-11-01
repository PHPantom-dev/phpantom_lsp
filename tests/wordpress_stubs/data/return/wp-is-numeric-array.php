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
assertType('false', wp_is_numeric_array(Faker::strArray())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('bool', wp_is_numeric_array(Faker::array())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('bool', wp_is_numeric_array(Faker::union(Faker::strArray(), Faker::intArray()))); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged

// Constant arrays
assertType('false', wp_is_numeric_array(['key1' => 'value1', 'key2' => 'value2'])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('true', wp_is_numeric_array(['value0', 'value1']));
assertType('bool', wp_is_numeric_array(['value0', 'key1' => 'value1'])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged

// Maybe array
assertType('bool', wp_is_numeric_array(Faker::mixed()));
