<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_is_numeric_array;
use function PHPStan\Testing\assertType;

$data = Faker::array();

if (wp_is_numeric_array($data) === false) {
    assertType('false', wp_is_numeric_array($data)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
}

if (wp_is_numeric_array($data) === true) {
    assertType('true', wp_is_numeric_array($data));
}
