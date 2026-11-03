<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_is_numeric_array;
use function PHPStan\Testing\assertType;

$data = Faker::array();

if (wp_is_numeric_array($data) === false) {
    assertType('false', wp_is_numeric_array($data));
}

if (wp_is_numeric_array($data) === true) {
    assertType('true', wp_is_numeric_array($data));
}
